#!/usr/bin/env python3
"""A reviewed Google Doc as the teaching loop's raw feedback: its Word export
(link-shared Docs need no sign-in) read for the text, every comment with the
words it's anchored to, and the suggested edits (tracked insertions and
deletions), with who made them.

  python3 scripts/teach/read_doc.py <doc link or id> <out.json> [--skip-author NAME]...

Writer's own clip comments (GeniusScriptwriter10) are skipped by default.
"""
import json, re, sys, urllib.request, zipfile, io, xml.etree.ElementTree as ET

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
link, out_path = sys.argv[1], sys.argv[2]
skip = {"GeniusScriptwriter10"} | {sys.argv[i + 1] for i, arg in enumerate(sys.argv) if arg == "--skip-author"}
doc_id = re.search(r"/document/d/([A-Za-z0-9_-]+)", link).group(1) if "/document/d/" in link else link
docx = urllib.request.urlopen(f"https://docs.google.com/document/d/{doc_id}/export?format=docx", timeout=60).read()
archive = zipfile.ZipFile(io.BytesIO(docx))
body = ET.fromstring(archive.read("word/document.xml")).find(W + "body")
comments = ET.fromstring(archive.read("word/comments.xml")) if "word/comments.xml" in archive.namelist() else []

meta = {}
for c in comments:
    lines = ["".join(t.text or "" for t in p.iter(W + "t")) for p in c.iter(W + "p")]
    meta[c.get(W + "id")] = {"id": c.get(W + "id"), "author": c.get(W + "author"), "date": c.get(W + "date"), "text": "\n".join(x for x in lines if x)}
anchors = {key: "" for key in meta}
inside = {key: False for key in meta}
open_ranges, paragraphs, suggestions = set(), [], []
order = {}
state = {"current": None}


def walk(element, kind=None):
    for child in element:
        tag = child.tag.replace(W, "")
        if tag == "commentRangeStart":
            open_ranges.add(child.get(W + "id"))
            order.setdefault(child.get(W + "id"), len(order))
        elif tag == "commentRangeEnd":
            open_ranges.discard(child.get(W + "id"))
        elif tag in ("ins", "del"):
            state["author"] = child.get(W + "author", "")
            walk(child, tag)
        elif tag in ("t", "delText"):
            text = child.text or ""
            mark = "del" if tag == "delText" else kind
            if mark in ("ins", "del"):
                if state["current"] is None:
                    state["current"] = {"before": "", "after": "", "paragraph": len(paragraphs) - 1, "author": state.get("author", "")}
                    suggestions.append(state["current"])
                state["current"]["after" if mark == "ins" else "before"] += text
            else:
                if text.strip():
                    state["current"] = None
                paragraphs[-1] += text
            for key in open_ranges:
                anchors[key] += text
                inside[key] |= mark == "ins"
        elif tag == "p":
            paragraphs.append("")
            if state["current"] is not None:
                state["current"]["before" if kind == "del" else "after"] += "\n"
            walk(child, kind)
            for key in open_ranges:
                anchors[key] += "\n"
        else:
            walk(child, kind)


walk(body)
for suggestion in suggestions:
    suggestion["before"], suggestion["after"] = suggestion["before"].strip(), suggestion["after"].strip()
items = []
# In the Doc's order (where each comment's words start), not comments.xml's.
for key, comment in sorted(meta.items(), key=lambda pair: order.get(pair[0], len(order))):
    if comment["author"] in skip:
        continue
    items.append({"kind": "comment", "author": comment["author"], "quote": anchors[key].strip(), "onSuggestedText": inside[key], "text": comment["text"]})
for suggestion in suggestions:
    items.append({"kind": "suggestion", "author": suggestion["author"], "quote": suggestion["before"], "replacement": suggestion["after"], "text": ""})
for number, item in enumerate(items, 1):
    item["id"] = f"F{number}"
json.dump({"doc": doc_id, "paragraphs": paragraphs, "items": items}, open(out_path, "w"), indent=1, ensure_ascii=False)
print(f"{len(paragraphs)} paragraphs; {sum(i['kind'] == 'comment' for i in items)} comments and {sum(i['kind'] == 'suggestion' for i in items)} suggestions to learn from")
