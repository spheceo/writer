//! Everything long Writer is doing now, for the progress panel every screen
//! shows (`GET /api/activity`): imports, Claude's jobs (builds, fixes,
//! restores), the footage going to Edit's registry, and the cloud's
//! transfers. Each says what it's working on, the step, how far (n of m, or
//! bytes) and since when.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::model::now_ms;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Work {
    pub key: String,
    /// The script it's on, to open it.
    pub script: Option<String>,
    /// What: "Importing", "Claude: fixing review notes", "Sharing footage with Edit"…
    pub what: String,
    /// On what: the script's title.
    pub title: String,
    pub step: String,
    pub done: u64,
    pub total: u64,
    /// `count` (n of m) or `bytes`.
    pub unit: &'static str,
    pub started: i64,
}

static WORK: Mutex<BTreeMap<String, Work>> = Mutex::new(BTreeMap::new());

/// Says how a piece of work is going (made on its first word).
pub fn set(key: &str, script: Option<&str>, what: &str, title: &str, step: &str, done: u64, total: u64, unit: &'static str) {
    if let Ok(mut work) = WORK.lock() {
        let started = work.get(key).map_or_else(now_ms, |known| known.started);
        work.insert(key.to_string(), Work { key: key.to_string(), script: script.map(str::to_string), what: what.to_string(), title: title.to_string(), step: step.to_string(), done, total, unit, started });
    }
}

pub fn end(key: &str) {
    if let Ok(mut work) = WORK.lock() {
        work.remove(key);
    }
}

/// Everything under way: what was set here, the imports, Claude's jobs and the cloud's transfer.
pub fn list() -> Vec<Work> {
    let mut out: Vec<Work> = WORK.lock().map(|work| work.values().cloned().collect()).unwrap_or_default();
    for import in crate::import::latest_progress() {
        out.push(Work {
            key: format!("import:{}", import.id),
            script: Some(import.id.clone()),
            what: "Importing".into(),
            title: import.title,
            step: if import.detail.is_empty() { import.stage } else { import.detail },
            done: u64::from(import.percent),
            total: 100,
            unit: "percent",
            started: 0,
        });
    }
    for (id, title, job) in crate::jobs::running_list() {
        let (done, total) = n_of_m(&job.stage).unwrap_or((0, 0));
        out.push(Work { key: format!("job:{id}"), script: Some(id), what: "Claude".into(), title, step: job.stage, done, total, unit: "count", started: job.started_at });
    }
    let cloud = crate::cloud::status();
    if let Some(moving) = cloud.transfer {
        out.push(Work {
            key: "cloud".into(),
            script: Some(moving.script.clone()),
            what: if moving.up { "Uploading to the cloud".into() } else { "Downloading from the cloud".into() },
            title: moving.name,
            step: format!("{} of {}", crate::log::size(moving.done), crate::log::size(moving.total)),
            done: moving.done,
            total: moving.total,
            unit: "bytes",
            started: 0,
        });
    }
    out
}

/// "Fixing note 2 of 5" → (2, 5).
fn n_of_m(text: &str) -> Option<(u64, u64)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words.windows(3).find_map(|three| {
        let done = three[0].trim_matches(|c: char| !c.is_ascii_digit()).parse().ok()?;
        (three[1] == "of").then_some(())?;
        let total = three[2].trim_matches(|c: char| !c.is_ascii_digit()).parse().ok()?;
        Some((done, total))
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_how_far_from_a_stage() {
        assert_eq!(super::n_of_m("Fixing note 2 of 5"), Some((2, 5)));
        assert_eq!(super::n_of_m("Transcribing the reused case · 3 of 12"), Some((3, 12)));
        assert_eq!(super::n_of_m("Writing the script"), None);
    }
}
