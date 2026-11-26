//! Windows auto-update from the project's GitHub releases (`latest.json`).
//!
//! Every update is signed with a key that never leaves the build PC; the public half is
//! compiled into the app (tauri.conf.json → plugins.updater.pubkey), and anything not signed
//! with it is refused before it is installed.

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
}

async fn find(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())
}

fn info(u: &tauri_plugin_updater::Update) -> UpdateInfo {
    UpdateInfo { version: u.version.clone(), current: u.current_version.clone(), notes: u.body.clone() }
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    Ok(find(&app).await?.as_ref().map(info))
}

/// Download, verify the signature, install (Windows asks for permission) and restart.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let Some(update) = find(&app).await? else { return Err("Pane is already up to date.".into()) };
    let progress = app.clone();
    let mut got = 0u64;
    update
        .download_and_install(
            move |chunk, total| {
                got += chunk as u64;
                let _ = progress.emit("fleet://update-progress", (got, total));
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

/// Look for updates shortly after start and then every few hours; tell the UI and Windows once
/// per new version.
pub fn spawn_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut told: Option<String> = None;
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            match find(&app).await {
                Ok(Some(u)) => {
                    let i = info(&u);
                    let _ = app.emit("fleet://update-available", i.clone());
                    if told.as_deref() != Some(i.version.as_str()) {
                        let _ = crate::system::notify(
                            app.clone(),
                            format!("Pane {} is available", i.version),
                            "Open Pane to install it.".into(),
                        );
                        told = Some(i.version);
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::info!("update check failed: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}
