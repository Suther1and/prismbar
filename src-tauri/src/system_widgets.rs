use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use sysinfo::{ProcessesToUpdate, System};
use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_FriendlyName, DEVPROPTYPE, DEVPROPKEY,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetForegroundWindow, GetMessageW,
    GetWindowThreadProcessId, PostMessageW, RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG,
    WM_CLIPBOARDUPDATE, WM_INPUTLANGCHANGEREQUEST, WNDCLASSW,
};

use crate::win_shell::wide;
use crate::windows_tracker::{extract_icon_for_path, friendly_exe_name};

const CF_UNICODETEXT: u32 = 13;

// DEVPKEY_Bluetooth_Battery: {104EA319-6EE2-4701-BD47-8DDBF425BBE5}, pid 2
const DEVPKEY_BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEntry {
    pub id: String,
    pub tag: String,
    pub text: String,
    pub color: Option<String>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeripheralBattery {
    pub name: String,
    pub kind: String, // "mouse" | "audio" | "other"
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayAppItem {
    pub pid: u32,
    pub name: String,
    pub exe_path: String,
    pub icon_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub connected: bool,
    pub kind: String, // "Ethernet" | "Wi-Fi" | "Нет сети"
    pub speed_label: String,
}

static CLIPBOARD_HISTORY: OnceLock<Mutex<Vec<ClipboardEntry>>> = OnceLock::new();
static LAST_EXTERNAL_FG: OnceLock<Mutex<isize>> = OnceLock::new();

fn clipboard_store() -> &'static Mutex<Vec<ClipboardEntry>> {
    CLIPBOARD_HISTORY.get_or_init(|| Mutex::new(Vec::new()))
}

fn last_fg_store() -> &'static Mutex<isize> {
    LAST_EXTERNAL_FG.get_or_init(|| Mutex::new(0))
}

pub fn read_clipboard_text() -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        if OpenClipboard(HWND::default()).is_err() {
            return None;
        }
        let mut out = None;
        if let Ok(handle) = GetClipboardData(CF_UNICODETEXT) {
            let hglobal = windows::Win32::Foundation::HGLOBAL(handle.0);
            let ptr = GlobalLock(hglobal) as *const u16;
            if !ptr.is_null() {
                let mut len = 0usize;
                while len < 4096 && *ptr.add(len) != 0 {
                    len += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                if !s.trim().is_empty() {
                    out = Some(s.trim().to_string());
                }
                let _ = GlobalUnlock(hglobal);
            }
        }
        let _ = CloseClipboard();
        out
    }
}

pub fn write_clipboard_text(text: &str) -> Result<(), String> {
    unsafe {
        let w = wide(text);
        let bytes = w.len() * 2;
        if OpenClipboard(HWND::default()).is_err() {
            return Err("Не удалось открыть буфер обмена".to_string());
        }
        let _ = EmptyClipboard();
        let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| {
            let _ = CloseClipboard();
            e.to_string()
        })?;
        let dst = GlobalLock(hmem) as *mut u16;
        if !dst.is_null() {
            std::ptr::copy_nonoverlapping(w.as_ptr(), dst, w.len());
            let _ = GlobalUnlock(hmem);
            let _ = SetClipboardData(CF_UNICODETEXT, windows::Win32::Foundation::HANDLE(hmem.0));
        }
        let _ = CloseClipboard();
        Ok(())
    }
}

fn classify_clip(text: &str) -> (String, Option<String>) {
    let t = text.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        return ("URL".to_string(), None);
    }
    if t.starts_with('#') && (t.len() == 4 || t.len() == 7) && t[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return ("HEX".to_string(), Some(t.to_string()));
    }
    if t.contains(":\\") || t.starts_with("\\\\") {
        return ("PATH".to_string(), None);
    }
    ("TXT".to_string(), None)
}

fn push_clipboard_text(text: String) {
    let clean = text.replace('\r', "").replace('\n', " ");
    let trimmed: String = clean.chars().take(180).collect();
    let (tag, color) = classify_clip(&trimmed);

    if let Ok(mut list) = clipboard_store().lock() {
        if let Some(pos) = list.iter().position(|c| c.text == trimmed) {
            let mut item = list.remove(pos);
            item.tag = tag;
            list.insert(0, item);
        } else {
            let id = format!(
                "clip-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            list.insert(
                0,
                ClipboardEntry {
                    id,
                    tag,
                    text: trimmed,
                    color,
                    pinned: false,
                },
            );
        }
        // Keep pinned + recent up to 12 entries
        list.sort_by_key(|c| !c.pinned);
        if list.len() > 12 {
            list.truncate(12);
        }
    }
}

pub fn get_clipboard_history() -> Vec<ClipboardEntry> {
    clipboard_store().lock().map(|v| v.clone()).unwrap_or_default()
}

