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

## Phase 4 — Subtitles + first render  (built; awaiting the owner's test)

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

## Phase 6 — Local GPU images  (resumed 2026-10-16 with Z-Image Turbo; built, awaiting the owner's test)

Spec: `docs/08-local-gpu.md`. Owner-approved profiles: `zimage_turbo` (6 GB+) and
`zimage_turbo_small` (4–5 GB), 08 §13.

- [x] GPU/VRAM detection + settings cache
- [x] Bundle stable-diffusion.cpp Vulkan sidecar, pinned version; choose persistent mode (§3)
- [x] `local_models.json` + Local models settings page (download, verify, delete)
- [x] `LocalSdCpp` provider + OOM fallback
- [x] Full prompt for local profiles (SD 1.5 adapter dropped; `llm` file role, optional sampler,
      `max_vram_mb`)
- [x] Cool-down, battery warning, speed/ETA, benchmark
- [x] Provider auto-selection + one-time install card
- [ ] Owner test

**Test:** on a 4 GB GPU laptop, 1-minute Khmer project with local images: all images
generated without quota, consistent style, no crash, temperatures acceptable.

## Local voices — VoxCPM2  (owner request 2026-10-16; built, awaiting the owner's test)

Spec: `docs/09-voice-profiles.md`.

- [x] audio.cpp v0.9.0 Vulkan server bundled (`scripts/fetch-audiocpp.ps1`), correct GPU on
      hybrid laptops, model unloaded when idle, stopped with the app
- [x] VoxCPM2 Q8 model download (pinned revision, sha256, resumable) in Settings → Local voices
- [x] `VoxCpmTts` engine: sentence chunking, same seed, length check + retry, OOM halving,
      trim / join / loudness, sentence timings for the word tracker
- [x] Voice Profiles: from a description (3 samples) or from the user's recording (consent),
      edit / duplicate / delete / listen / default; per-project voice on the Scenes page
- [x] One local model on the GPU at a time (voices vs local images)
- [ ] Owner test

**Test:** make a voice from a description and one from a recording; generate a 1-minute
Khmer project with it: same voice in every scene, natural pauses, subtitles follow the
sentences; switch a project to another voice → its scenes become outdated.

## Video from a document  (owner request 2026-10-17; built, awaiting the owner's test)

Spec: `docs/10-document-source.md`. Free tier only.

- [x] New project → "From a document": PDF pick (≤ 50 MB), page range for > 150 pages, video
      language, length, audience, focus, style; free-tier privacy note
- [x] Reading job: 50-page chunks, local text when clean (Khmer checked), PDF pages otherwise
      (mode logged per chunk), section notes → Source summary with page refs; resumable
- [x] Free-tier quota: daily / per-minute 429 pauses the job with the reset time, resume from the
      last finished chunk, no key switching; reading model in Settings → Models (Flash)
- [x] Review screen: tick, reorder, notes, page notes; nothing generated before confirm
- [x] Script from the selected points only (no added facts; general knowledge flagged; English
      terms kept in Khmer); scenes keep source pages (Script page editor)
- [x] "Source: …" credit line with Copy (editor side panel)
- [ ] Owner test

**Test:** make a 3-minute Khmer video from an English paper and one from a Khmer PDF: check the
summary against the PDF, untick / reorder points and confirm, check the scenes' page refs and
that no scene adds facts; copy the credit line.

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
- 2026-10-15 — Owner requests — (1) **Stickman** style preset, default for new projects,
  stick-figure rules in the Visual Bible prompt, Look & feel → Style switches a project
  (checked with 4 Flux images; "white paper" wording removed after it produced a photo of
  paper); (2) **light mode only**: dark editor theme and dark tokens removed; (3) **names stay
  as written**: product/brand/tech names in Latin letters in narration, Khmer pronunciation in
  spoken text, per-project Names list; (4) **grammar & phrasing check** with Gemini: batched,
  resumable suggestions with diff, Accept / Dismiss (all).
