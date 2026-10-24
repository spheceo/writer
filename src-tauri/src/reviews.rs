//! Sharing a script for review on a Repack link, and the notes reviewers
//! leave there. Every call to Repack's `reviews:*` functions is in this
//! module (repack/apps/web/convex/reviews.ts); the relay adds
//! `app: "writer"` to each.
//!
//! Share snapshots script.json to R2 and publishes its blocks (blocks.rs);
//! sharing again publishes the next version under the same link. While
//! Writer runs it asks for changed notes every 45 seconds and keeps them in
//! `reviews/notes.json` beside `scripts/` (each Mac its own copy, so two
//! Macs never conflict over it). Two minutes after a script's last new note,
//! its open notes are fixed by Claude (fixing.rs), unless that's turned off.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::blocks::{self, Range};
use crate::model::now_ms;
use crate::{cloud, import, jobs, settings, store};

const POLL: Duration = Duration::from_secs(45);
/// A sitting's notes are fixed together: this long after the last one.
pub const QUIET_MS: i64 = 2 * 60_000;
/// Runs a note gets by itself; after that, only the producer fixes it, by hand.
const MAX_ATTEMPTS: u32 = 2;

/// A script's review link, kept in its script.json so every Mac has it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub review_id: String,
    pub token: String,
    pub url: String,
    /// The latest version published.
    pub version: u32,
    /// The script's revision that version is.
    pub revision: u32,
    pub shared_at: i64,
}

/// A reviewer's note, or a reply to one, as Writer keeps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub review_id: String,
    /// The script it is on, once known.
    #[serde(default)]
    pub script_id: Option<String>,
    pub version: u32,
    #[serde(default)]
    pub parent_id: Option<String>,
    /// `{kind: "text", blockId, start, end, quote}`, as the page sends it.
    pub anchor: Value,
    pub text: String,
    pub author: String,
    /// `open` or `done`.
    pub status: String,
    #[serde(default)]
    pub done_in_version: Option<u32>,
    #[serde(default)]
    pub changed_ranges: Vec<Range>,
    #[serde(default)]
    pub fix_summary: Option<String>,
    /// Someone (a Mac's run) has taken it to fix.
    #[serde(default)]
    pub attempted: bool,
    /// Written by an app (Writer's own reply), not a reviewer.
    #[serde(default)]
    pub from_app: bool,
    /// Its guest deleted it (and so its replies): dropped when it comes in.
    #[serde(default, skip_serializing)]
    pub deleted: bool,
    /// What the reviewer means it to change (spec v2): wording, fact, cut,
    /// pacing, question, fyi, editor. It limits what a fix may touch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    /// `here` or `everywhere`: the ask across the whole script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// The exact words to put in place of the anchored ones, applied as written
    /// (the composer's "suggest words", before suggestions had their own kind).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// A suggestion (intent `suggestion`): the anchored words' replacement,
    /// "" to delete them; with `start == end` and no quote, words to insert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    /// An insertion's place, `{before, after}`: the words either side of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    /// One edit spanning blocks: its notes share this, and go together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// A suggestion's end: `accepted` or `rejected`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// On a footage clip: `cut`, `extend` or `restore` (with the anchor's `side`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// A reviewer confirmed the fix.
    #[serde(default)]
    pub confirmed: bool,
    /// When a reviewer last sent the fix back as not fixed (the note reopened): a retry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_fixed_at: Option<i64>,
    /// When a run last took it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Note {
    /// A reviewer's note, not a reply.
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    /// A suggested edit: words to put in, take out or replace, as written.
    pub fn is_suggestion(&self) -> bool {
        self.intent.as_deref() == Some("suggestion") || self.replacement.is_some()
    }

    pub fn is_open(&self) -> bool {
        self.status != "done"
    }

    /// Open and nobody's to fix: never taken, or sent back as not fixed since it was.
    pub fn untaken(&self) -> bool {
        self.is_open() && (!self.attempted || self.not_fixed_at.is_some_and(|at| self.attempt_at.is_none_or(|taken| at > taken)))
    }

    fn from(value: &Value) -> Option<Note> {
        let text = |key: &str| value[key].as_str().map(str::to_string);
        let number = |key: &str| value[key].as_f64().map(|n| n as i64);
        Some(Note {
            id: text("_id").or_else(|| text("id")).or_else(|| text("noteId"))?,
            review_id: text("reviewId")?,
            script_id: text("sourceId"),
            version: number("version").unwrap_or(1).max(1) as u32,
            parent_id: text("parentId"),
            anchor: value["anchor"].clone(),
            text: text("text").unwrap_or_default(),
            author: text("author").unwrap_or_else(|| "Reviewer".into()),
            status: text("status").unwrap_or_else(|| "open".into()),
            done_in_version: number("doneInVersion").map(|n| n as u32),
            changed_ranges: serde_json::from_value(value["changedRanges"].clone()).unwrap_or_default(),
            fix_summary: text("fixSummary"),
            attempted: value["attempt"].is_object(),
            from_app: value["fromApp"] == true,
            deleted: value["deleted"] == true,
            intent: text("intent").filter(|intent| !intent.trim().is_empty()),
            scope: text("scope"),
            suggestion: text("suggestion"),
            replacement: text("replacement"),
            context: value["context"].is_object().then(|| value["context"].clone()),
            group: text("group").filter(|group| !group.is_empty()),
            outcome: text("outcome"),
            action: text("action").filter(|action| !action.trim().is_empty()),
            confirmed: value["confirmed"].is_object(),
            not_fixed_at: value["notFixed"]["at"].as_f64().map(|at| at as i64),
            attempt_at: value["attempt"]["at"].as_f64().map(|at| at as i64),
            created_at: number("createdAt").unwrap_or(0),
            updated_at: number("updatedAt").or_else(|| number("createdAt")).unwrap_or(0),
        })
    }
}

