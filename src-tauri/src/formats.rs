//! Repack's script formats, word for word: an interrogation (the default),
//! FOIA, Courtroom, and compilations of several cases ("When…" or hidden).
//! Repack's text names its editor's tools; Writer's Claude answers in one
//! plan, so those names are put plainly (see `plain`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Interrogation,
    Foia,
    Courtroom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    When,
    Hidden,
}

/// A compilation's style comes from its title, as in Repack.
pub fn style(title: &str) -> Style {
    let lower = title.trim_start().trim_start_matches(['"', '“', '\'', '‘']).to_lowercase();
    let starts = lower.strip_prefix("when").is_some_and(|rest| rest.is_empty() || !rest.chars().next().is_some_and(char::is_alphanumeric));
    if starts { Style::When } else { Style::Hidden }
}

#[derive(Debug, Clone, Copy)]
pub struct Format {
    pub kind: Kind,
    pub compilation: bool,
    pub style: Style,
}

impl Format {
    pub fn new(kind: Kind, compilation: bool, title: &str) -> Self {
        Self { kind, compilation, style: style(title) }
    }
    pub fn foia(&self) -> bool {
        self.kind == Kind::Foia && !self.compilation
    }
    pub fn courtroom(&self) -> bool {
        self.kind == Kind::Courtroom && !self.compilation
    }
    pub fn court_compilation(&self) -> bool {
        self.kind == Kind::Courtroom && self.compilation
    }
    /// A bodycam compilation: Repack gives a compilation in its FOIA workspace both guides.
    pub fn foia_compilation(&self) -> bool {
        self.kind == Kind::Foia && self.compilation
    }
}

pub const CUT_IN_GUIDE: &str = include_str!("../guides/cut-in.md");
pub const JUDGMENT_GUIDE: &str = include_str!("../guides/judgment.md");
const FOIA_GUIDE: &str = include_str!("../guides/foia.md");
const COURTROOM_GUIDE: &str = include_str!("../guides/courtroom.md");
const COMPILATION_GUIDE: &str = include_str!("../guides/compilation.md");

/// The guides after the cut-in framework and judgment calls, labelled as
/// Repack labels them.
pub fn extra_guides(format: &Format) -> Vec<(&'static str, &'static str)> {
    let mut guides = Vec::new();
    if format.foia() {
        guides.push(("PRODUCER'S FOIA GUIDE", FOIA_GUIDE));
    } else if format.courtroom() {
        guides.push(("PRODUCER'S COURTROOM GUIDE", COURTROOM_GUIDE));
    } else if format.compilation {
        guides.push(("PRODUCER'S COMPILATION GUIDE", COMPILATION_GUIDE));
        if format.foia_compilation() {
            guides.push(("PRODUCER'S FOIA GUIDE", FOIA_GUIDE));
        }
        if format.court_compilation() {
            guides.push(("PRODUCER'S COURTROOM GUIDE", COURTROOM_GUIDE));
        }
    }
    guides
}

/// Repack's editor tools, said as what Writer does instead.
pub fn plain(text: &str) -> String {
    text.replace("build_script and remove_silences do their part", "The app does its part")
        .replace("build_script and remove_silences do this themselves", "The app does this itself")
        .replace("read_script gives each cut-in its purpose", "Each cut-in's purpose follows from where it sits")
        .replace("read_script flags any intro clip that is not replayed, and build_script refuses the plan", "the app refuses a plan whose intro clip is not replayed")
        .replace("Give build_script the opening the plan uses", "Give the plan's opening")
        .replace("save it on that cut-in with set_reuse_link, and the script shows it as a comment on the transition", "put it on that cut-in as its reuseLink")
        .replace("In the plan, give a reuse's cut-in its reuseLink; set_reuse_link changes it later.", "In the plan, give a reuse's cut-in its reuseLink.")
        .replace("read the runtime the tool reports", "check the runtime")
        .replace("Never count pages or words yourself. Every result that changes the timeline, read_script and write_cut_ins report pages: how many the script fills now and how that stands against the target; read_script also gives what is on each page and the lines free at its bottom. They also report narrationWords: the words written and the cut-ins still unwritten. ", "The app counts the pages of the finished plan and sends back one outside the target, saying how far off it is. ")
        .replace("after assembling, read pages, and if", "after assembling, estimate the pages, and if")
        .replace("build_script then reports the exact pages;", "The app then counts the exact pages;")
        .replace("build_script", "the plan")
}

const CARD: &str = r#"Cut-ins are 3s "cut in" cards (cut-in.mp4) that mark where narration text will go; they are not footage."#;

