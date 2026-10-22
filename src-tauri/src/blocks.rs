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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    /// `footage`, `narration`, `montage`, `heading` or `paragraph`.
    pub kind: String,
    /// What a footage clip is printed as in the Doc: its range ("Audio 1:02–1:40").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub text: String,
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

pub fn of(script: &Script) -> Vec<Block> {
    match script.commentary.as_ref() {
        Some(commentary) => commentary.draft.as_ref().map(commentary_blocks).unwrap_or_default(),
        None => footage_blocks(script),
    }
}

fn footage_blocks(script: &Script) -> Vec<Block> {
    script
        .clips
        .iter()
        .map(|clip| {
            if clip.is_cut_in() {
                Block { id: clip.id.clone(), kind: "narration".into(), label: None, text: clip.narration.clone().unwrap_or_default() }
            } else {
                let text = script
                    .words
                    .iter()
                    .filter(|word| word.start_ms < clip.end_ms && word.end_ms > clip.start_ms)
                    .map(|word| script.corrections.get(&word.id).unwrap_or(&word.text).as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                Block { id: clip.id.clone(), kind: "footage".into(), label: Some(range(clip.start_ms, clip.end_ms, &script.parts)), text }
            }
        })
        .collect()
}

fn commentary_blocks(draft: &crate::commentary::Draft) -> Vec<Block> {
    let mut out = Vec::new();
    if !draft.open.is_empty() {
        out.push(Block { id: "open".into(), kind: "montage".into(), label: None, text: draft.open.iter().map(cite_line).collect::<Vec<_>>().join("\n") });
    }
    for section in &draft.sections {
        if let Some(first) = section.paragraphs.first() {
            out.push(Block { id: format!("{}:heading", first.id), kind: "heading".into(), label: None, text: section.heading.clone() });
        }
        for paragraph in &section.paragraphs {
            out.push(Block { id: paragraph.id.clone(), kind: "paragraph".into(), label: None, text: paragraph_text(paragraph) });
        }
    }
    out
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
        Block { id: id.into(), kind: "narration".into(), label: None, text: text.into() }
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
        };
        // test/review.test.ts holds the same paragraph and these offsets.
        assert_eq!(paragraph_text(&paragraph), "He lied.\n[Stream 1:05–1:10] \"no\"\nThen this one.");
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
}
