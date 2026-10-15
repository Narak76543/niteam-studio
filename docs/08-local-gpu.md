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
| 4–5 GB | Available | `sd15_lcm` |
| 6–7 GB | Available | `sd15_lcm` (default) or `sdxl_lightning` (opt-in) |
| 8 GB+ | Available | user's choice incl. `flux_schnell_gguf` |

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
`lora` (per request). `prompt_style`: `sd15` (short, ≤ 75 tokens) or `flux` (the full 07 §3
prompt; T5 reads it). Optional `engine_args` (e.g. `--diffusion-fa`). **The shipped file has
no profiles until the owner approves the model list.**

```jsonc
{
  "engine_version": "<pinned release tag>",
  "profiles": [
    {
      "id": "sd15_lcm",
      "label": "Fast (4 GB GPU)",
      "min_vram_mb": 3800,
      "files": [
        { "role": "model", "url": "<https URL>", "sha256": "<hash>", "size_mb": 2000,
          "license": "<license name>", "license_url": "<url>" },
        { "role": "lora",  "url": "<LCM-LoRA for SD1.5>", "sha256": "<hash>", "size_mb": 130,
          "license": "<license name>", "license_url": "<url>" }
      ],
      "gen": { "width": 768, "height": 448, "steps": 6, "cfg": 1.5,
               "sampler": "lcm", "vae_tiling": true },
      "prompt_style": "sd15"
    }
  ]
}
```

Rules for choosing model files:
- **License must allow commercial use** (videos will be monetized). Record license name
  and URL per file; show them in Settings → Local models.
- **Do not use SDXL-Turbo** (non-commercial license).
- SD 1.5 profile: an SD 1.5 base or fine-tune that handles **simple line art / doodles on
  white** well (the default style is **Stickman** since 2026-10-15, 07 §1c) as well as
  illustration styles, fp16 `.safetensors`, plus the **LCM-LoRA for SD 1.5** (4–8 steps,
  low CFG, `lcm` sampler). Every candidate is tested with the Stickman prompt before approval.
- Larger profiles (`sdxl_lightning`, `flux_schnell_gguf`) only after `sd15_lcm` works;
  each must list its own VRAM minimum and license.
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
  `native_aspect()` → wide (generates 768×448).
- Generate at the profile's size, **seed** from the scene (07 §3), steps/cfg/sampler
  from the profile, VAE tiling on for profiles that need it. Unlike Cloudflare Flux (which
  rejects a seed, 07 §1a), stable-diffusion.cpp honours it, so local images are reproducible:
  the same prompt + seed gives the same picture.
- Output PNG → post-processing from 07 §4, except: crop 768×448 → **768×432** (16:9)
  instead of the square crop, then Lanczos upscale.
- Errors:
  - out of memory → `ImageError::Other("GPU ran out of memory. Try a smaller model profile or close other apps.")`
    and automatically retry once with VAE tiling + smaller size (640×384).
  - engine missing/crashes → readable error + "switch to Cloudflare" hint.
- Never returns `QuotaExhaustedDaily` (no quota locally).

## 7. Prompt style for SD 1.5 (`prompt_style: "sd15"`)

SD 1.5's text encoder only reads about **77 tokens**; anything after that is weak or
ignored. The prompt builder in 07 §3 gets a per-style adapter:

- Order: **style → subject (with bible descriptions, shortened) → shot → place →
  lighting**. Most important words first.
- Each style preset gets a **short style** (≤ 12 words) for `sd15`, e.g. Stickman:
  "stick figure doodle, black marker lines, white background, flat colour accents". The full
  style text (~40 tokens for Stickman) would use half the budget.
- Short comma-separated phrases, not long sentences.
- Bible entries get a `short` variant (≤ 12 words) generated with the Visual Bible
  for `sd15` use; fall back to truncation.
