//! System metrics via `sysinfo` (CPU, RAM, disks, network, GPU, temperatures, processes),
//! plus NVML for NVIDIA GPU temperatures when the driver is present.
//!
//! `Sampler` owns OS handles that are not `Send` on every platform, so it lives on its own thread.

use crate::types::*;
use std::collections::HashMap;
use std::time::Instant;
use crate::gpu::GpuMonitor;
use sysinfo::{
    Components, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind,
};

pub struct Sampler {
    sys: System,
    disks: Disks,
    networks: Networks,
    components: Option<Components>,
    gpus: GpuMonitor,
    nvml: Option<nvml_wrapper::Nvml>,
    last: Instant,
    ticks: u64,
    temps: Vec<TempReading>,
    nvml_temps: HashMap<String, f32>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Sampler {
    pub fn new() -> anyhow::Result<Self> {
        let mut sys = System::new().map_err(|e| anyhow::anyhow!("sysinfo: {e:?}"))?;
        sys.refresh_cpu_all();
        sys.refresh_memory();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, proc_kind());
        let disks = Disks::new_with_refreshed_list().map_err(|e| anyhow::anyhow!("disks: {e:?}"))?;
        let networks =
            Networks::new_with_refreshed_list().map_err(|e| anyhow::anyhow!("networks: {e:?}"))?;
        let components = Components::new_with_refreshed_list().ok();
        let gpus = GpuMonitor::new();
        let nvml = nvml_wrapper::Nvml::init().ok();
        Ok(Self {
            sys,
            disks,
            networks,
            components,
            gpus,
            nvml,
            last: Instant::now(),
            ticks: 0,
            temps: Vec::new(),
            nvml_temps: HashMap::new(),
        })
    }

    pub fn machine_info(&self, machine_name: &str, port: u16) -> MachineInfo {
        let os = System::long_os_version()
            .ok()
            .or_else(|| System::name().ok())
            .unwrap_or_else(|| "unknown".into());
        let boot = System::boot_time().unwrap_or(0) as i64 * 1000;
        MachineInfo {
            api_version: crate::config::API_VERSION,
            collector_version: env!("CARGO_PKG_VERSION").to_string(),
            machine_name: machine_name.to_string(),
            hostname: crate::config::hostname(),
            os,
            cpu_brand: self.sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
            logical_cores: self.sys.cpus().len(),
            physical_cores: System::physical_core_count().ok(),
            total_memory: self.sys.total_memory(),
            boot_time_ms: boot,
            port,
            features: vec!["files".into()],
        }
    }

    /// Take one sample. Returns the sample and *all* processes (callers pick the top ones).
    pub fn sample(&mut self) -> (MetricsSample, Vec<ProcessInfo>) {
        let elapsed = self.last.elapsed().as_secs_f64().max(0.2);
        self.last = Instant::now();
        self.ticks += 1;

        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true, proc_kind());
        self.disks.refresh(true);
        self.networks.refresh(true);
        // WMI temperature queries are slow; every ~10 s is plenty.
        if self.ticks % 5 == 1 {
            self.refresh_temps();
        }

        let rate = |bytes: u64| (bytes as f64 / elapsed) as u64;

        let mut seen_mounts = std::collections::HashSet::new();
        let mut disks = Vec::new();
        let (mut dr, mut dw) = (0u64, 0u64);
        for d in &self.disks {
            let mount = d.mount_point().to_string_lossy().into_owned();
            if !seen_mounts.insert(mount.clone()) || d.total_space() == 0 {
                continue;
            }
            let u = d.usage();
            dr += u.read_bytes;
            dw += u.written_bytes;
            disks.push(DiskInfo {
                name: d.name().to_string_lossy().into_owned(),
                mount,
                kind: format!("{:?}", d.kind()),
                total: d.total_space(),
                available: d.available_space(),
                read_bps: rate(u.read_bytes),
                write_bps: rate(u.written_bytes),
            });
        }

