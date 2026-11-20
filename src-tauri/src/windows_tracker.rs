use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, BOOL, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, HWND, LPARAM, RECT,
    RPC_E_CHANGED_MODE, SIZE, S_FALSE, S_OK, WPARAM,
};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
    GetDC, GetDIBits, GetObjectW, ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HALFTONE, HBITMAP, SRCCOPY,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHGetFileInfoW, ShellExecuteW,
    SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, EnumChildWindows, EnumWindows, GetClassLongPtrW, GetClassNameW,
    GetForegroundWindow, GetIconInfo, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    PostMessageW, SendMessageTimeoutW, SetForegroundWindow, ShowWindow, DI_NORMAL, GCLP_HICON,
    GCLP_HICONSM, GWL_EXSTYLE, GWL_STYLE, GW_OWNER, HICON, ICON_BIG, ICON_SMALL, ICON_SMALL2,
    SMTO_ABORTIFHUNG, SW_MINIMIZE, SW_RESTORE, SW_SHOW, WM_CLOSE, WM_GETICON, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW, WS_VISIBLE,
};

use crate::icon_cache::{self, IconCache};
use crate::win_shell::wide;

static FALLBACK_ICON_CACHE: OnceLock<Mutex<IconCache>> = OnceLock::new();

fn fallback_icon_cache() -> &'static Mutex<IconCache> {
    FALLBACK_ICON_CACHE.get_or_init(|| Mutex::new(IconCache::default()))
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Decides whether `CoUninitialize` must be called for a `CoInitializeEx` result (AD-5).
/// Only `S_OK` and `S_FALSE` incremented the thread's COM init count; `RPC_E_CHANGED_MODE`
/// and other failure HRESULTs MUST NEVER call `CoUninitialize`.
pub fn should_uninitialize_com(hr: HRESULT) -> bool {
    hr == S_OK || hr == S_FALSE
}

/// RAII COM apartment guard honoring `CoInitializeEx` HRESULT semantics (AD-5).
pub struct ComGuard {
    should_uninit: bool,
    usable: bool,
}

