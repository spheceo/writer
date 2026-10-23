import { useEffect, useState } from "react";
import { type About, doctor, getAbout, setUpdateChannel, updateStatus, type UpdateStatus } from "./api";
import type { Check } from "./types";
import { Dialog } from "./ui";

/**
 * What Writer needs on this Mac. Only Claude Code is the user's to install;
 * the relay (and every key on it) is built into the app, out of sight.
 */
export function SettingsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [checks, setChecks] = useState<Check[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [about, setAbout] = useState<About | null>(null);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);

  const run = () => {
    setBusy(true);
    void doctor()
      .then(setChecks)
      .catch(() => setChecks([]))
      .finally(() => setBusy(false));
  };

  useEffect(() => {
    if (!open) return;
    setChecks(null);
    run();
    void getAbout().then(setAbout, () => {});
    void updateStatus().then(setUpdate, () => {});
  }, [open]);

  const broken = checks?.filter((check) => !check.ok).length ?? 0;
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Setup"
      description="Claude Code is the one thing to install. ffmpeg comes with Writer, and Writer keeps its own yt-dlp up to date."
      width={480}
      footer={
        <>
          {checks && (
            <span className={broken ? "bad" : "ok"}>{broken ? `${broken} to fix` : "All set"}</span>
          )}
          {about && <span className="faint" title={about.release ? "Built by the release workflow" : "A dev build: it never updates itself"}>{about.name}</span>}
          <span className="spacer" />
          <button type="button" className="ghost" disabled={busy} onClick={run}>
            {busy ? "Checking…" : "Check again"}
          </button>
          <button type="button" className="primary" onClick={onClose}>Done</button>
        </>
      }
    >
      {checks ? (
        <ul className="checks">
          {checks.map((check) => (
            <li key={check.name}>
              <span className={check.ok ? "dot on" : "dot bad"} />
              <b>{check.name}</b>
              <span className={check.ok ? "faint" : "bad"} title={check.detail}>{check.detail}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="faint">Checking…</p>
      )}
      {update && (
        <div className="update-channel">
          <b>Updates</b>
          <select
            value={update.channel}
            disabled={!update.enabled}
            aria-label="Update channel"
            onChange={(event) => void setUpdateChannel(event.target.value as "nightly" | "stable").then(setUpdate, () => {})}
          >
            <option value="nightly">Nightly: every build as it's made</option>
            <option value="stable">Stable: releases only</option>
          </select>
          <span className="faint">
            {!update.enabled ? "This is a dev build: it never updates itself." : update.offer ? `${update.offer.tag} is ready to install.` : "Up to date."}
          </span>
        </div>
      )}
    </Dialog>
  );
}
