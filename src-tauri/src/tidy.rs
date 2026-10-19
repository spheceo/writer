//! Repack's mechanical clean-up of a cut (apps/web/src/domain/script.ts and
//! script-plan.ts): breathing room at clip edges, the clip-edge review and
//! its safe fixes, Courtroom's relaxed joins, and silence tightening read
//! from word timings and the sound itself.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::model::{Clip, Part, Word};

/// Breathing room kept before a clip's first word and after its last.
pub const LEAD_MS: u64 = 150;
pub const TAIL_MS: u64 = 400;
/// Breathing room never comes closer than this to a neighbouring word.
const CLEAR_MS: u64 = 60;
/// Peaks are sampled every 20 ms of source audio.
pub const PEAK_MS: u64 = 20;
/// The beat a tightened pause keeps, and the hesitation before an answer.
pub const PAUSE_BEAT_MS: u64 = 1_000;
pub const ANSWER_BEAT_MS: u64 = 2_000;
/// At or past this, nobody is talking: an empty room, a wait.
const DEAD_AIR_MS: u64 = 10_000;
/// Courtroom, later on: pauses shorter than this stay, and a block's clips
/// less than RELAXED_JOIN_MS apart play as one range.
pub const RELAXED_PAUSE_MS: u64 = 5_000;
const RELAXED_JOIN_MS: u64 = 10_000;
/// A FOIA video never runs shorter than this.
pub const FOIA_FLOOR_MS: u64 = 10 * 60_000;

/// A word-anchored range with breathing room: up to `lead` before the first
/// word and `tail` after the last, stopping short of the words either side.
pub fn breathe_with(words: &[Word], start_ms: u64, end_ms: u64, duration_ms: u64, lead: u64, tail: u64) -> (u64, u64) {
    let next = words.partition_point(|word| word.start_ms < end_ms);
    let prev = words[..next].iter().rev().find(|word| word.end_ms <= start_ms);
    let floor = prev.map(|word| word.end_ms + CLEAR_MS).unwrap_or(0);
    let ceiling = words.get(next).map(|word| word.start_ms.saturating_sub(CLEAR_MS)).unwrap_or(duration_ms.max(end_ms));
    let start = start_ms.min(start_ms.saturating_sub(lead).max(floor));
    let end = end_ms.max(end_ms.saturating_add(tail).min(ceiling));
    (start, end)
}

pub fn breathe(words: &[Word], start_ms: u64, end_ms: u64, duration_ms: u64) -> (u64, u64) {
    breathe_with(words, start_ms, end_ms, duration_ms, LEAD_MS, TAIL_MS)
}

fn finished(text: &str) -> bool {
    text.trim_end_matches(['"', '”', '’', '\'', ')', ']']).ends_with(['.', '?', '!', '…'])
}

/// A word bare of the punctuation after it, lowercased, for the filler lists.
fn core(text: &str) -> String {
    text.trim_end_matches([',', '.', '!', '?', '…']).to_lowercase()
}

/// "um", "ummm", "uh", "uhm", "hmm", "mm", "mhm" and their kin.
fn noise(word: &str, mm: bool, mhm: bool) -> bool {
    let rest_all = |rest: &str, c: char| !rest.is_empty() && rest.chars().all(|x| x == c);
    word.strip_prefix('u').is_some_and(|rest| rest_all(rest, 'm') || rest_all(rest, 'h'))
        || word == "uhm"
        || word.strip_prefix("hm").is_some_and(|rest| rest_all(rest, 'm'))
        || (mm && word.strip_prefix('m').is_some_and(|rest| rest_all(rest, 'm')))
        || (mhm && word == "mhm")
}

/// Words that trail a finished thought, or lead into the next one.
fn trailing(text: &str) -> bool {
    let word = core(text);
    noise(&word, true, true)
        || matches!(
            word.as_str(),
            "yeah" | "yes" | "yep" | "okay" | "ok" | "right" | "so" | "and" | "but" | "well" | "like" | "alright" | "anyway" | "or" | "now"
        )
}

