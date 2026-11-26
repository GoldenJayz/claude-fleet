//! Desktop app: runs this machine's collector in the background and shows the dashboard,
//! which connects to every collector in the machine list (this PC included).

use crate::{system, tray, updates};
use fleet_collector::{api, Collector, CollectorConfig, Overrides};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Manager, State};

/// What the dashboard needs to reach this machine's own collector.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalCollector {
    pub machine_name: String,
    pub port: u16,
    pub token: String,
    /// False when another collector (e.g. the standalone `fleet-collector`) already owns the port;
    /// the dashboard then talks to that one, which shares the same token.
    pub embedded: bool,
    pub error: Option<String>,
    pub data_dir: String,
}

#[tauri::command]
fn local_collector(state: State<'_, Mutex<LocalCollector>>) -> LocalCollector {
    state.lock().unwrap().clone()
}

/// A remote machine in the dashboard's list (this PC is always shown and is not stored).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedMachine {
    pub id: String,
    pub name: String,
    /// host:port
    pub address: String,
    pub token: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DashboardFile {
    #[serde(default)]
    machines: Vec<SavedMachine>,
}

/// `%APPDATA%\ClaudeFleet\dashboard.json`: adding a PC is an entry here, not a code change.
fn dashboard_path() -> std::path::PathBuf {
    fleet_collector::config::default_data_dir().join("dashboard.json")
}

#[tauri::command]
fn get_machines() -> Vec<SavedMachine> {
    std::fs::read_to_string(dashboard_path())
        .ok()
        .and_then(|s| serde_json::from_str::<DashboardFile>(&s).ok())
        .map(|f| f.machines)
        .unwrap_or_default()
}

#[tauri::command]
fn save_machines(machines: Vec<SavedMachine>) -> Result<(), String> {
    let path = dashboard_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fleet_collector::config::write_json_atomic(&path, &DashboardFile { machines }).map_err(|e| e.to_string())
}

#[tauri::command]
fn local_addresses() -> Vec<fleet_collector::config::LanAddress> {
    fleet_collector::config::lan_addresses()
}

fn start_collector(app: &tauri::AppHandle) -> anyhow::Result<LocalCollector> {
    let cfg = CollectorConfig::load_or_create(&Overrides::default())?;
    let port_free = std::net::TcpListener::bind(("0.0.0.0", cfg.port)).is_ok();
    let info = LocalCollector {
        machine_name: cfg.machine_name.clone(),
        port: cfg.port,
        token: cfg.token.clone(),
        embedded: port_free,
        error: None,
        data_dir: cfg.data_dir.display().to_string(),
    };
    if port_free {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let result = match Collector::start(cfg).await {
                Ok(c) => api::serve(c.state).await,
                Err(e) => Err(e),
            };
            if let Err(e) = result {
                tracing::error!("collector stopped: {e:#}");
                if let Some(s) = handle.try_state::<Mutex<LocalCollector>>() {
                    s.lock().unwrap().error = Some(format!("{e:#}"));
                }
            }
        });
    } else {
        tracing::info!("port {} already in use; using the collector that is running there", info.port);
    }
    Ok(info)
}

/// Window backdrop behind the (translucent) UI:
/// - "acrylic": the desktop behind the window, blurred
/// - "mica": the wallpaper tint Windows 11 uses for settings-style windows
/// - "clear": no blur at all, the desktop shows through as-is
/// The UI draws its own adjustable dark tint on top, so the native tint is left transparent.
fn set_effect(window: &tauri::WebviewWindow, effect: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = window_vibrancy::clear_acrylic(window);
        let _ = window_vibrancy::clear_mica(window);
        let r = match effect {
            "acrylic" => window_vibrancy::apply_acrylic(window, Some((0, 0, 0, 0))),
            "mica" => window_vibrancy::apply_mica(window, Some(true)),
            _ => Ok(()),
        };
        return r.map_err(|e| e.to_string());
    }
    #[allow(unreachable_code)]
    {
        let _ = (window, effect);
        Ok(())
    }
}

#[tauri::command]
fn set_window_effect(window: tauri::WebviewWindow, effect: String) -> Result<(), String> {
    set_effect(&window, &effect)
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=warn".into()),
        )
        .init();

    // Started by Windows at sign-in: go straight to the tray.
    let minimized = std::env::args().any(|a| a == "--minimized");

    tauri::Builder::default()
        // A second launch just brings the running window forward (one collector per PC).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                tray::hide_main(window.app_handle());
            }
        })
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                // The UI re-applies the saved choice on load; start with acrylic.
                if let Err(e) = set_effect(&window, "acrylic") {
                    tracing::warn!("acrylic unavailable: {e}");
                }
                if !minimized {
                    let _ = window.show();
                }
            }
            tray::create(app.handle())?;
            migrate_autostart(app.handle());
            let info = start_collector(app.handle()).unwrap_or_else(|e| LocalCollector {
                machine_name: fleet_collector::config::hostname(),
                port: fleet_collector::config::DEFAULT_PORT,
                token: String::new(),
                embedded: false,
                error: Some(format!("{e:#}")),
                data_dir: String::new(),
            });
            app.manage(Mutex::new(info));
            updates::spawn_checks(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            local_collector,
            set_window_effect,
            get_machines,
            save_machines,
            local_addresses,
            system::firewall_status,
            system::firewall_allow,
            system::firewall_remove,
            system::statusline_install,
            system::statusline_uninstall,
            system::get_autostart,
            system::set_autostart,
            system::notify,
            system::get_remote_shell,
            system::set_remote_shell,
            system::get_allow_screen,
            system::set_allow_screen,
            system::get_agent_access,
            system::set_agent_access,
            system::inbox_dir,
            system::open_inbox,
            system::reveal_received,
            updates::check_update,
            updates::install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Pane");
}

/// Before the rename to Pane, "Start with Windows" was saved under the name "Claude Fleet" (with
/// the old install path). Move it over: remove the old entry and re-enable under the new name.
fn migrate_autostart(app: &tauri::AppHandle) {
    use std::os::windows::process::CommandExt;
    use tauri_plugin_autostart::ManagerExt;
    const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NO_WINDOW: u32 = 0x0800_0000;
    let reg = |args: &[&str]| {
        std::process::Command::new("reg").args(args).creation_flags(NO_WINDOW).output().map(|o| o.status.success()).unwrap_or(false)
    };
    if !reg(&["query", RUN, "/v", "Claude Fleet"]) {
        return;
    }
    if reg(&["delete", RUN, "/v", "Claude Fleet", "/f"]) {
        match app.autolaunch().enable() {
            Ok(()) => tracing::info!("moved Start with Windows from \"Claude Fleet\" to \"Pane\""),
            Err(e) => tracing::warn!("couldn't re-enable Start with Windows after the rename: {e}"),
        }
        tray::sync_autostart(app);
    }
}
