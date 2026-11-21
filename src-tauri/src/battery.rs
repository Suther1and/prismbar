use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::{c_void, CString};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{GUID, PCSTR, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiGetDeviceInterfaceDetailW,
    SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, HDEVINFO,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_FriendlyName, DEVPROPKEY, DEVPROPTYPE,
};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_PENDING, HANDLE, HMODULE, HWND, INVALID_HANDLE_VALUE,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

const BATTERY_CACHE_TTL: Duration = Duration::from_secs(25);
const RAZER_READING_CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const HIDPP_IO_TIMEOUT_MS: u32 = 90;

const GENERIC_READ_FLAG: u32 = 0x8000_0000;
const GENERIC_WRITE_FLAG: u32 = 0x4000_0000;

// GUID_DEVINTERFACE_HID: {4D1E55B2-F16F-11CF-88CB-001111000030}
const GUID_DEVINTERFACE_HID: GUID = GUID::from_u128(0x4d1e55b2_f16f_11cf_88cb_001111000030);

// DEVPKEY_Bluetooth_Battery: {104EA319-6EE2-4701-BD47-8DDBF425BBE5}, pid 2
const DEVPKEY_BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};

// DEVPKEY_Device_IsConnected: {83DA6326-97A6-4088-9453-A1923F573B29}, pid 15
const DEVPKEY_DEVICE_IS_CONNECTED: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x83da6326_97a6_4088_9453_a1923f573b29),
    pid: 15,
};

const RAZER_VID: u16 = 0x1532;
const LOGITECH_VID: u16 = 0x046D;
const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;

const RAZER_REPORT_LEN: usize = 91;
const RAZER_CMD_CLASS_POWER: u8 = 0x07;
const RAZER_CMD_GET_BATTERY: u8 = 0x80;
const RAZER_CMD_GET_CHARGING: u8 = 0x84;
const RAZER_STATUS_OK: u8 = 0x02;
const RAZER_STATUS_UNSUPPORTED: u8 = 0x05;

const XINPUT_DEVTYPE_GAMEPAD: u8 = 0x00;
const XINPUT_BATTERY_TYPE_DISCONNECTED: u8 = 0x00;
const XINPUT_BATTERY_TYPE_WIRED: u8 = 0x01;
const XINPUT_BATTERY_TYPE_ALKALINE: u8 = 0x02;
const XINPUT_BATTERY_TYPE_NIMH: u8 = 0x03;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatterySubLevel {
    pub label: String,
    pub percent: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeripheralBattery {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub percent: u8,
    pub charging: bool,
    pub source: String,
    pub connected: bool,
    pub sub_levels: Option<Vec<BatterySubLevel>>,
}

static BATTERY_CACHE: OnceLock<Mutex<Option<(Instant, Vec<PeripheralBattery>)>>> = OnceLock::new();
static BATTERY_SCAN_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static RAZER_LAST_VALID: OnceLock<Mutex<HashMap<String, (Instant, u8, bool)>>> = OnceLock::new();

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

struct DevInfoGuard(HDEVINFO);

impl Drop for DevInfoGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }
}

struct HandleGuard(HANDLE);

impl HandleGuard {
    fn new(h: HANDLE) -> Option<Self> {
        if h.is_invalid() || h.0.is_null() {
            None
        } else {
            Some(Self(h))
        }
    }
}

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() && !self.0.0.is_null() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

// ======================= CLASSIFICATION & CLEANING =======================

pub fn clean_bt_device_name(raw: &str) -> String {
    let mut name = raw.trim().to_string();
    loop {
        let before = name.clone();
        for prefix in ["Транспорт AVRCP "] {
            if let Some(stripped) = name.strip_prefix(prefix) {
                name = stripped.trim().to_string();
            }
        }
        for suffix in [
            " Hands-Free AG",
            " Hands-Free HF",
            " Hands-Free",
            " Avrcp Transport",
            " Stereo",
            " A2DP SNK",
            " - Find My",
            " (Find My)",
        ] {
            if let Some(stripped) = name.strip_suffix(suffix) {
                name = stripped.trim().to_string();
            }
        }
        if name == before {
            break;
        }
    }
    name
}

pub fn classify_peripheral_kind(name: &str) -> &'static str {
    let low = name.to_ascii_lowercase();
    if low.contains("gamepad")
        || low.contains("controller")
        || low.contains("xbox")
        || low.contains("dualsense")
        || low.contains("dualshock")
        || low.contains("joy-con")
        || low.contains("wolverine")
        || low.contains("геймпад")
        || low.contains("контроллер")
    {
        "gamepad"
    } else if low.contains("keyboard")
        || low.contains("keychron")
        || low.contains("blackwidow")
        || low.contains("deathstalker")
        || low.contains("huntsman")
        || low.contains("mx keys")
        || low.contains("mx mechanical")
        || low.contains("g915")
        || low.contains("g715")
        || low.contains("nuphy")
        || low.contains("клавиатур")
    {
        "keyboard"
    } else if low.contains("airpods")
        || low.contains("head")
        || low.contains("buds")
        || low.contains("pods")
        || low.contains("sony")
        || low.contains("wh-1000")
        || low.contains("wf-1000")
        || low.contains("bose")
        || low.contains("beats")
        || low.contains("jabra")
        || low.contains("sennheiser")
        || low.contains("barracuda")
        || low.contains("blackshark")
        || low.contains("kraken")
        || low.contains("arctis")
        || low.contains("наушн")
        || low.contains("гарнит")
        || low.contains("audio")
    {
        "headphones"
    } else if low.contains("mouse")
        || low.contains("мышь")
        || low.contains("basilisk")
        || low.contains("viper")
        || low.contains("deathadder")
        || low.contains("cobra")
        || low.contains("naga")
        || low.contains("orochi")
        || low.contains("superlight")
        || low.contains("mx master")
        || low.contains("mx anywhere")
        || low.contains("hyperpolling")
        || low.contains("mouse dock")
        || low.contains("g pro")
        || low.contains("g305")
        || low.contains("g502")
        || low.contains("g703")
        || low.contains("g903")
        || low.contains("aerox")
    {
        "mouse"
    } else {
        "other"
    }
}

