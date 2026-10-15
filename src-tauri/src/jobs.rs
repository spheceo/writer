//! Script builds run on their own thread. A page on the tailnet asks how a
//! build is going rather than holding one request open for ten minutes, so a
//! dropped connection, a sleeping laptop or a reload does not lose the build.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::thread;

use serde::Serialize;
use tauri::AppHandle;

use crate::model::now_ms;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildStatus {
    pub running: bool,
    /// What the build is doing now, for the page to show.
    pub stage: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub report: Option<String>,
    pub cost_usd: Option<f64>,
    pub error: Option<String>,
}

fn jobs() -> &'static Mutex<HashMap<String, BuildStatus>> {
    static JOBS: OnceLock<Mutex<HashMap<String, BuildStatus>>> = OnceLock::new();
    JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The latest build of a script, running or finished, since the app started.
pub fn status(id: &str) -> Option<BuildStatus> {
    jobs().lock().ok()?.get(id).cloned()
}

pub fn running(id: &str) -> bool {
    status(id).is_some_and(|job| job.running)
}

/// Says what a running build is doing.
pub fn stage(id: &str, text: &str) {
    if let Ok(mut jobs) = jobs().lock() {
        if let Some(job) = jobs.get_mut(id) {
            job.stage = text.to_string();
        }
    }
}

pub fn start(app: AppHandle, id: String, note: String, kind: Option<crate::formats::Kind>) -> Result<BuildStatus, String> {
    if crate::import::importing(&id) {
        return Err("This import is still running.".into());
    }
    let job = {
        let mut jobs = jobs().lock().map_err(|_| "Busy.".to_string())?;
        if jobs.get(&id).is_some_and(|job| job.running) {
            return Err("Claude is already writing this script.".into());
        }
        let job = BuildStatus {
            running: true,
            stage: "Starting".into(),
            started_at: now_ms(),
            finished_at: None,
            report: None,
            cost_usd: None,
            error: None,
        };
        jobs.insert(id.clone(), job.clone());
        job
    };
    thread::spawn(move || {
        // A bug in a build ends it as a failed build, never one that reads
        // as running forever.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::build_once(&app, &id, &note, kind)))
            .unwrap_or_else(|_| Err("Writer hit a bug while building. Nothing was changed; build again.".into()));
        if let Ok(mut jobs) = jobs().lock() {
            if let Some(job) = jobs.get_mut(&id) {
                job.running = false;
                job.finished_at = Some(now_ms());
                match result {
                    Ok((report, cost)) => {
                        job.report = Some(report);
                        job.cost_usd = Some(cost);
                    }
                    Err(error) => job.error = Some(error),
                }
            }
        }
    });
    Ok(job)
}
