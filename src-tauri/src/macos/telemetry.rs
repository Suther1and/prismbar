use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use sysinfo::{
    CpuRefreshKind, Disks, MemoryRefreshKind, ProcessRefreshKind, ProcessesToUpdate, RefreshKind,
    System, UpdateKind,
};

use crate::macos::icon_cache::{self, IconCache};

const BYTES_PER_GIB: f64 = 1_073_741_824.0;
const TELEMETRY_CACHE_TTL: Duration = Duration::from_millis(1500);
const DISK_CACHE_TTL: Duration = Duration::from_secs(30);
const DEFAULT_PROCESS_LIMIT: usize = 6;

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

#[derive(Debug, Clone)]
pub struct RawProcessSample {
    pub pid: u32,
    pub raw_name: String,
    pub exe_path: Option<String>,
    pub mem_bytes: u64,
    pub cpu_pct_normalized: f32,
}

#[derive(Debug, Clone)]
struct CachedRawTelemetry {
    refreshed_at: Instant,
    cpu_name: String,
    cpu_pct: u32,
    ram_used_bytes: u64,
    ram_total_bytes: u64,
    samples: Vec<RawProcessSample>,
}

struct TelemetrySamplerState {
    sys: System,
    cached: Option<CachedRawTelemetry>,
    initialized: bool,
}

struct DiskCacheState {
    refreshed_at: Instant,
    disks: Vec<DiskHeadroomInfo>,
}

static SYS_STATE: OnceLock<Mutex<TelemetrySamplerState>> = OnceLock::new();
static DISK_CACHE: OnceLock<Mutex<Option<DiskCacheState>>> = OnceLock::new();
static CPU_BRAND_CACHE: OnceLock<String> = OnceLock::new();

fn round_gb_1(bytes: u64) -> f32 {
    let gb = bytes as f64 / BYTES_PER_GIB;
    ((gb * 10.0).round() / 10.0) as f32
}

fn pct_u32(used: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    ((used as f64 / total as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u32
}

fn read_mac_cpu_brand(sys: &System) -> String {
    CPU_BRAND_CACHE
        .get_or_init(|| {
            if let Ok(out) = Command::new("sysctl")
                .args(["-n", "machdep.cpu.brand_string"])
                .output()
            {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !s.is_empty() {
                        return s;
                    }
                }
            }
            sys.cpus()
                .first()
                .map(|c| c.brand().trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Apple Silicon".to_string())
        })
        .clone()
}

/// Normalizes a macOS process name and resolves its enclosing `.app` bundle when available.
pub fn normalize_mac_process_info(raw_name: &str, exe_path: Option<&str>) -> (String, Option<String>) {
    if let Some(exe) = exe_path.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(bundle_path) = icon_cache::app_bundle_ancestor(exe) {
            if let Some(stem) = Path::new(&bundle_path).file_stem().and_then(|s| s.to_str()) {
                let clean = stem.trim();
                if !clean.is_empty() {
                    return (clean.to_string(), Some(bundle_path));
                }
            }
        }
    }

    let mut name = raw_name.trim().to_string();
    for suffix in [
        " Helper (Renderer)",
        " Helper (GPU)",
        " Helper (Plugin)",
        " Helper (Alerts)",
        " Helper",
        " (Renderer)",
    ] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            let trimmed = stripped.trim();
            if !trimmed.is_empty() {
                name = trimmed.to_string();
                break;
            }
        }
    }
    (name, None)
}

fn is_excluded_process(pid: u32, raw_name: &str, normalized_name: &str) -> bool {
    if pid == 0 || pid == std::process::id() {
        return true;
    }
    let raw_low = raw_name.trim().to_ascii_lowercase();
    let norm_low = normalized_name.trim().to_ascii_lowercase();
    raw_low == "testbar"
        || norm_low == "testbar"
        || raw_low == "prismbar"
        || norm_low == "prismbar"
        || raw_low == "kernel_task"
        || norm_low == "kernel_task"
}

