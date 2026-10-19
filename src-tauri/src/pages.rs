//! How many Letter pages a footage script runs, worked out from its text
//! alone, as Repack's scriptPages does (apps/web/src/domain/script-pages.ts):
//! 12 pt Arial at 1.15 on a Letter sheet's text area, a 19 pt bold heading,
//! a timestamp line or cut-in that would cross the bottom starting the next
//! page whole. A compilation's script runs 6 to 7 pages.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::formats::{Kind, Style};
use crate::model::{Clip, Script};

/// The page's geometry in CSS px.
const WIDTH: f64 = 624.0;
const HEIGHT: f64 = 864.0;
const TEXT: f64 = 16.0;
const LINE: f64 = 18.4;
const HEADING: f64 = 19.0 * 96.0 / 72.0;
const RANGE_GAP: f64 = 4.8;

/// A compilation's script, in pages.
pub const COMPILATION_PAGES: (usize, usize) = (6, 7);

#[derive(Deserialize)]
struct Face {
    widths: HashMap<String, f64>,
    pairs: HashMap<String, f64>,
}

#[derive(Deserialize)]
struct Arial {
    regular: Face,
    bold: Face,
}

/// Arial's advance widths and kerning pairs, as Repack measured them in Chrome.
fn arial() -> &'static Arial {
    static ARIAL: OnceLock<Arial> = OnceLock::new();
    ARIAL.get_or_init(|| serde_json::from_str(include_str!("arial.json")).expect("Arial's metrics are valid JSON"))
}

fn text_width(text: &str, px: f64, bold: bool) -> f64 {
    let face = if bold { &arial().bold } else { &arial().regular };
    let digit = face.widths.get("0").copied().unwrap_or(556.0);
    let mut total = 0.0;
    let mut previous: Option<char> = None;
    for c in text.chars() {
        total += face.widths.get(c.encode_utf8(&mut [0; 4]) as &str).copied().unwrap_or(digit);
        if let Some(p) = previous {
            total += face.pairs.get(&format!("{p}{c}")).copied().unwrap_or(0.0);
        }
        previous = Some(c);
    }
    total * px / 1000.0
}

/// Lines a paragraph takes when wrapped at spaces, a word wider than the
/// line broken wherever it must be.
fn line_count(text: &str, px: f64, bold: bool) -> usize {
    let mut lines = 0;
    for paragraph in text.split('\n') {
        lines += 1;
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if text_width(&next, px, bold) <= WIDTH {
                line = next;
                continue;
            }
            if !line.is_empty() {
                lines += 1;
            }
            line = word.to_string();
            while text_width(&line, px, bold) > WIDTH {
                let chars: Vec<char> = line.chars().collect();
                let mut fit = 1;
                while fit + 1 <= chars.len() && text_width(&chars[..fit + 1].iter().collect::<String>(), px, bold) <= WIDTH {
                    fit += 1;
                }
                line = chars[fit..].iter().collect();
                lines += 1;
            }
        }
    }
    lines
}

fn stamp(ms: u64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    } else {
        format!("{minutes}:{:02}", seconds % 60)
    }
}

/// A range as the script prints it: on its own recording's clock, marked
/// when the recording is audio only.
fn clip_range(clip: &Clip, script: &Script) -> String {
    let parts = &script.parts;
    let (offset, audio) = if parts.len() > 1 {
        let part = &parts[crate::tidy::part_at(parts, clip.start_ms)];
        (part.start_ms, part.audio)
    } else {
        (0, false)
    };
    format!(
        "{}{}–{}",
        if audio { "Audio " } else { "" },
        stamp(clip.start_ms.saturating_sub(offset)),
        stamp(clip.end_ms.saturating_sub(offset))
    )
}

/// The script's title line, as its Doc heads it.
pub fn heading(script: &Script) -> String {
    if let Some(own) = script.heading.as_deref().map(str::trim).filter(|own| !own.is_empty()) {
        return own.to_string();
    }
    let series = match script.kind {
        Kind::Foia => "FOIA",
        Kind::Courtroom => "COURT",
        Kind::Interrogation => "INTERRO",
    };
    if script.compilation || script.suspects.is_empty() {
        format!("{series} {}", script.title)
    } else {
        format!("{series} {} - {}", script.suspects.join(" & "), script.title)
    }
}

/// How many of the script's assets it lists: a compilation's videos, else its one link.
fn asset_count(script: &Script) -> usize {
    let link = |url: &str| (url.starts_with("http://") || url.starts_with("https://")) && !url.contains(char::is_whitespace);
    let mut urls: Vec<&str> = script.parts.iter().filter_map(|part| part.url.as_deref()).filter(|url| link(url)).collect();
    if script.compilation {
        urls.len()
    } else if script.source_kind == "compilation" && script.parts.len() > 1 {
        // One case across videos lists each video once.
        let mut seen = std::collections::HashSet::new();
        urls.retain(|url| seen.insert(*url));
        urls.len()
    } else {
        usize::from(link(&script.source_url))
    }
}

