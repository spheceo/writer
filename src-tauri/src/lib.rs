mod brief;
mod captions;
mod casefiles;
mod exact;
mod turns;
mod account;
mod can;
mod activity;
mod agent;
mod avatar;
mod steps;
mod footage;
mod spelling;
mod archive;
mod blocks;
mod chat;
mod claude;
mod cloud;
mod commentary;
#[cfg(test)]
mod distill;
mod doctor;
mod drive;
mod feedback;
mod fixing;
mod formats;
mod handoff;
mod hearing;
mod http;
mod import;
mod jobs;
mod log;
mod machine;
mod media;
mod names;
mod net;
mod model;
mod pages;
mod proc;
mod research;
mod release;
mod house;
mod reuse;
mod reviews;
mod scout;
mod revise;
mod settings;
mod speakers;
mod spills;
mod storage;
mod stream;
mod store;
mod tidy;
mod tools;
mod transcribe;
mod update;
mod trello;
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
    // Scripts of kinds this person can't write are hidden from them.
    let grants = can::grants();
    rows.retain(|row| grants.may_write(can::summary_work(row)));
    for row in &mut rows {
        row.building = jobs::running(&row.id);
    }
    Ok(rows)
}

#[tauri::command]
fn get_script(app: AppHandle, id: String) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    let data = data_dir(&app)?;
    let dir = store::script_dir(&data, &id)?;
    storage::used(&data, &id);
    Ok(store::detail(store::read(&dir)?, &dir))
}

#[tauri::command]
async fn import_file(app: AppHandle, path: String, article: Option<String>, title: Option<String>, kind: Option<formats::Kind>) -> Result<ScriptDetail, String> {
    // Off the window's thread: an import takes minutes.
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_file(&app, &data, Path::new(&path), article.as_deref(), title.as_deref(), kind)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn import_youtube(app: AppHandle, url: String, article: Option<String>, title: Option<String>, kind: Option<formats::Kind>) -> Result<ScriptDetail, String> {
    // Off the window's thread: an import takes minutes.
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_youtube(&app, &data, &url, article.as_deref(), title.as_deref(), kind)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn import_drive(app: AppHandle, url: String, article: Option<String>, title: Option<String>, kind: Option<formats::Kind>) -> Result<ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_drive(&app, &data, &url, article.as_deref(), title.as_deref(), kind)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Answers a paused import (or retries a failed one) and carries it on.
#[tauri::command]
async fn resume_import(app: AppHandle, id: String, choice: import::Choice) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
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
async fn import_compilation(app: AppHandle, title: String, urls: Vec<String>, case: Option<bool>, kind: Option<formats::Kind>) -> Result<ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_compilation(&app, &data, &title, &urls, case.unwrap_or(false), kind)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn read_brief(text: String) -> brief::Brief {
    brief::parse(&text)
}

#[tauri::command]
async fn import_brief(app: AppHandle, text: String, kind: Option<formats::Kind>) -> Result<ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::from_brief(&app, &data, &text, kind)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn get_research(app: AppHandle, id: String) -> Result<Option<research::Research>, String> {
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require(can::COMMENTARY)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
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

pub fn set_pacing_now(data: &Path, id: &str, pacing: commentary::Pacing) -> Result<ScriptDetail, String> {
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    commentary::set_pacing(data, id, pacing)
}

#[tauri::command]
fn set_pacing(app: AppHandle, id: String, pacing: commentary::Pacing) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    set_pacing_now(&data_dir(&app)?, &id, pacing)
}

#[tauri::command]
fn update_source(app: AppHandle, id: String, source: String, role: Option<commentary::Role>, title: Option<String>) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    commentary::update(&data_dir(&app)?, &id, &source, role, title)
}

#[tauri::command]
fn remove_source(app: AppHandle, id: String, source: String) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    if jobs::running(&id) {
        return Err("Claude is writing this script. Remove sources once it's done.".into());
    }
    commentary::remove(&data_dir(&app)?, &id, &source)
}

#[tauri::command]
fn source_text(app: AppHandle, id: String, source: String) -> Result<String, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    commentary::read_text(&store::script_dir(&data_dir(&app)?, &id)?, &source)
}

#[tauri::command]
fn save_draft(app: AppHandle, id: String, revision: u32, draft: commentary::Draft) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require(can::SHARE)?;
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    if docx.is_empty() || docx.len() > 11_000_000 {
        return Err("That script is too large to export.".into());
    }
    if !exporting().lock().map_err(|_| "Busy.".to_string())?.insert(id.to_string()) {
        return Err("This script is already being exported. Try again in a moment.".into());
    }
    let began = std::time::Instant::now();
    let name = log::named(id, &script.title);
    let made = net::Relay::new(&settings::load(data)).and_then(|relay| relay.export_doc(&script.title, id, draft, docx));
    if let Ok(mut busy) = exporting().lock() {
        busy.remove(id);
    }
    match &made {
        Ok(found) => log::info("export", format!("{name}: {} Google Doc made in {}: {}", if draft { "draft" } else { "full" }, log::since_start(began), found["url"].as_str().unwrap_or(""))),
        Err(error) => log::error("export", format!("{name}: Google Doc export failed after {}: {error}", log::since_start(began))),
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
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
    tauri::async_runtime::spawn_blocking(move || doc_comments_now(&data_dir(&app)?, &id, doc.as_deref()))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn rename_script(app: AppHandle, id: String, title: String) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
    // Off the window's thread: an import takes minutes.
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        import::transcribe_existing(&app, &data, &id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn complete_transcription(app: AppHandle, id: String) -> Result<jobs::BuildStatus, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    jobs::start(app, id, jobs::Work::Transcribe)
}

#[tauri::command]
fn save_clips(app: AppHandle, id: String, revision: u32, clips: Vec<Clip>) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
    if import::importing(&id) {
        return Err("This import is still running.".into());
    }
    if jobs::running(&id) {
        return Err("Claude is still writing this script.".into());
    }
    let data = data_dir(&app)?;
    // The cloud's delete is recorded first: a crash between the two can't bring the script back.
    cloud::deleted(&data, &id);
    store::remove(&data, &id)?;
    store::library_changed();
    Ok(())
}

/// A commentary's downloaded footage, and how much is still coming (footage.rs).
#[tauri::command]
async fn get_footage(app: AppHandle, id: String) -> Result<footage::Footage, String> {
    let data = data_dir(&app)?;
    can::require_id(&data, &id)?;
    tauri::async_runtime::spawn_blocking(move || footage::now(&data, &id)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
fn cloud_status() -> cloud::Status {
    cloud::status()
}

/// What Writer holds on this Mac, and how much of it the cloud holds too.
pub fn storage_now(data: &Path) -> storage::Report {
    storage::report(data)
}

#[tauri::command]
fn storage_report(app: AppHandle) -> Result<storage::Report, String> {
    Ok(storage::report(&data_dir(&app)?))
}

/// Clears local footage that is verified in the cloud (previews included);
/// footage still uploading stays, and the answer says which.
pub fn clear_storage_now(data: &Path) -> storage::Cleared {
    storage::purge_all(data, false)
}

#[tauri::command]
async fn clear_storage(app: AppHandle) -> Result<storage::Cleared, String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<storage::Cleared, String> { Ok(storage::purge_all(&data_dir(&app)?, false)) })
        .await
        .map_err(|error| error.to_string())?
}

