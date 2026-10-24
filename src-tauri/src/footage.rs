//! A commentary's footage: the stretch of every YouTube clip the script
//! plays, downloaded at 480p into the script's folder once its clips are
//! known (after a build, a revision or a clip search, and when it's opened),
//! so Split plays the footage itself, as a footage script's preview does,
//! instead of showing a card. Kept by video and stretch in `footage.json`:
//! a clip that stays put isn't fetched again. Each Mac downloads its own
//! (the files aren't synced; they come from YouTube in a minute).

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::commentary::{Cite, Draft};
use crate::{proc, store};

/// Seconds either side of a clip, so a clip trimmed a little still plays from the file.
const PAD_MS: u64 = 3_000;
/// A clip longer than this is fetched only this long (a whole stream is never one clip).
const MAX_MS: u64 = 3 * 60_000;
/// H.264 in an mp4, which the Mac's player plays; 480p at most.
const FORMAT: &str = "b[vcodec^=avc1][height<=480][ext=mp4]/bv*[vcodec^=avc1][height<=480]+ba[ext=m4a]/b[height<=480][ext=mp4]/bv*[height<=480]+ba/b";

/// One downloaded stretch of a video.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Piece {
    /// The YouTube video's id.
    pub video: String,
    /// Where the file starts and ends in the video, ms.
    pub from_ms: u64,
    pub to_ms: u64,
    /// The file, inside the script's `footage/` folder.
    pub file: String,
    /// Its index (moov) is at the front, so a browser seeks in it at once.
    #[serde(default)]
    pub fast: bool,
}

/// What the view needs: the pieces there are, and how many are still coming.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Footage {
    pub pieces: Vec<Piece>,
    /// The folder they're in, for the Mac app's file URLs.
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub pending: usize,
    #[serde(default)]
    pub failed: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Kept {
    #[serde(default)]
    pieces: Vec<Piece>,
    /// Stretches that wouldn't download ("<video>@<from>-<to>"), not tried again this run.
    #[serde(default)]
    failed: Vec<String>,
    /// Files put under the script's review (writer/reviews/<id>/clips/), for its review page.
    #[serde(default)]
    uploaded: Vec<String>,
    /// The manifest as last written, so an earlier version's clips stay listed.
    #[serde(default)]
    published: Vec<serde_json::Value>,
}

fn folder(dir: &Path) -> PathBuf {
    dir.join("footage")
}

