use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;
use sysinfo::Disks;

use crate::macos::bridge_ffi;

pub const THUMBNAIL_CACHE_CAPACITY: usize = 256;
pub const MAX_THUMBNAIL_BATCH: usize = 48;
pub const MIN_THUMBNAIL_SIZE: u32 = 16;
pub const MAX_THUMBNAIL_SIZE: u32 = 128;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ThumbCacheKey {
    norm_path: String,
    modified_ms: u64,
    size: u32,
}

struct ThumbEntry {
    uri: String,
    last_used: u64,
}

struct ThumbnailLruCache {
    capacity: usize,
    tick: u64,
    entries: HashMap<ThumbCacheKey, ThumbEntry>,
}

impl ThumbnailLruCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            tick: 0,
            entries: HashMap::with_capacity(capacity.max(1)),
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    fn get(&mut self, key: &ThumbCacheKey) -> Option<String> {
        let now = self.next_tick();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = now;
            return Some(entry.uri.clone());
        }
        None
    }

    fn put(&mut self, key: ThumbCacheKey, uri: String) {
        let now = self.next_tick();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.uri = uri;
            entry.last_used = now;
            return;
        }
        while self.entries.len() >= self.capacity {
            let lru = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone());
            if let Some(evict) = lru {
                self.entries.remove(&evict);
            } else {
                break;
            }
        }
        self.entries.insert(
            key,
            ThumbEntry {
                uri,
                last_used: now,
            },
        );
    }

    fn invalidate_path(&mut self, norm_path: &str) {
        self.entries.retain(|k, _| k.norm_path != norm_path);
    }
}

static THUMBNAIL_CACHE: OnceLock<Mutex<ThumbnailLruCache>> = OnceLock::new();

fn thumbnail_cache() -> &'static Mutex<ThumbnailLruCache> {
    THUMBNAIL_CACHE.get_or_init(|| Mutex::new(ThumbnailLruCache::new(THUMBNAIL_CACHE_CAPACITY)))
}

fn lock_unpoisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn home_dir_string() -> String {
    dirs::home_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"))
        .to_string_lossy()
        .to_string()
}

/// Normalizes a POSIX path using an explicit `home_dir` (`~` / empty -> `home_dir`,
/// collapses `//`, resolves `.`/`..`, preserves `/` root, strips trailing `/`).
pub fn normalize_posix_path_with_home(raw: &str, home_dir: &str) -> Result<String, String> {
    if raw.contains('\0') || raw.contains('\r') || raw.contains('\n') {
        return Err("Путь содержит недопустимые управляющие символы".to_string());
    }
    let trimmed = raw.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed)
        .trim();

    let expanded = if unquoted.is_empty() || unquoted == "~" {
        home_dir.to_string()
    } else if let Some(rest) = unquoted.strip_prefix("~/") {
        let base = home_dir.trim_end_matches('/');
        if base.is_empty() {
            format!("/{rest}")
        } else {
            format!("{base}/{rest}")
        }
    } else {
        unquoted.to_string()
    };

    if !expanded.starts_with('/') {
        return Err(format!("Путь должен быть абсолютным (начинаться с /): {raw}"));
    }

    let mut stack: Vec<&str> = Vec::new();
    for seg in expanded.split('/') {
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
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", stack.join("/")))
    }
}

/// Normalizes a POSIX path (`~` or empty expands to `$HOME`).
pub fn normalize_posix_path(raw: &str) -> Result<String, String> {
    normalize_posix_path_with_home(raw, &home_dir_string())
}

/// Computes parent directory path for a normalized POSIX path (`/` -> `None`).
pub fn compute_posix_parent_path(norm: &str) -> Option<String> {
    let trimmed = norm.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return None;
    }
    let idx = trimmed.rfind('/')?;
    if idx == 0 {
        Some("/".to_string())
    } else {
        Some(trimmed[..idx].to_string())
    }
}

/// Builds clickable breadcrumbs starting with `{ name: "Mac", path: "/" }`.
pub fn build_posix_breadcrumbs(norm: &str) -> Vec<BreadcrumbPart> {
    let mut out = vec![BreadcrumbPart {
        name: "Mac".to_string(),
        path: "/".to_string(),
    }];
    let trimmed = norm.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return out;
    }
    let mut acc = String::new();
    for seg in trimmed.trim_start_matches('/').split('/') {
        if seg.is_empty() {
            continue;
        }
        acc.push('/');
        acc.push_str(seg);
        out.push(BreadcrumbPart {
            name: seg.to_string(),
            path: acc.clone(),
        });
    }
    out
}

