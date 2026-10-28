// Subtitle *states* of one scene: which cue (≤ 2 lines) is on screen and which word is being
// spoken, with start/end times. Used by the editor preview and by the export (one PNG per
// state), so both show exactly the same thing.

import { groupCues, splitLineWords, lineText, maxGraphemes, type Word } from "./subtitles";
import { timeLines } from "./subtitleTiming";
import type { Aspect, Language, WordTiming } from "./types";

export interface SubtitleState {
  /** Lines of the cue on screen. */
  lines: Word[][];
  /** Spoken word (index over all words of the cue), null when the tracker is off. */
  active: number | null;
  start_ms: number;
  end_ms: number;
}

export interface SceneSubtitleInput {
  narration: string;
  /** Voice file length: timings run over the whole file. */
  duration_ms: number;
  words: WordTiming[] | null;
}

/**
 * Consecutive states covering [0, duration]. Word times come from the voice timings (Edge
 * phrase timings, interpolated by grapheme count inside a phrase) or, without them, from
 * each word's share of the text.
 */
export function subtitleStates(scene: SceneSubtitleInput, lang: Language, tracker: boolean, aspect: Aspect = "16:9"): SubtitleState[] {
  const lines = splitLineWords(scene.narration, lang, maxGraphemes(lang, aspect));
  if (lines.length === 0 || scene.duration_ms <= 0) return [];
  const flat = lines.flat();
  const times = timeLines(
    flat.map((w) => w.text),
    scene.words,
    scene.duration_ms,
  );
  const states: SubtitleState[] = [];
  let offset = 0;
  for (const cue of groupCues(lines)) {
    const count = cue.reduce((n, l) => n + l.length, 0);
    const first = times[offset];
    const last = times[offset + count - 1];
    if (first && last) {
      if (tracker) {
        for (let k = 0; k < count; k++) {
          const t = times[offset + k];
          if (t) states.push({ lines: cue, active: k, start_ms: t.start_ms, end_ms: t.end_ms });
        }
      } else {
        states.push({ lines: cue, active: null, start_ms: first.start_ms, end_ms: last.end_ms });
      }
    }
    offset += count;
  }
  return states;
}

/** The state on screen at `ms` (the last one stays until the scene ends). */
export function stateAt(states: SubtitleState[], ms: number): SubtitleState | null {
  if (states.length === 0) return null;
  let lo = 0;
  let hi = states.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    const s = states[mid];
    if (s && s.start_ms <= ms) lo = mid;
    else hi = mid - 1;
  }
  return states[lo] ?? null;
}

export function cueText(lines: Word[][]): string {
  return lines.map(lineText).join("\n");
}
