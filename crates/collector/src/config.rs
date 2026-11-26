//! Collector configuration: where data lives, which port to serve on, and the pairing token.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 47821;
pub const API_VERSION: u32 = 1;

/// Persisted in `<data_dir>/collector.json`. Created on first run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectorFile {
    pub machine_name: String,
    pub port: u16,
    pub token: String,
    /// Where files sent from other PCs are saved. Empty = `Downloads\Pane`.
    #[serde(default)]
    pub inbox_dir: String,
    /// Let paired PCs open a PowerShell here. Off unless turned on from this PC.
    #[serde(default)]
    pub allow_shell: bool,
    /// Let paired devices take screenshots of this PC. Off unless turned on from this PC.
    #[serde(default)]
    pub allow_screen: bool,
    /// Agent tasks from paired devices: "off" (default), "edits" or "full".
    #[serde(default = "agent_off")]
    pub agent_access: String,
}

fn agent_off() -> String {
    "off".into()
}

#[derive(Debug, Clone)]
pub struct CollectorConfig {
    pub machine_name: String,
    pub port: u16,
    pub token: String,
    /// Where the collector keeps its DB, token and status-line capture.
    pub data_dir: PathBuf,
    /// Claude Code's config dir (`%USERPROFILE%\.claude`, or `CLAUDE_CONFIG_DIR`).
    pub claude_dir: PathBuf,
    /// Received files land here.
    pub inbox_dir: PathBuf,
}

/// Optional overrides (from the CLI or the desktop app).
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub data_dir: Option<PathBuf>,
    pub claude_dir: Option<PathBuf>,
    pub port: Option<u16>,
    pub machine_name: Option<String>,
    pub inbox_dir: Option<PathBuf>,
}

pub fn default_data_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("ClaudeFleet");
    }
    directories::BaseDirs::new()
        .map(|b| b.config_dir().join("ClaudeFleet"))
        .unwrap_or_else(|| PathBuf::from(".claude-fleet"))
}

pub fn default_claude_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".claude"))
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

/// `%USERPROFILE%\Downloads\Pane`.
pub fn default_inbox_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(|d| d.to_path_buf()))
        .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join("Downloads")))
        .unwrap_or_else(|| PathBuf::from("Downloads"))
        .join("Pane")
}

pub fn hostname() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}

/// 32 random bytes, hex encoded.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl CollectorConfig {
    /// Load `<data_dir>/collector.json`, creating it (with a fresh token) on first run.
    /// Overrides for port and name are written back so they stick.
    pub fn load_or_create(ov: &Overrides) -> Result<Self> {
        let data_dir = ov.data_dir.clone().unwrap_or_else(default_data_dir);
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("creating data dir {}", data_dir.display()))?;
        let path = data_dir.join("collector.json");

        let mut file: CollectorFile = match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).with_context(|| format!("parsing {}", path.display()))?,
            Err(_) => CollectorFile {
                machine_name: hostname(),
                port: DEFAULT_PORT,
                token: new_token(),
                inbox_dir: String::new(),
                allow_shell: false,
                allow_screen: false,
                agent_access: agent_off(),
            },
        };
        if let Some(p) = ov.port {
            file.port = p;
        }
        if let Some(n) = &ov.machine_name {
            file.machine_name = n.clone();
        }
        if file.token.len() < 32 {
            file.token = new_token();
        }
        write_json_atomic(&path, &file)?;

        let inbox_dir = ov
            .inbox_dir
            .clone()
            .or_else(|| (!file.inbox_dir.trim().is_empty()).then(|| PathBuf::from(file.inbox_dir.trim())))
            .unwrap_or_else(default_inbox_dir);
        Ok(Self {
            inbox_dir,
            machine_name: file.machine_name,
            port: file.port,
            token: file.token,
            data_dir,
            claude_dir: ov.claude_dir.clone().unwrap_or_else(default_claude_dir),
        })
    }

    /// Read fresh from disk, so a change made in Settings applies to the next session at once.
    pub fn allow_shell(&self) -> bool {
        read_file(&self.data_dir).map(|f| f.allow_shell).unwrap_or(false)
    }

    pub fn allow_screen(&self) -> bool {
        read_file(&self.data_dir).map(|f| f.allow_screen).unwrap_or(false)
    }

    /// Re-read each time, so turning it off takes effect at once.
    pub fn agent_access(&self) -> String {
        get_agent_access(&self.data_dir)
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("fleet.db")
    }

    /// Where the status-line script drops Claude Code's status JSON.
    pub fn statusline_capture_path(&self) -> PathBuf {
        self.data_dir.join("statusline.json")
    }
}

