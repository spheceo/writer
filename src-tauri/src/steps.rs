//! A footage script built in steps, through Anthropic's Agent SDK (agent.rs):
//! Opus reads the same evidence and guides as the one-answer build, then
//! writes the plan with Writer's tools, a section at a time, so no reply has
//! to hold the whole plan (the "ran out of room" a long script can hit).
//!
//! Writer holds the draft here and serves the tools over MCP on its local
//! server (`/mcp/<token>`, a random token per build: nothing else can call
//! them). Each item is checked as it arrives, exactly as the one-answer build
//! checks it (`claude::item_clip`); `review_plan` and `finish` check the whole
//! plan as it will be saved (`claude::compile_with`, the format's targets).
//! Repairs are targeted calls on items by ID, never the whole plan again.
//!
//! A fast build (`build_fast`) goes in three phases on the same tools: Opus
//! outlines the script, writes its sections at once (one call each), then
//! reviews and finishes the joined plan. Every phase reads the same system
//! prompt, tools and evidence, so the sections and the finish read the
//! outline's from the prompt cache.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

use crate::claude::{self, Built, Compiled};
use crate::formats::Format;
use crate::model::Script;
use crate::research::Research;

/// Items one call may add or replace: small replies, whatever the script's length.
const MAX_ITEMS_PER_CALL: usize = 40;
/// Turns before the build stops (each tool call is one).
const MAX_TURNS: u32 = 90;
/// Sections a fast build's outline may have: as many calls at once.
const MAX_SECTIONS: usize = 8;

/// Which part of a build a draft is for: the whole build in steps, or a fast
/// build's outline, one of its sections, or its finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Steps,
    Outline,
    Section,
    Finish,
}

/// A fast build's outline: its sections in timeline order.
#[derive(Debug, Clone)]
struct Outline {
    sections: Vec<Section>,
    opening: Option<String>,
    report: String,
}

#[derive(Debug, Clone)]
struct Section {
    title: String,
    plan: String,
    seconds: u64,
}

struct Draft {
    phase: Phase,
    script: Script,
    format: Format,
    peaks: Vec<f32>,
    /// The plan so far, in timeline order: each item with its ID ("i12").
    items: Vec<(String, Value)>,
    opening: Option<String>,
    next: u32,
    calls: u32,
    refused: u32,
    /// The biggest single tool call's arguments, in bytes: no reply holds more.
    biggest_call: usize,
    /// The plan as saved, once `finish` passes.
    done: Option<(Compiled, String)>,
    /// A plan that passed but missed its target, kept if no better comes.
    best: Option<(Compiled, String, String)>,
    /// `finish` called on an off-target plan: a second one keeps it.
    off_target_finishes: u32,
    /// A fast build's section that a cut-in comes before (every one but the first).
    after_cut_in: bool,
    outline: Option<Outline>,
    /// A section's note for the finish, once `finish_section` is called.
    section_note: Option<String>,
}

impl Draft {
    fn new(phase: Phase, script: &Script, format: Format, peaks: &[f32]) -> Self {
        Draft { phase, script: script.clone(), format, peaks: peaks.to_vec(), items: Vec::new(), opening: None, next: 0, calls: 0, refused: 0, biggest_call: 0, done: None, best: None, off_target_finishes: 0, after_cut_in: false, outline: None, section_note: None }
    }
}

static DRAFTS: Mutex<Option<HashMap<String, Draft>>> = Mutex::new(None);

/// A build is under way with this token.
pub fn exists(token: &str) -> bool {
    with_draft(token, |_| ()).is_some()
}

fn with_draft<T>(token: &str, act: impl FnOnce(&mut Draft) -> T) -> Option<T> {
    let mut drafts = DRAFTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    drafts.get_or_insert_with(HashMap::new).get_mut(token).map(act)
}

/// The system prompt: the one-answer build's, word for word, but for how the
/// plan is written, which the steps section below says.
fn system(script: &Script, format: &Format) -> String {
    let base = claude::system_prompt(script, format)
        .replace("then answer ONCE with the complete plan.", "then write the complete plan with Writer's tools, a section at a time.")
        .replace(
            "Here there are none and you need none: the complete transcript is in the message, your answer is the build_script plan.",
            "Here you have Writer's own tools instead (below): the complete transcript is in the message, and you write the plan with them.",
        )
        .replace(" Also give opening: \"one-run\" or \"two-split\".", " Also set the opening (set_opening): \"one-run\" or \"two-split\".");
    let opening = if format.foia() { " Call set_opening once with how this FOIA script opens." } else { "" };
    format!(
        "{base}\n\nWRITING THE PLAN WITH WRITER'S TOOLS\nWrite the plan in timeline order with add_section: one call per footage block and its cut-in, or two, at most {MAX_ITEMS_PER_CALL} items a call. Each call adds after the last item unless you name `after` (an item ID). Each item is checked as it arrives (its word IDs in order, recording boundaries, narration, em dashes, reused cases); if one is refused, the call adds nothing: fix that item and call again.{opening} When the whole plan is in, call review_plan: it checks the whole script as the app will save it (its shape, the intro replay, the format's rules and length targets) and gives the totals. Fix what it reports with replace_items or remove_items, naming items by ID; never send the whole plan again. Then call finish with your report. Only the tools save the plan: don't write it out in a reply. If finish says the length is off target, fix it once; calling finish again keeps it as it is."
    )
}

