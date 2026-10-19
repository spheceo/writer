use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Word {
    pub id: String,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// One timeline item, in play order. A footage clip is a source range. A
/// cut-in is a 3 s card that holds a narration paragraph, as in Repack.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// `cut-in`, or absent for footage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narration: Option<String>,
    /// `intro`, `bridge` or `conclusion`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narration_role: Option<String>,
}

pub const CUT_IN_MS: u64 = 3_000;

/// A Google Doc made from the script, and the revision it was made from, so
/// the editor can say when the script has changed since.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocExport {
    pub id: String,
    pub url: String,
    pub revision: u32,
    pub exported_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    /// The recording's file name.
    pub title: String,
    pub start_ms: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub audio: bool,
    /// What a recording cut short claims to be, when it holds less.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_ms: Option<u64>,
    /// Where the recording can be watched: its YouTube or Drive link, which
    /// the script's timestamps and assets link to, as Repack's do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A question that pauses an import, as Repack asks it on the script.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Decision {
    /// Some recordings of a case did not come through; the rest did.
    FailedRecordings {
        summary: String,
        failed: Vec<Failed>,
        total: usize,
    },
    /// The folder holds more than one import takes.
    TooMuch {
        summary: String,
        files: Vec<Offered>,
        #[serde(rename = "maxFiles")]
        max_files: usize,
        #[serde(rename = "maxMs")]
        max_ms: u64,
    },
    /// Nothing was heard, or the footage has no sound.
    NoSpeech { summary: String, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failed {
    pub id: String,
    pub title: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offered {
    pub id: String,
    pub title: String,
    pub size: u64,
    pub duration_ms: Option<u64>,
    pub audio: bool,
    pub too_long: bool,
}

impl Clip {
    pub fn is_cut_in(&self) -> bool {
        self.kind.as_deref() == Some("cut-in")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub id: String,
    pub title: String,
    /// `file` or `youtube`.
    pub source_kind: String,
    pub source_url: String,
    /// Local footage. A disk import keeps the file where it was.
    pub original_path: String,
    /// `processing`, `ready`, `error`, or `needs_choice` when an import has
    /// paused on a question only the producer can answer.
    pub status: String,
    pub stage: String,
    pub progress: u8,
    pub error: Option<String>,
    pub duration_ms: u64,
    #[serde(default)]
    pub words: Vec<Word>,
    #[serde(default)]
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub overview: Vec<f32>,
    /// The recordings a Drive folder was joined from, in play order. Empty
    /// for a single recording.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
    /// The question a paused import is waiting on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
    /// Imported knowing no words were heard; cut it by time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_speech: bool,
    /// What an import passed over without failing, said plainly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Interrogation, FOIA or Courtroom, as Repack's workspaces are.
    #[serde(default)]
    pub kind: crate::formats::Kind,
    /// Several cases, one per video, joined as one compilation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compilation: bool,
    /// Who each voice is, as "Name (Role)" or a role, from Haiku.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub speakers: BTreeMap<String, String>,
    /// The people the case is about, for the script's title line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suspects: Vec<String>,
    /// The heading as the producer rewrote it, in place of the generated
    /// "INTERRO … - title" line, as Repack keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    /// Words the producer respelled, by word ID. A word's timing never
    /// changes; only what it reads as.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub corrections: BTreeMap<String, String>,
    /// The last full export to Google Docs (not a draft), as Repack keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_export: Option<DocExport>,
    /// A commentary project's sources and written script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commentary: Option<crate::commentary::Commentary>,
    pub revision: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptDetail {
    #[serde(flatten)]
    pub script: Script,
    pub proxy_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub stage: String,
    pub progress: u8,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub updated_at: i64,
    pub word_count: usize,
    pub clip_count: usize,
    pub cut_in_count: usize,
    /// Claude is writing it now.
    pub building: bool,
    pub source_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
    /// A commentary script's narration words, once written.
    pub draft_words: usize,
    pub compilation: bool,
    /// When the last full Google Doc export was made, and whether the script changed since.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_exported_at: Option<i64>,
    pub doc_behind: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub id: String,
    pub title: String,
    pub stage: String,
    pub percent: u8,
    pub detail: String,
}

impl Script {
    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            title: self.title.clone(),
            status: self.status.clone(),
            stage: self.stage.clone(),
            progress: self.progress,
            error: self.error.clone(),
            duration_ms: self.duration_ms,
            updated_at: self.updated_at,
            word_count: self.words.len(),
            clip_count: self.clips.iter().filter(|clip| !clip.is_cut_in()).count(),
            cut_in_count: self.clips.iter().filter(|clip| clip.is_cut_in()).count(),
            building: false,
            source_kind: self.source_kind.clone(),
            decision: self.decision.clone(),
            draft_words: self.commentary.as_ref().and_then(|c| c.draft.as_ref()).map_or(0, |draft| draft.words()),
            compilation: self.compilation,
            doc_exported_at: self.doc_export.as_ref().map(|doc| doc.exported_at),
            doc_behind: self.doc_export.as_ref().is_some_and(|doc| doc.revision < self.revision),
            created_at: self.created_at,
        }
    }
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
