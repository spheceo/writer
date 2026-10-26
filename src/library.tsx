import { open as pickFile } from "@tauri-apps/plugin-dialog";
import { Fragment, type ReactNode, useEffect, useRef, useState } from "react";
import {
  checkVideos,
  createCommentary,
  deleteScript,
  getProgress,
  importDrive,
  importFile,
  importCompilation,
  importYoutube,
  inTauri,
  message,
  renameScript,
  restoreScript,
  resumeImport,
  trelloCard,
  trelloCards,
} from "./api";
import { clock } from "./clips";
import { pastedTrelloCard } from "./import-input";
import { AccountMenu } from "./account";
import { Avatar } from "./avatar";
import mark from "./assets/writer-mark.svg";
import { CAN, useGrants } from "./can";
import { useReviewCounts } from "./review";
import type { Choice, ClaudeStatus, ImportProgress, Kind, Script, Summary, TrelloCard, TrelloCards, TrelloList, VideoCheck } from "./types";
import { Dialog, Icon, Menu, Select, Tabs, type ToastMessage, Toasts } from "./ui";
import { UpdateButton } from "./update";

/** The New script dialog's tabs: one case's video (YouTube, Drive or a file on this Mac), a compilation, a commentary. */
type Source = "video" | "compilation" | "commentary";

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

/** How long ago a script was made, as Repack's library divides it. */
function madeLabel(at: number, now = new Date()) {
  const made = new Date(at);
  const day = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const days = Math.round((day(now) - day(made)) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return `${days} days ago`;
  if (days < 14) return "Last week";
  const months = (now.getFullYear() - made.getFullYear()) * 12 + now.getMonth() - made.getMonth();
  if (months < 1 || days < 30) return `${Math.floor(days / 7)} weeks ago`;
  if (months === 1) return "Last month";
  if (months < 12) return `${months} months ago`;
  const years = now.getFullYear() - made.getFullYear();
  return years <= 1 ? "Last year" : `${years} years ago`;
}

type Tone = "busy" | "idle" | "draft" | "done" | "stale" | "error";

const day = (at: number) => new Date(at).toLocaleDateString(undefined, { day: "numeric", month: "short" });

/**
 * Where a script has got to, as one step of its life, the way Repack says it:
 * importing, being written, a draft, then handed over as a Google Doc. The
 * hover says what the step is made of.
 */
function scriptStatus(script: Summary, progress: ImportProgress[]): { tone: Tone; label: string; title?: string } {
  if (script.archive) {
    return script.archive.restoring || script.building
      ? { tone: "busy", label: "Restoring footage" }
      : { tone: "idle", label: "Archived", title: `Footage archived by ${script.archive.by || "Repack"} on ${new Date(script.archive.at).toLocaleDateString()}. Restore brings it back from its links.` };
  }
  if (script.status === "processing") {
    const live = progress.find((item) => item.id === script.id);
    return { tone: "busy", label: `Importing · ${live?.percent ?? script.progress}%`, title: live?.detail };
  }
  if (script.building) return { tone: "busy", label: "Claude writing" };
  if (script.status === "needs_choice") return { tone: "stale", label: "Needs your choice", title: script.decision?.summary };
  if (script.status === "error") return { tone: "error", label: "Import failed", title: script.error ?? undefined };
  const made =
    script.sourceKind === "commentary"
      ? script.draftWords
        ? `${script.draftWords} words of narration`
        : ""
      : [script.durationMs ? `${clock(script.durationMs)} of footage` : "", script.cutInCount ? `${script.cutInCount} cut-ins` : ""]
          .filter(Boolean)
          .join(" · ");
  const written = script.sourceKind === "commentary" ? script.draftWords > 0 : script.cutInCount > 0;
  if (!written) {
    if (script.sourceKind !== "commentary" && !script.wordCount) return { tone: "idle", label: "No transcript", title: made || undefined };
    return { tone: "idle", label: "Not started", title: made || undefined };
  }
  if (!script.docExportedAt) return { tone: "draft", label: "Draft", title: made };
  return script.docBehind
    ? { tone: "stale", label: "Edited since Doc", title: `Changed after its Google Doc of ${day(script.docExportedAt)} · ${made}` }
    : { tone: "done", label: "In Google Docs", title: `Exported ${day(script.docExportedAt)} · ${made}` };
}

function Status({ tone, label, title }: { tone: Tone; label: string; title?: string }) {
  return (
    <span className={`script-status ${tone}`} title={title}>
      <i aria-hidden="true" />
      <span className="truncate">{label}</span>
    </span>
  );
}

/**
 * A YouTube import's title carries its channel and link: "Title (Channel,
 * https://…)". The list shows the title, the channel quietly after it.
 */
export function splitTitle(title: string) {
  const found = /^(.*)\s\((.+),\s*https?:\/\/[^\s)]+\)\s*$/.exec(title);
  return found ? { name: found[1], from: found[2] } : { name: title, from: "" };
}

