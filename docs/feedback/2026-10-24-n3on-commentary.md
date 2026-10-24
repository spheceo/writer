# Feedback: "The Disturbing Rise of N3on" (commentary), Oct 24 2026

Two sources:
- Marius's 8 notes on the review of v1: https://feedback.spheceo.com/r/ERD8i_JXywawF8ybNCTCog, script `2f02aa42-363b-4f98-8815-36ec39a9754b`, review `sh7cdnpyd5czeh9nfsqt1hbt2d8gw1d4`.
- The team's two general notes on the commentary style, sent through the user.

They are pulled into Writer with **Teach from these notes** (`POST /api/scripts/<id>/feedback`). That writes `scripts/<id>/feedback/feedback.json` with F1–F8, each with its note id, version, block, offsets, words, author, status and replies, and `base.json`, which is v1 as it was shared. The teaching loop's copy, with the team's notes as T1 and T2, is `~/writer-experiments/teach-2/doc/items.json`. The answer key is `~/writer-experiments/teach-2/keys/commentary-n3on.md` (private, outside the repo).

The earlier round of feedback, M's 28 comments on the Google Doc, lives in `~/writer-experiments/teach-1` (rows K1–K9).

## The team's notes, word for word

**T1**
> I think one thing in general we need to make the bot slow down on a bit is mentioning like "On steiny's podcast", "on bradley martyn's podcast", "according to logan paul". We say that like out of the blue, out of context. It is okay to say according to someone sometimes, but we can also say something on our own. We dont have to do the according to every single time
> it seems like it always does that

**T2**
> We need to fix writing style a bit. It's a lot of recitals: and then, and then, and then...
>
> Also it doesn't need to put a clip at the end of every sentence. It's almost as if it forces itself to do it. We want it to act way more natural, and only use a clip if it is freaking mandatory. Most can be explained by text. For example if something is being said in a clip, it can just quote the person and we can get a cool quote pop up typewriter effect.
>
> Another thing, when it adds comments about something, don't mark the whole text section. Only mark the part where it actually affects.
>
> I think those are the major ones

## The review's notes (v1, Marius), word for word

Offsets are UTF-16 offsets into the block's text as v1 was shared. "Done in v2" means the fixing run changed the script; v2 was never shared.

| ID | Block (place) | On | Note | State |
|---|---|---|---|---|
| F1 | `pbd55a3f2` 927–995 (2. Neon 1.0 ¶5) | "who had booked his flight to Miami became a beef that lasted months." | better to write something like: "who had spent a lot of time helping and putting him on, was now his nemesis... | done in v2 |
| F2 | `pdd4061f1` 0–171 (3. The Most Hated Streamer ¶1) | "Logan Paul, who met him through Adin, has a name for who N3on was back then: Neon 1.0. The character never switched off, and the people it hit hardest were other creators." | So this is not really relevant and makes no sense / I don't like how you always have to mention something about a creator saying something. We can also say something, doesn't have to be a celebrity saying something. You do this throughout the script, it's okay to do sometimes, but you are doing it constantly. / Also if he was n3on 1.0 how does it make sense that the character never switched off" If there's a 1.0 that means there's also newer versions, which means it did swithc off? | done in v2 |
| F3 | `pe4dce0f7` 759–791 (3. ¶2) | "The shock factor. He told Steiny" | Here again. "The shock factor." just doesn't fit. And then out of nowhere "he told steiny" which has no relevance to the clip since it's from the breakfast club. What would be better to say is just: "N3on later said.." / YOU DONT HAVE TO PUT A NAME ON EVERYTHING. PLEASE UNDERSTAND THIS | done in v2 |
| F4 | `pdd4061f1:heading` 3–26 (3. The Most Hated Streamer) | "The Most Hated Streamer" | Something general about this section: / It would be really good not only to focus on his bad things he said to creators, but also go through some of the controversies and stuff he said to randoms. Like he was extremely obnoxious and rude | done in v2 |
| F5 | `pde0824d5` 0–0, an insertion before "Just a few years ago, N3on was considere…" (Intro ¶1) | (insert) | Suggests "(1:44:14-1:44:24) + (1:44:12-1:44:14) ". The reason i'm adding these clips is to give the illusion that n3on is connected deep into hollywood. notice how I play the first clip that is actually further in the video than the 2nd clip. So i'm manipulating the clip, so it looks like after he says that stuff about him having something for n3on, that he says that he doesn't want to be on camera. Even though in the actual video he says that right when he meets them | open |
| F6 | `p8f81d1ff` 25–30 (Intro ¶7) | "West," | Right after kanye west here I want you to find a 5-8 second clip of him introducing kanye or something with him alongside kanye west | open |
| F7 | `p8f81d1ff` 66–73 (Intro ¶7) | "Azalea," | again here after azalea I want you to find a clip of n3on and Iggy together, where she praises n3on in some capacity | open |
| F8 | `p8f81d1ff` 211–215 (Intro ¶7) | "Fox." | obviously here we want to play the Steiny podcast clip of n3on saying he is going to collab with megan fox | open |

No note has replies.

## Patterns: teach, code, check

The counts below were measured on v1 with `commentary::measure` (`GET /api/scripts/<id>/audit`).

