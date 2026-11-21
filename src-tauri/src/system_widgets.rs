use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_FriendlyName, DEVPROPTYPE, DEVPROPKEY,
};
use windows::Win32::Foundation::{
    CloseHandle, GlobalFree, BOOL, ERROR_SUCCESS, E_UNEXPECTED, FILETIME, HANDLE, HGLOBAL, HWND,
    LPARAM, LRESULT, RECT, S_FALSE, S_OK, WPARAM,
};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, REG_BINARY, REG_DWORD, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN,
    VK_RETURN, VK_SPACE,
};
use windows::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHGetKnownFolderPath, SHQueryRecycleBinW, ShellExecuteW,
    KNOWN_FOLDER_FLAG, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHQUERYRBINFO,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, FindWindowW,
    GetClassLongPtrW, GetClassNameW, GetForegroundWindow, GetMessageW, GetWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, PostMessageW, RegisterClassW,
    SendMessageTimeoutW, ShowWindow, TranslateMessage, GCLP_HICON, GCLP_HICONSM, GWL_EXSTYLE,
    GWL_STYLE, GW_OWNER, HICON, HWND_MESSAGE, ICON_BIG, ICON_SMALL, ICON_SMALL2, MSG,
    SMTO_ABORTIFHUNG, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, WM_CLIPBOARDUPDATE, WM_GETICON,
    WM_INPUTLANGCHANGEREQUEST, WNDCLASSW, WS_CAPTION, WS_EX_TOOLWINDOW, WS_SYSMENU, WS_THICKFRAME,
};

use crate::icon_cache::{self, IconCache};
use crate::win_shell::wide;
use crate::windows_tracker::{
    extract_path_icon_raw, focus_or_toggle_window, friendly_exe_name, hicon_to_data_url,
    is_application_frame_host, launch_target_with_args, process_exe_path,
    resolve_uwp_core_window_hwnd, terminate_process_pids, AppWindowGroup,
};

const CF_UNICODETEXT: u32 = 13;
pub const MAX_CLIPBOARD_ENTRIES: usize = 40;
pub const MAX_CLIPBOARD_CHARS: usize = 65_536;
pub const CLIPBOARD_PREVIEW_MAX_CHARS: usize = 180;
const CLIPBOARD_OPEN_RETRY_ATTEMPTS: usize = 5;
const CLIPBOARD_OPEN_RETRY_DELAY_MS: u64 = 10;
const MAX_TRAY_APPS: usize = 48;
const MAX_SESSION_ALERTS: usize = 8;
const MAX_PLAUSIBLE_LINK_SPEED_BPS: u64 = 1_000_000_000_000; // 1 Tbps upper sanity bound

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
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub line_count: usize,
    #[serde(default)]
    pub char_count: usize,
    #[serde(default)]
    pub kind: String,
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

/// Alias matching Wave 1B naming while preserving full compatibility with `TrayAppItem`.
pub type TrayAppInfo = TrayAppItem;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionAlert {
    pub id: String,
    pub level: String, // "info" | "warn" | "error"
    pub title: String,
    pub detail: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub connected: bool,
    pub kind: String, // "Ethernet" | "Wi-Fi" | "VPN" | "Сеть" | "Нет сети"
    pub speed_label: String,
}

static CLIPBOARD_HISTORY: OnceLock<Mutex<Vec<ClipboardEntry>>> = OnceLock::new();
static CLIP_SEQ: AtomicU64 = AtomicU64::new(1);
static CLIPBOARD_PAUSED: AtomicBool = AtomicBool::new(false);
static CF_EXCLUDE_MONITOR_FMT: OnceLock<u32> = OnceLock::new();
static CF_CAN_INCLUDE_HISTORY_FMT: OnceLock<u32> = OnceLock::new();
static LAST_EXTERNAL_FG: OnceLock<Mutex<isize>> = OnceLock::new();
static SESSION_ALERTS: OnceLock<Mutex<Vec<SessionAlert>>> = OnceLock::new();
static ALERT_SEQ: AtomicU64 = AtomicU64::new(1);
static NATIVE_TRAY_WATCHER_GEN: AtomicU64 = AtomicU64::new(0);

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

fn exclude_monitor_format_id() -> u32 {
    *CF_EXCLUDE_MONITOR_FMT.get_or_init(|| unsafe {
        let w = wide("ExcludeClipboardContentFromMonitorProcessing");
        RegisterClipboardFormatW(PCWSTR(w.as_ptr()))
    })
}

fn can_include_history_format_id() -> u32 {
    *CF_CAN_INCLUDE_HISTORY_FMT.get_or_init(|| unsafe {
        let w = wide("CanIncludeInClipboardHistory");
        RegisterClipboardFormatW(PCWSTR(w.as_ptr()))
    })
}

/// Pure privacy decision helper: returns `true` when clipboard content MUST be skipped
/// because a password manager / sensitive app marked it excluded from monitoring or history.
pub fn should_skip_private_clipboard(
    has_exclude_monitor_format: bool,
    can_include_history_dword: Option<u32>,
) -> bool {
    if has_exclude_monitor_format {
        return true;
    }
    matches!(can_include_history_dword, Some(0))
}

unsafe fn open_clipboard_with_retry(attempts: usize, delay_ms: u64) -> bool {
    for attempt in 0..attempts.max(1) {
        if OpenClipboard(HWND::default()).is_ok() {
            return true;
        }
        if attempt + 1 < attempts && delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
    }
    false
}

unsafe fn is_open_clipboard_marked_private() -> bool {
    let exclude_fmt = exclude_monitor_format_id();
    let has_exclude = exclude_fmt != 0 && IsClipboardFormatAvailable(exclude_fmt).is_ok();
    if has_exclude {
        return true;
    }

    let history_fmt = can_include_history_format_id();
    let can_include_dword = if history_fmt != 0 && IsClipboardFormatAvailable(history_fmt).is_ok() {
        GetClipboardData(history_fmt).ok().and_then(|handle| {
            let hglobal = HGLOBAL(handle.0);
            let byte_size = GlobalSize(hglobal);
            if byte_size < std::mem::size_of::<u32>() {
                return None;
            }
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                return None;
            }
            let val = std::ptr::read_unaligned(ptr as *const u32);
            let _ = GlobalUnlock(hglobal);
            Some(val)
        })
    } else {
        None
    };

    should_skip_private_clipboard(has_exclude, can_include_dword)
}

fn read_clipboard_text_attempts(attempts: usize, delay_ms: u64) -> Option<String> {
    unsafe {
        let exclude_fmt = exclude_monitor_format_id();
        if exclude_fmt != 0 && IsClipboardFormatAvailable(exclude_fmt).is_ok() {
            return None;
        }
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        if !open_clipboard_with_retry(attempts, delay_ms) {
            return None;
        }
        let mut out = None;
        if !is_open_clipboard_marked_private() {
            if let Ok(handle) = GetClipboardData(CF_UNICODETEXT) {
                let hglobal = HGLOBAL(handle.0);
                let alloc_bytes = GlobalSize(hglobal);
                let max_u16 = if alloc_bytes >= 2 {
                    (alloc_bytes / 2).min(MAX_CLIPBOARD_CHARS + 1)
                } else {
                    MAX_CLIPBOARD_CHARS + 1
                };
                let ptr = GlobalLock(hglobal) as *const u16;
                if !ptr.is_null() {
                    let mut len = 0usize;
                    while len < max_u16 && *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
                    let normalized = normalize_clipboard_text(&s);
                    if !normalized.is_empty() {
                        out = Some(normalized);
                    }
                    let _ = GlobalUnlock(hglobal);
                }
            }
        }
        let _ = CloseClipboard();
        out
    }
}

pub fn read_clipboard_text() -> Option<String> {
    read_clipboard_text_attempts(CLIPBOARD_OPEN_RETRY_ATTEMPTS, CLIPBOARD_OPEN_RETRY_DELAY_MS)
}

pub fn write_clipboard_text(text: &str) -> Result<(), String> {
    unsafe {
        let w = wide(text);
        let bytes = w.len() * 2;
        if !open_clipboard_with_retry(CLIPBOARD_OPEN_RETRY_ATTEMPTS, CLIPBOARD_OPEN_RETRY_DELAY_MS) {
            return Err("Не удалось открыть буфер обмена".to_string());
        }
        if let Err(e) = EmptyClipboard() {
            let _ = CloseClipboard();
            return Err(format!("Не удалось очистить буфер обмена: {e}"));
        }
        let hmem = match GlobalAlloc(GMEM_MOVEABLE, bytes) {
            Ok(h) if !h.0.is_null() => h,
            Ok(_) => {
                let _ = CloseClipboard();
                return Err("Не удалось выделить память для буфера обмена".to_string());
            }
            Err(e) => {
                let _ = CloseClipboard();
                return Err(e.to_string());
            }
        };
        let dst = GlobalLock(hmem) as *mut u16;
        if dst.is_null() {
            let _ = GlobalFree(hmem);
            let _ = CloseClipboard();
            return Err("Не удалось заблокировать память буфера обмена".to_string());
        }
        std::ptr::copy_nonoverlapping(w.as_ptr(), dst, w.len());
        let _ = GlobalUnlock(hmem);
        // On SetClipboardData failure Windows does NOT assume ownership of hmem, so we must free it.
        let set_res = SetClipboardData(CF_UNICODETEXT, HANDLE(hmem.0));
        if let Err(e) = set_res {
            let _ = GlobalFree(hmem);
            let _ = CloseClipboard();
            return Err(format!("Не удалось записать в буфер обмена: {e}"));
        }
        let _ = CloseClipboard();
        Ok(())
    }
}

