//! Script builds run on their own thread. A page on the tailnet asks how a
//! build is going rather than holding one request open for ten minutes, so a
//! dropped connection, a sleeping laptop or a reload does not lose the build.
//! Each build's state is also kept beside its script (`build.json`), so one
//! that finished while no page watched still says how it went, and one that
//! Writer closed on says so instead of vanishing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::model::now_ms;

thread_local! { static CANCELLING: std::cell::RefCell<Option<Arc<AtomicBool>>> = const { std::cell::RefCell::new(None) }; }
fn cancellations() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static FLAGS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub fn cancelled() -> bool { CANCELLING.with(|slot| slot.borrow().as_ref().is_some_and(|flag| flag.load(Ordering::Relaxed))) }
pub(crate) fn cancellation_flag() -> Option<Arc<AtomicBool>> { CANCELLING.with(|slot| slot.borrow().clone()) }
pub fn check_cancelled() -> Result<(), String> { if cancelled() { Err("Stopped by you. Completed work is saved.".into()) } else { Ok(()) } }
pub fn wait(duration: std::time::Duration) -> Result<(), String> {
    let until = std::time::Instant::now() + duration;
    loop {
        check_cancelled()?;
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() { return Ok(()); }
        std::thread::sleep(left.min(std::time::Duration::from_millis(120)));
    }
}

/// Runs `run` on this thread under a job's stop flag: a thread a job starts
/// (the preview made beside the words) stops when the job does.
pub(crate) fn with_cancellation<T>(flag: Arc<AtomicBool>, run: impl FnOnce() -> T) -> T {
    CANCELLING.with(|slot| *slot.borrow_mut() = Some(flag));
    let result = run();
    CANCELLING.with(|slot| *slot.borrow_mut() = None);
    result
}

pub fn stop(data: &Path, id: &str) -> Result<(), String> {
    crate::can::require_id(data, id)?;
    if !running(id) { return Ok(()); }
    let flags = cancellations().lock().map_err(|_| "The job lock failed.")?;
    let flag = flags.get(id).ok_or("This job is already finishing.")?;
    flag.store(true, Ordering::Relaxed);
    drop(flags);
    stage(id, "Stopping…");
    Ok(())
}

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

/// The jobs running now, each with its script's title.
pub fn running_list() -> Vec<(String, String, BuildStatus)> {
    let titles = titles().lock().map(|titles| titles.clone()).unwrap_or_default();
    jobs()
        .lock()
        .map(|jobs| jobs.iter().filter(|(_, job)| job.running).map(|(id, job)| (id.clone(), titles.get(id).cloned().unwrap_or_default(), job.clone())).collect())
        .unwrap_or_default()
}

fn titles() -> &'static Mutex<HashMap<String, String>> {
    static TITLES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    TITLES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A script's title, read alone from its script.json.
pub fn title_of(data: &Path, id: &str) -> String {
    #[derive(Deserialize)]
    struct Slim {
        title: String,
    }
    crate::store::script_dir(data, id)
        .ok()
        .and_then(|dir| fs::read(dir.join("script.json")).ok())
        .and_then(|bytes| serde_json::from_slice::<Slim>(&bytes).ok())
        .map(|slim| slim.title)
        .unwrap_or_default()
}

pub fn running(id: &str) -> bool {
    jobs().lock().ok().and_then(|jobs| jobs.get(id).map(|job| job.running)).unwrap_or(false)
}

/// Says what a running build is doing (a new step goes in the log too;
/// a percentage ticking over doesn't).
pub fn stage(id: &str, text: &str) {
    let mut fresh = false;
    if let Ok(mut jobs) = jobs().lock() {
        if let Some(job) = jobs.get_mut(id) {
            fresh = job.stage != text && !text.contains('%');
            job.stage = text.to_string();
        }
    }
    if fresh {
        let title = titles().lock().ok().and_then(|titles| titles.get(id).cloned()).unwrap_or_default();
        crate::log::info("agent", format!("{}: {text}", crate::log::named(id, &title)));
    }
}

/// What a job does: write the whole script, revise a built one, bring a
/// compilation's reused cases into its footage, or find a commentary
/// script's clips.
pub enum Work {
    Build { note: String, kind: Option<crate::formats::Kind> },
    Revise { request: String, focus: crate::revise::Focus },
    Reuse { urls: Vec<String> },
    /// Search YouTube for a commentary script's clips.
    Find,
    /// A step in learning from a review (feedback.rs).
    Learn(crate::feedback::Step),
    /// Fix a shared script from its reviewers' notes (fixing.rs). `ready`:
    /// the review whose "Ready for changes" asked for it, shared again after.
    Review { notes: Vec<String>, ready: Option<String> },
    /// Bring an archived script's footage back (archive.rs).
    Restore,
    /// Explicitly retry only visual speaker checks that did not finish.
    Speakers,
    /// Hear a built captioned script's footage exactly and cut it again (exact.rs).
    Exact,
    /// Fill missing words in an already imported project, captions first.
    Transcribe,
}

