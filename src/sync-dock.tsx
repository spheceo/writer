import { useEffect, useRef, useState } from "react";
import { type Activity, getActivity } from "./api";
import { useCloud } from "./cloud-badge";

const share = (item: Activity) => (item.total ? Math.min(1, item.done / item.total) : null);
const percent = (item: Activity) => {
  const part = share(item);
  return part === null ? "" : ` ${Math.round(part * 100)}%`;
};
/** A key gone this long and back counts as new work, and opens the list again. */
const FORGET_MS = 60_000;

/**
 * Everything under way, at the bottom left on every screen: imports,
 * Claude's jobs and the cloud's transfers, each with its step and a bar.
 * A small button while anything runs or the cloud needs a word; its list
 * opens by itself when new work starts, goes away on a click anywhere
 * else, and comes back on a click of the button. Nothing shows when all
 * is quiet and synced.
 */
export function SyncDock({ onOpen }: { onOpen: (script: string) => void }) {
  const cloud = useCloud();
  const [work, setWork] = useState<Activity[]>([]);
  const [open, setOpen] = useState(false);
  const seen = useRef(new Map<string, number>());
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let live = true;
    // An update shows on its own button at the top (update.tsx).
    const ask = () => void getActivity().then((found) => live && setWork(found.filter((item) => item.key !== "update")), () => {});
    ask();
    const timer = window.setInterval(ask, 1500);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, []);

  // New work opens the list; work it has already shown doesn't, even when the person closed it.
  useEffect(() => {
    const now = Date.now();
    const fresh = work.some((item) => now - (seen.current.get(item.key) ?? 0) > FORGET_MS);
    for (const item of work) seen.current.set(item.key, now);
    if (fresh) setOpen(true);
  }, [work]);

  useEffect(() => {
    if (!open) return;
    const away = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const key = (event: KeyboardEvent) => event.key === "Escape" && setOpen(false);
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", key);
    };
  }, [open]);

  const state = cloud?.state ?? "";
  const trouble = state === "offline" || state === "error";
  const syncing = state === "syncing";
  const shown = work.length > 0 || trouble || syncing;
  useEffect(() => {
    if (!shown) setOpen(false);
  }, [shown]);
  if (!shown) return null;

  const left = cloud?.pending ? ` · ${cloud.pending} left` : "";
  const label =
    work.length === 1
      ? `${work[0].what}${percent(work[0])}`
      : work.length > 1
        ? `${work.length} things under way`
        : state === "offline"
          ? "Offline"
          : state === "error"
            ? "Sync problem"
            : `Syncing${left}`;
  const tone = state === "error" ? "bad" : state === "offline" ? "warn" : "busy";
  const cloudLine =
    state === "offline"
      ? "Offline: everything is saved on this Mac and syncs when the connection is back."
      : state === "error"
        ? (cloud?.error ?? "The cloud refused something; Writer keeps trying.")
        : syncing && !work.some((item) => item.key === "cloud")
          ? `Syncing with the cloud${left}.`
          : null;

  return (
    <div className="sync-dock" ref={root}>
      {open && (
        <div className="sync-list" role="dialog" aria-label="Under way">
          {cloudLine && <p className={`sync-cloud ${tone}`}>{cloudLine}</p>}
          {work.map((item) => {
            const part = share(item);
            return (
              <button
                key={item.key}
                type="button"
                className="sync-item"
                disabled={!item.script}
                title={`${item.what}: ${item.title}\n${item.step}`}
                onClick={() => {
                  if (!item.script) return;
                  setOpen(false);
                  onOpen(item.script);
                }}
              >
                <span className="sync-head">
                  <span className="truncate">
                    <b>{item.what}</b> · {item.title}
                  </span>
                  {part !== null && <span className="faint">{Math.round(part * 100)}%</span>}
                </span>
                <span className="sync-step truncate">{item.step}</span>
                <span className="sync-bar" data-unknown={part === null || undefined}>
                  <i style={part === null ? undefined : { width: `${Math.round(part * 100)}%` }} />
                </span>
              </button>
            );
          })}
        </div>
      )}
      <button type="button" className={`sync-chip ${tone}`} aria-expanded={open} onClick={() => setOpen(!open)}>
        <i aria-hidden="true" />
        <span>{label}</span>
      </button>
    </div>
  );
}