/// Hesitations that leave a line hanging wherever they fall.
fn hanging(text: &str) -> bool {
    let word = core(text);
    noise(&word, false, false) || matches!(word.as_str(), "so" | "and" | "but" | "well" | "like" | "or" | "now")
}

fn hesitant(text: &str) -> bool {
    let word = core(text);
    noise(&word, true, false) || matches!(word.as_str(), "so" | "and" | "but" | "well" | "like" | "okay" | "ok" | "yeah" | "or" | "now")
}

/// A clip edge that probably doesn't land where the thought does, with the
/// word to move it to and how many words that cuts or, with `adds`, brings in.
#[derive(Debug, Clone)]
pub struct Issue {
    pub id: String,
    pub end: bool,
    pub issue: String,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone)]
pub struct Fix {
    pub word: usize,
    pub dropped: usize,
    pub adds: bool,
}

/// Repack's boundaryIssues: trailing past a finished thought into fillers
/// ("no. Yeah, so"), stopping mid-sentence, ending on the other speaker's
/// brief reaction, starting on a filler, or starting mid-sentence.
pub fn boundary_issues(clips: &[Clip], words: &[Word], corrections: &BTreeMap<String, String>) -> Vec<Issue> {
    let text = |i: usize| corrections.get(&words[i].id).map(String::as_str).unwrap_or(words[i].text.as_str());
    let span = |a: isize, b: isize| {
        let (a, b) = (a.max(0) as usize, (b.max(0) as usize).min(words.len()));
        (a..b.max(a)).map(text).collect::<Vec<_>>().join(" ")
    };
    let mut issues = Vec::new();
    for clip in clips.iter().filter(|clip| !clip.is_cut_in()) {
        let inside: Vec<usize> = words
            .iter()
            .enumerate()
            .skip_while(|(_, word)| (word.start_ms + word.end_ms) / 2 < clip.start_ms)
            .take_while(|(_, word)| word.start_ms < clip.end_ms)
            .filter(|(_, word)| {
                let mid = (word.start_ms + word.end_ms) / 2;
                mid >= clip.start_ms && mid < clip.end_ms
            })
            .map(|(i, _)| i)
            .collect();
        let (Some(&a), Some(&b)) = (inside.first(), inside.last()) else { continue };
        let last_finished = |from: usize| (from.saturating_sub(12).max(a)..=from).rev().find(|&i| finished(text(i)));
        let fix = |i: usize, dropped: usize| (i >= a).then_some(Fix { word: i, dropped, adds: false });
        let (ai, bi) = (a as isize, b as isize);
        // End: trailing fillers after a finished thought, or fillers left hanging.
        let mut k = 0;
        while b >= a + k && trailing(text(b - k)) && k < 4 {
            k += 1;
        }
        let reply = words[b].speaker.is_some()
            && b > a
            && words[b].speaker != words[b - 1].speaker
            && !text(b - 1).trim_end_matches(['"', '”', '’', '\'', ')']).ends_with('?')
            && inside.iter().filter(|&&i| words[i].speaker == words[b].speaker && i + 4 > b).count() <= 3;
        if k > 0 && b >= a + k && finished(text(b - k)) {
            issues.push(Issue {
                id: clip.id.clone(),
                end: true,
                issue: format!("runs on past the end of the thought into \"{}\"", span(bi - k as isize + 1, bi + 1)),
                fix: fix(b - k, k),
            });
        } else if hanging(text(b)) {
            let mut k = 1;
            while b >= a + k && hanging(text(b - k)) && k < 4 {
                k += 1;
            }
            let at = if b >= a + k { last_finished(b - k) } else { None };
            issues.push(Issue {
                id: clip.id.clone(),
                end: true,
                issue: format!("ends on a filler, \"{}\"", span(bi - k as isize + 1, bi + 1)),
                fix: at.and_then(|at| fix(at, b - at)),
            });
        } else if reply {
            let mut r = b;
            while r > a && words[r - 1].speaker == words[b].speaker {
                r -= 1;
            }
            issues.push(Issue { id: clip.id.clone(), end: true, issue: "ends on the other speaker's brief reaction".into(), fix: fix(r - 1, b - r + 1) });
        } else if !finished(text(b)) && words.get(b + 1).is_some_and(|next| next.start_ms.saturating_sub(words[b].end_ms) < 600) {
            // Finishing the sentence beats cutting it back, when it ends within a few words.
            let mut ahead = b + 1;
            while ahead < words.len() && ahead - b <= 8 && words[ahead].speaker == words[b].speaker && !finished(text(ahead)) {
                ahead += 1;
            }
            let finishes = ahead < words.len() && ahead - b <= 8 && words[ahead].speaker == words[b].speaker;
            issues.push(Issue {
                id: clip.id.clone(),
                end: true,
                issue: "stops mid-sentence".into(),
                fix: if finishes {
                    Some(Fix { word: ahead, dropped: ahead - b, adds: true })
                } else {
                    last_finished(b).and_then(|at| fix(at, b - at))
                },
            });
        }
        // Start: mid-sentence, straight on from the same speaker's unfinished words.
        if a > 0 {
            let before = &words[a - 1];
            if !finished(text(a - 1)) && before.speaker == words[a].speaker && words[a].start_ms.saturating_sub(before.end_ms) < 1_500 {
                let mut from = a - 1;
                while from > a.saturating_sub(15) && !finished(text(from - 1)) {
                    from -= 1;
                }
                let found = from > 0 && finished(text(from - 1));
                // The sentence's first real word: fillers dropped from its front leave a clean start.
                let mut opens = from;
                while opens < a && hesitant(text(opens)) {
                    opens += 1;
                }
                if found && opens >= a {
                    continue;
                }
                issues.push(Issue {
                    id: clip.id.clone(),
                    end: false,
                    issue: format!("starts mid-sentence, cutting off \"{}\"", span((opens as isize).max(ai - 8), ai)),
                    fix: found.then_some(Fix { word: opens, dropped: a - opens, adds: true }),
                });
                continue;
            }
        }
        // Start: a filler or connector opening the clip.
        let mut h = 0;
        while a + h <= b && hesitant(text(a + h)) && h < 4 {
            h += 1;
        }
        if h > 0 && a + h <= b {
            issues.push(Issue {
                id: clip.id.clone(),
                end: false,
                issue: format!("starts on a filler, \"{}\"", span(ai, ai + h as isize)),
                fix: Some(Fix { word: a + h, dropped: h, adds: false }),
            });
        }
    }
    issues
}

