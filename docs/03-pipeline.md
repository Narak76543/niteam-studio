# 03 — Pipeline

Every step reads and writes `project.json` (see `02-architecture.md`). Each step is
idempotent: re-running it skips assets whose status is `done` and whose input hash
is unchanged.

> **Before implementing any Gemini call, read the current Gemini API docs** for that
> model (request schema, response shape, limits). Model IDs come from `settings.json`.

## Step 0 — Read the document ("From a document" projects only)

`docs/10-document-source.md`: 50-page chunks, extracted text when it passes the quality check
(else the chunk's PDF pages), section notes → Source summary → user review. Resumable per
chunk; free-tier quota pauses the job. The script (Step 1) then gets the selected points as a
source block and writes `source_pages` / `general_knowledge` per scene; its model chain starts
with `models.document`.

## Step 1 — Script generation

**Input:** topic, language, target minutes, style preset.
**Model:** a Gemini text model (configurable), with structured JSON output.

Scene budget: ~6–8 s per scene →
`scene_count ≈ target_minutes * 60 / 7` (20 min ≈ 170 scenes).

For long videos generate in **two calls**:
1. **Outline**: title, hook, 5–10 chapters with one-line summaries.
2. **Scenes per chapter**: one call per chapter, returning that chapter's scenes.
   (Avoids output-token limits and keeps each response small enough to retry.)

Scene JSON returned by the model:

```json
{ "narration": "…", "spoken_text": "…", "image_prompt": "…", "mood": "curious" }
```

Prompt rules to include:
- Storytelling tone, strong hook in scene 1–2, cliffhanger before each new chapter.
- `narration`: one or two sentences, in the project language.
- `spoken_text`: same as narration but numbers, dates, units and foreign names written
  out as they should be spoken (see `04-khmer.md`).
- `image_prompt`: **English**, describes only the scene content, no style words,
  no text/letters in the image, no real living people.
- Facts must be accurate; mark uncertain claims so the user can check them.

Validate the JSON with serde; on parse failure retry once with the error appended.

**Implementation (Phase 1):**
- Endpoint: `POST /v1beta/models/{text_model}:generateContent`, key in the `x-goog-api-key`
  header (never in the URL). Structured output via
  `generationConfig.responseFormat.text = { mimeType: "APPLICATION_JSON", schema }`
  (the current field; `responseSchema` is deprecated). If the API rejects `responseFormat`
  with a 400, the client falls back to `responseMimeType: "application/json"` +
  `responseJsonSchema` for the rest of the session. Parts with `thought: true` are ignored.
- Scene plan (`script_job::plan_scenes`): `total = round(minutes·60/7)` clamped to 3–200,
  `chapters = clamp(total/5, 2, 10)`, scenes spread evenly (3 min → 26 scenes / 5 chapters,
  20 min → 171 / 10). The outline schema asks for exactly that many chapters; each chapter
  call asks for exactly its scene count.
- `project.json` is saved after the outline and after each chapter (`chapter.done = true`).
  Re-running `script_generate` skips the outline and finished chapters. A retried chapter
  replaces its own scenes, never duplicates them.
- Each chapter prompt gets the full outline and the previous chapter's last narration line for
  continuity; chapter 1 must open with the hook, middle chapters end on a cliffhanger, the last
  one concludes.
- Moods are an enum (`curious, dramatic, calm, hopeful, tense, joyful, somber, awe, playful`);
  unknown values become `calm`. `fact_note` holds uncertain claims for the user to check.

## Step 1b — Grammar & phrasing check (Script page → "Check grammar")

`pipeline/proofread.rs` + `gemini/proofread.rs`, owner request 2026-10-15.
- Only in `script_draft`. Scenes are sent in batches of 15 (`scene_id`, `narration`,
  `spoken_text`) through the same `script::request_json` path as the script (text model chain,
  retries, `gemini-retry` scope `script`, one re-ask on invalid JSON). Schema:
  `{fixes: [{scene_id (enum of the batch ids), narration, spoken_text, note}]}`.
- Prompt: careful copy editor in the project language; fix spelling (Khmer coeng/vowels), grammar,
  unnatural phrasing, narration ≠ spoken_text; **smallest change**, keep meaning/facts/length;
  the names rule (04 §4c); return only scenes that need a change, `note` in the project language.
- Each fix is stored on its scene as `scene.proofread = {narration, spoken_text, note}` (a
  suggestion, never applied silently). `scene.proofread_hash` = hash of the text that was
  checked (+ language + names list), so a re-run only sends changed scenes (resumable; saved
  after every batch). A fix is dropped if the scene was edited meanwhile, if it changes
  nothing, or if it removes a name from `keep_terms`. Thai/Lao letters are rejected.
- UI: progress strip, scene-list marker, suggestion card with a grapheme-level diff
  (`src/lib/textDiff.ts`), **Accept** / **Dismiss** per scene, **Accept all** / **Dismiss all**.
  Accept goes through the normal edit path (voice etc. become outdated); Dismiss keeps the text
  and the scene isn't checked again until it changes. Editing a scene drops its suggestion.
- Events: `proofread-progress {project_id, stage: running|done|failed|cancelled, checked, total,
  suggestions, error, error_kind}`. Commands: `proofread_start / _cancel / _running`,
  `proofread_accept / proofread_dismiss(project_id, scene_ids)`. Script generation and the check
  never run at the same time for a project.

## Step 1c — Improve image prompts (Scenes page, optional)

`pipeline/prompt_rewrite.rs` + `gemini/image_prompts.rs`: rewrites the `image_prompt` of every
scene (batches of 12, with the Visual Bible, the scene's characters/place and the previous
batch's last prompt for variety) using the same rules as the script (`image_prompt_rules`,
07 §2). Narration and voices are untouched. Saved per batch; a scene whose narration changed
meanwhile keeps its prompt. Changed prompts make their images outdated, so the next image job
makes them again. Not while rendering; image generation and the rewrite never run at the same
time. Event `prompt-rewrite-progress {project_id, stage, done, total, changed, error,
error_kind}`; commands `image_prompts_rewrite / _cancel / _running`.

## Step 2 — User review & approve

UI only. On every edit, recompute `text_hash` (of `spoken_text` + voice settings) and
`prompt_hash` (of `style_prompt` + `image_prompt`); mismatching assets become `stale`.

## Step 3 — Voice (TTS)

Three engines behind the `TtsEngine` trait (`src-tauri/src/tts/`), chosen in Settings → Voice:

| Engine | How | Output | Word timings |
|---|---|---|---|
| **Edge TTS** (default) | `msedge-tts` crate, Microsoft Edge "Read aloud" WebSocket. **Unofficial** — see CLAUDE.md | MP3 `audio-24khz-48kbitrate-mono-mp3` | yes → `words.json` |
| **Gemini TTS** | Interactions API `POST /v1beta/interactions` with the user's key; model `gemini-3.8-flash-tts` or `-lite-tts` | WAV (24 kHz mono 16-bit) | no |
| **VoxCPM2 (local)** | audio.cpp server on the user's GPU, one request per sentence, cloning a Voice Profile — **see `09-voice-profiles.md`** | WAV (48 kHz mono 16-bit) | per sentence → `words.json` |

**Edge TTS**
- Voices: Khmer `km-KH-SreymomNeural` (default) / `km-KH-PisethNeural`; English
  `en-US-AvaNeural` (default) and other `en-US-*` neural voices. Settings expose **rate**
  (−50…+50 %) and **pitch** (−50…+50 Hz).
- Text is XML-escaped before it goes into the SSML (the crate inserts it verbatim).
- Each attempt runs in its own task with a 60 s timeout (3 attempts, 2 s / 4 s apart); release
  builds use `panic = "unwind"` so a crate bug becomes an error, not a crash.
- When all attempts fail the error is **"Edge TTS unavailable — switch to Gemini TTS in
  Settings"** (`error_kind: edge_unavailable`) and the job **stops** (remaining scenes stay
  `pending`) instead of failing each scene slowly. Engines are never mixed automatically inside
  one project — the user switches to Gemini TTS and presses Continue.
