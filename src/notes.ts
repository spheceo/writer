import type { Note } from "./types";

/** An edited paragraph can move a quote; repeated words must not move a note to an arbitrary occurrence. */
export function noteRange(text: string, anchor: NonNullable<Note["anchor"]>) {
  if (!anchor.quote) return null;
  if (text.slice(anchor.start, anchor.end) === anchor.quote) return { start: anchor.start, end: anchor.end };
  const start = text.indexOf(anchor.quote);
  return start >= 0 && text.indexOf(anchor.quote, start + 1) < 0 ? { start, end: start + anchor.quote.length } : null;
}