/// The tools, as MCP lists them: a fast build's every phase lists the same
/// ones, so they stay in the prompt cache from one phase to the next.
pub fn tools(token: &str) -> Value {
    let mut tools = steps_tools();
    if with_draft(token, |draft| draft.phase != Phase::Steps).unwrap_or(false) {
        let tools = tools.as_array_mut().expect("the tools are a list");
        let section = json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "title": { "type": "string" },
                "plan": { "type": "string", "description": "Enough to write it from without guessing: the moments it uses ([m:ss] and the word IDs where each clip starts and ends), what each cut-in tells and what the viewer already knows by then, what it must not repeat." },
                "footageSeconds": { "type": "integer", "minimum": 0 }
            },
            "required": ["title", "plan", "footageSeconds"]
        });
        tools.push(json!({
            "name": "set_outline",
            "description": "The outline phase only: the whole script's sections in timeline order, each for a writer working at the same time as the others.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": {
                "sections": { "type": "array", "minItems": 1, "maxItems": MAX_SECTIONS, "items": section },
                "opening": { "type": "string", "enum": ["one-run", "two-split"], "description": "FOIA only: how the script opens (as set_opening)." },
                "report": { "type": "string", "description": "For the producer, as finish's report: one to three short sentences, under 80 words. Moments as [m:ss], never word IDs." }
            }, "required": ["sections", "report"] }
        }));
        tools.push(json!({
            "name": "finish_section",
            "description": "A section phase only: your section is in. Give a note for the finish if the joining needs to know something.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": { "note": { "type": "string" } } }
        }));
    }
    tools
}

/// The tools of a build in steps.
fn steps_tools() -> Value {
    let item = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "kind": { "type": "string", "enum": ["clip", "cut-in"] },
            "firstWordId": { "type": "string", "description": "Clips only: the clip's first word, e.g. w12." },
            "lastWordId": { "type": "string", "description": "Clips only: the clip's last word." },
            "narration": { "type": "string", "description": "Cut-ins only: the finished paragraph." },
            "reuseLink": { "type": "string", "description": "Compilations only: on the transition into a reused case, the link to the earlier video. Never written in the narration." }
        },
        "required": ["kind"]
    });
    let items = json!({ "type": "array", "minItems": 1, "maxItems": MAX_ITEMS_PER_CALL, "items": item });
    json!([
        {
            "name": "add_section",
            "description": "Add items (clips and cut-ins) to the plan in timeline order: after the last item, or after the item named. Each item is checked; a refused item means nothing is added.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": { "items": items, "after": { "type": "string", "description": "Add after this item ID (\"start\" for the very beginning); after the last item if left out." } }, "required": ["items"] }
        },
        {
            "name": "replace_items",
            "description": "Replace the items from one ID to another (inclusive) with new items, checked as add_section checks them.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": { "from": { "type": "string" }, "to": { "type": "string" }, "items": items }, "required": ["from", "to", "items"] }
        },
        {
            "name": "remove_items",
            "description": "Remove items by ID.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": { "ids": { "type": "array", "items": { "type": "string" }, "minItems": 1 } }, "required": ["ids"] }
        },
        {
            "name": "set_opening",
            "description": "FOIA only: how the script opens. one-run when one run of intro clips leads to the case opening; two-split when the first clips, the intro cut-in and the rest of the intro clips come before it.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": { "opening": { "type": "string", "enum": ["one-run", "two-split"] } }, "required": ["opening"] }
        },
        {
            "name": "read_plan",
            "description": "The plan so far, item by item with IDs: each clip's times and its first and last words, each cut-in in full.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": {} }
        },
        {
            "name": "review_plan",
            "description": "Check the whole plan as the app will save it (shape, intro replay, the format's rules and length targets) without saving, and give the totals.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": {} }
        },
        {
            "name": "finish",
            "description": "Save the plan: the final check. If it passes the plan is saved and you're done; if not, it says why and nothing is saved.",
            "inputSchema": { "type": "object", "additionalProperties": false, "properties": { "report": { "type": "string", "description": "For the producer: one to three short sentences, under 80 words, naming any judgment call that changed the usual shape or any shortfall. Moments as [m:ss], never word IDs." } }, "required": ["report"] }
        }
    ])
}

/// One tool call: the text to answer with, and whether it's a refusal.
pub fn call(token: &str, name: &str, args: &Value) -> Option<(String, bool)> {
    with_draft(token, |draft| {
        draft.calls += 1;
        draft.biggest_call = draft.biggest_call.max(args.to_string().len());
        let answer = match out_of_phase(draft.phase, name) {
            Some(refusal) => Err(refusal),
            None => match name {
            "add_section" => add(draft, args),
            "replace_items" => replace(draft, args),
            "remove_items" => remove(draft, args),
            "set_opening" => match args["opening"].as_str() {
                Some(opening @ ("one-run" | "two-split")) => {
                    draft.opening = Some(opening.to_string());
                    Ok(format!("Opening set: {opening}."))
                }
                _ => Err("Give opening as one-run or two-split.".to_string()),
            },
            "read_plan" => Ok(read(draft)),
            "review_plan" => Ok(review(draft).0),
            "finish" => finish(draft, args["report"].as_str().unwrap_or("")),
            "set_outline" => set_outline(draft, args),
            "finish_section" => finish_section(draft, args),
            other => Err(format!("Writer has no tool called {other}.")),
            },
        };
        match answer {
            Ok(text) => (text, false),
            Err(text) => {
                draft.refused += 1;
                (text, true)
            }
        }
    })
}

