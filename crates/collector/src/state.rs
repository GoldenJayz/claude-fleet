//! The running collector: background loops plus the shared state the API reads.

use crate::config::CollectorConfig;
use crate::types::*;
use crate::{claude, db, metrics, procs, statusline};
use anyhow::Result;
use chrono::{Datelike, Local, TimeZone};
use rusqlite::Connection;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;

pub const SAMPLE_EVERY: Duration = Duration::from_secs(2);
pub const LOG_SCAN_EVERY: Duration = Duration::from_secs(5);
const SPARK_POINTS: usize = 30; // 60 s at 2 s
const TOP_PROCESSES: usize = 12;
const RECENT_PROMPTS: usize = 40;
const METRICS_RETENTION_MS: i64 = 48 * 3600 * 1000;

#[derive(Default)]
pub struct Live {
    pub sample: Option<MetricsSample>,
    pub spark: VecDeque<MetricsPoint>,
    pub top: Vec<ProcessInfo>,
    pub all: Vec<ProcessInfo>,
    pub heavy: Vec<HeavyProcess>,
}

pub struct Shared {
    pub cfg: CollectorConfig,
    pub machine: MachineInfo,
    /// Used by the API and the metrics writer.
    pub db: Mutex<Connection>,
    /// Used only by the log reader, so a big first import never blocks the API.
    pub ingest_db: Mutex<Connection>,
    pub live: RwLock<Live>,
    pub claude: RwLock<ClaudeSummary>,
    pub plan: RwLock<Option<PlanReading>>,
    pub events: broadcast::Sender<Arc<str>>,
    heavy: Mutex<procs::HeavyTracker>,
}

pub type SharedState = Arc<Shared>;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn local_midnight_ms(days_back: i64) -> i64 {
    let today = Local::now().date_naive() - chrono::Days::new(days_back as u64);
    Local
        .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|d| d.timestamp_millis())
        .unwrap_or(0)
}

/// Start of the current week (Monday 00:00 local).
fn week_start_ms() -> i64 {
    local_midnight_ms(Local::now().weekday().num_days_from_monday() as i64)
}

