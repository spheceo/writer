//! A script's preview, played straight from wherever it is, as Repack plays
//! its footage: the file on this Mac when it's here, otherwise the cloud's
//! copy, a range at a time, so opening a script never waits on a download.
//!
//! The window asks `preview://localhost/<script id>` (lib.rs registers the
//! scheme) and the local API's `/api/scripts/<id>/proxy` asks the same here.
//! The cloud's address is a presigned URL from the relay, good for an hour:
//! one is kept per file and asked again well before it runs out, so a long
//! watch never hits an expired one.
//!
//! WebKit reads a video's index in pieces of a few bytes (a long
//! compilation's is 19 MB), so each answer can't be a trip to the cloud: the
//! cloud's file is read in 2 MB blocks, kept in memory (the most recent
//! BLOCKS_KEPT), and the next few are fetched alongside the one asked for,
//! so the player finds them here when it gets to them.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::{cloud, store};

/// The most an open-ended ask ("bytes=N-") gets: the player asks for the next range as it plays.
const MOST: u64 = 4 * 1024 * 1024;
/// The most a range with an end gets. WebKit asks for a whole part of the
/// video's index at once (5.5 MB of a long compilation's) and refuses the
/// video when it gets less than it asked for.
const MOST_ASKED: u64 = 32 * 1024 * 1024;
/// The cloud's file is fetched and kept in blocks of this size.
const BLOCK: u64 = 2 * 1024 * 1024;
/// Blocks fetched ahead of the one asked for, at once.
const AHEAD: u64 = 4;
/// Blocks kept in memory (96 MB), the least recently used going first.
const BLOCKS_KEPT: usize = 48;
/// A presigned URL lasts an hour; a fresh one is asked after this.
const FRESH_FOR: Duration = Duration::from_secs(40 * 60);

/// One answer to a range request.
pub struct Part {
    /// 206 for a range, 200 for the whole (small) file, 416 when the range is past the end.
    pub status: u16,
    pub total: u64,
    pub start: u64,
    pub end: u64,
    pub bytes: Vec<u8>,
}

/// The cloud key each address was made for, and when.
static URLS: Mutex<Option<HashMap<String, (String, Instant)>>> = Mutex::new(None);

fn address(data: &Path, key: &str) -> Result<String, String> {
    if let Ok(urls) = URLS.lock() {
        if let Some((url, at)) = urls.as_ref().and_then(|urls| urls.get(key)) {
            if at.elapsed() < FRESH_FOR {
                return Ok(url.clone());
            }
        }
    }
    let url = cloud::get_url(data, key)?;
    if let Ok(mut urls) = URLS.lock() {
        urls.get_or_insert_with(HashMap::new).insert(key.to_string(), (url.clone(), Instant::now()));
    }
    Ok(url)
}

fn forget(key: &str) {
    if let Ok(mut urls) = URLS.lock() {
        if let Some(urls) = urls.as_mut() {
            urls.remove(key);
        }
    }
}

/// The range asked for, as start and inclusive end within `total`: all of
/// it when it has an end (up to MOST_ASKED), else at most MOST.
fn span(range: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let (start, end) = match range {
        None => (0, total.saturating_sub(1)),
        Some(header) => crate::http::parse_range(header, total)?,
    };
    let open = range.is_none_or(|header| header.trim().ends_with('-'));
    Some((start, end.min(start + if open { MOST } else { MOST_ASKED } - 1)))
}

