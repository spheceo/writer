//! A script as blocks of text, each with an ID that stays as the script is
//! edited: what a review link shows and its notes anchor to, and what a fix
//! is measured on. A footage script's blocks are its timeline items (a
//! footage clip's words, a cut-in's narration); a commentary script's are
//! its montage, its section headings and its paragraphs.
//!
//! Offsets into a block's text count UTF-16 code units, as JavaScript does,
//! so the review page and Writer's own view read them the same way.

use serde::{Deserialize, Serialize};

use crate::commentary::{clock, Cite, Paragraph, Piece};
use crate::model::{Part, Script};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    /// `footage`, `narration`, `montage`, `heading` or `paragraph`.
    pub kind: String,
    /// What a footage clip is printed as in the Doc: its range ("Audio 1:02–1:40").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub text: String,
    /// A footage clip's transcript just outside it, about 25 words each
    /// side, for "start earlier" and "end later" notes (anchor `side`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Where a footage clip plays in the preview (the proxy), ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_ms: Option<[u64; 2]>,
    /// Each word of `text`, in order, as [start, end] ms in the preview.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<[u64; 2]>,
    /// How long narration takes to read, at the channel's 150 words a minute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_ms: Option<u64>,
    /// How long the block runs in the video: a clip's length, narration's read time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Notes for the video editor on it ("edit: …").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edit_notes: Vec<String>,
    /// The words an edit note is on, when it's on some of the block rather
    /// than all of it: `note` is its index in `edit_notes`. A note with no
    /// mark is on the whole block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edit_marks: Vec<EditMark>,
}

/// An edit note's words, as a review anchor names them: UTF-16 offsets into
/// the block's text and the words there.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMark {
    pub note: usize,
    pub start: usize,
    pub end: usize,
    pub quote: String,
    /// A quote pop-up: the words go up on screen, typed out, instead of a clip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub popup: Option<Popup>,
}

/// A quote pop-up's card for Edit and the review page: who said it, their
/// words as the card types them, and where they said them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Popup {
    pub who: String,
    pub words: String,
    pub video: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Part of a block's text a fix changed. Empty (`start == end`) marks where
/// something was taken out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub block_id: String,
    pub start: usize,
    pub end: usize,
}

/// Narration is read at the channel's pace.
pub fn read_ms(text: &str) -> u64 {
    text.split_whitespace().count() as u64 * 60_000 / crate::commentary::WORDS_PER_MINUTE as u64
}

/// A commentary paragraph's note, every line of it, for Edit's agent (the
/// only editor): a line that doesn't say "edit:" gets it, so Edit reads it
/// as its own.
pub fn editor_lines<'a>(notes: impl Iterator<Item = &'a str>) -> Vec<String> {
    notes
        .flat_map(str::lines)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| if line.to_lowercase().starts_with("edit:") { line.to_string() } else { format!("edit: {line}") })
        .collect()
}

/// The `edit:` lines of a note or notes.
pub fn edit_lines<'a>(notes: impl Iterator<Item = &'a str>) -> Vec<String> {
    notes.flat_map(str::lines).map(str::trim).filter(|line| line.to_lowercase().starts_with("edit:")).map(str::to_string).collect()
}

/// Words of context either side of a footage clip.
const CONTEXT_WORDS: usize = 25;

pub fn of(script: &Script) -> Vec<Block> {
    match script.commentary.as_ref() {
        Some(commentary) => commentary.draft.as_ref().map(commentary_blocks).unwrap_or_default(),
        None => footage_blocks(script),
    }
}

/// Which stretch of a footage block a note's offsets are into.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Text,
    Before,
    After,
}

