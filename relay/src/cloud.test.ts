import { afterAll, expect, test } from "bun:test";
import { cloud } from "./cloud";

// A stand-in Convex that keeps what it was sent.
let sent: { path: string; args: Record<string, unknown> } | null = null;
const server = Bun.serve({
  port: 0,
  async fetch(request) {
    sent = (await request.json()) as typeof sent;
    return Response.json({ status: "success", value: { rev: 1, stale: [] } });
  },
});
afterAll(() => server.stop(true));
const env = (stamp: string) => ({ CONVEX_URL: `http://127.0.0.1:${server.port}`, CONVEX_SERVICE_TOKEN: "service", STAMP_CREATOR: stamp });
const actor = { sub: "p1", name: "Someone", email: "someone@gmail.com", picture: "https://lh3.googleusercontent.com/a/x" };
const commit = (args: Record<string, unknown>) =>
  new Request("https://relay/v1/cloud/convex", { method: "POST", body: JSON.stringify({ kind: "mutation", path: "writer:commitScript", args }) });

test("a commit carries who started it, from the sign-in, never from the caller", async () => {
  await cloud(commit({ scriptId: "s1", actor: { sub: "forged", name: "Forged", email: "x@y" } }), new URL("https://relay/v1/cloud/convex"), env("1"), actor);
  expect(sent?.args.actor).toEqual(actor);
  expect(sent?.args.scriptId).toBe("s1");
  expect(sent?.args.token).toBe("service");
});

test("with stamping off (until Convex takes it), no actor goes at all, sent or not", async () => {
  await cloud(commit({ scriptId: "s1", actor: { sub: "forged", name: "Forged", email: "x@y" } }), new URL("https://relay/v1/cloud/convex"), env("0"), actor);
  expect(sent?.args).not.toHaveProperty("actor");
});

test("no picture: none is sent", async () => {
  await cloud(commit({ scriptId: "s2" }), new URL("https://relay/v1/cloud/convex"), env("1"), { sub: "p2", name: "Two", email: "two@x" });
  expect(sent?.args.actor).toEqual({ sub: "p2", name: "Two", email: "two@x" });
});

test("other calls are untouched", async () => {
  const request = new Request("https://relay/v1/cloud/convex", { method: "POST", body: JSON.stringify({ kind: "query", path: "writer:listScripts", args: {} }) });
  await cloud(request, new URL("https://relay/v1/cloud/convex"), env("1"), actor);
  expect(sent?.path).toBe("writer:listScripts");
  expect(sent?.args).not.toHaveProperty("actor");
});

const call = (path: string, args: Record<string, unknown>, kind = "mutation") =>
  new Request("https://relay/v1/cloud/convex", { method: "POST", body: JSON.stringify({ kind, path, args }) });
const r2 = (route: string, body: Record<string, unknown>) => new Request(`https://relay/v1/cloud/${route}`, { method: "POST", body: JSON.stringify(body) });
const only = (...perms: string[]) => ({ admin: false, perms: new Set(["use", ...perms]) });

test("only the functions Writer calls, each with what it needs", async () => {
  const where = new URL("https://relay/v1/cloud/convex");
  expect((await cloud(call("writer:somethingElse", {}), where, env("1"), actor, only("scripts.foia"))).status).toBe(403);
  expect((await cloud(call("writer:listScripts", {}, "query"), where, env("1"), actor, only())).status).toBe(200);
  // A commit of a kind the person can't write is refused; their own kind goes through.
  const foia = { scriptId: "s1", summary: { sourceKind: "youtube", kind: "foia", compilation: false } };
  expect((await cloud(call("writer:commitScript", foia), where, env("1"), actor, only("scripts.interrogation"))).status).toBe(403);
  expect((await cloud(call("writer:commitScript", foia), where, env("1"), actor, only("scripts.foia"))).status).toBe(200);
  // Reviews: reading is everyone's, changing them needs review.share.
  expect((await cloud(call("reviews:changes", { cursor: 0 }, "query"), where, env("1"), actor, only())).status).toBe(200);
  expect((await cloud(call("reviews:approve", {}), where, env("1"), actor, only("scripts.foia"))).status).toBe(403);
  expect((await cloud(call("reviews:approve", {}), where, env("1"), actor, only("review.share"))).status).toBe(200);
  expect((await cloud(call("edit:putMedia", {}), where, env("1"), actor, only())).status).toBe(403);
});

