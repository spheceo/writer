import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { Monitor } from "../src/monitor";
import { Playhead } from "../src/playback";
import type { Clip, Word } from "../src/types";

test("Split monitor shows synchronized corrected subtitles and hides them over cut-ins and audition", () => {
  const saved = globalThis.window;
  globalThis.window = { localStorage: { getItem: () => null } } as unknown as Window & typeof globalThis;
  try {
    const playhead = new Playhead();
    const clips: Clip[] = [{ id: "footage", startMs: 10000, endMs: 12000 }, { id: "card", kind: "cut-in", startMs: 0, endMs: 3000, narration: "Finished narration" }];
    const words: Word[] = [{ id: "word", startMs: 10000, endMs: 11000, text: "Original" }];
    const render = () => renderToStaticMarkup(<Monitor src="test.mp4" clips={clips} words={words} corrections={{ word: "Corrected subtitle" }} labels={[]} playhead={playhead} player={null} seekBar onVideo={() => {}} />);
    playhead.set(100, 10100, true, false);
    expect(render()).toContain('class="caption">Corrected subtitle');
    expect(render()).toContain('aria-label="Hide subtitles"');
    playhead.set(2100, 100, true, false);
    expect(render()).not.toContain('class="caption"');
    expect(render()).toContain("Finished narration");
    playhead.set(100, 10100, true, true);
    expect(render()).not.toContain('class="caption"');
  } finally {
    globalThis.window = saved;
  }
});
