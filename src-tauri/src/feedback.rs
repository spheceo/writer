//! Learning from a review. Feedback on an exported Doc (comments, and
//! highlights with or without one) is pulled into the project; then Writer
//! tests whether Claude has learned it rather than just obeying it:
//!
//! 1. The answer key: Opus applies the feedback directly.
//! 2. The lesson: Opus turns the feedback into a skill in general terms that
//!    never points at this script.
//! 3. A round: a fresh Opus gets the script and the lesson only ("a reviewer
//!    found problems of these kinds; find and fix them"), and Opus grades
//!    what it changed against the feedback and the key.
//!
//! The producer improves the lesson (by hand, or by asking Opus to learn
//! from the round's misses) and runs rounds until the match is high enough.
//! Then the lesson is kept, so every later commentary build reads it, and
//! what the last round still missed is fixed directly.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::commentary::{self, Draft, Piece, Writing};
use crate::model::now_ms;
use crate::store;

const MAX_ITEMS: usize = 200;
const MAX_LESSON_CHARS: usize = 12_000;
/// A run of this many words shared with the script is a lesson pointing at it.
const SHINGLE: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Comment,
    Highlight,
}

/// One thing the reviewer marked.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// F1, F2… in the Doc's order.
    pub id: String,
    pub kind: Kind,
    /// The words it's on, as the Doc has them.
    pub quote: String,
    /// What the reviewer wrote; empty for a highlight alone.
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub replies: Vec<String>,
    /// The highlight's colour, as the Doc's HTML has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Where it is in the script ("2. The Collabs ¶3"), when Writer found it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// What the producer adds: what a bare highlight means, or a correction.
    #[serde(default)]
    pub note: String,
    /// Left out of the key, the lesson and the grading.
    #[serde(default)]
    pub skip: bool,
}

/// One paragraph (or montage clip, heading or check) that changed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub place: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Grade {
    Fixed,
    Partly,
    Missed,
    Wrong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub item: String,
    pub grade: Grade,
    /// The round's changes (T1, T2…) that deal with it.
    #[serde(default)]
    pub changes: Vec<String>,
    pub why: String,
}

/// A change the round made that no feedback asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extra {
    pub change: String,
    pub harmful: bool,
    pub why: String,
}

/// One test of the lesson.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Round {
    pub number: u32,
    /// The lesson as this round read it.
    pub lesson: String,
    pub changes: Vec<Change>,
    pub verdicts: Vec<Verdict>,
    pub extras: Vec<Extra>,
    /// Fixed counts 1, partly a half, of the items graded.
    pub score: f64,
    pub summary: String,
    pub cost_usd: f64,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    /// The review the notes are on (a Google Doc's id, for feedback pulled before Oct 24 2026).
    pub doc: String,
    /// The review page.
    pub doc_url: String,
    pub pulled_at: i64,
    /// The script's revision the feedback is matched against (base.json).
    pub revision: u32,
    pub items: Vec<Item>,
    /// The answer key's changes, once made (key.json holds the script).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Vec<Change>>,
    #[serde(default)]
    pub title: String,
    /// The lesson the next round reads. The producer may edit it.
    #[serde(default)]
    pub lesson: String,
    #[serde(default)]
    pub rounds: Vec<Round>,
    /// Said once, about the pull or the lesson.
    #[serde(default)]
    pub problems: Vec<String>,
    #[serde(default)]
    pub cost_usd: f64,
}

fn folder(dir: &Path) -> PathBuf {
    dir.join("feedback")
}

pub fn load(dir: &Path) -> Option<Feedback> {
    serde_json::from_slice(&fs::read(folder(dir).join("feedback.json")).ok()?).ok()
}

fn save(dir: &Path, feedback: &Feedback) -> Result<(), String> {
    write_json(&folder(dir).join("feedback.json"), feedback)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|error| error.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    fs::rename(&tmp, path).map_err(|error| error.to_string())
}

fn read_draft(path: &Path) -> Result<Draft, String> {
    let bytes = fs::read(path).map_err(|_| "A saved script for this feedback is missing. Pull the feedback again.".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "A saved script for this feedback could not be read.".to_string())
}

// ---- Kept lessons ----

/// A lesson kept from a review. Every commentary build reads them all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub id: String,
    pub title: String,
    pub text: String,
    /// The script and pull it was learned from.
    pub script: String,
    pub script_title: String,
    pub pulled_at: i64,
    /// The last round's match when it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    pub kept_at: i64,
}

fn lessons_path(data: &Path) -> PathBuf {
    data.join("lessons.json")
}

/// The kept lessons, and the IDs of those forgotten, so a Mac that still has
/// one doesn't bring it back when the cloud merges them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    #[serde(default)]
    pub lessons: Vec<Lesson>,
    #[serde(default)]
    pub forgotten: Vec<String>,
}

