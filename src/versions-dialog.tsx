import { useEffect, useState } from "react";
import { getVersion, listVersions, message } from "./api";
import { clock, isCutIn, keptMs } from "./clips";
import { wordDiff } from "./diff";
import type { Script, Snapshot, Version } from "./types";
import { Dialog } from "./ui";

const when = (at: number) => new Date(at).toLocaleString(undefined, { day: "numeric", month: "short", hour: "numeric", minute: "2-digit" });
const WHO: Record<Version["author"], string> = { you: "You", claude: "Claude", before: "Before" };

/**
 * The script's versions, as Repack lists them: newest first, who made each
 * and what changed. One opened shows how its narration differs from the
 * script now, and can be restored (itself saved as a new version).
 */
export function VersionsDialog({
  open,
  script,
  busy,
  onClose,
  onRestore,
}: {
  open: boolean;
  script: Script;
  busy: boolean;
  onClose: () => void;
  onRestore: (version: Version) => void;
}) {
  const [versions, setVersions] = useState<Version[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [picked, setPicked] = useState<Version | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);

  useEffect(() => {
    if (!open) return;
    setPicked(null);
    setSnapshot(null);
    setProblem(null);
    listVersions(script.id).then(
      (list) => setVersions([...list].reverse()),
      (cause) => setProblem(message(cause)),
    );
  }, [open, script.id]);

  useEffect(() => {
    if (!picked) return;
    setSnapshot(null);
    getVersion(script.id, picked.id).then(setSnapshot, (cause) => setProblem(message(cause)));
  }, [picked, script.id]);

  // How the version's narration reads against the script now, cut-in by cut-in.
  const narration = (() => {
    if (!snapshot) return [];
    const now = new Map(script.clips.filter(isCutIn).map((clip) => [clip.id, clip.narration?.trim() ?? ""]));
    return snapshot.clips.filter(isCutIn).flatMap((clip, index) => {
      const then = clip.narration?.trim() ?? "";
      const current = now.get(clip.id);
      if (current === then) return [];
      return [{ key: clip.id, label: `Cut-in ${index + 1}`, current: current ?? "", then, gone: current === undefined }];
    });
  })();

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Versions"
      description="Every save keeps the script as it stood; edits by the same author within ten minutes are one version."
      width={picked ? 720 : 520}
      footer={
        <>
          {problem && <span className="bad dialog-error">{problem}</span>}
          <span className="spacer" />
          {picked && (
            <button type="button" className="ghost" onClick={() => setPicked(null)}>
              All versions
            </button>
          )}
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
          {picked && (
            <button type="button" className="primary" disabled={busy || !snapshot || picked.id === versions?.[0]?.id} title="Make this version the script again; the restore is saved as a new version" onClick={() => onRestore(picked)}>
              Restore this version
            </button>
          )}
        </>
      }
    >
      {!picked ? (
        !versions ? (
          <p className="hint">Reading versions…</p>
        ) : !versions.length ? (
          <p className="hint">No versions yet. The first edit or build starts them.</p>
        ) : (
          <ul className="version-list">
            {versions.map((version, index) => (
              <li key={version.id}>
                <button type="button" className="version-row" onClick={() => setPicked(version)}>
                  <span className={`version-who ${version.author}`}>{WHO[version.author]}</span>
                  <span className="version-what">
                    {version.restored ? `Restored the version from ${when(version.restored)}` : version.summary}
                    {index === 0 && <span className="faint"> · current</span>}
                  </span>
                  <span className="faint">{when(version.at)}</span>
                </button>
              </li>
            ))}
          </ul>
        )
      ) : (
        <div className="version-detail">
          <p>
            <b>{WHO[picked.author]}</b> · {when(picked.at)} · {picked.restored ? `Restored the version from ${when(picked.restored)}` : picked.summary}
          </p>
          {!snapshot ? (
            <p className="hint">Opening…</p>
          ) : (
            <>
              <p className="hint">
                Then: {clock(keptMs(snapshot.clips))} · {snapshot.clips.filter(isCutIn).length} cut-ins. Now: {clock(keptMs(script.clips))} · {script.clips.filter(isCutIn).length} cut-ins.
              </p>
              {!narration.length ? (
                <p className="hint">Its narration reads as the script's does now.</p>
              ) : (
                <ul className="version-diffs">
                  {narration.map((item) => (
                    <li key={item.key}>
                      <span className="faint">
                        {item.label}
                        {item.gone ? " · not in the script now" : ""}
                      </span>
                      <p className="proposal-text">
                        {wordDiff(item.current, item.then).map((piece, at) =>
                          piece.kind === "same" ? <span key={at}>{piece.text}</span> : piece.kind === "add" ? <ins key={at}>{piece.text}</ins> : <del key={at}>{piece.text}</del>,
                        )}
                      </p>
                    </li>
                  ))}
                </ul>
              )}
              <p className="hint">Marked words: struck through is in the script now, highlighted is how this version read.</p>
            </>
          )}
        </div>
      )}
    </Dialog>
  );
}
