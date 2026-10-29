//! Remote support, as Edit has it (`apps/server/src/remote_support.ts`): an
//! admin can pull a teammate's diagnostics and pick up their stuck import
//! from their own Mac, without the teammate doing anything.
//!
//! Every signed-in Writer registers its Mac (`writerSupport:register` in
//! Repack's Convex, through the relay, which names who's asking) and asks
//! every few seconds for commands of the kinds it carries out. Each command is
//! claimed in the cloud, then on disk (a receipt in `remote-commands/`, made
//! only if it isn't there: never carried out twice, even across a restart),
//! carried out and reported back. Two kinds, both bounded (no agent turn, so
//! nothing spends beyond what the import itself would):
//!
//! - diagnostics: this Mac's scripts with their import and cloud states, the
//!   work under way and the end of the log, sent as a problem report to R2
//!   (`writer/reports/<day>/`, which only an admin can read);
//! - resume: a script the cloud refused, or never got, is tried again now,
//!   and an import that stopped starts again from what it kept.
//!
//! The admin's side (`just remote …`, scripts/remote.ts) goes through the
//! local API of the admin's own Writer, signed in as them (`POST /api/remote`).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{activity, cloud, import, jobs, log, machine, release, store};

/// The kinds this build carries out: Convex sends it no others.
pub const KINDS: [&str; 2] = ["diagnostics", "resume"];
/// How often to ask for commands, and to say again that this Mac is here.
const POLL: Duration = Duration::from_secs(20);
const REGISTER_EVERY: Duration = Duration::from_secs(600);
/// A report is text; a runaway log is cut.
const MAX_REPORT: usize = 2_000_000;
const LOG_LINES: usize = 3000;
const REPORTS: &str = "writer/reports";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Command {
    command_id: String,
    device_id: String,
    kind: String,
    #[serde(default)]
    script_id: Option<String>,
    expires_at: f64,
}

/// This Mac, as the admin's list shows it: its name and who's signed in on it.
fn label() -> String {
    let person = crate::account::status().person.map(|person| person.name).filter(|name| !name.trim().is_empty());
    let label = match person {
        Some(name) => format!("{} · {name}", machine::name()),
        None => machine::name(),
    };
    label.chars().take(120).collect()
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as f64).unwrap_or(0.0)
}

pub fn start(app: AppHandle, data: PathBuf) {
    std::thread::spawn(move || {
        // Who it was last registered for, and when.
        let mut registered: Option<(String, Instant)> = None;
        loop {
            if let Err(error) = round(&app, &data, &mut registered) {
                // Offline or signed out is ordinary; anything else is worth a line, once a round.
                if crate::account::can_call() && !error.starts_with("Can't reach") {
                    log::warn("remote", error);
                }
            }
            std::thread::sleep(POLL);
        }
    });
}

fn round(app: &AppHandle, data: &Path, registered: &mut Option<(String, Instant)>) -> Result<(), String> {
    if !crate::account::can_call() {
        return Ok(());
    }
    let Some(sub) = crate::account::status().person.map(|person| person.sub).filter(|sub| !sub.is_empty()) else {
        return Ok(());
    };
    let device = machine::id();
    let fresh = registered.as_ref().is_none_or(|(who, at)| *who != sub || at.elapsed() >= REGISTER_EVERY);
    if fresh {
        let first = registered.as_ref().is_none_or(|(who, _)| *who != sub);
        cloud::call(data, "mutation", "writerSupport:register", json!({ "deviceId": device, "label": label(), "build": release::name() }))?;
        *registered = Some((sub, Instant::now()));
        if first {
            settle_claimed(data, device)?;
        }
    }
    let pending = cloud::call(data, "query", "writerSupport:pending", json!({ "deviceId": device, "kinds": KINDS }))?;
    let commands: Vec<Command> = serde_json::from_value(pending).unwrap_or_default();
    for command in commands {
        if command.device_id == device && command.expires_at > now_ms() {
            carry_out(app, data, &command)?;
        }
    }
    Ok(())
}

