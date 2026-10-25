import { useCallback, useEffect, useState } from "react";
import { CommentsPanel, hoverNote } from "./comments";
import { approveReview, compressReview, fixNotes, restoreScript, message, onReviews, reviewCounts, reviewNotes, scriptAudit, shareScript } from "./api";
import type { BuildStatus, Cite, Paragraph, ReviewApproval, ReviewBlock, ReviewCounts, ReviewNote, Script } from "./types";
import { CAN, useGrants } from "./can";
import { Dialog, Icon } from "./ui";

/**
 * Review on a Repack link (reviews.rs): Share, the notes reviewers leave,
 * and what Claude changed for each, drawn as bands on the script. Hovering
 * a band says whose note it was, what they asked, and what Claude did.
 */

/** A note for the video editor: Edit's agent follows it in that section (a line starting "edit:"). */
export const isEditNote = (text: string | undefined) => !!text && text.split("\n").some((line) => /^\s*edit:/i.test(line));

/** Open notes per script, for the list's badges. */
export function useReviewCounts() {
  const [counts, setCounts] = useState<ReviewCounts | null>(null);
  useEffect(() => {
    let live = true;
    const ask = () => void reviewCounts().then((next) => live && setCounts(next), () => {});
    ask();
    const stop = onReviews(ask);
    const timer = window.setInterval(ask, 20_000);
    return () => {
      live = false;
      stop();
      window.clearInterval(timer);
    };
  }, []);
  return counts;
}

/** A script's notes and its blocks as they now read; asked again when notes change or the script does. */
export function useReview(id: string, revision: number) {
  const [notes, setNotes] = useState<ReviewNote[]>([]);
  const [blocks, setBlocks] = useState<ReviewBlock[]>([]);
  const [approved, setApproved] = useState<ReviewApproval | null>(null);
  const refresh = useCallback(
    () =>
      void reviewNotes(id).then(
        (found) => {
          setNotes(found.notes);
          setBlocks(found.blocks);
          setApproved(found.approved ?? null);
        },
        () => {},
      ),
    [id],
  );
  useEffect(() => {
    refresh();
    return onReviews(refresh);
  }, [refresh, revision]);
  return { notes, blocks, approved, refresh };
}

/** Part of a block's text a band covers: a fix's change, or an open note's words. */
export type Band = { start: number; end: number; notes: string[]; kind: "fix" | "open" | "note" };

/** Bands by block: each done note's changed ranges, and each open note's anchor. */
export function bandsOf(notes: ReviewNote[]): Map<string, Band[]> {
  const out = new Map<string, Band[]>();
  const add = (blockId: string, band: Band) => out.set(blockId, [...(out.get(blockId) ?? []), band]);
  for (const note of notes) {
    if (note.parentId) continue;
    if (note.status === "done") {
      for (const range of note.changedRanges) add(range.blockId, { start: range.start, end: range.end, notes: [note.id], kind: "fix" });
    } else if (note.anchor?.blockId) {
      add(note.anchor.blockId, { start: note.anchor.start ?? 0, end: note.anchor.end ?? 0, notes: [note.id], kind: "open" });
    }
  }
  return out;
}

/** A moment as blocks.rs prints a clip: whole seconds, rounded down. */
function stamp(ms: number) {
  const seconds = Math.floor(Math.max(0, ms) / 1000);
  const hours = Math.floor(seconds / 3600);
  const pad = (value: number) => String(value).padStart(2, "0");
  return hours > 0 ? `${hours}:${pad(Math.floor(seconds / 60) % 60)}:${pad(seconds % 60)}` : `${Math.floor(seconds / 60)}:${pad(seconds % 60)}`;
}

const citeLine = (cite: Cite) => `[${cite.video} ${stamp(cite.startMs)}–${stamp(cite.endMs)}]${cite.quote.trim() ? ` "${cite.quote.trim()}"` : ""}`;

/**
 * A paragraph's bands, piece by piece: its text is each piece on its own
 * line (blocks.rs paragraph_text), so a band on narration lands in that
 * piece's own offsets, and one touching a clip marks the clip.
 */