/// Why a tool isn't this phase's, if it isn't.
fn out_of_phase(phase: Phase, name: &str) -> Option<String> {
    let allowed = match name {
        "set_outline" => phase == Phase::Outline,
        "finish_section" => phase == Phase::Section,
        "review_plan" | "finish" | "set_opening" => matches!(phase, Phase::Steps | Phase::Finish),
        _ => phase != Phase::Outline,
    };
    let what = match phase {
        Phase::Steps | Phase::Finish => "review_plan, fix it, then finish",
        Phase::Outline => "set the outline with set_outline, adding no items",
        Phase::Section => "write your section with add_section, then call finish_section",
    };
    (!allowed).then(|| format!("{name} isn't part of this phase: {what}."))
}

fn set_outline(draft: &mut Draft, args: &Value) -> Result<String, String> {
    let given = args["sections"].as_array().filter(|sections| !sections.is_empty()).ok_or("Give at least one section.")?;
    if given.len() > MAX_SECTIONS {
        return Err(format!("At most {MAX_SECTIONS} sections: join some."));
    }
    let mut sections = Vec::new();
    for (index, section) in given.iter().enumerate() {
        let text = |key: &str| section[key].as_str().map(str::trim).unwrap_or("").to_string();
        let (title, plan) = (text("title"), text("plan"));
        if title.is_empty() || plan.is_empty() {
            return Err(format!("Section {} needs a title and a plan.", index + 1));
        }
        sections.push(Section { title, plan, seconds: section["footageSeconds"].as_u64().unwrap_or(0) });
    }
    let opening = match args["opening"].as_str() {
        None | Some("") => None,
        Some(opening @ ("one-run" | "two-split")) => Some(opening.to_string()),
        Some(_) => return Err("Give opening as one-run or two-split.".into()),
    };
    if draft.format.foia() && opening.is_none() {
        return Err("Give the FOIA script's opening: one-run or two-split.".into());
    }
    let report = args["report"].as_str().map(str::trim).unwrap_or("").to_string();
    if report.is_empty() {
        return Err("Give your report for the producer.".into());
    }
    let count = sections.len();
    draft.outline = Some(Outline { sections, opening, report });
    Ok(format!("Outline set: {count} sections. End your turn now."))
}

fn finish_section(draft: &mut Draft, args: &Value) -> Result<String, String> {
    if draft.items.is_empty() {
        return Err("Your section has no items yet: add them with add_section first.".into());
    }
    draft.section_note = Some(args["note"].as_str().map(str::trim).unwrap_or("").to_string());
    Ok("Section saved. End your turn now.".into())
}

/// The items a call brings, each checked where it will sit.
fn checked(draft: &Draft, at: usize, items: &Value) -> Result<Vec<Value>, String> {
    let items = items.as_array().filter(|items| !items.is_empty()).ok_or("Give at least one item.")?;
    if items.len() > MAX_ITEMS_PER_CALL {
        return Err(format!("At most {MAX_ITEMS_PER_CALL} items a call: split it."));
    }
    if draft.items.len() + items.len() > 500 {
        return Err("A plan holds at most 500 items.".into());
    }
    let mut opened = draft.after_cut_in || draft.items[..at].iter().any(|(_, item)| item["kind"] == "cut-in");
    for (index, item) in items.iter().enumerate() {
        let clip = claude::item_clip(&draft.script, &draft.format, item, opened).map_err(|refusal| format!("Item {} of this call was refused, so nothing was added: {refusal}", index + 1))?;
        opened |= clip.is_cut_in();
    }
    Ok(items.clone())
}

fn position(draft: &Draft, id: &str) -> Result<usize, String> {
    draft.items.iter().position(|(known, _)| known == id).ok_or_else(|| format!("There's no item {id} in the plan (read_plan lists them)."))
}

fn insert(draft: &mut Draft, at: usize, items: Vec<Value>) -> String {
    let mut lines = Vec::new();
    for (offset, item) in items.into_iter().enumerate() {
        draft.next += 1;
        let id = format!("i{}", draft.next);
        lines.push(line(&draft.script, &id, &item));
        draft.items.insert(at + offset, (id, item));
    }
    format!("{} added ({} items in the plan now):\n{}", lines.len(), draft.items.len(), lines.join("\n"))
}

fn add(draft: &mut Draft, args: &Value) -> Result<String, String> {
    let at = match args["after"].as_str() {
        None | Some("") => draft.items.len(),
        Some("start") => 0,
        Some(id) => position(draft, id)? + 1,
    };
    let items = checked(draft, at, &args["items"])?;
    Ok(insert(draft, at, items))
}

fn replace(draft: &mut Draft, args: &Value) -> Result<String, String> {
    let from = position(draft, args["from"].as_str().unwrap_or(""))?;
    let to = position(draft, args["to"].as_str().unwrap_or(""))?;
    if to < from {
        return Err("`from` comes after `to`: name the first item, then the last.".into());
    }
    let kept: Vec<(String, Value)> = draft.items.drain(from..=to).collect();
    match checked(draft, from, &args["items"]) {
        Ok(items) => Ok(format!("Replaced {} items. {}", kept.len(), insert(draft, from, items))),
        Err(refusal) => {
            // Nothing changes when a replacement is refused.
            for (offset, item) in kept.into_iter().enumerate() {
                draft.items.insert(from + offset, item);
            }
            Err(refusal)
        }
    }
}