impl ComGuard {
    pub fn init_sta() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self::from_hresult(hr)
    }

    pub(crate) fn from_hresult(hr: HRESULT) -> Self {
        let should_uninit = should_uninitialize_com(hr);
        let usable = should_uninit || hr == RPC_E_CHANGED_MODE;
        Self {
            should_uninit,
            usable,
        }
    }

    pub fn is_usable(&self) -> bool {
        self.usable
    }

    pub fn will_uninitialize(&self) -> bool {
        self.should_uninit
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

struct HBitmapGuard(HBITMAP);

impl Drop for HBitmapGuard {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            unsafe {
                let _ = DeleteObject(self.0);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenWindowInfo {
    pub hwnd: isize,
    pub pid: u32,
    pub title: String,
    pub exe_path: String,
    pub exe_name: String,
    pub aumid: Option<String>,
    pub is_active: bool,
    pub is_minimized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppWindowGroup {
    pub key: String,
    pub name: String,
    pub exe_path: String,
    pub aumid: Option<String>,
    pub icon_key: Option<String>,
    pub icon_data: Option<String>,
    pub is_active: bool,
    pub windows: Vec<OpenWindowInfo>,
}

pub fn is_application_frame_host(exe_path: &str) -> bool {
    let trimmed = exe_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let file_name = trimmed
        .rsplit(|c| c == '\\' || c == '/')
        .next()
        .unwrap_or(trimmed);
    file_name.eq_ignore_ascii_case("applicationframehost.exe")
}

pub fn process_exe_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        if ok.is_ok() && size > 0 {
            Some(String::from_utf16_lossy(&buf[..size as usize]))
        } else {
            None
        }
    }
}

/// Queries the packaged Application User Model ID (AUMID) for a process, if it is a
/// packaged UWP / MSIX / WinUI3 app. Returns `None` for classic unpackaged Win32 processes.
pub fn process_aumid(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut len: u32 = 256;
        let mut buf = vec![0u16; len as usize];
        let mut err = GetApplicationUserModelId(handle, &mut len, PWSTR(buf.as_mut_ptr()));
        if err == ERROR_INSUFFICIENT_BUFFER && len > 0 {
            buf.resize(len as usize, 0);
            err = GetApplicationUserModelId(handle, &mut len, PWSTR(buf.as_mut_ptr()));
        }
        let _ = CloseHandle(handle);
        if err == ERROR_SUCCESS && len > 1 {
            let actual = (len as usize).min(buf.len());
            let end = buf[..actual]
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(actual);
            let aumid = String::from_utf16_lossy(&buf[..end]).trim().to_string();
            if !aumid.is_empty() {
                return Some(aumid);
            }
        }
        None
    }
}

/// Enumerates child windows of an `ApplicationFrameHost.exe` top-level window to locate
/// the hosted UWP child process (preferring `Windows.UI.Core.CoreWindow`, then any child
/// whose PID differs from `host_pid`).
fn resolve_uwp_child_pid(host_hwnd: HWND, host_pid: u32) -> Option<u32> {
    struct ChildCtx {
        host_pid: u32,
        core_window_pid: Option<u32>,
        fallback_pid: Option<u32>,
    }

    extern "system" fn child_proc(child: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let ctx = &mut *(lparam.0 as *mut ChildCtx);
            let mut pid = 0u32;
            GetWindowThreadProcessId(child, Some(&mut pid));
            if pid == 0 || pid == ctx.host_pid {
                return BOOL(1);
            }
            let mut cls_buf = [0u16; 64];
            let len = GetClassNameW(child, &mut cls_buf);
            if len > 0 {
                let cls = String::from_utf16_lossy(&cls_buf[..len as usize]);
                if cls == "Windows.UI.Core.CoreWindow" {
                    ctx.core_window_pid = Some(pid);
                    return BOOL(0);
                }
            }
            if ctx.fallback_pid.is_none() {
                ctx.fallback_pid = Some(pid);
            }
            BOOL(1)
        }
    }

    let mut ctx = ChildCtx {
        host_pid,
        core_window_pid: None,
        fallback_pid: None,
    };
    unsafe {
        let _ = EnumChildWindows(host_hwnd, Some(child_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.core_window_pid.or(ctx.fallback_pid)
}

fn known_friendly_exe_name(stem_lower: &str) -> Option<&'static str> {
    match stem_lower {
        "explorer" => Some("Проводник"),
        "zen" => Some("Zen Browser"),
        "telegram" => Some("Telegram"),
        "sparkdesktop" => Some("Spark Mail"),
        "code" => Some("VS Code"),
        "cursor" => Some("Cursor"),
        "antigravity" => Some("Antigravity"),
        "browser" => Some("Яндекс Браузер"),
        "photoshop" => Some("Photoshop"),
        "lightroom" => Some("Lightroom"),
        "obsidian" => Some("Obsidian"),
        "discord" => Some("Discord"),
        "obs64" => Some("OBS Studio"),
        "windowsterminal" | "wt" => Some("Терминал"),
        "steam" => Some("Steam"),
        "chrome" => Some("Chrome"),
        "msedge" => Some("Edge"),
        "systemsettings" => Some("Параметры"),
        "calculatorapp" => Some("Калькулятор"),
        "winstore.app" => Some("Microsoft Store"),
        "photos" | "photosapp" => Some("Фотографии"),
        "mspaint" => Some("Paint"),
        "notepad" => Some("Блокнот"),
        "snippingtool" | "screenclippinghost" => Some("Ножницы"),
        _ => None,
    }
}

pub fn friendly_exe_name(exe_path: &str) -> String {
    let stem = Path::new(exe_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "App".to_string());
    let low = stem.to_ascii_lowercase();
    if let Some(known) = known_friendly_exe_name(&low) {
        known.to_string()
    } else {
        stem
    }
}

fn display_name_for_window(exe_path: &str, is_uwp_host: bool, title: &str) -> String {
    if exe_path.is_empty() || is_application_frame_host(exe_path) {
        return title.to_string();
    }
    let stem = Path::new(exe_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let low = stem.to_ascii_lowercase();
    if let Some(known) = known_friendly_exe_name(&low) {
        return known.to_string();
    }
    if is_uwp_host && !title.trim().is_empty() {
        return title.to_string();
    }
    if stem.is_empty() {
        title.to_string()
    } else {
        stem
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

/// Converts top-down 32-bit BGRA pixels (straight, premultiplied, or 0-alpha opaque)
/// into straight RGBA pixels suitable for PNG encoding.
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let len = bgra.len() - (bgra.len() % 4);
    let mut any_alpha = false;
    let mut has_translucent = false;
    let mut looks_premultiplied = true;

    for chunk in bgra[..len].chunks_exact(4) {
        let b = chunk[0];
        let g = chunk[1];
        let r = chunk[2];
        let a = chunk[3];
        if a > 0 {
            any_alpha = true;
        }
        if a > 0 && a < 255 {
            has_translucent = true;
        }
        if b > a || g > a || r > a {
            looks_premultiplied = false;
        }
    }

    let unpremultiply = any_alpha && has_translucent && looks_premultiplied;
    let mut rgba = vec![0u8; len];

    for (src, dst) in bgra[..len].chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
        let (mut r, mut g, mut b, mut a) = (src[2], src[1], src[0], src[3]);
        if !any_alpha {
            if r > 0 || g > 0 || b > 0 {
                a = 255;
            }
        } else if unpremultiply && a > 0 && a < 255 {
            let a32 = u32::from(a);
            let half = a32 / 2;
            r = ((u32::from(r) * 255 + half) / a32).min(255) as u8;
            g = ((u32::from(g) * 255 + half) / a32).min(255) as u8;
            b = ((u32::from(b) * 255 + half) / a32).min(255) as u8;
        }
        dst[0] = r;
        dst[1] = g;
        dst[2] = b;
        dst[3] = a;
    }

    rgba
}

pub fn hicon_to_data_url(hicon: HICON, size: i32) -> Option<String> {
    if hicon.0.is_null() || size <= 0 {
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
        let hbm = match CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0) {
            Ok(h) => h,
            Err(_) => {
                let _ = DeleteDC(mem_dc);
                ReleaseDC(HWND::default(), screen_dc);
                return None;
            }
        };
        let old = SelectObject(mem_dc, hbm);

        let total_bytes = (size * size * 4) as usize;
        std::ptr::write_bytes(bits_ptr as *mut u8, 0, total_bytes);
        let _ = DrawIconEx(mem_dc, 0, 0, hicon, size, size, 0, None, DI_NORMAL);

        let bgra = std::slice::from_raw_parts(bits_ptr as *const u8, total_bytes);
        let rgba = bgra_to_rgba(bgra);

        SelectObject(mem_dc, old);
        let _ = DeleteObject(hbm);
        let _ = DeleteDC(mem_dc);
        ReleaseDC(HWND::default(), screen_dc);

        encode_rgba_png(size as u32, size as u32, &rgba)
    }
}

fn hbitmap_to_data_url(hbm: HBITMAP) -> Option<String> {
    if hbm.0.is_null() {
        return None;
    }
    let _guard = HBitmapGuard(hbm);
    unsafe {
        let mut bm = BITMAP::default();
        let got = GetObjectW(
            hbm,
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut BITMAP as *mut std::ffi::c_void),
        );
        if got == 0 {
            return None;
        }
        let w = bm.bmWidth;
        let h = bm.bmHeight.abs();
        if w <= 0 || h <= 0 || w > 512 || h > 512 {
            return None;
        }

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let screen_dc = GetDC(HWND::default());
        let scanlines = GetDIBits(
            screen_dc,
            hbm,
            0,
            h as u32,
            Some(bgra.as_mut_ptr() as *mut std::ffi::c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(HWND::default(), screen_dc);
        if scanlines == 0 {
            return None;
        }

        let rgba = bgra_to_rgba(&bgra);
        encode_rgba_png(w as u32, h as u32, &rgba)
    }
}

/// Extracts a crisp 64x64 PNG icon for a packaged UWP/MSIX app via
/// `SHCreateItemFromParsingName("shell:AppsFolder\\<AUMID>")` -> `IShellItemImageFactory::GetImage`
/// protected by an RAII `ComGuard` (AD-5, AD-7).
pub fn extract_uwp_icon_by_aumid(aumid: &str) -> Option<String> {
    let trimmed = aumid.trim();
    if trimmed.is_empty() {
        return None;
    }
    let com = ComGuard::init_sta();
    if !com.is_usable() {
        return None;
    }
    let parsing_name = if trimmed
        .to_ascii_lowercase()
        .starts_with("shell:appsfolder\\")
    {
        trimmed.to_string()
    } else {
        format!("shell:AppsFolder\\{trimmed}")
    };
    let w = wide(&parsing_name);
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
        let size = SIZE { cx: 64, cy: 64 };
        let hbm = factory
            .GetImage(size, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
            .or_else(|_| factory.GetImage(size, SIIGBF_BIGGERSIZEOK))
            .ok()?;
        hbitmap_to_data_url(hbm)
    }
}

fn extract_path_icon_raw(path: &str) -> Option<String> {
    if path.trim().is_empty() {
        return None;
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
        let encoded = hicon_to_data_url(shfi.hIcon, 48);
        let _ = DestroyIcon(shfi.hIcon);
        encoded
    }
}

fn extract_live_window_hicon(hwnd: HWND) -> Option<String> {
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
                if let Some(url) = hicon_to_data_url(HICON(result as *mut _), 48) {
                    return Some(url);
                }
            }
        }
        for idx in [GCLP_HICON, GCLP_HICONSM] {
            let raw = GetClassLongPtrW(hwnd, idx);
            if raw != 0 {
                if let Some(url) = hicon_to_data_url(HICON(raw as *mut _), 48) {
                    return Some(url);
                }
            }
        }
    }
    None
}

