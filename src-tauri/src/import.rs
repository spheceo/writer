//! Imports, built to bounce back. Every stage keeps what it finished on disk
//! (the download, each Drive recording, the preview, each transcribed piece),
//! so Retry carries on from where an import stopped. What only the producer
//! can decide (some recordings failed, a folder holds too much, nothing was
//! heard) pauses the import with a question instead of failing it, as Repack
//! does.

use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::drive::{self, Recording};
use crate::media;
use crate::model::{now_ms, Clip, Decision, Failed, ImportProgress, Offered, Part, Script, ScriptDetail};
use crate::settings::{self, Settings};
use crate::store;
use crate::transcribe;
use crate::youtube::{self, Probe, Trouble};

/// Drive recordings downloaded at once.
const DRIVE_DOWNLOADS: usize = 3;

/// Repack's limits for one case.
const MAX_FILES: usize = 60;
const MAX_FILE_MS: u64 = 8 * 3_600_000;
const MAX_FILE_BYTES: u64 = 30_000_000_000;

/// Imports run side by side, up to three, each on its own script.
const MAX_IMPORTS: usize = 3;
static JOBS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Busy {
    id: String,
}

impl Busy {
    fn start(id: String) -> Result<Self, String> {
        let mut jobs = JOBS.lock().map_err(|_| "The import lock failed.".to_string())?;
        if jobs.contains(&id) {
            return Err("This import is already running.".into());
        }
        if jobs.len() >= MAX_IMPORTS {
            return Err(format!("{MAX_IMPORTS} imports are already running. Try again when one finishes."));
        }
        jobs.push(id.clone());
        Ok(Self { id })
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        if let Ok(mut jobs) = JOBS.lock() {
            jobs.retain(|id| id != &self.id);
        }
        if let Ok(mut latest) = LATEST.lock() {
            latest.retain(|progress| progress.id != self.id);
        }
    }
}

/// Whether this script is importing right now.
pub fn importing(id: &str) -> bool {
    JOBS.lock().map(|jobs| jobs.iter().any(|job| job == id)).unwrap_or(false)
}

/// The producer's answer to a paused or failed import.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Choice {
    /// Carry on from where it stopped, fetching again whatever failed.
    Retry,
    /// Go on without the recordings that failed.
    SkipFailed,
    /// Import only these recordings of a folder that holds too much.
    Select { ids: Vec<String> },
    /// Open footage with no speech without a transcript.
    NoTranscript,
    Cancel,
}

/// The article given with footage, read before the import starts.
fn article(link: Option<&str>) -> Result<Option<crate::research::Research>, String> {
    link.map(str::trim).filter(|link| !link.is_empty()).map(crate::research::provided).transpose()
}

pub fn from_file(app: &AppHandle, data: &Path, source: &Path, link: Option<&str>) -> Result<ScriptDetail, String> {
    let source = fs::canonicalize(source).map_err(|_| "That file can't be opened.".to_string())?;
    if !source.is_file() {
        return Err("That file can't be opened.".into());
    }
    let title = source
        .file_stem()
        .map(|name| name.to_string_lossy().chars().take(180).collect())
        .unwrap_or_else(|| "Untitled".into());
    let path = source.to_string_lossy().into_owned();
    let found = article(link)?;
    start(app, data, new_script(title, "file", path.clone(), path), found)
}

pub fn from_youtube(app: &AppHandle, data: &Path, url: &str, link: Option<&str>) -> Result<ScriptDetail, String> {
    youtube::check_url(url)?;
    let found = article(link)?;
    start(app, data, new_script("YouTube video".into(), "youtube", url.trim().to_string(), String::new()), found)
}

/// A Drive file, or a folder of recordings joined in name order, as Repack
/// takes them. Downloads go straight from Google to this Mac.
pub fn from_drive(app: &AppHandle, data: &Path, url: &str, link: Option<&str>) -> Result<ScriptDetail, String> {
    drive::parse(url)?;
    let found = article(link)?;
    start(app, data, new_script("Google Drive footage".into(), "drive", url.trim().to_string(), String::new()), found)
}

/// Several videos, one case each, joined as one compilation. The title sets
/// its style: one that opens with "When" is a standard compilation; any other
/// is a hidden one, as in Repack.
/// A compilation's lines: each a video link, or "REUSE" and a link for a
/// case the channel already covered, as Repack takes them.
pub fn compilation_lines(lines: &[String]) -> Result<(Vec<String>, Vec<crate::model::Reuse>), String> {
    lines_for(lines, false)
}

/// The same lines for one Courtroom case across several videos: no reuses,
/// and two videos are enough.
fn lines_for(lines: &[String], case: bool) -> Result<(Vec<String>, Vec<crate::model::Reuse>), String> {
    let mut urls = Vec::new();
    let mut reuses = Vec::new();
    for line in lines.iter().map(|line| line.trim()).filter(|line| !line.is_empty()) {
        let (reuse, url) = match line.split_once(char::is_whitespace) {
            Some((mark, rest)) if mark.eq_ignore_ascii_case("reuse") || mark.eq_ignore_ascii_case("reuse:") => (true, rest.trim()),
            _ => (false, line),
        };
        if url.contains("drive.google.com") {
            if !matches!(drive::parse(url)?, drive::Link::File(_)) {
                return Err("In a compilation, each Drive link is one video, not a folder.".into());
            }
        } else {
            youtube::check_url(url)?;
        }
        if reuse {
            reuses.push(crate::model::Reuse { url: url.to_string(), title: String::new(), after: urls.len() });
        } else {
            urls.push(url.to_string());
        }
    }
    if case && !reuses.is_empty() {
        return Err("A reused case belongs in a compilation, not one Courtroom case.".into());
    }
    if !(2..=20).contains(&urls.len()) {
        return Err(if case {
            "A Courtroom case across videos takes 2 to 20 links; for one video, use YouTube.".into()
        } else {
            "A compilation takes 2 to 20 videos to import, one link each.".into()
        });
    }
    if reuses.len() > 20 {
        return Err("A compilation takes at most 20 reused cases.".into());
    }
    Ok((urls, reuses))
}

/// Each compilation line checked before anything downloads: what the video
/// is, or why it can't be imported. Four are asked about at once.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCheck {
    pub url: String,
    pub reuse: bool,
    #[serde(flatten)]
    pub probe: youtube::Probe,
}

