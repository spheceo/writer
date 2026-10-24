import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { addReviewNote, checkReviewNote, deleteReviewNote, message, type NoteDetails, replyReviewNote, reviewFix, type ReviewFix } from "./api";
import mark from "./assets/writer-mark.svg";
import type { ReviewNote } from "./types";
import { Icon, Menu, type MenuItem, Select } from "./ui";

/*
 * The review's comments as feedback.spheceo.com shows them (the standard):
 * one set of notes, the same in both places. A note left here is the
 * review's note, shown on the page as Writer's and fixed on Ready for
 * changes like a reviewer's. The cards, states, intents, replies, unread
 * dots, filters and sort follow the page's (feedback/src/client/comments.tsx,
 * src/shared/notes.ts).
 */

/** The page's intent labels (feedback/src/shared/reviews.ts INTENT_LABELS). */
export const INTENT_LABELS: Record<string, string> = {
  wording: "Wording",
  fact: "Fact check",
  cut: "Cut / trim",
  pacing: "Pacing",
  other: "Other",
  editor: "For the editor",
  question: "Question",
  fyi: "FYI / praise",
  suggestion: "Suggestion",
};
const CHANGES = ["wording", "fact", "cut", "pacing", "other", "editor"];
const NOT_CHANGES = ["question", "fyi"];
const ACTIONS: Record<string, string> = { cut: "Cut", extend: "Extend", restore: "Bring back" };

/** A claim lasts this long (Repack's claimMs): past it, nobody's working on the note. */
const CLAIM_MS = 30 * 60_000;

// ---- Which note is lit: hovered or open, here or on its words ------------------

let lit: { hovered: string | null; active: string | null } = { hovered: null, active: null };
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((listen) => listen());
/** Lights a note as hovered (its card or its words), or none. */
export function hoverNote(id: string | null) {
  if (lit.hovered === id) return;
  lit = { ...lit, hovered: id };
  emit();
}
/** Opens a note: its card and its words stay lit. */
export function openNote(id: string | null) {
  if (lit.active === id) return;
  lit = { ...lit, active: id };
  emit();
}
export function useLit() {
  return useSyncExternalStore(
    (listen) => {
      listeners.add(listen);
      return () => listeners.delete(listen);
    },
    () => lit,
  );
}

/** The words of the lit notes, brighter: the script's marks carry their notes' IDs. */
export function LitStyle() {
  const { hovered, active } = useLit();
  const ids = [...new Set([hovered, active].filter((id): id is string => !!id))].filter((id) => /^[\w-]+$/.test(id));
  if (!ids.length) return null;
  const selectors = ids.flatMap((id) => [`.grow-mirror mark[data-ids~="${id}"]`, `[data-band~="${id}"]`]).join(", ");
  return <style>{`${selectors} { background: #f8d66d !important; box-shadow: 0 0 0 2px #f8d66d !important; }`}</style>;
}

// ---- Read and unread, kept on this Mac as the page keeps it in the browser ----

const readKey = (review: string) => `writer-read:${review}`;
function readSet(review: string): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(readKey(review)) ?? "[]") as string[]);
  } catch {
    return new Set();
  }
}
function markRead(review: string, ids: string[]) {
  const set = readSet(review);
  ids.forEach((id) => set.add(id));
  localStorage.setItem(readKey(review), JSON.stringify([...set].slice(-5000)));
}

// ---- How a note reads -----------------------------------------------------------

/** The page's "now", "5m", "2h", "3d", then a date. */
export function ago(ms: number, now = Date.now()) {
  const seconds = (now - ms) / 1000;
  if (seconds < 45) return "now";
  if (seconds < 3600) return `${Math.max(1, Math.round(seconds / 60))}m`;
  if (seconds < 86_400) return `${Math.round(seconds / 3600)}h`;
  if (seconds < 7 * 86_400) return `${Math.round(seconds / 86_400)}d`;
  return new Date(ms).toLocaleDateString([], { day: "numeric", month: "short" });
}

