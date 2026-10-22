//! Distilling a reference video: Gemini watches it window by window and logs
//! every stretch as the narrator talking (and what's on screen meanwhile), a
//! clip playing with its own sound, or something else (a sponsor read, an
//! intro sequence). The timelines show how a channel paces narration against
//! clips. A tool for now, run by hand:
//! `DISTILL=/path/to/video.mp4 DISTILL_TITLE="…" cargo test distill -- --ignored --nocapture`
//! with Writer's data folder (for the relay) in `WRITER_DATA`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::net::Relay;
use crate::proc;

const WINDOW_MS: u64 = 240_000;

const SYSTEM: &str = r#"You log the structure of a YouTube commentary or documentary video for an editor studying its pacing. You get one window of the video (picture and sound). Split the whole window, start to end with no gaps, into consecutive stretches, and classify each:
- "narration": the video's own narrator (the channel's host, who carries the video) is speaking. Note what's on screen meanwhile.
- "clip": footage plays with its own sound audible and the narrator silent: someone else speaking, or an event's own audio. A clip is a moment the narrator stops for.
- "other": anything else: a sponsor or ad read, an intro or outro sequence, a title card with music, silence.
A new stretch starts whenever the kind changes. Merge back-to-back stretches of the same kind only when nothing about them changes; a new clip from a different source is a new stretch.
For each stretch give: start and end as m:ss from the start of THIS window; kind; on_screen (what the picture shows, in a few words: "muted stream footage of N3on in a car", "screenshot of a tweet", "news article", "narrator's face cam", "N3on's stream, his own audio"); who (for a clip: who speaks or what is heard; for narration: "narrator"); words (the first few words said in the stretch, as heard); purpose (for a clip: what it shows or proves, in one short line; for narration: what the narration is doing, such as "sets up the next clip", "explains background", "lands the chapter's point"); lead_in (for a clip: the narrator's last words right before it, if they're in this window).
Also list any chapter heading or title card shown, with its time.
Times must be accurate to the second. Cover the whole window."#;

fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["stretches", "headings"],
        "properties": {
            "stretches": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["start", "end", "kind", "on_screen", "who", "words", "purpose", "lead_in"],
                    "properties": {
                        "start": { "type": "string" },
                        "end": { "type": "string" },
                        "kind": { "type": "string", "enum": ["narration", "clip", "other"] },
                        "on_screen": { "type": "string" },
                        "who": { "type": "string" },
                        "words": { "type": "string" },
                        "purpose": { "type": "string" },
                        "lead_in": { "type": "string" }
                    }
                }
            },
            "headings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["at", "text"],
                    "properties": { "at": { "type": "string" }, "text": { "type": "string" } }
                }
            }
        }
    })
}

fn duration_ms(video: &Path) -> u64 {
    proc::run("ffprobe", &["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", &video.to_string_lossy()], Duration::from_secs(60))
        .ok()
        .and_then(|text| text.trim().parse::<f64>().ok())
        .map(|seconds| (seconds * 1000.0) as u64)
        .unwrap_or(0)
}

