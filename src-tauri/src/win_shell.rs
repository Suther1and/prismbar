use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_void, OsStr};
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, SetLastError, BOOL, ERROR_ALREADY_EXISTS,
    ERROR_CLASS_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT,
    WAIT_OBJECT_0, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DwmFlush, DwmGetWindowAttribute, DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR,
    DWMWA_CLOAKED, DWMWA_COLOR_NONE, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_NCRENDERING_POLICY,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HDC, HMONITOR,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::Threading::{
    AttachThreadInput, CreateEventW, CreateMutexW, GetCurrentProcessId, GetCurrentThreadId,
    OpenEventW, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, SetEvent,
    WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, SYNCHRONIZATION_ACCESS_RIGHTS,
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
    AllowSetForegroundWindow, CallNextHookEx, CallWindowProcW, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, EnumWindows, FindWindowW, GetClassNameW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetShellWindow, GetSystemMetrics, GetWindowLongPtrW,
    GetWindowRect, GetWindowThreadProcessId, IsHungAppWindow, IsIconic, IsWindow,
    IsWindowVisible, IsZoomed, PostMessageW, PostQuitMessage, PostThreadMessageW,
    RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowPos, SetWindowsHookExW, ShowWindow, ShowWindowAsync, SystemParametersInfoW,
    TranslateMessage, UnhookWindowsHookEx, ASFW_ANY, CREATESTRUCTW, GWLP_USERDATA, GWLP_WNDPROC,
    GWL_EXSTYLE, GWL_STYLE, HC_ACTION, HHOOK, HWND_TOPMOST, KBDLLHOOKSTRUCT, MA_NOACTIVATE,
    MONITORINFOF_PRIMARY, MSG, MSLLHOOKSTRUCT, SHOW_WINDOW_CMD, SM_CXSCREEN, SM_CYSCREEN,
    SPIF_SENDCHANGE, SPI_GETWORKAREA, SPI_SETWORKAREA, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOCOPYBITS, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED,
    WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEACTIVATE,
    WM_NCACTIVATE, WM_NCCREATE, WM_NCDESTROY, WM_NCPAINT, WM_QUIT, WM_RBUTTONDOWN,
    WM_SETTINGCHANGE, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSEXW, WNDPROC, WS_BORDER, WS_CAPTION,
    WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_DLGFRAME, WS_EX_APPWINDOW, WS_EX_CLIENTEDGE,
    WS_EX_DLGMODALFRAME, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_POPUP, WS_SYSMENU,
    WS_THICKFRAME,
};

pub use crate::coordinator::{FLYOUT_ROOT_PAD_LOGICAL, START_ROOT_PAD_LOGICAL};
use crate::coordinator::{
    classify_fullscreen, classify_fullscreen_for_monitor, compute_appbar_rect, BarMode,
    DetailedForegroundWindow, ForegroundWindowInfo, MonitorFullscreenState, NotificationState,
    PixelRect,
};

pub const SINGLE_INSTANCE_MUTEX_NAME: &str = r"Local\TestBar_SingleInstance_v1";
pub const TASKBAR_CREATED_REHIDE_DELAYS_MS: [u64; 3] = [100, 300, 650];
pub const WM_APPBAR_CALLBACK: u32 = WM_APP + 10;
pub const WM_APPBAR_UPDATE_MODE: u32 = WM_APP + 12;
pub const HOOK_EVENT_CHANNEL_CAPACITY: usize = 64;
pub const VK_DUMMY_MASK: u16 = 0xE8;
pub const TESTBAR_INJECTED_EXTRA_INFO: usize = 0x5442_4152; // "TBAR" tag for our own SendInput chord forwards
pub const LLKHF_EXTENDED_FLAG: u32 = 0x0000_0001;
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
static FG_IS_ELEVATED: AtomicBool = AtomicBool::new(false);
static LAST_ELEVATED_CHECK_HWND: AtomicIsize = AtomicIsize::new(0);

static OVERLAY_ORIG_WNDPROCS: OnceLock<Mutex<HashMap<isize, isize>>> = OnceLock::new();
static FLYOUT_SUBCLASSED_HWND: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_NON_ACTIVATING: AtomicBool = AtomicBool::new(false);
static WINDOW_GLIDE_GEN: AtomicU64 = AtomicU64::new(0);
static CLEAN_EXIT_EVENT: Mutex<Option<CleanExitEventGuard>> = Mutex::new(None);

fn overlay_orig_wndprocs() -> &'static Mutex<HashMap<isize, isize>> {
    OVERLAY_ORIG_WNDPROCS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn set_overlay_open_hint(open: bool) {
    HOOK_OVERLAY_OPEN.store(open, Ordering::Release);
}

pub fn is_foreground_elevated() -> bool {
    FG_IS_ELEVATED.load(Ordering::Acquire)
}

/// Checks whether the current foreground window belongs to a **verified** elevated (Admin)
/// process (`TokenElevation.TokenIsElevated != 0`).
///
/// IMPORTANT (BUG #4/#5): Never treat `OpenProcess` / `OpenProcessToken` `ERROR_ACCESS_DENIED`
/// as elevated! Kernel anti-cheat drivers (Riot Vanguard, EAC, BattlEye) strip handle access on
/// **Medium-IL** games via `ObRegisterCallbacks`; falsely returning `true` would disable TestBar's
/// Win-key hook over fullscreen games. On genuinely High-IL Admin windows, UIPI already prevents
/// Medium-IL `WH_KEYBOARD_LL` interception and `dispatch_ll_keyboard_event_ex` falls back cleanly
/// if `SendInput` is ever rejected by UIPI.
fn query_hwnd_elevated_live(fg: HWND) -> bool {
    unsafe {
        if fg.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == 0 || pid == GetCurrentProcessId() {
            return false;
        }
        let proc_handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) if !h.is_invalid() => h,
            _ => return false,
        };

        let mut token = HANDLE::default();
        let elevated = match OpenProcessToken(proc_handle, TOKEN_QUERY, &mut token) {
            Ok(()) if !token.is_invalid() => {
                let mut elev = TOKEN_ELEVATION::default();
                let mut ret_len = 0u32;
                let ok = GetTokenInformation(
                    token,
                    TokenElevation,
                    Some(&mut elev as *mut TOKEN_ELEVATION as *mut c_void),
                    std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                    &mut ret_len,
                )
                .is_ok();
                let _ = CloseHandle(token);
                ok && elev.TokenIsElevated != 0
            }
            _ => false,
        };
        let _ = CloseHandle(proc_handle);
        elevated
    }
}

/// Refreshes the cached `FG_IS_ELEVATED` flag off-hook (safe to call from the fast poller)
/// so `WH_KEYBOARD_LL` never traps elevated Admin windows under UIPI (AD-3).
/// Caches by foreground `HWND` (`LAST_ELEVATED_CHECK_HWND`) so unchanged foreground ticks
/// execute ZERO `OpenProcess`/`OpenProcessToken` syscalls.
pub fn refresh_foreground_elevated_state() -> bool {
    let fg = unsafe { GetForegroundWindow() };
    let fg_raw = fg.0 as isize;
    if fg_raw == 0 {
        FG_IS_ELEVATED.store(false, Ordering::Release);
        LAST_ELEVATED_CHECK_HWND.store(0, Ordering::Release);
        return false;
    }
    if fg_raw == LAST_ELEVATED_CHECK_HWND.load(Ordering::Acquire) {
        return FG_IS_ELEVATED.load(Ordering::Acquire);
    }
    let elevated = query_hwnd_elevated_live(fg);
    FG_IS_ELEVATED.store(elevated, Ordering::Release);
    LAST_ELEVATED_CHECK_HWND.store(fg_raw, Ordering::Release);
    elevated
}

/// Locks the Windows workstation via the native `LockWorkStation` Win32 API
/// (dispatched off-hook for `Win+L` since Windows ignores injected `SendInput(VK_LWIN + 'L')`).
pub fn lock_workstation() {
    unsafe {
        let _ = LockWorkStation();
    }
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
    LockWorkstation,
    NativeStartSuppressed(isize),
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

pub fn clean_exit_event_name(pid: u32) -> String {
    format!(r"Local\TestBar_CleanExit_{pid}")
}

/// RAII Win32 manual-reset named event (`Local\TestBar_CleanExit_<pid>`) coordinating clean
/// shutdown between TestBar and its companion `--watchdog` process so the watchdog exits
/// silently on normal exit instead of racing `SPI_SETWORKAREA` twice.
pub struct CleanExitEventGuard {
    handle: isize,
    pid: u32,
}

impl CleanExitEventGuard {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn signal(&self) -> bool {
        if self.handle == 0 {
            return false;
        }
        unsafe { SetEvent(HANDLE(self.handle as *mut _)).is_ok() }
    }

    pub fn is_signaled(&self) -> bool {
        if self.handle == 0 {
            return false;
        }
        unsafe { WaitForSingleObject(HANDLE(self.handle as *mut _), 0) == WAIT_OBJECT_0 }
    }
}

impl Drop for CleanExitEventGuard {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                let _ = CloseHandle(HANDLE(self.handle as *mut _));
            }
            self.handle = 0;
        }
    }
}

/// Creates (or opens) the manual-reset named event `Local\TestBar_CleanExit_<pid>`.
pub fn create_clean_exit_event(pid: u32) -> Result<CleanExitEventGuard, String> {
    if pid == 0 {
        return Err("clean-exit event pid must be non-zero".to_string());
    }
    let name = clean_exit_event_name(pid);
    unsafe {
        let wide_name = wide(&name);
        let handle = CreateEventW(None, true, false, PCWSTR(wide_name.as_ptr()))
            .map_err(|e| format!("CreateEventW({name}) failed: {e}"))?;
        if handle.is_invalid() {
            return Err(format!("CreateEventW({name}) returned invalid handle"));
        }
        Ok(CleanExitEventGuard {
            handle: handle.0 as isize,
            pid,
        })
    }
}

/// Signals `Local\TestBar_CleanExit_<pid>` so an attached watchdog exits silently once the parent exits.
pub fn signal_clean_exit_event(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let name = clean_exit_event_name(pid);
    unsafe {
        let wide_name = wide(&name);
        if let Ok(handle) = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(wide_name.as_ptr())) {
            let ok = SetEvent(handle).is_ok();
            let _ = CloseHandle(handle);
            return ok;
        }
    }
    false
}

/// Non-blocking check returning `true` when `Local\TestBar_CleanExit_<pid>` exists and is signaled.
pub fn is_clean_exit_signaled(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    const SYNCHRONIZE_RIGHT: SYNCHRONIZATION_ACCESS_RIGHTS =
        SYNCHRONIZATION_ACCESS_RIGHTS(0x0010_0000);
    let name = clean_exit_event_name(pid);
    unsafe {
        let wide_name = wide(&name);
        if let Ok(handle) = OpenEventW(SYNCHRONIZE_RIGHT, false, PCWSTR(wide_name.as_ptr())) {
            let signaled = WaitForSingleObject(handle, 0) == WAIT_OBJECT_0;
            let _ = CloseHandle(handle);
            return signaled;
        }
    }
    false
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

fn is_workarea_compatible_with_monitor(wa: PixelRect, current_monitor: PixelRect) -> bool {
    if !is_valid_rect(wa) || !is_valid_rect(current_monitor) || !current_monitor.covers(wa) {
        return false;
    }
    let matched_edges = u8::from(wa.left == current_monitor.left)
        + u8::from(wa.top == current_monitor.top)
        + u8::from(wa.right == current_monitor.right)
        + u8::from(wa.bottom == current_monitor.bottom);
    if matched_edges < 3 {
        return false;
    }
    wa.width() >= (current_monitor.width() / 2) && wa.height() >= (current_monitor.height() / 2)
}

fn resolve_recovery_workarea(
    explicit: Option<PixelRect>,
    in_memory: Option<WorkAreaSnapshot>,
    disk_path: &Path,
    current_monitor: PixelRect,
    current_dpi: u32,
) -> Option<PixelRect> {
    let disk_snap = load_workarea_snapshot_from(disk_path);
    let explicit_valid = explicit.filter(|&r| {
        if !is_workarea_compatible_with_monitor(r, current_monitor) {
            return false;
        }
        // If explicit --workarea matches an existing snapshot whose monitor or DPI no longer matches,
        // reject it instead of restoring stale DPI-mismatched work-area geometry.
        if let Some(mem) = in_memory {
            if mem.work_area == r && !is_snapshot_compatible(&mem, current_monitor, current_dpi) {
                return false;
            }
        }
        if let Some(disk) = disk_snap {
            if disk.work_area == r && !is_snapshot_compatible(&disk, current_monitor, current_dpi) {
                return false;
            }
        }
        true
    });

    explicit_valid
        .or_else(|| {
            in_memory
                .filter(|s| {
                    is_snapshot_compatible(s, current_monitor, current_dpi)
                        && is_workarea_compatible_with_monitor(s.work_area, current_monitor)
                })
                .map(|s| s.work_area)
        })
        .or_else(|| {
            disk_snap
                .filter(|s| {
                    is_snapshot_compatible(s, current_monitor, current_dpi)
                        && is_workarea_compatible_with_monitor(s.work_area, current_monitor)
                })
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
/// and never receive unwanted Windows 11 DWM square drop-shadow outlines or borders (BUG #2).
pub fn style_overlay_window(hwnd_raw: isize, no_activate: bool) {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);

        let strip_style = (WS_CAPTION.0
            | WS_THICKFRAME.0
            | WS_BORDER.0
            | WS_DLGFRAME.0
            | WS_SYSMENU.0
            | WS_MINIMIZEBOX.0
            | WS_MAXIMIZEBOX.0) as isize;
        let add_style = (WS_POPUP.0 | WS_CLIPSIBLINGS.0 | WS_CLIPCHILDREN.0) as isize;
        let mut style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        style = (style & !strip_style) | add_style;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);

        let strip_ex = (WS_EX_APPWINDOW.0
            | WS_EX_WINDOWEDGE.0
            | WS_EX_DLGMODALFRAME.0
            | WS_EX_CLIENTEDGE.0) as isize;
        let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        ex &= !strip_ex;
        ex |= (WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0) as isize;
        if no_activate {
            ex |= WS_EX_NOACTIVATE.0 as isize;
        } else {
            ex &= !(WS_EX_NOACTIVATE.0 as isize);
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);

        let ncrp = DWMNCRP_DISABLED;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &ncrp as *const _ as *const _,
            std::mem::size_of_val(&ncrp) as u32,
        );

        let border_none: u32 = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border_none as *const _ as *const _,
            std::mem::size_of_val(&border_none) as u32,
        );

        let pref = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const _,
            std::mem::size_of_val(&pref) as u32,
        );

        // Install borderless non-client subclass BEFORE SWP_FRAMECHANGED so neither the
        // initial frame change nor any later WM_NCACTIVATE / WM_NCPAINT can ever paint a
        // classic Windows caption bar onto a DWMNCRP_DISABLED transparent overlay.
        install_overlay_borderless_subclass(hwnd_raw);

        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_NOCOPYBITS,
        );
    }
}

