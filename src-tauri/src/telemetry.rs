use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{
    CpuRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind,
    System, UpdateKind,
};
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

use crate::icon_cache::{self, IconCache};
use crate::win_shell::wide;
use crate::windows_tracker::{
    extract_icon_for_path, extract_icon_for_path_cached, friendly_exe_name, hicon_to_data_url,
    terminate_process_pids,
};

const BYTES_PER_GIB: f64 = 1_073_741_824.0;
const NVML_SUCCESS: i32 = 0;
const NVML_ERROR_UNINITIALIZED: i32 = 1;
const NVML_ERROR_INSUFFICIENT_SIZE: i32 = 7;
const NVML_ERROR_GPU_IS_LOST: i32 = 15;
const NVML_ERROR_RESET_REQUIRED: i32 = 18;
const NVML_VALUE_NOT_AVAILABLE_U64: u64 = u64::MAX;
const NVML_TDR_RETRY_COOLDOWN: Duration = Duration::from_secs(15);
const DISK_CACHE_TTL: Duration = Duration::from_secs(30);
const TELEMETRY_SAMPLE_MIN_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_PROCESS_LIMIT: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiskHeadroomInfo {
    pub mount: String,
    pub name: String,
    pub used_gb: f32,
    pub total_gb: f32,
    pub free_gb: f32,
    pub used_pct: u8,
}

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
    #[serde(default)]
    pub icon_key: Option<String>,
    #[serde(default)]
    pub hwnd: Option<isize>,
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
    #[serde(default)]
    pub disks: Vec<DiskHeadroomInfo>,
}

/// Alias matching Wave 1B naming while preserving full compatibility with `TelemetryState`.
pub type TelemetrySnapshot = TelemetryState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDiskSample {
    pub mount: String,
    pub name: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
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
type FnShutdown = unsafe extern "C" fn() -> i32;
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
    hmod: HMODULE,
    shutdown: Option<FnShutdown>,
    device: NvmlDevice,
    gpu_name: String,
    get_util: FnGetUtil,
    get_temp: FnGetTemp,
    get_mem: FnGetMem,
    get_graphics_procs: Option<NvmlProcQueryFn>,
    get_compute_procs: Option<NvmlProcQueryFn>,
}

impl NvmlApi {
    fn teardown(self) {
        unsafe {
            if let Some(shutdown) = self.shutdown {
                let _ = shutdown();
            }
            if !self.hmod.is_invalid() {
                let _ = FreeLibrary(self.hmod);
            }
        }
    }
}

unsafe impl Send for NvmlApi {}
unsafe impl Sync for NvmlApi {}

#[derive(Default)]
struct NvmlHolder {
    api: Option<NvmlApi>,
    last_init_attempt: Option<Instant>,
    permanently_unavailable: bool,
}

