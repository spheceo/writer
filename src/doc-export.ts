import { blocks, clock, duration, heading, isCutIn, keptMs, partAt, range } from "./clips";
import { isClip, totalLine } from "./commentary";
import type { DocContent } from "./docx";
import type { Cite, Clip, Draft, Script, Source, Word } from "./types";

/** The name on the notes Writer writes on a Doc, as Repack signs its own. */
export const COMMENT_AUTHOR = "GeniusScriptwriter10";
export const BLUR_NOTE = "Note to editor: Blur this";
/** The commentary channel marks words for a bleep or blur with a plain "Censor". */
export const CENSOR_NOTE = "Censor";

/** Writes a Doc's text while keeping its styles and comments in Docs' indexes. */
function writer() {
  const content: DocContent = { text: "", styles: [], notes: [] };
  const at = () => content.text.length + 1;
  const append = (value: string, style?: Omit<DocContent["styles"][number], "start" | "end">) => {
    const start = at();
    content.text += value;
    if (style) content.styles.push({ start, end: at(), ...style });
  };
  const note = (start: number, end: number, text: string) => {
    if (text.trim() && end > start) content.notes.push({ start, end, text: text.trim() });
  };
  return { content, at, append, note };
}

// Repack's words to blur (domain/script-notes.ts): longer phrases first, so
// "sexual assault" is one note rather than two.
const SENSITIVE =
  /\b(?:child porn(?:ography)?|child predators?|sexual assaults?|sexual battery|pornograph(?:y|ic)|porn|sexual(?:ly)?|sex|abus(?:e|es|ed|ing|er|ers|ive)|neglect(?:s|ed|ing|ful)?|nud(?:e|es|ity)|kill(?:s|ed|ing|er|ers)?|murder(?:s|ed|ing|er|ers|ous)?|stab(?:s|bed|bing)?|suicid(?:e|es|al)|rap(?:e|es|ed|ing|ist|ists)|molest(?:s|ed|ing|ation|er|ers)?|lascivious|lewd|pedophil(?:e|es|ia)|incest(?:uous)?)\b/gi;
const HUNGER = /\b(?:starv(?:e|es|ed|ing|ation)|malnourish(?:ed|ment)?|malnutrition)\b/gi;
const CHILD = /\b(?:child(?:ren)?|kids?|boys?|girls?|sons?|daughters?|bab(?:y|ies)|infants?|toddlers?|minors?|teen(?:ager)?s?)\b/i;
const AGE = /\b(\d{1,2})[- ]years?[- ]old\b/gi;
/** What the commentary scripts also censor: death, slurs and the rest the old scripts flag. */
const CENSORED = /\b(?:death|deaths|dead|die|dies|died|dying|grooming|groom(?:ed|er|ers)?|masturbat\w*|drugs?|meth|weed|cocaine|overdos\w*|shoot(?:ing|ings)?|guns?)\b/gi;

/** The words of a narration to blur, as [start, end) offsets into it. */
export function blurSpans(narration: string, extra = false): [number, number][] {
  const found: [number, number][] = [];
  for (const match of narration.matchAll(SENSITIVE)) found.push([match.index, match.index + match[0].length]);
  if (extra) for (const match of narration.matchAll(CENSORED)) found.push([match.index, match.index + match[0].length]);
  // One sentence at a time: a child elsewhere in the paragraph does not count.
  for (const sentence of narration.matchAll(/[^.!?…]+[.!?…]*/g)) {
    const text = sentence[0];
    const child = CHILD.test(text) || [...text.matchAll(AGE)].some((age) => Number(age[1]) < 18);
    if (!child) continue;
    for (const match of text.matchAll(HUNGER)) {
      const start = (sentence.index ?? 0) + (match.index ?? 0);
      found.push([start, start + match[0].length]);
    }
  }
  return found.sort((a, b) => a[0] - b[0]).filter((span, index, all) => index === 0 || span[0] >= all[index - 1][1]);
}

/** A clip's first and last words, as Repack notes them on its timestamp. */
export function boundary(clip: Clip, words: Word[]) {
  const inside = words.filter((word) => word.startMs >= clip.startMs - 50 && word.endMs <= clip.endMs + 50);
  if (!inside.length) return "...";
  return `${inside[0].text}...${inside[inside.length - 1].text}`;
}

/** A YouTube link at a moment. Other links are left as they are. */
export function at(link: string, ms: number) {
  try {
    const url = new URL(link);
    const host = url.hostname.replace(/^www\.|^m\./, "");
    if (host !== "youtu.be" && host !== "youtube.com") return link;
    url.searchParams.set("t", String(Math.floor(ms / 1000)));
    return url.toString();
  } catch {
    return link;
  }
}

