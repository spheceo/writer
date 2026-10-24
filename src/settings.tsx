import { type ReactNode, useEffect, useState } from "react";
import { type About, clearStorage, doctor, getAbout, message, saveYoutubeSignIn, setUpdateChannel, storageReport, updateStatus, type UpdateStatus, youtubeSignIn } from "./api";
import type { Check, StorageReport } from "./types";
import { Dialog, Select } from "./ui";
import { AccountRow } from "./account";

const mb = (bytes: number) => (bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(0, Math.round(bytes / 1e6))} MB`);

/** A titled group of rows, as Edit's Settings lays them out. */
function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="settings-group">
      <h3>{title}</h3>
      <div className="settings-card">{children}</div>
    </section>
  );
}

function Row({ name, detail, bad, children }: { name: string; detail?: ReactNode; bad?: boolean; children?: ReactNode }) {
  return (
    <div className="settings-row">
      <div className="settings-what">
        <b>{name}</b>
        {detail && <span className={bad ? "bad" : "faint"}>{detail}</span>}
      </div>
      {children}
    </div>
  );
}

/**
 * Writer's settings: who's signed in, the one thing to install (Claude
 * Code, shown only when it needs something), the YouTube sign-in downloads
 * fall back on, storage, and updates. Everything else Writer looks after.
 */
export function SettingsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [checks, setChecks] = useState<Check[] | null>(null);
  const [about, setAbout] = useState<About | null>(null);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  // The YouTube sign-in downloads fall back on when the proxy fails.
  const [signIn, setSignIn] = useState<{ saved: boolean; savedAt: number | null } | null>(null);
  const [signingIn, setSigningIn] = useState<{ busy?: boolean; said: string; bad?: boolean } | null>(null);
  const [storage, setStorage] = useState<StorageReport | null>(null);
  const [clearing, setClearing] = useState(false);
  const [clearNote, setClearNote] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    void doctor().then(setChecks, () => setChecks([]));
    void getAbout().then(setAbout, () => {});
    void updateStatus().then(setUpdate, () => {});
    void youtubeSignIn().then(setSignIn, () => {});
    setSigningIn(null);
    setClearNote(null);
    void storageReport().then(setStorage, () => setStorage(null));
  }, [open]);

  // Only what needs the person's hand: everything else Writer installs and keeps up itself.
  const broken = checks?.filter((check) => !check.ok) ?? [];
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Settings"
      width={500}
      footer={
        <>
          {about && <span className="faint" title={about.release ? "Built by the release workflow" : "A dev build: it never updates itself"}>{about.name}</span>}
          <span className="spacer" />
          <button type="button" className="primary" onClick={onClose}>Done</button>
        </>
      }
    >
      <div className="settings">
        <AccountRow />
        {broken.length > 0 && (
          <Group title="Needs attention">
            {broken.map((check) => (
              <Row key={check.name} name={check.name} detail={check.detail} bad />
            ))}
          </Group>
        )}
        {signIn && (
          <Group title="YouTube">
            <Row
              name="Sign-in for downloads"
              bad={signingIn?.bad}
              detail={
                signingIn?.said ??
                (signIn.saved
                  ? `Saved ${new Date(signIn.savedAt ?? 0).toLocaleDateString()}. Used only when a download through Writer's proxy fails.`
                  : "Used only when a download through Writer's proxy fails. Copied once from Chrome, Firefox, Brave or Edge.")
              }
            >
              <button
                type="button"
                className="secondary sm"
                disabled={signingIn?.busy}
                onClick={() => {
                  setSigningIn({ busy: true, said: "Reading your browser's sign-in… macOS may ask for your password once." });
                  void saveYoutubeSignIn()
                    .then((browser) => {
                      setSigningIn({ said: `Saved from ${browser}.` });
                      return youtubeSignIn().then(setSignIn);
                    })
                    .catch((cause) => setSigningIn({ said: message(cause), bad: true }));
                }}
              >
                {signingIn?.busy ? "Saving…" : signIn.saved ? "Save again" : "Save sign-in"}
              </button>
            </Row>
          </Group>
        )}
        {storage && (
          <Group title="Storage">
            <Row
              name={`${mb(storage.totalBytes)} on this Mac`}
              detail={
                clearNote ??
                (storage.reclaimableBytes > 0
                  ? `${mb(storage.reclaimableBytes)} of footage is safe in the cloud and can go.`
                  : storage.blockedBytes > 0
                    ? `${mb(storage.blockedBytes)} is still uploading; it stays until the cloud has it.`
                    : "Footage lives in the cloud. Previews come back when you open a script.")
              }
            >
              <button
                type="button"
                className="secondary sm"
                disabled={clearing || storage.reclaimableBytes === 0}
                onClick={() => {
                  setClearing(true);
                  void clearStorage()
                    .then((done) => {
                      setClearNote(done.freedBytes > 0 ? `Cleared ${mb(done.freedBytes)}.` : "Nothing to clear yet.");
                      return storageReport().then(setStorage, () => {});
                    })
                    .catch(() => setClearNote("Couldn't clear: check the connection, then try again."))
                    .finally(() => setClearing(false));
                }}
              >
                {clearing ? "Clearing…" : "Clear"}
              </button>
            </Row>
          </Group>
        )}
        {update && (
          <Group title="Updates">
            <Row
              name="Channel"
              detail={
                !update.enabled
                  ? "This is a dev build: it never updates itself."
                  : update.offer
                    ? `${update.offer.tag} is ready: use Update at the top of the list.`
                    : update.checkError
                      ? `Couldn't check just now: ${update.checkError}`
                      : "Up to date. Writer looks for a new build every minute."
              }
            >
              <Select
                value={update.channel}
                disabled={!update.enabled}
                label="Update channel"
                align="end"
                options={[
                  { value: "nightly", label: "Nightly", hint: "every build" },
                  { value: "stable", label: "Stable", hint: "releases only" },
                ]}
                onChange={(channel) => void setUpdateChannel(channel).then(setUpdate, () => {})}
              />
            </Row>
          </Group>
        )}
      </div>
    </Dialog>
  );
}
