import { type ReactNode, type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "./ui";

/** A sheet as Docs lays out the export, at 96 dpi with one-inch margins. */
export type Sheet = { width: number; height: number };
export const LETTER: Sheet = { width: 816, height: 1056 };
export const A4: Sheet = { width: 794, height: 1123 };
const MARGIN = 96;
const GAP = 16;
const RAIL_GAP = 12;
export const RAIL_W = 300;

export type Zoom = "fit" | number;
export const ZOOMS: { value: Zoom; label: string }[] = [
  { value: "fit", label: "Fit page" },
  { value: 0.5, label: "50%" },
  { value: 0.75, label: "75%" },
  { value: 0.9, label: "90%" },
  { value: 1, label: "100%" },
  { value: 1.25, label: "125%" },
];

/** A zoom kept per view, so Script and Split each open as they were left. */
export function useZoom(key: string) {
  const [zoom, setZoom] = useState<Zoom>(() => {
    const saved = window.localStorage.getItem(`writer.zoom.${key}`);
    if (!saved || saved === "fit") return "fit";
    const value = Number(saved);
    return ZOOMS.some((option) => option.value === value) ? value : "fit";
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
    const spread = sheet.width + (rail ? RAIL_GAP + RAIL_W : 0);
    const fit = () => setScale(zoom === "fit" ? Math.max(0.3, Math.min(1.5, (node.clientWidth - 48) / spread)) : zoom);
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(node);
    return () => observer.disconnect();
  }, [canvas, zoom, sheet.width, rail]);
  return scale;
}

function step(zoom: Zoom, scale: number, by: 1 | -1): Zoom {
  const fixed = ZOOMS.map((option) => option.value).filter((value): value is number => typeof value === "number");
  const now = zoom === "fit" ? scale : zoom;
  const next = by > 0 ? fixed.find((value) => value > now + 0.001) : [...fixed].reverse().find((value) => value < now - 0.001);
  return next ?? zoom;
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
  return (
    <div className="zoom">
      <div className="zoom-tools">
        <button type="button" aria-label="Zoom out" onClick={() => onZoom(step(zoom, scale, -1))}>
          <Icon name="minus" size={13} />
        </button>
        <label className="zoom-select">
          <span className="sr-only">Script zoom</span>
          <select value={String(zoom)} onChange={(event) => onZoom(event.target.value === "fit" ? "fit" : Number(event.target.value))}>
            {ZOOMS.map((option) => (
              <option key={option.label} value={String(option.value)}>{option.label}</option>
            ))}
          </select>
          <Icon name="chevron" size={14} />
        </label>
        <button type="button" aria-label="Zoom in" onClick={() => onZoom(step(zoom, scale, 1))}>
          <Icon name="plus" size={13} />
        </button>
      </div>
      <span className="zoom-value">{Math.round(scale * 100)}%</span>
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
      below = top + card.offsetHeight + 8;
    }
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: lays out again on every change it is told of
  useLayoutEffect(() => layout.current(), [version, children, rail]);
  useEffect(() => {
    const root = body.current;
    if (!root) return;
    const observer = new ResizeObserver(() => layout.current());
    observer.observe(root);
    return () => observer.disconnect();
  }, []);

  const height = pages * stride - GAP;
  const width = sheet.width + (rail ? RAIL_GAP + RAIL_W : 0);
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
          <aside className="doc-rail" ref={cards} style={{ height }}>
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
  onCommit,
}: {
  value: string;
  disabled: boolean;
  placeholder?: string;
  className?: string;
  label?: string;
  /** A single line, such as a heading: Enter commits rather than breaking it. */
  oneLine?: boolean;
  onCommit: (text: string) => void;
}) {
  const [text, setText] = useState(value);
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => setText(value), [value]);
  useLayoutEffect(() => {
    const node = area.current;
    if (!node) return;
    node.style.height = "auto";
    node.style.height = `${node.scrollHeight}px`;
  }, [text]);
  return (
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
    />
  );
}
