# 08 — Local GPU Image Generation

Adds a third image provider, `local_sdcpp`, that generates images **on the user's own
GPU** with **stable-diffusion.cpp** (MIT license). Free, unlimited, no daily quota,
works offline. Builds on the `ImageProvider` trait and prompt/post-processing rules in
`07-image-generation.md`; read that first.

> Before coding, read the current stable-diffusion.cpp README, `examples/cli/README.md`,
> `docs/lcm.md`, `docs/lora.md` and the backend/performance guides, and run
> `sd-cli --help` on the exact release you bundle. **Do not guess CLI flags.**
> Facts below were checked on 2026-10-15.

## 1. Scope

| Step | Local? |
|---|---|
| Images | ✅ This doc |
| Voice | Already CPU-friendly (Edge TTS / Gemini); not part of this doc |
| Script (LLM) | ❌ Stays on Gemini (small local LLMs write poor Khmer) |
| Video animation | ❌ Needs 12 GB+ VRAM |

## 2. Hardware tiers

Detect at startup and cache in `settings.json` (`gpu.name`, `gpu.vendor`, `gpu.vram_mb`).

| Dedicated VRAM | Local mode | Default model profile |
|---|---|---|
| < 4 GB / integrated only | Hidden (show "Your GPU has less than 4 GB, using Cloudflare") | — |
| 4–5 GB (DXGI < 5,500 MB) | Available | `zimage_turbo_small` (Q3_K) |
| 6 GB+ (DXGI ≥ 5,500 MB; 6 GB cards report ~5,900) | Available | `zimage_turbo` (Q4_K) |

**VRAM detection on Windows:** use the `windows` crate, DXGI
`CreateDXGIFactory1` → `EnumAdapters1` → `GetDesc1`, read `DedicatedVideoMemory`,
skip adapters with the software flag. Pick the adapter with the most dedicated memory.
Works for NVIDIA, AMD and Intel.

## 3. Engine: stable-diffusion.cpp sidecar

- Bundle the **Vulkan** Windows build (works on NVIDIA, AMD, Intel) as a Tauri sidecar,
  like FFmpeg: `src-tauri/binaries/sd-cli-x86_64-pc-windows-msvc.exe` plus its DLLs.
- CLI binary name in current releases: **`sd-cli`**.
- Optional later: a **CUDA** build for NVIDIA users, downloaded on demand (large DLLs),
  offered only if `gpu.vendor == NVIDIA`. Not in the first version.
- Pin the release version in `src-tauri/resources/local_models.json` (`engine_version`)
  and record it in the pipeline log.

### Keep the model loaded (important for speed)

Loading a model takes seconds; doing it for every one of 170 images is wasteful.
Evaluate in this order and document the choice in this file:

1. A **server / persistent mode** shipped with the release (check the repo's examples
   and the embedded web UI) → start once per generation run, send one request per image.
2. A maintained **Rust bindings crate** for stable-diffusion.cpp (check crates.io) →
   load the model once in-process. Only if it builds cleanly on Windows with Vulkan.
3. Fallback: **one `sd-cli` process per image**. Simple and robust; accept the overhead.

**Chosen (2026-10-15): option 1, `sd-server`.** The pinned release `master-929-3f8527a`
(2026-09-27, Vulkan Windows build, sha256 `60e6850d…2fd35`) ships `sd-server.exe` with a native
async API (`examples/server/api.md`): `POST /sdcpp/v1/img_gen` → `{id}` (202), poll
`GET /sdcpp/v1/jobs/{id}` (`queued|generating|completed|failed|cancelled`, result
`images[0].b64_json`), cancel with `POST /sdcpp/v1/jobs/{id}/cancel`, readiness =
`GET /sdcpp/v1/capabilities`. LoRAs only via the request's `lora[]` (`path` relative to
`--lora-model-dir`; prompt `<lora:…>` tags are ignored by the server). `local/engine.rs` starts
it once per image job on a free 127.0.0.1 port with the profile's files and keeps it until the
job ends (`kill_on_drop`). Bundled as an app **resource** (`resources/sdcpp/` → `sdcpp/`, the
exe needs its DLLs next to it), fetched by `scripts/fetch-sdcpp.ps1`; `sd-cli.exe` is not
shipped. Licences: `ggml.txt`, `stable-diffusion.cpp.txt` (MIT) ship with it.

**GPU device:** `sd-server --list-devices` prints `Vulkan0	Intel(R) UHD Graphics`,
`Vulkan1	NVIDIA GeForce RTX 4050 Laptop GPU` on the owner's laptop — hybrid laptops list the
integrated GPU first, so the app matches the DXGI GPU name and passes `--backend Vulkan1`.

