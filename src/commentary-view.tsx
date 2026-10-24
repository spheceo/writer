import { open as pickFile } from "@tauri-apps/plugin-dialog";
import { Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ChatLog, Composer, Elapsed, Said, Working } from "./agent";
import { addSource, previewSrc, buildStatus, findClips, footageUrl, getFeedback, getFinds, getFootage, getScript, startLearning, setPacing, inTauri, listScripts, message, removeSource, saveDraft, sourceText, startBuild, startRevision, updateSource } from "./api";
import { clock } from "./clips";
import { commentaryDocument, narrationWords, timing, totalLine, WORDS_PER_MINUTE } from "./commentary";
import { seen, useSplit, useView, type View, ViewTabs } from "./editor";
import { Monitor } from "./monitor";
import { type HoverClip, useHoverPreview } from "./hover-preview";
import { A4, Grow, Paper, pageInView, useScale, useZoom, ZoomTools } from "./paper";
import { CutPlayer, clipStarts, editedPosition, type Item, Playhead, usePlayheadValue } from "./playback";
import type { Adding, BuildStatus, Cite, Draft, Feedback, Finds, Footage, LearnStep, Pacing, Paragraph, Role, Script, Source, Summary } from "./types";
import { FoundClips } from "./found-clips";
import { FeedbackPanel } from "./feedback-panel";
import { DocsDialog } from "./docs-dialog";
import { COMMENTARY_SHORTCUTS, Shortcuts } from "./shortcuts";
import { type Band, BandTip, bandProps, bandsOf, isEditNote, pieceBands, ReviewButton, ReviewDialog, useReview, ArchivedBanner } from "./review";
import { Avatar } from "./avatar";
import { CAN, useGrants } from "./can";
import { Dialog, Icon, Menu, Select, Tabs } from "./ui";

const PACINGS: { value: Pacing; label: string; hint: string }[] = [
  { value: "channel", label: "Channel pacing", hint: "The channel's own videos: a line or two of setup, then the person talks. Clips run longer than the narration." },
  { value: "documentary", label: "Documentary pacing", hint: "Narration-led, like Turkey Tom: about three quarters narration, a short clip about every 30 to 40 seconds, and footage shown under the voice." },
];

const ROLES: { value: Role; label: string; hint: string }[] = [
  { value: "brief", label: "Brief", hint: "The producer's brief: the angle and the evidence to use" },
  { value: "evidence", label: "Evidence", hint: "Reports, transcripts and pages the script quotes from" },
  { value: "style", label: "Style", hint: "A past script, for the voice and shape. Never quoted" },
];

const size = (chars: number) => (chars >= 1_000_000 ? `${(chars / 1e6).toFixed(1)}M` : chars >= 1000 ? `${Math.round(chars / 1000)}k` : `${chars}`);

/** About four characters a token: what Opus reads, for a rough sense of size. */
const tokens = (chars: number) => Math.round(chars / 4);

/** A paragraph of narration takes as long to read as its words, at the app's pace. */
const readMs = (text: string) => Math.max(1500, Math.round((text.split(/\s+/).filter(Boolean).length * 60_000) / WORDS_PER_MINUTE));

/** In the preview, narration is a card for a moment, then the footage: what's on screen is the point. */
const CARD_MS = 5000;

/** A YouTube link's video id. */
const youtubeId = (url: string | undefined) => url?.match(/(?:youtube\.com\/(?:watch\?(?:.*&)?v=|shorts\/|live\/|embed\/)|youtu\.be\/)([A-Za-z0-9_-]{11})/)?.[1];

/** A clip's footage, downloaded: the file and where in it the clip starts. */
type Played = { src: string; offsetMs: number };

type Shown = { title: string; text: string };

/** How a clip search's stages begin, to tell one from a build after a reload. */
/** The stages of learning from feedback (feedback.rs), so a reopened page knows the job. */
const LEARNING_STAGES = ["Learning from the feedback", "Making the answer key", "Writing the lesson", "Round ", "Improving the lesson", "Fixing the "];
const FINDER_STAGES = ["Finding clips", "Planning the searches", "Searching YouTube", "Choosing which videos", "Reading captions", "Finding the moments", "Looking at the frames", "Adding the videos"];

/**
 * The script as a cut to preview: the montage, then each paragraph in order,
 * its narration as a card held for as long as it takes to read and its clips
 * from their recordings. A clip whose footage is not in Writer shows as a card
 * with its words. Item IDs match the page's anchors, so the page follows it.
 */
function previewCut(draft: Draft | null, sources: Source[], srcOf: Map<string, string>, downloaded: (cite: Cite) => Played | null, coming: boolean) {
  const items: Item[] = [];
  const cards = new Map<string, Shown>();
  let clips = 0;
  let playable = 0;
  if (!draft) return { items, cards, clips, playable };
  const clip = (cite: Cite, id: string) => {
    const src = srcOf.get(cite.source);
    clips += 1;
    if (src) {
      playable += 1;
      items.push({ id, startMs: cite.startMs, endMs: cite.endMs, src });
      return;
    }
    // A YouTube clip plays from the stretch downloaded beside the script.
    const played = downloaded(cite);
    if (played) {
      playable += 1;
      items.push({ id, startMs: Math.max(0, cite.startMs - played.offsetMs), endMs: Math.max(1, cite.endMs - played.offsetMs), src: played.src });
      return;
    }
    const source = sources.find((item) => item.id === cite.source);
    const youtube = youtubeId(cite.link) ?? youtubeId(source?.origin);
    items.push({ id, kind: "cut-in", startMs: 0, endMs: Math.min(CARD_MS, Math.max(1000, cite.endMs - cite.startMs)) });
    cards.set(id, {
      title: `${cite.source} · ${cite.video || source?.title} · ${youtube && coming ? "downloading" : youtube ? "couldn't be downloaded" : "not in Writer"}`,
      text: cite.quote.trim() ? `“${cite.quote.trim()}”` : "No speech. Import the recording into Writer to see it here.",
    });
  };
  draft.open.forEach((cite, index) => clip(cite, `o${index}`));
  draft.sections.forEach((section, s) =>
    section.paragraphs.forEach((paragraph, p) =>
      paragraph.pieces.forEach((piece, i) => {
        const id = piece.kind === "clip" ? `c${s}-${p}-${i}` : `s${s}-${p}-${i}`;
        if (piece.kind === "clip") return clip(piece, id);
        if (!piece.text.trim()) return;
        items.push({ id, kind: "cut-in", startMs: 0, endMs: Math.min(CARD_MS, readMs(piece.text)) });
        cards.set(id, { title: `Narration · ${section.heading}`, text: piece.text.trim() });
      }),
    ),
  );
  return { items, cards, clips, playable };
}

