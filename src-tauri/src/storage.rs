//! Local storage retention: Writer's footage lives in the cloud, not on
//! this Mac forever.
//!
//! Every import keeps its footage beside the script (`original.mp4`, the
//! 480p `proxy.mp4`, a compilation's `compilation/` videos, a Drive case's
//! `drive/` recordings, heard audio). Those files are also in the cloud:
//! the proxy and a single original sync to R2 under `writer/scripts/<id>/`
//! (cloud.rs), and each recording goes to Edit's originals (handoff.rs).
//! Once both uploads are confirmed for a script, the local copies are a
//! second copy of footage the cloud already holds, so they can go. The
//! script, its transcript, peaks, watcher answers, versions, chat and
//! every other small file stay, so the script still lists, reads and
//! searches; only playing its preview needs the preview back, which
//! `fetch_preview` downloads from the cloud on request.
//!
//! Two rules keep this safe:
//! - Media is only ever deleted for a script whose upload is confirmed
//!   (the cloud ledger holds that file at that size, or Edit's registry
//!   does via `edit-shared.json`). A script still importing, building, or
//!   waiting to upload is reported as blocked with the reason, never
//!   silently cleared.
//! - Only files inside the script's own folder are touched. A file
//!   imported from this Mac's disk lives outside it and is the producer's
//!   own, exactly as archive.rs treats it.
//!
//! What was purged is remembered in `cloud/storage.json`, so cloud.rs
//! keeps listing the footage in the cloud (a later edit doesn't drop it
//! from the script's files) and doesn't download it straight back.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::{archive, cloud, handoff, import, jobs, store};

/// One script's local storage.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptStorage {
    pub id: String,
    pub title: String,
    /// Everything in the script's folder.
    pub total_bytes: u64,
    /// Footage in it (video and sound, as archive.rs counts it).
    pub media_bytes: u64,
    /// Media that may be deleted now (0 when blocked).
    pub reclaimable_bytes: u64,
    /// Why the media may not be deleted yet, when it may not.
    pub blocked_reason: Option<String>,
}

/// The whole library's local storage, for Setup's readout.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub total_bytes: u64,
    pub media_bytes: u64,
    pub reclaimable_bytes: u64,
    /// Media that stays until its upload is confirmed.
    pub blocked_bytes: u64,
    pub scripts: Vec<ScriptStorage>,
}

/// What a clear did, per script.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleared {
    pub freed_bytes: u64,
    pub cleared: Vec<String>,
    pub blocked: Vec<ScriptStorage>,
}

// ---- What was purged, so the cloud leaves it there ---------------------------

fn purged_path(data: &Path) -> PathBuf {
    data.join("cloud").join("storage.json")
}

fn purged_map(data: &Path) -> BTreeMap<String, Vec<String>> {
    fs::read(purged_path(data)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn save_purged(data: &Path, map: &BTreeMap<String, Vec<String>>) {
    let path = purged_path(data);
    let _ = fs::create_dir_all(path.parent().unwrap_or(data));
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, serde_json::to_vec_pretty(map).unwrap_or_default()).is_ok() {
        let _ = fs::rename(tmp, path);
    }
}

/// The footage files of a script already purged from this Mac (cloud.rs
/// keeps them in the script's cloud files and doesn't pull them back).
pub fn purged_names(data: &Path, id: &str) -> Vec<String> {
    purged_map(data).get(id).cloned().unwrap_or_default()
}

pub fn is_purged(data: &Path, id: &str, name: &str) -> bool {
    purged_map(data).get(id).is_some_and(|names| names.iter().any(|other| other == name))
}

fn record_purged(data: &Path, id: &str, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let mut map = purged_map(data);
    let kept = map.entry(id.to_string()).or_default();
    for name in names {
        if !kept.contains(name) {
            kept.push(name.clone());
        }
    }
    save_purged(data, &map);
}

fn forget_purged(data: &Path, id: &str, name: &str) {
    let mut map = purged_map(data);
    if let Some(kept) = map.get_mut(id) {
        kept.retain(|other| other != name);
    }
    save_purged(data, &map);
}

// ---- Measuring ----------------------------------------------------------------

/// Every file in a script's folder: (path inside the folder, bytes).
fn files_in(dir: &Path) -> Vec<(String, u64)> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in fs::read_dir(&at).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(relative) = path.strip_prefix(dir) {
                let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                found.push((relative.to_string_lossy().replace('\\', "/"), size));
            }
        }
    }
    found
}