fn is_hex_color(t: &str) -> bool {
    t.starts_with('#')
        && matches!(t.len(), 4 | 7 | 9)
        && t[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn is_url_clip(t: &str) -> bool {
    if t.contains('\n') || t.chars().any(char::is_whitespace) {
        return false;
    }
    let low = t.to_ascii_lowercase();
    (low.starts_with("http://") || low.starts_with("https://")) && t.len() > 8
}

fn is_path_clip(t: &str) -> bool {
    if t.contains('\n') {
        return false;
    }
    let unquoted = t.trim_matches('"');
    if unquoted.starts_with("\\\\") && unquoted.len() > 3 {
        return true;
    }
    let bytes = unquoted.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn looks_like_code_clip(t: &str) -> bool {
    let trimmed = t.trim();
    if trimmed.is_empty() {
        return false;
    }
    // JSON object / array
    if (trimmed.starts_with('{') && trimmed.ends_with('}') && trimmed.contains(':'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']') && (trimmed.contains('{') || trimmed.contains('"')))
    {
        return true;
    }
    // Shebang / XML / HTML / PHP / SQL / common language constructs
    if trimmed.starts_with("#!/") || trimmed.starts_with("<?") {
        return true;
    }
    let low = trimmed.to_ascii_lowercase();
    if low.starts_with("select ") && low.contains(" from ")
        || low.starts_with("insert into ")
        || low.starts_with("create table ")
        || low.starts_with("update ") && low.contains(" set ")
    {
        return true;
    }
    let first_line = trimmed.lines().next().unwrap_or("").trim();
    let cli_prefixes = [
        "git ", "cargo ", "pnpm ", "npm ", "npx ", "yarn ", "docker ", "kubectl ", "curl ",
        "uv ", "python ", "pytest ", "rustc ", "winget ", "powershell ",
    ];
    if cli_prefixes.iter().any(|p| first_line.starts_with(p)) {
        return true;
    }
    let code_keywords = [
        "fn ", "pub fn ", "pub struct ", "struct ", "impl ", "enum ", "use ", "const ", "let ",
        "var ", "function ", "export ", "import ", "class ", "interface ", "type ", "def ",
        "async ", "await ", "return ", "#include ", "package ",
    ];
    let has_keyword_line = trimmed.lines().any(|line| {
        let l = line.trim_start();
        code_keywords.iter().any(|kw| l.starts_with(kw))
    });
    if has_keyword_line {
        return true;
    }
    let has_markup = trimmed.contains("</") && trimmed.contains('>');
    if has_markup {
        return true;
    }
    let has_syntax_ops = trimmed.contains("=>")
        || trimmed.contains("::")
        || trimmed.contains("===")
        || trimmed.contains("!==")
        || trimmed.contains("&&")
        || trimmed.contains("||")
        || (trimmed.contains('{') && trimmed.contains('}') && (trimmed.contains(';') || trimmed.contains("()")));
    if has_syntax_ops {
        return true;
    }
    let semi_lines = trimmed.lines().filter(|l| l.trim_end().ends_with(';')).count();
    semi_lines >= 2
}

/// Smart deterministic clipboard classifier returning `(kind, tag, color)`:
/// `kind` is `"url" | "path" | "hex" | "code" | "text"`.
pub fn classify_clipboard_content(text: &str) -> (String, String, Option<String>) {
    let t = text.trim();
    if is_url_clip(t) {
        return ("url".to_string(), "URL".to_string(), None);
    }
    if is_hex_color(t) {
        return ("hex".to_string(), "HEX".to_string(), Some(t.to_string()));
    }
    if is_path_clip(t) {
        return ("path".to_string(), "PATH".to_string(), None);
    }
    if looks_like_code_clip(t) {
        return ("code".to_string(), "CODE".to_string(), None);
    }
    ("text".to_string(), "TXT".to_string(), None)
}

/// Preserves full clipboard text losslessly (normalizing CRLF/CR to LF and trimming outer
/// whitespace, capped safely at `MAX_CLIPBOARD_CHARS` = 65,536 chars).
pub fn normalize_clipboard_text(text: &str) -> String {
    let normalized_nl = text.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized_nl.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= MAX_CLIPBOARD_CHARS {
        trimmed.to_string()
    } else {
        trimmed.chars().take(MAX_CLIPBOARD_CHARS).collect()
    }
}

/// Builds a clean 1–2 line display preview capped strictly at `CLIPBOARD_PREVIEW_MAX_CHARS` (180).
pub fn build_clipboard_preview(full_text: &str) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(2);
    let mut has_more_lines = false;
    for raw_line in full_text.lines() {
        let collapsed: String = raw_line
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if collapsed.is_empty() {
            continue;
        }
        if lines.len() < 2 {
            lines.push(collapsed);
        } else {
            has_more_lines = true;
            break;
        }
    }
    let joined = lines.join("\n");
    let count = joined.chars().count();
    if count > CLIPBOARD_PREVIEW_MAX_CHARS {
        let mut out: String = joined.chars().take(CLIPBOARD_PREVIEW_MAX_CHARS - 1).collect();
        out.push('…');
        out
    } else if has_more_lines && count < CLIPBOARD_PREVIEW_MAX_CHARS {
        format!("{joined}…")
    } else {
        joined
    }
}

fn build_clipboard_entry(id: String, text: String, pinned: bool) -> ClipboardEntry {
    let (kind, tag, color) = classify_clipboard_content(&text);
    let preview = build_clipboard_preview(&text);
    let line_count = if text.is_empty() {
        0
    } else {
        text.lines().count().max(1)
    };
    let char_count = text.chars().count();
    ClipboardEntry {
        id,
        tag,
        text,
        color,
        pinned,
        preview,
        line_count,
        char_count,
        kind,
    }
}

fn next_clip_id() -> String {
    let seq = CLIP_SEQ.fetch_add(1, Ordering::Relaxed);
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
        let normalized = normalize_clipboard_text(raw);
        if normalized.is_empty() {
            continue;
        }
        if let Some(existing) = list.iter_mut().find(|c| c.text == normalized) {
            let refreshed = build_clipboard_entry(existing.id.clone(), normalized, true);
            *existing = refreshed;
        } else {
            list.push(build_clipboard_entry(next_clip_id(), normalized, true));
        }
    }
    trim_unpinned_overflow(list);
}

pub fn push_clipboard_text_into(list: &mut Vec<ClipboardEntry>, text: &str) {
    let normalized = normalize_clipboard_text(text);
    if normalized.is_empty() {
        return;
    }

    if let Some(pos) = list.iter().position(|c| c.text == normalized) {
        let old = list.remove(pos);
        let updated = build_clipboard_entry(old.id, normalized, old.pinned);
        list.insert(0, updated);
    } else {
        list.insert(0, build_clipboard_entry(next_clip_id(), normalized, false));
    }
    trim_unpinned_overflow(list);
}

pub fn toggle_clipboard_pin_in(list: &mut Vec<ClipboardEntry>, id: &str) {
    if let Some(c) = list.iter_mut().find(|c| c.id == id) {
        c.pinned = !c.pinned;
    }
    trim_unpinned_overflow(list);
}

pub fn delete_clipboard_entry_in(list: &mut Vec<ClipboardEntry>, id: &str) -> bool {
    let before = list.len();
    list.retain(|c| c.id != id);
    list.len() != before
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

pub fn delete_clipboard_entry(id: &str) -> Vec<ClipboardEntry> {
    if let Ok(mut list) = clipboard_store().lock() {
        let _ = delete_clipboard_entry_in(&mut list, id);
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

pub fn set_clipboard_paused(paused: bool) -> bool {
    CLIPBOARD_PAUSED.store(paused, Ordering::Release);
    paused
}

pub fn is_clipboard_paused() -> bool {
    CLIPBOARD_PAUSED.load(Ordering::Acquire)
}

/// Builds an atomic 4-event `[VK_CONTROL down, 'V' down, 'V' up, VK_CONTROL up]` `SendInput` batch.
pub fn build_ctrl_v_inputs() -> [INPUT; 4] {
    [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(b'V' as u16),
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(b'V' as u16),
                    dwFlags: KEYEVENTF_KEYUP,
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    dwFlags: KEYEVENTF_KEYUP,
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
    ]
}

fn is_valid_external_hwnd(raw: isize, own_pid: u32) -> bool {
    if raw == 0 {
        return false;
    }
    let hwnd = HWND(raw as *mut _);
    unsafe {
        if !IsWindow(hwnd).as_bool() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid != 0 && pid != own_pid
    }
}

/// Polls until an external (non-TestBar) foreground window owns input focus, up to
/// `max_attempts` checks spaced by `step_delay_ms`.
pub fn wait_for_external_foreground<FPoll>(
    max_attempts: usize,
    step_delay_ms: u64,
    mut is_external_fg: FPoll,
) -> bool
where
    FPoll: FnMut() -> bool,
{
    let attempts = max_attempts.max(1);
    for attempt in 0..attempts {
        if is_external_fg() {
            return true;
        }
        if attempt + 1 < attempts && step_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(step_delay_ms));
        }
    }
    false
}

fn paste_clipboard_text_with_ops<FWrite, FValid, FFocus, FWaitFg, FSend>(
    target_hwnd: isize,
    fallback_hwnd: isize,
    text: &str,
    mut write_clip: FWrite,
    mut is_valid_external: FValid,
    mut focus_hwnd: FFocus,
    mut wait_external_fg: FWaitFg,
    mut send_ctrl_v: FSend,
) -> Result<(), String>
where
    FWrite: FnMut(&str) -> Result<(), String>,
    FValid: FnMut(isize) -> bool,
    FFocus: FnMut(isize),
    FWaitFg: FnMut() -> bool,
    FSend: FnMut() -> Result<(), String>,
{
    if text.is_empty() {
        return Err("Пустой текст для вставки".to_string());
    }
    // Always write to the Windows clipboard first so the entry is safely copied even if focus fails.
    write_clip(text)?;

    let resolved_hwnd = if target_hwnd != 0 && is_valid_external(target_hwnd) {
        target_hwnd
    } else if fallback_hwnd != 0 && is_valid_external(fallback_hwnd) {
        fallback_hwnd
    } else {
        0
    };

    if resolved_hwnd == 0 {
        return Err(
            "Текст скопирован в буфер, но нет внешнего целевого окна для вставки".to_string(),
        );
    }

    focus_hwnd(resolved_hwnd);
    if !wait_external_fg() {
        return Err(
            "Текст скопирован в буфер, но не удалось вернуть фокус целевому окну для вставки"
                .to_string(),
        );
    }

    send_ctrl_v()
}

/// Writes `text` safely to the Windows clipboard, restores focus to `target_hwnd` (or last
/// tracked external foreground window if `target_hwnd == 0`), verifies `GetForegroundWindow()`
/// actually left TestBar, and injects `Ctrl+V` via `SendInput`. Never injects blind `Ctrl+V`
/// if no valid external target window exists or focus never leaves TestBar.
pub fn paste_clipboard_text(target_hwnd: isize, text: &str) -> Result<(), String> {
    let own_pid = unsafe { GetCurrentProcessId() };
    let fallback_hwnd = last_fg_store().lock().ok().map(|g| *g).unwrap_or(0);
    paste_clipboard_text_with_ops(
        target_hwnd,
        fallback_hwnd,
        text,
        write_clipboard_text,
        |h| is_valid_external_hwnd(h, own_pid),
        |h| focus_or_toggle_window(h, false),
        || {
            wait_for_external_foreground(16, 10, || {
                let fg = unsafe { GetForegroundWindow() };
                is_valid_external_hwnd(fg.0 as isize, own_pid)
            })
        },
        || {
            let inputs = build_ctrl_v_inputs();
            let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
            if sent as usize == inputs.len() {
                Ok(())
            } else {
                Err("Не удалось вставить текст (целевое окно заблокировало ввод)".to_string())
            }
        },
    )
}

pub fn start_clipboard_listener<F>(initial_pinned: &[String], on_change: F)
where
    F: Fn(Vec<ClipboardEntry>) + Send + Sync + 'static,
{
    hydrate_pinned_clipboard(initial_pinned);
    if !is_clipboard_paused() {
        if let Some(initial) = read_clipboard_text() {
            push_clipboard_text(initial);
        }
    }

    static CLIP_NOTIFY_TX: OnceLock<mpsc::SyncSender<()>> = OnceLock::new();
    let (tx, rx) = mpsc::sync_channel::<()>(8);
    let _ = CLIP_NOTIFY_TX.set(tx);

    // Dedicated worker thread performs retry-capable clipboard reads OFF the WndProc pump.
    let _ = std::thread::Builder::new()
        .name("testbar-clip-worker".to_string())
        .spawn(move || {
            while rx.recv().is_ok() {
                // Drain coalesced bursts before reading
                while rx.try_recv().is_ok() {}
                if is_clipboard_paused() {
                    continue;
                }
                if let Some(text) = read_clipboard_text() {
                    push_clipboard_text(text);
                    on_change(get_clipboard_history());
                }
            }
        });

    let _ = std::thread::Builder::new()
        .name("testbar-clip-pump".to_string())
        .spawn(|| unsafe {
            extern "system" fn wnd_proc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                if msg == WM_CLIPBOARDUPDATE {
                    if !is_clipboard_paused() {
                        if let Some(tx) = CLIP_NOTIFY_TX.get() {
                            let _ = tx.try_send(());
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
                loop {
                    let ret = GetMessageW(&mut msg, HWND::default(), 0, 0);
                    if ret.0 <= 0 {
                        break;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
}

// ======================= WIN-CHORD SENDINPUT HELPERS =======================

/// Builds an atomic 4-event `[VK_LWIN down, vk down, vk up, VK_LWIN up]` `SendInput` batch.
/// Tagged with `TESTBAR_INJECTED_EXTRA_INFO` so TestBar's `WH_KEYBOARD_LL` hook recognizes
/// our own `Win+N`/`Win+B` chords and passes them through while still intercepting untagged
/// external injected `VK_LWIN` taps from third-party tools.
pub fn build_win_chord_inputs(vk: u16) -> [INPUT; 4] {
    [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_LWIN,
                    dwFlags: KEYEVENTF_EXTENDEDKEY,
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
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
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
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
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
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
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
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
                    dwExtraInfo: crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
    ];
    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn find_shell_tray_hwnd() -> Option<HWND> {
    unsafe {
        let cls = wide("Shell_TrayWnd");
        FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null())
            .ok()
            .filter(|h| !h.0.is_null())
    }
}

fn find_native_tray_overflow_hwnd() -> Option<HWND> {
    unsafe {
        let cls = wide("TopLevelWindowForOverflowXamlIsland");
        FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null())
            .ok()
            .filter(|h| !h.0.is_null())
    }
}

pub fn is_native_shell_tray_visible() -> bool {
    find_shell_tray_hwnd()
        .map(|hwnd| unsafe { IsWindowVisible(hwnd).as_bool() })
        .unwrap_or(false)
}

fn is_native_tray_or_overflow_focused(tray_hwnd: HWND) -> bool {
    unsafe {
        if let Some(ov) = find_native_tray_overflow_hwnd() {
            if IsWindowVisible(ov).as_bool() {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        if fg == tray_hwnd {
            return true;
        }
        let mut cls_buf = [0u16; 96];
        let len = GetClassNameW(fg, &mut cls_buf);
        if len > 0 {
            let cls = String::from_utf16_lossy(&cls_buf[..len as usize]);
            if matches!(
                cls.as_str(),
                "TopLevelWindowForOverflowXamlIsland"
                    | "NotifyIconOverflowWindow"
                    | "Shell_TrayWnd"
                    | "Xaml_WindowedPopupClass"
                    | "#32768"
            ) {
                return true;
            }
        }
        false
    }
}

/// Triggers native Windows 11 Notification Center via `Win+N` (`AD-7`, zero MSIX `UserNotificationListener`).
pub fn open_windows_notifications() -> Result<(), String> {
    send_win_chord(b'N' as u16)
}

/// Triggers native Windows system tray overflow (`Win+B` + `Enter`) cleanly.
/// When `Shell_TrayWnd` is hidden under TestBar, temporarily reveals it so Windows 11's XAML
/// overflow flyout actually opens, then spawns a bounded watcher thread that re-hides
/// `Shell_TrayWnd` once the overflow closes or loses focus.
pub fn trigger_native_tray_overflow() -> Result<(), String> {
    let tray_opt = find_shell_tray_hwnd();
    let was_visible = is_native_shell_tray_visible();

    if !was_visible {
        if let Some(tray_hwnd) = tray_opt {
            unsafe {
                let _ = ShowWindow(tray_hwnd, SW_SHOWNOACTIVATE);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    send_win_chord(b'B' as u16)?;

    if was_visible {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(60));
            send_single_vk(VK_RETURN);
        });
    } else if let Some(tray_hwnd) = tray_opt {
        let tray_raw = tray_hwnd.0 as isize;
        let gen = NATIVE_TRAY_WATCHER_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = std::thread::Builder::new()
            .name("testbar-native-tray-watcher".to_string())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                if NATIVE_TRAY_WATCHER_GEN.load(Ordering::SeqCst) != gen {
                    return;
                }
                send_single_vk(VK_RETURN);

                let hwnd = HWND(tray_raw as *mut _);
                let started = Instant::now();
                let mut saw_overflow_active = false;
                let max_watch = Duration::from_secs(15);
                let initial_grace = Duration::from_millis(900);

                while started.elapsed() < max_watch {
                    std::thread::sleep(Duration::from_millis(150));
                    if NATIVE_TRAY_WATCHER_GEN.load(Ordering::SeqCst) != gen
                        || crate::win_shell::is_restore_in_progress()
                    {
                        return;
                    }
                    let active = is_native_tray_or_overflow_focused(hwnd);
                    if active {
                        saw_overflow_active = true;
                        continue;
                    }
                    if saw_overflow_active || started.elapsed() >= initial_grace {
                        break;
                    }
                }

                if NATIVE_TRAY_WATCHER_GEN.load(Ordering::SeqCst) == gen
                    && !crate::win_shell::is_restore_in_progress()
                {
                    unsafe {
                        if IsWindow(hwnd).as_bool() {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                        }
                    }
                }
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

fn classify_peripheral_kind(clean_name: &str) -> &'static str {
    let low = clean_name.to_ascii_lowercase();
    if low.contains("mouse")
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
    }
}

const PERIPHERAL_BATTERY_CACHE_TTL: Duration = Duration::from_secs(30);

static PERIPHERAL_BATTERY_CACHE: OnceLock<Mutex<Option<(Instant, Vec<PeripheralBattery>)>>> =
    OnceLock::new();
static PERIPHERAL_BATTERY_SCAN_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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

fn scan_peripheral_batteries_live() -> Vec<PeripheralBattery> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for enumerator in ["BTH", "BTHLE"] {
        let enum_w = wide(enumerator);
        unsafe {
            let Ok(hdev) = SetupDiGetClassDevsW(
                None,
                PCWSTR(enum_w.as_ptr()),
                HWND::default(),
                DIGCF_PRESENT | DIGCF_ALLCLASSES,
            ) else {
                continue;
            };
            let _hdev_guard = DevInfoGuard(hdev);

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
                    // Properly aligned `[u16; 256]` (align 2) avoids unaligned `*const u16` UB.
                    let mut name_buf = [0u16; 256];
                    let name_bytes = std::slice::from_raw_parts_mut(
                        name_buf.as_mut_ptr() as *mut u8,
                        std::mem::size_of_val(&name_buf),
                    );
                    let mut name_req = 0u32;
                    let name = if SetupDiGetDevicePropertyW(
                        hdev,
                        &info,
                        &DEVPKEY_Device_FriendlyName,
                        &mut prop_type,
                        Some(name_bytes),
                        Some(&mut name_req),
                        0,
                    )
                    .is_ok()
                        && name_req >= 2
                    {
                        let u16_len = ((name_req as usize) / 2).min(name_buf.len());
                        let slice = &name_buf[..u16_len];
                        let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
                        String::from_utf16_lossy(&slice[..end])
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
                        results.push(PeripheralBattery {
                            kind: classify_peripheral_kind(&clean_name).to_string(),
                            name: clean_name,
                            percent: pct,
                        });
                    }
                }
            }
        }
    }

    results
}

/// Reads real Bluetooth device battery percentages reported by Windows 11 PnP
/// (`DEVPKEY_Bluetooth_Battery`), restricted strictly to `"BTH"` and `"BTHLE"` enumerators.
/// Cached for 30s and guarded by single-flight `try_lock` so rapid `Win` presses NEVER stack
/// parallel SetupAPI/PnP device-tree scans.
pub fn get_peripheral_batteries() -> Vec<PeripheralBattery> {
    let cache = PERIPHERAL_BATTERY_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((ts, ref list)) = *guard {
            if ts.elapsed() < PERIPHERAL_BATTERY_CACHE_TTL {
                return list.clone();
            }
        }
    }

    let scan_lock = PERIPHERAL_BATTERY_SCAN_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_scan_guard) = scan_lock.try_lock() else {
        // Another thread is already performing the PnP Bluetooth scan; return cached snapshot immediately.
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        return guard
            .as_ref()
            .map(|(_, list)| list.clone())
            .unwrap_or_default();
    };

    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((ts, ref list)) = *guard {
            if ts.elapsed() < PERIPHERAL_BATTERY_CACHE_TTL {
                return list.clone();
            }
        }
    }

    let fresh = scan_peripheral_batteries_live();
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((Instant::now(), fresh.clone()));
    fresh
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
            | "shellhost.exe"
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
            | "systemsettings.exe"
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
            | "crossdeviceservice.exe"
            | "yourphone.exe"
            | "gamebar.exe"
            | "gamebarftserver.exe"
            | "amdrsserv.exe"
            | "amdow.exe"
            | "atieclxx.exe"
            | "atiesrxx.exe"
            | "cncmd.exe"
            | "nvcontainer.exe"
            | "nvdisplay.container.exe"
            | "nvtelemetrycontainer.exe"
            | "nvsphelper64.exe"
            | "tabtip.exe"
            | "tabtip32.exe"
            | "msctfmonitor.exe"
            | "credentialuibroker.exe"
            | "useroobebroker.exe"
            | "systemsettingsbroker.exe"
            | "pickerhost.exe"
            | "openwith.exe"
            | "consent.exe"
            | "presentationfontcache.exe"
            | "update.exe"
    ) {
        return true;
    }

    if base.starts_with("gamebar")
        || base.starts_with("amdrsserv")
        || base.starts_with("widgets")
    {
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
        has_meaningful_window: bool,
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
        if base_exe.is_empty() {
            continue;
        }
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
        let meaningful =
            !c.title.trim().is_empty() || c.has_class_icon || c.has_app_frame_style;
        // Deduplicate strictly by lowercase executable filename (`base_exe`) so multi-worker
        // apps never spawn duplicate tray icons across sibling/versioned subdirectories.
        match groups.get_mut(&base_exe) {
            Some(acc) => {
                acc.pids.insert(c.pid);
                acc.has_meaningful_window |= meaningful;
                if score > acc.best_score {
                    acc.best_score = score;
                    acc.best_hwnd = c.hwnd;
                    acc.best_pid = c.pid;
                    acc.exe_path = c.exe_path;
                }
            }
            None => {
                let mut pids = HashSet::new();
                pids.insert(c.pid);
                groups.insert(
                    base_exe.clone(),
                    GroupAcc {
                        id: base_exe,
                        exe_path: c.exe_path,
                        best_hwnd: c.hwnd,
                        best_pid: c.pid,
                        best_score: score,
                        has_meaningful_window: meaningful,
                        pids,
                    },
                );
            }
        }
    }

    let mut items: Vec<TrayAppItem> = groups
        .into_values()
        .filter_map(|acc| {
            let mut all_pids: Vec<u32> = acc.pids.into_iter().collect();
            all_pids.sort_unstable();
            let hwnd = (acc.best_hwnd != 0).then_some(acc.best_hwnd);
            let name = friendly_exe_name(&acc.exe_path);
            let icon_key = resolve_icon_key(&acc.exe_path, hwnd);
            // Drop noisy headless worker hosts that have neither an extracted icon nor a
            // titled/framed/iconed window of their own.
            if icon_key.is_none() && !acc.has_meaningful_window {
                return None;
            }
            Some(TrayAppItem {
                id: acc.id,
                hwnd,
                pid: acc.best_pid,
                all_pids,
                name,
                exe_path: acc.exe_path,
                icon_key,
                icon_data: None,
            })
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

fn enumerate_tray_window_candidates(own_pid: u32) -> Vec<RawTrayWindowCandidate> {
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

    let mut ctx = EnumCtx {
        own_pid,
        exe_memo: HashMap::new(),
        candidates: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.candidates
}

fn extract_exe_icon_data_uri(path: &str) -> Option<String> {
    extract_path_icon_raw(path)
}

fn extract_hwnd_icon_data_uri(hwnd: HWND) -> Option<String> {
    if hwnd.0.is_null() {
        return None;
    }
    unsafe {
        for kind in [ICON_BIG, ICON_SMALL2, ICON_SMALL] {
            let mut result: usize = 0;
            let ok = SendMessageTimeoutW(
                hwnd,
                WM_GETICON,
                WPARAM(kind as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                40,
                Some(&mut result),
            );
            if ok.0 != 0 && result != 0 {
                if let Some(url) = hicon_to_data_url(HICON(result as *mut _), 64) {
                    return Some(url);
                }
            }
        }
        for idx in [GCLP_HICON, GCLP_HICONSM] {
            let raw = GetClassLongPtrW(hwnd, idx);
            if raw != 0 {
                if let Some(url) = hicon_to_data_url(HICON(raw as *mut _), 64) {
                    return Some(url);
                }
            }
        }
    }
    None
}

fn resolve_tray_icon_into_cache(
    exe_path: &str,
    hwnd_raw: Option<isize>,
    cache: &mut IconCache,
    icon_sources: &mut Vec<(String, String)>,
    seen_sources: &mut HashSet<String>,
) -> Option<String> {
    let trimmed = exe_path.trim();
    let hwnd = hwnd_raw
        .filter(|&h| h != 0)
        .map(|h| HWND(h as *mut _))
        .unwrap_or_default();

    if !trimmed.is_empty() && !is_application_frame_host(trimmed) {
        let key = icon_cache::path_icon_key(trimmed);
        if seen_sources.insert(key.clone()) {
            icon_sources.push((key.clone(), trimmed.to_string()));
        }
        if cache.get(&key).is_some() {
            return Some(key);
        }
        let extracted = extract_exe_icon_data_uri(trimmed).or_else(|| extract_hwnd_icon_data_uri(hwnd));
        if let Some(uri) = extracted {
            cache.put(key.clone(), uri);
            return Some(key);
        }
    }

    if let Some(raw) = hwnd_raw.filter(|&h| h != 0) {
        let key = icon_cache::hwnd_key(raw, 0);
        if cache.get(&key).is_some() {
            return Some(key);
        }
        let extracted = extract_hwnd_icon_data_uri(HWND(raw as *mut _))?;
        cache.put(key.clone(), extracted);
        return Some(key);
    }

    None
}

/// Pure filter removing user-hidden tray entries matching by normalized `id`, `exe_path`,
/// executable basename, or display `name` (case-insensitive).
pub fn filter_hidden_tray_apps(items: Vec<TrayAppInfo>, hidden_keys: &[String]) -> Vec<TrayAppInfo> {
    if hidden_keys.is_empty() {
        return items;
    }
    let hidden_set: HashSet<String> = hidden_keys
        .iter()
        .map(|k| canonical_tray_exe_key(k))
        .filter(|k| !k.is_empty())
        .collect();
    if hidden_set.is_empty() {
        return items;
    }

    items
        .into_iter()
        .filter(|item| {
            let norm_id = canonical_tray_exe_key(&item.id);
            let norm_exe = canonical_tray_exe_key(&item.exe_path);
            let base = exe_basename_lower(&norm_exe);
            let name_low = item.name.trim().to_ascii_lowercase();
            !(hidden_set.contains(&norm_id)
                || hidden_set.contains(&norm_exe)
                || (!base.is_empty() && hidden_set.contains(&base))
                || (!name_low.is_empty() && hidden_set.contains(&name_low)))
        })
        .collect()
}

// ======================= WIN11 NOTIFYICONSETTINGS TRAY (OPTION A) =======================

const NOTIFY_ICON_SETTINGS_SUBKEY: &str = r"Control Panel\NotifyIconSettings";
const MAX_NOTIFY_ICON_SNAPSHOT_READ_BYTES: usize = 256 * 1024;
const MAX_NOTIFY_ICON_SNAPSHOT_CACHE_BYTES: usize = 128 * 1024;
const FNV1A_TRAY_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_TRAY_PRIME: u64 = 0x0000_0100_0000_01b3;

static KNOWN_FOLDER_CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();

fn known_folder_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    KNOWN_FOLDER_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

struct RegKeyGuard(HKEY);

impl Drop for RegKeyGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
}

struct SnapshotHandleGuard(HANDLE);

impl Drop for SnapshotHandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

struct CoTaskMemPwstrGuard(PWSTR);

impl Drop for CoTaskMemPwstrGuard {
    fn drop(&mut self) {
        if !self.0 .0.is_null() {
            unsafe {
                CoTaskMemFree(Some(self.0 .0 as *const _));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawNotifyIconEntry {
    pub subkey: String,
    pub executable_path: String,
    pub initial_tooltip: String,
    pub publisher: String,
    pub is_promoted: Option<bool>,
    pub icon_snapshot: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveProcessEntry {
    pub pid: u32,
    pub exe_path: String,
    pub exe_name: String,
}

fn parse_guid_str(raw: &str) -> Option<GUID> {
    let trimmed = raw.trim().trim_start_matches('{').trim_end_matches('}');
    let parts: Vec<&str> = trimmed.split('-').collect();
    if parts.len() != 5 {
        return None;
    }
    if parts[0].len() != 8
        || parts[1].len() != 4
        || parts[2].len() != 4
        || parts[3].len() != 4
        || parts[4].len() != 12
    {
        return None;
    }
    let data1 = u32::from_str_radix(parts[0], 16).ok()?;
    let data2 = u16::from_str_radix(parts[1], 16).ok()?;
    let data3 = u16::from_str_radix(parts[2], 16).ok()?;
    let d4_hi = u16::from_str_radix(parts[3], 16).ok()?;
    let d4_lo = u64::from_str_radix(parts[4], 16).ok()?;
    let data4 = [
        (d4_hi >> 8) as u8,
        (d4_hi & 0xff) as u8,
        ((d4_lo >> 40) & 0xff) as u8,
        ((d4_lo >> 32) & 0xff) as u8,
        ((d4_lo >> 24) & 0xff) as u8,
        ((d4_lo >> 16) & 0xff) as u8,
        ((d4_lo >> 8) & 0xff) as u8,
        (d4_lo & 0xff) as u8,
    ];
    Some(GUID::from_values(data1, data2, data3, data4))
}

/// Splits a Win11 `NotifyIconSettings` Known Folder prefixed path like
/// `{6D809377-6AF0-444B-8957-A3773F02200E}\App\app.exe` into `("{6D809377-...}", "App\\app.exe")`.
pub fn split_known_folder_prefix(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let close_idx = trimmed.find('}')?;
    if close_idx != 37 {
        return None;
    }
    let guid_braced = &trimmed[..=close_idx];
    let rest = trimmed[close_idx + 1..].trim_start_matches(['\\', '/']);
    Some((guid_braced, rest))
}

/// Pure Known Folder path normalizer accepting an injectable resolver for deterministic unit tests.
pub fn resolve_known_folder_path_with<F>(raw: &str, resolver: F) -> String
where
    F: FnOnce(&str) -> Option<String>,
{
    let trimmed = raw.trim();
    if let Some((guid_braced, rest)) = split_known_folder_prefix(trimmed) {
        if let Some(base) = resolver(guid_braced) {
            let clean_base = base.trim().trim_end_matches(['\\', '/']);
            if rest.is_empty() {
                return clean_base.to_string();
            }
            return format!("{clean_base}\\{}", rest.replace('/', "\\"));
        }
    }
    trimmed.replace('/', "\\")
}

fn resolve_known_folder_guid_cached(guid_braced: &str) -> Option<String> {
    let key = guid_braced.to_ascii_uppercase();
    if let Some(hit) = known_folder_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned()
    {
        return hit;
    }

    let resolved = parse_guid_str(&key).and_then(|guid| unsafe {
        let pwstr = SHGetKnownFolderPath(&guid, KNOWN_FOLDER_FLAG(0), HANDLE::default()).ok()?;
        let _guard = CoTaskMemPwstrGuard(pwstr);
        let s = pwstr.to_string().ok()?;
        let trimmed = s.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    });

    known_folder_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, resolved.clone());
    resolved
}

fn resolve_known_folder_path_live(raw: &str) -> String {
    resolve_known_folder_path_with(raw, resolve_known_folder_guid_cached)
}

/// Validates a PNG header (`\x89PNG\r\n\x1a\n` + `IHDR`) and extracts `(width, height)` in `1..=2048`.
pub fn parse_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 {
        return None;
    }
    if bytes[..8] != PNG_SIG || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if !(1..=2048).contains(&width) || !(1..=2048).contains(&height) {
        return None;
    }
    Some((width, height))
}

/// Chooses whether to prefer Win11's stored `IconSnapshot` PNG (`tray:<hash>`) over extracting
/// the executable icon (`exe:<hash>`):
/// - Always prefers `IconSnapshot` for `nvcontainer.exe` (generic exe icon vs crisp 128px NVIDIA icon).
/// - Prefers `IconSnapshot` for `WindowsApps` paths (when `<= 256px`) and crisp native tray icons (`32..=256px`).
/// - Prefers crisp exe extraction first for tiny `16×16` snapshots or oversized `> 256px` blobs
///   (falling back to `IconSnapshot` if exe extraction fails).
pub fn should_prefer_icon_snapshot(exe_path: &str, snap_dims: Option<(u32, u32)>) -> bool {
    let Some((w, h)) = snap_dims else {
        return false;
    };
    let norm = canonical_tray_exe_key(exe_path);
    let base = exe_basename_lower(&norm);
    if base == "nvcontainer.exe" {
        return true;
    }
    if norm.contains(r"\windowsapps\") && w <= 256 && h <= 256 {
        return true;
    }
    (32..=256).contains(&w) && (32..=256).contains(&h)
}

/// Deterministic `IconCache` key (`tray:<16hex>`) for a `NotifyIconSettings` PNG `IconSnapshot`.
pub fn tray_snapshot_icon_key(subkey_name: &str, base_exe: &str) -> String {
    let mut hash = FNV1A_TRAY_OFFSET_BASIS;
    for byte in subkey_name.trim().to_ascii_lowercase().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A_TRAY_PRIME);
    }
    hash ^= u64::from(b'|');
    hash = hash.wrapping_mul(FNV1A_TRAY_PRIME);
    for byte in base_exe.trim().to_ascii_lowercase().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A_TRAY_PRIME);
    }
    format!("tray:{hash:016x}")
}

/// Detects temporary Windows 11 OS privacy indicator entries (mic / camera / location access badges)
/// that Explorer persists in `NotifyIconSettings` under arbitrary app executables (e.g. browsers).
pub fn is_os_privacy_tooltip(tooltip: &str) -> bool {
    let low = tooltip.trim().to_lowercase();
    if low.is_empty() {
        return false;
    }
    const PRIVACY_PHRASES: [&str; 19] = [
        "открыли доступ к своему микрофону",
        "открыли доступ к своей камере",
        "открыли доступ к своему местоположению",
        "контроля доступа",
        "использует ваш микрофон",
        "использует вашу камеру",
        "использует ваше местоположение",
        "доступ к микрофону",
        "доступ к камере",
        "доступ к местоположению",
        "using your microphone",
        "using your camera",
        "using your location",
        "accessed your microphone",
        "accessed your camera",
        "accessed your location",
        "shared your microphone",
        "shared your camera",
        "click to control access",
    ];
    PRIVACY_PHRASES.iter().any(|p| low.contains(p))
}

/// Identifies web browsers so they are excluded from `^` while already open on the taskbar,
/// while still allowing a background-only browser tray icon if no visible window is open.
pub fn is_browser_exe(base_exe: &str) -> bool {
    let base = exe_basename_lower(base_exe);
    matches!(
        base.as_str(),
        "chrome.exe"
            | "msedge.exe"
            | "firefox.exe"
            | "zen.exe"
            | "brave.exe"
            | "opera.exe"
            | "opera_gx.exe"
            | "vivaldi.exe"
            | "browser.exe"
            | "arc.exe"
            | "waterfox.exe"
            | "librewolf.exe"
            | "floorp.exe"
            | "thorium.exe"
            | "chromium.exe"
            | "iexplore.exe"
    )
}

/// Excludes TestBar itself and Windows OS shell/system hosts from `NotifyIconSettings` tray output
/// while explicitly allowing real user tray utilities like `nvcontainer.exe` (NVIDIA Settings),
/// `securityhealthsystray.exe` (Windows Security), and `magicpodsservice.exe`.
pub fn is_excluded_notify_icon_process(exe_path: &str) -> bool {
    let norm = canonical_tray_exe_key(exe_path);
    if norm.is_empty() {
        return true;
    }
    let base = exe_basename_lower(&norm);
    if base.is_empty() {
        return true;
    }

    // Explicitly allow real Win11 system/GPU tray icons that live under System32 or NvContainer.
    if matches!(base.as_str(), "nvcontainer.exe" | "securityhealthsystray.exe") {
        return false;
    }

    if base.starts_with("testbar")
        || base.starts_with("gamebar")
        || base.starts_with("widgets")
    {
        return true;
    }

    if matches!(
        base.as_str(),
        "explorer.exe"
            | "shellhost.exe"
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
            | "systemsettings.exe"
            | "lockapp.exe"
            | "smartscreen.exe"
            | "msedgewebview2.exe"
            | "backgroundtaskhost.exe"
            | "compkgsrv.exe"
            | "wmiprvse.exe"
            | "audiodg.exe"
            | "monotificationux.exe"
            | "mousocoreworker.exe"
            | "usoclient.exe"
            | "phoneexperiencehost.exe"
            | "crossdeviceresume.exe"
            | "crossdeviceservice.exe"
            | "yourphone.exe"
            | "tabtip.exe"
            | "tabtip32.exe"
            | "msctfmonitor.exe"
            | "credentialuibroker.exe"
            | "useroobebroker.exe"
            | "systemsettingsbroker.exe"
            | "pickerhost.exe"
            | "openwith.exe"
            | "consent.exe"
            | "officeclicktorun.exe"
            | "nvdisplay.container.exe"
    ) {
        return true;
    }

    const EXCLUDED_TOKENS: [&str; 4] = [
        "webview2",
        "crashpad",
        "crashhandler",
        "crashreporter",
    ];
    EXCLUDED_TOKENS.iter().any(|tok| base.contains(tok))
}

/// Cleans a `NotifyIconSettings` `InitialTooltip` to a concise app name (first non-empty line,
/// stripping dynamic `" — status"` / `" - status"` suffixes when the app prefix is >= 3 chars).
pub fn clean_tray_tooltip(initial_tooltip: &str) -> Option<String> {
    let first_line = initial_tooltip
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let mut cleaned = first_line;
    for sep in [" — ", " – ", " - ", " | ", "\t"] {
        if let Some((head, _)) = cleaned.split_once(sep) {
            let head_trimmed = head.trim();
            if head_trimmed.chars().count() >= 3 {
                cleaned = head_trimmed;
                break;
            }
        }
    }
    let cleaned = cleaned.trim().trim_end_matches([':', '.']).trim();
    if cleaned.is_empty()
        || cleaned.chars().count() > 48
        || cleaned.contains('\\')
        || cleaned.contains('/')
    {
        return None;
    }
    Some(cleaned.to_string())
}

fn known_tray_display_name(base_exe: &str) -> Option<&'static str> {
    match base_exe {
        "nvcontainer.exe" => Some("Настройки NVIDIA"),
        "securityhealthsystray.exe" => Some("Безопасность Windows"),
        "vgtray.exe" => Some("Riot Vanguard"),
        "magicpodsservice.exe" => Some("MagicPods"),
        "radeonsoftware.exe" => Some("AMD Software"),
        "wallpaper32.exe" | "wallpaper64.exe" => Some("Wallpaper Engine"),
        "spark desktop.exe" => Some("Spark"),
        "hearthstonedecktracker.exe" => Some("Hearthstone Deck Tracker"),
        "fluentflyout.exe" => Some("FluentFlyout"),
        "battle.net.exe" => Some("Battle.net"),
        _ => None,
    }
}

/// Resolves a clean human-readable tray app name from `InitialTooltip`, known tray utility
/// overrides, or executable stem fallback.
pub fn resolve_tray_display_name(exe_path: &str, initial_tooltip: &str) -> String {
    let base = exe_basename_lower(exe_path);
    if let Some(tip) = clean_tray_tooltip(initial_tooltip) {
        return tip;
    }
    if let Some(known) = known_tray_display_name(&base) {
        return known.to_string();
    }
    friendly_exe_name(exe_path)
}

fn score_notify_icon_entry(
    entry: &RawNotifyIconEntry,
    base_exe: &str,
    exact_path_match: bool,
) -> i32 {
    let mut score = 0i32;
    if exact_path_match {
        score += 50;
    }
    if entry.is_promoted == Some(true) {
        score += 30;
    }
    if let Some(cleaned) = clean_tray_tooltip(&entry.initial_tooltip) {
        score += 25;
        let stem = base_exe.strip_suffix(".exe").unwrap_or(base_exe);
        if stem.len() >= 3 && cleaned.to_lowercase().contains(stem) {
            score += 15;
        }
    }
    if let Some(snap) = entry.icon_snapshot.as_deref() {
        if let Some((w, h)) = parse_png_dimensions(snap) {
            score += if w >= 32 && h >= 32 { 20 } else { 10 };
        }
    }
    score
}

/// Pure Option-A Win11 tray builder intersecting `HKCU\Control Panel\NotifyIconSettings` entries
/// against live current-session processes:
/// - Keeps real tray-resident apps (Steam, Telegram, Battle.net, NVIDIA, Wallpaper Engine,
///   Riot Vanguard, PowerToys, etc.) visible in `^` even when they also have an open window
///   on the taskbar, enriching `hwnd` from the open taskbar group when present.
/// - Excludes OS privacy indicator ghosts (mic/camera/location tooltips), open browsers,
///   own TestBar process, and OS shell hosts.
/// - Deduplicates multi-subkey apps (e.g. Overwolf or upgraded paths) strictly by lowercase
///   executable filename, selecting the highest-scoring live subkey.
pub fn build_tray_apps_from_notify_settings<FResolveIcon>(
    entries: Vec<RawNotifyIconEntry>,
    live_processes: &[LiveProcessEntry],
    open_taskbar_groups: &[AppWindowGroup],
    fallback_hwnds: &HashMap<String, (isize, u32)>,
    extra_open_bases: &HashSet<String>,
    own_pid: u32,
    mut resolve_icon: FResolveIcon,
) -> Vec<TrayAppItem>
where
    FResolveIcon: FnMut(&str, Option<isize>, &str, Option<&[u8]>) -> Option<String>,
{
    let mut open_exe_paths = HashSet::new();
    let mut open_exe_bases: HashSet<String> = extra_open_bases.clone();
    let mut open_hwnd_by_base: HashMap<String, (isize, u32)> = HashMap::new();

    for g in open_taskbar_groups {
        let g_norm = canonical_tray_exe_key(&g.exe_path);
        let g_base = exe_basename_lower(&g_norm);
        if !g_norm.is_empty() {
            open_exe_paths.insert(g_norm);
        }
        if !g_base.is_empty() {
            open_exe_bases.insert(g_base.clone());
        }
        for w in &g.windows {
            let w_norm = canonical_tray_exe_key(&w.exe_path);
            let w_base = if !w_norm.is_empty() {
                open_exe_paths.insert(w_norm.clone());
                exe_basename_lower(&w_norm)
            } else {
                g_base.clone()
            };
            if !w_base.is_empty() {
                open_exe_bases.insert(w_base.clone());
                if w.hwnd != 0 {
                    // Prefer the active window if available, otherwise keep the first valid HWND.
                    if w.is_active || !open_hwnd_by_base.contains_key(&w_base) {
                        open_hwnd_by_base.insert(w_base, (w.hwnd, w.pid));
                    }
                }
            }
        }
    }

    let mut live_by_base: HashMap<String, Vec<&LiveProcessEntry>> = HashMap::new();
    for proc in live_processes {
        if proc.pid == 0 || proc.pid == own_pid {
            continue;
        }
        let base = if !proc.exe_name.trim().is_empty() {
            exe_basename_lower(&proc.exe_name)
        } else {
            exe_basename_lower(&proc.exe_path)
        };
        if !base.is_empty() {
            live_by_base.entry(base).or_default().push(proc);
        }
    }

    struct SelectedEntry {
        entry: RawNotifyIconEntry,
        resolved_exe_path: String,
        score: i32,
        cleaned_tip_len: usize,
    }

    let mut winners: HashMap<String, SelectedEntry> = HashMap::new();

    for entry in entries {
        if entry.executable_path.trim().is_empty() {
            continue;
        }
        if is_os_privacy_tooltip(&entry.initial_tooltip) {
            continue;
        }
        let norm_reg = canonical_tray_exe_key(&entry.executable_path);
        let base_exe = exe_basename_lower(&norm_reg);
        if base_exe.is_empty() || !base_exe.ends_with(".exe") {
            continue;
        }
        let Some(procs) = live_by_base.get(&base_exe) else {
            continue;
        };

        let mut exact_path_match = false;
        let mut live_exe_path: Option<&str> = None;
        for p in procs {
            let p_norm = canonical_tray_exe_key(&p.exe_path);
            if !p_norm.is_empty() {
                if p_norm == norm_reg {
                    exact_path_match = true;
                    live_exe_path = Some(&p.exe_path);
                    break;
                }
                if live_exe_path.is_none() {
                    live_exe_path = Some(&p.exe_path);
                }
            }
        }

        let resolved_exe_path = live_exe_path
            .unwrap_or(&entry.executable_path)
            .trim()
            .to_string();
        if is_excluded_notify_icon_process(&resolved_exe_path) {
            continue;
        }

        let resolved_norm = canonical_tray_exe_key(&resolved_exe_path);
        if is_browser_exe(&base_exe)
            && (open_exe_bases.contains(&base_exe) || open_exe_paths.contains(&resolved_norm))
        {
            continue;
        }

        let score = score_notify_icon_entry(&entry, &base_exe, exact_path_match);
        let cleaned_tip_len = clean_tray_tooltip(&entry.initial_tooltip)
            .map(|s| s.chars().count())
            .unwrap_or(usize::MAX);

        let replace = match winners.get(&base_exe) {
            None => true,
            Some(cur) => {
                score > cur.score
                    || (score == cur.score
                        && (cleaned_tip_len < cur.cleaned_tip_len
                            || (cleaned_tip_len == cur.cleaned_tip_len
                                && entry.subkey < cur.entry.subkey)))
            }
        };
        if replace {
            winners.insert(
                base_exe,
                SelectedEntry {
                    entry,
                    resolved_exe_path,
                    score,
                    cleaned_tip_len,
                },
            );
        }
    }

    let mut items: Vec<TrayAppItem> = winners
        .into_iter()
        .filter_map(|(base_exe, selected)| {
            let procs = live_by_base.get(&base_exe)?;
            let mut all_pids: Vec<u32> = procs.iter().map(|p| p.pid).collect();
            all_pids.sort_unstable();
            all_pids.dedup();
            if all_pids.is_empty() {
                return None;
            }

            let (hwnd, pid) = if let Some(&(h, p)) = open_hwnd_by_base.get(&base_exe) {
                (Some(h), if p != 0 { p } else { all_pids[0] })
            } else if let Some(&(h, p)) = fallback_hwnds.get(&base_exe) {
                (
                    (h != 0).then_some(h),
                    if p != 0 { p } else { all_pids[0] },
                )
            } else {
                (None, all_pids[0])
            };

            let name = resolve_tray_display_name(
                &selected.resolved_exe_path,
                &selected.entry.initial_tooltip,
            );
            let icon_key = resolve_icon(
                &selected.resolved_exe_path,
                hwnd,
                &selected.entry.subkey,
                selected.entry.icon_snapshot.as_deref(),
            );

            Some(TrayAppItem {
                id: base_exe,
                hwnd,
                pid,
                all_pids,
                name,
                exe_path: selected.resolved_exe_path,
                icon_key,
                icon_data: None,
            })
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

fn reg_query_string_value(hkey: HKEY, name: &str) -> Option<String> {
    let wname = wide(name);
    let mut val_type = REG_VALUE_TYPE(0);
    let mut byte_len: u32 = 0;
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(wname.as_ptr()),
            None,
            Some(&mut val_type),
            None,
            Some(&mut byte_len),
        )
    };
    if err != ERROR_SUCCESS || byte_len < 2 || byte_len > 32_768 {
        return None;
    }
    if val_type != REG_SZ && val_type != REG_EXPAND_SZ {
        return None;
    }
    let mut buf = vec![0u8; byte_len as usize];
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(wname.as_ptr()),
            None,
            Some(&mut val_type),
            Some(buf.as_mut_ptr()),
            Some(&mut byte_len),
        )
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    let u16_count = (byte_len as usize) / 2;
    let mut wide_chars = Vec::with_capacity(u16_count);
    for chunk in buf[..u16_count * 2].chunks_exact(2) {
        let code = u16::from_le_bytes([chunk[0], chunk[1]]);
        if code == 0 {
            break;
        }
        wide_chars.push(code);
    }
    let s = String::from_utf16_lossy(&wide_chars);
    let trimmed = s.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn reg_query_dword_value(hkey: HKEY, name: &str) -> Option<u32> {
    let wname = wide(name);
    let mut val_type = REG_VALUE_TYPE(0);
    let mut raw = [0u8; 4];
    let mut byte_len: u32 = 4;
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(wname.as_ptr()),
            None,
            Some(&mut val_type),
            Some(raw.as_mut_ptr()),
            Some(&mut byte_len),
        )
    };
    if err != ERROR_SUCCESS || val_type != REG_DWORD || byte_len < 4 {
        return None;
    }
    Some(u32::from_le_bytes(raw))
}

fn reg_query_binary_value(hkey: HKEY, name: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let wname = wide(name);
    let mut val_type = REG_VALUE_TYPE(0);
    let mut byte_len: u32 = 0;
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(wname.as_ptr()),
            None,
            Some(&mut val_type),
            None,
            Some(&mut byte_len),
        )
    };
    if err != ERROR_SUCCESS || byte_len == 0 || (byte_len as usize) > max_bytes {
        return None;
    }
    if val_type != REG_BINARY {
        return None;
    }
    let mut buf = vec![0u8; byte_len as usize];
    let err = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(wname.as_ptr()),
            None,
            Some(&mut val_type),
            Some(buf.as_mut_ptr()),
            Some(&mut byte_len),
        )
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    buf.truncate(byte_len as usize);
    Some(buf)
}

const NOTIFY_ICON_META_CACHE_TTL: Duration = Duration::from_secs(30);

struct CachedNotifyIconMeta {
    subkey_count: u32,
    last_write_u64: u64,
    refreshed_at: Instant,
    entries: Vec<RawNotifyIconEntry>,
}

static NOTIFY_ICON_META_CACHE: OnceLock<Mutex<Option<CachedNotifyIconMeta>>> = OnceLock::new();

/// Lazily reads a single subkey's `IconSnapshot` (`REG_BINARY`) ONLY on `IconCache` miss
/// (instead of reading ~2MB of PNG blobs across 130+ registry subkeys on every 4.8s poll).
fn read_notify_icon_snapshot_by_subkey(subkey: &str) -> Option<Vec<u8>> {
    if subkey.trim().is_empty() {
        return None;
    }
    let full_path = format!("{NOTIFY_ICON_SETTINGS_SUBKEY}\\{subkey}");
    let full_wide = wide(&full_path);
    let mut sub_hkey = HKEY::default();
    let err = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(full_wide.as_ptr()),
            0,
            KEY_READ,
            &mut sub_hkey,
        )
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    let _guard = RegKeyGuard(sub_hkey);
    reg_query_binary_value(sub_hkey, "IconSnapshot", MAX_NOTIFY_ICON_SNAPSHOT_READ_BYTES)
}

