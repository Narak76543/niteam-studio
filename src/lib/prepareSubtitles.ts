// Step 5 driver: asks Rust which scenes need subtitles, splits + times + draws them,
// and hands the PNGs back with `subs_save`. Only changed scenes are redrawn.

import { api } from "./api";
import { bandFor, drawCue, ensureFont, toBase64 } from "./subtitleCanvas";
import { groupCues, splitLines } from "./subtitles";
import { timeLines } from "./subtitleTiming";
import type { CueIn, Language, SubsTask } from "./types";

/** Cues (text + timing) for one scene; exported for tests. */
export function planCues(task: SubsTask, lang: Language): { lines: string[]; start_ms: number; end_ms: number }[] {
  const lines = splitLines(task.narration, lang);
  if (lines.length === 0) return [];
  const timings = timeLines(lines, task.words, task.duration_ms);
  const cues: { lines: string[]; start_ms: number; end_ms: number }[] = [];
  let i = 0;
  for (const group of groupCues(lines)) {
    const first = timings[i];
    const last = timings[i + group.length - 1];
    i += group.length;
    if (!first || !last) continue;
    cues.push({ lines: group, start_ms: first.start_ms, end_ms: last.end_ms });
  }
  return cues;
}

/**
 * Draws and saves subtitles for every scene that needs them.
 * `onProgress(done, total)` is called after each scene.
 */
export async function prepareSubtitles(
  projectId: string,
  lang: Language,
  resolution: string,
  onProgress: (done: number, total: number) => void,
  isCancelled: () => boolean,
): Promise<void> {
  const tasks = await api.subsPlan(projectId);
  onProgress(0, tasks.length);
  if (tasks.length === 0) return;
  const band = bandFor(resolution);
  await ensureFont(lang, tasks[0]?.narration.slice(0, 40) || "ក", band);

  let done = 0;
  for (const task of tasks) {
    if (isCancelled()) return;
    const cues: CueIn[] = [];
    for (const cue of planCues(task, lang)) {
      const png = await drawCue(cue.lines, lang, band);
      cues.push({
        text: cue.lines.join("\n"),
        start_ms: cue.start_ms,
        end_ms: cue.end_ms,
        png_base64: toBase64(png),
      });
    }
    if (cues.length > 0) await api.subsSave(projectId, task.scene_id, task.hash, cues);
    done += 1;
    onProgress(done, tasks.length);
  }
}
