import { useEffect, useState } from "react";
import { installUpdate, message, updateStatus, type UpdateStatus } from "./api";
import { Icon } from "./ui";

/** The update pill: "Update to nightly-2", one click installs it and restarts Writer. */
export function UpdatePill() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
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
  if (!status?.enabled || !status.offer) return null;
  const working = status.phase !== "idle";
  const label = status.phase === "downloading" ? "Downloading…" : status.phase === "installing" ? "Installing…" : status.phase === "restarting" ? "Restarting…" : `Update to ${status.offer.tag}`;
  return (
    <div className="update-pill">
      <button
        type="button"
        className="primary sm"
        disabled={working}
        title={`${status.name} → ${status.offer.tag}. Installs it and restarts Writer.${problem ?? status.error ? `\n${problem ?? status.error}` : ""}`}
        onClick={() => {
          setProblem(null);
          setStatus({ ...status, phase: "downloading" });
          void installUpdate().catch((cause) => setProblem(message(cause)));
        }}
      >
        <Icon name="send" size={13} />
        {label}
      </button>
      {(problem ?? status.error) && !working && <span className="bad">{problem ?? status.error}</span>}
    </div>
  );
}