const HUES = ["#8fbaf0", "#b4bcf0", "#f3a48e", "#d0aaf3", "#a9e091", "#86d3d6", "#f0a3c4", "#9fd3b5"];
function hue(name: string) {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return HUES[hash % HUES.length];
}
const initials = (name: string) => {
  const words = name.trim().split(/\s+/).filter(Boolean);
  return ((words[0]?.[0] ?? "?") + (words.length > 1 ? words[words.length - 1][0] : "")).toUpperCase();
};

function Avatar({ note }: { note: ReviewNote }) {
  if (note.fromApp) return <img className="cm-avatar app" src={mark} alt="" width={28} height={28} />;
  const colour = hue(note.author);
  return (
    <span className="cm-avatar" style={{ color: colour, background: `color-mix(in srgb, ${colour} 15%, transparent)` }}>
      {initials(note.author)}
    </span>
  );
}

/** What's happening with a note, as the page's agentState says it, in its order of precedence. */
export function stateOf(note: ReviewNote, replies: ReviewNote[], now = Date.now()): { kind: string; label: string; tip?: string } | null {
  const v = note.doneInVersion ? ` in v${note.doneInVersion}` : "";
  if (note.status === "done") {
    if (note.outcome === "accepted") return { kind: "applied", label: `Applied${v}`, tip: note.fixSummary ?? undefined };
    if (note.outcome === "rejected") return { kind: "notApplied", label: "Not applied", tip: note.fixSummary ?? undefined };
    if (note.confirmed) return { kind: "confirmed", label: note.confirmedBy ? `Confirmed by ${note.confirmedBy}` : "Confirmed", tip: note.fixSummary ?? undefined };
    return { kind: "done", label: `Done${v}`, tip: note.fixSummary ?? undefined };
  }
  if (note.fromApp && note.intent === "question") return { kind: "answer", label: "Waiting for your reply" };
  const working = note.attempted && (note.attemptAt ?? 0) > now - CLAIM_MS && !(note.notFixedAt && note.notFixedAt > (note.attemptAt ?? 0));
  const last = replies[replies.length - 1];
  if (!working && last?.fromApp && note.intent !== "question" && note.intent !== "fyi") return { kind: "left", label: `Left: ${last.text}` };
  if (working) return { kind: "working", label: "Writer is working on it" };
  if (note.notFixedAt) return { kind: "notFixed", label: note.notFixedVersion ? `Not fixed in v${note.notFixedVersion}: back with Writer` : "Not fixed: back with Writer" };
  if (note.intent === "question") return { kind: "answer", label: "Writer will answer" };
  if (note.intent === "fyi") return { kind: "answer", label: "No fix asked" };
  return null;
}

const quoteChip = (quote: string) => `“${quote.length > 60 ? `${quote.slice(0, 59)}…` : quote}”`;

// ---- The panel ---------------------------------------------------------------------

/** Words picked in the script to comment on: where, as the page anchors them. */
export type Draft = { blockId: string; start: number; end: number; quote: string };

type Filter = "all" | "open" | "done" | "unread" | "mine" | `person:${string}`;

