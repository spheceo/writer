#!/usr/bin/env python3
"""A taught draft with the reviewer's own typed words put in as written: a
suggested replacement of the whole Intro becomes the Intro's paragraphs, word
for word, each "(clip)" the reviewer left turned into the note for the
editor they asked for on it, and the montage before it gone when the
suggestion deleted it.

  python3 scripts/teach/with_suggestion.py <draft or result.json> <items.json> <suggestion id> <out.json>

Notes for the reviewer's "(clip)" placeholders come from the comments
anchored on them, in order; a comment anchored on a paragraph of the
suggestion becomes that paragraph's note.
"""
import json, re, sys
from pathlib import Path

source, items_path, suggestion_id, out_path = sys.argv[1:5]
value = json.loads(Path(source).read_text())
draft = value.get("draft", value)
items = json.loads(Path(items_path).read_text())["items"]
suggestion = next(item for item in items if item["id"] == suggestion_id)
paragraphs = [line.strip() for line in suggestion["replacement"].split("\n") if line.strip()]
# Comments on the suggested text, in the Doc's order: on "(clip)" or on a paragraph.
on_clips = [item for item in items if item["kind"] == "comment" and item.get("onSuggestedText") and item["quote"].strip() == "(clip)"]
on_text = [item for item in items if item["kind"] == "comment" and item.get("onSuggestedText") and item["quote"].strip() != "(clip)"]

intro = []
clip_comments = iter(on_clips)
for text in paragraphs:
    notes = []
    # Each "(clip)" sits right after the words it shows: the note says which.
    for before in text.split("(clip)")[:-1]:
        words = " ".join(re.sub(r"[,.;:]", "", before).split()[-4:])
        comment = next(clip_comments, None)
        asked = re.sub(r"^(find|here we need)\s+", "", comment["text"].strip(), flags=re.I) if comment else "the clip the narration names here"
        notes.append(f"Find, under \"{words}\": {asked}")
    for comment in on_text:
        if comment["quote"].strip() and comment["quote"].strip()[:60] in text:
            notes.append(f"edit: {comment['text'].strip()}")
    said = re.sub(r"\s*\(clip\)", "", text).strip()
    paragraph = {"id": "", "pieces": [{"kind": "say", "text": said}]}
    if notes:
        paragraph["note"] = " ".join(notes)
    intro.append(paragraph)

sections = draft["sections"]
at = next((index for index, section in enumerate(sections) if section["heading"].strip().lower().startswith("intro")), 0)
# Keep the ids the taught draft gave its intro paragraphs where there are as many; new ones are made on save.
for index, paragraph in enumerate(intro):
    if index < len(sections[at]["paragraphs"]):
        paragraph["id"] = sections[at]["paragraphs"][index].get("id", "")
sections[at] = {"heading": sections[at]["heading"], "paragraphs": intro}
if suggestion["quote"].lstrip().startswith("("):
    draft["open"] = []
Path(out_path).write_text(json.dumps(draft, indent=1, ensure_ascii=False))
print(f"Intro: {len(intro)} paragraphs, word for word; {sum('note' in p for p in intro)} with notes; montage {'removed' if not draft['open'] else 'kept'} → {out_path}")
