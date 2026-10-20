mod casefiles;
mod chat;
mod claude;
mod cloud;
mod commentary;
mod doctor;
mod drive;
mod formats;
mod http;
mod import;
mod jobs;
mod media;
mod net;
mod model;
mod pages;
mod proc;
mod research;
mod reuse;
mod revise;
mod settings;
mod speakers;
mod store;
mod tidy;
mod tools;
mod transcribe;
mod versions;
mod watch;
mod youtube;

use std::path::{Path, PathBuf};

use model::{now_ms, Clip, ScriptDetail};
use tauri::{AppHandle, Manager};

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|error| error.to_string())
}

#[tauri::command]
fn list_scripts(app: AppHandle) -> Result<Vec<model::Summary>, String> {
    let mut rows = store::list(&data_dir(&app)?)?;
    for row in &mut rows {
        row.building = jobs::running(&row.id);
    }
    Ok(rows)
}

#[tauri::command]
fn get_script(app: AppHandle, id: String) -> Result<ScriptDetail, String> {
    let data = data_dir(&app)?;
    let dir = store::script_dir(&data, &id)?;
    Ok(store::detail(store::read(&dir)?, &dir))
}

#[tauri::command]
async fn import_file(app: AppHandle, path: String, article: Option<String>) -> Result<ScriptDetail, String> {
    // Off the window's thread: an import takes minutes.
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_file(&app, &data, Path::new(&path), article.as_deref())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn import_youtube(app: AppHandle, url: String, article: Option<String>) -> Result<ScriptDetail, String> {
    // Off the window's thread: an import takes minutes.
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_youtube(&app, &data, &url, article.as_deref())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn import_drive(app: AppHandle, url: String, article: Option<String>) -> Result<ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_drive(&app, &data, &url, article.as_deref())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Answers a paused import (or retries a failed one) and carries it on.
#[tauri::command]
async fn resume_import(app: AppHandle, id: String, choice: import::Choice) -> Result<ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::resume(&app, &data, &id, choice)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn import_progress() -> Vec<model::ImportProgress> {
    import::latest_progress()
}

#[tauri::command]
async fn doctor(app: AppHandle) -> Result<Vec<doctor::Check>, String> {
    tauri::async_runtime::spawn_blocking(move || data_dir(&app).map(|data| doctor::run(&data)))
        .await
        .map_err(|error| error.to_string())?
}

/// Signs in to Repack's Drive account without downloading anything.
#[tauri::command]
async fn check_drive(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        drive::access_token(&settings::load(&data_dir(&app)?)).map(|_| "Google Drive is connected.".to_string())
    })
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn import_compilation(app: AppHandle, title: String, urls: Vec<String>, case: Option<bool>) -> Result<ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_compilation(&app, &data, &title, &urls, case.unwrap_or(false))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn get_research(app: AppHandle, id: String) -> Result<Option<research::Research>, String> {
    Ok(research_shown(&store::script_dir(&data_dir(&app)?, &id)?))
}

/// The research the page lists: the case folder's PDFs first, then what was searched for.
pub fn research_shown(dir: &Path) -> Option<research::Research> {
    match (research::load(dir), casefiles::load(dir).filter(|documents| !documents.read.is_empty())) {
        (None, None) => None,
        (found, _) => Some(casefiles::with_documents(dir, found.unwrap_or_default())),
    }
}

/// Looks for the case's news again. Reads the script, then searches.
#[tauri::command]
async fn research_again(app: AppHandle, id: String) -> Result<research::Research, String> {
    tauri::async_runtime::spawn_blocking(move || research_now(&data_dir(&app)?, &id))
        .await
        .map_err(|error| error.to_string())?
}

pub fn research_now(data: &Path, id: &str) -> Result<research::Research, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    if script.words.is_empty() {
        return Err("Transcribe the footage before researching it.".into());
    }
    Ok(casefiles::with_documents(&dir, research::run(&script, &dir)))
}

#[tauri::command]
async fn create_commentary(app: AppHandle, title: String, links: Vec<String>) -> Result<ScriptDetail, String> {
    // Off the window's thread: reading the links takes a moment.
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        commentary::create(&data, &title, &links, &settings::load(&data))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn add_source(app: AppHandle, id: String, adding: commentary::Adding, role: commentary::Role) -> Result<ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || add_source_now(&data_dir(&app)?, &id, adding, role))
        .await
        .map_err(|error| error.to_string())?
}

