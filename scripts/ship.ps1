# One command to release a new version (docs/RELEASING.md):
#   npm run release                  # 0.4.0 -> 0.4.1   (bug fixes)
#   npm run release -- minor         # 0.4.1 -> 0.5.0   (new features)
#   npm run release -- major         # 0.5.0 -> 1.0.0
#   npm run release -- 0.4.0         # exact version
#   npm run release -- minor -NotesFile notes.md   # your own "What's new" text
# Checks -> bump version -> signed build -> commit + tag + push -> GitHub release with the
# installer + latest.json -> confirms the updater link serves the new version.
# If the upload step fails after the push:  npm run release -- 0.4.0 -UploadOnly

param(
    [Parameter(Position = 0)][string]$Next = "patch",
    [string]$NotesFile = "",
    [switch]$SkipChecks,
    [switch]$UploadOnly
)

$ErrorActionPreference = "Stop"
$Repo = "Narak76543/niteam-studio"
$Root = Split-Path -Parent $PSScriptRoot
$KeyPath = Join-Path $env:USERPROFILE ".tauri\nitean-studio.key"
Set-Location $Root

function Step([string]$Text) { Write-Host ""; Write-Host "==> $Text" -ForegroundColor Cyan }
function Fail([string]$Text) { Write-Host ""; Write-Host "STOPPED: $Text" -ForegroundColor Red; exit 1 }

# Runs a native command; stderr progress is normal for npm/cargo/git, so judge by exit code only.
function Run([string]$What, [scriptblock]$Cmd) {
    $ErrorActionPreference = "Continue"
    & $Cmd
    $code = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($code -ne 0) { Fail "$What failed (exit code $code)." }
}
function Quiet([scriptblock]$Cmd) {
    $ErrorActionPreference = "Continue"
    $out = & $Cmd 2>&1 | ForEach-Object { "$_" }
    $script:LastCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    return $out
}

# ---------- work out the version ----------
$Current = (Get-Content "src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).version
$c = [version]$Current
switch ($Next) {
    "patch" { $Version = "$($c.Major).$($c.Minor).$($c.Build + 1)" }
    "minor" { $Version = "$($c.Major).$($c.Minor + 1).0" }
    "major" { $Version = "$($c.Major + 1).0.0" }
    default {
        if ($Next -notmatch '^\d+\.\d+\.\d+$') { Fail "Use patch, minor, major or a version like 0.4.0 (got '$Next')." }
        $Version = $Next
    }
}
if ($UploadOnly) { $Version = if ($Next -match '^\d+\.\d+\.\d+$') { $Next } else { $Current } }
$Tag = "v$Version"
$Out = Join-Path $Root "release\$Tag"

Write-Host "Nitean Studio release $Tag" -ForegroundColor Green
if (-not $UploadOnly) { Write-Host "(current version $Current)" }

# ---------- preflight ----------
Step "Checking everything is ready"
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Fail "GitHub CLI not installed. Run once:  winget install --id GitHub.cli   then open a NEW terminal and run:  gh auth login"
}
Quiet { gh auth status } | Out-Null
if ($LastCode -ne 0) { Fail "Not logged in to GitHub. Run once:  gh auth login   (GitHub.com -> HTTPS -> Login with a web browser)" }

