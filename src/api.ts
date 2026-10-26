import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { Adding, Brief, BuildStatus, ChatTurn, DocComment, DocFile, Check, Choice, ClaudeStatus, Draft, Feedback, Finds, Focus, LearnStep, Lesson, Pacing, Kind, Research, ReviewApproval, ReviewBlock, ReviewCounts, ReviewNote, ImportProgress, Role, Script, Settings, Snapshot, StorageCleared, StorageReport, Summary, TrelloCards, TrelloList, Version, VideoCheck, Footage, TrelloCard } from "./types";

export function inTauri() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Writer's own replies are JSON. Anything else came from in between (Vite
 * while the app restarts, a dropped tailnet hop), and says so plainly. A
 * connection that drops outright still rejects with fetch's TypeError, which
 * callers treat as "still running on the Mac". */
async function http<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, init);
  const text = await response.text();
  let body: unknown;
  try {
    body = text ? JSON.parse(text) : undefined;
  } catch {
    throw new Error(
      `Writer didn't answer (${response.status}). It may be restarting; try again in a moment.`,
    );
  }
  if (!response.ok) {
    const said = (body as { error?: string } | undefined)?.error;
    throw new Error(said || `Writer didn't answer (${response.status}). Try again in a moment.`);
  }
  return body as T;
}

const json = (body: unknown): RequestInit => ({
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify(body),
});

export const listScripts = () =>
  inTauri() ? invoke<Summary[]>("list_scripts") : http<Summary[]>("/api/scripts");

export const getScript = (id: string) =>
  inTauri() ? invoke<Script>("get_script", { id }) : http<Script>(`/api/scripts/${id}`);

/** The footage, and an optional article about the case, read in place of research, as Repack takes it. */
export const importFile = (path: string, article?: string, kind?: Kind) =>
  inTauri() ? invoke<Script>("import_file", { path, article: article || null, kind }) : http<Script>("/api/scripts/file", json({ path, article, kind }));

export const importYoutube = (url: string, article?: string, kind?: Kind) =>
  inTauri()
    ? invoke<Script>("import_youtube", { url, article: article || null, kind })
    : http<Script>("/api/scripts/youtube", json({ url, article, kind }));

/** A title given (a Trello card's) is kept in place of the folder's name. */
export const importDrive = (url: string, article?: string, title?: string, kind?: Kind) =>
  inTauri()
    ? invoke<Script>("import_drive", { url, article: article || null, title: title || null, kind })
    : http<Script>("/api/scripts/drive", json({ url, article, title, kind }));

/** The cards waiting on one of Repack's Trello lists, read through Writer's service. */
/** The card a pasted Trello link names, read for its videos (any board Repack's Trello reads). */
export const trelloCard = (link: string) =>
  inTauri() ? invoke<TrelloCard>("trello_card", { link }) : http<TrelloCard>("/api/trello/card", json({ link }));
export const trelloCards = (list: TrelloList) =>
  inTauri() ? invoke<TrelloCards>("trello_cards", { list }) : http<TrelloCards>(`/api/trello/${list}`);

export const renameScript = (id: string, title: string) =>
  inTauri()
    ? invoke<Script>("rename_script", { id, title })
    : http<Script>(`/api/scripts/${id}/title`, { ...json({ title }), method: "PUT" });

export const resumeImport = (id: string, choice: Choice) =>
  inTauri()
    ? invoke<Script>("resume_import", { id, choice })
    : http<Script>(`/api/scripts/${id}/resume`, json(choice));

export const doctor = () => (inTauri() ? invoke<Check[]>("doctor") : http<Check[]>("/api/doctor"));

export const checkDrive = () =>
  inTauri() ? invoke<string>("check_drive") : http<string>("/api/drive/check", { method: "POST" });

export const retryTranscribe = (id: string) =>
  inTauri()
    ? invoke<Script>("retry_transcribe", { id })
    : http<Script>(`/api/scripts/${id}/transcribe`, { method: "POST" });

export const saveClips = (id: string, revision: number, clips: Script["clips"]) =>
  inTauri()
    ? invoke<Script>("save_clips", { id, revision, clips })
    : http<Script>(`/api/scripts/${id}/clips`, { ...json({ revision, clips }), method: "PUT" });