/// Repack's polishClips: every safe edge fix (at most 4 words cut, or 8
/// brought in to finish a thought) and breathing room around every clip's
/// words. Returns the clips and how many edges it fixed.
pub fn polish(clips: Vec<Clip>, words: &[Word], corrections: &BTreeMap<String, String>, duration_ms: u64) -> (Vec<Clip>, usize) {
    const MAX_DROP: usize = 4;
    let fixes: Vec<Issue> = boundary_issues(&clips, words, corrections)
        .into_iter()
        .filter(|issue| issue.fix.as_ref().is_some_and(|fix| fix.dropped <= if fix.adds { MAX_DROP * 2 } else { MAX_DROP }))
        .collect();
    let polished = clips
        .into_iter()
        .map(|clip| {
            if clip.is_cut_in() {
                return clip;
            }
            let (mut start_ms, mut end_ms) = (clip.start_ms, clip.end_ms);
            for issue in fixes.iter().filter(|issue| issue.id == clip.id) {
                let Some(word) = issue.fix.as_ref().and_then(|fix| words.get(fix.word)) else { continue };
                if issue.end {
                    end_ms = word.end_ms;
                } else {
                    start_ms = word.start_ms;
                }
            }
            // Fixes at both ends of a short clip can cross: then it keeps its edges.
            if end_ms < start_ms + 200 {
                (start_ms, end_ms) = (clip.start_ms, clip.end_ms);
            }
            let first = words.iter().find(|word| (word.start_ms + word.end_ms) / 2 >= start_ms);
            let last = words.iter().rev().find(|word| (word.start_ms + word.end_ms) / 2 < end_ms);
            let (Some(first), Some(last)) = (first, last) else { return Clip { start_ms, end_ms, ..clip } };
            if last.end_ms <= first.start_ms {
                return Clip { start_ms, end_ms, ..clip };
            }
            let room = breathe(words, first.start_ms, last.end_ms, duration_ms);
            // As far as an edge may go without touching a neighbouring word.
            let reach = breathe_with(words, first.start_ms, last.end_ms, duration_ms, u64::MAX, u64::MAX);
            // An edge cutting into a neighbour comes back to the breathing
            // room; otherwise it keeps whichever gives the words more room.
            Clip {
                start_ms: if start_ms < reach.0 { room.0 } else { start_ms.min(room.0) },
                end_ms: if end_ms > reach.1 { room.1 } else { end_ms.max(room.1) },
                ..clip
            }
        })
        .collect();
    (polished, fixes.len())
}