/// A cleared preview back from the cloud, so the script plays here again.
pub fn restore_preview_now(data: &Path, id: &str) -> Result<model::ScriptDetail, String> {
    can::require_id(data, id)?;
    storage::fetch_preview(data, id)?;
    let dir = store::script_dir(data, id)?;
    Ok(store::detail(store::read(&dir)?, &dir))
}

#[tauri::command]
async fn restore_preview(app: AppHandle, id: String) -> Result<model::ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || restore_preview_now(&data_dir(&app)?, &id)).await.map_err(|error| error.to_string())?
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
        return build_commentary(&data, &dir, id, &script, note);
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
    // Only the producer's brief kept from the import isn't a search yet: the
    // news is still looked for, beside it (research::run keeps it).
    let searched = |found: &research::Research| !found.queries.is_empty() || found.articles.iter().any(|article| article.outlet != research::BRIEF);
    let research = match research::load(&dir).filter(|found| (!found.articles.is_empty() || found.problem.is_none()) && searched(found)) {
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
    // Setup's engine: the Agent SDK builds fast (or in steps, asked for); the CLI in one answer.
    let built = match claude::engine() {
        settings::Engine::Sdk if steps::fast_chosen() => steps::build_fast(&script, note, Some(&research), &peaks),
        settings::Engine::Sdk => steps::build(&script, note, Some(&research), &peaks),
        settings::Engine::Cli => claude::build(&script, note, Some(&research), &peaks),
    };
    let mut built = match built {
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
    jobs::check_cancelled()?;
    let mut script = store::read(&dir)?;
    let before = script.clone();
    let clips = if exact::wanted(&script) {
        let (clips, said) = cut_exactly(&data, &dir, id, &mut script, &built.plan, built.clips, &peaks);
        built.report = format!("{}\n\n{said}", built.report);
        clips
    } else {
        built.clips
    };
    script.clips = match check_clips(clips, script.duration_ms) {
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

/// A script built before its captioned footage was heard exactly, heard and
/// cut again: the same words and narration, read back from its clips
/// (`exact::plan_of`), with the producer's clip notes kept. Runs on a job.
pub fn exact_once(app: &AppHandle, id: &str) -> Result<(String, f64), String> {
    let data = data_dir(app)?;
    let dir = store::script_dir(&data, id)?;
    let mut script = store::read(&dir)?;
    if !exact::wanted(&script) {
        return Err("Only footage transcribed from YouTube's captions needs hearing exactly.".into());
    }
    if !script.clips.iter().any(|clip| clip.is_cut_in()) {
        return Err("Build the script first.".into());
    }
    let before = script.clone();
    let plan = exact::plan_of(&script);
    let peaks = tidy::peaks(&dir, &footage_path(&dir, &script));
    let (mut clips, said) = cut_exactly(&data, &dir, id, &mut script, &plan, before.clips.clone(), &peaks);
    exact::keep_notes(&before.words, &before.clips, &script.words, &mut clips);
    jobs::check_cancelled()?;
    if store::read(&dir)?.revision != before.revision {
        return Err("The script changed while its footage was heard, so nothing was applied. Try again.".into());
    }
    script.clips = check_clips(clips, script.duration_ms)?;
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    versions::keep(&dir, "claude", &before, &script);
    chat::add(&dir, "editor", &said, None, false);
    Ok((said, 0.0))
}

/// A captioned script's chosen footage heard exactly (exact.rs), then the
/// same plan cut again on those times, without any piece that has no sound.
/// Hearing that fails leaves the captions' cut, and says so.
fn cut_exactly(data: &Path, dir: &Path, id: &str, script: &mut model::Script, plan: &serde_json::Value, clips: Vec<model::Clip>, peaks: &[f32]) -> (Vec<model::Clip>, String) {
    let settings = settings::load(data);
    // Footage cleared from this Mac once the cloud held it comes back first.
    if !footage_path(dir, script).is_file() {
        jobs::stage(id, "Bringing the footage back to hear it");
        if let Err(error) = archive::restore_footage(data, id) {
            return (clips, format!("The footage isn't on this Mac and couldn't be brought back ({error}), so its cuts follow YouTube's captions. Build again to try once more."));
        }
    }
    let footage = footage_path(dir, script);
    let words = script.words.clone();
    let heard = match exact::run(script, dir, &footage, &clips, &settings, &|step| jobs::stage(id, step)) {
        Ok(heard) => heard,
        Err(error) => {
            log::warn("build", format!("{}: the chosen footage couldn't be heard exactly: {error}", log::named(id, &script.title)));
            return (clips, format!("The chosen footage couldn't be heard exactly ({error}), so its cuts follow YouTube's captions. Build again to try once more."));
        }
    };
    let mut clips = match claude::compile_with(script, plan, peaks) {
        Ok(done) => done.clips,
        Err(error) => {
            log::warn("build", format!("{}: the plan didn't cut again on the exact times: {error}", log::named(id, &script.title)));
            exact::remap_from(&words, &clips, &script.words, script.duration_ms)
        }
    };
    let silent = exact::silent(&clips, peaks);
    let dropped = silent.len();
    for index in silent.into_iter().rev() {
        clips.remove(index);
    }
    (clips, exact::said(&heard, dropped))
}

/// The footage a script plays: its original where it is still on this Mac,
/// else its preview.
pub(crate) fn footage_path(dir: &Path, script: &model::Script) -> PathBuf {
    let original = PathBuf::from(&script.original_path);
    if original.is_file() { original } else { store::proxy_path(dir) }
}

/// A commentary script is written from its sources alone: no speakers to
/// name or case to research.
fn build_commentary(data: &Path, dir: &Path, id: &str, script: &model::Script, note: &str) -> Result<(String, f64), String> {
    jobs::stage(id, "Writing the script");
    let built = commentary::build(dir, script, note, &feedback::lessons_text(data))?;
    jobs::check_cancelled()?;
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

/// The cards waiting on one of Repack's Trello lists (`raw` or `comp`).
pub(crate) fn trello_now(data: &std::path::Path, list: &str) -> Result<trello::Cards, String> {
    let titles: Vec<String> = store::list(data)?.into_iter().map(|row| row.title.trim().to_lowercase()).collect();
    trello::cards(&settings::load(data), list, &titles)
}

/// Whether downloads have the person's YouTube sign-in to fall back on (youtube.rs).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeSignIn {
    pub saved: bool,
    pub saved_at: Option<u64>,
}

pub fn youtube_sign_in_now(data: &Path) -> YoutubeSignIn {
    let saved_at = std::fs::metadata(youtube::cookies_file(data)).and_then(|meta| meta.modified()).ok().and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok()).map(|at| at.as_millis() as u64);
    YoutubeSignIn { saved: saved_at.is_some(), saved_at }
}

#[tauri::command]
fn youtube_sign_in_status(app: AppHandle) -> Result<YoutubeSignIn, String> {
    Ok(youtube_sign_in_now(&data_dir(&app)?))
}

/// Copies the person's YouTube sign-in from their browser into a file, once.
#[tauri::command]
async fn save_youtube_sign_in(app: AppHandle) -> Result<String, String> {
    let data = data_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        tools::ensure_yt_dlp().ok();
        youtube::sign_in(&data)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The card a pasted Trello link names, read for its videos.
pub fn trello_card_now(data: &Path, link: &str) -> Result<trello::Card, String> {
    let titles: Vec<String> = store::list(data)?.into_iter().map(|row| row.title.trim().to_lowercase()).collect();
    trello::card(&settings::load(data), link, &titles)
}

#[tauri::command]
async fn trello_card(app: AppHandle, link: String) -> Result<trello::Card, String> {
    tauri::async_runtime::spawn_blocking(move || trello_card_now(&data_dir(&app)?, &link))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn trello_cards(app: AppHandle, list: String) -> Result<trello::Cards, String> {
    tauri::async_runtime::spawn_blocking(move || trello_now(&data_dir(&app)?, &list))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn check_videos(app: AppHandle, lines: Vec<String>) -> Result<Vec<import::VideoCheck>, String> {
    tauri::async_runtime::spawn_blocking(move || import::check_videos(&data_dir(&app)?, &lines))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn start_build(app: AppHandle, id: String, note: String, kind: Option<formats::Kind>) -> Result<jobs::BuildStatus, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    store::script_dir(&data_dir(&app)?, &id)?;
    jobs::start(app, id, jobs::Work::Build { note, kind })
}

/// Opus revises a built footage script with targeted edits, as Repack's
/// chat editor does. Runs on a job's thread; the page follows it as a build.
pub fn revise_once(app: &AppHandle, id: &str, request: &str, focus: &revise::Focus) -> Result<(String, f64), String> {
    revise_with(app, id, request, request, focus, false)
}

/// One revision: `request` goes to Claude, `said` into the chat as the
/// producer's. `direct` (a fix from a review) writes saved narration
/// straight in rather than proposing it.
pub fn revise_with(app: &AppHandle, id: &str, request: &str, said: &str, focus: &revise::Focus, direct: bool) -> Result<(String, f64), String> {
    let data = data_dir(app)?;
    let dir = store::script_dir(&data, id)?;
    let script = store::read(&dir)?;
    if script.commentary.is_some() {
        return revise_commentary(&data, &dir, &script, request, said);
    }
    if !script.clips.iter().any(|clip| clip.is_cut_in()) {
        return Err("Build the script first; then ask for changes.".into());
    }
    let earlier = chat::recent(&dir);
    chat::add(&dir, "producer", said, None, false);
    let script = if script.parts.iter().any(|part| part.reuse.is_some() && part.highlights.is_none()) {
        jobs::stage(id, "Picking the reused cases' moments");
        let _ = reuse::pick_missing(&dir);
        store::read(&dir)?
    } else {
        script
    };
    let peaks = tidy::peaks(&dir, &footage_path(&dir, &script));
    let research = casefiles::with_documents(&dir, research::load(&dir).unwrap_or_default());
    let done = revise::revise(&script, request, focus, &earlier, Some(&research), &peaks, direct);
    let done = match done {
        Ok(done) => done,
        Err(error) => {
            chat::add(&dir, "editor", &error, None, true);
            return Err(error);
        }
    };
    // Starting over waits for the producer to confirm it in the chat.
    jobs::check_cancelled()?;
    if done.start_over {
        chat::ask_reset(&dir, &done.answer, Some(done.cost_usd));
        return Ok((done.answer, done.cost_usd));
    }
    // Edits made by hand while Claude worked would be lost: ask again instead.
    let mut now = store::read(&dir)?;
    if now.revision != script.revision {
        let error = "The script changed while Claude was editing it, so nothing was applied. Ask again.".to_string();
        chat::add(&dir, "editor", &error, None, true);
        return Err(error);
    }
    let before = now.clone();
    let mut answer = done.answer;
    let clips = if exact::wanted(&now) {
        let words = now.words.clone();
        let settings = settings::load(&data);
        let footage = footage_path(&dir, &now);
        match exact::run(&mut now, &dir, &footage, &done.clips, &settings, &|step| jobs::stage(id, step)) {
            Ok(heard) if heard.stretches > 0 => {
                let mut clips = exact::remap_from(&words, &done.clips, &now.words, now.duration_ms);
                let silent = exact::silent(&clips, &peaks);
                let dropped = silent.len();
                for index in silent.into_iter().rev() {
                    clips.remove(index);
                }
                answer = format!("{answer}\n\n{}", exact::said(&heard, dropped));
                clips
            }
            Ok(_) => done.clips,
            Err(error) => {
                answer = format!("{answer}\n\nThe footage this added couldn't be heard exactly ({error}), so its cuts follow YouTube's captions.");
                done.clips
            }
        }
    } else {
        done.clips
    };
    now.clips = match check_clips(clips, now.duration_ms) {
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
    chat::add(&dir, "editor", &answer, Some(done.cost_usd), false);
    Ok((answer, done.cost_usd))
}

/// A change to a written commentary script: Opus edits the paragraphs the
/// request is about and nothing else, written straight in (undo and the
/// versions bring back what was there).
pub fn revise_commentary(data: &Path, dir: &Path, script: &model::Script, request: &str, said: &str) -> Result<(String, f64), String> {
    // Edits name paragraphs by ID: a script never shared gets them now.
    let mut script = script.clone();
    if script.commentary.as_mut().and_then(|commentary| commentary.draft.as_mut()).is_some_and(|draft| draft.ensure_ids()) {
        store::write(dir, &script)?;
    }
    let script = &script;
    chat::add(dir, "producer", said, None, false);
    let done = commentary::revise(dir, script, request, &feedback::lessons_text(data)).inspect_err(|error| chat::add(dir, "editor", error, None, true))?;
    jobs::check_cancelled()?;
    if done.start_over {
        chat::ask_reset(dir, &done.answer, Some(done.cost_usd));
        return Ok((done.answer, done.cost_usd));
    }
    let mut now = store::read(dir)?;
    if now.revision != script.revision {
        let error = "The script changed while Claude was editing it, so nothing was applied. Ask again.".to_string();
        chat::add(dir, "editor", &error, None, true);
        return Err(error);
    }
    if now.commentary.as_ref().and_then(|commentary| commentary.draft.as_ref()) != Some(&done.draft) {
        let before = now.clone();
        if let Some(commentary) = now.commentary.as_mut() {
            commentary.draft = Some(done.draft);
        }
        now.revision += 1;
        now.updated_at = now_ms();
        store::write(dir, &now)?;
        versions::keep(dir, "claude", &before, &now);
    }
    chat::add(dir, "editor", &done.answer, Some(done.cost_usd), false);
    Ok((done.answer, done.cost_usd))
}

#[tauri::command]
fn start_revision(app: AppHandle, id: String, request: String, focus: Option<revise::Focus>) -> Result<jobs::BuildStatus, String> {
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
    let data = data_dir(&app)?;
    start_reuse_import_now(app, &data, id, urls.unwrap_or_default())
}

/// Searches YouTube for the clips a commentary project's brief needs.
pub fn start_find_clips_now(app: AppHandle, data: &Path, id: String) -> Result<jobs::BuildStatus, String> {
    let dir = store::script_dir(data, &id)?;
    let script = store::read(&dir)?;
    let commentary = script.commentary.as_ref().ok_or("Clips are found for a commentary script.")?;
    if !commentary.sources.iter().any(|source| source.role == commentary::Role::Brief) {
        return Err("Add the producer's brief first: the search starts from it.".into());
    }
    jobs::start(app, id, jobs::Work::Find)
}

#[tauri::command]
fn start_find_clips(app: AppHandle, id: String) -> Result<jobs::BuildStatus, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    let data = data_dir(&app)?;
    start_find_clips_now(app, &data, id)
}

/// The last clip search's findings, if any.
pub fn finds_now(data: &Path, id: &str) -> Result<Option<scout::Finds>, String> {
    Ok(scout::load(&store::script_dir(data, id)?))
}

#[tauri::command]
fn get_finds(app: AppHandle, id: String) -> Result<Option<scout::Finds>, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    finds_now(&data_dir(&app)?, &id)
}

/// Starts the feedback over from the notes on the script's review page.
pub fn pull_feedback_now(data: &Path, id: &str) -> Result<feedback::Feedback, String> {
    can::require(can::LESSONS)?;
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    feedback::pull(data, id)
}

/// Leaves a note on the script's review page from Writer: on words, with an intent or a suggestion, or about the whole script.
pub fn add_review_note_now(data: &Path, id: &str, text: &str, details: &reviews::NoteDetails) -> Result<(), String> {
    can::require(can::SHARE)?;
    can::require_id(data, id)?;
    let details = reviews::NoteDetails { author: Some(person_name()), ..details.clone() };
    reviews::add_note(data, id, text, &details)
}

/// The signed-in person's name, as review notes and replies are signed.
fn person_name() -> String {
    account::status().person.map(|person| if person.name.trim().is_empty() { person.email } else { person.name }).unwrap_or_else(|| "Writer".into())
}

/// A person's word on a fix from Writer: confirmed, or sent back with what still isn't right.
pub fn check_review_note_now(data: &Path, id: &str, note_id: &str, verdict: &str, text: Option<&str>) -> Result<(), String> {
    can::require(can::SHARE)?;
    can::require_id(data, id)?;
    reviews::check(data, note_id, &person_name(), verdict, text)
}

#[tauri::command]
async fn check_review_note(app: AppHandle, id: String, note_id: String, verdict: String, text: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || check_review_note_now(&data_dir(&app)?, &id, &note_id, &verdict, text.as_deref())).await.map_err(|error| error.to_string())?
}

/// How Ready for changes is going on the script's review, and whether the fixing computer is on.
pub fn review_fix_now(data: &Path, id: &str) -> Result<serde_json::Value, String> {
    can::require_id(data, id)?;
    Ok(reviews::fix_state(data, id))
}

#[tauri::command]
async fn script_audit(app: AppHandle, id: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || audit_now(&data_dir(&app)?, &id, None)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn review_fix(app: AppHandle, id: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || review_fix_now(&data_dir(&app)?, &id)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn add_review_note(app: AppHandle, id: String, text: String, details: Option<reviews::NoteDetails>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || add_review_note_now(&data_dir(&app)?, &id, &text, &details.unwrap_or_default())).await.map_err(|error| error.to_string())?
}

/// A reply under a note on the review page, signed with the signed-in person's name.
pub fn reply_review_note_now(data: &Path, id: &str, note_id: &str, text: &str) -> Result<(), String> {
    can::require(can::SHARE)?;
    can::require_id(data, id)?;
    reviews::reply_as(data, note_id, text, &person_name())
}

#[tauri::command]
async fn reply_review_note(app: AppHandle, id: String, note_id: String, text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || reply_review_note_now(&data_dir(&app)?, &id, &note_id, &text)).await.map_err(|error| error.to_string())?
}

/// Deletes a note left from Writer on the review page.
pub fn delete_review_note_now(data: &Path, id: &str, note_id: &str) -> Result<(), String> {
    can::require(can::SHARE)?;
    can::require_id(data, id)?;
    reviews::delete_note(data, note_id)
}

#[tauri::command]
async fn delete_review_note(app: AppHandle, id: String, note_id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete_review_note_now(&data_dir(&app)?, &id, &note_id)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
async fn pull_feedback(app: AppHandle, id: String) -> Result<feedback::Feedback, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    tauri::async_runtime::spawn_blocking(move || pull_feedback_now(&data_dir(&app)?, &id))
        .await
        .map_err(|error| error.to_string())?
}

pub fn feedback_now(data: &Path, id: &str) -> Result<Option<feedback::Feedback>, String> {
    can::require(can::LESSONS)?;
    Ok(feedback::load(&store::script_dir(data, id)?))
}

#[tauri::command]
fn get_feedback(app: AppHandle, id: String) -> Result<Option<feedback::Feedback>, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    feedback_now(&data_dir(&app)?, &id)
}

pub fn set_feedback_item_now(data: &Path, id: &str, item: &str, skip: Option<bool>, note: Option<String>) -> Result<feedback::Feedback, String> {
    can::require(can::LESSONS)?;
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    feedback::set_item(&store::script_dir(data, id)?, item, skip, note)
}

#[tauri::command]
fn set_feedback_item(app: AppHandle, id: String, item: String, skip: Option<bool>, note: Option<String>) -> Result<feedback::Feedback, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    set_feedback_item_now(&data_dir(&app)?, &id, &item, skip, note)
}

/// Saves the producer's lesson and says what in it still points at the script.
pub fn set_lesson_now(data: &Path, id: &str, lesson: &str) -> Result<serde_json::Value, String> {
    can::require(can::LESSONS)?;
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    let dir = store::script_dir(data, id)?;
    let feedback = feedback::set_lesson(&dir, lesson)?;
    Ok(serde_json::json!({ "feedback": feedback, "leaks": feedback::lesson_leaks(&dir)? }))
}

#[tauri::command]
fn set_lesson(app: AppHandle, id: String, lesson: String) -> Result<serde_json::Value, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    set_lesson_now(&data_dir(&app)?, &id, &lesson)
}

