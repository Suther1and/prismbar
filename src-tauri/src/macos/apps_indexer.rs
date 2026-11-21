use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use crate::macos::icon_cache::{self, IconCache};

pub const DEFAULT_USER_FILE_MAX_DEPTH: usize = 2;
pub const DEFAULT_USER_FILE_CAP: usize = 250;
pub const DEFAULT_USER_FILE_MAX_VISITED: usize = 4_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexedApp {
    pub id: String,
    pub name: String,
    pub category: String,
    pub target: String,
    pub aumid: Option<String>,
    pub icon_key: Option<String>,
    pub icon_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexedFile {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub location: String,
    pub target: String,
    pub modified_ts: u64,
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Categorizes a macOS application into Russian UI buckets.
pub fn categorize_app(name: &str, path: &str) -> String {
    let low = format!("{} {}", name.to_lowercase(), path.to_lowercase());
    if low.contains("safari")
        || low.contains("chrome")
        || low.contains("firefox")
        || low.contains("arc.app")
        || low.contains("zen")
        || low.contains("brave")
        || low.contains("edge")
        || low.contains("opera")
        || low.contains("vivaldi")
        || low.contains("orion")
        || low.contains("yandex")
        || low.contains("browser")
    {
        "Браузеры".to_string()
    } else if low.contains("xcode")
        || low.contains("code")
        || low.contains("cursor")
        || low.contains("antigravity")
        || low.contains("terminal")
        || low.contains("iterm")
        || low.contains("warp")
        || low.contains("ghostty")
        || low.contains("docker")
        || low.contains("postman")
        || low.contains("insomnia")
        || low.contains("intellij")
        || low.contains("webstorm")
        || low.contains("pycharm")
        || low.contains("rustrover")
        || low.contains("goland")
        || low.contains("clion")
        || low.contains("android studio")
        || low.contains("sublime")
        || low.contains("git")
        || low.contains("python")
    {
        "Разработка".to_string()
    } else if low.contains("telegram")
        || low.contains("discord")
        || low.contains("slack")
        || low.contains("zoom")
        || low.contains("whatsapp")
        || low.contains("teams")
        || low.contains("messages")
        || low.contains("facetime")
        || low.contains("mail")
        || low.contains("spark")
        || low.contains("thunderbird")
        || low.contains("signal")
        || low.contains("skype")
    {
        "Общение".to_string()
    } else if low.contains("spotify")
        || low.contains("music")
        || low.contains("vlc")
        || low.contains("iina")
        || low.contains("obs")
        || low.contains("quicktime")
        || low.contains("photos")
        || low.contains("photobooth")
        || low.contains("figma")
        || low.contains("photoshop")
        || low.contains("lightroom")
        || low.contains("blender")
        || low.contains("logic")
        || low.contains("garageband")
        || low.contains("final cut")
        || low.contains("davinci")
        || low.contains("podcasts")
        || low.contains("tv.app")
    {
        "Медиа".to_string()
    } else if low.contains("activity monitor")
        || low.contains("мониторинг")
        || low.contains("system settings")
        || low.contains("system preferences")
        || low.contains("настройки")
        || low.contains("calculator")
        || low.contains("notes")
        || low.contains("reminders")
        || low.contains("calendar")
        || low.contains("preview")
        || low.contains("finder")
        || low.contains("utility")
        || low.contains("utilities")
        || low.contains("keychain")
        || low.contains("console")
        || low.contains("automator")
        || low.contains("shortcuts")
        || low.contains("time machine")
        || low.contains("screenshot")
        || low.contains("1password")
        || low.contains("bitwarden")
        || low.contains("raycast")
        || low.contains("alfred")
    {
        "Утилиты".to_string()
    } else {
        "Прочее".to_string()
    }
}

/// Extracts a clean display name from `plutil -convert json` output, falling back to the `.app` stem.
pub fn extract_plist_app_name(json_str: &str, fallback_stem: &str) -> String {
    let clean_fallback = fallback_stem.trim().to_string();
    let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) else {
        return clean_fallback;
    };
    let is_usable = |s: &str| {
        let t = s.trim();
        !t.is_empty()
            && !t.contains("$(")
            && !t.eq_ignore_ascii_case("electron")
            && !t.eq_ignore_ascii_case("nwjs")
    };
    if let Some(display) = val.get("CFBundleDisplayName").and_then(|v| v.as_str()) {
        if is_usable(display) {
            return display.trim().to_string();
        }
    }
    if let Some(name) = val.get("CFBundleName").and_then(|v| v.as_str()) {
        if is_usable(name) {
            // Prefer a more descriptive `.app` folder name over generic short internal names
            if clean_fallback.len() > name.trim().len()
                && clean_fallback
                    .to_lowercase()
                    .contains(&name.trim().to_lowercase())
            {
                return clean_fallback;
            }
            return name.trim().to_string();
        }
    }
    clean_fallback
}

fn resolve_bundle_display_name(app_path: &Path) -> String {
    let fallback_stem = app_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "App".to_string());

    let plist_path = app_path.join("Contents/Info.plist");
    if !plist_path.is_file() {
        return fallback_stem;
    }

    let Ok(output) = Command::new("plutil")
        .arg("-convert")
        .arg("json")
        .arg("-o")
        .arg("-")
        .arg(&plist_path)
        .output()
    else {
        return fallback_stem;
    };

    if !output.status.success() {
        return fallback_stem;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    extract_plist_app_name(&json_str, &fallback_stem)
}

fn is_app_bundle_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().ends_with(".app"))
}