pub fn check_videos(data: &Path, lines: &[String]) -> Result<Vec<VideoCheck>, String> {
    let (urls, reuses) = compilation_lines(lines)?;
    let mut wanted: Vec<(String, bool)> = Vec::new();
    let mut reuse = reuses.iter().peekable();
    for (index, url) in urls.iter().enumerate() {
        while let Some(found) = reuse.next_if(|item| item.after == index) {
            wanted.push((found.url.clone(), true));
        }
        wanted.push((url.clone(), false));
    }
    wanted.extend(reuse.map(|item| (item.url.clone(), true)));
    if wanted.iter().any(|(url, _)| !url.contains("drive.google.com")) {
        ensure_yt_dlp()?;
    }
    let settings = settings::load(data);
    let checks: Vec<std::sync::Mutex<Option<VideoCheck>>> = wanted.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for lane in 0..4 {
            let (wanted, checks, settings) = (&wanted, &checks, &settings);
            scope.spawn(move || {
                for (index, (url, reuse)) in wanted.iter().enumerate().skip(lane).step_by(4) {
                    let probe = if url.contains("drive.google.com") {
                        match drive::access_token(settings).and_then(|token| drive::recordings(&token, &drive::parse(url)?)) {
                            Ok((title, recordings)) => Probe {
                                title,
                                channel: "Google Drive".into(),
                                duration_ms: recordings.first().and_then(|recording| recording.claimed_ms),
                                problem: recordings.first().is_some_and(|recording| recording.audio).then(|| "It's an audio recording, not a video.".into()),
                                warning: None,
                            },
                            Err(error) => Probe { problem: Some(error), ..Default::default() },
                        }
                    } else {
                        youtube::probe(url)
                    };
                    if let Ok(mut slot) = checks[index].lock() {
                        *slot = Some(VideoCheck { url: url.clone(), reuse: *reuse, probe });
                    }
                }
            });
        }
    });
    Ok(checks.into_iter().filter_map(|slot| slot.into_inner().ok().flatten()).collect())
}

/// Several videos joined in order: a compilation, one case each, or (`case`)
/// one Courtroom case whose hearing runs across them, as Repack's Courtroom
/// takes a case's YouTube links. A link's `t` is kept as a hint to where the
/// case starts in that video.
pub fn from_compilation(app: &AppHandle, data: &Path, title: &str, urls: &[String], case: bool) -> Result<ScriptDetail, String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 180 {
        return Err("Give it a title (up to 180 characters).".into());
    }
    let (urls, reuses) = lines_for(urls, case)?;
    let mut script = new_script(title.to_string(), "compilation", urls.join("\n"), String::new());
    script.compilation = !case;
    if case {
        script.kind = crate::formats::Kind::Courtroom;
    }
    script.reuses = reuses;
    start(app, data, script, None)
}

fn start(app: &AppHandle, data: &Path, mut script: Script, article: Option<crate::research::Research>) -> Result<ScriptDetail, String> {
    let _busy = Busy::start(script.id.clone())?;
    let dir = store::script_dir(data, &script.id)?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    store::write(&dir, &script)?;
    // The producer's article is the case's research: no search runs.
    if let Some(found) = &article {
        crate::research::keep(&dir, found)?;
    }
    run(app, data, &dir, &mut script)
}

/// Answers a paused import, or retries a failed one, then carries on.
pub fn resume(app: &AppHandle, data: &Path, id: &str, choice: Choice) -> Result<ScriptDetail, String> {
    let _busy = Busy::start(id.to_string())?;
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    let words_missing = script.status == "ready" && script.words.is_empty() && !script.no_speech;
    if !(script.status == "error" || script.status == "needs_choice" || words_missing) {
        return Err("This import isn't waiting on anything.".into());
    }
    match (&choice, &script.decision) {
        (Choice::Cancel, _) => {
            script.decision = None;
            script.status = "error".into();
            script.error = Some("Import cancelled. Retry starts it again from what it kept.".into());
            script.updated_at = now_ms();
            store::write(&dir, &script)?;
            return Ok(store::detail(script, &dir));
        }
        (Choice::Retry, _) => {}
        (Choice::SkipFailed, Some(Decision::FailedRecordings { failed, .. })) if script.source_kind == "compilation" => {
            let failed = failed.clone();
            let mut videos = Videos::read(&dir).ok_or("This compilation has lost its list of videos. Retry it.")?;
            for item in &failed {
                if !videos.skipped.contains(&item.id) {
                    videos.skipped.push(item.id.clone());
                }
                note(&mut script, format!("Left out {}: {}", item.title, item.error));
            }
            videos.write(&dir)?;
        }
        (Choice::SkipFailed, Some(Decision::FailedRecordings { failed, .. })) => {
            let failed = failed.clone();
            let mut manifest = Manifest::read(&dir).ok_or("This import has lost its list of recordings. Retry it.")?;
            for item in &failed {
                if !manifest.skipped.contains(&item.id) {
                    manifest.skipped.push(item.id.clone());
                }
                note(&mut script, format!("Left out {}: {}", item.title, item.error));
            }
            manifest.write(&dir)?;
        }
        (Choice::Select { ids }, Some(Decision::TooMuch { .. })) => {
            let mut manifest = Manifest::read(&dir).ok_or("This import has lost its list of recordings. Retry it.")?;
            let chosen: Vec<&Recording> = manifest.recordings.iter().filter(|recording| ids.contains(&recording.id)).collect();
            if chosen.is_empty() || chosen.len() != ids.len() {
                return Err("Choose at least one recording from this folder.".into());
            }
            if let Some(reason) = over_limits(&chosen) {
                return Err(reason);
            }
            manifest.chosen = Some(ids.clone());
            manifest.write(&dir)?;
        }
        (Choice::NoTranscript, Some(Decision::NoSpeech { .. })) => script.no_speech = true,
        _ => return Err("That answer doesn't fit what this import asked.".into()),
    }
    script.decision = None;
    script.status = "processing".into();
    script.error = None;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    run(app, data, &dir, &mut script)
}

/// Retranscribing footage that has none: the same as Retry.
pub fn transcribe_existing(app: &AppHandle, data: &Path, id: &str) -> Result<ScriptDetail, String> {
    resume(app, data, id, Choice::Retry)
}

enum Step {
    Done,
    Paused,
}

/// Runs the pipeline. A failure, or even a bug, ends as a failed import that
/// says so and can be retried; it never leaves the script stuck importing.
fn run(app: &AppHandle, data: &Path, dir: &Path, script: &mut Script) -> Result<ScriptDetail, String> {
    let outcome = catch_unwind(AssertUnwindSafe(|| pipeline(app, data, dir, script)));
    match outcome {
        Ok(Ok(Step::Done)) | Ok(Ok(Step::Paused)) => Ok(store::detail(script.clone(), dir)),
        Ok(Err(error)) => fail(app, dir, script, error),
        Err(panic) => {
            let what = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|text| text.to_string()))
                .unwrap_or_else(|| "no details".into());
            fail(app, dir, script, format!("Writer hit a bug while importing ({what}). Retry: what finished is kept."))
        }
    }
}

