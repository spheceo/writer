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

struct Draft {
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

/// The tools, as MCP lists them.
pub fn tools() -> Value {
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
        let answer = match name {
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
            other => Err(format!("Writer has no tool called {other}.")),
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

/// The items a call brings, each checked where it will sit.
fn checked(draft: &Draft, at: usize, items: &Value) -> Result<Vec<Value>, String> {
    let items = items.as_array().filter(|items| !items.is_empty()).ok_or("Give at least one item.")?;
    if items.len() > MAX_ITEMS_PER_CALL {
        return Err(format!("At most {MAX_ITEMS_PER_CALL} items a call: split it."));
    }
    if draft.items.len() + items.len() > 500 {
        return Err("A plan holds at most 500 items.".into());
    }
    let mut opened = draft.items[..at].iter().any(|(_, item)| item["kind"] == "cut-in");
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

/// Builds the script in steps. Nothing is saved to it unless a plan passes.
pub fn build(script: &Script, note: &str, research: Option<&Research>, peaks: &[f32]) -> Result<Built, String> {
    if script.words.is_empty() {
        return Err("Transcribe the footage before building the script.".into());
    }
    let note = note.trim();
    if note.chars().count() > 2_000 {
        return Err("Keep the note under 2,000 characters.".into());
    }
    let format = Format::new(script.kind, script.compilation, &script.title);
    let system = system(script, &format);
    let request = if note.is_empty() { "Start the scripts workflow.".to_string() } else { format!("Start the scripts workflow. {note}") };
    let prompt = format!("{}\n\n{request}", claude::evidence(script, research));
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    {
        let mut drafts = DRAFTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        drafts.get_or_insert_with(HashMap::new).insert(
            token.clone(),
            Draft { script: script.clone(), format, peaks: peaks.to_vec(), items: Vec::new(), opening: None, next: 0, calls: 0, refused: 0, biggest_call: 0, done: None, best: None, off_target_finishes: 0 },
        );
    }
    let url = format!("http://127.0.0.1:{}/mcp/{token}", crate::http::PORT);
    let asked = || {
        crate::agent::ask(
            crate::agent::Question { model: crate::claude::OPUS, system: &system, prompt: &prompt, schema: None, resume: None, read: None, tools: Some(&url), max_turns: Some(MAX_TURNS), timeout: Duration::from_secs(40 * 60), label: "Claude opus building in steps".into() },
            &crate::agent::workspace(),
        )
    };
    let answer = match asked() {
        Err(claude::Trouble::Again(_)) if with_draft(&token, |draft| draft.calls == 0).unwrap_or(false) => {
            std::thread::sleep(Duration::from_secs(60));
            asked()
        }
        other => other,
    };
    let draft = DRAFTS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get_or_insert_with(HashMap::new).remove(&token);
    let Some(draft) = draft else { return Err("Writer lost track of the build. Build again; nothing was changed.".into()) };
    crate::log::info("agent", format!("steps build: {} tool calls, {} refused, the biggest {}, {} items in the draft, {}", draft.calls, draft.refused, crate::log::size(draft.biggest_call as u64), draft.items.len(), if draft.done.is_some() { "finished" } else if draft.best.is_some() { "kept off target" } else { "not finished" }));
    let cost = answer.as_ref().map(|answer| answer.cost_usd).unwrap_or(0.0);
    let stats = json!({ "engine": "sdk", "toolCalls": draft.calls, "refused": draft.refused, "biggestCallBytes": draft.biggest_call, "outputTokens": answer.as_ref().ok().and_then(|answer| answer.result["usage"]["output_tokens"].as_u64()).unwrap_or(0), "turns": answer.as_ref().ok().and_then(|answer| answer.result["num_turns"].as_u64()).unwrap_or(0) });
    if let Some((done, report)) = draft.done {
        return Ok(Built { report: format!("{report}\n\n{}", claude::cleanup_line(&done)), clips: done.clips, cost_usd: cost, stats: stats.clone() });
    }
    if let Some((done, report, miss)) = draft.best {
        return Ok(Built { report: format!("{report}\n\n{}\n\n{miss}", claude::cleanup_line(&done)), clips: done.clips, cost_usd: cost, stats });
    }
    match answer {
        Err(claude::Trouble::Again(reason) | claude::Trouble::Stop(reason)) => Err(reason),
        Ok(answer) => Err(claude::stopped_short(&answer.result).map(|trouble| match trouble {
            claude::Trouble::Again(reason) | claude::Trouble::Stop(reason) => reason,
        }).unwrap_or_else(|| "Claude stopped before the plan was finished. Build again; nothing was changed.".into())),
    }
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
        DRAFTS.lock().unwrap().get_or_insert_with(HashMap::new).insert(token.into(), Draft { script, format, peaks: Vec::new(), items: Vec::new(), opening: None, next: 0, calls: 0, refused: 0, biggest_call: 0, done: None, best: None, off_target_finishes: 0 });
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
