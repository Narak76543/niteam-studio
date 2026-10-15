# 09 — Local voices (VoxCPM2) and Voice Profiles

Owner decision 2026-10-16 (CLAUDE.md, Exception 4): narration may also be made **on the
user's GPU** with **VoxCPM2** (OpenBMB, Apache-2.0) running in **audio.cpp** (Apache-2.0).
Edge TTS and Gemini TTS stay fully working; nothing is mixed automatically.

## 1. Why these parts (spike, 2026-10-16)

| Option | Result |
|---|---|
| audio.cpp v0.9.0 | Only runtime with Windows release builds (CUDA, Vulkan). **Chosen.** |
| VoxCPM.cpp, llama.cpp-omni | No releases / no Windows builds. |

Measured on the dev laptop (RTX 4050 Laptop, 6 GB) with the Q8_0 GGUF:

| Backend | Request | Result |
|---|---|---|
| CUDA | paragraph (24 s audio) | RTF ≈ 0.5, peak 5.4 GB |
| Vulkan | paragraph | **out of memory** in the AudioVAE graph |
| Vulkan | **one sentence per request** | RTF ≈ 0.63, peak 5.0 GB ✔ |

Decision (owner-approved): **Vulkan build + Q8_0 + sentence chunking**. Vulkan runs on
NVIDIA, AMD and Intel without a CUDA install; the bundled zip has no CUDA DLLs.

## 2. Files

- `scripts/fetch-audiocpp.ps1` — downloads the pinned **audio.cpp v0.9.0 Vulkan** zip
  (sha256 checked) into `src-tauri/resources/audiocpp/` (gitignored, bundled as a Tauri
  resource). Only `audiocpp_server.exe` + DLLs + `model_specs/` are kept.
