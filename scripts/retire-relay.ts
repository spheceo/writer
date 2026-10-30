/**
 * Retires writer-relay (Writer is discontinued): the Worker, with what it owns
 * (its secrets, its Durable Object `TranscriptionBudget` and the day counts in
 * it, its workers.dev address, its logs), then the Infisical folder
 * `/writer-relay` in Repack's project. Every value in that folder is a copy or
 * reference of one kept at the root or in `/edit-relay`, and those sources are
 * never touched: only the folder goes.
 *
 *   bun scripts/retire-relay.ts          checks and the plan; nothing changed
 *   bun scripts/retire-relay.ts --yes    does it, if every check passes
 *
 * Checks, all read-only:
 *   - no caller left: `origin/main` of Edit (Trello routes), feedback (Docs
 *     routes), accounts (Writer's manifest) and Repack name the relay's address
 *     nowhere outside Markdown. Each is fetched first. Whether what's deployed
 *     matches main is for whoever runs this to know.
 *   - every value in /writer-relay is also at the root or in /edit-relay, so
 *     deleting the folder loses no key. Folders sharing a value are listed: in
 *     Infisical's UI they should reference the root, never /writer-relay.
 *   - `wrangler delete --dry-run` passes.
 *
 * It prints names only, never a value. Signed in to Infisical with
 * `infisical login`, and to Cloudflare with `npx wrangler login`.
 */
import { resolve } from "node:path";

const PROJECT = "cdf4a0c5-eca4-4415-af3a-40871af285f3";
const FOLDER = "writer-relay";
const WORKER = "writer-relay";
const ADDRESS = "writer-relay.sphe-g-personal.workers.dev";
/** Repos that called the relay, next to this one: who called what. */
const CALLERS: Record<string, string> = {
  edit: "/v1/trello/card and /v1/trello/cards (Script mode, aud edit)",
  feedback: "/v1/docs/export and /v1/docs/exports (review page, aud feedback)",
  accounts: "/.well-known/spheceo-app.json (APP_MANIFESTS)",
  repack: "nothing known",
};

const yes = process.argv.includes("--yes");
const code = resolve(import.meta.dir, "../..");
const relay = resolve(import.meta.dir, "../relay");

async function run(command: string[], cwd?: string): Promise<{ code: number; out: string }> {
  const child = Bun.spawn(command, { cwd, stdout: "pipe", stderr: "pipe" });
  const out = await new Response(child.stdout).text();
  return { code: await child.exited, out };
}

async function folder(path: string): Promise<{ key: string; value: string }[]> {
  const exported = await run(["infisical", "export", `--projectId=${PROJECT}`, "--env=dev", `--path=${path}`, "--format=json", "--silent"]);
  if (exported.code !== 0) throw new Error(`Infisical refused ${path} (exit ${exported.code}). Sign in with \`infisical login\`.`);
  return JSON.parse(exported.out);
}

const problems: string[] = [];

// 1. Nobody calls it any more.
for (const [repo, calls] of Object.entries(CALLERS)) {
  const dir = resolve(code, repo);
  if ((await run(["git", "-C", dir, "fetch", "--quiet", "origin", "main"])).code !== 0) {
    problems.push(`couldn't fetch ${repo}'s main (${dir}): its calls (${calls}) aren't checked`);
    continue;
  }
  const found = await run(["git", "-C", dir, "grep", "-l", "-F", ADDRESS, "origin/main", "--", ".", ":!*.md"]);
  const files = found.out.split("\n").filter(Boolean).map((line) => line.replace(/^origin\/main:/, ""));
  console.log(`${repo.padEnd(9)} ${files.length ? `still calls it: ${files.join(", ")}` : "no call left"}  [${calls}]`);
  if (files.length) problems.push(`${repo} still names ${ADDRESS}`);
}

// 2. Deleting the folder loses no key.
const own = await folder(`/${FOLDER}`);
const sources = { "/": await folder("/"), "/edit-relay": await folder("/edit-relay") };
for (const { key, value } of own) {
  const kept = Object.entries(sources).flatMap(([path, items]) => items.filter((item) => item.value === value).map((item) => `${path === "/" ? "" : `${path}.`}${item.key}`));
  console.log(`/${FOLDER}.${key.padEnd(27)} ${kept.length ? `kept at ${kept.join(", ")}` : "ONLY HERE"}`);
  if (!kept.length) problems.push(`/${FOLDER}.${key} is the only copy of its value`);
}
const values = new Set(own.map((item) => item.value));
const listed = await run(["infisical", "secrets", "folders", "get", `--projectId=${PROJECT}`, "--env=dev", "--path=/", "--output=json", "--silent"]);
const others = listed.code === 0 ? (JSON.parse(listed.out) as { folderName: string }[]).map((item) => item.folderName).filter((name) => name !== FOLDER && name !== "edit-relay") : [];
const sharing: string[] = [];
for (const name of others) if ((await folder(`/${name}`)).some((item) => values.has(item.value))) sharing.push(`/${name}`);
if (sharing.length) console.log(`sharing a value with /${FOLDER} (check they reference the root, not it): ${sharing.join(", ")}`);

// 3. Cloudflare would delete it.
const dry = await run(["npx", "wrangler", "delete", WORKER, "--dry-run"], relay);
console.log(`wrangler delete ${WORKER} --dry-run: ${dry.code === 0 ? "ok" : `exit ${dry.code}`}`);
if (dry.code !== 0) problems.push("wrangler delete --dry-run failed: `npx wrangler login`?");

if (problems.length) {
  console.log(`\nnot ready:\n${problems.map((problem) => `  - ${problem}`).join("\n")}`);
  process.exit(1);
}
if (!yes) {
  console.log(`\nready. --yes deletes the Worker ${WORKER} (its secrets, Durable Object and logs with it), then Infisical's /${FOLDER} (${own.length} entries).`);
  process.exit(0);
}

// The Worker first, so nothing reads the keys while they go. Its own prompt and output, which hold no secret.
const deleted = Bun.spawn(["npx", "wrangler", "delete", WORKER], { cwd: relay, stdio: ["inherit", "inherit", "inherit"] });
if ((await deleted.exited) !== 0) throw new Error(`wrangler delete ${WORKER} failed: the Infisical folder is left as it was.`);
// The folder and its entries; Infisical's own output isn't shown, only the outcome.
const gone = await run(["infisical", "secrets", "folders", "delete", `--name=${FOLDER}`, "--path=/", `--projectId=${PROJECT}`, "--env=dev", "--silent"]);
if (gone.code !== 0) throw new Error(`The Worker is gone, but Infisical didn't delete /${FOLDER} (exit ${gone.code}): delete it in Infisical's UI.`);
console.log(`deleted the Worker ${WORKER} and Infisical's /${FOLDER}. Check the dashboard's Durable Objects for writer-relay_TranscriptionBudget.`);
