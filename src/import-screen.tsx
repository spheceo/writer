import { useEffect, useRef, useState } from "react";
import type { ImportProgress } from "./types";
import { Dialog } from "./ui";

/** An import's steps, in order, as import.rs reports them (its stage, and the words of its detail). */
const STEPS: { key: string; label: string; hint: string }[] = [
  { key: "download", label: "Download", hint: "The footage comes down from YouTube, Drive or this Mac." },
  { key: "preview", label: "Preview", hint: "Joined into one timeline and cut to a 480p preview the editors play." },
  { key: "research", label: "Research", hint: "The article and any case files are read for names and facts." },
  { key: "transcribe", label: "Transcribe", hint: "Every recording is heard and its words timed." },
  { key: "watch", label: "Check speakers", hint: "The footage is watched to check who says each line. Saved checks are reused after a restart." },
  { key: "speakers", label: "Name speakers", hint: "Each voice gets a name or a role." },
];

function stepOf(item: ImportProgress) {
  const said = `${item.stage} ${item.detail}`.toLowerCase();
  if (/who speaks|saved checks/.test(said)) return "watch";
  if (/naming the speakers|speaker/.test(said)) return "speakers";
  if (/transcrib|hearing|who speaks|heard/.test(said)) return "transcribe";
  if (/research|pdf|article|case file/.test(said)) return "research";
  if (/preview|joining|cutting|processing/.test(said)) return "preview";
  return "download";
}

function elapsed(ms: number) {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  return seconds >= 60 ? `${Math.floor(seconds / 60)} min ${String(seconds % 60).padStart(2, "0")} s` : `${seconds} s`;
}

/**
 * A script's import, shown over its project in the middle of the screen:
 * each step, how far it is, what it's doing now and for how long. The
 * import keeps going after the producer leaves; the list and the dock follow it.
 */
export function ImportScreen({ item, onLeave }: { item: ImportProgress | null; onLeave: () => void }) {
  const shown = useRef(item);
  if (item) shown.current = item;
  const current = shown.current;
  // When each step was first seen, for its time.
  const [began, setBegan] = useState<Record<string, number>>({});
  const timedId = useRef<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const step = current ? stepOf(current) : "download";
  useEffect(() => {
    if (!current) return;
    const changed = timedId.current !== current.id;
    timedId.current = current.id;
    setBegan((previous) => {
      const times = changed ? {} : previous;
      return times[step] ? times : { ...times, [step]: Date.now(), ...(times.start ? {} : { start: Date.now() }) };
    });
  }, [current?.id, step]);
  useEffect(() => {
    if (!item) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [!!item]);
  const at = STEPS.findIndex((candidate) => candidate.key === step);
  const percent = Math.max(0, Math.min(100, current?.percent ?? 0));
  return (
    <Dialog
      open={!!item}
      onClose={onLeave}
      title={`Importing ${current?.title ?? ""}`}
      description="Writer is getting the footage ready. It keeps going if you leave: the list and the panel at the bottom left follow it, and the script opens ready here."
      width={720}
      footer={
        <>
          <span className="faint">{began.start ? `Running for ${elapsed(now - began.start)}` : ""}</span>
          <span className="spacer" />
          <button type="button" className="primary" data-autofocus onClick={onLeave}>
            Back to scripts
          </button>
        </>
      }
    >
      <div className="import-screen">
        <div className="import-meter" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
          <div className="import-meter-head">
            <strong>{percent}%</strong>
            <span>{current?.detail || "Starting…"}</span>
          </div>
          <div className="import-bar">
            <i style={{ width: `${percent}%` }} />
          </div>
        </div>
        <ol className="import-steps">
          {STEPS.map((candidate, index) => {
            const state = index < at ? "done" : index === at ? "now" : "later";
            return (
              <li key={candidate.key} data-state={state}>
                <span className="import-dot" aria-hidden="true" />
                <div>
                  <strong>{candidate.label}</strong>
                  <p>{state === "now" && current?.detail ? current.detail : candidate.hint}</p>
                </div>
                <span className="faint import-time">
                  {state === "now" && began[candidate.key] ? elapsed(now - began[candidate.key]) : state === "done" ? "Done" : ""}
                </span>
              </li>
            );
          })}
        </ol>
      </div>
    </Dialog>
  );
}
