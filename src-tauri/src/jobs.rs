//! Script builds run on their own thread. A page on the tailnet asks how a
//! build is going rather than holding one request open for ten minutes, so a
//! dropped connection, a sleeping laptop or a reload does not lose the build.
//! Each build's state is also kept beside its script (`build.json`), so one
//! that finished while no page watched still says how it went, and one that
//! Writer closed on says so instead of vanishing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::model::now_ms;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn file(data: &Path, id: &str) -> Option<PathBuf> {
    crate::store::script_dir(data, id).ok().map(|dir| dir.join("build.json"))
}

fn keep(data: &Path, id: &str, job: &BuildStatus) {
    let Some(path) = file(data, id) else { return };
    let tmp = path.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(job) {
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(tmp, path);
        }
    }
}

/// The latest build of a script, running or finished: this run's, else the
/// one kept on disk from before.
pub fn status(data: &Path, id: &str) -> Option<BuildStatus> {
    if let Some(job) = jobs().lock().ok()?.get(id).cloned() {
        return Some(job);
    }
    let job: BuildStatus = serde_json::from_slice(&fs::read(file(data, id)?).ok()?).ok()?;
    // Running on disk but not here: Writer closed on it (and startup missed it).
    Some(if job.running { interrupted(job) } else { job })
}

fn interrupted(mut job: BuildStatus) -> BuildStatus {
    job.running = false;
    job.finished_at = Some(job.finished_at.unwrap_or_else(now_ms));
    job.error = Some("Writer closed while Claude was writing this script, so the build stopped. Nothing was changed; build again.".into());
    job
}

/// A build that died with the app is not still running. Run at startup.
pub fn mark_interrupted(data: &Path, id: &str) {
    let Some(path) = file(data, id) else { return };
    let Ok(bytes) = fs::read(&path) else { return };
    let Ok(job) = serde_json::from_slice::<BuildStatus>(&bytes) else { return };
    if job.running {
        keep(data, id, &interrupted(job));
    }
}

pub fn running(id: &str) -> bool {
    jobs().lock().ok().and_then(|jobs| jobs.get(id).map(|job| job.running)).unwrap_or(false)
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
    let data = crate::data_dir(&app)?;
    keep(&data, &id, &job);
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
                keep(&data, &id, job);
            }
        }
    });
    Ok(job)
}