/// Aggregates process samples into the top `limit` memory-consuming groups.
pub fn aggregate_mac_top_processes<FIcon>(
    samples: &[RawProcessSample],
    limit: usize,
    mut resolve_icon: FIcon,
) -> Vec<TopProcessInfo>
where
    FIcon: FnMut(&str) -> Option<String>,
{
    struct GroupAcc {
        display_name: String,
        primary_pid: u32,
        primary_mem: u64,
        pids: HashSet<u32>,
        total_mem: u64,
        total_cpu: f32,
        bundle_path: Option<String>,
    }

    let mut groups: HashMap<String, GroupAcc> = HashMap::new();

    for sample in samples {
        let (display_name, bundle_path) =
            normalize_mac_process_info(&sample.raw_name, sample.exe_path.as_deref());
        if display_name.is_empty()
            || is_excluded_process(sample.pid, &sample.raw_name, &display_name)
        {
            continue;
        }
        let key = display_name.to_lowercase();
        match groups.get_mut(&key) {
            Some(acc) => {
                acc.pids.insert(sample.pid);
                acc.total_mem = acc.total_mem.saturating_add(sample.mem_bytes);
                acc.total_cpu += sample.cpu_pct_normalized.max(0.0);
                if acc.bundle_path.is_none() && bundle_path.is_some() {
                    acc.bundle_path = bundle_path;
                }
                if sample.mem_bytes > acc.primary_mem {
                    acc.primary_pid = sample.pid;
                    acc.primary_mem = sample.mem_bytes;
                }
            }
            None => {
                let mut pids = HashSet::new();
                pids.insert(sample.pid);
                groups.insert(
                    key,
                    GroupAcc {
                        display_name,
                        primary_pid: sample.pid,
                        primary_mem: sample.mem_bytes,
                        pids,
                        total_mem: sample.mem_bytes,
                        total_cpu: sample.cpu_pct_normalized.max(0.0),
                        bundle_path,
                    },
                );
            }
        }
    }

    let mut ranked: Vec<GroupAcc> = groups.into_values().collect();
    ranked.sort_by(|a, b| {
        b.total_mem
            .cmp(&a.total_mem)
            .then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()))
    });

    ranked
        .into_iter()
        .take(limit)
        .map(|acc| {
            let mut pids: Vec<u32> = acc.pids.into_iter().collect();
            pids.sort_unstable();
            let icon_key = acc.bundle_path.as_deref().map(icon_cache::exe_key);
            let icon_data = acc.bundle_path.as_deref().and_then(&mut resolve_icon);
            TopProcessInfo {
                pid: acc.primary_pid,
                pids,
                name: acc.display_name,
                mem_gb: round_gb_1(acc.total_mem),
                vram_gb: 0.0,
                uses_gpu: false,
                cpu_pct: acc.total_cpu.round().clamp(0.0, 100.0) as u32,
                icon_data,
                icon_key,
                hwnd: None,
            }
        })
        .collect()
}

fn collect_disks_headroom_cached() -> Vec<DiskHeadroomInfo> {
    let lock = DISK_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref cached) = *guard {
            if cached.refreshed_at.elapsed() < DISK_CACHE_TTL {
                return cached.disks.clone();
            }
        }
    }

    let disks = Disks::new_with_refreshed_list();
    let mut seen_mounts = HashSet::new();
    let mut out = Vec::new();

    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy().to_string();
        let is_root = mount == "/";
        let is_external_volume = mount
            .strip_prefix("/Volumes/")
            .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'));
        if !is_root && !is_external_volume {
            continue;
        }
        if !seen_mounts.insert(mount.clone()) {
            continue;
        }
        let total_bytes = disk.total_space();
        if total_bytes == 0 {
            continue;
        }
        let free_bytes = disk.available_space().min(total_bytes);
        let used_bytes = total_bytes.saturating_sub(free_bytes);
        let name = if is_root {
            "Macintosh HD".to_string()
        } else {
            let raw = disk.name().to_string_lossy().trim().to_string();
            if raw.is_empty() || raw.starts_with("disk") {
                mount.trim_start_matches("/Volumes/").to_string()
            } else {
                raw
            }
        };
        out.push(DiskHeadroomInfo {
            mount,
            name,
            used_gb: round_gb_1(used_bytes),
            total_gb: round_gb_1(total_bytes),
            free_gb: round_gb_1(free_bytes),
            used_pct: pct_u32(used_bytes, total_bytes) as u8,
        });
    }

    out.sort_by(|a, b| (a.mount != "/").cmp(&(b.mount != "/")).then_with(|| a.name.cmp(&b.name)));

    let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(DiskCacheState {
        refreshed_at: Instant::now(),
        disks: out.clone(),
    });
    out
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

