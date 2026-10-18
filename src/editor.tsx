import { convertFileSrc } from "@tauri-apps/api/core";
import { type CSSProperties, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Composer, Said, Working } from "./agent";
import { buildStatus, getResearch, getScript, inTauri, message, retryTranscribe, saveClips, startBuild } from "./api";
import { clock, document, insertAfter, isCutIn, keptMs, purposes, splitClip } from "./clips";
import { DocsDialog } from "./docs-dialog";
import { FootageDoc } from "./footage-doc";
import { Monitor } from "./monitor";
import { CutPlayer, clipStarts, editedPosition, Playhead } from "./playback";
import { Timeline } from "./timeline";
import { type Picked, ResearchList, Transcript, useSpeakers } from "./transcript";
import type { BuildStatus, Clip, Kind, Research, Script } from "./types";
import { FOOTAGE_SHORTCUTS, Shortcuts } from "./shortcuts";
import { Icon } from "./ui";

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
  { value: "foia", label: "FOIA" },
  { value: "courtroom", label: "Courtroom" },
];

type Report = { text: string; costUsd: number };

/**
 * A footage script, as Repack's editor: the Video view (the monitor, the
 * transcript and research, the timeline), the Script view (its Doc), and Split
 * (the monitor beside the Doc). Claude sits at the right in all three.
 */
