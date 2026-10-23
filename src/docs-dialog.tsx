import { useEffect, useState } from "react";
import { docComments, exportDoc, getScript, listDocs, message } from "./api";
import { COMMENT_AUTHOR, docContent } from "./doc-export";
import { scriptDocx } from "./docx";
import type { DocComment, DocFile, Script } from "./types";
import { CAN, useGrants } from "./can";
import { Dialog } from "./ui";

/** Bytes as base64, a chunk at a time so a large file doesn't overflow the stack. */
function base64(bytes: Uint8Array) {
  let text = "";
  for (let at = 0; at < bytes.length; at += 0x8000) text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(text);
}

const madeAt = (value: string | number) =>
  new Date(value).toLocaleString(undefined, { day: "numeric", month: "short", hour: "numeric", minute: "2-digit" });

/** Comments people left, not the notes Writer put on the Doc itself. */
const people = (comments: DocComment[]) =>
  comments.filter((comment) => !comment.deleted && comment.author?.displayName !== COMMENT_AUTHOR && comment.content?.trim());

/** The comments as one note for a rewrite: what each is on, and what it says. */
export function commentsNote(comments: DocComment[]) {
  return people(comments)
    .filter((comment) => !comment.resolved)
    .map((comment) => {
      const on = comment.quotedFileContent?.value?.trim();
      const replies = (comment.replies ?? []).filter((reply) => !reply.deleted && reply.content?.trim()).map((reply) => ` / ${reply.content?.trim()}`);
      return `- ${on ? `On "${on.slice(0, 160)}": ` : ""}${comment.content?.trim()}${replies.join("")}`;
    })
    .join("\n");
}

/**
 * The script as a Google Doc, as Repack offers it: the latest export and
 * whether the script has changed since, a new export or a draft, every
 * earlier export, and the comments people left on the latest one.
 */