/// Which recording a moment is in; 0 for one recording.
pub fn part_at(parts: &[Part], ms: u64) -> usize {
    parts.iter().position(|part| ms < part.start_ms + part.duration_ms).unwrap_or(parts.len().saturating_sub(1))
}

/// Where a moment of footage first plays in the cut, on the edited clock.
fn source_position(clips: &[Clip], source_ms: u64) -> Option<u64> {
    let mut cursor = 0;
    for clip in clips {
        if !clip.is_cut_in() && source_ms >= clip.start_ms && source_ms < clip.end_ms {
            return Some(cursor + source_ms - clip.start_ms);
        }
        cursor += clip.end_ms - clip.start_ms;
    }
    None
}

/// Where Courtroom's relaxed pacing begins: a compilation's second
/// transition, a single case's halfway point.
fn relaxed_from(clips: &[Clip], compilation: bool) -> u64 {
    if compilation {
        let (mut at, mut transitions, mut footage) = (0, 0, false);
        for clip in clips {
            if clip.is_cut_in() {
                if !footage {
                    transitions += 1;
                    if transitions == 2 {
                        return at;
                    }
                }
                footage = false;
            } else {
                footage = true;
            }
            at += clip.end_ms - clip.start_ms;
        }
    }
    clips.iter().map(|clip| clip.end_ms - clip.start_ms).sum::<u64>() / 2
}

/// Where the pacing is relaxed, two clips of a block from one recording less
/// than 10 s apart play as one range, the footage between kept; never across
/// a narrator, whose voice stays out of clips.
pub fn join_late(clips: Vec<Clip>, words: &[Word], speakers: &BTreeMap<String, String>, parts: &[Part], compilation: bool) -> Vec<Clip> {
    let from = relaxed_from(&clips, compilation);
    let narrated = |a: u64, b: u64| {
        words.iter().any(|word| {
            word.start_ms < b
                && word.end_ms > a
                && word.speaker.as_ref().and_then(|speaker| speakers.get(speaker)).is_some_and(|name| name.to_lowercase().starts_with("narrator"))
        })
    };
    let mut joined: Vec<Clip> = Vec::with_capacity(clips.len());
    for clip in &clips {
        if let Some(last) = joined.last_mut() {
            if !last.is_cut_in()
                && !clip.is_cut_in()
                && clip.start_ms > last.end_ms
                && clip.start_ms - last.end_ms < RELAXED_JOIN_MS
                && part_at(parts, last.start_ms) == part_at(parts, clip.start_ms)
                && source_position(&clips, last.start_ms).unwrap_or(0) >= from
                && !narrated(last.end_ms, clip.start_ms)
            {
                last.end_ms = clip.end_ms;
                continue;
            }
        }
        joined.push(clip.clone());
    }
    joined
}