export function Editor({ script, onChange, onBack }: { script: Script; onChange: (script: Script) => void; onBack: () => void }) {
  const clips = script.clips;
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
  const [copied, setCopied] = useState(false);
  const [started, setStarted] = useState<number | null>(null);
  const [stage, setStage] = useState("");
  const [report, setReport] = useState<Report | null>(null);
  const [research, setResearch] = useState<Research | null>(null);
  const [note, setNote] = useState("");
  const [kind, setKind] = useState<Kind>(script.kind ?? "interrogation");
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
  const src = !script.proxyPath ? "" : inTauri() ? convertFileSrc(script.proxyPath) : `/api/scripts/${script.id}/proxy`;
  const busy = started !== null;

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

  const persist = async (next: Clip[], record = true) => {
    const before = clips;
    setSaving("saving");
    setError(null);
    try {
      onChange(await saveClips(script.id, script.revision, next));
      if (record) remember(before);
      setSaving("saved");
      window.setTimeout(() => setSaving((state) => (state === "saved" ? null : state)), 1600);
      return true;
    } catch (cause) {
      setSaving(null);
      setError(message(cause));
      return false;
    }
  };

  const undo = async () => {
    const previous = past.current.at(-1);
    if (!previous || saving === "saving") return;
    if (await persist(previous, false)) {
      past.current = past.current.slice(0, -1);
      future.current = [...future.current, clips];
      setHistory((n) => n + 1);
    }
  };

  const redo = async () => {
    const next = future.current.at(-1);
    if (!next || saving === "saving") return;
    if (await persist(next, false)) {
      future.current = future.current.slice(0, -1);
      past.current = [...past.current.slice(-29), clips];
      setHistory((n) => n + 1);
    }
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

  const addPicked = () => {
    if (!picked) return;
    const position = editedPosition(clips, playhead.ms);
    const clip: Clip = { id: crypto.randomUUID(), startMs: picked.startMs, endMs: picked.endMs };
    void persist(insertAfter(clips, position ? position.index : clips.length - 1, clip));
  };
  const liftPicked = () => {
    if (!picked) return;
    void persist(
      clips.flatMap((clip) => {
        if (isCutIn(clip) || clip.endMs <= picked.startMs || clip.startMs >= picked.endMs) return [clip];
        const kept: Clip[] = [];
        if (clip.startMs < picked.startMs) kept.push({ ...clip, endMs: picked.startMs });
        if (clip.endMs > picked.endMs) kept.push({ ...clip, id: kept.length ? crypto.randomUUID() : clip.id, startMs: picked.endMs });
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
      if (event.key === "y" || event.shiftKey) void redo();
      else void undo();
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
    setSaving("saving");
    setError(null);
    try {
      onChange(await retryTranscribe(script.id));
    } catch (cause) {
      setError(message(cause));
    } finally {
      setSaving(null);
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
      if (!job) return;
      if (job.error) {
        setError(job.error);
        return;
      }
      player.pause();
      try {
        const before = latest.current;
        onChange(await getScript(script.id));
        if (before.length) remember(before);
      } catch (cause) {
        setError(message(cause));
      }
      setReport({ text: job.report ?? "", costUsd: job.costUsd ?? 0 });
      void getResearch(script.id).then(setResearch).catch(() => {});
    };
    window.setTimeout(poll, 3000);
  };

  useEffect(() => {
    alive.current = true;
    void buildStatus(script.id)
      .then((job) => job?.running && follow(job.startedAt))
      .catch(() => {});
    return () => {
      alive.current = false;
    };
    // Only on opening a script; follow() keeps itself going.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [script.id]);

  const build = async () => {
    setReport(null);
    setError(null);
    setStage("Starting");
    try {
      const job = await startBuild(script.id, note, kind);
      setNote("");
      follow(job.startedAt);
    } catch (cause) {
      setError(message(cause));
    }
  };

  const copy = async () => {
    const text = document(script);
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
          {saving && <span className={saving === "saved" ? "save-state ok" : "save-state"}>{saving === "saved" ? "Saved" : "Saving…"}</span>}
        </div>
        <div className="top-actions">
          {busy && (
            <span className="agent-working">
              <span className="dot live" />
              Claude writing
            </span>
          )}
          <ViewTabs view={view} onChange={(next) => (setExpanded(false), setView(next))} />
          <button type="button" className="icon-tool" aria-label="Undo" title="Undo (⌘Z)" disabled={!past.current.length || saving === "saving"} onClick={() => void undo()}>
            <Icon name="undo" />
          </button>
          <button type="button" className="icon-tool" aria-label="Redo" title="Redo (⇧⌘Z)" disabled={!future.current.length || saving === "saving"} onClick={() => void redo()}>
            <Icon name="redo" />
          </button>
          <Shortcuts groups={FOOTAGE_SHORTCUTS} />
          <button type="button" className="icon-tool" aria-label="Copy script" disabled={!cueCount} title="Copy the script as Repack's Doc text" onClick={() => void copy()}>
            <Icon name={copied ? "check" : "copy"} />
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
          <button type="button" className="secondary sm" disabled={!cueCount} title="Export to Google Docs, see earlier exports and read comments" onClick={() => setDocsOpen(true)}>
            <Icon name="doc" size={14} />
            Google Doc
          </button>
        </div>
      </header>
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
            disabled={saving === "saving" || busy}
            onPreview={preview}
            onNarration={(id, text) => void persist(clips.map((item) => (item.id === id ? { ...item, narration: text } : item)))}
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
                  <button type="button" style={{ marginTop: 12 }} disabled={saving === "saving"} onClick={() => void retry()}>
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
                  onPick={setPicked}
                />
              )}
              {picked && (
                <div className="selection">
                  <span>
                    {picked.count} words <span className="mono faint">{clock(picked.endMs - picked.startMs)}</span>
                  </span>
                  <span className="spacer" />
                  <button type="button" className="tool" onClick={() => player.audition(picked.startMs)}>
                    <Icon name="play" size={11} />
                    Play
                  </button>
                  <button type="button" className="tool" disabled={saving === "saving"} title="Add these words to the cut, after the clip at the playhead" onClick={addPicked}>
                    <Icon name="plus" size={12} />
                    Add to cut
                  </button>
                  <button type="button" className="tool danger-text" disabled={saving === "saving" || !picked.inCut} title={picked.inCut ? "Take these words out of every clip in the cut" : "None of these words are in the cut"} onClick={liftPicked}>
                    Remove
                  </button>
                </div>
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
          disabled={saving === "saving" || busy}
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
          <div className="chat-log">
            {!cueCount && !busy && (
              <div className="agent-empty">
                <h2>Write the script</h2>
                <p>
                  Claude Opus reads the whole transcript with Repack's cut-in guide, picks every clip and writes each cut-in. It takes a few minutes.
                </p>
                {script.noSpeech && <p>No words were heard in this footage, so it has no transcript. Cut it by time on the timeline.</p>}
              </div>
            )}
            {notesShown && !!script.notes?.length && (
              <div className="turn notes">
                <ul>
                  {script.notes.map((text) => (
                    <li key={text}>{text}</li>
                  ))}
                </ul>
                <button type="button" className="link-button faint" onClick={() => setNotesShown(false)}>
                  Dismiss
                </button>
              </div>
            )}
            {report && !busy && <Said text={report.text || "Script written."} costUsd={report.costUsd} />}
            {cueCount > 0 && !report && !busy && (
              <div className="turn">
                <p className="muted">
                  {clock(keptMs(clips))} cut · {cueCount} cut-ins. Ask for a rebuild below; it replaces the clips and narration, and undo brings them back.
                </p>
              </div>
            )}
            {busy && <Working stage={stage} since={started} hint="Long footage takes 5 to 15 minutes. You can keep reading the transcript meanwhile." />}
          </div>
          <Composer
            value={note}
            disabled={busy}
            canSend={!busy && ready}
            label={cueCount ? "Rebuild the script" : "Build the script"}
            placeholder={busy ? "Claude is writing…" : cueCount ? "Ask for a rebuild, e.g. aim for 30 minutes, open on the confession…" : "Notes for the build (optional), e.g. aim for 30 minutes…"}
            hint={!ready ? "Transcribe the footage first" : undefined}
            extra={
              <select className="kind-select" value={kind} disabled={busy} title="The format Claude writes in" onChange={(event) => setKind(event.target.value as Kind)}>
                {KINDS.map((item) => (
                  <option key={item.value} value={item.value}>{item.label}</option>
                ))}
              </select>
            }
            onChange={setNote}
            onSend={() => void build()}
          />
        </aside>
      </div>
      <DocsDialog open={docsOpen} script={script} onClose={() => setDocsOpen(false)} onChange={onChange} />
    </div>
  );
}
