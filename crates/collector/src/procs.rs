//! Heavy-process detection, the protected list, and ending processes safely.
//!
//! - Heavy = high CPU or RAM in *every* sample of the last ~10 s, never a single spike.
//! - Protected processes are never ended: core Windows processes, Claude, and Pane
//!   itself (with its own child processes, e.g. its WebView2 window).
//! - Ending uses a plain `TerminateProcess` with this user's rights. We never ask for admin;
//!   if Windows refuses, the caller is told that admin rights would be needed.
//! - A request names the process by pid *and* start time, so a pid that was reused by a
//!   different program in the meantime is never hit.

use crate::types::{HeavyProcess, KillImpact, KillOutcome, KillResult, ProcessInfo};
use std::collections::{HashMap, HashSet, VecDeque};

/// Whole-machine CPU percentage a process must hold to count as heavy.
pub const HEAVY_CPU_PCT: f32 = 20.0;
/// RAM a process must hold: the larger of 2 GB and 15% of physical memory.
pub const HEAVY_MEM_MIN: u64 = 2 * 1024 * 1024 * 1024;
pub const HEAVY_MEM_FRACTION: f64 = 0.15;
/// How long it must stay high.
pub const SUSTAIN_MS: i64 = 10_000;
const KEEP_SAMPLES: usize = 15;

/// Lower-case names without ".exe".
const PROTECTED_SYSTEM: &[&str] = &[
    "system",
    "system idle process",
    "idle",
    "registry",
    "secure system",
    "memory compression",
    "smss",
    "csrss",
    "wininit",
    "winlogon",
    "services",
    "lsass",
    "lsaiso",
    "dwm",
    "explorer",
    "svchost",
    "fontdrvhost",
    "sihost",
];
const PROTECTED_CLAUDE: &[&str] = &["claude"];
const PROTECTED_SELF: &[&str] = &["claude-fleet", "fleet-collector"];

fn base_name(name: &str) -> String {
    let n = name.trim().to_ascii_lowercase();
    n.strip_suffix(".exe").map(str::to_string).unwrap_or(n)
}

pub fn mem_threshold(total_memory: u64) -> u64 {
    HEAVY_MEM_MIN.max((total_memory as f64 * HEAVY_MEM_FRACTION) as u64)
}

/// Why a process may not be ended, if it may not.
pub fn protection(p: &ProcessInfo, by_pid: &HashMap<u32, &ProcessInfo>, self_pid: u32) -> Option<&'static str> {
    if p.pid == 0 || p.pid == 4 {
        return Some("Windows system process");
    }
    let n = base_name(&p.name);
    if PROTECTED_SYSTEM.contains(&n.as_str()) {
        return Some("Windows system process");
    }
    if PROTECTED_CLAUDE.contains(&n.as_str()) {
        return Some("Claude");
    }
    if p.pid == self_pid || PROTECTED_SELF.contains(&n.as_str()) {
        return Some("Pane itself");
    }
    // Children of Pane (its WebView2 window) keep the dashboard alive.
    let mut seen = HashSet::new();
    let mut cur = p.parent_pid;
    while let Some(pp) = cur {
        if !seen.insert(pp) {
            break;
        }
        if pp == self_pid {
            return Some("part of Pane");
        }
        cur = by_pid.get(&pp).and_then(|x| x.parent_pid);
    }
    None
}

struct Track {
    start_time: u64,
    samples: VecDeque<(i64, f32, u64)>,
    heavy_since: Option<i64>,
}

#[derive(Default)]
pub struct HeavyTracker {
    tracks: HashMap<u32, Track>,
}

