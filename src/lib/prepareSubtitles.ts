// Export step: asks Rust which scenes need subtitle images, draws one PNG per subtitle state
// (cue + spoken word) and hands them back with `subs_save`. Only changed scenes are redrawn.

import { api } from "./api";
import { bandFor, drawCuePng, ensureFont, toBase64 } from "./subtitleCanvas";
import { cueText, subtitleStates } from "./subtitleStates";
import type { CueIn, Language } from "./types";

export async function prepareSubtitles(
  projectId: string,
  lang: Language,
  resolution: string,
  highlight: string | null,
  onProgress: (done: number, total: number) => void,
  isCancelled: () => boolean,
): Promise<void> {
  const tasks = await api.subsPlan(projectId);
  onProgress(0, tasks.length);
  if (tasks.length === 0) return;
  const band = bandFor(resolution);
  await ensureFont(tasks[0]?.narration.slice(0, 40) || "ក", band);

  let done = 0;
  for (const task of tasks) {
    if (isCancelled()) return;
    const cues: CueIn[] = [];
    for (const st of subtitleStates(task, lang, highlight !== null)) {
      const png = await drawCuePng(st.lines, st.active, { highlight }, band);
      cues.push({ text: cueText(st.lines), start_ms: st.start_ms, end_ms: st.end_ms, png_base64: toBase64(png) });
    }
    if (cues.length > 0) await api.subsSave(projectId, task.scene_id, task.hash, cues);
    done += 1;
    onProgress(done, tasks.length);
  }
}
