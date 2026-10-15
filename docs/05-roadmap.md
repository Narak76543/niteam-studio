# 05 — Roadmap

Work on **one phase at a time**. Only the phase marked **CURRENT** may be worked on.
When its acceptance criteria are all ticked, stop and ask the human to test.
After the human confirms, they (or you, if asked) move the **CURRENT** marker.

---

## Phase 1 — Scaffold, settings, script  ← **CURRENT**

Goal: create a project and get an editable Khmer/English script from Gemini.

- [ ] Tauri v2 + React + TS + Vite scaffold, strict TS, ESLint, clippy clean
- [ ] Folder layout as in `02-architecture.md`; error type in `error.rs`
- [ ] `storage/`: app data paths, `settings.json`, keyring secrets
- [ ] Settings page: paste key, **Test key** button (cheap Gemini text call), clear key
- [ ] `gemini/client.rs` + `retry.rs` (backoff rules from `03-pipeline.md`)
- [ ] Projects list + New Project wizard (topic, language, minutes, style preset)
- [ ] Script generation: outline call + per-chapter scene calls, JSON validated
- [ ] Script editor: edit narration / spoken_text / image_prompt, add/delete/reorder, Approve
- [ ] `project.json` atomic save; reopen app → project still there
- [ ] i18n km/en with bundled Khmer font in the UI

**Test:** create a 3-minute Khmer project about "Where did rice come from?" → ~25
scenes appear, editable, saved after restart. Key never appears in any file or log.

## Phase 2 — Voice

- [ ] `gemini/tts.rs` per scene, model selectable (flash / flash-lite), voice + direction
- [ ] WAV saved, duration via ffprobe sidecar, suspicious-length check
- [ ] Scheduler with semaphore, cancel, `scene-progress` events
- [ ] Khmer number-to-words fallback (`khmer_numbers.rs`) + unit tests
- [ ] Scene list shows play button + status per scene; regenerate voice for one scene
- [ ] Edited text → voice marked `stale` → regenerated on next run

**Test:** generate voices for the 3-minute project; close app mid-way; reopen → it
resumes without redoing finished scenes.

## Phase 3 — Images

- [ ] `gemini/image.rs` with style prompt prefix, 16:9, enlarged `image_1080.png`
- [ ] Safety-block handling with readable error
- [ ] Scene grid: thumbnail, play audio, regenerate image/voice, edit prompt
- [ ] Overall progress bar + per-scene status

**Test:** all ~25 images generated in a consistent style; one regenerated individually.

## Phase 4 — Subtitles + first render

- [ ] `lib/subtitles.ts`: Khmer segmentation per `04-khmer.md` + unit tests
- [ ] Canvas PNG rendering, font-loaded check, `subs_save` command
- [ ] FFmpeg sidecar bundled (ffmpeg + ffprobe for Windows x64)
- [ ] Encoder detection + cache
- [ ] Per-scene clip render with pan motion + subtitle overlays, then concat
- [ ] Render page: progress, ETA, cancel, open file/folder

**Test:** 3-minute Khmer video plays in VLC and Windows Media Player; subtitles are
correctly shaped, no word split across lines, in sync with voice.

## Phase 5 — Polish & long videos

- [ ] Background music with ducking + fades; bundled default tracks; user can add own
- [ ] Thumbnail generator
- [ ] 720p option; below-normal FFmpeg priority
- [ ] 20-minute test on an 8 GB RAM laptop: no crash, memory stays < 1.5 GB for the app
- [ ] Installer via `npm run tauri build`; first-run onboarding
- [ ] Pipeline log viewer in Settings (no secrets)

## Later / ideas (not scheduled)

- Word-level subtitle timing
- Optional image-to-video for hook scenes
- Multiple style presets with sample previews
- Script fact-check pass with grounding
- Batch mode: queue several topics

---

## Changelog

_Add one line per finished phase: date — phase — notes._
