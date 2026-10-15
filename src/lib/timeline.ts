// Editor timeline helpers (scene times come from `render_timeline`).

import type { TimelineScene } from "./types";

export function totalMs(scenes: TimelineScene[]): number {
  const last = scenes.at(-1);
  return last ? last.start_ms + last.clip_ms : 0;
}

/** Index of the scene playing at `ms` (binary search on start times). */
export function sceneIndexAt(scenes: TimelineScene[], ms: number): number {
  let lo = 0;
  let hi = scenes.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if ((scenes[mid]?.start_ms ?? 0) <= ms) lo = mid;
    else hi = mid - 1;
  }
  return Math.max(0, lo);
}

/** mm:ss */
export function formatTime(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/**
 * Timeline auto-follow: the new `scrollLeft` that brings the playhead back into view, or
 * `null` when it is already visible. `px` = playhead position in content pixels (including
 * the sticky label column of width `labelW`, which covers the left edge of the view).
 */
export function followScroll(px: number, scrollLeft: number, clientWidth: number, labelW: number): number | null {
  const left = scrollLeft + labelW;
  const right = scrollLeft + clientWidth - 40;
  if (px >= left && px <= right) return null;
  // Put the playhead a third into the visible track area.
  return Math.max(0, px - labelW - (clientWidth - labelW) / 3);
}
