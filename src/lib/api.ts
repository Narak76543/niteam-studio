// Every invoke()/listen() call lives here, typed to mirror the Rust command signatures.
// Tauri maps camelCase argument names to the snake_case Rust parameters.
// Commands returning `()` resolve to `null`.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

import type {
  KeyTestResult,
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
  apiKeyTest: () => invoke<KeyTestResult>("api_key_test"),

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

/** Tauri rejects with the command's error string; normalize anything else. */
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}