/// Reads Windows 11 tray icon metadata from `HKCU\Control Panel\NotifyIconSettings` using
/// strictly documented Win32 Registry APIs (zero DLL injection, zero `explorer.exe` memory access).
/// Uses `RegQueryInfoKeyW` (`subkey_count` + `ftLastWriteTime`) with a 30s metadata cache and
/// defers `IconSnapshot` binary reads to `IconCache` misses so steady-state polls do 1 syscall
/// and allocate zero PNG buffers.
fn read_notify_icon_settings_registry() -> Option<Vec<RawNotifyIconEntry>> {
    let root_wide = wide(NOTIFY_ICON_SETTINGS_SUBKEY);
    let mut root_hkey = HKEY::default();
    let open_err = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(root_wide.as_ptr()),
            0,
            KEY_READ,
            &mut root_hkey,
        )
    };
    if open_err != ERROR_SUCCESS {
        return None;
    }
    let _root_guard = RegKeyGuard(root_hkey);

    let mut subkey_count: u32 = 0;
    let mut last_write = FILETIME::default();
    let info_ok = unsafe {
        RegQueryInfoKeyW(
            root_hkey,
            PWSTR::null(),
            None,
            None,
            Some(&mut subkey_count),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&mut last_write),
        ) == ERROR_SUCCESS
    };
    let last_write_u64 =
        ((last_write.dwHighDateTime as u64) << 32) | (last_write.dwLowDateTime as u64);

    let cache = NOTIFY_ICON_META_CACHE.get_or_init(|| Mutex::new(None));
    if info_ok {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref cached) = *guard {
            if cached.subkey_count == subkey_count
                && cached.last_write_u64 == last_write_u64
                && cached.refreshed_at.elapsed() < NOTIFY_ICON_META_CACHE_TTL
            {
                return Some(cached.entries.clone());
            }
        }
    }

    let mut entries = Vec::with_capacity(subkey_count as usize);
    let mut index: u32 = 0;
    let mut name_buf = [0u16; 260];

    loop {
        let mut name_len = name_buf.len() as u32;
        let enum_err = unsafe {
            RegEnumKeyExW(
                root_hkey,
                index,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                PWSTR::null(),
                None,
                None,
            )
        };
        if enum_err != ERROR_SUCCESS {
            break;
        }
        index = index.saturating_add(1);

        let subkey = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        let subkey_wide = wide(&subkey);
        let mut sub_hkey = HKEY::default();
        let sub_err = unsafe {
            RegOpenKeyExW(
                root_hkey,
                PCWSTR(subkey_wide.as_ptr()),
                0,
                KEY_READ,
                &mut sub_hkey,
            )
        };
        if sub_err != ERROR_SUCCESS {
            continue;
        }
        let _sub_guard = RegKeyGuard(sub_hkey);

        let Some(raw_exe) = reg_query_string_value(sub_hkey, "ExecutablePath") else {
            continue;
        };
        let executable_path = resolve_known_folder_path_live(&raw_exe);
        let initial_tooltip = reg_query_string_value(sub_hkey, "InitialTooltip").unwrap_or_default();
        let publisher = reg_query_string_value(sub_hkey, "Publisher").unwrap_or_default();
        let is_promoted = reg_query_dword_value(sub_hkey, "IsPromoted").map(|v| v != 0);

        entries.push(RawNotifyIconEntry {
            subkey,
            executable_path,
            initial_tooltip,
            publisher,
            is_promoted,
            // Loaded lazily ONLY on IconCache miss!
            icon_snapshot: None,
        });
    }

    if info_ok {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(CachedNotifyIconMeta {
            subkey_count,
            last_write_u64,
            refreshed_at: Instant::now(),
            entries: entries.clone(),
        });
    }

    Some(entries)
}

