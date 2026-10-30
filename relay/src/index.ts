import { type AuthEnv, type Caller, identify, limitKey, mayReadTrello, mayUse, named } from "./auth";
import { type CloudEnv, cloud } from "./cloud";
import { manifestResponse } from "./manifest";

declare const FixedLengthStream: {
  new (size: number): {
    readable: ReadableStream<Uint8Array>;
    writable: WritableStream<Uint8Array>;
  };
};

/**
 * Writer's relay. Holds Writer's keys so the desktop app never does, and
 * nobody installing Writer needs Infisical or a key of their own.
 *
 *   POST /v1/transcribe?keyterm=…&keyterm=…   body: audio/ogg (Opus), Content-Length required
 *                         → ElevenLabs Scribe v2 JSON (words, timings, speakers),
 *                           asked exactly as Edit's relay asks it: the case's
 *                           names as keyterms (at most 100, each under 50
 *                           characters), no language given
 *   GET  /v1/drive/token  → a one-hour access token for Repack's Drive account
 *                           (its refresh token stays here)
 *   GET  /v1/proxy        → the DataImpulse proxy for YouTube downloads, or null
 *   POST /v1/watch        body: { system, text, clip? (base64 MP4), schema }
 *                         → Gemini's answer on a few minutes of footage, through
 *                           the Vercel AI Gateway (who speaks each line); with
 *                           no clip, on the text alone (the case's names for
 *                           the transcriber, as Edit's names.ts asks them)
 *   POST /v1/docs/export  body: { name, scriptId, draft, docx (base64) }
 *                         → a new Google Doc made from the Word file, comments
 *                           and all, in the "Writer scripts" folder of the
 *                           account that owns script Docs, open to anyone with
 *                           its link, as Repack exports
 *   GET  /v1/docs/exports?script=ID → every Doc exported from that script
 *   GET  /v1/docs/comments?script=ID&doc=DOC → the comments people left on
 *                           one of that script's Docs
 *   GET  /v1/trello/cards?list=raw|comp → the cards waiting for a script on
 *                           Repack's Trello lists, read only, as Repack reads
 *                           them (its key and token stay here)
 *   POST /v1/cloud/*      scripts and their files, synced between Macs:
 *                         presigned R2 URLs and Writer's Convex functions;
 *                         also the reviews (as Writer) and Edit's footage
 *                         registry and shared keys, for the handoff (cloud.ts)
 *   GET  /.well-known/spheceo-app.json → what Writer can do, for accounts'
 *                           admin pages (manifest.ts), public
 *   GET  /health
 *
 * Every /v1 route needs a signed-in person's access token with `use` (or an
 * admin's) (auth.ts); the kinds of work in the manifest Writer gates itself.
 * Nothing else passes: older Writers' team token was retired on 2026-10-23.
 *
 * Spend is bounded three ways: a burst limit per person, a daily byte quota
 * per person, and a daily byte cap across everyone (atomic reservations). Audio streams
 * straight through to ElevenLabs and the answer streams straight back, so the
 * Worker never buffers either. Byte quotas are a stand-in for duration; they
 * are not a guaranteed money cap for arbitrary uploaded audio.
 */

export interface Env extends CloudEnv, AuthEnv {
  /** ElevenLabs, Edit's main key (Infisical /writer-relay references /edit-relay's): Scribe, the transcription. */
  ELEVENLABS_MAIN_KEY?: string;
  GOOGLE_DRIVE_CLIENT_ID?: string;
  GOOGLE_DRIVE_CLIENT_SECRET?: string;
  GOOGLE_DRIVE_REFRESH_TOKEN?: string;
  /** DataImpulse gateway URL with its login, as Repack's PROXY_URL. */
  PROXY_URL?: string;
  /** The script Docs account's sign-in (full Drive), as Repack's GOOGLE_DOCS_REFRESH_TOKEN. */
  GOOGLE_DOCS_CLIENT_ID?: string;
  GOOGLE_DOCS_CLIENT_SECRET?: string;
  GOOGLE_DOCS_REFRESH_TOKEN?: string;
  /** Repack's Trello key and token, as its TRELLO_API_KEY and TRELLO_TOKEN. */
  TRELLO_API_KEY?: string;
  TRELLO_TOKEN?: string;
  /** Vercel AI Gateway, for the watcher. */
  AI_GATEWAY_API_KEY?: string;
  WATCH_BURST: { limit(options: { key: string }): Promise<{ success: boolean }> };
  /** Per person: every cloud call (sync, reviews), and the other routes that reach a shared account. */
  CLOUD_BURST: { limit(options: { key: string }): Promise<{ success: boolean }> };
  ROUTE_BURST: { limit(options: { key: string }): Promise<{ success: boolean }> };
  /** Repack's Trello token allows 100 requests in 10 s for everything it does. */
  TRELLO_BURST: { limit(options: { key: string }): Promise<{ success: boolean }> };
  /** Watcher calls a day, per person and across everyone. */
  WATCH_DAILY_PER_PERSON: string;
  WATCH_DAILY_TOTAL: string;
  BUDGET: {
    idFromName(name: string): unknown;
    get(id: unknown): { fetch(request: Request): Promise<Response> };
  };
  BURST: { limit(options: { key: string }): Promise<{ success: boolean }> };
  DAILY_BYTES_PER_IP: string;
  DAILY_BYTES_TOTAL: string;
  MAX_UPLOAD_BYTES: string;
}

