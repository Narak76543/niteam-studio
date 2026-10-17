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
│  ├─ components/               # shared UI; Phase 3: ImageSettingsSection, VisualBibleEditor,
│  │                            #   SceneThumb, ImagesPanel (estimate / pause), ImagePromptDialog
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
│     ├─ commands/   project.rs  script.rs  generate.rs  render.rs  settings.rs  voices.rs
│     ├─ gemini/     client.rs  transport.rs  retry.rs  script.rs  tts.rs  image.rs  models.rs
│     │              document.rs (PDF reading prompts, source block, credit; 10)
│     ├─ tts/        mod.rs (TtsEngine)  edge.rs  gemini.rs  audio.rs  voices.rs
│     │              voxcpm.rs  voxcpm_server.rs  chunker.rs  wav.rs  profiles.rs  (09)
│     ├─ images/     provider.rs (ImageProvider)  cloudflare.rs  gemini.rs  prompt.rs
│     │              postprocess.rs  budget.rs   (see 07-image-generation.md)
│     ├─ render/     ffmpeg.rs (sidecar runner)  encoders.rs  clip.rs  concat.rs
│     ├─ net/        transport.rs (HTTP behind a trait; mocked in tests)
│     ├─ stock/      Pexels, Pixabay, Openverse, Commons clients; license, rank, cache,
│     │              assets (downloads + licence records), search  (11)
│     ├─ pipeline/   project.rs  store.rs  editing.rs  hashing.rs  presets.rs  script_job.rs
│     │              jobs.rs  voice_job.rs  image_job.rs  visual_bible.rs  khmer_numbers.rs
│     │              subs.rs  render_job.rs  source_job.rs  source_edit.rs  (10)
│     ├─ source/     pdf.rs (lopdf: pages, ranges, text)  quality.rs  reset.rs  (10)
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
    - `scenes\<scene-id>\voice.mp3` (Edge) or `voice.wav` (Gemini), `words.json` (Edge
      phrase/word timings), `image_raw.jpg|png` (provider original), `image_1080.png` or
      `image_720.png` (16:9, pan size), `thumb.jpg` (384×216), `subs\000.png …`, `clip.mkv`
    - `concat.txt`, `output\final.mp4` (or `<Settings output folder>\<title>.mp4`),
      `output\thumbnail.png`
    - `logs\pipeline.log` (no secrets)
  - `voices\<profile-id>\` — local Voice Profiles: `profile.json`, `reference.wav`,
    `preview.wav` (see `09-voice-profiles.md`)
  - `logs\app.log` — app-level events such as key tests (no secrets)
- Secrets: `keyring` crate, service `NiteanStudio`:
  - user `gemini_api_key` — Gemini API key
  - user `cloudflare_api_token` — Cloudflare API token (Workers AI)
  - users `pexels_api_key`, `pixabay_api_key` — stock media keys (11)
  The Cloudflare **account ID** is not secret → `settings.json` (`image.cloudflare_account_id`).

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
  "voice": {                         // snapshot of Settings → Voice for this language
    "engine": "edge",                 // "edge" | "gemini" | "voxcpm"
    "voice_name": "km-KH-SreymomNeural", // Edge voice ID, Gemini voice name ("Kore") or Voice Profile id
    // voxcpm: "model" = "voxcpm2:<profile fingerprint>", "direction" = profile style (09 §4)
    "rate": 0, "pitch": 0,            // Edge only (% / Hz)
    "model": "gemini-3.8-flash-tts", "direction": "calm storyteller"  // Gemini only
  },
  "status": "script_draft",           // see state machine (incl. "paused_quota")
  "created_at": "…", "updated_at": "…",
  "visual_bible": {                   // 07 §2; generated with the script, editable
    "style": "…", "characters": { "farmer": "…" }, "places": { "village": "…" },
    "culture_notes": "…", "seed": 1847
  },
  "image_settings": {                 // snapshot of Settings → Images used for hashing
    "provider": "cloudflare_flux", "gemini_model": "…", "steps": 4,
    "key_scene_provider": null, "scenes_per_image": 1
  },
  "image_pause": null,
  "voice_profile": null,             // VoxCPM2 Voice Profile override (null = default, 09 §5)
  "keep_terms": ["React Native"],     // names never translated (04 §4c), Script page → Names                // { "resets_at_utc": "…", "done": 40, "total": 86 } while paused_quota
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
      "characters": ["farmer"],       // visual_bible.characters keys (07 §2)
      "place": "village",             // visual_bible.places key or null
      "shot": "wide",                 // wide | medium | close | detail | aerial
      "key_scene": true,              // scene 1, each chapter's first, last scene
      "voice":   { "status": "pending", "path": null, "duration_ms": null, "error": null,
                   "error_kind": null, "text_hash": null, "words_path": null },
      "image":   { "status": "pending", "path": null, "raw_path": null, "thumb_path": null,
                   "provider": null, "seed": null, "shared_from": null,
                   "error": null, "error_kind": null, "prompt_hash": null },
                   // prompt_hash = sha256(provider_id + model + final_prompt + seed + steps) (07 §8)
      "subs":    { "status": "pending", "hash": null,   // hashing::subs_hash the PNGs were drawn for
                   "lines": [ { "text": "line 1\nline 2", "start_ms": 0, "end_ms": 2400,
                                "path": "scenes/s001/subs/000.png" } ] },  // one cue = ≤ 2 lines
      "clip":    { "status": "pending", "path": null, "hash": null },  // scenes/<id>/clip.mkv
      "proofread": null,              // { narration, spoken_text, note } waiting for Accept/Dismiss
      "proofread_hash": null          // text last checked by the proofreader (03 Step 1b)
    }
  ],
  "render": { "resolution": "1080p", "music": "default_calm.mp3", "music_volume": 0.12,
              "encoder": null, "status": "idle", "output_path": null,
              "subtitle_highlight": "#FFD60A" }   // word-tracker colour, null = off
}
```

