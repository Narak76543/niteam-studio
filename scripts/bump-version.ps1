# Sets the app version in every place that holds it (docs/RELEASING.md, step 2):
#   powershell -ExecutionPolicy Bypass -File scripts/bump-version.ps1 -Version 0.4.0
# package.json, package-lock.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock, src-tauri/tauri.conf.json.

param([Parameter(Mandatory)][string]$Version)

$ErrorActionPreference = "Stop"

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Use three numbers, e.g. 0.4.0 (got '$Version')" }

$Root = Split-Path -Parent $PSScriptRoot
$Conf = Join-Path $Root "src-tauri\tauri.conf.json"
$Old = (Get-Content $Conf -Raw | ConvertFrom-Json).version
if ([version]$Version -le [version]$Old) { throw "New version $Version must be higher than the current $Old" }

function Replace-Once([string]$Path, [string]$Pattern, [string]$Replacement) {
    $text = [System.IO.File]::ReadAllText($Path)
    $re = [regex]$Pattern
    if (-not $re.IsMatch($text)) { throw "Version not found in $Path" }
    [System.IO.File]::WriteAllText($Path, $re.Replace($text, $Replacement, 1), (New-Object System.Text.UTF8Encoding $false))
}

Replace-Once $Conf '"version":\s*"[^"]+"' "`"version`": `"$Version`""
Replace-Once (Join-Path $Root "src-tauri\Cargo.toml") '(?m)^version = "[^"]+"' "version = `"$Version`""

Push-Location $Root
# npm/cargo print progress on stderr; Windows PowerShell 5.1 would treat that as an error under "Stop".
$ErrorActionPreference = "Continue"
try {
    npm version $Version --no-git-tag-version --allow-same-version 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "npm version failed" }
    cargo update --manifest-path src-tauri/Cargo.toml --package nitean-studio 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "cargo update failed (Cargo.lock)" }
} finally {
    $ErrorActionPreference = "Stop"
    Pop-Location
}

Write-Host "Version $Old -> $Version"
Write-Host "Changed: package.json, package-lock.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock, src-tauri/tauri.conf.json"