/// Commands claimed before Writer last closed: reported as they ended, or failed (never run again).
fn settle_claimed(data: &Path, device: &str) -> Result<(), String> {
    let claimed = cloud::call(data, "query", "writerSupport:claimed", json!({ "deviceId": device }))?;
    let commands: Vec<Command> = serde_json::from_value(claimed).unwrap_or_default();
    for command in commands {
        let receipt = receipt(data, &command.command_id);
        let (state, detail) = match receipt.as_ref().and_then(|value| value["state"].as_str().filter(|state| *state != "claimed").map(|state| (state.to_string(), value["detail"].as_str().unwrap_or("").to_string()))) {
            Some(ended) => ended,
            None => ("failed".to_string(), "Writer closed before it reported this. Nothing was run again.".to_string()),
        };
        finish(data, &command, &state, &detail)?;
    }
    Ok(())
}

fn carry_out(app: &AppHandle, data: &Path, command: &Command) -> Result<(), String> {
    let claimed = cloud::call(data, "mutation", "writerSupport:claim", json!({ "deviceId": command.device_id, "commandId": command.command_id }))?;
    if claimed != Value::Bool(true) {
        return Ok(());
    }
    // Received before (its receipt is here): reported as failed, its receipt left as it was.
    if !claim_receipt(data, &command.command_id, &command.kind)? {
        return finish(data, command, "failed", "This command was already received. Nothing was run again.");
    }
    let outcome = {
        log::info("remote", format!("an admin asked for {}{}", command.kind, command.script_id.as_deref().map(|id| format!(" of {id}")).unwrap_or_default()));
        match command.kind.as_str() {
            "diagnostics" => diagnostics(data).map(|key| format!("Sent: {key}")),
            "resume" => match command.script_id.as_deref() {
                Some(id) => resume(app, data, id),
                None => Err("No script was named.".to_string()),
            },
            other => Err(format!("This build doesn't know \"{other}\".")),
        }
    };
    let (state, detail) = match outcome {
        Ok(detail) => ("complete", detail),
        Err(error) => ("failed", error),
    };
    log::info("remote", format!("{} {state}: {detail}", command.kind));
    write_receipt(data, &command.command_id, &command.kind, state, &detail);
    finish(data, command, state, &detail)
}

fn finish(data: &Path, command: &Command, state: &str, detail: &str) -> Result<(), String> {
    cloud::call(
        data,
        "mutation",
        "writerSupport:finish",
        json!({ "deviceId": command.device_id, "commandId": command.command_id, "state": state, "detail": detail, "build": release::name() }),
    )
    .map(|_| ())
}

// --- Receipts: a command is carried out at most once on this Mac -------------

fn receipt_path(data: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || id.len() > 120 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("Invalid remote command id.".into());
    }
    Ok(data.join("remote-commands").join(format!("{id}.json")))
}

fn receipt(data: &Path, id: &str) -> Option<Value> {
    fs::read(receipt_path(data, id).ok()?).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// Made only if it isn't there: false when this command was received before.
fn claim_receipt(data: &Path, id: &str, kind: &str) -> Result<bool, String> {
    let path = receipt_path(data, id)?;
    fs::create_dir_all(path.parent().unwrap_or(data)).map_err(|error| error.to_string())?;
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(json!({ "id": id, "kind": kind, "state": "claimed" }).to_string().as_bytes()).map_err(|error| error.to_string())?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn write_receipt(data: &Path, id: &str, kind: &str, state: &str, detail: &str) {
    let Ok(path) = receipt_path(data, id) else { return };
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, json!({ "id": id, "kind": kind, "state": state, "detail": detail }).to_string()).is_ok() {
        let _ = fs::rename(tmp, path);
    }
}

// --- What a command does -------------------------------------------------------

/// Each script here, a line each: its import, and whether the cloud has it.
fn script_lines(data: &Path) -> Vec<String> {
    let rows = match store::list(data) {
        Ok(rows) => rows,
        Err(error) => return vec![format!("(couldn't list the scripts: {error})")],
    };
    rows.iter()
        .map(|row| {
            let cloud = match (cloud::refused(&row.id), cloud::rev_of(data, &row.id)) {
                (Some(why), _) => format!("NOT IN THE CLOUD: {why}"),
                (None, Some(rev)) => format!("cloud rev {rev}"),
                (None, None) => "only on this Mac".to_string(),
            };
            let import = match row.status.as_str() {
                "ready" => "ready".to_string(),
                "processing" => format!("importing: {} {}%", row.stage, row.progress),
                other => format!("{other}{}", row.error.as_deref().map(|error| format!(": {error}")).unwrap_or_default()),
            };
            let decision = if row.decision.is_some() { " · waiting on a choice" } else { "" };
            let building = if jobs::running(&row.id) { " · Claude writing" } else { "" };
            format!("{} \"{}\" ({}) · {import}{decision}{building} · {cloud}", row.id, row.title, row.source_kind)
        })
        .collect()
}

fn log_tail(count: usize) -> Vec<String> {
    let Some(file) = log::file() else { return Vec::new() };
    let text = fs::read_to_string(file).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..].iter().map(|line| line.to_string()).collect()
}