pub fn add_source_now(data: &Path, id: &str, adding: commentary::Adding, role: commentary::Role) -> Result<ScriptDetail, String> {
    if jobs::running(id) {
        return Err("Claude is writing this script. Add sources once it's done.".into());
    }
    commentary::add(data, id, adding, role, &settings::load(data))
}

#[tauri::command]
fn update_source(app: AppHandle, id: String, source: String, role: Option<commentary::Role>, title: Option<String>) -> Result<ScriptDetail, String> {
    commentary::update(&data_dir(&app)?, &id, &source, role, title)
}

#[tauri::command]
fn remove_source(app: AppHandle, id: String, source: String) -> Result<ScriptDetail, String> {
    if jobs::running(&id) {
        return Err("Claude is writing this script. Remove sources once it's done.".into());
    }
    commentary::remove(&data_dir(&app)?, &id, &source)
}

#[tauri::command]
fn source_text(app: AppHandle, id: String, source: String) -> Result<String, String> {
    commentary::read_text(&store::script_dir(&data_dir(&app)?, &id)?, &source)
}

#[tauri::command]
fn save_draft(app: AppHandle, id: String, revision: u32, draft: commentary::Draft) -> Result<ScriptDetail, String> {
    if jobs::running(&id) {
        return Err("Claude is writing this script.".into());
    }
    commentary::save(&data_dir(&app)?, &id, revision, draft)
}

/// Scripts being exported now, so a second click can't make a second Doc.
fn exporting() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static EXPORTING: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    EXPORTING.get_or_init(Default::default)
}

/// Exports the script as it stands to a new Google Doc, as Repack does: the
/// page lays it out as a Word file with a comment on each timestamp, the
/// relay makes the Doc and opens it to anyone with its link. A full export is
/// remembered as the script's Doc; a draft ("(DRAFT) …") is not.
pub fn export_doc_now(data: &Path, id: &str, draft: bool, docx: &str) -> Result<serde_json::Value, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    if docx.is_empty() || docx.len() > 11_000_000 {
        return Err("That script is too large to export.".into());
    }
    if !exporting().lock().map_err(|_| "Busy.".to_string())?.insert(id.to_string()) {
        return Err("This script is already being exported. Try again in a moment.".into());
    }
    let made = net::Relay::new(&settings::load(data)).and_then(|relay| relay.export_doc(&script.title, id, draft, docx));
    if let Ok(mut busy) = exporting().lock() {
        busy.remove(id);
    }
    let made = made?;
    if !draft {
        let doc_id = made["id"].as_str().unwrap_or("").to_string();
        let url = made["url"].as_str().unwrap_or("").to_string();
        // Read again: an edit made while Google worked stays.
        let mut script = store::read(&dir)?;
        script.doc_export = Some(model::DocExport { id: doc_id, url, revision: script.revision, exported_at: now_ms() });
        store::write(&dir, &script)?;
    }
    Ok(made)
}

#[tauri::command]
async fn export_doc(app: AppHandle, id: String, draft: bool, docx: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || export_doc_now(&data_dir(&app)?, &id, draft, &docx))
        .await
        .map_err(|error| error.to_string())?
}

/// Every Doc exported from the script, read from Drive each time.
pub fn list_docs_now(data: &Path, id: &str) -> Result<serde_json::Value, String> {
    store::script_dir(data, id)?;
    net::Relay::new(&settings::load(data))?.list_docs(id)
}

#[tauri::command]
async fn list_docs(app: AppHandle, id: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || list_docs_now(&data_dir(&app)?, &id))
        .await
        .map_err(|error| error.to_string())?
}

/// The comments people left on the script's latest Doc, or on one named.
pub fn doc_comments_now(data: &Path, id: &str, doc: Option<&str>) -> Result<serde_json::Value, String> {
    let dir = store::script_dir(data, id)?;
    let relay = net::Relay::new(&settings::load(data))?;
    let doc = match doc {
        Some(doc) => doc.to_string(),
        None => {
            let listed = relay.list_docs(id)?;
            let live = listed.as_array().and_then(|docs| docs.iter().find(|doc| doc["state"] == "live")).and_then(|doc| doc["id"].as_str());
            match live.map(str::to_string).or_else(|| store::read(&dir).ok().and_then(|script| script.doc_export.map(|doc| doc.id))) {
                Some(doc) => doc,
                None => return Err("This script has no exported Doc to read comments from.".into()),
            }
        }
    };
    let comments = relay.doc_comments(id, &doc)?;
    Ok(serde_json::json!({ "doc": doc, "comments": comments }))
}

