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
    ABM_SETPOS, APPBARDATA, QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DispatchMessageW, EnumWindows, FindWindowW, GetClassNameW,
    GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, IsWindowVisible, PostMessageW, RegisterWindowMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW, ShowWindow,
    SystemParametersInfoW, TranslateMessage, UnhookWindowsHookEx, GWL_EXSTYLE, HHOOK,
    HWND_TOPMOST, KBDLLHOOKSTRUCT, MSG, MSLLHOOKSTRUCT, SM_CXSCREEN, SM_CYSCREEN, SPIF_SENDCHANGE,
    SPI_SETWORKAREA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW,
    SW_SHOWNOACTIVATE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HOTKEY_EMERGENCY_ID: i32 = 0x7B01;

static APPBAR_HWND: AtomicIsize = AtomicIsize::new(0);
static NATIVE_HIDDEN: AtomicBool = AtomicBool::new(false);
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
static WIN_COMBO_USED: AtomicBool = AtomicBool::new(false);
static HOOK_CALLBACK: OnceLock<Box<dyn Fn(ShellHookEvent) + Send + Sync>> = OnceLock::new();

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
        SHAppBarMessage(ABM_NEW, &mut abd);
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
pub fn is_fullscreen_active() -> bool {
    unsafe {
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_BUSY || state == QUNS_RUNNING_D3D_FULL_SCREEN {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        if fg.0.is_null() || !IsWindowVisible(fg).as_bool() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return false;
        }
        let mut cls_buf = [0u16; 64];
        let len = GetClassNameW(fg, &mut cls_buf);
        if len > 0 {
            let cls = String::from_utf16_lossy(&cls_buf[..len as usize]);
            if matches!(cls.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
                return false;
            }
        }
        let mut rect = RECT::default();
        if GetWindowRect(fg, &mut rect).is_ok() {
            let mon = primary_monitor_rect();
            let covers = rect.left <= mon.left
                && rect.top <= mon.top
                && rect.right >= mon.right
                && rect.bottom >= mon.bottom;
            return covers;
        }
        false
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
