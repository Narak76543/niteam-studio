// Mirrors the Rust serde types (src-tauri/src/**). Field names are snake_case on the wire.

export type Language = "km" | "en";

export type ProjectStatus =
  | "script_draft"
  | "generating"
  | "review"
  | "rendering"
  | "done"
  | "paused_quota";

export type AssetStatus = "pending" | "running" | "done" | "failed" | "stale";

export interface ModelSettings {
  /** Text models in fallback order: 3 tries each, then the next one. */
  text_chain: string[];
  tts: string;
  tts_lite: string;
}

export type ProviderId = "cloudflare_flux" | "gemini_nano_banana";

/** Settings → Images (docs/07-image-generation.md §1). */
export interface ImageSettings {
  provider: ProviderId;
  gemini_model: string;
  /** Flux steps 4–8. */
  steps: number;
  key_scene_provider: ProviderId | null;
  /** Not secret; the API token is in the keyring. */
  cloudflare_account_id: string;
  cloudflare_concurrency: number;
}

export interface ImageCatalog {
  gemini_models: string[];
  flux_license_url: string;
  pan_1080: [number, number];
}

export interface ImageTestOutcome {
  ok: boolean;
  error_kind: string | null;
  error: string | null;
}

export interface ImageEstimate {
  images: number;
  cloudflare: number;
  gemini: number;
  cached: number;
  cloudflare_neurons: number;
  cloudflare_percent_of_daily: number;
  gemini_cost_usd: number;
  cloudflare_images_per_day: number;
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
  voice: VoiceDefaults;
  image: ImageSettings;
  render: RenderPrefs;
}

/** Settings → Video. */
export interface RenderPrefs {
  /** Cached detection result; null = detect on the next render. */
  encoder: string | null;
}

/** Returned by `subs_plan`: one scene whose subtitle PNGs must be drawn. */
export interface SubsTask {
  scene_id: string;
  narration: string;
  duration_ms: number;
  words: WordTiming[] | null;
  hash: string;
}

/** One drawn subtitle cue for `subs_save`. */
export interface CueIn {
  text: string;
  start_ms: number;
  end_ms: number;
  png_base64: string;
}

export type RenderStage = "clips" | "concat" | "done" | "failed" | "cancelled";

/** Payload of `render-progress`. */
export interface RenderProgress {
  project_id: string;
  stage: RenderStage;
  /** 1-based scene being rendered (0 during concat). */
  scene_index: number;
  total: number;
  /** 0–1 of this run's work. */
  percent: number;
  eta_seconds: number | null;
  output_path: string | null;
  error: string | null;
  error_kind: string | null;
}

/** Stable error codes from `GeminiError::kind` / `ProjectError::kind`. */
export type ErrorKind =
  | "ffmpeg_missing"
  | "ffmpeg"
  | "quota_daily"
  | "image_auth"
  | "edge_unavailable"
  | "suspicious_length"
  | "missing_key"
  | "invalid_key"
  | "rate_limited"
  | "payment"
  | "not_found"
  | "busy"
  | "network"
  | "blocked"
  | "bad_response"
  | "cancelled"
  | "other";

export const ERROR_KINDS: readonly ErrorKind[] = [
  "ffmpeg_missing",
  "ffmpeg",
  "quota_daily",
  "image_auth",
  "edge_unavailable",
  "suspicious_length",
  "missing_key",
  "invalid_key",
  "rate_limited",
  "payment",
  "not_found",
  "busy",
  "network",
  "blocked",
  "bad_response",
  "cancelled",
  "other",
];

export interface KeyTestOutcome {
  ok: boolean;
  model: string | null;
  display_name: string | null;
  error_kind: string | null;
  error: string | null;
}

export type RetryReason = "busy" | "rate_limited" | "network" | "next_model";