fn pipeline(app: &AppHandle, data: &Path, dir: &Path, script: &mut Script) -> Result<Step, String> {
    let settings = settings::load(data);

    // The footage.
    let mut joined = match script.source_kind.as_str() {
        "youtube" => {
            fetch_youtube(app, data, dir, script, &settings)?;
            false
        }
        "drive" => match fetch_drive(app, dir, script, &settings)? {
            Fetched::Paused => return Ok(Step::Paused),
            Fetched::One => false,
            Fetched::Joined => true,
        },
        "compilation" => match fetch_compilation(app, data, dir, script, &settings)? {
            Fetched::Paused => return Ok(Step::Paused),
            Fetched::One => false,
            Fetched::Joined => true,
        },
        _ => {
            if !Path::new(&script.original_path).is_file() {
                return Err(format!(
                    "The video is no longer at {}. Put it back, then retry.",
                    script.original_path
                ));
            }
            false
        }
    };

    // Its length and sound.
    let source = PathBuf::from(&script.original_path);
    script.stage = "processing".into();
    script.progress = script.progress.max(20);
    emit(app, script, "Reading the footage.");
    let found = media::streams(&source).map_err(|_| "This file can't be read as video or audio.".to_string())?;
    if found.duration_ms == 0 {
        return Err("This file has no length to play.".into());
    }
    let duration = if joined { found.duration_ms } else { media::playable_ms(&source, found.duration_ms) };
    if duration > media::MAX_CASE_MS || (!joined && duration > MAX_FILE_MS) {
        return Err("This footage runs longer than Writer imports at once (eight hours a file).".into());
    }
    if duration + 2_000 < found.duration_ms {
        note(script, format!(
            "The file is cut short: it claims {} but holds {}. Writer imported what it holds.",
            clock(found.duration_ms),
            clock(duration)
        ));
    }
    script.duration_ms = duration;
    store::write(dir, script)?;
    if !found.audio && !script.no_speech {
        return pause(app, dir, script, Decision::NoSpeech {
            summary: "This video has no sound, so there is nothing to transcribe. Open it without a transcript to cut it by time, or cancel.".into(),
            reason: "no_audio".into(),
        });
    }

    // The preview. Sound with no picture plays over black.
    if !found.video && !joined {
        build_preview_from(app, dir, script, &source, found)?;
        joined = true;
    }
    if !joined {
        ensure_preview(app, dir, script, &source)?;
    }

    // The words.
    if script.no_speech {
        script.words.clear();
        if script.clips.is_empty() {
            script.clips = vec![Clip { id: uuid::Uuid::new_v4().to_string(), start_ms: 0, end_ms: script.duration_ms, ..Default::default() }];
        }
    } else {
        script.stage = "transcribing".into();
        script.progress = 60;
        emit(app, script, "Transcribing.");
        store::write(dir, script)?;
        let (id, title) = (script.id.clone(), script.title.clone());
        let heard = transcribe::transcribe(script, dir, &settings, &|done, count| {
            let percent = 60 + ((done as u32 * 30) / count.max(1) as u32) as u8;
            let detail = if count > 1 { format!("Transcribing: {done} of {count} parts done.") } else { "Transcribing.".into() };
            emit_parts(app, &id, &title, "transcribing", percent, &detail);
        });
        match heard {
            Ok(()) => {}
            Err(error) if error == transcribe::NO_WORDS => {
                return pause(app, dir, script, Decision::NoSpeech {
                    summary: "Speech to text heard no words in this footage. Open it without a transcript to cut it by time, or cancel.".into(),
                    reason: "no_words".into(),
                });
            }
            Err(error) => return Err(error),
        }
        // Who speaks each line, by watching the footage, then names for the
        // voices, so the transcript reads with them. Neither can stop the
        // import: a failure leaves the voices as heard, or "Speaker 1", and
        // the build names them again.
        let watched = watch_speakers(app, dir, script, &settings);
        if watched || script.speakers.is_empty() {
            emit(app, script, "Naming the speakers.");
            let _ = if watched { crate::speakers::complete(script) } else { crate::speakers::identify(script) };
        }
    }

    // A Drive case folder's PDFs (reports, court papers), read for the case's
    // facts, as Repack reads them. Never stops the import.
    if script.source_kind == "drive" {
        if let Ok(drive::Link::Folder(folder)) = drive::parse(&script.source_url) {
            let fresh = !crate::casefiles::load(dir).is_some_and(|documents| documents.complete);
            let (id, title) = (script.id.clone(), script.title.clone());
            let documents = crate::casefiles::run(dir, &folder, &settings, &|done, count| {
                emit_parts(app, &id, &title, "researching", 98, &format!("Reading the case's PDFs: {done} of {count}."));
            });
            if fresh && !documents.skipped.is_empty() {
                let shown: Vec<&str> = documents.skipped.iter().take(3).map(String::as_str).collect();
                let more = documents.skipped.len().saturating_sub(3);
                note(script, format!(
                    "{} of the folder's PDFs {} passed over: {}{}",
                    documents.skipped.len(),
                    if documents.skipped.len() == 1 { "was" } else { "were" },
                    shown.join("; "),
                    if more > 0 { format!("; and {more} more.") } else { String::new() }
                ));
            }
        }
    }

    script.status = "ready".into();
    script.stage = "ready".into();
    script.progress = 100;
    script.error = None;
    script.decision = None;
    script.updated_at = now_ms();
    store::write(dir, script)?;
    emit(app, script, "Ready.");
    Ok(Step::Done)
}

