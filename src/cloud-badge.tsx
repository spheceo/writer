import { useEffect, useState } from "react";
import { type CloudStatus, cloudStatus } from "./api";
import { Icon } from "./ui";

const megabytes = (bytes: number) => (bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`);

/** The cloud's state, asked every few seconds: more often while files move. */
export function useCloud() {
  const [status, setStatus] = useState<CloudStatus | null>(null);
  useEffect(() => {
    let live = true;
    let timer = 0;
    const ask = () => {
      void cloudStatus()
        .then((next) => live && setStatus(next))
        .catch(() => {})
        .finally(() => {
          if (live) timer = window.setTimeout(ask, document.hidden ? 15000 : 2500);
        });
    };
    ask();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, []);
  return status;
}

function describe(status: CloudStatus | null): { tone: string; label: string; title: string } {
  if (!status || !status.state) return { tone: "idle", label: "Cloud", title: "Checking the cloud…" };
  const when = status.lastSync ? new Date(status.lastSync).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : null;
  switch (status.state) {
    case "off":
      return { tone: "idle", label: "Cloud off", title: "This copy of Writer has no relay token, so scripts stay on this Mac only." };
    case "offline":
      return { tone: "warn", label: "Offline", title: `Saved on this Mac; it syncs when the connection is back.${status.error ? `\n${status.error}` : ""}` };
    case "error":
      return { tone: "bad", label: "Sync problem", title: status.error ?? "The cloud refused something." };
    case "syncing": {
      const moving = status.transfer;
      const percent = moving && moving.total ? Math.round((moving.done / moving.total) * 100) : null;
      return {
        tone: "busy",
        label: `${moving ? `${moving.up ? "Uploading" : "Downloading"} ${percent ?? 0}%` : "Syncing"}${status.pending ? ` · ${status.pending} left` : ""}`,
        title: moving
          ? `${moving.up ? "Uploading" : "Downloading"} ${moving.name}: ${megabytes(moving.done)} of ${megabytes(moving.total)}${status.pending ? `\n${status.pending} more waiting` : ""}`
          : "Syncing with the cloud…",
      };
    }
    case "synced":
      return {
        tone: "ok",
        label: "Synced",
        title: `Every script is in the cloud (${status.scripts}), shared with your other Macs.${when ? `\nLast synced ${when}.` : ""}`,
      };
    default:
      return { tone: "idle", label: "Cloud", title: "Checking the cloud…" };
  }
}

/** The cloud's state as a small badge: in full on the list, as an icon in an editor. */
export function CloudBadge({ compact = false }: { compact?: boolean }) {
  const status = useCloud();
  const { tone, label, title } = describe(status);
  return (
    <span className={`cloud-badge ${tone}${compact ? " compact" : ""}`} title={title} role="status" aria-label={`Cloud: ${label}`}>
      <Icon name="cloud" size={compact ? 16 : 15} />
      {!compact && <span>{label}</span>}
      <i aria-hidden="true" />
    </span>
  );
}