/// A footage clip's words as its block shows them: each word's index in the
/// script's words and its UTF-16 span in the stretch's text.
pub fn word_spans(script: &Script, clip: &crate::model::Clip, side: Side) -> Vec<(usize, usize, usize)> {
    let indexes: Vec<usize> = match side {
        Side::Text => (0..script.words.len()).filter(|&at| script.words[at].start_ms < clip.end_ms && script.words[at].end_ms > clip.start_ms).collect(),
        Side::Before => {
            let all: Vec<usize> = (0..script.words.len()).filter(|&at| script.words[at].end_ms <= clip.start_ms).collect();
            all[all.len().saturating_sub(CONTEXT_WORDS)..].to_vec()
        }
        Side::After => (0..script.words.len()).filter(|&at| script.words[at].start_ms >= clip.end_ms).take(CONTEXT_WORDS).collect(),
    };
    let mut at = 0;
    indexes
        .into_iter()
        .map(|index| {
            let word = &script.words[index];
            let length = script.corrections.get(&word.id).unwrap_or(&word.text).encode_utf16().count();
            let span = (index, at, at + length);
            at += length + 1;
            span
        })
        .collect()
}

fn spoken(script: &Script, indexes: impl Iterator<Item = usize>) -> String {
    indexes.map(|at| script.corrections.get(&script.words[at].id).unwrap_or(&script.words[at].text).as_str()).collect::<Vec<_>>().join(" ")
}

fn footage_blocks(script: &Script) -> Vec<Block> {
    script
        .clips
        .iter()
        .map(|clip| {
            if clip.is_cut_in() {
                let text = clip.narration.clone().unwrap_or_default();
                let read = read_ms(&text);
                Block { id: clip.id.clone(), kind: "narration".into(), read_ms: Some(read), duration_ms: Some(read), text, edit_notes: edit_lines(clip.notes.iter().map(|note| note.text.as_str())), ..Default::default() }
            } else {
                let inside = word_spans(script, clip, Side::Text);
                let context = |side| {
                    let found = word_spans(script, clip, side);
                    (!found.is_empty()).then(|| spoken(script, found.iter().map(|(index, _, _)| *index)))
                };
                Block {
                    id: clip.id.clone(),
                    kind: "footage".into(),
                    label: Some(range(clip.start_ms, clip.end_ms, &script.parts)),
                    text: spoken(script, inside.iter().map(|(index, _, _)| *index)),
                    before: context(Side::Before),
                    after: context(Side::After),
                    preview_ms: Some([clip.start_ms, clip.end_ms]),
                    words: inside.iter().map(|(index, _, _)| [script.words[*index].start_ms, script.words[*index].end_ms]).collect(),
                    duration_ms: Some(clip.end_ms.saturating_sub(clip.start_ms)),
                    edit_notes: edit_lines(clip.notes.iter().map(|note| note.text.as_str())),
                    ..Default::default()
                }
            }
        })
        .collect()
}

fn commentary_blocks(draft: &crate::commentary::Draft) -> Vec<Block> {
    let mut out = Vec::new();
    if !draft.open.is_empty() {
        let clips: u64 = draft.open.iter().map(|cite| cite.end_ms.saturating_sub(cite.start_ms)).sum();
        out.push(Block { id: "open".into(), kind: "montage".into(), text: draft.open.iter().map(cite_line).collect::<Vec<_>>().join("\n"), duration_ms: Some(clips), ..Default::default() });
    }
    for section in &draft.sections {
        if let Some(first) = section.paragraphs.first() {
            out.push(Block { id: format!("{}:heading", first.id), kind: "heading".into(), text: section.heading.clone(), ..Default::default() });
        }
        for paragraph in &section.paragraphs {
            let said: String = paragraph.pieces.iter().filter_map(|piece| match piece { Piece::Say { text } => Some(text.as_str()), Piece::Clip(_) => None }).collect::<Vec<_>>().join(" ");
            let clips: u64 = paragraph.pieces.iter().filter_map(|piece| match piece { Piece::Clip(cite) => Some(cite.end_ms.saturating_sub(cite.start_ms)), Piece::Say { .. } => None }).sum();
            let read = read_ms(&said);
            out.push(Block {
                id: paragraph.id.clone(),
                kind: "paragraph".into(),
                text: paragraph_text(paragraph),
                read_ms: Some(read),
                duration_ms: Some(read + clips),
                edit_notes: editor_lines(paragraph.note.iter().map(String::as_str)),
                edit_marks: edit_marks(paragraph),
                ..Default::default()
            });
        }
    }
    out
}

