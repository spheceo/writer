//! Signing in to Writer, at accounts.spheceo.com, so the relay knows who is
//! asking and what they may use. Done as RFC 8252 has native apps do it:
//! Writer listens on `127.0.0.1:<a free port>/callback`, opens the browser at
//! `/app/authorize` with PKCE (S256) and a random `state`, and trades the code
//! it's sent back for tokens at `/app/token`.
//!
//! The access token (an hour) stays in memory and goes on every relay call
//! (`bearer`), renewed quietly ten minutes before it ends. The refresh token
//! (30 days from last use, a new one on every use) is kept in the Keychain;
//! a dev build, re-signed on every rebuild, would be asked for Keychain access
//! each time, so it keeps it in `account-dev.json` (0600) instead.
//!
//! Until everyone is on a build that signs in, the relay still takes the old
//! team token built into Writer: signed out, Writer sends that.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::{log, release};

const ACCOUNTS: &str = "https://accounts.spheceo.com";
const CLIENT_ID: &str = "writer";
const KEYCHAIN_SERVICE: &str = "com.spheceo.writer.accounts";
const KEYCHAIN_ACCOUNT: &str = "refresh-token";
/// Renewed this long before the access token ends.
const EARLY: Duration = Duration::from_secs(600);
/// How long a sign-in waits for the browser.
const WAIT: Duration = Duration::from_secs(600);
/// After a renewal the relay still refuses, no forced renewal for this long.
const FORCED_GAP: Duration = Duration::from_secs(60);
/// No answer from accounts: wait this long before trying again.
const OFFLINE_WAIT: Duration = Duration::from_secs(30);

/// Where people sign in. A debug build may point elsewhere, for testing.
fn accounts() -> String {
    if cfg!(debug_assertions) {
        if let Ok(url) = std::env::var("WRITER_ACCOUNTS_URL") {
            if !url.trim().is_empty() {
                return url.trim().trim_end_matches('/').to_string();
            }
        }
    }
    ACCOUNTS.into()
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub sub: String,
    pub email: String,
    pub name: String,
    pub image: Option<String>,
    pub perms: Vec<String>,
    pub admin: bool,
}

struct Session {
    access: String,
    until: Instant,
    person: Person,
}

#[derive(Default)]
struct State {
    session: Option<Session>,
    /// A refresh token is kept (signed in, though perhaps offline).
    kept: bool,
    signing_in: bool,
    error: Option<String>,
    /// No renewal before this: accounts asked Writer to wait (429), or didn't answer.
    hold: Option<Instant>,
    renewed: Option<Instant>,
    /// The sign-in page a waiting sign-in opened (no secret in it: PKCE keeps the verifier here).
    url: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State { session: None, kept: false, signing_in: false, error: None, hold: None, renewed: None, url: None });
/// One renewal at a time: a refresh token works once, and reusing it ends the sign-in.
static RENEWING: Mutex<()> = Mutex::new(());
static DATA: OnceLock<PathBuf> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub signed_in: bool,
    pub person: Option<Person>,
    pub signing_in: bool,
    /// While signing in: the page to open, if the browser didn't.
    pub sign_in_url: Option<String>,
    pub error: Option<String>,
    /// Signing in is a must: this copy has no old team token to fall back on.
    pub required: bool,
}

pub fn status() -> Status {
    let state = lock();
    Status {
        signed_in: state.kept,
        person: state.session.as_ref().map(|session| session.person.clone()),
        signing_in: state.signing_in,
        sign_in_url: state.signing_in.then(|| state.url.clone()).flatten(),
        error: state.error.clone(),
        required: !has_old_token(),
    }
}

fn has_old_token() -> bool {
    DATA.get().is_some_and(|data| !crate::settings::load(data).relay_token.is_empty())
}

fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit("account", ());
    }
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// At launch: picks up a kept sign-in, and keeps the access token fresh.
pub fn start(app: AppHandle, data: PathBuf) {
    let _ = DATA.set(data);
    let _ = APP.set(app);
    if load_refresh().is_some() {
        lock().kept = true;
    }
    std::thread::spawn(|| loop {
        let due = {
            let state = lock();
            state.kept && state.session.as_ref().is_none_or(|session| session.until <= Instant::now() + EARLY)
        };
        if due {
            let _ = renew();
        }
        std::thread::sleep(Duration::from_secs(60));
    });
}

/// The `Authorization` value for a relay call: the person's access token when
/// signed in (renewed first if it's about to end), or else the old team token.
pub fn bearer(old_token: &str) -> Option<String> {
    if let Some(token) = access_token() {
        return Some(format!("Bearer {token}"));
    }
    (!old_token.is_empty()).then(|| format!("Bearer {old_token}"))
}

/// Writer can reach the relay at all: signed in, or with the old team token.
pub fn can_call(old_token: &str) -> bool {
    lock().kept || !old_token.is_empty()
}

fn access_token() -> Option<String> {
    {
        let state = lock();
        if !state.kept {
            return None;
        }
        if let Some(session) = &state.session {
            if session.until > Instant::now() + Duration::from_secs(60) {
                return Some(session.access.clone());
            }
        }
    }
    renew().ok()
}

/// The relay refused the access token (401): ask for a new one next time,
/// unless one was just got (a relay that refuses every token mustn't turn
/// each call into a renewal).
pub fn refused() {
    let mut state = lock();
    if state.renewed.is_some_and(|at| at.elapsed() < FORCED_GAP) {
        return;
    }
    if let Some(session) = state.session.as_mut() {
        session.until = Instant::now();
    }
}

/// Debug builds only: renews now, as a refused token would, for testing.
#[cfg(debug_assertions)]
pub fn renew_now() -> Result<Status, String> {
    if let Some(session) = lock().session.as_mut() {
        session.until = Instant::now();
    }
    renew()?;
    Ok(status())
}

/// What a relay refusal means, said plainly: 401 is the sign-in (a token it
/// didn't accept is renewed before the next call), 403 a permission the
/// person lacks (the relay names it).
pub fn turned_away(code: u16, said: &str) -> String {
    if code == 403 && !said.is_empty() {
        return said.to_string();
    }
    if lock().kept {
        refused();
        return "Writer's service didn't accept your sign-in. Try again; if it keeps happening, sign in again in Setup.".into();
    }
    "Writer's service needs you to sign in. Open Setup and sign in.".into()
}

/// Trades the kept refresh token for a new pair. Only `invalid_grant` signs
/// out; told to wait (429) or not answered, Writer keeps the refresh token
/// and holds off before asking again.
fn renew() -> Result<String, String> {
    let _one = RENEWING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    {
        let state = lock();
        // Someone else may have just renewed it.
        if let Some(session) = &state.session {
            if session.until > Instant::now() + EARLY {
                return Ok(session.access.clone());
            }
        }
        if state.hold.is_some_and(|until| until > Instant::now()) {
            return Err("waiting before renewing the sign-in".into());
        }
    }
    let Some(refresh) = load_refresh() else {
        signed_out(None);
        return Err("Not signed in.".into());
    };
    match token_request(json!({ "grant_type": "refresh_token", "refresh_token": refresh, "client_id": CLIENT_ID })) {
        Ok(answer) => {
            let access = take(&answer)?;
            let mut state = lock();
            state.hold = None;
            state.renewed = Some(Instant::now());
            drop(state);
            log::info("account", "renewed the sign-in");
            Ok(access)
        }
        Err(Failure::Refused(why)) => {
            log::warn("account", format!("the sign-in was refused ({why}); signed out"));
            delete_refresh();
            signed_out(Some("Your sign-in ended. Sign in again.".into()));
            Err(why)
        }
        Err(Failure::Offline(why, wait)) => {
            log::warn("account", format!("couldn't renew the sign-in ({why}); trying again in {} s", wait.as_secs()));
            lock().hold = Some(Instant::now() + wait);
            Err(why)
        }
    }
}