/// Why a script's media may not be deleted yet, or nothing when it may.
/// `uploaded` is what the cloud holds for the script (name, bytes), from
/// the cloud ledger and Edit's shared list together.
fn blocked_reason(status: &str, busy: bool, media: &[(String, u64)], uploaded: &BTreeMap<String, u64>) -> Option<String> {
    if media.is_empty() {
        return Some("No footage on this Mac.".into());
    }
    if status != "ready" {
        return Some("Its import hasn't finished.".into());
    }
    if busy {
        return Some("Writer is working on it now.".into());
    }
    for (name, size) in media {
        if uploaded.get(name) != Some(size) {
            return Some(format!("{name} hasn't reached the cloud yet."));
        }
    }
    None
}

/// What the cloud holds of a script's footage, by its name in the folder:
/// the cloud ledger's files, plus the recordings in Edit's originals
/// (which cloud.rs leaves out of its own sync on purpose).
fn uploaded_files(data: &Path, dir: &Path, id: &str) -> BTreeMap<String, u64> {
    let mut uploaded = cloud::synced_files(data, id);
    // Edit's copies aren't in Writer's ledger; a recording the handoff
    // shared counts as uploaded at the size it has here.
    for name in handoff::shared_files(dir) {
        if let Ok(meta) = fs::metadata(dir.join(&name)) {
            uploaded.entry(name).or_insert(meta.len());
        }
    }
    uploaded
}

fn script_storage(data: &Path, dir: &Path) -> Option<ScriptStorage> {
    let script = store::read(dir).ok()?;
    let files = files_in(dir);
    let total_bytes: u64 = files.iter().map(|(_, size)| *size).sum();
    let media: Vec<(String, u64)> = files.into_iter().filter(|(name, _)| archive::is_footage(name)).collect();
    let media_bytes: u64 = media.iter().map(|(_, size)| *size).sum();
    let uploaded = uploaded_files(data, dir, &script.id);
    let busy = jobs::running(&script.id) || import::importing(&script.id);
    let reason = blocked_reason(&script.status, busy, &media, &uploaded);
    Some(ScriptStorage {
        reclaimable_bytes: if reason.is_none() { media_bytes } else { 0 },
        blocked_reason: reason,
        id: script.id,
        title: script.title,
        total_bytes,
        media_bytes,
    })
}

/// Every script's storage, largest media first.
pub fn report(data: &Path) -> Report {
    let root = store::scripts_root(data);
    let mut scripts: Vec<ScriptStorage> = fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| script_storage(data, &entry.path()))
        .collect();
    scripts.sort_by(|a, b| b.media_bytes.cmp(&a.media_bytes));
    Report {
        total_bytes: scripts.iter().map(|script| script.total_bytes).sum(),
        media_bytes: scripts.iter().map(|script| script.media_bytes).sum(),
        reclaimable_bytes: scripts.iter().map(|script| script.reclaimable_bytes).sum(),
        blocked_bytes: scripts.iter().filter(|script| script.reclaimable_bytes == 0).map(|script| script.media_bytes).sum(),
        scripts,
    }
}

// ---- Clearing -----------------------------------------------------------------

/// Proxies left on this Mac are a cache, capped at this much in all:
/// past it, the least recently changed scripts' previews go first (they
/// are fetched back from the cloud on request).
pub const CACHE_CAP_BYTES: u64 = 500_000_000;

/// A file verified in the cloud, live: its object is there at this size.
/// The ledger alone never deletes anything; this (an `r2/head` through
/// the relay, via cloud.rs) is the verified-upload signal. An error is
/// offline or refused, and also never deletes anything.
fn verified_in_cloud(data: &Path, dir: &Path, id: &str, name: &str, size: u64) -> Result<bool, String> {
    if cloud::verify_file(data, id, name, size)? {
        return Ok(true);
    }
    // A joined case's recordings live under Edit's keys, not Writer's.
    if let Some((_, key)) = handoff::shared_entries(dir).into_iter().find(|(file, _)| file == name) {
        return cloud::verify_key(data, &key, size);
    }
    Ok(false)
}

