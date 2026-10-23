#!/usr/bin/env python3
"""Scores one round's script against the loop's private answer key: the
counts the key's A rows name, measured from the draft, and every row judged
ok / half / miss by Opus (claude -p) on the script. A script the review was
on is also scored item by item: would the reviewer leave that note again?

  python3 scripts/teach/score.py <loop dir> <round> <name> <key.md> [--items items.json] [--scope all|here]

`--scope all` (a control or holdout) judges only the rows that hold for
every commentary, and says the reviewer's examples come from another script.
Writes score.json and score.txt beside the round's result.json.
"""
import json, re, subprocess, sys
from pathlib import Path

LOOP, ROUND, NAME, KEY = Path(sys.argv[1]).expanduser(), sys.argv[2], sys.argv[3], Path(sys.argv[4]).expanduser()
flag = lambda name, default=None: sys.argv[sys.argv.index(name) + 1] if name in sys.argv else default
ITEMS, SCOPE = flag("--items"), flag("--scope", "here")
folder = LOOP / "rounds" / ROUND / NAME
result = json.loads((folder / "result.json").read_text())
if "draft" not in result:
    sys.exit(f"{NAME} has no draft: {result.get('error', '')[:300]}")
draft, text = result["draft"], result["text"]

# ---- A: counts from the draft ----
paragraphs = [p for section in draft["sections"] for p in section["paragraphs"]]
say = lambda p: " ".join(piece.get("text", "") for piece in p["pieces"] if piece.get("kind", "say") == "say" and "text" in piece)
narration = "\n".join(say(p) for p in paragraphs)
words = len(narration.split())
sentences = re.split(r"(?<=[.!?…])\s+", narration)
creator = re.findall(r"\b[Aa]ccording to ((?:[A-Z][\w'’]+)(?: [A-Z][\w'’]+)*)", narration) + re.findall(r"\b((?:[A-Z][\w'’]+)(?: [A-Z][\w'’]+)*), who (?:went through|looked through|dug|combed)", narration)
ends_on_clip = sum(1 for p in paragraphs if p["pieces"] and "video" in p["pieces"][-1])
notes = [p.get("note") or "" for p in paragraphs if p.get("note")]
quote_notes = [n for n in notes if re.search(r"quote", n, re.I) and re.search(r"screen|pop|typ", n, re.I)]
intro = next((s for s in draft["sections"] if s["heading"].lower().startswith("intro")), draft["sections"][0])
intro_text = " ".join(say(p) for p in intro["paragraphs"])
counts = {
    "words": words,
    "paragraphs": len(paragraphs),
    "montageClips": len(draft.get("open", [])),
    "creatorCredits": creator,
    "memoryCues": re.findall(r"[^.]*\b(?:Remember (?:that|this)|Keep that in mind)\b[^.]*\.", narration),
    "thenOpeners": sum(1 for s in sentences if re.match(r"(?:And then|Then|After that|Next)\b", s.strip())),
    "endsOnClipShare": round(ends_on_clip / max(1, len(paragraphs)), 2),
    "editorNotes": len(notes),
    "quotePopupNotes": len(quote_notes),
    "introWords": len(intro_text.split()),
    "introParagraphs": len(intro["paragraphs"]),
    "introEndsOnQuestions": intro_text.rstrip().count("?") >= 2 and "?" in intro_text[-300:],
}

# ---- J: every row judged on the script ----
key = KEY.read_text()
rows = [line for line in key.splitlines() if re.match(r"\| K\d+ ", line)]
if SCOPE == "all":
    rows = [row for row in rows if row.rstrip(" |").split("|")[-1].strip().startswith("all")]
row_ids = [row.split("|")[1].strip() for row in rows]
items = json.loads(Path(ITEMS).expanduser().read_text())["items"] if ITEMS else []
review = "\n\n".join(f"{i['id']} ({i['kind']}) on \"{i['quote'][:400]}\": {i['text'] or 'replace with: ' + i.get('replacement', '')[:2500]}" for i in items)