/** Where each Writer transcript among the sources can be played from. */
function useSourceVideos(sources: Source[]) {
  const [srcOf, setSrcOf] = useState(new Map<string, string>());
  const key = sources.map((source) => `${source.id}:${source.origin}`).join("|");
  useEffect(() => {
    let live = true;
    const writer = sources.filter((source) => source.origin.startsWith("writer:"));
    void Promise.all(
      writer.map(async (source) => {
        const id = source.origin.slice("writer:".length);
        // From this Mac or straight from the cloud (stream.rs); a script that's gone has none.
        const script = await getScript(id).catch(() => null);
        return script ? ([source.id, previewSrc(id, script.durationMs)] as const) : null;
      }),
    ).then((found) => live && setSrcOf(new Map(found.filter((item): item is readonly [string, string] => !!item))));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return srcOf;
}

/** The script's YouTube footage, as it downloads: asked again while some is still coming. */
function useFootage(id: string, draft: Draft | null) {
  const [footage, setFootage] = useState<Footage | null>(null);
  useEffect(() => {
    let live = true;
    let timer = 0;
    const ask = () =>
      void getFootage(id)
        .then((found) => {
          if (!live) return;
          setFootage(found);
          if (found.pending > 0) timer = window.setTimeout(ask, 5000);
        })
        .catch(() => {});
    ask();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [id, draft]);
  return footage;
}

// A commentary has no split view: its clips play where they're mentioned,
// in a preview over the script (hover-preview.tsx). Footage scripts keep Split.
const VIEWS: { value: View; label: string }[] = [{ value: "script", label: "Script" }];

export function CommentaryEditor({
  script,
  onChange,
  onBack,
}: {
  script: Script;
  onChange: (script: Script) => void;
  onBack: () => void;
}) {
  const sources = script.commentary?.sources ?? [];
  const draft = script.commentary?.draft ?? null;
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState<"saving" | "saved" | null>(null);
  const [copied, setCopied] = useState(false);
  const [adding, setAdding] = useState(false);
  const [reading, setReading] = useState<Source | null>(null);
  const [started, setStarted] = useState<number | null>(null);
  const [stage, setStage] = useState("");
  const [report, setReport] = useState<{ text: string; costUsd: number } | null>(null);
  const [docsOpen, setDocsOpen] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  const review = useReview(script.id, script.revision);
  const bands = useMemo(() => bandsOf(review.notes), [review.notes]);
  const openNotes = review.notes.filter((note) => !note.parentId && note.status !== "done").length;
  const [tab, setTab] = useState<"claude" | "sources" | "clips" | "feedback">(() => (script.commentary?.draft ? "claude" : "sources"));
  // Teaching Writer from feedback needs review.lessons: without it there's no Feedback tab.
  const mayTeach = useGrants().has(CAN.lessons);
  const [finds, setFinds] = useState<Finds | null>(null);
  // What the running job is: a build, or a search for clips.
  const [finding, setFinding] = useState(false);
  const [feedback, setFeedback] = useState<Feedback | null>(null);
  const [learning, setLearning] = useState(false);
  const [note, setNote] = useState("");
  // What was asked of the build that is running, shown as the producer's message.
  const [sent, setSent] = useState<string | null>(null);
  const [savedView, setView] = useView("commentary", "script");
  const view = "script" as View;
  void savedView;
  const [agentShown, setAgentShown] = useState(() => window.localStorage.getItem("writer.agent") !== "closed");
  useEffect(() => window.localStorage.setItem("writer.agent", agentShown ? "open" : "closed"), [agentShown]);
  const split = useSplit();
  const latest = useRef(script);
  latest.current = script;

  // The preview: the script as a cut, played from the recordings Writer has.
  const srcOf = useSourceVideos(sources);
  const footage = useFootage(script.id, draft);
  const downloaded = useCallback(
    (cite: Cite): Played | null => {
      const video = youtubeId(cite.link) ?? youtubeId(sources.find((source) => source.id === cite.source)?.origin);
      const piece = video && footage?.pieces.find((item) => item.video === video && item.fromMs <= cite.startMs && item.toMs >= Math.min(cite.endMs, item.fromMs + 180_000));
      return piece && footage ? { src: footageUrl(script.id, footage, piece.file), offsetMs: piece.fromMs } : null;
    },
    [footage, sources, script.id],
  );
  const cut = useMemo(() => previewCut(draft, sources, srcOf, downloaded, (footage?.pending ?? 0) > 0), [draft, sources, srcOf, downloaded, footage]);
  // A clip mentioned in the script, previewed where it's mentioned.
  const hover = useHoverPreview();
  const hoverClip = (id: string, cite: Cite): HoverClip => {
    const item = cut.items.find((found) => found.id === id && !found.kind);
    const source = sources.find((found) => found.id === cite.source);
    const title = cite.video || source?.title || cite.source;
    if (item?.src) return { id, title, quote: cite.quote.trim(), src: item.src, startMs: item.startMs, endMs: item.endMs, atMs: cite.startMs, toMs: cite.endMs };
    const youtube = youtubeId(cite.link) ?? youtubeId(source?.origin);
    const waiting = youtube ? ((footage?.pending ?? 0) > 0 ? "Downloading this stretch…" : "This stretch couldn't be downloaded.") : "This recording isn't in Writer.";
    return { id, title, quote: cite.quote.trim(), startMs: 0, endMs: 0, atMs: cite.startMs, toMs: cite.endMs, waiting };
  };
  const [{ player, playhead }] = useState(() => {
    const playhead = new Playhead();
    return { playhead, player: new CutPlayer(null, cut.items, playhead) };
  });
  useEffect(() => () => player.dispose(), [player]);
  useEffect(() => player.update(cut.items), [player, cut.items]);
  const onVideo = useCallback((video: HTMLVideoElement | null) => player.attach(video), [player]);
  const preview = (id: string) => {
    const index = cut.items.findIndex((item) => item.id === id);
    if (index < 0) return;
    const at = clipStarts(cut.items)[index];
    if (view !== "split") setView("split");
    void player.play(at);
  };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target && (target.tagName === "TEXTAREA" || target.tagName === "INPUT" || target.tagName === "SELECT")) return;
      if (event.code === "Space" && view === "split") {
        event.preventDefault();
        player.toggle();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [player, view]);

  // A build runs on the Mac; the page asks after it every few seconds.
  const alive = useRef(true);
  const watching = useRef(false);
  const follow = (since: number) => {
    setStarted(since);
    if (watching.current) return;
    watching.current = true;
    const poll = async () => {
      if (!alive.current) {
        watching.current = false;
        return;
      }
      let job: BuildStatus | null;
      try {
        job = await buildStatus(script.id);
      } catch {
        window.setTimeout(poll, 5000);
        return;
      }
      if (job?.running) {
        setStage(job.stage);
        window.setTimeout(poll, 3000);
        return;
      }
      watching.current = false;
      setStarted(null);
      setSent(null);
      setFinding(false);
      setLearning(false);
      void getFinds(script.id).then(setFinds, () => {});
      void getFeedback(script.id).then(setFeedback, () => {});
      if (!job) return;
      seen(script.id, job.finishedAt);
      if (job.error) {
        setError(job.error);
        return;
      }
      try {
        onChange(await getScript(script.id));
      } catch (cause) {
        setError(message(cause));
      }
      setReport({ text: job.report ?? "", costUsd: job.costUsd ?? 0 });
    };
    window.setTimeout(poll, 3000);
  };

  useEffect(() => {
    void getFinds(script.id).then(setFinds, () => {});
    void getFeedback(script.id).then(setFeedback, () => {});
  }, [script.id]);

  useEffect(() => {
    alive.current = true;
    void buildStatus(script.id)
      .then((job) => {
        if (job?.running) {
          setFinding(FINDER_STAGES.some((start) => job.stage.startsWith(start)));
          setLearning(LEARNING_STAGES.some((start) => job.stage.startsWith(start)));
          follow(job.startedAt);
        }
        // A build that ended while no page watched it (or that Writer closed
        // on) says how it went, once.
        else if (job?.finishedAt && job.finishedAt > seen(script.id)) {
          seen(script.id, job.finishedAt);
          if (job.error) setError(job.error);
          else setReport({ text: job.report ?? "", costUsd: job.costUsd ?? 0 });
        }
      })
      .catch(() => {});
    return () => {
      alive.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [script.id]);

  // A fix Writer starts by itself (review notes) is followed as a build is.
  useEffect(() => {
    const timer = window.setInterval(() => {
      if (watching.current) return;
      void buildStatus(script.id).then((job) => job?.running && follow(job.startedAt), () => {});
    }, 10_000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [script.id]);

  const build = async (text: string) => {
    setReport(null);
    setError(null);
    setStage("Starting");
    try {
      const job = await startBuild(script.id, text);
      setSent(text.trim() || (draft ? "Rewrite the script." : "Write the script."));
      follow(job.startedAt);
      return true;
    } catch (cause) {
      setError(message(cause));
      return false;
    }
  };

  const find = async () => {
    setReport(null);
    setError(null);
    setStage("Finding clips");
    try {
      const job = await findClips(script.id);
      setFinding(true);
      follow(job.startedAt);
    } catch (cause) {
      setError(message(cause));
    }
  };

  const learn = async (step: LearnStep) => {
    setReport(null);
    setError(null);
    setStage("Learning from the feedback");
    try {
      const job = await startLearning(script.id, step);
      setLearning(true);
      follow(job.startedAt);
    } catch (cause) {
      setError(message(cause));
    }
  };

  const persist = async (next: Draft) => {
    setSaving("saving");
    try {
      onChange(await saveDraft(script.id, latest.current.revision, next));
      setSaving("saved");
      window.setTimeout(() => setSaving((now) => (now === "saved" ? null : now)), 1200);
    } catch (cause) {
      setSaving(null);
      setError(message(cause));
      // Changed elsewhere: take it as it is on disk, so the next edit saves.
      await getScript(script.id).then(onChange, () => {});
    }
  };

  const change = async (work: Promise<Script>) => {
    setError(null);
    try {
      onChange(await work);
    } catch (cause) {
      setError(message(cause));
    }
  };

  const copy = async () => {
    if (!draft) return;
    const text = commentaryDocument(script.title, draft, sources);
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Plain http on the tailnet has no Clipboard API; the old command works from a click.
      const area = window.document.createElement("textarea");
      area.value = text;
      area.readOnly = true;
      area.style.cssText = "position:fixed;top:0;left:0;opacity:0;pointer-events:none";
      window.document.body.append(area);
      area.select();
      const done = window.document.execCommand("copy");
      area.remove();
      if (!done) {
        setError("This browser would not copy the script. Open Writer on the Mac, or select the text by hand.");
        return;
      }
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  // Claude (or a restore) is at work; an archived script is read-only as well.
  const working = started !== null;
  const archived = !!script.archive;
  const busy = working || archived;
  const ready = sources.some((source) => source.role !== "style");
  const total = sources.reduce((sum, source) => sum + source.chars, 0);
  const times = draft ? timing(draft) : null;
  const said = (report && !working ? report.text : draft?.report) ?? "";

  const editParagraph = (section: number, index: number, next: Paragraph | null) => {
    if (!draft) return;
    const sections = draft.sections.map((item, at) =>
      at !== section
        ? item
        : { ...item, paragraphs: next ? item.paragraphs.map((p, i) => (i === index ? next : p)) : item.paragraphs.filter((_, i) => i !== index) },
    );
    void persist({ ...draft, sections });
  };

  // With a script written, a message changes the paragraphs it is about; a whole rewrite is asked for.
  const [rewriting, setRewriting] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const revise = async (text: string) => {
    setReport(null);
    setError(null);
    setStage("Editing the script");
    try {
      const job = await startRevision(script.id, text, { selected: [], picked: null, playheadMs: null });
      setSent(text.trim());
      follow(job.startedAt);
      return true;
    } catch (cause) {
      setError(message(cause));
      return false;
    }
  };
  const send = () => {
    if (busy || !ready || (draft && !note.trim())) return;
    void (draft && !rewriting ? revise(note) : build(note)).then((done) => {
      if (!done) return;
      setNote("");
      setRewriting(false);
    });
  };

  return (
    <div className="studio">
      <header className="topbar" data-tauri-drag-region>
        <button type="button" className="icon-tool" aria-label="All scripts" title="All scripts" onClick={onBack}>
          <Icon name="back" />
        </button>
        <div className="title-block" data-tauri-drag-region>
          <span>Scripts</span>
          <span>/</span>
          <h1>{script.title}</h1>
          <Avatar creator={script.createdBy} size={24} />
          {saving && <span className={saving === "saved" ? "save-state ok" : "save-state"}>{saving === "saved" ? "Saved" : "Saving…"}</span>}
        </div>
        <div className="top-actions">
          {working && (
            <span className="agent-working">
              <span className="dot live" />
              {finding ? "Finding clips" : learning ? "Learning from feedback" : "Claude writing"} · <Elapsed since={started} />
            </span>
          )}
          {VIEWS.length > 1 && <ViewTabs view={view} views={VIEWS} onChange={setView} />}
          <button
            type="button"
            className="icon-tool"
            aria-pressed={agentShown}
            aria-label={agentShown ? "Hide Claude" : "Show Claude"}
            title={agentShown ? "Hide Claude and the sources" : "Show Claude and the sources"}
            onClick={() => setAgentShown(!agentShown)}
          >
            <Icon name="panel" />
          </button>
          <ReviewButton script={script} open={openNotes} approved={review.approved} onClick={() => setReviewOpen(true)} />
          <button type="button" className="secondary sm" disabled={!draft} title="Export to Google Docs, see earlier exports and read comments" onClick={() => setDocsOpen(true)}>
            <Icon name="doc" size={14} />
            Google Doc
          </button>
          <Menu
            label="More"
            items={[
              { label: copied ? "Copied" : "Copy script text", icon: "copy", disabled: !draft, onSelect: () => void copy() },
              { label: "Keyboard shortcuts", icon: "keyboard", hint: "?", onSelect: () => setShortcutsOpen(true) },
            ]}
          />
          <Shortcuts groups={COMMENTARY_SHORTCUTS} open={shortcutsOpen} onOpenChange={setShortcutsOpen} />
        </div>
      </header>
      <ArchivedBanner script={script} working={working} onJob={(job) => follow(job.startedAt)} onProblem={setError} />
      {hover.preview}
      {error && (
        <div className="notice">
          {error}
          <button type="button" className="ghost sm" onClick={() => setError(null)}>Dismiss</button>
        </div>
      )}
      <div className="editor-grid" data-view={view} data-agent={agentShown || undefined} data-resizing={split.resizing || undefined} style={split.style}>
        <CommentaryDoc
          key={view}
          draft={draft}
          items={cut.items}
          playhead={playhead}
          follow={view === "split"}
          zoomKey={`commentary-${view}`}
          disabled={saving === "saving" || busy}
          onParagraph={editParagraph}
          onPreview={preview}
          onHover={(id, cite, element) => (element ? hover.enter(hoverClip(id, cite), element) : hover.leave())}
          onPin={(id, cite, element) => hover.pin(hoverClip(id, cite), element)}
          bands={bands}
          stats={draft && times ? `${clock(times.totalMs)} · ${narrationWords(draft).toLocaleString()} words · ${clock(times.clipsMs)} of clips` : ""}
          statsTitle={draft ? commentaryTotal(draft) : ""}
        />
        {view === "split" && (
          <>
            {split.gutter("video")}
            {agentShown && split.gutter("agent")}
          </>
        )}
        <Monitor
          src=""
          multi
          clips={cut.items}
          labels={[]}
          playhead={playhead}
          player={player}
          seekBar
          onVideo={onVideo}
          describe={(clip) => cut.cards.get(clip.id) ?? null}
        />

        <section className="agent" aria-label={tab === "claude" ? "Claude" : tab === "clips" ? "Clips" : tab === "feedback" ? "Feedback" : "Sources"}>
          <div className="agent-head">
            <div className="panel-tabs" role="tablist">
              <button type="button" role="tab" aria-selected={tab === "claude"} onClick={() => setTab("claude")}>
                Claude
              </button>
              <button type="button" role="tab" aria-selected={tab === "sources"} onClick={() => setTab("sources")}>
                Sources <small>{sources.length}</small>
              </button>
              <button type="button" role="tab" aria-selected={tab === "clips"} onClick={() => setTab("clips")}>
                Clips {finds && <small>{finds.clips.length}</small>}
              </button>
              {mayTeach && (
                <button type="button" role="tab" aria-selected={tab === "feedback"} onClick={() => setTab("feedback")}>
                  Feedback {feedback && <small>{feedback.items.length}</small>}
                </button>
              )}
            </div>
            {tab === "sources" && (
              <button type="button" className="tool" disabled={busy} onClick={() => setAdding(true)}>
                <Icon name="plus" size={13} />
                Add
              </button>
            )}
          </div>
          {tab === "feedback" && mayTeach ? (
            <>
              <FeedbackPanel
                status={
                  working && learning ? (
                    <Working stage={stage} since={started} hint="A round writes the whole script and grades it: about 15 minutes, more for the first round, which also makes the answer key and the lesson. You can close this window; it keeps going." />
                  ) : report && !working ? (
                    <Said text={report.text} costUsd={report.costUsd} />
                  ) : null
                }
                id={script.id}
                feedback={feedback}
                busy={busy}
                canPull={!!draft}
                onFeedback={setFeedback}
                onStep={(step) => void learn(step)}
                onError={setError}
              />
            </>
          ) : tab === "clips" ? (
            <>
              {working && finding && <Working stage={stage} since={started} hint="Searching, reading captions and checking frames takes 10 to 20 minutes. You can close this window; it keeps going." />}
              <FoundClips id={script.id} finds={finds} busy={busy} canFind={sources.some((source) => source.role === "brief")} onFind={() => void find()} />
            </>
          ) : tab === "claude" ? (
            <>
              <ChatLog trigger={`${said}${sent}${busy}${stage}`}>
                {!draft && !busy ? (
                  <div className="agent-empty">
                    <h2>Write the script</h2>
                    <p>
                      Claude Opus reads every source: the brief for the angle, the evidence to quote from, and past scripts for the
                      voice. It writes the whole script, and Writer checks every quote and link against the sources.
                    </p>
                    {!ready && (
                      <p>
                        <button type="button" className="link-button" onClick={() => setTab("sources")}>Add the brief or some evidence</button> first.
                      </p>
                    )}
                  </div>
                ) : (
                  <>
                    {said && !(working && !draft) && <Said text={said} costUsd={report && !working ? report.costUsd : 0} />}
                    {draft && !busy && cut.items.length > 0 && (
                      <div className="turn">
                        <p className="muted">
                          Split previews the script: {cut.playable} of {cut.clips} clips play from recordings in Writer, the rest show
                          their words, and narration is held for as long as it takes to read. Click a timestamp to play from it.
                        </p>
                      </div>
                    )}
                    {draft && draft.checks.length > 0 && !busy && (
                      <div className="turn">
                        <p><strong>Checks before recording</strong></p>
                        <ul>
                          {draft.checks.map((check, index) => (
                            <li key={index}>{check}</li>
                          ))}
                        </ul>
                      </div>
                    )}
                    {sent && working && (
                      <div className="user">
                        <div className="bubble">{sent}</div>
                      </div>
                    )}
                    {working && <Working stage={stage} since={started} hint="A full script from long sources takes 10 to 20 minutes. You can close this window; it keeps going." />}
                  </>
                )}
              </ChatLog>
              <div className="pacing-pick">
                <Select
                  size="sm"
                  value={script.commentary?.pacing ?? "channel"}
                  disabled={busy}
                  label="Pacing for the next build"
                  options={PACINGS.map((item) => ({ value: item.value, label: item.label }))}
                  onChange={(pacing) => void change(setPacing(script.id, pacing))}
                />
              </div>
              <Composer
                value={note}
                disabled={busy}
                canSend={!busy && ready && (!draft || !!note.trim())}
                label={draft ? (rewriting ? "Rewrite the script" : "Change the script") : "Write the script"}
                placeholder={
                  archived
                    ? "The footage is archived: restore it to change the script"
                    : busy
                    ? "Claude is writing…"
                    : !draft
                      ? "Notes for the first draft (optional), e.g. aim for 15 minutes…"
                      : rewriting
                        ? "Ask for a rewrite. It replaces the whole script…"
                        : "Ask for a change. Claude edits only the paragraphs it's about, or adds an edit: note for the editor…"
                }
                hint={!ready ? "Add a brief or evidence first" : undefined}
                onChange={setNote}
                onSend={send}
              />
              {draft && (
                <div className="agent-actions" style={{ padding: "0 14px 8px" }}>
                  <label className="toggle" title="Off: Claude changes only what the message is about. On: it writes the whole script again from the sources.">
                    <input type="checkbox" checked={rewriting} disabled={busy} onChange={(event) => setRewriting(event.target.checked)} />
                    Rewrite the whole script
                  </label>
                </div>
              )}
            </>
          ) : (
            <div className="sources">
              <p className="sources-sum faint">About {size(tokens(total))} tokens in all</p>
              {sources.length === 0 && (
                <div className="empty">
                  <p>No sources yet.</p>
                  <p className="hint">Add the producer's brief, the evidence, and a past script or two for the style.</p>
                </div>
              )}
              {sources.map((source) => (
                <div className="source-row" key={source.id}>
                  <span className="badge mono">{source.id}</span>
                  <div className="source-body">
                    <button type="button" className="open" title={`${source.title}\nRead it`} onClick={() => setReading(source)}>
                      {source.title}
                    </button>
                    <div className="source-meta">
                      <span className={`role-chip ${source.role}`}>
                        <Select
                          size="sm"
                          value={source.role}
                          disabled={busy}
                          label={`Use ${source.id} as: ${ROLES.find((role) => role.value === source.role)?.hint ?? ""}`}
                          options={ROLES.map((role) => ({ value: role.value, label: role.label }))}
                          onChange={(role) => void change(updateSource(script.id, source.id, { role }))}
                        />
                      </span>
                      <span className="meta">
                        {size(source.chars)} characters{source.timed ? " · timed" : ""}
                        {srcOf.has(source.id) ? " · plays" : ""} · {origin(source)}
                      </span>
                    </div>
                  </div>
                  <Menu
                    label={`More for ${source.title}`}
                    items={[
                      { label: "Read", onSelect: () => setReading(source) },
                      ...(source.origin.startsWith("http") ? [{ label: "Open link", onSelect: () => window.open(source.origin, "_blank") }] : []),
                      { label: "Remove", danger: true, disabled: busy, onSelect: () => void change(removeSource(script.id, source.id)) },
                    ]}
                  />
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
      <AddSourceDialog
        open={adding}
        script={script}
        onClose={() => setAdding(false)}
        onAdded={(next) => {
          onChange(next);
          setAdding(false);
        }}
      />
      <ReadDialog id={script.id} source={reading} onClose={() => setReading(null)} />
      <ReviewDialog
        open={reviewOpen}
        script={script}
        notes={review.notes}
        approved={review.approved}
        busy={busy}
        onClose={() => setReviewOpen(false)}
        onChange={onChange}
        onJob={(job) => {
          setReviewOpen(false);
          follow(job.startedAt);
        }}
        onRefresh={review.refresh}
      />
      <BandTip notes={review.notes} />
      <DocsDialog
        open={docsOpen}
        script={script}
        onClose={() => setDocsOpen(false)}
        onChange={onChange}
        onUseComments={(text) => {
          setNote(`Apply these comments from the producer on the exported Doc, and keep everything else as it is:\n${text}`);
          setTab("claude");
          setAgentShown(true);
        }}
      />
    </div>
  );
}

const commentaryTotal = (draft: Draft) => {
  const { narrationMs, clipsMs, totalMs } = timing(draft);
  return `About ${clock(narrationMs)} of narration + ${clock(clipsMs)} of clips = ${clock(totalMs)}`;
};

function origin(source: Source) {
  if (source.origin === "pasted") return "pasted";
  if (source.origin.startsWith("writer:")) return "from Writer";
  if (source.origin.startsWith("http")) {
    try {
      return new URL(source.origin).host.replace(/^www\./, "");
    } catch {
      return "link";
    }
  }
  return source.origin.split("/").pop() ?? source.origin;
}

/** A note beside the page, as Docs shows a comment. */
type Card =
  | { id: string; kind: "clip"; cite: Cite }
  | { id: string; kind: "note"; section: number; index: number; paragraph: Paragraph };

/**
 * The script as the exported Doc: A4 sheets, Arial, the bold section headings,
 * clips as linked timestamps, and every clip's words, what to verify and the
 * editor's notes as comments beside the page. In Split it follows the
 * preview: what plays is underlined in red, and a timestamp clicked plays.
 */
function CommentaryDoc({
  draft,
  items,
  playhead,
  follow,
  zoomKey,
  disabled,
  stats,
  statsTitle,
  onParagraph,
  onPreview,
  onHover,
  onPin,
  bands,
}: {
  draft: Draft | null;
  items: Item[];
  playhead: Playhead;
  follow: boolean;
  zoomKey: string;
  disabled: boolean;
  stats: string;
  statsTitle: string;
  onParagraph: (section: number, index: number, next: Paragraph | null) => void;
  onPreview: (id: string) => void;
  /** A clip's time hovered (an element) or left (null), and clicked to keep its preview open. */
  onHover?: (id: string, cite: Cite, element: Element | null) => void;
  onPin?: (id: string, cite: Cite, element: Element) => void;
  /** Review bands by block (a paragraph's ID, `<id>:heading`, `open`). */
  bands?: Map<string, Band[]>;
}) {
  const canvas = useRef<HTMLDivElement>(null);
  const [zoom, setZoom] = useZoom(zoomKey);
  // Comments are kept per view: on beside the Script view's page, off in Split, where the page is narrow.
  const [comments, setComments] = useState(() => {
    const saved = window.localStorage.getItem(`writer.comments.${zoomKey}`);
    return saved ? saved === "on" : !zoomKey.endsWith("split");
  });
  useEffect(() => window.localStorage.setItem(`writer.comments.${zoomKey}`, comments ? "on" : "off"), [zoomKey, comments]);
  const scale = useScale(canvas, zoom, A4, comments);
  const [page, setPage] = useState(1);
  const [pages, setPages] = useState(1);
  const [active, setActive] = useState<string | null>(null);
  const nowId = usePlayheadValue(playhead, (p) => (follow ? (editedPosition(items, p.ms)?.clip.id ?? null) : null));

  // The playing timestamp's red line fills as its clip runs out.
  useLayoutEffect(() => {
    if (!nowId) return;
    const node = canvas.current?.querySelector<HTMLElement>(`[data-anchor="${nowId}"]`);
    if (!node) return;
    const paint = () => {
      const at = editedPosition(items, playhead.ms);
      if (at?.clip.id === nowId) node.style.setProperty("--progress", String((at.sourceMs - at.clip.startMs) / Math.max(1, at.clip.endMs - at.clip.startMs)));
    };
    paint();
    // Kept a third of the way down while it plays.
    const scroller = canvas.current;
    if (scroller && playhead.playing) {
      const top = node.getBoundingClientRect().bottom - scroller.getBoundingClientRect().top + scroller.scrollTop - scroller.clientHeight / 3;
      scroller.scrollTo({ top: Math.max(0, top), behavior: "smooth" });
    }
    return playhead.subscribe(paint);
  }, [nowId, items, playhead]);

  const cards: Card[] = [];
  if (draft) {
    draft.open.forEach((cite, index) => cards.push({ id: `o${index}`, kind: "clip", cite }));
    draft.sections.forEach((section, s) =>
      section.paragraphs.forEach((paragraph, p) => {
        if (paragraph.note !== undefined) cards.push({ id: `n${s}-${p}`, kind: "note", section: s, index: p, paragraph });
        paragraph.pieces.forEach((piece, i) => piece.kind === "clip" && cards.push({ id: `c${s}-${p}-${i}`, kind: "clip", cite: piece }));
      }),
    );
  }

  const range = (cite: Cite, id: string, band?: Band[]) => {
    const times = `${clock(cite.startMs)}-${clock(cite.endMs)}`;
    const title = cite.verify ? `Verify: ${cite.verify}` : "";
    return (
      <span
        className="range"
        key={id}
        data-anchor={id}
        data-now={nowId === id || undefined}
        data-state={active === id ? "active" : undefined}
        data-plain={comments ? undefined : ""}
        title={title}
        {...bandProps(band)}
        onMouseEnter={(event) => {
          setActive(id);
          onHover?.(id, cite, event.currentTarget);
        }}
        onMouseLeave={() => {
          setActive(null);
          onHover?.(id, cite, null);
        }}
      >
        (
        <button type="button" onClick={(event) => (onPin ? onPin(id, cite, event.currentTarget) : onPreview(id))}>
          {times}
        </button>
        )
      </span>
    );
  };
  const ranges = (cites: { cite: Cite; id: string; band?: Band[] }[], key: string) => (
    <p className="ranges" key={key}>
      {cites.map(({ cite, id, band }, index) => (
        <Fragment key={id}>
          {index > 0 && <span className="plus">+</span>}
          {range(cite, id, band)}
        </Fragment>
      ))}
    </p>
  );

  const rail = comments
    ? cards.map((card) =>
        card.kind === "clip" ? (
          <button
            type="button"
            className="comment"
            key={card.id}
            data-card={card.id}
            data-state={active === card.id ? "active" : undefined}
            onMouseEnter={(event) => {
              setActive(card.id);
              onHover?.(card.id, card.cite, event.currentTarget);
            }}
            onMouseLeave={() => {
              setActive(null);
              onHover?.(card.id, card.cite, null);
            }}
            onClick={(event) => (onPin ? onPin(card.id, card.cite, event.currentTarget) : onPreview(card.id))}
          >
            <span className="card-head">
              <span className="avatar mono">{card.cite.source.replace(/^S/, "")}</span>
              <span className="comment-author">{card.cite.video}</span>
            </span>
            <span className="card-text">{card.cite.quote.trim() ? `“${card.cite.quote.trim()}”` : "Confirm speech and context by ear."}</span>
            {card.cite.verify && <span className="card-verify">Verify: {card.cite.verify}</span>}
          </button>
        ) : (
          <div
            className="comment human"
            key={card.id}
            data-card={card.id}
            data-edit={isEditNote(card.paragraph.note) || undefined}
            data-state={active === card.id ? "active" : undefined}
            onMouseEnter={() => setActive(card.id)}
            onMouseLeave={() => setActive(null)}
          >
            <span className="card-head">
              <span className="avatar"><Icon name="note" size={12} /></span>
              <span className="comment-author">{isEditNote(card.paragraph.note) ? "For editor · edit: lines go to Edit's agent" : "For editor"}</span>
            </span>
            <Grow
              className="card-field"
              value={card.paragraph.note ?? ""}
              disabled={disabled}
              placeholder="What to show, blur or bleep"
              onCommit={(text) => onParagraph(card.section, card.index, { ...card.paragraph, note: text.trim() ? text : undefined })}
            />
          </div>
        ),
      )
    : null;

  return (
    <section className="document" aria-label="Script document">
      <header className="doc-toolbar">
        <ZoomTools zoom={zoom} scale={scale} page={page} pages={pages} onZoom={setZoom} />
        <div className="doc-actions">
          {stats && <span className="doc-stats" title={statsTitle}>{stats}</span>}
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
      <div className="canvas" ref={canvas} onScroll={(event) => setPage(pageInView(event.currentTarget, scale, A4))}>
        <Paper sheet={A4} scale={scale} className="a4" rail={rail} version={comments} onPages={setPages}>
          {!draft && (
            <p className="doc-empty" data-block>
              The script appears here once Claude writes it, laid out as its Google Doc will be.
            </p>
          )}
          {draft?.sections.map((section, s) => (
            <Fragment key={`${s}-${section.heading}`}>
              <h2 className="doc-heading" data-block {...bandProps(section.paragraphs[0]?.id ? bands?.get(`${section.paragraphs[0].id}:heading`) : undefined)}>
                {section.heading.replace(/^(\d+\.) /, "$1 ")}
              </h2>
              {s === 0 && draft.open.length > 0 && (
                <div className="block" data-block {...bandProps(bands?.get("open"))}>
                  {ranges(draft.open.map((cite, index) => ({ cite, id: `o${index}` })), "open")}
                </div>
              )}
              {section.paragraphs.map((paragraph, p) => {
                // Clips that follow one another share a line, joined with "+".
                const lines: ({ kind: "say"; text: string; index: number } | { kind: "clips"; cites: { cite: Cite; id: string; band?: Band[] }[] })[] = [];
                const banded = pieceBands(paragraph, paragraph.id ? bands?.get(paragraph.id) : undefined);
                paragraph.pieces.forEach((piece, i) => {
                  if (piece.kind === "say") return lines.push({ kind: "say", text: piece.text, index: i });
                  const last = lines[lines.length - 1];
                  const item = { cite: piece, id: `c${s}-${p}-${i}`, band: banded.get(i) };
                  if (last?.kind === "clips") last.cites.push(item);
                  else lines.push({ kind: "clips", cites: [item] });
                });
                const noteId = `n${s}-${p}`;
                const setSay = (index: number, text: string) =>
                  onParagraph(s, p, {
                    ...paragraph,
                    pieces: paragraph.pieces.map((piece, at) => (at === index && piece.kind === "say" ? { ...piece, text } : piece)),
                  });
                return (
                  <div className="block" data-block data-anchor={noteId} data-state={active === noteId ? "active" : undefined} key={`${s}-${p}-${draft.writtenAt}`}>
                    {lines.map((line, index) =>
                      line.kind === "clips" ? (
                        ranges(line.cites, `r${index}`)
                      ) : (
                        <div className="said" key={`s${line.index}`} data-anchor={`s${s}-${p}-${line.index}`} data-now={nowId === `s${s}-${p}-${line.index}` || undefined}>
                          <Grow className="narration" value={line.text} disabled={disabled} bands={banded.get(line.index)} onCommit={(text) => setSay(line.index, text)} />
                        </div>
                      ),
                    )}
                    <div className="block-tools">
                      {paragraph.note === undefined && (
                        <button type="button" disabled={disabled} title="Add a note for the editor" onClick={() => onParagraph(s, p, { ...paragraph, note: "" })}>
                          <Icon name="note" size={14} />
                          Note
                        </button>
                      )}
                      <button type="button" className="danger-text" disabled={disabled} title="Remove this paragraph" onClick={() => onParagraph(s, p, null)}>
                        Remove
                      </button>
                    </div>
                  </div>
                );
              })}
            </Fragment>
          ))}
          {draft && <p className="doc-total" data-block>{totalLine(draft)}</p>}
        </Paper>
      </div>
    </section>
  );
}

type Kind = "link" | "file" | "text" | "writer";

function AddSourceDialog({
  open,
  script,
  onClose,
  onAdded,
}: {
  open: boolean;
  script: Script;
  onClose: () => void;
  onAdded: (script: Script) => void;
}) {
  const [kind, setKind] = useState<Kind>("link");
  const [value, setValue] = useState("");
  const [title, setTitle] = useState("");
  const [role, setRole] = useState<Role>("evidence");
  const [others, setOthers] = useState<Summary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setBusy(false);
    setValue("");
    setTitle("");
    const hasBrief = script.commentary?.sources.some((source) => source.role === "brief");
    setRole(hasBrief ? "evidence" : "brief");
    void listScripts()
      .then((rows) => setOthers(rows.filter((row) => row.sourceKind !== "commentary" && row.wordCount > 0)))
      .catch(() => setOthers([]));
  }, [open, script.commentary?.sources]);

  const choose = async () => {
    const picked = await pickFile({ multiple: false, filters: [{ name: "Text", extensions: ["txt", "md", "markdown", "srt", "vtt", "csv", "json"] }] });
    if (typeof picked === "string") setValue(picked);
  };

  const submit = async () => {
    const input = value.trim();
    if (!input) return;
    const adding: Adding =
      kind === "link"
        ? { kind: "link", url: input }
        : kind === "file"
          ? { kind: "file", path: input }
          : kind === "text"
            ? { kind: "text", title, text: value }
            : { kind: "writer", id: input };
    setBusy(true);
    setError(null);
    try {
      onAdded(await addSource(script.id, adding, role));
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Add a source"
      description="Claude reads every source in full when it writes the script."
      width={520}
      footer={
        <>
          {error && <span className="bad dialog-error">{error}</span>}
          <span className="spacer" />
          <button type="button" className="ghost" onClick={onClose}>Cancel</button>
          <button type="submit" form="add-source" className="primary" disabled={busy || !value.trim()}>
            {busy ? "Reading…" : "Add"}
          </button>
        </>
      }
    >
      <form
        id="add-source"
        className="importer"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <Tabs
          value={kind}
          onChange={(next) => {
            setKind(next);
            setValue("");
            setError(null);
          }}
          options={[
            { value: "link", label: <><Icon name="drive" size={14} />Link</> },
            { value: "file", label: <><Icon name="file" size={14} />This Mac</> },
            { value: "text", label: "Paste" },
            { value: "writer", label: <><Icon name="stack" size={14} />A transcript</> },
          ]}
        />
        <div className="source" key={kind}>
          {kind === "link" && (
            <>
              <input
                data-autofocus
                value={value}
                placeholder="https://docs.google.com/document/d/…"
                spellCheck={false}
                onChange={(event) => setValue(event.target.value)}
              />
              <p className="hint">A Google Doc, a Drive text file, or a web page. Shared with "Anyone with the link" or with Repack's Drive account.</p>
            </>
          )}
          {kind === "file" &&
            (inTauri() ? (
              <button type="button" className="drop" onClick={() => void choose()}>
                {value ? (
                  <>
                    <strong>{value.split("/").pop()}</strong>
                    <span>{value}</span>
                  </>
                ) : (
                  <>
                    <strong>Choose a text file…</strong>
                    <span>TXT, MD, SRT, VTT, CSV or JSON</span>
                  </>
                )}
              </button>
            ) : (
              <input data-autofocus value={value} placeholder="/Users/you/Downloads/report.txt" spellCheck={false} onChange={(event) => setValue(event.target.value)} />
            ))}
          {kind === "text" && (
            <>
              <input data-autofocus value={title} maxLength={180} placeholder="Title, e.g. Reddit capture u/mrbeast6000" onChange={(event) => setTitle(event.target.value)} />
              <textarea className="links" rows={8} value={value} placeholder="Paste the text" onChange={(event) => setValue(event.target.value)} />
            </>
          )}
          {kind === "writer" &&
            (others.length ? (
              <div className="checklist">
                {others.map((row) => (
                  <label key={row.id}>
                    <input type="radio" name="writer-source" checked={value === row.id} onChange={() => setValue(row.id)} />
                    <span>{row.title}</span>
                    <span className="faint">{clock(row.durationMs)}</span>
                  </label>
                ))}
              </div>
            ) : (
              <p className="hint">No transcribed footage in Writer yet. Import a video as a new script first, then add its transcript here.</p>
            ))}
          <div className="row role-pick">
            <span className="faint">Use it as</span>
            <Tabs value={role} onChange={setRole} options={ROLES.map((item) => ({ value: item.value, label: item.label }))} />
          </div>
          <p className="hint">{ROLES.find((item) => item.value === role)?.hint}.</p>
        </div>
      </form>
    </Dialog>
  );
}

function ReadDialog({ id, source, onClose }: { id: string; source: Source | null; onClose: () => void }) {
  const [text, setText] = useState<string | null>(null);
  const shown = useRef(source);
  if (source) shown.current = source;
  useEffect(() => {
    if (!source) return;
    setText(null);
    void sourceText(id, source.id)
      .then(setText)
      .catch((cause) => setText(message(cause)));
  }, [id, source]);
  const limit = 200_000;
  return (
    <Dialog open={!!source} onClose={onClose} title={`${shown.current?.id ?? ""} · ${shown.current?.title ?? ""}`} width={760}
      footer={<><span className="spacer" /><button type="button" className="ghost" data-autofocus onClick={onClose}>Close</button></>}
    >
      <pre className="source-text">
        {text === null ? "Reading…" : text.length > limit ? `${text.slice(0, limit)}\n\n… ${size(text.length - limit)} more characters` : text}
      </pre>
    </Dialog>
  );
}
