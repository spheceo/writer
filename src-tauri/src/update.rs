//! Writer updating itself, as Edit does, from the builds the release
//! workflow publishes. The repo is private, so the app never reads GitHub:
//! the workflow puts each published build's update zip and an index of them
//! (`writer/releases/index.json`) in the cloud through writer-relay, and
//! Writer reads them through the relay too. No GitHub token is anywhere.
//!
//! The channel (`update.json` beside the scripts) picks what's offered:
//! Nightly, the default, offers whatever was published last; Stable only the
//! newest release. Never a downgrade. Installing downloads the zip, checks
//! its checksum, unpacks it beside Writer.app, checks the new app's
//! signature and that the same team signed it, swaps the whole app in with
//! two renames, and restarts. Only release builds inside Writer.app update.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{cloud, release};

const INDEX: &str = "writer/releases/index.json";
const EVERY: Duration = Duration::from_secs(3_600);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Nightly,
    Stable,
}

/// A build Writer can update to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub tag: String,
    pub key: String,
    pub size: u64,
    pub sha256: String,
}

/// What the app shows: this build, the channel, what's on offer, and how an install is going.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub name: String,
    pub channel: Channel,
    /// Only release builds inside Writer.app update themselves.
    pub enabled: bool,
    pub offer: Option<Offer>,
    /// `idle`, `downloading`, `installing` or `restarting`.
    pub phase: String,
    pub error: Option<String>,
    pub checked_at: Option<i64>,
}

struct State {
    offer: Option<Offer>,
    phase: String,
    error: Option<String>,
    checked_at: Option<i64>,
}

static STATE: Mutex<State> = Mutex::new(State { offer: None, phase: String::new(), error: None, checked_at: None });

fn channel_path(data: &Path) -> PathBuf {
    data.join("update.json")
}

pub fn channel(data: &Path) -> Channel {
    let value: Value = fs::read(channel_path(data)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null);
    if value["channel"] == "stable" { Channel::Stable } else { Channel::Nightly }
}

pub fn set_channel(data: &Path, channel: Channel) -> Result<Status, String> {
    fs::write(channel_path(data), serde_json::to_vec(&serde_json::json!({ "channel": channel })).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let _ = check(data);
    Ok(status(data))
}

pub fn status(data: &Path) -> Status {
    let state = STATE.lock();
    let (offer, phase, error, checked_at) = state.map(|state| (state.offer.clone(), state.phase.clone(), state.error.clone(), state.checked_at)).unwrap_or((None, String::new(), None, None));
    Status { name: release::name(), channel: channel(data), enabled: enabled(), offer, phase: if phase.is_empty() { "idle".into() } else { phase }, error, checked_at }
}

/// Release builds inside Writer.app update themselves; dev builds never do.
pub fn enabled() -> bool {
    release::is_release() && bundle().is_some()
}

/// The Writer.app this runs from.
fn bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let macos = exe.parent()?;
    let app = macos.parent()?.parent()?;
    (app.extension().and_then(|ext| ext.to_str()) == Some("app") && macos.ends_with("Contents/MacOS")).then(|| app.to_path_buf())
}

/// This Mac's build target, as the assets are named.
fn target() -> &'static str {
    if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" }
}

/// Whether `tag` ("v0.0.1", "nightly-12") is ahead of this build: a nightly by its number, a release by its version.
pub fn ahead(tag: &str, version: &str, nightly: Option<u64>) -> bool {
    if let Some(number) = tag.strip_prefix("nightly-").and_then(|number| number.parse::<u64>().ok()) {
        return nightly.is_none_or(|current| number > current);
    }
    newer(tag, version)
}

/// Whether `version` ("v0.0.2") is newer than `current` ("0.0.1").
pub fn newer(version: &str, current: &str) -> bool {
    let parse = |text: &str| -> Option<[u64; 3]> {
        let parts: Vec<u64> = text.trim_start_matches('v').split('.').map(|part| part.parse().ok()).collect::<Option<_>>()?;
        (parts.len() == 3).then(|| [parts[0], parts[1], parts[2]])
    };
    matches!((parse(version), parse(current)), (Some(a), Some(b)) if a > b)
}