export const deleteScript = (id: string) =>
  inTauri() ? invoke<void>("delete_script", { id }) : http<void>(`/api/scripts/${id}`, { method: "DELETE" });

export const getSettings = () =>
  inTauri() ? invoke<Settings>("get_settings") : http<Settings>("/api/settings");

export const saveSettings = (settings: Settings) =>
  inTauri()
    ? invoke<Settings>("save_settings", { settings })
    : http<Settings>("/api/settings", { ...json(settings), method: "PUT" });

export const checkRelay = () =>
  inTauri()
    ? invoke<RelayHealth>("check_relay")
    : http<RelayHealth>("/api/relay/check", { method: "POST" });

type RelayHealth = { ok: boolean; transcription: boolean; drive: boolean; proxy: boolean; message: string };

export const claudeStatus = () =>
  inTauri() ? invoke<ClaudeStatus>("claude_status") : http<ClaudeStatus>("/api/claude");

export const startBuild = (id: string, note: string, kind?: Kind) =>
  inTauri()
    ? invoke<BuildStatus>("start_build", { id, note, kind })
    : http<BuildStatus>(`/api/scripts/${id}/build`, json({ note, kind }));

/** Opus revises a built script with targeted edits; followed as a build is. */
export const startRevision = (id: string, request: string, focus: Focus) =>
  inTauri()
    ? invoke<BuildStatus>("start_revision", { id, request, focus })
    : http<BuildStatus>(`/api/scripts/${id}/revise`, json({ request, focus }));

/** Brings a compilation's reused cases' videos into its footage for intro clips (all not yet in, by default); followed as a build is. */
export const importReuses = (id: string, urls: string[] = []) =>
  inTauri()
    ? invoke<BuildStatus>("start_reuse_import", { id, urls })
    : http<BuildStatus>(`/api/scripts/${id}/reuses`, json({ urls }));

/** Searches YouTube for the clips a commentary brief needs; followed as a build is. */
export const findClips = (id: string) =>
  inTauri() ? invoke<BuildStatus>("start_find_clips", { id }) : http<BuildStatus>(`/api/scripts/${id}/finds`, json({}));

export const getFinds = (id: string) =>
  inTauri() ? invoke<Finds | null>("get_finds", { id }) : http<Finds | null>(`/api/scripts/${id}/finds`);

/** Starts the feedback over from the notes on the script's review page. */
export const pullFeedback = (id: string) =>
  inTauri() ? invoke<Feedback>("pull_feedback", { id }) : http<Feedback>(`/api/scripts/${id}/feedback`, json({}));

/** What a note from Writer says beyond its text: the words it's on, its intent, a suggestion's new words. */
export type NoteDetails = { anchor?: { kind: "text"; blockId: string; start: number; end: number; quote: string }; intent?: string; replacement?: string };

export const resolveProposals = (id: string, revision: number, proposals: string[], accept: boolean) =>
  inTauri() ? invoke<Script>("resolve_proposals", { id, revision, proposals, accept }) : http<Script>(`/api/scripts/${id}/proposals`, json({ revision, proposals, accept }));

export const retrySpeakers = (id: string) =>
  inTauri() ? invoke<BuildStatus>("retry_speakers", { id }) : http<BuildStatus>(`/api/scripts/${id}/speakers/retry`, json({}));

export const stopJob = (id: string) =>
  inTauri() ? invoke<void>("stop_job", { id }) : http<void>(`/api/scripts/${id}/build/stop`, json({}));

/** Leaves a note on the script's review page from Writer: the same note a reviewer leaves there. */
export const addReviewNote = (id: string, text: string, details?: NoteDetails) =>
  inTauri() ? invoke<void>("add_review_note", { id, text, details }) : http<void>(`/api/scripts/${id}/review/notes`, json({ text, details }));

/** A reply under a note on the review page, signed with the signed-in person's name. */
export const replyReviewNote = (id: string, noteId: string, text: string) =>
  inTauri() ? invoke<void>("reply_review_note", { id, noteId, text }) : http<void>(`/api/scripts/${id}/review/notes/${noteId}/replies`, json({ text }));

