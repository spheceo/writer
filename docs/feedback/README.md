# Feedback and training rounds

This folder is the shared place to collect feedback and consolidate it by category or topic before training. Add feedback as it comes in; we plan to run training rounds every 2–3 days rather than train separately on each comment.

As of **25 October 2026**, the next round is tentatively planned for **27–28 October 2026** (Africa/Johannesburg). This is a planning window, not an automated schedule. After each round, record the results here and set the next window.

## Topics collected so far

| Category | Feedback record | Preparation for the next round |
| --- | --- | --- |
| Commentary: writing voice, attributions, narrative flow, clip use, quote pop-ups and precise editor notes | [N3on commentary](2026-10-24-n3on-commentary.md) | Consolidate the general lessons and the earlier round's misses; keep script-specific requests attached to their examples. |
| Interrogation compilations: openings, cut-in context, footage order, transitions, outros and consistency across cases | [Compilation feedback](2026-10-24-compilations.md) | Build and review the corresponding scripts in Writer so the round can grade Writer's output against the team's notes. |
| Bodycam compilations: pacing, introductions and compilation structure | [Compilation feedback](2026-10-24-compilations.md) | First review in (25 Oct 2026): names, places, summary cut-ins, outros and intro clips, now in `guides/house.md` and the audit; v2 of each script fixed. Tested — needs work: grade the next build against the review. |
| Courtroom cuts: clip edges on the intended words, intro pieces with no sound, captioned footage | [Courtroom cuts](2026-10-26-courtroom-cuts.md) | Product fix (26 Oct 2026): exact hearing of the chosen footage and no sliver pieces; v2 of both scripts on their links. Tested — needs work: the producer's review of v2. |

## How to consolidate feedback

- Group related notes under the same topic, even when they come from different scripts or reviewers. Link dated source records rather than losing their history.
- Keep the reviewer's wording, source/review link, script and version, and the affected passage or timestamp where available.
- State the recurring problem and the behavior we want. Distinguish a general lesson from a correction that belongs only to one script.
- Track each topic as **collecting**, **ready for a round**, **tested — needs work**, or **verified**, with links to evidence. A guide or code change alone does not mean the lesson has passed a training round.
- Keep app bugs and UI requests identifiable as product work. Import timeouts, progress visibility, recording boundaries, zoom, chat and diff controls need implementation and verification; include them in model training only if there is a corresponding model-behavior lesson.

For a new topic, start a dated Markdown file here and add it to the table above. Include:

```md
# Feedback: <category or topic>, <date>

Status: collecting

## Sources and examples
Reviewer wording, links, script/version and passages or timestamps.

## Consolidated lessons
Recurring problems and the desired behavior, grouped by theme.

## Next training round
Inputs, reviewed examples/answer key, checks and any missing preparation.

## Round results
Date, run location, what improved, misses/regressions and follow-up.
```

## Where the rounds live

These Markdown files hold the feedback, preparation and results summary. Existing local training materials live outside the repo in `~/writer-experiments/teach-1/` and `~/writer-experiments/teach-2/`; the latter has feedback items in `doc/` and private answer keys in `keys/`. Link the relevant run from its topic record after training.

Writer's in-app teaching flow also keeps each script's feedback and rounds in its own `scripts/<id>/feedback/` folder. See the [main README](../../README.md) for **Teach from these notes**, **Run round**, and **Improve the lesson**.
