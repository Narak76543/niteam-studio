// Real-world scenes (docs/11 §10): asks Rust which diagram / code pictures are missing or
// outdated, draws them in the WebView and hands the PNGs back. A drawing that fails (e.g.
// Mermaid can't parse) is stored as the scene's error until its text changes.

import { api, errorMessage } from "./api";
import { drawCode } from "./codeRender";
import { drawDiagram } from "./diagramRender";
import { toBase64 } from "./subtitleCanvas";
import type { DrawTask } from "./types";

async function draw(task: DrawTask): Promise<{ still: Uint8Array<ArrayBuffer>; frames: Uint8Array<ArrayBuffer>[] }> {
  if (task.kind === "diagram" && task.diagram) {
    return { still: await drawDiagram(task.diagram.mermaid, task.width, task.height, task.band_top), frames: [] };
  }
  if (task.kind === "code" && task.code) return drawCode(task.code, task.width, task.height, task.band_top);
  throw new Error("nothing to draw");
}

export async function prepareVisuals(
  projectId: string,
  onProgress?: (done: number, total: number) => void,
  isCancelled?: () => boolean,
): Promise<{ drawn: number; failed: number }> {
  const tasks = await api.visualsDrawPlan(projectId);
  let drawn = 0;
  let failed = 0;
  onProgress?.(0, tasks.length);
  for (const task of tasks) {
    if (isCancelled?.()) break;
    try {
      const { still, frames } = await draw(task);
      await api.visualDrawSave(projectId, task.scene_id, task.hash, toBase64(still), frames.map(toBase64));
      drawn += 1;
    } catch (e) {
      failed += 1;
      await api.visualDrawFailed(projectId, task.scene_id, task.hash, errorMessage(e)).catch(() => undefined);
    }
    onProgress?.(drawn + failed, tasks.length);
  }
  return { drawn, failed };
}
