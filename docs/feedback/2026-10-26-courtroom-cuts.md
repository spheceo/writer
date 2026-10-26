# Feedback: Courtroom cut precision and captioned footage, 26 October 2026

Status: tested — needs work (product fix verified on two scripts; no training round needed for the cut itself)

## Sources and examples

The producer, watching the first two scripts built from producer briefs on YouTube captions (`captions.rs`, `turns.rs`):

> I think we need to be more precise in the way that we cut, ensuring that we start at the intended word and end at the intended word. The first clip as well basically has no audio at all. I don't know how we let that through, especially for the intro.

- "CORRUPT Jail Guard Caught Smuggling Dr*gs Into Jail — Judge DESTROYS Her" (`d18e5ff5-7806-4c0c-9ae8-6effb5517f90`), review link https://feedback.spheceo.com/r/kHzy0wIoRnHio0vLRNyrBQ: v1 (revision 2) reviewed; v2 (revision 4) shared with the fix, then made the link's only version, v1, at the producer's ask.
- "Cop Used 15-Year-Old In Sadistic S*x Ring, Then She's Found Dead" (`1dae0ba4-5ddc-4008-ae23-1513ff6cdc4f`), review link https://feedback.spheceo.com/r/fWTzt4Co22WO4NRxo3SMsQ: v1 (revision 3) reviewed; v2 (revision 5) shared with the fix, then made the link's only version, v1, at the producer's ask. Its intro's first piece was a lone "The" (0:14:16.3–0:14:17.0), 0.66 s.

## Consolidated lessons

Product work, not model behaviour: the agent chose the right words both times.

- **Slivers.** The intro clip ran "The investigating officer is trying to present a lie… of William Farwell." Intro tightening cut the 0.9 s pause after "The" and left a 0.5–0.7 s piece holding one word: a blip, shown on the review page as a near-empty first clip. Now a piece under 1.2 s left by a cut pause joins its neighbour, keeping the pause (`tidy::join_slivers`); clips the plan chose apart never join.
- **Word ends on captions.** YouTube times each word's start only; its end was estimated from its length, so a clip could end inside or after its last word. Once a captioned build has chosen its clips, Scribe hears just those stretches (about 25 minutes of a 9-hour hearing: 3 s either side, pieces of one request each), every word there takes Scribe's times and spelling, Sonnet re-reads who says each line with Scribe's voice changes, and the same plan is cut again (`exact.rs`, `cut_exactly` in lib.rs). A revision hears only footage it newly plays. Footage cleared from the Mac is brought back first.
- **No sound.** A footage piece with nothing above a fifth of the speech level of the minute around it is dropped (`exact::silent`).
- An early diagnosis blamed caption drift; it was a faulty loudness measurement (an `astats` filter chain read 1–3 s off). Measured from raw samples, the captions' starts were right there.

## Verification

- Every clip edge of both v2 cuts checked against the sound and the neighbouring words: each clip starts 0–0.5 s before its first word and ends 0–0.5 s after its last, never into another word; where sound sits at an edge, it's the next speaker starting 40–80 ms later.
- Scribe on the chosen footage: script 1, 26:31 in 19 stretches (4,239 words retimed, 110 respelled, 93 to another speaker); script 2, 24:47 in 18 (3,761, 87, 66). Who speaks agrees with a hand check on 98.5% and 98.2% of the cut's words; the rest are one-word answers and crosstalk, a few of them still wrong (Young's "did you not acknowledge…" given to the judge).

## Next

The producer's review of the fixed cuts (now v1 on both links). Speaker labels in crosstalk remain the weak spot.