/// What the signed-in person may do to this script (can.rs): see it, write
/// it as the format asked for, fix its review notes, teach from it.
fn allowed(app: &AppHandle, id: &str, work: &Work) -> Result<(), String> {
    use tauri::Manager;
    let data = app.path().app_data_dir().map_err(|error| error.to_string())?;
    let script = crate::store::read(&crate::store::script_dir(&data, id)?)?;
    crate::can::require_script(&script)?;
    match work {
        Work::Build { kind: Some(kind), .. } => crate::can::require_kind(&script, *kind),
        Work::Review { .. } => crate::can::require(crate::can::SHARE),
        Work::Learn(_) => crate::can::require(crate::can::LESSONS),
        _ => Ok(()),
    }
}

pub fn start(app: AppHandle, id: String, work: Work) -> Result<BuildStatus, String> {
    if crate::import::importing(&id) {
        return Err("This import is still running.".into());
    }
    allowed(&app, &id, &work)?;
    let job = {
        let mut jobs = jobs().lock().map_err(|_| "Busy.".to_string())?;
        if jobs.get(&id).is_some_and(|job| job.running) {
            return Err("Claude is already writing this script.".into());
        }
        let job = BuildStatus {
            running: true,
            stage: match work {
                Work::Revise { .. } => "Editing the script".into(),
                Work::Reuse { .. } => "Bringing in the reused cases".into(),
                Work::Find => "Finding clips".into(),
                Work::Learn(_) => "Learning from the feedback".into(),
                Work::Review { .. } => "Fixing review notes".into(),
                Work::Restore => "Bringing the footage back".into(),
                Work::Speakers => "Checking speakers".into(),
                Work::Exact => "Hearing the chosen footage exactly".into(),
                Work::Transcribe => "Completing transcription".into(),
                Work::Build { .. } => "Starting".into(),
            },
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
    let title = title_of(&data, &id);
    if let Ok(mut titles) = titles().lock() {
        titles.insert(id.clone(), title.clone());
    }
    let what = match &work {
        Work::Build { .. } => "writing the script".to_string(),
        Work::Revise { request, .. } => format!("revising: \"{}\"", request.chars().take(80).collect::<String>()),
        Work::Reuse { urls } => format!("bringing in {} reused {}", urls.len(), if urls.len() == 1 { "case" } else { "cases" }),
        Work::Find => "finding clips on YouTube".to_string(),
        Work::Learn(_) => "learning from the feedback".to_string(),
        Work::Review { notes, .. } => format!("fixing {} review {}", notes.len(), if notes.len() == 1 { "note" } else { "notes" }),
        Work::Restore => "bringing the footage back".to_string(),
        Work::Speakers => "retrying unfinished speaker checks".to_string(),
        Work::Exact => "hearing the chosen footage exactly".to_string(),
        Work::Transcribe => "completing transcription".to_string(),
    };
    let name = crate::log::named(&id, &title);
    crate::log::info("agent", format!("{name}: started: {what}"));
    let began = std::time::Instant::now();
    let cancelling = Arc::new(AtomicBool::new(false));
    cancellations().lock().map_err(|_| "The job lock failed.")?.insert(id.clone(), cancelling.clone());
    thread::spawn(move || {
        CANCELLING.with(|slot| *slot.borrow_mut() = Some(cancelling));
        // A bug in a build ends it as a failed build, never one that reads
        // as running forever.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &work {
            Work::Build { note, kind } => crate::build_once(&app, &id, note, *kind),
            Work::Revise { request, focus } => crate::revise_once(&app, &id, request, focus),
            Work::Reuse { urls } => crate::reuse::import(&app, &id, urls),
            Work::Find => crate::scout::find(&app, &id),
            Work::Learn(step) => crate::feedback::run(&app, &id, step),
            Work::Review { notes, ready } => crate::fixing::run(&app, &id, notes, ready.as_deref()),
            Work::Restore => crate::archive::restore(&app, &id),
            Work::Speakers => crate::watch::retry(&app, &id),
            Work::Exact => crate::exact_once(&app, &id),
            Work::Transcribe => crate::transcribe::complete(&app, &id),
        }))
        .unwrap_or_else(|_| Err("Writer hit a bug while Claude worked. Nothing was changed; ask again.".into()));
        let result = check_cancelled().and(result);
        if let Ok(mut jobs) = jobs().lock() {
            if let Some(job) = jobs.get_mut(&id) {
                job.running = false;
                job.finished_at = Some(now_ms());
                match result {
                    Ok((report, cost)) => {
                        let first = report.lines().next().unwrap_or("").chars().take(200).collect::<String>();
                        crate::log::info("agent", format!("{name}: done in {} ({what}; ${cost:.2} at API prices): {first}", crate::log::since_start(began)));
                        job.report = Some(report);
                        job.cost_usd = Some(cost);
                        // A commentary's clips may have changed: their footage comes down now.
                        if !matches!(work, Work::Restore | Work::Speakers | Work::Transcribe) {
                            let _ = crate::footage::now(&data, &id);
                        }
                    }
                    Err(error) => {
                        crate::log::error("agent", format!("{name}: stopped after {} ({what}): {error}", crate::log::since_start(began)));
                        job.error = Some(error);
                    }
                }
                keep(&data, &id, job);
                if let Ok(mut flags) = cancellations().lock() { flags.remove(&id); }
            }
        }
        CANCELLING.with(|slot| *slot.borrow_mut() = None);
    });
    Ok(job)
}
