//! Writer's log, as Edit keeps its own: `logs/writer.log` beside the
//! scripts, in local time, a line each, `2026-10-22 21:23:05.123 INFO
//! [import] …`, rotated at about 8 MB (one older file kept as writer.log.1),
//! and the last 2000 lines in memory for the Logs window (`GET /api/logs`).
//!
//! Long work tells its story in plain words, naming the script: what it
//! started on, each step with its size and time, and how it ended. Nothing
//! secret goes in: tokens, keys and the signatures of presigned URLs are cut
//! out of every line (`redact`).

use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

const KEEP: usize = 2000;
const ROTATE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    /// When, ms since the epoch: the Logs window asks for lines after one.
    pub at: i64,
    pub time: String,
    pub level: &'static str,
    pub area: String,
    pub message: String,
}

impl Line {
    fn text(&self) -> String {
        format!("{} {:<5} [{}] {}", self.time, self.level, self.area, self.message)
    }
}

struct Log {
    file: Option<PathBuf>,
    ring: VecDeque<Line>,
}

static LOG: Mutex<Log> = Mutex::new(Log { file: None, ring: VecDeque::new() });

/// Starts writing to `<data>/logs/writer.log` (lines from before go in too).
pub fn init(data: &std::path::Path) {
    let dir = data.join("logs");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("writer.log");
    let early: Vec<Line> = LOG.lock().map(|log| log.ring.iter().cloned().collect()).unwrap_or_default();
    // The last lines from before this start, so the Logs window shows them too.
    let before: Vec<Line> = fs::read_to_string(&path).map(|text| text.lines().filter_map(parse).collect()).unwrap_or_default();
    if let Ok(mut log) = LOG.lock() {
        log.file = Some(path.clone());
        let mut ring: VecDeque<Line> = before.into_iter().rev().take(KEEP - early.len().min(KEEP)).collect::<Vec<_>>().into_iter().rev().collect();
        ring.extend(early.iter().cloned());
        log.ring = ring;
    }
    for line in early {
        append(&path, &line.text());
    }
}

pub fn file() -> Option<PathBuf> {
    LOG.lock().ok().and_then(|log| log.file.clone())
}

pub fn info(area: &str, message: impl AsRef<str>) {
    write("INFO", area, message.as_ref());
}

pub fn warn(area: &str, message: impl AsRef<str>) {
    write("WARN", area, message.as_ref());
}

pub fn error(area: &str, message: impl AsRef<str>) {
    write("ERROR", area, message.as_ref());
}

fn write(level: &'static str, area: &str, message: &str) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let line = Line { at: now.as_millis() as i64, time: local_time(now), level, area: area.to_string(), message: redact(message).replace('\n', " ⏎ ") };
    let text = line.text();
    let file = {
        let Ok(mut log) = LOG.lock() else { return };
        if log.ring.len() == KEEP {
            log.ring.pop_front();
        }
        log.ring.push_back(line);
        log.file.clone()
    };
    if let Some(path) = file {
        append(&path, &text);
    }
    if cfg!(debug_assertions) {
        eprintln!("{text}");
    }
}

/// Appends a line, starting a new file when this one passes ~8 MB.
fn append(path: &std::path::Path, text: &str) {
    if fs::metadata(path).map(|meta| meta.len() > ROTATE_BYTES).unwrap_or(false) {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{text}");
    }
}

/// A line as the file has it, back into its parts.
fn parse(text: &str) -> Option<Line> {
    let (time, rest) = (text.get(..23)?, text.get(24..)?);
    let (level, rest) = rest.split_once(' ')?;
    let level = match level {
        "INFO" => "INFO",
        "WARN" => "WARN",
        "ERROR" => "ERROR",
        _ => return None,
    };
    let rest = rest.trim_start().strip_prefix('[')?;
    let (area, message) = rest.split_once("] ")?;
    let number = |range: std::ops::Range<usize>| time.get(range).and_then(|part| part.parse::<i32>().ok());
    let mut parts: libc::tm = unsafe { std::mem::zeroed() };
    parts.tm_year = number(0..4)? - 1900;
    parts.tm_mon = number(5..7)? - 1;
    parts.tm_mday = number(8..10)?;
    parts.tm_hour = number(11..13)?;
    parts.tm_min = number(14..16)?;
    parts.tm_sec = number(17..19)?;
    parts.tm_isdst = -1;
    // SAFETY: mktime reads and normalises the struct it's given.
    let seconds = unsafe { libc::mktime(&mut parts) };
    Some(Line { at: seconds as i64 * 1000 + i64::from(number(20..23)?), time: time.to_string(), level, area: area.to_string(), message: message.to_string() })
}

/// The lines after `since` (ms), oldest first.
pub fn since(since: i64) -> Vec<Line> {
    LOG.lock().map(|log| log.ring.iter().filter(|line| line.at > since).cloned().collect()).unwrap_or_default()
}