- WordBoundary events (100-ns ticks) are saved as `scenes/<id>/words.json`:
  `[{ "text": "…", "start_ms": 112, "end_ms": 3262 }, …]`. For **Khmer, Edge reports
  space-separated phrases, not words** (it does not segment Khmer).

**Gemini TTS** (request shape from ai.google.dev/gemini-api/docs/speech-generation)
```json
{ "model": "gemini-3.8-flash-tts",
  "input": [{ "type": "user_input", "content": [{ "type": "text", "text": "<spoken_text>",
     "annotations": [{ "type": "speech_metadata", "style": "calm, warm storyteller" }] }] }],
  "response_format": { "type": "audio" },
  "generation_config": { "speech_config": [{ "voice": "Kore" }] },
  "store": false }
```
Audio = last `steps[type=model_output].content[type=audio].data` (base64 WAV; `audio/l16` is
wrapped in a WAV header). Same retry rules as all Gemini calls (5 attempts).

**Both engines**
- One request per scene with `spoken_text` (NFC, zero-width spaces removed; for Khmer the
  `khmer_numbers` fallback converts any digits left in it).
- Saved as `scenes/<id>/voice.mp3` (Edge) or `voice.wav` (Gemini, VoxCPM2); old voice files
  of the scene are removed first.
- Duration is measured in Rust from the file (WAV header / MP3 frame count) →
  `voice.duration_ms`. (ffprobe is not needed for this; it arrives with FFmpeg in Phase 4.)
