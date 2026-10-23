import { useEffect, useState } from "react";
import { type About, doctor, getAbout, getSettings, setEngine, setUpdateChannel, updateStatus, type UpdateStatus } from "./api";
import type { Check } from "./types";
import { Dialog } from "./ui";
import { AccountRow } from "./account";

/**
 * What Writer needs on this Mac. Only Claude Code is the user's to install;
 * the relay (and every key on it) is built into the app, out of sight, and
 * answers whoever is signed in.
 */
export function SettingsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [checks, setChecks] = useState<Check[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [about, setAbout] = useState<About | null>(null);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const [engine, setEngineShown] = useState<"cli" | "sdk" | null>(null);

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
    void getSettings().then((settings) => setEngineShown(settings.engine ?? "cli"), () => {});
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
      <AccountRow />
      {engine && (
        <div className="update-channel">
          <b>Claude</b>
          <select value={engine} aria-label="How Writer reaches Claude" onChange={(event) => void setEngine(event.target.value as "cli" | "sdk").then((settings) => setEngineShown(settings.engine ?? "cli"), () => {})}>
            <option value="cli">One answer (claude -p)</option>
            <option value="sdk">Agent SDK, building in steps (test)</option>
          </select>
        </div>
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
