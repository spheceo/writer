import { createRemoteJWKSet, type JWTVerifyGetKey, jwtVerify } from "jose";

/**
 * Who is asking. Writer signs people in at accounts.spheceo.com, and sends
 * their access token (an RS256 JWT, an hour long) on every request: checked
 * here against the accounts site's public keys, its issuer, `aud: "writer"`
 * and expiry (60 s of skew), then the route's permission from its `perms`.
 *
 * While older Writers are still about, the team-wide `CLIENT_TOKEN` built
 * into them is accepted too, compared in constant time, with every
 * permission. It goes once everyone is on a build that signs in.
 */

export interface AuthEnv {
  /** The old team-wide token, accepted for the transition only. */
  CLIENT_TOKEN?: string;
  /** The accounts site: the tokens' issuer, and where its JWKS is. */
  ACCOUNTS_URL?: string;
}

export type Permission = "use" | "ai" | "drive" | "trello" | "proxy" | "cloud";

export type Caller =
  | { kind: "person"; sub: string; email: string; perms: Set<string>; admin: boolean }
  | { kind: "legacy" };

const ISSUER = "https://accounts.spheceo.com";
const AUDIENCE = "writer";

/** What each route needs. `/health` needs nothing; an unknown route, `use`. */
export function permissionFor(pathname: string): Permission {
  if (pathname.startsWith("/v1/cloud/")) return "cloud";
  if (pathname === "/v1/watch") return "ai";
  if (pathname === "/v1/drive/token" || pathname.startsWith("/v1/docs/")) return "drive";
  if (pathname === "/v1/trello/cards") return "trello";
  if (pathname === "/v1/proxy") return "proxy";
  return "use";
}

export function allowed(caller: Caller, permission: Permission): boolean {
  return caller.kind === "legacy" || caller.admin || caller.perms.has(permission);
}

/** The keys, per isolate: jose caches them ten minutes and refetches once on an unknown `kid`. */
let keys: { url: string; get: JWTVerifyGetKey } | null = null;
/** At most one refetch per 30 s, however many unknown `kid`s come in. */
let cooldown = 30_000;

function keySet(issuer: string): JWTVerifyGetKey {
  const url = `${issuer}/.well-known/jwks.json`;
  if (keys?.url !== url) {
    keys = { url, get: createRemoteJWKSet(new URL(url), { cacheMaxAge: 10 * 60_000, cooldownDuration: cooldown, timeoutDuration: 5_000 }) };
  }
  return keys.get;
}

/** For tests: forget the cached keys, and refetch on an unknown `kid` after `ms`. */
export function forgetKeys(ms = 30_000) {
  keys = null;
  cooldown = ms;
}

export type Identified = { caller: Caller } | { refused: string };

/** The caller, or why they're refused (no token, a bad one, an expired one). */
export async function identify(request: Request, env: AuthEnv): Promise<Identified> {
  const header = request.headers.get("authorization") ?? "";
  if (!/^Bearer \S+$/.test(header)) return { refused: "Sign in to Writer." };
  const token = header.slice(7);
  if (env.CLIENT_TOKEN && (await sameText(token, env.CLIENT_TOKEN))) return { caller: { kind: "legacy" } };
  if (token.split(".").length !== 3) return { refused: "This copy of Writer's token isn't accepted. Sign in to Writer." };
  const issuer = (env.ACCOUNTS_URL || ISSUER).replace(/\/+$/, "");
  try {
    const { payload } = await jwtVerify(token, keySet(issuer), {
      issuer,
      audience: AUDIENCE,
      algorithms: ["RS256"],
      clockTolerance: 60,
      requiredClaims: ["sub", "exp"],
    });
    const perms = Array.isArray(payload.perms) ? payload.perms.filter((perm): perm is string => typeof perm === "string") : [];
    return {
      caller: {
        kind: "person",
        sub: String(payload.sub),
        email: typeof payload.email === "string" ? payload.email : "",
        perms: new Set(perms),
        admin: payload.admin === true,
      },
    };
  } catch (error) {
    const code = (error as { code?: string }).code ?? "";
    if (code === "ERR_JWT_EXPIRED") return { refused: "Your sign-in has expired. Sign in to Writer again." };
    if (code === "ERR_JWKS_TIMEOUT" || code === "ERR_JOSE_GENERIC") return { refused: "Couldn't check your sign-in with accounts.spheceo.com; try again in a minute." };
    return { refused: "Your sign-in isn't valid. Sign in to Writer again." };
  }
}

/** Who to name in the log: the person's id and their email's first part, never the token. */
export function named(caller: Caller): string {
  return caller.kind === "legacy" ? "old team token" : `${caller.email.split("@")[0] || "?"} (${caller.sub})`;
}

/** A key for per-person limits: the person, or for the old token, the connection. */
export function limitKey(caller: Caller, request: Request): string {
  return caller.kind === "person" ? `person:${caller.sub}` : (request.headers.get("cf-connecting-ip") ?? "unknown");
}

/** Equal, in constant time: both sides hashed first, so their lengths don't show. */
async function sameText(a: string, b: string): Promise<boolean> {
  const encode = (text: string) => crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  const [x, y] = (await Promise.all([encode(a), encode(b)])).map((digest) => new Uint8Array(digest));
  let difference = 0;
  for (let i = 0; i < x.length; i++) difference |= x[i] ^ y[i];
  return difference === 0;
}
