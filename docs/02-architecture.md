# 02 — Architecture

## Overview

```
┌──────────────── Tauri WebView (React + TS) ────────────────┐
│ Pages: Projects · Wizard · Script · Scenes · Render · Settings │
│ lib/subtitles.ts  → Khmer line breaking + canvas → PNG      │
│ lib/api.ts        → typed invoke() wrappers + event listeners│
└───────────────▲──────────────────────────┬─────────────────┘
        events  │ (progress, scene done)   │ invoke (commands)
┌───────────────┴──────────────────────────▼─────────────────┐
│ Rust backend (tokio)                                        │
│ commands/  → thin Tauri command layer                       │
│ gemini/    → HTTP client, retry/backoff, script, tts, image │
│ pipeline/  → project state, scheduler, render, ffmpeg       │
│ storage/   → paths, keyring, settings                       │
└───────────────┬────────────────────────────────────────────┘
                │ spawns sidecar
            ffmpeg.exe / ffprobe.exe
```

Rust owns all state, network and processes. React owns UI and the one thing only a
browser does well: **shaping and drawing Khmer text** (subtitle PNGs).

## Folder layout

```
nitean-studio/
├─ CLAUDE.md  AGENTS.md
├─ docs/
├─ src/                          # React frontend
│  ├─ pages/                     # Projects, NewProject, Script, Scenes, Render, Settings
│  ├─ components/
│  ├─ lib/
│  │  ├─ api.ts                  # ALL invoke()/listen() calls live here
│  │  ├─ types.ts                # mirrors Rust serde types
│  │  └─ subtitles.ts            # segmenting + canvas PNG rendering
│  ├─ i18n/ km.json en.json
│  └─ assets/fonts/              # NotoSansKhmer, KantumruyPro (OFL)
├─ src-tauri/
│  ├─ binaries/                  # ffmpeg-x86_64-pc-windows-msvc.exe, ffprobe-...
│  ├─ resources/music/           # royalty-free default tracks
│  └─ src/
│     ├─ main.rs  lib.rs
│     ├─ error.rs
│     ├─ commands/   project.rs  script.rs  generate.rs  render.rs  settings.rs
│     ├─ gemini/     client.rs  retry.rs  script.rs  tts.rs  image.rs  models.rs
│     ├─ pipeline/   project.rs  scheduler.rs  render.rs  ffmpeg.rs  encoders.rs  audio.rs
│     └─ storage/    paths.rs  secrets.rs  settings.rs
└─ package.json  vite.config.ts  tsconfig.json
```

## Storage

- App data: `%APPDATA%\NiteanStudio\` (use Tauri's `app_data_dir()`, never hard-code)
  - `settings.json` — everything except the API key
  - `projects\<project-id>\`
    - `project.json` — single source of truth (see data model)
    - `scenes\<scene-id>\voice.wav`, `image.png`, `subs\000.png …`, `clip.mp4`
    - `output\final.mp4`, `output\thumbnail.png`
    - `logs\pipeline.log` (no secrets)
- API key: `keyring` crate, service `NiteanStudio`, user `gemini_api_key`.

Writes to `project.json` are **atomic**: write `project.json.tmp`, then rename.

## Data model (`project.json`)

```jsonc
{
  "schema_version": 1,
  "id": "01J…",                       // ULID
  "title": "Where did rice come from?",
  "topic": "…",
  "language": "km",                   // "km" | "en"
  "target_minutes": 12,
  "style": {
    "preset": "storybook_watercolor",
    "style_prompt": "warm watercolor storybook illustration, soft light, …"
  },
  "voice": { "model": "gemini-3.8-flash-tts", "voice_name": "…", "direction": "calm storyteller" },
  "status": "script_draft",           // see state machine
  "created_at": "…", "updated_at": "…",
  "scenes": [
    {
      "id": "s001",
      "order": 1,
      "narration": "…",               // what the user sees/edits
      "spoken_text": "…",             // narration with numbers/names spelled out (TTS input)
      "image_prompt": "…",            // English, WITHOUT the style prompt
      "mood": "curious",
      "voice":   { "status": "pending", "path": null, "duration_ms": null, "error": null, "text_hash": null },
      "image":   { "status": "pending", "path": null, "error": null, "prompt_hash": null },
      "subs":    { "status": "pending", "lines": [ { "text": "…", "start_ms": 0, "end_ms": 2400, "path": "subs/000.png" } ] },
      "clip":    { "status": "pending", "path": null }
    }
  ],
  "render": { "resolution": "1080p", "music": "default_calm.mp3", "music_volume": 0.12,
              "encoder": null, "status": "idle", "output_path": null }
}
```

Asset `status`: `pending | running | done | failed | stale`.
An asset becomes `stale` when its input changes (compare `text_hash` / `prompt_hash`);
stale assets are regenerated, done ones are skipped. This is what makes resume work.

## Project state machine

```
script_draft ──approve──▶ generating ──all done──▶ review ──render──▶ rendering ──▶ done
     ▲                        │ (failures stay per-scene)  ▲              │
     └──────── edit script ───┴────────────────────────────┴── cancel ────┘
```

## Tauri commands (Rust → exposed to `src/lib/api.ts`)

| Command | Purpose |
|---|---|
| `settings_get`, `settings_save` | read/write settings.json |
| `api_key_set`, `api_key_test`, `api_key_clear`, `api_key_exists` | keyring; never return the key itself |
| `project_list`, `project_create`, `project_get`, `project_delete` | projects |
| `script_generate(project_id)` | call Gemini, fill scenes |
| `scene_update(project_id, scene)` | user edits; marks dependent assets stale |
| `scenes_reorder`, `scene_add`, `scene_delete` | script editing |
| `script_approve(project_id)` | → `generating`, starts scheduler |
| `generate_start / generate_cancel(project_id)` | run/cancel voice+image jobs |
| `scene_regenerate(project_id, scene_id, what: "voice"\|"image")` | single redo |
| `subs_save(project_id, scene_id, lines_with_png_bytes)` | frontend hands rendered subtitle PNGs to Rust |
| `render_start / render_cancel(project_id, options)` | FFmpeg render |
| `encoders_detect()` | which H.264 encoders work on this machine |

## Events (Rust → UI)

`scene-progress` `{project_id, scene_id, asset, status, error?}`
`generate-progress` `{project_id, done, total}`
`render-progress` `{project_id, stage, scene_index, total, percent, eta_seconds}`
`log` `{level, message}`

## Concurrency

- One `Semaphore` per Gemini endpoint kind (text / tts / image); default 3, from settings.
- Scheduler runs per-scene jobs with `tokio::spawn`, respects a `CancellationToken`.
- Only **one** FFmpeg process at a time (low-spec laptops). Run it with
  below-normal priority on Windows so the UI stays responsive.
- Memory: never load all images/audio into RAM; work file-to-file.