const SCRIBE = "https://api.elevenlabs.io/v1/speech-to-text";
const MODEL = "scribe_v2";

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/health") {
      return json(200, {
        ok: true,
        transcription: Boolean(env.ELEVENLABS_MAIN_KEY),
        drive: driveReady(env),
        proxy: Boolean(env.PROXY_URL),
        docs: docsReady(env),
        watch: Boolean(env.AI_GATEWAY_API_KEY),
        trello: Boolean(env.TRELLO_API_KEY && env.TRELLO_TOKEN),
        cloud: Boolean(env.R2_BUCKET && env.CONVEX_URL && env.CONVEX_SERVICE_TOKEN),
        paused: env.CONVEX_PAUSED === "1",
      });
    }
    // What Writer can do, for accounts.spheceo.com's admin pages: public.
    if (url.pathname === "/.well-known/spheceo-app.json") {
      if (request.method !== "GET" && request.method !== "HEAD") return json(405, { error: "GET the manifest." });
      return manifestResponse();
    }
    if (!url.pathname.startsWith("/v1/")) return json(404, { error: "not found" });
    // Docs export is also offered from the feedback site, whose tokens say aud "feedback".
    const docsRoute = url.pathname.startsWith("/v1/docs/");
    // Edit borrows only the fixed read-only Trello routes, using its own
    // short-lived accounts token. No shared credentials leave this relay.
    const trelloRoute = url.pathname === "/v1/trello/card" || url.pathname === "/v1/trello/cards";
    const who = await identify(request, env, docsRoute ? ["writer", "feedback"] : trelloRoute ? ["writer", "edit"] : "writer");
    if ("refused" in who) return json(401, { error: who.refused });
    const caller = who.caller;
    console.log(JSON.stringify({ who: named(caller), method: request.method, route: url.pathname }));
    // A reviewer on Feedback may export a script. Every other route still needs Writer.
    if (trelloRoute && !mayReadTrello(caller)) {
      return json(403, { error: "Your account can't import Trello scripts. Ask an admin for app access and an editing permission at accounts.spheceo.com." });
    }
    if (!mayUse(caller) && !(docsRoute && caller.audience === "feedback")) {
      return json(403, { error: "Your account can't use Writer yet. Ask an admin at accounts.spheceo.com." });
    }
    if (url.pathname.startsWith("/v1/cloud/")) {
      if (!(await env.CLOUD_BURST.limit({ key: limitKey(caller) })).success) return json(429, { error: "Too many cloud requests at once; try again in a minute." });
      return cloud(request, url, env, { sub: caller.sub, name: caller.name, email: caller.email, picture: caller.picture }, { admin: caller.admin, perms: caller.perms });
    }
    // The routes that spend a shared account's quota: a per-person pace.
    if (["/v1/drive/token", "/v1/proxy", "/v1/docs/export", "/v1/docs/exports", "/v1/docs/comments"].includes(url.pathname)) {
      if (!(await env.ROUTE_BURST.limit({ key: limitKey(caller) })).success) return json(429, { error: "Too many requests at once; try again in a minute." });
    }
    if (trelloRoute && !(await env.TRELLO_BURST.limit({ key: limitKey(caller) })).success) {
      return json(429, { error: "Too many Trello requests at once; try again in a minute." });
    }
    if (url.pathname === "/v1/drive/token") {
      if (request.method !== "GET") return json(405, { error: "GET a Drive token." });
      return driveToken(env);
    }
    if (url.pathname === "/v1/proxy") {
      if (request.method !== "GET") return json(405, { error: "GET the proxy." });
      return json(200, { proxyUrl: env.PROXY_URL || null });
    }
    if (url.pathname === "/v1/trello/card") {
      if (request.method !== "GET") return json(405, { error: "GET a card." });
      return trelloCard(url.searchParams.get("card") ?? "", env);
    }
    if (url.pathname === "/v1/trello/cards") {
      if (request.method !== "GET") return json(405, { error: "GET a list's cards." });
      return trelloCards(url.searchParams.get("list") ?? "", env);
    }
    if (url.pathname === "/v1/watch") {
      if (request.method !== "POST") return json(405, { error: "POST a clip to watch." });
      return watch(request, env, caller);
    }
    if (url.pathname === "/v1/docs/export") {
      if (request.method !== "POST") return json(405, { error: "POST a script to export." });
      return exportDoc(request, env);
    }
    if (url.pathname === "/v1/docs/exports") {
      if (request.method !== "GET") return json(405, { error: "GET a script's exports." });
      return listDocs(url.searchParams.get("script") ?? "", env);
    }
    if (url.pathname === "/v1/docs/comments") {
      if (request.method !== "GET") return json(405, { error: "GET a Doc's comments." });
      return docComments(url.searchParams.get("script") ?? "", url.searchParams.get("doc") ?? "", env);
    }
    if (url.pathname !== "/v1/transcribe") return json(404, { error: "not found" });
    if (request.method !== "POST") return json(405, { error: "POST audio to /v1/transcribe" });
    if (!env.ELEVENLABS_MAIN_KEY) {
      return json(503, { error: "Transcription is not configured on the relay yet." });
    }

    const type = request.headers.get("content-type") ?? "";
    if (!type.startsWith("audio/")) return json(415, { error: "Send audio (e.g. audio/ogg)." });
    const size = Number(request.headers.get("content-length"));
    if (!Number.isSafeInteger(size) || size <= 0) return json(411, { error: "Content-Length is required." });
    if (![env.MAX_UPLOAD_BYTES, env.DAILY_BYTES_PER_IP, env.DAILY_BYTES_TOTAL].every((value) => Number.isSafeInteger(Number(value)) && Number(value) > 0)) {
      return json(503, { error: "Transcription budget is misconfigured." });
    }
    if (size > Number(env.MAX_UPLOAD_BYTES)) return json(413, { error: "That recording is too long for one request." });

    const limit = limitKey(caller);
    if (!(await env.BURST.limit({ key: limit })).success) {
      return json(429, { error: "Too many transcriptions at once (the cap is 60 requests a minute per person); try again in a minute." });
    }
    const language = url.searchParams.get("language");
    if (language && !/^[a-zA-Z]{2,3}$/.test(language)) return json(400, { error: "Invalid language code." });
    // The case's names to listen for (Scribe's keyterms), checked as Edit's relay checks them.
    const keyterms = url.searchParams.getAll("keyterm");
    if (keyterms.length > 100 || keyterms.some((term) => term.length < 1 || term.length >= 50 || /[<>{}[\]\\\u0000-\u001f]/.test(term))) {
      return json(400, { error: "Invalid keyterms: at most 100, each under 50 characters." });
    }
    const day = new Date().toISOString().slice(0, 10);
    const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(limit));
    const client = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
    const budget = env.BUDGET.get(env.BUDGET.idFromName(day));
    const reserved = await budget.fetch(new Request("https://budget/reserve", {
      method: "POST",
      body: JSON.stringify({ client, size }),
    }));
    if (!reserved.ok) return reserved;

    const form = multipart(request.body!, size, type, [
      ["model_id", MODEL],
      ["diarize", "true"],
      ["timestamps_granularity", "word"],
      ["tag_audio_events", "false"],
      ...(language ? [["language_code", language] as [string, string]] : []),
      ...keyterms.map((term): [string, string] => ["keyterms", term]),
    ]);
    const upstream = await fetch(SCRIBE, {
      method: "POST",
      headers: { "xi-api-key": env.ELEVENLABS_MAIN_KEY, "content-type": form.contentType },
      body: form.body,
    });

    return new Response(upstream.body, {
      status: upstream.status,
      headers: {
        "content-type": upstream.headers.get("content-type") ?? "application/json",
        "x-writer-model": MODEL,
      },
    });
  },
};

