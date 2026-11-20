use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
    ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HALFTONE, SRCCOPY,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, ShellExecuteW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, EnumWindows, GetClassNameW, GetForegroundWindow, GetIconInfo,
    GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW, SendMessageTimeoutW,
    SetForegroundWindow, ShowWindow, DI_NORMAL, GWL_EXSTYLE, GWL_STYLE, GW_OWNER, HICON,
    ICON_BIG, ICON_SMALL, ICON_SMALL2, SMTO_ABORTIFHUNG, SW_MINIMIZE, SW_RESTORE, SW_SHOW,
    WM_CLOSE, WM_GETICON, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_VISIBLE,
};

use crate::win_shell::wide;

static ICON_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn icon_cache() -> &'static Mutex<HashMap<String, String>> {
    ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWindowInfo {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    pub exe_path: String,
    pub exe_name: String,
    pub is_active: bool,
    pub is_minimized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppWindowGroup {
    pub key: String,
    pub name: String,
    pub exe_path: String,
    pub icon_data: Option<String>,
    pub is_active: bool,
    pub windows: Vec<OpenWindowInfo>,
}

pub fn process_exe_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if ok.is_ok() && size > 0 {
            Some(String::from_utf16_lossy(&buf[..size as usize]))
        } else {
            None
        }
    }
}

pub fn friendly_exe_name(exe_path: &str) -> String {
    let stem = Path::new(exe_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "App".to_string());
    match stem.to_ascii_lowercase().as_str() {
        "explorer" => "Проводник".to_string(),
        "zen" => "Zen Browser".to_string(),
        "telegram" => "Telegram".to_string(),
        "sparkdesktop" => "Spark Mail".to_string(),
        "code" => "VS Code".to_string(),
        "cursor" => "Cursor".to_string(),
        "antigravity" => "Antigravity".to_string(),
        "browser" => "Яндекс Браузер".to_string(),
        "photoshop" => "Photoshop".to_string(),
        "lightroom" => "Lightroom".to_string(),
        "obsidian" => "Obsidian".to_string(),
        "discord" => "Discord".to_string(),
        "obs64" => "OBS Studio".to_string(),
        "windowsterminal" | "wt" => "Терминал".to_string(),
        "steam" => "Steam".to_string(),
        "chrome" => "Chrome".to_string(),
        "msedge" => "Edge".to_string(),
        _ => stem,
    }
}

fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Option<String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    let b64 = base64::engine::general_purpose::STANDARD.encode(out);
    Some(format!("data:image/png;base64,{b64}"))
}

pub fn hicon_to_data_url(hicon: HICON, size: i32) -> Option<String> {
    if hicon.0.is_null() {
        return None;
    }
    unsafe {
        let mut icon_info = std::mem::zeroed();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            return None;
        }
        if !icon_info.hbmColor.0.is_null() {
            let _ = DeleteObject(icon_info.hbmColor);
        }
        if !icon_info.hbmMask.0.is_null() {
            let _ = DeleteObject(icon_info.hbmMask);
        }

        let screen_dc = GetDC(HWND::default());
        let mem_dc = CreateCompatibleDC(screen_dc);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = size;
        bmi.bmiHeader.biHeight = -size; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm = windows::Win32::Graphics::Gdi::CreateDIBSection(
            mem_dc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        )
        .ok()?;
        let old = SelectObject(mem_dc, hbm);

        let total_bytes = (size * size * 4) as usize;
        std::ptr::write_bytes(bits_ptr as *mut u8, 0, total_bytes);
        let _ = DrawIconEx(mem_dc, 0, 0, hicon, size, size, 0, None, DI_NORMAL);

        let bgra = std::slice::from_raw_parts(bits_ptr as *const u8, total_bytes);
        let mut rgba = vec![0u8; total_bytes];
        let mut any_alpha = false;
        for i in (0..total_bytes).step_by(4) {
            rgba[i] = bgra[i + 2];
            rgba[i + 1] = bgra[i + 1];
            rgba[i + 2] = bgra[i];
            rgba[i + 3] = bgra[i + 3];
            if bgra[i + 3] > 0 {
                any_alpha = true;
            }
        }
        // Fallback if icon had 0 alpha on non-black pixels
        if !any_alpha {
            for i in (0..total_bytes).step_by(4) {
                if rgba[i] > 0 || rgba[i + 1] > 0 || rgba[i + 2] > 0 {
                    rgba[i + 3] = 255;
                }
            }
        }

        SelectObject(mem_dc, old);
        let _ = DeleteObject(hbm);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(HWND::default(), screen_dc);

        encode_rgba_png(size as u32, size as u32, &rgba)
    }
}

