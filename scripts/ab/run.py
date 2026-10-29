#!/usr/bin/env python3
"""Builds each test script on both engines from the same inputs, saving nothing
to the scripts (a debug build's POST /api/debug/ab-build), and keeps each
result as JSON for page.py. Run with `just dev` up and signed in.

  python3 scripts/ab/run.py <out dir> [engine,engine]

Engines: cli (one claude -p answer), sdk (the Agent SDK in steps, today's)
and fast (an outline, its sections at once, a finish); sdk,fast unless named.
A fast build's effort is the app's WRITER_FAST_EFFORT (medium unless set).
"""
import json, sys, time, urllib.request, concurrent.futures as cf
from pathlib import Path

OUT = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/writer-ab")
OUT.mkdir(parents=True, exist_ok=True)
ENGINES = (sys.argv[2] if len(sys.argv) > 2 else "sdk,fast").split(",")
# id, label, kind, runs
SCRIPTS = [
    ("57924d08", "Murderer Tricked Into a Confession (short interrogation, 8 min)", "interrogation", 1),
    ("2c57da30", "Girl Being Kidnapped (FOIA bodycam, 16 min)", "foia", 1),
    ("505316aa", "Trooper FAKES Testimony (courtroom, 21 min)", "courtroom", 2),
    ("b8572264", "Predator Teacher (compilation: 2 videos + a reused case, 67 min)", "interrogation", 1),
    ("37b8c334", "Judge EXPLODES (courtroom, 46 min, research-heavy)", "courtroom", 2),
    ("04b2d7f9-1b2e-4f9a-0000-000000000000", "Cristian Fernandez (interrogation, 61 min, the longest)", "interrogation", 2),
]
root = Path.home() / "Library/Application Support/com.spheceo.writer/scripts"
def full_id(prefix):
    return next(p.name for p in root.iterdir() if p.name.startswith(prefix[:8]))

def build(job):
    sid, label, kind, engine, run = job
    out = OUT / f"{sid[:8]}-{engine}-{run}.json"
    if out.exists():
        return out.name, "kept"
    body = json.dumps({"id": sid, "engine": engine, "kind": kind}).encode()
    req = urllib.request.Request("http://127.0.0.1:1421/api/debug/ab-build", data=body, headers={"content-type": "application/json"})
    began = time.time()
    try:
        with urllib.request.urlopen(req, timeout=3 * 3600) as answer:
            result = json.load(answer)
    except Exception as error:
        result = {"ok": False, "error": f"harness: {error}", "seconds": time.time() - began}
    result.update({"id": sid, "label": label, "run": run})
    out.write_text(json.dumps(result))
    return out.name, "ok" if result.get("ok") else f"failed: {str(result.get('error'))[:120]}"

jobs = []
for prefix, label, kind, runs in SCRIPTS:
    sid = full_id(prefix)
    for run in range(1, runs + 1):
        for engine in ENGINES:
            jobs.append((sid, label, kind, engine, run))
# Two at a time: each pair is one script's two builds, side by side.
with cf.ThreadPoolExecutor(max_workers=2) as pool:
    for name, said in pool.map(build, jobs):
        print(time.strftime("%H:%M:%S"), name, said, flush=True)