/** A person's word on a fix: confirmed, or sent back with what still isn't right (added as a reply). */
export const checkReviewNote = (id: string, noteId: string, verdict: "confirmed" | "notFixed", text?: string) =>
  inTauri() ? invoke<void>("check_review_note", { id, noteId, verdict, text }) : http<void>(`/api/scripts/${id}/review/notes/${noteId}/check`, json({ verdict, text }));

/** How Ready for changes is going on the script's review (the page's `fix`), and whether the fixing computer is on. */
export type ReviewFix = {
  state: "waiting" | "fixing" | "sharing" | "done" | "failed";
  by: string;
  requestedAt: number;
  version: number;
  stage: string | null;
  done: number | null;
  total: number | null;
  newVersion: number | null;
  error: string | null;
  stale: boolean;
  updatedAt: number;
};
/** What a reviewer would flag that a count can find (commentary::audit, claude::team_flags), shown before sharing. */
export const scriptAudit = (id: string) =>
  inTauri() ? invoke<{ flags: string[] }>("script_audit", { id }) : http<{ flags: string[] }>(`/api/scripts/${id}/audit`);

export const reviewFix = (id: string) =>
  inTauri()
    ? invoke<{ fix: ReviewFix | null; computer: "on" | "off" | "none" | null }>("review_fix", { id })
    : http<{ fix: ReviewFix | null; computer: "on" | "off" | "none" | null }>(`/api/scripts/${id}/review/fix`);

/** Deletes a note left from Writer (never a reviewer's), with its replies. */
export const deleteReviewNote = (id: string, noteId: string) =>
  inTauri() ? invoke<void>("delete_review_note", { id, noteId }) : http<void>(`/api/scripts/${id}/review/notes/${noteId}`, { method: "DELETE" });

export const getFeedback = (id: string) =>
  inTauri() ? invoke<Feedback | null>("get_feedback", { id }) : http<Feedback | null>(`/api/scripts/${id}/feedback`);

export const setFeedbackItem = (id: string, item: string, change: { skip?: boolean; note?: string }) =>
  inTauri()
    ? invoke<Feedback>("set_feedback_item", { id, item, skip: change.skip ?? null, note: change.note ?? null })
    : http<Feedback>(`/api/scripts/${id}/feedback/items/${item}`, { ...json(change), method: "PUT" });

/** Saves the lesson, and says what in it still points at the script. */
export const setLesson = (id: string, lesson: string) =>
  inTauri()
    ? invoke<{ feedback: Feedback; leaks: string[] }>("set_lesson", { id, lesson })
    : http<{ feedback: Feedback; leaks: string[] }>(`/api/scripts/${id}/feedback/lesson`, { ...json({ lesson }), method: "PUT" });

export const startLearning = (id: string, step: LearnStep) =>
  inTauri() ? invoke<BuildStatus>("start_feedback", { id, step }) : http<BuildStatus>(`/api/scripts/${id}/feedback/steps`, json(step));

export const keepLesson = (id: string) =>
  inTauri() ? invoke<Lesson[]>("keep_lesson", { id }) : http<Lesson[]>(`/api/scripts/${id}/feedback/keep`, json({}));

export const getLessons = () => (inTauri() ? invoke<Lesson[]>("get_lessons") : http<Lesson[]>("/api/lessons"));

export const forgetLesson = (lesson: string) =>
  inTauri() ? invoke<Lesson[]>("forget_lesson", { lesson }) : http<Lesson[]>(`/api/lessons/${lesson}`, { method: "DELETE" });

/** The frames the clip search looked at for one clip. */
export const framesUrl = (id: string, finds: Finds, frames: string) =>
  inTauri() ? convertFileSrc(`${finds.dir}/${frames}`) : `/api/scripts/${id}/finds/frames/${frames.split("/").pop()}`;

/** The script's conversation with the editor. */
/** The producer's answer to Claude asking to start over: accepted, the built script goes (the transcript and research stay). */
export const resetScript = (id: string, accept: boolean) =>
  inTauri() ? invoke<Script>("reset_script", { id, accept }) : http<Script>(`/api/scripts/${id}/reset`, json({ accept }));

export const getChat = (id: string) =>
  inTauri() ? invoke<ChatTurn[]>("get_chat", { id }) : http<ChatTurn[]>(`/api/scripts/${id}/chat`);

