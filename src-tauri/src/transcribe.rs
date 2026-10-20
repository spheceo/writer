use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::media;
use crate::model::{now_ms, Clip, Script, Word};
use crate::net::{self, Failed, Relay};
use crate::settings::Settings;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Piece {
    start_ms: u64,
    end_ms: u64,
    words: Vec<StoredWord>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredWord {
    text: String,
    start_ms: u64,
    end_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    speaker: Option<String>,
}

/// What `transcribe` says when the footage holds no speech at all.
pub const NO_WORDS: &str = "Speech to text heard no words. Nothing was invented.";

/// Hear the footage through the relay. A piece that already succeeded is not
/// sent again, so a retry does not pay ElevenLabs twice.
/// Pieces heard at once.
const PIECES_AT_ONCE: usize = 3;

/// `on_piece` gets how many pieces are done, of how many.
pub fn transcribe(script: &mut Script, dir: &Path, settings: &Settings, on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<(), String> {
    let source = Path::new(&script.original_path);
    if !source.is_file() {
        return Err("The original video is missing, so it can't be transcribed.".into());
    }
    // The waveform and quiet cut points are a help, not a need: without them
    // the footage is still heard, in pieces cut on the clock.
    let peaks = media::peaks(source).unwrap_or_default();
    script.overview = media::overview(&peaks, 800);
    if !peaks.is_empty() {
        crate::tidy::keep_peaks(dir, &peaks);
    }
    // A joined case is heard a recording at a time: no piece spans two, so
    // one recording's voices are never matched with another's.
    let pieces: Vec<(u64, u64)> = if script.parts.len() > 1 {
        script
            .parts
            .iter()
            .flat_map(|part| {
                let end = (part.start_ms + part.duration_ms).min(script.duration_ms);
                let found = media::plan_pieces_between(part.start_ms, end, &peaks);
                if found.is_empty() && end > part.start_ms { vec![(part.start_ms, end)] } else { found }
            })
            .collect()
    } else {
        media::plan_pieces(script.duration_ms, &peaks)
    };
    if pieces.is_empty() {
        return Err("This footage has no length.".into());
    }
    let transcript_dir = dir.join("transcript");
    let heard: Vec<Word> = hear_all(source, settings, &transcript_dir, &pieces, on_piece)?
        .into_iter()
        .enumerate()
        .flat_map(|(index, words)| {
            // Scribe names voices per piece: its speaker_0 in one piece is not
            // its speaker_0 in the next.
            let count = pieces.len();
            words.into_iter().map(move |mut word| {
                if count > 1 {
                    word.speaker = word.speaker.map(|speaker| format!("{}:{speaker}", index + 1));
                }
                word
            })
        })
        .collect();
    if heard.is_empty() {
        return Err(NO_WORDS.into());
    }
    script.words = heard
        .into_iter()
        .enumerate()
        .map(|(index, mut word)| {
            word.id = format!("w{index:05}");
            word
        })
        .collect();
    if script.clips.is_empty() {
        script.clips = vec![Clip {
            id: uuid::Uuid::new_v4().to_string(),
            start_ms: 0,
            end_ms: script.duration_ms,
            ..Default::default()
        }];
    }
    script.updated_at = now_ms();
    Ok(())
}

/// Every piece, three at once. After a failure no new piece starts; the
/// ones under way finish and are saved, so a retry has less to do.
fn hear_all(source: &Path, settings: &Settings, transcript_dir: &Path, pieces: &[(u64, u64)], on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<Vec<Word>>, String> {
    std::fs::create_dir_all(transcript_dir).map_err(|error| error.to_string())?;
    let count = pieces.len();
    let next = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<Vec<Word>, String>>>> = Mutex::new((0..count).map(|_| None).collect());
    on_piece(0, count);
    std::thread::scope(|scope| {
        for _ in 0..count.min(PIECES_AT_ONCE) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(&(start_ms, end_ms)) = pieces.get(index) else { break };
                let outcome = hear(source, settings, transcript_dir, index, count, start_ms, end_ms);
                let failed = outcome.is_err();
                if let Ok(mut slots) = results.lock() {
                    slots[index] = Some(outcome);
                }
                on_piece(finished.fetch_add(1, Ordering::Relaxed) + 1, count);
                if failed {
                    next.store(count, Ordering::Relaxed);
                    break;
                }
            });
        }
    });
    let mut heard = Vec::with_capacity(count);
    for slot in results.into_inner().unwrap_or_default() {
        match slot {
            Some(Ok(words)) => heard.push(words),
            Some(Err(error)) => return Err(error),
            None => heard.push(Vec::new()),
        }
    }
    Ok(heard)
}