impl HeavyTracker {
    /// Feed one sample of all processes; returns the processes that are heavy right now.
    pub fn update(&mut self, all: &[ProcessInfo], ts: i64, total_memory: u64, self_pid: u32) -> Vec<HeavyProcess> {
        let mem_t = mem_threshold(total_memory);
        let by_pid: HashMap<u32, &ProcessInfo> = all.iter().map(|p| (p.pid, p)).collect();
        let alive: HashSet<u32> = by_pid.keys().copied().collect();
        self.tracks.retain(|pid, _| alive.contains(pid));

        let mut out = Vec::new();
        for p in all {
            let t = self.tracks.entry(p.pid).or_insert_with(|| Track {
                start_time: p.start_time,
                samples: VecDeque::new(),
                heavy_since: None,
            });
            if t.start_time != p.start_time {
                // Same pid, different program.
                *t = Track { start_time: p.start_time, samples: VecDeque::new(), heavy_since: None };
            }
            t.samples.push_back((ts, p.cpu, p.memory));
            while t.samples.len() > KEEP_SAMPLES {
                t.samples.pop_front();
            }

            // Judge on time covered, not sample count: under heavy load the sampler can fall
            // behind (3-4 s between samples), and a count-based check would flap.
            let window: Vec<&(i64, f32, u64)> = t.samples.iter().filter(|s| ts - s.0 <= SUSTAIN_MS + 3_000).collect();
            let span = window.first().map(|f| ts - f.0).unwrap_or(0);
            let enough = window.len() >= 3 && span >= SUSTAIN_MS - 1_000;
            // Becoming heavy needs every sample above the line; once heavy, it stays heavy while
            // the window average holds above 80% of it, so a process hovering near the line
            // doesn't flicker in and out.
            let was = t.heavy_since.is_some();
            let n = window.len().max(1) as f64;
            let cpu_avg_w = window.iter().map(|s| s.1 as f64).sum::<f64>() / n;
            let mem_avg_w = window.iter().map(|s| s.2 as f64).sum::<f64>() / n;
            let cpu_heavy = if was {
                cpu_avg_w >= HEAVY_CPU_PCT as f64 * 0.8
            } else {
                enough && window.iter().all(|s| s.1 >= HEAVY_CPU_PCT)
            };
            let mem_heavy = if was {
                mem_avg_w >= mem_t as f64 * 0.8
            } else {
                enough && window.iter().all(|s| s.2 >= mem_t)
            };

            if cpu_heavy || mem_heavy {
                let since = *t.heavy_since.get_or_insert(window.first().map(|f| f.0).unwrap_or(ts));
                let cpu_avg = window.iter().map(|s| s.1).sum::<f32>() / window.len() as f32;
                out.push(HeavyProcess {
                    pid: p.pid,
                    name: p.name.clone(),
                    exe: p.exe.clone(),
                    start_time: p.start_time,
                    cpu_avg,
                    memory: p.memory,
                    since,
                    reason: match (cpu_heavy, mem_heavy) {
                        (true, true) => "cpu+memory",
                        (true, false) => "cpu",
                        _ => "memory",
                    }
                    .to_string(),
                    protected_reason: protection(p, &by_pid, self_pid).map(str::to_string),
                });
            } else {
                t.heavy_since = None;
            }
        }
        out.sort_by(|a, b| b.cpu_avg.total_cmp(&a.cpu_avg).then(b.memory.cmp(&a.memory)));
        out
    }
}

/// Root plus every descendant, parents before children. Protected processes and everything
/// below them are left out (and reported).
fn tree<'a>(
    root: &'a ProcessInfo,
    all: &'a [ProcessInfo],
    by_pid: &HashMap<u32, &ProcessInfo>,
    self_pid: u32,
) -> (Vec<&'a ProcessInfo>, Vec<(&'a ProcessInfo, &'static str)>) {
    let mut children: HashMap<u32, Vec<&ProcessInfo>> = HashMap::new();
    for p in all {
        if let Some(pp) = p.parent_pid {
            // Windows reuses pids: a "child" that started before its parent isn't one.
            if by_pid.get(&pp).is_some_and(|parent| parent.start_time <= p.start_time) {
                children.entry(pp).or_default().push(p);
            }
        }
    }
    let mut take = Vec::new();
    let mut skipped = Vec::new();
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(p) = stack.pop() {
        if !seen.insert(p.pid) {
            continue;
        }
        if let Some(reason) = protection(p, by_pid, self_pid) {
            skipped.push((p, reason));
            continue;
        }
        take.push(p);
        if let Some(c) = children.get(&p.pid) {
            stack.extend(c.iter().copied());
        }
    }
    (take, skipped)
}