fn cut(video: &Path, start_ms: u64, end_ms: u64, out: &Path) -> Result<Vec<u8>, String> {
    let (start, length) = (format!("{:.3}", start_ms as f64 / 1000.0), format!("{:.3}", (end_ms - start_ms) as f64 / 1000.0));
    proc::run(
        "ffmpeg",
        &[
            "-hide_banner", "-loglevel", "error", "-y", "-ss", &start, "-t", &length, "-i", &video.to_string_lossy(),
            "-map", "0:v:0", "-map", "0:a:0?", "-vf", "scale=-2:360,fps=3",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "30", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "48k", "-ac", "1", "-ar", "48000", "-movflags", "+faststart", &out.to_string_lossy(),
        ],
        Duration::from_secs(300),
    )?;
    let bytes = std::fs::read(out).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_file(out);
    Ok(bytes)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn unfence(text: &str) -> &str {
    let text = text.trim();
    let text = text.strip_prefix("```json").or_else(|| text.strip_prefix("```")).unwrap_or(text);
    text.strip_suffix("```").unwrap_or(text).trim()
}

/// One window watched, its answer kept in `out` so a rerun doesn't pay twice.
fn window(relay: &Relay, video: &Path, title: &str, out: &Path, index: usize, start_ms: u64, end_ms: u64) -> Result<(Value, f64), String> {
    let kept = out.join(format!("{index:02}.json"));
    if let Some(saved) = std::fs::read(&kept).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) {
        return Ok((saved, 0.0));
    }
    let clip = cut(video, start_ms, end_ms, &out.join(format!("{index:02}.mp4")))?;
    let text = format!(
        "Video: \"{title}\". This window runs from {} to {} of the video ({} long). Give every time from the start of this window.",
        crate::commentary::clock(start_ms),
        crate::commentary::clock(end_ms),
        crate::commentary::clock(end_ms - start_ms)
    );
    let encoded = base64(&clip);
    let mut last = String::new();
    for attempt in 1..=3 {
        match relay.watch(SYSTEM, &text, &encoded, &schema()) {
            Ok(reply) => {
                let cost = reply.pointer("/usage/cost").and_then(Value::as_f64).unwrap_or(0.0);
                let content = reply.pointer("/choices/0/message/content").and_then(Value::as_str).unwrap_or("");
                match serde_json::from_str::<Value>(unfence(content)) {
                    Ok(answer) => {
                        let saved = json!({ "index": index, "startMs": start_ms, "endMs": end_ms, "cost": cost, "answer": answer });
                        std::fs::write(&kept, serde_json::to_vec_pretty(&saved).unwrap_or_default()).map_err(|error| error.to_string())?;
                        return Ok((saved, cost));
                    }
                    Err(error) => last = format!("unreadable answer ({error})"),
                }
            }
            Err(failed) => {
                last = match failed {
                    crate::net::Failed::Status(code, body) => format!("{code}: {}", crate::net::said(&body)),
                    crate::net::Failed::Unsent(reason) | crate::net::Failed::Network(reason) => reason,
                }
            }
        }
        std::thread::sleep(Duration::from_secs(10 * attempt));
    }
    Err(last)
}

#[test]
#[ignore]
fn distill() {
    let video = PathBuf::from(std::env::var("DISTILL").expect("DISTILL=/path/to/video.mp4"));
    let title = std::env::var("DISTILL_TITLE").unwrap_or_else(|_| "Reference video".into());
    let data = PathBuf::from(std::env::var("WRITER_DATA").expect("WRITER_DATA=Writer's data folder"));
    let relay = Relay::new(&crate::settings::load(&data)).expect("the relay");
    let out = video.with_extension("distill");
    std::fs::create_dir_all(&out).unwrap();
    let total = duration_ms(&video);
    let windows: Vec<(usize, u64, u64)> = (0..total.div_ceil(WINDOW_MS)).map(|i| (i as usize, i * WINDOW_MS, ((i + 1) * WINDOW_MS).min(total))).collect();
    let next = AtomicUsize::new(0);
    let cost = Mutex::new(0.0);
    let failed = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                let at = next.fetch_add(1, Ordering::SeqCst);
                let Some(&(index, start, end)) = windows.get(at) else { break };
                match window(&relay, &video, &title, &out, index, start, end) {
                    Ok((_, spent)) => {
                        *cost.lock().unwrap() += spent;
                        println!("window {index} done (${spent:.4})");
                    }
                    Err(error) => {
                        println!("window {index} failed: {error}");
                        failed.lock().unwrap().push(index);
                    }
                }
            });
        }
    });
    println!("{}: {} windows, ${:.4}, failed {:?}", title, windows.len(), cost.lock().unwrap(), failed.lock().unwrap());
}