pub fn extract_icon_for_path_cached(path: &str, cache: &Mutex<IconCache>) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed
        .to_ascii_lowercase()
        .starts_with("shell:appsfolder\\")
    {
        let key = icon_cache::aumid_key(trimmed);
        if let Some(hit) = lock_unpoisoned(cache).get(&key) {
            return Some(hit);
        }
        let extracted = extract_uwp_icon_by_aumid(trimmed)?;
        lock_unpoisoned(cache).put(key, extracted.clone());
        return Some(extracted);
    }

    let key = icon_cache::path_icon_key(trimmed);
    if let Some(hit) = lock_unpoisoned(cache).get(&key) {
        return Some(hit);
    }
    let extracted = extract_path_icon_raw(trimmed)?;
    lock_unpoisoned(cache).put(key, extracted.clone());
    Some(extracted)
}

pub fn extract_icon_for_path(path: &str) -> Option<String> {
    extract_icon_for_path_cached(path, fallback_icon_cache())
}

fn resolve_window_icon_key(win: &OpenWindowInfo, cache: &Mutex<IconCache>) -> Option<String> {
    let hwnd = HWND(win.hwnd as *mut _);

    if let Some(ref aumid) = win.aumid {
        let key = icon_cache::aumid_key(aumid);
        if lock_unpoisoned(cache).get(&key).is_some() {
            return Some(key);
        }
        let extracted = extract_uwp_icon_by_aumid(aumid)
            .or_else(|| {
                if !win.exe_path.is_empty() && !is_application_frame_host(&win.exe_path) {
                    extract_path_icon_raw(&win.exe_path)
                } else {
                    None
                }
            })
            .or_else(|| extract_live_window_hicon(hwnd));
        if let Some(uri) = extracted {
            lock_unpoisoned(cache).put(key.clone(), uri);
            return Some(key);
        }
    }

    if !win.exe_path.is_empty() && !is_application_frame_host(&win.exe_path) {
        let key = icon_cache::path_icon_key(&win.exe_path);
        if lock_unpoisoned(cache).get(&key).is_some() {
            return Some(key);
        }
        let extracted =
            extract_path_icon_raw(&win.exe_path).or_else(|| extract_live_window_hicon(hwnd));
        if let Some(uri) = extracted {
            lock_unpoisoned(cache).put(key.clone(), uri);
            return Some(key);
        }
    }

    let key = icon_cache::hwnd_key(win.hwnd, 0);
    if lock_unpoisoned(cache).get(&key).is_some() {
        return Some(key);
    }
    let extracted = extract_live_window_hicon(hwnd)?;
    lock_unpoisoned(cache).put(key.clone(), extracted);
    Some(key)
}