### 1. Attributions out of the blue (T1, F2, F3)
- **Cause.** The guide asked for credit: "Credit the primary source when it matters: the person's own words ("he told one podcast host")". Teaching round 1 removed credits to other creators' video essays, but not credits for interviews, so the writer named the show or the host instead. v1 has 17 named attributions in 4,724 words, 3.6 per 1,000, e.g. "He told Michael Porter Jr." ×5, "told Steiny" ×4, "On Steiny's podcast", "on Logan Paul's podcast".
- **Teach.** `guides/commentary.md` Voice gets SAY IT OURSELVES, quoting T1 and F3's "YOU DONT HAVE TO PUT A NAME ON EVERYTHING." OWN THE RESEARCH now credits a primary source "only when it matters" and drops the "he told one podcast host" example.
- **Check.** `commentary::attributions` counts "according to <Name>", "on <Name>'s podcast/stream/show/…", "told/telling <Name>", "said to <Name>" and "interview/sat/sitting with <Name>". The audit flags more than 2 per 1,000 words of narration, with a floor of 4. MrBeast holdouts measure 0.7–0.9 per 1,000, N3on v1 3.6.

### 2. Recitals: "and then, and then, and then" (T2; earlier K4)
- **Cause.** Teaching rounds 1–3 cut the literal "Then" openers (v1 has 0). The recital stayed in the structure: "He… He… He…" runs, and events listed in order.
- **Teach.** NO RECITALS, with T2's words: join events by why, keep only what makes the point, vary how sentences open.
- **Check.** The audit counts sequence openers (Then, Later, A week later, Two weeks later…), "and then" anywhere, and runs of three or more sentences opening on the same name or pronoun. It flags 6 or more openers, 3 or more "and then", or 2 or more runs. The story editor (Opus) gets a sixth kind, A RECITAL, for the recitals no count can see.

### 3. A clip after every sentence (T2; earlier K8)
- **Cause.** The writer's prompt said "with clips from many sources backing every point". Channel pacing said "a line or two of narration (about 10 seconds) leads into a clip", which forces a clip after every line or two.
- **Teach.**
  - Clips: A CLIP ONLY WHEN IT'S ESSENTIAL, with T2's words.
  - The guide's first lines and WRITER_HEAD now say "where hearing or seeing them is the point".
  - Channel pacing keeps its clip-led measure but says not every line leads into a clip.
- **Check.** The share of sentences that lead straight into a clip. It flags over 10% for documentary pacing (v1: 8%; the Doc-reviewed draft: 12%) and over 20% for channel pacing (the MrBeast holdouts: 18–19%). The documentary check on paragraphs ending in a clip (over 30%) stays.

### 4. Quote pop-ups instead of clips (T2; earlier K7)
- **Code.** A new element on a paragraph, `quotes`: the words as the narration says them (`on`), who said them, and the source, recording and times, checked like a clip's quote. Writer refuses a pop-up whose words the source doesn't hold, or that the narration doesn't say.
  - It is stored as a note line, "Quote pop-up (typewriter): “…” (Who, Video m:ss-m:ss)", with a mark carrying the card (`commentary::Mark.quote`).
  - **Hand-off.** Edit takes commentaries through their Google Doc, where Doc comments become notes. The pop-up goes there as an `edit:` comment on the quoted words, which Edit's agent treats as an instruction. On the review page it is `editMarks[].popup`.
- **Teach.** QUOTE IT INSTEAD in Clips; THEIR OWN WORDS ON SCREEN points to it.
- **Check.** The existing check, a narration that reports speech eight or more times with no quote on screen, now counts real pop-ups. v1 reports speech 66 times and has 0 pop-ups.

### 5. Notes marked on the whole paragraph (T2; earlier K9/F25 "only highlight the sentence")
- **Cause.** A paragraph's note was one string. The Doc put it on the first narration piece, Writer's Script view lit the first narration, the review page drew the block, and a fix that added an editor note banded the whole block. The writer already tried to say where ("Find, under "streamed with Kanye West": …"), but nothing could carry it.
- **Code.**
  - Notes are written as `notes: [{on, text}]`, and each line keeps a mark on its words (`Paragraph.marks`).
  - The Doc comments, Writer's yellow and the review page's `editMarks` sit on those words (drawn by feedback/ in 8d95670: only the marked words light, a pop-up's card shows its words, who and where; a line without a mark, or an older version, lights the whole paragraph; it ships with the next feedback deploy).
  - A reviewer's "for the editor" note keeps the reviewer's words.
  - A fix's band covers only the note's words.
- **Teach.** NOTES ON THEIR WORDS in Editor notes, with T2's words.
- **Check.** The audit flags notes on no words, or on more than 45 words or 80% of a paragraph of 25 words or more. v1 has 44 such notes, all of them, since it predates marks.

### Before sharing
The Review dialog lists everything the checks flag above Share ("Share anyway"), the way Edit's audit runs before an export. Every share also logs them, so a share by the fixing computer is seen too.

## One-offs (this script's facts, not rules)
- F1: Adin's role ("who had spent a lot of time helping and putting him on, was now his nemesis").
- F2: the "Neon 1.0" framing contradicts itself.
- F4: add his controversies with strangers, not only with creators.
- F5–F8: the reviewer's own intro clips: the 1:44:14 / 1:44:12 reorder, him with Kanye, Iggy praising him, and the Steiny Megan Fox clip. These are production choices; teach-1's "reviewer's choices" source is how Writer carries them.

## Open questions for the user
- Channel pacing is clip-led by measurement (30–35% narration), while T2 asks for clips "only if it is freaking mandatory". I kept channel pacing and made the clip check looser for it (20%). Should the team's note change the channel's own pacing too?
- v2 (the four fixes) was never shared. Share it, or rebuild from the taught writer?
