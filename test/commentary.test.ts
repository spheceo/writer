import { expect, test } from "bun:test";
import { commentaryDocument, timing, totalLine } from "../src/commentary";
import type { Draft, Source } from "../src/types";

const clip = { source: "S2", video: "Exams Be Like (uncut)", link: "https://archive.org/x", startMs: 6_000, endMs: 19_000, quote: "Exams are like judgment day" };

const draft: Draft = {
  open: [{ ...clip, endMs: 9_000 }],
  sections: [
    { heading: "Intro", paragraphs: [{ pieces: [{ kind: "say", text: "He cut it." }] }] },
    {
      heading: "1. The Cuts",
      paragraphs: [
        {
          pieces: [{ kind: "say", text: "Here's the line…" }, { kind: "clip", ...clip, verify: "Check the voice" }, { kind: "say", text: "Gone." }],
          note: "Show both versions side by side.",
        },
      ],
    },
    { heading: "Ending", paragraphs: [{ pieces: [{ kind: "say", text: "So why?" }] }] },
  ],
  checks: ["Confirm the 2024 date."],
  report: "",
  writtenAt: 0,
};

const sources: Source[] = [
  { id: "S2", title: "Cuts report", role: "evidence", origin: "https://drive.google.com/file/d/abc/view", chars: 10, timed: false, preview: "", addedAt: 0 },
  { id: "S3", title: "Old script", role: "style", origin: "pasted", chars: 10, timed: false, preview: "", addedAt: 0 },
];

test("lays the script out as the channel's Doc", () => {
  const text = commentaryDocument("MrBeast Is Erasing His Past", draft, sources);
  expect(text).toStartWith("Intro\n(0:06-0:09)\nHe cut it.");
  expect(text).toContain("1. The Cuts\nHere's the line… (0:06-0:19) Gone.");
  expect(text).toContain("*For editor*: Show both versions side by side.");
  expect(text).toContain("*For editor*: (0:06-0:19) Exams Be Like (uncut): Check the voice");
  expect(text).not.toContain("Clips from\n");
  expect(text).not.toContain("Sources\n");
  expect(text).not.toContain("Old script");
  expect(text).not.toContain("Checks before recording\n");
});

test("times narration at 150 words a minute plus every clip", () => {
  const { narrationMs, clipsMs } = timing(draft);
  expect(narrationMs).toBe(Math.round((9 * 60_000) / 150));
  expect(clipsMs).toBe(3_000 + 13_000);
  expect(totalLine(draft)).toBe("Total time = 0min vo + 16s = 0min");
});
