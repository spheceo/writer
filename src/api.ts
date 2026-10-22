import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { Adding, BuildStatus, ChatTurn, DocComment, DocFile, Check, Choice, ClaudeStatus, Draft, Feedback, Finds, Focus, LearnStep, Lesson, Pacing, Kind, Research, ReviewBlock, ReviewCounts, ReviewNote, ImportProgress, Role, Script, Settings, Snapshot, Summary, TrelloCards, TrelloList, Version, VideoCheck } from "./types";

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
export const importFile = (path: string, article?: string) =>
  inTauri() ? invoke<Script>("import_file", { path, article: article || null }) : http<Script>("/api/scripts/file", json({ path, article }));

export const importYoutube = (url: string, article?: string) =>
  inTauri()
    ? invoke<Script>("import_youtube", { url, article: article || null })
    : http<Script>("/api/scripts/youtube", json({ url, article }));

/** A title given (a Trello card's) is kept in place of the folder's name. */
export const importDrive = (url: string, article?: string, title?: string) =>
  inTauri()
    ? invoke<Script>("import_drive", { url, article: article || null, title: title || null })
    : http<Script>("/api/scripts/drive", json({ url, article, title }));

/** The cards waiting on one of Repack's Trello lists, read through Writer's service. */
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

/** Sonnet revises a built script with targeted edits; followed as a build is. */
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

export const pullFeedback = (id: string, link: string | null) =>
  inTauri() ? invoke<Feedback>("pull_feedback", { id, link }) : http<Feedback>(`/api/scripts/${id}/feedback`, json({ link }));

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
export const importCompilation = (title: string, urls: string[], kase = false) =>
  inTauri()
    ? invoke<Script>("import_compilation", { title, urls, case: kase })
    : http<Script>("/api/scripts/compilation", json({ title, urls, case: kase }));

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

/** Shares the script on a Repack review link, or its next version on the same link. */
export const shareScript = (id: string) =>
  inTauri() ? invoke<Script>("share_script", { id }) : http<Script>(`/api/scripts/${id}/share`, json({}));

/** Open review notes per script, for the cards' badges, and whether Claude fixes new ones by itself. */
export const reviewCounts = () =>
  inTauri() ? invoke<ReviewCounts>("review_counts") : http<ReviewCounts>("/api/reviews");

/** A script's review notes, and its blocks as they now read. */
export const reviewNotes = (id: string) =>
  inTauri()
    ? invoke<{ notes: ReviewNote[]; blocks: ReviewBlock[] }>("review_notes", { id })
    : http<{ notes: ReviewNote[]; blocks: ReviewBlock[] }>(`/api/scripts/${id}/review`);

/** Has Claude fix the script's open notes now; followed as a build. */
export const fixNotes = (id: string) =>
  inTauri() ? invoke<BuildStatus>("fix_notes", { id }) : http<BuildStatus>(`/api/scripts/${id}/review/fix`, json({}));

export const setAutoFix = (on: boolean) =>
  inTauri() ? invoke<ReviewCounts>("set_auto_fix", { on }) : http<ReviewCounts>("/api/reviews/auto-fix", { ...json({ on }), method: "PUT" });

/** Calls `then` whenever review notes change (in the app; the dev page asks every half minute). */
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
