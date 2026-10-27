import { type CSSProperties, Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Asked, ChatLog, Composer, ResetAsk, Said, Working, useChatDraft } from "./agent";
import { buildStatus, clearChat, completeTranscription, correctWord, getChat, getResearch, getScript, importReuses, message, previewSrc, resetScript, resolveProposals, restoreVersion, retrySpeakers, saveClips, setHeading, startBuild, startRevision, stopJob } from "./api";
import { breathe, clock, insertAt, isCutIn, keptMs, purposes, splitClip } from "./clips";
import { Avatar } from "./avatar";
import { UpdateButton } from "./update";
import { DocsDialog } from "./docs-dialog";
import { VersionsDialog } from "./versions-dialog";
import { scriptText } from "./doc-export";
import { FootageDoc } from "./footage-doc";
import { ReusedCases } from "./reused-cases";
import { Monitor } from "./monitor";
import { CutPlayer, clipStarts, editedPosition, Playhead } from "./playback";
import { Timeline } from "./timeline";
import { type Picked, ResearchList, Transcript, useSpeakers } from "./transcript";
import type { BuildStatus, ChatTurn, Clip, Highlight, Kind, Research, Script } from "./types";
import { FOOTAGE_SHORTCUTS, Shortcuts } from "./shortcuts";
import { BandTip, bandsOf, ReviewButton, ReviewDialog, useReview, ArchivedBanner } from "./review";
import { Dialog, Icon, Menu } from "./ui";

export type View = "video" | "script" | "split";
const VIEWS: { value: View; label: string }[] = [
  { value: "video", label: "Video" },
  { value: "script", label: "Script" },
  { value: "split", label: "Split" },
];

/** A view kept per kind of script, as the page was last left. */
export function useView(key: string, fallback: View) {
  const [view, setView] = useState<View>(() => {
    const saved = window.localStorage.getItem(`writer.view.${key}`);
    return saved === "video" || saved === "script" || saved === "split" ? saved : fallback;
  });
  useEffect(() => window.localStorage.setItem(`writer.view.${key}`, view), [key, view]);
  return [view, setView] as const;
}