/// A stretch with no speech: where a cut would take it from, how long the
/// wait was, and how much of it is as loud as speech (0 to 1).
#[derive(Debug, Clone)]
pub struct Silence {
    pub start_ms: u64,
    pub end_ms: u64,
    pub gap_ms: u64,
    pub sound: f32,
}

/// Repack's silences: every gap of `min_ms` or more between words, less the
/// beat it keeps (the longer `answer_keep_ms` after a question), rated by how
/// much of what a cut removes is as loud as the recording's typical speech.
pub fn silences(words: &[Word], peaks: &[f32], duration_ms: u64, min_ms: u64, keep_ms: u64, answer_keep_ms: u64) -> Vec<Silence> {
    let index = |ms: u64| (ms / PEAK_MS) as usize;
    let mut speech: Vec<f32> = Vec::new();
    for word in words {
        let end = (word.end_ms.div_ceil(PEAK_MS) as usize).min(peaks.len());
        if index(word.start_ms) < end {
            speech.extend_from_slice(&peaks[index(word.start_ms)..end]);
        }
    }
    speech.sort_by(f32::total_cmp);
    let typical = speech.get(speech.len() / 2).copied().unwrap_or(0.0);
    let mut found = Vec::new();
    let mut add = |from: u64, to: u64, previous: Option<&Word>| {
        let gap_ms = to.saturating_sub(from);
        if gap_ms < min_ms {
            return;
        }
        let question = previous.is_some_and(|word| word.text.trim_end_matches(['"', '”', '\'']).ends_with('?'));
        let answer = question && gap_ms < 60_000;
        let _dead = !answer && (gap_ms >= DEAD_AIR_MS || previous.is_none());
        let keep = if answer { answer_keep_ms } else { keep_ms };
        let start_ms = if from > 0 { from + keep / 2 } else { 0 };
        let end_ms = if to < duration_ms { to.saturating_sub(keep / 2) } else { duration_ms };
        if end_ms < start_ms + 100 {
            return;
        }
        let (a, b) = (index(start_ms).min(peaks.len()), (end_ms.div_ceil(PEAK_MS) as usize).min(peaks.len()));
        let window = &peaks[a..b.max(a)];
        let loud = if typical > 0.0 { window.iter().filter(|&&value| value >= typical).count() } else { 0 };
        found.push(Silence { start_ms, end_ms, gap_ms, sound: if window.is_empty() { 0.0 } else { loud as f32 / window.len() as f32 } });
    };
    let mut spoken = 0;
    let mut previous: Option<&Word> = None;
    for word in words {
        add(spoken, word.start_ms, previous);
        if word.end_ms >= spoken {
            previous = Some(word);
        }
        spoken = spoken.max(word.end_ms);
    }
    add(spoken, duration_ms, previous);
    found
}

/// Stretches of over a second inside a range far quieter than the range's own
/// speech, read from the sound alone: a word can wrongly span a silence. Each
/// keeps 150 ms at both ends.
pub fn quiet_runs(peaks: &[f32], start_ms: u64, end_ms: u64) -> Vec<(u64, u64)> {
    const MIN_MS: u64 = 1_000;
    const KEEP_MS: u64 = 150;
    let from = start_ms.div_ceil(PEAK_MS) as usize;
    let to = ((end_ms / PEAK_MS) as usize).min(peaks.len());
    if to <= from || (to - from) as u64 * PEAK_MS < MIN_MS {
        return Vec::new();
    }
    let levels = &peaks[from..to];
    let mut sorted = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    let quiet = sorted[(sorted.len() as f32 * 0.9) as usize] * 0.2;
    let mut runs = Vec::new();
    let mut run: Option<usize> = None;
    for i in 0..=levels.len() {
        let still = i < levels.len() && levels[i] <= quiet;
        match (still, run) {
            (true, None) => run = Some(i),
            (false, Some(at)) => {
                if (i - at) as u64 * PEAK_MS >= MIN_MS {
                    runs.push(((from + at) as u64 * PEAK_MS + KEEP_MS, (from + i) as u64 * PEAK_MS - KEEP_MS));
                }
                run = None;
            }
            _ => {}
        }
    }
    runs
}

