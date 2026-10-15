# 03 — Pipeline

Every step reads and writes `project.json` (see `02-architecture.md`). Each step is
idempotent: re-running it skips assets whose status is `done` and whose input hash
is unchanged.

> **Before implementing any Gemini call, read the current Gemini API docs** for that
> model (request schema, response shape, limits). Model IDs come from `settings.json`.

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

## Step 2 — User review & approve

UI only. On every edit, recompute `text_hash` (of `spoken_text` + voice settings) and
`prompt_hash` (of `style_prompt` + `image_prompt`); mismatching assets become `stale`.

## Step 3 — Voice (TTS)

**Model:** `gemini-3.8-flash-tts` (quality) or `gemini-3.8-flash-lite-tts` (cheaper,
faster). Both support Khmer and English. Input limit 8,192 tokens — one scene is far below.

- One request per scene with `spoken_text`, chosen voice, short style direction
  ("calm, warm storyteller, medium pace").
- Save as `voice.wav` (convert from the returned PCM/format per the docs).
- Measure duration with `ffprobe` → `voice.duration_ms`.
- Add 250 ms of silence padding at the end of each scene clip (done at render time,
  not in the WAV) so scenes don't run into each other.
- If duration is wildly off (< 40 % or > 250 % of expected for the text length),
  mark `failed` with reason "suspicious audio length" — TTS sometimes truncates or loops.

## Step 4 — Images

**Model:** Gemini's current image-generation model (check docs; keep ID in settings).

- Prompt = `style_prompt + ", " + image_prompt + ", 16:9, no text, no watermark"`.
- Request 16:9 if supported; otherwise generate and center-crop to 16:9.
- Save as PNG; then create `image_1080.png` scaled so it is **~12 % larger** than the
  output frame (2150×1210 for 1080p) — needed for the pan motion.
- Safety blocks: mark `failed` with the reason; the user edits the prompt and retries.

## Retry / rate limits (all Gemini calls)

Every Gemini request (script, key test, and later TTS / images) goes through
`GeminiClient::call` → `retry::with_retry`; there is no other path to the API.

- 408 / 429 / 500 / 502 / 503 / 504 / network errors / timeouts → exponential backoff
  (+ up to 1 s jitter), honor `Retry-After` (seconds, capped at 120 s) if present.
  Max 5 attempts, then `failed`. Waits between attempts: 2 s, 4 s, 8 s, 16 s
  (5 attempts = 4 waits, ≈ 30 s in total; the formula would give 32 s for a 6th attempt).
- Before each wait a `gemini-retry` event is emitted (model, reason
  `busy | rate_limited | network | fallback`, next attempt n/5, delay) and a WARN line is written
  to the log, so the UI shows "Google is busy, retrying (attempt 2/5) in 4 s…" instead of
  looking stuck.
- 400 / 401 / 402 / 403 / 404 → no retry; show the message (bad key, no credits, safety block,
  invalid request, unknown model). 401/403 or a 400 mentioning "API key" → "check your key".
- Error bodies are parsed leniently: only `error.message` is used (`code` is an int in the
  classic shape and a string in the newer one).
- Every failure carries a stable `kind` (`busy, rate_limited, network, invalid_key, payment,
  not_found, blocked, bad_response, missing_key, other`). The UI shows a friendly translated
  message for the kind, a **Try again** button, and the raw Gemini error under **Details**;
  the raw error is also written to the log.

### Text fallback model

- Setting `models.text_fallback` (default `gemini-3.6-flash`, empty = off), editable in Settings.
- Rule: if a **text** call to `models.text` still fails with **503** after all 5 attempts, the
  same request is sent to the fallback model once — that one call again uses the normal retry
  rules. A `gemini-retry` notice with reason `fallback` is emitted first.
- Once a job had to switch, it stays on the fallback model for the rest of that job (no point
  waiting ~30 s on a busy model for every chapter). The next job starts on the primary again.
- Only 503 triggers it — not 429 (quota is per key, another model doesn't help), not 4xx.
- This is model fallback on the **same key**, never key rotation.

### Logs

`projects/<id>/logs/pipeline.log` (job start/finish, every retry, fallback switch, raw errors)
and `logs/app.log` (key tests). The API key is redacted before writing; files rotate to `.1`
at 1 MB.
- Semaphore per endpoint kind (default 3 concurrent).
- **One API key only. Never rotate keys.**

## Step 5 — Subtitles (frontend)

Done in React because only the browser shapes Khmer reliably. Details in `04-khmer.md`.

1. Split `narration` into display lines (max ~38 Khmer graphemes / ~42 Latin chars, max 2 lines on screen).
2. Timing: distribute the scene's `duration_ms` across lines proportionally to
   grapheme count (good enough; word-level timing is a later improvement).
3. Draw each line on a transparent `<canvas>` (full frame width, ~180 px tall),
   white text, dark outline + soft shadow, bundled Khmer font.
4. `canvas.toBlob('image/png')` → bytes → `subs_save` command → Rust writes files.

## Step 6 — Render (FFmpeg)

Render **per scene to `clip.mp4`, then concat**. Never build one giant filter graph
for 170 scenes.

### 6a. Scene clip

Variables: `W×H` = 1920×1080, `D` = scene duration in seconds (voice + 0.25 s),
`IW×IH` = enlarged image size. Pan direction alternates per scene (left→right,
right→left, top→bottom…) so motion doesn't feel repetitive.

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

- Use `crop` with a time expression for motion. **Do not use `zoompan`** — it is very slow.
- All clips must share identical codec, resolution, fps, pixel format, audio
  params, so concat can stream-copy.
- Spawn with below-normal process priority; parse `-progress pipe:1` for percent.

### 6b. Concat

`concat.txt` with `file 'scenes/s001/clip.mp4'` lines →
`ffmpeg -f concat -safe 0 -i concat.txt -c copy output/narration.mp4`

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

At startup (cache result in settings), try encoding 1 s of `testsrc` with each
encoder in order and keep the first that works:

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
