# Publishes a new Pane version: every installed copy (Windows and the phone) offers it.
#   .\scripts\release.ps1 -Version 0.2.1 -Notes "What changed"
#
# 1. Sets the version in package.json, Cargo.toml and tauri.conf.json.
# 2. Builds the Windows installer (signed with the updater key) and the Android APK.
# 3. Writes latest.json (what the apps check), commits the version bump, pushes, and creates the
#    GitHub release vX.Y.Z with the installers attached.
#
# Signing keys stay in %APPDATA%\ClaudeFleet\updater-signing and android-signing (never in the repo).
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Notes = '',
    [switch]$SkipAndroid
)
$ErrorActionPreference = 'Stop'

# Windows PowerShell treats anything a program writes to stderr as an error under 'Stop' (when
# output is redirected), even progress lines. Run programs leniently and check their exit code.
function Invoke-Native([scriptblock]$Command) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Command } finally { $ErrorActionPreference = $saved }
}
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
$repo = 'GoldenJayz/pane'

if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version must look like 1.2.3 (got '$Version')." }
$conf = Join-Path $root 'src-tauri\tauri.conf.json'
$current = (Get-Content $conf -Raw | ConvertFrom-Json).version
if ([version]$Version -le [version]$current) { throw "Version $Version must be newer than the current $current." }
if (Invoke-Native { git status --porcelain }) { throw 'Commit or stash your changes first, so the release matches the pushed code.' }
Invoke-Native { gh auth status } *> $null; if ($LASTEXITCODE -ne 0) { throw 'Sign in to GitHub first: gh auth login' }

# --- 1. version bump ---------------------------------------------------------------------------
function Set-Text($path, $pattern, $replacement) {
    $text = [IO.File]::ReadAllText($path)
    $new = [regex]::Replace($text, $pattern, $replacement, 1)
    if ($new -eq $text) { throw "Couldn't set the version in $path" }
    [IO.File]::WriteAllText($path, $new)
}
Set-Text (Join-Path $root 'package.json') '"version":\s*"[^"]+"' "`"version`": `"$Version`""
Set-Text $conf '"version":\s*"[^"]+"' "`"version`": `"$Version`""
Set-Text (Join-Path $root 'Cargo.toml') '(\[workspace\.package\]\s*\r?\nversion\s*=\s*)"[^"]+"' "`${1}`"$Version`""

# --- 2. builds ---------------------------------------------------------------------------------
$keys = Join-Path $env:APPDATA 'ClaudeFleet\updater-signing'
$env:TAURI_SIGNING_PRIVATE_KEY = Join-Path $keys 'claude-fleet.key'
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content (Join-Path $keys 'password.txt') -Raw).Trim()
Invoke-Native { npm run tauri build }
if ($LASTEXITCODE -ne 0) { throw 'Windows build failed.' }
$msi = Get-Item (Join-Path $root "target\release\bundle\msi\Pane_${Version}_x64_en-US.msi")
$sig = Get-Content "$($msi.FullName).sig" -Raw

$out = Join-Path $root "target\release-$Version"
New-Item -ItemType Directory -Force $out | Out-Null
# GitHub turns spaces in asset names into dots, so use a name without spaces.
$msiName = "Pane_${Version}_x64_en-US.msi"
Copy-Item $msi.FullName (Join-Path $out $msiName) -Force
$assets = @((Join-Path $out $msiName))
# Download links use the repo's old name: GitHub redirects it, and phones on 0.3.4 or older only
# accept update links under that address.
$base = "https://github.com/GoldenJayz/claude-fleet/releases/download/v$Version"
$platforms = [ordered]@{ 'windows-x86_64' = [ordered]@{ signature = $sig.Trim(); url = "$base/$msiName" } }

if (-not $SkipAndroid) {
    & (Join-Path $PSScriptRoot 'build-android.ps1')
    $apkName = "Pane_${Version}_arm64.apk"
    Copy-Item (Join-Path $root 'target\android\ClaudeFleet-arm64.apk') (Join-Path $out $apkName) -Force
    $sha = (Get-FileHash (Join-Path $out $apkName) -Algorithm SHA256).Hash.ToLower()
    # The Windows updater rejects the whole latest.json if any platform lacks a "signature", so
    # sign the APK with the updater key too (the phone itself checks sha256 and the APK signature).
    Remove-Item (Join-Path $out "$apkName.sig") -ErrorAction SilentlyContinue
    $keyPath = $env:TAURI_SIGNING_PRIVATE_KEY; $keyPass = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD   # they conflict with -f/-p
    Invoke-Native { npm run tauri -- signer sign -f $keyPath -p $keyPass (Join-Path $out $apkName) }
    if ($LASTEXITCODE -ne 0) { throw 'Signing the APK failed.' }
    $apkSig = (Get-Content (Join-Path $out "$apkName.sig") -Raw).Trim()
    $platforms['android-aarch64'] = [ordered]@{ signature = $apkSig; url = "$base/$apkName"; sha256 = $sha }
    $assets += (Join-Path $out $apkName)
}
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY, Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue

$manifest = [ordered]@{
    version   = $Version
    notes     = $Notes
    pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = $platforms
}
$latest = Join-Path $out 'latest.json'
[IO.File]::WriteAllText($latest, ($manifest | ConvertTo-Json -Depth 5))
$assets += $latest

# --- 3. publish --------------------------------------------------------------------------------
Invoke-Native { git add package.json Cargo.toml Cargo.lock src-tauri/tauri.conf.json }
Invoke-Native { git commit -m "Release v$Version" }
if ($LASTEXITCODE -ne 0) { throw 'git commit failed.' }
Invoke-Native { git push }
if ($LASTEXITCODE -ne 0) { throw 'git push failed; the release was not published.' }
$notesArg = if ($Notes) { $Notes } else { "Pane $Version" }
$head = Invoke-Native { git rev-parse HEAD }
Invoke-Native { gh release create "v$Version" @assets --repo $repo --target $head --title "Pane $Version" --notes $notesArg }
if ($LASTEXITCODE -ne 0) { throw 'Creating the GitHub release failed.' }
Write-Host "Released v$Version. Installed copies will offer the update within a few hours (or Settings > About > Check for updates)."
