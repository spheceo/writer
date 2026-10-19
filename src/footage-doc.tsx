import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import { blocks, clipRange, duration, generatedHeading, heading, isCutIn, keptMs } from "./clips";
import { assetLinks, BLUR_NOTE, caseStarts, COMMENT_AUTHOR, blurSpans, boundary, rangeUrl } from "./doc-export";
import { Grow, LETTER, Paper, pageInView, useScale, useZoom, ZoomTools } from "./paper";
import { editedPosition, type Playhead, usePlayheadValue } from "./playback";
import { wordDiff } from "./diff";
import type { Clip, Proposal, Script } from "./types";
import { Icon } from "./ui";

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
  onDecline,
  onPreview,
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
  /** Declines a proposed rewrite. Accepting writes it as the narration. */
  onDecline: (proposal: Proposal) => void;
  onPreview: (index: number) => void;
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
  const clicked = useRef(false);
  useEffect(() => window.localStorage.setItem(`writer.comments.${zoomKey}`, comments ? "on" : "off"), [zoomKey, comments]);

  const parts = blocks(clips);
  const assets = assetLinks(script);
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

  const rail = comments
    ? parts.flatMap((block) => [
        ...block.clips.map(({ clip }) => (
          <button
            type="button"
            className="comment"
            key={clip.id}
            data-card={clip.id}
            data-state={active === clip.id ? "active" : undefined}
            onMouseEnter={() => setActive(clip.id)}
            onMouseLeave={() => setActive(null)}
            onClick={() => {
              clicked.current = true;
              onPreview(block.clips.find((item) => item.clip.id === clip.id)?.index ?? 0);
            }}
          >
            <span className="card-head">
              <span className="avatar bot">G</span>
              <span className="comment-author">{COMMENT_AUTHOR}</span>
            </span>
            <span className="card-text">{boundary(clip, script.words, script.corrections)}</span>
          </button>
        )),
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

  return (
    <section className="document" aria-label="Script document">
      <header className="doc-toolbar">
        <ZoomTools zoom={zoom} scale={scale} page={page} pages={pages} onZoom={setZoom} />
        <div className="doc-actions">
          <span className="doc-stats">
            {cues.length} cut-ins · {words.toLocaleString()} words
          </span>
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
            {assets.length > 0 && (
              <>
                <h2>Assets:</h2>
                <ul>
                  {assets.map((asset) => (
                    <li key={asset.label}>
                      <a href={asset.url} target="_blank" rel="noreferrer">{asset.label}</a>
                    </li>
                  ))}
                </ul>
              </>
            )}
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
                <p className="ranges" data-block>
                  {block.clips.map(({ clip, index: at }, position) => (
                    <Fragment key={clip.id}>
                      {position ? <span className="plus">+</span> : null}
                      <span
                        className="range"
                        data-range={clip.id}
                        data-anchor={clip.id}
                        data-now={nowId === clip.id || undefined}
                        data-state={active === clip.id ? "active" : undefined}
                        data-plain={comments ? undefined : ""}
                        onMouseEnter={() => setActive(clip.id)}
                        onMouseLeave={() => setActive(null)}
                      >
                        (
                        <button
                          type="button"
                          title={rangeUrl(script, clip) ? `Preview · ${rangeUrl(script, clip)}` : "Preview"}
                          aria-label={`Preview ${clipRange(clip, script.parts, " to ")}`}
                          onClick={() => {
                            clicked.current = true;
                            onPreview(at);
                          }}
                        >
                          {clipRange(clip, script.parts)}
                        </button>
                        )
                      </span>
                    </Fragment>
                  ))}
                </p>
              )}
              {block.cue && (
                <div className="cue" data-block data-cue={block.cue.clip.id} data-anchor={`cue-${block.cue.clip.id}`} data-now={nowId === block.cue.clip.id || undefined}>
                  <Grow
                    className="narration"
                    label={`Narration ${index + 1}`}
                    value={block.cue.clip.narration ?? ""}
                    disabled={disabled}
                    placeholder="Write the narration for this cut-in…"
                    onCommit={(text) => block.cue && onNarration(block.cue.clip.id, text)}
                  />
                  {(() => {
                    const proposal = script.proposals?.find((item) => item.cutIn === block.cue?.clip.id);
                    if (!proposal) return null;
                    return (
                      <div className="proposal" aria-label="Proposed rewrite">
                        <p className="proposal-text">
                          {wordDiff(proposal.before, proposal.after).map((piece, at) =>
                            piece.kind === "same" ? <span key={at}>{piece.text}</span> : piece.kind === "add" ? <ins key={at}>{piece.text}</ins> : <del key={at}>{piece.text}</del>,
                          )}
                        </p>
                        <div className="proposal-actions">
                          <span className="faint">Claude proposed this rewrite.</span>
                          <button type="button" className="sm primary" disabled={disabled} onClick={() => block.cue && onNarration(block.cue.clip.id, proposal.after)}>
                            Accept
                          </button>
                          <button type="button" className="sm" disabled={disabled} onClick={() => onDecline(proposal)}>
                            Decline
                          </button>
                        </div>
                      </div>
                    );
                  })()}
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
