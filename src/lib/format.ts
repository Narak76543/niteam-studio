import type { Language } from "./types";

/** Short local date, e.g. "15 Oct 2026" / Khmer month names in km. */
export function formatDate(iso: string, lang: Language): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  return new Intl.DateTimeFormat(lang === "km" ? "km-KH" : "en-GB", {
    day: "numeric",
    month: "short",
    year: "numeric",
  }).format(d);
}

/** Mirrors `script_job::plan_scenes`: ~7 s per scene, 3..200 scenes. */
export function estimateScenes(minutes: number): number {
  return Math.min(200, Math.max(3, Math.round((minutes * 60) / 7)));
}
