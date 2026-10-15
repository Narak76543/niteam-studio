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
│  ├─ i18n/ km.json en.json      # + context.ts / I18nProvider.tsx
│  ├─ styles/                    # tokens.css, fonts.css, base.css (see 06-ui-design.md)
│  └─ assets/fonts/              # Manrope, Kantumruy Pro, Noto Sans Khmer (OFL, woff2)
├─ src-tauri/
│  ├─ binaries/                  # ffmpeg-x86_64-pc-windows-msvc.exe, ffprobe-...
│  ├─ resources/music/           # royalty-free default tracks
│  └─ src/
│     ├─ main.rs  lib.rs  state.rs
│     ├─ error.rs
│     ├─ commands/   project.rs  script.rs  generate.rs  render.rs  settings.rs
│     ├─ gemini/     client.rs  transport.rs  retry.rs  script.rs  tts.rs  image.rs  models.rs
│     ├─ pipeline/   project.rs  store.rs  editing.rs  hashing.rs  presets.rs  script_job.rs
│     │              scheduler.rs  render.rs  ffmpeg.rs  encoders.rs  audio.rs
│     └─ storage/    paths.rs  secrets.rs  settings.rs  atomic.rs  log.rs
└─ package.json  vite.config.ts  tsconfig.json
```

Files for later phases (`generate.rs`, `tts.rs`, `scheduler.rs`, …) are added in the phase that
needs them. Phase 1 modules:

- `pipeline/store.rs` — load/save `project.json` under a per-project async lock
- `pipeline/editing.rs` — create project, scene edit/add/delete/reorder, approve/reopen
- `pipeline/hashing.rs` — NFC normalization, `text_hash` / `prompt_hash`, stale marking
- `pipeline/script_job.rs` — resumable outline + per-chapter script generation
- `storage/atomic.rs` — `.tmp` + rename writes used for every JSON file
- `storage/log.rs` — append-only text logs with secret redaction and rotation
- `gemini/transport.rs` — HTTP behind a `Transport` trait (reqwest in the app, a scripted
  mock in unit tests for retry / model chain)
- `commands/retry_events.rs` — turns retry notices into `gemini-retry` events + log lines

**Toolchain note:** the dev machine currently builds with `x86_64-pc-windows-gnu` (MinGW).
The lib crate is `rlib`-only (a `cdylib` exceeds ld's export limit), and `build.rs` embeds the
Common-Controls manifest into every binary including unit tests (`linker_messages` is allowed
in `Cargo.toml` because MinGW adds its own default manifest). With MSVC these workarounds are
harmless; sidecar file names must use the matching target triple.

## Storage

- App data: `%APPDATA%\NiteanStudio\` (resolved as Tauri `path().data_dir()` + `NiteanStudio`,
  never hard-coded; `app_data_dir()` would give `%APPDATA%\com.nitean.studio`)
  - `settings.json` — everything except the API key
  - `projects\<project-id>\`
    - `project.json` — single source of truth (see data model)
    - `scenes\<scene-id>\voice.wav`, `image.png`, `subs\000.png …`, `clip.mp4`
    - `output\final.mp4`, `output\thumbnail.png`
    - `logs\pipeline.log` (no secrets)
  - `logs\app.log` — app-level events such as key tests (no secrets)
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
  "outline": {                        // null until the outline call returns
    "title": "…", "hook": "…",
    "chapters": [ { "title": "…", "summary": "…", "scene_count": 5, "done": true } ]
  },                                  // `done` = scenes received → resume point
  "scenes": [
    {
      "id": "s001",                   // never reused after a delete
      "order": 1,
      "chapter": 0,                   // 0-based index into outline.chapters
      "narration": "…",               // what the user sees/edits
      "spoken_text": "…",             // narration with numbers/names spelled out (TTS input)
      "image_prompt": "…",            // English, WITHOUT the style prompt
      "mood": "curious",              // one of gemini::script::MOODS
      "fact_note": null,              // model's "please check this claim" note, or null
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

All text is NFC-normalized and trimmed on save; `spoken_text` also loses zero-width spaces.

Asset `status`: `pending | running | done | failed | stale`.
An asset becomes `stale` when its input changes (compare `text_hash` / `prompt_hash`);
stale assets are regenerated, done ones are skipped. This is what makes resume work.

## Project state machine

```
script_draft ──approve──▶ generating ──all done──▶ review ──render──▶ rendering ──▶ done
     ▲                        │ (failures stay per-scene)  ▲              │
     └──────── edit script ───┴────────────────────────────┴── cancel ────┘
```

Script generation runs while the status stays `script_draft` (tracked in memory by
`ScriptJobs`). Scene edit/add/delete/reorder only work in `script_draft`. Approve requires
every scene to have narration, spoken text and image prompt, and the outline to be complete.
"Edit script" (`script_reopen`) goes back to `script_draft` from any state except `rendering`.

## Tauri commands (Rust → exposed to `src/lib/api.ts`)

| Command | Purpose |
|---|---|
| `settings_get`, `settings_save` | read/write settings.json |
| `api_key_set`, `api_key_test`, `api_key_clear`, `api_key_exists` | keyring; never return the key itself. `api_key_test` = `models.get` on the text model (no tokens used); returns `{ok, model, display_name, error_kind, error}` |
| `presets_list` | built-in style presets |
| `project_list`, `project_create`, `project_get`, `project_delete` | projects |
| `script_generate(project_id, restart)` | start/resume the background script job (`restart` clears outline + scenes) |
| `script_cancel(project_id)`, `script_running(project_id)` | cancel / query the script job |
| `scene_update(project_id, scene)` | user edits; marks dependent assets stale |
| `scenes_reorder(project_id, scene_ids)`, `scene_add(project_id, after_scene_id?)`, `scene_delete` | script editing |
| `script_approve(project_id)` | → `generating` (Phase 2 will also start the scheduler) |
| `script_reopen(project_id)` | "Edit script": back to `script_draft` |
| `generate_start / generate_cancel(project_id)` | run/cancel voice+image jobs |
| `scene_regenerate(project_id, scene_id, what: "voice"\|"image")` | single redo |
| `subs_save(project_id, scene_id, lines_with_png_bytes)` | frontend hands rendered subtitle PNGs to Rust |
| `render_start / render_cancel(project_id, options)` | FFmpeg render |
| `encoders_detect()` | which H.264 encoders work on this machine |

## Events (Rust → UI)

`script-progress` `{project_id, stage: outline|chapter|done|failed|cancelled, chapters_done, chapters_total, error, error_kind}`
`gemini-retry` `{scope: script|key_test, project_id?, model, previous_model?, reason: busy|rate_limited|network|next_model, attempt, max_attempts, delay_ms, status?, error}`
`scene-progress` `{project_id, scene_id, asset, status, error?}`
`generate-progress` `{project_id, done, total}`
`render-progress` `{project_id, stage, scene_index, total, percent, eta_seconds}`
`log` `{level, message}`

## Concurrency

- One `Semaphore` per Gemini endpoint kind (text / tts / image); default 3, from settings
  (read at startup — a change applies after restart). A permit is held per HTTP attempt,
  not during backoff sleeps.
- Every `project.json` read-modify-write goes through `ProjectStore::update`, which holds a
  per-project lock, so a background job and UI edits never overwrite each other.
- Scheduler runs per-scene jobs with `tokio::spawn`, respects a `CancellationToken`.
- Only **one** FFmpeg process at a time (low-spec laptops). Run it with
  below-normal priority on Windows so the UI stays responsive.
- Memory: never load all images/audio into RAM; work file-to-file.
