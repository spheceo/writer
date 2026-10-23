//! Archived scripts, and every script fully in the cloud.
//!
//! Repack's page archives a script to free space: its server deletes the
//! footage on R2 (Writer's and Edit's copies) and marks the script's row
//! `archived: {at, by, freedBytes}`. When Writer hears it (cloud.rs keeps
//! each row's state), it deletes the script's footage here too: originals,
//! the preview, Drive recordings, reused cases' videos and heard audio. The
//! script, its transcripts, the watcher's answers, versions, chat and every
//! other small file stay, so it still lists and reads, read-only. A file
//! from the Mac's disk outside Writer's folder is the producer's own: it's
//! never touched.
//!
//! Restore (Writer's button, or `restore` set on Repack's page) downloads
//! the footage again: from Edit's originals where they're back, else from
//! the recordings' YouTube and Drive links, and rebuilds the preview. The
//! transcripts were kept, so nothing is heard again. What can't come back
//! (a file from disk) is listed as missing. Then `writer:clearRestore`.
//!
//! Fully cloud: every script's footage is in Edit's footage registry
//! (handoff.rs) from its import on. The scripts from before are brought in
//! by a backfill, one at a time in the background; a YouTube original from
//! before Writer downloaded in Edit's format is downloaded again in it first,
//! so both apps share one file.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{cloud, handoff, import, jobs, media, settings, store, transcribe};

/// What the app shows of a script's archive.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub at: i64,
    pub by: String,
    pub freed_bytes: u64,
    /// Asked back (from Repack's page or here), and being restored.
    pub restoring: bool,
    /// What the last restore couldn't bring back.
    pub missing: Vec<String>,
}

/// This Mac's side: which archive it has cleared its footage for, which
/// restore it has carried out, and what that restore couldn't bring back.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Local {
    #[serde(default)]
    purged_for: Option<i64>,
    #[serde(default)]
    restored_for: Option<i64>,
    #[serde(default)]
    restoring: bool,
    #[serde(default)]
    missing: Vec<String>,
}

static FILES: Mutex<()> = Mutex::new(());

fn read<T: for<'a> Deserialize<'a> + Default>(path: &Path) -> T {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write<T: Serialize>(path: &Path, value: &T) {
    let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
    let tmp = path.with_extension("json.tmp");
    if fs::write(&tmp, serde_json::to_vec(value).unwrap_or_default()).is_ok() {
        let _ = fs::rename(tmp, path);
    }
}

fn states_path(data: &Path) -> PathBuf {
    data.join("cloud").join("archive.json")
}

fn local_path(data: &Path) -> PathBuf {
    data.join("cloud").join("archive-local.json")
}

/// The rows' archive states, as cloud.rs reads them on each pull.
pub fn keep_states(data: &Path, states: BTreeMap<String, Value>) {
    let _guard = FILES.lock();
    write(&states_path(data), &states);
}

fn states(data: &Path) -> BTreeMap<String, Value> {
    read(&states_path(data))
}

fn locals(data: &Path) -> BTreeMap<String, Local> {
    read(&local_path(data))
}

fn change_local(data: &Path, id: &str, edit: impl FnOnce(&mut Local)) {
    let _guard = FILES.lock();
    let path = local_path(data);
    let mut all: BTreeMap<String, Local> = read(&path);
    edit(all.entry(id.to_string()).or_default());
    write(&path, &all);
}

fn archived_at(state: &Value) -> Option<i64> {
    state["archived"]["at"].as_f64().map(|at| at as i64)
}

/// Whether the script is archived and not restored here since.
pub fn archived(data: &Path, id: &str) -> bool {
    let Some(at) = states(data).get(id).and_then(archived_at) else { return false };
    locals(data).get(id).and_then(|local| local.restored_for) != Some(at)
}

/// The script's archive, for the app to show, if it's archived.
pub fn state(data: &Path, id: &str) -> Option<State> {
    let state = states(data).get(id).cloned()?;
    let at = archived_at(&state)?;
    let local = locals(data).get(id).cloned().unwrap_or_default();
    if local.restored_for == Some(at) {
        return None;
    }
    Some(State {
        at,
        by: state["archived"]["by"].as_str().unwrap_or("").to_string(),
        freed_bytes: state["archived"]["freedBytes"].as_f64().unwrap_or(0.0) as u64,
        restoring: local.restoring || jobs::running(id) && state["restore"].is_object(),
        missing: local.missing,
    })
}

/// What counts as footage in a script's folder: video and sound, the big files an archive frees.
pub fn is_footage(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi" | "ts" | "m4a" | "mp3" | "aac" | "wav" | "flac" | "ogg" | "opus" | "part")
}

/// Deletes the footage in a script's folder, keeping everything else. Answers the bytes freed.
fn purge(dir: &Path) -> u64 {
    let mut freed = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in fs::read_dir(&at).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if is_footage(&entry.file_name().to_string_lossy()) {
                freed += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
                let _ = fs::remove_file(&path);
            }
        }
    }
    freed
}

