# Third-party notices

Nitean Studio bundles or downloads the components below. Each one stays under its own
license. The full license texts ship with the binaries (see "License file") or are at the
linked pages.

## Bundled in the installer

| Component | Version | License | Source / license file |
|---|---|---|---|
| **FFmpeg** (Gyan.dev "essentials" Windows build) | 9.0.2 | **GPL-3.0** (build uses GPL components) | <https://ffmpeg.org>, build: <https://github.com/GyanD/codexffmpeg/releases/tag/9.0.2>. License file: `FFMPEG-LICENSE.txt` next to the binary |
| **stable-diffusion.cpp** (`sd-server.exe`, Vulkan build) | master-929-3f8527a | MIT | <https://github.com/leejet/stable-diffusion.cpp>. License: `sdcpp/stable-diffusion.cpp.txt` |
| **ggml** (inside stable-diffusion.cpp) | — | MIT | <https://github.com/ggml-org/ggml>. License: `sdcpp/ggml.txt` |
| **libwebp** (`libwebp.dll`, `libwebpmux.dll`, `libsharpyuv.dll`, inside stable-diffusion.cpp) | — | BSD-3-Clause | <https://chromium.googlesource.com/webm/libwebp> |
| **audio.cpp** (`audiocpp_server.exe`, Vulkan build) | v0.9.0 | Apache-2.0 | <https://github.com/0xShug0/audio.cpp>. License: `audiocpp/LICENSE` |
| **Microsoft Visual C++ runtime** DLLs (inside audio.cpp) | — | Microsoft Visual C++ Redistributable license | <https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist> |
| **Kantumruy Pro** font | — | SIL OFL 1.1 | <https://github.com/sovichet/kantumruy-pro>. `src/assets/fonts/OFL-KantumruyPro.txt` |
| **Noto Sans Khmer** font | — | SIL OFL 1.1 | <https://github.com/notofonts/khmer>. `src/assets/fonts/OFL-NotoSansKhmer.txt` |
| **Manrope** font | — | SIL OFL 1.1 | <https://github.com/sharanda/manrope>. `src/assets/fonts/OFL-Manrope.txt` |

FFmpeg is a separate program that Nitean Studio starts as a child process. It is not linked
into the app. The corresponding FFmpeg source code is available from the FFmpeg project and
from the build page above.

## Downloaded inside the app (not in the installer)

Model files are downloaded only when the user turns on the feature, pinned to a fixed
revision and checked with SHA-256 (`src-tauri/resources/local_models.json`,
`src-tauri/resources/local_tts_models.json`).

| Model | File | License | Model page |
|---|---|---|---|
| **Z-Image Turbo** (GGUF by leejet) | `z_image_turbo-Q4_K.gguf` / `-Q3_K.gguf` | Apache-2.0 | <https://huggingface.co/Tongyi-MAI/Z-Image-Turbo> |
| **Qwen3-4B-Instruct-2507** (text encoder, GGUF by unsloth) | `Qwen3-4B-Instruct-2507-Q4_K_M.gguf` | Apache-2.0 | <https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507> |
| **FLUX.1 autoencoder** (repackaged by Comfy-Org) | `ae.safetensors` | Apache-2.0 | <https://huggingface.co/black-forest-labs/FLUX.1-schnell> |
| **VoxCPM2** (Q8_0 GGUF by audio.cpp) | `voxcpm2-q8_0.gguf` | Apache-2.0 | <https://huggingface.co/openbmb/VoxCPM2> |

## Libraries compiled into the app

Main Rust crates (all MIT and/or Apache-2.0): Tauri and its plugins, tokio, reqwest, serde,
keyring, image, **lopdf** (MIT, PDF reading), msedge-tts, thiserror, chrono, sha2,
unicode-segmentation, unicode-normalization, windows-rs. Full list: `src-tauri/Cargo.lock`
(`cargo tree`).

Main JavaScript packages: React and React DOM (MIT), lucide-react icons (ISC),
@tauri-apps/api and plugins (MIT/Apache-2.0), **Mermaid** 11.17.2 (MIT, diagrams in
real-world videos), **Shiki** 4.5.0 with `@shikijs/langs` / `@shikijs/themes` (MIT, code
colours; theme GitHub Dark, MIT), **JetBrains Mono** via `@fontsource-variable/jetbrains-mono`
5.3.0 (SIL OFL 1.1, code scenes). Full list: `package-lock.json`.

## Online services (not bundled)

The app calls these services with the user's own account: Google Gemini API,
Cloudflare Workers AI (`@cf/black-forest-labs/flux-1-schnell`), and Microsoft Edge
"Read aloud" (Edge TTS, unofficial). Their terms are set by each provider.

"Real-world Tech Explainer" videos search and download stock photos and clips from
**Wikimedia Commons**, **Openverse**, **Pixabay** and (optional) **Pexels**. Each file keeps
its own licence (CC0, public domain, CC BY, CC BY-SA if allowed, Pexels License, Pixabay
Content License), recorded per project in `assets/assets.json` and listed in the video's
Credits block. Photos provided by Pexels: <https://www.pexels.com>.
