import { afterAll, beforeEach, expect, test } from "bun:test";
import { exportJWK, generateKeyPair, SignJWT } from "jose";
import { allowed, forgetKeys, identify, permissionFor } from "./auth";

// A stand-in accounts site: its JWKS, with keys that can be swapped.
const keyA = await generateKeyPair("RS256");
const keyB = await generateKeyPair("RS256");
let served = [{ ...(await exportJWK(keyA.publicKey)), kid: "a", alg: "RS256", use: "sig" }];
let fetches = 0;
const server = Bun.serve({
  port: 0,
  fetch(request) {
    if (new URL(request.url).pathname !== "/.well-known/jwks.json") return new Response("not found", { status: 404 });
    fetches++;
    return Response.json({ keys: served });
  },
});
const issuer = `http://127.0.0.1:${server.port}`;
const env = { ACCOUNTS_URL: issuer, CLIENT_TOKEN: "old-team-token" };
afterAll(() => server.stop(true));
beforeEach(() => forgetKeys(0));

const now = () => Math.floor(Date.now() / 1000);
async function token(claims: Record<string, unknown> = {}, { key = keyA.privateKey, kid = "a", aud = "writer", iss = issuer, exp = now() + 3600 } = {}) {
  return new SignJWT({ email: "someone@gmail.com", name: "Someone", perms: ["use"], admin: false, ...claims })
    .setProtectedHeader({ alg: "RS256", kid })
    .setIssuer(iss)
    .setAudience(aud)
    .setSubject("person123")
    .setIssuedAt()
    .setExpirationTime(exp)
    .sign(key);
}
const asking = (bearer?: string) => new Request("https://relay/v1/transcribe", { headers: bearer ? { authorization: `Bearer ${bearer}` } : {} });

test("maps every route to its permission", () => {
  expect(permissionFor("/v1/transcribe")).toBe("use");
  expect(permissionFor("/v1/watch")).toBe("ai");
  expect(permissionFor("/v1/drive/token")).toBe("drive");
  expect(permissionFor("/v1/docs/export")).toBe("drive");
  expect(permissionFor("/v1/docs/comments")).toBe("drive");
  expect(permissionFor("/v1/trello/cards")).toBe("trello");
  expect(permissionFor("/v1/proxy")).toBe("proxy");
  expect(permissionFor("/v1/cloud/r2/url")).toBe("cloud");
  expect(permissionFor("/v1/cloud/convex")).toBe("cloud");
});

test("the old team token still passes, with everything", async () => {
  const who = await identify(asking("old-team-token"), env);
  expect("caller" in who && who.caller.kind).toBe("legacy");
  expect("caller" in who && allowed(who.caller, "cloud")).toBe(true);
  expect("refused" in (await identify(asking("old-team-tokeN"), env))).toBe(true);
  expect("refused" in (await identify(asking(), env))).toBe(true);
  // Without CLIENT_TOKEN on the relay, nothing but a person's token passes.
  expect("refused" in (await identify(asking("old-team-token"), { ACCOUNTS_URL: issuer }))).toBe(true);
});

test("a person's token passes, with only their permissions", async () => {
  const who = await identify(asking(await token({ perms: ["use", "ai"] })), env);
  if (!("caller" in who) || who.caller.kind !== "person") throw new Error(JSON.stringify(who));
  expect(who.caller.sub).toBe("person123");
  expect(allowed(who.caller, "use")).toBe(true);
  expect(allowed(who.caller, "ai")).toBe(true);
  expect(allowed(who.caller, "cloud")).toBe(false);
  const admin = await identify(asking(await token({ perms: [], admin: true })), env);
  expect("caller" in admin && allowed(admin.caller, "proxy")).toBe(true);
});

test("refuses a token for another app, from another issuer, or expired", async () => {
  expect("refused" in (await identify(asking(await token({}, { aud: "edit" })), env))).toBe(true);
  expect("refused" in (await identify(asking(await token({}, { iss: "https://evil.example" })), env))).toBe(true);
  const expired = await identify(asking(await token({}, { exp: now() - 120 })), env);
  expect("refused" in expired && expired.refused).toContain("expired");
  // 60 s of skew is allowed.
  expect("caller" in (await identify(asking(await token({}, { exp: now() - 30 })), env))).toBe(true);
});

test("refuses a token signed with a key the site doesn't publish, or not RS256", async () => {
  expect("refused" in (await identify(asking(await token({}, { key: keyB.privateKey, kid: "a" })), env))).toBe(true);
  const secret = new TextEncoder().encode("x".repeat(32));
  const hs = await new SignJWT({ perms: ["use"] }).setProtectedHeader({ alg: "HS256", kid: "a" }).setIssuer(issuer).setAudience("writer").setSubject("p").setExpirationTime(now() + 60).sign(secret);
  expect("refused" in (await identify(asking(hs), env))).toBe(true);
});

test("keeps the keys cached, and refetches once on an unknown kid", async () => {
  fetches = 0;
  await identify(asking(await token()), env);
  await identify(asking(await token()), env);
  expect(fetches).toBe(1);
  served = [...served, { ...(await exportJWK(keyB.publicKey)), kid: "b", alg: "RS256", use: "sig" }];
  expect("caller" in (await identify(asking(await token({}, { key: keyB.privateKey, kid: "b" })), env))).toBe(true);
  expect(fetches).toBe(2);
});
