//! Writer updating itself, as Edit does, from the builds the release
//! workflow publishes as GitHub Releases (the repo is public: no token).
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

use crate::release;

const RELEASES: &str = "https://api.github.com/repos/spheceo/writer/releases";
/// A new build shows up within this long. Each look reads the releases'
/// Atom feed, which GitHub's API limit (60 anonymous asks an hour, shared
/// by every Mac behind one address) doesn't cover; the API is asked only
/// when the feed's tags change, as Edit does.
const FEED: &str = "https://github.com/spheceo/writer/releases.atom";
const EVERY: Duration = Duration::from_secs(60);
/// The API is asked again after this long even with the feed unchanged (a
/// release whose builds were still uploading when it was last asked).
const ASK_ANYWAY: Duration = Duration::from_secs(30 * 60);

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
    /// The update zip on the GitHub release.
    pub url: String,
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
    /// Why the last look for updates failed, until one works.
    pub check_error: Option<String>,
}

struct State {
    offer: Option<Offer>,
    phase: String,
    error: Option<String>,
    checked_at: Option<i64>,
    check_error: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State { offer: None, phase: String::new(), error: None, checked_at: None, check_error: None });

/// The feed's tags and the channel when the API last answered, and when.
static ASKED: Mutex<Option<(Channel, String, Instant)>> = Mutex::new(None);

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
    let (offer, phase, error, checked_at, check_error) = state.map(|state| (state.offer.clone(), state.phase.clone(), state.error.clone(), state.checked_at, state.check_error.clone())).unwrap_or((None, String::new(), None, None, None));
    Status { name: release::name(), channel: channel(data), enabled: enabled(), offer, phase: if phase.is_empty() { "idle".into() } else { phase }, error, checked_at, check_error }
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

/// The update zip's name for this Mac ("Writer-aarch64-apple-darwin.zip").
fn asset_name(target: &str) -> String {
    format!("Writer-{target}.zip")
}

/// The build to offer from GitHub's releases on `channel`, if one is ahead
/// of this one: Nightly, whatever was published last (release or nightly);
/// Stable, the newest release (`latest`, which skips pre-releases).
pub fn pick(releases: &[Value], channel: Channel, version: &str, nightly: Option<u64>, target: &str) -> Option<(String, Value)> {
    let name = asset_name(target);
    let asset_of = |release: &Value| release["assets"].as_array().and_then(|assets| assets.iter().find(|asset| asset["name"] == name.as_str()).cloned());
    let chosen = releases
        .iter()
        .filter(|release| release["draft"] != true && (channel == Channel::Nightly || release["prerelease"] != true))
        .filter(|release| asset_of(release).is_some())
        // ISO 8601 times in UTC compare as text.
        .max_by(|a, b| a["published_at"].as_str().unwrap_or("").cmp(b["published_at"].as_str().unwrap_or("")))?;
    let tag = chosen["tag_name"].as_str()?;
    ahead(tag, version, nightly).then(|| (tag.to_string(), asset_of(chosen).unwrap_or(Value::Null)))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).user_agent("Writer").build()
}

fn get_json(url: &str) -> Result<Option<Value>, String> {
    match agent().get(url).set("accept", "application/vnd.github+json").call() {
        Ok(response) => response.into_json().map(Some).map_err(|error| format!("GitHub answered with something Writer can't read ({error})")),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => Err(format!("GitHub answered {code}")),
        Err(error) => Err(format!("couldn't reach GitHub ({error})")),
    }
}

/// Every tag in the releases' feed, sorted and joined: it changes when a
/// build is published or taken down (drafts aren't in it).
pub fn feed_tags(feed: &str) -> String {
    let mut tags: Vec<&str> = feed.split("/releases/tag/").skip(1).filter_map(|rest| rest.split(['"', '<', ' ', '\n']).next()).filter(|tag| !tag.is_empty()).collect();
    tags.sort_unstable();
    tags.dedup();
    tags.join(" ")
}

