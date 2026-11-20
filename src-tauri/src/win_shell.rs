use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::ffi::{c_void, OsStr};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, BOOL, ERROR_ALREADY_EXISTS,
    ERROR_CLASS_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcessId, OpenProcess, WaitForSingleObject, INFINITE,
    PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, SendInput, UnregisterHotKey, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VIRTUAL_KEY, VK_ESCAPE, VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHQueryUserNotificationState, ABE_BOTTOM, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
    ABM_SETPOS, ABN_FULLSCREENAPP, ABN_POSCHANGED, APPBARDATA, QUNS_ACCEPTS_NOTIFICATIONS,
    QUNS_APP, QUNS_BUSY, QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME,
    QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow,
    DispatchMessageW, EnumWindows, FindWindowW, GetClassNameW, GetForegroundWindow, GetMessageW,
    GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic,
    IsWindow, IsWindowVisible, PostMessageW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    SetWindowsHookExW, ShowWindow, SystemParametersInfoW, TranslateMessage, UnhookWindowsHookEx,
    CREATESTRUCTW, GWLP_USERDATA, GWLP_WNDPROC, GWL_EXSTYLE, GWL_STYLE, HHOOK, HWND_TOPMOST,
    KBDLLHOOKSTRUCT, MA_NOACTIVATE, MSG, MSLLHOOKSTRUCT, SM_CXSCREEN, SM_CYSCREEN, SPIF_SENDCHANGE,
    SPI_GETWORKAREA, SPI_SETWORKAREA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_NCCREATE, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WNDCLASSEXW, WNDPROC, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPED,
};

use crate::coordinator::{
    classify_fullscreen, compute_appbar_rect, BarMode, ForegroundWindowInfo, NotificationState,
    PixelRect,
};

pub const SINGLE_INSTANCE_MUTEX_NAME: &str = r"Local\TestBar_SingleInstance_v1";
pub const WM_APPBAR_CALLBACK: u32 = WM_APP + 10;
pub const WM_APPBAR_UPDATE_MODE: u32 = WM_APP + 12;

const APPBAR_MSG_WND_CLASS: &str = "TestBarAppBarMsgWnd";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HOTKEY_EMERGENCY_ID: i32 = 0x7B01;

static APPBAR_HWND: AtomicIsize = AtomicIsize::new(0);
static NATIVE_HIDDEN: AtomicBool = AtomicBool::new(false);
static RESTORE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static TEMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);
static ORIGINAL_WORKAREA: Mutex<Option<WorkAreaSnapshot>> = Mutex::new(None);
static APPBAR_CLASS_REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
static APPBAR_MSG_WINDOW: Mutex<Option<AppBarMessageWindow>> = Mutex::new(None);

static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static WIN_COMBO_USED: AtomicBool = AtomicBool::new(false);
static HOOK_CALLBACK: OnceLock<Box<dyn Fn(ShellHookEvent) + Send + Sync>> = OnceLock::new();
static FLYOUT_SUBCLASSED_HWND: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_ORIG_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_NON_ACTIVATING: AtomicBool = AtomicBool::new(false);

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

#[derive(Debug, Clone, Copy)]
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
        let mon = primary_monitor_rect();
        let snap = WorkAreaSnapshot {
            work_area: PixelRect::new(wa.left, wa.top, wa.right, wa.bottom),
            primary_monitor: PixelRect::new(mon.left, mon.top, mon.right, mon.bottom),
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
    let mon = primary_monitor_rect();
    let mon_px = PixelRect::new(mon.left, mon.top, mon.right, mon.bottom);
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

#[derive(Debug, Clone, Copy)]
pub struct MonitorRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub width: i32,
    pub height: i32,
}

pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
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
        }
    }
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
        let cur_tid = windows::Win32::System::Threading::GetCurrentThreadId();
        let target_tid = GetWindowThreadProcessId(hwnd, None);
        if fg_tid != cur_tid {
            let _ = windows::Win32::System::Threading::AttachThreadInput(cur_tid, fg_tid, true);
        }
        if target_tid != cur_tid {
            let _ = windows::Win32::System::Threading::AttachThreadInput(cur_tid, target_tid, true);
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        if target_tid != cur_tid {
            let _ = windows::Win32::System::Threading::AttachThreadInput(cur_tid, target_tid, false);
        }
        if fg_tid != cur_tid {
            let _ = windows::Win32::System::Threading::AttachThreadInput(cur_tid, fg_tid, false);
        }
    }
}