const driveReady = (env: Env) =>
  Boolean(env.GOOGLE_DRIVE_CLIENT_ID && env.GOOGLE_DRIVE_CLIENT_SECRET && env.GOOGLE_DRIVE_REFRESH_TOKEN);

/** One access token per isolate, reused until five minutes before it expires. */
let cached: { token: string; until: number } | null = null;

async function driveToken(env: Env): Promise<Response> {
  if (!driveReady(env)) return json(503, { error: "Google Drive is not configured on the relay yet." });
  if (cached && cached.until > Date.now()) {
    return json(200, { accessToken: cached.token, expiresIn: Math.floor((cached.until - Date.now()) / 1000) + 300 });
  }
  const answer = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      client_id: env.GOOGLE_DRIVE_CLIENT_ID!,
      client_secret: env.GOOGLE_DRIVE_CLIENT_SECRET!,
      refresh_token: env.GOOGLE_DRIVE_REFRESH_TOKEN!,
      grant_type: "refresh_token",
    }),
  });
  const body = (await answer.json().catch(() => ({}))) as {
    access_token?: string;
    expires_in?: number;
    error_description?: string;
    error?: string;
  };
  if (!answer.ok || !body.access_token) {
    const said = body.error_description ?? body.error ?? `status ${answer.status}`;
    return json(502, { error: `Google refused the relay's Drive sign-in (${said}). Its refresh token may need renewing.` });
  }
  const seconds = body.expires_in ?? 3600;
  cached = { token: body.access_token, until: Date.now() + (seconds - 300) * 1000 };
  return json(200, { accessToken: body.access_token, expiresIn: seconds });
}