/// The shelf is read and written by builds, the page and the cloud at once.
static SHELF: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_shelf(data: &Path) -> Shelf {
    fs::read(lessons_path(data)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Changes the shelf under the lock, and has the cloud take it soon.
fn change_shelf<T>(data: &Path, change: impl FnOnce(&mut Shelf) -> Result<T, String>) -> Result<T, String> {
    let _held = SHELF.lock().map_err(|_| "Busy.".to_string())?;
    let mut shelf = read_shelf(data);
    let out = change(&mut shelf)?;
    write_json(&lessons_path(data), &shelf)?;
    crate::cloud::sync_soon();
    Ok(out)
}

/// Two shelves as one: every lesson either has (the later keep of one ID
/// wins), less every one forgotten on either.
fn merge(a: &Shelf, b: &Shelf) -> Shelf {
    let mut forgotten: Vec<String> = a.forgotten.iter().chain(&b.forgotten).cloned().collect();
    forgotten.sort();
    forgotten.dedup();
    let mut lessons: Vec<Lesson> = Vec::new();
    for lesson in a.lessons.iter().chain(&b.lessons) {
        if forgotten.contains(&lesson.id) {
            continue;
        }
        match lessons.iter_mut().find(|known| known.id == lesson.id) {
            Some(known) if lesson.kept_at > known.kept_at => *known = lesson.clone(),
            Some(_) => {}
            None => lessons.push(lesson.clone()),
        }
    }
    lessons.sort_by(|x, y| x.kept_at.cmp(&y.kept_at).then_with(|| x.id.cmp(&y.id)));
    Shelf { lessons, forgotten }
}

/// For the cloud: this Mac's shelf merged with the cloud's copy (none when
/// there isn't one yet). Keeps the merge here, and answers it as JSON with
/// whether the cloud's copy needs replacing.
pub fn merge_lessons(data: &Path, remote: Option<&[u8]>) -> Result<(Vec<u8>, bool), String> {
    let _held = SHELF.lock().map_err(|_| "Busy.".to_string())?;
    let here = read_shelf(data);
    let there: Option<Shelf> = remote.and_then(|bytes| serde_json::from_slice(bytes).ok());
    let merged = merge(&here, there.as_ref().unwrap_or(&Shelf::default()));
    if merged != here {
        write_json(&lessons_path(data), &merged)?;
    }
    let bytes = serde_json::to_vec_pretty(&merged).map_err(|error| error.to_string())?;
    Ok((bytes, there.as_ref() != Some(&merged)))
}

pub fn lessons(data: &Path) -> Vec<Lesson> {
    read_shelf(data).lessons
}

/// Every kept lesson as one text for a build's system prompt.
pub fn lessons_text(data: &Path) -> String {
    lessons_except(data, None)
}

/// The kept lessons but the one learned from this pull, which a round is testing.
fn lessons_except(data: &Path, pull: Option<(&str, i64)>) -> String {
    lessons(data)
        .iter()
        .filter(|lesson| pull.is_none_or(|(script, at)| !(lesson.script == script && lesson.pulled_at == at)))
        .map(|lesson| format!("## {}\n{}", lesson.title, without_title(&lesson.text)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The lesson's text without its "# Lesson: …" line, which the title gives.
fn without_title(text: &str) -> &str {
    let text = text.trim();
    match text.split_once('\n') {
        Some((first, rest)) if first.starts_with("# ") => rest.trim(),
        _ => text,
    }
}

/// Keeps the feedback's lesson, in place of one kept from the same pull.
pub fn keep(data: &Path, id: &str) -> Result<Vec<Lesson>, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let feedback = load(&dir).ok_or("Pull feedback first.")?;
    if feedback.lesson.trim().is_empty() {
        return Err("There's no lesson yet. Run a round first.".into());
    }
    change_shelf(data, |shelf| {
        let replaced: Vec<String> = shelf.lessons.iter().filter(|lesson| lesson.script == id && lesson.pulled_at == feedback.pulled_at).map(|lesson| lesson.id.clone()).collect();
        shelf.lessons.retain(|lesson| !replaced.contains(&lesson.id));
        shelf.forgotten.extend(replaced);
        shelf.lessons.push(Lesson {
            id: format!("L{}", now_ms()),
            title: if feedback.title.trim().is_empty() { format!("From the review of {}", script.title) } else { feedback.title.clone() },
            text: feedback.lesson.clone(),
            script: id.to_string(),
            script_title: script.title.clone(),
            pulled_at: feedback.pulled_at,
            score: feedback.rounds.last().map(|round| round.score),
            kept_at: now_ms(),
        });
        Ok(shelf.lessons.clone())
    })
}

pub fn forget(data: &Path, lesson: &str) -> Result<Vec<Lesson>, String> {
    change_shelf(data, |shelf| {
        if !shelf.lessons.iter().any(|item| item.id == lesson) {
            return Err("That lesson isn't kept.".into());
        }
        shelf.lessons.retain(|item| item.id != lesson);
        shelf.forgotten.push(lesson.to_string());
        Ok(shelf.lessons.clone())
    })
}

// ---- Pulling the feedback ----

/// Reads the comments and highlights on a Doc (the script's latest export
/// unless a link names another) and starts over with them, matched against

/// Starts the feedback over from the notes on the script's review page
/// (feedback.spheceo.com): each reviewer's note, and each note left from
/// Writer, with the words it's on and its replies.
pub fn pull(data: &Path, id: &str) -> Result<Feedback, String> {
    let dir = store::script_dir(data, id)?;
    let script = store::read(&dir)?;
    let draft = script.commentary.as_ref().and_then(|commentary| commentary.draft.clone()).ok_or("Write the script before teaching from feedback on it.")?;
    let link = script.review.clone().ok_or("Share the script for review first: its notes are the feedback.")?;
    // The notes as the review page has them now, not as of the last poll.
    let _ = crate::reviews::refresh(data);
    let notes = crate::reviews::notes_of(data, id);
    let blocks = blocks(&draft);
    let mut problems = Vec::new();
    if link.revision != script.revision {
        problems.push("The script changed after it was last shared. The notes are matched against the script as it is now.".into());
    }
    let mut items: Vec<Item> = notes
        .iter()
        .filter(|note| note.parent_id.is_none() && !note.deleted)
        .filter(|note| !note.text.trim().is_empty() || note.replacement.is_some())
        .map(|note| {
            let quote = note.anchor["quote"].as_str().unwrap_or("").trim().to_string();
            // A suggestion says what to put in place of the words it's on.
            let comment = match note.replacement.as_deref() {
                Some("") => format!("Delete these words. {}", note.text.trim()).trim().to_string(),
                Some(words) if note.text.trim().is_empty() || note.text.contains(words) => format!("Replace with: {words}"),
                Some(words) => format!("{} (Replace with: {words})", note.text.trim()),
                None => note.text.trim().to_string(),
            };
            Item {
                id: String::new(),
                kind: Kind::Comment,
                quote,
                comment,
                author: if note.from_app { "Writer".into() } else { note.author.clone() },
                replies: crate::reviews::replies(data, &note.id).into_iter().filter(|reply| !reply.from_app).map(|reply| reply.text.trim().to_string()).filter(|text| !text.is_empty()).collect(),
                color: None,
                place: None,
                note: String::new(),
                skip: false,
            }
        })
        .collect();
    if items.is_empty() {
        return Err("The review has no notes yet. Leave some on its page, or add them here.".into());
    }
    items.truncate(MAX_ITEMS);
    for (index, item) in items.iter_mut().enumerate() {
        item.id = format!("F{}", index + 1);
        item.place = locate(&item.quote, &blocks);
    }
    let missing = items.iter().filter(|item| item.place.is_none() && !item.quote.is_empty()).count();
    if missing > 0 {
        problems.push(format!("{missing} of the noted passages aren't in the script as it is now; Claude sees their words and finds them itself."));
    }
    // Starting over: the earlier pull's key and rounds were about other feedback.
    let _ = fs::remove_dir_all(folder(&dir));
    write_json(&folder(&dir).join("base.json"), &draft)?;
    let feedback = Feedback {
        doc: link.review_id.clone(),
        doc_url: link.url.clone(),
        pulled_at: now_ms(),
        revision: script.revision,
        items,
        key: None,
        title: String::new(),
        lesson: String::new(),
        rounds: Vec::new(),
        problems,
        cost_usd: 0.0,
    };
    save(&dir, &feedback)?;
    Ok(feedback)
}


/// A piece of the script a reviewer can point at: a montage clip, a heading,
/// a paragraph (with its clips and note), or a check.
#[derive(Debug, Clone, PartialEq)]
struct Block {
    place: String,
    text: String,
}

fn cite_text(cite: &commentary::Cite) -> String {
    let quote = if cite.quote.trim().is_empty() { String::new() } else { format!(": \"{}\"", cite.quote.trim()) };
    format!("[CLIP {} {}-{}{quote}]", cite.video, commentary::clock(cite.start_ms), commentary::clock(cite.end_ms))
}

fn blocks(draft: &Draft) -> Vec<Block> {
    let mut out = Vec::new();
    for (index, cite) in draft.open.iter().enumerate() {
        out.push(Block { place: format!("Montage clip {}", index + 1), text: cite_text(cite) });
    }
    for section in &draft.sections {
        out.push(Block { place: section.heading.clone(), text: format!("HEADING: {}", section.heading) });
        for (index, paragraph) in section.paragraphs.iter().enumerate() {
            let mut text = paragraph
                .pieces
                .iter()
                .map(|piece| match piece {
                    Piece::Say { text } => text.trim().to_string(),
                    Piece::Clip(cite) => cite_text(cite),
                })
                .collect::<Vec<_>>()
                .join(" ");
            if let Some(note) = &paragraph.note {
                text.push_str(&format!(" [NOTE FOR EDITOR: {}]", note.trim()));
            }
            out.push(Block { place: format!("{} ¶{}", section.heading, index + 1), text });
        }
    }
    for (index, check) in draft.checks.iter().enumerate() {
        out.push(Block { place: format!("Check {}", index + 1), text: format!("CHECK: {check}") });
    }
    out
}

/// Paragraph by paragraph, what changed from one draft to the other.
fn diff(before: &Draft, after: &Draft) -> Vec<Change> {
    let (a, b) = (blocks(before), blocks(after));
    let (n, m) = (a.len(), b.len());
    // Longest common run of unchanged blocks.
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if a[i].text == b[j].text { table[i + 1][j + 1] + 1 } else { table[i + 1][j].max(table[i][j + 1]) };
        }
    }
    let mut changes = Vec::new();
    let (mut gone, mut came): (Vec<&Block>, Vec<&Block>) = (Vec::new(), Vec::new());
    let flush = |gone: &mut Vec<&Block>, came: &mut Vec<&Block>, changes: &mut Vec<Change>| {
        // Removed and added side by side read as one paragraph rewritten.
        let pairs = gone.len().max(came.len());
        for index in 0..pairs {
            let (old, new) = (gone.get(index), came.get(index));
            changes.push(Change {
                place: old.or(new).map(|block| block.place.clone()).unwrap_or_default(),
                before: old.map(|block| block.text.clone()),
                after: new.map(|block| block.text.clone()),
            });
        }
        gone.clear();
        came.clear();
    };
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i].text == b[j].text {
            flush(&mut gone, &mut came, &mut changes);
            i += 1;
            j += 1;
        } else if j < m && (i == n || table[i][j + 1] >= table[i + 1][j]) {
            came.push(&b[j]);
            j += 1;
        } else {
            gone.push(&a[i]);
            i += 1;
        }
    }
    flush(&mut gone, &mut came, &mut changes);
    changes
}