/// Restores the script: asks Repack to bring it back if it hasn't been asked,
/// and starts the restore here.
pub fn start_restore(app: &AppHandle, data: &Path, id: &str) -> Result<jobs::BuildStatus, String> {
    if state(data, id).is_none() {
        return Err("That script isn't archived.".into());
    }
    change_local(data, id, |local| local.restoring = true);
    jobs::start(app.clone(), id.to_string(), jobs::Work::Restore)
}

/// Brings an archived script's footage back. Runs as a job.
pub fn restore(app: &AppHandle, id: &str) -> Result<(String, f64), String> {
    let data = crate::data_dir(app)?;
    crate::log::info("archive", format!("{}: restoring its footage", crate::log::named(id, &crate::jobs::title_of(&data, id))));
    let result = restore_footage(&data, id);
    change_local(&data, id, |local| local.restoring = false);
    let missing = result?;
    let at = states(&data).get(id).and_then(archived_at);
    change_local(&data, id, |local| {
        local.restored_for = at;
        local.purged_for = None;
        local.missing = missing.clone();
    });
    // Repack's row: no longer asked back, and the footage is here again.
    let _ = cloud::call(&data, "mutation", "writer:clearRestore", json!({ "scriptId": id }));
    cloud::sync_soon();
    Ok((
        if missing.is_empty() { "The footage is back, and the transcripts as they were: nothing was heard again.".into() } else { format!("The footage is back, but not {}: it came from this Mac's disk, and the archive deleted it.", missing.join(", ")) },
        0.0,
    ))
}

