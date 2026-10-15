//! HTTP, done by Writer itself, so nobody needs curl or a key of their own.
//! Every secret (ElevenLabs, Repack's Drive account, the DataImpulse proxy)
//! lives on the relay; Writer only holds the relay's client token.

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::settings::{self, Settings};

/// Connects within 30 s; a read that stalls for two minutes is a failure the
/// caller can retry.
pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .user_agent("Writer")
        .build()
}

/// A failed request: what the server said; that nothing was sent (no
/// connection at all, so safe to send again); or that the connection broke.
pub enum Failed {
    Status(u16, String),
    Unsent(String),
    Network(String),
}

impl From<ureq::Error> for Failed {
    fn from(error: ureq::Error) -> Self {
        match error {
            ureq::Error::Status(code, response) => {
                Failed::Status(code, response.into_string().unwrap_or_default().chars().take(2_000).collect())
            }
            ureq::Error::Transport(transport) => match transport.kind() {
                ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed => Failed::Unsent(transport.to_string()),
                _ => Failed::Network(transport.to_string()),
            },
        }
    }
}

/// What a JSON error body says, or nothing.
pub fn said(body: &str) -> String {
    let value: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    value["error"]
        .as_str()
        .or_else(|| value["detail"]["message"].as_str())
        .or_else(|| value["message"].as_str())
        .unwrap_or("")
        .trim()
        .chars()
        .take(300)
        .collect()
}

pub struct Relay {
    base: String,
    token: String,
}

impl Relay {
    pub fn new(settings: &Settings) -> Result<Self, String> {
        let base = settings::check_relay(&settings.relay_url)?;
        if settings.relay_token.chars().any(char::is_control) {
            return Err("This copy of Writer has a broken service token. Get a fresh copy of Writer.".into());
        }
        Ok(Self { base, token: settings.relay_token.clone() })
    }

    fn request(&self, agent: &ureq::Agent, method: &str, path: &str) -> ureq::Request {
        let request = agent.request(method, &format!("{}{path}", self.base));
        if self.token.is_empty() {
            request
        } else {
            request.set("authorization", &format!("Bearer {}", self.token))
        }
    }

    fn get_json(&self, path: &str) -> Result<Value, String> {
        match self.request(&agent(), "GET", path).call() {
            Ok(response) => response.into_json().map_err(|_| "Writer's service answered with something Writer can't read.".into()),
            Err(error) => Err(match Failed::from(error) {
                Failed::Status(401, _) => "Writer's service turned this copy of Writer away. Get a fresh copy of Writer.".into(),
                Failed::Status(code, body) => {
                    let said = said(&body);
                    if said.is_empty() { format!("Writer's service answered {code}.") } else { said }
                }
                Failed::Unsent(reason) | Failed::Network(reason) => format!("Could not reach Writer's service ({reason}). Check the internet connection."),
            }),
        }
    }

    pub fn health(&self) -> Result<Value, String> {
        self.get_json("/health")
    }

    /// A one-hour access token for Repack's Drive account.
    pub fn drive_token(&self) -> Result<String, String> {
        let value = self.get_json("/v1/drive/token")?;
        value["accessToken"]
            .as_str()
            .filter(|token| !token.is_empty() && !token.chars().any(char::is_control))
            .map(str::to_string)
            .ok_or_else(|| "Writer's service sent no Google Drive access.".into())
    }

    /// The proxy YouTube downloads go through, if the relay has one.
    pub fn proxy(&self) -> Result<Option<String>, String> {
        let value = self.get_json("/v1/proxy")?;
        Ok(value["proxyUrl"].as_str().filter(|url| !url.is_empty()).map(str::to_string))
    }

    /// A JSON POST to the relay that may take minutes, with its refusal said plainly.
    fn post_json(&self, path: &str, body: &Value, minutes: u64) -> Result<Value, Failed> {
        let slow = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(30))
            .timeout(Duration::from_secs(minutes * 60))
            .user_agent("Writer")
            .build();
        let bytes = serde_json::to_vec(body).map_err(|error| Failed::Unsent(error.to_string()))?;
        let response = self
            .request(&slow, "POST", path)
            .set("content-type", "application/json")
            .send_bytes(&bytes)?;
        response.into_json().map_err(|error| Failed::Network(format!("unreadable answer: {error}")))
    }

    /// Gemini watches a window of footage: `system` and `text` say what to
    /// answer, `clip` is the MP4 in base64, `schema` the answer's shape. The
    /// relay picks the model and holds the key. Answers the gateway's JSON.
    pub fn watch(&self, system: &str, text: &str, clip: &str, schema: &Value) -> Result<Value, Failed> {
        let body = serde_json::json!({ "system": system, "text": text, "clip": clip, "schema": schema });
        self.post_json("/v1/watch", &body, 5)
    }

    /// Exports a script, as a Word file in base64, to a new Google Doc open to
    /// anyone with its link. Answers the Doc as the relay lists it.
    pub fn export_doc(&self, name: &str, script_id: &str, draft: bool, docx: &str) -> Result<Value, String> {
        let body = serde_json::json!({ "name": name, "scriptId": script_id, "draft": draft, "docx": docx });
        self.post_json("/v1/docs/export", &body, 3).map_err(|failed| explain_docs(failed))
    }

    /// Every Doc exported from a script, newest first.
    pub fn list_docs(&self, script_id: &str) -> Result<Value, String> {
        self.get_json(&format!("/v1/docs/exports?script={script_id}"))
    }

    /// The comments people left on one of a script's exported Docs.
    pub fn doc_comments(&self, script_id: &str, doc: &str) -> Result<Value, String> {
        self.get_json(&format!("/v1/docs/comments?script={script_id}&doc={doc}"))
    }

    /// Sends one piece of Opus audio to be heard. Waits up to half an hour.
    pub fn transcribe(&self, audio: &Path) -> Result<Value, Failed> {
        let size = std::fs::metadata(audio).map_err(|error| Failed::Network(error.to_string()))?.len();
        let file = File::open(audio).map_err(|error| Failed::Network(error.to_string()))?;
        let slow = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(30))
            .timeout(Duration::from_secs(30 * 60))
            .user_agent("Writer")
            .build();
        let response = self
            .request(&slow, "POST", "/v1/transcribe?language=en")
            .set("content-type", "audio/ogg")
            .set("content-length", &size.to_string())
            .send(file)?;
        response.into_json().map_err(|error| Failed::Network(format!("unreadable answer: {error}")))
    }
}

fn explain_docs(failed: Failed) -> String {
    match failed {
        Failed::Status(401, _) => "Writer's service turned this copy of Writer away. Get a fresh copy of Writer.".into(),
        Failed::Status(code, body) => {
            let said = said(&body);
            if said.is_empty() { format!("Google Docs export failed ({code}).") } else { said }
        }
        Failed::Unsent(reason) | Failed::Network(reason) => format!("Could not reach Writer's service ({reason}). Check the internet connection."),
    }
}
