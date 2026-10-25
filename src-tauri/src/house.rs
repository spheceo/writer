//! What Writer's house rules (guides/house.md) ask of a script's narration
//! that a reading can find: outros short and final, cut-ins in their length,
//! no addresses, care with harm and children, no narrated flashbacks,
//! varied transitions, dated case openings, and people introduced by full
//! name before an outro. One line each, said as the team's notes say it.
//! The footage checks (intro clips) are with the team's in `claude::team_flags`.

use crate::model::{Clip, Script};

/// An outro's most words: one or two sentences.
pub const OUTRO_WORDS: usize = 45;
/// A FOIA cut-in's most words, and a case opening's.
pub const CUT_IN_WORDS: usize = 65;

const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
const STREETS: [&str; 11] = ["street", "avenue", "ave", "lane", "ln", "road", "rd", "drive", "boulevard", "blvd", "parkway"];
const BUSINESSES: [&str; 5] = ["inn", "motel", "hotel", "school", "elementary"];
const NUMBERED: [&str; 5] = ["room", "apartment", "apt", "unit", "suite"];
const CARE: [&str; 13] = ["pornography", "porn", "abuse", "abused", "abusing", "rape", "raped", "raping", "sexual", "sexually", "molestation", "molested", "exploitation"];
const YOUNG: [&str; 4] = ["child", "children", "minor", "minors"];
const BLOWS: [&str; 14] = ["punched", "punching", "punch", "slapped", "slapping", "slap", "kicked", "kicking", "choked", "choking", "strangled", "strangling", "beaten", "stomped"];
const TELLING: [&str; 7] = ["to understand how", "to understand why", "it helps to go back", "let's go back", "let us go back", "let's rewind", "rewind to"];
/// A pair of capitalised words ending in one of these is a place or body, not a person.
const NOT_PEOPLE: [&str; 22] = [
    "county", "city", "jail", "court", "police", "department", "office", "state", "prison", "school", "hospital", "university", "inn", "sheriff", "sheriff's",
    "township", "district", "center", "centre", "unit", "division", "attorney",
];

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_string()).filter(|word| !word.is_empty()).collect()
}

fn lower(words: &[String]) -> Vec<String> {
    words.iter().map(|word| word.to_lowercase()).collect()
}

fn capital(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

fn quote(text: &str) -> String {
    let short: String = text.chars().take(60).collect();
    if short.len() < text.len() { format!("\"{short}…\"") } else { format!("\"{short}\"") }
}

/// An address, a room or a named school, hotel or business in a cut-in.
fn places(text: &str) -> Vec<String> {
    let said = words(text);
    let low = lower(&said);
    let mut found = Vec::new();
    for index in 1..said.len() {
        let word = low[index].trim_end_matches('.');
        let before = &said[index - 1];
        if STREETS.contains(&word) && capital(before) && capital(&said[index]) {
            let start = if index >= 2 && capital(&said[index - 2]) { index - 2 } else { index - 1 };
            found.push(said[start..=index].join(" "));
        }
        if BUSINESSES.contains(&word) && capital(&said[index]) && capital(before) && !NOT_PEOPLE.contains(&low[index - 1].as_str()) {
            let start = if index >= 2 && capital(&said[index - 2]) && low[index - 2] != "the" { index - 2 } else { index - 1 };
            found.push(said[start..=index].join(" "));
        }
        if NUMBERED.contains(&low[index - 1].as_str()) && said[index].chars().all(|c| c.is_ascii_digit() || c.is_ascii_uppercase()) && said[index].chars().any(|c| c.is_ascii_digit()) {
            found.push(format!("{} {}", said[index - 1], said[index]));
        }
    }
    found
}

/// "child pornography", "abuse of a child": a care word within two words of a child.
fn child_pairs(text: &str) -> Vec<String> {
    let low = lower(&words(text));
    let mut found = Vec::new();
    for (index, word) in low.iter().enumerate() {
        if !YOUNG.contains(&word.as_str()) {
            continue;
        }
        let from = index.saturating_sub(3);
        let to = (index + 3).min(low.len());
        if low[from..to].iter().any(|near| CARE.contains(&near.as_str())) {
            found.push(low[from..to].join(" "));
        }
    }
    found
}

fn care_words(text: &str) -> usize {
    lower(&words(text)).iter().filter(|word| CARE.contains(&word.as_str())).count()
}

/// Harm told by the blow: "hit in the head", "punched her".
fn blows(text: &str) -> Vec<String> {
    let low = lower(&words(text));
    let mut found = Vec::new();
    for (index, word) in low.iter().enumerate() {
        let next = low.get(index + 1).map(String::as_str).unwrap_or("");
        // "beating her", never a heart "beating again".
        let hit = matches!(word.as_str(), "hit" | "hitting" | "struck" | "beating" | "beat") && matches!(next, "in" | "her" | "him" | "them" | "the" | "his" | "their");
        if hit || BLOWS.contains(&word.as_str()) {
            found.push(low[index..(index + 3).min(low.len())].join(" "));
        }
    }
    found
}

/// A month or a year: what a case opening's date needs.
fn has_date(text: &str) -> bool {
    lower(&words(text)).iter().any(|word| MONTHS.contains(&word.as_str()) || (word.len() == 4 && (word.starts_with("19") || word.starts_with("20")) && word.chars().all(|c| c.is_ascii_digit())))
}

/// The people the footage names, by full name: the transcript's speakers and named suspects.
fn people(script: &Script) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    for name in script.speakers.values().chain(script.suspects.iter()) {
        let name = name.split('(').next().unwrap_or("").trim();
        let parts: Vec<String> = name.split_whitespace().map(str::to_string).collect();
        if parts.len() >= 2 && parts.iter().all(|part| capital(part)) && !NOT_PEOPLE.contains(&parts.last().map(|last| last.to_lowercase()).unwrap_or_default().as_str()) {
            let first = parts[0].to_lowercase();
            if ["deputy", "officer", "detective", "sergeant", "investigator", "trooper", "lieutenant", "dispatcher", "speaker", "narrator", "unknown"].contains(&first.as_str()) {
                continue;
            }
            if !out.contains(&parts) {
                out.push(parts);
            }
        }
    }
    out
}