/// Deletes a script's footage that the cloud holds, keeping everything
/// else. With `keep_preview`, the 480p preview stays as this Mac's cache
/// so the script still plays here (the automatic sweep's choice);
/// without it, the preview goes too and is fetched back on request (the
/// producer's Clear). Refuses, saying why, when any of it hasn't been
/// verified in the cloud yet; offline, nothing is deleted.
pub fn purge_script(data: &Path, id: &str, keep_preview: bool) -> Result<u64, String> {
    purge_with(data, id, keep_preview, &|name, size| verified_in_cloud(data, &store::script_dir(data, id)?, id, name, size))
}

fn purge_with(data: &Path, id: &str, keep_preview: bool, verify: &dyn Fn(&str, u64) -> Result<bool, String>) -> Result<u64, String> {
    if jobs::running(id) || import::importing(id) {
        return Err("Writer is working on this script now.".into());
    }
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let files = files_in(&dir);
    let mut media: Vec<(String, u64)> = files.into_iter().filter(|(name, _)| archive::is_footage(name)).collect();
    if keep_preview {
        media.retain(|(name, _)| name != "proxy.mp4");
    }
    let uploaded = uploaded_files(data, &dir, id);
    if let Some(reason) = blocked_reason(&script.status, false, &media, &uploaded) {
        return Err(reason);
    }
    for (name, size) in &media {
        match verify(name, *size) {
            Ok(true) => {}
            Ok(false) => return Err(format!("{name} isn't verified in the cloud yet, so it stays on this Mac.")),
            Err(error) => return Err(format!("Couldn't verify {name} in the cloud ({error}), so it stays on this Mac.")),
        }
    }
    let mut freed = 0;
    let mut names = Vec::new();
    for (name, size) in &media {
        let path = dir.join(name);
        // Inside the script's folder only: a file from this Mac's disk
        // (a single recording kept under drive/, say) that points outside
        // it is never touched; files_in only found what is inside.
        if fs::remove_file(&path).is_ok() {
            freed += *size;
            names.push(name.clone());
        }
    }
    record_purged(data, id, &names);
    if freed > 0 {
        crate::log::info("cloud", format!("{}: local footage cleared ({}, {} kept on this Mac)", crate::log::named(id, &script.title), crate::log::size(freed), if keep_preview { "preview" } else { "script and transcript only" }));
        store::library_changed();
    }
    Ok(freed)
}

/// Evicts cached previews past the cap, least recently changed first.
/// A preview is only evicted once verified in the cloud; one that can't
/// be verified stays, cap or not.
pub fn enforce_cache_cap(data: &Path, cap_bytes: u64) -> u64 {
    enforce_cache_cap_with(data, cap_bytes, &|id, name, size| verified_in_cloud(data, &store::script_dir(data, id).unwrap_or_default(), id, name, size))
}

fn enforce_cache_cap_with(data: &Path, cap_bytes: u64, verify: &dyn Fn(&str, &str, u64) -> Result<bool, String>) -> u64 {
    let root = store::scripts_root(data);
    let mut previews: Vec<(i64, String, u64)> = Vec::new();
    for entry in fs::read_dir(&root).into_iter().flatten().flatten().filter(|entry| entry.path().is_dir()) {
        let Ok(script) = store::read(&entry.path()) else { continue };
        let size = fs::metadata(store::proxy_path(&entry.path())).map(|meta| meta.len()).unwrap_or(0);
        if size > 0 {
            previews.push((script.updated_at, script.id, size));
        }
    }
    let mut total: u64 = previews.iter().map(|(_, _, size)| *size).sum();
    if total <= cap_bytes {
        return 0;
    }
    previews.sort();
    let mut freed = 0;
    for (_, id, size) in previews {
        if total <= cap_bytes {
            break;
        }
        if verify(&id, "proxy.mp4", size).map(|ok| !ok).unwrap_or(true) {
            continue;
        }
        let Ok(dir) = store::script_dir(data, &id) else { continue };
        if fs::remove_file(store::proxy_path(&dir)).is_ok() {
            record_purged(data, &id, &["proxy.mp4".to_string()]);
            freed += size;
            total -= size;
        }
    }
    freed
}

