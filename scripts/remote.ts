/**
 * Remote support from here, as Edit's `edit remote`: an admin pulls a
 * teammate's diagnostics or picks up their stuck import, and their Writer
 * does it without them doing anything (src-tauri/src/remote.rs).
 *
 *   just remote devices                       every Mac with Writer signed in, and who's on it
 *   just remote diagnostics <device>          its report: scripts, import and cloud states, the end of its log
 *   just remote resume <device> <script-id>   the cloud tries it again, a stopped import starts again
 *   just remote history <device>              what was asked of it, and how each ended
 *   just remote report <key>                  a report again, by its key (writer/reports/…)
 *
 * <device> is its id, or a part of its name ("macbook", "Sphe"). It goes
 * through this Mac's own Writer (`just dev`, signed in as an admin): the
 * relay checks that, and the teammate's Writer must be open.
 */
const API = "http://127.0.0.1:1421/api/remote";

type Device = { deviceId: string; label: string; build: string; owner: string; updatedAt: number };
type Command = { commandId: string; kind: string; scriptId?: string; state: string; createdAt: number; detail?: string };

async function ask<T>(body: Record<string, unknown>): Promise<T> {
  let response: Response;
  try {
    response = await fetch(API, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  } catch {
    fail("Writer isn't running here: start it with `just dev` and sign in.");
  }
  const value = (await response.json().catch(() => ({}))) as { error?: string };
  if (!response.ok) fail(value.error ?? `Writer answered ${response.status}.`);
  return value as T;
}

function fail(message: string): never {
  console.error(message);
  process.exit(1);
}

const ago = (at: number) => {
  const minutes = Math.round((Date.now() - at) / 60000);
  return minutes < 1 ? "just now" : minutes < 120 ? `${minutes} min ago` : `${Math.round(minutes / 60)} h ago`;
};

async function device(given: string | undefined): Promise<Device> {
  if (!given) fail("Name the device (just remote devices lists them).");
  const devices = await ask<Device[]>({ action: "devices" });
  const exact = devices.find((one) => one.deviceId === given);
  if (exact) return exact;
  const found = devices.filter((one) => one.label.toLowerCase().includes(given.toLowerCase()) || one.deviceId.startsWith(given));
  if (found.length === 1) return found[0];
  fail(found.length ? `"${given}" could be ${found.map((one) => `${one.label} (${one.deviceId})`).join(" or ")}.` : `No device "${given}".`);
}

async function report(key: string) {
  const { path, text } = await ask<{ path: string; text: string }>({ action: "report", key });
  console.log(text);
  console.error(`\nSaved to ${path}`);
}

const [action, first, second] = process.argv.slice(2);
switch (action) {
  case "devices": {
    const devices = (await ask<Device[]>({ action: "devices" })).sort((a, b) => b.updatedAt - a.updatedAt);
    for (const one of devices) console.log(`${one.deviceId}  ${one.label}  ·  ${one.build}  ·  seen ${ago(one.updatedAt)}`);
    if (!devices.length) console.log("No Writer has registered yet.");
    break;
  }
  case "history": {
    const target = await device(first);
    for (const item of await ask<Command[]>({ action: "history", device: target.deviceId })) {
      console.log(`${new Date(item.createdAt).toLocaleString()}  ${item.kind}${item.scriptId ? ` ${item.scriptId}` : ""}  ${item.state}${item.detail ? `: ${item.detail}` : ""}`);
    }
    break;
  }
  case "diagnostics":
  case "resume": {
    const target = await device(first);
    if (action === "resume" && !second) fail("Name the script to resume: just remote resume <device> <script-id> (its id is in its diagnostics).");
    const { commandId } = await ask<{ commandId: string }>({ action, device: target.deviceId, script: second });
    console.error(`Asked ${target.label} (${commandId}); waiting for it (its Writer must be open)…`);
    // Writer asks every 20 s; a report takes a few seconds more.
    for (let waited = 0; waited < 600; waited += 5) {
      await Bun.sleep(5000);
      const item = (await ask<Command[]>({ action: "history", device: target.deviceId })).find((one) => one.commandId === commandId);
      if (!item || item.state === "pending" || item.state === "claimed") continue;
      if (item.state === "failed") fail(`It failed: ${item.detail ?? "no reason given"}`);
      const sent = item.detail?.match(/^Sent: (\S+)/)?.[1];
      if (sent) await report(sent);
      else console.log(item.detail ?? "Done.");
      process.exit(0);
    }
    fail("No answer in ten minutes: their Writer isn't open (the request has expired).");
  }
  case "report":
    if (!first) fail("Give the report's key (writer/reports/…).");
    await report(first);
    break;
  default:
    fail("Use: just remote devices | diagnostics <device> | resume <device> <script-id> | history <device> | report <key>");
}
