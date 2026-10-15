# 05 — Roadmap

Work on **one phase at a time**. Only the phase marked **CURRENT** may be worked on.
When its acceptance criteria are all ticked, stop and ask the human to test.
After the human confirms, they (or you, if asked) move the **CURRENT** marker.

---

## Phase 1 — Scaffold, settings, script  ✅ (tested by the owner)

Goal: create a project and get an editable Khmer/English script from Gemini.

- [x] Tauri v2 + React + TS + Vite scaffold, strict TS, ESLint, clippy clean
- [x] Folder layout as in `02-architecture.md`; error type in `error.rs`
- [x] `storage/`: app data paths, `settings.json`, keyring secrets
- [x] Settings page: paste key, **Test key** button (cheap Gemini text call), clear key
- [x] `gemini/client.rs` + `retry.rs` (backoff rules from `03-pipeline.md`)
- [x] Projects list + New Project wizard (topic, language, minutes, style preset)
- [x] Script generation: outline call + per-chapter scene calls, JSON validated
- [x] Script editor: edit narration / spoken_text / image_prompt, add/delete/reorder, Approve
- [x] `project.json` atomic save; reopen app → project still there
- [x] i18n km/en with bundled Khmer font in the UI

**Test:** create a 3-minute Khmer project about "Where did rice come from?" → ~25
scenes appear, editable, saved after restart. Key never appears in any file or log.

## Phase 2 — Voice  ← **CURRENT**

- [ ] `tts/` module: `TtsEngine` trait with `EdgeTts` (default) and `GeminiTts`
- [ ] Gemini TTS per scene, model selectable (flash / flash-lite), voice + direction
- [ ] Edge TTS: km-KH-PisethNeural / km-KH-SreymomNeural + en-US voices, rate + pitch;
      clear "Edge TTS unavailable" error, Gemini TTS stays usable
- [ ] Settings: voice engine dropdown + voice / rate / pitch / Gemini model & direction
- [ ] Audio saved per scene (Edge MP3 / Gemini WAV); duration measured in Rust from the
      audio file (ffprobe arrives with the FFmpeg sidecar in Phase 4); suspicious-length check
- [ ] Edge WordBoundary events saved to `scenes/<id>/words.json`; subtitle timing helper uses
      them when present (proportional timing otherwise)
- [ ] Scheduler with semaphore, cancel, `scene-progress` events
- [ ] Khmer number-to-words fallback (`khmer_numbers.rs`) + unit tests
- [ ] Scene list shows play button + status per scene; regenerate voice for one scene
- [ ] Edited text or voice settings → voice marked `stale` → regenerated on next run

**Test:** generate voices for the 1-minute Khmer project "How was Angkor Wat built?" with
Edge TTS and play them back in the scene list; switch to Gemini TTS and regenerate one scene;
close the app mid-way through a 3-minute project; reopen → it resumes without redoing
finished scenes.

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

- 2026-10-15 — Phase 1 — Tauri v2 + React/TS scaffold; settings + keyring API key with a
  token-free key test (`models.get`); Gemini client with retry/backoff and per-kind semaphores;
  resumable outline + per-chapter script generation (structured JSON via `responseFormat`);
  script editor (narration / spoken text / image prompt / mood, fact notes, add/delete/reorder,
  approve/edit); atomic `project.json`; km/en UI with bundled fonts. `project.json` gained
  `outline`, `scene.chapter`, `scene.fact_note`. Built with the GNU toolchain (see 02 toolchain
  note). Khmer UI strings and prompts still need a native-speaker review.
- 2026-10-15 — Phase 1 fix — 503 "high demand" handling: retry waits are now visible in the UI
  ("Google is busy, retrying (attempt 2/5)…") via `gemini-retry`; text fallback model
  (`models.text_fallback`) after a 503 that survives all retries; friendly errors with
  **Try again** + raw **Details**; `pipeline.log` / `app.log`; retry + fallback unit tests
  against a mocked HTTP transport.
- 2026-10-15 — Phase 1 fix 2 — text model chain (`models.text_chain`, default
  3.5-flash-lite → 3.1-flash-lite → 3.8-flash; 3 attempts per model, then the next); UI shows
  model + attempt while retrying; exact HTTP status + body in `pipeline.log`; which model wrote
  each chapter is logged; Thai/Lao letters in Khmer replies are rejected. Live test: 1-minute
  Khmer "How was Angkor Wat built?" → 9 scenes / 2 chapters in ~11 s.
