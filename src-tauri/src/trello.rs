//! Cards waiting for a script on Repack's Trello lists, read through the
//! relay (its key and token stay there) and parsed as Repack's
//! script-trello.ts and script.ts parse them. Nothing is written to Trello.
//!
//! The Raw Channels list holds one case a card: its footage is the first
//! Drive folder (or Drive video), its article the first other web page. The
//! compilation list holds a compilation a card: its cases' links, "case N"
//! line by line. A compilation's case may be a YouTube video, a Drive video,
//! or a Google Doc of a script the channel already made, which is a reused
//! case (`import::lines_for`).

use serde::Serialize;
use serde_json::Value;

use crate::net::Relay;
use crate::settings::Settings;

/// A card labelled this is passed over, as Repack's Raw Channels skips it.
const SKIP_LABEL: &str = "missing footage";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub name: String,
    pub url: String,
    pub labels: Vec<String>,
    pub activity: Option<String>,
    /// `case` (Raw Channels), `compilation`, or `video` (a pasted card with one video).
    pub kind: &'static str,
    /// A case card's Drive folder or video, and its article.
    pub footage: Option<String>,
    pub article: Option<String>,
    /// A compilation card's lines, as the New script dialog takes them.
    pub lines: Vec<String>,
    pub videos: usize,
    pub reuses: usize,
    /// Why the card can't start a script as it is.
    pub problem: Option<String>,
    /// A script with the card's title already exists.
    pub scripted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cards {
    pub list: String,
    pub cards: Vec<Card>,
}