/// Whether any footage clip still includes some of a source interval.
fn kept(clips: &[Clip], start_ms: u64, end_ms: u64) -> bool {
    clips.iter().any(|clip| !clip.is_cut_in() && start_ms < clip.end_ms && end_ms > clip.start_ms)
}

/// Removes a source interval from every footage clip, keeping what sits outside it.
pub fn remove_range(clips: Vec<Clip>, start_ms: u64, end_ms: u64) -> Vec<Clip> {
    if end_ms <= start_ms {
        return clips;
    }
    clips
        .into_iter()
        .flat_map(|clip| {
            if clip.is_cut_in() || clip.end_ms <= start_ms || clip.start_ms >= end_ms {
                return vec![clip];
            }
            let mut kept = Vec::new();
            if clip.start_ms < start_ms {
                kept.push(Clip { end_ms: start_ms, ..clip.clone() });
            }
            if clip.end_ms > end_ms {
                // A second piece is a new timestamp: the comments stay on the first.
                let (id, notes) = if kept.is_empty() { (clip.id.clone(), clip.notes.clone()) } else { (uuid::Uuid::new_v4().to_string(), Vec::new()) };
                kept.push(Clip { id, start_ms: end_ms, notes, ..clip });
            }
            kept
        })
        .collect()
}

/// Splits a footage clip running across two recordings, so each clip is cut
/// from one. A piece under 100 ms is left out.
pub fn fit_to_parts(clips: Vec<Clip>, parts: &[Part]) -> Vec<Clip> {
    if parts.len() < 2 {
        return clips;
    }
    clips
        .into_iter()
        .flat_map(|clip| {
            if clip.is_cut_in() {
                return vec![clip];
            }
            let mut pieces: Vec<Clip> = Vec::new();
            for part in parts {
                let start_ms = clip.start_ms.max(part.start_ms);
                let end_ms = clip.end_ms.min(part.start_ms + part.duration_ms);
                if end_ms < start_ms + 100 {
                    continue;
                }
                let (id, notes) = if pieces.is_empty() { (clip.id.clone(), clip.notes.clone()) } else { (uuid::Uuid::new_v4().to_string(), Vec::new()) };
                pieces.push(Clip { id, start_ms, end_ms, notes, ..clip.clone() });
            }
            pieces
        })
        .collect()
}

/// The shortest beat, from `beat_ms` up, whose tightened cut still runs the
/// floor. None when even an untightened cut falls short.
pub fn beat_for_floor(runtime_at: impl Fn(u64) -> u64, beat_ms: u64, floor_ms: u64, untightened_ms: u64) -> Option<u64> {
    if runtime_at(beat_ms) >= floor_ms {
        return Some(beat_ms);
    }
    if runtime_at(untightened_ms) < floor_ms {
        return None;
    }
    let (mut low, mut high) = (beat_ms, untightened_ms);
    while high - low > 100 {
        let middle = (low + high) / 2;
        if runtime_at(middle) >= floor_ms {
            high = middle;
        } else {
            low = middle;
        }
    }
    Some(high)
}

/// How a cut was tightened: the clips, and how many quiet pauses and
/// stalls inside intro clips it cut.
pub struct Tightened {
    pub clips: Vec<Clip>,
    pub pauses: usize,
    pub stalls: usize,
}

