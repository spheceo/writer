//! Hearing a recording as Edit hears it (Edit's app/apps/server/src/
//! transcribe.ts, ported), so one transcription system serves both apps and
//! a recording is heard once for both: each transcript is written in Edit's
//! own format (`serialize`), to be shared beside the recording.
//!
//! A file up to 90 s is heard in one go. A longer one is heard twice at once:
//! in even pieces of at most 40 s, each with 4 s either side, for its words
//! and times (Scribe places words late on a long file), and whole, for who
//! speaks (a piece only numbers the voices in it; the whole file's numbers
//! hold from start to end). Every request listens for the case's names
//! (names.rs) as keyterms; names Scribe refuses are dropped and it's asked
//! again without them. A piece heard as gibberish is asked again without
//! names, else the whole file's words stand for it.
//!
//! What's heard is kept as it comes in: each piece until the file is done,
//! so a run cut short pays only for what it hadn't heard, and the whole
//! file's hearing for good (voices don't change with the names).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::net::{self, Failed, Relay};
use crate::settings::Settings;

/// One spoken word. Times are seconds into the recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: f64,
    pub end: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// A recording's transcript, as Edit keeps it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Transcript {
    pub model: String,
    #[serde(default)]
    pub language: Option<String>,
    pub words: Vec<Word>,
    /// Who each voice is ("speaker_0" → "Jane Doe (Mother)").
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub speakers: BTreeMap<String, String>,
    /// The case's names Scribe was told to listen for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keyterms: Vec<String>,
}

/// Seconds of speech each piece of a long file is heard for, besides its context.
pub const PIECE: f64 = 40.0;
/// Seconds heard either side of a piece, so a word across its edge is heard whole by one of them.
pub const CONTEXT: f64 = 4.0;
/// Files up to this long are heard in one go: short enough not to drift.
pub const WHOLE_UP_TO: f64 = 90.0;
/// Pieces heard at once (the relay takes 60 a minute from one address).
const PIECES_AT_ONCE: usize = 4;
/// Waits before asking a busy relay again (a 429), then gives up.
const BUSY_WAITS: [u64; 6] = [15, 30, 60, 60, 60, 60];
/// The longest audio the relay takes in one request is ~3 hours: a longer
/// file's whole hearing is in stretches of this many seconds, its voices
/// numbered per stretch.
const WHOLE_STRETCH: f64 = 2.5 * 3600.0;

