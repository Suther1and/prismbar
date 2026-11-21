use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{Interface, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, BOOL, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER,
    ERROR_SUCCESS, HWND, LPARAM, POINT, RECT, RPC_E_CHANGED_MODE, SIZE, STILL_ACTIVE, S_FALSE,
    S_OK, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetDIBits,
    GetMonitorInfoW, GetObjectW, MonitorFromPoint, MonitorFromRect, MonitorFromWindow, ReleaseDC,
    SelectObject, SetBrushOrgEx, SetStretchBltMode, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HALFTONE, HBITMAP, HDC, HGDIOBJ, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, SRCCOPY,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, STGM,
};
use windows::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_EXPAND_SZ,
    RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, GetExitCodeProcess, OpenProcess,
    QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, IShellLinkW, SHCreateItemFromParsingName, SHGetFileInfoW,
    ShellExecuteW, ShellLink, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SIIGBF_BIGGERSIZEOK,
    SIIGBF_ICONONLY, SIIGBF_RESIZETOFIT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, DrawIconEx, EnumChildWindows, EnumWindows, GetClassLongPtrW, GetClassNameW,
    GetForegroundWindow, GetIconInfo, GetWindow, GetWindowLongPtrW, GetWindowPlacement,
    GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsHungAppWindow,
    IsIconic, IsWindow, IsWindowVisible, IsZoomed, PostMessageW, SendMessageTimeoutW,
    SetForegroundWindow, SetWindowPos, ShowWindow, ShowWindowAsync, DI_NORMAL, GCLP_HICON,
    GCLP_HICONSM, GWL_EXSTYLE, GWL_STYLE, GW_OWNER, HICON, HWND_NOTOPMOST, HWND_TOPMOST, ICON_BIG,
    ICON_SMALL, ICON_SMALL2, MONITORINFOF_PRIMARY, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, WINDOWPLACEMENT,
    WM_CLOSE, WM_GETICON, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_VISIBLE,
};

use crate::coordinator::PixelRect;
use crate::icon_cache::{self, IconCache};
use crate::win_shell::wide;

const PID_META_CACHE_TTL: Duration = Duration::from_secs(30);
const PID_META_CACHE_MAX_ENTRIES: usize = 512;

#[derive(Debug, Clone)]
struct PidMetaEntry {
    exe_path: String,
    aumid: Option<String>,
    cached_at: Instant,
}

static FALLBACK_ICON_CACHE: OnceLock<Mutex<IconCache>> = OnceLock::new();
static PID_META_CACHE: OnceLock<Mutex<HashMap<u32, PidMetaEntry>>> = OnceLock::new();
static STABLE_GROUP_ORDER: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static STABLE_WINDOW_ORDER: OnceLock<Mutex<HashMap<String, Vec<isize>>>> = OnceLock::new();
/// Remembers each live window's last-known non-minimized `HMONITOR` so minimizing a window
/// on a secondary display (which Win32 parks at `(-32000, -32000)`) keeps it attributed to
/// its true monitor instead of snapping to primary.
static LAST_WINDOW_MONITOR: OnceLock<Mutex<HashMap<isize, isize>>> = OnceLock::new();
/// Serializes cold Win32 Shell icon extractions (`IShellItemImageFactory` / `SHGetFileInfoW`)
/// outside `IconCache` so concurrent poller/indexer/IPC threads never race on `shell32.dll`'s
/// internal icon cache or per-thread COM teardown.
static SHELL_ICON_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn shell_icon_lock() -> &'static Mutex<()> {
    SHELL_ICON_LOCK.get_or_init(|| Mutex::new(()))
}

fn fallback_icon_cache() -> &'static Mutex<IconCache> {
    FALLBACK_ICON_CACHE.get_or_init(|| Mutex::new(IconCache::default()))
}

fn pid_meta_cache() -> &'static Mutex<HashMap<u32, PidMetaEntry>> {
    PID_META_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn stable_group_order() -> &'static Mutex<Vec<String>> {
    STABLE_GROUP_ORDER.get_or_init(|| Mutex::new(Vec::new()))
}

fn stable_window_order() -> &'static Mutex<HashMap<String, Vec<isize>>> {
    STABLE_WINDOW_ORDER.get_or_init(|| Mutex::new(HashMap::new()))
}

fn last_window_monitor() -> &'static Mutex<HashMap<isize, isize>> {
    LAST_WINDOW_MONITOR.get_or_init(|| Mutex::new(HashMap::new()))
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

    pub fn init_mta() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
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

struct WindowDcGuard {
    hwnd: HWND,
    hdc: HDC,
}

impl Drop for WindowDcGuard {
    fn drop(&mut self) {
        if !self.hdc.is_invalid() {
            unsafe {
                ReleaseDC(self.hwnd, self.hdc);
            }
        }
    }
}

struct MemDcGuard(HDC);

impl Drop for MemDcGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = DeleteDC(self.0);
            }
        }
    }
}

struct SelectedObjGuard {
    hdc: HDC,
    prev: HGDIOBJ,
}

impl Drop for SelectedObjGuard {
    fn drop(&mut self) {
        if !self.hdc.is_invalid() && !self.prev.0.is_null() {
            unsafe {
                SelectObject(self.hdc, self.prev);
            }
        }
    }
}

pub const MONITOR_PRIMARY: &str = "primary";
pub const MONITOR_SECONDARY: &str = "secondary";

fn default_monitor_label() -> String {
    MONITOR_PRIMARY.to_string()
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
    #[serde(default)]
    pub is_topmost: bool,
    #[serde(default = "default_monitor_label")]
    pub monitor: String,
}

impl Default for OpenWindowInfo {
    fn default() -> Self {
        Self {
            hwnd: 0,
            pid: 0,
            title: String::new(),
            exe_path: String::new(),
            exe_name: String::new(),
            aumid: None,
            is_active: false,
            is_minimized: false,
            is_topmost: false,
            monitor: default_monitor_label(),
        }
    }
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

/// Queries both the full Win32 image path and packaged AUMID (if any) using a single
/// `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` handle.
fn query_process_exe_and_aumid_raw(pid: u32) -> (String, Option<String>) {
    if pid == 0 {
        return (String::new(), None);
    }
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return (String::new(), None);
        };

        let mut exe_buf = vec![0u16; 1024];
        let mut exe_size = exe_buf.len() as u32;
        let exe_path = if QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(exe_buf.as_mut_ptr()),
            &mut exe_size,
        )
        .is_ok()
            && exe_size > 0
        {
            String::from_utf16_lossy(&exe_buf[..exe_size as usize])
        } else {
            String::new()
        };

        let mut aumid_len: u32 = 256;
        let mut aumid_buf = vec![0u16; aumid_len as usize];
        let mut err = GetApplicationUserModelId(
            handle,
            &mut aumid_len,
            PWSTR(aumid_buf.as_mut_ptr()),
        );
        if err == ERROR_INSUFFICIENT_BUFFER && aumid_len > 0 {
            aumid_buf.resize(aumid_len as usize, 0);
            err = GetApplicationUserModelId(
                handle,
                &mut aumid_len,
                PWSTR(aumid_buf.as_mut_ptr()),
            );
        }
        let _ = CloseHandle(handle);

        let aumid = if err == ERROR_SUCCESS && aumid_len > 1 {
            let actual = (aumid_len as usize).min(aumid_buf.len());
            let end = aumid_buf[..actual]
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(actual);
            let s = String::from_utf16_lossy(&aumid_buf[..end]).trim().to_string();
            (!s.is_empty()).then_some(s)
        } else {
            None
        };

        (exe_path, aumid)
    }
}

/// Returns cached `(exe_path, aumid)` for `pid`, performing a single `OpenProcess` query on miss
/// or TTL expiry. Active PID pruning is performed at the end of each `EnumWindows` scan.
fn cached_process_exe_and_aumid(pid: u32) -> (String, Option<String>) {
    if pid == 0 {
        return (String::new(), None);
    }
    let now = Instant::now();
    {
        let cache = lock_unpoisoned(pid_meta_cache());
        if let Some(entry) = cache.get(&pid) {
            if now.saturating_duration_since(entry.cached_at) < PID_META_CACHE_TTL {
                return (entry.exe_path.clone(), entry.aumid.clone());
            }
        }
    }
    let (exe_path, aumid) = query_process_exe_and_aumid_raw(pid);
    {
        let mut cache = lock_unpoisoned(pid_meta_cache());
        if cache.len() >= PID_META_CACHE_MAX_ENTRIES && !cache.contains_key(&pid) {
            cache.retain(|_, v| now.saturating_duration_since(v.cached_at) < PID_META_CACHE_TTL);
            if cache.len() >= PID_META_CACHE_MAX_ENTRIES {
                cache.clear();
            }
        }
        cache.insert(
            pid,
            PidMetaEntry {
                exe_path: exe_path.clone(),
                aumid: aumid.clone(),
                cached_at: now,
            },
        );
    }
    (exe_path, aumid)
}

fn prune_pid_meta_cache(active_pids: &HashSet<u32>) {
    let mut cache = lock_unpoisoned(pid_meta_cache());
    cache.retain(|pid, _| active_pids.contains(pid));
}

pub fn process_exe_path(pid: u32) -> Option<String> {
    let (exe, _) = query_process_exe_and_aumid_raw(pid);
    (!exe.is_empty()).then_some(exe)
}

/// Checks whether a Win32 process `pid` is still alive (`STILL_ACTIVE`).
/// Returns `true` on `ERROR_ACCESS_DENIED` (elevated live process) and `false` if the process
/// does not exist or has already exited.
pub fn is_process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let mut exit_code: u32 = 0;
                let ok = GetExitCodeProcess(handle, &mut exit_code).is_ok();
                let _ = CloseHandle(handle);
                ok && exit_code == STILL_ACTIVE.0 as u32
            }
            Err(err) => err.code() == ERROR_ACCESS_DENIED.to_hresult(),
        }
    }
}

/// Queries the packaged Application User Model ID (AUMID) for a process, if it is a
/// packaged UWP / MSIX / WinUI3 app. Returns `None` for classic unpackaged Win32 processes.
pub fn process_aumid(pid: u32) -> Option<String> {
    query_process_exe_and_aumid_raw(pid).1
}

/// Enumerates child windows of an `ApplicationFrameHost.exe` top-level window to locate
/// the hosted UWP child window & PID (preferring `Windows.UI.Core.CoreWindow`, then any child
/// whose PID differs from `host_pid`).
pub fn resolve_uwp_core_window_hwnd(host_hwnd: HWND, host_pid: u32) -> Option<(HWND, u32)> {
    struct ChildCtx {
        host_pid: u32,
        core_window: Option<(HWND, u32)>,
        fallback: Option<(HWND, u32)>,
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
                    ctx.core_window = Some((child, pid));
                    return BOOL(0);
                }
            }
            if ctx.fallback.is_none() {
                ctx.fallback = Some((child, pid));
            }
            BOOL(1)
        }
    }

    let mut ctx = ChildCtx {
        host_pid,
        core_window: None,
        fallback: None,
    };
    unsafe {
        let _ = EnumChildWindows(host_hwnd, Some(child_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.core_window.or(ctx.fallback)
}

fn resolve_uwp_child_pid(host_hwnd: HWND, host_pid: u32) -> Option<u32> {
    resolve_uwp_core_window_hwnd(host_hwnd, host_pid).map(|(_, pid)| pid)
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
    if hicon.0.is_null() || size <= 0 || size > 512 {
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
        if screen_dc.is_invalid() {
            return None;
        }
        let _screen_guard = WindowDcGuard {
            hwnd: HWND::default(),
            hdc: screen_dc,
        };
        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_invalid() {
            return None;
        }
        let _mem_guard = MemDcGuard(mem_dc);

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = size;
        bmi.bmiHeader.biHeight = -size; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0).ok()?;
        if hbm.is_invalid() || bits_ptr.is_null() {
            if !hbm.is_invalid() {
                let _ = DeleteObject(hbm);
            }
            return None;
        }
        let _hbm_guard = HBitmapGuard(hbm);
        let old = SelectObject(mem_dc, hbm);
        let _sel_guard = SelectedObjGuard {
            hdc: mem_dc,
            prev: old,
        };

        let total_bytes = (size as usize).checked_mul(size as usize)?.checked_mul(4)?;
        std::ptr::write_bytes(bits_ptr as *mut u8, 0, total_bytes);
        SetStretchBltMode(mem_dc, HALFTONE);
        let _ = SetBrushOrgEx(mem_dc, 0, 0, None);
        let _ = DrawIconEx(mem_dc, 0, 0, hicon, size, size, 0, None, DI_NORMAL);

        let bgra = std::slice::from_raw_parts(bits_ptr as *const u8, total_bytes);
        let rgba = bgra_to_rgba(bgra);
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
        if screen_dc.is_invalid() {
            return None;
        }
        let _dc_guard = WindowDcGuard {
            hwnd: HWND::default(),
            hdc: screen_dc,
        };
        let scanlines = GetDIBits(
            screen_dc,
            hbm,
            0,
            h as u32,
            Some(bgra.as_mut_ptr() as *mut std::ffi::c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        if scanlines == 0 {
            return None;
        }

        let rgba = bgra_to_rgba(&bgra);
        encode_rgba_png(w as u32, h as u32, &rgba)
    }
}

/// Extracts a crisp 96x96 High-DPI PNG icon for a packaged UWP/MSIX app via
/// `SHCreateItemFromParsingName("shell:AppsFolder\\<AUMID>")` -> `IShellItemImageFactory::GetImage`
/// protected by an RAII `ComGuard` (AD-5, AD-7).
pub fn extract_uwp_icon_by_aumid(aumid: &str) -> Option<String> {
    let trimmed = aumid.trim();
    if trimmed.is_empty() {
        return None;
    }
    let _shell_guard = lock_unpoisoned(shell_icon_lock());
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
        let size = SIZE { cx: 96, cy: 96 };
        let hbm = factory
            .GetImage(size, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
            .or_else(|_| factory.GetImage(size, SIIGBF_ICONONLY))
            .or_else(|_| factory.GetImage(size, SIIGBF_BIGGERSIZEOK))
            .or_else(|_| factory.GetImage(size, SIIGBF_RESIZETOFIT))
            .ok()?;
        hbitmap_to_data_url(hbm)
    }
}

fn expand_common_env_vars(raw: &str) -> String {
    if !raw.contains('%') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        if let Some(end) = after_start.find('%') {
            let var_name = &after_start[..end];
            if !var_name.is_empty() {
                if let Ok(val) = std::env::var(var_name) {
                    out.push_str(&val);
                    rest = &after_start[end + 1..];
                    continue;
                }
            }
            out.push('%');
            out.push_str(var_name);
            out.push('%');
            rest = &after_start[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out
}

fn read_registry_default_string(root: HKEY, subkey: &str) -> Option<String> {
    let subkey_w = wide(subkey);
    let mut buf_bytes: u32 = 0;
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            PCWSTR::null(),
            flags,
            None,
            None,
            Some(&mut buf_bytes),
        )
    };
    if status != ERROR_SUCCESS || buf_bytes < 2 || buf_bytes > 16_384 {
        return None;
    }
    let mut buf = vec![0u16; (buf_bytes as usize + 1) / 2];
    let status2 = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            PCWSTR::null(),
            flags,
            None,
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut buf_bytes),
        )
    };
    if status2 != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let raw = String::from_utf16_lossy(&buf[..len]);
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(expand_common_env_vars(trimmed))
    }
}

fn lookup_app_path_registry(exe_name: &str) -> Option<String> {
    let subkey = format!(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{}",
        exe_name.trim()
    );
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Some(candidate) = read_registry_default_string(root, &subkey) {
            let norm = candidate.replace('/', "\\");
            if Path::new(&norm).exists() {
                return Some(norm);
            }
        }
    }
    None
}

fn lookup_exe_in_path_env(exe_name: &str) -> Option<String> {
    let clean = exe_name.trim();
    if clean.is_empty() {
        return None;
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(clean);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().replace('/', "\\"));
            }
        }
    }
    for sys_dir in [r"C:\Windows\System32", r"C:\Windows"] {
        let candidate = Path::new(sys_dir).join(clean);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

/// For Squirrel-packaged apps (`...\Discord\Update.exe`, `...\AnthropicClaude\Update.exe`,
/// `...\SparkDesktop\Update.exe`), locates the real versioned `app-*\<App>.exe` sibling
/// executable so icon extraction gets the crisp 96x96 app icon instead of Squirrel's stub icon.
pub fn resolve_squirrel_app_exe(update_exe_path: &Path) -> Option<PathBuf> {
    let fname = update_exe_path.file_name()?.to_string_lossy();
    if !fname.eq_ignore_ascii_case("update.exe") {
        return None;
    }
    let parent = update_exe_path.parent()?;
    let parent_stem = parent
        .file_name()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let mut app_dirs: Vec<PathBuf> = std::fs::read_dir(parent)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase().starts_with("app-"))
                    .unwrap_or(false)
        })
        .collect();
    // Prefer newest version directory first
    app_dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    for dir in app_dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut fallback: Option<PathBuf> = None;
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let is_exe = p
                .extension()
                .map(|e| e.eq_ignore_ascii_case("exe"))
                .unwrap_or(false);
            if !is_exe {
                continue;
            }
            let stem_low = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if stem_low.is_empty()
                || stem_low.contains("update")
                || stem_low.contains("squirrel")
                || stem_low.contains("uninstall")
                || stem_low.contains("crashpad")
                || stem_low.contains("helper")
            {
                continue;
            }
            if !parent_stem.is_empty()
                && (stem_low == parent_stem
                    || parent_stem.contains(&stem_low)
                    || stem_low.contains(&parent_stem))
            {
                return Some(p);
            }
            if fallback.is_none() {
                fallback = Some(p);
            }
        }
        if fallback.is_some() {
            return fallback;
        }
    }

    // Fallback: sibling `<ParentName>.exe` directly next to `Update.exe` (e.g. `Discord\Discord.exe`).
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file()
                || !p
                    .extension()
                    .map(|e| e.eq_ignore_ascii_case("exe"))
                    .unwrap_or(false)
            {
                continue;
            }
            let stem_low = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if stem_low.is_empty()
                || stem_low.contains("update")
                || stem_low.contains("squirrel")
                || stem_low.contains("uninstall")
            {
                continue;
            }
            if !parent_stem.is_empty()
                && (stem_low == parent_stem
                    || parent_stem.contains(&stem_low)
                    || stem_low.contains(&parent_stem))
            {
                return Some(p);
            }
        }
    }
    None
}

