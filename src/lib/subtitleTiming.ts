// Subtitle line timing (docs/04-khmer.md §2, docs/03-pipeline.md Step 5).
//
// With word timings (Edge TTS `words.json`) each line boundary is placed at the moment the
// voice reaches that point of the text. Without them, time is split proportionally to
// grapheme count. Khmer timings from Edge are phrase-level (space-separated chunks), so
// positions inside a phrase are interpolated by grapheme count.
//
// Lines come from `narration`, timings from `spoken_text` (numbers spelled out), so the two
// texts can differ. Positions are therefore matched as *fractions* of each text's length.

import type { WordTiming } from "./types";

export interface LineTiming {
  start_ms: number;
  end_ms: number;
}

const graphemes = new Intl.Segmenter("km", { granularity: "grapheme" });
const SPOKEN = /[\p{L}\p{M}\p{N}]/u;

/** Number of letter/digit grapheme clusters (spaces and punctuation ignored). */
export function spokenLength(text: string): number {
  let n = 0;
  for (const { segment } of graphemes.segment(text.normalize("NFC"))) {
    if (SPOKEN.test(segment)) n += 1;
  }
  return n;
}

function proportional(weights: number[], durationMs: number): LineTiming[] {
  // Lines without letters (e.g. only punctuation) still get an equal share.
  const w = weights.every((x) => x === 0) ? weights.map(() => 1) : weights;
  const total = w.reduce((a, b) => a + b, 0);
  let acc = 0;
  return w.map((x, i) => {
    const start = Math.round((acc / total) * durationMs);
    acc += x;
    const end = i === w.length - 1 ? durationMs : Math.round((acc / total) * durationMs);
    return { start_ms: start, end_ms: end };
  });
}

/** Time (ms) at which the voice reaches grapheme position `pos` of the timed text. */
function timeAt(words: { len: number; start: number; end: number }[], pos: number): number {
  let seen = 0;
  for (let i = 0; i < words.length; i++) {
    const w = words[i];
    if (!w) continue;
    if (pos <= seen + w.len) {
      const within = w.len > 0 ? (pos - seen) / w.len : 0;
      // A boundary exactly at the end of a phrase belongs to the pause before the next one.
      const next = words[i + 1];
      if (within >= 1 && next) return Math.round((w.end + next.start) / 2);
      return Math.round(w.start + (w.end - w.start) * within);
    }
    seen += w.len;
  }
  return words.at(-1)?.end ?? 0;
}

/**
 * Start/end of each subtitle line. The first line starts at 0 and the last ends at
 * `durationMs`, so subtitles cover the whole scene.
 */
export function timeLines(
  lines: string[],
  words: WordTiming[] | null | undefined,
  durationMs: number,
): LineTiming[] {
  if (lines.length === 0) return [];
  const weights = lines.map(spokenLength);
  const timed = (words ?? [])
    .map((w) => ({ len: spokenLength(w.text), start: w.start_ms, end: w.end_ms }))
    .filter((w) => w.len > 0 && w.end >= w.start);
  const timedTotal = timed.reduce((a, w) => a + w.len, 0);
  const lineTotal = weights.reduce((a, b) => a + b, 0);
  if (timed.length === 0 || timedTotal === 0 || lineTotal === 0) {
    return proportional(weights, durationMs);
  }
  const result: LineTiming[] = [];
  let acc = 0;
  let start = 0;
  weights.forEach((w, i) => {
    acc += w;
    const isLast = i === weights.length - 1;
    const pos = (acc / lineTotal) * timedTotal;
    const end = isLast ? durationMs : Math.min(durationMs, Math.max(start, timeAt(timed, pos)));
    result.push({ start_ms: start, end_ms: end });
    start = end;
  });
  return result;
}
