use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Claude Code, or writer-agent (and the Bun that runs it in a dev build).
fn is_claude(program: &str) -> bool {
    let name = std::path::Path::new(program).file_name().and_then(|name| name.to_str()).unwrap_or(program);
    matches!(name, "claude" | "writer-agent" | "bun") || name.starts_with("writer-agent-")
}

/// What Claude may see of Writer's environment: enough to find the person's
/// own sign-in and tools, nothing else. An ANTHROPIC_API_KEY (or a base URL,
/// Bedrock, Vertex) in the shell Writer started from would bill builds at API
/// prices, or send them elsewhere, instead of using the person's plan.
const CLAUDE_ENV: [&str; 11] = ["HOME", "USER", "LOGNAME", "PATH", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE", "SHELL", "TERM", "CLAUDE_CONFIG_DIR"];

/// An empty folder to start Claude in: it reads its own folder without
/// asking, so it starts where there is nothing (a question's folder to read
/// is added to it, and nothing else).
pub fn claude_home() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("writer-claude");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn is_yt_dlp(program: &str) -> bool {
    let name = std::path::Path::new(program).file_name().and_then(|name| name.to_str()).unwrap_or(program);
    matches!(name, "yt-dlp" | "yt-dlp_macos")
}

/// Arguments for a yt-dlp run: Writer's JavaScript runtime and ffmpeg first,
/// then whatever the caller asked for. Both are absolute paths from
/// `tools::locate`, never a name looked up on PATH.
fn yt_dlp_args(args: &[&str]) -> Vec<String> {
    let mut owned = Vec::new();
    if let Some((name, path)) = crate::tools::js_runtime() {
        owned.push("--js-runtimes".to_string());
        owned.push(crate::tools::js_runtime_arg(name, &path));
    }
    if let Some((path, _)) = crate::tools::locate("ffmpeg") {
        owned.push("--ffmpeg-location".to_string());
        owned.push(path.to_string_lossy().into_owned());
    }
    owned.extend(args.iter().map(|arg| (*arg).to_string()));
    owned
}

fn for_claude(command: &mut Command) {
    command.env_clear();
    for key in CLAUDE_ENV {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.current_dir(claude_home());
}

/// Run a program, streaming stdout lines, and kill it if it outlives `timeout`.
/// Stderr is returned only when the program fails.
pub fn run_lines(
    program: &str,
    args: &[&str],
    timeout: Duration,
    stdin_bytes: Option<&[u8]>,
    mut on_line: impl FnMut(&str),
) -> Result<(), String> {
    crate::jobs::check_cancelled()?;
    // yt-dlp needs a JavaScript runtime for YouTube, and ffmpeg to join a
    // picture stream to its sound, both named explicitly: a Dock-launched
    // Writer has no PATH worth searching. yt-dlp enables only deno by
    // default, and without ffmpeg it exits 0 having written the two streams
    // and not the file it was asked for.
    let owned_args: Vec<String> = if is_yt_dlp(program) { yt_dlp_args(args) } else { args.iter().map(|arg| arg.to_string()).collect() };
    let arg_refs: Vec<&str> = owned_args.iter().map(String::as_str).collect();
    let args: &[&str] = &arg_refs;
    let mut command = Command::new(crate::tools::resolve(program));
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if is_claude(program) {
        for_claude(&mut command);
        #[cfg(unix)]
        { use std::os::unix::process::CommandExt; command.process_group(0); }
    }
    if stdin_bytes.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| {
        format!("{program} failed to start ({error}). Install it and try again.")
    })?;
    // The input goes in on its own thread, so a program that writes a lot before
    // reading it all can't stall on a full pipe. One that quits before reading it
    // all (a broken pipe) is told by its exit and its stderr, which say why.
    let mut feeding = match stdin_bytes {
        Some(bytes) => {
            let mut stdin = child.stdin.take().ok_or(format!("{program} stdin"))?;
            let bytes = bytes.to_vec();
            Some(thread::spawn(move || match stdin.write_all(&bytes) {
                Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => Some(error.to_string()),
                _ => None,
            }))
        }
        None => None,
    };

    let mut stdout = child.stdout.take().ok_or(format!("{program} stdout"))?;
    let mut stderr = child.stderr.take().ok_or(format!("{program} stderr"))?;
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let mut reader = BufReader::new(&mut stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let _ = tx.send(line.trim_end().to_string());
                }
            }
        }
    });
    let errors = thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
        text
    });

    let started = Instant::now();
    loop {
        if crate::jobs::cancelled() {
            stop_child(&mut child, is_claude(program));
            return Err("Stopped by you. Completed work is saved.".into());
        }
        while let Ok(line) = rx.try_recv() {
            on_line(&line);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                while let Ok(line) = rx.try_recv() {
                    on_line(&line);
                }
                let err = errors.join().unwrap_or_default();
                let fed = feeding.take().and_then(|feeding| feeding.join().ok().flatten());
                if status.success() {
                    // Anything slow that went fine still says how long it took.
                    if started.elapsed() > Duration::from_secs(2) && program != "claude" {
                        crate::log::info("tool", format!("{} finished in {}", summary(program, args), crate::log::since_start(started)));
                    }
                    return Ok(());
                }
                // Some runs are expected to fail (a probe for a stream that isn't there): whoever ran it logs a failure that matters.
                crate::log::info("tool", format!("{} exited with an error after {}: {}", summary(program, args), crate::log::since_start(started), clip(&err)));
                if err.trim().is_empty() {
                    if let Some(fed) = fed {
                        return Err(format!("{program} failed: couldn't send it its input ({fed})"));
                    }
                }
                return Err(format!("{program} failed: {}", clip(&err)));
            }
            Ok(None) if started.elapsed() > timeout => {
                crate::log::error("tool", format!("{} was stopped after {}: it took too long", summary(program, args), crate::log::since_start(started)));
                stop_child(&mut child, is_claude(program));
                return Err(format!("{program} took too long and was stopped."));
            }
            Ok(None) => thread::sleep(Duration::from_millis(120)),
            Err(error) => return Err(format!("{program}: {error}")),
        }
    }
}

