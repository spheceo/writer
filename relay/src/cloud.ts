/**
 * Writer's cloud, as Edit's: every script's body and files on Cloudflare R2
 * under `writer/`, and its index in Convex (Repack's deployment, the `writer`
 * module). The relay holds the R2 keys and Convex's service token; Writer only
 * ever gets short-lived presigned URLs and the answers of `writer:` functions.
 *
 * Every route is POST JSON, from someone who may use Writer (index.ts checks
 * who's asking before routing here). Reading is open to them; writing needs
 * script work (a `scripts.*` capability: a commit or delete the script's own
 * kind), the shared lessons `review.lessons`, and reviews `review.share`.
 * The bucket is shared with Repack and Edit, so keys must sit under `writer/`.
 *
 *   /v1/cloud/r2/url                {key, method: GET|PUT|HEAD, expires?} → {url}
 *   /v1/cloud/r2/multipart/create   {key, contentType?}                    → {uploadId}
 *   /v1/cloud/r2/multipart/urls     {key, uploadId, parts: [n…]}          → {urls: {n: url}}
 *   /v1/cloud/r2/multipart/complete {key, uploadId, parts: [{partNumber, etag}]} → {ok, size?, etag?}
 *   /v1/cloud/r2/multipart/abort    {key, uploadId}                        → {ok}
 *   /v1/cloud/r2/head               {key}                                  → {exists, size?, etag?}
 *   /v1/cloud/r2/delete             {keys: [...]}  (at most 40)            → {ok, deleted}
 *   /v1/cloud/r2/list               {prefix, cursor?}  (a page of ≤1000)   → {objects: [{key, size}], cursor?}
 *   /v1/cloud/convex                {kind: query|mutation, path: "writer:…", args?}
 *                                   → Convex's answer ({status: "success", value} | {status: "error", errorMessage})
 *
 * Beyond `writer/` and `writer:`, two shared things, for sharing and the
 * handoff to Edit:
 *   - Repack's reviews (`reviews:*`), always as Writer: `app` is forced to
 *     "writer" whatever the caller sends.
 *   - Edit's footage registry: `edit:getMedia` and `edit:putMedia` only, and
 *     the keys under `edit/originals/` and `edit/analysis/` (presigned URLs,
 *     multipart and head; never delete or list, which stay Writer's own).
 */

export interface CloudEnv {
  /** "1" once Repack's writer:commitScript takes `actor` (it rejects arguments it doesn't know). */
  STAMP_CREATOR?: string;
  R2_ACCOUNT_ID?: string;
  R2_ACCESS_KEY_ID?: string;
  R2_SECRET_ACCESS_KEY?: string;
  R2_BUCKET?: string;
  /** The Convex deployment (https://….convex.cloud), shared with Repack. */
  CONVEX_URL?: string;
  /** Passed as `args.token` to every `writer:` function, which checks it. */
  CONVEX_SERVICE_TOKEN?: string;
}

const HOUR = 3600;
/** A presigned URL is checked when a transfer starts, so an hour covers any one. */
const MAX_EXPIRES = HOUR;
const DEFAULT_EXPIRES: Record<string, number> = { GET: HOUR, PUT: HOUR, HEAD: HOUR };
/** Subrequests per invocation are limited (50 on the free plan). */
const MAX_DELETE = 40;
const MAX_PART_URLS = 1000;
const MAX_BODY = 8 * 1024 * 1024;
const CONVEX_KINDS = new Set(["query", "mutation", "action"]);
/**
 * The functions Writer calls, and what each needs beyond `use` (manifest.ts):
 * `script` = the work the script is (its kind, from its summary), `any` = some
 * kind of script work, an id = that capability. Nothing else is reachable.
 */
