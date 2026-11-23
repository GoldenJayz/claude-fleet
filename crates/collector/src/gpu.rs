//! GPU load and memory for any vendor (NVIDIA, AMD, Intel), measured the way Task Manager does:
//! adapters from DXGI, then the `GPU Engine` and `GPU Adapter Memory` performance counters
//! matched to adapters by LUID. No admin rights needed.
//!
//! Load = the busiest engine *type* (3D, Copy, VideoDecode…), each type summed over processes.

use crate::types::GpuInfo;

#[cfg(windows)]
mod imp {
    use super::GpuInfo;
    use std::collections::HashMap;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    };

    struct Adapter {
        /// Windows can list one physical GPU under several LUIDs (e.g. iGPU with a hybrid
        /// display path); those are merged by name and memory size.
        luids: Vec<(u32, u32)>, // (high, low)
        name: String,
        vendor: Option<String>,
        dedicated: u64,
    }

    pub struct GpuMonitor {
        adapters: Vec<Adapter>,
        query: Option<PDH_HQUERY>,
        engine: Option<PDH_HCOUNTER>,
        memory: Option<PDH_HCOUNTER>,
    }

    fn vendor_name(id: u32) -> Option<String> {
        Some(
            match id {
                0x10DE => "NVIDIA",
                0x1002 | 0x1022 => "AMD",
                0x8086 => "Intel",
                0x1414 => "Microsoft",
                0x5143 => "Qualcomm",
                _ => return None,
            }
            .to_string(),
        )
    }

    fn adapters() -> Vec<Adapter> {
        let mut out = Vec::new();
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return out };
            let mut i = 0;
            while let Ok(a) = factory.EnumAdapters1(i) {
                i += 1;
                let Ok(d) = a.GetDesc1() else { continue };
                if d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue;
                }
                let len = d.Description.iter().position(|&c| c == 0).unwrap_or(d.Description.len());
                let name = String::from_utf16_lossy(&d.Description[..len]).trim().to_string();
                let luid = (d.AdapterLuid.HighPart as u32, d.AdapterLuid.LowPart);
                // Virtual display adapters (Parsec, remote desktop) have no memory of their own.
                if d.DedicatedVideoMemory == 0 {
                    continue;
                }
                let dedicated = d.DedicatedVideoMemory as u64;
                if let Some(same) = out.iter_mut().find(|x: &&mut Adapter| x.name == name && x.dedicated == dedicated) {
                    if !same.luids.contains(&luid) {
                        same.luids.push(luid);
                    }
                    continue;
                }
                out.push(Adapter {
                    luids: vec![luid],
                    name,
                    vendor: vendor_name(d.VendorId),
                    dedicated,
                });
            }
        }
        out
    }

    unsafe fn add_counter(q: PDH_HQUERY, path: &str) -> Option<PDH_HCOUNTER> {
        let w: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut c = PDH_HCOUNTER::default();
        let r = unsafe { PdhAddEnglishCounterW(q, PCWSTR(w.as_ptr()), 0, &mut c) };
        (r == ERROR_SUCCESS.0).then_some(c)
    }

    /// (instance name, value) for a wildcard counter.
    unsafe fn read_array(c: PDH_HCOUNTER) -> Vec<(String, f64)> {
        let mut size = 0u32;
        let mut count = 0u32;
        unsafe {
            PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if size == 0 {
                return Vec::new();
            }
            // u64-aligned buffer: the items contain f64s.
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items))
                != ERROR_SUCCESS.0
            {
                return Vec::new();
            }
            std::slice::from_raw_parts(items, count as usize)
                .iter()
                .filter(|it| it.FmtValue.CStatus == 0)
                .map(|it| {
                    (PCWSTR(it.szName.0).to_string().unwrap_or_default(), it.FmtValue.Anonymous.doubleValue)
                })
                .collect()
        }
    }

    /// `…luid_0x00000000_0x0574BFA1…` → (high, low)
    fn parse_luid(name: &str) -> Option<(u32, u32)> {
        let lower = name.to_ascii_lowercase();
        let rest = &lower[lower.find("luid_0x")? + 7..];
        let mut parts = rest.split('_');
        let high = u32::from_str_radix(parts.next()?, 16).ok()?;
        let low = u32::from_str_radix(parts.next()?.strip_prefix("0x")?, 16).ok()?;
        Some((high, low))
    }

    impl GpuMonitor {
        pub fn new() -> Self {
            let mut m = GpuMonitor { adapters: adapters(), query: None, engine: None, memory: None };
            unsafe {
                let mut q = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) == ERROR_SUCCESS.0 {
                    m.engine = add_counter(q, r"\GPU Engine(*)\Utilization Percentage");
                    m.memory = add_counter(q, r"\GPU Adapter Memory(*)\Dedicated Usage");
                    m.query = Some(q);
                    PdhCollectQueryData(q); // rates need a baseline
                }
            }
            m
        }

        pub fn sample(&mut self) -> Vec<GpuInfo> {
            let mut load: HashMap<(u32, u32), HashMap<String, f64>> = HashMap::new();
            let mut mem: HashMap<(u32, u32), f64> = HashMap::new();
            if let Some(q) = self.query {
                unsafe {
                    PdhCollectQueryData(q);
                    if let Some(c) = self.engine {
                        for (name, v) in read_array(c) {
                            let Some(luid) = parse_luid(&name) else { continue };
                            let engtype = name
                                .rsplit_once("engtype_")
                                .map(|(_, t)| t.to_string())
                                .unwrap_or_default();
                            *load.entry(luid).or_default().entry(engtype).or_default() += v;
                        }
                    }
                    if let Some(c) = self.memory {
                        for (name, v) in read_array(c) {
                            if let Some(luid) = parse_luid(&name) {
                                *mem.entry(luid).or_default() += v;
                            }
                        }
                    }
                }
            }
            self.adapters
                .iter()
                .map(|a| GpuInfo {
                    name: a.name.clone(),
                    vendor: a.vendor.clone(),
                    usage: self.query.map(|_| {
                        a.luids
                            .iter()
                            .filter_map(|l| load.get(l))
                            .flat_map(|types| types.values().cloned())
                            .fold(0.0, f64::max)
                            .clamp(0.0, 100.0) as f32
                    }),
                    memory_used: a.luids.iter().filter_map(|l| mem.get(l)).cloned().reduce(f64::max).map(|m| m as u64),
                    memory_total: Some(a.dedicated),
                    temperature_c: None,
                })
                .collect()
        }
    }

    impl Drop for GpuMonitor {
        fn drop(&mut self) {
            if let Some(q) = self.query.take() {
                unsafe {
                    PdhCloseQuery(q);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn luid() {
            assert_eq!(
                super::parse_luid("pid_1_luid_0x00000000_0x0574BFA1_phys_0_eng_0_engtype_3D"),
                Some((0, 0x0574BFA1))
            );
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::GpuInfo;
    pub struct GpuMonitor;
    impl GpuMonitor {
        pub fn new() -> Self {
            GpuMonitor
        }
        pub fn sample(&mut self) -> Vec<GpuInfo> {
            Vec::new()
        }
    }
}

pub use imp::GpuMonitor;