/// The watcher (watch.rs), right after transcription: whether it corrected
/// any of the footage. Whatever goes wrong, even a bug, is a note and the
/// transcript as heard.
fn watch_speakers(app: &AppHandle, dir: &Path, script: &mut Script, settings: &Settings) -> bool {
    script.progress = 90;
    emit(app, script, "Checking who speaks.");
    let (id, title) = (script.id.clone(), script.title.clone());
    let on_window = |done: usize, count: usize| {
        let percent = 90 + ((done * 8) / count.max(1)) as u8;
        let detail = if count > 1 { format!("Checking who speaks: {done} of {count} stretches watched.") } else { "Checking who speaks.".into() };
        emit_parts(app, &id, &title, "transcribing", percent, &detail);
    };
    let before = script.clone();
    let outcome = catch_unwind(AssertUnwindSafe(|| crate::watch::run(script, dir, settings, &on_window)))
        .unwrap_or_else(|_| Err("Writer hit a bug".into()));
    match outcome {
        Ok(None) => false,
        Ok(Some(report)) => {
            if !report.failed.is_empty() {
                note(script, format!(
                    "{} of {} stretches of footage couldn't be watched to check who speaks ({}); their speakers are as heard.",
                    report.failed.len(),
                    report.windows,
                    report.failed[0]
                ));
            }
            let _ = store::write(dir, script);
            true
        }
        Err(error) => {
            *script = before;
            note(script, format!("Writer couldn't watch the footage to check who speaks ({error}). The speakers are as heard."));
            false
        }
    }
}

/// Keeps a preview that matches the footage; otherwise makes it again.
fn ensure_preview(app: &AppHandle, dir: &Path, script: &mut Script, source: &Path) -> Result<(), String> {
    let proxy = store::proxy_path(dir);
    if preview_fits(&proxy, script.duration_ms) {
        return Ok(());
    }
    let _ = fs::remove_file(&proxy);
    script.progress = 20;
    emit(app, script, "Cutting a 480p preview.");
    let (id, title) = (script.id.clone(), script.title.clone());
    media::encode_proxy(source, &proxy, script.duration_ms, &|percent| {
        emit_parts(app, &id, &title, "processing", 20 + (u32::from(percent) * 38 / 100) as u8, "Cutting a 480p preview.");
    })
    .inspect_err(|_| {
        let _ = fs::remove_file(&proxy);
    })
}

fn preview_fits(proxy: &Path, duration_ms: u64) -> bool {
    proxy.is_file()
        && media::streams(proxy)
            .map(|found| found.video && found.duration_ms.abs_diff(duration_ms) <= 1_500)
            .unwrap_or(false)
}

/// An audio file's preview: its sound over black.
fn build_preview_from(app: &AppHandle, dir: &Path, script: &mut Script, source: &Path, found: media::Streams) -> Result<(), String> {
    let proxy = store::proxy_path(dir);
    if preview_fits(&proxy, script.duration_ms) {
        return Ok(());
    }
    emit(app, script, "Making a preview for the audio.");
    let (id, title) = (script.id.clone(), script.title.clone());
    let parts = [media::JoinPart { path: source, streams: media::Streams { duration_ms: script.duration_ms, ..found } }];
    media::join(&parts, &proxy, &|percent| {
        emit_parts(app, &id, &title, "processing", 20 + (u32::from(percent) * 38 / 100) as u8, "Making a preview for the audio.");
    })
    .inspect_err(|_| {
        let _ = fs::remove_file(&proxy);
    })
}

/// Downloads in the last hour before the proxy takes over. One or two videos
/// on this Mac's own address cost nothing; a run of them is what gets an
/// address flagged.
const BUSY_DOWNLOADS: usize = 4;

/// Notes a YouTube download and says whether this hour is a busy one.
fn busy_on_youtube(data: &Path) -> bool {
    let path = data.join("youtube-downloads.json");
    let now = now_ms();
    let mut recent: Vec<i64> = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    recent.retain(|at| now - at < 3_600_000);
    let busy = recent.len() >= BUSY_DOWNLOADS;
    recent.push(now);
    let _ = fs::write(&path, serde_json::to_vec(&recent).unwrap_or_default());
    busy
}

/// YouTube, resuming a download already under way. It goes out on this Mac's
/// own address unless this is a busy hour; a bot check or a dropped
/// connection with nothing pulled yet gets one more try, through the relay's
/// proxy on a fresh exit, as Repack does.
fn fetch_youtube(app: &AppHandle, data: &Path, dir: &Path, script: &mut Script, settings: &Settings) -> Result<(), String> {
    let dest = dir.join("original.mp4");
    if readable(&dest) {
        script.original_path = dest.to_string_lossy().into_owned();
        return Ok(());
    }
    script.stage = "downloading".into();
    let url = script.source_url.clone();
    if script.title == "YouTube video" {
        // A lookup is one small request; it never needs the proxy.
        emit(app, script, "Looking up the video.");
        ensure_yt_dlp()?;
        script.title = youtube::title(&url, None);
        store::write(dir, script)?;
    }
    let (id, title) = (script.id.clone(), script.title.clone());
    youtube_to(data, &url, &dest, settings, &|percent, said| {
        emit_parts(app, &id, &title, "downloading", 5 + (u32::from(percent) * 14 / 100) as u8, said.unwrap_or("Downloading from YouTube."));
    })?;
    script.original_path = dest.to_string_lossy().into_owned();
    store::write(dir, script)?;
    Ok(())
}

/// Writer's own yt-dlp, fetched or refreshed if it needs to be.
fn ensure_yt_dlp() -> Result<(), String> {
    if let Err(error) = crate::tools::ensure_yt_dlp() {
        if crate::tools::locate("yt-dlp").is_none() {
            return Err(format!("Writer couldn't get yt-dlp to download YouTube videos. {error}"));
        }
    }
    Ok(())
}

/// One YouTube video to `dest`, resuming a download already under way. It
/// goes out on this Mac's own address unless this is a busy hour; a bot check
/// or a dropped connection with nothing pulled yet gets one more try, through
/// the relay's proxy on a fresh exit, as Repack does. `on` hears how far the
/// download is (0 to 100) and, now and then, what is happening instead.
fn youtube_to(data: &Path, url: &str, dest: &Path, settings: &Settings, on: &dyn Fn(u8, Option<&str>)) -> Result<(), String> {
    ensure_yt_dlp()?;
    let relay_proxy = || crate::net::Relay::new(settings).and_then(|relay| relay.proxy()).ok().flatten();
    let mut proxy = if busy_on_youtube(data) { relay_proxy() } else { None };
    let folder = dest.parent().unwrap_or(data);
    on(0, None);
    let mut tried_again = false;
    loop {
        let file = proxy_file(folder, proxy.as_deref())?;
        let guard = file.as_ref().map(|path| DeleteFile(path.as_path()));
        let result = youtube::download(url, dest, guard.as_ref().map(|file| file.0), &|percent| on(percent, None));
        drop(guard);
        let error = match result {
            Ok(()) => break,
            Err(error) => error,
        };
        let proxied = proxy.is_some();
        match youtube::trouble(&error) {
            Trouble::Blocked | Trouble::Connection if !tried_again && pulled(dest) == 0 => {
                tried_again = true;
                // Blocked on this Mac's address: the proxy, when there is one.
                // Already on the proxy: a fresh exit, made by proxy_file.
                if !proxied {
                    proxy = relay_proxy();
                }
                if proxy.is_none() {
                    std::thread::sleep(Duration::from_secs(10));
                }
                on(0, Some("Trying YouTube again."));
            }
            Trouble::Blocked if proxied => {
                return Err("YouTube kept blocking the download, through the proxy too. Retry in a few minutes.".into())
            }
            Trouble::Blocked => {
                return Err("YouTube asked this Mac to prove it isn't a bot, and there's no proxy to try instead. Retry later.".into())
            }
            Trouble::Connection => return Err("The connection to YouTube kept dropping. Retry: what downloaded is kept.".into()),
            Trouble::Final(reason) => return Err(reason),
        }
    }
    if !readable(dest) {
        let _ = fs::remove_file(dest);
        return Err("The download came out unreadable, so it was dropped. Retry fetches it again.".into());
    }
    Ok(())
}

