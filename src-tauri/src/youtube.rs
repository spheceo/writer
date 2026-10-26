use std::path::Path;
use std::time::Duration;

use crate::proc;

/// A single YouTube watch or short link. Other hosts are refused so yt-dlp
/// is not pointed at an arbitrary site.
pub fn check_url(raw: &str) -> Result<(), String> {
    let url = raw.trim();
    if url.is_empty() || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("Paste one YouTube link.".into());
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return Err("Use an https YouTube link.".into());
    };
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host_port.split(':').next().unwrap_or(host_port);
    let path = path.split('?').next().unwrap_or(path);
    let ok = match host {
        "www.youtube.com" | "youtube.com" | "m.youtube.com" => {
            path.starts_with("watch") || path.starts_with("shorts/")
        }
        "youtu.be" => !path.is_empty() && !path.contains('/'),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err("That is not a YouTube video link.".into())
    }
}

/// DataImpulse bills per exit. One attempt uses one sticky port in their range.
pub fn sticky_proxy(url: &str) -> String {
    let Some(scheme) = url.find("://") else {
        return url.to_string();
    };
    let auth_end = url.rfind('@').map(|i| i + 1).unwrap_or(scheme + 3);
    let rest = &url[auth_end..];
    let host_port = rest.split('/').next().unwrap_or(rest);
    let Some((host, _)) = host_port.rsplit_once(':') else {
        return url.to_string();
    };
    if host != "gw.dataimpulse.com" {
        return url.to_string();
    }
    let port = 10_000
        + (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
            % 10_001);
    let mut out = String::with_capacity(url.len() + 4);
    out.push_str(&url[..auth_end]);
    out.push_str(host);
    out.push(':');
    out.push_str(&port.to_string());
    out.push_str(&url[auth_end + host_port.len()..]);
    out
}

/// Titles the New script check just read, by link, so the import that
/// follows it doesn't ask YouTube again, one video at a time.
static CHECKED: std::sync::Mutex<Option<std::collections::HashMap<String, (String, std::time::Instant)>>> = std::sync::Mutex::new(None);

fn checked_title(url: &str) -> Option<String> {
    let checked = CHECKED.lock().ok()?;
    let (title, at) = checked.as_ref()?.get(url)?;
    (at.elapsed() < Duration::from_secs(3_600)).then(|| title.clone())
}

pub fn title(url: &str, proxy_file: Option<&Path>) -> String {
    if let Some(title) = checked_title(url) {
        return title;
    }
    let mut args = vec!["--no-playlist", "--skip-download", "--print", "%(title)s"];
    let config = proxy_file.map(|path| path.to_string_lossy().into_owned());
    if config.is_some() {
        args.extend(["--config-locations"]);
    }
    let config_ref = config.as_deref();
    if let Some(path) = config_ref {
        args.push(path);
    }
    args.push(url);
    proc::run("yt-dlp", &args, Duration::from_secs(60))
        .ok()
        .and_then(|text| {
            let line = text.lines().find(|line| !line.trim().is_empty())?.trim();
            let line = line.replace(['\n', '\r'], " ");
            if line.is_empty() {
                None
            } else {
                Some(line.chars().take(180).collect())
            }
        })
        .unwrap_or_else(|| "YouTube video".into())
}

/// What a compilation's video is, checked before anything downloads, as
/// Repack checks its videos: its title, channel and length, or why it can't
/// be imported (private or gone, live or upcoming, age-restricted, no
/// length, over 10 hours).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub title: String,
    pub channel: String,
    pub duration_ms: Option<u64>,
    pub problem: Option<String>,
    /// Something that couldn't be checked but may still import (through the proxy).
    pub warning: Option<String>,
}