/// Each recording again, then the preview; answers what couldn't come back.
fn restore_footage(data: &Path, id: &str) -> Result<Vec<String>, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let settings = settings::load(data);
    let recordings = transcribe::recordings(&dir, &script);
    let sources = import::part_sources(&dir, &script);
    let targets = import::part_targets(&dir, &script);
    let joined = script.parts.len() > 1;
    let mut missing = Vec::new();
    let mut files: Vec<Option<PathBuf>> = Vec::new();
    for index in 0..recordings.len() {
        let title = if joined { script.parts[index].title.clone() } else { script.title.clone() };
        // Where it belongs: its own file, or for a reused case's footage, a file of its own to join from.
        let target = if joined {
            match targets.get(index).cloned().flatten() {
                Some(path) => path,
                None => dir.join(".reuse").join(format!("restored-{index:02}.mp4")),
            }
        } else {
            PathBuf::from(&script.original_path)
        };
        if import::readable(&target) {
            files.push(Some(target));
            continue;
        }
        jobs::stage(id, &format!("Bringing back {title}"));
        let source = sources.get(index).cloned().flatten();
        let link = source.as_ref().and_then(|(source, _)| link_of(source)).or_else(|| reuse_link(&script, index));
        let inside = target.starts_with(&dir);
        let got = match (&source, &link) {
            _ if !inside => Err("from disk".to_string()),
            (Some((source, _)), _) if from_registry(data, id, source, &target).is_ok() => Ok(()),
            (_, Some(link)) => import::fetch_video(data, link, &target, &settings, &|percent, _| jobs::stage(id, &format!("Bringing back {title} · {percent}%"))),
            _ => Err("no link".into()),
        };
        match got {
            Ok(()) => {
                crate::log::info("archive", format!("{}: {title} back ({})", crate::log::named(id, &script.title), crate::log::size(fs::metadata(&target).map(|meta| meta.len()).unwrap_or(0))));
                files.push(Some(target));
            }
            Err(error) => {
                crate::log::warn("archive", format!("{}: {title} can't come back: {error}", crate::log::named(id, &script.title)));
                missing.push(title);
                files.push(None);
            }
        }
    }
    // The preview, as the import made it.
    let proxy = store::proxy_path(&dir);
    if !proxy.is_file() && files.iter().all(Option::is_some) {
        jobs::stage(id, "Rebuilding the preview");
        if joined {
            let mut parts = Vec::new();
            for (part, file) in script.parts.iter().zip(&files) {
                let path = file.as_deref().ok_or("A recording is missing.")?;
                let found = media::streams(path)?;
                parts.push((path.to_path_buf(), media::Streams { duration_ms: part.duration_ms, ..found }));
            }
            let join: Vec<media::JoinPart> = parts.iter().map(|(path, streams)| media::JoinPart { path, streams: media::Streams { duration_ms: streams.duration_ms, video: streams.video, audio: streams.audio } }).collect();
            media::join(&join, &proxy, &|percent| jobs::stage(id, &format!("Rebuilding the preview · {percent}%")))?;
        } else if let Some(Some(original)) = files.first() {
            let found = media::streams(original)?;
            if found.video {
                media::encode_proxy(original, &proxy, script.duration_ms, &|percent| jobs::stage(id, &format!("Rebuilding the preview · {percent}%")))?;
            } else {
                let parts = [media::JoinPart { path: original, streams: media::Streams { duration_ms: script.duration_ms, ..found } }];
                media::join(&parts, &proxy, &|percent| jobs::stage(id, &format!("Rebuilding the preview · {percent}%")))?;
            }
        }
    }
    // In Edit's registry again (the archive marked it not ready); the words are kept.
    if handoff::SHARE {
        jobs::stage(id, "Putting the footage back in the cloud");
        handoff::share(data, id, false)?;
    }
    Ok(missing)
}

/// A recording from Edit's originals, when they're there (Edit restored it, or it never left).
pub(crate) fn from_registry(data: &Path, id: &str, source: &str, target: &Path) -> Result<(), String> {
    let record = cloud::call(data, "query", "edit:getMedia", json!({ "source": source }))?;
    let key = record["originalKey"].as_str().filter(|_| record["originalReady"] == true).ok_or("not in the registry")?;
    let size = record["size"].as_f64().unwrap_or(0.0) as u64;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    cloud::get_file(data, id, key, size, target)
}

/// A source's link to download it from.
fn link_of(source: &str) -> Option<String> {
    let (kind, id) = source.split_once(':')?;
    match kind {
        "youtube" => Some(format!("https://www.youtube.com/watch?v={id}")),
        "drive" => Some(format!("https://drive.google.com/file/d/{id}/view")),
        _ => None,
    }
}

/// A reused case's footage link, for its part of a compilation.
fn reuse_link(script: &crate::model::Script, index: usize) -> Option<String> {
    let url = script.parts.get(index)?.reuse.as_ref()?;
    let reuse = script.reuses.iter().find(|reuse| &reuse.url == url)?;
    Some(reuse.footage.clone().unwrap_or_else(|| reuse.url.clone()))
}

// ---- Watching, and the backfill -------------------------------------------------

static BACKFILLING: AtomicBool = AtomicBool::new(false);

