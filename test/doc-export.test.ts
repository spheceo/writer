import { expect, test } from "bun:test";
import { at, blurSpans, boundary, commentaryContent, footageContent } from "../src/doc-export";
import { scriptDocx } from "../src/docx";
import type { Draft, Script } from "../src/types";

const words = [
  { id: "w0", text: "Did", startMs: 1_000, endMs: 1_200, speaker: "A" },
  { id: "w1", text: "you", startMs: 1_200, endMs: 1_400, speaker: "A" },
  { id: "w2", text: "do", startMs: 1_400, endMs: 1_600, speaker: "A" },
  { id: "w3", text: "it?", startMs: 1_600, endMs: 1_900, speaker: "A" },
];

const script = {
  id: "s",
  title: "The Case",
  sourceKind: "youtube",
  sourceUrl: "https://youtu.be/abc",
  words,
  clips: [
    { id: "c1", startMs: 900, endMs: 2_000 },
    { id: "n1", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "He was charged with murder." },
  ],
  kind: "interrogation",
} as unknown as Script;

test("lays a footage script out as Repack's Doc, a comment on each timestamp", () => {
  const content = footageContent(script);
  expect(content.text).toStartWith("INTERRO The Case\n\nTotal Duration: 0m04s\n\n(0:01‑0:02)\n\nHe was charged with murder.");
  const link = content.styles.find((style) => style.url);
  expect(link?.url).toBe("https://youtu.be/abc?t=0");
  const notes = content.notes.map((note) => [content.text.slice(note.start - 1, note.end - 1), note.text]);
  expect(notes).toEqual([
    ["(0:01‑0:02)", "Did...it?"],
    ["murder", "Note to editor: Blur this"],
  ]);
});

test("finds a clip's first and last words", () => {
  expect(boundary({ id: "c", startMs: 1_150, endMs: 1_700 }, words)).toBe("you...do");
  expect(boundary({ id: "c", startMs: 5_000, endMs: 6_000 }, words)).toBe("...");
});

test("links a YouTube moment and leaves other links alone", () => {
  expect(at("https://youtu.be/abc?t=36", 84_500)).toBe("https://youtu.be/abc?t=84");
  expect(at("https://archive.org/details/x", 84_000)).toBe("https://archive.org/details/x");
});

test("censors what the channel censors, only in commentary", () => {
  expect(blurSpans("He nearly died.")).toEqual([]);
  expect(blurSpans("He nearly died.", true)).toEqual([[10, 14]]);
});

test("exports the commentary script with short cues and editor comments", () => {
  const draft: Draft = {
    open: [],
    sections: [
      {
        heading: "Intro",
        paragraphs: [
          {
            pieces: [
              { kind: "say", text: "Watch this…" },
              { kind: "clip", source: "S2", video: "Exams (uncut)", link: "https://youtu.be/x", startMs: 6_000, endMs: 19_000, quote: "Exams are like judgment day", verify: "Check the voice" },
            ],
            note: "Show both versions.",
          },
        ],
      },
    ],
    checks: ["Confirm the date."],
    report: "",
    writtenAt: 0,
  };
  const content = commentaryContent("Title", draft, []);
  expect(content.text).toStartWith("Intro\nWatch this… (0:06-0:19)\n");
  expect(content.text).not.toContain("Checks before recording:");
  expect(content.text).not.toContain(" · Exams (uncut)");
  const texts = content.notes.map((note) => note.text);
  expect(texts).toContain("Exams (uncut)\n“Exams are like judgment day”\nVerify: Check the voice");
  expect(texts).toContain("For editor: Show both versions.");
  const bytes = scriptDocx(content, "GeniusScriptwriter10", new Date(0).toISOString());
  expect(bytes[0]).toBe(0x50);
  expect(bytes[1]).toBe(0x4b);
});