/// Lowercase words and digits, one space apart.
fn normalise(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_alphanumeric() { c.to_lowercase().next().unwrap_or(c) } else if c == '\'' || c == '’' { '\0' } else { ' ' })
        .filter(|c| *c != '\0')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The block the words are in: the whole run, else its first or last words.
fn locate(quote: &str, blocks: &[Block]) -> Option<String> {
    let words: Vec<&str> = quote.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let whole = normalise(quote);
    let head = normalise(&words[..words.len().min(8)].join(" "));
    let tail = normalise(&words[words.len().saturating_sub(8)..].join(" "));
    let normalised: Vec<String> = blocks.iter().map(|block| normalise(&block.text)).collect();
    for probe in [whole, head, tail] {
        if probe.split(' ').count() < 3 && probe.len() < 12 {
            continue;
        }
        if let Some(index) = normalised.iter().position(|text| text.contains(&probe)) {
            return Some(blocks[index].place.clone());
        }
    }
    None
}

// ---- Keeping a lesson general ----

fn shingles(text: &str) -> HashSet<String> {
    let words: Vec<&str> = text.split(' ').filter(|word| !word.is_empty()).collect();
    words.windows(SHINGLE).map(|run| run.join(" ")).collect()
}

/// Names the script uses that the guide doesn't: words its narration only
/// ever capitalises, at least once mid-sentence (so "Same kid." isn't one),
/// said at least twice. Each comes as the script writes it.
fn names(draft: &Draft) -> Vec<String> {
    const COMMON: [&str; 12] = ["youtube", "twitch", "twitter", "instagram", "tiktok", "kick", "reddit", "google", "discord", "internet", "janitor", "intro"];
    let guide = normalise(commentary::GUIDE);
    let guide: HashSet<&str> = guide.split(' ').collect();
    // Per word: how often, whether always capitalised, whether once mid-sentence, as written.
    let mut counts: std::collections::BTreeMap<String, (usize, bool, bool, String)> = std::collections::BTreeMap::new();
    let narration = draft.sections.iter().flat_map(|section| section.paragraphs.iter().flat_map(|paragraph| &paragraph.pieces)).filter_map(|piece| match piece {
        Piece::Say { text } => Some(text.as_str()),
        Piece::Clip(_) => None,
    });
    for text in narration {
        let mut starts = true;
        for token in text.split_whitespace() {
            let word: String = token.chars().filter(|c| c.is_alphanumeric()).collect();
            if word.chars().count() >= 3 {
                let capital = word.chars().next().is_some_and(char::is_uppercase);
                let entry = counts.entry(word.to_lowercase()).or_insert((0, true, false, word.clone()));
                entry.0 += 1;
                entry.1 &= capital;
                entry.2 |= capital && !starts;
            }
            starts = token.ends_with(['.', '!', '?', '…', ':', '"', '“', '”']);
        }
    }
    counts
        .into_iter()
        .filter(|(word, (count, always, inside, _))| *always && *inside && *count >= 2 && !guide.contains(word.as_str()) && !COMMON.contains(&word.as_str()))
        .map(|(_, (_, _, _, written))| written)
        .collect()
}

