use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, SendInput, UnregisterHotKey, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VIRTUAL_KEY, VK_ESCAPE, VK_LWIN, VK_RWIN,
};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, SHQueryUserNotificationState, ABE_BOTTOM, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
    ABM_SETPOS, APPBARDATA, QUNS_ACCEPTS_NOTIFICATIONS, QUNS_APP, QUNS_BUSY, QUNS_NOT_PRESENT,
    QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CallWindowProcW, CreateWindowExW, DefWindowProcW, DispatchMessageW,
    EnumWindows, FindWindowW, GetClassNameW, GetForegroundWindow, GetMessageW, GetSystemMetrics,
    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    PostMessageW,
    RegisterWindowMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    SetWindowsHookExW, ShowWindow, SystemParametersInfoW, TranslateMessage, UnhookWindowsHookEx,
    GWLP_WNDPROC, GWL_EXSTYLE, GWL_STYLE, HHOOK, HWND_TOPMOST, KBDLLHOOKSTRUCT, MA_NOACTIVATE,
    MSG, MSLLHOOKSTRUCT, SM_CXSCREEN, SM_CYSCREEN, SPIF_SENDCHANGE, SPI_SETWORKAREA,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, SW_SHOWNOACTIVATE,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDPROC,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

use crate::coordinator::{classify_fullscreen, ForegroundWindowInfo, NotificationState, PixelRect};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HOTKEY_EMERGENCY_ID: i32 = 0x7B01;

static APPBAR_HWND: AtomicIsize = AtomicIsize::new(0);
static NATIVE_HIDDEN: AtomicBool = AtomicBool::new(false);
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static WIN_COMBO_USED: AtomicBool = AtomicBool::new(false);
static HOOK_CALLBACK: OnceLock<Box<dyn Fn(ShellHookEvent) + Send + Sync>> = OnceLock::new();
static FLYOUT_SUBCLASSED_HWND: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_ORIG_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static FLYOUT_NON_ACTIVATING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
pub enum ShellHookEvent {
    ToggleStart,
    EmergencyRestore,
    GlobalMouseDown(i32, i32),
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

/// Guaranteed restoration of Windows 11 native taskbar(s) + AppBar removal + workarea refresh.
pub fn restore_native_taskbar() {
    unregister_appbar();
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

        // Reset work area gracefully so maximized windows re-adjust immediately
        let mon = primary_monitor_rect();
        let mut wa = RECT {
            left: mon.left,
            top: mon.top,
            right: mon.right,
            bottom: mon.bottom - 48,
        };
        let _ = SystemParametersInfoW(
            SPI_SETWORKAREA,
            0,
            Some(&mut wa as *mut _ as *mut _),
            SPIF_SENDCHANGE,
        );
        NATIVE_HIDDEN.store(false, Ordering::SeqCst);
    }
}

pub fn register_appbar(hwnd_raw: isize, reserve_height_px: i32) {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let mon = primary_monitor_rect();
        let prev = APPBAR_HWND.swap(hwnd_raw, Ordering::SeqCst);
        if prev != 0 && prev != hwnd_raw {
            let mut old = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: HWND(prev as *mut _),
                ..Default::default()
            };
            SHAppBarMessage(ABM_REMOVE, &mut old);
        }

        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: WM_APP + 10,
            uEdge: ABE_BOTTOM,
            rc: RECT {
                left: mon.left,
                top: mon.bottom - reserve_height_px,
                right: mon.right,
                bottom: mon.bottom,
            },
            ..Default::default()
        };
        if prev != hwnd_raw {
            SHAppBarMessage(ABM_NEW, &mut abd);
        }
        SHAppBarMessage(ABM_QUERYPOS, &mut abd);
        abd.rc.top = abd.rc.bottom - reserve_height_px;
        SHAppBarMessage(ABM_SETPOS, &mut abd);
    }
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
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
    }
}

/// Spawns lightweight companion watchdog: `testbar.exe --watchdog <pid>`.
/// If main process is killed via Task Manager or crashes, watchdog restores native taskbar in < 5ms.
pub fn spawn_watchdog() {
    if let Ok(exe) = std::env::current_exe() {
        let pid = unsafe { GetCurrentProcessId() };
        let _ = std::process::Command::new(exe)
            .arg("--watchdog")
            .arg(pid.to_string())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

pub fn run_watchdog(parent_pid: u32) {
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, parent_pid) {
            let _ = WaitForSingleObject(handle, INFINITE);
        }
        restore_native_taskbar();
    }
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
        let _ = RegisterWindowMessageW(PCWSTR(wide("TaskbarCreated").as_ptr()));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
            if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == HOTKEY_EMERGENCY_ID {
                restore_native_taskbar();
                if let Some(cb) = HOOK_CALLBACK.get() {
                    cb(ShellHookEvent::EmergencyRestore);
                }
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if let Ok(h) = hook {
            let _ = UnhookWindowsHookEx(h);
        }
        if let Ok(mh) = mouse_hook {
            let _ = UnhookWindowsHookEx(mh);
        }
        let _ = UnregisterHotKey(HWND::default(), HOTKEY_EMERGENCY_ID);
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
    use std::sync::Mutex;
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyWindow, SendMessageW, MA_ACTIVATE, WS_POPUP,
    };

    static SUBCLASS_TEST_LOCK: Mutex<()> = Mutex::new(());

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
}
