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

Whatever mode: run at **below-normal priority**, one generation at a time, cancellable
(kill the child / drop the context), and never run at the same time as an FFmpeg render.

## 4. Model profiles

Defined in `src-tauri/resources/local_models.json` (the app reads it; files are
downloaded on demand, never bundled in the installer):

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
- SD 1.5 profile: an SD 1.5 base or fine-tune good at illustration/painterly styles,
  fp16 `.safetensors`, plus the **LCM-LoRA for SD 1.5** (4–8 steps, low CFG, `lcm` sampler).
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
  from the profile, VAE tiling on for profiles that need it.
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