const FUNCTIONS: Record<string, "read" | "script" | "any" | string> = {
  "writer:listScripts": "read",
  "writer:getScript": "read",
  "writer:commitScript": "script",
  "writer:deleteScript": "script",
  "writer:clearRestore": "any",
  "reviews:open": "read",
  "reviews:changes": "read",
  "reviews:publish": "review.share",
  "reviews:approve": "review.share",
  "reviews:unapprove": "review.share",
  "reviews:claim": "review.share",
  "reviews:release": "review.share",
  "reviews:resolve": "review.share",
  "reviews:reply": "review.share",
  "reviews:note": "review.share",
  "reviews:deleteNote": "review.share",
  // Fixing on the one computer chosen on accounts' Fixing page (accounts/docs/fixing.md).
  "reviews:checkIn": "read",
  "reviews:takeFix": "review.share",
  "reviews:fixProgress": "review.share",
  // Synced with the review page: a fix checked from Writer, and one review read whole (its notes, fix and computer).
  "reviews:check": "review.share",
  "reviews:get": "read",
  // A shared version's page data replaced in place: the script fixed and shown as the same version.
  "reviews:setMeta": "review.share",
  "edit:getMedia": "read",
  "edit:putMedia": "any",
};
const REVIEWS_PATH = /^reviews:/;
export const callable = (path: string) => Object.hasOwn(FUNCTIONS, path);
/** The shared lessons every commentary build reads: only someone who may teach Writer writes them. */
const LESSONS_KEY = "writer/lessons.json";
/** Edit's prefixes Writer may read and write, for footage and transcripts it shares. */
const SHARED_PREFIXES = ["edit/originals/", "edit/analysis/"];

/** Who started a script, from the verified sign-in: what commitScript records on its first commit. */
export type Actor = { sub: string; name: string; email: string; picture?: string };

/** What the caller was granted, from their verified token. */
export type Grants = { admin: boolean; perms: Set<string> };
const ALL: Grants = { admin: true, perms: new Set() };
const has = (grants: Grants, id: string) => grants.admin || grants.perms.has(id);
const anyWork = (grants: Grants) => grants.admin || [...grants.perms].some((perm) => perm.startsWith("scripts."));

/** The work a script is, from its summary, as Writer's can.rs reads it. */
export function workOf(summary: unknown): string | null {
  if (!isObject(summary)) return null;
  if (summary.sourceKind === "commentary") return "scripts.commentary";
  const kind = typeof summary.kind === "string" ? summary.kind : "interrogation";
  if (summary.compilation === true) return kind === "courtroom" ? "scripts.courtroom-compilation" : kind === "foia" ? "scripts.foia-compilation" : "scripts.interrogation-compilation";
  return ["interrogation", "foia", "courtroom"].includes(kind) ? `scripts.${kind}` : null;
}

const refuse = (what: string) => json(403, { error: `Your account can't ${what}. Ask an admin at accounts.spheceo.com.` });

export async function cloud(request: Request, url: URL, env: CloudEnv, actor?: Actor, grants: Grants = ALL): Promise<Response> {
  if (request.method !== "POST") return json(405, { error: `POST JSON to ${url.pathname}` });
  if (Number(request.headers.get("content-length") ?? 0) > MAX_BODY) return json(413, { error: "Request too large." });
  let body: Record<string, unknown>;
  try {
    const parsed: unknown = await request.json();
    if (!isObject(parsed)) throw new Error("not an object");
    body = parsed;
  } catch {
    return json(400, { error: "Send a JSON object." });
  }
  const route = url.pathname.slice("/v1/cloud/".length);
  if (route === "convex") return convex(body, env, actor, grants);
  const handler = route.startsWith("r2/") && Object.hasOwn(R2_ROUTES, route.slice(3)) ? R2_ROUTES[route.slice(3)] : undefined;
  if (!handler) return json(404, { error: "not found" });
  // Reading is everyone's; writing needs script work, and the lessons the teaching capability.
  const writes = route === "r2/delete" || route.startsWith("r2/multipart/") || (route === "r2/url" && body.method === "PUT");
  if (writes) {
    const keys = route === "r2/delete" ? (Array.isArray(body.keys) ? body.keys : []) : [body.key];
    if (keys.includes(LESSONS_KEY) && !has(grants, "review.lessons")) return refuse("change Writer's shared lessons");
    if (!anyWork(grants)) return refuse("write scripts");
  }
  const r2 = r2Config(env);
  if (!r2) return json(503, { error: "Cloud storage is not configured on the relay yet." });
  try {
    return await handler(body, r2);
  } catch (error) {
    if (error instanceof BadRequest) return json(400, { error: error.message });
    if (error instanceof Upstream) return json(502, { error: error.message });
    throw error;
  }
}

/** The actor as Convex takes it: no empty picture. */
export function stamp(actor: Actor): Actor {
  return actor.picture ? actor : { sub: actor.sub, name: actor.name, email: actor.email };
}

