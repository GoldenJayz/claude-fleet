# Builds the Claude Fleet phone app for arm64 Android phones (e.g. Galaxy S23).
#   .\scripts\build-android.ps1            -> target\android\ClaudeFleet-arm64.apk
#   .\scripts\build-android.ps1 -Install   -> also installs it on the phone connected over USB (adb)
param([switch]$Install)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if (-not $env:NDK_HOME) {
    $ndk = Get-ChildItem "$env:LOCALAPPDATA\Android\Sdk\ndk" -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1
    if (-not $ndk) { throw 'Android NDK not found. Install it from Android Studio > SDK Manager > SDK Tools.' }
    $env:NDK_HOME = $ndk.FullName
}

$android = Join-Path $root 'src-tauri\gen\android'
if (-not (Test-Path (Join-Path $android 'keystore.properties'))) {
    $props = Join-Path $env:APPDATA 'ClaudeFleet\android-signing\keystore.properties'
    if (Test-Path $props) { Copy-Item $props $android }
    else { Write-Warning 'No signing key found; the release APK will be unsigned and cannot be installed.' }
}

# 1. Frontend + Rust library. Tauri then links the library into the Android project with a
#    symbolic link, which Windows only allows in Developer Mode. Without it that step fails,
#    so we copy the library ourselves and run Gradle directly.
$lib = Join-Path $root 'target\aarch64-linux-android\release\libclaude_fleet_lib.so'
$started = Get-Date
npx tauri android build --apk --target aarch64
if ($LASTEXITCODE -ne 0) {
    if (-not (Test-Path $lib) -or (Get-Item $lib).LastWriteTime -lt $started) { throw 'The Rust build failed (see above).' }
    Write-Host 'Packaging with Gradle (copying the library instead of linking it)...'
    Copy-Item $lib (Join-Path $android 'app\src\main\jniLibs\arm64-v8a\') -Force
    Push-Location $android
    try {
        .\gradlew.bat assembleArm64Release -x rustBuildArm64Release --console=plain
        if ($LASTEXITCODE -ne 0) { throw 'Gradle build failed.' }
    } finally { Pop-Location }
}

$apk = Get-ChildItem (Join-Path $android 'app\build\outputs\apk') -Recurse -Filter '*arm64*release*.apk' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $apk) { throw 'No APK was produced.' }
$out = Join-Path $root 'target\android'
New-Item -ItemType Directory -Force $out | Out-Null
$dest = Join-Path $out 'ClaudeFleet-arm64.apk'
Copy-Item $apk.FullName $dest -Force
Write-Host "APK: $dest ($([math]::Round((Get-Item $dest).Length / 1MB, 1)) MB)"

if ($Install) {
    $adb = Join-Path $env:LOCALAPPDATA 'Android\Sdk\platform-tools\adb.exe'
    & $adb install -r $dest
    if ($LASTEXITCODE -ne 0) { throw 'adb install failed. Is the phone plugged in with USB debugging on, and the prompt accepted?' }
    & $adb shell monkey -p dev.claudefleet.app -c android.intent.category.LAUNCHER 1 | Out-Null
    Write-Host 'Installed and opened on the phone.'
}
