use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

use crate::win_shell::wide;
use crate::windows_tracker::extract_icon_for_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedApp {
    pub id: String,
    pub name: String,
    pub category: String,
    pub target: String,
    pub icon_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGame {
    pub app_id: String,
    pub name: String,
    pub launcher: String,
    pub target: String,
    pub cover_data: Option<String>,
    pub last_updated: u64,
}

fn resolve_lnk_target(lnk_path: &Path) -> Option<String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = link.cast().ok()?;
        let w = wide(&lnk_path.to_string_lossy());
        pf.Load(PCWSTR(w.as_ptr()), STGM(0)).ok()?;
        let mut buf = [0u16; 1024];
        let mut fd = std::mem::zeroed();
        link.GetPath(&mut buf, &mut fd, 0).ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let resolved = String::from_utf16_lossy(&buf[..len]);
        if resolved.is_empty() {
            None
        } else {
            Some(resolved)
        }
    }
}

fn categorize_app(name: &str, path: &str) -> String {
    let low = format!("{} {}", name.to_ascii_lowercase(), path.to_ascii_lowercase());
    if low.contains("browser") || low.contains("chrome") || low.contains("edge") || low.contains("firefox") || low.contains("zen") || low.contains("yandex") {
        "Интернет".to_string()
    } else if low.contains("telegram") || low.contains("discord") || low.contains("spark") || low.contains("slack") || low.contains("zoom") || low.contains("whatsapp") {
        "Общение".to_string()
    } else if low.contains("code") || low.contains("cursor") || low.contains("antigravity") || low.contains("studio") || low.contains("unity") || low.contains("git") || low.contains("terminal") || low.contains("warp") || low.contains("python") {
        "Разработка".to_string()
    } else if low.contains("photoshop") || low.contains("lightroom") || low.contains("figma") || low.contains("blender") || low.contains("adobe") {
        "Дизайн".to_string()
    } else if low.contains("claude") || low.contains("comfy") || low.contains("ollama") || low.contains("gpt") {
        "AI".to_string()
    } else if low.contains("steam") || low.contains("battle.net") || low.contains("epic") || low.contains("riot") || low.contains("game") || low.contains("play") {
        "Игры".to_string()
    } else if low.contains("music") || low.contains("музыка") || low.contains("obs") || low.contains("vlc") || low.contains("spotify") {
        "Медиа".to_string()
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
            } else if p.extension().map(|e| e.eq_ignore_ascii_case("lnk")).unwrap_or(false) {
                out.push(p);
            }
        }
    }
}

pub fn scan_installed_apps() -> Vec<IndexedApp> {
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

    let mut apps = Vec::new();
    let mut seen = HashSet::new();

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    for lnk in lnks {
        let name = match lnk.file_stem().map(|s| s.to_string_lossy().to_string()) {
            Some(n) if !n.trim().is_empty() => n,
            _ => continue,
        };
        let low_name = name.to_ascii_lowercase();
        if low_name.contains("uninstall")
            || low_name.contains("удалить")
            || low_name.contains("readme")
            || low_name.contains("help")
            || low_name.contains("справка")
            || low_name.contains("license")
            || low_name.contains("website")
        {
            continue;
        }
        let target = resolve_lnk_target(&lnk)
            .filter(|t| t.to_ascii_lowercase().ends_with(".exe") && Path::new(t).exists())
            .unwrap_or_else(|| lnk.to_string_lossy().to_string());

        let dedup_key = low_name.clone();
        if !seen.insert(dedup_key) {
            continue;
        }

        let category = categorize_app(&name, &target);
        let icon_data = extract_icon_for_path(&target).or_else(|| extract_icon_for_path(&lnk.to_string_lossy()));
        apps.push(IndexedApp {
            id: format!("app-{}", apps.len()),
            name,
            category,
            target,
            icon_data,
        });
        if apps.len() >= 180 {
            break;
        }
    }

    unsafe {
        CoUninitialize();
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

fn parse_acf_field(content: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&needle) {
            let rest = trimmed[needle.len()..].trim();
            if let Some(stripped) = rest.strip_prefix('"') {
                if let Some(end) = stripped.find('"') {
                    return Some(stripped[..end].to_string());
                }
            }
        }
    }
    None
}

