use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::proc;

/// A joined case may run as long as Repack lets one run.
pub const MAX_CASE_MS: u64 = 20 * 3_600_000;

/// What a file holds: its length and whether it has picture and sound.
pub struct Streams {
    pub duration_ms: u64,
    pub video: bool,
    pub audio: bool,
}

pub fn streams(path: &Path) -> Result<Streams, String> {
    let text = proc::run(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
            &path.to_string_lossy(),
        ],
        Duration::from_secs(60),
    )?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "Could not read this file.".to_string())?;
    let duration = value["format"]["duration"]
        .as_f64()
        .or_else(|| {
            value["format"]["duration"]
                .as_str()
                .and_then(|text| text.parse::<f64>().ok())
        })
        .unwrap_or(0.0);
    let duration_ms = if duration.is_finite() && duration > 0.0 {
        (duration * 1000.0).round() as u64
    } else {
        0
    };
    let has = |kind: &str| {
        value["streams"]
            .as_array()
            .map(|items| items.iter().any(|s| s["codec_type"] == kind))
            .unwrap_or(false)
    };
    Ok(Streams { duration_ms, video: has("video"), audio: has("audio") })
}

/// How much of a file actually plays, from its last packet. An interrupted
/// upload keeps the header of the whole recording, so the length it claims
/// can be hours longer than what it holds.
pub fn held_ms(path: &Path, claimed_ms: u64) -> Option<u64> {
    // One stream at a time: ffprobe takes a single specifier ("a:0,v:0" is
    // refused), so the sound's and the picture's last packets are read apart.
    let last_of = |stream: &str| -> Option<f64> {
        let mut last = 0f64;
        proc::run_lines(
            "ffprobe",
            &["-v", "error", "-select_streams", stream, "-show_entries", "packet=pts_time,duration_time", "-of", "csv=p=0", &path.to_string_lossy()],
            tool_time(claimed_ms),
            None,
            |line| {
                let mut fields = line.split(',');
                let pts = fields.next().and_then(|text| text.trim().parse::<f64>().ok()).unwrap_or(0.0);
                let duration = fields.next().and_then(|text| text.trim().parse::<f64>().ok()).unwrap_or(0.0);
                if pts.is_finite() && duration.is_finite() {
                    last = last.max(pts + duration);
                }
            },
        )
        .ok()?;
        (last > 0.0).then_some(last)
    };
    let last = last_of("a:0").into_iter().chain(last_of("v:0")).fold(0f64, f64::max);
    (last > 0.0).then(|| (last * 1000.0).round() as u64)
}

/// The length to use: what the file holds when it is cut short by more than
/// two seconds, otherwise what it claims.
pub fn playable_ms(path: &Path, claimed_ms: u64) -> u64 {
    match held_ms(path, claimed_ms) {
        Some(held) if held > 0 && held + 2_000 < claimed_ms => held,
        _ => claimed_ms,
    }
}

/// Time for ffmpeg to work through footage: an hour, or ten minutes plus half
/// its length, whichever is longer.
pub fn tool_time(duration_ms: u64) -> Duration {
    Duration::from_secs((600 + duration_ms / 2_000).max(3_600))
}

/// One recording of a case to be joined.
pub struct JoinPart<'a> {
    pub path: &'a Path,
    pub streams: Streams,
}

/// Joins a case's recordings, in order, into one 480p preview with sound,
/// which is also the footage Writer transcribes. Recordings of any size are
/// fitted to 854×480; an audio recording plays over black, and a silent video
/// gets silence, so picture and sound stay in step at every join.
pub fn join(parts: &[JoinPart], dest: &Path, on_percent: &dyn Fn(u8)) -> Result<(), String> {
    join_sized(parts, dest, on_percent, 854, 480, "27")
}

