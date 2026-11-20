use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_FriendlyName, DEVPROPTYPE, DEVPROPKEY,
};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, RECT, WPARAM};
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
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN, VK_RETURN, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, FindWindowW, GetClassLongPtrW,
    GetClassNameW, GetForegroundWindow, GetMessageW, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, PostMessageW, RegisterClassW, TranslateMessage, GCLP_HICON, GCLP_HICONSM,
    GWL_EXSTYLE, GWL_STYLE, GW_OWNER, HWND_MESSAGE, MSG, WM_CLIPBOARDUPDATE,
    WM_INPUTLANGCHANGEREQUEST, WNDCLASSW, WS_CAPTION, WS_EX_TOOLWINDOW, WS_SYSMENU, WS_THICKFRAME,
};

use crate::icon_cache::IconCache;
use crate::win_shell::wide;
use crate::windows_tracker::{
    focus_or_toggle_window, friendly_exe_name, is_application_frame_host, launch_target_with_args,
    process_exe_path, resolve_exe_or_hwnd_icon_key, resolve_uwp_core_window_hwnd,
    terminate_process_pids, AppWindowGroup,
};

const CF_UNICODETEXT: u32 = 13;
const MAX_CLIPBOARD_ENTRIES: usize = 12;
const MAX_TRAY_APPS: usize = 48;
const MAX_SESSION_ALERTS: usize = 8;

// DEVPKEY_Bluetooth_Battery: {104EA319-6EE2-4701-BD47-8DDBF425BBE5}, pid 2
const DEVPKEY_BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEntry {
    pub id: String,
    pub tag: String,
    pub text: String,
    pub color: Option<String>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeripheralBattery {
    pub name: String,
    pub kind: String, // "mouse" | "audio" | "other"
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrayAppItem {
    pub id: String,
    pub hwnd: Option<isize>,
    pub pid: u32,
    pub all_pids: Vec<u32>,
    pub name: String,
    pub exe_path: String,
    pub icon_key: Option<String>,
    pub icon_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionAlert {
    pub id: String,
    pub level: String, // "info" | "warn" | "error"
    pub title: String,
    pub detail: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub connected: bool,
    pub kind: String, // "Ethernet" | "Wi-Fi" | "Нет сети"
    pub speed_label: String,
}

static CLIPBOARD_HISTORY: OnceLock<Mutex<Vec<ClipboardEntry>>> = OnceLock::new();
static CLIP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static LAST_EXTERNAL_FG: OnceLock<Mutex<isize>> = OnceLock::new();
static SESSION_ALERTS: OnceLock<Mutex<Vec<SessionAlert>>> = OnceLock::new();
static ALERT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn clipboard_store() -> &'static Mutex<Vec<ClipboardEntry>> {
    CLIPBOARD_HISTORY.get_or_init(|| Mutex::new(Vec::new()))
}

fn last_fg_store() -> &'static Mutex<isize> {
    LAST_EXTERNAL_FG.get_or_init(|| Mutex::new(0))
}

fn session_alerts_store() -> &'static Mutex<Vec<SessionAlert>> {
    SESSION_ALERTS.get_or_init(|| Mutex::new(Vec::new()))
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ======================= LOCAL TESTBAR SESSION ALERTS =======================

pub fn push_session_alert_into(
    list: &mut Vec<SessionAlert>,
    level: &str,
    title: impl Into<String>,
    detail: Option<String>,
    timestamp_ms: u64,
) -> SessionAlert {
    let seq = ALERT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let normalized_level = match level {
        "warn" | "error" => level,
        _ => "info",
    };
    let alert = SessionAlert {
        id: format!("alert-{timestamp_ms}-{seq}"),
        level: normalized_level.to_string(),
        title: title.into(),
        detail: detail.filter(|d| !d.trim().is_empty()),
        timestamp_ms,
    };
    list.insert(0, alert.clone());
    if list.len() > MAX_SESSION_ALERTS {
        list.truncate(MAX_SESSION_ALERTS);
    }
    alert
}

pub fn push_session_alert(
    level: &str,
    title: impl Into<String>,
    detail: Option<String>,
) -> SessionAlert {
    let ts = now_epoch_ms();
    if let Ok(mut list) = session_alerts_store().lock() {
        return push_session_alert_into(&mut list, level, title, detail, ts);
    }
    SessionAlert {
        id: format!("alert-{ts}"),
        level: level.to_string(),
        title: title.into(),
        detail,
        timestamp_ms: ts,
    }
}

pub fn get_session_alerts() -> Vec<SessionAlert> {
    session_alerts_store()
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default()
}

pub fn clear_session_alerts() -> Vec<SessionAlert> {
    if let Ok(mut list) = session_alerts_store().lock() {
        list.clear();
    }
    Vec::new()
}

// ======================= CLIPBOARD =======================

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
    if t.starts_with('#')
        && (t.len() == 4 || t.len() == 7)
        && t[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        return ("HEX".to_string(), Some(t.to_string()));
    }
    if t.contains(":\\") || t.starts_with("\\\\") {
        return ("PATH".to_string(), None);
    }
    ("TXT".to_string(), None)
}

