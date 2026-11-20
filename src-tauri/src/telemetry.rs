use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::{Mutex, OnceLock};
use sysinfo::{ProcessesToUpdate, System};
use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::win_shell::wide;
use crate::windows_tracker::{extract_icon_for_path, friendly_exe_name};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopProcessInfo {
    pub pid: u32,
    pub name: String,
    pub mem_gb: f32,
    pub cpu_pct: u32,
    pub icon_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

type NvmlDevice = *mut std::ffi::c_void;
type FnInit = unsafe extern "C" fn() -> i32;
type FnGetHandle = unsafe extern "C" fn(u32, *mut NvmlDevice) -> i32;
type FnGetName = unsafe extern "C" fn(NvmlDevice, *mut u8, u32) -> i32;
type FnGetUtil = unsafe extern "C" fn(NvmlDevice, *mut NvmlUtilization) -> i32;
type FnGetTemp = unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> i32;
type FnGetMem = unsafe extern "C" fn(NvmlDevice, *mut NvmlMemory) -> i32;

struct NvmlApi {
    device: NvmlDevice,
    gpu_name: String,
    get_util: FnGetUtil,
    get_temp: FnGetTemp,
    get_mem: FnGetMem,
}

unsafe impl Send for NvmlApi {}
unsafe impl Sync for NvmlApi {}

static NVML_API: OnceLock<Option<NvmlApi>> = OnceLock::new();
static SYS_STATE: OnceLock<Mutex<System>> = OnceLock::new();

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

        let init: FnInit = std::mem::transmute(get_fn("nvmlInit_v2").or_else(|| get_fn("nvmlInit"))?);
        if init() != 0 {
            return None;
        }
        let get_handle: FnGetHandle = std::mem::transmute(get_fn("nvmlDeviceGetHandleByIndex_v2").or_else(|| get_fn("nvmlDeviceGetHandleByIndex"))?);
        let get_name: FnGetName = std::mem::transmute(get_fn("nvmlDeviceGetName")?);
        let get_util: FnGetUtil = std::mem::transmute(get_fn("nvmlDeviceGetUtilizationRates")?);
        let get_temp: FnGetTemp = std::mem::transmute(get_fn("nvmlDeviceGetTemperature")?);
        let get_mem: FnGetMem = std::mem::transmute(get_fn("nvmlDeviceGetMemoryInfo")?);

        let mut dev: NvmlDevice = std::ptr::null_mut();
        if get_handle(0, &mut dev) != 0 || dev.is_null() {
            return None;
        }

        let mut name_buf = [0u8; 96];
        let gpu_name = if get_name(dev, name_buf.as_mut_ptr(), name_buf.len() as u32) == 0 {
            let end = name_buf.iter().position(|&b| b == 0).unwrap_or(name_buf.len());
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
        })
    }
}

fn query_gpu() -> (bool, String, u32, u32, f32, f32, u32) {
    let api_opt = NVML_API.get_or_init(init_nvml);
    let Some(api) = api_opt else {
        return (false, "GPU".to_string(), 0, 0, 0.0, 0.0, 0);
    };
    unsafe {
        let mut util = NvmlUtilization { gpu: 0, memory: 0 };
        let mut temp = 0u32;
        let mut mem = NvmlMemory { total: 0, free: 0, used: 0 };
        let _ = (api.get_util)(api.device, &mut util);
        let _ = (api.get_temp)(api.device, 0, &mut temp);
        let _ = (api.get_mem)(api.device, &mut mem);

        let used_gb = (mem.used as f64 / 1_073_741_824.0) as f32;
        let total_gb = (mem.total as f64 / 1_073_741_824.0).round() as f32;
        let pct = if mem.total > 0 {
            ((mem.used as f64 / mem.total as f64) * 100.0).round() as u32
        } else {
            0
        };
        (true, api.gpu_name.clone(), util.gpu, temp, (used_gb * 10.0).round() / 10.0, total_gb, pct)
    }
}

pub fn get_telemetry() -> TelemetryState {
    let sys_mutex = SYS_STATE.get_or_init(|| {
        let mut s = System::new_all();
        s.refresh_all();
        Mutex::new(s)
    });

    let mut sys = sys_mutex.lock().unwrap();
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
    let ram_total_gb = ((total_ram_bytes as f64 / 1_073_741_824.0).round()) as f32;
    let ram_used_gb = (((used_ram_bytes as f64 / 1_073_741_824.0) * 10.0).round() / 10.0) as f32;
    let ram_pct = ((used_ram_bytes as f64 / total_ram_bytes as f64) * 100.0).round().clamp(0.0, 100.0) as u32;

    // Group processes by executable name so browser tabs/helpers aggregate cleanly
    let cpu_count = sys.cpus().len().max(1) as f32;
    let mut grouped: HashMap<String, (u32, u64, f32, Option<String>)> = HashMap::new();
    for (pid, proc_) in sys.processes() {
        let raw_name = proc_.name().to_string_lossy().to_string();
        let low = raw_name.to_ascii_lowercase();
        if matches!(
            low.as_str(),
            "system" | "registry" | "memory compression" | "idle" | "svchost.exe" | "dwm.exe" | "testbar.exe"
        ) {
            continue;
        }
        let exe_path = proc_.exe().map(|p| p.to_string_lossy().to_string());
        let nice_name = exe_path
            .as_deref()
            .map(friendly_exe_name)
            .unwrap_or_else(|| raw_name.trim_end_matches(".exe").to_string());
        let mem = proc_.memory();
        let cpu = proc_.cpu_usage() / cpu_count;

        let entry = grouped.entry(nice_name).or_insert((pid.as_u32(), 0, 0.0, exe_path.clone()));
        entry.1 += mem;
        entry.2 += cpu;
        if mem > entry.1 / 2 {
            entry.0 = pid.as_u32();
        }
    }

    let mut top: Vec<(String, u32, u64, f32, Option<String>)> = grouped
        .into_iter()
        .map(|(name, (pid, mem, cpu, exe))| (name, pid, mem, cpu, exe))
        .collect();
    top.sort_by(|a, b| b.2.cmp(&a.2));

    let top_processes = top
        .into_iter()
        .take(5)
        .map(|(name, pid, mem, cpu, exe)| TopProcessInfo {
            pid,
            name,
            mem_gb: (((mem as f64 / 1_073_741_824.0) * 10.0).round() / 10.0) as f32,
            cpu_pct: cpu.round().clamp(0.0, 100.0) as u32,
            icon_data: exe.as_deref().and_then(extract_icon_for_path),
        })
        .collect();

    let (gpu_available, gpu_name, gpu_pct, gpu_temp_c, vram_used_gb, vram_total_gb, vram_pct) =
        query_gpu();

    TelemetryState {
        cpu_name,
        cpu_pct,
        ram_used_gb,
        ram_total_gb,
        ram_pct,
        gpu_available,
        gpu_name,
        gpu_pct,
        gpu_temp_c,
        vram_used_gb,
        vram_total_gb,
        vram_pct,
        top_processes,
    }
}