/** Starts the conversation again; the script's edits stay. */
export const clearChat = (id: string) =>
  inTauri() ? invoke<void>("clear_chat", { id }) : http<void>(`/api/scripts/${id}/chat`, { method: "DELETE" });

/** Declines a proposed rewrite; the saved narration stays. */
export const declineProposal = (id: string, proposal: string) =>
  inTauri()
    ? invoke<Script>("decline_proposal", { id, proposal })
    : http<Script>(`/api/scripts/${id}/proposals/${proposal}`, { method: "DELETE" });

/** Respells a transcript word; an empty text puts it back. */
export const correctWord = (id: string, revision: number, word: string, text: string) =>
  inTauri()
    ? invoke<Script>("correct_word", { id, revision, word, text })
    : http<Script>(`/api/scripts/${id}/corrections`, { ...json({ revision, word, text }), method: "PUT" });

/** The producer's own heading; an empty one puts the generated heading back. */
export const setHeading = (id: string, revision: number, heading: string) =>
  inTauri()
    ? invoke<Script>("set_heading", { id, revision, heading })
    : http<Script>(`/api/scripts/${id}/heading`, { ...json({ revision, heading }), method: "PUT" });

/** The script's versions, oldest first. */
export const listVersions = (id: string) =>
  inTauri() ? invoke<Version[]>("list_versions", { id }) : http<Version[]>(`/api/scripts/${id}/versions`);

export const getVersion = (id: string, version: string) =>
  inTauri() ? invoke<Snapshot>("get_version", { id, version }) : http<Snapshot>(`/api/scripts/${id}/versions/${version}`);

/** Makes an earlier version the script again. */
export const restoreVersion = (id: string, revision: number, version: string) =>
  inTauri()
    ? invoke<Script>("restore_version", { id, revision, version })
    : http<Script>(`/api/scripts/${id}/versions/${version}/restore`, json({ revision }));

export const getResearch = (id: string) =>
  inTauri() ? invoke<Research | null>("get_research", { id }) : http<Research | null>(`/api/scripts/${id}/research`);

/** Looks for the news about the case again, replacing what was found. */
export const researchAgain = (id: string) =>
  inTauri()
    ? invoke<Research>("research_again", { id })
    : http<Research>(`/api/scripts/${id}/research`, { method: "POST" });

/** Videos joined in order: a compilation, one case each, or (`kase`) one Courtroom case across them. */
export const importCompilation = (title: string, urls: string[], kase = false, kind?: Kind) =>
  inTauri()
    ? invoke<Script>("import_compilation", { title, urls, case: kase, kind })
    : http<Script>("/api/scripts/compilation", json({ title, urls, case: kase, kind }));

/** A pasted brief read into its title, notes and labelled links, before anything imports. */
export const readBrief = (text: string) => (inTauri() ? invoke<Brief>("read_brief", { text }) : http<Brief>("/api/briefs/read", json({ text })));

/** A brief imported as one case: one video as itself, several as one Courtroom case, each keeping its label. */
export const importBrief = (text: string, kind?: Kind) =>
  inTauri() ? invoke<Script>("import_brief", { text, kind }) : http<Script>("/api/scripts/brief", json({ text, kind }));

/** A compilation's lines checked before anything downloads, as Repack checks its videos. */
export const checkVideos = (lines: string[]) =>
  inTauri() ? invoke<VideoCheck[]>("check_videos", { lines }) : http<VideoCheck[]>("/api/videos/check", json({ lines }));

export const buildStatus = (id: string) =>
  inTauri()
    ? invoke<BuildStatus | null>("build_status", { id })
    : http<BuildStatus | null>(`/api/scripts/${id}/build`);

/** A commentary project, its links read in as sources (the first is the brief). */
export const createCommentary = (title: string, links: string[]) =>
  inTauri()
    ? invoke<Script>("create_commentary", { title, links })
    : http<Script>("/api/scripts/commentary", json({ title, links }));

export const addSource = (id: string, adding: Adding, role: Role) =>
  inTauri()
    ? invoke<Script>("add_source", { id, adding, role })
    : http<Script>(`/api/scripts/${id}/sources`, json({ adding, role }));

export const setPacing = (id: string, pacing: Pacing) =>
  inTauri() ? invoke<Script>("set_pacing", { id, pacing }) : http<Script>(`/api/scripts/${id}/pacing`, { ...json({ pacing }), method: "PUT" });