pub fn start_feedback_now(app: AppHandle, data: &Path, id: String, step: feedback::Step) -> Result<jobs::BuildStatus, String> {
    feedback::ready(&store::script_dir(data, &id)?, &step)?;
    jobs::start(app, id, jobs::Work::Learn(step))
}

#[tauri::command]
fn start_feedback(app: AppHandle, id: String, step: feedback::Step) -> Result<jobs::BuildStatus, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    let data = data_dir(&app)?;
    start_feedback_now(app, &data, id, step)
}

pub fn keep_lesson_now(data: &Path, id: &str) -> Result<Vec<feedback::Lesson>, String> {
    can::require(can::LESSONS)?;
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    feedback::keep(data, id)
}

#[tauri::command]
fn keep_lesson(app: AppHandle, id: String) -> Result<Vec<feedback::Lesson>, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    keep_lesson_now(&data_dir(&app)?, &id)
}

#[tauri::command]
fn get_lessons(app: AppHandle) -> Result<Vec<feedback::Lesson>, String> {
    can::require(can::LESSONS)?;
    Ok(feedback::lessons(&data_dir(&app)?))
}

#[tauri::command]
fn forget_lesson(app: AppHandle, lesson: String) -> Result<Vec<feedback::Lesson>, String> {
    can::require(can::LESSONS)?;
    feedback::forget(&data_dir(&app)?, &lesson)
}

