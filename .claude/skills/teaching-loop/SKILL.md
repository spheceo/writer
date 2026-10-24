---
name: teaching-loop
description: Teach Writer's script writer from a reviewer's feedback (Doc comments, suggested edits, review-link notes) without handing it the notes. Use when a reviewed script comes back and the team wants Writer to learn from it.
---

# Teaching Writer from a review

Ported from Edit's teaching loop (`edit/.claude/skills/teaching-loop`). The point, in the user's words: "have the agents be taught these things instead of us blatantly handing over the notes." The reviewer's feedback is the answer key; Writer's agent writes the script again without ever seeing it; it's scored against the key; what it missed becomes rules in Writer's guides and checks in Writer's code; repeat until it matches, with a control so no lesson makes another script worse.

## Rules
- Never show the agent the feedback, the key or where the problems were. Rules must make sense on any script: the principle, the reviewer's words so the reason travels, and an invented example if one helps. Never this script's names, quotes or places (feedback.rs `leaks` is the check for a lesson's text).
- The key lives outside the repo and outside anything a prompt reads: `~/writer-experiments/<loop>/keys/<script>.md`.
- The control runs every round. The holdout runs once, at the end, and nothing is taught from it.
- Stop when every row is ok on the reviewed script's build, or after two rounds without improvement (Edit's bar); at most ~10 rounds. What's left goes to the write-up, and, for the script itself, is fixed directly (feedback.rs's Finish does this in the app).

## The loop
1. **Read the feedback.** `python3 scripts/teach/read_doc.py <doc link> ~/writer-experiments/<loop>/doc/items.json`: the Doc's text, every comment with the words it's on, and the suggested edits (tracked changes in the Word export, which a link-shared Doc gives without sign-in). Writer's own clip comments are skipped. Review-link notes come with their anchors and, for suggestions, `replacement`.
2. **Write the key by hand**: one row per pattern, many comments to a row (four "not according to X" notes are one pattern). Each row (K1, C1, I1… any capital and a number): the pattern, the reviewer's words (item IDs), the check (A = counted from the draft, J = judged), the scope (`all` = holds for every script of the format; `here` = this script's facts). See `teach-1/keys/n3on.md`.
3. **Baseline.** `scripts/teach/snapshot.py` turns the reviewed draft into a round result (`BEFORE`); `scripts/teach/round.py <loop> R0 name=<id>:build …` writes fresh scripts with the debug route `POST /api/debug/teach` (`build` = from the sources, as a new build; `review` = the written script handed back blind, as the team does), saving nothing in Writer. `just dev` must be up, and nothing under `src-tauri/` may be edited while a round runs (the dev app restarts).
4. **Score.** `scripts/teach/score.py <loop> <round> <name> <key> [--items items.json] [--scope all]`: the A counts from the draft, every row judged ok / half / miss by Opus with evidence, and on the reviewed script whether each of the reviewer's notes would come back. Read the scripts yourself too: the grader is a second opinion, not the judge.
5. **Teach, in this order** (Edit's order; a miss is usually not a missing sentence):
   1. Did the writer have what it needed? (the brief, the sources it was meant to use, room in the prompt)
   2. Do Writer's own checks contradict the rule? (`commentary::compile`, the pacing check, `audit`)
   3. Can a check catch it? An audit that names the pattern is what makes a lesson stick: `commentary::audit` flags it and the build sends the script back once to fix it, as a refused clip is.
   4. Only then the smallest text change: a rule in `src-tauri/guides/commentary.md` (or the format's guide), a CAPS name, the principle, the reviewer's words.
   Commit each round's changes; note the commit in `LOG.md`.
6. **Next round**, on the reviewed script and the control, until the bar. Then the holdout, once.
7. **The script itself**: the reviewed script reviewed blind by the taught writer (`review` mode), the reviewer's own words where they typed them (suggested edits are applied as written), and what the last round still missed fixed directly. Publish it as a new review link (v1).

## Where things are
- `src-tauri/src/feedback.rs`: the in-app version (Feedback tab: a Doc's comments → key → lesson → blind rounds → kept lessons that every commentary build reads).
- `src-tauri/src/fixing.rs`: review-link notes, suggestions applied as written (`replacement`, deletions, insertions, groups) and resolved accepted / rejected.
- `src-tauri/guides/`: the guides every build reads. `commentary::audit`: the checks that send a script back.
- `~/writer-experiments/<loop>/`: `doc/`, `keys/`, `rounds/R<n>/<name>/{result.json,script.txt,score.json,score.txt}`, `LOG.md` (the source of truth: per round, per row, and what was taught).
- Review-page notes come in through Writer itself: **Teach from these notes** (`POST /api/scripts/<id>/feedback`) keeps each note's id, version, block, offsets, words, intent, status and signed replies in `scripts/<id>/feedback/feedback.json`, with `base.json` the version the notes are on as it was shared. Copy them into the loop's `doc/items.json` (teach-2 shows the shape, with general team notes as T rows).
- A round's result.json carries `measures` (`commentary::measure`: attributions, sentences into clips, and then / sequence openers / recital runs, whole-paragraph notes, quote pop-ups) and the audit's `flags`; score.py adds them to its counts. `POST /api/debug/audit {id, draft}` measures any draft; `GET /api/scripts/<id>/audit` the saved script (a footage compilation's `claude::team_flags` too).
- teach-2 (Oct 24 2026): keys for the commentary style (C rows, the N3on v1 review and the team's notes), interrogation compilations (I rows) and bodycam compilations (B rows, provisional: no reviewed bodycam comp yet). The record of the feedback is `docs/feedback/2026-10-24-*.md`.