export const updateSource = (id: string, source: string, change: { role?: Role; title?: string }) =>
  inTauri()
    ? invoke<Script>("update_source", { id, source, role: change.role ?? null, title: change.title ?? null })
    : http<Script>(`/api/scripts/${id}/sources/${source}`, { ...json(change), method: "PUT" });

export const removeSource = (id: string, source: string) =>
  inTauri()
    ? invoke<Script>("remove_source", { id, source })
    : http<Script>(`/api/scripts/${id}/sources/${source}`, { method: "DELETE" });

export const sourceText = (id: string, source: string) =>
  inTauri() ? invoke<string>("source_text", { id, source }) : http<string>(`/api/scripts/${id}/sources/${source}`);

export const saveDraft = (id: string, revision: number, draft: Draft) =>
  inTauri()
    ? invoke<Script>("save_draft", { id, revision, draft })
    : http<Script>(`/api/scripts/${id}/draft`, { ...json({ revision, draft }), method: "PUT" });

/** A new Google Doc from the script (a Word file in base64), as Repack exports. */
export const exportDoc = (id: string, draft: boolean, docx: string) =>
  inTauri() ? invoke<DocFile>("export_doc", { id, draft, docx }) : http<DocFile>(`/api/scripts/${id}/docs`, json({ draft, docx }));

/** Every Doc exported from the script, read from Drive each time. */
export const listDocs = (id: string) =>
  inTauri() ? invoke<DocFile[]>("list_docs", { id }) : http<DocFile[]>(`/api/scripts/${id}/docs`);

/** The comments on the script's latest Doc, or on the one named. */
export const docComments = (id: string, doc?: string) =>
  inTauri()
    ? invoke<{ doc: string; comments: DocComment[] }>("doc_comments", { id, doc: doc ?? null })
    : http<{ doc: string; comments: DocComment[] }>(doc ? `/api/scripts/${id}/docs/${doc}/comments` : `/api/scripts/${id}/docs/comments`);

/** Every import running now. */
export const getProgress = () =>
  inTauri() ? invoke<ImportProgress[]>("import_progress") : http<ImportProgress[]>("/api/progress");

export function message(error: unknown) {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Something went wrong.";
}

export type CloudStatus = {
  enabled: boolean;
  state: "off" | "syncing" | "synced" | "offline" | "error" | "";
  pending: number;
  transfer: { script: string; name: string; up: boolean; done: number; total: number } | null;
  lastSync: number | null;
  error: string | null;
  scripts: number;
};

/** Where syncing to the cloud has got to. */
export const cloudStatus = () => (inTauri() ? invoke<CloudStatus>("cloud_status") : http<CloudStatus>("/api/cloud"));

/** What Writer holds on this Mac, and how much the cloud holds too. */
export const storageReport = () =>
  inTauri() ? invoke<StorageReport>("storage_report") : http<StorageReport>("/api/storage");

/** Clears local footage verified in the cloud; still-uploading footage stays. */
export const clearStorage = () =>
  inTauri() ? invoke<StorageCleared>("clear_storage") : http<StorageCleared>("/api/storage/clear", { method: "POST" });

/**
 * Where a script's preview plays from: the file on this Mac when it's here,
 * otherwise the cloud's copy a range at a time (stream.rs), as Repack plays
 * its footage. `v` changes the address when the footage does.
 */
export const previewSrc = (id: string, v: number | string) =>
  inTauri() ? `preview://localhost/${id}?v=${v}` : `/api/scripts/${id}/proxy?v=${v}`;

/** A cleared preview back from the cloud, so the script plays here again. */
export const restorePreview = (id: string) =>
  inTauri() ? invoke<Script>("restore_preview", { id }) : http<Script>(`/api/scripts/${id}/preview`, { method: "POST" });

/** Shares the script on a Repack review link, or its next version on the same link. */
/** The script as it is now made its review's only version, v1, on the same link. */
export const compressReview = (id: string) =>
  inTauri() ? invoke<Script>("compress_review", { id }) : http<Script>(`/api/scripts/${id}/review/compress`, json({}));
export const shareScript = (id: string) =>
  inTauri() ? invoke<Script>("share_script", { id }) : http<Script>(`/api/scripts/${id}/share`, json({}));

