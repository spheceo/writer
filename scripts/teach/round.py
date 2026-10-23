#!/usr/bin/env python3
"""One round of the teaching loop: each named script built from scratch (or
reviewed blind) by the dev app's debug route, saved nowhere in Writer, kept
under the loop's folder for score.py. Run with `just dev` up.

  python3 scripts/teach/round.py <loop dir> <round> name=<script id>:<build|review|apply>[:<draft.json>[:<request.txt>]] ...

`review` and `apply` start from the given draft (a result.json's or a bare
draft) instead of the saved one; `apply` carries out the request's text.

Writes <loop>/rounds/<round>/<name>/result.json and script.txt.
"""
import json, sys, time, urllib.request, concurrent.futures as cf
from pathlib import Path

LOOP, ROUND = Path(sys.argv[1]).expanduser(), sys.argv[2]
JOBS = [(arg.split("=", 1)[0], *arg.split("=", 1)[1].split(":")) for arg in sys.argv[3:]]


def draft_and_request(draft_file, request_file):
    out = {}
    if draft_file:
        value = json.loads(Path(draft_file).expanduser().read_text())
        out["draft"] = value.get("draft", value)
    if request_file:
        out["request"] = Path(request_file).expanduser().read_text()
    return out


def run(name, script, mode, draft_file=None, request_file=None):
    out = LOOP / "rounds" / ROUND / name
    out.mkdir(parents=True, exist_ok=True)
    began = time.time()
    request = urllib.request.Request(
        "http://127.0.0.1:1421/api/debug/teach",
        data=json.dumps({"id": script, "mode": mode, **draft_and_request(draft_file, request_file)}).encode(),
        headers={"content-type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=3 * 3600) as answer:
            result = json.loads(answer.read())
    except Exception as error:
        body = getattr(error, "read", lambda: b"")()
        result = {"error": (body.decode(errors="replace") if body else str(error))[:2000]}
    result["name"], result["wallSeconds"] = name, round(time.time() - began)
    (out / "result.json").write_text(json.dumps(result, indent=1, ensure_ascii=False))
    if "text" in result:
        (out / "script.txt").write_text(result["text"])
    print(f"{time.strftime('%H:%M:%S')} {ROUND}/{name} {'ok' if 'draft' in result else 'FAILED: ' + result.get('error', '')[:300]}", flush=True)


with cf.ThreadPoolExecutor(max_workers=len(JOBS)) as pool:
    list(pool.map(lambda job: run(*job), JOBS))
