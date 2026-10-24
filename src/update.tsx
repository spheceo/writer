import { type CSSProperties, useEffect, useState } from "react";
import { getActivity, installUpdate, message, updateStatus, type UpdateStatus } from "./api";
import { Icon } from "./ui";

/** Where an update is: asked every 15 s (Writer itself looks at GitHub every minute), every second while one installs. */
function useUpdateStatus() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const working = !!status && status.phase !== "idle";
  useEffect(() => {
    let live = true;
    const ask = () => void updateStatus().then((next) => live && setStatus(next), () => {});
    ask();
    const timer = window.setInterval(ask, working ? 1000 : 15_000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [working]);
  return [status, setStatus] as const;
}

/** How far the update's download has got, 0 to 1, read while it runs. */
function useDownloaded(running: boolean) {
  const [share, setShare] = useState(0);
  useEffect(() => {
    if (!running) return;
    let live = true;
    const ask = () =>
      void getActivity().then((work) => {
        const item = work.find((found) => found.key === "update");
        if (live && item?.total) setShare(Math.min(1, item.done / item.total));
      }, () => {});
    ask();
    const timer = window.setInterval(ask, 500);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [running]);
  return share;
}

/**
 * "Update to nightly-2", as Edit's, in the header: one click installs it and
 * restarts Writer, the button filling up as it downloads. Nothing when Writer
 * is up to date.
 */
export function UpdateButton({ floating = false }: { floating?: boolean }) {
  const [status, setStatus] = useUpdateStatus();
  const [problem, setProblem] = useState<string | null>(null);
  const share = useDownloaded(status?.phase === "downloading");
  if (!status?.enabled || !status.offer) return null;
  const working = status.phase !== "idle";
  const label = status.phase === "downloading" ? "Updating…" : status.phase === "installing" ? "Installing…" : status.phase === "restarting" ? "Restarting…" : `Update to ${status.offer.tag}`;
  const filled = status.phase === "downloading" ? share : working ? 1 : 0;
  const said = problem ?? status.error;
  return (
    <div className={floating ? "update-pill" : "update-inline"}>
      <button
        type="button"
        className="secondary update-button"
        disabled={working}
        data-working={working || undefined}
        style={{ "--filled": `${Math.round(filled * 100)}%` } as CSSProperties}
        title={`${status.name} → ${status.offer.tag}. Installs it and restarts Writer; your work is saved.${said ? `\n${said}` : ""}`}
        onClick={() => {
          setProblem(null);
          setStatus({ ...status, phase: "downloading" });
          void installUpdate().catch((cause) => setProblem(message(cause)));
        }}
      >
        <Icon name="update-up" size={14} />
        {label}
      </button>
      {said && !working && <span className="bad">{said}</span>}
    </div>
  );
}

/** The same, at the top right of the sign-in window, which has no header. */
export function UpdatePill() {
  return <UpdateButton floating />;
}
