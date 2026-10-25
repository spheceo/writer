import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { blocks, clipRange, duration, generatedHeading, heading, isCutIn, keptMs } from "./clips";
import { BLUR_NOTE, caseStarts, COMMENT_AUTHOR, blurSpans, boundary, reuseNote } from "./doc-export";
import { Grow, LETTER, Paper, pageInView, useScale, useZoom, ZoomTools } from "./paper";
import { editedPosition, type Playhead, usePlayheadValue } from "./playback";
import { wordDiff } from "./diff";
import type { Clip, Note, Script } from "./types";
import { type Band, bandProps, isEditNote } from "./review";
import { Icon } from "./ui";
import { ScriptSelectionActions, type ScriptSelection } from "./script-selection";
import { noteRange } from "./notes";

/**
 * The script as Repack's Script view shows it: Letter pages in the exported
 * Doc's type, the title line, its footage and length, then each block's
 * timestamps and the narration under them, with the Doc's comments beside.
 * In Split it follows the preview: the timestamp playing is underlined in red
 * as it runs out, and clicking one plays it.
 */
export function FootageDoc({
  script,
  clips,
  playhead,
  follow,
  zoomKey,
  disabled,
  onNarration,
  onHeading,
  onResolve,
  onChat,
  onNotes,
  onPreview,
  bands,
}: {
  script: Script;
  clips: Clip[];
  playhead: Playhead;
  /** Whether the page follows what plays beside it. */
  follow: boolean;
  zoomKey: string;
  disabled: boolean;
  onNarration: (id: string, text: string) => void;
  /** The producer's own heading; empty puts the generated one back. */
  onHeading: (text: string) => void;
  onResolve: (ids: string[], accept: boolean) => void;
  onChat: (selection: { id: string; quote: string; start: number; end: number; narrationNumber: number }) => void;
  /** The comments on a timestamp, as they now stand. */
  onNotes: (id: string, notes: Note[]) => void;
  onPreview: (index: number) => void;
  /** Review bands by block (a clip's ID): what each fix changed, and open notes' words. */
  bands?: Map<string, Band[]>;
}) {
  const canvas = useRef<HTMLDivElement>(null);
  const [zoom, setZoom] = useZoom(zoomKey);
  // Comments are kept per view: on beside the Script view's page, off in Split, where the page is narrow.
  const [comments, setComments] = useState(() => {
    const saved = window.localStorage.getItem(`writer.comments.${zoomKey}`);
    return saved ? saved === "on" : !zoomKey.endsWith("split");
  });
  const scale = useScale(canvas, zoom, LETTER, comments);
  const [page, setPage] = useState(1);
  const [pages, setPages] = useState(1);
  const [active, setActive] = useState<string | null>(null);
  // The timestamp last clicked, which can take a comment, as in Repack.
  const [chosen, setChosen] = useState<string | null>(null);
  // The timestamp a comment is being written on, and its words so far.
  const [writing, setWriting] = useState<{ id: string; text: string; anchor?: Note["anchor"] } | null>(null);
  const [selection, setSelection] = useState<ScriptSelection | null>(null);
  const [botNotes, setBotNotes] = useState(false);
  const saveNote = () => {
    if (!writing?.text.trim()) return setWriting(null);
    const clip = clips.find((item) => item.id === writing.id);
    if (clip) onNotes(clip.id, [...(clip.notes ?? []), { id: crypto.randomUUID(), text: writing.text.trim().slice(0, 2000), at: Date.now(), anchor: writing.anchor }]);
    setWriting(null);
  };
  const clicked = useRef(false);
  useEffect(() => window.localStorage.setItem(`writer.comments.${zoomKey}`, comments ? "on" : "off"), [zoomKey, comments]);

  const parts = blocks(clips);
  const cases = caseStarts(script);
  const nowId = usePlayheadValue(playhead, (p) => (follow && !p.auditioning ? (editedPosition(clips, p.ms)?.clip.id ?? null) : null));

  // The playing timestamp's red line fills as its clip runs out.
  useLayoutEffect(() => {
    if (!nowId) return;
    const node = canvas.current?.querySelector<HTMLElement>(`[data-range="${nowId}"]`);
    if (!node) return;
    const paint = () => {
      const at = editedPosition(clips, playhead.ms);
      if (at?.clip.id === nowId) node.style.setProperty("--progress", String((at.sourceMs - at.clip.startMs) / Math.max(1, at.clip.endMs - at.clip.startMs)));
    };
    paint();
    return playhead.subscribe(paint);
  }, [nowId, clips, playhead]);

  // A new line playing, or one clicked, is kept a third of the way down.
  useLayoutEffect(() => {
    if (!nowId || (!playhead.playing && !clicked.current)) return;
    clicked.current = false;
    const scroller = canvas.current;
    const node = scroller?.querySelector<HTMLElement>(`[data-range="${nowId}"], [data-cue="${nowId}"]`);
    if (!scroller || !node) return;
    const top = node.getBoundingClientRect().bottom - scroller.getBoundingClientRect().top + scroller.scrollTop - scroller.clientHeight / 3;
    scroller.scrollTo({ top: Math.max(0, top), behavior: "smooth" });
  }, [nowId, playhead]);

  const cues = clips.filter(isCutIn);
  const words = cues.reduce((sum, cue) => sum + (cue.narration ?? "").split(/\s+/).filter(Boolean).length, 0);

  const notesFor = (clip: Clip, anchor: string) => [
    ...(clip.notes ?? []).map((note) => <div className="comment human" key={note.id} data-card={anchor} data-state={active === clip.id ? "active" : undefined} data-edit={isEditNote(note.text) || undefined}>
      <span className="card-head"><span className="avatar">Y</span><span className="comment-author">You</span>
        <button type="button" className="icon-tool sm card-remove" aria-label="Delete comment" disabled={disabled} onClick={() => onNotes(clip.id, (clip.notes ?? []).filter((item) => item.id !== note.id))}><Icon name="close" size={12} /></button>
      </span>
      {note.anchor && <blockquote className="card-quote">{note.anchor.quote}</blockquote>}
      <span className="card-text">{note.text}</span>
    </div>),
    writing?.id === clip.id ? <form className="comment human writing" key={`write-${clip.id}`} data-card={anchor} onSubmit={(event) => { event.preventDefault(); saveNote(); }}>
      {writing.anchor && <blockquote className="card-quote">{writing.anchor.quote}</blockquote>}
      <textarea autoFocus aria-label="Comment on selected section" rows={3} maxLength={2000} value={writing.text} placeholder="Add a comment…" onChange={(event) => setWriting({ ...writing, text: event.target.value })} onKeyDown={(event) => { if (event.key === "Escape") setWriting(null); if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); saveNote(); } }} />
      <span className="card-actions"><button type="button" className="sm ghost" onClick={() => setWriting(null)}>Cancel</button><button type="submit" className="sm primary" disabled={disabled || !writing.text.trim()}>Comment</button></span>
    </form> : chosen === clip.id && !disabled ? <button type="button" className="comment add" key={`add-${clip.id}`} data-card={anchor} onClick={() => setWriting({ id: clip.id, text: "" })}>+ Add comment</button> : null,
  ];

  const rail = comments
    ? parts.flatMap((block) => [
        ...block.clips.flatMap(({ clip }) => [
          ...(botNotes ? [<button
            type="button"
            className="comment"
            key={clip.id}
            data-card={clip.id}
            data-state={active === clip.id ? "active" : undefined}
            onMouseEnter={() => setActive(clip.id)}
            onMouseLeave={() => setActive(null)}
            onClick={() => {
              clicked.current = true;
              setChosen(clip.id);
              onPreview(block.clips.find((item) => item.clip.id === clip.id)?.index ?? 0);
            }}
          >
            <span className="card-head">
              <span className="avatar bot">G</span>
              <span className="comment-author">{COMMENT_AUTHOR}</span>
            </span>
            <span className="card-text">{boundary(clip, script.words, script.corrections)}</span>
          </button>] : []),
          ...notesFor(clip, clip.id),
        ]),
        ...(block.cue ? notesFor(block.cue.clip, `cue-${block.cue.clip.id}`) : []),
        ...(block.cue?.clip.reuse
          ? [
              <div className="comment" key={`reuse-${block.cue.clip.id}`} data-card={`cue-${block.cue.clip.id}`}>
                <span className="card-head">
                  <span className="avatar bot">G</span>
                  <span className="comment-author">{COMMENT_AUTHOR}</span>
                </span>
                <span className="card-text">
                  Reuse:{" "}
                  <a href={block.cue.clip.reuse} target="_blank" rel="noreferrer">
                    {block.cue.clip.reuse}
                  </a>
                  {reuseNote(script, block.cue.clip.reuse).split("\n").slice(1).map((line) => (
                    <span key={line}>
                      <br />
                      {line}
                    </span>
                  ))}
                </span>
              </div>,
            ]
          : []),
        ...(block.cue && blurSpans(block.cue.clip.narration ?? "").length
          ? [
              <div className="comment" key={`blur-${block.cue.clip.id}`} data-card={`cue-${block.cue.clip.id}`}>
                <span className="card-head">
                  <span className="avatar bot">G</span>
                  <span className="comment-author">{COMMENT_AUTHOR}</span>
                </span>
                <span className="card-text">
                  {BLUR_NOTE}:{" "}
                  {blurSpans(block.cue.clip.narration ?? "")
                    .map(([a, b]) => `“${(block.cue?.clip.narration ?? "").slice(a, b)}”`)
                    .join(", ")}
                </span>
              </div>,
            ]
          : []),
      ])
    : null;

  // A Courtroom case runs on after its first block in its Doc; here, as in
  // Repack's Script view, its blocks only lose the blank lines between them.
  const running = script.kind === "courtroom" && !script.compilation;

  /** A block's timestamps, each previewing its clip. */
  const ranges = (block: (typeof parts)[number]) =>
    block.clips.map(({ clip, index: at }, position) => (
      <Fragment key={clip.id}>
        {position ? <span className="plus">+</span> : null}
        <span
          className="range"
          data-range={clip.id}
          data-anchor={clip.id}
          data-now={nowId === clip.id || undefined}
          data-state={active === clip.id ? "active" : undefined}
          data-plain={botNotes || clip.notes?.length ? undefined : ""}
          {...bandProps(bands?.get(clip.id))}
          onMouseEnter={() => setActive(clip.id)}
          onMouseLeave={() => setActive(null)}
        >
          (
          <button
            type="button"
            title="Play this clip"
            aria-label={`Preview ${clipRange(clip, script.parts, " to ")}`}
            onClick={() => {
              clicked.current = true;
              setChosen(clip.id);
              onPreview(at);
            }}
          >
            {clipRange(clip, script.parts)}
          </button>
          )
        </span>
      </Fragment>
    ));

  const narration = (cue: Clip, index: number) => {
    const proposal = script.proposals?.find((item) => item.cutIn === cue.id);
    const text = cue.narration ?? "";
    const localBands: Band[] = (cue.notes ?? []).flatMap((note) => {
      if (!note.anchor) return [];
      const span = noteRange(text, note.anchor);
      return span ? [{ ...span, notes: [], kind: "note" as const }] : [];
    });
    return proposal ? <section className="proposed-narration" aria-label={`Proposed narration ${index + 1}`}>
      {wordDiff(proposal.before, proposal.after).map((piece, at) => piece.kind === "same" ? <span key={at}>{piece.text}</span> : piece.kind === "add" ? <ins key={at}>{piece.text}</ins> : <del key={at}>{piece.text}</del>)}
    </section> : <Grow className="narration" label={`Narration ${index + 1}`} value={text} disabled={disabled} placeholder="Write the narration for this cut-in…" bands={[...(bands?.get(cue.id) ?? []), ...localBands]} onCommit={(text) => onNarration(cue.id, text)} onWords={(start, end, _rect, input) => setSelection({ id: cue.id, input, start, end, quote: input.value.slice(start, end) })} />;
  };
  const proposals = script.proposals ?? [];
  const quoteToChat = (picked: Pick<ScriptSelection, "id" | "start" | "end" | "quote">) => {
    const narrationNumber = parts.findIndex((block) => block.cue?.clip.id === picked.id) + 1;
    onChat({ ...picked, narrationNumber });
  };

  return (
    <section className="document" aria-label="Script document">
      {selection && <ScriptSelectionActions selection={selection} scroller={canvas} onDismiss={() => setSelection(null)} onComment={() => {
        const { id, start, end, quote } = selection;
        setComments(true);
        setChosen(id);
        setWriting({ id, text: "", anchor: { start, end, quote } });
        setSelection(null);
      }} onChat={() => { quoteToChat(selection); setSelection(null); }} />}
      <header className="doc-toolbar">
        <ZoomTools zoom={zoom} scale={scale} page={page} pages={pages} onZoom={setZoom} />
        <div className="doc-actions">
          <span className="doc-stats">
            {cues.length} cut-ins · {words.toLocaleString()} words
          </span>
          <button type="button" className="icon-tool" aria-label="Add comment on selected timestamp" title={chosen ? "Add comment on selected timestamp" : "Select a timestamp to add a comment"} disabled={disabled || !chosen} onClick={() => { if (chosen) { setComments(true); setWriting({ id: chosen, text: "" }); } }}><Icon name="plus" size={15} /></button>
          {comments && <button type="button" className="icon-tool" aria-label="Show transcript boundary comments" title="Transcript boundary comments" aria-pressed={botNotes} onClick={() => setBotNotes(!botNotes)}><Icon name="file" size={15} /></button>}
          <button
            type="button"
            className="icon-tool"
            aria-pressed={comments}
            aria-label={comments ? "Hide comments" : "Show comments"}
            title={comments ? "Hide comments" : "Show comments"}
            onClick={() => setComments(!comments)}
          >
            <Icon name="note" size={15} />
          </button>
        </div>
      </header>
      {proposals.length > 0 && <div className="proposal-review" role="region" aria-label="Suggested changes">
        <div><strong>Suggested changes</strong><span><del>Removed</del> <ins>Added</ins> · {proposals.length} {proposals.length === 1 ? "paragraph" : "paragraphs"}</span></div>
        <div className="proposal-review-actions">
          <button type="button" className="tool" disabled={disabled} onClick={() => quoteToChat({ id: proposals[0].cutIn, start: 0, end: proposals[0].before.length, quote: proposals[0].before })}><Icon name="note" size={14} /> Follow up</button>
          <button type="button" className="tool" disabled={disabled} onClick={() => onResolve(proposals.map((item) => item.id), false)}><Icon name="close" size={14} /> Reject</button>
          <button type="button" className="tool accept-changes" disabled={disabled} onClick={() => onResolve(proposals.map((item) => item.id), true)}><Icon name="check" size={14} /> Accept changes</button>
        </div>
      </div>}
      <div className="canvas" ref={canvas} onScroll={(event) => setPage(pageInView(event.currentTarget, scale, LETTER))}>
        <Paper sheet={LETTER} scale={scale} rail={rail} version={comments} onPages={setPages}>
          <h1 className="doc-title" data-block>
            <Grow
              className="doc-title-input"
              label="Heading"
              oneLine
              value={heading(script)}
              disabled={disabled}
              onCommit={(text) => onHeading(text.trim() === generatedHeading(script) ? "" : text)}
            />
          </h1>
          <section className="assets" data-block>
            <p>Total Duration: {duration(keptMs(clips))}</p>
          </section>
          {parts.length === 0 && (
            <p className="doc-empty" data-block>
              The script appears here once it is built, laid out as its Google Doc will be.
            </p>
          )}
          {parts.map((block, index) => (
            <Fragment key={block.cue?.clip.id ?? block.clips[0]?.clip.id ?? index}>
              {cases.indexOf(index) > 0 && <hr className="case-divider" data-block />}
              {block.clips.length > 0 && (
                <p className="ranges" data-block data-tight={(running && index > 0) || undefined}>
                  {ranges(block)}
                </p>
              )}
              {block.cue && (
                <div
                  className="cue"
                  data-block
                  data-tight={running || undefined}
                  data-cue={block.cue.clip.id}
                  data-anchor={`cue-${block.cue.clip.id}`}
                  data-now={nowId === block.cue.clip.id || undefined}
                >
                  {narration(block.cue.clip, index)}
                </div>
              )}
            </Fragment>
          ))}
          {script.kind === "courtroom" && parts.length > 0 && (
            <p className="doc-total" data-block>
              Total length of clips: {duration(keptMs(clips.filter((clip) => !isCutIn(clip))))}
            </p>
          )}
        </Paper>
      </div>
    </section>
  );
}