fn read_feed() -> Result<String, String> {
    let response = agent().get(FEED).set("accept", "application/atom+xml").call().map_err(|error| format!("couldn't read the releases feed ({error})"))?;
    response.into_string().map(|body| feed_tags(&body)).map_err(|error| error.to_string())
}

/// The minute's look: the feed first, and GitHub's API only when it changed
/// (or the channel did, or half an hour went by). A feed that can't be read
/// falls back to asking the API.
pub fn poll(data: &Path) -> Result<Option<Offer>, String> {
    let channel = channel(data);
    let tags = read_feed().ok();
    let unchanged = match (&tags, ASKED.lock().ok().and_then(|asked| asked.clone())) {
        (Some(tags), Some((was, known, at))) => was == channel && &known == tags && at.elapsed() < ASK_ANYWAY,
        _ => false,
    };
    if unchanged {
        let Ok(mut state) = STATE.lock() else { return Ok(None) };
        state.checked_at = Some(crate::model::now_ms());
        state.check_error = None;
        return Ok(state.offer.clone());
    }
    let offer = check(data)?;
    if let (Some(tags), Ok(mut asked)) = (tags, ASKED.lock()) {
        *asked = Some((channel, tags, Instant::now()));
    }
    Ok(offer)
}

/// Asks GitHub's API what's on offer now (Setup's Check again, a channel change).
pub fn check(data: &Path) -> Result<Option<Offer>, String> {
    let answer = ask(data);
    if let Ok(mut state) = STATE.lock() {
        state.check_error = answer.as_ref().err().cloned();
    }
    answer
}

fn ask(data: &Path) -> Result<Option<Offer>, String> {
    let channel = channel(data);
    let releases: Vec<Value> = match channel {
        Channel::Stable => get_json(&format!("{RELEASES}/latest"))?.into_iter().collect(),
        Channel::Nightly => get_json(&format!("{RELEASES}?per_page=30"))?.and_then(|list| list.as_array().cloned()).unwrap_or_default(),
    };
    let nightly = release::tag().and_then(|tag| tag.strip_prefix("nightly-")).and_then(|number| number.parse().ok());
    let offer = match pick(&releases, channel, release::VERSION, nightly, target()) {
        Some((tag, asset)) => {
            let url = asset["browser_download_url"].as_str().ok_or("an update without a download link")?.to_string();
            // GitHub hashes uploads itself; the .sha256 beside the zip is the fallback.
            let sha256 = match asset["digest"].as_str().and_then(|digest| digest.strip_prefix("sha256:")) {
                Some(digest) => digest.to_string(),
                None => agent()
                    .get(&format!("{url}.sha256"))
                    .call()
                    .map_err(|error| format!("{tag} has no checksum ({error})"))?
                    .into_string()
                    .map_err(|error| error.to_string())?
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string(),
            };
            Some(Offer { tag, url, size: asset["size"].as_u64().unwrap_or(0), sha256 })
        }
        None => None,
    };
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

/// Downloads the update zip, its progress in the panel of what's under way.
fn download(offer: &Offer, path: &Path) -> Result<(), String> {
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .user_agent("Writer")
        .build()
        .get(&offer.url)
        .call()
        .map_err(|error| format!("couldn't download {} ({error})", offer.tag))?;
    let mut reader = response.into_reader();
    let mut file = fs::File::create(path).map_err(|error| error.to_string())?;
    let mut buffer = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer).map_err(|error| format!("the download of {} stopped ({error})", offer.tag))?;
        if read == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..read]).map_err(|error| error.to_string())?;
        done += read as u64;
        crate::activity::set("update", None, "Updating Writer", &offer.tag, &format!("{} of {}", crate::log::size(done), crate::log::size(offer.size)), done, offer.size, "bytes");
    }
    crate::activity::end("update");
    Ok(())
}

