/** A stretch of a word diff: kept, taken out, or put in. */
export type Piece = { kind: "same" | "del" | "add"; text: string };

/**
 * The words that change between two paragraphs, as Repack shows a proposed
 * rewrite: the longest run of words both share stays, the rest is marked as
 * taken out or put in. Spaces travel with the word before them.
 */
export function wordDiff(before: string, after: string): Piece[] {
  const a = before.match(/\S+\s*/g) ?? [];
  const b = after.match(/\S+\s*/g) ?? [];
  const same = (x: string, y: string) => x.trim() === y.trim();
  // Longest common subsequence, from the end, so the walk below goes forward.
  const table = Array.from({ length: a.length + 1 }, () => new Uint16Array(b.length + 1));
  for (let i = a.length - 1; i >= 0; i--)
    for (let j = b.length - 1; j >= 0; j--)
      table[i][j] = same(a[i], b[j]) ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
  const out: Piece[] = [];
  const push = (kind: Piece["kind"], text: string) => {
    const last = out.at(-1);
    if (last?.kind === kind) last.text += text;
    else out.push({ kind, text });
  };
  let i = 0;
  let j = 0;
  while (i < a.length && j < b.length) {
    if (same(a[i], b[j])) {
      push("same", b[j]);
      i++;
      j++;
    } else if (table[i + 1][j] >= table[i][j + 1]) push("del", a[i++]);
    else push("add", b[j++]);
  }
  while (i < a.length) push("del", a[i++]);
  while (j < b.length) push("add", b[j++]);
  return out;
}