/// Where in a paragraph's block text each of its marked notes is (its
/// lines are `edit_notes` in order).
pub fn edit_marks(paragraph: &Paragraph) -> Vec<EditMark> {
    let lines = paragraph.note_lines();
    let spans = piece_spans(paragraph);
    paragraph
        .live_marks()
        .filter_map(|(mark, (piece, start, end))| {
            let Piece::Say { text } = &paragraph.pieces[piece] else { return None };
            let at = spans[piece].0;
            let units = |bytes: usize| text[..bytes].encode_utf16().count();
            Some(EditMark {
                note: lines.iter().position(|line| *line == mark.line.trim())?,
                start: at + units(start),
                end: at + units(end),
                quote: text[start..end].to_string(),
                popup: mark.quote.as_ref().map(|popup| Popup {
                    who: popup.who.clone(),
                    words: popup.said.quote.trim().to_string(),
                    video: popup.said.video.clone(),
                    link: popup.said.link.clone(),
                    start_ms: popup.said.start_ms,
                    end_ms: popup.said.end_ms,
                }),
            })
        })
        .collect()
}

/// A paragraph's pieces with their UTF-16 spans in its block text (each on its own line).
pub fn piece_spans(paragraph: &Paragraph) -> Vec<(usize, usize)> {
    let mut at = 0;
    paragraph
        .pieces
        .iter()
        .map(|piece| {
            let length = match piece {
                Piece::Say { text } => text.encode_utf16().count(),
                Piece::Clip(cite) => cite_line(cite).encode_utf16().count(),
            };
            let span = (at, at + length);
            at += length + 1;
            span
        })
        .collect()
}

/// A place in the script to jump to, for the review page's outline rail.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub block_id: String,
    pub title: String,
    /// `section`, `case` (a compilation's) or `recording` (a joined case's).
    pub kind: String,
}

/// Where the script's parts begin: a commentary's sections; a footage
/// script's opening, each case or recording, and its ending.
pub fn outline(script: &Script, blocks: &[Block]) -> Vec<Mark> {
    let mark = |block_id: &str, title: String, kind: &str| Mark { block_id: block_id.to_string(), title, kind: kind.to_string() };
    if script.commentary.is_some() {
        return blocks
            .iter()
            .filter_map(|block| match block.kind.as_str() {
                "montage" => Some(mark(&block.id, "Montage".into(), "section")),
                "heading" => Some(mark(&block.id, block.text.clone(), "section")),
                _ => None,
            })
            .collect();
    }
    let mut out = Vec::new();
    if let Some(first) = script.clips.first() {
        out.push(mark(&first.id, "Opening".into(), "section"));
    }
    if script.parts.len() > 1 {
        let mut case = 0;
        for (index, part) in script.parts.iter().enumerate() {
            let end = part.start_ms + part.duration_ms;
            let Some(clip) = script.clips.iter().find(|clip| !clip.is_cut_in() && clip.start_ms >= part.start_ms && clip.start_ms < end) else { continue };
            let (title, kind) = if part.reuse.is_some() {
                (format!("Reused: {}", part.title), "case")
            } else if script.compilation {
                case += 1;
                (format!("Case {case}: {}", part.title), "case")
            } else {
                (format!("Recording {}: {}", index + 1, part.title), "recording")
            };
            if !out.iter().any(|kept: &Mark| kept.block_id == clip.id) {
                out.push(mark(&clip.id, title, kind));
            }
        }
    }
    if let Some(ending) = script.clips.iter().rfind(|clip| clip.narration_role.as_deref() == Some("conclusion")) {
        out.push(mark(&ending.id, "Ending".into(), "section"));
    }
    // In play order.
    let order = |id: &str| blocks.iter().position(|block| block.id == id).unwrap_or(usize::MAX);
    out.sort_by_key(|mark| order(&mark.block_id));
    out
}