/// One stretch of the footage heard alone (a reused case's video joined to
/// the end of a compilation), on the footage's clock. Its voices are its own,
/// marked with `tag`, never matched with the rest of the footage's. Its
/// pieces are kept in `transcript_dir`, so a retry never pays twice. No
/// speech at all is no words, not a failure.
pub fn hear_span(
    source: &Path,
    settings: &Settings,
    transcript_dir: &Path,
    span: (u64, u64),
    peaks: &[f32],
    tag: &str,
    on_piece: &(dyn Fn(usize, usize) + Sync),
) -> Result<Vec<Word>, String> {
    let (from, to) = span;
    let mut pieces = media::plan_pieces_between(from, to, peaks);
    if pieces.is_empty() && to > from {
        pieces = vec![(from, to)];
    }
    Ok(hear_all(source, settings, transcript_dir, &pieces, on_piece)?
        .into_iter()
        .enumerate()
        .flat_map(|(index, words)| {
            words.into_iter().map(move |mut word| {
                word.speaker = Some(format!("{tag}:{}:{}", index + 1, word.speaker.as_deref().unwrap_or("speaker")));
                word
            })
        })
        .collect())
}

/// One piece: from its saved answer, or heard now and saved before anything
/// else happens, so it is never paid for twice.
fn hear(source: &Path, settings: &Settings, transcript_dir: &Path, index: usize, count: usize, start_ms: u64, end_ms: u64) -> Result<Vec<Word>, String> {
    let saved = transcript_dir.join(format!("{index}.json"));
    if let Some(piece) = read_piece(&saved, start_ms, end_ms) {
        return Ok(piece);
    }
    let audio = transcript_dir.join(format!("{index}.ogg"));
    media::encode_opus(source, &audio, start_ms, end_ms)?;
    let result = send_with_retries(settings, &audio, index, count);
    let _ = std::fs::remove_file(&audio);
    let words = parse_scribe(&result?, start_ms)?;
    let piece = Piece {
        start_ms,
        end_ms,
        words: words
            .iter()
            .map(|word| StoredWord {
                text: word.text.clone(),
                start_ms: word.start_ms,
                end_ms: word.end_ms,
                speaker: word.speaker.clone(),
            })
            .collect(),
    };
    let tmp = transcript_dir.join(format!("{index}.json.tmp"));
    std::fs::write(&tmp, serde_json::to_vec(&piece).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    std::fs::rename(tmp, saved).map_err(|error| error.to_string())?;
    Ok(words)
}

fn read_piece(path: &Path, start_ms: u64, end_ms: u64) -> Option<Vec<Word>> {
    let bytes = std::fs::read(path).ok()?;
    let piece: Piece = serde_json::from_slice(&bytes).ok()?;
    if piece.start_ms != start_ms || piece.end_ms != end_ms {
        return None;
    }
    Some(
        piece
            .words
            .into_iter()
            .map(|word| Word {
                id: String::new(),
                text: word.text,
                start_ms: word.start_ms,
                end_ms: word.end_ms,
                speaker: word.speaker,
            })
            .collect(),
    )
}

/// Whether trying the same piece again could work.
enum Failure {
    Again(String),
    Stop(String),
}

/// Only an answer that cannot have been billed is sent again: no connection
/// at all, or the relay or ElevenLabs saying it is busy (after 30 s, then
/// 60 s, as Repack waits). Anything else stops, and the pieces already heard
/// stay saved, so retrying the import never pays for them twice.
fn send_with_retries(settings: &Settings, audio: &Path, index: usize, count: usize) -> Result<Value, String> {
    let waits = [30, 60];
    let mut attempt = 0;
    loop {
        match send(settings, audio) {
            Ok(value) => return Ok(value),
            Err(Failure::Again(reason)) if attempt < waits.len() => {
                std::thread::sleep(Duration::from_secs(waits[attempt]));
                attempt += 1;
                let _ = reason;
            }
            Err(Failure::Again(reason)) | Err(Failure::Stop(reason)) => {
                let which = if count > 1 { format!(" (part {} of {count})", index + 1) } else { String::new() };
                return Err(format!("{reason}{which} Parts already heard are kept; retry the import to carry on."));
            }
        }
    }
}

fn send(settings: &Settings, audio: &Path) -> Result<Value, Failure> {
    let relay = Relay::new(settings).map_err(Failure::Stop)?;
    match relay.transcribe(audio) {
        Ok(value) => Ok(value),
        Err(Failed::Status(code, body)) => Err(explain_relay(code, &net::said(&body))),
        // Nothing was sent if the relay could not be reached at all.
        Err(Failed::Unsent(reason)) => Err(Failure::Again(format!("Could not reach Writer's service ({reason})."))),
        Err(Failed::Network(reason)) => Err(Failure::Stop(format!("The connection to Writer's service broke ({reason})."))),
    }
}

/// What a relay answer means for the producer, and whether to try again.
fn explain_relay(code: u16, said: &str) -> Failure {
    let lower = said.to_lowercase();
    let detail = if said.is_empty() { String::new() } else { format!(" It said: {said}") };
    if lower.contains("quota") || lower.contains("credit") || code == 402 {
        return Failure::Stop(format!("ElevenLabs has no credit left for this.{detail}"));
    }
    match code {
        401 | 403 => Failure::Stop(format!("Writer's service turned this copy of Writer away. Get a fresh copy of Writer.{detail}")),
        413 => Failure::Stop(format!("This audio was too large to send in one piece.{detail}")),
        429 => Failure::Again(format!("Transcription is busy, or today's allowance is used up.{detail}")),
        502 | 503 | 504 | 529 => Failure::Again(format!("Transcription was unavailable ({code}).{detail}")),
        500..=599 => Failure::Stop(format!("Transcription failed ({code}).{detail}")),
        _ => Failure::Stop(format!("Transcription failed ({code}).{detail}")),
    }
}

fn parse_scribe(value: &Value, offset_ms: u64) -> Result<Vec<Word>, String> {
    let words = value["words"]
        .as_array()
        .ok_or("The transcription service returned no words.")?;
    let mut out = Vec::new();
    for word in words {
        if word["type"] != "word" {
            continue;
        }
        let Some(text) = word["text"].as_str().map(str::trim) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let Some(start) = word["start"].as_f64() else {
            continue;
        };
        let Some(end) = word["end"].as_f64() else {
            continue;
        };
        let start_ms = offset_ms + (start.max(0.0) * 1000.0).round() as u64;
        let end_ms = offset_ms + (end.max(0.0) * 1000.0).round() as u64;
        out.push(Word {
            id: String::new(),
            text: text.to_string(),
            start_ms,
            end_ms: end_ms.max(start_ms + 1),
            speaker: word["speaker_id"].as_str().map(str::to_string),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{explain_relay, parse_scribe, Failure};

    #[test]
    fn retries_passing_relay_failures_only() {
        assert!(matches!(explain_relay(503, ""), Failure::Again(_)));
        assert!(matches!(explain_relay(500, ""), Failure::Stop(_)));
        assert!(matches!(explain_relay(429, "daily cap"), Failure::Again(_)));
        assert!(matches!(explain_relay(401, ""), Failure::Stop(_)));
        assert!(matches!(explain_relay(400, "quota_exceeded"), Failure::Stop(text) if text.contains("no credit")));
    }
    use serde_json::json;

    #[test]
    fn keeps_spoken_words_and_offsets_the_piece() {
        let words = parse_scribe(
            &json!({
                "words": [
                    { "text": "Hello", "start": 0.1, "end": 0.4, "type": "word", "speaker_id": "speaker_0" },
                    { "text": " ", "start": 0.4, "end": 0.45, "type": "spacing" },
                    { "text": "(laughs)", "start": 0.5, "end": 0.9, "type": "audio_event" },
                    { "text": "there", "start": 0.9, "end": 1.2, "type": "word" }
                ]
            }),
            5_000,
        )
        .unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "Hello");
        assert_eq!(words[0].start_ms, 5_100);
        assert_eq!(words[0].speaker.as_deref(), Some("speaker_0"));
        assert_eq!(words[1].text, "there");
        assert!(words[1].speaker.is_none());
    }
}