pub fn probe(url: &str) -> Probe {
    let said = match proc::run("yt-dlp", &["--no-playlist", "--skip-download", "--dump-single-json", "--no-warnings", url], Duration::from_secs(90)) {
        Ok(text) => text,
        Err(error) => {
            let (said, blocking) = probe_trouble(&error);
            return if blocking { Probe { problem: Some(said), ..Default::default() } } else { Probe { warning: Some(said), ..Default::default() } };
        }
    };
    let Ok(info) = serde_json::from_str::<serde_json::Value>(said.trim()) else {
        return Probe { problem: Some("YouTube didn't say what this video is.".into()), ..Default::default() };
    };
    let title: String = info["title"].as_str().unwrap_or("YouTube video").chars().take(180).collect();
    if info["title"].is_string() {
        if let Ok(mut checked) = CHECKED.lock() {
            let checked = checked.get_or_insert_with(Default::default);
            checked.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(3_600));
            checked.insert(url.to_string(), (title.clone(), std::time::Instant::now()));
        }
    }
    let channel = info["channel"].as_str().or(info["uploader"].as_str()).unwrap_or("").to_string();
    let duration_ms = info["duration"].as_f64().filter(|seconds| *seconds > 0.0).map(|seconds| (seconds * 1000.0) as u64);
    let live = info["live_status"].as_str().unwrap_or("");
    let problem = if info["is_live"].as_bool() == Some(true) || live == "is_live" {
        Some("It's live now; import it once the stream has ended.".to_string())
    } else if live == "is_upcoming" {
        Some("It hasn't premiered yet.".to_string())
    } else if info["age_limit"].as_u64().unwrap_or(0) >= 18 {
        Some("It's age-restricted, so it can't be downloaded.".to_string())
    } else if duration_ms.is_none() {
        Some("YouTube doesn't give its length.".to_string())
    } else if duration_ms.is_some_and(|ms| ms > 10 * 3_600_000) {
        Some("It runs over 10 hours.".to_string())
    } else {
        None
    };
    Probe { title, channel, duration_ms, problem, warning: None }
}

/// yt-dlp's failure as a reason a person can act on, and whether it stops
/// the import (a sign-in or connection trouble may not: the download can go
/// through the proxy).
fn probe_trouble(error: &str) -> (String, bool) {
    let lower = error.to_lowercase();
    if lower.contains("private video") {
        ("It's private.".into(), true)
    } else if lower.contains("confirm your age") || lower.contains("age-restricted") || lower.contains("inappropriate") {
        ("It's age-restricted, so it can't be downloaded.".into(), true)
    } else if lower.contains("not a bot") || lower.contains("sign in") {
        ("YouTube asked this Mac to sign in, so it couldn't be checked. It should still import through the proxy.".into(), false)
    } else if lower.contains("unavailable") || lower.contains("removed") || lower.contains("does not exist") || lower.contains("terminated") {
        ("It's no longer available.".into(), true)
    } else if lower.contains("premiere") || lower.contains("live event") {
        ("It hasn't premiered yet.".into(), true)
    } else {
        ("YouTube couldn't be asked about it, so it wasn't checked. Check the link opens.".into(), false)
    }
}

/// The video Edit downloads (its youtube.ts FORMAT): H.264 up to 1080p over HTTPS with M4A sound.
/// The last resort still asks for a video: a route YouTube is withholding
/// formats from offers only storyboards, and `b` alone would take one and
/// call it a download.
pub const FORMAT: &str = "bv*[vcodec^=avc1][height<=1080][protocol=https]+ba[ext=m4a][protocol=https]/b[vcodec^=avc1][ext=mp4]/bv*[height<=1080]+ba/b[vcodec!=none]";
/// The same through the paid proxy, at 720p: about half the bytes.
pub const FORMAT_PROXIED: &str = "bv*[vcodec^=avc1][height<=720][protocol=https]+ba[ext=m4a][protocol=https]/b[vcodec^=avc1][ext=mp4][height<=720]/bv*[height<=720]+ba/b[height<=720][vcodec!=none]";