/// What tightening needs to know of the script.
pub struct Footage<'a> {
    pub words: &'a [Word],
    pub peaks: &'a [f32],
    pub parts: &'a [Part],
    pub duration_ms: u64,
    pub compilation: bool,
    /// Courtroom: the second half (or second case on) is let breathe.
    pub relaxed: bool,
}

/// Repack's silence tightening at one beat: quiet gaps in the kept footage
/// cut to the beat (two before an answer), relaxed Courtroom pauses under 5 s
/// kept, gaps with sound under them kept, then pauses over a second inside
/// intro clips cut from the sound. `intro` says which footage blocks are intro.
pub fn tighten(footage: &Footage, clips: &[Clip], beat_ms: u64, intro: &dyn Fn(usize) -> bool) -> Tightened {
    let gaps = silences(footage.words, footage.peaks, footage.duration_ms, 2_000, beat_ms, ANSWER_BEAT_MS.max(beat_ms));
    let quiet: Vec<&Silence> = gaps
        .iter()
        .filter(|gap| {
            gap.sound < 0.1
                && kept(clips, gap.start_ms, gap.end_ms)
                && !(footage.relaxed
                    && gap.gap_ms < RELAXED_PAUSE_MS
                    && source_position(clips, gap.start_ms).is_some_and(|at| at >= relaxed_from(clips, footage.compilation)))
        })
        .collect();
    let mut cut = fit_to_parts(clips.to_vec(), footage.parts);
    for gap in &quiet {
        cut = remove_range(cut, gap.start_ms, gap.end_ms);
    }
    // Intro clips are quick proof: a pause over a second inside one stalls
    // the opening, so it is read from the sound. Only the intro's own clips.
    let mut block = 0;
    let mut stalls = 0;
    let mut out = Vec::with_capacity(cut.len());
    for clip in cut {
        if clip.is_cut_in() {
            block += 1;
            out.push(clip);
            continue;
        }
        if !intro(block) {
            out.push(clip);
            continue;
        }
        let runs = quiet_runs(footage.peaks, clip.start_ms, clip.end_ms);
        if runs.is_empty() {
            out.push(clip);
            continue;
        }
        let mut from = clip.start_ms;
        let mut pieces = Vec::new();
        for (start, end) in runs {
            pieces.push(Clip { id: uuid::Uuid::new_v4().to_string(), start_ms: from, end_ms: start, notes: Vec::new(), ..clip.clone() });
            from = end;
            stalls += 1;
        }
        pieces.push(Clip { id: uuid::Uuid::new_v4().to_string(), start_ms: from, end_ms: clip.end_ms, notes: Vec::new(), ..clip.clone() });
        // The timestamp's comments stay with its first piece.
        if let Some(first) = pieces.iter_mut().find(|piece| piece.end_ms >= piece.start_ms + 100) {
            first.notes = clip.notes.clone();
        }
        out.extend(pieces.into_iter().filter(|piece| piece.end_ms >= piece.start_ms + 100));
    }
    Tightened { clips: out, pauses: quiet.len(), stalls }
}

/// The footage's 20 ms peaks, kept beside the script once read: transcription
/// saves them, and a script made before that reads them once from its footage.
pub fn peaks(dir: &Path, footage: &Path) -> Vec<f32> {
    let path = dir.join("peaks.bin");
    if let Ok(bytes) = fs::read(&path) {
        return bytes.chunks_exact(2).map(|pair| f32::from(u16::from_le_bytes([pair[0], pair[1]])) / 65_535.0).collect();
    }
    let peaks = crate::media::peaks(footage).unwrap_or_default();
    if !peaks.is_empty() {
        keep_peaks(dir, &peaks);
    }
    peaks
}

