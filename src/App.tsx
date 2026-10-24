import { useEffect, useRef, useState } from "react";
import { claudeStatus, getProgress, getScript, listScripts, message, onLibrary } from "./api";
import { CommentaryEditor } from "./commentary-view";
import { Editor } from "./editor";
import { Library } from "./library";
import { Icon } from "./ui";
import { SettingsDialog } from "./settings";
import { LogsDialog } from "./logs";
import { SyncDock } from "./sync-dock";
import { LitStyle } from "./comments";
import { UpdatePill } from "./update";
import { ImportScreen } from "./import-screen";
import { SignInScreen, useAccount } from "./account";
import type { ClaudeStatus, ImportProgress, Script, Summary } from "./types";

type Route = { name: "library" } | { name: "script"; id: string };

// The route lives in the URL, so a reload (a phone or laptop on the tailnet
// does that often) comes back to the same script.
function parse(hash: string): Route {
  const script = /^#\/script\/([0-9a-f-]+)$/i.exec(hash);
  if (script) return { name: "script", id: script[1] };
  return { name: "library" };
}

const hashOf = (route: Route) => (route.name === "script" ? `#/script/${route.id}` : "#/");

export function App() {
  const [route, setRouteState] = useState<Route>(() => parse(window.location.hash));
  const [ready, setReady] = useState(false);
  const [scripts, setScripts] = useState<Summary[]>([]);
  const [claude, setClaude] = useState<ClaudeStatus | null>(null);
  const [progress, setProgress] = useState<ImportProgress[]>([]);
  const [open, setOpen] = useState<Script | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState(false);
  const [logs, setLogs] = useState(false);
  const [account] = useAccount();
  // ⌘⇧L opens the Logs window, ⌘, Settings, from anywhere.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === "l") {
        event.preventDefault();
        setLogs(true);
      } else if ((event.metaKey || event.ctrlKey) && !event.shiftKey && event.key === ",") {
        event.preventDefault();
        setSettings(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const setRoute = (next: Route) => {
    setRouteState(next);
    const hash = hashOf(next);
    if (window.location.hash !== hash) window.history.pushState(null, "", hash);
  };

  useEffect(() => {
    const onPop = () => setRouteState(parse(window.location.hash));
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  const refresh = () => {
    void listScripts().then(setScripts).catch((cause) => setError(message(cause)));
  };

  useEffect(() => {
    // `just dev` serves the page before the Rust API is listening. Keep
    // asking until it answers rather than showing the refused request.
    let waiting: number | undefined;
    const connect = () => {
      void listScripts()
        .then((rows) => {
          setScripts(rows);
          setError(null);
          setReady(true);
          void claudeStatus().then(setClaude).catch(() => setClaude(null));
        })
        .catch(() => {
          setError("Starting Writer…");
          waiting = window.setTimeout(connect, 1500);
        });
    };
    connect();
    // Hidden windows stop asking; the next look catches up.
    const timer = window.setInterval(() => {
      if (document.hidden) return;
      void getProgress()
        .then((list) => setProgress(Array.isArray(list) ? list : []))
        .catch(() => {});
    }, 800);
    return () => {
      window.clearTimeout(waiting);
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    if (route.name !== "script" || !ready) return;
    void getScript(route.id).then(setOpen).catch((cause) => setError(message(cause)));
  }, [route, ready]);

  // A script open here that another Mac changed comes down through the cloud
  // into its file; the open copy follows it.
  const opened_ = route.name === "script" && open && open.id === route.id ? open : null;
  const shown = useRef(opened_);
  shown.current = opened_;
  useEffect(() => {
    if (route.name !== "script" || !ready) return;
    const timer = window.setInterval(() => {
      const now = shown.current;
      if (!now || document.hidden) return;
      void getScript(now.id)
        .then((fresh) => {
          const current = shown.current;
          if (current && fresh.id === current.id && fresh.updatedAt > current.updatedAt && fresh.revision !== current.revision) setOpen(fresh);
        })
        .catch(() => {});
    }, 8000);
    return () => window.clearInterval(timer);
  }, [route, ready]);

  // Scripts that come, go or change behind the list's back (the cloud, the
  // local API) show up straight away, and the list catches up when the window comes back.
  useEffect(() => {
    const stop = onLibrary(refresh);
    const back = () => !document.hidden && refresh();
    document.addEventListener("visibilitychange", back);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", back);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // While something imports or Claude is writing, keep the list current. The
  // request that started it may have been dropped on the way.
  const working = progress.length > 0 || scripts.some((row) => row.status === "processing" || row.building);
  useEffect(() => {
    if (!working || route.name !== "library") return;
    const timer = window.setInterval(() => !document.hidden && refresh(), 4000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [working, route.name]);

  const home = () => {
    setRoute({ name: "library" });
    refresh();
  };
  // Who's signed in decides which scripts they see (can.rs): list them again when that changes.
  const grantsKey = account ? `${account.signedIn}|${account.person?.sub ?? ""}|${account.person?.admin ?? ""}|${(account.person?.perms ?? []).join(",")}` : "";
  useEffect(() => {
    if (grantsKey) refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [grantsKey]);
  const opened = route.name === "script" && open && open.id === route.id ? open : null;
  // The open script still importing: its progress shows over the project, which loads when it's done.
  const importing = route.name === "script" ? (progress.find((item) => item.id === route.id) ?? null) : null;
  const wasImporting = useRef(false);
  useEffect(() => {
    if (route.name !== "script" || !ready) return;
    if (importing) {
      wasImporting.current = true;
      // Its script.json may not be there for the first moments.
      if (!opened) void getScript(route.id).then(setOpen).catch(() => {});
      return;
    }
    if (wasImporting.current) {
      wasImporting.current = false;
      void getScript(route.id).then(setOpen).catch((cause) => setError(message(cause)));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [!!importing, importing?.stage, route, ready]);
  // Signed out, the whole window is the sign-in.
  if (account && !account.signedIn) {
    return (
      <>
        <SignInScreen />
        <UpdatePill />
      </>
    );
  }
  return (
    <>
      {opened?.commentary ? (
        <CommentaryEditor script={opened} onChange={setOpen} onBack={home} />
      ) : opened ? (
        <Editor script={opened} onChange={setOpen} onBack={home} />
      ) : route.name === "script" ? (
        <div className="studio">
          <header className="topbar" data-tauri-drag-region>
            <button type="button" className="icon-tool" aria-label="All scripts" title="All scripts" onClick={home}>
              <Icon name="back" />
            </button>
            <div className="title-block" data-tauri-drag-region>Scripts</div>
          </header>
          <p className={error ? "notice" : "empty"}>{error ?? "Opening…"}</p>
        </div>
      ) : (
        <Library
          scripts={scripts}
          claude={claude}
          onSettings={() => setSettings(true)}
          onLogs={() => setLogs(true)}
          progress={progress}
          loadError={error}
          onOpen={(id) => setRoute({ name: "script", id })}
          onChanged={refresh}
        />
      )}
      <ImportScreen item={importing} onLeave={home} />
      <SettingsDialog open={settings} onClose={() => setSettings(false)} />
      <LitStyle />
      <SyncDock onOpen={(id) => setRoute({ name: "script", id })} />
      <LogsDialog open={logs} onClose={() => setLogs(false)} />
    </>
  );
}
