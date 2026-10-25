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

## Bodycam compilations sent for review (Oct 25 2026)
Three bodycam (FOIA) compilations from the Interrogation "Ready for script (comp)" cards, built in Writer the way a producer does it (card link → link check → import → reused cases → build → checks → review link). None has notes yet. Once they're reviewed, they're the reviewed bodycam compilations the key above is waiting for.

| Card | Script | Review link | Version |
|---|---|---|---|
| [999 Cop Reports Dead Body…](https://trello.com/c/fCTopHe2) | `69f4a95f-f73d-495b-a0a6-934898a578dc` (built Oct 24 on the MacBook Air) | https://feedback.spheceo.com/r/nfNcuBcHVZh6vhZpjecPng | v1 |
| [1005 Pr*datory Dad…](https://trello.com/c/Dzd1VldB) | `c611d693-97f1-43de-8390-6aa03f02d671` | https://feedback.spheceo.com/r/jCu39Cd5NTqrxjxZmYn2YA | v2 (v1 had misheard names: Reece, Levia, Jamalee, "Springfield, Ohio"; v2 is rebuilt from per-case research) |
| [996 Officers Traumatized…](https://trello.com/c/zZr4gnzA) | `2ec5d0bc-d069-47fd-bc19-40b0fc7632ac` | https://feedback.spheceo.com/r/tZ8BMESSv8Z-gPY89KncxA | v1 |

Things for the reviewer to check:
- 1005, case 3 (Marcus W. Leyva): neither the footage nor the reports give the year, so the dates are months only.
- 996: three reused cases (5cN67V8lkeE, case 4 of "When HERO Cops Save Kids From House Of Horror"; pWoE9YSIxMw and OClS41Ju36A, cases 2 and 4 of "When Evil Parents Finally Get Exposed by the DCF"). Writer wrote only their transitions, from the earlier scripts' own narration. The editor lifts those segments whole.
- 996, case 2 (Ketchum, Idaho): no date anywhere, so its case opening starts with the place.

## Review of the three bodycam compilations (Oct 25 2026)
Status: tested — needs work (fixes in v2 of each; the house rules have not passed a training round yet)

Reviewed against foia.md, compilation.md and this record, relayed by the Feedback thread. The user: "can we get the agent to fix this and we update its in-house guide here in Writer. All of these issues you've raised."

**What's right in all three:** the structure and the transitions (one sentence ending "…", straight into the clips, never "the last case"; the hidden compilation's first case, then "Moving on to the case…").

**What all three break** (each is now a rule in `src-tauri/guides/house.md` and, where a count can find it, a check in `house::text_flags` / `claude::team_flags`):
- People aren't introduced by full name and age where they first matter; some are first-name only, some first appear in an outro.
- Addresses and places that don't matter: Southeast Burgundy Lane, El Prado Avenue, room 209 of the Red Roof Inn, Buena High School in Sierra Vista.
- Cut-ins summarise what the footage just showed instead of adding context: "Reese denied all of it.", "To every question, Benjamin gave the same answer: no…", "Jason blamed Jamilee, and Jamilee blamed Jason…", "Dougie said he would never normally press charges…". (Judged; guide only.)
- Outros: some have no clear outcome (996 case 3 "…to type up the charge against him."; 999 case 2 restates the cut-in before it), some are padded with dates, charges and the judge (999 case 3, 91 words; 1005 cases 1, 2 and 4, 76–78 words). Short and final: an outcome in one or two sentences.
- Opening clips over 8 s or filler ([8:26–8:27] "That's, that, that's,"; [6:18–6:28]; [6:04–6:20]; [18:28–18:37]), and in 996 and 999 they never play in the body. (Compilations had skipped the replay check single cases get.)

**Per script**
- 996: "hit in the head" (harm by what it meant, never the blow); the case 2 mother never named (Briana); case 2 opening undated; two transitions open "Up next, we have a case where…"; an empty [0:00–0:00] clip at the end of case 3 (product bug: a 200 ms spill of a padded edge into the reused video; `tidy::fit_to_parts` now drops spills under 1 s).
- 999: facts told twice (former sergeant, 911, "shot herself", Publix); meta flashback ("To understand how … it helps to go back…"); case 3 transition doesn't tie to the title and gives the outcome away; case 1 outro states guilt as a conclusion ("investigators concluded…"); case 1 opening 83 words.
- 1005: "child pornography" twice in the case 3 outro (never pair a care word with "child"); care words beyond need (charge lists, "sexually assaulted… charged with rape"); case 2 outro includes what came after (the resignation) and has no "In the end"; cut-ins of 79 and 80 words against 50–65; case 4 first block 4:26 against one to two minutes; case 3 date to check ("January 8, 2026": the reports say "Thursday, January 8", which fits 2026, but state no year).

**Guides.** `foia.md` and `compilation.md` stay byte-identical to Repack's `skills/foia` and `skills/compilation`, so Repack's updates can still be copied over whole. Writer's own rules live in `guides/house.md`, given after them ("WRITER'S HOUSE RULES") to FOIA cases and non-courtroom compilations, in builds and revisions alike.

**Outcome (Oct 25 2026).** Each script was fixed by Writer's agent through chat revisions given the review's notes and the house rules, the proposals accepted, and the audit run after each round. Shared on the same links:
- 996: v2, https://feedback.spheceo.com/r/tZ8BMESSv8Z-gPY89KncxA (script rev 10, 6 pages). Five rounds: the notes; a 9 s intro clip the clean-up's padding had pushed back over 8 s; a 5-page script (tightened cut-ins lost a page) brought back to 6 with footage and three context cut-ins. Left flagged on purpose: case 2 has no date in any source. Briana, Douglas ("Dougie"), Jesse and Joseph have no surnames or ages in the footage or research.
- 999: v2, https://feedback.spheceo.com/r/nfNcuBcHVZh6vhZpjecPng (script rev 6, 6 pages, audit clean). To check: Ken's date (August 31, 2011, worked out from the footage), Jenny's surname ("Games" from the closing card; Zelich says "Gomez"), and that Ken's outcome is unknown, so his outro ends on the truck search and his denial.
- 1005: v3, https://feedback.spheceo.com/r/jCu39Cd5NTqrxjxZmYn2YA (script rev 8, 7 pages). Case 3 opens "On January 8," with no year; no report states it, though the weekday fits 2026. "Nicole Abair" is the false name Jamilee Hunt gave; the audit lists it as a first-name mention, and that's intended.

Product changes from this round: `guides/house.md`; `house::text_flags` plus intro-clip, first-block and empty-clip checks in `claude::team_flags` (FOIA single cases get the text checks too); revisions of a compilation are sent back when their edits would take it out of 6–7 pages (`revise::keeps_length`); the audit reports pages; `tidy::fit_to_parts` drops sub-second spills into the next recording.

**Oct 25 2026, later:** at the user's request, each link now holds only its final script, as v1 ("Make this v1", `reviews:compress`). 996 (rev 10), 999 (rev 6) and 1005 (rev 8) are v1 on the same links. Their earlier versions are gone from the links. None had notes to move.