// ---- The contract: Repack's reviews:* functions ------------------------------

pub struct Published {
    pub review_id: String,
    pub token: String,
    pub url: String,
    pub version: u32,
}

fn publish(data: &Path, script_id: &str, title: &str, key: &str, meta: Value, revision: u32) -> Result<Published, String> {
    let value = cloud::call(
        data,
        "mutation",
        "reviews:publish",
        json!({ "sourceId": script_id, "title": title, "kind": "script", "key": key, "meta": meta, "sourceRev": revision, "by": by() }),
    )?;
    let text = |key: &str| value[key].as_str().map(str::to_string);
    Ok(Published {
        review_id: text("reviewId").ok_or("Repack published the review but sent no ID.")?,
        token: text("token").unwrap_or_default(),
        url: text("url").ok_or("Repack published the review but sent no link.")?,
        version: value["version"].as_f64().map(|n| n as u32).unwrap_or(1),
    })
}

/// Who shares, claims and replies, as Repack shows it.
fn by() -> String {
    format!("Writer on {}", cloud::holder_name())
}

/// Every note and reply written or changed after `cursor` (Repack numbers
/// each write), and the cursor to ask from next.
fn changes(data: &Path, cursor: i64) -> Result<(Vec<Note>, i64), String> {
    let mut notes = Vec::new();
    let mut cursor = cursor;
    // A page at a time, while Repack says there's more.
    for _ in 0..50 {
        let value = cloud::call(data, "query", "reviews:changes", json!({ "cursor": cursor }))?;
        notes.extend(value["notes"].as_array().into_iter().flatten().filter_map(Note::from));
        cursor = value["cursor"].as_f64().map_or(cursor, |next| next as i64);
        if value["more"] != true {
            break;
        }
    }
    Ok((notes, cursor))
}

/// Takes notes to fix; answers the IDs this Mac got (not replies, done, or under another run's claim).
pub fn claim(data: &Path, note_ids: &[String]) -> Result<Vec<String>, String> {
    // Repack takes a hundred at a time.
    let value = cloud::call(data, "mutation", "reviews:claim", json!({ "noteIds": &note_ids[..note_ids.len().min(100)], "by": by() }))?;
    Ok(value["claimed"].as_array().into_iter().flatten().filter_map(|id| id.as_str().map(str::to_string)).collect())
}

/// Lets go of notes a run couldn't get to, so a later run may take them.
pub fn release(data: &Path, note_ids: &[String]) -> Result<(), String> {
    cloud::call(data, "mutation", "reviews:release", json!({ "noteIds": note_ids, "by": by() }))?;
    Ok(())
}