- SD 1.5 supports a negative prompt, but with LCM's low CFG its effect is small. Use a
  short fixed one: "text, watermark, letters, blurry, deformed hands, extra fingers".
- Unit tests: token-budget estimate (approximate by words ×1.3) stays ≤ 75.

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
for free with no daily limit. Download the local model (~2 GB)?" → Install / Not now.

## 10. Tests

- VRAM detection: unit-test the adapter-selection logic with fake adapter lists.
- Manifest parsing + profile filtering by VRAM.
- Download: resume from `.part`, sha256 mismatch → `corrupt`.
- Prompt adapter `sd15`: order, token budget, negative prompt.
- Post-processing: 768×448 → 768×432 crop.
- Provider: mocked engine runner for success, OOM-then-retry, crash.
- Manual: real run on a 4 GB GPU, 10 images, note seconds/image and peak VRAM in the Changelog.

## 11. Roadmap entry (add as Phase 6 in `05-roadmap.md`)

- [ ] GPU/VRAM detection + settings cache
- [ ] Bundle stable-diffusion.cpp Vulkan sidecar, pinned version; choose persistent mode (§3)
- [ ] `local_models.json` + Local models settings page (download, verify, delete)
- [ ] `LocalSdCpp` provider + OOM fallback
- [ ] `sd15` prompt adapter + short bible entries
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
  (`LocalSdCpp`), `images/prompt_sd15.rs`, `commands/local.rs`.
- Commands: `local_status`, `local_install(profile_id)` (background, `model-download-progress
  {profile_id, stage, done_mb, total_mb, error}`), `local_install_cancel`, `local_verify`,
  `local_delete`, `local_offer_dismiss`, `local_benchmark`.
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

## 13. Next version: approved profile (not implemented yet)

Owner-approved on 2026-10-15, to replace the SD 1.5 plan (§4, §7 no longer apply):

- **`zimage_turbo`** (default, min VRAM 4 GB): diffusion `leejet/Z-Image-Turbo-GGUF`
  (`z_image_turbo-Q4_K.gguf`, 3,864,250,304 bytes, sha256 `14b375ab…086fbd`, for ≥ 6 GB —
  DXGI reports ~5,900 MB on 6 GB cards, so use ≥ 5,500 MB; `z_image_turbo-Q3_K.gguf`,
  3,143,559,104 bytes, sha256 `4b44bdaa…232980`, for 4–5 GB); LLM text encoder (`--llm`)
  `unsloth/Qwen3-4B-Instruct-2507-GGUF` `Qwen3-4B-Instruct-2507-Q4_K_M.gguf` (2,497,281,120
  bytes, sha256 `3605803b…c67e597`); VAE `ae.safetensors` (335,304,388 bytes, sha256
  `afc8e282…529e38`). All Apache-2.0.
- Gen: 8 steps, `--cfg-scale 1.0`, `--diffusion-fa`, `--offload-to-cpu`, 1024×576 (16:9 and
  multiples of 64 → no crop). Sampler: engine default (Z-Image docs set none). All flags
  confirmed in the pinned `sd-cli` / `sd-server --help`; `docs/z_image.md` read.
- Prompts: full 07 §3 prompt (Qwen text encoder, no 77-token limit), keep "clean image
  without any lettering" (Z-Image renders text readily).
- **Open question for the owner:** the official VAE (`black-forest-labs/FLUX.1-schnell`) is
  gated — anonymous download returns HTTP 401. Proposed source: the byte-identical copy in
  `Comfy-Org/Lumina_Image_2.0_Repackaged/split_files/vae/ae.safetensors` (same size and
  sha256, Apache-2.0, not gated); licence URL stays the Black Forest Labs page.
- Code needed: `llm` file role, per-file `min_vram_mb` / `max_vram_mb` variants, optional
  sampler, remove the SD 1.5 adapter; show the final JSON to the owner before downloading.
- `flux_schnell_gguf` (optional, min 6 GB): after Z-Image works.