/// One list's cards, `raw` or `comp`, each parsed for what a script needs.
/// `titles` are the scripts already made, lowercased, as Repack hides a card
/// whose name a script has.
pub fn cards(settings: &Settings, list: &str, titles: &[String]) -> Result<Cards, String> {
    if list != "raw" && list != "comp" {
        return Err("Choose the Raw Channels list or the compilation list.".into());
    }
    let value = Relay::new(settings)?.trello_cards(list)?;
    let compilation = list == "comp";
    let cards = value["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|card| parse(card, compilation, titles))
        .collect();
    Ok(Cards { list: value["list"].as_str().unwrap_or(list).to_string(), cards })
}

/// The card a pasted link names (trello.com/c/<id>/…): its YouTube videos, from
/// wherever on the card they are (description, attachments, checklists,
/// comments), as a compilation's lines when there are two or more, or as one
/// video's import. A compilation card's "case N" lines are read as they are
/// on the compilation list, reused cases included.
pub fn card(settings: &Settings, link: &str, titles: &[String]) -> Result<Card, String> {
    let id = card_id(link).ok_or("That isn't a Trello card link (trello.com/c/…).")?;
    let value = Relay::new(settings)?.trello_card(&id)?;
    let name: String = value["name"].as_str().unwrap_or("Trello card").trim().chars().take(180).collect();
    let desc = value["desc"].as_str().unwrap_or("");
    let more: Vec<String> = ["attachments", "checklists", "comments"].iter().flat_map(|field| value[*field].as_array().cloned().unwrap_or_default()).filter_map(|item| item.as_str().map(str::to_string)).collect();
    // The description's cases first, as the compilation list reads them; then any video found elsewhere on the card.
    let mut links = compilation_links(desc);
    for found in links_in(&format!("{desc}\n{}", more.join("\n"))) {
        if crate::handoff::youtube_id(&found.url).is_some() && !links.iter().any(|known| same_video(&known.url, &found.url)) {
            links.push(found);
        }
    }
    links.retain(|link| link.reuse || crate::handoff::youtube_id(&link.url).is_some() || link.url.contains("drive.google.com"));
    let videos = links.iter().filter(|link| !link.reuse).count();
    let mut found = Card {
        id: value["id"].as_str().unwrap_or(&id).to_string(),
        scripted: titles.iter().any(|title| *title == name.to_lowercase()),
        name,
        url: value["url"].as_str().unwrap_or(link).to_string(),
        labels: value["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect(),
        activity: value["activity"].as_str().map(str::to_string),
        kind: if videos >= 2 { "compilation" } else { "video" },
        footage: None,
        article: None,
        lines: links.iter().map(Link::line).collect(),
        videos,
        reuses: links.len() - videos,
        problem: None,
    };
    if videos == 1 {
        found.footage = links.iter().find(|link| !link.reuse).map(|link| link.url.clone());
    }
    if videos == 0 {
        found.problem = Some("No YouTube or Drive video on the card.".into());
    }
    Ok(found)
}

/// The id in a card link: trello.com/c/<id>/<slug>.
pub fn card_id(link: &str) -> Option<String> {
    let rest = link.trim().split_once("trello.com/c/")?.1;
    let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (6..=32).contains(&id.len()).then_some(id)
}

fn parse(card: &Value, compilation: bool, titles: &[String]) -> Option<Card> {
    let name: String = card["name"].as_str()?.trim().chars().take(180).collect();
    let labels: Vec<String> = card["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    if !compilation && labels.iter().any(|label| label.trim().eq_ignore_ascii_case(SKIP_LABEL)) {
        return None;
    }
    let desc = card["desc"].as_str().unwrap_or("");
    let attachments: Vec<String> = card["attachments"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    let mut found = Card {
        id: card["id"].as_str().unwrap_or("").to_string(),
        scripted: titles.iter().any(|title| *title == name.to_lowercase()),
        name,
        url: card["url"].as_str().unwrap_or("").to_string(),
        labels,
        activity: card["activity"].as_str().map(str::to_string),
        kind: if compilation { "compilation" } else { "case" },
        footage: None,
        article: None,
        lines: Vec::new(),
        videos: 0,
        reuses: 0,
        problem: None,
    };
    if compilation {
        let links = compilation_links(desc);
        found.videos = links.iter().filter(|link| !link.reuse).count();
        found.reuses = links.len() - found.videos;
        found.lines = links.iter().map(Link::line).collect();
        found.problem = (found.videos < 2).then(|| {
            if found.videos == 0 { "No video links in the description.".into() } else { "Only one video to import; a compilation needs two.".into() }
        });
    } else {
        let (footage, article) = case_links(desc, &attachments);
        found.problem = footage.is_none().then(|| "No Google Drive footage on the card.".into());
        found.footage = footage;
        found.article = article;
    }
    Some(found)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub url: String,
    pub reuse: bool,
    /// A reused case's earlier script, as the card points to it: the earlier
    /// compilation's card (or Doc) and the case's number there.
    pub earlier: Option<String>,
}

impl Link {
    /// The line New script takes: "REUSE <video> <earlier card> case N".
    pub fn line(&self) -> String {
        match (self.reuse, &self.earlier) {
            (true, Some(earlier)) => format!("REUSE {} {earlier}", self.url),
            (true, None) => format!("REUSE {}", self.url),
            (false, _) => self.url.clone(),
        }
    }
}

/// "case 1 - Reuse:", "Case 4:", "**CASE #2**", "- case no. 3".
fn case_line(line: &str) -> bool {
    let rest = line.trim_start_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let Some(rest) = rest.strip_prefix("case") else { return false };
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('#').or_else(|| rest.strip_prefix("no.")).or_else(|| rest.strip_prefix("no")).or_else(|| rest.strip_prefix("number")).unwrap_or(rest);
    let digits = rest.trim_start().chars().take_while(char::is_ascii_digit).count();
    (1..=2).contains(&digits)
}

/// A line that only titles what follows it: "Thumb:", "## Notes".
fn heading_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(char::is_whitespace) {
        return true;
    }
    match line.split_once(':') {
        Some((before, after)) => {
            (1..=40).contains(&before.chars().count()) && after.chars().all(|c| !c.is_alphanumeric())
        }
        None => false,
    }
}

/// A compilation card's links: every line from a "case N" line to the next
/// heading, or, when the card numbers no cases, the whole description.
pub fn compilation_links(description: &str) -> Vec<Link> {
    let mut cases = Vec::new();
    let mut in_case = false;
    for line in description.lines() {
        if case_line(line) {
            in_case = true;
        } else if heading_line(line) {
            // "Reused:" heads more of the cases.
            in_case = in_case && says_reuse(line);
        }
        if in_case {
            cases.push(line);
        }
    }
    let links = links_in(&cases.join("\n"));
    if links.is_empty() { links_in(description) } else { links }
}

/// The video and script links in some text, in order. A link is a reuse when
/// its line says so ("REUSE https://…"), the line above it holds that word
/// and no link, as Repack reads them, or it's under a heading that does
/// ("Reused:"); a Google Doc is always one, a script the channel already
/// made. A reused video followed by another Trello card and "case N" (or a
/// Doc) is a case of that earlier compilation: where its script is.
pub fn links_in(text: &str) -> Vec<Link> {
    let mut found: Vec<Link> = Vec::new();
    let mut marked = false;
    // Under a heading that says reuse, until the next heading.
    let mut section = false;
    // The last link found can still take the earlier card on a line after it.
    let mut open = false;
    for line in text.lines() {
        let urls: Vec<String> = urls(line).iter().map(|raw| unwrap_redirect(raw)).collect();
        let mut words = line.to_string();
        for url in &urls {
            words = words.replace(url.as_str(), " ");
        }
        let says = says_reuse(&words);
        let wordy = words.chars().any(char::is_alphanumeric);
        if urls.is_empty() {
            // "Reused:" heads a section; "case 2 - Reuse:" only its own case.
            if case_line(line) {
                section = false;
            } else if heading_line(line) {
                section = says;
            }
            // Blank lines between the word and its link change nothing.
            if says {
                marked = true;
            } else if wordy {
                marked = false;
                open = false;
            }
            continue;
        }
        let videos: Vec<&String> = urls
            .iter()
            .filter(|url| crate::youtube::check_url(url).is_ok() || matches!(crate::drive::parse(url), Ok(crate::drive::Link::File(_))))
            .collect();
        let marked_here = says || marked || section;
        // A Doc beside a reused video is that case's earlier script; any other Doc is a reused case of its own.
        let (beside, docs): (Vec<&String>, Vec<&String>) = urls.iter().filter(|url| crate::reuse::is_doc(url)).partition(|_| marked_here && !videos.is_empty());
        // The earlier compilation this case was in: its card or Doc, and the case's number there.
        let earlier = urls
            .iter()
            .find_map(|url| card_id(url).filter(|_| url.contains("trello.com/c/")).map(|id| format!("https://trello.com/c/{id}")))
            .or_else(|| beside.first().map(|doc| doc.to_string()))
            .map(|link| match case_number(&words) {
                Some(case) => format!("{link} case {case}"),
                None => link,
            });
        if videos.is_empty() && docs.is_empty() && beside.is_empty() {
            // "https://trello.com/c/… (Case 4)" under the reused video it belongs to.
            if let (true, Some(earlier), Some(last)) = (open, earlier, found.last_mut()) {
                if last.earlier.is_none() && !crate::reuse::is_doc(&last.url) && (last.reuse || earlier.contains(" case ")) {
                    last.reuse = true;
                    last.earlier = Some(earlier);
                }
            }
            open = false;
            continue;
        }
        for doc in &docs {
            if !found.iter().any(|known| same_video(&known.url, doc)) {
                found.push(Link { url: doc.to_string(), reuse: true, earlier: None });
            }
        }
        if videos.is_empty() {
            open = false;
        } else {
            for url in videos {
                let reuse = marked_here || earlier.as_deref().is_some_and(|link| link.contains(" case "));
                if !found.iter().any(|known| same_video(&known.url, url) && known.reuse == reuse) {
                    found.push(Link { url: url.clone(), reuse, earlier: if reuse { earlier.clone() } else { None } });
                }
            }
            open = found.last().is_some_and(|last| last.earlier.is_none());
        }
        marked = false;
    }
    found
}

/// "(Case 4)", "**case 2**", "case #3": the case's number in an earlier compilation.
fn case_number(words: &str) -> Option<usize> {
    let lower = words.to_lowercase();
    let mut rest = lower.as_str();
    while let Some(at) = rest.find("case") {
        let after = rest[at + 4..].trim_start_matches(|c: char| c.is_whitespace() || c == '#' || c == '*' || c == '.' || c == ':' || c == 'n' || c == 'o');
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if (1..=2).contains(&digits.len()) {
            return digits.parse().ok().filter(|case| *case > 0);
        }
        rest = &rest[at + 4..];
    }
    None
}

fn says_reuse(words: &str) -> bool {
    let lower = words.to_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .any(|word| matches!(word.trim_matches('-'), "reuse" | "re-use" | "reused" | "re-used" | "reuses"))
}

/// The links in a line, without the punctuation a sentence leaves after one.
pub fn urls(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find("http") {
        let tail = &rest[at..];
        if !(tail.starts_with("https://") || tail.starts_with("http://")) {
            rest = &rest[at + 4..];
            continue;
        }
        let end = tail.find(|c: char| c.is_whitespace() || "\"'<>()[]`".contains(c)).unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(|c: char| ".,;:!?*_".contains(c));
        if url.len() > "https://".len() {
            out.push(url.to_string());
        }
        rest = &tail[end..];
    }
    out
}

/// Google's "https://www.google.com/url?q=…" redirect, taken off.
pub fn unwrap_redirect(url: &str) -> String {
    if url.contains("google.com/url?") {
        if let Some(target) = url.split(['?', '&']).find_map(|pair| pair.strip_prefix("q=")) {
            return crate::research::percent_decode(target);
        }
    }
    url.to_string()
}

/// The same video or Doc, however its link is written.
fn same_video(a: &str, b: &str) -> bool {
    key(a) == key(b)
}

fn key(url: &str) -> String {
    if let Some(id) = url.split_once("docs.google.com/document/d/").map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or("")) {
        return format!("doc {id}");
    }
    if let Ok(crate::drive::Link::File(id)) = crate::drive::parse(url) {
        return format!("drive {id}");
    }
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    if let Some(id) = query.split('&').find_map(|pair| pair.strip_prefix("v=")) {
        return format!("yt {id}");
    }
    let id = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    format!("yt {id}")
}

/// Hosts whose pages are never a case's article, as Repack's NOT_ARTICLE.
const NOT_ARTICLE: &[&str] = &[
    "google.com", "youtube.com", "youtu.be", "trello.com", "dropbox.com", "facebook.com", "instagram.com", "tiktok.com", "x.com",
    "twitter.com", "reddit.com", "evidence.com", "evidencelibrary.com", "justfoia.com", "nextrequest.com", "govqa.us", "visuallabsinc.com",
];

/// A case card's footage (its first Drive folder, or failing that its first
/// Drive video) and article (its first link to any other page).
pub fn case_links(description: &str, attachments: &[String]) -> (Option<String>, Option<String>) {
    let all: Vec<String> = description.lines().flat_map(urls).chain(attachments.iter().cloned()).map(|url| unwrap_redirect(&url)).collect();
    let drive: Vec<(String, bool)> = all
        .iter()
        .filter_map(|url| match crate::drive::parse(url) {
            Ok(crate::drive::Link::Folder(id)) => Some((format!("https://drive.google.com/drive/folders/{id}"), true)),
            Ok(crate::drive::Link::File(id)) => Some((format!("https://drive.google.com/file/d/{id}/view"), false)),
            Err(_) => None,
        })
        .collect();
    let footage = drive.iter().find(|(_, folder)| *folder).or(drive.first()).map(|(url, _)| url.clone());
    let article = all
        .iter()
        .find(|url| {
            let Some(rest) = url.strip_prefix("https://") else { return false };
            let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
            let host = host.to_lowercase();
            let path = path.split(['?', '#']).next().unwrap_or("");
            !path.is_empty() && !NOT_ARTICLE.iter().any(|blocked| host == *blocked || host.ends_with(&format!(".{blocked}")))
        })
        .cloned();
    (footage, article)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_id_from_links() {
        assert_eq!(card_id("https://trello.com/c/fCTopHe2/999-cop-reports-dead-body").as_deref(), Some("fCTopHe2"));
        assert_eq!(card_id("trello.com/c/fCTopHe2").as_deref(), Some("fCTopHe2"));
        assert!(card_id("https://trello.com/b/abc/board").is_none());
    }

    #[test]
    fn reads_a_compilation_card_as_repack_does() {
        let desc = "Thumb:\nsomething https://www.youtube.com/watch?v=thumb0000a\n\nCase 1:\nhttps://www.youtube.com/watch?v=aaaaaaaaaaa\n\
                    case 2 - Reuse:\n\nhttps://youtu.be/bbbbbbbbbbb?t=40\n**CASE #3**\n[https://drive.google.com/file/d/1abcdefghijklmnop/view](https://drive.google.com/file/d/1abcdefghijklmnop/view \"‌\")\n\
                    Case 4: the old bodycam script https://docs.google.com/document/d/1docdocdocdocdocdoc/edit\n## Notes\nhttps://www.youtube.com/watch?v=notes000000";
        let links = compilation_links(desc);
        assert_eq!(
            links,
            vec![
                Link { url: "https://www.youtube.com/watch?v=aaaaaaaaaaa".into(), reuse: false, earlier: None },
                Link { url: "https://youtu.be/bbbbbbbbbbb?t=40".into(), reuse: true, earlier: None },
                Link { url: "https://drive.google.com/file/d/1abcdefghijklmnop/view".into(), reuse: false, earlier: None },
                Link { url: "https://docs.google.com/document/d/1docdocdocdocdocdoc/edit".into(), reuse: true, earlier: None },
            ]
        );
    }

    #[test]
    fn a_reused_heading_covers_its_cases_and_their_earlier_cards() {
        // Card 996, as Trello gives it: three reused cases, each with the earlier compilation it was in.
        let desc = "Cases:\n\n\u{200c}\n\nOfficers Traumatized By What These Parents Did To Their Kids:\n\n\
                    [Parents Keep 2y/o Trapped](https://www.youtube.com/watch?v=UxAuydwwtbQ \"\u{200c}\")\n\n\
                    [Police Rescue Child](https://www.youtube.com/watch?v=uJlVqE3UWjY \"\u{200c}\")\n\n\u{200c}\n\nReused:\n\n\
                    [Police RESCUE 5 Kids](https://www.youtube.com/watch?v=5cN67V8lkeE \"\u{200c}\")\n\n\
                    [https://trello.com/c/U0FrQAUk](https://trello.com/c/U0FrQAUk \"smartCard-inline\")    (Case 4)\n\n\u{200c}\n\n\
                    [DCF Steps In](https://www.youtube.com/watch?v=pWoE9YSIxMw \"\u{200c}\")\n\n\
                    [https://trello.com/c/FsehmKPF](https://trello.com/c/FsehmKPF \"smartCard-inline\")    (Case 2)\n";
        let lines: Vec<String> = compilation_links(desc).iter().map(Link::line).collect();
        assert_eq!(
            lines,
            [
                "https://www.youtube.com/watch?v=UxAuydwwtbQ",
                "https://www.youtube.com/watch?v=uJlVqE3UWjY",
                "REUSE https://www.youtube.com/watch?v=5cN67V8lkeE https://trello.com/c/U0FrQAUk case 4",
                "REUSE https://www.youtube.com/watch?v=pWoE9YSIxMw https://trello.com/c/FsehmKPF case 2",
            ]
        );
        // Card 938's way: the earlier card on the reused video's own line.
        let desc = "Cases:\n\n1. [**A**](https://www.youtube.com/watch?v=J572aDTcZdE \"\u{200c}\")\n2. [**B**](https://www.youtube.com/watch?v=pWoE9YSIxMw \"\u{200c}\")\n\n**Reuse:**\n\n\
                    1. [**C**](https://www.youtube.com/watch?v=U0MFeMW3R8s \"\u{200c}\") [https://trello.com/c/bAmtOq5N/615-when-good-cops](https://trello.com/c/bAmtOq5N/615-when-good-cops \"smartCard-inline\")  **case 2**";
        let lines: Vec<String> = compilation_links(desc).iter().map(Link::line).collect();
        assert_eq!(lines.last().map(String::as_str), Some("REUSE https://www.youtube.com/watch?v=U0MFeMW3R8s https://trello.com/c/bAmtOq5N case 2"));
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn a_card_without_cases_gives_every_link() {
        let links = compilation_links("https://www.youtube.com/watch?v=aaaaaaaaaaa\nREUSE https://www.youtube.com/watch?v=bbbbbbbbbbb");
        assert_eq!(links.iter().map(|link| link.reuse).collect::<Vec<_>>(), [false, true]);
    }

    #[test]
    fn reads_a_case_card_as_repack_does() {
        let desc = "Footage: https://drive.google.com/file/d/1filefilefilefile/view\nFolder https://drive.google.com/drive/folders/1folderfolderfolder?usp=sharing\n\
                    https://www.youtube.com/watch?v=x https://www.wsbtv.com/news/local/story-123/.";
        let (footage, article) = case_links(desc, &[]);
        assert_eq!(footage.as_deref(), Some("https://drive.google.com/drive/folders/1folderfolderfolder"));
        assert_eq!(article.as_deref(), Some("https://www.wsbtv.com/news/local/story-123/"));
    }
}