/// Keeps a token answer: the refresh token first, so it's never lost.
fn take(answer: &Value) -> Result<String, String> {
    let access = answer["access_token"].as_str().filter(|token| !token.is_empty()).ok_or("accounts.spheceo.com sent no access token")?.to_string();
    let refresh = answer["refresh_token"].as_str().filter(|token| !token.is_empty()).ok_or("accounts.spheceo.com sent no refresh token")?;
    save_refresh(refresh)?;
    let seconds = answer["expires_in"].as_u64().unwrap_or(3600).clamp(60, 86_400);
    let mut person = claims(&access);
    if let Some(user) = answer["user"].as_object() {
        let text = |key: &str| user.get(key).and_then(Value::as_str).map(str::to_string);
        person.email = text("email").unwrap_or(person.email);
        person.name = text("name").unwrap_or(person.name);
        person.image = text("image").or(person.image);
    }
    let mut state = lock();
    state.session = Some(Session { access: access.clone(), until: Instant::now() + Duration::from_secs(seconds), person });
    state.kept = true;
    state.error = None;
    drop(state);
    changed();
    Ok(access)
}

fn signed_out(error: Option<String>) {
    let mut state = lock();
    state.session = None;
    state.kept = false;
    state.error = error;
    drop(state);
    changed();
}

/// What the access token says about its person. Only read for showing them
/// and greying out what they can't use; the relay is what checks it.
fn claims(token: &str) -> Person {
    let payload = token.split('.').nth(1).and_then(|part| URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).ok());
    let value: Value = payload.and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null);
    let text = |key: &str| value[key].as_str().unwrap_or("").to_string();
    Person {
        sub: text("sub"),
        email: text("email"),
        name: text("name"),
        image: value["picture"].as_str().or(value["image"].as_str()).map(str::to_string),
        perms: value["perms"].as_array().map(|perms| perms.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
        admin: value["admin"] == true,
    }
}

#[derive(Debug)]
enum Failure {
    /// `invalid_grant`: the code or refresh token is dead.
    Refused(String),
    /// Anything else (no answer, 429, the site broke): try again after this long.
    Offline(String, Duration),
}

/// What an error answer from accounts means. Only `invalid_grant` ends a
/// sign-in; 429 says how long to wait (Retry-After, a minute if it doesn't).
fn classify(code: u16, retry_after: Option<&str>, body: &Value) -> Failure {
    let error = body["error"].as_str().unwrap_or("");
    let detail = body["error_description"].as_str().map(|text| format!(": {text}")).unwrap_or_default();
    let said = format!("{code} {}{detail}", if error.is_empty() { "error" } else { error });
    if error == "invalid_grant" {
        return Failure::Refused(said);
    }
    let wait = if code == 429 {
        Duration::from_secs(retry_after.and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(60).clamp(1, 600))
    } else {
        OFFLINE_WAIT
    };
    Failure::Offline(said, wait)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout(Duration::from_secs(30)).user_agent(&user_agent()).build()
}

/// What admins see as "where" on a person's page: "Writer/nightly-3 (macOS)",
/// "Writer/v0.0.1 (macOS)", or for a dev build "Writer/0.0.0-dev (macOS)".
fn user_agent() -> String {
    let build = release::tag().map(str::to_string).unwrap_or_else(|| format!("{}-dev", release::VERSION));
    format!("Writer/{build} (macOS)")
}

fn token_request(body: Value) -> Result<Value, Failure> {
    match agent().post(&format!("{}/app/token", accounts())).set("content-type", "application/json").send_json(body) {
        Ok(response) => response.into_json().map_err(|_| Failure::Offline("accounts.spheceo.com answered with something Writer can't read".into(), OFFLINE_WAIT)),
        Err(ureq::Error::Status(code, response)) => {
            let retry_after = response.header("retry-after").map(str::to_string);
            let body: Value = response.into_json().unwrap_or(Value::Null);
            Err(classify(code, retry_after.as_deref(), &body))
        }
        Err(ureq::Error::Transport(transport)) => Err(Failure::Offline(transport.to_string(), OFFLINE_WAIT)),
    }
}