/// What in a lesson points at the script: phrases copied from it, and its names.
fn leaks(lesson: &str, draft: &Draft) -> Vec<String> {
    let lesson_words = normalise(lesson);
    let script: HashSet<String> = blocks(draft).iter().flat_map(|block| shingles(&normalise(&block.text))).collect();
    let guide = shingles(&normalise(commentary::GUIDE));
    let mut found: Vec<String> = shingles(&lesson_words).into_iter().filter(|run| script.contains(run) && !guide.contains(run)).collect();
    found.sort();
    // Overlapping runs are one phrase; say a few.
    let mut phrases: Vec<String> = Vec::new();
    for run in found {
        if !phrases.iter().any(|phrase| phrase.split(' ').take(3).eq(run.split(' ').take(3))) {
            phrases.push(format!("\"{run}\""));
        }
    }
    // A name counts only capitalised, so "mark" the verb isn't Mark the person.
    let written: HashSet<String> = lesson.split(|c: char| !c.is_alphanumeric()).map(str::to_string).collect();
    phrases.extend(names(draft).into_iter().filter(|name| written.contains(name)).map(|name| format!("the name \"{name}\"")));
    phrases.truncate(12);
    phrases
}

// ---- The steps ----

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "step")]
pub enum Step {
    /// Make the key and the lesson if they're missing, then test the lesson.
    Round,
    /// Rewrite the lesson from the last round's misses.
    Improve,
    /// Take a round's script, with what it missed fixed directly.
    Finish { round: u32 },
}

fn active(feedback: &Feedback) -> Vec<&Item> {
    feedback.items.iter().filter(|item| !item.skip).collect()
}

fn item_text(item: &Item) -> String {
    let mut out = format!("{} · {}", item.id, if item.kind == Kind::Comment { "comment" } else { "highlight" });
    if !item.author.is_empty() {
        out.push_str(&format!(" by {}", item.author));
    }
    if let Some(place) = &item.place {
        out.push_str(&format!(" · in {place}"));
    }
    if !item.quote.is_empty() {
        out.push_str(&format!("\nOn: \"{}\"", item.quote));
    }
    if !item.comment.is_empty() {
        out.push_str(&format!("\nSays: {}", item.comment));
    }
    for reply in &item.replies {
        out.push_str(&format!("\nReply: {reply}"));
    }
    if !item.note.trim().is_empty() {
        out.push_str(&format!("\nThe producer adds: {}", item.note.trim()));
    } else if item.comment.is_empty() {
        out.push_str("\n(Highlighted without a comment: the reviewer flagged it as a problem.)");
    }
    out
}