pub fn extract_icon_for_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let key = path.to_ascii_lowercase();
    if let Ok(cache) = icon_cache().lock() {
        if let Some(hit) = cache.get(&key) {
            return Some(hit.clone());
        }
    }
    unsafe {
        let w = wide(path);
        let mut shfi = SHFILEINFOW::default();
        let res = SHGetFileInfoW(
            PCWSTR(w.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut shfi),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if res == 0 || shfi.hIcon.0.is_null() {
            return None;
        }
        let encoded = hicon_to_data_url(shfi.hIcon, 32);
        let _ = DestroyIcon(shfi.hIcon);
        if let Some(ref data) = encoded {
            if let Ok(mut cache) = icon_cache().lock() {
                cache.insert(key, data.clone());
            }
        }
        encoded
    }
}

fn extract_window_icon(hwnd: HWND, exe_path: &str) -> Option<String> {
    if let Some(icon) = extract_icon_for_path(exe_path) {
        return Some(icon);
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
                if let Some(url) = hicon_to_data_url(HICON(result as *mut _), 32) {
                    return Some(url);
                }
            }
        }
    }
    None
}

pub fn list_open_window_groups() -> Vec<AppWindowGroup> {
    struct Ctx {
        own_pid: u32,
        fg_hwnd: HWND,
        items: Vec<OpenWindowInfo>,
    }

    extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let ctx = &mut *(lparam.0 as *mut Ctx);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            if (style & WS_VISIBLE.0) == 0 {
                return BOOL(1);
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if (ex & WS_EX_TOOLWINDOW.0) != 0 {
                return BOOL(1);
            }
            let owner = GetWindow(hwnd, GW_OWNER).unwrap_or_default();
            if !owner.0.is_null() && (ex & WS_EX_APPWINDOW.0) == 0 {
                return BOOL(1);
            }

            // Skip DWM-cloaked UWP/virtual-desktop hidden windows
            let mut cloaked: u32 = 0;
            if DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut _,
                std::mem::size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0
            {
                return BOOL(1);
            }

            let title_len = GetWindowTextLengthW(hwnd);
            if title_len <= 0 {
                return BOOL(1);
            }
            let mut title_buf = vec![0u16; (title_len + 1) as usize];
            let got = GetWindowTextW(hwnd, &mut title_buf);
            if got <= 0 {
                return BOOL(1);
            }
            let title = String::from_utf16_lossy(&title_buf[..got as usize]);
            if title == "Program Manager" || title == "Windows Input Experience" {
                return BOOL(1);
            }

            let mut cls_buf = [0u16; 64];
            let cls_len = GetClassNameW(hwnd, &mut cls_buf);
            if cls_len > 0 {
                let cls = String::from_utf16_lossy(&cls_buf[..cls_len as usize]);
                if matches!(
                    cls.as_str(),
                    "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow"
                ) {
                    return BOOL(1);
                }
            }

            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == ctx.own_pid {
                return BOOL(1);
            }

            let exe_path = process_exe_path(pid).unwrap_or_default();
            let is_uwp_host = exe_path.to_ascii_lowercase().ends_with("applicationframehost.exe");
            let exe_name = if exe_path.is_empty() || is_uwp_host {
                title.clone()
            } else {
                friendly_exe_name(&exe_path)
            };

            ctx.items.push(OpenWindowInfo {
                hwnd: hwnd.0 as isize,
                pid,
                title,
                exe_path,
                exe_name,
                is_active: hwnd == ctx.fg_hwnd,
                is_minimized: IsIconic(hwnd).as_bool(),
            });
        }
        BOOL(1)
    }

    let mut ctx = Ctx {
        own_pid: unsafe { GetCurrentProcessId() },
        fg_hwnd: unsafe { GetForegroundWindow() },
        items: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }

    let mut groups: Vec<AppWindowGroup> = Vec::new();
    let mut index_map: HashMap<String, usize> = HashMap::new();

    for win in ctx.items {
        let is_uwp_host = win.exe_path.to_ascii_lowercase().ends_with("applicationframehost.exe");
        let key = if win.exe_path.is_empty() {
            format!("pid:{}", win.pid)
        } else if is_uwp_host {
            format!("uwp:{}", win.title.to_ascii_lowercase())
        } else {
            win.exe_path.to_ascii_lowercase()
        };
        if let Some(&idx) = index_map.get(&key) {
            if win.is_active {
                groups[idx].is_active = true;
            }
            groups[idx].windows.push(win);
        } else {
            let icon_data = extract_window_icon(HWND(win.hwnd as *mut _), &win.exe_path);
            let idx = groups.len();
            index_map.insert(key.clone(), idx);
            groups.push(AppWindowGroup {
                key,
                name: win.exe_name.clone(),
                exe_path: win.exe_path.clone(),
                icon_data,
                is_active: win.is_active,
                windows: vec![win],
            });
        }
    }

    groups
}