fn os() -> String {
    let version = std::process::Command::new("sw_vers").arg("-productVersion").output().ok().and_then(|out| String::from_utf8(out.stdout).ok()).map(|text| text.trim().to_string()).unwrap_or_default();
    format!("macOS {version} {}", std::env::consts::ARCH).replace("  ", " ")
}

/// The UTC day of a time in ms, as `2026-10-29`.
fn day(ms: i64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn slug(text: &str) -> String {
    let lowered: String = text.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let joined = lowered.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    let cut: String = joined.chars().take(40).collect();
    if cut.is_empty() { "mac".into() } else { cut }
}

/// Sends this Mac's report to the team's reports (a copy stays here); where it went.
pub fn diagnostics(data: &Path) -> Result<String, String> {
    let person = crate::account::status().person;
    let from = person.as_ref().map(|person| format!("{} <{}>", person.name, person.email)).unwrap_or_else(|| "unknown".into());
    let status = cloud::status();
    let work: Vec<String> = activity::list().iter().map(|work| serde_json::to_string(work).unwrap_or_default()).collect();
    let builds: Vec<String> = jobs::running_list().into_iter().map(|(id, title, _)| format!("{id} \"{title}\"")).collect();
    let now = now_ms() as i64;
    let report = [
        format!("From: {from}"),
        format!("Mac: {}", machine::name()),
        format!("Writer: {}", release::name()),
        format!("Sent: {} ({now})", day(now)),
        String::new(),
        "Diagnostics asked for by an admin (just remote diagnostics).".into(),
        format!("{} · {} · device {}", release::name(), os(), machine::id()),
        format!("Cloud: {}{} · {} waiting{}", status.state, status.error.as_deref().map(|error| format!(" ({error})")).unwrap_or_default(), status.pending, status.last_sync.map(|at| format!(" · last synced {}", day(at))).unwrap_or_default()),
        String::new(),
        "Scripts:".into(),
    ]
    .into_iter()
    .chain(script_lines(data))
    .chain([String::new(), "Under way:".into()])
    .chain(work)
    .chain(builds.into_iter().map(|build| format!("Claude writing: {build}")))
    .chain([String::new(), "Log:".into()])
    .chain(log_tail(LOG_LINES))
    .collect::<Vec<_>>()
    .join("\n");
    let mut body = log::redact(&report);
    if body.len() > MAX_REPORT {
        let mut cut = body.len() - MAX_REPORT;
        while !body.is_char_boundary(cut) {
            cut += 1;
        }
        body = format!("(the start was cut)\n{}", &body[cut..]);
    }
    let key = format!("{REPORTS}/{}/{}-{now}.txt", day(now), slug(&machine::name()));
    let kept = data.join("reports-sent");
    let _ = fs::create_dir_all(&kept).and_then(|_| fs::write(kept.join(format!("{}-{now}.txt", day(now))), &body));
    cloud::put_bytes(data, &key, body.as_bytes(), "text/plain; charset=utf-8")?;
    log::info("remote", format!("sent a problem report ({})", log::size(body.len() as u64)));
    Ok(key)
}

/// Picks up a script: the cloud tries it again if it refused it or never got
/// it, and an import that stopped starts again from what it kept. Never an
/// agent turn. What happened, in a line.
pub fn resume(app: &AppHandle, data: &Path, id: &str) -> Result<String, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir).map_err(|_| format!("There's no script {id} on this Mac."))?;
    let mut done = Vec::new();
    match (cloud::refused(id), cloud::rev_of(data, id)) {
        (Some(why), _) => {
            cloud::try_again(id);
            done.push(format!("the cloud refused it ({why}): trying again now"));
        }
        (None, None) => {
            cloud::try_again(id);
            done.push("it wasn't in the cloud: syncing it now".to_string());
        }
        (None, Some(rev)) => done.push(format!("in the cloud (rev {rev})")),
    }
    let words_missing = script.status == "ready" && script.words.is_empty() && !script.no_speech;
    if script.status == "processing" {
        done.push("its import is already running".into());
    } else if script.status == "needs_choice" {
        done.push("its import is waiting on a choice only they can make (open it on their Mac)".into());
    } else if script.status == "error" || words_missing {
        let (app, data, id) = (app.clone(), data.to_path_buf(), id.to_string());
        std::thread::spawn(move || match import::resume(&app, &data, &id, import::Choice::Retry) {
            Ok(_) => log::info("remote", format!("{}: the import an admin resumed finished", log::named(&id, ""))),
            Err(error) => log::warn("remote", format!("{}: the import an admin resumed stopped: {error}", log::named(&id, ""))),
        });
        done.push("its import started again".into());
    } else {
        done.push("its import had finished".into());
    }
    Ok(format!("\"{}\": {}", script.title, done.join("; ")))
}