const INTERROGATION_SHAPE: &str = r#"Cut-ins are 3s "cut in" cards (cut-in.mp4) that mark where narration text will go; they are not footage. Unless the producer asks for something else, assemble a video in this shape:
1. Calm opening: one or two clips, rarely three, about 9-12 seconds in all, that quietly begin the case, such as the interrogator sitting down and introducing themselves. Nothing dramatic is said yet; use your judgment for what introduces this case well.
2. Cut-in (the intro paragraph).
3. Hook: 15-30 seconds of the most striking moments from anywhere in the footage.
4. Cut-in (the case opening, "On [date]…").
The calm opening and the hook are the intro. Their clips may run in whatever order plays best and need not follow the footage's order, though they can. Never put the reading of rights in the intro: no Miranda warning and no talk about the person's rights, in the calm opening or in the hook. Every intro clip, in the calm opening and in the hook, is a promise: its moment plays again later in the body, where it happens in the story, so the viewer sees what the intro showed them. If the calm opening is the interrogator saying hello and sitting down, the body starts from that greeting, not after it. Choose hook moments the body keeps; read_script flags any intro clip that is not replayed, and build_script refuses the plan.
5. About 2-3 minutes of carefully selected initial footage that starts the case, with enough context to follow it, then a cut-in.
6. Keep early footage blocks around 2-3 minutes before a cut-in. Only once the viewer is well into the story and its stakes and context are established may blocks run 5-7 minutes, and only when careful clip selection sustains interest throughout. Long blocks are unacceptable if the footage drags: shorten them, remove weak passages or bring in narration at a natural story beat. Follow the story in source order after the hook. These intervals are pacing guides, not quotas; never stretch footage or interrupt a meaningful exchange just to meet them.
7. End with a cut-in after the final section."#;

const INTERROGATION_LENGTH: &str = "Length: unless the producer names one, use 30-50 minutes, aiming around 40, as a starting guide for a full interrogation video, not a hard limit. Go beyond 50 minutes when the selected footage and script stay interesting throughout and each additional beat earns its place. Never trim strong material merely to stay below 50 minutes or add weak material to reach a minimum. An explicit requested length takes precedence; land within about 10% of it. Never pad with dead air, repetition, duplicated footage or invented narration. If the source supports only a shorter engaging cut, preserve quality and explain it clearly in the report; do not claim a length target was reached.";

const INTERROGATION_SEQUENCE: &str = r#"Writing sequence is strict: cut-in 1 is the intro, two or three short sentences of about 40 to 55 words, which shocks in the first seconds by setting a specific, sourced expectation about this person beside the fact that breaks it, then opens a second question about why it happened or how it unravels; it leaves out anything a viewer would assume, never describes physical appearance, clothing or manner, places the scene where the opening footage actually is, and is the only cut-in that leans on the present tense; cut-in 2 opens the case ("On [date], …") with the verified date, place and how it reached police; later cut-ins develop the story with Anchor → Bridge → Teaser; only the final cut-in closes with the sourced outcome."#;

const FOIA_SHAPE: &str = r#"Cut-ins are 3s "cut in" cards (cut-in.mp4) that mark where narration text will go; they are not footage. This is a FOIA video: police footage released on request (bodycam, dashcam, the station), one case, told from the officers' arrival to the outcome. Choose the opening by the size of the case, unless the producer names one:
Bigger cases (a death, harm to a child, anything with many people or many scenes) take the two-split intro:
1. First intro clips: the initial contact (officers arriving, the first words with the person or the caller) and at most one striking line, about 10-20 seconds in all.
2. Cut-in (the intro).
3. The rest of the intro clips: the most striking moments from anywhere in the footage, about 15-25 seconds in all. Show the people at the centre of the case and the scene itself, not only officers talking about it.
4. Cut-in (the case opening, "On [date]…").
Small cases (one suspect, one scene, such as a man caught with images on his laptop) open on one run of clips:
1. Intro clips: the most striking moments from anywhere in the footage, about 20-30 seconds in all, usually four or five short clips.
2. Cut-in (the case opening, "On [date]…"), which is the only cut-in before the story.
Either way, intro clips are short proof, a few seconds each, and may run in whatever order plays best. Each one is a moment that also plays in the body, in its place in the story: the intro is a promise the footage keeps, never footage seen only there.
Then:
5. Footage blocks and cut-ins in source order. The first block after the case opening runs about 1-2 minutes, so the story's first turn comes quickly; after it, blocks run about 3 minutes, and a cut-in falls where the footage changes scene, a new person arrives, officers learn something new, or just before an escalation. Do not interrupt a strong moment to place one.
6. End with the outro: the final footage block, then one cut-in.
Give build_script the opening the plan uses: opening "one-run" or "two-split". With the two-split, the clips of both intro runs must play again in the body."#;