fn walk_app_bundles(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    visited_dirs: &mut HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }
    if let Ok(canon) = fs::canonicalize(dir) {
        if !visited_dirs.insert(canon) {
            return;
        }
    }
    let Ok(read_iter) = fs::read_dir(dir) else {
        return;
    };
    for entry in read_iter.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        if is_app_bundle_path(&path) {
            // Follow firmlinks/symlinks for `.app` bundles (e.g. /Applications/Safari.app),
            // and never recurse inside `.app` Contents/.
            if path.is_dir() {
                out.push(path);
            }
            continue;
        }
        if depth < max_depth {
            let is_real_dir = fs::symlink_metadata(&path)
                .map(|m| m.is_dir() && !m.file_type().is_symlink())
                .unwrap_or(false);
            if is_real_dir {
                walk_app_bundles(&path, depth + 1, max_depth, visited_dirs, out);
            }
        }
    }
}

fn system_settings_entries() -> Vec<IndexedApp> {
    let panes = [
        (
            "Настройки: Универсальный доступ",
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        ),
        (
            "Настройки: Bluetooth",
            "x-apple.systempreferences:com.apple.BluetoothSettings",
        ),
        (
            "Настройки: Экран",
            "x-apple.systempreferences:com.apple.Displays-Settings.extension",
        ),
        (
            "Настройки: Сеть",
            "x-apple.systempreferences:com.apple.Network-Settings.extension",
        ),
        (
            "Настройки: Звук",
            "x-apple.systempreferences:com.apple.Sound-Settings.extension",
        ),
        (
            "Настройки: Клавиатура",
            "x-apple.systempreferences:com.apple.Keyboard-Settings.extension",
        ),
    ];

    panes
        .iter()
        .enumerate()
        .map(|(i, (name, target))| IndexedApp {
            id: format!("syspref-{i}"),
            name: (*name).to_string(),
            category: "Утилиты".to_string(),
            target: (*target).to_string(),
            aumid: None,
            icon_key: None,
            icon_data: None,
        })
        .collect()
}