- `src-tauri/resources/local_tts_models.json` — the voice model: `voxcpm2_q8`,
  `voxcpm2-q8_0.gguf` (2.8 GB) from a **pinned Hugging Face revision**, sha256, licence.
  Downloaded on demand with the same resumable, verified installer as the image profiles
  (`local/install.rs`, `Package` trait) into `%LOCALAPPDATA%\NiteanStudio\models\voxcpm2_q8\`.
- `src-tauri/src/tts/`
  - `voxcpm_server.rs` — one shared `audiocpp_server` process (`AppState.voice_server`).
  - `voxcpm.rs` — `VoxCpmTts` (`TtsEngine`): chunk → speak → check → trim → join.
  - `chunker.rs` — sentence splitting; `wav.rs` — WAV parse / trim / normalise / write.
  - `profiles.rs` — Voice Profiles store.
- `src-tauri/src/commands/voices.rs` — Settings → Local voices + project voice commands.
- UI: `VoiceProfilesSection`, `NewVoiceDialog`, `EditVoiceDialog`, `ProjectVoicePicker`.

## 3. The server (`voxcpm_server.rs`)

- Started on first use with a generated `server.json` (127.0.0.1, free port, `backend:
  vulkan`, `device`, model path, `idle_unload_ms: 120000`) in
  `%LOCALAPPDATA%\NiteanStudio\audiocpp\` (+ `server.log`). Below-normal priority, no window.
- **Device:** `audiocpp_server --backend vulkan --list-devices` → the device whose name equals
  the DXGI GPU (`settings.gpu.name`), else the first `[GPU]`; cached per app run. (Hybrid
  laptops list Intel as 0 and NVIDIA as 1.)
- Restarted when it died or the spec changed; stopped before a local image job and on app exit
  (`RunEvent::Exit`). After 2 minutes without requests the model is unloaded (VRAM freed),
  the process stays.
- **Request** `POST /v1/audio/speech` — fields verified against v0.9.0 (unknown fields are
  silently ignored, so the mapping is pinned by tests):

| Field | Meaning |
|---|---|
| `input` | text; a leading `(…)` is the voice description / style |
| `seed` | deterministic |
| `num_inference_steps` | profile "quality steps" (4–30, default 10) |
| `guidance_scale` | profile "closeness" (1–5, default 2) |
| `voice_ref` `{type: path, path}` | reference clip to clone |
| `reference_text` | its transcript ("ultimate" cloning) |

  `cfg_value`, `inference_timesteps`, `instruct` have **no effect** in this server.
  Output: 48 kHz mono 16-bit WAV.
- Errors: "failed to allocate" / out of memory → `OutOfMemory`; missing exe / model or crash →
  stops the voice job (`error_kind: local_voice_unavailable`), like Edge's `edge_unavailable`.

## 4. Synthesis of one scene (`voxcpm.rs`)

1. `chunker::chunk(spoken_text)` — sentences (`។ ៕ ? ! .` and newlines), over-long ones split
   at spaces then by grapheme clusters (cap: 110 graphemes Khmer ≈ 10 s, 200 English); very
   short sentences are merged forward.
2. Each piece: hold the app-wide GPU lock (`AppState.ffmpeg_lock`, shared with FFmpeg and
   local images), request with the profile's reference clip and **the same seed** for every
   piece of every scene.
3. Trim silence (−45 dBFS, 40 ms margin). If the length is far from the text's expected
   length (`audio::suspicious_length`), retry with seed+1, up to 3 takes; the last take is kept
   and the voice job flags the scene as usual.
4. Out of GPU memory → split the piece in two halves (up to 3 levels).
5. Join: 80 ms lead, 220 ms between pieces; normalise to −18 dBFS RMS, peak ≤ −1 dBFS.
6. Result: `voice.wav` + `words.json` with **one entry per piece** (sentence timings). The
   subtitle word tracker uses them like Edge's Khmer phrase timings.

The voice job runs **one scene at a time** with this engine (one GPU). The project's
`voice` snapshot is `engine: "voxcpm"`, `voice_name: <profile id>`, `direction: <style>`,
`model: "voxcpm2:<profile fingerprint>"` — the fingerprint hashes the reference audio,
transcript, style, cfg and steps, so editing a profile makes its scenes `stale`.

## 5. Voice Profiles

Stored in app data, not per project: `%APPDATA%\NiteanStudio\voices\<id>\profile.json`,
`reference.wav`, `preview.wav` (cached "Listen" sample; removed when the profile changes).

```jsonc
{
  "id": "01k…", "name": "Narrator", "language": "km",
  "mode": "design",                  // design | clone | ultimate
  "description": "warm, calm female narrator",
  "reference_wav": "reference.wav",
  "reference_transcript": "សួស្តី! ថ្ងៃនេះ…",   // empty = plain cloning
  "style_hint": "calm, medium pace",  // sent as "(…)" before the text; no parentheses
  "cfg_value": 2.0, "inference_timesteps": 10,
  "created_at": "…", "preview_wav": "preview.wav",
  "consent_at": null                  // set for "From my recording"
}
```

**Every profile synthesizes by cloning its reference clip** — a description alone gives a
different voice on every request, so it is only used to *find* a voice:

- **From a description:** the user describes the voice (English); the app speaks a fixed test
  sentence 3 times with different seeds (`voices/_design/0..2.wav`); the chosen sample becomes
  `reference.wav` and the test sentence its transcript (mode `design`, ultimate cloning).
- **From my recording:** WAV/MP3/M4A/FLAC/OGG → FFmpeg to 16 kHz mono → silence trimmed →
  must be 5–30 s of speech. Optional transcript → `ultimate`, else `clone`. **Consent
  required**: the user confirms it is their own voice or one they may use (`consent_at`).
- Edit (name, style, closeness, steps), Duplicate, Delete, Listen, Make default.
- `settings.voice.voxcpm_profile` = default (last chosen) profile. `project.voice_profile`
  overrides it per project (Scenes page picker); a deleted profile falls back to the default.

## 6. Sharing the GPU with local images

Only one local model on the GPU at a time:
- A local image job stops the voice server first and is refused while a voice job is using it.
- Voice generation / design / preview is refused while a local image job runs.
- Every request also holds the app-wide GPU/FFmpeg lock, so export and voices never overlap.

## 7. Commands

| Command | Purpose |
|---|---|
| `voices_status()` | GPU, engine present, model status, profiles, default profile |
| `voices_model_install / _verify / _delete()` | download (event `model-download-progress`, `profile_id: voxcpm2_q8`; pause = `local_install_cancel`), sha256 check, delete |
| `voice_design_samples(language, description)` | make 3 candidates; returns the count |
| `voice_design_sample_audio(index)` | candidate WAV bytes |
| `voice_profile_create_design(name, language, description, index)` | candidate → profile |
| `voice_profile_import(path, name, language, transcript, consent)` | recording → profile |
| `voice_profile_update(id, name, style_hint, cfg_value, inference_timesteps)` | edit |
| `voice_profile_duplicate / _delete(id)`, `voice_profile_set_default(id)` | manage |
| `voice_profile_preview(id)`, `voice_profile_reference_audio(id)` | WAV bytes for playback |
| `project_set_voice_profile(project_id, profile_id?)` | per-project voice (also the new default) |

## 8. Tests

- Unit: chunking (Khmer clusters never split), halves, WAV round-trip / trim / normalise,
  request mapping, device parsing / picking, error classification, profile store
  (create / duplicate / delete / path safety / clamping), fingerprint changes, engine with a
  fake speaker (one request per sentence, same seed, retry with a new seed, OOM halving,
  timings, loudness).
- Ignored, real GPU: `real_engine_speaks_a_khmer_scene` (`VOXCPM_MODEL`, `VOXCPM_REF`,
  `VOXCPM_OUT`). Dev laptop, 2026-10-16: 2 Khmer sentences → 15.3 s of audio in 27.5 s
  including model load, NVIDIA device chosen automatically.
