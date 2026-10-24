//! Local API for the dev page. Binds 127.0.0.1 only. Vite on port 1420 is the
//! part that is reachable over Tailscale, and it proxies `/api` here.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::model::Clip;
use crate::store;
use crate::{
    build_status, claude_status, delete_script, get_script, get_settings, list_scripts,
    remove_source, rename_script, save_clips, save_draft, save_settings, source_text, start_build, start_revision, update_source,
};

pub const PORT: u16 = 1421;

pub fn start(app: AppHandle) {
    thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("writer: API port {PORT} is busy ({error})");
                return;
            }
        };
        for connection in listener.incoming() {
            let Ok(stream) = connection else { continue };
            let app = app.clone();
            thread::spawn(move || {
                if let Err(error) = handle(app, stream) {
                    eprintln!("writer: {error}");
                }
            });
        }
    });
}

fn handle(app: AppHandle, mut stream: TcpStream) -> Result<(), String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut header = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if header.len() > 16_384 {
            return write_json(&mut stream, 400, &json!({ "error": "The request header is too large." }));
        }
        let n = stream.read(&mut byte).map_err(|error| error.to_string())?;
        if n == 0 {
            return Ok(());
        }
        header.push(byte[0]);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&header);
    let mut lines = text.lines();
    let Some(start) = lines.next() else {
        return Ok(());
    };
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut content_length = 0usize;
    let mut range = None;
    let (mut host, mut origin) = (String::new(), None);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().unwrap_or(0);
        } else if name.eq_ignore_ascii_case("range") {
            range = Some(value.trim().to_string());
        } else if name.eq_ignore_ascii_case("host") {
            host = value.trim().to_ascii_lowercase();
        } else if name.eq_ignore_ascii_case("origin") {
            origin = Some(value.trim().to_ascii_lowercase());
        }
    }
    // Only Writer's tools for a build in steps (/mcp/<token>, its token random
    // per build) are for anyone on this Mac. The window talks to Writer over
    // IPC, so a release build serves nothing else; a dev build serves the API
    // to the dev page and scripts here, never to a web page (CSRF) or a name
    // that only resolves here (DNS rebinding).
    if !path.starts_with("/mcp/") {
        if !cfg!(debug_assertions) {
            return write_json(&mut stream, 404, &json!({ "error": "Not found." }));
        }
        if !local_host(&host) || origin.as_deref().is_some_and(|origin| !local_origin(origin)) {
            return write_json(&mut stream, 403, &json!({ "error": "That request isn't from Writer." }));
        }
    }
    if content_length > 12_000_000 {
        return write_json(&mut stream, 413, &json!({ "error": "That request is too large." }));
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body).map_err(|error| error.to_string())?;
    }
    let _ = stream.set_read_timeout(None);
    // The Logs window's tail: the lines after `since` (ms).
    if method == "GET" && path == "/api/logs" {
        let since = query.split('&').find_map(|pair| pair.strip_prefix("since=")).and_then(|value| value.parse::<i64>().ok()).unwrap_or(0);
        return write_json(&mut stream, 200, &json!({ "lines": crate::log::since(since), "file": crate::log::file() }));
    }
    // A script starter's photo, fetched once and kept (avatar.rs); 404 means "show initials".
    if method == "GET" && path == "/api/avatar" {
        let param = |key: &str| query.split('&').find_map(|pair| pair.strip_prefix(&format!("{key}="))).map(decode_query).unwrap_or_default();
        let found = crate::avatar::local(&data_dir(&app)?, &param("sub"), &param("picture")).and_then(|file| std::fs::read(file).ok());
        return match found {
            Some(bytes) => write_bytes(&mut stream, 200, &[("content-type", crate::avatar::content_type(&bytes)), ("cache-control", "max-age=86400")], &bytes),
            None => write_json(&mut stream, 404, &json!({ "error": "No photo." })),
        };
    }
    route(app, &mut stream, method, path, &body, range.as_deref())
}

/// The API's own address, as a request names it.
fn local_host(host: &str) -> bool {
    host == format!("127.0.0.1:{PORT}") || host == format!("localhost:{PORT}")
}