/// The build to offer from the index on `channel`, if one is ahead of this one.
pub fn pick(index: &Value, channel: Channel, version: &str, nightly: Option<u64>, target: &str) -> Option<Offer> {
    let builds = index["releases"].as_array()?;
    let has = |build: &&Value| build["assets"][target]["key"].is_string();
    let chosen = match channel {
        // Stable: the newest release by its version.
        Channel::Stable => builds.iter().filter(has).filter(|build| build["prerelease"] != true).max_by(|a, b| {
            let version = |build: &Value| build["tag"].as_str().map(|tag| tag.trim_start_matches('v').split('.').filter_map(|part| part.parse::<u64>().ok()).collect::<Vec<_>>()).unwrap_or_default();
            version(a).cmp(&version(b))
        }),
        // Nightly: whatever was published last, release or nightly.
        Channel::Nightly => builds.iter().filter(has).max_by(|a, b| a["publishedAt"].as_str().unwrap_or("").cmp(b["publishedAt"].as_str().unwrap_or(""))),
    }?;
    let tag = chosen["tag"].as_str()?;
    if !ahead(tag, version, nightly) {
        return None;
    }
    let asset = &chosen["assets"][target];
    Some(Offer { tag: tag.to_string(), key: asset["key"].as_str()?.to_string(), size: asset["size"].as_u64().unwrap_or(0), sha256: asset["sha256"].as_str()?.to_string() })
}

/// Asks what's on offer now.
pub fn check(data: &Path) -> Result<Option<Offer>, String> {
    let index = cloud::get_bytes(data, INDEX)?.map(|bytes| serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null)).unwrap_or(Value::Null);
    let nightly = release::tag().and_then(|tag| tag.strip_prefix("nightly-")).and_then(|number| number.parse().ok());
    let offer = pick(&index, channel(data), release::VERSION, nightly, target());
    if let Ok(mut state) = STATE.lock() {
        if state.offer.as_ref().map(|known| &known.tag) != offer.as_ref().map(|found| &found.tag) {
            if let Some(found) = &offer {
                crate::log::info("update", format!("{} is out (this is {})", found.tag, release::name()));
            }
        }
        state.offer = offer.clone();
        state.checked_at = Some(crate::model::now_ms());
    }
    Ok(offer)
}

/// Checks on start and every hour, for as long as Writer runs (release builds only).
pub fn start(data: PathBuf) {
    if !enabled() {
        return;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        if let Err(error) = check(&data) {
            crate::log::warn("update", format!("couldn't check for updates: {error}"));
        }
        std::thread::sleep(EVERY);
    });
}

fn set_phase(phase: &str, error: Option<String>) {
    if let Ok(mut state) = STATE.lock() {
        state.phase = phase.to_string();
        state.error = error;
    }
}

/// Downloads, checks and swaps in the build on offer, then restarts into it.
pub fn install(app: &tauri::AppHandle, data: &Path) -> Result<(), String> {
    if !enabled() {
        return Err("Only Writer as downloaded updates itself; this is a dev build.".into());
    }
    let offer = STATE.lock().ok().and_then(|state| state.offer.clone()).ok_or("There's no update to install.")?;
    let bundle = bundle().ok_or("Writer isn't running from Writer.app.")?;
    let began = Instant::now();
    crate::log::info("update", format!("installing {} over {}", offer.tag, release::name()));
    let result = (|| {
        set_phase("downloading", None);
        let dir = bundle.parent().ok_or("Writer.app has no folder.")?;
        let zip = data.join(format!("update-{}.zip", offer.tag));
        cloud::get_file(data, "update", &offer.key, offer.size, &zip)?;
        let sum: String = Sha256::digest(fs::read(&zip).map_err(|error| error.to_string())?).iter().map(|byte| format!("{byte:02x}")).collect();
        if sum != offer.sha256 {
            let _ = fs::remove_file(&zip);
            return Err(format!("the download of {} didn't match its checksum", offer.tag));
        }
        set_phase("installing", None);
        let installed = install_app(&zip, &bundle, dir);
        let _ = fs::remove_file(&zip);
        installed
    })();
    match result {
        Ok(()) => {
            crate::log::info("update", format!("{} installed in {}; restarting", offer.tag, crate::log::since_start(began)));
            set_phase("restarting", None);
            app.restart();
        }
        Err(error) => {
            crate::log::error("update", format!("{} didn't install: {error}", offer.tag));
            set_phase("idle", Some(error.clone()));
            Err(error)
        }
    }
}

