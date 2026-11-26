//! Plan usage via Claude Code's status line.
//!
//! Claude Code runs the `statusLine.command` from settings.json and pipes JSON to it on stdin;
//! for subscribers that JSON carries `rate_limits.five_hour` / `rate_limits.seven_day`
//! (`used_percentage`, `resets_at` in Unix seconds). Our script saves it; we read it here.
//!
//! Installing merges into settings.json: every other setting is kept, a timestamped backup is
//! written first, and an existing status line is preserved by chaining to it.

use crate::config::CollectorConfig;
use crate::types::{LimitWindow, PlanReading, StatusLineState};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SCRIPT: &str = include_str!("../assets/statusline.sh");
const SCRIPT_NAME: &str = "statusline.sh";
const CHAIN_NAME: &str = "statusline-chain.sh";
/// Appended to the command so we can recognise our own entry (Git Bash treats it as a comment).
const MARKER: &str = "# claude-fleet";

/// Write (or refresh) the script into the data dir. Called on every start.
pub fn write_script(cfg: &CollectorConfig) -> Result<PathBuf> {
    let path = cfg.data_dir.join(SCRIPT_NAME);
    // LF line endings: Git Bash chokes on CRLF.
    std::fs::write(&path, SCRIPT.replace("\r\n", "\n"))?;
    Ok(path)
}

/// Claude Code runs status-line commands through Git Bash on Windows: forward slashes only.
fn command_for(script: &Path) -> String {
    let p = script.to_string_lossy().replace('\\', "/");
    format!("bash \"{p}\" {MARKER}")
}

fn settings_path(cfg: &CollectorConfig) -> PathBuf {
    cfg.claude_dir.join("settings.json")
}

fn read_settings(path: &Path) -> Result<Value> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(json!({})),
        Ok(s) => serde_json::from_str(&s).with_context(|| format!("{} is not valid JSON", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e.into()),
    }
}

fn write_settings(path: &Path, v: &Value) -> Result<()> {
    let tmp = path.with_extension("json.claude-fleet.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v)? + "\n")?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn backup(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let b = path.with_file_name(format!("settings.json.claude-fleet-backup-{stamp}"));
    std::fs::copy(path, &b)?;
    Ok(Some(b))
}

pub fn state(cfg: &CollectorConfig) -> StatusLineState {
    let installed = read_settings(&settings_path(cfg))
        .ok()
        .and_then(|v| v.pointer("/statusLine/command").and_then(Value::as_str).map(|c| c.contains(MARKER)))
        .unwrap_or(false);
    let chained_command = std::fs::read_to_string(cfg.data_dir.join(CHAIN_NAME))
        .ok()
        .and_then(|s| s.lines().filter(|l| !l.starts_with('#')).map(str::trim).find(|l| !l.is_empty()).map(str::to_string));
    StatusLineState { installed, chained_command }
}

pub struct InstallReport {
    pub settings: PathBuf,
    pub backup: Option<PathBuf>,
    pub chained: Option<String>,
    pub already: bool,
}

pub fn install(cfg: &CollectorConfig) -> Result<InstallReport> {
    let script = write_script(cfg)?;
    let path = settings_path(cfg);
    let mut v = read_settings(&path)?;
    let Some(obj) = v.as_object_mut() else { bail!("{} is not a JSON object", path.display()) };

    let existing = obj.get("statusLine").cloned();
    let existing_cmd = existing
        .as_ref()
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if existing_cmd.as_deref().is_some_and(|c| c.contains(MARKER)) {
        return Ok(InstallReport { settings: path, backup: None, chained: None, already: true });
    }

    let backup = backup(&path)?;
    let chain_path = cfg.data_dir.join(CHAIN_NAME);
    if let Some(cmd) = &existing_cmd {
        std::fs::write(
            &chain_path,
            format!("#!/usr/bin/env bash\n# Your previous Claude Code status line, run by Pane.\n{cmd}\n"),
        )?;
    } else {
        let _ = std::fs::remove_file(&chain_path);
    }

    let mut new_sl = json!({ "type": "command", "command": command_for(&script) });
    // Keep any extra keys the old status line had (e.g. padding).
    if let Some(Value::Object(old)) = existing {
        for (k, val) in old {
            if k != "type" && k != "command" {
                new_sl[k] = val;
            }
        }
    }
    obj.insert("statusLine".into(), new_sl);
    write_settings(&path, &v)?;
    Ok(InstallReport { settings: path, backup, chained: existing_cmd, already: false })
}

pub fn uninstall(cfg: &CollectorConfig) -> Result<Option<PathBuf>> {
    let path = settings_path(cfg);
    let mut v = read_settings(&path)?;
    let Some(obj) = v.as_object_mut() else { bail!("{} is not a JSON object", path.display()) };
    let ours = obj
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(MARKER));
    if !ours {
        return Ok(None);
    }
    let backup = backup(&path)?;
    let chain_path = cfg.data_dir.join(CHAIN_NAME);
    match state(cfg).chained_command {
        Some(cmd) => {
            obj["statusLine"]["command"] = Value::String(cmd);
        }
        None => {
            obj.remove("statusLine");
        }
    }
    let _ = std::fs::remove_file(chain_path);
    write_settings(&path, &v)?;
    Ok(backup)
}

/// Parse the latest capture, if any.
pub fn read_plan(cfg: &CollectorConfig) -> Option<PlanReading> {
    let path = cfg.statusline_capture_path();
    let meta = std::fs::metadata(&path).ok()?;
    let captured_at_ms = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
    let rl = v.get("rate_limits")?;
    let window = |k: &str| -> Option<LimitWindow> {
        let w = rl.get(k)?;
        Some(LimitWindow {
            used_percentage: w.get("used_percentage")?.as_f64()?,
            resets_at_ms: (w.get("resets_at")?.as_f64()? * 1000.0) as i64,
        })
    };
    Some(PlanReading {
        captured_at_ms,
        five_hour: window("five_hour"),
        seven_day: window("seven_day"),
        claude_version: v.get("version").and_then(Value::as_str).map(str::to_string),
        source: "statusLine".into(),
    })
}
