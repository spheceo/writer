import { expect, test } from "bun:test";
import { captionAt, captionCues } from "../src/captions";
import { editedPosition } from "../src/playback";
import type { Word, Clip } from "../src/types";
const word = (text: string, startMs: number, speaker = "a"): Word => ({ id: text, text, startMs, endMs: startMs + 300, speaker });
test("captions break at sentences, pauses, speaker changes and eight words", () => {
  const words = [word("Hello.", 0), word("Next", 300), word("speaker", 600, "b"), word("pause", 2000, "b")];
  expect(captionCues(words).map(c => c.map(w => w.text))).toEqual([["Hello."], ["Next"], ["speaker"], ["pause"]]);
  expect(captionCues(Array.from({ length: 9 }, (_, i) => word(String(i), i * 300))).map(c => c.length)).toEqual([8, 1]);
});
test("captions follow source jumps, corrections and clip trims; disappear in gaps", () => {
  const cues = captionCues([word("cut", 10000), word("heard", 10300), word("word", 10600)]);
  const clips: Clip[] = [{ id: "one", startMs: 10300, endMs: 10600 }, { id: "two", startMs: 10000, endMs: 10300 }];
  const at = editedPosition(clips, 100)!;
  expect(at.sourceMs).toBe(10400);
  expect(captionAt(cues, at.sourceMs, at.clip, { heard: "corrected" })).toBe("corrected");
  const jump = editedPosition(clips, 400)!;
  expect(captionAt(cues, jump.sourceMs, jump.clip, {})).toBe("cut");
  expect(captionAt(cues, 9999, clips[0], {})).toBeNull();
  expect(captionAt(cues, 12000, clips[0], {})).toBeNull();
});