enum NvmlInitOutcome {
    Ready(NvmlApi),
    NotInstalled,
    TransientFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvmlQueryHealth {
    Healthy,
    PartialDegraded,
    LostNeedsReset,
}

/// Returns `true` when an NVML status code indicates driver TDR/reset or lost GPU context.
pub fn is_nvml_fatal_code(rc: i32) -> bool {
    matches!(
        rc,
        NVML_ERROR_UNINITIALIZED | NVML_ERROR_GPU_IS_LOST | NVML_ERROR_RESET_REQUIRED
    )
}

/// Classifies the trio of primary NVML polling return codes (`util`, `temp`, `mem`).
pub fn classify_nvml_query_codes(rc_util: i32, rc_temp: i32, rc_mem: i32) -> NvmlQueryHealth {
    if is_nvml_fatal_code(rc_util) || is_nvml_fatal_code(rc_temp) || is_nvml_fatal_code(rc_mem) {
        return NvmlQueryHealth::LostNeedsReset;
    }
    if rc_util != NVML_SUCCESS && rc_temp != NVML_SUCCESS && rc_mem != NVML_SUCCESS {
        return NvmlQueryHealth::LostNeedsReset;
    }
    if rc_util == NVML_SUCCESS && rc_temp == NVML_SUCCESS && rc_mem == NVML_SUCCESS {
        NvmlQueryHealth::Healthy
    } else {
        NvmlQueryHealth::PartialDegraded
    }
}

struct CachedDiskHeadroom {
    disks: Vec<DiskHeadroomInfo>,
    refreshed_at: Instant,
}

#[derive(Clone)]
struct CachedRawTelemetry {
    refreshed_at: Instant,
    cpu_name: String,
    cpu_pct: u32,
    ram_used_bytes: u64,
    ram_total_bytes: u64,
    samples: Vec<RawProcessSample>,
    gpu: Option<GpuTelemetrySnapshot>,
}

struct TelemetrySamplerState {
    sys: System,
    cached: Option<CachedRawTelemetry>,
}

static NVML_STATE: OnceLock<Mutex<NvmlHolder>> = OnceLock::new();
static SYS_STATE: OnceLock<Mutex<TelemetrySamplerState>> = OnceLock::new();
static DISK_CACHE: OnceLock<Mutex<Option<CachedDiskHeadroom>>> = OnceLock::new();

fn round_gb_1(bytes: u64) -> f32 {
    (((bytes as f64 / BYTES_PER_GIB) * 10.0).round() / 10.0) as f32
}

fn normalize_disk_mount(raw: &str) -> String {
    let trimmed = raw.trim().replace('/', "\\");
    if trimmed.len() >= 2 && trimmed.as_bytes()[1] == b':' {
        trimmed.chars().take(2).collect::<String>().to_ascii_uppercase()
    } else {
        trimmed
    }
}

/// Pure storage-headroom builder turning raw mounted-volume samples into sorted `DiskHeadroomInfo`
/// entries (`C:` first, then alphabetical), skipping zero-byte pseudo-volumes and deduplicating mounts.
pub fn build_disk_headroom_list(raw: &[RawDiskSample]) -> Vec<DiskHeadroomInfo> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<DiskHeadroomInfo> = Vec::new();

    for sample in raw {
        if sample.total_bytes == 0 {
            continue;
        }
        let mount = normalize_disk_mount(&sample.mount);
        if mount.is_empty() {
            continue;
        }
        let key = mount.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }

        let free_bytes = sample.available_bytes.min(sample.total_bytes);
        let used_bytes = sample.total_bytes.saturating_sub(free_bytes);
        let used_pct = ((used_bytes as f64 / sample.total_bytes as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        let label = sample.name.trim();
        let name = if label.is_empty() {
            mount.clone()
        } else {
            label.to_string()
        };

        out.push(DiskHeadroomInfo {
            mount,
            name,
            used_gb: round_gb_1(used_bytes),
            total_gb: round_gb_1(sample.total_bytes),
            free_gb: round_gb_1(free_bytes),
            used_pct,
        });
    }

    out.sort_by(|a, b| {
        let a_is_c = a.mount.eq_ignore_ascii_case("C:");
        let b_is_c = b.mount.eq_ignore_ascii_case("C:");
        b_is_c
            .cmp(&a_is_c)
            .then_with(|| a.mount.to_ascii_lowercase().cmp(&b.mount.to_ascii_lowercase()))
    });
    out
}

/// Returns mounted disk headroom refreshed at most once every 30 seconds so telemetry polling causes zero disk thrash.
pub fn get_disks_headroom_cached() -> Vec<DiskHeadroomInfo> {
    let cache = DISK_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.as_ref() {
            if entry.refreshed_at.elapsed() < DISK_CACHE_TTL {
                return entry.disks.clone();
            }
        }
    }