const FOIA_LENGTH: &str = "Length: unless the producer names one, aim for 20-30 minutes, about 8-12 footage blocks. Never under 10 minutes: that floor is hard, and an explicit requested length is the only thing that overrides it. Reach the length with footage first: more of the story's beats, fuller exchanges, the scene itself. Never pad with repetition, duplicated footage or invented narration. When the footage the story can use still falls short, keep longer pauses rather than go under the floor: build_script and remove_silences do this themselves, tightening pauses only as far as the floor allows, and say so when they have. Tell the producer when that happened. Never cut or trim footage that would take the video under the floor. Let a case with more to show run past 30 minutes when every beat earns its place. An explicit requested length takes precedence; land within about 10% of it. After arranging AND tightening silences, read the runtime the tool reports. Review whether an important beat is missing or a passage drags, and adjust.";

const FOIA_SEQUENCE: &str = r#"Writing sequence in a FOIA script: read_script gives each cut-in its purpose. With the two-split intro, cut-in 1 is the intro: one or two sentences, about 30-45 words, in the present or present perfect ("Officers have just responded to what appeared to be a routine welfare check"), that set the routine-looking situation and turn to what it became, ending on an ellipsis; no date, no names, no place beyond the city; cut-in 2 opens the case. With one run of intro clips, cut-in 1 is the case opening and there is no separate intro. The case opening is past tense, about 50-65 words: "On [verified date], [what happened] in [place]…", how it reached police, who the people are when they first matter, ending on a tease of what the next footage shows. Later cut-ins develop the story with Anchor → Bridge → Teaser, about 50-65 words each, ending on an ellipsis. Only the final cut-in closes: the outro is short and final, usually one or two sentences ("In the end, [full name] was arrested and charged with …"), with the furthest verified outcome and no new tease. Whatever its purpose, the first cut-in is saved with the intro role and the final one with the conclusion role; the others are bridges. The FOIA guide is given beside the Cut-In framework; where they differ, follow the FOIA guide."#;

fn pacing(from: &str, until: &str) -> String {
    format!("Pacing: {until} is cut tight, so someone is speaking almost all the time. From {from} on, relax: let the hearing breathe, build each block from as few timestamp ranges as you can, one continuous range wherever the footage allows, and keep its natural pauses. Leave out only what does not serve the story, and dead air of five seconds or more. build_script and remove_silences do their part: from {from} on, they join a block's clips that are less than 10 seconds apart into one range, and leave pauses shorter than 5 seconds.")
}

const COURTROOM_SHAPE: &str = r#"Cut-ins are 3s "cut in" cards (cut-in.mp4) that mark where narration text will go; they are not footage. This is a Courtroom video: one case told through court footage (a hearing or trial, sometimes with bodycam or dashcam played in court), from the backstory to the ruling. Unless the producer asks for something else, assemble it in this shape:
1. Intro clips: three to five short clips, about 25-45 seconds in all, of the moments the title promises: the officer caught out on the stand, the sharpest exchange, the judge's sharpest line. A few seconds each; one may run longer (up to about 25 seconds) when it is the moment the whole video is about. Never the ruling itself. They may run in whatever order plays best.
2. One cut-in holding the intro and the case opening, two paragraphs, with no footage between them: the narrator reads straight from one into the other.
3. Footage blocks and cut-ins in source order. Court footage carries itself, so cut-ins are few: between two cut-ins, let the hearing play a long section, usually 4-8 minutes, longer when an exchange holds. Place a cut-in only where the viewer needs what the footage cannot give them: a new person or witness to introduce, a jump in time or place, another hearing or video, the ruling. Never place one just to break up a long stretch or to restate what the footage shows. Do not stitch a block from many short pieces: cut a stretch only to drop what does not serve (a recess, a sidebar, paperwork, a long silence, an exchange that repeats), and even then keep it to two or three pieces. The hearing mostly plays through, its seams falling where the cut-ins go. Do not interrupt a strong exchange to place one.
4. End with the outro: the final footage block (the ruling as the judge gives it, when the footage has it), then one cut-in.
Every intro clip is a moment that also plays in the body, in its place in the story, unless it comes from a video the producer added for the intro and the body never uses. Many court videos are other creators' uploads with their own voice-over, recaps, intro and requests to subscribe: usually one speaker who talks about the people in the third person and is never answered. The transcript labels that speaker Narrator when the import recognised one, but check for yourself. Never put any of it in a clip, trim a clip so it neither starts nor ends on it, and never repeat its wording in the narration. When a narrator talks over footage the story needs, tell the producer."#;

