use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::ffi::{c_void, OsStr};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, BOOL, ERROR_ALREADY_EXISTS,
    ERROR_CLASS_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT,
    WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, HDC, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::Threading::{
    AttachThreadInput, CreateMutexW, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, SendInput, UnregisterHotKey, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
    VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHQueryUserNotificationState, ABE_BOTTOM, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
    ABM_SETPOS, ABN_FULLSCREENAPP, ABN_POSCHANGED, APPBARDATA, QUNS_ACCEPTS_NOTIFICATIONS,
    QUNS_APP, QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, EnumWindows, FindWindowW, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, PostMessageW, PostQuitMessage, PostThreadMessageW,
    RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    SetWindowsHookExW, ShowWindow, SystemParametersInfoW, TranslateMessage, UnhookWindowsHookEx,
    CREATESTRUCTW, GWLP_USERDATA, GWLP_WNDPROC, GWL_EXSTYLE, GWL_STYLE, HC_ACTION, HHOOK,
    HWND_TOPMOST, KBDLLHOOKSTRUCT, MA_NOACTIVATE, MONITORINFOF_PRIMARY, MSG, MSLLHOOKSTRUCT,
    SHOW_WINDOW_CMD,
    SM_CXSCREEN, SM_CYSCREEN, SPIF_SENDCHANGE, SPI_GETWORKAREA, SPI_SETWORKAREA, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WM_APP, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_NCCREATE, WM_QUIT,
    WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSEXW, WNDPROC, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPED,
};

use crate::coordinator::{
    classify_fullscreen, compute_appbar_rect_scaled, compute_effective_scale, BarMode,
    ForegroundWindowInfo, NotificationState, PixelRect,
};

pub const SINGLE_INSTANCE_MUTEX_NAME: &str = r"Local\TestBar_SingleInstance_v1";
pub const WM_APPBAR_CALLBACK: u32 = WM_APP + 10;
pub const WM_APPBAR_UPDATE_MODE: u32 = WM_APP + 12;
pub const HOOK_EVENT_CHANNEL_CAPACITY: usize = 64;
pub const VK_DUMMY_MASK: u16 = 0xE8;
pub const LLKHF_INJECTED_FLAG: u32 = 0x0000_0010;
pub const LLKHF_LOWER_IL_INJECTED_FLAG: u32 = 0x0000_0002;
pub const LLKHF_ALTDOWN_FLAG: u32 = 0x0000_0020;
pub const MIN_TEXT_SCALE_PERCENT: u32 = 100;
pub const MAX_TEXT_SCALE_PERCENT: u32 = 225;

const ACCESSIBILITY_SUBKEY: &str = r"Software\Microsoft\Accessibility";
const TEXT_SCALE_FACTOR_VALUE: &str = "TextScaleFactor";

const APPBAR_MSG_WND_CLASS: &str = "TestBarAppBarMsgWnd";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HOTKEY_EMERGENCY_ID: i32 = 0x7B01;

static APPBAR_HWND: AtomicIsize = AtomicIsize::new(0);
static NATIVE_HIDDEN: AtomicBool = AtomicBool::new(false);
static KEEP_NATIVE_SECONDARY_TASKBAR: AtomicBool = AtomicBool::new(false);
static RESTORE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static TEMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);
static ORIGINAL_WORKAREA: Mutex<Option<WorkAreaSnapshot>> = Mutex::new(None);
static APPBAR_CLASS_REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
static APPBAR_MSG_WINDOW: Mutex<Option<AppBarMessageWindow>> = Mutex::new(None);

static HOOK_OVERLAY_OPEN: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ACTIVE: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

static FLYOUT_SUBCLASSED_HWND: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_ORIG_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_NON_ACTIVATING: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_overlay_open_hint(open: bool) {
    HOOK_OVERLAY_OPEN.store(open, Ordering::Release);
}

pub fn is_restore_in_progress() -> bool {
    RESTORE_IN_PROGRESS.load(Ordering::SeqCst)
}

/// Ensures Per-Monitor V2 DPI awareness before querying or setting monitor/work-area rects,
/// including before Tauri/`tao` initializes and throughout `--watchdog` mode.
pub fn ensure_process_dpi_aware() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellHookEvent {
    ToggleStart,
    EmergencyRestore,
    GlobalMouseDown(i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellAppBarEvent {
    PositionChanged,
    FullscreenHint(bool),
    TaskbarCreated,
    DisplayChanged,
    DpiChanged(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkAreaSnapshot {
    pub work_area: PixelRect,
    pub primary_monitor: PixelRect,
    pub dpi: u32,
}

impl WorkAreaSnapshot {
    pub fn is_valid(&self) -> bool {
        is_valid_rect(self.work_area)
            && is_valid_rect(self.primary_monitor)
            && self.primary_monitor.covers(self.work_area)
            && self.dpi > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchdogArgs {
    pub parent_pid: u32,
    pub work_area: Option<PixelRect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchdogCli {
    NotWatchdog,
    Run(WatchdogArgs),
    Invalid(String),
}

/// RAII Win32 named mutex guard ensuring only one TestBar instance mutates shell state (AD-2).
/// Uses an unowned kernel mutex (`bInitialOwner = false`), so `Drop` only closes the handle
/// and is safe regardless of thread affinity.
pub struct SingleInstanceGuard {
    handle: isize,
}

impl SingleInstanceGuard {
    pub fn acquire() -> Result<Option<Self>, String> {
        Self::acquire_named(SINGLE_INSTANCE_MUTEX_NAME)
    }

    pub fn acquire_named(name: &str) -> Result<Option<Self>, String> {
        if name.is_empty() || name.contains('\0') {
            return Err(
                "single-instance mutex name must be non-empty and contain no NUL bytes".to_string(),
            );
        }
        unsafe {
            SetLastError(WIN32_ERROR(0));
            let wide_name = wide(name);
            let handle = CreateMutexW(None, false, PCWSTR(wide_name.as_ptr()))
                .map_err(|e| format!("CreateMutexW({name}) failed: {e}"))?;
            if handle.is_invalid() {
                return Err(format!("CreateMutexW({name}) returned invalid handle"));
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return Ok(None);
            }
            Ok(Some(Self {
                handle: handle.0 as isize,
            }))
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                let _ = CloseHandle(HANDLE(self.handle as *mut _));
            }
            self.handle = 0;
        }
    }
}

pub fn is_valid_rect(rect: PixelRect) -> bool {
    rect.right > rect.left && rect.bottom > rect.top
}

pub fn format_workarea_arg(rect: PixelRect) -> String {
    format!("{},{},{},{}", rect.left, rect.top, rect.right, rect.bottom)
}

fn parse_workarea_arg(raw: &str) -> Result<PixelRect, String> {
    let parts: Vec<&str> = raw.split(',').collect();
    if parts.len() != 4 {
        return Err(format!("expected 4 comma-separated integers, got '{raw}'"));
    }
    let left = parts[0]
        .parse::<i32>()
        .map_err(|_| format!("invalid workarea left '{}'", parts[0]))?;
    let top = parts[1]
        .parse::<i32>()
        .map_err(|_| format!("invalid workarea top '{}'", parts[1]))?;
    let right = parts[2]
        .parse::<i32>()
        .map_err(|_| format!("invalid workarea right '{}'", parts[2]))?;
    let bottom = parts[3]
        .parse::<i32>()
        .map_err(|_| format!("invalid workarea bottom '{}'", parts[3]))?;
    let rect = PixelRect::new(left, top, right, bottom);
    if !is_valid_rect(rect) {
        return Err(format!(
            "inverted or empty workarea rect ({left},{top},{right},{bottom})"
        ));
    }
    Ok(rect)
}

fn parse_pid_arg(raw: &str) -> Result<u32, String> {
    match raw.parse::<u32>() {
        Ok(pid) if pid > 0 => Ok(pid),
        _ => Err(format!("invalid parent pid '{raw}'")),
    }
}

pub fn parse_watchdog_args(args: &[String]) -> WatchdogCli {
    let has_watchdog = args
        .iter()
        .skip(1)
        .any(|a| a == "--watchdog" || a.starts_with("--watchdog="));
    if !has_watchdog {
        return WatchdogCli::NotWatchdog;
    }

    let mut parent_pid: Option<u32> = None;
    let mut work_area: Option<PixelRect> = None;
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--watchdog" {
            if parent_pid.is_some() {
                return WatchdogCli::Invalid("duplicate --watchdog flag".to_string());
            }
            let Some(val) = args.get(i + 1) else {
                return WatchdogCli::Invalid("missing parent pid after --watchdog".to_string());
            };
            match parse_pid_arg(val) {
                Ok(pid) => parent_pid = Some(pid),
                Err(reason) => return WatchdogCli::Invalid(reason),
            }
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--watchdog=") {
            if parent_pid.is_some() {
                return WatchdogCli::Invalid("duplicate --watchdog flag".to_string());
            }
            match parse_pid_arg(val) {
                Ok(pid) => parent_pid = Some(pid),
                Err(reason) => return WatchdogCli::Invalid(reason),
            }
            i += 1;
        } else if arg == "--workarea" {
            if work_area.is_some() {
                return WatchdogCli::Invalid("duplicate --workarea flag".to_string());
            }
            let Some(val) = args.get(i + 1) else {
                return WatchdogCli::Invalid("missing value after --workarea".to_string());
            };
            match parse_workarea_arg(val) {
                Ok(rect) => work_area = Some(rect),
                Err(reason) => return WatchdogCli::Invalid(reason),
            }
            i += 2;
        } else if let Some(val) = arg.strip_prefix("--workarea=") {
            if work_area.is_some() {
                return WatchdogCli::Invalid("duplicate --workarea flag".to_string());
            }
            match parse_workarea_arg(val) {
                Ok(rect) => work_area = Some(rect),
                Err(reason) => return WatchdogCli::Invalid(reason),
            }
            i += 1;
        } else {
            return WatchdogCli::Invalid(format!("unexpected watchdog argument '{arg}'"));
        }
    }

    match parent_pid {
        Some(pid) => WatchdogCli::Run(WatchdogArgs {
            parent_pid: pid,
            work_area,
        }),
        None => WatchdogCli::Invalid("missing parent pid for --watchdog".to_string()),
    }
}

pub fn recovery_workarea_path() -> PathBuf {
    let base = dirs::data_local_dir()
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);
    base.join("TestBar").join("recovery-workarea.json")
}

pub fn primary_monitor_dpi() -> u32 {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x > 0 {
            return dpi_x;
        }
        96
    }
}

/// Pure parser for Windows Accessibility `TextScaleFactor` (DWORD percent, default 100, clamped 100..=225).
pub fn parse_text_scale_factor(raw: Option<u32>) -> f64 {
    let pct = raw.unwrap_or(MIN_TEXT_SCALE_PERCENT);
    let clamped = pct.clamp(MIN_TEXT_SCALE_PERCENT, MAX_TEXT_SCALE_PERCENT);
    clamped as f64 / 100.0
}

/// Reads `HKCU\Software\Microsoft\Accessibility\TextScaleFactor` (DWORD) and returns
/// the WebView2 rasterization text-scale multiplier (`1.0..=2.25`).
pub fn windows_text_scale_factor() -> f64 {
    let raw = unsafe {
        let subkey = wide(ACCESSIBILITY_SUBKEY);
        let value_name = wide(TEXT_SCALE_FACTOR_VALUE);
        let mut value: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        );
        if status == ERROR_SUCCESS {
            Some(value)
        } else {
            None
        }
    };
    parse_text_scale_factor(raw)
}

pub fn query_current_workarea_snapshot() -> Option<WorkAreaSnapshot> {
    unsafe {
        let mut wa = RECT::default();
        if SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut wa as *mut RECT as *mut c_void),
            Default::default(),
        )
        .is_err()
        {
            return None;
        }
        let snap = WorkAreaSnapshot {
            work_area: PixelRect::new(wa.left, wa.top, wa.right, wa.bottom),
            primary_monitor: primary_monitor_rect().to_pixel_rect(),
            dpi: primary_monitor_dpi(),
        };
        if snap.is_valid() {
            Some(snap)
        } else {
            None
        }
    }
}

pub fn save_workarea_snapshot_to(path: &Path, snapshot: &WorkAreaSnapshot) -> Result<(), String> {
    if !snapshot.is_valid() {
        return Err("cannot save invalid or inverted work-area snapshot".to_string());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create recovery directory: {e}"))?;
    }
    let payload = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("failed to serialize work-area snapshot: {e}"))?;

    let seq = TEMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("recovery-workarea.json");
    let tmp_path = path.with_file_name(format!("{file_name}.{}.{}.tmp", std::process::id(), seq));

    let write_res: Result<(), String> = (|| {
        let mut file = std::fs::File::create(&tmp_path)
            .map_err(|e| format!("failed to create temp recovery file: {e}"))?;
        file.write_all(payload.as_bytes())
            .map_err(|e| format!("failed to write temp recovery file: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("failed to sync temp recovery file: {e}"))?;
        Ok(())
    })();
    if let Err(err) = write_res {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Err(err) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to atomically replace recovery file: {err}"));
    }
    Ok(())
}

pub fn load_workarea_snapshot_from(path: &Path) -> Option<WorkAreaSnapshot> {
    let raw = std::fs::read_to_string(path).ok()?;
    let snap: WorkAreaSnapshot = serde_json::from_str(&raw).ok()?;
    if snap.is_valid() {
        Some(snap)
    } else {
        None
    }
}

fn is_snapshot_compatible(
    snap: &WorkAreaSnapshot,
    current_monitor: PixelRect,
    current_dpi: u32,
) -> bool {
    snap.is_valid() && snap.primary_monitor == current_monitor && snap.dpi == current_dpi
}

fn is_native_taskbar_visible() -> bool {
    unsafe {
        let cls = wide("Shell_TrayWnd");
        if let Ok(hwnd) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) {
            if !hwnd.0.is_null() {
                return IsWindowVisible(hwnd).as_bool();
            }
        }
        false
    }
}

fn select_capture_snapshot(
    current: WorkAreaSnapshot,
    native_taskbar_visible: bool,
    existing_disk: Option<WorkAreaSnapshot>,
) -> (WorkAreaSnapshot, bool) {
    if !native_taskbar_visible {
        if let Some(disk) = existing_disk {
            if is_snapshot_compatible(&disk, current.primary_monitor, current.dpi) {
                return (disk, false);
            }
        }
    }
    (current, true)
}

