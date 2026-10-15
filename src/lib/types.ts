// Mirrors the Rust serde types (src-tauri/src/**). Field names are snake_case on the wire.

export type Language = "km" | "en";

export type ProjectStatus = "script_draft" | "generating" | "review" | "rendering" | "done";

export type AssetStatus = "pending" | "running" | "done" | "failed" | "stale";

export interface ModelSettings {
  text: string;
  tts: string;
  tts_lite: string;
  image: string;
}

export interface ConcurrencySettings {
  text: number;
  tts: number;
  image: number;
}

export interface Settings {
  schema_version: number;
  ui_language: Language | null;
  output_dir: string | null;
  models: ModelSettings;
  concurrency: ConcurrencySettings;
}

export interface KeyTestResult {
  model: string;
  display_name: string | null;
}

export interface StylePreset {
  id: string;
  label: string;
  style_prompt: string;
}

export interface Style {
  preset: string;
  style_prompt: string;
}

export interface VoiceSettings {
  model: string;
  voice_name: string;
  direction: string;
}

export interface OutlineChapter {
  title: string;
  summary: string;
  scene_count: number;
  done: boolean;
}

export interface Outline {
  title: string;
  hook: string;
  chapters: OutlineChapter[];
}

export interface VoiceAsset {
  status: AssetStatus;
  path: string | null;
  duration_ms: number | null;
  error: string | null;
  text_hash: string | null;
}

export interface ImageAsset {
  status: AssetStatus;
  path: string | null;
  error: string | null;
  prompt_hash: string | null;
}

export interface SubLine {
  text: string;
  start_ms: number;
  end_ms: number;
  path: string;
}

export interface SubsAsset {
  status: AssetStatus;
  lines: SubLine[];
}

export interface ClipAsset {
  status: AssetStatus;
  path: string | null;
}

export interface Scene {
  id: string;
  order: number;
  chapter: number;
  narration: string;
  spoken_text: string;
  image_prompt: string;
  mood: string;
  fact_note: string | null;
  voice: VoiceAsset;
  image: ImageAsset;
  subs: SubsAsset;
  clip: ClipAsset;
}

export interface RenderSettings {
  resolution: string;
  music: string | null;
  music_volume: number;
  encoder: string | null;
  status: string;
  output_path: string | null;
}

export interface Project {
  schema_version: number;
  id: string;
  title: string;
  topic: string;
  language: Language;
  target_minutes: number;
  style: Style;
  voice: VoiceSettings;
  status: ProjectStatus;
  created_at: string;
  updated_at: string;
  outline: Outline | null;
  scenes: Scene[];
  render: RenderSettings;
}

export interface ProjectSummary {
  id: string;
  title: string;
  topic: string;
  language: Language;
  target_minutes: number;
  style_preset: string;
  status: ProjectStatus;
  scene_count: number;
  updated_at: string;
}

export interface NewProjectInput {
  topic: string;
  language: Language;
  target_minutes: number;
  style_preset: string;
}

export interface SceneEdit {
  id: string;
  narration: string;
  spoken_text: string;
  image_prompt: string;
  mood: string;
}

export type ScriptStage = "outline" | "chapter" | "done" | "failed" | "cancelled";

/** Payload of the `script-progress` event. */
export interface ScriptProgress {
  project_id: string;
  stage: ScriptStage;
  chapters_done: number;
  chapters_total: number;
  error: string | null;
}

/** Must match `gemini::script::MOODS`. */
export const MOODS = [
  "curious",
  "dramatic",
  "calm",
  "hopeful",
  "tense",
  "joyful",
  "somber",
  "awe",
  "playful",
] as const;