// ---- The watcher: Gemini watches footage, through the AI Gateway ----

const GATEWAY = "https://ai-gateway.vercel.sh/v1/chat/completions";
const WATCH_MODEL = "google/gemini-3.8-flash";
/** A window of 360p, 3 fps footage is a few MB; base64 adds a third. */
const MAX_WATCH_BYTES = 24_000_000;
/** Writer's own prompts are a few kB; a transcript window's text far under this. */
const MAX_WATCH_SYSTEM = 20_000;
const MAX_WATCH_TEXT = 200_000;

async function watch(request: Request, env: Env, caller: Caller): Promise<Response> {
  if (!env.AI_GATEWAY_API_KEY) return json(503, { error: "The watcher is not configured on the relay yet." });
  const size = Number(request.headers.get("content-length"));
  if (!Number.isSafeInteger(size) || size <= 0) return json(411, { error: "Content-Length is required." });
  if (size > MAX_WATCH_BYTES) return json(413, { error: "That clip is too large to watch in one request." });
  if (!(await env.WATCH_BURST.limit({ key: limitKey(caller) })).success) {
    return json(429, { error: "Too many clips at once; try again in a minute." });
  }
  // A day's watching, per person and for everyone: the key pays per call.
  const day = new Date().toISOString().slice(0, 10);
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(limitKey(caller)));
  const client = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
  const reserved = await env.BUDGET.get(env.BUDGET.idFromName(`watch:${day}`)).fetch(new Request("https://budget/reserve", {
    method: "POST",
    body: JSON.stringify({ client, size: 1, pool: "watch" }),
  }));
  if (!reserved.ok) return reserved;
  const body = (await request.json().catch(() => null)) as {
    system?: unknown;
    text?: unknown;
    clip?: unknown;
    schema?: unknown;
  } | null;
  if (!body || typeof body.system !== "string" || typeof body.text !== "string" || (body.clip !== undefined && typeof body.clip !== "string") || typeof body.schema !== "object" || !body.schema) {
    return json(400, { error: "Send system, text, schema and (to watch footage) clip." });
  }
  if (body.system.length > MAX_WATCH_SYSTEM || body.text.length > MAX_WATCH_TEXT || JSON.stringify(body.schema).length > MAX_WATCH_SYSTEM) {
    return json(413, { error: "That watch request's instructions are too long." });
  }
  // The model is the relay's choice, so the key only ever pays for the watcher.
  const upstream = await fetch(GATEWAY, {
    method: "POST",
    headers: { authorization: `Bearer ${env.AI_GATEWAY_API_KEY}`, "content-type": "application/json" },
    body: JSON.stringify({
      model: WATCH_MODEL,
      messages: [
        { role: "system", content: body.system },
        {
          role: "user",
          content: [
            { type: "text", text: body.text },
            ...(typeof body.clip === "string" ? [{ type: "file", file: { filename: "window.mp4", file_data: `data:video/mp4;base64,${body.clip}` } }] : []),
          ],
        },
      ],
      response_format: { type: "json_schema", json_schema: { name: "watched", strict: true, schema: body.schema } },
      // Text alone is a quick listing (names.ts asks with low effort too).
      ...(typeof body.clip === "string" ? {} : { reasoning: { effort: "low" } }),
    }),
  });
  return new Response(upstream.body, {
    status: upstream.status,
    headers: { "content-type": upstream.headers.get("content-type") ?? "application/json" },
  });
}