fn remove(draft: &mut Draft, args: &Value) -> Result<String, String> {
    let ids: Vec<String> = args["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    for id in &ids {
        position(draft, id)?;
    }
    draft.items.retain(|(id, _)| !ids.contains(id));
    Ok(format!("Removed {} ({} items in the plan now).", ids.join(", "), draft.items.len()))
}

/// An item as the agent reads it back.
fn line(script: &Script, id: &str, item: &Value) -> String {
    if item["kind"] == "cut-in" {
        let narration = item["narration"].as_str().unwrap_or("").trim();
        let reuse = item["reuseLink"].as_str().map(|link| format!(" (reused case {link})")).unwrap_or_default();
        return format!("{id} cut-in{reuse}, {} words: {narration}", narration.split_whitespace().count());
    }
    let first = claude::word_at(&script.words, item["firstWordId"].as_str());
    let last = claude::word_at(&script.words, item["lastWordId"].as_str());
    match (first, last) {
        (Some(first), Some(last)) if first <= last => {
            let words = &script.words[first..=last];
            let text = |range: &[crate::model::Word]| range.iter().map(|word| word.text.as_str()).collect::<Vec<_>>().join(" ");
            let opening = text(&words[..words.len().min(6)]);
            let ending = text(&words[words.len().saturating_sub(6)..]);
            let (start, end) = (words[0].start_ms, words[words.len() - 1].end_ms);
            format!("{id} clip {}–{} [{}–{}] {} s: \"{opening}{}\"", words[0].id, words[words.len() - 1].id, claude::clock(start), claude::clock(end), (end.saturating_sub(start)) / 1000, if words.len() > 6 { format!(" … {ending}") } else { String::new() })
        }
        _ => format!("{id} clip {} to {}", item["firstWordId"], item["lastWordId"]),
    }
}

fn read(draft: &Draft) -> String {
    if draft.items.is_empty() {
        return "The plan is empty.".into();
    }
    let opening = draft.opening.as_deref().map(|opening| format!("Opening: {opening}.\n")).unwrap_or_default();
    format!("{opening}{}", draft.items.iter().map(|(id, item)| line(&draft.script, id, item)).collect::<Vec<_>>().join("\n"))
}

fn plan(draft: &Draft, report: &str) -> Value {
    let mut plan = json!({ "items": draft.items.iter().map(|(_, item)| item.clone()).collect::<Vec<_>>(), "report": report });
    if let Some(opening) = &draft.opening {
        plan["opening"] = json!(opening);
    }
    plan
}

/// The whole plan checked as it will be saved: what to say, and the compiled
/// cut and how it misses its target if it passed.
fn review(draft: &Draft) -> (String, Option<(Compiled, Option<String>)>) {
    if draft.items.is_empty() {
        return ("The plan is empty: add the sections first.".into(), None);
    }
    match claude::compile_with(&draft.script, &plan(draft, ""), &draft.peaks) {
        Err(refusal) => (format!("Not yet: {refusal}"), None),
        Ok(done) => {
            let footage: u64 = done.clips.iter().filter(|clip| !clip.is_cut_in()).map(|clip| clip.end_ms.saturating_sub(clip.start_ms)).sum();
            let cut_ins = done.clips.iter().filter(|clip| clip.is_cut_in()).count();
            let words: usize = done.clips.iter().filter_map(|clip| clip.narration.as_deref()).map(|text| text.split_whitespace().count()).sum();
            let pages = crate::pages::count(&draft.script, &done.clips);
            let miss = claude::off_target(&draft.script, &draft.format, &done);
            let totals = format!("{} of footage in {} clips, {cut_ins} cut-ins ({words} words), about {pages} Letter pages. {}", claude::clock(footage), done.clips.len() - cut_ins, claude::cleanup_line(&done));
            let said = match &miss {
                None => format!("It passes: {totals}"),
                Some(miss) => format!("It passes, but off target: {miss} {totals}"),
            };
            (said, Some((done, miss)))
        }
    }
}

fn finish(draft: &mut Draft, report: &str) -> Result<String, String> {
    let report = report.trim();
    if report.is_empty() {
        return Err("Give your report for the producer.".into());
    }
    let (said, outcome) = review(draft);
    let Some((done, miss)) = outcome else { return Err(said) };
    let saved = claude::report(&plan(draft, report));
    match miss {
        None => {
            draft.done = Some((done, saved));
            Ok("Saved. The script is done: end your turn now.".into())
        }
        Some(miss) => {
            draft.off_target_finishes += 1;
            if draft.off_target_finishes >= 2 {
                draft.best = Some((done, saved, miss));
                return Ok("Kept as it is, off target. The script is done: end your turn now.".into());
            }
            draft.best = Some((done, saved, miss.clone()));
            Err(format!("{said} Fix the length with replace_items or remove_items, then call finish again (a second finish keeps it as it is)."))
        }
    }
}

/// The request after the evidence, once the note is checked.
fn request(script: &Script, note: &str) -> Result<String, String> {
    if script.words.is_empty() {
        return Err("Transcribe the footage before building the script.".into());
    }
    let note = note.trim();
    if note.chars().count() > 2_000 {
        return Err("Keep the note under 2,000 characters.".into());
    }
    Ok(if note.is_empty() { "Start the scripts workflow.".to_string() } else { format!("Start the scripts workflow. {note}") })
}

