//! The conversation with the editor on a footage script: what the producer
//! asked and what Claude answered, build and revision alike, kept beside the
//! script (`chat.json`) so it survives a reload and goes back to Claude as
//! context on the next request, as Repack keeps a script's chat.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::now_ms;

/// Turns kept, and how many recent ones go back to Claude.
const KEPT: usize = 80;
pub const SENT: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    /// `producer` or `editor`.
    pub role: String,
    pub text: String,
    pub at: i64,
    /// What the editor's answer cost at API prices, covered by the plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// An answer that ended in an error rather than edits.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
    /// Claude asked to start the script over: `pending` until the producer
    /// answers, then `accepted` or `declined`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<String>,
}

pub fn read(dir: &Path) -> Vec<Turn> {
    fs::read(dir.join("chat.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write(dir: &Path, turns: &[Turn]) -> Result<(), String> {
    let tmp = dir.join("chat.json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(turns).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    fs::rename(tmp, dir.join("chat.json")).map_err(|error| error.to_string())
}

pub fn add(dir: &Path, role: &str, text: &str, cost_usd: Option<f64>, failed: bool) {
    let mut turns = read(dir);
    turns.push(Turn { role: role.into(), text: text.trim().to_string(), at: now_ms(), cost_usd, failed, reset: None });
    let skip = turns.len().saturating_sub(KEPT);
    let _ = write(dir, &turns[skip..]);
}

/// Claude's answer asking to start the script over, waiting for the producer.
/// An earlier ask still waiting is declined: only the newest can be answered.
pub fn ask_reset(dir: &Path, text: &str, cost_usd: Option<f64>) {
    let mut turns = read(dir);
    for turn in &mut turns {
        if turn.reset.as_deref() == Some("pending") {
            turn.reset = Some("declined".into());
        }
    }
    turns.push(Turn { role: "editor".into(), text: text.trim().to_string(), at: now_ms(), cost_usd, failed: false, reset: Some("pending".into()) });
    let skip = turns.len().saturating_sub(KEPT);
    let _ = write(dir, &turns[skip..]);
}

/// The producer's answer to the waiting ask; an error when there's none.
pub fn settle_reset(dir: &Path, accepted: bool) -> Result<(), String> {
    let mut turns = read(dir);
    let turn = turns.iter_mut().rev().find(|turn| turn.reset.as_deref() == Some("pending")).ok_or("Claude hasn't asked to start over.")?;
    turn.reset = Some(if accepted { "accepted" } else { "declined" }.into());
    write(dir, &turns)
}

/// Starts the conversation again. The script's edits stay.
pub fn clear(dir: &Path) -> Result<(), String> {
    match fs::remove_file(dir.join("chat.json")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// The recent conversation, as context for Claude.
pub fn recent(dir: &Path) -> String {
    let turns = read(dir);
    let skip = turns.len().saturating_sub(SENT);
    turns[skip..]
        .iter()
        .map(|turn| format!("{}: {}", if turn.role == "producer" { "Producer" } else { "Editor" }, turn.text))
        .collect::<Vec<_>>()
        .join("\n\n")
}