#[link(name = "kernel32")]
extern "system" {
    fn ProcessIdToSessionId(dw_process_id: u32, p_session_id: *mut u32) -> i32;
}

fn process_session_id(pid: u32) -> Option<u32> {
    let mut sid: u32 = 0;
    let ok = unsafe { ProcessIdToSessionId(pid, &mut sid) };
    (ok != 0).then_some(sid)
}

/// Enumerates live processes in the current interactive user session whose lowercase executable
/// basename appears in `candidate_bases` (fast Toolhelp32 scan; queries full image path at most
/// ONCE per unique executable basename to avoid 45x OpenProcess churn on multi-process apps).
fn enumerate_live_session_processes(
    own_pid: u32,
    candidate_bases: &HashSet<String>,
) -> Vec<LiveProcessEntry> {
    if candidate_bases.is_empty() {
        return Vec::new();
    }

    let my_session = process_session_id(own_pid);

    let snap = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(h) if !h.is_invalid() => h,
        _ => return Vec::new(),
    };
    let _snap_guard = SnapshotHandleGuard(snap);

    let mut pe = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snap, &mut pe) }.is_err() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut path_by_base: HashMap<String, String> = HashMap::new();
    loop {
        let pid = pe.th32ProcessID;
        if pid != 0 && pid != own_pid {
            let nul_pos = pe
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(pe.szExeFile.len());
            let exe_name = String::from_utf16_lossy(&pe.szExeFile[..nul_pos]);
            let base_low = exe_name.trim().to_ascii_lowercase();

            if candidate_bases.contains(&base_low) {
                let same_session = match (my_session, process_session_id(pid)) {
                    (Some(mine), Some(theirs)) => theirs == mine,
                    // SYSTEM / Session-0 services (e.g. OfficeClickToRun, NVDisplay.Container)
                    // fail ProcessIdToSessionId from a Medium-IL user process; exclude them so
                    // only real interactive-session user tray apps appear.
                    (Some(_), None) => false,
                    (None, _) => true,
                };

                if same_session {
                    let exe_path = if let Some(cached_path) = path_by_base.get(&base_low) {
                        cached_path.clone()
                    } else {
                        let resolved = process_exe_path(pid).unwrap_or_default();
                        if !resolved.is_empty() {
                            path_by_base.insert(base_low, resolved.clone());
                        }
                        resolved
                    };
                    result.push(LiveProcessEntry {
                        pid,
                        exe_path,
                        exe_name,
                    });
                }
            }
        }

        if unsafe { Process32NextW(snap, &mut pe) }.is_err() {
            break;
        }
    }

    result
}