/// The format's length, as its guide sets it: (low, aim, high) ms. None
/// where it sets no runtime (a compilation measured in pages, a commentary).
pub fn target_ms(script: &Script) -> Option<(u64, u64, u64)> {
    use crate::formats::Kind;
    let minutes = |low: f64, aim: f64, high: f64| Some(((low * 60_000.0) as u64, (aim * 60_000.0) as u64, (high * 60_000.0) as u64));
    if script.commentary.is_some() {
        return None;
    }
    if script.compilation {
        let cases = crate::model::case_count(&script.parts).max(1) as f64;
        return match script.kind {
            Kind::Courtroom => minutes(8.0 * cases, 10.0 * cases, 12.0 * cases),
            _ => None,
        };
    }
    match script.kind {
        Kind::Interrogation => minutes(30.0, 40.0, 50.0),
        Kind::Foia => minutes(20.0, 25.0, 30.0),
        Kind::Courtroom => minutes(18.0, 21.5, 25.0),
    }
}

/// A paragraph as one text: narration as written, each clip on its own line
/// as `[video m:ss–m:ss] "quote"`. src/review.ts reads it the same way.
pub fn paragraph_text(paragraph: &Paragraph) -> String {
    paragraph
        .pieces
        .iter()
        .map(|piece| match piece {
            Piece::Say { text } => text.clone(),
            Piece::Clip(cite) => cite_line(cite),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cite_line(cite: &Cite) -> String {
    let quote = if cite.quote.trim().is_empty() { String::new() } else { format!(" \"{}\"", cite.quote.trim()) };
    format!("[{} {}–{}]{quote}", cite.video, clock(cite.start_ms), clock(cite.end_ms))
}

/// A moment as the Doc prints it: whole seconds, rounded down.
fn stamp(ms: u64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    } else {
        format!("{}:{:02}", minutes, seconds % 60)
    }
}

/// A clip's range as the Doc prints it: on its own recording's clock in a
/// joined case, marked when that recording is sound only (src/clips.ts clipRange).
fn range(start_ms: u64, end_ms: u64, parts: &[Part]) -> String {
    let part = (parts.len() > 1).then(|| parts.iter().position(|part| start_ms < part.start_ms + part.duration_ms).unwrap_or(parts.len() - 1)).map(|index| &parts[index]);
    let offset = part.map_or(0, |part| part.start_ms);
    let audio = if part.is_some_and(|part| part.audio) { "Audio " } else { "" };
    format!("{audio}{}–{}", stamp(start_ms.saturating_sub(offset)), stamp(end_ms.saturating_sub(offset)))
}

/// What changed from `before` to `after`, word by word, on `after`'s text:
/// a new block is changed whole, a block whose words changed has each run of
/// changed words (an empty range where words were only taken out), a footage
/// clip whose range moved without its words changing is changed whole, and a
/// block taken out leaves an empty range at the start of the block after it.
pub fn changed(before: &[Block], after: &[Block]) -> Vec<Range> {
    let mut out = Vec::new();
    for block in after {
        match before.iter().find(|old| old.id == block.id) {
            None => out.push(Range { block_id: block.id.clone(), start: 0, end: units(&block.text) }),
            Some(old) if old.text != block.text => out.extend(word_ranges(&old.text, &block.text).into_iter().map(|(start, end)| Range { block_id: block.id.clone(), start, end })),
            Some(old) if old.label != block.label => out.push(Range { block_id: block.id.clone(), start: 0, end: units(&block.text) }),
            Some(_) => {}
        }
    }
    // A block taken out: marked where it was, at the next block that stayed.
    let kept: Vec<&str> = after.iter().map(|block| block.id.as_str()).collect();
    for (index, old) in before.iter().enumerate() {
        if kept.contains(&old.id.as_str()) {
            continue;
        }
        let next = before[index + 1..].iter().find(|later| kept.contains(&later.id.as_str())).map(|later| later.id.clone()).or_else(|| after.last().map(|last| last.id.clone()));
        if let Some(block_id) = next {
            let mark = Range { block_id, start: 0, end: 0 };
            if !out.iter().any(|range| range.block_id == mark.block_id && range.start == 0) {
                out.push(mark);
            }
        }
    }
    out
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Words with their UTF-16 offsets.
fn words(text: &str) -> Vec<(&str, usize, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    let mut start: Option<(usize, usize)> = None;
    for (byte, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some((from_byte, from)) = start.take() {
                out.push((&text[from_byte..byte], from, at));
            }
        } else if start.is_none() {
            start = Some((byte, at));
        }
        at += ch.len_utf16();
    }
    if let Some((from_byte, from)) = start {
        out.push((&text[from_byte..], from, at));
    }
    out
}

/// The runs of `after`'s words not in the longest common run of words with
/// `before`, as UTF-16 ranges; runs a word or two apart are one.
fn word_ranges(before: &str, after: &str) -> Vec<(usize, usize)> {
    let old = words(before);
    let new = words(after);
    let (n, m) = (old.len(), new.len());
    // Past this, the table is too big to be worth it: the whole block changed.
    if n * m > 4_000_000 {
        return vec![(0, units(after))];
    }
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if old[i].0 == new[j].0 { table[i + 1][j + 1] + 1 } else { table[i + 1][j].max(table[i][j + 1]) };
        }
    }
    // Walk it: each new word is kept or changed, and each place words were only taken out is marked.
    let mut changed = vec![false; m];
    let mut removed_before: Vec<usize> = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut dropped = false;
    while i < n || j < m {
        if i < n && j < m && old[i].0 == new[j].0 {
            if dropped {
                removed_before.push(j);
                dropped = false;
            }
            i += 1;
            j += 1;
        } else if j < m && (i == n || table[i][j + 1] >= table[i + 1][j]) {
            changed[j] = true;
            dropped = false;
            j += 1;
        } else {
            dropped = true;
            i += 1;
        }
    }
    if dropped {
        removed_before.push(m);
    }
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut j = 0;
    while j < m {
        if !changed[j] {
            j += 1;
            continue;
        }
        let first = j;
        while j < m && changed[j] {
            j += 1;
        }
        let (start, end) = (new[first].1, new[j - 1].2);
        match runs.last_mut() {
            // Close enough to the run before (a word or two between): one run.
            Some(last) if new.iter().filter(|word| word.1 >= last.1 && word.2 <= start).count() <= 2 => last.1 = end,
            _ => runs.push((start, end)),
        }
    }
    for at in removed_before {
        if changed.get(at).copied().unwrap_or(false) || (at > 0 && changed[at - 1]) {
            continue;
        }
        let point = if at < m { new[at].1 } else { units(after) };
        if !runs.iter().any(|(start, end)| point >= *start && point <= *end) {
            runs.push((point, point));
        }
    }
    runs.sort();
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, text: &str) -> Block {
        Block { id: id.into(), kind: "narration".into(), text: text.into(), ..Default::default() }
    }

    #[test]
    fn marks_the_words_that_changed() {
        let ranges = changed(&[block("a", "He said he was home all night.")], &[block("a", "He swore he was home all night.")]);
        assert_eq!(ranges, vec![Range { block_id: "a".into(), start: 3, end: 8 }]);
    }

    #[test]
    fn marks_a_new_block_whole_and_a_removed_one_where_it_was() {
        let before = [block("a", "One."), block("b", "Two."), block("c", "Three.")];
        let after = [block("a", "One."), block("c", "Three."), block("d", "Four.")];
        let ranges = changed(&before, &after);
        assert!(ranges.contains(&Range { block_id: "d".into(), start: 0, end: 5 }));
        assert!(ranges.contains(&Range { block_id: "c".into(), start: 0, end: 0 }));
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn marks_words_taken_out_with_an_empty_range() {
        let ranges = changed(&[block("a", "He was at the old home.")], &[block("a", "He was at home.")]);
        assert_eq!(ranges, vec![Range { block_id: "a".into(), start: 10, end: 10 }]);
        // "was," became "was": that word changed.
        let ranges = changed(&[block("a", "He was, he said, at home.")], &[block("a", "He was at home.")]);
        assert_eq!(ranges, vec![Range { block_id: "a".into(), start: 3, end: 6 }]);
    }

    #[test]
    fn counts_offsets_as_javascript_does() {
        // "é" is one unit, "😀" two.
        let ranges = changed(&[block("a", "😀 café old")], &[block("a", "😀 café new")]);
        assert_eq!(ranges, vec![Range { block_id: "a".into(), start: 8, end: 11 }]);
    }

    #[test]
    fn writes_a_paragraph_as_src_review_ts_reads_it() {
        use crate::commentary::Cite;
        let paragraph = Paragraph {
            id: "p1".into(),
            pieces: vec![
                Piece::Say { text: "He lied.".into() },
                Piece::Clip(Cite { source: "S1".into(), video: "Stream".into(), link: None, start_ms: 65_400, end_ms: 70_900, quote: "no".into(), verify: None }),
                Piece::Say { text: "Then this one.".into() },
            ],
            note: None,
            marks: Vec::new(),
        };
        // test/review.test.ts holds the same paragraph and these offsets.
        assert_eq!(paragraph_text(&paragraph), "He lied.\n[Stream 1:05–1:10] \"no\"\nThen this one.");
    }

    #[test]
    fn marks_each_note_on_its_words_in_the_block_text() {
        use crate::commentary::{Cite, Mark, Popup as Card};
        let said = Cite { source: "S1".into(), video: "Stream".into(), link: None, start_ms: 65_400, end_ms: 70_900, quote: "I lied".into(), verify: None };
        let paragraph = Paragraph {
            id: "p1".into(),
            pieces: vec![
                Piece::Say { text: "He lied.".into() },
                Piece::Clip(said.clone()),
                Piece::Say { text: "Then 😀 he said: I lied.".into() },
            ],
            note: Some("Show: the 😀 post.\nQuote it.\nFind: the stream.".into()),
            marks: vec![
                Mark { line: "Quote it.".into(), on: "I lied".into(), quote: Some(Card { who: "Him".into(), said }) },
                Mark { line: "Show: the 😀 post.".into(), on: "He lied.".into(), quote: None },
                // Its words are gone from the paragraph: on the whole block.
                Mark { line: "Find: the stream.".into(), on: "gone words".into(), quote: None },
            ],
        };
        let text = paragraph_text(&paragraph);
        let marks = edit_marks(&paragraph);
        assert_eq!(marks.len(), 2);
        let units: Vec<u16> = text.encode_utf16().collect();
        let words = |mark: &EditMark| String::from_utf16(&units[mark.start..mark.end]).unwrap();
        assert_eq!((marks[0].note, words(&marks[0]).as_str(), marks[0].popup.as_ref().map(|card| card.words.as_str())), (1, "I lied", Some("I lied")));
        assert_eq!((marks[1].note, words(&marks[1]).as_str()), (0, "He lied."));
    }

    #[test]
    fn prints_ranges_as_the_doc_does() {
        let parts = vec![
            Part { title: "One".into(), start_ms: 0, duration_ms: 60_000, ..Default::default() },
            Part { title: "Two".into(), start_ms: 60_000, duration_ms: 60_000, audio: true, ..Default::default() },
        ];
        assert_eq!(range(5_400, 20_900, &[]), "0:05–0:20");
        assert_eq!(range(65_000, 90_000, &parts), "Audio 0:05–0:30");
        assert_eq!(range(3_725_000, 3_730_000, &[]), "1:02:05–1:02:10");
    }

    #[test]
    fn every_note_for_the_editor_goes_to_edit() {
        let note = "Find, under 'streamed with Kanye West': clip of him introducing Kanye\nedit: a blue waveform under the call\n";
        assert_eq!(editor_lines(std::iter::once(note)), vec!["edit: Find, under 'streamed with Kanye West': clip of him introducing Kanye", "edit: a blue waveform under the call"]);
    }
}