/// Zero-stall foreground activation helper:
/// 1. Fast path: if `hwnd` is already the visible foreground window, return immediately.
/// 2. Direct path: call `AllowSetForegroundWindow(ASFW_ANY)` + `SetForegroundWindow(hwnd)` FIRST
///    with ZERO `AttachThreadInput` (avoids synchronously stalling our GUI thread against heavy
///    Chromium/Zen/Electron message queues).
/// 3. Fallback: only attach to `fg_tid` (and `target_tid` if distinct) when direct
///    `SetForegroundWindow` actually failed, and detach immediately afterward.
pub fn force_focus_window(hwnd_raw: isize) {
    if hwnd_raw == 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindow(hwnd).as_bool() {
            return;
        }
        let already_visible = IsWindowVisible(hwnd).as_bool();
        if already_visible && GetForegroundWindow() == hwnd {
            return;
        }

        // Fast direct attempt without AttachThreadInput.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        if !already_visible {
            let _ = ShowWindowAsync(hwnd, SW_SHOW);
        }
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() == hwnd {
            return;
        }

        // Fallback: Windows foreground lock rejected direct SetForegroundWindow.
        let fg = GetForegroundWindow();
        let cur_tid = GetCurrentThreadId();
        let fg_tid = if !fg.0.is_null()
            && IsWindow(fg).as_bool()
            && !IsHungAppWindow(fg).as_bool()
        {
            GetWindowThreadProcessId(fg, None)
        } else {
            0
        };
        let target_tid = if !IsHungAppWindow(hwnd).as_bool() {
            GetWindowThreadProcessId(hwnd, None)
        } else {
            0
        };

        let attach_fg = fg_tid != 0 && fg_tid != cur_tid;
        let attach_target = target_tid != 0 && target_tid != cur_tid && target_tid != fg_tid;
        let fg_attached = attach_fg && AttachThreadInput(cur_tid, fg_tid, true).as_bool();
        let target_attached =
            attach_target && AttachThreadInput(cur_tid, target_tid, true).as_bool();

        let _ = SetForegroundWindow(hwnd);

        if target_attached {
            let _ = AttachThreadInput(cur_tid, target_tid, false);
        }
        if fg_attached {
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        }
    }
}

/// Atomically applies physical pixel position, size, and optional topmost Z-order in a single
/// `SetWindowPos` call without activating or showing a hidden window.
pub fn set_window_bounds_atomic(
    hwnd_raw: isize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    topmost: bool,
) {
    if hwnd_raw == 0 || w <= 0 || h <= 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindow(hwnd).as_bool() {
            return;
        }
        let (z_insert, flags) = if topmost {
            (HWND_TOPMOST, SWP_NOACTIVATE | SWP_NOCOPYBITS)
        } else {
            (HWND::default(), SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOCOPYBITS)
        };
        let _ = SetWindowPos(hwnd, z_insert, x, y, w, h, flags);
    }
}

/// Smooth cubic ease-out horizontal interpolation helper for `glide_window_x_dwm`.
pub fn interpolate_glide_x(from_x: i32, to_x: i32, progress: f64) -> i32 {
    let t = if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let eased = 1.0 - (1.0 - t).powi(3);
    from_x + (((to_x - from_x) as f64) * eased).round() as i32
}

/// Cancels any currently running `glide_window_x_dwm` animation immediately.
pub fn cancel_window_x_glide() {
    WINDOW_GLIDE_GEN.fetch_add(1, Ordering::AcqRel);
}

/// Glides a visible window horizontally using `SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE`
/// synced to `DwmFlush()` so WebView2 never reallocates its swapchain during preview scrubbing.
pub fn glide_window_x_dwm(hwnd_raw: isize, from_x: i32, to_x: i32, y: i32, duration_ms: u64) {
    let my_gen = WINDOW_GLIDE_GEN.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    if hwnd_raw == 0 {
        return;
    }
    if duration_ms == 0 || from_x == to_x {
        unsafe {
            let hwnd = HWND(hwnd_raw as *mut _);
            if IsWindow(hwnd).as_bool() {
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    to_x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        return;
    }

    let duration_f = duration_ms.clamp(1, 1000) as f64;
    std::thread::spawn(move || unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let started = std::time::Instant::now();
        let mut last_x = from_x;
        loop {
            if WINDOW_GLIDE_GEN.load(Ordering::Acquire) != my_gen {
                break;
            }
            if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
                break;
            }
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let progress = (elapsed_ms / duration_f).min(1.0);
            let next_x = interpolate_glide_x(from_x, to_x, progress);
            if next_x != last_x || progress >= 1.0 {
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    next_x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                last_x = next_x;
            }
            if progress >= 1.0 {
                break;
            }
            if DwmFlush().is_err() {
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
        }
    });
}

/// Toggles click-through hit-transparency (`WS_EX_TRANSPARENT`) on an overlay window
/// ONLY when the bit actually changes and explicitly WITHOUT calling `SetWindowPos(..., SWP_FRAMECHANGED)`
/// so transparent Win11 WebView2 windows never flicker borders (AD-1 compliant).
pub fn set_window_hit_transparent(hwnd_raw: isize, transparent: bool) {
    if hwnd_raw == 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindow(hwnd).as_bool() {
            return;
        }
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let bit = WS_EX_TRANSPARENT.0 as isize;
        let next = if transparent {
            current | bit
        } else {
            current & !bit
        };
        if next != current {
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
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
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_NOCOPYBITS);
    }
}

/// Idempotently subclasses any TestBar overlay HWND (`taskbar`, `taskbar-secondary`, `start`,
/// `flyout`) so Win32 classic non-client painting (`WM_NCPAINT`, `WM_NCACTIVATE` with `lParam=0`)
/// can NEVER draw a `#BFCDDB`/`#99B4D1` 23px title bar onto a `DWMNCRP_DISABLED` transparent
/// WebView2 overlay, while preserving `tao`'s `WM_NCACTIVATE` focus tracking (`lParam = -1`).
pub fn install_overlay_borderless_subclass(hwnd_raw: isize) {
    if hwnd_raw == 0 {
        return;
    }
    let wnd_proc_addr = overlay_wnd_proc as *const () as usize as isize;
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let current_proc = GetWindowLongPtrW(hwnd, GWLP_WNDPROC);
        if current_proc == 0 || current_proc == wnd_proc_addr {
            return;
        }
        {
            let mut map = overlay_orig_wndprocs()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if map.contains_key(&hwnd_raw) {
                return;
            }
            map.insert(hwnd_raw, current_proc);
        }
        let prev = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wnd_proc_addr);
        let mut map = overlay_orig_wndprocs()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if prev != 0 {
            map.insert(hwnd_raw, prev);
        } else {
            map.remove(&hwnd_raw);
        }
    }
}

/// Subclasses the pre-warmed `flyout` window once so non-activating flyouts
/// (`preview`, `osd`) return `MA_NOACTIVATE` on `WM_MOUSEACTIVATE` without
/// flipping `GWL_EXSTYLE` + `SWP_FRAMECHANGED` per open, while also enforcing
/// borderless non-client paint suppression (`WM_NCPAINT` / `WM_NCACTIVATE(..., -1)`).
pub fn install_flyout_noactivate_subclass(hwnd_raw: isize) {
    if hwnd_raw == 0 {
        return;
    }
    FLYOUT_SUBCLASSED_HWND.store(hwnd_raw, Ordering::SeqCst);
    install_overlay_borderless_subclass(hwnd_raw);
}

pub fn set_flyout_non_activating(non_activating: bool) {
    FLYOUT_NON_ACTIVATING.store(non_activating, Ordering::SeqCst);
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let hwnd_raw = hwnd.0 as isize;
    if msg == WM_MOUSEACTIVATE
        && hwnd_raw == FLYOUT_SUBCLASSED_HWND.load(Ordering::SeqCst)
        && FLYOUT_NON_ACTIVATING.load(Ordering::SeqCst)
    {
        return LRESULT(MA_NOACTIVATE as isize);
    }

    // Suppress all GDI non-client painting (prevents classic 23px title bar on DWMNCRP_DISABLED HWNDs).
    if msg == WM_NCPAINT {
        return LRESULT(0);
    }

    let orig = overlay_orig_wndprocs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&hwnd_raw)
        .copied()
        .unwrap_or(0);

    // Per MSDN, passing lParam = -1 on WM_NCACTIVATE prevents DefWindowProcW from repainting
    // the non-client area while still letting tao's WndProc update WindowEvent::Focused.
    let effective_lparam = if msg == WM_NCACTIVATE {
        LPARAM(-1)
    } else {
        lparam
    };

    let res = if orig != 0 {
        let prev_proc: WNDPROC = Some(std::mem::transmute(orig));
        CallWindowProcW(prev_proc, hwnd, msg, wparam, effective_lparam)
    } else {
        DefWindowProcW(hwnd, msg, wparam, effective_lparam)
    };

    if msg == WM_NCDESTROY {
        overlay_orig_wndprocs()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&hwnd_raw);
        let _ = FLYOUT_SUBCLASSED_HWND.compare_exchange(
            hwnd_raw,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    res
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
                SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOCOPYBITS
                    | if no_activate {
                        SWP_NOACTIVATE
                    } else {
                        Default::default()
                    },
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

/// Re-hides Windows 11 native taskbar(s) across staggered delays after `TaskbarCreated`
/// because Explorer often finishes initializing `Shell_TrayWnd` / `Shell_SecondaryTrayWnd`
/// hundreds of milliseconds after broadcasting `TaskbarCreated`.
pub fn schedule_staggered_native_rehide() {
    if RESTORE_IN_PROGRESS.load(Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let mut elapsed = 0u64;
        for &target_ms in &TASKBAR_CREATED_REHIDE_DELAYS_MS {
            let wait_ms = target_ms.saturating_sub(elapsed);
            if wait_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            }
            elapsed = target_ms;
            if RESTORE_IN_PROGRESS.load(Ordering::SeqCst) {
                return;
            }
            ensure_native_taskbar_hidden_if_needed();
        }
    });
}

trait AppBarShellOps: Send {
    fn primary_monitor_rect(&self) -> PixelRect;
    fn primary_monitor_dpi(&self) -> u32;
    fn text_scale_factor(&self) -> f64;
    fn hide_native_taskbar(&self);
    fn schedule_staggered_rehide(&self) {}
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

