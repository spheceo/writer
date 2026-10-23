use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const DEFAULT_RELAY: &str = "https://writer-relay.sphe-g-personal.workers.dev";

/// The relay Writer talks to, built into the app, so users never see or
/// enter it. A settings.json beside the scripts still overrides it, for
/// development. No token is built in: people sign in (account.rs).
const BUILT_RELAY: Option<&str> = option_env!("WRITER_RELAY_URL");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub relay_url: String,
    /// Claude fixes new review notes by itself while Writer runs (reviews.rs).
    #[serde(default = "on")]
    pub auto_fix: bool,
    /// How Writer reaches Claude: `cli` (one `claude -p` answer per question,
    /// a script's plan in one reply) or `sdk` (the Agent SDK through
    /// writer-agent, as Edit; a script built in steps with Writer's tools).
    #[serde(default)]
    pub engine: Engine,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Cli,
    Sdk,
}

fn on() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            relay_url: BUILT_RELAY.filter(|url| !url.is_empty()).unwrap_or(DEFAULT_RELAY).into(),
            auto_fix: true,
            engine: Engine::default(),
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
        relay_url: if file.relay_url.trim().is_empty() { built.relay_url } else { file.relay_url },
        auto_fix: file.auto_fix,
        engine: file.engine,
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
        auto_fix: incoming.auto_fix,
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

/// Turns Claude's fixing of new review notes on or off, keeping the rest.
pub fn set_auto_fix(data: &Path, on: bool) -> Result<Settings, String> {
    let mut settings = load(data);
    settings.auto_fix = on;
    save(data, settings)
}

pub fn check_relay(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(DEFAULT_RELAY.into());
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("The relay URL is invalid.".into());
    }
    if !(url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost"))
    {
        return Err("The relay URL has to be https, or localhost for a local worker.".into());
    }
    Ok(url.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_url_rejects_plain_http() {
        assert!(check_relay("http://example.com").is_err());
        assert_eq!(
            check_relay("https://writer.example/").unwrap(),
            "https://writer.example"
        );
        assert!(check_relay("http://localhost:8787").is_ok());
    }

    #[test]
    fn forgets_the_retired_token() {
        let data = std::env::temp_dir().join(format!("writer-settings-{}", std::process::id()));
        fs::create_dir_all(&data).unwrap();
        fs::write(path(&data), r#"{"relayUrl":"https://writer.example","relayToken":"old","autoFix":false}"#).unwrap();
        forget_old_token(&data);
        let text = fs::read_to_string(path(&data)).unwrap();
        assert!(!text.contains("relayToken") && text.contains("https://writer.example") && text.contains("false"), "{text}");
        let _ = fs::remove_dir_all(&data);
    }
}