export function CommentsPanel({
  scriptId,
  reviewId,
  notes,
  version,
  shared,
  canWrite,
  draft,
  onDraft,
  onJump,
  onRefresh,
  onError,
}: {
  scriptId: string;
  /** The review the notes are on; none until the script is shared. */
  reviewId?: string;
  notes: ReviewNote[];
  /** The latest version shared. */
  version?: number;
  shared: boolean;
  /** May write (review.share): leave, answer and delete notes. */
  canWrite: boolean;
  /** Words picked in the script for the next comment, or none (a general comment). */
  draft?: Draft | null;
  onDraft?: (draft: Draft | null) => void;
  /** Shows a note's words in the script. */
  onJump?: (note: ReviewNote) => void;
  onRefresh: () => void;
  onError: (error: string) => void;
}) {
  const { hovered, active } = useLit();
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<"work" | "newest">(() => (localStorage.getItem("writer-comments-sort") === "newest" ? "newest" : "work"));
  const [read, setRead] = useState(() => (reviewId ? readSet(reviewId) : new Set<string>()));
  useEffect(() => setRead(reviewId ? readSet(reviewId) : new Set()), [reviewId]);
  const seen = (ids: string[]) => {
    if (!reviewId) return;
    markRead(reviewId, ids);
    setRead(readSet(reviewId));
  };

  const roots = notes.filter((note) => !note.parentId);
  const repliesOf = useMemo(() => {
    const out = new Map<string, ReviewNote[]>();
    for (const note of notes) if (note.parentId) out.set(note.parentId, [...(out.get(note.parentId) ?? []), note]);
    for (const list of out.values()) list.sort((a, b) => a.createdAt - b.createdAt);
    return out;
  }, [notes]);
  // #n per version, in the order written (the page's numbering).
  const numbers = useMemo(() => {
    const out = new Map<string, number>();
    const count = new Map<number, number>();
    for (const note of [...roots].sort((a, b) => a.createdAt - b.createdAt)) {
      const next = (count.get(note.version) ?? 0) + 1;
      count.set(note.version, next);
      out.set(note.id, next);
    }
    return out;
  }, [roots]);
  const unread = (note: ReviewNote) => !note.fromApp && !read.has(note.id);
  const threadUnread = (note: ReviewNote) => unread(note) || (repliesOf.get(note.id) ?? []).some(unread);
  const people = [...new Set(roots.map((note) => (note.fromApp ? "Writer" : note.author)))].sort();
  const counts = {
    all: roots.length,
    open: roots.filter((note) => note.status !== "done").length,
    done: roots.filter((note) => note.status === "done").length,
    unread: roots.filter(threadUnread).length,
    mine: roots.filter((note) => note.fromApp).length,
  };
  const shown = roots
    .filter((note) =>
      filter === "all"
        ? true
        : filter === "open"
          ? note.status !== "done"
          : filter === "done"
            ? note.status === "done"
            : filter === "unread"
              ? threadUnread(note)
              : filter === "mine"
                ? note.fromApp
                : (note.fromApp ? "Writer" : note.author) === filter.slice("person:".length),
    )
    .sort((a, b) =>
      sort === "newest"
        ? b.createdAt - a.createdAt
        : (a.anchor?.blockId ?? "~").localeCompare(b.anchor?.blockId ?? "~") || (a.anchor?.start ?? 0) - (b.anchor?.start ?? 0) || a.createdAt - b.createdAt,
    );

  const filterOptions = [
    { value: "all", label: "All comments", hint: String(counts.all) },
    { value: "open", label: "Open", hint: String(counts.open) },
    { value: "done", label: "Done", hint: String(counts.done) },
    { value: "unread", label: "Unread", hint: String(counts.unread) },
    { value: "mine", label: "Writer's", hint: String(counts.mine) },
    ...people.map((person) => ({ value: `person:${person}`, label: person, hint: String(roots.filter((note) => (note.fromApp ? "Writer" : note.author) === person).length) })),
    ...(counts.unread > 0 ? [{ value: "read", label: "Mark all as read" }] : []),
  ];

  const list = useRef<HTMLDivElement>(null);
  // A note opened from its words: its card comes into view.
  useEffect(() => {
    if (!active) return;
    list.current?.querySelector<HTMLElement>(`[data-note="${active}"]`)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [active]);

  if (!shared) {
    return (
      <div className="cm-panel">
        <p className="cm-empty">Share the script for review (Share, at the top): its comments are the feedback, the same here and on the review page.</p>
      </div>
    );
  }
  return (
    <div className="cm-panel">
      <div className="cm-toolbar">
        <Select
          size="sm"
          value={filter}
          label="Show"
          options={filterOptions}
          onChange={(value) => (value === "read" ? seen(notes.map((note) => note.id)) : setFilter(value as Filter))}
        />
        <span className="spacer" />
        <Select
          size="sm"
          align="end"
          value={sort}
          label="Sort"
          options={[
            { value: "work", label: "In order of the work" },
            { value: "newest", label: "Newest first" },
          ]}
          onChange={(value) => {
            setSort(value);
            localStorage.setItem("writer-comments-sort", value);
          }}
        />
      </div>
      <ReadyLine scriptId={scriptId} />
      <div className="cm-cards" ref={list}>
        {roots.length === 0 && <p className="cm-empty">No comments yet. Select words in the script to comment on them, or leave a general comment below.</p>}
        {roots.length > 0 && shown.length === 0 && <p className="cm-empty">Nothing here.</p>}
        {shown.map((note) => (
          <Card
            key={note.id}
            note={note}
            number={numbers.get(note.id)}
            current={version}
            replies={repliesOf.get(note.id) ?? []}
            unread={unread}
            lit={hovered === note.id || active === note.id}
            canWrite={canWrite}
            onOpen={() => {
              openNote(note.id);
              seen([note.id, ...(repliesOf.get(note.id) ?? []).map((reply) => reply.id)]);
              onJump?.(note);
            }}
            onReply={async (text) => {
              await replyReviewNote(scriptId, note.id, text);
              onRefresh();
            }}
            onCheck={async (verdict, text) => {
              await checkReviewNote(scriptId, note.id, verdict, text);
              onRefresh();
            }}
            onDelete={async () => {
              if (!window.confirm(note.intent === "suggestion" ? "Delete this suggestion?" : "Delete this note?")) return;
              try {
                await deleteReviewNote(scriptId, note.id);
                onRefresh();
              } catch (cause) {
                onError(message(cause));
              }
            }}
            onError={onError}
          />
        ))}
      </div>
      {canWrite && <Composer scriptId={scriptId} draft={draft ?? null} onDraft={onDraft} onSent={onRefresh} onError={onError} />}
    </div>
  );
}

function Card({
  note,
  number,
  current,
  replies,
  unread,
  lit,
  canWrite,
  onOpen,
  onReply,
  onCheck,
  onDelete,
  onError,
}: {
  note: ReviewNote;
  number?: number;
  current?: number;
  replies: ReviewNote[];
  unread: (note: ReviewNote) => boolean;
  lit: boolean;
  canWrite: boolean;
  onOpen: () => void;
  onReply: (text: string) => Promise<void>;
  /** A person's word on a fix: confirmed, or sent back with what still isn't right. */
  onCheck: (verdict: "confirmed" | "notFixed", text?: string) => Promise<void>;
  onDelete: () => void;
  onError: (error: string) => void;
}) {
  const [replying, setReplying] = useState<"reply" | "notFixed" | false>(false);
  const checkable = canWrite && note.status === "done" && !note.confirmed && note.outcome !== "rejected";
  const state = stateOf(note, replies);
  const author = note.fromApp ? "Writer" : note.author;
  const menu: MenuItem[] = [
    ...(note.anchor?.blockId ? [{ label: note.status === "done" && note.doneInVersion ? `Show the words (fixed in v${note.doneInVersion})` : "Show the words", icon: "search", onSelect: onOpen }] : []),
    ...(checkable
      ? [
          {
            label: "Confirm it's fixed",
            icon: "check",
            separated: true,
            onSelect: () => void onCheck("confirmed").catch((cause) => onError(message(cause))),
          },
          { label: "Not fixed…", icon: "reset", onSelect: () => setReplying("notFixed") },
        ]
      : []),
    ...(canWrite && note.fromApp ? [{ label: note.intent === "suggestion" ? "Delete suggestion" : "Delete note", danger: true, separated: true, onSelect: onDelete }] : []),
  ];
  const suggestion = note.intent === "suggestion" && note.replacement !== undefined && note.replacement !== null;
  return (
    <article
      className="cm-card"
      data-note={note.id}
      data-lit={lit || undefined}
      data-status={note.status}
      onPointerEnter={() => hoverNote(note.id)}
      onPointerLeave={() => hoverNote(null)}
    >
      <div className="cm-main" role="button" tabIndex={0} onClick={onOpen} onKeyDown={(event) => event.key === "Enter" && onOpen()}>
        <header className="cm-head">
          <Avatar note={note} />
          <b className="cm-author">{author}</b>
          <span className="cm-ago" title={new Date(note.createdAt).toLocaleString()}>{ago(note.createdAt)}</span>
          {unread(note) && <i className="cm-unread" aria-label="Unread" />}
          {number !== undefined && <span className="cm-number">#{number}</span>}
          <span className="spacer" />
          {menu.length > 0 && (
            <span onClick={(event) => event.stopPropagation()}>
              <Menu label="Note actions" items={menu} />
            </span>
          )}
        </header>
        <div className="cm-chips">
          {!suggestion && (
            <span className="cm-place">
              {note.anchor?.quote ? quoteChip(note.anchor.quote) : (
                <>
                  <Icon name="globe" size={11} /> General
                </>
              )}
            </span>
          )}
          {current !== undefined && note.version !== current && <span className="cm-tag">v{note.version}</span>}
          {note.intent && INTENT_LABELS[note.intent] && <span className={`cm-tag intent-${note.intent}`}>{INTENT_LABELS[note.intent]}</span>}
          {note.scope === "everywhere" && <span className="cm-tag">Everywhere</span>}
          {note.action && ACTIONS[note.action] && <span className="cm-tag action">{ACTIONS[note.action]}</span>}
        </div>
        {suggestion && (
          <p className="cm-suggest">
            {note.context?.before && <span className="faint">…{note.context.before}</span>}
            {note.anchor?.quote && <del>{note.anchor.quote}</del>}
            {note.replacement && <ins>{note.replacement}</ins>}
            {note.context?.after && <span className="faint">{note.context.after}…</span>}
          </p>
        )}
        {note.text && !(suggestion && /^(Replace|Delete|Add)\b/.test(note.text)) && <p className="cm-text">{note.text}</p>}
        {note.suggestion && <p className="cm-legacy">→ {note.suggestion}</p>}
        {state && (
          <p className={`cm-state ${state.kind}`} tabIndex={state.tip ? 0 : undefined}>
            <Icon name={state.kind === "done" || state.kind === "confirmed" || state.kind === "applied" ? "check" : state.kind === "working" ? "history" : "minus"} size={12} />
            <span className={state.tip ? "cm-summary" : undefined}>{state.label}</span>
            {state.tip && <span className="cm-tip" role="tooltip">{state.tip}</span>}
          </p>
        )}
      </div>
      {replies.length > 0 && (
        <ol className="cm-replies">
          {replies.map((reply) => (
            <li key={reply.id}>
              <span className="cm-reply-head">
                <b>{reply.fromApp && reply.author === "Writer" ? "Writer" : reply.author}</b>
                <span className="cm-ago">{ago(reply.createdAt)}</span>
                {unread(reply) && <i className="cm-unread" aria-label="Unread" />}
              </span>
              <span>{reply.text}</span>
            </li>
          ))}
        </ol>
      )}
      {canWrite && (
        <footer className="cm-foot">
          {replying ? (
            <ReplyForm
              back={replying === "notFixed"}
              onCancel={() => setReplying(false)}
              onSend={async (text) => {
                try {
                  if (replying === "notFixed") await onCheck("notFixed", text);
                  else await onReply(text);
                  setReplying(false);
                } catch (cause) {
                  onError(message(cause));
                }
              }}
            />
          ) : (
            <button type="button" className="link-button faint" onClick={() => setReplying("reply")}>
              Reply
            </button>
          )}
        </footer>
      )}
    </article>
  );
}

/** A reply, or (`back`) the page's "Not fixed…": what still isn't right, optional, then sent back to Writer. */
function ReplyForm({ back, onCancel, onSend }: { back?: boolean; onCancel: () => void; onSend: (text: string) => Promise<void> }) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const send = async () => {
    if ((!text.trim() && !back) || sending) return;
    setSending(true);
    await onSend(text);
    setSending(false);
  };
  return (
    <div className="cm-reply-form">
      {back && <span className="cm-back-label">What still isn’t right? Optional: it’s added as a reply.</span>}
      <textarea
        autoFocus
        rows={2}
        value={text}
        placeholder={back ? "e.g. The face is still visible at 1:04" : "Reply…"}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) void send();
          if (event.key === "Escape") onCancel();
        }}
      />
      <div className="cm-actions">
        <button type="button" className="ghost sm" onClick={onCancel}>Cancel</button>
        <button type="button" className={back ? "danger-soft sm" : "primary sm"} disabled={(!text.trim() && !back) || sending} onClick={() => void send()}>
          {sending ? "Sending…" : back ? "Send back to Writer" : "Reply"}
        </button>
      </div>
    </div>
  );
}