/// Every review's approval, by script.
fn approvals(data: &Path) -> Result<BTreeMap<String, Approval>, String> {
    let rows = cloud::call(data, "query", "reviews:open", json!({}))?;
    Ok(rows
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| Some((row["sourceId"].as_str()?.to_string(), Approval::from(&row["approved"])?)))
        .collect())
}

/// Approves the script's latest shared version: the link takes no more
/// notes and Repack deletes the other versions' files; every note stays.
pub fn approve(data: &Path, script_id: &str) -> Result<Approval, String> {
    let script = store::read(&store::script_dir(data, script_id)?)?;
    let link = script.review.ok_or("Share the script before approving it.")?;
    let value = cloud::call(data, "mutation", "reviews:approve", json!({ "sourceId": script_id, "version": link.version, "by": by() }))?;
    let approval = Approval::from(&value["approved"]).ok_or("Repack approved it but didn't say which version.")?;
    crate::log::info("review", format!("{}: v{} approved", crate::log::named(script_id, &script.title), approval.version));
    change(data, |known| {
        known.approved.insert(script_id.to_string(), approval.clone());
    });
    Ok(approval)
}

/// Takes an approval back: the link takes notes again (deleted files stay deleted).
pub fn unapprove(data: &Path, script_id: &str) -> Result<(), String> {
    cloud::call(data, "mutation", "reviews:unapprove", json!({ "sourceId": script_id }))?;
    change(data, |known| {
        known.approved.remove(script_id);
    });
    Ok(())
}

/// The script's approval, if its review has one.
pub fn approval(data: &Path, script_id: &str) -> Option<Approval> {
    load(data).approved.get(script_id).cloned()
}

pub fn resolve(data: &Path, note_id: &str, version: u32, ranges: &[Range], summary: &str) -> Result<(), String> {
    cloud::call(data, "mutation", "reviews:resolve", json!({ "noteId": note_id, "version": version, "changedRanges": ranges, "fixSummary": summary }))?;
    Ok(())
}

/// A suggestion resolved, `accepted` (applied) or `rejected` (the summary says why).
pub fn resolve_suggestion(data: &Path, note_id: &str, version: u32, ranges: &[Range], summary: &str, accepted: bool) -> Result<(), String> {
    let outcome = if accepted { "accepted" } else { "rejected" };
    cloud::call(data, "mutation", "reviews:resolve", json!({ "noteId": note_id, "version": version, "changedRanges": ranges, "fixSummary": summary, "outcome": outcome }))?;
    Ok(())
}

pub fn reply(data: &Path, note_id: &str, text: &str) -> Result<(), String> {
    cloud::call(data, "mutation", "reviews:reply", json!({ "noteId": note_id, "text": text, "author": "Writer" }))?;
    Ok(())
}

// ---- Sharing -------------------------------------------------------------------