/// A draft served under a new random token.
fn open(draft: Draft) -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    DRAFTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get_or_insert_with(HashMap::new).insert(token.clone(), draft);
    Ok(token)
}

fn close(token: &str) -> Option<Draft> {
    DRAFTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get_or_insert_with(HashMap::new).remove(token)
}

/// One Opus turn on a draft's tools, asked again once if it failed before any tool call.
fn ask(system: &str, prompt: &str, token: &str, effort: Option<&str>, max_turns: u32, label: &str) -> Result<crate::agent::Answer, claude::Trouble> {
    let url = format!("http://127.0.0.1:{}/mcp/{token}", crate::http::PORT);
    let asked = || {
        crate::agent::ask(
            crate::agent::Question { model: crate::claude::OPUS, system, prompt, schema: None, resume: None, read: None, tools: Some(&url), max_turns: Some(max_turns), effort, timeout: Duration::from_secs(40 * 60), label: label.into() },
            &crate::agent::workspace(),
        )
    };
    match asked() {
        Err(claude::Trouble::Again(_)) if with_draft(token, |draft| draft.calls == 0).unwrap_or(false) => {
            std::thread::sleep(Duration::from_secs(60));
            asked()
        }
        other => other,
    }
}

fn output_tokens(answer: &Result<crate::agent::Answer, claude::Trouble>) -> u64 {
    answer.as_ref().ok().and_then(|answer| answer.result["usage"]["output_tokens"].as_u64()).unwrap_or(0)
}

fn turns(answer: &Result<crate::agent::Answer, claude::Trouble>) -> u64 {
    answer.as_ref().ok().and_then(|answer| answer.result["num_turns"].as_u64()).unwrap_or(0)
}

/// Builds the script in steps. Nothing is saved to it unless a plan passes.
pub fn build(script: &Script, note: &str, research: Option<&Research>, peaks: &[f32]) -> Result<Built, String> {
    let request = request(script, note)?;
    let format = Format::new(script.kind, script.compilation, &script.title);
    let system = system(script, &format);
    let prompt = format!("{}\n\n{request}", claude::evidence(script, research));
    let token = open(Draft::new(Phase::Steps, script, format, peaks))?;
    let answer = ask(&system, &prompt, &token, None, MAX_TURNS, "Claude opus building in steps");
    let Some(draft) = close(&token) else { return Err("Writer lost track of the build. Build again; nothing was changed.".into()) };
    crate::log::info("agent", format!("steps build: {} tool calls, {} refused, the biggest {}, {} items in the draft, {}", draft.calls, draft.refused, crate::log::size(draft.biggest_call as u64), draft.items.len(), outcome_of(&draft)));
    let cost = answer.as_ref().map(|answer| answer.cost_usd).unwrap_or(0.0);
    let stats = json!({ "engine": "sdk", "toolCalls": draft.calls, "refused": draft.refused, "biggestCallBytes": draft.biggest_call, "outputTokens": output_tokens(&answer), "turns": turns(&answer) });
    built(draft, answer, cost, stats)
}

fn outcome_of(draft: &Draft) -> &'static str {
    if draft.done.is_some() {
        "finished"
    } else if draft.best.is_some() {
        "kept off target"
    } else {
        "not finished"
    }
}

/// What a finished draft built: the plan that passed, else the one kept off target, else why not.
fn built(draft: Draft, answer: Result<crate::agent::Answer, claude::Trouble>, cost: f64, stats: Value) -> Result<Built, String> {
    let planned = plan(&draft, "");
    if let Some((done, report)) = draft.done {
        return Ok(Built { report: format!("{report}\n\n{}", claude::cleanup_line(&done)), clips: done.clips, cost_usd: cost, plan: planned, stats: stats.clone() });
    }
    if let Some((done, report, miss)) = draft.best {
        return Ok(Built { report: format!("{report}\n\n{}\n\n{miss}", claude::cleanup_line(&done)), clips: done.clips, cost_usd: cost, plan: planned, stats });
    }
    match answer {
        Err(claude::Trouble::Again(reason) | claude::Trouble::Stop(reason)) => Err(reason),
        Ok(answer) => Err(claude::stopped_short(&answer.result).map(|trouble| match trouble {
            claude::Trouble::Again(reason) | claude::Trouble::Stop(reason) => reason,
        }).unwrap_or_else(|| "Claude stopped before the plan was finished. Build again; nothing was changed.".into())),
    }
}

/// A fast build asked for (`WRITER_BUILD=fast`), until comparisons settle it.
pub fn fast_chosen() -> bool {
    std::env::var("WRITER_BUILD").is_ok_and(|value| value == "fast")
}

/// How hard a fast build's Opus thinks, the same in every phase so the cache
/// holds: `WRITER_FAST_EFFORT`, or medium.
fn fast_effort() -> &'static str {
    match std::env::var("WRITER_FAST_EFFORT").as_deref() {
        Ok("low") => "low",
        Ok("high") => "high",
        Ok("xhigh") => "xhigh",
        Ok("max") => "max",
        _ => "medium",
    }
}

