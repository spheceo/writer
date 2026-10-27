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
        // A compilation with one new case plus reuses still has its own full-
        // quality case file. Never register its 480p preview as the original.
        let file = if script.source_kind == "compilation" { crate::import::part_files(dir, script).into_iter().next().flatten().unwrap_or(footage) } else { footage };
        return vec![Heard { file, span: None, offset_ms: 0, length_ms: script.duration_ms, key: "whole".into() }];
    }
    let own = crate::import::part_files(dir, script);
    script
        .parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let end = (part.start_ms + part.duration_ms).min(script.duration_ms);
            let key = recording_key(dir, script, index, part.reuse.as_deref());
            match own.get(index).cloned().flatten() {
                Some(file) => Heard { file, span: None, offset_ms: part.start_ms, length_ms: end - part.start_ms, key },
                None => Heard { file: footage.clone(), span: Some((part.start_ms, end)), offset_ms: part.start_ms, length_ms: end - part.start_ms, key },
            }
        })
        .collect()
}

/// Reused cases are cached as `reuse-NN`, matching the import that brought
/// them in. An older completion stored that hearing as `part-NN`; keep using
/// it until a captions cache exists, so the paid transcript is not bought again.
fn recording_key(dir: &Path, script: &Script, index: usize, reuse: Option<&str>) -> String {
    let legacy = format!("part-{:02}", index + 1);
    let Some(slot) = reuse.and_then(|url| script.reuses.iter().position(|known| known.url == url)) else { return legacy };
    let reuse_key = format!("reuse-{slot:02}");
    let has = |key: &str| {
        let folder = dir.join("transcript").join(key);
        folder.join("captions.json").is_file() || folder.join("transcript.json").is_file()
    };
    if has(&reuse_key) || !has(&legacy) { reuse_key } else { legacy }
}

