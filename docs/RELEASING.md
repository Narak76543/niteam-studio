# How to release a new version of Nitean Studio

**What a "release" is:** you build a new installer on your computer and upload it to GitHub.
Every installed copy of Nitean Studio (0.4.0 or newer) asks GitHub "is there something newer?"
when it starts. If there is, it shows **Update available** and installs it for the user.
Technical background: `docs/12-updates.md`.

## The quick way: one command

After the one-time setup below, a release is:

```powershell
git add -A
git commit -m "what you changed"
git push

npm run release              # bug-fix release:  0.4.0 -> 0.4.1
```

| Command | Use it for | Example |
|---|---|---|
| `npm run release` | bug fixes | 0.4.0 → 0.4.1 |
| `npm run release -- minor` | new features | 0.4.1 → 0.5.0 |
| `npm run release -- major` | huge change | 0.5.0 → 1.0.0 |
| `npm run release -- 0.4.0` | an exact number | → 0.4.0 |

Add `-NotesFile notes.md` to any of them to use your own "What's new" text (see Step 3 below
for how to write it). Without it, the notes are your commit messages since the last release.

It takes 10–20 minutes and does everything in order:

1. Checks you're ready: GitHub login, signing key, app closed, everything committed and pushed.
   If something is missing it **stops and tells you the exact command to fix it**.
2. Runs lint, typecheck and all tests (`-SkipChecks` to skip; not recommended).
3. Sets the new version number.
4. Builds and signs the installer. **If the build fails, nothing is published** and the
   version change is undone.
5. Commits "Release vX.Y.Z", creates the tag, pushes both.
6. Creates the GitHub release with the installer + `latest.json`.
7. Confirms the update link serves the new version and prints **DONE**.

If it stops after the push (for example, the internet dropped during the upload), finish with:

```powershell
npm run release -- 0.4.0 -UploadOnly
```

(use the version it was releasing). That only uploads; it doesn't build or bump again.

**First release (0.4.0):** write `notes.md` and run `npm run release -- minor -NotesFile notes.md`.
Then tell your users to install 0.4.0 by hand once (see "The very first release with updates").

The step-by-step sections further down are what the command does for you. You only need
them if you want to release by hand.

---

## Before your first release (one time only)

0. **Install the GitHub CLI and log in** (lets the release command upload for you):
   ```powershell
   winget install --id GitHub.cli
   ```
   Close the terminal, open a new one, then:
   ```powershell
   gh auth login
   ```
   Choose **GitHub.com → HTTPS → Yes → Login with a web browser**, copy the code it shows,
   and approve in the browser. Check with `gh auth status` (it says "Logged in to github.com").

1. **Back up your signing key.** Copy the folder `C:\Users\<you>\.tauri\` (two files:
   `nitean-studio.key` and `nitean-studio.key.pub`) to a USB drive or password manager.
   - This key proves updates really come from you. If you lose it, installed apps can never
     update again, and every user has to reinstall by hand.
   - Never upload the `.key` file anywhere and never commit it to git.
2. **Check you can open** https://github.com/Narak76543/niteam-studio/releases while signed in to
   GitHub, and that you see a **Draft a new release** button.
3. **Check the engine files are on this computer.** Run these once; each one says "Already
   present" if it's fine:
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts/fetch-ffmpeg.ps1
   powershell -ExecutionPolicy Bypass -File scripts/fetch-sdcpp.ps1
   powershell -ExecutionPolicy Bypass -File scripts/fetch-audiocpp.ps1
   ```

Run all commands below in a terminal opened in the project folder `D:\development\nitean-studio`
(in VS Code: **Terminal → New Terminal**).

---

## Releasing by hand (what `npm run release` does)

## Step 1 — Make sure the app is ready

- Close `npm run tauri dev` if it's running (press `Ctrl+C` in its terminal).
- Check that everything passes:
  ```powershell
  npm run lint
  npm run typecheck
  npm test
  cargo test --manifest-path src-tauri/Cargo.toml
  ```
  All four must end without errors. If one fails, fix it (or ask Claude) before going on.

## Step 2 — Choose the new version number

Versions have three numbers: **MAJOR.MINOR.PATCH**, for example `0.4.0`.

| Change | Example | New version |
|---|---|---|
| Only bug fixes | fixed a crash | `0.4.0` → `0.4.1` |
| New features | added in-app updates | `0.4.1` → `0.5.0` |
| Very big change / 1.0 launch | | `0.9.0` → `1.0.0` |

The new number **must be higher** than the last one, or apps won't see the update.

Set it everywhere with one command (replace `0.4.0` with your number):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/bump-version.ps1 -Version 0.4.0
```

It should print `Version 0.3.0 -> 0.4.0`.

## Step 3 — Write the release notes

Create a file `notes.md` in the project folder. Users see this text in the **Update available**
dialog, so write it for them (short, plain, Khmer and English is good):

```text
What's new
- New look: cleaner design, light and dark mode
- The app now updates itself
- Fixed: …

