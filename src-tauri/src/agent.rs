//! Claude through Anthropic's Agent SDK, as Edit's agent reaches it: Writer
//! starts writer-agent (sidecar/, a Bun program beside Writer in the app)
//! once per question, hands it the question as one JSON line, and reads one
//! line per event back: each reply's stop reason, then the result. It runs
//! the user's own `claude`, with nothing of their setup (sidecar/src/main.ts).
//!
//! Used when Setup's engine is `sdk`; `claude::ask` sends each single answer
//! here, and `steps` builds a script through Writer's tools here.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::claude::Trouble;

/// writer-agent beside Writer in the app; in a dev build, the source run with
/// Bun (the repo's `sidecar/`).
fn launcher() -> Result<(String, Vec<String>), String> {
    if let Some((path, _)) = crate::tools::locate("writer-agent") {
        return Ok((path.to_string_lossy().into_owned(), Vec::new()));
    }
    if cfg!(debug_assertions) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sidecar/src/main.ts");
        if source.is_file() {
            return Ok(("bun".into(), vec![source.to_string_lossy().into_owned()]));
        }
    }
    Err("This copy of Writer is missing writer-agent. Get a fresh copy of Writer.".into())
}

/// One question for Claude.
pub struct Question<'a> {
    pub model: &'a str,
    pub system: &'a str,
    pub prompt: &'a str,
    pub schema: Option<&'a Value>,
    pub resume: Option<&'a str>,
    pub read: Option<&'a Path>,
    /// Writer's tools, served at this address (steps.rs).
    pub tools: Option<&'a str>,
    pub max_turns: Option<u32>,
    pub timeout: Duration,
    /// For the log: "Claude opus", "Claude opus (repair turn)".
    pub label: String,
}

/// What a question came back with.
#[derive(Debug, Default)]
pub struct Answer {
    pub structured: Option<Value>,
    pub text: Option<String>,
    pub session: Option<String>,
    pub cost_usd: f64,
    /// The result, as `claude::why_stopped` reads it.
    pub result: Value,
    /// Replies in all, and the biggest one's output tokens.
    pub replies: usize,
    pub biggest_reply: u64,
}

pub fn ask(question: Question, data: &Path) -> Result<Answer, Trouble> {
    let (program, mut args) = launcher().map_err(Trouble::Stop)?;
    let claude = crate::tools::locate("claude").map(|(path, _)| path).ok_or_else(|| Trouble::Stop("Install Claude Code on this Mac (the claude command), then build again.".into()))?;
    let request = json!({
        "binary": claude,
        "cwd": data,
        "version": crate::release::tag().map(str::to_string).unwrap_or_else(|| format!("{}-dev", crate::release::VERSION)),
        "model": question.model,
        "system": question.system,
        "prompt": question.prompt,
        "schema": question.schema,
        "resume": question.resume,
        "readDir": question.read,
        "mcp": question.tools.map(|url| json!({ "name": "writer", "url": url })),
        "maxTurns": question.max_turns,
    });
    let label = question.label.clone();
    let began = std::time::Instant::now();
    crate::log::info("agent", format!("{label} via the Agent SDK: asked ({} of prompt)", crate::log::size(question.prompt.len() as u64)));
    let mut answer = Answer::default();
    let mut failed: Option<String> = None;
    let mut stops: Vec<String> = Vec::new();
    let line_args: Vec<&str> = args.iter().map(String::as_str).collect();
    let ran = crate::proc::run_lines(&program, &line_args, question.timeout, Some(request.to_string().as_bytes()), |line| {
        let Ok(event) = serde_json::from_str::<Value>(line) else { return };
        match event["type"].as_str() {
            Some("reply") => {
                answer.replies += 1;
                let tokens = event["outputTokens"].as_u64().unwrap_or(0);
                answer.biggest_reply = answer.biggest_reply.max(tokens);
                if let Some(stop) = event["stopReason"].as_str() {
                    if stop == "max_tokens" || stop == "refusal" {
                        stops.push(stop.to_string());
                    }
                }
            }
            Some("result") => answer.result = event,
            Some("error") => failed = Some(event["message"].as_str().unwrap_or("writer-agent stopped").to_string()),
            _ => {}
        }
    });
    args.clear();
    let result = answer.result.clone();
    // The result in the shape claude -p prints it, so the same readers apply.
    let printed = json!({
        "subtype": result["subtype"],
        "stop_reason": result["stopReason"],
        "terminal_reason": result["terminalReason"],
        "num_turns": result["numTurns"],
        "usage": { "output_tokens": result["outputTokens"] },
        "is_error": result["isError"],
        "result": result["result"],
        "api_error_status": result["apiErrorStatus"],
    });
    let why = format!("{}; {} replies, the biggest {} output tokens{}", crate::claude::why_stopped(&printed), answer.replies, answer.biggest_reply, if stops.is_empty() { String::new() } else { format!(", stopped short: {}", stops.join(", ")) });
    if result.is_null() {
        let said = failed.or_else(|| ran.err()).unwrap_or_else(|| "writer-agent ended without an answer".into());
        crate::log::error("agent", format!("{label}: failed after {}: {}", crate::log::since_start(began), said.chars().take(300).collect::<String>()));
        return Err(crate::claude::explain_failure(&said, None));
    }
    if result["isError"] == true {
        let errors = result["errors"].as_array().map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")).unwrap_or_default();
        let said = result["result"].as_str().map(str::to_string).filter(|text| !text.is_empty()).unwrap_or(errors);
        crate::log::error("agent", format!("{label}: stopped after {} ({why}): {}", crate::log::since_start(began), said.chars().take(300).collect::<String>()));
        return Err(crate::claude::stopped_short(&printed).unwrap_or_else(|| crate::claude::explain_failure(&said, result["apiErrorStatus"].as_u64())));
    }
    answer.cost_usd = result["costUsd"].as_f64().unwrap_or(0.0);
    answer.session = result["sessionId"].as_str().map(str::to_string);
    answer.text = result["result"].as_str().map(str::to_string);
    answer.structured = result["structured"].as_object().map(|_| result["structured"].clone());
    crate::log::info("agent", format!("{label}: answered in {} (${:.2} at API prices; {why})", crate::log::since_start(began), answer.cost_usd));
    answer.result = printed;
    Ok(answer)
}

/// Where a question runs: an empty folder, since Claude reads its own folder
/// without asking (Writer's data folder holds its settings and logs).
pub fn workspace() -> PathBuf {
    crate::proc::claude_home()
}