// ---- Google Docs: export a script as Repack does ----

const DOCS_FOLDER = "Writer scripts";
const DOC_MIME = "application/vnd.google-apps.document";
const FOLDER_MIME = "application/vnd.google-apps.folder";
const DOCX_MIME = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const DRIVE = "https://www.googleapis.com/drive/v3/";
const MAX_DOCX_BYTES = 8_000_000;

const docsReady = (env: Env) =>
  Boolean(env.GOOGLE_DOCS_CLIENT_ID && env.GOOGLE_DOCS_CLIENT_SECRET && env.GOOGLE_DOCS_REFRESH_TOKEN);

let docsCached: { token: string; until: number } | null = null;

/** The script Docs account's access token. It never leaves the relay. */
async function docsToken(env: Env): Promise<string> {
  if (docsCached && docsCached.until > Date.now()) return docsCached.token;
  const answer = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      client_id: env.GOOGLE_DOCS_CLIENT_ID!,
      client_secret: env.GOOGLE_DOCS_CLIENT_SECRET!,
      refresh_token: env.GOOGLE_DOCS_REFRESH_TOKEN!,
      grant_type: "refresh_token",
    }),
  });
  const body = (await answer.json().catch(() => ({}))) as { access_token?: string; expires_in?: number };
  if (!answer.ok || !body.access_token) throw new Problem(502, "Google refused the relay's Docs sign-in. Its refresh token may need renewing.");
  docsCached = { token: body.access_token, until: Date.now() + ((body.expires_in ?? 3600) - 300) * 1000 };
  return body.access_token;
}

class Problem extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

async function google(env: Env, path: string, init: RequestInit = {}): Promise<Response> {
  const url = new URL(path, DRIVE);
  if (url.hostname !== "www.googleapis.com") throw new Problem(500, "Unexpected Google API host.");
  const headers = new Headers(init.headers);
  headers.set("authorization", `Bearer ${await docsToken(env)}`);
  if (init.body && !headers.has("content-type")) headers.set("content-type", "application/json");
  const response = await fetch(url, { ...init, headers });
  if (!response.ok) {
    const reason = ((await response.json().catch(() => ({}))) as { error?: { message?: string } }).error?.message ?? "";
    throw new Problem(response.status === 404 ? 404 : 502, `Google ${response.status}: ${reason.slice(0, 300) || "request failed"}`);
  }
  return response;
}