/** The page's composer: the words it's on (or General), a type, the comment, and new words for a suggestion. */
function Composer({ scriptId, draft, onDraft, onSent, onError }: { scriptId: string; draft: Draft | null; onDraft?: (draft: Draft | null) => void; onSent: () => void; onError: (error: string) => void }) {
  const [text, setText] = useState("");
  const [intent, setIntent] = useState<string>("");
  const [replacement, setReplacement] = useState("");
  const [sending, setSending] = useState(false);
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (draft) field.current?.focus();
  }, [draft]);
  useEffect(() => {
    if (!draft && intent === "suggestion") setIntent("");
  }, [draft, intent]);
  const suggesting = intent === "suggestion" && !!draft;
  const ready = suggesting ? true : !!text.trim();
  const send = async () => {
    if (!ready || sending) return;
    setSending(true);
    try {
      const details: NoteDetails = {};
      if (draft) details.anchor = { kind: "text", ...draft };
      if (intent) details.intent = intent;
      if (suggesting) details.replacement = replacement;
      await addReviewNote(scriptId, text, details);
      setText("");
      setReplacement("");
      setIntent("");
      onDraft?.(null);
      onSent();
    } catch (cause) {
      onError(message(cause));
    } finally {
      setSending(false);
    }
  };
  const typeItems: MenuItem[] = [
    ...CHANGES.map((value, index) => ({ label: INTENT_LABELS[value], checked: intent === value, separated: index === 0 ? false : undefined, onSelect: () => setIntent(value) })),
    ...(draft ? [{ label: "Suggest new words", checked: intent === "suggestion", onSelect: () => setIntent("suggestion") }] : []),
    ...NOT_CHANGES.map((value, index) => ({ label: INTENT_LABELS[value], checked: intent === value, separated: index === 0, onSelect: () => setIntent(value) })),
    ...(intent ? [{ label: "Clear the type", separated: true, onSelect: () => setIntent("") }] : []),
  ];
  return (
    <form
      className="cm-composer"
      onSubmit={(event) => {
        event.preventDefault();
        void send();
      }}
    >
      <div className="cm-composer-bar">
        <span className="cm-place" title={draft?.quote}>
          {draft ? (
            <>
              {quoteChip(draft.quote.length > 28 ? `${draft.quote.slice(0, 27)}…` : draft.quote)}
              <button type="button" aria-label="Let go of the words" onClick={() => onDraft?.(null)}>
                ×
              </button>
            </>
          ) : (
            "General · select words to comment on them"
          )}
        </span>
        <span className="spacer" />
        <Menu label="Type" trigger={<span className="cm-type">{intent ? INTENT_LABELS[intent] : "Type"} <Icon name="chevron" size={11} /></span>} items={typeItems} />
      </div>
      {suggesting && (
        <input
          className="cm-replacement"
          value={replacement}
          placeholder="New words (empty deletes them)"
          onChange={(event) => setReplacement(event.target.value)}
        />
      )}
      <textarea
        ref={field}
        rows={2}
        value={text}
        placeholder={suggesting ? "Add a comment (optional)" : draft ? "Leave a comment…" : "Select words in the script to comment on them, or leave a general comment…"}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) void send();
          if (event.key === "Escape" && !text) onDraft?.(null);
        }}
      />
      <div className="cm-actions">
        <span className="faint cm-hint">⌘↵ sends</span>
        <button type="submit" className="primary sm" disabled={!ready || sending}>
          {sending ? "Sending…" : suggesting ? "Suggest" : "Comment"}
        </button>
      </div>
    </form>
  );
}