/// Returns `true` when `path` points to a 0-byte Windows App Execution Alias stub under
/// `\Microsoft\WindowsApps\` (e.g. `wt.exe`), whose shell icon is often a blank generic exe
/// unless resolved via its UWP `shell:AppsFolder\<AUMID>` entry.
pub fn is_windows_apps_execution_alias(path: &str) -> bool {
    let clean = path.trim().trim_matches('"');
    let norm = clean.replace('/', "\\").to_ascii_lowercase();
    if !norm.contains(r"\microsoft\windowsapps\") || !norm.ends_with(".exe") {
        return false;
    }
    // Use `symlink_metadata` first because `fs::metadata` on `IO_REPARSE_TAG_APPEXECLINK`
    // reparse points can fail with ERROR_CANT_ACCESS_FILE (1920).
    match std::fs::symlink_metadata(clean).or_else(|_| std::fs::metadata(clean)) {
        Ok(meta) => meta.len() == 0,
        Err(_) => true,
    }
}

/// Resolves a local path, Squirrel `Update.exe`, or bare executable name (e.g. `wt.exe`,
/// `spark`, `discord`, `claude`, `browser.exe`) into an existing canonical backslash path via
/// filesystem check, Windows Registry `App Paths` (`HKCU`/`HKLM`), and `PATH`.
pub fn resolve_icon_source_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    let low = trimmed.to_ascii_lowercase();
    if trimmed.contains("://")
        || low.starts_with("ms-")
        || low.starts_with("shell:")
        || low.starts_with("mailto:")
    {
        return None;
    }

    let norm = expand_common_env_vars(trimmed).replace('/', "\\");
    if norm.contains('\\') {
        let p = Path::new(&norm);
        // Handle Windows App Execution Alias reparse points (`...\WindowsApps\wt.exe`) whose
        // `Path::exists()` can return false even when the alias exists on disk.
        if is_windows_apps_execution_alias(&norm)
            && (std::fs::symlink_metadata(p).is_ok() || p.exists())
        {
            return Some(norm);
        }
        if p.exists() {
            if let Some(real_exe) = resolve_squirrel_app_exe(p) {
                return Some(real_exe.to_string_lossy().to_string());
            }
            return Some(norm);
        }
        // Stale absolute path: try resolving its bare executable name via App Paths / PATH
        if let Some(fname) = p.file_name().map(|s| s.to_string_lossy().to_string()) {
            if fname.to_ascii_lowercase().ends_with(".exe")
                && !fname.eq_ignore_ascii_case("update.exe")
            {
                if let Some(found) = lookup_app_path_registry(&fname)
                    .or_else(|| lookup_exe_in_path_env(&fname))
                {
                    let fp = Path::new(&found);
                    if let Some(real_exe) = resolve_squirrel_app_exe(fp) {
                        return Some(real_exe.to_string_lossy().to_string());
                    }
                    return Some(found);
                }
            }
        }
        return None;
    }

    // Bare name such as "wt.exe", "spark", "discord", "claude", "browser.exe"
    let mut candidates = Vec::with_capacity(2);
    if low.ends_with(".exe") {
        candidates.push(norm);
    } else {
        candidates.push(format!("{norm}.exe"));
        candidates.push(norm);
    }
    for cand in candidates {
        if let Some(found) =
            lookup_app_path_registry(&cand).or_else(|| lookup_exe_in_path_env(&cand))
        {
            let fp = Path::new(&found);
            if let Some(real_exe) = resolve_squirrel_app_exe(fp) {
                return Some(real_exe.to_string_lossy().to_string());
            }
            return Some(found);
        }
    }
    None
}

/// Extracts a crystal-clear 96x96 High-DPI native icon via `IShellItemImageFactory::GetImage`
/// (wrapped in RAII `ComGuard::init_sta()`), falling back to `SHGetFileInfoW` + 64x64 HALFTONE
/// `hicon_to_data_url` ONLY when `IShellItemImageFactory` fails.
pub fn extract_path_icon_raw(path: &str) -> Option<String> {
    let trimmed = path.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed
        .to_ascii_lowercase()
        .starts_with("shell:appsfolder\\")
    {
        return extract_uwp_icon_by_aumid(trimmed);
    }

    // Windows Terminal App Execution Alias stub (`...\WindowsApps\wt.exe` or bare `wt.exe`) has
    // 0 bytes (AppExecLink reparse point) and yields a generic box or `exists()==false` unless
    // extracted directly via its UWP AppsFolder entry.
    let low_trimmed = trimmed.replace('/', "\\").to_ascii_lowercase();
    if low_trimmed == "wt.exe"
        || (is_windows_apps_execution_alias(trimmed) && low_trimmed.ends_with(r"\wt.exe"))
    {
        for aumid in [
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            "Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe!App",
        ] {
            if let Some(uwp) = extract_uwp_icon_by_aumid(aumid) {
                return Some(uwp);
            }
        }
    }

    let resolved = resolve_icon_source_path(trimmed)?;

    if is_windows_apps_execution_alias(&resolved)
        && resolved.to_ascii_lowercase().ends_with(r"\wt.exe")
    {
        for aumid in [
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            "Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe!App",
        ] {
            if let Some(uwp) = extract_uwp_icon_by_aumid(aumid) {
                return Some(uwp);
            }
        }
    }

    // 1. Primary path: 96x96 native High-DPI icon via IShellItemImageFactory
    // Keep ComGuard alive across BOTH IShellItemImageFactory and SHGetFileInfoW fallback
    // (MSDN requires COM initialized for SHGetFileInfoW), serialized by SHELL_ICON_LOCK.
    let _shell_guard = lock_unpoisoned(shell_icon_lock());
    let com = ComGuard::init_sta();
    if com.is_usable() {
        let w = wide(&resolved);
        unsafe {
            if let Ok(factory) = SHCreateItemFromParsingName::<_, _, IShellItemImageFactory>(
                PCWSTR(w.as_ptr()),
                None,
            ) {
                let size = SIZE { cx: 96, cy: 96 };
                let img = factory
                    .GetImage(size, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)
                    .or_else(|_| factory.GetImage(size, SIIGBF_ICONONLY));
                if let Ok(hbm) = img {
                    if let Some(uri) = hbitmap_to_data_url(hbm) {
                        return Some(uri);
                    }
                }
            }
        }
    }

    // 2. Fallback: SHGetFileInfoW + 64x64 HALFTONE DrawIconEx
    unsafe {
        let w = wide(&resolved);
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
        let encoded = hicon_to_data_url(shfi.hIcon, 64);
        let _ = DestroyIcon(shfi.hIcon);
        encoded
    }
}

fn extract_live_window_hicon(hwnd: HWND) -> Option<String> {
    if hwnd.0.is_null() {
        return None;
    }
    unsafe {
        if !IsWindow(hwnd).as_bool() || IsHungAppWindow(hwnd).as_bool() {
            return None;
        }
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

pub fn extract_icon_for_path_cached(path: &str, cache: &Mutex<IconCache>) -> Option<String> {
    let trimmed = path.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed
        .to_ascii_lowercase()
        .starts_with("shell:appsfolder\\")
    {
        let key = icon_cache::aumid_key(trimmed);
        {
            let mut guard = lock_unpoisoned(cache);
            if let Some(hit) = guard.get(&key) {
                return Some(hit);
            }
            if guard.is_negative_cached(&key) {
                return None;
            }
        }
        match extract_uwp_icon_by_aumid(trimmed) {
            Some(extracted) => {
                lock_unpoisoned(cache).put(key, extracted.clone());
                return Some(extracted);
            }
            None => {
                lock_unpoisoned(cache).mark_negative_miss(key);
                return None;
            }
        }
    }

    let key = icon_cache::path_icon_key(trimmed);
    {
        let mut guard = lock_unpoisoned(cache);
        if let Some(hit) = guard.get(&key) {
            return Some(hit);
        }
        if guard.is_negative_cached(&key) {
            return None;
        }
    }
    match extract_path_icon_raw(trimmed) {
        Some(extracted) => {
            lock_unpoisoned(cache).put(key, extracted.clone());
            Some(extracted)
        }
        None => {
            lock_unpoisoned(cache).mark_negative_miss(key);
            None
        }
    }
}

pub fn extract_icon_for_path(path: &str) -> Option<String> {
    extract_icon_for_path_cached(path, fallback_icon_cache())
}

/// Resolves an icon into the unified `IconCache` for an executable/shortcut/UWP path
/// (preferring 96x96 `IShellItemImageFactory` via `extract_path_icon_raw`, falling back to
/// `SHGetFileInfoW` 64x64 HALFTONE and `WM_GETICON`/`GCLP_HICON` on `hwnd`, then `hwnd:<hwnd>:0`).
/// Uses bounded TTL negative caching so iconless/elevated apps never thrash on poller ticks.
pub fn resolve_exe_or_hwnd_icon_key(
    exe_path: &str,
    hwnd_raw: Option<isize>,
    cache: &Mutex<IconCache>,
) -> Option<String> {
    let trimmed = exe_path.trim();
    let hwnd = hwnd_raw
        .filter(|&h| h != 0)
        .map(|h| HWND(h as *mut _))
        .unwrap_or_default();

    if trimmed
        .to_ascii_lowercase()
        .starts_with("shell:appsfolder\\")
    {
        let key = icon_cache::aumid_key(trimmed);
        let neg_hit = {
            let mut guard = lock_unpoisoned(cache);
            if guard.get(&key).is_some() {
                return Some(key);
            }
            guard.is_negative_cached(&key)
        };
        if !neg_hit {
            let extracted =
                extract_uwp_icon_by_aumid(trimmed).or_else(|| extract_live_window_hicon(hwnd));
            if let Some(uri) = extracted {
                lock_unpoisoned(cache).put(key.clone(), uri);
                return Some(key);
            }
            lock_unpoisoned(cache).mark_negative_miss(key);
        }
    } else if !trimmed.is_empty() && !is_application_frame_host(trimmed) {
        let key = icon_cache::path_icon_key(trimmed);
        let neg_hit = {
            let mut guard = lock_unpoisoned(cache);
            if guard.get(&key).is_some() {
                return Some(key);
            }
            guard.is_negative_cached(&key)
        };
        if !neg_hit {
            let extracted =
                extract_path_icon_raw(trimmed).or_else(|| extract_live_window_hicon(hwnd));
            if let Some(uri) = extracted {
                lock_unpoisoned(cache).put(key.clone(), uri);
                return Some(key);
            }
            lock_unpoisoned(cache).mark_negative_miss(key);
        }
    }

    if let Some(raw) = hwnd_raw.filter(|&h| h != 0) {
        let key = icon_cache::hwnd_key(raw, 0);
        {
            let mut guard = lock_unpoisoned(cache);
            if guard.get(&key).is_some() {
                return Some(key);
            }
            if guard.is_negative_cached(&key) {
                return None;
            }
        }
        match extract_live_window_hicon(HWND(raw as *mut _)) {
            Some(extracted) => {
                lock_unpoisoned(cache).put(key.clone(), extracted);
                return Some(key);
            }
            None => {
                lock_unpoisoned(cache).mark_negative_miss(key);
                return None;
            }
        }
    }

    None
}

fn resolve_window_icon_key(win: &OpenWindowInfo, cache: &Mutex<IconCache>) -> Option<String> {
    let hwnd = HWND(win.hwnd as *mut _);

    if let Some(ref aumid) = win.aumid {
        let key = icon_cache::aumid_key(aumid);
        let neg_hit = {
            let mut guard = lock_unpoisoned(cache);
            if guard.get(&key).is_some() {
                return Some(key);
            }
            guard.is_negative_cached(&key)
        };
        if !neg_hit {
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
            lock_unpoisoned(cache).mark_negative_miss(key);
        }
    }

    if !win.exe_path.is_empty() && !is_application_frame_host(&win.exe_path) {
        let key = icon_cache::path_icon_key(&win.exe_path);
        let neg_hit = {
            let mut guard = lock_unpoisoned(cache);
            if guard.get(&key).is_some() {
                return Some(key);
            }
            guard.is_negative_cached(&key)
        };
        if !neg_hit {
            let extracted =
                extract_path_icon_raw(&win.exe_path).or_else(|| extract_live_window_hicon(hwnd));
            if let Some(uri) = extracted {
                lock_unpoisoned(cache).put(key.clone(), uri);
                return Some(key);
            }
            lock_unpoisoned(cache).mark_negative_miss(key);
        }
    }

    let key = icon_cache::hwnd_key(win.hwnd, 0);
    {
        let mut guard = lock_unpoisoned(cache);
        if guard.get(&key).is_some() {
            return Some(key);
        }
        if guard.is_negative_cached(&key) {
            return None;
        }
    }
    match extract_live_window_hicon(hwnd) {
        Some(extracted) => {
            lock_unpoisoned(cache).put(key.clone(), extracted);
            Some(key)
        }
        None => {
            lock_unpoisoned(cache).mark_negative_miss(key);
            None
        }
    }
}

/// Computes a reloadable icon source path (`shell:AppsFolder\<AUMID>` or `exe_path`)
/// for an open window so `icon_sources` can seamlessly re-extract icons after LRU eviction.
pub fn icon_source_for_window(win: &OpenWindowInfo) -> Option<String> {
    if let Some(aumid) = win.aumid.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if aumid.to_ascii_lowercase().starts_with("shell:appsfolder\\") {
            return Some(aumid.to_string());
        }
        return Some(format!("shell:AppsFolder\\{aumid}"));
    }
    let exe = win.exe_path.trim();
    if !exe.is_empty() && !is_application_frame_host(exe) {
        return Some(exe.to_string());
    }
    None
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

/// Keeps taskbar application groups and their preview windows in stable first-seen order so
/// clicking/focusing a window (which jumps it to top of Win32 `EnumWindows` Z-order) NEVER
/// teleports unpinned taskbar icons or shuffles preview cards left/right.
/// - Still-open group keys keep their exact existing slot index.
/// - Closed group keys are removed cleanly.
/// - Brand-new group keys are appended strictly at the end.
/// - Inside each group, existing windows preserve their exact first-seen slot order (brand-new
///   windows appearing together on first discovery are ordered deterministically by HWND).
pub fn stabilize_window_groups_order(
    mut groups: Vec<AppWindowGroup>,
    group_order: &mut Vec<String>,
    window_order: &mut HashMap<String, Vec<isize>>,
) -> Vec<AppWindowGroup> {
    let active_keys: HashSet<String> = groups.iter().map(|g| g.key.clone()).collect();
    group_order.retain(|k| active_keys.contains(k));

    let mut known_keys: HashSet<String> = group_order.iter().cloned().collect();
    for g in &groups {
        if known_keys.insert(g.key.clone()) {
            group_order.push(g.key.clone());
        }
    }

    let key_index: HashMap<String, usize> = group_order
        .iter()
        .enumerate()
        .map(|(i, k)| (k.clone(), i))
        .collect();
    groups.sort_by_key(|g| key_index.get(&g.key).copied().unwrap_or(usize::MAX));

    window_order.retain(|k, _| active_keys.contains(k));
    for g in &mut groups {
        let seen_hwnds = window_order.entry(g.key.clone()).or_default();
        let active_hwnds: HashSet<isize> = g.windows.iter().map(|w| w.hwnd).collect();
        seen_hwnds.retain(|h| active_hwnds.contains(h));

        let existing_set: HashSet<isize> = seen_hwnds.iter().copied().collect();
        let mut new_hwnds: Vec<isize> = g
            .windows
            .iter()
            .map(|w| w.hwnd)
            .filter(|h| !existing_set.contains(h))
            .collect();
        new_hwnds.sort_unstable();
        new_hwnds.dedup();
        seen_hwnds.extend(new_hwnds);

        let hwnd_index: HashMap<isize, usize> = seen_hwnds
            .iter()
            .enumerate()
            .map(|(i, &h)| (h, i))
            .collect();
        g.windows
            .sort_by_key(|w| hwnd_index.get(&w.hwnd).copied().unwrap_or(usize::MAX));
    }

    groups
}

/// Classifies a Win32 `MONITORINFO.dwFlags` value into `"primary"` or `"secondary"`.
pub fn classify_monitor_flags(dw_flags: u32) -> &'static str {
    if (dw_flags & MONITORINFOF_PRIMARY) != 0 {
        MONITOR_PRIMARY
    } else {
        MONITOR_SECONDARY
    }
}

/// Converts a `WINDOWPLACEMENT.rcNormalPosition` rectangle (workspace coordinates relative to
/// the primary monitor work area) into virtual-screen coordinates suitable for `MonitorFromRect`.
pub fn workspace_rect_to_screen_rect(
    normal_rc: PixelRect,
    primary_monitor_rc: PixelRect,
    primary_work_rc: PixelRect,
) -> PixelRect {
    let dx = primary_work_rc.left - primary_monitor_rc.left;
    let dy = primary_work_rc.top - primary_monitor_rc.top;
    PixelRect::new(
        normal_rc.left + dx,
        normal_rc.top + dy,
        normal_rc.right + dx,
        normal_rc.bottom + dy,
    )
}

/// Pure monitor attribution resolver combining live `MonitorFromWindow` readings, a last-known
/// non-minimized `HMONITOR` cache, and `GetWindowPlacement` fallback for already-minimized windows.
pub fn resolve_monitor_with_cache<FClassify, FPlacement>(
    hwnd: isize,
    is_minimized: bool,
    live_hmon: Option<isize>,
    prev_monitors: &HashMap<isize, isize>,
    updated_monitors: &mut HashMap<isize, isize>,
    mut classify_hmon: FClassify,
    placement_hmon: FPlacement,
) -> &'static str
where
    FClassify: FnMut(isize) -> Option<&'static str>,
    FPlacement: FnOnce() -> Option<isize>,
{
    if !is_minimized {
        if let Some(hmon) = live_hmon {
            if let Some(label) = classify_hmon(hmon) {
                updated_monitors.insert(hwnd, hmon);
                return label;
            }
        }
        if let Some(&cached) = prev_monitors.get(&hwnd) {
            if let Some(label) = classify_hmon(cached) {
                return label;
            }
        }
        return MONITOR_PRIMARY;
    }

    if let Some(&cached) = prev_monitors.get(&hwnd) {
        if let Some(label) = classify_hmon(cached) {
            return label;
        }
    }

    if let Some(hmon) = placement_hmon() {
        if let Some(label) = classify_hmon(hmon) {
            updated_monitors.insert(hwnd, hmon);
            return label;
        }
    }

    MONITOR_PRIMARY
}

/// Filters grouped open windows to a specific monitor (`"primary"` or `"secondary"`).
///
/// Prunes groups that have no windows on `monitor` and recomputes each remaining group's
/// `is_active` strictly from its filtered windows so focusing a Monitor 1 window never falsely
/// highlights the same app's Monitor 2 taskbar button.
pub fn filter_window_groups_for_monitor(
    groups: &[AppWindowGroup],
    monitor: &str,
) -> Vec<AppWindowGroup> {
    let target = monitor.trim();
    if target.is_empty() || target.eq_ignore_ascii_case("all") {
        return groups.to_vec();
    }

    let mut filtered = Vec::with_capacity(groups.len());
    for group in groups {
        let windows: Vec<OpenWindowInfo> = group
            .windows
            .iter()
            .filter(|w| {
                let win_mon = if w.monitor.is_empty() {
                    MONITOR_PRIMARY
                } else {
                    w.monitor.as_str()
                };
                win_mon.eq_ignore_ascii_case(target)
            })
            .cloned()
            .collect();

        if windows.is_empty() {
            continue;
        }

        let is_active = windows.iter().any(|w| w.is_active);
        filtered.push(AppWindowGroup {
            key: group.key.clone(),
            name: group.name.clone(),
            exe_path: group.exe_path.clone(),
            aumid: group.aumid.clone(),
            icon_key: group.icon_key.clone(),
            icon_data: group.icon_data.clone(),
            is_active,
            windows,
        });
    }
    filtered
}

fn classify_hmonitor(hmon_raw: isize) -> Option<&'static str> {
    if hmon_raw == 0 {
        return None;
    }
    let hmon = HMONITOR(hmon_raw as *mut std::ffi::c_void);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(hmon, &mut mi).as_bool() } {
        Some(classify_monitor_flags(mi.dwFlags))
    } else {
        None
    }
}