pub fn extract_bt_mac_from_instance_id(instance_id: &str) -> Option<String> {
    // Strip `{...}` GUID segments first so the BT Base UUID suffix `00805F9B34FB` is never mistaken for a MAC.
    let mut cleaned = String::with_capacity(instance_id.len());
    let mut in_brace = false;
    for ch in instance_id.chars() {
        match ch {
            '{' => in_brace = true,
            '}' => {
                in_brace = false;
                cleaned.push(' ');
            }
            _ if !in_brace => cleaned.push(ch),
            _ => {}
        }
    }

    let upper = cleaned.to_ascii_uppercase();
    if let Some(pos) = upper.find("DEV_") {
        let candidate: String = upper[pos + 4..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if candidate.len() == 12 && candidate != "000000000000" {
            return Some(candidate);
        }
    }

    for token in upper.split(|c: char| !c.is_ascii_hexdigit()) {
        if token.len() == 12 && token != "000000000000" {
            return Some(token.to_string());
        }
    }
    None
}

#[derive(Debug, Clone)]
struct RawBtCandidate {
    instance_id: String,
    clean_name: String,
    mac: Option<String>,
    percent: u8,
    is_connected: Option<bool>,
}

fn should_skip_bt_candidate(is_connected: Option<bool>, percent: u8) -> bool {
    is_connected == Some(false) || percent > 100
}

fn deduplicate_bt_candidates(candidates: Vec<RawBtCandidate>) -> Vec<PeripheralBattery> {
    let mut by_key: HashMap<String, RawBtCandidate> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for cand in candidates {
        if should_skip_bt_candidate(cand.is_connected, cand.percent) {
            continue;
        }
        if cand.clean_name.is_empty() {
            continue;
        }
        let dedup_key = cand
            .mac
            .as_ref()
            .map(|m| format!("mac:{m}"))
            .unwrap_or_else(|| format!("name:{}", cand.clean_name.to_ascii_lowercase()));

        if let Some(existing) = by_key.get_mut(&dedup_key) {
            // Prefer entry that explicitly confirmed `is_connected == Some(true)` or non-generic name
            let replace_name = existing.clean_name == "Bluetooth" && cand.clean_name != "Bluetooth";
            let better_conn = cand.is_connected == Some(true) && existing.is_connected != Some(true);
            if replace_name {
                existing.clean_name = cand.clean_name.clone();
            }
            if better_conn || (existing.percent == 0 && cand.percent > 0) {
                existing.percent = cand.percent;
                existing.is_connected = cand.is_connected;
            }
        } else {
            order.push(dedup_key.clone());
            by_key.insert(dedup_key, cand);
        }
    }

    order
        .into_iter()
        .filter_map(|k| by_key.remove(&k))
        .map(|cand| {
            let id = cand
                .mac
                .as_ref()
                .map(|m| format!("bt:{}", m.to_ascii_lowercase()))
                .unwrap_or_else(|| {
                    let slug: String = cand
                        .clean_name
                        .to_ascii_lowercase()
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                        .collect();
                    format!("bt:{slug}")
                });
            let kind = classify_peripheral_kind(&cand.clean_name).to_string();
            PeripheralBattery {
                id,
                name: cand.clean_name,
                kind,
                percent: cand.percent.min(100),
                charging: false,
                source: "bluetooth".to_string(),
                connected: true,
                sub_levels: None,
            }
        })
        .collect()
}

// ======================= BACKEND 1: WINDOWS PNP BLUETOOTH =======================

unsafe fn read_dev_string_prop(
    hdev: HDEVINFO,
    info: &SP_DEVINFO_DATA,
    key: &DEVPROPKEY,
) -> Option<String> {
    let mut prop_type = DEVPROPTYPE(0);
    let mut buf = [0u16; 256];
    let bytes = std::slice::from_raw_parts_mut(
        buf.as_mut_ptr() as *mut u8,
        std::mem::size_of_val(&buf),
    );
    let mut req_size = 0u32;
    SetupDiGetDevicePropertyW(
        hdev,
        info,
        key,
        &mut prop_type,
        Some(bytes),
        Some(&mut req_size),
        0,
    )
    .ok()?;
    if req_size < 2 {
        return None;
    }
    let u16_len = ((req_size as usize) / 2).min(buf.len());
    let slice = &buf[..u16_len];
    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    let s = String::from_utf16_lossy(&slice[..end]).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

unsafe fn read_dev_is_connected(hdev: HDEVINFO, info: &SP_DEVINFO_DATA) -> Option<bool> {
    let mut prop_type = DEVPROPTYPE(0);
    let mut buf = [0u8; 4];
    let mut req_size = 0u32;
    SetupDiGetDevicePropertyW(
        hdev,
        info,
        &DEVPKEY_DEVICE_IS_CONNECTED,
        &mut prop_type,
        Some(&mut buf),
        Some(&mut req_size),
        0,
    )
    .ok()?;
    if req_size >= 1 {
        Some(buf[0] != 0)
    } else {
        None
    }
}

unsafe fn read_dev_instance_id(hdev: HDEVINFO, info: &SP_DEVINFO_DATA) -> String {
    let mut buf = [0u16; 260];
    let mut req_len = 0u32;
    if SetupDiGetDeviceInstanceIdW(hdev, info, Some(&mut buf), Some(&mut req_len)).is_ok() {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    } else {
        String::new()
    }
}

fn scan_pnp_bluetooth_batteries() -> Vec<PeripheralBattery> {
    let mut candidates = Vec::new();

    for enumerator in ["BTHENUM", "BTH", "BTHLE", "BTHLEDEVICE"] {
        let enum_w = to_wide(enumerator);
        unsafe {
            let Ok(hdev) = SetupDiGetClassDevsW(
                None,
                PCWSTR(enum_w.as_ptr()),
                HWND::default(),
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            ) else {
                continue;
            };
            let _guard = DevInfoGuard(hdev);

            let mut idx = 0u32;
            loop {
                let mut info = SP_DEVINFO_DATA {
                    cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                    ..Default::default()
                };
                if SetupDiEnumDeviceInfo(hdev, idx, &mut info).is_err() {
                    break;
                }
                idx += 1;

                let mut prop_type = DEVPROPTYPE(0);
                let mut bat_buf = [0u8; 4];
                let mut req_size = 0u32;
                if SetupDiGetDevicePropertyW(
                    hdev,
                    &info,
                    &DEVPKEY_BLUETOOTH_BATTERY,
                    &mut prop_type,
                    Some(&mut bat_buf),
                    Some(&mut req_size),
                    0,
                )
                .is_err()
                    || req_size == 0
                {
                    continue;
                }

                let raw_pct = bat_buf[0];
                let is_connected = read_dev_is_connected(hdev, &info);
                if should_skip_bt_candidate(is_connected, raw_pct) {
                    continue;
                }

                let raw_name = read_dev_string_prop(hdev, &info, &DEVPKEY_Device_FriendlyName)
                    .or_else(|| read_dev_string_prop(hdev, &info, &DEVPKEY_Device_DeviceDesc))
                    .unwrap_or_else(|| "Bluetooth".to_string());
                let clean_name = clean_bt_device_name(&raw_name);
                if clean_name.is_empty() {
                    continue;
                }

                let instance_id = read_dev_instance_id(hdev, &info);
                let mac = extract_bt_mac_from_instance_id(&instance_id);
                candidates.push(RawBtCandidate {
                    instance_id,
                    clean_name,
                    mac,
                    percent: raw_pct,
                    is_connected,
                });
            }
        }
    }

    let _ = candidates.first().map(|c| &c.instance_id);
    deduplicate_bt_candidates(candidates)
}

// ======================= DYNAMIC HID.DLL & KERNEL32 IO =======================

#[repr(C)]
#[derive(Default)]
struct HiddAttributes {
    size: u32,
    vendor_id: u16,
    product_id: u16,
    version_number: u16,
}

#[repr(C)]
#[derive(Default)]
struct HidpCaps {
    usage: u16,
    usage_page: u16,
    input_report_byte_length: u16,
    output_report_byte_length: u16,
    feature_report_byte_length: u16,
    reserved: [u16; 17],
    number_link_collection_nodes: u16,
    number_input_button_caps: u16,
    number_input_value_caps: u16,
    number_input_data_indices: u16,
    number_output_button_caps: u16,
    number_output_value_caps: u16,
    number_output_data_indices: u16,
    number_feature_button_caps: u16,
    number_feature_value_caps: u16,
    number_feature_data_indices: u16,
}

type FnHidDGetAttributes = unsafe extern "system" fn(HANDLE, *mut HiddAttributes) -> u8;
type FnHidDGetProductString = unsafe extern "system" fn(HANDLE, *mut u16, u32) -> u8;
type FnHidDGetPreparsedData = unsafe extern "system" fn(HANDLE, *mut *mut c_void) -> u8;
type FnHidDFreePreparsedData = unsafe extern "system" fn(*mut c_void) -> u8;
type FnHidPGetCaps = unsafe extern "system" fn(*mut c_void, *mut HidpCaps) -> i32;
type FnHidDSetFeature = unsafe extern "system" fn(HANDLE, *const u8, u32) -> u8;
type FnHidDGetFeature = unsafe extern "system" fn(HANDLE, *mut u8, u32) -> u8;

struct HidApi {
    _hmod: HMODULE,
    get_attributes: FnHidDGetAttributes,
    get_product_string: FnHidDGetProductString,
    get_preparsed_data: FnHidDGetPreparsedData,
    free_preparsed_data: FnHidDFreePreparsedData,
    get_caps: FnHidPGetCaps,
    set_feature: FnHidDSetFeature,
    get_feature: FnHidDGetFeature,
}

unsafe impl Send for HidApi {}
unsafe impl Sync for HidApi {}

static HID_API: OnceLock<Result<HidApi, String>> = OnceLock::new();

fn load_hid_api() -> Result<&'static HidApi, String> {
    let res = HID_API.get_or_init(|| unsafe {
        let dll = to_wide("hid.dll");
        let hmod = LoadLibraryW(PCWSTR(dll.as_ptr()))
            .map_err(|e| format!("LoadLibraryW(hid.dll) failed: {e}"))?;
        if hmod.is_invalid() {
            return Err("LoadLibraryW(hid.dll) invalid".to_string());
        }
        let get_fn = |name: &str| -> Result<*const (), String> {
            let cs = CString::new(name).map_err(|_| format!("Invalid {name}"))?;
            GetProcAddress(hmod, PCSTR(cs.as_ptr() as *const u8))
                .map(|p| p as *const ())
                .ok_or_else(|| format!("Missing {name} in hid.dll"))
        };
        Ok(HidApi {
            _hmod: hmod,
            get_attributes: std::mem::transmute(get_fn("HidD_GetAttributes")?),
            get_product_string: std::mem::transmute(get_fn("HidD_GetProductString")?),
            get_preparsed_data: std::mem::transmute(get_fn("HidD_GetPreparsedData")?),
            free_preparsed_data: std::mem::transmute(get_fn("HidD_FreePreparsedData")?),
            get_caps: std::mem::transmute(get_fn("HidP_GetCaps")?),
            set_feature: std::mem::transmute(get_fn("HidD_SetFeature")?),
            get_feature: std::mem::transmute(get_fn("HidD_GetFeature")?),
        })
    });
    res.as_ref().map_err(Clone::clone)
}

#[repr(C)]
struct RawOverlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    h_event: HANDLE,
}

