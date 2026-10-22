//! Every script in the cloud, as Edit keeps its projects: each script's body
//! (script.json) and files (the proxy, the original, transcript pieces,
//! sources, research) on Cloudflare R2 under `writer/scripts/<id>/`, and an
//! index in Convex (Repack's deployment, the `writer` module) naming each
//! script's current rev and files. Writer reaches both only through its relay,
//! which hands out short-lived presigned URLs and calls `writer:` functions.
//!
//! Saves land on disk as always. A background thread pushes what changed a
//! few seconds later (the changed files, then a new body, then a commit naming
//! the rev it began from) and pulls what other Macs pushed. A commit another
//! Mac overtook is refused; then this Mac's version is kept as a "(conflicted
//! copy)" and the cloud's taken, never an overwrite. Scripts still importing
//! or being written by Claude wait until they are done. Deleting a script
//! deletes it everywhere.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::model::{now_ms, Script};
use crate::{import, jobs, settings, store};

/// How often the loop looks for changes, and how often it asks the cloud.
const TICK: Duration = Duration::from_secs(3);
const PULL_EVERY: Duration = Duration::from_secs(30);
/// A save is pushed once the script has been still this long.
const SETTLE_MS: i64 = 2_000;
const PREFIX: &str = "writer/scripts";
/// The lessons kept from reviews (feedback.rs), one file every Mac merges into.
const LESSONS_KEY: &str = "writer/lessons.json";

/// What the app shows: off, syncing, synced, offline or a problem.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub state: String,
    /// Scripts waiting to go up or come down.
    pub pending: usize,
    pub transfer: Option<Transfer>,
    pub last_sync: Option<i64>,
    pub error: Option<String>,
    /// Scripts the cloud holds.
    pub scripts: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub script: String,
    pub name: String,
    pub up: bool,
    pub done: u64,
    pub total: u64,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);
/// Ask the cloud now rather than at the next half minute.
static PULL_NOW: Mutex<bool> = Mutex::new(true);

pub fn status() -> Status {
    STATUS.lock().map(|status| status.clone().unwrap_or_default()).unwrap_or_default()
}

fn set(change: impl FnOnce(&mut Status)) {
    if let Ok(mut status) = STATUS.lock() {
        change(status.get_or_insert_with(Status::default));
    }
}