/// Fast single-pass PID-filtered `EnumWindows` inspecting ONLY windows owned by matched live
/// tray processes (skips all other PIDs immediately after `GetWindowThreadProcessId`).
/// Returns `(best_hwnd_by_base, visible_main_window_bases)`.
fn enumerate_best_hwnds_for_tray_pids(
    pid_to_base: &HashMap<u32, String>,
    own_pid: u32,
) -> (HashMap<String, (isize, u32)>, HashSet<String>) {
    if pid_to_base.is_empty() {
        return (HashMap::new(), HashSet::new());
    }

    struct FastCtx<'a> {
        own_pid: u32,
        pid_to_base: &'a HashMap<u32, String>,
        best: HashMap<String, (i32, isize, u32)>,
        visible_main_bases: HashSet<String>,
    }

    extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let ctx = &mut *(lparam.0 as *mut FastCtx<'_>);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == ctx.own_pid {
                return BOOL(1);
            }
            let Some(base_exe) = ctx.pid_to_base.get(&pid) else {
                return BOOL(1);
            };

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

            if is_visible && !is_tool_window && !has_owner && !title.trim().is_empty() {
                ctx.visible_main_bases.insert(base_exe.clone());
            }

            let cand = RawTrayWindowCandidate {
                hwnd: hwnd.0 as isize,
                pid,
                exe_path: String::new(),
                title,
                class_name,
                is_visible,
                is_tool_window,
                has_owner,
                has_class_icon,
                has_app_frame_style,
            };
            let mut score = score_tray_candidate(&cand);
            if is_visible && !is_tool_window && !has_owner {
                score += 20;
            }
            match ctx.best.get_mut(base_exe) {
                Some(entry) => {
                    if score > entry.0 {
                        *entry = (score, hwnd.0 as isize, pid);
                    }
                }
                None => {
                    ctx.best
                        .insert(base_exe.clone(), (score, hwnd.0 as isize, pid));
                }
            }
        }
        BOOL(1)
    }

    let mut ctx = FastCtx {
        own_pid,
        pid_to_base,
        best: HashMap::new(),
        visible_main_bases: HashSet::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }

    let best_hwnds = ctx
        .best
        .into_iter()
        .map(|(k, (_, h, p))| (k, (h, p)))
        .collect();
    (best_hwnds, ctx.visible_main_bases)
}