fn collect_installed_apps() -> Vec<IndexedApp> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }

    let mut bundle_paths = Vec::new();
    let mut visited_dirs = HashSet::new();
    for root in &roots {
        walk_app_bundles(root, 0, 2, &mut visited_dirs, &mut bundle_paths);
    }

    let finder_path = PathBuf::from("/System/Library/CoreServices/Finder.app");
    if finder_path.is_dir() {
        bundle_paths.push(finder_path);
    }

    let mut seen_targets = HashSet::new();
    let mut seen_names = HashSet::new();
    let mut apps = Vec::new();

    for bundle in bundle_paths {
        let target = bundle.to_string_lossy().trim_end_matches('/').to_string();
        let target_low = target.to_lowercase();
        if target_low.ends_with("/testbar.app") || !seen_targets.insert(target_low.clone()) {
            continue;
        }

        let mut name = resolve_bundle_display_name(&bundle);
        if name.eq_ignore_ascii_case("testbar") {
            continue;
        }
        if target == "/System/Applications/Utilities/Activity Monitor.app" {
            name = "Мониторинг системы (Activity Monitor)".to_string();
        }
        if !seen_names.insert(name.to_lowercase()) {
            continue;
        }

        let category = categorize_app(&name, &target);
        let icon_key = icon_cache::exe_key(&target);
        apps.push(IndexedApp {
            id: String::new(),
            name,
            category,
            target,
            aumid: None,
            icon_key: Some(icon_key),
            icon_data: None,
        });
    }

    apps.extend(system_settings_entries());

    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.target.to_lowercase().cmp(&b.target.to_lowercase()))
    });

    for (idx, app) in apps.iter_mut().enumerate() {
        if app.id.is_empty() {
            app.id = format!("app-{idx}");
        }
    }

    apps
}

/// Scans installed macOS `.app` bundles + System Settings panes without pre-warming icons.
pub fn scan_installed_apps() -> Vec<IndexedApp> {
    collect_installed_apps()
}

/// Scans installed macOS `.app` bundles, registers `icon_sources`, and pre-warms leading app icons.
pub fn scan_installed_apps_cached(
    cache: &Mutex<IconCache>,
    icon_sources: Option<&Mutex<HashMap<String, String>>>,
) -> Vec<IndexedApp> {
    let apps = collect_installed_apps();
    if let Some(sources) = icon_sources {
        let mut map = lock_unpoisoned(sources);
        for app in &apps {
            if let Some(ref key) = app.icon_key {
                map.insert(key.clone(), app.target.clone());
            }
        }
    }

    let mut warmed = 0usize;
    for app in &apps {
        if warmed >= 120 {
            break;
        }
        if app.icon_key.is_some()
            && icon_cache::extract_icon_for_path_cached(&app.target, cache).is_some()
        {
            warmed += 1;
        }
    }
    apps
}

/// Returns `true` if a directory name should be excluded during user file indexing.
pub fn is_excluded_user_dir_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.starts_with('.') || trimmed.starts_with('$') {
        return true;
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "node_modules"
            | "target"
            | "deriveddata"
            | "venv"
            | "__pycache__"
            | "dist"
            | "build"
            | "out"
            | ".git"
    )
}

/// Returns `true` if a filename should be excluded during user file indexing.
pub fn is_excluded_user_file_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('.')
        || trimmed.starts_with('~')
        || trimmed.starts_with("~$")
    {
        return true;
    }
    let low = trimmed.to_ascii_lowercase();
    if matches!(low.as_str(), ".ds_store" | "desktop.ini" | "thumbs.db") {
        return true;
    }
    low.ends_with(".tmp")
        || low.ends_with(".crdownload")
        || low.ends_with(".part")
        || low.ends_with(".download")
        || low.ends_with(".bak")
}

