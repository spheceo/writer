import type { Clip, Word } from "./types";

const MAX_CHARS = 42,
  MAX_WORDS = 8,
  PAUSE_MS = 700,
  HOLD_MS = 400;

/**
 * The transcript as subtitle lines a viewer can read at a glance. A line ends
 * at a pause, a change of speaker, the end of a sentence, or when it is full.
 */
export function captionCues(words: Word[]) {
  const cues: Word[][] = [];
  let chars = 0;
  for (const word of words) {
    const cue = cues.at(-1);
    const last = cue?.at(-1);
    if (
      !cue ||
      !last ||
      last.speaker !== word.speaker ||
      word.startMs - last.endMs > PAUSE_MS ||
      /[.!?]["')\]]*$/.test(last.text) ||
      cue.length >= MAX_WORDS ||
      chars + word.text.length + 1 > MAX_CHARS
    ) {
      cues.push([word]);
      chars = word.text.length;
    } else {
      cue.push(word);
      chars += word.text.length + 1;
    }
  }
  return cues;
}

/**
 * The line to show at a moment of the footage, or null between lines. Words
 * the clip cuts away are left out, so the subtitle never shows unheard speech.
 */
export function captionAt(
  cues: Word[][],
  sourceMs: number,
  clip: Clip,
  corrections: Record<string, string>,
) {
  let lo = 0,
    hi = cues.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (cues[mid][0].startMs <= sourceMs) lo = mid + 1;
    else hi = mid;
  }
  const cue = cues[lo - 1];
  if (!cue || sourceMs > (cue.at(-1)?.endMs ?? 0) + HOLD_MS) return null;
  const heard = cue.filter(
    (w) => w.endMs > clip.startMs && w.startMs < clip.endMs,
  );
  return heard.length
    ? heard.map((w) => corrections[w.id] ?? w.text).join(" ")
    : null;
}
