import { useSyncExternalStore } from "react";
import { isCutIn } from "./clips";
import type { Clip } from "./types";

/**
 * Where the preview's playhead is, held outside React state as Repack holds
 * it: the player reports every frame, and only what shows the playhead
 * follows it, each by subscribing. `ms` is on the edited clock; `sourceMs` is
 * the footage's own clock, for the transcript's live word.
 */
export class Playhead {
  ms = 0;
  sourceMs = 0;
  playing = false;
  /** Playing the footage straight through rather than the cut. */
  auditioning = false;
  private listeners = new Set<() => void>();
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  set(ms: number, sourceMs: number, playing: boolean, auditioning: boolean) {
    if (ms === this.ms && sourceMs === this.sourceMs && playing === this.playing && auditioning === this.auditioning) return;
    this.ms = ms;
    this.sourceMs = sourceMs;
    this.playing = playing;
    this.auditioning = auditioning;
    for (const listener of this.listeners) listener();
  }
}

/** Something read from the playhead, redrawing the caller only when it changes. */
export function usePlayheadValue<T>(playhead: Playhead, read: (playhead: Playhead) => T) {
  return useSyncExternalStore(playhead.subscribe, () => read(playhead), () => read(playhead));
}

export const usePlaying = (playhead: Playhead) => usePlayheadValue(playhead, (p) => p.playing);

/** Where each item starts on the edited clock, with the total last. */
export function clipStarts(clips: Clip[]) {
  const starts = [0];
  for (const clip of clips) starts.push(starts[starts.length - 1] + (clip.endMs - clip.startMs));
  return starts;
}

export const editedDuration = (clips: Clip[]) => clips.reduce((sum, clip) => sum + (clip.endMs - clip.startMs), 0);

/** The item at an edited moment, and where in it: footage on its own clock, a card from its start. */
export function editedPosition(clips: Clip[], ms: number) {
  let at = 0;
  for (let index = 0; index < clips.length; index++) {
    const clip = clips[index];
    const length = clip.endMs - clip.startMs;
    if (ms < at + length || index === clips.length - 1) {
      return { index, clip, sourceMs: clip.startMs + Math.max(0, Math.min(length, ms - at)) };
    }
    at += length;
  }
  return null;
}

/** The edited moment a footage moment plays at, if the cut plays it. */
export function editedAtSource(clips: Clip[], sourceMs: number) {
  let at = 0;
  for (const clip of clips) {
    if (!isCutIn(clip) && sourceMs >= clip.startMs && sourceMs < clip.endMs) return at + sourceMs - clip.startMs;
    at += clip.endMs - clip.startMs;
  }
  return null;
}

/** An item of a cut. A commentary's clips each name the recording they play from. */
export type Item = Clip & { src?: string };

/**
 * Plays the cut from one local video: footage clips in order, each cut-in as
 * a card held for its length while the picture waits. A frame loop, not the
 * video's own time events, decides where a clip ends, so cuts land within a
 * frame. Auditioning plays the footage straight through instead.
 */
export class CutPlayer {
  private clips: Item[];
  private starts: number[];
  /** The recording the picture has loaded, for a cut that plays from several. */
  private loaded: string | null = null;
  /** Waiting on a recording to load: where it should start, and whether to play. */
  private pending: { index: number; ms: number } | null = null;
  private frame = 0;
  private running = false;
  private index = 0;
  /** For a card: when it began, on the page's clock, and where on the edited clock. */
  private cardFrom = 0;
  private cardAt = 0;
  private offset = 0;
  private rate = 1;
  private auditioning = false;

  constructor(
    private video: HTMLVideoElement | null,
    clips: Item[],
    private playhead: Playhead,
  ) {
    this.clips = clips;
    this.starts = clipStarts(clips);
  }

  get duration() {
    return this.starts[this.starts.length - 1];
  }

  get playing() {
    return this.running;
  }

  /** The item showing now, or null over nothing. */
  get current() {
    return this.auditioning ? null : (this.clips[this.index] ?? null);
  }

  attach(video: HTMLVideoElement | null) {
    if (this.video === video) return;
    this.pause();
    this.video = video;
    this.loaded = null;
    if (video) video.playbackRate = this.rate;
    this.seek(this.offset);
  }

  update(clips: Item[]) {
    const ms = this.position();
    const was = this.running;
    this.pause();
    this.clips = clips;
    this.starts = clipStarts(clips);
    this.seek(Math.min(ms, this.duration));
    if (was && !this.auditioning) void this.play();
  }

  setRate(rate: number) {
    this.rate = rate;
    if (this.video) this.video.playbackRate = rate;
    if (this.running && this.isCard()) {
      this.cardAt = this.position();
      this.cardFrom = performance.now();
    }
  }

  setVolume(level: number) {
    if (!this.video) return;
    this.video.volume = Math.max(0, Math.min(1, level));
    this.video.muted = level <= 0;
  }

  private isCard() {
    const clip = this.clips[this.index];
    return !!clip && isCutIn(clip);
  }

  /** Where the cut is now, on the edited clock. */
  position() {
    if (this.auditioning) return this.offset;
    const clip = this.clips[this.index];
    if (!clip) return this.offset;
    if (this.pending) return this.pending.ms;
    if (isCutIn(clip)) {
      if (!this.running) return this.offset;
      return Math.min(this.starts[this.index + 1], this.cardAt + (performance.now() - this.cardFrom) * this.rate);
    }
    const source = (this.video?.currentTime ?? 0) * 1000;
    return this.starts[this.index] + Math.max(0, Math.min(clip.endMs - clip.startMs, source - clip.startMs));
  }