        let (mut rx, mut tx) = (0u64, 0u64);
        for (name, data) in &self.networks {
            let n = name.to_string_lossy().to_ascii_lowercase();
            // Loopback and Hyper-V virtual switches mirror real traffic; skip them.
            if n.contains("loopback") || n.starts_with("vethernet") || n.starts_with("lo") && n.len() <= 3 {
                continue;
            }
            rx += data.received();
            tx += data.transmitted();
        }

        let gpus = self.gpu_infos();

        let cpus = self.sys.cpus();
        let sample = MetricsSample {
            ts: now_ms(),
            cpu: self.sys.global_cpu_usage(),
            cpu_per_core: cpus.iter().map(|c| c.usage()).collect(),
            cpu_freq_mhz: cpus.first().map(|c| c.frequency()).unwrap_or(0),
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            disks,
            disk_read_bps: rate(dr),
            disk_write_bps: rate(dw),
            net_rx_bps: rate(rx),
            net_tx_bps: rate(tx),
            gpus,
            temps: self.temps.clone(),
        };

        let ncpu = cpus.len().max(1) as f32;
        let processes = self
            .sys
            .processes()
            .values()
            .filter(|p| p.pid().as_u32() != 0)
            .map(|p| ProcessInfo {
                pid: p.pid().as_u32(),
                parent_pid: p.parent().map(|pp| pp.as_u32()),
                name: p.name().to_string_lossy().into_owned(),
                exe: p.exe().map(|e| e.to_string_lossy().into_owned()),
                cpu: (p.cpu_usage() / ncpu).min(100.0),
                memory: p.memory(),
                run_time_s: p.run_time(),
                start_time: p.start_time(),
            })
            .collect();

        (sample, processes)
    }

    fn refresh_temps(&mut self) {
        let mut temps = Vec::new();
        if let Some(c) = self.components.as_mut() {
            c.refresh(true);
            for comp in c.iter() {
                if let Some(t) = comp.temperature() {
                    if t.is_finite() && t > 0.0 && t < 150.0 {
                        temps.push(TempReading { label: comp.label().to_string(), celsius: t });
                    }
                }
            }
        }
        self.nvml_temps.clear();
        if let Some(nvml) = &self.nvml {
            use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
            let count = nvml.device_count().unwrap_or(0);
            for i in 0..count {
                if let Ok(dev) = nvml.device_by_index(i) {
                    if let (Ok(name), Ok(t)) = (dev.name(), dev.temperature(TemperatureSensor::Gpu)) {
                        temps.push(TempReading { label: format!("{name} (GPU)"), celsius: t as f32 });
                        self.nvml_temps.insert(name, t as f32);
                    }
                }
            }
        }
        self.temps = temps;
    }

    fn gpu_infos(&mut self) -> Vec<GpuInfo> {
        let mut gpus = self.gpus.sample();
        for g in &mut gpus {
            g.temperature_c = self
                .nvml_temps
                .iter()
                .find(|(n, _)| g.name.contains(n.as_str()) || n.contains(g.name.as_str()))
                .map(|(_, t)| *t);
        }
        gpus
    }
}

fn proc_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_memory()
        .with_cpu()
        .with_exe(UpdateKind::OnlyIfNotSet)
}

/// Top processes for display: union of the top `n` by CPU and the top `n` by memory.
pub fn top_processes(all: &[ProcessInfo], n: usize) -> Vec<ProcessInfo> {
    let mut by_cpu: Vec<&ProcessInfo> = all.iter().collect();
    by_cpu.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    let mut by_mem: Vec<&ProcessInfo> = all.iter().collect();
    by_mem.sort_by(|a, b| b.memory.cmp(&a.memory));
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in by_cpu.into_iter().take(n).chain(by_mem.into_iter().take(n)) {
        if seen.insert(p.pid) {
            out.push(p.clone());
        }
    }
    out.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
    out
}