/** Open review notes per script, for the cards' badges, and whether Claude fixes new ones by itself. */
export const reviewCounts = () =>
  inTauri() ? invoke<ReviewCounts>("review_counts") : http<ReviewCounts>("/api/reviews");

/** A script's review notes, and its blocks as they now read. */
export type ReviewState = { notes: ReviewNote[]; blocks: ReviewBlock[]; approved: ReviewApproval | null };

export const reviewNotes = (id: string) =>
  inTauri() ? invoke<ReviewState>("review_notes", { id }) : http<ReviewState>(`/api/scripts/${id}/review`);

/** Approves the latest shared version (Repack deletes the other versions' files; the notes stay), or takes the approval back. */
export const approveReview = (id: string, approve: boolean) =>
  inTauri() ? invoke<ReviewState>("approve_review", { id, approve }) : http<ReviewState>(`/api/scripts/${id}/review/approval`, { ...json({ approve }), method: "PUT" });

/** Has Claude fix the script's open notes now; followed as a build. */
export const fixNotes = (id: string) =>
  inTauri() ? invoke<BuildStatus>("fix_notes", { id }) : http<BuildStatus>(`/api/scripts/${id}/review/fix`, json({}));

export const setEngine = (engine: "cli" | "sdk") =>
  inTauri() ? invoke<Settings>("set_engine", { engine }) : http<Settings>("/api/settings/engine", { ...json({ engine }), method: "PUT" });

/** Calls `then` whenever review notes change (in the app; the dev page asks every half minute). */
/** Scripts came, went or changed behind the page's back (the cloud, the local
 * API): Writer says so; a page in a browser asks every few seconds. */
export function onLibrary(then: () => void): () => void {
  if (!inTauri()) {
    let seen: number | null = null;
    const timer = window.setInterval(() => {
      if (document.hidden) return;
      void http<{ version: number }>("/api/library")
        .then(({ version }) => {
          if (seen !== null && version !== seen) then();
          seen = version;
        })
        .catch(() => {});
    }, 3_000);
    return () => window.clearInterval(timer);
  }
  let stop: (() => void) | null = null;
  let gone = false;
  void import("@tauri-apps/api/event").then(({ listen }) =>
    listen("scripts", then).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    }),
  );
  return () => {
    gone = true;
    stop?.();
  };
}

export function onReviews(then: () => void): () => void {
  if (!inTauri()) {
    const timer = window.setInterval(then, 30_000);
    return () => window.clearInterval(timer);
  }
  let stop: (() => void) | null = null;
  let gone = false;
  void import("@tauri-apps/api/event").then(({ listen }) =>
    listen("reviews", then).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    }),
  );
  return () => {
    gone = true;
    stop?.();
  };
}


/** Brings an archived script's footage back from its links (or Edit's originals); followed as a build. */
export const restoreScript = (id: string) =>
  inTauri() ? invoke<BuildStatus>("restore_script", { id }) : http<BuildStatus>(`/api/scripts/${id}/restore`, json({}));

/** A line of Writer's log. */
export type LogLine = { at: number; time: string; level: "INFO" | "WARN" | "ERROR"; area: string; message: string };

/** The log's lines after `since` (ms), and where its file is. */
export const getLogs = (since: number) =>
  inTauri() ? invoke<{ lines: LogLine[]; file: string | null }>("get_logs", { since }) : http<{ lines: LogLine[]; file: string | null }>(`/api/logs?since=${since}`);

/** Shows the log file in Finder. */
export const revealLogs = () => (inTauri() ? invoke<void>("reveal_logs") : http<void>("/api/logs/reveal", json({})));

/** Something long under way: what, on what, the step and how far. */
export type Activity = { key: string; script: string | null; what: string; title: string; step: string; done: number; total: number; unit: "count" | "bytes" | "percent"; started: number };

export const getActivity = () => (inTauri() ? invoke<Activity[]>("get_activity") : http<Activity[]>("/api/activity"));

/** Which Writer this is: "Writer nightly-1", "Writer v0.0.0", or a dev build. */
export type About = { name: string; version: string; tag: string | null; release: boolean };
export const getAbout = () => (inTauri() ? invoke<About>("about") : http<About>("/api/about"));