/// Acts on archives and restores heard from the cloud, and brings the
/// scripts from before into the footage registry, one at a time.
pub fn start(app: AppHandle, data: PathBuf) {
    std::thread::spawn(move || {
        let started = Instant::now();
        let next_youtube: Mutex<Instant> = Mutex::new(Instant::now());
        let next_youtube = std::sync::Arc::new(next_youtube);
        loop {
            std::thread::sleep(Duration::from_secs(20));
            react(&app, &data);
            // The backfill waits for the app to settle, and never runs two at once.
            if handoff::SHARE && started.elapsed() > Duration::from_secs(120) && !BACKFILLING.swap(true, Ordering::SeqCst) {
                let (data, next_youtube) = (data.clone(), next_youtube.clone());
                std::thread::spawn(move || {
                    let _ = backfill_next(&data, &next_youtube);
                    BACKFILLING.store(false, Ordering::SeqCst);
                });
            }
        }
    });
}

fn react(app: &AppHandle, data: &Path) {
    for (id, state) in states(data) {
        if jobs::running(&id) || import::importing(&id) {
            continue;
        }
        let local = locals(data).get(&id).cloned().unwrap_or_default();
        let at = archived_at(&state);
        if at.is_some() && local.restored_for == at {
            continue;
        }
        if state["restore"].is_object() && !local.restoring {
            let _ = start_restore(app, data, &id);
            continue;
        }
        if at.is_some() && local.purged_for != at {
            if let Ok(dir) = store::script_dir(data, &id) {
                let freed = purge(&dir);
                crate::log::info("archive", format!("{}: archived by {}; its footage deleted here ({})", crate::log::named(&id, &crate::jobs::title_of(data, &id)), state["archived"]["by"].as_str().unwrap_or("Repack"), crate::log::size(freed)));
            }
            change_local(data, &id, |local| local.purged_for = at);
        }
    }
}

/// What the backfill has done, by script: "done", or why it passed one over.
fn backfill_path(data: &Path) -> PathBuf {
    data.join("cloud").join("backfill.json")
}

/// The next script from before not yet in the footage registry, brought in.
/// One that failed is tried again after six hours; one waiting its turn for
/// YouTube, at the next round.
fn backfill_next(data: &Path, next_youtube: &Mutex<Instant>) -> Result<(), String> {
    let mut done: BTreeMap<String, String> = read(&backfill_path(data));
    let now = crate::model::now_ms();
    let due = |said: &str| said.strip_prefix("failed@").and_then(|rest| rest.split(':').next()).and_then(|at| at.parse::<i64>().ok()).is_some_and(|at| now - at > 6 * 3_600_000);
    let rows = store::list(data)?;
    let Some(row) = rows.iter().find(|row| done.get(&row.id).is_none_or(|said| due(said))) else { return Ok(()) };
    let id = row.id.clone();
    let began = Instant::now();
    match backfill(data, &id, next_youtube) {
        Ok(said) => {
            if said != "passed over" {
                crate::log::info("backfill", format!("{} in {}: {said}", crate::log::named(&id, &row.title), crate::log::since_start(began)));
            }
            done.insert(id, said);
        }
        Err(error) if error == WAITING => return Ok(()),
        Err(error) => {
            crate::log::warn("backfill", format!("{}: failed, trying again in six hours: {error}", crate::log::named(&id, &row.title)));
            done.insert(id, format!("failed@{now}: {error}"));
        }
    }
    write(&backfill_path(data), &done);
    Ok(())
}

const WAITING: &str = "waiting for YouTube";