fn feedback_text(items: &[&Item]) -> String {
    items.iter().map(|item| item_text(item)).collect::<Vec<_>>().join("\n\n")
}

pub(crate) fn script_text(draft: &Draft) -> String {
    blocks(draft).iter().map(|block| format!("[{}] {}", block.place, block.text)).collect::<Vec<_>>().join("\n")
}

fn changes_text(label: &str, changes: &[Change]) -> String {
    if changes.is_empty() {
        return "(no changes)".into();
    }
    changes
        .iter()
        .enumerate()
        .map(|(index, change)| {
            format!(
                "{label}{} · {}\nBefore: {}\nAfter: {}",
                index + 1,
                change.place,
                change.before.as_deref().unwrap_or("(not there)"),
                change.after.as_deref().unwrap_or("(removed)")
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn apply_request(items: &[&Item]) -> String {
    format!(
        "A reviewer left this feedback on the script. Apply every item, each where it is, as it asks. Where an item is a question, answer it in the script the way the reviewer is pointing. A highlight without a comment marks a passage the reviewer flagged: fix what's wrong with it as the guide would. Keep everything the feedback doesn't touch exactly as it is: every other paragraph, clip, time, quote and note. Send the complete script. In the report, one short line per item: what you changed.\n\nFEEDBACK\n\n{}",
        feedback_text(items)
    )
}

const BLIND_REQUEST: &str = "A reviewer read this script and marked problems in it, every one of a kind the lesson above describes. You aren't told where they are. Read the script with the lesson in mind, find each problem, and fix it where it is. Change nothing the lesson doesn't call for: every other paragraph, clip, time, quote and note stays exactly as it is. Send the complete script. In the report, one short line per fix: where, and what was wrong.";

const TEACHER: &str = r###"You teach the script writer for a commentary channel, the Internet Janitor (investigative video essays read by one narrator, with clips backing each point). A reviewer marked problems in one of its scripts. Turn the review into a lesson: a skill the writer reads before writing or fixing any script, so that it spots and fixes these problems on its own, in this script and every later one.

The writer will be tested: it gets this script and your lesson, but never the feedback and never where the problems are. So the lesson must teach the kind of problem, not the instance.
- For each kind of problem: what it is, why it's wrong for this channel, how to spot it in a draft (what to look for, as concretely as you can without pointing at this script), and how to fix it.
- Group items that share a cause into one kind. Say what the reviewer wants, not only what they don't.
- Never quote this script, never name its people, videos, places or chapters, and never point at a place in it. Where an example helps, make one up about a different person and a different story.
- Keep what the channel's guide already says out, unless the review shows the writer isn't following it; then say plainly how it slipped.
- Plain words, short. Markdown: a first line "# Lesson: <a short title>", then a "## " heading per kind. Under 700 words."###;

fn lesson_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string", "description": "A short title for the lesson." },
            "lesson": { "type": "string", "description": "The whole lesson, in Markdown." },
            "report": { "type": "string", "description": "One or two sentences for the producer: what changed in the lesson, or anything in the review you couldn't teach in general terms." }
        },
        "required": ["title", "lesson", "report"]
    })
}

/// Opus writes or rewrites the lesson, sent back while it points at the script.
fn teach(prompt: &str, base: &Draft) -> Result<(String, String, String, f64), String> {
    let schema = lesson_schema().to_string();
    let mut cost = 0.0;
    let mut turn = crate::claude::ask_model_patiently("opus", TEACHER, &schema, prompt, None)?;
    cost += turn.cost_usd;
    for attempt in 0..3 {
        let plan = turn.plan.clone().ok_or("Claude did not send a lesson.")?;
        let title = plan["title"].as_str().unwrap_or("").trim().to_string();
        let lesson = plan["lesson"].as_str().unwrap_or("").trim().to_string();
        let report = plan["report"].as_str().unwrap_or("").trim().to_string();
        if lesson.is_empty() {
            return Err("Claude sent an empty lesson.".into());
        }
        let lesson: String = lesson.chars().take(MAX_LESSON_CHARS).collect();
        let found = leaks(&lesson, base);
        if found.is_empty() || attempt == 2 || turn.session.is_none() {
            let report = if found.is_empty() {
                report
            } else {
                format!("{report} The lesson still points at the script ({}); edit those parts before the next round.", found.join(", "))
            };
            return Ok((title, lesson, report, cost));
        }
        let refusal = format!(
            "The app sent the lesson back: it points at this script, so the test would show the writer the answers. It uses {}. Rewrite those parts in general terms, or with a made-up example about someone else, and send the whole lesson again.",
            found.join(", ")
        );
        turn = crate::claude::ask_model_patiently("opus", TEACHER, &schema, &refusal, turn.session.as_deref())?;
        cost += turn.cost_usd;
    }
    unreachable!("the loop returns by its third try")
}

const GRADER: &str = r#"You grade a test of a script writer. A reviewer left feedback on a script. The answer key is the script with that feedback applied directly. The test is the same script fixed by a writer who never saw the feedback, only a lesson drawn from it, and had to find the problems itself.
For each feedback item, grade the test:
- fixed: it found this problem where it is and fixed it the way the reviewer asked. Its words can differ from the key's; what matters is that the reviewer would accept it.
- partly: it found the problem but the fix falls short, or fixed only part of it.
- missed: it left the problem as it was.
- wrong: it changed that passage in a way the reviewer wouldn't want, or made it worse.
Name the test's changes (T1, T2…) that deal with each item. Then list every test change that deals with no item, and say whether it's harmful (it breaks something the reviewer or the guide would want, or loses something that was fine) or harmless (a fair fix of the same kind, or neutral).
Be strict and specific: each "why" is one or two sentences a producer can act on, saying what the test did and what the key did."#;