/// A page Writer serves itself: the dev page (Vite, :1420) on this Mac.
fn local_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://") else { return false };
    let Some((name, port)) = rest.rsplit_once(':') else { return false };
    (port == "1420" || port == PORT.to_string()) && (name == "localhost" || name == "127.0.0.1")
}

fn route(
    app: AppHandle,
    stream: &mut TcpStream,
    method: &str,
    path: &str,
    body: &[u8],
    range: Option<&str>,
) -> Result<(), String> {
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    // Writer's tools for a build in steps (steps.rs), as an MCP server over HTTP.
    if let ["mcp", token] = segments.as_slice() {
        return mcp(stream, method, token, body);
    }
    // A script of a kind this person can't write isn't there for them (can.rs).
    if let ["api", "scripts", id, ..] = segments.as_slice() {
        if !["file", "youtube", "compilation", "drive", "commentary"].contains(id) {
            if let Err(error) = crate::can::require_id(&data_dir(&app)?, id) {
                return write_json(stream, 403, &json!({ "error": error }));
            }
        }
    }
    match (method, segments.as_slice()) {
        ("GET", ["api", "scripts"]) => json_result(stream, list_scripts(app)),
        ("PUT", ["api", "settings", "engine"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let engine = if value["engine"] == "sdk" { crate::settings::Engine::Sdk } else { crate::settings::Engine::Cli };
            json_result(stream, crate::settings::set_engine(&data_dir(&app)?, engine))
        }
        ("GET", ["api", "library"]) => write_json(stream, 200, &json!({ "version": crate::store::library_version() })),
        ("GET", ["api", "account"]) => write_json(stream, 200, &crate::account::status()),
        ("POST", ["api", "account", "sign-in"]) => json_result(stream, crate::account::sign_in().map(|_| crate::account::status())),
        ("POST", ["api", "open"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            json_result(stream, crate::open_page_now(value["url"].as_str().unwrap_or("")))
        }
        ("POST", ["api", "account", "sign-in-code"]) => json_result(stream, crate::account::sign_in_with_code().map(|_| crate::account::status())),
        ("POST", ["api", "account", "cancel"]) => {
            crate::account::cancel();
            write_json(stream, 200, &crate::account::status())
        }
        ("POST", ["api", "account", "sign-out"]) => {
            crate::account::sign_out();
            write_json(stream, 200, &crate::account::status())
        }
        // Debug builds only: one audio file heard through the relay as Writer
        // calls it, for testing the sign-in without an import.
        // Debug builds only: one engine's build of a script, saved nowhere (comparing engines).
        #[cfg(debug_assertions)]
        ("POST", ["api", "debug", "ab-build"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let kind = serde_json::from_value(value["kind"].clone()).ok();
            json_result(stream, crate::ab_build_now(&data_dir(&app)?, value["id"].as_str().unwrap_or(""), value["engine"].as_str().unwrap_or("cli"), kind))
        }
        // Debug builds only: a commentary written from scratch, or the written
        // one reviewed blind, saved nowhere (scripts/teach, the teaching loop).
        #[cfg(debug_assertions)]
        ("POST", ["api", "debug", "teach"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let draft = serde_json::from_value(value["draft"].clone()).ok();
            json_result(stream, crate::teach_with(&data_dir(&app)?, value["id"].as_str().unwrap_or(""), value["mode"].as_str().unwrap_or("build"), draft, value["request"].as_str().unwrap_or("")))
        }
        // Debug builds only: a reviewer's choices added to a commentary as production inputs (a Reviewer source).
        #[cfg(debug_assertions)]
        ("POST", ["api", "debug", "reviewer-choices"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let adding: crate::commentary::Adding = serde_json::from_value(json!({ "kind": "reviewer", "author": value["author"], "choices": value["choices"] })).map_err(|error| error.to_string())?;
            let data = data_dir(&app)?;
            let settings = crate::settings::load(&data);
            json_result(stream, crate::commentary::add(&data, value["id"].as_str().unwrap_or(""), adding, crate::commentary::Role::Brief, &settings).map(|_| json!({ "ok": true })))
        }
        // Debug builds only: research a commentary's names and correct its machine-heard sources (spelling.rs).
        #[cfg(debug_assertions)]
        ("POST", ["api", "debug", "spelling"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let dir = store::script_dir(&data_dir(&app)?, value["id"].as_str().unwrap_or(""))?;
            let script = store::read(&dir)?;
            json_result(stream, crate::spelling::correct(&dir, &script).map(|said| json!({ "corrected": said, "kept": crate::spelling::summary(&dir) })))
        }
        #[cfg(debug_assertions)]
        ("POST", ["api", "debug", "copy-script"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let draft = serde_json::from_value(value["draft"].clone()).map_err(|_| "Send the draft to copy in.".to_string())?;
            json_result(stream, crate::copy_script_now(&data_dir(&app)?, value["id"].as_str().unwrap_or(""), value["title"].as_str().unwrap_or("Untitled"), draft))
        }
        #[cfg(debug_assertions)]
        ("POST", ["api", "debug", "renew"]) => json_result(stream, crate::account::renew_now()),
        #[cfg(debug_assertions)]
        ("POST", ["api", "debug", "transcribe"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let path = std::path::PathBuf::from(value["path"].as_str().unwrap_or(""));
            let keyterms: Vec<String> = value["keyterms"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            let relay = crate::net::Relay::new(&crate::settings::load(&data_dir(&app)?))?;
            json_result(stream, relay.transcribe(&path, &keyterms).map_err(|failed| match failed {
                crate::net::Failed::Status(code, body) => crate::transcribe::explain(code, &crate::net::said(&body)),
                crate::net::Failed::Unsent(why) | crate::net::Failed::Network(why) => why,
            }))
        }
        ("GET", ["api", "update"]) => write_json(stream, 200, &crate::update::status(&data_dir(&app)?)),
        ("POST", ["api", "update", "check"]) => {
            let data = data_dir(&app)?;
            json_result(stream, crate::update::check(&data).map(|_| crate::update::status(&data)))
        }
        ("POST", ["api", "update", "install"]) => {
            let data = data_dir(&app)?;
            json_result(stream, crate::update::install(&app, &data).map(|_| json!({ "ok": true })))
        }
        ("PUT", ["api", "update", "channel"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let channel = if value["channel"] == "stable" { crate::update::Channel::Stable } else { crate::update::Channel::Nightly };
            json_result(stream, crate::update::set_channel(&data_dir(&app)?, channel))
        }
        ("GET", ["api", "about"]) => write_json(stream, 200, &crate::release::about()),
        ("GET", ["api", "activity"]) => write_json(stream, 200, &crate::activity::list()),
        ("POST", ["api", "logs", "reveal"]) => json_result(stream, crate::reveal_logs_now()),
        ("GET", ["api", "progress"]) => write_json(stream, 200, &crate::import::latest_progress()),
        ("GET", ["api", "cloud"]) => write_json(stream, 200, &crate::cloud::status()),
        ("GET", ["api", "storage"]) => write_json(stream, 200, &crate::storage::report(&data_dir(&app)?)),
        ("POST", ["api", "storage", "clear"]) => write_json(stream, 200, &crate::storage::purge_all(&data_dir(&app)?, false)),
        ("POST", ["api", "scripts", id, "preview"]) => json_result(stream, crate::restore_preview_now(&data_dir(&app)?, id)),
        ("GET", ["api", "settings"]) => json_result(stream, get_settings(app)),
        ("PUT", ["api", "settings"]) => {
            let settings = serde_json::from_slice(body).map_err(|_| "Those settings could not be read.".to_string());
            json_result(stream, settings.and_then(|settings| save_settings(app, settings)))
        }
        ("GET", ["api", "doctor"]) => write_json(stream, 200, &crate::doctor::run(&data_dir(&app)?)),
        ("POST", ["api", "drive", "check"]) => {
            let settings = crate::settings::load(&data_dir(&app)?);
            json_result(stream, crate::drive::access_token(&settings).map(|_| "Google Drive is connected.".to_string()))
        }
        ("POST", ["api", "relay", "check"]) => json_result(stream, crate::relay_health(&data_dir(&app)?)),
        ("GET", ["api", "claude"]) => write_json(stream, 200, &claude_status()),
        ("POST", ["api", "scripts", "file"]) => {
            let path = json_str(body, "path")?;
            let article = json_str(body, "article").ok();
            json_result(stream, crate::import::from_file(&app, &data_dir(&app)?, std::path::Path::new(&path), article.as_deref()))
        }
        ("POST", ["api", "scripts", "youtube"]) => {
            let url = json_str(body, "url")?;
            let article = json_str(body, "article").ok();
            json_result(stream, crate::import::from_youtube(&app, &data_dir(&app)?, &url, article.as_deref()))
        }
        ("POST", ["api", "scripts", "compilation"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let title = value["title"].as_str().unwrap_or("").to_string();
            let urls: Vec<String> = value["urls"].as_array().into_iter().flatten().filter_map(|url| url.as_str().map(str::to_string)).collect();
            let case = value["case"].as_bool().unwrap_or(false);
            json_result(stream, crate::import::from_compilation(&app, &data_dir(&app)?, &title, &urls, case))
        }
        ("POST", ["api", "videos", "check"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let lines: Vec<String> = value["lines"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            json_result(stream, crate::import::check_videos(&data_dir(&app)?, &lines))
        }
        ("POST", ["api", "scripts", "drive"]) => {
            let url = json_str(body, "url")?;
            let article = json_str(body, "article").ok();
            let title = json_str(body, "title").ok();
            json_result(stream, crate::import::from_drive(&app, &data_dir(&app)?, &url, article.as_deref(), title.as_deref()))
        }
        ("GET", ["api", "youtube", "sign-in"]) => write_json(stream, 200, &json!(crate::youtube_sign_in_now(&data_dir(&app)?))),
        ("POST", ["api", "youtube", "sign-in"]) => json_result(stream, crate::youtube::sign_in(&data_dir(&app)?).map(|browser| json!({ "browser": browser }))),
        ("POST", ["api", "trello", "card"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            json_result(stream, crate::trello_card_now(&data_dir(&app)?, value["link"].as_str().unwrap_or("")))
        }
        ("GET", ["api", "trello", list]) => json_result(stream, crate::trello_now(&data_dir(&app)?, list)),
        ("POST", ["api", "scripts", "commentary"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let title = value["title"].as_str().unwrap_or("").to_string();
            let links: Vec<String> = value["links"].as_array().into_iter().flatten().filter_map(|link| link.as_str().map(str::to_string)).collect();
            let data = data_dir(&app)?;
            json_result(stream, crate::can::require(crate::can::COMMENTARY).and_then(|_| crate::commentary::create(&data, &title, &links, &crate::settings::load(&data))))
        }
        ("POST", ["api", "scripts", id, "sources"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let adding = serde_json::from_value(value["adding"].clone()).map_err(|_| "That source could not be read.".to_string())?;
            let role = serde_json::from_value(value["role"].clone()).unwrap_or_default();
            json_result(stream, crate::add_source_now(&data_dir(&app)?, id, adding, role))
        }
        ("GET", ["api", "scripts", id, "sources", source]) => {
            json_result(stream, source_text(app, (*id).to_string(), (*source).to_string()))
        }
        ("PUT", ["api", "scripts", id, "sources", source]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let role = serde_json::from_value(value["role"].clone()).ok();
            let title = value["title"].as_str().map(str::to_string);
            json_result(stream, update_source(app, (*id).to_string(), (*source).to_string(), role, title))
        }
        ("DELETE", ["api", "scripts", id, "sources", source]) => {
            json_result(stream, remove_source(app, (*id).to_string(), (*source).to_string()))
        }
        ("POST", ["api", "scripts", id, "docs"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let draft = value["draft"].as_bool().unwrap_or(false);
            let docx = value["docx"].as_str().unwrap_or("").to_string();
            json_result(stream, crate::export_doc_now(&data_dir(&app)?, id, draft, &docx))
        }
        ("GET", ["api", "scripts", id, "docs"]) => json_result(stream, crate::list_docs_now(&data_dir(&app)?, id)),
        ("GET", ["api", "scripts", id, "docs", "comments"]) => json_result(stream, crate::doc_comments_now(&data_dir(&app)?, id, None)),
        ("GET", ["api", "scripts", id, "docs", doc, "comments"]) => json_result(stream, crate::doc_comments_now(&data_dir(&app)?, id, Some(doc))),
        ("PUT", ["api", "scripts", id, "draft"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The script could not be read.".to_string())?;
            let revision = value["revision"].as_u64().ok_or("Missing revision.")? as u32;
            let draft = serde_json::from_value(value["draft"].clone()).map_err(|_| "The script could not be read.".to_string())?;
            json_result(stream, save_draft(app, (*id).to_string(), revision, draft))
        }
        ("POST", ["api", "scripts", id, "resume"]) => {
            let choice: crate::import::Choice =
                serde_json::from_slice(body).map_err(|_| "That answer could not be read.".to_string())?;
            json_result(stream, crate::import::resume(&app, &data_dir(&app)?, id, choice))
        }
        ("GET", ["api", "scripts", id, "research"]) => {
            let dir = store::script_dir(&data_dir(&app)?, id)?;
            write_json(stream, 200, &crate::research_shown(&dir))
        }
        ("POST", ["api", "scripts", id, "research"]) => json_result(stream, crate::research_now(&data_dir(&app)?, id)),
        ("PUT", ["api", "scripts", id, "title"]) => {
            let title = json_str(body, "title")?;
            json_result(stream, rename_script(app, (*id).to_string(), title))
        }
        ("GET", ["api", "scripts", id]) => json_result(stream, get_script(app, (*id).to_string())),
        ("DELETE", ["api", "scripts", id]) => match delete_script(app, (*id).to_string()) {
            Ok(()) => write_json(stream, 200, &json!({ "ok": true })),
            Err(error) => write_json(stream, 400, &json!({ "error": error })),
        },
        ("GET", ["api", "scripts", id, "proxy"]) => send_proxy(app, stream, id, range),
        ("POST", ["api", "scripts", id, "transcribe"]) => {
            json_result(stream, crate::import::transcribe_existing(&app, &data_dir(&app)?, id))
        }
        ("POST", ["api", "scripts", id, "build"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let note = value["note"].as_str().unwrap_or("").to_string();
            let kind = serde_json::from_value(value["kind"].clone()).ok();
            json_result(stream, start_build(app, (*id).to_string(), note, kind))
        }
        ("GET", ["api", "scripts", id, "build"]) => write_json(stream, 200, &build_status(app, (*id).to_string())),
        ("POST", ["api", "scripts", id, "revise"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let request = value["request"].as_str().unwrap_or("").to_string();
            let focus = serde_json::from_value(value["focus"].clone()).ok();
            json_result(stream, start_revision(app, (*id).to_string(), request, focus))
        }
        ("POST", ["api", "scripts", id, "reuses"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let urls: Vec<String> = serde_json::from_value(value["urls"].clone()).unwrap_or_default();
            let data = data_dir(&app)?;
            json_result(stream, crate::start_reuse_import_now(app, &data, (*id).to_string(), urls))
        }
        ("PUT", ["api", "scripts", id, "pacing"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let pacing = serde_json::from_value(value["pacing"].clone()).map_err(|_| "Choose channel or documentary pacing.".to_string())?;
            json_result(stream, crate::set_pacing_now(&data_dir(&app)?, id, pacing))
        }
        ("POST", ["api", "scripts", id, "finds"]) => {
            let data = data_dir(&app)?;
            json_result(stream, crate::start_find_clips_now(app, &data, (*id).to_string()))
        }
        ("GET", ["api", "scripts", id, "feedback"]) => json_result(stream, crate::feedback_now(&data_dir(&app)?, id)),
        ("POST", ["api", "scripts", id, "feedback"]) => {
            let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            json_result(stream, crate::pull_feedback_now(&data_dir(&app)?, id, value["link"].as_str()))
        }
        ("PUT", ["api", "scripts", id, "feedback", "items", item]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let note = value["note"].as_str().map(str::to_string);
            json_result(stream, crate::set_feedback_item_now(&data_dir(&app)?, id, item, value["skip"].as_bool(), note))
        }
        ("PUT", ["api", "scripts", id, "feedback", "lesson"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            json_result(stream, crate::set_lesson_now(&data_dir(&app)?, id, value["lesson"].as_str().unwrap_or("")))
        }
        ("POST", ["api", "scripts", id, "feedback", "steps"]) => {
            let step = serde_json::from_slice(body).map_err(|_| "Which step: round, improve or finish?".to_string())?;
            let data = data_dir(&app)?;
            json_result(stream, crate::start_feedback_now(app, &data, (*id).to_string(), step))
        }
        ("POST", ["api", "scripts", id, "feedback", "keep"]) => json_result(stream, crate::keep_lesson_now(&data_dir(&app)?, id)),
        ("GET", ["api", "lessons"]) => {
            let data = data_dir(&app)?;
            json_result(stream, crate::can::require(crate::can::LESSONS).map(|_| crate::feedback::lessons(&data)))
        }
        ("DELETE", ["api", "lessons", lesson]) => {
            let data = data_dir(&app)?;
            json_result(stream, crate::can::require(crate::can::LESSONS).and_then(|_| crate::feedback::forget(&data, lesson)))
        }
        ("GET", ["api", "scripts", id, "finds"]) => json_result(stream, crate::finds_now(&data_dir(&app)?, id)),
        ("GET", ["api", "scripts", id, "finds", "frames", name]) => send_frames(app, stream, id, name),
        ("GET", ["api", "scripts", id, "footage"]) => json_result(stream, crate::footage::now(&data_dir(&app)?, id)),
        ("GET", ["api", "scripts", id, "footage", name]) => {
            let dir = store::script_dir(&data_dir(&app)?, id).map_err(|_| "Unknown script.".to_string())?;
            match crate::footage::file(&dir, name) {
                Some(path) => send_file(stream, &path, range),
                None => write_json(stream, 404, &json!({ "error": "No such footage." })),
            }
        }
        ("POST", ["api", "scripts", id, "share"]) => json_result(stream, crate::share_now(&data_dir(&app)?, id)),
        ("PUT", ["api", "reviews", "auto-fix"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let data = data_dir(&app)?;
            json_result(stream, crate::can::require(crate::can::SHARE).and_then(|_| crate::settings::set_auto_fix(&data, value["on"].as_bool().unwrap_or(true))).map(|_| crate::reviews::counts(&data)))
        }
        // Shares the recordings and words with Edit now, without marking the script sent.
        ("POST", ["api", "scripts", id, "edit-share"]) => json_result(stream, crate::handoff::share(&data_dir(&app)?, id, true)),
        ("POST", ["api", "scripts", id, "restore"]) => {
            let data = data_dir(&app)?;
            json_result(stream, crate::restore_now(app, &data, id))
        }
        ("GET", ["api", "reviews"]) => write_json(stream, 200, &crate::reviews::counts(&data_dir(&app)?)),
        ("GET", ["api", "scripts", id, "review"]) => json_result(stream, crate::review_notes_now(&data_dir(&app)?, id)),
        ("PUT", ["api", "scripts", id, "review", "approval"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            json_result(stream, crate::approve_review_now(&data_dir(&app)?, id, value["approve"].as_bool().unwrap_or(true)))
        }
        ("POST", ["api", "scripts", id, "review", "fix"]) => {
            let data = data_dir(&app)?;
            json_result(stream, crate::fix_notes_now(app, &data, (*id).to_string()))
        }
        ("GET", ["api", "scripts", id, "chat"]) => {
            json_result(stream, store::script_dir(&data_dir(&app)?, id).map(|dir| crate::chat::read(&dir)))
        }
        ("DELETE", ["api", "scripts", id, "chat"]) => match crate::clear_chat_now(&data_dir(&app)?, id) {
            Ok(()) => write_json(stream, 200, &json!({ "ok": true })),
            Err(error) => write_json(stream, 400, &json!({ "error": error })),
        },
        ("DELETE", ["api", "scripts", id, "proposals", proposal]) => {
            json_result(stream, crate::decline_proposal_now(&data_dir(&app)?, id, proposal))
        }
        ("PUT", ["api", "scripts", id, "corrections"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let revision = value["revision"].as_u64().ok_or("Missing revision.")? as u32;
            let word = value["word"].as_str().unwrap_or("");
            let text = value["text"].as_str().unwrap_or("");
            json_result(stream, crate::correct_word_now(&data_dir(&app)?, id, revision, word, text))
        }
        ("GET", ["api", "scripts", id, "versions"]) => json_result(stream, crate::list_versions_now(&data_dir(&app)?, id)),
        ("GET", ["api", "scripts", id, "versions", version]) => json_result(stream, crate::get_version_now(&data_dir(&app)?, id, version)),
        ("POST", ["api", "scripts", id, "versions", version, "restore"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let revision = value["revision"].as_u64().ok_or("Missing revision.")? as u32;
            json_result(stream, crate::restore_version_now(&data_dir(&app)?, id, revision, version))
        }
        ("PUT", ["api", "scripts", id, "heading"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
            let revision = value["revision"].as_u64().ok_or("Missing revision.")? as u32;
            json_result(stream, crate::set_heading_now(&data_dir(&app)?, id, revision, value["heading"].as_str().unwrap_or("")))
        }
        ("PUT", ["api", "scripts", id, "clips"]) => {
            let value: Value = serde_json::from_slice(body).map_err(|_| "The cut could not be read.".to_string())?;
            let revision = value["revision"].as_u64().ok_or("Missing revision.")? as u32;
            let clips: Vec<Clip> = serde_json::from_value(value["clips"].clone()).map_err(|_| "The cut could not be read.".to_string())?;
            json_result(stream, save_clips(app, (*id).to_string(), revision, clips))
        }
        _ => write_json(stream, 404, &json!({ "error": "Not found." })),
    }
}

fn data_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_data_dir().map_err(|error| error.to_string())
}

fn send_proxy(app: AppHandle, stream: &mut TcpStream, id: &str, range: Option<&str>) -> Result<(), String> {
    let data = app.path().app_data_dir().map_err(|error| error.to_string())?;
    let dir = store::script_dir(&data, id).map_err(|_| "Unknown script.".to_string())?;
    let path = store::proxy_path(&dir);
    if !path.is_file() {
        // Cleared from this Mac once verified in the cloud: fetch it back
        // as cache. Offline, say so plainly instead of "no preview".
        match crate::storage::fetch_preview(&data, id) {
            Ok(_) => {}
            Err(error) => return write_json(stream, 503, &json!({ "error": format!("The preview is in the cloud and can't be fetched now: {error}") })),
        }
    }
    send_file(stream, &path, range)
}

/// A clip's frames, as the last clip search looked at them.
fn send_frames(app: AppHandle, stream: &mut TcpStream, id: &str, name: &str) -> Result<(), String> {
    let dir = store::script_dir(&data_dir(&app)?, id).map_err(|_| "Unknown script.".to_string())?;
    let safe = name.strip_prefix("frames-").and_then(|rest| rest.strip_suffix(".jpg")).is_some_and(|tag| !tag.is_empty() && tag.chars().all(|c| c.is_ascii_digit()));
    let path = dir.join("finds").join("frames").join(name);
    if !safe || !path.is_file() {
        return write_json(stream, 404, &json!({ "error": "No such frames." }));
    }
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    write_bytes(stream, 200, &[("content-type", "image/jpeg")], &bytes)
}

/// Writer's tools for one build in steps, over MCP's HTTP transport: JSON-RPC
/// requests answered as JSON, notifications accepted, no event stream. Only
/// the build's own random token reaches its draft.
fn mcp(stream: &mut TcpStream, method: &str, token: &str, body: &[u8]) -> Result<(), String> {
    if !crate::steps::exists(token) {
        return write_json(stream, 404, &json!({ "error": "No build is under way here." }));
    }
    if method == "DELETE" {
        return write_bytes(stream, 200, &[], b"");
    }
    if method != "POST" {
        return write_bytes(stream, 405, &[("allow", "POST, DELETE")], b"");
    }
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let answers: Vec<Value> = match &request {
        Value::Array(batch) => batch.iter().filter_map(|one| mcp_answer(token, one)).collect(),
        one => mcp_answer(token, one).into_iter().collect(),
    };
    match (request.is_array(), answers.len()) {
        (_, 0) => write_bytes(stream, 202, &[], b""),
        (false, _) => write_json(stream, 200, &answers[0]),
        (true, _) => write_json(stream, 200, &answers),
    }
}

/// One JSON-RPC request's answer, or none for a notification.
fn mcp_answer(token: &str, request: &Value) -> Option<Value> {
    let id = request.get("id").cloned().filter(|id| !id.is_null());
    let failure = |code: i64, message: &str| json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } });
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => json!({
            "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "writer", "version": crate::release::VERSION },
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": crate::steps::tools() }),
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let args = request["params"]["arguments"].clone();
            match crate::steps::call(token, name, if args.is_null() { &Value::Null } else { &args }) {
                Some((text, refused)) => json!({ "content": [{ "type": "text", "text": text }], "isError": refused }),
                None => return id.is_some().then(|| failure(-32001, "This build has ended.")),
            }
        }
        _ if id.is_none() => return None,
        other => return Some(failure(-32601, &format!("Writer has no method {other}."))),
    };
    id.map(|id| json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

/// A query parameter's value, %-escapes and `+` undone.
fn decode_query(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                Ok(byte) => {
                    out.push(byte);
                    i += 2;
                }
                Err(_) => out.push(b'%'),
            },
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn json_str(body: &[u8], key: &str) -> Result<String, String> {
    let value: Value = serde_json::from_slice(body).map_err(|_| "The request could not be read.".to_string())?;
    value[key].as_str().map(str::to_string).ok_or_else(|| format!("Missing {key}."))
}

fn json_result<T: Serialize>(stream: &mut TcpStream, result: Result<T, String>) -> Result<(), String> {
    match result {
        Ok(value) => write_json(stream, 200, &value),
        Err(error) => write_json(stream, 400, &json!({ "error": error })),
    }
}

fn write_json(stream: &mut TcpStream, status: u16, value: &impl Serialize) -> Result<(), String> {
    let body = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    write_bytes(
        stream,
        status,
        &[("content-type", "application/json")],
        &body,
    )
}

fn write_bytes(stream: &mut TcpStream, status: u16, headers: &[(&str, &str)], body: &[u8]) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        206 => "Partial Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        416 => "Range Not Satisfiable",
        _ => "Error",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).map_err(|error| error.to_string())?;
    stream.write_all(body).map_err(|error| error.to_string())?;
    Ok(())
}

fn send_file(stream: &mut TcpStream, path: &PathBuf, range: Option<&str>) -> Result<(), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    let span = match range {
        None => Some((0, size.saturating_sub(1))),
        Some(header) => parse_range(header, size),
    };
    let Some((start, end)) = span else {
        let header = format!("bytes */{size}");
        return write_bytes(stream, 416, &[("content-range", &header)], b"");
    };
    if size == 0 {
        return write_bytes(stream, 200, &[("content-type", "video/mp4"), ("accept-ranges", "bytes")], b"");
    }
    let length = end - start + 1;
    file.seek(SeekFrom::Start(start)).map_err(|error| error.to_string())?;
    let status = if range.is_some() { 206 } else { 200 };
    let content_range = format!("bytes {start}-{end}/{size}");
    let length_text = length.to_string();
    let reason = if status == 206 { "Partial Content" } else { "OK" };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: video/mp4\r\naccept-ranges: bytes\r\ncontent-range: {content_range}\r\ncontent-length: {length_text}\r\nconnection: close\r\n\r\n"
    );
    stream.write_all(head.as_bytes()).map_err(|error| error.to_string())?;
    let mut left = length;
    let mut buf = [0u8; 64 * 1024];
    while left > 0 {
        let chunk = buf.len().min(left as usize);
        let n = file.read(&mut buf[..chunk]).map_err(|error| error.to_string())?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n]).map_err(|error| error.to_string())?;
        left -= n as u64;
    }
    Ok(())
}

/// One `bytes=` range. The end is inclusive.
fn parse_range(header: &str, size: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') || size == 0 {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    if start.is_empty() {
        return None;
    }
    let start: u64 = start.parse().ok()?;
    if start >= size {
        return None;
    }
    let end = if end.is_empty() {
        size - 1
    } else {
        end.parse::<u64>().ok()?.min(size - 1)
    };
    (end >= start).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::parse_range;

    #[test]
    fn reads_a_byte_range() {
        assert_eq!(parse_range("bytes=0-", 100), Some((0, 99)));
        assert_eq!(parse_range("bytes=10-20", 100), Some((10, 20)));
        assert_eq!(parse_range("bytes=90-500", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=0-1,2-3", 100), None);
    }
}