// --- The admin's side ------------------------------------------------------------

/// What `POST /api/remote` asks (the admin's own Writer, for `just remote`).
pub fn admin(data: &Path, request: &Value) -> Result<Value, String> {
    let device = request["device"].as_str().unwrap_or("");
    let valid = |id: &str| !id.is_empty() && id.len() <= 120 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    match request["action"].as_str().unwrap_or("") {
        "devices" => cloud::call(data, "query", "writerSupport:devices", json!({})),
        "history" if valid(device) => cloud::call(data, "query", "writerSupport:history", json!({ "deviceId": device })),
        kind @ ("diagnostics" | "resume") if valid(device) => {
            let script = request["script"].as_str().unwrap_or("");
            if kind == "resume" && !valid(script) {
                return Err("Give the script's id.".into());
            }
            let command = uuid::Uuid::new_v4().to_string();
            let mut args = json!({ "deviceId": device, "commandId": command, "kind": kind });
            if kind == "resume" {
                args["scriptId"] = json!(script);
            }
            cloud::call(data, "mutation", "writerSupport:requestAction", args)?;
            Ok(json!({ "commandId": command }))
        }
        "report" => {
            let key = request["key"].as_str().unwrap_or("");
            if !key.starts_with(&format!("{REPORTS}/")) || key.contains("..") {
                return Err("Give a report's key (writer/reports/…).".into());
            }
            let bytes = cloud::get_bytes(data, key)?.ok_or("That report isn't there.")?;
            let inbox = data.join("reports-inbox");
            fs::create_dir_all(&inbox).map_err(|error| error.to_string())?;
            let path = inbox.join(key[REPORTS.len() + 1..].replace('/', "_"));
            fs::write(&path, &bytes).map_err(|error| error.to_string())?;
            Ok(json!({ "path": path, "text": String::from_utf8_lossy(&bytes) }))
        }
        "history" | "diagnostics" | "resume" => Err("Give a valid device id.".into()),
        _ => Err("Choose devices, diagnostics, resume, history or report.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_names() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(1_793_232_000_000), "2026-10-29");
        assert_eq!(slug("Sphe's MacBook Pro"), "sphe-s-macbook-pro");
        assert_eq!(slug("…"), "mac");
    }

    #[test]
    fn a_command_is_received_once() {
        let data = std::env::temp_dir().join(format!("writer-remote-{}", uuid::Uuid::new_v4()));
        assert!(claim_receipt(&data, "c-1", "diagnostics").unwrap());
        assert!(!claim_receipt(&data, "c-1", "diagnostics").unwrap());
        write_receipt(&data, "c-1", "diagnostics", "complete", "Sent: x");
        assert_eq!(receipt(&data, "c-1").unwrap()["state"], "complete");
        assert!(claim_receipt(&data, "../x", "resume").is_err());
        let _ = fs::remove_dir_all(data);
    }
}
