use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, DIGCF_PRESENT, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_FriendlyName, DEVPROPTYPE, DEVPROPKEY,
};
use windows::Win32::Foundation::{
    GlobalFree, BOOL, E_UNEXPECTED, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, RECT, S_FALSE, S_OK,
    WPARAM,
};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, GetKeyboardLayoutList, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN,
    VK_RETURN, VK_SPACE,
};
use windows::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHQueryRecycleBinW, ShellExecuteW, SHERB_NOCONFIRMATION,
    SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHQUERYRBINFO,
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
    resolve_exe_or_hwnd_icon_key, resolve_uwp_core_window_hwnd, terminate_process_pids,
    AppWindowGroup,
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
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(b'V' as u16),
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

/// Reads real Bluetooth device battery percentages reported by Windows 11 PnP
/// (`DEVPKEY_Bluetooth_Battery`), restricted strictly to `"BTH"` and `"BTHLE"` enumerators
/// with properly aligned UTF-16 stack buffers.
pub fn get_peripheral_batteries() -> Vec<PeripheralBattery> {
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
            let _ = SetupDiDestroyDeviceInfoList(hdev);
        }
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

/// Enumerates real Windows background/tray-capable user apps owning hidden or tool top-level
/// windows (excluding open taskbar apps and OS daemons), resolving crisp icons via `IconCache`.
pub fn list_tray_background_apps_cached(
    cache: &Mutex<IconCache>,
    open_taskbar_groups: &[AppWindowGroup],
) -> Vec<TrayAppItem> {
    let own_pid = unsafe { GetCurrentProcessId() };
    let candidates = enumerate_tray_window_candidates(own_pid);

    build_tray_background_apps(candidates, open_taskbar_groups, own_pid, |exe, hwnd| {
        resolve_exe_or_hwnd_icon_key(exe, hwnd, cache)
    })
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

/// Enumerates tray background apps with user ignore-list filtering using narrow `Mutex<IconCache>`
/// locking: `EnumWindows` enumeration and any `SHGetFileInfoW`/`SendMessageTimeoutW` icon
/// extraction run completely outside the shared `IconCache` mutex.
pub fn list_tray_background_apps_filtered_with_open_mutex(
    cache: &Mutex<IconCache>,
    open_taskbar_groups: &[AppWindowGroup],
    hidden_keys: &[String],
) -> (Vec<TrayAppInfo>, Vec<(String, String)>) {
    let own_pid = unsafe { GetCurrentProcessId() };
    let candidates = enumerate_tray_window_candidates(own_pid);
    let mut icon_sources = Vec::new();
    let mut seen_sources = HashSet::new();

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

/// Enumerates tray background apps with user ignore-list filtering and returns BOTH the filtered
/// `TrayAppInfo` list AND `(icon_key, exe_path)` icon source pairs so callers can register them
/// in `AppState.icon_sources` (surviving 256-LRU eviction).
pub fn list_tray_background_apps_filtered_with_open(
    cache: &mut IconCache,
    open_taskbar_groups: &[AppWindowGroup],
    hidden_keys: &[String],
) -> (Vec<TrayAppInfo>, Vec<(String, String)>) {
    let own_pid = unsafe { GetCurrentProcessId() };
    let candidates = enumerate_tray_window_candidates(own_pid);
    let mut icon_sources = Vec::new();
    let mut seen_sources = HashSet::new();

    // Also synthesize open-exe exclusions from visible standard desktop windows when caller
    // passes an empty `open_taskbar_groups` slice.
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

/// Enumerates tray background apps (auto-excluding visible desktop taskbar apps and user
/// `hidden_keys`), populating `cache` and returning `(apps, icon_sources)`.
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
}
