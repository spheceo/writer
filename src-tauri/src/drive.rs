//! Google Drive footage, read as Repack reads it: with the Drive account its
//! media worker uses. The relay holds that account's sign-in and hands Writer
//! a one-hour access token, so nobody installing Writer needs Infisical, a
//! Google key, or curl.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::net::{self, Failed, Relay};
use crate::settings::Settings;

const API: &str = "https://www.googleapis.com/drive/v3";
const MAX_ENTRIES: usize = 1_000;
const VIDEO_EXT: &[&str] = &["mp4", "wmv", "mov", "mkv", "avi", "m4v", "webm"];
const AUDIO_EXT: &[&str] = &["mp3", "m4a", "wav", "aac", "ogg", "oga", "opus", "flac", "wma", "amr"];

#[derive(Debug, PartialEq)]
pub enum Link {
    File(String),
    Folder(String),
}

/// A drive.google.com file or folder link, as Repack accepts them.
pub fn parse(url: &str) -> Result<Link, String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("Paste a Google Drive link.")?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if host != "drive.google.com" {
        return Err("Paste a drive.google.com link to a file or a folder.".into());
    }
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let id_after = |marker: &str| {
        path.split_once(marker)
            .map(|(_, tail)| tail.split('/').next().unwrap_or("").to_string())
    };
    let ok = |id: &str| (10..=200).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if let Some(id) = id_after("folders/") {
        return if ok(&id) { Ok(Link::Folder(id)) } else { Err("That folder link is not complete.".into()) };
    }
    if let Some(id) = id_after("file/d/") {
        return if ok(&id) { Ok(Link::File(id)) } else { Err("That file link is not complete.".into()) };
    }
    let id = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("id="))
        .unwrap_or("")
        .to_string();
    if ok(&id) {
        return Ok(Link::File(id));
    }
    Err("Paste a Drive link to a video or to a folder of recordings.".into())
}

/// A Drive API call as JSON, with the refusals that matter said plainly.
fn get_json(token: &str, url: &str) -> Result<Value, String> {
    match net::agent().get(url).set("authorization", &format!("Bearer {token}")).call() {
        Ok(response) => response.into_json().map_err(|_| "Google Drive answered with something Writer can't read.".into()),
        Err(error) => Err(match Failed::from(error) {
            Failed::Status(403 | 404, _) => {
                "Drive won't share that with Repack's Drive account. Share it with that account, then try again.".into()
            }
            Failed::Status(code, body) => {
                let value: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                value["error"]["message"]
                    .as_str()
                    .map(|reason| format!("Google Drive said: {reason}"))
                    .unwrap_or_else(|| format!("Google Drive answered {code}."))
            }
            Failed::Unsent(reason) | Failed::Network(reason) => format!("Could not reach Google Drive ({reason})."),
        }),
    }
}