fn backfill(data: &Path, id: &str, next_youtube: &Mutex<Instant>) -> Result<String, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    if script.status != "ready" || script.commentary.is_some() || script.title.starts_with("TEST") || archived(data, id) || jobs::running(id) || import::importing(id) {
        return Ok("passed over".into());
    }
    // A YouTube original from before is downloaded again in Edit's format,
    // at most one a quarter hour, so YouTube never sees a run of them. It goes
    // in the script's folder; a file elsewhere on the Mac is left as it is.
    let original = PathBuf::from(&script.original_path);
    let mut replaced = false;
    if script.source_kind == "youtube" && original.is_file() && codec(&original).as_deref() != Some("h264") {
        let wait = next_youtube.lock().map(|at| at.saturating_duration_since(Instant::now())).unwrap_or_default();
        if !wait.is_zero() {
            return Err(WAITING.into());
        }
        if let Ok(mut at) = next_youtube.lock() {
            *at = Instant::now() + Duration::from_secs(15 * 60);
        }
        let fresh = dir.join("original-edit.mp4");
        let settings = settings::load(data);
        // One YouTube won't give again (age-restricted, gone) keeps Writer's older download.
        // From Edit's originals when another Mac (or Edit) already has it; else YouTube on this Mac's own address only.
        if let Err(error) = import::fetch_video_paced(data, &script.source_url, &fresh, &settings, import::Pace::Backfill, &|_, _| {}) {
            let _ = fs::remove_file(&fresh);
            let kept = handoff::share(data, id, false)?;
            return Ok(format!("kept the older download ({error}); {kept}"));
        }
        let before = media::streams(&original).map(|found| found.duration_ms).unwrap_or(0);
        let after = media::streams(&fresh).map(|found| found.duration_ms).unwrap_or(0);
        // The same video, or the old one stays.
        if before.abs_diff(after) <= 1_000 {
            let target = dir.join("original.mp4");
            fs::rename(&fresh, &target).map_err(|error| error.to_string())?;
            if target != original {
                let mut now = store::read(&dir)?;
                now.original_path = target.to_string_lossy().into_owned();
                store::write(&dir, &now)?;
            }
            replaced = true;
        } else {
            let _ = fs::remove_file(&fresh);
        }
    }
    if replaced {
        // Writer's older download may be in the registry already: this one takes its place.
        handoff::replace_originals(data, id)?;
    }
    handoff::share(data, id, false)
}

fn codec(path: &Path) -> Option<String> {
    let path = path.to_string_lossy().into_owned();
    let out = crate::proc::run("ffprobe", &["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=codec_name", "-of", "csv=p=0", &path], Duration::from_secs(30)).ok()?;
    Some(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footage_is_video_and_sound_only() {
        for name in ["original.mp4", "proxy.mp4", "drive/00.MOV", "heard-x/audio.m4a", "transcript/whole/audio.ogg", "x.mp4.part"] {
            assert!(is_footage(name), "{name}");
        }
        for name in ["script.json", "peaks.bin", "transcript/whole/transcript.json", "watch/0.json", "finds/frames/frames-01.jpg", "names.json"] {
            assert!(!is_footage(name), "{name}");
        }
    }

    #[test]
    fn purges_footage_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("writer-archive-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("drive")).unwrap();
        fs::create_dir_all(dir.join("transcript/whole")).unwrap();
        fs::write(dir.join("script.json"), "{}").unwrap();
        fs::write(dir.join("proxy.mp4"), vec![0u8; 1000]).unwrap();
        fs::write(dir.join("drive/00.mp4"), vec![0u8; 500]).unwrap();
        fs::write(dir.join("drive/manifest.json"), "{}").unwrap();
        fs::write(dir.join("transcript/whole/transcript.json"), "{}").unwrap();
        assert_eq!(purge(&dir), 1500);
        assert!(dir.join("script.json").is_file() && dir.join("drive/manifest.json").is_file() && dir.join("transcript/whole/transcript.json").is_file());
        assert!(!dir.join("proxy.mp4").exists() && !dir.join("drive/00.mp4").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn links_back_to_each_source() {
        assert_eq!(link_of("youtube:OVITtAlFG6M").as_deref(), Some("https://www.youtube.com/watch?v=OVITtAlFG6M"));
        assert_eq!(link_of("drive:1AbC").as_deref(), Some("https://drive.google.com/file/d/1AbC/view"));
        assert_eq!(link_of("sha256:ab"), None);
    }
}