fn read_file(data_dir: &Path) -> Option<CollectorFile> {
    let s = std::fs::read_to_string(data_dir.join("collector.json")).ok()?;
    serde_json::from_str(&s).ok()
}

/// Turn the remote shell on or off for this PC (only callable locally; there is no API for it).
pub fn set_allow_shell(data_dir: &Path, allow: bool) -> Result<bool> {
    let path = data_dir.join("collector.json");
    let mut f = read_file(data_dir).with_context(|| format!("reading {}", path.display()))?;
    f.allow_shell = allow;
    write_json_atomic(&path, &f)?;
    Ok(allow)
}

pub fn get_allow_shell(data_dir: &Path) -> bool {
    read_file(data_dir).map(|f| f.allow_shell).unwrap_or(false)
}

/// Allow screenshots of this PC (only callable locally; there is no API for it).
pub fn set_allow_screen(data_dir: &Path, allow: bool) -> Result<bool> {
    let path = data_dir.join("collector.json");
    let mut f = read_file(data_dir).with_context(|| format!("reading {}", path.display()))?;
    f.allow_screen = allow;
    write_json_atomic(&path, &f)?;
    Ok(allow)
}

pub fn get_allow_screen(data_dir: &Path) -> bool {
    read_file(data_dir).map(|f| f.allow_screen).unwrap_or(false)
}

/// Set agent access for this PC (only callable locally; there is no API for it).
pub fn set_agent_access(data_dir: &Path, level: &str) -> Result<String> {
    anyhow::ensure!(["off", "edits", "full"].contains(&level), "unknown access level {level}");
    let path = data_dir.join("collector.json");
    let mut f = read_file(data_dir).with_context(|| format!("reading {}", path.display()))?;
    f.agent_access = level.into();
    write_json_atomic(&path, &f)?;
    Ok(f.agent_access)
}

pub fn get_agent_access(data_dir: &Path) -> String {
    read_file(data_dir)
        .map(|f| f.agent_access)
        .filter(|a| ["edits", "full"].contains(&a.as_str()))
        .unwrap_or_else(agent_off)
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// One of this machine's addresses that other PCs could use to reach the collector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanAddress {
    pub ip: String,
    pub interface: String,
    /// "lan", "tailscale" or "virtual" (Hyper-V / WSL / VM adapters, rarely what you want).
    pub kind: String,
}

/// IPv4 addresses on private, link-local-free networks, best candidates first.
pub fn lan_addresses() -> Vec<LanAddress> {
    let Ok(nets) = sysinfo::Networks::new_with_refreshed_list() else { return Vec::new() };
    let mut out = Vec::new();
    for (name, data) in &nets {
        let iface = name.to_string_lossy().into_owned();
        let lower = iface.to_ascii_lowercase();
        for n in data.ip_networks() {
            let std::net::IpAddr::V4(v4) = n.addr else { continue };
            if v4.is_loopback() || v4.is_link_local() || !crate::api::is_lan(n.addr) {
                continue;
            }
            let o = v4.octets();
            let kind = if o[0] == 100 && (o[1] & 0xC0) == 64 || lower.contains("tailscale") {
                "tailscale"
            } else if ["vethernet", "wsl", "virtualbox", "vmware", "hyper-v", "docker", "vpn", "tap"]
                .iter()
                .any(|v| lower.contains(v))
            {
                "virtual"
            } else {
                "lan"
            };
            out.push(LanAddress { ip: v4.to_string(), interface: iface.clone(), kind: kind.to_string() });
        }
    }
    let rank = |k: &str| match k {
        "lan" => 0,
        "tailscale" => 1,
        _ => 2,
    };
    out.sort_by_key(|a| rank(&a.kind));
    out
}