/// How many Letter pages the script runs, as its Doc lays it out.
pub fn count(script: &Script, clips: &[Clip]) -> usize {
    let assets = asset_count(script);
    let mut used = line_count(&heading(script), HEADING, true) as f64 * HEADING * 1.15
        + 32.0
        + if assets > 0 { LINE + 6.0 } else { 0.0 }
        + assets as f64 * LINE
        + (16.0 + LINE);
    let mut gap = 32.0;
    let mut page = 0;
    let mut place = |used: &mut f64, gap: f64, height: f64, keep: f64| {
        let top = *used + gap;
        if top > 1.0 && top + height + keep > HEIGHT + 1.0 {
            page += 1;
            *used = height;
        } else {
            *used = top + height;
        }
    };
    // Blocks: the footage before each cut-in, and that cut-in.
    let mut blocks: Vec<(Vec<&Clip>, Option<&Clip>)> = Vec::new();
    let mut footage = Vec::new();
    for clip in clips {
        if clip.is_cut_in() {
            blocks.push((std::mem::take(&mut footage), Some(clip)));
        } else {
            footage.push(clip);
        }
    }
    if !footage.is_empty() {
        blocks.push((footage, None));
    }
    // Where a compilation's cases start: its transitions, and a hidden one's first block.
    let hidden = crate::formats::Format::new(script.kind, script.compilation, &script.title).style == Style::Hidden;
    let starts: Vec<usize> = if script.compilation {
        blocks.iter().enumerate().filter(|(index, (clips, _))| clips.is_empty() || (*index == 0 && hidden)).map(|(index, _)| index).collect()
    } else {
        Vec::new()
    };
    let running = script.kind == Kind::Courtroom && !script.compilation;
    for (index, (clips, cue)) in blocks.iter().enumerate() {
        let divider = starts.iter().position(|start| *start == index).is_some_and(|case| case > 0);
        let narration_lines = |cue: &Clip| line_count(cue.narration.as_deref().unwrap_or(""), TEXT, false) as f64;
        if !clips.is_empty() {
            let (mut rows, mut row) = (1.0, 0.0);
            for (at, clip) in clips.iter().enumerate() {
                let mut pieces = Vec::new();
                if at > 0 {
                    pieces.push(text_width("+", TEXT, false));
                }
                pieces.push(text_width("(", TEXT, false) + text_width(&clip_range(clip, script), TEXT, false) + text_width(")", TEXT, false));
                for piece in pieces {
                    if row > 0.0 && row + RANGE_GAP + piece > WIDTH {
                        rows += 1.0;
                        row = piece;
                    } else {
                        row += if row > 0.0 { RANGE_GAP } else { 0.0 } + piece;
                    }
                }
            }
            if divider {
                place(&mut used, gap, LINE, 16.0 + rows * LINE);
                gap = 16.0;
            }
            place(&mut used, gap, rows * LINE, 0.0);
            gap = if running {
                if cue.is_some() && index == 0 { 16.0 } else { 0.0 }
            } else if cue.is_some() {
                16.0
            } else {
                32.0
            };
        }
        if let Some(cue) = cue {
            if divider && clips.is_empty() {
                place(&mut used, gap, LINE, 16.0 + narration_lines(cue) * LINE);
                gap = 16.0;
            }
            place(&mut used, gap, narration_lines(cue) * LINE, 0.0);
            gap = if running { 0.0 } else { 32.0 };
        }
    }
    if script.kind == Kind::Courtroom {
        place(&mut used, 32.0, LINE, 0.0);
    }
    page + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_arial_and_wraps_lines() {
        // As Repack's textWidth measures them, kerning included.
        assert!((text_width("Hello", 16.0, false) - 36.448).abs() < 0.001);
        assert!((text_width("INTERRO The Case", HEADING, true) - 233.674_666).abs() < 0.001);
        assert_eq!(line_count("short", TEXT, false), 1);
        assert_eq!(line_count(&"word ".repeat(200), TEXT, false), 13);
    }

    #[test]
    fn a_long_script_runs_onto_more_pages() {
        let mut script: Script = serde_json::from_value(serde_json::json!({
            "id": "a1", "title": "Case", "sourceKind": "file", "sourceUrl": "",
            "originalPath": "", "status": "ready", "stage": "", "progress": 100,
            "error": null, "durationMs": 10_000, "revision": 1, "createdAt": 0, "updatedAt": 0
        }))
        .unwrap();
        let block = |n: usize| {
            vec![
                Clip { id: format!("c{n}"), start_ms: 0, end_ms: 5_000, ..Default::default() },
                Clip { id: format!("n{n}"), start_ms: 0, end_ms: 3_000, kind: Some("cut-in".into()), narration: Some("A sentence of narration. ".repeat(12)), ..Default::default() },
            ]
        };
        script.clips = (0..2).flat_map(block).collect();
        assert_eq!(count(&script, &script.clips), 1);
        let long: Vec<Clip> = (0..40).flat_map(block).collect();
        assert!(count(&script, &long) > 5);
    }
}