pub fn download(
    url: &str,
    dest: &Path,
    proxy_file: Option<&Path>,
    cookies: Option<&Path>,
    on_percent: &dyn Fn(u8),
) -> Result<(), String> {
    let dest_text = dest.to_string_lossy().to_string();
    let mut args = vec![
        "--no-playlist",
        "--no-progress",
        "--no-warnings",
        "--newline",
        // A dropped transfer is picked up again from the .part file.
        "--retries",
        "3",
        "--fragment-retries",
        "5",
        "--continue",
        "--socket-timeout",
        "30",
        // Edit's format, so both apps download the same file: it is shared
        // under the video's ID (edit/originals/youtube/<id>/).
        "-f",
        if proxy_file.is_some() { FORMAT_PROXIED } else { FORMAT },
        "--merge-output-format",
        "mp4",
        "-o",
        &dest_text,
    ];
    let config = proxy_file.map(|path| path.to_string_lossy().into_owned());
    if let Some(path) = config.as_deref() {
        args.push("--config-locations");
        args.push(path);
    }
    // The person's YouTube sign-in as a cookies file (youtube::sign_in made it):
    // read from disk, never from the browser, so nothing asks for the Keychain.
    let jar = cookies.map(|path| path.to_string_lossy().into_owned());
    if let Some(path) = jar.as_deref() {
        args.push("--cookies");
        args.push(path);
    }
    args.push(url);
    proc::run_lines(
        "yt-dlp",
        &args,
        // A stall is caught by --socket-timeout; this is only for a run that
        // never ends. A 9-hour hearing through the proxy took over 25 minutes.
        Duration::from_secs(3 * 60 * 60),
        None,
        |line| {
            let rest = line.trim().strip_prefix("[download]").unwrap_or("");
            let rest = rest.trim();
            if let Some(number) = rest.split('%').next() {
                if let Ok(percent) = number.trim().parse::<f32>() {
                    on_percent(percent.clamp(0.0, 100.0) as u8);
                }
            }
        },
    )
    .map_err(|error| {
        if error.contains("failed to start") {
            "Install yt-dlp to import a YouTube link (brew install yt-dlp).".into()
        } else {
            error
        }
    })?;
    if !dest.is_file() {
        // yt-dlp said yes and made nothing at `dest`: say what it did make,
        // so the log names the route's answer instead of guessing later.
        let made = dest
            .parent()
            .and_then(|dir| std::fs::read_dir(dir).ok())
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| format!("{} ({})", entry.file_name().to_string_lossy(), entry.metadata().map(|meta| meta.len()).unwrap_or(0)))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        crate::log::warn("import", format!("YouTube {url}: yt-dlp finished but {} is missing; the folder holds: {made}", dest.display()));
        return Err("The download did not produce a video.".into());
    }
    Ok(())
}

/// What went wrong with a YouTube download, for deciding whether one more
/// try on a fresh proxy exit can help.
#[derive(Debug, PartialEq)]
pub enum Trouble {
    /// YouTube asked for a bot check, or refused this address.
    Blocked,
    /// The connection itself failed (handshake, reset, timeout).
    Connection,
    /// Trying again won't help; the message says why.
    Final(String),
}

pub fn trouble(error: &str) -> Trouble {
    let lower = error.to_lowercase();
    let stale = if lower.contains("older than 90 days") { " yt-dlp on this Mac is out of date too: brew upgrade yt-dlp." } else { "" };
    if lower.contains("private video") {
        return Trouble::Final("That video is private.".into());
    }
    if lower.contains("members-only") || lower.contains("join this channel") {
        return Trouble::Final("That video is for channel members only.".into());
    }
    if lower.contains("confirm your age") || lower.contains("age-restricted") {
        return Trouble::Final("That video is age-restricted, and YouTube wants a signed-in account for it.".into());
    }
    if lower.contains("requested format is not available") {
        return Trouble::Blocked;
    }
    if lower.contains("video unavailable") || lower.contains("has been removed") || lower.contains("not available") {
        return Trouble::Final(format!("YouTube says that video is unavailable.{stale}"));
    }
    if lower.contains("install yt-dlp") {
        return Trouble::Final(error.to_string());
    }
    if lower.contains("not a bot") || lower.contains("sign in to confirm") || lower.contains("http error 403") || lower.contains("http error 429") {
        return Trouble::Blocked;
    }
    // This route gave no video (formats withheld, or yt-dlp finished with
    // nothing usable): another route — a fresh proxy exit, the sign-in, this
    // Mac — may still get one, so it is never the final word on its own.
    if lower.contains("did not produce a video") || lower.contains("requested format is not available") || lower.contains("came out unreadable") || lower.contains("only images are available") || lower.contains("no supported javascript runtime") || lower.contains("javascript runtime could be found") {
        return Trouble::Blocked;
    }
    if lower.contains("timed out") || lower.contains("connection") || lower.contains("ssl") || lower.contains("unable to download") {
        return Trouble::Connection;
    }
    // Say the ERROR, not a WARNING that happened to come first in yt-dlp's output.
    let said: String = error.lines().rev().find(|line| line.to_uppercase().contains("ERROR")).unwrap_or(error).chars().take(300).collect();
    Trouble::Final(format!("The YouTube download failed: {said}{stale}"))
}