/// The build in steps' system prompt and how a fast build's phases go.
fn fast_system(script: &Script, format: &Format) -> String {
    format!(
        "{}\n\nA FAST BUILD\nThis build goes in three phases, each its own turn with these same tools, and the end of the message says which is yours. The outline: plan the whole script and set it with set_outline, adding no items. A section: write only your section's items with add_section (fixing them with replace_items or remove_items), then call finish_section. The finish: the sections are joined into the plan; review it, fix it and finish it as above.",
        system(script, format)
    )
}

fn outline_list(outline: &Outline) -> String {
    outline.sections.iter().enumerate().map(|(index, section)| format!("{}. {} (about {} s of footage): {}", index + 1, section.title, section.seconds, section.plan)).collect::<Vec<_>>().join("\n")
}

fn outline_phase() -> String {
    format!(
        "YOUR PHASE: THE OUTLINE\nPlan the whole script as you would write it: which moments, in what order, the intro, what each cut-in tells, the format's length targets. Then split it, in timeline order, into 2 to {MAX_SECTIONS} sections of about equal work for writers working at the same time, each seeing only this evidence and your outline: the first holds the opening (the intro clips and the intro cut-in), the last the ending (the outro); a compilation's case is one section, or two when it's long. For each give a title; its plan, specific enough to write from without guessing (the moments it uses as [m:ss] and the word IDs where each clip starts and ends, what each cut-in tells and what the viewer already knows by then, what it must not repeat); and its footage in seconds, so the sections add up to the format's target. Call set_outline once, then end your turn."
    )
}

fn section_phase(outline: &Outline, index: usize) -> String {
    let count = outline.sections.len();
    let place = match (index, index + 1 == count) {
        (0, true) => "It is the whole script: it opens with the intro clips and ends with the outro.".to_string(),
        (0, false) => "It opens the script, with the intro clips; section 2's items come after yours.".to_string(),
        (_, true) => format!("It ends the script, with the outro; it comes after section {index}'s items, so a cut-in has already come before it."),
        _ => format!("It comes after section {index}'s items and before section {}'s, so a cut-in has already come before it.", index + 2),
    };
    format!(
        "YOUR PHASE: SECTION {} OF {count}\nThe outline, written from this evidence:\n{}\n\nYou write section {} only: {}. {place} The other sections are written at the same time by writers who don't see yours: keep to your plan's moments and length, and don't tell what another section tells. Write your items in timeline order with add_section. When they're in, call finish_section, with a note if the joining needs to know something, then end your turn.",
        index + 1,
        outline_list(outline),
        index + 1,
        outline.sections[index].title
    )
}

fn finish_phase(outline: &Outline, notes: &[String], missing: &[usize]) -> String {
    let notes: Vec<String> = notes.iter().enumerate().filter(|(_, note)| !note.is_empty()).map(|(index, note)| format!("- Section {}: {note}", index + 1)).collect();
    let notes = if notes.is_empty() { String::new() } else { format!("\n\nThe sections' notes:\n{}", notes.join("\n")) };
    let missing = if missing.is_empty() {
        String::new()
    } else {
        let names = missing.iter().map(|index| (index + 1).to_string()).collect::<Vec<_>>().join(", ");
        format!("\n\nSection {names} came back empty: write it first, with add_section after the item before it.")
    };
    format!(
        "YOUR PHASE: THE FINISH\nThe sections were written at once from this outline and joined in order; the whole plan is in (read_plan lists it).\n{}{notes}{missing}\n\nCall review_plan and fix what it reports by ID. Read the cut-ins where the sections meet: their writers didn't see each other's words, so cut a fact told twice and make each one follow on from what came before. Then call finish with the outline's report, corrected if the plan changed: {}",
        outline_list(outline),
        outline.report
    )
}