export function pieceBands(paragraph: Paragraph, bands: Band[] | undefined): Map<number, Band[]> {
  const out = new Map<number, Band[]>();
  if (!bands?.length) return out;
  let at = 0;
  paragraph.pieces.forEach((piece, index) => {
    const text = piece.kind === "say" ? piece.text : citeLine(piece);
    const [from, to] = [at, at + text.length];
    for (const band of bands) {
      // An empty band at a piece's start belongs to it; elsewhere a band must overlap.
      const touches = band.start === band.end ? band.start >= from && band.start <= to : band.start < to && band.end > from;
      if (!touches) continue;
      const local = { ...band, start: Math.max(0, band.start - from), end: Math.min(text.length, band.end - from) };
      out.set(index, [...(out.get(index) ?? []), local]);
    }
    at = to + 1;
  });
  return out;
}

/** Where a piece's text starts in its paragraph's block text (each piece on its own line, blocks.rs paragraph_text): a comment's words are named there. */
export function pieceOffset(paragraph: Paragraph, index: number) {
  let at = 0;
  paragraph.pieces.slice(0, index).forEach((piece) => (at += (piece.kind === "say" ? piece.text : citeLine(piece)).length + 1));
  return at;
}

/** What an element-wide band carries for the tooltip (a timestamp, a clip, a heading). */
export function bandProps(bands: Band[] | undefined) {
  if (!bands?.length) return {};
  return { "data-band": bands.flatMap((band) => band.notes).join(" "), "data-band-kind": bands.some((band) => band.kind === "fix") ? "fix" : "open" };
}

// ---- The tooltip --------------------------------------------------------------

type Tip = { ids: string[]; x: number; y: number } | null;
let tipListener: ((tip: Tip) => void) | null = null;

/** Shows the notes behind a band at the pointer; null hides it. */
export function showTip(tip: Tip) {
  tipListener?.(tip);
}

/**
 * The card a band shows on hover: the reviewer's words and name, Claude's
 * summary of what it changed, and the version. Elements marked with
 * `data-band` (bandProps) show it by themselves; text bands call showTip.
 */
