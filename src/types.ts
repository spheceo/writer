export type Word = {
  id: string;
  text: string;
  startMs: number;
  endMs: number;
  speaker?: string | null;
};

/** A timeline item, in play order: a footage range, or a 3 s cut-in card holding narration. */
export type Clip = {
  id: string;
  startMs: number;
  endMs: number;
  kind?: "cut-in";
  narration?: string;
  narrationRole?: "intro" | "bridge" | "conclusion";
  /** On a compilation's transition into a reused case: the earlier video's link, a comment there, never narration. */
  reuse?: string;
  /** What the producer wrote on a footage clip's timestamp; each goes into the Doc as a comment there. */
  notes?: Note[];
};

export type Note = { id: string; text: string; at: number };

/** One recording of a joined Drive folder, on the joined clock. */
export type Part = {
  title: string;
  startMs: number;
  durationMs: number;
  audio: boolean;
  claimedMs?: number;
  /** The recording's YouTube or Drive link, which its timestamps link to. */
  url?: string;
  /** Footage of a compilation's reused case, joined after its own videos for intro clips: the reuse link. Never a case. */
  reuse?: string;
  /** A reused case's strongest short moments, picked once. */
  highlights?: Highlight[];
};

/** A short moment of a reused case, on the joined footage's clock. */
export type Highlight = { firstWordId: string; lastWordId: string; startMs: number; endMs: number; quote: string; why: string };

/** A question a paused import waits on, as Repack asks it. */
export type Decision =
  | { kind: "failed_recordings"; summary: string; failed: { id: string; title: string; error: string }[]; total: number }
  | {
      kind: "too_much";
      summary: string;
      files: { id: string; title: string; size: number; durationMs: number | null; audio: boolean; tooLong: boolean }[];
      maxFiles: number;
      maxMs: number;
    }
  | { kind: "no_speech"; summary: string; reason: "no_audio" | "no_words" };

export type Choice =
  | { kind: "retry" }
  | { kind: "skip_failed" }
  | { kind: "select"; ids: string[] }
  | { kind: "no_transcript" }
  | { kind: "cancel" };

export type Check = { name: string; ok: boolean; detail: string };

export type Script = {
  id: string;
  title: string;
  sourceKind: string;
  sourceUrl: string;
  originalPath: string;
  status: string;
  stage: string;
  progress: number;
  error: string | null;
  durationMs: number;
  words: Word[];
  clips: Clip[];
  overview: number[];
  parts?: Part[];
  decision?: Decision;
  noSpeech?: boolean;
  notes?: string[];
  kind?: Kind;
  compilation?: boolean;
  /** Who each voice is, "Name (Role)" or a role, by speaker ID. */
  speakers?: Record<string, string>;
  suspects?: string[];
  /** A commentary project's sources and written script. */
  commentary?: Commentary;
  /** The heading as the producer rewrote it, in place of the generated one. */
  heading?: string;
  /** Words the producer respelled, by word ID. */
  corrections?: Record<string, string>;
  /** A compilation's reused cases: each takes only its transition, carrying its link. */
  /** A reused case given as its earlier script's Doc carries the footage it lists and its narration. */
  reuses?: { url: string; title: string; after: number; footage?: string; narration?: string }[];
  /** Rewrites of saved narration waiting for the producer to accept or decline. */
  proposals?: Proposal[];
  /** The last full export to Google Docs. */
  docExport?: { id: string; url: string; revision: number; exportedAt: number };
  revision: number;
  createdAt: number;
  updatedAt: number;
  proxyPath: string | null;
};

export type Summary = {
  id: string;
  title: string;
  status: string;
  stage: string;
  progress: number;
  error: string | null;
  durationMs: number;
  updatedAt: number;
  wordCount: number;
  clipCount: number;
  cutInCount: number;
  building: boolean;
  sourceKind: string;
  decision?: Decision;
  /** A commentary script's narration words, once written. */
  draftWords: number;
  compilation: boolean;
  /** When the last full Google Doc export was made, and whether the script changed since. */
  docExportedAt?: number;
  docBehind: boolean;
  createdAt: number;
};