/// The bytes of a script's preview in `range` (a `bytes=` header), from this Mac or the cloud.
pub fn part(data: &Path, id: &str, range: Option<&str>) -> Result<Part, String> {
    let dir = store::script_dir(data, id)?;
    crate::storage::used(data, id);
    let proxy = store::proxy_path(&dir);
    if proxy.is_file() {
        let mut file = File::open(&proxy).map_err(|error| error.to_string())?;
        let total = file.metadata().map_err(|error| error.to_string())?.len();
        let Some((start, end)) = span(range, total) else { return Ok(Part { status: 416, total, start: 0, end: 0, bytes: Vec::new() }) };
        let mut bytes = vec![0; (end - start + 1) as usize];
        file.seek(SeekFrom::Start(start)).map_err(|error| error.to_string())?;
        file.read_exact(&mut bytes).map_err(|error| error.to_string())?;
        return Ok(Part { status: if range.is_some() || end + 1 < total { 206 } else { 200 }, total, start, end, bytes });
    }
    let (key, total, _) = cloud::file_key(data, id, "proxy.mp4").ok_or("This script's preview isn't in the cloud yet.")?;
    keep_here(data, id);
    let Some((start, end)) = span(range, total) else { return Ok(Part { status: 416, total, start: 0, end: 0, bytes: Vec::new() }) };
    // Answered from the blocks the range spans, each fetched once and kept;
    // those after it come at once, ahead of the player.
    let (first, last) = (start / BLOCK, end / BLOCK);
    for index in first + 1..=last + AHEAD {
        if index * BLOCK < total {
            prefetch(data, &key, total, index);
        }
    }
    let mut bytes = Vec::with_capacity((end - start + 1) as usize);
    for index in first..=last {
        let block = block(data, &key, total, index)?;
        let from = start.max(index * BLOCK) - index * BLOCK;
        let to = (end.min(index * BLOCK + BLOCK - 1) - index * BLOCK).min((block.len() as u64).saturating_sub(1));
        bytes.extend_from_slice(block.get(from as usize..=to as usize).unwrap_or_default());
    }
    let end = start + (bytes.len() as u64).saturating_sub(1);
    Ok(Part { status: 206, total, start, end, bytes })
}

/// A script being played from the cloud is brought down behind it, once
/// per run: a cut jumps around its footage, and each jump into a part not
/// yet fetched waits on the cloud. Once it's here (renamed into place
/// whole), the same address answers from the file, and every jump is instant.
fn keep_here(data: &Path, id: &str) {
    static COMING: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let Ok(mut coming) = COMING.lock() else { return };
    if !coming.get_or_insert_with(HashSet::new).insert(id.to_string()) {
        return;
    }
    let (data, id) = (data.to_path_buf(), id.to_string());
    std::thread::spawn(move || {
        if let Err(error) = crate::storage::fetch_preview(&data, &id) {
            crate::log::warn("preview", format!("{id}: couldn't bring the preview to this Mac ({error}); it plays from the cloud"));
            if let Ok(mut coming) = COMING.lock() {
                coming.get_or_insert_with(HashSet::new).remove(&id);
            }
        }
    });
}

/// The blocks kept, the order they were used in, and those on their way.
#[derive(Default)]
struct Blocks {
    kept: HashMap<(String, u64), Arc<Vec<u8>>>,
    order: VecDeque<(String, u64)>,
    coming: HashSet<(String, u64)>,
}

fn blocks() -> &'static (Mutex<Blocks>, Condvar) {
    static BLOCKS: OnceLock<(Mutex<Blocks>, Condvar)> = OnceLock::new();
    BLOCKS.get_or_init(|| (Mutex::new(Blocks::default()), Condvar::new()))
}

/// One agent for every fetch, so its connections to the cloud are kept open.
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).max_idle_connections_per_host(8).build())
}