/// `on_piece` gets how far it has got, in thousandths of the whole.
pub fn transcribe(script: &mut Script, dir: &Path, settings: &Settings, on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<(), String> {
    let source = Path::new(&script.original_path);
    // The waveform and quiet cut points are a help, not a need.
    let peaks = if source.is_file() { media::peaks(source).unwrap_or_default() } else { Vec::new() };
    if !peaks.is_empty() {
        script.overview = media::overview(&peaks, 800);
        crate::tidy::keep_peaks(dir, &peaks);
    }
    if script.duration_ms == 0 {
        return Err("This footage has no length.".into());
    }
    // Caption reads and completed hearings need no new network call for names.
    let names = std::sync::OnceLock::new();
    let keyterms = || names.get_or_init(|| crate::names::case_names(dir, script, settings));
    let recordings = recordings(dir, script);
    let count = recordings.len();
    let links = links(script);
    let want_captions = crate::captions::wanted(script);
    let data = dir.parent().and_then(Path::parent).map(Path::to_path_buf);
    let cookies = data.as_deref().map(crate::youtube::cookies_file);
    let mut captioned = 0;
    let mut heard: Vec<Word> = Vec::new();
    for (index, recording) in recordings.iter().enumerate() {
        let progress = |done: usize, total: usize| on_piece((index * 1000 + done * 1000 / total.max(1)) / count, 1000);
        crate::jobs::check_cancelled()?;
        let work = dir.join("transcript").join(&recording.key);
        // A completion explicitly asks for the caption path even for an older
        // audio import with unfinished visual speaker checks. Its saved words
        // stay; caption turns supply the gaps and speaker labels. Ordinary
        // import retries still reuse a completed hearing first.
        let local = hearing::cached(&work).filter(|found| !found.words.is_empty());
        let (before_captions, after_captions) = if script.status == "ready" { (None, local) } else { (local, None) };
        let transcript = resolve(
            before_captions,
            || links.get(index).filter(|_| want_captions).and_then(Option::as_deref).and_then(|url| crate::captions::transcript(url, &work, cookies.as_deref())),
            || after_captions.or_else(|| data.as_deref().and_then(|data| crate::handoff::shared_transcript(data, dir, script, &recording.key, keyterms()))),
            || {
                let preview;
                let (file, span) = if recording.file.is_file() {
                    (recording.file.as_path(), recording.span)
                } else if let Some(data) = data.as_deref() {
                    // Only fetch footage if neither captions nor a shared hearing
                    // could supply the words. Synced YouTube projects need neither.
                    preview = crate::storage::fetch_preview(data, &script.id)?;
                    (preview.as_path(), Some((recording.offset_ms, recording.offset_ms + recording.length_ms)))
                } else {
                    return Err("The original video is missing, so it can't be transcribed.".into());
                };
                hearing::hear(&Recording { file, span }, &work, settings, keyterms(), &progress)
            },
        )?;
        if transcript.model == crate::captions::MODEL {
            captioned += 1;
            crate::log::info("hear", format!("{}: {} words from YouTube's captions", recording.key, transcript.words.len()));
        } else {
            let _ = std::fs::create_dir_all(&work);
            std::fs::write(work.join("transcript.json"), hearing::serialize(&transcript)).map_err(|error| error.to_string())?;
        }
        progress(1, 1);
        // A joined case's voices are its recordings' own: one recording's speaker_0 is not the next one's.
        let tag = (count > 1).then(|| format!("{}", index + 1));
        let recovered = on_clock(&transcript, recording.offset_ms, recording.length_ms, tag.as_deref());
        let saved: Vec<Word> = script.words.iter().filter(|word| word.start_ms >= recording.offset_ms && word.start_ms < recording.offset_ms + recording.length_ms).cloned().collect();
        heard.extend(fill_missing(&saved, recovered, &script.corrections));
    }
    if heard.is_empty() {
        return Err(NO_WORDS.into());
    }
    script.captions |= captioned > 0;
    // A recovery adds missing words without changing existing text, corrections,
    // word IDs (also used by reuse highlights), or the producer's timeline.
    if script.words.is_empty() {
        for (index, word) in heard.iter_mut().enumerate() { word.id = format!("w{index:05}"); }
    }
    script.words = heard;
    if script.clips.is_empty() {
        script.clips = vec![Clip { id: uuid::Uuid::new_v4().to_string(), start_ms: 0, end_ms: script.duration_ms, ..Default::default() }];
    }
    script.updated_at = now_ms();
    Ok(())
}

/// Source order is deliberately lazy: a completed local hearing, captions,
/// a shared hearing, then audio. Caption-only words never become an audio cache.
fn resolve(
    local: Option<hearing::Transcript>,
    captions: impl FnOnce() -> Option<hearing::Transcript>,
    shared: impl FnOnce() -> Option<hearing::Transcript>,
    audio: impl FnOnce() -> Result<hearing::Transcript, String>,
) -> Result<hearing::Transcript, String> {
    local.or_else(captions).or_else(shared).map(Ok).unwrap_or_else(audio)
}

/// Match saved words to their recovered counterparts before filling gaps.
/// Timing alone cannot tell a duplicate from a short missing word between
/// two saved ones. Matches are monotone and local in time, so repeated words
/// in another sentence cannot consume this sentence's missing speech.
fn fill_missing(saved: &[Word], recovered: Vec<Word>, corrections: &std::collections::BTreeMap<String, String>) -> Vec<Word> {
    const DRIFT: u64 = 1_500;
    let normalized: Vec<String> = recovered.iter().map(|word| hearing::normal(&word.text)).collect();
    let mut anchors: Vec<Option<std::ops::Range<usize>>> = vec![None; saved.len()];
    let mut taken = vec![false; recovered.len()];
    let tokens = |text: &str| text.split_whitespace().map(hearing::normal).filter(|word| !word.is_empty()).collect::<Vec<_>>();
    let (mut first, mut floor) = (0, 0);
    for (index, old) in saved.iter().enumerate() {
        while first < recovered.len() && recovered[first].end_ms.saturating_add(DRIFT) < old.start_ms { first += 1; }
        let mut variants = vec![tokens(&old.text)];
        if let Some(corrected) = corrections.get(&old.id) { variants.insert(0, tokens(corrected)); }
        let mut best: Option<(u64, std::ops::Range<usize>)> = None;
        for at in first.max(floor)..recovered.len() {
            if recovered[at].start_ms > old.end_ms.saturating_add(DRIFT) { break; }
            for wanted in variants.iter().filter(|words| !words.is_empty()) {
                let end = at + wanted.len();
                if normalized.get(at..end) == Some(wanted.as_slice()) && !taken[at..end].iter().any(|used| *used) {
                    let distance = old.start_ms.abs_diff(recovered[at].start_ms);
                    if best.as_ref().is_none_or(|(known, _)| distance < *known) { best = Some((distance, at..end)); }
                }
            }
        }
        if let Some((_, range)) = best {
            taken[range.clone()].fill(true);
            floor = range.end;
            anchors[index] = Some(range);
        }
    }
    // Unmatched saved spellings still stand. Pair each with at most one
    // overlapping recovered word between the same anchors, never all nearby
    // speech. This retains disagreements without swallowing adjacent gaps.
    let mut ceiling = vec![recovered.len(); saved.len()];
    let mut next = recovered.len();
    for index in (0..saved.len()).rev() {
        ceiling[index] = next;
        if let Some(range) = &anchors[index] { next = range.start; }
    }
    first = 0;
    floor = 0;
    for (index, old) in saved.iter().enumerate() {
        if let Some(range) = &anchors[index] { floor = range.end; continue; }
        while first < recovered.len() && recovered[first].end_ms.saturating_add(150) < old.start_ms { first += 1; }
        let best = (first.max(floor)..ceiling[index]).take_while(|&at| recovered[at].start_ms < old.end_ms.saturating_add(150))
            .filter(|&at| !taken[at] && old.start_ms < recovered[at].end_ms.saturating_add(150))
            .min_by_key(|&at| old.start_ms.abs_diff(recovered[at].start_ms));
        if let Some(at) = best { taken[at] = true; floor = at + 1; }
    }
    let mut out = saved.to_vec();
    for (mut word, used) in recovered.into_iter().zip(taken) {
        if !used { word.id = format!("w-{}", uuid::Uuid::new_v4()); out.push(word); }
    }
    out.sort_by_key(|word| (word.start_ms, word.end_ms));
    out
}

/// Explicit completion of an existing project, including one with only some
/// words. The job caches each recording and applies only transcript fields to
/// the current script; cuts, narration and corrections are kept as saved.
pub fn complete(app: &tauri::AppHandle, id: &str) -> Result<(String, f64), String> {
    let data = crate::data_dir(app)?;
    crate::can::require_id(&data, id)?;
    if crate::archive::state(&data, id).is_some() {
        return Err("Restore the footage before completing its transcription.".into());
    }
    let dir = crate::store::script_dir(&data, id)?;
    let mut script = crate::store::read(&dir)?;
    if script.status != "ready" || script.commentary.is_some() {
        return Err("Finish or retry the import before completing its transcription.".into());
    }
    if script.no_speech {
        return Err("This project was opened without speech. It has no transcription to complete.".into());
    }
    let revision = script.revision;
    let before_count = script.words.len();
    let settings = crate::settings::load(&data);
    transcribe(&mut script, &dir, &settings, &|done, total| {
        crate::jobs::stage(id, &format!("Completing transcription: {}%", done * 100 / total.max(1)));
    })?;
    crate::jobs::check_cancelled()?;
    if script.captions {
        crate::jobs::stage(id, "Reading who says each line");
        crate::import::read_turns(app, &dir, &mut script, &settings);
    } else if script.speakers.is_empty() {
        let _ = crate::speakers::identify(&mut script);
    }
    crate::jobs::check_cancelled()?;
    let mut fresh = crate::store::read(&dir)?;
    if fresh.revision != revision {
        return Err("The project changed during transcription. The transcripts are saved; complete transcription again to apply them.".into());
    }
    let before = fresh.clone();
    // Newly recovered speech needs exact timing before it is used in a cut,
    // even if this region had been checked before those words were available.
    let known: std::collections::HashSet<&str> = fresh.words.iter().map(|word| word.id.as_str()).collect();
    fresh.exact.retain(|[from, to]| !script.words.iter().any(|word| !known.contains(word.id.as_str()) && word.start_ms < *to && word.end_ms > *from));
    fresh.words = script.words;
    fresh.speakers = script.speakers;
    fresh.suspects = script.suspects;
    fresh.captions = script.captions;
    fresh.notes = script.notes;
    fresh.revision += 1;
    fresh.updated_at = now_ms();
    crate::store::write(&dir, &fresh)?;
    crate::versions::keep(&dir, "complete transcription", &before, &fresh);
    crate::store::library_changed();
    crate::handoff::share_soon(data, id.to_string());
    Ok((format!("Transcription completed: {} words added; {} words in total. Existing cuts, narration and spelling corrections kept.", fresh.words.len().saturating_sub(before_count), fresh.words.len()), 0.0))
}

/// Each recording's YouTube link, in the order of `recordings`.
fn links(script: &Script) -> Vec<Option<String>> {
    let youtube = |url: &str| crate::youtube::check_url(url).is_ok().then(|| url.to_string());
    if script.parts.len() < 2 {
        return vec![script.parts.first().and_then(|part| part.url.as_deref()).and_then(youtube).or_else(|| youtube(&script.source_url))];
    }
    script.parts.iter().map(|part| part.url.as_deref().and_then(youtube)).collect()
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
            let end_ms = (offset_ms + (word.end.max(0.0) * 1000.0).round() as u64).max(start_ms + 1).min(offset_ms + length_ms);
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

/// One recording already joined onto a compilation. Captions stay on that
/// recording's own clock until mapped onto `span` in the joined footage.
pub struct SpanSource<'a> {
    pub file: &'a Path,
    pub span: (u64, u64),
    pub url: Option<&'a str>,
    pub cookies: Option<&'a Path>,
}

pub fn transcribe_span(source: SpanSource<'_>, settings: &Settings, work: &Path, keyterms: &[String], tag: &str, on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<(Vec<Word>, bool), String> {
    crate::jobs::check_cancelled()?;
    if let Some(url) = source.url.filter(|url| crate::youtube::check_url(url).is_ok()) {
        if let Some(transcript) = crate::captions::transcript(url, work, source.cookies) {
            on_piece(1, 1);
            let length = source.span.1.saturating_sub(source.span.0).max(1);
            return Ok((on_clock(&transcript, source.span.0, length, Some(tag)), true));
        }
    }
    // Audio keeps the previous rule: a span that covers the whole file is
    // heard as the file, and a finished hearing is reused only when it
    // already listened for these names.
    let whole = media::streams(source.file).map(|found| found.duration_ms).unwrap_or(0);
    let span = (source.span.0 > 0 || source.span.1 < whole).then_some(source.span);
    let transcript = hearing::hear(&Recording { file: source.file, span }, work, settings, keyterms, on_piece)?;
    let (from, to) = span.unwrap_or((0, whole.max(1)));
    Ok((on_clock(&transcript, from, to.saturating_sub(from).max(1), Some(tag)), false))
}

/// Audio-only fallback for the clip finder, which already tried captions.
pub fn hear_span(source: &Path, settings: &Settings, work: &Path, span: (u64, u64), keyterms: &[String], tag: &str, on_piece: &(dyn Fn(usize, usize) + Sync)) -> Result<Vec<Word>, String> {
    transcribe_span(SpanSource { file: source, span, url: None, cookies: None }, settings, work, keyterms, tag, on_piece).map(|(words, _)| words)
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

    fn transcript(model: &str) -> hearing::Transcript {
        hearing::Transcript { model: model.into(), words: vec![hearing::Word { text: "Hello".into(), start: 0.5, end: 0.8, speaker: Some("voice".into()) }], ..Default::default() }
    }

    #[test]
    fn captions_run_before_shared_or_paid_audio_and_finished_hearings_are_reused() {
        let captioned = resolve(None, || Some(transcript(crate::captions::MODEL)), || panic!("must not request Edit's audio transcript"), || panic!("must not buy audio transcription")).unwrap();
        assert_eq!(captioned.model, crate::captions::MODEL);
        let local = resolve(Some(transcript("scribe_v2")), || panic!("already completed"), || panic!("already completed"), || panic!("already completed")).unwrap();
        assert_eq!(local.model, "scribe_v2");
        let shared = resolve(None, || None, || Some(transcript("scribe_v2")), || panic!("must reuse Edit's hearing")).unwrap();
        assert_eq!(shared.model, "scribe_v2");
        assert!(resolve(None, || None, || None, || Err("audio unavailable".into())).is_err());
    }

    #[test]
    fn recovery_fills_missing_sections_and_keeps_saved_words_ids_and_labels() {
        let saved = vec![
            Word { id: "keep-first".into(), text: "Original".into(), start_ms: 500, end_ms: 850, speaker: Some("named-voice".into()) },
            Word { id: "keep-last".into(), text: "End".into(), start_ms: 9_000, end_ms: 9_200, speaker: None },
        ];
        let recovered = vec![
            Word { id: String::new(), text: "Different".into(), start_ms: 600, end_ms: 900, speaker: None },
            Word { id: String::new(), text: "Missing".into(), start_ms: 3_000, end_ms: 3_300, speaker: Some("turn".into()) },
            Word { id: String::new(), text: "END!".into(), start_ms: 9_100, end_ms: 9_300, speaker: None },
        ];
        let filled = fill_missing(&saved, recovered.clone(), &Default::default());
        assert_eq!(filled.len(), 3);
        assert_eq!(serde_json::to_value(&filled[0]).unwrap(), serde_json::to_value(&saved[0]).unwrap());
        assert_eq!(serde_json::to_value(&filled[2]).unwrap(), serde_json::to_value(&saved[1]).unwrap());
        assert_eq!(filled[1].text, "Missing");
        assert!(filled[1].id.starts_with("w-"));
        // Repeating completion adds no copies and keeps every saved ID.
        assert_eq!(serde_json::to_value(fill_missing(&filled, recovered, &Default::default())).unwrap(), serde_json::to_value(&filled).unwrap());
    }

    #[test]
    fn short_gaps_repeated_words_and_corrected_phrases_are_merged_once() {
        let w = |id: &str, text: &str, start_ms, end_ms| Word { id: id.into(), text: text.into(), start_ms, end_ms, speaker: Some("kept".into()) };
        let saved = vec![w("we", "We", 0, 120), w("ready", "ready", 230, 450)];
        let recovered = vec![w("", "We", 0, 120), w("", "are", 130, 220), w("", "ready", 230, 450)];
        let filled = fill_missing(&saved, recovered.clone(), &Default::default());
        assert_eq!(filled.iter().map(|word| word.text.as_str()).collect::<Vec<_>>(), ["We", "are", "ready"]);
        assert_eq!((&filled[0].id, &filled[2].id), (&saved[0].id, &saved[1].id));
        assert_eq!(serde_json::to_value(fill_missing(&filled, recovered, &Default::default())).unwrap(), serde_json::to_value(&filled).unwrap());
        let repeated = fill_missing(&[w("no", "No", 0, 150)], vec![w("", "No", 50, 200), w("", "no", 500, 650)], &Default::default());
        assert_eq!(repeated.len(), 2);
        assert_eq!(repeated[0].id, "no");
        let corrections = std::collections::BTreeMap::from([("name".into(), "Jane Doe".into())]);
        let saved = vec![w("name", "Jadon", 0, 120), w("ready", "ready", 230, 450)];
        let recovered = vec![w("", "Jane", 0, 80), w("", "Doe", 90, 120), w("", "is", 130, 220), w("", "ready", 230, 450)];
        let filled = fill_missing(&saved, recovered, &corrections);
        assert_eq!(filled.iter().map(|word| word.text.as_str()).collect::<Vec<_>>(), ["Jadon", "is", "ready"]);
        assert_eq!(filled[0].id, "name");
        assert_eq!(corrections[&filled[0].id], "Jane Doe");
    }

    #[test]
    fn reused_youtube_recordings_take_captions_before_cached_audio_on_the_joined_clock() {
        let dir = std::env::temp_dir().join(format!("writer-reuse-captions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut captions = transcript(crate::captions::MODEL);
        captions.words.push(hearing::Word { text: "outside".into(), start: 2.0, end: 2.2, speaker: None });
        std::fs::write(dir.join("captions.json"), hearing::serialize(&captions)).unwrap();
        std::fs::write(dir.join("transcript.json"), hearing::serialize(&transcript("scribe_v2"))).unwrap();
        let source = || SpanSource { file: Path::new("/missing/joined.mp4"), span: (60_000, 61_000), url: Some("https://youtu.be/aaaaaaaaaaa"), cookies: None };
        let (words, captioned) = transcribe_span(source(), &Settings::default(), &dir, &[], "reuse1", &|_, _| {}).unwrap();
        assert!(captioned);
        assert_eq!(words.len(), 1);
        assert_eq!((words[0].start_ms, words[0].end_ms), (60_500, 60_800));
        assert_eq!(words[0].speaker.as_deref(), Some("reuse1:voice"));
        assert_eq!(hearing::cached(&dir).unwrap().model, "scribe_v2");
        // Drive/audio-only sources reuse their completed hearing, not a
        // caption file accidentally kept in that recording's folder.
        let (audio, captioned) = transcribe_span(SpanSource { url: None, ..source() }, &Settings::default(), &dir, &[], "reuse1", &|_, _| {}).unwrap();
        assert!(!captioned);
        assert_eq!(audio.len(), 1);
        let script: Script = serde_json::from_value(serde_json::json!({
            "id":"abc", "title":"Existing", "sourceKind":"compilation", "sourceUrl":"", "originalPath":"/missing/joined.mp4",
            "status":"ready", "stage":"ready", "progress":100, "durationMs":61000, "revision":2,"createdAt":0,"updatedAt":0,
            "reuses":[{"url":"https://docs.google.com/document/d/earlier/edit", "after":1,"footage":"https://youtu.be/aaaaaaaaaaa"}],
            "parts":[{"title":"First", "startMs":0,"durationMs":60000}, {"title":"Reused", "startMs":60000,"durationMs":1000,"reuse":"https://docs.google.com/document/d/earlier/edit", "url":"https://youtu.be/aaaaaaaaaaa"}]
        })).unwrap();
        assert_eq!(recordings(&dir, &script)[1].key, "reuse-00");
        let legacy = dir.join("transcript/part-02");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("transcript.json"), hearing::serialize(&transcript("scribe_v2"))).unwrap();
        assert_eq!(recordings(&dir, &script)[1].key, "part-02", "an older hearing kept under the part key is still found");
        let reuse_cache = dir.join("transcript/reuse-00");
        std::fs::create_dir_all(&reuse_cache).unwrap();
        std::fs::write(reuse_cache.join("captions.json"), hearing::serialize(&captions)).unwrap();
        assert_eq!(recordings(&dir, &script)[1].key, "reuse-00", "captions cached by the reused-case import win");
        assert_eq!(links(&script)[1].as_deref(), Some("https://youtu.be/aaaaaaaaaaa"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_existing_mixed_project_completes_from_caches_without_local_footage() {
        let dir = std::env::temp_dir().join(format!("writer-complete-{}", uuid::Uuid::new_v4()));
        let first = dir.join("transcript/part-01");
        let second = dir.join("transcript/part-02");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let mut captions = transcript(crate::captions::MODEL);
        captions.words.push(hearing::Word { text: "Recovered".into(), start: 3.0, end: 3.4, speaker: Some("t0002".into()) });
        std::fs::write(first.join("captions.json"), hearing::serialize(&captions)).unwrap();
        // Explicit completion switches older audio imports to captions too.
        std::fs::write(first.join("transcript.json"), hearing::serialize(&transcript("scribe_v2"))).unwrap();
        std::fs::write(second.join("transcript.json"), hearing::serialize(&transcript("scribe_v2"))).unwrap();
        let mut script: Script = serde_json::from_value(serde_json::json!({
            "id":"abc", "title":"Existing", "sourceKind":"compilation", "sourceUrl":"https://youtu.be/aaaaaaaaaaa", "originalPath":"/missing/footage.mp4",
            "status":"ready", "stage":"ready", "progress":100, "durationMs":12000, "revision":2, "createdAt":0, "updatedAt":0,
            "parts":[{"title":"YouTube", "startMs":0,"durationMs":6000,"url":"https://youtu.be/aaaaaaaaaaa"}, {"title":"Drive", "startMs":6000,"durationMs":6000,"url":"https://drive.google.com/file/d/1filefilefilefile/view"}],
            "words":[{"id":"existing", "text":"Hello", "startMs":500, "endMs":800,"speaker":"named"}],
            "corrections":{"existing":"Corrected"}, "overview":[0.5],
            "clips":[{"id":"keep-cut", "startMs":500,"endMs":800},{"id":"keep-narration", "kind":"cut-in", "startMs":0,"endMs":3000,"narration":"Keep this narration"}]
        })).unwrap();
        let before = serde_json::to_value(&script).unwrap();
        transcribe(&mut script, &dir, &Settings::default(), &|_, _| {}).unwrap();
        let after = serde_json::to_value(&script).unwrap();
        assert!(script.captions);
        assert_eq!(script.words.iter().map(|word| word.start_ms).collect::<Vec<_>>(), vec![500, 3000, 6500]);
        assert_eq!(script.words[0].id, "existing");
        assert_eq!(script.words[0].speaker.as_deref(), Some("named"));
        for field in ["clips", "corrections", "overview", "revision"] { assert_eq!(before[field], after[field], "{field}"); }
        let words = serde_json::to_value(&script.words).unwrap();
        // A corrupt prior hearing must not prevent using cached captions.
        std::fs::write(first.join("transcript.json"), "broken").unwrap();
        transcribe(&mut script, &dir, &Settings::default(), &|_, _| {}).unwrap();
        assert_eq!(words, serde_json::to_value(&script.words).unwrap());
        assert_eq!(crate::captions::cached(&first).unwrap().model, crate::captions::MODEL);
        // Starting with no saved words assigns distinct IDs across recordings.
        script.words.clear();
        transcribe(&mut script, &dir, &Settings::default(), &|_, _| {}).unwrap();
        assert_eq!(script.words.iter().map(|word| &word.id).collect::<std::collections::HashSet<_>>().len(), script.words.len());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Real caption fetch -> joined-clock reused source -> SDK speaker pass
    /// -> cached retry, entirely in /tmp and without downloading/hearing audio.
    /// cargo test --manifest-path src-tauri/Cargo.toml youtube_caption_recovery_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn youtube_caption_recovery_live() {
        let dir = std::env::temp_dir().join(format!("writer-caption-live-{}", uuid::Uuid::new_v4()));
        let work = dir.join("transcript/reuse-00");
        std::fs::create_dir_all(&work).unwrap();
        crate::tools::init(&dir);
        let (mut words, captioned) = transcribe_span(
            SpanSource { file: &dir.join("no-footage.mp4"), span: (60_000, 210_000), url: Some("https://www.youtube.com/watch?v=liWNx2Vpv5U"), cookies: None },
            &Settings::default(), &work, &[], "reuse1", &|_, _| {},
        ).expect("real YouTube captions");
        assert!(captioned && words.len() > 20);
        assert!(!work.join("transcript.json").exists(), "no audio hearing should be purchased");
        for word in &mut words { word.id = format!("w-{}", uuid::Uuid::new_v4()); }
        let mut script: Script = serde_json::from_value(serde_json::json!({
            "id":"abc", "title":"Local news caption recovery check", "sourceKind":"compilation", "sourceUrl":"", "originalPath":"",
            "status":"ready", "stage":"ready", "progress":100, "durationMs":210000, "revision":2,"createdAt":0,"updatedAt":0,
            "captions":true,
            "parts":[{"title":"Existing", "startMs":0,"durationMs":60000}, {"title":"YouTube news", "startMs":60000,"durationMs":150000,"reuse":"https://www.youtube.com/watch?v=liWNx2Vpv5U", "url":"https://www.youtube.com/watch?v=liWNx2Vpv5U"}],
            "words":[{"id":"keep-word","text":"Saved","startMs":0,"endMs":120}],
            "corrections":{"keep-word":"Saved correction"},
            "clips":[{"id":"keep-cut", "startMs":0,"endMs":1000},{"id":"keep-narration","kind":"cut-in","startMs":0,"endMs":3000,"narration":"Keep this narration."}]
        })).unwrap();
        script.words.extend(words);
        let before = serde_json::to_value(&script).unwrap();
        let ids = script.words.iter().map(|word| word.id.clone()).collect::<Vec<_>>();
        let report = crate::turns::run(&mut script, &dir, &[], "", &|done, total| println!("speaker pass: {done}/{total}")).expect("SDK speaker labeling");
        assert_eq!(report.failed, 0);
        assert!(report.people > 0);
        assert_eq!(ids, script.words.iter().map(|word| word.id.clone()).collect::<Vec<_>>());
        for field in ["clips", "corrections"] { assert_eq!(before[field], serde_json::to_value(&script).unwrap()[field]); }
        let retry = crate::turns::run(&mut script, &dir, &[], "", &|_, _| {}).unwrap();
        assert_eq!(retry.failed, 0);
        assert!(retry.cached >= retry.windows + 1, "cast and windows should be reused");
        println!("live captions: {} words, {} people, {} windows; retry reused {} answers; saved cuts/narration/corrections/IDs intact; cache {}", script.words.len(), report.people, report.windows, retry.cached, dir.display());
    }

    #[test]
    fn youtube_cases_use_captions_in_every_format_including_one_new_case_plus_reuse() {
        let mut script: Script = serde_json::from_value(serde_json::json!({
            "id":"a", "title":"Compilation", "sourceKind":"compilation", "sourceUrl":"https://youtu.be/aaaaaaaaaaa\nREUSE https://docs.google.com/document/d/earlier/edit",
            "originalPath":"", "status":"ready", "stage":"", "progress":100, "durationMs":1000, "revision":1, "createdAt":0, "updatedAt":0,
            "parts":[{"title":"New case", "startMs":0, "durationMs":1000, "url":"https://youtu.be/aaaaaaaaaaa"}]
        })).unwrap();
        for kind in [crate::formats::Kind::Interrogation, crate::formats::Kind::Foia, crate::formats::Kind::Courtroom] {
            script.kind = kind;
            assert!(crate::captions::wanted(&script));
            assert_eq!(links(&script), vec![Some("https://youtu.be/aaaaaaaaaaa".into())]);
        }
        script.parts.push(crate::model::Part { title: "Drive source".into(), start_ms: 1000, duration_ms: 1000, url: Some("https://drive.google.com/file/d/1filefilefilefile/view".into()), ..Default::default() });
        assert_eq!(links(&script), vec![Some("https://youtu.be/aaaaaaaaaaa".into()), None]);
    }

    #[test]
    fn caption_words_stop_at_their_recording_boundary_on_a_compilation_clock() {
        let transcript = hearing::Transcript { model: crate::captions::MODEL.into(), words: vec![hearing::Word { text: "Last".into(), start: 0.9, end: 1.2, speaker: Some("t0001".into()) }], ..Default::default() };
        let words = on_clock(&transcript, 5000, 1000, Some("2"));
        assert_eq!((words[0].start_ms, words[0].end_ms), (5900, 6000));
        assert_eq!(words[0].speaker.as_deref(), Some("2:t0001"));
    }

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
