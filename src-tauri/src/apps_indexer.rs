use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;
use windows::core::{Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER, STGM,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2, IShellLinkW,
    SHGetKnownFolderItem, ShellLink, KF_FLAG_DEFAULT, SIGDN_DESKTOPABSOLUTEPARSING,
    SIGDN_NORMALDISPLAY,
};

use crate::icon_cache::{self, IconCache};
use crate::win_shell::wide;
use crate::windows_tracker::{extract_icon_for_path_cached, ComGuard};

/// `PKEY_AppUserModel_ID` (`{9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}`, pid 5).
const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9F4C_2855_9F79_4B39_A8D0_E1D4_2DE1_D5F3),
    pid: 5,
};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
const MAX_GAME_COVER_BYTES: usize = 1_800_000;

pub const DEFAULT_USER_FILE_MAX_DEPTH: usize = 2;
pub const DEFAULT_USER_FILE_CAP: usize = 400;
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
    /// `"file"` | `"folder"`
    pub kind: String,
    /// `"Рабочий стол"` | `"Документы"` | `"Загрузки"`
    pub location: String,
    pub target: String,
    pub modified_ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGame {
    pub app_id: String,
    pub name: String,
    pub launcher: String,
    pub target: String,
    pub cover_data: Option<String>,
    #[serde(default)]
    pub cover_path: Option<String>,
    pub last_updated: u64,
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn pwstr_to_string_and_free(raw: PWSTR) -> Option<String> {
    if raw.0.is_null() {
        return None;
    }
    let text = unsafe {
        let s = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0 as *const std::ffi::c_void));
        s
    }?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Reusable `IShellLinkW` + `IPersistFile` COM pair so Start Menu indexing creates one COM object
/// per scan instead of calling `CoCreateInstance` hundreds of times.
struct LnkResolver {
    link: IShellLinkW,
    persist: IPersistFile,
}

impl LnkResolver {
    fn new() -> Option<Self> {
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let persist: IPersistFile = link.cast().ok()?;
            Some(Self { link, persist })
        }
    }

    fn resolve_target_args_and_workdir(&self, lnk_path: &Path) -> Option<(String, String, String)> {
        unsafe {
            let w = wide(&lnk_path.to_string_lossy());
            self.persist.Load(PCWSTR(w.as_ptr()), STGM(0)).ok()?;

            let mut path_buf = [0u16; 1024];
            let mut fd = std::mem::zeroed();
            self.link.GetPath(&mut path_buf, &mut fd, 0).ok()?;
            let path_len = path_buf
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(path_buf.len());
            let resolved = String::from_utf16_lossy(&path_buf[..path_len])
                .trim()
                .to_string();
            if resolved.is_empty() {
                return None;
            }

            let mut args_buf = [0u16; 512];
            let args = if self.link.GetArguments(&mut args_buf).is_ok() {
                let args_len = args_buf
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(args_buf.len());
                String::from_utf16_lossy(&args_buf[..args_len])
                    .trim()
                    .to_string()
            } else {
                String::new()
            };

            let mut wd_buf = [0u16; 1024];
            let work_dir = if self.link.GetWorkingDirectory(&mut wd_buf).is_ok() {
                let wd_len = wd_buf.iter().position(|&c| c == 0).unwrap_or(wd_buf.len());
                String::from_utf16_lossy(&wd_buf[..wd_len])
                    .trim()
                    .to_string()
            } else {
                String::new()
            };

            Some((resolved, args, work_dir))
        }
    }
}

fn normalize_dir_for_compare(dir: &str) -> String {
    let normalized = dir.trim().trim_matches('"').replace('/', "\\");
    let stripped = if normalized.len() > 3 {
        normalized.trim_end_matches('\\')
    } else {
        normalized.as_str()
    };
    stripped.to_ascii_lowercase()
}

/// Returns true when a Start Menu `.lnk` shortcut must be kept as the launch target instead of
/// unwrapping to its bare `.exe` — either because it carries CLI arguments or because it specifies
/// a custom `WorkingDirectory` different from the `.exe`'s own folder.
pub fn should_keep_lnk_target(exe: &str, args: &str, working_dir: &str) -> bool {
    if !args.trim().is_empty() {
        return true;
    }
    let wd_norm = normalize_dir_for_compare(working_dir);
    if wd_norm.is_empty() {
        return false;
    }
    let exe_parent = Path::new(exe)
        .parent()
        .map(|p| normalize_dir_for_compare(&p.to_string_lossy()))
        .unwrap_or_default();
    wd_norm != exe_parent
}

pub fn is_ignored_app_name(name: &str) -> bool {
    let low = name.trim().to_lowercase();
    if low.is_empty() {
        return true;
    }
    low.contains("uninstall")
        || low.contains("удалить")
        || low.contains("деинсталл")
        || low.contains("readme")
        || low.contains("release notes")
        || low.contains("license")
        || low.contains("лиценз")
        || low.contains("website")
        || low.contains("web site")
        || low.contains("documentation")
        || low.contains("manual")
        || low.contains("справка")
        || low == "help"
        || low.ends_with(" help")
}

pub fn is_packaged_uwp_aumid(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    let candidate = lower.strip_prefix("shell:appsfolder\\").unwrap_or(&lower);
    if candidate.is_empty()
        || candidate.contains(":\\")
        || candidate.contains(":/")
        || candidate.starts_with('{')
        || candidate.starts_with("http://")
        || candidate.starts_with("https://")
    {
        return false;
    }
    if let Some((pkg, app_id)) = candidate.split_once('!') {
        !pkg.trim().is_empty() && !app_id.trim().is_empty()
    } else {
        false
    }
}

pub fn categorize_app(name: &str, path: &str) -> String {
    let low = format!("{} {}", name.to_lowercase(), path.to_lowercase());
    if low.contains("browser")
        || low.contains("chrome")
        || low.contains("edge")
        || low.contains("firefox")
        || low.contains("zen")
        || low.contains("yandex")
        || low.contains("brave")
        || low.contains("opera")
    {
        "Интернет".to_string()
    } else if low.contains("telegram")
        || low.contains("discord")
        || low.contains("spark")
        || low.contains("slack")
        || low.contains("zoom")
        || low.contains("whatsapp")
        || low.contains("teams")
        || low.contains("mail")
        || low.contains("почта")
    {
        "Общение".to_string()
    } else if low.contains("code")
        || low.contains("cursor")
        || low.contains("antigravity")
        || low.contains("studio")
        || low.contains("unity")
        || low.contains("git")
        || low.contains("terminal")
        || low.contains("терминал")
        || low.contains("warp")
        || low.contains("python")
        || low.contains("powershell")
        || low.contains("docker")
    {
        "Разработка".to_string()
    } else if low.contains("photoshop")
        || low.contains("lightroom")
        || low.contains("figma")
        || low.contains("blender")
        || low.contains("adobe")
        || low.contains("paint")
        || low.contains("illustrator")
    {
        "Дизайн".to_string()
    } else if low.contains("claude")
        || low.contains("comfy")
        || low.contains("ollama")
        || low.contains("gpt")
        || low.contains("copilot")
    {
        "AI".to_string()
    } else if low.contains("steam")
        || low.contains("battle.net")
        || low.contains("epic")
        || low.contains("riot")
        || low.contains("xbox")
        || low.contains("game")
        || low.contains("play")
    {
        "Игры".to_string()
    } else if low.contains("music")
        || low.contains("музыка")
        || low.contains("obs")
        || low.contains("vlc")
        || low.contains("spotify")
        || low.contains("photos")
        || low.contains("фотограф")
        || low.contains("media player")
        || low.contains("медиапроигр")
    {
        "Медиа".to_string()
    } else if low.contains("immersivecontrolpanel")
        || low.contains("параметры")
        || low.contains("settings")
        || low.contains("calculator")
        || low.contains("калькулятор")
        || low.contains("windowsstore")
        || low.contains("microsoft store")
        || low.contains("notepad")
        || low.contains("блокнот")
        || low.contains("snipping")
        || low.contains("ножницы")
        || low.contains("clock")
        || low.contains("часы")
        || low.contains("sechealthui")
        || low.contains("безопасност")
        || low.contains("taskmgr")
        || low.contains("explorer")
        || low.contains("проводник")
    {
        "Система".to_string()
    } else {
        "Программы".to_string()
    }
}