fn read(dir: &Path) -> Kept {
    fs::read(folder(dir).join("footage.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write(dir: &Path, kept: &Kept) {
    let _ = fs::create_dir_all(folder(dir));
    let tmp = folder(dir).join("footage.json.tmp");
    if fs::write(&tmp, serde_json::to_vec_pretty(kept).unwrap_or_default()).is_ok() {
        let _ = fs::rename(&tmp, folder(dir).join("footage.json"));
    }
}

/// The YouTube video a clip plays: from its link, else its source's.
fn video_of(cite: &Cite, origins: &BTreeMap<String, String>) -> Option<String> {
    cite.link.as_deref().and_then(crate::handoff::youtube_id).or_else(|| origins.get(&cite.source).and_then(|origin| crate::handoff::youtube_id(origin)))
}

/// Every stretch the draft plays, by video: what's needed, padded.
fn wanted(draft: &Draft, origins: &BTreeMap<String, String>) -> Vec<(String, u64, u64)> {
    let mut out: Vec<(String, u64, u64)> = Vec::new();
    for cite in draft.cites() {
        let Some(video) = video_of(cite, origins) else { continue };
        let from = cite.start_ms.saturating_sub(PAD_MS);
        let to = (cite.end_ms + PAD_MS).min(from + MAX_MS);
        if to <= from {
            continue;
        }
        // Overlapping or touching stretches of one video are one download.
        match out.iter_mut().find(|(other, a, b)| *other == video && from <= *b + PAD_MS && to + PAD_MS >= *a && (to.max(*b) - from.min(*a)) <= MAX_MS) {
            Some((_, a, b)) => {
                *a = (*a).min(from);
                *b = (*b).max(to);
            }
            None => out.push((video, from, to)),
        }
    }
    out
}

fn covered(pieces: &[Piece], video: &str, from: u64, to: u64, dir: &Path) -> bool {
    pieces.iter().any(|piece| piece.video == video && piece.from_ms <= from + PAD_MS && piece.to_ms + PAD_MS >= to && folder(dir).join(&piece.file).is_file())
}

/// The script's footage now; downloads what's missing in the background.
pub fn now(data: &Path, id: &str) -> Result<Footage, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let Some(commentary) = script.commentary.as_ref() else { return Ok(Footage::default()) };
    let origins: BTreeMap<String, String> = commentary.sources.iter().map(|source| (source.id.clone(), source.origin.clone())).collect();
    let kept = read(&dir);
    let missing: Vec<(String, u64, u64)> = commentary
        .draft
        .as_ref()
        .map(|draft| wanted(draft, &origins))
        .unwrap_or_default()
        .into_iter()
        .filter(|(video, from, to)| !covered(&kept.pieces, video, *from, *to, &dir) && !kept.failed.contains(&format!("{video}@{from}-{to}")))
        .collect();
    let pending = missing.len();
    // A shared script's review still lacking a piece, or its manifest, gets them put up too.
    let unpublished = script.review.is_some() && (kept.published.is_empty() || kept.pieces.iter().any(|piece| !kept.uploaded.contains(&piece.file)));
    if pending > 0 || unpublished {
        soon(data.to_path_buf(), id.to_string());
    }
    Ok(Footage { pieces: kept.pieces.into_iter().filter(|piece| folder(&dir).join(&piece.file).is_file()).collect(), dir: folder(&dir).to_string_lossy().into_owned(), pending, failed: kept.failed.len() })
}

static RUNNING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Downloads a script's missing footage on a thread of its own, once at a time per script.
pub fn soon(data: PathBuf, id: String) {
    {
        let Ok(mut running) = RUNNING.lock() else { return };
        if !running.get_or_insert_with(HashSet::new).insert(id.clone()) {
            return;
        }
    }
    std::thread::spawn(move || {
        if let Err(error) = fetch_all(&data, &id) {
            crate::log::warn("footage", format!("script {}: {error}", &id[..8.min(id.len())]));
        }
        if let Ok(mut running) = RUNNING.lock() {
            running.get_or_insert_with(HashSet::new).remove(&id);
        }
    });
}

fn fetch_all(data: &Path, id: &str) -> Result<(), String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let Some(commentary) = script.commentary.as_ref() else { return Ok(()) };
    let Some(draft) = commentary.draft.as_ref() else { return Ok(()) };
    // A shared script's review page gets what's here first, and a manifest saying what's still coming.
    let shared = script.review.is_some();
    if shared {
        if let Err(error) = publish(data, id) {
            crate::log::warn("footage", format!("{}: the review's clips couldn't be put up: {error}", crate::log::named(id, &script.title)));
        }
    }
    let origins: BTreeMap<String, String> = commentary.sources.iter().map(|source| (source.id.clone(), source.origin.clone())).collect();
    crate::tools::ensure_yt_dlp().ok();
    let began = std::time::Instant::now();
    let (mut got, mut lost) = (0, 0);
    for (video, from, to) in wanted(draft, &origins) {
        let mut kept = read(&dir);
        let tag = format!("{video}@{from}-{to}");
        if covered(&kept.pieces, &video, from, to, &dir) || kept.failed.contains(&tag) {
            continue;
        }
        match fetch(&dir, &video, from, to) {
            Ok(piece) => {
                kept.pieces.retain(|other| !(other.video == piece.video && other.from_ms >= piece.from_ms && other.to_ms <= piece.to_ms));
                kept.pieces.push(piece);
                got += 1;
                write(&dir, &kept);
                // Shared since the run began counts too.
                if shared || store::read(&dir).is_ok_and(|now| now.review.is_some()) {
                    if let Err(error) = publish(data, id) {
                        crate::log::warn("footage", format!("{}: a clip couldn't be put up for the review: {error}", crate::log::named(id, &script.title)));
                    }
                }
                continue;
            }
            Err(error) => {
                crate::log::warn("footage", format!("{}: {video} {}–{} didn't download: {error}", crate::log::named(id, &script.title), crate::commentary::clock(from), crate::commentary::clock(to)));
                kept.failed.push(tag);
                lost += 1;
            }
        }
        write(&dir, &kept);
    }
    if got + lost > 0 {
        crate::log::info("footage", format!("{}: {got} clip stretches downloaded{} in {}", crate::log::named(id, &script.title), if lost > 0 { format!(", {lost} couldn't be") } else { String::new() }, crate::log::since_start(began)));
    }
    Ok(())
}