/// Computes the canonical grouping key for an open window so packaged UWP apps hosted
/// under `ApplicationFrameHost.exe` group by their real `aumid` (or child exe) instead of
/// collapsing together under `applicationframehost.exe`.
pub fn compute_window_group_key(
    exe_path: &str,
    aumid: Option<&str>,
    pid: u32,
    title: &str,
) -> String {
    if let Some(a) = aumid.map(str::trim).filter(|s| !s.is_empty()) {
        return format!("aumid:{}", a.to_ascii_lowercase());
    }
    let norm_exe = exe_path.trim().replace('/', "\\").to_lowercase();
    if norm_exe.is_empty() {
        format!("pid:{pid}")
    } else if is_application_frame_host(&norm_exe) {
        format!("uwp:{}", title.trim().to_ascii_lowercase())
    } else {
        norm_exe
    }
}

/// Pure grouping routine combining enumerated top-level windows into `AppWindowGroup`s
/// with namespaced `icon_key` references and `icon_data: None` (AD-6).
pub fn group_open_windows<FResolveIcon>(
    items: Vec<OpenWindowInfo>,
    mut resolve_icon_key: FResolveIcon,
) -> Vec<AppWindowGroup>
where
    FResolveIcon: FnMut(&OpenWindowInfo) -> Option<String>,
{
    let mut groups: Vec<AppWindowGroup> = Vec::new();
    let mut index_map: HashMap<String, usize> = HashMap::new();

    for win in items {
        let key = compute_window_group_key(
            &win.exe_path,
            win.aumid.as_deref(),
            win.pid,
            &win.title,
        );
        if let Some(&idx) = index_map.get(&key) {
            if win.is_active {
                groups[idx].is_active = true;
            }
            if groups[idx].aumid.is_none() && win.aumid.is_some() {
                groups[idx].aumid = win.aumid.clone();
            }
            if groups[idx].icon_key.is_none() {
                groups[idx].icon_key = resolve_icon_key(&win);
            }
            groups[idx].windows.push(win);
        } else {
            let icon_key = resolve_icon_key(&win);
            let idx = groups.len();
            index_map.insert(key.clone(), idx);
            groups.push(AppWindowGroup {
                key,
                name: win.exe_name.clone(),
                exe_path: win.exe_path.clone(),
                aumid: win.aumid.clone(),
                icon_key,
                icon_data: None,
                is_active: win.is_active,
                windows: vec![win],
            });
        }
    }

    groups
}

