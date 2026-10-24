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
/// Files bigger than this go up in parts of PART_SIZE (R2 takes 5 GiB in one PUT).
const MULTIPART_OVER: u64 = 100_000_000;
const PART_SIZE: u64 = 64 * 1024 * 1024;
/// A script the cloud keeps refusing waits this long, doubling to BACKOFF_MAX, while the rest sync.
const BACKOFF_FIRST: Duration = Duration::from_secs(30);
const BACKOFF_MAX: Duration = Duration::from_secs(3600);
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
/// Scripts whose push failed (not for being offline): when to try again, and how long it waited last.
static BACKOFF: Mutex<BTreeMap<String, (Instant, Duration)>> = Mutex::new(BTreeMap::new());

fn backed_off(id: &str) -> bool {
    BACKOFF.lock().ok().and_then(|map| map.get(id).map(|(until, _)| *until > Instant::now())).unwrap_or(false)
}

fn back_off(id: &str) -> Duration {
    let Ok(mut map) = BACKOFF.lock() else { return BACKOFF_FIRST };
    let wait = map.get(id).map_or(BACKOFF_FIRST, |(_, last)| (*last * 2).min(BACKOFF_MAX));
    map.insert(id.to_string(), (Instant::now() + wait, wait));
    wait
}

fn forgive(id: &str) {
    if let Ok(mut map) = BACKOFF.lock() {
        map.remove(id);
    }
}

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
    /// Which shape of library row (`summary`) the cloud has for it: one
    /// committed before a field was added is committed again, body unchanged.
    #[serde(default)]
    summary: u32,
}

/// The library row's shape: 1 added `kind`, which Edit's New project dialog shows and filters by.
const SUMMARY_VERSION: u32 = 1;

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

/// Downloads what is at a key into a file, its progress shown under `script`.
pub fn get_file(data: &Path, script: &str, key: &str, size: u64, path: &Path) -> Result<(), String> {
    let cloud = Cloud::new(data).ok_or(OFF)?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("file").to_string();
    cloud.download(script, &name, key, size, path).map_err(|problem| problem.message)
}

/// A script file's key and size in the cloud, as this Mac last synced it.
pub fn file_key(data: &Path, id: &str, name: &str) -> Option<(String, u64)> {
    let ledger = load(data);
    let kept = ledger.scripts.get(id)?.files.get(name)?;
    Some((kept.key.clone(), kept.size))
}

/// Verified in the cloud: the object at a script file's ledger key is
/// there, at the size the ledger and this Mac agree on. The ledger alone
/// is only what this Mac last believed; this is the signal share and
/// storage deletion gate on (an `r2/head` through the relay).
pub fn verify_file(data: &Path, id: &str, name: &str, local_size: u64) -> Result<bool, String> {
    let Some((key, size)) = file_key(data, id, name) else { return Ok(false) };
    if size != local_size {
        return Ok(false);
    }
    Ok(head(data, &key)? == Some(local_size))
}

/// Verified at a key outside the script's own files (Edit's originals,
/// for a joined case's recordings): the object is there at `size`.
pub fn verify_key(data: &Path, key: &str, size: u64) -> Result<bool, String> {
    Ok(head(data, key)? == Some(size))
}

/// What the cloud holds of a script, by file name (storage.rs: footage
/// whose name and size are here has reached the cloud).
pub fn synced_files(data: &Path, id: &str) -> BTreeMap<String, u64> {
    load(data).scripts.get(id).map(|known| known.files.iter().map(|(name, kept)| (name.clone(), kept.size)).collect()).unwrap_or_default()
}

/// Who this Mac is, as commits and claims name it.
pub fn holder_name() -> String {
    holder()
}

const OFF: &str = "This copy of Writer has no cloud: it needs its relay token.";


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
                    let outcome = cloud.tick(&data, pull);
                    let was = status();
                    match &outcome {
                        Ok(_) if was.state == "offline" || was.state == "error" => crate::log::info("cloud", "back in sync"),
                        Err(problem) if was.error.as_deref() != Some(problem.message.as_str()) => {
                            if problem.offline {
                                crate::log::warn("cloud", format!("offline: {}", problem.message));
                            } else {
                                crate::log::error("cloud", format!("sync stopped: {}", problem.message));
                            }
                        }
                        _ => {}
                    }
                    match outcome {
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
    holder: String,
    quick: ureq::Agent,
    long: ureq::Agent,
}

