/**
 * A stand-in for accounts.spheceo.com, to test Writer's sign-in and the
 * relay's check on one Mac before the real site is live. It speaks the
 * contract in accounts/docs/plan.md: /app/authorize (signs in "someone"
 * straight away and redirects to the loopback callback), /app/token (PKCE
 * checked; refresh tokens rotate, a dead one reused after 60 s ends the whole
 * chain), /app/revoke and the JWKS. Its keys are made fresh each run.
 *
 *   STUB_PORT=8788 STUB_PERMS=use,ai STUB_TTL=3600 bun relay/test/accounts-stub.ts
 *   STUB_DENY=1 answers every sign-in with access_denied.
 *   STUB_PICTURE=https://… puts a photo in the token's `picture` claim.
 *   GET /stub/mint?aud=writer&perms=use,cloud → an access token, for curl tests.
 *   POST /app/device: a code (RFC 8628), approved on the third poll of /app/token.
 *   GET /stub/limit?on=1 → /app/token and /app/revoke answer 429 (Retry-After: 60) until ?on=0.
 *
 * It never prints a token.
 */
import { exportJWK, generateKeyPair, SignJWT } from "jose";

const port = Number(process.env.STUB_PORT ?? 8788);
const issuer = `http://127.0.0.1:${port}`;
const ttl = Number(process.env.STUB_TTL ?? 3600);
const perms = (process.env.STUB_PERMS ?? "use,ai").split(",").filter(Boolean);
const { privateKey, publicKey } = await generateKeyPair("RS256");
const jwk = { ...(await exportJWK(publicKey)), kid: "stub-1", alg: "RS256", use: "sig" };

type Code = { challenge: string; redirect: string; client: string; until: number };
type Refresh = { chain: string; dead: boolean; diedAt?: number; next?: string };
const codes = new Map<string, Code>();
const refreshes = new Map<string, Refresh>();
const deadChains = new Set<string>();
const random = () => Buffer.from(crypto.getRandomValues(new Uint8Array(24))).toString("base64url");
const say = (text: string) => console.log(`[accounts-stub] ${text}`);
let limited = false;
const devices = new Map<string, { client: string; polls: number }>();

async function access(aud: string, granted = perms) {
  return new SignJWT({ email: "someone@gmail.com", name: "Someone", perms: granted, admin: false, ...(process.env.STUB_PICTURE ? { picture: process.env.STUB_PICTURE } : {}) })
    .setProtectedHeader({ alg: "RS256", kid: jwk.kid })
    .setIssuer(issuer)
    .setAudience(aud)
    .setSubject("person-stub-1")
    .setIssuedAt()
    .setExpirationTime(Math.floor(Date.now() / 1000) + ttl)
    .sign(privateKey);
}

async function pair(client: string, chain: string) {
  const refresh = random();
  refreshes.set(refresh, { chain, dead: false });
  return { access_token: await access(client), token_type: "Bearer", expires_in: ttl, refresh_token: refresh, user: { email: "someone@gmail.com", name: "Someone", image: process.env.STUB_PICTURE ?? null } };
}

const oauthError = (error: string, status = 400) => Response.json({ error }, { status });