const quoted = (value: string) => value.replace(/\\/g, "\\\\").replace(/'/g, "\\'");

/** One folder in the owner's Drive holds every exported script. */
async function docsFolder(env: Env): Promise<string> {
  const params = new URLSearchParams({
    q: `name = '${DOCS_FOLDER}' and mimeType = '${FOLDER_MIME}' and 'root' in parents and trashed = false`,
    fields: "files(id)",
    pageSize: "1",
  });
  const found = (await (await google(env, `files?${params}`)).json()) as { files?: { id: string }[] };
  if (found.files?.[0]?.id) return found.files[0].id;
  const made = (await (await google(env, "files?fields=id", {
    method: "POST",
    body: JSON.stringify({ name: DOCS_FOLDER, mimeType: FOLDER_MIME }),
  })).json()) as { id: string };
  return made.id;
}

/** Anyone with the link can edit; nobody can find it by searching, as Repack shares. */
async function share(env: Env, id: string) {
  await google(env, `files/${id}/permissions?fields=id&supportsAllDrives=true`, {
    method: "POST",
    body: JSON.stringify({ type: "anyone", role: "writer", allowFileDiscovery: false }),
  });
}

const scriptId = (value: unknown) => typeof value === "string" && /^[0-9a-f-]{8,80}$/i.test(value);

/**
 * A new Doc each time, never synced into an old one: Docs anchors comments
 * only when they arrive in an imported file, and numbers their anchors from
 * zero on every import (Repack's reasoning, script-documents.ts).
 */
async function exportDoc(request: Request, env: Env): Promise<Response> {
  if (!docsReady(env)) return json(503, { error: "Google Docs is not configured on the relay yet." });
  const size = Number(request.headers.get("content-length"));
  if (!Number.isSafeInteger(size) || size <= 0) return json(411, { error: "Content-Length is required." });
  if (size > MAX_DOCX_BYTES * 1.4) return json(413, { error: "That script is too large to export." });
  const body = (await request.json().catch(() => null)) as { name?: unknown; scriptId?: unknown; draft?: unknown; docx?: unknown } | null;
  if (!body || typeof body.name !== "string" || !body.name.trim() || body.name.length > 300 || !scriptId(body.scriptId) || typeof body.docx !== "string") {
    return json(400, { error: "Send name, scriptId, draft and docx." });
  }
  try {
    const docx = Uint8Array.from(atob(body.docx), (char) => char.charCodeAt(0));
    if (docx.length > MAX_DOCX_BYTES) return json(413, { error: "That script is too large to export." });
    const draft = body.draft === true;
    const metadata = {
      name: draft ? `(DRAFT) ${body.name}` : body.name,
      mimeType: DOC_MIME,
      appProperties: { writerScript: body.scriptId, writerKind: draft ? "draft" : "script" },
      parents: [await docsFolder(env)],
    };
    const boundary = `writer-${crypto.randomUUID()}`;
    const made = (await (await google(env, "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,name,createdTime", {
      method: "POST",
      headers: { "content-type": `multipart/related; boundary=${boundary}` },
      body: new Blob([
        `--${boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n${JSON.stringify(metadata)}\r\n--${boundary}\r\nContent-Type: ${DOCX_MIME}\r\n\r\n`,
        docx,
        `\r\n--${boundary}--`,
      ]),
    })).json()) as { id: string; name: string; createdTime?: string };
    await share(env, made.id);
    return json(200, {
      id: made.id,
      url: `https://docs.google.com/document/d/${made.id}/edit`,
      name: made.name,
      draft,
      createdAt: made.createdTime ?? null,
      state: "live",
    });
  } catch (error) {
    return problem(error);
  }
}

/** Every Doc exported from a script, newest first, as Drive has them now. */
async function listDocs(script: string, env: Env): Promise<Response> {
  if (!docsReady(env)) return json(503, { error: "Google Docs is not configured on the relay yet." });
  if (!scriptId(script)) return json(400, { error: "Which script?" });
  try {
    const params = new URLSearchParams({
      q: `'${await docsFolder(env)}' in parents and mimeType = '${DOC_MIME}' and appProperties has { key='writerScript' and value='${quoted(script)}' }`,
      fields: "files(id,name,createdTime,trashed,appProperties)",
      orderBy: "createdTime desc",
      pageSize: "200",
    });
    const found = (await (await google(env, `files?${params}`)).json()) as {
      files?: { id: string; name: string; createdTime?: string; trashed?: boolean; appProperties?: Record<string, string> }[];
    };
    return json(
      200,
      (found.files ?? []).map((file) => ({
        id: file.id,
        url: `https://docs.google.com/document/d/${file.id}/edit`,
        name: file.name,
        draft: file.appProperties?.writerKind === "draft",
        createdAt: file.createdTime ?? null,
        state: file.trashed ? "trashed" : "live",
      })),
    );
  } catch (error) {
    return problem(error);
  }
}

/**
 * The comments on a Doc, only when it is one this script exported: the
 * relay's Docs access reaches the whole Drive, a client token only these.
 */
async function docComments(script: string, doc: string, env: Env): Promise<Response> {
  if (!docsReady(env)) return json(503, { error: "Google Docs is not configured on the relay yet." });
  if (!scriptId(script) || !/^[A-Za-z0-9_-]{10,200}$/.test(doc)) return json(400, { error: "Which script and Doc?" });
  try {
    const file = (await (await google(env, `files/${doc}?fields=appProperties,parents`)).json()) as {
      appProperties?: Record<string, string>;
      parents?: string[];
    };
    if (file.appProperties?.writerScript !== script || !file.parents?.includes(await docsFolder(env))) {
      return json(404, { error: "That Doc is not one of this script's exports." });
    }
    const comments: unknown[] = [];
    let page = "";
    do {
      const params = new URLSearchParams({
        fields: "nextPageToken,comments(id,author(displayName),content,quotedFileContent(value),resolved,deleted,createdTime,replies(author(displayName),content,deleted))",
        pageSize: "100",
        includeDeleted: "false",
        ...(page ? { pageToken: page } : {}),
      });
      const found = (await (await google(env, `files/${doc}/comments?${params}`)).json()) as { comments?: unknown[]; nextPageToken?: string };
      comments.push(...(found.comments ?? []));
      page = found.nextPageToken ?? "";
    } while (page && comments.length < 2000);
    return json(200, comments);
  } catch (error) {
    return problem(error);
  }
}

const problem = (error: unknown) =>
  error instanceof Problem ? json(error.status, { error: error.message }) : json(502, { error: `Google Docs failed: ${String(error).slice(0, 200)}` });

/**
 * A multipart/form-data body that streams `audio` between text fields, with an
 * exact Content-Length (FixedLengthStream) so nothing is buffered.
 */
function multipart(
  audio: ReadableStream<Uint8Array>,
  audioSize: number,
  audioType: string,
  fields: [string, string][],
) {
  const boundary = `writer-${crypto.randomUUID()}`;
  const encoder = new TextEncoder();
  const head = encoder.encode(
    fields
      .map(([name, value]) => `--${boundary}\r\nContent-Disposition: form-data; name="${name}"\r\n\r\n${value}\r\n`)
      .join("") +
      `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="audio.ogg"\r\nContent-Type: ${audioType}\r\n\r\n`,
  );
  const tail = encoder.encode(`\r\n--${boundary}--\r\n`);
  const { readable, writable } = new FixedLengthStream(head.length + audioSize + tail.length);
  const writer = writable.getWriter();
  (async () => {
    await writer.write(head);
    const reader = audio.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      await writer.write(value);
    }
    await writer.write(tail);
    await writer.close();
  })().catch((error) => writer.abort(error));
  return { body: readable, contentType: `multipart/form-data; boundary=${boundary}` };
}