/// Publishes the script as it stands: its first version, or the next under
/// the same link. Answers the script with its link.
pub fn share(data: &Path, id: &str) -> Result<crate::model::ScriptDetail, String> {
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    if script.status != "ready" {
        return Err("Share a script once its import has finished.".into());
    }
    if jobs::running(id) {
        return Err("Claude is still working on this script. Share it when that's done.".into());
    }
    // A commentary's paragraphs get the IDs notes anchor to.
    if let Some(draft) = script.commentary.as_mut().and_then(|commentary| commentary.draft.as_mut()) {
        if draft.ensure_ids() {
            store::write(&dir, &script)?;
        }
    } else if script.commentary.is_some() {
        return Err("Write the script before sharing it.".into());
    }
    let found = blocks::of(&script);
    if found.is_empty() {
        return Err("There's nothing in the script to share yet.".into());
    }
    // The review page plays the preview from the cloud, so sharing waits
    // until that preview is verified there (an r2/head at its ledger key,
    // cloud.rs): the ledger alone once let a share publish a preview key
    // for an object that wasn't there. A preview already cleared from
    // this Mac is fine, exactly because it is verified.
    if script.commentary.is_none() {
        match cloud::file_key(data, id, "proxy.mp4") {
            Some((key, size)) => match cloud::head(data, &key) {
                Ok(Some(found)) if found == size => {}
                Ok(_) => return Err("Its preview hasn't reached the cloud yet. Wait for the sync to finish, then share again.".into()),
                Err(error) => return Err(format!("Couldn't verify its preview in the cloud ({error}). Check the connection, then share again.")),
            },
            None if store::proxy_path(&dir).is_file() => {
                return Err("Its preview hasn't reached the cloud yet. Wait for the sync to finish, then share again.".into())
            }
            None => {}
        }
    }
    let next = script.review.as_ref().map_or(1, |link| link.version + 1);
    let body = serde_json::to_vec(&script).map_err(|error| error.to_string())?;
    // The review's ID is Repack's to give, on the first publish: snapshots
    // are kept by the script's ID instead.
    let key = format!("writer/reviews/{id}/v{next}/script.json");
    cloud::put_bytes(data, &key, &body, "application/json")?;
    let found_count = found.len();
    let meta = meta_of(data, id, &script, found)?;
    let size = serde_json::to_vec(&meta).map(|bytes| bytes.len()).unwrap_or(0);
    let published = publish(data, id, &script.title, &key, meta, script.revision).inspect_err(|error| crate::log::error("review", format!("{}: couldn't share: {error}", crate::log::named(id, &script.title))))?;
    crate::log::info("review", format!("{}: shared as v{} ({} blocks, {} of page data) at {}", crate::log::named(id, &script.title), published.version, found_count, crate::log::size(size as u64), published.url));
    script.review = Some(Link {
        review_id: published.review_id.clone(),
        token: published.token,
        url: published.url,
        version: published.version,
        revision: script.revision,
        shared_at: now_ms(),
    });
    store::write(&dir, &script)?;
    remember_link(data, &published.review_id, id);
    // A commentary's review page plays its clips from stretches put up beside it (footage.rs).
    if script.commentary.is_some() {
        crate::footage::soon(data.to_path_buf(), id.to_string());
    }
    // A version shared after an approval is the next round, unapproved.
    change(data, |known| {
        known.approved.remove(id);
    });
    poll_soon();
    Ok(store::detail(script, &dir))
}

/// The most a version's meta may be (Repack's cap is 512 KB).
const META_BYTES: usize = 480_000;

/// What the review page shows and plays: the blocks, the outline rail with
/// running times against the format's target, and the preview the clips
/// play from. Word timings that would take the meta past Repack's cap go in
/// a file of their own (`wordsKey`).
fn meta_of(data: &Path, id: &str, script: &crate::model::Script, mut found: Vec<blocks::Block>) -> Result<Value, String> {
    let footage = script.commentary.is_none();
    let mut meta = json!({
        "format": if footage { "footage" } else { "commentary" },
        "revision": script.revision,
        "outline": blocks::outline(script, &found),
    });
    if let Some((low, aim, high)) = blocks::target_ms(script) {
        meta["targetMs"] = json!(aim);
        meta["targetRangeMs"] = json!([low, high]);
    }
    if footage {
        // The proxy the script syncs: the clips' times are on its clock.
        if let Some((key, size)) = cloud::file_key(data, id, "proxy.mp4") {
            meta["preview"] = json!({ "key": key, "durationMs": script.duration_ms, "sizeBytes": size });
        }
    }
    meta["blocks"] = json!(found);
    if serde_json::to_vec(&meta).map(|bytes| bytes.len()).unwrap_or(0) > META_BYTES {
        let words: serde_json::Map<String, Value> = found.iter_mut().filter(|block| !block.words.is_empty()).map(|block| (block.id.clone(), json!(std::mem::take(&mut block.words)))).collect();
        let key = format!("writer/reviews/{id}/{}/words.json", uuid::Uuid::new_v4());
        cloud::put_bytes(data, &key, &serde_json::to_vec(&words).map_err(|error| error.to_string())?, "application/json")?;
        meta["blocks"] = json!(found);
        meta["wordsKey"] = json!(key);
        if serde_json::to_vec(&meta).map(|bytes| bytes.len()).unwrap_or(0) > META_BYTES {
            return Err("This script is too long to share in one review page.".into());
        }
    }
    Ok(meta)
}