// ---- Signing in ----

/// Opens the browser to sign in, and waits (in the background) for it to come back.
pub fn sign_in() -> Result<(), String> {
    {
        let mut state = lock();
        if state.signing_in {
            CANCEL.store(true, Ordering::SeqCst);
        }
        state.signing_in = true;
        state.error = None;
    }
    changed();
    // Let a sign-in already waiting notice it was replaced.
    std::thread::sleep(Duration::from_millis(250));
    CANCEL.store(false, Ordering::SeqCst);
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| finish_err(format!("Writer couldn't listen for the sign-in: {error}")))?;
    let port = listener.local_addr().map_err(|error| finish_err(error.to_string()))?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let verifier = random(32)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random(16)?;
    let url = authorize_url(&redirect, &state, &challenge);
    lock().url = Some(url.clone());
    changed();
    // A debug build can leave the browser to someone else (a test opening the page for it).
    if cfg!(debug_assertions) && std::env::var("WRITER_NO_BROWSER").is_ok_and(|value| value == "1") {
        log::info("account", format!("signing in: waiting on port {port}; the page wasn't opened (WRITER_NO_BROWSER)"));
    } else {
        log::info("account", format!("signing in: browser opened, waiting on port {port}"));
        crate::proc::run("open", &[&url], Duration::from_secs(10)).map_err(|error| finish_err(format!("Writer couldn't open the browser: {error}")))?;
    }
    std::thread::spawn(move || {
        let result = wait_for_code(&listener, &state).and_then(|code| {
            token_request(json!({ "grant_type": "authorization_code", "code": code, "code_verifier": verifier, "redirect_uri": redirect, "client_id": CLIENT_ID }))
                .map_err(|failure| match failure {
                    Failure::Refused(why) => format!("accounts.spheceo.com refused the sign-in ({why})."),
                    Failure::Offline(why, wait) if why.starts_with("429") => format!("accounts.spheceo.com is busy; try again in {} s.", wait.as_secs()),
                    Failure::Offline(why, _) => format!("Couldn't reach accounts.spheceo.com ({why})."),
                })
                .and_then(|answer| take(&answer))
        });
        match result {
            Ok(_) => {
                let who = status().person.map(|person| person.email.split('@').next().unwrap_or("").to_string()).unwrap_or_default();
                log::info("account", format!("signed in as {who}"));
                lock().signing_in = false;
                changed();
            }
            Err(why) if why.is_empty() => {}
            Err(why) => {
                log::warn("account", format!("sign-in failed: {why}"));
                let _ = finish_err(why);
            }
        }
    });
    Ok(())
}

fn finish_err(why: String) -> String {
    let mut state = lock();
    state.signing_in = false;
    state.error = Some(why.clone());
    drop(state);
    changed();
    why
}

/// Stops waiting for the browser.
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
    lock().signing_in = false;
    changed();
}