/// Invalidates the 1500ms telemetry cache so the next sample reflects process termination immediately.
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
            initialized: false,
        })
    });

    let mut state = state_mutex.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref cached) = state.cached {
        if cached.refreshed_at.elapsed() < TELEMETRY_CACHE_TTL {
            return cached.clone();
        }
    }

    if !state.initialized {
        state.initialized = true;
        // Brief baseline delta wait once per app session (> MINIMUM_CPU_UPDATE_INTERVAL) so first CPU reading is meaningful.
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(20));
    }

    state.sys.refresh_cpu_specifics(lean_cpu_refresh_kind());
    state.sys.refresh_memory_specifics(lean_memory_refresh_kind());
    state.sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        lean_process_refresh_kind(),
    );

    let cpu_name = read_mac_cpu_brand(&state.sys);
    let cpu_pct = state.sys.global_cpu_usage().round().clamp(0.0, 100.0) as u32;
    let ram_total_bytes = state.sys.total_memory().max(1);
    let ram_used_bytes = state.sys.used_memory().min(ram_total_bytes);
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

    let fresh = CachedRawTelemetry {
        refreshed_at: Instant::now(),
        cpu_name,
        cpu_pct,
        ram_used_bytes,
        ram_total_bytes,
        samples,
    };
    state.cached = Some(fresh.clone());
    fresh
}

fn build_telemetry_state<FIcon>(resolve_icon: FIcon) -> TelemetryState
where
    FIcon: FnMut(&str) -> Option<String>,
{
    let raw = collect_raw_telemetry_cached();
    let disks = collect_disks_headroom_cached();
    let top_processes =
        aggregate_mac_top_processes(&raw.samples, DEFAULT_PROCESS_LIMIT, resolve_icon);

    TelemetryState {
        cpu_name: raw.cpu_name,
        cpu_pct: raw.cpu_pct,
        ram_used_gb: round_gb_1(raw.ram_used_bytes),
        ram_total_gb: round_gb_1(raw.ram_total_bytes),
        ram_pct: pct_u32(raw.ram_used_bytes, raw.ram_total_bytes),
        gpu_available: false,
        gpu_name: String::new(),
        gpu_pct: 0,
        gpu_temp_c: 0,
        vram_used_gb: 0.0,
        vram_total_gb: 0.0,
        vram_pct: 0,
        top_processes,
        gpu_processes: Vec::new(),
        disks,
    }
}

/// Samples macOS telemetry using the shared `IconCache` to attach `.app` icons to top processes.
pub fn get_telemetry_cached(cache: &Mutex<IconCache>) -> TelemetryState {
    build_telemetry_state(|bundle| icon_cache::extract_icon_for_path_cached(bundle, cache))
}

/// Samples macOS telemetry without an external `IconCache`.
pub fn get_telemetry() -> TelemetryState {
    build_telemetry_state(|_| None)
}

/// Terminates a single process by PID via `kill -TERM`.
pub fn kill_process_pid(pid: u32) -> Result<(), String> {
    if pid == 0 || pid == std::process::id() {
        return Err("Нельзя завершить системный или собственный процесс".to_string());
    }
    let output = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output()
        .map_err(|e| format!("Не удалось завершить процесс {pid}: {e}"))?;

    if output.status.success() {
        invalidate_telemetry_cache();
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.to_ascii_lowercase().contains("no such process") {
            invalidate_telemetry_cache();
            Ok(())
        } else if stderr.is_empty() {
            Err(format!("Не удалось завершить процесс PID {pid}"))
        } else {
            Err(format!("Не удалось завершить процесс PID {pid}: {stderr}"))
        }
    }
}

