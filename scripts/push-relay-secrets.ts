/**
 * Puts writer-relay's secrets on the Worker from Infisical: Repack's project,
 * `dev`, folder `/writer-relay` (references to the canonical values: the root's,
 * and ElevenLabs to /edit-relay's ELEVENLABS_MAIN_KEY, the key Edit's relay
 * uses too). Each value goes from Infisical to `wrangler secret put` on its
 * stdin, in memory: nothing is printed, written to disk or passed as an
 * argument. It prints names only.
 *
 *   bun scripts/push-relay-secrets.ts                    what it would put (names), nothing sent
 *   bun scripts/push-relay-secrets.ts --yes              put them all
 *   bun scripts/push-relay-secrets.ts --yes --only A,B   put only these (the rest of the Worker's untouched)
 *   bun scripts/push-relay-secrets.ts --yes --only CONVEX_URL --from=prod:/mamagi
 *                                                        these from another folder (the new Convex deployment's)
 *
 * A Worker secret that isn't in the folder is left as it is (never deleted).
 * Signed in to Infisical with `infisical login` (its keyring), and to
 * Cloudflare with `npx wrangler login`.
 */
import { resolve } from "node:path";

const PROJECT = "cdf4a0c5-eca4-4415-af3a-40871af285f3";
const FOLDER = "/writer-relay";
/** What the relay reads (relay/src/index.ts, cloud.ts, auth.ts); nothing else is put. */
const EXPECTED = [
  "AI_GATEWAY_API_KEY",
  "ELEVENLABS_MAIN_KEY",
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
const yes = args.includes("--yes");
// `--from=prod:/mamagi`: another environment and folder, for --only's names (it holds others the relay doesn't read).
const from = args.find((arg) => arg.startsWith("--from="))?.slice("--from=".length);
const [env, folder] = from ? [from.slice(0, from.indexOf(":")), from.slice(from.indexOf(":") + 1)] : ["dev", FOLDER];
if (from && (!env || !folder.startsWith("/"))) throw new Error("--from is ENV:/folder, e.g. --from=prod:/mamagi.");
// `--only A,B` (or the names given bare): just these.
const only = args.filter((arg) => !arg.startsWith("--")).flatMap((arg) => arg.split(",")).filter(Boolean);
for (const name of only) if (!EXPECTED.includes(name)) throw new Error(`${name} isn't one of the relay's secrets.`);
if (from && !only.length) throw new Error("--from puts only what --only names.");

const exported = Bun.spawn(["infisical", "export", `--projectId=${PROJECT}`, `--env=${env}`, `--path=${folder}`, "--format=json", "--silent"], { stdout: "pipe", stderr: "pipe" });
const [text, code] = [await new Response(exported.stdout).text(), await exported.exited];
if (code !== 0) throw new Error(`Infisical refused (exit ${code}). Sign in with \`infisical login\`.`);
const values = new Map((JSON.parse(text) as { key: string; value: string }[]).map((item) => [item.key, item.value]));

const unexpected = [...values.keys()].filter((key) => !EXPECTED.includes(key));
if (!from && unexpected.length) throw new Error(`${FOLDER} has keys the relay doesn't read: ${unexpected.join(", ")}. Remove them or add them here.`);
const wanted = only.length ? only : EXPECTED;
const missing = wanted.filter((key) => !values.get(key));
if (missing.length) throw new Error(`${env}:${folder} is missing (or has empty, or an unresolved reference for): ${missing.join(", ")}.`);
const broken = wanted.filter((key) => values.get(key)!.includes("${"));
if (broken.length) throw new Error(`These still hold an unresolved reference: ${broken.join(", ")}.`);

if (!yes) {
  console.log(`would put on writer-relay, from ${env}:${folder}: ${[...wanted].sort().join(", ")}`);
  console.log("nothing sent: add --yes to put them");
  process.exit(0);
}
const relay = resolve(import.meta.dir, "../relay");
let failed = 0;
for (const key of [...wanted].sort()) {
  const put = Bun.spawn(["npx", "wrangler", "secret", "put", key], { cwd: relay, stdin: "pipe", stdout: "pipe", stderr: "pipe" });
  put.stdin.write(values.get(key)!);
  put.stdin.end();
  const done = await put.exited;
  // Wrangler's own output never holds the value, but it isn't shown either: only the outcome.
  if (done !== 0) {
    failed += 1;
    const why = (await new Response(put.stderr).text()).split("\n").find((line) => line.includes("ERROR") || line.includes("✘")) ?? `exit ${done}`;
    console.error(`couldn't put ${key}: ${why.slice(0, 200)}`);
    continue;
  }
  console.log(`put ${key}`);
}
console.log(`${wanted.length - failed} of ${wanted.length} secrets are on writer-relay.`);
process.exit(failed === 0 ? 0 : 1);