    let disks = Disks::new_with_refreshed_list();
    let samples: Vec<RawDiskSample> = disks
        .iter()
        .map(|d| RawDiskSample {
            mount: d.mount_point().to_string_lossy().to_string(),
            name: d.name().to_string_lossy().to_string(),
            total_bytes: d.total_space(),
            available_bytes: d.available_space(),
        })
        .collect();
    let built = build_disk_headroom_list(&samples);

    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(CachedDiskHeadroom {
        disks: built.clone(),
        refreshed_at: Instant::now(),
    });
    built
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
    if low.starts_with("testbar") && low.ends_with(".exe") {
        return true;
    }
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
                    icon_key: None,
                    hwnd: None,
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

fn resolve_group_hwnd(primary_pid: u32, pids: &[u32], pid_to_hwnd: &HashMap<u32, isize>) -> Option<isize> {
    if let Some(&hwnd) = pid_to_hwnd.get(&primary_pid) {
        if hwnd != 0 {
            return Some(hwnd);
        }
    }
    pids.iter()
        .find_map(|pid| pid_to_hwnd.get(pid).copied().filter(|&h| h != 0))
}

/// Pure multi-PID process-group aggregator for Wave 1B icon-key IPC (`AD-6`):
/// populates `icon_key` (leaving `icon_data: None`), maps focusable `hwnd` via `pid_to_hwnd`,
/// and returns deduplicated `(icon_key, exe_path)` source registrations for cache reload on LRU eviction.
pub fn aggregate_process_groups_with_icons<FKey>(
    samples: &[RawProcessSample],
    gpu_by_pid: &HashMap<u32, GpuProcessSample>,
    pid_to_hwnd: &HashMap<u32, isize>,
    limit: usize,
    mut resolve_icon_key: FKey,
) -> (Vec<TopProcessInfo>, Vec<TopProcessInfo>, Vec<(String, String)>)
where
    FKey: FnMut(&str) -> Option<String>,
{
    let mut exe_by_name_lower: HashMap<String, String> = HashMap::new();
    for s in samples {
        if s.pid == 0 || is_excluded_telemetry_process(&s.raw_name) {
            continue;
        }
        if let Some(exe) = s.exe_path.as_deref().filter(|p| !p.trim().is_empty()) {
            let key = friendly_exe_name(exe).to_lowercase();
            exe_by_name_lower
                .entry(key)
                .or_insert_with(|| exe.trim().to_string());
        }
    }

    let (mut top_processes, mut gpu_processes) =
        aggregate_process_groups(samples, gpu_by_pid, limit, |_| None);

    let mut memo: HashMap<String, Option<String>> = HashMap::new();
    let mut seen_sources: HashSet<String> = HashSet::new();
    let mut icon_sources: Vec<(String, String)> = Vec::new();

    let mut enrich = |item: &mut TopProcessInfo| {
        item.icon_data = None;
        item.hwnd = resolve_group_hwnd(item.pid, &item.pids, pid_to_hwnd);
        let group_key = item.name.to_lowercase();
        let Some(exe_path) = exe_by_name_lower.get(&group_key) else {
            return;
        };
        let resolved_key = if let Some(cached) = memo.get(exe_path) {
            cached.clone()
        } else {
            let k = resolve_icon_key(exe_path);
            memo.insert(exe_path.clone(), k.clone());
            k
        };
        if let Some(k) = resolved_key {
            if seen_sources.insert(k.clone()) {
                icon_sources.push((k.clone(), exe_path.clone()));
            }
            item.icon_key = Some(k);
        }
    };

    for item in &mut top_processes {
        enrich(item);
    }
    for item in &mut gpu_processes {
        enrich(item);
    }

    (top_processes, gpu_processes, icon_sources)
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
        disks: Vec::new(),
    }
}