pub fn normalize_clipboard_text(text: &str) -> String {
    text.replace('\r', "")
        .replace('\n', " ")
        .trim()
        .chars()
        .take(180)
        .collect()
}

fn next_clip_id() -> String {
    let seq = CLIP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("clip-{}-{seq}", now_epoch_ms())
}

fn trim_unpinned_overflow(list: &mut Vec<ClipboardEntry>) {
    list.sort_by_key(|c| !c.pinned);
    let pinned_count = list.iter().filter(|c| c.pinned).count();
    let keep_len = MAX_CLIPBOARD_ENTRIES.max(pinned_count);
    if list.len() > keep_len {
        list.truncate(keep_len);
    }
}

/// Hydrates persisted pinned clipboard texts (from `config.pinned_clipboard`) into the
/// live vault without duplicating existing entries.
pub fn hydrate_pinned_clipboard_into(list: &mut Vec<ClipboardEntry>, pinned_texts: &[String]) {
    for raw in pinned_texts {
        let trimmed = normalize_clipboard_text(raw);
        if trimmed.is_empty() {
            continue;
        }
        let (tag, color) = classify_clip(&trimmed);
        if let Some(existing) = list.iter_mut().find(|c| c.text == trimmed) {
            existing.pinned = true;
            existing.tag = tag;
            existing.color = color;
        } else {
            list.push(ClipboardEntry {
                id: next_clip_id(),
                tag,
                text: trimmed,
                color,
                pinned: true,
            });
        }
    }
    trim_unpinned_overflow(list);
}

pub fn push_clipboard_text_into(list: &mut Vec<ClipboardEntry>, text: &str) {
    let trimmed = normalize_clipboard_text(text);
    if trimmed.is_empty() {
        return;
    }
    let (tag, color) = classify_clip(&trimmed);

    if let Some(pos) = list.iter().position(|c| c.text == trimmed) {
        let mut item = list.remove(pos);
        item.tag = tag;
        item.color = color;
        list.insert(0, item);
    } else {
        list.insert(
            0,
            ClipboardEntry {
                id: next_clip_id(),
                tag,
                text: trimmed,
                color,
                pinned: false,
            },
        );
    }
    trim_unpinned_overflow(list);
}

pub fn toggle_clipboard_pin_in(list: &mut Vec<ClipboardEntry>, id: &str) {
    if let Some(c) = list.iter_mut().find(|c| c.id == id) {
        c.pinned = !c.pinned;
    }
    trim_unpinned_overflow(list);
}

pub fn clear_unpinned_clipboard_in(list: &mut Vec<ClipboardEntry>) {
    list.retain(|c| c.pinned);
}

fn push_clipboard_text(text: String) {
    if let Ok(mut list) = clipboard_store().lock() {
        push_clipboard_text_into(&mut list, &text);
    }
}

pub fn hydrate_pinned_clipboard(pinned_texts: &[String]) -> Vec<ClipboardEntry> {
    if let Ok(mut list) = clipboard_store().lock() {
        hydrate_pinned_clipboard_into(&mut list, pinned_texts);
        return list.clone();
    }
    vec![]
}

pub fn get_clipboard_history() -> Vec<ClipboardEntry> {
    clipboard_store().lock().map(|v| v.clone()).unwrap_or_default()
}

pub fn toggle_clipboard_pin(id: &str) -> Vec<ClipboardEntry> {
    if let Ok(mut list) = clipboard_store().lock() {
        toggle_clipboard_pin_in(&mut list, id);
        return list.clone();
    }
    vec![]
}

pub fn clear_unpinned_clipboard() -> Vec<ClipboardEntry> {
    if let Ok(mut list) = clipboard_store().lock() {
        clear_unpinned_clipboard_in(&mut list);
        return list.clone();
    }
    vec![]
}

pub fn start_clipboard_listener<F>(initial_pinned: &[String], on_change: F)
where
    F: Fn(Vec<ClipboardEntry>) + Send + Sync + 'static,
{
    hydrate_pinned_clipboard(initial_pinned);
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

// ======================= WIN-CHORD SENDINPUT HELPERS =======================

/// Builds an atomic 4-event `[VK_LWIN down, vk down, vk up, VK_LWIN up]` `SendInput` batch.
/// Because all injected events carry `LLKHF_INJECTED`, TestBar's `WH_KEYBOARD_LL` hook
/// ignores them cleanly and never falsely toggles TestBar Start.
pub fn build_win_chord_inputs(vk: u16) -> [INPUT; 4] {
    [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    dwFlags: KEYEVENTF_EXTENDEDKEY,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    dwFlags: KEYEVENTF_KEYUP,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    dwFlags: KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                    ..Default::default()
                },
            },
        },
    ]
}

fn send_win_chord(vk: u16) -> Result<(), String> {
    let inputs = build_win_chord_inputs(vk);
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err("Не удалось отправить системную комбинацию клавиш".to_string())
    }
}

fn send_single_vk(vk: VIRTUAL_KEY) {
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: KEYEVENTF_KEYUP,
                    ..Default::default()
                },
            },
        },
    ];
    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn is_native_shell_tray_visible() -> bool {
    unsafe {
        let cls = wide("Shell_TrayWnd");
        let Ok(hwnd) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) else {
            return false;
        };
        !hwnd.0.is_null() && IsWindowVisible(hwnd).as_bool()
    }
}

