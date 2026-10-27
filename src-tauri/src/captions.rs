//! YouTube's own captions as a recording's words, in place of hearing it:
//! free, and minutes for a hearing of many hours. Used for every YouTube source, including
//! compilation cases; other recordings fall back to hearing.
//!
//! YouTube's speech recognition (the `en-orig` track, never a translation)
//! gives each word its start, punctuation and capitals, and marks a change of
//! speaker with ">>" (`isSpeakerChange`), but says nothing of who speaks.
//! Each ">>" starts a turn of its own (`t0001`, `t0002`…), which the turns
//! pass (turns.rs) then gives to people by reading the conversation.
//!
//! A word has no end: it ends at the next word's start or after the time
//! it takes to say (`spoken_ms`), whichever comes first, so the pauses
//! between words are there for the edges and the tightening.
//!
//! Measured on Oct 25 2026 against ElevenLabs on a 20-minute hearing Writer
//! had heard (3,042 words matched): starts 49 ms from ElevenLabs' (110 ms
//! for nine words in ten); ends 58 ms late (140 ms), where the next word's
//! start would be 373 ms; 93% of ">>" marks fall on a change of speaker,
//! but they mark only about two thirds of the changes.
//!
//! Kept as `captions.json` beside the recording's hearing, never as its
//! `transcript.json`, so it's never handed to Edit as heard words.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::hearing::{Transcript, Word};
use crate::proc;

/// What a transcript made from captions is called (`Transcript.model`).
pub const MODEL: &str = "youtube-captions";
/// Milliseconds a letter: the 75th percentile of 89k words ElevenLabs timed
/// in Writer's scripts (2 letters 120 ms, 8 letters 459, 12 letters 740).
const MS_A_LETTER: f64 = 60.0;
/// A digit is said in a word or more ("$8,320").
const MS_A_DIGIT: f64 = 250.0;
const SHORTEST_MS: f64 = 60.0;

/// Whether a script's footage takes YouTube's captions where they're there.
pub fn wanted(script: &crate::model::Script) -> bool {
    script.source_kind == "youtube" || script.parts.iter().any(|part| part.url.as_deref().is_some_and(|url| crate::youtube::check_url(url).is_ok()))
}

/// How long `word` takes to say, at most, in ms.
pub fn spoken_ms(word: &str) -> f64 {
    let letters = word.chars().filter(|c| c.is_alphabetic()).count() as f64;
    let digits = word.chars().filter(|c| c.is_ascii_digit()).count() as f64;
    (letters * MS_A_LETTER + digits * MS_A_DIGIT).max(SHORTEST_MS)
}

/// The recording's words from YouTube's captions, kept in `work`; None when
/// the video has no English speech recognition of its own (a translated
/// track, or only captions an uploader timed by line, which isn't timed by
/// the word), or YouTube can't be asked.
pub fn transcript(url: &str, work: &Path, cookies: Option<&Path>) -> Option<Transcript> {
    let kept = work.join("captions.json");
    if let Some(found) = cached(work) {
        return Some(found);
    }
    let _ = fs::create_dir_all(work);
    let raw = fetch(url, work, cookies)?;
    let transcript = parse(&raw);
    if transcript.words.is_empty() {
        return None;
    }
    let _ = fs::write(&kept, serde_json::to_vec(&transcript).ok()?);
    Some(transcript)
}

/// Caption provenance is separate from a paid hearing, including when
/// another background stage decides whether a new hearing is necessary.
pub(crate) fn cached(work: &Path) -> Option<Transcript> {
    fs::read(work.join("captions.json")).ok().and_then(|bytes| serde_json::from_slice::<Transcript>(&bytes).ok())
        .filter(|found| found.model == MODEL && !found.words.is_empty())
}

/// yt-dlp's json3 of the `en-orig` track. A busy YouTube (429) is asked again once.
fn fetch(url: &str, work: &Path, cookies: Option<&Path>) -> Option<Value> {
    let template = work.join("captions").to_string_lossy().into_owned();
    let jar = cookies.filter(|path| path.is_file()).map(|path| path.to_string_lossy().into_owned());
    let mut args = vec!["--skip-download", "--no-playlist", "--no-warnings", "--write-auto-subs", "--sub-langs", "en-orig", "--sub-format", "json3", "-o", &template];
    if let Some(path) = jar.as_deref() {
        args.push("--cookies");
        args.push(path);
    }
    args.push(url);
    let file = work.join("captions.en-orig.json3");
    for attempt in 0..2 {
        let _ = fs::remove_file(&file);
        match proc::run("yt-dlp", &args, Duration::from_secs(120)) {
            Ok(_) if file.is_file() => {
                let value = fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
                let _ = fs::remove_file(&file);
                return value;
            }
            Ok(_) => return None,
            Err(error) if attempt == 0 && error.contains("429") => std::thread::sleep(Duration::from_secs(20)),
            Err(error) => {
                crate::log::warn("hear", format!("YouTube's captions couldn't be read: {}", error.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("yt-dlp failed")));
                return None;
            }
        }
    }
    None
}