/// A transcript as Edit writes it: its fields in order, the maps sorted and
/// left out when empty, each word's speaker only when it has one, and
/// numbers as JavaScript prints them.
pub fn serialize(transcript: &Transcript) -> String {
    let words: Vec<String> = transcript
        .words
        .iter()
        .map(|word| {
            let speaker = word.speaker.as_ref().map(|speaker| format!(",\"speaker\":{}", quote(speaker))).unwrap_or_default();
            format!("{{\"text\":{},\"start\":{},\"end\":{}{speaker}}}", quote(&word.text), number(word.start), number(word.end))
        })
        .collect();
    let language = transcript.language.as_ref().map_or("null".to_string(), |language| quote(language));
    let mut text = format!("{{\"model\":{},\"language\":{language},\"words\":[{}]", quote(&transcript.model), words.join(","));
    if !transcript.speakers.is_empty() {
        let map: Vec<String> = transcript.speakers.iter().map(|(key, value)| format!("{}:{}", quote(key), quote(value))).collect();
        text.push_str(&format!(",\"speakers\":{{{}}}", map.join(",")));
    }
    if !transcript.keyterms.is_empty() {
        text.push_str(&format!(",\"keyterms\":{}", serde_json::to_string(&transcript.keyterms).unwrap_or_default()));
    }
    text.push('}');
    text
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

/// A number as JavaScript's JSON prints it: 1 not 1.0, the shortest form that reads back.
fn number(value: f64) -> String {
    if value.is_finite() { format!("{value}") } else { "null".into() }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn fixed3(value: f64) -> String {
    format!("{value:.3}")
}

/// A piece of a long file: the stretch it speaks for (`core`) and what's sent (`from`–`to`, with context).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub core_from: f64,
    pub core_to: f64,
    pub from: f64,
    pub to: f64,
}

/// A file of `seconds` cut into even pieces of at most `PIECE` seconds, each with `CONTEXT` either side.
pub fn plan(seconds: f64) -> Vec<Piece> {
    let count = ((seconds / PIECE).ceil() as usize).max(1);
    (0..count)
        .map(|index| {
            let core_from = seconds * index as f64 / count as f64;
            let core_to = seconds * (index + 1) as f64 / count as f64;
            Piece { core_from, core_to, from: (core_from - CONTEXT).max(0.0), to: (core_to + CONTEXT).min(seconds) }
        })
        .collect()
}

/// A word as compared across transcripts: lower case, letters and digits only.
pub fn normal(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// The longest common subsequence of two short lists, in runs of two or more
/// (one common word, "the", is chance): pairs of indexes, in order.
fn matched(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let mut table = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            table[i][j] = if a[i] == b[j] && !a[i].is_empty() { table[i + 1][j + 1] + 1 } else { table[i + 1][j].max(table[i][j + 1]) };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] && !a[i].is_empty() {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
        .iter()
        .enumerate()
        .filter(|(at, (x, y))| {
            let prev = at.checked_sub(1).and_then(|at| pairs.get(at));
            let next = pairs.get(at + 1);
            prev.is_some_and(|(px, py)| px + 1 == *x && py + 1 == *y) || next.is_some_and(|(nx, ny)| *nx == x + 1 && *ny == y + 1)
        })
        .map(|(_, pair)| *pair)
        .collect()
}

/// The pieces' words as one transcript. Neighbours both hear the context
/// around their seam: it's handed over at a word both heard at the same
/// moment (the one nearest the seam), so no word is lost or heard twice;
/// with no such word, at the seam itself.
pub fn join(pieces: &[Piece], heard: &[Vec<Word>]) -> Vec<Word> {
    let mut first = vec![0usize; heard.len()];
    let mut last: Vec<usize> = heard.iter().map(Vec::len).collect();
    for index in 0..heard.len().saturating_sub(1) {
        let seam = pieces[index].core_to;
        let (left, right) = (&heard[index], &heard[index + 1]);
        let near = CONTEXT + 0.2;
        let left_at: Vec<usize> = (0..left.len()).filter(|&at| left[at].end >= seam - near).collect();
        let right_at: Vec<usize> = (0..right.len()).filter(|&at| right[at].start <= seam + near).collect();
        let pairs: Vec<(usize, usize)> = matched(
            &left_at.iter().map(|&at| normal(&left[at].text)).collect::<Vec<_>>(),
            &right_at.iter().map(|&at| normal(&right[at].text)).collect::<Vec<_>>(),
        )
        .into_iter()
        .map(|(a, b)| (left_at[a], right_at[b]))
        .filter(|&(a, b)| !normal(&left[a].text).is_empty() && (left[a].start - right[b].start).abs() <= 0.8)
        .collect();
        if let Some(&first_pair) = pairs.first() {
            let middle = |(a, b): (usize, usize)| ((left[a].end + right[b].end) / 2.0 - seam).abs();
            let (a, b) = pairs.iter().copied().fold(first_pair, |best, pair| if middle(pair) < middle(best) { pair } else { best });
            last[index] = a + 1;
            first[index + 1] = b + 1;
        } else {
            let before = |word: &Word| (word.start + word.end) / 2.0 < seam;
            last[index] = left.iter().filter(|word| before(word)).count();
            first[index + 1] = right.iter().filter(|word| before(word)).count();
        }
    }
    heard
        .iter()
        .enumerate()
        .flat_map(|(index, piece)| {
            let end = last[index].max(first[index]).min(piece.len());
            piece[first[index].min(end)..end].to_vec()
        })
        .collect()
}

/// Two long word lists lined up: the longest common subsequence within a
/// band around the diagonal, as their lengths allow (the whole table is too
/// big for hours of words). Pairs of indexes, in order.
pub fn aligned(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 {
        return Vec::new();
    }
    let band = 400usize.max(n.abs_diff(m) + 200);
    let centre = |i: usize| ((i as f64 * m as f64) / n as f64).round() as usize;
    let lo = |i: usize| centre(i).saturating_sub(band);
    let hi = |i: usize| (centre(i) + band).min(m);
    let width = 2 * band + 2;
    // Moves: 1 match (diagonal), 2 skip a, 3 skip b.
    let mut moves = vec![0u8; (n + 1) * width];
    let mut previous = vec![0i32; width];
    let mut current = vec![0i32; width];
    let at = |row: &[i32], i: usize, j: usize| if j < lo(i) || j > hi(i) { -1 } else { row[j - lo(i)] };
    for i in 1..=n {
        current.iter_mut().for_each(|cell| *cell = 0);
        for j in lo(i)..=hi(i) {
            let mut best = -1;
            let mut step = 0u8;
            if j > 0 && a[i - 1] == b[j - 1] && !a[i - 1].is_empty() {
                let diagonal = at(&previous, i - 1, j - 1);
                if diagonal >= 0 {
                    best = diagonal + 1;
                    step = 1;
                }
            }
            let up = at(&previous, i - 1, j);
            if up > best {
                best = up;
                step = 2;
            }
            let left = if j > lo(i) { current[j - 1 - lo(i)] } else { -1 };
            if left > best {
                best = left;
                step = 3;
            }
            current[j - lo(i)] = best.max(0);
            moves[i * width + (j - lo(i))] = step;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (n, m.min(hi(n)));
    while i > 0 && j >= lo(i) {
        match moves[i * width + (j - lo(i))] {
            1 => {
                pairs.push((i - 1, j - 1));
                i -= 1;
                if j == 0 {
                    break;
                }
                j -= 1;
            }
            3 => {
                if j == 0 {
                    break;
                }
                j -= 1;
            }
            _ => i -= 1,
        }
    }
    pairs.reverse();
    pairs
}

/// Gives each word the voice the whole file heard it in: the two transcripts'
/// words lined up by what they say (the whole file's times drift); a word
/// only the pieces heard takes the voice of the nearer matched word.
pub fn voices_from(words: &mut [Word], whole: &[Word]) {
    let pairs = aligned(&words.iter().map(|word| normal(&word.text)).collect::<Vec<_>>(), &whole.iter().map(|word| normal(&word.text)).collect::<Vec<_>>());
    let mut voice: Vec<Option<Option<String>>> = vec![None; words.len()];
    for (a, b) in pairs {
        voice[a] = Some(whole[b].speaker.clone());
    }
    let mut after: Vec<Option<usize>> = vec![None; words.len()];
    let mut next = None;
    for index in (0..words.len()).rev() {
        after[index] = next;
        if voice[index].is_some() {
            next = Some(index);
        }
    }
    let mut before: Option<usize> = None;
    for index in 0..words.len() {
        if let Some(found) = voice[index].clone() {
            words[index].speaker = found;
            before = Some(index);
            continue;
        }
        let gap_before = before.map_or(f64::INFINITY, |prior| words[index].start - words[prior].end);
        let gap_after = after[index].map_or(f64::INFINITY, |later| words[later].start - words[index].end);
        let chosen = if gap_after < gap_before { after[index] } else { before };
        words[index].speaker = chosen.and_then(|at| voice[at].clone()).flatten();
    }
}

/// The share of words' letters outside the Latin script (a Latin-script
/// recording heard as gibberish is mostly not).
pub fn garbled_share(words: &[Word]) -> f64 {
    let (mut letters, mut other) = (0usize, 0usize);
    for ch in words.iter().flat_map(|word| word.text.chars()) {
        if !ch.is_alphabetic() {
            continue;
        }
        letters += 1;
        if !latin(ch) {
            other += 1;
        }
    }
    if letters < 20 { 0.0 } else { other as f64 / letters as f64 }
}

fn latin(ch: char) -> bool {
    matches!(ch as u32, 0x41..=0x5A | 0x61..=0x7A | 0xAA | 0xBA | 0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x24F | 0x1E00..=0x1EFF | 0x2C60..=0x2C7F | 0xA720..=0xA7FF | 0xAB30..=0xAB6F | 0xFF21..=0xFF3A | 0xFF41..=0xFF5A)
}

/// Whether a transcript asked with `heard` listened for `names` well enough
/// not to be heard again: it was given names, and at most half of these are new.
pub fn covers(heard: &[String], names: &[String]) -> bool {
    if names.is_empty() {
        return true;
    }
    let had: Vec<String> = heard.iter().map(|name| name.to_lowercase()).collect();
    if had.is_empty() {
        return false;
    }
    let missing = names.iter().filter(|name| !had.contains(&name.to_lowercase())).count();
    missing * 2 <= names.len()
}

/// Scribe's answer, keeping only spoken words (not spacing or sound events).
pub fn parse_scribe(value: &Value) -> Result<Transcript, String> {
    let words = value["words"].as_array().ok_or("The transcription service returned no words.")?;
    let mut out = Vec::new();
    for item in words {
        if item["type"] != "word" {
            continue;
        }
        let (Some(text), Some(start), Some(end)) = (item["text"].as_str(), item["start"].as_f64(), item["end"].as_f64()) else { continue };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        out.push(Word { text: text.to_string(), start: start.max(0.0), end: end.max(start), speaker: item["speaker_id"].as_str().map(str::to_string) });
    }
    Ok(Transcript { model: "scribe_v2".into(), language: value["language_code"].as_str().map(str::to_string), words: out, ..Default::default() })
}

// ---- Hearing a file ---------------------------------------------------------

/// What `hear` is asked to hear: a file, or a stretch of one (ms).
pub struct Recording<'a> {
    pub file: &'a Path,
    pub span: Option<(u64, u64)>,
}

/// Hears a recording (or a stretch of one) into `work` and answers its
/// transcript, times into what was heard. `work` keeps what was heard;
/// `transcript.json` there is Edit's format. A transcript already there that
/// listened for these names (`covers`) is used as it is.
pub fn hear(recording: &Recording, work: &Path, settings: &Settings, keyterms: &[String], on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<Transcript, String> {
    fs::create_dir_all(work).map_err(|error| error.to_string())?;
    let target = work.join("transcript.json");
    if let Some(kept) = fs::read(&target).ok().and_then(|bytes| serde_json::from_slice::<Transcript>(&bytes).ok()) {
        if covers(&kept.keyterms, keyterms) {
            crate::log::info("hear", format!("{}: heard before ({} words); not heard again", recording.file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(), kept.words.len()));
            return Ok(kept);
        }
    }
    let audio = work.join("audio.ogg");
    let began = std::time::Instant::now();
    let what = format!("{}{}", recording.file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(), recording.span.map(|(from, to)| format!(" {}–{}", crate::commentary::clock(from), crate::commentary::clock(to))).unwrap_or_default());
    encode_audio(recording, &audio)?;
    let seconds = crate::media::streams(&audio).map(|found| found.duration_ms / 1000).unwrap_or(0);
    let pieces = if seconds as f64 <= WHOLE_UP_TO { 1 } else { plan(seconds as f64).len() };
    crate::log::info("hear", format!("{what}: hearing {} of sound in {pieces} {} and whole, with {} names", crate::log::took(std::time::Duration::from_secs(seconds)), if pieces == 1 { "piece" } else { "pieces" }, keyterms.len()));
    let result = hear_audio(&audio, work, settings, keyterms, on_piece);
    let _ = fs::remove_file(&audio);
    let mut transcript = match result {
        Ok(found) => found,
        Err(error) => {
            crate::log::error("hear", format!("{what}: stopped after {}: {error}", crate::log::since_start(began)));
            return Err(error);
        }
    };
    let voices = transcript.words.iter().filter_map(|word| word.speaker.as_deref()).collect::<std::collections::BTreeSet<_>>().len();
    crate::log::info("hear", format!("{what}: heard in {}: {} words, {voices} voices", crate::log::since_start(began), transcript.words.len()));
    transcript.keyterms = keyterms.to_vec();
    let tmp = work.join("transcript.json.tmp");
    fs::write(&tmp, serialize(&transcript)).map_err(|error| error.to_string())?;
    fs::rename(tmp, &target).map_err(|error| error.to_string())?;
    Ok(transcript)
}

fn hear_audio(audio: &Path, work: &Path, settings: &Settings, keyterms: &[String], on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<Transcript, String> {
    let seconds = crate::media::streams(audio).map(|found| found.duration_ms as f64 / 1000.0).unwrap_or(0.0);
    if seconds <= WHOLE_UP_TO {
        on_piece(0, 1);
        let heard = saved(&work.join("whole.json"), &fixed3(seconds), || ask(settings, audio, keyterms))?;
        on_piece(1, 1);
        return Ok(heard);
    }
    let pieces = plan(seconds);
    let names = hex(&Sha256::digest(keyterms.join("\n").as_bytes()))[..12].to_string();
    let pieces_dir = work.join("pieces");
    let stretches = whole_stretches(seconds);
    let total = pieces.len() + stretches.len();
    let done = AtomicUsize::new(0);
    let tick = || on_piece(done.fetch_add(1, Ordering::Relaxed) + 1, total);
    on_piece(0, total);
    let stop = AtomicBool::new(false);
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<Result<Vec<Word>, String>>>> = Mutex::new((0..pieces.len()).map(|_| None).collect());
    let whole: Mutex<Option<Result<Transcript, String>>> = Mutex::new(None);
    std::thread::scope(|scope| {
        // The whole file, for its voices: any earlier hearing of it will do.
        scope.spawn(|| {
            let heard = whole_hearing(audio, work, settings, keyterms, &stretches, &tick);
            if heard.is_err() {
                stop.store(true, Ordering::Relaxed);
            }
            *whole.lock().unwrap() = Some(heard);
        });
        for _ in 0..pieces.len().min(PIECES_AT_ONCE) {
            scope.spawn(|| loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(piece) = pieces.get(index) else { break };
                let file = pieces_dir.join(format!("{names}-{index}-{}-{}.json", fixed3(piece.from), fixed3(piece.to)));
                let heard = saved(&file, "", || ask_piece(settings, audio, work, index, piece, keyterms, ""))
                    .map(|part| part.words.into_iter().map(|word| Word { text: word.text, start: round3(word.start + piece.from), end: round3(word.end + piece.from), speaker: None }).collect());
                let failed = heard.is_err();
                slots.lock().unwrap()[index] = Some(heard);
                tick();
                if failed {
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
            });
        }
    });
    let whole = whole.into_inner().unwrap().unwrap_or_else(|| Err("The whole file wasn't heard.".into()))?;
    let mut heard = Vec::with_capacity(pieces.len());
    for slot in slots.into_inner().unwrap() {
        match slot {
            Some(Ok(words)) => heard.push(words),
            Some(Err(error)) => return Err(error),
            None => return Err("Transcription stopped before every piece was heard. Retry: what was heard is kept.".into()),
        }
    }
    // A piece heard as gibberish: asked again without names, else the whole file's words for that stretch.
    if garbled_share(&whole.words) < 0.2 {
        for (index, piece) in pieces.iter().enumerate() {
            if garbled_share(&heard[index]) < 0.2 {
                continue;
            }
            crate::log::warn("hear", format!("piece {} ({}–{} s) came back garbled; hearing it again without names", index + 1, fixed3(piece.from), fixed3(piece.to)));
            let again: Vec<Word> = ask_piece(settings, audio, work, index, piece, &[], "-again")?
                .words
                .into_iter()
                .map(|word| Word { text: word.text, start: round3(word.start + piece.from), end: round3(word.end + piece.from), speaker: None })
                .collect();
            heard[index] = if garbled_share(&again) < 0.2 {
                again
            } else {
                whole.words.iter().filter(|word| word.start >= piece.from && word.end <= piece.to).map(|word| Word { speaker: None, ..word.clone() }).collect()
            };
        }
    }
    let mut words = join(&pieces, &heard);
    voices_from(&mut words, &whole.words);
    let _ = fs::remove_dir_all(&pieces_dir);
    Ok(Transcript { model: whole.model, language: whole.language, words, ..Default::default() })
}

/// The whole file's stretches for its voices: one, unless it's too long for one request.
fn whole_stretches(seconds: f64) -> Vec<(f64, f64)> {
    let count = ((seconds / WHOLE_STRETCH).ceil() as usize).max(1);
    (0..count).map(|index| (seconds * index as f64 / count as f64, seconds * (index + 1) as f64 / count as f64)).collect()
}

/// The whole file heard for its voices, kept for good. In stretches only
/// when one request can't take it all; then each stretch's voices are its
/// own ("2:speaker_0").
fn whole_hearing(audio: &Path, work: &Path, settings: &Settings, keyterms: &[String], stretches: &[(f64, f64)], tick: &(dyn Fn() + Sync)) -> Result<Transcript, String> {
    if stretches.len() == 1 {
        let seconds = stretches[0].1;
        let heard = saved(&work.join("whole.json"), &fixed3(seconds), || ask(settings, audio, keyterms));
        tick();
        return heard;
    }
    let mut all = Transcript { model: "scribe_v2".into(), ..Default::default() };
    for (index, &(from, to)) in stretches.iter().enumerate() {
        let file = work.join(format!("whole-{index}.json"));
        let heard = saved(&file, &format!("{}-{}", fixed3(from), fixed3(to)), || {
            let cut = work.join(format!("whole-{index}.ogg"));
            let result = cut_audio(audio, from, to, &cut).and_then(|_| ask(settings, &cut, keyterms));
            let _ = fs::remove_file(&cut);
            result
        })?;
        tick();
        all.language = all.language.or(heard.language);
        all.words.extend(heard.words.into_iter().map(|word| Word {
            start: round3(word.start + from),
            end: round3(word.end + from),
            speaker: word.speaker.map(|speaker| format!("{}:{speaker}", index + 1)),
            text: word.text,
        }));
    }
    Ok(all)
}

fn ask_piece(settings: &Settings, audio: &Path, work: &Path, index: usize, piece: &Piece, keyterms: &[String], tag: &str) -> Result<Transcript, String> {
    let file = work.join(format!("piece-{index}{tag}.ogg"));
    let result = cut_audio(audio, piece.from, piece.to, &file).and_then(|_| ask(settings, &file, keyterms));
    let _ = fs::remove_file(&file);
    result
}

/// What `hear` answers, kept in `file` once heard with `tag` (what it must
/// match to be used again), so a later run reads it rather than paying again.
fn saved(file: &Path, tag: &str, hear: impl FnOnce() -> Result<Transcript, String>) -> Result<Transcript, String> {
    #[derive(Serialize, Deserialize)]
    struct Kept {
        tag: String,
        transcript: Transcript,
    }
    if let Some(kept) = fs::read(file).ok().and_then(|bytes| serde_json::from_slice::<Kept>(&bytes).ok()) {
        if kept.tag == tag {
            return Ok(kept.transcript);
        }
    }
    let transcript = hear()?;
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = PathBuf::from(format!("{}.tmp", file.display()));
    if let Ok(bytes) = serde_json::to_vec(&Kept { tag: tag.to_string(), transcript: transcript.clone() }) {
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(&tmp, file);
        }
    }
    Ok(transcript)
}

/// Mono 16 kHz Opus of the recording's first sound (or a stretch of it), as Edit encodes it.
fn encode_audio(recording: &Recording, out: &Path) -> Result<(), String> {
    let source = recording.file.to_string_lossy().into_owned();
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into()];
    if let Some((from, to)) = recording.span {
        args.extend(["-ss".into(), fixed3(from as f64 / 1000.0), "-t".into(), fixed3(to.saturating_sub(from) as f64 / 1000.0)]);
    }
    args.extend(["-i".into(), source, "-map".into(), "0:a:0".into()]);
    args.extend(opus(out));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::proc::run("ffmpeg", &refs, Duration::from_secs(60 * 60)).map(|_| ()).map_err(|error| format!("Could not take the sound out of the footage: {error}"))
}

fn cut_audio(audio: &Path, from: f64, to: f64, out: &Path) -> Result<(), String> {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into(), "-ss".into(), fixed3(from), "-t".into(), fixed3(to - from), "-i".into(), audio.to_string_lossy().into_owned()];
    args.extend(opus(out));
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::proc::run("ffmpeg", &refs, Duration::from_secs(10 * 60)).map(|_| ()).map_err(|error| format!("Could not cut the sound: {error}"))
}

fn opus(out: &Path) -> Vec<String> {
    ["-ac", "1", "-ar", "16000", "-c:a", "libopus", "-b:a", "32k", "-application", "voip"].iter().map(|arg| arg.to_string()).chain([out.to_string_lossy().into_owned()]).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One audio file heard by the relay. A busy relay (429) is asked again
/// after a wait, unless the day's allowance is used up; names Scribe refuses
/// (400 or 422: nothing heard or billed) are dropped and it's asked again.
fn ask(settings: &Settings, audio: &Path, keyterms: &[String]) -> Result<Transcript, String> {
    let relay = Relay::new(settings)?;
    let mut names = keyterms.to_vec();
    let mut waits = BUSY_WAITS.iter();
    let mut attempt = 1;
    let piece = audio.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    loop {
        let began = std::time::Instant::now();
        let answer = relay.transcribe(audio, &names);
        if began.elapsed() > Duration::from_secs(20) {
            crate::log::info("hear", format!("{piece}: Scribe answered in {}", crate::log::since_start(began)));
        }
        match answer {
            Ok(value) => return parse_scribe(&value),
            Err(Failed::Status(code @ (400 | 422), body)) if !names.is_empty() => {
                crate::log::warn("hear", format!("{piece}: Scribe refused the case's {} names ({code}: {}); asking without them", names.len(), net::said(&body)));
                names.clear();
            }
            Err(Failed::Status(429, body)) if !net::said(&body).to_lowercase().contains("allowance") => match waits.next() {
                Some(wait) => {
                    crate::log::warn("hear", format!("{piece}: transcription busy (429), attempt {attempt}; asking again in {wait} s"));
                    attempt += 1;
                    std::thread::sleep(Duration::from_secs(*wait));
                }
                None => return Err(format!("Transcription stayed busy after {attempt} attempts. {} Retry: what was heard is kept.", net::said(&body))),
            },
            Err(Failed::Unsent(reason)) => match waits.next() {
                Some(wait) => {
                    crate::log::warn("hear", format!("{piece}: couldn't reach Writer's service ({reason}), attempt {attempt}; trying again in {wait} s"));
                    attempt += 1;
                    std::thread::sleep(Duration::from_secs(*wait));
                }
                None => return Err(format!("Could not reach Writer's service after {attempt} attempts ({reason}).")),
            },
            Err(Failed::Status(code, body)) => return Err(crate::transcribe::explain(code, &net::said(&body))),
            Err(Failed::Network(reason)) => return Err(format!("The connection to Writer's service broke ({reason}). Retry: what was heard is kept.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str, start: f64, speaker: Option<&str>) -> Word {
        Word { text: text.into(), start, end: start + 0.3, speaker: speaker.map(str::to_string) }
    }

    #[test]
    fn writes_a_transcript_as_edit_does() {
        let mut speakers = BTreeMap::new();
        speakers.insert("speaker_1".to_string(), "Officer".to_string());
        speakers.insert("speaker_0".to_string(), "Jane".to_string());
        let transcript = Transcript {
            model: "scribe_v2".into(),
            language: Some("eng".into()),
            words: vec![Word { text: "Hi".into(), start: 0.5, end: 0.75, speaker: Some("speaker_1".into()) }, Word { text: "you".into(), start: 1.0, end: 1.5, speaker: None }],
            speakers,
            keyterms: Vec::new(),
        };
        // Edit's test (transcribe.test.ts), byte for byte.
        assert_eq!(
            serialize(&transcript),
            r#"{"model":"scribe_v2","language":"eng","words":[{"text":"Hi","start":0.5,"end":0.75,"speaker":"speaker_1"},{"text":"you","start":1,"end":1.5}],"speakers":{"speaker_0":"Jane","speaker_1":"Officer"}}"#
        );
    }

    #[test]
    fn plans_even_pieces_of_at_most_40_s_with_4_s_either_side() {
        let pieces = plan(100.0);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0], Piece { core_from: 0.0, core_to: 100.0 / 3.0, from: 0.0, to: 100.0 / 3.0 + 4.0 });
        assert_eq!(pieces[2].to, 100.0);
        assert!((pieces[1].from - (100.0 / 3.0 - 4.0)).abs() < 1e-9);
    }

    #[test]
    fn joins_pieces_at_a_word_both_heard_nothing_lost_or_twice() {
        let pieces = [Piece { core_from: 0.0, core_to: 10.0, from: 0.0, to: 14.0 }, Piece { core_from: 10.0, core_to: 20.0, from: 6.0, to: 20.0 }];
        let left = vec![w("we", 8.0, None), w("were", 8.5, None), w("going", 9.6, None), w("to", 10.2, None), w("the", 10.6, None), w("car", 11.0, None)];
        let right = vec![w("we're", 8.1, None), w("going", 9.65, None), w("to", 10.25, None), w("the", 10.6, None), w("car", 11.05, None), w("now", 12.0, None)];
        let texts = |words: Vec<Word>| words.into_iter().map(|word| word.text).collect::<Vec<_>>();
        assert_eq!(texts(join(&pieces, &[left, right])), ["we", "were", "going", "to", "the", "car", "now"]);
        // Nothing in common: handed over at the seam.
        let quiet = join(&pieces, &[vec![w("hello", 9.0, None), w("there", 10.5, None)], vec![w("hey", 9.2, None), w("you", 10.6, None), w("again", 13.0, None)]]);
        assert_eq!(texts(quiet), ["hello", "you", "again"]);
    }

    #[test]
    fn each_word_takes_the_voice_the_whole_file_heard_it_in() {
        // The whole file's times drift by 20 s; its voices hold.
        let whole = vec![
            w("Hey", 30.0, Some("speaker_1")),
            w("man,", 30.3, Some("speaker_1")),
            w("are", 30.6, Some("speaker_1")),
            w("you", 31.0, Some("speaker_1")),
            w("Reginald?", 31.3, Some("speaker_1")),
            w("Yes,", 33.0, Some("speaker_0")),
            w("I", 33.2, Some("speaker_0")),
            w("am.", 33.4, Some("speaker_0")),
        ];
        let mut words = vec![w("Hey,", 10.0, None), w("man,", 10.3, None), w("are", 10.6, None), w("you", 11.0, None), w("Reginald?", 11.3, None), w("Uh,", 12.6, None), w("yes,", 13.0, None), w("I", 13.2, None), w("am.", 13.4, None)];
        voices_from(&mut words, &whole);
        let voices: Vec<&str> = words.iter().map(|word| word.speaker.as_deref().unwrap_or("")).collect();
        assert_eq!(voices, ["speaker_1", "speaker_1", "speaker_1", "speaker_1", "speaker_1", "speaker_0", "speaker_0", "speaker_0", "speaker_0"]);
        let s = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
        assert_eq!(aligned(&s(&["a", "b", "c"]), &s(&["x", "a", "c"])), vec![(0, 1), (2, 2)]);
    }

    #[test]
    fn hears_again_for_names_only_when_given_none_or_mostly_new_ones() {
        let s = |items: &[&str]| items.iter().map(|item| item.to_string()).collect::<Vec<_>>();
        assert!(covers(&s(&["A", "B"]), &[]));
        assert!(!covers(&[], &s(&["A"])));
        assert!(covers(&s(&["a", "b", "c"]), &s(&["A", "B", "D"])));
        assert!(!covers(&s(&["a"]), &s(&["A", "B", "C"])));
    }

    #[test]
    fn tells_gibberish_from_a_latin_script_recording() {
        let latin: Vec<Word> = "the quick brown fox jumps over the lazy dog".split(' ').map(|text| w(text, 0.0, None)).collect();
        assert_eq!(garbled_share(&latin), 0.0);
        let other: Vec<Word> = "Привет как дела сегодня утром".split(' ').map(|text| w(text, 0.0, None)).collect();
        assert!(garbled_share(&other) > 0.9);
    }
}
