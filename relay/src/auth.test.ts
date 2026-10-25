import { afterAll, beforeEach, expect, test } from "bun:test";
import { exportJWK, generateKeyPair, SignJWT } from "jose";
import { forgetKeys, identify, mayUse, OUT_OF_DATE } from "./auth";

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
const env = { ACCOUNTS_URL: issuer };
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

test("needs only use; v1 permissions and unknown ids are ignored", async () => {
  const v1 = await identify(asking(await token({ perms: ["ai", "cloud", "use", "scripts.interrogation", "something.new"] })), env);
  expect("caller" in v1 && mayUse(v1.caller)).toBe(true);
  const without = await identify(asking(await token({ perms: ["cloud", "scripts.commentary"] })), env);
  expect("caller" in without && mayUse(without.caller)).toBe(false);
  const admin = await identify(asking(await token({ perms: [], admin: true })), env);
  expect("caller" in admin && mayUse(admin.caller)).toBe(true);
});

test("refuses the retired team token and no token, telling old builds to update", async () => {
  const old = await identify(asking("old-team-token"), env);
  expect("refused" in old && old.refused).toBe(OUT_OF_DATE);
  expect("refused" in (await identify(asking(), env))).toBe(true);
});

test("a person's token passes, naming who it is", async () => {
  const who = await identify(asking(await token({ perms: ["use"] })), env);
  if (!("caller" in who)) throw new Error(JSON.stringify(who));
  expect(who.caller.sub).toBe("person123");
  expect(who.caller.email).toBe("someone@gmail.com");
});

test("refuses a token for another app, from another issuer, or expired", async () => {
  expect("refused" in (await identify(asking(await token({}, { aud: "edit" })), env))).toBe(true);
  expect("refused" in (await identify(asking(await token({}, { aud: "feedback" })), env))).toBe(true);
  const feedback = await identify(asking(await token({ perms: [] }, { aud: "feedback" })), env, ["writer", "feedback"]);
  expect("caller" in feedback && feedback.caller.audience === "feedback" && !mayUse(feedback.caller)).toBe(true);
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
