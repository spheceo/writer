use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const DEFAULT_RELAY: &str = "https://writer-relay.sphe-g-personal.workers.dev";

/// The relay Writer talks to, built into the app (`just bundle` sets these),
/// so users never see or enter it. A settings.json beside the scripts still
/// overrides them, for development.
const BUILT_RELAY: Option<&str> = option_env!("WRITER_RELAY_URL");
const BUILT_TOKEN: Option<&str> = option_env!("WRITER_RELAY_TOKEN");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub relay_url: String,
    #[serde(default)]
    pub relay_token: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            relay_url: BUILT_RELAY.filter(|url| !url.is_empty()).unwrap_or(DEFAULT_RELAY).into(),
            relay_token: BUILT_TOKEN.unwrap_or("").into(),
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
        relay_token: if file.relay_token.is_empty() { built.relay_token } else { file.relay_token },
    }
}

pub fn save(data: &Path, incoming: Settings) -> Result<Settings, String> {
    let settings = Settings {
        relay_url: check_relay(&incoming.relay_url)?,
        relay_token: check_secret("Relay token", &incoming.relay_token, 200)?,
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

fn check_secret(label: &str, value: &str, max: usize) -> Result<String, String> {
    let value = value.trim();
    if value.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(format!("{label} has a space or a line break."));
    }
    if value.len() > max {
        return Err(format!("{label} is too long."));
    }
    Ok(value.to_string())
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
}