const KIND_ICON: Record<string, string> = { youtube: "youtube", drive: "drive", file: "file", commentary: "doc", compilation: "stack" };

/** The list's sections, as Edit's Home splits its own: one case's footage, compilations, commentaries. */
type Section = "all" | "footage" | "compilations" | "commentary";

const SECTIONS: { value: Section; label: string }[] = [
  { value: "all", label: "All" },
  { value: "footage", label: "Footage" },
  { value: "compilations", label: "Compilations" },
  { value: "commentary", label: "Commentary" },
];

function sectionOf(script: Summary): Section {
  if (script.sourceKind === "commentary") return "commentary";
  return script.compilation ? "compilations" : "footage";
}

/** A footage script's or a compilation's style, coloured as Edit colours them. */
type Style = Kind;
const STYLES: { value: Style; label: string; colour: string }[] = [
  { value: "foia", label: "Bodycam", colour: "#7aa2ff" },
  { value: "interrogation", label: "Interrogation", colour: "#ff5f6d" },
  { value: "courtroom", label: "Courtroom", colour: "#c4a7ff" },
];

export function Library({
  scripts,
  claude,
  onSettings,
  onLogs,
  progress,
  loadError,
  onOpen,
  onChanged,
}: {
  scripts: Summary[];
  claude: ClaudeStatus | null;
  onSettings: () => void;
  /** Writer's log (⌘⇧L). */
  onLogs: () => void;
  progress: ImportProgress[];
  loadError: string | null;
  onOpen: (id: string) => void;
  onChanged: () => void;
}) {
  const [creating, setCreating] = useState(false);
  const reviews = useReviewCounts();
  const [renaming, setRenaming] = useState<Summary | null>(null);
  const [deleting, setDeleting] = useState<Summary | null>(null);
  const [recovering, setRecovering] = useState<Summary | null>(null);
  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const next = useRef(0);
  const [query, setQuery] = useState("");
  const [section, setSectionNow] = useState<Section>(() => (localStorage.getItem("writer.section") as Section | null) ?? "all");
  const setSection = (next: Section) => {
    setSectionNow(next);
    setStyle(null);
    localStorage.setItem("writer.section", next);
  };
  const [style, setStyle] = useState<Style | null>(null);
  const search = useRef<HTMLInputElement>(null);
  // "/" finds a script from anywhere on the page, as in Repack's lists.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (event.key !== "/" || (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA"))) return;
      event.preventDefault();
      search.current?.focus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const toast = (text: ToastMessage["text"], action?: ToastMessage["action"]) =>
    setToasts((list) => [...list, { id: ++next.current, text, action }]);
  const dismiss = (id: number) => setToasts((list) => list.filter((item) => item.id !== id));

  // An import whose script the list hasn't caught up with yet.
  const importing = progress.filter((item) => !scripts.some((script) => script.id === item.id));
  const empty = scripts.length === 0 && !importing.length;
  // Newest first, under a divider for each stretch of time, as Repack lists them.
  const needle = query.trim().toLowerCase();
  // Footage a commentary was written from lives inside that commentary: the list shows it only to a search.
  const listed = scripts.filter((script) => !script.inCommentary);
  const sections = SECTIONS.map((option) => ({ ...option, count: option.value === "all" ? listed.length : listed.filter((script) => sectionOf(script) === option.value).length })).filter(
    (option) => option.count > 0 || option.value === "all",
  );
  const shownSection = sections.some((option) => option.value === section) ? section : "all";
  const inSection = (script: Summary) => shownSection === "all" || sectionOf(script) === shownSection;
  const styles = shownSection === "commentary" ? [] : STYLES.filter((option) => listed.some((script) => inSection(script) && sectionOf(script) !== "commentary" && (script.kind ?? "interrogation") === option.value));
  // A search looks through everything; otherwise the section and the style narrow the list.
  const rows = [...scripts]
    .sort((a, b) => b.createdAt - a.createdAt)
    .filter((script) =>
      needle
        ? script.title.toLowerCase().includes(needle)
        : !script.inCommentary && inSection(script) && (!style || (sectionOf(script) !== "commentary" && (script.kind ?? "interrogation") === style)),
    );
  const open = (script: Summary) => (stuck(script) ? setRecovering(script) : onOpen(script.id));

  return (
    <div className="library">
      <header className="app-header" data-tauri-drag-region>
        <span className="brand" data-tauri-drag-region>
          <img src={mark} alt="" width={22} height={22} />
          Writer
        </span>
        {!empty && (
          <label className="list-search">
            <Icon name="search" size={14} />
            <input
              ref={search}
              value={query}
              placeholder="Search scripts"
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="off"
              spellCheck={false}
              aria-label="Search scripts"
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") setQuery("");
                if (event.key === "Enter" && rows[0]) open(rows[0]);
              }}
            />
            {query ? (
              <button type="button" className="clear" aria-label="Clear search" onClick={() => setQuery("")}>
                ×
              </button>
            ) : (
              <kbd>/</kbd>
            )}
          </label>
        )}
        <span className="spacer" data-tauri-drag-region />
        <div className="heading-actions">
          {/* Claude only says something when it needs the person: signed out or missing. */}
          {claude && !claude.loggedIn && (
            <button type="button" className="claude-status bad" title={claude.message} onClick={onSettings}>
              <span className="dot bad" />
              Claude needs signing in
            </button>
          )}
          <UpdateButton />
          <button type="button" className="primary" onClick={() => setCreating(true)}>
            <Icon name="plus" size={15} />
            New script
          </button>
          <AccountMenu onSettings={onSettings} onLogs={onLogs} />
        </div>
      </header>
      <div className="library-body">
      {!empty && !needle && (
        <div className="list-filters">
          <div className="seg" role="tablist" aria-label="Sections">
            {sections.map((option) => (
              <button key={option.value} type="button" role="tab" aria-selected={shownSection === option.value} className={shownSection === option.value ? "on" : ""} onClick={() => setSection(option.value)}>
                {option.label} <span>{option.count}</span>
              </button>
            ))}
          </div>
          <span className="spacer" />
          {styles.length > 1 &&
            styles.map((option) => (
              <button key={option.value} type="button" className={style === option.value ? "style-pill on" : "style-pill"} aria-pressed={style === option.value} onClick={() => setStyle(style === option.value ? null : option.value)}>
                <i style={{ background: option.colour }} />
                {option.label}
              </button>
            ))}
        </div>
      )}
      {loadError && <div className="notice">{loadError}</div>}
      {empty ? (
        <div className="welcome">
          <img src={mark} alt="" width={64} height={64} />
          <h2>Write your first script</h2>
          <p>Bring in footage or sources. Writer transcribes them, then Claude can write alongside you.</p>
          <button type="button" className="primary" onClick={() => setCreating(true)}>
            <Icon name="plus" size={15} />
            New script
          </button>
        </div>
      ) : (
        <table className="table" aria-label="Scripts">
          <thead>
            <tr className="table-row table-head">
              <th className="cell" scope="col">Name</th>
              <th className="cell" scope="col">Status</th>
              <th className="cell" scope="col">Updated</th>
              <th className="cell" scope="col"><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {importing.map((item) => (
              <tr className="table-row" key={item.id}>
                <td className="cell cell-primary">
                  <div className="script-title">
                    <span className="kind-mark"><Icon name="youtube" size={13} /></span>
                    <span className="row-link truncate" title={item.title}>{item.title}</span>
                  </div>
                </td>
                <td className="cell"><Status tone="busy" label={`Importing · ${item.percent}%`} title={item.detail} /></td>
                <td className="cell">just now</td>
                <td className="cell cell-end" />
              </tr>
            ))}
            {rows.map((script, index) => {
              const made = madeLabel(script.createdAt);
              const locked = script.status === "processing" || script.building;
              return (
                <Fragment key={script.id}>
                  {/* Today needs no heading; older scripts get one per stretch of time. */}
                  {made !== "Today" && (index === 0 || madeLabel(rows[index - 1].createdAt) !== made) && (
                    <tr className="script-group">
                      <th colSpan={4} scope="rowgroup">{made}</th>
                    </tr>
                  )}
                  <tr className="table-row openable" onClick={() => open(script)}>
                    <td className="cell cell-primary">
                      <div className="script-title">
                        <span className="kind-mark" title={script.sourceKind === "commentary" ? "Commentary" : "Footage"}>
                          <Icon name={script.compilation ? "stack" : (KIND_ICON[script.sourceKind] ?? "file")} size={13} />
                        </span>
                        <button
                          type="button"
                          className="row-link"
                          title={script.title}
                          onClick={(event) => {
                            event.stopPropagation();
                            open(script);
                          }}
                        >
                          {splitTitle(script.title).name}
                          {splitTitle(script.title).from && <span className="from"> · {splitTitle(script.title).from}</span>}
                        </button>
                        {reviews?.approved[script.id] && (
                          <span className="script-kind approved-mark" title={`Approved by ${reviews.approved[script.id].by}, ${new Date(reviews.approved[script.id].at).toLocaleString()}`}>
                            Approved v{reviews.approved[script.id].version}
                          </span>
                        )}
                        {(reviews?.open[script.id] ?? 0) > 0 && (
                          <span className="count-badge" title={`${reviews?.open[script.id]} open review ${reviews?.open[script.id] === 1 ? "note" : "notes"} from its Repack link`}>
                            {reviews?.open[script.id]}
                          </span>
                        )}
                        {/* The section says what it is; a search's results, from every section, say it themselves. */}
                        {needle && script.inCommentary ? (
                          <span className="script-kind" title={`Footage of the commentary “${script.inCommentary}”`}>In {script.inCommentary}</span>
                        ) : needle && script.compilation ? (
                          <span className="script-kind">Compilation</span>
                        ) : needle && script.sourceKind === "commentary" ? (
                          <span className="script-kind">Commentary</span>
                        ) : null}
                        <Avatar creator={script.createdBy} />
                      </div>
                    </td>
                    <td className="cell">
                      <Status {...scriptStatus(script, progress)} />
                    </td>
                    <td className="cell">
                      <time title={new Date(script.updatedAt).toLocaleString()}>{ago(script.updatedAt)}</time>
                    </td>
                    <td className="cell cell-end">
                      <Menu
                        label={`Manage ${script.title}`}
                        items={[
                          ...(stuck(script)
                            ? [{ label: script.status === "needs_choice" ? "Answer…" : "Retry import…", onSelect: () => setRecovering(script) }]
                            : []),
                          { label: "Open script", onSelect: () => onOpen(script.id) },
                          ...(script.archive && !script.archive.restoring
                            ? [{ label: "Restore footage", onSelect: () => void restoreScript(script.id).then(() => onChanged(), (cause) => toast(<>{message(cause)}</>)) }]
                            : []),
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
                    </td>
                  </tr>
                </Fragment>
              );
            })}
            {rows.length === 0 && !importing.length && (
              <tr className="table-row empty-row">
                <td className="cell" colSpan={4}>
                  {needle ? `Nothing matches “${query.trim()}”.` : style ? `No ${STYLES.find((option) => option.value === style)?.label} scripts here yet.` : "Nothing here yet."}{" "}
                  <button
                    type="button"
                    className="link-button"
                    onClick={() => {
                      setQuery("");
                      setStyle(null);
                    }}
                  >
                    Show all
                  </button>
                </td>
              </tr>
            )}
          </tbody>
        </table>
      )}
      </div>

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
        onImporting={onOpen}
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
  video: {
    placeholder: "Paste a YouTube, Google Drive or Trello card link",
    hint: "Writer downloads it, makes a preview and transcribes it.",
  },
  commentary: {
    placeholder: "Sources (optional): the brief first, then evidence. One Google Doc, Drive or web link per line.",
    hint: "A video essay written from sources rather than one case's footage. You can add sources once it's made.",
  },
  compilation: {
    placeholder: "Paste YouTube, Drive files or folders, and Google Docs script links in case order, or a Trello card.\nMark an earlier video REUSE or (reuse). Script Docs are always reused.",
    hint: "2 to 20 videos, in the order they play. A title that starts with \"When\" makes a standard compilation.",
  },
};

const isDrive = (url: string) => /drive\.google\.com/.test(url);
/** What the Video tab will do with what's in it, said in a few words. */
const videoHint = (value: string, file: string) =>
  file
    ? "The video stays where it is on this Mac. Writer makes a preview and transcribes it."
    : isDrive(value)
      ? "A Drive video, or a folder of recordings joined in name order."
      : value.trim()
        ? "Downloaded on this Mac through Writer's proxy, then transcribed."
        : SOURCES.video.hint;

/** A Google Doc link: the script of a case the channel already made, always a reused case. */
const isDoc = (url: string) => url.includes("docs.google.com/document/d/");

/** One of Repack's Trello lists, as its "Not scripted yet" cards; a card fills the dialog. */
function TrelloPicker({ list, problem, all, onUse }: { list?: TrelloCards; problem: string | null; all: boolean; onUse: (card: TrelloCard) => void }) {
  if (problem) return <p className="bad">{problem}</p>;
  if (!list) return <p className="faint">Reading Trello…</p>;
  const shown = list.cards.filter((card) => all || !card.scripted);
  const hidden = list.cards.length - shown.length;
  return (
    <div className="trello-picker">
      <p className="hint">
        {list.list} · {shown.length} {shown.length === 1 ? "card" : "cards"}
        {hidden ? ` · ${hidden} already scripted, hidden` : ""}
      </p>
      <div className="checklist video-checks">
        {shown.length === 0 && <p className="faint trello-empty">Nothing waiting for a script.</p>}
        {shown.map((card) => (
          <div key={card.id} className={card.problem ? "video-check trello-card bad-row" : "video-check trello-card"}>
            <span className="video-check-what">
              <a className="truncate" href={card.url} target="_blank" rel="noreferrer" title={card.name}>
                <strong>{card.name}</strong>
              </a>
              <span className={card.problem ? "bad" : "faint"}>
                {card.problem ??
                  (card.kind === "case"
                    ? [card.footage?.includes("/folders/") ? "Drive folder" : "Drive video", card.article ? "article" : ""].filter(Boolean).join(" · ")
                    : [`${card.videos} ${card.videos === 1 ? "video" : "videos"}`, card.reuses ? `${card.reuses} reused` : ""].filter(Boolean).join(" · "))}
                {card.scripted ? " · scripted" : ""}
              </span>
            </span>
            <button type="button" className="sm" disabled={!!card.problem} onClick={() => onUse(card)}>
              Use
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

const wait = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

function ImportDialog({
  open,
  onClose,
  onStarted,
  onFinished,
  onFailed,
  onOpenNow,
  onImporting,
}: {
  open: boolean;
  onClose: () => void;
  onStarted: () => void;
  onFinished: (script: Script) => void;
  onFailed: (reason: string) => void;
  onOpenNow: (script: Script) => void;
  /** An import started: open its project, where its progress shows. */
  onImporting: (id: string) => void;
}) {
  // Only the kinds of script this person may start (can.ts; the app refuses the rest too).
  const grants = useGrants();
  const single = grants.singleKinds.length > 0;
  const many = grants.compilationKinds.length > 0;
  const courtroom = grants.has(CAN.courtroom);
  const sources: Source[] = [
    ...(single ? (["video"] as const) : []),
    ...(many || courtroom ? (["compilation"] as const) : []),
    ...(grants.has(CAN.commentary) ? (["commentary"] as const) : []),
  ];
  const [source, setSource] = useState<Source>("video");
  // A video picked on this Mac, instead of a link.
  const [file, setFile] = useState("");
  const [articleShown, setArticleShown] = useState(false);
  const [value, setValue] = useState("");
  const [title, setTitle] = useState("");
  const [article, setArticle] = useState("");
  // A compilation's videos as checked, before anything downloads, as Repack checks them.
  const [checked, setChecked] = useState<VideoCheck[] | null>(null);
  // One Courtroom case whose hearing runs across the videos, rather than a case each.
  const [oneCase, setOneCase] = useState(false);
  const [format, setFormat] = useState<Kind>("interrogation");
  const formats = source === "compilation" ? (oneCase ? ["courtroom" as Kind] : grants.compilationKinds) : grants.singleKinds;
  const formatKey = formats.join();
  useEffect(() => {
    if (formats.length && !formats.includes(format)) setFormat(formats[0]);
  }, [formatKey, format]);
  // A source this person can't use isn't offered: move to the first they can.
  useEffect(() => {
    if (sources.length && !sources.includes(source)) setSource(sources[0]);
  }, [sources.join(), source]);
  // Courtroom without compilations: several videos can only be one case.
  useEffect(() => {
    if (courtroom && !many) setOneCase(true);
    if (!courtroom) setOneCase(false);
  }, [courtroom, many]);
  const [checking, setChecking] = useState(false);
  // Repack's Trello lists, open in place of the fields while a card is picked.
  const [trello, setTrello] = useState<TrelloList | null>(null);
  const [cards, setCards] = useState<Partial<Record<TrelloList, TrelloCards>>>({});
  const [cardsProblem, setCardsProblem] = useState<string | null>(null);
  const [allCards, setAllCards] = useState(false);
  // A Raw Channels card's name, kept as the Drive import's title.
  const [cardTitle, setCardTitle] = useState("");
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
    if (typeof picked === "string") {
      setFile(picked);
      setValue("");
    }
  };

  const openTrello = async (list: TrelloList, again = false) => {
    setTrello(list);
    setCardsProblem(null);
    if (cards[list] && !again) return;
    try {
      const found = await trelloCards(list);
      setCards((known) => ({ ...known, [list]: found }));
    } catch (cause) {
      setCardsProblem(message(cause));
    }
  };
  // A card fills the dialog as Repack's Start script does: a case card's
  // footage, article and name; a compilation card's title and links, checked.
  const useCard = (card: TrelloCard) => {
    setTrello(null);
    setError(null);
    setChecked(null);
    // A pasted card with one video is that video's import.
    if (card.kind === "video" || card.kind === "case") {
      setSource("video");
      setFile("");
      setValue(card.footage ?? "");
      setArticle(card.article ?? "");
      setArticleShown(!!card.article);
      setCardTitle(card.name);
      return;
    }
    setSource("compilation");
    setOneCase(false);
    setTitle(card.name);
    setValue(card.lines.join("\n"));
    if (card.lines.length >= 2) void check(card.lines);
  };

  // A Trello card link pasted in a link field: the card is read and fills the dialog, as a picked card does.
  const [readingCard, setReadingCard] = useState(false);
  const paste = (text: string) => {
    setValue(text);
    const link = pastedTrelloCard(text);
    if (!link) return;
    setReadingCard(true);
    setError(null);
    void trelloCard(link)
      .then((card) => {
        if (card.problem) setError(`${card.name}: ${card.problem}`);
        else useCard(card);
      })
      .catch((cause) => setError(message(cause)))
      .finally(() => setReadingCard(false));
  };

  const check = async (lines = links) => {
    setChecking(true);
    setError(null);
    try {
      setChecked(await checkVideos(lines));
    } catch (cause) {
      setError(message(cause));
    } finally {
      setChecking(false);
    }
  };
  // The checked list, as lines again: a row toggled to a reuse or removed changes the import.
  const checkedLines = (rows: VideoCheck[]) => rows.map((row) => (row.reuse ? ["REUSE", row.url, row.earlier].filter(Boolean).join(" ") : row.url));
  const imported = checked?.filter((row) => !row.reuse) ?? [];
  const totalMs = imported.reduce((sum, row) => sum + (row.durationMs ?? 0), 0);
  const blocked = !checked
    ? null
    : imported.some((row) => row.problem)
      ? "Remove or replace the videos that can't be imported."
      : checked.some((row) => isDoc(row.url) && row.problem)
        ? "Remove or replace the script Docs that can't be read."
      : oneCase && checked.some((row) => row.reuse)
        ? "A reused case belongs in a compilation, not one Courtroom case."
        : (oneCase ? imported.length < 2 : imported.length < 1 || checked.length < 2)
        ? "Use at least two cases, including new footage."
        : imported.length > 20
          ? "A compilation takes at most 20 videos to import."
          : totalMs > 20 * 3_600_000
            ? "Together the videos run past 20 hours."
            : null;

  // An import takes minutes. Errors in the first moments (a bad link, no
  // access) stay in the dialog; after that it closes and the list follows it.
  const submit = async () => {
    const input = value.trim();
    if (!input && !file && source !== "commentary") return;
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
    if (source === "compilation" && (!title.trim() || links.length < 1)) {
      setError("Give the compilation a title and paste its case links.");
      return;
    }
    // A compilation's videos are checked before they import.
    if (source === "compilation" && !checked) return void check();
    if (source === "compilation" && blocked) return setError(blocked);
    setBusy(true);
    setError(null);
    // Imports already under way, so the new one is the one that appears.
    const before = new Set((await getProgress().catch(() => [])).map((item) => item.id));
    const work =
      source === "compilation"
        ? importCompilation(title, checked ? checkedLines(checked) : links, oneCase, format)
        : file
          ? importFile(file, article.trim(), format)
          : isDrive(input)
            ? importDrive(input, article.trim(), cardTitle, format)
            : importYoutube(input, article.trim(), format);
    const outcome = work.then(
      (script) => ({ kind: "done" as const, script }),
      (cause: unknown) => ({ kind: "failed" as const, cause }),
    );
    // The import's own script appears in the progress list within moments; it opens there.
    const appeared = (async () => {
      for (let tries = 0; tries < 40; tries++) {
        await wait(400);
        const found = (await getProgress().catch(() => [])).find((item) => !before.has(item.id));
        if (found) return { kind: "running" as const, id: found.id };
      }
      return { kind: "running" as const, id: null };
    })();
    const first = await Promise.race([outcome, appeared]);
    if (first.kind === "failed" && !(first.cause instanceof TypeError)) {
      setError(message(first.cause));
      setBusy(false);
      return;
    }
    setBusy(false);
    setValue("");
    setTitle("");
    setArticle("");
    setCardTitle("");
    setFile("");
    setArticleShown(false);
    setChecked(null);
    setOneCase(false);
    onClose();
    onStarted();
    // Straight to the project: the import shows there, and the producer can leave it.
    const opened = first.kind === "done" ? first.script.id : first.kind === "running" ? first.id : null;
    if (opened) onImporting(opened);
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
      width={560}
      footer={
        <>
          {error && <span className="bad dialog-error">{error}</span>}
          <span className="spacer" />
          <button type="button" className="ghost" onClick={onClose}>Cancel</button>
          <button
            type="submit"
            form="import"
            className="primary"
            disabled={busy || checking || (source === "commentary" ? !title.trim() : !value.trim() && !file) || (source === "compilation" && !!checked && !!blocked)}
          >
            {busy
              ? source === "commentary"
                ? "Reading…"
                : "Starting…"
              : source === "commentary"
                ? "Create"
                : source === "compilation"
                  ? checking
                    ? "Checking…"
                    : checked
                      ? `Import ${imported.length} ${imported.length === 1 ? "video" : "videos"}`
                      : "Check videos"
                  : "Import"}
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
            setChecked(null);
            setError(null);
            setCardTitle("");
            setFile("");
            setTrello(null);
          }}
          options={[
            { value: "video" as Source, label: <><Icon name="youtube" size={14} />Video</> },
            { value: "compilation" as Source, label: <><Icon name="stack" size={14} />{many ? "Compilation" : "Several videos"}</> },
            { value: "commentary" as Source, label: <><Icon name="doc" size={14} />Commentary</> },
          ].filter((option) => sources.includes(option.value))}
        />
        {!sources.length && <p className="bad">Your account can't start scripts in Writer yet. Ask an admin at accounts.spheceo.com.</p>}
        <div className="trello-bar">
          {trello ? (
            <>
              {(["raw", "comp"] as const).filter((list) => (list === "raw" ? single : many)).map((list) => (
                <button key={list} type="button" className={trello === list ? "sm on" : "sm ghost"} onClick={() => void openTrello(list)}>
                  {list === "raw" ? "Raw Channels" : "Compilations"}
                </button>
              ))}
              <span className="spacer" />
              <label className="faint toggle">
                <input type="checkbox" checked={allCards} onChange={(event) => setAllCards(event.target.checked)} /> Show scripted
              </label>
              <button type="button" className="link-button faint" onClick={() => void openTrello(trello, true)}>Refresh</button>
              <button type="button" className="link-button faint" onClick={() => setTrello(null)}>Close</button>
            </>
          ) : (
            (single || many) && <button type="button" className="link-button" onClick={() => void openTrello((source === "compilation" && many) || !single ? "comp" : "raw")}>
              <Icon name="stack" size={13} /> Import from Trello
            </button>
          )}
        </div>
        {trello ? (
          <TrelloPicker list={cards[trello]} problem={cardsProblem} all={allCards} onUse={useCard} />
        ) : (
        <div className="source" key={source} ref={fields}>
          {source !== "commentary" && <label className="creation-format">
            <span>{source === "compilation" && !oneCase ? "Compilation type" : "Script type"}</span>
            <Select value={format} label={source === "compilation" ? "Compilation type" : "Script type"} disabled={busy || oneCase} options={formats.map((value) => ({ value, label: value === "foia" ? "Bodycam" : value === "courtroom" ? "Courtroom" : "Interrogation" }))} onChange={setFormat} />
          </label>}
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
              {!checked ? (
                <textarea
                  className="links"
                  rows={5}
                  value={value}
                  placeholder={SOURCES.compilation.placeholder}
                  spellCheck={false}
                  onChange={(event) => paste(event.target.value)}
                />
              ) : (
                <div className="checklist video-checks">
                  {checked.map((row, index) => (
                    <div key={`${row.url}-${index}`} className={row.problem && (!row.reuse || isDoc(row.url)) ? "video-check bad-row" : "video-check"}>
                      <span className="faint mono">{row.reuse ? `↺ ${index + 1}` : index + 1}</span>
                      <span className="video-check-what">
                        <strong className="truncate" title={row.title || row.url}>{row.title || row.url}</strong>
                        <span className={row.problem && (!row.reuse || isDoc(row.url)) ? "bad" : "faint"}>
                          {isDoc(row.url)
                            ? (row.problem ?? `Reused script Doc: kept as written; it takes only its transition.${row.warning ? ` ${row.warning}` : ""}`)
                            : row.reuse
                            ? `Reused case: not imported; it takes only its transition.${row.earlierFound ? ` ${row.earlierFound}` : ""}`
                            : (row.problem ?? row.warning ?? [row.durationMs ? clock(row.durationMs) : "", row.channel].filter(Boolean).join(" · "))}
                        </span>
                      </span>
                      <label className="faint toggle" title="A case the channel already covered: not imported, it takes only its transition">
                        <input type="checkbox" checked={row.reuse} disabled={isDoc(row.url)} onChange={(event) => setChecked(checked.map((item, at) => (at === index ? { ...item, reuse: event.target.checked } : item)))} /> Reuse
                      </label>
                      <button type="button" className="link-button faint" aria-label="Remove" title="Remove" onClick={() => setChecked(checked.filter((_, at) => at !== index))}>
                        ×
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {courtroom && (
                <label className="faint toggle" title="Repack's Courtroom: one case's hearing in several videos, joined in order as one case; a link's t marks where the case starts">
                  <input type="checkbox" checked={oneCase} disabled={!many} onChange={(event) => setOneCase(event.target.checked)} /> One Courtroom case across these videos
                </label>
              )}
              <p className="hint">
                {checked ? (
                  <>
                    {imported.length} {imported.length === 1 ? "video" : "videos"} to import{totalMs ? `, ${clock(totalMs)}` : ""}
                    {checked.length > imported.length ? ` · ${checked.length - imported.length} reused` : ""} ·{" "}
                    {oneCase ? "one Courtroom case" : /^\s*["“'‘]?when\b/i.test(title) ? "standard compilation" : "hidden compilation"} ·{" "}
                    <button type="button" className="link-button" onClick={() => (setValue(checkedLines(checked).join("\n")), setChecked(null))}>
                      Edit the links
                    </button>
                  </>
                ) : (
                  <>
                    {links.length} {links.length === 1 ? "line" : "lines"} · {oneCase ? "one Courtroom case" : /^\s*["“'‘]?when\b/i.test(title) ? "standard compilation" : "hidden compilation"}
                  </>
                )}
              </p>
            </>
          ) : (
            <>
              {file ? (
                <div className="picked-file">
                  <Icon name="file" size={14} />
                  <span className="truncate" title={file}>{file.split("/").pop()}</span>
                  <button type="button" className="link-button faint" onClick={() => setFile("")}>Use a link instead</button>
                </div>
              ) : (
                <input data-autofocus value={value} placeholder={SOURCES.video.placeholder} spellCheck={false} onChange={(event) => paste(event.target.value)} />
              )}
              <p className="hint">
                {readingCard ? "Reading the Trello card…" : videoHint(value, file)}
                {desktop && !file && (
                  <>
                    {" "}
                    <button type="button" className="link-button" onClick={() => void choose()}>Choose a file on this Mac…</button>
                  </>
                )}
              </p>
              {articleShown ? (
                <input value={article} placeholder="News article on the case: https://…" spellCheck={false} aria-label="Article" onChange={(event) => setArticle(event.target.value)} />
              ) : (
                <button type="button" className="link-button faint add-article" onClick={() => setArticleShown(true)}>
                  <Icon name="plus" size={12} /> Add a news article
                </button>
              )}
              {cardTitle && isDrive(value) && (
                <p className="hint">
                  Titled from Trello: <strong>{cardTitle}</strong> ·{" "}
                  <button type="button" className="link-button" onClick={() => setCardTitle("")}>Use the video's own name</button>
                </p>
              )}
            </>
          )}
          {source !== "video" && <p className="hint">{readingCard ? "Reading the Trello card…" : SOURCES[source].hint}</p>}
        </div>
        )}
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