/// Pure assembler for icon-key + HWND + disk headroom telemetry snapshots.
pub fn build_telemetry_snapshot_with_icons<FKey>(
    cpu_name: String,
    cpu_pct: u32,
    ram_used_bytes: u64,
    ram_total_bytes: u64,
    gpu: Option<GpuTelemetrySnapshot>,
    samples: &[RawProcessSample],
    disks: Vec<DiskHeadroomInfo>,
    pid_to_hwnd: &HashMap<u32, isize>,
    resolve_icon_key: FKey,
) -> (TelemetrySnapshot, Vec<(String, String)>)
where
    FKey: FnMut(&str) -> Option<String>,
{
    let total_ram = ram_total_bytes.max(1);
    let ram_total_gb = (total_ram as f64 / BYTES_PER_GIB).round() as f32;
    let ram_used_gb = round_gb_1(ram_used_bytes);
    let ram_pct = ((ram_used_bytes as f64 / total_ram as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u32;

    let gpu_snap = gpu.unwrap_or_else(GpuTelemetrySnapshot::unavailable);
    let (top_processes, gpu_processes, icon_sources) = aggregate_process_groups_with_icons(
        samples,
        &gpu_snap.per_pid_gpu,
        pid_to_hwnd,
        DEFAULT_PROCESS_LIMIT,
        resolve_icon_key,
    );

    (
        TelemetrySnapshot {
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
            disks,
        },
        icon_sources,
    )
}

fn init_nvml() -> NvmlInitOutcome {
    unsafe {
        let candidates = [
            "nvml.dll",
            r"C:\Windows\System32\nvml.dll",
            r"C:\Program Files\NVIDIA Corporation\NVSMI\nvml.dll",
        ];
        let mut hmod = None;
        for c in candidates {
            let w = wide(c);
            if let Ok(m) = LoadLibraryW(PCWSTR(w.as_ptr())) {
                if !m.is_invalid() {
                    hmod = Some(m);
                    break;
                }
            }
        }
        let Some(hmod) = hmod else {
            return NvmlInitOutcome::NotInstalled;
        };

        let get_fn = |name: &str| -> Option<*const ()> {
            let cs = CString::new(name).ok()?;
            GetProcAddress(hmod, PCSTR(cs.as_ptr() as *const u8)).map(|f| f as *const ())
        };

        let shutdown: Option<FnShutdown> = get_fn("nvmlShutdown").map(|p| std::mem::transmute(p));

        let cleanup = |initialized: bool| {
            if initialized {
                if let Some(f) = shutdown {
                    let _ = f();
                }
            }
            let _ = FreeLibrary(hmod);
        };

        let Some(init_ptr) = get_fn("nvmlInit_v2").or_else(|| get_fn("nvmlInit")) else {
            cleanup(false);
            return NvmlInitOutcome::NotInstalled;
        };
        let init: FnInit = std::mem::transmute(init_ptr);
        if init() != NVML_SUCCESS {
            cleanup(false);
            return NvmlInitOutcome::TransientFailure;
        }

        let Some(handle_ptr) = get_fn("nvmlDeviceGetHandleByIndex_v2")
            .or_else(|| get_fn("nvmlDeviceGetHandleByIndex"))
        else {
            cleanup(true);
            return NvmlInitOutcome::NotInstalled;
        };
        let Some(name_ptr) = get_fn("nvmlDeviceGetName") else {
            cleanup(true);
            return NvmlInitOutcome::NotInstalled;
        };
        let Some(util_ptr) = get_fn("nvmlDeviceGetUtilizationRates") else {
            cleanup(true);
            return NvmlInitOutcome::NotInstalled;
        };
        let Some(temp_ptr) = get_fn("nvmlDeviceGetTemperature") else {
            cleanup(true);
            return NvmlInitOutcome::NotInstalled;
        };
        let Some(mem_ptr) = get_fn("nvmlDeviceGetMemoryInfo") else {
            cleanup(true);
            return NvmlInitOutcome::NotInstalled;
        };

        let get_handle: FnGetHandle = std::mem::transmute(handle_ptr);
        let get_name: FnGetName = std::mem::transmute(name_ptr);
        let get_util: FnGetUtil = std::mem::transmute(util_ptr);
        let get_temp: FnGetTemp = std::mem::transmute(temp_ptr);
        let get_mem: FnGetMem = std::mem::transmute(mem_ptr);

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
            cleanup(true);
            return NvmlInitOutcome::TransientFailure;
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

        NvmlInitOutcome::Ready(NvmlApi {
            hmod,
            shutdown,
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
) -> (Vec<(u32, u64)>, bool) {
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
                return (Vec::new(), is_nvml_fatal_code(rc));
            }
            let n = (count as usize).min(buf.len());
            (
                buf[..n]
                    .iter()
                    .filter(|p| p.pid != 0)
                    .map(|p| (p.pid, p.used_gpu_memory))
                    .collect(),
                false,
            )
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
                return (Vec::new(), is_nvml_fatal_code(rc));
            }
            let n = (count as usize).min(buf.len());
            (
                buf[..n]
                    .iter()
                    .filter(|p| p.pid != 0)
                    .map(|p| (p.pid, p.used_gpu_memory))
                    .collect(),
                false,
            )
        }
    }
}

fn query_gpu() -> Option<GpuTelemetrySnapshot> {
    let lock = NVML_STATE.get_or_init(|| Mutex::new(NvmlHolder::default()));
    let mut holder = lock.lock().unwrap_or_else(|e| e.into_inner());
    if holder.permanently_unavailable {
        return None;
    }

    if holder.api.is_none() {
        if let Some(last) = holder.last_init_attempt {
            if last.elapsed() < NVML_TDR_RETRY_COOLDOWN {
                return None;
            }
        }
        holder.last_init_attempt = Some(Instant::now());
        match init_nvml() {
            NvmlInitOutcome::Ready(api) => {
                holder.api = Some(api);
            }
            NvmlInitOutcome::NotInstalled => {
                holder.permanently_unavailable = true;
                return None;
            }
            NvmlInitOutcome::TransientFailure => {
                return None;
            }
        }
    }

    let (snapshot, needs_reset) = {
        let api = holder.api.as_ref()?;
        unsafe {
            let mut util = NvmlUtilization { gpu: 0, memory: 0 };
            let mut temp = 0u32;
            let mut mem = NvmlMemory {
                total: 0,
                free: 0,
                used: 0,
            };
            let rc_util = (api.get_util)(api.device, &mut util);
            let rc_temp = (api.get_temp)(api.device, 0, &mut temp);
            let rc_mem = (api.get_mem)(api.device, &mut mem);

            if classify_nvml_query_codes(rc_util, rc_temp, rc_mem) == NvmlQueryHealth::LostNeedsReset
            {
                (None, true)
            } else {
                let (graphics, gfx_lost) = api
                    .get_graphics_procs
                    .map(|f| query_nvml_running_procs(api.device, f))
                    .unwrap_or_else(|| (Vec::new(), false));
                let (compute, cmp_lost) = api
                    .get_compute_procs
                    .map(|f| query_nvml_running_procs(api.device, f))
                    .unwrap_or_else(|| (Vec::new(), false));

                if gfx_lost || cmp_lost {
                    (None, true)
                } else {
                    let per_pid_gpu = merge_nvml_process_lists(&graphics, &compute);
                    let (used_gb, total_gb, pct) = if rc_mem == NVML_SUCCESS {
                        let used_gb = round_gb_1(mem.used);
                        let total_gb = (mem.total as f64 / BYTES_PER_GIB).round() as f32;
                        let pct = if mem.total > 0 {
                            ((mem.used as f64 / mem.total as f64) * 100.0)
                                .round()
                                .clamp(0.0, 100.0) as u32
                        } else {
                            0
                        };
                        (used_gb, total_gb, pct)
                    } else {
                        (0.0, 0.0, 0)
                    };

                    (
                        Some(GpuTelemetrySnapshot {
                            available: true,
                            name: api.gpu_name.clone(),
                            gpu_pct: if rc_util == NVML_SUCCESS {
                                util.gpu.min(100)
                            } else {
                                0
                            },
                            temp_c: if rc_temp == NVML_SUCCESS { temp } else { 0 },
                            vram_used_gb: used_gb,
                            vram_total_gb: total_gb,
                            vram_pct: pct,
                            per_pid_gpu,
                        }),
                        false,
                    )
                }
            }
        }
    };

    if needs_reset {
        if let Some(old) = holder.api.take() {
            old.teardown();
        }
        holder.last_init_attempt = Some(Instant::now());
        return None;
    }

    snapshot
}

fn lean_process_refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_memory()
        .with_cpu()
        .with_exe(UpdateKind::OnlyIfNotSet)
}

