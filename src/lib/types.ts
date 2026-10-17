// Mirrors the Rust serde types (src-tauri/src/**). Field names are snake_case on the wire.

export type Language = "km" | "en";
export type Theme = "light" | "dark";

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
  /** Reads "From document" PDFs (free-tier Flash; docs/10-document-source.md). */
  document: string[];
  tts: string;
  tts_lite: string;
}

export type ProviderId = "cloudflare_flux" | "gemini_nano_banana" | "local_sdcpp";

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
  /** Installed local model profile (docs/08-local-gpu.md). */
  local_profile: string | null;
}

export interface GpuInfo {
  name: string;
  vendor: "nvidia" | "amd" | "intel" | "other";
  vram_mb: number;
}

/** Settings → Stock photos & videos (docs/11-real-world-visuals.md). */
export interface StockSettings {
  pexels: boolean;
  pixabay: boolean;
  openverse: boolean;
  commons: boolean;
  /** Off by default: the video may have to be shared under CC BY-SA too. */
  allow_by_sa: boolean;
  auto_pick: boolean;
}

export type StockSource = "pexels" | "pixabay" | "openverse" | "wikimedia" | "user";
export type StockMedia = "photo" | "video";
export type LicenseCode = "pexels" | "pixabay" | "cc0" | "pdm" | "by" | "by_sa" | "own";

export interface StockLicense {
  code: LicenseCode;
  version: string | null;
  url: string | null;
}

/** One stock search result (`stock::Candidate`). */
export interface StockCandidate {
  source: StockSource;
  source_id: string;
  media: StockMedia;
  title: string;
  tags: string[];
  author: string;
  author_url: string | null;
  license: StockLicense;
  page_url: string;
  file_url: string;
  thumb_url: string;
  width: number;
  height: number;
  duration_ms: number | null;
  /** Commons reuse restrictions (trademarked, personality…). */
  restrictions: string[];
  ai_generated: boolean;
  score: number;
  low_res: boolean;
}

export interface SourceNotice {
  source: StockSource;
  kind: string;
  message: string;
  retry_at_utc: string | null;
}

export interface StockSearchOutcome {
  candidates: StockCandidate[];
  notices: SourceNotice[];
}

export interface StockTestOutcome {
  ok: boolean;
  results: number;
  error_kind: string | null;
  details: string | null;
}

export type VisualKind = "ai_image" | "real_photo" | "stock_video" | "diagram" | "code" | "screenshot";

export interface DiagramSpec {
  template: string;
  mermaid: string;
}

export interface CodeSpec {
  language: string;
  text: string;
  terminal: boolean;
  type_out: boolean;
}

export interface SceneVisual {
  kind: VisualKind;
  keywords: string[];
  diagram: DiagramSpec | null;
  code: CodeSpec | null;
  candidates: StockCandidate[];
  asset: string | null;
  focus: [number, number] | null;
  auto_picked: boolean;
  status: AssetStatus;
  error: string | null;
  error_kind: string | null;
  render_path: string | null;
  thumb_path: string | null;
  render_hash: string | null;
}

/** `scene_visual_update` payload. */
export interface VisualEdit {
  kind: VisualKind;
  keywords: string[];
  diagram: DiagramSpec | null;
  code: CodeSpec | null;
  focus: [number, number] | null;
  confirm: boolean;
}

export interface LocalPrefs {
  cooldown_every: number;
  cooldown_seconds: number;
  offer_dismissed: boolean;
  seconds_per_image: number | null;
}

export interface LocalFileInfo {
  name: string;
  role: "model" | "diffusion" | "vae" | "clip_l" | "t5xxl" | "llm" | "lora";
  size_mb: number;
  license: string;
  license_url: string;
}

export interface LocalProfileStatus {
  id: string;
  label: string;
  min_vram_mb: number;
  total_mb: number;
  fits: boolean;
  installed: boolean;
  downloaded_mb: number;
  files: LocalFileInfo[];
}

export interface LocalStatus {
  gpu: GpuInfo | null;
  capable: boolean;
  engine_version: string;
  engine_present: boolean;
  profiles: LocalProfileStatus[];
  installing: string | null;
  selected: string | null;
  prefs: LocalPrefs;
  power: { has_battery: boolean; on_battery: boolean };
}

/** Payload of `model-download-progress`. */
export interface DownloadProgress {
  profile_id: string;
  stage: "running" | "done" | "failed" | "cancelled";
  done_mb: number;
  total_mb: number;
  error: string | null;
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
  /** Made on the user's GPU (free). */
  local: number;
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
  theme: Theme | null;
  output_dir: string | null;
  models: ModelSettings;
  concurrency: ConcurrencySettings;
  voice: VoiceDefaults;
  image: ImageSettings;
  render: RenderPrefs;
  gpu: GpuInfo | null;
  local: LocalPrefs;
  stock: StockSettings;
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
  | "stock_no_key"
  | "stock_bad_key"
  | "stock_rate_limited"
  | "stock_unavailable"
  | "stock_bad_response"
  | "stock_download"
  | "stock_no_results"
  | "local_engine"
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
  "stock_no_key",
  "stock_bad_key",
  "stock_rate_limited",
  "stock_unavailable",
  "stock_bad_response",
  "stock_download",
  "stock_no_results",
  "local_engine",
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
  scope: "script" | "key_test" | "voice" | "image" | "source";
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
  /** Real-world Tech Explainer (docs/11): visual types instead of AI images only. */
  real_world: boolean;
}

export interface Style {
  preset: string;
  style_prompt: string;
}

/** `voxcpm` = VoxCPM2 on the user's GPU (docs/09-voice-profiles.md). */
export type EngineKind = "edge" | "gemini" | "voxcpm";

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
  /** Default Voice Profile id for VoxCPM2 ("" = none yet). */
  voxcpm_profile: string;
}

