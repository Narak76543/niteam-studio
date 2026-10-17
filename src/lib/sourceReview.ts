// Review screen helpers ("From document", docs/10-document-source.md §6).

import type { ReviewEdit, SourceSection, SourceSummary } from "./types";

/** Every item's tick and note, key points in their current order. */
export function toReviewEdit(s: SourceSummary): ReviewEdit {
  return {
    key_point_order: s.key_points.map((k) => k.id),
    items: [...s.key_points, ...s.facts, ...s.terms].map((i) => ({ id: i.id, selected: i.selected, note: i.note })),
  };
}

/** Moves key point `id` one place up (-1) or down (+1). */
export function moveKeyPoint(s: SourceSummary, id: string, by: -1 | 1): SourceSummary {
  const i = s.key_points.findIndex((k) => k.id === id);
  const j = i + by;
  const moved = s.key_points[i];
  if (!moved || j < 0 || j >= s.key_points.length) return s;
  const key_points = s.key_points.filter((_, n) => n !== i);
  key_points.splice(j, 0, moved);
  return { ...s, key_points };
}

/** Updates one item (key point, fact or term) by id. */
export function updateItem(
  s: SourceSummary,
  id: string,
  patch: Partial<{ selected: boolean; note: string }>,
): SourceSummary {
  const map = (list: SourceSummary["key_points"]) => list.map((i) => (i.id === id ? { ...i, ...patch } : i));
  return { ...s, key_points: map(s.key_points), facts: map(s.facts), terms: map(s.terms) };
}

/** Section notes that cite page `page`. */
export function notesForPage(sections: SourceSection[], page: number): SourceSection[] {
  return sections.filter((s) => s.pages.includes(page));
}

/** "p. 3, 4" → compact "3–5, 8" style ranges. */
export function pageRanges(pages: number[]): string {
  const sorted = [...new Set(pages)].sort((a, b) => a - b);
  const runs: [number, number][] = [];
  for (const p of sorted) {
    const last = runs[runs.length - 1];
    if (last && p === last[1] + 1) last[1] = p;
    else runs.push([p, p]);
  }
  return runs.map(([a, b]) => (a === b ? `${a}` : `${a}–${b}`)).join(", ");
}