fn grade_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "verdicts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "item": { "type": "string", "description": "The feedback item's ID, e.g. F3." },
                        "grade": { "type": "string", "enum": ["fixed", "partly", "missed", "wrong"] },
                        "changes": { "type": "array", "items": { "type": "string" } },
                        "why": { "type": "string" }
                    },
                    "required": ["item", "grade", "changes", "why"]
                }
            },
            "extras": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "change": { "type": "string", "description": "The test change's ID, e.g. T4." },
                        "harmful": { "type": "boolean" },
                        "why": { "type": "string" }
                    },
                    "required": ["change", "harmful", "why"]
                }
            },
            "summary": { "type": "string", "description": "Two or three sentences: what the lesson taught well, and what it failed to teach." }
        },
        "required": ["verdicts", "extras", "summary"]
    })
}

fn score(verdicts: &[Verdict]) -> f64 {
    if verdicts.is_empty() {
        return 0.0;
    }
    let points: f64 = verdicts
        .iter()
        .map(|verdict| match verdict.grade {
            Grade::Fixed => 1.0,
            Grade::Partly => 0.5,
            Grade::Missed | Grade::Wrong => 0.0,
        })
        .sum();
    points / verdicts.len() as f64
}

fn grade(items: &[&Item], key: &[Change], test: &[Change]) -> Result<(Vec<Verdict>, Vec<Extra>, String, f64), String> {
    let prompt = format!(
        "FEEDBACK\n\n{}\n\nTHE ANSWER KEY'S CHANGES (the feedback applied directly)\n\n{}\n\nTHE TEST'S CHANGES (made from the lesson alone)\n\n{}",
        feedback_text(items),
        changes_text("K", key),
        changes_text("T", test)
    );
    let turn = crate::claude::ask_model_patiently("opus", GRADER, &grade_schema().to_string(), &prompt, None)?;
    let plan = turn.plan.ok_or("Claude did not send its grades.")?;
    let parse = |value: &Value| -> Option<Verdict> { serde_json::from_value(value.clone()).ok() };
    let mut verdicts: Vec<Verdict> = plan["verdicts"].as_array().map(Vec::as_slice).unwrap_or(&[]).iter().filter_map(parse).collect();
    // One verdict per item, in order; an item it skipped counts as missed.
    verdicts.retain(|verdict| items.iter().any(|item| item.id == verdict.item));
    let mut ordered = Vec::new();
    for item in items {
        match verdicts.iter().position(|verdict| verdict.item == item.id) {
            Some(at) => ordered.push(verdicts.remove(at)),
            None => ordered.push(Verdict { item: item.id.clone(), grade: Grade::Missed, changes: Vec::new(), why: "The grader didn't grade it.".into() }),
        }
    }
    let extras = plan["extras"].as_array().map(Vec::as_slice).unwrap_or(&[]).iter().filter_map(|value| serde_json::from_value(value.clone()).ok()).collect();
    Ok((ordered, extras, plan["summary"].as_str().unwrap_or("").trim().to_string(), turn.cost_usd))
}

fn grade_name(grade: Grade) -> &'static str {
    match grade {
        Grade::Fixed => "fixed",
        Grade::Partly => "partly",
        Grade::Missed => "missed",
        Grade::Wrong => "wrong",
    }
}

fn percent(score: f64) -> String {
    format!("{}%", (score * 100.0).round())
}

