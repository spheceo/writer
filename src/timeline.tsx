import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { clock, cutInCard, insertAfter, isCutIn, nearestEdge, range, splitClip, trimClip } from "./clips";
import { type CutPlayer, clipStarts, editedPosition, type Playhead } from "./playback";
import type { Clip, Part, Word } from "./types";
import { Icon } from "./ui";

/** Left inset, so the playhead at 0:00 is not flush with the edge. */
const INSET = 14;
/** Closest zoom: 600 px a second. */
const MAX_PPMS = 0.6;
const SNAP_PX = 10;
const RULER_STEPS = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200];
const FOOTAGE = "#7088dc";
const CARD = "#a27ce0";

type Drag =
  | { mode: "scrub" }
  | { mode: "trim"; id: string; edge: "in" | "out"; x: number; clip: Clip; ms: number; snapped: boolean }
  | { mode: "bar"; x: number; scroll: number };

/**
 * The sequence, as Repack draws it: a time ruler, the picture track (footage
 * and cut-in cards) and the sound track under one playhead. Drag the ruler to
 * scrub; drag a clip's edge to trim it, snapping to the words.
 */
export function Timeline({
  clips,
  words,
  overview,
  durationMs,
  parts,
  labels,
  playhead,
  player,
  selected,
  disabled,
  onSelect,
  onEdit,
}: {
  clips: Clip[];
  words: Word[];
  /** Loudness across the whole footage, evenly spaced. */
  overview: number[];
  durationMs: number;
  parts?: Part[];
  /** Each cut-in's purpose, in order. */
  labels: string[];
  playhead: Playhead;
  player: CutPlayer | null;
  selected: string[];
  disabled: boolean;
  onSelect: (ids: string[]) => void;
  onEdit: (clips: Clip[]) => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const ruler = useRef<HTMLCanvasElement>(null);
  const wave = useRef<HTMLCanvasElement>(null);
  const line = useRef<HTMLSpanElement>(null);
  const [view, setView] = useState({ width: 800, scroll: 0 });
  const [zoom, setZoom] = useState<number | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);

  const starts = useMemo(() => clipStarts(clips), [clips]);
  const total = starts[starts.length - 1];
  const fit = (view.width - INSET * 2) / Math.max(total, 1000);
  const ppms = Math.max(fit, Math.min(MAX_PPMS, zoom ?? fit));
  const x = (ms: number) => INSET + ms * ppms;
  const content = x(total) + INSET;
  const msAt = (clientX: number) => {
    const box = scroller.current?.getBoundingClientRect();
    if (!box || !scroller.current) return 0;
    return Math.max(0, Math.min(total, (clientX - box.left + scroller.current.scrollLeft - INSET) / ppms));
  };

  useEffect(() => {
    const node = scroller.current;
    if (!node) return;
    const measure = () => setView({ width: node.clientWidth, scroll: node.scrollLeft });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  // The ruler and the sound are drawn for the stretch in view only.
  useLayoutEffect(() => {
    const ratio = window.devicePixelRatio || 1;
    const prepare = (canvas: HTMLCanvasElement | null) => {
      if (!canvas) return null;
      const width = view.width;
      const height = canvas.clientHeight;
      canvas.width = Math.floor(width * ratio);
      canvas.height = Math.floor(height * ratio);
      const pen = canvas.getContext("2d");
      if (!pen) return null;
      pen.scale(ratio, ratio);
      pen.clearRect(0, 0, width, height);
      return { pen, width, height };
    };
    const from = (view.scroll - INSET) / ppms;
    const to = (view.scroll + view.width - INSET) / ppms;
    const top = prepare(ruler.current);
    if (top) {
      const { pen, height } = top;
      const step = RULER_STEPS.find((seconds) => seconds * 1000 * ppms >= 72) ?? 7200;
      const minor = step >= 5 ? step / 5 : step / 2;
      pen.font = "10.5px ui-monospace, SF Mono, Menlo, monospace";
      pen.textBaseline = "top";
      for (let s = Math.max(0, Math.floor(from / 1000 / minor) * minor); s * 1000 <= Math.min(to, total); s += minor) {
        const px = x(s * 1000) - view.scroll;
        const major = Math.abs(s / step - Math.round(s / step)) < 1e-6;
        pen.fillStyle = major ? "#4a4a4a" : "#262626";
        pen.fillRect(Math.round(px), major ? height - 10 : height - 5, 1, major ? 10 : 5);
        if (major) {
          pen.fillStyle = "#7a7a7a";
          // The playhead's marker sits over 0:00, so its label steps aside.
          pen.fillText(clock(s * 1000), Math.round(px) + (s === 0 ? 10 : 4), 6);
        }
      }
    }
    const bottom = prepare(wave.current);
    if (bottom && overview.length && durationMs > 0) {
      const { pen, width, height } = bottom;
      for (let px = 0; px < width; px += 2) {
        const ms = (view.scroll + px - INSET) / ppms;
        if (ms < 0 || ms > total) continue;
        const at = editedPosition(clips, ms);
        if (!at || isCutIn(at.clip)) continue;
        const peak = overview[Math.min(overview.length - 1, Math.floor((at.sourceMs / durationMs) * overview.length))] ?? 0;
        const bar = Math.max(1.5, peak * (height - 12));
        pen.fillStyle = selected.includes(at.clip.id) ? "#c9d3ff" : "#9fb0ec";
        pen.fillRect(px, (height - bar) / 2, 1.2, bar);
      }
    }
  });

  // The playhead is moved directly, without drawing the timeline again; while
  // playing, the view follows it.
  useEffect(() => {
    const paint = () => {
      const node = line.current;
      if (node) node.style.transform = `translateX(${x(Math.min(playhead.ms, total))}px)`;
      const box = scroller.current;
      if (!box || !playhead.playing || drag) return;
      const at = x(playhead.ms);
      if (at < box.scrollLeft + 20 || at > box.scrollLeft + box.clientWidth - 40) box.scrollLeft = at - box.clientWidth * 0.2;
    };
    paint();
    return playhead.subscribe(paint);
  });

  const zoomAround = (factor: number, clientX?: number) => {
    const box = scroller.current;
    if (!box) return;
    const pointer = clientX === undefined ? box.clientWidth / 2 : clientX - box.getBoundingClientRect().left;
    const ms = (box.scrollLeft + pointer - INSET) / ppms;
    const next = Math.max(fit, Math.min(MAX_PPMS, ppms * factor));
    setZoom(next <= fit * 1.001 ? null : next);
    requestAnimationFrame(() => {
      box.scrollLeft = INSET + ms * next - pointer;
    });
  };

  useEffect(() => {
    const box = scroller.current;
    if (!box) return;
    // Pinch or ⌘-wheel zooms about the pointer; a plain wheel scrolls along.
    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey || event.metaKey) {
        event.preventDefault();
        zoomAround(Math.exp(-event.deltaY * 0.01), event.clientX);
      } else if (Math.abs(event.deltaY) > Math.abs(event.deltaX)) {
        event.preventDefault();
        box.scrollLeft += event.deltaY;
      }
    };
    box.addEventListener("wheel", onWheel, { passive: false });
    return () => box.removeEventListener("wheel", onWheel);
  });

  const move = (event: React.PointerEvent) => {
    if (!drag) return;
    if (drag.mode === "scrub") player?.seek(msAt(event.clientX));
    else if (drag.mode === "trim") {
      const shift = (event.clientX - drag.x) / ppms;
      const raw = (drag.edge === "in" ? drag.clip.startMs : drag.clip.endMs) + shift;
      const snap = nearestEdge(words, raw, drag.edge, SNAP_PX / ppms);
      setDrag({ ...drag, ms: snap ?? raw, snapped: snap !== null });
    } else if (drag.mode === "bar" && scroller.current) {
      const track = event.currentTarget.getBoundingClientRect().width || 1;
      scroller.current.scrollLeft = drag.scroll + ((event.clientX - drag.x) / track) * content;
    }
  };
  const release = () => {
    if (drag?.mode === "trim" && Math.abs(drag.ms - (drag.edge === "in" ? drag.clip.startMs : drag.clip.endMs)) > 10) {
      onEdit(trimClip(clips, drag.id, drag.edge, drag.ms, durationMs));
    }
    setDrag(null);
  };

  const position = editedPosition(clips, playhead.ms);
  const splittable = !!position && !isCutIn(position.clip) && position.sourceMs > position.clip.startMs + 100 && position.sourceMs < position.clip.endMs - 100;
  const lastPicked = clips.reduce((last, clip, index) => (selected.includes(clip.id) ? index : last), -1);
  const insertCutIn = () => {
    const at = lastPicked >= 0 ? lastPicked : position ? (playhead.ms - starts[position.index] < (position.clip.endMs - position.clip.startMs) / 2 ? position.index - 1 : position.index) : clips.length - 1;
    const card = cutInCard();
    onEdit(insertAfter(clips, at, card));
    onSelect([card.id]);
  };
  const cueNumber = (clip: Clip) => clips.filter(isCutIn).findIndex((item) => item.id === clip.id);
  const thumb = { left: (view.scroll / content) * 100, width: Math.min(100, (view.width / content) * 100) };

  return (
    <section className="panel timeline-panel" aria-label="Timeline">
      <header className="panel-head">
        <div className="tools">
          <button
            type="button"
            className="icon-tool"
            disabled={disabled || !splittable}
            aria-label="Split at playhead"
            title="Split at playhead (⌘K)"
            onClick={() => position && onEdit(splitClip(clips, position.index, position.sourceMs))}
          >
            <Icon name="scissors" size={14} />
          </button>
          <span className="tool-divider" />
          <button type="button" className="tool" disabled={disabled} title={lastPicked >= 0 ? "Insert a cut-in after the selected clip" : "Insert a cut-in at the playhead"} onClick={insertCutIn}>
            <Icon name="type" size={13} />
            Cut-in
          </button>
          {selected.length > 0 && (
            <button
              type="button"
              className="tool"
              disabled={disabled}
              title="Remove the selected items (⌫)"
              onClick={() => {
                onEdit(clips.filter((clip) => !selected.includes(clip.id)));
                onSelect([]);
              }}
            >
              <Icon name="trash" size={13} />
              Remove {selected.length > 1 ? selected.length : ""}
            </button>
          )}
        </div>
        <span className="timeline-hint">Click to select · drag an edge to trim · double-click to jump there</span>
        <span className="faint timeline-zoom">
          <button type="button" className="icon-tool" aria-label="Zoom out" title="Zoom out (−)" onClick={() => zoomAround(1 / 1.6)}>
            <Icon name="minus" size={13} />
          </button>
          <button type="button" className="icon-tool" aria-label="Zoom in" title="Zoom in (=)" onClick={() => zoomAround(1.6)}>
            <Icon name="plus" size={13} />
          </button>
        </span>
      </header>
      <div className="timeline-body">
        <div className="tracks">
          <div className="track-heads" aria-hidden="true">
            <span className="ruler-head" />
            <span className="track-head">V1</span>
            <span className="track-head">A1</span>
          </div>
          <div
            ref={scroller}
            className="scroller"
            onScroll={(event) => setView({ width: event.currentTarget.clientWidth, scroll: event.currentTarget.scrollLeft })}
            onPointerMove={move}
            onPointerUp={release}
            onPointerCancel={() => setDrag(null)}
          >
            <div className="sequence" style={{ width: content }}>
              <canvas
                ref={ruler}
                className="ruler"
                style={{ width: view.width }}
                aria-label="Timeline ruler"
                onPointerDown={(event) => {
                  event.currentTarget.setPointerCapture(event.pointerId);
                  player?.seek(msAt(event.clientX));
                  setDrag({ mode: "scrub" });
                }}
              />
              <div
                className="video-track"
                onPointerDown={(event) => {
                  if (event.target !== event.currentTarget) return;
                  player?.seek(msAt(event.clientX));
                  onSelect([]);
                }}
              >
                {clips.map((clip, index) => {
                  const card = isCutIn(clip);
                  const left = x(starts[index]);
                  const trimmed = drag?.mode === "trim" && drag.id === clip.id ? drag : null;
                  const shown = trimmed ? (trimmed.edge === "in" ? { ...clip, startMs: trimmed.ms } : { ...clip, endMs: trimmed.ms }) : clip;
                  const width = Math.max(2, (shown.endMs - shown.startMs) * ppms - 1);
                  const shift = trimmed?.edge === "in" ? (trimmed.ms - clip.startMs) * ppms : 0;
                  return (
                    <button
                      type="button"
                      key={clip.id}
                      className={["clip", card ? "cut-in-clip" : "", hovered === clip.id ? "hovered" : "", selected.includes(clip.id) ? "selected" : ""].join(" ")}
                      style={{ left: left + Math.min(0, shift), width, ["--label" as string]: card ? CARD : FOOTAGE }}
                      title={card ? `Cut-in · ${labels[cueNumber(clip)] ?? ""}\n${clip.narration?.trim() ?? ""}` : `${range(clip, parts)} in the footage`}
                      aria-label={card ? `Cut-in ${cueNumber(clip) + 1}` : `Clip ${index + 1}, ${range(clip, parts)}`}
                      aria-pressed={selected.includes(clip.id)}
                      onPointerEnter={() => setHovered(clip.id)}
                      onPointerLeave={() => setHovered(null)}
                      onPointerDown={(event) => {
                        if ((event.target as HTMLElement).dataset.edge) return;
                        const many = event.metaKey || event.ctrlKey || event.shiftKey;
                        onSelect(many ? (selected.includes(clip.id) ? selected.filter((id) => id !== clip.id) : [...selected, clip.id]) : [clip.id]);
                      }}
                      onDoubleClick={() => player?.seek(starts[index])}
                    >
                      {width > 28 && (
                        <span className="clip-label">
                          {card ? (
                            <>
                              <Icon name="type" size={11} />
                              {width > 58 ? labels[cueNumber(clip)]?.toLowerCase() ?? "cut in" : null}
                            </>
                          ) : (
                            <>
                              <b>{String(index + 1).padStart(2, "0")}</b>
                              {width > 80 ? <span>{range(clip, parts)}</span> : null}
                            </>
                          )}
                        </span>
                      )}
                      {!card && width > 16 && !disabled && (
                        <>
                          {(["in", "out"] as const).map((edge) => (
                            <span
                              key={edge}
                              data-edge={edge}
                              className={`handle ${edge}`}
                              title="Drag to trim"
                              onPointerDown={(event) => {
                                event.stopPropagation();
                                scroller.current?.setPointerCapture(event.pointerId);
                                setDrag({ mode: "trim", id: clip.id, edge, x: event.clientX, clip, ms: edge === "in" ? clip.startMs : clip.endMs, snapped: false });
                              }}
                            />
                          ))}
                        </>
                      )}
                    </button>
                  );
                })}
                {!clips.length && <span className="track-empty">The sequence is empty. Select words in the transcript to add a clip, or build the script.</span>}
              </div>
              <div className="audio-track">
                {clips.map((clip, index) =>
                  isCutIn(clip) ? null : (
                    <span
                      key={clip.id}
                      className={["audio-clip", selected.includes(clip.id) ? "selected" : ""].join(" ")}
                      style={{ left: x(starts[index]), width: Math.max(2, (clip.endMs - clip.startMs) * ppms - 1) }}
                    />
                  ),
                )}
                <canvas ref={wave} className="wave" style={{ width: view.width }} />
              </div>
              {drag?.mode === "trim" && (
                <span className="trim-tip" style={{ left: x(starts[clips.findIndex((clip) => clip.id === drag.id)]) + (drag.ms - drag.clip.startMs) * ppms }}>
                  {drag.edge === "in" ? "In" : "Out"} {clock(drag.ms)}
                  {drag.snapped ? " · word" : ""}
                </span>
              )}
              <span ref={line} className="playhead" />
            </div>
          </div>
        </div>
        <div className="zoom-bar">
          <span className="zoom-label" aria-hidden="true" />
          <div
            className="zoom-track"
            title="Drag to scroll · double-click to fit"
            onDoubleClick={() => setZoom(null)}
            onPointerMove={move}
            onPointerUp={release}
          >
            <span
              className="zoom-thumb"
              style={{ left: `${thumb.left}%`, width: `${thumb.width}%` }}
              onPointerDown={(event) => {
                event.currentTarget.parentElement?.setPointerCapture(event.pointerId);
                setDrag({ mode: "bar", x: event.clientX, scroll: scroller.current?.scrollLeft ?? 0 });
              }}
            />
          </div>
        </div>
      </div>
    </section>
  );
}