/// The flags, one line each. `purposes` is each cut-in's, in order (`claude::purposes`).
pub fn text_flags(script: &Script, clips: &[Clip], purposes: &[&str], compilation: bool) -> Vec<String> {
    let cut_ins: Vec<(&str, &str)> = clips.iter().filter(|clip| clip.is_cut_in()).zip(purposes.iter()).map(|(clip, purpose)| (clip.narration.as_deref().unwrap_or("").trim(), *purpose)).collect();
    let outro = |purpose: &str| matches!(purpose, "outro" | "conclusion");
    let mut flags = Vec::new();

    // Outros are short and final.
    let long_outros: Vec<String> = cut_ins.iter().filter(|(text, purpose)| outro(purpose) && words(text).len() > OUTRO_WORDS).map(|(text, _)| format!("{} words, {}", words(text).len(), quote(text))).collect();
    if !long_outros.is_empty() {
        flags.push(format!("{} outro{} run{} over {OUTRO_WORDS} words ({}). An outro is the outcome in one or two sentences: who was arrested, charged, convicted or sentenced, without every date, count and the judge, and nothing that came after.", long_outros.len(), if long_outros.len() == 1 { "" } else { "s" }, if long_outros.len() == 1 { "s" } else { "" }, long_outros.join("; ")));
    }
    if compilation {
        let bare: Vec<String> = cut_ins.iter().filter(|(text, purpose)| outro(purpose) && !text.is_empty() && !text.to_lowercase().contains("in the end")).map(|(text, _)| quote(text)).collect();
        if !bare.is_empty() {
            flags.push(format!("Outros that don't open \"In the end, …\": {}. Each case's outro gives its outcome, opening \"In the end, …\".", bare.join("; ")));
        }
    }
    let judged: Vec<String> = cut_ins.iter().filter(|(text, purpose)| outro(purpose) && (text.contains("concluded that") || text.contains("determined that"))).map(|(text, _)| quote(text)).collect();
    if !judged.is_empty() {
        flags.push(format!("An outro states a conclusion of guilt ({}). Keep allegations, charges and outcomes distinct: \"he was charged with…\", \"a jury found him guilty of…\".", judged.join("; ")));
    }

    // Cut-ins keep to their length.
    let long: Vec<String> = cut_ins
        .iter()
        .enumerate()
        .filter(|(index, (text, purpose))| !(outro(purpose) || matches!(*purpose, "intro" | "hook" | "title-line" | "transition" | "reuse-link" | "intro-and-case-opening") || (!compilation && *index == 0)) && words(text).len() > CUT_IN_WORDS)
        .map(|(_, (text, purpose))| format!("a {} of {} words, {}", purpose.replace('-', " "), words(text).len(), quote(text)))
        .collect();
    if !long.is_empty() {
        flags.push(format!("Cut-ins over {CUT_IN_WORDS} words ({}). A cut-in runs 50 to 65 words: what the viewer needs and the footage can't say, never a retelling of what it just showed.", long.join("; ")));
    }

    // Places that don't matter.
    let named: Vec<String> = cut_ins.iter().flat_map(|(text, _)| places(text)).collect();
    if !named.is_empty() {
        flags.push(format!("Addresses or named places the story doesn't turn on: {}. Give the city, state and agency; leave out streets, room numbers, schools and businesses unless the story turns on them.", named.join(", ")));
    }

    // Care with harm and children.
    let pairs: Vec<String> = cut_ins.iter().flat_map(|(text, _)| child_pairs(text)).collect();
    if !pairs.is_empty() {
        flags.push(format!("A care word beside \"child\" ({}). Never pair them: write \"images of a child\" or name the charge once in plain terms.", pairs.iter().map(|pair| format!("\"{pair}\"")).collect::<Vec<_>>().join(", ")));
    }
    let heavy: Vec<String> = cut_ins.iter().filter(|(text, _)| care_words(text) >= 3).map(|(text, _)| format!("{} in {}", care_words(text), quote(text))).collect();
    if !heavy.is_empty() {
        flags.push(format!("Care words beyond what the story needs ({}). Use sexual, rape, abuse and the like once to make the charge clear, never in a list of every count.", heavy.join("; ")));
    }
    let struck: Vec<String> = cut_ins.iter().flat_map(|(text, _)| blows(text)).collect();
    if !struck.is_empty() {
        flags.push(format!("Harm told by the blow ({}). Say what it meant (\"the baby had been hurt\"), never the blow itself.", struck.iter().map(|blow| format!("\"{blow}\"")).collect::<Vec<_>>().join(", ")));
    }

    // Tell the story forward.
    let telling: Vec<String> = cut_ins.iter().filter(|(text, _)| TELLING.iter().any(|phrase| text.to_lowercase().contains(phrase))).map(|(text, _)| quote(text)).collect();
    if !telling.is_empty() {
        flags.push(format!("Narration about the telling ({}). Tell the story forward; when the footage jumps back, give the time and move on (\"Earlier that afternoon, …\").", telling.join("; ")));
    }

    // Transitions open differently; case openings are dated.
    if compilation {
        let mut seen: Vec<String> = Vec::new();
        let mut twice: Vec<String> = Vec::new();
        for (text, purpose) in &cut_ins {
            if *purpose != "transition" || text.is_empty() {
                continue;
            }
            let opening = lower(&words(text)).into_iter().take(4).collect::<Vec<_>>().join(" ");
            if seen.contains(&opening) && !twice.contains(&opening) {
                twice.push(opening.clone());
            }
            seen.push(opening);
        }
        if !twice.is_empty() {
            flags.push(format!("Transitions open the same way: {}. Vary how each transition opens.", twice.iter().map(|opening| format!("\"{opening}…\"")).collect::<Vec<_>>().join(", ")));
        }
        let undated: Vec<String> = cut_ins.iter().filter(|(text, purpose)| *purpose == "case-opening" && !has_date(text)).map(|(text, _)| quote(text)).collect();
        if !undated.is_empty() {
            flags.push(format!("Case openings without a date ({}). Open on the verified date (\"On May 29, 2024, …\"); when no source gives one, say so in the report.", undated.join("; ")));
        }
    }

    // People: by full name before an outro.
    let mut cases: Vec<Vec<(&str, &str)>> = vec![Vec::new()];
    for (text, purpose) in &cut_ins {
        if *purpose == "transition" {
            cases.push(Vec::new());
        }
        if let Some(case) = cases.last_mut() {
            case.push((text, purpose));
        }
    }
    let known = people(script);
    let mut short_names = Vec::new();
    let mut late_names = Vec::new();
    for case in &cases {
        for person in &known {
            let full = person.join(" ");
            let first_name = &person[0];
            let surname = person.last().map(String::as_str).unwrap_or("");
            // By first name: relatives share a surname, and a transcript's spelling may differ from the reports'.
            let first = case.iter().find(|(text, _)| words(text).iter().any(|word| word == first_name || word.strip_suffix("'s") == Some(first_name.as_str())));
            if let Some((text, purpose)) = first {
                let said = words(text);
                // "Kenneth Bluew" introduces the speaker heard as "Ken Blue".
                let introduced = text.contains(&full)
                    || said.windows(2).any(|pair| capital(&pair[1]) && (pair[0].starts_with(first_name.as_str()) || (surname.len() >= 4 && pair[1].starts_with(&surname[..4]))) && capital(&pair[0]));
                if outro(purpose) {
                    late_names.push(full.clone());
                } else if !introduced {
                    short_names.push(full.clone());
                }
            }
        }
        // A full name the outro brings in that the case never said.
        for (text, _) in case.iter().filter(|(_, purpose)| outro(purpose)) {
            let said = words(text);
            let mut at = 0;
            while at < said.len() {
                let run = said[at..].iter().take_while(|word| capital(word) && !NOT_PEOPLE.contains(&word.to_lowercase().as_str())).count();
                if run >= 2 {
                    let mut name: Vec<&String> = said[at..at + run].iter().collect();
                    while name.first().is_some_and(|word| MONTHS.contains(&word.to_lowercase().as_str()) || ["In", "The", "On", "By", "After", "A", "An", "Both", "His", "Her"].contains(&word.as_str())) {
                        name.remove(0);
                    }
                    let earlier = case.iter().filter(|(_, earlier)| !outro(earlier)).any(|(other, _)| words(other).iter().any(|word| name.iter().any(|part| part.len() > 2 && *part == word)));
                    let full = name.iter().map(|word| word.as_str()).collect::<Vec<_>>().join(" ");
                    if name.len() >= 2 && !earlier && !late_names.contains(&full) {
                        late_names.push(full);
                    }
                }
                at += run.max(1);
            }
        }
    }
    if !short_names.is_empty() {
        flags.push(format!("People first named without their full name: {}. Introduce each person by full name, and age when the evidence gives it, the first time they matter.", short_names.join(", ")));
    }
    if !late_names.is_empty() {
        flags.push(format!("People first named in an outro: {}. Introduce them where they first matter in the case; an outro only uses names the viewer knows.", late_names.join(", ")));
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Clip;

    fn cut_in(text: &str) -> Clip {
        Clip { id: text.into(), start_ms: 0, end_ms: 3_000, kind: Some("cut-in".into()), narration: Some(text.into()), ..Default::default() }
    }

    #[test]
    fn finds_what_the_team_flagged() {
        assert_eq!(places("Officers went to a home on Southeast Burgundy Lane in Port St. Lucie."), ["Southeast Burgundy Lane"]);
        assert_eq!(places("He called from room 209 of the Red Roof Inn."), ["room 209", "Red Roof Inn"]);
        assert_eq!(places("He taught at Buena High School in Sierra Vista."), ["Buena High School"]);
        assert!(places("Deputies from the Flagler County Sheriff's Office drove to Main.").is_empty());
        assert_eq!(child_pairs("charged with two counts of dissemination of child pornography").len(), 1);
        assert!(child_pairs("the child was safe with her grandmother").is_empty());
        assert_eq!(blows("she had been hit in the head").len(), 1);
        assert!(blows("the news hit hard").is_empty());
        assert!(blows("her heart was beating again").is_empty() && blows("he kept beating her").len() == 1);
        assert!(has_date("On May 29, 2024, officers") && !has_date("South of Ketchum, Idaho, a man called"));
    }

    #[test]
    fn flags_outros_names_and_repeated_transitions() {
        let mut script: Script = serde_json::from_value(serde_json::json!({
            "id": "a1", "title": "Case", "sourceKind": "compilation", "sourceUrl": "", "originalPath": "", "status": "ready", "stage": "",
            "progress": 100, "error": null, "durationMs": 200_000, "revision": 1, "createdAt": 0, "updatedAt": 0
        }))
        .unwrap();
        script.speakers.insert("1:speaker_0".into(), "Briana Cole (Suspect)".into());
        let clips = vec![
            cut_in("A mother turned on her own baby."),
            Clip { id: "f".into(), start_ms: 0, end_ms: 9_000, ..Default::default() },
            cut_in("On March 3, 2024, deputies met Briana outside the car."),
            Clip { id: "g".into(), start_ms: 9_000, end_ms: 99_000, ..Default::default() },
            cut_in("In the end, Briana Cole was arrested, and her friend Mark Steiner was charged too."),
            cut_in("Up next, we have a case where a father drank."),
            Clip { id: "h".into(), start_ms: 99_000, end_ms: 104_000, ..Default::default() },
            cut_in("Officers in Graham were called."),
            Clip { id: "i".into(), start_ms: 104_000, end_ms: 200_000, ..Default::default() },
            cut_in("He was taken to the station to type up the charge against him, which the officers did that night after a long talk with his wife and the five-year-old girl."),
            cut_in("Up next, we have a case where parents kept five kids."),
        ];
        let purposes = ["intro", "case-opening", "outro", "transition", "case-opening", "outro", "transition"];
        let flags = text_flags(&script, &clips, &purposes, true);
        let all = flags.join("\n");
        assert!(all.contains("without their full name: Briana Cole"), "{all}");
        assert!(all.contains("first named in an outro: Mark Steiner"), "{all}");
        assert!(all.contains("Transitions open the same way: \"up next we have…\""), "{all}");
        assert!(all.contains("Case openings without a date"), "{all}");
        assert!(all.contains("Outros that don't open \"In the end"), "{all}");
    }
}