/// Write yt-dlp's proxy line. Caller deletes the file; it holds the password.
pub fn write_proxy_file(dir: &Path, proxy: &str) -> Result<std::path::PathBuf, String> {
    if proxy.chars().any(|c| c.is_control()) || proxy.contains('"') {
        return Err("The proxy URL has a character yt-dlp's config can't take.".into());
    }
    // Downloads of different videos share a folder and may run together.
    // Each owns its config until yt-dlp exits and its guard removes it.
    let path = dir.join(format!("proxy-{}.conf", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|error| error.to_string())?;
    use std::io::Write;
    // A yt-dlp config file holds command-line options, one set per line.
    writeln!(file, "--proxy \"{proxy}\"").map_err(|error| error.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_watch_and_short_links_only() {
        assert!(check_url("https://www.youtube.com/watch?v=abc").is_ok());
        assert!(check_url("https://youtu.be/abc").is_ok());
        assert!(check_url("https://www.youtube.com/shorts/abc").is_ok());
        assert!(check_url("https://vimeo.com/1").is_err());
        assert!(check_url("https://www.youtube.com/playlist?list=1").is_err());
    }

    #[test]
    fn sorts_youtube_trouble() {
        assert_eq!(trouble("ERROR: Sign in to confirm you're not a bot"), Trouble::Blocked);
        assert_eq!(trouble("ERROR: unable to download video data: HTTP Error 403: Forbidden"), Trouble::Blocked);
        assert_eq!(trouble("ERROR: The read operation timed out"), Trouble::Connection);
        assert!(matches!(trouble("ERROR: Private video. Sign in"), Trouble::Final(text) if text.contains("private")));
        assert!(matches!(trouble("WARNING: older than 90 days ERROR: Video unavailable"), Trouble::Final(text) if text.contains("brew upgrade")));
    }

    #[test]
    fn a_route_that_gives_no_video_is_retried_not_final() {
        // The first (proxy) route withholding a video must fall through to
        // the next route instead of ending the import on the first try.
        assert_eq!(trouble("The download did not produce a video."), Trouble::Blocked);
        assert_eq!(trouble("ERROR: [youtube] abc: Requested format is not available. Use --list-formats for a list of available formats"), Trouble::Blocked);
        assert_eq!(trouble("The download came out unreadable, so it was dropped."), Trouble::Blocked);
        assert_eq!(trouble("ERROR: Only images are available for download. use --list-formats to see them"), Trouble::Blocked);
        assert_eq!(trouble("yt-dlp failed: WARNING: [youtube] No supported JavaScript runtime could be found. Only deno is enabled by default"), Trouble::Blocked);
        // A final message names the ERROR line, not a WARNING before it.
        assert!(matches!(trouble("WARNING: [youtube] No supported JavaScript runtime could be found\nERROR: [youtube] abc: Private video"), Trouble::Final(text) if text.contains("Private") || text.contains("private")));
        assert!(matches!(trouble("WARNING: something deprecated\nERROR: [youtube] abc: This video is no longer available"), Trouble::Final(text) if text.contains("no longer available") && !text.starts_with("The YouTube download failed: WARNING")));
    }

    #[test]
    fn formats_never_settle_for_a_storyboard() {
        for format in [FORMAT, FORMAT_PROXIED] {
            assert!(format.ends_with("[vcodec!=none]"), "{format}");
            assert!(!format.ends_with("/b"), "{format}");
        }
    }

    #[test]
    fn writes_the_proxy_as_yt_dlp_reads_it() {
        let dir = std::env::temp_dir().join(format!("writer-proxy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_proxy_file(&dir, "http://user;ttl.120:secret@gw.dataimpulse.com:10001").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "--proxy \"http://user;ttl.120:secret@gw.dataimpulse.com:10001\"\n");
        let other = write_proxy_file(&dir, "http://other:example@gw.dataimpulse.com:10002").unwrap();
        assert_ne!(path, other);
        std::fs::remove_file(other).unwrap();
        assert!(path.is_file(), "finishing one download must not remove another's proxy config");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dataimpulse_gets_one_sticky_port() {
        let out = sticky_proxy("http://user:secret@gw.dataimpulse.com:823");
        assert!(out.starts_with("http://user:secret@gw.dataimpulse.com:"));
        let port: u16 = out.rsplit(':').next().unwrap().parse().unwrap();
        assert!((10_000..20_001).contains(&port));
        let other = "http://user:secret@proxy.example:823";
        assert_eq!(sticky_proxy(other), other);
    }
}

/// Where the person's YouTube sign-in is kept, as a cookies file yt-dlp reads
/// with `--cookies`: made once by `sign_in`, then read silently by downloads.
pub fn cookies_file(data: &Path) -> std::path::PathBuf {
    data.join("youtube-cookies.txt")
}

/// The person's YouTube sign-in, copied once from their browser into
/// `cookies_file`, when they ask (Setup). Firefox's cookies are read without
/// any prompt; Chrome, Brave and Edge keep theirs encrypted with a Keychain
/// key, so macOS asks once, now, and never again: downloads only ever read
/// the file. Answers which browser it came from.
pub fn sign_in(data: &Path) -> Result<String, String> {
    let file = cookies_file(data);
    let tmp = file.with_extension("txt.new");
    let mut tried: Vec<(&str, Reading)> = Vec::new();
    for browser in ["firefox", "chrome", "brave", "edge"] {
        let _ = std::fs::remove_file(&tmp);
        let tmp_text = tmp.to_string_lossy().into_owned();
        let ran = proc::run(
            "yt-dlp",
            &["-q", "--no-warnings", "--skip-download", "--no-playlist", "--cookies-from-browser", browser, "--cookies", &tmp_text, "https://www.youtube.com/watch?v=jNQXAC9IVRw"],
            Duration::from_secs(120),
        );
        let signed_in = std::fs::read_to_string(&tmp).is_ok_and(|text| text.contains(".youtube.com") && (text.contains("SAPISID") || text.contains("LOGIN_INFO") || text.contains("__Secure-3PSID")));
        if signed_in {
            std::fs::rename(&tmp, &file).map_err(|error| error.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
            }
            return Ok(browser_name(browser).to_string());
        }
        let error = ran.err().unwrap_or_default();
        crate::log::info("youtube", format!("no YouTube sign-in from {browser}: {}", error.lines().last().unwrap_or("signed out of YouTube")));
        tried.push((browser, reading(&error)));
    }
    let _ = std::fs::remove_file(&tmp);
    Err(sign_in_problem(&tried))
}

/// What reading one browser's sign-in came to, when it didn't give one.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Reading {
    /// The browser isn't on this Mac (no cookies database).
    Missing,
    /// macOS didn't let Writer read it (the Keychain prompt was refused).
    Refused,
    /// It's here but not signed in to YouTube.
    SignedOut,
}

fn reading(error: &str) -> Reading {
    let lower = error.to_lowercase();
    if lower.contains("could not find") && lower.contains("cookies database") {
        Reading::Missing
    } else if lower.contains("keychain") || lower.contains("decrypt") || lower.contains("password") || lower.contains("not permitted") || lower.contains("permission") {
        Reading::Refused
    } else {
        Reading::SignedOut
    }
}

fn browser_name(browser: &str) -> &str {
    match browser {
        "firefox" => "Firefox",
        "chrome" => "Chrome",
        "brave" => "Brave",
        "edge" => "Edge",
        other => other,
    }
}

/// One plain sentence on why no sign-in was saved, and what to do about it.
fn sign_in_problem(tried: &[(&str, Reading)]) -> String {
    let named = |kind: Reading| tried.iter().filter(|(_, reading)| *reading == kind).map(|(browser, _)| browser_name(browser)).collect::<Vec<_>>();
    let list = |names: &[&str]| match names {
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    };
    let refused = named(Reading::Refused);
    if !refused.is_empty() {
        return format!("macOS didn't let Writer read {}'s sign-in. Click Save sign-in again and choose Always Allow when macOS asks for your password.", list(&refused));
    }
    let signed_out = named(Reading::SignedOut);
    if !signed_out.is_empty() {
        return format!("{} isn't signed in to YouTube. Sign in at youtube.com there, then click Save sign-in again.", list(&signed_out));
    }
    "Writer reads your YouTube sign-in from Chrome, Firefox, Brave or Edge, and none of them is on this Mac (Safari's can't be read). Sign in to YouTube in one of them, then click Save sign-in again.".into()
}

#[cfg(test)]
mod sign_in_tests {
    use super::*;

    #[test]
    fn says_plainly_why_no_sign_in_was_saved() {
        let missing = "ERROR: could not find chrome cookies database in \"/Users/x/Library/Application Support/Google/Chrome\"";
        assert_eq!(reading(missing), Reading::Missing);
        assert_eq!(reading("ERROR: cannot decrypt v10 cookies: no key found (Keychain refused)"), Reading::Refused);
        assert_eq!(reading(""), Reading::SignedOut);
        assert!(sign_in_problem(&[("firefox", Reading::Missing), ("chrome", Reading::Missing)]).contains("none of them is on this Mac"));
        assert!(sign_in_problem(&[("firefox", Reading::Missing), ("chrome", Reading::SignedOut)]).starts_with("Chrome isn't signed in"));
        assert!(sign_in_problem(&[("chrome", Reading::Refused), ("brave", Reading::SignedOut)]).starts_with("macOS didn't let Writer read Chrome's"));
    }
}
