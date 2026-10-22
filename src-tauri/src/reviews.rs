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
    pub created_at: i64,
    pub updated_at: i64,
}

impl Note {
    /// A reviewer's note, not a reply.
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none()
    }

    pub fn is_open(&self) -> bool {
        self.status != "done"
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
    let next = script.review.as_ref().map_or(1, |link| link.version + 1);
    let body = serde_json::to_vec(&script).map_err(|error| error.to_string())?;
    // The review's ID is Repack's to give, on the first publish: snapshots
    // are kept by the script's ID instead.
    let key = format!("writer/reviews/{id}/v{next}/script.json");
    cloud::put_bytes(data, &key, &body, "application/json")?;
    let meta = json!({
        "format": if script.commentary.is_some() { "commentary" } else { "footage" },
        "revision": script.revision,
        "blocks": found,
    });
    let published = publish(data, id, &script.title, &key, meta, script.revision)?;
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
    // A version shared after an approval is the next round, unapproved.
    change(data, |known| {
        known.approved.remove(id);
    });
    poll_soon();
    Ok(store::detail(script, &dir))
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
    if settings::load(data).relay_token.is_empty() {
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
            // Opened again by the reviewer ("Not fixed"): a new note to fix.
            if known.notes.get(&note.id).is_some_and(|kept| !kept.is_open()) && note.is_open() && !note.attempted {
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
    let known = load(data);
    let mut by_script: BTreeMap<String, Vec<&Note>> = BTreeMap::new();
    for note in known.notes.values() {
        if note.is_root() && note.is_open() && !note.attempted && !known.tried.contains_key(&note.id) {
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
        let ready = store::script_dir(data, &script).and_then(|dir| store::read(&dir)).is_ok_and(|found| found.status == "ready");
        if !ready {
            continue;
        }
        let ids: Vec<String> = notes.iter().map(|note| note.id.clone()).collect();
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
        assert_eq!(Approval::from(&json!({ "version": 3.0, "by": "Writer on Mac", "at": 7.0 })), Some(Approval { version: 3, by: "Writer on Mac".into(), at: 7 }));
        assert_eq!(Approval::from(&Value::Null), None);
    }
}
