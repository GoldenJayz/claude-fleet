//! SQLite storage: Claude prompts/responses, log-file offsets, metrics history, kill log.

use crate::types::*;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// v2: responses record `stop_reason`, plus each session's current step. Older log tables are
/// rebuilt from the JSONL files (a couple of seconds).
const SCHEMA_VERSION: i64 = 2;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=5000;
         CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    let stored: i64 = meta_get(&conn, "schema_version")?.and_then(|v| v.parse().ok()).unwrap_or(0);
    if stored < SCHEMA_VERSION {
        conn.execute_batch(
            "DROP TABLE IF EXISTS messages; DROP TABLE IF EXISTS prompts;
             DROP TABLE IF EXISTS file_offsets; DROP TABLE IF EXISTS session_activity;",
        )?;
    }
    conn.execute_batch(
        "
         CREATE TABLE IF NOT EXISTS file_offsets(
            path TEXT PRIMARY KEY, offset INTEGER NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS prompts(
            uuid TEXT PRIMARY KEY, session_id TEXT NOT NULL, ts INTEGER NOT NULL,
            kind TEXT NOT NULL, text TEXT NOT NULL, project TEXT);
         CREATE INDEX IF NOT EXISTS prompts_session_ts ON prompts(session_id, ts);
         CREATE INDEX IF NOT EXISTS prompts_ts ON prompts(ts);
         CREATE TABLE IF NOT EXISTS messages(
            id TEXT PRIMARY KEY, session_id TEXT NOT NULL, ts INTEGER NOT NULL,
            model TEXT NOT NULL, effort TEXT, sidechain INTEGER NOT NULL,
            input INTEGER NOT NULL, output INTEGER NOT NULL,
            cache_write INTEGER NOT NULL, cache_read INTEGER NOT NULL, thinking INTEGER NOT NULL,
            stop_reason TEXT);
         CREATE TABLE IF NOT EXISTS session_activity(
            session_id TEXT PRIMARY KEY, ts INTEGER NOT NULL, kind TEXT NOT NULL,
            tool TEXT, detail TEXT, sidechain INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS messages_session_ts ON messages(session_id, ts);
         CREATE INDEX IF NOT EXISTS messages_ts ON messages(ts);
         CREATE TABLE IF NOT EXISTS metrics(
            ts INTEGER PRIMARY KEY, cpu REAL NOT NULL, mem_pct REAL NOT NULL, gpu REAL,
            disk_read INTEGER NOT NULL, disk_write INTEGER NOT NULL,
            net_rx INTEGER NOT NULL, net_tx INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS kill_log(
            id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, machine TEXT NOT NULL,
            pid INTEGER NOT NULL, name TEXT NOT NULL, tree INTEGER NOT NULL,
            requested_by TEXT NOT NULL, ok INTEGER NOT NULL, detail TEXT);",
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO meta(key, value) VALUES('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )?;
    Ok(conn)
}

pub fn meta_get(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| r.get(0))
        .optional()?)
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT OR REPLACE INTO meta(key, value) VALUES(?1, ?2)", params![key, value])?;
    Ok(())
}

// ---------- metrics ----------

pub fn insert_metrics(conn: &Connection, p: &MetricsPoint) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO metrics(ts, cpu, mem_pct, gpu, disk_read, disk_write, net_rx, net_tx)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            p.ts,
            p.cpu,
            p.mem_pct,
            p.gpu,
            p.disk_read_bps as i64,
            p.disk_write_bps as i64,
            p.net_rx_bps as i64,
            p.net_tx_bps as i64
        ],
    )?;
    Ok(())
}