/// One video, from YouTube or a Drive file link, to `dest`: a compilation's,
/// or a reused case's brought in later. One already downloaded is kept.
pub(crate) fn fetch_video(data: &Path, url: &str, dest: &Path, settings: &Settings, on: &dyn Fn(u8, Option<&str>)) -> Result<(), String> {
    if readable(dest) {
        return Ok(());
    }
    if url.contains("drive.google.com") {
        let token = drive::access_token(settings)?;
        let (_, recordings) = drive::recordings(&token, &drive::parse(url)?)?;
        let recording = recordings.first().ok_or("That Drive link has no video.")?;
        let size = recording.size.max(1);
        return drive::download(&token, recording, dest, &|bytes| on((bytes.min(size) * 100 / size) as u8, None));
    }
    youtube_to(data, url, dest, settings, on)
}

/// Bytes of a YouTube download already on disk, finished or not.
fn pulled(dest: &Path) -> u64 {
    let stem = dest.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    fs::read_dir(dest.parent().unwrap_or(dest))
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with(&stem))
                .filter_map(|entry| entry.metadata().ok())
                .map(|meta| meta.len())
                .sum()
        })
        .unwrap_or(0)
}

pub(crate) fn readable(path: &Path) -> bool {
    path.is_file() && media::streams(path).map(|found| found.duration_ms > 0).unwrap_or(false)
}

/// A Drive case's recordings as first listed, and what the producer chose.
/// Kept beside the downloads, so a retry neither lists the folder again nor
/// asks again.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    recordings: Vec<Recording>,
    #[serde(default)]
    chosen: Option<Vec<String>>,
    #[serde(default)]
    skipped: Vec<String>,
}

impl Manifest {
    fn path(dir: &Path) -> PathBuf {
        dir.join("drive").join("manifest.json")
    }

    fn read(dir: &Path) -> Option<Self> {
        serde_json::from_slice(&fs::read(Self::path(dir)).ok()?).ok()
    }

    fn write(&self, dir: &Path) -> Result<(), String> {
        let path = Self::path(dir);
        fs::create_dir_all(path.parent().unwrap_or(dir)).map_err(|error| error.to_string())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        fs::rename(tmp, path).map_err(|error| error.to_string())
    }

    /// The recordings to import, with their place in the first listing, which
    /// names their files on disk and so never changes.
    fn wanted(&self) -> Vec<(usize, &Recording)> {
        self.recordings
            .iter()
            .enumerate()
            .filter(|(_, recording)| self.chosen.as_ref().is_none_or(|ids| ids.contains(&recording.id)))
            .filter(|(_, recording)| !self.skipped.contains(&recording.id))
            .collect()
    }
}

fn too_long(recording: &Recording) -> bool {
    recording.claimed_ms.unwrap_or(0) > MAX_FILE_MS || recording.size > MAX_FILE_BYTES
}

fn over_limits(recordings: &[&Recording]) -> Option<String> {
    let total: u64 = recordings.iter().map(|recording| recording.claimed_ms.unwrap_or(0)).sum();
    if recordings.len() > MAX_FILES {
        Some(format!("A case takes at most {MAX_FILES} recordings."))
    } else if total > media::MAX_CASE_MS {
        Some("A case takes at most twenty hours of footage.".into())
    } else if recordings.iter().any(|recording| too_long(recording)) {
        Some("A recording over eight hours or 30 GB can't be imported.".into())
    } else {
        None
    }
}

enum Fetched {
    Paused,
    One,
    Joined,
}

struct Token {
    value: String,
    at: Instant,
}

impl Token {
    /// A sign-in lasts an hour; a long folder outlasts it.
    fn get(slot: &mut Option<Token>, settings: &Settings) -> Result<String, String> {
        if slot.as_ref().is_none_or(|token| token.at.elapsed() > Duration::from_secs(45 * 60)) {
            *slot = Some(Token { value: drive::access_token(settings)?, at: Instant::now() });
        }
        Ok(slot.as_ref().map(|token| token.value.clone()).unwrap_or_default())
    }
}

fn drive_file(folder: &Path, index: usize, recording: &Recording) -> PathBuf {
    let ext = recording
        .name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| "mp4".into());
    folder.join(format!("{index:02}.{ext}"))
}

fn downloaded(path: &Path, recording: &Recording) -> bool {
    let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if recording.size > 0 {
        size == recording.size
    } else {
        size > 0
    }
}

