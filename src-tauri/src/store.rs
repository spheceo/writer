use std::fs;
use std::path::{Path, PathBuf};

use crate::model::{now_ms, Script, ScriptDetail, Summary};

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
    ScriptDetail {
        proxy_path: proxy.is_file().then(|| proxy.to_string_lossy().into_owned()),
        script,
    }
}

pub fn read(dir: &Path) -> Result<Script, String> {
    let bytes = fs::read(dir.join("script.json")).map_err(|_| "That script is missing.".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "That script could not be read.".to_string())
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
            Ok(script) => rows.push(script.summary()),
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
