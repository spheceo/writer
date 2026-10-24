import { expect, test } from "bun:test";
import { at, blurSpans, boundary, commentaryContent, footageContent, scriptText } from "../src/doc-export";
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
  expect(content.text).toBe("INTERRO The Case\n\nAssets:\n-\tFootage\n\nTotal Duration: 0m4s\n\n(0:00‑0:02)\n\nHe was charged with murder.\n\n");
  const links = content.styles.filter((style) => style.url).map((style) => style.url);
  expect(links).toEqual(["https://youtu.be/abc", "https://youtu.be/abc?t=0"]);
  const notes = content.notes.map((note) => [content.text.slice(note.start - 1, note.end - 1), note.text]);
  expect(notes).toEqual([
    ["(0:00‑0:02)", "did...it"],
    ["murder", "Note to editor: Blur this"],
  ]);
  expect(scriptText(script)).toContain("-\tFootage https://youtu.be/abc\n");
});

test("finds a clip's first and last words as Repack notes them", () => {
  expect(boundary({ startMs: 1_250, endMs: 1_500 }, words)).toBe("you...do");
  expect(boundary({ startMs: 1_150, endMs: 1_700 }, words, { w3: "IT!" })).toBe("did...it");
  expect(boundary({ startMs: 5_000, endMs: 6_000 }, words)).toBe("...");
});

test("links a compilation's ranges to their own videos and divides its cases", () => {
  const compilation = {
    ...script,
    title: "It Went Wrong",
    sourceKind: "compilation",
    sourceUrl: "https://youtu.be/a\nhttps://drive.google.com/file/d/B/view",
    compilation: true,
    parts: [
      { title: "A", startMs: 0, durationMs: 60_000, audio: false, url: "https://youtu.be/a" },
      { title: "B", startMs: 60_000, durationMs: 60_000, audio: false, url: "https://drive.google.com/file/d/B/view" },
    ],
    clips: [
      { id: "c1", startMs: 1_000, endMs: 5_000 },
      { id: "n1", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "Hook." },
      { id: "n2", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "Next." },
      { id: "c2", startMs: 70_000, endMs: 75_500 },
      { id: "n3", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "End." },
    ],
  } as unknown as Script;
  const content = footageContent(compilation);
  expect(content.text).toStartWith("INTERRO It Went Wrong\n\nAssets:\n-\tFootage 1\n-\tFootage 2\n\nTotal Duration: 0m19s\n\n(0:01‑0:05)\n\nHook.\n\n \nNext.\n\n(0:10‑0:15)\n\nEnd.");
  const links = content.styles.filter((style) => style.url).map((style) => style.url);
  expect(links).toEqual(["https://youtu.be/a", "https://drive.google.com/file/d/B/view", "https://youtu.be/a?t=1", "https://drive.google.com/file/d/B/view?t=10"]);
  expect(content.styles.filter((style) => style.divider)).toHaveLength(1);
});

test("runs a Courtroom case on in one paragraph and marks audio recordings", () => {
  const court = {
    ...script,
    kind: "courtroom",
    sourceKind: "drive",
    sourceUrl: "https://drive.google.com/drive/folders/F",
    parts: [
      { title: "Hearing", startMs: 0, durationMs: 60_000, audio: false, url: "https://drive.google.com/file/d/H/view" },
      { title: "Call", startMs: 60_000, durationMs: 60_000, audio: true, url: "https://drive.google.com/file/d/C/view" },
    ],
    clips: [
      { id: "c1", startMs: 1_000, endMs: 5_000 },
      { id: "n1", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "The promise.\n\nThe backstory." },
      { id: "c2", startMs: 61_000, endMs: 64_000 },
      { id: "n2", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "Then." },
    ],
  } as unknown as Script;
  const text = footageContent(court).text;
  expect(text).toContain("-\tFootage\n");
  expect(text).toContain("(0:01‑0:05)\n\nThe promise.\n\nThe backstory. (Audio 0:01‑0:04) Then.\n");
  expect(text).toEndWith("\nTotal length of clips: 0m7s\n");
});

test("footage from this Mac lists no assets and links nothing", () => {
  const local = { ...script, sourceKind: "file", sourceUrl: "/Users/me/case.mp4" } as unknown as Script;
  const content = footageContent(local);
  expect(content.text).not.toContain("Assets:");
  expect(content.styles.some((style) => style.url)).toBe(false);
});