export function BandTip({ notes }: { notes: ReviewNote[] }) {
  const [tip, setTip] = useState<Tip>(null);
  useEffect(() => {
    tipListener = setTip;
    const over = (event: PointerEvent) => {
      const target = (event.target as HTMLElement | null)?.closest?.<HTMLElement>("[data-band]");
      if (target) {
        const ids = (target.dataset.band ?? "").split(" ").filter(Boolean);
        setTip({ ids, x: event.clientX, y: event.clientY });
        hoverNote(ids[0] ?? null);
      } else if ((event.target as HTMLElement | null)?.closest?.("[data-band-text]") == null) setTip(null);
    };
    window.addEventListener("pointermove", over);
    return () => {
      window.removeEventListener("pointermove", over);
      if (tipListener === setTip) tipListener = null;
    };
  }, []);
  const shown = tip ? notes.filter((note) => tip.ids.includes(note.id)) : [];
  if (!tip || !shown.length) return null;
  const left = Math.min(tip.x + 14, window.innerWidth - 340);
  const top = tip.y + 16 + 220 > window.innerHeight ? Math.max(8, tip.y - 230) : tip.y + 16;
  return (
    <div className="band-tip" style={{ left, top }} role="tooltip">
      {shown.map((note) => (
        <div key={note.id} className="band-tip-note">
          <span className="band-tip-head">
            <b>{note.author}</b>
            <span className="faint">{note.status === "done" ? `Fixed in v${note.doneInVersion ?? "?"}` : `Open · on v${note.version}`}</span>
          </span>
          <q>{note.text}</q>
          {note.fixSummary && (
            <span className="band-tip-fix">
              <span className="faint">Claude: </span>
              {note.fixSummary}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}

// ---- Share and the notes -------------------------------------------------------

const when = (ms: number) => new Date(ms).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

/** The topbar's Share button: "Share" until shared, then the open notes' count. */
export function ReviewButton({ script, open, approved, onClick }: { script: Script; open: number; approved: ReviewApproval | null; onClick: () => void }) {
  const shared = !!script.review;
  return (
    <button
      type="button"
      className={approved ? "secondary sm review-button approved" : open ? "secondary sm review-button has-notes" : "secondary sm review-button"}
      title={
        approved
          ? `Approved v${approved.version} by ${approved.by}, ${when(approved.at)}.`
          : shared
            ? `Shared for review (v${script.review?.version}). ${open ? `${open} open ${open === 1 ? "note" : "notes"}.` : "No open notes."}`
            : "Share the script on a Repack link for review"
      }
      onClick={onClick}
    >
      <Icon name={approved ? "check" : "share"} size={14} />
      {approved ? `Approved v${approved.version}` : shared ? "Review" : "Share"}
      {open > 0 && <span className="count-badge">{open}</span>}
    </button>
  );
}

/**
 * Sharing and the notes: the link to copy, Share again for the next version,
 * whether Claude fixes new notes by itself, and every note with what Claude
 * did about it.
 */
export function ReviewDialog({
  open,
  script,
  notes,
  approved,
  busy,
  onClose,
  onChange,
  onJob,
  onRefresh,
}: {
  open: boolean;
  script: Script;
  notes: ReviewNote[];
  /** The approved version, made here or on Repack's page. */
  approved: ReviewApproval | null;
  busy: boolean;
  onClose: () => void;
  onChange: (script: Script) => void;
  /** A fix started from here, for the editor to follow. */
  onJob: (job: BuildStatus) => void;
  onRefresh: () => void;
}) {
  const [sharing, setSharing] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [fixesHere, setFixesHere] = useState<boolean | null>(null);
  // Sharing, approving and fixing need review.share; reading the notes doesn't.
  const mayShare = useGrants().has(CAN.share);
  // What a reviewer would flag, checked before it's shared, as Edit's audit is before an export.
  const [flags, setFlags] = useState<string[]>([]);
  useEffect(() => {
    if (!open) return;
    setProblem(null);
    onRefresh();
    void reviewCounts().then((counts) => setFixesHere(counts.fixesHere), () => {});
  }, [open]);
  useEffect(() => {
    if (!open) return;
    void scriptAudit(script.id).then((found) => setFlags(found.flags), () => setFlags([]));
  }, [open, script.id, script.revision]);

  const link = script.review;
  const share = async () => {
    setSharing(true);
    setProblem(null);
    try {
      onChange(await shareScript(script.id));
    } catch (cause) {
      setProblem(message(cause));
    } finally {
      setSharing(false);
    }
  };
  // The script as it is now as the link's only version, v1: the earlier versions go, their notes move onto v1.
  const makeFirst = async () => {
    if (!link) return;
    if (!window.confirm(`Make the script as it is now v1 on this link? v1 to v${link.version} are removed from the link and every note is kept on v1. Writer's own version history is untouched.`)) return;
    setSharing(true);
    setProblem(null);
    try {
      onChange(await compressReview(script.id));
    } catch (cause) {
      setProblem(message(cause));
    } finally {
      setSharing(false);
    }
  };
  const copy = async () => {
    if (!link) return;
    await navigator.clipboard.writeText(link.url);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  const [approving, setApproving] = useState(false);
  const approve = async (yes: boolean) => {
    if (!link) return;
    const ask = yes
      ? `Approve v${link.version}? The review link stops taking notes and the other versions' files are deleted. Every note from every round is kept, and Writer's own version history is untouched.`
      : `Take back the approval of v${approved?.version}? The link takes notes again; files already deleted stay deleted.`;
    if (!window.confirm(ask)) return;
    setApproving(true);
    setProblem(null);
    try {
      await approveReview(script.id, yes);
      onRefresh();
    } catch (cause) {
      setProblem(message(cause));
    } finally {
      setApproving(false);
    }
  };
  const fix = async () => {
    setProblem(null);
    try {
      onJob(await fixNotes(script.id));
    } catch (cause) {
      setProblem(message(cause));
    }
  };

  const roots = notes.filter((note) => !note.parentId);
  const openNotes = roots.filter((note) => note.status !== "done");
  const waiting = openNotes.length;
  const behind = link && link.revision !== script.revision;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={link ? "Review" : "Share for review"}
      description={
        link
          ? "Anyone with the link can read the script and leave notes under a name. Share again to publish the script as it is now, as the next version on the same link."
          : "Publishes the script on a Repack link. Anyone with it can read the script and leave notes under a name, with no account."
      }
      width={560}
      footer={
        <>
          {problem && <span className="bad">{problem}</span>}
          <span className="spacer" />
          {!mayShare && <span className="faint">Sharing, approving and fixing need “Share for review”: ask an admin.</span>}
          {mayShare && link && !approved && (
            <button type="button" className="ghost" disabled={busy || approving} title="The script is done: the link takes no more notes, and the other versions' files are deleted (their notes stay)" onClick={() => void approve(true)}>
              {approving ? "Approving…" : `Approve v${link.version}`}
            </button>
          )}
          {mayShare && link && waiting > 0 && !approved && (
            <button type="button" className="ghost" disabled={busy} onClick={() => void fix()}>
              Fix {waiting} {waiting === 1 ? "note" : "notes"} now
            </button>
          )}
          {mayShare && link && link.version > 1 && !approved && (
            <button type="button" className="ghost" disabled={sharing || busy} title="The script as it is now becomes the link's only version, v1; every note is kept" onClick={() => void makeFirst()}>
              Make this v1
            </button>
          )}
          {mayShare && (
            <button type="button" className={link && !behind ? "ghost" : "primary"} disabled={sharing || busy} onClick={() => void share()}>
              {sharing ? "Sharing…" : `${link ? `Share again as v${link.version + 1}` : "Share"}${flags.length ? " anyway" : ""}`}
            </button>
          )}
        </>
      }
    >
      {mayShare && flags.length > 0 && !approved && (
        <div className="review-flags">
          <strong>Before sharing: {flags.length === 1 ? "one thing" : `${flags.length} things`} the team would flag</strong>
          <ul>
            {flags.map((flag, index) => (
              <li key={index}>{flag}</li>
            ))}
          </ul>
        </div>
      )}
      {link && (
        <div className="review-link">
          <input readOnly value={link.url} aria-label="Review link" onFocus={(event) => event.currentTarget.select()} />
          <button type="button" className="sm" onClick={() => void copy()}>
            <Icon name={copied ? "check" : "copy"} size={14} />
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
      )}
      {link && (
        <p className="faint review-meta">
          v{link.version} shared {when(link.sharedAt)}
          {behind ? " · the script has changed since" : ""}
        </p>
      )}
      {approved && (
        <p className="review-approved">
          <Icon name="check" size={14} />
          <span>
            <b>Approved v{approved.version}</b> · by {approved.by} · {when(approved.at)}
            <span className="faint"> · The link takes no more notes, and nothing is fixed by itself. Sharing again starts v{(link?.version ?? approved.version) + 1}, unapproved.</span>
          </span>
          {mayShare && (
            <button type="button" className="link-button" disabled={approving} onClick={() => void approve(false)}>
              Take back
            </button>
          )}
        </p>
      )}
      {mayShare && link && fixesHere !== null && !approved && (
        <p className="faint review-auto">
          {fixesHere
            ? "When a reviewer clicks Ready for changes, Claude fixes the notes on this Mac and shares the next version."
            : "When a reviewer clicks Ready for changes, the fixing computer chosen on accounts.spheceo.com fixes the notes and shares the next version."}
        </p>
      )}
      {link && (
        <CommentsPanel
          scriptId={script.id}
          reviewId={link.reviewId}
          notes={notes}
          version={link.version}
          shared
          canWrite={mayShare && !approved}
          onRefresh={onRefresh}
          onError={setProblem}
        />
      )}
    </Dialog>
  );
}

// ---- Archived ---------------------------------------------------------------------

const gigabytes = (bytes: number) => (bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`);

/**
 * An archived script, read-only: its footage was deleted to free space (from
 * Repack's page), its words and everything small kept. Restore brings the
 * footage back from its links; what came from this Mac's disk can't come back.
 */
export function ArchivedBanner({ script, working, onJob, onProblem }: { script: Script; working: boolean; onJob: (job: BuildStatus) => void; onProblem: (text: string) => void }) {
  const archive = script.archive;
  if (!archive) return null;
  const restore = async () => {
    try {
      onJob(await restoreScript(script.id));
    } catch (cause) {
      onProblem(message(cause));
    }
  };
  return (
    <div className="notice archived-notice">
      <span>
        <b>Footage archived</b> by {archive.by || "Repack"}, {when(archive.at)}
        {archive.freedBytes > 0 ? `, freeing ${gigabytes(archive.freedBytes)}` : ""}. The script and its words are kept; it reads but can't be edited until the footage is back.
        {archive.missing.length > 0 && <span className="bad"> Missing after the last restore (from this Mac's disk): {archive.missing.join(", ")}.</span>}
      </span>
      <button type="button" className="primary sm" disabled={working || archive.restoring} onClick={() => void restore()}>
        {working || archive.restoring ? "Restoring…" : "Restore footage"}
      </button>
    </div>
  );
}