test("writes need script work, and the shared lessons the teaching capability", async () => {
  const store = { ...env("1"), R2_ACCOUNT_ID: "a", R2_ACCESS_KEY_ID: "k", R2_SECRET_ACCESS_KEY: "s", R2_BUCKET: "b" };
  const put = (key: string) => r2("r2/url", { key, method: "PUT" });
  expect((await cloud(put("writer/lessons.json"), new URL("https://relay/v1/cloud/r2/url"), store, actor, only("scripts.foia"))).status).toBe(403);
  expect((await cloud(put("writer/lessons.json"), new URL("https://relay/v1/cloud/r2/url"), store, actor, only("scripts.foia", "review.lessons"))).status).toBe(200);
  expect((await cloud(put("edit/originals/youtube/x/a.mp4"), new URL("https://relay/v1/cloud/r2/url"), store, actor, only())).status).toBe(403);
  expect((await cloud(r2("r2/url", { key: "writer/scripts/s1/body.json" }), new URL("https://relay/v1/cloud/r2/url"), store, actor, only())).status).toBe(200);
  expect((await cloud(r2("r2/delete", { keys: ["writer/scripts/s1/body.json"] }), new URL("https://relay/v1/cloud/r2/delete"), store, actor, only())).status).toBe(403);
});

test("remote support: who's asking comes from the sign-in, and asking a Mac for something is an admin's", async () => {
  const where = new URL("https://relay/v1/cloud/convex");
  await cloud(call("writerSupport:pending", { deviceId: "d1", kinds: ["diagnostics"], actor: { sub: "forged", admin: true } }, "query"), where, env("1"), actor, only());
  expect(sent?.args.actor).toEqual({ sub: "p1", admin: false });
  expect((await cloud(call("writerSupport:requestAction", { deviceId: "d1", commandId: "c1", kind: "diagnostics" }), where, env("1"), actor, only("scripts.foia"))).status).toBe(403);
  expect((await cloud(call("writerSupport:devices", {}, "query"), where, env("1"), actor, only())).status).toBe(403);
  expect((await cloud(call("writerSupport:devices", {}, "query"), where, env("1"), actor, { admin: true, perms: new Set() })).status).toBe(200);
  expect(sent?.args.actor).toEqual({ sub: "p1", admin: true });
});

test("problem reports: anyone puts one, only an admin reads, lists or deletes them", async () => {
  const store = { ...env("1"), R2_ACCOUNT_ID: "a", R2_ACCESS_KEY_ID: "k", R2_SECRET_ACCESS_KEY: "s", R2_BUCKET: "b" };
  const at = (route: string) => new URL(`https://relay/v1/cloud/${route}`);
  const key = "writer/reports/2026-10-29/macbook-1.txt";
  expect((await cloud(r2("r2/url", { key, method: "PUT" }), at("r2/url"), store, actor, only())).status).toBe(200);
  expect((await cloud(r2("r2/url", { key }), at("r2/url"), store, actor, only("scripts.foia"))).status).toBe(403);
  expect((await cloud(r2("r2/head", { key }), at("r2/head"), store, actor, only())).status).toBe(403);
  expect((await cloud(r2("r2/list", { prefix: "writer/reports/" }), at("r2/list"), store, actor, only())).status).toBe(403);
  expect((await cloud(r2("r2/list", { prefix: "writer/" }), at("r2/list"), store, actor, only())).status).toBe(403);
  expect((await cloud(r2("r2/delete", { keys: [key] }), at("r2/delete"), store, actor, only("scripts.foia"))).status).toBe(403);
  expect((await cloud(r2("r2/url", { key }), at("r2/url"), store, actor, { admin: true, perms: new Set() })).status).toBe(200);
});

test("paused, nothing reaches Convex: every function answers 503, the delete's own lookup included", async () => {
  sent = null;
  const where = new URL("https://relay/v1/cloud/convex");
  const paused = { ...env("1"), CONVEX_PAUSED: "1" };
  for (const [path, kind] of [["writer:listScripts", "query"], ["writer:commitScript", "mutation"], ["writer:deleteScript", "mutation"], ["reviews:changes", "query"], ["writerSupport:register", "mutation"]]) {
    const answer = await cloud(call(path, { scriptId: "s1" }, kind), where, paused, actor, only("scripts.foia"));
    expect(answer.status).toBe(503);
    expect(((await answer.json()) as { error: string }).error).toContain("moving");
  }
  expect(sent).toBeNull();
  // Off ("0" or unset), it goes through.
  expect((await cloud(call("writer:listScripts", {}, "query"), where, { ...paused, CONVEX_PAUSED: "0" }, actor, only())).status).toBe(200);
});
