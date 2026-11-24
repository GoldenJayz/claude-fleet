//! Plan usage straight from Anthropic: the same numbers Claude Code's `/usage` shows.
//!
//! The status line only reports usage when Claude Code runs in a terminal, so desktop-app
//! sessions left the banner stale. This asks Anthropic directly every few minutes, using the
//! sign-in Claude Code already saved on this PC (`~/.claude/.credentials.json`).
//!
//! - Only the usage endpoint is called. No prompts, logs or other data are sent.
//! - The token is only read, never refreshed or written by us. When it's about to expire we run
//!   `claude auth status`, which makes Claude Code renew its own sign-in the normal way; doing
//!   it ourselves could log the terminal Claude Code out.
//! - Any failure just means the status-line reading (if any) is shown instead.

use crate::config::CollectorConfig;
use crate::types::{LimitWindow, PlanReading};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const POLL_EVERY: Duration = Duration::from_secs(300);
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Renew the sign-in when it has less than this left.
const RENEW_MARGIN_MS: i64 = 10 * 60 * 1000;
/// Don't start `claude auth status` more often than this.
const RENEW_COOLDOWN: Duration = Duration::from_secs(30 * 60);

static LAST_RENEW: Mutex<Option<Instant>> = Mutex::new(None);

struct Creds {
    token: String,
    expires_at_ms: Option<i64>,
}

fn read_creds(cfg: &CollectorConfig) -> Result<Creds, String> {
    let path = cfg.claude_dir.join(".credentials.json");
    let text = std::fs::read_to_string(&path).map_err(|_| "Claude Code isn't signed in on this PC".to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "couldn't read Claude Code's sign-in".to_string())?;
    let o = v.get("claudeAiOauth").ok_or("Claude Code isn't signed in with a Claude account")?;
    let token = o.get("accessToken").and_then(Value::as_str).filter(|t| !t.is_empty()).ok_or("Claude Code isn't signed in")?;
    Ok(Creds { token: token.to_string(), expires_at_ms: o.get("expiresAt").and_then(Value::as_i64) })
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Let Claude Code renew its own sign-in (`claude auth status` does that without using the model).
fn renew_via_claude() -> bool {
    {
        let mut last = LAST_RENEW.lock().unwrap();
        if last.is_some_and(|t| t.elapsed() < RENEW_COOLDOWN) {
            return false;
        }
        *last = Some(Instant::now());
    }
    // `claude` is usually an npm .cmd shim, so go through cmd.exe.
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd.exe");
        c.args(["/D", "/C", "claude", "auth", "status"]);
        c
    } else {
        let mut c = std::process::Command::new("claude");
        c.args(["auth", "status"]);
        c
    };
    cmd.env("DISABLE_AUTOUPDATER", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(mut child) = cmd.spawn() else { return false };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if started.elapsed() < Duration::from_secs(60) => std::thread::sleep(Duration::from_millis(250)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

fn window(v: &Value, key: &str) -> Option<LimitWindow> {
    let w = v.get(key)?;
    let used = w.get("utilization")?.as_f64()?;
    let resets = w.get("resets_at")?.as_str()?;
    let resets_at_ms = chrono::DateTime::parse_from_rfc3339(resets).ok()?.timestamp_millis();
    Some(LimitWindow { used_percentage: used, resets_at_ms })
}

/// Parse the usage endpoint's reply.
pub fn parse(v: &Value, captured_at_ms: i64) -> Option<PlanReading> {
    let five_hour = window(v, "five_hour");
    let seven_day = window(v, "seven_day");
    if five_hour.is_none() && seven_day.is_none() {
        return None;
    }
    Some(PlanReading { captured_at_ms, five_hour, seven_day, claude_version: None, source: "anthropic".into() })
}

/// Blocking: read the sign-in (renewing it through Claude Code if needed) and fetch usage.
pub fn fetch(cfg: &CollectorConfig) -> Result<PlanReading, String> {
    let mut creds = read_creds(cfg)?;
    if creds.expires_at_ms.is_some_and(|e| e - now_ms() < RENEW_MARGIN_MS) {
        if renew_via_claude() {
            creds = read_creds(cfg)?;
        }
        if creds.expires_at_ms.is_some_and(|e| e <= now_ms()) {
            return Err("Claude Code's sign-in has expired; run `claude` once in a terminal".into());
        }
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false).build(),
    );
    let mut resp = agent
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {}", creds.token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .header("User-Agent", concat!("claude-fleet/", env!("CARGO_PKG_VERSION")))
        .call()
        .map_err(|e| format!("couldn't reach Anthropic: {e}"))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(match status {
            401 | 403 => "Anthropic refused Claude Code's sign-in".into(),
            429 => "Anthropic asked us to slow down".into(),
            s => format!("Anthropic returned HTTP {s}"),
        });
    }
    let v: Value = resp.body_mut().read_json().map_err(|e| format!("unexpected reply: {e}"))?;
    parse(&v, now_ms()).ok_or_else(|| "no plan limits in the reply".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reply() {
        let v: Value = serde_json::from_str(
            r#"{"five_hour":{"utilization":16.0,"resets_at":"2026-11-24T05:30:00.522979+00:00"},
                "seven_day":{"utilization":55.0,"resets_at":"2026-11-28T12:00:00.522999+00:00"},
                "seven_day_opus":null}"#,
        )
        .unwrap();
        let p = parse(&v, 1).unwrap();
        assert_eq!(p.five_hour.as_ref().unwrap().used_percentage, 16.0);
        assert_eq!(p.five_hour.unwrap().resets_at_ms, 1_795_498_200_522);
        assert_eq!(p.seven_day.unwrap().used_percentage, 55.0);
        assert_eq!(p.source, "anthropic");
        assert!(parse(&serde_json::json!({"five_hour": null}), 1).is_none());
    }
}
