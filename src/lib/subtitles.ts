// Subtitle line breaking (docs/04-khmer.md §2).
//
// Khmer has no spaces between words: the browser's word segmenter (Khmer dictionary data)
// finds word boundaries, spaces / zero-width spaces mark phrase pauses, `។ ៕ ! ?` end
// sentences. Words are packed greedily into lines of at most ~38 graphemes (42 for
// English), preferring a phrase/sentence boundary near the end of the line. Lines are never
// broken inside a word or a grapheme cluster, and punctuation never starts a line.

import type { Language } from "./types";

export const MAX_GRAPHEMES: Record<Language, number> = { km: 38, en: 42 };
/** Max lines on screen at once (one subtitle image). */
export const LINES_PER_CUE = 2;
/** Look for a phrase boundary in the last 30 % of a full line. */
const PREFER_FROM = 0.7;
/** A line that is just one word this short is merged with its neighbour. */
const ORPHAN_GRAPHEMES = 3;

const ZWSP = "​";
const SENTENCE_END = /[។៕!?.]$/u;

const graphemeSeg = new Intl.Segmenter("km", { granularity: "grapheme" });
const wordSegs: Record<Language, Intl.Segmenter> = {
  km: new Intl.Segmenter("km", { granularity: "word" }),
  en: new Intl.Segmenter("en", { granularity: "word" }),
};

export function graphemeCount(text: string): number {
  return Array.from(graphemeSeg.segment(text)).length;
}

/** 0 = word boundary, 1 = phrase (space), 2 = sentence end. */
type Strength = 0 | 1 | 2;

interface Token {
  text: string;
  /** Boundary strength *before* this token. */
  before: Strength;
  /** A space separated it from the previous token (re-inserted when joined). */
  spaced: boolean;
}

function tokenize(text: string, lang: Language): Token[] {
  const tokens: Token[] = [];
  let pendingStrength: Strength = 0;
  let pendingSpace = false;
  // Zero-width spaces are break hints: turn them into a marker the segmenter keeps apart.
  const parts = text.split(ZWSP);
  parts.forEach((part, pi) => {
    if (pi > 0) pendingStrength = Math.max(pendingStrength, 1) as Strength;
    for (const { segment, isWordLike } of wordSegs[lang].segment(part)) {
      if (/^\s+$/u.test(segment)) {
        pendingStrength = Math.max(pendingStrength, 1) as Strength;
        pendingSpace = true;
        continue;
      }
      const prev = tokens.at(-1);
      // Punctuation sticks to the word before it, so it never starts a line.
      if (!isWordLike && prev && !pendingSpace && pendingStrength === 0) {
        prev.text += segment;
        continue;
      }
      if (!isWordLike && prev && /^\p{P}+$/u.test(segment) && !/^[«“"'(]/u.test(segment)) {
        prev.text += (pendingSpace ? " " : "") + segment;
        pendingSpace = false;
        continue;
      }
      const sentence = prev !== undefined && SENTENCE_END.test(prev.text);
      tokens.push({
        text: segment,
        before: sentence ? 2 : pendingStrength,
        spaced: pendingSpace,
      });
      pendingStrength = 0;
      pendingSpace = false;
    }
  });
  if (tokens[0]) tokens[0].before = 0;
  return tokens;
}

function join(tokens: Token[]): string {
  return tokens.map((t, i) => (i > 0 && t.spaced ? ` ${t.text}` : t.text)).join("");
}

function width(tokens: Token[]): number {
  return graphemeCount(join(tokens));
}

/** Splits a single over-long token at grapheme boundaries. */
function splitLong(token: Token, max: number): Token[] {
  const out: Token[] = [];
  let cur = "";
  let n = 0;
  for (const { segment } of graphemeSeg.segment(token.text)) {
    if (n === max) {
      out.push({ text: cur, before: out.length === 0 ? token.before : 0, spaced: out.length === 0 && token.spaced });
      cur = "";
      n = 0;
    }
    cur += segment;
    n += 1;
  }
  if (cur) out.push({ text: cur, before: out.length === 0 ? token.before : 0, spaced: out.length === 0 && token.spaced });
  return out;
}

function pack(tokens: Token[], max: number): Token[][] {
  const lines: Token[][] = [];
  let line: Token[] = [];
  for (const tok of tokens.flatMap((t) => (graphemeCount(t.text) > max ? splitLong(t, max) : [t]))) {
    if (line.length === 0 || width([...line, tok]) <= max) {
      line.push(tok);
      continue;
    }
    // Line is full: break at the strongest boundary in its last 30 %, else before `tok`.
    let breakAt = -1;
    let best: Strength = 0;
    for (let j = line.length - 1; j > 0; j--) {
      const t = line[j];
      if (!t || width(line.slice(0, j)) < max * PREFER_FROM) break;
      if (t.before > best) {
        best = t.before;
        breakAt = j;
      }
    }
    if (breakAt > 0) {
      lines.push(line.slice(0, breakAt));
      line = [...line.slice(breakAt), tok];
      if (width(line) > max) {
        lines.push(line.slice(0, -1));
        line = [tok];
      }
    } else {
      lines.push(line);
      line = [tok];
    }
  }
  if (line.length > 0) lines.push(line);
  return lines;
}

/** Moves a short single-word line together with a word from its neighbour when it fits. */
function fixOrphans(lines: Token[][], max: number): Token[][] {
  for (let i = 1; i < lines.length; i++) {
    const cur = lines[i];
    const prev = lines[i - 1];
    if (!cur || !prev || cur.length !== 1 || width(cur) > ORPHAN_GRAPHEMES) continue;
    if (width([...prev, ...cur]) <= max) {
      lines[i - 1] = [...prev, ...cur];
      lines.splice(i, 1);
      i -= 1;
    } else if (prev.length > 1) {
      const moved = prev.at(-1);
      if (moved && width([moved, ...cur]) <= max) {
        lines[i - 1] = prev.slice(0, -1);
        lines[i] = [moved, ...cur];
      }
    }
  }
  return lines;
}

/** Display lines for one scene's narration (NFC, ≤ max graphemes, never inside a word). */
export function splitLines(text: string, lang: Language, max = MAX_GRAPHEMES[lang]): string[] {
  const clean = text.normalize("NFC").replace(/[\r\n\t]+/gu, " ").trim();
  if (!clean) return [];
  const tokens = tokenize(clean, lang);
  return fixOrphans(pack(tokens, max), max)
    .map((l) => join(l).replace(new RegExp(ZWSP, "gu"), "").trim())
    .filter((l) => l.length > 0);
}

/** Groups display lines into on-screen cues of at most `LINES_PER_CUE` lines. */
export function groupCues<T>(lines: T[], perCue = LINES_PER_CUE): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < lines.length; i += perCue) out.push(lines.slice(i, i + perCue));
  return out;
}
