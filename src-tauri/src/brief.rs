//! A producer's brief as it's pasted: a title, notes on the case, and links,
//! each under whatever line names it ("Part 3:", "Surveillance Footage:",
//! "More on the text messages etc:"). No two briefs are laid out alike, so
//! this reads them roughly: a line ending in a colon, or the words on a
//! link's own line, label the links that follow; prose is the producer's
//! notes. Each link is given a role from its label so the build knows what
//! it is looking at:
//!
//! - `hearing`: the case's main footage, in order ("Part 1", "Day 2", "the
//!   trial", "sentencing", or a link with no label);
//! - `supporting`: other footage of the case (surveillance, bodycam, an
//!   interrogation, a 911 call), shown where the story reaches it;
//! - `background`: a video about the case ("More on…", news, an explainer),
//!   read for its facts and shown only where nothing else has the moment;
//! - `article`: a web page, read for the case's facts.
//!
//! The import (`import::from_brief`) joins the videos as one case (hearing
//! first, by part, then supporting, then background) and keeps each one's
//! label with the script (`Script.labels`), which the evidence names beside
//! each recording (`claude::evidence`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    #[default]
    Hearing,
    Supporting,
    Background,
    Article,
}

/// One link in a brief, with the line that names it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub url: String,
    /// The brief's own words for it ("Part 2", "Surveillance Footage"); empty when none.
    pub label: String,
    pub role: Role,
    /// "Part 3" / "Day 3" / "3 of 6": where it falls in the hearing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    pub title: String,
    /// The producer's notes on the case, as written.
    pub notes: String,
    /// In the brief's order.
    pub links: Vec<Link>,
}

impl Brief {
    /// The videos to import, in the order they join: the hearing by part
    /// (then as listed), then supporting footage, then background.
    pub fn videos(&self) -> Vec<&Link> {
        let mut videos: Vec<&Link> = self.links.iter().filter(|link| link.role != Role::Article).collect();
        videos.sort_by_key(|link| (link.role as u8, link.part.unwrap_or(u32::MAX)));
        videos
    }

    pub fn articles(&self) -> Vec<&Link> {
        self.links.iter().filter(|link| link.role == Role::Article).collect()
    }
}

/// A recording's label as the script keeps it: what the brief called it and its role.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub label: String,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
}

impl Label {
    /// How the evidence names it beside the recording's words.
    pub fn describe(&self, hearing_parts: usize) -> String {
        let named = if self.label.is_empty() { String::new() } else { format!("\"{}\": ", self.label) };
        let role = match self.role {
            Role::Hearing if hearing_parts > 1 => format!("the hearing, part {} of {hearing_parts}", self.part.map(|part| part.to_string()).unwrap_or_else(|| "?".into())),
            Role::Hearing => "the hearing".into(),
            Role::Supporting => "supporting footage, not the hearing: show it where the story reaches it (or where the court plays it), or in the intro clips".into(),
            Role::Background => "background on the case, not the hearing: read it for facts; show it only for a moment nothing else has".into(),
            Role::Article => "an article".into(),
        };
        format!("{named}{role}")
    }
}

pub fn parse(text: &str) -> Brief {
    let mut brief = Brief::default();
    let mut notes: Vec<String> = Vec::new();
    // The line naming the links below it, until the next one.
    let mut heading = String::new();
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', '•']).trim();
        if line.is_empty() {
            continue;
        }
        let urls = urls_in(line);
        if urls.is_empty() {
            let label = line.strip_suffix(':').or_else(|| line.strip_suffix('：')).map(str::trim);
            if brief.title.is_empty() && label.is_none() && !is_label(line) {
                brief.title = line.chars().take(180).collect();
            } else if let Some(label) = label.filter(|label| label.chars().count() <= 80) {
                heading = label.to_string();
            } else if is_label(line) {
                heading = line.to_string();
            } else {
                notes.push(line.to_string());
            }
            continue;
        }
        // Words beside a link on its own line name it; otherwise the heading does.
        let mut rest = line.to_string();
        for url in &urls {
            rest = rest.replace(url.as_str(), " ");
        }
        let beside = rest.trim().trim_matches([':', '-', '–', '—', '(', ')', ' ']).trim().to_string();
        let label = if beside.is_empty() { heading.clone() } else { beside };
        for url in urls {
            let url = clean(&url);
            if brief.links.iter().any(|link| link.url == url) {
                continue;
            }
            let video = is_video(&url);
            let (role, part) = classify(&label, video);
            brief.links.push(Link { url, label: label.clone(), role, part });
        }
    }
    brief.notes = notes.join("\n\n");
    // A brief with no hearing has its first video as the case ("Context: … <link>").
    if !brief.links.iter().any(|link| link.role == Role::Hearing) {
        if let Some(first) = brief.links.iter_mut().find(|link| link.role != Role::Article) {
            first.role = Role::Hearing;
        }
    }
    // Hearing parts with no number of their own are numbered as listed.
    let mut next = 1;
    let numbered = brief.links.iter().filter(|link| link.role == Role::Hearing).all(|link| link.part.is_none());
    for link in brief.links.iter_mut().filter(|link| link.role == Role::Hearing) {
        if numbered {
            link.part = Some(next);
        }
        next += 1;
    }
    brief
}