/// Shares the script on a Repack review link, or its next version on the same link.
/// The latest shared version replaced in place by the script as it is now.
pub fn replace_shared_now(data: &Path, id: &str) -> Result<ScriptDetail, String> {
    can::require(can::SHARE)?;
    can::require_id(data, id)?;
    reviews::replace_latest(data, id)
}

/// A checked draft put in as the commentary's script (the teaching loop's
/// result, or a fix made through the debug routes), a new revision.
#[cfg(debug_assertions)]
pub fn use_draft_now(data: &Path, id: &str, mut draft: commentary::Draft) -> Result<ScriptDetail, String> {
    can::require_id(data, id)?;
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    let commentary = script.commentary.as_mut().ok_or("That isn't a commentary script.")?;
    draft.ensure_ids();
    commentary.draft = Some(draft);
    script.revision += 1;
    script.updated_at = model::now_ms();
    store::write(&dir, &script)?;
    store::library_changed();
    Ok(store::detail(script, &dir))
}

/// The script as it is now made its review's only version, v1, on the same link.
pub fn compress_shared_now(data: &Path, id: &str) -> Result<ScriptDetail, String> {
    can::require(can::SHARE)?;
    can::require_id(data, id)?;
    reviews::compress(data, id)
}

#[tauri::command]
async fn compress_review(app: AppHandle, id: String) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    tauri::async_runtime::spawn_blocking(move || compress_shared_now(&data_dir(&app)?, &id))
        .await
        .map_err(|error| error.to_string())?
}