/// Block `index` of the cloud's file at `key`: kept, on its way (waited for), or fetched now.
fn block(data: &Path, key: &str, total: u64, index: u64) -> Result<Arc<Vec<u8>>, String> {
    let name = (key.to_string(), index);
    let (lock, ready) = blocks();
    {
        let mut blocks = lock.lock().map_err(|_| "The preview's blocks are unavailable.".to_string())?;
        loop {
            if let Some(found) = blocks.kept.get(&name).cloned() {
                blocks.order.retain(|other| other != &name);
                blocks.order.push_back(name);
                return Ok(found);
            }
            if !blocks.coming.contains(&name) {
                blocks.coming.insert(name.clone());
                break;
            }
            blocks = ready.wait(blocks).map_err(|_| "The preview's blocks are unavailable.".to_string())?;
        }
    }
    let fetched = fetch_block(data, key, total, index);
    let mut blocks = lock.lock().map_err(|_| "The preview's blocks are unavailable.".to_string())?;
    blocks.coming.remove(&name);
    let result = fetched.map(|bytes| {
        let bytes = Arc::new(bytes);
        blocks.kept.insert(name.clone(), bytes.clone());
        blocks.order.push_back(name);
        while blocks.order.len() > BLOCKS_KEPT {
            if let Some(old) = blocks.order.pop_front() {
                blocks.kept.remove(&old);
            }
        }
        bytes
    });
    ready.notify_all();
    result
}

/// Fetches a block in the background, unless it's kept or on its way.
fn prefetch(data: &Path, key: &str, total: u64, index: u64) {
    let name = (key.to_string(), index);
    if let Ok(blocks) = blocks().0.lock() {
        if blocks.kept.contains_key(&name) || blocks.coming.contains(&name) {
            return;
        }
    }
    let (data, key) = (data.to_path_buf(), key.to_string());
    std::thread::spawn(move || {
        let _ = block(&data, &key, total, index);
    });
}

fn fetch_block(data: &Path, key: &str, total: u64, index: u64) -> Result<Vec<u8>, String> {
    let (start, end) = (index * BLOCK, ((index + 1) * BLOCK).min(total) - 1);
    let fetch = |url: &str| agent().get(url).set("range", &format!("bytes={start}-{end}")).call();
    let response = match fetch(&address(data, key)?) {
        Ok(response) => response,
        // An address refused (expired, or the relay's keys changed): ask once more for a new one.
        Err(ureq::Error::Status(403, _)) => {
            forget(key);
            fetch(&address(data, key)?).map_err(|error| format!("the cloud refused the preview ({error})"))?
        }
        Err(error) => return Err(format!("couldn't reach the cloud for the preview ({error})")),
    };
    let mut bytes = Vec::with_capacity((end - start + 1) as usize);
    response.into_reader().take(BLOCK).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// The answer to the window's `preview://localhost/<id>` request.
pub fn respond(data: &Path, request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let id = request.uri().path().trim_matches('/').to_string();
    let range = request.headers().get("range").and_then(|value| value.to_str().ok());
    let builder = tauri::http::Response::builder().header("accept-ranges", "bytes").header("access-control-allow-origin", "*");
    let answer = if store::id_ok(&id) { crate::can::require_id(data, &id).and_then(|_| part(data, &id, range)) } else { Err("Unknown script.".into()) };
    match answer {
        Ok(part) if part.status == 416 => builder.status(416).header("content-range", format!("bytes */{}", part.total)).body(Vec::new()),
        Ok(part) => builder
            .status(part.status)
            .header("content-type", "video/mp4")
            .header("content-range", format!("bytes {}-{}/{}", part.start, part.end, part.total))
            .header("content-length", part.bytes.len().to_string())
            .body(part.bytes),
        Err(error) => {
            crate::log::warn("preview", format!("{id}: {error}"));
            builder.status(503).header("content-type", "text/plain").body(error.into_bytes())
        }
    }
    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_the_range_asked_and_at_most_four_megabytes_of_an_open_one() {
        assert_eq!(span(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(span(Some("bytes=10-20"), 100), Some((10, 20)));
        assert_eq!(span(Some("bytes=0-"), 10 * MOST), Some((0, MOST - 1)));
        // A range with an end gets all of it, as WebKit wants.
        assert_eq!(span(Some("bytes=8304274-13833837"), 690_000_000), Some((8_304_274, 13_833_837)));
        assert_eq!(span(Some("bytes=200-"), 100), None);
        assert_eq!(span(None, 50), Some((0, 49)));
    }
}