អ្វីដែលថ្មី
- រចនាថ្មី ស្អាតជាងមុន
- កម្មវិធីអាចធ្វើបច្ចុប្បន្នភាពដោយខ្លួនឯង
```

(`notes.md` is only for this release; you can delete it afterwards.)

## Step 4 — Save the code to GitHub

```powershell
git add -A
git commit -m "Release v0.4.0"
git push
```

Use your version number. `git status` should then say "nothing to commit, working tree clean".

## Step 5 — Build the release

```powershell
powershell -ExecutionPolicy Bypass -File scripts/release.ps1 -NotesFile notes.md
```

- This takes **5–15 minutes**. Lots of text scrolls by; that's normal.
- At the end you see:
  ```
  Release files in D:\development\nitean-studio\release\v0.4.0
    Nitean-Studio_0.4.0_x64-setup.exe
    latest.json
  ```
- Open that folder (`release\v0.4.0`). You'll upload these two files in Step 7.

## Step 6 — Test the installer yourself

1. Double-click `Nitean-Studio_0.4.0_x64-setup.exe` and install it.
   - If Windows says **"Windows protected your PC"**: click **More info → Run anyway**.
     (The installer isn't Microsoft code-signed yet; that's a separate thing from the update key.)
2. Open the app. Check **Settings → About** shows the new version, open a project, play a voice.

If something is broken, **stop here** and fix it; nothing has been published yet.

## Step 7 — Publish on GitHub

1. Go to https://github.com/Narak76543/niteam-studio/releases and click **Draft a new release**.
2. **Choose a tag** → type `v0.4.0` (a small **v** + your version) → click
   **Create new tag: v0.4.0 on publish**. Target: `main`.
3. **Release title:** `Nitean Studio 0.4.0`.
4. **Description:** paste the text of `notes.md`.
5. **Attach binaries:** drag in **both** files from `release\v0.4.0`:
   `Nitean-Studio_0.4.0_x64-setup.exe` **and** `latest.json`. Wait until both finish uploading.
   Don't rename them.
6. At the bottom:
   - **Set as a pre-release**: leave this **off**
   - **Set as the latest release**: leave this **on**
7. Click **Publish release** (not "Save draft"; a draft is invisible to the app).

## Step 8 — Check it worked

1. Open this link in your browser:
   https://github.com/Narak76543/niteam-studio/releases/latest/download/latest.json
   You should see text with `"version": "0.4.0"`. If you see "Not Found", see Troubleshooting.
2. On a computer with an **older** version (0.4.0 or later installed), open the app and go to
   **Settings → About → Check for updates**. It should say **Nitean Studio 0.4.0 is available**.

Done 🎉 — every user gets the update the next time they open the app.

---

## The very first release with updates (0.4.0)

Versions 0.1.0–0.3.x **don't have the updater**. For this one release, tell your users to
download and install 0.4.0 by hand from the GitHub release page. From then on, every later
version (0.4.1, 0.5.0, …) reaches them automatically.

The first time you can actually *see* an automatic update is the release **after** 0.4.0:
keep 0.4.0 installed, publish 0.4.1, then open the app.

---

## Troubleshooting

| Problem | Fix |
|---|---|
| `npm run release` says **GitHub CLI not installed** / **Not logged in** | Do step 0 of "Before your first release". |
| `npm run release` says **changes that are not committed** / **not on GitHub yet** | Run `git add -A`, `git commit -m "..."`, `git push`, then the release command again. |
| `npm run release` says **Nitean Studio is running** | Close the app and any `npm run tauri dev` terminal (`Ctrl+C`). |
| `npm run release` stopped after "Saving version … to GitHub" | Run `npm run release -- <that version> -UploadOnly`. |
| `bump-version` says "must be higher than the current" | Pick a bigger number than the one shown. |
| `release.ps1` says **Signing key not found** | The key isn't at `C:\Users\<you>\.tauri\nitean-studio.key`. Copy it back from your backup. |
| `release.ps1` says **Signature missing** | The build didn't use the key. Run the script again; don't run `npm run tauri build` on its own for releases. |
| Build fails with **"being used by another process"** | Close the app and `npm run tauri dev`, then run the script again. |
| Build fails on `sdcpp` / `audiocpp` / `ffmpeg` | Run the three `fetch-*.ps1` commands from "Before your first release". |
| The `latest.json` link shows **Not Found** | The release is still a draft or pre-release, or `latest.json` wasn't uploaded. Edit the release on GitHub and fix it. |
| App says **Couldn't check for updates** | No internet, or the `latest.json` link above doesn't work yet. |
| App doesn't offer the update | The installed version is older than 0.4.0 (no updater), or the new version number isn't higher. |
| Update downloads, then fails with a **signature** error | The installer was built with a different key than the one in the app. Always build with `release.ps1` and the same key. Never generate a new key after 0.4.0. |
| You uploaded a broken version | Publish a fixed one with a higher number (e.g. 0.4.2) right away. Don't delete the release; apps simply move on to the newer one. |

---

## Short checklist (by hand; or just run `npm run release`)

- [ ] Tests pass (Step 1)
- [ ] `scripts/bump-version.ps1 -Version X.Y.Z`
- [ ] `notes.md` written
- [ ] `git add -A`, `git commit -m "Release vX.Y.Z"`, `git push`
- [ ] `scripts/release.ps1 -NotesFile notes.md`
- [ ] Installed and tried it myself
- [ ] GitHub release `vX.Y.Z`: both files uploaded, not a draft, not a pre-release
- [ ] `latest.json` link shows the new version