pub fn capture_and_persist_original_workarea() -> Option<WorkAreaSnapshot> {
    let current = query_current_workarea_snapshot()?;
    let path = recovery_workarea_path();
    let existing_disk = load_workarea_snapshot_from(&path);
    let (chosen, should_persist) =
        select_capture_snapshot(current, is_native_taskbar_visible(), existing_disk);
    {
        let mut guard = ORIGINAL_WORKAREA.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(chosen);
    }
    if should_persist {
        let _ = save_workarea_snapshot_to(&path, &chosen);
    }
    Some(chosen)
}

fn resolve_recovery_workarea(
    explicit: Option<PixelRect>,
    in_memory: Option<WorkAreaSnapshot>,
    disk_path: &Path,
    current_monitor: PixelRect,
    current_dpi: u32,
) -> Option<PixelRect> {
    explicit
        .filter(|r| is_valid_rect(*r) && current_monitor.covers(*r))
        .or_else(|| {
            in_memory
                .filter(|s| is_snapshot_compatible(s, current_monitor, current_dpi))
                .map(|s| s.work_area)
        })
        .or_else(|| {
            load_workarea_snapshot_from(disk_path)
                .filter(|s| is_snapshot_compatible(s, current_monitor, current_dpi))
                .map(|s| s.work_area)
        })
}

fn get_recovery_workarea(explicit: Option<PixelRect>) -> Option<PixelRect> {
    let in_memory = *ORIGINAL_WORKAREA.lock().unwrap_or_else(|e| e.into_inner());
    let mon_px = primary_monitor_rect().to_pixel_rect();
    let dpi = primary_monitor_dpi();
    resolve_recovery_workarea(explicit, in_memory, &recovery_workarea_path(), mon_px, dpi)
}

fn invoke_chained_panic_restore<R, P>(restore_fn: R, prev_hook: P)
where
    R: FnOnce(),
    P: FnOnce(),
{
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(restore_fn));
    prev_hook();
}

/// Installs an idempotent chained Rust panic hook that restores the native taskbar
/// non-blockingly without double-panicking before delegating to the previous panic hook.
pub fn install_panic_restore_hook() {
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        invoke_chained_panic_restore(restore_native_taskbar_nonblocking, || prev_hook(info));
    }));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub width: i32,
    pub height: i32,
    pub dpi: u32,
}

impl MonitorRect {
    pub fn to_pixel_rect(self) -> PixelRect {
        PixelRect::new(self.left, self.top, self.right, self.bottom)
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

unsafe fn monitor_dpi(hmon: HMONITOR) -> u32 {
    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x > 0 {
        dpi_x
    } else {
        96
    }
}

pub fn primary_monitor_rect() -> MonitorRect {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmon, &mut info).as_bool() {
            let r = info.rcMonitor;
            return MonitorRect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
                width: r.right - r.left,
                height: r.bottom - r.top,
                dpi: monitor_dpi(hmon),
            };
        }
        let w = GetSystemMetrics(SM_CXSCREEN);
        let h = GetSystemMetrics(SM_CYSCREEN);
        MonitorRect {
            left: 0,
            top: 0,
            right: w,
            bottom: h,
            width: w,
            height: h,
            dpi: 96,
        }
    }
}

/// Enumerates all non-primary display monitors via `EnumDisplayMonitors` with exact
/// physical bounds and per-monitor effective DPI, sorted deterministically by `(left, top)`.
pub fn secondary_monitor_rects() -> Vec<MonitorRect> {
    extern "system" fn enum_proc(
        hmon: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        unsafe {
            let out = &mut *(lparam.0 as *mut Vec<MonitorRect>);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if GetMonitorInfoW(hmon, &mut info).as_bool()
                && (info.dwFlags & MONITORINFOF_PRIMARY) == 0
            {
                let r = info.rcMonitor;
                out.push(MonitorRect {
                    left: r.left,
                    top: r.top,
                    right: r.right,
                    bottom: r.bottom,
                    width: r.right - r.left,
                    height: r.bottom - r.top,
                    dpi: monitor_dpi(hmon),
                });
            }
        }
        BOOL(1)
    }

    let mut monitors: Vec<MonitorRect> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(enum_proc),
            LPARAM(&mut monitors as *mut _ as isize),
        );
    }
    monitors.sort_by_key(|m| (m.left, m.top));
    monitors
}

/// Apply borderless tool-window topmost styling so windows never show in Alt+Tab
/// and never receive unwanted Windows 11 DWM square drop-shadow outlines.
pub fn style_overlay_window(hwnd_raw: isize, no_activate: bool) {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        ex |= (WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) as isize;
        if no_activate {
            ex |= WS_EX_NOACTIVATE.0 as isize;
        } else {
            ex &= !(WS_EX_NOACTIVATE.0 as isize);
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);

        let pref = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _,
            std::mem::size_of_val(&pref) as u32,
        );

        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

pub fn force_focus_window(hwnd_raw: isize) {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let fg = GetForegroundWindow();
        let fg_tid = GetWindowThreadProcessId(fg, None);
        let cur_tid = GetCurrentThreadId();
        let target_tid = GetWindowThreadProcessId(hwnd, None);
        if fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, true);
        }
        if target_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, target_tid, true);
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        if target_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, target_tid, false);
        }
        if fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        }
    }
}

pub fn is_cursor_near_window(hwnd_raw: isize, pad_x: i32, pad_top: i32, pad_bottom: i32) -> bool {
    unsafe {
        let mut pt = POINT::default();
        let mut rect = RECT::default();
        let hwnd = HWND(hwnd_raw as *mut _);
        if GetCursorPos(&mut pt).is_ok() && GetWindowRect(hwnd, &mut rect).is_ok() {
            return pt.x >= rect.left.saturating_sub(pad_x)
                && pt.x <= rect.right.saturating_add(pad_x)
                && pt.y >= rect.top.saturating_sub(pad_top)
                && pt.y <= rect.bottom.saturating_add(pad_bottom);
        }
        false
    }
}

pub fn get_window_pixel_rect(hwnd_raw: isize) -> Option<PixelRect> {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_ok() {
            Some(PixelRect::new(r.left, r.top, r.right, r.bottom))
        } else {
            None
        }
    }
}

/// Positions and resizes a window in physical pixels without activating it and
/// without changing its visibility (never passes `SWP_SHOWWINDOW`).
pub fn position_window(hwnd_raw: isize, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
    }
}

/// Subclasses the pre-warmed `flyout` window once so non-activating flyouts
/// (`preview`, `osd`) return `MA_NOACTIVATE` on `WM_MOUSEACTIVATE` without
/// flipping `GWL_EXSTYLE` + `SWP_FRAMECHANGED` per open.
pub fn install_flyout_noactivate_subclass(hwnd_raw: isize) {
    if hwnd_raw == 0 {
        return;
    }
    if FLYOUT_SUBCLASSED_HWND
        .compare_exchange(0, hwnd_raw, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let current_proc = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        if current_proc != 0 {
            FLYOUT_ORIG_WNDPROC.store(current_proc, Ordering::SeqCst);
        }
        let prev = SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            flyout_wnd_proc as *const () as usize as isize,
        );
        if prev != 0 {
            FLYOUT_ORIG_WNDPROC.store(prev, Ordering::SeqCst);
        } else {
            FLYOUT_ORIG_WNDPROC.store(0, Ordering::SeqCst);
            FLYOUT_SUBCLASSED_HWND.store(0, Ordering::SeqCst);
        }
    }
}

pub fn set_flyout_non_activating(non_activating: bool) {
    FLYOUT_NON_ACTIVATING.store(non_activating, Ordering::SeqCst);
}