fn lean_cpu_refresh_kind() -> CpuRefreshKind {
    CpuRefreshKind::nothing().with_cpu_usage()
}

fn lean_memory_refresh_kind() -> MemoryRefreshKind {
    MemoryRefreshKind::nothing().with_ram()
}

/// Invalidates the 2-second raw telemetry cooldown cache (e.g. immediately after terminating
/// a process group so the next sample reflects the exited process right away).
pub fn invalidate_telemetry_cache() {
    if let Some(lock) = SYS_STATE.get() {
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        state.cached = None;
    }
}

fn collect_raw_telemetry_cached() -> CachedRawTelemetry {
    let state_mutex = SYS_STATE.get_or_init(|| {
        let specifics = RefreshKind::nothing()
            .with_cpu(lean_cpu_refresh_kind())
            .with_memory(lean_memory_refresh_kind())
            .with_processes(lean_process_refresh_kind());
        Mutex::new(TelemetrySamplerState {
            sys: System::new_with_specifics(specifics),
            cached: None,
        })
    });

    // Coalesce concurrent/rapid callers (e.g. repeated Win taps + slow poller) behind a 2s
    // cooldown so sysinfo never storms 300x OpenProcess + NVML kernel calls back-to-back.
    let mut state = state_mutex.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref cached) = state.cached {
        if cached.refreshed_at.elapsed() < TELEMETRY_SAMPLE_MIN_INTERVAL {
            return cached.clone();
        }
    }

    state.sys.refresh_cpu_specifics(lean_cpu_refresh_kind());
    state.sys.refresh_memory_specifics(lean_memory_refresh_kind());
    state
        .sys
        .refresh_processes_specifics(ProcessesToUpdate::All, true, lean_process_refresh_kind());

    let cpu_name = state
        .sys
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

    let cpu_pct = state.sys.global_cpu_usage().round().clamp(0.0, 100.0) as u32;
    let ram_total_bytes = state.sys.total_memory().max(1);
    let ram_used_bytes = state.sys.used_memory();
    let cpu_count = state.sys.cpus().len().max(1) as f32;

    let samples: Vec<RawProcessSample> = state
        .sys
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

    let gpu = query_gpu();

    let fresh = CachedRawTelemetry {
        refreshed_at: Instant::now(),
        cpu_name,
        cpu_pct,
        ram_used_bytes,
        ram_total_bytes,
        samples,
        gpu,
    };
    state.cached = Some(fresh.clone());
    fresh
}