fn walk_start_menu(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 4 || !dir.exists() {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk_start_menu(&p, out, depth + 1);
            } else if p
                .extension()
                .map(|e| e.eq_ignore_ascii_case("lnk"))
                .unwrap_or(false)
            {
                out.push(p);
            }
        }
    }
}

/// Discovers packaged UWP/MSIX apps from `FOLDERID_AppsFolder` via `SHGetKnownFolderItem`
/// + `IEnumShellItems` using an RAII `ComGuard` (AD-5).
fn enumerate_uwp_apps_folder() -> Vec<IndexedApp> {
    let com = ComGuard::init_sta();
    if !com.is_usable() {
        return Vec::new();
    }

    let folder: IShellItem = match unsafe {
        SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
    } {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let enumerator: IEnumShellItems = match unsafe { folder.BindToHandler(None, &BHID_EnumItems) } {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    let mut batch: [Option<IShellItem>; 32] = Default::default();

    loop {
        let mut fetched = 0u32;
        let res = unsafe { enumerator.Next(&mut batch, Some(&mut fetched)) };
        let count = (fetched as usize).min(batch.len());
        if count == 0 {
            break;
        }
        for slot in batch.iter_mut().take(count) {
            let Some(item) = slot.take() else {
                continue;
            };
            let Some(name) = unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY).ok() }
                .and_then(pwstr_to_string_and_free)
            else {
                continue;
            };
            if is_ignored_app_name(&name) {
                continue;
            }

            let raw_aumid = item
                .cast::<IShellItem2>()
                .ok()
                .and_then(|item2| unsafe { item2.GetString(&PKEY_APP_USER_MODEL_ID).ok() })
                .and_then(pwstr_to_string_and_free)
                .or_else(|| {
                    unsafe { item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING).ok() }
                        .and_then(pwstr_to_string_and_free)
                });

            let Some(aumid) = raw_aumid.filter(|val| is_packaged_uwp_aumid(val)) else {
                continue;
            };

            let target = format!("shell:AppsFolder\\{aumid}");
            let icon_key = icon_cache::aumid_key(&aumid);
            let category = categorize_app(&name, &aumid);
            out.push(IndexedApp {
                id: String::new(),
                name,
                category,
                target,
                aumid: Some(aumid),
                icon_key: Some(icon_key),
                icon_data: None,
            });
        }
        if res.is_err() || count < batch.len() {
            break;
        }
    }

    out
}

/// Pure, testable merger/deduplicator combining Start Menu `.lnk` apps and packaged UWP
/// `shell:AppsFolder` apps with NO artificial 180-item truncation cap.
pub fn merge_and_deduplicate_apps(
    lnk_apps: Vec<IndexedApp>,
    uwp_apps: Vec<IndexedApp>,
) -> Vec<IndexedApp> {
    let mut merged: Vec<IndexedApp> = Vec::with_capacity(lnk_apps.len() + uwp_apps.len());
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut seen_targets: HashSet<String> = HashSet::new();
    let mut seen_aumids: HashSet<String> = HashSet::new();

    for app in lnk_apps {
        let name_trimmed = app.name.trim();
        let target_trimmed = app.target.trim();
        if is_ignored_app_name(name_trimmed) || target_trimmed.is_empty() {
            continue;
        }
        let name_key = name_trimmed.to_lowercase();
        let target_key = target_trimmed.replace('/', "\\").to_lowercase();
        if by_name.contains_key(&name_key) || !seen_targets.insert(target_key) {
            continue;
        }
        if let Some(ref aumid) = app.aumid {
            let aumid_key = aumid.trim().to_lowercase();
            if !aumid_key.is_empty() && !seen_aumids.insert(aumid_key) {
                continue;
            }
        }
        let idx = merged.len();
        by_name.insert(name_key, idx);
        merged.push(app);
    }

    for uwp in uwp_apps {
        let name_trimmed = uwp.name.trim();
        let target_trimmed = uwp.target.trim();
        if is_ignored_app_name(name_trimmed) || target_trimmed.is_empty() {
            continue;
        }
        let name_key = name_trimmed.to_lowercase();
        let target_key = target_trimmed.replace('/', "\\").to_lowercase();
        let aumid_key = uwp
            .aumid
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        if let Some(ref ak) = aumid_key {
            if seen_aumids.contains(ak) {
                continue;
            }
        }
        if seen_targets.contains(&target_key) {
            continue;
        }

        if let Some(&existing_idx) = by_name.get(&name_key) {
            let existing = &mut merged[existing_idx];
            // If the Start Menu entry didn't resolve to a real .exe (e.g. a bare .lnk wrapper
            // for a Store/UWP app), upgrade it to the packaged UWP entry; otherwise attach AUMID
            // metadata so Win32 + UWP icon/matching stays unified without creating a duplicate row.
            let existing_is_exe = existing.target.to_ascii_lowercase().ends_with(".exe");
            if !existing_is_exe {
                *existing = uwp;
            } else if existing.aumid.is_none() && uwp.aumid.is_some() {
                existing.aumid = uwp.aumid;
            }
            if let Some(ak) = aumid_key {
                seen_aumids.insert(ak);
            }
            seen_targets.insert(target_key);
            continue;
        }

        if let Some(ak) = aumid_key {
            seen_aumids.insert(ak);
        }
        seen_targets.insert(target_key);
        let idx = merged.len();
        by_name.insert(name_key, idx);
        merged.push(uwp);
    }

    merged.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.target.to_lowercase().cmp(&b.target.to_lowercase()))
    });

    for (idx, app) in merged.iter_mut().enumerate() {
        app.id = if app.aumid.is_some() {
            format!("uwp-{idx}")
        } else {
            format!("app-{idx}")
        };
    }

    merged
}

/// Scans all installed Start Menu `.lnk` apps and packaged UWP `shell:AppsFolder` apps with no
/// 180-app truncation cap, populating unified `IconCache` and registering `icon_sources` so
/// `get_icons` can hydrate any evicted icon on demand (AD-5, AD-6).
pub fn scan_installed_apps_cached(
    cache: &Mutex<IconCache>,
    icon_sources: Option<&Mutex<HashMap<String, String>>>,
) -> Vec<IndexedApp> {
    let mut roots = vec![PathBuf::from(
        r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs",
    )];
    if let Some(roaming) = dirs::config_dir() {
        roots.push(roaming.join(r"Microsoft\Windows\Start Menu\Programs"));
    }

    let mut lnks = Vec::new();
    for r in &roots {
        walk_start_menu(r, &mut lnks, 0);
    }

    let mut lnk_apps = Vec::new();
    let mut icon_paths: HashMap<String, String> = HashMap::new();

    {
        let com = ComGuard::init_sta();
        let lnk_resolver = if com.is_usable() {
            LnkResolver::new()
        } else {
            None
        };
        for lnk in lnks {
            let name = match lnk.file_stem().map(|s| s.to_string_lossy().to_string()) {
                Some(n) if !n.trim().is_empty() => n.trim().to_string(),
                _ => continue,
            };
            if is_ignored_app_name(&name) {
                continue;
            }

            let lnk_str = lnk.to_string_lossy().to_string();
            let resolved = lnk_resolver
                .as_ref()
                .and_then(|resolver| resolver.resolve_target_args_and_workdir(&lnk));

            let (target, icon_source) = match resolved {
                Some((exe, args, work_dir))
                    if exe.to_ascii_lowercase().ends_with(".exe") && Path::new(&exe).exists() =>
                {
                    // Keep the .lnk when shortcut carries required CLI arguments (e.g. Discord Update.exe)
                    // or a custom WorkingDirectory different from the .exe's folder.
                    if !should_keep_lnk_target(&exe, &args, &work_dir) {
                        (exe.clone(), exe)
                    } else {
                        (lnk_str.clone(), lnk_str)
                    }
                }
                _ => (lnk_str.clone(), lnk_str),
            };

            let icon_key = icon_cache::path_icon_key(&icon_source);
            icon_paths.insert(icon_key.clone(), icon_source);
            let category = categorize_app(&name, &target);

            lnk_apps.push(IndexedApp {
                id: String::new(),
                name,
                category,
                target,
                aumid: None,
                icon_key: Some(icon_key),
                icon_data: None,
            });
        }
    }

    let uwp_apps = enumerate_uwp_apps_folder();
    for uwp in &uwp_apps {
        if let Some(ref key) = uwp.icon_key {
            icon_paths.insert(key.clone(), uwp.target.clone());
        }
    }

    let apps = merge_and_deduplicate_apps(lnk_apps, uwp_apps);

    // Register every discovered icon source so `get_icons` can re-hydrate any LRU-evicted key
    if let Some(sources) = icon_sources {
        let mut map = lock_unpoisoned(sources);
        for app in &apps {
            if let Some(ref key) = app.icon_key {
                let src = icon_paths
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| app.target.clone());
                map.insert(key.clone(), src);
            }
        }
    }

    // Pre-warm UWP icons and leading Win32 app icons into unified IconCache without thrashing
    // the 256-entry LRU budget (remaining keys hydrate lazily on demand via `get_icons`).
    let mut prewarmed = 0usize;
    let prewarm_limit = 140usize;
    for app in &apps {
        if prewarmed >= prewarm_limit {
            break;
        }
        let Some(ref key) = app.icon_key else {
            continue;
        };
        let src = icon_paths
            .get(key)
            .map(String::as_str)
            .unwrap_or(&app.target);
        if app.aumid.is_some() || prewarmed < 80 {
            if extract_icon_for_path_cached(src, cache).is_some() {
                prewarmed += 1;
            }
        }
    }

    apps
}