fn try_put_tray_snapshot_into_cache(
    exe_path: &str,
    subkey: &str,
    snapshot: Option<&[u8]>,
    cache: &mut IconCache,
    icon_sources: &mut Vec<(String, String)>,
    seen_sources: &mut HashSet<String>,
) -> Option<String> {
    let bytes = snapshot?;
    if bytes.len() > MAX_NOTIFY_ICON_SNAPSHOT_CACHE_BYTES || parse_png_dimensions(bytes).is_none() {
        return None;
    }
    let base = exe_basename_lower(exe_path);
    let key = tray_snapshot_icon_key(subkey, &base);
    let trimmed = exe_path.trim();
    if !trimmed.is_empty() && seen_sources.insert(key.clone()) {
        icon_sources.push((key.clone(), trimmed.to_string()));
    }
    if cache.get(&key).is_some() {
        return Some(key);
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    cache.put(key.clone(), format!("data:image/png;base64,{encoded}"));
    Some(key)
}

fn try_put_tray_snapshot_into_cache_mutex(
    exe_path: &str,
    subkey: &str,
    snapshot: Option<&[u8]>,
    cache: &Mutex<IconCache>,
    icon_sources: &mut Vec<(String, String)>,
    seen_sources: &mut HashSet<String>,
) -> Option<String> {
    let bytes = snapshot?;
    if bytes.len() > MAX_NOTIFY_ICON_SNAPSHOT_CACHE_BYTES || parse_png_dimensions(bytes).is_none() {
        return None;
    }
    let base = exe_basename_lower(exe_path);
    let key = tray_snapshot_icon_key(subkey, &base);
    let trimmed = exe_path.trim();
    if !trimmed.is_empty() && seen_sources.insert(key.clone()) {
        icon_sources.push((key.clone(), trimmed.to_string()));
    }
    let hit = {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        guard.get(&key).is_some()
    };
    if hit {
        return Some(key);
    }
    // Base64 encode outside the shared IconCache mutex.
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let uri = format!("data:image/png;base64,{encoded}");
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    guard.put(key.clone(), uri);
    Some(key)
}

fn resolve_notify_tray_icon_into_cache(
    exe_path: &str,
    hwnd_raw: Option<isize>,
    subkey: &str,
    snapshot: Option<&[u8]>,
    cache: &mut IconCache,
    icon_sources: &mut Vec<(String, String)>,
    seen_sources: &mut HashSet<String>,
) -> Option<String> {
    let trimmed = exe_path.trim();
    let base = exe_basename_lower(trimmed);
    let tray_key = tray_snapshot_icon_key(subkey, &base);
    if cache.get(&tray_key).is_some() {
        if !trimmed.is_empty() && seen_sources.insert(tray_key.clone()) {
            icon_sources.push((tray_key.clone(), trimmed.to_string()));
        }
        return Some(tray_key);
    }
    if !trimmed.is_empty() && base != "nvcontainer.exe" && !is_application_frame_host(trimmed) {
        let exe_key = icon_cache::path_icon_key(trimmed);
        if cache.get(&exe_key).is_some() {
            if seen_sources.insert(exe_key.clone()) {
                icon_sources.push((exe_key.clone(), trimmed.to_string()));
            }
            return Some(exe_key);
        }
    }

    // Cold cache miss: lazily load IconSnapshot from HKCU ONLY for this subkey.
    let owned_snap = if snapshot.is_none() {
        read_notify_icon_snapshot_by_subkey(subkey)
    } else {
        None
    };
    let effective_snap = snapshot.or(owned_snap.as_deref());
    let snap_dims = effective_snap.and_then(parse_png_dimensions);
    if should_prefer_icon_snapshot(exe_path, snap_dims) {
        try_put_tray_snapshot_into_cache(
            exe_path,
            subkey,
            effective_snap,
            cache,
            icon_sources,
            seen_sources,
        )
        .or_else(|| {
            resolve_tray_icon_into_cache(exe_path, hwnd_raw, cache, icon_sources, seen_sources)
        })
    } else {
        resolve_tray_icon_into_cache(exe_path, hwnd_raw, cache, icon_sources, seen_sources)
            .or_else(|| {
                try_put_tray_snapshot_into_cache(
                    exe_path,
                    subkey,
                    effective_snap,
                    cache,
                    icon_sources,
                    seen_sources,
                )
            })
    }
}

fn resolve_notify_tray_icon_into_cache_mutex(
    exe_path: &str,
    hwnd_raw: Option<isize>,
    subkey: &str,
    snapshot: Option<&[u8]>,
    cache: &Mutex<IconCache>,
    icon_sources: &mut Vec<(String, String)>,
    seen_sources: &mut HashSet<String>,
) -> Option<String> {
    let trimmed = exe_path.trim();
    let base = exe_basename_lower(trimmed);
    let tray_key = tray_snapshot_icon_key(subkey, &base);
    let exe_key = (!trimmed.is_empty()
        && base != "nvcontainer.exe"
        && !is_application_frame_host(trimmed))
    .then(|| icon_cache::path_icon_key(trimmed));

    // Fast-path cache check: 100% hit on steady-state polls -> ZERO registry I/O, ZERO GDI!
    let cached_hit = {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if guard.get(&tray_key).is_some() {
            Some(tray_key)
        } else if let Some(ref ek) = exe_key {
            if guard.get(ek).is_some() {
                Some(ek.clone())
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some(hit_key) = cached_hit {
        if !trimmed.is_empty() && seen_sources.insert(hit_key.clone()) {
            icon_sources.push((hit_key.clone(), trimmed.to_string()));
        }
        return Some(hit_key);
    }

    // Cold cache miss: lazily load IconSnapshot outside the IconCache mutex.
    let owned_snap = if snapshot.is_none() {
        read_notify_icon_snapshot_by_subkey(subkey)
    } else {
        None
    };
    let effective_snap = snapshot.or(owned_snap.as_deref());
    let snap_dims = effective_snap.and_then(parse_png_dimensions);
    if should_prefer_icon_snapshot(exe_path, snap_dims) {
        try_put_tray_snapshot_into_cache_mutex(
            exe_path,
            subkey,
            effective_snap,
            cache,
            icon_sources,
            seen_sources,
        )
        .or_else(|| {
            resolve_tray_icon_into_cache_mutex(
                exe_path,
                hwnd_raw,
                cache,
                icon_sources,
                seen_sources,
            )
        })
    } else {
        resolve_tray_icon_into_cache_mutex(exe_path, hwnd_raw, cache, icon_sources, seen_sources)
            .or_else(|| {
                try_put_tray_snapshot_into_cache_mutex(
                    exe_path,
                    subkey,
                    effective_snap,
                    cache,
                    icon_sources,
                    seen_sources,
                )
            })
    }
}

fn collect_live_notify_tray_inputs(
    own_pid: u32,
    open_taskbar_groups: &[AppWindowGroup],
) -> Option<(
    Vec<RawNotifyIconEntry>,
    Vec<LiveProcessEntry>,
    HashMap<String, (isize, u32)>,
    HashSet<String>,
)> {
    let entries = read_notify_icon_settings_registry()?;
    let candidate_bases: HashSet<String> = entries
        .iter()
        .filter(|e| !is_os_privacy_tooltip(&e.initial_tooltip))
        .map(|e| exe_basename_lower(&e.executable_path))
        .filter(|b| !b.is_empty() && b.ends_with(".exe"))
        .collect();

    let live_processes = enumerate_live_session_processes(own_pid, &candidate_bases);
    let mut open_bases_with_hwnd = HashSet::new();
    for g in open_taskbar_groups {
        let g_base = exe_basename_lower(&g.exe_path);
        if !g_base.is_empty() && g.windows.iter().any(|w| w.hwnd != 0) {
            open_bases_with_hwnd.insert(g_base);
        }
        for w in &g.windows {
            if w.hwnd != 0 {
                let w_base = exe_basename_lower(&w.exe_path);
                if !w_base.is_empty() {
                    open_bases_with_hwnd.insert(w_base);
                }
            }
        }
    }

    let mut pid_to_base = HashMap::with_capacity(live_processes.len());
    for p in &live_processes {
        let base = if !p.exe_name.trim().is_empty() {
            exe_basename_lower(&p.exe_name)
        } else {
            exe_basename_lower(&p.exe_path)
        };
        if !base.is_empty() && !open_bases_with_hwnd.contains(&base) {
            pid_to_base.insert(p.pid, base);
        }
    }
    let (fallback_hwnds, visible_main_bases) =
        enumerate_best_hwnds_for_tray_pids(&pid_to_base, own_pid);
    Some((entries, live_processes, fallback_hwnds, visible_main_bases))
}

/// Enumerates real Windows 11 tray apps via `HKCU\Control Panel\NotifyIconSettings` intersected
/// with live current-session processes (Option A), falling back to legacy hidden-window scan only
/// if the registry key is unavailable.
pub fn list_tray_background_apps_cached(
    cache: &Mutex<IconCache>,
    open_taskbar_groups: &[AppWindowGroup],
) -> Vec<TrayAppItem> {
    let (items, _) = list_tray_background_apps_filtered_with_open_mutex(cache, open_taskbar_groups, &[]);
    items
}

fn resolve_tray_icon_into_cache_mutex(
    exe_path: &str,
    hwnd_raw: Option<isize>,
    cache: &Mutex<IconCache>,
    icon_sources: &mut Vec<(String, String)>,
    seen_sources: &mut HashSet<String>,
) -> Option<String> {
    let trimmed = exe_path.trim();
    let hwnd = hwnd_raw
        .filter(|&h| h != 0)
        .map(|h| HWND(h as *mut _))
        .unwrap_or_default();

    if !trimmed.is_empty() && !is_application_frame_host(trimmed) {
        let key = icon_cache::path_icon_key(trimmed);
        if seen_sources.insert(key.clone()) {
            icon_sources.push((key.clone(), trimmed.to_string()));
        }
        let (hit, neg) = {
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            if guard.get(&key).is_some() {
                (true, false)
            } else {
                (false, guard.is_negative_cached(&key))
            }
        };
        if hit {
            return Some(key);
        }
        if !neg {
            // Run SHGetFileInfoW / SendMessageTimeoutW outside the shared IconCache mutex
            let extracted =
                extract_exe_icon_data_uri(trimmed).or_else(|| extract_hwnd_icon_data_uri(hwnd));
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(uri) = extracted {
                guard.put(key.clone(), uri);
                return Some(key);
            }
            guard.mark_negative_miss(key);
        }
    }

    if let Some(raw) = hwnd_raw.filter(|&h| h != 0) {
        let key = icon_cache::hwnd_key(raw, 0);
        let (hit, neg) = {
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            if guard.get(&key).is_some() {
                (true, false)
            } else {
                (false, guard.is_negative_cached(&key))
            }
        };
        if hit {
            return Some(key);
        }
        if neg {
            return None;
        }
        let extracted = extract_hwnd_icon_data_uri(HWND(raw as *mut _));
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        match extracted {
            Some(uri) => {
                guard.put(key.clone(), uri);
                return Some(key);
            }
            None => {
                guard.mark_negative_miss(key);
                return None;
            }
        }
    }

    None
}

/// Enumerates real Windows 11 tray apps (`NotifyIconSettings` × live current-session processes)
/// with user ignore-list filtering using narrow `Mutex<IconCache>` locking: registry reads,
/// Toolhelp32 process scan, PID-filtered `EnumWindows`, base64 encoding, and icon extraction
/// all run completely outside the shared `IconCache` mutex.
pub fn list_tray_background_apps_filtered_with_open_mutex(
    cache: &Mutex<IconCache>,
    open_taskbar_groups: &[AppWindowGroup],
    hidden_keys: &[String],
) -> (Vec<TrayAppInfo>, Vec<(String, String)>) {
    let own_pid = unsafe { GetCurrentProcessId() };
    let mut icon_sources = Vec::new();
    let mut seen_sources = HashSet::new();

    if let Some((entries, live_processes, fallback_hwnds, visible_main_bases)) =
        collect_live_notify_tray_inputs(own_pid, open_taskbar_groups)
    {
        let items = build_tray_apps_from_notify_settings(
            entries,
            &live_processes,
            open_taskbar_groups,
            &fallback_hwnds,
            &visible_main_bases,
            own_pid,
            |exe, hwnd, subkey, snap| {
                resolve_notify_tray_icon_into_cache_mutex(
                    exe,
                    hwnd,
                    subkey,
                    snap,
                    cache,
                    &mut icon_sources,
                    &mut seen_sources,
                )
            },
        );
        let filtered = filter_hidden_tray_apps(items, hidden_keys);
        return (filtered, icon_sources);
    }

    // Legacy fallback if HKCU\Control Panel\NotifyIconSettings cannot be opened
    let candidates = enumerate_tray_window_candidates(own_pid);
    let mut extra_open_groups: Vec<AppWindowGroup> = Vec::new();
    let effective_open: &[AppWindowGroup] = if open_taskbar_groups.is_empty() {
        let mut seen_open = HashSet::new();
        for c in &candidates {
            if c.is_visible && !c.is_tool_window && !c.has_owner && !c.title.trim().is_empty() {
                let norm = canonical_tray_exe_key(&c.exe_path);
                if !norm.is_empty() && seen_open.insert(norm.clone()) {
                    extra_open_groups.push(AppWindowGroup {
                        key: norm,
                        name: friendly_exe_name(&c.exe_path),
                        exe_path: c.exe_path.clone(),
                        aumid: None,
                        icon_key: None,
                        icon_data: None,
                        is_active: false,
                        windows: Vec::new(),
                    });
                }
            }
        }
        &extra_open_groups
    } else {
        open_taskbar_groups
    };

    let items = build_tray_background_apps(candidates, effective_open, own_pid, |exe, hwnd| {
        resolve_tray_icon_into_cache_mutex(exe, hwnd, cache, &mut icon_sources, &mut seen_sources)
    });
    let filtered = filter_hidden_tray_apps(items, hidden_keys);
    (filtered, icon_sources)
}

/// Enumerates real Windows 11 tray apps with user ignore-list filtering and returns BOTH the
/// filtered `TrayAppInfo` list AND `(icon_key, exe_path)` icon source pairs so callers can
/// register them in `AppState.icon_sources` (surviving 256-LRU eviction).
pub fn list_tray_background_apps_filtered_with_open(
    cache: &mut IconCache,
    open_taskbar_groups: &[AppWindowGroup],
    hidden_keys: &[String],
) -> (Vec<TrayAppInfo>, Vec<(String, String)>) {
    let own_pid = unsafe { GetCurrentProcessId() };
    let mut icon_sources = Vec::new();
    let mut seen_sources = HashSet::new();

    if let Some((entries, live_processes, fallback_hwnds, visible_main_bases)) =
        collect_live_notify_tray_inputs(own_pid, open_taskbar_groups)
    {
        let items = build_tray_apps_from_notify_settings(
            entries,
            &live_processes,
            open_taskbar_groups,
            &fallback_hwnds,
            &visible_main_bases,
            own_pid,
            |exe, hwnd, subkey, snap| {
                resolve_notify_tray_icon_into_cache(
                    exe,
                    hwnd,
                    subkey,
                    snap,
                    cache,
                    &mut icon_sources,
                    &mut seen_sources,
                )
            },
        );
        let filtered = filter_hidden_tray_apps(items, hidden_keys);
        return (filtered, icon_sources);
    }

    // Legacy fallback if HKCU\Control Panel\NotifyIconSettings cannot be opened
    let candidates = enumerate_tray_window_candidates(own_pid);
    let mut extra_open_groups: Vec<AppWindowGroup> = Vec::new();
    let effective_open: &[AppWindowGroup] = if open_taskbar_groups.is_empty() {
        let mut seen_open = HashSet::new();
        for c in &candidates {
            if c.is_visible && !c.is_tool_window && !c.has_owner && !c.title.trim().is_empty() {
                let norm = canonical_tray_exe_key(&c.exe_path);
                if !norm.is_empty() && seen_open.insert(norm.clone()) {
                    extra_open_groups.push(AppWindowGroup {
                        key: norm,
                        name: friendly_exe_name(&c.exe_path),
                        exe_path: c.exe_path.clone(),
                        aumid: None,
                        icon_key: None,
                        icon_data: None,
                        is_active: false,
                        windows: Vec::new(),
                    });
                }
            }
        }
        &extra_open_groups
    } else {
        open_taskbar_groups
    };

    let items = build_tray_background_apps(candidates, effective_open, own_pid, |exe, hwnd| {
        resolve_tray_icon_into_cache(exe, hwnd, cache, &mut icon_sources, &mut seen_sources)
    });
    let filtered = filter_hidden_tray_apps(items, hidden_keys);
    (filtered, icon_sources)
}

/// Enumerates tray background apps (excluding user `hidden_keys`), populating `cache` and
/// returning `(apps, icon_sources)`.
pub fn list_tray_background_apps_filtered(
    cache: &mut IconCache,
    hidden_keys: &[String],
) -> (Vec<TrayAppInfo>, Vec<(String, String)>) {
    list_tray_background_apps_filtered_with_open(cache, &[], hidden_keys)
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

/// Activates a background tray app by restoring & focusing its existing window when valid
/// (avoiding duplicate app spawns), falling back to a smart shell/UI launch when no restorable
/// window exists (e.g. `windowsdefender:` for SecurityHealthSystray, NVIDIA App/Control Panel
/// for `nvcontainer.exe`, or single-instance relaunch for standard apps).
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

    let base = exe_basename_lower(exe_path);
    if base == "securityhealthsystray.exe" {
        return launch_target_with_args("windowsdefender:", None, false);
    }
    if base == "nvcontainer.exe" {
        const NV_APP_PATH: &str = r"C:\Program Files\NVIDIA Corporation\NVIDIA app\CEF\NVIDIA app.exe";
        if std::path::Path::new(NV_APP_PATH).exists() {
            return launch_target_with_args(NV_APP_PATH, None, false);
        }
        if launch_target_with_args("nvcplui.exe", None, false).is_ok() {
            return Ok(());
        }
    }

    launch_target_with_args(exe_path, None, false)
}

pub fn end_tray_app_task(pids: &[u32]) -> Result<(), String> {
    terminate_process_pids(pids)
}

// ======================= NETWORK, SHELL UTILITIES & POWER =======================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawNetworkAdapter {
    pub is_up: bool,
    pub is_hardware: bool,
    pub is_filter: bool,
    pub if_type: u32,
    pub transmit_link_speed: u64,
}

/// Formats a Windows `MIB_IF_ROW2.TransmitLinkSpeed` value safely, guarding `u64::MAX`
/// (`NDIS_LINK_SPEED_UNKNOWN`) and implausibly huge values so UI never prints `18446744074 Гбит/с`.
pub fn format_link_speed(bps: u64) -> String {
    if bps == 0 || bps == u64::MAX || bps > MAX_PLAUSIBLE_LINK_SPEED_BPS {
        return "Подключено".to_string();
    }
    let mbps = bps / 1_000_000;
    if mbps >= 1000 {
        format!("{} Гбит/с", (mbps as f64 / 1000.0).round() as u64)
    } else if mbps > 0 {
        format!("{mbps} Мбит/с")
    } else {
        "Подключено".to_string()
    }
}

fn known_finite_speed(bps: u64) -> u64 {
    if bps == u64::MAX || bps > MAX_PLAUSIBLE_LINK_SPEED_BPS {
        0
    } else {
        bps
    }
}

/// Pure network selector preferring active hardware Ethernet/Wi-Fi adapters, and falling back
/// cleanly to active non-loopback virtual/VPN/USB-tether/WWAN interfaces when no hardware NIC
/// is reported up.
pub fn select_best_network_info(adapters: &[RawNetworkAdapter]) -> NetworkInfo {
    const IF_TYPE_ETHERNET: u32 = 6;
    const IF_TYPE_PPP: u32 = 23;
    const IF_TYPE_LOOPBACK: u32 = 24;
    const IF_TYPE_PROP_VIRTUAL: u32 = 53;
    const IF_TYPE_WIFI: u32 = 71;
    const IF_TYPE_TUNNEL: u32 = 131;
    const IF_TYPE_WWANPP: u32 = 243;
    const IF_TYPE_WWANPP2: u32 = 244;

    // Pass 1: Hardware Ethernet / Wi-Fi NICs (prefer highest finite known link speed)
    let mut best_hw: Option<&RawNetworkAdapter> = None;
    for a in adapters {
        if !a.is_up || !a.is_hardware || a.is_filter {
            continue;
        }
        if a.if_type == IF_TYPE_ETHERNET || a.if_type == IF_TYPE_WIFI {
            let take = match best_hw {
                None => true,
                Some(b) => known_finite_speed(a.transmit_link_speed) > known_finite_speed(b.transmit_link_speed),
            };
            if take {
                best_hw = Some(a);
            }
        }
    }

    if let Some(row) = best_hw {
        let kind = if row.if_type == IF_TYPE_WIFI {
            "Wi-Fi"
        } else {
            "Ethernet"
        }
        .to_string();
        return NetworkInfo {
            connected: true,
            kind,
            speed_label: format_link_speed(row.transmit_link_speed),
        };
    }

    // Pass 2: Active non-loopback virtual/VPN/tether/cellular fallback
    let mut best_fallback: Option<&RawNetworkAdapter> = None;
    for a in adapters {
        if !a.is_up || a.is_filter || a.if_type == IF_TYPE_LOOPBACK {
            continue;
        }
        let supported = matches!(
            a.if_type,
            IF_TYPE_ETHERNET
                | IF_TYPE_WIFI
                | IF_TYPE_PPP
                | IF_TYPE_PROP_VIRTUAL
                | IF_TYPE_TUNNEL
                | IF_TYPE_WWANPP
                | IF_TYPE_WWANPP2
        );
        if !supported {
            continue;
        }
        let take = match best_fallback {
            None => true,
            Some(b) => {
                let a_is_nic = a.if_type == IF_TYPE_ETHERNET || a.if_type == IF_TYPE_WIFI;
                let b_is_nic = b.if_type == IF_TYPE_ETHERNET || b.if_type == IF_TYPE_WIFI;
                (a_is_nic && !b_is_nic)
                    || (a_is_nic == b_is_nic
                        && known_finite_speed(a.transmit_link_speed)
                            > known_finite_speed(b.transmit_link_speed))
            }
        };
        if take {
            best_fallback = Some(a);
        }
    }

    if let Some(row) = best_fallback {
        let kind = match row.if_type {
            IF_TYPE_WIFI => "Wi-Fi",
            IF_TYPE_ETHERNET => "Ethernet",
            IF_TYPE_TUNNEL | IF_TYPE_PROP_VIRTUAL | IF_TYPE_PPP => "VPN",
            IF_TYPE_WWANPP | IF_TYPE_WWANPP2 => "Сотовая сеть",
            _ => "Сеть",
        }
        .to_string();
        return NetworkInfo {
            connected: true,
            kind,
            speed_label: format_link_speed(row.transmit_link_speed),
        };
    }

    NetworkInfo {
        connected: false,
        kind: "Нет сети".to_string(),
        speed_label: "Офлайн".to_string(),
    }
}

struct MibTableGuard(*mut MIB_IF_TABLE2);

impl Drop for MibTableGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                FreeMibTable(self.0 as *const _);
            }
        }
    }
}