fn primary_monitor_rects() -> (PixelRect, PixelRect) {
    let hmon = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !hmon.is_invalid() && unsafe { GetMonitorInfoW(hmon, &mut mi).as_bool() } {
        let monitor_rc = PixelRect::new(
            mi.rcMonitor.left,
            mi.rcMonitor.top,
            mi.rcMonitor.right,
            mi.rcMonitor.bottom,
        );
        let work_rc = PixelRect::new(
            mi.rcWork.left,
            mi.rcWork.top,
            mi.rcWork.right,
            mi.rcWork.bottom,
        );
        (monitor_rc, work_rc)
    } else {
        (PixelRect::new(0, 0, 0, 0), PixelRect::new(0, 0, 0, 0))
    }
}

fn monitor_from_window_placement(
    hwnd: HWND,
    primary_monitor_rc: PixelRect,
    primary_work_rc: PixelRect,
) -> Option<isize> {
    let mut wp = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    if unsafe { GetWindowPlacement(hwnd, &mut wp).is_err() } {
        return None;
    }
    let normal_rc = PixelRect::new(
        wp.rcNormalPosition.left,
        wp.rcNormalPosition.top,
        wp.rcNormalPosition.right,
        wp.rcNormalPosition.bottom,
    );
    let screen = workspace_rect_to_screen_rect(normal_rc, primary_monitor_rc, primary_work_rc);
    let screen_rect = RECT {
        left: screen.left,
        top: screen.top,
        right: screen.right,
        bottom: screen.bottom,
    };
    let hmon = unsafe { MonitorFromRect(&screen_rect, MONITOR_DEFAULTTONEAREST) };
    if hmon.is_invalid() {
        None
    } else {
        Some(hmon.0 as isize)
    }
}

/// Enumerates open user windows grouped by application and also returns unique
/// `(icon_key, source_path)` pairs so the host can register every window icon into `icon_sources`
/// and cleanly recover icons after LRU eviction.
pub fn list_open_window_groups_cached_with_sources(
    cache: &Mutex<IconCache>,
) -> (Vec<AppWindowGroup>, Vec<(String, String)>) {
    struct Ctx {
        own_pid: u32,
        fg_hwnd: HWND,
        primary_monitor_rc: PixelRect,
        primary_work_rc: PixelRect,
        prev_monitors: HashMap<isize, isize>,
        updated_monitors: HashMap<isize, isize>,
        live_hwnds: HashSet<isize>,
        items: Vec<OpenWindowInfo>,
        active_pids: HashSet<u32>,
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
            ctx.active_pids.insert(host_pid);

            let (host_exe, host_aumid) = cached_process_exe_and_aumid(host_pid);
            let is_uwp_host = is_application_frame_host(&host_exe);

            let (effective_pid, exe_path, aumid) = if is_uwp_host {
                if let Some(child_pid) = resolve_uwp_child_pid(hwnd, host_pid) {
                    ctx.active_pids.insert(child_pid);
                    let (child_exe_raw, child_aumid) = cached_process_exe_and_aumid(child_pid);
                    let child_exe = if child_exe_raw.is_empty() {
                        host_exe.clone()
                    } else {
                        child_exe_raw
                    };
                    (child_pid, child_exe, child_aumid)
                } else {
                    (host_pid, host_exe, None)
                }
            } else {
                (host_pid, host_exe, host_aumid)
            };

            let exe_name = display_name_for_window(&exe_path, is_uwp_host, &title);
            let hwnd_raw = hwnd.0 as isize;
            let is_minimized = IsIconic(hwnd).as_bool();
            ctx.live_hwnds.insert(hwnd_raw);

            let live_hmon = if is_minimized {
                None
            } else {
                let h = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                if h.is_invalid() {
                    None
                } else {
                    Some(h.0 as isize)
                }
            };
            let primary_mon_rc = ctx.primary_monitor_rc;
            let primary_work_rc = ctx.primary_work_rc;
            let monitor = resolve_monitor_with_cache(
                hwnd_raw,
                is_minimized,
                live_hmon,
                &ctx.prev_monitors,
                &mut ctx.updated_monitors,
                classify_hmonitor,
                || monitor_from_window_placement(hwnd, primary_mon_rc, primary_work_rc),
            );

            ctx.items.push(OpenWindowInfo {
                hwnd: hwnd_raw,
                pid: effective_pid,
                title,
                exe_path,
                exe_name,
                aumid,
                is_active: hwnd == ctx.fg_hwnd,
                is_minimized,
                is_topmost: (ex & WS_EX_TOPMOST.0) != 0,
                monitor: monitor.to_string(),
            });
        }
        BOOL(1)
    }

    let (primary_monitor_rc, primary_work_rc) = primary_monitor_rects();
    let prev_monitors = lock_unpoisoned(last_window_monitor()).clone();

    let mut ctx = Ctx {
        own_pid: unsafe { GetCurrentProcessId() },
        fg_hwnd: unsafe { GetForegroundWindow() },
        primary_monitor_rc,
        primary_work_rc,
        prev_monitors,
        updated_monitors: HashMap::new(),
        live_hwnds: HashSet::new(),
        items: Vec::new(),
        active_pids: HashSet::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    prune_pid_meta_cache(&ctx.active_pids);
    {
        let mut cache_map = lock_unpoisoned(last_window_monitor());
        for (hwnd_raw, hmon_raw) in ctx.updated_monitors {
            cache_map.insert(hwnd_raw, hmon_raw);
        }
        cache_map.retain(|hwnd_raw, _| ctx.live_hwnds.contains(hwnd_raw));
    }

    let mut icon_sources: Vec<(String, String)> = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();
    let groups = group_open_windows(ctx.items, |win| {
        let resolved = resolve_window_icon_key(win, cache)?;
        if let Some(source) = icon_source_for_window(win) {
            if seen_keys.insert(resolved.clone()) {
                icon_sources.push((resolved.clone(), source));
            }
        }
        Some(resolved)
    });

    let groups = {
        let mut g_order = lock_unpoisoned(stable_group_order());
        let mut w_order = lock_unpoisoned(stable_window_order());
        stabilize_window_groups_order(groups, &mut g_order, &mut w_order)
    };

    (groups, icon_sources)
}

pub fn list_open_window_groups_cached(cache: &Mutex<IconCache>) -> Vec<AppWindowGroup> {
    list_open_window_groups_cached_with_sources(cache).0
}

pub fn list_open_window_groups() -> Vec<AppWindowGroup> {
    list_open_window_groups_cached(fallback_icon_cache())
}

pub fn focus_or_toggle_window(hwnd_raw: isize, toggle_minimize: bool) {
    if hwnd_raw == 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindow(hwnd).as_bool() {
            return;
        }
        let fg = GetForegroundWindow();
        if toggle_minimize && fg == hwnd && !IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_MINIMIZE);
            return;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindowAsync(hwnd, SW_SHOW);
        }

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

pub fn close_window(hwnd_raw: isize) {
    if hwnd_raw == 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if IsWindow(hwnd).as_bool() {
            let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

/// Pure, DWM-frame-compensated snap target geometry computation for `"left"`, `"right"`,
/// and `"center"` zones inside a monitor's physical `work_area`.
/// `frame_inset` is `(left, top, right, bottom)` invisible DWM shadow padding (`ext - win`).
pub fn compute_snap_rect(
    work_area: PixelRect,
    current_visible: Option<PixelRect>,
    frame_inset: (i32, i32, i32, i32),
    zone: &str,
) -> Result<PixelRect, String> {
    if work_area.width() <= 0 || work_area.height() <= 0 {
        return Err("Некорректная рабочая область монитора".to_string());
    }
    let (inset_l, inset_t, inset_r, inset_b) = (
        frame_inset.0.max(0),
        frame_inset.1.max(0),
        frame_inset.2.max(0),
        frame_inset.3.max(0),
    );
    let visible = match zone.trim().to_ascii_lowercase().as_str() {
        "left" => {
            let mid_x = work_area.left + (work_area.width() / 2);
            PixelRect::new(work_area.left, work_area.top, mid_x, work_area.bottom)
        }
        "right" => {
            let mid_x = work_area.left + (work_area.width() / 2);
            PixelRect::new(mid_x, work_area.top, work_area.right, work_area.bottom)
        }
        "center" => {
            let default_w = ((work_area.width() as f64) * 0.72).round() as i32;
            let default_h = ((work_area.height() as f64) * 0.78).round() as i32;
            let max_w = ((work_area.width() as f64) * 0.90).round() as i32;
            let max_h = ((work_area.height() as f64) * 0.90).round() as i32;
            let (target_w, target_h) = match current_visible {
                Some(cur)
                    if cur.width() >= 320
                        && cur.height() >= 240
                        && cur.width() <= max_w
                        && cur.height() <= max_h =>
                {
                    (cur.width(), cur.height())
                }
                _ => (
                    default_w.clamp(320.min(work_area.width()), work_area.width()),
                    default_h.clamp(240.min(work_area.height()), work_area.height()),
                ),
            };
            let left = work_area.left + (work_area.width() - target_w) / 2;
            let top = work_area.top + (work_area.height() - target_h) / 2;
            PixelRect::new(left, top, left + target_w, top + target_h)
        }
        other => return Err(format!("Неизвестная зона привязки окна: {other}")),
    };

    Ok(PixelRect::new(
        visible.left - inset_l,
        visible.top - inset_t,
        visible.right + inset_r,
        visible.bottom + inset_b,
    ))
}

unsafe fn query_window_dwm_frame_insets(hwnd: HWND) -> (Option<PixelRect>, (i32, i32, i32, i32)) {
    let mut win_rect = RECT::default();
    if GetWindowRect(hwnd, &mut win_rect).is_err() {
        return (None, (0, 0, 0, 0));
    }
    let mut ext_rect = RECT::default();
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut ext_rect as *mut RECT as *mut _,
        std::mem::size_of::<RECT>() as u32,
    )
    .is_ok()
        && ext_rect.right > ext_rect.left
        && ext_rect.bottom > ext_rect.top
    {
        let inset_l = (ext_rect.left - win_rect.left).clamp(0, 48);
        let inset_t = (ext_rect.top - win_rect.top).clamp(0, 48);
        let inset_r = (win_rect.right - ext_rect.right).clamp(0, 48);
        let inset_b = (win_rect.bottom - ext_rect.bottom).clamp(0, 48);
        let ext_px = PixelRect::new(ext_rect.left, ext_rect.top, ext_rect.right, ext_rect.bottom);
        (Some(ext_px), (inset_l, inset_t, inset_r, inset_b))
    } else {
        let win_px = PixelRect::new(win_rect.left, win_rect.top, win_rect.right, win_rect.bottom);
        (Some(win_px), (0, 0, 0, 0))
    }
}

/// DPI/work-area/DWM-frame-aware pure-Win32 window snapping (`"left" | "right" | "maximize" | "restore" | "center"`).
pub fn snap_window(hwnd_raw: isize, zone: &str) -> Result<(), String> {
    if hwnd_raw == 0 {
        return Err("Некорректный дескриптор окна".to_string());
    }
    let norm_zone = zone.trim().to_ascii_lowercase();
    if !matches!(
        norm_zone.as_str(),
        "left" | "right" | "maximize" | "restore" | "center"
    ) {
        return Err(format!("Неизвестная зона привязки окна: {zone}"));
    }

    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindow(hwnd).as_bool() {
            return Err("Окно уже закрыто".to_string());
        }
        if IsHungAppWindow(hwnd).as_bool() {
            return Err("Окно не отвечает".to_string());
        }

        if norm_zone == "maximize" {
            let _ = ShowWindowAsync(hwnd, SW_MAXIMIZE);
            focus_or_toggle_window(hwnd_raw, false);
            return Ok(());
        }
        if norm_zone == "restore" {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
            focus_or_toggle_window(hwnd_raw, false);
            return Ok(());
        }

        if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mon_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut mon_info).as_bool() {
            return Err("Не удалось определить рабочую область монитора".to_string());
        }
        let work_area = PixelRect::new(
            mon_info.rcWork.left,
            mon_info.rcWork.top,
            mon_info.rcWork.right,
            mon_info.rcWork.bottom,
        );
        let (current_visible, insets) = query_window_dwm_frame_insets(hwnd);
        let target = compute_snap_rect(work_area, current_visible, insets, &norm_zone)?;

        SetWindowPos(
            hwnd,
            HWND::default(),
            target.left,
            target.top,
            target.width(),
            target.height(),
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .map_err(|err| {
            if err.code() == ERROR_ACCESS_DENIED.to_hresult() {
                "Окно запущено от имени администратора — перемещение заблокировано UIPI".to_string()
            } else {
                format!("Не удалось разместить окно: {err}")
            }
        })?;
        focus_or_toggle_window(hwnd_raw, false);
        Ok(())
    }
}