/// Terminates all PIDs belonging to a grouped macOS application.
pub fn kill_process_group(pids: &[u32]) -> Result<(), String> {
    let self_pid = std::process::id();
    let targets: Vec<u32> = pids
        .iter()
        .copied()
        .filter(|&p| p > 0 && p != self_pid)
        .collect();
    if targets.is_empty() {
        return Err("Список PID пуст".to_string());
    }

    let mut any_ok = false;
    let mut first_err: Option<String> = None;
    for pid in targets {
        match kill_process_pid(pid) {
            Ok(()) => any_ok = true,
            Err(err) => {
                if first_err.is_none() {
                    first_err = Some(err);
                }
            }
        }
    }

    if any_ok {
        invalidate_telemetry_cache();
        Ok(())
    } else {
        Err(first_err.unwrap_or_else(|| "Не удалось завершить процессы".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1_073_741_824;

    #[test]
    fn groups_chrome_helper_processes_by_outer_app_bundle() {
        let samples = vec![
            RawProcessSample {
                pid: 100,
                raw_name: "Google Chrome".into(),
                exe_path: Some("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into()),
                mem_bytes: GIB,
                cpu_pct_normalized: 5.0,
            },
            RawProcessSample {
                pid: 101,
                raw_name: "Google Chrome Helper (Renderer)".into(),
                exe_path: Some(
                    "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)".into(),
                ),
                mem_bytes: 2 * GIB,
                cpu_pct_normalized: 10.0,
            },
            RawProcessSample {
                pid: 200,
                raw_name: "TestBar".into(),
                exe_path: Some("/Applications/TestBar.app/Contents/MacOS/TestBar".into()),
                mem_bytes: 4 * GIB,
                cpu_pct_normalized: 1.0,
            },
            RawProcessSample {
                pid: 201,
                raw_name: "PrismBar".into(),
                exe_path: Some("/Applications/PrismBar.app/Contents/MacOS/PrismBar".into()),
                mem_bytes: 4 * GIB,
                cpu_pct_normalized: 1.0,
            },
            RawProcessSample {
                pid: 300,
                raw_name: "rust-analyzer".into(),
                exe_path: Some("/Users/alex/.cargo/bin/rust-analyzer".into()),
                mem_bytes: GIB / 2,
                cpu_pct_normalized: 25.0,
            },
        ];

        let top = aggregate_mac_top_processes(&samples, 6, |bundle| Some(format!("icon:{bundle}")));
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].name, "Google Chrome");
        assert_eq!(top[0].pids, vec![100, 101]);
        assert_eq!(top[0].pid, 101);
        assert!((top[0].mem_gb - 3.0).abs() < 0.05);
        assert_eq!(top[0].cpu_pct, 15);
        assert_eq!(
            top[0].icon_data.as_deref(),
            Some("icon:/Applications/Google Chrome.app")
        );
        assert_eq!(top[1].name, "rust-analyzer");
        assert_eq!(top[1].icon_data, None);
    }

    #[test]
    fn caps_top_processes_at_six_sorted_by_memory_desc() {
        let samples: Vec<RawProcessSample> = (1..=10u32)
            .map(|i| RawProcessSample {
                pid: i + 10,
                raw_name: format!("App{i}"),
                exe_path: None,
                mem_bytes: u64::from(i) * (GIB / 4),
                cpu_pct_normalized: 1.0,
            })
            .collect();
        let top = aggregate_mac_top_processes(&samples, 6, |_| None);
        assert_eq!(top.len(), 6);
        assert_eq!(top[0].name, "App10");
        assert_eq!(top[5].name, "App5");
    }
}
