//! Things that touch Windows itself, each only on an explicit click in Settings:
//! the firewall rule, Claude Code's status line, start-with-Windows, and notifications.

use fleet_collector::{statusline, CollectorConfig, Overrides};
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

pub const FIREWALL_RULE: &str = "Claude Fleet collector";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Run a PowerShell script without a console window; returns stdout.
fn powershell(script: &str) -> Result<String, String> {
    // -EncodedCommand (UTF-16LE base64) avoids every quoting problem.
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64(&utf16);
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &encoded]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("starting PowerShell: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatus {
    pub exists: bool,
    pub enabled: bool,
    pub port: Option<String>,
    pub program: Option<String>,
    /// True when the rule points at this exact program (e.g. not an older install path).
    pub matches_this_app: bool,
    /// True when the rule admits every private range, not just this PC's subnet. Older rules
    /// only allowed the local subnet, which blocks phones on a VPN that hands out its own
    /// subnet (e.g. 192.168.2.x on a router's OpenVPN while the PCs are on 192.168.1.x).
    pub covers_private_ranges: bool,
}

#[tauri::command]
pub fn firewall_status() -> Result<FirewallStatus, String> {
    // Reading rules needs no admin rights.
    let out = powershell(&format!(
        "$r = Get-NetFirewallRule -DisplayName '{FIREWALL_RULE}' -ErrorAction SilentlyContinue | Select-Object -First 1
         if (-not $r) {{ 'none'; exit }}
         $p = $r | Get-NetFirewallPortFilter; $a = $r | Get-NetFirewallApplicationFilter; $f = $r | Get-NetFirewallAddressFilter
         \"$($r.Enabled)|$($p.LocalPort)|$(@($f.RemoteAddress) -join ',')|$($a.Program)\""
    ))?;
    if out == "none" || out.is_empty() {
        return Ok(FirewallStatus { exists: false, enabled: false, port: None, program: None, matches_this_app: false, covers_private_ranges: false });
    }
    let parts: Vec<&str> = out.splitn(4, '|').collect();
    let remote = parts.get(2).copied().unwrap_or("").to_ascii_lowercase();
    let program = parts.get(3).map(|s| s.to_string()).filter(|s| !s.is_empty() && s != "Any");
    let exe = std::env::current_exe().ok().map(|p| p.display().to_string().to_ascii_lowercase());
    Ok(FirewallStatus {
        exists: true,
        enabled: parts.first().is_some_and(|e| e.eq_ignore_ascii_case("true")),
        port: parts.get(1).map(|s| s.to_string()),
        matches_this_app: program.as_ref().map(|p| p.to_ascii_lowercase()) == exe,
        covers_private_ranges: remote.split(',').any(|r| r == "any" || r.starts_with("192.168.0.0/")),
        program,
    })
}

/// Adds (or replaces) an inbound rule for this app's port, limited to private addresses (the
/// local subnet, 10/8, 172.16/12, 192.168/16, so VPN clients on their own subnet get in) and
/// Tailscale (100.64.0.0/10), on Private and Domain networks. Windows shows a UAC prompt;
/// nothing changes if the user declines.
#[tauri::command]
pub fn firewall_allow(port: u16) -> Result<FirewallStatus, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.display().to_string();
    let netsh = format!(
        "/c netsh advfirewall firewall delete rule name=\"{FIREWALL_RULE}\" >nul 2>&1 & \
         netsh advfirewall firewall add rule name=\"{FIREWALL_RULE}\" dir=in action=allow protocol=TCP \
         localport={port} program=\"{exe}\" profile=private,domain remoteip=localsubnet,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,100.64.0.0/10 \
         description=\"Lets your other PCs and phones reach Claude Fleet on this one (private networks, VPN and Tailscale only).\""
    );
    let args = netsh.replace('\'', "''");
    powershell(&format!(
        "try {{ Start-Process -FilePath cmd.exe -ArgumentList '{args}' -Verb RunAs -Wait -WindowStyle Hidden }}
         catch {{ Write-Error 'cancelled'; exit 1 }}"
    ))
    .map_err(|e| if e.contains("cancel") { "Windows didn't get admin approval, so nothing was changed.".to_string() } else { e })?;
    firewall_status()
}