/// Toggles `WS_EX_TOPMOST` (Always on Top) for a window, returning the new topmost state
/// or a clear UIPI error if the target belongs to an elevated process.
pub fn toggle_window_topmost(hwnd_raw: isize) -> Result<bool, String> {
    if hwnd_raw == 0 {
        return Err("Некорректный дескриптор окна".to_string());
    }
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindow(hwnd).as_bool() {
            return Err("Окно уже закрыто".to_string());
        }
        if IsHungAppWindow(hwnd).as_bool() {
            return Err("Окно не отвечает".to_string());
        }
        let was_topmost = (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
        let next_topmost = !was_topmost;
        let insert_after = if next_topmost {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .map_err(|err| {
            if err.code() == ERROR_ACCESS_DENIED.to_hresult() {
                "Окно запущено от имени администратора — «Поверх всех окон» заблокировано UIPI"
                    .to_string()
            } else {
                format!("Не удалось изменить закрепление окна поверх всех: {err}")
            }
        })?;

        let actual_topmost = (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
        if actual_topmost != next_topmost {
            return Err(
                "Не удалось изменить «Поверх всех окон» (окно защищено правами администратора / UIPI)"
                    .to_string(),
            );
        }
        Ok(actual_topmost)
    }
}

/// Minimizes all visible non-minimized user windows except `keep_hwnd_raw`, returning
/// the number of windows minimized.
pub fn minimize_other_windows(keep_hwnd_raw: isize) -> Result<u32, String> {
    if keep_hwnd_raw == 0 {
        return Err("Некорректный дескриптор окна".to_string());
    }
    let keep_hwnd = HWND(keep_hwnd_raw as *mut _);
    unsafe {
        if !IsWindow(keep_hwnd).as_bool() {
            return Err("Целевое окно уже закрыто".to_string());
        }
    }

    struct MinCtx {
        own_pid: u32,
        keep_hwnd: HWND,
        minimized: u32,
    }

    extern "system" fn min_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            let ctx = &mut *(lparam.0 as *mut MinCtx);
            if hwnd == ctx.keep_hwnd {
                return BOOL(1);
            }
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
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
            if GetWindowTextLengthW(hwnd) <= 0 {
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
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 || pid == ctx.own_pid {
                return BOOL(1);
            }
            if IsHungAppWindow(hwnd).as_bool() {
                return BOOL(1);
            }
            if ShowWindowAsync(hwnd, SW_MINIMIZE).as_bool() {
                ctx.minimized = ctx.minimized.saturating_add(1);
            }
        }
        BOOL(1)
    }

    let mut ctx = MinCtx {
        own_pid: unsafe { GetCurrentProcessId() },
        keep_hwnd,
        minimized: 0,
    };
    unsafe {
        let _ = EnumWindows(Some(min_proc), LPARAM(&mut ctx as *mut _ as isize));
    }
    focus_or_toggle_window(keep_hwnd_raw, false);
    Ok(ctx.minimized)
}

/// Pure helper selecting smart new-window CLI arguments for Chromium/Gecko browsers,
/// Electron editors, Windows Terminal, and Explorer while preserving caller-provided args.
pub fn build_new_window_launch_args(exe_path: &str, args: Option<&str>) -> Option<String> {
    let user_args = args.map(str::trim).filter(|s| !s.is_empty());
    let base = normalized_exe_basename(exe_path);
    let smart_flag = match base.as_str() {
        "chrome.exe"
        | "msedge.exe"
        | "brave.exe"
        | "browser.exe"
        | "vivaldi.exe"
        | "opera.exe"
        | "code.exe"
        | "cursor.exe"
        | "antigravity.exe" => Some("--new-window"),
        "firefox.exe" | "zen.exe" => Some("-new-window"),
        "explorer.exe" => Some("/n"),
        "wt.exe" | "windowsterminal.exe" => Some("-w new"),
        _ => None,
    };

    match (smart_flag, user_args) {
        (Some(flag), Some(existing)) => {
            let low = existing.to_ascii_lowercase();
            if low.contains("--new-window")
                || low.contains("-new-window")
                || low.contains("--incognito")
                || low.contains("--inprivate")
                || low.contains("-private-window")
                || low.contains("-w ")
                || low == "/n"
                || low.starts_with("/n ")
            {
                Some(existing.to_string())
            } else {
                Some(format!("{flag} {existing}"))
            }
        }
        (Some(flag), None) => Some(flag.to_string()),
        (None, Some(existing)) => Some(existing.to_string()),
        (None, None) => None,
    }
}

/// Launches a fresh window instance for `exe_path`, injecting smart `--new-window` / `-new-window`
/// flags for Chromium/Gecko/VS Code/Terminal/Explorer apps where appropriate.
pub fn launch_new_window_instance(exe_path: &str, args: Option<&str>) -> Result<(), String> {
    let trimmed = exe_path.trim();
    if trimmed.is_empty() {
        return Err("Пустой путь запуска".to_string());
    }
    let effective_args = build_new_window_launch_args(trimmed, args);
    launch_target_with_args(trimmed, effective_args.as_deref(), false)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JumpListItem {
    pub title: String,
    pub target: String,
    pub args: Option<String>,
    /// `"task"` | `"recent"`
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedLaunchSpec {
    pub target: String,
    pub args: Option<String>,
    pub verb: &'static str,
}

fn format_terminate_error(pid: u32, err: windows::core::Error, stage: &str) -> String {
    if err.code() == ERROR_ACCESS_DENIED.to_hresult() {
        format!("Отказано в доступе при завершении процесса {pid} (нужны права администратора)")
    } else {
        format!("Не удалось {stage} процесс {pid}: {err}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PidTerminateOutcome {
    Terminated,
    AlreadyExited,
    AccessDenied(String),
    Failed(String),
}

pub fn summarize_pid_termination_results(results: &[PidTerminateOutcome]) -> Result<(), String> {
    if results.is_empty() {
        return Err("Нет активных процессов для завершения".to_string());
    }
    let mut terminated = 0usize;
    let mut already_exited = 0usize;
    let mut access_denied: Option<String> = None;
    let mut other_err: Option<String> = None;

    for outcome in results {
        match outcome {
            PidTerminateOutcome::Terminated => terminated += 1,
            PidTerminateOutcome::AlreadyExited => already_exited += 1,
            PidTerminateOutcome::AccessDenied(msg) => {
                if access_denied.is_none() {
                    access_denied = Some(msg.clone());
                }
            }
            PidTerminateOutcome::Failed(msg) => {
                if other_err.is_none() {
                    other_err = Some(msg.clone());
                }
            }
        }
    }

    if let Some(msg) = access_denied {
        return Err(msg);
    }
    if terminated > 0 || (already_exited > 0 && other_err.is_none()) {
        return Ok(());
    }
    Err(other_err.unwrap_or_else(|| "Не удалось завершить процесс".to_string()))
}

fn terminate_single_pid_outcome(pid: u32) -> PidTerminateOutcome {
    if pid == 0 {
        return PidTerminateOutcome::Failed("Некорректный PID процесса".to_string());
    }
    unsafe {
        let handle = match OpenProcess(PROCESS_TERMINATE, false, pid) {
            Ok(h) => h,
            Err(err) => {
                if err.code() == ERROR_ACCESS_DENIED.to_hresult() {
                    return PidTerminateOutcome::AccessDenied(format_terminate_error(
                        pid, err, "открыть",
                    ));
                }
                if err.code() == ERROR_INVALID_PARAMETER.to_hresult() {
                    return PidTerminateOutcome::AlreadyExited;
                }
                return PidTerminateOutcome::Failed(format_terminate_error(pid, err, "открыть"));
            }
        };
        let res = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        match res {
            Ok(()) => PidTerminateOutcome::Terminated,
            Err(err) => {
                if err.code() == ERROR_ACCESS_DENIED.to_hresult() {
                    PidTerminateOutcome::AccessDenied(format_terminate_error(
                        pid,
                        err,
                        "завершить",
                    ))
                } else {
                    PidTerminateOutcome::Failed(format_terminate_error(pid, err, "завершить"))
                }
            }
        }
    }
}

pub fn terminate_process_by_pid(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("Некорректный PID процесса".to_string());
    }
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format_terminate_error(pid, e, "открыть"))?;
        let res = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);
        res.map_err(|e| format_terminate_error(pid, e, "завершить"))
    }
}

/// Terminates all unique non-zero PIDs belonging to a background/multi-process app,
/// tolerating sibling PIDs that already exited while always propagating OS permission errors.
pub fn terminate_process_pids(pids: &[u32]) -> Result<(), String> {
    let own_pid = unsafe { GetCurrentProcessId() };
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for &pid in pids {
        if pid != 0 && pid != own_pid && seen.insert(pid) {
            unique.push(pid);
        }
    }
    if unique.is_empty() {
        return Err("Нет активных процессов для завершения".to_string());
    }
    let outcomes: Vec<PidTerminateOutcome> = unique
        .into_iter()
        .map(terminate_single_pid_outcome)
        .collect();
    summarize_pid_termination_results(&outcomes)
}

/// Validates and normalizes target + optional CLI args for direct Win32 `ShellExecuteW`
/// launch (`"runas"` or `"open"`), avoiding any shell/PowerShell string interpolation.
pub fn normalize_launch_spec(
    target: &str,
    args: Option<&str>,
    as_admin: bool,
) -> Result<NormalizedLaunchSpec, String> {
    let raw_target = target.trim();
    if raw_target.is_empty() {
        return Err("Пустой путь запуска".to_string());
    }
    if raw_target.contains('\0') || raw_target.contains('\r') || raw_target.contains('\n') {
        return Err("Недопустимые символы в пути запуска".to_string());
    }

    let (clean_target, inline_args) = if let Some(after_open) = raw_target.strip_prefix('"') {
        let Some(close_idx) = after_open.find('"') else {
            return Err("Незакрытая кавычка в пути запуска".to_string());
        };
        let t = after_open[..close_idx].trim();
        let rest = after_open[close_idx + 1..].trim();
        (t, (!rest.is_empty()).then_some(rest))
    } else {
        (raw_target, None)
    };

    if clean_target.is_empty() {
        return Err("Пустой путь запуска".to_string());
    }

    let extra_args = args.map(str::trim).filter(|s| !s.is_empty());
    if let Some(a) = extra_args {
        if a.contains('\0') || a.contains('\r') || a.contains('\n') {
            return Err("Недопустимые символы в аргументах запуска".to_string());
        }
    }

    let merged_args = match (inline_args, extra_args) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (Some(a), None) => Some(a.to_string()),
        (None, Some(b)) => Some(b.to_string()),
        (None, None) => None,
    };

    Ok(NormalizedLaunchSpec {
        target: clean_target.to_string(),
        args: merged_args,
        verb: if as_admin { "runas" } else { "open" },
    })
}

pub fn format_shell_execute_error(code: isize, target: &str) -> String {
    match code {
        2 | 3 => format!("Файл или путь не найден: {target}"),
        5 => format!("Отказано в доступе или запуск отменён: {target}"),
        31 => format!("Нет связанного приложения для: {target}"),
        _ => format!("Не удалось открыть {target} (код {code})"),
    }
}

pub fn launch_target_with_args(
    target: &str,
    args: Option<&str>,
    as_admin: bool,
) -> Result<(), String> {
    let spec = normalize_launch_spec(target, args, as_admin)?;
    unsafe {
        let verb = wide(spec.verb);
        let file = wide(&spec.target);
        let params_wide = spec.args.as_deref().map(wide);
        let params_ptr = params_wide
            .as_ref()
            .map(|w| PCWSTR(w.as_ptr()))
            .unwrap_or_else(PCWSTR::null);
        let res = ShellExecuteW(
            HWND::default(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            params_ptr,
            PCWSTR::null(),
            SW_SHOW,
        );
        let code = res.0 as isize;
        if code <= 32 {
            return Err(format_shell_execute_error(code, &spec.target));
        }
    }
    Ok(())
}

pub fn launch_target(target: &str, as_admin: bool) -> Result<(), String> {
    launch_target_with_args(target, None, as_admin)
}

/// Parses a Game Combo target string into an executable/URI target plus optional CLI arguments.
/// Supports:
/// - URI schemes (`steam://rungameid/570`, `https://...`)
/// - Quoted paths with trailing args (`"C:\...\Update.exe" --processStart Discord.exe`)
/// - Unquoted paths with `.exe` followed by CLI args (`C:\...\Update.exe --processStart Discord.exe`)
/// - Bare Squirrel `Discord\Update.exe` (automatically injects `--processStart Discord.exe`)
pub fn parse_combo_target(raw: &str) -> Result<NormalizedLaunchSpec, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Пустой путь запуска".to_string());
    }

    let mut spec = if trimmed.starts_with('"') || trimmed.contains("://") {
        normalize_launch_spec(trimmed, None, false)?
    } else {
        let lower = trimmed.to_ascii_lowercase();
        let mut split_at: Option<usize> = None;
        let mut search_from = 0usize;
        while let Some(rel) = lower[search_from..].find(".exe") {
            let end = search_from + rel + 4;
            if let Some(next_ch) = trimmed[end..].chars().next() {
                if next_ch.is_whitespace() {
                    split_at = Some(end);
                    break;
                }
            }
            search_from = end;
        }
        if let Some(idx) = split_at {
            let exe_part = trimmed[..idx].trim();
            let args_part = trimmed[idx..].trim();
            normalize_launch_spec(exe_part, (!args_part.is_empty()).then_some(args_part), false)?
        } else {
            normalize_launch_spec(trimmed, None, false)?
        }
    };

    if spec.args.is_none() && crate::config::is_discord_update_target(&spec.target) {
        spec.args = Some("--processStart Discord.exe".to_string());
    }

    Ok(spec)
}

