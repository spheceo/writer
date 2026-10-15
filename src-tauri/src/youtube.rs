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

pub fn title(url: &str, proxy_file: Option<&Path>) -> String {
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

pub fn download(
    url: &str,
    dest: &Path,
    proxy_file: Option<&Path>,
    on_percent: &dyn Fn(u8),
) -> Result<(), String> {
    let dest_text = dest.to_string_lossy().to_string();
    let mut args = vec![
        "--no-playlist",
        "--no-progress",
        "--newline",
        // A dropped transfer is picked up again from the .part file.
        "--retries",
        "3",
        "--fragment-retries",
        "5",
        "--continue",
        "--socket-timeout",
        "30",
        "-f",
        "bv*[height<=720]+ba/b[height<=720]/b",
        "--merge-output-format",
        "mp4",
        "--max-filesize",
        "1000M",
        "-o",
        &dest_text,
    ];
    let config = proxy_file.map(|path| path.to_string_lossy().into_owned());
    if let Some(path) = config.as_deref() {
        args.push("--config-locations");
        args.push(path);
    }
    args.push(url);
    proc::run_lines(
        "yt-dlp",
        &args,
        Duration::from_secs(25 * 60),
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
    if lower.contains("video unavailable") || lower.contains("has been removed") || lower.contains("not available") {
        return Trouble::Final(format!("YouTube says that video is unavailable.{stale}"));
    }
    if lower.contains("install yt-dlp") {
        return Trouble::Final(error.to_string());
    }
    if lower.contains("not a bot") || lower.contains("sign in to confirm") || lower.contains("http error 403") || lower.contains("http error 429") {
        return Trouble::Blocked;
    }
    if lower.contains("timed out") || lower.contains("connection") || lower.contains("ssl") || lower.contains("unable to download") {
        return Trouble::Connection;
    }
    let said: String = error.chars().take(300).collect();
    Trouble::Final(format!("The YouTube download failed: {said}{stale}"))
}

/// Write yt-dlp's proxy line. Caller deletes the file; it holds the password.
pub fn write_proxy_file(dir: &Path, proxy: &str) -> Result<std::path::PathBuf, String> {
    if proxy.chars().any(|c| c.is_control()) || proxy.contains('"') {
        return Err("The proxy URL has a character yt-dlp's config can't take.".into());
    }
    let path = dir.join("proxy.conf");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|error| error.to_string())?;
    use std::io::Write;
    writeln!(file, "proxy = \"{proxy}\"").map_err(|error| error.to_string())?;
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
    fn dataimpulse_gets_one_sticky_port() {
        let out = sticky_proxy("http://user:secret@gw.dataimpulse.com:823");
        assert!(out.starts_with("http://user:secret@gw.dataimpulse.com:"));
        let port: u16 = out.rsplit(':').next().unwrap().parse().unwrap();
        assert!((10_000..20_001).contains(&port));
        let other = "http://user:secret@proxy.example:823";
        assert_eq!(sticky_proxy(other), other);
    }
}
