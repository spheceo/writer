import { expect, test } from "bun:test";
import { keptMs, lift, splitAt } from "../src/clips";
import type { Clip } from "../src/types";

const whole: Clip[] = [{ id: "a", startMs: 0, endMs: 10_000 }];

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
