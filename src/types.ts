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

/** Who started a script (none on scripts from before sign-in). */
export type Creator = { sub: string; name: string; email?: string; picture?: string | null };

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
  createdBy?: Creator;
  /** Who each voice is, "Name (Role)" or a role, by speaker ID. */
  speakers?: Record<string, string>;
  suspects?: string[];
  /** A commentary project's sources and written script. */
  commentary?: Commentary;
  /** Its review link on Repack, once shared. */
  review?: ReviewLink | null;
  /** Archived from Repack's page: its footage is gone until restored. */
  archive?: Archive;
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

/** A script archived from Repack's page to free space: footage deleted, the script and its words kept. */
export type Archive = { at: number; by: string; freedBytes: number; restoring: boolean; missing: string[] };

export type Summary = {
  archive?: Archive;
  createdBy?: Creator;
  /** Footage a commentary was written from: that commentary's title. The list keeps it inside it. */
  inCommentary?: string;
  /** Interrogation, FOIA or Courtroom. */
  kind?: Kind;
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
  /** How Writer reaches Claude: one `claude -p` answer, or the Agent SDK building in steps. */
  engine?: "cli" | "sdk";
};

/** One script's footage on this Mac, and whether the cloud holds it. */
export type ScriptStorage = {
  id: string;
  title: string;
  totalBytes: number;
  mediaBytes: number;
  reclaimableBytes: number;
  blockedReason: string | null;
};

/** Writer's local storage: footage is the cloud's, this is cache. */
export type StorageReport = {
  totalBytes: number;
  mediaBytes: number;
  reclaimableBytes: number;
  blockedBytes: number;
  scripts: ScriptStorage[];
};

export type StorageCleared = { freedBytes: number; cleared: string[]; blocked: ScriptStorage[] };

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

/** A moment a commentary brief needs, as the clip search planned it. */
export type Need = { id: string; chapter: string; what: string; visual: boolean; queries: string[] };

export type FoundVideo = {
  id: string;
  title: string;
  channel: string;
  durationMs: number;
  views?: number;
  needs: string[];
  given: boolean;
  about: string;
  problem?: string;
  source?: string;
  /** YouTube gave no captions, so Writer transcribed its audio. */
  heard: boolean;
};

/** A clip found in a YouTube video's captions, its words checked against them. */
export type Found = {
  need: string;
  video: string;
  startMs: number;
  endMs: number;
  quote: string;
  why: string;
  strength: "strong" | "partial";
  look?: { shows: "yes" | "no" | "unsure"; seen: string; frames: string };
};

/** The last clip search for a commentary project. */
export type Finds = {
  needs: Need[];
  videos: FoundVideo[];
  clips: Found[];
  searches: number;
  problems: string[];
  costUsd: number;
  startedAt: number;
  finishedAt: number;
  dir: string;
};

/** One thing a reviewer marked on the Doc: a comment, or a highlight on its own. */
export type FeedbackItem = {
  id: string;
  kind: "comment" | "highlight";
  quote: string;
  comment: string;
  author: string;
  replies: string[];
  color?: string;
  place?: string;
  note: string;
  skip: boolean;
};

/** A paragraph (or montage clip, heading or check) that changed between two versions of the script. */
export type ScriptChange = { place: string; before?: string; after?: string };

export type Grade = "fixed" | "partly" | "missed" | "wrong";

/** One test of the lesson: the script fixed from the lesson alone, graded against the feedback. */
export type LessonRound = {
  number: number;
  lesson: string;
  changes: ScriptChange[];
  verdicts: { item: string; grade: Grade; changes: string[]; why: string }[];
  extras: { change: string; harmful: boolean; why: string }[];
  score: number;
  summary: string;
  costUsd: number;
  at: number;
};

/** Feedback pulled from a Doc, and the lesson being learned from it. */
export type Feedback = {
  doc: string;
  docUrl: string;
  pulledAt: number;
  revision: number;
  items: FeedbackItem[];
  key?: ScriptChange[];
  title: string;
  lesson: string;
  rounds: LessonRound[];
  problems: string[];
  costUsd: number;
};

/** A lesson kept from a review. Every commentary build reads it. */
export type Lesson = { id: string; title: string; text: string; script: string; scriptTitle: string; pulledAt: number; score?: number; keptAt: number };