/// Triggers native Windows 11 Notification Center via `Win+N` (`AD-7`, zero MSIX `UserNotificationListener`).
pub fn open_windows_notifications() -> Result<(), String> {
    send_win_chord(b'N' as u16)
}

/// Triggers native Windows system tray focus (`Win+B`) safely without breaking `hide_native`.
/// Only dispatches a follow-up `Enter` when `Shell_TrayWnd` is currently visible so we never
/// inject a stray `Enter` key into the user's active application.
pub fn trigger_native_tray_overflow() -> Result<(), String> {
    let native_visible = is_native_shell_tray_visible();
    send_win_chord(b'B' as u16)?;
    if native_visible {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(60));
            send_single_vk(VK_RETURN);
        });
    }
    Ok(())
}

// ======================= KEYBOARD LAYOUT =======================

/// Pure primary-language-ID mapper turning a Win32 `HKL` raw value into a 2-letter UI code.
pub fn lang_id_to_layout_code(hkl_raw: usize) -> &'static str {
    let primary_lang = hkl_raw & 0x03FF;
    match primary_lang {
        0x19 => "RU",
        0x09 => "EN",
        0x22 => "UK",
        0x07 => "DE",
        0x0C => "FR",
        0x0A => "ES",
        0x10 => "IT",
        0x15 => "PL",
        0x1F => "TR",
        0x11 => "JA",
        0x04 => "ZH",
        0x12 => "KO",
        _ => "EN",
    }
}

unsafe fn resolve_layout_target_hwnd() -> HWND {
    let fg = GetForegroundWindow();
    let own_pid = GetCurrentProcessId();
    let mut target = fg;
    let mut target_pid = 0u32;

    if !fg.0.is_null() {
        GetWindowThreadProcessId(fg, Some(&mut target_pid));
        if target_pid != 0 && target_pid != own_pid {
            if let Ok(mut guard) = last_fg_store().lock() {
                *guard = fg.0 as isize;
            }
        } else if let Ok(guard) = last_fg_store().lock() {
            if *guard != 0 {
                let candidate = HWND(*guard as *mut _);
                if IsWindow(candidate).as_bool() {
                    target = candidate;
                    GetWindowThreadProcessId(target, Some(&mut target_pid));
                }
            }
        }
    } else if let Ok(guard) = last_fg_store().lock() {
        if *guard != 0 {
            let candidate = HWND(*guard as *mut _);
            if IsWindow(candidate).as_bool() {
                target = candidate;
                GetWindowThreadProcessId(target, Some(&mut target_pid));
            }
        }
    }

    // For UWP apps hosted inside ApplicationFrameHost.exe, resolve the hosted CoreWindow
    // so keyboard layout reads and switches target the real UWP input thread.
    if !target.0.is_null() && target_pid != 0 {
        if let Some(exe) = process_exe_path(target_pid) {
            if is_application_frame_host(&exe) {
                if let Some((core_hwnd, _)) = resolve_uwp_core_window_hwnd(target, target_pid) {
                    return core_hwnd;
                }
            }
        }
    }

    target
}

/// Tracks the last non-TestBar foreground window (including UWP `CoreWindow`) and returns
/// its live keyboard layout (`"RU"`, `"EN"`, etc.).
pub fn get_keyboard_layout_code() -> String {
    unsafe {
        let target = resolve_layout_target_hwnd();
        let tid = if target.0.is_null() {
            0
        } else {
            GetWindowThreadProcessId(target, None)
        };
        let hkl = GetKeyboardLayout(tid);
        lang_id_to_layout_code(hkl.0 as usize).to_string()
    }
}