pub fn impact(pid: u32, start_time: Option<u64>, all: &[ProcessInfo], self_pid: u32) -> Result<KillImpact, String> {
    let by_pid: HashMap<u32, &ProcessInfo> = all.iter().map(|p| (p.pid, p)).collect();
    let root = find(pid, start_time, &by_pid)?;
    let (take, skipped) = tree(root, all, &by_pid, self_pid);
    Ok(KillImpact {
        pid: root.pid,
        name: root.name.clone(),
        exe: root.exe.clone(),
        start_time: root.start_time,
        cpu: root.cpu,
        memory: root.memory,
        protected_reason: protection(root, &by_pid, self_pid).map(str::to_string),
        tree_count: take.len(),
        tree_cpu: take.iter().map(|p| p.cpu).sum(),
        tree_memory: take.iter().map(|p| p.memory).sum(),
        tree_protected: skipped.iter().map(|(p, r)| format!("{} ({r})", p.name)).collect(),
    })
}

fn find<'a>(pid: u32, start_time: Option<u64>, by_pid: &HashMap<u32, &'a ProcessInfo>) -> Result<&'a ProcessInfo, String> {
    let p = *by_pid.get(&pid).ok_or_else(|| format!("Process {pid} is no longer running."))?;
    if let Some(st) = start_time {
        if st != p.start_time {
            return Err(format!("Process {pid} has exited and its ID now belongs to {}. Nothing was ended.", p.name));
        }
    }
    Ok(p)
}

/// End a process (and optionally its tree, children first).
pub fn kill(pid: u32, start_time: Option<u64>, tree_mode: bool, all: &[ProcessInfo], self_pid: u32) -> Result<KillResult, String> {
    let by_pid: HashMap<u32, &ProcessInfo> = all.iter().map(|p| (p.pid, p)).collect();
    let root = find(pid, start_time, &by_pid)?;
    if let Some(reason) = protection(root, &by_pid, self_pid) {
        return Err(format!("{} is protected ({reason}) and was not ended.", root.name));
    }
    let (mut targets, skipped) = if tree_mode { tree(root, all, &by_pid, self_pid) } else { (vec![root], vec![]) };
    // Children before parents, so a parent can't respawn them mid-way.
    targets.reverse();

    let mut outcomes = Vec::new();
    for p in targets {
        let r = terminate(p.pid);
        outcomes.push(KillOutcome {
            pid: p.pid,
            name: p.name.clone(),
            memory: p.memory,
            cpu: p.cpu,
            ok: r.is_ok(),
            needs_admin: matches!(r, Err(TerminateError::AccessDenied)),
            error: r.err().map(|e| e.to_string()),
        });
    }
    let ok = outcomes.iter().any(|o| o.ok) && outcomes.iter().find(|o| o.pid == root.pid).is_some_and(|o| o.ok);
    Ok(KillResult {
        ok,
        tree: tree_mode,
        outcomes,
        skipped_protected: skipped.iter().map(|(p, r)| format!("{} ({r})", p.name)).collect(),
    })
}

#[derive(Debug)]
pub enum TerminateError {
    AccessDenied,
    Gone,
    Other(String),
}

impl std::fmt::Display for TerminateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TerminateError::AccessDenied => {
                write!(f, "Windows needs administrator rights to end this process; Pane doesn't run as admin.")
            }
            TerminateError::Gone => write!(f, "It had already exited."),
            TerminateError::Other(e) => write!(f, "{e}"),
        }
    }
}

#[cfg(windows)]
fn terminate(pid: u32) -> Result<(), TerminateError> {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER};
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    let classify = |e: windows::core::Error| {
        if e.code() == ERROR_ACCESS_DENIED.to_hresult() {
            TerminateError::AccessDenied
        } else if e.code() == ERROR_INVALID_PARAMETER.to_hresult() {
            TerminateError::Gone
        } else {
            TerminateError::Other(e.message().to_string())
        }
    };
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, false, pid).map_err(classify)?;
        let r = TerminateProcess(h, 1).map_err(classify);
        let _ = CloseHandle(h);
        r
    }
}