/** Split's widths: the picture's share against the script's, and Claude's width. */
export function useSplit() {
  const [split, setSplit] = useState<{ video: number | null; agent: number | null }>(() => {
    try {
      return { video: null, agent: null, ...JSON.parse(window.localStorage.getItem("writer.split") ?? "{}") };
    } catch {
      return { video: null, agent: null };
    }
  });
  useEffect(() => window.localStorage.setItem("writer.split", JSON.stringify(split)), [split]);
  const [resizing, setResizing] = useState<"video" | "agent" | null>(null);
  const resize = (which: "video" | "agent") => (event: React.PointerEvent<HTMLDivElement>) => {
    const editor = event.currentTarget.parentElement;
    if (!editor) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setResizing(which);
    const box = editor.getBoundingClientRect();
    const agent = editor.querySelector<HTMLElement>(".agent")?.getBoundingClientRect().width ?? 0;
    const move = (moved: PointerEvent) => {
      if (which === "agent") {
        setSplit((now) => ({ ...now, agent: Math.max(260, Math.min(620, box.right - 6 - moved.clientX)) }));
        return;
      }
      const video = moved.clientX - box.left - 6;
      const script = box.right - 6 - (agent ? agent + 6 : 0) - 6 - moved.clientX;
      setSplit((now) => ({ ...now, video: Math.max(0.25, Math.min(4, video / Math.max(1, script))) }));
    };
    const up = () => {
      setResizing(null);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };
  const style = {
    ...(split.video ? { "--split-video": `${split.video}fr` } : {}),
    ...(split.agent ? { "--split-agent": `${split.agent}px` } : {}),
  } as CSSProperties;
  const gutter = (which: "video" | "agent") => (
    // biome-ignore lint/a11y/noStaticElementInteractions: a pointer handle; the panels' widths have defaults
    <div
      className="gutter"
      data-gutter={which}
      data-active={resizing === which || undefined}
      title="Drag to resize · double-click to reset"
      onPointerDown={resize(which)}
      onDoubleClick={() => setSplit((now) => ({ ...now, [which]: null }))}
    />
  );
  return { style, resizing, gutter };
}

/** The view tabs, as Repack's top bar has them, a highlight sliding between. */
export function ViewTabs({ view, onChange, views = VIEWS }: { view: View; onChange: (view: View) => void; views?: typeof VIEWS }) {
  return (
    <div className="view-tabs" role="tablist" aria-label="Editor view" data-view={view} style={{ ["--count" as string]: views.length, ["--at" as string]: views.findIndex((item) => item.value === view) }}>
      {views.map((item) => (
        <button type="button" role="tab" key={item.value} aria-selected={view === item.value} onClick={() => onChange(item.value)}>
          {item.label}
        </button>
      ))}
    </div>
  );
}

const KINDS: { value: Kind; label: string }[] = [
  { value: "interrogation", label: "Interrogation" },
  { value: "foia", label: "Bodycam" },
  { value: "courtroom", label: "Courtroom" },
];

/** When the last build of a script whose outcome was shown here ended; or marks one shown. */
export function seen(id: string, finishedAt?: number | null) {
  const key = `writer.build-seen.${id}`;
  if (finishedAt) window.localStorage.setItem(key, String(finishedAt));
  return Number(window.localStorage.getItem(key) ?? 0);
}

/**
 * A footage script, as Repack's editor: the Video view (the monitor, the
 * transcript and research, the timeline), the Script view (its Doc), and Split
 * (the monitor beside the Doc). Claude sits at the right in all three.
 */
export function Editor({ script, onChange, onBack }: { script: Script; onChange: (script: Script) => void; onBack: () => void }) {
  // The timeline as edited here. Each edit shows at once and is saved in
  // turn, each save quoting the revision the one before it returned, as
  // Repack's editor queues them. Changes from elsewhere (a build, an export)
  // are taken once nothing is waiting to be saved.
  const [clips, setClips] = useState(script.clips);
  const shown = useRef(script.clips);
  const revision = useRef(script.revision);
  const pending = useRef(0);
  const epoch = useRef(0);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  useEffect(() => {
    if (pending.current) return;
    shown.current = script.clips;
    revision.current = script.revision;
    setClips(script.clips);
  }, [script]);
  const [view, setView] = useView("footage", "video");
  const [expanded, setExpanded] = useState(false);
  const [agentShown, setAgentShown] = useState(() => window.localStorage.getItem("writer.agent") !== "closed");
  useEffect(() => window.localStorage.setItem("writer.agent", agentShown ? "open" : "closed"), [agentShown]);
  const [tab, setTab] = useState<"transcript" | "research">("transcript");
  const [selected, setSelected] = useState<string[]>([]);
  const [picked, setPicked] = useState<Picked | null>(null);
  const [saving, setSaving] = useState<"saving" | "saved" | null>(null);
  const [error, setError] = useState<string | null>(script.error);
  const [notesShown, setNotesShown] = useState(true);
  const [docsOpen, setDocsOpen] = useState(false);
  const [versionsOpen, setVersionsOpen] = useState(false);
  const [reviewOpen, setReviewOpen] = useState(false);
  const review = useReview(script.id, script.revision);
  const bands = useMemo(() => bandsOf(review.notes), [review.notes]);
  const openNotes = review.notes.filter((note) => !note.parentId && note.status !== "done").length;
  const [copied, setCopied] = useState(false);
  const [started, setStarted] = useState<number | null>(null);
  const [stage, setStage] = useState("");
  const [chat, setChat] = useState<ChatTurn[]>([]);
  // What was just asked, shown until the conversation on disk has it.
  const [sent, setSent] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [research, setResearch] = useState<Research | null>(null);
  const [note, setNote] = useChatDraft(script.id);
  const [quote, setQuote] = useState<{ id: string; quote: string; start: number; end: number; narrationNumber: number } | null>(null);
  const [resolving, setResolving] = useState(false);
  const split = useSplit();

  // One playhead per player, made together, so a reload of the code keeps them paired.
  const [{ player, playhead }] = useState(() => {
    const playhead = new Playhead();
    return { playhead, player: new CutPlayer(null, clips, playhead) };
  });
  useEffect(() => () => player.dispose(), [player]);
  useEffect(() => player.update(clips), [player, clips]);
  const onVideo = useCallback((video: HTMLVideoElement | null) => player.attach(video), [player]);

  useEffect(() => {
    void getResearch(script.id).then(setResearch).catch(() => {});
  }, [script.id]);

  // Closing or reloading with an edit still on its way asks first.
  useEffect(() => {
    if (saving !== "saving") return;
    const hold = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", hold);
    return () => window.removeEventListener("beforeunload", hold);
  }, [saving]);

  const speakers = useSpeakers(script.words, script.speakers);
  const labels = useMemo(() => purposes(script), [script]);
  const cueCount = clips.filter(isCutIn).length;
  // The footage's length in the address: a reused case joined onto it is a new file at the same path.
  // Played from this Mac or straight from the cloud, never waiting on a download (stream.rs).
  const src = script.proxyPath || script.status === "ready" ? previewSrc(script.id, script.durationMs) : "";
  // Claude (or a restore) is at work; an archived script is read-only as well.
  const working = started !== null;
  const archived = !!script.archive;
  const busy = working || archived || resolving;
  const unfinishedSpeakers = !!(script.speakerCheck && (script.speakerCheck.failed.length || script.speakerCheck.needsApply))
    || !!script.notes?.some((text) => text.startsWith("Who says each line couldn't be read"));

  // Undo and redo: up to 30 earlier timelines, cut edits and narration alike.
  // A fresh build goes on the list too, so it can be undone.
  const past = useRef<Clip[][]>([]);
  const future = useRef<Clip[][]>([]);
  const [, setHistory] = useState(0);
  const remember = (before: Clip[]) => {
    past.current = [...past.current.slice(-29), before];
    future.current = [];
    setHistory((n) => n + 1);
  };

  /**
   * Shows an edit and queues its save. A refused save (the script changed
   * in another window or on another Mac) drops the edits queued after it and
   * takes the script as it is on disk, so the next edit saves again.
   */
  const persist = (next: Clip[], record = true) => {
    const before = shown.current;
    shown.current = next;
    setClips(next);
    if (record) remember(before);
    pending.current += 1;
    setSaving("saving");
    setError(null);
    const mine = epoch.current;
    const run = queue.current.then(async () => {
      try {
        // An edit queued behind a refused save is dropped, but still counted off below.
        if (mine !== epoch.current) return false;
        const saved = await saveClips(script.id, revision.current, next);
        revision.current = saved.revision;
        onChange(saved);
        return true;
      } catch (cause) {
        epoch.current += 1;
        future.current = [];
        setError(message(cause));
        try {
          const fresh = await getScript(script.id);
          revision.current = fresh.revision;
          shown.current = fresh.clips;
          setClips(fresh.clips);
          onChange(fresh);
        } catch {
          // The error says what went wrong; the next edit tries again.
        }
        return false;
      } finally {
        pending.current -= 1;
        if (!pending.current) {
          setSaving((state) => (state === "saving" ? "saved" : state));
          window.setTimeout(() => setSaving((state) => (state === "saved" ? null : state)), 1600);
        }
      }
    });
    queue.current = run;
    return run;
  };

  const undo = () => {
    const previous = past.current.at(-1);
    if (!previous || busy) return;
    past.current = past.current.slice(0, -1);
    future.current = [...future.current, shown.current];
    setHistory((n) => n + 1);
    void persist(previous, false);
  };

  const redo = () => {
    const next = future.current.at(-1);
    if (!next || busy) return;
    future.current = future.current.slice(0, -1);
    past.current = [...past.current.slice(-29), shown.current];
    setHistory((n) => n + 1);
    void persist(next, false);
  };

  // A timestamp clicked in the script: Split plays it there; Script opens the
  // Video view on it.
  const preview = (index: number) => {
    const at = clipStarts(clips)[index] ?? 0;
    if (view === "split") void player.play(at);
    else {
      if (view === "script") setView("video");
      player.seek(at);
    }
  };

  /** Premiere's insert edit, as Repack makes it: the words go in at the playhead with breathing room, and the playhead moves past them. */
  const insertPicked = (at = playhead.ms) => {
    if (!picked) return;
    const clip: Clip = { id: crypto.randomUUID(), ...breathe(script.words, picked.startMs, picked.endMs, script.durationMs || picked.endMs) };
    const next = insertAt(clips, at, clip);
    void persist(next);
    player.seek(clipStarts(next)[next.findIndex((item) => item.id === clip.id) + 1]);
  };
  const [spelling, setSpelling] = useState<string | null>(null);
  useEffect(() => setSpelling(null), [picked?.first.id, picked?.count]);
  const liftPicked = () => {
    if (!picked) return;
    void persist(
      clips.flatMap((clip) => {
        if (isCutIn(clip) || clip.endMs <= picked.startMs || clip.startMs >= picked.endMs) return [clip];
        const kept: Clip[] = [];
        if (clip.startMs < picked.startMs) kept.push({ ...clip, endMs: picked.startMs });
        if (clip.endMs > picked.endMs) kept.push(kept.length ? { ...clip, id: crypto.randomUUID(), startMs: picked.endMs, notes: undefined } : { ...clip, startMs: picked.endMs });
        return kept;
      }),
    );
  };

  // Repack's keys: Space plays, J and L skip, Home and End, ⌘K splits, ⌫
  // removes what is selected, F fills the frame, ⌘Z undoes.
  const keys = useRef<(event: KeyboardEvent) => void>(() => {});
  keys.current = (event) => {
    const target = event.target as HTMLElement | null;
    const typing = !!target && (target.tagName === "TEXTAREA" || target.tagName === "INPUT" || target.tagName === "SELECT");
    if ((event.metaKey || event.ctrlKey) && /^[zZy]$/.test(event.key)) {
      if (typing) return;
      event.preventDefault();
      if (event.key === "y" || event.shiftKey) redo();
      else undo();
      return;
    }
    if (typing) return;
    if ((event.metaKey || event.ctrlKey) && event.key === "k") {
      event.preventDefault();
      const position = editedPosition(clips, playhead.ms);
      if (position) {
        const next = splitClip(clips, position.index, position.sourceMs);
        if (next !== clips) void persist(next);
      }
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.code === "Space") {
      event.preventDefault();
      player.toggle();
    } else if (event.key === "j" || event.key === "J") player.seek(playhead.ms - 10000);
    else if (event.key === "l" || event.key === "L") player.seek(playhead.ms + 10000);
    else if (event.key === "Home") player.seek(0);
    else if (event.key === "End") player.seek(keptMs(clips));
    else if ((event.key === "f" || event.key === "F") && view === "video") setExpanded(!expanded);
    else if ((event.key === "Backspace" || event.key === "Delete") && selected.length && view === "video") {
      void persist(clips.filter((clip) => !selected.includes(clip.id)));
      setSelected([]);
    } else if (event.key === "Escape") setSelected([]);
  };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => keys.current(event);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const retry = async () => {
    setError(null);
    try {
      const job = await completeTranscription(script.id);
      follow(job.startedAt);
    } catch (cause) {
      setError(message(cause));
    }
  };

  // A build runs on the Mac. The page asks after it every few seconds, so a
  // dropped tailnet connection or a reload only pauses the asking.
  const latest = useRef(clips);
  latest.current = clips;
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
      void loadChat().finally(() => setSent(null));
      if (!job) return;
      seen(script.id, job.finishedAt);
      if (job.error) {
        setError(job.error);
        return;
      }
      player.pause();
      try {
        const before = latest.current;
        const fresh = await getScript(script.id);
        onChange(fresh);
        // Undo brings back what the job changed; bringing in footage changes no clip.
        if (before.length && JSON.stringify(before) !== JSON.stringify(fresh.clips)) remember(before);
      } catch (cause) {
        setError(message(cause));
      }
      void getResearch(script.id).then(setResearch).catch(() => {});
    };
    window.setTimeout(poll, 3000);
  };

  const loadChat = () =>
    getChat(script.id)
      .then(setChat)
      .catch(() => {});

  useEffect(() => {
    alive.current = true;
    void loadChat();
    void buildStatus(script.id)
      .then((job) => {
        if (job?.running) follow(job.startedAt);
        // A build that ended while no page watched it (or that Writer closed
        // on) says how it went, once: its answer is in the conversation.
        else if (job?.finishedAt && job.finishedAt > seen(script.id)) {
          seen(script.id, job.finishedAt);
          if (job.error) setError(job.error);
        }
      })
      .catch(() => {});
    return () => {
      alive.current = false;
    };
    // Only on opening a script; follow() keeps itself going.
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

  const build = async (request = note) => {
    setError(null);
    setStage("Starting");
    try {
      const job = await startBuild(script.id, request);
      setSent(request.trim() ? `Build the script. ${request.trim()}` : "Build the script.");
      setNote("");
      follow(job.startedAt);
    } catch (cause) {
      setError(message(cause));
    }
  };

  // Once the script is built, a message asks Opus for targeted edits, as
  // Repack's chat does, with what the producer has selected as "this".
  const revise = async (retry?: string) => {
    const words = (retry ?? note).trim();
    const request = quote && !retry ? `Quoted from narration ${quote.narrationNumber} (clip ${quote.id}, characters ${quote.start}–${quote.end}):\n“${quote.quote}”\n\n${words}` : words;
    if (!request) return;
    setError(null);
    setStage("Editing the script");
    try {
      const job = await startRevision(script.id, request, {
        selected: quote && !retry ? [quote.id] : selected,
        picked: picked ? [picked.startMs, picked.endMs] : null,
        playheadMs: Math.round(playhead.ms),
      });
      setSent(request);
      setNote("");
      setQuote(null);
      follow(job.startedAt);
    } catch (cause) {
      setError(message(cause));
    }
  };

  /** Brings reused cases' videos into the footage, followed as a build is. */
  const bringReuses = async (urls?: string[]) => {
    setError(null);
    setStage("Bringing in the reused cases");
    try {
      const job = await importReuses(script.id, urls);
      setSent("Import the reused cases' videos for the opening.");
      follow(job.startedAt);
    } catch (cause) {
      setError(message(cause));
    }
  };
  /** A reused case's moment as an intro clip at the very start, with breathing room. */
  const insertMoment = (moment: Highlight) => {
    const clip: Clip = { id: crypto.randomUUID(), ...breathe(script.words, moment.startMs, moment.endMs, script.durationMs || moment.endMs) };
    void persist(insertAt(clips, 0, clip));
  };

  /** Runs a change that saves on its own (spelling, heading, a declined proposal) after the edits queued before it. */
  const change = (work: () => Promise<Script>) => {
    const mine = epoch.current;
    pending.current += 1;
    const run = queue.current.then(async () => {
      try {
        if (mine !== epoch.current) return;
        const saved = await work();
        revision.current = saved.revision;
        // Shown at once: a restore changes the clips, and the next edit builds on these.
        shown.current = saved.clips;
        setClips(saved.clips);
        onChange(saved);
      } catch (cause) {
        setError(message(cause));
      } finally {
        pending.current -= 1;
      }
    });
    queue.current = run;
    return run;
  };

  const reviewChanges = async (ids: string[], accept: boolean) => {
    if (busy) return;
    setResolving(true);
    try {
      await change(async () => {
        const before = shown.current;
        const saved = await resolveProposals(script.id, revision.current, ids, accept);
        if (accept) remember(before);
        return saved;
      });
    } finally { setResolving(false); }
  };

  const copy = async () => {
    const text = scriptText(script);
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Over the tailnet the page is plain http, where the Clipboard API is
      // missing. The old copy command still works from a click.
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

  const ready = script.words.length > 0;
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
          <span className="script-format">{KINDS.find((item) => item.value === script.kind)?.label ?? "Interrogation"}{script.compilation ? " compilation" : ""}</span>
          <Avatar creator={script.createdBy} size={24} />
          {saving && <span className={saving === "saved" ? "save-state ok" : "save-state"}>{saving === "saved" ? "Saved" : "Saving…"}</span>}
        </div>
        <div className="top-actions">
          <UpdateButton />
          {working && (
            <span className="agent-working">
              <span className="dot live" />
              {/reused|^Adding the footage/i.test(stage) ? "Importing footage" : "Claude writing"}
            </span>
          )}
          <ViewTabs view={view} onChange={(next) => (setExpanded(false), setView(next))} />
          <button type="button" className="icon-tool" aria-label="Undo" title="Undo (⌘Z)" disabled={!past.current.length || busy} onClick={undo}>
            <Icon name="undo" />
          </button>
          <button type="button" className="icon-tool" aria-label="Redo" title="Redo (⇧⌘Z)" disabled={!future.current.length || busy} onClick={redo}>
            <Icon name="redo" />
          </button>
          <button
            type="button"
            className="icon-tool"
            aria-pressed={agentShown}
            aria-label={agentShown ? "Hide Claude" : "Show Claude"}
            title={agentShown ? "Hide Claude" : "Show Claude"}
            onClick={() => setAgentShown(!agentShown)}
          >
            <Icon name="panel" />
          </button>
          <ReviewButton script={script} open={openNotes} approved={review.approved} onClick={() => setReviewOpen(true)} />
          <Menu
            label="More"
            items={[
              { label: "Google Doc", icon: "doc", disabled: !cueCount, onSelect: () => setDocsOpen(true) },
              ...(unfinishedSpeakers ? [{ label: "Retry unfinished speaker checks", icon: "reset", disabled: busy, onSelect: () => void retrySpeakers(script.id).then((job) => follow(job.startedAt), (cause) => setError(message(cause))) }] : []),
              ...(!script.noSpeech ? [{ label: "Complete transcription", icon: "reset", disabled: busy || saving === "saving", onSelect: () => void retry() }] : []),
              { label: "Versions", icon: "history", onSelect: () => setVersionsOpen(true) },
              { label: copied ? "Copied" : "Copy script text", icon: "copy", disabled: !cueCount, onSelect: () => void copy() },
              { label: "Keyboard shortcuts", icon: "keyboard", hint: "?", onSelect: () => setShortcutsOpen(true) },
            ]}
          />
          <Shortcuts groups={FOOTAGE_SHORTCUTS} open={shortcutsOpen} onOpenChange={setShortcutsOpen} />
        </div>
      </header>
      <ArchivedBanner script={script} working={working} onJob={(job) => follow(job.startedAt)} onProblem={setError} />
      {error && (
        <div className="notice">
          {error}
          <button type="button" className="ghost sm" onClick={() => setError(null)}>Dismiss</button>
        </div>
      )}
      <div
        className="editor-grid"
        data-view={view}
        data-expanded={expanded || undefined}
        data-agent={agentShown || undefined}
        data-resizing={split.resizing || undefined}
        style={split.style}
      >
        {view !== "video" && (
          <FootageDoc
            key={view}
            script={script}
            clips={clips}
            playhead={playhead}
            follow={view === "split"}
            zoomKey={view}
            disabled={busy}
            onPreview={(index) => view === "script" ? player.seek(clipStarts(clips)[index] ?? 0) : preview(index)}
            onNarration={(id, text) => void persist(clips.map((item) => (item.id === id ? { ...item, narration: text } : item)))}
            onHeading={(text) => void change(() => setHeading(script.id, revision.current, text))}
            onResolve={(ids, accept) => void reviewChanges(ids, accept)}
            onChat={(picked) => {
              setQuote(picked);
              setAgentShown(true);
              window.setTimeout(() => document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus(), 0);
            }}
            onNotes={(id, notes) => void persist(clips.map((item) => (item.id === id ? { ...item, notes: notes.length ? notes : undefined } : item)))}
            bands={bands}
          />
        )}
        {view === "split" && (
          <>
            {split.gutter("video")}
            {agentShown && split.gutter("agent")}
          </>
        )}
        <Monitor
          src={src}
          clips={clips}
          labels={labels}
          playhead={playhead}
          player={player}
          words={script.words}
          corrections={script.corrections}
          seekBar={view === "split"}
          expanded={expanded}
          onExpand={view === "video" ? setExpanded : undefined}
          onVideo={onVideo}
        />
        <section className="panel bin" aria-label="Media">
          <header className="panel-head">
            <div className="panel-tabs" role="tablist">
              <button type="button" role="tab" aria-selected={tab === "transcript"} onClick={() => setTab("transcript")}>
                Transcript
              </button>
              <button type="button" role="tab" aria-selected={tab === "research"} onClick={() => setTab("research")}>
                Research {research?.articles.length ? <small>{research.articles.length}</small> : null}
              </button>
            </div>
          </header>
          {tab === "transcript" ? (
            <>
              {script.words.length === 0 && !script.noSpeech ? (
                <div className="empty">
                  <p>No transcript yet.</p>
                  <button type="button" style={{ marginTop: 12 }} disabled={busy || saving === "saving"} onClick={() => void retry()}>
                    Transcribe
                  </button>
                </div>
              ) : (
                <Transcript
                  words={script.words}
                  clips={clips}
                  parts={script.parts}
                  speakers={speakers}
                  playhead={playhead}
                  player={player}
                  noSpeech={script.noSpeech}
                  corrections={script.corrections}
                  onPick={setPicked}
                />
              )}
              {picked && (
                <div className="selection">
                  <span>
                    {picked.count} {picked.count === 1 ? "word" : "words"} <span className="mono faint">{clock(picked.endMs - picked.startMs)}</span>
                  </span>
                  <span className="spacer" />
                  <button type="button" className="tool" onClick={() => player.audition(picked.startMs)}>
                    <Icon name="play" size={11} />
                    Play
                  </button>
                  <button type="button" className="tool" disabled={busy} title="Insert these words at the playhead, as an insert edit" onClick={() => insertPicked()}>
                    <Icon name="plus" size={12} />
                    Insert
                  </button>
                  <button type="button" className="tool" disabled={busy} title="Insert these words at the very start" onClick={() => insertPicked(0)}>
                    At start
                  </button>
                  {picked.count === 1 && !busy && (
                    <button type="button" className="tool" title="Fix how this word is spelled; its timing stays" onClick={() => setSpelling(script.corrections?.[picked.first.id] ?? picked.first.text)}>
                      Fix spelling
                    </button>
                  )}
                  <button type="button" className="tool danger-text" disabled={busy || !picked.inCut} title={picked.inCut ? "Take these words out of every clip in the cut" : "None of these words are in the cut"} onClick={liftPicked}>
                    Remove
                  </button>
                </div>
              )}
              {picked && spelling !== null && !busy && (
                <form
                  className="selection"
                  onSubmit={(event) => {
                    event.preventDefault();
                    const word = picked.first;
                    void change(() => correctWord(script.id, revision.current, word.id, spelling));
                    setSpelling(null);
                  }}
                >
                  <input
                    // biome-ignore lint/a11y/noAutofocus: opened by the button beside it
                    autoFocus
                    value={spelling}
                    maxLength={200}
                    aria-label="Spelling"
                    onChange={(event) => setSpelling(event.target.value.replace(/\s/g, ""))}
                    onKeyDown={(event) => event.key === "Escape" && setSpelling(null)}
                  />
                  <button type="submit" className="tool">Save</button>
                  {script.corrections?.[picked.first.id] && (
                    <button type="button" className="tool" title={`Back to "${picked.first.text}"`} onClick={() => (void change(() => correctWord(script.id, revision.current, picked.first.id, "")), setSpelling(null))}>
                      Undo fix
                    </button>
                  )}
                </form>
              )}
            </>
          ) : (
            <ResearchList id={script.id} research={research} onChange={setResearch} />
          )}
        </section>
        <Timeline
          clips={clips}
          words={script.words}
          overview={script.overview}
          durationMs={script.durationMs}
          parts={script.parts}
          labels={labels}
          playhead={playhead}
          player={player}
          selected={selected}
          disabled={busy}
          onSelect={setSelected}
          onEdit={(next) => void persist(next)}
        />
        <aside className="agent" aria-label="Claude">
          <div className="agent-head">
            <div className="panel-tabs" role="tablist">
              <button type="button" role="tab" aria-selected>
                Claude
              </button>
            </div>
          </div>
          <ChatLog trigger={`${chat.length}:${sent ?? ""}:${busy}`}>
            {!cueCount && !busy && !chat.length && (
              <div className="agent-empty">
                <h2>Write the script</h2>
                <p>
                  Claude Opus reads the whole transcript with Repack's cut-in guide, picks every clip and writes each cut-in. It takes a few minutes.
                </p>
                {script.noSpeech && <p>No words were heard in this footage, so it has no transcript. Cut it by time on the timeline.</p>}
              </div>
            )}
            <ReusedCases
              script={script}
              busy={busy}
              onImport={(urls) => void bringReuses(urls)}
              onPlay={(moment: Highlight) => player.audition(moment.startMs)}
              onInsert={insertMoment}
            />
            {notesShown && !!script.notes?.length && (
              <div className="turn notes">
                <ul>
                  {script.notes.map((text) => (
                    <li key={text}>{text}</li>
                  ))}
                </ul>
                {unfinishedSpeakers && <button type="button" className="tool" disabled={busy} onClick={() => void retrySpeakers(script.id).then((job) => follow(job.startedAt), (cause) => setError(message(cause)))}>Retry unfinished speaker checks</button>}
                <button type="button" className="link-button faint" onClick={() => setNotesShown(false)}>
                  Dismiss
                </button>
              </div>
            )}
            {chat.map((turn, index) =>
              turn.role === "producer" ? (
                <Asked key={`${turn.at}-${index}`} text={turn.text} at={turn.at} onRetry={!busy && (index === chat.length - 1 || (index === chat.length - 2 && !!chat[index + 1].failed)) ? () => void (cueCount ? revise(turn.text) : build(turn.text.replace(/^Build the script\.?\s*/i, ""))) : undefined} />
              ) : (
                <Fragment key={`${turn.at}-${index}`}>
                  {/* A reset asks with its own line and card, as Edit does, not a reply. */}
                  {!turn.reset && <Said text={turn.text} at={turn.at} costUsd={turn.costUsd} failed={turn.failed} />}
                  {turn.reset && (
                    <ResetAsk
                      state={turn.reset}
                      busy={busy}
                      onAnswer={(accept) =>
                        void resetScript(script.id, accept).then(
                          (fresh) => {
                            remember(shown.current);
                            onChange(fresh);
                            setClips(fresh.clips);
                            shown.current = fresh.clips;
                            revision.current = fresh.revision;
                            void getChat(script.id).then(setChat, () => {});
                          },
                          (cause) => setError(message(cause)),
                        )
                      }
                    />
                  )}
                </Fragment>
              ),
            )}
            {sent && working && <Asked text={sent} />}
            {cueCount > 0 && !busy && !chat.length && (
              <div className="turn">
                <p className="muted">
                  {clock(keptMs(clips))} cut · {cueCount} cut-ins. Ask for a change below: Claude edits the script where it needs to, and undo brings back what it changed.
                </p>
              </div>
            )}
            {!!script.proposals?.length && !busy && (
              <div className="proposals-note">
                <span>
                  {script.proposals.length === 1 ? "A rewrite is" : `${script.proposals.length} rewrites are`} waiting for your review beside the narration.
                </span>
                {view === "video" && (
                  <button type="button" className="link-button" onClick={() => setView("script")}>
                    Review in Script view
                  </button>
                )}
              </div>
            )}
            {working && (
              <Working
                stage={stage}
                since={started}
                hint={
                  stage === "Editing the script"
                    ? "Edits usually take a minute or two."
                    : /reused/i.test(stage) || stage.startsWith("Adding the footage")
                      ? "Each reused case is downloaded, transcribed and read for its strongest moments. A long video takes several minutes."
                      : "Long footage takes 5 to 15 minutes. You can keep reading the transcript meanwhile."
                }
              />
            )}
          </ChatLog>
          <Composer
            onStop={working ? () => void stopJob(script.id).catch((cause) => setError(message(cause))) : undefined}
            value={note}
            disabled={busy}
            canSend={!busy && ready && (!cueCount || !!note.trim())}
            label={cueCount ? "Ask for a change" : "Build the script"}
            model="Opus 5.5"
            quote={quote ? `Narration ${quote.narrationNumber}: “${quote.quote}”` : undefined}
            onClearQuote={() => setQuote(null)}
            onNewChat={chat.length > 0 ? () => setClearing(true) : undefined}
            placeholder={
              archived
                ? "The footage is archived: restore it to change the script"
                : busy
                ? "Claude is working…"
                : quote ? "What should we do with this passage?" : cueCount
                  ? "Ask for a change, e.g. tighten the intro, rewrite cut-in 4 shorter, fix the edges…"
                  : "Notes for the build (optional), e.g. aim for 30 minutes…"
            }
            hint={!ready ? "Transcribe the footage first" : undefined}
            onChange={setNote}
            onSend={() => void (cueCount ? revise() : build())}
          />
        </aside>
      </div>
      <Dialog
        open={clearing}
        onClose={() => setClearing(false)}
        title="Start a new conversation?"
        description="This conversation with Claude is cleared. The script stays exactly as it is."
        width={400}
        footer={
          <>
            <span className="spacer" />
            <button type="button" className="ghost" onClick={() => setClearing(false)}>Cancel</button>
            <button
              type="button"
              className="primary"
              data-autofocus
              onClick={() => {
                setClearing(false);
                void clearChat(script.id).then(() => setChat([]), (cause) => setError(message(cause)));
              }}
            >
              New conversation
            </button>
          </>
        }
      />
      <VersionsDialog
        open={versionsOpen}
        script={script}
        busy={busy}
        onClose={() => setVersionsOpen(false)}
        onRestore={(version) => {
          remember(shown.current);
          void change(() => restoreVersion(script.id, revision.current, version.id));
          setVersionsOpen(false);
        }}
      />
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
        onUseComments={cueCount ? (text) => setNote(`Work through these comments from the Google Doc:\n${text}`) : undefined}
      />
    </div>
  );
}