/// One stretch of one video, as an mp4 the player plays from its start.
fn fetch(dir: &Path, video: &str, from: u64, to: u64) -> Result<Piece, String> {
    let out = folder(dir);
    fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    let file = format!("{video}-{}-{}.mp4", from / 1000, to.div_ceil(1000));
    let target = out.join(&file);
    let span = format!("*{:.1}-{:.1}", from as f64 / 1000.0, to as f64 / 1000.0);
    let url = format!("https://www.youtube.com/watch?v={video}");
    let target_text = target.to_string_lossy().into_owned();
    let ffmpeg = crate::tools::locate("ffmpeg").map(|(path, _)| path.to_string_lossy().into_owned());
    let mut args = vec!["-q", "--no-warnings", "--no-playlist", "--force-overwrites", "-f", FORMAT, "--merge-output-format", "mp4", "--download-sections", &span, "--force-keyframes-at-cuts", "-o", &target_text];
    if let Some(path) = ffmpeg.as_deref() {
        args.push("--ffmpeg-location");
        args.push(path);
    }
    args.push(&url);
    proc::run("yt-dlp", &args, Duration::from_secs(5 * 60)).map_err(|problem| problem.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("yt-dlp failed").chars().take(200).collect::<String>())?;
    if !target.is_file() {
        return Err("nothing came down".into());
    }
    let fast = faststart(&target).is_ok();
    Ok(Piece { video: video.to_string(), from_ms: from, to_ms: to, file, fast })
}

/// The file's index moved to its front (no re-encode), so a browser can seek in it before it's all down.
fn faststart(path: &Path) -> Result<(), String> {
    let tmp = path.with_extension("fast.mp4");
    proc::run("ffmpeg", &["-v", "error", "-y", "-i", &path.to_string_lossy(), "-c", "copy", "-movflags", "+faststart", &tmp.to_string_lossy()], Duration::from_secs(120))?;
    fs::rename(&tmp, path).map_err(|error| error.to_string())
}

/// Where a script's review clips live in the cloud: one folder per script, its manifest beside them.
fn clips_key(id: &str, file: &str) -> String {
    format!("writer/reviews/{id}/clips/{file}")
}

/// Puts a shared commentary's downloaded stretches under its review and
/// rewrites the manifest the review page reads (feedback.spheceo.com's
/// contract): one entry per clip the script plays, by its label and printed
/// range, "ready" with the file that covers it, else "pending" while it
/// downloads or "failed". Entries an earlier version listed stay, so its
/// page still finds them. The manifest is written after the files it names.
pub fn publish(data: &Path, id: &str) -> Result<usize, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let Some(commentary) = script.commentary.as_ref() else { return Ok(0) };
    let Some(draft) = commentary.draft.as_ref() else { return Ok(0) };
    if script.review.is_none() {
        return Ok(0);
    }
    let origins: BTreeMap<String, String> = commentary.sources.iter().map(|source| (source.id.clone(), source.origin.clone())).collect();
    let mut kept = read(&dir);
    let mut sent = 0;
    // Every piece here goes up once (made seekable first).
    for index in 0..kept.pieces.len() {
        let piece = kept.pieces[index].clone();
        let path = folder(&dir).join(&piece.file);
        if kept.uploaded.contains(&piece.file) || !path.is_file() {
            continue;
        }
        if !piece.fast && faststart(&path).is_ok() {
            kept.pieces[index].fast = true;
        }
        crate::cloud::put_file(data, id, &clips_key(id, &piece.file), &path)?;
        kept.uploaded.push(piece.file.clone());
        sent += 1;
        write(&dir, &kept);
        manifest(data, id, draft, &origins, &mut kept)?;
        write(&dir, &kept);
    }
    manifest(data, id, draft, &origins, &mut kept)?;
    write(&dir, &kept);
    if sent > 0 {
        crate::log::info("footage", format!("{}: {sent} clip stretches put up for the review", crate::log::named(id, &script.title)));
    }
    Ok(sent)
}