/// Looks on start and every minute, for as long as Writer runs (release
/// builds only). A failure is logged once until a look works again.
pub fn start(data: PathBuf) {
    if !enabled() {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        let mut failing = false;
        loop {
            match poll(&data) {
                Ok(_) => failing = false,
                Err(error) if !failing => {
                    failing = true;
                    crate::log::warn("update", format!("couldn't check for updates: {error}"));
                }
                Err(_) => {}
            }
            std::thread::sleep(EVERY);
        }
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
        download(&offer, &zip).inspect_err(|_| crate::activity::end("update"))?;
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
        // Both signed by the same Developer ID team, or no update: an ad hoc
        // signature proves nothing about who made the zip.
        let team = team_of(bundle).ok_or("this Writer isn't signed with a Developer ID, so it can't check an update; install the new Writer from its disk image")?;
        if team_of(&fresh).as_deref() != Some(team.as_str()) {
            return Err(format!("the update isn't signed by {team}, who signed this Writer"));
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
    let out = std::process::Command::new("/usr/bin/codesign").args(["-dv", &app.to_string_lossy()]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines().find_map(|line| line.strip_prefix("TeamIdentifier=")).map(str::trim).filter(|team| *team != "not set").map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn releases() -> Vec<Value> {
        let build = |tag: &str, prerelease: bool, at: &str| {
            json!({ "tag_name": tag, "draft": false, "prerelease": prerelease, "published_at": at, "assets": [
                { "name": "Writer-aarch64-apple-darwin.zip", "browser_download_url": format!("https://github.com/spheceo/writer/releases/download/{tag}/Writer-aarch64-apple-darwin.zip"), "size": 10, "digest": "sha256:ab" }
            ]})
        };
        vec![
            build("nightly-2", true, "2026-10-23T10:00:00Z"),
            build("v0.0.0", false, "2026-10-23T09:00:00Z"),
            build("nightly-1", true, "2026-10-23T08:00:00Z"),
            json!({ "tag_name": "nightly-3", "draft": true, "prerelease": true, "published_at": "2026-10-23T11:00:00Z", "assets": [] }),
        ]
    }

    #[test]
    fn nightly_offers_whatever_came_out_last_and_stable_only_releases() {
        let target = "aarch64-apple-darwin";
        let tag = |found: Option<(String, Value)>| found.map(|(tag, _)| tag);
        assert_eq!(tag(pick(&releases(), Channel::Nightly, "0.0.0", Some(1), target)), Some("nightly-2".into()));
        // On v0.0.0 (no nightly number), the newer nightly is offered; a draft never is.
        assert_eq!(tag(pick(&releases(), Channel::Nightly, "0.0.0", None, target)), Some("nightly-2".into()));
        // Stable on nightly-1 (built at 0.0.0): v0.0.0 isn't newer, so nothing.
        assert!(pick(&releases(), Channel::Stable, "0.0.0", Some(1), target).is_none());
        assert_eq!(tag(pick(&releases(), Channel::Stable, "-1.0.0", None, target)), None);
        // Nothing for an Intel Mac among these.
        assert!(pick(&releases(), Channel::Nightly, "0.0.0", Some(1), "x86_64-apple-darwin").is_none());
    }

    #[test]
    fn reads_the_feeds_tags() {
        let feed = r#"<entry><link rel="alternate" type="text/html" href="https://github.com/spheceo/writer/releases/tag/nightly-22"/></entry>
<entry><link rel="alternate" type="text/html" href="https://github.com/spheceo/writer/releases/tag/nightly-21"/><id>tag:github.com,2008:Repository/1/nightly-21</id></entry>
<entry><link href="https://github.com/spheceo/writer/releases/tag/v0.0.0"/></entry>"#;
        assert_eq!(feed_tags(feed), "nightly-21 nightly-22 v0.0.0");
        assert_eq!(feed_tags("<feed></feed>"), "");
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
