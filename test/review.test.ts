import { expect, test } from "bun:test";
import { bandsOf, pieceBands } from "../src/review";
import type { Paragraph, ReviewNote } from "../src/types";

// The same paragraph as blocks.rs's test: "He lied.\n[Stream 1:05–1:10] \"no\"\nThen this one."
const paragraph: Paragraph = {
  id: "p1",
  pieces: [
    { kind: "say", text: "He lied." },
    { kind: "clip", source: "S1", video: "Stream", startMs: 65_400, endMs: 70_900, quote: "no" },
    { kind: "say", text: "Then this one." },
  ],
};

test("a band on a paragraph lands in the piece it covers, in that piece's offsets", () => {
  // The last piece starts at 8 + 1 + 23 + 1 = 33; "this" is 5..9 in it.
  const bands = pieceBands(paragraph, [{ start: 38, end: 42, notes: ["n1"], kind: "fix" }]);
  expect([...bands.keys()]).toEqual([2]);
  expect(bands.get(2)?.[0]).toMatchObject({ start: 5, end: 9 });
});

test("a band over a clip's line marks the clip", () => {
  const bands = pieceBands(paragraph, [{ start: 12, end: 15, notes: ["n1"], kind: "fix" }]);
  expect([...bands.keys()]).toEqual([1]);
});

test("done notes band what changed; open ones their words; replies nothing", () => {
  const note = (over: Partial<ReviewNote>): ReviewNote => ({
    id: "n",
    reviewId: "r",
    version: 1,
    anchor: { kind: "text", blockId: "p1", start: 0, end: 2, quote: "He" },
    text: "Cut",
    author: "Ana",
    status: "open",
    changedRanges: [],
    attempted: false,
    createdAt: 0,
    updatedAt: 0,
    ...over,
  });
  const bands = bandsOf([
    note({ id: "a" }),
    note({ id: "b", status: "done", changedRanges: [{ blockId: "p2", start: 3, end: 8 }] }),
    note({ id: "c", parentId: "a" }),
  ]);
  expect(bands.get("p1")).toEqual([{ start: 0, end: 2, notes: ["a"], kind: "open" }]);
  expect(bands.get("p2")).toEqual([{ start: 3, end: 8, notes: ["b"], kind: "fix" }]);
});