  private report() {
    const ms = this.position();
    const sourceMs = this.auditioning
      ? (this.video?.currentTime ?? 0) * 1000
      : (editedPosition(this.clips, ms)?.sourceMs ?? 0);
    if (this.auditioning) {
      const edited = editedAtSource(this.clips, sourceMs);
      if (edited !== null) this.offset = edited;
    }
    this.playhead.set(this.auditioning ? this.offset : ms, sourceMs, this.running, this.auditioning);
  }

  /** Show item `index` from `ms` on the edited clock. */
  private enter(index: number, ms: number) {
    this.index = index;
    const clip = this.clips[index];
    if (!clip) return;
    if (isCutIn(clip)) {
      this.video?.pause();
      this.cardAt = ms;
      this.cardFrom = performance.now();
      return;
    }
    const source = clip.startMs + (ms - this.starts[index]);
    const video = this.video;
    if (!video) return;
    if (this.load(clip, index, ms)) return;
    // Footage that carries straight on from the last clip needs no seek.
    if (Math.abs(video.currentTime * 1000 - source) > 60) video.currentTime = source / 1000;
    if (this.running && video.paused) void video.play().catch(() => {});
  }

  /**
   * Loads the recording an item plays from, when it is not the one loaded,
   * and shows the item once it is in. Says whether it had to.
   */
  private load(clip: Item, index: number, ms: number) {
    const video = this.video;
    if (!video || !clip.src || clip.src === this.loaded) return false;
    this.loaded = clip.src;
    this.pending = { index, ms };
    video.pause();
    video.src = clip.src;
    const ready = () => {
      if (this.pending?.index !== index || this.loaded !== clip.src) return;
      this.pending = null;
      video.currentTime = (clip.startMs + (ms - this.starts[index])) / 1000;
      if (this.running) void video.play().catch(() => {});
    };
    video.addEventListener("loadedmetadata", ready, { once: true });
    return true;
  }

  seek(ms: number) {
    const was = this.running;
    this.stopLoop();
    this.video?.pause();
    this.auditioning = false;
    this.offset = Math.max(0, Math.min(this.duration, ms));
    const at = editedPosition(this.clips, this.offset);
    this.pending = null;
    if (at) {
      this.index = at.index;
      if (!isCutIn(at.clip) && this.video && !this.load(at.clip, at.index, this.offset)) this.video.currentTime = at.sourceMs / 1000;
    }
    this.report();
    if (was) void this.play();
  }

  async play(from?: number) {
    if (!this.clips.length) return;
    this.auditioning = false;
    let start = from ?? this.position();
    if (start >= this.duration - 30) start = 0;
    this.offset = start;
    const at = editedPosition(this.clips, start);
    if (!at) return;
    this.pending = null;
    this.running = true;
    this.enter(at.index, start);
    this.loop();
  }

  /** Plays the footage from a moment, outside the cut. */
  audition(sourceMs: number) {
    this.stopLoop();
    this.auditioning = true;
    const video = this.video;
    if (!video) return;
    video.currentTime = sourceMs / 1000;
    this.running = true;
    void video.play().catch(() => {});
    this.loop();
  }

  /** Shows a footage moment without playing, as clicking a transcript word does. */
  cue(sourceMs: number) {
    this.stopLoop();
    this.video?.pause();
    const video = this.video;
    if (video) video.currentTime = sourceMs / 1000;
    const edited = editedAtSource(this.clips, sourceMs);
    if (edited !== null) {
      this.auditioning = false;
      this.offset = edited;
      this.index = editedPosition(this.clips, edited)?.index ?? 0;
    } else this.auditioning = true;
    this.playhead.set(this.offset, sourceMs, false, this.auditioning);
  }

  pause() {
    if (!this.running) return;
    this.offset = this.position();
    this.stopLoop();
    this.video?.pause();
    this.report();
  }

  toggle() {
    if (this.running) this.pause();
    else if (this.auditioning) this.audition(this.playhead.sourceMs);
    else void this.play();
  }

  private stopLoop() {
    this.running = false;
    cancelAnimationFrame(this.frame);
  }

  private loop() {
    cancelAnimationFrame(this.frame);
    const tick = () => {
      if (!this.running) return;
      if (this.pending) {
        this.report();
        this.frame = requestAnimationFrame(tick);
        return;
      }
      if (!this.auditioning) {
        const clip = this.clips[this.index];
        const ms = this.position();
        if (!clip) {
          this.finish();
          return;
        }
        const end = this.starts[this.index + 1];
        // A frame early, so the last frame of a clip is the last one shown.
        if (ms >= end - (isCutIn(clip) ? 0 : 16)) {
          if (this.index + 1 >= this.clips.length) {
            this.finish();
            return;
          }
          this.enter(this.index + 1, end);
        }
      } else if (this.video?.ended) {
        this.pause();
        return;
      }
      this.report();
      this.frame = requestAnimationFrame(tick);
    };
    this.frame = requestAnimationFrame(tick);
  }

  private finish() {
    this.stopLoop();
    this.video?.pause();
    this.offset = this.duration;
    this.index = Math.max(0, this.clips.length - 1);
    this.report();
  }

  dispose() {
    this.stopLoop();
    this.video?.pause();
  }
}
