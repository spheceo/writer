//! A footage script's versions, as Repack keeps them: every save records the
//! cut as it then stood, the same author's edits within ten minutes merged
//! into one version, each with who made it and a line saying what changed.
//! Restoring one saves it as the script again, itself a new version. Kept
//! beside the script (`versions/`), so they sync with it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{now_ms, Clip, Script};

/// Edits by one author this close together are one version.
const MERGE_MS: i64 = 10 * 60_000;
/// Versions kept; the oldest go first.
const KEPT: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: String,
    pub at: i64,
    /// `you`, `claude`, or `before` for the script as it was before the first recorded edit.
    pub author: String,
    pub summary: String,
    /// The version it was compared with when summarised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// For a restore: when the version it brought back was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<i64>,
}

/// What a version holds: the cut and what reads with it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub corrections: BTreeMap<String, String>,
    #[serde(default)]
    pub heading: Option<String>,
}

impl Snapshot {
    pub fn of(script: &Script) -> Self {
        Self { clips: script.clips.clone(), corrections: script.corrections.clone(), heading: script.heading.clone() }
    }
}

fn folder(dir: &Path) -> PathBuf {
    dir.join("versions")
}

pub fn list(dir: &Path) -> Vec<Version> {
    fs::read(folder(dir).join("index.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn read(dir: &Path, id: &str) -> Result<Snapshot, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err("Unknown version.".into());
    }
    let bytes = fs::read(folder(dir).join(format!("{id}.json"))).map_err(|_| "That version is missing.".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "That version could not be read.".to_string())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(value).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    fs::rename(tmp, path).map_err(|error| error.to_string())
}

/// What changed between two cuts, in a line, as Repack's version list says it.
pub fn summary(before: &Snapshot, after: &Snapshot) -> String {
    let footage = |snap: &Snapshot| snap.clips.iter().filter(|clip| !clip.is_cut_in()).map(|clip| (clip.id.clone(), (clip.start_ms, clip.end_ms))).collect::<BTreeMap<_, _>>();
    let cues = |snap: &Snapshot| snap.clips.iter().filter(|clip| clip.is_cut_in()).map(|clip| (clip.id.clone(), clip.narration.clone().unwrap_or_default())).collect::<BTreeMap<_, _>>();
    let notes = |snap: &Snapshot| snap.clips.iter().map(|clip| clip.notes.len()).sum::<usize>();
    let (was, now) = (footage(before), footage(after));
    let (said, says) = (cues(before), cues(after));
    let mut parts: Vec<String> = Vec::new();
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let kept = now.keys().filter(|id| was.contains_key(*id)).count();
    if !was.is_empty() && !now.is_empty() && kept == 0 {
        parts.push("Rebuilt the cut".into());
    } else {
        let added = now.keys().filter(|id| !was.contains_key(*id)).count();
        let removed = was.keys().filter(|id| !now.contains_key(*id)).count();
        let trimmed = now.iter().filter(|(id, range)| was.get(*id).is_some_and(|old| old != *range)).count();
        let order = |snap: &Snapshot| snap.clips.iter().map(|clip| clip.id.clone()).filter(|id| was.contains_key(id) && now.contains_key(id)).collect::<Vec<_>>();
        let moved = order(before) != order(after);
        if added > 0 {
            parts.push(format!("{} added", plural(added, "clip", "clips")));
        }
        if removed > 0 {
            parts.push(format!("{} removed", plural(removed, "clip", "clips")));
        }
        if trimmed > 0 {
            parts.push(format!("{} trimmed", plural(trimmed, "clip", "clips")));
        }
        if moved {
            parts.push("clips moved".into());
        }
    }
    let written = says.iter().filter(|(id, text)| !text.trim().is_empty() && said.get(*id).is_none_or(|old| old.trim().is_empty())).count();
    let rewritten = says.iter().filter(|(id, text)| said.get(*id).is_some_and(|old| !old.trim().is_empty() && old != *text)).count();
    let cut = said.keys().filter(|id| !says.contains_key(*id)).count();
    if written > 0 {
        parts.push(format!("wrote {}", plural(written, "cut-in", "cut-ins")));
    }
    if rewritten > 0 {
        parts.push(format!("rewrote {}", plural(rewritten, "cut-in", "cut-ins")));
    }
    if cut > 0 {
        parts.push(format!("{} removed", plural(cut, "cut-in", "cut-ins")));
    }
    let (n0, n1) = (notes(before), notes(after));
    if n1 > n0 {
        parts.push(format!("{} added", plural(n1 - n0, "comment", "comments")));
    } else if n0 > n1 {
        parts.push(format!("{} deleted", plural(n0 - n1, "comment", "comments")));
    }
    if before.corrections != after.corrections {
        parts.push("spelling fixed".into());
    }
    if before.heading != after.heading {
        parts.push("heading changed".into());
    }
    if parts.is_empty() {
        return "No change to the cut".into();
    }
    let mut line = parts.join(" · ");
    if let Some(first) = line.get(..1) {
        line = first.to_uppercase() + &line[1..];
    }
    line
}

/// Records a save: `before` is the script as it was, `after` as it is now.
/// The same author's saves within ten minutes merge into one version. Never
/// fails a save: a version that can't be written is skipped.
pub fn keep(dir: &Path, author: &str, before: &Script, after: &Script) {
    let _ = try_keep(dir, author, &Snapshot::of(before), &Snapshot::of(after), None);
}

/// Records a restore of the version made at `restored`, never merged with another.
pub fn keep_restored(dir: &Path, before: &Script, after: &Script, restored: i64) {
    let _ = try_keep(dir, "you", &Snapshot::of(before), &Snapshot::of(after), Some(restored));
}

fn try_keep(dir: &Path, author: &str, before: &Snapshot, after: &Snapshot, restored: Option<i64>) -> Result<(), String> {
    let said = restored.map(|_| "Restored an earlier version".to_string());
    let root = folder(dir);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let mut list = list(dir);
    let now = now_ms();
    // The first recorded edit keeps the script as it was before it too.
    if list.is_empty() && !before.clips.is_empty() {
        let id = uuid::Uuid::new_v4().to_string();
        write_json(&root.join(format!("{id}.json")), before)?;
        list.push(Version { id, at: now - 1, author: "before".into(), summary: "Before the first recorded edit".into(), base: None, restored: None });
    }
    let last = list.last().cloned();
    let merge = said.is_none() && last.as_ref().is_some_and(|last| last.author == author && now - last.at < MERGE_MS);
    if merge {
        let last = last.expect("merging needs a version");
        let base = last.base.as_deref().and_then(|base| read(dir, base).ok()).unwrap_or_else(|| before.clone());
        write_json(&root.join(format!("{}.json", last.id)), after)?;
        if let Some(entry) = list.last_mut() {
            entry.at = now;
            entry.summary = summary(&base, after);
        }
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        write_json(&root.join(format!("{id}.json")), after)?;
        let base = last.map(|last| last.id);
        list.push(Version { id, at: now, author: author.into(), summary: said.unwrap_or_else(|| summary(before, after)), base, restored });
    }
    while list.len() > KEPT {
        let gone = list.remove(0);
        let _ = fs::remove_file(root.join(format!("{}.json", gone.id)));
    }
    write_json(&root.join("index.json"), &list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(clips: Vec<Clip>) -> Snapshot {
        Snapshot { clips, corrections: BTreeMap::new(), heading: None }
    }

    fn clip(id: &str, start_ms: u64, end_ms: u64) -> Clip {
        Clip { id: id.into(), start_ms, end_ms, ..Default::default() }
    }

    fn cue(id: &str, text: &str) -> Clip {
        Clip { id: id.into(), start_ms: 0, end_ms: 3_000, kind: Some("cut-in".into()), narration: Some(text.into()), ..Default::default() }
    }

    #[test]
    fn says_what_changed() {
        let before = snap(vec![clip("a", 0, 5_000), cue("n", "Old."), clip("b", 6_000, 9_000), cue("m", "")]);
        let after = snap(vec![clip("b", 6_000, 8_000), cue("n", "New."), clip("c", 10_000, 12_000), cue("m", "Written.")]);
        assert_eq!(summary(&before, &after), "1 clip added · 1 clip removed · 1 clip trimmed · wrote 1 cut-in · rewrote 1 cut-in");
        let rebuilt = snap(vec![clip("x", 0, 1_000), cue("y", "All new.")]);
        assert!(summary(&before, &rebuilt).starts_with("Rebuilt the cut"));
        assert_eq!(summary(&before, &before), "No change to the cut");
    }

    #[test]
    fn merges_one_authors_saves_and_keeps_the_start() {
        let dir = std::env::temp_dir().join(format!("writer-versions-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let mut script: Script = serde_json::from_value(serde_json::json!({
            "id": "a1", "title": "Case", "sourceKind": "file", "sourceUrl": "",
            "originalPath": "", "status": "ready", "stage": "", "progress": 100,
            "error": null, "durationMs": 10_000, "revision": 1, "createdAt": 0, "updatedAt": 0
        }))
        .unwrap();
        script.clips = vec![clip("a", 0, 5_000)];
        let mut one = script.clone();
        one.clips[0].end_ms = 4_000;
        keep(&dir, "you", &script, &one);
        let mut two = one.clone();
        two.clips.push(clip("b", 6_000, 7_000));
        keep(&dir, "you", &one, &two);
        keep(&dir, "claude", &two, &script);
        let list = list(&dir);
        assert_eq!(list.iter().map(|v| v.author.as_str()).collect::<Vec<_>>(), ["before", "you", "claude"]);
        assert_eq!(list[1].summary, "1 clip added · 1 clip trimmed");
        assert_eq!(read(&dir, &list[0].id).unwrap().clips[0].end_ms, 5_000);
        let _ = fs::remove_dir_all(dir);
    }
}