- If duration is wildly off (< 40 % or > 250 % of the expected length for the text and rate),
  the file is kept but the scene is marked `failed` with `error_kind: suspicious_length`, so the
  user can listen and regenerate.
- Add 250 ms of silence padding at the end of each scene clip (done at render time,
  not in the audio file) so scenes don't run into each other.

**Scheduler** (`pipeline/voice_job.rs`)
- Starts automatically after **Approve**, or with Generate / Continue on the Scenes page.
- The Settings voice is resolved for the project language and stored as `project.voice`; it is
  part of every scene's `text_hash`, so changing voice, rate, pitch or engine makes existing
  voices `stale`.
- Scenes whose voice is `done`, hash-current and whose file exists are skipped (resume).
- Up to `concurrency.tts` scenes in parallel (VoxCPM2: one at a time); cancel via `generate_cancel`; scenes interrupted
  by cancel go back to `pending`. `project.json` is saved after every scene.
- Events: `scene-progress` per scene, `generate-progress` for the job, `gemini-retry`
  (scope `voice`) while Gemini TTS / Edge TTS retry.
- Single scene: `scene_regenerate(project_id, scene_id, "voice")` forces that scene.

## Step 4 — Images

**Specified in [`07-image-generation.md`](07-image-generation.md)** — providers (Cloudflare
Flux Schnell default, Gemini Nano Banana paid), Visual Bible, prompt building,
post-processing, budget, quota pause, caching. Summary:

- Provider per scene: `image.provider`, or `image.key_scene_provider` for key scenes.
- Prompt = style + shot phrase + subject (bible descriptions inlined) + place + culture notes
  + composition phrase (square providers) + "clean image without any lettering", ≤ 2,048 chars.
- Seed = `visual_bible.seed` + scene number; Regenerate uses a new random seed.
- Raw image saved; square → center-crop 16:9; Lanczos3 upscale to the pan size
  (2150×1210 for 1080p, 1434×806 for 720p); 384×216 `thumb.jpg`.
- Key scenes first; Cloudflare 2 at a time; cached by prompt hash.
- Daily Cloudflare allowance used up → `paused_quota`, resume after 00:00 UTC.
- Safety blocks: mark `failed` with the reason; the user edits the prompt and retries.

## Retry / rate limits (all Gemini calls)

Every Gemini request (script, key test, and later TTS / images) goes through
`GeminiClient::call` → `retry::with_retry`; there is no other path to the API.

- 408 / 429 / 500 / 502 / 503 / 504 / network errors / timeouts → exponential backoff
  (+ up to 1 s jitter), honor `Retry-After` (seconds, capped at 120 s) if present.
  Waits double: 2 s, 4 s, 8 s, … (n attempts = n − 1 waits).
  - **Text calls:** 3 attempts per model (waits 2 s, 4 s), then the next model of the chain
    (see below).
  - **Other calls** (key test; later TTS / images): 5 attempts (waits 2, 4, 8, 16 s).
- Before each wait a `gemini-retry` event is emitted (model, reason
  `busy | rate_limited | network | next_model`, next attempt n/max, delay, exact error) and a
  WARN line is written to the log, so the UI shows
  "Google is busy, retrying… (model X, attempt 2/3, in 4 s)" instead of looking stuck.
