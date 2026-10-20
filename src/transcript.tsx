import { memo, useEffect, useMemo, useRef, useState } from "react";
import { message, researchAgain } from "./api";
import { clock, covers, lines, partAt } from "./clips";
import type { CutPlayer, Playhead } from "./playback";
import type { Clip, Part, Research, Word } from "./types";
import { Icon } from "./ui";

const SPEAKERS = ["#8fbaf0", "#f3a48e", "#a9e091", "#d0aaf3", "#e6cb82", "#86d3d6"];

/** Each voice's name and colour, in the order they speak. */
export function useSpeakers(words: Word[], named?: Record<string, string>) {
  return useMemo(() => {
    const map = new Map<string, { name: string; colour: string }>();
    for (const word of words) {
      const key = word.speaker ?? "";
      if (!map.has(key)) map.set(key, { name: named?.[key] ?? `Speaker ${map.size + 1}`, colour: SPEAKERS[map.size % SPEAKERS.length] });
    }
    return map;
  }, [words, named]);
}

/** Words picked in the transcript: their footage, how many, whether the cut plays any, and the first and last. */
export type Picked = { startMs: number; endMs: number; count: number; inCut: boolean; first: Word; last: Word };

/**
 * The transcript, word by word: click a word to show its frame, double-click
 * to play the footage from it, shift-click to select a stretch. The word being
 * spoken is lit directly, without drawing the transcript again.
 */