/// Builds the script fast: an outline, its sections written at once, then a
/// finish on the joined plan. Without an outline, it builds in steps instead.
/// Nothing is saved to the script unless a plan passes.
pub fn build_fast(script: &Script, note: &str, research: Option<&Research>, peaks: &[f32]) -> Result<Built, String> {
    let request = request(script, note)?;
    let format = Format::new(script.kind, script.compilation, &script.title);
    let system = fast_system(script, &format);
    let base = format!("{}\n\n{request}", claude::evidence(script, research));
    let effort = fast_effort();
    let began = std::time::Instant::now();
    let (mut cost, mut tokens, mut turned, mut calls, mut refused, mut biggest) = (0.0, 0, 0, 0, 0, 0);
    let mut count = |answer: &Result<crate::agent::Answer, claude::Trouble>, draft: Option<&Draft>| {
        cost += answer.as_ref().map(|answer| answer.cost_usd).unwrap_or(0.0);
        tokens += output_tokens(answer);
        turned += turns(answer);
        if let Some(draft) = draft {
            calls += draft.calls;
            refused += draft.refused;
            biggest = biggest.max(draft.biggest_call);
        }
    };

    let token = open(Draft::new(Phase::Outline, script, format, peaks))?;
    let answer = ask(&system, &format!("{base}\n\n{}", outline_phase()), &token, Some(effort), 12, "Claude opus outlining");
    let drafted = close(&token);
    count(&answer, drafted.as_ref());
    let Some(outline) = drafted.and_then(|draft| draft.outline) else {
        crate::log::info("agent", "fast build: no outline came back; building in steps instead");
        return build(script, note, research, peaks);
    };
    let outlined = began.elapsed();

    // The sections at once, each on its own draft.
    let sections: Vec<(Option<Draft>, Result<crate::agent::Answer, claude::Trouble>)> = std::thread::scope(|scope| {
        let running: Vec<_> = (0..outline.sections.len())
            .map(|index| {
                let (system, base, outline) = (&system, &base, &outline);
                scope.spawn(move || {
                    let mut draft = Draft::new(Phase::Section, script, format, peaks);
                    draft.after_cut_in = index > 0;
                    let token = match open(draft) {
                        Ok(token) => token,
                        Err(error) => return (None, Err(claude::Trouble::Stop(error))),
                    };
                    let answer = ask(system, &format!("{base}\n\n{}", section_phase(outline, index)), &token, Some(effort), 40, &format!("Claude opus writing section {} of {}", index + 1, outline.sections.len()));
                    (close(&token), answer)
                })
            })
            .collect();
        running.into_iter().map(|section| section.join().unwrap_or_else(|_| (None, Err(claude::Trouble::Stop("A section's writer stopped.".into()))))).collect()
    });
    let written = began.elapsed();

    // Joined in the outline's order, with new IDs.
    let mut joined = Draft::new(Phase::Finish, script, format, peaks);
    joined.opening = outline.opening.clone();
    let (mut notes, mut missing) = (Vec::new(), Vec::new());
    for (index, (draft, answer)) in sections.into_iter().enumerate() {
        count(&answer, draft.as_ref());
        let draft = draft.filter(|draft| !draft.items.is_empty());
        if draft.is_none() {
            missing.push(index);
        }
        notes.push(draft.as_ref().and_then(|draft| draft.section_note.clone()).unwrap_or_default());
        for (_, item) in draft.map(|draft| draft.items).unwrap_or_default() {
            joined.next += 1;
            joined.items.push((format!("i{}", joined.next), item));
        }
    }
    crate::log::info("agent", format!("fast build: outline in {}, {} sections in {} ({} empty), {} items joined", crate::log::took(outlined), outline.sections.len(), crate::log::took(written - outlined), missing.len(), joined.items.len()));

    let token = open(joined)?;
    let answer = ask(&system, &format!("{base}\n\n{}", finish_phase(&outline, &notes, &missing)), &token, Some(effort), MAX_TURNS, "Claude opus finishing");
    let Some(draft) = close(&token) else { return Err("Writer lost track of the build. Build again; nothing was changed.".into()) };
    count(&answer, Some(&draft));
    let finished = began.elapsed();
    crate::log::info("agent", format!("fast build: finished in {}, {} tool calls in all, {} refused, the biggest {}, {} items in the draft, {}", crate::log::took(finished - written), calls, refused, crate::log::size(biggest as u64), draft.items.len(), outcome_of(&draft)));
    let stats = json!({
        "engine": "fast", "effort": effort, "sections": outline.sections.len(), "emptySections": missing.len(),
        "toolCalls": calls, "refused": refused, "biggestCallBytes": biggest, "outputTokens": tokens, "turns": turned,
        "outlineSeconds": outlined.as_secs_f64(), "sectionsSeconds": (written - outlined).as_secs_f64(), "finishSeconds": (finished - written).as_secs_f64(),
    });
    built(draft, answer, cost, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Word;

    fn script() -> Script {
        let words: Vec<Word> = (0..40)
            .map(|index| Word { id: format!("w{index}"), text: if index % 5 == 4 { "end.".into() } else { format!("word{index}") }, start_ms: index * 1000, end_ms: index * 1000 + 800, speaker: None })
            .collect();
        let mut script: Script = serde_json::from_value(json!({
            "id": "s1", "title": "A case", "sourceKind": "youtube", "sourceUrl": "", "originalPath": "", "status": "ready", "stage": "", "progress": 100, "error": null, "durationMs": 40_000, "revision": 1, "createdAt": 0, "updatedAt": 0
        }))
        .unwrap();
        script.words = words;
        script
    }

    fn start(token: &str) {
        let script = script();
        let format = Format::new(script.kind, script.compilation, &script.title);
        DRAFTS.lock().unwrap().get_or_insert_with(HashMap::new).insert(token.into(), Draft::new(Phase::Steps, &script, format, &[]));
    }

    #[test]
    fn checks_each_item_as_it_arrives_and_adds_nothing_on_a_refusal() {
        start("t1");
        let (said, refused) = call("t1", "add_section", &json!({ "items": [{ "kind": "clip", "firstWordId": "w0", "lastWordId": "w4" }, { "kind": "cut-in", "narration": "An intro — with a dash." }] })).unwrap();
        assert!(refused && said.contains("Item 2") && said.contains("em dash"), "{said}");
        let (said, _) = call("t1", "read_plan", &json!({})).unwrap();
        assert_eq!(said, "The plan is empty.");
        let (said, refused) = call("t1", "add_section", &json!({ "items": [{ "kind": "clip", "firstWordId": "w0", "lastWordId": "w4" }, { "kind": "cut-in", "narration": "An intro." }] })).unwrap();
        assert!(!refused && said.contains("i1 clip w0–w4") && said.contains("i2 cut-in"), "{said}");
        let (said, refused) = call("t1", "add_section", &json!({ "items": [{ "kind": "clip", "firstWordId": "w9", "lastWordId": "w3" }] })).unwrap();
        assert!(refused && said.contains("Invalid clip range"), "{said}");
    }

    #[test]
    fn repairs_by_id_and_finishes_once_the_whole_plan_passes() {
        start("t2");
        call("t2", "add_section", &json!({ "items": [{ "kind": "clip", "firstWordId": "w0", "lastWordId": "w4" }, { "kind": "cut-in", "narration": "An intro." }] })).unwrap();
        // No outro yet: the whole-plan check says so, and nothing is saved.
        let (said, refused) = call("t2", "finish", &json!({ "report": "Done." })).unwrap();
        assert!(refused && said.contains("outro"), "{said}");
        call("t2", "add_section", &json!({ "items": [{ "kind": "clip", "firstWordId": "w5", "lastWordId": "w9" }, { "kind": "cut-in", "narration": "A middle." }, { "kind": "clip", "firstWordId": "w0", "lastWordId": "w9" }, { "kind": "cut-in", "narration": "The outro." }] })).unwrap();
        let (said, refused) = call("t2", "replace_items", &json!({ "from": "i4", "to": "i4", "items": [{ "kind": "cut-in", "narration": "A better middle." }] })).unwrap();
        assert!(!refused && said.contains("Replaced 1"), "{said}");
        let (said, refused) = call("t2", "review_plan", &json!({})).unwrap();
        assert!(!refused && said.starts_with("It passes"), "{said}");
        let (said, refused) = call("t2", "finish", &json!({ "report": "A short case." })).unwrap();
        assert!(!refused && said.starts_with("Saved"), "{said}");
        assert!(with_draft("t2", |draft| draft.done.is_some()).unwrap());
    }

    #[test]
    fn a_refused_replacement_changes_nothing() {
        start("t3");
        call("t3", "add_section", &json!({ "items": [{ "kind": "clip", "firstWordId": "w0", "lastWordId": "w4" }, { "kind": "cut-in", "narration": "An intro." }] })).unwrap();
        let (_, refused) = call("t3", "replace_items", &json!({ "from": "i1", "to": "i2", "items": [{ "kind": "cut-in", "narration": "" }] })).unwrap();
        assert!(refused);
        let (said, _) = call("t3", "read_plan", &json!({})).unwrap();
        assert!(said.contains("i1 clip") && said.contains("i2 cut-in"), "{said}");
    }

    fn start_in(token: &str, phase: Phase) {
        let script = script();
        let format = Format::new(script.kind, script.compilation, &script.title);
        DRAFTS.lock().unwrap().get_or_insert_with(HashMap::new).insert(token.into(), Draft::new(phase, &script, format, &[]));
    }

    #[test]
    fn a_fast_builds_phases_each_take_only_their_own_tools() {
        start_in("f1", Phase::Outline);
        let (said, refused) = call("f1", "add_section", &json!({ "items": [{ "kind": "clip", "firstWordId": "w0", "lastWordId": "w4" }] })).unwrap();
        assert!(refused && said.contains("set_outline"), "{said}");
        let (said, refused) = call("f1", "set_outline", &json!({ "sections": [{ "title": "Opening", "plan": "w0 to w4, then the intro.", "footageSeconds": 5 }, { "title": "Ending", "plan": "w5 to w9, then the outro.", "footageSeconds": 5 }], "report": "Short." })).unwrap();
        assert!(!refused && said.contains("2 sections"), "{said}");
        assert_eq!(with_draft("f1", |draft| draft.outline.as_ref().map(|outline| outline.sections.len())).unwrap(), Some(2));

        start_in("f2", Phase::Section);
        let (said, refused) = call("f2", "finish_section", &json!({})).unwrap();
        assert!(refused && said.contains("no items"), "{said}");
        let (_, refused) = call("f2", "finish", &json!({ "report": "Done." })).unwrap();
        assert!(refused);
        call("f2", "add_section", &json!({ "items": [{ "kind": "clip", "firstWordId": "w0", "lastWordId": "w4" }, { "kind": "cut-in", "narration": "An intro." }] })).unwrap();
        let (said, refused) = call("f2", "finish_section", &json!({ "note": "Ends on the arrest." })).unwrap();
        assert!(!refused && said.starts_with("Section saved"), "{said}");

        // Only a fast build lists the fast tools, and every phase the same ones.
        let names = |token: &str| tools(token).as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert!(names("f1").contains(&"set_outline".to_string()) && names("f1") == names("f2"));
        start("f3");
        assert!(!names("f3").contains(&"set_outline".to_string()));
    }

    #[test]
    fn a_section_after_the_first_writes_as_if_a_cut_in_came_before() {
        let outline = Outline { sections: vec![Section { title: "Opening".into(), plan: "The intro.".into(), seconds: 60 }, Section { title: "Ending".into(), plan: "The outro.".into(), seconds: 60 }], opening: None, report: "Short.".into() };
        assert!(section_phase(&outline, 0).contains("opens the script"));
        assert!(section_phase(&outline, 1).contains("ends the script") && section_phase(&outline, 1).contains("a cut-in has already come before it"));
        assert!(finish_phase(&outline, &["".into(), "".into()], &[1]).contains("Section 2 came back empty"));
    }

    #[test]
    fn the_prompt_asks_for_tools_not_one_answer() {
        let script = script();
        let format = Format::new(script.kind, script.compilation, &script.title);
        let prompt = system(&script, &format);
        assert!(!prompt.contains("answer ONCE"));
        assert!(prompt.contains("WRITING THE PLAN WITH WRITER'S TOOLS"));
        assert!(!prompt.contains("your answer is the build_script plan"));
    }
}