/// json3's events into words: each segment a word at its event's start plus
/// its offset; ">>" (or `isSpeakerChange`) opens a new turn; sounds in
/// brackets ("[music]", "[applause]") are left out.
pub fn parse(value: &Value) -> Transcript {
    let mut said: Vec<(String, f64, bool)> = Vec::new();
    for event in value["events"].as_array().into_iter().flatten() {
        let start = event["tStartMs"].as_f64().unwrap_or(0.0);
        for seg in event["segs"].as_array().into_iter().flatten() {
            let text = seg["utf8"].as_str().unwrap_or("");
            let mut change = seg["isSpeakerChange"].as_i64() == Some(1);
            let mut text = text.trim();
            if let Some(rest) = text.strip_prefix(">>") {
                change = true;
                text = rest.trim();
            }
            if text.is_empty() {
                if change {
                    // A ">>" on its own: the next word opens the turn.
                    said.push((String::new(), start + seg["tOffsetMs"].as_f64().unwrap_or(0.0), true));
                }
                continue;
            }
            let at = start + seg["tOffsetMs"].as_f64().unwrap_or(0.0);
            for (index, piece) in text.split_whitespace().enumerate() {
                said.push((piece.to_string(), at, change && index == 0));
            }
        }
    }
    // Bracketed sounds go, and a ">>" left alone passes its turn to the next word.
    let mut words: Vec<(String, f64, bool)> = Vec::new();
    let mut carry = false;
    let mut bracket = false;
    for (text, at, change) in said {
        let opens = text.starts_with('[');
        if opens || bracket || text.is_empty() {
            carry |= change;
            bracket = (opens || bracket) && !text.ends_with(']');
            continue;
        }
        words.push((text, at, change || carry));
        carry = false;
    }
    words.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut turn = 0usize;
    let mut out = Vec::with_capacity(words.len());
    for (index, (text, at, change)) in words.iter().enumerate() {
        if *change || index == 0 {
            turn += 1;
        }
        let next = words.get(index + 1).map(|word| word.1).unwrap_or(at + 2_000.0);
        let end = (at + spoken_ms(text)).min(next.max(at + 1.0));
        out.push(Word { text: text.clone(), start: at / 1000.0, end: end / 1000.0, speaker: Some(format!("t{turn:04}")) });
    }
    Transcript { model: MODEL.into(), language: Some("en".into()), words: out, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn turns_open_at_each_speaker_change_and_ends_are_spoken_lengths() {
        let value = json!({ "events": [
            { "tStartMs": 0, "dDurationMs": 1_237_720, "id": 1 },
            { "tStartMs": 80, "dDurationMs": 3120, "segs": [{ "utf8": "paint" }, { "utf8": " the", "tOffsetMs": 160 }, { "utf8": " truck.", "tOffsetMs": 320 }] },
            { "tStartMs": 1510, "aAppend": 1, "segs": [{ "utf8": "\n" }] },
            { "tStartMs": 3200, "segs": [{ "utf8": ">> Where's", "isSpeakerChange": 1 }, { "utf8": " [music]", "tOffsetMs": 200 }, { "utf8": " the", "tOffsetMs": 239 }, { "utf8": " truck?", "tOffsetMs": 320 }] },
            { "tStartMs": 6720, "segs": [{ "utf8": ">>" }, { "utf8": " $8,320", "tOffsetMs": 80 }] },
        ]});
        let transcript = parse(&value);
        let texts: Vec<&str> = transcript.words.iter().map(|word| word.text.as_str()).collect();
        assert_eq!(texts, vec!["paint", "the", "truck.", "Where's", "the", "truck?", "$8,320"]);
        let turns: Vec<&str> = transcript.words.iter().map(|word| word.speaker.as_deref().unwrap()).collect();
        assert_eq!(turns, vec!["t0001", "t0001", "t0001", "t0002", "t0002", "t0002", "t0003"]);
        // "paint" would take 300 ms but "the" starts at 240.
        assert_eq!(transcript.words[0].end, 0.24);
        // "truck." is said in 300 ms, long before the next turn at 3.2 s.
        assert!((transcript.words[2].end - 0.70).abs() < 1e-9);
        assert_eq!(transcript.model, MODEL);
    }

    #[test]
    fn a_word_takes_about_sixty_ms_a_letter() {
        assert_eq!(spoken_ms("a"), 60.0);
        assert_eq!(spoken_ms("Objection."), 540.0);
        assert_eq!(spoken_ms("$8,320"), 1_000.0);
    }
}
