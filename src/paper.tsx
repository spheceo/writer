import { type ReactNode, type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import { hoverNote, openNote } from "./comments";
import { type Band, showTip } from "./review";
import { Icon, Select } from "./ui";

/** A sheet as Docs lays out the export, at 96 dpi with one-inch margins. */
export type Sheet = { width: number; height: number };
export const LETTER: Sheet = { width: 816, height: 1056 };
export const A4: Sheet = { width: 794, height: 1123 };
const MARGIN = 96;
const GAP = 16;
const RAIL_GAP = 12;
export const RAIL_W = 300;

export type Zoom = "fit" | "width" | number;
export const ZOOMS: { value: Zoom; label: string }[] = [
  { value: "fit", label: "Fit page" },
  { value: "width", label: "Fit width" },
  { value: 0.25, label: "25%" },
  { value: 0.5, label: "50%" },
  { value: 0.75, label: "75%" },
  { value: 1, label: "100%" },
  { value: 1.25, label: "125%" },
  { value: 1.5, label: "150%" },
  { value: 2, label: "200%" },
];

/** A zoom kept per view, so Script and Split each open as they were left. */
export function useZoom(key: string) {
  const [zoom, setZoom] = useState<Zoom>(() => {
    const saved = window.localStorage.getItem(`writer.zoom.${key}`);
    if (!saved) return key.endsWith("split") ? "width" : "fit";
    if (saved === "fit" || saved === "width") return saved;
    const value = Number(saved);
    return Number.isFinite(value) && value >= 0.01 && value <= 4 ? value : "fit";
  });
  useEffect(() => window.localStorage.setItem(`writer.zoom.${key}`, String(zoom)), [key, zoom]);
  return [zoom, setZoom] as const;
}

/** The scale a zoom shows at: "Fit page" sizes the page and its comments to the canvas. */
export function useScale(canvas: RefObject<HTMLDivElement | null>, zoom: Zoom, sheet: Sheet, rail: boolean) {
  const [scale, setScale] = useState(typeof zoom === "number" ? zoom : 0.6);
  useEffect(() => {
    const node = canvas.current;
    if (!node) return;
    const fit = () => {
      const width = (node.clientWidth - 48 - (rail ? RAIL_GAP + RAIL_W : 0)) / sheet.width;
      const height = (node.clientHeight - 48) / sheet.height;
      setScale(typeof zoom === "number" ? zoom : Math.max(0.1, Math.min(width, zoom === "fit" ? height : Infinity)));
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(node);
    return () => observer.disconnect();
  }, [canvas, zoom, sheet.width, sheet.height, rail]);
  return scale;
}

function step(zoom: Zoom, scale: number, by: 1 | -1): Zoom {
  const now = typeof zoom === "number" ? zoom : scale;
  return Math.min(4, Math.max(0.01, (Math.round(now * 10) + by) / 10));
}

/** The document toolbar's left side, as Repack's: step, choose, the scale, and the page in view. */
export function ZoomTools({
  zoom,
  scale,
  page,
  pages,
  onZoom,
}: {
  zoom: Zoom;
  scale: number;
  page: number;
  pages: number;
  onZoom: (zoom: Zoom) => void;
}) {
  const sizes = [...new Set([...ZOOMS.flatMap((option) => typeof option.value === "number" ? [option.value] : []), ...(typeof zoom === "number" ? [zoom] : [])])].sort((a, b) => a - b);
  return (
    <div className="zoom">
      <div className="zoom-tools">
        <button type="button" aria-label="Zoom out" onClick={() => onZoom(step(zoom, scale, -1))}>
          <Icon name="minus" size={13} />
        </button>
        <Select
          size="sm"
          className="zoom-select"
          value={String(zoom)}
          label="Script zoom"
          options={[{ value: "fit", label: "Fit page" }, { value: "width", label: "Fit width" }, ...sizes.map((value) => ({ value: String(value), label: `${Math.round(value * 100)}%` }))]}
          onChange={(value) => onZoom(value === "fit" || value === "width" ? value : Number(value))}
        />
        <button type="button" aria-label="Zoom in" onClick={() => onZoom(step(zoom, scale, 1))}>
          <Icon name="plus" size={13} />
        </button>
      </div>
      {typeof zoom !== "number" && <span className="zoom-value">{Math.round(scale * 100)}%</span>}
      <span className="page-of">Page {Math.min(page, pages)} of {pages}</span>
    </div>
  );
}

/** Which sheet is in view, from the canvas's scroll. */
export const pageInView = (canvas: HTMLElement, scale: number, sheet: Sheet) =>
  Math.floor((canvas.scrollTop / scale + sheet.height / 3) / (sheet.height + GAP)) + 1;

/** Where an element sits in a container, in the container's own pixels. */
function within(node: HTMLElement, root: HTMLElement) {
  let y = 0;
  let at: HTMLElement | null = node;
  while (at && at !== root) {
    y += at.offsetTop;
    at = at.offsetParent as HTMLElement | null;
  }
  return y;
}

/**
 * The script as its Doc: white sheets with one-inch margins and the text in
 * one column over them. A block (`data-block`) that would cross a sheet's
 * bottom margin moves down to start the next. Comments (`data-card`) sit
 * beside the page, each by its anchor (`data-anchor`), pushed down past the
 * one above.
 */
export function Paper({
  sheet,
  scale,
  className,
  rail,
  version,
  onPages,
  children,
}: {
  sheet: Sheet;
  scale: number;
  className?: string;
  /** Comment cards, or nothing for a page without a rail. */
  rail?: ReactNode;
  /** Anything that changes the layout without resizing the text: lays out again. */
  version?: unknown;
  onPages?: (pages: number) => void;
  children: ReactNode;
}) {
  const body = useRef<HTMLDivElement>(null);
  const cards = useRef<HTMLElement>(null);
  const [pages, setPages] = useState(1);
  const stride = sheet.height + GAP;

  const layout = useRef(() => {});
  layout.current = () => {
    const root = body.current;
    if (!root) return;
    const blocks = [...root.querySelectorAll<HTMLElement>(":scope > [data-block]")];
    for (const block of blocks) block.style.paddingTop = "";
    for (const block of blocks) {
      const top = MARGIN + block.offsetTop;
      const height = block.offsetHeight;
      const at = Math.floor(top / stride);
      if (top + height > at * stride + sheet.height - MARGIN && height <= sheet.height - 2 * MARGIN) {
        block.style.paddingTop = `${(at + 1) * stride + MARGIN - top}px`;
      }
    }
    const count = Math.max(1, Math.floor((MARGIN + root.offsetHeight - 1) / stride) + 1);
    setPages(count);
    onPages?.(count);
    let below = 0;
    for (const card of cards.current?.querySelectorAll<HTMLElement>("[data-card]") ?? []) {
      const anchor = root.querySelector<HTMLElement>(`[data-anchor="${card.dataset.card}"]`);
      const top = Math.max(anchor ? MARGIN + within(anchor, root) - 10 : below, below);
      card.style.top = `${top}px`;
      below = top + (card.offsetHeight + 8) / scale;
    }
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: lays out again on every change it is told of
  useLayoutEffect(() => layout.current(), [version, children, rail, scale]);
  useEffect(() => {
    const root = body.current;
    if (!root) return;
    const observer = new ResizeObserver(() => layout.current());
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  const height = pages * stride - GAP;
  const width = sheet.width + (rail ? (RAIL_GAP + RAIL_W) / scale : 0);
  return (
    <div className="sheet" style={{ width: width * scale, height: height * scale }}>
      <div className="spread" style={{ width, height, transform: `scale(${scale})`, ["--inverse" as string]: 1 / scale }}>
        <article className={className ? `page ${className}` : "page"} style={{ width: sheet.width, height }}>
          {Array.from({ length: pages }, (_, index) => (
            <div className="paper" key={index} style={{ top: index * stride, height: sheet.height }} />
          ))}
          <div className="page-body" ref={body}>
            {children}
          </div>
        </article>
        {rail ? (
          <aside className="doc-rail" ref={cards} style={{ height, width: RAIL_W / scale, marginLeft: RAIL_GAP / scale }}>
            {rail}
          </aside>
        ) : null}
      </div>
    </div>
  );
}

/** A textarea that grows with its text and saves when it loses focus. */
export function Grow({
  value,
  disabled,
  placeholder,
  className,
  label,
  oneLine,
  bands,
  onWords,
  onCommit,
}: {
  value: string;
  disabled: boolean;
  placeholder?: string;
  className?: string;
  label?: string;
  /** A single line, such as a heading: Enter commits rather than breaking it. */
  oneLine?: boolean;
  /** Review bands on the text (review.tsx): drawn under it, their notes shown on hover. */
  bands?: Band[];
  /** Words selected (snapped to whole words, as the review page snaps them): where, and where on screen. */
  onWords?: (start: number, end: number, rect: DOMRect, input: HTMLTextAreaElement) => void;
  onCommit: (text: string) => void;
}) {
  const [text, setText] = useState(value);
  const area = useRef<HTMLTextAreaElement>(null);
  const mirror = useRef<HTMLDivElement>(null);
  const banded = !!bands?.length;
  useEffect(() => setText(value), [value]);
  useLayoutEffect(() => {
    const node = area.current;
    if (!node) return;
    node.style.height = "auto";
    node.style.height = `${node.scrollHeight}px`;
    // The bands sit in a copy of the text behind it, laid out as it is.
    const copy = mirror.current;
    if (!copy) return;
    const style = window.getComputedStyle(node);
    for (const key of MIRRORED) copy.style.setProperty(key, style.getPropertyValue(key));
  }, [text, banded]);
  const textarea = (
    <textarea
      ref={area}
      className={className}
      aria-label={label}
      rows={1}
      value={text}
      spellCheck
      disabled={disabled}
      placeholder={placeholder}
      onChange={(event) => setText(oneLine ? event.target.value.replace(/\n/g, " ") : event.target.value)}
      onKeyDown={(event) => {
        if (oneLine && event.key === "Enter") {
          event.preventDefault();
          event.currentTarget.blur();
        }
      }}
      onBlur={() => text !== value && onCommit(text)}
      onPointerMove={
        banded
          ? (event) => {
              const ids = markAt(event.clientX, event.clientY);
              showTip(ids.length ? { ids, x: event.clientX, y: event.clientY } : null);
              hoverNote(ids[0] ?? null);
            }
          : undefined
      }
      onPointerLeave={
        banded
          ? () => {
              showTip(null);
              hoverNote(null);
            }
          : undefined
      }
      onMouseUp={(event) => {
        const node = event.currentTarget;
        if (node.selectionStart !== node.selectionEnd) return picked(node);
        // A click on noted words opens their comment, as on the review page.
        const ids = banded ? markAt(event.clientX, event.clientY) : [];
        if (ids.length) openNote(ids[0]);
      }}
      onKeyUp={(event) => event.shiftKey && picked(event.currentTarget)}
      onSelect={(event) => picked(event.currentTarget)}
    />
  );
  function markAt(x: number, y: number) {
    const marks = [...(mirror.current?.querySelectorAll<HTMLElement>("mark") ?? [])];
    const hit = marks.find((mark) => [...mark.getClientRects()].some((box) => x >= box.left - 2 && x <= box.right + 2 && y >= box.top && y <= box.bottom));
    return (hit?.dataset.ids ?? "").split(" ").filter(Boolean);
  }
  // Selected words, widened to whole words (the review page's snapToWords).
  function picked(node: HTMLTextAreaElement) {
    if (!onWords || node.selectionStart === node.selectionEnd) return;
    let [start, end] = [node.selectionStart, node.selectionEnd];
    while (start > 0 && /[\p{L}\p{N}'’-]/u.test(node.value[start - 1])) start -= 1;
    while (end < node.value.length && /[\p{L}\p{N}'’-]/u.test(node.value[end])) end += 1;
    while (start < end && /\s/.test(node.value[start])) start += 1;
    while (end > start && /\s/.test(node.value[end - 1])) end -= 1;
    if (end > start) onWords(start, end, node.getBoundingClientRect(), node);
  }
  if (!banded || !bands) return textarea;
  return (
    <div className="grow-banded" data-band-text>
      <div className="grow-mirror" ref={mirror} aria-hidden="true">
        {marked(text, bands)}
        {"\u200b"}
      </div>
      {textarea}
    </div>
  );
}

/** What the copy behind a banded field takes from it, so its lines break alike. */
const MIRRORED = [
  "font-family", "font-size", "font-weight", "font-style", "line-height", "letter-spacing", "word-spacing", "text-transform", "text-indent", "text-align",
  "padding-top", "padding-right", "padding-bottom", "padding-left", "border-top-width", "border-right-width", "border-bottom-width", "border-left-width", "box-sizing",
];

/** The text cut at every band's edges, each banded stretch a mark carrying its notes. */
function marked(text: string, bands: Band[]) {
  const edges = [...new Set([0, text.length, ...bands.flatMap((band) => [band.start, band.end]).map((at) => Math.max(0, Math.min(text.length, at)))])].sort((a, b) => a - b);
  const out: ReactNode[] = [];
  edges.forEach((at, index) => {
    // A cut: where words were taken out.
    const cuts = bands.filter((band) => band.start === band.end && Math.min(band.start, text.length) === at);
    if (cuts.length) out.push(<mark key={`cut-${at}`} className="cut" data-ids={cuts.flatMap((band) => band.notes).join(" ")} data-kind={cuts.some((band) => band.kind === "fix") ? "fix" : "open"} />);
    const next = edges[index + 1];
    if (next === undefined || next === at) return;
    const over = bands.filter((band) => band.start < next && band.end > at);
    const piece = text.slice(at, next);
    out.push(
      over.length ? (
        <mark key={at} data-ids={over.flatMap((band) => band.notes).join(" ")} data-kind={over.some((band) => band.kind === "fix") ? "fix" : over.some((band) => band.kind === "open") ? "open" : "note"}>
          {piece}
        </mark>
      ) : (
        <span key={at}>{piece}</span>
      ),
    );
  });
  return out;
}