fn file_modified_ts(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn walk_user_files_root(
    dir: &Path,
    location: &str,
    depth: usize,
    visited: &mut usize,
    out: &mut Vec<IndexedFile>,
) {
    if depth > DEFAULT_USER_FILE_MAX_DEPTH
        || out.len() >= DEFAULT_USER_FILE_CAP
        || *visited >= DEFAULT_USER_FILE_MAX_VISITED
    {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut subdirs: Vec<PathBuf> = Vec::new();

    for entry in entries.flatten() {
        if out.len() >= DEFAULT_USER_FILE_CAP || *visited >= DEFAULT_USER_FILE_MAX_VISITED {
            break;
        }
        *visited += 1;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if is_excluded_user_file_name(&name) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }

        let is_dir = meta.is_dir();
        let is_app = is_dir && is_app_bundle_path(&path);
        if is_dir && !is_app && is_excluded_user_dir_name(&name) {
            continue;
        }

        let modified_ts = file_modified_ts(&meta);
        let kind = if is_dir && !is_app { "folder" } else { "file" };
        let id = format!("file-{}", out.len());
        out.push(IndexedFile {
            id,
            name,
            kind: kind.to_string(),
            location: location.to_string(),
            target: path.to_string_lossy().to_string(),
            modified_ts,
        });

        if is_dir && !is_app && depth < DEFAULT_USER_FILE_MAX_DEPTH {
            subdirs.push(path);
        }
    }

    for sub in subdirs {
        if out.len() >= DEFAULT_USER_FILE_CAP || *visited >= DEFAULT_USER_FILE_MAX_VISITED {
            break;
        }
        walk_user_files_root(&sub, location, depth + 1, visited, out);
    }
}

/// Scans `~/Desktop`, `~/Documents`, and `~/Downloads` up to depth 2 (max 250 items).
pub fn scan_user_files() -> Vec<IndexedFile> {
    let home = dirs::home_dir();
    let roots = [
        (
            "Рабочий стол",
            dirs::desktop_dir().or_else(|| home.as_ref().map(|h| h.join("Desktop"))),
        ),
        (
            "Документы",
            dirs::document_dir().or_else(|| home.as_ref().map(|h| h.join("Documents"))),
        ),
        (
            "Загрузки",
            dirs::download_dir().or_else(|| home.as_ref().map(|h| h.join("Downloads"))),
        ),
    ];

    let mut out = Vec::new();
    let mut visited = 0usize;
    for (label, path_opt) in roots {
        let Some(root) = path_opt else {
            continue;
        };
        if root.is_dir() {
            walk_user_files_root(&root, label, 0, &mut visited, &mut out);
        }
    }

    out.sort_by(|a, b| {
        b.modified_ts
            .cmp(&a.modified_ts)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    for (idx, item) in out.iter_mut().enumerate() {
        item.id = format!("file-{idx}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorizes_mac_apps_accurately() {
        assert_eq!(
            categorize_app("Safari", "/Applications/Safari.app"),
            "Браузеры"
        );
        assert_eq!(
            categorize_app("Visual Studio Code", "/Applications/Visual Studio Code.app"),
            "Разработка"
        );
        assert_eq!(
            categorize_app("Telegram", "/Applications/Telegram.app"),
            "Общение"
        );
        assert_eq!(categorize_app("Figma", "/Applications/Figma.app"), "Медиа");
        assert_eq!(
            categorize_app(
                "Activity Monitor",
                "/System/Applications/Utilities/Activity Monitor.app"
            ),
            "Утилиты"
        );
        assert_eq!(
            categorize_app("Notion", "/Applications/Notion.app"),
            "Прочее"
        );
    }

    #[test]
    fn extracts_plist_display_name_with_fallbacks() {
        let json = r#"{"CFBundleDisplayName":"Яндекс Браузер","CFBundleName":"Yandex"}"#;
        assert_eq!(extract_plist_app_name(json, "Yandex"), "Яндекс Браузер");

        let electron = r#"{"CFBundleName":"Electron"}"#;
        assert_eq!(extract_plist_app_name(electron, "MyApp"), "MyApp");

        let short = r#"{"CFBundleName":"Code"}"#;
        assert_eq!(
            extract_plist_app_name(short, "Visual Studio Code"),
            "Visual Studio Code"
        );

        assert_eq!(extract_plist_app_name("not-json", "Obsidian"), "Obsidian");
    }

    #[test]
    fn user_file_exclusions_filter_dev_noise() {
        assert!(is_excluded_user_dir_name("node_modules"));
        assert!(is_excluded_user_dir_name("DerivedData"));
        assert!(is_excluded_user_dir_name(".git"));
        assert!(!is_excluded_user_dir_name("Projects"));

        assert!(is_excluded_user_file_name(".DS_Store"));
        assert!(is_excluded_user_file_name("report.crdownload"));
        assert!(!is_excluded_user_file_name("report.pdf"));
    }
}
