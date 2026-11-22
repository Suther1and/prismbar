use base64::Engine;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};
use windows::core::{HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    GlobalFree, BOOL, HANDLE, HWND, POINT, RPC_E_CHANGED_MODE, SIZE, S_FALSE, S_OK,
};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    FILE_ATTRIBUTE_HIDDEN,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GHND};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SHFileOperationW, ShellExecuteW,
    DROPFILES, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
    SHFILEOPSTRUCTW, SIIGBF_ICONONLY, SIIGBF_RESIZETOFIT, SIIGBF_THUMBNAILONLY,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;

use crate::win_shell::wide;
use crate::windows_tracker::bgra_to_rgba;

const CF_HDROP: u32 = 15;
const DRIVE_REMOVABLE_TYPE: u32 = 2;
const DRIVE_FIXED_TYPE: u32 = 3;
const DRIVE_REMOTE_TYPE: u32 = 4;
const DRIVE_CDROM_TYPE: u32 = 5;
const DRIVE_RAMDISK_TYPE: u32 = 6;

pub const THUMBNAIL_CACHE_CAPACITY: usize = 256;
pub const MAX_THUMBNAIL_BATCH: usize = 48;
pub const MIN_THUMBNAIL_SIZE: u32 = 16;
pub const MAX_THUMBNAIL_SIZE: u32 = 128;

const CLIPBOARD_OPEN_RETRIES: usize = 5;
const CLIPBOARD_RETRY_DELAY_MS: u64 = 10;

static THUMBNAIL_CACHE: OnceLock<Mutex<ThumbnailLruCache>> = OnceLock::new();
static EXPLORER_SHELL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn thumbnail_cache() -> &'static Mutex<ThumbnailLruCache> {
    THUMBNAIL_CACHE.get_or_init(|| Mutex::new(ThumbnailLruCache::new(THUMBNAIL_CACHE_CAPACITY)))
}