fn extract_shell_icon_png(exe_path: &str) -> Option<String> {
    let trimmed = exe_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    unsafe {
        let mut sfi = SHFILEINFOW::default();
        let w = wide(trimmed);
        let res = SHGetFileInfoW(
            PCWSTR(w.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if res == 0 || sfi.hIcon.is_invalid() {
            return None;
        }
        let data_url = hicon_to_data_url(sfi.hIcon, 32);
        let _ = DestroyIcon(sfi.hIcon);
        data_url
    }
}

fn resolve_exe_icon_key_into_cache(exe_path: &str, cache: &mut IconCache) -> Option<String> {
    let trimmed = exe_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let key = icon_cache::path_icon_key(trimmed);
    if cache.get(&key).is_some() {
        return Some(key);
    }
    if cache.is_negative_cached(&key) {
        return None;
    }
    match extract_shell_icon_png(trimmed) {
        Some(data_uri) => {
            cache.put(key.clone(), data_uri);
            Some(key)
        }
        None => {
            cache.mark_negative_miss(key);
            None
        }
    }
}

fn resolve_exe_icon_key_into_cache_mutex(
    exe_path: &str,
    cache: &Mutex<IconCache>,
) -> Option<String> {
    let trimmed = exe_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let key = icon_cache::path_icon_key(trimmed);
    {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if guard.get(&key).is_some() {
            return Some(key);
        }
        if guard.is_negative_cached(&key) {
            return None;
        }
    }
    // Perform SHGetFileInfoW / GDI icon extraction outside the shared cache mutex.
    let extracted = extract_shell_icon_png(trimmed);
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    match extracted {
        Some(data_uri) => {
            guard.put(key.clone(), data_uri);
            Some(key)
        }
        None => {
            guard.mark_negative_miss(key);
            None
        }
    }
}

/// Samples lean telemetry with disk headroom, process-to-HWND mapping, and `icon_key`-only entries (`AD-6`),
/// running CPU/RAM/NVML/disk scans completely outside `cache` and locking `IconCache` only for brief
/// individual cache hits/writes.
pub fn sample_telemetry_with_icons_mutex(
    cache: &Mutex<IconCache>,
    pid_to_hwnd: &HashMap<u32, isize>,
) -> (TelemetrySnapshot, Vec<(String, String)>) {
    let raw = collect_raw_telemetry_cached();
    let disks = get_disks_headroom_cached();

    build_telemetry_snapshot_with_icons(
        raw.cpu_name,
        raw.cpu_pct,
        raw.ram_used_bytes,
        raw.ram_total_bytes,
        raw.gpu,
        &raw.samples,
        disks,
        pid_to_hwnd,
        |exe| resolve_exe_icon_key_into_cache_mutex(exe, cache),
    )
}

/// Samples lean telemetry with disk headroom, process-to-HWND mapping, and `icon_key`-only entries (`AD-6`).
/// Returns `(snapshot, icon_sources)` so Wave 1B registers `(icon_key, exe_path)` for LRU reload.
pub fn sample_telemetry_with_icons(
    cache: &mut IconCache,
    pid_to_hwnd: &HashMap<u32, isize>,
) -> (TelemetrySnapshot, Vec<(String, String)>) {
    let raw = collect_raw_telemetry_cached();
    let disks = get_disks_headroom_cached();

    build_telemetry_snapshot_with_icons(
        raw.cpu_name,
        raw.cpu_pct,
        raw.ram_used_bytes,
        raw.ram_total_bytes,
        raw.gpu,
        &raw.samples,
        disks,
        pid_to_hwnd,
        |exe| resolve_exe_icon_key_into_cache(exe, cache),
    )
}

fn collect_telemetry_with_icon_resolver<FIcon>(resolve_icon: FIcon) -> TelemetryState
where
    FIcon: FnMut(&str) -> Option<String>,
{
    let raw = collect_raw_telemetry_cached();
    let mut state = build_telemetry_state(
        raw.cpu_name,
        raw.cpu_pct,
        raw.ram_used_bytes,
        raw.ram_total_bytes,
        raw.gpu,
        &raw.samples,
        resolve_icon,
    );
    state.disks = get_disks_headroom_cached();
    state
}

pub fn get_telemetry_cached(cache: &Mutex<IconCache>) -> TelemetryState {
    collect_telemetry_with_icon_resolver(|exe| extract_icon_for_path_cached(exe, cache))
}

pub fn get_telemetry() -> TelemetryState {
    collect_telemetry_with_icon_resolver(extract_icon_for_path)
}

/// Backward-compatible alias for legacy telemetry callers.
pub fn sample_telemetry() -> TelemetrySnapshot {
    get_telemetry()
}

/// Terminates ALL PIDs belonging to a multi-process app group and propagates OS permission errors.
pub fn kill_process_group(pids: &[u32]) -> Result<(), String> {
    let res = terminate_process_pids(pids);
    if res.is_ok() {
        invalidate_telemetry_cache();
    }
    res
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

    #[test]
    fn disk_headroom_builder_sorts_c_first_deduplicates_and_skips_zero_volumes() {
        let raw = vec![
            RawDiskSample {
                mount: r"E:\".into(),
                name: "Games".into(),
                total_bytes: 1000 * GIB,
                available_bytes: 250 * GIB,
            },
            RawDiskSample {
                mount: r"C:\".into(),
                name: "Windows".into(),
                total_bytes: 500 * GIB,
                available_bytes: 100 * GIB,
            },
            RawDiskSample {
                mount: r"c:/".into(), // duplicate of C: must be skipped
                name: "Duplicate".into(),
                total_bytes: 500 * GIB,
                available_bytes: 50 * GIB,
            },
            RawDiskSample {
                mount: r"D:\".into(),
                name: "".into(),
                total_bytes: 200 * GIB,
                available_bytes: 150 * GIB,
            },
            RawDiskSample {
                mount: r"Z:\".into(),
                name: "Empty Optical".into(),
                total_bytes: 0,
                available_bytes: 0,
            },
        ];

        let disks = build_disk_headroom_list(&raw);
        assert_eq!(disks.len(), 3);
        assert_eq!(disks[0].mount, "C:");
        assert_eq!(disks[0].name, "Windows");
        assert_eq!(disks[0].used_gb, 400.0);
        assert_eq!(disks[0].free_gb, 100.0);
        assert_eq!(disks[0].total_gb, 500.0);
        assert_eq!(disks[0].used_pct, 80);

        assert_eq!(disks[1].mount, "D:");
        assert_eq!(disks[1].name, "D:");
        assert_eq!(disks[1].used_pct, 25);

        assert_eq!(disks[2].mount, "E:");
        assert_eq!(disks[2].used_pct, 75);
    }

    #[test]
    fn nvml_return_code_classifier_detects_tdr_reset_and_partial_degradation() {
        assert_eq!(
            classify_nvml_query_codes(NVML_SUCCESS, NVML_SUCCESS, NVML_SUCCESS),
            NvmlQueryHealth::Healthy
        );
        assert_eq!(
            classify_nvml_query_codes(NVML_SUCCESS, 3, NVML_SUCCESS),
            NvmlQueryHealth::PartialDegraded
        );
        assert_eq!(
            classify_nvml_query_codes(NVML_ERROR_GPU_IS_LOST, NVML_SUCCESS, NVML_SUCCESS),
            NvmlQueryHealth::LostNeedsReset
        );
        assert_eq!(
            classify_nvml_query_codes(NVML_SUCCESS, NVML_ERROR_RESET_REQUIRED, NVML_SUCCESS),
            NvmlQueryHealth::LostNeedsReset
        );
        assert_eq!(
            classify_nvml_query_codes(NVML_SUCCESS, NVML_SUCCESS, NVML_ERROR_UNINITIALIZED),
            NvmlQueryHealth::LostNeedsReset
        );
        assert_eq!(
            classify_nvml_query_codes(99, 99, 99),
            NvmlQueryHealth::LostNeedsReset
        );
    }

    #[test]
    fn telemetry_snapshot_with_icons_populates_icon_key_hwnd_disks_and_omits_inline_base64() {
        let samples = vec![
            sample(101, "chrome.exe", Some(r"C:\Apps\Chrome\chrome.exe"), 1.0, 2.0),
            sample(102, "chrome.exe", Some(r"C:\Apps\Chrome\chrome.exe"), 3.0, 4.0),
            sample(201, "Code.exe", Some(r"C:\Apps\VS Code\Code.exe"), 2.0, 5.0),
        ];

        // Notice Chrome's primary PID is 102 (highest RAM), while its visible window belongs to PID 101!
        let mut pid_to_hwnd = HashMap::new();
        pid_to_hwnd.insert(101, 0x1234_isize);
        pid_to_hwnd.insert(201, 0x5678_isize);

        let disks = vec![DiskHeadroomInfo {
            mount: "C:".into(),
            name: "OS".into(),
            used_gb: 200.0,
            total_gb: 500.0,
            free_gb: 300.0,
            used_pct: 40,
        }];

        let (snap, sources) = build_telemetry_snapshot_with_icons(
            "Ryzen 9".into(),
            22,
            16 * GIB,
            64 * GIB,
            None,
            &samples,
            disks,
            &pid_to_hwnd,
            |exe| Some(icon_cache::path_icon_key(exe)),
        );

        assert_eq!(snap.disks.len(), 1);
        assert_eq!(snap.disks[0].mount, "C:");
        assert_eq!(snap.top_processes.len(), 2);
        assert_eq!(snap.top_processes[0].name, "Chrome");
        assert_eq!(snap.top_processes[0].pid, 102);
        assert_eq!(snap.top_processes[0].hwnd, Some(0x1234));
        assert!(snap.top_processes[0].icon_data.is_none());
        assert_eq!(
            snap.top_processes[0].icon_key.as_deref(),
            Some(icon_cache::path_icon_key(r"C:\Apps\Chrome\chrome.exe").as_str())
        );

        assert_eq!(snap.top_processes[1].name, "VS Code");
        assert_eq!(snap.top_processes[1].hwnd, Some(0x5678));
        assert!(snap.top_processes[1].icon_data.is_none());

        // Deduplicated across top_processes and gpu_processes: exactly 2 unique icon_sources
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn telemetry_excludes_testbar_variants_and_coalesces_burst_samples() {
        assert!(is_excluded_telemetry_process("testbar.exe"));
        assert!(is_excluded_telemetry_process("TestBar-v2.exe"));
        assert!(is_excluded_telemetry_process("testbar_dev.exe"));
        assert!(!is_excluded_telemetry_process("chrome.exe"));

        invalidate_telemetry_cache();
        let cache = Mutex::new(IconCache::new(64));
        let (first, _) = sample_telemetry_with_icons_mutex(&cache, &HashMap::new());
        let t0 = Instant::now();
        let (second, _) = sample_telemetry_with_icons_mutex(&cache, &HashMap::new());
        let elapsed = t0.elapsed();
        assert_eq!(first.cpu_pct, second.cpu_pct);
        assert_eq!(first.ram_pct, second.ram_pct);
        assert!(
            elapsed < Duration::from_millis(250),
            "2nd telemetry sample inside 2s cooldown should hit cache immediately, took {:?}",
            elapsed
        );
        invalidate_telemetry_cache();
    }
}