#[tauri::command]
async fn doc_comments(app: AppHandle, id: String, doc: Option<String>) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || doc_comments_now(&data_dir(&app)?, &id, doc.as_deref()))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn rename_script(app: AppHandle, id: String, title: String) -> Result<ScriptDetail, String> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 180 || title.chars().any(char::is_control) {
        return Err("Give it a title of up to 180 characters.".into());
    }
    let data = data_dir(&app)?;
    let dir = store::script_dir(&data, &id)?;
    let mut script = store::read(&dir)?;
    script.title = title.to_string();
    // The title heads the script, so its Google Doc is out of date now.
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    Ok(store::detail(script, &dir))
}

#[tauri::command]
async fn retry_transcribe(app: AppHandle, id: String) -> Result<ScriptDetail, String> {
    // Off the window's thread: an import takes minutes.
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::transcribe_existing(&app, &data, &id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn save_clips(app: AppHandle, id: String, revision: u32, clips: Vec<Clip>) -> Result<ScriptDetail, String> {
    if import::importing(&id) {
        return Err("This import is still running.".into());
    }
    let data = data_dir(&app)?;
    let dir = store::script_dir(&data, &id)?;
    let mut script = store::read(&dir)?;
    if script.revision != revision {
        return Err("This script changed. Reload it.".into());
    }
    let before = script.clone();
    script.clips = check_clips(clips, script.duration_ms)?;
    // A proposal whose paragraph changed (accepted, or edited by hand) is done.
    script.proposals = revise::settle(&script.clips, std::mem::take(&mut script.proposals));
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    versions::keep(&dir, "you", &before, &script);
    Ok(store::detail(script, &dir))
}

#[tauri::command]
fn delete_script(app: AppHandle, id: String) -> Result<(), String> {
    if import::importing(&id) {
        return Err("This import is still running.".into());
    }
    if jobs::running(&id) {
        return Err("Claude is still writing this script.".into());
    }
    let data = data_dir(&app)?;
    store::remove(&data, &id)?;
    cloud::deleted(&data, &id);
    Ok(())
}

#[tauri::command]
fn cloud_status() -> cloud::Status {
    cloud::status()
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<settings::Settings, String> {
    Ok(settings::load(&data_dir(&app)?))
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: settings::Settings) -> Result<settings::Settings, String> {
    settings::save(&data_dir(&app)?, settings)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayHealth {
    ok: bool,
    transcription: bool,
    drive: bool,
    proxy: bool,
    message: String,
}

/// The relay is up, Writer's token is accepted, and what it can do.
pub fn relay_health(data: &Path) -> Result<RelayHealth, String> {
    let relay = net::Relay::new(&settings::load(data))?;
    let value = relay.health()?;
    let flag = |key: &str| value[key].as_bool().unwrap_or(false);
    let (transcription, drive, proxy) = (flag("transcription"), flag("drive"), flag("proxy"));
    // /health is open; asking for the proxy proves the token works.
    relay.proxy()?;
    let missing: Vec<&str> = [(transcription, "transcription"), (drive, "Google Drive"), (proxy, "the YouTube proxy")]
        .iter()
        .filter(|(on, _)| !on)
        .map(|(_, name)| *name)
        .collect();
    let message = if missing.is_empty() {
        "Connected: transcription, Google Drive and the YouTube proxy are ready.".to_string()
    } else {
        format!("Connected, but {} isn't set up on Writer's service yet.", missing.join(" and "))
    };
    Ok(RelayHealth { ok: flag("ok"), transcription, drive, proxy, message })
}

#[tauri::command]
async fn check_relay(app: AppHandle) -> Result<RelayHealth, String> {
    tauri::async_runtime::spawn_blocking(move || relay_health(&data_dir(&app)?))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn claude_status() -> claude::ClaudeStatus {
    claude::status()
}

/// Claude plans the whole script and the timeline is replaced with it. The
/// script on disk is read again afterwards, so an edit made during the build
/// to anything but the timeline stays. Runs on a build job's thread.
pub fn build_once(app: &AppHandle, id: &str, note: &str, kind: Option<formats::Kind>) -> Result<(String, f64), String> {
    let data = data_dir(app)?;
    let dir = store::script_dir(&data, id)?;
    let mut script = store::read(&dir)?;
    if script.commentary.is_some() {
        return build_commentary(&dir, id, &script, note);
    }
    if let Some(kind) = kind {
        script.kind = kind;
        store::write(&dir, &script)?;
    }
    // Names for the voices, then the news about the case. Neither is needed
    // to write the script, so neither can stop it.
    if script.speakers.is_empty() {
        jobs::stage(id, "Naming the speakers");
        if speakers::identify(&mut script).is_ok() {
            // Read again: only the names and suspects are this step's to write.
            let mut now = store::read(&dir)?;
            now.speakers = script.speakers.clone();
            now.suspects = script.suspects.clone();
            store::write(&dir, &now)?;
        }
    }
    let research = match research::load(&dir).filter(|found| !found.articles.is_empty() || found.problem.is_none()) {
        Some(found) => found,
        // The case folder's incident report gives the facts, as in Repack: no search.
        None if casefiles::load(&dir).is_some_and(|documents| documents.has_report()) => research::Research {
            problem: Some("not searched: the case folder's incident report gives the facts.".into()),
            at: now_ms(),
            ..Default::default()
        },
        None => {
            jobs::stage(id, "Researching the case");
            research::run(&script, &dir)
        }
    };
    let research = casefiles::with_documents(&dir, research);
    // A reused case brought in whose moments weren't picked yet, as Repack
    // picks them before the editor's next turn. A failure leaves them out.
    if script.parts.iter().any(|part| part.reuse.is_some() && part.highlights.is_none()) {
        jobs::stage(id, "Picking the reused cases' moments");
        let _ = reuse::pick_missing(&dir);
        script = store::read(&dir)?;
    }
    // The sound, so pauses with speech the transcript missed are kept.
    let peaks = tidy::peaks(&dir, &footage_path(&dir, &script));
    jobs::stage(id, "Writing the script");
    chat::add(&dir, "producer", &if note.trim().is_empty() { "Build the script.".to_string() } else { format!("Build the script. {}", note.trim()) }, None, false);
    let mut built = match claude::build(&script, note, Some(&research), &peaks) {
        Ok(built) => built,
        Err(error) => {
            chat::add(&dir, "editor", &error, None, true);
            return Err(error);
        }
    };
    let found = match (&research.problem, research.articles.len()) {
        (_, 0) => format!("Research: {}", research.problem.clone().unwrap_or_else(|| "nothing found.".into())),
        (_, n) => format!(
            "Research: {n} {} ({}).",
            if n == 1 { "report" } else { "reports" },
            research.articles.iter().map(|article| article.outlet.as_str()).collect::<Vec<_>>().join(", ")
        ),
    };
    built.report = format!("{}\n\n{found}", built.report.trim());
    let mut script = store::read(&dir)?;
    let before = script.clone();
    script.clips = match check_clips(built.clips, script.duration_ms) {
        Ok(clips) => clips,
        Err(error) => {
            chat::add(&dir, "editor", &error, None, true);
            return Err(error);
        }
    };
    script.proposals.clear();
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    versions::keep(&dir, "claude", &before, &script);
    chat::add(&dir, "editor", &built.report, Some(built.cost_usd), false);
    Ok((built.report, built.cost_usd))
}

/// The footage a script plays: its original where it is still on this Mac,
/// else its preview.
fn footage_path(dir: &Path, script: &model::Script) -> PathBuf {
    let original = PathBuf::from(&script.original_path);
    if original.is_file() { original } else { store::proxy_path(dir) }
}

/// A commentary script is written from its sources alone: no speakers to
/// name or case to research.
fn build_commentary(dir: &Path, id: &str, script: &model::Script, note: &str) -> Result<(String, f64), String> {
    jobs::stage(id, "Writing the script");
    let built = commentary::build(dir, script, note)?;
    // Sources added or renamed during the build stay; the script is replaced.
    let mut script = store::read(dir)?;
    if let Some(found) = script.commentary.as_mut() {
        found.draft = Some(built.draft);
    }
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(dir, &script)?;
    Ok((built.report, built.cost_usd))
}

#[tauri::command]
async fn check_videos(app: AppHandle, lines: Vec<String>) -> Result<Vec<import::VideoCheck>, String> {
    tauri::async_runtime::spawn_blocking(move || import::check_videos(&data_dir(&app)?, &lines))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn start_build(app: AppHandle, id: String, note: String, kind: Option<formats::Kind>) -> Result<jobs::BuildStatus, String> {
    store::script_dir(&data_dir(&app)?, &id)?;
    jobs::start(app, id, jobs::Work::Build { note, kind })
}

/// Sonnet revises a built footage script with targeted edits, as Repack's
/// chat editor does. Runs on a job's thread; the page follows it as a build.
pub fn revise_once(app: &AppHandle, id: &str, request: &str, focus: &revise::Focus) -> Result<(String, f64), String> {
    let data = data_dir(app)?;
    let dir = store::script_dir(&data, id)?;
    let script = store::read(&dir)?;
    if script.commentary.is_some() {
        return Err("A commentary script is rewritten from its sources; use Rewrite.".into());
    }
    if !script.clips.iter().any(|clip| clip.is_cut_in()) {
        return Err("Build the script first; then ask for changes.".into());
    }
    let earlier = chat::recent(&dir);
    chat::add(&dir, "producer", request, None, false);
    let script = if script.parts.iter().any(|part| part.reuse.is_some() && part.highlights.is_none()) {
        jobs::stage(id, "Picking the reused cases' moments");
        let _ = reuse::pick_missing(&dir);
        store::read(&dir)?
    } else {
        script
    };
    let peaks = tidy::peaks(&dir, &footage_path(&dir, &script));
    let research = casefiles::with_documents(&dir, research::load(&dir).unwrap_or_default());
    let done = revise::revise(&script, request, focus, &earlier, Some(&research), &peaks);
    let done = match done {
        Ok(done) => done,
        Err(error) => {
            chat::add(&dir, "editor", &error, None, true);
            return Err(error);
        }
    };
    // Edits made by hand while Claude worked would be lost: ask again instead.
    let mut now = store::read(&dir)?;
    if now.revision != script.revision {
        let error = "The script changed while Claude was editing it, so nothing was applied. Ask again.".to_string();
        chat::add(&dir, "editor", &error, None, true);
        return Err(error);
    }
    let before = now.clone();
    now.clips = match check_clips(done.clips, now.duration_ms) {
        Ok(clips) => clips,
        Err(error) => {
            let error = format!("Claude's edits left the timeline unusable ({error}), so nothing was applied. Ask again.");
            chat::add(&dir, "editor", &error, None, true);
            return Err(error);
        }
    };
    now.corrections = done.corrections;
    now.proposals = done.proposals;
    now.revision += 1;
    now.updated_at = now_ms();
    store::write(&dir, &now)?;
    versions::keep(&dir, "claude", &before, &now);
    chat::add(&dir, "editor", &done.answer, Some(done.cost_usd), false);
    Ok((done.answer, done.cost_usd))
}

#[tauri::command]
fn start_revision(app: AppHandle, id: String, request: String, focus: Option<revise::Focus>) -> Result<jobs::BuildStatus, String> {
    store::script_dir(&data_dir(&app)?, &id)?;
    jobs::start(app, id, jobs::Work::Revise { request, focus: focus.unwrap_or_default() })
}

/// Brings a compilation's reused cases' videos into its footage, for the
/// opening's intro clips. Runs as a job; the page follows it as a build.
pub fn start_reuse_import_now(app: AppHandle, data: &Path, id: String, urls: Vec<String>) -> Result<jobs::BuildStatus, String> {
    let dir = store::script_dir(data, &id)?;
    let script = store::read(&dir)?;
    let urls = if urls.is_empty() { reuse::missing(&script) } else { urls };
    if urls.is_empty() {
        return Err("Every reused case is already imported.".into());
    }
    jobs::start(app, id, jobs::Work::Reuse { urls })
}

#[tauri::command]
fn start_reuse_import(app: AppHandle, id: String, urls: Option<Vec<String>>) -> Result<jobs::BuildStatus, String> {
    let data = data_dir(&app)?;
    start_reuse_import_now(app, &data, id, urls.unwrap_or_default())
}

#[tauri::command]
fn get_chat(app: AppHandle, id: String) -> Result<Vec<chat::Turn>, String> {
    Ok(chat::read(&store::script_dir(&data_dir(&app)?, &id)?))
}

pub fn clear_chat_now(data: &Path, id: &str) -> Result<(), String> {
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    chat::clear(&store::script_dir(data, id)?)
}

#[tauri::command]
fn clear_chat(app: AppHandle, id: String) -> Result<(), String> {
    clear_chat_now(&data_dir(&app)?, &id)
}

/// Declines a proposed rewrite: the saved narration stays.
pub fn decline_proposal_now(data: &Path, id: &str, proposal: &str) -> Result<ScriptDetail, String> {
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    script.proposals.retain(|item| item.id != proposal);
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    Ok(store::detail(script, &dir))
}

#[tauri::command]
fn decline_proposal(app: AppHandle, id: String, proposal: String) -> Result<ScriptDetail, String> {
    decline_proposal_now(&data_dir(&app)?, &id, &proposal)
}

/// Respells a transcript word, or puts it back with an empty text. Its timing never changes.
pub fn correct_word_now(data: &Path, id: &str, revision: u32, word: &str, text: &str) -> Result<ScriptDetail, String> {
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    if script.revision != revision {
        return Err("This script changed. Reload it.".into());
    }
    let Some(found) = script.words.iter().find(|item| item.id == word) else {
        return Err("That word is not in the transcript.".into());
    };
    let text = text.trim();
    if text.chars().count() > 200 {
        return Err("Keep the word under 200 characters.".into());
    }
    let before = script.clone();
    if text.is_empty() || text == found.text {
        script.corrections.remove(word);
    } else {
        script.corrections.insert(word.to_string(), text.to_string());
    }
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    versions::keep(&dir, "you", &before, &script);
    Ok(store::detail(script, &dir))
}

#[tauri::command]
fn correct_word(app: AppHandle, id: String, revision: u32, word: String, text: String) -> Result<ScriptDetail, String> {
    correct_word_now(&data_dir(&app)?, &id, revision, &word, &text)
}

/// The producer's own heading in place of the generated one; empty puts the generated one back.
pub fn set_heading_now(data: &Path, id: &str, revision: u32, heading: &str) -> Result<ScriptDetail, String> {
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    if script.revision != revision {
        return Err("This script changed. Reload it.".into());
    }
    let heading = heading.trim();
    if heading.chars().count() > 300 || heading.chars().any(char::is_control) {
        return Err("Keep the heading to one line of up to 300 characters.".into());
    }
    let before = script.clone();
    script.heading = (!heading.is_empty()).then(|| heading.to_string());
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    versions::keep(&dir, "you", &before, &script);
    Ok(store::detail(script, &dir))
}

#[tauri::command]
fn set_heading(app: AppHandle, id: String, revision: u32, heading: String) -> Result<ScriptDetail, String> {
    set_heading_now(&data_dir(&app)?, &id, revision, &heading)
}

pub fn list_versions_now(data: &Path, id: &str) -> Result<Vec<versions::Version>, String> {
    Ok(versions::list(&store::script_dir(data, id)?))
}

#[tauri::command]
fn list_versions(app: AppHandle, id: String) -> Result<Vec<versions::Version>, String> {
    list_versions_now(&data_dir(&app)?, &id)
}

pub fn get_version_now(data: &Path, id: &str, version: &str) -> Result<versions::Snapshot, String> {
    versions::read(&store::script_dir(data, id)?, version)
}

#[tauri::command]
fn get_version(app: AppHandle, id: String, version: String) -> Result<versions::Snapshot, String> {
    get_version_now(&data_dir(&app)?, &id, &version)
}

/// Makes an earlier version the script again; the restore is a version too.
/// Spelling fixes on words the transcript no longer has are left out.
pub fn restore_version_now(data: &Path, id: &str, revision: u32, version: &str) -> Result<ScriptDetail, String> {
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    if script.revision != revision {
        return Err("This script changed. Reload it.".into());
    }
    let snapshot = versions::read(&dir, version)?;
    let at = versions::list(&dir).iter().find(|item| item.id == version).map(|item| item.at).unwrap_or(0);
    let before = script.clone();
    script.clips = check_clips(snapshot.clips, script.duration_ms)?;
    script.corrections = snapshot.corrections.into_iter().filter(|(word, _)| script.words.iter().any(|item| &item.id == word)).collect();
    script.heading = snapshot.heading;
    script.proposals = revise::settle(&script.clips, std::mem::take(&mut script.proposals));
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    versions::keep_restored(&dir, &before, &script, at);
    Ok(store::detail(script, &dir))
}

#[tauri::command]
fn restore_version(app: AppHandle, id: String, revision: u32, version: String) -> Result<ScriptDetail, String> {
    restore_version_now(&data_dir(&app)?, &id, revision, &version)
}

#[tauri::command]
fn build_status(app: AppHandle, id: String) -> Option<jobs::BuildStatus> {
    jobs::status(&data_dir(&app).ok()?, &id)
}

/// Timeline items stay in the order given: a hook plays footage from anywhere.
fn check_clips(clips: Vec<Clip>, duration_ms: u64) -> Result<Vec<Clip>, String> {
    if clips.len() > 500 {
        return Err("Too many clips.".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for clip in &clips {
        let bad_id = clip.id.is_empty()
            || clip.id.len() > 80
            || clip.id.chars().any(|c| c.is_control() || c.is_whitespace())
            || !seen.insert(clip.id.clone());
        let bad_range = clip.end_ms <= clip.start_ms
            || (!clip.is_cut_in() && duration_ms > 0 && clip.end_ms > duration_ms);
        if bad_id || bad_range {
            return Err("A clip has invalid boundaries.".into());
        }
        if clip.notes.len() > 50 || clip.notes.iter().any(|note| note.text.trim().is_empty() || note.text.chars().count() > 2_000 || note.id.is_empty() || note.id.len() > 80) {
            return Err("A timestamp holds up to 50 comments of up to 2,000 characters.".into());
        }
        match clip.kind.as_deref() {
            None => {
                if clip.narration.is_some() || clip.narration_role.is_some() || clip.reuse.is_some() {
                    return Err("Only a cut-in holds narration.".into());
                }
            }
            Some("cut-in") => {
                if clip.narration.as_deref().unwrap_or("").chars().count() > 6_000 {
                    return Err("A cut-in holds at most 6,000 characters.".into());
                }
                if !matches!(clip.narration_role.as_deref(), None | Some("intro" | "bridge" | "conclusion")) {
                    return Err("Unknown narration role.".into());
                }
            }
            Some(_) => return Err("Unknown timeline item.".into()),
        }
    }
    Ok(clips)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if let Ok(dir) = app.path().app_data_dir() {
                let _ = std::fs::create_dir_all(dir.join("scripts"));
                store::mark_interrupted(&dir);
                tools::init(&dir);
                // Fetch or refresh Writer's own yt-dlp before anyone needs it.
                std::thread::spawn(|| {
                    let _ = tools::ensure_yt_dlp();
                });
            }
            // The dev page on Tailscale talks to this. It stays on localhost;
            // Vite is what listens on the tailnet.
            http::start(app.handle().clone());
            // Every script in the cloud, pushed and pulled in the background.
            if let Ok(dir) = app.path().app_data_dir() {
                cloud::start(dir);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_scripts,
            get_script,
            import_file,
            import_youtube,
            import_drive,
            import_compilation,
            get_research,
            research_again,
            resume_import,
            import_progress,
            doctor,
            check_drive,
            rename_script,
            retry_transcribe,
            save_clips,
            delete_script,
            get_settings,
            save_settings,
            check_relay,
            claude_status,
            check_videos,
            start_build,
            start_revision,
            start_reuse_import,
            build_status,
            get_chat,
            clear_chat,
            decline_proposal,
            correct_word,
            set_heading,
            list_versions,
            get_version,
            restore_version,
            create_commentary,
            add_source,
            update_source,
            remove_source,
            source_text,
            save_draft,
            export_doc,
            list_docs,
            doc_comments,
            cloud_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running writer");
}