impl Shared {
    pub fn emit(&self, ev: &StreamEvent) {
        if let Ok(s) = serde_json::to_string(ev) {
            let _ = self.events.send(Arc::from(s));
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let live = self.live.read().unwrap();
        Snapshot {
            machine: self.machine.clone(),
            metrics: live.sample.clone(),
            history: live.spark.iter().cloned().collect(),
            processes: live.top.clone(),
            heavy: live.heavy.clone(),
            heavy_rules: self.heavy_rules(),
            claude: self.claude.read().unwrap().clone(),
            plan: self.plan.read().unwrap().clone(),
            status_line: statusline::state(&self.cfg),
        }
    }

    pub fn heavy_rules(&self) -> HeavyRules {
        HeavyRules {
            cpu_pct: procs::HEAVY_CPU_PCT,
            memory_bytes: procs::mem_threshold(self.machine.total_memory),
            sustain_ms: procs::SUSTAIN_MS,
        }
    }

    /// Log a finished incoming file and tell connected dashboards about it.
    pub fn record_received(&self, mut f: ReceivedFile) -> ReceivedFile {
        match db::insert_received(&self.db.lock().unwrap(), &f) {
            Ok(id) => f.id = id,
            Err(e) => tracing::warn!("couldn't log received file: {e:#}"),
        }
        tracing::info!("received {} ({} bytes) from {} ({})", f.name, f.size, f.from, f.ip);
        self.emit(&StreamEvent::FileReceived(f.clone()));
        f
    }

    /// Log a remote shell session start (id 0) or end, and announce it on the stream.
    pub fn record_shell(&self, mut s: ShellSession) -> ShellSession {
        match db::upsert_shell(&self.db.lock().unwrap(), &s) {
            Ok(id) => s.id = id,
            Err(e) => tracing::warn!("couldn't log shell session: {e:#}"),
        }
        match s.ended {
            None => tracing::info!("remote shell opened by {} ({})", s.from, s.ip),
            Some(_) => tracing::info!("remote shell by {} ({}) closed", s.from, s.ip),
        }
        self.emit(&StreamEvent::Shell(s.clone()));
        s
    }

    pub fn self_pid() -> u32 {
        std::process::id()
    }

    pub fn kill_impact(&self, pid: u32, start_time: Option<u64>) -> Result<KillImpact, String> {
        let live = self.live.read().unwrap();
        procs::impact(pid, start_time, &live.all, Self::self_pid())
    }

    /// End a process (or tree) and record the attempt in the kill log, whatever the outcome.
    pub fn kill(&self, pid: u32, start_time: Option<u64>, tree: bool, requested_by: &str) -> Result<KillResult, String> {
        let all = self.live.read().unwrap().all.clone();
        let name = all.iter().find(|p| p.pid == pid).map(|p| p.name.clone()).unwrap_or_else(|| format!("pid {pid}"));
        let result = procs::kill(pid, start_time, tree, &all, Self::self_pid());
        let (ok, detail) = match &result {
            Ok(r) => {
                let ended = r.outcomes.iter().filter(|o| o.ok).count();
                let failed: Vec<String> = r
                    .outcomes
                    .iter()
                    .filter(|o| !o.ok)
                    .map(|o| format!("{}: {}", o.name, o.error.clone().unwrap_or_default()))
                    .collect();
                let mut d = format!("ended {ended} of {}", r.outcomes.len());
                if !failed.is_empty() {
                    d.push_str(&format!("; failed: {}", failed.join("; ")));
                }
                if !r.skipped_protected.is_empty() {
                    d.push_str(&format!("; left running (protected): {}", r.skipped_protected.join(", ")));
                }
                (r.ok, Some(d))
            }
            Err(e) => (false, Some(e.clone())),
        };
        let entry = KillLogEntry {
            id: 0,
            ts: now_ms(),
            machine: self.cfg.machine_name.clone(),
            pid,
            name: name.clone(),
            tree,
            requested_by: requested_by.to_string(),
            ok,
            detail,
        };
        tracing::info!("kill {} ({pid}) tree={tree} by {requested_by}: ok={ok} {:?}", name, entry.detail);
        if let Err(e) = db::insert_kill(&self.db.lock().unwrap(), &entry) {
            tracing::warn!("writing kill log: {e:#}");
        }
        result
    }

    pub fn build_claude_summary(&self, conn: &Connection) -> Result<ClaudeSummary> {
        let now = now_ms();
        let far = i64::MAX;
        let mut recent = db::recent_prompts(conn, RECENT_PROMPTS, None, now)?;
        for p in &mut recent {
            if p.effort.is_none() {
                p.effort = claude::default_effort(&self.cfg.claude_dir, p.model.as_deref());
                p.effort_is_default = p.effort.is_some();
            }
        }
        Ok(ClaudeSummary {
            today: db::totals(conn, local_midnight_ms(0), far)?,
            week: db::totals(conn, week_start_ms(), far)?,
            today_by_model: db::totals_by_model(conn, local_midnight_ms(0), far)?,
            working: db::working_prompts(conn, now)?,
            recent,
            default_effort: claude::default_effort(&self.cfg.claude_dir, None),
            last_activity_ts: db::last_activity(conn)?,
            files_tracked: db::files_tracked(conn)?,
        })
    }
}

pub struct Collector {
    pub state: SharedState,
}

impl Collector {
    /// Open the DB, start sampling, log reading and plan polling. Does not start the HTTP server.
    pub async fn start(cfg: CollectorConfig) -> Result<Self> {
        let db_conn = db::open(&cfg.db_path())?;
        let _ = db::close_stale_shells(&db_conn, now_ms());
        let ingest = db::open(&cfg.db_path())?;
        statusline::write_script(&cfg)?;

        // The sampler owns OS handles, so it gets its own thread; it reports machine info first.
        let (info_tx, info_rx) = std::sync::mpsc::channel();
        let (sample_tx, mut sample_rx) = tokio::sync::mpsc::channel::<(MetricsSample, Vec<ProcessInfo>)>(4);
        let name = cfg.machine_name.clone();
        let port = cfg.port;
        std::thread::Builder::new().name("fleet-sampler".into()).spawn(move || {
            let mut sampler = match metrics::Sampler::new() {
                Ok(s) => s,
                Err(e) => {
                    let _ = info_tx.send(Err(e));
                    return;
                }
            };
            let _ = info_tx.send(Ok(sampler.machine_info(&name, port)));
            // sysinfo needs a gap between CPU refreshes for a meaningful first reading.
            std::thread::sleep(Duration::from_millis(500));
            loop {
                let started = std::time::Instant::now();
                if sample_tx.blocking_send(sampler.sample()).is_err() {
                    break;
                }
                std::thread::sleep(SAMPLE_EVERY.saturating_sub(started.elapsed()));
            }
        })?;
        let machine = tokio::task::spawn_blocking(move || info_rx.recv())
            .await??
            .map_err(|e| anyhow::anyhow!("starting sampler: {e:#}"))?;

        let (events, _) = broadcast::channel(64);
        let state = Arc::new(Shared {
            machine,
            db: Mutex::new(db_conn),
            ingest_db: Mutex::new(ingest),
            live: RwLock::new(Live::default()),
            claude: RwLock::new(ClaudeSummary {
                today: TokenTotals::default(),
                week: TokenTotals::default(),
                today_by_model: vec![],
                recent: vec![],
                working: vec![],
                default_effort: None,
                last_activity_ts: None,
                files_tracked: 0,
            }),
            plan: RwLock::new(statusline::read_plan(&cfg)),
            cfg,
            events,
            heavy: Mutex::new(procs::HeavyTracker::default()),
        });

        // Metrics consumer: keep live state, persist history, push to subscribers.
        {
            let st = state.clone();
            tokio::spawn(async move {
                let mut last_prune = 0i64;
                while let Some((sample, all)) = sample_rx.recv().await {
                    let point = MetricsPoint::from(&sample);
                    let top = metrics::top_processes(&all, TOP_PROCESSES);
                    let heavy = st.heavy.lock().unwrap().update(&all, sample.ts, sample.mem_total, Shared::self_pid());
                    {
                        let mut live = st.live.write().unwrap();
                        live.spark.push_back(point.clone());
                        while live.spark.len() > SPARK_POINTS {
                            live.spark.pop_front();
                        }
                        live.sample = Some(sample.clone());
                        live.top = top.clone();
                        live.all = all;
                        live.heavy = heavy.clone();
                    }
                    let st2 = st.clone();
                    let prune = point.ts - last_prune > 3600 * 1000;
                    if prune {
                        last_prune = point.ts;
                    }
                    let _ = tokio::task::spawn_blocking(move || {
                        let conn = st2.db.lock().unwrap();
                        if let Err(e) = db::insert_metrics(&conn, &point) {
                            tracing::warn!("saving metrics: {e:#}");
                        }
                        if prune {
                            let _ = db::prune_metrics(&conn, point.ts - METRICS_RETENTION_MS);
                        }
                    })
                    .await;
                    st.emit(&StreamEvent::Metrics { sample, processes: top, heavy });
                }
            });
        }

        // Claude log reader + summary.
        {
            let st = state.clone();
            tokio::spawn(async move {
                let mut tick = 0u64;
                loop {
                    let st2 = st.clone();
                    // Keep "working" states fresh (they expire on silence) and roll over at midnight.
                    let force = tick % 2 == 0;
                    let res = tokio::task::spawn_blocking(move || -> Result<Option<ClaudeSummary>> {
                        let mut conn = st2.ingest_db.lock().unwrap();
                        let stats = claude::scan(&mut conn, &st2.cfg.claude_dir)?;
                        if stats.changed() {
                            tracing::info!(
                                "logs: +{} prompts, +{} responses ({} files, {} KB)",
                                stats.new_prompts,
                                stats.new_messages,
                                stats.files_read,
                                stats.bytes_read / 1024
                            );
                        }
                        if stats.changed() || force {
                            Ok(Some(st2.build_claude_summary(&conn)?))
                        } else {
                            Ok(None)
                        }
                    })
                    .await;
                    match res {
                        Ok(Ok(Some(summary))) => {
                            *st.claude.write().unwrap() = summary.clone();
                            st.emit(&StreamEvent::Claude(summary));
                        }
                        Ok(Ok(None)) => {}
                        Ok(Err(e)) => tracing::warn!("log scan: {e:#}"),
                        Err(e) => tracing::warn!("log scan task: {e}"),
                    }
                    tick += 1;
                    tokio::time::sleep(LOG_SCAN_EVERY).await;
                }
            });
        }

        // Plan usage: asked from Anthropic every few minutes, plus whatever the status line
        // captured; the newer reading wins.
        {
            let st = state.clone();
            tokio::spawn(async move {
                let mut from_api: Option<PlanReading> = None;
                let mut next_api = std::time::Instant::now();
                let mut last_error: Option<String> = None;
                loop {
                    if std::time::Instant::now() >= next_api {
                        next_api = std::time::Instant::now() + crate::usage::POLL_EVERY;
                        let cfg = st.cfg.clone();
                        match tokio::task::spawn_blocking(move || crate::usage::fetch(&cfg)).await {
                            Ok(Ok(p)) => {
                                from_api = Some(p);
                                last_error = None;
                            }
                            Ok(Err(e)) => {
                                if last_error.as_deref() != Some(e.as_str()) {
                                    tracing::info!("plan usage from Anthropic unavailable: {e}");
                                }
                                last_error = Some(e);
                            }
                            Err(_) => {}
                        }
                    }
                    let from_status = statusline::read_plan(&st.cfg);
                    let latest = [from_api.clone(), from_status].into_iter().flatten().max_by_key(|p| p.captured_at_ms);
                    let changed = {
                        let cur = st.plan.read().unwrap();
                        cur.as_ref().map(|p| p.captured_at_ms) != latest.as_ref().map(|p| p.captured_at_ms)
                    };
                    if changed {
                        *st.plan.write().unwrap() = latest.clone();
                        if let Some(p) = latest {
                            st.emit(&StreamEvent::Plan(p));
                        }
                    }
                    tokio::time::sleep(LOG_SCAN_EVERY).await;
                }
            });
        }

        Ok(Self { state })
    }
}

pub fn metrics_history(st: &Shared, minutes: i64) -> Result<Vec<MetricsPoint>> {
    let minutes = minutes.clamp(1, 48 * 60);
    // Aim for ≤ 360 points.
    let bucket_ms = ((minutes * 60_000) / 360).max(2000);
    let conn = st.db.lock().unwrap();
    db::metrics_history(&conn, now_ms() - minutes * 60_000, bucket_ms)
}

pub fn daily(st: &Shared, days: i64) -> Result<Vec<DailyTokens>> {
    let conn = st.db.lock().unwrap();
    db::daily(&conn, local_midnight_ms(days.clamp(1, 365) - 1))
}

pub fn prompts(st: &Shared, limit: usize, before: Option<i64>) -> Result<Vec<PromptEntry>> {
    let conn = st.db.lock().unwrap();
    let mut out = db::recent_prompts(&conn, limit.clamp(1, 200), before, now_ms())?;
    for p in &mut out {
        if p.effort.is_none() {
            p.effort = claude::default_effort(&st.cfg.claude_dir, p.model.as_deref());
            p.effort_is_default = p.effort.is_some();
        }
    }
    Ok(out)
}

pub fn received(st: &Shared, limit: usize) -> Result<Vec<ReceivedFile>> {
    let conn = st.db.lock().unwrap();
    db::list_received(&conn, limit.clamp(1, 500))
}

pub fn shell_sessions(st: &Shared, limit: usize) -> Result<Vec<ShellSession>> {
    let conn = st.db.lock().unwrap();
    db::list_shell(&conn, limit.clamp(1, 500))
}

pub fn kills(st: &Shared, limit: usize) -> Result<Vec<KillLogEntry>> {
    let conn = st.db.lock().unwrap();
    db::list_kills(&conn, limit.clamp(1, 500))
}

pub fn tokens(st: &Shared, from: i64, to: i64) -> Result<TokenTotals> {
    let conn = st.db.lock().unwrap();
    db::totals(&conn, from, to)
}
