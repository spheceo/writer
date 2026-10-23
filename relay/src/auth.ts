import { createRemoteJWKSet, type JWTVerifyGetKey, jwtVerify } from "jose";

/**
 * Who is asking. Writer signs people in at accounts.spheceo.com, and sends
 * their access token (an RS256 JWT, an hour long) on every request: checked
 * here against the accounts site's public keys, its issuer, `aud: "writer"`
 * and expiry (60 s of skew), then `use` in its `perms` (or `admin`): every
 * route needs only that (contract v2). What kinds of work someone may do
 * (manifest.ts) Writer checks itself; ids the relay doesn't know, v1's
 * `ai`, `cloud`… included, are simply ignored.
 *
 * Nothing else passes. The team-wide token older Writers had built in was
 * retired on 2026-10-23; they're told to update and sign in.
 */

export interface AuthEnv {
  /** The accounts site: the tokens' issuer, and where its JWKS is. */
  ACCOUNTS_URL?: string;
}

export type Caller = { sub: string; email: string; perms: Set<string>; admin: boolean };

const ISSUER = "https://accounts.spheceo.com";
const AUDIENCE = "writer";

/** May use Writer at all: every route needs this and nothing more. */
export function mayUse(caller: Caller): boolean {
  return caller.admin || caller.perms.has("use");
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

/** What an older Writer, still sending the retired team token, is told. */
export const OUT_OF_DATE = "This copy of Writer is out of date. Update Writer (the update button, or the latest download at github.com/spheceo/writer/releases), then sign in.";

/** The caller, or why they're refused (no token, a bad one, an expired one). */
export async function identify(request: Request, env: AuthEnv): Promise<Identified> {
  const header = request.headers.get("authorization") ?? "";
  if (!/^Bearer \S+$/.test(header)) return { refused: "Sign in to Writer." };
  const token = header.slice(7);
  // Not a JWT: an older Writer's retired team token.
  if (token.split(".").length !== 3) return { refused: OUT_OF_DATE };
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
  return `${caller.email.split("@")[0] || "?"} (${caller.sub})`;
}

/** A key for per-person limits. */
export function limitKey(caller: Caller): string {
  return `person:${caller.sub}`;
}
