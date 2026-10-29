# 12 — In-app updates

Owner decision 2026-10-29: Nitean Studio checks for, downloads and installs new versions
itself, using the official **Tauri updater** (`tauri-plugin-updater`, MIT/Apache-2.0) with
GitHub Releases as the server. Not an AI service; no account or key on the user's side.

## 1. How it works

```
app start ──4 s──▶ check()  GET https://github.com/Narak76543/niteam-studio/releases/latest/download/latest.json
                     │       (redirects to the newest non-draft, non-pre-release)
                     ├─ newer version  ─▶ sidebar card "Update available · Version x" ─▶ dialog
                     ├─ same / older   ─▶ nothing
                     └─ error/offline  ─▶ nothing (start-up check is silent)

dialog: version, current version, release notes (latest.json "notes"), running-jobs warning
  [Later]  [Install and restart] ─▶ download (progress %) ─▶ signature check ─▶ NSIS installer
                                    (passive) ─▶ app exits, installer updates, app reopens
```

- **Settings → About**: version, **Check for updates** (spinner while checking), then
  "You're on the latest version", "Couldn't check…" or the update notice with **View update**.
- **Sidebar**: card above the language footer while an update is known; × hides it until the
  next start. Compact sidebar (editor, < 1280 px): download icon with a coral dot.
- Projects are untouched by the installer (they live in `%APPDATA%\NiteanStudio`); pipeline steps
  are resumable, so a job stopped by the restart continues later. The dialog says so.
- Code: `src/lib/updater.ts` (state hook, start-up check), `src/lib/api.ts` (`updateCheck`,
  `updateInstall`), `src/components/UpdateDialog.tsx`, Sidebar `UpdateNotice`, Settings
  `UpdateStatus`. Rust: plugin registered in `src-tauri/src/lib.rs`; permissions
  `updater:default`, `process:allow-restart`.

## 2. Configuration (`src-tauri/tauri.conf.json`)

- `bundle.createUpdaterArtifacts: true` → the build writes `…_x64-setup.exe.sig` next to the installer.
- `plugins.updater.pubkey` — public half of the signing key (content, not a path).
- `plugins.updater.endpoints` — the GitHub `latest/download/latest.json` URL above.
- `plugins.updater.windows.installMode: "passive"` — small progress window, no questions.

## 3. Signing key (owner only)

- Private key: `%USERPROFILE%\.tauri\nitean-studio.key` (generated 2026-10-29, **no password**);
  public key: same path + `.pub`. **Never commit the private key.**
- **Back it up** (password manager / offline drive). If it is lost, installed copies can never
  verify another update — users would have to reinstall manually.
- Until the first release that contains the updater (0.4.0) is published, the key can still be
  replaced freely (e.g. with a password-protected one: `npm run tauri signer generate -- -w
  %USERPROFILE%\.tauri\nitean-studio.key -f`, then paste the new `.pub` into `pubkey` and set
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` when building). After that, never change it.

## 4. Releasing a new version

Step-by-step guide for the owner: `docs/RELEASING.md`. Normally one command does all of the
steps below: `npm run release [-- patch|minor|major|X.Y.Z] [-NotesFile notes.md]`
(`scripts/ship.ps1`; needs the GitHub CLI logged in once with `gh auth login`).

1. `scripts/bump-version.ps1 -Version X.Y.Z` (package.json + lock, Cargo.toml + lock,
   tauri.conf.json; SemVer, must be higher than the installed one).
2. `powershell -ExecutionPolicy Bypass -File scripts/release.ps1 -Notes "What changed"`
   (or `-NotesFile notes.md`). Builds with the signing key and writes `release/v<version>/`:
   `Nitean-Studio_<version>_x64-setup.exe` + `latest.json` (`version`, `notes`, `pub_date`,
   `platforms.windows-x86_64.{signature,url}`).
3. On GitHub create release tag **`v<version>`** (not draft, not pre-release) and upload
   **both** files. The `url` in `latest.json` points at that exact asset name.
4. Check: `curl -L https://github.com/Narak76543/niteam-studio/releases/latest/download/latest.json`
   shows the new version; an older installed copy shows "Update available" on its next start.

Versions before 0.4.0 have no updater: users on 0.1.0–0.3.x install 0.4.0 by hand once.
