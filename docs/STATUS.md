# Status — where we stopped (2026-10-17)

Read this first when continuing. Details live in the numbered docs; this file is the short
"what is done / what is next" list.

**2026-10-17:** the 2026-10-16 work below is committed (`5e47a1f` Z-Image profiles, `421ab84`
settings patch fix, `cc58dac` image prompts / UI pass / voice refs). **Not committed yet:** the
Soft 2D Explainer preset + culture-notes fix — brief, results and next steps in
`docs/tasks/soft-2d-explainer-style.md`; waiting for the owner to review the contact sheets.

## Done (committed on `main`)

| Commit | What |
|---|---|
| `6923cd4` | Settings → Local voices: engine line no longer says "audio.cpp" twice |
| `c15cfda` | **Local voices (VoxCPM2)**: audio.cpp v0.9.0 Vulkan server, Q8 model download, `VoxCpmTts` engine (sentence chunking, fixed seed, retry, OOM halving, timings), Voice Profiles (from description / from own recording with consent), project voice picker. Spec: `docs/09-voice-profiles.md` |
| `4f11f80` | Editor layout: long projects stay inside the window; timeline follows the playhead |
| `570a46e` | Phase 6 (local images) paused for this version; Z-Image Turbo plan recorded in 08 §13 |
| `0ee7812`, `40f306c` | Phase 6 backend + UI (GPU detection, sd-server engine, model downloads, settings) |
| `7080498` | Stickman preset, light mode only, names stay as written, Gemini grammar check |

Checks at `c15cfda`: clippy clean, 206 Rust tests + 32 frontend tests pass, typecheck and
lint clean. Real-GPU test: 2 Khmer sentences → 15.3 s of audio in 27.5 s (incl. model load).

## State on the owner's laptop (not in git)

- Voice model installed: `%LOCALAPPDATA%\NiteanStudio\models\voxcpm2_q8\voxcpm2-q8_0.gguf`
  (sha256 verified).
- Settings → Voice engine = **VoxCPM2** (saved).
- 3 voice profiles in `%APPDATA%\NiteanStudio\voices\`: **Female narrator** (261 Hz),
  **Male narrator (deep)** (102 Hz), **Male narrator (young)** (109 Hz). Made with the app's
  "From a description" flow; pitch checked.
- To do by the owner: press **Make default** on *Female narrator* (Settings → Local voices).

## Waiting for the owner

1. **Test local voices** (`docs/05-roadmap.md` → "Local voices"): 1-minute Khmer project with
   one of the new voices — same voice in every scene, natural pauses, subtitles follow the
   sentences; switching the project voice marks scenes outdated.
2. **Test local images (Z-Image Turbo)** (`docs/05-roadmap.md` → Phase 6): the model files are
   already in `%LOCALAPPDATA%\NiteanStudio\models\zimage_turbo\` (sha256 verified), so
   Settings → Local images should show it as installed. Select it, run a 1-minute Khmer project
   with local images. Measured outside the app: ~28 s / image on the RTX 4050.
3. **Delete the FLUX test files?** `models\flux_schnell_gguf\` (11.7 GB) is not used by the app
   (FLUX rejected, 08 §13). Also the old test downloads in the scratch folder (~31 GB).
4. **Timeline thumbnails** — fixed 2026-10-16 (`MIN_THUMB_PX = 12` in `Timeline.tsx`).
6. **Images on topic + UI pass (2026-10-16, not committed yet)**: new `image_prompt` rules and
   fuller Stickman preset (new projects), **Improve image prompts** on the Scenes page (existing
   projects; uses Gemini text calls), Projects cover images, chapter-grouped Scenes, Settings
   section list, wider editor. Test: run *Improve image prompts* on one project, then generate
   images.
5. Tests of Phase 2 (voice), Phase 4 (render) and the editor / grammar check.

## Local images: what was decided (2026-10-16)

- FLUX.1 schnell Q4_K_S was downloaded and benchmarked: 45–210 s / image and RAM full on the
  6 GB laptop (model streamed from RAM every step). Rejected — numbers in 08 §13.
- Z-Image Turbo (the plan approved on 2026-10-15) ships instead: ~28 s / image, peak VRAM 4.4 GB.
- Style A/B on Z-Image: the current Stickman preset vs. a stricter "pure stick figure" text —
  the preset gave the better temple; preset unchanged.
- Settings-save fix done (sections send only changed fields). SD 1.5 leftovers and 18 unused
  i18n keys removed.

## Dev notes

- Real-GPU image test: `cargo test engine_probe -- --ignored --nocapture` with `NITEAN_SDCPP`,
  `NITEAN_FILES`, `NITEAN_ARGS`, `NITEAN_OUT`, `NITEAN_TAG` (see `local/engine_probe.rs`).
- Run clippy/tests with a separate `CARGO_TARGET_DIR` while `npm run tauri dev` is running
  (it locks `target\debug\sdcpp`).
- The engines are not in git: run `scripts/fetch-ffmpeg.ps1`, `fetch-sdcpp.ps1`,
  `fetch-audiocpp.ps1` once.
- Real-GPU voice test: `cargo test real_engine -- --ignored --nocapture` with `VOXCPM_MODEL`,
  `VOXCPM_REF`, `VOXCPM_OUT` set (see `docs/09-voice-profiles.md` §8).