/** Payload of the `gemini-retry` event. */
export interface GeminiRetry {
  scope: "script" | "key_test" | "voice" | "image";
  project_id: string | null;
  model: string;
  /** For `next_model`: the model that just gave up. */
  previous_model: string | null;
  reason: RetryReason;
  attempt: number;
  max_attempts: number;
  delay_ms: number;
  status: number | null;
  error: string;
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

export type EngineKind = "edge" | "gemini";

/** Voice snapshot stored on the project (resolved from Settings for its language). */
export interface VoiceSettings {
  engine: EngineKind;
  model: string;
  voice_name: string;
  direction: string;
  rate: number;
  pitch: number;
}

/** Settings → Voice. */
export interface VoiceDefaults {
  engine: EngineKind;
  edge_voice_km: string;
  edge_voice_en: string;
  /** Edge rate in % (−50…+50). */
  rate: number;
  /** Edge pitch in Hz (−50…+50). */
  pitch: number;
  gemini_voice: string;
  gemini_model: string;
  direction: string;
}

export interface VoiceInfo {
  id: string;
  label: string;
  language: Language;
}

export interface VoiceCatalog {
  edge: VoiceInfo[];
  /** [name, character] — Gemini voices are multilingual. */
  gemini: [string, string][];
}

/** One entry of `scenes/<id>/words.json` (Edge WordBoundary; Khmer = phrase level). */
export interface WordTiming {
  text: string;
  start_ms: number;
  end_ms: number;
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
  /** Relative to the project folder: scenes/<id>/voice.mp3 | voice.wav */
  path: string | null;
  duration_ms: number | null;
  error: string | null;
  error_kind: string | null;
  text_hash: string | null;
  words_path: string | null;
}

export interface ImageAsset {
  status: AssetStatus;
  /** scenes/<id>/image_1080.png (pan size, 16:9) */
  path: string | null;
  raw_path: string | null;
  thumb_path: string | null;
  provider: string | null;
  /** Seed from "Regenerate"; null = bible seed + scene number. */
  seed: number | null;
  /** Scene whose image this one reuses (2 scenes per image). */
  shared_from: string | null;
  error: string | null;
  error_kind: string | null;
  prompt_hash: string | null;
}

export type Shot = "wide" | "medium" | "close" | "detail" | "aerial";
export const SHOTS: readonly Shot[] = ["wide", "medium", "close", "detail", "aerial"];

/** docs/07-image-generation.md §2 */
export interface VisualBible {
  style: string;
  characters: Record<string, string>;
  places: Record<string, string>;
  culture_notes: string;
  seed: number;
}

export interface ImageSnapshot {
  provider: ProviderId;
  gemini_model: string;
  steps: number;
  key_scene_provider: ProviderId | null;
  scenes_per_image: number;
}

export interface ImagePause {
  resets_at_utc: string;
  done: number;
  total: number;
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
  hash: string | null;
}

export interface ClipAsset {
  status: AssetStatus;
  /** scenes/<id>/clip.mkv */
  path: string | null;
  hash: string | null;
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
  characters: string[];
  place: string | null;
  shot: Shot;
  key_scene: boolean;
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
  visual_bible: VisualBible | null;
  image_settings: ImageSnapshot;
  image_pause: ImagePause | null;
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
  /** Omit to keep the scene's current image fields. */
  visual?: { characters: string[]; place: string | null; shot: Shot };
}

export type ScriptStage = "outline" | "chapter" | "done" | "failed" | "cancelled";

/** Payload of the `script-progress` event. */
export interface ScriptProgress {
  project_id: string;
  stage: ScriptStage;
  chapters_done: number;
  chapters_total: number;
  /** Raw error text (shown under "Details"). */
  error: string | null;
  error_kind: string | null;
}

/** Payload of `scene-progress`. */
export interface SceneProgress {
  project_id: string;
  scene_id: string;
  asset: "voice" | "image";
  status: AssetStatus;
  duration_ms: number | null;
  error: string | null;
  error_kind: string | null;
}

export type GenerateStage = "running" | "done" | "cancelled" | "failed" | "paused_quota";

/** Payload of `generate-progress`. */
export interface GenerateProgress {
  project_id: string;
  job: "voice" | "image";
  stage: GenerateStage;
  done: number;
  failed: number;
  total: number;
  error: string | null;
  error_kind: string | null;
  /** paused_quota: when the free allowance resets (RFC 3339 UTC). */
  resets_at_utc: string | null;
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