/** A key Writer may touch: under `writer/`, no empty, `.` or `..` segments, no control characters. */
export function validKey(key: unknown): key is string {
  if (typeof key !== "string" || !key.startsWith("writer/")) return false;
  return wellFormed(key);
}

/** A key Writer may read or write but never delete: its own, or Edit's shared footage and transcripts. */
export function sharedKey(key: unknown): key is string {
  if (typeof key !== "string" || !(key.startsWith("writer/") || SHARED_PREFIXES.some((prefix) => key.startsWith(prefix)))) return false;
  return wellFormed(key);
}

function wellFormed(key: string): boolean {
  if (key.length > 1024 || /[\u0000-\u001f\u007f\\]/.test(key)) return false;
  return key.split("/").every((segment) => segment !== "" && segment !== "." && segment !== "..");
}

/** A listing prefix: `writer/` itself or a key-like prefix under it (may end in `/`). */
export function validPrefix(prefix: unknown): prefix is string {
  if (typeof prefix !== "string") return false;
  if (prefix === "writer/") return true;
  return validKey(prefix.endsWith("/") ? prefix.slice(0, -1) : prefix);
}

// --- R2 (S3 API, SigV4) -------------------------------------------------------

export interface R2 { host: string; bucket: string; accessKeyId: string; secret: string }

export function r2Config(env: CloudEnv): R2 | null {
  if (!env.R2_ACCOUNT_ID || !env.R2_ACCESS_KEY_ID || !env.R2_SECRET_ACCESS_KEY || !env.R2_BUCKET) return null;
  return { host: `${env.R2_ACCOUNT_ID}.r2.cloudflarestorage.com`, bucket: env.R2_BUCKET, accessKeyId: env.R2_ACCESS_KEY_ID, secret: env.R2_SECRET_ACCESS_KEY };
}

export class BadRequest extends Error {}
export class Upstream extends Error {}

