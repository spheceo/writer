// Scrollbars only while scrolling: the native ones are hidden (styles.css),
// and anything that scrolls gets a thin thumb, no track, over its edge while
// it moves, gone a moment after it stops. The thumb can be dragged.

const IDLE_MS = 800;
const MIN = 28;

type Bar = { thumb: HTMLDivElement; axis: "y" | "x"; timer: number; held: boolean; hover: boolean };
const bars = new WeakMap<Element, { y?: Bar; x?: Bar }>();

function scrolled(target: EventTarget | null): Element | null {
  if (target === document || target === window) return document.scrollingElement;
  return target instanceof Element ? target : null;
}

function bar(element: Element, axis: "y" | "x"): Bar {
  const found = bars.get(element) ?? {};
  const existing = found[axis];
  if (existing?.thumb.isConnected) return existing;
  const thumb = document.createElement("div");
  thumb.className = `scroll-thumb ${axis}`;
  document.body.append(thumb);
  const made: Bar = { thumb, axis, timer: 0, held: false, hover: false };
  thumb.addEventListener("pointerenter", () => { made.hover = true; show(element, made); });
  thumb.addEventListener("pointerleave", () => { made.hover = false; later(made); });
  thumb.addEventListener("pointerdown", (event) => drag(event, element, made));
  bars.set(element, { ...found, [axis]: made });
  return made;
}

function place(element: Element, made: Bar): boolean {
  const box = element === document.scrollingElement ? new DOMRect(0, 0, innerWidth, innerHeight) : element.getBoundingClientRect();
  const [size, view, at] = made.axis === "y"
    ? [element.scrollHeight, element.clientHeight, element.scrollTop]
    : [element.scrollWidth, element.clientWidth, element.scrollLeft];
  if (size <= view + 1 || view === 0) return false;
  const length = Math.max(MIN, (view * view) / size);
  const offset = (at / (size - view)) * (view - length);
  const style = made.thumb.style;
  if (made.axis === "y") {
    Object.assign(style, { top: `${box.top + offset}px`, left: `${box.right - 9}px`, height: `${length}px`, width: "" });
  } else {
    Object.assign(style, { left: `${box.left + offset}px`, top: `${box.bottom - 9}px`, width: `${length}px`, height: "" });
  }
  return true;
}

function show(element: Element, made: Bar) {
  if (!place(element, made)) return;
  made.thumb.classList.add("on");
  later(made);
}

function later(made: Bar) {
  clearTimeout(made.timer);
  made.timer = window.setTimeout(() => {
    if (!made.held && !made.hover) made.thumb.classList.remove("on");
  }, IDLE_MS);
}

function drag(event: PointerEvent, element: Element, made: Bar) {
  event.preventDefault();
  made.held = true;
  made.thumb.setPointerCapture(event.pointerId);
  const start = made.axis === "y" ? event.clientY : event.clientX;
  const from = made.axis === "y" ? element.scrollTop : element.scrollLeft;
  const [size, view] = made.axis === "y" ? [element.scrollHeight, element.clientHeight] : [element.scrollWidth, element.clientWidth];
  const length = Math.max(MIN, (view * view) / size);
  const ratio = (size - view) / Math.max(1, view - length);
  const move = (next: PointerEvent) => {
    const delta = (made.axis === "y" ? next.clientY : next.clientX) - start;
    if (made.axis === "y") element.scrollTop = from + delta * ratio;
    else element.scrollLeft = from + delta * ratio;
  };
  const up = () => {
    made.held = false;
    made.thumb.removeEventListener("pointermove", move);
    later(made);
  };
  made.thumb.addEventListener("pointermove", move);
  made.thumb.addEventListener("pointerup", up, { once: true });
  made.thumb.addEventListener("pointercancel", up, { once: true });
}

export function startScrollbars() {
  document.addEventListener(
    "scroll",
    (event) => {
      const element = scrolled(event.target);
      if (!element) return;
      for (const axis of ["y", "x"] as const) {
        const [size, view] = axis === "y" ? [element.scrollHeight, element.clientHeight] : [element.scrollWidth, element.clientWidth];
        if (size > view + 1) show(element, bar(element, axis));
      }
    },
    { capture: true, passive: true },
  );
}
