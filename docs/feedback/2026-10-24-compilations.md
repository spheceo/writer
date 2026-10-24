# Feedback: bodycam and interrogation compilations, made ready for training (Oct 24 2026)

## The guides
- Writer's `guides/compilation.md`, `foia.md`, `cut-in.md` and `judgment.md` are byte-identical to Repack's `apps/web/src/server/skills/*/SKILL.md`.
- The compilation layouts, length, reuse rules, transitions and sequences in `formats.rs` match Repack's `script-compilation.ts` word for word, with Repack's tools said as what Writer does.
- **Interrogation compilation**: cut-in + judgment + compilation, as in Repack's Interrogation workspace. Repack's compilations come from Trello "Ready for script (comp)".
- **Bodycam compilation** (`scripts.foia-compilation`, since Oct 24 2026): Writer gives it the compilation guide and the FOIA guide.
  - Repack can't make one: `hasCompilations()` is true only for the Interrogation workspace, and its API answers "This workspace writes single cases only." Its references are all bodycam comps, though ("When GOOD Cops Discover Horrifying Crime Scenes", "When Parents Abandon Their Children"), so the compilation guide fits.
  - Where the two guides differ (FOIA's one-case opening, word counts, "officers never named"), Writer says to follow the compilation guide.
- **Not in Repack's guides, in Writer's or anywhere else**: the transition the team's finished comps use. Both reviewed comps move on inside the previous case's outro: "…eligible for release.\n\nIn a similar case, another young man thought he could outsmart investigators…". The team's doc "How we want our edits to be" (Edit `docs/moodboard/README.md` L74–77) also likes "But this is just one of three cases when…".

## The team's feedback on compilations
**There is none on a bodycam compilation**, anywhere: Repack, Writer, Edit, `~/edit-experiments`, `~/.edit/reviews.json`. Writer's only compilation, "Cop Reports Dead Body…" (`27995ddf`), has no notes.

The only compilation feedback is JJ's, on Edit's cuts of two hidden interrogation comps. Words are quoted exactly; SCRIPT marks what the script decides.

**"18-Year-Old Shoots 3 Teens"** (`~/edit-experiments/comp-18yo/v*-feedback/comments.txt`)
- v1 L5 (0:02) "We need to be able to see the occupants." — SCRIPT (opening clip choice)
- v1 L13 (0:03) "There are some mistakes and the cuts here." — SCRIPT/EDIT (intro clip edges)
- v1 L63 (23:15) "When we say "in a similar case," we just need to switch over to the next case." — the transition (EDIT; also how the script hands over)
- v1 L147 (50:54) "There is a voiceover mistake here. It seems like we start in the middle of something." — SCRIPT
- v1 L171 "Check this through for the rest of the video and ensure that the previous feedback given in the rest of the video is also applied to the third case right here." — every case
- v3 L25 (1:07) "play this longer or remove it" — SCRIPT/EDIT
- v3 L37 (6:54) "There are some errors here in the cutting. It seems like there is some switching up in the footage that shouldn't be there." — SCRIPT (footage order)

**"Helpful" Wife Has No Idea** (`~/edit-experiments/helpful-wife/…`, Edit `docs/feedback/2026-10-20-helpful-wife.md`)
- v2 L19 "I think we should generally try to change the pace of this segment right here so it becomes a lot more interesting. We can also do that by looking at how we can change the first part." — SCRIPT/EDIT
- v2 L35 "There are simply too many mistakes in this intro right here." — SCRIPT/EDIT
- v2 L49 (1:06) "I don't like the clips right here. I feel we can do a little better." — SCRIPT (intro clips)
- v2 L169 (23:27) "There is a mistake in the last line." — SCRIPT (an outro)
- v2 L221 (38:47) "We need to change how we start the cutin." — SCRIPT
- v2 L225 (38:56) "…we need to change the last line right here. There is a mistake in the text." — SCRIPT
- v3c L5 "I need you to redo the first 30 seconds. This is not good enough." — SCRIPT/EDIT
- Edit doc L66 "Dead space like this must be removed" (a 1.6 s pause inside the first teaser) — SCRIPT/EDIT

**Bodycam singles.** These are the nearest thing to bodycam-comp feedback, and the FOIA guide already teaches most of it:
- "I think we need to have the first cut in right here - right now we wait 18 seconds before we have the first cut in" (Mom Leaves v6, M)
- "who is james? we haven't really heard about him before or seen his face while he's talking so this might be a little difficult for the viewer..." (Mom & Teen)
- "we've called her both Roxy and, now, a social worker" (Mom Leaves)
- "start* sohuld be the first word here" and "need to cut deadspace here" (Mom & Teen)

## Patterns: teach, code, check
The rules go to the interrogation and bodycam compilation kinds, `When` and hidden alike. Courtroom compilations have no notes and aren't touched.

**Teach.** The compilation sequence (`formats.rs` `TEAM_COMPILATION`, after Repack's rules) carries each rule with the team's words:

| Rule | Team's words | Check (`claude::team_flags`) |
|---|---|---|
| EVERY CUT-IN STANDS ON ITS OWN | "It seems like we start in the middle of something." / "We need to change how we start the cutin." | A case opening or transition opening on And, So, Also, Then, He, She, They, It, This, That, These or Those. "But" is allowed for "But this is just one of three cases when…". |
| NO DEAD SPACE IN THE TEASERS; show the people | "Dead space like this must be removed" / "We need to be able to see the occupants." | An intro clip (the opening's, or a case's after its transition) holding a pause over 1.5 s between words. |
| THE INTRO IS SHORT AND STRONG | "I need you to redo the first 30 seconds. This is not good enough." / "There are simply too many mistakes in this intro" | The opening's clips run over 45 s before the first cut-in, or a case's intro clips over 40 s (the guide: 2–5 clips of 1–8 s). |
| EACH CASE'S FOOTAGE PLAYS IN ITS OWN ORDER | "It seems like there is some switching up in the footage that shouldn't be there." | A case's body footage going back more than 2 s to a moment it passed, within one recording. |
| THE LAST LINE IS RIGHT | "There is a mistake in the last line." ×2 | Judged (key row). The cause isn't in the notes. |
| ONE STANDARD FOR EVERY CASE | "ensure that the previous feedback … is also applied to the third case" | Judged (key row). |
| "In a similar case, …" in a hidden comp; the next case's intro clips follow at once | "When we say "in a similar case," we just need to switch over to the next case." | Judged. Repack's transitions stay the default. |

**Code.** The checks run where the build's whole-plan checks do: `off_target`, on both the steps path and the one-answer path. A plan they flag goes back once as off target; the steps path keeps a second finish as it is, and the one-answer path keeps the best plan after two repairs. They are shown in the Review dialog before sharing (`GET /api/scripts/<id>/audit`) and logged on every share.

## What belongs to Edit (listed, not changed)
- **A typewriter quote card.** Edit has `red_quote`, whose words rise in, and the `typewriter` text preset with typing sounds (`clips.ts`, `typing_sounds.ts`), but no op that builds a typed quote card. Writer's quote pop-ups reach Edit as `edit: Quote pop-up (typewriter): “…” (Who, Video m:ss-m:ss)` Doc comments on the quoted words. Edit could add a `quote_card {text, who, start, end}` op (typewriter preset, stagger from the words' read time, typing sounds) and an `agent/skills` line telling its agent to use it for those notes.
- **Commentary scripts.** `writer_handoff.ts` refuses them ("export its Google Doc and import that"). If Edit ever reads them directly, the structured shape is `Paragraph.marks` (`line`, `on`, `quote: {who, said: Cite}`), or the review block's `editMarks`.
- **"in a similar case" → switch to the next case at once**: already in `agent/styles/interrogation/notes.md:6` and `skills/outro.md:6`.
- **"We need to blur the previous subtitles here accordingly when we say "in a similar case."**: an edit at the transition (the uploader's burned-in subtitles).
- **A bodycam style folder.** `agent/styles/` has only courtroom and interrogation; bodycam singles and comps use the base skills. A bodycam comp's case headers, transitions and pacing would go there. The intro lessons (`intro_long` over 40 s, `intro_no_arrest`) already exist as audits.
- **Edit-only notes** (zooms, music, B-roll, labels, "show things like half a second before they're actually being said"): not script.

## What's needed before real training rounds
- **A reviewed bodycam compilation.** There's no answer key without one. The bodycam key (`~/writer-experiments/teach-2/keys/bodycam-compilation.md`) is built from the general compilation rows and the bodycam-single notes, marked provisional.
- **Interrogation comps.** JJ's notes are on Edit's cuts of comps whose scripts aren't in Writer's library. The key's script rows hold; to grade Writer itself, build the same cases in Writer (the 18-Year-Old and Helpful Wife videos) and have the team review those scripts.