pub fn get_network_info() -> NetworkInfo {
    unsafe {
        let mut table_ptr: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table_ptr).is_err() || table_ptr.is_null() {
            return NetworkInfo {
                connected: true,
                kind: "Сеть".to_string(),
                speed_label: "Подключено".to_string(),
            };
        }
        let _guard = MibTableGuard(table_ptr);
        let table = &*table_ptr;
        let rows: &[MIB_IF_ROW2] =
            std::slice::from_raw_parts(table.Table.as_ptr(), table.NumEntries as usize);

        let adapters: Vec<RawNetworkAdapter> = rows
            .iter()
            .map(|row| {
                let bits = row.InterfaceAndOperStatusFlags._bitfield;
                RawNetworkAdapter {
                    is_up: row.OperStatus == IfOperStatusUp,
                    is_hardware: (bits & 0x01) != 0,
                    is_filter: (bits & 0x02) != 0,
                    if_type: row.Type,
                    transmit_link_speed: row.TransmitLinkSpeed,
                }
            })
            .collect();

        select_best_network_info(&adapters)
    }
}

/// Empties the Windows Recycle Bin cleanly via `SHEmptyRecycleBinW`.
/// Returns `Ok(())` idempotently when the Recycle Bin is already empty.
pub fn empty_recycle_bin() -> Result<(), String> {
    unsafe {
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        if SHQueryRecycleBinW(PCWSTR::null(), &mut info).is_ok() && info.i64NumItems == 0 {
            return Ok(());
        }
        let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
        match SHEmptyRecycleBinW(HWND::default(), PCWSTR::null(), flags) {
            Ok(()) => Ok(()),
            Err(err)
                if err.code() == S_OK
                    || err.code() == S_FALSE
                    || err.code() == E_UNEXPECTED =>
            {
                Ok(())
            }
            Err(err) => Err(format!("Не удалось очистить корзину: {err}")),
        }
    }
}