/// The time on this Mac's clock, to the millisecond.
fn local_time(now: Duration) -> String {
    let seconds = now.as_secs() as libc::time_t;
    let mut parts: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: localtime_r only writes the struct it's given.
    unsafe { libc::localtime_r(&seconds, &mut parts) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        parts.tm_year + 1900,
        parts.tm_mon + 1,
        parts.tm_mday,
        parts.tm_hour,
        parts.tm_min,
        parts.tm_sec,
        now.subsec_millis()
    )
}

/// A line with nothing secret in it: a presigned URL loses its signed query,
/// a bearer token, an API key or a token parameter is replaced.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, word) in text.split(' ').enumerate() {
        if index > 0 {
            out.push(' ');
        }
        let lower = word.to_lowercase();
        if (lower.starts_with("http://") || lower.starts_with("https://")) && word.contains('?') {
            let (base, query) = word.split_once('?').unwrap_or((word, ""));
            let secret = ["x-amz-", "signature", "token", "key=", "sig="].iter().any(|mark| query.to_lowercase().contains(mark));
            out.push_str(base);
            if secret {
                out.push_str("?…");
            } else {
                out.push('?');
                out.push_str(query);
            }
        } else if lower.starts_with("sk-") || lower.starts_with("xi-") || (word.len() >= 32 && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && word.chars().any(|c| c.is_ascii_digit()) && word.chars().any(|c| c.is_ascii_alphabetic()) && !word.contains('-')) {
            out.push_str("[secret]");
        } else {
            out.push_str(word);
        }
    }
    let lower = out.to_lowercase();
    if let Some(at) = lower.find("bearer ") {
        let end = out[at + 7..].find(' ').map_or(out.len(), |rest| at + 7 + rest);
        out.replace_range(at + 7..end, "[secret]");
    }
    out
}

/// A size as people read it: "812 KB", "1.2 GB".
pub fn size(bytes: u64) -> String {
    let bytes = bytes as f64;
    if bytes >= 1e9 {
        format!("{:.1} GB", bytes / 1e9)
    } else if bytes >= 1e6 {
        format!("{:.0} MB", bytes / 1e6)
    } else if bytes >= 1e3 {
        format!("{:.0} KB", bytes / 1e3)
    } else {
        format!("{bytes:.0} B")
    }
}

/// A time as people read it: "450 ms", "12.3 s", "2 min 10 s", "1 h 4 min".
pub fn took(elapsed: Duration) -> String {
    let ms = elapsed.as_millis();
    let seconds = elapsed.as_secs();
    if ms < 1_000 {
        format!("{ms} ms")
    } else if seconds < 60 {
        format!("{:.1} s", elapsed.as_secs_f64())
    } else if seconds < 3_600 {
        format!("{} min {} s", seconds / 60, seconds % 60)
    } else {
        format!("{} h {} min", seconds / 3_600, (seconds % 3_600) / 60)
    }
}

/// Since `start`, as people read it.
pub fn since_start(start: Instant) -> String {
    took(start.elapsed())
}

/// A script as lines name it: its title, short, and its ID's start.
pub fn named(id: &str, title: &str) -> String {
    let short: String = title.chars().take(60).collect();
    let more = if title.chars().count() > 60 { "…" } else { "" };
    format!("\"{short}{more}\" ({})", id.chars().take(8).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_secrets_out() {
        let line = redact("PUT https://acct.r2.cloudflarestorage.com/bucket/writer/x.json?X-Amz-Algorithm=AWS4&X-Amz-Signature=abc failed");
        assert_eq!(line, "PUT https://acct.r2.cloudflarestorage.com/bucket/writer/x.json?… failed");
        assert_eq!(redact("authorization: Bearer abc123def done"), "authorization: Bearer [secret] done");
        assert_eq!(redact("key sk-ant-12345 used"), "key [secret] used");
        assert_eq!(redact("https://youtu.be/abc?t=5 kept"), "https://youtu.be/abc?t=5 kept");
        // A script's ID is no secret.
        assert_eq!(redact("script 505316aa-1b19-482b-9265-6b652da3b764"), "script 505316aa-1b19-482b-9265-6b652da3b764");
    }

    #[test]
    fn says_sizes_and_times_plainly() {
        assert_eq!(size(1_234_000_000), "1.2 GB");
        assert_eq!(size(812_000), "812 KB");
        assert_eq!(took(Duration::from_millis(450)), "450 ms");
        assert_eq!(took(Duration::from_secs(130)), "2 min 10 s");
        assert_eq!(took(Duration::from_secs(3_840)), "1 h 4 min");
    }

    #[test]
    fn reads_back_a_line_from_the_file() {
        let line = parse("2026-10-22 21:40:20.462 INFO  [import] \"trooper-3min\" (06f63a36): importing").unwrap();
        assert_eq!((line.level, line.area.as_str(), line.message.as_str()), ("INFO", "import", "\"trooper-3min\" (06f63a36): importing"));
        assert_eq!(local_time(Duration::from_millis(line.at as u64)), "2026-10-22 21:40:20.462");
        assert!(parse("not a log line").is_none());
    }

    #[test]
    fn writes_local_time_to_the_millisecond() {
        let text = local_time(Duration::from_millis(1_792_700_000_123));
        assert_eq!(text.len(), 23);
        assert!(text.ends_with(".123"));
    }
}
