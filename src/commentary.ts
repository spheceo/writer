import { clock } from "./clips";
import type { Cite, Draft, NoteMark, Paragraph, Piece, Source } from "./types";

/** Narration is read at about this pace, as the app estimates it. */
export const WORDS_PER_MINUTE = 150;

export const isClip = (piece: Piece): piece is { kind: "clip" } & Cite => piece.kind === "clip";

export function cites(draft: Draft): Cite[] {
  return [
    ...draft.open,
    ...draft.sections.flatMap((section) => section.paragraphs.flatMap((paragraph) => paragraph.pieces.filter(isClip))),
  ];
}

/** Words the narrator reads. Headings go on screen. */
export function narrationWords(draft: Draft) {
  return draft.sections
    .flatMap((section) => section.paragraphs.flatMap((paragraph) => paragraph.pieces))
    .reduce((sum, piece) => sum + (piece.kind === "say" ? piece.text.split(/\s+/).filter(Boolean).length : 0), 0);
}

export function timing(draft: Draft) {
  const narrationMs = Math.round((narrationWords(draft) * 60_000) / WORDS_PER_MINUTE);
  const clipsMs = cites(draft).reduce((sum, cite) => sum + (cite.endMs - cite.startMs), 0);
  return { narrationMs, clipsMs, totalMs: narrationMs + clipsMs };
}

const minutes = (ms: number) => `${Math.round(ms / 60_000)}min`;

function minutesSeconds(ms: number) {
  const total = Math.round(ms / 1000);
  const [m, s] = [Math.floor(total / 60), total % 60];
  return m ? (s ? `${m}min ${s}s` : `${m}min`) : `${s}s`;
}

/** As the channel's scripts close: "Total time = 15min vo + 21min 3s = 36min". */
export function totalLine(draft: Draft) {
  const { narrationMs, clipsMs, totalMs } = timing(draft);
  return `Total time = ${minutes(narrationMs)} vo + ${minutesSeconds(clipsMs)} = ${minutes(totalMs)}`;
}

/** A clip as the editor reads it: its times, then the recording. */
export const cue = (cite: Cite) => `(${clock(cite.startMs)}-${clock(cite.endMs)})`;

function paragraphText(pieces: Piece[]) {
  return pieces.reduce((text, piece, index) => {
    const value = piece.kind === "say" ? piece.text.trim() : cue(piece);
    if (!value) return text;
    const separator = index && isClip(piece) && isClip(pieces[index - 1]) ? " + " : " ";
    return text ? `${text}${separator}${value}` : value;
  }, "");
}

/**
 * The script in the channel's Doc layout: the montage line, each section's
 * heading and paragraphs with their clips inline, editor notes, the total
 * time, then where every clip comes from and what to check before recording.
 */
export function commentaryDocument(_title: string, draft: Draft, _sources: Source[]) {
  const out: string[] = [];
  draft.sections.forEach((section, index) => {
    const block: string[] = [section.heading];
    if (index === 0 && draft.open.length) block.push(draft.open.map(cue).join(" + "));
    for (const paragraph of section.paragraphs) {
      block.push(paragraphText(paragraph.pieces));
      const notes = [
        paragraph.note?.trim(),
        ...paragraph.pieces.filter(isClip).flatMap((clip) => (clip.verify ? [`${cue(clip)} ${clip.video}: ${clip.verify.replace(/^Inherited edit reference from S\d+\. Speech was not transcribed in the supplied script; /, "")}`] : [])),
      ].filter(Boolean);
      for (const note of notes) block.push(`*For editor*: ${note}`);
    }
    out.push(block.join("\n"));
  });
  if (!draft.sections.length && draft.open.length) out.push(draft.open.map(cue).join(" + "));
  out.push(totalLine(draft));

  return `${out.join("\n\n")}\n`;
}

/** A character as marks are matched (commentary.rs find_words): case, curly quotes and spaces aside. */
function canon(c: string) {
  if (c === "\u2018" || c === "\u2019" || c === "\u02bc") return "'";
  if (c === "\u201c" || c === "\u201d") return '"';
  if (/\s/.test(c)) return " ";
  return c.toLowerCase();
}

/** Where `words` are in `text` ([start, end) in its UTF-16 units), matched as commentary.rs find_words matches them. */
export function findWords(text: string, words: string): [number, number] | null {
  const trimmed = words.trim().replace(/^["“”'‘’\s]+|["“”'‘’\s]+$/g, "");
  const wanted: string[] = [];
  for (const c of trimmed) {
    const k = canon(c);
    if (!(k === " " && wanted[wanted.length - 1] === " ")) wanted.push(k);
  }
  if (wanted.filter((c) => /[\p{L}\p{N}]/u.test(c)).length < 2) return null;
  const have: [number, string][] = [];
  let at = 0;
  for (const c of text) {
    const k = canon(c);
    if (!(k === " " && have[have.length - 1]?.[1] === " ")) have.push([at, k]);
    at += c.length;
  }
  const search = (want: string[]): [number, number] | null => {
    const word = (c: string | undefined) => c !== undefined && /[\p{L}\p{N}]/u.test(c);
    for (let from = 0; from + want.length <= have.length; from++) {
      // Whole words only: "lied" isn't in "applied".
      const bounded = (!word(have[from - 1]?.[1]) || !word(want[0])) && (!word(have[from + want.length]?.[1]) || !word(want[want.length - 1]));
      if (bounded && want.every((c, i) => have[from + i][1] === c)) {
        const [last] = have[from + want.length - 1];
        return [have[from][0], last + (text.codePointAt(last)! > 0xffff ? 2 : 1)];
      }
    }
    return null;
  };
  const found = search(wanted);
  if (found) return found;
  let end = wanted.length;
  while (end > 0 && !/[\p{L}\p{N}]/u.test(wanted[end - 1])) end -= 1;
  return end > 0 && end < wanted.length ? search(wanted.slice(0, end)) : null;
}

/** A paragraph's note lines, trimmed, in order. */
export const noteLines = (paragraph: Paragraph) => (paragraph.note ?? "").split("\n").map((line) => line.trim()).filter(Boolean);

/** The marks still on their words: the narration piece, its span there and the note line's index. */
export function liveMarks(paragraph: Paragraph) {
  const lines = noteLines(paragraph);
  const out: { mark: NoteMark; piece: number; start: number; end: number; line: number }[] = [];
  for (const mark of paragraph.marks ?? []) {
    const line = lines.indexOf(mark.line.trim());
    if (line < 0) continue;
    for (const [piece, item] of paragraph.pieces.entries()) {
      if (item.kind !== "say") continue;
      const span = findWords(item.text, mark.on);
      if (span) {
        out.push({ mark, piece, start: span[0], end: span[1], line });
        break;
      }
    }
  }
  return out;
}