pub fn toggle_clipboard_pin(id: &str) -> Vec<ClipboardEntry> {
    if let Ok(mut list) = clipboard_store().lock() {
        if let Some(c) = list.iter_mut().find(|c| c.id == id) {
            c.pinned = !c.pinned;
        }
        list.sort_by_key(|c| !c.pinned);
        return list.clone();
    }
    vec![]
}

pub fn clear_unpinned_clipboard() -> Vec<ClipboardEntry> {
    if let Ok(mut list) = clipboard_store().lock() {
        list.retain(|c| c.pinned);
        return list.clone();
    }
    vec![]
}

pub fn start_clipboard_listener<F>(on_change: F)
where
    F: Fn(Vec<ClipboardEntry>) + Send + Sync + 'static,
{
    if let Some(initial) = read_clipboard_text() {
        push_clipboard_text(initial);
    }

    static CLIP_CB: OnceLock<Box<dyn Fn(Vec<ClipboardEntry>) + Send + Sync>> = OnceLock::new();
    let _ = CLIP_CB.set(Box::new(on_change));

    std::thread::spawn(|| unsafe {
        extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            if msg == WM_CLIPBOARDUPDATE {
                if let Some(text) = read_clipboard_text() {
                    push_clipboard_text(text);
                    if let Some(cb) = CLIP_CB.get() {
                        cb(get_clipboard_history());
                    }
                }
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        let cls = wide("TestBarClipboardListener");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            lpszClassName: PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);
        if let Ok(hwnd) = CreateWindowExW(
            Default::default(),
            PCWSTR(cls.as_ptr()),
            PCWSTR::null(),
            Default::default(),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            None,
            None,
        ) {
            let _ = AddClipboardFormatListener(hwnd);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

/// Tracks the last non-TestBar foreground window and returns its live keyboard layout ("RU", "EN", etc.)
pub fn get_keyboard_layout_code() -> String {
    unsafe {
        let fg = GetForegroundWindow();
        let own_pid = GetCurrentProcessId();
        let mut target = fg;
        if !fg.0.is_null() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            if pid != 0 && pid != own_pid {
                if let Ok(mut guard) = last_fg_store().lock() {
                    *guard = fg.0 as isize;
                }
            } else if let Ok(guard) = last_fg_store().lock() {
                if *guard != 0 {
                    target = HWND(*guard as *mut _);
                }
            }
        }
        let tid = GetWindowThreadProcessId(target, None);
        let hkl = GetKeyboardLayout(tid);
        let lang_id = (hkl.0 as usize) & 0xFFFF;
        match lang_id & 0x03FF {
            0x19 => "RU".to_string(),
            0x09 => "EN".to_string(),
            0x22 => "UK".to_string(),
            0x07 => "DE".to_string(),
            0x0C => "FR".to_string(),
            _ => "EN".to_string(),
        }
    }
}

pub fn switch_keyboard_layout() -> String {
    unsafe {
        let target_raw = last_fg_store().lock().map(|g| *g).unwrap_or(0);
        let target = if target_raw != 0 {
            HWND(target_raw as *mut _)
        } else {
            GetForegroundWindow()
        };
        // INPUTLANGCHANGE_FORWARD = 2
        let _ = PostMessageW(target, WM_INPUTLANGCHANGEREQUEST, WPARAM(2), LPARAM(0));
    }
    std::thread::sleep(std::time::Duration::from_millis(30));
    get_keyboard_layout_code()
}

/// Reads real Bluetooth device battery percentages reported by Windows 11 PnP (`DEVPKEY_Bluetooth_Battery`).
pub fn get_peripheral_batteries() -> Vec<PeripheralBattery> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    unsafe {
        let Ok(hdev) = SetupDiGetClassDevsW(None, PCWSTR::null(), HWND::default(), DIGCF_PRESENT | DIGCF_ALLCLASSES) else {
            return results;
        };

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
            .is_ok()
                && req_size >= 1
            {
                let pct = bat_buf[0].min(100);
                let mut name_buf = [0u8; 512];
                let mut name_req = 0u32;
                let name = if SetupDiGetDevicePropertyW(
                    hdev,
                    &info,
                    &DEVPKEY_Device_FriendlyName,
                    &mut prop_type,
                    Some(&mut name_buf),
                    Some(&mut name_req),
                    0,
                )
                .is_ok()
                    && name_req >= 2
                {
                    let u16s = std::slice::from_raw_parts(
                        name_buf.as_ptr() as *const u16,
                        (name_req as usize) / 2,
                    );
                    let end = u16s.iter().position(|&c| c == 0).unwrap_or(u16s.len());
                    String::from_utf16_lossy(&u16s[..end])
                } else {
                    "Bluetooth".to_string()
                };

                let clean_name = name
                    .replace(" Hands-Free AG", "")
                    .replace(" Avrcp Transport", "")
                    .replace(" Stereo", "")
                    .trim()
                    .to_string();

                if !clean_name.is_empty() && seen.insert(clean_name.to_ascii_lowercase()) {
                    let low = clean_name.to_ascii_lowercase();
                    let kind = if low.contains("mouse") || low.contains("мышь") || low.contains("logi") || low.contains("razer") || low.contains("superlight") || low.contains("viper") {
                        "mouse"
                    } else if low.contains("airpods") || low.contains("head") || low.contains("buds") || low.contains("pods") || low.contains("sony") || low.contains("bose") {
                        "audio"
                    } else {
                        "other"
                    };
                    results.push(PeripheralBattery {
                        name: clean_name,
                        kind: kind.to_string(),
                        percent: pct,
                    });
                }
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(hdev);
    }

    results
}

/// Enumerates real user background & tray-resident apps (non-system user processes with real .exe icons)
/// for the compact 6-column 30-40 icon Tray Overflow (`^`) grid.
pub fn list_tray_background_apps() -> Vec<TrayAppItem> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let own_pid = unsafe { GetCurrentProcessId() };
    let mut seen_exe = HashSet::new();
    let mut items = Vec::new();

    for (pid, proc_) in sys.processes() {
        let p_id = pid.as_u32();
        if p_id == own_pid || p_id < 100 {
            continue;
        }
        let Some(exe) = proc_.exe() else { continue };
        let exe_str = exe.to_string_lossy().to_string();
        let low = exe_str.to_ascii_lowercase();

        // Filter out core OS system daemons; keep real user / tray / utility apps
        if low.starts_with(r"c:\windows\system32\")
            || low.starts_with(r"c:\windows\syswow64\")
            || low.starts_with(r"c:\windows\systemapps\")
            || low.contains("webview2")
            || low.contains("crashpad")
            || low.contains("updater")
            || low.contains("helper")
            || low.contains("service")
            || low.contains("broker")
            || low.contains("conhost")
            || low.contains("explorer.exe")
        {
            continue;
        }

        if !seen_exe.insert(low) {
            continue;
        }

        let name = friendly_exe_name(&exe_str);
        let icon_data = extract_icon_for_path(&exe_str);
        items.push(TrayAppItem {
            pid: p_id,
            name,
            exe_path: exe_str,
            icon_data,
        });
        if items.len() >= 42 {
            break;
        }
    }

    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

pub fn get_network_info() -> NetworkInfo {
    unsafe {
        let mut table_ptr = std::ptr::null_mut();
        if GetIfTable2(&mut table_ptr).is_err() || table_ptr.is_null() {
            return NetworkInfo {
                connected: true,
                kind: "Сеть".to_string(),
                speed_label: "Подключено".to_string(),
            };
        }
        let table = &*table_ptr;
        let rows = std::slice::from_raw_parts(table.Table.as_ptr(), table.NumEntries as usize);

        let mut best: Option<&MIB_IF_ROW2> = None;
        for row in rows {
            if row.OperStatus != IfOperStatusUp {
                continue;
            }
            if row.InterfaceAndOperStatusFlags._bitfield & 0x01 == 0 {
                // Skip non-hardware software/virtual loopback interfaces when a hardware NIC is up
                continue;
            }
            // 6 = Ethernet, 71 = Wi-Fi
            if row.Type == 6 || row.Type == 71 {
                if best.map(|b| row.TransmitLinkSpeed > b.TransmitLinkSpeed).unwrap_or(true) {
                    best = Some(row);
                }
            }
        }

        let info = if let Some(row) = best {
            let kind = if row.Type == 71 { "Wi-Fi" } else { "Ethernet" }.to_string();
            let mbps = row.TransmitLinkSpeed / 1_000_000;
            let speed_label = if mbps >= 1000 {
                format!("{} Гбит/с", (mbps as f64 / 1000.0).round() as u64)
            } else if mbps > 0 {
                format!("{mbps} Мбит/с")
            } else {
                "Подключено".to_string()
            };
            NetworkInfo {
                connected: true,
                kind,
                speed_label,
            }
        } else {
            NetworkInfo {
                connected: false,
                kind: "Нет сети".to_string(),
                speed_label: "Офлайн".to_string(),
            }
        };

        FreeMibTable(table_ptr as *const _);
        info
    }
}

pub fn system_power_action(action: &str) -> Result<(), String> {
    match action {
        "lock" => unsafe {
            LockWorkStation().map_err(|e| e.to_string())?;
            Ok(())
        },
        "sleep" => unsafe {
            let ok = SetSuspendState(false, false, false);
            if ok.as_bool() {
                Ok(())
            } else {
                Err("Не удалось перевести ПК в сон".to_string())
            }
        },
        "reboot" => {
            std::process::Command::new("shutdown.exe")
                .args(["/r", "/t", "0"])
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        "shutdown" => {
            std::process::Command::new("shutdown.exe")
                .args(["/s", "/t", "0"])
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        _ => Err(format!("Неизвестное действие питания: {action}")),
    }
}