"From a document" projects also have `"source": { … }` (file, page range, audience, focus,
reading stage, chunks with section notes, the reviewed summary, quota pause, credit line) and
per scene `"source_pages": [3, 4]`, `"general_knowledge": false`; the PDF is
`projects/<id>/source/document.pdf`. Full shape: `10-document-source.md` §2.

Every scene also has `"visual": { "kind": "ai_image", … }` (Real-world Tech Explainer: real
photo, stock video, diagram, code, screenshot), and stock files live in
`projects/<id>/assets/` with `assets.json` (licence records). Full shape: `11-real-world-visuals.md` §1.

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

`paused_quota`: the image job stopped because the Cloudflare daily allowance ran out
(07 §6). Finished images are kept; `image_pause.resets_at_utc` says when to resume. Resume
goes back to `generating`. Voices can still be generated while paused.

"From a document" projects stay in `script_draft` while the PDF is read and reviewed
(`source.stage`: `reading → review → confirmed`); `script_generate` refuses until confirmed.

Script generation runs while the status stays `script_draft` (tracked in memory by
`ScriptJobs`). Scene edit/add/delete/reorder only work in `script_draft`. Approve requires
every scene to have narration, spoken text and image prompt, and the outline to be complete.
"Edit script" (`script_reopen`) goes back to `script_draft` from any state except `rendering`.

## Tauri commands (Rust → exposed to `src/lib/api.ts`)

