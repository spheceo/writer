import { afterAll, beforeEach, expect, test } from "bun:test";
import { exportJWK, generateKeyPair, SignJWT } from "jose";
import worker, { type Env } from "./index";
import { forgetKeys } from "./auth";

const pair = await generateKeyPair("RS256");
const jwk = { ...(await exportJWK(pair.publicKey)), kid: "trello-tests", alg: "RS256", use: "sig" };
const accounts = Bun.serve({ port: 0, fetch: () => Response.json({ keys: [jwk] }) });
const issuer = `http://127.0.0.1:${accounts.port}`;
afterAll(() => accounts.stop(true));
beforeEach(() => forgetKeys(0));
const env = {
  ACCOUNTS_URL: issuer,
  TRELLO_BURST: { limit: async () => ({ success: true }) },
} as unknown as Env;
async function request(path: string, audience = "edit", perms = ["use", "edit.bodycam"], extra: { admin?: boolean; expired?: boolean; method?: string; issuer?: string } = {}) {
  const jwt = await new SignJWT({ perms, admin: extra.admin ?? false }).setProtectedHeader({ alg: "RS256", kid: jwk.kid })
    .setIssuer(extra.issuer ?? issuer).setAudience(audience).setSubject("trello-editor").setExpirationTime(Math.floor(Date.now() / 1000) + (extra.expired ? -120 : 60)).sign(pair.privateKey);
  return worker.fetch(new Request(`https://relay${path}`, { method: extra.method ?? "GET", headers: { authorization: `Bearer ${jwt}` } }), env);
}

test("Edit use plus an editing grant reaches only the two Trello readers", async () => {
  // Unconfigured Trello gives 503 after auth, without calling Trello. The
  // handler tests the real audience, route and capability checks together.
  for (const path of ["/v1/trello/card?card=card1234", "/v1/trello/cards?list=comp"]) {
    expect((await request(path)).status).toBe(503);
    expect((await request(path, "edit", [], { admin: true })).status).toBe(503);
    expect((await request(path, "writer", ["use"])).status).toBe(503);
    for (const perms of [[], ["use"], ["edit.bodycam"], ["use", "subtitles"], ["use", "scripts.interrogation"]]) {
      expect((await request(path, "edit", perms)).status).toBe(403);
    }
    expect((await request(path, "feedback", ["use", "edit.bodycam"])).status).toBe(401);
    expect((await request(path, "edit", ["use", "edit.bodycam"], { expired: true })).status).toBe(401);
    expect((await request(path, "edit", ["use", "edit.bodycam"], { issuer: "https://other.example" })).status).toBe(401);
    for (const method of ["POST", "PUT", "DELETE"]) {
      expect((await request(path, "edit", ["use", "edit.bodycam"], { method })).status).toBe(405);
    }
  }
});

test("Edit tokens cannot reach other Writer routes, even as admin", async () => {
  for (const path of ["/v1/drive/token", "/v1/proxy", "/v1/docs/exports", "/v1/cloud/convex", "/v1/watch", "/v1/trello/card/extra"]) {
    expect((await request(path)).status).toBe(401);
    expect((await request(path, "edit", [], { admin: true })).status).toBe(401);
  }
  expect((await worker.fetch(new Request("https://relay/v1/trello/card?card=card1234"), env)).status).toBe(401);
});

test("Edit Trello reads retain Writer's per-person rate limit", async () => {
  const original = env.TRELLO_BURST;
  const seen: string[] = [];
  env.TRELLO_BURST = { limit: async ({ key }) => { seen.push(key); return { success: false }; } };
  try {
    expect((await request("/v1/trello/card?card=card1234")).status).toBe(429);
    expect(seen).toEqual(["person:trello-editor"]);
  } finally { env.TRELLO_BURST = original; }
});

test("an authorized Edit read returns card links and labels without exposing Trello credentials", async () => {
  const savedFetch = globalThis.fetch;
  const upstream: { url: string; method: string }[] = [];
  globalThis.fetch = (async (input: Parameters<typeof fetch>[0], init?: Parameters<typeof fetch>[1]) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    if (url.hostname !== "api.trello.com") return savedFetch(input, init);
    upstream.push({ url: `${url.origin}${url.pathname}`, method: init?.method ?? (input instanceof Request ? input.method : "GET") });
    return Response.json({ id: "card1234", name: "Compilation", desc: "Case 1: https://drive.google.com/drive/folders/1folderfolderfolder", url: "https://trello.com/c/card1234", labels: [{ name: "Compilation" }], attachments: [{ name: "Case 2 (reuse): Script", url: "https://docs.google.com/document/d/1docdocdocdocdoc/edit" }] });
  }) as typeof fetch;
  env.TRELLO_API_KEY = "synthetic-test-key";
  env.TRELLO_TOKEN = "synthetic-test-token";
  try {
    const response = await request("/v1/trello/card?card=card1234");
    expect(response.status).toBe(200);
    const body = await response.text();
    expect(body).toContain("drive.google.com/drive/folders/");
    expect(body).toContain("Case 2 (reuse): Script");
    expect(body).toContain("docs.google.com/document/d/");
    expect(body).not.toContain("synthetic-test");
    expect(upstream).toEqual([{ url: "https://api.trello.com/1/cards/card1234", method: "GET" }]);
    expect((await request("/v1/trello/card?card=card1234", "edit", ["use"])).status).toBe(403);
    expect((await request("/v1/trello/card?card=card1234", "edit", ["use", "edit.bodycam"], { method: "POST" })).status).toBe(405);
    expect(upstream.length).toBe(1);
  } finally {
    globalThis.fetch = savedFetch;
    delete env.TRELLO_API_KEY;
    delete env.TRELLO_TOKEN;
  }
});