/**
 * The Trello lists scripts come from, as Repack's workspaces name them. Only
 * these are read, and nothing is ever written to Trello.
 */
const TRELLO_LISTS: Record<string, { id: string; name: string }> = {
  // Raw Channels board: one case a card, footage in a Drive folder.
  raw: { id: "69eb256a4a7d292ca721520a", name: "Raw Channels · Script (Sphe)" },
  // Interrogation board: a compilation a card, its cases' links in the description.
  comp: { id: "6abfd240d4c0d819f06458e9", name: "Interrogation · Ready for script (comp)" },
};

/**
 * One card by its link's id (trello.com/c/<id>), from any board Repack's
 * Trello token can read, with everything a script's links can be in: its
 * description, attachments, checklist items and comments. Read only.
 */
/** Keep the attachment's label: it can carry its case number or reuse marker. */
export function attachmentLabels(items: { name?: string; url?: string }[]) {
  return items.filter((item) => item.url?.startsWith("http")).map((item) => `${(item.name ?? "").replace(/[\r\n]/g, " ")} <${item.url}>`);
}

async function trelloCard(card: string, env: Env): Promise<Response> {
  if (!env.TRELLO_API_KEY || !env.TRELLO_TOKEN) return json(503, { error: "Trello is not configured on the relay yet." });
  if (!/^[A-Za-z0-9]{6,32}$/.test(card)) return json(400, { error: "That isn't a Trello card link." });
  const query = new URLSearchParams({
    fields: "name,desc,url,dateLastActivity,labels,idBoard",
    attachments: "true",
    attachment_fields: "name,url",
    checklists: "all",
    checklist_fields: "name",
    actions: "commentCard",
    actions_limit: "100",
    key: env.TRELLO_API_KEY,
    token: env.TRELLO_TOKEN,
  });
  const answer = await fetch(`https://api.trello.com/1/cards/${card}?${query}`, { signal: AbortSignal.timeout(20_000) }).catch(() => null);
  if (!answer) return json(502, { error: "Trello did not answer." });
  if (answer.status === 404 || answer.status === 401) return json(404, { error: "That card isn't on a board Writer can read (Repack's Trello)." });
  if (!answer.ok) return json(502, { error: `Trello answered ${answer.status}.` });
  type Found = {
    id: string; name: string; desc?: string; url: string; dateLastActivity?: string;
    labels?: { name?: string }[]; attachments?: { name?: string; url?: string }[];
    checklists?: { checkItems?: { name?: string }[] }[];
    actions?: { data?: { text?: string } }[];
  };
  const found = (await answer.json()) as Found;
  return json(200, {
    id: found.id,
    name: found.name,
    desc: found.desc ?? "",
    url: found.url,
    activity: found.dateLastActivity ?? null,
    labels: (found.labels ?? []).map((label) => label.name ?? "").filter(Boolean),
    attachmentLabels: attachmentLabels(found.attachments ?? []),
    attachments: (found.attachments ?? []).map((item) => item.url ?? "").filter((link) => link.startsWith("http")),
    checklists: (found.checklists ?? []).flatMap((list) => (list.checkItems ?? []).map((item) => item.name ?? "")).filter(Boolean),
    comments: (found.actions ?? []).map((action) => action.data?.text ?? "").filter(Boolean),
  });
}