const COURTROOM_LENGTH: &str = "Length: unless the producer names one, aim for a cut of about 18-25 minutes, about 3-5 footage blocks, with about 300-500 words of narration in all; for a longer cut, still about one cut-in every 4-8 minutes. Reach it with the hearing's own footage: the beats that build to the ruling, whole exchanges, the evidence as it is played. Never pad with repetition, duplicated footage or invented narration. A hearing with more to show may run past 25 minutes when every block earns its place. An explicit requested length takes precedence; land within about 10% of it. After arranging AND tightening silences, read the runtime the tool reports. Review whether an important beat is missing or a block drags, and adjust.";

const FACTS: &str = r#"Before you finish, check the facts the way the reviewers do (the Courtroom guide's "Getting the facts right" has their examples): speaker labels and names in the transcript are guesses, so confirm who is who from the hearing (a motion read aloud names its filer, the judge addresses people by name) and use a role when you cannot; introduce every person the first time the narration names them, in the narration if the footage that introduces them is cut; keep events in the order the footage shows them; name exactly what was asked or ordered and against whom, and never build a claim on one word the transcript may have misheard."#;

const COURTROOM_SEQUENCE: &str = r#"Writing sequence in a Courtroom script: read_script gives each cut-in its purpose. Cut-in 1 is the intro and the case opening, two paragraphs separated by a blank line. The intro is one or two sentences, about 20-45 words: the whole video's promise in a line (who, what they claimed, the moment it turned), never the ruling. The case opening is the backstory, one full paragraph of about 80-150 words, past tense: when and where, who the people are by full name and role, what happened before court (the stop, the charge, the dispute), why it reached a courtroom, and what the hearing is, ending on the moment the first footage picks up. Later cut-ins are short, two or three sentences, about 35-70 words each, Anchor → Bridge → Teaser: what the last stretch showed, where the hearing went next, a tease of the next block's turn, usually ending on an ellipsis. The outro states the ruling plainly with its specifics (the verdict, the fine or judgment and how it was worked out, what the judge said to the officer), says who came out ahead, may add what happened after, and adds no new tease. Whatever its purpose, the first cut-in is saved with the intro role and the final one with the conclusion role; the others are bridges. The Courtroom guide is given beside the Cut-In framework; where they differ, follow the Courtroom guide."#;

const COURTROOM_DOCUMENT: &str = "The exported Doc has a bold COURT title, Assets links and the total duration, then the intro clips' line, the intro paragraph, and everything after it as one running paragraph: the case opening, then each footage range inline followed by its cut-in, to the outro. Write each cut-in so it reads on from the footage before it in that paragraph. The Script view keeps each timestamp line above its cut-in so both stay editable, and its pages follow that layout.";

