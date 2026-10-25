import { blocks, clipRange, clock, duration, heading, isCutIn, keptMs, partAt } from "./clips";
import { isClip, liveMarks, noteLines, totalLine } from "./commentary";
import type { DocContent } from "./docx";
import { noteRange } from "./notes";
import type { Cite, Clip, Draft, Script, Source, Word } from "./types";

/** The name on the notes Writer writes on a Doc, as Repack signs its own. */
export const COMMENT_AUTHOR = "GeniusScriptwriter10";
export const BLUR_NOTE = "Note to editor: Blur this";
/** The commentary channel marks words for a bleep or blur with a plain "Censor". */
export const CENSOR_NOTE = "Censor";
/** What a reused case's transition prints when nothing is written on it, for its link's comment to sit on. */
export const REUSE_MARK = "REUSE";

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

/**
 * A clip's first and last words, as Repack notes them on its timestamp: any
 * word the clip touches, lowercased and without the punctuation around it
 * ("yeah," reads as "yeah"), as respelled where the producer fixed one.
 */
export function boundary(clip: Pick<Clip, "startMs" | "endMs">, words: Word[], corrections: Record<string, string> = {}) {
  const inside = words.filter((word) => word.startMs < clip.endMs && word.endMs > clip.startMs);
  const first = inside[0];
  const last = inside.at(-1);
  if (!first || !last) return "...";
  const bare = (word: Word) => {
    const text = (corrections[word.id] ?? word.text).toLowerCase();
    return text.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "") || text;
  };
  return `${bare(first)}...${bare(last)}`;
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

/** A reused case's comment: its link, and where the channel's earlier script of it is, when the card said. */
export function reuseNote(script: Pick<Script, "reuses">, url: string) {
  const reuse = script.reuses?.find((item) => item.url === url);
  if (!reuse?.script) return `Reuse: ${url}`;
  const casePart = /\bcase (\d+)$/i.exec(reuse.earlier ?? "")?.[1];
  const name = reuse.scriptTitle ? `“${reuse.scriptTitle}”` : "the earlier script";
  return `Reuse: ${url}\nIts script: ${casePart ? `case ${casePart} of ` : ""}${name} ${reuse.script}`;
}

const isLink = (value: string | undefined): value is string => !!value && /^https?:\/\/\S+$/i.test(value);

/**
 * Where a clip's footage can be watched, as Repack links it: its own
 * recording's link from the moment it starts on that recording's clock; a
 * single YouTube video from the moment; a single Drive file or folder as it
 * is. Footage from this Mac has nowhere to link.
 */
export function rangeUrl(script: Pick<Script, "sourceUrl" | "sourceKind" | "parts" | "compilation">, clip: Pick<Clip, "startMs">) {
  const index = partAt(script.parts, clip.startMs);
  const part = index >= 0 ? script.parts?.[index] : undefined;
  const url = part?.url ?? (script.compilation ? "" : script.sourceUrl);
  if (!isLink(url)) return "";
  if (!part?.url && script.sourceKind !== "youtube") return url;
  try {
    const watch = new URL(url);
    watch.searchParams.set("t", String(Math.floor((clip.startMs - (part?.startMs ?? 0)) / 1000)));
    return watch.toString();
  } catch {
    return url;
  }
}

/**
 * What the script lists under "Assets:", as Repack lists it: one case's
 * footage once (a Drive folder as the folder), or a compilation's videos in
 * order, "Footage 1", "Footage 2"….
 */
export function assetLinks(script: Pick<Script, "sourceUrl" | "sourceKind" | "parts" | "compilation">) {
  if (script.compilation)
    // A reused case's footage is linked from its transition, not listed here.
    return (script.parts ?? []).flatMap((part, index) => (isLink(part.url) && !part.reuse ? [{ label: `Footage ${index + 1}`, url: part.url }] : []));
  // One Courtroom case across several videos links each video once, in order.
  if (script.sourceKind === "compilation" && (script.parts?.length ?? 0) > 1) {
    const urls = [...new Set((script.parts ?? []).map((part) => part.url).filter(isLink))];
    return urls.map((url, index) => ({ label: `Footage ${index + 1}`, url }));
  }
  return isLink(script.sourceUrl) ? [{ label: "Footage", url: script.sourceUrl }] : [];
}

/**
 * Where a compilation's cases start, by block: each transition (a cut-in with
 * no footage before it), and the first block of a hidden compilation, as
 * Repack divides them. A single case has none.
 */
