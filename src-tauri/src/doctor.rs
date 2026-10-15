//! What Writer leans on, checked up front. Only Claude Code is the user's to
//! install: ffmpeg and ffprobe ship inside Writer, Writer keeps its own
//! yt-dlp, and every key lives on the relay.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::{proc, tools};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

fn version(program: &str, args: &[&str]) -> Option<String> {
    proc::run(program, args, Duration::from_secs(30))
        .ok()
        .and_then(|text| text.lines().next().map(|line| line.trim().chars().take(60).collect()))
}

fn bundled_tool(name: &'static str, program: &str) -> Check {
    match tools::locate(program) {
        Some((_, from)) => {
            let found = version(program, &["-version"]).unwrap_or_default();
            let number = found.split_whitespace().nth(2).unwrap_or("").to_string();
            Check { name, ok: true, detail: format!("{number} · {from}").trim_start_matches(" · ").to_string() }
        }
        None => Check {
            name,
            ok: false,
            detail: "Missing. The packaged app bundles it; for development, brew install ffmpeg.".into(),
        },
    }
}

/// Each check runs on its own thread, so the slowest sets the pace.
pub fn run(data: &Path) -> Vec<Check> {
    std::thread::scope(|scope| {
        let claude = scope.spawn(claude_check);
        let ffmpeg = scope.spawn(|| bundled_tool("ffmpeg", "ffmpeg"));
        let ffprobe = scope.spawn(|| bundled_tool("ffprobe", "ffprobe"));
        let yt_dlp = scope.spawn(yt_dlp_check);
        let relay = scope.spawn(|| relay_check(data));
        let watcher = scope.spawn(|| watcher_check(data));
        [claude, ffmpeg, ffprobe, yt_dlp, relay, watcher]
            .into_iter()
            .map(|handle| handle.join().unwrap_or(Check { name: "Check", ok: false, detail: "This check crashed.".into() }))
            .collect()
    })
}

fn claude_check() -> Check {
    match tools::locate("claude") {
        Some(_) => {
            let status = crate::claude::status();
            Check {
                name: "Claude Code",
                ok: status.logged_in,
                detail: if status.logged_in {
                    format!("{} · {}", version("claude", &["--version"]).unwrap_or_default(), status.message)
                } else {
                    "Installed but signed out. Run claude in a terminal once and sign in.".into()
                },
            }
        }
        None => Check {
            name: "Claude Code",
            ok: false,
            detail: "Install Claude Code, then run claude once to sign in. It's the one thing Writer needs from you.".into(),
        },
    }
}

fn yt_dlp_check() -> Check {
    match tools::locate("yt-dlp") {
        Some((_, from)) => Check {
            name: "yt-dlp",
            ok: true,
            detail: format!("{} · {from}", version("yt-dlp", &["--version"]).unwrap_or_default()),
        },
        None => Check { name: "yt-dlp", ok: true, detail: "Writer fetches it the first time you import from YouTube.".into() },
    }
}

fn relay_check(data: &Path) -> Check {
    match crate::relay_health(data) {
        Ok(health) => Check { name: "Writer service", ok: health.transcription && health.drive, detail: health.message },
        Err(error) => Check { name: "Writer service", ok: false, detail: error },
    }
}

/// The watcher is optional: without it, imports keep the speakers as heard.
fn watcher_check(data: &Path) -> Check {
    let health = crate::net::Relay::new(&crate::settings::load(data)).and_then(|relay| relay.health());
    match health {
        Ok(value) if value["watch"].as_bool().unwrap_or(false) => {
            Check { name: "Speaker check", ok: true, detail: "Gemini watches each import's footage to check who speaks.".into() }
        }
        Ok(_) => Check { name: "Speaker check", ok: false, detail: "Not set up on Writer's service yet; imports keep the speakers as heard.".into() },
        Err(error) => Check { name: "Speaker check", ok: false, detail: error },
    }
}