/// Launches every non-empty target in a Game Combo, parsing CLI arguments cleanly and
/// returning any aggregated launch error without aborting sibling targets.
pub fn launch_combo_targets(targets: &[String]) -> Result<(), String> {
    let specs: Vec<Result<NormalizedLaunchSpec, String>> = targets
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(parse_combo_target)
        .collect();
    if specs.is_empty() {
        return Err("В связке нет целей для запуска".to_string());
    }
    let mut errors = Vec::new();
    for spec_res in specs {
        match spec_res {
            Ok(spec) => {
                if let Err(err) = launch_target_with_args(&spec.target, spec.args.as_deref(), false)
                {
                    errors.push(err);
                }
            }
            Err(err) => errors.push(err),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Formats an unescaped Windows command-line raw argument for `explorer.exe /select,"<path>"`
/// so paths containing spaces never get broken by Rust's default `.arg()` quote escaping (AD-7).
pub fn format_explorer_select_raw_arg(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Пустой путь".to_string());
    }
    if trimmed.contains('\0') || trimmed.contains('\r') || trimmed.contains('\n') {
        return Err("Недопустимый путь".to_string());
    }
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed)
        .trim();
    if unquoted.is_empty() {
        return Err("Пустой путь".to_string());
    }
    if unquoted.contains('"') {
        return Err("Путь содержит недопустимые кавычки".to_string());
    }
    let lower = unquoted.to_ascii_lowercase();
    if lower.contains("://") || lower.starts_with("shell:") || lower.starts_with("ms-") {
        return Err("Расположение файла недоступно для системных URI".to_string());
    }
    let normalized = unquoted.replace('/', "\\");
    Ok(format!("/select,\"{normalized}\""))
}

pub fn open_file_location(path: &str) -> Result<(), String> {
    let raw = format_explorer_select_raw_arg(path)?;
    std::process::Command::new("explorer.exe")
        .raw_arg(raw)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось открыть Проводник: {e}"))
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
    let ps_escaped = target.replace('\'', "''");
    std::process::Command::new("powershell.exe")
        .args([
            "-NoExit",
            "-Command",
            &format!("Set-Location -LiteralPath '{ps_escaped}'"),
        ])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn normalized_exe_basename(exe_path: &str) -> String {
    exe_path
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .rsplit('\\')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Normalizes helper/CEF child executables (such as `D:\Steam\bin\cef\cef.win64\steamwebhelper.exe`
/// or Squirrel `Discord\Update.exe`) to their parent desktop application executable so Jump List
/// AppID lookup and quick tasks resolve identically to native Windows 11.
pub fn normalize_jump_list_exe_path(exe_path: &str) -> String {
    let clean = exe_path.trim().trim_matches('"').replace('/', "\\");
    if clean.is_empty() {
        return clean;
    }
    let base = normalized_exe_basename(&clean);
    if base == "steamwebhelper.exe" {
        let p = Path::new(&clean);
        let mut cur = p.parent();
        for _ in 0..5 {
            let Some(dir) = cur else { break };
            let candidate = dir.join("steam.exe");
            if candidate.is_file() {
                return candidate.to_string_lossy().to_string();
            }
            cur = dir.parent();
        }
        // Synthetic / offline path normalization fallback: strip `\bin\cef\...`
        let low = clean.to_ascii_lowercase();
        if let Some(idx) = low.find(r"\bin\cef\") {
            return format!(r"{}\steam.exe", &clean[..idx]);
        }
    } else if base == "update.exe" {
        if let Some(real_exe) = resolve_squirrel_app_exe(Path::new(&clean)) {
            return real_exe.to_string_lossy().to_string();
        }
    }
    clean
}

/// Computes the exact Windows Shell AppID CRC64-ECMA-182 hash used as the 16-hex-digit filename
/// prefix under `%APPDATA%\Microsoft\Windows\Recent\CustomDestinations\<hash>.customDestinations-ms`.
pub fn windows_appid_crc64(input: &str) -> u64 {
    const POLY: u64 = 0x92C6_4265_D321_39A4;
    static TABLE: OnceLock<[u64; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u64; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u64;
            for _ in 0..8 {
                if (c & 1) != 0 {
                    c = (c >> 1) ^ POLY;
                } else {
                    c >>= 1;
                }
            }
            *slot = c;
        }
        t
    });

    let upper = input.trim().to_uppercase();
    let mut hash: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    for unit in upper.encode_utf16() {
        for b in unit.to_le_bytes() {
            let idx = ((hash ^ u64::from(b)) & 0xFF) as usize;
            hash = (hash >> 8) ^ table[idx];
        }
    }
    hash
}

fn known_folder_substituted_exe_path(exe_path: &str) -> Option<String> {
    let norm = exe_path.trim().trim_matches('"').replace('/', "\\");
    if norm.is_empty() {
        return None;
    }
    let norm_low = norm.to_ascii_lowercase();
    let mut mappings: Vec<(String, &'static str)> = Vec::with_capacity(8);
    if let Some(local) = dirs::data_local_dir() {
        mappings.push((
            local.to_string_lossy().replace('/', "\\"),
            "{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}",
        ));
    }
    if let Some(roaming) = dirs::config_dir() {
        mappings.push((
            roaming.to_string_lossy().replace('/', "\\"),
            "{3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}",
        ));
    }
    mappings.push((
        r"C:\Program Files (x86)".to_string(),
        "{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}",
    ));
    mappings.push((
        r"C:\Program Files".to_string(),
        "{6D809377-6AF0-444B-8957-A3773F02200E}",
    ));
    mappings.push((
        r"C:\Windows\System32".to_string(),
        "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}",
    ));
    mappings.push((
        r"C:\Windows".to_string(),
        "{F38BF404-1D43-42F2-9305-67DE0B28FC23}",
    ));
    // Prefer longest prefix match first (e.g. `System32` before `Windows`, `Program Files (x86)` before `Program Files`).
    mappings.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (prefix, guid) in mappings {
        let p_low = prefix.trim_end_matches('\\').to_ascii_lowercase();
        if p_low.is_empty() {
            continue;
        }
        if norm_low.starts_with(&p_low) {
            let rest = &norm[p_low.len()..];
            if rest.starts_with('\\') {
                return Some(format!("{guid}{rest}"));
            }
        }
    }
    None
}

/// Builds the O(1) candidate Windows AppID strings whose CRC64 hashes name
/// `%APPDATA%\Microsoft\Windows\Recent\CustomDestinations\<16hex>.customDestinations-ms`.
pub fn candidate_jump_list_app_ids(exe_path: &str, aumid: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |cand: &str| {
        let t = cand.trim();
        if !t.is_empty() && seen.insert(t.to_ascii_uppercase()) {
            out.push(t.to_string());
        }
    };

    if let Some(raw_aumid) = aumid {
        let trimmed = raw_aumid.trim();
        let stripped = if trimmed
            .to_ascii_lowercase()
            .starts_with("shell:appsfolder\\")
        {
            &trimmed["shell:appsfolder\\".len()..]
        } else {
            trimmed
        };
        push(stripped);
    }

    let norm_exe = normalize_jump_list_exe_path(exe_path);
    let base = normalized_exe_basename(&norm_exe);
    match base.as_str() {
        "steam.exe" | "steamwebhelper.exe" => push("Valve.Steam.Client"),
        "telegram.exe" => push("Telegram.TelegramDesktop"),
        "discord.exe" => push("com.squirrel.Discord.Discord"),
        "code.exe" => push("Microsoft.VisualStudioCode"),
        "cursor.exe" => push("Cursor"),
        "spotify.exe" => push("SpotifyAB.SpotifyMusic"),
        "explorer.exe" => push("Microsoft.Windows.Explorer"),
        _ => {}
    }

    if let Some(kf) = known_folder_substituted_exe_path(&norm_exe) {
        push(&kf);
    }
    if !norm_exe.is_empty() {
        push(&norm_exe);
    }
    out
}

fn read_u16_le(data: &[u8], off: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(off..off + 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(off..off + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn decode_utf16le_bytes(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let u = u16::from_le_bytes([chunk[0], chunk[1]]);
        if u == 0 {
            break;
        }
        units.push(u);
    }
    String::from_utf16_lossy(&units).trim().to_string()
}

fn decode_cstr_lossy(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

fn read_lnk_string_data(data: &[u8], pos: &mut usize, is_unicode: bool) -> Option<String> {
    let char_count = read_u16_le(data, *pos)? as usize;
    *pos += 2;
    let byte_len = if is_unicode {
        char_count.checked_mul(2)?
    } else {
        char_count
    };
    let slice = data.get(*pos..(*pos).checked_add(byte_len)?)?;
    *pos += byte_len;
    if is_unicode {
        Some(decode_utf16le_bytes(slice))
    } else {
        Some(decode_cstr_lossy(slice))
    }
}

fn parse_property_store_block(block: &[u8]) -> (Option<String>, bool) {
    // FMTID_SummaryInformation {F29F85E0-4FF9-1068-AB91-08002B27B3D9}, PID 2 = PKEY_Title
    const FMTID_SUMMARY_INFO: [u8; 16] = [
        0xE0, 0x85, 0x9F, 0xF2, 0xF9, 0x4F, 0x68, 0x10, 0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3,
        0xD9,
    ];
    // FMTID_AppUserModel {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, PID 6 = IsDestListSeparator
    const FMTID_APP_USER_MODEL: [u8; 16] = [
        0x55, 0x28, 0x4C, 0x9F, 0x79, 0x9F, 0x39, 0x4B, 0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5,
        0xF3,
    ];
    const SPS_VERSION: u32 = 0x5350_5331; // "1SPS"

    let mut title: Option<String> = None;
    let mut is_separator = false;
    let mut pos = 0usize;

    while pos + 4 <= block.len() {
        let Some(storage_size) = read_u32_le(block, pos) else {
            break;
        };
        if storage_size == 0 {
            break;
        }
        let storage_end = pos.saturating_add(storage_size as usize).min(block.len());
        if storage_size < 24 || storage_end <= pos + 24 {
            break;
        }
        let version = read_u32_le(block, pos + 4).unwrap_or(0);
        let fmtid = &block[pos + 8..pos + 24];
        if version == SPS_VERSION
            && (fmtid == FMTID_SUMMARY_INFO || fmtid == FMTID_APP_USER_MODEL)
        {
            let mut p = pos + 24;
            while p + 4 <= storage_end {
                let Some(val_size) = read_u32_le(block, p) else {
                    break;
                };
                if val_size == 0 {
                    break;
                }
                let val_end = p.saturating_add(val_size as usize).min(storage_end);
                if val_size < 13 || val_end <= p + 13 {
                    break;
                }
                let pid = read_u32_le(block, p + 4).unwrap_or(0);
                // Byte at p+8 is reserved (0); typed property value starts at p+9: [vt:u16, pad:u16, data...]
                let vt = read_u16_le(block, p + 9).unwrap_or(0);
                let val_data_start = p + 13;

                if fmtid == FMTID_SUMMARY_INFO && pid == 2 {
                    if vt == 31 && val_data_start + 4 <= val_end {
                        // VT_LPWSTR
                        let chars = read_u32_le(block, val_data_start).unwrap_or(0) as usize;
                        let str_start = val_data_start + 4;
                        let str_end = str_start
                            .saturating_add(chars.saturating_mul(2))
                            .min(val_end);
                        let s = decode_utf16le_bytes(&block[str_start..str_end]);
                        if !s.is_empty() && !(s.starts_with('@') && s.contains(",-")) {
                            title = Some(s);
                        }
                    } else if vt == 30 && val_data_start + 4 <= val_end {
                        // VT_LPSTR
                        let len = read_u32_le(block, val_data_start).unwrap_or(0) as usize;
                        let str_start = val_data_start + 4;
                        let str_end = str_start.saturating_add(len).min(val_end);
                        let s = decode_cstr_lossy(&block[str_start..str_end]);
                        if !s.is_empty() && !(s.starts_with('@') && s.contains(",-")) {
                            title = Some(s);
                        }
                    }
                } else if fmtid == FMTID_APP_USER_MODEL
                    && pid == 6
                    && vt == 11
                    && val_data_start + 2 <= val_end
                {
                    // VT_BOOL: non-zero (typically 0xFFFF) means separator entry
                    if read_u16_le(block, val_data_start).unwrap_or(0) != 0 {
                        is_separator = true;
                    }
                }

                p = val_end;
            }
        }
        pos = storage_end;
    }

    (title, is_separator)
}

/// Parses a single MS-SHLLINK payload starting at `*pos`, advancing `*pos` to the exact byte
/// immediately after its `TerminalBlock`. Returns `Ok(Some(item))` for valid entries,
/// `Ok(None)` for Jump List separator entries, or `Err(())` if malformed.
fn parse_embedded_shell_link(
    data: &[u8],
    pos: &mut usize,
    kind: &str,
    fallback_exe: &str,
) -> Result<Option<JumpListItem>, ()> {
    const SHLLINK_HEADER_MAGIC: [u8; 20] = [
        0x4C, 0x00, 0x00, 0x00, 0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x46,
    ];
    let start = *pos;
    if data.get(start..start + 76).is_none() || data[start..start + 20] != SHLLINK_HEADER_MAGIC {
        return Err(());
    }
    let flags = read_u32_le(data, start + 20).ok_or(())?;
    let is_unicode = (flags & 0x0080) != 0;
    let mut p = start + 76;

    // 1. LinkTargetIDList
    if (flags & 0x0001) != 0 {
        let idlist_size = read_u16_le(data, p).ok_or(())? as usize;
        p = p.checked_add(2).and_then(|v| v.checked_add(idlist_size)).ok_or(())?;
        if p > data.len() {
            return Err(());
        }
    }

    // 2. LinkInfo (extract LocalBasePath + CommonPathSuffix)
    let mut link_info_path = String::new();
    if (flags & 0x0002) != 0 {
        let li_start = p;
        let li_size = read_u32_le(data, li_start).ok_or(())? as usize;
        let li_end = li_start.checked_add(li_size).ok_or(())?;
        if li_size < 28 || li_end > data.len() {
            return Err(());
        }
        let li_hdr_size = read_u32_le(data, li_start + 4).unwrap_or(0) as usize;
        let li_flags = read_u32_le(data, li_start + 8).unwrap_or(0);
        if (li_flags & 0x01) != 0 {
            let (base_path, suffix) = if li_hdr_size >= 0x24 && li_start + 36 <= li_end {
                let u_off = read_u32_le(data, li_start + 28).unwrap_or(0) as usize;
                let s_off = read_u32_le(data, li_start + 32).unwrap_or(0) as usize;
                let bp = if u_off > 0 && li_start + u_off < li_end {
                    decode_utf16le_bytes(&data[li_start + u_off..li_end])
                } else {
                    String::new()
                };
                let sf = if s_off > 0 && li_start + s_off < li_end {
                    decode_utf16le_bytes(&data[li_start + s_off..li_end])
                } else {
                    String::new()
                };
                (bp, sf)
            } else {
                let a_off = read_u32_le(data, li_start + 16).unwrap_or(0) as usize;
                let s_off = read_u32_le(data, li_start + 24).unwrap_or(0) as usize;
                let bp = if a_off > 0 && li_start + a_off < li_end {
                    decode_cstr_lossy(&data[li_start + a_off..li_end])
                } else {
                    String::new()
                };
                let sf = if s_off > 0 && li_start + s_off < li_end {
                    decode_cstr_lossy(&data[li_start + s_off..li_end])
                } else {
                    String::new()
                };
                (bp, sf)
            };
            link_info_path = if suffix.is_empty() {
                base_path
            } else if base_path.ends_with('\\') || suffix.starts_with('\\') {
                format!("{base_path}{suffix}")
            } else {
                format!("{base_path}\\{suffix}")
            };
        }
        p = li_end;
    }

    // 3. StringData
    let name_str = if (flags & 0x0004) != 0 {
        read_lnk_string_data(data, &mut p, is_unicode).ok_or(())?
    } else {
        String::new()
    };
    let rel_path = if (flags & 0x0008) != 0 {
        read_lnk_string_data(data, &mut p, is_unicode).ok_or(())?
    } else {
        String::new()
    };
    if (flags & 0x0010) != 0 {
        let _working_dir = read_lnk_string_data(data, &mut p, is_unicode).ok_or(())?;
    }
    let args_str = if (flags & 0x0020) != 0 {
        read_lnk_string_data(data, &mut p, is_unicode).ok_or(())?
    } else {
        String::new()
    };
    if (flags & 0x0040) != 0 {
        let _icon_loc = read_lnk_string_data(data, &mut p, is_unicode).ok_or(())?;
    }

    // 4. ExtraData blocks up to TerminalBlock (< 4)
    let mut prop_title: Option<String> = None;
    let mut env_target: Option<String> = None;
    let mut is_separator = false;

    while p + 4 <= data.len() {
        let block_size = read_u32_le(data, p).ok_or(())? as usize;
        if block_size < 4 {
            p += 4;
            break;
        }
        let block_end = p.checked_add(block_size).ok_or(())?;
        if block_end > data.len() {
            return Err(());
        }
        if block_size >= 8 {
            let sig = read_u32_le(data, p + 4).unwrap_or(0);
            if sig == 0xA000_0009 {
                let (t, sep) = parse_property_store_block(&data[p + 8..block_end]);
                if t.is_some() {
                    prop_title = t;
                }
                if sep {
                    is_separator = true;
                }
            } else if sig == 0xA000_0001 && block_size >= 788 {
                // EnvironmentVariableDataBlock: 260-char UTF-16LE TargetUnicode at offset 268
                let expanded = expand_common_env_vars(&decode_utf16le_bytes(&data[p + 268..p + 788]));
                if !expanded.is_empty() {
                    env_target = Some(expanded);
                }
            }
        }
        p = block_end;
    }

    *pos = p;
    if is_separator {
        return Ok(None);
    }

    let target = if !link_info_path.trim().is_empty() {
        link_info_path.trim().to_string()
    } else if let Some(et) = env_target.filter(|s| !s.trim().is_empty()) {
        et
    } else if !rel_path.trim().is_empty() && !rel_path.trim().starts_with('.') {
        rel_path.trim().to_string()
    } else if !args_str.trim().is_empty() && !fallback_exe.trim().is_empty() {
        fallback_exe.trim().to_string()
    } else {
        return Ok(None);
    };

    let args = if args_str.trim().is_empty() {
        None
    } else {
        Some(args_str.trim().to_string())
    };

    let title = prop_title
        .or_else(|| {
            let n = name_str.trim();
            if !n.is_empty() && !(n.starts_with('@') && n.contains(",-")) {
                Some(n.to_string())
            } else {
                None
            }
        })
        .or_else(|| {
            Path::new(&target)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| target.clone());

    Ok(Some(JumpListItem {
        title,
        target,
        args,
        kind: kind.to_string(),
    }))
}

/// Pure-Rust bounds-checked parser for Windows `.customDestinations-ms` Jump List containers.
/// Parses both Custom categories (e.g. Steam's recent games `Недавние`) and Tasks categories
/// (e.g. Store, Library, Friends, Settings, Big Picture, Exit Steam) in `< 1ms` with zero COM overhead.
pub fn parse_custom_destinations_bytes(data: &[u8], fallback_exe: &str) -> Vec<JumpListItem> {
    const CLSID_SHELL_LINK: [u8; 16] = [
        0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x46,
    ];
    const CATEGORY_FOOTER: u32 = 0xBABF_FBAB; // little-endian bytes: AB FB BF BA

    if data.len() < 12 {
        return Vec::new();
    }
    let Some(cat_count) = read_u32_le(data, 4) else {
        return Vec::new();
    };
    let cat_count = (cat_count as usize).min(32);
    let mut pos = 12usize;
    let mut out = Vec::new();

    for _ in 0..cat_count {
        let Some(cat_type) = read_u32_le(data, pos) else {
            break;
        };
        pos += 4;

        let (item_count, kind): (usize, &'static str) = match cat_type {
            0 => {
                // Custom named category
                let Some(title_chars) = read_u16_le(data, pos).map(|v| v as usize) else {
                    break;
                };
                pos += 2;
                let Some(title_end) = pos.checked_add(title_chars.saturating_mul(2)) else {
                    break;
                };
                if title_end > data.len() {
                    break;
                }
                let cat_title = decode_utf16le_bytes(&data[pos..title_end]).to_lowercase();
                pos = title_end;
                let Some(count) = read_u32_le(data, pos).map(|v| v as usize) else {
                    break;
                };
                pos += 4;
                let k = if cat_title.contains("task") || cat_title.contains("задач") {
                    "task"
                } else {
                    "recent"
                };
                (count.min(64), k)
            }
            1 => {
                // Known category (Frequent / Recent stored in AutomaticDestinations): 4-byte ID, no inline items
                if pos + 4 > data.len() {
                    break;
                }
                pos += 4;
                (0, "recent")
            }
            2 => {
                // Tasks category
                let Some(count) = read_u32_le(data, pos).map(|v| v as usize) else {
                    break;
                };
                pos += 4;
                (count.min(64), "task")
            }
            _ => break,
        };

        for _ in 0..item_count {
            if data.get(pos..pos + 16) != Some(&CLSID_SHELL_LINK[..]) {
                break;
            }
            pos += 16;
            match parse_embedded_shell_link(data, &mut pos, kind, fallback_exe) {
                Ok(Some(item)) => out.push(item),
                Ok(None) => {}
                Err(()) => break,
            }
        }

        if read_u32_le(data, pos) == Some(CATEGORY_FOOTER) {
            pos += 4;
        }
    }

    out
}

/// Performs an O(1) direct filename lookup in `%APPDATA%\Microsoft\Windows\Recent\CustomDestinations`
/// using exact Windows AppID CRC64 hashes (NEVER linearly scanning the directory).
fn load_native_custom_destinations(exe_path: &str, aumid: Option<&str>) -> Vec<JumpListItem> {
    let Some(custom_dir) =
        dirs::config_dir().map(|d| d.join(r"Microsoft\Windows\Recent\CustomDestinations"))
    else {
        return Vec::new();
    };
    let norm_exe = normalize_jump_list_exe_path(exe_path);
    for cand in candidate_jump_list_app_ids(&norm_exe, aumid) {
        let hash = windows_appid_crc64(&cand);
        let path = custom_dir.join(format!("{hash:016x}.customDestinations-ms"));
        if !path.is_file() {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.len() == 0 || meta.len() > 1024 * 1024 {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            let parsed = parse_custom_destinations_bytes(&bytes, &norm_exe);
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }
    Vec::new()
}

/// Returns built-in app-specific quick Jump List tasks for common browsers, editors,
/// terminals, Explorer, Notepad, OBS, and Steam.
pub fn known_app_quick_tasks(exe_path: &str, aumid: Option<&str>) -> Vec<JumpListItem> {
    let normalized = normalize_jump_list_exe_path(exe_path);
    let exe_clean = normalized.trim().trim_matches('"');
    let base = normalized_exe_basename(exe_clean);
    let aumid_low = aumid.unwrap_or("").trim().to_ascii_lowercase();
    let mut tasks = Vec::new();

    let task = |title: &str, target: &str, args: Option<&str>| JumpListItem {
        title: title.to_string(),
        target: target.to_string(),
        args: args.map(str::to_string),
        kind: "task".to_string(),
    };

    if matches!(base.as_str(), "zen.exe" | "firefox.exe") && !exe_clean.is_empty() {
        tasks.push(task("Приватное окно", exe_clean, Some("-private-window")));
    } else if matches!(
        base.as_str(),
        "chrome.exe" | "brave.exe" | "browser.exe" | "vivaldi.exe" | "opera.exe"
    ) && !exe_clean.is_empty()
    {
        tasks.push(task("Окно инкогнито", exe_clean, Some("--incognito")));
    } else if base == "msedge.exe" && !exe_clean.is_empty() {
        tasks.push(task("Окно InPrivate", exe_clean, Some("--inprivate")));
    } else if matches!(base.as_str(), "code.exe" | "cursor.exe" | "antigravity.exe")
        && !exe_clean.is_empty()
    {
        tasks.push(task("Новое пустое окно", exe_clean, Some("--new-window")));
    } else if matches!(base.as_str(), "wt.exe" | "windowsterminal.exe")
        || aumid_low.contains("windowsterminal")
    {
        tasks.push(task("Новая вкладка PowerShell", "wt.exe", Some("new-tab powershell")));
        tasks.push(task("Новая вкладка CMD", "wt.exe", Some("new-tab cmd")));
    } else if base == "notepad.exe" || aumid_low.contains("windowsnotepad") {
        let target = if !exe_clean.is_empty() && !is_application_frame_host(exe_clean) {
            exe_clean
        } else {
            "notepad.exe"
        };
        tasks.push(task("Новый документ", target, None));
    } else if base == "explorer.exe" {
        if let Some(dl) = dirs::download_dir().filter(|p| p.exists()) {
            tasks.push(task("Загрузки", &dl.to_string_lossy(), None));
        }
        if let Some(docs) = dirs::document_dir().filter(|p| p.exists()) {
            tasks.push(task("Документы", &docs.to_string_lossy(), None));
        }
        if let Some(desk) = dirs::desktop_dir().filter(|p| p.exists()) {
            tasks.push(task("Рабочий стол", &desk.to_string_lossy(), None));
        }
    } else if base == "obs64.exe" && !exe_clean.is_empty() {
        tasks.push(task("Запустить в трее", exe_clean, Some("--minimize-to-tray")));
    } else if base == "steam.exe" {
        tasks.push(task("Библиотека игр", "steam://open/games", None));
        tasks.push(task("Список друзей", "steam://open/friends", None));
    }

    tasks
}

fn recent_extensions_for_app(base: &str, aumid_low: &str) -> (&'static [&'static str], bool) {
    match base {
        "explorer.exe" => (&[], true),
        "code.exe" | "cursor.exe" | "antigravity.exe" => (
            &[
                "rs", "ts", "tsx", "js", "jsx", "json", "toml", "md", "py", "cs", "cpp", "h",
                "html", "css", "yaml", "yml", "code-workspace", "sql",
            ],
            true,
        ),
        "notepad.exe" | "notepad++.exe" => (
            &["txt", "md", "log", "json", "ini", "cfg", "xml", "csv", "toml", "env"],
            false,
        ),
        "obsidian.exe" => (&["md"], false),
        "photoshop.exe" | "lightroom.exe" | "mspaint.exe" | "photos.exe" | "photosapp.exe" => (
            &[
                "psd", "psb", "png", "jpg", "jpeg", "webp", "tif", "tiff", "bmp", "svg", "dng",
                "raw",
            ],
            false,
        ),
        "zen.exe" | "firefox.exe" | "chrome.exe" | "msedge.exe" | "browser.exe" | "brave.exe" => {
            (&["html", "htm", "pdf", "svg", "mhtml"], false)
        }
        "winword.exe" => (&["doc", "docx", "rtf", "odt", "pdf"], false),
        "excel.exe" => (&["xls", "xlsx", "csv", "ods"], false),
        "powerpnt.exe" => (&["ppt", "pptx"], false),
        "vlc.exe" | "mpv.exe" | "obs64.exe" => {
            (&["mp4", "mkv", "webm", "mov", "avi", "mp3", "flac", "wav"], false)
        }
        _ if aumid_low.contains("windowsnotepad") => {
            (&["txt", "md", "log", "json", "ini", "csv"], false)
        }
        _ if aumid_low.contains("photos") || aumid_low.contains("paint") => {
            (&["png", "jpg", "jpeg", "webp", "bmp", "svg"], false)
        }
        _ => (&[], false),
    }
}

fn looks_like_directory_path(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.ends_with('\\') || trimmed.ends_with('/') {
        return true;
    }
    let p = Path::new(trimmed);
    if p.is_dir() {
        return true;
    }
    p.extension().is_none()
}

/// Pure matcher selecting up to `limit` recent Windows `.lnk` entries relevant to the
/// target app (`exe_path` / `aumid`), deduplicating targets case-insensitively.
pub fn match_recent_entries_for_app(
    exe_path: &str,
    aumid: Option<&str>,
    recent_entries: &[(String, String)],
    limit: usize,
) -> Vec<JumpListItem> {
    if limit == 0 || recent_entries.is_empty() {
        return Vec::new();
    }
    let base = normalized_exe_basename(exe_path);
    let aumid_low = aumid.unwrap_or("").trim().to_ascii_lowercase();
    let (exts, allow_dirs) = recent_extensions_for_app(&base, &aumid_low);
    if exts.is_empty() && !allow_dirs {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut seen_targets = HashSet::new();
    let self_norm = exe_path.trim().replace('/', "\\").to_ascii_lowercase();

    for (raw_title, raw_target) in recent_entries {
        let target = raw_target.trim();
        if target.is_empty() {
            continue;
        }
        let target_norm = target.replace('/', "\\").to_ascii_lowercase();
        if target_norm == self_norm || target_norm.ends_with(".exe") {
            continue;
        }

        let ext = Path::new(target)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        let matches = match ext.as_deref() {
            Some(e) => exts.contains(&e),
            None => allow_dirs && looks_like_directory_path(target),
        };
        if !matches || !seen_targets.insert(target_norm) {
            continue;
        }

        let clean_title = raw_title
            .trim()
            .strip_suffix(".lnk")
            .or_else(|| raw_title.trim().strip_suffix(".LNK"))
            .unwrap_or(raw_title.trim());
        let title = if clean_title.is_empty() {
            Path::new(target)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| target.to_string())
        } else {
            clean_title.to_string()
        };

        out.push(JumpListItem {
            title,
            target: target.to_string(),
            args: None,
            kind: "recent".to_string(),
        });
        if out.len() >= limit {
            break;
        }
    }

    out
}

fn resolve_recent_lnk_target_with(
    link: &IShellLinkW,
    pf: &IPersistFile,
    lnk_path: &Path,
) -> Option<String> {
    unsafe {
        let w = wide(&lnk_path.to_string_lossy());
        pf.Load(PCWSTR(w.as_ptr()), STGM(0)).ok()?;
        let mut buf = [0u16; 1024];
        let mut fd = std::mem::zeroed();
        link.GetPath(&mut buf, &mut fd, 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let resolved = String::from_utf16_lossy(&buf[..len]).trim().to_string();
        if resolved.is_empty() {
            None
        } else {
            Some(resolved)
        }
    }
}

fn collect_windows_recent_targets(max_scan: usize) -> Vec<(String, String)> {
    let Some(recent_dir) = dirs::config_dir().map(|d| d.join(r"Microsoft\Windows\Recent")) else {
        return Vec::new();
    };
    let Ok(read_dir) = std::fs::read_dir(&recent_dir) else {
        return Vec::new();
    };

    let mut lnk_files: Vec<(std::time::SystemTime, PathBuf)> = read_dir
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_lnk = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("lnk"))
                .unwrap_or(false);
            if !is_lnk {
                return None;
            }
            let modified = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((modified, path))
        })
        .collect();

    // Newest first
    lnk_files.sort_by(|a, b| b.0.cmp(&a.0));

    let com = ComGuard::init_sta();
    if !com.is_usable() {
        return Vec::new();
    }

    // Reuse a single IShellLinkW + IPersistFile instance across the entire Recent directory scan.
    let (link, pf) = unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER)
        else {
            return Vec::new();
        };
        let Ok(pf) = link.cast::<IPersistFile>() else {
            return Vec::new();
        };
        (link, pf)
    };

    let mut resolved = Vec::new();
    for (_, lnk_path) in lnk_files.into_iter().take(max_scan) {
        let Some(target) = resolve_recent_lnk_target_with(&link, &pf, &lnk_path) else {
            continue;
        };
        if !Path::new(&target).exists() {
            continue;
        }
        let title = lnk_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| target.clone());
        resolved.push((title, target));
    }
    resolved
}

/// Returns structured Jump List actions, preferring O(1) native Windows `.customDestinations-ms`
/// Jump Lists (e.g. Steam's recent games & native tasks via AppID CRC64 direct lookup) and falling
/// back to app-specific quick tasks + matching Windows Recent `.lnk` items.
pub fn get_jump_list(exe_path: &str, aumid: Option<&str>) -> Vec<JumpListItem> {
    let norm_exe = normalize_jump_list_exe_path(exe_path);
    let native = load_native_custom_destinations(&norm_exe, aumid);
    if !native.is_empty() {
        let mut seen = HashSet::new();
        let mut recents = Vec::new();
        let mut tasks = Vec::new();
        for item in native {
            let key = format!(
                "{}|{}|{}",
                item.title.to_ascii_lowercase(),
                item.target.to_ascii_lowercase(),
                item.args.as_deref().unwrap_or("").to_ascii_lowercase()
            );
            if !seen.insert(key) {
                continue;
            }
            if item.kind == "task" {
                if tasks.len() < 8 {
                    tasks.push(item);
                }
            } else if recents.len() < 4 {
                recents.push(item);
            }
        }
        let mut combined = Vec::with_capacity(tasks.len() + recents.len());
        combined.extend(tasks);
        combined.extend(recents);
        if !combined.is_empty() {
            return combined;
        }
    }

    let mut items = known_app_quick_tasks(&norm_exe, aumid);
    let base = normalized_exe_basename(&norm_exe);
    let aumid_low = aumid.unwrap_or("").trim().to_ascii_lowercase();
    let (exts, allow_dirs) = recent_extensions_for_app(&base, &aumid_low);
    if !exts.is_empty() || allow_dirs {
        let recent_raw = collect_windows_recent_targets(48);
        let recent_matched = match_recent_entries_for_app(&norm_exe, aumid, &recent_raw, 4);
        let mut seen: HashSet<String> = items
            .iter()
            .map(|i| {
                format!(
                    "{}|{}",
                    i.target.to_ascii_lowercase(),
                    i.args.as_deref().unwrap_or("")
                )
            })
            .collect();
        for r in recent_matched {
            let key = format!("{}|", r.target.to_ascii_lowercase());
            if seen.insert(key) {
                items.push(r);
            }
        }
    }
    items.truncate(6);
    items
}

/// Computes exact aspect-preserving thumbnail dimensions fitting cleanly inside `(max_w, max_h)`
/// without upscaling past `1.0x` or distorting narrow/tall windows (e.g. Steam Friends).
pub fn compute_aspect_preserving_thumb_size(
    crop_w: i32,
    crop_h: i32,
    max_w: i32,
    max_h: i32,
) -> (i32, i32) {
    let cw = crop_w.max(1) as f64;
    let ch = crop_h.max(1) as f64;
    let mw = max_w.max(1) as f64;
    let mh = max_h.max(1) as f64;
    let scale = (mw / cw).min(mh / ch).min(1.0);
    let dst_w = ((cw * scale).round() as i32).clamp(1, max_w.max(1));
    let dst_h = ((ch * scale).round() as i32).clamp(1, max_h.max(1));
    (dst_w, dst_h)
}

/// High-quality 2D area-averaging box downsample converting a cropped top-down 32bpp BGRA region
/// into crisp opaque RGBA pixels (eliminates GDI `StretchBlt(HALFTONE)` aliasing/blur at 4x–8x
/// downscale factors and avoids a second GDI DC/bitmap allocation).
#[allow(clippy::too_many_arguments)]
pub fn downsample_bgra_region_to_rgba(
    bgra: &[u8],
    src_w: usize,
    src_h: usize,
    crop_x: usize,
    crop_y: usize,
    crop_w: usize,
    crop_h: usize,
    dst_w: usize,
    dst_h: usize,
) -> Vec<u8> {
    if src_w == 0 || src_h == 0 || crop_w == 0 || crop_h == 0 || dst_w == 0 || dst_h == 0 {
        return Vec::new();
    }
    let expected = src_w.saturating_mul(src_h).saturating_mul(4);
    if bgra.len() < expected || crop_x + crop_w > src_w || crop_y + crop_h > src_h {
        return Vec::new();
    }

    let mut rgba = vec![0u8; dst_w * dst_h * 4];
    let scale_x = crop_w as f64 / dst_w as f64;
    let scale_y = crop_h as f64 / dst_h as f64;

    // Precompute horizontal span weights once per column for O(src_pixels) crisp box filtering.
    let mut x_spans: Vec<(usize, usize, f64, f64)> = Vec::with_capacity(dst_w);
    for dx in 0..dst_w {
        let x0 = crop_x as f64 + dx as f64 * scale_x;
        let x1 = (crop_x as f64 + (dx + 1) as f64 * scale_x).min((crop_x + crop_w) as f64);
        let ix0 = (x0.floor() as usize).min(src_w - 1);
        let ix1 = ((x1.ceil() as usize).saturating_sub(1)).clamp(ix0, src_w - 1);
        x_spans.push((ix0, ix1, x0, x1));
    }

    for dy in 0..dst_h {
        let y0 = crop_y as f64 + dy as f64 * scale_y;
        let y1 = (crop_y as f64 + (dy + 1) as f64 * scale_y).min((crop_y + crop_h) as f64);
        let iy0 = (y0.floor() as usize).min(src_h - 1);
        let iy1 = ((y1.ceil() as usize).saturating_sub(1)).clamp(iy0, src_h - 1);

        for (dx, &(ix0, ix1, x0, x1)) in x_spans.iter().enumerate() {
            let mut sum_r = 0.0f64;
            let mut sum_g = 0.0f64;
            let mut sum_b = 0.0f64;
            let mut sum_w = 0.0f64;

            for sy in iy0..=iy1 {
                let wy = ((sy + 1) as f64).min(y1) - (sy as f64).max(y0);
                if wy <= 0.0 {
                    continue;
                }
                let row_off = sy * src_w * 4;
                for sx in ix0..=ix1 {
                    let wx = ((sx + 1) as f64).min(x1) - (sx as f64).max(x0);
                    if wx <= 0.0 {
                        continue;
                    }
                    let w = wx * wy;
                    let idx = row_off + sx * 4;
                    sum_b += bgra[idx] as f64 * w;
                    sum_g += bgra[idx + 1] as f64 * w;
                    sum_r += bgra[idx + 2] as f64 * w;
                    sum_w += w;
                }
            }

            let out_idx = (dy * dst_w + dx) * 4;
            if sum_w > 0.0 {
                let inv = 1.0 / sum_w;
                rgba[out_idx] = (sum_r * inv).round().clamp(0.0, 255.0) as u8;
                rgba[out_idx + 1] = (sum_g * inv).round().clamp(0.0, 255.0) as u8;
                rgba[out_idx + 2] = (sum_b * inv).round().clamp(0.0, 255.0) as u8;
            }
            rgba[out_idx + 3] = 255;
        }
    }

    rgba
}

/// Computes the visible source crop `(crop_x, crop_y, crop_w, crop_h)` inside `win_rect`
/// using `DWMWA_EXTENDED_FRAME_BOUNDS` so invisible Windows 11 DWM drop-shadow borders are
/// trimmed cleanly off hover preview thumbnails.
pub fn compute_dwm_thumbnail_crop(win_rect: RECT, ext_rect: Option<RECT>) -> (i32, i32, i32, i32) {
    let src_w = (win_rect.right - win_rect.left).max(1);
    let src_h = (win_rect.bottom - win_rect.top).max(1);
    let Some(ext) = ext_rect else {
        return (0, 0, src_w, src_h);
    };
    if ext.right <= ext.left || ext.bottom <= ext.top {
        return (0, 0, src_w, src_h);
    }
    let crop_x = (ext.left - win_rect.left).clamp(0, src_w - 1);
    let crop_y = (ext.top - win_rect.top).clamp(0, src_h - 1);
    let crop_right = (ext.right - win_rect.left).clamp(crop_x + 1, src_w);
    let crop_bottom = (ext.bottom - win_rect.top).clamp(crop_y + 1, src_h);
    let crop_w = crop_right - crop_x;
    let crop_h = crop_bottom - crop_y;
    if crop_w < 16 || crop_h < 16 {
        return (0, 0, src_w, src_h);
    }
    (crop_x, crop_y, crop_w, crop_h)
}

/// Captures a crisp aspect-preserving live visual thumbnail of an open window using
/// `PrintWindow(PW_RENDERFULLCONTENT)` into a single top-down 32bpp DIBSection, trimming invisible
/// Win11 DWM shadow borders via `DWMWA_EXTENDED_FRAME_BOUNDS` and downsampling with exact 2D area
/// averaging. Every GDI handle is guarded by RAII cleanup.
pub fn capture_window_thumbnail(hwnd_raw: isize, max_w: i32, max_h: i32) -> Option<String> {
    if hwnd_raw == 0 || max_w <= 0 || max_h <= 0 {
        return None;
    }
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        if !IsWindow(hwnd).as_bool() || IsIconic(hwnd).as_bool() || IsHungAppWindow(hwnd).as_bool()
        {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        let src_w = (rect.right - rect.left).max(1);
        let src_h = (rect.bottom - rect.top).max(1);
        if src_w < 32 || src_h < 32 {
            return None;
        }

        let mut ext_rect = RECT::default();
        let ext_opt = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut ext_rect as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()
        .map(|()| ext_rect);
        let (crop_x, crop_y, crop_w, crop_h) = compute_dwm_thumbnail_crop(rect, ext_opt);
        let (dst_w, dst_h) = compute_aspect_preserving_thumb_size(crop_w, crop_h, max_w, max_h);

        let screen_dc = GetDC(HWND::default());
        if screen_dc.is_invalid() {
            return None;
        }
        let _screen_guard = WindowDcGuard {
            hwnd: HWND::default(),
            hdc: screen_dc,
        };

        let src_dc = CreateCompatibleDC(screen_dc);
        if src_dc.is_invalid() {
            return None;
        }
        let _src_dc_guard = MemDcGuard(src_dc);

        let mut bmi = BITMAPINFO::default();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = src_w;
        bmi.bmiHeader.biHeight = -src_h; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let src_bmp =
            CreateDIBSection(src_dc, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0).ok()?;
        if src_bmp.is_invalid() || bits_ptr.is_null() {
            if !src_bmp.is_invalid() {
                let _ = DeleteObject(src_bmp);
            }
            return None;
        }
        let _src_bmp_guard = HBitmapGuard(src_bmp);
        let old_src = SelectObject(src_dc, src_bmp);
        let _src_sel_guard = SelectedObjGuard {
            hdc: src_dc,
            prev: old_src,
        };

        // PW_RENDERFULLCONTENT = 2 captures DirectComposition/CEF/Chromium surfaces cleanly.
        let printed = windows::Win32::Storage::Xps::PrintWindow(
            hwnd,
            src_dc,
            windows::Win32::Storage::Xps::PRINT_WINDOW_FLAGS(2),
        )
        .as_bool();
        if !printed {
            let win_dc = GetDC(hwnd);
            if !win_dc.is_invalid() {
                let _win_dc_guard = WindowDcGuard { hwnd, hdc: win_dc };
                let _ = BitBlt(src_dc, 0, 0, src_w, src_h, win_dc, 0, 0, SRCCOPY);
            } else {
                return None;
            }
        }

        let src_total = (src_w as usize).checked_mul(src_h as usize)?.checked_mul(4)?;
        let bgra = std::slice::from_raw_parts(bits_ptr as *const u8, src_total);
        let rgba = downsample_bgra_region_to_rgba(
            bgra,
            src_w as usize,
            src_h as usize,
            crop_x as usize,
            crop_y as usize,
            crop_w as usize,
            crop_h as usize,
            dst_w as usize,
            dst_h as usize,
        );
        if rgba.is_empty() {
            return None;
        }
        encode_rgba_png(dst_w as u32, dst_h as u32, &rgba)
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
                is_topmost: false,
                monitor: MONITOR_PRIMARY.to_string(),
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
                is_topmost: false,
                monitor: MONITOR_PRIMARY.to_string(),
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
                is_topmost: true,
                monitor: MONITOR_PRIMARY.to_string(),
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
                is_topmost: false,
                monitor: MONITOR_PRIMARY.to_string(),
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
                is_topmost: false,
                monitor: MONITOR_PRIMARY.to_string(),
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

    #[test]
    fn format_explorer_select_raw_arg_handles_spaces_quotes_and_rejects_invalid() {
        // Spaces preserved inside exact unescaped /select,"..." raw arg
        assert_eq!(
            format_explorer_select_raw_arg(r"C:\Program Files\VS Code\Code.exe").unwrap(),
            r#"/select,"C:\Program Files\VS Code\Code.exe""#
        );
        // Forward slashes normalized to backslashes
        assert_eq!(
            format_explorer_select_raw_arg("C:/Users/Test User/Docs/file.txt").unwrap(),
            r#"/select,"C:\Users\Test User\Docs\file.txt""#
        );
        // Matching surrounding quotes stripped cleanly without double-quoting
        assert_eq!(
            format_explorer_select_raw_arg(r#"  "C:\Program Files\Zen\zen.exe"  "#).unwrap(),
            r#"/select,"C:\Program Files\Zen\zen.exe""#
        );

        // Rejects empty, whitespace, embedded quotes, control chars, and URI schemes
        assert!(format_explorer_select_raw_arg("").is_err());
        assert!(format_explorer_select_raw_arg("   ").is_err());
        assert!(format_explorer_select_raw_arg(r#""""#).is_err());
        assert!(format_explorer_select_raw_arg(r#"C:\Bad"Quote\app.exe"#).is_err());
        assert!(format_explorer_select_raw_arg("C:\\Bad\nPath\\app.exe").is_err());
        assert!(format_explorer_select_raw_arg("C:\\Bad\0Path\\app.exe").is_err());
        assert!(format_explorer_select_raw_arg("https://example.com").is_err());
        assert!(format_explorer_select_raw_arg("shell:AppsFolder\\Microsoft.Store!App").is_err());
        assert!(format_explorer_select_raw_arg("ms-settings:display").is_err());
    }

    #[test]
    fn normalize_launch_spec_uses_direct_shellexecute_verb_and_normalizes_args() {
        // Plain exe with as_admin = true uses native "runas" verb (never PowerShell quoting)
        let admin = normalize_launch_spec(r"C:\Program Files\App\O'Brien's App.exe", None, true)
            .expect("apostrophe path must normalize cleanly");
        assert_eq!(admin.verb, "runas");
        assert_eq!(admin.target, r"C:\Program Files\App\O'Brien's App.exe");
        assert_eq!(admin.args, None);

        // Quoted exe with inline args + explicit PinItem args merged cleanly
        let quoted = normalize_launch_spec(
            r#""C:\Program Files\VS Code\Code.exe" --new-window"#,
            Some("--disable-gpu"),
            false,
        )
        .expect("quoted exe with args must parse");
        assert_eq!(quoted.verb, "open");
        assert_eq!(quoted.target, r"C:\Program Files\VS Code\Code.exe");
        assert_eq!(quoted.args.as_deref(), Some("--new-window --disable-gpu"));

        // Blank args collapse to None
        let clean = normalize_launch_spec(r"C:\Windows\notepad.exe", Some("   "), false).unwrap();
        assert_eq!(clean.args, None);

        // Invalid inputs rejected
        assert!(normalize_launch_spec("", None, false).is_err());
        assert!(normalize_launch_spec("   ", None, true).is_err());
        assert!(normalize_launch_spec(r#""C:\Unclosed\app.exe"#, None, false).is_err());
        assert!(normalize_launch_spec("C:\\App\nEvil.exe", None, false).is_err());
        assert!(normalize_launch_spec(r"C:\App.exe", Some("--bad\0arg"), false).is_err());

        // Error messages are readable Russian strings
        assert!(format_shell_execute_error(2, "missing.exe").contains("не найден"));
        assert!(format_shell_execute_error(5, "admin.exe").contains("доступе"));
    }

    #[test]
    fn jump_list_quick_tasks_and_recent_matching_cover_browsers_editors_terminals_and_dedup() {
        // Browser private/incognito tasks
        let zen_tasks = known_app_quick_tasks(r"C:\Program Files\Zen Browser\zen.exe", None);
        assert_eq!(zen_tasks.len(), 1);
        assert_eq!(zen_tasks[0].kind, "task");
        assert_eq!(zen_tasks[0].args.as_deref(), Some("-private-window"));

        let chrome_tasks = known_app_quick_tasks(r"C:\Program Files\Google\Chrome\chrome.exe", None);
        assert_eq!(chrome_tasks[0].args.as_deref(), Some("--incognito"));

        let edge_tasks = known_app_quick_tasks(r"C:\Program Files\Edge\msedge.exe", None);
        assert_eq!(edge_tasks[0].args.as_deref(), Some("--inprivate"));

        // Windows Terminal via AUMID
        let wt_tasks = known_app_quick_tasks(
            r"C:\Windows\System32\ApplicationFrameHost.exe",
            Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"),
        );
        assert_eq!(wt_tasks.len(), 2);
        assert_eq!(wt_tasks[0].target, "wt.exe");

        // Recent matching for VS Code: keeps code files & folders, skips images/exe, deduplicates, respects limit
        let recent = vec![
            ("Cargo.toml.lnk".to_string(), r"C:\Projects\testbar\Cargo.toml".to_string()),
            ("duplicate.lnk".to_string(), r"c:/projects/testbar/cargo.toml".to_string()),
            ("photo.png.lnk".to_string(), r"C:\Users\loki2\Pictures\photo.png".to_string()),
            ("Code.exe.lnk".to_string(), r"C:\Program Files\VS Code\Code.exe".to_string()),
            ("testbar".to_string(), r"C:\Users\loki2\Documents\testbar".to_string()),
            ("lib.rs".to_string(), r"C:\Projects\testbar\src\lib.rs".to_string()),
            ("index.ts".to_string(), r"C:\Projects\testbar\src\index.ts".to_string()),
        ];

        let code_recent = match_recent_entries_for_app(
            r"C:\Program Files\VS Code\Code.exe",
            None,
            &recent,
            3,
        );
        assert_eq!(code_recent.len(), 3);
        assert_eq!(code_recent[0].title, "Cargo.toml");
        assert_eq!(code_recent[0].kind, "recent");
        assert_eq!(code_recent[0].target, r"C:\Projects\testbar\Cargo.toml");
        assert_eq!(code_recent[1].title, "testbar");
        assert_eq!(code_recent[2].title, "lib.rs");

        // Notepad matches text/toml, ignores folders and png
        let note_recent =
            match_recent_entries_for_app(r"C:\Windows\System32\notepad.exe", None, &recent, 4);
        assert_eq!(note_recent.len(), 1);
        assert_eq!(note_recent[0].title, "Cargo.toml");

        // Explorer matches directories only
        let exp_recent =
            match_recent_entries_for_app(r"C:\Windows\explorer.exe", None, &recent, 4);
        assert_eq!(exp_recent.len(), 1);
        assert_eq!(exp_recent[0].title, "testbar");
    }

    #[test]
    fn summarize_pid_termination_tolerates_already_exited_and_propagates_access_denied() {
        assert!(summarize_pid_termination_results(&[]).is_err());
        assert!(summarize_pid_termination_results(&[
            PidTerminateOutcome::Terminated,
            PidTerminateOutcome::AlreadyExited,
        ])
        .is_ok());

        let denied = summarize_pid_termination_results(&[
            PidTerminateOutcome::Terminated,
            PidTerminateOutcome::AccessDenied("Отказано в доступе".into()),
        ])
        .unwrap_err();
        assert!(denied.contains("Отказано в доступе"));

        let failed = summarize_pid_termination_results(&[PidTerminateOutcome::Failed(
            "OS error".into(),
        )])
        .unwrap_err();
        assert!(failed.contains("OS error"));
    }

    #[test]
    fn dwm_thumbnail_crop_strips_invisible_shadow_borders_and_clamps_safely() {
        // Typical Win11 window with 7px left/right/bottom invisible DWM resize shadow
        let win = RECT {
            left: 93,
            top: 100,
            right: 1107,
            bottom: 807,
        };
        let ext = RECT {
            left: 100,
            top: 100,
            right: 1100,
            bottom: 800,
        };
        assert_eq!(compute_dwm_thumbnail_crop(win, Some(ext)), (7, 0, 1000, 700));

        // Missing or invalid extended rect falls back to full window rect
        assert_eq!(compute_dwm_thumbnail_crop(win, None), (0, 0, 1014, 707));
        assert_eq!(
            compute_dwm_thumbnail_crop(
                win,
                Some(RECT {
                    left: 500,
                    top: 500,
                    right: 500,
                    bottom: 400,
                })
            ),
            (0, 0, 1014, 707)
        );
    }

    #[test]
    fn snap_rect_geometry_compensates_dwm_frame_insets_and_rejects_invalid_zone() {
        let wa = PixelRect::new(0, 0, 1920, 1016);
        let inset = (7, 0, 7, 7);

        let left = compute_snap_rect(wa, None, inset, "left").unwrap();
        assert_eq!(left, PixelRect::new(-7, 0, 960 + 7, 1016 + 7));

        let right = compute_snap_rect(wa, None, inset, "right").unwrap();
        assert_eq!(right, PixelRect::new(960 - 7, 0, 1920 + 7, 1016 + 7));

        // Center preserves comfortable current window size and centers it inside work_area
        let cur = PixelRect::new(50, 50, 1050, 750); // 1000x700
        let center_keep = compute_snap_rect(wa, Some(cur), inset, "center").unwrap();
        assert_eq!(
            center_keep,
            PixelRect::new(460 - 7, 158, 1460 + 7, 858 + 7)
        );

        // Oversized current window falls back to 72% x 78% centered
        let huge = PixelRect::new(0, 0, 1920, 1016);
        let center_default = compute_snap_rect(wa, Some(huge), (0, 0, 0, 0), "center").unwrap();
        assert_eq!(center_default.width(), 1382);
        assert_eq!(center_default.height(), 792);

        assert!(compute_snap_rect(wa, None, inset, "diagonal").is_err());
        assert!(snap_window(0, "left").is_err());
        assert!(snap_window(123, "bogus").is_err());
        assert!(toggle_window_topmost(0).is_err());
        assert!(minimize_other_windows(0).is_err());
    }

    #[test]
    fn build_new_window_launch_args_handles_chromium_gecko_editors_terminal_and_custom_args() {
        assert_eq!(
            build_new_window_launch_args(r"C:\Program Files\Google\Chrome\chrome.exe", None)
                .as_deref(),
            Some("--new-window")
        );
        assert_eq!(
            build_new_window_launch_args(r"C:\Program Files\VS Code\Code.exe", Some("--profile Dev"))
                .as_deref(),
            Some("--new-window --profile Dev")
        );
        // Existing new-window/incognito flag is not duplicated
        assert_eq!(
            build_new_window_launch_args(r"C:\Program Files\Google\Chrome\chrome.exe", Some("--incognito"))
                .as_deref(),
            Some("--incognito")
        );
        assert_eq!(
            build_new_window_launch_args(r"C:\Program Files\Zen Browser\zen.exe", None).as_deref(),
            Some("-new-window")
        );
        assert_eq!(
            build_new_window_launch_args(r"C:\Windows\explorer.exe", None).as_deref(),
            Some("/n")
        );
        assert_eq!(
            build_new_window_launch_args(r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\wt.exe", None)
                .as_deref(),
            Some("-w new")
        );
        assert_eq!(
            build_new_window_launch_args(r"C:\Windows\notepad.exe", None),
            None
        );
        assert!(launch_new_window_instance("   ", None).is_err());
    }

    #[test]
    fn open_window_info_is_topmost_serde_default_icon_source_and_negative_cache() {
        // Legacy JSON payload without isTopmost deserializes cleanly with is_topmost = false
        let raw = r#"{"hwnd":1,"pid":2,"title":"T","exePath":"C:\\Windows\\notepad.exe","exeName":"Блокнот","aumid":null,"isActive":false,"isMinimized":false}"#;
        let parsed: OpenWindowInfo = serde_json::from_str(raw).unwrap();
        assert!(!parsed.is_topmost);
        assert_eq!(
            icon_source_for_window(&parsed).as_deref(),
            Some(r"C:\Windows\notepad.exe")
        );

        let uwp_win = OpenWindowInfo {
            aumid: Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into()),
            exe_path: r"C:\Windows\System32\ApplicationFrameHost.exe".into(),
            ..Default::default()
        };
        assert_eq!(
            icon_source_for_window(&uwp_win).as_deref(),
            Some(r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App")
        );

        // Missing file path marks negative miss in IconCache so subsequent ticks short-circuit
        let cache = Mutex::new(IconCache::new(16));
        let bogus = r"C:\NonExistent_TestBar_Path_99999\missing_app.exe";
        assert!(extract_icon_for_path_cached(bogus, &cache).is_none());
        let key = icon_cache::path_icon_key(bogus);
        assert!(cache.lock().unwrap().is_negative_cached(&key));
        assert!(resolve_exe_or_hwnd_icon_key(bogus, None, &cache).is_none());
    }

    #[test]
    fn toggle_window_topmost_flips_flag_on_own_window() {
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_POPUP};
        let _z_lock = crate::WIN32_TOPMOST_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        unsafe {
            let cls = wide("STATIC");
            let hwnd = CreateWindowExW(
                Default::default(),
                PCWSTR(cls.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                100,
                100,
                HWND::default(),
                None,
                None,
                None,
            )
            .expect("CreateWindowExW STATIC failed");
            let raw = hwnd.0 as isize;
            let on = toggle_window_topmost(raw).expect("toggle topmost on failed");
            assert!(on);
            let off = toggle_window_topmost(raw).expect("toggle topmost off failed");
            assert!(!off);
            let _ = DestroyWindow(hwnd);
        }
    }

    fn decode_png_dimensions(data_uri: &str) -> (u32, u32) {
        let b64 = data_uri
            .strip_prefix("data:image/png;base64,")
            .expect("expected PNG data URI");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("valid base64");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().expect("valid PNG header");
        (reader.info().width, reader.info().height)
    }

    #[test]
    fn high_dpi_96px_shell_icon_extraction_bare_exe_and_squirrel_resolution() {
        // 1. Explorer extracts a true High-DPI (>= 64px, normally 96x96) PNG via IShellItemImageFactory
        let explorer_uri = extract_path_icon_raw(r"C:\Windows\explorer.exe")
            .expect("explorer.exe high-DPI icon must extract");
        let (w, h) = decode_png_dimensions(&explorer_uri);
        assert!(
            w >= 64 && h >= 64,
            "expected High-DPI icon >= 64x64 (96x96 IShellItemImageFactory), got {w}x{h}"
        );

        // 2. Forward-slash path normalizes cleanly to backslash for SHCreateItemFromParsingName
        let fwd_uri = extract_path_icon_raw("C:/Windows/explorer.exe")
            .expect("forward-slash path must extract cleanly");
        let (fw, fh) = decode_png_dimensions(&fwd_uri);
        assert!(fw >= 64 && fh >= 64);

        // 3. Bare executable names resolve via PATH / App Paths (with or without .exe)
        let resolved_cmd = resolve_icon_source_path("cmd.exe")
            .expect("bare cmd.exe must resolve via PATH/System32");
        assert!(resolved_cmd.to_ascii_lowercase().ends_with("cmd.exe"));
        let resolved_explorer = resolve_icon_source_path("explorer")
            .expect("bare explorer without .exe must resolve");
        assert!(resolved_explorer.to_ascii_lowercase().ends_with("explorer.exe"));
        assert!(extract_path_icon_raw("notepad.exe").is_some());

        // 4. Squirrel Update.exe resolves to newest sibling app-*\<App>.exe for real icon extraction
        let tmp = std::env::temp_dir().join(format!(
            "testbar-squirrel-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let discord_root = tmp.join("Discord");
        std::fs::create_dir_all(discord_root.join("app-1.0.9001")).unwrap();
        std::fs::create_dir_all(discord_root.join("app-1.0.9168")).unwrap();
        let update_exe = discord_root.join("Update.exe");
        std::fs::write(&update_exe, b"stub").unwrap();
        std::fs::write(discord_root.join("app-1.0.9001").join("Discord.exe"), b"old").unwrap();
        let newest_exe = discord_root.join("app-1.0.9168").join("Discord.exe");
        std::fs::write(&newest_exe, b"new").unwrap();

        let resolved_squirrel = resolve_icon_source_path(&update_exe.to_string_lossy())
            .expect("Squirrel Update.exe must resolve to real app exe");
        assert_eq!(
            PathBuf::from(&resolved_squirrel),
            newest_exe,
            "Squirrel Update.exe must prefer newest app-*\\Discord.exe"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn stabilize_window_groups_order_prevents_z_order_teleporting_and_preview_shuffling() {
        let make_win = |hwnd: isize, exe: &str, active: bool| OpenWindowInfo {
            hwnd,
            pid: hwnd as u32,
            title: format!("Win {hwnd}"),
            exe_path: exe.to_string(),
            exe_name: friendly_exe_name(exe),
            aumid: None,
            is_active: active,
            is_minimized: false,
            is_topmost: false,
            monitor: MONITOR_PRIMARY.to_string(),
        };
        let make_group = |key: &str, wins: Vec<OpenWindowInfo>| AppWindowGroup {
            key: key.to_string(),
            name: key.to_string(),
            exe_path: key.to_string(),
            aumid: None,
            icon_key: None,
            icon_data: None,
            is_active: wins.iter().any(|w| w.is_active),
            windows: wins,
        };

        let mut group_order: Vec<String> = Vec::new();
        let mut window_order: HashMap<String, Vec<isize>> = HashMap::new();

        // Scan 1: initial Z-order [Chrome(200, 100), Settings(300), Notepad(400)]
        let scan1 = stabilize_window_groups_order(
            vec![
                make_group(
                    "chrome.exe",
                    vec![
                        make_win(200, r"C:\Apps\chrome.exe", true),
                        make_win(100, r"C:\Apps\chrome.exe", false),
                    ],
                ),
                make_group(
                    "settings.exe",
                    vec![make_win(300, r"C:\Apps\settings.exe", false)],
                ),
                make_group(
                    "notepad.exe",
                    vec![make_win(400, r"C:\Apps\notepad.exe", false)],
                ),
            ],
            &mut group_order,
            &mut window_order,
        );
        let keys1: Vec<&str> = scan1.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys1, vec!["chrome.exe", "settings.exe", "notepad.exe"]);
        // Multi-window Chrome initially ordered deterministically by HWND [100, 200]
        let chrome_hwnds1: Vec<isize> = scan1[0].windows.iter().map(|w| w.hwnd).collect();
        assert_eq!(chrome_hwnds1, vec![100, 200]);

        // Scan 2: user clicks Settings (jumps to Z-order #1) and focuses Chrome window 200 over 100,
        //         plus Notepad jumps above Chrome! Order MUST NOT change at all!
        let scan2 = stabilize_window_groups_order(
            vec![
                make_group(
                    "settings.exe",
                    vec![make_win(300, r"C:\Apps\settings.exe", true)],
                ),
                make_group(
                    "notepad.exe",
                    vec![make_win(400, r"C:\Apps\notepad.exe", false)],
                ),
                make_group(
                    "chrome.exe",
                    vec![
                        make_win(200, r"C:\Apps\chrome.exe", false),
                        make_win(100, r"C:\Apps\chrome.exe", false),
                    ],
                ),
            ],
            &mut group_order,
            &mut window_order,
        );
        let keys2: Vec<&str> = scan2.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(
            keys2,
            vec!["chrome.exe", "settings.exe", "notepad.exe"],
            "clicking Settings MUST NOT teleport icons across the taskbar"
        );
        let chrome_hwnds2: Vec<isize> = scan2[0].windows.iter().map(|w| w.hwnd).collect();
        assert_eq!(
            chrome_hwnds2,
            vec![100, 200],
            "Z-order focus change MUST NOT shuffle preview cards"
        );

        // Scan 3: Settings closes, brand-new Discord opens (at Z-order #1) and Chrome opens 3rd window (hwnd 50)
        //         -> Chrome stays #0, Notepad stays #1, Discord appends STRICTLY at END (#2),
        //         and Chrome's new window 50 appends at END of Chrome's previews [100, 200, 50]!
        let scan3 = stabilize_window_groups_order(
            vec![
                make_group(
                    "discord.exe",
                    vec![make_win(500, r"C:\Apps\discord.exe", true)],
                ),
                make_group(
                    "notepad.exe",
                    vec![make_win(400, r"C:\Apps\notepad.exe", false)],
                ),
                make_group(
                    "chrome.exe",
                    vec![
                        make_win(50, r"C:\Apps\chrome.exe", false),
                        make_win(200, r"C:\Apps\chrome.exe", false),
                        make_win(100, r"C:\Apps\chrome.exe", false),
                    ],
                ),
            ],
            &mut group_order,
            &mut window_order,
        );
        let keys3: Vec<&str> = scan3.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(keys3, vec!["chrome.exe", "notepad.exe", "discord.exe"]);
        let chrome_hwnds3: Vec<isize> = scan3[0].windows.iter().map(|w| w.hwnd).collect();
        assert_eq!(chrome_hwnds3, vec![100, 200, 50]);
    }

    #[test]
    fn open_window_info_monitor_defaults_to_primary_in_default_and_legacy_json() {
        let def = OpenWindowInfo::default();
        assert_eq!(def.monitor, MONITOR_PRIMARY);

        let raw = r#"{"hwnd":7,"pid":8,"title":"Legacy","exePath":"C:\\Windows\\notepad.exe","exeName":"Notepad","aumid":null,"isActive":false,"isMinimized":false}"#;
        let parsed: OpenWindowInfo = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.monitor, MONITOR_PRIMARY);

        let with_sec = r#"{"hwnd":9,"pid":10,"title":"Monitor2","exePath":"C:\\Apps\\code.exe","exeName":"VS Code","aumid":null,"isActive":true,"isMinimized":false,"monitor":"secondary"}"#;
        let parsed_sec: OpenWindowInfo = serde_json::from_str(with_sec).unwrap();
        assert_eq!(parsed_sec.monitor, MONITOR_SECONDARY);
    }

    #[test]
    fn monitor_flags_and_workspace_to_screen_rect_conversion() {
        assert_eq!(classify_monitor_flags(MONITORINFOF_PRIMARY), MONITOR_PRIMARY);
        assert_eq!(classify_monitor_flags(0), MONITOR_SECONDARY);

        // Bottom taskbar (primary work top/left matches monitor top/left => zero offset)
        let primary_mon = PixelRect::new(0, 0, 3840, 1600);
        let bottom_work = PixelRect::new(0, 0, 3840, 1542);
        let mon2_normal = PixelRect::new(3900, 120, 5100, 1800);
        assert_eq!(
            workspace_rect_to_screen_rect(mon2_normal, primary_mon, bottom_work),
            mon2_normal
        );

        // Top/left taskbar offset shifts workspace coords back to screen coords
        let shifted_work = PixelRect::new(60, 48, 3840, 1600);
        assert_eq!(
            workspace_rect_to_screen_rect(PixelRect::new(100, 200, 900, 800), primary_mon, shifted_work),
            PixelRect::new(160, 248, 960, 848)
        );
    }

    #[test]
    fn resolve_monitor_with_cache_preserves_minimized_secondary_and_tracks_drags() {
        const HMON_PRIMARY: isize = 0x1001;
        const HMON_SECONDARY: isize = 0x2002;
        const HMON_STALE: isize = 0x9999;

        let classify = |h: isize| match h {
            HMON_PRIMARY => Some(MONITOR_PRIMARY),
            HMON_SECONDARY => Some(MONITOR_SECONDARY),
            _ => None,
        };

        let mut prev: HashMap<isize, isize> = HashMap::new();
        let mut updated: HashMap<isize, isize> = HashMap::new();

        // 1. Live non-minimized window on secondary monitor records HMON_SECONDARY
        let m1 = resolve_monitor_with_cache(
            501,
            false,
            Some(HMON_SECONDARY),
            &prev,
            &mut updated,
            classify,
            || panic!("placement fallback must not run for live non-minimized windows"),
        );
        assert_eq!(m1, MONITOR_SECONDARY);
        assert_eq!(updated.get(&501), Some(&HMON_SECONDARY));

        // 2. Window 501 is now minimized -> preserves cached HMON_SECONDARY instead of snapping to primary
        prev.extend(updated.drain());
        let m2 = resolve_monitor_with_cache(
            501,
            true,
            None,
            &prev,
            &mut updated,
            classify,
            || panic!("placement fallback must not run when valid cached HMONITOR exists"),
        );
        assert_eq!(m2, MONITOR_SECONDARY);

        // 3. Brand-new window 502 was already minimized before TestBar launched -> uses placement fallback
        let m3 = resolve_monitor_with_cache(
            502,
            true,
            None,
            &prev,
            &mut updated,
            classify,
            || Some(HMON_SECONDARY),
        );
        assert_eq!(m3, MONITOR_SECONDARY);
        assert_eq!(updated.get(&502), Some(&HMON_SECONDARY));

        // 4. Stale cached HMONITOR (e.g. unplugged display) degrades to placement fallback cleanly
        prev.insert(503, HMON_STALE);
        let m4 = resolve_monitor_with_cache(
            503,
            true,
            None,
            &prev,
            &mut updated,
            classify,
            || Some(HMON_PRIMARY),
        );
        assert_eq!(m4, MONITOR_PRIMARY);
        assert_eq!(updated.get(&503), Some(&HMON_PRIMARY));

        // 5. User drags window 501 from secondary to primary -> cache updates to HMON_PRIMARY
        let m5 = resolve_monitor_with_cache(
            501,
            false,
            Some(HMON_PRIMARY),
            &prev,
            &mut updated,
            classify,
            || None,
        );
        assert_eq!(m5, MONITOR_PRIMARY);
        assert_eq!(updated.get(&501), Some(&HMON_PRIMARY));
    }

    #[test]
    fn filter_window_groups_for_monitor_isolates_monitors_and_recomputes_is_active() {
        let make_win = |hwnd: isize, active: bool, monitor: &str| OpenWindowInfo {
            hwnd,
            pid: hwnd as u32,
            title: format!("Win {hwnd}"),
            exe_path: r"C:\Apps\app.exe".to_string(),
            exe_name: "App".to_string(),
            aumid: None,
            is_active: active,
            is_minimized: false,
            is_topmost: false,
            monitor: monitor.to_string(),
        };

        let groups = vec![
            // VS Code split across BOTH monitors: active window is on Monitor 1, inactive on Monitor 2
            AppWindowGroup {
                key: "code.exe".to_string(),
                name: "VS Code".to_string(),
                exe_path: r"C:\Apps\Code.exe".to_string(),
                aumid: None,
                icon_key: Some("exe:code".to_string()),
                icon_data: None,
                is_active: true,
                windows: vec![
                    make_win(1001, true, MONITOR_PRIMARY),
                    make_win(1002, false, MONITOR_SECONDARY),
                ],
            },
            // Chrome ONLY on Monitor 1 (including a legacy empty-monitor window treated as primary)
            AppWindowGroup {
                key: "chrome.exe".to_string(),
                name: "Chrome".to_string(),
                exe_path: r"C:\Apps\chrome.exe".to_string(),
                aumid: None,
                icon_key: Some("exe:chrome".to_string()),
                icon_data: None,
                is_active: false,
                windows: vec![
                    make_win(2001, false, MONITOR_PRIMARY),
                    make_win(2002, false, ""),
                ],
            },
            // Discord ONLY on Monitor 2
            AppWindowGroup {
                key: "discord.exe".to_string(),
                name: "Discord".to_string(),
                exe_path: r"C:\Apps\Discord.exe".to_string(),
                aumid: None,
                icon_key: Some("exe:discord".to_string()),
                icon_data: None,
                is_active: false,
                windows: vec![make_win(3001, false, MONITOR_SECONDARY)],
            },
        ];

        // Primary taskbar: sees VS Code (only hwnd 1001, active=true) + Chrome (2001, 2002); NO Discord
        let primary = filter_window_groups_for_monitor(&groups, MONITOR_PRIMARY);
        let primary_keys: Vec<&str> = primary.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(primary_keys, vec!["code.exe", "chrome.exe"]);
        assert_eq!(primary[0].windows.len(), 1);
        assert_eq!(primary[0].windows[0].hwnd, 1001);
        assert!(primary[0].is_active);
        assert_eq!(primary[1].windows.len(), 2);
        assert!(!primary[1].is_active);

        // Secondary taskbar: sees VS Code (only hwnd 1002, active=FALSE!) + Discord (3001); NO Chrome
        let secondary = filter_window_groups_for_monitor(&groups, MONITOR_SECONDARY);
        let secondary_keys: Vec<&str> = secondary.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(secondary_keys, vec!["code.exe", "discord.exe"]);
        assert_eq!(secondary[0].windows.len(), 1);
        assert_eq!(secondary[0].windows[0].hwnd, 1002);
        assert!(
            !secondary[0].is_active,
            "focusing a Monitor 1 window must NOT mark Monitor 2 group active"
        );
        assert_eq!(secondary[1].windows.len(), 1);
        assert_eq!(secondary[1].windows[0].hwnd, 3001);

        // Defensive "all" / "" returns full group list unmodified
        assert_eq!(filter_window_groups_for_monitor(&groups, "all").len(), 3);
        assert_eq!(filter_window_groups_for_monitor(&groups, "").len(), 3);
    }

    #[test]
    fn steam_appid_crc64_cef_normalization_and_native_custom_destinations_smoke() {
        // Exact Windows AppID CRC64-ECMA-182 hash verified against live Windows 11 CustomDestinations
        assert_eq!(
            windows_appid_crc64("Valve.Steam.Client"),
            0xd356_105f_ac55_27ef
        );
        assert_eq!(
            windows_appid_crc64("VALVE.STEAM.CLIENT"),
            0xd356_105f_ac55_27ef
        );

        // Steam CEF helper normalizes cleanly to parent steam.exe
        let norm = normalize_jump_list_exe_path(r"D:\Steam\bin\cef\cef.win64\steamwebhelper.exe");
        assert!(
            norm.to_ascii_lowercase().ends_with(r"\steam.exe"),
            "expected steamwebhelper.exe to normalize to steam.exe, got {norm}"
        );
        let cands = candidate_jump_list_app_ids(&norm, None);
        assert!(
            cands.iter().any(|c| c.eq_ignore_ascii_case("Valve.Steam.Client")),
            "expected Valve.Steam.Client candidate for Steam, got {cands:?}"
        );

        // If Steam's native CustomDestinations file exists on this host, verify O(1) parse in < 20ms
        if let Some(cd_path) = dirs::config_dir()
            .map(|d| d.join(r"Microsoft\Windows\Recent\CustomDestinations\d356105fac5527ef.customDestinations-ms"))
            .filter(|p| p.is_file())
        {
            let started = Instant::now();
            let items = get_jump_list(r"D:\Steam\bin\cef\cef.win64\steamwebhelper.exe", None);
            let elapsed = started.elapsed();
            assert!(
                elapsed < Duration::from_millis(25),
                "Steam jump list lookup must be O(1) fast (<25ms), took {elapsed:?} for {}",
                cd_path.display()
            );
            assert!(
                items.iter().any(|i| i.kind == "task"
                    && i.args
                        .as_deref()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .contains("steam://")),
                "expected native Steam tasks from CustomDestinations, got {items:?}"
            );
            assert!(
                items.len() >= 4,
                "expected rich Steam native jump list (>=4 items), got {items:?}"
            );
        }
    }

    #[test]
    fn aspect_preserving_thumbnail_sizing_and_area_box_downsample() {
        // 16:9 desktop window fits cleanly within 440x260
        assert_eq!(
            compute_aspect_preserving_thumb_size(1920, 1080, 440, 260),
            (440, 248)
        );
        // Narrow tall Steam Friends window (300x800) preserves exact 3:8 ratio without stretching!
        let (fw, fh) = compute_aspect_preserving_thumb_size(300, 800, 440, 260);
        assert_eq!((fw, fh), (98, 260));
        // Small window never upscales past 1.0x
        assert_eq!(
            compute_aspect_preserving_thumb_size(180, 120, 440, 260),
            (180, 120)
        );

        // 4x4 BGRA source (2x2 red top-left + 2x2 blue bottom-right cropped to 4x4) -> 2x2 RGBA exact average
        let mut bgra = vec![0u8; 4 * 4 * 4];
        for y in 0..4 {
            for x in 0..4 {
                let idx = (y * 4 + x) * 4;
                if x < 2 && y < 2 {
                    // Pure red in BGRA = (0, 0, 240, 0)
                    bgra[idx] = 0;
                    bgra[idx + 1] = 0;
                    bgra[idx + 2] = 240;
                } else {
                    // Pure blue in BGRA = (200, 0, 0, 0)
                    bgra[idx] = 200;
                    bgra[idx + 1] = 0;
                    bgra[idx + 2] = 0;
                }
            }
        }
        let rgba = downsample_bgra_region_to_rgba(&bgra, 4, 4, 0, 0, 4, 4, 2, 2);
        assert_eq!(rgba.len(), 2 * 2 * 4);
        // Top-left pixel -> pure red, alpha=255
        assert_eq!(&rgba[0..4], &[240, 0, 0, 255]);
        // Bottom-right pixel -> pure blue, alpha=255
        assert_eq!(&rgba[12..16], &[0, 0, 200, 255]);
    }

    #[test]
    fn pin_icon_extraction_wt_discord_and_uwp_smoke() {
        // 1. Windows Terminal AppExecLink alias (`WindowsApps\wt.exe`) resolves cleanly via UWP
        if let Some(local) = dirs::data_local_dir() {
            let wt = local.join(r"Microsoft\WindowsApps\wt.exe");
            if std::fs::symlink_metadata(&wt).is_ok() {
                assert!(is_windows_apps_execution_alias(&wt.to_string_lossy()));
                let uri = extract_path_icon_raw(&wt.to_string_lossy())
                    .expect("WindowsApps\\wt.exe must resolve crisp UWP icon");
                assert!(uri.starts_with("data:image/png;base64,"));
            }

            // 2. Squirrel Discord\Update.exe resolves sibling app-*\Discord.exe (or Discord.exe)
            let discord_upd = local.join(r"Discord\Update.exe");
            if discord_upd.is_file() {
                let real = resolve_squirrel_app_exe(&discord_upd)
                    .expect("Discord\\Update.exe must resolve real Discord.exe");
                assert!(
                    real.to_string_lossy().to_ascii_lowercase().contains("discord.exe"),
                    "unexpected resolved Squirrel exe: {}",
                    real.display()
                );
                let uri = extract_path_icon_raw(&discord_upd.to_string_lossy())
                    .expect("Discord\\Update.exe must extract crisp icon");
                assert!(uri.starts_with("data:image/png;base64,"));
            }
        }

        // 3. Quoted shell:AppsFolder\... path extracts cleanly
        let quoted_settings = r#""shell:AppsFolder\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel""#;
        let uri = extract_path_icon_raw(quoted_settings)
            .expect("quoted shell:AppsFolder AUMID must extract cleanly");
        assert!(uri.starts_with("data:image/png;base64,"));
    }
}
