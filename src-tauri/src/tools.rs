//! Finding the programs Writer runs. A Mac app opened from the Dock doesn't
//! get a terminal's PATH, so Homebrew's folder and ~/.local/bin are never
//! searched for it. Writer looks in order: its own copy (yt-dlp), the copy
//! bundled inside the app (ffmpeg, ffprobe), then the usual install places.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

static DATA: OnceLock<PathBuf> = OnceLock::new();

pub fn init(data: &Path) {
    let _ = DATA.set(data.to_path_buf());
}

/// Writer's own copies. yt-dlp is kept unpacked (yt-dlp's onedir macOS
/// build): the single-file build unpacks itself on every run, which took
/// about 20 s a launch.
fn managed(program: &str) -> Option<PathBuf> {
    // Only yt-dlp: nothing else is ever taken from a folder Writer writes to.
    if program != "yt-dlp" {
        return None;
    }
    Some(yt_dlp_dir(&DATA.get()?.join("bin")).join("yt-dlp_macos"))
}

/// macOS's own tools, which the updater and sign-in trust: only ever from
/// /usr/bin, never a same-named program earlier on a user-writable PATH.
const SYSTEM: [&str; 6] = ["codesign", "ditto", "open", "xattr", "osascript", "spctl"];

fn yt_dlp_dir(bin: &Path) -> PathBuf {
    bin.join("yt-dlp-dist")
}

/// Tauri puts a bundled program beside Writer's own binary.
fn bundled(program: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(program))
}

fn installed(program: &str) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
        home.join(".claude/local"),
        home.join(".bun/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ]
    .into_iter()
    .map(|dir| dir.join(program))
    .collect()
}

/// Where a program is, and how it got there.
pub fn locate(program: &str) -> Option<(PathBuf, &'static str)> {
    if SYSTEM.contains(&program) {
        return Some((PathBuf::from("/usr/bin").join(program), "part of macOS"));
    }
    if let Some(path) = managed(program).filter(|path| path.is_file()) {
        return Some((path, "kept up to date by Writer"));
    }
    if let Some(path) = bundled(program).filter(|path| path.is_file()) {
        return Some((path, "bundled with Writer"));
    }
    installed(program).into_iter().find(|path| path.is_file()).map(|path| (path, "installed on this Mac"))
}

/// The path to run for a program, or its bare name if Writer can't find it.
pub fn resolve(program: &str) -> String {
    locate(program)
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.to_string())
}

const YT_DLP: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

/// yt-dlp goes stale as YouTube changes, so Writer keeps its own copy: the
/// official macOS build from yt-dlp's GitHub releases, checked against the
/// release's SHA-256 list, and replaced when a month old. Nobody installs it.
pub fn ensure_yt_dlp() -> Result<PathBuf, String> {
    let data = DATA.get().ok_or("Writer's data folder isn't ready.")?;
    let bin = data.join("bin");
    let dir = yt_dlp_dir(&bin);
    let program = dir.join("yt-dlp_macos");
    // The old single-file copy is slow to start; drop it.
    let _ = std::fs::remove_file(bin.join("yt-dlp"));
    let age = std::fs::metadata(dir.join(".fetched"))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok());
    match age {
        Some(age) if age < Duration::from_secs(30 * 86_400) && program.is_file() => Ok(program),
        // Stale: try the newest, and keep the old copy if that fails.
        Some(_) if program.is_file() => {
            if fetch_yt_dlp(&bin).is_err() {
                let _ = std::fs::write(dir.join(".fetched"), b"");
            }
            Ok(program)
        }
        _ => fetch_yt_dlp(&bin).map(|_| program),
    }
}

fn fetch_yt_dlp(bin: &Path) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout(Duration::from_secs(10 * 60))
        .user_agent("Writer")
        .build();
    let sums = agent
        .get(&format!("{YT_DLP}/SHA2-256SUMS"))
        .call()
        .map_err(|error| format!("Could not reach yt-dlp's releases ({error})."))?
        .into_string()
        .map_err(|error| error.to_string())?;
    let expected = sums
        .lines()
        .find_map(|line| {
            let (hash, name) = line.split_once(char::is_whitespace)?;
            (name.trim() == "yt-dlp_macos.zip").then(|| hash.trim().to_ascii_lowercase())
        })
        .ok_or("yt-dlp's release has no macOS build listed.")?;
    let mut bytes = Vec::new();
    agent
        .get(&format!("{YT_DLP}/yt-dlp_macos.zip"))
        .call()
        .map_err(|error| format!("Could not download yt-dlp ({error})."))?
        .into_reader()
        .take(300_000_000)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("The yt-dlp download broke off ({error})."))?;
    let actual: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    if actual != expected {
        return Err("The yt-dlp download didn't match its published checksum, so it was not used.".into());
    }
    std::fs::create_dir_all(bin).map_err(|error| error.to_string())?;
    let zip = bin.join("yt-dlp_macos.zip");
    let unpacked = bin.join("yt-dlp-unpacking");
    let _ = std::fs::remove_dir_all(&unpacked);
    std::fs::write(&zip, &bytes).map_err(|error| error.to_string())?;
    // ditto ships with macOS and keeps the bundle's permissions.
    let status = std::process::Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&zip)
        .arg(&unpacked)
        .status()
        .map_err(|error| error.to_string());
    let _ = std::fs::remove_file(&zip);
    if !status?.success() {
        let _ = std::fs::remove_dir_all(&unpacked);
        return Err("The yt-dlp download couldn't be unpacked.".into());
    }
    let program = find(&unpacked, "yt-dlp_macos", 3).ok_or("The yt-dlp download has no program in it.")?;
    let root = program.parent().ok_or("No folder for yt-dlp.")?.to_path_buf();
    std::fs::write(root.join(".fetched"), b"").map_err(|error| error.to_string())?;
    let dir = yt_dlp_dir(bin);
    let old = bin.join("yt-dlp-old");
    let _ = std::fs::remove_dir_all(&old);
    if dir.exists() {
        std::fs::rename(&dir, &old).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&root, &dir).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_dir_all(&old);
    let _ = std::fs::remove_dir_all(&unpacked);
    Ok(())
}

/// A file by name, at most `depth` folders down.
fn find(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() && entry.file_name() == name {
            return Some(path);
        }
        if depth > 0 && path.is_dir() {
            if let Some(found) = find(&path, name, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}
