# Nitean Studio

Nitean Studio (និទាន = "tale") is a Windows desktop app that turns a topic, or a PDF
document, into a narrated explainer video of up to 20 minutes, in **Khmer or English**:
AI narration over AI-generated illustrations with slow pan motion, burned-in subtitles
and background music.

- **Script** written by Gemini with your own API key (bring your own key).
- **Voices**: Edge TTS (unofficial), Gemini TTS, or local VoxCPM2 voice profiles on a capable GPU.
- **Images**: Cloudflare Workers AI (Flux, free daily allowance), Gemini images (paid), or
  local Z-Image Turbo on a GPU with 4 GB+ VRAM.
- **Subtitles** rendered correctly for Khmer, with a CapCut-style editor.
- **Video from a document**: read a PDF, review the summary, then generate.

## Install

Download `Nitean Studio_0.1.0_x64-setup.exe` from the
[Releases](https://github.com/Narak76543/niteam-studio/releases) page and run it.
It installs for the current user, so administrator rights are not needed.

**Windows SmartScreen warning:** the installer is not code-signed yet, so Windows may show
"Windows protected your PC". Click **More info → Run anyway**.

The installer bundles FFmpeg and the local image/voice engines. AI model files are **not**
in the installer. The app downloads them when you turn on Local GPU images or local voices
in Settings.

You need your own **Gemini API key** (Settings → Gemini). For the default image provider you
also need a free Cloudflare account ID and API token. Keys are stored in Windows Credential
Manager and are never written to project files.

## Build from source

Requirements: Windows 10/11, [Node.js](https://nodejs.org/) 20+, Rust stable
(the `x86_64-pc-windows-gnu` toolchain is what we use), and the
[Tauri v2 prerequisites](https://tauri.app/start/prerequisites/) (WebView2).

```powershell
git clone https://github.com/Narak76543/niteam-studio.git
cd niteam-studio
npm install

# Once after cloning: download the pinned third-party binaries (checksums verified).
# They are not in git.
powershell -ExecutionPolicy Bypass -File scripts/fetch-ffmpeg.ps1    # FFmpeg sidecar  (~100 MB)
powershell -ExecutionPolicy Bypass -File scripts/fetch-sdcpp.ps1     # local image engine (~90 MB)
powershell -ExecutionPolicy Bypass -File scripts/fetch-audiocpp.ps1  # local voice engine (~170 MB)

npm run tauri dev      # run in development
npm run tauri build    # build the NSIS installer (src-tauri/target/release/bundle/nsis/)
```

Checks:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
npm run lint; npm run typecheck; npm test
```

Design and architecture docs are in [`docs/`](docs/). Start with `docs/01-product.md`.

## Licenses

Third-party components that are bundled or downloaded are listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