pub fn point_inside_hwnd(hwnd_raw: isize, x: i32, y: i32) -> bool {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_ok() {
            return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
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

/// Safely hides Windows 11 native taskbar(s) without injecting or terminating explorer.exe.
pub fn hide_native_taskbar() {
    if RESTORE_IN_PROGRESS.load(Ordering::SeqCst) {
        return;
    }
    unsafe {
        let cls = wide("Shell_TrayWnd");
        if let Ok(hwnd) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) {
            if !hwnd.0.is_null() {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        // Also hide secondary monitor taskbars
        extern "system" fn hide_secondary(hwnd: HWND, _: LPARAM) -> BOOL {
            unsafe {
                let mut buf = [0u16; 64];
                let len = GetClassNameW(hwnd, &mut buf);
                if len > 0 {
                    let name = String::from_utf16_lossy(&buf[..len as usize]);
                    if name == "Shell_SecondaryTrayWnd" {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
            BOOL(1)
        }
        let _ = EnumWindows(Some(hide_secondary), LPARAM(0));
        NATIVE_HIDDEN.store(true, Ordering::SeqCst);
    }
}

trait AppBarShellOps: Send {
    fn primary_monitor_rect(&self) -> PixelRect;
    fn primary_monitor_dpi(&self) -> u32;
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
        let mon = primary_monitor_rect();
        PixelRect::new(mon.left, mon.top, mon.right, mon.bottom)
    }

    fn primary_monitor_dpi(&self) -> u32 {
        primary_monitor_dpi()
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
    let target = compute_appbar_rect(mon_px, dpi, ctx.bar_mode.get());
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
    unsafe {
        let cls = wide("Shell_TrayWnd");
        if let Ok(hwnd) = FindWindowW(PCWSTR(cls.as_ptr()), PCWSTR::null()) {
            if !hwnd.0.is_null() {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
        }
        extern "system" fn show_secondary(hwnd: HWND, _: LPARAM) -> BOOL {
            unsafe {
                let mut buf = [0u16; 64];
                let len = GetClassNameW(hwnd, &mut buf);
                if len > 0 {
                    let name = String::from_utf16_lossy(&buf[..len as usize]);
                    if name == "Shell_SecondaryTrayWnd" {
                        let _ = ShowWindow(hwnd, SW_SHOW);
                    }
                }
            }
            BOOL(1)
        }
        let _ = EnumWindows(Some(show_secondary), LPARAM(0));

        if let Some(wa_rect) = get_recovery_workarea(explicit_workarea) {
            let mut wa = RECT {
                left: wa_rect.left,
                top: wa_rect.top,
                right: wa_rect.right,
                bottom: wa_rect.bottom,
            };
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

/// Detects fullscreen games / video players on primary monitor so taskbar can auto-hide.
/// Gathers Win32 state and delegates strictly to `coordinator::classify_fullscreen`.
pub fn is_fullscreen_active() -> bool {
    unsafe {
        let notif_state = match SHQueryUserNotificationState() {
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

        let mon = primary_monitor_rect();
        let mon_rect = PixelRect::new(mon.left, mon.top, mon.right, mon.bottom);

        let fg = GetForegroundWindow();
        let fg_info = if fg.0.is_null() {
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

        classify_fullscreen(notif_state, fg_info.as_ref(), mon_rect)
    }
}

/// Low-level keyboard hook that intercepts ONLY lone Win tap (leaving Win+E/D/R/L/V/Shift+S untouched)
/// and registers global emergency restore hotkey `Ctrl+Alt+Shift+Esc`.
pub fn start_shell_hook_thread<F>(on_event: F)
where
    F: Fn(ShellHookEvent) + Send + Sync + 'static,
{
    let _ = HOOK_CALLBACK.set(Box::new(on_event));
    std::thread::spawn(|| unsafe {
        let _ = RegisterHotKey(
            HWND::default(),
            HOTKEY_EMERGENCY_ID,
            MOD_CONTROL | MOD_ALT | MOD_SHIFT,
            VK_ESCAPE.0 as u32,
        );

        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), None, 0);
        let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(ll_mouse_proc), None, 0);
        let mut msg = MSG::default();
        let mut emergency_triggered = false;
        while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
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
        let _ = UnregisterHotKey(HWND::default(), HOTKEY_EMERGENCY_ID);

        if emergency_triggered {
            if NATIVE_HIDDEN.load(Ordering::SeqCst) {
                restore_native_taskbar_nonblocking();
            }
            if let Some(cb) = HOOK_CALLBACK.get() {
                cb(ShellHookEvent::EmergencyRestore);
            }
        }
    });
}

unsafe extern "system" fn ll_mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let msg = wparam.0 as u32;
        if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN {
            let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            if let Some(cb) = HOOK_CALLBACK.get() {
                cb(ShellHookEvent::GlobalMouseDown(ms.pt.x, ms.pt.y));
            }
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

fn send_dummy_mask_key() {
    unsafe {
        // Masking key (0xE8 unassigned) prevents Windows shell from opening native Start
        // when releasing a lone Win key, without affecting any other application.
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0xE8),
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0xE8),
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

unsafe extern "system" fn ll_keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode as u16;
        let msg = wparam.0 as u32;
        let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;

        if is_win {
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                if !WIN_DOWN.swap(true, Ordering::SeqCst) {
                    WIN_COMBO_USED.store(false, Ordering::SeqCst);
                }
            } else if msg == WM_KEYUP || msg == WM_SYSKEYUP {
                let was_down = WIN_DOWN.swap(false, Ordering::SeqCst);
                let combo = WIN_COMBO_USED.swap(false, Ordering::SeqCst);
                if was_down && !combo {
                    // Lone Win tap detected! Suppress native Start via mask key & toggle TestBar Start.
                    send_dummy_mask_key();
                    if let Some(cb) = HOOK_CALLBACK.get() {
                        cb(ShellHookEvent::ToggleStart);
                    }
                }
            }
        } else if WIN_DOWN.load(Ordering::SeqCst) && vk != 0xE8 {
            // Any other key pressed while Win is held -> OS shortcut (Win+E, Win+D, Win+Shift+S, etc.)
            WIN_COMBO_USED.store(true, Ordering::SeqCst);
        }
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

#[allow(dead_code)]
pub fn create_hidden_message_window(class_name: &str) -> Option<HWND> {
    unsafe {
        let cls = wide(class_name);
        CreateWindowExW(
            Default::default(),
            PCWSTR(cls.as_ptr()),
            PCWSTR::null(),
            Default::default(),
            0,
            0,
            0,
            0,
            HWND::default(),
            None,
            None,
            None,
        )
        .ok()
    }
}

#[allow(dead_code)]
pub fn post_close_to_hwnd(hwnd_raw: isize) {
    unsafe {
        let _ = PostMessageW(HWND(hwnd_raw as *mut _), windows::Win32::UI::WindowsAndMessaging::WM_CLOSE, WPARAM(0), LPARAM(0));
    }
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
}