fn explorer_shell_lock() -> &'static Mutex<()> {
    EXPLORER_SHELL_LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BreadcrumbPart {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriveEntry {
    pub letter: String,
    pub path: String,
    pub label: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuickRoot {
    pub id: String,
    pub label: String,
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_hidden: bool,
    pub size_bytes: u64,
    pub modified_ms: u64,
    pub extension: String,
    pub category: String,
    pub has_preview: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryListing {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub breadcrumbs: Vec<BreadcrumbPart>,
    pub entries: Vec<FileEntry>,
    pub total_count: usize,
    pub drives: Vec<DriveEntry>,
    pub quick_roots: Vec<QuickRoot>,
}

/// Decides whether `CoUninitialize` must be called for a `CoInitializeEx` HRESULT (AD-5).
pub fn should_uninitialize_com(hr: HRESULT) -> bool {
    hr == S_OK || hr == S_FALSE
}

/// RAII STA COM guard (`COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE`) honoring `AD-5`.
struct ExplorerComGuard {
    should_uninit: bool,
    usable: bool,
}

impl ExplorerComGuard {
    fn init_sta() -> Self {
        let hr = unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
        };
        Self::from_hresult(hr)
    }

    fn from_hresult(hr: HRESULT) -> Self {
        let should_uninit = should_uninitialize_com(hr);
        let usable = should_uninit || hr == RPC_E_CHANGED_MODE;
        Self {
            should_uninit,
            usable,
        }
    }

    fn is_usable(&self) -> bool {
        self.usable
    }
}

impl Drop for ExplorerComGuard {
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

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ThumbCacheKey {
    norm_path: String,
    modified_ms: u64,
    size: u32,
}

#[derive(Debug, Clone)]
struct ThumbCacheEntry {
    value: String,
    last_used: u64,
}

#[derive(Debug)]
struct ThumbnailLruCache {
    capacity: usize,
    tick: u64,
    entries: HashMap<ThumbCacheKey, ThumbCacheEntry>,
}

impl ThumbnailLruCache {
    fn new(capacity: usize) -> Self {
        let cap = capacity.max(1);
        Self {
            capacity: cap,
            tick: 0,
            entries: HashMap::with_capacity(cap.min(THUMBNAIL_CACHE_CAPACITY)),
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    fn get(&mut self, key: &ThumbCacheKey) -> Option<String> {
        let tick = self.next_tick();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = tick;
            Some(entry.value.clone())
        } else {
            None
        }
    }

    fn put(&mut self, key: ThumbCacheKey, value: String) {
        let tick = self.next_tick();
        if let Some(existing) = self.entries.get_mut(&key) {
            existing.value = value;
            existing.last_used = tick;
            return;
        }
        while self.entries.len() >= self.capacity {
            let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.entries.remove(&oldest_key);
        }
        self.entries.insert(key, ThumbCacheEntry { value, last_used: tick });
    }

    fn invalidate_path(&mut self, norm_path: &str) {
        let target = norm_path.to_lowercase();
        self.entries.retain(|k, _| k.norm_path != target);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Strips extended-length Win32 prefixes (`\\?\`, `\\?\UNC\`), normalizes slashes, collapses
/// redundant separators, handles bare drive specs (`C:` -> `C:\`), and resolves `.` / `..`.
pub fn normalize_windows_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Пустой путь".to_string());
    }
    if trimmed.contains('\0') || trimmed.contains('\r') || trimmed.contains('\n') {
        return Err("Путь содержит недопустимые управляющие символы".to_string());
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

    let mut s = unquoted.replace('/', "\\");
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        s = format!(r"\\{rest}");
    } else if let Some(rest) = s.strip_prefix(r"\\?\unc\") {
        s = format!(r"\\{rest}");
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }

    let is_unc = s.starts_with(r"\\");
    if is_unc {
        let without_prefix = s.trim_start_matches('\\');
        let parts: Vec<&str> = without_prefix
            .split('\\')
            .filter(|p| !p.is_empty() && *p != ".")
            .collect();
        if parts.len() < 2 {
            return Err(format!("Некорректный сетевой путь: {raw}"));
        }
        let root = format!(r"\\{}\{}", parts[0], parts[1]);
        let mut stack: Vec<&str> = Vec::new();
        for seg in &parts[2..] {
            if *seg == ".." {
                stack.pop();
            } else {
                stack.push(seg);
            }
        }
        if stack.is_empty() {
            return Ok(root);
        }
        return Ok(format!("{root}\\{}", stack.join("\\")));
    }

    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let drive = (bytes[0] as char).to_ascii_uppercase();
        let tail = &s[2..];
        if tail.is_empty() || tail.chars().all(|c| c == '\\') {
            return Ok(format!("{drive}:\\"));
        }
        let mut stack: Vec<&str> = Vec::new();
        for seg in tail.split('\\') {
            if seg.is_empty() || seg == "." {
                continue;
            }
            if seg == ".." {
                stack.pop();
            } else {
                stack.push(seg);
            }
        }
        if stack.is_empty() {
            return Ok(format!("{drive}:\\"));
        }
        return Ok(format!("{drive}:\\{}", stack.join("\\")));
    }

    // Relative path fallback: resolve against current_dir if possible
    let base = std::env::current_dir().map_err(|e| format!("Не удалось определить текущую папку: {e}"))?;
    let joined = base.join(&s);
    normalize_windows_path(&joined.to_string_lossy())
}

fn is_root_path(norm: &str) -> bool {
    let bytes = norm.as_bytes();
    if bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        return true;
    }
    if let Some(rest) = norm.strip_prefix(r"\\") {
        let parts: Vec<&str> = rest.split('\\').filter(|p| !p.is_empty()).collect();
        return parts.len() == 2;
    }
    false
}

/// Computes parent directory path for a normalized Windows path (`None` for drive/UNC roots).
pub fn compute_parent_path(norm: &str) -> Option<String> {
    if is_root_path(norm) {
        return None;
    }
    if let Some(rest) = norm.strip_prefix(r"\\") {
        let parts: Vec<&str> = rest.split('\\').filter(|p| !p.is_empty()).collect();
        if parts.len() <= 2 {
            return None;
        }
        return Some(format!(r"\\{}", parts[..parts.len() - 1].join("\\")));
    }
    let idx = norm.rfind('\\')?;
    if idx <= 2 {
        Some(format!("{}\\", &norm[..2]))
    } else {
        Some(norm[..idx].to_string())
    }
}

/// Builds clickable breadcrumb segments from a normalized Windows or UNC path.
pub fn build_breadcrumbs(norm: &str) -> Vec<BreadcrumbPart> {
    if let Some(rest) = norm.strip_prefix(r"\\") {
        let parts: Vec<&str> = rest.split('\\').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            return Vec::new();
        }
        let root = format!(r"\\{}\{}", parts[0], parts[1]);
        let mut out = vec![BreadcrumbPart {
            name: root.clone(),
            path: root.clone(),
        }];
        let mut acc = root;
        for seg in &parts[2..] {
            acc = format!("{acc}\\{seg}");
            out.push(BreadcrumbPart {
                name: (*seg).to_string(),
                path: acc.clone(),
            });
        }
        return out;
    }

    let bytes = norm.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        let drive_label = norm[..2].to_string();
        let drive_root = norm[..3].to_string();
        let mut out = vec![BreadcrumbPart {
            name: drive_label,
            path: drive_root.clone(),
        }];
        let mut acc = drive_root;
        for seg in norm[3..].split('\\').filter(|s| !s.is_empty()) {
            if acc.ends_with('\\') {
                acc.push_str(seg);
            } else {
                acc.push('\\');
                acc.push_str(seg);
            }
            out.push(BreadcrumbPart {
                name: seg.to_string(),
                path: acc.clone(),
            });
        }
        return out;
    }

    Vec::new()
}

/// Classifies a file extension into `"folder" | "image" | "video" | "audio" | "code" | "archive" | "document" | "app" | "file"`
/// and indicates whether `IShellItemImageFactory` thumbnail preview should be attempted first.
pub fn classify_entry(is_dir: bool, ext: &str) -> (&'static str, bool) {
    if is_dir {
        return ("folder", false);
    }
    match ext {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "ico" | "svg" | "tif" | "tiff"
        | "heic" | "heif" | "avif" | "psd" => ("image", true),
        "raw" | "dng" | "cr2" | "nef" | "arw" => ("image", true),
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v" => ("video", true),
        "flv" | "mpeg" | "mpg" | "m2ts" | "mts" | "3gp" => ("video", false),
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" | "wma" | "opus" | "aiff" | "mid"
        | "midi" => ("audio", false),
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "iso" | "cab" | "tgz" => {
            ("archive", false)
        }
        "pdf" => ("document", true),
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" | "rtf"
        | "txt" | "md" | "csv" | "epub" | "djvu" => ("document", false),
        "exe" | "msi" | "msix" | "appx" | "bat" | "cmd" | "lnk" | "url" | "ps1" | "vbs"
        | "scr" | "com" => ("app", false),
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "pyw" | "go" | "c" | "h"
        | "cpp" | "hpp" | "cc" | "cs" | "java" | "kt" | "kts" | "swift" | "rb" | "php"
        | "html" | "htm" | "css" | "scss" | "sass" | "less" | "json" | "jsonc" | "toml"
        | "yaml" | "yml" | "xml" | "sql" | "sh" | "bash" | "zsh" | "lua" | "r" | "dart"
        | "vue" | "svelte" | "zig" | "lock" | "ini" | "cfg" | "conf" | "env" | "gitignore"
        | "dockerfile" => ("code", false),
        _ => ("file", false),
    }
}

fn metadata_modified_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn build_file_entry(name: String, path: String, meta_opt: Option<&fs::Metadata>, fallback_is_dir: bool) -> FileEntry {
    let is_dir = meta_opt.map(|m| m.is_dir()).unwrap_or(fallback_is_dir);
    let attrs = meta_opt.map(|m| m.file_attributes()).unwrap_or(0);
    let is_hidden = (attrs & FILE_ATTRIBUTE_HIDDEN.0) != 0;
    let size_bytes = if is_dir {
        0
    } else {
        meta_opt.map(|m| m.len()).unwrap_or(0)
    };
    let modified_ms = meta_opt.map(metadata_modified_ms).unwrap_or(0);
    let extension = if is_dir {
        String::new()
    } else {
        Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default()
    };
    let (category, has_preview) = classify_entry(is_dir, &extension);

    FileEntry {
        name,
        path,
        is_dir,
        is_hidden,
        size_bytes,
        modified_ms,
        extension,
        category: category.to_string(),
        has_preview,
    }
}

/// Natural case-insensitive comparison (`"file2"` < `"file10"`, `"Глава 2"` < `"Глава 10"`).
/// Uses strict two-level token ordering so transitivity holds across leading zeros and casing.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = 0usize;
    let mut ib = 0usize;
    let mut secondary: Option<Ordering> = None;

    while ia < a.len() && ib < b.len() {
        let ra = &a[ia..];
        let rb = &b[ib..];
        let ca = ra.chars().next().unwrap();
        let cb = rb.chars().next().unwrap();

        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let da_len = ra.bytes().take_while(|c| c.is_ascii_digit()).count();
            let db_len = rb.bytes().take_while(|c| c.is_ascii_digit()).count();
            let na = &ra[..da_len];
            let nb = &rb[..db_len];
            let sa = na.trim_start_matches('0');
            let sb = nb.trim_start_matches('0');

            let ord = sa.len().cmp(&sb.len()).then_with(|| sa.cmp(sb));
            if ord != Ordering::Equal {
                return ord;
            }
            if secondary.is_none() {
                let tie = db_len.cmp(&da_len);
                if tie != Ordering::Equal {
                    secondary = Some(tie);
                }
            }
            ia += da_len;
            ib += db_len;
        } else {
            let ord = ca.to_lowercase().cmp(cb.to_lowercase());
            if ord != Ordering::Equal {
                return ord;
            }
            if secondary.is_none() {
                let tie = ca.cmp(&cb);
                if tie != Ordering::Equal {
                    secondary = Some(tie);
                }
            }
            ia += ca.len_utf8();
            ib += cb.len_utf8();
        }
    }

    a.len()
        .saturating_sub(ia)
        .cmp(&b.len().saturating_sub(ib))
        .then_with(|| secondary.unwrap_or(Ordering::Equal))
}

fn sort_entries(entries: &mut [FileEntry], sort_by: Option<&str>, sort_dir: Option<&str>) {
    let field = sort_by
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "name".to_string());
    let desc = sort_dir
        .map(|s| s.trim().eq_ignore_ascii_case("desc"))
        .unwrap_or(false);

    entries.sort_by(|a, b| {
        // Directories appear before files for name/size/type; date sort interleaves chronologically.
        if field != "date" && a.is_dir != b.is_dir {
            return b.is_dir.cmp(&a.is_dir);
        }

        let primary = match field.as_str() {
            "date" => a.modified_ms.cmp(&b.modified_ms),
            "size" => a.size_bytes.cmp(&b.size_bytes),
            "type" => {
                if a.is_dir && b.is_dir {
                    Ordering::Equal
                } else {
                    a.extension
                        .cmp(&b.extension)
                        .then_with(|| a.category.cmp(&b.category))
                }
            }
            _ => natural_cmp(&a.name, &b.name),
        };

        let ordered_primary = if desc { primary.reverse() } else { primary };
        ordered_primary.then_with(|| natural_cmp(&a.name, &b.name))
    });
}

fn default_drive_label(drive_type: u32) -> &'static str {
    match drive_type {
        DRIVE_REMOVABLE_TYPE => "Съёмный диск",
        DRIVE_FIXED_TYPE => "Локальный диск",
        DRIVE_REMOTE_TYPE => "Сетевой диск",
        DRIVE_CDROM_TYPE => "Оптический диск",
        DRIVE_RAMDISK_TYPE => "RAM-диск",
        _ => "Диск",
    }
}

/// Enumerates mounted Windows logical drives with capacity and volume labels.
pub fn list_drives() -> Vec<DriveEntry> {
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();

    for bit in 0u32..26u32 {
        if (mask & (1u32 << bit)) == 0 {
            continue;
        }
        let letter_ch = (b'A' + bit as u8) as char;
        let letter = format!("{letter_ch}:");
        let path = format!("{letter_ch}:\\");
        let wpath = wide(&path);

        let drive_type = unsafe { GetDriveTypeW(PCWSTR(wpath.as_ptr())) };
        if drive_type <= 1 {
            continue;
        }

        let mut free_avail: u64 = 0;
        let mut total_bytes: u64 = 0;
        let space_ok = unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(wpath.as_ptr()),
                Some(&mut free_avail),
                Some(&mut total_bytes),
                None,
            )
            .is_ok()
        };
        if !space_ok && (drive_type == DRIVE_CDROM_TYPE || drive_type == DRIVE_REMOVABLE_TYPE) {
            continue;
        }

        let mut label = String::new();
        if space_ok {
            let mut vol_buf = [0u16; 261];
            let vol_ok = unsafe {
                GetVolumeInformationW(
                    PCWSTR(wpath.as_ptr()),
                    Some(&mut vol_buf),
                    None,
                    None,
                    None,
                    None,
                )
                .is_ok()
            };
            if vol_ok {
                let end = vol_buf.iter().position(|&c| c == 0).unwrap_or(vol_buf.len());
                label = String::from_utf16_lossy(&vol_buf[..end]).trim().to_string();
            }
        }
        if label.is_empty() {
            label = default_drive_label(drive_type).to_string();
        }

        out.push(DriveEntry {
            letter,
            path,
            label,
            total_bytes,
            free_bytes: free_avail,
        });
    }

    out
}

/// Returns standard existing Windows user shell folders (Desktop, Downloads, Documents, Pictures, Home).
pub fn list_quick_roots() -> Vec<QuickRoot> {
    let candidates = [
        ("desktop", "Рабочий стол", "desktop", dirs::desktop_dir()),
        ("downloads", "Загрузки", "downloads", dirs::download_dir()),
        ("documents", "Документы", "documents", dirs::document_dir()),
        ("pictures", "Изображения", "pictures", dirs::picture_dir()),
        ("home", "Домашняя папка", "home", dirs::home_dir()),
    ];

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for (id, label, kind, path_opt) in candidates {
        let Some(p) = path_opt else {
            continue;
        };
        if !p.is_dir() {
            continue;
        }
        let Ok(norm) = normalize_windows_path(&p.to_string_lossy()) else {
            continue;
        };
        if seen.insert(norm.to_lowercase()) {
            out.push(QuickRoot {
                id: id.to_string(),
                label: label.to_string(),
                path: norm,
                kind: kind.to_string(),
            });
        }
    }

    out
}

/// Lists a directory in a single `read_dir` pass using `WIN32_FIND_DATAW`-cached metadata.
pub fn list_directory(
    path: &str,
    sort_by: Option<&str>,
    sort_dir: Option<&str>,
    show_hidden: bool,
) -> Result<DirectoryListing, String> {
    let raw = if path.trim().is_empty() {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from(r"C:\"))
            .to_string_lossy()
            .to_string()
    } else {
        path.to_string()
    };

    let current_path = normalize_windows_path(&raw)?;
    let dir_path = Path::new(&current_path);
    if !dir_path.exists() {
        return Err(format!("Папка не найдена: {current_path}"));
    }
    if !dir_path.is_dir() {
        return Err(format!("Указанный путь не является папкой: {current_path}"));
    }

    let read_iter = fs::read_dir(dir_path)
        .map_err(|e| format!("Не удалось открыть папку «{current_path}»: {e}"))?;

    let mut entries = Vec::new();
    for item in read_iter {
        let Ok(entry) = item else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let child_path = if current_path.ends_with('\\') {
            format!("{current_path}{name}")
        } else {
            format!("{current_path}\\{name}")
        };

        // `DirEntry::metadata` reads cached `WIN32_FIND_DATAW` on Windows without extra syscalls.
        let meta_opt = entry
            .metadata()
            .ok()
            .or_else(|| fs::symlink_metadata(&child_path).ok());
        let fallback_is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let built = build_file_entry(name, child_path, meta_opt.as_ref(), fallback_is_dir);

        if !show_hidden && built.is_hidden {
            continue;
        }
        entries.push(built);
    }

    sort_entries(&mut entries, sort_by, sort_dir);
    let total_count = entries.len();
    let parent_path = compute_parent_path(&current_path);
    let breadcrumbs = build_breadcrumbs(&current_path);
    let drives = list_drives();
    let quick_roots = list_quick_roots();

    Ok(DirectoryListing {
        current_path,
        parent_path,
        breadcrumbs,
        entries,
        total_count,
        drives,
        quick_roots,
    })
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

fn hbitmap_to_png_data_url(hbm: HBITMAP, opaque_if_no_alpha: bool) -> Option<String> {
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

        let byte_len = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
        let mut bgra = vec![0u8; byte_len];

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

        let has_any_alpha = bgra.chunks_exact(4).any(|px| px[3] > 0);
        let rgba = if opaque_if_no_alpha && !has_any_alpha {
            let mut out = vec![0u8; byte_len];
            for (src, dst) in bgra.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = 255;
            }
            out
        } else {
            bgra_to_rgba(&bgra)
        };

        encode_rgba_png(w as u32, h as u32, &rgba)
    }
}

fn extract_shell_thumbnail(norm_path: &str, size: u32, try_thumbnail: bool) -> Option<String> {
    let w = wide(norm_path);
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
        let sz = SIZE {
            cx: size as i32,
            cy: size as i32,
        };

        if try_thumbnail {
            if let Ok(hbm) = factory.GetImage(sz, SIIGBF_RESIZETOFIT | SIIGBF_THUMBNAILONLY) {
                if let Some(uri) = hbitmap_to_png_data_url(hbm, true) {
                    return Some(uri);
                }
            }
        }

        let hbm = factory
            .GetImage(sz, SIIGBF_RESIZETOFIT | SIIGBF_ICONONLY)
            .or_else(|_| factory.GetImage(sz, SIIGBF_RESIZETOFIT))
            .ok()?;
        hbitmap_to_png_data_url(hbm, false)
    }
}

/// Fetches PNG data-URI thumbnails/icons for up to 48 paths (`size` clamped to `16..=128`),
/// backed by a 256-entry LRU cache keyed by `(normalized_path, modified_ms, clamped_size)`.
pub fn get_file_thumbnails(paths: Vec<String>, size: u32) -> HashMap<String, String> {
    let clamped_size = size.clamp(MIN_THUMBNAIL_SIZE, MAX_THUMBNAIL_SIZE);
    let mut out = HashMap::new();
    if paths.is_empty() {
        return out;
    }

    struct PendingItem {
        raw_path: String,
        norm_path: String,
        key: ThumbCacheKey,
        try_thumbnail: bool,
    }

    let mut prepared = Vec::with_capacity(paths.len().min(MAX_THUMBNAIL_BATCH));
    for raw_path in paths.into_iter().take(MAX_THUMBNAIL_BATCH) {
        let Ok(norm_path) = normalize_windows_path(&raw_path) else {
            continue;
        };
        let Ok(meta) = fs::metadata(&norm_path).or_else(|_| fs::symlink_metadata(&norm_path)) else {
            continue;
        };
        let is_dir = meta.is_dir();
        let modified_ms = metadata_modified_ms(&meta);
        let ext = if is_dir {
            String::new()
        } else {
            Path::new(&norm_path)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default()
        };
        let (_, try_thumbnail) = classify_entry(is_dir, &ext);
        let key = ThumbCacheKey {
            norm_path: norm_path.to_lowercase(),
            modified_ms,
            size: clamped_size,
        };
        prepared.push(PendingItem {
            raw_path,
            norm_path,
            key,
            try_thumbnail,
        });
    }

    if prepared.is_empty() {
        return out;
    }

    let mut misses: Vec<PendingItem> = Vec::new();
    {
        let mut cache = lock_unpoisoned(thumbnail_cache());
        for item in prepared {
            if let Some(hit) = cache.get(&item.key) {
                out.insert(item.raw_path, hit);
            } else {
                misses.push(item);
            }
        }
    }

    if misses.is_empty() {
        return out;
    }

    let mut computed: Vec<(String, ThumbCacheKey, String)> = Vec::new();
    let mut seen_keys: HashMap<ThumbCacheKey, String> = HashMap::new();
    {
        let _shell_guard = lock_unpoisoned(explorer_shell_lock());
        let com = ExplorerComGuard::init_sta();
        if com.is_usable() {
            for item in misses {
                if let Some(existing_uri) = seen_keys.get(&item.key) {
                    out.insert(item.raw_path, existing_uri.clone());
                    continue;
                }
                if let Some(uri) =
                    extract_shell_thumbnail(&item.norm_path, clamped_size, item.try_thumbnail)
                {
                    seen_keys.insert(item.key.clone(), uri.clone());
                    out.insert(item.raw_path.clone(), uri.clone());
                    computed.push((item.raw_path, item.key, uri));
                }
            }
        }
    }

    if !computed.is_empty() {
        let mut cache = lock_unpoisoned(thumbnail_cache());
        for (_, key, uri) in computed {
            cache.put(key, uri);
        }
    }

    out
}

/// Validates a single Windows file or folder name (rejects invalid chars, trailing dots/spaces,
/// control chars, `.` / `..`, and Windows reserved DOS device names).
pub fn validate_windows_filename(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Имя не может быть пустым".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Недопустимое имя файла или папки".to_string());
    }
    if trimmed.ends_with('.') || trimmed.ends_with(' ') {
        return Err("Имя не может заканчиваться точкой или пробелом".to_string());
    }
    if trimmed.encode_utf16().count() > 255 {
        return Err("Имя слишком длинное (максимум 255 символов)".to_string());
    }

    for ch in trimmed.chars() {
        if (ch as u32) < 0x20 {
            return Err("Имя содержит недопустимые управляющие символы".to_string());
        }
        if matches!(ch, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') {
            return Err(format!("Имя содержит запрещённый символ «{ch}»"));
        }
    }

    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let is_reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if is_reserved {
        return Err(format!("Имя «{trimmed}» зарезервировано системой Windows"));
    }

    Ok(trimmed.to_string())
}

fn format_shell_execute_error(code: isize, target: &str) -> String {
    match code {
        2 | 3 => format!("Файл или папка не найдены: {target}"),
        5 => format!("Отказано в доступе: {target}"),
        31 => format!("Нет связанного приложения для открытия: {target}"),
        _ => format!("Не удалось открыть «{target}» (код {code})"),
    }
}

/// Opens a file or folder via Win32 `ShellExecuteW` (`"open"`).
pub fn fs_open_item(path: &str) -> Result<(), String> {
    let norm = normalize_windows_path(path)?;
    if !Path::new(&norm).exists() {
        return Err(format!("Элемент не найден: {norm}"));
    }
    let _com = ExplorerComGuard::init_sta();
    let verb = wide("open");
    let target = wide(&norm);
    let code = unsafe {
        ShellExecuteW(
            HWND::default(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOW,
        )
        .0 as isize
    };
    if code <= 32 {
        return Err(format_shell_execute_error(code, &norm));
    }
    Ok(())
}

/// Creates a new folder under `parent_path` after Windows filename validation.
pub fn fs_create_folder(parent_path: &str, name: &str) -> Result<FileEntry, String> {
    let parent_norm = normalize_windows_path(parent_path)?;
    let parent = Path::new(&parent_norm);
    if !parent.is_dir() {
        return Err(format!("Родительская папка не найдена: {parent_norm}"));
    }

    let valid_name = validate_windows_filename(name)?;
    let target_path = parent.join(&valid_name);
    if target_path.exists() || fs::symlink_metadata(&target_path).is_ok() {
        return Err(format!("Элемент «{valid_name}» уже существует"));
    }

    fs::create_dir(&target_path)
        .map_err(|e| format!("Не удалось создать папку «{valid_name}»: {e}"))?;

    let target_norm = if parent_norm.ends_with('\\') {
        format!("{parent_norm}{valid_name}")
    } else {
        format!("{parent_norm}\\{valid_name}")
    };
    let meta = fs::metadata(&target_path).ok();
    Ok(build_file_entry(valid_name, target_norm, meta.as_ref(), true))
}

/// Atomically renames a file or folder within the same parent directory and invalidates thumbnails.
pub fn fs_rename_item(path: &str, new_name: &str) -> Result<FileEntry, String> {
    let src_norm = normalize_windows_path(path)?;
    if is_root_path(&src_norm) {
        return Err("Нельзя переименовать корневой диск".to_string());
    }
    let src_path = Path::new(&src_norm);
    if fs::symlink_metadata(src_path).is_err() {
        return Err(format!("Элемент не найден: {src_norm}"));
    }

    let valid_name = validate_windows_filename(new_name)?;
    let parent = src_path
        .parent()
        .ok_or_else(|| "Не удалось определить родительскую папку".to_string())?;
    let parent_norm = normalize_windows_path(&parent.to_string_lossy())?;
    let dst_norm = if parent_norm.ends_with('\\') {
        format!("{parent_norm}{valid_name}")
    } else {
        format!("{parent_norm}\\{valid_name}")
    };
    let dst_path = Path::new(&dst_norm);

    let src_name = src_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if src_name == valid_name {
        let meta = fs::symlink_metadata(src_path)
            .ok()
            .or_else(|| fs::metadata(src_path).ok());
        let is_dir = src_path.is_dir();
        return Ok(build_file_entry(valid_name, dst_norm, meta.as_ref(), is_dir));
    }

    // Allow case-only renames on case-insensitive NTFS while rejecting collisions with other files.
    let case_only_change = src_name.to_lowercase() == valid_name.to_lowercase();
    if !case_only_change && (dst_path.exists() || fs::symlink_metadata(dst_path).is_ok()) {
        return Err(format!("Элемент «{valid_name}» уже существует"));
    }

    fs::rename(src_path, dst_path)
        .map_err(|e| format!("Не удалось переименовать в «{valid_name}»: {e}"))?;

    {
        let mut cache = lock_unpoisoned(thumbnail_cache());
        cache.invalidate_path(&src_norm);
        cache.invalidate_path(&dst_norm);
    }

    let meta = fs::metadata(dst_path)
        .ok()
        .or_else(|| fs::symlink_metadata(dst_path).ok());
    let is_dir = dst_path.is_dir();
    Ok(build_file_entry(valid_name, dst_norm, meta.as_ref(), is_dir))
}

fn encode_double_null_utf16(paths: &[String]) -> Vec<u16> {
    let mut wide_chars = Vec::new();
    for p in paths {
        wide_chars.extend(OsStr::new(p).encode_wide());
        wide_chars.push(0);
    }
    wide_chars.push(0);
    wide_chars
}

/// Builds a `CF_HDROP` memory block (`DROPFILES` + double-null-terminated UTF-16 path list).
pub fn build_cf_hdrop_payload_bytes(paths: &[String]) -> Result<Vec<u8>, String> {
    if paths.is_empty() {
        return Err("Список файлов пуст".to_string());
    }
    let mut normalized = Vec::with_capacity(paths.len());
    for raw in paths {
        normalized.push(normalize_windows_path(raw)?);
    }

    let dropfiles = DROPFILES {
        pFiles: std::mem::size_of::<DROPFILES>() as u32,
        pt: POINT { x: 0, y: 0 },
        fNC: BOOL(0),
        fWide: BOOL(1),
    };
    let wide_chars = encode_double_null_utf16(&normalized);

    let header_size = std::mem::size_of::<DROPFILES>();
    let total_bytes = header_size + wide_chars.len() * 2;
    let mut bytes = vec![0u8; total_bytes];

    unsafe {
        std::ptr::copy_nonoverlapping(
            &dropfiles as *const DROPFILES as *const u8,
            bytes.as_mut_ptr(),
            header_size,
        );
        std::ptr::copy_nonoverlapping(
            wide_chars.as_ptr() as *const u8,
            bytes[header_size..].as_mut_ptr(),
            wide_chars.len() * 2,
        );
    }

    Ok(bytes)
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

/// Copies one or more filesystem items into the Windows clipboard as `CF_HDROP`.
pub fn fs_copy_files_to_clipboard(paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Err("Не выбраны файлы для копирования".to_string());
    }
    let mut verified = Vec::with_capacity(paths.len());
    for raw in &paths {
        let norm = normalize_windows_path(raw)?;
        if fs::symlink_metadata(&norm).is_err() {
            return Err(format!("Элемент не найден: {norm}"));
        }
        verified.push(norm);
    }
    let payload = build_cf_hdrop_payload_bytes(&verified)?;

    unsafe {
        let hmem = match GlobalAlloc(GHND, payload.len()) {
            Ok(h) if !h.0.is_null() => h,
            Ok(_) => return Err("Не удалось выделить память для буфера обмена".to_string()),
            Err(e) => return Err(format!("Ошибка GlobalAlloc: {e}")),
        };

        let ptr = GlobalLock(hmem) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(hmem);
            return Err("Не удалось заблокировать память буфера обмена".to_string());
        }
        std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr, payload.len());
        let _ = GlobalUnlock(hmem);

        if !open_clipboard_with_retry(CLIPBOARD_OPEN_RETRIES, CLIPBOARD_RETRY_DELAY_MS) {
            let _ = GlobalFree(hmem);
            return Err("Не удалось открыть буфер обмена".to_string());
        }

        if let Err(e) = EmptyClipboard() {
            let _ = CloseClipboard();
            let _ = GlobalFree(hmem);
            return Err(format!("Не удалось очистить буфер обмена: {e}"));
        }

        // Windows owns `hmem` only when `SetClipboardData` succeeds.
        if let Err(e) = SetClipboardData(CF_HDROP, HANDLE(hmem.0)) {
            let _ = CloseClipboard();
            let _ = GlobalFree(hmem);
            return Err(format!("Не удалось записать файлы в буфер обмена: {e}"));
        }

        let _ = CloseClipboard();
        Ok(())
    }
}

/// Safely moves files/folders to the Windows Recycle Bin (`SHFileOperationW` + `FOF_ALLOWUNDO`).
/// Permanent deletion is strictly forbidden.
pub fn fs_trash_items(paths: Vec<String>) -> Result<usize, String> {
    if paths.is_empty() {
        return Err("Не выбраны элементы для удаления".to_string());
    }

    let mut unique_paths = Vec::with_capacity(paths.len());
    let mut seen = HashSet::new();
    for raw in &paths {
        let norm = normalize_windows_path(raw)?;
        if is_root_path(&norm) {
            return Err(format!("Нельзя удалить корневой диск: {norm}"));
        }
        if fs::symlink_metadata(&norm).is_err() {
            return Err(format!("Элемент не найден: {norm}"));
        }
        if seen.insert(norm.to_lowercase()) {
            unique_paths.push(norm);
        }
    }

    let _com = ExplorerComGuard::init_sta();
    let wide_from = encode_double_null_utf16(&unique_paths);

    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(wide_from.as_ptr()),
        pTo: PCWSTR::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        fAnyOperationsAborted: BOOL(0),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: PCWSTR::null(),
    };

    let code = unsafe { SHFileOperationW(&mut op) };
    if code != 0 || op.fAnyOperationsAborted.as_bool() {
        return Err(format!(
            "Не удалось переместить в корзину (код {code})"
        ));
    }

    {
        let mut cache = lock_unpoisoned(thumbnail_cache());
        for p in &unique_paths {
            cache.invalidate_path(p);
        }
    }

    Ok(unique_paths.len())
}

/// Formats an unescaped raw argument for `explorer.exe` (`/select,"<path>"` for files,
/// `"<path>"` for directories) honoring AD-7.
pub fn format_explorer_open_raw_arg(path: &str, is_dir: bool) -> Result<String, String> {
    let lower = path.trim().to_ascii_lowercase();
    if lower.contains("://") || lower.starts_with("shell:") || lower.starts_with("ms-") {
        return Err("Системные URI не поддерживаются в Проводнике".to_string());
    }
    let norm = normalize_windows_path(path)?;
    if is_dir {
        Ok(format!("\"{norm}\""))
    } else {
        Ok(format!("/select,\"{norm}\""))
    }
}

/// Reveals a file or opens a folder in native Windows Explorer using `CommandExt::raw_arg` (AD-7).
pub fn fs_open_in_explorer(path: &str) -> Result<(), String> {
    let norm = normalize_windows_path(path)?;
    let p = Path::new(&norm);
    if !p.exists() && fs::symlink_metadata(p).is_err() {
        return Err(format!("Элемент не найден: {norm}"));
    }
    let raw = format_explorer_open_raw_arg(&norm, p.is_dir())?;
    std::process::Command::new("explorer.exe")
        .raw_arg(raw)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось запустить Проводник: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_FLAGS_AND_ATTRIBUTES};

    fn unique_test_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "testbar_fx_{tag}_{}_{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn natural_sort_handles_numbers_case_and_unicode() {
        let mut names = vec![
            "file10.txt",
            "File2.txt",
            "file1.txt",
            "file01.txt",
            "file20.txt",
            "Глава 10.md",
            "глава 2.md",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            vec![
                "file01.txt",
                "file1.txt",
                "File2.txt",
                "file10.txt",
                "file20.txt",
                "глава 2.md",
                "Глава 10.md",
            ]
        );
    }

    #[test]
    fn path_normalization_and_breadcrumbs_handle_windows_variants() {
        assert_eq!(normalize_windows_path("c:").unwrap(), r"C:\");
        assert_eq!(normalize_windows_path("C:/").unwrap(), r"C:\");
        assert_eq!(
            normalize_windows_path(r"\\?\C:\Users\Test\Docs\").unwrap(),
            r"C:\Users\Test\Docs"
        );
        assert_eq!(
            normalize_windows_path(r"\\?\UNC\server\share\sub\..\folder").unwrap(),
            r"\\server\share\folder"
        );
        assert!(normalize_windows_path("").is_err());
        assert!(normalize_windows_path("C:\\bad\0path").is_err());

        assert_eq!(compute_parent_path(r"C:\"), None);
        assert_eq!(compute_parent_path(r"C:\Users"), Some(r"C:\".to_string()));
        assert_eq!(
            compute_parent_path(r"C:\Users\Test"),
            Some(r"C:\Users".to_string())
        );

        let crumbs = build_breadcrumbs(r"C:\Users\Test\Projects");
        assert_eq!(
            crumbs,
            vec![
                BreadcrumbPart {
                    name: "C:".to_string(),
                    path: r"C:\".to_string()
                },
                BreadcrumbPart {
                    name: "Users".to_string(),
                    path: r"C:\Users".to_string()
                },
                BreadcrumbPart {
                    name: "Test".to_string(),
                    path: r"C:\Users\Test".to_string()
                },
                BreadcrumbPart {
                    name: "Projects".to_string(),
                    path: r"C:\Users\Test\Projects".to_string()
                },
            ]
        );

        let root_crumbs = build_breadcrumbs(r"C:\");
        assert_eq!(
            root_crumbs,
            vec![BreadcrumbPart {
                name: "C:".to_string(),
                path: r"C:\".to_string()
            }]
        );
    }

    #[test]
    fn windows_filename_validation_matrix() {
        assert_eq!(
            validate_windows_filename("  Мой документ.pdf  ").unwrap(),
            "Мой документ.pdf"
        );
        assert_eq!(validate_windows_filename(".gitignore").unwrap(), ".gitignore");
        assert_eq!(validate_windows_filename("COM0").unwrap(), "COM0");

        for invalid in [
            "",
            "   ",
            ".",
            "..",
            "a/b",
            r"a\b",
            "a<b",
            "a>b",
            "a:b",
            "a\"b",
            "a|b",
            "a?b",
            "a*b",
            "bad\x01name",
            "trailing.",
            "trailing. ",
            "CON",
            "con.txt",
            "PRN",
            "AUX.log",
            "NUL.tar.gz",
            "COM1",
            "com9.bin",
            "LPT1",
            "lpt9.dat",
        ] {
            assert!(
                validate_windows_filename(invalid).is_err(),
                "expected invalid filename to be rejected: {invalid:?}"
            );
        }
    }

    #[test]
    fn directory_listing_create_and_rename_in_temp_dir() {
        let root = unique_test_dir("listing");
        let file_a = root.join("file10.txt");
        let file_b = root.join("file2.png");
        fs::write(&file_a, b"hello world").unwrap();
        fs::write(&file_b, b"fake png bytes").unwrap();

        let created_dir = fs_create_folder(&root.to_string_lossy(), "  Новая папка  ").unwrap();
        assert!(created_dir.is_dir);
        assert_eq!(created_dir.name, "Новая папка");
        assert_eq!(created_dir.category, "folder");

        // Duplicate create must fail
        assert!(fs_create_folder(&root.to_string_lossy(), "Новая папка").is_err());

        let listing = list_directory(&root.to_string_lossy(), Some("name"), Some("asc"), false)
            .expect("list temp directory");
        assert_eq!(listing.total_count, 3);
        assert_eq!(listing.entries[0].name, "Новая папка");
        assert!(listing.entries[0].is_dir);
        assert_eq!(listing.entries[1].name, "file2.png");
        assert_eq!(listing.entries[1].category, "image");
        assert!(listing.entries[1].has_preview);
        assert_eq!(listing.entries[2].name, "file10.txt");
        assert_eq!(listing.entries[2].category, "document");
        assert!(!listing.drives.is_empty());

        // Rename file and verify case-only rename also works cleanly
        let renamed = fs_rename_item(&file_a.to_string_lossy(), "notes.rs").unwrap();
        assert_eq!(renamed.name, "notes.rs");
        assert_eq!(renamed.category, "code");

        let case_renamed = fs_rename_item(&renamed.path, "Notes.rs").unwrap();
        assert_eq!(case_renamed.name, "Notes.rs");

        // Collision with existing file must fail
        assert!(fs_rename_item(&case_renamed.path, "file2.png").is_err());

        // Sort descending by size keeps directories first
        let by_size = list_directory(&root.to_string_lossy(), Some("size"), Some("desc"), false)
            .expect("list sorted by size");
        assert!(by_size.entries[0].is_dir);
        assert_eq!(by_size.entries[1].name, "file2.png"); // 14 bytes > 11 bytes

        // Cleanup individual test items cleanly
        let _ = fs::remove_file(&case_renamed.path);
        let _ = fs::remove_file(&file_b);
        let _ = fs::remove_dir(&created_dir.path);
        let _ = fs::remove_dir(&root);
    }

    #[test]
    fn list_directory_filters_hidden_files_and_extracts_thumbnails() {
        let root = unique_test_dir("hidden_thumb");
        let visible_png = root.join("sample.png");
        let hidden_txt = root.join("secret.txt");

        // Write a real 2x2 RGBA PNG so Shell thumbnail/icon extraction succeeds cleanly
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ])
            .unwrap();
        }
        fs::write(&visible_png, &png_bytes).unwrap();
        fs::write(&hidden_txt, b"hidden").unwrap();

        let hidden_w = wide(&hidden_txt.to_string_lossy());
        unsafe {
            SetFileAttributesW(
                PCWSTR(hidden_w.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(FILE_ATTRIBUTE_HIDDEN.0),
            )
            .expect("mark test file hidden");
        }

        let no_hidden =
            list_directory(&root.to_string_lossy(), Some("name"), Some("asc"), false).unwrap();
        assert_eq!(no_hidden.entries.len(), 1);
        assert_eq!(no_hidden.entries[0].name, "sample.png");

        let with_hidden =
            list_directory(&root.to_string_lossy(), Some("name"), Some("asc"), true).unwrap();
        assert_eq!(with_hidden.entries.len(), 2);
        let hidden_entry = with_hidden
            .entries
            .iter()
            .find(|e| e.name == "secret.txt")
            .unwrap();
        assert!(hidden_entry.is_hidden);

        // Extract thumbnail for file + folder (and verify clamping + batch cap)
        let vis_str = visible_png.to_string_lossy().to_string();
        let root_str = root.to_string_lossy().to_string();
        let mut req = vec![vis_str.clone(), root_str.clone()];
        for i in 0..60 {
            req.push(format!("{}\\missing_{i}.txt", root.to_string_lossy()));
        }
        let thumbs = get_file_thumbnails(req, 512);
        let uri = thumbs
            .get(&vis_str)
            .expect("real PNG should return thumbnail or shell icon data URI");
        assert!(uri.starts_with("data:image/png;base64,"));
        let folder_uri = thumbs
            .get(&root_str)
            .expect("folder should return shell icon data URI");
        assert!(folder_uri.starts_with("data:image/png;base64,"));

        // Safety checks for trash/clipboard guards
        assert!(fs_trash_items(vec![]).is_err());
        assert!(fs_trash_items(vec![r"C:\".to_string()]).is_err());
        assert!(fs_trash_items(vec![root.join("does_not_exist.txt").to_string_lossy().to_string()]).is_err());
        assert!(fs_copy_files_to_clipboard(vec![]).is_err());

        // Clear hidden attribute before removing
        unsafe {
            let _ = SetFileAttributesW(PCWSTR(hidden_w.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0));
        }
        let _ = fs::remove_file(&visible_png);
        let _ = fs::remove_file(&hidden_txt);
        let _ = fs::remove_dir(&root);
    }

    #[test]
    fn thumbnail_lru_cache_evicts_oldest_and_invalidates_path() {
        let mut cache = ThumbnailLruCache::new(2);
        let k1 = ThumbCacheKey {
            norm_path: r"c:\a.png".to_string(),
            modified_ms: 100,
            size: 48,
        };
        let k2 = ThumbCacheKey {
            norm_path: r"c:\b.png".to_string(),
            modified_ms: 200,
            size: 48,
        };
        let k3 = ThumbCacheKey {
            norm_path: r"c:\c.png".to_string(),
            modified_ms: 300,
            size: 48,
        };

        cache.put(k1.clone(), "uri1".to_string());
        cache.put(k2.clone(), "uri2".to_string());
        assert_eq!(cache.get(&k1).as_deref(), Some("uri1")); // bumps k1
        cache.put(k3.clone(), "uri3".to_string()); // evicts k2

        assert_eq!(cache.len(), 2);
        assert!(cache.get(&k2).is_none());
        assert_eq!(cache.get(&k1).as_deref(), Some("uri1"));
        assert_eq!(cache.get(&k3).as_deref(), Some("uri3"));

        cache.invalidate_path(r"C:\A.PNG");
        assert!(cache.get(&k1).is_none());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cf_hdrop_payload_and_explorer_args_format_cleanly() {
        let bytes = build_cf_hdrop_payload_bytes(&[r"C:\Temp\a.txt".to_string()]).unwrap();
        assert_eq!(std::mem::size_of::<DROPFILES>(), 20);
        let p_files = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let f_wide = i32::from_le_bytes(bytes[16..20].try_into().unwrap());
        assert_eq!(p_files, 20);
        assert_eq!(f_wide, 1);
        // Must end with UTF-16 double null terminator (0x0000, 0x0000)
        assert_eq!(&bytes[bytes.len() - 4..], &[0, 0, 0, 0]);

        assert_eq!(
            format_explorer_open_raw_arg(r"C:\Program Files\App\test.txt", false).unwrap(),
            r#"/select,"C:\Program Files\App\test.txt""#
        );
        assert_eq!(
            format_explorer_open_raw_arg(r"C:\Program Files\App", true).unwrap(),
            r#""C:\Program Files\App""#
        );
        assert!(format_explorer_open_raw_arg("shell:AppsFolder\\Test", false).is_err());
    }

    #[test]
    fn com_guard_never_uninitializes_on_rpc_changed_mode() {
        assert!(should_uninitialize_com(S_OK));
        assert!(should_uninitialize_com(S_FALSE));
        assert!(!should_uninitialize_com(RPC_E_CHANGED_MODE));

        let g_changed = ExplorerComGuard::from_hresult(RPC_E_CHANGED_MODE);
        assert!(g_changed.is_usable());
        assert!(!g_changed.should_uninit);
    }

    #[test]
    fn date_sort_interleaves_dirs_and_files_chronologically() {
        let mk = |name: &str, is_dir: bool, modified_ms: u64| FileEntry {
            name: name.to_string(),
            path: format!(r"C:\Test\{name}"),
            is_dir,
            is_hidden: false,
            size_bytes: 0,
            modified_ms,
            extension: String::new(),
            category: if is_dir { "folder" } else { "file" }.to_string(),
            has_preview: false,
        };
        let mut entries = vec![
            mk("OldFolder", true, 100),
            mk("NewFile.txt", false, 300),
            mk("NewFolder", true, 250),
            mk("OlderFile.txt", false, 150),
        ];

        sort_entries(&mut entries, Some("date"), Some("desc"));
        let names_desc: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names_desc,
            vec!["NewFile.txt", "NewFolder", "OlderFile.txt", "OldFolder"]
        );

        sort_entries(&mut entries, Some("name"), Some("asc"));
        assert!(entries[0].is_dir && entries[1].is_dir);
        assert!(!entries[2].is_dir && !entries[3].is_dir);
    }
}