const R2_ROUTES: Record<string, (body: Record<string, unknown>, r2: R2) => Promise<Response>> = {
  async url(body, r2) {
    const key = needSharedKey(body.key);
    const method = body.method === undefined ? "GET" : body.method;
    if (method !== "GET" && method !== "PUT" && method !== "HEAD") throw new BadRequest("method must be GET, PUT or HEAD.");
    let expires = DEFAULT_EXPIRES[method];
    if (body.expires !== undefined) {
      if (typeof body.expires !== "number" || !Number.isInteger(body.expires) || body.expires <= 0) throw new BadRequest("expires must be a whole number of seconds.");
      expires = Math.min(body.expires, MAX_EXPIRES);
    }
    return json(200, { url: await objectUrl(r2, method, key, {}, expires), expires });
  },

  async "multipart/create"(body, r2) {
    const key = needSharedKey(body.key);
    const headers: Record<string, string> = {};
    if (body.contentType !== undefined) {
      if (typeof body.contentType !== "string" || !/^[\w.+-]+\/[\w.+-]+(; ?[\w.+-]+=[\w.+-]+)*$/.test(body.contentType) || body.contentType.length > 200) throw new BadRequest("Invalid contentType.");
      headers["content-type"] = body.contentType;
    }
    const answer = await r2Fetch(r2, "POST", key, { uploads: "" }, { headers });
    const uploadId = xmlValue(await answer.text(), "UploadId");
    if (!uploadId) throw new Upstream("R2 gave no upload id.");
    return json(200, { uploadId });
  },

  async "multipart/urls"(body, r2) {
    const key = needSharedKey(body.key);
    const uploadId = needUploadId(body.uploadId);
    if (!Array.isArray(body.parts) || body.parts.length === 0 || body.parts.length > MAX_PART_URLS) throw new BadRequest(`parts must list 1 to ${MAX_PART_URLS} part numbers.`);
    const urls: Record<string, string> = {};
    for (const part of body.parts) {
      urls[String(needPartNumber(part))] = await objectUrl(r2, "PUT", key, { partNumber: String(part), uploadId }, DEFAULT_EXPIRES.PUT);
    }
    return json(200, { urls });
  },

  async "multipart/complete"(body, r2) {
    const key = needSharedKey(body.key);
    const uploadId = needUploadId(body.uploadId);
    if (!Array.isArray(body.parts) || body.parts.length === 0 || body.parts.length > 10000) throw new BadRequest("parts must list the uploaded parts.");
    const parts = body.parts.map((part: unknown) => {
      if (!isObject(part) || typeof part.etag !== "string" || !part.etag || part.etag.length > 200) throw new BadRequest("Each part needs partNumber and etag.");
      return { number: needPartNumber(part.partNumber), etag: part.etag };
    }).sort((a, b) => a.number - b.number);
    const xml = `<CompleteMultipartUpload>${parts.map((part) => `<Part><PartNumber>${part.number}</PartNumber><ETag>${escapeXml(part.etag)}</ETag></Part>`).join("")}</CompleteMultipartUpload>`;
    const answer = await r2Fetch(r2, "POST", key, { uploadId }, { body: xml, headers: { "content-type": "application/xml" } });
    const text = await answer.text();
    // S3 can answer 200 and still fail, with an <Error> body.
    if (text.includes("<Error>")) throw new Upstream(`R2: ${xmlValue(text, "Code") ?? "Error"}: ${xmlValue(text, "Message") ?? text.slice(0, 300)}`);
    const head = await r2Fetch(r2, "HEAD", key, {}, { allow404: true });
    const size = head.status === 200 ? Number(head.headers.get("content-length")) : undefined;
    return json(200, { ok: true, size, etag: unescapeXml(xmlValue(text, "ETag") ?? head.headers.get("etag") ?? "") || undefined });
  },

  async "multipart/abort"(body, r2) {
    const key = needSharedKey(body.key);
    await r2Fetch(r2, "DELETE", key, { uploadId: needUploadId(body.uploadId) }, { allow404: true });
    return json(200, { ok: true });
  },

  async head(body, r2) {
    const answer = await r2Fetch(r2, "HEAD", needSharedKey(body.key), {}, { allow404: true });
    if (answer.status === 404) return json(200, { exists: false });
    return json(200, { exists: true, size: Number(answer.headers.get("content-length")), etag: answer.headers.get("etag") ?? undefined, contentType: answer.headers.get("content-type") ?? undefined });
  },

  async delete(body, r2) {
    if (!Array.isArray(body.keys) || body.keys.length > MAX_DELETE) throw new BadRequest(`keys must list at most ${MAX_DELETE} keys.`);
    const keys = body.keys.map(needKey);
    await Promise.all(keys.map((key) => r2Fetch(r2, "DELETE", key, {}, { allow404: true })));
    return json(200, { ok: true, deleted: keys.length });
  },

  async list(body, r2) {
    if (!validPrefix(body.prefix)) throw new BadRequest("prefix must start with writer/.");
    const query: Record<string, string> = { "list-type": "2", prefix: body.prefix, "max-keys": "1000" };
    if (body.cursor !== undefined) {
      if (typeof body.cursor !== "string" || !body.cursor || body.cursor.length > 2048) throw new BadRequest("Invalid cursor.");
      query["continuation-token"] = body.cursor;
    }
    const text = await (await r2Fetch(r2, "GET", "", query)).text();
    const objects = [...text.matchAll(/<Contents>([\s\S]*?)<\/Contents>/g)].map(([, item]) => ({
      key: unescapeXml(xmlValue(item, "Key") ?? ""),
      size: Number(xmlValue(item, "Size") ?? 0),
    })).filter((object) => object.key.startsWith("writer/"));
    const truncated = xmlValue(text, "IsTruncated") === "true";
    const cursor = truncated ? unescapeXml(xmlValue(text, "NextContinuationToken") ?? "") || undefined : undefined;
    return json(200, { objects, cursor });
  },
};

export function needKey(key: unknown): string {
  if (!validKey(key)) throw new BadRequest("key must be a path under writer/ (no empty, . or .. segments).");
  return key;
}

export function needSharedKey(key: unknown): string {
  if (!sharedKey(key)) throw new BadRequest("key must be a path under writer/, edit/originals/ or edit/analysis/ (no empty, . or .. segments).");
  return key;
}

function needUploadId(id: unknown): string {
  if (typeof id !== "string" || !id || id.length > 1024 || /[\u0000-\u001f\u007f]/.test(id)) throw new BadRequest("Invalid uploadId.");
  return id;
}

function needPartNumber(part: unknown): number {
  if (typeof part !== "number" || !Number.isInteger(part) || part < 1 || part > 10000) throw new BadRequest("Part numbers run from 1 to 10000.");
  return part;
}

