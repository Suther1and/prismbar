use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::sync::{Mutex, OnceLock};
use sysinfo::{ProcessesToUpdate, System};
use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::icon_cache::IconCache;
use crate::win_shell::wide;
use crate::windows_tracker::{
    extract_icon_for_path, extract_icon_for_path_cached, friendly_exe_name, terminate_process_pids,
};

const BYTES_PER_GIB: f64 = 1_073_741_824.0;
const NVML_SUCCESS: i32 = 0;
const NVML_ERROR_INSUFFICIENT_SIZE: i32 = 7;
const NVML_VALUE_NOT_AVAILABLE_U64: u64 = u64::MAX;
const DEFAULT_PROCESS_LIMIT: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TopProcessInfo {
    pub pid: u32,
    pub pids: Vec<u32>,
    pub name: String,
    pub mem_gb: f32,
    pub vram_gb: f32,
    pub uses_gpu: bool,
    pub cpu_pct: u32,
    pub icon_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryState {
    pub cpu_name: String,
    pub cpu_pct: u32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub ram_pct: u32,
    pub gpu_available: bool,
    pub gpu_name: String,
    pub gpu_pct: u32,
    pub gpu_temp_c: u32,
    pub vram_used_gb: f32,
    pub vram_total_gb: f32,
    pub vram_pct: u32,
    pub top_processes: Vec<TopProcessInfo>,
    pub gpu_processes: Vec<TopProcessInfo>,
}

#[derive(Debug, Clone)]
pub struct RawProcessSample {
    pub pid: u32,
    pub raw_name: String,
    pub exe_path: Option<String>,
    pub mem_bytes: u64,
    pub cpu_pct_normalized: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuProcessSample {
    pub vram_bytes: u64,
    pub active_context: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GpuTelemetrySnapshot {
    pub available: bool,
    pub name: String,
    pub gpu_pct: u32,
    pub temp_c: u32,
    pub vram_used_gb: f32,
    pub vram_total_gb: f32,
    pub vram_pct: u32,
    pub per_pid_gpu: HashMap<u32, GpuProcessSample>,
}

impl GpuTelemetrySnapshot {
    pub fn unavailable() -> Self {
        Self {
            available: false,
            name: "GPU".to_string(),
            gpu_pct: 0,
            temp_c: 0,
            vram_used_gb: 0.0,
            vram_total_gb: 0.0,
            vram_pct: 0,
            per_pid_gpu: HashMap::new(),
        }
    }
}

// Dynamic NVML bindings (zero build dependency on CUDA SDK — loads nvml.dll directly from NVIDIA driver)
#[repr(C)]
struct NvmlUtilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvmlProcessInfoV1 {
    pid: u32,
    used_gpu_memory: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NvmlProcessInfoV2 {
    pid: u32,
    used_gpu_memory: u64,
    gpu_instance_id: u32,
    compute_instance_id: u32,
}

type NvmlDevice = *mut std::ffi::c_void;
type FnInit = unsafe extern "C" fn() -> i32;
type FnGetHandle = unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32;
type FnGetName = unsafe extern "C" fn(NvmlDevice, *mut u8, u32) -> i32;
type FnGetUtil = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> i32;
type FnGetTemp = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;
type FnGetMem = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> i32;
type FnGetRunningProcsV2 = unsafe extern "C" fn(NvmlDevice, *mut u32, *mut NvmlProcessInfoV2) -> i32;
type FnGetRunningProcsV1 = unsafe extern "C" fn(NvmlDevice, *mut u32, *mut NvmlProcessInfoV1) -> i32;

#[derive(Clone, Copy)]
enum NvmlProcQueryFn {
    V2(FnGetRunningProcsV2),
    V1(FnGetRunningProcsV1),
}

struct NvmlApi {
    device: NvmlDevice,
    gpu_name: String,
    get_util: FnGetUtil,
    get_temp: FnGetTemp,
    get_mem: FnGetMem,
    get_graphics_procs: Option<NvmlProcQueryFn>,
    get_compute_procs: Option<NvmlProcQueryFn>,
}

unsafe impl Send for NvmlApi {}
unsafe impl Sync for NvmlApi {}

static NVML_API: OnceLock<Option<NvmlApi>> = OnceLock::new();
static SYS_STATE: OnceLock<Mutex<System>> = OnceLock::new();

fn round_gb_1(bytes: u64) -> f32 {
    (((bytes as f64 / BYTES_PER_GIB) * 10.0).round() / 10.0) as f32
}

fn sanitize_nvml_vram_bytes(raw: u64) -> u64 {
    if raw == NVML_VALUE_NOT_AVAILABLE_U64 {
        0
    } else {
        raw
    }
}

/// Merges NVML graphics and compute running-process lists per PID cleanly,
/// treating `NVML_VALUE_NOT_AVAILABLE` (`u64::MAX`) as `0` bytes while preserving `active_context`.
pub fn merge_nvml_process_lists(
    graphics: &[(u32, u64)],
    compute: &[(u32, u64)],
) -> HashMap<u32, GpuProcessSample> {
    let mut map: HashMap<u32, GpuProcessSample> = HashMap::new();
    for &(pid, raw_bytes) in graphics.iter().chain(compute.iter()) {
        if pid == 0 {
            continue;
        }
        let clean_bytes = sanitize_nvml_vram_bytes(raw_bytes);
        let entry = map.entry(pid).or_default();
        entry.active_context = true;
        entry.vram_bytes = entry.vram_bytes.saturating_add(clean_bytes);
    }
    map
}

fn is_excluded_telemetry_process(raw_name: &str) -> bool {
    let low = raw_name.trim().to_ascii_lowercase();
    matches!(
        low.as_str(),
        ""
            | "system"
            | "system idle process"
            | "registry"
            | "memory compression"
            | "idle"
            | "svchost.exe"
            | "dwm.exe"
            | "testbar.exe"
    )
}

/// Pure multi-PID process-group aggregator producing BOTH:
/// - `top_processes`: top RAM consumers for Work («Кто сейчас ест память»)
/// - `gpu_processes`: top GPU VRAM + active GPU / RAM consumers for Studio («Активные процессы GPU / ОЗУ»)
pub fn aggregate_process_groups<FIcon>(
    samples: &[RawProcessSample],
    gpu_by_pid: &HashMap<u32, GpuProcessSample>,
    limit: usize,
    mut resolve_icon: FIcon,
) -> (Vec<TopProcessInfo>, Vec<TopProcessInfo>)
where
    FIcon: FnMut(&str) -> Option<String>,
{
    struct GroupAcc {
        display_name: String,
        primary_pid: u32,
        primary_mem: u64,
        primary_vram: u64,
        pids: HashSet<u32>,
        total_mem: u64,
        total_vram: u64,
        uses_gpu: bool,
        total_cpu: f32,
        exe_path: Option<String>,
    }

    let mut groups: HashMap<String, GroupAcc> = HashMap::new();

    for sample in samples {
        if sample.pid == 0 || is_excluded_telemetry_process(&sample.raw_name) {
            continue;
        }
        let nice_name = sample
            .exe_path
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(friendly_exe_name)
            .unwrap_or_else(|| {
                let trimmed = sample.raw_name.trim();
                trimmed
                    .strip_suffix(".exe")
                    .or_else(|| trimmed.strip_suffix(".EXE"))
                    .unwrap_or(trimmed)
                    .to_string()
            });
        if nice_name.trim().is_empty() {
            continue;
        }
        let group_key = nice_name.to_lowercase();
        let gpu_info = gpu_by_pid.get(&sample.pid).copied().unwrap_or_default();

        match groups.get_mut(&group_key) {
            Some(acc) => {
                acc.pids.insert(sample.pid);
                acc.total_mem = acc.total_mem.saturating_add(sample.mem_bytes);
                acc.total_vram = acc.total_vram.saturating_add(gpu_info.vram_bytes);
                acc.uses_gpu |= gpu_info.active_context || gpu_info.vram_bytes > 0;
                acc.total_cpu += sample.cpu_pct_normalized.max(0.0);
                if acc.exe_path.is_none() && sample.exe_path.is_some() {
                    acc.exe_path = sample.exe_path.clone();
                }
                if sample.mem_bytes > acc.primary_mem
                    || (sample.mem_bytes == acc.primary_mem
                        && gpu_info.vram_bytes > acc.primary_vram)
                {
                    acc.primary_pid = sample.pid;
                    acc.primary_mem = sample.mem_bytes;
                    acc.primary_vram = gpu_info.vram_bytes;
                }
            }
            None => {
                let mut pids = HashSet::new();
                pids.insert(sample.pid);
                groups.insert(
                    group_key,
                    GroupAcc {
                        display_name: nice_name,
                        primary_pid: sample.pid,
                        primary_mem: sample.mem_bytes,
                        primary_vram: gpu_info.vram_bytes,
                        pids,
                        total_mem: sample.mem_bytes,
                        total_vram: gpu_info.vram_bytes,
                        uses_gpu: gpu_info.active_context || gpu_info.vram_bytes > 0,
                        total_cpu: sample.cpu_pct_normalized.max(0.0),
                        exe_path: sample.exe_path.clone(),
                    },
                );
            }
        }
    }

    struct BuiltGroup {
        info: TopProcessInfo,
        mem_bytes: u64,
        vram_bytes: u64,
        exe_path: Option<String>,
    }

    let mut built: Vec<BuiltGroup> = groups
        .into_values()
        .map(|acc| {
            let mut pids: Vec<u32> = acc.pids.into_iter().collect();
            pids.sort_unstable();
            BuiltGroup {
                info: TopProcessInfo {
                    pid: acc.primary_pid,
                    pids,
                    name: acc.display_name,
                    mem_gb: round_gb_1(acc.total_mem),
                    vram_gb: round_gb_1(acc.total_vram),
                    uses_gpu: acc.uses_gpu,
                    cpu_pct: acc.total_cpu.round().clamp(0.0, 100.0) as u32,
                    icon_data: None,
                },
                mem_bytes: acc.total_mem,
                vram_bytes: acc.total_vram,
                exe_path: acc.exe_path,
            }
        })
        .collect();

    let mut icon_memo: HashMap<String, Option<String>> = HashMap::new();
    let mut get_icon = |exe: &Option<String>| -> Option<String> {
        let path = exe.as_deref()?;
        if let Some(cached) = icon_memo.get(path) {
            return cached.clone();
        }
        let resolved = resolve_icon(path);
        icon_memo.insert(path.to_string(), resolved.clone());
        resolved
    };

    // 1. RAM-sorted top processes (Work: "Кто сейчас ест память")
    built.sort_by(|a, b| {
        b.mem_bytes
            .cmp(&a.mem_bytes)
            .then_with(|| b.vram_bytes.cmp(&a.vram_bytes))
            .then_with(|| a.info.name.to_lowercase().cmp(&b.info.name.to_lowercase()))
    });
    let top_processes: Vec<TopProcessInfo> = built
        .iter()
        .take(limit)
        .map(|g| {
            let mut info = g.info.clone();
            info.icon_data = get_icon(&g.exe_path);
            info
        })
        .collect();

    // 2. GPU VRAM + active GPU context + RAM-sorted processes (Studio: "Активные процессы GPU / ОЗУ")
    built.sort_by(|a, b| {
        (b.vram_bytes > 0)
            .cmp(&(a.vram_bytes > 0))
            .then_with(|| b.vram_bytes.cmp(&a.vram_bytes))
            .then_with(|| b.info.uses_gpu.cmp(&a.info.uses_gpu))
            .then_with(|| b.mem_bytes.cmp(&a.mem_bytes))
            .then_with(|| a.info.name.to_lowercase().cmp(&b.info.name.to_lowercase()))
    });
    let gpu_processes: Vec<TopProcessInfo> = built
        .iter()
        .take(limit)
        .map(|g| {
            let mut info = g.info.clone();
            info.icon_data = get_icon(&g.exe_path);
            info
        })
        .collect();

    (top_processes, gpu_processes)
}

/// Pure assembler turning CPU/RAM metrics, optional NVML snapshot, and process samples into `TelemetryState`.
/// Gracefully handles `gpu == None` (NVML absent) without crashing or fabricating GPU data.
pub fn build_telemetry_state<FIcon>(
    cpu_name: String,
    cpu_pct: u32,
    ram_used_bytes: u64,
    ram_total_bytes: u64,
    gpu: Option<GpuTelemetrySnapshot>,
    samples: &[RawProcessSample],
    resolve_icon: FIcon,
) -> TelemetryState
where
    FIcon: FnMut(&str) -> Option<String>,
{
    let total_ram = ram_total_bytes.max(1);
    let ram_total_gb = (total_ram as f64 / BYTES_PER_GIB).round() as f32;
    let ram_used_gb = round_gb_1(ram_used_bytes);
    let ram_pct = ((ram_used_bytes as f64 / total_ram as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u32;

    let gpu_snap = gpu.unwrap_or_else(GpuTelemetrySnapshot::unavailable);
    let (top_processes, gpu_processes) = aggregate_process_groups(
        samples,
        &gpu_snap.per_pid_gpu,
        DEFAULT_PROCESS_LIMIT,
        resolve_icon,
    );

    TelemetryState {
        cpu_name,
        cpu_pct: cpu_pct.min(100),
        ram_used_gb,
        ram_total_gb,
        ram_pct,
        gpu_available: gpu_snap.available,
        gpu_name: gpu_snap.name,
        gpu_pct: gpu_snap.gpu_pct.min(100),
        gpu_temp_c: gpu_snap.temp_c,
        vram_used_gb: gpu_snap.vram_used_gb,
        vram_total_gb: gpu_snap.vram_total_gb,
        vram_pct: gpu_snap.vram_pct.min(100),
        top_processes,
        gpu_processes,
    }
}

fn init_nvml() -> Option<NvmlApi> {
    unsafe {
        let candidates = [
            "nvml.dll",
            r"C:\Windows\System32\nvml.dll",
            r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll",
        ];
        let mut hmod = None;
        for c in candidates {
            let w = wide(c);
            if let Ok(m) = LoadLibraryW(windows::core::PCWSTR(w.as_ptr())) {
                if !m.is_invalid() {
                    hmod = Some(m);
                    break;
                }
            }
        }
        let hmod = hmod?;

        let get_fn = |name: &str| -> Option<*const ()> {
            let cs = CString::new(name).ok()?;
            GetProcAddress(hmod, PCSTR(cs.as_ptr() as *const u8)).map(|f| f as *const ())
        };

        let init: FnInit =
            std::mem::transmute(get_fn("nvmlInit_v2").or_else(|| get_fn("nvmlInit"))?);
        if init() != NVML_SUCCESS {
            return None;
        }
        let get_handle: FnGetHandle = std::mem::transmute(
            get_fn("nvmlDeviceGetHandleByIndex_v2")
                .or_else(|| get_fn("nvmlDeviceGetHandleByIndex"))?,
        );
        let get_name: FnGetName = std::mem::transmute(get_fn("nvmlDeviceGetName")?);
        let get_util: FnGetUtil = std::mem::transmute(get_fn("nvmlDeviceGetUtilizationRates")?);
        let get_temp: FnGetTemp = std::mem::transmute(get_fn("nvmlDeviceGetTemperature")?);
        let get_mem: FnGetMem = std::mem::transmute(get_fn("nvmlDeviceGetMemoryInfo")?);

        let get_graphics_procs = get_fn("nvmlDeviceGetGraphicsRunningProcesses_v3")
            .or_else(|| get_fn("nvmlDeviceGetGraphicsRunningProcesses_v2"))
            .map(|p| NvmlProcQueryFn::V2(std::mem::transmute(p)))
            .or_else(|| {
                get_fn("nvmlDeviceGetGraphicsRunningProcesses")
                    .map(|p| NvmlProcQueryFn::V1(std::mem::transmute(p)))
            });

        let get_compute_procs = get_fn("nvmlDeviceGetComputeRunningProcesses_v3")
            .or_else(|| get_fn("nvmlDeviceGetComputeRunningProcesses_v2"))
            .map(|p| NvmlProcQueryFn::V2(std::mem::transmute(p)))
            .or_else(|| {
                get_fn("nvmlDeviceGetComputeRunningProcesses")
                    .map(|p| NvmlProcQueryFn::V1(std::mem::transmute(p)))
            });

        let mut dev: NvmlDevice = std::ptr::null_mut();
        if get_handle(0, &mut dev) != NVML_SUCCESS || dev.is_null() {
            return None;
        }

        let mut name_buf = [0u8; 96];
        let gpu_name =
            if get_name(dev, name_buf.as_mut_ptr(), name_buf.len() as u32) == NVML_SUCCESS {
                let end = name_buf
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(name_buf.len());
                let raw = String::from_utf8_lossy(&name_buf[..end]).to_string();
                raw.replace("NVIDIA GeForce ", "").replace("NVIDIA ", "")
            } else {
                "GPU".to_string()
            };

        Some(NvmlApi {
            device: dev,
            gpu_name,
            get_util,
            get_temp,
            get_mem,
            get_graphics_procs,
            get_compute_procs,
        })
    }
}

unsafe fn query_nvml_running_procs(
    device: NvmlDevice,
    func: NvmlProcQueryFn,
) -> Vec<(u32, u64)> {
    match func {
        NvmlProcQueryFn::V2(f) => {
            let mut count: u32 = 64;
            let mut buf = vec![NvmlProcessInfoV2::default(); count as usize];
            let mut rc = f(device, &mut count, buf.as_mut_ptr());
            if rc == NVML_ERROR_INSUFFICIENT_SIZE && count > buf.len() as u32 && count <= 512 {
                buf.resize(count as usize, NvmlProcessInfoV2::default());
                rc = f(device, &mut count, buf.as_mut_ptr());
            }
            if rc != NVML_SUCCESS {
                return Vec::new();
            }
            let n = (count as usize).min(buf.len());
            buf[..n]
                .iter()
                .filter(|p| p.pid != 0)
                .map(|p| (p.pid, p.used_gpu_memory))
                .collect()
        }
        NvmlProcQueryFn::V1(f) => {
            let mut count: u32 = 64;
            let mut buf = vec![NvmlProcessInfoV1::default(); count as usize];
            let mut rc = f(device, &mut count, buf.as_mut_ptr());
            if rc == NVML_ERROR_INSUFFICIENT_SIZE && count > buf.len() as u32 && count <= 512 {
                buf.resize(count as usize, NvmlProcessInfoV1::default());
                rc = f(device, &mut count, buf.as_mut_ptr());
            }
            if rc != NVML_SUCCESS {
                return Vec::new();
            }
            let n = (count as usize).min(buf.len());
            buf[..n]
                .iter()
                .filter(|p| p.pid != 0)
                .map(|p| (p.pid, p.used_gpu_memory))
                .collect()
        }
    }
}

fn query_gpu() -> Option<GpuTelemetrySnapshot> {
    let api = NVML_API.get_or_init(init_nvml).as_ref()?;
    unsafe {
        let mut util = NvmlUtilization { gpu: 0, memory: 0 };
        let mut temp = 0u32;
        let mut mem = NvmlMemory {
            total: 0,
            free: 0,
            used: 0,
        };
        let _ = (api.get_util)(api.device, &mut util);
        let _ = (api.get_temp)(api.device, 0, &mut temp);
        let _ = (api.get_mem)(api.device, &mut mem);

        let graphics = api
            .get_graphics_procs
            .map(|f| query_nvml_running_procs(api.device, f))
            .unwrap_or_default();
        let compute = api
            .get_compute_procs
            .map(|f| query_nvml_running_procs(api.device, f))
            .unwrap_or_default();
        let per_pid_gpu = merge_nvml_process_lists(&graphics, &compute);

        let used_gb = round_gb_1(mem.used);
        let total_gb = (mem.total as f64 / BYTES_PER_GIB).round() as f32;
        let pct = if mem.total > 0 {
            ((mem.used as f64 / mem.total as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u32
        } else {
            0
        };

        Some(GpuTelemetrySnapshot {
            available: true,
            name: api.gpu_name.clone(),
            gpu_pct: util.gpu.min(100),
            temp_c: temp,
            vram_used_gb: used_gb,
            vram_total_gb: total_gb,
            vram_pct: pct,
            per_pid_gpu,
        })
    }
}

fn collect_telemetry_with_icon_resolver<FIcon>(resolve_icon: FIcon) -> TelemetryState
where
    FIcon: FnMut(&str) -> Option<String>,
{
    let sys_mutex = SYS_STATE.get_or_init(|| {
        let mut s = System::new_all();
        s.refresh_all();
        Mutex::new(s)
    });

    let (cpu_name, cpu_pct, ram_used_bytes, ram_total_bytes, samples) = {
        let mut sys = sys_mutex.lock().unwrap_or_else(|e| e.into_inner());
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes(ProcessesToUpdate::All, true);

        let cpu_name = sys
            .cpus()
            .first()
            .map(|c| {
                let b = c.brand().trim();
                if b.contains("Ryzen 9") {
                    "Ryzen 9".to_string()
                } else if b.contains("Ryzen 7") {
                    "Ryzen 7".to_string()
                } else if b.contains("Core") {
                    b.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
                } else {
                    b.chars().take(18).collect()
                }
            })
            .unwrap_or_else(|| "CPU".to_string());

        let cpu_pct = sys.global_cpu_usage().round().clamp(0.0, 100.0) as u32;
        let total_ram_bytes = sys.total_memory().max(1);
        let used_ram_bytes = sys.used_memory();
        let cpu_count = sys.cpus().len().max(1) as f32;

        let samples: Vec<RawProcessSample> = sys
            .processes()
            .iter()
            .map(|(pid, proc_)| RawProcessSample {
                pid: pid.as_u32(),
                raw_name: proc_.name().to_string_lossy().to_string(),
                exe_path: proc_.exe().map(|p| p.to_string_lossy().to_string()),
                mem_bytes: proc_.memory(),
                cpu_pct_normalized: proc_.cpu_usage() / cpu_count,
            })
            .collect();

        (cpu_name, cpu_pct, used_ram_bytes, total_ram_bytes, samples)
    };

    let gpu = query_gpu();
    build_telemetry_state(
        cpu_name,
        cpu_pct,
        ram_used_bytes,
        ram_total_bytes,
        gpu,
        &samples,
        resolve_icon,
    )
}

pub fn get_telemetry_cached(cache: &Mutex<IconCache>) -> TelemetryState {
    collect_telemetry_with_icon_resolver(|exe| extract_icon_for_path_cached(exe, cache))
}

pub fn get_telemetry() -> TelemetryState {
    collect_telemetry_with_icon_resolver(extract_icon_for_path)
}

/// Terminates ALL PIDs belonging to a multi-process app group and propagates OS permission errors.
pub fn kill_process_group(pids: &[u32]) -> Result<(), String> {
    terminate_process_pids(pids)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1_073_741_824;

    fn sample(
        pid: u32,
        raw_name: &str,
        exe_path: Option<&str>,
        mem_gib: f64,
        cpu_pct: f32,
    ) -> RawProcessSample {
        RawProcessSample {
            pid,
            raw_name: raw_name.to_string(),
            exe_path: exe_path.map(str::to_string),
            mem_bytes: (mem_gib * GIB as f64) as u64,
            cpu_pct_normalized: cpu_pct,
        }
    }

    #[test]
    fn aggregates_multi_pid_groups_and_ranks_gpu_vram_separately_from_ram() {
        let samples = vec![
            // Excluded OS/TestBar processes
            sample(4, "System", None, 2.0, 1.0),
            sample(900, "testbar.exe", Some(r"C:\Apps\testbar.exe"), 0.5, 0.5),
            sample(901, "dwm.exe", Some(r"C:\Windows\System32\dwm.exe"), 1.0, 2.0),
            // Chrome with 3 PIDs -> 6.0 GiB total RAM, 0.4 GiB GPU VRAM on PID 102
            sample(101, "chrome.exe", Some(r"C:\Program Files\Google\Chrome\chrome.exe"), 1.2, 4.0),
            sample(102, "chrome.exe", Some(r"C:\Program Files\Google\Chrome\chrome.exe"), 3.3, 6.0),
            sample(103, "chrome.exe", Some(r"C:\Program Files\Google\Chrome\chrome.exe"), 1.5, 2.0),
            // Blender with 1 PID -> 2.0 GiB RAM, 8.5 GiB GPU VRAM (graphics + compute merged)
            sample(201, "blender.exe", Some(r"C:\Program Files\Blender\blender.exe"), 2.0, 18.0),
            // VS Code with 2 PIDs -> 4.0 GiB RAM, active GPU context (WDDM NVML_VALUE_NOT_AVAILABLE -> 0 VRAM)
            sample(301, "Code.exe", Some(r"C:\Apps\VS Code\Code.exe"), 2.5, 5.0),
            sample(302, "Code.exe", Some(r"C:\Apps\VS Code\Code.exe"), 1.5, 1.0),
            // Pure RAM app (Obsidian) -> 3.0 GiB RAM, no GPU
            sample(401, "Obsidian.exe", Some(r"C:\Apps\Obsidian\Obsidian.exe"), 3.0, 1.0),
        ];

        let gpu_map = merge_nvml_process_lists(
            &[
                (102, (0.4 * GIB as f64) as u64),
                (201, (2.5 * GIB as f64) as u64),
                (301, NVML_VALUE_NOT_AVAILABLE_U64),
            ],
            &[(201, (6.0 * GIB as f64) as u64)],
        );

        assert_eq!(gpu_map.get(&301).unwrap().vram_bytes, 0);
        assert!(gpu_map.get(&301).unwrap().active_context);
        assert_eq!(
            round_gb_1(gpu_map.get(&201).unwrap().vram_bytes),
            8.5
        );

        let (top_ram, gpu_procs) =
            aggregate_process_groups(&samples, &gpu_map, 5, |exe| Some(format!("icon:{exe}")));

        // Work RAM order: Chrome (6.0) > VS Code (4.0) > Obsidian (3.0) > blender (2.0)
        let ram_names: Vec<&str> = top_ram.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(ram_names, vec!["Chrome", "VS Code", "Obsidian", "blender"]);

        // Chrome aggregates all 3 PIDs and chooses largest-mem PID 102 as primary
        assert_eq!(top_ram[0].pid, 102);
        assert_eq!(top_ram[0].pids, vec![101, 102, 103]);
        assert_eq!(top_ram[0].mem_gb, 6.0);
        assert_eq!(top_ram[0].vram_gb, 0.4);
        assert!(top_ram[0].uses_gpu);
        assert_eq!(top_ram[0].cpu_pct, 12);

        // Studio GPU/RAM order: blender (8.5 VRAM) > Chrome (0.4 VRAM) > VS Code (active GPU ctx) > Obsidian (RAM fallback)
        let gpu_names: Vec<&str> = gpu_procs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(gpu_names, vec!["blender", "Chrome", "VS Code", "Obsidian"]);
        assert_eq!(gpu_procs[0].vram_gb, 8.5);
        assert_eq!(gpu_procs[0].mem_gb, 2.0);
        assert!(gpu_procs[0].uses_gpu);
        assert!(gpu_procs[2].uses_gpu);
        assert_eq!(gpu_procs[2].vram_gb, 0.0);
        assert!(!gpu_procs[3].uses_gpu);
    }

    #[test]
    fn nvml_absent_fallback_preserves_cpu_ram_and_populates_gpu_processes_gracefully() {
        let samples = vec![
            sample(501, "zen.exe", Some(r"C:\Program Files\Zen\zen.exe"), 3.2, 8.0),
            sample(502, "zen.exe", Some(r"C:\Program Files\Zen\zen.exe"), 1.3, 4.0),
            sample(601, "Telegram.exe", Some(r"C:\Apps\Telegram.exe"), 0.8, 1.0),
        ];

        let state = build_telemetry_state(
            "Ryzen 9".to_string(),
            34,
            24 * GIB,
            64 * GIB,
            None, // NVML unavailable
            &samples,
            |_| None,
        );

        assert!(!state.gpu_available);
        assert_eq!(state.gpu_name, "GPU");
        assert_eq!(state.gpu_pct, 0);
        assert_eq!(state.gpu_temp_c, 0);
        assert_eq!(state.vram_used_gb, 0.0);
        assert_eq!(state.vram_total_gb, 0.0);
        assert_eq!(state.ram_used_gb, 24.0);
        assert_eq!(state.ram_total_gb, 64.0);
        assert_eq!(state.ram_pct, 38);

        assert_eq!(state.top_processes.len(), 2);
        assert_eq!(state.top_processes[0].name, "Zen Browser");
        assert_eq!(state.top_processes[0].pids, vec![501, 502]);
        assert_eq!(state.top_processes[0].mem_gb, 4.5);
        assert_eq!(state.top_processes[0].vram_gb, 0.0);
        assert!(!state.top_processes[0].uses_gpu);

        // Studio gpu_processes still degrades gracefully to RAM-ranked processes
        assert_eq!(state.gpu_processes.len(), 2);
        assert_eq!(state.gpu_processes[0].name, "Zen Browser");
    }
}
