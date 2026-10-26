import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { isCutIn } from "./clips";
import { type CutPlayer, clipStarts, editedPosition, type Playhead, usePlayheadValue, usePlaying } from "./playback";
import { captionAt, captionCues } from "./captions";
import type { Clip, Word } from "./types";
import { Icon } from "./ui";

/** A timecode as an editor reads one: hours, minutes, seconds and frames at 30 fps. */
export function timecode(ms: number) {
  const frames = Math.floor((Math.max(0, ms) / 1000) * 30);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(Math.floor(frames / 108000))}:${pad(Math.floor(frames / 1800) % 60)}:${pad(Math.floor(frames / 30) % 60)}:${pad(frames % 30)}`;
}

function PlayheadTimecode({ playhead }: { playhead: Playhead }) {
  const ms = usePlayheadValue(playhead, (p) => Math.floor((p.auditioning ? p.sourceMs : p.ms) / 33.34));
  return <span className="timecode">{timecode(ms * 33.34)}</span>;
}

/** Repack's source-timed cues: no subtitles over cut-ins or source audition. */
function Caption({ clips, cues, corrections, playhead, frameRatio }: { frameRatio: number; clips: Clip[]; cues: Word[][]; corrections: Record<string, string>; playhead: Playhead }) {
  const caption = usePlayheadValue(playhead, (p) => {
    const at = editedPosition(clips, p.ms);
    return !p.auditioning && at && !isCutIn(at.clip) ? captionAt(cues, p.sourceMs, at.clip, corrections) : null;
  });
  return caption ? <div className="caption-frame" style={{ ["--ratio" as string]: frameRatio }}><p className="caption">{caption}</p></div> : null;
}

/** Split's seek bar: the cut end to end, cut-ins marked, the playhead on it. */
function SeekBar({ clips, playhead, player }: { clips: Clip[]; playhead: Playhead; player: CutPlayer | null }) {
  const bar = useRef<HTMLDivElement>(null);
  const fill = useRef<HTMLSpanElement>(null);
  const starts = clipStarts(clips);
  const total = starts[starts.length - 1] || 1;
  useEffect(() => {
    const paint = () => {
      if (fill.current) fill.current.style.width = `${(Math.min(playhead.ms, total) / total) * 100}%`;
    };
    paint();
    return playhead.subscribe(paint);
  }, [playhead, total]);
  const seekTo = (clientX: number) => {
    const box = bar.current?.getBoundingClientRect();
    if (box) player?.seek(((clientX - box.left) / box.width) * total);
  };
  return (
    <div
      ref={bar}
      className="seek-bar"
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        seekTo(event.clientX);
      }}
      onPointerMove={(event) => event.buttons === 1 && seekTo(event.clientX)}
    >
      <span className="seek-track">
        <span ref={fill} className="seek-fill" />
        {clips.map((clip, index) =>
          isCutIn(clip) ? <span key={clip.id} className="seek-mark" style={{ left: `${(starts[index] / total) * 100}%` }} /> : null,
        )}
      </span>
    </div>
  );
}

/**
 * The program monitor: the cut playing, each cut-in as a card over the held
 * frame, and Repack's transport under it.
 */
export function Monitor({
  src,
  clips,
  labels,
  playhead,
  player,
  seekBar,
  expanded,
  onExpand,
  onVideo,
  empty,
  multi = false,
  describe,
  words,
  corrections,
}: {
  src: string;
  words?: Word[];
  corrections?: Record<string, string>;
  /** The cut plays from several recordings, which the player loads itself. */
  multi?: boolean;
  /** What a card says, where the cut's own cards are not cut-ins. */
  describe?: (clip: Clip) => { title: string; text: string } | null;
  clips: Clip[];
  labels: string[];
  playhead: Playhead;
  player: CutPlayer | null;
  seekBar: boolean;
  expanded?: boolean;
  onExpand?: (expanded: boolean) => void;
  onVideo: (video: HTMLVideoElement | null) => void;
  empty?: ReactNode;
}) {
  const cues = useMemo(() => captionCues(words ?? []), [words]);
  const [captions, setCaptions] = useState(true);
  const playing = usePlaying(playhead);
  const auditioning = usePlayheadValue(playhead, (p) => p.auditioning);
  const showing = usePlayheadValue(playhead, (p) => (p.auditioning ? null : (editedPosition(clips, p.ms)?.clip ?? null)));
  const [volume, setVolume] = useState(() => Number(window.localStorage.getItem("writer.volume") ?? "1"));
  const [rate, setRate] = useState(1);
  const [frameRatio, setFrameRatio] = useState(16 / 9);
  const measureVideo = (event: React.SyntheticEvent<HTMLVideoElement>) => {
    const video = event.currentTarget;
    if (video.videoWidth && video.videoHeight) setFrameRatio(video.videoWidth / video.videoHeight);
  };
  const before = useRef(1);
  const total = clipStarts(clips)[clips.length] ?? 0;
  const card = showing && isCutIn(showing) ? showing : null;
  const cueNumber = card ? clips.filter(isCutIn).findIndex((item) => item.id === card.id) : -1;

  useEffect(() => {
    player?.setVolume(volume);
    window.localStorage.setItem("writer.volume", String(volume));
  }, [player, volume]);
  useEffect(() => player?.setRate(rate), [player, rate]);

  return (
    <section className="panel monitor" aria-label="Program monitor">
      <div className="screen">
        {multi ? (
          <video onLoadedMetadata={measureVideo} ref={onVideo} playsInline preload="auto" aria-label="Script preview" onClick={() => player?.toggle()} />
        ) : src ? (
          <video onLoadedMetadata={measureVideo} ref={onVideo} src={src} playsInline preload="auto" aria-label="Edited video preview" onClick={() => player?.toggle()} />
        ) : (
          <div className="none">{empty ?? "No preview yet."}</div>
        )}
        {card && describe?.(card) ? (
          <div className="cut-in-card described">
            <strong>{describe(card)?.title}</strong>
            <p>{describe(card)?.text}</p>
          </div>
        ) : card && (
          <div className="cut-in-card">
            <strong>
              cut in #{cueNumber + 1}
              {labels[cueNumber] ? <span> · {labels[cueNumber]}</span> : null}
            </strong>
            {card.narration?.trim() ? <p>{card.narration.trim()}</p> : <p className="faint">No narration yet.</p>}
          </div>
        )}
        {captions && words && <Caption clips={clips} cues={cues} corrections={corrections ?? {}} playhead={playhead} frameRatio={frameRatio} />}
        {auditioning && <span className="screen-badge">Source</span>}
      </div>
      {seekBar && <SeekBar clips={clips} playhead={playhead} player={player} />}
      <div className="transport">
        <PlayheadTimecode playhead={playhead} />
        <div className="controls">
          <button type="button" className="icon-tool" aria-label="Go to start" title="Go to start (Home)" onClick={() => player?.seek(0)}>
            <Icon name="start" size={15} />
          </button>
          <button type="button" className="icon-tool skip" aria-label="Back 10 seconds" title="Back 10 seconds (J)" onClick={() => player?.seek(playhead.ms - 10000)}>
            <Icon name="undo" size={18} />
            <span aria-hidden="true">10</span>
          </button>
          <button type="button" className="play" disabled={!clips.length && !src} aria-label={playing ? "Pause preview" : "Play preview"} title="Play / pause (Space)" onClick={() => player?.toggle()}>
            <Icon name={playing ? "pause" : "play"} size={15} />
          </button>
          <button type="button" className="icon-tool skip" aria-label="Forward 10 seconds" title="Forward 10 seconds (L)" onClick={() => player?.seek(playhead.ms + 10000)}>
            <Icon name="redo" size={18} />
            <span aria-hidden="true">10</span>
          </button>
          <button type="button" className="icon-tool" aria-label="Go to end" title="Go to end (End)" onClick={() => player?.seek(total)}>
            <Icon name="end" size={15} />
          </button>
        </div>
        <div className="transport-end">
          {words && <button type="button" className="icon-tool" aria-pressed={captions} aria-label={captions ? "Hide subtitles" : "Show subtitles"} title={captions ? "Hide subtitles" : "Show subtitles"} onClick={() => setCaptions(!captions)}>CC</button>}

          <button
            type="button"
            className="icon-tool"
            aria-label={volume ? "Mute" : "Unmute"}
            title={volume ? "Mute" : "Unmute"}
            onClick={() => {
              if (volume) before.current = volume;
              setVolume(volume ? 0 : before.current || 1);
            }}
          >
            <Icon name={volume ? "volume" : "mute"} size={15} />
          </button>
          <button
            type="button"
            className="icon-tool rate"
            aria-pressed={rate !== 1}
            aria-label={`Playback speed ${rate}×`}
            title={rate === 1 ? "Play at 2× speed" : "Play at normal speed"}
            onClick={() => setRate(rate === 1 ? 2 : 1)}
          >
            {rate}×
          </button>
          {onExpand && (
            <button
              type="button"
              className="icon-tool"
              aria-pressed={expanded}
              aria-label={expanded ? "Exit full view" : "Full view"}
              title={`${expanded ? "Exit full view" : "Full view"} (F)`}
              onClick={() => onExpand(!expanded)}
            >
              <Icon name={expanded ? "collapse" : "expand"} size={14} />
            </button>
          )}
          <span className="timecode total">{timecode(total)}</span>
        </div>
      </div>
    </section>
  );
}