type FnReadFile = unsafe extern "system" fn(HANDLE, *mut c_void, u32, *mut u32, *mut RawOverlapped) -> i32;
type FnWriteFile = unsafe extern "system" fn(HANDLE, *const c_void, u32, *mut u32, *mut RawOverlapped) -> i32;
type FnGetOverlappedResult = unsafe extern "system" fn(HANDLE, *const RawOverlapped, *mut u32, i32) -> i32;
type FnCancelIoEx = unsafe extern "system" fn(HANDLE, *const RawOverlapped) -> i32;

struct KernelIoApi {
    read_file: FnReadFile,
    write_file: FnWriteFile,
    get_overlapped_result: FnGetOverlappedResult,
    cancel_io_ex: FnCancelIoEx,
}

static KERNEL_IO_API: OnceLock<Result<KernelIoApi, String>> = OnceLock::new();

fn load_kernel_io() -> Result<&'static KernelIoApi, String> {
    let res = KERNEL_IO_API.get_or_init(|| unsafe {
        let dll = to_wide("kernel32.dll");
        let hmod = GetModuleHandleW(PCWSTR(dll.as_ptr()))
            .or_else(|_| LoadLibraryW(PCWSTR(dll.as_ptr())))
            .map_err(|e| format!("kernel32.dll load failed: {e}"))?;
        let get_fn = |name: &str| -> Result<*const (), String> {
            let cs = CString::new(name).map_err(|_| format!("Invalid {name}"))?;
            GetProcAddress(hmod, PCSTR(cs.as_ptr() as *const u8))
                .map(|p| p as *const ())
                .ok_or_else(|| format!("Missing {name} in kernel32.dll"))
        };
        Ok(KernelIoApi {
            read_file: std::mem::transmute(get_fn("ReadFile")?),
            write_file: std::mem::transmute(get_fn("WriteFile")?),
            get_overlapped_result: std::mem::transmute(get_fn("GetOverlappedResult")?),
            cancel_io_ex: std::mem::transmute(get_fn("CancelIoEx")?),
        })
    });
    res.as_ref().map_err(Clone::clone)
}