pub fn authorize_url(redirect: &str, state: &str, challenge: &str) -> String {
    let query = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ]
    .iter()
    .map(|(key, value)| format!("{key}={}", encode(value)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{}/app/authorize?{query}", accounts())
}

/// The code the browser brings back to `/callback`, its `state` checked. An
/// empty error means the wait was cancelled (or replaced by a new sign-in).
fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|error| error.to_string())?;
    let began = Instant::now();
    loop {
        if CANCEL.load(Ordering::SeqCst) {
            return Err(String::new());
        }
        if began.elapsed() > WAIT {
            return Err("The sign-in timed out. Try again.".into());
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(answer) = callback(stream, state) {
                    return answer;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(200)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// One request to the listener: `None` for anything but `/callback` (a favicon, say).
fn callback(mut stream: TcpStream, state: &str) -> Option<Result<String, String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;
    let target = line.split_whitespace().nth(1)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/callback" {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return None;
    }
    let params = parse_query(query);
    let get = |key: &str| params.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str());
    let answer = if get("state") != Some(state) {
        Err("The sign-in came back without Writer's check (state). Try again.".to_string())
    } else if let Some(error) = get("error") {
        Err(match (error, get("error_description")) {
            ("access_denied", Some(why)) => why.to_string(),
            ("access_denied", None) => "Your account can't use Writer yet. Ask an admin at accounts.spheceo.com.".to_string(),
            (error, why) => format!("The sign-in failed ({error}{}).", why.map(|why| format!(": {why}")).unwrap_or_default()),
        })
    } else if let Some(code) = get("code").filter(|code| !code.is_empty()) {
        Ok(code.to_string())
    } else {
        Err("The sign-in came back without a code. Try again.".to_string())
    };
    let (title, text) = match &answer {
        Ok(_) => ("Signed in to Writer", "You're signed in. You can close this tab and go back to Writer.".to_string()),
        Err(why) => ("Writer couldn't sign you in", why.clone()),
    };
    let page = format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title><body style=\"font:16px -apple-system,system-ui,sans-serif;max-width:32rem;margin:20vh auto;padding:0 1rem;color:#222\"><h1 style=\"font-size:1.4rem\">{title}</h1><p>{}</p></body>",
        html(&text)
    );
    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}", page.len());
    Some(answer)
}

// ---- Signing out ----

pub fn sign_out() {
    let _one = RENEWING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(refresh) = load_refresh() {
        // The site forgets it; done even if it can't be reached, Writer forgets it too.
        if let Err(wait) = revoke(&refresh) {
            // Told to wait (429) or not answered: once more, later, in the background.
            std::thread::spawn(move || {
                std::thread::sleep(wait);
                if revoke(&refresh).is_err() {
                    log::warn("account", "accounts.spheceo.com still didn't take the sign-out; that sign-in lapses after 30 days unused");
                }
            });
        }
    }
    delete_refresh();
    log::info("account", "signed out");
    signed_out(None);
}

/// Tells accounts to forget a refresh token; on failure, how long to wait before trying again.
fn revoke(refresh: &str) -> Result<(), Duration> {
    match agent().post(&format!("{}/app/revoke", accounts())).set("content-type", "application/json").send_json(json!({ "token": refresh, "client_id": CLIENT_ID })) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, response)) => {
            let retry_after = response.header("retry-after").map(str::to_string);
            log::warn("account", format!("accounts.spheceo.com answered {code} to the sign-out"));
            match classify(code, retry_after.as_deref(), &Value::Null) {
                Failure::Offline(_, wait) if code == 429 || code >= 500 => Err(wait),
                _ => Ok(()),
            }
        }
        Err(ureq::Error::Transport(transport)) => {
            log::warn("account", format!("couldn't tell accounts.spheceo.com about the sign-out: {transport}"));
            Err(OFFLINE_WAIT)
        }
    }
}

// ---- Where the refresh token is kept ----

fn dev_file() -> Option<PathBuf> {
    DATA.get().map(|data| data.join("account-dev.json"))
}

fn load_refresh() -> Option<String> {
    if release::is_release() {
        return security_framework::passwords::get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).ok().and_then(|bytes| String::from_utf8(bytes).ok()).filter(|token| !token.is_empty());
    }
    let text = std::fs::read_to_string(dev_file()?).ok()?;
    serde_json::from_str::<Value>(&text).ok()?["refreshToken"].as_str().filter(|token| !token.is_empty()).map(str::to_string)
}

fn save_refresh(token: &str) -> Result<(), String> {
    if release::is_release() {
        return security_framework::passwords::set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, token.as_bytes()).map_err(|error| format!("Writer couldn't keep the sign-in in the Keychain: {error}"));
    }
    let path = dev_file().ok_or("Writer has no data folder.")?;
    write_private(&path, &json!({ "refreshToken": token }).to_string())
}

fn delete_refresh() {
    if release::is_release() {
        let _ = security_framework::passwords::delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
    } else if let Some(path) = dev_file() {
        let _ = std::fs::remove_file(path);
    }
}

