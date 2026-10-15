# Status — where we stopped (2026-10-16)

Read this first when continuing. Details live in the numbered docs; this file is the short
"what is done / what is next" list.

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
2. **Flux local-image plan** — shown again 2026-10-16, waiting for "go" (includes ~11.7 GB of
   downloads). Plan below.
3. **Timeline thumbnails** (not fixed yet): `Timeline.tsx` only draws a thumbnail when the clip
   is wider than 36 px; at the smallest zoom most clips are 28–36 px. Proposed fix: show at any
   width, hide only below ~12 px.
4. Tests of Phase 2 (voice), Phase 4 (render) and the editor / grammar check.

## Flux local-image plan (approved in principle, not started)

Profile `flux_schnell_gguf`, `min_vram_mb: 5500`, sd-server **Vulkan** (no CUDA), 1024×576,
4 steps, cfg 1.0, sampler euler, `--offload-to-cpu`.

| File | Source (pinned revision) | sha256 | Size |
|---|---|---|---|
| `flux1-schnell-Q4_K_S.gguf` | city96/FLUX.1-schnell-gguf @ `f495746ed9c5efcf4661f53ef05401dceadc17d2` | `4fd16477b3a5296d0cf722c4b92a9fd7f30d09ac7495826e4465d8de9c9fd973` | 6470 MB |
| `clip_l.safetensors` | comfyanonymous/flux_text_encoders @ `6af2a98e3f615bdfa612fbd85da93d1ed5f69ef5` | `660c6f5b1abae9dc498ac2d21e1347d2abdb0cf6c0c0c8576cd796491d9a6cdd` | ~246 MB |
| `t5xxl_fp8_e4m3fn.safetensors` | comfyanonymous/flux_text_encoders (same revision — re-check) | `7d330da4816157540d6bb7838bf63a0f02f573fc48ca4d8de34bb0cbfd514f09` | 4668 MB |
| `ae.safetensors` (VAE) | Comfy-Org/Lumina_Image_2.0_Repackaged @ `5b072540ef86570fecb8249c505f23d5bdeb88cd` | `afc8e28272cd15db3919bacdb6918ce9c1ed22e96cb12c4d5ed0fba823529e38` (identical to the official gated file) | ~320 MB |

Owner decisions: Vulkan sd-server; use the VAE only if its sha matches; **show the
`local_models.json` entries before downloading**.

Risk: the fp8 T5 may expand to fp16 in RAM (~9.3 GB of 15.7 GB). Fallback: T5 Q4_K_M GGUF.

Steps:
1. Add the profile to `src-tauri/resources/local_models.json` → show the owner → download.
2. Benchmark: s/image, peak VRAM, peak RAM.
3. Style A/B test: current stickman prompt vs. the owner's stickman text (a few images).
4. Fix: saving one settings section must not overwrite others (merge by section on save).
5. Remove SD 1.5 leftovers (`images/prompt_sd15.rs`, `short_style`) and ~20 unused i18n keys.
6. Update docs 05 / 07 / 08 (Phase 6 resumed with Flux), changelog.
7. Ask the owner before deleting old test downloads in the scratch folder (~31 GB).

## Dev notes

- Run clippy/tests with a separate `CARGO_TARGET_DIR` while `npm run tauri dev` is running
  (it locks `target\debug\sdcpp`).
- The engines are not in git: run `scripts/fetch-ffmpeg.ps1`, `fetch-sdcpp.ps1`,
  `fetch-audiocpp.ps1` once.
- Real-GPU voice test: `cargo test real_engine -- --ignored --nocapture` with `VOXCPM_MODEL`,
  `VOXCPM_REF`, `VOXCPM_OUT` set (see `docs/09-voice-profiles.md` §8).