/** A step in learning from feedback, run as a job. */
export type LearnStep = { step: "round" } | { step: "improve" } | { step: "finish"; round: number };

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

/** `id` is given when the script is first shared, and kept: review notes anchor to it. */
export type Paragraph = { id?: string; pieces: Piece[]; note?: string };

export type Section = { heading: string; paragraphs: Paragraph[] };

export type Draft = {
  open: Cite[];
  sections: Section[];
  checks: string[];
  report: string;
  writtenAt: number;
};

/** How a commentary script balances narration against clips: the channel's clip-led way, or narration-led like Turkey Tom's documentaries. */
export type Pacing = "channel" | "documentary";

export type Commentary = { pacing?: Pacing; sources: Source[]; draft?: Draft };

export type Adding =
  | { kind: "link"
  | { kind: "reviewer"; author: string; choices: { reference: string; text: string }[] }; url: string }
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
export type ChatTurn = {
  role: "producer" | "editor";
  text: string;
  at: number;
  costUsd?: number;
  failed?: boolean;
  /** Claude asked to start the script over: pending until the producer answers. */
  reset?: "pending" | "accepted" | "declined";
};

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
  kind: "case" | "compilation" | "video";
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

/** A script's review link on Repack (reviews.rs), once shared. */
export type ReviewLink = { reviewId: string; token: string; url: string; version: number; revision: number; sharedAt: number };
/** Part of a block's text, in UTF-16 units; empty where words were taken out. */
export type ReviewRange = { blockId: string; start: number; end: number };
/** A script as the review page shows it: blocks whose IDs stay as it's edited (blocks.rs). */
export type ReviewBlock = {
  id: string;
  kind: "footage" | "narration" | "montage" | "heading" | "paragraph";
  label?: string;
  text: string;
  /** A footage clip's transcript just outside it, for "start earlier" / "end later". */
  before?: string;
  after?: string;
  previewMs?: [number, number];
  words?: [number, number][];
  readMs?: number;
  durationMs?: number;
  editNotes?: string[];
};
export type ReviewAnchor = { kind?: string; blockId?: string; start?: number; end?: number; quote?: string; side?: "before" | "after" };
/** A reviewer's note, or a reply to one, with what the fix changed once it's done. */
export type ReviewNote = {
  id: string;
  reviewId: string;
  scriptId?: string | null;
  version: number;
  parentId?: string | null;
  anchor: ReviewAnchor | null;
  text: string;
  author: string;
  /** Left from Writer (a note of our own, or Writer's reply), not by a reviewer. */
  fromApp?: boolean;
  status: "open" | "done";
  doneInVersion?: number | null;
  changedRanges: ReviewRange[];
  fixSummary?: string | null;
  attempted: boolean;
  /** What the reviewer means it to change: wording, fact, cut, pacing, question, fyi, editor. */
  intent?: string | null;
  scope?: "here" | "everywhere" | null;
  /** The exact words to put in place of the anchored ones. */
  suggestion?: string | null;
  /** On a footage clip: cut, extend or restore. */
  action?: string | null;
  /** A reviewer confirmed the fix, and who. */
  confirmed?: boolean;
  confirmedBy?: string | null;
  /** The version the fix it was sent back from was in. */
  notFixedVersion?: number | null;
  /** Sent back as not fixed (a retry), when. */
  notFixedAt?: number | null;
  /** When a computer took it to fix (claims last 30 minutes). */
  attemptAt?: number | null;
  /** A suggestion's new words ("" deletes the anchored ones), and the words either side of an insertion. */
  replacement?: string | null;
  context?: { before: string; after: string } | null;
  /** One edit across blocks: its notes share this. */
  group?: string | null;
  /** A suggestion's end: accepted or rejected. */
  outcome?: "accepted" | "rejected" | null;
  createdAt: number;
  updatedAt: number;
};
/** A review's approved version: the link takes no more notes, and Repack deletes the other versions' files (their notes stay). */
export type ReviewApproval = { version: number; by: string; at: number };
export type ReviewCounts = { open: Record<string, number>; approved: Record<string, ReviewApproval>; fixesHere: boolean };

/** A stretch of a YouTube video a commentary plays, downloaded beside it (footage.rs). */
export type FootagePiece = { video: string; fromMs: number; toMs: number; file: string };
export type Footage = { pieces: FootagePiece[]; dir: string; pending: number; failed: number };