export function DocsDialog({
  open,
  script,
  onClose,
  onChange,
  onUseComments,
}: {
  open: boolean;
  script: Script;
  onClose: () => void;
  onChange: (script: Script) => void;
  /** Hands the open comments to a rewrite, when the script can be rewritten from them. */
  onUseComments?: (note: string) => void;
}) {
  const doc = script.docExport;
  // Exporting to Docs is part of sharing (review.share); reading a Doc's comments isn't.
  const mayShare = useGrants().has(CAN.share);
  const behind = !!doc && doc.revision !== script.revision;
  const [exporting, setExporting] = useState<"script" | "draft" | null>(null);
  const [made, setMade] = useState<DocFile | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [earlierOpen, setEarlierOpen] = useState(false);
  const [earlier, setEarlier] = useState<DocFile[] | null>(null);
  const [comments, setComments] = useState<DocComment[] | null>(null);
  const [commentsProblem, setCommentsProblem] = useState<string | null>(null);
  const [showResolved, setShowResolved] = useState(false);

  useEffect(() => {
    if (!open) return;
    setProblem(null);
    setMade(null);
    setComments(null);
    setCommentsProblem(null);
  }, [open]);

  // Read afresh each time: Docs are deleted in Drive itself.
  useEffect(() => {
    if (!open || !earlierOpen) return;
    let alive = true;
    setEarlier(null);
    listDocs(script.id)
      .then((list) => alive && setEarlier(list))
      .catch((cause) => alive && setProblem(message(cause)));
    return () => {
      alive = false;
    };
  }, [open, earlierOpen, script.id]);

  const readComments = async () => {
    setCommentsProblem(null);
    setComments(null);
    try {
      setComments((await docComments(script.id)).comments);
    } catch (cause) {
      setCommentsProblem(message(cause));
      setComments([]);
    }
  };

  const make = async (kind: "script" | "draft") => {
    setExporting(kind);
    setProblem(null);
    setMade(null);
    try {
      const docx = scriptDocx(docContent(script), COMMENT_AUTHOR, new Date(script.updatedAt || Date.now()).toISOString());
      const file = await exportDoc(script.id, kind === "draft", base64(docx));
      setMade(file);
      if (kind === "script") onChange(await getScript(script.id));
      if (earlierOpen) setEarlier(await listDocs(script.id).catch(() => earlier));
      window.open(file.url, "_blank", "noopener,noreferrer");
    } catch (cause) {
      setProblem(message(cause));
    } finally {
      setExporting(null);
    }
  };

  const shown = comments ? people(comments).filter((comment) => showResolved || !comment.resolved) : [];
  const open_ = comments ? people(comments).filter((comment) => !comment.resolved) : [];
  const exportable = script.commentary ? !!script.commentary.draft : script.clips.some((clip) => clip.kind === "cut-in");

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Google Doc"
      description="A new Doc from the script as it stands, with a comment on each timestamp, open to anyone with its link. Each export is a new Doc, as in Repack."
      width={560}
      footer={
        <>
          {problem && <span className="bad dialog-error">{problem}</span>}
          <span className="spacer" />
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
          {mayShare ? (
            <>
              <button
                type="button"
                disabled={!exportable || !!exporting}
                title="The same Doc, named “(DRAFT)” before the title"
                onClick={() => void make("draft")}
              >
                {exporting === "draft" ? "Exporting…" : "Export draft"}
              </button>
              <button type="button" className="primary" disabled={!exportable || !!exporting} onClick={() => void make("script")}>
                {exporting === "script" ? "Exporting…" : doc ? "Export again" : "Export"}
              </button>
            </>
          ) : (
            <span className="faint">Exporting needs “Share for review”: ask an admin.</span>
          )}
        </>
      }
    >
      <div className="doc-state">
        {doc ? (
          <a href={doc.url} target="_blank" rel="noreferrer">
            Latest export · {madeAt(doc.exportedAt)} ↗
          </a>
        ) : (
          <strong>Not exported yet</strong>
        )}
        <p className="hint">
          {!exportable
            ? "Write the script first."
            : behind
              ? "The script has changed since. Export again for an up-to-date Doc."
              : doc
                ? "Up to date with the script."
                : "Docs land in the Writer scripts folder of the account that owns Repack's script Docs."}
        </p>
        {made && (
          <p className="ok">
            Exported {made.draft ? "a draft" : "the script"}.{" "}
            <a href={made.url} target="_blank" rel="noreferrer">Open ↗</a>
          </p>
        )}
      </div>

      <div className="doc-section">
        <button type="button" className="ghost sm" aria-expanded={earlierOpen} onClick={() => setEarlierOpen(!earlierOpen)}>
          {earlierOpen ? "▾" : "▸"} All exports
        </button>
        {earlierOpen && (
          <div className="doc-list">
            {!earlier ? (
              <p className="hint">Looking in Drive…</p>
            ) : !earlier.length ? (
              <p className="hint">Nothing has been exported from this script yet.</p>
            ) : (
              <ul>
                {earlier.map((file) => (
                  <li key={file.id} data-state={file.state}>
                    <span className="kind">{file.draft ? "Draft" : "Script"}</span>
                    <span className="faint">
                      {file.createdAt ? madeAt(file.createdAt) : "Earlier"}
                      {file.state === "trashed" ? " · in Drive's trash" : ""}
                    </span>
                    <a href={file.url} target="_blank" rel="noreferrer">Open ↗</a>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>

      <div className="doc-section">
        <div className="row">
          <button type="button" className="ghost sm" disabled={!doc && !earlier?.length} onClick={() => void readComments()}>
            {comments === null ? "Read comments from the Doc" : "Read again"}
          </button>
          {comments && comments.length > 0 && (
            <label className="faint toggle">
              <input type="checkbox" checked={showResolved} onChange={(event) => setShowResolved(event.target.checked)} /> Resolved too
            </label>
          )}
        </div>
        {commentsProblem && <p className="bad">{commentsProblem}</p>}
        {comments && !commentsProblem && (
          <div className="doc-comments">
            {!shown.length ? (
              <p className="hint">No {showResolved ? "" : "open "}comments from people on the latest Doc.</p>
            ) : (
              <ul>
                {shown.map((comment) => (
                  <li key={comment.id} className={comment.resolved ? "resolved" : ""}>
                    {comment.quotedFileContent?.value && <q>{comment.quotedFileContent.value.slice(0, 200)}</q>}
                    <p>
                      <b>{comment.author?.displayName ?? "Someone"}</b> {comment.content}
                    </p>
                    {(comment.replies ?? [])
                      .filter((reply) => !reply.deleted && reply.content?.trim())
                      .map((reply, index) => (
                        <p className="reply" key={index}>
                          <b>{reply.author?.displayName ?? "Someone"}</b> {reply.content}
                        </p>
                      ))}
                  </li>
                ))}
              </ul>
            )}
            {onUseComments && open_.length > 0 && (
              <button
                type="button"
                className="sm"
                title="Opens a rewrite with these comments as its notes"
                onClick={() => {
                  onUseComments(commentsNote(comments));
                  onClose();
                }}
              >
                Rewrite with {open_.length} {open_.length === 1 ? "comment" : "comments"}…
              </button>
            )}
          </div>
        )}
      </div>
    </Dialog>
  );
}