/// A short line naming what follows ("Surveillance Footage", "Part 2").
fn is_label(line: &str) -> bool {
    let lower = line.to_lowercase();
    line.chars().count() <= 40 && !line.ends_with(['.', '!', '?']) && (part_number(&lower).is_some() || SUPPORTING.iter().chain(BACKGROUND).any(|word| lower.contains(word)))
}

const BACKGROUND: &[&str] = &["more on", "more about", "background", "context", "news", "article", "explainer", "explained", "documentary", "report", "coverage", "breakdown", "text messages", "texts"];
const SUPPORTING: &[&str] = &["surveillance", "cctv", "security footage", "security camera", "bodycam", "body cam", "body-cam", "dashcam", "dash cam", "interrogation", "interview", "911", "jail call", "phone call", "arrest", "doorbell", "ring camera", "cell phone", "footage"];
const HEARING: &[&str] = &["trial", "hearing", "sentencing", "verdict", "court", "testimony", "arraignment", "full case"];

fn classify(label: &str, video: bool) -> (Role, Option<u32>) {
    if !video {
        return (Role::Article, None);
    }
    let lower = label.to_lowercase();
    let part = part_number(&lower);
    if part.is_some() || HEARING.iter().any(|word| lower.contains(word)) {
        return (Role::Hearing, part);
    }
    if BACKGROUND.iter().any(|word| lower.contains(word)) {
        return (Role::Background, None);
    }
    if SUPPORTING.iter().any(|word| lower.contains(word)) {
        return (Role::Supporting, None);
    }
    (Role::Hearing, None)
}

/// "part 3", "pt. 3", "day 3", "3 of 6", "part three".
fn part_number(lower: &str) -> Option<u32> {
    const WORDS: [&str; 12] = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"];
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).collect();
    for (index, word) in words.iter().enumerate() {
        let next = words.get(index + 1).copied().unwrap_or("");
        if matches!(*word, "part" | "pt" | "day" | "session") {
            if let Ok(number) = next.parse() {
                return Some(number);
            }
            if let Some(at) = WORDS.iter().position(|name| *name == next) {
                return Some(at as u32 + 1);
            }
        }
        if next == "of" && words.get(index + 2).is_some_and(|total| total.parse::<u32>().is_ok()) {
            if let Ok(number) = word.parse() {
                return Some(number);
            }
        }
    }
    None
}

fn urls_in(line: &str) -> Vec<String> {
    line.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"')
        .filter_map(|token| {
            let at = token.find("https://").or_else(|| token.find("http://"))?;
            let url = token[at..].trim_end_matches(['.', ',', ')', ']', ';']);
            (url.len() > 10).then(|| url.to_string())
        })
        .collect()
}

fn is_video(url: &str) -> bool {
    let host = url.split("://").nth(1).and_then(|rest| rest.split(['/', '?']).next()).unwrap_or("");
    let doc = url.contains("docs.google.com/");
    !doc && (host.ends_with("youtube.com") || host == "youtu.be" || host == "drive.google.com")
}

