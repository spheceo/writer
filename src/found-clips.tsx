import { useState } from "react";
import { framesUrl } from "./api";
import { clock } from "./clips";
import type { Finds, Found, FoundVideo, Need } from "./types";
import { Icon } from "./ui";

const LOOKS = { yes: "Frames match", no: "Frames don't match", unsure: "Frames unclear" } as const;

/** A YouTube link that opens at the clip's first second. */
const at = (video: string, ms: number) => `https://www.youtube.com/watch?v=${video}&t=${Math.floor(ms / 1000)}s`;

/**
 * The clips a search of YouTube found for the brief, by the moment each one
 * serves: the video and times, the words as the captions have them, why it
 * fits, and for a moment that's about what's on screen, what its frames show.
 */
export function FoundClips({
  id,
  finds,
  busy,
  canFind,
  onFind,
}: {
  id: string;
  finds: Finds | null;
  busy: boolean;
  canFind: boolean;
  onFind: () => void;
}) {
  const [problemsOpen, setProblemsOpen] = useState(false);
  const videos = new Map((finds?.videos ?? []).map((video) => [video.id, video]));
  const chapters: { name: string; needs: Need[] }[] = [];
  for (const need of finds?.needs ?? []) {
    const last = chapters[chapters.length - 1];
    if (last && last.name === need.chapter) last.needs.push(need);
    else chapters.push({ name: need.chapter, needs: [need] });
  }
  const covered = new Set(finds?.clips.map((clip) => clip.need));
  const read = finds?.videos.filter((video) => !video.problem).length ?? 0;
  return (
    <div className="sources found-clips">
      <div className="found-head">
        {finds ? (
          <p className="faint">
            {finds.clips.length} clips for {covered.size} of {finds.needs.length} moments · {read} videos read · {finds.searches} searches
            {finds.costUsd > 0 ? ` · $${finds.costUsd.toFixed(2)}` : ""}
          </p>
        ) : (
          <p className="faint">Search YouTube for the clips the brief asks for.</p>
        )}
        <button type="button" className="sm" disabled={busy || !canFind} title={canFind ? "Plan searches from the brief, read the likely videos' captions, and pick the moments" : "Add the producer's brief first"} onClick={onFind}>
          <Icon name="search" size={13} />
          {finds ? "Search again" : "Find clips"}
        </button>
      </div>
      {!finds && (
        <div className="empty">
          <p>
            Opus turns the brief into the moments the video needs. Writer searches YouTube for each one and reads the likely videos'
            captions, and Sonnet picks the moments. Every quote is checked against the captions, and where a moment is about what's on
            screen, Sonnet looks at its frames.
          </p>
          <p className="hint">Each video with a clip is added as a source, so the next build can play it. A search takes 10 to 20 minutes.</p>
        </div>
      )}
      {chapters.map((chapter) => (
        <section key={chapter.name} className="found-chapter">
          <h3>{chapter.name || "Other"}</h3>
          {chapter.needs.map((need) => {
            const clips = finds?.clips.filter((clip) => clip.need === need.id) ?? [];
            return (
              <div key={need.id} className="found-need">
                <p className="found-what">
                  <span className="badge mono">{need.id}</span>
                  <span>{need.what}</span>
                </p>
                {clips.length === 0 ? (
                  <p className="faint found-none" title={`Searched: ${need.queries.join(" · ")}`}>Not found on YouTube</p>
                ) : (
                  <ul>
                    {clips.map((clip, index) => (
                      <Clip key={index} id={id} finds={finds!} clip={clip} video={videos.get(clip.video)} />
                    ))}
                  </ul>
                )}
              </div>
            );
          })}
        </section>
      ))}
      {finds && finds.problems.length > 0 && (
        <div className="found-problems">
          <button type="button" className="link-button" aria-expanded={problemsOpen} onClick={() => setProblemsOpen(!problemsOpen)}>
            {finds.problems.length} {finds.problems.length === 1 ? "thing" : "things"} went wrong
          </button>
          {problemsOpen && (
            <ul>
              {finds.problems.map((problem, index) => (
                <li key={index} className="faint">{problem}</li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

function Clip({ id, finds, clip, video }: { id: string; finds: Finds; clip: Found; video?: FoundVideo }) {
  return (
    <li className="found-clip" data-strength={clip.strength}>
      <div className="found-line">
        <a href={at(clip.video, clip.startMs)} target="_blank" rel="noreferrer" title={video?.about || video?.title}>
          <span className="mono">{clock(clip.startMs)}–{clock(clip.endMs)}</span> {video?.title ?? clip.video}
        </a>
        <span className={`chip ${clip.strength}`} title={clip.strength === "strong" ? "The moment itself" : "Related, weaker or second-hand"}>
          {clip.strength === "strong" ? "Strong" : "Partial"}
        </span>
      </div>
      {video && (
        <p className="faint found-channel">
          {[video.channel, video.heard ? "transcribed by Writer" : "", video.source ? `source ${video.source}` : ""].filter(Boolean).join(" · ")}
        </p>
      )}
      {clip.quote && <blockquote>"{clip.quote}"</blockquote>}
      <p className="found-why">{clip.why}</p>
      {clip.look && (
        <div className="found-look" data-shows={clip.look.shows}>
          <a href={framesUrl(id, finds, clip.look.frames)} target="_blank" rel="noreferrer" title="Open the frames">
            <img src={framesUrl(id, finds, clip.look.frames)} alt={`Frames from ${video?.title ?? "the clip"}`} />
          </a>
          <p>
            <strong>{LOOKS[clip.look.shows]}.</strong> {clip.look.seen}
          </p>
        </div>
      )}
    </li>
  );
}