fn manifest(data: &Path, id: &str, draft: &Draft, origins: &BTreeMap<String, String>, kept: &mut Kept) -> Result<(), String> {
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut add = |block: &str, cite: &Cite| {
        let Some(video) = video_of(cite, origins) else { return };
        let ready = kept.pieces.iter().find(|piece| piece.video == video && piece.from_ms <= cite.start_ms && piece.to_ms >= cite.end_ms.min(piece.from_ms + MAX_MS) && kept.uploaded.contains(&piece.file));
        let failed = kept.failed.iter().filter_map(|tag| {
            let (failed_video, range) = tag.split_once('@')?;
            let (a, b) = range.split_once('-')?;
            Some((failed_video.to_string(), a.parse::<u64>().ok()?, b.parse::<u64>().ok()?))
        }).any(|(failed_video, from, to)| failed_video == video && from <= cite.start_ms && to >= cite.end_ms.min(from + MAX_MS));
        let mut entry = serde_json::json!({ "blockId": block, "label": cite.video.trim(), "startMs": cite.start_ms, "endMs": cite.end_ms });
        match ready {
            Some(piece) => {
                entry["status"] = "ready".into();
                entry["file"] = piece.file.clone().into();
                entry["fromMs"] = piece.from_ms.into();
                entry["toMs"] = piece.to_ms.into();
            }
            None => entry["status"] = (if failed { "failed" } else { "pending" }).into(),
        }
        entries.push(entry);
    };
    for cite in &draft.open {
        add("open", cite);
    }
    for paragraph in draft.sections.iter().flat_map(|section| &section.paragraphs) {
        for piece in &paragraph.pieces {
            if let crate::commentary::Piece::Clip(cite) = piece {
                add(&paragraph.id, cite);
            }
        }
    }
    // An earlier version's ready clips stay listed, unless this draft lists the same clip.
    let same = |a: &serde_json::Value, b: &serde_json::Value| a["label"].as_str().map(str::to_lowercase) == b["label"].as_str().map(str::to_lowercase) && a["startMs"] == b["startMs"] && a["endMs"] == b["endMs"];
    for old in &kept.published {
        if old["status"] == "ready" && !entries.iter().any(|entry| same(entry, old)) && old["file"].as_str().is_some_and(|file| kept.uploaded.iter().any(|up| up == file)) {
            entries.push(old.clone());
        }
    }
    let body = serde_json::json!({ "updatedAt": crate::model::now_ms(), "clips": entries });
    crate::cloud::put_bytes(data, &clips_key(id, "manifest.json"), &serde_json::to_vec(&body).map_err(|error| error.to_string())?, "application/json")?;
    kept.published = body["clips"].as_array().cloned().unwrap_or_default();
    Ok(())
}



/// A file of the script's footage, by its name only (for the dev page).
pub fn file(dir: &Path, name: &str) -> Option<PathBuf> {
    let safe = name.ends_with(".mp4") && !name.contains(['/', '\\']) && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    let path = folder(dir).join(name);
    (safe && path.is_file()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_clips_of_one_video_are_one_download() {
        let draft: Draft = serde_json::from_value(serde_json::json!({
            "open": [],
            "sections": [{ "heading": "Intro", "paragraphs": [{ "id": "p1", "pieces": [
                { "kind": "clip", "source": "S2", "video": "v", "startMs": 10_000, "endMs": 20_000, "quote": "a" },
                { "kind": "clip", "source": "S2", "video": "v", "startMs": 22_000, "endMs": 30_000, "quote": "b" },
                { "kind": "clip", "source": "S2", "video": "v", "startMs": 600_000, "endMs": 610_000, "quote": "c" },
                { "kind": "clip", "source": "S3", "video": "w", "startMs": 1_000, "endMs": 5_000, "quote": "d" }
            ] }] }],
            "checks": [], "report": "", "writtenAt": 0
        }))
        .unwrap();
        let origins: BTreeMap<String, String> = [("S2".to_string(), "https://www.youtube.com/watch?v=abcdefghijk".to_string()), ("S3".to_string(), "pasted".to_string())].into_iter().collect();
        let wanted = wanted(&draft, &origins);
        assert_eq!(wanted, vec![("abcdefghijk".to_string(), 7_000, 33_000), ("abcdefghijk".to_string(), 597_000, 613_000)]);
        assert!(file(Path::new("/tmp"), "../x.mp4").is_none());
    }
}
