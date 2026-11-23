//! Incremental reader for Claude Code's JSONL logs in `<claude_dir>/projects/**.jsonl`.
//!
//! Field locations were checked against real logs (Claude Code 2.1.3xx), see docs/data-sources.md:
//! - `type:"assistant"`: `message.model`, `message.id`, `message.usage.*`,
//!   `effort` / `perTurnEffort` (top level), `isSidechain`.
//!   One line per content block, all repeating the same usage → dedupe on `message.id`.
//! - `type:"user"`: typed prompts have `origin.kind:"human"` (older: `turnOrigin:"human"`);
//!   skip `isMeta`, `isSidechain`, tool results and injected `<…>` envelopes.
//! - Subagent logs live in `<session>/subagents/agent-*.jsonl` with the parent's `sessionId`.

use crate::db::{self, NewMessage, NewPrompt};
use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const MAX_PROMPT_CHARS: usize = 4000;

/// Envelopes Claude Code injects as "user" turns; never shown as prompts.
const INJECTED_TAGS: &[&str] = &[
    "local-command-stdout",
    "local-command-stderr",
    "local-command-caveat",
    "ci-monitor-event",
    "task-notification",
    "side-sessions-event",
    "user-prompt-submit-hook",
    "bash-stdout",
    "bash-stderr",
    "system-reminder",
];

#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    pub files_seen: usize,
    pub files_read: usize,
    pub bytes_read: u64,
    pub new_prompts: usize,
    pub new_messages: usize,
    pub bad_lines: usize,
}

impl ScanStats {
    pub fn changed(&self) -> bool {
        self.new_prompts > 0 || self.new_messages > 0
    }
}

fn find_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => find_jsonl(&p, out),
            Ok(t) if t.is_file() && p.extension().is_some_and(|x| x == "jsonl") => out.push(p),
            _ => {}
        }
    }
}

/// Read everything new since the last scan. Safe to call every few seconds.
pub fn scan(conn: &mut Connection, claude_dir: &Path) -> Result<ScanStats> {
    let mut files = Vec::new();
    find_jsonl(&claude_dir.join("projects"), &mut files);
    let mut stats = ScanStats { files_seen: files.len(), ..Default::default() };

    for path in files {
        if let Err(e) = scan_file(conn, &path, &mut stats) {
            tracing::warn!("reading {}: {e:#}", path.display());
        }
    }
    Ok(stats)
}

fn scan_file(conn: &mut Connection, path: &Path, stats: &mut ScanStats) -> Result<()> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let key = path.to_string_lossy().into_owned();

    let mut offset = match db::get_offset(conn, &key)? {
        Some((off, _, _)) if off == size => return Ok(()),
        Some((off, _, _)) if off < size => off,
        // Shrunk (rewritten/compacted) or never seen: read from the start. Inserts are idempotent.
        _ => 0,
    };

    let mut f = std::fs::File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::with_capacity((size - offset) as usize);
    f.take(size - offset).read_to_end(&mut buf)?;
    // Only consume complete lines; a half-written last line is picked up next time.
    let Some(last_nl) = buf.iter().rposition(|&b| b == b'\n') else { return Ok(()) };
    let complete = &buf[..=last_nl];

    stats.files_read += 1;
    stats.bytes_read += complete.len() as u64;
    let is_subagent_file = path.components().any(|c| c.as_os_str() == "subagents");

    let tx = conn.transaction()?;
    for line in complete.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        // Cheap pre-filter: only user/assistant lines matter, and most bytes are other types.
        let quick = std::str::from_utf8(line).unwrap_or("");
        if !(quick.contains("\"assistant\"") || quick.contains("\"user\"")) {
            continue;
        }
        let v: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(_) => {
                stats.bad_lines += 1;
                continue;
            }
        };
        match v.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                if let Some(m) = parse_assistant(&v, is_subagent_file) {
                    if let Some(a) = parse_activity(&v, &m) {
                        db::upsert_activity(&tx, &a)?;
                    }
                    if db::insert_message(&tx, &m)? {
                        stats.new_messages += 1;
                    }
                }
            }
            Some("user") => {
                if let Some(p) = parse_prompt(&v) {
                    if db::insert_prompt(&tx, &p)? {
                        stats.new_prompts += 1;
                    }
                }
            }
            _ => {}
        }
    }
    offset += complete.len() as u64;
    db::set_offset(&tx, &key, offset, size, mtime)?;
    tx.commit()?;
    Ok(())
}