pub fn scan_installed_apps() -> Vec<IndexedApp> {
    let cache = Mutex::new(IconCache::default());
    scan_installed_apps_cached(&cache, None)
}

/// Returns true if a directory name should be excluded from user file indexing.
pub fn is_excluded_user_dir_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.starts_with('.') || trimmed.starts_with('$') {
        return true;
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "node_modules"
            | "target"
            | "venv"
            | "__pycache__"
            | "dist"
            | "build"
            | "out"
            | "appdata"
            | "system volume information"
            | "windowsapps"
    )
}

/// Returns true if a file name should be excluded from user file indexing (hidden/temp/system).
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
    if matches!(low.as_str(), "desktop.ini" | "thumbs.db" | "ntuser.dat") {
        return true;
    }
    low.ends_with(".tmp")
        || low.ends_with(".crdownload")
        || low.ends_with(".part")
        || low.ends_with(".bak")
}

fn file_modified_ts(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_reparse_point_or_symlink(entry: &fs::DirEntry, path: &Path) -> Option<(bool, Option<fs::Metadata>)> {
    let ft = entry.file_type().ok()?;
    if ft.is_symlink() {
        return None;
    }
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
    {
        return None;
    }
    Some((ft.is_dir(), Some(meta)))
}

fn walk_user_files(
    dir: &Path,
    location: &str,
    depth: usize,
    max_depth: usize,
    max_visited: usize,
    visited: &mut usize,
    out: &mut Vec<IndexedFile>,
    seen_targets: &mut HashSet<String>,
) {
    if depth > max_depth || *visited >= max_visited || !dir.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *visited >= max_visited {
            return;
        }
        *visited += 1;

        let path = entry.path();
        let Some(name_os) = path.file_name() else {
            continue;
        };
        let name = name_os.to_string_lossy().trim().to_string();
        if name.is_empty() {
            continue;
        }
        // Use `entry.file_type()` + `symlink_metadata` (never `entry.metadata()` which follows links)
        // so Windows NTFS directory junctions (My Music / My Pictures / My Videos) & symlinks are skipped.
        let Some((is_dir, meta_opt)) = is_reparse_point_or_symlink(&entry, &path) else {
            continue;
        };
        if is_dir {
            if is_excluded_user_dir_name(&name) {
                continue;
            }
        } else if is_excluded_user_file_name(&name) {
            continue;
        }

        let target = path.to_string_lossy().to_string();
        let target_key = target.replace('/', "\\").to_lowercase();
        let modified_ts = meta_opt.as_ref().map(file_modified_ts).unwrap_or(0);
        if seen_targets.insert(target_key) {
            out.push(IndexedFile {
                id: String::new(),
                name,
                kind: if is_dir {
                    "folder".to_string()
                } else {
                    "file".to_string()
                },
                location: location.to_string(),
                target,
                modified_ts,
            });
        }

        if is_dir && depth < max_depth && *visited < max_visited {
            walk_user_files(
                &path,
                location,
                depth + 1,
                max_depth,
                max_visited,
                visited,
                out,
                seen_targets,
            );
        }
    }
}