/// An access token for Repack's Drive account, from the relay. Good for
/// about an hour.
pub fn access_token(settings: &Settings) -> Result<String, String> {
    Relay::new(settings)?.drive_token()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recording {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub audio: bool,
    /// The length Drive reports, when it knows.
    #[serde(default)]
    pub claimed_ms: Option<u64>,
}

fn claimed(file: &Value) -> Option<u64> {
    file["videoMediaMetadata"]["durationMillis"]
        .as_str()
        .and_then(|text| text.parse().ok())
        .or_else(|| file["videoMediaMetadata"]["durationMillis"].as_u64())
}

fn kind(name: &str, mime: &str) -> Option<bool> {
    let ext = name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default();
    if mime.starts_with("video/") || VIDEO_EXT.contains(&ext.as_str()) {
        Some(false)
    } else if mime.starts_with("audio/") || AUDIO_EXT.contains(&ext.as_str()) {
        Some(true)
    } else {
        None
    }
}

const FIELDS: &str = "id,name,mimeType,size,md5Checksum,shortcutDetails,trashed,videoMediaMetadata";

/// The recordings behind a link and a title for them. A folder is one case:
/// every video and audio file in it and its subfolders, in name order.
pub fn recordings(token: &str, link: &Link) -> Result<(String, Vec<Recording>), String> {
    match link {
        Link::File(id) => {
            let file = get_json(token, &format!("{API}/files/{id}?fields={FIELDS}&supportsAllDrives=true"))?;
            let (id, mime) = match file["shortcutDetails"]["targetId"].as_str() {
                Some(target) => (target.to_string(), file["shortcutDetails"]["targetMimeType"].as_str().unwrap_or("")),
                None => (id.clone(), file["mimeType"].as_str().unwrap_or("")),
            };
            let name = file["name"].as_str().unwrap_or("Drive video").to_string();
            let audio = kind(&name, mime).ok_or("That Drive file is not a video or audio recording.")?;
            let size = file["size"].as_str().and_then(|size| size.parse().ok()).unwrap_or(0);
            Ok((stem(&name), vec![Recording { id, name, size, audio, claimed_ms: claimed(&file) }]))
        }
        Link::Folder(id) => {
            let folder = get_json(token, &format!("{API}/files/{id}?fields=name&supportsAllDrives=true"))?;
            let mut found = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let mut queue = vec![id.clone()];
            let mut entries = 0;
            while let Some(parent) = queue.pop() {
                let mut page: Option<String> = None;
                loop {
                    let mut url = format!(
                        "{API}/files?q=%27{parent}%27+in+parents+and+trashed+%3D+false&fields=nextPageToken,files({FIELDS})&pageSize=1000&includeItemsFromAllDrives=true&supportsAllDrives=true"
                    );
                    if let Some(token) = &page {
                        let token = token.replace('%', "%25").replace('+', "%2B").replace('/', "%2F").replace('=', "%3D").replace('&', "%26");
                        url.push_str(&format!("&pageToken={token}"));
                    }
                    let list = get_json(token, &url)?;
                    for file in list["files"].as_array().into_iter().flatten() {
                        entries += 1;
                        if entries > MAX_ENTRIES {
                            return Err("That folder holds more than 1,000 items.".into());
                        }
                        let name = file["name"].as_str().unwrap_or("").to_string();
                        let (id, mime) = match file["shortcutDetails"]["targetId"].as_str() {
                            Some(target) => (target, file["shortcutDetails"]["targetMimeType"].as_str().unwrap_or("")),
                            None => (file["id"].as_str().unwrap_or(""), file["mimeType"].as_str().unwrap_or("")),
                        };
                        if id.is_empty() {
                            continue;
                        }
                        if mime == "application/vnd.google-apps.folder" {
                            queue.push(id.to_string());
                            continue;
                        }
                        let Some(audio) = kind(&name, mime) else { continue };
                        let size: u64 = file["size"].as_str().and_then(|size| size.parse().ok()).unwrap_or(0);
                        // The same recording added twice plays once.
                        let fingerprint = format!("{}:{size}", file["md5Checksum"].as_str().unwrap_or(id));
                        if !seen.insert(fingerprint) {
                            continue;
                        }
                        found.push(Recording { id: id.to_string(), name, size, audio, claimed_ms: claimed(file) });
                    }
                    page = list["nextPageToken"].as_str().map(str::to_string);
                    if page.is_none() {
                        break;
                    }
                }
            }
            if found.is_empty() {
                return Err("That folder has no video or audio recordings.".into());
            }
            found.sort_by(|a, b| (a.name.to_lowercase(), &a.id).cmp(&(b.name.to_lowercase(), &b.id)));
            Ok((folder["name"].as_str().unwrap_or("Drive folder").to_string(), found))
        }
    }
}

pub fn stem(name: &str) -> String {
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    stem.chars().take(180).collect()
}

/// Streams one recording to `dest`, picking up from the bytes already there.
/// A connection that drops or stalls for two minutes is resumed, up to four
/// times. `on_bytes` gets the bytes written so far.
pub fn download(token: &str, recording: &Recording, dest: &Path, on_bytes: &dyn Fn(u64)) -> Result<(), String> {
    let url = format!("{API}/files/{}?alt=media&supportsAllDrives=true", recording.id);
    let agent = net::agent();
    let mut last_problem = String::new();
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2 * attempt));
        }
        let have = std::fs::metadata(dest).map(|meta| meta.len()).unwrap_or(0);
        if recording.size > 0 && have >= recording.size {
            break;
        }
        let mut request = agent.get(&url).set("authorization", &format!("Bearer {token}"));
        if have > 0 {
            request = request.set("range", &format!("bytes={have}-"));
        }
        let response = match request.call() {
            Ok(response) => response,
            Err(error) => match Failed::from(error) {
                Failed::Status(416, _) => break,
                Failed::Status(401 | 403 | 404, _) => {
                    return Err(format!("Drive won't let Repack's account download {}.", recording.name));
                }
                Failed::Status(code, _) => {
                    last_problem = format!("Google Drive answered {code}");
                    continue;
                }
                Failed::Unsent(reason) | Failed::Network(reason) => {
                    last_problem = reason;
                    continue;
                }
            },
        };
        // A server that ignores the range sends the whole file again.
        let resumed = response.status() == 206;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(dest)
            .map_err(|error| error.to_string())?;
        let mut written = if resumed { have } else { 0 };
        let mut reader = response.into_reader();
        let mut buf = vec![0u8; 1 << 20];
        let finished = loop {
            match reader.read(&mut buf) {
                Ok(0) => break true,
                Ok(n) => {
                    file.write_all(&buf[..n]).map_err(|error| format!("Could not save {} ({error}).", recording.name))?;
                    written += n as u64;
                    on_bytes(written);
                }
                Err(error) => {
                    last_problem = error.to_string();
                    break false;
                }
            }
        };
        if finished {
            break;
        }
    }
    let size = std::fs::metadata(dest).map(|meta| meta.len()).unwrap_or(0);
    if recording.size > 0 && size < recording.size {
        let why = if last_problem.is_empty() { String::new() } else { format!(" Last problem: {last_problem}.") };
        return Err(format!(
            "{} came down short ({} of {} MB).{why}",
            recording.name,
            size / 1_000_000,
            recording.size / 1_000_000
        ));
    }
    // Drive answers some refusals with a small page instead of media.
    if size < 4096 {
        let head = std::fs::read(dest).unwrap_or_default();
        if head.first() == Some(&b'{') || head.first() == Some(&b'<') {
            return Err(format!("Drive sent a page instead of {}.", recording.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse, Link};

    #[test]
    fn reads_drive_links() {
        assert_eq!(
            parse("https://drive.google.com/drive/folders/1MH8rAlvaFqJM6qhvRwNO6HNX_XIcjK3q?usp=sharing").unwrap(),
            Link::Folder("1MH8rAlvaFqJM6qhvRwNO6HNX_XIcjK3q".into())
        );
        assert_eq!(
            parse("https://drive.google.com/file/d/1kZfdwolRFif_EVLXghwEZEbMVTboqshC/view").unwrap(),
            Link::File("1kZfdwolRFif_EVLXghwEZEbMVTboqshC".into())
        );
        assert_eq!(
            parse("https://drive.google.com/open?id=1kZfdwolRFif_EVLXghwEZEbMVTboqshC").unwrap(),
            Link::File("1kZfdwolRFif_EVLXghwEZEbMVTboqshC".into())
        );
        assert!(parse("https://evil.example/drive/folders/1MH8rAlvaFqJM6qhvRwNO6HNX_XIcjK3q").is_err());
        assert!(parse("https://drive.google.com/drive/folders/x\"y").is_err());
    }
}

#[cfg(test)]
mod live_tests {
    /// Reads a real Repack case folder's listing, no download:
    /// cargo test -- --ignored lists_a_repack_folder --nocapture
    #[test]
    #[ignore]
    fn lists_a_repack_folder() {
        let data = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Application Support/com.spheceo.writer");
        let token = super::access_token(&crate::settings::load(&data)).unwrap();
        let link = super::parse("https://drive.google.com/drive/folders/1MH8rAlvaFqJM6qhvRwNO6HNX_XIcjK3q").unwrap();
        let (title, recordings) = super::recordings(&token, &link).unwrap();
        println!("{title}");
        for recording in &recordings {
            println!("  {} · {} MB{}", recording.name, recording.size / 1_000_000, if recording.audio { " · audio" } else { "" });
        }
        assert!(!recordings.is_empty());
    }

    /// Downloads the smallest recording, cuts the file in half, and checks
    /// the next try picks up from the half that's there:
    /// cargo test -- --ignored resumes_a_cut_download --nocapture
    #[test]
    #[ignore]
    fn resumes_a_cut_download() {
        let data = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Application Support/com.spheceo.writer");
        let token = super::access_token(&crate::settings::load(&data)).unwrap();
        let link = super::parse("https://drive.google.com/drive/folders/1MH8rAlvaFqJM6qhvRwNO6HNX_XIcjK3q").unwrap();
        let (_, recordings) = super::recordings(&token, &link).unwrap();
        let small = recordings.iter().filter(|recording| recording.size > 0).min_by_key(|recording| recording.size).unwrap();
        let dest = std::env::temp_dir().join("writer-resume-test.bin");
        let _ = std::fs::remove_file(&dest);
        super::download(&token, small, &dest, &|_| {}).unwrap();
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), small.size);
        let half = small.size / 2;
        std::fs::OpenOptions::new().write(true).open(&dest).unwrap().set_len(half).unwrap();
        let first = std::cell::Cell::new(None);
        super::download(&token, small, &dest, &|bytes| {
            if first.get().is_none() {
                first.set(Some(bytes));
            }
        })
        .unwrap();
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), small.size);
        assert!(first.get().unwrap() > half, "the second try started over instead of resuming");
        println!("resumed {} from {half} of {} bytes", small.name, small.size);
        let _ = std::fs::remove_file(&dest);
    }
}