- 400 / 401 / 402 / 403 / 404 → no retry; show the message (bad key, no credits, safety block,
  invalid request, unknown model). 401/403 or a 400 mentioning "API key" → "check your key".
- Error bodies are parsed leniently: `error.message` is used for the short message (`code` is
  an int in the classic shape and a string in the newer one). The **exact HTTP status and raw
  body** (cut at 2,000 characters) are kept as the error's `detail` and written to the log on
  every failed attempt.
- Every failure carries a stable `kind` (`busy, rate_limited, network, invalid_key, payment,
  not_found, blocked, bad_response, missing_key, other`). The UI shows a friendly translated
  message for the kind, a **Try again** button, and the raw Gemini error under **Details**;
  the raw error is also written to the log.

### Text model chain (fallback)

- Setting `models.text_chain`: ordered list of text models, 1–6 entries, editable in Settings.
  Default: `gemini-3.5-flash-lite` → `gemini-3.1-flash-lite` → `gemini-3.8-flash`
  (lighter models first: cheaper and usually less overloaded). Settings files from before
  this change (`text` / `text_fallback`) load with this default.
- Rule for every text call (outline or one chapter): try the current model with 3 attempts.
  If it still fails with a transient error (429 / 5xx / timeout / network) **or 404** (model
  renamed or retired), emit `gemini-retry` with reason `next_model` and try the next model
  (3 attempts again). 400 / key errors / safety blocks stop immediately — another model would
  get the same answer.
- If every model fails, the call fails with "All text models failed (tried a → b → c)" and
  the last model's exact error.
- The model that answered is logged ("chapter 2/5 written by gemini-3.1-flash-lite"). Later
  calls in the same job start from that model; the next job starts at the top of the list.
- Calls are per outline / per chapter, so a failure only costs that chapter: finished chapters
  are saved and **Try again** resumes at the failed one.
- Same key throughout — model fallback, never key rotation.
- Khmer guard: chapter replies whose narration or spoken text contain Thai/Lao letters
  (U+0E00–U+0EFF) are rejected and re-asked once (lite models occasionally mix them in).

### Logs

`projects/<id>/logs/pipeline.log` (job start with the model chain, every failed attempt with
exact HTTP status + body, model switches, which model wrote the outline and each chapter)
and `logs/app.log` (key tests). The API key is redacted before writing; files rotate to `.1`
at 1 MB.
- Semaphore per endpoint kind (default 3 concurrent).
- **One API key only. Never rotate keys.**

## Step 5 — Subtitles (frontend)

Done in React because only the browser shapes Khmer reliably. Details in `04-khmer.md`.

