// Plain-text script for external review (ChatGPT, Gemini, …) before approval.

import type { Language, Outline, Scene } from "./types";

type ReviewScene = Pick<Scene, "order" | "chapter" | "narration" | "spoken_text" | "fact_note">;

type ReviewProject = {
  title: string;
  language: Language;
  target_minutes: number;
  outline: Outline | null;
};

const LANGUAGES: Record<Language, string> = { km: "Khmer", en: "English" };

/**
 * Whole script, scene by scene, with chapter headings. Labels stay in English so any
 * assistant understands the structure even when the script itself is Khmer.
 */
export function scriptReviewText(project: ReviewProject, scenes: ReviewScene[]): string {
  const header = [
    project.title.trim(),
    `${LANGUAGES[project.language]} · ${project.target_minutes} min · ${scenes.length} scene${
      scenes.length === 1 ? "" : "s"
    }`,
  ]
    .filter(Boolean)
    .join("\n");

  const blocks: string[] = [];
  let chapter = -1;
  for (const scene of scenes) {
    const lines: string[] = [];
    if (scene.chapter !== chapter) {
      chapter = scene.chapter;
      const title = project.outline?.chapters[chapter]?.title.trim();
      lines.push(title ? `Chapter ${chapter + 1} — ${title}` : `Chapter ${chapter + 1}`);
    }
    lines.push(`Scene ${scene.order}`, `Narration: ${scene.narration.trim()}`);
    lines.push(`Spoken: ${scene.spoken_text.trim()}`);
    const fact = scene.fact_note?.trim();
    if (fact) lines.push(`Fact check: ${fact}`);
    blocks.push(lines.join("\n"));
  }

  return [header, ...blocks].join("\n\n") + "\n";
}
