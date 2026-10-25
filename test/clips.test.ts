import { expect, test } from "bun:test";
import { breathe, insertAt, keptMs, lift, moveItems, recordingSpans, splitAt } from "../src/clips";
import type { Clip } from "../src/types";

const whole: Clip[] = [{ id: "a", startMs: 0, endMs: 10_000 }];

test("recording boundaries follow trimmed and reordered footage without changing its edits", () => {
  const parts = [
    { title: "First", startMs: 0, durationMs: 5_000, audio: false },
    { title: "Second", startMs: 5_000, durationMs: 5_000, audio: false },
  ];
  expect(recordingSpans({ id: "trimmed", startMs: 3_000, endMs: 8_000 }, parts)).toEqual([
    { index: 0, title: "First", startMs: 3_000, endMs: 5_000 },
    { index: 1, title: "Second", startMs: 5_000, endMs: 8_000 },
  ]);
  expect(recordingSpans({ id: "later", startMs: 6_000, endMs: 10_000 }, parts).map((part) => part.index)).toEqual([1]);
  expect(recordingSpans({ ...whole[0], kind: "cut-in" }, parts)).toEqual([]);
});

test("split keeps both halves", () => {
  const next = splitAt(whole, 4_000);
  expect(next).toHaveLength(2);
  expect(next[0].endMs).toBe(4_000);
  expect(next[1].startMs).toBe(4_000);
  expect(next[1].endMs).toBe(10_000);
  expect(keptMs(next)).toBe(10_000);
});

test("split ignores an edge", () => {
  expect(splitAt(whole, 50)).toEqual(whole);
});

test("lift removes the middle", () => {
  const next = lift(whole, 2_000, 5_000);
  expect(next.map((clip) => [clip.startMs, clip.endMs])).toEqual([
    [0, 2_000],
    [5_000, 10_000],
  ]);
  expect(keptMs(next)).toBe(7_000);
});

test("lift can drop a clip", () => {
  expect(lift(whole, 0, 10_000)).toEqual([]);
});

test("moves items to an edit point, keeping their order", () => {
  const items = ["a", "b", "c", "d"].map((id) => ({ id, startMs: 0, endMs: 1000 }));
  const ids = (list: { id: string }[]) => list.map((item) => item.id).join("");
  expect(ids(moveItems(items, ["d"], 0))).toBe("dabc");
  expect(ids(moveItems(items, ["a", "c"], 4))).toBe("bdac");
  expect(ids(moveItems(items, ["b", "c"], 1))).toBe("abcd");
  expect(moveItems(items, ["b"], 2)).toBe(items);
});

test("inserts at the playhead as an insert edit, splitting the clip there", () => {
  const items = [{ id: "a", startMs: 0, endMs: 10_000 }];
  const next = insertAt(items, 4_000, { id: "x", startMs: 50_000, endMs: 52_000 });
  expect(next.map((item) => [item.startMs, item.endMs])).toEqual([[0, 4_000], [50_000, 52_000], [4_000, 10_000]]);
  expect(insertAt(items, 0, { id: "x", startMs: 1, endMs: 2 })[0].id).toBe("x");
});

test("gives words breathing room that stops short of their neighbours", () => {
  const said = [
    { id: "w0", text: "a", startMs: 0, endMs: 500 },
    { id: "w1", text: "b", startMs: 600, endMs: 900 },
    { id: "w2", text: "c", startMs: 1_200, endMs: 1_500 },
  ];
  expect(breathe(said, 600, 900, 5_000)).toEqual({ startMs: 560, endMs: 1_140 });
});