/// Clears every script whose footage is confirmed in the cloud.
pub fn purge_all(data: &Path, keep_preview: bool) -> Cleared {
    let before = report(data);
    let mut cleared = Cleared::default();
    for script in &before.scripts {
        if script.media_bytes == 0 {
            continue;
        }
        match purge_script(data, &script.id, keep_preview) {
            Ok(freed) => {
                cleared.freed_bytes += freed;
                if freed > 0 {
                    cleared.cleared.push(script.id.clone());
                }
            }
            Err(_) => cleared.blocked.push(script.clone()),
        }
    }
    cleared
}

/// The script's preview back from the cloud, when it was cleared: the
/// proxy is in Writer's cloud files under its ledger key.
pub fn fetch_preview(data: &Path, id: &str) -> Result<PathBuf, String> {
    let dir = store::script_dir(data, id)?;
    let proxy = store::proxy_path(&dir);
    if proxy.is_file() {
        return Ok(proxy);
    }
    let (key, size) = cloud::file_key(data, id, "proxy.mp4").ok_or("This script's preview isn't in the cloud yet. Wait for it to sync, then try again.")?;
    cloud::get_file(data, id, &key, size, &proxy)?;
    forget_purged(data, id, "proxy.mp4");
    store::library_changed();
    Ok(proxy)
}

// ---- The sweep ------------------------------------------------------------------