1. Split `narration` into display lines (`src/lib/subtitles.ts`: max ~38 Khmer graphemes /
   ~42 Latin chars; phrase/sentence boundaries preferred in the last 30 % of a line; punctuation
   never starts a line; a one-word line of ≤ 3 graphemes is merged with its neighbour). Lines
   are grouped into **cues of at most 2 lines** (one PNG per cue, shown from the first line's
   start to the last line's end).
2. Timing (`src/lib/subtitleTiming.ts`): if `words.json` exists, each line break is placed
   where the voice reaches that point of the text (inside a phrase, interpolated by grapheme
   count; at a phrase end, in the pause before the next phrase). Lines come from `narration`
   and timings from `spoken_text`, so positions are matched as fractions of each text's
   letter count. Without `words.json` (Gemini TTS), the scene's `duration_ms` is split
   proportionally to grapheme count. The first line starts at 0, the last ends at the
   scene duration.
3. **States** (`src/lib/subtitleStates.ts`): with the word tracker on, one state per word
   (the cue with that word highlighted, from the word's start to the next word's start);
   off, one state per cue. Word times come from `timeLines` over the words (Edge phrase
   timings interpolated by grapheme count). States are consecutive and cover the whole voice.
   Each state is drawn on a transparent `<canvas>` by `paintCue` (`src/lib/subtitleCanvas.ts`):
   full frame width × 240 px at 1080p (160 px at 720p), 56 px Kantumruy Pro 600, line height
   1.6, white fill, ~6 px dark outline + soft shadow, highlight box behind the spoken word,
   lines bottom-aligned. The font is awaited with `document.fonts.load` and checked first.
   The editor preview draws with the same `paintCue`, so preview and export match.
4. `subs_plan(project_id)` lists scenes whose subtitles are missing or outdated
   (`subs.hash` ≠ `hashing::subs_hash` = narration + voice hash + duration + resolution +
   language + `SUBS_VERSION`). For each: draw → base64 PNGs → `subs_save(project_id,
   scene_id, hash, cues)`. Rust re-checks the hash (the scene may have changed meanwhile),
   validates the PNGs and times (≤ 600 states, in time order), writes
   `scenes/<id>/subs/NNN.png` plus **`subs/track.ffconcat`** (`render/subs_track.rs`: each image
   with `duration` = next start − its start, last file repeated as the concat demuxer
   requires), and marks a done clip stale. The subtitle hash includes the highlight colour.
   Bump `SUBS_VERSION` when the subtitle look or line breaking changes (now 2).

## Step 6 — Render (FFmpeg)

Render **per scene to `clip.mp4`, then concat**. Never build one giant filter graph
for 170 scenes.

### 6a. Scene clip

Variables: `W×H` = 1920×1080, `D` = scene duration in seconds (voice + 0.25 s),
`IW×IH` = enlarged image size. Pan direction alternates per scene (left→right,
right→left, top→bottom, bottom→top; scenes 2k / 2k+1 always opposite, so the two scenes of a
shared image pan in opposite directions) so motion doesn't feel repetitive.

**Voice length:** Edge TTS MP3s carry ~0.23 s of silence before and ~0.8 s after the speech.
When `words.json` exists the clip uses the voice only up to **0.15 s after the last word**
(`render_job::trimmed_voice_ms`), so scenes are ~0.6 s apart instead of ~1.3 s. Gemini TTS
voices (no timings) are used whole.

**As implemented** (`render/clip.rs`, pure + unit-tested): the command below, but the
subtitles are **one input**, the scene's image track (`-f concat -safe 0 -i
scenes/<id>/subs/track.ffconcat`), drawn by **one** `overlay=x=0:y=816:eof_action=repeat`
(each image stays until the next; one overlay regardless of how many words), `format=yuv420p`
comes *after* the overlay, and clips are **`clip.mkv` with PCM audio** (`-c:a pcm_s16le -ar 48000 -ac 2
-f matroska`). PCM avoids the AAC priming gap at every scene boundary; audio is encoded once,
at concat. Paths are relative to the project folder (FFmpeg runs with it as working
directory). Each clip is written to `clip.tmp.mkv` and renamed when complete.

```bash
ffmpeg -y -loop 1 -framerate 30 -t D -i image_1080.png \
  -i voice.wav \
  -i subs/000.png -i subs/001.png \
  -filter_complex "
    [0:v]crop=W:H:x='(iw-W)*t/D':y='(ih-H)/2',format=yuv420p[bg];
    [bg][2:v]overlay=0:H-220:enable='between(t,0.00,2.40)'[v1];
    [v1][3:v]overlay=0:H-220:enable='between(t,2.40,5.10)'[v];
    [1:a]apad=pad_dur=0.25[a]" \
  -map "[v]" -map "[a]" -t D \
  -c:v <ENCODER> <ENCODER_OPTS> -r 30 -g 60 \
  -c:a aac -b:a 160k -ar 48000 -ac 2 \
  scenes/s001/clip.mp4
```

- Use `crop` with a time expression for motion. **Do not use `zoompan`** — it is very slow
  on large inputs.
  **Exception (2026-10-17, real-world photos only, `11-real-world-visuals.md` §9):** a gentle
  zoom uses `zoompan` **when its input is only slightly larger than the frame** — the photo is
  prepared at frame × zoom (≤ 1.08, e.g. 2074×1166 for 1080p) before the clip. Measured on
  the owner's laptop (RTX 4050 Laptop), 12 s of 1080p from a 2150×1210 image:

  | Motion | libx264 veryfast | h264_nvenc | stalled frames |
  |---|---|---|---|
  | `crop` pan (AI images) | 2.0 s | 1.7 s | 18 / 319 |
  | zoom with `scale=…:eval=frame` + `crop` | 2.7 s | 2.4 s | 57 / 319 (stutters) |
  | zoom with `zoompan` | 2.7 s | 2.2 s | 0 / 319 |

  (stalled = no change from the previous frame, signalstats YDIF). About +35 % render time
  for photo scenes; expect the same ratio on the 8 GB target. Never feed `zoompan` a
  full-size photo — always the prepared frame × zoom picture.
- All clips must share identical codec, resolution, fps, pixel format, audio
  params, so concat can stream-copy.
- Spawn with below-normal process priority; parse `-progress pipe:1` for percent.

### 6b. Concat

`concat.txt` with `file 'scenes/s001/clip.mkv'` lines →
`ffmpeg -f concat -safe 0 -i concat.txt -map 0:v -map 0:a -c:v copy -c:a aac -b:a 192k
-ar 48000 -ac 2 -movflags +faststart output/final.tmp.mp4`, then moved to the final place:
`<Settings output folder>/<title>.mp4` (unsafe file-name characters → `_`) or
`<project>/output/final.mp4`. (Phase 5 inserts the music step between concat and the move.)

### 6x. Render job (`pipeline/render_job.rs`)

- `render_start` refuses while a script / voice / image job runs, and `check_ready` requires
  every voice and image `done` with files present and every scene's subtitles current
  (the Render page draws missing subtitles first).
- Clip cache: `clip.hash` = sha of image hash + path, voice hash + path + used length, subtitle
  hash, encoder, resolution, pan, `CLIP_VERSION`. Done + same hash + file present → skipped.
- One FFmpeg process at a time app-wide (`AppState::ffmpeg_lock`), `BELOW_NORMAL_PRIORITY_CLASS`
  + `CREATE_NO_WINDOW`, `-progress pipe:1` parsed for `out_time_us`. Cancel kills the child and
  deletes the current `clip.tmp.mkv`; finished clips stay.
- Status: `rendering` while running → `done` with `render.output_path`; on failure or cancel
  back to `review`. A crash during `rendering` is reset to `review` on startup.
- Events: `render-progress {project_id, stage: clips|concat|done|failed|cancelled, scene_index,
  total, percent (0–1 of this run's work, concat weighted 10 %), eta_seconds, output_path,
  error, error_kind (ffmpeg_missing | ffmpeg | cancelled | other)}`.

### FFmpeg sidecar

Pinned Windows x64 build **FFmpeg 9.0.2 "essentials"** (gyan.dev, GPL, includes libx264,
NVENC, QSV, AMF), fetched by `scripts/fetch-ffmpeg.ps1` (sha256-checked) to
`src-tauri/binaries/ffmpeg-<host-triple>.exe` and listed in `bundle.externalBin`. Not committed
(~100 MB). Tauri copies it next to the app exe as `ffmpeg.exe`, which is where
`render::ffmpeg::Ffmpeg::locate` looks. Its LICENSE/README are copied next to it; the installer
(Phase 5) must ship them (GPL: license text + where to get the source).

### 6c. Music + ducking

```bash
ffmpeg -y -i output/narration.mp4 -stream_loop -1 -i music.mp3 -filter_complex "
  [1:a]volume=0.12[m];
  [m][0:a]sidechaincompress=threshold=0.03:ratio=8:attack=20:release=400[duck];
  [0:a][duck]amix=inputs=2:duration=first:dropout_transition=0[a]" \
  -map 0:v -map "[a]" -c:v copy -c:a aac -b:a 192k output/final.mp4
```

Fade music in over the first 2 s and out over the last 3 s.

### 6d. Thumbnail

Take the hook scene's image (or let the user pick), render the video title as a
big Khmer/English headline via the same canvas method, save `thumbnail.png` (1280×720).

## Encoder detection

On the first render (or **Detect again** in Settings → Video; cached as
`settings.render.encoder`, which the user may also pick by hand), try encoding 1 s of 1080p
`testsrc` with each encoder in order and keep the first that works:

| Encoder | Options |
|---|---|
| `h264_qsv` (Intel) | `-preset veryfast -global_quality 23` |
| `h264_amf` (AMD) | `-quality speed -rc cqp -qp_i 22 -qp_p 24` |
| `h264_nvenc` (NVIDIA) | `-preset p4 -cq 23` |
| `libx264` (fallback) | `-preset veryfast -crf 23` |

## Resume & cancel

- After each asset finishes: update scene status and save `project.json` atomically.
- On app start, any asset left in `running` → reset to `pending`.
- Cancel: trigger the `CancellationToken`, kill the FFmpeg child, delete partial
  `clip.mp4` of the current scene only.