#[tauri::command]
pub fn firewall_remove() -> Result<FirewallStatus, String> {
    let args = format!("/c netsh advfirewall firewall delete rule name=\"{FIREWALL_RULE}\"").replace('\'', "''");
    powershell(&format!(
        "try {{ Start-Process -FilePath cmd.exe -ArgumentList '{args}' -Verb RunAs -Wait -WindowStyle Hidden }}
         catch {{ Write-Error 'cancelled'; exit 1 }}"
    ))
    .map_err(|e| if e.contains("cancel") { "Windows didn't get admin approval, so nothing was changed.".to_string() } else { e })?;
    firewall_status()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusLineChange {
    pub installed: bool,
    pub backup: Option<String>,
    pub chained: Option<String>,
    pub settings_path: String,
}

fn cfg() -> Result<CollectorConfig, String> {
    CollectorConfig::load_or_create(&Overrides::default()).map_err(|e| format!("{e:#}"))
}

/// Merges the status line into Claude Code's settings.json (backup first, other settings kept,
/// an existing status line keeps running).
#[tauri::command]
pub fn statusline_install() -> Result<StatusLineChange, String> {
    let c = cfg()?;
    let r = statusline::install(&c).map_err(|e| format!("{e:#}"))?;
    Ok(StatusLineChange {
        installed: true,
        backup: r.backup.map(|b| b.display().to_string()),
        chained: r.chained,
        settings_path: r.settings.display().to_string(),
    })
}

#[tauri::command]
pub fn statusline_uninstall() -> Result<StatusLineChange, String> {
    let c = cfg()?;
    let backup = statusline::uninstall(&c).map_err(|e| format!("{e:#}"))?;
    Ok(StatusLineChange {
        installed: false,
        backup: backup.map(|b| b.display().to_string()),
        chained: None,
        settings_path: c.claude_dir.join("settings.json").display().to_string(),
    })
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    crate::tray::sync_autostart(&app);
    Ok(al.is_enabled().unwrap_or(false))
}

#[tauri::command]
pub fn notify(app: AppHandle, title: String, body: String) -> Result<(), String> {
    app.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
}

// ---- Remote shell and received files ----------------------------------------------------

fn data_dir() -> std::path::PathBuf {
    fleet_collector::config::default_data_dir()
}

fn inbox() -> Result<std::path::PathBuf, String> {
    CollectorConfig::load_or_create(&Overrides::default()).map(|c| c.inbox_dir).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn get_remote_shell() -> bool {
    fleet_collector::config::get_allow_shell(&data_dir())
}

/// Only this PC can allow remote shells on itself; there is deliberately no API for it.
#[tauri::command]
pub fn set_remote_shell(enabled: bool) -> Result<bool, String> {
    fleet_collector::config::set_allow_shell(&data_dir(), enabled).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn get_allow_screen() -> bool {
    fleet_collector::config::get_allow_screen(&data_dir())
}

/// Only this PC can allow screenshots of itself; there is no API for it.
#[tauri::command]
pub fn set_allow_screen(enabled: bool) -> Result<bool, String> {
    fleet_collector::config::set_allow_screen(&data_dir(), enabled).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn get_agent_access() -> String {
    fleet_collector::config::get_agent_access(&data_dir())
}

/// Only this PC can let other devices run Claude Code tasks on it; there is no API for it.
#[tauri::command]
pub fn set_agent_access(level: String) -> Result<String, String> {
    fleet_collector::config::set_agent_access(&data_dir(), &level).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn inbox_dir() -> Result<String, String> {
    inbox().map(|p| p.display().to_string())
}

fn explorer(args: &[&std::ffi::OsStr]) -> Result<(), String> {
    std::process::Command::new("explorer.exe").args(args).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_inbox() -> Result<(), String> {
    let dir = inbox()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    explorer(&[dir.as_os_str()])
}

/// Show a received file in Explorer. Only paths inside the received-files folder are allowed.
#[tauri::command]
pub fn reveal_received(path: String) -> Result<(), String> {
    let dir = inbox()?.canonicalize().map_err(|e| e.to_string())?;
    let p = std::path::PathBuf::from(&path).canonicalize().map_err(|_| "That file has been moved or deleted.".to_string())?;
    if !p.starts_with(&dir) {
        return Err("Not in the received files folder.".into());
    }
    // `explorer /select,` wants a plain path, not the \\?\ form canonicalize returns.
    let plain = p.display().to_string().trim_start_matches(r"\\?\").to_string();
    let arg = std::ffi::OsString::from(format!("/select,{plain}"));
    explorer(&[arg.as_os_str()])
}