export function caseStarts(script: Pick<Script, "clips" | "compilation" | "title">) {
  if (!script.compilation) return [];
  const hidden = !/^\s*["“'‘]?when\b/i.test(script.title);
  return blocks(script.clips).flatMap((block, index) => (!block.clips.length || (index === 0 && hidden) ? [index] : []));
}

/**
 * An interrogation, FOIA, Courtroom or compilation script as Repack exports
 * it: the title line, the assets, the total duration, then each block's
 * ranges (each linked to its moment and noted with its first and last words)
 * and its narration, with Repack's blur notes on the words to blur. A
 * Courtroom case runs on after its first block in one paragraph and ends on
 * how long its clips run. `copy` writes each asset's link after its label,
 * for text that has no links.
 */
export function footageContent(script: Script, copy = false): DocContent {
  const { content, at: here, append, note } = writer();
  append(`${heading(script)}\n`, { title: true });
  const assets = assetLinks(script);
  if (assets.length) {
    append("\nAssets:\n");
    for (const { label, url } of assets) {
      append("-\t", { asset: true });
      append(label, { url });
      if (copy) append(` ${url}`);
      append("\n");
    }
  }
  append(`\nTotal Duration: ${duration(keptMs(script.clips))}\n\n`);
  const running = script.kind === "courtroom" && !script.compilation;
  const cases = caseStarts(script);
  for (const [index, block] of blocks(script.clips).entries()) {
    const inline = running && index > 0;
    // A new case in a compilation is set off by a line, as Repack divides cases.
    if (cases.indexOf(index) > 0) append(" \n", { divider: true });
    if (inline && block.clips.length && !content.text.endsWith("\n")) append(" ");
    block.clips.forEach(({ clip }, position) => {
      if (position) append(" + ");
      const start = here();
      // The brackets belong to the note, not to the link.
      append("(");
      const url = rangeUrl(script, clip);
      // A non-breaking hyphen: Docs would otherwise wrap a timestamp there.
      append(clipRange(clip, script.parts, "\u2011"), url ? { url } : undefined);
      append(")");
      // Its first and last words, then what the producer wrote on the
      // timestamp, each as Repack's own note.
      note(start, here(), boundary(clip, script.words, script.corrections));
      for (const written of clip.notes ?? []) note(start, here(), written.text);
    });
    if (block.clips.length) append(inline ? " " : "\n\n");
    // A reused case with nothing written on it still prints a word for its link's comment.
    const narration = block.cue?.clip.narration?.trim() || (block.cue?.clip.reuse ? REUSE_MARK : "");
    if (block.cue && narration && isCutIn(block.cue.clip)) {
      // Running on, a cut-in with no footage before it still needs a space after what came before.
      if (inline && content.text && !/\s$/.test(content.text)) append(" ");
      const from = here();
      if (block.cue.clip.reuse) note(from, from + narration.length, reuseNote(script, block.cue.clip.reuse));
      for (const [a, b] of blurSpans(narration)) note(from + a, from + b, BLUR_NOTE);
      for (const written of block.cue.clip.notes ?? []) {
        const anchor = written.anchor;
        const span = anchor ? noteRange(narration, anchor) : { start: 0, end: narration.length };
        if (span) note(from + span.start, from + span.end, written.text);
        else note(from, from + narration.length, `${anchor ? `“${anchor.quote}”\n` : ""}${written.text}`);
      }
      append(running ? narration : `${narration}\n\n`);
    }
  }
  if (!content.text.endsWith("\n")) append("\n");
  if (script.kind === "courtroom") append(`\nTotal length of clips: ${duration(keptMs(script.clips.filter((clip) => !isCutIn(clip))))}\n`);
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
      // Where each narration piece starts in the Doc, less what trimming took off its front.
      const pieceAt = new Map<number, number>();
      for (const [position, piece] of paragraph.pieces.entries()) {
        if (position) append(isClip(piece) && isClip(paragraph.pieces[position - 1]) ? " + " : " ");
        if (isClip(piece)) {
          clip(piece);
          continue;
        }
        const text = piece.text.trim();
        const from = here();
        pieceAt.set(position, from - (piece.text.length - piece.text.trimStart().length));
        for (const [a, b] of blurSpans(text, true)) note(from + a, from + b, CENSOR_NOTE);
        append(text);
        said.push([from, here()]);
      }
      // Each note on the words it's about; a quote pop-up as an edit: note, which Edit's agent carries out.
      const marked = new Set<number>();
      for (const { mark, piece, start: a, end: b, line } of liveMarks(paragraph)) {
        const at = pieceAt.get(piece);
        if (at === undefined) continue;
        marked.add(line);
        note(at + a, at + b, mark.quote ? `edit: ${mark.line.trim()}` : `For editor: ${mark.line.trim()}`);
      }
      // The rest sit on the paragraph's first words, as a comment.
      const rest = noteLines(paragraph).filter((_, index) => !marked.has(index));
      if (rest.length) {
        const [a, b] = said[0] ?? [start, here()];
        note(a, b, `For editor: ${rest.join("\n")}`);
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

/** The script as text, laid out as its Doc is, for the clipboard. */
export function scriptText(script: Script) {
  return script.commentary ? docContent(script).text : footageContent(script, true).text;
}