const COURTROOM_COMPILATION_SHAPE: &str = r#"Cut-ins are 3s "cut in" cards (cut-in.mp4) that mark where narration text will go; they are not footage. This is a Courtroom compilation: its title opens with "When", and every video is its own court case, an example of that title. Unless the producer asks for something else, assemble it in this shape:
The opening, about the whole compilation:
1. Intro clips: two to four very short clips, 1-8 seconds each, of the sharpest courtroom moments, taken from different cases. The first must connect with the title: find the title's keywords (who it is about, what happens, to whom) and open on a moment that shows them, such as the judge's sharpest line at the person the title names. Never a ruling.
2. Cut-in (the hook).
3. Intro clips: two to four more, from cases not yet shown where possible.
4. Cut-in (the title line, "These were the moments when…").
Then each case, one per video, in the order of the videos:
5. Cut-in (the transition into the case), placed directly after the previous cut-in with no footage between them.
6. The case's intro clips: two to five short clips, 1-8 seconds each, from that video, that show what the transition says. They are a promise: each moment should return soon in the case, not ten minutes in. Never the ruling.
7. Cut-in (the case opening: the backstory).
8. Footage blocks and cut-ins through the case in source order. Court footage carries itself, so cut-ins are few: between two cut-ins, let the hearing play a long section, usually 4-8 minutes, longer when an exchange holds. Place a cut-in only where the viewer needs what the footage cannot give them: a new person to introduce, a jump in time, another hearing, the ruling. Never place one just to break up a long stretch. Do not stitch a block from many short pieces: cut a stretch only to drop what does not serve the title (a recess, a sidebar, paperwork, a long silence, an exchange that repeats), and even then keep it to two or three pieces. Do not interrupt a strong exchange to place one.
9. The case's outro: a cut-in after its final footage, the ruling as the judge gives it when the footage has it.
The next case's transition follows that outro directly. The last case ends on its outro.
Plan it whole, in this order: while reading each video, note its two or three sharpest short moments for the opening.
A transition is the only cut-in with no footage before it. That is how the script tells a case's transition, case opening and outro apart, so never place two cut-ins together anywhere else and never leave out a case's intro clips.
Choosing footage: every case is there because of the title, so keep the stretches that deliver it (for a title about an attorney taunting a judge, the provocation and the judge's answer) and leave out what does not lead back to it. Many court videos are other creators' uploads with their own voice-over, recaps, intro and requests to subscribe: usually one speaker who talks about the people in the third person and is never answered. The transcript labels that speaker Narrator when the import recognised one, but check for yourself. Never put any of it in a clip, trim a clip so it neither starts nor ends on it, and never repeat its wording in the narration. When a narrator talks over footage the story needs, tell the producer. Where the producer's link to a video started partway in, that case begins near there: footage long before it may be another case on the same upload."#;

fn courtroom_compilation_length(cases: usize) -> String {
    format!("Length: unless the producer names one, each case runs about 8-12 minutes, so about {}-{} minutes for these {cases} cases, with about 1-3 footage blocks per case. There is no page target. Reach it with each hearing's own footage: the beats that build to the ruling, whole exchanges, the evidence as it is played. Let a case with less to show run shorter rather than pad it, and do not let one case crowd out the others. Never pad with repetition, duplicated footage or invented narration. An explicit requested length takes precedence: share it across the cases and land within about 10% of it. After arranging AND tightening silences, read the runtime the tool reports, then review case by case whether a beat is missing or a block drags, and adjust.", 8 * cases, 12 * cases)
}

const COURTROOM_COMPILATION_SEQUENCE: &str = r#"Writing sequence in a Courtroom compilation: read_script gives each cut-in its purpose (hook, title-line, transition, case-opening, story-development or outro). The hook is one or two short sentences holding everything the title says, simply: a general statement about the courtroom theme, then the question the video answers ("Some attorneys walk into court convinced the rules don't apply to them. But what happens when they push a judge one step too far?"). It names no person, date or place. The title line restates the title as what the viewer is about to watch ("These were the moments when…"). The first transition opens "We begin with the case of…" or "We begin with a case where…" and points straight at the title; later transitions ("Moving on to the next case where…", "Up next, we have a case where…") name what is specific to that case and tie it to the title, end on an ellipsis, never give away the ruling, and introduce even the last case as the next case, never "the final case". Each case opening is the Courtroom backstory, one paragraph of about 60-120 words, past tense: when and where when the evidence gives it, who the people are by full name and role, what happened before court, why it reached a courtroom and what this hearing is, ending on the moment the first footage picks up. Later cut-ins inside a case are short, two or three sentences, about 35-70 words each, Anchor → Bridge → Teaser, usually ending on an ellipsis. Each case's outro states its ruling plainly with its specifics (the verdict, the sanction, what the judge said), says who came out ahead, may add what happened after, and adds no new tease. No two outros open the same way: open each on its own case's consequence, never a stock phrase such as "In the end,". Every fact comes from that video and its own research, never from another case. Save the hook with the intro role, each case's outro with the conclusion role, and every other cut-in as a bridge. The compilation guide gives the opening and the transitions; the Courtroom guide gives how each case is told. Where either differs from the Cut-In framework, follow them."#;

const COURTROOM_COMPILATION_DOCUMENT: &str = "The document has a bold COURT title, Assets links, then footage range lines alternating with saved narration, case after case.";

fn case_steps(from: usize) -> String {
    format!(
        "{from}. Cut-in (the transition into the case), placed directly after the previous cut-in with no footage between them.
{}. The case's intro clips: two to five very short clips, 1-8 seconds each, from anywhere in that video, that show what the transition says. They are a promise to the viewer: the moment each comes from should return soon in the case, not ten minutes in. If it would, choose an earlier moment or trim the footage before it.
{}. Cut-in (the case opening, \"On [date]…\").
{}. The first footage block, usually up to about a minute. When the video opens in the middle of the action (a chase, a struggle), the case opening carries more detail and the first block may run longer.
{}. Footage blocks and cut-ins through the case in source order, ending with a cut-in (the case's outro) after its final footage.",
        from + 1,
        from + 2,
        from + 3,
        from + 4
    )
}

fn when_shape() -> String {
    format!(
        "{CARD} This is a standard compilation: its title opens with \"When\", and every case in it is an example of that title. Unless the producer asks for something else, assemble it in this shape:
The opening, about the whole compilation:
1. Intro clips: two to four very short clips, 1-8 seconds each, of the most striking moments, taken from different cases. A reused case whose video was imported counts as a case here: its picked moments may open the video too. The first must connect with the title: find the title's keywords (who it is about, what happens, to whom) and open on a moment that shows them. Never open on something far from what the title promises.
2. Cut-in (the hook).
3. Intro clips: two to four more, from cases not yet shown where possible.
4. Cut-in (the title line, \"These were the moments when…\").
Then each case, one per video, in the order of the videos:
{}
The next case's transition follows that outro directly. The last case ends on its outro.
Plan it whole, in this order: while reading each video, note its two or three most striking short moments for the opening.",
        case_steps(5)
    )
}

fn hidden_shape() -> String {
    format!(
        "{CARD} This is a hidden compilation: its title reads like a single case, and the viewer should not know there is more than one case until the first has ended. Unless the producer asks for something else, assemble it in this shape:
The first case, video 1, exactly as a single-case video, as if it were the whole video:
1. Calm opening: one or two clips, rarely three, about 9-12 seconds in all, that quietly begin the case, such as the interrogator sitting down and introducing themselves. Nothing dramatic is said yet.
2. Cut-in (the intro paragraph).
3. Hook: 15-30 seconds of the most striking moments from anywhere in video 1.
4. Cut-in (the case opening, \"On [date]…\").
5. Footage blocks and cut-ins through the case in source order, ending with a cut-in (the outro) after its final footage. Keep early blocks around 2-3 minutes before a cut-in.
The title, the intro and the hook are about the first case alone. Nothing before its outro shows or mentions another case.
Then each later case, one per video, in the order of the videos:
{}
The next case's transition follows that outro directly. The last case ends on its outro.",
        case_steps(6)
    )
}

const SHARED: &str = r#"A transition is the only cut-in with no footage before it. That is how the script tells a case's transition, case opening and outro apart, so never place two cut-ins together anywhere else and never leave out a case's intro clips. Never put the reading of rights in any intro clips: no Miranda warning and no talk about the person's rights.
Choosing footage in a compilation: every case is there because of the title, so choose the moments that deliver what the title promises, and leave out side conversations, long legal explanations and background that do not lead back to it. These videos were published by other creators and often carry that creator's own voice-over, recaps, intro, sponsor read and requests to subscribe: usually one speaker who talks about the people in the third person and is never answered. The transcript labels that speaker Narrator when the import recognised one, but check for yourself: an unlabelled or wrongly labelled speaker who talks that way is a narrator too. Never put any of that in a clip, and trim a clip so it neither starts nor ends on it. Use only what was said at the scene, on the call or in the interview room. Read the voice-over for what it tells you about the case, and never repeat its wording in the narration. When a narrator talks over footage the story needs, say so to the producer instead of keeping it silently. A short video, about 10-20 minutes, is usually used almost whole. A longer one is trimmed: when a question is asked or an answer given again, keep it once unless the repeat reveals something new, and let a cut-in carry what was left out (a 35-minute video may make a 24-minute case). Blocks inside a case mostly run one to three minutes. Place a cut-in where the footage changes location, where a new person enters, just before an escalation, after a key statement or reveal, or between two long stretches of dialogue. Do not interrupt a strong emotional moment: when the footage is already intense, let it run.
Reuse cases: the producer may mark a case as a reuse ("REUSE" and a link). It is a case the channel has already covered in an earlier video, and the editor lifts that finished segment into this one, so here it has no footage, no intro clips and no cut-ins of its own. It takes a single cut-in in its place in the order: its transition, written like any other. The link is never written in the narration: save it on that cut-in with set_reuse_link, and the script shows it as a comment on the transition. The next case's transition follows directly. Reuses the producer marked when making the script are listed with the videos, each with its link and its place in the order; they may name others in chat. In the plan, give a reuse's cut-in its reuseLink; set_reuse_link changes it later. You cannot watch a reuse video: write its transition from its title and what the producer tells you about the case, and ask for one line about it when the title alone does not say what happened."#;

const REUSE_FIRST: &str = " If the first case of this hidden compilation is the reuse, the script begins with a cut-in that carries only the reuse link (its reuseLink in the plan) and no narration, calm opening or hook, and the cut-in directly after it is the transition into the second case.";

/// Repack's, word for word (its 6 to 7 pages are `pages::COMPILATION_PAGES`);
/// `plain` says its tools as what Writer does: the app counts the finished
/// plan's pages and sends back one outside the target.
const COMPILATION_LENGTH: &str = "Length: a compilation is measured by its written script, not its runtime. The script, with its timestamp lines and every cut-in, runs 6 to 7 Letter pages unless the producer names another length. It may run just over, by one more page, when the cases truly need it, and never further. Runtime has no target: such a video often lands around an hour, and may run well over or well under that when the footage calls for it. Let each case run as long as it stays interesting and no longer, and do not let one case crowd out the others. An explicit requested length takes precedence: share it across the cases and land within about 10% of it.
Never count pages or words yourself. Every result that changes the timeline, read_script and write_cut_ins report pages: how many the script fills now and how that stands against the target; read_script also gives what is on each page and the lines free at its bottom. They also report narrationWords: the words written and the cut-ins still unwritten. Two things fill the pages: the cut-ins, each one concise paragraph, and the timestamp lines, which grow with the number of clips. So the length is largely settled when the cut is assembled: after assembling, read pages, and if the timestamp lines and empty cut-ins already take most of the target, there are too many cut-ins or the footage is cut into more clips than it needs; if they take very little, a case may be missing cut-ins where the placing rules call for one. Share the room across the cases by how much each has to tell, not evenly, and a reused case takes only its transition. Plan the whole script to the target before building: a page holds about 45 lines, a cut-in takes a line for about every 13 words plus one, and each footage block a line or two of timestamps. build_script then reports the exact pages; when they fall outside the target, bring the script inside it: when over, take out what repeats the footage, a cut-in the story does not need, or clips that split one moment into many timestamps; when under, add only sourced context the viewer lacks. Never reach the target by squeezing a paragraph. A cut-in must still read as a story told to someone who has not seen the case: who called and why, what the officer found, how one thing led to the next. A paragraph cut down to bare facts in clipped sentences is worse than a script that runs a page long, so when the choice is between the two, keep the paragraph whole, run over, and say so. The judgment calls show what a squeezed paragraph looks like. Never pad with dead air, repetition, duplicated footage or invented narration, and when the cases cannot honestly fit the target, say so with the page count rather than claim it was reached. After arranging AND tightening silences, read the runtime the tool reports, then review case by case whether a distinct beat is missing or a passage drags.";

const EACH_CASE: &str = r#"Each case is then written as a single case is: its case opening ("On [date], …") with the verified date, place and how it reached police, ending on a tease of what the next footage shows; story development with Anchor → Bridge → Teaser; and its own outro ("In the end, …") with that case's sourced outcome, kept short, with no new tease. Every fact comes from that video and its own research (under its video in the evidence), never from another case."#;

const TRANSITIONS: &str = r#"transitions ("Moving on to the next case where…", "Up next, we have a case where…", "That case was already disturbing, but the next one went even further, when…") bring the viewer's attention back by naming what is specific to that case and teasing it, never "Next is another case." Every transition must connect the case's specific, sourced events to the video's title: name the title's theme and show how this case delivers it, rather than only announcing the person, place or case. Introduce even the last case as the next case ("Up next…", "Moving on to the next case…"), never as "the final case", "the last case" or "our final case"; do not signal that the video is about to end."#;

fn when_sequence() -> String {
    format!("Writing sequence in a compilation: read_script gives each cut-in its purpose (hook, title-line, transition, case-opening, story-development or outro). The hook is one or two short sentences holding everything the title says, simply: a general statement, then the question the video answers (\"Some parents are capable of the unthinkable – letting their children live in deplorable conditions. But what happens when officers arrive and uncover a house of horror?\"). The title line restates the title as what the viewer is about to watch (\"These were the moments when…\"). The first transition opens \"We begin with the case of…\" or \"We begin with a case where…\" and points straight at the title, so the viewer knows the promise is being kept; later {TRANSITIONS} {EACH_CASE} Save the hook with the intro role, each case's outro with the conclusion role, and every other cut-in as a bridge.")
}

fn hidden_sequence() -> String {
    format!("Writing sequence in a hidden compilation: read_script gives each cut-in its purpose (intro, case-opening, story-development, outro, transition, or reuse-link). The first case is written exactly as a single case: cut-in 1 is the intro, two or three short sentences of about 40 to 55 words, which shocks in the first seconds by setting a specific, sourced expectation about this person beside the fact that breaks it, then opens a second question about why it happened or how it unravels; it leaves out anything a viewer would assume, never describes physical appearance, clothing or manner, places the scene where the opening footage actually is, and is about the first case only; cut-in 2 opens that case. After the first case's outro, the {TRANSITIONS} The first of them, \"Moving on to the case where…\", is where the viewer learns there is more than one case. {EACH_CASE} Save the intro with the intro role, each case's outro with the conclusion role, and every other cut-in as a bridge.")
}

/// Shape, length and writing sequence for the system prompt, plainly worded.
pub fn shape_length_sequence(format: &Format, cases: usize) -> (String, String, String) {
    let (shape, length, sequence) = if format.court_compilation() {
        (
            format!("{COURTROOM_COMPILATION_SHAPE}\n{}", pacing("the second case's transition", "The opening and the first case")),
            courtroom_compilation_length(cases.max(1)),
            format!("{COURTROOM_COMPILATION_SEQUENCE} {FACTS}"),
        )
    } else if format.compilation {
        let (shape, sequence) = match format.style {
            Style::When => (when_shape(), when_sequence()),
            Style::Hidden => (hidden_shape(), hidden_sequence()),
        };
        let reuse_first = if format.style == Style::Hidden { REUSE_FIRST } else { "" };
        (
            format!("{shape}\n{SHARED}{reuse_first}"),
            COMPILATION_LENGTH.to_string(),
            format!("{sequence} The compilation guide is given beside the Cut-In framework; where they differ, follow the compilation guide."),
        )
    } else if format.foia() {
        (FOIA_SHAPE.to_string(), FOIA_LENGTH.to_string(), FOIA_SEQUENCE.to_string())
    } else if format.courtroom() {
        (
            format!("{COURTROOM_SHAPE}\n{}", pacing("the video's halfway point", "The first half of the video")),
            COURTROOM_LENGTH.to_string(),
            format!("{COURTROOM_SEQUENCE} {FACTS}"),
        )
    } else {
        (INTERROGATION_SHAPE.to_string(), INTERROGATION_LENGTH.to_string(), INTERROGATION_SEQUENCE.to_string())
    };
    (plain(&shape), plain(&length), plain(&sequence))
}

/// What the finished document looks like, as Repack tells its editor.
pub fn document(format: &Format) -> &'static str {
    if format.courtroom() {
        COURTROOM_DOCUMENT
    } else if format.court_compilation() {
        COURTROOM_COMPILATION_DOCUMENT
    } else {
        "The document has a bold INTERRO title, then footage range lines alternating with the narration."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_style_from_the_title() {
        assert_eq!(style("When Cops Realize"), Style::When);
        assert_eq!(style("\"When\" a Cop Lies"), Style::When);
        assert_eq!(style("Whenever it rains"), Style::Hidden);
        assert_eq!(style("12-Year-Old Realizes Police Know"), Style::Hidden);
    }

    #[test]
    fn every_format_is_said_without_repacks_tools() {
        for (kind, compilation, title) in [
            (Kind::Interrogation, false, "A"),
            (Kind::Foia, false, "A"),
            (Kind::Courtroom, false, "A"),
            (Kind::Interrogation, true, "When A"),
            (Kind::Interrogation, true, "A"),
            (Kind::Courtroom, true, "When A"),
        ] {
            let (shape, length, sequence) = shape_length_sequence(&Format::new(kind, compilation, title), 3);
            for text in [shape, length, sequence] {
                assert!(!text.contains("build_script") && !text.contains("read_script") && !text.contains("remove_silences"), "{text}");
            }
        }
    }

    #[test]
    fn a_bodycam_compilation_reads_both_guides_as_repack_gives_them() {
        let names = |format: Format| extra_guides(&format).into_iter().map(|(name, _)| name).collect::<Vec<_>>();
        assert_eq!(names(Format::new(Kind::Foia, true, "When Cops")), vec!["PRODUCER'S COMPILATION GUIDE", "PRODUCER'S FOIA GUIDE"]);
        assert_eq!(names(Format::new(Kind::Interrogation, true, "When Cops")), vec!["PRODUCER'S COMPILATION GUIDE"]);
        assert_eq!(names(Format::new(Kind::Foia, false, "A case")), vec!["PRODUCER'S FOIA GUIDE"]);
        // Its shape, length and sequence are the compilation's, as in Repack.
        let (shape, length, _) = shape_length_sequence(&Format::new(Kind::Foia, true, "When Cops"), 3);
        assert!(shape.contains("standard compilation") && length.contains("6 to 7 Letter pages"));
    }

    #[test]
    fn compilations_carry_repacks_reuse_rules_and_length() {
        let (hidden, length, _) = shape_length_sequence(&Format::new(Kind::Interrogation, true, "Cop Reports Dead Body"), 3);
        assert!(hidden.contains("Reuse cases: the producer may mark a case as a reuse"));
        assert!(hidden.contains("If the first case of this hidden compilation is the reuse"));
        let (when, _, _) = shape_length_sequence(&Format::new(Kind::Interrogation, true, "When Cops"), 3);
        assert!(when.contains("Reuse cases:") && !when.contains("If the first case of this hidden compilation"));
        // Repack's tools are said as what Writer does.
        assert!(!length.contains("read_script") && !length.contains("build_script") && !length.contains("write_cut_ins"));
        assert!(length.contains("The app counts the pages of the finished plan"));
        assert!(!hidden.contains("set_reuse_link"));
    }
}
