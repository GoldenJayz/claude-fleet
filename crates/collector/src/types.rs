//! Wire types for the v1 API. Everything is JSON, camelCase, times in Unix milliseconds.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineInfo {
    pub api_version: u32,
    pub collector_version: String,
    pub machine_name: String,
    pub hostname: String,
    pub os: String,
    pub cpu_brand: String,
    pub logical_cores: usize,
    pub physical_cores: Option<usize>,
    pub total_memory: u64,
    pub boot_time_ms: i64,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub kind: String,
    pub total: u64,
    pub available: u64,
    /// Bytes/second since the previous sample.
    pub read_bps: u64,
    pub write_bps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vendor: Option<String>,
    /// 0–100, busiest engine (same as Task Manager).
    pub usage: Option<f32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub temperature_c: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempReading {
    pub label: String,
    pub celsius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub exe: Option<String>,
    /// Percent of the whole machine (0–100), not of one core.
    pub cpu: f32,
    pub memory: u64,
    pub run_time_s: u64,
    /// Seconds since the epoch; with `pid` it identifies a process even if the pid is reused.
    pub start_time: u64,
}

/// A process that has used a lot of CPU or RAM for the whole last ~10 s.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeavyProcess {
    pub pid: u32,
    pub name: String,
    pub exe: Option<String>,
    pub start_time: u64,
    pub cpu_avg: f32,
    pub memory: u64,
    /// When it became heavy (ms).
    pub since: i64,
    /// "cpu", "memory" or "cpu+memory"
    pub reason: String,
    /// Set when it must not be ended, e.g. "Windows system process".
    pub protected_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeavyRules {
    pub cpu_pct: f32,
    pub memory_bytes: u64,
    pub sustain_ms: i64,
}

/// What ending a process (or its tree) would free, for the confirmation dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillImpact {
    pub pid: u32,
    pub name: String,
    pub exe: Option<String>,
    pub start_time: u64,
    pub cpu: f32,
    pub memory: u64,
    pub protected_reason: Option<String>,
    pub tree_count: usize,
    pub tree_cpu: f32,
    pub tree_memory: u64,
    /// Protected processes inside the tree that would be left running.
    pub tree_protected: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillOutcome {
    pub pid: u32,
    pub name: String,
    pub memory: u64,
    pub cpu: f32,
    pub ok: bool,
    pub needs_admin: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillResult {
    pub ok: bool,
    pub tree: bool,
    pub outcomes: Vec<KillOutcome>,
    pub skipped_protected: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KillLogEntry {
    pub id: i64,
    pub ts: i64,
    pub machine: String,
    pub pid: u32,
    pub name: String,
    pub tree: bool,
    pub requested_by: String,
    pub ok: bool,
    pub detail: Option<String>,
}

/// One metrics sample, taken every ~2 s.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSample {
    pub ts: i64,
    /// 0–100
    pub cpu: f32,
    pub cpu_per_core: Vec<f32>,
    pub cpu_freq_mhz: u64,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub disks: Vec<DiskInfo>,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    pub gpus: Vec<GpuInfo>,
    pub temps: Vec<TempReading>,
}

/// A compact point for sparklines / history charts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsPoint {
    pub ts: i64,
    pub cpu: f32,
    pub mem_pct: f32,
    pub gpu: Option<f32>,
    pub disk_read_bps: u64,
    pub disk_write_bps: u64,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
}

impl From<&MetricsSample> for MetricsPoint {
    fn from(s: &MetricsSample) -> Self {
        MetricsPoint {
            ts: s.ts,
            cpu: s.cpu,
            mem_pct: if s.mem_total > 0 { s.mem_used as f32 * 100.0 / s.mem_total as f32 } else { 0.0 },
            gpu: primary_gpu(&s.gpus).and_then(|g| g.usage),
            disk_read_bps: s.disk_read_bps,
            disk_write_bps: s.disk_write_bps,
            net_rx_bps: s.net_rx_bps,
            net_tx_bps: s.net_tx_bps,
        }
    }
}

/// The GPU worth showing in the ring: the one with the most dedicated memory.
pub fn primary_gpu(gpus: &[GpuInfo]) -> Option<&GpuInfo> {
    gpus.iter().max_by_key(|g| g.memory_total.unwrap_or(0))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub thinking: u64,
    /// Number of API responses.
    pub requests: u64,
    /// Number of prompts you typed.
    pub prompts: u64,
}

impl TokenTotals {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTotals {
    pub model: String,
    pub tokens: TokenTotals,
}

/// The step a working session is on, from its latest log line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub ts: i64,
    /// "thinking", "text" or "tool"
    pub kind: String,
    pub tool: Option<String>,
    /// Short, local-only hint: a file name, a command description, a search pattern.
    pub detail: Option<String>,
    pub in_subagent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptEntry {
    pub id: String,
    pub session_id: String,
    pub ts: i64,
    /// "prompt" (typed text), "command" (a /slash command) or "bash" (a ! command).
    pub kind: String,
    pub text: String,
    pub project: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// True when no log line carried an effort and the settings.json default was used.
    pub effort_is_default: bool,
    pub tokens: TokenTotals,
    pub subagent_tokens: u64,
    /// Same as `status == "working"`.
    pub active: bool,
    /// "working" (Claude is still on it), "done" (finished), or "stopped" (interrupted / abandoned).
    pub status: String,
    pub activity: Option<Activity>,
    /// Time of the last response for this prompt.
    pub last_activity_ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyTokens {
    /// Local date, YYYY-MM-DD.
    pub day: String,
    pub tokens: TokenTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSummary {
    pub today: TokenTotals,
    pub week: TokenTotals,
    pub today_by_model: Vec<ModelTotals>,
    pub recent: Vec<PromptEntry>,
    /// Prompts Claude is working on right now (one per busy session).
    pub working: Vec<PromptEntry>,
    pub default_effort: Option<String>,
    pub last_activity_ts: Option<i64>,
    pub files_tracked: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    /// 0–100
    pub used_percentage: f64,
    pub resets_at_ms: i64,
}

/// Plan usage captured by the status-line script (per account, so every machine reports the same).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanReading {
    /// When Claude Code last handed this data to the status line.
    pub captured_at_ms: i64,
    pub five_hour: Option<LimitWindow>,
    pub seven_day: Option<LimitWindow>,
    pub claude_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusLineState {
    pub installed: bool,
    pub chained_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub machine: MachineInfo,
    pub metrics: Option<MetricsSample>,
    /// Last ~60 s for sparklines.
    pub history: Vec<MetricsPoint>,
    pub processes: Vec<ProcessInfo>,
    pub heavy: Vec<HeavyProcess>,
    pub heavy_rules: HeavyRules,
    pub claude: ClaudeSummary,
    pub plan: Option<PlanReading>,
    pub status_line: StatusLineState,
}

/// Messages pushed on /v1/stream.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    Snapshot(Box<Snapshot>),
    Metrics {
        sample: MetricsSample,
        processes: Vec<ProcessInfo>,
        heavy: Vec<HeavyProcess>,
    },
    Claude(ClaudeSummary),
    Plan(PlanReading),
}