fn fetch_drive(app: &AppHandle, dir: &Path, script: &mut Script, settings: &Settings) -> Result<Fetched, String> {
    let folder = dir.join("drive");
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let mut token: Option<Token> = None;
    script.stage = "downloading".into();

    let manifest = match Manifest::read(dir) {
        Some(manifest) => manifest,
        None => {
            emit(app, script, "Signing in to Google Drive.");
            let link = drive::parse(&script.source_url)?;
            let (title, recordings) = drive::recordings(&Token::get(&mut token, settings)?, &link)?;
            if script.title == "Google Drive footage" {
                script.title = title;
            }
            let manifest = Manifest { recordings, ..Default::default() };
            manifest.write(dir)?;
            store::write(dir, script)?;
            manifest
        }
    };

    // A joined case whose preview is already made needs nothing more.
    let proxy = store::proxy_path(dir);
    if script.parts.len() > 1 && preview_fits(&proxy, script.parts.iter().map(|part| part.duration_ms).sum()) {
        script.original_path = proxy.to_string_lossy().into_owned();
        return Ok(Fetched::Joined);
    }

    if manifest.chosen.is_none() {
        let all: Vec<&Recording> = manifest.recordings.iter().collect();
        if let Some(reason) = over_limits(&all) {
            let files = manifest
                .recordings
                .iter()
                .map(|recording| Offered {
                    id: recording.id.clone(),
                    title: recording.name.clone(),
                    size: recording.size,
                    duration_ms: recording.claimed_ms,
                    audio: recording.audio,
                    too_long: too_long(recording),
                })
                .collect();
            return pause(app, dir, script, Decision::TooMuch {
                summary: format!("This folder holds more footage than one import takes. {reason} Choose the recordings to bring in."),
                files,
                max_files: MAX_FILES,
                max_ms: media::MAX_CASE_MS,
            })
            .map(|_| Fetched::Paused);
        }
    }

    let wanted = manifest.wanted();
    if wanted.is_empty() {
        return Err("No recordings are left to import from this folder.".into());
    }

    // Room for the downloads and the joined preview.
    let needed: u64 = wanted
        .iter()
        .map(|(index, recording)| {
            let have = fs::metadata(drive_file(&folder, *index, recording)).map(|meta| meta.len()).unwrap_or(0);
            recording.size.saturating_sub(have)
        })
        .sum();
    check_space(dir, needed + needed / 5 + 1_000_000_000)?;

    // Three recordings come down at once. Each reports its own bytes; the
    // progress shown is their sum.
    let total_bytes = wanted.iter().map(|(_, recording)| recording.size).sum::<u64>().max(1);
    let count = wanted.len();
    let pulled: Vec<AtomicU64> = wanted.iter().map(|_| AtomicU64::new(0)).collect();
    let next = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let failed: Mutex<Vec<(usize, Failed)>> = Mutex::new(Vec::new());
    let token = Mutex::new(token);
    let (id, title) = (script.id.clone(), script.title.clone());
    let report = || {
        let bytes: u64 = pulled.iter().map(|value| value.load(Ordering::Relaxed)).sum();
        let percent = 5 + (bytes * 33 / total_bytes).min(33) as u8;
        let detail = if count == 1 {
            "Downloading from Google Drive.".to_string()
        } else {
            format!("Downloading recordings from Google Drive ({} of {count} done).", finished.load(Ordering::Relaxed))
        };
        emit_parts(app, &id, &title, "downloading", percent, &detail);
    };
    let fresh_token = || -> Result<String, String> {
        let mut slot = token.lock().map_err(|_| "The Drive sign-in lock failed.".to_string())?;
        Token::get(&mut slot, settings)
    };
    std::thread::scope(|scope| {
        for _ in 0..count.min(DRIVE_DOWNLOADS) {
            scope.spawn(|| loop {
                let step = next.fetch_add(1, Ordering::Relaxed);
                let Some((index, recording)) = wanted.get(step) else { break };
                let dest = drive_file(&folder, *index, recording);
                let mut problem = None;
                if !downloaded(&dest, recording) {
                    let progress = |bytes: u64| {
                        pulled[step].store(bytes.min(recording.size), Ordering::Relaxed);
                        report();
                    };
                    // One that comes down short is fetched again once, from where
                    // it stopped; short twice, it is dropped so a retry starts it clean.
                    let mut result = fresh_token().and_then(|token| drive::download(&token, recording, &dest, &progress));
                    if result.as_ref().is_err_and(|error| error.contains("came down short")) {
                        result = fresh_token().and_then(|token| drive::download(&token, recording, &dest, &progress));
                        if result.is_err() {
                            let _ = fs::remove_file(&dest);
                        }
                    }
                    problem = result.err();
                }
                if problem.is_none() {
                    let found = media::streams(&dest).ok().filter(|found| found.duration_ms > 0 && (found.video || found.audio));
                    if found.is_none() {
                        let _ = fs::remove_file(&dest);
                        problem = Some("Couldn't be read as video or audio, so it was dropped.".into());
                    }
                }
                pulled[step].store(recording.size, Ordering::Relaxed);
                finished.fetch_add(1, Ordering::Relaxed);
                report();
                if let Some(error) = problem {
                    if let Ok(mut list) = failed.lock() {
                        list.push((step, Failed { id: recording.id.clone(), title: recording.name.clone(), error }));
                    }
                }
            });
        }
    });
    let mut failed = failed.into_inner().unwrap_or_default();
    failed.sort_by_key(|(step, _)| *step);
    let failed: Vec<Failed> = failed.into_iter().map(|(_, item)| item).collect();

    if !failed.is_empty() {
        if failed.len() == wanted.len() {
            let first = &failed[0];
            return Err(format!("None of the recordings came through. {}: {}", first.title, first.error));
        }
        let ready = wanted.len() - failed.len();
        let verb = if ready == 1 { "is" } else { "are" };
        let summary = if failed.len() == 1 {
            format!("{} couldn't be imported; the other {ready} {verb} ready. Go on without it, try it again, or cancel.", failed[0].title)
        } else {
            format!("{} recordings couldn't be imported; the other {ready} {verb} ready. Go on without them, try them again, or cancel.", failed.len())
        };
        return pause(app, dir, script, Decision::FailedRecordings { summary, failed, total: wanted.len() }).map(|_| Fetched::Paused);
    }

    if wanted.len() == 1 && !wanted[0].1.audio {
        script.original_path = drive_file(&folder, wanted[0].0, wanted[0].1).to_string_lossy().into_owned();
        script.parts.clear();
        store::write(dir, script)?;
        return Ok(Fetched::One);
    }

    // Join, each recording at the length it actually holds.
    let paths: Vec<PathBuf> = wanted.iter().map(|(index, recording)| drive_file(&folder, *index, recording)).collect();
    let items: Vec<(String, PathBuf, Option<String>)> = wanted
        .iter()
        .zip(paths.iter().cloned())
        .map(|((_, recording), path)| (drive::stem(&recording.name), path, Some(drive_link(&recording.id))))
        .collect();
    join_preview(app, dir, script, &items, "recordings")?;
    // The recordings are in the joined footage now; the list stays for Retry.
    for path in &paths {
        let _ = fs::remove_file(path);
    }
    Ok(Fetched::Joined)
}