- 2026-10-15 — Phase 6 (paused for this version, owner: "make it stable, no need to implement
  more") — built and tested: DXGI GPU detection, stable-diffusion.cpp sd-server engine
  (pinned master-929-3f8527a, Vulkan, correct device on hybrid laptops), local_sdcpp provider,
  resumable sha256-verified model downloads, Settings → Local images, provider option, speed /
  battery hints, offer card. `local_models.json` ships with **no profiles**, so the Local GPU
  option stays disabled and nothing is downloaded. Models tried on the RTX 4050: DreamShaper 8
  LCM (~7 s/image) and SD 1.5 + LCM-LoRA (~10 s/image) — neither draws stick figures. Next
  version: the owner-approved Z-Image Turbo profile (08 §13).
- 2026-10-16 — Local voices (owner request) — VoxCPM2 (Apache-2.0) via the audio.cpp v0.9.0
  Vulkan server (Apache-2.0), Q8 model downloaded on demand. Spike: Vulkan runs out of GPU
  memory on whole paragraphs, so scenes are spoken sentence by sentence (RTF ≈ 0.63, peak
  5.0 GB on the RTX 4050). Voice Profiles (description → 3 samples, or own recording with
  consent) are cloned with a fixed seed so the voice stays the same; per-sentence timings feed
  the word tracker. Real-GPU test: 2 Khmer sentences, 15.3 s of audio in 27.5 s incl. load.
- 2026-10-16 — Phase 6 resumed — **Z-Image Turbo** ships as the local image model
  (`zimage_turbo` Q4_K for 6 GB+, `zimage_turbo_small` Q3_K for 4–5 GB; Qwen3-4B text encoder via
  the new `llm` role; 1024×576, 8 steps). RTX 4050 Laptop: ~28 s/image, peak VRAM 4.4 GB, RAM
  stays free. **FLUX.1 schnell was tested and rejected**: the 6.5 GB model does not fit in free
  VRAM and is streamed every step (45–210 s/image, RAM full) — details in 08 §13. Removed the
  SD 1.5 prompt adapter, preset short styles and 18 unused i18n keys. Fix: Settings sections now
  save only the fields they changed (`settings_save` takes a patch merged on the backend), so a
  section holding an older copy no longer undoes a finished download or another section.
- 2026-10-16 — Fix: cloned voices with long recordings failed every scene ("VoxCPM2 AudioVAE
  encoder sample capacity exceeded"). The audio.cpp encoder takes at most ~14.7 s of reference
  audio, but imports allowed 30 s. Limit is now 14 s: longer clips without a transcript are cut
  at a pause (on import, and in place for existing profiles on next use); with a transcript
  they are refused with a clear message.
- 2026-10-16 — Imported voice recordings are cleaned with FFmpeg (rumble cut below 80 Hz,
  gentle noise reduction, headroom against clipping) before they become the reference clip,
  because VoxCPM2 copies background noise into every scene. Chosen by ear on the Vanda clip.
- 2026-10-16 — Images on topic (owner request): scene `image_prompt` rules rewritten so each
  image explains its narration (exact object/event, 2–4 topic details, varied settings, visual
  devices for abstract ideas; no style/lighting words); Stickman preset draws full scenes
  instead of "lots of white space" (checked with Z-Image: clearly richer, on-topic frames).
  New **Improve image prompts** action rewrites the prompts of existing projects (narration
  unchanged). UI: Projects cards show the cover image, Scenes grouped by chapter with compact
  cards, Script page shows the image beside its prompt, Settings section list, editor gets the
  full width (icon sidebar) and thumbnails on every timeline clip, long titles wrap.
- 2026-10-17 — **Soft 2D Explainer** style preset (`soft_2d_explainer`, owner brief in
  `docs/tasks/soft-2d-explainer-style.md`): one shared round-headed cartoon figure (Visual Bible
  and scene prompts describe people only by clothing, one prop, headwear), fixed preset rules
  added to every final prompt (figure design, no line art on backgrounds, no text/letters/
  watermark). Bible `culture_notes` is now one short phrase that fits every scene. Tested on
  local Z-Image with two 6-scene Khmer history projects (3 seeds each): consistent characters,
  no text; 28 s / image at 1024×576 (1280×720: 55 s, not adopted). Not yet tested in the app UI.
- 2026-10-17 — **Video from a document** (spec `docs/10-document-source.md`): PDF → Gemini
  section notes per 50-page chunk (extracted text when clean, PDF pages for scanned / garbled
  chunks, e.g. most Khmer PDFs) → reviewed Source summary → script from the selected points with
  page refs and a general-knowledge flag → credit line. Free tier only: reading on
  gemini-3.8-flash (Flash-Lite got Khmer scan details wrong), daily / per-minute quota pauses and
  resumes from the last chunk. Live-tested on an English paper, a Khmer text PDF and a Khmer scan.
- 2026-10-17 — Subtitles: regular weight (400) and smaller (44 px at 1080p, 29 px at 720p; was
  600 / 56 px), thinner outline. `SUBS_VERSION` 3, so existing subtitle PNGs are redrawn.
- 2026-10-17 — **v0.1.0 installer**: NSIS only (`bundle.targets`), per-user install, WebView2
  download bootstrapper, bundles FFmpeg + sd-server + audio.cpp + license files, no models.
  Uninstall keeps user data (`NiteanStudio` folders and keyring entries). Installer is
  English only (NSIS has no Khmer). Unsigned. Repo pushed to GitHub with README and
  THIRD_PARTY_NOTICES.md.
- 2026-10-17 — **Dark mode + cleaner local voices**: Settings → Appearance (Same as Windows /
  Light / Dark, `settings.theme`), dark tokens in `tokens.css`. VoxCPM2 output: 8 ms fades at
  each sentence and a pause expander that turns the copied reference hiss down 12–15 dB
  between words (measured on a real scene); speech unchanged.
