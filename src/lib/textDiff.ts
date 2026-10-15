// Grapheme-level diff for proofreading suggestions (Khmer has no spaces between words, and a
// change inside a cluster must show the whole cluster).

export interface DiffPart {
  text: string;
  kind: "same" | "del" | "ins";
}

const seg = new Intl.Segmenter("km", { granularity: "grapheme" });

function graphemes(s: string): string[] {
  return Array.from(seg.segment(s.normalize("NFC")), (g) => g.segment);
}

/** Longest-common-subsequence diff of two short texts (scene narration). */
export function diffText(before: string, after: string): DiffPart[] {
  const a = graphemes(before);
  const b = graphemes(after);
  const n = a.length;
  const m = b.length;
  // lcs[i][j] = LCS length of a[i..] and b[j..]
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    const row = lcs[i];
    const next = lcs[i + 1];
    if (!row || !next) continue;
    for (let j = m - 1; j >= 0; j--) {
      row[j] = a[i] === b[j] ? (next[j + 1] ?? 0) + 1 : Math.max(next[j] ?? 0, row[j + 1] ?? 0);
    }
  }
  const parts: DiffPart[] = [];
  const push = (text: string, kind: DiffPart["kind"]) => {
    const last = parts.at(-1);
    if (last && last.kind === kind) last.text += text;
    else parts.push({ text, kind });
  };
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      push(a[i] ?? "", "same");
      i++;
      j++;
    } else if (j < m && (i >= n || (lcs[i]?.[j + 1] ?? 0) >= (lcs[i + 1]?.[j] ?? 0))) {
      push(b[j] ?? "", "ins");
      j++;
    } else {
      push(a[i] ?? "", "del");
      i++;
    }
  }
  return parts;
}