/** A presigned URL for `key` in the bucket (path-style; `key` "" is the bucket). */
export function objectUrl(r2: R2, method: string, key: string, query: Record<string, string>, expires: number) {
  const path = `/${encodePath(r2.bucket)}${key ? `/${encodePath(key)}` : ""}`;
  return presign({ method, url: `https://${r2.host}${path}`, query, accessKeyId: r2.accessKeyId, secret: r2.secret, region: "auto", expires });
}

/** One S3 call from the relay itself, through a minute-long presigned URL. */
export async function r2Fetch(r2: R2, method: string, key: string, query: Record<string, string>, options: { body?: string; headers?: Record<string, string>; allow404?: boolean } = {}) {
  const answer = await fetch(await objectUrl(r2, method, key, query, 60), { method, body: options.body, headers: options.headers });
  if (answer.ok || (options.allow404 && answer.status === 404)) return answer;
  const text = method === "HEAD" ? "" : await answer.text();
  throw new Upstream(`R2 ${answer.status}: ${xmlValue(text, "Code") ?? ""} ${xmlValue(text, "Message") ?? text.slice(0, 300)}`.trim());
}

export interface PresignInput {
  method: string;
  /** Scheme, host and canonical (already encoded) path. */
  url: string;
  query?: Record<string, string>;
  accessKeyId: string;
  secret: string;
  region: string;
  service?: string;
  expires: number;
  date?: Date;
}

/** AWS SigV4 query-string signing (host header only, unsigned payload). */
export async function presign(input: PresignInput): Promise<string> {
  const target = new URL(input.url);
  const service = input.service ?? "s3";
  const stamp = (input.date ?? new Date()).toISOString().replace(/[-:]/g, "").replace(/\.\d{3}/, "");
  const day = stamp.slice(0, 8);
  const scope = `${day}/${input.region}/${service}/aws4_request`;
  const params: Record<string, string> = {
    ...input.query,
    "X-Amz-Algorithm": "AWS4-HMAC-SHA256",
    "X-Amz-Credential": `${input.accessKeyId}/${scope}`,
    "X-Amz-Date": stamp,
    "X-Amz-Expires": String(input.expires),
    "X-Amz-SignedHeaders": "host",
  };
  const query = Object.entries(params)
    .map(([key, value]) => [rfc3986(key), rfc3986(value)])
    .sort(([a, x], [b, y]) => (a < b ? -1 : a > b ? 1 : x < y ? -1 : x > y ? 1 : 0))
    .map(([key, value]) => `${key}=${value}`)
    .join("&");
  const canonical = [input.method, target.pathname, query, `host:${target.host}\n`, "host", "UNSIGNED-PAYLOAD"].join("\n");
  const toSign = ["AWS4-HMAC-SHA256", stamp, scope, hex(await sha256(canonical))].join("\n");
  let signing: ArrayBuffer = new TextEncoder().encode(`AWS4${input.secret}`).buffer as ArrayBuffer;
  for (const part of [day, input.region, service, "aws4_request"]) signing = await hmac(signing, part);
  const signature = hex(new Uint8Array(await hmac(signing, toSign)));
  return `${target.origin}${target.pathname}?${query}&X-Amz-Signature=${signature}`;
}