    fn schedule_staggered_rehide(&self) {
        schedule_staggered_native_rehide();
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
    last_text_scale_pct: Cell<u32>,
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

/// Anchors a bottom AppBar reservation rect strictly to `primary_monitor.bottom` so hidden
/// `Shell_TrayWnd` reservations returned by `ABM_QUERYPOS` never stack vertically on top of TestBar.
pub fn resolve_bottom_appbar_rect(primary_monitor: PixelRect, reserve_h: i32) -> PixelRect {
    let clamped_h = reserve_h.clamp(0, primary_monitor.height());
    PixelRect::new(
        primary_monitor.left,
        primary_monitor.bottom - clamped_h,
        primary_monitor.right,
        primary_monitor.bottom,
    )
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
    // Taskbars neutralize Windows TextScaleFactor: reserve 67px Dock / 58px Edge scaled strictly by dpi / 96.0.
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
    // Anchor strictly to primary_monitor.bottom (never subtract reserve_h from hidden Shell_TrayWnd's shifted rc.bottom).
    let resolved = resolve_bottom_appbar_rect(mon_px, reserve_h);
    abd.rc = RECT {
        left: resolved.left,
        top: resolved.top,
        right: resolved.right,
        bottom: resolved.bottom,
    };
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
            ctx.shell_ops.schedule_staggered_rehide();
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
            WM_SETTINGCHANGE => {
                let current_pct = (ctx.shell_ops.text_scale_factor() * 100.0).round() as u32;
                if current_pct != ctx.last_text_scale_pct.get() {
                    ctx.last_text_scale_pct.set(current_pct);
                    // AppBar height is DPI-only (neutralizes TextScaleFactor), so avoid redundant ABM_SETPOS
                    // while still notifying Start/views via PositionChanged.
                    apply_appbar_on_hwnd(hwnd, ctx, false, true, None);
                    (ctx.on_event)(ShellAppBarEvent::PositionChanged);
                }
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
            let initial_text_scale_pct = (shell_ops.text_scale_factor() * 100.0).round() as u32;
            let ctx = Box::new(AppBarWindowContext {
                shell_ops,
                appbar_registered: Cell::new(false),
                bar_mode: Cell::new(initial_mode),
                taskbar_created_msg,
                last_appbar_rect: Cell::new(None),
                last_text_scale_pct: Cell::new(initial_text_scale_pct),
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
    ensure_native_start_menu_healthy();
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
    ensure_native_start_menu_healthy();
    NATIVE_HIDDEN.store(false, Ordering::SeqCst);
}

/// Guaranteed restoration of Windows 11 native taskbar(s) + AppBar removal + exact HiDPI workarea refresh.
/// Signals the process clean-exit event so any companion `--watchdog` exits silently without double-restoring.
pub fn restore_native_taskbar() {
    let pid = unsafe { GetCurrentProcessId() };
    let signaled = {
        let guard = CLEAN_EXIT_EVENT.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().map(CleanExitEventGuard::signal).unwrap_or(false)
    };
    if !signaled {
        let _ = signal_clean_exit_event(pid);
    }
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
/// Creates the named clean-exit event `Local\TestBar_CleanExit_<pid>` first so clean shutdowns
/// let the watchdog exit silently, while Task Manager kills or crashes trigger full restoration.
pub fn spawn_watchdog(work_area: Option<PixelRect>) {
    if let Ok(exe) = std::env::current_exe() {
        let pid = unsafe { GetCurrentProcessId() };
        if let Ok(ev) = create_clean_exit_event(pid) {
            let mut slot = CLEAN_EXIT_EVENT.lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some(ev);
        }
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
    // Hold our own handle to the named clean-exit event BEFORE waiting on the parent so the
    // kernel object outlives parent termination if the parent signaled it during clean shutdown.
    let clean_exit_guard = create_clean_exit_event(args.parent_pid).ok();
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, args.parent_pid) {
            let _ = WaitForSingleObject(handle, INFINITE);
            let _ = CloseHandle(handle);
        }
    }
    if clean_exit_guard
        .as_ref()
        .map(CleanExitEventGuard::is_signaled)
        .unwrap_or_else(|| is_clean_exit_signaled(args.parent_pid))
    {
        return;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerMonitorFullscreenSnapshot {
    pub notification_state: NotificationState,
    pub foreground: Option<DetailedForegroundWindow>,
    pub primary_monitor: PixelRect,
    pub secondary_monitor: Option<PixelRect>,
}

impl PerMonitorFullscreenSnapshot {
    /// Returns `true` when the current foreground window belongs to TestBar itself
    /// (e.g. Start or an interactive Flyout is open or mid-exit animation).
    pub fn fg_is_own_process(&self) -> bool {
        self.foreground.as_ref().is_some_and(|fg| fg.is_own_process)
    }
}

fn query_user_notification_state() -> NotificationState {
    unsafe {
        match SHQueryUserNotificationState() {
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
        }
    }
}

unsafe fn query_window_and_extended_rect(hwnd: HWND) -> Option<(PixelRect, PixelRect)> {
    let mut win_rect = RECT::default();
    GetWindowRect(hwnd, &mut win_rect).ok()?;
    let win_px = PixelRect::new(win_rect.left, win_rect.top, win_rect.right, win_rect.bottom);
    let mut ext_rect = RECT::default();
    let ext_px = if DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut ext_rect as *mut RECT as *mut c_void,
        std::mem::size_of::<RECT>() as u32,
    )
    .is_ok()
    {
        let candidate =
            PixelRect::new(ext_rect.left, ext_rect.top, ext_rect.right, ext_rect.bottom);
        if is_valid_rect(candidate) {
            candidate
        } else {
            win_px
        }
    } else {
        win_px
    };
    Some((win_px, ext_px))
}

/// Captures the Win32 inputs required by `coordinator::classify_fullscreen`, using
/// `DWMWA_EXTENDED_FRAME_BOUNDS` (falling back to `GetWindowRect`) so invisible DWM
/// shadow frames never cause false positives on maximized apps.
pub fn query_fullscreen_snapshot() -> FullscreenSnapshot {
    unsafe {
        let notification_state = query_user_notification_state();
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
            query_window_and_extended_rect(fg).map(|(_win_px, ext_px)| ForegroundWindowInfo {
                visible,
                is_own_process,
                class_name,
                style,
                rect: ext_px,
            })
        };

        FullscreenSnapshot {
            notification_state,
            foreground,
            primary_monitor,
        }
    }
}

/// Captures per-monitor fullscreen state inputs using `MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST)`,
/// `DWMWA_EXTENDED_FRAME_BOUNDS`, and `IsZoomed` so fullscreen suppression is attributed strictly
/// to the display hosting the fullscreen foreground window.
pub fn query_per_monitor_fullscreen_snapshot() -> PerMonitorFullscreenSnapshot {
    unsafe {
        let notification_state = query_user_notification_state();
        let primary_monitor = primary_monitor_rect().to_pixel_rect();
        let secondary_monitor = secondary_monitor_rects()
            .into_iter()
            .next()
            .map(MonitorRect::to_pixel_rect);

        let fg = GetForegroundWindow();
        let foreground = if fg.0.is_null() {
            None
        } else {
            let visible = IsWindowVisible(fg).as_bool() && !IsIconic(fg).as_bool();
            let is_maximized = IsZoomed(fg).as_bool();
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
            let hmon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let (monitor_rect, work_area_rect) = if GetMonitorInfoW(hmon, &mut info).as_bool() {
                (
                    PixelRect::new(
                        info.rcMonitor.left,
                        info.rcMonitor.top,
                        info.rcMonitor.right,
                        info.rcMonitor.bottom,
                    ),
                    PixelRect::new(
                        info.rcWork.left,
                        info.rcWork.top,
                        info.rcWork.right,
                        info.rcWork.bottom,
                    ),
                )
            } else {
                (primary_monitor, primary_monitor)
            };

            query_window_and_extended_rect(fg).map(|(window_rect, extended_rect)| {
                DetailedForegroundWindow {
                    visible,
                    is_own_process,
                    is_maximized,
                    class_name,
                    style,
                    window_rect,
                    extended_rect,
                    monitor_rect,
                    work_area_rect,
                }
            })
        };

        PerMonitorFullscreenSnapshot {
            notification_state,
            foreground,
            primary_monitor,
            secondary_monitor,
        }
    }
}

/// Detects fullscreen games / video players independently for primary and secondary monitors
/// AND reports `fg_is_own_process` so `WindowCoordinator` can freeze fullscreen state whenever
/// TestBar's own Start/Flyout window holds foreground focus.
pub fn detect_fullscreen_per_monitor_with_fg() -> (MonitorFullscreenState, bool) {
    let snap = query_per_monitor_fullscreen_snapshot();
    let fg_is_own = snap.fg_is_own_process();
    let primary = classify_fullscreen_for_monitor(
        snap.notification_state,
        snap.foreground.as_ref(),
        snap.primary_monitor,
        true,
    );
    let secondary = snap
        .secondary_monitor
        .map(|sec| {
            classify_fullscreen_for_monitor(
                snap.notification_state,
                snap.foreground.as_ref(),
                sec,
                false,
            )
        })
        .unwrap_or(false);
    (MonitorFullscreenState { primary, secondary }, fg_is_own)
}

/// Detects fullscreen games / video players independently for primary and secondary monitors
/// via `MonitorFromWindow` + `DWMWA_EXTENDED_FRAME_BOUNDS` + `coordinator::classify_fullscreen_for_monitor`.
pub fn detect_fullscreen_per_monitor() -> MonitorFullscreenState {
    detect_fullscreen_per_monitor_with_fg().0
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
    LockWorkstation,
}

/// Pure decision returned by the Hold-and-Forward `WH_KEYBOARD_LL` state machine (BUG #4, AD-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinHookDecision {
    /// When `true`, `ll_keyboard_proc` returns `LRESULT(1)` to swallow the physical event.
    pub swallow: bool,
    /// When `Some(win_vk)`, atomically inject `[win_vk DOWN, current_key DOWN]` via `SendInput`
    /// before returning `LRESULT(1)` to swallow the physical `current_key DOWN`.
    pub forward_win_chord_vk: Option<u16>,
    /// Optional off-hook action to enqueue via bounded `try_send`.
    pub action: WinKeyAction,
}

impl WinHookDecision {
    pub const PASS: Self = Self {
        swallow: false,
        forward_win_chord_vk: None,
        action: WinKeyAction::None,
    };

    pub const SWALLOW: Self = Self {
        swallow: true,
        forward_win_chord_vk: None,
        action: WinKeyAction::None,
    };
}

/// Returns `true` for bare Shift/Ctrl/Alt modifier virtual-key codes.
/// When pressed while Win is held & unforwarded, these pass through to `win32k` without
/// forwarding `Win DOWN` yet so aborted `Win+Shift`/`Win+Ctrl`/`Win+Alt` holds never leak
/// native Windows Start, while full chords like `Win+Shift+S` or `Win+Ctrl+Left` fire cleanly
/// once the non-modifier key arrives.
pub fn is_bare_modifier_vk(vk: u16) -> bool {
    matches!(
        vk,
        0x10 // VK_SHIFT
            | 0x11 // VK_CONTROL
            | 0x12 // VK_MENU
            | 0xA0 // VK_LSHIFT
            | 0xA1 // VK_RSHIFT
            | 0xA2 // VK_LCONTROL
            | 0xA3 // VK_RCONTROL
            | 0xA4 // VK_LMENU
            | 0xA5 // VK_RMENU
    )
}

/// Returns `true` ONLY for Windows 11's native Start Menu host (`StartMenuExperienceHost.exe`).
/// Strictly returns `false` for `ShellExperienceHost.exe` (`Win+N` Notifications),
/// `ScreenClippingHost.exe` (`Win+Shift+S`), `SearchHost.exe`, and all other executables.
pub fn is_native_start_menu_exe(exe_path_or_name: &str) -> bool {
    let trimmed = exe_path_or_name.trim();
    if trimmed.is_empty() {
        return false;
    }
    let base = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    base.eq_ignore_ascii_case("startmenuexperiencehost.exe")
}

/// Pure, Win32-independent Hold-and-Forward state machine for `WH_KEYBOARD_LL` Win-key handling (AD-3).
///
/// - Swallows fresh lone physical `VK_LWIN` / `VK_RWIN` `KEYDOWN` and `KEYUP` completely so
///   `win32k` / `StartMenuExperienceHost` NEVER see Win events on 1x/2x/3x/50x rapid taps.
/// - On the first physical non-Win `KEYDOWN` while Win is held & unforwarded:
///   - `Win+L` (no other modifiers) swallows `'L'` and emits `WinKeyAction::LockWorkstation`
///     (native `LockWorkStation()`, since Windows ignores injected `Win+L`).
///   - Any other chord (`Win+D/E/R/V/Tab/Shift+S/Arrows/Space/1..9/...`) atomically forwards
///     `[win_vk DOWN, key DOWN]` and lets all subsequent chord keys & physical `Win UP` pass natively.
/// - Modifier-first chords (e.g. `Shift+Win+S`) and elevated foreground windows pass straight through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinKeyStateMachine {
    lwin_down: bool,
    rwin_down: bool,
    win_forwarded: bool,
    combo_used: bool,
}

impl WinKeyStateMachine {
    pub const fn new() -> Self {
        Self {
            lwin_down: false,
            rwin_down: false,
            win_forwarded: false,
            combo_used: false,
        }
    }

    pub fn is_any_win_down(&self) -> bool {
        self.lwin_down || self.rwin_down
    }

    pub fn is_win_key_down(&self, vk: u16) -> bool {
        if vk == VK_LWIN.0 {
            self.lwin_down
        } else if vk == VK_RWIN.0 {
            self.rwin_down
        } else {
            false
        }
    }

    pub fn is_win_forwarded(&self) -> bool {
        self.win_forwarded
    }

    pub fn is_combo_used(&self) -> bool {
        self.combo_used
    }

    /// Reconciles tracked Win key flags against OS async key state ONLY when Win was actually
    /// forwarded to `win32k` (e.g. after UAC or `Ctrl+Alt+Del` swallowed `Win Up` on Secure Desktop).
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
            self.win_forwarded = false;
        }
    }

    pub(crate) fn revert_win_forwarded(&mut self) {
        self.win_forwarded = false;
    }

    pub(crate) fn mark_combo_used(&mut self) {
        if self.is_any_win_down() {
            self.combo_used = true;
        }
    }

    /// Evaluates one low-level keyboard event and returns the exact Hold-and-Forward decision.
    pub fn on_ll_key_event(
        &mut self,
        vk: u16,
        transition: KeyTransition,
        flags: u32,
        dw_extra_info: usize,
        phys: PhysicalKeySnapshot,
        fg_elevated: bool,
    ) -> WinHookDecision {
        let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;
        let is_injected = (flags & (LLKHF_INJECTED_FLAG | LLKHF_LOWER_IL_INJECTED_FLAG)) != 0;

        // Always pass our own tagged SendInput events (chord forwards, Win+N/Win+B, Escape
        // dismissal), dummy mask keys, and injected non-Win keys (IME/macros/typing).
        // External untagged injected VK_LWIN/VK_RWIN (e.g. Start11/PowerToys/keyboard drivers)
        // MUST still flow through Hold-and-Forward so they can never leak native Win11 Start!
        if vk == VK_DUMMY_MASK
            || dw_extra_info == TESTBAR_INJECTED_EXTRA_INFO
            || (is_injected && !is_win)
        {
            return WinHookDecision::PASS;
        }

        // Never trap elevated Admin windows when idle (UIPI would block SendInput chord forwarding).
        if fg_elevated && !self.is_any_win_down() {
            return WinHookDecision::PASS;
        }

        let alt_down = (flags & LLKHF_ALTDOWN_FLAG) != 0;

        if is_win {
            match transition {
                KeyTransition::Down => {
                    // Only reconcile against GetAsyncKeyState when win32k actually saw a forwarded Win DOWN;
                    // for swallowed unforwarded Win holds, GetAsyncKeyState(VK_LWIN) is intentionally 0!
                    if self.is_any_win_down() && self.win_forwarded {
                        self.reconcile_physical_win_keys(phys.lwin_down, phys.rwin_down);
                    }

                    let was_this_down = self.is_win_key_down(vk);
                    let other_down = if vk == VK_LWIN.0 {
                        self.rwin_down
                    } else {
                        self.lwin_down
                    };
                    if vk == VK_LWIN.0 {
                        self.lwin_down = true;
                    } else {
                        self.rwin_down = true;
                    }

                    if was_this_down {
                        // OS key auto-repeat while held: preserve combo/forwarded state.
                        if alt_down {
                            self.combo_used = true;
                        }
                        return WinHookDecision {
                            swallow: !self.win_forwarded,
                            forward_win_chord_vk: None,
                            action: WinKeyAction::None,
                        };
                    }

                    if other_down {
                        // Dual-Win chord (LWin + RWin): never trigger Start.
                        self.combo_used = true;
                        return WinHookDecision {
                            swallow: !self.win_forwarded,
                            forward_win_chord_vk: None,
                            action: WinKeyAction::None,
                        };
                    }

                    if alt_down || phys.modifier_down {
                        // Modifier-first hold (e.g. Shift+Win+S, Ctrl+Win+Left, or Alt+Shift rollover):
                        // NEVER forward bare Win DOWN to win32k yet! Swallow with combo_used=true,
                        // win_forwarded=false so releasing Shift+Win without a 3rd key NEVER opens
                        // native Start (nor TestBar Start), while pressing a 3rd non-modifier key
                        // atomically forwards [Win DOWN, key DOWN] with Shift/Ctrl already down in win32k!
                        self.combo_used = true;
                        self.win_forwarded = false;
                        return WinHookDecision::SWALLOW;
                    }

                    // Fresh lone Win press from idle: SWALLOW completely so win32k never arms Start!
                    self.combo_used = false;
                    self.win_forwarded = false;
                    WinHookDecision::SWALLOW
                }
                KeyTransition::Up => {
                    let was_this_down = self.is_win_key_down(vk);
                    let other_down = if vk == VK_LWIN.0 {
                        self.rwin_down
                    } else {
                        self.lwin_down
                    };
                    if vk == VK_LWIN.0 {
                        self.lwin_down = false;
                    } else {
                        self.rwin_down = false;
                    }

                    if !was_this_down {
                        if !other_down {
                            self.combo_used = false;
                            self.win_forwarded = false;
                        }
                        return WinHookDecision::PASS;
                    }

                    if other_down {
                        // Other Win key still held (dual-Win chord).
                        self.combo_used = true;
                        return WinHookDecision {
                            swallow: !self.win_forwarded,
                            forward_win_chord_vk: None,
                            action: WinKeyAction::None,
                        };
                    }

                    let was_forwarded = self.win_forwarded;
                    let was_combo = self.combo_used || alt_down;
                    self.win_forwarded = false;
                    self.combo_used = false;

                    if was_forwarded {
                        // win32k saw Win DOWN for a chord -> pass physical Win UP to release cleanly.
                        WinHookDecision::PASS
                    } else {
                        // win32k NEVER saw Win DOWN -> swallow Win UP and toggle TestBar Start on clean tap.
                        WinHookDecision {
                            swallow: true,
                            forward_win_chord_vk: None,
                            action: if was_combo {
                                WinKeyAction::None
                            } else {
                                WinKeyAction::ToggleStart
                            },
                        }
                    }
                }
            }
        } else {
            // Non-Win key: releasing a prior key (typing rollover) never poisons lone Win.
            if transition != KeyTransition::Down {
                return WinHookDecision::PASS;
            }
            if !self.is_any_win_down() {
                return WinHookDecision::PASS;
            }

            if self.win_forwarded {
                // Reconcile forwarded Win state (e.g. Secure Desktop ate Win UP).
                self.reconcile_physical_win_keys(phys.lwin_down, phys.rwin_down);
                if self.is_any_win_down() {
                    self.combo_used = true;
                }
                return WinHookDecision::PASS;
            }

            // Bare modifiers (Shift/Ctrl/Alt) while Win is held & unforwarded:
            // let win32k update the modifier state natively WITHOUT forwarding Win DOWN yet!
            // If user releases Win without pressing a non-modifier key (e.g. grazed Win+Shift or
            // aborted Win+Shift+S), win32k never saw Win DOWN -> zero native Start popup, and
            // combo_used=true prevents falsely toggling TestBar Start. If a non-modifier key
            // follows (e.g. 'S' in Win+Shift+S or Left in Win+Ctrl+Left), Shift/Ctrl is already
            // down in win32k so injecting [Win DOWN, key DOWN] fires the OS chord cleanly!
            if is_bare_modifier_vk(vk) {
                self.combo_used = true;
                return WinHookDecision::PASS;
            }

            // First non-modifier, non-Win KeyDown while Win is held & swallowed!
            let active_win_vk = if self.lwin_down { VK_LWIN.0 } else { VK_RWIN.0 };

            // Pure Win+L (no Ctrl/Shift/Alt): Windows ignores injected Win+L, so invoke LockWorkStation() off-hook.
            if vk == b'L' as u16 && !alt_down && !phys.modifier_down {
                self.lwin_down = false;
                self.rwin_down = false;
                self.win_forwarded = false;
                self.combo_used = false;
                return WinHookDecision {
                    swallow: true,
                    forward_win_chord_vk: None,
                    action: WinKeyAction::LockWorkstation,
                };
            }

            // All other chords (Win+D/E/R/V/Tab/Shift+S/Arrows/Space/1..9/...):
            // atomically inject [win_vk DOWN, key DOWN] and swallow physical key DOWN!
            self.win_forwarded = true;
            self.combo_used = true;
            WinHookDecision {
                swallow: true,
                forward_win_chord_vk: Some(active_win_vk),
                action: WinKeyAction::None,
            }
        }
    }

    /// Simplified pure transition helper (uses tracked Win keys as physical state, `dw_extra_info = 0`, `fg_elevated = false`).
    pub fn on_key_event(&mut self, vk: u16, transition: KeyTransition, flags: u32) -> WinKeyAction {
        let phys = PhysicalKeySnapshot {
            lwin_down: self.lwin_down,
            rwin_down: self.rwin_down,
            modifier_down: false,
        };
        self.on_ll_key_event(vk, transition, flags, 0, phys, false)
            .action
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

const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
const EVENT_OBJECT_CREATE: u32 = 0x8000;
const EVENT_OBJECT_DESTROY: u32 = 0x8001;
const EVENT_OBJECT_SHOW: u32 = 0x8002;
const EVENT_OBJECT_HIDE: u32 = 0x8003;
const EVENT_OBJECT_UNCLOAKED: u32 = 0x8018;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;
const WM_CANCELMODE: u32 = 0x001F;
const OBJID_WINDOW: i32 = 0;
const CHILDID_SELF: i32 = 0;

struct DesktopActivityNotifier {
    seq: AtomicU64,
    mu: Mutex<()>,
    cvar: Condvar,
}

static DESKTOP_ACTIVITY: OnceLock<DesktopActivityNotifier> = OnceLock::new();

fn desktop_activity_notifier() -> &'static DesktopActivityNotifier {
    DESKTOP_ACTIVITY.get_or_init(|| DesktopActivityNotifier {
        seq: AtomicU64::new(0),
        mu: Mutex::new(()),
        cvar: Condvar::new(),
    })
}

/// Pure WinEvent filter deciding whether an OS shell event should wake the event-driven
/// desktop poller. Strictly filters `EVENT_OBJECT_CREATE/DESTROY/SHOW/HIDE` to top-level
/// `OBJID_WINDOW && CHILDID_SELF` events (rejecting caret/cursor/child-control noise).
pub(crate) fn is_desktop_activity_event(
    event: u32,
    id_object: i32,
    id_child: i32,
    is_child_hwnd: bool,
) -> bool {
    match event {
        EVENT_SYSTEM_FOREGROUND | EVENT_SYSTEM_MINIMIZESTART | EVENT_SYSTEM_MINIMIZEEND => true,
        EVENT_OBJECT_CREATE
        | EVENT_OBJECT_DESTROY
        | EVENT_OBJECT_SHOW
        | EVENT_OBJECT_HIDE => {
            id_object == OBJID_WINDOW && id_child == CHILDID_SELF && !is_child_hwnd
        }
        EVENT_OBJECT_UNCLOAKED => id_object == OBJID_WINDOW && id_child == CHILDID_SELF,
        _ => false,
    }
}

/// Non-blocking desktop activity signal safe for `WinEventProc` callbacks:
/// atomically increments the sequence counter and wakes any waiting poller via `Condvar`
/// without ever sleeping or blocking on I/O.
pub(crate) fn notify_desktop_activity() {
    let n = desktop_activity_notifier();
    n.seq.fetch_add(1, Ordering::Release);
    // `mu` is only ever held for ~10ns by the waiter's seq-check before SleepConditionVariableSRW;
    // up to 4 spin attempts guarantee zero missed wakeups while remaining strictly non-blocking.
    for _ in 0..4 {
        if let Ok(_guard) = n.mu.try_lock() {
            n.cvar.notify_all();
            return;
        }
        std::hint::spin_loop();
    }
    n.cvar.notify_all();
}

/// Returns the current monotonic desktop activity sequence number.
pub fn current_desktop_activity_seq() -> u64 {
    desktop_activity_notifier().seq.load(Ordering::Acquire)
}

/// Sleeps the caller until a relevant top-level desktop WinEvent (`FOREGROUND`, `MINIMIZE`,
/// `CREATE/DESTROY/SHOW/HIDE`, `UNCLOAKED`) increments the sequence counter past `*last_seq`,
/// or until `timeout` elapses.
///
/// Returns `true` if woken by an OS desktop event (updating `*last_seq`), or `false` on timeout.
pub fn wait_for_desktop_activity_or_timeout(
    last_seq: &mut u64,
    timeout: std::time::Duration,
) -> bool {
    let n = desktop_activity_notifier();
    let cur = n.seq.load(Ordering::Acquire);
    if cur != *last_seq {
        *last_seq = cur;
        return true;
    }
    if timeout.is_zero() {
        return false;
    }

    let deadline = std::time::Instant::now() + timeout;
    let mut guard = n.mu.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let cur = n.seq.load(Ordering::Acquire);
        if cur != *last_seq {
            *last_seq = cur;
            return true;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        let (next_guard, wait_res) = n
            .cvar
            .wait_timeout(guard, remaining)
            .unwrap_or_else(|e| e.into_inner());
        guard = next_guard;
        let cur = n.seq.load(Ordering::Acquire);
        if cur != *last_seq {
            *last_seq = cur;
            return true;
        }
        if wait_res.timed_out() {
            return false;
        }
    }
}

type WinEventProc = unsafe extern "system" fn(
    h_win_event_hook: isize,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    id_event_thread: u32,
    dwms_event_time: u32,
);

#[link(name = "user32")]
extern "system" {
    fn SetWinEventHook(
        event_min: u32,
        event_max: u32,
        hmod_win_event_proc: isize,
        pfn_win_event_proc: WinEventProc,
        id_process: u32,
        id_thread: u32,
        dw_flags: u32,
    ) -> isize;
    fn UnhookWinEvent(h_win_event_hook: isize) -> i32;
}

fn query_process_image_path_fast(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if ok && len > 0 {
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        } else {
            None
        }
    }
}

fn is_hwnd_native_start_menu(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == GetCurrentProcessId() {
            return false;
        }
        query_process_image_path_fast(pid)
            .map(|p| is_native_start_menu_exe(&p))
            .unwrap_or(false)
    }
}