/// Pure, testable user-files scanner across caller-specified root folders with a hard traversal cap.
pub fn scan_user_files_in_roots_bounded(
    roots: &[(&str, PathBuf)],
    max_depth: usize,
    max_items: usize,
    max_visited: usize,
) -> Vec<IndexedFile> {
    let mut out = Vec::new();
    let mut seen_targets = HashSet::new();
    let mut visited = 0usize;

    for (location, root) in roots {
        if visited >= max_visited {
            break;
        }
        if root.is_dir() {
            walk_user_files(
                root,
                location,
                0,
                max_depth,
                max_visited,
                &mut visited,
                &mut out,
                &mut seen_targets,
            );
        }
    }

    out.sort_by(|a, b| {
        b.modified_ts
            .cmp(&a.modified_ts)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    if out.len() > max_items {
        out.truncate(max_items);
    }
    for (idx, item) in out.iter_mut().enumerate() {
        item.id = format!("ufile-{idx}");
    }
    out
}

/// Pure, testable user-files scanner across caller-specified root folders.
/// Respects `max_depth`, excludes `node_modules`/`.git`/`target`/hidden/temp/junction entries,
/// enforces a 4,000-entry traversal cap, sorts newest-modified first, and caps at `max_items`.
pub fn scan_user_files_in_roots(
    roots: &[(&str, PathBuf)],
    max_depth: usize,
    max_items: usize,
) -> Vec<IndexedFile> {
    scan_user_files_in_roots_bounded(roots, max_depth, max_items, DEFAULT_USER_FILE_MAX_VISITED)
}

/// Scans `Desktop`, `Documents`, and `Downloads` (depth <= 2, capped at 400 recent items)
/// for instant type-anywhere search in TestBar Start Menu.
pub fn scan_user_files() -> Vec<IndexedFile> {
    let mut roots: Vec<(&str, PathBuf)> = Vec::with_capacity(3);
    if let Some(desktop) = dirs::desktop_dir() {
        roots.push(("Рабочий стол", desktop));
    }
    if let Some(docs) = dirs::document_dir() {
        roots.push(("Документы", docs));
    }
    if let Some(downloads) = dirs::download_dir() {
        roots.push(("Загрузки", downloads));
    }
    scan_user_files_in_roots(&roots, DEFAULT_USER_FILE_MAX_DEPTH, DEFAULT_USER_FILE_CAP)
}

#[derive(Debug, PartialEq, Eq)]
enum VdfToken {
    Str(String),
    LBrace,
    RBrace,
}

fn tokenize_vdf(content: &str) -> Vec<VdfToken> {
    let mut tokens = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // Line comments: // ...
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'{' {
            tokens.push(VdfToken::LBrace);
            i += 1;
            continue;
        }
        if b == b'}' {
            tokens.push(VdfToken::RBrace);
            i += 1;
            continue;
        }
        if b == b'"' {
            i += 1;
            let mut out = String::new();
            let mut chars = content[i..].char_indices();
            let mut consumed = 0usize;
            while let Some((idx, ch)) = chars.next() {
                consumed = idx + ch.len_utf8();
                if ch == '\\' {
                    if let Some((nidx, next_ch)) = chars.next() {
                        consumed = nidx + next_ch.len_utf8();
                        match next_ch {
                            '\\' => out.push('\\'),
                            '"' => out.push('"'),
                            'n' => out.push('\n'),
                            't' => out.push('\t'),
                            other => {
                                out.push('\\');
                                out.push(other);
                            }
                        }
                        continue;
                    }
                }
                if ch == '"' {
                    break;
                }
                out.push(ch);
            }
            i += consumed;
            tokens.push(VdfToken::Str(out));
            continue;
        }
        // Unquoted token fallback
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'{'
            && bytes[i] != b'}'
            && bytes[i] != b'"'
        {
            i += 1;
        }
        tokens.push(VdfToken::Str(content[start..i].to_string()));
    }

    tokens
}

fn collect_vdf_kv_pairs(content: &str) -> Vec<(String, String)> {
    let tokens = tokenize_vdf(content);
    let mut pairs = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        if let VdfToken::Str(ref key) = tokens[i] {
            if let Some(VdfToken::Str(ref val)) = tokens.get(i + 1) {
                pairs.push((key.clone(), val.clone()));
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    pairs
}

fn normalize_steam_dir_str(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut norm = trimmed.replace('/', "\\");
    while norm.len() > 3 && norm.ends_with('\\') {
        norm.pop();
    }
    let path = PathBuf::from(&norm);
    if norm.to_ascii_lowercase().ends_with(".exe") {
        path.parent().map(Path::to_path_buf)
    } else {
        Some(path)
    }
}

fn read_registry_sz(root: HKEY, subkey: &str, value_name: &str) -> Option<String> {
    let subkey_w = wide(subkey);
    let value_w = wide(value_name);
    let mut byte_len: u32 = 0;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            PCWSTR(value_w.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut byte_len),
        )
    };
    if status != ERROR_SUCCESS || byte_len < 2 || byte_len > 16_384 {
        return None;
    }
    let mut buf = vec![0u16; (byte_len as usize / 2) + 2];
    let mut actual_bytes = (buf.len() * 2) as u32;
    let status2 = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            PCWSTR(value_w.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut actual_bytes),
        )
    };
    if status2 != ERROR_SUCCESS {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let text = String::from_utf16_lossy(&buf[..end]).trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Resolves ordered, deduplicated Steam root candidates, always placing Windows Registry
/// paths ahead of standard fallback directories (AD-7).
pub fn resolve_steam_root_candidates(
    registry_paths: &[Option<String>],
    fallback_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for candidate in registry_paths
        .iter()
        .flatten()
        .filter_map(|raw| normalize_steam_dir_str(raw))
        .chain(
            fallback_paths
                .iter()
                .filter_map(|p| normalize_steam_dir_str(&p.to_string_lossy())),
        )
    {
        let key = candidate.to_string_lossy().to_ascii_lowercase();
        if seen.insert(key) {
            out.push(candidate);
        }
    }

    out
}

fn default_steam_fallback_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\Program Files (x86)\Steam"),
        PathBuf::from(r"C:\Program Files\Steam"),
        PathBuf::from(r"C:\Steam"),
        PathBuf::from(r"D:\Steam"),
        PathBuf::from(r"E:\Steam"),
    ]
}

