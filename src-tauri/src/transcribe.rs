//! A script's words: each recording heard as Edit hears it (hearing.rs),
//! listening for the case's names (names.rs), then put on the script's
//! clock. A joined case is heard recording by recording, from each one's own
//! file where it's kept (else its stretch of the joined footage), so no
//! piece spans two and each recording's transcript is the one Edit reads.
//! Each recording's hearing is kept under `transcript/<recording>/`, so a
//! retry never pays twice.

use std::path::{Path, PathBuf};

use crate::hearing::{self, Recording};
use crate::media;
use crate::model::{now_ms, Clip, Script, Word};
use crate::settings::Settings;

/// What `transcribe` says when the footage holds no speech at all.
pub const NO_WORDS: &str = "Speech to text heard no words. Nothing was invented.";

/// One recording of a script: its own file (or a stretch of the joined
/// footage), where it starts on the script's clock, how much of it plays,
/// and where its hearing is kept.
pub struct Heard {
    pub file: PathBuf,
    pub span: Option<(u64, u64)>,
    pub offset_ms: u64,
    pub length_ms: u64,
    pub key: String,
}

/// The script's recordings, in play order.
pub fn recordings(dir: &Path, script: &Script) -> Vec<Heard> {
    let footage = PathBuf::from(&script.original_path);
    if script.parts.len() < 2 {
        return vec![Heard { file: footage, span: None, offset_ms: 0, length_ms: script.duration_ms, key: "whole".into() }];
    }
    let own = crate::import::part_files(dir, script);
    script
        .parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let end = (part.start_ms + part.duration_ms).min(script.duration_ms);
            match own.get(index).cloned().flatten() {
                Some(file) => Heard { file, span: None, offset_ms: part.start_ms, length_ms: end - part.start_ms, key: format!("part-{:02}", index + 1) },
                None => Heard { file: footage.clone(), span: Some((part.start_ms, end)), offset_ms: part.start_ms, length_ms: end - part.start_ms, key: format!("part-{:02}", index + 1) },
            }
        })
        .collect()
}

/// `on_piece` gets how far it has got, in thousandths of the whole.
pub fn transcribe(script: &mut Script, dir: &Path, settings: &Settings, on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<(), String> {
    let source = Path::new(&script.original_path);
    if !source.is_file() {
        return Err("The original video is missing, so it can't be transcribed.".into());
    }
    // The waveform and quiet cut points are a help, not a need.
    let peaks = media::peaks(source).unwrap_or_default();
    script.overview = media::overview(&peaks, 800);
    if !peaks.is_empty() {
        crate::tidy::keep_peaks(dir, &peaks);
    }
    if script.duration_ms == 0 {
        return Err("This footage has no length.".into());
    }
    let keyterms = crate::names::case_names(dir, script, settings);
    let recordings = recordings(dir, script);
    let count = recordings.len();
    let mut heard: Vec<Word> = Vec::new();
    for (index, recording) in recordings.iter().enumerate() {
        let progress = |done: usize, total: usize| on_piece((index * 1000 + done * 1000 / total.max(1)) / count, 1000);
        // Heard once for both apps: Edit's transcript of this recording, when it has one.
        let work = dir.join("transcript").join(&recording.key);
        if let Some(data) = dir.parent().and_then(Path::parent) {
            if let Some(found) = crate::handoff::shared_transcript(data, dir, script, &recording.key, &keyterms) {
                let _ = std::fs::create_dir_all(&work);
                let _ = std::fs::write(work.join("transcript.json"), hearing::serialize(&found));
            }
        }
        let transcript = hearing::hear(&Recording { file: &recording.file, span: recording.span }, &dir.join("transcript").join(&recording.key), settings, &keyterms, &progress)?;
        // A joined case's voices are its recordings' own: one recording's speaker_0 is not the next one's.
        let tag = (count > 1).then(|| format!("{}", index + 1));
        heard.extend(on_clock(&transcript, recording.offset_ms, recording.length_ms, tag.as_deref()));
    }
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
        script.clips = vec![Clip { id: uuid::Uuid::new_v4().to_string(), start_ms: 0, end_ms: script.duration_ms, ..Default::default() }];
    }
    script.updated_at = now_ms();
    Ok(())
}