/**
 * An interrogation, FOIA, Courtroom or compilation script as Repack exports
 * it: the title line, the total duration, then each block's ranges (each
 * linked to its moment and noted with its first and last words) and its
 * narration, with Repack's blur notes on the words to blur.
 */
export function footageContent(script: Script): DocContent {
  const { content, at: here, append, note } = writer();
  append(`${heading(script)}\n`, { title: true });
  append(`\nTotal Duration: ${duration(keptMs(script.clips))}\n\n`);
  const youtube = script.sourceKind === "youtube" && !script.compilation ? script.sourceUrl : "";
  let lastPart = -1;
  for (const [index, block] of blocks(script.clips).entries()) {
    const first = block.clips[0]?.clip;
    const part = first ? partAt(script.parts, first.startMs) : lastPart;
    // A new case in a compilation is set off by a line, as Repack divides cases.
    if (script.compilation && index > 0 && part !== lastPart && block.clips.length) append(" \n", { divider: true });
    if (first) lastPart = part;
    block.clips.forEach(({ clip }, position) => {
      if (position) append(" + ");
      const start = here();
      append("(");
      // A non-breaking hyphen: Docs would otherwise wrap a timestamp there.
      append(range(clip, script.parts, "‑"), youtube ? { url: at(youtube, clip.startMs) } : undefined);
      append(")");
      note(start, here(), boundary(clip, script.words));
    });
    if (block.clips.length) append("\n\n");
    const narration = block.cue?.clip.narration?.trim();
    if (block.cue && narration && isCutIn(block.cue.clip)) {
      const from = here();
      for (const [a, b] of blurSpans(narration)) note(from + a, from + b, BLUR_NOTE);
      append(`${narration}\n\n`);
    }
  }
  if (!content.text.endsWith("\n")) append("\n");
  return content;
}

function cueNote(cite: Cite) {
  const quote = cite.quote.trim();
  const words = quote ? `“${quote.length > 400 ? `${quote.slice(0, 400)}…` : quote}”` : "Confirm speech and context by ear.";
  const verify = cite.verify?.replace(/^Inherited edit reference from S\d+\. Speech was not transcribed in the supplied script; /, "");
  return `${cite.video}\n${words}${verify ? `\nVerify: ${verify}` : ""}`;
}

/**
 * A commentary script in the channel's Doc layout: each section's
 * bold heading, the montage, paragraphs with their clips inline (each linked
 * and noted with its words and what to verify), the editor notes and words to
 * censor as comments, then the total time. Source inventories and recording
 * checklists stay in the Writer project, outside the script body.
 */
export function commentaryContent(_title: string, draft: Draft, _sources: Source[]): DocContent {
  const { content, at: here, append, note } = writer();
  content.layout = { fontSizePt: 11, headingSizePt: 13, paragraphSpacePt: 12, page: "A4" };
  const clip = (cite: Cite) => {
    const start = here();
    append("(");
    const times = `${clock(cite.startMs)}-${clock(cite.endMs)}`;
    append(times, cite.link ? { url: at(cite.link, cite.startMs) } : undefined);
    append(")");
    note(start, here(), cueNote(cite));
  };
  draft.sections.forEach((section, index) => {
    append(`${section.heading.replace(/^(\d+\.) /, "$1\t")}\n`, { bold: true });
    if (index === 0 && draft.open.length) {
      draft.open.forEach((cite, position) => {
        if (position) append(" + ");
        clip(cite);
      });
      append("\n");
    }
    for (const paragraph of section.paragraphs) {
      const start = here();
      const said: [number, number][] = [];
      for (const [position, piece] of paragraph.pieces.entries()) {
        if (position) append(isClip(piece) && isClip(paragraph.pieces[position - 1]) ? " + " : " ");
        if (isClip(piece)) {
          clip(piece);
          continue;
        }
        const text = piece.text.trim();
        const from = here();
        for (const [a, b] of blurSpans(text, true)) note(from + a, from + b, CENSOR_NOTE);
        append(text);
        said.push([from, here()]);
      }
      // The editor's note sits on the paragraph's first words, as a comment.
      if (paragraph.note?.trim()) {
        const [a, b] = said[0] ?? [start, here()];
        note(a, b, `For editor: ${paragraph.note.trim()}`);
      }
      append("\n");
    }
  });
  append(`\n${totalLine(draft)}\n`);
  return content;
}

/** The Doc's content for any script. */
export function docContent(script: Script): DocContent {
  const draft = script.commentary?.draft;
  if (script.commentary) {
    if (!draft) throw new Error("Write the script before exporting it.");
    return commentaryContent(script.title, draft, script.commentary.sources);
  }
  return footageContent(script);
}