pub fn share_now(data: &Path, id: &str) -> Result<ScriptDetail, String> {
    can::require(can::SHARE)?;
    reviews::share(data, id)
}

#[tauri::command]
async fn share_script(app: AppHandle, id: String) -> Result<ScriptDetail, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    tauri::async_runtime::spawn_blocking(move || share_now(&data_dir(&app)?, &id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn review_counts(app: AppHandle) -> Result<reviews::Counts, String> {
    Ok(reviews::counts(&data_dir(&app)?))
}

/// A script's review notes, and its blocks as they now read, for the bands.
pub fn review_notes_now(data: &Path, id: &str) -> Result<serde_json::Value, String> {
    let script = store::read(&store::script_dir(data, id)?)?;
    Ok(serde_json::json!({ "notes": reviews::notes_of(data, id), "blocks": blocks::of(&script), "approved": reviews::approval(data, id) }))
}

#[tauri::command]
fn review_notes(app: AppHandle, id: String) -> Result<serde_json::Value, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    review_notes_now(&data_dir(&app)?, &id)
}

/// Fixes a script's open notes now, by hand: on any computer, not only the fixing one.
pub fn fix_notes_now(app: AppHandle, data: &Path, id: String) -> Result<jobs::BuildStatus, String> {
    can::require(can::SHARE)?;
    if let Some(approved) = reviews::approval(data, &id) {
        return Err(format!("v{} is approved, so its notes aren't fixed. Take the approval back to fix them.", approved.version));
    }
    let notes = reviews::open_notes(data, &id);
    if notes.is_empty() {
        return Err("No notes are open.".into());
    }
    reviews::mark_tried(data, &notes);
    jobs::start(app, id, jobs::Work::Review { notes, ready: None })
}

#[tauri::command]
fn fix_notes(app: AppHandle, id: String) -> Result<jobs::BuildStatus, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    let data = data_dir(&app)?;
    fix_notes_now(app, &data, id)
}

/// Approves the script's latest shared version, or (`approve` false) takes the approval back.
pub fn approve_review_now(data: &Path, id: &str, approve: bool) -> Result<serde_json::Value, String> {
    can::require(can::SHARE)?;
    if approve {
        reviews::approve(data, id)?;
    } else {
        reviews::unapprove(data, id)?;
    }
    review_notes_now(data, id)
}

#[tauri::command]
async fn approve_review(app: AppHandle, id: String, approve: bool) -> Result<serde_json::Value, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    tauri::async_runtime::spawn_blocking(move || approve_review_now(&data_dir(&app)?, &id, approve))
        .await
        .map_err(|error| error.to_string())?
}

/// Brings an archived script's footage back; followed as a build.
pub fn restore_now(app: AppHandle, data: &Path, id: &str) -> Result<jobs::BuildStatus, String> {
    archive::start_restore(&app, data, id)
}

#[tauri::command]
fn restore_script(app: AppHandle, id: String) -> Result<jobs::BuildStatus, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    let data = data_dir(&app)?;
    restore_now(app, &data, &id)
}

#[tauri::command]
fn update_status(app: AppHandle) -> Result<update::Status, String> {
    Ok(update::status(&data_dir(&app)?))
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<update::Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let data = data_dir(&app)?;
        update::check(&data)?;
        Ok(update::status(&data))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || update::install(&app, &data_dir(&app)?))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn set_update_channel(app: AppHandle, channel: update::Channel) -> Result<update::Status, String> {
    tauri::async_runtime::spawn_blocking(move || update::set_channel(&data_dir(&app)?, channel))
        .await
        .map_err(|error| error.to_string())?
}

/// A script starter's photo on disk (fetched once), or none: the page shows initials then.
#[tauri::command]
async fn avatar(app: AppHandle, sub: String, picture: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(avatar::local(&data_dir(&app)?, &sub, &picture).map(|path| path.to_string_lossy().into_owned())))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn account_status() -> account::Status {
    account::status()
}

#[tauri::command]
fn sign_in() -> Result<account::Status, String> {
    account::sign_in()?;
    Ok(account::status())
}

/// Opens one of Writer's own pages in the browser: accounts.spheceo.com, or Writer's issues on GitHub.
#[tauri::command]
fn open_page(url: String) -> Result<(), String> {
    open_page_now(&url)
}

pub fn open_page_now(url: &str) -> Result<(), String> {
    let allowed = ["https://accounts.spheceo.com", "https://github.com/spheceo/writer/issues"];
    if !allowed.iter().any(|start| url == *start || url.starts_with(&format!("{start}/"))) {
        return Err("Writer only opens its own pages.".into());
    }
    proc::run("open", &[url], std::time::Duration::from_secs(10)).map(|_| ())
}