/// Unpacks the zip beside Writer.app, checks the new app, and swaps it in.
fn install_app(zip: &Path, bundle: &Path, dir: &Path) -> Result<(), String> {
    let unpacked = dir.join(format!(".Writer-update-{}", std::process::id()));
    let unwritable = |error: std::io::Error| format!("can't write to {} ({error}); install the new Writer from its disk image", dir.display());
    let _ = fs::remove_dir_all(&unpacked);
    fs::create_dir_all(&unpacked).map_err(unwritable)?;
    let outcome = (|| {
        // ditto keeps the bundle's signature, links and attributes as zipped.
        crate::proc::run("ditto", &["-x", "-k", &zip.to_string_lossy(), &unpacked.to_string_lossy()], Duration::from_secs(300)).map_err(|error| format!("couldn't unpack the update: {error}"))?;
        let fresh = unpacked.join("Writer.app");
        if !fresh.join("Contents/MacOS/Writer").is_file() {
            return Err("the update has no Writer.app".to_string());
        }
        crate::proc::run("codesign", &["--verify", "--deep", "--strict", &fresh.to_string_lossy()], Duration::from_secs(120)).map_err(|error| format!("the update's signature is broken: {error}"))?;
        if let Some(team) = team_of(bundle) {
            if team_of(&fresh).as_deref() != Some(team.as_str()) {
                return Err(format!("the update isn't signed by {team}, who signed this Writer"));
            }
        }
        let old = dir.join(format!(".Writer-old-{}.app", std::process::id()));
        fs::rename(bundle, &old).map_err(unwritable)?;
        if let Err(error) = fs::rename(&fresh, bundle) {
            let _ = fs::rename(&old, bundle);
            return Err(unwritable(error));
        }
        let _ = fs::remove_dir_all(&old);
        Ok(())
    })();
    let _ = fs::remove_dir_all(&unpacked);
    outcome
}

/// The team that signed an app (Developer ID), or none when it's signed ad hoc.
fn team_of(app: &Path) -> Option<String> {
    let out = std::process::Command::new("codesign").args(["-dv", &app.to_string_lossy()]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines().find_map(|line| line.strip_prefix("TeamIdentifier=")).map(str::trim).filter(|team| *team != "not set").map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn index() -> Value {
        let asset = |tag: &str| json!({ "aarch64-apple-darwin": { "key": format!("writer/releases/{tag}/Writer-aarch64-apple-darwin.zip"), "size": 10, "sha256": "ab" } });
        json!({ "releases": [
            { "tag": "nightly-1", "prerelease": true, "publishedAt": "2026-10-23T08:00:00Z", "assets": asset("nightly-1") },
            { "tag": "v0.0.0", "prerelease": false, "publishedAt": "2026-10-23T09:00:00Z", "assets": asset("v0.0.0") },
            { "tag": "nightly-2", "prerelease": true, "publishedAt": "2026-10-23T10:00:00Z", "assets": asset("nightly-2") }
        ]})
    }

    #[test]
    fn nightly_offers_whatever_came_out_last_and_stable_only_releases() {
        let target = "aarch64-apple-darwin";
        assert_eq!(pick(&index(), Channel::Nightly, "0.0.0", Some(1), target).map(|offer| offer.tag), Some("nightly-2".into()));
        // On v0.0.0 (no nightly number), the newer nightly is offered.
        assert_eq!(pick(&index(), Channel::Nightly, "0.0.0", None, target).map(|offer| offer.tag), Some("nightly-2".into()));
        // Stable on nightly-1 (built at 0.0.0): v0.0.0 isn't newer, so nothing.
        assert!(pick(&index(), Channel::Stable, "0.0.0", Some(1), target).is_none());
        assert_eq!(pick(&index(), Channel::Stable, "0.0.0", None, target).map(|offer| offer.tag), None);
        // Nothing for an Intel Mac in this index.
        assert!(pick(&index(), Channel::Nightly, "0.0.0", Some(1), "x86_64-apple-darwin").is_none());
    }

    #[test]
    fn never_offers_a_downgrade() {
        assert!(!ahead("nightly-1", "0.0.0", Some(2)));
        assert!(ahead("nightly-3", "0.0.0", Some(2)));
        assert!(!ahead("v0.0.0", "0.0.0", None));
        assert!(ahead("v0.0.1", "0.0.0", None));
        assert!(newer("v0.1.0", "0.0.9"));
        assert!(!newer("v0.0.1", "0.0.2"));
    }
}