/// Joins recordings, or a compilation's videos, into one preview, each at the
/// length it actually holds, and records where each one sits.
fn join_preview(app: &AppHandle, dir: &Path, script: &mut Script, items: &[(String, PathBuf, Option<String>)], what: &str) -> Result<(), String> {
    let count = items.len();
    let mut parts = Vec::with_capacity(count);
    let mut join = Vec::with_capacity(count);
    let mut start_ms = 0;
    for (title, path, url) in items {
        let found = media::streams(path).map_err(|_| format!("Could not read {title}."))?;
        let claimed = found.duration_ms;
        let held = media::playable_ms(path, claimed);
        if held < claimed {
            note(script, format!("{title} is cut short: it claims {} but holds {}. Writer joined what it holds.", clock(claimed), clock(held)));
        }
        parts.push(Part { title: title.clone(), start_ms, duration_ms: held, audio: !found.video, claimed_ms: (held < claimed).then_some(claimed), url: url.clone(), ..Default::default() });
        start_ms += held;
        join.push(media::JoinPart { path: path.as_path(), streams: media::Streams { duration_ms: held, ..found } });
    }
    script.stage = "processing".into();
    script.progress = 40;
    let detail = format!("Joining {count} {what}.");
    emit(app, script, &detail);
    let (id, title) = (script.id.clone(), script.title.clone());
    let proxy = store::proxy_path(dir);
    media::join(&join, &proxy, &|percent| {
        emit_parts(app, &id, &title, "processing", 40 + (u32::from(percent) * 18 / 100) as u8, &detail);
    })
    .inspect_err(|_| {
        let _ = fs::remove_file(&proxy);
    })?;
    script.parts = parts;
    script.original_path = proxy.to_string_lossy().into_owned();
    store::write(dir, script)
}

/// A Drive recording's page, which plays it from a moment with `t`.
fn drive_link(id: &str) -> String {
    format!("https://drive.google.com/file/d/{id}/view")
}

/// The links of a joined script's recordings, in play order, for one joined
/// before its parts kept them: a Drive folder's chosen recordings, or a
/// compilation's videos less those left out. None when they no longer line up.
pub fn part_links(dir: &Path, script: &Script) -> Option<Vec<String>> {
    let links: Vec<String> = if script.source_kind == "compilation" {
        let videos = Videos::read(dir)?;
        videos
            .videos
            .iter()
            .enumerate()
            .filter(|(index, _)| !videos.skipped.contains(&index.to_string()))
            .map(|(_, video)| video.url.clone())
            .collect()
    } else if script.source_kind == "drive" {
        Manifest::read(dir)?.wanted().iter().map(|(_, recording)| drive_link(&recording.id)).collect()
    } else {
        return None;
    };
    (links.len() == script.parts.len()).then_some(links)
}

/// A compilation's videos, one case each, as the producer listed them.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Videos {
    videos: Vec<Video>,
    #[serde(default)]
    skipped: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Video {
    url: String,
    title: String,
}

impl Videos {
    fn path(dir: &Path) -> PathBuf {
        dir.join("compilation").join("videos.json")
    }
    fn read(dir: &Path) -> Option<Self> {
        serde_json::from_slice(&fs::read(Self::path(dir)).ok()?).ok()
    }
    fn write(&self, dir: &Path) -> Result<(), String> {
        let path = Self::path(dir);
        fs::create_dir_all(path.parent().unwrap_or(dir)).map_err(|error| error.to_string())?;
        fs::write(&path, serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?).map_err(|error| error.to_string())
    }
}

/// A compilation: several videos, each its own case, from YouTube or Drive
/// file links, joined in the order given. One that fails pauses the import
/// with the same question a Drive folder asks.
fn fetch_compilation(app: &AppHandle, data: &Path, dir: &Path, script: &mut Script, settings: &Settings) -> Result<Fetched, String> {
    let folder = dir.join("compilation");
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    script.stage = "downloading".into();
    let videos = match Videos::read(dir) {
        Some(videos) => videos,
        None => {
            emit(app, script, "Looking up the videos.");
            let mut videos = Vec::new();
            for url in script.source_url.lines().map(str::trim).filter(|url| !url.is_empty()) {
                let title = if url.contains("drive.google.com") {
                    let token = drive::access_token(settings)?;
                    let link = drive::parse(url)?;
                    drive::recordings(&token, &link).map(|(title, _)| title).unwrap_or_else(|_| "Drive video".into())
                } else {
                    ensure_yt_dlp()?;
                    youtube::title(url, None)
                };
                videos.push(Video { url: url.to_string(), title });
            }
            let videos = Videos { videos, skipped: Vec::new() };
            videos.write(dir)?;
            videos
        }
    };
    // A reused case is never downloaded; its title is what its transition is written from.
    if script.reuses.iter().any(|reuse| reuse.title.is_empty()) {
        for index in 0..script.reuses.len() {
            if !script.reuses[index].title.is_empty() {
                continue;
            }
            let url = script.reuses[index].url.clone();
            let title = if url.contains("drive.google.com") {
                drive::access_token(settings)
                    .and_then(|token| drive::recordings(&token, &drive::parse(&url)?))
                    .map(|(title, _)| title)
                    .unwrap_or_else(|_| "Reused case".into())
            } else {
                ensure_yt_dlp()?;
                youtube::title(&url, None)
            };
            script.reuses[index].title = title;
        }
        store::write(dir, script)?;
    }

    let proxy = store::proxy_path(dir);
    if script.parts.len() > 1 && preview_fits(&proxy, script.parts.iter().map(|part| part.duration_ms).sum()) {
        script.original_path = proxy.to_string_lossy().into_owned();
        return Ok(Fetched::Joined);
    }
    let wanted: Vec<(usize, &Video)> = videos
        .videos
        .iter()
        .enumerate()
        .filter(|(index, _)| !videos.skipped.contains(&index.to_string()))
        .collect();
    if wanted.len() < 2 {
        return Err(if script.compilation { "A compilation needs at least two videos." } else { "A case across videos needs at least two of them." }.into());
    }
    let count = wanted.len();
    let mut failed = Vec::new();
    let mut items = Vec::new();
    for (step, (index, video)) in wanted.iter().enumerate() {
        let dest = folder.join(format!("{index:02}.mp4"));
        let base = 5 + (step * 33 / count) as u8;
        let span = (33 / count).max(1) as u8;
        let detail = format!("Downloading video {} of {count}.", step + 1);
        let (id, title) = (script.id.clone(), script.title.clone());
        let result = fetch_video(data, &video.url, &dest, settings, &|percent, said| {
            emit_parts(app, &id, &title, "downloading", base + (u32::from(percent) * u32::from(span) / 100) as u8, said.unwrap_or(&detail));
        });
        match result.and_then(|_| if readable(&dest) { Ok(()) } else { Err("Couldn't be read as video, so it was dropped.".into()) }) {
            Ok(()) => items.push((video.title.clone(), dest, Some(video.url.clone()))),
            Err(error) => {
                if !readable(&dest) && pulled(&dest) == 0 {
                    let _ = fs::remove_file(&dest);
                }
                failed.push(Failed { id: index.to_string(), title: video.title.clone(), error });
            }
        }
    }
    if !failed.is_empty() {
        if failed.len() == count {
            let first = &failed[0];
            return Err(format!("None of the videos came through. {}: {}", first.title, first.error));
        }
        let ready = count - failed.len();
        let verb = if ready == 1 { "is" } else { "are" };
        let summary = if failed.len() == 1 {
            format!("{} couldn't be imported; the other {ready} {verb} ready. Go on without it, try it again, or cancel.", failed[0].title)
        } else {
            format!("{} videos couldn't be imported; the other {ready} {verb} ready. Go on without them, try them again, or cancel.", failed.len())
        };
        return pause(app, dir, script, Decision::FailedRecordings { summary, failed, total: count }).map(|_| Fetched::Paused);
    }
    join_preview(app, dir, script, &items, "videos")?;
    for (_, path, _) in &items {
        let _ = fs::remove_file(path);
    }
    Ok(Fetched::Joined)
}