impl Cloud {
    /// The cloud is on whenever someone is signed in.
    fn new(data: &Path) -> Option<Self> {
        let settings = settings::load(data);
        let base = settings::check_relay(&settings.relay_url).ok()?;
        if !crate::account::can_call() {
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
        Some(Self { base, holder: holder(), quick, long })
    }

    fn relay(&self, route: &str, body: Value) -> Done<Value> {
        let Some(authorization) = crate::account::bearer() else {
            return Err(Problem::said("Sign in to Writer to use the cloud."));
        };
        let answer = self
            .quick
            .post(&format!("{}/v1/cloud/{route}", self.base))
            .set("authorization", &authorization)
            .set("content-type", "application/json")
            .send_json(body);
        match answer {
            Ok(response) => response.into_json().map_err(|_| Problem::said("Writer's service answered with something Writer can't read.")),
            Err(ureq::Error::Status(code @ (401 | 403), response)) => {
                Err(Problem::said(crate::account::turned_away(code, &crate::net::said(&response.into_string().unwrap_or_default()))))
            }
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

    /// Uploads a file, its progress shown as it goes: in parts past
    /// MULTIPART_OVER (one PUT to R2 takes at most 5 GiB).
    fn upload(&self, script: &str, name: &str, path: &Path, key: &str) -> Done<u64> {
        let size = fs::metadata(path).map_err(|error| Problem::said(error.to_string()))?.len();
        if size > MULTIPART_OVER {
            return self.upload_parts(script, name, path, key, size);
        }
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

    /// A big file in parts of PART_SIZE: each a presigned PUT, then completed
    /// (or abandoned, so R2 keeps no half of it).
    fn upload_parts(&self, script: &str, name: &str, path: &Path, key: &str, size: u64) -> Done<u64> {
        let created = self.relay("r2/multipart/create", json!({ "key": key }))?;
        let upload_id = created["uploadId"].as_str().ok_or_else(|| Problem::said("The cloud gave no upload id."))?.to_string();
        let count = size.div_ceil(PART_SIZE);
        let outcome = (|| -> Done<()> {
            let mut parts = Vec::new();
            let mut file = File::open(path).map_err(|error| Problem::said(error.to_string()))?;
            for first in (1..=count).step_by(100) {
                let numbers: Vec<u64> = (first..=count.min(first + 99)).collect();
                let urls = self.relay("r2/multipart/urls", json!({ "key": key, "uploadId": upload_id, "parts": numbers }))?;
                for number in numbers {
                    let url = urls["urls"][number.to_string()].as_str().ok_or_else(|| Problem::said("The cloud gave no part URL."))?.to_string();
                    let length = PART_SIZE.min(size - (number - 1) * PART_SIZE);
                    let mut chunk = vec![0u8; length as usize];
                    file.read_exact(&mut chunk).map_err(|error| Problem::said(error.to_string()))?;
                    let response = self.long.put(&url).set("content-length", &length.to_string()).send_bytes(&chunk).map_err(|error| transfer_problem(error, name))?;
                    let etag = response.header("etag").unwrap_or("").to_string();
                    parts.push(json!({ "partNumber": number, "etag": etag }));
                    set(|status| status.transfer = Some(Transfer { script: script.to_string(), name: name.to_string(), done: (number * PART_SIZE).min(size), total: size, up: true }));
                }
            }
            self.relay("r2/multipart/complete", json!({ "key": key, "uploadId": upload_id, "parts": parts }))?;
            Ok(())
        })();
        if let Err(problem) = outcome {
            let _ = self.relay("r2/multipart/abort", json!({ "key": key, "uploadId": upload_id }));
            return Err(problem);
        }
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
            // Archived from Repack's page, or asked back: archive.rs acts on it.
            let states: BTreeMap<String, Value> = rows
                .iter()
                .filter_map(|row| Some((row["scriptId"].as_str()?.to_string(), json!({ "archived": row["archived"], "restore": row["restore"] }))))
                .filter(|(_, state)| state["archived"].is_object() || state["restore"].is_object())
                .collect();
            crate::archive::keep_states(data, states);
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
            // An archived script stays as the cloud has it until it's restored.
            if !store::id_ok(&id) || busy(&id) || crate::archive::archived(data, &id) || backed_off(&id) {
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
            let mut files = local_files(&dir, &script);
            // Footage cleared from this Mac once the cloud held it
            // (storage.rs) is still the script's file in the cloud: a
            // later edit must not drop it from the script's files.
            if let Some(known) = &known {
                for name in crate::storage::purged_names(data, &id) {
                    if let Some(kept) = known.files.get(&name) {
                        files.entry(name).or_insert_with(|| kept.clone());
                    }
                }
            }
            if known.as_ref().is_some_and(|known| known.body == hash(&body) && same_files(&known.files, &files) && known.summary >= SUMMARY_VERSION) {
                continue;
            }
            changed.push((id, dir, script, body, known, files));
        }
        let mut left = changed.len();
        let mut refused: Option<Problem> = None;
        for (id, dir, script, body, known, files) in changed {
            set(|status| {
                status.state = "syncing".into();
                status.pending = left + waiting;
            });
            match self.push(&id, &dir, &script, &body, known, files) {
                Ok(Pushed::Done(known)) => {
                    forgive(&id);
                    ledger.scripts.insert(id, known);
                    save(data, &ledger);
                }
                Ok(Pushed::Overtaken) => {
                    waiting += 1;
                    if let Ok(mut now) = PULL_NOW.lock() {
                        *now = true;
                    }
                }
                Err(problem) if problem.offline => return Err(problem),
                // Refused: this one waits, longer each time, and the rest go on.
                Err(problem) => {
                    let wait = back_off(&id);
                    crate::log::warn("cloud", format!("{}: couldn't save to the cloud ({}); trying again in {} s", crate::log::named(&id, &script.title), problem.message, wait.as_secs()));
                    waiting += 1;
                    refused.get_or_insert(Problem::said(format!("{} couldn't be saved to the cloud: {}", script.title, problem.message)));
                }
            }
            left -= 1;
        }
        if pull {
            self.sync_lessons(data)?;
        }
        match refused {
            Some(problem) => Err(problem),
            None => Ok(waiting),
        }
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
        // Being deleted here: never brought back.
        if !store::id_ok(&id) || busy(&id) || deleting(data).contains(&id) {
            return Ok(());
        }
        let rev = row["rev"].as_u64().unwrap_or(0);
        let dir = store::scripts_root(data).join(&id);
        let known = ledger.scripts.get(&id).cloned();
        if !row["deletedAt"].is_null() {
            // Deleted on another Mac: gone here too. Changed here since, it's
            // kept as a copy under a new id (the deleted id can't be saved again).
            if dir.is_dir() {
                let name = crate::log::named(&id, row["title"].as_str().unwrap_or(""));
                if changed_here(&dir, known.as_ref()) {
                    keep_copy(data, &dir, "restored copy")?;
                    crate::log::info("cloud", format!("{name}: deleted on another Mac after it changed here, so this Mac's version is kept as a restored copy"));
                } else {
                    crate::log::info("cloud", format!("{name}: deleted on another Mac, so here too"));
                }
                let _ = fs::remove_dir_all(&dir);
                store::library_changed();
            }
            ledger.scripts.remove(&id);
            return Ok(());
        }
        if known.as_ref().is_some_and(|known| known.rev >= rev) {
            return Ok(());
        }
        let body = self.fetch(row["bodyKey"].as_str().unwrap_or(""))?;
        let at_start = fs::read(dir.join("script.json")).map(|bytes| hash(&bytes)).ok();
        if dir.is_dir() {
            let here = fs::read(dir.join("script.json")).unwrap_or_default();
            if hash(&here) == hash(&body) {
                // The same script, as on a first sync of a Mac that already has it.
            } else if changed_here(&dir, known.as_ref()) {
                keep_copy(data, &dir, "conflicted copy")?;
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
            // An archived script's footage was deleted from the cloud: only its small files come down.
            if row["archived"].is_object() && crate::archive::is_footage(name) {
                continue;
            }
            // Footage cleared from this Mac once verified here
            // (storage.rs) stays cleared: the same file at the same size
            // is not downloaded back. A changed one (a rebuilt preview
            // from another Mac) comes down as any changed file does.
            if crate::archive::is_footage(name) && crate::storage::is_purged(data, &id, name) && known.as_ref().and_then(|known| known.files.get(name)).is_some_and(|kept| kept.size == size) {
                files.insert(name.to_string(), Kept { key: key.to_string(), size, mtime: known.as_ref().and_then(|known| known.files.get(name)).map_or(0, |kept| kept.mtime) });
                continue;
            }
            if !present {
                self.download(&id, name, key, size, &path)?;
            }
            files.insert(name.to_string(), Kept { key: key.to_string(), size, mtime: mtime(&path) });
        }
        // Saved here while the files came down: that version is kept as a copy, not lost.
        if fs::read(dir.join("script.json")).map(|bytes| hash(&bytes)).ok() != at_start {
            crate::log::warn("cloud", format!("{}: changed here while rev {rev} came down; this Mac's version is kept as a copy", crate::log::named(&id, row["title"].as_str().unwrap_or(""))));
            keep_copy(data, &dir, "conflicted copy")?;
        }
        // Who started it, as the cloud recorded it from their verified sign-in, wins over the body's.
        let body = with_creator(body, &row["createdBy"]);
        let tmp = dir.join("script.json.tmp");
        fs::write(&tmp, &body).and_then(|_| fs::rename(&tmp, dir.join("script.json"))).map_err(|error| Problem::said(error.to_string()))?;
        crate::log::info("cloud", format!("{}: rev {rev} came from {}", crate::log::named(&id, row["title"].as_str().unwrap_or("")), row["updatedBy"].as_str().unwrap_or("another Mac")));
        // A row committed before the summary's newest field gets committed again, from here.
        let summary = if row["summary"]["kind"].is_string() { SUMMARY_VERSION } else { 0 };
        ledger.scripts.insert(id, Known { rev, body: hash(&body), files, summary });
        store::library_changed();
        Ok(())
    }

    /// Sends a script's changed files and its body, then commits them.
    fn push(&self, id: &str, dir: &Path, script: &Script, body: &[u8], known: Option<Known>, files: BTreeMap<String, Kept>) -> Done<Pushed> {
        let base = known.as_ref().map_or(0, |known| known.rev);
        let mut kept = BTreeMap::new();
        let began = std::time::Instant::now();
        let (mut sent, mut sent_bytes) = (0usize, 0u64);
        // Each push's files go under keys no committed revision uses, so a push
        // that loses to another Mac's never overwrites the winner's files.
        let push_id = &uuid::Uuid::new_v4().simple().to_string()[..10];
        let mut uploaded: Vec<String> = Vec::new();
        for (name, local) in files {
            let previous = known.as_ref().and_then(|known| known.files.get(&name));
            let key = format!("{PREFIX}/{id}/files/r{}-{push_id}/{name}", base + 1);
            if previous.is_some_and(|previous| previous.size == local.size && previous.mtime == local.mtime) {
                kept.insert(name, previous.cloned().unwrap_or_default());
                continue;
            }
            let Some(path) = inside(dir, &name) else { continue };
            // A file that vanished while it went up (a temp piece) is left out.
            match self.upload(id, &name, &path, &key) {
                Ok(size) => {
                    sent += 1;
                    sent_bytes += size;
                    if size > 50_000_000 {
                        crate::log::info("cloud", format!("{}: {name} uploaded: {}", crate::log::named(id, &script.title), crate::log::size(size)));
                    }
                    uploaded.push(key.clone());
                    kept.insert(name, Kept { key, size, mtime: local.mtime });
                }
                Err(problem) if problem.offline => return Err(problem),
                Err(_) if !path.exists() => continue,
                Err(problem) => {
                    self.remove(&uploaded);
                    return Err(problem);
                }
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
                let rev = value["rev"].as_u64().unwrap_or(base + 1);
                let files = if sent > 0 { format!(" with {sent} {} ({})", if sent == 1 { "file" } else { "files" }, crate::log::size(sent_bytes)) } else { String::new() };
                crate::log::info("cloud", format!("{}: saved to the cloud as rev {rev}{files} in {}", crate::log::named(id, &script.title), crate::log::since_start(began)));
                Ok(Pushed::Done(Known { rev, body: digest, files: kept, summary: SUMMARY_VERSION }))
            }
            Err((code, _)) if code == "conflict" => {
                crate::log::warn("cloud", format!("{}: another Mac saved it first (rev {base} is behind); taking theirs and keeping this Mac's as a copy", crate::log::named(id, &script.title)));
                uploaded.push(body_key);
                self.remove(&uploaded);
                Ok(Pushed::Overtaken)
            }
            Err((code, data)) => {
                uploaded.push(body_key);
                self.remove(&uploaded);
                Err(Problem::said(format!(
                "The cloud refused {}: {}",
                script.title,
                data["message"].as_str().unwrap_or(&code)
            )))
            }
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
/// folder. A joined case's recordings, and any original in Edit's originals
/// (edit-shared.json), are left out: they are shared under edit/originals/,
/// and the preview is what another Mac plays.
fn local_files(dir: &Path, script: &Script) -> BTreeMap<String, Kept> {
    let original = Path::new(&script.original_path);
    // In Edit's originals now: found there, not kept twice.
    let shared = crate::handoff::shared_files(dir);
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
            // A commentary's clip footage is each Mac's own (footage.rs): it comes from YouTube.
            if relative.starts_with("footage/") {
                continue;
            }
            let recording = (relative.starts_with("drive/") || relative.starts_with("compilation/")) && !relative.ends_with(".json");
            if recording && path != original {
                continue;
            }
            if shared.contains(&relative) {
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

/// A pulled body with the cloud's `createdBy` in it, when the cloud has one
/// and the body says otherwise; the body as it was if not.
fn with_creator(body: Vec<u8>, created_by: &Value) -> Vec<u8> {
    let Ok(creator) = serde_json::from_value::<crate::model::Creator>(created_by.clone()) else { return body };
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else { return body };
    if serde_json::from_value::<crate::model::Creator>(value["createdBy"].clone()).ok().as_ref() == Some(&creator) {
        return body;
    }
    value["createdBy"] = serde_json::to_value(&creator).unwrap_or(Value::Null);
    serde_json::to_vec_pretty(&value).unwrap_or(body)
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
fn keep_copy(data: &Path, dir: &Path, label: &str) -> Done<()> {
    let id = uuid::Uuid::new_v4().to_string();
    let copy = store::scripts_root(data).join(&id);
    copy_dir(dir, &copy).map_err(|error| Problem::said(error.to_string()))?;
    let mut script = store::read(&copy).map_err(Problem::said)?;
    script.id = id;
    script.title = format!("{} ({label})", script.title);
    script.updated_at = now_ms();
    store::write(&copy, &script).map_err(Problem::said)?;
    store::library_changed();
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clouds_creator_wins() {
        let body = br#"{"id":"a","createdBy":{"sub":"me","name":"Me","email":"me@x"}}"#.to_vec();
        let cloud = serde_json::json!({ "sub": "p1", "name": "Someone", "email": "s@x", "picture": "https://lh3.googleusercontent.com/a" });
        let out: Value = serde_json::from_slice(&with_creator(body.clone(), &cloud)).unwrap();
        assert_eq!(out["createdBy"]["sub"], "p1");
        assert_eq!(out["id"], "a");
        // As the cloud's list shows it (Repack's shownCreator): no email, picture null.
        let shown = serde_json::json!({ "sub": "p2", "name": "Two", "picture": null });
        let out: Value = serde_json::from_slice(&with_creator(body.clone(), &shown)).unwrap();
        assert_eq!(out["createdBy"]["sub"], "p2");
        // No creator in the cloud (an older script): the body as it was.
        assert_eq!(with_creator(body.clone(), &Value::Null), body);
    }
}