/**
 * How "Ready for changes" is going, as the page's line under its toolbar
 * says it (feedback/src/client/ready.tsx), from the review's `fix` and
 * whether the fixing computer is on: asked every 12 s while shown.
 */
function ReadyLine({ scriptId }: { scriptId: string }) {
  const [state, setState] = useState<{ fix: ReviewFix | null; computer: "on" | "off" | "none" | null } | null>(null);
  const [seen, setSeen] = useState(() => Number(localStorage.getItem(`writer-ready-seen:${scriptId}`) ?? 0));
  useEffect(() => {
    let live = true;
    const ask = () => void reviewFix(scriptId).then((next) => live && setState(next), () => {});
    ask();
    const timer = window.setInterval(ask, 12_000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [scriptId]);
  const fix = state?.fix;
  if (!fix || fix.requestedAt === seen) return null;
  const running = fix.state === "fixing" || fix.state === "sharing";
  const ended = fix.state === "done" || fix.state === "failed" || (running && fix.stale);
  const text =
    fix.state === "waiting"
      ? state?.computer === "on"
        ? `${fix.by} asked for changes. Found a computer; starting…`
        : state?.computer === "none"
          ? "No fixing computer is chosen for Writer yet: choose one on the accounts dashboard. This waits until then."
          : "No computer is on to make these changes: this waits, and starts when it comes on."
      : running
        ? fix.stale
          ? "The fixing computer stopped answering. Ask again with Ready for changes."
          : (fix.stage ?? (fix.state === "sharing" ? `Sharing v${fix.version + 1}…` : fix.total ? `Fixing comment ${(fix.done ?? 0) + 1} of ${fix.total}…` : "Fixing the comments…"))
        : fix.state === "failed"
          ? `${fix.error ?? "Writer couldn't make the changes."} Ask again with Ready for changes.`
          : fix.newVersion
            ? `v${fix.newVersion} has the changes.`
            : (fix.stage ?? "Writer went through the comments; nothing needed a new version.");
  const tone = fix.state === "failed" || (running && fix.stale) ? "failed" : fix.state === "done" ? "done" : "working";
  return (
    <div className={`cm-ready ${tone}`}>
      <span className="cm-ready-text">
        <Icon name={tone === "done" ? "check" : tone === "failed" ? "minus" : "history"} size={12} />
        {text}
      </span>
      {ended && (
        <button
          type="button"
          className="link-button faint"
          onClick={() => {
            localStorage.setItem(`writer-ready-seen:${scriptId}`, String(fix.requestedAt));
            setSeen(fix.requestedAt);
          }}
        >
          Dismiss
        </button>
      )}
      {running && !fix.stale && fix.total ? (
        <span className="cm-ready-bar">
          <i style={{ width: `${Math.round((((fix.done ?? 0) + 0.5) / fix.total) * 100)}%` }} />
        </span>
      ) : null}
    </div>
  );
}
