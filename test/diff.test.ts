import { expect, test } from "bun:test";
import { wordDiff } from "../src/diff";

test("marks the words a rewrite changes", () => {
  expect(wordDiff("He said he was home.", "He swore he was home all night.")).toEqual([
    { kind: "same", text: "He " },
    { kind: "del", text: "said " },
    { kind: "add", text: "swore " },
    { kind: "same", text: "he was " },
    { kind: "del", text: "home." },
    { kind: "add", text: "home all night." },
  ]);
  expect(wordDiff("Same.", "Same.")).toEqual([{ kind: "same", text: "Same." }]);
});