/// A recording's words on the script's clock: from `offset_ms`, only what
/// plays (`length_ms`), voices marked with `tag` when there is one.
fn on_clock(transcript: &hearing::Transcript, offset_ms: u64, length_ms: u64, tag: Option<&str>) -> Vec<Word> {
    transcript
        .words
        .iter()
        .filter(|word| ((word.start * 1000.0).round() as u64) < length_ms)
        .map(|word| {
            let start_ms = offset_ms + (word.start.max(0.0) * 1000.0).round() as u64;
            let end_ms = (offset_ms + (word.end.max(0.0) * 1000.0).round() as u64).max(start_ms + 1);
            Word {
                id: String::new(),
                text: word.text.clone(),
                start_ms,
                end_ms,
                speaker: word.speaker.as_ref().map(|speaker| match tag {
                    Some(tag) => format!("{tag}:{speaker}"),
                    None => speaker.clone(),
                }),
            }
        })
        .collect()
}

/// One stretch of the footage heard alone (a reused case's video joined to
/// the end of a compilation, or a video with no captions), on the footage's
/// clock. Its voices are its own, marked with `tag`. Kept in `work`, so a
/// retry never pays twice. No speech at all is no words, not a failure.
pub fn hear_span(source: &Path, settings: &Settings, work: &Path, span: (u64, u64), keyterms: &[String], tag: &str, on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<Word>, String> {
    let whole = media::streams(source).map(|found| found.duration_ms).unwrap_or(0);
    let span = (span.0 > 0 || span.1 < whole).then_some(span);
    let transcript = hearing::hear(&Recording { file: source, span }, work, settings, keyterms, on_piece)?;
    let (from, to) = span.unwrap_or((0, whole.max(1)));
    Ok(on_clock(&transcript, from, to.saturating_sub(from).max(1), Some(tag)))
}

/// What a relay refusal means for the producer.
pub fn explain(code: u16, said: &str) -> String {
    let lower = said.to_lowercase();
    let detail = if said.is_empty() { String::new() } else { format!(" It said: {said}") };
    if lower.contains("quota") || lower.contains("credit") || code == 402 {
        return format!("ElevenLabs has no credit left for this.{detail}");
    }
    match code {
        401 | 403 => crate::account::turned_away(code, said),
        413 => format!("This audio was too large to send in one piece.{detail}"),
        429 => format!("Today's transcription allowance is used up. It resets at midnight UTC; what was heard is kept.{detail}"),
        500..=599 => format!("Transcription failed ({code}). Retry: what was heard is kept.{detail}"),
        _ => format!("Transcription failed ({code}).{detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puts_a_recordings_words_on_the_scripts_clock() {
        let transcript = hearing::Transcript {
            model: "scribe_v2".into(),
            words: vec![
                hearing::Word { text: "Hello".into(), start: 0.1, end: 0.4, speaker: Some("speaker_0".into()) },
                hearing::Word { text: "late".into(), start: 9.5, end: 9.8, speaker: None },
            ],
            ..Default::default()
        };
        let words = on_clock(&transcript, 5_000, 9_000, Some("2"));
        assert_eq!(words.len(), 1);
        assert_eq!((words[0].start_ms, words[0].end_ms), (5_100, 5_400));
        assert_eq!(words[0].speaker.as_deref(), Some("2:speaker_0"));
    }

    #[test]
    fn says_what_a_refusal_means() {
        assert!(explain(400, "quota_exceeded").contains("no credit"));
        assert!(explain(401, "").to_lowercase().contains("sign in"));
    }
}