fn query_registry_steam_paths() -> Vec<Option<String>> {
    vec![
        read_registry_sz(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        read_registry_sz(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamExe"),
        read_registry_sz(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        read_registry_sz(HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ]
}

fn all_steam_root_candidates() -> Vec<PathBuf> {
    resolve_steam_root_candidates(&query_registry_steam_paths(), &default_steam_fallback_paths())
}

/// Dynamically detects the installed Steam root directory via Windows Registry
/// (`HKCU\Software\Valve\Steam\SteamPath`, `HKLM\SOFTWARE\WOW6432Node\Valve\Steam\InstallPath`)
/// with standard fallback paths.
pub fn detect_steam_root() -> Option<PathBuf> {
    all_steam_root_candidates()
        .into_iter()
        .find(|dir| dir.is_dir())
}

/// Dynamically detects `steam.exe` via Registry `SteamPath` + fallback locations.
pub fn detect_steam_exe() -> Option<String> {
    for root in all_steam_root_candidates() {
        let exe = root.join("steam.exe");
        if exe.is_file() {
            return Some(exe.to_string_lossy().to_string());
        }
    }
    None
}

/// Parses both modern (`"path" "D:\\SteamLibrary"`) and legacy (`"1" "E:\\SteamLib"`)
/// `libraryfolders.vdf` formats into normalized, deduplicated library directory paths.
pub fn parse_libraryfolders_vdf(content: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    for (key, val) in collect_vdf_kv_pairs(content) {
        let is_modern_path = key.eq_ignore_ascii_case("path");
        let is_legacy_slot = !key.is_empty()
            && key.chars().all(|c| c.is_ascii_digit())
            && (val.contains(":\\") || val.contains(":/") || val.starts_with("\\\\"));
        if !is_modern_path && !is_legacy_slot {
            continue;
        }
        if let Some(dir) = normalize_steam_dir_str(&val) {
            let norm_key = dir.to_string_lossy().to_ascii_lowercase();
            if seen.insert(norm_key) {
                dirs.push(dir);
            }
        }
    }

    dirs
}

/// Parses a Steam `appmanifest_<appid>.acf` file into `(app_id, name, last_activity_ts)`.
/// Skips Steamworks Common Redistributables (`appid == 228980`).
pub fn parse_appmanifest_acf(content: &str) -> Option<(String, String, u64)> {
    let mut app_id: Option<String> = None;
    let mut name: Option<String> = None;
    let mut last_updated: u64 = 0;
    let mut last_played: u64 = 0;

    for (key, val) in collect_vdf_kv_pairs(content) {
        if key.eq_ignore_ascii_case("appid") && app_id.is_none() {
            let id = val.trim().to_string();
            if !id.is_empty() {
                app_id = Some(id);
            }
        } else if key.eq_ignore_ascii_case("name") && name.is_none() {
            let clean = val.trim().to_string();
            if !clean.is_empty() {
                name = Some(clean);
            }
        } else if key.eq_ignore_ascii_case("lastupdated") {
            if let Ok(ts) = val.trim().parse::<u64>() {
                last_updated = last_updated.max(ts);
            }
        } else if key.eq_ignore_ascii_case("lastplayed") {
            if let Ok(ts) = val.trim().parse::<u64>() {
                last_played = last_played.max(ts);
            }
        }
    }

    let id = app_id?;
    if id == "228980" {
        return None;
    }
    let title = name?;
    Some((id, title, last_updated.max(last_played)))
}

fn detect_cover_image_mime(bytes: &[u8], path: &Path) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())?;
    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Safely reads an on-disk game cover image up to 1.8 MB and returns a `data:image/...;base64,...` URI.
/// Checks file size BEFORE allocating memory and validates magic bytes / image extension.
pub fn load_game_cover_data(cover_path: &str) -> Option<String> {
    let trimmed = cover_path.trim();
    if trimmed.is_empty()
        || trimmed.contains('\0')
        || trimmed.contains('\r')
        || trimmed.contains('\n')
    {
        return None;
    }
    let path = Path::new(trimmed);
    if !path.is_file() {
        return None;
    }
    let meta = fs::metadata(path).ok()?;
    let size = meta.len();
    if size == 0 || size > MAX_GAME_COVER_BYTES as u64 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_GAME_COVER_BYTES {
        return None;
    }
    let mime = detect_cover_image_mime(&bytes, path)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

/// Locates the best cached Steam cover image path on disk without reading any image bytes.
pub fn find_steam_cover_path(steam_root: &Path, app_id: &str) -> Option<PathBuf> {
    let cache_dir = steam_root.join("appcache").join("librarycache");
    let candidates = [
        cache_dir.join(app_id).join("library_hero.jpg"),
        cache_dir.join(format!("{app_id}_library_hero.jpg")),
        cache_dir.join(app_id).join("header.jpg"),
        cache_dir.join(format!("{app_id}_header.jpg")),
        cache_dir.join(app_id).join("library_600x900.jpg"),
        cache_dir.join(format!("{app_id}_library_600x900.jpg")),
    ];
    for p in candidates {
        if p.is_file() {
            return Some(p);
        }
    }
    // Also scan nested hashed subfolders inside appcache/librarycache/<appid>/
    let app_dir = cache_dir.join(app_id);
    if app_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&app_dir) {
            for e in entries.flatten() {
                let sub = e.path();
                if sub.is_dir() {
                    for fname in ["library_hero.jpg", "header.jpg", "library_600x900.jpg"] {
                        let cand = sub.join(fname);
                        if cand.is_file() {
                            return Some(cand);
                        }
                    }
                }
            }
        }
    }
    None
}

fn scan_steam_games_in_roots_mode(steam_roots: &[PathBuf], inline_covers: bool) -> Vec<SteamGame> {
    let mut primary_steam: Option<PathBuf> = None;
    let mut library_dirs: Vec<PathBuf> = Vec::new();
    let mut seen_libs: HashSet<String> = HashSet::new();

    let mut push_lib = |dir: PathBuf| {
        if dir.is_dir() {
            let key = dir.to_string_lossy().to_ascii_lowercase();
            if seen_libs.insert(key) {
                library_dirs.push(dir);
            }
        }
    };

    for root in steam_roots {
        if !root.is_dir() {
            continue;
        }
        if primary_steam.is_none() {
            primary_steam = Some(root.clone());
        }
        push_lib(root.clone());

        for vdf_rel in [
            Path::new("steamapps").join("libraryfolders.vdf"),
            Path::new("config").join("libraryfolders.vdf"),
        ] {
            let vdf = root.join(vdf_rel);
            if let Ok(content) = fs::read_to_string(&vdf) {
                for extra in parse_libraryfolders_vdf(&content) {
                    push_lib(extra);
                }
            }
        }
    }

    let mut games = Vec::new();
    let mut seen_app_ids = HashSet::new();

    for lib in &library_dirs {
        let steamapps = lib.join("steamapps");
        if let Ok(entries) = fs::read_dir(&steamapps) {
            for entry in entries.flatten() {
                let p = entry.path();
                let fname = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let fname_low = fname.to_ascii_lowercase();
                if !(fname_low.starts_with("appmanifest_") && fname_low.ends_with(".acf")) {
                    continue;
                }
                if let Ok(content) = fs::read_to_string(&p) {
                    let Some((app_id, name, last_updated)) = parse_appmanifest_acf(&content) else {
                        continue;
                    };
                    if !seen_app_ids.insert(app_id.clone()) {
                        continue;
                    }
                    let cover_path = primary_steam
                        .as_deref()
                        .and_then(|sr| find_steam_cover_path(sr, &app_id))
                        .or_else(|| find_steam_cover_path(lib, &app_id))
                        .map(|cp| cp.to_string_lossy().to_string());
                    let cover_data = if inline_covers {
                        cover_path.as_deref().and_then(load_game_cover_data)
                    } else {
                        None
                    };

                    games.push(SteamGame {
                        app_id: app_id.clone(),
                        name,
                        launcher: "Steam".to_string(),
                        target: format!("steam://rungameid/{app_id}"),
                        cover_data,
                        cover_path,
                        last_updated,
                    });
                }
            }
        }
    }

    games.sort_by(|a, b| {
        b.last_updated
            .cmp(&a.last_updated)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    games
}

/// Pure, testable multi-library Steam scanner given candidate Steam roots (legacy inline cover mode).
pub fn scan_steam_games_in_roots(steam_roots: &[PathBuf]) -> Vec<SteamGame> {
    scan_steam_games_in_roots_mode(steam_roots, true)
}

/// Pure, testable multi-library Steam scanner populating `cover_path` only (`cover_data: None`).
pub fn scan_steam_games_lightweight_in_roots(steam_roots: &[PathBuf]) -> Vec<SteamGame> {
    scan_steam_games_in_roots_mode(steam_roots, false)
}

pub fn scan_steam_games() -> Vec<SteamGame> {
    let candidates = all_steam_root_candidates();
    scan_steam_games_in_roots(&candidates)
}

/// Parses an Epic Games Launcher `.item` manifest JSON into a `SteamGame` entry.
/// Skips incomplete installs (`bIsIncompleteInstall == true`) and DLCs (`MainGameAppName != AppName`).
pub fn parse_epic_manifest_item(content: &str, fallback_ts: u64) -> Option<SteamGame> {
    let json: serde_json::Value = serde_json::from_str(content).ok()?;
    if json
        .get("bIsIncompleteInstall")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return None;
    }

    let app_name = json
        .get("AppName")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let display_name = json
        .get("DisplayName")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    if let Some(main_app) = json
        .get("MainGameAppName")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !main_app.eq_ignore_ascii_case(app_name) {
            return None;
        }
    }

    let ns = json
        .get("CatalogNamespace")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let item_id = json
        .get("CatalogItemId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let target = match (ns, item_id) {
        (Some(n), Some(i)) => {
            format!("com.epicgames.launcher://apps/{n}%3A{i}%3A{app_name}?action=launch&silent=true")
        }
        _ => format!("com.epicgames.launcher://apps/{app_name}?action=launch&silent=true"),
    };

    Some(SteamGame {
        app_id: format!("epic:{app_name}"),
        name: display_name.to_string(),
        launcher: "Epic Games".to_string(),
        target,
        cover_data: None,
        cover_path: None,
        last_updated: fallback_ts,
    })
}

/// Scans an Epic Games manifests folder (`*.item`) gracefully, returning empty if absent.
pub fn scan_epic_games_in_dir(dir: &Path) -> Vec<SteamGame> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut games = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries.flatten() {
        let p = entry.path();
        let is_item = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("item"))
            .unwrap_or(false);
        if !is_item || !p.is_file() {
            continue;
        }
        let fallback_ts = fs::metadata(&p).ok().map(|m| file_modified_ts(&m)).unwrap_or(0);
        if let Ok(content) = fs::read_to_string(&p) {
            if let Some(game) = parse_epic_manifest_item(&content, fallback_ts) {
                let key = game.app_id.to_ascii_lowercase();
                if seen.insert(key) {
                    games.push(game);
                }
            }
        }
    }
    games
}

struct RegKeyGuard(HKEY);

impl Drop for RegKeyGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
}

/// Pure builder turning GOG Galaxy registry string values into a `SteamGame` entry.
/// Skips DLC entries where `depends_on` is populated.
pub fn build_gog_game_entry(
    subkey_name: &str,
    game_id: Option<&str>,
    game_name: Option<&str>,
    exe: Option<&str>,
    install_path: Option<&str>,
    depends_on: Option<&str>,
) -> Option<SteamGame> {
    if depends_on
        .map(str::trim)
        .is_some_and(|dep| !dep.is_empty())
    {
        return None;
    }

    let id = game_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let s = subkey_name.trim();
            (!s.is_empty()).then_some(s)
        })?;
    let name = game_name.map(str::trim).filter(|s| !s.is_empty())?;

    let direct_exe = exe
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|raw| {
            let p = PathBuf::from(raw);
            if p.is_absolute() {
                p
            } else if let Some(base) = install_path.map(str::trim).filter(|s| !s.is_empty()) {
                Path::new(base).join(p)
            } else {
                p
            }
        })
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string());

    let target = direct_exe.unwrap_or_else(|| format!("goggalaxy://openGameView/{id}"));

    Some(SteamGame {
        app_id: format!("gog:{id}"),
        name: name.to_string(),
        launcher: "GOG".to_string(),
        target,
        cover_data: None,
        cover_path: None,
        last_updated: 0,
    })
}

