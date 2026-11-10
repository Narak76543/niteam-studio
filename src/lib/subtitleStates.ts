// Subtitle *states* of one scene: which cue (≤ 2 lines) is on screen and which word is being
// spoken, with start/end times. Used by the editor preview and by the export (one PNG per
// state), so both show exactly the same thing.
//
// The word box glides to the next word on the same line (docs/04-khmer.md §3 "Sliding
// highlight"): the first SLIDE_FRAMES − 1 video frames of a word are in-between states with
// the box part-way from the previous word, then the word's own state holds until it ends.

import { groupCues, splitLineWords, lineText, maxGraphemes, type Word } from "./subtitles";
import { timeLines } from "./subtitleTiming";
import type { Aspect, Language, WordTiming } from "./types";

/** The box on its way from word `from` to the active word; `t` in (0, 1). */
export interface Slide {
  from: number;
  t: number;
}

export interface SubtitleState {
  /** Lines of the cue on screen. */
  lines: Word[][];
  /** Spoken word (index over all words of the cue), null when the tracker is off. */
  active: number | null;
  /** In-between frame of the box gliding to `active`; null when it is at rest. */
  slide: Slide | null;
  start_ms: number;
  end_ms: number;
}

/** One video frame of the render (`render::clip::FPS` = 30). */
const FRAME_MS = 1000 / 30;
/** The glide takes this many frames (~100 ms); the last one is the word's own state. */
export const SLIDE_FRAMES = 3;
/** Shorter words jump: the glide needs its frames plus a moment at rest. */
const MIN_SLIDE_WORD_MS = (SLIDE_FRAMES + 2) * FRAME_MS;

/** Slow-in, slow-out (smoothstep): with only two in-between frames it reads as a glide. */
function ease(x: number): number {
  return x * x * (3 - 2 * x);
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
        const lineOf = cue.flatMap((l, li) => l.map(() => li));
        for (let k = 0; k < count; k++) {
          const t = times[offset + k];
          if (!t) continue;
          let start = t.start_ms;
          const glide = k > 0 && lineOf[k] === lineOf[k - 1] && t.end_ms - t.start_ms >= MIN_SLIDE_WORD_MS;
          if (glide) {
            for (let j = 1; j < SLIDE_FRAMES; j++) {
              const end = t.start_ms + Math.round(j * FRAME_MS);
              states.push({ lines: cue, active: k, slide: { from: k - 1, t: ease(j / SLIDE_FRAMES) }, start_ms: start, end_ms: end });
              start = end;
            }
          }
          states.push({ lines: cue, active: k, slide: null, start_ms: start, end_ms: t.end_ms });
        }
      } else {
        states.push({ lines: cue, active: null, slide: null, start_ms: first.start_ms, end_ms: last.end_ms });
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