pub fn switch_keyboard_layout() -> String {
    let before = get_keyboard_layout_code();
    unsafe {
        let target = resolve_layout_target_hwnd();
        if !target.0.is_null() && IsWindow(target).as_bool() {
            // INPUTLANGCHANGE_FORWARD = 2
            let _ = PostMessageW(target, WM_INPUTLANGCHANGEREQUEST, WPARAM(2), LPARAM(0));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(35));
    let after = get_keyboard_layout_code();
    if after != before {
        return after;
    }

    // Some modern UWP / WinUI3 surfaces ignore WM_INPUTLANGCHANGEREQUEST; fall back to
    // Win+Space only if at least two keyboard layouts are actually installed.
    let layout_count = unsafe { GetKeyboardLayoutList(None) };
    if layout_count >= 2 && send_win_chord(VK_SPACE.0).is_ok() {
        std::thread::sleep(std::time::Duration::from_millis(55));
        return get_keyboard_layout_code();
    }
    after
}

// ======================= BLUETOOTH PERIPHERALS =======================

/// Reads real Bluetooth device battery percentages reported by Windows 11 PnP (`DEVPKEY_Bluetooth_Battery`).
pub fn get_peripheral_batteries() -> Vec<PeripheralBattery> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    unsafe {
        let Ok(hdev) = SetupDiGetClassDevsW(
            None,
            PCWSTR::null(),
            HWND::default(),
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        ) else {
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
                    let kind = if low.contains("mouse")
                        || low.contains("мышь")
                        || low.contains("logi")
                        || low.contains("razer")
                        || low.contains("superlight")
                        || low.contains("viper")
                    {
                        "mouse"
                    } else if low.contains("airpods")
                        || low.contains("head")
                        || low.contains("buds")
                        || low.contains("pods")
                        || low.contains("sony")
                        || low.contains("bose")
                    {
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

// ======================= TWO-TIER TRAY BACKGROUND APPS =======================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTrayWindowCandidate {
    pub hwnd: isize,
    pub pid: u32,
    pub exe_path: String,
    pub title: String,
    pub class_name: String,
    pub is_visible: bool,
    pub is_tool_window: bool,
    pub has_owner: bool,
    pub has_class_icon: bool,
    pub has_app_frame_style: bool,
}

pub fn canonical_tray_exe_key(exe_path: &str) -> String {
    exe_path.trim().replace('/', "\\").to_ascii_lowercase()
}

fn exe_basename_lower(exe_path: &str) -> String {
    let norm = canonical_tray_exe_key(exe_path);
    norm.rsplit('\\').next().unwrap_or(&norm).to_string()
}

/// Filters out synthetic Win32 internal helper window classes (IME, OLE, tooltips, DWM)
/// so headless console/worker processes that only own an IME window are not treated as tray apps.
pub fn is_synthetic_tray_window_class(class_name: &str) -> bool {
    matches!(
        class_name.trim(),
        "IME"
            | "MSCTFIME UI"
            | "Default IME"
            | "CicMarshalWndClass"
            | "GDI+ Hook Window Class"
            | "tooltips_class32"
            | "OleMainThreadWndClass"
            | "WorkerW"
            | "Progman"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "#32768"
            | "EdgeUiInputWndClass"
            | "DummyDWMListenerWindow"
            | "TaskListThumbnailWnd"
    )
}

/// Filters out Windows OS daemons, shell hosts, TestBar itself, and headless subprocess helpers
/// while keeping real user background & tray-resident applications.
pub fn is_excluded_tray_process(exe_path: &str) -> bool {
    let norm = canonical_tray_exe_key(exe_path);
    if norm.is_empty() {
        return true;
    }
    let base = exe_basename_lower(&norm);
    if base.is_empty() {
        return true;
    }

    // Explicitly allow real Windows Security tray icon even though it lives under System32
    if base == "securityhealthsystray.exe" {
        return false;
    }

    if matches!(
        base.as_str(),
        "testbar.exe"
            | "explorer.exe"
            | "shellexperiencehost.exe"
            | "startmenuexperiencehost.exe"
            | "textinputhost.exe"
            | "searchhost.exe"
            | "searchapp.exe"
            | "searchindexer.exe"
            | "runtimebroker.exe"
            | "svchost.exe"
            | "sihost.exe"
            | "taskhostw.exe"
            | "ctfmon.exe"
            | "dwm.exe"
            | "dllhost.exe"
            | "conhost.exe"
            | "applicationframehost.exe"
            | "lockapp.exe"
            | "smartscreen.exe"
            | "msedgewebview2.exe"
            | "backgroundtaskhost.exe"
            | "compkgsrv.exe"
            | "wmiprvse.exe"
            | "audiodg.exe"
            | "fontdrvhost.exe"
            | "spoolsv.exe"
            | "dashost.exe"
            | "wudfhost.exe"
            | "mousocoreworker.exe"
            | "usoclient.exe"
            | "widgets.exe"
            | "widgetservice.exe"
            | "phoneexperiencehost.exe"
            | "crossdeviceresume.exe"
            | "gamebar.exe"
            | "gamebarftserver.exe"
            | "nvcontainer.exe"
            | "update.exe"
    ) {
        return true;
    }

    if norm.starts_with(r"c:\windows\system32\")
        || norm.starts_with(r"c:\windows\syswow64\")
        || norm.starts_with(r"c:\windows\systemapps\")
        || norm.starts_with(r"c:\windows\immersivecontrolpanel\")
        || norm.starts_with(r"c:\windows\winsxs\")
        || norm.starts_with(r"c:\windows\servicing\")
    {
        return true;
    }

    const EXCLUDED_BASENAME_TOKENS: [&str; 9] = [
        "webview2",
        "crashpad",
        "crashhandler",
        "crashreporter",
        "webhelper",
        "updater",
        "helper",
        "broker",
        "elevation_service",
    ];
    EXCLUDED_BASENAME_TOKENS.iter().any(|tok| base.contains(tok))
}

/// Scores candidate top-level windows for a background app so we pick its real activatable
/// main/owner window (with title/icon) instead of a tiny hidden helper HWND.
pub fn score_tray_candidate(c: &RawTrayWindowCandidate) -> i32 {
    let mut score = 0;
    if !c.has_owner {
        score += 40;
    }
    if !c.is_tool_window {
        score += 30;
    }
    if !c.title.trim().is_empty() {
        score += 25;
    }
    if c.has_class_icon {
        score += 15;
    }
    if c.has_app_frame_style {
        score += 10;
    }
    if !c.is_visible {
        score += 5;
    }
    score
}

/// Pure grouping & deduplication routine turning enumerated top-level windows into compact
/// `TrayAppItem` entries:
/// - Excludes own process, synthetic IME/OLE classes, OS/helper processes, and any process
///   already represented by a visible taskbar `AppWindowGroup`.
/// - Deduplicates by canonical lowercase `exe_path`, collecting `all_pids` and keeping the
///   highest-scoring hidden/tray `hwnd`.
/// - Resolves `icon_key` via `resolve_icon_key` (`IconCache` pipeline).
pub fn build_tray_background_apps<FResolveIcon>(
    candidates: Vec<RawTrayWindowCandidate>,
    open_taskbar_groups: &[AppWindowGroup],
    own_pid: u32,
    mut resolve_icon_key: FResolveIcon,
) -> Vec<TrayAppItem>
where
    FResolveIcon: FnMut(&str, Option<isize>) -> Option<String>,
{
    let mut open_exe_paths = HashSet::new();
    let mut open_exe_bases = HashSet::new();
    let mut open_pids = HashSet::new();

    for g in open_taskbar_groups {
        let g_norm = canonical_tray_exe_key(&g.exe_path);
        if !g_norm.is_empty() {
            open_exe_paths.insert(g_norm.clone());
            let base = exe_basename_lower(&g_norm);
            if !base.is_empty() {
                open_exe_bases.insert(base);
            }
        }
        for w in &g.windows {
            if w.pid != 0 {
                open_pids.insert(w.pid);
            }
            let w_norm = canonical_tray_exe_key(&w.exe_path);
            if !w_norm.is_empty() {
                open_exe_paths.insert(w_norm.clone());
                let base = exe_basename_lower(&w_norm);
                if !base.is_empty() {
                    open_exe_bases.insert(base);
                }
            }
        }
    }

    struct GroupAcc {
        id: String,
        exe_path: String,
        best_hwnd: isize,
        best_pid: u32,
        best_score: i32,
        pids: HashSet<u32>,
    }

    let mut groups: HashMap<String, GroupAcc> = HashMap::new();

    for c in candidates {
        if c.pid == 0 || c.pid == own_pid || c.hwnd == 0 {
            continue;
        }
        if is_synthetic_tray_window_class(&c.class_name) {
            continue;
        }
        if is_excluded_tray_process(&c.exe_path) {
            continue;
        }

        let norm_exe = canonical_tray_exe_key(&c.exe_path);
        let base_exe = exe_basename_lower(&norm_exe);
        if open_pids.contains(&c.pid)
            || open_exe_paths.contains(&norm_exe)
            || open_exe_bases.contains(&base_exe)
        {
            continue;
        }

        // Skip standard visible unowned desktop windows that belong on the taskbar rather than tray.
        if c.is_visible && !c.is_tool_window && !c.has_owner && !c.title.trim().is_empty() {
            continue;
        }

        let score = score_tray_candidate(&c);
        match groups.get_mut(&norm_exe) {
            Some(acc) => {
                acc.pids.insert(c.pid);
                if score > acc.best_score {
                    acc.best_score = score;
                    acc.best_hwnd = c.hwnd;
                    acc.best_pid = c.pid;
                }
            }
            None => {
                let mut pids = HashSet::new();
                pids.insert(c.pid);
                groups.insert(
                    norm_exe.clone(),
                    GroupAcc {
                        id: norm_exe,
                        exe_path: c.exe_path,
                        best_hwnd: c.hwnd,
                        best_pid: c.pid,
                        best_score: score,
                        pids,
                    },
                );
            }
        }
    }

    let mut items: Vec<TrayAppItem> = groups
        .into_values()
        .map(|acc| {
            let mut all_pids: Vec<u32> = acc.pids.into_iter().collect();
            all_pids.sort_unstable();
            let hwnd = (acc.best_hwnd != 0).then_some(acc.best_hwnd);
            let name = friendly_exe_name(&acc.exe_path);
            let icon_key = resolve_icon_key(&acc.exe_path, hwnd);
            TrayAppItem {
                id: acc.id,
                hwnd,
                pid: acc.best_pid,
                all_pids,
                name,
                exe_path: acc.exe_path,
                icon_key,
                icon_data: None,
            }
        })
        .collect();

    items.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    if items.len() > MAX_TRAY_APPS {
        items.truncate(MAX_TRAY_APPS);
    }
    items
}

/// Enumerates real Windows background/tray-capable user apps owning hidden or tool top-level
/// windows (excluding open taskbar apps and OS daemons), resolving crisp icons via `IconCache`.
pub fn list_tray_background_apps_cached(
    cache: &Mutex<IconCache>,
    open_taskbar_groups: &[AppWindowGroup],
) -> Vec<TrayAppItem> {
    struct EnumCtx {
        own_pid: u32,
        exe_memo: HashMap<u32, Option<String>>,
        candidates: Vec<RawTrayWindowCandidate>,
    }

    extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let ctx = &mut *(lparam.0 as *mut EnumCtx);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == ctx.own_pid {
                return BOOL(1);
            }

            let mut cls_buf = [0u16; 96];
            let cls_len = GetClassNameW(hwnd, &mut cls_buf);
            let class_name = if cls_len > 0 {
                String::from_utf16_lossy(&cls_buf[..cls_len as usize])
            } else {
                String::new()
            };
            if is_synthetic_tray_window_class(&class_name) {
                return BOOL(1);
            }

            let exe_opt = ctx
                .exe_memo
                .entry(pid)
                .or_insert_with(|| process_exe_path(pid))
                .clone();
            let Some(exe_path) = exe_opt else {
                return BOOL(1);
            };
            if is_excluded_tray_process(&exe_path) {
                return BOOL(1);
            }

            let title_len = GetWindowTextLengthW(hwnd);
            let title = if title_len > 0 && title_len < 512 {
                let mut buf = vec![0u16; (title_len + 1) as usize];
                let got = GetWindowTextW(hwnd, &mut buf);
                if got > 0 {
                    String::from_utf16_lossy(&buf[..got as usize])
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let is_visible = IsWindowVisible(hwnd).as_bool();
            let is_tool_window = (ex_style & WS_EX_TOOLWINDOW.0) != 0;
            let has_owner = !GetWindow(hwnd, GW_OWNER).unwrap_or_default().0.is_null();
            let has_class_icon =
                GetClassLongPtrW(hwnd, GCLP_HICON) != 0 || GetClassLongPtrW(hwnd, GCLP_HICONSM) != 0;
            let has_app_frame_style = (style & (WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0)) != 0;

            ctx.candidates.push(RawTrayWindowCandidate {
                hwnd: hwnd.0 as isize,
                pid,
                exe_path,
                title,
                class_name,
                is_visible,
                is_tool_window,
                has_owner,
                has_class_icon,
                has_app_frame_style,
            });
        }
        BOOL(1)
    }

    let own_pid = unsafe { GetCurrentProcessId() };
    let mut ctx = EnumCtx {
        own_pid,
        exe_memo: HashMap::new(),
        candidates: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }

    build_tray_background_apps(ctx.candidates, open_taskbar_groups, own_pid, |exe, hwnd| {
        resolve_exe_or_hwnd_icon_key(exe, hwnd, cache)
    })
}

/// Pure decision helper determining whether a candidate hidden HWND represents a real restorable
/// UI window (vs a 0x0 invisible tool/message helper where single-instance relaunch is better).
pub fn should_activate_hidden_hwnd(
    is_valid_window: bool,
    exe_matches: bool,
    is_iconic: bool,
    is_tool_window: bool,
    has_app_frame_style: bool,
    rect_width: i32,
    rect_height: i32,
) -> bool {
    if !is_valid_window || !exe_matches {
        return false;
    }
    is_iconic || !is_tool_window || has_app_frame_style || (rect_width > 40 && rect_height > 40)
}

/// Activates a background tray app by restoring & focusing its existing hidden window when valid
/// (avoiding duplicate app spawns), falling back to `launch_target_with_args` only when no
/// restorable hidden window exists.
pub fn activate_tray_app(hwnd_opt: Option<isize>, exe_path: &str) -> Result<(), String> {
    if let Some(raw) = hwnd_opt.filter(|&h| h != 0) {
        unsafe {
            let hwnd = HWND(raw as *mut _);
            let is_valid = IsWindow(hwnd).as_bool();
            if is_valid {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                let exe_matches = if exe_path.trim().is_empty() {
                    true
                } else {
                    process_exe_path(pid)
                        .map(|actual| {
                            canonical_tray_exe_key(&actual) == canonical_tray_exe_key(exe_path)
                        })
                        .unwrap_or(false)
                };
                let is_iconic = IsIconic(hwnd).as_bool();
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                let is_tool = (ex_style & WS_EX_TOOLWINDOW.0) != 0;
                let has_frame = (style & (WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0)) != 0;
                let mut rc = RECT::default();
                let (w, h) = if GetWindowRect(hwnd, &mut rc).is_ok() {
                    (rc.right - rc.left, rc.bottom - rc.top)
                } else {
                    (0, 0)
                };

                if should_activate_hidden_hwnd(is_valid, exe_matches, is_iconic, is_tool, has_frame, w, h) {
                    focus_or_toggle_window(raw, false);
                    return Ok(());
                }
            }
        }
    }

    launch_target_with_args(exe_path, None, false)
}

pub fn end_tray_app_task(pids: &[u32]) -> Result<(), String> {
    terminate_process_pids(pids)
}

// ======================= NETWORK & POWER =======================

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
                if best
                    .map(|b| row.TransmitLinkSpeed > b.TransmitLinkSpeed)
                    .unwrap_or(true)
                {
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
        "reboot" => std::process::Command::new("shutdown.exe")
            .args(["/r", "/t", "0"])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "shutdown" => std::process::Command::new("shutdown.exe")
            .args(["/s", "/t", "0"])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string()),
        _ => Err(format!("Неизвестное действие питания: {action}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_tracker::OpenWindowInfo;

    fn make_open_group(exe_path: &str, pid: u32) -> AppWindowGroup {
        AppWindowGroup {
            key: exe_path.to_ascii_lowercase(),
            name: friendly_exe_name(exe_path),
            exe_path: exe_path.to_string(),
            aumid: None,
            icon_key: Some("exe:open".to_string()),
            icon_data: None,
            is_active: false,
            windows: vec![OpenWindowInfo {
                hwnd: 999,
                pid,
                title: "Visible App".to_string(),
                exe_path: exe_path.to_string(),
                exe_name: friendly_exe_name(exe_path),
                aumid: None,
                is_active: false,
                is_minimized: false,
            }],
        }
    }

    fn make_cand(
        hwnd: isize,
        pid: u32,
        exe_path: &str,
        title: &str,
        class_name: &str,
    ) -> RawTrayWindowCandidate {
        RawTrayWindowCandidate {
            hwnd,
            pid,
            exe_path: exe_path.to_string(),
            title: title.to_string(),
            class_name: class_name.to_string(),
            is_visible: false,
            is_tool_window: false,
            has_owner: false,
            has_class_icon: true,
            has_app_frame_style: true,
        }
    }

    #[test]
    fn tray_background_apps_filters_os_helpers_open_taskbar_and_deduplicates_pids() {
        let own_pid = 777;
        let open_groups = vec![make_open_group(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            4000,
        )];

        let candidates = vec![
            // Own TestBar process -> excluded
            make_cand(1, own_pid, r"C:\Apps\TestBar\testbar.exe", "TestBar", "WRY"),
            // OS & shell daemons -> excluded
            make_cand(2, 100, r"C:\Windows\explorer.exe", "Shell", "Shell_TrayWnd"),
            make_cand(3, 101, r"C:\Windows\SystemApps\ShellExperienceHost\ShellExperienceHost.exe", "", "ApplicationFrame"),
            make_cand(4, 102, r"C:\Windows\System32\svchost.exe", "", "SvcWnd"),
            make_cand(5, 103, r"C:\Windows\SystemApps\MicrosoftWindows.Client.CBS\TextInputHost.exe", "", "Core"),
            make_cand(6, 104, r"C:\Windows\SystemApps\SearchHost\SearchHost.exe", "", "Core"),
            // Subprocess helpers -> excluded
            make_cand(7, 200, r"C:\Program Files (x86)\Steam\bin\cef\steamwebhelper.exe", "", "Chrome"),
            make_cand(8, 201, r"C:\Apps\Discord\crashpad_handler.exe", "", "Crash"),
            // Synthetic IME-only window -> excluded
            make_cand(9, 202, r"C:\Apps\Node\node.exe", "", "Default IME"),
            // App already visible on taskbar -> excluded even if another PID has a hidden window
            make_cand(10, 4001, r"C:\Program Files\Google\Chrome\Application\Chrome.EXE", "", "Chrome_WidgetWin_0"),
            // Real background tray apps: Telegram (2 hidden windows across 2 PIDs) + Steam + Windows Security Tray
            RawTrayWindowCandidate {
                hwnd: 501,
                pid: 5001,
                exe_path: r"C:\Apps\Telegram\Telegram.exe".to_string(),
                title: "".to_string(),
                class_name: "QtTrayIcon".to_string(),
                is_visible: false,
                is_tool_window: true,
                has_owner: true,
                has_class_icon: false,
                has_app_frame_style: false,
            },
            RawTrayWindowCandidate {
                hwnd: 502,
                pid: 5002,
                exe_path: r"c:/apps/telegram/telegram.exe".to_string(),
                title: "Telegram".to_string(),
                class_name: "Qt5QWindowIcon".to_string(),
                is_visible: false,
                is_tool_window: false,
                has_owner: false,
                has_class_icon: true,
                has_app_frame_style: true,
            },
            make_cand(601, 6001, r"C:\Program Files (x86)\Steam\steam.exe", "Steam", "SDL_app"),
            make_cand(701, 7001, r"C:\Windows\System32\SecurityHealthSystray.exe", "Windows Security", "SecurityHealth"),
        ];

        let items = build_tray_background_apps(candidates, &open_groups, own_pid, |exe, hwnd| {
            Some(format!("exe:{}:{}", exe_basename_lower(exe), hwnd.unwrap_or(0)))
        });

        assert_eq!(items.len(), 3);
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["SecurityHealthSystray", "Steam", "Telegram"]);

        let tg = items.iter().find(|i| i.name == "Telegram").unwrap();
        // Best HWND is the unowned titled main window 502, NOT the owned tool window 501
        assert_eq!(tg.hwnd, Some(502));
        assert_eq!(tg.pid, 5002);
        assert_eq!(tg.all_pids, vec![5001, 5002]);
        assert_eq!(tg.icon_key.as_deref(), Some("exe:telegram.exe:502"));
        assert!(tg.icon_data.is_none(), "tray payload must carry icon_key only (AD-6)");
    }

    #[test]
    fn tray_activation_decision_prefers_real_hidden_window_and_falls_back_for_zero_size_tool() {
        // Real hidden main window -> activate HWND directly
        assert!(should_activate_hidden_hwnd(true, true, false, false, true, 1280, 720));
        // Minimized window -> activate HWND directly
        assert!(should_activate_hidden_hwnd(true, true, true, true, false, 0, 0));
        // 0x0 invisible tool helper window without frame -> fall back to launch_target
        assert!(!should_activate_hidden_hwnd(true, true, false, true, false, 0, 0));
        // Recycled HWND whose exe no longer matches -> never focus wrong app
        assert!(!should_activate_hidden_hwnd(true, false, false, false, true, 800, 600));
        // Destroyed HWND -> fall back
        assert!(!should_activate_hidden_hwnd(false, true, false, false, true, 800, 600));
    }

    #[test]
    fn win_chord_inputs_and_layout_code_mapping() {
        let win_n = build_win_chord_inputs(b'N' as u16);
        assert_eq!(win_n.len(), 4);
        unsafe {
            assert_eq!(win_n[0].Anonymous.ki.wVk, VK_LWIN);
            assert_eq!(win_n[0].Anonymous.ki.dwFlags, KEYEVENTF_EXTENDEDKEY);
            assert_eq!(win_n[1].Anonymous.ki.wVk, VIRTUAL_KEY(b'N' as u16));
            assert_eq!(win_n[2].Anonymous.ki.wVk, VIRTUAL_KEY(b'N' as u16));
            assert_eq!(win_n[2].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
            assert_eq!(win_n[3].Anonymous.ki.wVk, VK_LWIN);
            assert_eq!(
                win_n[3].Anonymous.ki.dwFlags,
                KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
            );
        }

        assert_eq!(lang_id_to_layout_code(0x0419_0419), "RU");
        assert_eq!(lang_id_to_layout_code(0x0409_0409), "EN");
        assert_eq!(lang_id_to_layout_code(0x0422_0422), "UK");
        assert_eq!(lang_id_to_layout_code(0x0407_0407), "DE");
        assert_eq!(lang_id_to_layout_code(0x0000_0000), "EN");
    }

    #[test]
    fn session_alerts_ring_buffer_caps_newest_first() {
        let mut buf = Vec::new();
        for i in 0..12u64 {
            push_session_alert_into(&mut buf, "info", format!("Event {i}"), None, 1000 + i);
        }
        assert_eq!(buf.len(), MAX_SESSION_ALERTS);
        assert_eq!(buf[0].title, "Event 11");
        assert_eq!(buf[MAX_SESSION_ALERTS - 1].title, "Event 4");
    }

    #[test]
    fn clipboard_hydrates_pinned_entries_deduplicates_and_retains_pins_across_overflow() {
        let mut vault = Vec::new();
        hydrate_pinned_clipboard_into(
            &mut vault,
            &[
                "https://github.com/testbar".to_string(),
                "#38BDF8".to_string(),
                "   ".to_string(),
                "#38BDF8".to_string(), // duplicate must be ignored
                r"C:\Projects\testbar".to_string(),
            ],
        );

        assert_eq!(vault.len(), 3);
        assert!(vault.iter().all(|c| c.pinned));
        assert_eq!(vault[0].tag, "URL");
        assert_eq!(vault[1].tag, "HEX");
        assert_eq!(vault[1].color.as_deref(), Some("#38BDF8"));
        assert_eq!(vault[2].tag, "PATH");
        // Unique IDs even when hydrated within the same millisecond
        let unique_ids: HashSet<&str> = vault.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(unique_ids.len(), 3);

        // Copying an already-pinned snippet deduplicates and preserves `pinned = true`
        push_clipboard_text_into(&mut vault, "#38BDF8\r\n");
        assert_eq!(vault.len(), 3);
        assert!(vault.iter().find(|c| c.text == "#38BDF8").unwrap().pinned);

        // Copying 15 new unpinned entries must NEVER evict any pinned entries
        for i in 0..15 {
            push_clipboard_text_into(&mut vault, &format!("unpinned snippet {i}"));
        }
        assert_eq!(vault.len(), MAX_CLIPBOARD_ENTRIES);
        assert_eq!(vault.iter().filter(|c| c.pinned).count(), 3);
        assert!(vault.iter().any(|c| c.text == "https://github.com/testbar" && c.pinned));
        assert!(vault.iter().any(|c| c.text == "#38BDF8" && c.pinned));
        assert!(vault.iter().any(|c| c.text == r"C:\Projects\testbar" && c.pinned));
        // Most recent unpinned sits directly after pinned entries
        assert_eq!(vault[3].text, "unpinned snippet 14");
        assert!(!vault[3].pinned);

        // Unpin one entry, then clear unpinned -> only remaining 2 pins survive
        let url_id = vault
            .iter()
            .find(|c| c.text == "https://github.com/testbar")
            .unwrap()
            .id
            .clone();
        toggle_clipboard_pin_in(&mut vault, &url_id);
        assert!(!vault.iter().find(|c| c.id == url_id).unwrap().pinned);

        clear_unpinned_clipboard_in(&mut vault);
        assert_eq!(vault.len(), 2);
        assert!(vault.iter().all(|c| c.pinned));
        assert!(!vault.iter().any(|c| c.text == "https://github.com/testbar"));
    }
}
