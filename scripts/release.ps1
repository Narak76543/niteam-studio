# Builds a signed release and the latest.json the in-app updater reads (docs/12-updates.md).
#   powershell -ExecutionPolicy Bypass -File scripts/release.ps1 -Notes "What changed in this version"
#   (or -NotesFile notes.md)
# Bump the version first in package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json.
# Output: release/v<version>/ with the installer and latest.json - upload BOTH to the GitHub
# release tagged v<version> (not a draft, not a pre-release).

param(
    [string]$Notes = "",
    [string]$NotesFile = "",
    [string]$KeyPath = (Join-Path $env:USERPROFILE ".tauri\nitean-studio.key"),
    [switch]$SkipBuild,
    [switch]$NoHints   # set by scripts/ship.ps1, which uploads the files itself
)

$ErrorActionPreference = "Stop"

$Repo = "Narak76543/niteam-studio"
$Root = Split-Path -Parent $PSScriptRoot
$Conf = Get-Content (Join-Path $Root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$Version = $Conf.version

if ($NotesFile) { $Notes = Get-Content $NotesFile -Raw -Encoding UTF8 }

if (-not $SkipBuild) {
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
        if (-not (Test-Path $KeyPath)) { throw "Signing key not found: $KeyPath (see docs/12-updates.md)" }
        $env:TAURI_SIGNING_PRIVATE_KEY = $KeyPath
    }
    if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "" }
    Push-Location $Root
    # The build prints progress on stderr; Windows PowerShell 5.1 would treat that as an error under "Stop".
    $ErrorActionPreference = "Continue"
    try {
        npm run tauri build
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
    } finally {
        $ErrorActionPreference = "Stop"
        Pop-Location
    }
}

$NsisDir = Join-Path $Root "src-tauri\target\release\bundle\nsis"
$Exe = Get-ChildItem $NsisDir -Filter "*_${Version}_x64-setup.exe" | Select-Object -First 1
if (-not $Exe) { throw "Installer for $Version not found in $NsisDir" }
$Sig = "$($Exe.FullName).sig"
if (-not (Test-Path $Sig)) { throw "Signature missing: $Sig (was TAURI_SIGNING_PRIVATE_KEY set?)" }

# GitHub turns spaces in asset names into dots; use a stable name without spaces.
$AssetName = "Nitean-Studio_${Version}_x64-setup.exe"
$Out = Join-Path $Root "release\v$Version"
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item $Exe.FullName (Join-Path $Out $AssetName) -Force

$Manifest = [ordered]@{
    version   = $Version
    notes     = $Notes.Trim()
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = (Get-Content $Sig -Raw).Trim()
            url       = "https://github.com/$Repo/releases/download/v$Version/$AssetName"
        }
    }
}
$Json = $Manifest | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $Out "latest.json"), $Json, (New-Object System.Text.UTF8Encoding $false))

Write-Host ""
Write-Host "Release files in $Out"
Write-Host "  $AssetName"
Write-Host "  latest.json"
if ($NoHints) { exit 0 }
Write-Host ""
Write-Host "Next: create GitHub release 'v$Version' at https://github.com/$Repo/releases/new"
Write-Host "      and upload both files. Installed apps see the update on their next start."
