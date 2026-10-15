# Downloads the pinned stable-diffusion.cpp Vulkan build (sd-server.exe + DLLs) into
# src-tauri/resources/sdcpp/ (bundled as an app resource, docs/08-local-gpu.md §3).
# Run once after cloning:
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-sdcpp.ps1
# Not committed (~90 MB). Keep $Tag in sync with resources/local_models.json "engine_version".

$ErrorActionPreference = "Stop"

$Tag = "master-929-3f8527a"
$Url = "https://github.com/leejet/stable-diffusion.cpp/releases/download/$Tag/sd-master-3f8527a-bin-win-vulkan-x64.zip"
$Sha256 = "60e6850d650417409f18c2170ab5e27335db96da70cd3d1ad1e930bcffc2fd35"

$Root = Split-Path -Parent $PSScriptRoot
$Dest = Join-Path $Root "src-tauri\resources\sdcpp"
$Marker = Join-Path $Dest "VERSION.txt"

if ((Test-Path $Marker) -and ((Get-Content $Marker -Raw).Trim() -eq $Tag)) {
    Write-Host "Already present: $Dest ($Tag)"
    exit 0
}

$Work = Join-Path ([System.IO.Path]::GetTempPath()) "nitean-sdcpp-$Tag"
New-Item -ItemType Directory -Force $Work | Out-Null
$Zip = Join-Path $Work "sdcpp.zip"

Write-Host "Downloading stable-diffusion.cpp $Tag (Vulkan) ..."
Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing

$Actual = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
if ($Actual -ne $Sha256) {
    throw "sha256 mismatch: expected $Sha256, got $Actual"
}

if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
New-Item -ItemType Directory -Force $Dest | Out-Null
Expand-Archive -Path $Zip -DestinationPath $Dest -Force
# Only the server is used; the CLI stays out of the installer.
Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $Dest "sd-cli.exe")
Set-Content -Path $Marker -Value $Tag -Encoding ascii
Remove-Item -Recurse -Force $Work

Write-Host "stable-diffusion.cpp $Tag -> $Dest"