/// Opens the native Windows 11 Screen Snip overlay (`ms-screenclip:`) via `ShellExecuteW`.
pub fn open_screen_snip() -> Result<(), String> {
    unsafe {
        let verb = wide("open");
        let target = wide("ms-screenclip:");
        let res = ShellExecuteW(
            HWND::default(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOW,
        );
        let code = res.0 as isize;
        if code <= 32 {
            return Err(format!("Не удалось открыть Ножницы (код {code})"));
        }
        Ok(())
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
                is_topmost: false,
                monitor: "primary".to_string(),
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

        // Verify newly added Windows shell/GPU worker hosts are strictly excluded
        for excluded in [
            r"C:\Windows\SystemApps\ShellHost\ShellHost.exe",
            r"C:\Program Files\WindowsApps\SystemSettings.exe",
            r"C:\Program Files\AMD\CNext\AMDRSServ.exe",
            r"C:\Program Files\WindowsApps\Xbox\GameBarPresenceWriter.exe",
            r"C:\Program Files\WindowsApps\Widgets.exe",
            r"C:\Windows\System32\MoUsoCoreWorker.exe",
        ] {
            assert!(
                is_excluded_tray_process(excluded),
                "noisy host {excluded} must be excluded from tray"
            );
        }

        // Verify strict deduplication by lowercase exe filename across DIFFERENT folders
        // AND dropping of headless untitled/iconless worker processes!
        let dup_candidates = vec![
            RawTrayWindowCandidate {
                hwnd: 801,
                pid: 8001,
                exe_path: r"C:\Apps\Foo\bin\MyTray.exe".to_string(),
                title: "".to_string(),
                class_name: "Worker".to_string(),
                is_visible: false,
                is_tool_window: true,
                has_owner: true,
                has_class_icon: false,
                has_app_frame_style: false,
            },
            RawTrayWindowCandidate {
                hwnd: 802,
                pid: 8002,
                exe_path: r"C:\Apps\Foo\v2\mytray.EXE".to_string(),
                title: "MyTray".to_string(),
                class_name: "MainWnd".to_string(),
                is_visible: false,
                is_tool_window: false,
                has_owner: false,
                has_class_icon: true,
                has_app_frame_style: true,
            },
            // Headless iconless blank worker -> dropped when resolve_icon_key returns None
            RawTrayWindowCandidate {
                hwnd: 901,
                pid: 9001,
                exe_path: r"C:\Apps\Silent\silentdaemon.exe".to_string(),
                title: "".to_string(),
                class_name: "HiddenMsg".to_string(),
                is_visible: false,
                is_tool_window: true,
                has_owner: false,
                has_class_icon: false,
                has_app_frame_style: false,
            },
        ];
        let deduped = build_tray_background_apps(dup_candidates, &[], own_pid, |exe, _| {
            if exe_basename_lower(exe) == "mytray.exe" {
                Some("exe:mytray".to_string())
            } else {
                None
            }
        });
        assert_eq!(deduped.len(), 1, "same lowercase exe across dirs must dedup to 1 and blank iconless worker must be dropped");
        assert_eq!(deduped[0].id, "mytray.exe");
        assert_eq!(deduped[0].hwnd, Some(802));
        assert_eq!(deduped[0].all_pids, vec![8001, 8002]);
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
            for input in &win_n {
                assert_eq!(
                    input.Anonymous.ki.dwExtraInfo,
                    crate::win_shell::TESTBAR_INJECTED_EXTRA_INFO,
                    "all Win-chord inputs must carry TESTBAR_INJECTED_EXTRA_INFO"
                );
            }
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

        // Copying 45 new unpinned entries fills vault to 40 and must NEVER evict pinned entries
        assert_eq!(MAX_CLIPBOARD_ENTRIES, 40);
        for i in 0..45 {
            push_clipboard_text_into(&mut vault, &format!("unpinned snippet {i}"));
        }
        assert_eq!(vault.len(), MAX_CLIPBOARD_ENTRIES);
        assert_eq!(vault.iter().filter(|c| c.pinned).count(), 3);
        assert!(vault.iter().any(|c| c.text == "https://github.com/testbar" && c.pinned));
        assert!(vault.iter().any(|c| c.text == "#38BDF8" && c.pinned));
        assert!(vault.iter().any(|c| c.text == r"C:\Projects\testbar" && c.pinned));
        // Most recent unpinned sits directly after pinned entries
        assert_eq!(vault[3].text, "unpinned snippet 44");
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

    #[test]
    fn clipboard_lossless_multiline_classifier_privacy_delete_and_pause() {
        let multiline_code = "pub fn hello(name: &str) -> String {\r\n    let greeting = format!(\"Hello, {name}!\");\r\n    // line 3 extra context ".to_string()
            + &"x".repeat(260)
            + "\r\n    greeting\r\n}";

        let mut vault = Vec::new();
        push_clipboard_text_into(&mut vault, &multiline_code);
        assert_eq!(vault.len(), 1);
        let entry = &vault[0];
        // Full text preserves newlines and length well beyond 180 chars
        assert!(entry.text.contains('\n'));
        assert!(entry.char_count > 300);
        assert_eq!(entry.line_count, 5);
        assert_eq!(entry.kind, "code");
        assert_eq!(entry.tag, "CODE");
        assert!(
            entry.preview.chars().count() <= CLIPBOARD_PREVIEW_MAX_CHARS,
            "preview must be <= 180 chars, got {}",
            entry.preview.chars().count()
        );
        assert!(entry.preview.lines().count() <= 2);

        // Classifier matrix
        assert_eq!(classify_clipboard_content("https://example.com/a?b=1").0, "url");
        assert_eq!(classify_clipboard_content(r"C:\Users\Alex\file.rs").0, "path");
        assert_eq!(classify_clipboard_content(r"\\server\share\docs").0, "path");
        assert_eq!(classify_clipboard_content("#a78bfa").0, "hex");
        assert_eq!(classify_clipboard_content("#38BDF8FF").0, "hex");
        assert_eq!(classify_clipboard_content("{\"ok\": true, \"count\": 3}").0, "code");
        assert_eq!(classify_clipboard_content("SELECT id FROM users WHERE active = 1").0, "code");
        assert_eq!(classify_clipboard_content("git status -sb").0, "code");
        assert_eq!(
            classify_clipboard_content("Привет, это обычная заметка.\nВторая строка текста.").0,
            "text"
        );

        // Privacy format decision matrix
        assert!(should_skip_private_clipboard(true, None));
        assert!(should_skip_private_clipboard(true, Some(1)));
        assert!(should_skip_private_clipboard(false, Some(0)));
        assert!(!should_skip_private_clipboard(false, Some(1)));
        assert!(!should_skip_private_clipboard(false, None));

        // Delete entry
        let id = entry.id.clone();
        assert!(delete_clipboard_entry_in(&mut vault, &id));
        assert!(vault.is_empty());
        assert!(!delete_clipboard_entry_in(&mut vault, "nonexistent"));

        // Pause toggle + Ctrl+V input batch
        assert!(set_clipboard_paused(true));
        assert!(is_clipboard_paused());
        assert!(!set_clipboard_paused(false));
        assert!(!is_clipboard_paused());

        let ctrl_v = build_ctrl_v_inputs();
        assert_eq!(ctrl_v.len(), 4);
        unsafe {
            assert_eq!(ctrl_v[0].Anonymous.ki.wVk, VK_CONTROL);
            assert_eq!(ctrl_v[1].Anonymous.ki.wVk, VIRTUAL_KEY(b'V' as u16));
            assert_eq!(ctrl_v[2].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
            assert_eq!(ctrl_v[3].Anonymous.ki.wVk, VK_CONTROL);
            assert_eq!(ctrl_v[3].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
        }
    }

    #[test]
    fn network_info_guards_unknown_link_speed_and_falls_back_to_vpn_virtual() {
        assert_eq!(format_link_speed(u64::MAX), "Подключено");
        assert_eq!(format_link_speed(0), "Подключено");
        assert_eq!(format_link_speed(100_000_000), "100 Мбит/с");
        assert_eq!(format_link_speed(1_000_000_000), "1 Гбит/с");
        assert_eq!(format_link_speed(2_500_000_000), "3 Гбит/с");

        // Hardware NIC with NDIS_LINK_SPEED_UNKNOWN (u64::MAX) must NOT beat real 1Gbps NIC or format overflow
        let hw_unknown = select_best_network_info(&[
            RawNetworkAdapter {
                is_up: true,
                is_hardware: true,
                is_filter: false,
                if_type: 6,
                transmit_link_speed: u64::MAX,
            },
            RawNetworkAdapter {
                is_up: true,
                is_hardware: true,
                is_filter: false,
                if_type: 71,
                transmit_link_speed: 866_000_000,
            },
        ]);
        assert!(hw_unknown.connected);
        assert_eq!(hw_unknown.kind, "Wi-Fi");
        assert_eq!(hw_unknown.speed_label, "866 Мбит/с");

        // Single hardware NIC reporting u64::MAX falls back cleanly to "Подключено"
        let single_unknown = select_best_network_info(&[RawNetworkAdapter {
            is_up: true,
            is_hardware: true,
            is_filter: false,
            if_type: 6,
            transmit_link_speed: u64::MAX,
        }]);
        assert!(single_unknown.connected);
        assert_eq!(single_unknown.speed_label, "Подключено");

        // No hardware NIC up, but WireGuard/VPN tunnel is up -> connected VPN (not offline!)
        let vpn_only = select_best_network_info(&[
            RawNetworkAdapter {
                is_up: true,
                is_hardware: false,
                is_filter: false,
                if_type: 24, // loopback must be ignored
                transmit_link_speed: 10_000_000_000,
            },
            RawNetworkAdapter {
                is_up: true,
                is_hardware: false,
                is_filter: false,
                if_type: 53, // proprietary virtual / VPN
                transmit_link_speed: 100_000_000,
            },
        ]);
        assert!(vpn_only.connected);
        assert_eq!(vpn_only.kind, "VPN");
        assert_eq!(vpn_only.speed_label, "100 Мбит/с");

        // Only loopback up -> offline
        let offline = select_best_network_info(&[RawNetworkAdapter {
            is_up: true,
            is_hardware: false,
            is_filter: false,
            if_type: 24,
            transmit_link_speed: 1_000_000_000,
        }]);
        assert!(!offline.connected);
        assert_eq!(offline.kind, "Нет сети");
    }

    #[test]
    fn tray_hidden_filter_and_live_bluetooth_probe_safe() {
        let items = vec![
            TrayAppInfo {
                id: r"c:\apps\telegram\telegram.exe".into(),
                hwnd: Some(10),
                pid: 100,
                all_pids: vec![100],
                name: "Telegram".into(),
                exe_path: r"C:\Apps\Telegram\Telegram.exe".into(),
                icon_key: Some("exe:tg".into()),
                icon_data: None,
            },
            TrayAppInfo {
                id: r"c:\apps\steam\steam.exe".into(),
                hwnd: Some(20),
                pid: 200,
                all_pids: vec![200],
                name: "Steam".into(),
                exe_path: r"C:\Apps\Steam\Steam.exe".into(),
                icon_key: Some("exe:st".into()),
                icon_data: None,
            },
        ];

        let filtered = filter_hidden_tray_apps(items, &["TELEGRAM.EXE".into()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Steam");

        // Live Bluetooth battery probe runs with aligned buffer + BTH/BTHLE enumerators without UB
        let _bats = get_peripheral_batteries();
    }

    #[test]
    fn paste_clipboard_guards_missing_target_and_verifies_external_foreground() {
        // 1. resolved_hwnd == 0 MUST copy text as safe fallback and return Err WITHOUT calling SendInput
        let mut wrote = String::new();
        let mut focus_called = false;
        let mut send_called = false;
        let res_no_target = paste_clipboard_text_with_ops(
            0,
            0,
            "secret snippet",
            |t| {
                wrote = t.to_string();
                Ok(())
            },
            |_| false,
            |_| focus_called = true,
            || true,
            || {
                send_called = true;
                Ok(())
            },
        );
        assert!(res_no_target.is_err());
        assert_eq!(wrote, "secret snippet");
        assert!(!focus_called);
        assert!(!send_called, "blind Ctrl+V must never be injected when resolved_hwnd == 0");

        // 2. Foreground never leaves TestBar -> abort WITHOUT calling SendInput
        let mut focused_hwnd = 0isize;
        let mut send_called_timeout = false;
        let res_fg_timeout = paste_clipboard_text_with_ops(
            0,
            4242,
            "hello",
            |_| Ok(()),
            |h| h == 4242,
            |h| focused_hwnd = h,
            || wait_for_external_foreground(3, 0, || false),
            || {
                send_called_timeout = true;
                Ok(())
            },
        );
        assert!(res_fg_timeout.is_err());
        assert_eq!(focused_hwnd, 4242);
        assert!(
            !send_called_timeout,
            "Ctrl+V must never be injected if foreground stays on TestBar"
        );

        // 3. Valid target + foreground switches to external process on 3rd poll -> SendInput runs once
        let mut polls = 0usize;
        let mut send_count = 0usize;
        let res_ok = paste_clipboard_text_with_ops(
            9001,
            4242,
            "paste me",
            |_| Ok(()),
            |h| h == 9001,
            |h| focused_hwnd = h,
            || {
                wait_for_external_foreground(5, 0, || {
                    polls += 1;
                    polls >= 3
                })
            },
            || {
                send_count += 1;
                Ok(())
            },
        );
        assert!(res_ok.is_ok());
        assert_eq!(focused_hwnd, 9001);
        assert_eq!(polls, 3);
        assert_eq!(send_count, 1);

        // 4. Empty text rejects immediately without touching clipboard or SendInput
        let mut write_called = false;
        let res_empty = paste_clipboard_text_with_ops(
            9001,
            0,
            "",
            |_| {
                write_called = true;
                Ok(())
            },
            |_| true,
            |_| {},
            || true,
            || Ok(()),
        );
        assert!(res_empty.is_err());
        assert!(!write_called);
    }

    fn make_fake_png(width: u32, height: u32) -> Vec<u8> {
        let mut png = Vec::with_capacity(32);
        png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0]);
        png
    }

    #[test]
    fn notify_icon_known_folder_and_png_snapshot_resolution() {
        let raw = r"{6D809377-6AF0-444B-8957-A3773F02200E}\NVIDIA Corporation\NvContainer\nvcontainer.exe";
        let (guid, rest) = split_known_folder_prefix(raw).expect("GUID prefix must split");
        assert_eq!(guid, "{6D809377-6AF0-444B-8957-A3773F02200E}");
        assert_eq!(rest, r"NVIDIA Corporation\NvContainer\nvcontainer.exe");
        assert!(parse_guid_str(guid).is_some());

        let resolved = resolve_known_folder_path_with(raw, |g| {
            assert_eq!(g, "{6D809377-6AF0-444B-8957-A3773F02200E}");
            Some(r"C:\Program Files".to_string())
        });
        assert_eq!(
            resolved,
            r"C:\Program Files\NVIDIA Corporation\NvContainer\nvcontainer.exe"
        );

        // Plain path stays untouched (slashes normalized)
        assert_eq!(
            resolve_known_folder_path_with("C:/Apps/Steam/steam.exe", |_| None),
            r"C:\Apps\Steam\steam.exe"
        );

        // PNG IHDR validation & snapshot preference matrix
        let png16 = make_fake_png(16, 16);
        let png32 = make_fake_png(32, 32);
        let png128 = make_fake_png(128, 128);
        let png512 = make_fake_png(512, 512);
        assert_eq!(parse_png_dimensions(&png32), Some((32, 32)));
        assert_eq!(parse_png_dimensions(b"not a png"), None);
        assert_eq!(parse_png_dimensions(&make_fake_png(0, 32)), None);

        // nvcontainer always prefers IconSnapshot (even at 16 or 128px)
        assert!(should_prefer_icon_snapshot(
            r"C:\Program Files\NVIDIA Corporation\NvContainer\nvcontainer.exe",
            parse_png_dimensions(&png128)
        ));
        // WindowsApps prefers IconSnapshot <= 256px
        assert!(should_prefer_icon_snapshot(
            r"C:\Program Files\WindowsApps\FluentFlyout\FluentFlyout.exe",
            parse_png_dimensions(&png16)
        ));
        // Desktop 32px native tray icon prefers IconSnapshot
        assert!(should_prefer_icon_snapshot(
            r"C:\Program Files (x86)\Steam\steam.exe",
            parse_png_dimensions(&png32)
        ));
        // Desktop 16px tiny snapshot prefers crisp exe icon extraction first
        assert!(!should_prefer_icon_snapshot(
            r"C:\Apps\Telegram\Telegram.exe",
            parse_png_dimensions(&png16)
        ));
        // Oversized 512px snapshot (e.g. Antigravity) prefers crisp exe icon extraction first
        assert!(!should_prefer_icon_snapshot(
            r"C:\Users\u\AppData\Local\Programs\Antigravity\Antigravity.exe",
            parse_png_dimensions(&png512)
        ));

        // Deterministic tray: cache key
        let k1 = tray_snapshot_icon_key("1234567890", "nvcontainer.exe");
        let k2 = tray_snapshot_icon_key("1234567890", "NVCONTAINER.EXE");
        assert!(k1.starts_with("tray:"));
        assert_eq!(k1, k2);
    }

    #[test]
    fn notify_icon_privacy_tooltip_browser_and_exclusion_filters() {
        // Live Windows 11 Russian & English OS privacy indicator tooltips MUST be filtered
        assert!(is_os_privacy_tooltip(
            "Вы открыли доступ к своему микрофону. Щёлкните для контроля доступа."
        ));
        assert!(is_os_privacy_tooltip(
            "Discord is using your microphone. Click to control access."
        ));
        assert!(is_os_privacy_tooltip("Zoom accessed your camera"));
        // Normal tray tooltips must NOT be misclassified
        assert!(!is_os_privacy_tooltip("Настройки NVIDIA"));
        assert!(!is_os_privacy_tooltip("Telegram Desktop"));
        assert!(!is_os_privacy_tooltip("Безопасность Windows — Действия не требуются"));

        // Tooltip cleanup & friendly overrides
        assert_eq!(
            resolve_tray_display_name(
                r"C:\Windows\System32\SecurityHealthSystray.exe",
                "Безопасность Windows — Действия не требуются"
            ),
            "Безопасность Windows"
        );
        assert_eq!(
            resolve_tray_display_name(r"C:\Program Files\Riot Vanguard\vgtray.exe", ""),
            "Riot Vanguard"
        );
        assert_eq!(
            resolve_tray_display_name(r"C:\Apps\MagicPods\MagicPodsService.exe", ""),
            "MagicPods"
        );
        assert_eq!(
            resolve_tray_display_name(
                r"C:\Program Files\NVIDIA Corporation\NvContainer\nvcontainer.exe",
                "Настройки NVIDIA"
            ),
            "Настройки NVIDIA"
        );

        // Browsers detected
        for b in ["chrome.exe", "zen.exe", "msedge.exe", "firefox.exe", "brave.exe"] {
            assert!(is_browser_exe(b), "{b} must be detected as browser");
        }
        assert!(!is_browser_exe("steam.exe"));
        assert!(!is_browser_exe("telegram.exe"));

        // NotifyIconSettings exclusion matrix: allows real tray daemons, blocks OS/self
        assert!(!is_excluded_notify_icon_process(
            r"C:\Program Files\NVIDIA Corporation\NvContainer\nvcontainer.exe"
        ));
        assert!(!is_excluded_notify_icon_process(
            r"C:\Windows\System32\SecurityHealthSystray.exe"
        ));
        assert!(!is_excluded_notify_icon_process(
            r"C:\Apps\MagicPods\MagicPodsService.exe"
        ));
        for excl in [
            r"C:\Windows\explorer.exe",
            r"C:\Apps\TestBar\testbar-v2.exe",
            r"C:\Windows\System32\MoNotificationUx.exe",
            r"C:\Program Files\WindowsApps\PhoneExperienceHost.exe",
        ] {
            assert!(
                is_excluded_notify_icon_process(excl),
                "{excl} must be excluded from NotifyIconSettings tray"
            );
        }
    }

    #[test]
    fn notify_icon_option_a_keeps_open_tray_apps_excludes_open_browsers_and_dedups_overwolf() {
        let own_pid = 777;
        let png32 = make_fake_png(32, 32);
        let png128 = make_fake_png(128, 128);

        let open_groups = vec![
            make_open_group(r"C:\Program Files (x86)\Steam\steam.exe", 2001),
            make_open_group(r"C:\Program Files\Zen Browser\zen.exe", 3001),
            make_open_group(r"C:\Program Files\Google\Chrome\Application\chrome.exe", 3002),
        ];

        let live_processes = vec![
            LiveProcessEntry {
                pid: own_pid,
                exe_path: r"C:\Apps\TestBar\testbar-v2.exe".into(),
                exe_name: "testbar-v2.exe".into(),
            },
            LiveProcessEntry {
                pid: 2001,
                exe_path: r"C:\Program Files (x86)\Steam\steam.exe".into(),
                exe_name: "steam.exe".into(),
            },
            LiveProcessEntry {
                pid: 2002,
                exe_path: r"C:\Program Files\NVIDIA Corporation\NvContainer\nvcontainer.exe".into(),
                exe_name: "nvcontainer.exe".into(),
            },
            LiveProcessEntry {
                pid: 2003,
                exe_path: String::new(), // Simulates protected anti-cheat process where QueryFullProcessImageNameW failed
                exe_name: "vgtray.exe".into(),
            },
            LiveProcessEntry {
                pid: 2004,
                exe_path: r"C:\Program Files (x86)\Overwolf\0.264\Overwolf.exe".into(),
                exe_name: "Overwolf.exe".into(),
            },
            LiveProcessEntry {
                pid: 2005,
                exe_path: r"C:\Program Files (x86)\Overwolf\0.264\Overwolf.exe".into(),
                exe_name: "Overwolf.exe".into(),
            },
            LiveProcessEntry {
                pid: 3001,
                exe_path: r"C:\Program Files\Zen Browser\zen.exe".into(),
                exe_name: "zen.exe".into(),
            },
            LiveProcessEntry {
                pid: 3002,
                exe_path: r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
                exe_name: "chrome.exe".into(),
            },
            LiveProcessEntry {
                pid: 3003,
                exe_path: r"C:\Program Files\Mozilla Firefox\firefox.exe".into(),
                exe_name: "firefox.exe".into(),
            },
            LiveProcessEntry {
                pid: 4000,
                exe_path: r"C:\Windows\explorer.exe".into(),
                exe_name: "explorer.exe".into(),
            },
        ];

        let entries = vec![
            // Open Steam -> MUST stay in tray (Option A) with HWND enriched from open_groups (999)
            RawNotifyIconEntry {
                subkey: "100".into(),
                executable_path: r"C:\Program Files (x86)\Steam\steam.exe".into(),
                initial_tooltip: "Steam".into(),
                publisher: "Valve".into(),
                is_promoted: Some(false),
                icon_snapshot: Some(png32.clone()),
            },
            // NVIDIA Container -> MUST be kept with 128px snapshot & localized tooltip
            RawNotifyIconEntry {
                subkey: "101".into(),
                executable_path: r"C:\Program Files\NVIDIA Corporation\NvContainer\nvcontainer.exe".into(),
                initial_tooltip: "Настройки NVIDIA".into(),
                publisher: "NVIDIA".into(),
                is_promoted: Some(true),
                icon_snapshot: Some(png128),
            },
            // Riot Vanguard (empty live exe_path due to anti-cheat DACL) -> falls back to registry path cleanly!
            RawNotifyIconEntry {
                subkey: "102".into(),
                executable_path: r"C:\Program Files\Riot Vanguard\vgtray.exe".into(),
                initial_tooltip: String::new(),
                publisher: "Riot Games".into(),
                is_promoted: Some(false),
                icon_snapshot: Some(png32.clone()),
            },
            // Multi-subkey Overwolf: old version vs current version -> dedups to 1 winning entry with both PIDs
            RawNotifyIconEntry {
                subkey: "103".into(),
                executable_path: r"C:\Program Files (x86)\Overwolf\0.250\Overwolf.exe".into(),
                initial_tooltip: String::new(),
                publisher: "Overwolf".into(),
                is_promoted: Some(false),
                icon_snapshot: None,
            },
            RawNotifyIconEntry {
                subkey: "104".into(),
                executable_path: r"C:\Program Files (x86)\Overwolf\0.264\Overwolf.exe".into(),
                initial_tooltip: "Overwolf".into(),
                publisher: "Overwolf".into(),
                is_promoted: Some(true),
                icon_snapshot: Some(png32.clone()),
            },
            // Open Zen browser with OS mic privacy ghost -> excluded by BOTH privacy tooltip & open browser rule
            RawNotifyIconEntry {
                subkey: "105".into(),
                executable_path: r"C:\Program Files\Zen Browser\zen.exe".into(),
                initial_tooltip: "Вы открыли доступ к своему микрофону. Щёлкните для контроля доступа.".into(),
                publisher: "".into(),
                is_promoted: Some(false),
                icon_snapshot: Some(png32.clone()),
            },
            // Open Chrome -> excluded because it's a browser currently open on the taskbar
            RawNotifyIconEntry {
                subkey: "106".into(),
                executable_path: r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
                initial_tooltip: "Google Chrome".into(),
                publisher: "Google".into(),
                is_promoted: Some(false),
                icon_snapshot: Some(png32.clone()),
            },
            // Background-only Firefox (NOT open on taskbar) -> kept!
            RawNotifyIconEntry {
                subkey: "107".into(),
                executable_path: r"C:\Program Files\Mozilla Firefox\firefox.exe".into(),
                initial_tooltip: "Firefox".into(),
                publisher: "Mozilla".into(),
                is_promoted: Some(false),
                icon_snapshot: Some(png32.clone()),
            },
            // Stale uninstalled/not-running app -> excluded (no live process)
            RawNotifyIconEntry {
                subkey: "108".into(),
                executable_path: r"C:\Apps\NotRunning\ghost.exe".into(),
                initial_tooltip: "Ghost".into(),
                publisher: "".into(),
                is_promoted: Some(false),
                icon_snapshot: Some(png32.clone()),
            },
            // Explorer & own TestBar -> excluded
            RawNotifyIconEntry {
                subkey: "109".into(),
                executable_path: r"C:\Windows\explorer.exe".into(),
                initial_tooltip: "Explorer".into(),
                publisher: "Microsoft".into(),
                is_promoted: Some(false),
                icon_snapshot: Some(png32.clone()),
            },
            RawNotifyIconEntry {
                subkey: "110".into(),
                executable_path: r"C:\Apps\TestBar\testbar-v2.exe".into(),
                initial_tooltip: "TestBar".into(),
                publisher: "".into(),
                is_promoted: Some(false),
                icon_snapshot: Some(png32),
            },
        ];

        let mut fallback_hwnds = HashMap::new();
        fallback_hwnds.insert("overwolf.exe".to_string(), (8888isize, 2004u32));

        let items = build_tray_apps_from_notify_settings(
            entries,
            &live_processes,
            &open_groups,
            &fallback_hwnds,
            &HashSet::new(),
            own_pid,
            |exe, _hwnd, subkey, snap| {
                let dims = snap.and_then(parse_png_dimensions);
                if should_prefer_icon_snapshot(exe, dims) {
                    Some(tray_snapshot_icon_key(subkey, &exe_basename_lower(exe)))
                } else {
                    Some(icon_cache::path_icon_key(exe))
                }
            },
        );

        let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["firefox.exe", "overwolf.exe", "vgtray.exe", "steam.exe", "nvcontainer.exe"]
        );

        let steam = items.iter().find(|i| i.id == "steam.exe").unwrap();
        assert_eq!(steam.hwnd, Some(999), "open tray app must receive HWND from open taskbar group");
        assert!(steam.icon_data.is_none(), "AD-6: tray payload must carry icon_key only");

        let nv = items.iter().find(|i| i.id == "nvcontainer.exe").unwrap();
        assert_eq!(nv.name, "Настройки NVIDIA");
        assert!(nv.icon_key.as_deref().unwrap().starts_with("tray:"));

        let vg = items.iter().find(|i| i.id == "vgtray.exe").unwrap();
        assert_eq!(vg.name, "Riot Vanguard");
        assert_eq!(vg.exe_path, r"C:\Program Files\Riot Vanguard\vgtray.exe");

        let ow = items.iter().find(|i| i.id == "overwolf.exe").unwrap();
        assert_eq!(ow.name, "Overwolf");
        assert_eq!(ow.hwnd, Some(8888));
        assert_eq!(ow.all_pids, vec![2004, 2005]);
    }

    #[test]
    fn notify_icon_live_smoke_reads_registry_safely() {
        let cache = Mutex::new(IconCache::default());
        let (items, _sources) =
            list_tray_background_apps_filtered_with_open_mutex(&cache, &[], &[]);
        for item in &items {
            assert!(item.icon_data.is_none(), "AD-6: tray item must not carry inline icon_data");
            assert_ne!(item.id, "explorer.exe");
            assert!(!item.id.starts_with("testbar"));
            assert!(!is_os_privacy_tooltip(&item.name));
        }

        // Second call hits NotifyIconSettings metadata cache + IconCache without error
        let (items2, _sources2) =
            list_tray_background_apps_filtered_with_open_mutex(&cache, &[], &[]);
        for item in &items2 {
            assert!(item.icon_data.is_none());
        }
    }

    #[test]
    fn peripheral_batteries_uses_ttl_cache_on_consecutive_calls() {
        let first = get_peripheral_batteries();
        let t0 = Instant::now();
        let second = get_peripheral_batteries();
        let elapsed = t0.elapsed();
        assert_eq!(first.len(), second.len());
        assert!(
            elapsed < Duration::from_millis(50),
            "2nd peripheral battery call within 30s TTL must hit cache immediately, took {:?}",
            elapsed
        );
    }
}