fn enumerate_hid_interface_paths() -> Vec<String> {
    let mut paths = Vec::new();
    unsafe {
        let Ok(hdev) = SetupDiGetClassDevsW(
            Some(&GUID_DEVINTERFACE_HID),
            PCWSTR::null(),
            HWND::default(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) else {
            return paths;
        };
        let _guard = DevInfoGuard(hdev);

        let mut idx = 0u32;
        loop {
            let mut iface_data = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(
                hdev,
                None,
                &GUID_DEVINTERFACE_HID,
                idx,
                &mut iface_data,
            )
            .is_err()
            {
                break;
            }
            idx += 1;

            let mut req_size = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(
                hdev,
                &iface_data,
                None,
                0,
                Some(&mut req_size),
                None,
            );
            if req_size <= 4 {
                continue;
            }

            let mut buf = vec![0u32; (req_size as usize).div_ceil(4)];
            buf[0] = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            let detail_ptr = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            if SetupDiGetDeviceInterfaceDetailW(
                hdev,
                &iface_data,
                Some(detail_ptr),
                req_size,
                None,
                None,
            )
            .is_ok()
            {
                let path_u16_ptr = (buf.as_ptr() as *const u8).add(4) as *const u16;
                let max_u16 = (req_size as usize).saturating_sub(4) / 2;
                let u16_slice = std::slice::from_raw_parts(path_u16_ptr, max_u16);
                let end = u16_slice
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(u16_slice.len());
                let path = String::from_utf16_lossy(&u16_slice[..end]);
                if !path.is_empty() {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

pub fn parse_vid_pid_from_hid_path(path: &str) -> Option<(u16, u16)> {
    let low = path.to_ascii_lowercase();
    let vid_pos = low.find("vid_")?;
    let pid_pos = low.find("pid_")?;
    let vid_hex = low.get(vid_pos + 4..vid_pos + 8)?;
    let pid_hex = low.get(pid_pos + 4..pid_pos + 8)?;
    let vid = u16::from_str_radix(vid_hex, 16).ok()?;
    let pid = u16::from_str_radix(pid_hex, 16).ok()?;
    Some((vid, pid))
}

fn open_hid_handle(path: &str, desired_access: u32, flags: FILE_FLAGS_AND_ATTRIBUTES) -> Option<HandleGuard> {
    let w = to_wide(path);
    let h = unsafe {
        CreateFileW(
            PCWSTR(w.as_ptr()),
            desired_access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )
    }
    .ok()?;
    if h == INVALID_HANDLE_VALUE {
        None
    } else {
        HandleGuard::new(h)
    }
}

fn read_hid_attributes(api: &HidApi, handle: HANDLE) -> Option<HiddAttributes> {
    unsafe {
        let mut attrs = HiddAttributes {
            size: std::mem::size_of::<HiddAttributes>() as u32,
            ..Default::default()
        };
        if (api.get_attributes)(handle, &mut attrs) != 0 {
            Some(attrs)
        } else {
            None
        }
    }
}

fn read_hid_caps(api: &HidApi, handle: HANDLE) -> Option<HidpCaps> {
    unsafe {
        let mut preparsed: *mut c_void = std::ptr::null_mut();
        if (api.get_preparsed_data)(handle, &mut preparsed) == 0 || preparsed.is_null() {
            return None;
        }
        let mut caps = HidpCaps::default();
        let st = (api.get_caps)(preparsed, &mut caps);
        let _ = (api.free_preparsed_data)(preparsed);
        if st == HIDP_STATUS_SUCCESS || st >= 0 {
            Some(caps)
        } else {
            None
        }
    }
}

fn read_hid_product_string(api: &HidApi, handle: HANDLE) -> Option<String> {
    unsafe {
        let mut buf = [0u16; 128];
        let byte_len = std::mem::size_of_val(&buf) as u32;
        if (api.get_product_string)(handle, buf.as_mut_ptr(), byte_len) == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

// ======================= BACKEND 2: RAZER USB HID CLEAN-ROOM =======================

#[derive(Debug, Clone, Copy)]
struct RazerDeviceSpec {
    pid: u16,
    name: &'static str,
    kind: &'static str,
    default_tx_id: u8,
}

const RAZER_SKIP_WIRED_PIDS: &[u16] = &[0x0517, 0x0F20, 0x48F0];

const RAZER_WIRELESS_PIDS: &[RazerDeviceSpec] = &[
    RazerDeviceSpec { pid: 0x00CC, name: "Razer Basilisk V3 Pro 35K", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00CD, name: "Razer Basilisk V3 Pro 35K", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00AA, name: "Razer Basilisk V3 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00AB, name: "Razer Basilisk V3 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00A5, name: "Razer Viper V2 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00A6, name: "Razer Viper V2 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00C0, name: "Razer Viper V3 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00C1, name: "Razer Viper V3 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00B6, name: "Razer DeathAdder V3 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00B7, name: "Razer DeathAdder V3 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00C2, name: "Razer DeathAdder V3 HyperSpeed", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00C3, name: "Razer DeathAdder V3 HyperSpeed", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00AF, name: "Razer Cobra Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00B0, name: "Razer Cobra Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00A7, name: "Razer Naga V2 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00A8, name: "Razer Naga V2 Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00B3, name: "Razer HyperPolling Wireless Dongle", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x00A4, name: "Razer Mouse Dock Pro", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x007A, name: "Razer Viper Ultimate", kind: "mouse", default_tx_id: 0x3F },
    RazerDeviceSpec { pid: 0x007B, name: "Razer Viper Ultimate", kind: "mouse", default_tx_id: 0x3F },
    RazerDeviceSpec { pid: 0x0088, name: "Razer Basilisk Ultimate", kind: "mouse", default_tx_id: 0x3F },
    RazerDeviceSpec { pid: 0x008A, name: "Razer Basilisk Ultimate", kind: "mouse", default_tx_id: 0x3F },
    RazerDeviceSpec { pid: 0x0090, name: "Razer Orochi V2", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x0091, name: "Razer Orochi V2", kind: "mouse", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x0528, name: "Razer Barracuda Pro", kind: "headphones", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x0543, name: "Razer BlackShark V2 Pro", kind: "headphones", default_tx_id: 0x1F },
    RazerDeviceSpec { pid: 0x029C, name: "Razer BlackWidow V3 Mini", kind: "keyboard", default_tx_id: 0x1F },
];

fn lookup_razer_wireless_spec(pid: u16) -> Option<&'static RazerDeviceSpec> {
    if RAZER_SKIP_WIRED_PIDS.contains(&pid) {
        return None;
    }
    RAZER_WIRELESS_PIDS.iter().find(|spec| spec.pid == pid)
}

/// Computes the 1-byte XOR checksum over payload offsets `[2..88]` (`buf[3..=88]`).
pub fn compute_razer_crc(report: &[u8; RAZER_REPORT_LEN]) -> u8 {
    let mut crc = 0u8;
    for &b in &report[3..=88] {
        crc ^= b;
    }
    crc
}

/// Clean-room 91-byte Razer USB HID feature report builder (1-byte Report ID `0x00` + 90-byte payload).
pub fn build_razer_battery_report(tx_id: u8, cmd_id: u8) -> [u8; RAZER_REPORT_LEN] {
    let mut buf = [0u8; RAZER_REPORT_LEN];
    buf[0] = 0x00; // Report ID
    buf[1] = 0x00; // status: new command
    buf[2] = tx_id;
    buf[3] = 0x00; // remaining_packets hi
    buf[4] = 0x00; // remaining_packets lo
    buf[5] = 0x00; // protocol_type
    buf[6] = 0x02; // data_size
    buf[7] = RAZER_CMD_CLASS_POWER;
    buf[8] = cmd_id;
    buf[89] = compute_razer_crc(&buf);
    buf[90] = 0x00;
    buf
}

pub fn map_razer_raw_battery(raw: u8) -> u8 {
    ((raw as u32 * 100 + 127) / 255).min(100) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RazerProbeResponse {
    BatteryRaw(u8),
    SleepingOrUnavailable(u8),
    Invalid,
}

pub fn parse_razer_battery_response(buf: &[u8; RAZER_REPORT_LEN], expected_cmd: u8) -> RazerProbeResponse {
    let status = buf[1];
    if buf[7] != RAZER_CMD_CLASS_POWER || buf[8] != expected_cmd {
        return RazerProbeResponse::Invalid;
    }
    if status == RAZER_STATUS_OK {
        RazerProbeResponse::BatteryRaw(buf[10])
    } else if status == RAZER_STATUS_UNSUPPORTED || status == 0x04 {
        RazerProbeResponse::SleepingOrUnavailable(status)
    } else {
        RazerProbeResponse::Invalid
    }
}

fn razer_tx_candidates(default_tx: u8) -> [u8; 3] {
    match default_tx {
        0x3F => [0x3F, 0x1F, 0xFF],
        0xFF => [0xFF, 0x1F, 0x3F],
        _ => [0x1F, 0x3F, 0xFF],
    }
}

fn razer_feature_exchange(
    api: &HidApi,
    handle: HANDLE,
    tx_id: u8,
    cmd_id: u8,
) -> Result<[u8; RAZER_REPORT_LEN], u32> {
    let req = build_razer_battery_report(tx_id, cmd_id);
    unsafe {
        if (api.set_feature)(handle, req.as_ptr(), req.len() as u32) == 0 {
            return Err(GetLastError().0);
        }
        std::thread::sleep(Duration::from_millis(25));
        let mut resp = [0u8; RAZER_REPORT_LEN];
        resp[0] = 0x00;
        if (api.get_feature)(handle, resp.as_mut_ptr(), resp.len() as u32) == 0 {
            return Err(GetLastError().0);
        }
        Ok(resp)
    }
}

fn resolve_razer_reading_with_cache(
    device_id: &str,
    live_raw: Option<u8>,
    live_charging: bool,
    now: Instant,
) -> Option<(u8, bool)> {
    let cache = RAZER_LAST_VALID.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    match live_raw {
        Some(raw) if raw > 0 => {
            let pct = map_razer_raw_battery(raw);
            guard.insert(device_id.to_string(), (now, pct, live_charging));
            Some((pct, live_charging))
        }
        _ => {
            if let Some(&(ts, pct, chg)) = guard.get(device_id) {
                if now.saturating_duration_since(ts) < RAZER_READING_CACHE_TTL {
                    return Some((pct, chg));
                }
            }
            None
        }
    }
}

fn scan_razer_hid_batteries() -> Vec<PeripheralBattery> {
    let Ok(api) = load_hid_api() else {
        return Vec::new();
    };
    let paths = enumerate_hid_interface_paths();
    let mut best_by_name: HashMap<String, (bool, PeripheralBattery)> = HashMap::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for path in paths {
        let Some((vid, pid)) = parse_vid_pid_from_hid_path(&path) else {
            continue;
        };
        if vid != RAZER_VID {
            continue;
        }
        let Some(spec) = lookup_razer_wireless_spec(pid) else {
            continue;
        };

        let device_id = format!("razer:{pid:04x}");
        if seen_ids.contains(&device_id) {
            continue;
        }

        let Some(zero_guard) = open_hid_handle(&path, 0, FILE_FLAGS_AND_ATTRIBUTES(0)) else {
            continue;
        };
        if let Some(attrs) = read_hid_attributes(api, zero_guard.0) {
            if attrs.vendor_id != RAZER_VID || attrs.product_id != pid {
                continue;
            }
        }
        let Some(caps) = read_hid_caps(api, zero_guard.0) else {
            continue;
        };
        if caps.feature_report_byte_length as usize != RAZER_REPORT_LEN {
            continue;
        }

        let prod_name = read_hid_product_string(api, zero_guard.0);
        let display_name = prod_name
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| spec.name.to_string());

        let mut active_guard: Option<HandleGuard> = Some(zero_guard);
        let mut rw_attempted = false;
        let mut matched_reading: Option<(RazerProbeResponse, u8)> = None;

        for tx_id in razer_tx_candidates(spec.default_tx_id) {
            let handle = match active_guard.as_ref() {
                Some(g) => g.0,
                None => break,
            };

            let exchange = match razer_feature_exchange(api, handle, tx_id, RAZER_CMD_GET_BATTERY) {
                Ok(resp) => Ok(resp),
                Err(_) if !rw_attempted => {
                    rw_attempted = true;
                    drop(active_guard.take());
                    active_guard = open_hid_handle(
                        &path,
                        GENERIC_READ_FLAG | GENERIC_WRITE_FLAG,
                        FILE_FLAGS_AND_ATTRIBUTES(0),
                    );
                    if let Some(ref rw) = active_guard {
                        razer_feature_exchange(api, rw.0, tx_id, RAZER_CMD_GET_BATTERY)
                    } else {
                        break;
                    }
                }
                Err(e) => Err(e),
            };

            if let Ok(resp) = exchange {
                let parsed = parse_razer_battery_response(&resp, RAZER_CMD_GET_BATTERY);
                match parsed {
                    RazerProbeResponse::BatteryRaw(_) | RazerProbeResponse::SleepingOrUnavailable(_) => {
                        matched_reading = Some((parsed, tx_id));
                        break;
                    }
                    RazerProbeResponse::Invalid => {}
                }
            }
        }

        let Some((probe, tx_id)) = matched_reading else {
            continue;
        };

        let mut charging = false;
        if matches!(probe, RazerProbeResponse::BatteryRaw(raw) if raw > 0) {
            if let Some(ref g) = active_guard {
                if let Ok(chg_resp) = razer_feature_exchange(api, g.0, tx_id, RAZER_CMD_GET_CHARGING) {
                    if let RazerProbeResponse::BatteryRaw(flag) =
                        parse_razer_battery_response(&chg_resp, RAZER_CMD_GET_CHARGING)
                    {
                        charging = flag != 0;
                    }
                }
            }
        }

        let live_raw = match probe {
            RazerProbeResponse::BatteryRaw(raw) => Some(raw),
            RazerProbeResponse::SleepingOrUnavailable(_) | RazerProbeResponse::Invalid => None,
        };
        let is_live = matches!(live_raw, Some(r) if r > 0);

        if let Some((percent, resolved_charging)) =
            resolve_razer_reading_with_cache(&device_id, live_raw, charging, Instant::now())
        {
            seen_ids.insert(device_id.clone());
            let entry = PeripheralBattery {
                id: device_id,
                name: display_name.clone(),
                kind: spec.kind.to_string(),
                percent,
                charging: resolved_charging,
                source: "razer_hid".to_string(),
                connected: true,
                sub_levels: None,
            };
            let norm = display_name.trim().to_ascii_lowercase();
            match best_by_name.get_mut(&norm) {
                Some((prev_live, prev_entry)) => {
                    if is_live && !*prev_live {
                        *prev_live = true;
                        *prev_entry = entry;
                    }
                }
                None => {
                    best_by_name.insert(norm, (is_live, entry));
                }
            }
        }
    }

    best_by_name.into_values().map(|(_, entry)| entry).collect()
}

// ======================= BACKEND 3: LOGITECH HID++ 2.0 =======================

const LOGITECH_RECEIVER_PIDS: &[u16] = &[
    0xC52B, 0xC52F, 0xC534, 0xC539, 0xC53A, 0xC53D, 0xC53F, 0xC547, 0xC548,
];

fn build_hidpp_request(
    use_long: bool,
    report_len: usize,
    dev_idx: u8,
    feature_idx: u8,
    func: u8,
    sw_id: u8,
    params: &[u8],
) -> Vec<u8> {
    let min_len = if use_long { 20 } else { 7 };
    let mut pkt = vec![0u8; report_len.max(min_len)];
    pkt[0] = if use_long { 0x11 } else { 0x10 };
    pkt[1] = dev_idx;
    pkt[2] = feature_idx;
    pkt[3] = ((func & 0x0F) << 4) | (sw_id & 0x0F);
    let max_params = pkt.len().saturating_sub(4);
    let n = params.len().min(max_params);
    pkt[4..4 + n].copy_from_slice(&params[..n]);
    pkt
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidppBatteryReading {
    pub percent: u8,
    pub charging: bool,
}

pub fn parse_hidpp_unified_battery_status(params: &[u8]) -> Option<HidppBatteryReading> {
    if params.len() < 3 {
        return None;
    }
    let soc = params[0];
    let level_mask = params[1];
    let chg_status = params[2];
    let percent = if soc <= 100 && soc > 0 {
        soc
    } else if soc == 0 && (level_mask & 0x0F) != 0 {
        if (level_mask & 0x08) != 0 {
            90
        } else if (level_mask & 0x04) != 0 {
            60
        } else if (level_mask & 0x02) != 0 {
            25
        } else {
            10
        }
    } else if soc <= 100 {
        soc
    } else {
        return None;
    };
    let charging = matches!(chg_status, 1..=4);
    Some(HidppBatteryReading { percent, charging })
}

pub fn parse_hidpp_battery_level_status(params: &[u8]) -> Option<HidppBatteryReading> {
    if params.len() < 3 {
        return None;
    }
    let percent = params[0];
    if percent > 100 {
        return None;
    }
    let charging = matches!(params[2], 1..=4);
    Some(HidppBatteryReading { percent, charging })
}

fn hidpp_exchange(
    io: &KernelIoApi,
    handle: HANDLE,
    use_long: bool,
    out_len: usize,
    in_len: usize,
    dev_idx: u8,
    feature_idx: u8,
    func: u8,
    params: &[u8],
) -> Option<Vec<u8>> {
    const SW_ID: u8 = 0x0A;
    let req = build_hidpp_request(use_long, out_len, dev_idx, feature_idx, func, SW_ID, params);

    unsafe {
        let Ok(write_evt) = CreateEventW(None, false, false, PCWSTR::null()) else {
            return None;
        };
        let _wevt_guard = HandleGuard::new(write_evt)?;
        let mut ov_write = RawOverlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            h_event: write_evt,
        };
        let mut written = 0u32;
        let wr = (io.write_file)(
            handle,
            req.as_ptr() as *const c_void,
            req.len() as u32,
            &mut written,
            &mut ov_write,
        );
        if wr == 0 {
            let err = GetLastError().0;
            if err != ERROR_IO_PENDING.0 {
                return None;
            }
            if WaitForSingleObject(write_evt, HIDPP_IO_TIMEOUT_MS) != WAIT_OBJECT_0 {
                let _ = (io.cancel_io_ex)(handle, &ov_write);
                let mut dummy = 0u32;
                let _ = (io.get_overlapped_result)(handle, &ov_write, &mut dummy, 1);
                return None;
            }
            let mut transferred = 0u32;
            if (io.get_overlapped_result)(handle, &ov_write, &mut transferred, 0) == 0 {
                return None;
            }
        }

        let deadline = Instant::now() + Duration::from_millis(HIDPP_IO_TIMEOUT_MS as u64);
        let read_size = in_len.max(20);
        let Ok(read_evt) = CreateEventW(None, false, false, PCWSTR::null()) else {
            return None;
        };
        let _revt_guard = HandleGuard::new(read_evt)?;

        for _ in 0..8 {
            let rem_ms = deadline
                .saturating_duration_since(Instant::now())
                .as_millis() as u32;
            if rem_ms == 0 {
                break;
            }
            let mut rbuf = vec![0u8; read_size];
            let mut ov_read = RawOverlapped {
                internal: 0,
                internal_high: 0,
                offset: 0,
                offset_high: 0,
                h_event: read_evt,
            };
            let mut read_bytes = 0u32;
            let rr = (io.read_file)(
                handle,
                rbuf.as_mut_ptr() as *mut c_void,
                rbuf.len() as u32,
                &mut read_bytes,
                &mut ov_read,
            );
            if rr == 0 {
                let err = GetLastError().0;
                if err != ERROR_IO_PENDING.0 {
                    return None;
                }
                let wait_rc = WaitForSingleObject(read_evt, rem_ms);
                if wait_rc == WAIT_TIMEOUT || wait_rc != WAIT_OBJECT_0 {
                    let _ = (io.cancel_io_ex)(handle, &ov_read);
                    let mut dummy = 0u32;
                    let _ = (io.get_overlapped_result)(handle, &ov_read, &mut dummy, 1);
                    return None;
                }
                if (io.get_overlapped_result)(handle, &ov_read, &mut read_bytes, 0) == 0 {
                    return None;
                }
            }
            let n = (read_bytes as usize).min(rbuf.len());
            if n < 7 {
                continue;
            }
            if (rbuf[0] != 0x10 && rbuf[0] != 0x11) || rbuf[1] != dev_idx {
                continue;
            }
            if rbuf[2] == 0xFF || rbuf[2] == 0x8F {
                return None;
            }
            if rbuf[2] == feature_idx && (rbuf[3] & 0x0F) == SW_ID {
                return Some(rbuf[4..n].to_vec());
            }
        }
    }
    None
}

fn probe_hidpp_device_index(
    io: &KernelIoApi,
    handle: HANDLE,
    use_long: bool,
    out_len: usize,
    in_len: usize,
    dev_idx: u8,
) -> Option<(HidppBatteryReading, Option<String>)> {
    // IRoot (0x00) -> GetFeature(IFeatureSet 0x0001) to confirm HID++ 2.0 device presence
    let root_fset = hidpp_exchange(
        io,
        handle,
        use_long,
        out_len,
        in_len,
        dev_idx,
        0x00,
        0x00,
        &[0x00, 0x01, 0x00],
    )?;
    if root_fset.first().copied().unwrap_or(0) == 0 {
        return None;
    }

    // Try Unified Battery (0x1004) first
    if let Some(ub_feat) = hidpp_exchange(
        io,
        handle,
        use_long,
        out_len,
        in_len,
        dev_idx,
        0x00,
        0x00,
        &[0x10, 0x04, 0x00],
    ) {
        let feat_idx = ub_feat.first().copied().unwrap_or(0);
        if feat_idx != 0 {
            if let Some(status_params) = hidpp_exchange(
                io, handle, use_long, out_len, in_len, dev_idx, feat_idx, 0x01, &[],
            ) {
                if let Some(reading) = parse_hidpp_unified_battery_status(&status_params) {
                    let custom_name = query_hidpp_device_name(io, handle, use_long, out_len, in_len, dev_idx);
                    return Some((reading, custom_name));
                }
            }
        }
    }

    // Fallback: Battery Level Status (0x1000)
    if let Some(bs_feat) = hidpp_exchange(
        io,
        handle,
        use_long,
        out_len,
        in_len,
        dev_idx,
        0x00,
        0x00,
        &[0x10, 0x00, 0x00],
    ) {
        let feat_idx = bs_feat.first().copied().unwrap_or(0);
        if feat_idx != 0 {
            if let Some(status_params) = hidpp_exchange(
                io, handle, use_long, out_len, in_len, dev_idx, feat_idx, 0x00, &[],
            ) {
                if let Some(reading) = parse_hidpp_battery_level_status(&status_params) {
                    let custom_name = query_hidpp_device_name(io, handle, use_long, out_len, in_len, dev_idx);
                    return Some((reading, custom_name));
                }
            }
        }
    }

    None
}

fn query_hidpp_device_name(
    io: &KernelIoApi,
    handle: HANDLE,
    use_long: bool,
    out_len: usize,
    in_len: usize,
    dev_idx: u8,
) -> Option<String> {
    // DeviceName feature 0x0005
    let feat = hidpp_exchange(
        io,
        handle,
        use_long,
        out_len,
        in_len,
        dev_idx,
        0x00,
        0x00,
        &[0x00, 0x05, 0x00],
    )?;
    let feat_idx = feat.first().copied().unwrap_or(0);
    if feat_idx == 0 {
        return None;
    }
    let count_params = hidpp_exchange(io, handle, use_long, out_len, in_len, dev_idx, feat_idx, 0x00, &[])?;
    let name_len = count_params.first().copied().unwrap_or(0) as usize;
    if name_len == 0 || name_len > 64 {
        return None;
    }
    let mut name_bytes = Vec::with_capacity(name_len);
    let mut offset = 0usize;
    while offset < name_len {
        let chunk = hidpp_exchange(
            io,
            handle,
            use_long,
            out_len,
            in_len,
            dev_idx,
            feat_idx,
            0x01,
            &[offset as u8],
        )?;
        let take = chunk.len().min(name_len - offset);
        if take == 0 {
            break;
        }
        name_bytes.extend_from_slice(&chunk[..take]);
        offset += take;
    }
    let clean = String::from_utf8_lossy(&name_bytes)
        .trim_end_matches('\0')
        .trim()
        .to_string();
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

fn scan_logitech_hidpp_batteries() -> Vec<PeripheralBattery> {
    let Ok(hid_api) = load_hid_api() else {
        return Vec::new();
    };
    let Ok(io) = load_kernel_io() else {
        return Vec::new();
    };

    let paths = enumerate_hid_interface_paths();
    let mut results = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();

    for path in paths {
        let Some((vid, pid)) = parse_vid_pid_from_hid_path(&path) else {
            continue;
        };
        if vid != LOGITECH_VID {
            continue;
        }

        let Some(zero_guard) = open_hid_handle(&path, 0, FILE_FLAGS_AND_ATTRIBUTES(0)) else {
            continue;
        };
        let Some(caps) = read_hid_caps(hid_api, zero_guard.0) else {
            continue;
        };
        if caps.usage_page != 0xFF00 {
            continue;
        }
        let out_len = caps.output_report_byte_length as usize;
        let in_len = caps.input_report_byte_length as usize;
        if out_len < 7 || in_len < 7 {
            continue;
        }
        let use_long = out_len >= 20;
        let fallback_name = read_hid_product_string(hid_api, zero_guard.0)
            .unwrap_or_else(|| format!("Logitech {pid:04X}"));
        drop(zero_guard);

        let Some(rw_guard) = open_hid_handle(
            &path,
            GENERIC_READ_FLAG | GENERIC_WRITE_FLAG,
            FILE_FLAG_OVERLAPPED,
        ) else {
            continue;
        };

        let indices: Vec<u8> = if LOGITECH_RECEIVER_PIDS.contains(&pid) {
            vec![1, 2, 3, 4, 5, 6]
        } else {
            vec![0xFF, 1, 2]
        };

        for dev_idx in indices {
            if let Some((reading, custom_name)) =
                probe_hidpp_device_index(io, rw_guard.0, use_long, out_len, in_len, dev_idx)
            {
                let name = custom_name.unwrap_or_else(|| fallback_name.clone());
                let key = format!("logitech:{pid:04x}:{dev_idx:02x}");
                let name_dedup = format!("logi:{}", name.to_ascii_lowercase());
                if seen_keys.insert(name_dedup) {
                    let kind = classify_peripheral_kind(&name).to_string();
                    results.push(PeripheralBattery {
                        id: key,
                        name,
                        kind,
                        percent: reading.percent,
                        charging: reading.charging,
                        source: "logitech_hidpp".to_string(),
                        connected: true,
                        sub_levels: None,
                    });
                }
                if dev_idx == 0xFF {
                    break;
                }
            }
        }
    }

    results
}

// ======================= BACKEND 4: XINPUT WIRELESS GAMEPADS =======================

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct XInputBatteryInformation {
    battery_type: u8,
    battery_level: u8,
}

type FnXInputGetBatteryInformation = unsafe extern "system" fn(
    dw_user_index: u32,
    dev_type: u8,
    p_battery_information: *mut XInputBatteryInformation,
) -> u32;

struct XInputApi {
    _hmod: HMODULE,
    get_battery_info: FnXInputGetBatteryInformation,
}

unsafe impl Send for XInputApi {}
unsafe impl Sync for XInputApi {}

static XINPUT_API: OnceLock<Option<XInputApi>> = OnceLock::new();

fn load_xinput_api() -> Option<&'static XInputApi> {
    XINPUT_API
        .get_or_init(|| unsafe {
            let dll = to_wide("xinput1_4.dll");
            let hmod = LoadLibraryW(PCWSTR(dll.as_ptr())).ok()?;
            if hmod.is_invalid() {
                return None;
            }
            let cs = CString::new("XInputGetBatteryInformation").ok()?;
            let ptr = GetProcAddress(hmod, PCSTR(cs.as_ptr() as *const u8))?;
            Some(XInputApi {
                _hmod: hmod,
                get_battery_info: std::mem::transmute(ptr),
            })
        })
        .as_ref()
}

pub fn map_xinput_battery(slot: u32, battery_type: u8, battery_level: u8) -> Option<PeripheralBattery> {
    if battery_type == XINPUT_BATTERY_TYPE_DISCONNECTED
        || battery_type == XINPUT_BATTERY_TYPE_WIRED
        || (battery_type != XINPUT_BATTERY_TYPE_ALKALINE && battery_type != XINPUT_BATTERY_TYPE_NIMH)
    {
        return None;
    }

    let percent = match battery_level {
        0 => 10,
        1 => 30,
        2 => 65,
        3 => 100,
        _ => return None,
    };

    Some(PeripheralBattery {
        id: format!("xinput:{slot}"),
        name: format!("Xbox Геймпад #{}", slot + 1),
        kind: "gamepad".to_string(),
        percent,
        charging: false,
        source: "xinput".to_string(),
        connected: true,
        sub_levels: None,
    })
}

fn scan_xinput_batteries() -> Vec<PeripheralBattery> {
    let Some(api) = load_xinput_api() else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for slot in 0..4u32 {
        let mut info = XInputBatteryInformation::default();
        let rc = unsafe { (api.get_battery_info)(slot, XINPUT_DEVTYPE_GAMEPAD, &mut info) };
        if rc == 0 {
            if let Some(entry) = map_xinput_battery(slot, info.battery_type, info.battery_level) {
                results.push(entry);
            }
        }
    }
    results
}

// ======================= UNIFIED SCAN & CACHE =======================

fn merge_peripheral_batteries(
    razer: Vec<PeripheralBattery>,
    logitech: Vec<PeripheralBattery>,
    bluetooth: Vec<PeripheralBattery>,
    xinput: Vec<PeripheralBattery>,
) -> Vec<PeripheralBattery> {
    let mut out = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    // Highest-fidelity protocols first (USB HID / HID++ report charging & higher resolution than BT)
    for item in razer.into_iter().chain(logitech).chain(bluetooth).chain(xinput) {
        let norm_name = item.name.trim().to_ascii_lowercase();
        if seen_names.insert(norm_name) {
            out.push(item);
        }
    }

    out.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    out
}

fn scan_all_peripheral_batteries_live() -> Vec<PeripheralBattery> {
    let bt = scan_pnp_bluetooth_batteries();
    let razer = scan_razer_hid_batteries();
    let logi = scan_logitech_hidpp_batteries();
    let xinput = scan_xinput_batteries();
    merge_peripheral_batteries(razer, logi, bt, xinput)
}

pub fn get_peripheral_batteries() -> Vec<PeripheralBattery> {
    let cache = BATTERY_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((ts, ref list)) = *guard {
            if ts.elapsed() < BATTERY_CACHE_TTL {
                return list.clone();
            }
        }
    }

    let scan_lock = BATTERY_SCAN_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_scan_guard) = scan_lock.try_lock() else {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        return guard.as_ref().map(|(_, list)| list.clone()).unwrap_or_default();
    };

    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((ts, ref list)) = *guard {
            if ts.elapsed() < BATTERY_CACHE_TTL {
                return list.clone();
            }
        }
    }

    let fresh = scan_all_peripheral_batteries_live();
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((Instant::now(), fresh.clone()));
    fresh
}

pub fn refresh_peripheral_batteries() -> Vec<PeripheralBattery> {
    let scan_lock = BATTERY_SCAN_LOCK.get_or_init(|| Mutex::new(()));
    let _scan_guard = scan_lock.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = scan_all_peripheral_batteries_live();
    let cache = BATTERY_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((Instant::now(), fresh.clone()));
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn razer_report_crc_and_offsets_match_protocol_spec() {
        let pkt_bat = build_razer_battery_report(0x1F, RAZER_CMD_GET_BATTERY);
        assert_eq!(pkt_bat.len(), 91);
        assert_eq!(pkt_bat[0], 0x00); // Report ID
        assert_eq!(pkt_bat[1], 0x00); // Status = new command
        assert_eq!(pkt_bat[2], 0x1F); // tx_id
        assert_eq!(pkt_bat[6], 0x02); // data_size
        assert_eq!(pkt_bat[7], 0x07); // class = power
        assert_eq!(pkt_bat[8], 0x80); // cmd = get battery
        assert_eq!(pkt_bat[89], 0x02 ^ 0x07 ^ 0x80);
        assert_eq!(pkt_bat[90], 0x00);

        let pkt_chg = build_razer_battery_report(0x3F, RAZER_CMD_GET_CHARGING);
        assert_eq!(pkt_chg[2], 0x3F);
        assert_eq!(pkt_chg[8], 0x84);
        assert_eq!(pkt_chg[89], 0x02 ^ 0x07 ^ 0x84);
    }

    #[test]
    fn razer_response_parsing_and_sleep_cache_work() {
        assert_eq!(map_razer_raw_battery(0), 0);
        assert_eq!(map_razer_raw_battery(128), 50);
        assert_eq!(map_razer_raw_battery(255), 100);

        let mut resp = [0u8; RAZER_REPORT_LEN];
        resp[1] = 0x02;
        resp[7] = 0x07;
        resp[8] = 0x80;
        resp[10] = 204; // ~80%
        assert_eq!(
            parse_razer_battery_response(&resp, RAZER_CMD_GET_BATTERY),
            RazerProbeResponse::BatteryRaw(204)
        );

        resp[1] = 0x05; // sleeping / unsupported
        assert_eq!(
            parse_razer_battery_response(&resp, RAZER_CMD_GET_BATTERY),
            RazerProbeResponse::SleepingOrUnavailable(0x05)
        );

        resp[1] = 0x02;
        resp[7] = 0x01; // mismatched class
        assert_eq!(
            parse_razer_battery_response(&resp, RAZER_CMD_GET_BATTERY),
            RazerProbeResponse::Invalid
        );

        // Cache preserves last valid reading during sleep
        let t0 = Instant::now();
        let dev = "razer:test-00cc";
        assert_eq!(
            resolve_razer_reading_with_cache(dev, Some(204), true, t0),
            Some((80, true))
        );
        assert_eq!(
            resolve_razer_reading_with_cache(dev, None, false, t0 + Duration::from_secs(60)),
            Some((80, true))
        );
        assert_eq!(
            resolve_razer_reading_with_cache(dev, Some(0), false, t0 + Duration::from_secs(120)),
            Some((80, true))
        );
        // Expired after 15 min
        assert_eq!(
            resolve_razer_reading_with_cache(
                dev,
                None,
                false,
                t0 + Duration::from_secs(15 * 60 + 1)
            ),
            None
        );
    }

    #[test]
    fn pnp_name_cleaning_mac_extraction_and_dedup_work() {
        assert_eq!(clean_bt_device_name("AirPods Max Hands-Free AG"), "AirPods Max");
        assert_eq!(clean_bt_device_name("AirPods Max - Find My"), "AirPods Max");
        assert_eq!(
            clean_bt_device_name("Sony WH-1000XM5 Avrcp Transport"),
            "Sony WH-1000XM5"
        );
        assert_eq!(clean_bt_device_name("Bose QC Ultra Stereo"), "Bose QC Ultra");
        assert_eq!(clean_bt_device_name("Beats Studio Hands-Free HF"), "Beats Studio");

        assert_eq!(classify_peripheral_kind("AirPods Max"), "headphones");
        assert_eq!(classify_peripheral_kind("Razer Basilisk V3 Pro 35K"), "mouse");
        assert_eq!(classify_peripheral_kind("Keychron K3 Pro Keyboard"), "keyboard");
        assert_eq!(classify_peripheral_kind("Xbox Wireless Controller"), "gamepad");

        assert_eq!(
            extract_bt_mac_from_instance_id(
                r"BTHENUM\{0000111e-0000-1000-8000-00805f9b34fb}_LOCALMFG&000f\7&123456&0&AABBCCDDEEFF_C00000000"
            ),
            Some("AABBCCDDEEFF".to_string())
        );
        assert_eq!(
            extract_bt_mac_from_instance_id(r"BTHLE\Dev_112233445566\8&abc&0&0011"),
            Some("112233445566".to_string())
        );

        let deduped = deduplicate_bt_candidates(vec![
            RawBtCandidate {
                instance_id: "BTHENUM\\DEV_AABBCCDDEEFF".to_string(),
                clean_name: "AirPods Max".to_string(),
                mac: Some("AABBCCDDEEFF".to_string()),
                percent: 87,
                is_connected: None, // classic BT audio entry without IsConnected -> keep!
            },
            RawBtCandidate {
                instance_id: "BTHLE\\DEV_AABBCCDDEEFF".to_string(),
                clean_name: "AirPods Max".to_string(),
                mac: Some("AABBCCDDEEFF".to_string()),
                percent: 87,
                is_connected: Some(true),
            },
            RawBtCandidate {
                instance_id: "BTHLE\\DEV_998877665544".to_string(),
                clean_name: "Old Headphones".to_string(),
                mac: Some("998877665544".to_string()),
                percent: 50,
                is_connected: Some(false), // explicitly disconnected -> skip!
            },
        ]);

        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].id, "bt:aabbccddeeff");
        assert_eq!(deduped[0].name, "AirPods Max");
        assert_eq!(deduped[0].kind, "headphones");
        assert_eq!(deduped[0].percent, 87);
        assert_eq!(deduped[0].source, "bluetooth");
    }

    #[test]
    fn hid_path_parser_logitech_and_xinput_mapping_work() {
        assert_eq!(
            parse_vid_pid_from_hid_path(
                r"\\?\hid#vid_1532&pid_00cd&mi_02#7&1234#{4d1e55b2-f16f-11cf-88cb-001111000030}"
            ),
            Some((0x1532, 0x00CD))
        );
        assert!(lookup_razer_wireless_spec(0x00CC).is_some());
        assert!(lookup_razer_wireless_spec(0x00CD).is_some());
        assert!(lookup_razer_wireless_spec(0x0517).is_none());
        assert!(lookup_razer_wireless_spec(0x0F20).is_none());
        assert!(lookup_razer_wireless_spec(0x48F0).is_none());

        assert_eq!(
            parse_hidpp_unified_battery_status(&[74, 0x04, 1]),
            Some(HidppBatteryReading {
                percent: 74,
                charging: true
            })
        );
        assert_eq!(
            parse_hidpp_battery_level_status(&[45, 30, 0]),
            Some(HidppBatteryReading {
                percent: 45,
                charging: false
            })
        );

        assert_eq!(map_xinput_battery(0, XINPUT_BATTERY_TYPE_DISCONNECTED, 3), None);
        assert_eq!(map_xinput_battery(0, XINPUT_BATTERY_TYPE_WIRED, 3), None);
        let pad = map_xinput_battery(1, XINPUT_BATTERY_TYPE_NIMH, 2).unwrap();
        assert_eq!(pad.id, "xinput:1");
        assert_eq!(pad.kind, "gamepad");
        assert_eq!(pad.percent, 65);
        assert_eq!(pad.source, "xinput");
    }

    #[test]
    #[ignore]
    fn probe_live_devices_diag() {
        println!("=== PnP Bluetooth Battery Diagnostic ===");
        for enumerator in ["BTHENUM", "BTH", "BTHLE", "BTHLEDEVICE"] {
            let enum_w = to_wide(enumerator);
            unsafe {
                let Ok(hdev) = SetupDiGetClassDevsW(
                    None,
                    PCWSTR(enum_w.as_ptr()),
                    HWND::default(),
                    DIGCF_PRESENT | DIGCF_ALLCLASSES,
                ) else {
                    println!("[{enumerator}] SetupDiGetClassDevsW failed");
                    continue;
                };
                let _guard = DevInfoGuard(hdev);
                let mut idx = 0u32;
                loop {
                    let mut info = SP_DEVINFO_DATA {
                        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                        ..Default::default()
                    };
                    if SetupDiEnumDeviceInfo(hdev, idx, &mut info).is_err() {
                        break;
                    }
                    idx += 1;
                    let inst = read_dev_instance_id(hdev, &info);
                    let fname = read_dev_string_prop(hdev, &info, &DEVPKEY_Device_FriendlyName);
                    let desc = read_dev_string_prop(hdev, &info, &DEVPKEY_Device_DeviceDesc);
                    let conn = read_dev_is_connected(hdev, &info);
                    let mut prop_type = DEVPROPTYPE(0);
                    let mut bat_buf = [0u8; 4];
                    let mut req = 0u32;
                    let has_bat = SetupDiGetDevicePropertyW(
                        hdev,
                        &info,
                        &DEVPKEY_BLUETOOTH_BATTERY,
                        &mut prop_type,
                        Some(&mut bat_buf),
                        Some(&mut req),
                        0,
                    )
                    .is_ok()
                        && req > 0;
                    if has_bat || fname.is_some() {
                        println!(
                            "[{enumerator}] inst={inst} name={fname:?} desc={desc:?} conn={conn:?} has_bat={has_bat} bat={:?}",
                            &bat_buf[..req.min(4) as usize]
                        );
                    }
                }
            }
        }

        println!("=== Razer VID_1532 HID Diagnostic ===");
        if let Ok(hid_api) = load_hid_api() {
            for path in enumerate_hid_interface_paths() {
                if let Some((vid, pid)) = parse_vid_pid_from_hid_path(&path) {
                    if vid == RAZER_VID {
                        let g = open_hid_handle(&path, 0, FILE_FLAGS_AND_ATTRIBUTES(0));
                        let caps = g.as_ref().and_then(|h| read_hid_caps(hid_api, h.0));
                        let prod = g.as_ref().and_then(|h| read_hid_product_string(hid_api, h.0));
                        println!(
                            "[RAZER] pid=0x{pid:04X} prod={prod:?} feat_len={:?} usage_page={:?} path={path}",
                            caps.as_ref().map(|c| c.feature_report_byte_length),
                            caps.as_ref().map(|c| format!("0x{:04X}", c.usage_page)),
                        );
                        if caps.as_ref().is_some_and(|c| c.feature_report_byte_length == 91) {
                            for tx in [0x1F, 0x3F, 0xFF] {
                                if let Some(ref handle_g) = g {
                                    let res = razer_feature_exchange(
                                        hid_api,
                                        handle_g.0,
                                        tx,
                                        RAZER_CMD_GET_BATTERY,
                                    );
                                    println!(
                                        "  tx=0x{tx:02X} (access=0): {:?}",
                                        res.map(|r| (r[1], r[7], r[8], r[9], r[10]))
                                    );
                                }
                                if let Some(rw) = open_hid_handle(
                                    &path,
                                    GENERIC_READ_FLAG | GENERIC_WRITE_FLAG,
                                    FILE_FLAGS_AND_ATTRIBUTES(0),
                                ) {
                                    let res = razer_feature_exchange(
                                        hid_api,
                                        rw.0,
                                        tx,
                                        RAZER_CMD_GET_BATTERY,
                                    );
                                    println!(
                                        "  tx=0x{tx:02X} (RW): {:?}",
                                        res.map(|r| (r[1], r[7], r[8], r[9], r[10]))
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        println!("=== Final refresh_peripheral_batteries() ===");
        let final_list = refresh_peripheral_batteries();
        println!("{final_list:#?}");
    }
}
