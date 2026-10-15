import { open as pickFile } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useEffect, useRef, useState } from "react";
import {
  createCommentary,
  deleteScript,
  importDrive,
  importFile,
  importCompilation,
  importYoutube,
  inTauri,
  message,
  renameScript,
  resumeImport,
} from "./api";
import { clock } from "./clips";
import { SettingsDialog } from "./settings";
import type { Choice, ClaudeStatus, ImportProgress, Script, Summary } from "./types";
import { Dialog, Icon, Menu, Tabs, type ToastMessage, Toasts } from "./ui";

type Source = "youtube" | "drive" | "file" | "compilation" | "commentary";

function ago(at: number) {
  const minutes = Math.round((Date.now() - at) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Date(at).toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

function Status({ script, progress }: { script: Summary; progress: ImportProgress[] }) {
  if (script.status === "processing") {
    const percent = progress.find((item) => item.id === script.id)?.percent ?? script.progress;
    return <span className="status busy"><i />Importing {percent}%</span>;
  }
  if (script.building) return <span className="status busy"><i />Writing…</span>;
  if (script.status === "needs_choice") return <span className="status ask"><i />Needs your choice</span>;
  if (script.status === "error") return <span className="status bad"><i />Failed</span>;
  if (script.sourceKind === "commentary") {
    return script.draftWords > 0 ? <span className="status done"><i />Script</span> : <span className="status ready"><i />Sources</span>;
  }
  if (script.cutInCount > 0) return <span className="status done"><i />Script</span>;
  if (script.wordCount > 0) return <span className="status ready"><i />Transcribed</span>;
  return <span className="status"><i />No transcript</span>;
}

export function Library({
  scripts,
  claude,
  progress,
  loadError,
  onOpen,
  onChanged,
}: {
  scripts: Summary[];
  claude: ClaudeStatus | null;
  progress: ImportProgress[];
  loadError: string | null;
  onOpen: (id: string) => void;
  onChanged: () => void;
}) {
  const [creating, setCreating] = useState(false);
  const [settings, setSettings] = useState(false);
  const [renaming, setRenaming] = useState<Summary | null>(null);
  const [deleting, setDeleting] = useState<Summary | null>(null);
  const [recovering, setRecovering] = useState<Summary | null>(null);
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const next = useRef(0);

  const toast = (text: ToastMessage["text"], action?: ToastMessage["action"]) =>
    setToasts((list) => [...list, { id: ++next.current, text, action }]);
  const dismiss = (id: number) => setToasts((list) => list.filter((item) => item.id !== id));

  // An import whose script the list hasn't caught up with yet.
  const importing = progress.filter((item) => !scripts.some((script) => script.id === item.id));
  const empty = scripts.length === 0 && !importing.length;

  return (
    <div className="app">
      <header className="top home" data-tauri-drag-region>
        <span className="brand" data-tauri-drag-region><i />Writer</span>
        <span className="spacer" data-tauri-drag-region />
        <span className="claude" title={claude?.message}>
          <span className={claude?.loggedIn ? "dot on" : "dot"} />
          Claude
        </span>
        <button type="button" className="icon-button" aria-label="Settings" title="Settings" onClick={() => setSettings(true)}>
          <Icon name="settings" />
        </button>
        <button type="button" className="primary sm" onClick={() => setCreating(true)}>
          <Icon name="plus" size={14} />
          New script
        </button>
      </header>
      {loadError && <div className="notice">{loadError}</div>}
      <main className="page">
        {empty ? (
          <div className="welcome">
            <img src="/favicon.svg" alt="" width={44} height={44} />
            <h2>No scripts yet</h2>
            <p>Bring in an interrogation from YouTube, Google Drive or this Mac, or start a commentary from its sources, and Claude writes the script.</p>
            <button type="button" className="primary" onClick={() => setCreating(true)}>
              <Icon name="plus" size={14} />
              New script
            </button>
          </div>
        ) : (
          <div className="column">
            <div className="list-head">
              <h2>Scripts</h2>
              <span className="faint">{scripts.length}</span>
            </div>
            <div className="list">
              {importing.map((item) => (
                <div className="item" key={item.id} style={{ ["--i" as string]: 0 }}>
                  <div className="open">
                    <strong>{item.title}</strong>
                    <span className="meta">{item.detail}</span>
                    <div className="bar"><span style={{ width: `${item.percent}%` }} /></div>
                  </div>
                  <span className="status busy"><i />Importing {item.percent}%</span>
                  <span className="menu-spacer" />
                </div>
              ))}
              {scripts.map((script, index) => {
                const live = script.status === "processing" ? progress.find((item) => item.id === script.id) ?? null : null;
                const locked = script.status === "processing" || script.building;
                return (
                  <div className="item" key={script.id} style={{ ["--i" as string]: Math.min(index, 12) }}>
                    <button type="button" className="open" onClick={() => (stuck(script) ? setRecovering(script) : onOpen(script.id))}>
                      <strong>{script.title}</strong>
                      <span className="meta">
                        {live ? (
                          live.detail
                        ) : script.status === "needs_choice" && script.decision ? (
                          <span className="ask">{script.decision.summary}</span>
                        ) : script.status === "error" ? (
                          <span className="bad">{script.error}</span>
                        ) : (
                          [
                            script.sourceKind === "commentary" ? "Commentary" : null,
                            script.durationMs ? clock(script.durationMs) : null,
                            script.cutInCount ? `${script.cutInCount} cut-ins` : null,
                            script.draftWords ? `${script.draftWords} words` : null,
                            `updated ${ago(script.updatedAt)}`,
                          ]
                            .filter(Boolean)
                            .join("  ·  ")
                        )}
                      </span>
                      {live && <div className="bar"><span style={{ width: `${live.percent}%` }} /></div>}
                    </button>
                    <Status script={script} progress={progress} />
                    <Menu
                      label={`More for ${script.title}`}
                      items={[
                        ...(stuck(script)
                          ? [{ label: script.status === "needs_choice" ? "Answer…" : "Retry import…", onSelect: () => setRecovering(script) }]
                          : []),
                        { label: "Open", onSelect: () => onOpen(script.id) },
                        { label: "Rename…", onSelect: () => setRenaming(script) },
                        {
                          label: "Delete…",
                          danger: true,
                          disabled: locked,
                          hint: locked ? "busy" : undefined,
                          onSelect: () => setDeleting(script),
                        },
                      ]}
                    />
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </main>

      <ImportDialog
        open={creating}
        onClose={() => setCreating(false)}
        onStarted={onChanged}
        onFinished={(script) => {
          onChanged();
          toast(<><b>{script.title}</b> is ready.</>, { label: "Open", onClick: () => onOpen(script.id) });
        }}
        onFailed={(reason) => {
          onChanged();
          toast(<span className="bad">{reason}</span>);
        }}
        onOpenNow={(script) => onOpen(script.id)}
      />
      <RenameDialog
        script={renaming}
        onClose={() => setRenaming(null)}
        onRenamed={() => {
          setRenaming(null);
          onChanged();
        }}
      />
      <DeleteDialog
        script={deleting}
        onClose={() => setDeleting(null)}
        onDeleted={(title) => {
          setDeleting(null);
          onChanged();
          toast(<>Deleted <b>{title}</b>.</>);
        }}
      />
      <RecoverDialog
        script={recovering}
        onClose={() => setRecovering(null)}
        onStarted={() => {
          setRecovering(null);
          onChanged();
        }}
        onFinished={(script) => {
          onChanged();
          if (script.status === "ready") {
            toast(<><b>{script.title}</b> is ready.</>, { label: "Open", onClick: () => onOpen(script.id) });
          } else if (script.status === "needs_choice") {
            toast(<><b>{script.title}</b> needs your choice.</>);
          }
        }}
        onFailed={(reason) => {
          onChanged();
          toast(<span className="bad">{reason}</span>);
        }}
      />
      <SettingsDialog open={settings} onClose={() => setSettings(false)} />
      <Toasts toasts={toasts} onDismiss={dismiss} />
    </div>
  );
}

/** An import that stopped: failed, or paused on a question. */
const stuck = (script: Summary) =>
  script.status === "needs_choice" || (script.status === "error" && script.wordCount === 0);

const hours = (ms: number) => {
  const total = Math.round(ms / 60_000);
  return total >= 60 ? `${Math.floor(total / 60)} h ${total % 60} min` : `${total} min`;
};

/**
 * Answers a stopped import. A failed one can be retried from what it kept;
 * a paused one shows its question, as Repack asks it on the script.
 */
function RecoverDialog({
  script,
  onClose,
  onStarted,
  onFinished,
  onFailed,
}: {
  script: Summary | null;
  onClose: () => void;
  onStarted: () => void;
  onFinished: (script: Script) => void;
  onFailed: (reason: string) => void;
}) {
  const shown = useRef(script);
  if (script) shown.current = script;
  const current = shown.current;
  const decision = current?.status === "needs_choice" ? current.decision : undefined;
  const [picked, setPicked] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setError(null);
    setBusy(false);
    if (script?.decision?.kind !== "too_much") return;
    // Start from the recordings that fit, in order, as far as the limits go.
    const { files, maxFiles, maxMs } = script.decision;
    const chosen: string[] = [];
    let total = 0;
    for (const file of files) {
      if (file.tooLong || chosen.length >= maxFiles || total + (file.durationMs ?? 0) > maxMs) continue;
      chosen.push(file.id);
      total += file.durationMs ?? 0;
    }
    setPicked(chosen);
  }, [script]);

  const answer = async (choice: Choice) => {
    if (!current) return;
    setBusy(true);
    setError(null);
    const outcome = resumeImport(current.id, choice).then(
      (next) => ({ kind: "done" as const, next }),
      (cause: unknown) => ({ kind: "failed" as const, cause }),
    );
    const first = await Promise.race([outcome, wait(2500).then(() => ({ kind: "running" as const }))]);
    if (first.kind === "failed" && !(first.cause instanceof TypeError)) {
      setError(message(first.cause));
      setBusy(false);
      return;
    }
    setBusy(false);
    onStarted();
    void outcome.then((result) => {
      if (result.kind === "done") onFinished(result.next);
      else if (!(result.cause instanceof TypeError)) onFailed(message(result.cause));
    });
  };

  const foot = (buttons: ReactNode) => (
    <>
      {error && <span className="bad dialog-error">{error}</span>}
      <span className="spacer" />
      {buttons}
    </>
  );

  if (decision?.kind === "too_much") {
    const chosen = decision.files.filter((file) => picked.includes(file.id));
    const total = chosen.reduce((sum, file) => sum + (file.durationMs ?? 0), 0);
    const over = chosen.length > decision.maxFiles || total > decision.maxMs;
    return (
      <Dialog
        open={!!script}
        onClose={onClose}
        title="Choose the recordings"
        description={decision.summary}
        width={540}
        footer={foot(
          <>
            <button type="button" className="ghost" onClick={() => void answer({ kind: "cancel" })}>Cancel import</button>
            <button
              type="button"
              className="primary"
              disabled={busy || !chosen.length || over}
              onClick={() => void answer({ kind: "select", ids: picked })}
            >
              {over ? "Too much selected" : `Import ${chosen.length}`}
            </button>
          </>,
        )}
      >
        <p className="hint">
          {chosen.length} of at most {decision.maxFiles} recordings · {hours(total)} of at most {hours(decision.maxMs)}
        </p>
        <div className="checklist">
          {decision.files.map((file) => (
            <label key={file.id} className={file.tooLong ? "off" : ""}>
              <input
                type="checkbox"
                disabled={file.tooLong}
                checked={picked.includes(file.id)}
                onChange={(event) =>
                  setPicked((list) => (event.target.checked ? [...list, file.id] : list.filter((id) => id !== file.id)))
                }
              />
              <span>{file.title}</span>
              <span className="faint">
                {file.tooLong ? "too long" : file.durationMs ? hours(file.durationMs) : `${Math.round(file.size / 1e6)} MB`}
              </span>
            </label>
          ))}
        </div>
      </Dialog>
    );
  }

  if (decision?.kind === "failed_recordings") {
    const one = decision.failed.length === 1;
    return (
      <Dialog
        open={!!script}
        onClose={onClose}
        title={one ? "A recording didn't come through" : `${decision.failed.length} recordings didn't come through`}
        description={decision.summary}
        width={500}
        footer={foot(
          <>
            <button type="button" className="ghost" onClick={() => void answer({ kind: "cancel" })}>Cancel import</button>
            <button type="button" disabled={busy} onClick={() => void answer({ kind: "retry" })}>
              {one ? "Retry it" : "Retry them"}
            </button>
            <button type="button" className="primary" disabled={busy} onClick={() => void answer({ kind: "skip_failed" })}>
              {one ? "Continue without it" : "Continue without them"}
            </button>
          </>,
        )}
      >
        <ul className="failures">
          {decision.failed.map((item) => (
            <li key={item.id}>
              <strong>{item.title}</strong>
              <span>{item.error}</span>
            </li>
          ))}
        </ul>
      </Dialog>
    );
  }

  if (decision?.kind === "no_speech") {
    return (
      <Dialog
        open={!!script}
        onClose={onClose}
        title={decision.reason === "no_audio" ? "This footage has no sound" : "No words were heard"}
        description={decision.summary}
        width={460}
        footer={foot(
          <>
            <button type="button" className="ghost" onClick={() => void answer({ kind: "cancel" })}>Cancel import</button>
            <button type="button" className="primary" disabled={busy} onClick={() => void answer({ kind: "no_transcript" })}>
              Open without a transcript
            </button>
          </>,
        )}
      />
    );
  }

  return (
    <Dialog
      open={!!script}
      onClose={onClose}
      title="The import stopped"
      description={
        <>
          <span className="bad">{current?.error ?? "It didn't finish."}</span>
          <br />
          Retry carries on from where it stopped. Downloads, the preview and transcribed parts are kept, so nothing is
          paid for twice.
        </>
      }
      width={480}
      footer={foot(
        <>
          <button type="button" className="ghost" onClick={onClose}>Close</button>
          <button type="button" className="primary" disabled={busy} onClick={() => void answer({ kind: "retry" })}>
            {busy ? "Starting…" : "Retry import"}
          </button>
        </>,
      )}
    />
  );
}

const SOURCES: Record<Source, { placeholder: string; hint: string }> = {
  youtube: {
    placeholder: "https://www.youtube.com/watch?v=…",
    hint: "Downloads on this Mac through Writer's proxy, then transcribes.",
  },
  drive: {
    placeholder: "https://drive.google.com/drive/folders/…",
    hint: "A video, or a folder of recordings joined in name order. Shared with Repack's Drive account.",
  },
  file: {
    placeholder: "/Users/you/Movies/interrogation.mp4",
    hint: "The video stays where it is. Writer makes a preview and transcribes it.",
  },
  commentary: {
    placeholder: "Optional: the brief, then evidence, one Google Doc, Drive or web link per line",
    hint: "A video essay in the Internet Janitor's style, written from sources rather than one case's footage. The first link is the brief; add more sources, and past scripts for the style, once it's made.",
  },
  compilation: {
    placeholder: "One YouTube or Drive video link per line, a case each",
    hint: "2 to 12 videos, one case each, in the order they play. A title that opens with \"When\" makes a standard compilation; any other hides that there's more than one case.",
  },
};

const wait = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

function ImportDialog({
  open,
  onClose,
  onStarted,
  onFinished,
  onFailed,
  onOpenNow,
}: {
  open: boolean;
  onClose: () => void;
  onStarted: () => void;
  onFinished: (script: Script) => void;
  onFailed: (reason: string) => void;
  onOpenNow: (script: Script) => void;
}) {
  const [source, setSource] = useState<Source>("youtube");
  const [value, setValue] = useState("");
  const [title, setTitle] = useState("");
  const fields = useRef<HTMLDivElement>(null);
  // A new tab puts the cursor in its first field, ready to paste.
  useEffect(() => {
    if (open) fields.current?.querySelector<HTMLElement>("input, textarea")?.focus();
  }, [source, open]);
  const links = value.split("\n").map((line) => line.trim()).filter(Boolean);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setBusy(false);
  }, [open]);

  const desktop = inTauri();
  const choose = async () => {
    const picked = await pickFile({
      multiple: false,
      filters: [{ name: "Video", extensions: ["mp4", "mov", "mkv", "webm", "m4v"] }],
    });
    if (typeof picked === "string") setValue(picked);
  };

  // An import takes minutes. Errors in the first moments (a bad link, no
  // access) stay in the dialog; after that it closes and the list follows it.
  const submit = async () => {
    const input = value.trim();
    if (!input && source !== "commentary") return;
    if (source === "commentary") {
      if (!title.trim()) {
        setError("Give the script a title.");
        return;
      }
      setBusy(true);
      setError(null);
      try {
        const script = await createCommentary(title, links);
        setValue("");
        setTitle("");
        onClose();
        onStarted();
        onOpenNow(script);
      } catch (cause) {
        setError(message(cause));
      } finally {
        setBusy(false);
      }
      return;
    }
    if (source === "compilation" && (!title.trim() || links.length < 2)) {
      setError("Give the compilation a title and at least two links.");
      return;
    }
    setBusy(true);
    setError(null);
    const work =
      source === "youtube"
        ? importYoutube(input)
        : source === "drive"
          ? importDrive(input)
          : source === "compilation"
            ? importCompilation(title, links)
            : importFile(input);
    const outcome = work.then(
      (script) => ({ kind: "done" as const, script }),
      (cause: unknown) => ({ kind: "failed" as const, cause }),
    );
    const first = await Promise.race([outcome, wait(2500).then(() => ({ kind: "running" as const }))]);
    if (first.kind === "failed" && !(first.cause instanceof TypeError)) {
      setError(message(first.cause));
      setBusy(false);
      return;
    }
    setBusy(false);
    setValue("");
    setTitle("");
    onClose();
    onStarted();
    void outcome.then((result) => {
      if (result.kind === "done") onFinished(result.script);
      else if (!(result.cause instanceof TypeError)) onFailed(message(result.cause));
    });
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="New script"
      description="Bring in the footage. Writer transcribes it, then Claude can write the script."
      width={500}
      footer={
        <>
          {error && <span className="bad dialog-error">{error}</span>}
          <span className="spacer" />
          <button type="button" className="ghost" onClick={onClose}>Cancel</button>
          <button type="submit" form="import" className="primary" disabled={busy || (source === "commentary" ? !title.trim() : !value.trim())}>
            {busy ? (source === "commentary" ? "Reading…" : "Starting…") : source === "commentary" ? "Create" : "Import"}
          </button>
        </>
      }
    >
      <form
        id="import"
        className="importer"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <Tabs
          value={source}
          onChange={(next) => {
            setSource(next);
            setValue("");
            setError(null);
          }}
          options={[
            { value: "youtube", label: <><Icon name="youtube" size={14} />YouTube</> },
            { value: "drive", label: <><Icon name="drive" size={14} />Google Drive</> },
            { value: "file", label: <><Icon name="file" size={14} />This Mac</> },
            { value: "compilation", label: <><Icon name="stack" size={14} />Compilation</> },
            { value: "commentary", label: <><Icon name="file" size={14} />Commentary</> },
          ]}
        />
        <div className="source" key={source} ref={fields}>
          {source === "commentary" ? (
            <>
              <input
                data-autofocus
                value={title}
                placeholder="Title, e.g. MrBeast Is Erasing His Past"
                maxLength={180}
                onChange={(event) => setTitle(event.target.value)}
              />
              <textarea
                className="links"
                rows={4}
                value={value}
                placeholder={SOURCES.commentary.placeholder}
                spellCheck={false}
                onChange={(event) => setValue(event.target.value)}
              />
            </>
          ) : source === "compilation" ? (
            <>
              <input
                data-autofocus
                value={title}
                placeholder="Title, e.g. When Cops Realize They Arrested the Wrong Man"
                maxLength={180}
                onChange={(event) => setTitle(event.target.value)}
              />
              <textarea
                className="links"
                rows={5}
                value={value}
                placeholder={SOURCES.compilation.placeholder}
                spellCheck={false}
                onChange={(event) => setValue(event.target.value)}
              />
              <p className="hint">
                {links.length} {links.length === 1 ? "video" : "videos"} ·{" "}
                {/^\s*["“'‘]?when\b/i.test(title) ? "standard compilation" : "hidden compilation"}
              </p>
            </>
          ) : source === "file" && desktop ? (
            <button type="button" className="drop" onClick={() => void choose()}>
              {value ? (
                <>
                  <strong>{value.split("/").pop()}</strong>
                  <span>{value}</span>
                </>
              ) : (
                <>
                  <strong>Choose a video…</strong>
                  <span>MP4, MOV, MKV, WebM or M4V</span>
                </>
              )}
            </button>
          ) : (
            <input
              data-autofocus
              value={value}
              placeholder={SOURCES[source].placeholder}
              spellCheck={false}
              onChange={(event) => setValue(event.target.value)}
            />
          )}
          <p className="hint">{SOURCES[source].hint}</p>
        </div>
      </form>
    </Dialog>
  );
}

function RenameDialog({
  script,
  onClose,
  onRenamed,
}: {
  script: Summary | null;
  onClose: () => void;
  onRenamed: () => void;
}) {
  const [title, setTitle] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (script) {
      setTitle(script.title);
      setError(null);
    }
  }, [script]);
  const save = async () => {
    if (!script) return;
    setBusy(true);
    try {
      await renameScript(script.id, title);
      onRenamed();
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open={!!script}
      onClose={onClose}
      title="Rename script"
      description="The title also heads the script: INTERRO - title."
      footer={
        <>
          {error && <span className="bad dialog-error">{error}</span>}
          <span className="spacer" />
          <button type="button" className="ghost" onClick={onClose}>Cancel</button>
          <button type="submit" form="rename" className="primary" disabled={busy || !title.trim() || title === script?.title}>
            Rename
          </button>
        </>
      }
    >
      <form
        id="rename"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <input data-autofocus value={title} maxLength={180} onChange={(event) => setTitle(event.target.value)} />
      </form>
    </Dialog>
  );
}

function DeleteDialog({
  script,
  onClose,
  onDeleted,
}: {
  script: Summary | null;
  onClose: () => void;
  onDeleted: (title: string) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Still named while the dialog animates out.
  const shown = useRef(script);
  if (script) shown.current = script;
  useEffect(() => setError(null), [script]);
  const remove = async () => {
    if (!script) return;
    setBusy(true);
    try {
      await deleteScript(script.id);
      onDeleted(script.title);
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open={!!script}
      onClose={onClose}
      title="Delete script?"
      description={
        <>
          <b>{shown.current?.title}</b> and its transcript, preview and narration are removed from this Mac. A video
          opened from disk stays where it is.
        </>
      }
      width={420}
      footer={
        <>
          {error && <span className="bad dialog-error">{error}</span>}
          <span className="spacer" />
          <button type="button" className="ghost" data-autofocus onClick={onClose}>Cancel</button>
          <button type="button" className="danger-fill" disabled={busy} onClick={() => void remove()}>
            Delete
          </button>
        </>
      }
    />
  );
}