/// Clears confirmed originals in the background: a script's own footage
/// (its original and each joined recording) once the cloud holds it,
/// keeping the preview so the script still plays on this Mac. Runs every
/// few minutes; a script being worked on is left alone by purge_script.
pub fn start(data: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            let report = report(&data);
            for script in report.scripts.iter().filter(|script| script.reclaimable_bytes > 0) {
                // The duplicate footage goes by itself once verified;
                // previews stay as cache until the cap or Clear.
                let _ = purge_script(&data, &script.id, true);
            }
            enforce_cache_cap(&data, CACHE_CAP_BYTES);
            std::thread::sleep(Duration::from_secs(300));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(names: &[(&str, u64)]) -> Vec<(String, u64)> {
        names.iter().map(|(name, size)| (name.to_string(), *size)).collect()
    }

    fn uploaded(names: &[(&str, u64)]) -> BTreeMap<String, u64> {
        names.iter().map(|(name, size)| (name.to_string(), *size)).collect()
    }

    #[test]
    fn confirmed_media_may_go_and_the_rest_may_not() {
        let files = media(&[("original.mp4", 100), ("proxy.mp4", 90)]);
        // Still importing, or busy: never.
        assert!(blocked_reason("processing", false, &files, &uploaded(&[("original.mp4", 100), ("proxy.mp4", 90)])).unwrap().contains("import"));
        assert!(blocked_reason("ready", true, &files, &uploaded(&[("original.mp4", 100), ("proxy.mp4", 90)])).unwrap().contains("working"));
        // Nothing uploaded yet, or only part of it: never, and it says which file.
        assert!(blocked_reason("ready", false, &files, &uploaded(&[])).unwrap().contains("original.mp4"));
        assert!(blocked_reason("ready", false, &files, &uploaded(&[("original.mp4", 100)])).unwrap().contains("proxy.mp4"));
        // A file that changed since it went up (a rebuilt preview) hasn't gone up.
        assert!(blocked_reason("ready", false, &files, &uploaded(&[("original.mp4", 100), ("proxy.mp4", 80)])).is_some());
        // Everything there, at the size it has here: it may go.
        assert!(blocked_reason("ready", false, &files, &uploaded(&[("original.mp4", 100), ("proxy.mp4", 90)])).is_none());
        // No footage at all is not an error to clear, just nothing to do.
        assert!(blocked_reason("ready", false, &[], &uploaded(&[])).unwrap().contains("No footage"));
    }

    #[test]
    fn purge_keeps_the_script_and_its_words() {
        let data = std::env::temp_dir().join(format!("writer-storage-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        let dir = data.join("scripts").join("aaaa1111");
        fs::create_dir_all(dir.join("transcript/whole")).unwrap();
        fs::create_dir_all(dir.join("compilation")).unwrap();
        let script = r#"{"id":"aaaa1111","title":"T","sourceKind":"youtube","sourceUrl":"https://youtube.com/watch?v=x","originalPath":"","status":"ready","stage":"ready","progress":100,"error":null,"durationMs":1000,"revision":1,"createdAt":0,"updatedAt":0}"#;
        fs::write(dir.join("script.json"), script).unwrap();
        fs::write(dir.join("proxy.mp4"), vec![0u8; 500]).unwrap();
        fs::write(dir.join("compilation/00.mp4"), vec![0u8; 700]).unwrap();
        fs::write(dir.join("transcript/whole/transcript.json"), "{}").unwrap();
        // Not in the cloud yet: refused, and nothing is deleted.
        assert!(purge_with(&data, "aaaa1111", false, &|_, _| Ok(true)).is_err());
        assert!(dir.join("proxy.mp4").is_file() && dir.join("compilation/00.mp4").is_file());
        // In the ledger, but live verification says no (or can't say):
        // still refused. The ledger alone never deletes anything.
        let ledger = data.join("cloud");
        fs::create_dir_all(&ledger).unwrap();
        fs::write(ledger.join("ledger.json"), r#"{"scripts":{"aaaa1111":{"rev":1,"body":"x","files":{"proxy.mp4":{"key":"k1","size":500,"mtime":0},"compilation/00.mp4":{"key":"k2","size":700,"mtime":0}}}}}"#).unwrap();
        assert!(purge_with(&data, "aaaa1111", false, &|_, _| Ok(false)).is_err());
        assert!(purge_with(&data, "aaaa1111", false, &|_, _| Err("offline".into())).is_err());
        assert!(dir.join("proxy.mp4").is_file() && dir.join("compilation/00.mp4").is_file());
        let shown = report(&data);
        assert_eq!((shown.total_bytes, shown.media_bytes, shown.reclaimable_bytes), (script.len() as u64 + 1202, 1200, 1200));
        // Verified in the cloud: the sweep's purge keeps the preview.
        assert_eq!(purge_with(&data, "aaaa1111", true, &|_, _| Ok(true)).unwrap(), 700);
        assert!(dir.join("proxy.mp4").is_file() && !dir.join("compilation/00.mp4").exists());
        assert!(is_purged(&data, "aaaa1111", "compilation/00.mp4"));
        // A full clear takes the preview too, and keeps the small files.
        assert_eq!(purge_with(&data, "aaaa1111", false, &|_, _| Ok(true)).unwrap(), 500);
        assert!(dir.join("script.json").is_file() && dir.join("transcript/whole/transcript.json").is_file());
        assert_eq!(report(&data).media_bytes, 0);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn cache_cap_evicts_oldest_verified_preview_first() {
        let data = std::env::temp_dir().join(format!("writer-storage-cap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        for (id, at, size) in [("aaaa1111", 1, 600u64), ("bbbb2222", 2, 600), ("cccc3333", 3, 600)] {
            let dir = data.join("scripts").join(id);
            fs::create_dir_all(&dir).unwrap();
            let script = format!(r#"{{"id":"{id}","title":"T","sourceKind":"youtube","sourceUrl":"u","originalPath":"","status":"ready","stage":"ready","progress":100,"error":null,"durationMs":1000,"revision":1,"createdAt":0,"updatedAt":{at}}}"#);
            fs::write(dir.join("script.json"), script).unwrap();
            fs::write(dir.join("proxy.mp4"), vec![0u8; size as usize]).unwrap();
        }
        // Under the cap: nothing goes, verified or not.
        assert_eq!(enforce_cache_cap_with(&data, 2_000, &|_, _, _| Ok(true)), 0);
        // Over it: the oldest goes, down to the cap; unverified stays.
        assert_eq!(enforce_cache_cap_with(&data, 1_200, &|id, _, _| Ok(id != "bbbb2222")), 600);
        assert!(!data.join("scripts/aaaa1111/proxy.mp4").exists());
        assert!(data.join("scripts/bbbb2222/proxy.mp4").is_file());
        assert!(data.join("scripts/cccc3333/proxy.mp4").is_file());
        let _ = fs::remove_dir_all(&data);
    }
}
