//! Phone app: just the dashboard. It runs no collector and touches nothing on the phone
//! except showing notifications and installing its own updates; machines and settings live in
//! the page's own storage.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::time::Duration;
use tauri::{Emitter, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

/// Async so the permission prompt doesn't block the UI thread.
#[tauri::command]
async fn notify(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    let n = app.notification();
    if n.permission_state().map_err(|e| e.to_string())? != PermissionState::Granted
        && n.request_permission().map_err(|e| e.to_string())? != PermissionState::Granted
    {
        return Err("Notifications are turned off for Claude Fleet".into());
    }
    n.builder().title(title).body(body).show().map_err(|e| e.to_string())
}

// ---- Updates -------------------------------------------------------------------------------
//
// The same `latest.json` the Windows updater reads, from the project's GitHub releases. The
// APK's SHA-256 is checked against the manifest, and Android itself only installs it over this
// app if it is signed with the same key (which never leaves the build PC).

const MANIFEST_URL: &str = "https://github.com/GoldenJayz/claude-fleet/releases/latest/download/latest.json";
const DOWNLOAD_PREFIX: &str = "https://github.com/GoldenJayz/claude-fleet/releases/download/";
const PLATFORM: &str = "android-aarch64";

#[derive(Deserialize)]
struct Manifest {
    version: String,
    notes: Option<String>,
    platforms: HashMap<String, Platform>,
}

#[derive(Deserialize)]
struct Platform {
    url: String,
    sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    version: String,
    current: String,
    notes: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(300))).build())
}

fn user_agent() -> &'static str {
    concat!("claude-fleet-android/", env!("CARGO_PKG_VERSION"))
}

fn manifest() -> Result<Manifest, String> {
    agent()
        .get(MANIFEST_URL)
        .header("User-Agent", user_agent())
        .call()
        .map_err(|e| format!("couldn't check for updates: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("unexpected update manifest: {e}"))
}

fn parse_version(v: &str) -> (u64, u64, u64) {
    let mut it = v.trim_start_matches('v').split(['.', '-']).map(|p| p.parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

async fn newer(app: &tauri::AppHandle) -> Result<Option<(Manifest, String)>, String> {
    let m = tauri::async_runtime::spawn_blocking(manifest).await.map_err(|e| e.to_string())??;
    let current = app.package_info().version.to_string();
    let has_apk = m.platforms.contains_key(PLATFORM);
    Ok((has_apk && parse_version(&m.version) > parse_version(&current)).then_some((m, current)))
}

#[tauri::command]
async fn check_update(app: tauri::AppHandle) -> Result<Option<UpdateInfo>, String> {
    Ok(newer(&app).await?.map(|(m, current)| UpdateInfo { version: m.version, current, notes: m.notes }))
}

/// Download and verify the new APK; returns its path for the installer (MainActivity).
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<String, String> {
    let Some((m, _)) = newer(&app).await? else { return Err("Claude Fleet is already up to date.".into()) };
    let p = m.platforms.get(PLATFORM).ok_or("no Android build in this release")?;
    if !p.url.starts_with(DOWNLOAD_PREFIX) {
        return Err("the update points somewhere other than Claude Fleet's releases; not installing it".into());
    }
    let want = p.sha256.clone().ok_or("the update has no checksum; not installing it")?.to_lowercase();
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("updates");
    let url = p.url.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("ClaudeFleet.apk");
        let mut resp = agent().get(&url).header("User-Agent", user_agent()).call().map_err(|e| format!("download failed: {e}"))?;
        let total = resp.body().content_length();
        let mut reader = resp.body_mut().with_config().limit(500 * 1024 * 1024).reader();
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut got = 0u64;
        let mut last_emit = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("download failed: {e}"))?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            got += n as u64;
            if got - last_emit > 256 * 1024 {
                last_emit = got;
                let _ = handle.emit("fleet://update-progress", (got, total));
            }
        }
        file.flush().map_err(|e| e.to_string())?;
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if digest != want {
            let _ = std::fs::remove_file(&path);
            return Err("the download doesn't match the release's checksum; not installing it".into());
        }
        let _ = handle.emit("fleet://update-progress", (got, Some(got)));
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![notify, check_update, install_update])
        .run(tauri::generate_context!())
        .expect("error while running Claude Fleet");
}