/// Stops before a download this Mac has no room for.
fn check_space(dir: &Path, needed: u64) -> Result<(), String> {
    let Ok(text) = crate::proc::run("df", &["-k", &dir.to_string_lossy()], Duration::from_secs(10)) else {
        return Ok(());
    };
    let free_kb: Option<u64> = text.lines().nth(1).and_then(|line| line.split_whitespace().nth(3)?.parse().ok());
    match free_kb {
        Some(free) if free * 1024 < needed => Err(format!(
            "This needs about {:.1} GB free on this Mac, and {:.1} GB is free. Make room, then retry.",
            needed as f64 / 1e9,
            (free * 1024) as f64 / 1e9
        )),
        _ => Ok(()),
    }
}

fn note(script: &mut Script, text: String) {
    if !script.notes.contains(&text) {
        script.notes.push(text);
    }
}

fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds >= 3600 {
        format!("{}:{:02}:{:02}", seconds / 3600, (seconds / 60) % 60, seconds % 60)
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// Saves a question and ends the job without failing it.
fn pause(app: &AppHandle, dir: &Path, script: &mut Script, decision: Decision) -> Result<Step, String> {
    script.status = "needs_choice".into();
    script.stage = "waiting".into();
    script.error = None;
    script.decision = Some(decision);
    script.updated_at = now_ms();
    store::write(dir, script)?;
    emit(app, script, "Waiting for your choice.");
    Ok(Step::Paused)
}

fn fail(app: &AppHandle, dir: &Path, script: &mut Script, error: String) -> Result<ScriptDetail, String> {
    script.status = "error".into();
    script.error = Some(error.clone());
    script.decision = None;
    script.updated_at = now_ms();
    let _ = store::write(dir, script);
    emit(app, script, &error);
    Err(error)
}

fn new_script(title: String, kind: &str, source_url: String, original: String) -> Script {
    let now = now_ms();
    Script {
        id: uuid::Uuid::new_v4().to_string(),
        title,
        source_kind: kind.into(),
        source_url,
        original_path: original,
        status: "processing".into(),
        stage: "reading".into(),
        progress: 5,
        error: None,
        duration_ms: 0,
        words: Vec::new(),
        clips: Vec::new(),
        overview: Vec::new(),
        parts: Vec::new(),
        decision: None,
        no_speech: false,
        notes: Vec::new(),
        kind: Default::default(),
        compilation: false,
        speakers: Default::default(),
        suspects: Vec::new(),
        commentary: None,
        heading: None,
        corrections: Default::default(),
        proposals: Vec::new(),
        reuses: Vec::new(),
        doc_export: None,
        revision: 1,
        created_at: now,
        updated_at: now,
    }
}

fn emit(app: &AppHandle, script: &Script, detail: &str) {
    emit_parts(app, &script.id, &script.title, &script.stage, script.progress, detail);
}

static LATEST: Mutex<Vec<ImportProgress>> = Mutex::new(Vec::new());

/// Every import running now, so a browser can poll them.
pub fn latest_progress() -> Vec<ImportProgress> {
    LATEST.lock().map(|latest| latest.clone()).unwrap_or_default()
}

fn emit_parts(app: &AppHandle, id: &str, title: &str, stage: &str, percent: u8, detail: &str) {
    let progress = ImportProgress {
        id: id.to_string(),
        title: title.to_string(),
        stage: stage.to_string(),
        percent,
        detail: detail.to_string(),
    };
    if let Ok(mut latest) = LATEST.lock() {
        if importing(id) {
            latest.retain(|item| item.id != id);
            latest.push(progress.clone());
        }
    }
    let _ = app.emit("import-progress", progress);
}

/// A fresh sticky proxy exit for each try. The file holds the proxy's login
/// only while yt-dlp runs.
fn proxy_file(dir: &Path, proxy: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(proxy) = proxy else {
        return Ok(None);
    };
    Ok(Some(youtube::write_proxy_file(dir, &youtube::sticky_proxy(proxy))?))
}

struct DeleteFile<'a>(&'a Path);

impl Drop for DeleteFile<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{over_limits, Manifest};
    use crate::drive::Recording;

    fn recording(id: &str, hours: u64) -> Recording {
        Recording { id: id.into(), name: format!("{id}.mp4"), size: 1_000, audio: false, claimed_ms: Some(hours * 3_600_000) }
    }

    #[test]
    fn limits_follow_repack() {
        let short: Vec<Recording> = (0..3).map(|n| recording(&n.to_string(), 2)).collect();
        assert!(over_limits(&short.iter().collect::<Vec<_>>()).is_none());
        let long = [recording("a", 9)];
        assert!(over_limits(&long.iter().collect::<Vec<_>>()).unwrap().contains("eight hours"));
        let many: Vec<Recording> = (0..61).map(|n| recording(&n.to_string(), 0)).collect();
        assert!(over_limits(&many.iter().collect::<Vec<_>>()).unwrap().contains("60"));
    }

    #[test]
    fn a_retry_wants_only_what_was_chosen_and_not_skipped() {
        let manifest = Manifest {
            recordings: vec![recording("a", 1), recording("b", 1), recording("c", 1)],
            chosen: Some(vec!["a".into(), "b".into()]),
            skipped: vec!["b".into()],
        };
        let wanted: Vec<&str> = manifest.wanted().iter().map(|(_, recording)| recording.id.as_str()).collect();
        assert_eq!(wanted, ["a"]);
        assert_eq!(manifest.wanted()[0].0, 0);
    }
}