#[cfg(not(windows))]
fn terminate(pid: u32) -> Result<(), TerminateError> {
    let s = sysinfo::System::new_all().map_err(|e| TerminateError::Other(format!("{e:?}")))?;
    match s.process(sysinfo::Pid::from_u32(pid)) {
        Some(p) if p.kill() => Ok(()),
        Some(_) => Err(TerminateError::AccessDenied),
        None => Err(TerminateError::Gone),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: Option<u32>, name: &str, cpu: f32, mem: u64) -> ProcessInfo {
        ProcessInfo { pid, parent_pid: parent, name: name.into(), exe: None, cpu, memory: mem, run_time_s: 100, start_time: 1000 + pid as u64 }
    }

    #[test]
    fn spike_is_not_heavy_but_sustained_is() {
        let mut t = HeavyTracker::default();
        let mut heavy = vec![];
        for i in 0..8 {
            let cpu = if i == 3 { 90.0 } else { 5.0 };
            heavy = t.update(&[p(10, None, "spiky.exe", cpu, 1), p(11, None, "busy.exe", 50.0, 1)], i * 2000, 16 << 30, 1);
        }
        let names: Vec<_> = heavy.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["busy.exe"]);
    }

    #[test]
    fn stays_heavy_through_a_dip() {
        let mut t = HeavyTracker::default();
        let cpus = [25.0, 26.0, 24.0, 25.0, 27.0, 25.0, 18.0, 24.0, 26.0];
        let mut last = vec![];
        for (i, c) in cpus.iter().enumerate() {
            last = t.update(&[p(11, None, "busy.exe", *c, 1)], i as i64 * 2000, 16 << 30, 1);
        }
        assert_eq!(last.len(), 1, "a single dip to 18% should not clear it");
    }

    #[test]
    fn slow_sampling_does_not_flap() {
        // Sampler falling behind: 4 s between samples instead of 2.
        let mut t = HeavyTracker::default();
        let mut seen = vec![];
        for i in 0..10 {
            let h = t.update(&[p(11, None, "busy.exe", 30.0, 1)], i * 4000, 16 << 30, 1);
            seen.push(!h.is_empty());
        }
        // Heavy from the first moment 10 s are covered, and never drops after that.
        let first = seen.iter().position(|x| *x).expect("never became heavy");
        assert!(first <= 3, "became heavy too late: {seen:?}");
        assert!(seen[first..].iter().all(|x| *x), "flapped: {seen:?}");
    }

    #[test]
    fn protected_names_and_self_children() {
        let all = vec![p(500, None, "claude-fleet.exe", 1.0, 1), p(501, Some(500), "msedgewebview2.exe", 1.0, 1), p(600, None, "CSRSS.EXE", 1.0, 1), p(700, None, "Claude.exe", 1.0, 1), p(800, None, "game.exe", 1.0, 1)];
        let by: HashMap<u32, &ProcessInfo> = all.iter().map(|x| (x.pid, x)).collect();
        assert_eq!(protection(&all[1], &by, 500), Some("part of Pane"));
        assert_eq!(protection(&all[2], &by, 500), Some("Windows system process"));
        assert_eq!(protection(&all[3], &by, 500), Some("Claude"));
        assert_eq!(protection(&all[4], &by, 500), None);
    }

    #[test]
    fn tree_skips_protected_branches() {
        let all = vec![p(10, None, "launcher.exe", 1.0, 1), p(11, Some(10), "worker.exe", 1.0, 1), p(12, Some(10), "claude.exe", 1.0, 1), p(13, Some(12), "bash.exe", 1.0, 1)];
        let imp = impact(10, Some(1010), &all, 1).unwrap();
        assert_eq!(imp.tree_count, 2); // launcher + worker; claude and its child are left alone
        assert_eq!(imp.tree_protected.len(), 1);
    }

    #[test]
    fn reused_pid_is_refused() {
        let all = vec![p(10, None, "other.exe", 1.0, 1)];
        assert!(kill(10, Some(42), false, &all, 1).is_err());
    }
}