Whatever mode: run at **below-normal priority**, one generation at a time, cancellable
(kill the child / drop the context), and never run at the same time as an FFmpeg render:
take the app-wide `AppState::ffmpeg_lock` (03 Step 6x) for each image, since the export may
use NVENC/QSV/AMF on the same GPU.

## 4. Model profiles

Defined in `src-tauri/resources/local_models.json` (compiled into the app with
`include_str!`; files are downloaded on demand, never bundled in the installer). File `role`s:
`model` (full checkpoint, `-m`), `diffusion` (`--diffusion-model`), `vae`, `clip_l`, `t5xxl`,
`llm` (`--llm`, e.g. Qwen3 for Z-Image), `lora` (per request). `prompt_style`: only `full` (the
07 §3 prompt, as for Cloudflare). Optional `max_vram_mb` (a profile is offered only below it, so a
small variant does not show up on bigger cards), optional `gen.sampler` (omitted = engine
default), optional `engine_args`. Profiles in the shipped file are owner-approved (§13).

```jsonc
{
  "engine_version": "<pinned release tag>",
  "profiles": [
    {
      "id": "zimage_turbo",
      "label": "Z-Image Turbo (6 GB+ GPU)",
      "min_vram_mb": 5500,
      "files": [
        { "role": "diffusion", "name": "z_image_turbo-Q4_K.gguf", "url": "<pinned https URL>",
          "sha256": "<hash>", "size_mb": 3686, "license": "Apache-2.0", "license_url": "<url>" },
        { "role": "llm", "name": "Qwen3-4B-Instruct-2507-Q4_K_M.gguf", "…": "…" },
        { "role": "vae", "name": "ae.safetensors", "…": "…" }
      ],
      "gen": { "width": 1024, "height": 576, "steps": 8, "cfg": 1.0, "vae_tiling": false },
      "prompt_style": "full",
      "engine_args": ["--diffusion-fa", "--offload-to-cpu"]
    }
  ]
}
```

Rules for choosing model files:
- **License must allow commercial use** (videos will be monetized). Record license name
  and URL per file; show them in Settings → Local models.
- **Do not use SDXL-Turbo** (non-commercial license).
- Every candidate is tested with the Stickman prompt (default style since 2026-10-15, 07 §1c)
  on the owner's laptop before approval: speed, peak VRAM **and peak RAM** (the diffusion model
  must fit in free VRAM, or every step streams it from RAM — see §13 for FLUX).
- Each profile lists its own VRAM minimum and license per file.
- Human must approve the final model list before release (quality + license check).

## 5. Model download & management