fn scan_gog_registry_branch(root: HKEY, base_subkey: &str) -> Vec<SteamGame> {
    let mut hkey = HKEY::default();
    let subkey_w = wide(base_subkey);
    let open_status = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
    };
    if open_status != ERROR_SUCCESS || hkey.is_invalid() {
        return Vec::new();
    }
    let _guard = RegKeyGuard(hkey);

    let mut games = Vec::new();
    let mut index = 0u32;
    let mut name_buf = [0u16; 256];

    loop {
        let mut name_len = name_buf.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                hkey,
                index,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                PWSTR::null(),
                None,
                None,
            )
        };
        index = index.saturating_add(1);
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status != ERROR_SUCCESS {
            break;
        }
        let sub_name = String::from_utf16_lossy(&name_buf[..(name_len as usize).min(name_buf.len())])
            .trim()
            .to_string();
        if sub_name.is_empty() {
            continue;
        }
        let full_subkey = format!("{base_subkey}\\{sub_name}");
        let game_id = read_registry_sz(root, &full_subkey, "gameID");
        let game_name = read_registry_sz(root, &full_subkey, "gameName")
            .or_else(|| read_registry_sz(root, &full_subkey, "GAMENAME"));
        let exe = read_registry_sz(root, &full_subkey, "exe");
        let install_path = read_registry_sz(root, &full_subkey, "path");
        let depends_on = read_registry_sz(root, &full_subkey, "dependsOn");

        if let Some(game) = build_gog_game_entry(
            &sub_name,
            game_id.as_deref(),
            game_name.as_deref(),
            exe.as_deref(),
            install_path.as_deref(),
            depends_on.as_deref(),
        ) {
            games.push(game);
        }
    }

    games
}

fn scan_gog_games() -> Vec<SteamGame> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for branch in [
        r"SOFTWARE\WOW6432Node\GOG.com\Games",
        r"SOFTWARE\GOG.com\Games",
    ] {
        for g in scan_gog_registry_branch(HKEY_LOCAL_MACHINE, branch) {
            let key = g.app_id.to_ascii_lowercase();
            if seen.insert(key) {
                out.push(g);
            }
        }
    }
    out
}

