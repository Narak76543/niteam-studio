# Nitean Studio — Agent Guide

Nitean Studio (និទាន = "tale") is a Windows desktop app that generates long-form
(up to 20 min) narrated explainer videos in **Khmer or English**: AI narration over
AI-generated illustrations with slow pan motion, burned-in subtitles and background music.

**Read these before writing code, in this order:**

1. `docs/01-product.md` — what we build, for whom, user flow
2. `docs/02-architecture.md` — stack, folder layout, data model, IPC
3. `docs/03-pipeline.md` — each generation/render step in detail
4. `docs/04-khmer.md` — Khmer text, TTS and subtitle rules (easy to get wrong)
5. `docs/05-roadmap.md` — phases, acceptance criteria, **current phase**
6. `docs/07-image-generation.md` — image providers (Cloudflare Flux default, Gemini paid),
   Visual Bible, prompt building, post-processing, quota pause, caching
7. `docs/08-local-gpu.md` — local GPU images (`local_sdcpp`, stable-diffusion.cpp sidecar,
   VRAM tiers, model profiles + downloads, SD 1.5 prompt adapter) — **Phase 6 only**

Only work on the phase marked **CURRENT** in `docs/05-roadmap.md`. When it is done,
stop and ask the human to test. Do not start the next phase on your own.

## Hard rules

- **Target hardware is a low-spec Windows laptop** (8 GB RAM, integrated GPU, no CUDA).
  No Remotion, no Node at runtime, no headless Chrome, no local AI models (except
  Exception 3 below).
- **All AI runs through the Gemini API** with the user's own key (BYOK).
  **Exception (decided by the project owner, 2026-10-15):** voices may also come from
  **Edge TTS** (Microsoft Edge "Read aloud", via the `msedge-tts` crate). It is an unofficial,
  undocumented service that can break or be blocked at any time and its terms are unclear:
  keep it behind the `TtsEngine` trait, label it unofficial in the UI, show a clear
  "Edge TTS unavailable — switch to Gemini TTS in Settings" error, and keep Gemini TTS fully
  working as the alternative.
  **Exception 2 (images, `docs/07-image-generation.md`):** images default to **Cloudflare
  Workers AI** (`@cf/black-forest-labs/flux-1-schnell`) with the user's own Cloudflare account
  (free daily allowance); Gemini image models remain the paid alternative.
  **Exception 3 (local images, `docs/08-local-gpu.md`, Phase 6 only; decided by the project
  owner, 2026-10-15):** images may be generated locally with **stable-diffusion.cpp** (MIT,
  bundled as a sidecar) and on-demand downloaded model files whose licenses allow commercial
  use (owner-approved list in `local_models.json`), only on GPUs with ≥ 4 GB dedicated VRAM.
  Integrated-GPU / < 4 GB machines keep using Cloudflare. Text and voice stay as above.
  No other non-Gemini AI services and no other local models.
  - Store the Gemini key **and the Cloudflare API token** only via the `keyring` crate
    (Windows Credential Manager; users `gemini_api_key`, `cloudflare_api_token`). The
    Cloudflare **account ID** is not secret and lives in `settings.json`.
  - Never log the key or token, never write them to project files.
  - **Never rotate multiple keys/accounts/tokens to bypass rate limits or daily quotas**
    (violates Google's and Cloudflare's terms). Handle 429 with backoff + a concurrency limit;
    when the Cloudflare daily allowance is used up, **pause** (`paused_quota`) and resume after
    the reset — never switch to another account or token.
- **Check the current Gemini API docs before writing any Gemini request code**, and the
  current Cloudflare Workers AI docs before changing the Cloudflare client.
  Do not guess request/response field names. Keep model IDs in config, not hard-coded.
- **Khmer subtitles are rendered as PNGs in the WebView canvas**, never with
  FFmpeg `drawtext`/libass. See `docs/04-khmer.md`.
- Every pipeline step must be **resumable**: persist state after each scene step.
- Keep the UI responsive: long work runs in Rust async tasks, progress goes to the UI
  via Tauri events.

## Stack

Tauri v2 · Rust (tokio, reqwest, serde, keyring, thiserror/anyhow) ·
React + TypeScript + Vite · FFmpeg as a Tauri sidecar.

## Commands

```bash
npm install
powershell -ExecutionPolicy Bypass -File scripts/fetch-ffmpeg.ps1   # once: FFmpeg sidecar
npm run tauri dev      # run app in dev
npm run tauri build    # Windows installer
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
npm run lint && npm run typecheck
```

## Conventions

- Rust: `thiserror` for module error types, `anyhow` only at command boundaries.
  Tauri commands return `Result<T, String>` with user-readable messages.
- No `unwrap()`/`expect()` outside tests.
- TypeScript: strict mode, no `any`. All `invoke()` calls go through `src/lib/api.ts`
  with typed wrappers mirroring Rust command signatures.
- UI strings go through the i18n files (`src/i18n/km.json`, `src/i18n/en.json`).
- Small focused modules; one responsibility per file.
- When you change the data model or a pipeline step, **update the matching doc** in `docs/`.

## Definition of done (every phase)

- Builds on Windows with `npm run tauri dev`, no warnings from clippy/tsc.
- Acceptance criteria in `docs/05-roadmap.md` met and ticked.
- Short note added to the "Changelog" section at the bottom of `docs/05-roadmap.md`.
