//! A script's preview, played straight from wherever it is, as Repack plays
//! its footage: the file on this Mac when it's here, otherwise the cloud's
//! copy, a range at a time, so opening a script never waits on a download.
//!
//! The window asks `preview://localhost/<script id>` (lib.rs registers the
//! scheme) and the local API's `/api/scripts/<id>/proxy` asks the same here.
//! The cloud's address is a presigned URL from the relay, good for an hour:
//! one is kept per file and asked again well before it runs out, so a long
//! watch never hits an expired one.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::{cloud, store};

/// The most one answer carries: the player asks for the next range as it plays.
const MOST: u64 = 4 * 1024 * 1024;
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

/// The range asked for, as start and inclusive end within `total`, at most MOST long.
fn span(range: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let (start, end) = match range {
        None => (0, total.saturating_sub(1)),
        Some(header) => crate::http::parse_range(header, total)?,
    };
    Some((start, end.min(start + MOST - 1)))
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
    let Some((start, end)) = span(range, total) else { return Ok(Part { status: 416, total, start: 0, end: 0, bytes: Vec::new() }) };
    let fetch = |url: &str| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            .build()
            .get(url)
            .set("range", &format!("bytes={start}-{end}"))
            .call()
    };
    let response = match fetch(&address(data, &key)?) {
        Ok(response) => response,
        // An address refused (expired, or the relay's keys changed): ask once more for a new one.
        Err(ureq::Error::Status(403, _)) => {
            forget(&key);
            fetch(&address(data, &key)?).map_err(|error| format!("the cloud refused the preview ({error})"))?
        }
        Err(error) => return Err(format!("couldn't reach the cloud for the preview ({error})")),
    };
    let mut bytes = Vec::with_capacity((end - start + 1) as usize);
    response.into_reader().take(MOST).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    let end = start + (bytes.len() as u64).saturating_sub(1);
    Ok(Part { status: 206, total, start, end, bytes })
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
    fn answers_a_range_at_most_four_megabytes_long() {
        assert_eq!(span(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(span(Some("bytes=10-20"), 100), Some((10, 20)));
        assert_eq!(span(Some("bytes=0-"), 10 * MOST), Some((0, MOST - 1)));
        assert_eq!(span(Some("bytes=200-"), 100), None);
        assert_eq!(span(None, 50), Some((0, 49)));
    }
}