/// Validates a single POSIX file/folder name (forbids empty, `.`, `..`, `/`, NUL, control chars).
pub fn validate_posix_filename(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Имя не может быть пустым".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Недопустимое имя файла или папки".to_string());
    }
    if trimmed.len() > 255 {
        return Err("Имя слишком длинное (максимум 255 байт)".to_string());
    }
    for ch in trimmed.chars() {
        if ch == '/' || ch == '\0' || (ch as u32) < 0x20 {
            return Err("Имя содержит запрещённый символ".to_string());
        }
    }
    Ok(trimmed.to_string())
}

fn is_app_bundle_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".app")
}

/// Classifies an entry into `(category, has_preview)`.
pub fn classify_entry(is_dir: bool, ext: &str) -> (&'static str, bool) {
    if is_dir {
        return ("folder", false);
    }
    match ext {
        "app" => ("app", true),
        "pkg" | "command" | "workflow" => ("app", false),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "ico" | "icns" | "svg" | "tif"
        | "tiff" | "heic" | "heif" | "avif" | "psd" | "raw" | "dng" | "cr2" | "nef" | "arw" => {
            ("image", true)
        }
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v" => ("video", true),
        "flv" | "mpeg" | "mpg" | "m2ts" | "mts" | "3gp" => ("video", false),
        "mp3" | "wav" | "flac" | "ogg" | "aac" | "m4a" | "wma" | "opus" | "aiff" | "caf"
        | "m4r" | "mid" | "midi" => ("audio", false),
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "iso" | "dmg" | "tgz" => {
            ("archive", false)
        }
        "pdf" => ("document", true),
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" | "rtf"
        | "txt" | "md" | "csv" | "epub" | "djvu" | "pages" | "numbers" | "key" => {
            ("document", false)
        }
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "pyw" | "go" | "c" | "h"
        | "cpp" | "hpp" | "cc" | "cs" | "java" | "kt" | "kts" | "swift" | "m" | "mm" | "rb"
        | "php" | "html" | "htm" | "css" | "scss" | "sass" | "less" | "json" | "jsonc"
        | "toml" | "yaml" | "yml" | "xml" | "plist" | "sql" | "sh" | "bash" | "zsh" | "lua"
        | "r" | "dart" | "vue" | "svelte" | "zig" | "lock" | "ini" | "cfg" | "conf" | "env"
        | "gitignore" | "dockerfile" => ("code", false),
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

fn is_hidden_posix_name(name: &str) -> bool {
    name.starts_with('.') || name == "Icon\r"
}

fn build_file_entry(
    name: String,
    path: String,
    meta_opt: Option<&fs::Metadata>,
    fallback_is_dir: bool,
) -> FileEntry {
    let raw_is_dir = meta_opt.map(|m| m.is_dir()).unwrap_or(fallback_is_dir);
    let is_app_bundle = raw_is_dir && is_app_bundle_name(&name);
    // Treat macOS `.app` bundles as launchable items rather than traversable directories.
    let is_dir = raw_is_dir && !is_app_bundle;
    let is_hidden = is_hidden_posix_name(&name);
    let size_bytes = if raw_is_dir {
        0
    } else {
        meta_opt.map(|m| m.len()).unwrap_or(0)
    };
    let modified_ms = meta_opt.map(metadata_modified_ms).unwrap_or(0);
    let extension = if is_dir {
        String::new()
    } else if is_app_bundle {
        "app".to_string()
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
        if a.is_dir != b.is_dir {
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

/// Enumerates `/` (`Mac` / `Macintosh HD`) and mounted `/Volumes/*` drives via `sysinfo::Disks`.
pub fn list_drives() -> Vec<DriveEntry> {
    let disks = Disks::new_with_refreshed_list();
    let mut seen_mounts = HashSet::new();
    let mut out = Vec::new();

    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy().to_string();
        if mount == "/" {
            if seen_mounts.insert(mount) {
                out.push(DriveEntry {
                    letter: "Mac".to_string(),
                    path: "/".to_string(),
                    label: "Macintosh HD".to_string(),
                    total_bytes: disk.total_space(),
                    free_bytes: disk.available_space(),
                });
            }
            continue;
        }
        if let Some(vol_name) = mount.strip_prefix("/Volumes/") {
            if vol_name.is_empty() || vol_name.contains('/') {
                continue;
            }
            if seen_mounts.insert(mount.clone()) {
                let raw_name = disk.name().to_string_lossy().trim().to_string();
                let label = if raw_name.is_empty() || raw_name.starts_with("disk") {
                    vol_name.to_string()
                } else {
                    raw_name
                };
                out.push(DriveEntry {
                    letter: vol_name.to_string(),
                    path: mount,
                    label,
                    total_bytes: disk.total_space(),
                    free_bytes: disk.available_space(),
                });
            }
        }
    }

    out.sort_by(|a, b| {
        (a.path != "/")
            .cmp(&(b.path != "/"))
            .then_with(|| natural_cmp(&a.label, &b.label))
    });
    out
}

/// Lists standard existing macOS user roots (`Дом`, `Рабочий стол`, `Загрузки`, `Документы`, `Программы`, `iCloud Drive`).
pub fn list_quick_roots() -> Vec<QuickRoot> {
    let home = dirs::home_dir().or_else(|| std::env::var_os("HOME").map(PathBuf::from));
    let icloud = home
        .as_ref()
        .map(|h| h.join("Library/Mobile Documents/com~apple~CloudDocs"));

    let candidates = [
        ("home", "Дом", "home", home.clone()),
        (
            "desktop",
            "Рабочий стол",
            "desktop",
            dirs::desktop_dir().or_else(|| home.as_ref().map(|h| h.join("Desktop"))),
        ),
        (
            "downloads",
            "Загрузки",
            "downloads",
            dirs::download_dir().or_else(|| home.as_ref().map(|h| h.join("Downloads"))),
        ),
        (
            "documents",
            "Документы",
            "documents",
            dirs::document_dir().or_else(|| home.as_ref().map(|h| h.join("Documents"))),
        ),
        (
            "apps",
            "Программы",
            "apps",
            Some(PathBuf::from("/Applications")),
        ),
        ("cloud", "iCloud Drive", "cloud", icloud),
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
        let Ok(norm) = normalize_posix_path(&p.to_string_lossy()) else {
            continue;
        };
        if seen.insert(norm.clone()) {
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

/// Lists a directory on macOS, hiding dotfiles and `.DS_Store` unless `show_hidden` is true.
pub fn list_directory(
    path: &str,
    sort_by: Option<&str>,
    sort_dir: Option<&str>,
    show_hidden: bool,
) -> Result<DirectoryListing, String> {
    let current_path = normalize_posix_path(path)?;
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
        if !show_hidden && (is_hidden_posix_name(&name) || name == ".DS_Store") {
            continue;
        }

        let child_path = if current_path == "/" {
            format!("/{name}")
        } else {
            format!("{current_path}/{name}")
        };

        // Follow firmlinks/symlinks for `.app` bundles (e.g. Safari.app), fallback to symlink_metadata.
        let meta_opt = fs::metadata(&child_path)
            .ok()
            .or_else(|| fs::symlink_metadata(&child_path).ok());
        let fallback_is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let built = build_file_entry(name, child_path, meta_opt.as_ref(), fallback_is_dir);

        entries.push(built);
    }

    sort_entries(&mut entries, sort_by, sort_dir);
    let total_count = entries.len();
    let parent_path = compute_posix_parent_path(&current_path);
    let breadcrumbs = build_posix_breadcrumbs(&current_path);
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

/// Fetches QuickLook thumbnails / native icons for up to 48 paths, backed by a 256-entry LRU cache.
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
        is_app_bundle: bool,
        try_thumbnail: bool,
    }

    let mut prepared = Vec::with_capacity(paths.len().min(MAX_THUMBNAIL_BATCH));
    for raw_path in paths.into_iter().take(MAX_THUMBNAIL_BATCH) {
        let Ok(norm_path) = normalize_posix_path(&raw_path) else {
            continue;
        };
        let Ok(meta) = fs::metadata(&norm_path).or_else(|_| fs::symlink_metadata(&norm_path)) else {
            continue;
        };
        let raw_is_dir = meta.is_dir();
        let is_app_bundle = raw_is_dir && is_app_bundle_name(&norm_path);
        let is_dir = raw_is_dir && !is_app_bundle;
        let modified_ms = metadata_modified_ms(&meta);
        let ext = if is_dir {
            String::new()
        } else if is_app_bundle {
            "app".to_string()
        } else {
            Path::new(&norm_path)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default()
        };
        let (_, try_thumbnail) = classify_entry(is_dir, &ext);
        let key = ThumbCacheKey {
            norm_path: norm_path.clone(),
            modified_ms,
            size: clamped_size,
        };
        prepared.push(PendingItem {
            raw_path,
            norm_path,
            key,
            is_app_bundle,
            try_thumbnail,
        });
    }

    if prepared.is_empty() {
        return out;
    }

    let mut misses = Vec::new();
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

    let mut computed = Vec::new();
    let mut seen_keys: HashMap<ThumbCacheKey, String> = HashMap::new();
    for item in misses {
        if let Some(existing_uri) = seen_keys.get(&item.key) {
            out.insert(item.raw_path, existing_uri.clone());
            continue;
        }
        let uri_opt = if item.is_app_bundle || !item.try_thumbnail {
            bridge_ffi::icon_png_for_path(&item.norm_path, clamped_size as i32)
        } else {
            bridge_ffi::thumbnail_png_for_path(&item.norm_path, clamped_size as i32)
        };
        if let Some(uri) = uri_opt {
            seen_keys.insert(item.key.clone(), uri.clone());
            out.insert(item.raw_path, uri.clone());
            computed.push((item.key, uri));
        }
    }

    if !computed.is_empty() {
        let mut cache = lock_unpoisoned(thumbnail_cache());
        for (key, uri) in computed {
            cache.put(key, uri);
        }
    }

    out
}

/// Opens a file, folder, or `.app` bundle via macOS `open`.
pub fn fs_open_item(path: &str) -> Result<(), String> {
    let norm = normalize_posix_path(path)?;
    if !Path::new(&norm).exists() {
        return Err(format!("Элемент не найден: {norm}"));
    }
    Command::new("open")
        .arg(&norm)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось открыть «{norm}»: {e}"))
}

/// Creates a new directory under `parent_path`.
pub fn fs_create_folder(parent_path: &str, name: &str) -> Result<FileEntry, String> {
    let parent_norm = normalize_posix_path(parent_path)?;
    let parent = Path::new(&parent_norm);
    if !parent.is_dir() {
        return Err(format!("Родительская папка не найдена: {parent_norm}"));
    }

    let valid_name = validate_posix_filename(name)?;
    let target_path = parent.join(&valid_name);
    if target_path.exists() || fs::symlink_metadata(&target_path).is_ok() {
        return Err(format!("Элемент «{valid_name}» уже существует"));
    }

    fs::create_dir(&target_path)
        .map_err(|e| format!("Не удалось создать папку «{valid_name}»: {e}"))?;

    let target_norm = if parent_norm == "/" {
        format!("/{valid_name}")
    } else {
        format!("{parent_norm}/{valid_name}")
    };
    let meta = fs::metadata(&target_path).ok();
    Ok(build_file_entry(valid_name, target_norm, meta.as_ref(), true))
}

/// Renames a file, folder, or `.app` bundle within its parent directory.
pub fn fs_rename_item(path: &str, new_name: &str) -> Result<FileEntry, String> {
    let src_norm = normalize_posix_path(path)?;
    if src_norm == "/" {
        return Err("Нельзя переименовать корневой раздел".to_string());
    }
    let src_path = Path::new(&src_norm);
    if fs::symlink_metadata(src_path).is_err() {
        return Err(format!("Элемент не найден: {src_norm}"));
    }

    let valid_name = validate_posix_filename(new_name)?;
    let parent = src_path
        .parent()
        .ok_or_else(|| "Не удалось определить родительскую папку".to_string())?;
    let parent_norm = normalize_posix_path(&parent.to_string_lossy())?;
    let dst_norm = if parent_norm == "/" {
        format!("/{valid_name}")
    } else {
        format!("{parent_norm}/{valid_name}")
    };
    let dst_path = Path::new(&dst_norm);

    let src_name = src_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if src_name == valid_name {
        let meta = fs::metadata(src_path)
            .ok()
            .or_else(|| fs::symlink_metadata(src_path).ok());
        let is_dir = src_path.is_dir();
        return Ok(build_file_entry(valid_name, dst_norm, meta.as_ref(), is_dir));
    }

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

/// Copies file URLs into the macOS general pasteboard (`NSPasteboard`).
pub fn fs_copy_files_to_clipboard(paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Err("Не выбраны файлы для копирования".to_string());
    }
    let mut verified = Vec::with_capacity(paths.len());
    for raw in &paths {
        let norm = normalize_posix_path(raw)?;
        if fs::symlink_metadata(&norm).is_err() && !Path::new(&norm).exists() {
            return Err(format!("Элемент не найден: {norm}"));
        }
        verified.push(norm);
    }
    bridge_ffi::clipboard_set_files(&verified)
}

/// Safely moves files/folders/bundles into macOS Trash via `NSFileManager`.
pub fn fs_trash_items(paths: Vec<String>) -> Result<usize, String> {
    if paths.is_empty() {
        return Err("Не выбраны элементы для удаления".to_string());
    }
    let mut unique_paths = Vec::with_capacity(paths.len());
    let mut seen = HashSet::new();
    for raw in &paths {
        let norm = normalize_posix_path(raw)?;
        if norm == "/" {
            return Err("Нельзя удалить корневой раздел /".to_string());
        }
        if fs::symlink_metadata(&norm).is_err() && !Path::new(&norm).exists() {
            return Err(format!("Элемент не найден: {norm}"));
        }
        if seen.insert(norm.clone()) {
            unique_paths.push(norm);
        }
    }

    let mut trashed = 0usize;
    for p in &unique_paths {
        bridge_ffi::trash_item(p)?;
        trashed += 1;
    }

    {
        let mut cache = lock_unpoisoned(thumbnail_cache());
        for p in &unique_paths {
            cache.invalidate_path(p);
        }
    }

    Ok(trashed)
}

/// Reveals a file, folder, or `.app` bundle in macOS Finder (`open -R`).
pub fn fs_open_in_explorer(path: &str) -> Result<(), String> {
    let norm = normalize_posix_path(path)?;
    let p = Path::new(&norm);
    if !p.exists() && fs::symlink_metadata(p).is_err() {
        return Err(format!("Элемент не найден: {norm}"));
    }
    Command::new("open")
        .arg("-R")
        .arg(&norm)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Не удалось открыть Finder: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_posix_paths_and_tilde() {
        assert_eq!(
            normalize_posix_path_with_home("", "/Users/alex").unwrap(),
            "/Users/alex"
        );
        assert_eq!(
            normalize_posix_path_with_home("~", "/Users/alex").unwrap(),
            "/Users/alex"
        );
        assert_eq!(
            normalize_posix_path_with_home("~/Desktop//Work/", "/Users/alex").unwrap(),
            "/Users/alex/Desktop/Work"
        );
        assert_eq!(
            normalize_posix_path_with_home("///", "/Users/alex").unwrap(),
            "/"
        );
        assert_eq!(
            normalize_posix_path_with_home("/Users/alex/docs/../pics/", "/Users/alex").unwrap(),
            "/Users/alex/pics"
        );
        assert_eq!(
            normalize_posix_path_with_home("/../../", "/Users/alex").unwrap(),
            "/"
        );
        assert!(normalize_posix_path_with_home("relative/path", "/Users/alex").is_err());
    }

    #[test]
    fn computes_parents_and_breadcrumbs() {
        assert_eq!(compute_posix_parent_path("/"), None);
        assert_eq!(compute_posix_parent_path("/Users").as_deref(), Some("/"));
        assert_eq!(
            compute_posix_parent_path("/Users/alex/Desktop").as_deref(),
            Some("/Users/alex")
        );

        assert_eq!(
            build_posix_breadcrumbs("/"),
            vec![BreadcrumbPart {
                name: "Mac".into(),
                path: "/".into()
            }]
        );
        assert_eq!(
            build_posix_breadcrumbs("/Users/alex/Desktop"),
            vec![
                BreadcrumbPart {
                    name: "Mac".into(),
                    path: "/".into()
                },
                BreadcrumbPart {
                    name: "Users".into(),
                    path: "/Users".into()
                },
                BreadcrumbPart {
                    name: "alex".into(),
                    path: "/Users/alex".into()
                },
                BreadcrumbPart {
                    name: "Desktop".into(),
                    path: "/Users/alex/Desktop".into()
                },
            ]
        );
    }

    #[test]
    fn validates_posix_filenames() {
        assert_eq!(validate_posix_filename("  Notes.md ").unwrap(), "Notes.md");
        assert!(validate_posix_filename("").is_err());
        assert!(validate_posix_filename(".").is_err());
        assert!(validate_posix_filename("..").is_err());
        assert!(validate_posix_filename("a/b").is_err());
        assert!(validate_posix_filename("bad\0name").is_err());
    }

    #[test]
    fn treats_app_bundles_as_launchable_items_not_dirs() {
        let entry = build_file_entry(
            "Safari.app".to_string(),
            "/Applications/Safari.app".to_string(),
            None,
            true,
        );
        assert!(!entry.is_dir);
        assert_eq!(entry.category, "app");
        assert_eq!(entry.extension, "app");
        assert!(entry.has_preview);
    }

    #[test]
    fn natural_sorting_places_dirs_before_files_and_sorts_numbers() {
        let mut entries = vec![
            build_file_entry("file10.txt".into(), "/file10.txt".into(), None, false),
            build_file_entry("file2.txt".into(), "/file2.txt".into(), None, false),
            build_file_entry("Docs".into(), "/Docs".into(), None, true),
        ];
        sort_entries(&mut entries, None, None);
        assert_eq!(entries[0].name, "Docs");
        assert_eq!(entries[1].name, "file2.txt");
        assert_eq!(entries[2].name, "file10.txt");
    }
}