/// History since `since_ms`, averaged into buckets of `bucket_ms` so charts stay light.
pub fn metrics_history(conn: &Connection, since_ms: i64, bucket_ms: i64) -> Result<Vec<MetricsPoint>> {
    let bucket = bucket_ms.max(1);
    let mut stmt = conn.prepare(
        "SELECT (ts / ?2) * ?2 AS b, AVG(cpu), AVG(mem_pct), AVG(gpu),
                AVG(disk_read), AVG(disk_write), AVG(net_rx), AVG(net_tx)
         FROM metrics WHERE ts >= ?1 GROUP BY b ORDER BY b",
    )?;
    let rows = stmt.query_map(params![since_ms, bucket], |r| {
        Ok(MetricsPoint {
            ts: r.get(0)?,
            cpu: r.get::<_, f64>(1)? as f32,
            mem_pct: r.get::<_, f64>(2)? as f32,
            gpu: r.get::<_, Option<f64>>(3)?.map(|g| g as f32),
            disk_read_bps: r.get::<_, f64>(4)? as u64,
            disk_write_bps: r.get::<_, f64>(5)? as u64,
            net_rx_bps: r.get::<_, f64>(6)? as u64,
            net_tx_bps: r.get::<_, f64>(7)? as u64,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn prune_metrics(conn: &Connection, older_than_ms: i64) -> Result<usize> {
    Ok(conn.execute("DELETE FROM metrics WHERE ts < ?1", params![older_than_ms])?)
}

// ---------- claude ----------

pub struct NewPrompt {
    pub uuid: String,
    pub session_id: String,
    pub ts: i64,
    pub kind: String,
    pub text: String,
    pub project: Option<String>,
}

pub struct NewMessage {
    pub id: String,
    pub session_id: String,
    pub ts: i64,
    pub model: String,
    pub effort: Option<String>,
    pub sidechain: bool,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub thinking: u64,
    pub stop_reason: Option<String>,
}

/// What a session did last: "thinking", "text", or "tool" (with the tool name and a short detail).
pub struct NewActivity {
    pub session_id: String,
    pub ts: i64,
    pub kind: String,
    pub tool: Option<String>,
    pub detail: Option<String>,
    pub sidechain: bool,
}

pub fn upsert_activity(conn: &Connection, a: &NewActivity) -> Result<()> {
    conn.execute(
        "INSERT INTO session_activity(session_id, ts, kind, tool, detail, sidechain) VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(session_id) DO UPDATE SET ts=excluded.ts, kind=excluded.kind, tool=excluded.tool,
            detail=excluded.detail, sidechain=excluded.sidechain
         WHERE excluded.ts >= session_activity.ts",
        params![a.session_id, a.ts, a.kind, a.tool, a.detail, a.sidechain as i64],
    )?;
    Ok(())
}

pub fn insert_prompt(conn: &Connection, p: &NewPrompt) -> Result<bool> {
    Ok(conn.execute(
        "INSERT OR IGNORE INTO prompts(uuid, session_id, ts, kind, text, project) VALUES(?1,?2,?3,?4,?5,?6)",
        params![p.uuid, p.session_id, p.ts, p.kind, p.text, p.project],
    )? > 0)
}

/// Claude Code writes one log line per content block and repeats the same usage on each,
/// so responses are keyed by `message.id` and inserted once.
pub fn insert_message(conn: &Connection, m: &NewMessage) -> Result<bool> {
    Ok(conn.execute(
        "INSERT OR IGNORE INTO messages(id, session_id, ts, model, effort, sidechain,
            input, output, cache_write, cache_read, thinking, stop_reason)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            m.id,
            m.session_id,
            m.ts,
            m.model,
            m.effort,
            m.sidechain as i64,
            m.input as i64,
            m.output as i64,
            m.cache_write as i64,
            m.cache_read as i64,
            m.thinking as i64,
            m.stop_reason
        ],
    )? > 0)
}

pub fn get_offset(conn: &Connection, path: &str) -> Result<Option<(u64, u64, i64)>> {
    Ok(conn
        .query_row(
            "SELECT offset, size, mtime FROM file_offsets WHERE path=?1",
            params![path],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as u64, r.get(2)?)),
        )
        .optional()?)
}

pub fn set_offset(conn: &Connection, path: &str, offset: u64, size: u64, mtime: i64) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO file_offsets(path, offset, size, mtime) VALUES(?1,?2,?3,?4)",
        params![path, offset as i64, size as i64, mtime],
    )?;
    Ok(())
}

pub fn files_tracked(conn: &Connection) -> Result<u64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM file_offsets", [], |r| r.get::<_, i64>(0))? as u64)
}

const SUM_COLS: &str = "COALESCE(SUM(input),0), COALESCE(SUM(output),0), COALESCE(SUM(cache_write),0),
                        COALESCE(SUM(cache_read),0), COALESCE(SUM(thinking),0), COUNT(*)";

fn totals_from_row(r: &rusqlite::Row, offset: usize) -> rusqlite::Result<TokenTotals> {
    Ok(TokenTotals {
        input: r.get::<_, i64>(offset)? as u64,
        output: r.get::<_, i64>(offset + 1)? as u64,
        cache_write: r.get::<_, i64>(offset + 2)? as u64,
        cache_read: r.get::<_, i64>(offset + 3)? as u64,
        thinking: r.get::<_, i64>(offset + 4)? as u64,
        requests: r.get::<_, i64>(offset + 5)? as u64,
        prompts: 0,
    })
}

