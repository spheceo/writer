import { invoke } from "@tauri-apps/api/core";
import type { Adding, BuildStatus, ChatTurn, DocComment, DocFile, Check, Choice, ClaudeStatus, Draft, Focus, Kind, Research, ImportProgress, Role, Script, Settings, Summary } from "./types";

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

export const importFile = (path: string) =>
  inTauri() ? invoke<Script>("import_file", { path }) : http<Script>("/api/scripts/file", json({ path }));

export const importYoutube = (url: string) =>
  inTauri()
    ? invoke<Script>("import_youtube", { url })
    : http<Script>("/api/scripts/youtube", json({ url }));

export const importDrive = (url: string) =>
  inTauri() ? invoke<Script>("import_drive", { url }) : http<Script>("/api/scripts/drive", json({ url }));

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

export const getResearch = (id: string) =>
  inTauri() ? invoke<Research | null>("get_research", { id }) : http<Research | null>(`/api/scripts/${id}/research`);

/** Looks for the news about the case again, replacing what was found. */
export const researchAgain = (id: string) =>
  inTauri()
    ? invoke<Research>("research_again", { id })
    : http<Research>(`/api/scripts/${id}/research`, { method: "POST" });

export const importCompilation = (title: string, urls: string[]) =>
  inTauri()
    ? invoke<Script>("import_compilation", { title, urls })
    : http<Script>("/api/scripts/compilation", json({ title, urls }));

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