/// One step, on a job's thread.
pub fn run(app: &AppHandle, id: &str, step: &Step) -> Result<(String, f64), String> {
    let data = crate::data_dir(app)?;
    let dir = store::script_dir(&data, id)?;
    let script = store::read(&dir)?;
    let mut feedback = load(&dir).ok_or("Pull feedback first.")?;
    let base = read_draft(&folder(&dir).join("base.json"))?;
    let items: Vec<Item> = active(&feedback).into_iter().cloned().collect();
    let items: Vec<&Item> = items.iter().collect();
    if items.is_empty() {
        return Err("Every item is left out. Include at least one.".into());
    }
    let stage = |text: &str| crate::jobs::stage(id, text);
    let lessons = lessons_except(&data, Some((id, feedback.pulled_at)));
    let mut cost = 0.0;
    match step {
        Step::Round => {
            if feedback.key.is_none() {
                stage("Making the answer key: the feedback applied directly");
                let built = commentary::write(&dir, &script, &Writing { base: Some(&base), lessons: &lessons, extra: "", request: &apply_request(&items), pace: false })?;
                write_json(&folder(&dir).join("key.json"), &built.draft)?;
                feedback.key = Some(diff(&base, &built.draft));
                feedback.cost_usd += built.cost_usd;
                cost += built.cost_usd;
                save(&dir, &feedback)?;
            }
            let mut said = String::new();
            if feedback.lesson.trim().is_empty() {
                stage("Writing the lesson");
                let prompt = format!("THE SCRIPT\n\n{}\n\nTHE REVIEW\n\n{}", script_text(&base), feedback_text(&items));
                let (title, lesson, report, spent) = teach(&prompt, &base)?;
                feedback.title = title;
                feedback.lesson = lesson;
                feedback.cost_usd += spent;
                cost += spent;
                said = report;
                save(&dir, &feedback)?;
            }
            let number = feedback.rounds.last().map_or(1, |round| round.number + 1);
            stage(&format!("Round {number}: fixing the script from the lesson alone"));
            let lesson = feedback.lesson.clone();
            let extra = format!("LESSON FROM A REVIEW\n{lesson}");
            let built = commentary::write(&dir, &script, &Writing { base: Some(&base), lessons: &lessons, extra: &extra, request: BLIND_REQUEST, pace: false })?;
            write_json(&folder(&dir).join(format!("round-{number}.json")), &built.draft)?;
            let changes = diff(&base, &built.draft);
            stage(&format!("Round {number}: grading against the answer key"));
            let key = feedback.key.clone().unwrap_or_default();
            let (verdicts, extras, summary, spent) = grade(&items, &key, &changes)?;
            let round_cost = built.cost_usd + spent;
            let round = Round { number, lesson, changes, score: score(&verdicts), verdicts, extras, summary, cost_usd: round_cost, at: now_ms() };
            let fixed = round.verdicts.iter().filter(|verdict| verdict.grade == Grade::Fixed).count();
            let harmful = round.extras.iter().filter(|extra| extra.harmful).count();
            let report = format!(
                "Round {number}: {} match. {fixed} of {} items fixed{}.{}{}",
                percent(round.score),
                round.verdicts.len(),
                if harmful > 0 { format!(", {harmful} unasked changes that hurt") } else { String::new() },
                if round.summary.is_empty() { String::new() } else { format!(" {}", round.summary) },
                if said.is_empty() { String::new() } else { format!("\n\nThe lesson: {said}") }
            );
            feedback.cost_usd += round_cost;
            cost += round_cost;
            feedback.rounds.push(round);
            save(&dir, &feedback)?;
            Ok((report, cost))
        }
        Step::Improve => {
            let round = feedback.rounds.last().cloned().ok_or("Run a round first: the lesson learns from what it missed.")?;
            stage(&format!("Improving the lesson from round {}", round.number));
            let graded = round
                .verdicts
                .iter()
                .map(|verdict| format!("{} · {} · {}", verdict.item, grade_name(verdict.grade), verdict.why))
                .collect::<Vec<_>>()
                .join("\n");
            let extras = round
                .extras
                .iter()
                .filter(|extra| extra.harmful)
                .map(|extra| format!("{} · {}", extra.change, extra.why))
                .collect::<Vec<_>>()
                .join("\n");
            let prompt = format!(
                "THE SCRIPT\n\n{}\n\nTHE REVIEW\n\n{}\n\nYOUR LESSON AS ROUND {} READ IT{}\n\n{}\n\nHOW THE WRITER DID ({} match)\n{}\n\nCHANGES IT MADE THAT HURT\n{}\n\nTHE WRITER'S CHANGES\n\n{}\n\nRewrite the lesson so a writer who reads it catches what was missed or half fixed, and stops making the changes that hurt. Keep what worked. The same rules hold: never quote this script, name its people or point at a place in it.",
                script_text(&base),
                feedback_text(&items),
                round.number,
                if round.lesson != feedback.lesson { " (the producer has edited it since; start from the edited one below)" } else { "" },
                feedback.lesson,
                percent(round.score),
                graded,
                if extras.is_empty() { "(none)".to_string() } else { extras },
                changes_text("T", &round.changes)
            );
            let (title, lesson, report, spent) = teach(&prompt, &base)?;
            if !title.is_empty() {
                feedback.title = title;
            }
            feedback.lesson = lesson;
            feedback.cost_usd += spent;
            save(&dir, &feedback)?;
            Ok((format!("The lesson is rewritten from round {}. {report}", round.number), spent))
        }
        Step::Finish { round } => {
            let found = feedback.rounds.iter().find(|item| item.number == *round).cloned().ok_or("There's no such round.")?;
            let draft = read_draft(&folder(&dir).join(format!("round-{round}.json")))?;
            let left: Vec<&Item> = items
                .iter()
                .copied()
                .filter(|item| found.verdicts.iter().find(|verdict| verdict.item == item.id).is_none_or(|verdict| verdict.grade != Grade::Fixed))
                .collect();
            let (draft, report) = if left.is_empty() {
                (draft, format!("Round {round}'s script is the script now. It fixed every item."))
            } else {
                stage(&format!("Fixing the {} items round {round} missed", left.len()));
                let built = commentary::write(&dir, &script, &Writing { base: Some(&draft), lessons: &lessons_text(&data), extra: "", request: &apply_request(&left), pace: false })?;
                cost += built.cost_usd;
                (built.draft, format!("Round {round}'s script is the script now, with the {} items it missed fixed directly.", left.len()))
            };
            let mut now = store::read(&dir)?;
            if let Some(commentary) = now.commentary.as_mut() {
                commentary.draft = Some(draft);
            }
            now.revision += 1;
            now.updated_at = now_ms();
            store::write(&dir, &now)?;
            feedback.cost_usd += cost;
            save(&dir, &feedback)?;
            Ok((report, cost))
        }
    }
}

/// What a step needs before it starts, so the page hears at once.
pub fn ready(dir: &Path, step: &Step) -> Result<(), String> {
    let feedback = load(dir).ok_or("Pull feedback first.")?;
    if active(&feedback).is_empty() {
        return Err("Every item is left out. Include at least one.".into());
    }
    match step {
        Step::Round => Ok(()),
        Step::Improve if feedback.rounds.is_empty() => Err("Run a round first: the lesson learns from what it missed.".into()),
        Step::Improve => Ok(()),
        Step::Finish { round } if !feedback.rounds.iter().any(|item| item.number == *round) => Err("There's no such round.".into()),
        Step::Finish { .. } => Ok(()),
    }
}

