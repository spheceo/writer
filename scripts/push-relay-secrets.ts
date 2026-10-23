/**
 * Puts writer-relay's secrets on the Worker from Infisical: Repack's project,
 * `dev`, folder `/writer-relay` (references to the canonical values at the
 * root). Each value goes from Infisical to `wrangler secret put` through
 * pipes, in memory: nothing is printed, written to disk or passed as an
 * argument.
 *
 *   bun scripts/push-relay-secrets.ts             # every secret
 *   bun scripts/push-relay-secrets.ts --dry-run   # only says which it would put
 *   bun scripts/push-relay-secrets.ts PROXY_URL   # just these
 *
 * Signed in to Infisical with `infisical login` (its keyring), and to
 * Cloudflare with `bunx wrangler login`.
 */
import { resolve } from "node:path";

const PROJECT = "cdf4a0c5-eca4-4415-af3a-40871af285f3";
const FOLDER = "/writer-relay";
/** What the relay reads (relay/src/index.ts, cloud.ts, auth.ts); nothing else is put. */
const EXPECTED = [
  "ELEVENLABS_API_KEY",
  "AI_GATEWAY_API_KEY",
  "GOOGLE_DRIVE_CLIENT_ID",
  "GOOGLE_DRIVE_CLIENT_SECRET",
  "GOOGLE_DRIVE_REFRESH_TOKEN",
  "GOOGLE_DOCS_CLIENT_ID",
  "GOOGLE_DOCS_CLIENT_SECRET",
  "GOOGLE_DOCS_REFRESH_TOKEN",
  "PROXY_URL",
  "TRELLO_API_KEY",
  "TRELLO_TOKEN",
  "R2_ACCOUNT_ID",
  "R2_ACCESS_KEY_ID",
  "R2_SECRET_ACCESS_KEY",
  "R2_BUCKET",
  "CONVEX_URL",
  "CONVEX_SERVICE_TOKEN",
];

const args = process.argv.slice(2);
const dry = args.includes("--dry-run");
const only = args.filter((arg) => !arg.startsWith("--"));
for (const name of only) if (!EXPECTED.includes(name)) throw new Error(`${name} isn't one of the relay's secrets.`);

const exported = Bun.spawn(["infisical", "export", `--projectId=${PROJECT}`, "--env=dev", `--path=${FOLDER}`, "--format=json", "--silent"], { stdout: "pipe", stderr: "pipe" });
const [text, code] = [await new Response(exported.stdout).text(), await exported.exited];
if (code !== 0) throw new Error(`Infisical refused (exit ${code}). Sign in with \`infisical login\`.`);
const values = new Map((JSON.parse(text) as { key: string; value: string }[]).map((item) => [item.key, item.value]));

const unexpected = [...values.keys()].filter((key) => !EXPECTED.includes(key));
if (unexpected.length) throw new Error(`${FOLDER} has keys the relay doesn't read: ${unexpected.join(", ")}. Remove them or add them here.`);
const wanted = only.length ? only : EXPECTED;
const missing = wanted.filter((key) => !values.get(key));
if (missing.length) throw new Error(`${FOLDER} is missing (or has empty, or an unresolved reference for): ${missing.join(", ")}.`);
const broken = wanted.filter((key) => values.get(key)!.includes("${"));
if (broken.length) throw new Error(`These still hold an unresolved reference: ${broken.join(", ")}.`);

const relay = resolve(import.meta.dir, "../relay");
for (const key of wanted) {
  if (dry) {
    console.log(`would put ${key}`);
    continue;
  }
  const put = Bun.spawn(["bunx", "wrangler", "secret", "put", key], { cwd: relay, stdin: "pipe", stdout: "pipe", stderr: "pipe" });
  put.stdin.write(values.get(key)!);
  put.stdin.end();
  const done = await put.exited;
  // Wrangler's own output never holds the value, but it isn't shown either: only the outcome.
  if (done !== 0) {
    const why = (await new Response(put.stderr).text()).split("\n").find((line) => line.includes("ERROR") || line.includes("✘")) ?? `exit ${done}`;
    throw new Error(`Couldn't put ${key}: ${why.slice(0, 200)}`);
  }
  console.log(`put ${key}`);
}
console.log(dry ? `${wanted.length} secrets would be put on writer-relay.` : `${wanted.length} secrets are on writer-relay.`);
