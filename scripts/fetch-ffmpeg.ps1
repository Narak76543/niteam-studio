# Downloads the pinned FFmpeg build and places it as a Tauri sidecar
# (src-tauri/binaries/ffmpeg-<target-triple>.exe). Run once after cloning:
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-ffmpeg.ps1
# The binary is not committed (about 100 MB). Pinned in docs/03-pipeline.md ("FFmpeg sidecar").

$ErrorActionPreference = "Stop"

$Version = "9.0.2"
$Url = "https://github.com/GyanD/codexffmpeg/releases/download/$Version/ffmpeg-$Version-essentials_build.zip"
$Sha256 = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba"

$Root = Split-Path -Parent $PSScriptRoot
$BinDir = Join-Path $Root "src-tauri\binaries"
$Triple = ((rustc -vV) | Select-String "^host: ").ToString().Substring(6).Trim()
$Target = Join-Path $BinDir "ffmpeg-$Triple.exe"

if (Test-Path $Target) {
    Write-Host "Already present: $Target"
    exit 0
}

$Work = Join-Path ([System.IO.Path]::GetTempPath()) "nitean-ffmpeg-$Version"
New-Item -ItemType Directory -Force $Work | Out-Null
$Zip = Join-Path $Work "ffmpeg.zip"

Write-Host "Downloading FFmpeg $Version ..."
Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing

$Actual = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
if ($Actual -ne $Sha256) {
    throw "sha256 mismatch: expected $Sha256, got $Actual"
}

Expand-Archive -Path $Zip -DestinationPath $Work -Force
$Src = Join-Path $Work "ffmpeg-$Version-essentials_build"

New-Item -ItemType Directory -Force $BinDir | Out-Null
Copy-Item (Join-Path $Src "bin\ffmpeg.exe") $Target
Copy-Item (Join-Path $Src "LICENSE") (Join-Path $BinDir "FFMPEG-LICENSE.txt")
Copy-Item (Join-Path $Src "README.txt") (Join-Path $BinDir "FFMPEG-README.txt")
Remove-Item -Recurse -Force $Work

Write-Host "FFmpeg $Version -> $Target"
