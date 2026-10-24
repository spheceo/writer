import { useEffect, useState } from "react";
import { Dialog, Icon } from "./ui";

export type Shortcut = { keys: string[]; what: string };

/** What can be done, by key and by pointer: footage scripts have the timeline and transcript too. */
export const FOOTAGE_SHORTCUTS: { title: string; items: Shortcut[] }[] = [
  {
    title: "Playback",
    items: [
      { keys: ["Space"], what: "Play or pause the cut" },
      { keys: ["J"], what: "Back 10 seconds" },
      { keys: ["L"], what: "Forward 10 seconds" },
      { keys: ["Home"], what: "Go to the start" },
      { keys: ["End"], what: "Go to the end" },
      { keys: ["F"], what: "Full view of the picture (Video view)" },
    ],
  },
  {
    title: "Editing",
    items: [
      { keys: ["⌘", "Z"], what: "Undo" },
      { keys: ["⇧", "⌘", "Z"], what: "Redo" },
      { keys: ["⌘", "K"], what: "Split the clip at the playhead" },
      { keys: ["⌫"], what: "Remove the selected timeline items" },
      { keys: ["Esc"], what: "Clear the selection" },
    ],
  },
  {
    title: "Pointer",
    items: [
      { keys: ["Click"], what: "A transcript word shows its frame; a timestamp in the script plays it" },
      { keys: ["Double-click"], what: "A transcript word plays the footage from there" },
      { keys: ["⇧", "Click"], what: "Select a stretch of transcript to add or remove" },
      { keys: ["Drag"], what: "A clip's edge trims it, snapping to words; the ruler scrubs" },
      { keys: ["⌘", "Scroll"], what: "Zoom the timeline" },
    ],
  },
];

export const COMMENTARY_SHORTCUTS: { title: string; items: Shortcut[] }[] = [
  {
    title: "Preview (Split)",
    items: [
      { keys: ["Space"], what: "Play or pause the script" },
      { keys: ["Click"], what: "A timestamp or its comment plays from there" },
    ],
  },
  {
    title: "Writing",
    items: [
      { keys: ["Enter"], what: "Send to Claude from the composer" },
      { keys: ["⇧", "Enter"], what: "A new line in the composer" },
      { keys: ["Hover"], what: "A paragraph shows Note and Remove in the margin" },
    ],
  },
];

/** A keyboard button for the top bar, and the sheet it opens; "?" opens it too. */
/** The shortcuts sheet, opened by ? anywhere; with `open`/`onOpenChange` it's opened from elsewhere (a menu) and has no button of its own. */
export function Shortcuts({ groups, open: shown, onOpenChange }: { groups: { title: string; items: Shortcut[] }[]; open?: boolean; onOpenChange?: (open: boolean) => void }) {
  const [own, setOwn] = useState(false);
  const open = shown ?? own;
  const setOpen = (next: boolean | ((now: boolean) => boolean)) => {
    const value = typeof next === "function" ? next(open) : next;
    if (onOpenChange) onOpenChange(value);
    else setOwn(value);
  };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (event.key !== "?" || (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA"))) return;
      event.preventDefault();
      setOpen((now) => !now);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <>
      {!onOpenChange && (
        <button type="button" className="icon-tool" aria-label="Shortcuts" title="Shortcuts (?)" onClick={() => setOpen(true)}>
          <Icon name="keyboard" />
        </button>
      )}
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        title="Shortcuts"
        width={520}
        footer={
          <>
            <span className="spacer" />
            <button type="button" className="ghost" data-autofocus onClick={() => setOpen(false)}>
              Close
            </button>
          </>
        }
      >
        <div className="shortcuts">
          {groups.map((group) => (
            <section key={group.title}>
              <h3>{group.title}</h3>
              <ul>
                {group.items.map((item) => (
                  <li key={item.what}>
                    <span>{item.what}</span>
                    <span className="keys">
                      {item.keys.map((key) => (
                        <kbd key={key}>{key}</kbd>
                      ))}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      </Dialog>
    </>
  );
}
