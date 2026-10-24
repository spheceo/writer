import { useEffect, useState } from "react";
import { installUpdate, message, updateStatus, type UpdateStatus } from "./api";
import { Icon } from "./ui";

/** Where an update is: asked every 15 s (Writer itself looks at GitHub every minute). */
function useUpdateStatus() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  useEffect(() => {
    let live = true;
    const ask = () => void updateStatus().then((next) => live && setStatus(next), () => {});
    ask();
    const timer = window.setInterval(ask, 15_000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, []);
  return [status, setStatus] as const;
}

/** "Update to nightly-2", as Edit's: one click installs it and restarts Writer. Nothing when Writer is up to date. */
export function UpdateButton({ floating = false }: { floating?: boolean }) {
  const [status, setStatus] = useUpdateStatus();
  const [problem, setProblem] = useState<string | null>(null);
  if (!status?.enabled || !status.offer) return null;
  const working = status.phase !== "idle";
  const label = status.phase === "downloading" ? "Downloading…" : status.phase === "installing" ? "Installing…" : status.phase === "restarting" ? "Restarting…" : `Update to ${status.offer.tag}`;
  const said = problem ?? status.error;
  return (
    <div className={floating ? "update-pill" : "update-inline"}>
      <button
        type="button"
        className="secondary"
        disabled={working}
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

/** The same, at the bottom right of screens with no room in their header. */
export function UpdatePill() {
  return <UpdateButton floating />;
}