| Command | Purpose |
|---|---|
| `settings_get`, `settings_save(patch)` | read settings.json / merge the changed fields (`lib/settingsPatch.ts`) into it |
| `api_key_set`, `api_key_test`, `api_key_clear`, `api_key_exists` | keyring; never return the key itself. `api_key_test` = `models.get` on the text model (no tokens used); returns `{ok, model, display_name, error_kind, error}` |
| `presets_list` | built-in style presets |
| `project_list`, `project_create`, `project_get`, `project_delete` | projects (list items carry `cover_scene` / `cover_version`: first scene with a thumbnail, for the Projects cards) |
| `script_generate(project_id, restart)` | start/resume the background script job (`restart` clears outline + scenes) |
| `script_cancel(project_id)`, `script_running(project_id)` | cancel / query the script job |
| `scene_update(project_id, scene)` | user edits; marks dependent assets stale |
| `scenes_reorder(project_id, scene_ids)`, `scene_add(project_id, after_scene_id?)`, `scene_delete` | script editing |
| `script_approve(project_id)` | → `generating` (Phase 2 will also start the scheduler) |
| `script_reopen(project_id)` | "Edit script": back to `script_draft` |
| `generate_start / generate_cancel / generate_running(project_id)` | run / cancel / query the voice job (images join in Phase 3) |
| `scene_regenerate(project_id, scene_id, what: "voice"\|"image")` | single redo (Phase 2: voice) |
| `scene_voice_audio(project_id, scene_id)` | raw MP3/WAV bytes for playback (only `scenes/…` paths) |
| `voices_catalog()`, `voice_preview(language, voice)` | voice lists; speak a sample with unsaved settings |
| `cloudflare_token_set / _exists / _clear`, `cloudflare_test()` | keyring token; 1-step test image |
| `images_estimate(project_id)` | counts per provider, neurons, % of daily allowance, Gemini cost |
| `images_start / images_cancel / images_running(project_id)` | image job (also resumes `paused_quota`) |
| `scene_image_thumb(project_id, scene_id)` | `thumb.jpg` bytes for the grid |
| `scene_image_prompt_update(project_id, scene_id, image_prompt)` | edit a prompt after approval |
| `visual_bible_update(project_id, bible)`, `visual_bible_generate(project_id)` | Look & feel |
| `project_set_scenes_per_image(project_id, n)` | 1 or 2 (07 §5) |
| `subs_plan(project_id)` | scenes whose subtitle PNGs are missing/outdated: narration, duration, Edge timings, hash |
| `subs_save(project_id, scene_id, hash, cues[{text, start_ms, end_ms, png_base64}])` | frontend hands rendered subtitle PNGs to Rust |
| `render_start / render_cancel / render_running(project_id)` | FFmpeg render job (clips + concat) |
| `render_open_output(project_id, reveal)` | open the finished MP4 / show it in Explorer |
| `render_timeline(project_id)` | editor preview: per scene start, clip/voice/file length, pan, readiness, narration, Edge timings |
| `scene_image_full(project_id, scene_id)` | `image_1080.png` bytes for the editor preview |
| `render_set_subtitle_style(project_id, highlight)` | word-tracker colour `#RRGGBB` or null |
| `proofread_start / proofread_cancel / proofread_running(project_id)` | grammar & phrasing check job (03 Step 1b) |
| `proofread_accept / proofread_dismiss(project_id, scene_ids)` | apply / drop suggestions |
| `image_prompts_rewrite / _cancel / _running(project_id)` | "Improve image prompts": rewrite every scene's image prompt with the current rules (03 Step 4a); event `prompt-rewrite-progress` |
| `project_set_keep_terms(project_id, terms)` | names that stay exactly as written |
| `project_set_style(project_id, preset)` | switch style preset + rewrite the Visual Bible |
| `voices_status`, `voices_model_install / _verify / _delete`, `voice_design_samples`, `voice_design_sample_audio`, `voice_profile_create_design / _import / _update / _duplicate / _delete / _preview / _reference_audio / _set_default`, `project_set_voice_profile` | local voices (VoxCPM2) and Voice Profiles — see `09-voice-profiles.md` §7 |
| `encoders_detect()` | first working H.264 encoder; cached in `settings.render.encoder` |
| `source_pdf_info(path)`, `project_create_from_document(input)` | "From a document": file check (pages, size, range needed) / create the project and copy the PDF (10) |
| `source_read / _cancel / _running(project_id)` | reading job (resumes after a quota pause); event `source-progress` |
| `source_review_save / source_confirm(project_id, edit)`, `source_reopen`, `source_reread` | review edits, confirm (credit + keep terms), back to the points, read again |
| `stock_key_set / _exists / _clear / _test(source)`, `stock_search`, `stock_pick` | stock photos & videos — see `11-real-world-visuals.md` §5 |

## Events (Rust → UI)

`script-progress` `{project_id, stage: outline|chapter|done|failed|cancelled, chapters_done, chapters_total, error, error_kind}`
`gemini-retry` `{scope: script|key_test, project_id?, model, previous_model?, reason: busy|rate_limited|network|next_model, attempt, max_attempts, delay_ms, status?, error}`
`scene-progress` `{project_id, scene_id, asset, status, duration_ms?, error?, error_kind?}`
`generate-progress` `{project_id, job: voice|image, stage: running|done|cancelled|failed|paused_quota, done, failed, total, error?, error_kind?, resets_at_utc?}`
`render-progress` `{project_id, stage: clips|concat|done|failed|cancelled, scene_index, total, percent, eta_seconds, output_path?, error?, error_kind?}`
`source-progress` `{project_id, stage: reading|summarizing|done|paused|failed|cancelled, chunks_done, chunks_total, resets_at_utc?, daily, error?, error_kind?}`
`log` `{level, message}`

## Concurrency

- One `Semaphore` per Gemini endpoint kind (text / tts / image); default 3, from settings
  (read at startup — a change applies after restart). A permit is held per HTTP attempt,
  not during backoff sleeps.
- Every `project.json` read-modify-write goes through `ProjectStore::update`, which holds a
  per-project lock, so a background job and UI edits never overwrite each other.
- Scheduler runs per-scene jobs with `tokio::spawn`, respects a `CancellationToken`.
- The local voice engine (`AppState.voice_server`, one audiocpp_server process) and local
  images never use the GPU at the same time (09 §6).
- Only **one** FFmpeg process at a time (low-spec laptops). Run it with
  below-normal priority on Windows so the UI stays responsive.
- Memory: never load all images/audio into RAM; work file-to-file.
