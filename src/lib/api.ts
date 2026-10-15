// Every invoke()/listen() call lives here, typed to mirror the Rust command signatures.
// Tauri maps camelCase argument names to the snake_case Rust parameters.
// Commands returning `()` resolve to `null`.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

import type {
  GeminiRetry,
  GenerateProgress,
  CueIn,
  ImageCatalog,
  RenderProgress,
  TimelineScene,
  SubsTask,
  ImageEstimate,
  ImageTestOutcome,
  VisualBible,
  KeyTestOutcome,
  Language,
  SceneProgress,
  VoiceCatalog,
  VoiceDefaults,
  NewProjectInput,
  Project,
  ProjectSummary,
  SceneEdit,
  ScriptProgress,
  Settings,
  StylePreset,
} from "./types";

export const api = {
  // settings
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSave: (settings: Settings) => invoke<Settings>("settings_save", { settings }),

  // API key (the key itself is never returned)
  apiKeySet: (key: string) => invoke<null>("api_key_set", { key }),
  apiKeyExists: () => invoke<boolean>("api_key_exists"),
  apiKeyClear: () => invoke<null>("api_key_clear"),
  apiKeyTest: () => invoke<KeyTestOutcome>("api_key_test"),

  // projects
  presetsList: () => invoke<StylePreset[]>("presets_list"),
  projectList: () => invoke<ProjectSummary[]>("project_list"),
  projectCreate: (input: NewProjectInput) => invoke<Project>("project_create", { input }),
  projectGet: (projectId: string) => invoke<Project>("project_get", { projectId }),
  projectDelete: (projectId: string) => invoke<null>("project_delete", { projectId }),

  // script
  scriptGenerate: (projectId: string, restart: boolean) =>
    invoke<null>("script_generate", { projectId, restart }),
  scriptCancel: (projectId: string) => invoke<null>("script_cancel", { projectId }),
  scriptRunning: (projectId: string) => invoke<boolean>("script_running", { projectId }),
  sceneUpdate: (projectId: string, scene: SceneEdit) =>
    invoke<Project>("scene_update", { projectId, scene }),
  sceneAdd: (projectId: string, afterSceneId: string | null) =>
    invoke<Project>("scene_add", { projectId, afterSceneId }),
  sceneDelete: (projectId: string, sceneId: string) =>
    invoke<Project>("scene_delete", { projectId, sceneId }),
  scenesReorder: (projectId: string, sceneIds: string[]) =>
    invoke<Project>("scenes_reorder", { projectId, sceneIds }),
  scriptApprove: (projectId: string) => invoke<Project>("script_approve", { projectId }),
  scriptReopen: (projectId: string) => invoke<Project>("script_reopen", { projectId }),

  // voices (Phase 2)
  generateStart: (projectId: string) => invoke<null>("generate_start", { projectId }),
  generateCancel: (projectId: string) => invoke<null>("generate_cancel", { projectId }),
  generateRunning: (projectId: string) => invoke<boolean>("generate_running", { projectId }),
  sceneRegenerate: (projectId: string, sceneId: string, what: "voice" | "image") =>
    invoke<null>("scene_regenerate", { projectId, sceneId, what }),
  /** Raw MP3/WAV bytes of a scene's voice. */
  sceneVoiceAudio: (projectId: string, sceneId: string) =>
    invoke<ArrayBuffer>("scene_voice_audio", { projectId, sceneId }),
  voicesCatalog: () => invoke<VoiceCatalog>("voices_catalog"),
  /** Speaks a sample with (possibly unsaved) voice settings; raw audio bytes. */
  voicePreview: (language: Language, voice: VoiceDefaults) =>
    invoke<ArrayBuffer>("voice_preview", { language, voice }),

  // images (Phase 3)
  cloudflareTokenSet: (token: string) => invoke<null>("cloudflare_token_set", { token }),
  cloudflareTokenExists: () => invoke<boolean>("cloudflare_token_exists"),
  cloudflareTokenClear: () => invoke<null>("cloudflare_token_clear"),
  cloudflareTest: () => invoke<ImageTestOutcome>("cloudflare_test"),
  imagesCatalog: () => invoke<ImageCatalog>("images_catalog"),
  imagesEstimate: (projectId: string) => invoke<ImageEstimate>("images_estimate", { projectId }),
  imagesStart: (projectId: string) => invoke<null>("images_start", { projectId }),
  imagesCancel: (projectId: string) => invoke<null>("images_cancel", { projectId }),
  imagesRunning: (projectId: string) => invoke<boolean>("images_running", { projectId }),
  /** thumb.jpg bytes (384×216). */
  sceneImageThumb: (projectId: string, sceneId: string) =>
    invoke<ArrayBuffer>("scene_image_thumb", { projectId, sceneId }),
  sceneImagePromptUpdate: (projectId: string, sceneId: string, imagePrompt: string) =>
    invoke<Project>("scene_image_prompt_update", { projectId, sceneId, imagePrompt }),
  visualBibleUpdate: (projectId: string, bible: VisualBible) =>
    invoke<Project>("visual_bible_update", { projectId, bible }),
  visualBibleGenerate: (projectId: string) => invoke<Project>("visual_bible_generate", { projectId }),
  projectSetScenesPerImage: (projectId: string, n: 1 | 2) =>
    invoke<Project>("project_set_scenes_per_image", { projectId, n }),

  // subtitles + render (Phase 4)
  subsPlan: (projectId: string) => invoke<SubsTask[]>("subs_plan", { projectId }),
  subsSave: (projectId: string, sceneId: string, hash: string, cues: CueIn[]) =>
    invoke<null>("subs_save", { projectId, sceneId, hash, cues }),
  encodersDetect: () => invoke<string>("encoders_detect"),
  renderTimeline: (projectId: string) => invoke<TimelineScene[]>("render_timeline", { projectId }),
  /** image_1080.png bytes for the editor preview. */
  sceneImageFull: (projectId: string, sceneId: string) =>
    invoke<ArrayBuffer>("scene_image_full", { projectId, sceneId }),
  renderSetSubtitleStyle: (projectId: string, highlight: string | null) =>
    invoke<Project>("render_set_subtitle_style", { projectId, highlight }),
  renderStart: (projectId: string) => invoke<null>("render_start", { projectId }),
  renderCancel: (projectId: string) => invoke<null>("render_cancel", { projectId }),
  renderRunning: (projectId: string) => invoke<boolean>("render_running", { projectId }),
  /** Opens the finished video, or shows it in Explorer with `reveal`. */
  renderOpenOutput: (projectId: string, reveal: boolean) =>
    invoke<null>("render_open_output", { projectId, reveal }),

  // native helpers
  pickFolder: async (): Promise<string | null> => {
    const picked = await openDialog({ directory: true, multiple: false });
    return typeof picked === "string" ? picked : null;
  },
  openExternal: (url: string) => openUrl(url),
};

export function onScriptProgress(cb: (p: ScriptProgress) => void): Promise<UnlistenFn> {
  return listen<ScriptProgress>("script-progress", (e) => cb(e.payload));
}

export function onSceneProgress(cb: (p: SceneProgress) => void): Promise<UnlistenFn> {
  return listen<SceneProgress>("scene-progress", (e) => cb(e.payload));
}

export function onGenerateProgress(cb: (p: GenerateProgress) => void): Promise<UnlistenFn> {
  return listen<GenerateProgress>("generate-progress", (e) => cb(e.payload));
}

export function onRenderProgress(cb: (p: RenderProgress) => void): Promise<UnlistenFn> {
  return listen<RenderProgress>("render-progress", (e) => cb(e.payload));
}

/** Retry/fallback notices for any Gemini call (filter by `scope` / `project_id`). */
export function onGeminiRetry(cb: (r: GeminiRetry) => void): Promise<UnlistenFn> {
  return listen<GeminiRetry>("gemini-retry", (e) => cb(e.payload));
}

/** Tauri rejects with the command's error string; normalize anything else. */
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