/// Builds a tagged `[VK_ESCAPE down, VK_ESCAPE up]` `SendInput` batch for dismissing
/// Immersive Windows 11 `StartMenuExperienceHost.exe` if stealing foreground focus alone
/// did not dismiss it.
pub(crate) fn build_tagged_escape_inputs() -> [INPUT; 2] {
    [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_ESCAPE,
                    dwExtraInfo: TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_ESCAPE,
                    dwFlags: KEYEVENTF_KEYUP,
                    dwExtraInfo: TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
    ]
}

/// Pure repair decision for `StartMenuExperienceHost.exe`'s persistent `CoreWindow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMenuVisibilityAction {
    /// `WS_VISIBLE` is already set (`IsWindowVisible == true`) — Win11 DWM cloaking/uncloaking works normally.
    AlreadyHealthy,
    /// `WS_VISIBLE` was cleared while DWM-cloaked (`!visible && cloaked != 0`):
    /// restore `WS_VISIBLE` with `ShowWindowAsync(SW_SHOWNOACTIVATE)` without popping Start open.
    RestoreVisibleOnly,
    /// `WS_VISIBLE` was cleared and DWM is uncloaked (`!visible && cloaked == 0`):
    /// restore `WS_VISIBLE` with `ShowWindowAsync(SW_SHOWNOACTIVATE)` and immediately dismiss
    /// (`WM_CANCELMODE` + `dismiss_native_start_menu`) so Start does not remain open on screen.
    RestoreVisibleAndDismiss,
}

/// Pure testable decision helper determining whether `StartMenuExperienceHost.exe`'s
/// persistent `CoreWindow` needs `WS_VISIBLE` restored (and dismissed if uncloaked).
pub fn should_restore_start_menu_visibility(
    is_visible: bool,
    cloaked_flags: u32,
) -> StartMenuVisibilityAction {
    if is_visible {
        StartMenuVisibilityAction::AlreadyHealthy
    } else if cloaked_flags != 0 {
        StartMenuVisibilityAction::RestoreVisibleOnly
    } else {
        StartMenuVisibilityAction::RestoreVisibleAndDismiss
    }
}

/// Returns `true` strictly for the UWP `CoreWindow` class used by Windows 11 Start Menu,
/// excluding hidden Win32 helper classes (`IME`, `MSCTFIME UI`, etc.) inside the same process.
pub fn is_start_menu_core_window_class(class_name: &str) -> bool {
    class_name == "Windows.UI.Core.CoreWindow"
}

/// Ensures Windows 11's persistent `StartMenuExperienceHost.exe` `CoreWindow` retains `WS_VISIBLE`
/// so native Win+Start DWM uncloaking works cleanly after TestBar exits or recovers from a crash.
/// Lock-free and thread-free (`EnumWindows` + `ShowWindowAsync` + `PostMessageW`), safe for panic hooks.
pub fn ensure_native_start_menu_healthy() {
    unsafe {
        let mut repairs: Vec<(isize, StartMenuVisibilityAction)> = Vec::new();
        extern "system" fn collect_broken_start_windows(hwnd: HWND, lparam: LPARAM) -> BOOL {
            unsafe {
                let mut cls_buf = [0u16; 64];
                let cls_len = GetClassNameW(hwnd, &mut cls_buf);
                if cls_len <= 0 {
                    return BOOL(1);
                }
                let cls = String::from_utf16_lossy(&cls_buf[..cls_len as usize]);
                if !is_start_menu_core_window_class(&cls) || !is_hwnd_native_start_menu(hwnd) {
                    return BOOL(1);
                }
                let is_visible = IsWindowVisible(hwnd).as_bool();
                if is_visible {
                    return BOOL(1);
                }
                // Default to cloaked=1 if DwmGetWindowAttribute fails so we restore WS_VISIBLE
                // silently without unnecessarily shifting focus.
                let mut cloaked: u32 = 1;
                let mut queried: u32 = 0;
                if DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_CLOAKED,
                    &mut queried as *mut u32 as *mut c_void,
                    std::mem::size_of::<u32>() as u32,
                )
                .is_ok()
                {
                    cloaked = queried;
                }
                let action = should_restore_start_menu_visibility(is_visible, cloaked);
                if action != StartMenuVisibilityAction::AlreadyHealthy {
                    let out = &mut *(lparam.0 as *mut Vec<(isize, StartMenuVisibilityAction)>);
                    out.push((hwnd.0 as isize, action));
                }
            }
            BOOL(1)
        }

        let _ = EnumWindows(
            Some(collect_broken_start_windows),
            LPARAM(&mut repairs as *mut Vec<(isize, StartMenuVisibilityAction)> as isize),
        );

        for (hwnd_raw, action) in repairs {
            let hwnd = HWND(hwnd_raw as *mut _);
            match action {
                StartMenuVisibilityAction::AlreadyHealthy => {}
                StartMenuVisibilityAction::RestoreVisibleOnly => {
                    let _ = ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE);
                }
                StartMenuVisibilityAction::RestoreVisibleAndDismiss => {
                    let _ = ShowWindowAsync(hwnd, SW_SHOWNOACTIVATE);
                    let _ = PostMessageW(hwnd, WM_CANCELMODE, WPARAM(0), LPARAM(0));
                    dismiss_native_start_menu(hwnd_raw, 0);
                }
            }
        }
    }
}

/// Dismisses Immersive Windows 11 `StartMenuExperienceHost.exe` cleanly (100% AD-2 compliant).
/// Never call `ShowWindowAsync(SW_HIDE)` on `StartMenuExperienceHost.exe` — `SW_HIDE` clears
/// `WS_VISIBLE` on its persistent `CoreWindow` and breaks Win11 DWM uncloaking even after TestBar exits;
/// Start auto-dismisses immediately the moment foreground focus moves away from it.
/// 1. Moves foreground focus to `fallback_focus_hwnd` (if valid, visible, non-iconic, and not
///    itself `StartMenuExperienceHost.exe`), otherwise falls back to `GetShellWindow()`.
/// 2. If `StartMenuExperienceHost.exe` is STILL foreground after focus transfer, sends a single
///    tagged `VK_ESCAPE` tap (`TESTBAR_INJECTED_EXTRA_INFO`) so Start dismisses immediately.
pub fn dismiss_native_start_menu(start_menu_hwnd: isize, fallback_focus_hwnd: isize) {
    unsafe {
        let valid_fallback = if fallback_focus_hwnd != 0 && fallback_focus_hwnd != start_menu_hwnd {
            let h = HWND(fallback_focus_hwnd as *mut _);
            if IsWindow(h).as_bool()
                && IsWindowVisible(h).as_bool()
                && !IsIconic(h).as_bool()
                && !is_hwnd_native_start_menu(h)
            {
                Some(fallback_focus_hwnd)
            } else {
                None
            }
        } else {
            None
        };

        let target = valid_fallback.or_else(|| {
            let shell = GetShellWindow();
            if !shell.0.is_null() && shell.0 as isize != start_menu_hwnd {
                Some(shell.0 as isize)
            } else {
                None
            }
        });

        if let Some(target_hwnd) = target {
            force_focus_window(target_hwnd);
        }

        let fg = GetForegroundWindow();
        let still_native_start = !fg.0.is_null()
            && ((start_menu_hwnd != 0 && fg.0 as isize == start_menu_hwnd)
                || is_hwnd_native_start_menu(fg));
        if still_native_start {
            let esc = build_tagged_escape_inputs();
            let _ = SendInput(&esc, std::mem::size_of::<INPUT>() as i32);
        }
    }
}

