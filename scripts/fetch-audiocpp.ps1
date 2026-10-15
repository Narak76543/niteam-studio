# Downloads the pinned audio.cpp Vulkan build (audiocpp_server.exe + DLLs + model_specs) into
# src-tauri/resources/audiocpp/ (bundled as an app resource, docs/09-voice-profiles.md).
# Run once after cloning:
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-audiocpp.ps1
# Not committed (~170 MB). Keep $Tag in sync with resources/local_tts_models.json "engine_version".

$ErrorActionPreference = "Stop"

$Tag = "v0.9.0"
$Url = "https://github.com/0xShug0/audio.cpp/releases/download/$Tag/audio-$Tag-bin-windows-x64-vulkan.zip"
$Sha256 = "f884538138e44528a0bf17bb75dbe7ff7350cb91a14cb72c3e9cc1d4a31d6f1f"

$Root = Split-Path -Parent $PSScriptRoot
$Dest = Join-Path $Root "src-tauri\resources\audiocpp"
$Marker = Join-Path $Dest "VERSION.txt"

if ((Test-Path $Marker) -and ((Get-Content $Marker -Raw).Trim() -eq $Tag)) {
    Write-Host "Already present: $Dest ($Tag)"
    exit 0
}

$Work = Join-Path ([System.IO.Path]::GetTempPath()) "nitean-audiocpp-$Tag"
New-Item -ItemType Directory -Force $Work | Out-Null
$Zip = Join-Path $Work "audiocpp.zip"

Write-Host "Downloading audio.cpp $Tag (Vulkan) ..."
Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing

$Actual = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
if ($Actual -ne $Sha256) {
    throw "sha256 mismatch: expected $Sha256, got $Actual"
}

if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
New-Item -ItemType Directory -Force $Dest | Out-Null
Expand-Archive -Path $Zip -DestinationPath $Dest -Force
# Only the server is used; the CLI, GGUF tool and Python tools stay out of the installer.
foreach ($Extra in @("audiocpp_cli.exe", "audiocpp_gguf.exe", "tools")) {
    $P = Join-Path $Dest $Extra
    if (Test-Path $P) { Remove-Item -Recurse -Force $P }
}
Set-Content -Path $Marker -Value $Tag -Encoding ascii
Remove-Item -Recurse -Force $Work

Write-Host "audio.cpp $Tag -> $Dest"
