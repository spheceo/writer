use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const DEFAULT_RELAY: &str = "https://writer-relay.sphe-g-personal.workers.dev";

/// The relay Writer talks to, built into the app, so users never see or
/// enter it, and the only one a release build sends its person's token to.
/// A dev build may use a local worker instead (settings.json). No token is
/// built in: people sign in (account.rs).
const BUILT_RELAY: Option<&str> = option_env!("WRITER_RELAY_URL");

fn built_relay() -> String {
    BUILT_RELAY.filter(|url| !url.is_empty()).unwrap_or(DEFAULT_RELAY).trim_end_matches('/').to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub relay_url: String,
    /// How Writer reaches Claude: `sdk` (the Agent SDK through writer-agent,
    /// as Edit; a script built in steps with Writer's tools), everyone's
    /// since Oct 24 2026, or `cli` (one `claude -p` answer per question),
    /// kept for comparisons (`WRITER_ENGINE=cli`) and not offered in Settings.
    #[serde(default)]
    pub engine: Engine,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Cli,
    #[default]
    Sdk,
}

/// The Agent SDK for everyone: a `cli` saved before Settings stopped
/// offering the choice no longer applies; only `WRITER_ENGINE=cli` asks for it.
fn engine_now() -> Engine {
    if std::env::var("WRITER_ENGINE").is_ok_and(|value| value == "cli") { Engine::Cli } else { Engine::Sdk }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            relay_url: built_relay(),
            engine: engine_now(),
        }
    }
}

fn path(data: &Path) -> std::path::PathBuf {
    data.join("settings.json")
}

pub fn load(data: &Path) -> Settings {
    let built = Settings::default();
    let Some(file) = fs::read(path(data)).ok().and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok()) else {
        return built;
    };
    Settings {
        relay_url: check_relay(&file.relay_url).unwrap_or(built.relay_url),
        engine: engine_now(),
    }
}

/// Older Writers kept the retired team token in settings.json: rewrites the
/// file without it.
pub fn forget_old_token(data: &Path) {
    let had = fs::read(path(data)).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()).is_some_and(|value| value.get("relayToken").is_some());
    if had {
        let _ = save(data, load(data));
    }
}

pub fn save(data: &Path, incoming: Settings) -> Result<Settings, String> {
    let settings = Settings {
        relay_url: check_relay(&incoming.relay_url)?,
        engine: incoming.engine,
    };
    fs::create_dir_all(data).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?;
    let file = path(data);
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut out = options.open(&file).map_err(|error| error.to_string())?;
    out.write_all(&bytes).map_err(|error| error.to_string())?;
    Ok(settings)
}

/// Picks how Writer reaches Claude, keeping the rest.
pub fn set_engine(data: &Path, engine: Engine) -> Result<Settings, String> {
    let mut settings = load(data);
    settings.engine = engine;
    save(data, settings)
}

/// The relay to use: the built one, or in a dev build a worker on this Mac.
/// Anything else would send the person's token to someone else's server.
pub fn check_relay(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() || url == built_relay() {
        return Ok(built_relay());
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("The relay URL is invalid.".into());
    }
    let local = ["http://127.0.0.1:", "http://localhost:"].iter().any(|start| url.strip_prefix(start).is_some_and(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit())));
    if cfg!(debug_assertions) && local {
        return Ok(url.to_string());
    }
    Err("Writer talks only to its own relay.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_built_relay_or_a_local_worker() {
        assert!(check_relay("http://example.com").is_err());
        assert!(check_relay("https://writer.example/").is_err());
        assert!(check_relay("http://localhost.evil.example").is_err());
        assert_eq!(check_relay("").unwrap(), built_relay());
        assert_eq!(check_relay(&format!("{}/", built_relay())).unwrap(), built_relay());
        // Tests are a debug build: a local worker is allowed there only.
        assert!(check_relay("http://localhost:8787").is_ok());
    }

    #[test]
    fn forgets_the_retired_token() {
        let data = std::env::temp_dir().join(format!("writer-settings-{}", std::process::id()));
        fs::create_dir_all(&data).unwrap();
        fs::write(path(&data), r#"{"relayUrl":"https://writer.example","relayToken":"old","autoFix":false}"#).unwrap();
        forget_old_token(&data);
        let text = fs::read_to_string(path(&data)).unwrap();
        // The old file's relay isn't Writer's own, so the built one replaces it;
        // autoFix went when fixing moved to accounts' Fixing page.
        assert!(!text.contains("relayToken") && !text.contains("https://writer.example") && text.contains(&built_relay()) && !text.contains("autoFix"), "{text}");
        let _ = fs::remove_dir_all(&data);
    }
}