/// One compilation case may be a folder. Keep a full-size case master for
/// Edit, separately from the 480p preview, and retain all downloaded originals.
pub fn join_original(parts: &[JoinPart], dest: &Path, on_percent: &dyn Fn(u8)) -> Result<(), String> {
    let mut width = 854;
    let mut height = 480;
    for part in parts.iter().filter(|part| part.streams.video) {
        let text = proc::run("ffprobe", &["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "json", &part.path.to_string_lossy()], Duration::from_secs(30))?;
        let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        width = width.max(value["streams"][0]["width"].as_u64().unwrap_or(854));
        height = height.max(value["streams"][0]["height"].as_u64().unwrap_or(480));
    }
    join_sized(parts, dest, on_percent, width.div_ceil(2) * 2, height.div_ceil(2) * 2, "18")
}

fn join_sized(parts: &[JoinPart], dest: &Path, on_percent: &dyn Fn(u8), width: u64, height: u64, crf: &str) -> Result<(), String> {
    let total: u64 = parts.iter().map(|part| part.streams.duration_ms).sum();
    if total == 0 || total > MAX_CASE_MS {
        return Err("Use footage between a second and twenty hours in all.".into());
    }
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-y".into()];
    let mut filters = Vec::new();
    let mut joined = String::new();
    for (index, part) in parts.iter().enumerate() {
        args.push("-i".into());
        args.push(part.path.to_string_lossy().into_owned());
        let seconds = format!("{:.3}", part.streams.duration_ms as f64 / 1000.0);
        if part.streams.video {
            filters.push(format!(
                "[{index}:v]scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,format=yuv420p,trim=duration={seconds},setpts=PTS-STARTPTS[v{index}]"
            ));
        } else {
            filters.push(format!("color=c=black:s={width}x{height}:r=30:d={seconds},format=yuv420p,setsar=1[v{index}]"));
        }
        if part.streams.audio {
            filters.push(format!(
                "[{index}:a]aresample=48000,aformat=channel_layouts=stereo,apad,atrim=duration={seconds},asetpts=PTS-STARTPTS[a{index}]"
            ));
        } else {
            filters.push(format!("anullsrc=r=48000:cl=stereo,atrim=duration={seconds}[a{index}]"));
        }
        joined.push_str(&format!("[v{index}][a{index}]"));
    }
    filters.push(format!("{joined}concat=n={}:v=1:a=1[v][a]", parts.len()));
    let dest_text = dest.to_string_lossy().into_owned();
    args.extend(
        [
            "-filter_complex", &filters.join(";"), "-map", "[v]", "-map", "[a]",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", crf, "-g", "30",
            "-c:a", "aac", "-b:a", "96k", "-movflags", "+faststart",
            "-progress", "pipe:1", "-nostats", &dest_text,
        ]
        .map(str::to_string),
    );
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    proc::run_lines("ffmpeg", &args, tool_time(total), None, |line| {
        let micros = line
            .strip_prefix("out_time_us=")
            .or_else(|| line.strip_prefix("out_time_ms="));
        if let Some(micros) = micros.and_then(|value| value.trim().parse::<u64>().ok()) {
            on_percent(((micros / 1000) * 100 / total).min(100) as u8);
        }
    })?;
    if !dest.is_file() {
        return Err("The joined footage was not written.".into());
    }
    Ok(())
}

/// Adds recordings to the end of a joined preview, each first encoded alone by
/// `join`, so they share its codecs, size and rate: the streams are copied
/// end to end and what is already joined is never encoded again. Pieces that
/// don't match, or a copy that comes out the wrong length, are joined afresh
/// instead. Says whether the streams were copied.
pub fn append(base: &Path, more: &[&Path], dest: &Path, on_percent: &dyn Fn(u8)) -> Result<bool, String> {
    let pieces: Vec<(&Path, Streams)> = std::iter::once(base)
        .chain(more.iter().copied())
        .map(|path| streams(path).map(|found| (path, found)))
        .collect::<Result<_, _>>()?;
    let total: u64 = pieces.iter().map(|(_, found)| found.duration_ms).sum();
    if total > MAX_CASE_MS {
        return Err("Use footage between a second and twenty hours in all.".into());
    }
    // Copied end to end only when every piece's streams match the first's.
    let first = format_of(base);
    if first.is_none() || more.iter().any(|path| format_of(path) != first) {
        let parts: Vec<JoinPart> = pieces.into_iter().map(|(path, streams)| JoinPart { path, streams }).collect();
        return join(&parts, dest, on_percent).map(|_| false);
    }
    let list = dest.with_extension("txt");
    let quote = |path: &Path| format!("file '{}'\n", path.to_string_lossy().replace('\'', "'\\''"));
    std::fs::write(&list, pieces.iter().map(|(path, _)| quote(path)).collect::<String>()).map_err(|error| error.to_string())?;
    let (list_text, dest_text) = (list.to_string_lossy().into_owned(), dest.to_string_lossy().into_owned());
    let copied = proc::run(
        "ffmpeg",
        &["-hide_banner", "-y", "-f", "concat", "-safe", "0", "-i", &list_text, "-c", "copy", "-movflags", "+faststart", &dest_text],
        tool_time(total),
    );
    let _ = std::fs::remove_file(&list);
    // Each join may drift by a frame; more than that and the copy went wrong.
    let slack = 500 * pieces.len() as u64;
    if copied.is_ok() && streams(dest).is_ok_and(|found| found.duration_ms.abs_diff(total) <= slack) {
        on_percent(100);
        return Ok(true);
    }
    let parts: Vec<JoinPart> = pieces.into_iter().map(|(path, streams)| JoinPart { path, streams }).collect();
    join(&parts, dest, on_percent).map(|_| false)
}

/// A file's streams as a copy must match them: codecs, picture size, frame
/// rate and pixel format, sound rate and channels. None when unreadable.
fn format_of(path: &Path) -> Option<String> {
    proc::run(
        "ffprobe",
        &[
            "-v", "error",
            "-show_entries", "stream=codec_type,codec_name,profile,width,height,pix_fmt,r_frame_rate,sample_rate,channels",
            "-of", "csv=p=0",
            &path.to_string_lossy(),
        ],
        Duration::from_secs(60),
    )
    .ok()
    .filter(|text| !text.trim().is_empty())
}

/// 480p preview with sound, so the window can play the cut.
/// `on_percent` is 0–100 of the encode. ffmpeg's progress clock is microseconds.
pub fn encode_proxy(
    source: &Path,
    dest: &Path,
    duration_ms: u64,
    on_percent: &dyn Fn(u8),
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let dest_text = dest.to_string_lossy().to_string();
    let source_text = source.to_string_lossy().to_string();
    proc::run_lines(
        "ffmpeg",
        &[
            "-hide_banner",
            "-y",
            "-i",
            &source_text,
            "-vf",
            "scale=-2:480,fps=30",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "27",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "30",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
            &dest_text,
        ],
        tool_time(duration_ms),
        None,
        |line| {
            let micros = line
                .strip_prefix("out_time_us=")
                .or_else(|| line.strip_prefix("out_time_ms="));
            if let Some(micros) = micros.and_then(|value| value.trim().parse::<u64>().ok()) {
                if duration_ms > 0 {
                    on_percent(((micros / 1000) * 100 / duration_ms).min(100) as u8);
                }
            }
        },
    )?;
    if !dest.is_file() {
        return Err("The preview was not written.".into());
    }
    Ok(())
}

/// Loudest sample of each 20 ms, from 0 to 1. Used to cut long transcripts
/// on a quiet moment, and to draw the overview.
pub fn peaks(source: &Path) -> Result<Vec<f32>, String> {
    let source_text = source.to_string_lossy().to_string();
    let mut child = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            &source_text,
            "-vn",
            "-ac",
            "1",
            "-ar",
            "24000",
            "-f",
            "s16le",
            "pipe:1",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("ffmpeg failed to start ({error})."))?;
    let mut stdout = child.stdout.take().ok_or("ffmpeg stdout")?;
    let mut peaks = Vec::new();
    let mut window = [0u8; 480 * 2];
    let mut filled = 0usize;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = stdout.read(&mut buf).map_err(|error| error.to_string())?;
        if n == 0 {
            break;
        }
        let mut offset = 0;
        while offset < n {
            let take = (window.len() - filled).min(n - offset);
            window[filled..filled + take].copy_from_slice(&buf[offset..offset + take]);
            filled += take;
            offset += take;
            if filled == window.len() {
                peaks.push(window_peak(&window));
                filled = 0;
            }
        }
    }
    if filled >= 2 {
        peaks.push(window_peak(&window[..filled - (filled % 2)]));
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if !status.success() {
        return Err("Could not read the audio.".into());
    }
    Ok(peaks)
}

fn window_peak(bytes: &[u8]) -> f32 {
    let mut loud = 0i16;
    for sample in bytes.chunks_exact(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs();
        if value > loud.unsigned_abs() {
            loud = value as i16;
        }
    }
    loud.unsigned_abs() as f32 / 32768.0
}

pub fn overview(peaks: &[f32], buckets: usize) -> Vec<f32> {
    if peaks.is_empty() || buckets == 0 {
        return Vec::new();
    }
    let mut out = vec![0f32; buckets];
    for (index, peak) in peaks.iter().enumerate() {
        let bucket = (index * buckets / peaks.len()).min(buckets - 1);
        if *peak > out[bucket] {
            out[bucket] = *peak;
        }
    }
    out
}

#[cfg(test)]
mod join_tests {
    use super::{append, join, streams, JoinPart};
    use std::time::Duration;
    use std::path::Path;
    use std::process::Command;

    fn make(args: &[&str], out: &Path) {
        let status = Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).arg(out).status().unwrap();
        assert!(status.success());
    }

    /// Needs ffmpeg: cargo test -- --ignored joins_mixed_recordings
    #[test]
    #[ignore]
    fn joins_mixed_recordings() {
        let dir = std::env::temp_dir().join("writer-join-test");
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a.mp4"), dir.join("b.mp3"), dir.join("c.mov"));
        make(&["-f", "lavfi", "-i", "testsrc=s=640x360:d=3", "-f", "lavfi", "-i", "sine=d=3", "-shortest"], &a);
        make(&["-f", "lavfi", "-i", "sine=f=330:d=2"], &b);
        make(&["-f", "lavfi", "-i", "testsrc=s=1280x720:d=2"], &c);
        let parts: Vec<JoinPart> = [&a, &b, &c]
            .iter()
            .map(|path| JoinPart { path: path.as_path(), streams: streams(path).unwrap() })
            .collect();
        let out = dir.join("joined.mp4");
        join(&parts, &out, &|_| {}).unwrap();
        let found = streams(&out).unwrap();
        assert!(found.video && found.audio);
        assert!((6_900..=7_200).contains(&found.duration_ms), "{}", found.duration_ms);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Needs ffmpeg; exercises the master used for a multi-recording Drive case.
    #[test]
    #[ignore]
    fn folder_master_retains_full_resolution_and_all_recordings() {
        let dir = std::env::temp_dir().join(format!("writer-folder-master-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, out) = (dir.join("a.mp4"), dir.join("b.mp4"), dir.join("master.mp4"));
        make(&["-f", "lavfi", "-i", "testsrc=s=1280x720:d=1", "-f", "lavfi", "-i", "sine=d=1", "-shortest"], &a);
        make(&["-f", "lavfi", "-i", "testsrc=s=640x360:d=1"], &b);
        let parts: Vec<JoinPart> = [&a, &b].iter().map(|path| JoinPart { path, streams: streams(path).unwrap() }).collect();
        super::join_original(&parts, &out, &|_| {}).unwrap();
        let found = streams(&out).unwrap();
        assert!(found.video && found.audio);
        assert!((1900..=2200).contains(&found.duration_ms));
        let dimensions = crate::proc::run("ffprobe", &["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0", &out.to_string_lossy()], Duration::from_secs(30)).unwrap();
        assert_eq!(dimensions.trim(), "1280,720");
        assert!(a.is_file() && b.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Needs ffmpeg: cargo test -- --ignored appends_without_encoding_again
    #[test]
    #[ignore]
    fn appends_without_encoding_again() {
        let dir = std::env::temp_dir().join("writer-append-test");
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a.mp4"), dir.join("b.mp4"), dir.join("c.mov"));
        make(&["-f", "lavfi", "-i", "testsrc=s=640x360:d=3", "-f", "lavfi", "-i", "sine=d=3", "-shortest"], &a);
        make(&["-f", "lavfi", "-i", "testsrc=s=1280x720:d=4", "-f", "lavfi", "-i", "sine=f=330:d=4", "-shortest"], &c);
        let one = |path: &Path, out: &Path| join(&[JoinPart { path, streams: streams(path).unwrap() }], out, &|_| {}).unwrap();
        let (base, more) = (dir.join("base.mp4"), dir.join("more.mp4"));
        one(&a, &base);
        one(&c, &more);
        assert!(append(&base, &[more.as_path()], &b, &|_| {}).unwrap(), "copied, not encoded again");
        let found = streams(&b).unwrap();
        assert!(found.video && found.audio);
        assert!((6_800..=7_300).contains(&found.duration_ms), "{}", found.duration_ms);
        // A piece that doesn't match is joined afresh, never copied.
        assert!(!append(&a, &[more.as_path()], &b, &|_| {}).unwrap());
        assert!((6_800..=7_300).contains(&streams(&b).unwrap().duration_ms));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
