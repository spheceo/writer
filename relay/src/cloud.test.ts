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
