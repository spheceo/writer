//! The handoff to Edit: one footage registry and one transcript per
//! recording, shared, so a recording is downloaded and heard once for both
//! apps. Each recording Writer downloads goes to Edit's R2 originals and
//! Convex registry under the same source Edit names it by (`drive:<id>`,
//! `youtube:<id>`, `sha256:<hex>` for a file from disk), and its words, as
//! hearing.rs hears them, to `edit/analysis/<source>/transcript.json` in
//! Edit's own format (with the voices as Writer's watcher and naming left
//! them). Before hearing a recording, Writer reads that transcript if Edit
//! (or another Mac) heard it already. Edit's cloud_media.ts has the layout.
//!
//! With `SHARE` off, nothing is written under edit/, and what would be is
//! set out in `edit-share.json` beside the script.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::hearing::{self, Transcript};
use crate::model::Script;
use crate::{cloud, settings, store, transcribe};

/// Writes to edit/ (originals, the registry, transcripts), in the format and
/// keys Edit confirmed on 2026-10-22. Off, only `edit-share.json` is written.
pub const SHARE: bool = true;

/// A YouTube video's ID from its link.
pub fn youtube_id(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://").or_else(|| url.trim().strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.trim_start_matches("www.").trim_start_matches("m.");
    let id = match host {
        "youtu.be" => path.split(['?', '#', '/']).next()?.to_string(),
        "youtube.com" => {
            let (path, query) = path.split_once('?').unwrap_or((path, ""));
            match path.split('/').collect::<Vec<_>>().as_slice() {
                ["watch"] => query.split('&').find_map(|pair| pair.strip_prefix("v="))?.split('#').next()?.to_string(),
                ["shorts" | "live" | "embed", id, ..] => id.to_string(),
                _ => return None,
            }
        }
        _ => return None,
    };
    (id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(id)
}

/// A source's place in Edit's keys: `drive/<id>`, `youtube/<id>`, `sha256/<hex>`.
fn source_path(source: &str) -> String {
    source.replacen(':', "/", 1)
}

/// A file name as Edit's `segment` keeps it in a key: no slashes or control
/// characters, at most 120 characters (a long one keeps its extension).
fn segment(name: &str) -> String {
    let cleaned: String = name.replace(['/', '\\', ':', '\0'], "_").trim_start_matches('.').chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    let cleaned = if cleaned.is_empty() { "footage.mp4" } else { cleaned };
    if cleaned.chars().count() <= 120 {
        return cleaned.to_string();
    }
    let ext = cleaned.rsplit_once('.').map(|(_, ext)| ext).filter(|ext| ext.len() < 10).map(|ext| format!(".{ext}")).unwrap_or_default();
    format!("{}{ext}", cleaned.chars().take(110).collect::<String>().trim())
}

pub fn original_key(source: &str, name: &str) -> String {
    format!("edit/originals/{}/{}", source_path(source), segment(name))
}

pub fn transcript_key(source: &str) -> String {
    format!("edit/analysis/{}/transcript.json", source_path(source))
}

/// A file from disk, named by its content as Edit names it.
pub(crate) fn sha256_of(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// One recording to share: its own file, its source and name, and where it
/// sits on the script's clock.
pub(crate) struct Shared {
    file: PathBuf,
    source: String,
    name: String,
    offset_ms: u64,
    length_ms: u64,
    /// Its voices' mark in the script's words ("2:" in a joined case).
    tag: Option<String>,
    /// Where its hearing is kept.
    key: String,
}

/// The script's recordings that have their own file and a source.
pub(crate) fn shared(dir: &Path, script: &Script) -> Vec<Shared> {
    let recordings = transcribe::recordings(dir, script);
    let sources = crate::import::part_sources(dir, script);
    let joined = recordings.len() > 1;
    recordings
        .into_iter()
        .enumerate()
        .filter(|(_, recording)| recording.span.is_none() && recording.file.is_file())
        .filter_map(|(index, recording)| {
            let (source, name) = match sources.get(index).cloned().flatten() {
                Some(found) => found,
                None if script.source_kind == "file" || (script.source_kind == "compilation" && script.parts.get(index).and_then(|part| part.url.as_deref()).is_some_and(|url| matches!(crate::drive::parse(url), Ok(crate::drive::Link::Folder(_))))) => {
                    let name = recording.file.file_name()?.to_string_lossy().into_owned();
                    (format!("sha256:{}", sha256_of(&recording.file).ok()?), name)
                }
                None => return None,
            };
            Some(Shared { file: recording.file, source, name, offset_ms: recording.offset_ms, length_ms: recording.length_ms, tag: joined.then(|| format!("{}", index + 1)), key: recording.key })
        })
        .collect()
}

/// A recording's hearing in Edit's format (hearing.rs), if it has one.
fn heard_of(dir: &Path, recording: &Shared) -> Option<Transcript> {
    fs::read(dir.join("transcript").join(&recording.key).join("transcript.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// A recording's words in Edit's format. When the script's words are that
/// hearing's, they go as the script has them, with the voices Writer's
/// watcher and naming settled and the names given to them; a hearing made
/// after the script's words goes as heard.
fn transcript_of(dir: &Path, script: &Script, recording: &Shared) -> Transcript {
    let heard = heard_of(dir, recording).unwrap_or_default();
    let strip = |speaker: &str| -> Option<String> {
        match &recording.tag {
            Some(tag) => speaker.strip_prefix(&format!("{tag}:")).map(str::to_string),
            None => Some(speaker.to_string()),
        }
    };
    let end = recording.offset_ms + recording.length_ms;
    let words = script
        .words
        .iter()
        .filter(|word| word.start_ms >= recording.offset_ms && word.start_ms < end)
        .map(|word| hearing::Word {
            text: word.text.clone(),
            start: (word.start_ms - recording.offset_ms) as f64 / 1000.0,
            end: (word.end_ms.saturating_sub(recording.offset_ms)) as f64 / 1000.0,
            speaker: word.speaker.as_deref().and_then(strip),
        })
        .collect();
    let words: Vec<hearing::Word> = words;
    let derived = !heard.words.is_empty() && heard.words.len() == words.len() && heard.words.iter().zip(&words).all(|(a, b)| a.text == b.text);
    if !heard.words.is_empty() && !derived {
        return heard;
    }
    let speakers = script.speakers.iter().filter_map(|(label, name)| strip(label).map(|label| (label, name.clone()))).collect();
    Transcript { model: if heard.model.is_empty() { "scribe_v2".into() } else { heard.model }, language: heard.language, words, speakers, keyterms: heard.keyterms }
}

/// Puts the script's recordings and their transcripts where Edit finds them:
/// each original uploaded and registered unless the registry has it ready,
/// each transcript unless one is there. Answers what it did.
/// `hear`: a recording not yet heard Edit's way is heard first, so its words
/// can go too (an import). Without it (the backfill, a
/// restore), footage goes and words only where a hearing is already there.
pub fn share(data: &Path, id: &str, hear: bool) -> Result<String, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let recordings = shared(&dir, &script);
    let mut plan = Vec::new();
    let mut done = Vec::new();
    let name = crate::log::named(id, &script.title);
    let began = std::time::Instant::now();
    let key_of_work = format!("handoff:{id}");
    for (index, recording) in recordings.iter().enumerate() {
        crate::activity::set(&key_of_work, Some(id), "Sharing footage with Edit", &script.title, &format!("{} ({} of {})", recording.name, index + 1, recordings.len()), index as u64, recordings.len() as u64, "count");
        let size = fs::metadata(&recording.file).map(|meta| meta.len()).unwrap_or(0);
        let duration = crate::media::streams(&recording.file).map(|found| found.duration_ms as f64 / 1000.0).ok();
        let key = original_key(&recording.source, &recording.name);
        if !SHARE {
            let transcript = (!script.words.is_empty()).then(|| hearing::serialize(&transcript_of(&dir, &script, recording)));
            plan.push(json!({
                "source": recording.source,
                "file": recording.file,
                "putMedia": [{ "source": recording.source, "name": recording.name, "originalKey": key, "size": size }, { "source": recording.source, "originalReady": true, "size": size, "duration": duration }],
                "upload": { "from": recording.file, "to": key },
                "transcript": transcript.as_ref().map(|text| json!({ "to": transcript_key(&recording.source), "bytes": text.len(), "head": text.chars().take(400).collect::<String>() })),
            }));
            continue;
        }
        let record = cloud::call(data, "query", "edit:getMedia", json!({ "source": recording.source }))?;
        let record = if record["originalReady"] != true {
            let key = record["originalKey"].as_str().map(str::to_string).unwrap_or_else(|| key.clone());
            if record.is_null() {
                cloud::call(data, "mutation", "edit:putMedia", json!({ "source": recording.source, "name": recording.name, "originalKey": key, "size": size }))?;
            }
            let sending = std::time::Instant::now();
            crate::log::info("handoff", format!("{name}: uploading {} ({}, {}) to Edit's originals", recording.name, recording.source, crate::log::size(size)));
            if let Err(error) = cloud::put_file(data, id, &key, &recording.file) {
                crate::activity::end(&key_of_work);
                crate::log::error("handoff", format!("{name}: {} didn't upload: {error}", recording.name));
                return Err(error);
            }
            crate::log::info("handoff", format!("{name}: {} in Edit's originals: {} in {}", recording.name, crate::log::size(size), crate::log::since_start(sending)));
            let mut ready = json!({ "source": recording.source, "originalReady": true, "size": size });
            if let Some(duration) = duration {
                ready["duration"] = json!(duration);
            }
            let record = cloud::call(data, "mutation", "edit:putMedia", ready)?;
            done.push(format!("uploaded {}", recording.name));
            record
        } else {
            record
        };
        keep_shared(&dir, &recording.file, &recording.source, record["originalKey"].as_str().unwrap_or(&key))?;
        let at = transcript_key(&recording.source);
        if !script.words.is_empty() && cloud::head(data, &at)?.is_none() {
            // Heard as Edit hears it, with the case's names, if it wasn't yet
            // (a script transcribed before Writer heard footage Edit's way).
            let keyterms = crate::names::case_names(&dir, &script, &settings::load(data));
            let unheard = heard_of(&dir, recording).is_none_or(|heard| !hearing::covers(&heard.keyterms, &keyterms));
            if unheard && (!hear || captioned_recording(&dir, recording)) {
                continue;
            }
            if unheard {
                hearing::hear(&hearing::Recording { file: &recording.file, span: None }, &dir.join("transcript").join(&recording.key), &settings::load(data), &keyterms, &|_, _| {})?;
            }
            let text = hearing::serialize(&transcript_of(&dir, &script, recording));
            cloud::put_bytes(data, &at, text.as_bytes(), "application/json")?;
            crate::log::info("handoff", format!("{name}: words of {} shared with Edit ({})", recording.name, crate::log::size(text.len() as u64)));
            done.push(format!("shared the words of {}", recording.name));
        }
    }
    if !SHARE {
        let _ = fs::write(dir.join("edit-share.json"), serde_json::to_vec_pretty(&plan).unwrap_or_default());
        return Ok(format!("Would share {} recordings with Edit (switched off until Edit confirms).", plan.len()));
    }
    crate::activity::end(&key_of_work);
    if !done.is_empty() {
        crate::log::info("handoff", format!("{name}: shared with Edit in {}: {}", crate::log::since_start(began), done.join("; ")));
    }
    Ok(if recordings.is_empty() {
        "no footage on this Mac to share (a file from disk that's no longer there, or none at all)".into()
    } else if done.is_empty() {
        "Edit has every recording and transcript already.".into()
    } else {
        done.join("; ")
    })
}

/// A caption-first import must not silently purchase a Scribe hearing in
/// the background just to populate Edit's shared hearing cache. Edit can
/// use the same original captions; any actual hearing remains separate.
fn captioned_recording(dir: &Path, recording: &Shared) -> bool {
    crate::captions::cached(&dir.join("transcript").join(&recording.key)).is_some()
}

/// Puts the script's recordings in Edit's originals over what's there: for
/// a YouTube video downloaded again in Edit's format, replacing Writer's
/// older download of it.
pub fn replace_originals(data: &Path, id: &str) -> Result<(), String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    for recording in shared(&dir, &script) {
        let record = cloud::call(data, "query", "edit:getMedia", json!({ "source": recording.source }))?;
        let size = fs::metadata(&recording.file).map(|meta| meta.len()).unwrap_or(0);
        if record.is_null() || record["size"].as_f64().map(|known| known as u64) == Some(size) {
            continue;
        }
        let key = record["originalKey"].as_str().map(str::to_string).unwrap_or_else(|| original_key(&recording.source, &recording.name));
        cloud::call(data, "mutation", "edit:putMedia", json!({ "source": recording.source, "originalReady": false }))?;
        cloud::put_file(data, id, &key, &recording.file)?;
        let mut ready = json!({ "source": recording.source, "originalReady": true, "size": size });
        if let Some(duration) = crate::media::streams(&recording.file).ok().map(|found| found.duration_ms as f64 / 1000.0) {
            ready["duration"] = json!(duration);
        }
        cloud::call(data, "mutation", "edit:putMedia", ready)?;
    }
    Ok(())
}

/// What of the script is in Edit's originals: its file (by its path in the
/// script's folder), source and key. Writer stops syncing its own copy of
/// those (cloud.rs), and another Mac finds them there.
fn keep_shared(dir: &Path, file: &Path, source: &str, key: &str) -> Result<(), String> {
    let path = dir.join("edit-shared.json");
    let mut kept: Vec<serde_json::Value> = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let relative = file.strip_prefix(dir).map(|rest| rest.to_string_lossy().replace('\\', "/")).ok();
    kept.retain(|entry| entry["source"] != source);
    kept.push(json!({ "source": source, "key": key, "file": relative }));
    fs::write(&path, serde_json::to_vec_pretty(&kept).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    // Read the current script after the upload, preserving edits made while
    // sharing. store::write reconciles receipts on every later save too.
    if source.starts_with("sha256:") {
        let current = store::read(dir)?;
        if matches!(script_with_shared_keys(dir, &current), std::borrow::Cow::Owned(_)) { store::write(dir, &current)?; }
    }
    Ok(())
}

/// Registry keys may predate this script's local filename. Keep the actual
/// shared key in every serialized body, including a later save of an older
/// in-memory script that still held its proposed filename.
pub(crate) fn script_with_shared_keys<'a>(dir: &Path, script: &'a Script) -> std::borrow::Cow<'a, Script> {
    if !script.parts.iter().any(|part| part.media_source.as_deref().is_some_and(|source| source.starts_with("sha256:"))) { return std::borrow::Cow::Borrowed(script); }
    let entries: Vec<serde_json::Value> = fs::read(dir.join("edit-shared.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let mut changed = None;
    for (index, part) in script.parts.iter().enumerate() {
        let Some(source) = part.media_source.as_deref() else { continue };
        let Some(key) = entries.iter().find(|entry| entry["source"] == source).and_then(|entry| entry["key"].as_str()) else { continue };
        if !source.starts_with("sha256:") || !key.starts_with(&format!("edit/originals/{}/", source_path(source))) || key.split('/').any(|piece| piece == "..") || part.original_key.as_deref() == Some(key) { continue; }
        changed.get_or_insert_with(|| script.clone()).parts[index].original_key = Some(key.to_string());
    }
    changed.map(std::borrow::Cow::Owned).unwrap_or(std::borrow::Cow::Borrowed(script))
}

/// The files of a script's folder in Edit's originals, with their keys
/// (storage.rs verifies a recording is really there before deleting it).
pub fn shared_entries(dir: &Path) -> Vec<(String, String)> {
    let kept: Vec<serde_json::Value> = fs::read(dir.join("edit-shared.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    kept.iter().filter_map(|entry| Some((entry["file"].as_str()?.to_string(), entry["key"].as_str()?.to_string()))).collect()
}

/// The files of a script's folder that are in Edit's originals.
pub fn shared_files(dir: &Path) -> Vec<String> {
    let kept: Vec<serde_json::Value> = fs::read(dir.join("edit-shared.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    kept.iter().filter_map(|entry| entry["file"].as_str().map(str::to_string)).collect()
}

/// Shares the script with Edit in the background; a failure is kept for the next try.
pub fn share_soon(data: PathBuf, id: String) {
    std::thread::spawn(move || {
        if let Err(error) = share(&data, &id, true) {
            if let Ok(dir) = store::script_dir(&data, &id) {
                let _ = fs::write(dir.join("edit-share-error.txt"), error);
            }
        }
    });
}

/// A recording's transcript as Edit (or another Mac) shared it, if one is
/// there and listened for these names: heard once, for both apps.
pub fn shared_transcript(data: &Path, dir: &Path, script: &Script, key: &str, keyterms: &[String]) -> Option<Transcript> {
    if !SHARE {
        return None;
    }
    let recording = shared(dir, script).into_iter().find(|recording| recording.key == key)?;
    let bytes = cloud::get_bytes(data, &transcript_key(&recording.source)).ok()??;
    let transcript: Transcript = serde_json::from_slice(&bytes).ok()?;
    hearing::covers(&transcript.keyterms, keyterms).then_some(transcript).filter(|found| !found.words.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_only_recordings_do_not_buy_a_background_hearing_for_edit() {
        let dir = std::env::temp_dir().join(format!("writer-caption-share-{}", std::process::id()));
        let work = dir.join("transcript/part-01");
        fs::create_dir_all(&work).unwrap();
        let transcript = Transcript { model: crate::captions::MODEL.into(), words: vec![hearing::Word { text: "Hello".into(), start: 0.0, end: 0.4, speaker: Some("t0001".into()) }], ..Default::default() };
        fs::write(work.join("captions.json"), hearing::serialize(&transcript)).unwrap();
        let mut recording = Shared { file: dir.join("01.mp4"), source: "youtube:aaaaaaaaaaa".into(), name: "First case".into(), offset_ms: 0, length_ms: 1000, tag: Some("1".into()), key: "part-01".into() };
        assert!(captioned_recording(&dir, &recording));
        recording.key = "part-02".into();
        assert!(!captioned_recording(&dir, &recording));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn existing_registry_basename_wins_and_survives_later_saves_of_an_older_script() {
        let dir = std::env::temp_dir().join(format!("writer-existing-master-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = format!("sha256:{}", "a".repeat(64));
        let proposed = original_key(&source, "01.mp4");
        let existing = original_key(&source, "00.mp4");
        let script: Script = serde_json::from_value(json!({
            "id":"a", "title":"Second compilation", "sourceKind":"compilation", "sourceUrl":"", "originalPath":"", "status":"ready", "stage":"", "progress":100,
            "durationMs":1000, "revision":1, "createdAt":0, "updatedAt":0,
            "parts":[{"title":"Same folder", "startMs":0, "durationMs":1000, "mediaSource":source, "originalKey":proposed}]
        })).unwrap();
        store::write(&dir, &script).unwrap();
        let mut current = script.clone();
        current.title = "Producer's newer title".into();
        store::write(&dir, &current).unwrap();
        keep_shared(&dir, &dir.join("01.mp4"), &source, &existing).unwrap();
        let saved = store::read(&dir).unwrap();
        assert_eq!(saved.title, "Producer's newer title");
        assert_eq!(saved.parts[0].original_key.as_deref(), Some(existing.as_str()));
        store::write(&dir, &script).unwrap();
        let body: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("script.json")).unwrap()).unwrap();
        assert_eq!(body["parts"][0]["originalKey"], existing);
        assert_eq!(body["parts"][0]["mediaSource"], source);
        let _ = fs::remove_dir_all(dir);
    }

    /// Needs ffmpeg; verifies the one-new-case-plus-reuse path shares the
    /// master under exactly the identity exported to Edit, never the preview.
    #[test]
    #[ignore]
    fn folder_master_handoff_uses_the_original_and_serialized_registry_identity() {
        let dir = std::env::temp_dir().join(format!("writer-folder-handoff-{}", std::process::id()));
        fs::create_dir_all(dir.join("compilation")).unwrap();
        let master = dir.join("compilation/00.mp4");
        assert!(std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=s=320x180:d=1"]).arg(&master).status().unwrap().success());
        let source = format!("sha256:{}", sha256_of(&master).unwrap());
        let key = original_key(&source, "00.mp4");
        fs::write(dir.join("compilation/videos.json"), r#"{"videos":[{"url":"https://drive.google.com/drive/folders/1folderfolderfolder","title":"Case 1"}],"skipped":[]}"#).unwrap();
        let script: Script = serde_json::from_value(json!({
            "id":"folder", "title":"Compilation", "sourceKind":"compilation", "sourceUrl":"https://drive.google.com/drive/folders/1folderfolderfolder",
            "originalPath": dir.join("preview.mp4"), "status":"ready", "stage":"", "progress":100, "durationMs":1000,
            "revision":1, "createdAt":0, "updatedAt":0, "compilation":true,
            "parts":[{"title":"Case 1", "startMs":0, "durationMs":1000, "url":"https://drive.google.com/drive/folders/1folderfolderfolder", "mediaSource":source, "originalKey":key}],
            "reuses":[{"url":"https://docs.google.com/document/d/1docdocdocdocdoc/edit", "after":1, "scriptText":"Finished cut-in."}]
        })).unwrap();
        let files = shared(&dir, &script);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file, master);
        assert_eq!(files[0].source, source);
        assert_eq!(original_key(&files[0].source, &files[0].name), key);
        let body = serde_json::to_value(&script).unwrap();
        assert_eq!(body["parts"][0]["mediaSource"], source);
        assert_eq!(body["parts"][0]["originalKey"], key);
        assert_eq!(body["reuses"][0]["scriptText"], "Finished cut-in.");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_youtube_links() {
        assert_eq!(youtube_id("https://www.youtube.com/watch?v=OVITtAlFG6M&t=5").as_deref(), Some("OVITtAlFG6M"));
        assert_eq!(youtube_id("https://youtu.be/OVITtAlFG6M?si=x").as_deref(), Some("OVITtAlFG6M"));
        assert_eq!(youtube_id("https://youtube.com/shorts/OVITtAlFG6M").as_deref(), Some("OVITtAlFG6M"));
        assert_eq!(youtube_id("https://example.com/watch?v=OVITtAlFG6M"), None);
    }

    #[test]
    fn keys_as_edit_lays_them_out() {
        assert_eq!(original_key("drive:1AbC", "Bodycam 1.mp4"), "edit/originals/drive/1AbC/Bodycam 1.mp4");
        assert_eq!(original_key("youtube:OVITtAlFG6M", "a/b: c.mp4"), "edit/originals/youtube/OVITtAlFG6M/a_b_ c.mp4");
        assert_eq!(transcript_key("sha256:ab12"), "edit/analysis/sha256/ab12/transcript.json");
        assert!(segment(&format!("{}.mp4", "x".repeat(200))).ends_with(".mp4"));
    }
}