// ---- What this Mac knows ----------------------------------------------------------

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Known {
    /// Where `reviews:changes` carries on from.
    #[serde(default)]
    cursor: i64,
    notes: BTreeMap<String, Note>,
    /// Review ID → script ID.
    links: BTreeMap<String, String>,
    /// Notes this Mac has started fixing, so a run is never repeated.
    #[serde(default)]
    tried: BTreeMap<String, i64>,
    /// How many runs each note has had here: auto-fix stops at MAX_ATTEMPTS,
    /// so a note reopened again and again can't keep Claude busy.
    #[serde(default)]
    attempts: BTreeMap<String, u32>,
    /// Script ID → its approved version, as Repack's reviews:open says.
    #[serde(default)]
    approved: BTreeMap<String, Approval>,
}

/// A review's approved version: when it's approved, the link takes no more
/// notes, nothing is fixed by itself, and Repack deletes the other versions'
/// files (their notes stay).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub version: u32,
    pub by: String,
    pub at: i64,
}

impl Approval {
    fn from(value: &Value) -> Option<Approval> {
        Some(Approval { version: value["version"].as_f64()? as u32, by: value["by"].as_str().unwrap_or("").to_string(), at: value["at"].as_f64().unwrap_or(0.0) as i64 })
    }
}

static KNOWN: Mutex<()> = Mutex::new(());

fn known_path(data: &Path) -> PathBuf {
    data.join("reviews").join("notes.json")
}