unsafe extern "system" fn flyout_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if hwnd.0 as isize == FLYOUT_SUBCLASSED_HWND.load(Ordering::SeqCst) {
        if msg == WM_MOUSEACTIVATE && FLYOUT_NON_ACTIVATING.load(Ordering::SeqCst) {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        let orig = FLYOUT_ORIG_WNDPROC.load(Ordering::SeqCst);
        if orig != 0 {
            let prev_proc: WNDPROC = Some(std::mem::transmute(orig));
            return CallWindowProcW(prev_proc, hwnd, msg, wparam, lparam);
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub fn set_window_visible(hwnd_raw: isize, visible: bool, no_activate: bool) {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if visible {
            let _ = ShowWindow(hwnd, if no_activate { SW_SHOWNOACTIVATE } else { SW_SHOW });
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | if no_activate { SWP_NOACTIVATE } else { Default::default() },
            );
            if !no_activate {
                force_focus_window(hwnd_raw);
            }
        } else {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

pub fn secondary_native_taskbar_should_stay_visible(mode: &str) -> bool {
    mode.trim().eq_ignore_ascii_case("native")
}

pub fn set_keep_native_secondary_taskbar(keep_native: bool) {
    KEEP_NATIVE_SECONDARY_TASKBAR.store(keep_native, Ordering::SeqCst);
}

pub fn apply_secondary_taskbar_mode_to_native(mode: &str, hide_native: bool) {
    set_keep_native_secondary_taskbar(secondary_native_taskbar_should_stay_visible(mode));
    if hide_native && !RESTORE_IN_PROGRESS.load(Ordering::SeqCst) {
        hide_native_taskbar();
    }
}

fn is_any_secondary_native_taskbar_visible() -> bool {
    unsafe {
        let mut any_visible = false;
        extern "system" fn check_secondary(hwnd: HWND, lparam: LPARAM) -> BOOL {
            unsafe {
                let mut buf = [0u16; 64];
                let len = GetClassNameW(hwnd, &mut buf);
                if len > 0 {
                    let name = String::from_utf16_lossy(&buf[..len as usize]);
                    if name == "Shell_SecondaryTrayWnd" && IsWindowVisible(hwnd).as_bool() {
                        *(lparam.0 as *mut bool) = true;
                        return BOOL(0);
                    }
                }
            }
            BOOL(1)
        }
        let _ = EnumWindows(
            Some(check_secondary),
            LPARAM(&mut any_visible as *mut bool as isize),
        );
        any_visible
    }
}

fn set_native_taskbars_visibility(primary_cmd: SHOW_WINDOW_CMD, secondary_cmd: SHOW_WINDOW_CMD) {
    unsafe {
        let cls = wide("Shell_TrayWnd");
        if let Ok(hwnd) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) {
            if !hwnd.0.is_null() {
                let _ = ShowWindow(hwnd, primary_cmd);
            }
        }
        extern "system" fn set_secondary(hwnd: HWND, lparam: LPARAM) -> BOOL {
            unsafe {
                let mut buf = [0u16; 64];
                let len = GetClassNameW(hwnd, &mut buf);
                if len > 0 {
                    let name = String::from_utf16_lossy(&buf[..len as usize]);
                    if name == "Shell_SecondaryTrayWnd" {
                        let _ = ShowWindow(hwnd, SHOW_WINDOW_CMD(lparam.0 as i32));
                    }
                }
            }
            BOOL(1)
        }
        let _ = EnumWindows(Some(set_secondary), LPARAM(secondary_cmd.0 as isize));
    }
}

/// Safely hides Windows 11 native taskbar(s) without injecting or terminating explorer.exe.
/// Keeps `Shell_SecondaryTrayWnd` visible when `secondary_taskbar_mode == "native"`.
pub fn hide_native_taskbar() {
    if RESTORE_IN_PROGRESS.load(Ordering::SeqCst) {
        return;
    }
    let secondary_cmd = if KEEP_NATIVE_SECONDARY_TASKBAR.load(Ordering::SeqCst) {
        SW_SHOW
    } else {
        SW_HIDE
    };
    set_native_taskbars_visibility(SW_HIDE, secondary_cmd);
    NATIVE_HIDDEN.store(true, Ordering::SeqCst);
}

/// Lightweight check for the 650ms poller: only re-hides native taskbars if Explorer
/// snuck `Shell_TrayWnd` (or `Shell_SecondaryTrayWnd` when hidden) back visible while TestBar owns the shell.
pub fn ensure_native_taskbar_hidden_if_needed() {
    if RESTORE_IN_PROGRESS.load(Ordering::SeqCst) {
        return;
    }
    let keep_secondary = KEEP_NATIVE_SECONDARY_TASKBAR.load(Ordering::SeqCst);
    if is_native_taskbar_visible()
        || (!keep_secondary && is_any_secondary_native_taskbar_visible())
    {
        hide_native_taskbar();
    }
}

trait AppBarShellOps: Send {
    fn primary_monitor_rect(&self) -> PixelRect;
    fn primary_monitor_dpi(&self) -> u32;
    fn text_scale_factor(&self) -> f64;
    fn hide_native_taskbar(&self);
    fn abm_new(&self, hwnd: HWND, abd: &mut APPBARDATA) -> bool;
    fn abm_querypos(&self, hwnd: HWND, abd: &mut APPBARDATA);
    fn abm_setpos(&self, hwnd: HWND, abd: &mut APPBARDATA);
    fn abm_remove(&self, hwnd: HWND);
    fn on_appbar_hwnd_registered(&self, hwnd: HWND);
    fn on_appbar_hwnd_removed(&self, hwnd: HWND);
}

struct Win32AppBarShellOps;

impl AppBarShellOps for Win32AppBarShellOps {
    fn primary_monitor_rect(&self) -> PixelRect {
        primary_monitor_rect().to_pixel_rect()
    }

    fn primary_monitor_dpi(&self) -> u32 {
        primary_monitor_dpi()
    }

    fn text_scale_factor(&self) -> f64 {
        windows_text_scale_factor()
    }

    fn hide_native_taskbar(&self) {
        hide_native_taskbar();
    }

    fn abm_new(&self, _hwnd: HWND, abd: &mut APPBARDATA) -> bool {
        unsafe { SHAppBarMessage(ABM_NEW, abd) != 0 }
    }

    fn abm_querypos(&self, _hwnd: HWND, abd: &mut APPBARDATA) {
        unsafe {
            let _ = SHAppBarMessage(ABM_QUERYPOS, abd);
        }
    }

    fn abm_setpos(&self, _hwnd: HWND, abd: &mut APPBARDATA) {
        unsafe {
            let _ = SHAppBarMessage(ABM_SETPOS, abd);
        }
    }

    fn abm_remove(&self, hwnd: HWND) {
        unsafe {
            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: hwnd,
                ..Default::default()
            };
            let _ = SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
    }

    fn on_appbar_hwnd_registered(&self, hwnd: HWND) {
        APPBAR_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    }

    fn on_appbar_hwnd_removed(&self, hwnd: HWND) {
        let _ = APPBAR_HWND.compare_exchange(
            hwnd.0 as isize,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }
}

struct AppBarWindowContext {
    shell_ops: Box<dyn AppBarShellOps>,
    appbar_registered: Cell<bool>,
    bar_mode: Cell<BarMode>,
    taskbar_created_msg: u32,
    last_appbar_rect: Cell<Option<PixelRect>>,
    on_event: Box<dyn Fn(ShellAppBarEvent) + Send + Sync>,
}

/// Dedicated hidden top-level `WS_OVERLAPPED` window (`hWndParent = HWND(0)`, never `HWND_MESSAGE`)
/// that hosts AppBar registration, `TaskbarCreated` broadcasts, `WM_DISPLAYCHANGE`, and `WM_DPICHANGED`.
pub struct AppBarMessageWindow {
    hwnd: isize,
    taskbar_created_msg: u32,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

fn ensure_appbar_window_class() -> Result<(), String> {
    APPBAR_CLASS_REGISTERED
        .get_or_init(|| unsafe {
            let hinstance = GetModuleHandleW(None)
                .map_err(|e| format!("GetModuleHandleW failed: {e}"))?;
            let cls = wide(APPBAR_MSG_WND_CLASS);
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(appbar_msg_wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: PCWSTR(cls.as_ptr()),
                ..Default::default()
            };
            SetLastError(WIN32_ERROR(0));
            let atom = RegisterClassExW(&wc);
            if atom == 0 {
                let err = GetLastError();
                if err != ERROR_CLASS_ALREADY_EXISTS {
                    return Err(format!("RegisterClassExW failed: {err:?}"));
                }
            }
            Ok(())
        })
        .clone()
}

fn apply_appbar_on_hwnd(
    hwnd: HWND,
    ctx: &AppBarWindowContext,
    force_new: bool,
    only_if_changed: bool,
    dpi_override: Option<u32>,
) {
    let mon_px = ctx.shell_ops.primary_monitor_rect();
    let dpi = dpi_override
        .filter(|&d| d > 0)
        .unwrap_or_else(|| ctx.shell_ops.primary_monitor_dpi());
    let effective_scale = compute_effective_scale(dpi, ctx.shell_ops.text_scale_factor());
    let target = compute_appbar_rect_scaled(mon_px, effective_scale, ctx.bar_mode.get());
    let reserve_h = target.height();

    let mut abd = APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: WM_APPBAR_CALLBACK,
        uEdge: ABE_BOTTOM,
        rc: RECT {
            left: target.left,
            top: target.top,
            right: target.right,
            bottom: target.bottom,
        },
        ..Default::default()
    };

    if force_new || !ctx.appbar_registered.get() {
        if !ctx.shell_ops.abm_new(hwnd, &mut abd) {
            ctx.appbar_registered.set(false);
            ctx.last_appbar_rect.set(None);
            return;
        }
        ctx.appbar_registered.set(true);
        ctx.last_appbar_rect.set(None);
        ctx.shell_ops.on_appbar_hwnd_registered(hwnd);
    }
    ctx.shell_ops.abm_querypos(hwnd, &mut abd);
    abd.rc.top = abd.rc.bottom - reserve_h;
    let resolved = PixelRect::new(abd.rc.left, abd.rc.top, abd.rc.right, abd.rc.bottom);
    if only_if_changed && ctx.last_appbar_rect.get() == Some(resolved) {
        return;
    }
    ctx.shell_ops.abm_setpos(hwnd, &mut abd);
    ctx.last_appbar_rect.set(Some(resolved));
}

fn remove_appbar_on_hwnd(hwnd: HWND, ctx: &AppBarWindowContext) {
    ctx.shell_ops.abm_remove(hwnd);
    ctx.shell_ops.on_appbar_hwnd_removed(hwnd);
    ctx.appbar_registered.set(false);
    ctx.last_appbar_rect.set(None);
}

unsafe extern "system" fn appbar_msg_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = lparam.0 as *const CREATESTRUCTW;
        if !cs.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
        }
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppBarWindowContext;
    if ctx_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ctx = &*ctx_ptr;

        if ctx.taskbar_created_msg != 0 && msg == ctx.taskbar_created_msg {
            ctx.shell_ops.hide_native_taskbar();
            ctx.appbar_registered.set(false);
            apply_appbar_on_hwnd(hwnd, ctx, true, false, None);
            (ctx.on_event)(ShellAppBarEvent::TaskbarCreated);
            return Some(LRESULT(0));
        }

        match msg {
            WM_APPBAR_CALLBACK => {
                match wparam.0 as u32 {
                    ABN_POSCHANGED => {
                        apply_appbar_on_hwnd(hwnd, ctx, false, true, None);
                        (ctx.on_event)(ShellAppBarEvent::PositionChanged);
                    }
                    ABN_FULLSCREENAPP => {
                        let entering = lparam.0 != 0;
                        (ctx.on_event)(ShellAppBarEvent::FullscreenHint(entering));
                    }
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_APPBAR_UPDATE_MODE => {
                let next_mode = if wparam.0 == 1 {
                    BarMode::Edge
                } else {
                    BarMode::Dock
                };
                ctx.bar_mode.set(next_mode);
                apply_appbar_on_hwnd(hwnd, ctx, false, false, None);
                (ctx.on_event)(ShellAppBarEvent::PositionChanged);
                Some(LRESULT(0))
            }
            WM_DISPLAYCHANGE => {
                apply_appbar_on_hwnd(hwnd, ctx, false, false, None);
                (ctx.on_event)(ShellAppBarEvent::DisplayChanged);
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                let raw_dpi = (wparam.0 & 0xFFFF) as u32;
                let dpi = if raw_dpi > 0 {
                    raw_dpi
                } else {
                    ctx.shell_ops.primary_monitor_dpi()
                };
                apply_appbar_on_hwnd(hwnd, ctx, false, false, Some(dpi));
                (ctx.on_event)(ShellAppBarEvent::DpiChanged(dpi));
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                if ctx.appbar_registered.get() {
                    remove_appbar_on_hwnd(hwnd, ctx);
                }
                let _ = DestroyWindow(hwnd);
                Some(LRESULT(0))
            }
            WM_DESTROY => {
                if ctx.appbar_registered.get() {
                    remove_appbar_on_hwnd(hwnd, ctx);
                }
                PostQuitMessage(0);
                Some(LRESULT(0))
            }
            _ => None,
        }
    }));

    match result {
        Ok(Some(lres)) => lres,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

impl AppBarMessageWindow {
    pub fn spawn<F>(initial_mode: BarMode, on_event: F) -> Result<Self, String>
    where
        F: Fn(ShellAppBarEvent) + Send + Sync + 'static,
    {
        Self::spawn_with_ops(initial_mode, Box::new(Win32AppBarShellOps), on_event)
    }

    fn spawn_with_ops<F>(
        initial_mode: BarMode,
        shell_ops: Box<dyn AppBarShellOps>,
        on_event: F,
    ) -> Result<Self, String>
    where
        F: Fn(ShellAppBarEvent) + Send + Sync + 'static,
    {
        ensure_appbar_window_class()?;
        let (tx, rx) = mpsc::sync_channel::<Result<(isize, u32), String>>(1);

        let join_handle = std::thread::spawn(move || unsafe {
            let hinstance = match GetModuleHandleW(None) {
                Ok(h) => h,
                Err(e) => {
                    let _ = tx.send(Err(format!("GetModuleHandleW failed: {e}")));
                    return;
                }
            };

            let taskbar_created_msg =
                RegisterWindowMessageW(PCWSTR(wide("TaskbarCreated").as_ptr()));
            let ctx = Box::new(AppBarWindowContext {
                shell_ops,
                appbar_registered: Cell::new(false),
                bar_mode: Cell::new(initial_mode),
                taskbar_created_msg,
                last_appbar_rect: Cell::new(None),
                on_event: Box::new(on_event),
            });
            let ctx_ptr = Box::into_raw(ctx);

            let cls = wide(APPBAR_MSG_WND_CLASS);
            let hwnd = match CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(cls.as_ptr()),
                PCWSTR::null(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                HWND::default(),
                None,
                hinstance,
                Some(ctx_ptr as *const c_void),
            ) {
                Ok(h) if !h.0.is_null() => h,
                Ok(_) => {
                    drop(Box::from_raw(ctx_ptr));
                    let _ = tx.send(Err("CreateWindowExW returned null HWND".to_string()));
                    return;
                }
                Err(e) => {
                    drop(Box::from_raw(ctx_ptr));
                    let _ = tx.send(Err(format!("CreateWindowExW failed: {e}")));
                    return;
                }
            };

            apply_appbar_on_hwnd(hwnd, &*ctx_ptr, true, false, None);

            let _ = tx.send(Ok((hwnd.0 as isize, taskbar_created_msg)));

            let mut msg = MSG::default();
            loop {
                let ret = GetMessageW(&mut msg, HWND::default(), 0, 0);
                if ret.0 <= 0 {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            // Guarantee cleanup even if GetMessageW exited abnormally without WM_CLOSE/WM_DESTROY.
            let ctx_ref = &*ctx_ptr;
            if ctx_ref.appbar_registered.get() {
                remove_appbar_on_hwnd(hwnd, ctx_ref);
            }
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            if IsWindow(hwnd).as_bool() {
                let _ = DestroyWindow(hwnd);
            }
            drop(Box::from_raw(ctx_ptr));
        });

        match rx.recv() {
            Ok(Ok((hwnd, taskbar_created_msg))) => Ok(Self {
                hwnd,
                taskbar_created_msg,
                join_handle: Some(join_handle),
            }),
            Ok(Err(e)) => {
                let _ = join_handle.join();
                Err(e)
            }
            Err(e) => {
                let _ = join_handle.join();
                Err(format!("appbar message window thread exited early: {e}"))
            }
        }
    }

    pub fn hwnd(&self) -> isize {
        self.hwnd
    }

    pub fn taskbar_created_msg(&self) -> u32 {
        self.taskbar_created_msg
    }

    pub fn update_mode(&self, mode: BarMode) {
        if self.hwnd != 0 {
            let mode_arg = if mode == BarMode::Edge { 1 } else { 0 };
            unsafe {
                let _ = PostMessageW(
                    HWND(self.hwnd as *mut _),
                    WM_APPBAR_UPDATE_MODE,
                    WPARAM(mode_arg),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn request_close_nonblocking(&self) {
        if self.hwnd != 0 {
            unsafe {
                let _ = PostMessageW(HWND(self.hwnd as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn stop(&mut self) {
        let post_ok = if self.hwnd != 0 {
            let hwnd = HWND(self.hwnd as *mut _);
            self.hwnd = 0;
            unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).is_ok() }
        } else {
            false
        };
        if let Some(jh) = self.join_handle.take() {
            if post_ok && jh.thread().id() != std::thread::current().id() {
                let _ = jh.join();
            }
        }
    }
}

impl Drop for AppBarMessageWindow {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start_appbar_message_window<F>(initial_mode: BarMode, on_event: F) -> Result<(), String>
where
    F: Fn(ShellAppBarEvent) + Send + Sync + 'static,
{
    if RESTORE_IN_PROGRESS.load(Ordering::SeqCst) {
        return Err("restore in progress; refusing to start AppBar message window".to_string());
    }
    stop_appbar_message_window();
    let wnd = AppBarMessageWindow::spawn(initial_mode, on_event)?;
    let (aborted, prev) = {
        let mut slot = APPBAR_MSG_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
        if RESTORE_IN_PROGRESS.load(Ordering::SeqCst) {
            (Some(wnd), None)
        } else {
            (None, slot.replace(wnd))
        }
    };
    if let Some(mut old) = prev {
        old.stop();
    }
    if let Some(mut new_wnd) = aborted {
        new_wnd.stop();
        return Err("restore began while starting AppBar message window".to_string());
    }
    Ok(())
}

pub fn update_appbar_mode(mode: BarMode) {
    let slot = APPBAR_MSG_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(wnd) = slot.as_ref() {
        wnd.update_mode(mode);
    }
}

pub fn stop_appbar_message_window() {
    let wnd = {
        let mut slot = APPBAR_MSG_WINDOW.lock().unwrap_or_else(|e| e.into_inner());
        slot.take()
    };
    if let Some(mut w) = wnd {
        w.stop();
    }
}

/// Prompts Explorer to prune orphaned AppBar reservations after an abrupt crash
/// by registering and immediately removing a local hidden top-level dummy `WS_OVERLAPPED` AppBar.
pub fn flush_orphaned_appbars() {
    if ensure_appbar_window_class().is_err() {
        return;
    }
    unsafe {
        let Ok(hinstance) = GetModuleHandleW(None) else {
            return;
        };
        let cls = wide(APPBAR_MSG_WND_CLASS);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR(cls.as_ptr()),
            PCWSTR::null(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            HWND::default(),
            None,
            hinstance,
            None,
        ) else {
            return;
        };
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: WM_APP + 11,
            uEdge: ABE_BOTTOM,
            ..Default::default()
        };
        let _ = SHAppBarMessage(ABM_NEW, &mut abd);
        let _ = SHAppBarMessage(ABM_REMOVE, &mut abd);
        let _ = DestroyWindow(hwnd);
    }
}

fn show_native_taskbars_and_restore_workarea(explicit_workarea: Option<PixelRect>) {
    set_native_taskbars_visibility(SW_SHOW, SW_SHOW);
    if let Some(wa_rect) = get_recovery_workarea(explicit_workarea) {
        let mut wa = RECT {
            left: wa_rect.left,
            top: wa_rect.top,
            right: wa_rect.right,
            bottom: wa_rect.bottom,
        };
        unsafe {
            let _ = SystemParametersInfoW(
                SPI_SETWORKAREA,
                0,
                Some(&mut wa as *mut RECT as *mut c_void),
                SPIF_SENDCHANGE,
            );
        }
    }
}

/// Non-blocking restoration path for panic hooks and emergency hotkey handlers:
/// unregisters the AppBar, shows native taskbars and restores work area immediately,
/// and posts `WM_CLOSE` via `try_lock` without blocking on `APPBAR_MSG_WINDOW` or `join()`.
pub fn restore_native_taskbar_nonblocking() {
    RESTORE_IN_PROGRESS.store(true, Ordering::SeqCst);
    unregister_appbar();
    show_native_taskbars_and_restore_workarea(None);
    if let Ok(slot) = APPBAR_MSG_WINDOW.try_lock() {
        if let Some(wnd) = slot.as_ref() {
            wnd.request_close_nonblocking();
        }
    }
    NATIVE_HIDDEN.store(false, Ordering::SeqCst);
}

fn restore_native_taskbar_internal(explicit_workarea: Option<PixelRect>, flush_orphans: bool) {
    RESTORE_IN_PROGRESS.store(true, Ordering::SeqCst);
    stop_appbar_message_window();
    unregister_appbar();
    if flush_orphans {
        flush_orphaned_appbars();
    }
    show_native_taskbars_and_restore_workarea(explicit_workarea);
    NATIVE_HIDDEN.store(false, Ordering::SeqCst);
}

/// Guaranteed restoration of Windows 11 native taskbar(s) + AppBar removal + exact HiDPI workarea refresh.
pub fn restore_native_taskbar() {
    restore_native_taskbar_internal(None, false);
}

pub fn unregister_appbar() {
    let prev = APPBAR_HWND.swap(0, Ordering::SeqCst);
    if prev != 0 {
        unsafe {
            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: HWND(prev as *mut _),
                ..Default::default()
            };
            let _ = SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
    }
}

/// Spawns lightweight companion watchdog: `testbar.exe --watchdog <pid> [--workarea <l,t,r,b>]`.
/// If main process is killed via Task Manager or crashes, watchdog flushes orphaned AppBars and restores native taskbar.
pub fn spawn_watchdog(work_area: Option<PixelRect>) {
    if let Ok(exe) = std::env::current_exe() {
        let pid = unsafe { GetCurrentProcessId() };
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--watchdog").arg(pid.to_string());
        if let Some(rect) = work_area.filter(|r| is_valid_rect(*r)) {
            cmd.arg("--workarea").arg(format_workarea_arg(rect));
        }
        let _ = cmd.creation_flags(CREATE_NO_WINDOW).spawn();
    }
}

pub fn run_watchdog(args: WatchdogArgs) {
    ensure_process_dpi_aware();
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, args.parent_pid) {
            let _ = WaitForSingleObject(handle, INFINITE);
            let _ = CloseHandle(handle);
        }
    }
    // Avoid clobbering a newly restarted TestBar instance on quick restart.
    let _guard = match SingleInstanceGuard::acquire() {
        Ok(Some(guard)) => Some(guard),
        Ok(None) => return,
        Err(_) => None,
    };
    restore_native_taskbar_internal(args.work_area, true);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullscreenSnapshot {
    pub notification_state: NotificationState,
    pub foreground: Option<ForegroundWindowInfo>,
    pub primary_monitor: PixelRect,
}

/// Captures the Win32 inputs required by `coordinator::classify_fullscreen`.
pub fn query_fullscreen_snapshot() -> FullscreenSnapshot {
    unsafe {
        let notification_state = match SHQueryUserNotificationState() {
            Ok(state) if state == QUNS_NOT_PRESENT => NotificationState::NotPresent,
            Ok(state) if state == QUNS_BUSY => NotificationState::Busy,
            Ok(state) if state == QUNS_RUNNING_D3D_FULL_SCREEN => {
                NotificationState::RunningD3dFullScreen
            }
            Ok(state) if state == QUNS_PRESENTATION_MODE => NotificationState::PresentationMode,
            Ok(state) if state == QUNS_ACCEPTS_NOTIFICATIONS => {
                NotificationState::AcceptsNotifications
            }
            Ok(state) if state == QUNS_QUIET_TIME => NotificationState::QuietTime,
            Ok(state) if state == QUNS_APP => NotificationState::App,
            _ => NotificationState::Unknown,
        };

        let primary_monitor = primary_monitor_rect().to_pixel_rect();

        let fg = GetForegroundWindow();
        let foreground = if fg.0.is_null() {
            None
        } else {
            let visible = IsWindowVisible(fg).as_bool() && !IsIconic(fg).as_bool();
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            let is_own_process = pid == GetCurrentProcessId();

            let mut cls_buf = [0u16; 256];
            let len = GetClassNameW(fg, &mut cls_buf);
            let class_name = if len > 0 {
                String::from_utf16_lossy(&cls_buf[..len as usize])
            } else {
                String::new()
            };

            let style = GetWindowLongPtrW(fg, GWL_STYLE) as u32;
            let mut rect = RECT::default();
            if GetWindowRect(fg, &mut rect).is_ok() {
                Some(ForegroundWindowInfo {
                    visible,
                    is_own_process,
                    class_name,
                    style,
                    rect: PixelRect::new(rect.left, rect.top, rect.right, rect.bottom),
                })
            } else {
                None
            }
        };

        FullscreenSnapshot {
            notification_state,
            foreground,
            primary_monitor,
        }
    }
}

/// Detects fullscreen games / video players on primary monitor so taskbar can auto-hide.
/// Gathers Win32 state and delegates strictly to `coordinator::classify_fullscreen`.
pub fn is_fullscreen_active() -> bool {
    let snap = query_fullscreen_snapshot();
    classify_fullscreen(
        snap.notification_state,
        snap.foreground.as_ref(),
        snap.primary_monitor,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTransition {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinKeyAction {
    None,
    ToggleStart,
}

/// Pure, Win32-independent state machine for `WH_KEYBOARD_LL` lone-Win detection (AD-3).
/// Ignores injected events (`LLKHF_INJECTED` / `LLKHF_LOWER_IL_INJECTED`) and `VK_DUMMY_MASK` (`0xE8`),
/// tracks `VK_LWIN` and `VK_RWIN` independently, preserves auto-repeat combo state, and emits
/// `WinKeyAction::ToggleStart` strictly on a true lone Win press-and-release.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinKeyStateMachine {
    lwin_down: bool,
    rwin_down: bool,
    combo_used: bool,
}

impl WinKeyStateMachine {
    pub const fn new() -> Self {
        Self {
            lwin_down: false,
            rwin_down: false,
            combo_used: false,
        }
    }

    pub fn is_any_win_down(&self) -> bool {
        self.lwin_down || self.rwin_down
    }

    /// Reconciles tracked Win key flags against physical key state (e.g. after `Win+L`,
    /// UAC, or `Ctrl+Alt+Del` swallowed `Win Up` on the Secure Desktop).
    pub(crate) fn reconcile_physical_win_keys(
        &mut self,
        lwin_physically_down: bool,
        rwin_physically_down: bool,
    ) {
        if !lwin_physically_down {
            self.lwin_down = false;
        }
        if !rwin_physically_down {
            self.rwin_down = false;
        }
        if !self.lwin_down && !self.rwin_down {
            self.combo_used = false;
        }
    }

    pub(crate) fn mark_combo_used(&mut self) {
        if self.is_any_win_down() {
            self.combo_used = true;
        }
    }

    pub fn on_key_event(&mut self, vk: u16, transition: KeyTransition, flags: u32) -> WinKeyAction {
        if (flags & (LLKHF_INJECTED_FLAG | LLKHF_LOWER_IL_INJECTED_FLAG)) != 0
            || vk == VK_DUMMY_MASK
        {
            return WinKeyAction::None;
        }

        let alt_down = (flags & LLKHF_ALTDOWN_FLAG) != 0;

        if vk == VK_LWIN.0 || vk == VK_RWIN.0 {
            let (this_down, other_down) = if vk == VK_LWIN.0 {
                (&mut self.lwin_down, self.rwin_down)
            } else {
                (&mut self.rwin_down, self.lwin_down)
            };

            match transition {
                KeyTransition::Down => {
                    let was_this_down = *this_down;
                    *this_down = true;
                    if !was_this_down {
                        if other_down || alt_down {
                            // Dual-Win or Alt+Win chord: never treat as a lone Win tap.
                            self.combo_used = true;
                        } else {
                            // Fresh lone Win press from idle.
                            self.combo_used = false;
                        }
                    } else if alt_down {
                        self.combo_used = true;
                    }
                    // Auto-repeat (was_this_down == true) preserves existing combo_used state.
                    WinKeyAction::None
                }
                KeyTransition::Up => {
                    let was_this_down = *this_down;
                    *this_down = false;
                    if !was_this_down {
                        if !other_down {
                            self.combo_used = false;
                        }
                        return WinKeyAction::None;
                    }
                    if other_down {
                        // Other Win key is still held (dual-Win chord): stay marked as combo
                        // and wait until both Win keys have been released.
                        self.combo_used = true;
                        return WinKeyAction::None;
                    }
                    let was_combo = self.combo_used || alt_down;
                    self.combo_used = false;
                    if was_combo {
                        WinKeyAction::None
                    } else {
                        WinKeyAction::ToggleStart
                    }
                }
            }
        } else {
            // Only non-Win KeyDown while Win is held indicates a shortcut chord;
            // releasing a key pressed prior to Win (fast typing rollover) must not poison lone Win.
            if transition == KeyTransition::Down && self.is_any_win_down() {
                self.combo_used = true;
            }
            WinKeyAction::None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEnqueueResult {
    Sent,
    DroppedFull,
    Disconnected,
}

#[derive(Clone)]
pub struct HookEventSender {
    tx: mpsc::SyncSender<ShellHookEvent>,
    dropped_count: Arc<AtomicU64>,
}

impl HookEventSender {
    /// Non-blocking enqueue safe for `WH_KEYBOARD_LL` and `WH_MOUSE_LL` callbacks.
    pub fn try_send(&self, event: ShellHookEvent) -> HookEnqueueResult {
        match self.tx.try_send(event) {
            Ok(()) => HookEnqueueResult::Sent,
            Err(mpsc::TrySendError::Full(_)) => {
                self.dropped_count.fetch_add(1, Ordering::Relaxed);
                HookEnqueueResult::DroppedFull
            }
            Err(mpsc::TrySendError::Disconnected(_)) => HookEnqueueResult::Disconnected,
        }
    }

    /// Blocking send reserved strictly for post-unhook critical delivery (e.g. `EmergencyRestore`).
    pub fn send_blocking(&self, event: ShellHookEvent) -> Result<(), ShellHookEvent> {
        self.tx.send(event).map_err(|e| e.0)
    }

    pub fn dropped_count(&self) -> u64 {
        self.dropped_count.load(Ordering::Relaxed)
    }
}

/// Bounded hook event channel (`capacity = 64`) decoupling low-level Win32 hook callbacks
/// from coordinator/IPC dispatch on a dedicated worker thread (AD-3).
pub struct HookEventChannel {
    sender: HookEventSender,
    receiver: mpsc::Receiver<ShellHookEvent>,
}

impl Default for HookEventChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl HookEventChannel {
    pub fn new() -> Self {
        Self::with_capacity(HOOK_EVENT_CHANNEL_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let clamped_capacity = capacity.max(1);
        let (tx, receiver) = mpsc::sync_channel(clamped_capacity);
        let dropped_count = Arc::new(AtomicU64::new(0));
        Self {
            sender: HookEventSender { tx, dropped_count },
            receiver,
        }
    }

    pub fn sender(&self) -> HookEventSender {
        self.sender.clone()
    }

    pub fn dropped_count(&self) -> u64 {
        self.sender.dropped_count()
    }

    pub fn into_parts(self) -> (HookEventSender, mpsc::Receiver<ShellHookEvent>) {
        (self.sender, self.receiver)
    }

    /// Spawns a dedicated worker thread that drains events into `on_event`.
    /// Panics inside `on_event` are caught so the worker continues draining subsequent events
    /// and exits cleanly once all `HookEventSender` handles drop.
    pub fn spawn_worker<F>(self, on_event: F) -> (HookEventSender, std::thread::JoinHandle<()>)
    where
        F: Fn(ShellHookEvent) + Send + 'static,
    {
        let (sender, receiver) = self.into_parts();
        let handle = std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    on_event(event);
                }));
            }
        });
        (sender, handle)
    }
}

thread_local! {
    static HOOK_WIN_KEY_STATE: Cell<WinKeyStateMachine> = const { Cell::new(WinKeyStateMachine::new()) };
    static HOOK_EVENT_SENDER: RefCell<Option<HookEventSender>> = const { RefCell::new(None) };
}

struct HookThreadGuard;

impl Drop for HookThreadGuard {
    fn drop(&mut self) {
        let _ = HOOK_EVENT_SENDER.try_with(|slot| {
            if let Ok(mut guard) = slot.try_borrow_mut() {
                let _ = guard.take();
            }
        });
        let _ = HOOK_WIN_KEY_STATE.try_with(|state| state.set(WinKeyStateMachine::new()));
        HOOK_THREAD_ID.store(0, Ordering::SeqCst);
        HOOK_THREAD_ACTIVE.store(false, Ordering::SeqCst);
    }
}

/// Posts `WM_QUIT` to the active global shell hook thread so it unhooks cleanly and drops its sender.
pub(crate) fn stop_shell_hook_thread() {
    let tid = HOOK_THREAD_ID.load(Ordering::SeqCst);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Low-level keyboard & mouse hook thread that intercepts ONLY lone Win taps
/// (leaving Win+E/D/R/L/V/Shift+S/Tab untouched), forwards global mouse clicks via
/// non-blocking `try_send` into a bounded `sync_channel(64)` drained by a dedicated worker,
/// and registers global emergency restore hotkey `Ctrl+Alt+Shift+Esc`.
pub fn start_shell_hook_thread<F>(on_event: F)
where
    F: Fn(ShellHookEvent) + Send + 'static,
{
    if HOOK_THREAD_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let (sender, _worker_handle) = HookEventChannel::new().spawn_worker(on_event);
    std::thread::spawn(move || unsafe {
        let _active_guard = HookThreadGuard;
        HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
        let _ = HOOK_WIN_KEY_STATE.try_with(|state| state.set(WinKeyStateMachine::new()));
        let _ = HOOK_EVENT_SENDER.try_with(|slot| {
            *slot.borrow_mut() = Some(sender);
        });

        let hotkey_ok = RegisterHotKey(
            HWND::default(),
            HOTKEY_EMERGENCY_ID,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT,
            VK_ESCAPE.0 as u32,
        )
        .is_ok();

        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), None, 0);
        let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(ll_mouse_proc), None, 0);

        if !hotkey_ok && hook.is_err() && mouse_hook.is_err() {
            return;
        }

        let mut msg = MSG::default();
        let mut emergency_triggered = false;
        loop {
            let ret = GetMessageW(&mut msg, HWND::default(), 0, 0);
            if ret.0 <= 0 {
                break;
            }
            if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_EMERGENCY_ID {
                emergency_triggered = true;
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Unhook global hooks and hotkey BEFORE performing any taskbar restoration.
        if let Ok(h) = hook {
            let _ = UnhookWindowsHookEx(h);
        }
        if let Ok(mh) = mouse_hook {
            let _ = UnhookWindowsHookEx(mh);
        }
        if hotkey_ok {
            let _ = UnregisterHotKey(HWND::default(), HOTKEY_EMERGENCY_ID);
        }

        let sender = HOOK_EVENT_SENDER
            .try_with(|slot| slot.try_borrow_mut().ok().and_then(|mut g| g.take()))
            .ok()
            .flatten();
        let _ = HOOK_WIN_KEY_STATE.try_with(|state| state.set(WinKeyStateMachine::new()));

        if emergency_triggered {
            if NATIVE_HIDDEN.load(Ordering::SeqCst) {
                restore_native_taskbar_nonblocking();
            }
            if let Some(tx) = sender {
                let _ = tx.send_blocking(ShellHookEvent::EmergencyRestore);
            }
        }
    });
}

#[derive(Debug, Clone, Copy, Default)]
struct PhysicalKeySnapshot {
    lwin_down: bool,
    rwin_down: bool,
    modifier_down: bool,
}

fn is_vk_physically_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

fn query_physical_keys() -> PhysicalKeySnapshot {
    PhysicalKeySnapshot {
        lwin_down: is_vk_physically_down(VK_LWIN),
        rwin_down: is_vk_physically_down(VK_RWIN),
        modifier_down: is_vk_physically_down(VK_CONTROL)
            || is_vk_physically_down(VK_SHIFT)
            || is_vk_physically_down(VK_MENU),
    }
}

fn dispatch_ll_mouse_event(msg: u32, x: i32, y: i32, overlay_open: bool) {
    if msg != WM_LBUTTONDOWN && msg != WM_RBUTTONDOWN && msg != WM_MBUTTONDOWN {
        return;
    }
    if !overlay_open {
        return;
    }
    let _ = HOOK_EVENT_SENDER.try_with(|slot| {
        if let Ok(guard) = slot.try_borrow() {
            if let Some(sender) = guard.as_ref() {
                let _ = sender.try_send(ShellHookEvent::GlobalMouseDown(x, y));
            }
        }
    });
}

unsafe extern "system" fn ll_mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            dispatch_ll_mouse_event(
                wparam.0 as u32,
                ms.pt.x,
                ms.pt.y,
                HOOK_OVERLAY_OPEN.load(Ordering::Acquire),
            );
        }));
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

fn send_masked_win_keyup(vk: u16) {
    unsafe {
        // Inject [0xE8 DOWN, 0xE8 UP, vk KEYUP] in one atomic batch while swallowing the
        // physical Win KEYUP via LRESULT(1). This guarantees the mask key strictly precedes
        // the Win release seen by Explorer so native Windows Start never pops over TestBar.
        // All 3 events carry LLKHF_INJECTED so our WH_KEYBOARD_LL hook ignores them.
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(VK_DUMMY_MASK),
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(VK_DUMMY_MASK),
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        dwFlags: KEYEVENTF_KEYUP | KEYEVENTF_EXTENDEDKEY,
                        ..Default::default()
                    },
                },
            },
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Processes a single low-level keyboard event. Returns `true` ONLY when a lone physical
/// Win `KEYUP` triggered `ToggleStart` and was successfully enqueued + replaced with a
/// masked `[0xE8 down, 0xE8 up, Win up]` sequence, telling `ll_keyboard_proc` to return
/// `LRESULT(1)` and swallow the physical `KEYUP`.
fn dispatch_ll_keyboard_event<FQuery, FMask>(
    msg: u32,
    vk_code: u32,
    flags: u32,
    query_physical: FQuery,
    send_mask: FMask,
) -> bool
where
    FQuery: FnOnce() -> PhysicalKeySnapshot,
    FMask: FnOnce(u16),
{
    let transition = match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => KeyTransition::Down,
        WM_KEYUP | WM_SYSKEYUP => KeyTransition::Up,
        _ => return false,
    };
    let Ok(vk) = u16::try_from(vk_code) else {
        return false;
    };
    let is_injected_or_mask = (flags & (LLKHF_INJECTED_FLAG | LLKHF_LOWER_IL_INJECTED_FLAG)) != 0
        || vk == VK_DUMMY_MASK;
    let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;

    let action = HOOK_WIN_KEY_STATE
        .try_with(|cell| {
            let mut state = cell.get();
            if !is_injected_or_mask && transition == KeyTransition::Down {
                if state.is_any_win_down() || is_win {
                    let phys = query_physical();
                    if state.is_any_win_down() {
                        state.reconcile_physical_win_keys(phys.lwin_down, phys.rwin_down);
                    }
                    let act = state.on_key_event(vk, transition, flags);
                    if is_win && phys.modifier_down {
                        state.mark_combo_used();
                    }
                    cell.set(state);
                    return act;
                }
            }
            let act = state.on_key_event(vk, transition, flags);
            cell.set(state);
            act
        })
        .unwrap_or(WinKeyAction::None);

    if action == WinKeyAction::ToggleStart {
        // Enqueue ToggleStart first; only mask and swallow physical Win KEYUP if enqueued.
        let result = HOOK_EVENT_SENDER
            .try_with(|slot| {
                slot.try_borrow()
                    .ok()
                    .and_then(|guard| {
                        guard
                            .as_ref()
                            .map(|sender| sender.try_send(ShellHookEvent::ToggleStart))
                    })
                    .unwrap_or(HookEnqueueResult::Disconnected)
            })
            .unwrap_or(HookEnqueueResult::Disconnected);
        if result == HookEnqueueResult::Sent {
            send_mask(vk);
            return true;
        }
    }
    false
}

unsafe extern "system" fn ll_keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        let suppress = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            dispatch_ll_keyboard_event(
                wparam.0 as u32,
                kb.vkCode,
                kb.flags.0,
                query_physical_keys,
                send_masked_win_keyup,
            )
        }))
        .unwrap_or(false);
        if suppress {
            return LRESULT(1);
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetDesktopWindow, GetParent, SendMessageW, GA_PARENT, MA_ACTIVATE, WS_CHILD,
        WS_POPUP, WS_VISIBLE,
    };

    static SUBCLASS_TEST_LOCK: Mutex<()> = Mutex::new(());
    static TEST_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let seq = TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "testbar_{prefix}_{}_{}",
            std::process::id(),
            seq
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    struct RemoveFileOnDrop(PathBuf);
    impl Drop for RemoveFileOnDrop {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum FakeOp {
        HideNative,
        AbmNew(bool),
        AbmQueryPos(PixelRect),
        AbmSetPos(PixelRect),
        AbmRemove,
    }

    struct FakeShellState {
        monitor: PixelRect,
        dpi: u32,
        text_scale: f64,
        abm_new_results: VecDeque<bool>,
        querypos_bottom_offset: i32,
        ops: Vec<FakeOp>,
    }

    struct FakeAppBarShellOps {
        state: Arc<Mutex<FakeShellState>>,
    }

    impl AppBarShellOps for FakeAppBarShellOps {
        fn primary_monitor_rect(&self) -> PixelRect {
            self.state.lock().unwrap().monitor
        }

        fn primary_monitor_dpi(&self) -> u32 {
            self.state.lock().unwrap().dpi
        }

        fn text_scale_factor(&self) -> f64 {
            self.state.lock().unwrap().text_scale
        }

        fn hide_native_taskbar(&self) {
            self.state.lock().unwrap().ops.push(FakeOp::HideNative);
        }

        fn abm_new(&self, _hwnd: HWND, _abd: &mut APPBARDATA) -> bool {
            let mut st = self.state.lock().unwrap();
            let ok = st.abm_new_results.pop_front().unwrap_or(true);
            st.ops.push(FakeOp::AbmNew(ok));
            ok
        }

        fn abm_querypos(&self, _hwnd: HWND, abd: &mut APPBARDATA) {
            let mut st = self.state.lock().unwrap();
            abd.rc.bottom -= st.querypos_bottom_offset;
            let r = PixelRect::new(abd.rc.left, abd.rc.top, abd.rc.right, abd.rc.bottom);
            st.ops.push(FakeOp::AbmQueryPos(r));
        }

        fn abm_setpos(&self, _hwnd: HWND, abd: &mut APPBARDATA) {
            let mut st = self.state.lock().unwrap();
            let r = PixelRect::new(abd.rc.left, abd.rc.top, abd.rc.right, abd.rc.bottom);
            st.ops.push(FakeOp::AbmSetPos(r));
        }

        fn abm_remove(&self, _hwnd: HWND) {
            self.state.lock().unwrap().ops.push(FakeOp::AbmRemove);
        }

        fn on_appbar_hwnd_registered(&self, _hwnd: HWND) {}

        fn on_appbar_hwnd_removed(&self, _hwnd: HWND) {}
    }

    struct TestWindowGuard {
        hwnd: HWND,
        reset_subclass_statics: bool,
    }

    impl Drop for TestWindowGuard {
        fn drop(&mut self) {
            unsafe {
                if self.reset_subclass_statics {
                    let orig = FLYOUT_ORIG_WNDPROC.load(Ordering::SeqCst);
                    if orig != 0 {
                        let _ = SetWindowLongPtrW(self.hwnd, GWLP_WNDPROC, orig);
                    }
                    FLYOUT_SUBCLASSED_HWND.store(0, Ordering::SeqCst);
                    FLYOUT_ORIG_WNDPROC.store(0, Ordering::SeqCst);
                    FLYOUT_NON_ACTIVATING.store(false, Ordering::SeqCst);
                }
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }

    fn create_hidden_static_window() -> HWND {
        unsafe {
            let cls = wide("STATIC");
            CreateWindowExW(
                Default::default(),
                PCWSTR(cls.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                10,
                10,
                100,
                100,
                HWND::default(),
                None,
                None,
                None,
            )
            .expect("CreateWindowExW STATIC failed")
        }
    }

    #[test]
    fn position_window_updates_rect_without_showing_hidden_window() {
        let hwnd = create_hidden_static_window();
        let _guard = TestWindowGuard {
            hwnd,
            reset_subclass_statics: false,
        };

        unsafe {
            assert!(!IsWindowVisible(hwnd).as_bool());
            position_window(hwnd.0 as isize, 120, 140, 320, 240);
            assert!(
                !IsWindowVisible(hwnd).as_bool(),
                "position_window must never force a hidden window visible"
            );

            let mut rect = RECT::default();
            GetWindowRect(hwnd, &mut rect).expect("GetWindowRect failed");
            assert_eq!(rect.left, 120);
            assert_eq!(rect.top, 140);
            assert_eq!(rect.right, 440);
            assert_eq!(rect.bottom, 380);
        }
    }

    #[test]
    fn flyout_subclass_handles_wm_mouseactivate_by_mode() {
        let _lock = SUBCLASS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        FLYOUT_SUBCLASSED_HWND.store(0, Ordering::SeqCst);
        FLYOUT_ORIG_WNDPROC.store(0, Ordering::SeqCst);
        FLYOUT_NON_ACTIVATING.store(false, Ordering::SeqCst);

        let hwnd = create_hidden_static_window();
        let _guard = TestWindowGuard {
            hwnd,
            reset_subclass_statics: true,
        };
        let hwnd_raw = hwnd.0 as isize;

        install_flyout_noactivate_subclass(hwnd_raw);
        let orig_after_first = FLYOUT_ORIG_WNDPROC.load(Ordering::SeqCst);
        assert_ne!(orig_after_first, 0);
        assert_eq!(FLYOUT_SUBCLASSED_HWND.load(Ordering::SeqCst), hwnd_raw);

        // Idempotent re-install must not overwrite original WndProc with flyout_wnd_proc
        install_flyout_noactivate_subclass(hwnd_raw);
        assert_eq!(FLYOUT_ORIG_WNDPROC.load(Ordering::SeqCst), orig_after_first);

        unsafe {
            set_flyout_non_activating(true);
            let res_noact = SendMessageW(hwnd, WM_MOUSEACTIVATE, WPARAM(0), LPARAM(0));
            assert_eq!(res_noact, LRESULT(MA_NOACTIVATE as isize));

            set_flyout_non_activating(false);
            let res_act = SendMessageW(hwnd, WM_MOUSEACTIVATE, WPARAM(0), LPARAM(0));
            assert_eq!(res_act, LRESULT(MA_ACTIVATE as isize));
        }
    }

    #[test]
    fn single_instance_mutex_rejects_duplicate_and_reacquires_after_drop() {
        let name = format!(
            r"Local\TestBar_UnitTest_SingleInstance_{}_{}",
            std::process::id(),
            TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed)
        );

        assert!(SingleInstanceGuard::acquire_named("").is_err());
        assert!(SingleInstanceGuard::acquire_named("\0LeadingNul").is_err());
        assert!(SingleInstanceGuard::acquire_named("Local\\Bad\0Name").is_err());

        let first = SingleInstanceGuard::acquire_named(&name)
            .expect("first acquire should succeed")
            .expect("first acquire should own mutex");

        let second = SingleInstanceGuard::acquire_named(&name)
            .expect("second acquire should not return Win32 error");
        assert!(
            second.is_none(),
            "second acquire while first is alive must return Ok(None) on ERROR_ALREADY_EXISTS"
        );

        drop(first);

        let third = SingleInstanceGuard::acquire_named(&name)
            .expect("third acquire after drop should succeed");
        assert!(
            third.is_some(),
            "third acquire after first drops must succeed"
        );
    }

    #[test]
    fn workarea_snapshot_atomic_roundtrip_and_invalid_rejection() {
        let dir = unique_test_dir("workarea");
        let path = dir.join("recovery-workarea.json");

        let dpi_cases = [
            (96u32, PixelRect::new(0, 0, 1920, 1080), PixelRect::new(0, 0, 1920, 1032)),
            (120u32, PixelRect::new(0, 0, 2560, 1440), PixelRect::new(0, 0, 2560, 1380)),
            (144u32, PixelRect::new(0, 0, 2560, 1440), PixelRect::new(0, 0, 2560, 1368)),
            (192u32, PixelRect::new(0, 0, 3840, 2160), PixelRect::new(0, 0, 3840, 2064)),
        ];

        for (dpi, primary_monitor, work_area) in dpi_cases {
            let snap = WorkAreaSnapshot {
                work_area,
                primary_monitor,
                dpi,
            };
            save_workarea_snapshot_to(&path, &snap).expect("atomic save should succeed");
            let loaded = load_workarea_snapshot_from(&path)
                .expect("load_workarea_snapshot_from should return saved snapshot");
            assert_eq!(loaded, snap);

            // Verify no leftover .tmp files in directory after atomic rename
            let entries: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            assert_eq!(entries, vec!["recovery-workarea.json".to_string()]);
        }

        // Relative filename (parent() == Some("")) must succeed without failing create_dir_all("")
        let rel_path = PathBuf::from(format!(
            "testbar_rel_workarea_{}_{}.json",
            std::process::id(),
            TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _rel_guard = RemoveFileOnDrop(rel_path.clone());
        let rel_snap = WorkAreaSnapshot {
            work_area: PixelRect::new(0, 0, 1920, 1032),
            primary_monitor: PixelRect::new(0, 0, 1920, 1080),
            dpi: 96,
        };
        save_workarea_snapshot_to(&rel_path, &rel_snap)
            .expect("relative-path atomic save should succeed");
        assert_eq!(load_workarea_snapshot_from(&rel_path), Some(rel_snap));

        // Inverted, zero-DPI, or out-of-monitor snapshots are rejected on save and load
        let inverted_x = WorkAreaSnapshot {
            work_area: PixelRect::new(1920, 0, 1920, 1032),
            primary_monitor: PixelRect::new(0, 0, 1920, 1080),
            dpi: 96,
        };
        assert!(save_workarea_snapshot_to(&path, &inverted_x).is_err());

        let inverted_y = WorkAreaSnapshot {
            work_area: PixelRect::new(0, 1080, 1920, 1000),
            primary_monitor: PixelRect::new(0, 0, 1920, 1080),
            dpi: 96,
        };
        assert!(save_workarea_snapshot_to(&path, &inverted_y).is_err());

        let zero_dpi = WorkAreaSnapshot {
            work_area: PixelRect::new(0, 0, 1920, 1032),
            primary_monitor: PixelRect::new(0, 0, 1920, 1080),
            dpi: 0,
        };
        assert!(save_workarea_snapshot_to(&path, &zero_dpi).is_err());

        let exceeds_monitor = WorkAreaSnapshot {
            work_area: PixelRect::new(0, 0, 2560, 1368),
            primary_monitor: PixelRect::new(0, 0, 1920, 1080),
            dpi: 96,
        };
        assert!(!exceeds_monitor.is_valid());
        assert!(save_workarea_snapshot_to(&path, &exceeds_monitor).is_err());

        // Corrupted JSON, inverted rect, or out-of-monitor rect on disk returns None on load
        std::fs::write(&path, "{not valid json").unwrap();
        assert!(load_workarea_snapshot_from(&path).is_none());

        std::fs::write(&path, serde_json::to_string(&inverted_x).unwrap()).unwrap();
        assert!(load_workarea_snapshot_from(&path).is_none());

        std::fs::write(&path, serde_json::to_string(&exceeds_monitor).unwrap()).unwrap();
        assert!(load_workarea_snapshot_from(&path).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watchdog_cli_parsing_matrix() {
        let to_vec = |slice: &[&str]| -> Vec<String> { slice.iter().map(|s| s.to_string()).collect() };

        assert_eq!(parse_watchdog_args(&to_vec(&["testbar.exe"])), WatchdogCli::NotWatchdog);
        assert_eq!(
            parse_watchdog_args(&to_vec(&["testbar.exe", "--no-hide-native"])),
            WatchdogCli::NotWatchdog
        );

        assert_eq!(
            parse_watchdog_args(&to_vec(&["testbar.exe", "--watchdog", "4242"])),
            WatchdogCli::Run(WatchdogArgs {
                parent_pid: 4242,
                work_area: None,
            })
        );

        assert_eq!(
            parse_watchdog_args(&to_vec(&["testbar.exe", "--watchdog=4242"])),
            WatchdogCli::Run(WatchdogArgs {
                parent_pid: 4242,
                work_area: None,
            })
        );

        let expected_wa = PixelRect::new(0, 0, 2560, 1368);
        assert_eq!(
            parse_watchdog_args(&to_vec(&[
                "testbar.exe",
                "--watchdog",
                "4242",
                "--workarea",
                &format_workarea_arg(expected_wa),
            ])),
            WatchdogCli::Run(WatchdogArgs {
                parent_pid: 4242,
                work_area: Some(expected_wa),
            })
        );

        // --workarea before --watchdog and --watchdog=<pid> form
        assert_eq!(
            parse_watchdog_args(&to_vec(&[
                "testbar.exe",
                "--workarea=-1920,0,0,1032",
                "--watchdog=4242",
            ])),
            WatchdogCli::Run(WatchdogArgs {
                parent_pid: 4242,
                work_area: Some(PixelRect::new(-1920, 0, 0, 1032)),
            })
        );

        let invalid_cases: &[&[&str]] = &[
            &["testbar.exe", "--watchdog"],
            &["testbar.exe", "--watchdog="],
            &["testbar.exe", "--watchdog=abc"],
            &["testbar.exe", "--watchdog=0"],
            &["testbar.exe", "--watchdog", "abc"],
            &["testbar.exe", "--watchdog", "0"],
            &["testbar.exe", "--watchdog", "-5"],
            &["testbar.exe", "--watchdog", "4242", "--workarea"],
            &["testbar.exe", "--watchdog", "4242", "--workarea", "0,0,2560"],
            &["testbar.exe", "--watchdog", "4242", "--workarea", "0,0,abc,1368"],
            &["testbar.exe", "--watchdog", "4242", "--workarea", "2560,0,2560,1368"],
            &["testbar.exe", "--watchdog", "4242", "--workarea", "0,1368,2560,1368"],
            &["testbar.exe", "--watchdog", "4242", "--workarea", "500,100,400,900"],
            &["testbar.exe", "--watchdog", "4242", "--unexpected"],
            &["testbar.exe", "--no-hide-native", "--watchdog", "4242"],
            &["testbar.exe", "--watchdog", "4242", "--watchdog", "4242"],
            &["testbar.exe", "--watchdog=4242", "--watchdog=4242"],
            &[
                "testbar.exe",
                "--watchdog",
                "4242",
                "--workarea",
                "0,0,1920,1032",
                "--workarea",
                "0,0,1920,1000",
            ],
        ];
        for case in invalid_cases {
            assert!(
                matches!(parse_watchdog_args(&to_vec(case)), WatchdogCli::Invalid(_)),
                "expected Invalid for {case:?}"
            );
        }
    }

    #[test]
    fn recovery_workarea_resolution_priority_geometry_checks_and_crash_capture() {
        let dir = unique_test_dir("recovery_fallback");
        let path = dir.join("recovery-workarea.json");

        let mon_1440 = PixelRect::new(0, 0, 2560, 1440);
        let mon_1080 = PixelRect::new(0, 0, 1920, 1080);
        let disk_rect = PixelRect::new(0, 0, 2560, 1368);
        let mem_rect = PixelRect::new(0, 0, 2560, 1360);
        let cli_rect = PixelRect::new(0, 0, 2560, 1350);

        // All missing -> None (skip SPI_SETWORKAREA)
        assert_eq!(resolve_recovery_workarea(None, None, &path, mon_1440, 144), None);

        // Persisted disk snapshot used when CLI & in-memory are absent and monitor/DPI match
        let disk_snap = WorkAreaSnapshot {
            work_area: disk_rect,
            primary_monitor: mon_1440,
            dpi: 144,
        };
        save_workarea_snapshot_to(&path, &disk_snap).unwrap();
        assert_eq!(
            resolve_recovery_workarea(None, None, &path, mon_1440, 144),
            Some(disk_rect)
        );

        // Stale monitor geometry or DPI change rejects persisted snapshot
        assert_eq!(resolve_recovery_workarea(None, None, &path, mon_1080, 144), None);
        assert_eq!(resolve_recovery_workarea(None, None, &path, mon_1440, 96), None);

        // In-memory snapshot takes priority over disk when explicit is None
        let mem_snap = WorkAreaSnapshot {
            work_area: mem_rect,
            primary_monitor: mon_1440,
            dpi: 144,
        };
        assert_eq!(
            resolve_recovery_workarea(None, Some(mem_snap), &path, mon_1440, 144),
            Some(mem_rect)
        );

        // Invalid or mismatched in-memory snapshot falls back to valid disk snapshot
        let mismatched_mem = WorkAreaSnapshot {
            work_area: PixelRect::new(0, 0, 1920, 1032),
            primary_monitor: mon_1080,
            dpi: 96,
        };
        assert_eq!(
            resolve_recovery_workarea(None, Some(mismatched_mem), &path, mon_1440, 144),
            Some(disk_rect)
        );

        // Explicit CLI --workarea takes priority when contained by current monitor
        assert_eq!(
            resolve_recovery_workarea(Some(cli_rect), Some(mem_snap), &path, mon_1440, 144),
            Some(cli_rect)
        );

        // Explicit CLI --workarea outside current monitor is rejected and falls back
        let oversized_cli = PixelRect::new(0, 0, 3840, 2064);
        assert_eq!(
            resolve_recovery_workarea(Some(oversized_cli), None, &path, mon_1440, 144),
            Some(disk_rect)
        );
        assert_eq!(
            resolve_recovery_workarea(Some(PixelRect::new(100, 0, 100, 500)), None, &path, mon_1440, 144),
            Some(disk_rect)
        );

        // Crash-preservation capture selection: when Shell_TrayWnd is already hidden, preserve matching disk snapshot
        let polluted_current = WorkAreaSnapshot {
            work_area: mon_1440,
            primary_monitor: mon_1440,
            dpi: 144,
        };
        assert_eq!(
            select_capture_snapshot(polluted_current, false, Some(disk_snap)),
            (disk_snap, false)
        );
        // When Shell_TrayWnd IS visible, capture and persist fresh current snapshot
        assert_eq!(
            select_capture_snapshot(polluted_current, true, Some(disk_snap)),
            (polluted_current, true)
        );
        // When Shell_TrayWnd is hidden but disk snapshot is for a different monitor/DPI, overwrite with current
        assert_eq!(
            select_capture_snapshot(polluted_current, false, Some(mismatched_mem)),
            (polluted_current, true)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn top_level_appbar_message_window_reconciliation_retry_dedup_dpi_and_cleanup() {
        let (ev_tx, ev_rx) = mpsc::channel::<ShellAppBarEvent>();
        let mon = PixelRect::new(0, 0, 2560, 1440);
        let fake_state = Arc::new(Mutex::new(FakeShellState {
            monitor: mon,
            dpi: 144, // 150% DPI -> Dock 64*1.5=96px, Edge 52*1.5=78px
            text_scale: 1.0,
            abm_new_results: VecDeque::from([false, true, true]), // 1st ABM_NEW fails -> must retry!
            querypos_bottom_offset: 0,
            ops: Vec::new(),
        }));
        let fake_ops = Box::new(FakeAppBarShellOps {
            state: Arc::clone(&fake_state),
        });

        let mut wnd = AppBarMessageWindow::spawn_with_ops(BarMode::Dock, fake_ops, move |ev| {
            let _ = ev_tx.send(ev);
        })
        .expect("AppBarMessageWindow::spawn_with_ops failed");

        let hwnd = HWND(wnd.hwnd() as *mut _);
        assert_ne!(wnd.hwnd(), 0);
        assert_ne!(wnd.taskbar_created_msg(), 0);

        unsafe {
            assert!(IsWindow(hwnd).as_bool(), "window must exist while running");
            assert!(!IsWindowVisible(hwnd).as_bool(), "window must remain hidden");

            // Must be top-level (GetParent == null and ancestor parent == desktop, never HWND_MESSAGE)
            let parent_is_null = GetParent(hwnd).map(|p| p.0.is_null()).unwrap_or(true);
            assert!(parent_is_null, "window must have no parent (top-level)");
            assert_eq!(
                GetAncestor(hwnd, GA_PARENT),
                GetDesktopWindow(),
                "window must be a top-level desktop child, never HWND_MESSAGE"
            );

            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            assert_ne!(ex_style & WS_EX_TOOLWINDOW.0, 0, "must have WS_EX_TOOLWINDOW");
            assert_ne!(ex_style & WS_EX_NOACTIVATE.0, 0, "must have WS_EX_NOACTIVATE");

            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            assert_eq!(style & WS_CHILD.0, 0, "must not have WS_CHILD");
            assert_eq!(style & WS_VISIBLE.0, 0, "must not have WS_VISIBLE");

            // Initial ABM_NEW returned false -> no QueryPos/SetPos yet
            assert_eq!(fake_state.lock().unwrap().ops, vec![FakeOp::AbmNew(false)]);
            fake_state.lock().unwrap().ops.clear();

            // Next reconciliation retries ABM_NEW (which now succeeds) and reserves Dock 96px at 144 DPI
            PostMessageW(
                hwnd,
                WM_APPBAR_CALLBACK,
                WPARAM(ABN_POSCHANGED as usize),
                LPARAM(0),
            )
            .unwrap();
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::PositionChanged
            );
            let dock_144 = PixelRect::new(0, 1440 - 96, 2560, 1440);
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![
                    FakeOp::AbmNew(true),
                    FakeOp::AbmQueryPos(dock_144),
                    FakeOp::AbmSetPos(dock_144),
                ]
            );
            fake_state.lock().unwrap().ops.clear();

            // Repeated ABN_POSCHANGED with identical resolved rect deduplicates ABM_SETPOS
            PostMessageW(
                hwnd,
                WM_APPBAR_CALLBACK,
                WPARAM(ABN_POSCHANGED as usize),
                LPARAM(0),
            )
            .unwrap();
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::PositionChanged
            );
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![FakeOp::AbmQueryPos(dock_144)]
            );
            fake_state.lock().unwrap().ops.clear();

            // ABN_POSCHANGED with shifted bottom from QueryPos DOES call ABM_SETPOS
            fake_state.lock().unwrap().querypos_bottom_offset = 40;
            PostMessageW(
                hwnd,
                WM_APPBAR_CALLBACK,
                WPARAM(ABN_POSCHANGED as usize),
                LPARAM(0),
            )
            .unwrap();
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::PositionChanged
            );
            let shifted_dock = PixelRect::new(0, 1400 - 96, 2560, 1400);
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![
                    FakeOp::AbmQueryPos(PixelRect::new(0, 1440 - 96, 2560, 1400)),
                    FakeOp::AbmSetPos(shifted_dock),
                ]
            );
            fake_state.lock().unwrap().querypos_bottom_offset = 0;
            fake_state.lock().unwrap().ops.clear();

            // Live mode update to Edge switches height to 52 * 1.5 = 78px
            wnd.update_mode(BarMode::Edge);
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::PositionChanged
            );
            let edge_144 = PixelRect::new(0, 1440 - 78, 2560, 1440);
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![FakeOp::AbmQueryPos(edge_144), FakeOp::AbmSetPos(edge_144)]
            );
            fake_state.lock().unwrap().ops.clear();

            // WM_DPICHANGED propagates wparam DPI (192 = 200%) -> Edge 52 * 2 = 104px even while fake DPI is 144
            let suggested = RECT::default();
            let _ = SendMessageW(
                hwnd,
                WM_DPICHANGED,
                WPARAM((192 << 16) | 192),
                LPARAM(&suggested as *const RECT as isize),
            );
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::DpiChanged(192)
            );
            let edge_192 = PixelRect::new(0, 1440 - 104, 2560, 1440);
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![FakeOp::AbmQueryPos(edge_192), FakeOp::AbmSetPos(edge_192)]
            );
            fake_state.lock().unwrap().ops.clear();

            // ABN_FULLSCREENAPP emits FullscreenHint without mutating AppBar
            PostMessageW(
                hwnd,
                WM_APPBAR_CALLBACK,
                WPARAM(ABN_FULLSCREENAPP as usize),
                LPARAM(1),
            )
            .unwrap();
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::FullscreenHint(true)
            );
            assert!(fake_state.lock().unwrap().ops.is_empty());

            // WM_DISPLAYCHANGE reconciles AppBar and emits DisplayChanged
            PostMessageW(hwnd, WM_DISPLAYCHANGE, WPARAM(32), LPARAM(0)).unwrap();
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::DisplayChanged
            );
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![FakeOp::AbmQueryPos(edge_144), FakeOp::AbmSetPos(edge_144)]
            );
            fake_state.lock().unwrap().ops.clear();

            // TaskbarCreated re-hides native taskbars and forces ABM_NEW + QueryPos + SetPos
            PostMessageW(hwnd, wnd.taskbar_created_msg(), WPARAM(0), LPARAM(0)).unwrap();
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::TaskbarCreated
            );
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![
                    FakeOp::HideNative,
                    FakeOp::AbmNew(true),
                    FakeOp::AbmQueryPos(edge_144),
                    FakeOp::AbmSetPos(edge_144),
                ]
            );
            fake_state.lock().unwrap().ops.clear();
        }

        wnd.stop();
        assert_eq!(fake_state.lock().unwrap().ops, vec![FakeOp::AbmRemove]);
        unsafe {
            assert!(
                !IsWindow(hwnd).as_bool(),
                "window must be cleanly destroyed after stop()"
            );
        }
    }

    #[test]
    fn chained_panic_restore_helper_delegates_and_never_double_panics() {
        let order = std::cell::RefCell::new(Vec::new());
        invoke_chained_panic_restore(
            || order.borrow_mut().push("restore"),
            || order.borrow_mut().push("prev"),
        );
        assert_eq!(*order.borrow(), vec!["restore", "prev"]);

        let mut prev_called = false;
        invoke_chained_panic_restore(
            || panic!("simulated panic inside restore"),
            || prev_called = true,
        );
        assert!(
            prev_called,
            "previous panic hook must still execute when restore panics"
        );
    }

    #[test]
    fn primary_monitor_dpi_returns_valid_dpi() {
        let dpi = primary_monitor_dpi();
        assert!(dpi >= 96, "expected DPI >= 96, got {dpi}");
    }

    #[test]
    fn win_key_state_machine_lone_tap_repeat_and_stray_keyup() {
        let mut sm = WinKeyStateMachine::new();
        let lwin = VK_LWIN.0;
        let rwin = VK_RWIN.0;

        // Stray keyups from idle return None and keep idle state
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(rwin, KeyTransition::Up, 0), WinKeyAction::None);
        assert!(!sm.is_any_win_down());

        // Lone LWin tap
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert!(sm.is_any_win_down());
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::ToggleStart);
        assert!(!sm.is_any_win_down());

        // Lone RWin tap
        assert_eq!(sm.on_key_event(rwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(rwin, KeyTransition::Up, 0), WinKeyAction::ToggleStart);
        assert!(!sm.is_any_win_down());

        // Lone LWin held with OS key auto-repeat still emits ToggleStart on release
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::ToggleStart);
        assert!(!sm.is_any_win_down());
    }

    #[test]
    fn win_key_state_machine_preserves_os_chords_and_repeat_after_combo() {
        let mut sm = WinKeyStateMachine::new();
        let lwin = VK_LWIN.0;
        let rwin = VK_RWIN.0;

        // Standard Win+<key> shortcuts: Win+E/D/R/L/V/Tab
        for vk in [0x45u16, 0x44, 0x52, 0x4C, 0x56, 0x09] {
            assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
            assert_eq!(sm.on_key_event(vk, KeyTransition::Down, 0), WinKeyAction::None);
            assert_eq!(sm.on_key_event(vk, KeyTransition::Up, 0), WinKeyAction::None);
            assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::None);
        }

        // Win+Shift+S
        let shift = 0xA0u16; // VK_LSHIFT
        let key_s = 0x53u16;
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(shift, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_s, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_s, KeyTransition::Up, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(shift, KeyTransition::Up, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::None);

        // LWin released before E Up
        let key_e = 0x45u16;
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_e, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_e, KeyTransition::Up, 0), WinKeyAction::None);

        // LWin auto-repeat AFTER E Down must NEVER clear combo_used
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_e, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_e, KeyTransition::Up, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::None);

        // Next lone Win tap after chords succeeds cleanly
        assert_eq!(sm.on_key_event(rwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(rwin, KeyTransition::Up, 0), WinKeyAction::ToggleStart);
    }

    #[test]
    fn win_key_state_machine_ignores_injected_and_dummy_mask_and_handles_dual_win() {
        use windows::Win32::UI::WindowsAndMessaging::{LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED};

        assert_eq!(LLKHF_INJECTED_FLAG, LLKHF_INJECTED.0);
        assert_eq!(LLKHF_LOWER_IL_INJECTED_FLAG, LLKHF_LOWER_IL_INJECTED.0);

        let mut sm = WinKeyStateMachine::new();
        let lwin = VK_LWIN.0;
        let rwin = VK_RWIN.0;

        // Injected Win taps are completely ignored
        for inj_flag in [LLKHF_INJECTED_FLAG, LLKHF_LOWER_IL_INJECTED_FLAG] {
            assert_eq!(
                sm.on_key_event(lwin, KeyTransition::Down, inj_flag),
                WinKeyAction::None
            );
            assert!(!sm.is_any_win_down());
            assert_eq!(
                sm.on_key_event(lwin, KeyTransition::Up, inj_flag),
                WinKeyAction::None
            );
        }

        // Injected non-Win keys and 0xE8 mask key during a real lone Win hold do NOT poison it
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(0x45, KeyTransition::Down, LLKHF_INJECTED_FLAG),
            WinKeyAction::None
        );
        assert_eq!(
            sm.on_key_event(0x45, KeyTransition::Down, LLKHF_LOWER_IL_INJECTED_FLAG),
            WinKeyAction::None
        );
        assert_eq!(
            sm.on_key_event(VK_DUMMY_MASK, KeyTransition::Down, 0),
            WinKeyAction::None
        );
        assert_eq!(
            sm.on_key_event(VK_DUMMY_MASK, KeyTransition::Up, 0),
            WinKeyAction::None
        );
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::ToggleStart);

        // Dual-Win overlapping chords in every release order never trigger ToggleStart
        let dual_sequences = [
            [(lwin, KeyTransition::Down), (rwin, KeyTransition::Down), (lwin, KeyTransition::Up), (rwin, KeyTransition::Up)],
            [(lwin, KeyTransition::Down), (rwin, KeyTransition::Down), (rwin, KeyTransition::Up), (lwin, KeyTransition::Up)],
            [(rwin, KeyTransition::Down), (lwin, KeyTransition::Down), (rwin, KeyTransition::Up), (lwin, KeyTransition::Up)],
            [(rwin, KeyTransition::Down), (lwin, KeyTransition::Down), (lwin, KeyTransition::Up), (rwin, KeyTransition::Up)],
        ];
        for seq in dual_sequences {
            for (vk, tr) in seq {
                assert_eq!(sm.on_key_event(vk, tr, 0), WinKeyAction::None);
            }
            assert!(!sm.is_any_win_down());
            // State is never wedged: subsequent lone Win tap succeeds immediately
            assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
            assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::ToggleStart);
        }
    }

    #[test]
    fn hook_event_channel_burst_drop_fifo_drain_panic_survival_and_disconnect() {
        // 1. Burst > 64 events while receiver is not yet draining
        let channel = HookEventChannel::new();
        assert_eq!(HOOK_EVENT_CHANNEL_CAPACITY, 64);
        let producer = channel.sender();

        for i in 0..HOOK_EVENT_CHANNEL_CAPACITY {
            assert_eq!(
                producer.try_send(ShellHookEvent::GlobalMouseDown(i as i32, i as i32)),
                HookEnqueueResult::Sent
            );
        }
        assert_eq!(producer.dropped_count(), 0);

        // Excess events return DroppedFull immediately without blocking and increment dropped_count
        let excess = 25usize;
        for i in 0..excess {
            assert_eq!(
                producer.try_send(ShellHookEvent::GlobalMouseDown(1000 + i as i32, 0)),
                HookEnqueueResult::DroppedFull
            );
        }
        assert_eq!(producer.dropped_count(), excess as u64);
        assert_eq!(channel.dropped_count(), excess as u64);

        // Spawn worker to drain all 64 buffered events in exact FIFO order + blocking EmergencyRestore
        let drained = Arc::new(Mutex::new(Vec::new()));
        let drained_clone = Arc::clone(&drained);
        let (worker_sender, worker_handle) = channel.spawn_worker(move |ev| {
            drained_clone.lock().unwrap().push(ev);
        });

        // Post-unhook critical delivery via send_blocking succeeds even after a full burst
        worker_sender
            .send_blocking(ShellHookEvent::EmergencyRestore)
            .expect("send_blocking should deliver EmergencyRestore");

        drop(producer);
        drop(worker_sender);
        worker_handle
            .join()
            .expect("worker thread must exit cleanly when all senders drop");

        let events = drained.lock().unwrap();
        assert_eq!(events.len(), HOOK_EVENT_CHANNEL_CAPACITY + 1);
        for i in 0..HOOK_EVENT_CHANNEL_CAPACITY {
            assert_eq!(events[i], ShellHookEvent::GlobalMouseDown(i as i32, i as i32));
        }
        assert_eq!(events[HOOK_EVENT_CHANNEL_CAPACITY], ShellHookEvent::EmergencyRestore);

        // 2. Worker survives a panicking callback via catch_unwind and continues draining
        let panic_channel = HookEventChannel::new();
        let after_panic = Arc::new(Mutex::new(Vec::new()));
        let after_panic_clone = Arc::clone(&after_panic);
        let (tx, jh) = panic_channel.spawn_worker(move |ev| {
            if ev == ShellHookEvent::GlobalMouseDown(-1, -1) {
                panic!("simulated worker callback panic");
            }
            after_panic_clone.lock().unwrap().push(ev);
        });
        assert_eq!(tx.try_send(ShellHookEvent::ToggleStart), HookEnqueueResult::Sent);
        assert_eq!(
            tx.try_send(ShellHookEvent::GlobalMouseDown(-1, -1)),
            HookEnqueueResult::Sent
        );
        assert_eq!(
            tx.try_send(ShellHookEvent::EmergencyRestore),
            HookEnqueueResult::Sent
        );
        drop(tx);
        jh.join().expect("worker must survive panicking callback");
        assert_eq!(
            *after_panic.lock().unwrap(),
            vec![ShellHookEvent::ToggleStart, ShellHookEvent::EmergencyRestore]
        );

        // 3. Disconnected receiver returns Disconnected on try_send and Err on send_blocking
        let (dead_sender, rx) = HookEventChannel::new().into_parts();
        drop(rx);
        assert_eq!(
            dead_sender.try_send(ShellHookEvent::ToggleStart),
            HookEnqueueResult::Disconnected
        );
        assert_eq!(
            dead_sender.send_blocking(ShellHookEvent::EmergencyRestore),
            Err(ShellHookEvent::EmergencyRestore)
        );

        // 4. with_capacity(0) clamps to capacity 1 so try_send is never degenerate
        let (cap0_tx, cap0_rx) = HookEventChannel::with_capacity(0).into_parts();
        assert_eq!(
            cap0_tx.try_send(ShellHookEvent::ToggleStart),
            HookEnqueueResult::Sent
        );
        assert_eq!(
            cap0_tx.try_send(ShellHookEvent::ToggleStart),
            HookEnqueueResult::DroppedFull
        );
        assert_eq!(cap0_tx.dropped_count(), 1);
        assert_eq!(cap0_rx.recv().unwrap(), ShellHookEvent::ToggleStart);
    }

    #[test]
    fn win_key_state_machine_realistic_flags_rollover_altdown_and_reconcile() {
        use windows::Win32::UI::WindowsAndMessaging::{
            LLKHF_ALTDOWN, LLKHF_EXTENDED, LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED, LLKHF_UP,
        };

        assert_eq!(LLKHF_ALTDOWN_FLAG, LLKHF_ALTDOWN.0);
        let ext = LLKHF_EXTENDED.0;
        let up = LLKHF_UP.0;
        let alt = LLKHF_ALTDOWN.0;
        let inj = LLKHF_INJECTED.0;
        let low_inj = LLKHF_LOWER_IL_INJECTED.0;

        let mut sm = WinKeyStateMachine::new();
        let lwin = VK_LWIN.0;
        let rwin = VK_RWIN.0;
        let key_a = 0x41u16;
        let key_l = 0x4Cu16;

        // Realistic Win32 flags: LLKHF_EXTENDED on down, LLKHF_EXTENDED | LLKHF_UP on up
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(lwin, KeyTransition::Up, ext | up),
            WinKeyAction::ToggleStart
        );

        // Combined injected bitmasks (LLKHF_EXTENDED | LLKHF_INJECTED, etc.) are completely ignored
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(key_a, KeyTransition::Down, ext | inj),
            WinKeyAction::None
        );
        assert_eq!(
            sm.on_key_event(lwin, KeyTransition::Up, ext | up | inj),
            WinKeyAction::None
        );
        assert_eq!(
            sm.on_key_event(key_a, KeyTransition::Down, up | low_inj),
            WinKeyAction::None
        );
        assert_eq!(
            sm.on_key_event(lwin, KeyTransition::Up, ext | up),
            WinKeyAction::ToggleStart
        );

        // Typing rollover: A Down -> LWin Down -> A Up -> LWin Up still triggers ToggleStart
        assert_eq!(sm.on_key_event(key_a, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_a, KeyTransition::Up, up), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(lwin, KeyTransition::Up, ext | up),
            WinKeyAction::ToggleStart
        );

        // Alt+Win via LLKHF_ALTDOWN on Win Down or Win Up never triggers ToggleStart
        assert_eq!(
            sm.on_key_event(lwin, KeyTransition::Down, ext | alt),
            WinKeyAction::None
        );
        assert_eq!(
            sm.on_key_event(lwin, KeyTransition::Up, ext | up | alt),
            WinKeyAction::None
        );
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(lwin, KeyTransition::Up, ext | up | alt),
            WinKeyAction::None
        );

        // Win+L loses LWin Up on Secure Desktop -> reconcile_physical_win_keys clears stuck state
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_l, KeyTransition::Down, 0), WinKeyAction::None);
        assert!(sm.is_any_win_down());
        sm.reconcile_physical_win_keys(false, false);
        assert!(!sm.is_any_win_down());

        // Both LWin and RWin immediately work again after reconciliation
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(lwin, KeyTransition::Up, ext | up),
            WinKeyAction::ToggleStart
        );
        assert_eq!(sm.on_key_event(rwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(rwin, KeyTransition::Up, ext | up),
            WinKeyAction::ToggleStart
        );
    }

    #[test]
    fn ll_keyboard_and_mouse_dispatch_wiring_mask_order_modifiers_reconcile_and_overlay_gate() {
        use windows::Win32::UI::WindowsAndMessaging::{
            LLKHF_ALTDOWN, LLKHF_EXTENDED, LLKHF_UP, WM_MOUSEMOVE,
        };

        struct HookTlsResetGuard;
        impl Drop for HookTlsResetGuard {
            fn drop(&mut self) {
                let _ = HOOK_EVENT_SENDER.try_with(|slot| {
                    if let Ok(mut g) = slot.try_borrow_mut() {
                        *g = None;
                    }
                });
                let _ = HOOK_WIN_KEY_STATE.try_with(|c| c.set(WinKeyStateMachine::new()));
            }
        }
        let _reset = HookTlsResetGuard;

        let ext = LLKHF_EXTENDED.0;
        let up = LLKHF_UP.0;
        let alt = LLKHF_ALTDOWN.0;
        let lwin = VK_LWIN.0 as u32;
        let rwin = VK_RWIN.0 as u32;
        let key_l = 0x4Cu32;

        let (sender, rx) = HookEventChannel::with_capacity(2).into_parts();
        HOOK_WIN_KEY_STATE.with(|c| c.set(WinKeyStateMachine::new()));
        HOOK_EVENT_SENDER.with(|slot| *slot.borrow_mut() = Some(sender.clone()));

        let idle_phys = || PhysicalKeySnapshot::default();
        let masked_vks: RefCell<Vec<u16>> = RefCell::new(Vec::new());
        let record_mask = |vk: u16| masked_vks.borrow_mut().push(vk);

        // 1. Lone LWin tap: Down passes through (false), Up enqueues ToggleStart, injects mask+keyup, and swallows physical KEYUP (true)
        assert!(
            !dispatch_ll_keyboard_event(WM_KEYDOWN, lwin, ext, idle_phys, record_mask),
            "Win KEYDOWN must pass through via CallNextHookEx"
        );
        assert!(masked_vks.borrow().is_empty());
        assert!(
            dispatch_ll_keyboard_event(WM_KEYUP, lwin, ext | up, idle_phys, record_mask),
            "lone LWin KEYUP must return suppress=true (LRESULT(1)) after sending masked sequence"
        );
        assert_eq!(*masked_vks.borrow(), vec![VK_LWIN.0]);
        assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::ToggleStart);

        // 1b. Win+E chord passes completely untouched (suppress=false on every event, no mask key)
        let key_e = 0x45u32;
        assert!(!dispatch_ll_keyboard_event(WM_KEYDOWN, lwin, ext, idle_phys, record_mask));
        assert!(!dispatch_ll_keyboard_event(
            WM_KEYDOWN,
            key_e,
            0,
            || PhysicalKeySnapshot {
                lwin_down: true,
                rwin_down: false,
                modifier_down: false,
            },
            record_mask,
        ));
        assert!(!dispatch_ll_keyboard_event(WM_KEYUP, key_e, up, idle_phys, record_mask));
        assert!(
            !dispatch_ll_keyboard_event(WM_KEYUP, lwin, ext | up, idle_phys, record_mask),
            "Win+E chord KEYUP must NOT be swallowed"
        );
        assert_eq!(*masked_vks.borrow(), vec![VK_LWIN.0]);
        assert!(rx.try_recv().is_err());

        // 2. WM_SYSKEYDOWN / WM_SYSKEYUP with LLKHF_ALTDOWN never triggers ToggleStart, mask, or swallow
        assert!(!dispatch_ll_keyboard_event(WM_SYSKEYDOWN, lwin, ext | alt, idle_phys, record_mask));
        assert!(!dispatch_ll_keyboard_event(WM_SYSKEYUP, lwin, ext | up | alt, idle_phys, record_mask));
        assert_eq!(masked_vks.borrow().len(), 1);
        assert!(rx.try_recv().is_err());

        // 3. Pre-held Ctrl/Shift/Alt detected via physical key snapshot suppresses lone Win
        assert!(!dispatch_ll_keyboard_event(
            WM_KEYDOWN,
            lwin,
            ext,
            || PhysicalKeySnapshot {
                lwin_down: false,
                rwin_down: false,
                modifier_down: true,
            },
            record_mask,
        ));
        assert!(!dispatch_ll_keyboard_event(WM_KEYUP, lwin, ext | up, idle_phys, record_mask));
        assert_eq!(masked_vks.borrow().len(), 1);
        assert!(rx.try_recv().is_err());

        // 4. Win+L Secure Desktop lost LWin Up: next RWin tap reconciles physical state and swallows RWin KEYUP
        assert!(!dispatch_ll_keyboard_event(WM_KEYDOWN, lwin, ext, idle_phys, record_mask));
        assert!(!dispatch_ll_keyboard_event(
            WM_KEYDOWN,
            key_l,
            0,
            || PhysicalKeySnapshot {
                lwin_down: true,
                rwin_down: false,
                modifier_down: false,
            },
            record_mask,
        ));
        // LWin Up is lost; user unlocks desktop and taps RWin -> physical LWin is up -> succeeds with VK_RWIN!
        assert!(!dispatch_ll_keyboard_event(WM_KEYDOWN, rwin, ext, idle_phys, record_mask));
        assert!(
            dispatch_ll_keyboard_event(WM_KEYUP, rwin, ext | up, idle_phys, record_mask),
            "lone RWin KEYUP must return suppress=true (LRESULT(1))"
        );
        assert_eq!(*masked_vks.borrow(), vec![VK_LWIN.0, VK_RWIN.0]);
        assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::ToggleStart);

        // 5. Out-of-range vkCode (> u16::MAX) is ignored safely without truncation
        assert!(!dispatch_ll_keyboard_event(WM_KEYDOWN, (1u32 << 16) | lwin, ext, idle_phys, record_mask));
        assert!(!dispatch_ll_keyboard_event(WM_KEYUP, (1u32 << 16) | lwin, ext | up, idle_phys, record_mask));
        assert_eq!(masked_vks.borrow().len(), 2);
        assert!(rx.try_recv().is_err());

        // 6. When channel is full, try_send returns DroppedFull -> mask key NOT invoked and KEYUP NOT swallowed
        assert_eq!(
            sender.try_send(ShellHookEvent::GlobalMouseDown(1, 1)),
            HookEnqueueResult::Sent
        );
        assert_eq!(
            sender.try_send(ShellHookEvent::GlobalMouseDown(2, 2)),
            HookEnqueueResult::Sent
        );
        assert!(!dispatch_ll_keyboard_event(WM_KEYDOWN, lwin, ext, idle_phys, record_mask));
        assert!(
            !dispatch_ll_keyboard_event(WM_KEYUP, lwin, ext | up, idle_phys, record_mask),
            "physical Win KEYUP must NOT be swallowed when ToggleStart was dropped on full channel"
        );
        assert_eq!(
            masked_vks.borrow().len(),
            2,
            "send_masked_win_keyup must NOT run when ToggleStart was dropped on full channel"
        );
        assert_eq!(sender.dropped_count(), 1);
        let _ = rx.try_recv().unwrap();
        let _ = rx.try_recv().unwrap();

        // 7. Mouse dispatch is gated by overlay_open across L/R/M button downs
        dispatch_ll_mouse_event(WM_LBUTTONDOWN, 10, 20, false);
        dispatch_ll_mouse_event(WM_RBUTTONDOWN, 30, 40, false);
        dispatch_ll_mouse_event(WM_MBUTTONDOWN, 50, 60, false);
        assert!(rx.try_recv().is_err());

        dispatch_ll_mouse_event(WM_MOUSEMOVE, 99, 99, true); // non-click ignored
        dispatch_ll_mouse_event(WM_LBUTTONDOWN, 10, 20, true);
        dispatch_ll_mouse_event(WM_RBUTTONDOWN, 30, 40, true);
        assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::GlobalMouseDown(10, 20));
        assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::GlobalMouseDown(30, 40));
        dispatch_ll_mouse_event(WM_MBUTTONDOWN, 50, 60, true);
        assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::GlobalMouseDown(50, 60));
    }

    #[test]
    fn parse_text_scale_factor_clamping_and_live_registry_probe() {
        assert!((parse_text_scale_factor(None) - 1.0).abs() < 1e-9);
        assert!((parse_text_scale_factor(Some(0)) - 1.0).abs() < 1e-9);
        assert!((parse_text_scale_factor(Some(50)) - 1.0).abs() < 1e-9);
        assert!((parse_text_scale_factor(Some(100)) - 1.0).abs() < 1e-9);
        assert!((parse_text_scale_factor(Some(140)) - 1.4).abs() < 1e-9);
        assert!((parse_text_scale_factor(Some(175)) - 1.75).abs() < 1e-9);
        assert!((parse_text_scale_factor(Some(225)) - 2.25).abs() < 1e-9);
        assert!((parse_text_scale_factor(Some(400)) - 2.25).abs() < 1e-9);

        let live = windows_text_scale_factor();
        assert!(
            (1.0..=2.25).contains(&live),
            "expected TextScaleFactor in 1.0..=2.25, got {live}"
        );
    }

    #[test]
    fn appbar_message_window_reserves_effective_scale_with_text_scale_factor() {
        let (ev_tx, ev_rx) = mpsc::channel::<ShellAppBarEvent>();
        let mon = PixelRect::new(0, 0, 2560, 1440);
        let fake_state = Arc::new(Mutex::new(FakeShellState {
            monitor: mon,
            dpi: 96,         // 100% DPI
            text_scale: 1.4, // 140% TextScaleFactor -> effective_scale = 1.4
            abm_new_results: VecDeque::from([true]),
            querypos_bottom_offset: 0,
            ops: Vec::new(),
        }));
        let fake_ops = Box::new(FakeAppBarShellOps {
            state: Arc::clone(&fake_state),
        });

        let mut wnd = AppBarMessageWindow::spawn_with_ops(BarMode::Dock, fake_ops, move |ev| {
            let _ = ev_tx.send(ev);
        })
        .expect("spawn_with_ops failed");

        // Dock 64 * 1.4 = 89.6 -> 90px
        let expected_dock = PixelRect::new(0, 1440 - 90, 2560, 1440);
        assert_eq!(
            fake_state.lock().unwrap().ops,
            vec![
                FakeOp::AbmNew(true),
                FakeOp::AbmQueryPos(expected_dock),
                FakeOp::AbmSetPos(expected_dock),
            ]
        );
        fake_state.lock().unwrap().ops.clear();

        // Edge 52 * 1.4 = 72.8 -> 73px
        wnd.update_mode(BarMode::Edge);
        assert_eq!(
            ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            ShellAppBarEvent::PositionChanged
        );
        let expected_edge = PixelRect::new(0, 1440 - 73, 2560, 1440);
        assert_eq!(
            fake_state.lock().unwrap().ops,
            vec![FakeOp::AbmQueryPos(expected_edge), FakeOp::AbmSetPos(expected_edge)]
        );

        wnd.stop();
    }
}