export const Transcript = memo(function Transcript({
  words,
  clips,
  parts,
  speakers,
  playhead,
  player,
  noSpeech,
  corrections,
  onPick,
}: {
  words: Word[];
  clips: Clip[];
  /** Words the producer respelled, by word ID. */
  corrections?: Record<string, string>;
  parts?: Part[];
  speakers: Map<string, { name: string; colour: string }>;
  playhead: Playhead;
  player: CutPlayer | null;
  noSpeech?: boolean;
  onPick: (picked: Picked | null) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const groups = useMemo(() => lines(words), [words]);
  const [anchor, setAnchor] = useState<number | null>(null);
  const [focus, setFocus] = useState<number | null>(null);
  const [query, setQuery] = useState("");

  // One word picked counts too, as in Repack: it can be played, inserted or respelled.
  const range = anchor !== null && focus !== null && words[anchor] && words[focus] ? ([Math.min(anchor, focus), Math.max(anchor, focus)] as const) : null;
  useEffect(() => {
    if (!range) return onPick(null);
    const [lo, hi] = range;
    onPick({
      startMs: words[lo].startMs,
      endMs: words[hi].endMs,
      count: hi - lo + 1,
      inCut: words.slice(lo, hi + 1).some((word) => covers(clips, word.startMs)),
      first: words[lo],
      last: words[hi],
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [anchor, focus, words, clips]);

  // Escape lets a selection go.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setAnchor(null);
        setFocus(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // The spoken word, lit and kept in view while it plays.
  useEffect(() => {
    let lit: HTMLElement | null = null;
    const paint = () => {
      const ms = playhead.sourceMs;
      let lo = 0;
      let hi = words.length - 1;
      let found = -1;
      while (lo <= hi) {
        const mid = (lo + hi) >> 1;
        if (words[mid].endMs <= ms) lo = mid + 1;
        else if (words[mid].startMs > ms) hi = mid - 1;
        else {
          found = mid;
          break;
        }
      }
      const node = found >= 0 ? box.current?.querySelector<HTMLElement>(`[data-index="${found}"]`) : null;
      if (node === lit) return;
      lit?.classList.remove("on");
      lit = node ?? null;
      if (!node) return;
      node.classList.add("on");
      const scroller = box.current;
      if (!scroller || !playhead.playing) return;
      const top = node.offsetTop;
      if (top < scroller.scrollTop + 40 || top > scroller.scrollTop + scroller.clientHeight - 80) {
        scroller.scrollTo({ top: top - scroller.clientHeight / 3, behavior: "smooth" });
      }
    };
    paint();
    return playhead.subscribe(paint);
  }, [playhead, words]);

  const needle = query.trim().toLowerCase();
  const hits = useMemo(() => {
    if (needle.length < 2) return new Set<number>();
    const found = new Set<number>();
    words.forEach((word, index) => (corrections?.[word.id] ?? word.text).toLowerCase().includes(needle) && found.add(index));
    return found;
  }, [needle, words, corrections]);
  const jump = () => {
    const first = [...hits][0];
    const node = first !== undefined ? box.current?.querySelector<HTMLElement>(`[data-index="${first}"]`) : null;
    if (!node || !box.current) return;
    box.current.scrollTo({ top: node.offsetTop - box.current.clientHeight / 3 });
    player?.cue(words[first].startMs);
  };

  // Which words the cut plays, in one sweep rather than a search per word.
  const kept = useMemo(() => {
    const spans = clips.filter((clip) => clip.kind !== "cut-in").map((clip) => [clip.startMs, clip.endMs] as const).sort((a, b) => a[0] - b[0]);
    const out: boolean[] = [];
    let at = 0;
    let reach = -1;
    for (const word of words) {
      while (at < spans.length && spans[at][0] <= word.startMs) reach = Math.max(reach, spans[at++][1]);
      out.push(word.startMs < reach);
    }
    return out;
  }, [clips, words]);

  let index = -1;
  return (
    <>
      <div className="transcript-search">
        <Icon name="search" size={13} />
        <input
          value={query}
          placeholder="Search"
          aria-label="Search transcript"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && jump()}
        />
        {needle.length >= 2 && <span className="faint">{hits.size}</span>}
      </div>
      <div className="transcript" ref={box}>
        {groups.length === 0 && <div className="empty">{noSpeech ? "No words were heard in this footage." : "No transcript yet."}</div>}
        {groups.map((line, at) => {
          const who = speakers.get(line.speaker ?? "");
          const part = partAt(parts, line.startMs);
          const opens = part >= 0 && (at === 0 || partAt(parts, groups[at - 1].startMs) !== part);
          return (
            <div key={line.words[0].id}>
              {opens && parts && (
                <div className="recording">
                  {parts[part].reuse ? "Reused case, for intro clips" : `Recording ${part + 1}`} · {parts[part].title}
                  {parts[part].audio && <span>audio</span>}
                </div>
              )}
              <div className="line">
                <span className="who">
                  <b style={{ color: who?.colour }}>{who?.name ?? "Speaker"}</b>
                  <button type="button" className="mono" onClick={() => player?.cue(line.startMs)}>
                    {clock(line.startMs)}
                  </button>
                </span>
                <p>
                  {line.words.map((word) => {
                    index += 1;
                    const at = index;
                    const picked = range && at >= range[0] && at <= range[1];
                    return (
                      <button
                        type="button"
                        key={word.id}
                        data-index={at}
                        className={["word", picked ? "pick" : "", kept[at] ? "" : "out", hits.has(at) ? "hit" : ""].join(" ")}
                        onClick={(event) => {
                          if (event.shiftKey && anchor !== null) {
                            setFocus(at);
                            return;
                          }
                          setAnchor(at);
                          setFocus(at);
                          player?.cue(word.startMs);
                        }}
                        onDoubleClick={() => player?.audition(word.startMs)}
                      >
                        {corrections?.[word.id] ?? word.text}{" "}
                      </button>
                    );
                  })}
                </p>
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
});

/** What research found about the case, and a way to look again. */
export function ResearchList({ id, research, onChange }: { id: string; research: Research | null; onChange: (research: Research) => void }) {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const again = async () => {
    setBusy(true);
    setProblem(null);
    try {
      onChange(await researchAgain(id));
    } catch (cause) {
      setProblem(message(cause));
    } finally {
      setBusy(false);
    }
  };
  const count = research?.articles.length ?? 0;
  return (
    <div className="research-list">
      <div className="research-head">
        <span className="faint">{busy ? "Searching…" : count ? `${count} ${count === 1 ? "report" : "reports"} on the case` : (research?.problem ?? "Nothing found yet")}</span>
        <span className="spacer" />
        <button type="button" className="tool" disabled={busy} onClick={() => void again()}>
          Research again
        </button>
      </div>
      {problem && <p className="bad">{problem}</p>}
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
    </div>
  );
}
