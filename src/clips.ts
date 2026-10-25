import type { Clip, Part, Script, Word } from "./types";

export function clock(ms: number) {
  const total = Math.max(0, Math.round(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (value: number) => String(value).padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`;
}

export const isCutIn = (clip: Clip) => clip.kind === "cut-in";

/** Recording boundaries inside a footage clip, even when the sequence keeps it joined. */
export function recordingSpans(clip: Clip, parts?: Part[]) {
  if (isCutIn(clip) || !parts || parts.length < 2) return [];
  return parts.flatMap((part, index) => {
    const startMs = Math.max(clip.startMs, part.startMs);
    const endMs = Math.min(clip.endMs, part.startMs + part.durationMs);
    return endMs > startMs ? [{ index, title: part.title, startMs, endMs }] : [];
  });
}

/** Length of the edited video: every footage range and every cut-in card. */
export function keptMs(clips: Clip[]) {
  return clips.reduce((sum, clip) => sum + (clip.endMs - clip.startMs), 0);
}

/** Split the footage clip under `at` (source milliseconds). Edges closer than 100 ms stay put. */
export function splitAt(clips: Clip[], at: number): Clip[] {
  const ms = Math.round(at);
  return clips.flatMap((clip) => {
    if (isCutIn(clip) || ms < clip.startMs + 100 || ms > clip.endMs - 100) return [clip];
    return [
      { ...clip, endMs: ms },
      { id: crypto.randomUUID(), startMs: ms, endMs: clip.endMs },
    ];
  });
}

/** Remove a source range from every footage clip, keeping whatever sits outside it. */
export function lift(clips: Clip[], startMs: number, endMs: number): Clip[] {
  if (endMs <= startMs) return clips;
  return clips.flatMap((clip) => {
    if (isCutIn(clip) || clip.endMs <= startMs || clip.startMs >= endMs) return [clip];
    const parts: Clip[] = [];
    if (clip.startMs < startMs) parts.push({ ...clip, endMs: startMs });
    if (clip.endMs > endMs) {
      // A second piece is a new timestamp: the comments stay on the first.
      parts.push(parts.length ? { ...clip, id: crypto.randomUUID(), startMs: endMs, notes: undefined } : { ...clip, startMs: endMs });
    }
    return parts;
  });
}

export function covers(clips: Clip[], ms: number) {
  return clips.some((clip) => !isCutIn(clip) && ms >= clip.startMs && ms < clip.endMs);
}

export type Block = { clips: { clip: Clip; index: number }[]; cue: { clip: Clip; index: number } | null };

/** Footage runs, each ended by the cut-in that follows it, as Repack groups a script. */
export function blocks(clips: Clip[]): Block[] {
  const out: Block[] = [{ clips: [], cue: null }];
  clips.forEach((clip, index) => {
    const open = out[out.length - 1];
    if (isCutIn(clip)) {
      open.cue = { clip, index };
      out.push({ clips: [], cue: null });
    } else {
      open.clips.push({ clip, index });
    }
  });
  const last = out[out.length - 1];
  if (!last.clips.length && !last.cue) out.pop();
  return out;
}

/** What a cut-in is for, read from where it sits. */
export function purpose(index: number, count: number) {
  if (index === 0) return "Intro";
  if (index === count - 1) return "Conclusion";
  if (index === 1) return "Case opening";
  return "Story";
}

const LABELS: Record<string, string> = {
  intro: "Intro",
  hook: "Hook",
  "title-line": "Title line",
  transition: "Transition",
  "case-opening": "Case opening",
  "intro-and-case-opening": "Intro and case opening",
  "story-development": "Story",
  conclusion: "Outro",
  outro: "Outro",
};

/** Whether at least half of a clip plays again among later clips. */
function replayed(clip: Clip, later: Clip[]) {
  const spans = later
    .map((other) => [Math.max(other.startMs, clip.startMs), Math.min(other.endMs, clip.endMs)] as const)
    .filter(([from, to]) => to > from)
    .sort((a, b) => a[0] - b[0]);
  let total = 0;
  let reached = clip.startMs;
  for (const [from, to] of spans) {
    total += Math.max(0, to - Math.max(from, reached));
    reached = Math.max(reached, to);
  }
  return total * 2 >= clip.endMs - clip.startMs;
}

/**
 * Every cut-in's purpose, by format, as Repack reads it from where each sits.
 * A FOIA opening is read from the layout: two-split when the second block's
 * clips all play again later.
 */
export function purposes(script: Pick<Script, "clips" | "kind" | "compilation" | "title">): string[] {
  const parts = blocks(script.clips);
  const cues = parts.filter((block) => block.cue);
  const bare = cues.map((block) => block.clips.length === 0);
  const count = cues.length;
  const when = /^\s*["“'‘]?when\b/i.test(script.title);
  const kind = script.kind ?? "interrogation";
  const twoSplit =
    kind === "foia" &&
    parts.length >= 3 &&
    parts[1].clips.length > 0 &&
    parts[1].clips.every(({ clip }) => replayed(clip, parts.slice(2).flatMap((block) => block.clips.map((item) => item.clip))));
  const names = bare.map((isBare, index) => {
    if (script.compilation) {
      if (isBare) return "transition";
      if (index === 0) return when ? "hook" : "intro";
      if (index === 1 && !bare[0]) return when ? "title-line" : "case-opening";
      if (bare[index - 1]) return "case-opening";
      return index === count - 1 || bare[index + 1] ? "outro" : "story-development";
    }
    if (index === count - 1 && index > 0) return "conclusion";
    if (kind === "courtroom") return index === 0 ? "intro-and-case-opening" : "story-development";
    if (kind === "foia") {
      if (index === 0) return twoSplit ? "intro" : "case-opening";
      if (index === 1 && twoSplit) return "case-opening";
      return "story-development";
    }
    return index === 0 ? "intro" : index === 1 ? "case-opening" : "story-development";
  });
  return names.map((name) => LABELS[name] ?? name);
}

/** The script's title line, as Repack heads its Docs: the producer's own wording when they rewrote it. */
export function heading(script: Pick<Script, "kind" | "title" | "suspects" | "compilation" | "heading">) {
  return script.heading?.trim() || generatedHeading(script);
}

/** The title line built from what is known: the series, who is interrogated, then the title. */
export function generatedHeading(script: Pick<Script, "kind" | "title" | "suspects" | "compilation">) {
  const series = script.kind === "foia" ? "FOIA" : script.kind === "courtroom" ? "COURT" : "INTERRO";
  const suspects = script.compilation ? [] : (script.suspects ?? []);
  return suspects.length ? `${series} ${suspects.join(" & ")} - ${script.title}` : `${series} ${script.title}`;
}

/** Which recording of a joined case a moment is in, or -1 for a single recording. */
export function partAt(parts: Part[] | undefined, ms: number) {
  if (!parts || parts.length < 2) return -1;
  const index = parts.findIndex((part) => ms < part.startMs + part.durationMs);
  return index < 0 ? parts.length - 1 : index;
}

/** A moment as the script prints it, Repack's way: whole seconds, rounded down. */
export function stamp(ms: number) {
  const seconds = Math.floor(Math.max(0, ms) / 1000);
  const minutes = Math.floor(seconds / 60);
  const pad = (value: number) => String(value).padStart(2, "0");
  return minutes >= 60 ? `${Math.floor(minutes / 60)}:${pad(minutes % 60)}:${pad(seconds % 60)}` : `${minutes}:${pad(seconds % 60)}`;
}

/**
 * A clip's range as the script prints it. In a joined case it reads on its
 * own recording's clock, as Repack prints it, so it can be found in that file.
 */
export function range(clip: Pick<Clip, "startMs" | "endMs">, parts?: Part[], dash = "–") {
  const index = partAt(parts, clip.startMs);
  const offset = index >= 0 && parts ? parts[index].startMs : 0;
  return `${stamp(clip.startMs - offset)}${dash}${stamp(clip.endMs - offset)}`;
}

/** A clip's range as the script prints it, marked when it is of an audio-only recording, as Repack marks it. */
export function clipRange(clip: Pick<Clip, "startMs" | "endMs">, parts?: Part[], dash = "–") {
  const index = partAt(parts, clip.startMs);
  return `${index >= 0 && parts?.[index].audio ? "Audio " : ""}${range(clip, parts, dash)}`;
}

/** How long the cut runs, as Repack states it under the assets: "38m12s", "1h2m5s". */
export function duration(ms: number) {
  const seconds = Math.round(ms / 1000);
  const minutes = Math.floor(seconds / 60);
  return `${minutes >= 60 ? `${Math.floor(minutes / 60)}h` : ""}${minutes % 60}m${seconds % 60}s`;
}

export type Line = { words: Word[]; startMs: number; endMs: number; speaker: string | null };

export function lines(words: Word[]): Line[] {
  const out: Line[] = [];
  let bucket: Word[] = [];
  const flush = () => {
    if (!bucket.length) return;
    out.push({
      words: bucket,
      startMs: bucket[0].startMs,
      endMs: bucket[bucket.length - 1].endMs,
      speaker: bucket[0].speaker ?? null,
    });
    bucket = [];
  };
  for (const word of words) {
    const previous = bucket[bucket.length - 1];
    const gap = previous ? word.startMs - previous.endMs : 0;
    const speakerChanged = bucket.length > 0 && bucket[0].speaker !== word.speaker;
    if (bucket.length && (gap > 800 || speakerChanged || bucket.length >= 14)) flush();
    bucket.push(word);
  }
  flush();
  return out;
}

/** A cut-in card as builds make them: three seconds, holding its narration. */
export const CUT_IN_MS = 3000;
export const cutInCard = (narration = ""): Clip => ({ id: crypto.randomUUID(), kind: "cut-in", startMs: 0, endMs: CUT_IN_MS, narration });

/** Splits item `index` at a moment of its footage. Edges closer than 100 ms stay put. */
export function splitClip(clips: Clip[], index: number, sourceMs: number): Clip[] {
  const clip = clips[index];
  const ms = Math.round(sourceMs);
  if (!clip || isCutIn(clip) || ms < clip.startMs + 100 || ms > clip.endMs - 100) return clips;
  return [...clips.slice(0, index), { ...clip, endMs: ms }, { id: crypto.randomUUID(), startMs: ms, endMs: clip.endMs }, ...clips.slice(index + 1)];
}

/** Breathing room kept before a clip's first word and after its last, never closer than 60 ms to another word. */
export const LEAD_MS = 150;
export const TAIL_MS = 400;

/** A word-anchored range with breathing room, as Repack's breathe gives it. */
export function breathe(words: Word[], startMs: number, endMs: number, durationMs: number) {
  let lo = 0;
  let hi = words.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (words[mid].startMs < endMs) lo = mid + 1;
    else hi = mid;
  }
  const next = words[lo];
  let prev: Word | undefined;
  for (let i = lo - 1; i >= 0; i--)
    if (words[i].endMs <= startMs) {
      prev = words[i];
      break;
    }
  return {
    startMs: Math.round(Math.min(startMs, Math.max(startMs - LEAD_MS, prev ? prev.endMs + 60 : 0))),
    endMs: Math.round(Math.max(endMs, Math.min(endMs + TAIL_MS, next ? next.startMs - 60 : durationMs))),
  };
}

/**
 * Premiere's insert edit, as Repack makes it: a footage clip under the
 * moment is split there, and the item goes in at the nearest edit point.
 */
export function insertAt(clips: Clip[], editedMs: number, item: Clip) {
  const ms = Math.round(editedMs);
  let at = 0;
  let list = clips;
  for (let index = 0; index < clips.length; index++) {
    const length = clips[index].endMs - clips[index].startMs;
    if (ms < at + length) {
      if (!isCutIn(clips[index])) list = splitClip(clips, index, clips[index].startMs + (ms - at));
      break;
    }
    at += length;
  }
  const bounds = [0];
  for (const clip of list) bounds.push(bounds[bounds.length - 1] + clip.endMs - clip.startMs);
  let best = 0;
  bounds.forEach((bound, index) => {
    if (Math.abs(bound - ms) < Math.abs(bounds[best] - ms)) best = index;
  });
  return [...list.slice(0, best), item, ...list.slice(best)];
}

/**
 * Moves items, keeping their order, to the edit point `to` (0 is before the
 * first item, the length is after the last), as dragging them there does.
 */
export function moveItems(clips: Clip[], ids: string[], to: number) {
  const moving = clips.filter((clip) => ids.includes(clip.id));
  if (!moving.length) return clips;
  const before = clips.slice(0, to).filter((clip) => !ids.includes(clip.id)).length;
  const rest = clips.filter((clip) => !ids.includes(clip.id));
  const next = [...rest.slice(0, before), ...moving, ...rest.slice(before)];
  return next.every((clip, index) => clip === clips[index]) ? clips : next;
}

/** Puts an item in after `index`, or first for -1. */
export const insertAfter = (clips: Clip[], index: number, clip: Clip) => [...clips.slice(0, index + 1), clip, ...clips.slice(index + 1)];

/** Moves one edge of a footage clip, keeping it at least 200 ms long and inside the footage. */
export function trimClip(clips: Clip[], id: string, edge: "in" | "out", sourceMs: number, durationMs: number): Clip[] {
  return clips.map((clip) => {
    if (clip.id !== id || isCutIn(clip)) return clip;
    const ms = Math.round(sourceMs);
    return edge === "in"
      ? { ...clip, startMs: Math.max(0, Math.min(ms, clip.endMs - 200)) }
      : { ...clip, endMs: Math.min(durationMs || Number.MAX_SAFE_INTEGER, Math.max(ms, clip.startMs + 200)) };
  });
}

/** The word nearest a footage moment, for edges that land between words. */
export function nearestEdge(words: Word[], ms: number, edge: "in" | "out", reach = 400) {
  let best: number | null = null;
  for (const word of words) {
    const at = edge === "in" ? word.startMs : word.endMs;
    if (Math.abs(at - ms) <= reach && (best === null || Math.abs(at - ms) < Math.abs(best - ms))) best = at;
    if (word.startMs > ms + reach) break;
  }
  return best;
}
