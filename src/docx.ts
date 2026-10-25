// Repack's Word writer (apps/web/src/domain/script-docx.ts), with bold lines
// for a commentary's section headings.

/** A script laid out for Docs: its text, styled ranges and comments, in Docs' 1-based UTF-16 indexes. */
export type DocContent = {
  text: string;
  layout?: { fontSizePt: number; headingSizePt: number; paragraphSpacePt: number; page: "A4" };
  styles: { start: number; end: number; url?: string; title?: boolean; bold?: boolean; asset?: boolean; divider?: boolean }[];
  notes: { start: number; end: number; text: string }[];
};

export const DOCX_MIME =
  "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

const W =
  'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"';
const RELS =
  "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';

/** Characters XML cannot carry are dropped rather than failing the whole sync. */
const xml = (value: string) =>
  value
    .replace(/[^\t\n\r\u0020-\uD7FF\uE000-\uFFFD\u{10000}-\u{10FFFF}]/gu, "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");

const run = (value: string, props = "") =>
  value
    ? `<w:r>${props ? `<w:rPr>${props}</w:rPr>` : ""}${value
        .split("\t")
        .map((part) =>
          part ? `<w:t xml:space="preserve">${xml(part)}</w:t>` : "",
        )
        .join("<w:tab/>")}</w:r>`
    : "";

const CRC = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

/** Uncompressed archive: these documents are a few kilobytes. */
function zip(files: [path: string, text: string][]) {
  const encoder = new TextEncoder();
  const parts: Uint8Array[] = [];
  const directory: Uint8Array[] = [];
  let offset = 0;
  for (const [path, text] of files) {
    const name = encoder.encode(path);
    const data = encoder.encode(text);
    let crc = 0xffffffff;
    for (const byte of data) crc = CRC[(crc ^ byte) & 0xff] ^ (crc >>> 8);
    // Fields shared by the local header and the central directory entry.
    const meta = new DataView(new ArrayBuffer(26));
    meta.setUint16(0, 20, true);
    meta.setUint16(2, 0x0800, true); // UTF-8 names
    meta.setUint16(8, 33, true); // 1980-01-01; a zero date is invalid
    meta.setUint32(10, (crc ^ 0xffffffff) >>> 0, true);
    meta.setUint32(14, data.length, true);
    meta.setUint32(18, data.length, true);
    meta.setUint16(22, name.length, true);
    const shared = new Uint8Array(meta.buffer);
    const tail = new DataView(new ArrayBuffer(14));
    tail.setUint32(10, offset, true);
    parts.push(Uint8Array.of(0x50, 0x4b, 3, 4), shared, name, data);
    directory.push(
      Uint8Array.of(0x50, 0x4b, 1, 2, 20, 0),
      shared,
      new Uint8Array(tail.buffer),
      name,
    );
    offset += 30 + name.length + data.length;
  }
  const size = directory.reduce((sum, part) => sum + part.length, 0);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(8, files.length, true);
  end.setUint16(10, files.length, true);
  end.setUint32(12, size, true);
  end.setUint32(16, offset, true);
  const all = [...parts, ...directory, new Uint8Array(end.buffer)];
  const bytes = new Uint8Array(offset + size + 22);
  let at = 0;
  for (const part of all) {
    bytes.set(part, at);
    at += part.length;
  }
  return bytes;
}

/**
 * Docs only anchors comments that arrive inside an imported file, so the
 * script is uploaded as a Word document whose comments wrap their timestamps.
 */
