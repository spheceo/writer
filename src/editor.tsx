import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { buildStatus, getResearch, getScript, inTauri, message, researchAgain, retryTranscribe, saveClips, startBuild } from "./api";
import { type Block, blocks, clock, covers, document, heading, isCutIn, keptMs, lift, lines, partAt, purposes, range } from "./clips";
import type { BuildStatus, Clip, Kind, Part, Research, Script, Word } from "./types";
import { DocsDialog } from "./docs-dialog";
import { Icon, Tabs } from "./ui";

const SPEAKERS = ["#8fbaf0", "#f3a48e", "#a9e091", "#d0aaf3", "#e6cb82", "#86d3d6"];

export function Editor({
  script,
  onChange,
  onBack,
}: {
  script: Script;
  onChange: (script: Script) => void;
  onBack: () => void;
}) {
  const video = useRef<HTMLVideoElement>(null);
  const wave = useRef<HTMLCanvasElement>(null);
  const transcript = useRef<HTMLDivElement>(null);
  const timer = useRef<number | null>(null);
  const running = useRef(false);
  const cursorRef = useRef<number | null>(null);
  const jumped = useRef(false);
  const [playing, setPlaying] = useState(false);
  const [at, setAt] = useState(0);
  const [cursor, setCursorState] = useState<number | null>(null);
  const [card, setCard] = useState<Clip | null>(null);
  const [cutMode, setCutMode] = useState(true);
  const [anchor, setAnchor] = useState<string | null>(null);
  const [focus, setFocus] = useState<string | null>(null);
  const [saving, setSaving] = useState<"saving" | "saved" | null>(null);
  const [notesShown, setNotesShown] = useState(true);
  const [docsOpen, setDocsOpen] = useState(false);

  // Closing or reloading with an edit still on its way asks first.
  useEffect(() => {
    if (saving !== "saving") return;
    const hold = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", hold);
    return () => window.removeEventListener("beforeunload", hold);
  }, [saving]);
  const [error, setError] = useState<string | null>(script.error);
  const [copied, setCopied] = useState(false);
  const [started, setStarted] = useState<number | null>(null);
  const [stage, setStage] = useState("");
  const [research, setResearch] = useState<Research | null>(null);
  useEffect(() => {
    void getResearch(script.id).then(setResearch).catch(() => {});
  }, [script.id]);
  const [report, setReport] = useState<Report | null>(null);
  const [rebuilding, setRebuilding] = useState(false);

  const clips = script.clips;
  const latest = useRef(clips);
  latest.current = clips;
  const groups = useMemo(() => lines(script.words), [script.words]);
  const parts = useMemo(() => blocks(clips), [clips]);
  const cueCount = clips.filter(isCutIn).length;
  const runtime = keptMs(clips);
  const speakers = useMemo(() => {
    const map = new Map<string, { name: string; colour: string }>();
    for (const word of script.words) {
      const key = word.speaker ?? "";
      if (!map.has(key)) {
        const named = script.speakers?.[key];
        map.set(key, { name: named ?? `Speaker ${map.size + 1}`, colour: SPEAKERS[map.size % SPEAKERS.length] });
      }
    }
    return map;
  }, [script.words, script.speakers]);
  // Each cut-in's purpose label, by format.
  const labels = useMemo(() => purposes(script), [script]);
  const src = !script.proxyPath
    ? ""
    : inTauri()
      ? convertFileSrc(script.proxyPath)
      : `/api/scripts/${script.id}/proxy`;

  const setCursor = (index: number | null) => {
    cursorRef.current = index;
    setCursorState(index);
  };
  const clearTimer = () => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
  };
  useEffect(() => clearTimer, []);

  useEffect(() => {
    const canvas = wave.current;
    if (!canvas) return;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.floor(width * ratio);
    canvas.height = Math.floor(height * ratio);
    const pen = canvas.getContext("2d");
    if (!pen) return;
    pen.scale(ratio, ratio);
    pen.clearRect(0, 0, width, height);
    const bars = script.overview.length ? script.overview : [0.2];
    const gap = width / bars.length;
    bars.forEach((peak, index) => {
      const bar = Math.max(2, peak * (height - 4));
      const ms = script.durationMs ? (index / bars.length) * script.durationMs : 0;
      pen.fillStyle = covers(clips, ms) ? "#7088dc" : "#262626";
      pen.fillRect(index * gap, (height - bar) / 2, Math.max(1, gap - 1), bar);
    });
    if (script.durationMs > 0) {
      pen.fillStyle = "#4f9dff";
      pen.fillRect((at / script.durationMs) * width, 0, 1.5, height);
    }
  }, [script.overview, clips, script.durationMs, at]);

  // The word under the playhead.
  const live = useMemo(() => {
    const words = script.words;
    let lo = 0;
    let hi = words.length - 1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (words[mid].endMs <= at) lo = mid + 1;
      else if (words[mid].startMs > at) hi = mid - 1;
      else return words[mid].id;
    }
    return null;
  }, [script.words, at]);

  // Keep the transcript on the playhead while playing, or after a jump from the script.
  useEffect(() => {
    if (!live || !(playing || jumped.current)) return;
    jumped.current = false;
    const box = transcript.current;
    const node = box?.querySelector<HTMLElement>(`[data-id="${live}"]`);
    if (!box || !node) return;
    // The transcript is the word's offset parent.
    const top = node.offsetTop;
    if (top < box.scrollTop + 40 || top > box.scrollTop + box.clientHeight - 80) {
      box.scrollTo({ top: top - box.clientHeight / 3, behavior: playing ? "smooth" : "auto" });
    }
  }, [live, playing]);

  const seek = (ms: number) => {
    const node = video.current;
    if (node) node.currentTime = Math.max(0, ms) / 1000;
    setAt(ms);
  };

  const stop = () => {
    running.current = false;
    clearTimer();
    video.current?.pause();
  };

  /** Move the cut to item `index`. A cut-in holds its card for 3 s, as the card does in Repack. */
  const goto = (index: number, play: boolean) => {
    clearTimer();
    jumped.current = true;
    if (index >= clips.length) {
      stop();
      setPlaying(false);
      setCard(null);
      setCursor(null);
      return;
    }
    const item = clips[index];
    setCursor(index);
    if (isCutIn(item)) {
      video.current?.pause();
      setCard(item);
      if (play) timer.current = window.setTimeout(() => goto(index + 1, running.current), item.endMs - item.startMs);
      return;
    }
    setCard(null);
    seek(item.startMs);
    if (play) void video.current?.play();
  };

  const toggle = () => {
    const node = video.current;
    if (!cutMode) {
      if (!node) return;
      if (node.paused) void node.play();
      else node.pause();
      return;
    }
    if (running.current) {
      stop();
      setPlaying(false);
      return;
    }
    running.current = true;
    setPlaying(true);
    const current = cursorRef.current;
    const item = current === null ? null : clips[current];
    if (item && !isCutIn(item) && at >= item.startMs && at < item.endMs - 50) {
      void node?.play();
      return;
    }
    const inside = clips.findIndex((clip) => !isCutIn(clip) && at >= clip.startMs && at < clip.endMs);
    goto(current ?? (inside >= 0 ? inside : 0), true);
  };

  const switchMode = (cut: boolean) => {
    if (cut === cutMode) return;
    stop();
    setPlaying(false);
    setCard(null);
    setCutMode(cut);
  };

  const onTime = () => {
    const node = video.current;
    if (!node) return;
    const ms = node.currentTime * 1000;
    setAt(ms);
    if (!cutMode || !running.current) return;
    const index = cursorRef.current;
    const item = index === null ? null : clips[index];
    if (index !== null && item && !isCutIn(item) && ms >= item.endMs) goto(index + 1, true);
  };

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

  const pick = (word: Word, shift: boolean) => {
    seek(word.startMs);
    setCard(null);
    const inside = clips.findIndex((clip) => !isCutIn(clip) && word.startMs >= clip.startMs && word.startMs < clip.endMs);
    setCursor(inside >= 0 ? inside : null);
    if (!shift || !anchor) {
      setAnchor(word.id);
      setFocus(word.id);
      return;
    }
    setFocus(word.id);
  };

  const clearSelection = () => {
    setAnchor(null);
    setFocus(null);
  };

  const selected = (() => {
    if (!anchor || !focus) return null;
    const from = script.words.findIndex((word) => word.id === anchor);
    const to = script.words.findIndex((word) => word.id === focus);
    if (from < 0 || to < 0 || from === to) return null;
    const [lo, hi] = from < to ? [from, to] : [to, from];
    const startMs = script.words[lo].startMs;
    const endMs = script.words[hi].endMs;
    const inCut = script.words.slice(lo, hi + 1).some((word) => covers(clips, word.startMs));
    return { startMs, endMs, count: hi - lo + 1, inCut };
  })();

  // Space plays and pauses, Escape drops a selection, unless you are typing.
  const keys = useRef({ toggle, clearSelection, undo, redo });
  keys.current = { toggle, clearSelection, undo, redo };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if ((event.metaKey || event.ctrlKey) && (event.key === "z" || event.key === "Z" || event.key === "y")) {
        // A text field keeps its own undo while it is being typed in.
        if (target && (target.tagName === "TEXTAREA" || target.tagName === "INPUT")) return;
        event.preventDefault();
        if (event.key === "y" || event.shiftKey) void keys.current.redo();
        else void keys.current.undo();
        return;
      }
      if (target && (target.tagName === "TEXTAREA" || target.tagName === "INPUT")) return;
      if (event.code === "Space") {
        event.preventDefault();
        keys.current.toggle();
      } else if (event.key === "Escape") {
        keys.current.clearSelection();
      }
    };
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
      stop();
      setPlaying(false);
      setCard(null);
      setCursor(null);
      setRebuilding(false);
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

  const build = async (note: string, kind: Kind) => {
    setReport(null);
    setError(null);
    setStage("Starting");
    try {
      const job = await startBuild(script.id, note, kind);
      follow(job.startedAt);
      return true;
    } catch (cause) {
      setError(message(cause));
      return false;
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

  const position = (() => {
    if (cursor === null) return null;
    let ms = 0;
    for (let index = 0; index < cursor; index++) ms += clips[index].endMs - clips[index].startMs;
    const item = clips[cursor];
    return item && !isCutIn(item) ? ms + Math.max(0, Math.min(at, item.endMs) - item.startMs) : ms;
  })();
  const cueIndex = (clip: Clip) => clips.filter(isCutIn).findIndex((item) => item.id === clip.id);

  return (
    <div className="app">
      <header className="top" data-tauri-drag-region>
        <button type="button" className="ghost sm" onClick={onBack}>← Library</button>
        <h1 data-tauri-drag-region>{script.title}</h1>
        <span className="spacer" data-tauri-drag-region />
        {saving && <span className={saving === "saved" ? "ok" : "muted"}>{saving === "saved" ? "Saved" : "Saving…"}</span>}
        <button
          type="button"
          className="icon-button"
          aria-label="Undo"
          title="Undo (⌘Z)"
          disabled={!past.current.length || saving === "saving"}
          onClick={() => void undo()}
        >
          <Icon name="undo" />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label="Redo"
          title="Redo (⇧⌘Z)"
          disabled={!future.current.length || saving === "saving"}
          onClick={() => void redo()}
        >
          <Icon name="redo" />
        </button>
        <button type="button" className="sm" disabled={!cueCount} title="Export to Google Docs, see earlier exports and read comments" onClick={() => setDocsOpen(true)}>
          Google Doc
        </button>
        <button
          type="button"
          className={cueCount ? "primary sm" : "sm"}
          disabled={!cueCount}
          title="Copies the script as Repack's Doc text: title, length, then ranges and narration"
          onClick={() => void copy()}
        >
          {copied ? "Copied" : "Copy script"}
        </button>
      </header>
      {error && (
        <div className="notice">
          {error}
          <button type="button" className="ghost sm" onClick={() => setError(null)}>Dismiss</button>
        </div>
      )}
      {notesShown && !!script.notes?.length && (
        <div className="notice info">
          <ul>
            {script.notes.map((text) => (
              <li key={text}>{text}</li>
            ))}
          </ul>
          <button type="button" className="ghost sm" onClick={() => setNotesShown(false)}>Dismiss</button>
        </div>
      )}
      <div className="editor">
        <section className="panel">
          <div className="monitor">
            {src ? (
              <video
                ref={video}
                src={src}
                onClick={toggle}
                onTimeUpdate={onTime}
                onPlay={() => setPlaying(true)}
                onPause={() => !running.current && setPlaying(false)}
                onEnded={() => cutMode && running.current && cursor !== null && goto(cursor + 1, true)}
              />
            ) : (
              <div className="none">No preview yet.</div>
            )}
            {card && (
              <div className="card">
                <span className="role">Cut-in · {labels[cueIndex(card)]}</span>
                <p>{card.narration || "No narration yet."}</p>
              </div>
            )}
          </div>
          <div className="transport">
            <button
              type="button"
              className="play primary"
              aria-label={playing ? "Pause" : "Play"}
              title="Play or pause (Space)"
              onClick={toggle}
            >
              {playing ? "❚❚" : "▶"}
            </button>
            <span className="mono">
              {cutMode && position !== null ? clock(position) : clock(at)}
              <span className="faint"> / {cutMode ? clock(runtime) : clock(script.durationMs)}</span>
            </span>
            <span className="spacer" />
            <div className="segmented" role="group" aria-label="What plays">
              <button
                type="button"
                className={cutMode ? "on" : ""}
                title="Play the edited video: the chosen clips and cut-in cards, in order"
                onClick={() => switchMode(true)}
              >
                Cut
              </button>
              <button
                type="button"
                className={cutMode ? "" : "on"}
                title="Play the original footage straight through"
                onClick={() => switchMode(false)}
              >
                Source
              </button>
            </div>
          </div>
          {clips.length > 0 && (
            <div className="lane">
              <span>Cut</span>
              <div className="strip">
                {clips.map((clip, index) => (
                  <button
                    type="button"
                    key={clip.id}
                    title={isCutIn(clip) ? `Cut-in · ${labels[cueIndex(clip)]}` : `Clip ${range(clip, script.parts)}`}
                    className={[isCutIn(clip) ? "say" : "", cursor === index ? "on" : ""].join(" ")}
                    style={{ flexGrow: clip.endMs - clip.startMs }}
                    onClick={() => goto(index, running.current)}
                  />
                ))}
              </div>
            </div>
          )}
          <div className="lane">
            <span>Source</span>
            <canvas
              ref={wave}
              className="wave"
              title="The whole recording. Blue is in the cut."
              onClick={(event) => {
                const bounds = event.currentTarget.getBoundingClientRect();
                jumped.current = true;
                seek(((event.clientX - bounds.left) / bounds.width) * script.durationMs);
                setCard(null);
              }}
            />
          </div>
          <div className="panel-head sub">
            <h2>Transcript</h2>
            <span className="faint">Click a word to jump there · Shift-click to select · Dimmed words aren't in the cut</span>
          </div>
          <div className="transcript" ref={transcript}>
            {groups.length === 0 && (
              <div className="empty">
                {script.noSpeech ? (
                  <p>No words were heard in this footage.</p>
                ) : (
                  <>
                    <p>No transcript yet.</p>
                    <button type="button" style={{ marginTop: 12 }} disabled={saving === "saving"} onClick={retry}>
                      Transcribe
                    </button>
                  </>
                )}
              </div>
            )}
            {groups.map((line, index) => {
              const who = speakers.get(line.speaker ?? "");
              const part = partAt(script.parts, line.startMs);
              const opens = part >= 0 && (index === 0 || partAt(script.parts, groups[index - 1].startMs) !== part);
              return (
                <div key={line.words[0].id}>
                {opens && script.parts && (
                  <div className="recording">
                    Recording {part + 1} · {script.parts[part].title}
                    {script.parts[part].audio && <span>audio</span>}
                  </div>
                )}
                <p className="line">
                  <span className="who">
                    <b style={{ color: who?.colour }}>{who?.name ?? "Speaker"}</b>
                    <span>{clock(line.startMs)}</span>
                  </span>
                  {line.words.map((word) => {
                    const picked = selected && word.startMs >= selected.startMs && word.endMs <= selected.endMs;
                    const kept = covers(clips, word.startMs);
                    return (
                      <button
                        type="button"
                        key={word.id}
                        data-id={word.id}
                        className={["word", picked ? "pick" : word.id === live ? "on" : "", kept ? "" : "out"].join(" ")}
                        onClick={(event) => pick(word, event.shiftKey)}
                      >
                        {word.text}{" "}
                      </button>
                    );
                  })}
                </p>
                </div>
              );
            })}
          </div>
          {selected && (
            <div className="selection">
              <span>
                {selected.count} words selected <span className="mono faint">{clock(selected.endMs - selected.startMs)}</span>
              </span>
              <span className="spacer" />
              <button type="button" className="ghost sm" onClick={clearSelection}>Clear</button>
              <button
                type="button"
                className="sm danger"
                disabled={saving === "saving" || !selected.inCut}
                title={selected.inCut ? "Takes these words out of every clip in the cut" : "None of these words are in the cut"}
                onClick={() => {
                  void persist(lift(clips, selected.startMs, selected.endMs));
                  clearSelection();
                }}
              >
                Remove from cut
              </button>
            </div>
          )}
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Script</h2>
            {cueCount > 0 && (
              <span className="faint">{clock(runtime)} · {cueCount} cut-ins</span>
            )}
            <span className="spacer" />
            {cueCount > 0 && !rebuilding && started === null && (
              <button type="button" className="ghost sm" onClick={() => setRebuilding(true)}>Rebuild…</button>
            )}
          </div>
          {cueCount > 0 ? (
            <div className="doc">
              {(rebuilding || started !== null) && (
                <Builder
                  ready={script.words.length > 0}
                  started={started}
                  stage={stage}
                  kind={script.kind ?? "interrogation"}
                  compilation={!!script.compilation}
                  onBuild={build}
                  onCancel={() => setRebuilding(false)}
                  rebuild
                />
              )}
              {report && started === null && (
                <div className="report">
                  <p>{report.text || "Script written."}</p>
                  {report.costUsd > 0 && (
                    <small>About ${report.costUsd.toFixed(2)} at API prices, covered by your Claude plan.</small>
                  )}
                  <button type="button" className="ghost sm" onClick={() => setReport(null)}>Dismiss</button>
                </div>
              )}
              <ResearchPanel id={script.id} research={research} onChange={setResearch} />
              <h3>{heading(script)}</h3>
              <p className="length">Total duration {clock(runtime)}</p>
              {parts.map((block, index) => (
                <ScriptBlock
                  key={block.cue?.clip.id ?? block.clips[0]?.clip.id ?? index}
                  block={block}
                  recordings={script.parts}
                  cursor={cursor}
                  role={block.cue ? labels[cueIndex(block.cue.clip)] : null}
                  disabled={saving === "saving"}
                  onPlay={(at) => goto(at, true)}
                  onJump={(at) => goto(at, running.current)}
                  onRemove={(id) => persist(clips.filter((item) => item.id !== id))}
                  onNarration={(id, text) =>
                    persist(clips.map((item) => (item.id === id ? { ...item, narration: text } : item)))
                  }
                />
              ))}
            </div>
          ) : (
            <div className="start">
              <h3>Write the script</h3>
              <p className="muted">
                Claude Opus reads the whole transcript with Repack's cut-in guide, picks every clip and writes each
                cut-in. It takes a few minutes.
              </p>
              {script.noSpeech ? (
                <p className="faint">No words were heard in this footage, so it has no transcript. Cut it by time on the timeline.</p>
              ) : (
                <Builder
                  ready={script.words.length > 0}
                  started={started}
                  stage={stage}
                  kind={script.kind ?? "interrogation"}
                  compilation={!!script.compilation}
                  onBuild={build}
                />
              )}
            </div>
          )}
        </section>
      </div>
      <DocsDialog open={docsOpen} script={script} onClose={() => setDocsOpen(false)} onChange={onChange} />
    </div>
  );
}

function ScriptBlock({
  block,
  recordings,
  cursor,
  role,
  disabled,
  onPlay,
  onJump,
  onRemove,
  onNarration,
}: {
  block: Block;
  recordings?: Part[];
  cursor: number | null;
  role: string | null;
  disabled: boolean;
  onPlay: (index: number) => void;
  onJump: (index: number) => void;
  onRemove: (id: string) => void;
  onNarration: (id: string, text: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const footage = block.clips.reduce((sum, { clip }) => sum + clip.endMs - clip.startMs, 0);
  const here = block.clips.some(({ index }) => index === cursor);
  const cue = block.cue;
  return (
    <div className="block">
      {block.clips.length > 0 && (
        <div className={here ? "footage on" : "footage"}>
          <button type="button" className="ghost sm play-block" onClick={() => onPlay(block.clips[0].index)}>
            ▶ Footage <span className="mono">{clock(footage)}</span>
          </button>
          <span className="faint">
            {block.clips.length} {block.clips.length === 1 ? "clip" : "clips"} · from {range(block.clips[0].clip, recordings)}
          </span>
          <span className="spacer" />
          <button type="button" className="ghost sm" onClick={() => setOpen(!open)}>
            {open ? "Hide clips" : "Show clips"}
          </button>
        </div>
      )}
      {open && (
        <div className="ranges">
          {block.clips.map(({ clip, index }) => (
            <span className={cursor === index ? "chip on" : "chip"} key={clip.id}>
              <button type="button" title="Jump to this clip" onClick={() => onJump(index)}>{range(clip, recordings)}</button>
              <button
                type="button"
                className="x"
                aria-label="Remove clip"
                title="Remove this clip from the cut"
                disabled={disabled}
                onClick={() => onRemove(clip.id)}
              >
                ✕
              </button>
            </span>
          ))}
        </div>
      )}
      {cue && (
        <div className={cursor === cue.index ? "cue on" : "cue"}>
          <div className="role">
            {role}
            <span className="faint">{(cue.clip.narration ?? "").trim().split(/\s+/).filter(Boolean).length} words</span>
          </div>
          <Narration
            value={cue.clip.narration ?? ""}
            onCommit={(text) => onNarration(cue.clip.id, text)}
          />
        </div>
      )}
    </div>
  );
}

function Narration({ value, onCommit }: { value: string; onCommit: (text: string) => void }) {
  const [text, setText] = useState(value);
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => setText(value), [value]);
  useLayoutEffect(() => {
    const node = area.current;
    if (!node) return;
    node.style.height = "auto";
    node.style.height = `${node.scrollHeight}px`;
  }, [text]);
  return (
    <textarea
      ref={area}
      rows={1}
      value={text}
      spellCheck
      placeholder="Write the narration for this cut-in…"
      onChange={(event) => setText(event.target.value)}
      onBlur={() => text !== value && onCommit(text)}
    />
  );
}

type Report = { text: string; costUsd: number };

const KINDS: { value: Kind; label: string }[] = [
  { value: "interrogation", label: "Interrogation" },
  { value: "foia", label: "FOIA" },
  { value: "courtroom", label: "Courtroom" },
];

/** What research found, and a way to look again. */
function ResearchPanel({
  id,
  research,
  onChange,
}: {
  id: string;
  research: Research | null;
  onChange: (research: Research) => void;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  if (!research && !busy) return null;
  const count = research?.articles.length ?? 0;
  const again = async () => {
    setBusy(true);
    setProblem(null);
    try {
      onChange(await researchAgain(id));
      setOpen(true);
    } catch (cause) {
      setProblem(message(cause));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="research">
      <div className="research-head">
        <button type="button" className="ghost sm" onClick={() => setOpen(!open)}>
          {open ? "▾" : "▸"} Research
        </button>
        <span className="faint">
          {busy ? "Searching…" : count ? `${count} ${count === 1 ? "report" : "reports"}` : (research?.problem ?? "Nothing found")}
        </span>
        <span className="spacer" />
        <button type="button" className="ghost sm" disabled={busy} onClick={() => void again()}>
          Research again
        </button>
      </div>
      {problem && <p className="bad">{problem}</p>}
      {open && count > 0 && (
        <ul className="articles">
          {research?.articles.map((article) => (
            <li key={article.url}>
              <a href={article.url} target="_blank" rel="noreferrer">{article.title || article.url}</a>
              <span className="faint">
                {article.outlet}
                {article.published ? ` · ${article.published.slice(0, 10)}` : ""}
              </span>
              <p>{article.summary}</p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Builder({
  ready,
  started,
  stage,
  kind: saved,
  compilation,
  onBuild,
  onCancel,
  rebuild = false,
}: {
  ready: boolean;
  started: number | null;
  stage: string;
  kind: Kind;
  compilation: boolean;
  onBuild: (note: string, kind: Kind) => Promise<boolean>;
  onCancel?: () => void;
  rebuild?: boolean;
}) {
  const [note, setNote] = useState("");
  const [kind, setKind] = useState<Kind>(saved);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (started === null) return;
    setNow(Date.now());
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(tick);
  }, [started]);

  if (started !== null) {
    return (
      <div className="builder busy">
        <div className="thinking">
          <span className="dot live" />
          <span>{stage && stage !== "Writing the script" ? `${stage}…` : "Opus is writing the script"}</span>
          <span className="mono faint">{clock(Math.max(0, now - started))}</span>
        </div>
        <span className="faint">Long footage takes 5 to 15 minutes. You can keep reading the transcript meanwhile.</span>
      </div>
    );
  }
  return (
    <div className="builder">
      {rebuild && <strong>Rebuild the script</strong>}
      <div className="row">
        <Tabs value={kind} onChange={setKind} options={KINDS} />
      </div>
      {compilation && <p className="hint">A compilation: each video is its own case, told in this format.</p>}
      <textarea
        value={note}
        rows={2}
        placeholder="Optional notes for Claude, e.g. aim for 30 minutes, open on the confession"
        onChange={(event) => setNote(event.target.value)}
      />
      <div className="row">
        <span className="faint">
          {!ready
            ? "Transcribe the footage first."
            : rebuild
              ? "This replaces the current clips and narration."
              : "Runs on Opus through your Claude login."}
        </span>
        <span className="spacer" />
        {onCancel && <button type="button" className="ghost sm" onClick={onCancel}>Cancel</button>}
        <button
          type="button"
          className={rebuild ? "sm danger" : "primary"}
          disabled={!ready}
          onClick={() => void onBuild(note, kind).then((done) => done && setNote(""))}
        >
          {rebuild ? "Replace script" : "Build script"}
        </button>
      </div>
    </div>
  );
}
