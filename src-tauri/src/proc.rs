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
                    return Ok(());
                }
                return Err(format!("{program} failed: {}", clip(&err)));
            }
            Ok(None) if started.elapsed() > timeout => {
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

fn clip(text: &str) -> String {
    let trimmed = text.trim();
    let end = trimmed.char_indices().nth(400).map(|(i, _)| i).unwrap_or(trimmed.len());
    if trimmed.is_empty() {
        "no details".to_string()
    } else {
        trimmed[..end].replace('\n', " ")
    }
}
