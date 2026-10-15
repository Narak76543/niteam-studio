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

## Phase 2 — Voice  (built; awaiting the owner's test)

- [x] `tts/` module: `TtsEngine` trait with `EdgeTts` (default) and `GeminiTts`
- [x] Gemini TTS per scene, model selectable (flash / flash-lite), voice + direction
- [x] Edge TTS: km-KH-PisethNeural / km-KH-SreymomNeural + en-US voices, rate + pitch;
      clear "Edge TTS unavailable" error, Gemini TTS stays usable
- [x] Settings: voice engine dropdown + voice / rate / pitch / Gemini model & direction
- [x] Audio saved per scene (Edge MP3 / Gemini WAV); duration measured in Rust from the
      audio file (ffprobe arrives with the FFmpeg sidecar in Phase 4); suspicious-length check
- [x] Edge WordBoundary events saved to `scenes/<id>/words.json`; subtitle timing helper uses
      them when present (proportional timing otherwise)
- [x] Scheduler with semaphore, cancel, `scene-progress` events
- [x] Khmer number-to-words fallback (`khmer_numbers.rs`) + unit tests
- [x] Scene list shows play button + status per scene; regenerate voice for one scene
- [x] Edited text or voice settings → voice marked `stale` → regenerated on next run

**Test:** generate voices for the 1-minute Khmer project "How was Angkor Wat built?" with
Edge TTS and play them back in the scene list; switch to Gemini TTS and regenerate one scene;
close the app mid-way through a 3-minute project; reopen → it resumes without redoing
finished scenes.

## Phase 3 — Images  ✅ (tested by the owner, 2026-10-15)

Spec: `docs/07-image-generation.md` (§7 quality check and §9 thumbnail are Phase 5).

- [x] `images/provider.rs`: `ImageProvider` trait, `ImageRequest`, `GeneratedImage`, `ImageError`
      (quota / rate limit / safety / auth / transient / other)
- [x] Cloudflare Workers AI Flux Schnell provider (default): account ID in settings, API token
      in keyring (`cloudflare_api_token`), base64 JPEG from `result.image`, daily-quota error →
      `QuotaExhaustedDaily` (next 00:00 UTC); mocked-HTTP tests (success, auth, quota, 5xx retry)
- [x] Gemini Nano Banana provider (paid): 16:9 native, up to 3 reference images,
      "Gemini image models need billing. Switch to Cloudflare Flux in Settings." on free tier
- [x] Settings → Images: provider, Cloudflare account ID + token (save / clear / **Test
      connection** = 1-step "a small green leaf"), steps 4–8, Gemini model, key-scene provider,
      Cloudflare concurrency (default 2), Flux license link
- [x] Visual Bible: generated once per project by the text model, stored as `visual_bible`,
      editable in the Script page **Look & feel** tab; scenes gain `characters`, `place`,
      `shot`, `key_scene` (shot varied, key scenes = first, chapter starts, last)
- [x] `images/prompt.rs` (pure): style, shot phrase, bible inlining, place, culture notes,
      square composition phrase, "clean image without any lettering", positive phrasing,
      2,048-char truncation (drop culture notes, then place), seed = bible seed + scene number;
      unit tests
- [x] `images/postprocess.rs`: raw saved, square → 16:9 center crop, Lanczos3 upscale to
      2150×1210 (1080p) / 1434×806 (720p), 384×216 `thumb.jpg`; unit tests
- [x] Image job: key scenes first, Cloudflare 2 at a time, cache by
      `sha256(provider + model + prompt + seed + steps)`, Regenerate = new random seed
- [x] Budget: project option **Scenes per image** (1 or 2; pairs share one image) and an
      estimate before generating ("N images · X Cloudflare (~P% of today's allowance) · Y Gemini (~$C)")
- [x] Quota pause: `paused_quota` status, progress saved, "Resume after 7:00 AM" in local time,
      **Resume** button, "Ready to resume" once the reset time has passed; never switch accounts
- [x] Safety blocks → `failed` with a readable reason; the user edits the prompt and retries
- [x] Scene grid: thumbnail, play audio, regenerate image / voice, edit image prompt;
      overall progress + per-scene image status

**Test:** 1-minute Khmer project "How was Angkor Wat built?" → ~9 images with Cloudflare Flux
in one consistent style (same characters/places look alike); one regenerated with a new seed;
edit one image prompt → only that image becomes outdated; Look & feel edit → all images outdated.

## Phase 4 — Subtitles + first render  ← **CURRENT**

- [x] `lib/subtitles.ts`: Khmer segmentation per `04-khmer.md` + unit tests
- [x] Canvas PNG rendering, font-loaded check, `subs_save` command
- [x] FFmpeg sidecar bundled (Windows x64, pinned 9.0.2, `scripts/fetch-ffmpeg.ps1`).
      ffprobe is not bundled: nothing needs it (durations are measured in Rust), saves ~100 MB
- [x] Encoder detection + cache
- [x] Per-scene clip render with pan motion + subtitle overlays, then concat
- [x] Render page: progress, ETA, cancel, open file/folder

