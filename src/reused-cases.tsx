import { useState } from "react";
import { clock } from "./clips";
import type { Highlight, Script } from "./types";
import { Icon } from "./ui";

/** A reused case given as the Google Doc of its earlier script. */
const isDoc = (url: string) => url.includes("docs.google.com/document/d/");

/**
 * A compilation's reused cases, as Repack's Videos tab lists them: each one
 * not yet in the footage can be imported (its whole video, for the opening's
 * intro clips), and each one imported shows its picked moments, ready to
 * play or insert.
 */
export function ReusedCases({
  script,
  busy,
  onImport,
  onPlay,
  onInsert,
}: {
  script: Script;
  busy: boolean;
  /** Imports these reused cases' videos, or every one not yet in. */
  onImport: (urls?: string[]) => void;
  onPlay: (moment: Highlight) => void;
  /** Inserts a moment at the very start, as an intro clip. */
  onInsert: (moment: Highlight) => void;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const reuses = script.reuses ?? [];
  if (!script.compilation || script.sourceKind !== "compilation" || !reuses.length) return null;
  const parts = script.parts ?? [];
  // A reused case given as its earlier script's Doc can bring in only the footage that Doc lists.
  const noFootage = (reuse: (typeof reuses)[number]) => isDoc(reuse.url) && !reuse.footage;
  const missing = reuses.filter((reuse) => !parts.some((part) => part.reuse === reuse.url) && !noFootage(reuse));
  return (
    <div className="reused-cases">
      <div className="reused-head">
        <strong>Reused cases</strong>
        {missing.length > 0 && (
          <button type="button" className="sm" disabled={busy || !script.words.length} title="Import their whole videos, so their strongest moments can open this video" onClick={() => onImport()}>
            {missing.length === reuses.length ? "Import" : `Import ${missing.length} more`}
          </button>
        )}
      </div>
      {missing.length > 0 && (
        <p className="faint">
          {missing.length === 1 ? "One reused case isn't" : `${missing.length} reused cases aren't`} in the footage. Import {missing.length === 1 ? "its video" : "their videos"} to open the video on {missing.length === 1 ? "its" : "their"} strongest moments, as intro clips.
        </p>
      )}
      <ul>
        {reuses.map((reuse) => {
          const part = parts.find((item) => item.reuse === reuse.url);
          const moments = part?.highlights ?? [];
          return (
            <li key={reuse.url}>
              <div className="reused-row">
                <a href={reuse.url} target="_blank" rel="noreferrer" title={reuse.url}>
                  {part?.title || reuse.title || "Reused case"}
                </a>
                {part ? (
                  moments.length ? (
                    <button type="button" className="link-button" aria-expanded={open === reuse.url} onClick={() => setOpen(open === reuse.url ? null : reuse.url)}>
                      {moments.length} moments
                    </button>
                  ) : (
                    <span className="faint" title="Picked before the next build or request">Imported</span>
                  )
                ) : (
                  <span className="faint">{isDoc(reuse.url) ? (reuse.footage ? "Script Doc · linked only" : "Script Doc · lists no footage") : "Linked only"}</span>
                )}
              </div>
              {open === reuse.url && moments.length > 0 && (
                <ul className="moments">
                  {moments.map((moment) => (
                    <li key={moment.firstWordId} title={moment.why}>
                      <button type="button" className="icon-tool" aria-label="Play this moment" title="Play" onClick={() => onPlay(moment)}>
                        <Icon name="play" size={11} />
                      </button>
                      <span className="mono faint">{clock(moment.startMs - (part?.startMs ?? 0))}</span>
                      <span className="moment-quote">{moment.quote}</span>
                      <button type="button" className="tool" disabled={busy} title="Insert at the very start, as an intro clip" onClick={() => onInsert(moment)}>
                        <Icon name="plus" size={11} />
                        Intro
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