/// Written whole, readable by this user only.
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let temp = path.with_extension(format!("tmp{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp).map_err(|error| error.to_string())?;
    file.write_all(text.as_bytes()).and_then(|_| file.sync_all()).map_err(|error| error.to_string())?;
    std::fs::rename(&temp, path).map_err(|error| error.to_string())
}

// ---- Small helpers ----

/// `bytes` random bytes, as base64url (PKCE's verifier, `state`).
fn random(bytes: usize) -> Result<String, String> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|error| finish_err(format!("Writer couldn't make a random sign-in check: {error}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buffer))
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key), decode(value))
        })
        .collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_for_a_pkce_sign_in() {
        let url = authorize_url("http://127.0.0.1:5123/callback", "st+ate", "chal");
        assert!(url.contains("/app/authorize?response_type=code&"));
        assert!(url.contains("client_id=writer"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5123%2Fcallback"));
        assert!(url.contains("state=st%2Bate"));
        assert!(url.contains("code_challenge=chal&code_challenge_method=S256"));
    }

    #[test]
    fn challenge_is_s256_of_the_verifier() {
        // RFC 7636's own example.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn reads_the_callback_query() {
        let params = parse_query("code=abc%2F1&state=x+y&error_description=Ask%20an%20admin%");
        assert_eq!(params[0], ("code".into(), "abc/1".into()));
        assert_eq!(params[1], ("state".into(), "x y".into()));
        assert_eq!(params[2].1, "Ask an admin%");
    }

    #[test]
    fn says_which_writer_is_signing_in() {
        let agent = user_agent();
        assert!(agent.starts_with("Writer/") && agent.ends_with(" (macOS)"), "{agent}");
    }

    #[test]
    fn only_invalid_grant_ends_a_sign_in() {
        assert!(matches!(classify(400, None, &json!({ "error": "invalid_grant" })), Failure::Refused(_)));
        match classify(429, Some("7"), &json!({ "error": "temporarily_unavailable" })) {
            Failure::Offline(said, wait) => assert_eq!((said.starts_with("429"), wait.as_secs()), (true, 7)),
            other => panic!("{other:?}"),
        }
        assert!(matches!(classify(429, None, &Value::Null), Failure::Offline(_, wait) if wait.as_secs() == 60));
        assert!(matches!(classify(429, Some("soon"), &Value::Null), Failure::Offline(_, wait) if wait.as_secs() == 60));
        assert!(matches!(classify(400, None, &json!({ "error": "invalid_request" })), Failure::Offline(_, wait) if wait == OFFLINE_WAIT));
        assert!(matches!(classify(503, None, &Value::Null), Failure::Offline(..)));
    }

    #[test]
    fn reads_who_from_the_token() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"p1","email":"someone@gmail.com","name":"Someone","perms":["use","ai"],"admin":false}"#);
        let person = claims(&format!("h.{payload}.s"));
        assert_eq!((person.sub.as_str(), person.email.as_str(), person.perms.len(), person.admin), ("p1", "someone@gmail.com", 2, false));
        assert!(claims("not a token").email.is_empty());
    }

    #[test]
    fn answers_the_browser_and_checks_state() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            let get = |path: &str| {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
                let mut page = String::new();
                let _ = std::io::Read::read_to_string(&mut stream, &mut page);
                page
            };
            let favicon = get("/favicon.ico");
            let page = get("/callback?code=the-code&state=right");
            (favicon, page)
        });
        assert_eq!(wait_for_code(&listener, "right").unwrap(), "the-code");
        let (favicon, page) = browser.join().unwrap();
        assert!(favicon.starts_with("HTTP/1.1 404"));
        assert!(page.contains("Signed in to Writer"));

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(stream, "GET /callback?code=c&state=wrong HTTP/1.1\r\n\r\n").unwrap();
            let _ = std::io::Read::read_to_string(&mut stream, &mut String::new());
        });
        assert!(wait_for_code(&listener, "right").unwrap_err().contains("state"));
    }
}