async function trelloCards(list: string, env: Env): Promise<Response> {
  if (!env.TRELLO_API_KEY || !env.TRELLO_TOKEN) return json(503, { error: "Trello is not configured on the relay yet." });
  const wanted = TRELLO_LISTS[list];
  if (!wanted) return json(400, { error: `Choose a list: ${Object.keys(TRELLO_LISTS).join(" or ")}.` });
  const query = new URLSearchParams({
    fields: "name,desc,url,dateLastActivity,labels",
    attachments: "true",
    attachment_fields: "name,url",
    key: env.TRELLO_API_KEY,
    token: env.TRELLO_TOKEN,
  });
  const answer = await fetch(`https://api.trello.com/1/lists/${wanted.id}/cards?${query}`, { signal: AbortSignal.timeout(20_000) }).catch(() => null);
  if (!answer) return json(502, { error: "Trello did not answer." });
  if (!answer.ok) return json(502, { error: `Trello answered ${answer.status}.` });
  type Card = { id: string; name: string; desc?: string; url: string; dateLastActivity?: string; labels?: { name?: string }[]; attachments?: { name?: string; url?: string }[] };
  const cards = (await answer.json()) as Card[];
  return json(200, {
    list: wanted.name,
    cards: cards.map((card) => ({
      id: card.id,
      name: card.name,
      desc: card.desc ?? "",
      url: card.url,
      activity: card.dateLastActivity ?? null,
      labels: (card.labels ?? []).map((label) => label.name ?? "").filter(Boolean),
      attachmentLabels: attachmentLabels(card.attachments ?? []),
      attachments: (card.attachments ?? []).map((item) => item.url ?? "").filter((link) => link.startsWith("http")),
    })),
  });
}

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

/** A daily cap in words: hours of audio (32 kbps Opus, ~14.4 MB an hour) or watcher calls. */
export function amount(cap: number, calls: boolean): string {
  return calls ? `${cap} clips` : `about ${Math.round(cap / 14_400_000)} hours of audio`;
}

/** One object per UTC day. Reserve before calling Scribe: simultaneous
 * requests cannot spend the same allowance. A failed request keeps its
 * reservation, because a network error cannot prove Scribe did not bill. */
interface BudgetStorage {
  get<T>(key: string): Promise<T | undefined>;
  put(key: string, value: number): Promise<void>;
  transaction<T>(callback: (storage: BudgetStorage) => Promise<T>): Promise<T>;
  setAlarm(time: number): Promise<void>;
  deleteAll(): Promise<void>;
}
interface BudgetState {
  storage: BudgetStorage;
}
export class TranscriptionBudget {
  constructor(
    private state: BudgetState,
    private env: Env,
  ) {}
  async fetch(request: Request): Promise<Response> {
    const { client, size, pool } = (await request.json()) as { client: string; size: number; pool?: string };
    if (!/^[a-f0-9]{64}$/.test(client) || !Number.isSafeInteger(size) || size <= 0 || size > Number(this.env.MAX_UPLOAD_BYTES)) {
      return json(400, { error: "Invalid budget reservation." });
    }
    // Transcription counts bytes of audio; the watcher counts calls.
    const watch = pool === "watch";
    const [perPerson, total, what] = watch
      ? [Number(this.env.WATCH_DAILY_PER_PERSON), Number(this.env.WATCH_DAILY_TOTAL), "watching"]
      : [Number(this.env.DAILY_BYTES_PER_IP), Number(this.env.DAILY_BYTES_TOTAL), "transcription"];
    if (!(perPerson > 0 && total > 0)) return json(503, { error: `The ${what} budget is misconfigured.` });
    const answer = await this.state.storage.transaction(async (storage) => {
      const ipKey = `ip:${client}`;
      const ipUsed = (await storage.get<number>(ipKey)) ?? 0;
      const allUsed = (await storage.get<number>("all")) ?? 0;
      // Which cap was hit, said plainly ("allowance" is what Writer's hearing.rs
      // looks for to stop asking again until tomorrow).
      if (ipUsed + size > perPerson) {
        return json(429, { cap: "person", error: `Your daily ${what} allowance (${amount(perPerson, watch)} per person) is used up; it resets at midnight UTC.` });
      }
      if (allUsed + size > total) {
        return json(429, { cap: "team", error: `The team's shared daily ${what} allowance (${amount(total, watch)} across everyone) is used up; it resets at midnight UTC.` });
      }
      await storage.put(ipKey, ipUsed + size);
      await storage.put("all", allUsed + size);
      return json(200, { ok: true });
    });
    await this.state.storage.setAlarm(Date.now() + 2 * 86400_000);
    return answer;
  }

  async alarm() {
    await this.state.storage.deleteAll();
  }
}