/** Who's signed in (account.rs), at accounts.spheceo.com. */
export type Person = { sub: string; email: string; name: string; image: string | null; perms: string[]; admin: boolean };
export type AccountStatus = {
  signedIn: boolean;
  person: Person | null;
  signingIn: boolean;
  /** While signing in: the page to open, if the browser didn't. */
  signInUrl: string | null;
  /** Signing in with a code on another device: what to type, and where. */
  code: { userCode: string; url: string; urlComplete: string | null } | null;
  error: string | null;
};
/** A script starter's photo, kept on this Mac: a src for <img>, or null (show initials). */
export const avatarSrc = async (sub: string, picture: string): Promise<string | null> => {
  if (!inTauri()) return `/api/avatar?sub=${encodeURIComponent(sub)}&picture=${encodeURIComponent(picture)}`;
  const path = await invoke<string | null>("avatar", { sub, picture });
  return path ? convertFileSrc(path) : null;
};
export const accountStatus = () => (inTauri() ? invoke<AccountStatus>("account_status") : http<AccountStatus>("/api/account"));
export const signIn = () => (inTauri() ? invoke<AccountStatus>("sign_in") : http<AccountStatus>("/api/account/sign-in", json({})));
export const signInWithCode = () => (inTauri() ? invoke<AccountStatus>("sign_in_with_code") : http<AccountStatus>("/api/account/sign-in-code", json({})));
/** Opens one of Writer's own pages (accounts.spheceo.com, Writer's issues) in the browser. */
export const openPage = (url: string) => (inTauri() ? invoke<void>("open_page", { url }) : http<void>("/api/open", json({ url })));
export const cancelSignIn = () => (inTauri() ? invoke<AccountStatus>("cancel_sign_in") : http<AccountStatus>("/api/account/cancel", json({})));
export const signOut = () => (inTauri() ? invoke<AccountStatus>("sign_out") : http<AccountStatus>("/api/account/sign-out", json({})));

/** Self-update (update.rs): this build, the channel, and the build on offer. */
export type UpdateStatus = {
  name: string;
  channel: "nightly" | "stable";
  enabled: boolean;
  offer: { tag: string; size: number } | null;
  phase: "idle" | "downloading" | "installing" | "restarting";
  error: string | null;
  checkedAt: number | null;
  /** Why the last look for updates failed, until one works. */
  checkError: string | null;
};
export const updateStatus = () => (inTauri() ? invoke<UpdateStatus>("update_status") : http<UpdateStatus>("/api/update"));
export const checkUpdate = () => (inTauri() ? invoke<UpdateStatus>("check_update") : http<UpdateStatus>("/api/update/check", json({})));
export const installUpdate = () => (inTauri() ? invoke<void>("install_update") : http<void>("/api/update/install", json({})));
export const setUpdateChannel = (channel: "nightly" | "stable") =>
  inTauri() ? invoke<UpdateStatus>("set_update_channel", { channel }) : http<UpdateStatus>("/api/update/channel", { ...json({ channel }), method: "PUT" });

/** A commentary's downloaded footage; asking starts what's missing (footage.rs). */
export const getFootage = (id: string) => (inTauri() ? invoke<Footage>("get_footage", { id }) : http<Footage>(`/api/scripts/${id}/footage`));

/** Where a downloaded stretch plays from: the file itself in the Mac's window, the API on the dev page. */
export const footageUrl = (id: string, footage: Footage, file: string) => (inTauri() ? convertFileSrc(`${footage.dir}/${file}`) : `/api/scripts/${id}/footage/${encodeURIComponent(file)}`);

/** Whether downloads can fall back on the person's YouTube sign-in (a cookies file, read silently). */
export const youtubeSignIn = () =>
  inTauri() ? invoke<{ saved: boolean; savedAt: number | null }>("youtube_sign_in_status") : http<{ saved: boolean; savedAt: number | null }>("/api/youtube/sign-in");
/** Copies the YouTube sign-in from the browser into that file, once (Chrome asks macOS for its key this one time). */
export const saveYoutubeSignIn = () => (inTauri() ? invoke<string>("save_youtube_sign_in") : http<{ browser: string }>("/api/youtube/sign-in", json({})).then((answer) => answer.browser));