/// Token totals for responses in `[from_ms, to_ms)`.
pub fn totals(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<TokenTotals> {
    let mut t = conn.query_row(
        &format!("SELECT {SUM_COLS} FROM messages WHERE ts >= ?1 AND ts < ?2"),
        params![from_ms, to_ms],
        |r| totals_from_row(r, 0),
    )?;
    t.prompts = conn.query_row(
        "SELECT COUNT(*) FROM prompts WHERE ts >= ?1 AND ts < ?2 AND kind != 'command'",
        params![from_ms, to_ms],
        |r| r.get::<_, i64>(0),
    )? as u64;
    Ok(t)
}

pub fn totals_by_model(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<ModelTotals>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT model, {SUM_COLS} FROM messages WHERE ts >= ?1 AND ts < ?2 GROUP BY model
         ORDER BY SUM(output) DESC"
    ))?;
    let rows = stmt.query_map(params![from_ms, to_ms], |r| {
        Ok(ModelTotals { model: r.get(0)?, tokens: totals_from_row(r, 1)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Per local day for the last `days` days (oldest first).
pub fn daily(conn: &Connection, since_ms: i64) -> Result<Vec<DailyTokens>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT date(ts / 1000, 'unixepoch', 'localtime') AS d, {SUM_COLS}
         FROM messages WHERE ts >= ?1 GROUP BY d ORDER BY d"
    ))?;
    let rows = stmt.query_map(params![since_ms], |r| {
        Ok(DailyTokens { day: r.get(0)?, tokens: totals_from_row(r, 1)? })
    })?;
    let mut out: Vec<DailyTokens> = rows.collect::<Result<_, _>>()?;
    let mut pstmt = conn.prepare(
        "SELECT date(ts / 1000, 'unixepoch', 'localtime') AS d, COUNT(*) FROM prompts
         WHERE ts >= ?1 AND kind != 'command' GROUP BY d",
    )?;
    let counts = pstmt.query_map(params![since_ms], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    for c in counts {
        let (d, n) = c?;
        if let Some(e) = out.iter_mut().find(|e| e.day == d) {
            e.tokens.prompts = n as u64;
        }
    }
    Ok(out)
}

/// Most recent prompts, newest first, each with the tokens of every response that
/// followed it in the same session until the next prompt (subagent responses included).
pub fn recent_prompts(
    conn: &Connection,
    limit: usize,
    before_ms: Option<i64>,
    now_ms: i64,
) -> Result<Vec<PromptEntry>> {
    let mut stmt = conn.prepare(
        "SELECT p.uuid, p.session_id, p.ts, p.kind, p.text, p.project,
                (SELECT MIN(p2.ts) FROM prompts p2 WHERE p2.session_id = p.session_id AND p2.ts > p.ts)
         FROM prompts p WHERE p.ts < ?1 ORDER BY p.ts DESC LIMIT ?2",
    )?;
    let rows: Vec<(String, String, i64, String, String, Option<String>, Option<i64>)> = stmt
        .query_map(params![before_ms.unwrap_or(i64::MAX), limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
        })?
        .collect::<Result<_, _>>()?;

    let mut sum_stmt = conn.prepare(&format!(
        "SELECT {SUM_COLS}, COALESCE(SUM(CASE WHEN sidechain=1 THEN input+output+cache_write+cache_read END),0),
                MAX(ts)
         FROM messages WHERE session_id=?1 AND ts >= ?2 AND ts < ?3"
    ))?;
    let mut first_stmt = conn.prepare(
        "SELECT model, effort FROM messages WHERE session_id=?1 AND ts >= ?2 AND ts < ?3
         ORDER BY sidechain ASC, ts ASC LIMIT 1",
    )?;

    let mut out = Vec::with_capacity(rows.len());
    for (uuid, session_id, ts, kind, text, project, next_ts) in rows {
        let end = next_ts.unwrap_or(i64::MAX);
        let (tokens, sub, last_ts) = sum_stmt.query_row(params![session_id, ts, end], |r| {
            Ok((totals_from_row(r, 0)?, r.get::<_, i64>(6)? as u64, r.get::<_, Option<i64>>(7)?))
        })?;
        let first: Option<(String, Option<String>)> = first_stmt
            .query_row(params![session_id, ts, end], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let status = if next_ts.is_some() { "done" } else { prompt_status(conn, &session_id, ts, now_ms)? };
        let activity = if status == "working" { session_activity(conn, &session_id)? } else { None };
        out.push(PromptEntry {
            id: uuid,
            session_id,
            ts,
            kind,
            text,
            project,
            model: first.as_ref().map(|f| f.0.clone()),
            effort: first.and_then(|f| f.1),
            effort_is_default: false,
            tokens,
            subagent_tokens: sub,
            active: status == "working",
            status: status.to_string(),
            activity,
            last_activity_ts: last_ts,
        });
    }
    Ok(out)
}

/// A session is still working on its latest prompt when its last main-thread response asked for
/// a tool (`stop_reason: tool_use`, or no response yet) and its log files changed in the last
/// 10 minutes. `end_turn` means finished; silence after `tool_use` means interrupted or abandoned.
const WORKING_WINDOW_MS: i64 = 10 * 60_000;

fn prompt_status(conn: &Connection, session_id: &str, prompt_ts: i64, now_ms: i64) -> Result<&'static str> {
    let last_stop: Option<Option<String>> = conn
        .query_row(
            "SELECT stop_reason FROM messages WHERE session_id=?1 AND ts >= ?2 AND sidechain=0
             ORDER BY ts DESC LIMIT 1",
            params![session_id, prompt_ts],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(Some(stop)) = &last_stop {
        if matches!(stop.as_str(), "end_turn" | "stop_sequence" | "max_tokens" | "refusal") {
            return Ok("done");
        }
    }
    let pattern = format!("%{session_id}%");
    let touched: Option<i64> =
        conn.query_row("SELECT MAX(mtime) FROM file_offsets WHERE path LIKE ?1", params![pattern], |r| r.get(0))?;
    let last = touched.unwrap_or(prompt_ts).max(prompt_ts);
    Ok(if now_ms - last < WORKING_WINDOW_MS { "working" } else { "stopped" })
}

fn session_activity(conn: &Connection, session_id: &str) -> Result<Option<Activity>> {
    Ok(conn
        .query_row(
            "SELECT ts, kind, tool, detail, sidechain FROM session_activity WHERE session_id=?1",
            params![session_id],
            |r| {
                Ok(Activity {
                    ts: r.get(0)?,
                    kind: r.get(1)?,
                    tool: r.get(2)?,
                    detail: r.get(3)?,
                    in_subagent: r.get::<_, i64>(4)? != 0,
                })
            },
        )
        .optional()?)
}

/// The latest prompt of every session that is still working, newest first.
pub fn working_prompts(conn: &Connection, now_ms: i64) -> Result<Vec<PromptEntry>> {
    let since = now_ms - WORKING_WINDOW_MS;
    let mut stmt = conn.prepare(
        "SELECT MAX(p.ts) FROM prompts p
         WHERE p.session_id IN (SELECT session_id FROM messages WHERE ts >= ?1
                                UNION SELECT session_id FROM prompts WHERE ts >= ?1)
         GROUP BY p.session_id",
    )?;
    let latest: Vec<i64> = stmt.query_map(params![since], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for ts in latest {
        // The prompt at exactly `ts`, with full details.
        for p in recent_prompts(conn, 1, Some(ts + 1), now_ms)? {
            if p.status == "working" && p.ts == ts {
                out.push(p);
            }
        }
    }
    out.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(out)
}

pub fn insert_kill(conn: &Connection, e: &KillLogEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO kill_log(ts, machine, pid, name, tree, requested_by, ok, detail) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![e.ts, e.machine, e.pid, e.name, e.tree as i64, e.requested_by, e.ok as i64, e.detail],
    )?;
    Ok(())
}

pub fn list_kills(conn: &Connection, limit: usize) -> Result<Vec<KillLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, machine, pid, name, tree, requested_by, ok, detail FROM kill_log ORDER BY ts DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |r| {
        Ok(KillLogEntry {
            id: r.get(0)?,
            ts: r.get(1)?,
            machine: r.get(2)?,
            pid: r.get::<_, i64>(3)? as u32,
            name: r.get(4)?,
            tree: r.get::<_, i64>(5)? != 0,
            requested_by: r.get(6)?,
            ok: r.get::<_, i64>(7)? != 0,
            detail: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn last_activity(conn: &Connection) -> Result<Option<i64>> {
    Ok(conn.query_row("SELECT MAX(ts) FROM messages", [], |r| r.get(0))?)
}
