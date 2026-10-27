//! YouTube's own captions as a recording's words, in place of hearing it:
//! free, and minutes for a hearing of many hours. Used for every YouTube source, including
//! compilation cases; other recordings fall back to hearing.
//!
//! Prefer word-timed English captions, especially the original `en-orig`
//! track; English manual and other automatic tracks are fallbacks. A manual
//! cue's words are spread across its duration: approximate until exact.rs
//! hears the chosen footage. YouTube's word captions mark a change of
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
/// the video has no usable English captions, or YouTube can't be asked.
/// Manual cue timings are estimates; the script's caption provenance ensures
/// selected footage still goes through exact hearing before cutting.
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

/// Fetch English manual/automatic tracks together. yt-dlp prefers a manual
/// track over an automatic one with the same language code. Keep any usable
/// earlier file even when a later track fails, and isolate files from retries.
fn fetch(url: &str, work: &Path, cookies: Option<&Path>) -> Option<Value> {
    let folder = work.join(format!(".captions-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&folder).ok()?;
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    let _scratch = Scratch(folder.clone());
    let template = folder.join("captions").to_string_lossy().into_owned();
    let jar = cookies.filter(|path| path.is_file()).map(|path| path.to_string_lossy().into_owned());
    // --dump-single-json otherwise implies simulation, even with --write-subs.
    let mut args = vec!["--skip-download", "--no-simulate", "--no-playlist", "--no-warnings", "--write-subs", "--write-auto-subs", "--sub-langs", "en.*", "--sub-format", "json3", "--dump-single-json", "-o", &template];
    if let Some(path) = jar.as_deref() { args.extend(["--cookies", path]); }
    args.push(url);
    for attempt in 0..2 {
        if attempt > 0 {
            if let Ok(entries) = fs::read_dir(&folder) {
                for entry in entries.flatten() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        let result = proc::run("yt-dlp", &args, Duration::from_secs(120));
        let info = result.as_ref().ok().and_then(|text| text.lines().rev().find_map(|line| serde_json::from_str::<Value>(line).ok()));
        if let Some((language, value)) = select(&folder, info.as_ref()) {
            crate::log::info("hear", format!("YouTube captions: {language} ({})", if word_timed(&value) { "word-timed" } else { "cue timings estimated; chosen footage is heard exactly" }));
            return Some(value);
        }
        match result {
            Err(error) if attempt == 0 && error.contains("429") => crate::jobs::wait(Duration::from_secs(20)).ok()?,
            Err(error) => {
                crate::log::warn("hear", format!("YouTube's captions couldn't be read: {}", error.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("yt-dlp failed")));
                return None;
            }
            Ok(_) => return None,
        }
    }
    None
}

/// Real English tracks: `en`, `en-orig`, and regional tags such as `en-US`.
/// `en.*` also writes translated tracks named with another video's id
/// (`en-Zd4rRTALbmM`, `en-en-Zd4rRTALbmM`). Those are not this recording.
fn english(language: &str) -> bool {
    if !language.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return false;
    }
    let lower = language.to_ascii_lowercase().replace('_', "-");
    if lower == "en" || lower == "en-orig" {
        return true;
    }
    let mut parts = lower.split('-').filter(|part| !part.is_empty());
    if parts.next() != Some("en") {
        return false;
    }
    let rest: Vec<&str> = parts.collect();
    !rest.is_empty() && rest.iter().all(|part| (2..=4).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphabetic()))
}

fn word_timed(value: &Value) -> bool {
    let (mut total, mut timed) = (0, 0);
    for event in value["events"].as_array().into_iter().flatten() {
        for segment in event["segs"].as_array().into_iter().flatten() {
            let count = segment["utf8"].as_str().unwrap_or("").trim_start_matches('>').split_whitespace().count();
            total += count;
            if count == 1 && segment["tOffsetMs"].as_f64().is_some() { timed += 1; }
        }
    }
    total > 0 && timed * 2 >= total
}

/// Stable preference: original English word timings, other word timings,
/// manual cues, then automatic cues; plain en before regional variants.
/// Automatically translated tracks aren't a transcript of the spoken words.
fn select(folder: &Path, info: Option<&Value>) -> Option<(String, Value)> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(folder).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(language) = name.strip_prefix("captions.").and_then(|name| name.strip_suffix(".json3")).filter(|language| english(language)) else { continue };
        let manual = info.and_then(|info| info["subtitles"].get(language)).and_then(Value::as_array).is_some_and(|tracks| !tracks.is_empty());
        let translated = info.and_then(|info| info["requested_subtitles"][language]["url"].as_str()).is_some_and(|url| url.split(['?', '&']).any(|part| part.starts_with("tlang=")));
        if translated && !manual { continue; }
        let Some(value) = fs::read(entry.path()).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).filter(|value| !parse(value).words.is_empty()) else { continue };
        let rank = match (language == "en-orig", word_timed(&value), manual) {
            (true, true, _) => 0,
            (_, true, _) => 1,
            (_, _, true) => 2,
            _ => 3,
        };
        candidates.push(((rank, language != "en", language.to_string()), value));
    }
    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    candidates.into_iter().next().map(|((_, _, language), value)| (language, value))
}