pub fn list_open_window_groups_cached(cache: &Mutex<IconCache>) -> Vec<AppWindowGroup> {
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
                    "Progman"
                        | "WorkerW"
                        | "Shell_TrayWnd"
                        | "Shell_SecondaryTrayWnd"
                        | "Windows.UI.Core.CoreWindow"
                ) {
                    return BOOL(1);
                }
            }

            let mut host_pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut host_pid));
            if host_pid == 0 || host_pid == ctx.own_pid {
                return BOOL(1);
            }

            let host_exe = process_exe_path(host_pid).unwrap_or_default();
            let is_uwp_host = is_application_frame_host(&host_exe);

            let (effective_pid, exe_path, aumid) = if is_uwp_host {
                if let Some(child_pid) = resolve_uwp_child_pid(hwnd, host_pid) {
                    let child_exe = process_exe_path(child_pid).unwrap_or_else(|| host_exe.clone());
                    let child_aumid = process_aumid(child_pid);
                    (child_pid, child_exe, child_aumid)
                } else {
                    (host_pid, host_exe, None)
                }
            } else {
                let pkg_aumid = process_aumid(host_pid);
                (host_pid, host_exe, pkg_aumid)
            };

            let exe_name = display_name_for_window(&exe_path, is_uwp_host, &title);

            ctx.items.push(OpenWindowInfo {
                hwnd: hwnd.0 as isize,
                pid: effective_pid,
                title,
                exe_path,
                exe_name,
                aumid,
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

    group_open_windows(ctx.items, |win| resolve_window_icon_key(win, cache))
}

pub fn list_open_window_groups() -> Vec<AppWindowGroup> {
    list_open_window_groups_cached(fallback_icon_cache())
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
        let _ = CloseHandle(handle);
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
        .args([
            "-NoExit",
            "-Command",
            &format!("Set-Location -LiteralPath '{target}'"),
        ])
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

        let scale = (max_w as f32 / src_w as f32)
            .min(max_h as f32 / src_h as f32)
            .min(1.0);
        let dst_w = ((src_w as f32 * scale).round() as i32).max(16);
        let dst_h = ((src_h as f32 * scale).round() as i32).max(16);

        let screen_dc = GetDC(HWND::default());
        let src_dc = CreateCompatibleDC(screen_dc);
        let src_bmp = CreateCompatibleBitmap(screen_dc, src_w, src_h);
        let old_src = SelectObject(src_dc, src_bmp);

        // PW_RENDERFULLCONTENT = 2
        let printed = windows::Win32::Storage::Xps::PrintWindow(
            hwnd,
            src_dc,
            windows::Win32::Storage::Xps::PRINT_WINDOW_FLAGS(2),
        )
        .as_bool();
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
        let dst_bmp = CreateDIBSection(dst_dc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0).ok();

        let mut encoded = None;
        if let Some(dst_bmp) = dst_bmp {
            let old_dst = SelectObject(dst_dc, dst_bmp);
            SetStretchBltMode(dst_dc, HALFTONE);
            let _ = StretchBlt(
                dst_dc, 0, 0, dst_w, dst_h, src_dc, 0, 0, src_w, src_h, SRCCOPY,
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::E_FAIL;
    use windows::Win32::System::Com::COINIT_MULTITHREADED;

    #[test]
    fn com_guard_hresult_matrix_and_rpc_e_changed_mode_safety() {
        assert!(should_uninitialize_com(S_OK));
        assert!(should_uninitialize_com(S_FALSE));
        assert!(!should_uninitialize_com(RPC_E_CHANGED_MODE));
        assert!(!should_uninitialize_com(E_FAIL));

        // Pure from_hresult behavior (without touching COM thread state on the test runner thread)
        let g_changed = ComGuard::from_hresult(RPC_E_CHANGED_MODE);
        assert!(g_changed.is_usable());
        assert!(!g_changed.will_uninitialize());
        drop(g_changed); // must NOT call CoUninitialize

        let g_fail = ComGuard::from_hresult(E_FAIL);
        assert!(!g_fail.is_usable());
        assert!(!g_fail.will_uninitialize());
        drop(g_fail);

        // Real OS thread test: MTA thread calling ComGuard::init_sta() gets RPC_E_CHANGED_MODE,
        // remains usable, and dropping ComGuard MUST NOT uninitialize the thread's MTA apartment.
        std::thread::spawn(|| unsafe {
            let hr_mta = CoInitializeEx(None, COINIT_MULTITHREADED);
            assert_eq!(hr_mta, S_OK, "fresh worker thread must initialize MTA with S_OK");

            {
                let sta_guard = ComGuard::init_sta();
                assert!(
                    sta_guard.is_usable(),
                    "COM must remain usable when apartment is already MTA"
                );
                assert!(
                    !sta_guard.will_uninitialize(),
                    "ComGuard MUST NOT call CoUninitialize on RPC_E_CHANGED_MODE"
                );
            }

            // If ComGuard erroneously called CoUninitialize(), this would return S_OK instead of S_FALSE.
            let hr_check = CoInitializeEx(None, COINIT_MULTITHREADED);
            assert_eq!(
                hr_check, S_FALSE,
                "existing MTA apartment must remain initialized after ComGuard drop"
            );
            CoUninitialize();
            CoUninitialize();
        })
        .join()
        .expect("MTA ComGuard thread test panicked");

        // Fresh uninitialized thread: ComGuard::init_sta() succeeds with S_OK and cleanly uninitializes on Drop.
        std::thread::spawn(|| unsafe {
            {
                let sta_guard = ComGuard::init_sta();
                assert!(sta_guard.is_usable());
                assert!(sta_guard.will_uninitialize());
            }
            // After ComGuard drops and calls CoUninitialize, switching to MTA on the clean thread succeeds with S_OK.
            let hr_mta = CoInitializeEx(None, COINIT_MULTITHREADED);
            assert_eq!(
                hr_mta, S_OK,
                "ComGuard::drop must cleanly uninitialize STA when S_OK was returned"
            );
            CoUninitialize();
        })
        .join()
        .expect("STA ComGuard thread test panicked");
    }

    #[test]
    fn uwp_windows_group_by_real_aumid_never_collapsing_under_application_frame_host() {
        assert!(is_application_frame_host(
            r"C:\Windows\System32\ApplicationFrameHost.exe"
        ));
        assert!(is_application_frame_host(
            "c:/windows/system32/applicationframehost.exe"
        ));
        assert!(!is_application_frame_host(r"C:\Windows\notepad.exe"));

        let items = vec![
            OpenWindowInfo {
                hwnd: 101,
                pid: 4001,
                title: "Параметры".to_string(),
                exe_path: r"C:\Windows\ImmersiveControlPanel\SystemSettings.exe".to_string(),
                exe_name: "Параметры".to_string(),
                aumid: Some(
                    "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"
                        .to_string(),
                ),
                is_active: false,
                is_minimized: false,
            },
            OpenWindowInfo {
                hwnd: 102,
                pid: 4002,
                title: "Калькулятор".to_string(),
                exe_path: r"C:\Program Files\WindowsApps\Microsoft.WindowsCalculator\CalculatorApp.exe"
                    .to_string(),
                exe_name: "Калькулятор".to_string(),
                aumid: Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".to_string()),
                is_active: false,
                is_minimized: false,
            },
            OpenWindowInfo {
                hwnd: 103,
                pid: 4002,
                title: "Калькулятор — Инженерный".to_string(),
                exe_path: r"C:\Program Files\WindowsApps\Microsoft.WindowsCalculator\CalculatorApp.exe"
                    .to_string(),
                exe_name: "Калькулятор".to_string(),
                aumid: Some("microsoft.windowscalculator_8wekyb3d8bbwe!app".to_string()),
                is_active: true,
                is_minimized: false,
            },
            OpenWindowInfo {
                hwnd: 104,
                pid: 5001,
                title: "VS Code — testbar".to_string(),
                exe_path: r"C:\Users\loki2\AppData\Local\Programs\Microsoft VS Code\Code.exe"
                    .to_string(),
                exe_name: "VS Code".to_string(),
                aumid: None,
                is_active: false,
                is_minimized: false,
            },
            OpenWindowInfo {
                hwnd: 105,
                pid: 5002,
                title: "VS Code — notes".to_string(),
                exe_path: r"C:/Users/loki2/AppData/Local/Programs/Microsoft VS Code/code.exe"
                    .to_string(),
                exe_name: "VS Code".to_string(),
                aumid: None,
                is_active: false,
                is_minimized: true,
            },
        ];

        let groups = group_open_windows(items, |w| {
            if let Some(ref aumid) = w.aumid {
                Some(icon_cache::aumid_key(aumid))
            } else {
                Some(icon_cache::exe_key(&w.exe_path))
            }
        });

        assert_eq!(
            groups.len(),
            3,
            "Settings, Calculator (2 windows), and VS Code (2 windows) must form 3 distinct groups"
        );

        // Settings group
        assert_eq!(
            groups[0].key,
            "aumid:windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"
        );
        assert!(groups[0].icon_key.as_deref().unwrap().starts_with("aumid:"));
        assert_eq!(groups[0].icon_data, None);
        assert_eq!(groups[0].windows.len(), 1);
        assert!(!groups[0].is_active);

        // Calculator group (case-insensitive AUMID merged 2 windows and propagated is_active)
        assert_eq!(
            groups[1].key,
            "aumid:microsoft.windowscalculator_8wekyb3d8bbwe!app"
        );
        assert_eq!(groups[1].windows.len(), 2);
        assert!(groups[1].is_active);
        assert_eq!(groups[1].icon_data, None);
        assert_ne!(groups[0].icon_key, groups[1].icon_key);

        // Win32 VS Code group merged forward/backslash case-insensitively
        assert!(groups[2].key.ends_with("code.exe"));
        assert!(groups[2].icon_key.as_deref().unwrap().starts_with("exe:"));
        assert_eq!(groups[2].windows.len(), 2);
        assert_eq!(groups[2].icon_data, None);
    }

    #[test]
    fn bgra_to_rgba_handles_straight_premultiplied_and_zero_alpha() {
        // Straight alpha with translucent pixel where R > A (must NOT be un-premultiplied)
        let straight = [10u8, 20, 200, 128]; // B=10, G=20, R=200, A=128
        assert_eq!(bgra_to_rgba(&straight), vec![200, 20, 10, 128]);

        // Premultiplied BGRA (e.g. 50% alpha white -> B=128, G=128, R=128, A=128 -> straight RGBA 255,255,255,128)
        let premul = [128u8, 128, 128, 128, 0, 0, 0, 0];
        let out = bgra_to_rgba(&premul);
        assert_eq!(out, vec![255, 255, 255, 128, 0, 0, 0, 0]);

        // Zero-alpha opaque fallback (non-black pixels become opaque, pure black stays transparent)
        let zero_alpha = [0u8, 0, 0, 0, 50, 100, 150, 0];
        assert_eq!(bgra_to_rgba(&zero_alpha), vec![0, 0, 0, 0, 150, 100, 50, 255]);
    }

    #[test]
    fn extract_real_win32_and_uwp_icons_into_icon_cache() {
        let cache = Mutex::new(IconCache::new(16));

        // 1. Classic Win32 executable (explorer.exe) extracts PNG and caches under exe:<hash>
        let explorer_uri = extract_icon_for_path_cached(r"C:\Windows\explorer.exe", &cache)
            .expect("explorer.exe icon must extract on Windows");
        assert!(explorer_uri.starts_with("data:image/png;base64,"));
        let explorer_key = icon_cache::exe_key(r"C:\Windows\explorer.exe");
        let batch = cache.lock().unwrap().get_batch(&[explorer_key.clone()]);
        assert_eq!(batch.get(&explorer_key), Some(&explorer_uri));

        // 2. Built-in Windows 11 UWP Settings app via shell:AppsFolder + IShellItemImageFactory
        let settings_aumid =
            "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel";
        let uwp_uri = extract_uwp_icon_by_aumid(settings_aumid)
            .expect("Windows Settings UWP icon must extract via IShellItemImageFactory");
        assert!(uwp_uri.starts_with("data:image/png;base64,"));
        assert!(uwp_uri.len() > 100, "expected non-trivial PNG payload for Settings UWP icon");
        let uwp_key = icon_cache::aumid_key(settings_aumid);
        cache.lock().unwrap().put(uwp_key.clone(), uwp_uri.clone());
        let batch_uwp = cache.lock().unwrap().get_batch(&[uwp_key.clone()]);
        assert_eq!(batch_uwp.get(&uwp_key), Some(&uwp_uri));
    }
}