- Location: `%LOCALAPPDATA%\NiteanStudio\models\<profile_id>\` (use Tauri path APIs).
  Large files do not belong in roaming `%APPDATA%`.
- Download with `reqwest` streaming to `<file>.part`, **resumable** (HTTP Range),
  progress events (`model-download-progress`), verify **sha256**, then rename.
- Check free disk space before starting (need size + 10 %).
- Settings → **Local models** page: profiles with size, license link, status
  (not installed / downloading x % / installed / corrupt), Install, Delete, Verify.
- A profile is usable only when every file is installed and verified.

## 6. The `LocalSdCpp` provider

Implements `ImageProvider` from `07-image-generation.md`:

- `id()` → `"local_sdcpp"`, `supports_reference_images()` → `false`,
  `native_aspect()` → wide (generates 1024×576).
- Generate at the profile's size, **seed** from the scene (07 §3), steps/cfg/sampler
  from the profile, VAE tiling on for profiles that need it. Unlike Cloudflare Flux (which
  rejects a seed, 07 §1a), stable-diffusion.cpp honours it, so local images are reproducible:
  the same prompt + seed gives the same picture.
- Output PNG → post-processing from 07 §4 (crop to the project's aspect: 16:9 leaves the
  1024×576 Z-Image picture as is; 9:16 cuts a 576×1024 column from it, see the vertical
  probe below), then Lanczos upscale.
- Errors:
  - out of memory → `ImageError::Other("GPU ran out of memory. Try a smaller model profile or close other apps.")`
    and automatically retry once with VAE tiling + smaller size (640×384).
  - engine missing/crashes → readable error + "switch to Cloudflare" hint.
- Never returns `QuotaExhaustedDaily` (no quota locally).

## 7. Prompts

Local profiles get the same full prompt as Cloudflare (07 §3): Z-Image's Qwen3 text encoder
has no 77-token limit. No negative prompt (cfg 1.0 ignores it); "clean image without any
lettering" stays in the prompt (Z-Image renders text readily). The SD 1.5 short-prompt adapter
was removed on 2026-10-16.

## 8. Laptop-friendly behavior

- **Cool-down:** optional pause of N seconds every M images (default off on desktops,
  suggested 20 s every 10 images on laptops — detect battery presence to suggest it).
- **On battery:** warn before starting a long local run.
- Show **speed** in the UI: "≈ 4.1 s / image · 38 images left · ~3 min".
- Benchmark button in Settings: generates 2 test images and stores seconds/image,
  used for estimates.

## 9. Provider selection

| Situation | Default image provider |
|---|---|
| No GPU ≥ 4 GB | `cloudflare_flux` |
| GPU ≥ 4 GB **and** a local profile installed | `local_sdcpp` |
| User choice | any provider; per project override; `key_scene_provider` may be paid Gemini |

First time a 4 GB+ GPU is detected, show a one-time card: "Your GPU can make images
for free with no daily limit. Download the local model (~6 GB)?" → Install / Not now.

## 10. Tests

- VRAM detection: unit-test the adapter-selection logic with fake adapter lists.
- Manifest parsing + profile filtering by VRAM.
- Download: resume from `.part`, sha256 mismatch → `corrupt`.
- Profile offered only between `min_vram_mb` and `max_vram_mb`.
- Provider: mocked engine runner for success, OOM-then-retry, crash.
- Manual: real run on a 4 GB GPU, 10 images, note seconds/image and peak VRAM in the Changelog.

## 11. Roadmap entry (add as Phase 6 in `05-roadmap.md`)

- [ ] GPU/VRAM detection + settings cache
- [ ] Bundle stable-diffusion.cpp Vulkan sidecar, pinned version; choose persistent mode (§3)
- [ ] `local_models.json` + Local models settings page (download, verify, delete)
- [ ] `LocalSdCpp` provider + OOM fallback
- [ ] Full prompt for local profiles (SD 1.5 adapter dropped)
- [ ] Cool-down, battery warning, speed/ETA, benchmark
- [ ] Provider auto-selection + one-time install card

**Test:** on a 4 GB GPU laptop, 1-minute Khmer project with local images: all images
generated without quota, consistent style, no crash, temperatures acceptable.
(The owner's laptop has an RTX 4050 Laptop GPU with 6 GB — WMI reports 4 GB because its
field caps at 4 GB, hence DXGI in §2. The 4 GB tier is simulated there by limiting the
profile to 4 GB.)

## 12. Implementation notes (Phase 6)

- `local/gpu.rs` (DXGI, cached as `settings.gpu` at startup), `local/manifest.rs`,
  `local/download.rs` (Range resume, sha256, disk space + 10 %), `local/install.rs` (status,
  install, verify, delete, power status), `local/engine.rs` (sd-server), `local/provider.rs`
  (`LocalSdCpp`), `commands/local.rs`.
- Commands: `local_status`, `local_install(profile_id)` (background, `model-download-progress
  {profile_id, stage, done_mb, total_mb, error}`), `local_install_cancel`, `local_verify`,
  `local_delete`, `local_select(profile_id)` ("Use this model" for an installed profile; small
  variants are not listed above their `max_vram_mb`), `local_offer_dismiss`, `local_benchmark`.
- Settings: `image.local_profile`, `gpu`, `local {cooldown_every, cooldown_seconds,
  offer_dismissed, seconds_per_image}`. The first installed profile becomes
  `image.local_profile` and switches the provider from Cloudflare to `local_sdcpp` (§9).
- Errors: engine missing / crashed → `ImageError::Engine` (`local_engine`, stops the job like a
  bad token); out of memory → one retry at 640×384 with VAE tiling, then a normal failure.
- Measured on the owner's RTX 4050 Laptop (6 GB, Vulkan): DreamShaper 8 LCM, 768×448,
  6 steps → ~7 s / image (first image ~35 s incl. shader compilation). Quality for the
  Stickman style was poor (cartoon people, not stick figures) — see the model proposal.

## 12b. Sharing the GPU with local voices

Local voices (VoxCPM2, `09-voice-profiles.md`) use the same GPU. Only one local model is on
the GPU at a time: building the `local_sdcpp` provider stops the voice server and is refused
while a voice job is using it; voice jobs are refused while a local image job runs.

## 13. Shipped profiles (2026-10-16)

Owner-approved on 2026-10-15 (Z-Image Turbo) and 2026-10-16 (go-ahead), replacing the SD 1.5
plan. All files Apache-2.0, pinned to a Hugging Face revision, sha256 re-checked on 2026-10-16:

| Profile | Diffusion (`leejet/Z-Image-Turbo-GGUF` @ `c61c0e42`) | Text encoder (`--llm`) | VAE |
|---|---|---|---|
| `zimage_turbo` (≥ 5,500 MB) | `z_image_turbo-Q4_K.gguf`, 3,864,250,304 B, `14b375ab…086fbd` | `unsloth/Qwen3-4B-Instruct-2507-GGUF` @ `a06e946b`, `Qwen3-4B-Instruct-2507-Q4_K_M.gguf`, 2,497,281,120 B, `3605803b…c67e597` | `ae.safetensors`, 335,304,388 B, `afc8e282…529e38` |
| `zimage_turbo_small` (3,800–5,499 MB) | `z_image_turbo-Q3_K.gguf`, 3,143,559,104 B, `4b44bdaa…232980` | same | same |

- VAE source: the official `black-forest-labs/FLUX.1-schnell` file is gated (HTTP 401), so the
  byte-identical copy in `Comfy-Org/Lumina_Image_2.0_Repackaged` @ `5b072540`
  (`split_files/vae/ae.safetensors`) is used; owner condition "only if the sha matches" — it does.
- Gen: 1024×576, 8 steps, cfg 1.0, engine-default sampler, `--diffusion-fa --offload-to-cpu`.
- **Measured 2026-10-16** on the owner's RTX 4050 Laptop (6 GB, DXGI 5,920 MB, 16 GB RAM,
  Vulkan, Brave open): `zimage_turbo` **~28 s / image** (first image ~52 s incl. load), peak
  VRAM 4.4 GB, sd-server RAM 6.6 GB, ≥ 2.1 GB system RAM free throughout. Stickman preset:
  real doodle stick figures; Angkor-like temple. A stricter "pure stick figure" style text was
  tried too: thinner figures but Chinese-looking pagodas — the preset stays as it is.
  `zimage_turbo_small` is not measured (no 4–5 GB GPU available).
- **2026-10-17 (Soft 2D Explainer trial, same laptop):** 1024×576 **28.4 s / image** warm (first
  44 s incl. load), 18 images in 527 s, peak VRAM 4.5 GB, no OOM retry. **1280×720: 54–56 s /
  image**, peak VRAM 4.7 GB — about 2× the time for 1.56× the pixels, and the same seed gives a
  different picture. Upscaled to the 1080p pan image (2150×1210, Lanczos3) the 1024×576 image is
  only slightly softer; flat cartoon outlines stay clean. The profile stays at 1024×576.
  **Vertical (9:16) probes are planned next** (576×1024 generation on the RTX 4050: time,
  VRAM, OOM and quality after upscaling to the 1210×2150 pan size; the code will pick the
  generation path from the recorded result, not a hard rule, with the 9:16 crop of the
  16:9 image as fallback).
- **Negative prompt:** Z-Image runs at cfg 1.0, where `sd-server` ignores the negative prompt
  (the provider sends an empty one). "No text" / "no line art" rules go in the positive prompt
  (`StylePreset::prompt_rules`, 07 §1c); the Qwen3 encoder follows them (no lettering in 34
  test images).
- **Memory:** a 3-seed × 6-scene run (sd-server ~6.6 GB RAM) was stopped once by the host when
  the laptop ran low on system memory (16 GB, other apps open). Long runs need the browser and
  dev tools closed.

### FLUX.1 schnell — tested, rejected (2026-10-16)

`city96/FLUX.1-schnell-gguf` Q4_K_S (6,470 MB) + `clip_l` + `t5xxl_fp8_e4m3fn` + `ae`, 1024×576,
4 steps, euler. Good doodle quality, but on the 6 GB laptop the diffusion model does not fit in
free VRAM (~4.7 GB) and is streamed from RAM on every step (~10–15 s / step):

| Engine flags | s / image | Problem |
|---|---|---|
| `--offload-to-cpu` | ~70 | all 11.8 GB of weights in RAM → 14 MB free, swapping |
| + `--mmap`, `--tensor-type-rules` (T5 as q8_0) | ~85 | no change |
| + `--params-backend te=disk` | ~45 same prompt, ~210 new prompt | T5 re-read from disk for every new prompt |
| + `--max-vram 4.2` | ~45 | no change |

A T5 Q4_K_M GGUF would fix RAM but not the per-step streaming, so FLUX is not shipped.