#[tauri::command]
async fn sign_in_with_code() -> Result<account::Status, String> {
    tauri::async_runtime::spawn_blocking(|| {
        account::sign_in_with_code()?;
        Ok(account::status())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
fn cancel_sign_in() -> account::Status {
    account::cancel();
    account::status()
}

#[tauri::command]
async fn sign_out() -> Result<account::Status, String> {
    tauri::async_runtime::spawn_blocking(|| {
        account::sign_out();
        account::status()
    })
    .await
    .map_err(|error| error.to_string())
}

/// Which Writer this is: its name ("Writer nightly-1"), version and tag.
#[tauri::command]
fn about() -> release::About {
    release::about()
}

/// The log's lines after `since` (ms), for the Logs window, and where the file is.
#[tauri::command]
fn get_logs(since: i64) -> serde_json::Value {
    serde_json::json!({ "lines": log::since(since), "file": log::file() })
}

/// Everything long under way, for the progress panel.
#[tauri::command]
fn get_activity() -> Vec<activity::Work> {
    activity::list()
}

/// Shows the log file in Finder.
pub fn reveal_logs_now() -> Result<(), String> {
    let file = log::file().ok_or("There's no log file yet.")?;
    proc::run("open", &["-R", &file.to_string_lossy()], std::time::Duration::from_secs(10)).map(|_| ())
}

#[tauri::command]
fn reveal_logs() -> Result<(), String> {
    reveal_logs_now()
}

/// Debug builds only: a round of the teaching loop (scripts/teach). `build`
/// writes the commentary from its sources as a new build would; `review`
/// hands the written one back as the team does, without their notes. Saves
/// nothing; answers the draft, as text too, and what it took.
#[cfg(debug_assertions)]
pub fn teach_now(data: &Path, id: &str, mode: &str) -> Result<serde_json::Value, String> {
    teach_with(data, id, mode, None, "")
}

/// A teaching step on a given draft: `apply` carries out `request` on it
/// (what the last round still missed, fixed directly), as a revision.
/// What a reviewer would flag on the script as it is (or on a draft
/// given), with the counts behind it: shown before sharing.
pub fn audit_now(data: &Path, id: &str, draft: Option<commentary::Draft>) -> Result<serde_json::Value, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    can::require_script(&script)?;
    if script.commentary.is_some() {
        let (measures, flags) = commentary::checks(&dir, &script, draft.as_ref())?;
        return Ok(serde_json::json!({ "measures": measures, "flags": flags }));
    }
    // A footage script's Letter pages, as its Doc lays them out (a compilation runs 6 to 7).
    Ok(serde_json::json!({ "measures": null, "pages": pages::count(&script, &script.clips), "flags": claude::audit(&script) }))
}

#[cfg(debug_assertions)]
pub fn teach_with(data: &Path, id: &str, mode: &str, draft: Option<commentary::Draft>, request: &str) -> Result<serde_json::Value, String> {
    const REVIEW: &str = "The team read this script and sent it back with a round of notes. You don't get the notes: review the whole script the way the team would, find everything they would flag, and fix it across the script, the house way (the guide). Keep what they wouldn't flag as it is. Send the complete script. In the report, a short list of what you found and what you changed.";
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let lessons = feedback::lessons_text(data);
    let began = std::time::Instant::now();
    let built = match mode {
        "build" => commentary::build(&dir, &script, "", &lessons)?,
        "review" => {
            let base = draft.clone().or_else(|| script.commentary.as_ref().and_then(|commentary| commentary.draft.clone())).ok_or("That script isn't written yet.")?;
            commentary::write(&dir, &script, &commentary::Writing { base: Some(&base), lessons: &lessons, extra: "", request: REVIEW, pace: false })?
        }
        "apply" => {
            let base = draft.or_else(|| script.commentary.as_ref().and_then(|commentary| commentary.draft.clone())).ok_or("That script isn't written yet.")?;
            if request.trim().is_empty() {
                return Err("Say what to carry out.".into());
            }
            commentary::write(&dir, &script, &commentary::Writing { base: Some(&base), lessons: &lessons, extra: "", request, pace: false })?
        }
        _ => return Err("Ask for mode build, review or apply.".into()),
    };
    let (measures, flags) = commentary::checks(&dir, &script, Some(&built.draft))?;
    Ok(serde_json::json!({
        "id": id,
        "mode": mode,
        "measures": measures,
        "flags": flags,
        "draft": built.draft,
        "text": feedback::script_text(&built.draft),
        "report": built.report,
        "costUsd": built.cost_usd,
        "seconds": began.elapsed().as_secs(),
    }))
}

/// Debug builds only: a new commentary script copied from one (its sources
/// and settings), with the given draft and title, ready to share as a review
/// of its own. Answers its id.
#[cfg(debug_assertions)]
pub fn copy_script_now(data: &Path, id: &str, title: &str, draft: commentary::Draft) -> Result<serde_json::Value, String> {
    let from = store::script_dir(data, id)?;
    let mut script = store::read(&from)?;
    let new_id = uuid::Uuid::new_v4().to_string();
    let to = store::scripts_root(data).join(&new_id);
    std::fs::create_dir_all(&to).map_err(|error| error.to_string())?;
    let sources = from.join("sources");
    if sources.is_dir() {
        std::fs::create_dir_all(to.join("sources")).map_err(|error| error.to_string())?;
        for entry in std::fs::read_dir(&sources).map_err(|error| error.to_string())?.flatten() {
            std::fs::copy(entry.path(), to.join("sources").join(entry.file_name())).map_err(|error| error.to_string())?;
        }
    }
    script.id = new_id.clone();
    script.title = title.to_string();
    script.review = None;
    script.doc_export = None;
    script.revision = 1;
    script.created_at = model::now_ms();
    script.updated_at = script.created_at;
    if let Some(commentary) = script.commentary.as_mut() {
        commentary.draft = Some(draft);
    }
    store::write(&to, &script)?;
    store::library_changed();
    Ok(serde_json::json!({ "id": new_id }))
}

/// Debug builds only: builds a script with one engine and saves nothing, for
/// comparing them. The speakers and research are made once and kept, so every
/// build of the script reads the same evidence.
#[cfg(debug_assertions)]
pub fn ab_build_now(data: &Path, id: &str, engine: &str, kind: Option<formats::Kind>) -> Result<serde_json::Value, String> {
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    if let Some(kind) = kind {
        script.kind = kind;
    }
    if script.speakers.is_empty() && speakers::identify(&mut script).is_ok() {
        let mut now = store::read(&dir)?;
        now.speakers = script.speakers.clone();
        now.suspects = script.suspects.clone();
        store::write(&dir, &now)?;
    }
    let research = research::load(&dir).filter(|found| !found.articles.is_empty() || found.problem.is_none()).unwrap_or_else(|| research::run(&script, &dir));
    let research = casefiles::with_documents(&dir, research);
    let peaks = tidy::peaks(&dir, &footage_path(&dir, &script));
    let began = std::time::Instant::now();
    let built = match engine {
        "sdk" => steps::build(&script, "", Some(&research), &peaks),
        "fast" => steps::build_fast(&script, "", Some(&research), &peaks),
        _ => claude::with_engine(settings::Engine::Cli, || claude::build(&script, "", Some(&research), &peaks)),
    };
    let seconds = began.elapsed().as_secs_f64();
    Ok(match built {
        Ok(built) => serde_json::json!({ "ok": true, "engine": engine, "kind": script.kind, "clips": built.clips, "report": built.report, "costUsd": built.cost_usd, "seconds": seconds, "stats": built.stats, "pages": pages::count(&script, &built.clips) }),
        Err(error) => serde_json::json!({ "ok": false, "engine": engine, "kind": script.kind, "error": error, "seconds": seconds }),
    })
}

/// How Writer reaches Claude (Setup): one `claude -p` answer, or the Agent SDK in steps.
#[tauri::command]
fn set_engine(app: AppHandle, engine: settings::Engine) -> Result<settings::Settings, String> {
    settings::set_engine(&data_dir(&app)?, engine)
}

#[tauri::command]
fn get_chat(app: AppHandle, id: String) -> Result<Vec<chat::Turn>, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    Ok(chat::read(&store::script_dir(&data_dir(&app)?, &id)?))
}

/// The producer's answer to Claude asking to start over: accepted, the
/// built script goes (every clip and cut-in, or a commentary's draft) and
/// the transcript, speakers, sources and research stay; the script as it
/// was is kept in Versions. Declined, nothing changes.
pub fn reset_script_now(data: &Path, id: &str, accept: bool) -> Result<model::ScriptDetail, String> {
    can::require_id(data, id)?;
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    let dir = store::script_dir(data, id)?;
    chat::settle_reset(&dir, accept)?;
    let mut now = store::read(&dir)?;
    if accept {
        let before = now.clone();
        match now.commentary.as_mut() {
            Some(commentary) => commentary.draft = None,
            None => {
                now.clips.clear();
                now.proposals.clear();
            }
        }
        now.revision += 1;
        now.updated_at = now_ms();
        store::write(&dir, &now)?;
        versions::keep(&dir, "producer", &before, &now);
    }
    Ok(store::detail(store::read(&dir)?, &dir))
}

#[tauri::command]
async fn reset_script(app: AppHandle, id: String, accept: bool) -> Result<model::ScriptDetail, String> {
    tauri::async_runtime::spawn_blocking(move || reset_script_now(&data_dir(&app)?, &id, accept)).await.map_err(|error| error.to_string())?
}

pub fn clear_chat_now(data: &Path, id: &str) -> Result<(), String> {
    if jobs::running(id) {
        return Err("Claude is still working on this script.".into());
    }
    chat::clear(&store::script_dir(data, id)?)
}

#[tauri::command]
fn clear_chat(app: AppHandle, id: String) -> Result<(), String> {
    can::require_id(&data_dir(&app)?, &id)?;
    clear_chat_now(&data_dir(&app)?, &id)
}

/// Resolves the selected proposals together against the revision reviewed.
pub fn resolve_proposals_now(data: &Path, id: &str, revision: u32, proposals: &[String], accept: bool) -> Result<ScriptDetail, String> {
    can::require_id(data, id)?;
    if jobs::running(id) || import::importing(id) { return Err("Wait for the current work to finish before reviewing changes.".into()); }
    let dir = store::script_dir(data, id)?;
    let mut script = store::read(&dir)?;
    if script.revision != revision { return Err("The script changed. Review the latest changes first.".into()); }
    let before = script.clone();
    script = resolved_proposals(&script, proposals, accept)?;
    script.revision += 1;
    script.updated_at = now_ms();
    store::write(&dir, &script)?;
    versions::keep(&dir, "you", &before, &script);
    Ok(store::detail(script, &dir))
}

fn resolved_proposals(original: &model::Script, proposals: &[String], accept: bool) -> Result<model::Script, String> {
    let mut script = original.clone();
    if proposals.is_empty() || proposals.iter().any(|id| !script.proposals.iter().any(|item| &item.id == id)) { return Err("These suggested changes are no longer waiting for review.".into()); }
    if accept {
        for proposal in script.proposals.iter().filter(|item| proposals.contains(&item.id)) {
            let cue = script.clips.iter_mut().find(|clip| clip.id == proposal.cut_in && clip.is_cut_in()).ok_or("A proposed paragraph is no longer in the script.")?;
            if cue.narration.as_deref().unwrap_or("") != proposal.before { return Err("A proposed paragraph changed. Review it again before accepting.".into()); }
            cue.narration = Some(proposal.after.clone());
        }
        script.clips = check_clips(script.clips, script.duration_ms)?;
    }
    script.proposals.retain(|item| !proposals.contains(&item.id));
    Ok(script)
}

#[tauri::command]
fn resolve_proposals(app: AppHandle, id: String, revision: u32, proposals: Vec<String>, accept: bool) -> Result<ScriptDetail, String> {
    resolve_proposals_now(&data_dir(&app)?, &id, revision, &proposals, accept)
}

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
    can::require_id(&data_dir(&app)?, &id)?;
    decline_proposal_now(&data_dir(&app)?, &id, &proposal)
}