fn ts_ms(v: &Value) -> Option<i64> {
    let s = v.get("timestamp")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

fn u(v: &Value, path: &[&str]) -> u64 {
    let mut cur = v;
    for p in path {
        match cur.get(p) {
            Some(n) => cur = n,
            None => return 0,
        }
    }
    cur.as_u64().unwrap_or(0)
}

pub fn parse_assistant(v: &Value, is_subagent_file: bool) -> Option<NewMessage> {
    let msg = v.get("message")?;
    let model = msg.get("model")?.as_str()?;
    if model.starts_with('<') {
        return None; // "<synthetic>" placeholders are not API calls
    }
    let usage = msg.get("usage")?;
    let id = msg
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| v.get("requestId").and_then(Value::as_str))
        .or_else(|| v.get("uuid").and_then(Value::as_str))?;
    let effort = v
        .get("effort")
        .or_else(|| v.get("perTurnEffort"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(NewMessage {
        id: id.to_string(),
        session_id: v.get("sessionId")?.as_str()?.to_string(),
        ts: ts_ms(v)?,
        model: model.to_string(),
        effort,
        sidechain: is_subagent_file || v.get("isSidechain").and_then(Value::as_bool).unwrap_or(false),
        input: u(usage, &["input_tokens"]),
        output: u(usage, &["output_tokens"]),
        cache_write: u(usage, &["cache_creation_input_tokens"]),
        cache_read: u(usage, &["cache_read_input_tokens"]),
        thinking: u(usage, &["output_tokens_details", "thinking_tokens"]),
        stop_reason: msg.get("stop_reason").and_then(Value::as_str).map(str::to_string),
    })
}

fn short(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > n { s.chars().take(n).collect::<String>() + "…" } else { s }
}

fn file_name(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_string()
}

/// One line = one content block. Record what it was, for the "working now" view.
pub fn parse_activity(v: &Value, m: &NewMessage) -> Option<db::NewActivity> {
    let block = v.pointer("/message/content")?.as_array()?.last()?;
    let kind = block.get("type")?.as_str()?;
    let (kind, tool, detail) = match kind {
        "tool_use" => {
            let name = block.get("name").and_then(Value::as_str).unwrap_or("tool").to_string();
            let input = block.get("input").cloned().unwrap_or(Value::Null);
            let s = |k: &str| input.get(k).and_then(Value::as_str);
            let detail = match name.as_str() {
                "Bash" | "PowerShell" => s("description").map(|d| short(d, 80)).or_else(|| s("command").map(|c| short(c, 60))),
                "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => s("file_path").or(s("notebook_path")).map(file_name),
                "Grep" | "Glob" => s("pattern").map(|p| short(p, 60)),
                "WebSearch" => s("query").map(|q| short(q, 60)),
                "WebFetch" => s("url").map(|u| short(u, 60)),
                "Agent" | "Task" => s("description").map(|d| short(d, 60)),
                _ => None,
            };
            ("tool", Some(name), detail)
        }
        "thinking" | "redacted_thinking" => ("thinking", None, None),
        "text" => ("text", None, None),
        _ => return None,
    };
    Some(db::NewActivity {
        session_id: m.session_id.clone(),
        ts: m.ts,
        kind: kind.to_string(),
        tool,
        detail,
        sidechain: m.sidechain,
    })
}

/// Remove `<tag …>…</tag>` blocks for the given tag.
fn strip_blocks(s: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(&open) {
        out.push_str(&rest[..start]);
        match rest[start..].find(&close) {
            Some(end) => rest = &rest[start + end + close.len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Inner text of the first `<tag>…</tag>`.
fn tag_text<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = s.find(&open)? + open.len();
    let end = s[start..].find(&close)? + start;
    Some(s[start..end].trim())
}

/// Drop `<pasted_content …>` wrapper tags but keep what was pasted.
fn unwrap_pasted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        let next = [rest.find("<pasted_content"), rest.find("</pasted_content")]
            .into_iter()
            .flatten()
            .min();
        match next {
            Some(i) => {
                out.push_str(&rest[..i]);
                match rest[i..].find('>') {
                    Some(j) => rest = &rest[i + j + 1..],
                    None => {
                        rest = "";
                    }
                }
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

pub fn parse_prompt(v: &Value) -> Option<NewPrompt> {
    if v.get("isMeta").and_then(Value::as_bool) == Some(true)
        || v.get("isSidechain").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    // Newer logs say who started the turn; anything but a human is injected.
    if let Some(kind) = v.pointer("/origin/kind").and_then(Value::as_str) {
        if kind != "human" {
            return None;
        }
    } else if let Some(t) = v.get("turnOrigin").and_then(Value::as_str) {
        if t != "human" {
            return None;
        }
    }

    let content = v.pointer("/message/content")?;
    let mut text = String::new();
    let mut images = 0;
    match content {
        Value::String(s) => text.push_str(s),
        Value::Array(parts) => {
            for p in parts {
                match p.get("type").and_then(Value::as_str) {
                    Some("tool_result") => return None,
                    Some("text") => {
                        if let Some(t) = p.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(t);
                        }
                    }
                    Some("image") => images += 1,
                    _ => {}
                }
            }
        }
        _ => return None,
    }

    let stripped = strip_blocks(&text, "system-reminder");
    let trimmed = stripped.trim();
    let (kind, mut shown) = if let Some(name) = tag_text(trimmed, "command-name") {
        let args = tag_text(trimmed, "command-args").unwrap_or("");
        let name = if name.starts_with('/') { name.to_string() } else { format!("/{name}") };
        ("command", format!("{name} {args}").trim().to_string())
    } else if let Some(cmd) = tag_text(trimmed, "bash-input") {
        ("bash", format!("! {cmd}"))
    } else {
        if let Some(tag) = trimmed.strip_prefix('<') {
            let name: String = tag.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
            if INJECTED_TAGS.contains(&name.as_str()) {
                return None;
            }
        }
        ("prompt", unwrap_pasted(trimmed).trim().to_string())
    };
    if images > 0 {
        let tag = if images == 1 { "[image]".to_string() } else { format!("[{images} images]") };
        shown = if shown.is_empty() { tag } else { format!("{tag} {shown}") };
    }
    if shown.is_empty() {
        return None;
    }
    if shown.chars().count() > MAX_PROMPT_CHARS {
        shown = shown.chars().take(MAX_PROMPT_CHARS).collect::<String>() + "…";
    }

    let project = v
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(|c| Path::new(c).file_name().map(|n| n.to_string_lossy().into_owned()));

    Some(NewPrompt {
        uuid: v.get("uuid")?.as_str()?.to_string(),
        session_id: v.get("sessionId")?.as_str()?.to_string(),
        ts: ts_ms(v)?,
        kind: kind.to_string(),
        text: shown,
        project,
    })
}

/// Default effort from `settings.json` (`effortLevel`), with per-model overrides under
/// `modelSettings.<model>.effortLevel` when present.
pub fn default_effort(claude_dir: &Path, model: Option<&str>) -> Option<String> {
    let s = std::fs::read_to_string(claude_dir.join("settings.json")).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    if let (Some(m), Some(ms)) = (model, v.get("modelSettings").and_then(Value::as_object)) {
        for (k, cfg) in ms {
            if m.contains(k.as_str()) {
                if let Some(e) = cfg.get("effortLevel").or_else(|| cfg.get("effort")).and_then(Value::as_str) {
                    return Some(e.to_string());
                }
            }
        }
    }
    v.get("effortLevel").and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn human_prompt_with_reminder() {
        let v = json!({"type":"user","uuid":"u1","sessionId":"s","timestamp":"2026-11-23T10:00:00Z",
            "origin":{"kind":"human"},"cwd":"C:\\x\\proj",
            "message":{"role":"user","content":"<system-reminder>ignore</system-reminder>Fix the bug"}});
        let p = parse_prompt(&v).unwrap();
        assert_eq!(p.text, "Fix the bug");
        assert_eq!(p.kind, "prompt");
        assert_eq!(p.project.as_deref(), Some("proj"));
    }

    #[test]
    fn skips_injected() {
        for v in [
            json!({"type":"user","uuid":"u","sessionId":"s","timestamp":"2026-11-23T10:00:00Z","isMeta":true,"message":{"content":"x"}}),
            json!({"type":"user","uuid":"u","sessionId":"s","timestamp":"2026-11-23T10:00:00Z","origin":{"kind":"peer"},"message":{"content":"x"}}),
            json!({"type":"user","uuid":"u","sessionId":"s","timestamp":"2026-11-23T10:00:00Z","message":{"content":"<local-command-stdout>hi</local-command-stdout>"}}),
            json!({"type":"user","uuid":"u","sessionId":"s","timestamp":"2026-11-23T10:00:00Z","message":{"content":[{"type":"tool_result","content":"x"}]}}),
        ] {
            assert!(parse_prompt(&v).is_none(), "{v}");
        }
    }

    #[test]
    fn slash_command() {
        let v = json!({"type":"user","uuid":"u","sessionId":"s","timestamp":"2026-11-23T10:00:00Z",
            "message":{"content":"<command-name>/model</command-name>\n<command-message>model</command-message>\n<command-args>opus</command-args>"}});
        let p = parse_prompt(&v).unwrap();
        assert_eq!((p.kind.as_str(), p.text.as_str()), ("command", "/model opus"));
    }

    #[test]
    fn assistant_usage() {
        let v = json!({"type":"assistant","sessionId":"s","timestamp":"2026-11-23T10:00:01Z","effort":"high",
            "message":{"id":"msg_1","model":"claude-opus-5-5","usage":{"input_tokens":2,"output_tokens":823,
            "cache_creation_input_tokens":28007,"cache_read_input_tokens":45803,"output_tokens_details":{"thinking_tokens":396}}}});
        let m = parse_assistant(&v, false).unwrap();
        assert_eq!((m.input, m.output, m.cache_write, m.cache_read, m.thinking), (2, 823, 28007, 45803, 396));
        assert_eq!(m.effort.as_deref(), Some("high"));
    }
}