**Test:** 3-minute Khmer video plays in VLC and Windows Media Player; subtitles are
correctly shaped, no word split across lines, in sync with voice.

## Phase 5 — Polish & long videos

- [ ] Background music with ducking + fades; bundled default tracks; user can add own
- [ ] Thumbnail generator
- [ ] 720p option; below-normal FFmpeg priority
- [ ] 20-minute test on an 8 GB RAM laptop: no crash, memory stays < 1.5 GB for the app
- [ ] Installer via `npm run tauri build`; first-run onboarding
- [ ] Pipeline log viewer in Settings (no secrets)

## Phase 6 — Local GPU images

Spec: `docs/08-local-gpu.md`. The model list in `local_models.json` (URLs, sizes, sha256,
licenses that allow commercial use) needs the owner's approval before it is filled in.

- [ ] GPU/VRAM detection + settings cache
- [ ] Bundle stable-diffusion.cpp Vulkan sidecar, pinned version; choose persistent mode (§3)
- [ ] `local_models.json` + Local models settings page (download, verify, delete)
- [ ] `LocalSdCpp` provider + OOM fallback
- [ ] `sd15` prompt adapter + short bible entries
- [ ] Cool-down, battery warning, speed/ETA, benchmark
- [ ] Provider auto-selection + one-time install card

**Test:** on a 4 GB GPU laptop, 1-minute Khmer project with local images: all images
generated without quota, consistent style, no crash, temperatures acceptable.

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
- 2026-10-15 — Phase 2 — voices: `TtsEngine` with Edge TTS (default, unofficial, `msedge-tts`)
  and Gemini TTS (Interactions API); Settings → Voice (engine, km/en voice, rate, pitch, Gemini
  voice/model/style, preview); per-scene `voice.mp3|wav` + `words.json` (Khmer timings are
  phrase-level); duration measured in Rust (no ffprobe yet); suspicious-length check; parallel
  voice job with resume/cancel/events; Edge outage stops the job with a "switch to Gemini TTS"
  message; Khmer number-to-words fallback; Scenes page with play / regenerate; subtitle timing
  helper. Live: 1-minute Khmer Angkor Wat project → 9 Edge voices (71 s) in ~10 s.
- 2026-10-15 — Phase 3 — images: `ImageProvider` with Cloudflare Workers AI Flux Schnell
  (default; account ID in settings, token in keyring) and Gemini Nano Banana (paid, 16:9,
  references); Visual Bible written after the outline and editable in Script → **Look & feel**;
  scenes gain characters/place/shot/key_scene (editable under "Picture"); pure prompt builder
  with 2,048-char truncation; square → 16:9 crop + Lanczos3 upscale + 384×216 thumb; image job
  (key scenes first, Cloudflare 2 at a time, hash cache, new-seed regenerate, 2 scenes per
  image); estimate before generating; `paused_quota` with local "Resume after" time and
  **Resume** / "Ready to resume" (no account switching); Settings → Images with Test
  connection; Scenes page shows thumbnails, voice + image status, regenerate / edit prompt.
  Not yet live-tested against Cloudflare (needs the owner's account ID + token).
- 2026-10-15 — Phase 3 test — owner confirmed images work (Cloudflare Flux, 9 images for the
  1-minute Khmer Angkor Wat project in ~25 s after the `seed` fix).
- 2026-10-15 — Phase 4 — subtitles + render: Khmer line breaking (`lib/subtitles.ts`, word
  segmenter, ≤ 38 graphemes, phrase/sentence breaks preferred, no orphans), cues of ≤ 2 lines
  timed from Edge phrase timings, drawn on canvas (Noto Sans Khmer, white + dark outline) and
  saved via `subs_save` with an input hash; FFmpeg 9.0.2 sidecar; encoder detection
  (QSV → AMF → NVENC → x264) cached in Settings → Video; per-scene `clip.mkv` (time-based crop
  pan, subtitle overlays, H.264 + PCM) cached by input hash, then concat with AAC once →
  one MP4 (`output/final.mp4` or `<output folder>/<title>.mp4`); trailing TTS silence trimmed
  to 0.15 s after the last word (pauses between scenes ~0.6 s instead of ~1.3 s); Render page
  with progress, ETA, cancel, open video / show in folder. Measured on the dev laptop (Intel
  QSV): 9 scenes / 69 s of video in 18 s; re-render with cached clips 1.4 s; audio/video
  durations equal within 30 ms.
- 2026-10-15 — Phase 4 (owner request) — CapCut-style editor replaces the Render page: live
  preview drawn in the WebView (image pan identical to the FFmpeg crop, voice-driven clock,
  subtitles via the same `paintCue` as the export), timeline with thumbnails / subtitle cues /
  voice blocks / playhead / zoom, right panel, Export dialog. Subtitles now Kantumruy Pro
  (Khmer + Latin subsets) with a **word tracker**: the spoken word on a coloured box
  (5 colours or off, per project). Export draws one PNG per word state and FFmpeg reads them as
  one `track.ffconcat` input with a single overlay (SUBS_VERSION 2, CLIP_VERSION 2). Verified:
  track timing exact in FFmpeg (states switch at 1.5 s / 3.5 s as listed); 9-scene export 17 s.