#[tauri::command]
fn retry_speakers(app: AppHandle, id: String) -> Result<jobs::BuildStatus, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    jobs::start(app, id, jobs::Work::Speakers)
}

#[tauri::command]
fn stop_job(app: AppHandle, id: String) -> Result<(), String> { jobs::stop(&data_dir(&app)?, &id) }

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
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
    set_heading_now(&data_dir(&app)?, &id, revision, &heading)
}

pub fn list_versions_now(data: &Path, id: &str) -> Result<Vec<versions::Version>, String> {
    Ok(versions::list(&store::script_dir(data, id)?))
}

#[tauri::command]
fn list_versions(app: AppHandle, id: String) -> Result<Vec<versions::Version>, String> {
    can::require_id(&data_dir(&app)?, &id)?;
    list_versions_now(&data_dir(&app)?, &id)
}

pub fn get_version_now(data: &Path, id: &str, version: &str) -> Result<versions::Snapshot, String> {
    versions::read(&store::script_dir(data, id)?, version)
}

#[tauri::command]
fn get_version(app: AppHandle, id: String, version: String) -> Result<versions::Snapshot, String> {
    can::require_id(&data_dir(&app)?, &id)?;
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
    can::require_id(&data_dir(&app)?, &id)?;
    restore_version_now(&data_dir(&app)?, &id, revision, &version)
}

#[tauri::command]
fn build_status(app: AppHandle, id: String) -> Option<jobs::BuildStatus> {
    jobs::status(&data_dir(&app).ok()?, &id)
}

