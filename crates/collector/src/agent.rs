//! Agent tasks: another device hands this PC a prompt, and Claude Code works on it here.
//!
//! Each task runs `claude -p` (headless) in a folder on this PC, as the signed-in user, with the
//! user's own Claude Code sign-in. Progress streams to every dashboard; you can follow up on a
//! finished task (it resumes the same Claude session) or cancel it.
//!
//! Safety:
//! - Off unless turned on from this PC (`agent_access` in collector.json): "edits" allows the
//!   read-only, edit-files and auto permission modes; "full" also allows bypassing permissions.
//! - The prompt goes to Claude Code on stdin, never on a command line, so it can't inject commands.
//!   Every other argument is checked against a fixed list (or is a session UUID).
//! - One task runs at a time; the rest wait in a queue. Everything is logged in `agent_tasks`.

use crate::db;
use crate::state::{Shared, SharedState};
use crate::types::{AgentStatus, AgentStep, AgentTask, StreamEvent};
use serde_json::Value;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

/// Permission modes a task may use, cheapest-to-trust first.
pub const MODES: &[&str] = &["plan", "acceptEdits", "auto", "bypassPermissions"];
pub const MODELS: &[&str] = &["opus", "sonnet", "haiku", "fable"];
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

const MAX_STEPS: usize = 600;
const MAX_STEP_CHARS: usize = 4000;
const MAX_PROMPT_CHARS: usize = 100_000;

struct Running {
    task: AgentTask,
    pid: Option<u32>,
    cancel: bool,
}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<AgentTask>,
    running: Option<Running>,
}

static QUEUE: Mutex<Option<Queue>> = Mutex::new(None);
static WAKE: Condvar = Condvar::new();

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

/// What this PC allows: "off", "edits" or "full".
pub fn access(st: &Shared) -> String {
    st.cfg.agent_access()
}

pub fn mode_allowed(access: &str, mode: &str) -> bool {
    match access {
        "full" => MODES.contains(&mode),
        "edits" => MODES.contains(&mode) && mode != "bypassPermissions",
        _ => false,
    }
}

/// Start the worker that runs queued tasks one at a time.
pub fn start(st: SharedState) {
    *QUEUE.lock().unwrap() = Some(Queue::default());
    std::thread::Builder::new()
        .name("agent-worker".into())
        .spawn(move || loop {
            let task = {
                let mut g = QUEUE.lock().unwrap();
                loop {
                    let q = g.as_mut().unwrap();
                    if let Some(t) = q.waiting.pop_front() {
                        q.running = Some(Running { task: t.clone(), pid: None, cancel: false });
                        break t;
                    }
                    g = WAKE.wait(g).unwrap();
                }
            };
            run(&st, task);
            QUEUE.lock().unwrap().as_mut().unwrap().running = None;
        })
        .expect("starting agent worker");
}

pub struct NewTask {
    pub prompt: String,
    pub cwd: String,
    pub mode: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Continue the conversation of this earlier task (which may still be queued or running:
    /// the follow-up then waits its turn and picks up the session once it exists).
    pub follow_up_of: Option<i64>,
    pub from: String,
    pub ip: String,
}

/// Validate and queue a task. Errors are (http status, message).
pub fn submit(st: &Shared, n: NewTask) -> Result<AgentTask, (u16, String)> {
    let access = access(st);
    if access == "off" {
        return Err((403, format!("Agent tasks are turned off on {}. Turn them on in that PC's Settings → System.", st.cfg.machine_name)));
    }
    if !MODES.contains(&n.mode.as_str()) {
        return Err((400, format!("unknown permission mode '{}'", n.mode)));
    }
    if !mode_allowed(&access, &n.mode) {
        return Err((403, format!("{} only allows tasks without full access. Turn on full access in that PC's Settings → System.", st.cfg.machine_name)));
    }
    if n.model.as_deref().is_some_and(|m| !MODELS.contains(&m)) {
        return Err((400, "unknown model".into()));
    }
    if n.effort.as_deref().is_some_and(|e| !EFFORTS.contains(&e)) {
        return Err((400, "unknown effort level".into()));
    }
    // A follow-up runs in its parent's folder (Claude keeps sessions per folder).
    let parent = match n.follow_up_of {
        Some(id) => Some(get(st, id).ok_or((404, "the task to follow up on doesn't exist".to_string()))?),
        None => None,
    };
    let prompt = n.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err((400, "the task is empty".into()));
    }
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err((400, "the task is too long".into()));
    }
    let cwd = PathBuf::from(parent.as_ref().map(|p| p.cwd.clone()).unwrap_or(n.cwd).trim());
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err((400, format!("{} has no folder '{}'", st.cfg.machine_name, cwd.display())));
    }
    let mut task = AgentTask {
        id: 0,
        created: now_ms(),
        started: None,
        ended: None,
        from: n.from,
        ip: n.ip,
        prompt,
        cwd: cwd.display().to_string(),
        mode: n.mode,
        model: n.model,
        effort: n.effort,
        resume: parent.as_ref().and_then(|p| p.session_id.clone()),
        follow_up_of: n.follow_up_of,
        status: "queued".into(),
        session_id: None,
        result: None,
        error: None,
        cost_usd: None,
        turns: None,
        step_count: 0,
        steps: vec![],
    };
    task.id = db::insert_agent_task(&st.db.lock().unwrap(), &task).map_err(|e| (500, format!("{e:#}")))?;
    tracing::info!("agent task {} queued by {} ({}) in {}", task.id, task.from, task.ip, task.cwd);
    QUEUE.lock().unwrap().as_mut().ok_or((503, "agent not started".to_string()))?.waiting.push_back(task.clone());
    WAKE.notify_one();
    st.emit(&StreamEvent::Task(task.clone()));
    Ok(task)
}