pub fn focus_or_toggle_window(hwnd_raw: isize, toggle_minimize: bool) {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let fg = GetForegroundWindow();
        if toggle_minimize && fg == hwnd && !IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            return;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let fg_tid = GetWindowThreadProcessId(fg, None);
        let cur_tid = GetCurrentThreadId();
        let target_tid = GetWindowThreadProcessId(hwnd, None);
        if fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, true);
        }
        if target_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, target_tid, true);
        }
        let _ = SetForegroundWindow(hwnd);
        if target_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, target_tid, false);
        }
        if fg_tid != cur_tid {
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        }
    }
}

pub fn close_window(hwnd_raw: isize) {
    unsafe {
        let _ = PostMessageW(HWND(hwnd_raw as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

pub fn terminate_process_by_pid(pid: u32) -> Result<(), String> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format!("Не удалось открыть процесс {pid}: {e}"))?;
        let res = TerminateProcess(handle, 1);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        res.map_err(|e| format!("Не удалось завершить процесс {pid}: {e}"))
    }
}

pub fn launch_target(target: &str, as_admin: bool) -> Result<(), String> {
    if target.trim().is_empty() {
        return Err("Пустой путь запуска".to_string());
    }
    unsafe {
        let verb = wide(if as_admin { "runas" } else { "open" });
        let file = wide(target);
        let res = ShellExecuteW(
            HWND::default(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOW,
        );
        if (res.0 as isize) <= 32 {
            return Err(format!("Не удалось открыть: {target}"));
        }
    }
    Ok(())
}

pub fn open_file_location(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("Пустой путь".to_string());
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,\"{path}\""))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn open_in_terminal(dir_path: &str) -> Result<(), String> {
    let target = if Path::new(dir_path).is_dir() {
        dir_path.to_string()
    } else {
        Path::new(dir_path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| r"C:\".to_string())
    };
    if std::process::Command::new("wt.exe")
        .args(["-d", &target])
        .spawn()
        .is_ok()
    {
        return Ok(());
    }
    std::process::Command::new("powershell.exe")
        .args(["-NoExit", "-Command", &format!("Set-Location -LiteralPath '{target}'")])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Captures a live visual thumbnail of an open window (using PrintWindow PW_RENDERFULLCONTENT)
/// downscaled cleanly for hover preview cards.
pub fn capture_window_thumbnail(hwnd_raw: isize, max_w: i32, max_h: i32) -> Option<String> {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if IsIconic(hwnd).as_bool() {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        let src_w = (rect.right - rect.left).max(1);
        let src_h = (rect.bottom - rect.top).max(1);
        if src_w < 32 || src_h < 32 {
            return None;
        }

        let scale = (max_w as f32 / src_w as f32).min(max_h as f32 / src_h as f32).min(1.0);
        let dst_w = ((src_w as f32 * scale).round() as i32).max(16);
        let dst_h = ((src_h as f32 * scale).round() as i32).max(16);

        let screen_dc = GetDC(HWND::default());
        let src_dc = CreateCompatibleDC(screen_dc);
        let src_bmp = CreateCompatibleBitmap(screen_dc, src_w, src_h);
        let old_src = SelectObject(src_dc, src_bmp);

        // PW_RENDERFULLCONTENT = 2
        let printed = windows::Win32::Storage::Xps::PrintWindow(hwnd, src_dc, windows::Win32::Storage::Xps::PRINT_WINDOW_FLAGS(2)).as_bool();
        if !printed {
            let win_dc = GetDC(hwnd);
            let _ = BitBlt(src_dc, 0, 0, src_w, src_h, win_dc, 0, 0, SRCCOPY);
            ReleaseDC(hwnd, win_dc);
        }

        let dst_dc = CreateCompatibleDC(screen_dc);
        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = dst_w;
        bmi.bmiHeader.biHeight = -dst_h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let dst_bmp = windows::Win32::Graphics::Gdi::CreateDIBSection(
            dst_dc,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        )
        .ok();

        let mut encoded = None;
        if let Some(dst_bmp) = dst_bmp {
            let old_dst = SelectObject(dst_dc, dst_bmp);
            SetStretchBltMode(dst_dc, HALFTONE);
            let _ = StretchBlt(dst_dc, 0, 0, dst_w, dst_h, src_dc, 0, 0, src_w, src_h, SRCCOPY);
            let total = (dst_w * dst_h * 4) as usize;
            let bgra = std::slice::from_raw_parts(bits_ptr as *const u8, total);
            let mut rgba = vec![0u8; total];
            for i in (0..total).step_by(4) {
                rgba[i] = bgra[i + 2];
                rgba[i + 1] = bgra[i + 1];
                rgba[i + 2] = bgra[i];
                rgba[i + 3] = 255;
            }
            encoded = encode_rgba_png(dst_w as u32, dst_h as u32, &rgba);
            SelectObject(dst_dc, old_dst);
            let _ = DeleteObject(dst_bmp);
        }

        SelectObject(src_dc, old_src);
        let _ = DeleteObject(src_bmp);
        let _ = DeleteDC(src_dc);
        let _ = DeleteDC(dst_dc);
        ReleaseDC(HWND::default(), screen_dc);

        encoded
    }
}