if (-not $UploadOnly) {
    if ([version]$Version -le $c) { Fail "New version $Version must be higher than the current $Current." }
    if (-not (Test-Path $KeyPath)) { Fail "Signing key not found at $KeyPath. Copy it back from your backup (docs/12-updates.md)." }
    if (Get-Process nitean-studio -ErrorAction SilentlyContinue) {
        Fail "Nitean Studio is running (or 'npm run tauri dev'). Close it first - the build needs its files."
    }
    $dirty = Quiet { git status --porcelain }
    if ($dirty) { Fail "You have changes that are not committed. Commit and push them first:`n  git add -A`n  git commit -m `"...`"`n  git push" }
    $branch = (Quiet { git rev-parse --abbrev-ref HEAD } | Select-Object -First 1)
    if ($branch -ne "main") { Fail "Switch to the main branch first (you are on '$branch')." }
    Quiet { git fetch origin main --tags } | Out-Null
    $behind = (Quiet { git rev-list --count HEAD..origin/main } | Select-Object -First 1)
    $ahead = (Quiet { git rev-list --count origin/main..HEAD } | Select-Object -First 1)
    if ([int]$behind -gt 0) { Fail "GitHub has newer commits. Run:  git pull" }
    if ([int]$ahead -gt 0) { Fail "Your commits are not on GitHub yet. Run:  git push" }
    Quiet { git rev-parse -q --verify "refs/tags/$Tag" } | Out-Null
    if ($LastCode -eq 0) { Fail "Tag $Tag already exists. Choose a higher version." }
    foreach ($f in @("src-tauri\resources\sdcpp", "src-tauri\resources\audiocpp")) {
        if (-not (Test-Path $f)) { Fail "Missing $f. Run the fetch-*.ps1 scripts (docs/RELEASING.md, 'Before your first release')." }
    }
    if (-not (Get-ChildItem "src-tauri\binaries" -Filter "ffmpeg-*.exe" -ErrorAction SilentlyContinue)) {
        Fail "Missing FFmpeg. Run: powershell -ExecutionPolicy Bypass -File scripts/fetch-ffmpeg.ps1"
    }
}
Write-Host "OK"

# ---------- release notes ----------
if ($NotesFile) {
    if (-not (Test-Path $NotesFile)) { Fail "Notes file not found: $NotesFile" }
    $Notes = (Get-Content $NotesFile -Raw -Encoding UTF8).Trim()
} else {
    $last = (Quiet { git describe --tags --abbrev=0 } | Select-Object -First 1)
    $range = if ($LastCode -eq 0 -and $last) { "$last..HEAD" } else { "HEAD" }
    $lines = Quiet { git log $range --no-merges --pretty=format:"- %s" } |
        Where-Object { $_ -and $_ -notmatch '^- Release v' }
    $Notes = if ($lines) { "What's new`n" + ($lines -join "`n") } else { "Improvements and fixes." }
}

if (-not $UploadOnly) {
    # ---------- checks ----------
    if (-not $SkipChecks) {
        Step "Running checks (lint, typecheck, tests)"
        Run "Lint" { npm run lint }
        Run "Typecheck" { npm run typecheck }
        Run "Frontend tests" { npm test }
        Run "Rust tests" { cargo test --manifest-path src-tauri/Cargo.toml }
    }

    # ---------- bump + build ----------
    Step "Setting version $Version"
    Run "Version bump" { powershell -ExecutionPolicy Bypass -File scripts/bump-version.ps1 -Version $Version }

    Step "Building the signed installer (5-15 minutes)"
    $notesTmp = Join-Path ([System.IO.Path]::GetTempPath()) "nitean-notes-$Version.md"
    [System.IO.File]::WriteAllText($notesTmp, $Notes, (New-Object System.Text.UTF8Encoding $false))
    $ErrorActionPreference = "Continue"
    & powershell -ExecutionPolicy Bypass -File scripts/release.ps1 -NotesFile $notesTmp -NoHints
    $buildCode = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($buildCode -ne 0) {
        Quiet { git checkout -- package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json } | Out-Null
        Fail "The build failed, so nothing was published. The version change was undone; fix the error above and run the command again."
    }

    # ---------- commit, tag, push ----------
    Step "Saving version $Version to GitHub"
    Run "git add" { git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json }
    Run "git commit" { git commit -m "Release $Tag" }
    Run "git tag" { git tag -a $Tag -m "Nitean Studio $Version" }
    Run "git push" { git push origin main }
    Run "git push tag" { git push origin $Tag }
}

# ---------- GitHub release ----------
Step "Publishing the GitHub release"
$Exe = Join-Path $Out "Nitean-Studio_${Version}_x64-setup.exe"
$Json = Join-Path $Out "latest.json"
if (-not (Test-Path $Exe) -or -not (Test-Path $Json)) { Fail "Release files missing in $Out. Run without -UploadOnly." }
$notesUp = Join-Path ([System.IO.Path]::GetTempPath()) "nitean-notes-up-$Version.md"
$jsonNotes = (Get-Content $Json -Raw -Encoding UTF8 | ConvertFrom-Json).notes
[System.IO.File]::WriteAllText($notesUp, $(if ($jsonNotes) { $jsonNotes } else { $Notes }), (New-Object System.Text.UTF8Encoding $false))

Quiet { gh release view $Tag --repo $Repo } | Out-Null
if ($LastCode -eq 0) {
    Run "Upload files" { gh release upload $Tag $Exe $Json --repo $Repo --clobber }
    Run "Publish" { gh release edit $Tag --repo $Repo --draft=false --prerelease=false --latest }
} else {
    Run "Create release" {
        gh release create $Tag $Exe $Json --repo $Repo --verify-tag --latest `
            --title "Nitean Studio $Version" --notes-file $notesUp
    }
}

# ---------- confirm ----------
Step "Checking the updater link"
$url = "https://github.com/$Repo/releases/latest/download/latest.json"
$seen = $null
for ($i = 0; $i -lt 6; $i++) {
    try {
        $seen = (Invoke-RestMethod -Uri $url -Headers @{ "Cache-Control" = "no-cache" } -TimeoutSec 20).version
        if ($seen -eq $Version) { break }
    } catch { $seen = $null }
    Start-Sleep -Seconds 5
}
Write-Host ""
if ($seen -eq $Version) {
    Write-Host "DONE: Nitean Studio $Version is live." -ForegroundColor Green
    Write-Host "  Release page: https://github.com/$Repo/releases/tag/$Tag"
    Write-Host "  Installed apps (0.4.0+) show 'Update available' on their next start."
} else {
    Write-Host "Published, but $url still shows '$seen'." -ForegroundColor Yellow
    Write-Host "GitHub can take a minute to update. Open the link in a browser shortly; it should show $Version."
}