unsafe extern "system" fn win_event_desktop_activity_proc(
    _h_win_event_hook: isize,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    if hwnd.0.is_null() {
        return;
    }
    // Fast reject before touching Win32 GetWindowLongPtrW for non-window/child-element events.
    if matches!(
        event,
        EVENT_OBJECT_CREATE | EVENT_OBJECT_DESTROY | EVENT_OBJECT_SHOW | EVENT_OBJECT_HIDE
    ) && (id_object != OBJID_WINDOW || id_child != CHILDID_SELF)
    {
        return;
    }
    let is_child_hwnd = matches!(
        event,
        EVENT_OBJECT_CREATE | EVENT_OBJECT_DESTROY | EVENT_OBJECT_SHOW | EVENT_OBJECT_HIDE
    ) && ((GetWindowLongPtrW(hwnd, GWL_STYLE) as u32) & WS_CHILD.0) != 0;

    if is_desktop_activity_event(event, id_object, id_child, is_child_hwnd) {
        notify_desktop_activity();
    }
}

/// Out-of-context `EVENT_SYSTEM_FOREGROUND` + `EVENT_OBJECT_UNCLOAKED` suppressor guard
/// (100% AD-2 compliant — zero DLL injection, zero `ReadProcessMemory` on `explorer.exe`).
/// Always wakes the event-driven desktop activity notifier on relevant window transitions, and
/// whenever native Windows 11 Start (`StartMenuExperienceHost.exe` ONLY — never
/// `ShellExperienceHost.exe` / `ScreenClippingHost.exe`) uncloaks or takes foreground focus
/// while TestBar owns the taskbar, emits `ShellHookEvent::NativeStartSuppressed(hwnd)` so
/// `lib.rs` immediately steals focus back (auto-dismissing Immersive Start cleanly!).
unsafe extern "system" fn win_event_start_guard_proc(
    _h_win_event_hook: isize,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    if hwnd.0.is_null() {
        return;
    }
    let relevant = match event {
        EVENT_SYSTEM_FOREGROUND => true,
        EVENT_OBJECT_UNCLOAKED => id_object == OBJID_WINDOW && id_child == CHILDID_SELF,
        _ => false,
    };
    if !relevant {
        return;
    }
    notify_desktop_activity();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !NATIVE_HIDDEN.load(Ordering::Acquire) || is_restore_in_progress() {
            return;
        }
        if !is_hwnd_native_start_menu(hwnd) {
            return;
        }
        let _ = PostMessageW(hwnd, WM_CANCELMODE, WPARAM(0), LPARAM(0));
        let _ = HOOK_EVENT_SENDER.try_with(|slot| {
            if let Ok(guard) = slot.try_borrow() {
                if let Some(sender) = guard.as_ref() {
                    let _ = sender.try_send(ShellHookEvent::NativeStartSuppressed(hwnd.0 as isize));
                }
            }
        });
    }));
}

/// Low-level keyboard & mouse hook thread that intercepts ONLY lone Win taps
/// (leaving Win+E/D/R/L/V/Shift+S/Tab untouched), suppresses stray native Win11
/// `StartMenuExperienceHost.exe` popups via `SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND` +
/// `EVENT_OBJECT_UNCLOAKED`, AD-2), wakes the event-driven desktop poller on top-level window
/// lifecycle & minimize events, forwards global mouse clicks via non-blocking `try_send`
/// into a bounded `sync_channel(64)` drained by a dedicated worker, and registers global
/// emergency restore hotkey `Ctrl+Alt+Shift+Esc`.
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
        let fg_event_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            0,
            win_event_start_guard_proc,
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let uncloak_event_hook = SetWinEventHook(
            EVENT_OBJECT_UNCLOAKED,
            EVENT_OBJECT_UNCLOAKED,
            0,
            win_event_start_guard_proc,
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let minimize_event_hook = SetWinEventHook(
            EVENT_SYSTEM_MINIMIZESTART,
            EVENT_SYSTEM_MINIMIZEEND,
            0,
            win_event_desktop_activity_proc,
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        let object_window_event_hook = SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_HIDE,
            0,
            win_event_desktop_activity_proc,
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );

        if !hotkey_ok
            && hook.is_err()
            && mouse_hook.is_err()
            && fg_event_hook == 0
            && uncloak_event_hook == 0
            && minimize_event_hook == 0
            && object_window_event_hook == 0
        {
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
        if object_window_event_hook != 0 {
            let _ = UnhookWinEvent(object_window_event_hook);
        }
        if minimize_event_hook != 0 {
            let _ = UnhookWinEvent(minimize_event_hook);
        }
        if uncloak_event_hook != 0 {
            let _ = UnhookWinEvent(uncloak_event_hook);
        }
        if fg_event_hook != 0 {
            let _ = UnhookWinEvent(fg_event_hook);
        }
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
pub struct PhysicalKeySnapshot {
    pub lwin_down: bool,
    pub rwin_down: bool,
    pub modifier_down: bool,
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
    // Any mouse click while Win is held (even when no TestBar overlay is open) marks
    // combo_used = true so Win+Click never falsely pops Start on Win release.
    let _ = HOOK_WIN_KEY_STATE.try_with(|cell| {
        let mut state = cell.get();
        if state.is_any_win_down() {
            state.mark_combo_used();
            cell.set(state);
        }
    });
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

/// Builds an atomic 2-event `[win_vk DOWN, key_vk DOWN]` `SendInput` batch for Hold-and-Forward
/// `Win+<key>` chords, preserving exact `scanCode` and `KEYEVENTF_EXTENDEDKEY` (critical for
/// `Win+Arrows`, `Win+Home`, `Win+RShift`, etc.).
pub(crate) fn build_win_chord_forward_inputs(
    win_vk: u16,
    key_vk: u16,
    key_scan: u16,
    key_flags: u32,
) -> [INPUT; 2] {
    let win_scan: u16 = if win_vk == VK_LWIN.0 {
        0x5B
    } else if win_vk == VK_RWIN.0 {
        0x5C
    } else {
        0
    };
    let key_dw_flags = if (key_flags & LLKHF_EXTENDED_FLAG) != 0 {
        KEYEVENTF_EXTENDEDKEY
    } else {
        Default::default()
    };
    [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(win_vk),
                    wScan: win_scan,
                    dwFlags: KEYEVENTF_EXTENDEDKEY,
                    dwExtraInfo: TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(key_vk),
                    wScan: key_scan,
                    dwFlags: key_dw_flags,
                    dwExtraInfo: TESTBAR_INJECTED_EXTRA_INFO,
                    ..Default::default()
                },
            },
        },
    ]
}

/// Atomically injects `[win_vk DOWN, key_vk DOWN]` when the first non-Win key of a chord is
/// pressed while Win is held & swallowed. Returns `true` ONLY when Windows accepted both events.
fn send_win_chord_forward(win_vk: u16, key_vk: u16, key_scan: u16, key_flags: u32) -> bool {
    let inputs = build_win_chord_forward_inputs(win_vk, key_vk, key_scan, key_flags);
    unsafe {
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        sent as usize == inputs.len()
    }
}

/// Processes a single low-level keyboard event via Hold-and-Forward (BUG #4, AD-3).
/// Returns `true` when `ll_keyboard_proc` must return `LRESULT(1)` to swallow the event:
/// - Fresh lone physical `Win DOWN`, repeat `Win DOWN` while unforwarded, and lone `Win UP`
///   are ALL swallowed with ZERO `SendInput` and ZERO `0xE8` — `win32k` never sees Win on taps.
/// - First non-Win key of a chord atomically injects `[win_vk DOWN, key DOWN]` and swallows
///   physical `key DOWN` (except `Win+L`, which swallows `'L'` and enqueues `LockWorkstation`).
fn dispatch_ll_keyboard_event_ex<FQuery, FForwardChord>(
    msg: u32,
    vk_code: u32,
    scan_code: u16,
    flags: u32,
    dw_extra_info: usize,
    fg_elevated: bool,
    query_physical: FQuery,
    forward_chord: FForwardChord,
) -> bool
where
    FQuery: FnOnce() -> PhysicalKeySnapshot,
    FForwardChord: FnOnce(u16, u16, u16, u32) -> bool,
{
    let transition = match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => KeyTransition::Down,
        WM_KEYUP | WM_SYSKEYUP => KeyTransition::Up,
        _ => return false,
    };
    let Ok(vk) = u16::try_from(vk_code) else {
        return false;
    };

    let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;
    let is_injected = (flags & (LLKHF_INJECTED_FLAG | LLKHF_LOWER_IL_INJECTED_FLAG)) != 0;

    // Fast-path pass our own tagged SendInput batches, 0xE8 mask keys, and injected non-Win keys
    // without touching TLS state -> zero re-entrancy hazards, zero typing/IME interference.
    // External untagged injected VK_LWIN/VK_RWIN (Start11/PowerToys/drivers) STILL enter
    // Hold-and-Forward so they can never leak native Start!
    if vk == VK_DUMMY_MASK
        || dw_extra_info == TESTBAR_INJECTED_EXTRA_INFO
        || (is_injected && !is_win)
    {
        return false;
    }

    // Never swallow keys when hook worker sender is torn down.
    let has_sender = HOOK_EVENT_SENDER
        .try_with(|slot| slot.try_borrow().ok().is_some_and(|g| g.is_some()))
        .unwrap_or(false);
    if !has_sender {
        return false;
    }

    let decision = HOOK_WIN_KEY_STATE
        .try_with(|cell| {
            let mut state = cell.get();
            if fg_elevated && !state.is_any_win_down() {
                return WinHookDecision::PASS;
            }
            let phys = if transition == KeyTransition::Down && (state.is_any_win_down() || is_win) {
                query_physical()
            } else {
                PhysicalKeySnapshot::default()
            };
            let dec = state.on_ll_key_event(vk, transition, flags, dw_extra_info, phys, fg_elevated);
            cell.set(state);
            dec
        })
        .unwrap_or(WinHookDecision::PASS);

    if let Some(win_vk) = decision.forward_win_chord_vk {
        if !forward_chord(win_vk, vk, scan_code, flags) {
            // UIPI blocked SendInput (foreground became elevated before poller refreshed):
            // revert win_forwarded so we never claim win32k saw Win DOWN, and pass key through.
            let _ = HOOK_WIN_KEY_STATE.try_with(|cell| {
                let mut state = cell.get();
                state.revert_win_forwarded();
                cell.set(state);
            });
            return false;
        }
        return true;
    }

    match decision.action {
        WinKeyAction::ToggleStart => {
            let _ = HOOK_EVENT_SENDER.try_with(|slot| {
                if let Ok(guard) = slot.try_borrow() {
                    if let Some(sender) = guard.as_ref() {
                        let _ = sender.try_send(ShellHookEvent::ToggleStart);
                    }
                }
            });
        }
        WinKeyAction::LockWorkstation => {
            let _ = HOOK_EVENT_SENDER.try_with(|slot| {
                if let Ok(guard) = slot.try_borrow() {
                    if let Some(sender) = guard.as_ref() {
                        let _ = sender.try_send(ShellHookEvent::LockWorkstation);
                    }
                }
            });
        }
        WinKeyAction::None => {}
    }

    decision.swallow
}