pub fn keep_peaks(dir: &Path, peaks: &[f32]) {
    let bytes: Vec<u8> = peaks.iter().flat_map(|peak| ((peak.clamp(0.0, 1.0) * 65_535.0).round() as u16).to_le_bytes()).collect();
    let tmp = dir.join("peaks.bin.tmp");
    if fs::write(&tmp, bytes).is_ok() {
        let _ = fs::rename(tmp, dir.join("peaks.bin"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(said: &[(&str, &str, u64, u64)]) -> Vec<Word> {
        said.iter()
            .enumerate()
            .map(|(i, (text, speaker, start, end))| Word { id: format!("w{i}"), text: text.to_string(), start_ms: *start, end_ms: *end, speaker: Some(speaker.to_string()) })
            .collect()
    }

    fn clip(id: &str, start_ms: u64, end_ms: u64) -> Clip {
        Clip { id: id.into(), start_ms, end_ms, ..Default::default() }
    }

    #[test]
    fn fixes_an_edge_that_runs_on_into_fillers() {
        let said = words(&[("No.", "a", 0, 400), ("Yeah,", "a", 500, 800), ("so", "a", 900, 1_100), ("Next.", "b", 3_000, 3_400)]);
        let issues = boundary_issues(&[clip("c", 0, 1_200)], &said, &BTreeMap::new());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].end && issues[0].issue.contains("runs on past"));
        let (fixed, count) = polish(vec![clip("c", 0, 1_200)], &said, &BTreeMap::new(), 10_000);
        assert_eq!(count, 1);
        // Back to "No." with room that stops short of "Yeah,".
        assert_eq!((fixed[0].start_ms, fixed[0].end_ms), (0, 440));
    }

    #[test]
    fn fixes_that_cross_leave_a_short_clip_as_it_was() {
        // Starts on a filler ("Okay.") and ends on one ("so"), whose fix goes back to "Okay.".
        let said = words(&[("Okay.", "a", 0, 300), ("John,", "a", 400, 700), ("so", "a", 800, 1_000), ("Next.", "b", 3_000, 3_300)]);
        let (fixed, _) = polish(vec![clip("c", 0, 1_100)], &said, &BTreeMap::new(), 10_000);
        assert!(fixed[0].end_ms > fixed[0].start_ms);
    }

    #[test]
    fn finishes_a_sentence_cut_short() {
        let said = words(&[("Do", "a", 0, 200), ("you", "a", 250, 400), ("feel", "a", 450, 600), ("remorse?", "a", 650, 1_000), ("No.", "b", 2_000, 2_300)]);
        let (fixed, count) = polish(vec![clip("c", 0, 520)], &said, &BTreeMap::new(), 10_000);
        assert_eq!(count, 1);
        assert_eq!(fixed[0].end_ms, 1_400);
    }

    #[test]
    fn tightens_only_quiet_gaps() {
        let said = words(&[("Hi.", "a", 0, 500), ("There.", "a", 5_500, 6_000), ("Again.", "a", 11_000, 11_500)]);
        // Loud sound in the second gap (untranscribed speech), silence in the first.
        let mut peaks = vec![0.5f32; 600];
        for peak in &mut peaks[30..270] {
            *peak = 0.0;
        }
        let footage = Footage { words: &said, peaks: &peaks, parts: &[], duration_ms: 12_000, compilation: false, relaxed: false };
        let done = tighten(&footage, &[clip("c", 0, 12_000)], PAUSE_BEAT_MS, &|_| false);
        assert_eq!(done.pauses, 1);
        let kept: u64 = done.clips.iter().map(|clip| clip.end_ms - clip.start_ms).sum();
        assert_eq!(kept, 12_000 - 4_000);
    }

    #[test]
    fn finds_the_shortest_beat_that_keeps_the_floor() {
        let beat = beat_for_floor(|beat| 9_000 + beat, 1_000, 10_000, 50_000);
        assert!(beat.is_some_and(|beat| (1_000..=1_100).contains(&beat)));
        assert_eq!(beat_for_floor(|_| 5_000, 1_000, 10_000, 50_000), None);
    }
}
