use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Run a program, streaming stdout lines, and kill it if it outlives `timeout`.
/// Stderr is returned only when the program fails.
pub fn run_lines(
    program: &str,
    args: &[&str],
    timeout: Duration,
    stdin_bytes: Option<&[u8]>,
    mut on_line: impl FnMut(&str),
) -> Result<(), String> {
    let mut command = Command::new(crate::tools::resolve(program));
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin_bytes.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| {
        format!("{program} failed to start ({error}). Install it and try again.")
    })?;
    if let Some(bytes) = stdin_bytes {
        let mut stdin = child.stdin.take().ok_or(format!("{program} stdin"))?;
        stdin
            .write_all(bytes)
            .map_err(|error| format!("{program}: {error}"))?;
        drop(stdin);
    }

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
        while let Ok(line) = rx.try_recv() {
            on_line(&line);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                while let Ok(line) = rx.try_recv() {
                    on_line(&line);
                }
                let err = errors.join().unwrap_or_default();
                if status.success() {
                    // Anything slow that went fine still says how long it took.
                    if started.elapsed() > Duration::from_secs(2) && program != "claude" {
                        crate::log::info("tool", format!("{} finished in {}", summary(program, args), crate::log::since_start(started)));
                    }
                    return Ok(());
                }
                // Some runs are expected to fail (a probe for a stream that isn't there): whoever ran it logs a failure that matters.
                crate::log::info("tool", format!("{} exited with an error after {}: {}", summary(program, args), crate::log::since_start(started), clip(&err)));
                return Err(format!("{program} failed: {}", clip(&err)));
            }
            Ok(None) if started.elapsed() > timeout => {
                crate::log::error("tool", format!("{} was stopped after {}: it took too long", summary(program, args), crate::log::since_start(started)));
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} took too long and was stopped."));
            }
            Ok(None) => thread::sleep(Duration::from_millis(120)),
            Err(error) => return Err(format!("{program}: {error}")),
        }
    }
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
