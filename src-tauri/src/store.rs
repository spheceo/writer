use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use tauri::Emitter;

use crate::model::{now_ms, Script, ScriptDetail, Summary};

/// Goes up whenever a script arrives, goes or changes other than through the
/// open page (the cloud pulled it, the local API made or deleted it), so the
/// library's list follows: a `scripts` event to the window, and this count
/// for a page in a browser (`GET /api/library`).
static LIBRARY: AtomicU64 = AtomicU64::new(0);
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn watch(app: tauri::AppHandle) {
    let _ = APP.set(app);
}

pub fn library_changed() {
    LIBRARY.fetch_add(1, Ordering::SeqCst);
    if let Some(app) = APP.get() {
        let _ = app.emit("scripts", ());
    }
}

pub fn library_version() -> u64 {
    LIBRARY.load(Ordering::SeqCst)
}

pub fn scripts_root(data: &Path) -> PathBuf {
    data.join("scripts")
}

pub fn script_dir(data: &Path, id: &str) -> Result<PathBuf, String> {
    if !id_ok(id) {
        return Err("Unknown script.".into());
    }
    Ok(scripts_root(data).join(id))
}

pub fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
}

pub fn proxy_path(dir: &Path) -> PathBuf {
    dir.join("proxy.mp4")
}

pub fn detail(script: Script, dir: &Path) -> ScriptDetail {
    let proxy = proxy_path(dir);
    let archive = dir.parent().and_then(Path::parent).and_then(|data| crate::archive::state(data, &script.id));
    ScriptDetail {
        proxy_path: proxy.is_file().then(|| proxy.to_string_lossy().into_owned()),
        archive,
        script,
    }
}

pub fn read(dir: &Path) -> Result<Script, String> {
    let bytes = fs::read(dir.join("script.json")).map_err(|_| "That script is missing.".to_string())?;
    let mut script: Script = serde_json::from_slice(&bytes).map_err(|_| "That script could not be read.".to_string())?;
    // A script joined before its recordings kept their links gets them from
    // the import's own list, so its timestamps link as a new one's do.
    if script.parts.iter().any(|part| part.url.is_none()) {
        if let Some(links) = crate::import::part_links(dir, &script) {
            for (part, link) in script.parts.iter_mut().zip(links) {
                part.url.get_or_insert(link);
            }
        }
    }
    Ok(script)
}

pub fn write(dir: &Path, script: &Script) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let path = dir.join("script.json");
    let tmp = dir.join("script.json.tmp");
    let bytes = serde_json::to_vec_pretty(script).map_err(|error| error.to_string())?;
    fs::write(&tmp, bytes).map_err(|error| error.to_string())?;
    fs::rename(&tmp, path).map_err(|error| error.to_string())
}

pub fn list(data: &Path) -> Result<Vec<Summary>, String> {
    let root = scripts_root(data);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let mut rows = Vec::new();
    for entry in fs::read_dir(&root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry.path().is_dir() {
            continue;
        }
        match read(&entry.path()) {
            Ok(script) => {
                let mut row = script.summary();
                row.archive = crate::archive::state(data, &script.id);
                rows.push(row);
            }
            Err(_) => continue,
        }
    }
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(rows)
}

/// An import that died with the app is not still running.
pub fn mark_interrupted(data: &Path) {
    let root = scripts_root(data);
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if let Some(id) = dir.file_name().and_then(|name| name.to_str()) {
            crate::jobs::mark_interrupted(data, id);
        }
        let Ok(mut script) = read(&dir) else {
            continue;
        };
        if script.status == "processing" {
            script.status = "error".into();
            script.error = Some("The import stopped when Writer closed. Retry: what finished is kept.".into());
            script.updated_at = now_ms();
            let _ = write(&dir, &script);
        }
    }
}

pub fn remove(data: &Path, id: &str) -> Result<(), String> {
    let dir = script_dir(data, id)?;
    if dir.is_dir() {
        fs::remove_dir_all(dir).map_err(|error| error.to_string())?;
    }
    Ok(())
}