/// What this Mac knows of each script in the cloud.
#[derive(Default, Serialize, Deserialize)]
struct Ledger {
    #[serde(default)]
    scripts: BTreeMap<String, Known>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Known {
    rev: u64,
    /// The script.json this rev was, by its hash.
    body: String,
    files: BTreeMap<String, Kept>,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
struct Kept {
    key: String,
    size: u64,
    /// This Mac's modified time, in ms: a file whose size and time match is unchanged.
    #[serde(default)]
    mtime: u64,
}

fn ledger_path(data: &Path) -> PathBuf {
    data.join("cloud").join("ledger.json")
}

fn load(data: &Path) -> Ledger {
    fs::read(ledger_path(data)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(data: &Path, ledger: &Ledger) {
    let path = ledger_path(data);
    let _ = fs::create_dir_all(path.parent().unwrap_or(data));
    let tmp = path.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec_pretty(ledger) {
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}

/// Scripts deleted on this Mac, not yet deleted in the cloud. A file of its
/// own, so deleting never waits on a sync that is uploading footage.
static DELETING: Mutex<()> = Mutex::new(());

fn deleting_path(data: &Path) -> PathBuf {
    data.join("cloud").join("deleted.json")
}

fn deleting(data: &Path) -> Vec<String> {
    let _guard = DELETING.lock();
    fs::read(deleting_path(data)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn set_deleting(data: &Path, change: impl FnOnce(&mut Vec<String>)) {
    let _guard = DELETING.lock();
    let path = deleting_path(data);
    let mut list: Vec<String> = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    change(&mut list);
    let _ = fs::create_dir_all(path.parent().unwrap_or(data));
    let _ = fs::write(&path, serde_json::to_vec(&list).unwrap_or_default());
}

/// A script deleted on this Mac: deleted from the cloud, and so from every
/// other Mac, by the next round.
pub fn deleted(data: &Path, id: &str) {
    set_deleting(data, |list| {
        if !list.iter().any(|other| other == id) {
            list.push(id.to_string());
        }
    });
}

/// Syncs on the next tick rather than waiting for the next pull.
pub fn sync_soon() {
    if let Ok(mut now) = PULL_NOW.lock() {
        *now = true;
    }
}

/// Beyond the sync: one call through the relay to a Convex function it lets
/// Writer reach besides its own (`reviews:*`, always as Writer, and Edit's
/// `edit:getMedia`/`edit:putMedia`), for sharing (reviews.rs) and the
/// handoff to Edit. Answers the function's value, or what it refused with.
pub fn call(data: &Path, kind: &str, path: &str, args: Value) -> Result<Value, String> {
    let cloud = Cloud::new(data).ok_or(OFF)?;
    let answer = cloud.relay("convex", json!({ "kind": kind, "path": path, "args": args })).map_err(|problem| problem.message)?;
    if answer["status"] == "success" {
        return Ok(answer["value"].clone());
    }
    let said = answer["errorData"]["message"].as_str().or(answer["errorMessage"].as_str()).unwrap_or("unknown error");
    // Convex leads with "[Request ID: …] Server Error" and the error's class.
    let said = said.rsplit("Uncaught ConvexError: ").next().unwrap_or(said);
    let said = said.rsplit("Uncaught Error: ").next().unwrap_or(said);
    Err(format!("The cloud refused {path}: {}", said.lines().last().unwrap_or(said).trim()))
}

/// Puts bytes at a key Writer may write (its own, or Edit's shared prefixes).
pub fn put_bytes(data: &Path, key: &str, bytes: &[u8], content_type: &str) -> Result<(), String> {
    let cloud = Cloud::new(data).ok_or(OFF)?;
    let url = cloud.url(key, "PUT").map_err(|problem| problem.message)?;
    cloud.long.put(&url).set("content-type", content_type).send_bytes(bytes).map_err(|error| transfer_problem(error, key).message)?;
    Ok(())
}

/// Uploads a file to a key, its progress shown under `script` as the sync's is.
pub fn put_file(data: &Path, script: &str, key: &str, path: &Path) -> Result<u64, String> {
    let cloud = Cloud::new(data).ok_or(OFF)?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("file").to_string();
    cloud.upload(script, &name, path, key).map_err(|problem| problem.message)
}

/// The size of what is at a key, or nothing there.
pub fn head(data: &Path, key: &str) -> Result<Option<u64>, String> {
    let cloud = Cloud::new(data).ok_or(OFF)?;
    let answer = cloud.relay("r2/head", json!({ "key": key })).map_err(|problem| problem.message)?;
    Ok((answer["exists"] == true).then(|| answer["size"].as_u64().unwrap_or(0)))
}

/// What is at a key, or nothing there.
pub fn get_bytes(data: &Path, key: &str) -> Result<Option<Vec<u8>>, String> {
    let cloud = Cloud::new(data).ok_or(OFF)?;
    let url = cloud.url(key, "GET").map_err(|problem| problem.message)?;
    match cloud.long.get(&url).call() {
        Ok(response) => {
            let mut bytes = Vec::new();
            response.into_reader().read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            Ok(Some(bytes))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(error) => Err(transfer_problem(error, key).message),
    }
}

/// Who this Mac is, as commits and claims name it.
pub fn holder_name() -> String {
    holder()
}

const OFF: &str = "This copy of Writer has no cloud: it needs its relay token.";

// ---- Send to Edit: a mark on the script's row in Convex (writer:setHandoff) ----

/// A script marked ready for Edit: when, by whom, at which rev. Kept beside
/// the ledger, never in script.json, so marking never makes a new rev.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Handoff {
    pub at: i64,
    pub by: String,
    pub rev: u64,
    /// The script has been saved since it was marked.
    pub changed: bool,
}

fn handoffs_path(data: &Path) -> PathBuf {
    data.join("cloud").join("handoffs.json")
}

static HANDOFFS: Mutex<()> = Mutex::new(());

fn keep_handoffs(data: &Path, change: impl FnOnce(&mut BTreeMap<String, Value>)) {
    let _guard = HANDOFFS.lock();
    let path = handoffs_path(data);
    let mut known: BTreeMap<String, Value> = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    change(&mut known);
    let _ = fs::create_dir_all(path.parent().unwrap_or(data));
    let _ = fs::write(&path, serde_json::to_vec(&known).unwrap_or_default());
}

/// The script's mark for Edit, if it has one.
pub fn handoff(data: &Path, id: &str) -> Option<Handoff> {
    let known: BTreeMap<String, Value> = fs::read(handoffs_path(data)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let mark = known.get(id)?;
    let rev = mark["rev"].as_f64()? as u64;
    let ledger = load(data);
    let synced = ledger.scripts.get(id);
    let dir = store::script_dir(data, id).ok()?;
    let changed = synced.is_none_or(|known| known.rev > rev || changed_here(&dir, Some(known)));
    Some(Handoff { at: mark["at"].as_f64().unwrap_or(0.0) as i64, by: mark["by"].as_str().unwrap_or("").to_string(), rev, changed })
}

/// Marks the script ready for Edit (or clears the mark). It must be in the
/// cloud as it is now: Edit reads it from there.
pub fn set_handoff(data: &Path, id: &str, ready: bool) -> Result<Option<Handoff>, String> {
    if ready {
        let dir = store::script_dir(data, id)?;
        let synced = load(data).scripts.get(id).cloned();
        if synced.is_none() || changed_here(&dir, synced.as_ref()) {
            sync_soon();
            return Err("The script isn't in the cloud as it is now yet. Send it again once the cloud says Synced.".into());
        }
    }
    let value = call(data, "mutation", "writer:setHandoff", json!({ "scriptId": id, "ready": ready, "by": format!("Writer on {}", holder()) }))?;
    let mark = value["handoff"].clone();
    keep_handoffs(data, |known| {
        if mark.is_object() {
            known.insert(id.to_string(), mark);
        } else {
            known.remove(id);
        }
    });
    Ok(handoff(data, id))
}

/// Starts syncing in the background, for as long as Writer runs.
pub fn start(data: PathBuf) {
    std::thread::spawn(move || {
        let mut last_pull: Option<Instant> = None;
        loop {
            let pull = last_pull.is_none_or(|at| at.elapsed() >= PULL_EVERY) || PULL_NOW.lock().map(|now| *now).unwrap_or(false);
            match Cloud::new(&data) {
                None => set(|status| {
                    *status = Status { state: "off".into(), ..Status::default() };
                }),
                Some(cloud) => {
                    set(|status| status.enabled = true);
                    match cloud.tick(&data, pull) {
                        Ok(pending) => set(|status| {
                            status.pending = pending;
                            status.state = if pending > 0 { "syncing".into() } else { "synced".into() };
                            status.error = None;
                            status.transfer = None;
                            status.last_sync = Some(now_ms());
                        }),
                        Err(problem) => set(|status| {
                            status.state = if problem.offline { "offline".into() } else { "error".into() };
                            status.error = Some(problem.message);
                            status.transfer = None;
                        }),
                    }
                    if pull {
                        last_pull = Some(Instant::now());
                        if let Ok(mut now) = PULL_NOW.lock() {
                            *now = false;
                        }
                    }
                }
            }
            std::thread::sleep(TICK);
        }
    });
}

struct Problem {
    message: String,
    /// The connection, not the cloud: waits quietly and tries again.
    offline: bool,
}

impl Problem {
    fn said(message: impl Into<String>) -> Self {
        Self { message: message.into(), offline: false }
    }
}

type Done<T> = Result<T, Problem>;

struct Cloud {
    base: String,
    token: String,
    holder: String,
    quick: ureq::Agent,
    long: ureq::Agent,
}

impl Cloud {
    /// The cloud is on whenever this copy of Writer has its relay token.
    fn new(data: &Path) -> Option<Self> {
        let settings = settings::load(data);
        let base = settings::check_relay(&settings.relay_url).ok()?;
        if settings.relay_token.is_empty() || settings.relay_token.chars().any(char::is_control) {
            return None;
        }
        let quick = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout(Duration::from_secs(60))
            .user_agent("Writer")
            .build();
        // Footage takes as long as it takes; only a stall fails it.
        let long = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(30))
            .timeout_read(Duration::from_secs(120))
            .timeout_write(Duration::from_secs(120))
            .user_agent("Writer")
            .build();
        Some(Self { base, token: settings.relay_token, holder: holder(), quick, long })
    }

    fn relay(&self, route: &str, body: Value) -> Done<Value> {
        let answer = self
            .quick
            .post(&format!("{}/v1/cloud/{route}", self.base))
            .set("authorization", &format!("Bearer {}", self.token))
            .set("content-type", "application/json")
            .send_json(body);
        match answer {
            Ok(response) => response.into_json().map_err(|_| Problem::said("Writer's service answered with something Writer can't read.")),
            Err(ureq::Error::Status(401, _)) => Err(Problem::said("Writer's service turned this copy of Writer away. Get a fresh copy of Writer.")),
            Err(ureq::Error::Status(code, response)) => {
                let said = crate::net::said(&response.into_string().unwrap_or_default());
                Err(Problem::said(if said.is_empty() { format!("The cloud answered {code}.") } else { said }))
            }
            Err(ureq::Error::Transport(transport)) => Err(Problem { message: format!("Can't reach the cloud ({transport})."), offline: true }),
        }
    }

    /// A `writer:` Convex function's value, or its refusal with its code.
    fn convex(&self, kind: &str, name: &str, args: Value) -> Done<Result<Value, (String, Value)>> {
        let answer = self.relay("convex", json!({ "kind": kind, "path": format!("writer:{name}"), "args": args }))?;
        if answer["status"] == "success" {
            return Ok(Ok(answer["value"].clone()));
        }
        let data = answer["errorData"].clone();
        let code = data["code"].as_str().unwrap_or("").to_string();
        if code.is_empty() {
            return Err(Problem::said(format!("The cloud refused: {}", answer["errorMessage"].as_str().unwrap_or("unknown error"))));
        }
        Ok(Err((code, data)))
    }

    fn url(&self, key: &str, method: &str) -> Done<String> {
        let answer = self.relay("r2/url", json!({ "key": key, "method": method }))?;
        answer["url"].as_str().map(str::to_string).ok_or_else(|| Problem::said("The cloud gave no address for a file."))
    }

    fn remove(&self, keys: &[String]) {
        for batch in keys.chunks(40) {
            let _ = self.relay("r2/delete", json!({ "keys": batch }));
        }
    }

    /// Uploads a file, its progress shown as it goes.
    fn upload(&self, script: &str, name: &str, path: &Path, key: &str) -> Done<u64> {
        let size = fs::metadata(path).map_err(|error| Problem::said(error.to_string()))?.len();
        let url = self.url(key, "PUT")?;
        let file = File::open(path).map_err(|error| Problem::said(error.to_string()))?;
        let counted = Counted { inner: file, done: 0, total: size, script: script.to_string(), name: name.to_string(), up: true };
        self.long
            .put(&url)
            .set("content-length", &size.to_string())
            .send(counted)
            .map_err(|error| transfer_problem(error, name))?;
        set(|status| status.transfer = None);
        Ok(size)
    }

    fn upload_bytes(&self, key: &str, bytes: &[u8]) -> Done<()> {
        let url = self.url(key, "PUT")?;
        self.long
            .put(&url)
            .set("content-type", "application/json")
            .send_bytes(bytes)
            .map_err(|error| transfer_problem(error, "script.json"))?;
        Ok(())
    }

    /// Downloads a file beside its place, then moves it in whole.
    fn download(&self, script: &str, name: &str, key: &str, size: u64, path: &Path) -> Done<()> {
        let url = self.url(key, "GET")?;
        let response = self.long.get(&url).call().map_err(|error| transfer_problem(error, name))?;
        let partial = path.with_file_name(format!("{}.partial", path.file_name().and_then(|n| n.to_str()).unwrap_or("file")));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| Problem::said(error.to_string()))?;
        }
        let mut out = File::create(&partial).map_err(|error| Problem::said(error.to_string()))?;
        let mut reader = Counted { inner: response.into_reader(), done: 0, total: size, script: script.to_string(), name: name.to_string(), up: false };
        std::io::copy(&mut reader, &mut out).map_err(|error| Problem { message: format!("{name} stopped coming down ({error})."), offline: true })?;
        out.flush().map_err(|error| Problem::said(error.to_string()))?;
        set(|status| status.transfer = None);
        fs::rename(&partial, path).map_err(|error| Problem::said(error.to_string()))
    }

    fn fetch(&self, key: &str) -> Done<Vec<u8>> {
        let url = self.url(key, "GET")?;
        let response = self.long.get(&url).call().map_err(|error| transfer_problem(error, "script.json"))?;
        let mut bytes = Vec::new();
        response.into_reader().read_to_end(&mut bytes).map_err(|error| Problem { message: error.to_string(), offline: true })?;
        Ok(bytes)
    }

    /// One round: deletions, then what came from other Macs, then what changed here.
    /// Answers how many scripts still wait.
    fn tick(&self, data: &Path, pull: bool) -> Done<usize> {
        let mut ledger = load(data);
        let mut waiting = 0;

        let gone = deleting(data);
        for id in &gone {
            if let Ok(answer) = self.convex("mutation", "deleteScript", json!({ "scriptId": id, "holder": self.holder }))? {
                let keys: Vec<String> = answer["keys"].as_array().into_iter().flatten().filter_map(|key| key.as_str().map(str::to_string)).collect();
                self.remove(&keys);
                ledger.scripts.remove(id);
                save(data, &ledger);
                set_deleting(data, |list| list.retain(|other| other != id));
            }
        }
        let gone = deleting(data);

        if pull {
            let rows = match self.convex("query", "listScripts", json!({}))? {
                Ok(value) => value.as_array().cloned().unwrap_or_default(),
                Err((code, _)) => return Err(Problem::said(format!("The cloud refused to list scripts ({code})."))),
            };
            set(|status| status.scripts = rows.iter().filter(|row| row["deletedAt"].is_null()).count());
            // Which scripts are marked for Edit, and at what rev, as the cloud says.
            let marks: BTreeMap<String, Value> = rows
                .iter()
                .filter_map(|row| Some((row["scriptId"].as_str()?.to_string(), row["handoff"].clone())))
                .filter(|(_, mark)| mark.is_object())
                .collect();
            keep_handoffs(data, |known| *known = marks);
            for row in rows {
                if gone.iter().any(|id| row["scriptId"].as_str() == Some(id)) {
                    continue;
                }
                match self.pull(data, &mut ledger, &row) {
                    Ok(()) => {}
                    Err(problem) if problem.offline => return Err(problem),
                    Err(_) => waiting += 1,
                }
                save(data, &ledger);
            }
        }

        let root = store::scripts_root(data);
        let mut ids: Vec<String> = fs::read_dir(&root)
            .map(|entries| entries.flatten().filter(|entry| entry.path().is_dir()).filter_map(|entry| entry.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        ids.sort();
        // What changed here, found first so the count of what is left can be shown.
        let mut changed = Vec::new();
        for id in ids {
            if !store::id_ok(&id) || busy(&id) {
                continue;
            }
            let dir = root.join(&id);
            let Ok(body) = fs::read(dir.join("script.json")) else { continue };
            let Ok(script) = serde_json::from_slice::<Script>(&body) else { continue };
            if script.status == "processing" || now_ms() - script.updated_at < SETTLE_MS {
                waiting += 1;
                continue;
            }
            let known = ledger.scripts.get(&id).cloned();
            let files = local_files(&dir, &script);
            if known.as_ref().is_some_and(|known| known.body == hash(&body) && same_files(&known.files, &files)) {
                continue;
            }
            changed.push((id, dir, script, body, known, files));
        }
        let mut left = changed.len();
        for (id, dir, script, body, known, files) in changed {
            set(|status| {
                status.state = "syncing".into();
                status.pending = left + waiting;
            });
            match self.push(&id, &dir, &script, &body, known, files)? {
                Pushed::Done(known) => {
                    ledger.scripts.insert(id, known);
                    save(data, &ledger);
                }
                Pushed::Overtaken => {
                    waiting += 1;
                    if let Ok(mut now) = PULL_NOW.lock() {
                        *now = true;
                    }
                }
            }
            left -= 1;
        }
        if pull {
            self.sync_lessons(data)?;
        }
        Ok(waiting)
    }

    /// Merges the kept lessons both ways: this Mac's and the cloud's, each
    /// lesson by its ID and each one forgotten anywhere gone everywhere.
    fn sync_lessons(&self, data: &Path) -> Done<()> {
        let url = self.url(LESSONS_KEY, "GET")?;
        let remote = match self.quick.get(&url).call() {
            Ok(response) => {
                let mut bytes = Vec::new();
                response.into_reader().read_to_end(&mut bytes).map_err(|error| Problem { message: error.to_string(), offline: true })?;
                Some(bytes)
            }
            Err(ureq::Error::Status(404, _)) => None,
            Err(error) => return Err(transfer_problem(error, "lessons.json")),
        };
        let (merged, upload) = crate::feedback::merge_lessons(data, remote.as_deref()).map_err(Problem::said)?;
        if upload {
            self.upload_bytes(LESSONS_KEY, &merged)?;
        }
        Ok(())
    }

    /// Brings one script from the cloud when it is newer than this Mac's.
    fn pull(&self, data: &Path, ledger: &mut Ledger, row: &Value) -> Done<()> {
        let id = row["scriptId"].as_str().unwrap_or("").to_string();
        if !store::id_ok(&id) || busy(&id) {
            return Ok(());
        }
        let rev = row["rev"].as_u64().unwrap_or(0);
        let dir = store::scripts_root(data).join(&id);
        let known = ledger.scripts.get(&id).cloned();
        if !row["deletedAt"].is_null() {
            // Deleted on another Mac: gone here too, unless this Mac changed it since.
            if dir.is_dir() && !changed_here(&dir, known.as_ref()) {
                let _ = fs::remove_dir_all(&dir);
            }
            ledger.scripts.remove(&id);
            return Ok(());
        }
        if known.as_ref().is_some_and(|known| known.rev >= rev) {
            return Ok(());
        }
        let body = self.fetch(row["bodyKey"].as_str().unwrap_or(""))?;
        if dir.is_dir() {
            let here = fs::read(dir.join("script.json")).unwrap_or_default();
            if hash(&here) == hash(&body) {
                // The same script, as on a first sync of a Mac that already has it.
            } else if changed_here(&dir, known.as_ref()) {
                keep_copy(data, &dir)?;
            }
        }
        fs::create_dir_all(&dir).map_err(|error| Problem::said(error.to_string()))?;
        let mut files = BTreeMap::new();
        for file in row["files"].as_array().into_iter().flatten() {
            let (Some(name), Some(key)) = (file["name"].as_str(), file["key"].as_str()) else { continue };
            let size = file["size"].as_u64().unwrap_or(0);
            let Some(path) = inside(&dir, name) else { continue };
            // A small JSON file (the chat, versions, research) can change and
            // keep its size, so on a newer script it is always fetched again.
            let small_json = name.ends_with(".json") && size < 2_000_000;
            let present = !small_json && fs::metadata(&path).map(|meta| meta.len() == size).unwrap_or(false);
            if !present {
                self.download(&id, name, key, size, &path)?;
            }
            files.insert(name.to_string(), Kept { key: key.to_string(), size, mtime: mtime(&path) });
        }
        let tmp = dir.join("script.json.tmp");
        fs::write(&tmp, &body).and_then(|_| fs::rename(&tmp, dir.join("script.json"))).map_err(|error| Problem::said(error.to_string()))?;
        ledger.scripts.insert(id, Known { rev, body: hash(&body), files });
        Ok(())
    }

    /// Sends a script's changed files and its body, then commits them.
    fn push(&self, id: &str, dir: &Path, script: &Script, body: &[u8], known: Option<Known>, files: BTreeMap<String, Kept>) -> Done<Pushed> {
        let base = known.as_ref().map_or(0, |known| known.rev);
        let mut kept = BTreeMap::new();
        for (name, local) in files {
            let previous = known.as_ref().and_then(|known| known.files.get(&name));
            let key = format!("{PREFIX}/{id}/files/{name}");
            if previous.is_some_and(|previous| previous.size == local.size && previous.mtime == local.mtime) {
                kept.insert(name, previous.cloned().unwrap_or_default());
                continue;
            }
            let Some(path) = inside(dir, &name) else { continue };
            // A file that vanished while it went up (a temp piece) is left out.
            match self.upload(id, &name, &path, &key) {
                Ok(size) => {
                    kept.insert(name, Kept { key, size, mtime: local.mtime });
                }
                Err(problem) if problem.offline => return Err(problem),
                Err(_) if !path.exists() => continue,
                Err(problem) => return Err(problem),
            }
        }
        let digest = hash(body);
        let body_key = format!("{PREFIX}/{id}/body/{}-{}.json", base + 1, &digest[..12]);
        self.upload_bytes(&body_key, body)?;
        let listed: Vec<Value> = kept.iter().map(|(name, file)| json!({ "name": name, "key": file.key, "size": file.size })).collect();
        let summary = serde_json::to_value(script.summary()).unwrap_or(Value::Null);
        let answer = self.convex(
            "mutation",
            "commitScript",
            json!({
                "scriptId": id,
                "baseRev": base,
                "title": script.title,
                "bodyKey": body_key,
                "bodySize": body.len(),
                "files": listed,
                "summary": summary,
                "holder": self.holder,
            }),
        )?;
        match answer {
            Ok(value) => {
                let stale: Vec<String> = value["stale"].as_array().into_iter().flatten().filter_map(|key| key.as_str().map(str::to_string)).collect();
                self.remove(&stale);
                Ok(Pushed::Done(Known { rev: value["rev"].as_u64().unwrap_or(base + 1), body: digest, files: kept }))
            }
            Err((code, _)) if code == "conflict" => {
                self.remove(&[body_key]);
                Ok(Pushed::Overtaken)
            }
            Err((code, data)) => Err(Problem::said(format!(
                "The cloud refused {}: {}",
                script.title,
                data["message"].as_str().unwrap_or(&code)
            ))),
        }
    }
}

enum Pushed {
    Done(Known),
    /// Another Mac committed first: the next pull keeps this version as a copy.
    Overtaken,
}

/// A script Writer is working on now: importing or being written.
fn busy(id: &str) -> bool {
    import::importing(id) || jobs::running(id)
}

/// Every file of a script but its body, by its path inside the script's
/// folder. A joined case's recordings are left out: they are Edit's
/// originals (shared under edit/originals/), and the joined preview is what
/// another Mac plays.
fn local_files(dir: &Path, script: &Script) -> BTreeMap<String, Kept> {
    let original = Path::new(&script.original_path);
    let mut found = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        let Ok(entries) = fs::read_dir(&at) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.ends_with(".tmp") || name.ends_with(".partial") {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(relative) = path.strip_prefix(dir) else { continue };
            let relative = relative.to_string_lossy().replace('\\', "/");
            // A build's state belongs to the Mac running it: another Mac
            // would read a build running there as one Writer closed on.
            if relative == "script.json" || relative == "build.json" {
                continue;
            }
            let recording = (relative.starts_with("drive/") || relative.starts_with("compilation/")) && !relative.ends_with(".json");
            if recording && path != original {
                continue;
            }
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            found.insert(relative, Kept { key: String::new(), size, mtime: mtime(&path) });
        }
    }
    found
}

fn same_files(known: &BTreeMap<String, Kept>, here: &BTreeMap<String, Kept>) -> bool {
    known.len() == here.len()
        && here.iter().all(|(name, file)| known.get(name).is_some_and(|other| other.size == file.size && other.mtime == file.mtime))
}

/// Whether this Mac has changed a script since it last synced it.
fn changed_here(dir: &Path, known: Option<&Known>) -> bool {
    let body = fs::read(dir.join("script.json")).unwrap_or_default();
    match known {
        None => true,
        Some(known) => known.body != hash(&body),
    }
}

/// This Mac's version of a script the cloud overtook, kept beside it under a new id.
fn keep_copy(data: &Path, dir: &Path) -> Done<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let copy = store::scripts_root(data).join(&id);
    copy_dir(dir, &copy).map_err(|error| Problem::said(error.to_string()))?;
    let mut script = store::read(&copy).map_err(Problem::said)?;
    script.id = id;
    script.title = format!("{} (conflicted copy)", script.title);
    script.updated_at = now_ms();
    store::write(&copy, &script).map_err(Problem::said)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A file's place inside a script's folder, refusing anything that would leave it.
fn inside(dir: &Path, name: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = name.split('/').collect();
    if parts.iter().any(|part| part.is_empty() || *part == "." || *part == ".." || part.contains('\\')) {
        return None;
    }
    Some(parts.iter().fold(dir.to_path_buf(), |path, part| path.join(part)))
}

fn mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_millis() as u64)
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// This Mac's name, so the cloud says who saved what.
fn holder() -> String {
    std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Writer".into())
        .chars()
        .take(120)
        .collect()
}

fn transfer_problem(error: ureq::Error, name: &str) -> Problem {
    match error {
        ureq::Error::Status(code, _) => Problem::said(format!("The cloud refused {name} ({code}).")),
        ureq::Error::Transport(transport) => Problem { message: format!("{name} didn't get through ({transport})."), offline: true },
    }
}

/// A reader that reports how far a transfer has got.
struct Counted<R> {
    inner: R,
    done: u64,
    total: u64,
    script: String,
    name: String,
    up: bool,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.done += read as u64;
        let transfer = Transfer { script: self.script.clone(), name: self.name.clone(), up: self.up, done: self.done, total: self.total };
        set(|status| {
            status.state = "syncing".into();
            status.transfer = Some(transfer);
        });
        Ok(read)
    }
}