/// json3's events into words: each segment a word at its event's start plus
/// its offset; ">>" (or `isSpeakerChange`) opens a new turn; sounds in
/// brackets ("[music]", "[applause]") are left out.
pub fn parse(value: &Value) -> Transcript {
    let events = value["events"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut said: Vec<(String, f64, bool, Option<f64>)> = Vec::new();
    for (event_index, event) in events.iter().enumerate() {
        let Some(start) = event["tStartMs"].as_f64().filter(|at| at.is_finite() && *at >= 0.0) else { continue };
        let next_event = events[event_index + 1..].iter().filter_map(|next| next["tStartMs"].as_f64()).find(|at| *at > start);
        let duration = event["dDurationMs"].as_f64().filter(|duration| *duration > 0.0).map(|duration| start + duration);
        let event_end = match (duration, next_event) { (Some(a), Some(b)) => Some(a.min(b)), (a, b) => a.or(b) };
        let segments = event["segs"].as_array().map(Vec::as_slice).unwrap_or_default();
        for (segment_index, segment) in segments.iter().enumerate() {
            let text = segment["utf8"].as_str().unwrap_or("").trim();
            let change = segment["isSpeakerChange"].as_i64() == Some(1) || text.starts_with(">>");
            let text = text.strip_prefix(">>").unwrap_or(text).trim();
            let offset = segment["tOffsetMs"].as_f64().unwrap_or(0.0);
            if !offset.is_finite() || offset < 0.0 { continue; }
            let at = start + offset;
            let next = segments[segment_index + 1..].iter().filter_map(|next| next["tOffsetMs"].as_f64()).find(|next| *next > offset).map(|next| start + next);
            let limit = match (next, event_end) { (Some(a), Some(b)) => Some(a.min(b)), (a, b) => a.or(b) }.filter(|end| *end > at);
            let pieces: Vec<&str> = text.split_whitespace().collect();
            if pieces.is_empty() {
                if change { said.push((String::new(), at, true, limit)); }
                continue;
            }
            let total: f64 = pieces.iter().map(|piece| spoken_ms(piece)).sum();
            let span = limit.map(|end| end - at).unwrap_or(total);
            let mut elapsed = 0.0;
            for (index, piece) in pieces.iter().enumerate() {
                said.push((piece.to_string(), at + elapsed / total * span, change && index == 0, limit));
                elapsed += spoken_ms(piece);
            }
        }
    }
    let mut words: Vec<(String, f64, bool, Option<f64>)> = Vec::new();
    let (mut carry, mut bracket) = (false, false);
    for (text, at, change, limit) in said {
        let opens = text.starts_with('[');
        if opens || bracket || text.is_empty() {
            carry |= change;
            bracket = (opens || bracket) && !text.ends_with(']');
            continue;
        }
        words.push((text, at, change || carry, limit));
        carry = false;
    }
    words.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut turn = 0usize;
    let mut out = Vec::with_capacity(words.len());
    for (index, (text, at, change, limit)) in words.iter().enumerate() {
        if *change || index == 0 { turn += 1; }
        let next = words.get(index + 1).map(|word| word.1).unwrap_or(at + 2_000.0);
        let end = (at + spoken_ms(text)).min(next.max(at + 1.0)).min(limit.unwrap_or(f64::INFINITY)).max(at + 1.0);
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
    fn manual_cues_get_distinct_bounded_word_times_and_keep_speaker_changes() {
        let value = json!({"events":[
            {"tStartMs":1000,"dDurationMs":1500,"segs":[{"utf8":">> We are ready."}]},
            {"tStartMs":4000,"dDurationMs":500,"segs":[{"utf8":">> Yes."}]}
        ]});
        let words = parse(&value).words;
        assert_eq!(words.iter().map(|word| word.text.as_str()).collect::<Vec<_>>(), ["We", "are", "ready.", "Yes."]);
        assert!(words[..3].windows(2).all(|pair| pair[0].start < pair[0].end && pair[0].end <= pair[1].start));
        assert!(words[2].end <= 2.5);
        assert_eq!(words[3].start, 4.0);
        assert_ne!(words[0].speaker, words[3].speaker);
    }

    #[test]
    fn english_selection_uses_variants_manual_cues_and_usable_earlier_files() {
        let dir = std::env::temp_dir().join(format!("writer-caption-selection-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let cue = |text: &str| json!({"events":[{"tStartMs":1000,"dDurationMs":1200,"segs":[{"utf8":text}]}]});
        let timed = json!({"events":[{"tStartMs":1000,"dDurationMs":1000,"segs":[{"utf8":"Original", "tOffsetMs":0},{"utf8":" English", "tOffsetMs":400}]}]});
        let put = |language: &str, value: &Value| fs::write(dir.join(format!("captions.{language}.json3")), value.to_string()).unwrap();
        put("en-US", &cue("Manual English"));
        put("fr", &timed);
        put("en-orig", &json!({"events":[]}));
        assert_eq!(select(&dir, None).unwrap().0, "en-US");
        let info = json!({"subtitles":{"en-US":[{}]}, "requested_subtitles":{"en":{"url":"https://example.test/captions?lang=fr&tlang=en"}}});
        put("en", &timed);
        put("en-Zd4rRTALbmM", &timed);
        assert_eq!(select(&dir, Some(&info)).unwrap().0, "en-US", "automatic translations are not the spoken transcript");
        put("en-orig", &timed);
        assert_eq!(select(&dir, Some(&info)).unwrap().0, "en-orig");
        fs::remove_file(dir.join("captions.en-orig.json3")).unwrap();
        fs::remove_file(dir.join("captions.en.json3")).unwrap();
        fs::write(dir.join("captions.en-GB.json3"), "broken").unwrap();
        assert_eq!(select(&dir, Some(&info)).unwrap().0, "en-US", "bad/failed later tracks don't discard usable captions");
        assert!(!english("eng"));
        assert!(english("en-GB"));
        assert!(english("en-CA"));
        assert!(!english("en-Zd4rRTALbmM"), "a translated track named with another video id is not this recording");
        assert!(!english("en-en-Zd4rRTALbmM"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_word_takes_about_sixty_ms_a_letter() {
        assert_eq!(spoken_ms("a"), 60.0);
        assert_eq!(spoken_ms("Objection."), 540.0);
        assert_eq!(spoken_ms("$8,320"), 1_000.0);
    }
}