/// A YouTube link with only its video and start time: the brief's playlist
/// and search parameters (`list`, `index`, `pp`) go.
fn clean(url: &str) -> String {
    let host_youtube = url.contains("youtube.com/watch");
    if !host_youtube {
        return url.to_string();
    }
    let Some((base, query)) = url.split_once('?') else { return url.to_string() };
    let kept: Vec<&str> = query.split('#').next().unwrap_or("").split('&').filter(|pair| pair.starts_with("v=") || pair.starts_with("t=")).collect();
    if kept.is_empty() {
        url.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COURT: &str = "Cop Used 15-Year-Old In Sadistic S*x Ring, Then She's Found Dead
Surveillance Footage:

https://www.youtube.com/watch?v=iIFlj7gV5Tc

More on the text messages etc:

https://www.youtube.com/watch?v=swTrrS-LupM&pp=ygUPTWF0dGhldyBGYXJ3ZWxs


Part 1:

https://www.youtube.com/watch?v=_OUHOqTMAb4&list=PLLq-6my_qlf8OBr5VKLKk0s7Z1cj4z1Yf&index=2

Part 2:

https://www.youtube.com/watch?v=wm_US4mlpoM&list=PLLq-6my_qlf8OBr5VKLKk0s7Z1cj4z1Yf&index=2

Part 3:

https://www.youtube.com/watch?v=X9S5DhhmvYg&list=PLLq-6my_qlf8OBr5VKLKk0s7Z1cj4z1Yf&index=3
";

    #[test]
    fn labels_each_link_by_the_line_above_it() {
        let brief = parse(COURT);
        assert_eq!(brief.title, "Cop Used 15-Year-Old In Sadistic S*x Ring, Then She's Found Dead");
        assert!(brief.notes.is_empty());
        let roles: Vec<(Role, Option<u32>)> = brief.links.iter().map(|link| (link.role, link.part)).collect();
        assert_eq!(roles, vec![(Role::Supporting, None), (Role::Background, None), (Role::Hearing, Some(1)), (Role::Hearing, Some(2)), (Role::Hearing, Some(3))]);
        assert_eq!(brief.links[0].label, "Surveillance Footage");
        assert_eq!(brief.links[1].url, "https://www.youtube.com/watch?v=swTrrS-LupM");
        assert_eq!(brief.links[2].url, "https://www.youtube.com/watch?v=_OUHOqTMAb4");
        // The hearing joins first, in its order; then the rest.
        let order: Vec<&str> = brief.videos().iter().map(|link| link.label.as_str()).collect();
        assert_eq!(order, vec!["Part 1", "Part 2", "Part 3", "Surveillance Footage", "More on the text messages etc"]);
    }

    #[test]
    fn prose_is_the_producers_notes() {
        let brief = parse("CORRUPT Jail Guard Caught Smuggling Dr*gs Into Jail — Judge DESTROYS Her\nContext, really great case:\n\nIn May 2021, Fulton County detention officer Jadon Young was found unresponsive in a locked jail tower. She was sentenced to 40 years.\n\nhttps://www.youtube.com/watch?v=48RFLHRSwiA\n");
        assert_eq!(brief.title, "CORRUPT Jail Guard Caught Smuggling Dr*gs Into Jail — Judge DESTROYS Her");
        assert!(brief.notes.starts_with("In May 2021, Fulton County"));
        assert_eq!(brief.links.len(), 1);
        // Its heading is the context line, but a brief's only video is the case.
        assert_eq!(brief.links[0].role, Role::Hearing);
        assert_eq!(brief.videos().len(), 1);
    }

    #[test]
    fn words_beside_a_link_name_it_and_pages_are_articles() {
        let brief = parse("Some case\nBodycam - https://youtu.be/abcdefghijk\nhttps://www.youtube.com/watch?v=ABCDEFGHIJK (sentencing)\nhttps://news.example.com/story.html\nDay 2: https://www.youtube.com/watch?v=BBBBBBBBBBB");
        let roles: Vec<(Role, Option<u32>)> = brief.links.iter().map(|link| (link.role, link.part)).collect();
        assert_eq!(roles, vec![(Role::Supporting, None), (Role::Hearing, None), (Role::Article, None), (Role::Hearing, Some(2))]);
        assert_eq!(brief.links[0].label, "Bodycam");
        assert_eq!(brief.articles().len(), 1);
    }

    #[test]
    fn unlabelled_videos_are_the_hearing_in_order() {
        let brief = parse("Title here\nhttps://www.youtube.com/watch?v=AAAAAAAAAAA\nhttps://www.youtube.com/watch?v=BBBBBBBBBBB");
        assert_eq!(brief.links.iter().map(|link| link.part).collect::<Vec<_>>(), vec![Some(1), Some(2)]);
        assert_eq!(part_number("pt. three"), Some(3));
        assert_eq!(part_number("2 of 6"), Some(2));
    }
}
