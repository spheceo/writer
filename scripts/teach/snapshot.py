#!/usr/bin/env python3
"""A script's saved commentary draft as a round result (the "before" the
review was on), rendered as the loop's script.txt, for score.py.

  python3 scripts/teach/snapshot.py <loop dir> <round> <name> <script id> [draft.json]
"""
import json, sys
from pathlib import Path

LOOP, ROUND, NAME, ID = Path(sys.argv[1]).expanduser(), sys.argv[2], sys.argv[3], sys.argv[4]
scripts = Path.home() / "Library/Application Support/com.spheceo.writer/scripts"
draft = json.loads(Path(sys.argv[5]).read_text()) if len(sys.argv) > 5 else json.loads((scripts / ID / "script.json").read_text())["commentary"]["draft"]


def clock(ms):
    s = int(ms // 1000)
    return f"{s // 3600}:{s // 60 % 60:02}:{s % 60:02}" if s >= 3600 else f"{s // 60}:{s % 60:02}"


def cite(c):
    quote = f': "{c["quote"].strip()}"' if c.get("quote", "").strip() else ""
    return f"[CLIP {c['video']} {clock(c['startMs'])}-{clock(c['endMs'])}{quote}]"


lines = [f"[Montage clip {i + 1}] {cite(c)}" for i, c in enumerate(draft.get("open", []))]
for section in draft["sections"]:
    lines.append(f"[{section['heading']}] HEADING: {section['heading']}")
    for i, p in enumerate(section["paragraphs"]):
        text = " ".join(piece["text"].strip() if piece.get("kind", "say") == "say" and "text" in piece else cite(piece) for piece in p["pieces"])
        if p.get("note"):
            text += f" [NOTE FOR EDITOR: {p['note'].strip()}]"
        lines.append(f"[{section['heading']} ¶{i + 1}] {text}")
lines += [f"[Check {i + 1}] CHECK: {c}" for i, c in enumerate(draft.get("checks", []))]
out = LOOP / "rounds" / ROUND / NAME
out.mkdir(parents=True, exist_ok=True)
(out / "result.json").write_text(json.dumps({"id": ID, "mode": "saved", "draft": draft, "text": "\n".join(lines), "report": draft.get("report", ""), "costUsd": 0, "seconds": 0}, indent=1, ensure_ascii=False))
(out / "script.txt").write_text("\n".join(lines))
print(f"{len(lines)} lines → {out}")
