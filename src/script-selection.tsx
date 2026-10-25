import { type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./ui";

export type ScriptSelection = { id: string; input: HTMLTextAreaElement; start: number; end: number; quote: string };

/** Repack's selection mirror, measured in document pixels and then scaled once. */
function selectionRect(selection: ScriptSelection) {
  const { input, start, end } = selection;
  const mirror = document.createElement("div");
  const style = getComputedStyle(input);
  for (const property of ["font-family", "font-size", "font-weight", "font-style", "line-height", "letter-spacing", "word-spacing", "text-align", "text-indent", "text-transform", "tab-size", "direction", "padding"]) mirror.style.setProperty(property, style.getPropertyValue(property));
  Object.assign(mirror.style, { position: "fixed", left: "0", top: "0", visibility: "hidden", boxSizing: "border-box", width: `${input.clientWidth}px`, whiteSpace: "pre-wrap", overflowWrap: "break-word" });
  const text = document.createTextNode(input.value);
  mirror.append(text);
  document.body.append(mirror);
  try {
    const range = document.createRange();
    range.setStart(text, Math.min(start, text.length));
    range.setEnd(text, Math.min(end, text.length));
    const rect = range.getClientRects()[0];
    if (!rect) return null;
    const box = input.getBoundingClientRect();
    const scale = box.width / Math.max(1, input.offsetWidth);
    return { left: box.left + rect.left * scale, top: box.top + (rect.top - input.scrollTop) * scale, bottom: box.top + (rect.bottom - input.scrollTop) * scale, width: rect.width * scale };
  } finally { mirror.remove(); }
}

export function ScriptSelectionActions({ selection, scroller, onComment, onChat, onDismiss }: {
  selection: ScriptSelection;
  scroller: RefObject<HTMLDivElement | null>;
  onComment: () => void;
  onChat: () => void;
  onDismiss: () => void;
}) {
  const pill = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: 0, top: 0, visible: false });
  useLayoutEffect(() => {
    const update = () => {
      const rect = selectionRect(selection);
      const viewport = scroller.current?.getBoundingClientRect();
      const node = pill.current;
      if (!rect || !viewport || !node) return;
      setPosition({ left: Math.max(8, Math.min(rect.left + rect.width / 2 - node.offsetWidth / 2, window.innerWidth - node.offsetWidth - 8)), top: Math.max(viewport.top + 8, rect.top - node.offsetHeight - 8), visible: rect.bottom > viewport.top && rect.top < viewport.bottom });
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(selection.input);
    window.addEventListener("scroll", update, true);
    window.addEventListener("resize", update);
    return () => { observer.disconnect(); window.removeEventListener("scroll", update, true); window.removeEventListener("resize", update); };
  }, [selection, scroller]);
  useEffect(() => {
    const away = (event: PointerEvent) => { if (!pill.current?.contains(event.target as Node) && event.target !== selection.input) onDismiss(); };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") onDismiss(); };
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", key);
    return () => { window.removeEventListener("pointerdown", away, true); window.removeEventListener("keydown", key); };
  }, [selection.input, onDismiss]);
  return createPortal(<div ref={pill} className="script-selection" role="toolbar" aria-label="Selected script text" data-script-selection-actions style={{ left: position.left, top: position.top, visibility: position.visible ? "visible" : "hidden" }} onMouseDown={(event) => event.preventDefault()}>
    <button type="button" onClick={onComment}><Icon name="note" size={14} /> Add comment</button>
    <span className="tool-divider" />
    <button type="button" onClick={onChat}><Icon name="quote" size={14} /> Add to chat</button>
  </div>, document.body);
}
