import type { Project, ScriptTone } from "./types";

/** Mood words for the thumbnail, by script tone. */
const TONE_MOOD: Record<ScriptTone, string> = {
  funny: "playful and funny, a big exaggerated reaction on the character's face, bright cheerful colours",
  dramatic: "dramatic and intriguing, strong contrast, a sense of mystery or surprise",
  calm: "clean, calm and curious, soft contrast, a clear friendly mood",
};

const NO_TEXT_RULES =
  "ABSOLUTELY NO TEXT in the image: no words, letters, numbers, captions, subtitles, titles, signs, labels, " +
  "logos, watermarks, speech bubbles or UI elements anywhere. If something would normally carry writing " +
  "(a sign, a book, a screen), draw it blank or with abstract shapes instead.";

function clean(text: string, max: number): string {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > max ? `${one.slice(0, max - 1).trimEnd()}…` : one;
}

/** The scene whose picture best sells the video: the first key scene, else the first scene. */
function coverScene(project: Project) {
  const withPrompt = project.scenes.filter((s) => s.image_prompt.trim() !== "");
  return withPrompt.find((s) => s.key_scene) ?? withPrompt[0] ?? null;
}

/**
 * A ready-to-paste prompt (English) for ChatGPT or Gemini that makes a YouTube thumbnail
 * for this video. It never asks for text: the creator adds the title in an editor.
 */
export function buildThumbnailPrompt(project: Project): string {
  const title = project.outline?.title || project.title;
  const hook = project.outline?.hook ?? "";
  const scene = coverScene(project);
  const style = clean(project.style.style_prompt, 700);
  const idea = scene ? clean(scene.image_prompt, 500) : "";
  const bible = project.visual_bible;
  const culture = bible?.culture_notes ? clean(bible.culture_notes, 200) : "";

  const lines = [
    "Create ONE YouTube thumbnail image, 16:9 landscape (1280×720 px), for a narrated explainer video.",
    "",
    `VIDEO TOPIC (context only, never write it in the image): ${clean(project.topic, 300)}`,
    `VIDEO TITLE (context only, never write it in the image): ${clean(title, 200)}`,
  ];
  if (hook) lines.push(`WHAT THE VIDEO PROMISES: ${clean(hook, 300)}`);
  lines.push("", "THE PICTURE:");
  if (idea) lines.push(`- Main idea (from the video's key scene): ${idea}`);
  lines.push(
    "- One single clear focal subject that tells the topic at a glance, large and in the centre-left of the frame.",
    "- The main character (if any) shows one strong emotion (curious, surprised, excited) with big readable eyes and eyebrows.",
    "- Simple, uncluttered background with two or three props that belong ONLY to this topic.",
    "- High contrast and saturated colours so it stays readable as a small phone thumbnail; clean outlines, no tiny details.",
    "- Keep the bottom-right corner fairly empty (YouTube places the video length there).",
    `- Mood: ${TONE_MOOD[project.tone]}.`,
  );
  if (culture) lines.push(`- Look and setting: ${culture}`);
  lines.push("", `ART STYLE (match the video's pictures): ${style}`, "", NO_TEXT_RULES);
  return lines.join("\n");
}