test("a rewritten heading heads the Doc", () => {
  expect(footageContent({ ...script, heading: "INTERRO John Doe - The Case" } as Script).text).toStartWith("INTERRO John Doe - The Case\n");
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

test("puts the producer's comments on a timestamp into the Doc there", () => {
  const noted = {
    ...script,
    clips: [{ ...script.clips[0], notes: [{ id: "n", text: "Blur the plate.", at: 0 }] }, script.clips[1]],
  } as Script;
  const content = footageContent(noted);
  const on = content.notes.filter((note) => content.text.slice(note.start - 1, note.end - 1) === "(0:00‑0:02)").map((note) => note.text);
  expect(on).toEqual(["did...it", "Blur the plate."]);
});

test("a reused case's transition carries its link as a comment, and REUSE when nothing is written", () => {
  const reused = {
    ...script,
    compilation: true,
    title: "Cops Who Got It Wrong",
    clips: [
      script.clips[0],
      { id: "n1", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "Intro." },
      { id: "r", startMs: 0, endMs: 3_000, kind: "cut-in", reuse: "https://youtu.be/old" },
    ],
  } as unknown as Script;
  const content = footageContent(reused);
  expect(content.text).toContain("Intro.\n\n \nREUSE\n");
  const notes = content.notes.map((note) => [content.text.slice(note.start - 1, note.end - 1), note.text]);
  expect(notes).toContainEqual(["REUSE", "Reuse: https://youtu.be/old"]);
});

test("a Courtroom cut-in with no footage before it still runs on after a space", () => {
  const court = {
    ...script,
    kind: "courtroom",
    clips: [
      script.clips[0],
      { id: "n1", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "The promise.\n\nThe backstory." },
      { id: "n2", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "Then." },
    ],
  } as unknown as Script;
  expect(footageContent(court).text).toContain("The backstory. Then.");
});

test("a reused case's footage opens the video linked to its own video, never listed as an asset", () => {
  const old = "https://youtu.be/old";
  const compilation = {
    ...script,
    title: "When Liars Get Caught",
    sourceKind: "compilation",
    sourceUrl: "https://youtu.be/a\nhttps://youtu.be/b",
    compilation: true,
    reuses: [{ url: old, title: "Earlier", after: 1 }],
    parts: [
      { title: "A", startMs: 0, durationMs: 60_000, audio: false, url: "https://youtu.be/a" },
      { title: "B", startMs: 60_000, durationMs: 60_000, audio: false, url: "https://youtu.be/b" },
      { title: "Earlier", startMs: 120_000, durationMs: 60_000, audio: false, url: old, reuse: old },
    ],
    clips: [
      { id: "r1", startMs: 125_000, endMs: 128_000 },
      { id: "n1", startMs: 0, endMs: 3_000, kind: "cut-in", narration: "Hook." },
    ],
  } as unknown as Script;
  const content = footageContent(compilation);
  expect(content.text).toContain("Assets:\n-\tFootage 1\n-\tFootage 2\n\n");
  expect(content.text).toContain("(0:05‑0:08)");
  expect(content.styles.filter((style) => style.url).map((style) => style.url)).toContain(`${old}?t=5`);
});

test("a commentary note sits on the words it's about, and a quote pop-up goes to Edit as an edit: note", () => {
  const said = { source: "S1", video: "Stream", startMs: 130_000, endMs: 134_000, quote: "I want a Lambo." };
  const draft: Draft = {
    open: [],
    sections: [
      {
        heading: "Intro",
        paragraphs: [
          {
            pieces: [{ kind: "say", text: "  He wanted a car. He said it himself: I want a Lambo. Then he went quiet." }],
            note: "Show: the dealership photo.\nQuote pop-up (typewriter): “I want a Lambo.” (Him, Stream 2:10-2:14)\nTense music from here.",
            marks: [
              { line: "Show: the dealership photo.", on: "He wanted a car." },
              { line: "Quote pop-up (typewriter): “I want a Lambo.” (Him, Stream 2:10-2:14)", on: "I want a Lambo.", quote: { who: "Him", said } },
            ],
          },
        ],
      },
    ],
    checks: [],
    report: "",
    writtenAt: 0,
  };
  const content = commentaryContent("Title", draft, []);
  const notes = content.notes.map((note) => [content.text.slice(note.start - 1, note.end - 1), note.text]);
  expect(notes).toContainEqual(["He wanted a car.", "For editor: Show: the dealership photo."]);
  expect(notes).toContainEqual(["I want a Lambo.", "edit: Quote pop-up (typewriter): “I want a Lambo.” (Him, Stream 2:10-2:14)"]);
  // The line on no words stays on the paragraph's first words.
  expect(notes.find(([, text]) => text.includes("Tense music"))?.[1]).toBe("For editor: Tense music from here.");
});