system = """You judge a script written by an AI script writer for the Internet Janitor, a commentary channel (investigative video essays read by one narrator, with clips backing each point). A reviewer's feedback on an earlier script was turned into an answer key: rows, each a pattern the reviewer wants, with their words and how it's checked. Judge the new script on each row:
- ok: the script does what the row asks throughout; the reviewer would leave no note of this kind.
- half: mostly, with one or two places the reviewer would still flag, or the fix is weaker than asked.
- miss: the reviewer would leave this kind of note again.
Quote the script's words as evidence (short), and say in one or two sentences why. Judge only what the row asks. The writer never saw the feedback or the key."""
if SCOPE == "all":
    system += "\nThis script is about a different subject from the one reviewed: judge the general pattern, not the reviewer's specific facts or names."
prompt = f"THE ANSWER KEY\n\n" + "\n".join(rows) + f"\n\nMEASURED FROM THE SCRIPT\n{json.dumps(counts, ensure_ascii=False)}\n\nTHE SCRIPT\n\n{text}"
schema = {
    "type": "object", "additionalProperties": False,
    "properties": {
        "rows": {"type": "array", "items": {"type": "object", "additionalProperties": False, "properties": {
            "row": {"type": "string"}, "grade": {"type": "string", "enum": ["ok", "half", "miss"]},
            "evidence": {"type": "string"}, "why": {"type": "string"}}, "required": ["row", "grade", "evidence", "why"]}},
        "items": {"type": "array", "items": {"type": "object", "additionalProperties": False, "properties": {
            "item": {"type": "string"}, "again": {"type": "string", "enum": ["no", "partly", "yes"]}, "why": {"type": "string"}}, "required": ["item", "again", "why"]}},
        "summary": {"type": "string"},
    },
    "required": ["rows", "items", "summary"],
}
if items:
    prompt += f"\n\nTHE REVIEWER'S NOTES ON THE EARLIER SCRIPT (F1…)\n\n{review}\n\nAlso, for every note F1…: would the reviewer leave that note (or one like it) again on THIS script? no / partly / yes. A note asking for something (a clip, b-roll, a quote on screen) is 'no' only if this script asks for it or does it."
else:
    prompt += "\n\nLeave items empty."
run = subprocess.run(
    ["claude", "-p", "--model", "opus", "--safe-mode", "--strict-mcp-config", "--tools", "", "--permission-prompts", "none",
     "--system-prompt", system, "--json-schema", json.dumps(schema), "--output-format", "json"],
    input=prompt, capture_output=True, text=True, timeout=3600,
)
answer = json.loads(run.stdout or "{}")
judged = answer.get("structured_output") or {}
grades = {r["row"]: r for r in judged.get("rows", []) if r.get("row") in row_ids}
points = {"ok": 1, "half": 0.5, "miss": 0}
row_score = sum(points[grades[r]["grade"]] for r in row_ids if r in grades) / max(1, len(row_ids))
again = judged.get("items", [])
item_score = sum({"no": 1, "partly": 0.5, "yes": 0}[i["again"]] for i in again) / max(1, len(again)) if again else None
score = {
    "round": ROUND, "name": NAME, "scope": SCOPE, "counts": counts,
    "rows": [grades.get(r, {"row": r, "grade": "miss", "evidence": "", "why": "not judged"}) for r in row_ids],
    "rowScore": round(row_score, 3), "items": again, "itemScore": None if item_score is None else round(item_score, 3),
    "summary": judged.get("summary", ""), "graderCostUsd": answer.get("total_cost_usd"), "buildCostUsd": result.get("costUsd"), "buildSeconds": result.get("seconds"),
}
(folder / "score.json").write_text(json.dumps(score, indent=1, ensure_ascii=False))
lines = [f"{ROUND}/{NAME} ({SCOPE}): rows {round(row_score * 100)}%" + ("" if item_score is None else f", reviewer's notes not repeated {round(item_score * 100)}%")]
lines.append("counts: " + json.dumps({k: (len(v) if isinstance(v, list) else v) for k, v in counts.items()}))
for r in score["rows"]:
    lines.append(f"  {r['row']} {r['grade']:4} {r['why']}")
for i in again:
    if i["again"] != "no":
        lines.append(f"  {i['item']} again:{i['again']} {i['why']}")
lines.append(score["summary"])
(folder / "score.txt").write_text("\n".join(lines) + "\n")
print("\n".join(lines))