/// Lightweight multi-launcher game discovery (Steam + Epic Games + GOG Galaxy).
/// Leaves `cover_data: None` and populates `cover_path` so IPC payloads stay ~15 KB
/// while UI hydrates covers lazily via `load_game_cover_data`.
pub fn scan_games_lightweight() -> Vec<SteamGame> {
    let mut combined = Vec::new();
    let mut seen = HashSet::new();

    let mut push_unique = |list: Vec<SteamGame>| {
        for game in list {
            let key = game.app_id.to_ascii_lowercase();
            if seen.insert(key) {
                combined.push(game);
            }
        }
    };

    push_unique(scan_steam_games_lightweight_in_roots(&all_steam_root_candidates()));
    push_unique(scan_epic_games_in_dir(Path::new(
        r"C:\ProgramData\Epic\EpicGamesLauncher\Data\Manifests",
    )));
    push_unique(scan_gog_games());

    combined.sort_by(|a, b| {
        b.last_updated
            .cmp(&a.last_updated)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    combined
}

pub fn pick_folder_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Выбери папку для закрепления в PrismBar")
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}

pub fn pick_file_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Выбери файл или программу для закрепления в PrismBar")
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use windows::Win32::Foundation::{E_FAIL, RPC_E_CHANGED_MODE, S_FALSE, S_OK};

    fn make_temp_dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let id = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "testbar-apps-indexer-{tag}-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn com_guard_honors_rpc_e_changed_mode_without_mismatched_uninit() {
        let ok = ComGuard::from_hresult(S_OK);
        assert!(ok.is_usable());
        assert!(ok.will_uninitialize());
        // Avoid calling CoUninitialize in synthetic test by forgetting after inspecting flags
        std::mem::forget(ok);

        let already_same = ComGuard::from_hresult(S_FALSE);
        assert!(already_same.is_usable());
        assert!(already_same.will_uninitialize());
        std::mem::forget(already_same);

        let changed = ComGuard::from_hresult(RPC_E_CHANGED_MODE);
        assert!(changed.is_usable());
        assert!(!changed.will_uninitialize());
        drop(changed);

        let failed = ComGuard::from_hresult(E_FAIL);
        assert!(!failed.is_usable());
        assert!(!failed.will_uninitialize());
        drop(failed);
    }

    #[test]
    fn uwp_aumid_classifier_accepts_packaged_and_rejects_paths_and_guids() {
        assert!(is_packaged_uwp_aumid(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        ));
        assert!(is_packaged_uwp_aumid(
            "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"
        ));
        assert!(is_packaged_uwp_aumid(
            "shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"
        ));
        assert!(!is_packaged_uwp_aumid(r"C:\Windows\notepad.exe"));
        assert!(!is_packaged_uwp_aumid(
            "{6D809377-6AF0-444B-8957-A3773F02200E}\\foo.exe"
        ));
        assert!(!is_packaged_uwp_aumid("https://example.com"));
        assert!(!is_packaged_uwp_aumid("no_bang_identifier"));
        assert!(!is_packaged_uwp_aumid(""));
    }

    #[test]
    fn merge_and_deduplicate_apps_handles_uwp_lnk_overlap_and_removes_180_cap() {
        let mut lnk_apps = Vec::new();
        for i in 0..210 {
            lnk_apps.push(IndexedApp {
                id: String::new(),
                name: format!("Tool {i:03}"),
                category: "Программы".to_string(),
                target: format!(r"C:\Tools\tool{i:03}.exe"),
                aumid: None,
                icon_key: Some(icon_cache::exe_key(&format!(r"C:\Tools\tool{i:03}.exe"))),
                icon_data: None,
            });
        }
        // Duplicate .lnk name + junk uninstall shortcut + bare .lnk wrapper for Calculator
        lnk_apps.push(IndexedApp {
            id: String::new(),
            name: "Tool 001".to_string(),
            category: "Программы".to_string(),
            target: r"C:\Other\tool001.exe".to_string(),
            aumid: None,
            icon_key: None,
            icon_data: None,
        });
        lnk_apps.push(IndexedApp {
            id: String::new(),
            name: "Uninstall Tool 001".to_string(),
            category: "Программы".to_string(),
            target: r"C:\Tools\unins000.exe".to_string(),
            aumid: None,
            icon_key: None,
            icon_data: None,
        });
        lnk_apps.push(IndexedApp {
            id: String::new(),
            name: "Calculator".to_string(),
            category: "Система".to_string(),
            target: r"C:\ProgramData\Start Menu\Calculator.lnk".to_string(),
            aumid: None,
            icon_key: None,
            icon_data: None,
        });

        let uwp_apps = vec![
            IndexedApp {
                id: String::new(),
                name: "Calculator".to_string(),
                category: "Система".to_string(),
                target: "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
                    .to_string(),
                aumid: Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".to_string()),
                icon_key: Some(icon_cache::aumid_key(
                    "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
                )),
                icon_data: None,
            },
            // Duplicate AUMID under localized name must not produce a second entry
            IndexedApp {
                id: String::new(),
                name: "Калькулятор".to_string(),
                category: "Система".to_string(),
                target: "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
                    .to_string(),
                aumid: Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".to_string()),
                icon_key: Some(icon_cache::aumid_key(
                    "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
                )),
                icon_data: None,
            },
            IndexedApp {
                id: String::new(),
                name: "Параметры".to_string(),
                category: "Система".to_string(),
                target: "shell:AppsFolder\\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel".to_string(),
                aumid: Some("windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel".to_string()),
                icon_key: Some(icon_cache::aumid_key("windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel")),
                icon_data: None,
            },
        ];

        let merged = merge_and_deduplicate_apps(lnk_apps, uwp_apps);

        // 210 unique Win32 tools + 2 unique UWP apps = 212 (> 180 proves no cap!)
        assert_eq!(merged.len(), 212);
        assert!(!merged.iter().any(|a| a.name.contains("Uninstall")));

        // Bare .lnk Calculator was upgraded to the real UWP AUMID entry without duplicating
        let calc_entries: Vec<_> = merged
            .iter()
            .filter(|a| a.aumid.as_deref() == Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"))
            .collect();
        assert_eq!(calc_entries.len(), 1);
        assert!(calc_entries[0]
            .target
            .starts_with("shell:AppsFolder\\Microsoft.WindowsCalculator"));
        assert!(calc_entries[0]
            .icon_key
            .as_deref()
            .is_some_and(|k| k.starts_with("aumid:")));
    }

    #[test]
    fn user_files_scanner_filters_excluded_dirs_hidden_temp_and_enforces_depth_and_cap() {
        let root = make_temp_dir("user-files");
        let docs = root.join("Documents");
        fs::create_dir_all(docs.join("Projects").join("Nested")).unwrap();
        fs::create_dir_all(docs.join("Projects").join("Nested").join("TooDeep")).unwrap();
        fs::create_dir_all(docs.join("node_modules").join("react")).unwrap();
        fs::create_dir_all(docs.join(".git").join("objects")).unwrap();
        fs::create_dir_all(docs.join("target").join("debug")).unwrap();

        // Included files at depth 0, 1, 2
        fs::write(docs.join("Report.pdf"), b"pdf").unwrap();
        fs::write(docs.join("Projects").join("Roadmap.md"), b"md").unwrap();
        fs::write(
            docs.join("Projects").join("Nested").join("Design.fig"),
            b"fig",
        )
        .unwrap();

        // Depth 3 file should be excluded when max_depth = 2
        fs::write(
            docs.join("Projects")
                .join("Nested")
                .join("TooDeep")
                .join("Deep.txt"),
            b"deep",
        )
        .unwrap();

        // Excluded directories and temp/hidden files
        fs::write(
            docs.join("node_modules").join("react").join("index.js"),
            b"js",
        )
        .unwrap();
        fs::write(docs.join(".git").join("HEAD"), b"ref").unwrap();
        fs::write(docs.join("target").join("debug").join("bin.exe"), b"exe").unwrap();
        fs::write(docs.join("desktop.ini"), b"ini").unwrap();
        fs::write(docs.join("Thumbs.db"), b"db").unwrap();
        fs::write(docs.join("~$Report.docx"), b"lock").unwrap();
        fs::write(docs.join(".hidden"), b"secret").unwrap();
        fs::write(docs.join("download.crdownload"), b"partial").unwrap();

        let files = scan_user_files_in_roots(&[("Документы", docs.clone())], 2, 400);
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();

        assert!(names.contains(&"Report.pdf"));
        assert!(names.contains(&"Projects"));
        assert!(names.contains(&"Roadmap.md"));
        assert!(names.contains(&"Nested"));
        assert!(names.contains(&"Design.fig"));

        assert!(!names.contains(&"Deep.txt"));
        assert!(!names.contains(&"node_modules"));
        assert!(!names.contains(&"index.js"));
        assert!(!names.contains(&".git"));
        assert!(!names.contains(&"HEAD"));
        assert!(!names.contains(&"target"));
        assert!(!names.contains(&"bin.exe"));
        assert!(!names.contains(&"desktop.ini"));
        assert!(!names.contains(&"Thumbs.db"));
        assert!(!names.contains(&"~$Report.docx"));
        assert!(!names.contains(&".hidden"));
        assert!(!names.contains(&"download.crdownload"));

        // Verify cap enforcement
        let capped = scan_user_files_in_roots(&[("Документы", docs)], 2, 2);
        assert_eq!(capped.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn real_uwp_apps_folder_enumeration_discovers_packaged_apps_with_icon_keys() {
        let uwp = enumerate_uwp_apps_folder();
        assert!(
            !uwp.is_empty(),
            "Windows 11 FOLDERID_AppsFolder should discover packaged UWP apps"
        );
        assert!(uwp.iter().all(|a| {
            a.aumid.is_some()
                && a.target.starts_with("shell:AppsFolder\\")
                && a.icon_key
                    .as_deref()
                    .is_some_and(|k| k.starts_with("aumid:"))
        }));
    }

    #[test]
    fn steam_registry_candidate_resolution_prioritizes_registry_and_deduplicates() {
        let candidates = resolve_steam_root_candidates(
            &[
                Some("G:/Games/Steam/".to_string()),
                Some(r"G:\Games\Steam\steam.exe".to_string()),
                Some(r"C:\Program Files (x86)\Steam".to_string()),
                None,
            ],
            &[
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\Steam"),
            ],
        );

        assert_eq!(
            candidates,
            vec![
                PathBuf::from(r"G:\Games\Steam"),
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\Steam"),
            ]
        );
    }

    #[test]
    fn steam_vdf_acf_and_multi_library_scanner_discovers_games_and_hero_covers() {
        // 1. Parse both modern `"path"` and legacy numeric slot `libraryfolders.vdf`
        let modern_vdf = r#"
"libraryfolders"
{
    "0"
    {
        "path"      "C:\\Program Files (x86)\\Steam"
        "label"     ""
    }
    "1"
    {
        "path"      "F:\\SteamLibrary"
    }
}
"#;
        let modern_dirs = parse_libraryfolders_vdf(modern_vdf);
        assert_eq!(
            modern_dirs,
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"F:\SteamLibrary"),
            ]
        );

        let legacy_vdf = r#"
"LibraryFolders"
{
    "TimeNextStatsReport" "1700000000"
    "1" "D:\\Games\\SteamLib"
    "2" "E:/ExtraSteam/"
}
"#;
        let legacy_dirs = parse_libraryfolders_vdf(legacy_vdf);
        assert_eq!(
            legacy_dirs,
            vec![
                PathBuf::from(r"D:\Games\SteamLib"),
                PathBuf::from(r"E:\ExtraSteam"),
            ]
        );

        // 2. End-to-end temp multi-library scan with modern VDF, ACF manifests, redist skip & covers
        let root = make_temp_dir("steam-multi-lib");
        let primary = root.join("PrimarySteam");
        let secondary = root.join("SecondDriveLib");
        fs::create_dir_all(primary.join("steamapps")).unwrap();
        fs::create_dir_all(primary.join("appcache").join("librarycache").join("2694490")).unwrap();
        fs::create_dir_all(secondary.join("steamapps")).unwrap();

        let sec_escaped = secondary.to_string_lossy().replace('\\', "\\\\");
        let prim_escaped = primary.to_string_lossy().replace('\\', "\\\\");
        fs::write(
            primary.join("steamapps").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n  \"0\" {{ \"path\" \"{prim_escaped}\" }}\n  \"1\" {{ \"path\" \"{sec_escaped}\" }}\n}}\n"
            ),
        )
        .unwrap();

        // Game 1 in primary library with nested library_hero.jpg
        fs::write(
            primary.join("steamapps").join("appmanifest_2694490.acf"),
            "\"AppState\"\n{\n  \"appid\" \"2694490\"\n  \"name\" \"Path of Exile 2\"\n  \"LastUpdated\" \"1732000000\"\n  \"LastPlayed\" \"1732500000\"\n}\n",
        )
        .unwrap();
        fs::write(
            primary
                .join("appcache")
                .join("librarycache")
                .join("2694490")
                .join("library_hero.jpg"),
            b"\xFF\xD8\xFFhero-bytes",
        )
        .unwrap();

        // Steamworks Redistributables (228980) must be skipped
        fs::write(
            primary.join("steamapps").join("appmanifest_228980.acf"),
            "\"AppState\"\n{\n  \"appid\" \"228980\"\n  \"name\" \"Steamworks Common Redistributables\"\n  \"LastUpdated\" \"1739999999\"\n}\n",
        )
        .unwrap();

        // Game 2 in secondary drive library with flat <appid>_library_600x900.jpg fallback in primary cache
        fs::write(
            secondary.join("steamapps").join("appmanifest_570.acf"),
            "\"AppState\"\n{\n  \"appid\" \"570\"\n  \"name\" \"Dota 2\"\n  \"LastUpdated\" \"1731000000\"\n}\n",
        )
        .unwrap();
        fs::write(
            primary
                .join("appcache")
                .join("librarycache")
                .join("570_library_600x900.jpg"),
            b"\xFF\xD8\xFFportrait-bytes",
        )
        .unwrap();

        let games = scan_steam_games_in_roots(&[primary.clone()]);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].app_id, "2694490");
        assert_eq!(games[0].name, "Path of Exile 2");
        assert_eq!(games[0].target, "steam://rungameid/2694490");
        assert_eq!(games[0].last_updated, 1_732_500_000);
        assert!(games[0]
            .cover_data
            .as_deref()
            .is_some_and(|u| u.starts_with("data:image/jpeg;base64,")));

        assert_eq!(games[1].app_id, "570");
        assert_eq!(games[1].name, "Dota 2");
        assert!(games[1]
            .cover_data
            .as_deref()
            .is_some_and(|u| u.starts_with("data:image/jpeg;base64,")));
        assert!(games[1].cover_path.is_some());

        // 3. Lightweight scan populates cover_path ONLY (cover_data == None), and load_game_cover_data hydrates lazily
        let light = scan_steam_games_lightweight_in_roots(&[primary]);
        assert_eq!(light.len(), 2);
        assert!(light.iter().all(|g| g.cover_data.is_none()));
        assert!(light.iter().all(|g| g.cover_path.is_some()));

        let hydrated = load_game_cover_data(light[0].cover_path.as_deref().unwrap());
        assert!(hydrated
            .as_deref()
            .is_some_and(|u| u.starts_with("data:image/jpeg;base64,")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn user_files_traversal_cap_bounds_large_directories() {
        let root = make_temp_dir("user-files-cap");
        for i in 0..35 {
            fs::write(root.join(format!("file_{i:02}.txt")), b"data").unwrap();
        }
        // Hard cap of 10 visited entries must stop traversal cleanly well below 35
        let bounded = scan_user_files_in_roots_bounded(&[("Документы", root.clone())], 2, 400, 10);
        assert_eq!(bounded.len(), 10);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn epic_and_gog_discovery_parses_games_and_skips_dlcs_and_incomplete_installs() {
        let dir = make_temp_dir("epic-gog");

        // Valid Epic game manifest
        let epic_ok = r#"{
            "DisplayName": "Alan Wake 2",
            "AppName": "AlanWake2App",
            "MainGameAppName": "AlanWake2App",
            "CatalogNamespace": "ns123",
            "CatalogItemId": "item456",
            "bIsIncompleteInstall": false
        }"#;
        // Epic DLC (MainGameAppName != AppName) -> must skip
        let epic_dlc = r#"{
            "DisplayName": "Alan Wake 2 Night Springs DLC",
            "AppName": "AlanWake2Dlc1",
            "MainGameAppName": "AlanWake2App",
            "CatalogNamespace": "ns123",
            "CatalogItemId": "item789",
            "bIsIncompleteInstall": false
        }"#;
        // Epic incomplete install -> must skip
        let epic_incomplete = r#"{
            "DisplayName": "Broken Game",
            "AppName": "BrokenApp",
            "bIsIncompleteInstall": true
        }"#;

        fs::write(dir.join("ok.item"), epic_ok).unwrap();
        fs::write(dir.join("dlc.item"), epic_dlc).unwrap();
        fs::write(dir.join("inc.item"), epic_incomplete).unwrap();
        fs::write(dir.join("ignore.txt"), b"not a manifest").unwrap();

        let epic_games = scan_epic_games_in_dir(&dir);
        assert_eq!(epic_games.len(), 1);
        assert_eq!(epic_games[0].app_id, "epic:AlanWake2App");
        assert_eq!(epic_games[0].name, "Alan Wake 2");
        assert_eq!(epic_games[0].launcher, "Epic Games");
        assert_eq!(
            epic_games[0].target,
            "com.epicgames.launcher://apps/ns123%3Aitem456%3AAlanWake2App?action=launch&silent=true"
        );
        assert!(epic_games[0].cover_data.is_none());

        // Missing Epic dir degrades gracefully to empty Vec
        assert!(scan_epic_games_in_dir(&dir.join("missing_dir")).is_empty());

        // GOG entry builder: direct exe if file exists, protocol fallback otherwise, skip DLC
        let fake_exe = dir.join("Cyberpunk2077.exe");
        fs::write(&fake_exe, b"MZ").unwrap();
        let gog_direct = build_gog_game_entry(
            "1423049311",
            Some("1423049311"),
            Some("Cyberpunk 2077"),
            Some(&fake_exe.to_string_lossy()),
            Some(&dir.to_string_lossy()),
            None,
        )
        .expect("valid GOG game");
        assert_eq!(gog_direct.app_id, "gog:1423049311");
        assert_eq!(gog_direct.launcher, "GOG");
        assert_eq!(gog_direct.target, fake_exe.to_string_lossy());

        let gog_proto = build_gog_game_entry(
            "1207658924",
            Some("1207658924"),
            Some("The Witcher 3"),
            Some(r"Z:\Nonexistent\witcher3.exe"),
            None,
            None,
        )
        .expect("GOG fallback target");
        assert_eq!(gog_proto.target, "goggalaxy://openGameView/1207658924");

        let gog_dlc = build_gog_game_entry(
            "1207658999",
            Some("1207658999"),
            Some("Blood and Wine DLC"),
            None,
            None,
            Some("1207658924"),
        );
        assert!(gog_dlc.is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_game_cover_data_validates_magic_bytes_size_and_paths() {
        let dir = make_temp_dir("cover-loader");

        let png_path = dir.join("cover.png");
        fs::write(&png_path, b"\x89PNG\r\n\x1a\nfake-png-body").unwrap();
        let png_uri = load_game_cover_data(&png_path.to_string_lossy()).expect("valid png");
        assert!(png_uri.starts_with("data:image/png;base64,"));

        let webp_path = dir.join("cover.webp");
        fs::write(&webp_path, b"RIFF1234WEBPbody").unwrap();
        let webp_uri = load_game_cover_data(&webp_path.to_string_lossy()).expect("valid webp");
        assert!(webp_uri.starts_with("data:image/webp;base64,"));

        // Non-image file must be rejected
        let txt_path = dir.join("notes.txt");
        fs::write(&txt_path, b"hello world").unwrap();
        assert!(load_game_cover_data(&txt_path.to_string_lossy()).is_none());

        // Oversized (> 1.8 MB) file must be rejected without loading
        let huge_path = dir.join("huge.jpg");
        let mut huge = vec![0u8; MAX_GAME_COVER_BYTES + 16];
        huge[0] = 0xFF;
        huge[1] = 0xD8;
        huge[2] = 0xFF;
        fs::write(&huge_path, huge).unwrap();
        assert!(load_game_cover_data(&huge_path.to_string_lossy()).is_none());

        // Missing or control-char paths rejected
        assert!(load_game_cover_data("").is_none());
        assert!(load_game_cover_data("C:\\bad\0path.jpg").is_none());
        assert!(load_game_cover_data(&dir.join("missing.jpg").to_string_lossy()).is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn should_keep_lnk_target_preserves_custom_args_and_custom_working_dir() {
        // Same working dir as exe folder (with or without trailing slash / different case) -> safe to unwrap
        assert!(!should_keep_lnk_target(r"C:\QUIK_VTB\info.exe", "", ""));
        assert!(!should_keep_lnk_target(
            r"C:\QUIK_VTB\info.exe",
            "",
            r"C:\QUIK_VTB"
        ));
        assert!(!should_keep_lnk_target(
            r"C:\QUIK_VTB\info.exe",
            "",
            r"c:/quik_vtb\"
        ));

        // Custom CLI args -> must keep .lnk
        assert!(should_keep_lnk_target(
            r"C:\Users\me\AppData\Local\Discord\Update.exe",
            "--processStart Discord.exe",
            ""
        ));

        // Custom working directory different from exe parent -> must keep .lnk
        assert!(should_keep_lnk_target(
            r"C:\SomeApp\bin\app.exe",
            "",
            r"C:\SomeApp\data"
        ));
    }
}