export type VoiceProfileMode = "design" | "clone" | "ultimate";

/** Settings → Voices: a reusable local voice (docs/09-voice-profiles.md). */
export interface VoiceProfile {
  id: string;
  name: string;
  language: Language;
  mode: VoiceProfileMode;
  description: string;
  reference_wav: string;
  reference_transcript: string;
  style_hint: string;
  cfg_value: number;
  inference_timesteps: number;
  created_at: string;
  preview_wav: string | null;
  consent_at: string | null;
}

export interface VoicesStatus {
  gpu: GpuInfo | null;
  capable: boolean;
  engine_version: string;
  engine_present: boolean;
  model: LocalProfileStatus | null;
  installing: string | null;
  profiles: VoiceProfile[];
  default_profile: string;
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
  local_profile: string | null;
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
  /** Pending proofreading suggestion (Script page). */
  proofread: ProofSuggestion | null;
  proofread_hash: string | null;
  /** "From document" projects: PDF pages the scene's claims come from. */
  source_pages: number[];
  /** Background explanation, not a claim from the document. */
  general_knowledge: boolean;
  /** Real-world Tech Explainer: what the scene shows. */
  visual: SceneVisual;
}

export interface ProofSuggestion {
  narration: string;
  spoken_text: string;
  note: string;
}

/** Payload of `proofread-progress`. */
export interface ProofProgress {
  project_id: string;
  stage: "running" | "done" | "failed" | "cancelled";
  checked: number;
  total: number;
  suggestions: number;
  error: string | null;
  error_kind: string | null;
}

/** Payload of `prompt-rewrite-progress` ("Improve image prompts", Scenes page). */
export interface RewriteProgress {
  project_id: string;
  stage: "running" | "done" | "failed" | "cancelled";
  done: number;
  total: number;
  changed: number;
  error: string | null;
  error_kind: string | null;
}

export interface RenderSettings {
  resolution: string;
  music: string | null;
  music_volume: number;
  encoder: string | null;
  status: string;
  output_path: string | null;
  /** `#RRGGBB` box behind the spoken word; null = word tracker off. */
  subtitle_highlight: string | null;
}

export type PanDir = "lr" | "rl" | "tb" | "bt";

/** `render_timeline`: one scene of the editor preview (same times/pan as the render). */
export interface TimelineScene {
  scene_id: string;
  order: number;
  start_ms: number;
  /** Clip length (used voice + 0.25 s pause). */
  clip_ms: number;
  /** Voice length used (trailing silence trimmed). */
  voice_ms: number;
  /** Full voice file length (subtitle timings run over this). */
  duration_ms: number;
  pan: PanDir;
  voice_ready: boolean;
  image_ready: boolean;
  narration: string;
  words: WordTiming[] | null;
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
  /** Names kept exactly as written (React Native, Android, …). */
  keep_terms: string[];
  /** VoxCPM2 Voice Profile for this project (null = the default). */
  voice_profile: string | null;
  /** "From document" projects (docs/10-document-source.md). */
  source: SourceDoc | null;
  scenes: Scene[];
  render: RenderSettings;
}

export type Audience = "general" | "students" | "experts";
export type SourceStage = "reading" | "review" | "confirmed";

export interface SourceSection {
  heading: string;
  notes: string;
  pages: number[];
}

export interface SourceChunk {
  from: number;
  to: number;
  mode: "text" | "pdf" | null;
  sections: SourceSection[];
  done: boolean;
}

export interface SourceItem {
  id: string;
  /** Key terms only. */
  term: string;
  text: string;
  pages: number[];
  selected: boolean;
  note: string;
}

export interface SourceSummary {
  title: string;
  authors: string;
  year: string;
  key_points: SourceItem[];
  facts: SourceItem[];
  terms: SourceItem[];
}

export interface QuotaPause {
  resets_at_utc: string;
  daily: boolean;
}

export interface SourceDoc {
  file_name: string;
  size_bytes: number;
  page_count: number;
  page_from: number;
  page_to: number;
  audience: Audience;
  focus: string;
  stage: SourceStage;
  chunks: SourceChunk[];
  summary: SourceSummary | null;
  pause: QuotaPause | null;
  credit: string;
}

/** `source_pdf_info`: the picked file. */
export interface PdfInfo {
  file_name: string;
  size_bytes: number;
  page_count: number;
  needs_range: boolean;
  max_pages: number;
}

export interface NewDocumentInput {
  path: string;
  language: Language;
  target_minutes: number;
  style_preset: string;
  audience: Audience;
  focus: string;
  page_from: number | null;
  page_to: number | null;
}

export interface ReviewEdit {
  key_point_order: string[];
  items: { id: string; selected: boolean; note: string }[];
}

/** Payload of `source-progress`. */
export interface SourceProgress {
  project_id: string;
  stage: "reading" | "summarizing" | "done" | "paused" | "failed" | "cancelled";
  chunks_done: number;
  chunks_total: number;
  resets_at_utc: string | null;
  daily: boolean;
  error: string | null;
  error_kind: string | null;
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
  /** First scene with a thumbnail, and a key that changes with it. */
  cover_scene: string | null;
  cover_version: string | null;
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
  asset: "voice" | "image" | "visual";
  status: AssetStatus;
  duration_ms: number | null;
  error: string | null;
  error_kind: string | null;
}

export type GenerateStage = "running" | "done" | "cancelled" | "failed" | "paused_quota";

/** Payload of `generate-progress`. */
export interface GenerateProgress {
  project_id: string;
  job: "voice" | "image" | "visual";
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