fn load(data: &Path) -> Known {
    fs::read(known_path(data)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save(data: &Path, known: &Known) {
    let path = known_path(data);
    let _ = fs::create_dir_all(path.parent().unwrap_or(data));
    let tmp = path.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(known) {
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}

fn change(data: &Path, edit: impl FnOnce(&mut Known)) {
    let _guard = KNOWN.lock();
    let mut known = load(data);
    edit(&mut known);
    save(data, &known);
}

fn remember_link(data: &Path, review_id: &str, script_id: &str) {
    change(data, |known| {
        known.links.insert(review_id.to_string(), script_id.to_string());
    });
}

/// Every script's link, read from the scripts themselves: for a review
/// shared on another Mac.
fn scan_links(data: &Path) -> BTreeMap<String, String> {
    let mut links = BTreeMap::new();
    for entry in fs::read_dir(store::scripts_root(data)).into_iter().flatten().flatten() {
        let Ok(bytes) = fs::read(entry.path().join("script.json")) else { continue };
        // Only the two fields needed, not the whole script.
        #[derive(Deserialize)]
        struct Slim {
            id: String,
            #[serde(default)]
            review: Option<Link>,
        }
        if let Ok(slim) = serde_json::from_slice::<Slim>(&bytes) {
            if let Some(link) = slim.review {
                links.insert(link.review_id, slim.id);
            }
        }
    }
    links
}

/// What the app shows: open notes per script, for the cards' badges.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub open: BTreeMap<String, usize>,
    pub approved: BTreeMap<String, Approval>,
    pub auto_fix: bool,
}

pub fn counts(data: &Path) -> Counts {
    let known = load(data);
    let mut open = BTreeMap::new();
    for note in known.notes.values().filter(|note| note.is_root() && note.is_open()) {
        if let Some(script) = note.script_id.as_ref().or_else(|| known.links.get(&note.review_id)) {
            *open.entry(script.clone()).or_insert(0) += 1;
        }
    }
    Counts { open, approved: known.approved, auto_fix: settings::load(data).auto_fix }
}

/// A script's notes, oldest first, with their replies.
pub fn notes_of(data: &Path, script_id: &str) -> Vec<Note> {
    let known = load(data);
    let mut notes: Vec<Note> = known
        .notes
        .values()
        .filter(|note| note.script_id.as_deref().or_else(|| known.links.get(&note.review_id).map(String::as_str)) == Some(script_id))
        .cloned()
        .collect();
    notes.sort_by_key(|note| note.created_at);
    notes
}

/// A note fixed here: done in the next version, with what changed.
pub fn mark_fixed(data: &Path, note_id: &str, version: u32, ranges: Vec<Range>, summary: &str) {
    change(data, |known| {
        if let Some(note) = known.notes.get_mut(note_id) {
            note.status = "done".into();
            note.done_in_version = Some(version);
            note.changed_ranges = ranges;
            note.fix_summary = Some(summary.to_string());
            note.updated_at = now_ms();
        }
    });
}

pub fn mark_tried(data: &Path, note_ids: &[String]) {
    change(data, |known| {
        for id in note_ids {
            known.tried.insert(id.clone(), now_ms());
            *known.attempts.entry(id.clone()).or_default() += 1;
        }
    });
}

// ---- Asking for changes, and fixing --------------------------------------------------

static POLL_NOW: Mutex<bool> = Mutex::new(false);

pub fn poll_soon() {
    if let Ok(mut now) = POLL_NOW.lock() {
        *now = true;
    }
}

/// Asks for changed notes while Writer runs, and starts fixes when a sitting is over.
pub fn start(app: AppHandle, data: PathBuf) {
    std::thread::spawn(move || {
        let mut waited = POLL;
        loop {
            let now = POLL_NOW.lock().map(|mut now| std::mem::take(&mut *now)).unwrap_or(false);
            if now || waited >= POLL {
                waited = Duration::ZERO;
                if let Ok(true) = poll(&data) {
                    let _ = app.emit("reviews", ());
                }
                start_fixes(&app, &data);
            }
            std::thread::sleep(Duration::from_secs(3));
            waited += Duration::from_secs(3);
        }
    });
}

/// One round of changes; says whether anything changed.
fn poll(data: &Path) -> Result<bool, String> {
    if !crate::account::can_call() {
        return Ok(false);
    }
    // Approvals, made here or on Repack's page: only reviews:open carries them.
    let approved = approvals(data)?;
    let mut moved = false;
    change(data, |known| {
        moved = known.approved != approved;
        known.approved = approved;
    });
    let cursor = load(data).cursor;
    let (notes, next) = changes(data, cursor)?;
    if notes.is_empty() && next == cursor {
        return Ok(moved);
    }
    let (fresh, replies) = (notes.iter().filter(|note| note.is_root() && !note.deleted).count(), notes.iter().filter(|note| !note.is_root()).count());
    crate::log::info("review", format!("{} notes changed on review links ({fresh} notes, {replies} replies, {} deleted)", notes.len(), notes.iter().filter(|note| note.deleted).count()));
    let unknown = {
        let known = load(data);
        notes.iter().any(|note| note.script_id.is_none() && !known.links.contains_key(&note.review_id))
    };
    let scanned = if unknown { scan_links(data) } else { BTreeMap::new() };
    let mut changed = moved;
    change(data, |known| {
        known.links.extend(scanned);
        for mut note in notes {
            // Deleted by its guest: gone, with its replies, never a phantom open note.
            if note.deleted {
                let id = note.id.clone();
                let before = known.notes.len();
                known.notes.retain(|kept_id, kept| kept_id != &id && kept.parent_id.as_deref() != Some(id.as_str()));
                known.tried.remove(&id);
                changed |= known.notes.len() != before;
                continue;
            }
            if note.script_id.is_none() {
                note.script_id = known.links.get(&note.review_id).cloned();
            }
            let same = known.notes.get(&note.id).is_some_and(|kept| kept.updated_at == note.updated_at && kept.status == note.status);
            // Sent back as not fixed (or opened again): a new note to fix.
            let reopened = known.notes.get(&note.id).is_some_and(|kept| (!kept.is_open() && note.is_open()) || kept.not_fixed_at != note.not_fixed_at);
            if reopened && note.untaken() {
                known.tried.remove(&note.id);
            }
            if !same {
                known.notes.insert(note.id.clone(), note);
                changed = true;
            }
        }
        known.cursor = next;
    });
    Ok(changed)
}

/// Starts a fix on each script whose reviewer has been quiet for two
/// minutes, with open notes nobody has taken: one run per script, never
/// during a build or an import, and not at all when turned off.
fn start_fixes(app: &AppHandle, data: &Path) {
    if !settings::load(data).auto_fix {
        return;
    }
    let grants = crate::can::grants();
    if !grants.has(crate::can::SHARE) {
        return;
    }
    let known = load(data);
    let mut by_script: BTreeMap<String, Vec<&Note>> = BTreeMap::new();
    for note in known.notes.values() {
        let spent = known.attempts.get(&note.id).copied().unwrap_or(0) >= MAX_ATTEMPTS;
        if note.is_root() && note.untaken() && !known.tried.contains_key(&note.id) && !spent {
            // An approved script is finished: nothing is fixed by itself.
            if let Some(script) = note.script_id.as_ref().or_else(|| known.links.get(&note.review_id)).filter(|script| !known.approved.contains_key(*script)) {
                by_script.entry(script.clone()).or_default().push(note);
            }
        }
    }
    for (script, notes) in by_script {
        let last = notes.iter().map(|note| note.created_at).max().unwrap_or(0);
        if now_ms() - last < QUIET_MS || jobs::running(&script) || import::importing(&script) {
            continue;
        }
        let ready = store::script_dir(data, &script).and_then(|dir| store::read(&dir)).is_ok_and(|found| found.status == "ready" && crate::can::sees(&grants, &found));
        if !ready {
            continue;
        }
        let ids: Vec<String> = notes.iter().map(|note| note.id.clone()).collect();
        crate::log::info("review", format!("script {}: {} new {} quiet for two minutes; fixing them", &script[..8.min(script.len())], ids.len(), if ids.len() == 1 { "note" } else { "notes" }));
        mark_tried(data, &ids);
        let _ = jobs::start(app.clone(), script, jobs::Work::Review { notes: ids });
    }
}

/// A script's open notes, for fixing them by hand: one tried before is
/// tried again if its claim has lapsed or was let go (Repack decides).
pub fn open_notes(data: &Path, script_id: &str) -> Vec<String> {
    notes_of(data, script_id).into_iter().filter(|note| note.is_root() && note.is_open()).map(|note| note.id).collect()
}

pub fn note(data: &Path, id: &str) -> Option<Note> {
    load(data).notes.get(id).cloned()
}

/// The replies under a note, oldest first: the reviewer's and Writer's own.
pub fn replies(data: &Path, id: &str) -> Vec<Note> {
    let mut found: Vec<Note> = load(data).notes.values().filter(|note| note.parent_id.as_deref() == Some(id)).cloned().collect();
    found.sort_by_key(|note| note.created_at);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_notes_and_approvals_as_repack_sends_them() {
        let note = Note::from(&json!({
            "noteId": "n1", "reviewId": "r1", "sourceId": "s1", "version": 2.0, "parentId": null,
            "anchor": { "kind": "text", "blockId": "b1", "start": 0, "end": 4, "quote": "Four" },
            "text": "Cut", "author": "Ana", "fromApp": false, "status": "open", "attempt": { "by": "x", "at": 1.0 },
            "deleted": true, "createdAt": 5.0, "updatedAt": 6.0
        }))
        .unwrap();
        assert_eq!((note.id.as_str(), note.version, note.attempted, note.deleted, note.is_open()), ("n1", 2, true, true, true));
        // Sent back as not fixed after the run took it: fixable again.
        let retry = Note::from(&json!({
            "noteId": "n2", "reviewId": "r1", "version": 1.0, "text": "Still wrong", "author": "Ana", "status": "open",
            "intent": "wording", "scope": "here", "suggestion": null, "action": false, "general": null,
            "attempt": { "by": "Writer", "at": 10.0 }, "notFixed": { "by": "Ana", "at": 20.0, "version": 2.0 }, "confirmed": null,
            "createdAt": 1.0, "updatedAt": 20.0
        }))
        .unwrap();
        assert!(retry.untaken());
        assert_eq!((retry.intent.as_deref(), retry.action.as_deref(), retry.suggestion.as_deref(), retry.confirmed), (Some("wording"), None, None, false));
        assert!(!Note { not_fixed_at: Some(5), ..retry.clone() }.untaken());
        assert_eq!(Approval::from(&json!({ "version": 3.0, "by": "Writer on Mac", "at": 7.0 })), Some(Approval { version: 3, by: "Writer on Mac".into(), at: 7 }));
        assert_eq!(Approval::from(&Value::Null), None);
    }
}