const rfc3986 = (value: string) => encodeURIComponent(value).replace(/[!'()*]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`);
const encodePath = (path: string) => path.split("/").map(rfc3986).join("/");

async function hmac(key: ArrayBuffer, data: string): Promise<ArrayBuffer> {
  const imported = await crypto.subtle.importKey("raw", key, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return crypto.subtle.sign("HMAC", imported, new TextEncoder().encode(data));
}

async function sha256(text: string) {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
}

const hex = (bytes: Uint8Array) => Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");

export function xmlValue(xml: string, tag: string): string | undefined {
  return xml.match(new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`))?.[1];
}

export const escapeXml = (text: string) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&apos;");
export const unescapeXml = (text: string) => text
  .replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, "\"").replace(/&apos;/g, "'")
  .replace(/&#(\d+);/g, (_, code) => String.fromCodePoint(Number(code)))
  .replace(/&#x([0-9a-fA-F]+);/g, (_, code) => String.fromCodePoint(parseInt(code, 16)))
  .replace(/&amp;/g, "&");

// --- Convex ---------------------------------------------------------------------

/** Calls a `writer:` function (or a `reviews:` one, as Writer, or Edit's footage registry) with the service token added to its args. */
async function convex(body: Record<string, unknown>, env: CloudEnv, actor?: Actor, grants: Grants = ALL): Promise<Response> {
  const kind = body.kind;
  if (typeof kind !== "string" || !CONVEX_KINDS.has(kind)) return json(400, { error: "kind must be query, mutation or action." });
  if (typeof body.path !== "string" || !callable(body.path)) {
    return json(403, { error: "That function isn't one Writer calls." });
  }
  if (body.args !== undefined && !isObject(body.args)) return json(400, { error: "args must be an object." });
  const needs = FUNCTIONS[body.path];
  if (needs === "any" && !anyWork(grants)) return refuse("write scripts");
  if (needs.includes(".") && !has(grants, needs)) return refuse(needs === "review.share" ? "share or fix reviews" : "do that");
  if (needs === "script" && !grants.admin) {
    const args = (body.args as Record<string, unknown> | undefined) ?? {};
    // A commit says what it is; a delete is checked against what the script is in the cloud.
    let summary = args.summary;
    if (body.path === "writer:deleteScript") {
      if (!env.CONVEX_URL || !env.CONVEX_SERVICE_TOKEN) return json(503, { error: "Convex is not configured on the relay yet." });
      const row = await convexValue(env, "query", "writer:getScript", { scriptId: args.scriptId });
      if (row === undefined) return json(502, { error: "Convex: couldn't look the script up." });
      summary = isObject(row) ? row.summary : null;
      if (row === null) summary = undefined;
    }
    const work = summary === undefined ? null : workOf(summary);
    if (summary !== undefined && !(work ? has(grants, work) : anyWork(grants))) return refuse("change scripts of that kind");
    if (summary === undefined && !anyWork(grants)) return refuse("write scripts");
  }
  // A review is always Writer's from here, whatever the caller says.
  if (REVIEWS_PATH.test(body.path)) body.args = { ...(body.args as object | undefined), app: "writer" };
  // Who started a script comes from the verified sign-in, never from the caller (commitScript keeps it on create only).
  if (body.path === "writer:commitScript") {
    const { actor: _sent, ...args } = (body.args as Record<string, unknown> | undefined) ?? {};
    body.args = env.STAMP_CREATOR === "1" && actor ? { ...args, actor: stamp(actor) } : args;
  }
  if (!env.CONVEX_URL || !env.CONVEX_SERVICE_TOKEN) return json(503, { error: "Convex is not configured on the relay yet." });
  let answer: Response;
  try {
    answer = await fetch(`${env.CONVEX_URL.replace(/\/+$/, "")}/api/${kind}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ path: body.path, args: { ...(body.args as object | undefined), token: env.CONVEX_SERVICE_TOKEN }, format: "json" }),
    });
  } catch (error) {
    return json(502, { error: `Convex: ${error instanceof Error ? error.message : String(error)}` });
  }
  const text = await answer.text();
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    parsed = undefined;
  }
  // Convex answered (the function ran, or isn't there): its own shape, as 200
  // ({status: "success", value} | {status: "error", errorMessage, errorData}).
  if (isObject(parsed) && parsed.status === "success") return json(200, { status: "success", value: parsed.value ?? null });
  if (isObject(parsed) && parsed.status === "error") {
    return json(200, { status: "error", errorMessage: String(parsed.errorMessage ?? "unknown error").trim(), errorData: parsed.errorData });
  }
  // Anything else (a request Convex refused as {code, message}, a gateway
  // error) comes back as a relay error with Convex's status.
  const message = isObject(parsed) && parsed.code
    ? `${parsed.code}: ${parsed.message ?? ""}`
    : text.slice(0, 500) || `HTTP ${answer.status}`;
  return json(answer.ok ? 502 : answer.status, { error: `Convex: ${message}` });
}

/** One Convex call's value as the relay itself (undefined when it failed). */
async function convexValue(env: CloudEnv, kind: string, path: string, args: Record<string, unknown>): Promise<unknown> {
  try {
    const answer = await fetch(`${env.CONVEX_URL!.replace(/\/+$/, "")}/api/${kind}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ path, args: { ...args, token: env.CONVEX_SERVICE_TOKEN }, format: "json" }),
    });
    const parsed: unknown = await answer.json();
    return isObject(parsed) && parsed.status === "success" ? (parsed.value ?? null) : undefined;
  } catch {
    return undefined;
  }
}

export function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
}