unsafe extern "system" fn ll_keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        let suppress = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            dispatch_ll_keyboard_event_ex(
                wparam.0 as u32,
                kb.vkCode,
                kb.scanCode as u16,
                kb.flags.0,
                kb.dwExtraInfo,
                is_foreground_elevated(),
                query_physical_keys,
                send_win_chord_forward,
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
                let raw = self.hwnd.0 as isize;
                if self.reset_subclass_statics {
                    let orig = overlay_orig_wndprocs()
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&raw)
                        .unwrap_or(0);
                    if orig != 0 {
                        let _ = SetWindowLongPtrW(self.hwnd, GWLP_WNDPROC, orig);
                    }
                    let _ = FLYOUT_SUBCLASSED_HWND.compare_exchange(
                        raw,
                        0,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    );
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
        FLYOUT_NON_ACTIVATING.store(false, Ordering::SeqCst);

        let hwnd = create_hidden_static_window();
        let _guard = TestWindowGuard {
            hwnd,
            reset_subclass_statics: true,
        };
        let hwnd_raw = hwnd.0 as isize;

        install_flyout_noactivate_subclass(hwnd_raw);
        let orig_after_first = overlay_orig_wndprocs()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&hwnd_raw)
            .copied()
            .unwrap_or(0);
        assert_ne!(orig_after_first, 0);
        assert_eq!(FLYOUT_SUBCLASSED_HWND.load(Ordering::SeqCst), hwnd_raw);

        // Idempotent re-install must not overwrite original WndProc with overlay_wnd_proc
        install_flyout_noactivate_subclass(hwnd_raw);
        assert_eq!(
            overlay_orig_wndprocs()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&hwnd_raw)
                .copied()
                .unwrap_or(0),
            orig_after_first
        );

        unsafe {
            set_flyout_non_activating(true);
            let res_noact = SendMessageW(hwnd, WM_MOUSEACTIVATE, WPARAM(0), LPARAM(0));
            assert_eq!(res_noact, LRESULT(MA_NOACTIVATE as isize));

            set_flyout_non_activating(false);
            let res_act = SendMessageW(hwnd, WM_MOUSEACTIVATE, WPARAM(0), LPARAM(0));
            assert_eq!(res_act, LRESULT(MA_ACTIVATE as isize));

            // WM_NCPAINT is suppressed directly with LRESULT(0)
            let res_ncpaint = SendMessageW(hwnd, WM_NCPAINT, WPARAM(1), LPARAM(0));
            assert_eq!(res_ncpaint, LRESULT(0));
        }

        // Verify multi-window overlay subclass rewrites WM_NCACTIVATE lParam -> -1 and blocks WM_NCPAINT
        // while forwarding WM_NCACTIVATE to the underlying (tao) WndProc and cleaning up on WM_NCDESTROY.
        static RECORDED_NCACTIVATE_WPARAM: AtomicU64 = AtomicU64::new(u64::MAX);
        static RECORDED_NCACTIVATE_LPARAM: AtomicIsize = AtomicIsize::new(0);
        static RECORDED_NCPAINT_CALLS: AtomicU32 = AtomicU32::new(0);

        unsafe extern "system" fn recording_wnd_proc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            if msg == WM_NCACTIVATE {
                RECORDED_NCACTIVATE_WPARAM.store(wparam.0 as u64, Ordering::SeqCst);
                RECORDED_NCACTIVATE_LPARAM.store(lparam.0, Ordering::SeqCst);
            } else if msg == WM_NCPAINT {
                RECORDED_NCPAINT_CALLS.fetch_add(1, Ordering::SeqCst);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        let start_hwnd = create_hidden_static_window();
        let start_raw = start_hwnd.0 as isize;
        unsafe {
            SetWindowLongPtrW(
                start_hwnd,
                GWLP_WNDPROC,
                recording_wnd_proc as *const () as usize as isize,
            );
        }
        RECORDED_NCACTIVATE_WPARAM.store(u64::MAX, Ordering::SeqCst);
        RECORDED_NCACTIVATE_LPARAM.store(0, Ordering::SeqCst);
        RECORDED_NCPAINT_CALLS.store(0, Ordering::SeqCst);

        install_overlay_borderless_subclass(start_raw);
        assert!(
            overlay_orig_wndprocs()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&start_raw)
        );
        // Subclassing start_hwnd must NOT disturb flyout's subclass entry
        assert_eq!(FLYOUT_SUBCLASSED_HWND.load(Ordering::SeqCst), hwnd_raw);

        unsafe {
            let nc_res = SendMessageW(start_hwnd, WM_NCACTIVATE, WPARAM(1), LPARAM(0));
            assert_ne!(nc_res, LRESULT(0), "DefWindowProcW(WM_NCACTIVATE, 1, -1) must return TRUE");
            assert_eq!(RECORDED_NCACTIVATE_WPARAM.load(Ordering::SeqCst), 1);
            assert_eq!(
                RECORDED_NCACTIVATE_LPARAM.load(Ordering::SeqCst),
                -1,
                "WM_NCACTIVATE must forward lParam=-1 so DefWindowProcW never paints classic caption"
            );

            let nc_deact = SendMessageW(start_hwnd, WM_NCACTIVATE, WPARAM(0), LPARAM(0));
            assert_ne!(nc_deact, LRESULT(0));
            assert_eq!(RECORDED_NCACTIVATE_WPARAM.load(Ordering::SeqCst), 0);
            assert_eq!(RECORDED_NCACTIVATE_LPARAM.load(Ordering::SeqCst), -1);

            let paint_res = SendMessageW(start_hwnd, WM_NCPAINT, WPARAM(1), LPARAM(0));
            assert_eq!(paint_res, LRESULT(0));
            assert_eq!(
                RECORDED_NCPAINT_CALLS.load(Ordering::SeqCst),
                0,
                "WM_NCPAINT must never reach DefWindowProcW"
            );

            let _ = DestroyWindow(start_hwnd);
        }

        assert!(
            !overlay_orig_wndprocs()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains_key(&start_raw),
            "WM_NCDESTROY must clean up subclass table entry"
        );
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
    fn style_overlay_window_strips_frame_edges_and_sets_popup_tool_topmost() {
        let _z_lock = crate::WIN32_TOPMOST_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let hwnd = unsafe {
            let cls = wide("STATIC");
            CreateWindowExW(
                WS_EX_APPWINDOW | WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_DLGMODALFRAME,
                PCWSTR(cls.as_ptr()),
                PCWSTR::null(),
                WS_OVERLAPPED | WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
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
        };
        let _guard = TestWindowGuard {
            hwnd,
            reset_subclass_statics: true,
        };

        style_overlay_window(hwnd.0 as isize, true);
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            assert_eq!(
                style
                    & (WS_CAPTION.0
                        | WS_THICKFRAME.0
                        | WS_BORDER.0
                        | WS_DLGFRAME.0
                        | WS_SYSMENU.0
                        | WS_MINIMIZEBOX.0
                        | WS_MAXIMIZEBOX.0),
                0,
                "all frame/caption/border styles must be stripped"
            );
            assert_ne!(style & WS_POPUP.0, 0);
            assert_ne!(style & WS_CLIPSIBLINGS.0, 0);
            assert_ne!(style & WS_CLIPCHILDREN.0, 0);

            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            assert_eq!(
                ex & (WS_EX_APPWINDOW.0
                    | WS_EX_WINDOWEDGE.0
                    | WS_EX_DLGMODALFRAME.0
                    | WS_EX_CLIENTEDGE.0),
                0,
                "WS_EX_APPWINDOW and all extended frame edges must be stripped"
            );
            assert_ne!(ex & WS_EX_TOOLWINDOW.0, 0);
            assert_ne!(ex & WS_EX_TOPMOST.0, 0);
            assert_ne!(ex & WS_EX_NOACTIVATE.0, 0);
            assert!(
                overlay_orig_wndprocs()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains_key(&(hwnd.0 as isize)),
                "style_overlay_window must auto-install borderless non-client subclass"
            );
        }

        // Switching no_activate = false clears WS_EX_NOACTIVATE while keeping frame stripped
        style_overlay_window(hwnd.0 as isize, false);
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            assert_eq!(ex & WS_EX_NOACTIVATE.0, 0);
            assert_ne!(ex & WS_EX_TOOLWINDOW.0, 0);
            assert_ne!(ex & WS_EX_TOPMOST.0, 0);
        }
    }

    #[test]
    fn appbar_rect_anchors_to_primary_monitor_bottom_without_stacking() {
        let mon = PixelRect::new(0, 0, 3840, 1600);
        let dock_96 = compute_appbar_rect(mon, 96, BarMode::Dock);
        assert_eq!(dock_96, PixelRect::new(0, 1600 - 67, 3840, 1600));
        assert_eq!(
            resolve_bottom_appbar_rect(mon, dock_96.height()),
            PixelRect::new(0, 1533, 3840, 1600)
        );

        let edge_144 = compute_appbar_rect(mon, 144, BarMode::Edge);
        assert_eq!(edge_144, PixelRect::new(0, 1600 - 87, 3840, 1600));
        assert_eq!(
            resolve_bottom_appbar_rect(mon, edge_144.height()),
            PixelRect::new(0, 1513, 3840, 1600)
        );

        // Negative or oversized height clamps cleanly inside monitor bounds
        assert_eq!(resolve_bottom_appbar_rect(mon, -10), PixelRect::new(0, 1600, 3840, 1600));
        assert_eq!(resolve_bottom_appbar_rect(mon, 9999), mon);
    }

    #[test]
    fn top_level_appbar_message_window_reconciliation_retry_dedup_dpi_and_cleanup() {
        let (ev_tx, ev_rx) = mpsc::channel::<ShellAppBarEvent>();
        let mon = PixelRect::new(0, 0, 2560, 1440);
        let fake_state = Arc::new(Mutex::new(FakeShellState {
            monitor: mon,
            dpi: 144, // 150% DPI -> Dock 67*1.5=100.5->101px, Edge 58*1.5=87px
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

            // Next reconciliation retries ABM_NEW (which now succeeds) and reserves Dock 101px at 144 DPI
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
            let dock_144 = PixelRect::new(0, 1440 - 101, 2560, 1440);
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

            // ABN_POSCHANGED where hidden Shell_TrayWnd shifts QueryPos rc.bottom up by 40px
            // MUST NOT stack vertically — resolved rect stays anchored to primary monitor bottom (1440)!
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
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![FakeOp::AbmQueryPos(PixelRect::new(0, 1440 - 101, 2560, 1400))]
            );
            fake_state.lock().unwrap().querypos_bottom_offset = 0;
            fake_state.lock().unwrap().ops.clear();

            // Live mode update to Edge switches height to 58 * 1.5 = 87px
            wnd.update_mode(BarMode::Edge);
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::PositionChanged
            );
            let edge_144 = PixelRect::new(0, 1440 - 87, 2560, 1440);
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![FakeOp::AbmQueryPos(edge_144), FakeOp::AbmSetPos(edge_144)]
            );
            fake_state.lock().unwrap().ops.clear();

            // WM_DPICHANGED propagates wparam DPI (192 = 200%) -> Edge 58 * 2 = 116px even while fake DPI is 144
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
            let edge_192 = PixelRect::new(0, 1440 - 116, 2560, 1440);
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

        // Standard Win+<key> shortcuts: Win+E/D/R/V/Tab forward chord, Win+L emits LockWorkstation
        for vk in [0x45u16, 0x44, 0x52, 0x56, 0x09] {
            assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
            assert_eq!(sm.on_key_event(vk, KeyTransition::Down, 0), WinKeyAction::None);
            assert_eq!(sm.on_key_event(vk, KeyTransition::Up, 0), WinKeyAction::None);
            assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::None);
        }
        // Pure Win+L emits LockWorkstation and clears held state immediately
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, 0), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(0x4Cu16, KeyTransition::Down, 0),
            WinKeyAction::LockWorkstation
        );
        assert!(!sm.is_any_win_down());
        assert_eq!(sm.on_key_event(0x4Cu16, KeyTransition::Up, 0), WinKeyAction::None);
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Up, 0), WinKeyAction::None);

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
        let idle = PhysicalKeySnapshot::default();

        // TestBar's own tagged injected Win events (TESTBAR_INJECTED_EXTRA_INFO) bypass cleanly
        for inj_flag in [LLKHF_INJECTED_FLAG, LLKHF_LOWER_IL_INJECTED_FLAG] {
            let dec_down = sm.on_ll_key_event(
                lwin,
                KeyTransition::Down,
                inj_flag,
                TESTBAR_INJECTED_EXTRA_INFO,
                idle,
                false,
            );
            assert_eq!(dec_down, WinHookDecision::PASS);
            assert!(!sm.is_any_win_down());
            let dec_up = sm.on_ll_key_event(
                lwin,
                KeyTransition::Up,
                inj_flag,
                TESTBAR_INJECTED_EXTRA_INFO,
                idle,
                false,
            );
            assert_eq!(dec_up, WinHookDecision::PASS);
        }

        // External untagged injected Win taps (e.g. Start11 / PowerToys / drivers) MUST be
        // swallowed by Hold-and-Forward so they never leak native Windows 11 Start!
        for inj_flag in [LLKHF_INJECTED_FLAG, LLKHF_LOWER_IL_INJECTED_FLAG] {
            let dec_down = sm.on_ll_key_event(lwin, KeyTransition::Down, inj_flag, 0, idle, false);
            assert_eq!(dec_down, WinHookDecision::SWALLOW);
            assert!(sm.is_any_win_down());
            let dec_up = sm.on_ll_key_event(lwin, KeyTransition::Up, inj_flag, 0, idle, false);
            assert!(dec_up.swallow);
            assert_eq!(dec_up.action, WinKeyAction::ToggleStart);
            assert!(!sm.is_any_win_down());
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

        // Combined injected bitmasks: non-Win injected keys and TestBar's own tagged injected LWin UP are ignored
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(key_a, KeyTransition::Down, ext | inj),
            WinKeyAction::None
        );
        assert_eq!(
            sm.on_ll_key_event(
                lwin,
                KeyTransition::Up,
                ext | up | inj,
                TESTBAR_INJECTED_EXTRA_INFO,
                PhysicalKeySnapshot::default(),
                false,
            ),
            WinHookDecision::PASS
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

        // Forwarded chord (Win+D) whose LWin Up is lost on UAC/Secure Desktop -> reconcile_physical_win_keys clears stuck state
        let key_d = 0x44u16;
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(sm.on_key_event(key_d, KeyTransition::Down, 0), WinKeyAction::None);
        assert!(sm.is_any_win_down());
        assert!(sm.is_win_forwarded());
        sm.reconcile_physical_win_keys(false, false);
        assert!(!sm.is_any_win_down());
        assert!(!sm.is_win_forwarded());

        // Win+L immediately triggers LockWorkstation and resets Win state cleanly for post-unlock taps
        assert_eq!(sm.on_key_event(lwin, KeyTransition::Down, ext), WinKeyAction::None);
        assert_eq!(
            sm.on_key_event(key_l, KeyTransition::Down, 0),
            WinKeyAction::LockWorkstation
        );
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
    fn hold_and_forward_rapid_taps_chords_win_l_modifier_first_elevated_and_mouse_gate() {
        use windows::Win32::UI::WindowsAndMessaging::{
            LLKHF_ALTDOWN, LLKHF_EXTENDED, LLKHF_INJECTED, LLKHF_UP, WM_MOUSEMOVE,
        };

        assert_eq!(LLKHF_EXTENDED_FLAG, LLKHF_EXTENDED.0);

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
        let inj = LLKHF_INJECTED.0;
        let lwin = VK_LWIN.0 as u32;
        let rwin = VK_RWIN.0 as u32;
        let key_d = 0x44u32;
        let key_e = 0x45u32;
        let key_l = 0x4Cu32;
        let key_s = 0x53u32;
        let key_tab = 0x09u32;
        let key_lshift = 0xA0u32;
        let key_right = 0x27u32; // VK_RIGHT (extended!)
        let key_up = 0x26u32; // VK_UP (extended!)

        let (sender, rx) = HookEventChannel::with_capacity(16).into_parts();
        HOOK_WIN_KEY_STATE.with(|c| c.set(WinKeyStateMachine::new()));
        HOOK_EVENT_SENDER.with(|slot| *slot.borrow_mut() = Some(sender.clone()));

        let idle_phys = || PhysicalKeySnapshot::default();
        let forwarded: RefCell<Vec<(u16, u16, u16, u32)>> = RefCell::new(Vec::new());
        let ok_forward = |win_vk: u16, key_vk: u16, scan: u16, flags: u32| -> bool {
            forwarded.borrow_mut().push((win_vk, key_vk, scan, flags));
            true
        };
        let blocked_forward = |win_vk: u16, key_vk: u16, scan: u16, flags: u32| -> bool {
            forwarded.borrow_mut().push((win_vk, key_vk, scan, flags));
            false
        };

        // 1. Rapid 1x, 2x, 3x, 5x taps (LWin + RWin + OS key-repeat):
        //    EVERY physical DOWN, repeat DOWN, and physical UP is SWALLOWED (true),
        //    ZERO SendInput chord forwards happen -> win32k NEVER sees Win!
        for (idx, vk) in [lwin, lwin, lwin, rwin, lwin].into_iter().enumerate() {
            assert!(
                dispatch_ll_keyboard_event_ex(WM_KEYDOWN, vk, 0x5B, ext, 0, false, idle_phys, ok_forward),
                "tap #{idx} physical Win KEYDOWN must be swallowed"
            );
            if idx == 1 {
                // OS auto-repeat while held (note: GetAsyncKeyState is 0 because Win DOWN was swallowed!)
                assert!(
                    dispatch_ll_keyboard_event_ex(WM_KEYDOWN, vk, 0x5B, ext, 0, false, idle_phys, ok_forward),
                    "auto-repeat Win KEYDOWN while unforwarded must stay swallowed"
                );
            }
            assert!(
                dispatch_ll_keyboard_event_ex(WM_KEYUP, vk, 0x5B, ext | up, 0, false, idle_phys, ok_forward),
                "tap #{idx} physical Win KEYUP must be swallowed"
            );
            assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::ToggleStart);
        }
        assert!(
            forwarded.borrow().is_empty(),
            "lone rapid taps must invoke ZERO SendInput calls"
        );

        // 2. Win+L: swallows LWin DOWN + 'L' DOWN, enqueues LockWorkstation, zero SendInput!
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, lwin, 0x5B, ext, 0, false, idle_phys, ok_forward
        ));
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, key_l, 0x26, 0, 0, false, idle_phys, ok_forward
        ));
        assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::LockWorkstation);
        assert!(forwarded.borrow().is_empty());
        // Stray key-ups after unlock pass harmlessly and next lone Win tap works immediately
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, key_l, 0x26, up, 0, false, idle_phys, ok_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, ok_forward
        ));
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, lwin, 0x5B, ext, 0, false, idle_phys, ok_forward
        ));
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, ok_forward
        ));
        assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::ToggleStart);

        // 3. Win+D, Win+E, Win+Tab, Win+Right -> Win+Up (extended arrows!), Win+Shift+S (Win-first):
        //    First non-Win key atomically forwards [LWin DOWN, key DOWN] and swallows physical key DOWN;
        //    re-entrant TESTBAR_INJECTED_EXTRA_INFO events pass untouched; subsequent keys & physical Win UP pass through!
        for (vk, scan, flg) in [
            (key_d, 0x20u16, 0u32),
            (key_e, 0x12u16, 0u32),
            (key_tab, 0x0Fu16, 0u32),
            (key_right, 0x4Du16, ext),
        ] {
            forwarded.borrow_mut().clear();
            assert!(dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, lwin, 0x5B, ext, 0, false, idle_phys, ok_forward
            ));
            // Physical non-Win key DOWN triggers atomic forward and is swallowed (not duplicated)
            assert!(dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, vk, scan, flg, 0, false, idle_phys, ok_forward
            ));
            assert_eq!(*forwarded.borrow(), vec![(VK_LWIN.0, vk as u16, scan, flg)]);
            // Simulated re-entrant tagged [LWin DOWN, key DOWN] pass straight through untouched
            assert!(!dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, lwin, 0x5B, ext | inj, TESTBAR_INJECTED_EXTRA_INFO, false, idle_phys, ok_forward
            ));
            assert!(!dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, vk, scan, flg | inj, TESTBAR_INJECTED_EXTRA_INFO, false, idle_phys, ok_forward
            ));
            // Multi-step snap: pressing VK_UP while LWin is STILL held passes straight through (no 2nd forward!)
            if vk == key_right {
                let lwin_held = || PhysicalKeySnapshot {
                    lwin_down: true,
                    rwin_down: false,
                    modifier_down: false,
                };
                assert!(!dispatch_ll_keyboard_event_ex(
                    WM_KEYUP, key_right, 0x4D, ext | up, 0, false, lwin_held, ok_forward
                ));
                assert!(!dispatch_ll_keyboard_event_ex(
                    WM_KEYDOWN, key_up, 0x48, ext, 0, false, lwin_held, ok_forward
                ));
                assert!(!dispatch_ll_keyboard_event_ex(
                    WM_KEYUP, key_up, 0x48, ext | up, 0, false, lwin_held, ok_forward
                ));
                assert_eq!(forwarded.borrow().len(), 1);
            } else {
                assert!(!dispatch_ll_keyboard_event_ex(
                    WM_KEYUP, vk, scan, flg | up, 0, false, idle_phys, ok_forward
                ));
            }
            // Physical LWin UP passes through CallNextHookEx to release Win cleanly in win32k
            assert!(!dispatch_ll_keyboard_event_ex(
                WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, ok_forward
            ));
            assert!(rx.try_recv().is_err());
        }

        // Verify build_win_chord_forward_inputs preserves KEYEVENTF_EXTENDEDKEY & tag,
        // and build_tagged_escape_inputs carries VK_ESCAPE + TESTBAR_INJECTED_EXTRA_INFO!
        let arrow_batch = build_win_chord_forward_inputs(VK_LWIN.0, key_right as u16, 0x4D, ext);
        unsafe {
            assert_eq!(arrow_batch[0].Anonymous.ki.wVk, VK_LWIN);
            assert_eq!(arrow_batch[0].Anonymous.ki.wScan, 0x5B);
            assert_eq!(arrow_batch[0].Anonymous.ki.dwFlags, KEYEVENTF_EXTENDEDKEY);
            assert_eq!(arrow_batch[0].Anonymous.ki.dwExtraInfo, TESTBAR_INJECTED_EXTRA_INFO);
            assert_eq!(arrow_batch[1].Anonymous.ki.wVk, VIRTUAL_KEY(key_right as u16));
            assert_eq!(arrow_batch[1].Anonymous.ki.wScan, 0x4D);
            assert_eq!(arrow_batch[1].Anonymous.ki.dwFlags, KEYEVENTF_EXTENDEDKEY);
            assert_eq!(arrow_batch[1].Anonymous.ki.dwExtraInfo, TESTBAR_INJECTED_EXTRA_INFO);

            let esc_batch = build_tagged_escape_inputs();
            assert_eq!(esc_batch[0].Anonymous.ki.wVk, VK_ESCAPE);
            assert_eq!(esc_batch[0].Anonymous.ki.dwExtraInfo, TESTBAR_INJECTED_EXTRA_INFO);
            assert_eq!(esc_batch[1].Anonymous.ki.wVk, VK_ESCAPE);
            assert_eq!(esc_batch[1].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
            assert_eq!(esc_batch[1].Anonymous.ki.dwExtraInfo, TESTBAR_INJECTED_EXTRA_INFO);
        }

        // 4. Win+Shift+S (Win-first): bare LShift passes through natively WITHOUT forwarding Win yet,
        //    then 'S' atomically forwards [LWin, S] while Shift is already down in win32k!
        forwarded.borrow_mut().clear();
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, lwin, 0x5B, ext, 0, false, idle_phys, ok_forward
        ));
        assert!(
            !dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, key_lshift, 0x2A, 0, 0, false, idle_phys, ok_forward
            ),
            "bare Shift while Win held must pass through without forwarding Win yet"
        );
        assert!(
            forwarded.borrow().is_empty(),
            "no Win DOWN may be forwarded on bare Shift alone"
        );
        let win_shift_held = || PhysicalKeySnapshot {
            lwin_down: true,
            rwin_down: false,
            modifier_down: true,
        };
        assert!(
            dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, key_s, 0x1F, 0, 0, false, win_shift_held, ok_forward
            ),
            "non-modifier 'S' must swallow physical 'S' and forward [LWin, S]"
        );
        assert_eq!(*forwarded.borrow(), vec![(VK_LWIN.0, key_s as u16, 0x1F, 0)]);
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, key_s, 0x1F, up, 0, false, idle_phys, ok_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, key_lshift, 0x2A, up, 0, false, idle_phys, ok_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, ok_forward
        ));
        assert!(rx.try_recv().is_err());

        // 4b. Grazed/aborted bare modifier while Win held (Win+Shift / Win+Ctrl / Win+Alt released
        //     without pressing a non-modifier key): ZERO Win forwarded (zero native Start!) AND
        //     physical Win UP is swallowed with ZERO TestBar ToggleStart!
        for mod_vk in [0xA0u32, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5] {
            forwarded.borrow_mut().clear();
            assert!(dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, lwin, 0x5B, ext, 0, false, idle_phys, ok_forward
            ));
            assert!(!dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, mod_vk, 0x2A, 0, 0, false, idle_phys, ok_forward
            ));
            assert!(!dispatch_ll_keyboard_event_ex(
                WM_KEYUP, mod_vk, 0x2A, up, 0, false, idle_phys, ok_forward
            ));
            assert!(
                dispatch_ll_keyboard_event_ex(
                    WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, ok_forward
                ),
                "unforwarded Win UP after bare modifier {mod_vk:#x} must still be swallowed"
            );
            assert!(
                forwarded.borrow().is_empty(),
                "aborted Win+modifier {mod_vk:#x} must NEVER send Win DOWN to win32k"
            );
            assert!(
                rx.try_recv().is_err(),
                "aborted Win+modifier {mod_vk:#x} must NOT toggle TestBar Start"
            );
        }

        // 5a. LEAK 1 FIX — Modifier-first Shift+Win / Alt+Win released WITHOUT a 3rd key:
        //     SWALLOWS both Win DOWN and Win UP -> ZERO Win forwarded to win32k, ZERO native Start, ZERO TestBar Start!
        forwarded.borrow_mut().clear();
        let mod_held = || PhysicalKeySnapshot {
            lwin_down: false,
            rwin_down: false,
            modifier_down: true,
        };
        assert!(
            dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, lwin, 0x5B, ext, 0, false, mod_held, ok_forward
            ),
            "modifier-first Shift+Win DOWN must be swallowed until a 3rd non-modifier key arrives"
        );
        assert!(
            dispatch_ll_keyboard_event_ex(
                WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, ok_forward
            ),
            "aborted modifier-first Shift+Win UP must be swallowed"
        );
        assert!(
            dispatch_ll_keyboard_event_ex(
                WM_SYSKEYDOWN, lwin, 0x5B, ext | alt, 0, false, idle_phys, ok_forward
            ),
            "modifier-first Alt+Win DOWN must be swallowed"
        );
        assert!(
            dispatch_ll_keyboard_event_ex(
                WM_SYSKEYUP, lwin, 0x5B, ext | up | alt, 0, false, idle_phys, ok_forward
            ),
            "aborted modifier-first Alt+Win UP must be swallowed"
        );
        assert!(forwarded.borrow().is_empty());
        assert!(rx.try_recv().is_err());

        // 5b. LEAK 1 FIX — Modifier-first Shift+Win+S: Win DOWN swallowed, then 'S' atomically
        //     forwards [LWin DOWN, 'S' DOWN] while Shift is already held in win32k!
        forwarded.borrow_mut().clear();
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, lwin, 0x5B, ext, 0, false, mod_held, ok_forward
        ));
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, key_s, 0x1F, 0, 0, false, win_shift_held, ok_forward
        ));
        assert_eq!(*forwarded.borrow(), vec![(VK_LWIN.0, key_s as u16, 0x1F, 0)]);
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, key_s, 0x1F, up, 0, false, idle_phys, ok_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, ok_forward
        ));
        assert!(rx.try_recv().is_err());

        // 5c. LEAK 2 FIX — Tagged TESTBAR_INJECTED_EXTRA_INFO (e.g. Win+N) bypasses hook untouched,
        //     while external untagged LLKHF_INJECTED VK_LWIN (Start11/PowerToys/drivers) is SWALLOWED!
        forwarded.borrow_mut().clear();
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, lwin, 0x5B, ext | inj, TESTBAR_INJECTED_EXTRA_INFO, false, idle_phys, ok_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, b'N' as u32, 0x31, inj, TESTBAR_INJECTED_EXTRA_INFO, false, idle_phys, ok_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, b'N' as u32, 0x31, inj | up, TESTBAR_INJECTED_EXTRA_INFO, false, idle_phys, ok_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, lwin, 0x5B, ext | inj | up, TESTBAR_INJECTED_EXTRA_INFO, false, idle_phys, ok_forward
        ));
        assert!(rx.try_recv().is_err());

        assert!(
            dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, lwin, 0x5B, ext | inj, 0, false, idle_phys, ok_forward
            ),
            "external untagged injected LWin DOWN must be swallowed"
        );
        assert!(
            dispatch_ll_keyboard_event_ex(
                WM_KEYUP, lwin, 0x5B, ext | inj | up, 0, false, idle_phys, ok_forward
            ),
            "external untagged injected LWin UP must be swallowed"
        );
        assert_eq!(rx.try_recv().unwrap(), ShellHookEvent::ToggleStart);

        // 6. Elevated Admin foreground window (fg_elevated = true): passes Win taps & chords completely untouched!
        forwarded.borrow_mut().clear();
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, lwin, 0x5B, ext, 0, true, idle_phys, ok_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYUP, lwin, 0x5B, ext | up, 0, true, idle_phys, ok_forward
        ));
        assert!(forwarded.borrow().is_empty());
        assert!(rx.try_recv().is_err());

        // 6b. UIPI fallback if SendInput unexpectedly returns false during chord forward:
        //     reverts win_forwarded and does NOT swallow the physical key!
        forwarded.borrow_mut().clear();
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, lwin, 0x5B, ext, 0, false, idle_phys, blocked_forward
        ));
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, key_d, 0x20, 0, 0, false, idle_phys, blocked_forward
        ));
        // Physical LWin UP is still swallowed (since win32k never saw Win DOWN) and does NOT open Start (combo_used=true)
        assert!(dispatch_ll_keyboard_event_ex(
            WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, blocked_forward
        ));
        assert!(rx.try_recv().is_err());

        // 7. Win + Mouse Click marks combo_used so Win release swallows without opening Start
        for mouse_msg in [WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN] {
            assert!(dispatch_ll_keyboard_event_ex(
                WM_KEYDOWN, lwin, 0x5B, ext, 0, false, idle_phys, ok_forward
            ));
            dispatch_ll_mouse_event(mouse_msg, 123, 456, false);
            assert!(dispatch_ll_keyboard_event_ex(
                WM_KEYUP, lwin, 0x5B, ext | up, 0, false, idle_phys, ok_forward
            ));
            assert!(rx.try_recv().is_err());
        }

        // 8. Out-of-range vkCode (> u16::MAX) is ignored safely
        assert!(!dispatch_ll_keyboard_event_ex(
            WM_KEYDOWN, (1u32 << 16) | lwin, 0, ext, 0, false, idle_phys, ok_forward
        ));

        // 9. Mouse dispatch is gated by overlay_open across L/R/M button downs
        dispatch_ll_mouse_event(WM_LBUTTONDOWN, 10, 20, false);
        dispatch_ll_mouse_event(WM_RBUTTONDOWN, 30, 40, false);
        dispatch_ll_mouse_event(WM_MBUTTONDOWN, 50, 60, false);
        assert!(rx.try_recv().is_err());

        dispatch_ll_mouse_event(WM_MOUSEMOVE, 99, 99, true);
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
    fn appbar_message_window_neutralizes_text_scale_factor_and_notifies_on_setting_change() {
        let (ev_tx, ev_rx) = mpsc::channel::<ShellAppBarEvent>();
        let mon = PixelRect::new(0, 0, 2560, 1440);
        let fake_state = Arc::new(Mutex::new(FakeShellState {
            monitor: mon,
            dpi: 96,         // 100% DPI -> AppBar stays crisp 67px Dock / 58px Edge despite 140% TextScaleFactor!
            text_scale: 1.4,
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

        // Dock 67px at 96 DPI (TextScaleFactor neutralized on taskbar)
        let expected_dock = PixelRect::new(0, 1440 - 67, 2560, 1440);
        assert_eq!(
            fake_state.lock().unwrap().ops,
            vec![
                FakeOp::AbmNew(true),
                FakeOp::AbmQueryPos(expected_dock),
                FakeOp::AbmSetPos(expected_dock),
            ]
        );
        fake_state.lock().unwrap().ops.clear();

        // Edge 58px at 96 DPI
        wnd.update_mode(BarMode::Edge);
        assert_eq!(
            ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            ShellAppBarEvent::PositionChanged
        );
        let expected_edge = PixelRect::new(0, 1440 - 58, 2560, 1440);
        assert_eq!(
            fake_state.lock().unwrap().ops,
            vec![FakeOp::AbmQueryPos(expected_edge), FakeOp::AbmSetPos(expected_edge)]
        );
        fake_state.lock().unwrap().ops.clear();

        // Unrelated WM_SETTINGCHANGE (TextScaleFactor unchanged at 1.4) does nothing
        unsafe {
            let hwnd = HWND(wnd.hwnd() as *mut _);
            let _ = SendMessageW(hwnd, WM_SETTINGCHANGE, WPARAM(0), LPARAM(0));
            assert!(fake_state.lock().unwrap().ops.is_empty());
            assert!(ev_rx.try_recv().is_err());

            // Live TextScaleFactor change (1.4 -> 1.75): notifies Start via PositionChanged without changing DPI-only AppBar rect!
            fake_state.lock().unwrap().text_scale = 1.75;
            let _ = SendMessageW(hwnd, WM_SETTINGCHANGE, WPARAM(0), LPARAM(0));
            assert_eq!(
                ev_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
                ShellAppBarEvent::PositionChanged
            );
            assert_eq!(
                fake_state.lock().unwrap().ops,
                vec![FakeOp::AbmQueryPos(expected_edge)]
            );
        }

        wnd.stop();
    }

    #[test]
    fn clean_exit_event_roundtrip_and_zero_pid_rejection() {
        assert!(create_clean_exit_event(0).is_err());
        assert!(!signal_clean_exit_event(0));
        assert!(!is_clean_exit_signaled(0));

        let fake_pid = 900_000 + TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed) as u32;
        assert!(!is_clean_exit_signaled(fake_pid));

        let guard = create_clean_exit_event(fake_pid).expect("create_clean_exit_event failed");
        assert_eq!(guard.pid(), fake_pid);
        assert!(!guard.is_signaled());
        assert!(!is_clean_exit_signaled(fake_pid));

        assert!(signal_clean_exit_event(fake_pid));
        assert!(guard.is_signaled());
        assert!(is_clean_exit_signaled(fake_pid));

        drop(guard);
        assert!(!is_clean_exit_signaled(fake_pid));
    }

    #[test]
    fn recovery_workarea_rejects_shrunk_monitor_cli_and_stale_dpi_snapshot() {
        let dir = unique_test_dir("recovery_expanded");
        let path = dir.join("recovery-workarea.json");

        let mon_1440 = PixelRect::new(0, 0, 2560, 1440);
        let stale_1080_wa = PixelRect::new(0, 0, 1920, 1032);

        // Stale 1080p CLI --workarea on an expanded 1440p monitor MUST be rejected (only 2 edges match!)
        assert!(!is_workarea_compatible_with_monitor(stale_1080_wa, mon_1440));
        assert_eq!(
            resolve_recovery_workarea(Some(stale_1080_wa), None, &path, mon_1440, 96),
            None
        );

        // Same 1440p resolution, but DPI changed from 96 -> 144: explicit CLI matching stale 96-DPI disk snapshot is rejected!
        let stale_96_dpi_wa = PixelRect::new(0, 0, 2560, 1392);
        let disk_snap_96 = WorkAreaSnapshot {
            work_area: stale_96_dpi_wa,
            primary_monitor: mon_1440,
            dpi: 96,
        };
        save_workarea_snapshot_to(&path, &disk_snap_96).unwrap();
        assert_eq!(
            resolve_recovery_workarea(Some(stale_96_dpi_wa), None, &path, mon_1440, 144),
            None
        );
        // Matching DPI accepts it cleanly
        assert_eq!(
            resolve_recovery_workarea(Some(stale_96_dpi_wa), None, &path, mon_1440, 96),
            Some(stale_96_dpi_wa)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_bounds_hit_transparent_and_glide_interpolation() {
        let _z_lock = crate::WIN32_TOPMOST_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(interpolate_glide_x(100, 300, 0.0), 100);
        assert_eq!(interpolate_glide_x(100, 300, 1.0), 300);
        assert_eq!(interpolate_glide_x(100, 300, 0.5), 275); // cubic ease-out: 1 - 0.5^3 = 0.875 -> 100 + 175
        assert_eq!(interpolate_glide_x(300, 100, 1.5), 100);
        assert_eq!(interpolate_glide_x(300, 100, f64::NAN), 100);

        let hwnd = create_hidden_static_window();
        let _guard = TestWindowGuard {
            hwnd,
            reset_subclass_statics: false,
        };
        let raw = hwnd.0 as isize;

        set_window_bounds_atomic(raw, 200, 180, 420, 260, true);
        unsafe {
            assert!(!IsWindowVisible(hwnd).as_bool());
            let mut r = RECT::default();
            GetWindowRect(hwnd, &mut r).unwrap();
            assert_eq!((r.left, r.top, r.right, r.bottom), (200, 180, 620, 440));

            set_window_hit_transparent(raw, true);
            let ex_on = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            assert_ne!(ex_on & WS_EX_TRANSPARENT.0, 0);

            set_window_hit_transparent(raw, false);
            let ex_off = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            assert_eq!(ex_off & WS_EX_TRANSPARENT.0, 0);
        }
    }

    #[test]
    fn native_start_menu_exe_and_bare_modifier_classification() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
        };

        // Strictly matches StartMenuExperienceHost.exe ONLY
        assert!(is_native_start_menu_exe("StartMenuExperienceHost.exe"));
        assert!(is_native_start_menu_exe("startmenuexperiencehost.exe"));
        assert!(is_native_start_menu_exe(
            r"C:\Windows\SystemApps\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\StartMenuExperienceHost.exe"
        ));
        assert!(is_native_start_menu_exe(
            "C:/Windows/SystemApps/StartMenuExperienceHost/STARTMENUEXPERIENCEHOST.EXE"
        ));

        // Strictly rejects Win+N Notifications (ShellExperienceHost), Snipping (ScreenClippingHost), Search, etc.
        for non_start in [
            "",
            "   ",
            "ShellExperienceHost.exe",
            r"C:\Windows\SystemApps\ShellExperienceHost_cw5n1h2txyewy\ShellExperienceHost.exe",
            r"C:\Windows\SystemApps\ScreenClippingHost\ScreenClippingHost.exe",
            r"C:\Windows\SystemApps\SearchHost\SearchHost.exe",
            r"C:\Windows\explorer.exe",
            r"C:\Apps\TestBar\TestBar-v2.exe",
        ] {
            assert!(
                !is_native_start_menu_exe(non_start),
                "{non_start:?} must NOT be suppressed as StartMenuExperienceHost.exe"
            );
        }

        // Bare modifiers match all 9 Win32 Shift/Ctrl/Alt VK codes and reject Win/letter/arrow keys
        for m in [
            VK_SHIFT.0,
            VK_CONTROL.0,
            VK_MENU.0,
            VK_LSHIFT.0,
            VK_RSHIFT.0,
            VK_LCONTROL.0,
            VK_RCONTROL.0,
            VK_LMENU.0,
            VK_RMENU.0,
        ] {
            assert!(is_bare_modifier_vk(m), "VK {m:#x} must be bare modifier");
        }
        for non_mod in [VK_LWIN.0, VK_RWIN.0, b'S' as u16, b'L' as u16, 0x25u16, 0x09u16] {
            assert!(!is_bare_modifier_vk(non_mod), "VK {non_mod:#x} must NOT be bare modifier");
        }
    }

    #[test]
    fn desktop_activity_event_filter_and_condvar_wait_or_timeout() {
        // Foreground & minimize always wake
        assert!(is_desktop_activity_event(EVENT_SYSTEM_FOREGROUND, 0, 0, false));
        assert!(is_desktop_activity_event(EVENT_SYSTEM_MINIMIZESTART, 0, 0, false));
        assert!(is_desktop_activity_event(EVENT_SYSTEM_MINIMIZEEND, 0, 0, false));

        // Object lifecycle strictly filtered to top-level OBJID_WINDOW + CHILDID_SELF
        for ev in [
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_HIDE,
        ] {
            assert!(is_desktop_activity_event(ev, OBJID_WINDOW, CHILDID_SELF, false));
            assert!(!is_desktop_activity_event(ev, OBJID_WINDOW, CHILDID_SELF, true));
            assert!(!is_desktop_activity_event(ev, -8, CHILDID_SELF, false)); // OBJID_CARET
            assert!(!is_desktop_activity_event(ev, -4, CHILDID_SELF, false)); // OBJID_CLIENT
            assert!(!is_desktop_activity_event(ev, OBJID_WINDOW, 1, false)); // non-self child
        }
        assert!(is_desktop_activity_event(
            EVENT_OBJECT_UNCLOAKED,
            OBJID_WINDOW,
            CHILDID_SELF,
            false
        ));
        assert!(!is_desktop_activity_event(EVENT_OBJECT_UNCLOAKED, -4, 0, false));
        assert!(!is_desktop_activity_event(0x800B, OBJID_WINDOW, CHILDID_SELF, false)); // LOCATIONCHANGE

        // Timeout returns false without mutating seq
        let mut seq = current_desktop_activity_seq();
        let before = seq;
        let t0 = std::time::Instant::now();
        assert!(!wait_for_desktop_activity_or_timeout(
            &mut seq,
            std::time::Duration::from_millis(25)
        ));
        assert_eq!(seq, before);
        assert!(t0.elapsed() >= std::time::Duration::from_millis(15));

        // Already-pending event wakes immediately (0ms wait)
        notify_desktop_activity();
        let t_fast = std::time::Instant::now();
        assert!(wait_for_desktop_activity_or_timeout(
            &mut seq,
            std::time::Duration::from_secs(2)
        ));
        assert!(seq > before);
        assert!(t_fast.elapsed() < std::time::Duration::from_millis(50));

        // Cross-thread wakeup wakes promptly out of a long timeout
        let prev_seq = seq;
        let t_wake = std::time::Instant::now();
        let handle = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(25));
            notify_desktop_activity();
        });
        assert!(wait_for_desktop_activity_or_timeout(
            &mut seq,
            std::time::Duration::from_secs(2)
        ));
        let _ = handle.join();
        assert!(seq > prev_seq);
        assert!(
            t_wake.elapsed() < std::time::Duration::from_millis(500),
            "Condvar wakeup took unexpectedly long: {:?}",
            t_wake.elapsed()
        );
    }

    #[test]
    fn refresh_foreground_elevated_state_caches_and_force_focus_handles_null_safely() {
        // Null / invalid HWND must be a clean no-op
        force_focus_window(0);
        force_focus_window(0x1234_5678);

        // Consecutive elevated checks return consistent value and hit HWND cache
        let first = refresh_foreground_elevated_state();
        let second = refresh_foreground_elevated_state();
        assert_eq!(first, second);
        assert_eq!(is_foreground_elevated(), second);
    }

    #[test]
    fn start_menu_health_decision_and_guard_never_calls_sw_hide() {
        // Already visible: healthy regardless of cloaked flag
        assert_eq!(
            should_restore_start_menu_visibility(true, 0),
            StartMenuVisibilityAction::AlreadyHealthy
        );
        assert_eq!(
            should_restore_start_menu_visibility(true, 2),
            StartMenuVisibilityAction::AlreadyHealthy
        );

        // Hidden + cloaked: restore WS_VISIBLE silently without dismissing/focusing
        assert_eq!(
            should_restore_start_menu_visibility(false, 1),
            StartMenuVisibilityAction::RestoreVisibleOnly
        );
        assert_eq!(
            should_restore_start_menu_visibility(false, 2),
            StartMenuVisibilityAction::RestoreVisibleOnly
        );
        assert_eq!(
            should_restore_start_menu_visibility(false, u32::MAX),
            StartMenuVisibilityAction::RestoreVisibleOnly
        );

        // Hidden + uncloaked: restore WS_VISIBLE and immediately cancel/dismiss
        assert_eq!(
            should_restore_start_menu_visibility(false, 0),
            StartMenuVisibilityAction::RestoreVisibleAndDismiss
        );

        // Strictly targets persistent UWP CoreWindow, never IME/helper windows
        assert!(is_start_menu_core_window_class("Windows.UI.Core.CoreWindow"));
        assert!(!is_start_menu_core_window_class("IME"));
        assert!(!is_start_menu_core_window_class("MSCTFIME UI"));
        assert!(!is_start_menu_core_window_class(""));

        // Verify win_event_start_guard_proc does not invoke SW_HIDE anywhere in its code
        let src = include_str!("win_shell.rs");
        let start = src
            .find("\nunsafe extern \"system\" fn win_event_start_guard_proc(")
            .expect("win_event_start_guard_proc must exist");
        let tail = &src[start..];
        let end = tail
            .find("\n/// Low-level keyboard & mouse hook thread")
            .expect("end of win_event_start_guard_proc");
        let body = &tail[..end];
        let has_sw_hide_code = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .any(|line| line.contains("SW_HIDE"));
        assert!(
            !has_sw_hide_code,
            "win_event_start_guard_proc must never call SW_HIDE on StartMenuExperienceHost.exe"
        );
        assert!(body.contains("WM_CANCELMODE"));
        assert!(body.contains("NativeStartSuppressed"));

        // Verify both internal & nonblocking restore paths call ensure_native_start_menu_healthy
        for fn_sig in [
            "\npub fn restore_native_taskbar_nonblocking()",
            "\nfn restore_native_taskbar_internal(",
        ] {
            let fn_start = src.find(fn_sig).expect("restore function must exist");
            let fn_tail = &src[fn_start..];
            let fn_end = fn_tail.find("\n}\n").expect("restore function end");
            assert!(
                fn_tail[..fn_end].contains("ensure_native_start_menu_healthy()"),
                "{fn_sig} must call ensure_native_start_menu_healthy()"
            );
        }

        // Smoke-check live execution (non-blocking no-op when Start CoreWindow is healthy/absent)
        ensure_native_start_menu_healthy();
    }
}