/// Cancel a queued or running task.
pub fn cancel(st: &Shared, id: i64) -> Result<(), String> {
    let mut g = QUEUE.lock().unwrap();
    let q = g.as_mut().ok_or("agent not started")?;
    if let Some(pos) = q.waiting.iter().position(|t| t.id == id) {
        let mut t = q.waiting.remove(pos).unwrap();
        drop(g);
        t.status = "cancelled".into();
        t.ended = Some(now_ms());
        save_and_emit(st, &t);
        return Ok(());
    }
    if let Some(r) = q.running.as_mut().filter(|r| r.task.id == id) {
        r.cancel = true;
        if let Some(pid) = r.pid {
            kill_tree(pid);
        }
        return Ok(());
    }
    Err("that task isn't queued or running".into())
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}

fn kill_tree(pid: u32) {
    let mut c = Command::new("taskkill");
    c.args(["/T", "/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null());
    no_window(&mut c);
    let _ = c.status();
}

fn no_window(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    let _ = c;
}

/// The task with all its steps: the running one from memory, others from the log.
pub fn get(st: &Shared, id: i64) -> Option<AgentTask> {
    {
        let g = QUEUE.lock().unwrap();
        if let Some(q) = g.as_ref() {
            if let Some(r) = q.running.as_ref().filter(|r| r.task.id == id) {
                return Some(r.task.clone());
            }
            if let Some(t) = q.waiting.iter().find(|t| t.id == id) {
                return Some(t.clone());
            }
        }
    }
    db::get_agent_task(&st.db.lock().unwrap(), id).ok().flatten()
}

/// Recent tasks without their steps (for lists and the snapshot).
pub fn recent(st: &Shared, limit: usize) -> Vec<AgentTask> {
    let mut list = db::list_agent_tasks(&st.db.lock().unwrap(), limit).unwrap_or_default();
    // The running task's live state (status, step count) is fresher than the log's.
    if let Some(r) = QUEUE.lock().unwrap().as_ref().and_then(|q| q.running.as_ref()) {
        if let Some(t) = list.iter_mut().find(|t| t.id == r.task.id) {
            *t = summary(&r.task);
        }
    }
    list
}

pub fn status(st: &Shared) -> AgentStatus {
    let g = QUEUE.lock().unwrap();
    let q = g.as_ref();
    AgentStatus {
        access: access(st),
        machine: st.cfg.machine_name.clone(),
        running: q.and_then(|q| q.running.as_ref()).map(|r| r.task.id),
        queued: q.map(|q| q.waiting.len()).unwrap_or(0),
        folders: recent_folders(&st.cfg.claude_dir, 15),
        home: dirs_home().map(|p| p.display().to_string()),
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// Folders Claude Code was recently used in on this PC (newest first), from its project logs.
pub fn recent_folders(claude_dir: &Path, limit: usize) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(claude_dir.join("projects")) else { return vec![] };
    let mut found: Vec<(std::time::SystemTime, String)> = vec![];
    for dir in rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let newest = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
            .max_by_key(|(t, _)| *t);
        let Some((mtime, file)) = newest else { continue };
        let Ok(f) = std::fs::File::open(&file) else { continue };
        let cwd = BufReader::new(f)
            .lines()
            .take(40)
            .map_while(Result::ok)
            .find_map(|l| serde_json::from_str::<Value>(&l).ok()?.get("cwd")?.as_str().map(str::to_string));
        if let Some(c) = cwd.filter(|c| Path::new(c).is_dir()) {
            if !found.iter().any(|(_, x)| x.eq_ignore_ascii_case(&c)) {
                found.push((mtime, c));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().take(limit).map(|(_, c)| c).collect()
}

fn summary(t: &AgentTask) -> AgentTask {
    AgentTask { step_count: t.steps.len(), steps: vec![], ..t.clone() }
}

fn save_and_emit(st: &Shared, t: &AgentTask) {
    if let Err(e) = db::update_agent_task(&st.db.lock().unwrap(), t) {
        tracing::warn!("couldn't save agent task {}: {e:#}", t.id);
    }
    st.emit(&StreamEvent::Task(summary(t)));
}

/// Apply a change to the running task (and tell dashboards about new steps).
fn with_running(st: &Shared, f: impl FnOnce(&mut AgentTask) -> Option<AgentStep>) {
    let (step, id) = {
        let mut g = QUEUE.lock().unwrap();
        let Some(r) = g.as_mut().and_then(|q| q.running.as_mut()) else { return };
        let step = f(&mut r.task);
        if let Some(s) = &step {
            if r.task.steps.len() < MAX_STEPS {
                r.task.steps.push(s.clone());
            }
        }
        (step, r.task.id)
    };
    if let Some(step) = step {
        st.emit(&StreamEvent::TaskStep { task_id: id, step });
    }
}

fn step(kind: &str, text: impl Into<String>) -> AgentStep {
    AgentStep { ts: now_ms(), kind: kind.into(), text: clip(&text.into(), MAX_STEP_CHARS) }
}

/// One line describing a tool call, e.g. "Bash: cargo test" or "Edit: src/main.rs".
fn describe_tool(name: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str);
    let detail = match name {
        "Bash" | "PowerShell" => s("command").map(str::to_string),
        "Read" | "Edit" | "Write" | "NotebookEdit" | "MultiEdit" => s("file_path").or(s("notebook_path")).map(str::to_string),
        "Grep" | "Glob" => s("pattern").map(|p| match s("path") {
            Some(path) => format!("{p} in {path}"),
            None => p.to_string(),
        }),
        "WebFetch" => s("url").map(str::to_string),
        "WebSearch" => s("query").map(str::to_string),
        "Task" | "Agent" => s("description").map(str::to_string),
        "TodoWrite" => Some("updated the to-do list".into()),
        _ => None,
    }
    .unwrap_or_else(|| clip(&input.to_string(), 200));
    format!("{name}: {}", clip(&detail, 600))
}

fn run(st: &Shared, mut task: AgentTask) {
    // A follow-up queued while its parent was still working: use the parent's session now.
    if let (Some(parent), None) = (task.follow_up_of, &task.resume) {
        match get(st, parent).and_then(|p| p.session_id) {
            Some(sid) if is_uuid(&sid) => {
                task.resume = Some(sid.clone());
                with_running(st, |t| {
                    t.resume = Some(sid);
                    None
                });
            }
            _ => {
                with_running(st, |t| {
                    t.status = "failed".into();
                    t.started = Some(now_ms());
                    t.ended = Some(now_ms());
                    t.error = Some("The earlier task never started a Claude session, so there's nothing to continue.".into());
                    None
                });
                if let Some(t) = get(st, task.id) {
                    save_and_emit(st, &t);
                }
                return;
            }
        }
    }
    with_running(st, |t| {
        t.status = "running".into();
        t.started = Some(now_ms());
        None
    });
    if let Some(t) = get(st, task.id) {
        save_and_emit(st, &t);
    }

    let mut args: Vec<String> = ["/D", "/C", "claude", "-p", "--output-format", "stream-json", "--verbose", "--permission-mode"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(task.mode.clone());
    if let Some(m) = &task.model {
        args.extend(["--model".into(), m.clone()]);
    }
    if let Some(e) = &task.effort {
        args.extend(["--effort".into(), e.clone()]);
    }
    if let Some(r) = task.resume.as_ref().filter(|r| is_uuid(r)) {
        args.extend(["--resume".into(), r.clone()]);
    }
    // `claude` is usually an npm .cmd shim, so it goes through cmd.exe. Only fixed words and
    // checked values are on this command line; the prompt goes in on stdin.
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd.exe");
        c.args(&args);
        c
    } else {
        let mut c = Command::new("claude");
        c.args(&args[3..]);
        c
    };
    cmd.current_dir(&task.cwd)
        .env("DISABLE_AUTOUPDATER", "1")
        .env("CLAUDE_FLEET_TASK_FROM", &task.from)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut cmd);

    let finish = |status: &str, error: Option<String>| {
        with_running(st, |t| {
            t.status = status.into();
            t.ended = Some(now_ms());
            if error.is_some() {
                t.error = error.clone();
            }
            error.clone().map(|e| step("error", e))
        });
        if let Some(t) = get(st, task.id) {
            tracing::info!("agent task {} {}", t.id, t.status);
            save_and_emit(st, &t);
        }
    };

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return finish("failed", Some(format!("couldn't start Claude Code: {e}. Is it installed on this PC?"))),
    };
    {
        let mut g = QUEUE.lock().unwrap();
        if let Some(r) = g.as_mut().and_then(|q| q.running.as_mut()) {
            r.pid = Some(child.id());
            if r.cancel {
                kill_tree(child.id());
            }
        }
    }
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(task.prompt.as_bytes());
    }
    let stderr_tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
    if let Some(err) = child.stderr.take() {
        let tail = stderr_tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let mut t = tail.lock().unwrap();
                t.push_back(line);
                if t.len() > 20 {
                    t.pop_front();
                }
            }
        });
    }

    let mut got_result = false;
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            match v.get("type").and_then(Value::as_str) {
                Some("system") if v.get("subtype").and_then(Value::as_str) == Some("init") => {
                    let model = v.get("model").and_then(Value::as_str).unwrap_or("").to_string();
                    let sid = v.get("session_id").and_then(Value::as_str).map(str::to_string);
                    with_running(st, |t| {
                        t.session_id = sid;
                        Some(step("note", format!("Claude Code started in {} · {model} · {}", t.cwd, t.mode)))
                    });
                }
                Some("assistant") => {
                    for c in v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                        let s = match c.get("type").and_then(Value::as_str) {
                            Some("text") => c.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()).map(|t| step("text", t)),
                            Some("tool_use") => {
                                let name = c.get("name").and_then(Value::as_str).unwrap_or("tool");
                                Some(step("tool", describe_tool(name, c.get("input").unwrap_or(&Value::Null))))
                            }
                            _ => None,
                        };
                        if s.is_some() {
                            with_running(st, |_| s);
                        }
                    }
                }
                Some("user") => {
                    // Only failed tool calls are worth showing; normal results are too long.
                    for c in v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                        if c.get("type").and_then(Value::as_str) == Some("tool_result") && c.get("is_error").and_then(Value::as_bool) == Some(true) {
                            let text = match c.get("content") {
                                Some(Value::String(s)) => s.clone(),
                                Some(Value::Array(a)) => a.iter().filter_map(|x| x.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
                                _ => String::new(),
                            };
                            with_running(st, |_| Some(step("toolError", clip(&text, 600))));
                        }
                    }
                }
                Some("result") => {
                    got_result = true;
                    let is_error = v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    let result = v.get("result").and_then(Value::as_str).map(str::to_string);
                    with_running(st, |t| {
                        t.result = result.clone();
                        t.cost_usd = v.get("total_cost_usd").and_then(Value::as_f64);
                        t.turns = v.get("num_turns").and_then(Value::as_i64);
                        if let Some(sid) = v.get("session_id").and_then(Value::as_str) {
                            t.session_id = Some(sid.to_string());
                        }
                        if is_error {
                            t.error = Some(result.clone().unwrap_or_else(|| {
                                v.get("subtype").and_then(Value::as_str).unwrap_or("error").to_string()
                            }));
                        }
                        None
                    });
                }
                _ => {}
            }
        }
    }
    let exit = child.wait();
    let cancelled = QUEUE.lock().unwrap().as_ref().and_then(|q| q.running.as_ref()).is_some_and(|r| r.cancel);
    let errored = get(st, task.id).and_then(|t| t.error).is_some();
    if cancelled {
        finish("cancelled", None);
    } else if got_result && !errored {
        finish("done", None);
    } else if got_result {
        finish("failed", None);
    } else {
        let tail = stderr_tail.lock().unwrap().iter().cloned().collect::<Vec<_>>().join("\n");
        let code = exit.ok().and_then(|s| s.code()).map(|c| format!(" (exit code {c})")).unwrap_or_default();
        let msg = if tail.trim().is_empty() { format!("Claude Code stopped without a result{code}.") } else { clip(&tail, 1500) };
        finish("failed", Some(msg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_by_access() {
        assert!(!mode_allowed("off", "plan"));
        assert!(mode_allowed("edits", "acceptEdits"));
        assert!(mode_allowed("edits", "auto"));
        assert!(!mode_allowed("edits", "bypassPermissions"));
        assert!(mode_allowed("full", "bypassPermissions"));
        assert!(!mode_allowed("full", "default; rm -rf"));
    }

    #[test]
    fn uuids() {
        assert!(is_uuid("52172b7a-1fc4-4823-b432-740c4bf9bc62"));
        assert!(!is_uuid("52172b7a-1fc4-4823-b432-740c4bf9bc6&"));
        assert!(!is_uuid("x & calc"));
    }

    #[test]
    fn tool_lines() {
        let v: Value = serde_json::json!({"command": "cargo test"});
        assert_eq!(describe_tool("Bash", &v), "Bash: cargo test");
        let v: Value = serde_json::json!({"pattern": "TODO", "path": "src"});
        assert_eq!(describe_tool("Grep", &v), "Grep: TODO in src");
    }
}