/// Timeline items stay in the order given: a hook plays footage from anywhere.
pub(crate) fn check_clips(clips: Vec<Clip>, duration_ms: u64) -> Result<Vec<Clip>, String> {
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
        if clip.notes.len() > 50 || clip.notes.iter().any(|note| note.text.trim().is_empty() || note.text.chars().count() > 2_000 || note.id.is_empty() || note.id.len() > 80 || note.anchor.as_ref().is_some_and(|anchor| anchor.start >= anchor.end || anchor.end > 12_000 || anchor.quote.is_empty() || anchor.quote.chars().count() > 6_000)) {
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
        // preview://localhost/<id>: a script's preview from this Mac or the cloud (stream.rs).
        .register_asynchronous_uri_scheme_protocol("preview", |context, request, responder| {
            let app = context.app_handle().clone();
            std::thread::spawn(move || match app.path().app_data_dir() {
                Ok(data) => responder.respond(stream::respond(&data, &request)),
                Err(_) => responder.respond(tauri::http::Response::builder().status(503).body(Vec::new()).unwrap_or_default()),
            });
        })
        .setup(|app| {
            if let Ok(dir) = app.path().app_data_dir() {
                log::init(&dir);
                log::info("app", format!("{} started", release::name()));
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
            // The library's list follows scripts that come or go behind its back.
            store::watch(app.handle().clone());
            // Every script in the cloud, pushed and pulled in the background.
            if let Ok(dir) = app.path().app_data_dir() {
                // Who's signed in, their access token kept fresh for the relay.
                settings::forget_old_token(&dir);
                account::start(app.handle().clone(), dir.clone());
                cloud::start(dir.clone());
                // Review notes from Repack's links, and fixing them.
                reviews::start(app.handle().clone(), dir.clone());
                // Archives heard from Repack, restores, and the footage backfill.
                archive::start(app.handle().clone(), dir.clone());
                // Local footage cleared once verified in the cloud.
                storage::start(dir.clone());
                // Self-update, for Writer as downloaded (release builds only).
                update::start(dir);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_footage,
            list_scripts,
            get_script,
            import_file,
            import_youtube,
            import_drive,
            import_compilation,
            read_brief,
            import_brief,
            get_research,
            research_again,
            resume_import,
            import_progress,
            doctor,
            check_drive,
            rename_script,
            retry_transcribe,
            complete_transcription,
            save_clips,
            delete_script,
            get_settings,
            save_settings,
            check_relay,
            claude_status,
            check_videos,
            trello_cards,
            trello_card,
            youtube_sign_in_status,
            save_youtube_sign_in,
            start_build,
            start_revision,
            start_reuse_import,
            start_find_clips,
            share_script,
            compress_review,
            review_counts,
            review_notes,
            fix_notes,
            set_engine,
            approve_review,
            get_logs,
            about,
            update_status,
            check_update,
            install_update,
            set_update_channel,
            account_status,
            avatar,
            sign_in,
            sign_in_with_code,
            open_page,
            cancel_sign_in,
            sign_out,
            get_activity,
            reveal_logs,
            restore_script,
            set_pacing,
            get_finds,
            pull_feedback,
            add_review_note,
            reply_review_note,
            check_review_note,
            review_fix,
            script_audit,
            delete_review_note,
            get_feedback,
            set_feedback_item,
            set_lesson,
            start_feedback,
            keep_lesson,
            get_lessons,
            forget_lesson,
            build_status,
            get_chat,
            clear_chat,
            reset_script,
            decline_proposal,
            resolve_proposals,
            retry_speakers,
            stop_job,
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
            cloud_status,
            storage_report,
            clear_storage,
            restore_preview
        ])
        .run(tauri::generate_context!())
        .expect("error while running writer");
}

#[cfg(test)]
mod reset_tests {
    use super::*;

    #[test]
    fn suggested_changes_accept_or_reject_together_without_overwriting_newer_words() {
        let mut script: model::Script = serde_json::from_value(serde_json::json!({
            "id": "a1", "title": "Case", "sourceKind": "file", "sourceUrl": "", "originalPath": "", "status": "ready", "stage": "ready", "progress": 100, "error": null, "durationMs": 10_000, "revision": 3, "createdAt": 0, "updatedAt": 0,
            "clips": [{"id":"n1","kind":"cut-in","startMs":0,"endMs":3000,"narration":"First paragraph."},{"id":"n2","kind":"cut-in","startMs":0,"endMs":3000,"narration":"Second paragraph."}],
            "proposals": [{"id":"p1","cutIn":"n1","before":"First paragraph.","after":"First rewrite.","at":0},{"id":"p2","cutIn":"n2","before":"Second paragraph.","after":"Second rewrite.","at":0}]
        })).unwrap();
        let ids = vec!["p1".into(), "p2".into()];
        let rejected = resolved_proposals(&script, &ids, false).unwrap();
        assert!(rejected.proposals.is_empty());
        assert_eq!(rejected.clips[0].narration.as_deref(), Some("First paragraph."));
        let accepted = resolved_proposals(&script, &ids, true).unwrap();
        assert!(accepted.proposals.is_empty());
        assert_eq!(accepted.clips[1].narration.as_deref(), Some("Second rewrite."));
        script.clips[1].narration = Some("A manual edit.".into());
        assert!(resolved_proposals(&script, &ids, true).is_err());
        assert_eq!(script.clips[0].narration.as_deref(), Some("First paragraph."));
        assert!(resolved_proposals(&script, &["old-proposal".into()], false).is_err());
    }

    #[test]
    fn starting_over_clears_the_built_script_only_once_accepted() {
        let data = std::env::temp_dir().join(format!("writer-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let id = "aaaa1111-0000-0000-0000-000000000000";
        let dir = data.join("scripts").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let script = serde_json::json!({
            "id": id, "title": "Case", "sourceKind": "file", "sourceUrl": "", "originalPath": "", "status": "ready", "stage": "",
            "progress": 100, "error": null, "durationMs": 10_000, "revision": 3, "createdAt": 0, "updatedAt": 0,
            "words": [{ "id": "w0", "text": "Hi.", "startMs": 0, "endMs": 500 }],
            "clips": [{ "id": "c1", "startMs": 0, "endMs": 3_000 }, { "id": "k1", "kind": "cut-in", "startMs": 0, "endMs": 3_000, "narration": "It began." }]
        });
        std::fs::write(dir.join("script.json"), serde_json::to_vec(&script).unwrap()).unwrap();
        // Nothing asked: nothing to answer.
        assert!(reset_script_now(&data, id, true).is_err());
        chat::ask_reset(&dir, "Start over? Confirm below.", None);
        // Declined: the script stays.
        let kept = reset_script_now(&data, id, false).unwrap();
        assert_eq!(kept.script.clips.len(), 2);
        assert_eq!(chat::read(&dir).last().unwrap().reset.as_deref(), Some("declined"));
        // Accepted: the clips and cut-ins go, the words stay.
        chat::ask_reset(&dir, "Start over? Confirm below.", None);
        let fresh = reset_script_now(&data, id, true).unwrap();
        assert!(fresh.script.clips.is_empty());
        assert_eq!(fresh.script.words.len(), 1);
        assert_eq!(fresh.script.revision, 4);
        assert_eq!(chat::read(&dir).last().unwrap().reset.as_deref(), Some("accepted"));
        let _ = std::fs::remove_dir_all(&data);
    }
}