Bun.serve({
  port,
  hostname: "127.0.0.1",
  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === "/.well-known/jwks.json") return Response.json({ keys: [jwk] });
    if (url.pathname === "/stub/mint") {
      return new Response(await access(url.searchParams.get("aud") ?? "writer", (url.searchParams.get("perms") ?? "").split(",").filter(Boolean)));
    }
    if (url.pathname === "/stub/limit") {
      limited = url.searchParams.get("on") === "1";
      say(`rate limit ${limited ? "on" : "off"}`);
      return new Response(limited ? "on" : "off");
    }
    if (limited && (url.pathname === "/app/token" || url.pathname === "/app/revoke")) {
      say(`429 for ${url.pathname}`);
      return Response.json({ error: "temporarily_unavailable", error_description: "Too many requests; try again in a minute." }, { status: 429, headers: { "retry-after": "60" } });
    }
    if (url.pathname === "/app/device" && request.method === "POST") {
      const { client_id } = (await request.json().catch(() => ({}))) as { client_id?: string };
      const device_code = random();
      devices.set(device_code, { client: client_id ?? "writer", polls: 0 });
      say(`device: a code for ${client_id}`);
      return Response.json({ device_code, user_code: "BCDF-GHJK", verification_uri: `${issuer}/device`, verification_uri_complete: `${issuer}/device?code=BCDF-GHJK`, expires_in: 600, interval: 1 });
    }
    if (url.pathname === "/app/authorize") {
      const q = url.searchParams;
      const redirect = q.get("redirect_uri") ?? "";
      const target = new URL(redirect);
      if (target.protocol !== "http:" || target.hostname !== "127.0.0.1" || target.pathname !== "/callback") return new Response("bad redirect_uri", { status: 400 });
      if (!["writer", "edit"].includes(q.get("client_id") ?? "")) return new Response("bad client_id", { status: 400 });
      if (q.get("code_challenge_method") !== "S256" || !q.get("code_challenge") || !q.get("state")) return new Response("PKCE (S256) and state are required", { status: 400 });
      if (process.env.STUB_DENY) {
        target.search = new URLSearchParams({ error: "access_denied", error_description: "someone@gmail.com isn't on the list yet. Ask an admin.", state: q.get("state")! }).toString();
      } else {
        const code = random();
        codes.set(code, { challenge: q.get("code_challenge")!, redirect, client: q.get("client_id")!, until: Date.now() + 60_000 });
        target.search = new URLSearchParams({ code, state: q.get("state")! }).toString();
      }
      say(`authorize: ${q.get("client_id")} → ${target.origin}${target.pathname}`);
      return Response.redirect(target.toString(), 302);
    }
    if (url.pathname === "/app/token" && request.method === "POST") {
      const type = request.headers.get("content-type") ?? "";
      const body: Record<string, string> = type.includes("json") ? await request.json() : Object.fromEntries(new URLSearchParams(await request.text()));
      if (body.grant_type === "authorization_code") {
        const code = codes.get(body.code);
        codes.delete(body.code);
        if (!code || code.until < Date.now() || code.redirect !== body.redirect_uri || code.client !== body.client_id) return oauthError("invalid_grant");
        const digest = Buffer.from(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(body.code_verifier ?? ""))).toString("base64url");
        if (digest !== code.challenge) {
          say("token: PKCE verifier didn't match");
          return oauthError("invalid_grant");
        }
        say("token: code exchanged");
        return Response.json(await pair(body.client_id, random()));
      }
      if (body.grant_type === "urn:ietf:params:oauth:grant-type:device_code") {
        const device = devices.get(body.device_code);
        if (!device) return oauthError("invalid_grant");
        device.polls += 1;
        if (device.polls < 3) return Response.json({ error: "authorization_pending", interval: 1 }, { status: 400 });
        devices.delete(body.device_code);
        say("token: device code approved");
        return Response.json(await pair(device.client, random()));
      }
      if (body.grant_type === "refresh_token") {
        const old = refreshes.get(body.refresh_token);
        if (!old || deadChains.has(old.chain)) return oauthError("invalid_grant");
        if (old.dead) {
          // The 60 s grace: the same new pair again.
          if (old.next && Date.now() - (old.diedAt ?? 0) < 60_000) {
            say("token: reused within the grace, same pair");
            return Response.json({ ...(await pair(body.client_id, old.chain)), refresh_token: old.next });
          }
          deadChains.add(old.chain);
          say("token: a dead refresh token was reused; chain revoked");
          return oauthError("invalid_grant");
        }
        const next = await pair(body.client_id, old.chain);
        Object.assign(old, { dead: true, diedAt: Date.now(), next: next.refresh_token });
        say("token: refreshed (rotated)");
        return Response.json(next);
      }
      return oauthError("unsupported_grant_type");
    }
    if (url.pathname === "/app/revoke" && request.method === "POST") {
      const { token } = (await request.json().catch(() => ({}))) as { token?: string };
      const known = token ? refreshes.get(token) : undefined;
      if (known) deadChains.add(known.chain);
      say(`revoke: ${known ? "chain ended" : "unknown token"}`);
      return new Response(null, { status: 200 });
    }
    return new Response("not found", { status: 404 });
  },
});
say(`listening on ${issuer} (perms ${perms.join(",") || "none"}, tokens ${ttl} s)`);