/// The producer's changes to an item: leave it out, or say what it means.
pub fn set_item(dir: &Path, item: &str, skip: Option<bool>, note: Option<String>) -> Result<Feedback, String> {
    let mut feedback = load(dir).ok_or("Pull feedback first.")?;
    let found = feedback.items.iter_mut().find(|found| found.id == item).ok_or("There's no such item.")?;
    if let Some(skip) = skip {
        found.skip = skip;
    }
    if let Some(note) = note {
        if note.chars().count() > 2_000 {
            return Err("Keep the note under 2,000 characters.".into());
        }
        found.note = note.trim().to_string();
    }
    // The key applied the feedback as it was.
    feedback.key = None;
    let _ = fs::remove_file(folder(dir).join("key.json"));
    save(dir, &feedback)?;
    Ok(feedback)
}

/// The producer's own edit of the lesson the next round reads.
pub fn set_lesson(dir: &Path, lesson: &str) -> Result<Feedback, String> {
    let mut feedback = load(dir).ok_or("Pull feedback first.")?;
    if lesson.chars().count() > MAX_LESSON_CHARS {
        return Err(format!("Keep the lesson under {MAX_LESSON_CHARS} characters."));
    }
    feedback.lesson = lesson.trim().to_string();
    save(dir, &feedback)?;
    Ok(feedback)
}

/// What a lesson the producer typed still gives away.
pub fn lesson_leaks(dir: &Path) -> Result<Vec<String>, String> {
    let feedback = load(dir).ok_or("Pull feedback first.")?;
    Ok(leaks(&feedback.lesson, &read_draft(&folder(dir).join("base.json"))?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commentary::{Cite, Paragraph, Section};

    fn draft(paragraphs: &[&str]) -> Draft {
        Draft {
            open: vec![Cite { source: "S2".into(), video: "Stream".into(), link: None, start_ms: 0, end_ms: 4_000, quote: "Get him off the stage".into(), verify: None }],
            sections: vec![Section {
                heading: "1. The Rise".into(),
                paragraphs: paragraphs.iter().map(|text| Paragraph { id: String::new(), pieces: vec![Piece::Say { text: text.to_string() }], note: None }).collect(),
            }],
            checks: vec!["Check the voice.".into()],
            report: String::new(),
            written_at: 0,
        }
    }

    #[test]
    fn finds_what_changed_paragraph_by_paragraph() {
        let before = draft(&["One.", "Two.", "Three."]);
        let after = draft(&["One.", "Two, said better.", "Three.", "Four."]);
        let changes = diff(&before, &after);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].before.as_deref(), Some("Two."));
        assert_eq!(changes[0].after.as_deref(), Some("Two, said better."));
        assert_eq!(changes[1].before, None);
        assert_eq!(changes[1].after.as_deref(), Some("Four."));
        assert!(diff(&before, &before).is_empty());
    }

    #[test]
    fn places_a_quote_in_the_script() {
        let script = draft(&["Neon was everywhere that summer.", "Then Kanye called him up for a stream."]);
        let blocks = blocks(&script);
        assert_eq!(locate("Kanye called him up for a stream", &blocks).as_deref(), Some("1. The Rise ¶2"));
        assert_eq!(locate("get him off the stage", &blocks).as_deref(), Some("Montage clip 1"));
        assert_eq!(locate("Nothing like this is in it at all", &blocks), None);
    }

    #[test]
    fn a_lesson_that_points_at_the_script_is_caught() {
        let script = draft(&["Then Kanye called him up for a stream, and Kanye stayed for an hour. Same kid. Same chat. And Mark said hi to Mark."]);
        assert!(leaks("## Clips that repeat\nWhen a point is made, cut the second clip that makes it again. Same as before: mark the place.", &script).is_empty());
        let found = leaks("Don't write lines like then kanye called him up for a stream. Kanye and Mark.", &script);
        assert!(found.iter().any(|leak| leak.contains("called him up for a stream")));
        assert!(found.iter().any(|leak| leak == "the name \"Kanye\""));
        assert!(found.iter().any(|leak| leak == "the name \"Mark\""));
    }

    #[test]
    fn scores_fixed_as_one_and_partly_as_half() {
        let verdict = |grade| Verdict { item: "F1".into(), grade, changes: Vec::new(), why: String::new() };
        assert_eq!(score(&[verdict(Grade::Fixed), verdict(Grade::Partly), verdict(Grade::Missed), verdict(Grade::Wrong)]), 0.375);
    }

    #[test]
    fn merges_kept_lessons_from_two_macs() {
        let lesson = |id: &str, at: i64, text: &str| Lesson {
            id: id.into(),
            title: id.into(),
            text: text.into(),
            script: "s".into(),
            script_title: "S".into(),
            pulled_at: 1,
            score: None,
            kept_at: at,
        };
        let here = Shelf { lessons: vec![lesson("L1", 1, "old"), lesson("L2", 2, "")], forgotten: vec!["L3".into()] };
        let there = Shelf { lessons: vec![lesson("L1", 5, "new"), lesson("L3", 3, ""), lesson("L4", 4, "")], forgotten: Vec::new() };
        let merged = merge(&here, &there);
        assert_eq!(merged.lessons.iter().map(|lesson| lesson.id.as_str()).collect::<Vec<_>>(), ["L2", "L4", "L1"]);
        assert_eq!(merged.lessons[2].text, "new");
        assert_eq!(merged.forgotten, ["L3"]);
        assert_eq!(merge(&merged, &here), merged);
    }
}