fn load_steam_cover_data(steam_root: &Path, app_id: &str) -> Option<String> {
    let cache_dir = steam_root.join("appcache").join("librarycache");
    let candidates = [
        cache_dir.join(app_id).join("library_hero.jpg"),
        cache_dir.join(app_id).join("header.jpg"),
        cache_dir.join(format!("{app_id}_library_hero.jpg")),
        cache_dir.join(format!("{app_id}_header.jpg")),
    ];
    for p in candidates {
        if p.exists() {
            if let Ok(bytes) = fs::read(&p) {
                if bytes.len() <= 1_800_000 {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                    return Some(format!("data:image/jpeg;base64,{b64}"));
                }
            }
        }
    }
    // Also scan nested hashed subfolders inside appcache/librarycache/<appid>/
    let app_dir = cache_dir.join(app_id);
    if app_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&app_dir) {
            for e in entries.flatten() {
                let sub = e.path();
                if sub.is_dir() {
                    for fname in ["library_hero.jpg", "header.jpg"] {
                        let cand = sub.join(fname);
                        if cand.exists() {
                            if let Ok(bytes) = fs::read(&cand) {
                                if bytes.len() <= 1_800_000 {
                                    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                                    return Some(format!("data:image/jpeg;base64,{b64}"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn scan_steam_games() -> Vec<SteamGame> {
    let steam_roots = [
        PathBuf::from(r"D:\steam"),
        PathBuf::from(r"C:\Program Files (x86)\Steam"),
        PathBuf::from(r"C:\Steam"),
        PathBuf::from(r"E:\Steam"),
    ];

    let mut primary_steam: Option<PathBuf> = None;
    let mut library_dirs: Vec<PathBuf> = Vec::new();
    for root in &steam_roots {
        if root.exists() {
            if primary_steam.is_none() {
                primary_steam = Some(root.clone());
            }
            library_dirs.push(root.clone());
            let vdf = root.join("steamapps").join("libraryfolders.vdf");
            if let Ok(content) = fs::read_to_string(&vdf) {
                for line in content.lines() {
                    let t = line.trim();
                    if t.starts_with("\"path\"") {
                        if let Some(val) = parse_acf_field(t, "path") {
                            let clean = val.replace("\\\\", "\\");
                            let p = PathBuf::from(clean);
                            if p.exists() && !library_dirs.contains(&p) {
                                library_dirs.push(p);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut games = Vec::new();
    let mut seen = HashSet::new();

    for lib in &library_dirs {
        let steamapps = lib.join("steamapps");
        if let Ok(entries) = fs::read_dir(&steamapps) {
            for entry in entries.flatten() {
                let p = entry.path();
                let fname = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                    continue;
                }
                if let Ok(content) = fs::read_to_string(&p) {
                    let app_id = match parse_acf_field(&content, "appid") {
                        Some(id) if id != "228980" => id, // skip Steamworks Common Redistributables
                        _ => continue,
                    };
                    if !seen.insert(app_id.clone()) {
                        continue;
                    }
                    let name = match parse_acf_field(&content, "name") {
                        Some(n) => n,
                        None => continue,
                    };
                    let last_updated = parse_acf_field(&content, "LastUpdated")
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    let cover_data = primary_steam
                        .as_deref()
                        .and_then(|sr| load_steam_cover_data(sr, &app_id))
                        .or_else(|| load_steam_cover_data(lib, &app_id));

                    games.push(SteamGame {
                        app_id: app_id.clone(),
                        name,
                        launcher: "Steam".to_string(),
                        target: format!("steam://rungameid/{app_id}"),
                        cover_data,
                        last_updated,
                    });
                }
            }
        }
    }

    games.sort_by(|a, b| b.last_updated.cmp(&a.last_updated));
    games
}

pub fn pick_folder_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Выбери папку для закрепления в TestBar")
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}

pub fn pick_file_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Выбери файл или программу для закрепления в TestBar")
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}
