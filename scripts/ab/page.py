#!/usr/bin/env python3
"""The blind comparison page from run.py's results: each script's two builds
side by side as the Doc reads (cut-ins in full, each clip's words with its
times), labelled A and B in a random order per pair, with the numbers under
them and the key behind a "Reveal" button.

  python3 scripts/ab/page.py <results dir> <page.html>
"""
import html, json, random, re, sys
from pathlib import Path

RESULTS = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/writer-ab")
PAGE = Path(sys.argv[2] if len(sys.argv) > 2 else RESULTS / "index.html")
SCRIPTS = Path.home() / "Library/Application Support/com.spheceo.writer/scripts"
TELLS = {"Repairs", "Biggest single reply (output tokens, about)"}
ENGINE = {"cli": "Old: one answer (claude -p)", "sdk": "New: Agent SDK, in steps"}


def clock(ms):
    s = int(ms // 1000)
    return f"{s // 3600}:{s // 60 % 60:02}:{s % 60:02}" if s >= 3600 else f"{s // 60}:{s % 60:02}"


def words_in(words, start, end):
    return [w for w in words if w["startMs"] >= start - 50 and w["endMs"] <= end + 50]


def measure(result, script):
    words = script["words"]
    duration = script["durationMs"] or max((w["endMs"] for w in words), default=1)
    clips = result.get("clips", [])
    footage = [c for c in clips if c.get("kind") != "cut-in"]
    cut_ins = [c for c in clips if c.get("kind") == "cut-in"]
    # Coverage: how much of each third of the source the cut uses, and the longest stretch it leaves out.
    spans = sorted((c["startMs"], c["endMs"]) for c in footage)
    merged = []
    for a, b in spans:
        if merged and a <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], b)
        else:
            merged.append([a, b])
    def used(lo, hi):
        return sum(max(0, min(b, hi) - max(a, lo)) for a, b in merged)
    thirds = [round(100 * used(duration * i / 3, duration * (i + 1) / 3) / (duration / 3)) for i in range(3)]
    edges = [0] + [x for span in merged for x in span] + [duration]
    gap = max(edges[i + 1] - edges[i] for i in range(0, len(edges) - 1, 2)) if merged else duration
    # Clip edges: does each clip end on a finished sentence?
    ending = 0
    for c in footage:
        inside = words_in(words, c["startMs"], c["endMs"])
        if inside and re.search(r"[.?!][\"”']?$", inside[-1]["text"].strip()):
            ending += 1
    stats = result.get("stats") or {}
    engine = result.get("engine")
    biggest = stats.get("biggestReplyTokens") if engine == "cli" else round((stats.get("biggestCallBytes") or 0) / 3.5)
    return {
        "Built": "yes" if result.get("ok") else f"no: {result.get('error', '')[:160]}",
        "Footage kept": clock(sum(c["endMs"] - c["startMs"] for c in footage)),
        "Clips · cut-ins": f"{len(footage)} · {len(cut_ins)}",
        "Narration words": sum(len((c.get("narration") or "").split()) for c in cut_ins),
        "Letter pages": result.get("pages", "—"),
        "Source used (1st · 2nd · 3rd third)": " · ".join(f"{t}%" for t in thirds),
        "Longest stretch left out": clock(gap),
        "Clips ending on a finished sentence": f"{round(100 * ending / len(footage)) if footage else 0}%",
        "Repairs": stats.get("repairs", "—") if engine == "cli" else f"{stats.get('refused', 0)} refused tool calls in {stats.get('toolCalls', 0)}",
        "Biggest single reply (output tokens, about)": biggest if biggest else "—",
        "Cost (API prices)": f"${result.get('costUsd', 0):.2f}",
        "Time": f"{round(result.get('seconds', 0) / 60, 1)} min",
    }


def render(result, script):
    if not result.get("ok"):
        return f'<p class="bad">This build failed: {html.escape(str(result.get("error", "")))}</p>'
    words = script["words"]
    out = []
    for c in result["clips"]:
        if c.get("kind") == "cut-in":
            text = html.escape(c.get("narration") or "")
            reuse = f'<span class="reuse">reused case: {html.escape(c["reuse"])}</span>' if c.get("reuse") else ""
            out.append(f'<div class="cut-in">{reuse}{text.replace(chr(10) + chr(10), "<br><br>")}</div>')
        else:
            said = " ".join(w["text"] for w in words_in(words, c["startMs"], c["endMs"]))
            out.append(f'<div class="clip"><span class="t">{clock(c["startMs"])}–{clock(c["endMs"])}</span> {html.escape(said)}</div>')
    report = html.escape(result.get("report", "")).replace("\n", "<br>")
    return "".join(out) + f'<div class="report"><b>Report</b><br>{report}</div>'