fn stop_child(child: &mut std::process::Child, group: bool) {
    #[cfg(unix)]
    if group { unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); } }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn run(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut lines = Vec::new();
    run_lines(program, args, timeout, None, |line| lines.push(line.to_string()))?;
    Ok(lines.join("\n"))
}

/// A command as a log line names it: the program and its arguments, files by
/// name only, long arguments cut.
fn summary(program: &str, args: &[&str]) -> String {
    let shown: Vec<String> = args
        .iter()
        .take(14)
        .map(|arg| {
            let arg = if arg.starts_with('/') { std::path::Path::new(arg).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| arg.to_string()) } else { arg.to_string() };
            if arg.chars().count() > 48 { format!("{}…", arg.chars().take(48).collect::<String>()) } else { arg }
        })
        .collect();
    format!("{program} {}{}", shown.join(" "), if args.len() > 14 { " …" } else { "" })
}

fn clip(text: &str) -> String {
    let trimmed = text.trim();
    let end = trimmed.char_indices().nth(400).map(|(i, _)| i).unwrap_or(trimmed.len());
    if trimmed.is_empty() {
        "no details".to_string()
    } else {
        trimmed[..end].replace('\n', " ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_quits_before_reading_its_input_says_why() {
        let input = vec![b'x'; 4 << 20];
        let error = run_lines("sh", &["-c", "echo 'Prompt is too long' >&2; exit 1"], Duration::from_secs(10), Some(&input), |_| {}).unwrap_err();
        assert!(error.contains("Prompt is too long"), "{error}");
        assert!(!error.contains("Broken pipe"), "{error}");
    }

    #[test]
    fn stopping_a_job_terminates_its_running_command() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let request = flag.clone();
        let worker = std::thread::spawn(move || crate::jobs::with_cancellation(flag, || run("/bin/sleep", &["30"], Duration::from_secs(60))));
        std::thread::sleep(Duration::from_millis(150));
        let began = Instant::now();
        request.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(worker.join().unwrap().unwrap_err().contains("Stopped by you"));
        assert!(began.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn yt_dlp_is_pointed_at_ffmpeg() {
        let args = yt_dlp_args(&["--version"]);
        let place = args.iter().position(|arg| arg == "--ffmpeg-location").expect("ffmpeg location");
        let path = args.get(place + 1).map(String::as_str).unwrap_or("");
        assert!(std::path::Path::new(path).is_file(), "{path}");
        assert!(args.iter().any(|arg| arg == "--version"));
    }

    #[test]
    fn claude_sees_no_api_key() {
        assert!(is_claude("/opt/homebrew/bin/claude") && is_claude("writer-agent") && is_claude("writer-agent-aarch64-apple-darwin") && !is_claude("yt-dlp"));
        let mut command = Command::new("/usr/bin/env");
        std::env::set_var("ANTHROPIC_API_KEY", "sk-test-not-real");
        for_claude(&mut command);
        let shown = String::from_utf8(command.output().unwrap().stdout).unwrap();
        assert!(!shown.contains("ANTHROPIC_API_KEY") && shown.contains("HOME="), "{shown}");
        std::env::remove_var("ANTHROPIC_API_KEY");
    }
}