export type ImportProgress = {
  id: string;
  title: string;
  stage: string;
  percent: number;
  detail: string;
};

export type Settings = {
  relayUrl: string;
  relayToken: string;
};

export type ClaudeStatus = {
  installed: boolean;
  loggedIn: boolean;
  subscription: string | null;
  message: string;
};

/** Repack's formats. A compilation is any of them over several videos. */
export type Kind = "interrogation" | "foia" | "courtroom";

export type Article = {
  title: string;
  outlet: string;
  published: string | null;
  url: string;
  summary: string;
  facts: string[];
};

export type Research = { articles: Article[]; queries: string[]; at: number; problem?: string };

/** A script build. It runs on the Mac; the page only asks how it is going. */
export type BuildStatus = {
  running: boolean;
  stage: string;
  startedAt: number;
  finishedAt: number | null;
  report: string | null;
  costUsd: number | null;
  error: string | null;
};

/** What a commentary source is for: the producer's brief, evidence, or a past script for the house style. */
export type Role = "brief" | "evidence" | "style";

export type Source = {
  id: string;
  title: string;
  role: Role;
  /** A link, a path on this Mac, "pasted", or "writer:<script id>". */
  origin: string;
  chars: number;
  timed: boolean;
  preview: string;
  addedAt: number;
};

/** A moment in a source recording the script plays. */
export type Cite = {
  source: string;
  video: string;
  link?: string;
  startMs: number;
  endMs: number;
  quote: string;
  verify?: string;
};

export type Piece = { kind: "say"; text: string } | ({ kind: "clip" } & Cite);

export type Paragraph = { pieces: Piece[]; note?: string };

export type Section = { heading: string; paragraphs: Paragraph[] };

export type Draft = {
  open: Cite[];
  sections: Section[];
  checks: string[];
  report: string;
  writtenAt: number;
};

export type Commentary = { sources: Source[]; draft?: Draft };

export type Adding =
  | { kind: "link"; url: string }
  | { kind: "file"; path: string }
  | { kind: "text"; title: string; text: string }
  | { kind: "writer"; id: string };

/** A Google Doc exported from a script, as Drive has it now. */
export type DocFile = {
  id: string;
  url: string;
  name: string;
  draft: boolean;
  createdAt: string | null;
  state: "live" | "trashed" | "deleted";
};

/** A comment on an exported Doc, as Drive lists it. */
export type DocComment = {
  id: string;
  author?: { displayName?: string };
  content?: string;
  quotedFileContent?: { value?: string };
  resolved?: boolean;
  deleted?: boolean;
  createdTime?: string;
  replies?: { author?: { displayName?: string }; content?: string; deleted?: boolean }[];
};

/** A rewrite of saved narration Claude proposed, for the producer to accept or decline. */
export type Proposal = { id: string; cutIn: string; before: string; after: string; at: number };

/** One message in a footage script's conversation with the editor. */
export type ChatTurn = { role: "producer" | "editor"; text: string; at: number; costUsd?: number; failed?: boolean };

/** What the producer is looking at as they ask for a change. */
export type Focus = { selected: string[]; picked: [number, number] | null; playheadMs: number | null };

/** A saved version of a footage script: who made it and what changed. */
export type Version = { id: string; at: number; author: "you" | "claude" | "before"; summary: string; base?: string; restored?: number };

/** What a version holds. */
export type Snapshot = { clips: Clip[]; corrections: Record<string, string>; heading: string | null };

/** A compilation's line, checked before anything downloads. */
/** A card waiting on one of Repack's Trello lists, parsed for what a script needs. */
export type TrelloCard = {
  id: string;
  name: string;
  url: string;
  labels: string[];
  activity: string | null;
  kind: "case" | "compilation";
  footage: string | null;
  article: string | null;
  lines: string[];
  videos: number;
  reuses: number;
  problem: string | null;
  scripted: boolean;
};
export type TrelloList = "raw" | "comp";
export type TrelloCards = { list: string; cards: TrelloCard[] };
export type VideoCheck = { url: string; reuse: boolean; title: string; channel: string; durationMs: number | null; problem: string | null; warning: string | null };