results = [json.loads(p.read_text()) for p in sorted(RESULTS.glob("*.json"))]
pairs = {}
for r in results:
    pairs.setdefault((r["id"], r["run"]), {})[r["engine"]] = r
rng = random.Random(20261023)
sections, key = [], []
for (sid, run), pair in pairs.items():
    if set(pair) != {"cli", "sdk"}:
        continue
    script = json.loads((SCRIPTS / sid / "script.json").read_text())
    order = ["cli", "sdk"]
    rng.shuffle(order)
    a, b = pair[order[0]], pair[order[1]]
    ma, mb = measure(a, script), measure(b, script)
    # Repairs and reply sizes give the engine away, so they wait for the reveal.
    rows = "".join(f"<tr{' class=tell' if k in TELLS else ''}><th>{k}</th><td>{html.escape(str(ma[k]))}</td><td>{html.escape(str(mb[k]))}</td></tr>" for k in ma)
    label = html.escape(a["label"]) + (f" · run {run}" if any(k == (sid, 2) for k in pairs) else "")
    n = len(sections) + 1
    key.append(f"<li>{label}: A is {ENGINE[order[0]]}, B is {ENGINE[order[1]]}</li>")
    sections.append(f"""
<section>
  <h2>{n}. {label}</h2>
  <div class="pair">
    <div class="side"><h3>A <span class="who" data-who="{ENGINE[order[0]]}"></span></h3><div class="doc">{render(a, script)}</div></div>
    <div class="side"><h3>B <span class="who" data-who="{ENGINE[order[1]]}"></span></h3><div class="doc">{render(b, script)}</div></div>
  </div>
  <table><tr><th></th><th>A</th><th>B</th></tr>{rows}</table>
</section>""")

PAGE.write_text(f"""<!doctype html><html lang=en><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1">
<title>Writer builds: old and new, blind</title>
<style>
body{{margin:0;padding:32px;background:#0b0b0b;color:#ededed;font:14px/1.55 -apple-system,"Inter",system-ui,sans-serif}}
h1{{font-size:22px;font-weight:600;margin:0 0 6px}} .sub{{color:#969696;max-width:860px;margin:0 0 20px}}
button{{height:34px;padding:0 14px;border:1px solid #363636;border-radius:8px;background:#151515;color:#ededed;font:inherit;cursor:pointer}}
section{{margin:36px 0;padding-top:20px;border-top:1px solid #242424}} h2{{font-size:17px;font-weight:600;margin:0 0 12px}}
.pair{{display:grid;grid-template-columns:1fr 1fr;gap:16px}} .side h3{{font-size:15px;margin:0 0 8px}}
.who{{color:#969696;font-weight:400}} .revealed .who::after{{content:" · " attr(data-who)}}
.doc{{height:640px;overflow:auto;padding:16px 18px;border:1px solid #242424;border-radius:10px;background:#fff;color:#1a1a1a;font:13px/1.5 Georgia,"Times New Roman",serif}}
.clip{{margin:0 0 8px;color:#333}} .clip .t{{font:11px ui-monospace,Menlo,monospace;color:#6a6a6a;margin-right:4px}}
.cut-in{{margin:12px 0;padding:8px 10px;border-left:3px solid #7088dc;background:#f3f5fb;font-family:-apple-system,"Inter",sans-serif}}
.reuse{{display:block;font-size:11px;color:#6a6a6a}} .report{{margin-top:18px;padding-top:10px;border-top:1px solid #ddd;font-family:-apple-system,sans-serif;color:#444}}
table{{width:100%;margin-top:12px;border-collapse:collapse;font-size:13px}} th,td{{padding:6px 10px;border-bottom:1px solid #242424;text-align:left;vertical-align:top}} th{{color:#969696;font-weight:500;width:34%}}
.bad{{color:#c0392b}} #key{{display:none;color:#ababab}} .revealed #key{{display:block}}
.tell{{display:none}} .revealed .tell{{display:table-row}}
</style>
<h1>Writer builds: the old and the new, blind</h1>
<p class=sub>Each script was built twice from the same transcript, research and guides: once by today's build (one answer from Opus), once by the new one (the Agent SDK, writing the plan in steps with Writer's tools). Each pair is labelled A and B in a random order. Read them, decide which you'd rather hand to the editor, then reveal which is which. The numbers under each pair are measured from the builds; the two that give the engine away (repairs and reply sizes) appear with the reveal.</p>
<button onclick="document.body.classList.toggle('revealed')">Reveal which is which</button>
<ol id=key>{''.join(key)}</ol>
{''.join(sections)}
""")
print(f"{len(sections)} pairs → {PAGE}")
