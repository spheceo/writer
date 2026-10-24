import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { clock } from "./clips";

/** What a hovered clip plays: its downloaded stretch and where in it, or a card when it isn't down yet. */
export type HoverClip = {
  id: string;
  title: string;
  quote: string;
  /** The file, and the clip's start and end in it (ms). Absent while it downloads, or when it can't be. */
  src?: string;
  startMs: number;
  endMs: number;
  /** The clip's own times in its recording, as the script prints them. */
  atMs: number;
  toMs: number;
  /** Shown instead of the video: "Downloading this stretch…". */
  waiting?: string;
};

const WIDTH = 360;
const GAP = 10;
/** How long a pointer rests on a time before its clip shows, and leaves before it goes. */
const SHOW_MS = 140;
const HIDE_MS = 220;

/**
 * A commentary's clip, shown where it's mentioned: hovering a time in the
 * script fades in a small player that plays that stretch of the downloaded
 * footage, looping it softly; leaving lets it fade away. Clicking pins it
 * with sound (a click is what lets a page play sound).
 */
export function useHoverPreview() {
  const [shown, setShownNow] = useState<{ clip: HoverClip; rect: DOMRect; pinned: boolean; leaving?: boolean } | null>(null);
  // Going away fades out first, so nothing vanishes in one frame.
  const fade = useRef(0);
  const setShown = (next: typeof shown | ((now: typeof shown) => typeof shown)) =>
    setShownNow((now) => {
      const value = typeof next === "function" ? next(now) : next;
      window.clearTimeout(fade.current);
      if (value === null && now && !now.leaving) {
        fade.current = window.setTimeout(() => setShownNow((later) => (later?.leaving ? null : later)), 160);
        return { ...now, leaving: true };
      }
      return value;
    });
  const timers = useRef({ show: 0, hide: 0 });
  const over = useRef(false);
  const enter = (clip: HoverClip, element: Element) => {
    window.clearTimeout(timers.current.hide);
    window.clearTimeout(timers.current.show);
    const rect = element.getBoundingClientRect();
    timers.current.show = window.setTimeout(() => setShown((now) => (now?.pinned && now.clip.id !== clip.id ? now : { clip, rect, pinned: now?.pinned && now.clip.id === clip.id ? true : false })), SHOW_MS);
  };
  const leave = () => {
    window.clearTimeout(timers.current.show);
    window.clearTimeout(timers.current.hide);
    timers.current.hide = window.setTimeout(() => !over.current && setShown((now) => (now?.pinned ? now : null)), HIDE_MS);
  };
  const pin = (clip: HoverClip, element: Element) => {
    window.clearTimeout(timers.current.show);
    setShown({ clip, rect: element.getBoundingClientRect(), pinned: true });
  };
  const close = () => setShown(null);
  const preview = shown && (
    <HoverPreview
      key={shown.clip.id}
      clip={shown.clip}
      rect={shown.rect}
      pinned={shown.pinned}
      leaving={!!shown.leaving}
      onPointer={(inside) => {
        over.current = inside;
        if (inside) window.clearTimeout(timers.current.hide);
        else leave();
      }}
      onClose={close}
    />
  );
  useEffect(() => {
    if (!shown?.pinned) return;
    const key = (event: KeyboardEvent) => event.key === "Escape" && close();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [shown?.pinned]);
  return { enter, leave, pin, close, preview };
}

function HoverPreview({ clip, rect, pinned, leaving, onPointer, onClose }: { clip: HoverClip; rect: DOMRect; pinned: boolean; leaving: boolean; onPointer: (inside: boolean) => void; onClose: () => void }) {
  const video = useRef<HTMLVideoElement>(null);
  const card = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  const [dip, setDip] = useState(false);
  const [place, setPlace] = useState<{ left: number; top: number; below: boolean }>({ left: rect.left, top: rect.bottom + GAP, below: true });

  // Beside the time it belongs to: below it when there's room, else above, kept on screen.
  useLayoutEffect(() => {
    const height = card.current?.offsetHeight ?? 260;
    const below = rect.bottom + GAP + height < window.innerHeight - 8;
    const left = Math.min(Math.max(8, rect.left + rect.width / 2 - WIDTH / 2), window.innerWidth - WIDTH - 8);
    setPlace({ left, top: below ? rect.bottom + GAP : rect.top - GAP - height, below });
  }, [rect]);

  useEffect(() => {
    const element = video.current;
    if (!element || !clip.src) return;
    element.muted = !pinned;
    if (pinned) void element.play().catch(() => {});
  }, [pinned, clip.src]);

  useEffect(() => {
    const element = video.current;
    if (!element || !clip.src) return;
    const start = clip.startMs / 1000;
    const end = Math.max(start + 0.5, clip.endMs / 1000);
    const seek = () => {
      element.currentTime = start;
      void element.play().catch(() => {});
    };
    // Round again at the end, through a short dip rather than a jump.
    const tick = () => {
      if (element.currentTime >= end - 0.25 && !dip) {
        setDip(true);
        window.setTimeout(() => {
          element.currentTime = start;
          setDip(false);
        }, 220);
      }
    };
    const playing = () => setReady(true);
    element.addEventListener("loadedmetadata", seek);
    element.addEventListener("timeupdate", tick);
    element.addEventListener("playing", playing);
    if (element.readyState >= 1) seek();
    return () => {
      element.removeEventListener("loadedmetadata", seek);
      element.removeEventListener("timeupdate", tick);
      element.removeEventListener("playing", playing);
    };
  }, [clip.src, clip.startMs, clip.endMs, dip]);

  return createPortal(
    <div
      ref={card}
      className="hover-preview"
      data-below={place.below || undefined}
      data-pinned={pinned || undefined}
      data-leaving={leaving || undefined}
      style={{ left: place.left, top: place.top, width: WIDTH }}
      onPointerEnter={() => onPointer(true)}
      onPointerLeave={() => onPointer(false)}
    >
      <div className="hover-screen" data-ready={(ready && !dip) || undefined}>
        {clip.src ? (
          <video ref={video} src={clip.src} muted={!pinned} playsInline preload="auto" />
        ) : (
          <div className="hover-waiting">{clip.waiting ?? "No footage for this clip yet."}</div>
        )}
        {clip.src && !ready && <div className="hover-loading" />}
      </div>
      <div className="hover-meta">
        <span className="hover-title">{clip.title}</span>
        <span className="hover-time">
          {clock(clip.atMs)}–{clock(clip.toMs)}
        </span>
      </div>
      {clip.quote && <p className="hover-quote">“{clip.quote}”</p>}
      {pinned && (
        <button type="button" className="hover-close" aria-label="Close the preview" onClick={onClose}>
          ×
        </button>
      )}
    </div>,
    document.body,
  );
}