export function scriptDocx(
  content: DocContent,
  author: string,
  updatedAt: string,
) {
  // Use the revision's real date; an omitted date imports as 1 Jan 1970.
  const commentDate = new Date(updatedAt)
    .toISOString()
    .replace(/\.\d{3}Z$/, "Z");
  const links: string[] = [];
  const paragraphs: string[] = [];
  const linkProps = '<w:color w:val="1155CC"/><w:u w:val="single"/>';
  // Style and note indexes are 1-based, as Docs counts them.
  let offset = 1;
  for (const line of content.text.replace(/\n$/, "").split("\n")) {
    const end = offset + line.length;
    const styles = content.styles.filter(
      (style) => style.start >= offset && style.start < end,
    );
    const props = styles.some((style) => style.title)
      ? '<w:b/><w:sz w:val="38"/><w:szCs w:val="38"/>'
      : styles.some((style) => style.bold)
        ? `<w:b/>${content.layout ? `<w:sz w:val="${content.layout.headingSizePt * 2}"/><w:szCs w:val="${content.layout.headingSizePt * 2}"/>` : ""}`
        : "";
    const linked = styles.filter((style) => style.url);
    // The line is cut wherever a link or a comment starts or stops.
    const cuts = new Set([offset, end]);
    for (const link of linked) {
      cuts.add(link.start);
      cuts.add(Math.min(link.end, end));
    }
    for (const note of content.notes)
      for (const edge of [note.start, note.end])
        if (edge > offset && edge < end) cuts.add(edge);
    const edges = [...cuts].sort((a, b) => a - b);
    let runs = "";
    for (let i = 0; i < edges.length; i++) {
      const from = edges[i];
      // Comments open before the text they cover and close after it.
      content.notes.forEach((note, id) => {
        if (note.end === from)
          runs += `<w:commentRangeEnd w:id="${id}"/><w:r><w:commentReference w:id="${id}"/></w:r>`;
      });
      content.notes.forEach((note, id) => {
        if (note.start === from && note.start < end)
          runs += `<w:commentRangeStart w:id="${id}"/>`;
      });
      const to = edges[i + 1];
      if (to === undefined) break;
      const piece = line.slice(from - offset, to - offset);
      const link = linked.find((l) => l.start <= from && l.end >= to);
      if (link?.url) {
        links.push(link.url);
        runs += `<w:hyperlink r:id="rId${links.length + 2}">${run(piece, linkProps)}</w:hyperlink>`;
      } else runs += run(piece, props);
    }
    const houseHeading = content.layout && styles.some((style) => style.bold);
    const houseParagraph = houseHeading
      ? `<w:pPr><w:keepNext/><w:spacing w:before="280" w:after="80"/>${/^\d+\.\t/.test(line) ? '<w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs><w:ind w:left="720" w:hanging="360"/>' : ""}</w:pPr>`
      : "";
    paragraphs.push(
      `<w:p>${houseParagraph || (styles.some((style) => style.divider) ? '<w:pPr><w:keepNext/><w:pBdr><w:top w:val="single" w:sz="6" w:space="0" w:color="999999"/></w:pBdr><w:spacing w:after="240"/></w:pPr>' : styles.some((style) => style.asset) ? '<w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr>' : "")}${runs}</w:p>`,
    );
    offset = end + 1;
  }
  const initials = author
    .split(/\s+/)
    .map((word) => word[0] ?? "")
    .join("")
    .toUpperCase();
  return zip([
    [
      "[Content_Types].xml",
      `${XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="${DOCX_MIME}.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>`,
    ],
    [
      "_rels/.rels",
      `${XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/officeDocument" Target="word/document.xml"/></Relationships>`,
    ],
    [
      "word/document.xml",
      `${XML}<w:document ${W} xmlns:r="${RELS}"><w:body>${paragraphs.join("")}<w:sectPr><w:pgSz w:w="${content.layout?.page === "A4" ? 11906 : 12240}" w:h="${content.layout?.page === "A4" ? 16838 : 15840}"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>`,
    ],
    [
      "word/_rels/document.xml.rels",
      `${XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="${RELS}/styles" Target="styles.xml"/><Relationship Id="rId2" Type="${RELS}/comments" Target="comments.xml"/>${links.map((url, i) => `<Relationship Id="rId${i + 3}" Type="${RELS}/hyperlink" Target="${xml(url)}" TargetMode="External"/>`).join("")}</Relationships>`,
    ],
    [
      "word/styles.xml",
      `${XML}<w:styles ${W}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:eastAsia="Arial" w:cs="Arial"/><w:sz w:val="${(content.layout?.fontSizePt ?? 12) * 2}"/><w:szCs w:val="${(content.layout?.fontSizePt ?? 12) * 2}"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="${(content.layout?.paragraphSpacePt ?? 0) * 20}" w:after="${(content.layout?.paragraphSpacePt ?? 0) * 20}" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>`,
    ],
    [
      "word/comments.xml",
      `${XML}<w:comments ${W}>${content.notes.map((note, i) => `<w:comment w:id="${i}" w:author="${xml(author)}" w:initials="${xml(initials)}" w:date="${commentDate}"><w:p><w:r><w:t xml:space="preserve">${xml(note.text)}</w:t></w:r></w:p></w:comment>`).join("")}</w:comments>`,
    ],
  ]);
}
