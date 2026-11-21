use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const SECONDARY_TASKBAR_MODE_TESTBAR: &str = "testbar";
pub const SECONDARY_TASKBAR_MODE_NATIVE: &str = "native";
pub const SECONDARY_TASKBAR_MODE_HIDDEN: &str = "hidden";

pub const GLASS_DEPTH_CRYSTAL: &str = "crystal";
pub const GLASS_DEPTH_OBSIDIAN: &str = "obsidian";
pub const GLASS_DEPTH_MATTE: &str = "matte";

pub const WIDGET_PRESET_MEDIA_CLIPBOARD: &str = "media_clipboard";
pub const WIDGET_PRESET_TELEMETRY_PROCESSES: &str = "telemetry_processes";
pub const WIDGET_PRESET_AUDIO_MIC: &str = "audio_mic";
pub const WIDGET_PRESET_FULL_GRID: &str = "full_grid";

pub const MAX_FRECENCY_ENTRIES: usize = 200;
const SAVE_RENAME_RETRIES: usize = 3;
const SAVE_RENAME_RETRY_DELAY_MS: u64 = 15;
const MAX_IMPORT_CONFIG_BYTES: usize = 4 * 1024 * 1024;

pub fn default_secondary_taskbar_mode() -> String {
    SECONDARY_TASKBAR_MODE_TESTBAR.to_string()
}

pub fn normalize_secondary_taskbar_mode(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "native" => SECONDARY_TASKBAR_MODE_NATIVE,
        "hidden" => SECONDARY_TASKBAR_MODE_HIDDEN,
        _ => SECONDARY_TASKBAR_MODE_TESTBAR,
    }
}

pub fn validate_secondary_taskbar_mode(mode: &str) -> Result<&'static str, String> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "testbar" => Ok(SECONDARY_TASKBAR_MODE_TESTBAR),
        "native" => Ok(SECONDARY_TASKBAR_MODE_NATIVE),
        "hidden" => Ok(SECONDARY_TASKBAR_MODE_HIDDEN),
        other => Err(format!(
            "Недопустимый режим второго монитора: '{other}' (допустимы: testbar, native, hidden)"
        )),
    }
}

pub fn default_glass_depth() -> String {
    GLASS_DEPTH_OBSIDIAN.to_string()
}

pub fn normalize_glass_depth(depth: &str) -> &'static str {
    match depth.trim().to_ascii_lowercase().as_str() {
        "crystal" => GLASS_DEPTH_CRYSTAL,
        "matte" => GLASS_DEPTH_MATTE,
        _ => GLASS_DEPTH_OBSIDIAN,
    }
}

pub fn validate_glass_depth(depth: &str) -> Result<&'static str, String> {
    match depth.trim().to_ascii_lowercase().as_str() {
        "crystal" => Ok(GLASS_DEPTH_CRYSTAL),
        "obsidian" => Ok(GLASS_DEPTH_OBSIDIAN),
        "matte" => Ok(GLASS_DEPTH_MATTE),
        other => Err(format!(
            "Недопустимая глубина стекла: '{other}' (допустимы: crystal, obsidian, matte)"
        )),
    }
}

pub fn default_widget_preset() -> String {
    WIDGET_PRESET_MEDIA_CLIPBOARD.to_string()
}

pub fn normalize_widget_preset(preset: &str) -> &'static str {
    match preset.trim().to_ascii_lowercase().as_str() {
        "telemetry_processes" => WIDGET_PRESET_TELEMETRY_PROCESSES,
        "audio_mic" => WIDGET_PRESET_AUDIO_MIC,
        "full_grid" => WIDGET_PRESET_FULL_GRID,
        _ => WIDGET_PRESET_MEDIA_CLIPBOARD,
    }
}

pub fn validate_widget_preset(preset: &str) -> Result<&'static str, String> {
    match preset.trim().to_ascii_lowercase().as_str() {
        "media_clipboard" => Ok(WIDGET_PRESET_MEDIA_CLIPBOARD),
        "telemetry_processes" => Ok(WIDGET_PRESET_TELEMETRY_PROCESSES),
        "audio_mic" => Ok(WIDGET_PRESET_AUDIO_MIC),
        "full_grid" => Ok(WIDGET_PRESET_FULL_GRID),
        other => Err(format!(
            "Недопустимый набор виджетов: '{other}' (допустимы: media_clipboard, telemetry_processes, audio_mic, full_grid)"
        )),
    }
}

pub fn sanitize_accent_hex_rgb(
    accent_hex: Option<&str>,
    accent_rgb: Option<&str>,
) -> (String, String) {
    let hex = accent_hex
        .map(str::trim)
        .filter(|h| {
            h.len() == 7 && h.starts_with('#') && h[1..].chars().all(|c| c.is_ascii_hexdigit())
        })
        .unwrap_or("#A78BFA");
    let rgb = accent_rgb
        .map(str::trim)
        .filter(|r| {
            let parts: Vec<&str> = r.split(',').collect();
            parts.len() == 3 && parts.iter().all(|p| p.trim().parse::<u8>().is_ok())
        })
        .map(|r| {
            r.split(',')
                .map(|p| p.trim())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|| {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[1..3], 16),
                u8::from_str_radix(&hex[3..5], 16),
                u8::from_str_radix(&hex[5..7], 16),
            ) {
                format!("{r},{g},{b}")
            } else {
                "167,139,250".to_string()
            }
        });
    (hex.to_uppercase(), rgb)
}

const AUTOSTART_RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const AUTOSTART_VALUE_NAME: &str = "TestBar";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PinItem {
    pub id: String,
    /// "app" | "folder" | "file" | "url" | "combo"
    #[serde(default)]
    pub kind: String,
    #[serde(alias = "name")]
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub args: Option<String>,
    #[serde(default)]
    pub alt_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_label: Option<String>,
    #[serde(default)]
    pub accent_rgb: String,
    #[serde(default)]
    pub icon_key: String,
    #[serde(default)]
    pub icon_data: Option<String>,
    #[serde(default)]
    pub match_exe: Option<String>,
    #[serde(default)]
    pub match_aumid: Option<String>,
    #[serde(default, alias = "targets")]
    pub combo_targets: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomSpace {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub accent_hex: String,
    #[serde(default)]
    pub accent_rgb: String,
    #[serde(default = "default_widget_preset")]
    pub widget_preset: String,
    #[serde(default)]
    pub items: Vec<PinItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FrecencyEntry {
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub last_used_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestBarConfig {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub bar_mode: String, // "dock" | "edge"
    #[serde(default = "default_secondary_taskbar_mode")]
    pub secondary_taskbar_mode: String, // "testbar" | "native" | "hidden"
    #[serde(default)]
    pub autostart: bool,
    #[serde(default = "default_glass_depth")]
    pub glass_depth: String, // "crystal" | "obsidian" | "matte"
    #[serde(default)]
    pub clock_show_seconds: bool,
    #[serde(default)]
    pub clock_show_weekday: bool,
    #[serde(default)]
    pub start_remember_space: bool,
    #[serde(default)]
    pub hidden_tray_keys: Vec<String>,
    #[serde(default)]
    pub frecency: HashMap<String, FrecencyEntry>,
    #[serde(default)]
    pub taskbar_pins: Vec<PinItem>,
    #[serde(default)]
    pub personal_pins: Vec<PinItem>,
    #[serde(default)]
    pub personal_links: Vec<PinItem>,
    #[serde(default)]
    pub work_folders: Vec<PinItem>,
    #[serde(default)]
    pub work_apps: Vec<PinItem>,
    #[serde(default)]
    pub studio_pins: Vec<PinItem>,
    #[serde(default)]
    pub studio_tools: Vec<PinItem>,
    #[serde(default)]
    pub game_launchers: Vec<PinItem>,
    #[serde(default)]
    pub game_combos: Vec<PinItem>,
    #[serde(default)]
    pub custom_spaces: Vec<CustomSpace>,
    #[serde(default)]
    pub pinned_clipboard: Vec<String>,
}

fn find_first_existing(candidates: &[PathBuf]) -> Option<String> {
    for p in candidates {
        if p.exists() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

fn user_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"))
}

fn local_appdata() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| user_home().join(r"AppData\Local"))
}

fn roaming_appdata() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| user_home().join(r"AppData\Roaming"))
}

fn detect_app_target(name_key: &str) -> String {
    let local = local_appdata();
    let roaming = roaming_appdata();
    let pf = PathBuf::from(r"C:\Program Files");
    let pf86 = PathBuf::from(r"C:\Program Files (x86)");

    let candidates: Vec<PathBuf> = match name_key {
        "explorer" => vec![PathBuf::from(r"C:\Windows\explorer.exe")],
        "zen" => vec![
            pf.join(r"Zen Browser\zen.exe"),
            local.join(r"Zen Browser\zen.exe"),
            pf.join(r"Mozilla Firefox\firefox.exe"),
        ],
        "telegram" => vec![
            roaming.join(r"Telegram Desktop\Telegram.exe"),
            local.join(r"Programs\Telegram Desktop\Telegram.exe"),
        ],
        "spark" => vec![
            local.join(r"SparkDesktop\SparkDesktop.exe"),
            local.join(r"Programs\SparkDesktop\SparkDesktop.exe"),
            local.join(r"Programs\Spark\Spark.exe"),
            pf.join(r"SparkDesktop\SparkDesktop.exe"),
        ],
        "obsidian" => vec![
            local.join(r"Programs\obsidian\Obsidian.exe"),
            local.join(r"Obsidian\Obsidian.exe"),
        ],
        "claude" => vec![
            local.join(r"AnthropicClaude\claude.exe"),
            local.join(r"Programs\Claude\Claude.exe"),
        ],
        "yandex" => vec![
            local.join(r"Yandex\YandexBrowser\Application\browser.exe"),
            pf86.join(r"Yandex\YandexBrowser\Application\browser.exe"),
            pf.join(r"Yandex\YandexBrowser\Application\browser.exe"),
        ],
        "ymusic" => vec![
            local.join(r"Programs\YandexMusic\Яндекс Музыка.exe"),
            local.join(r"Programs\YandexMusic\YandexMusic.exe"),
        ],
        "discord" => vec![local.join(r"Discord\Update.exe")],
        "obs" => vec![pf.join(r"obs-studio\bin\64bit\obs64.exe")],
        "antigravity" => vec![local.join(r"Programs\Antigravity\Antigravity.exe")],
        "cursor" => vec![local.join(r"Programs\cursor\Cursor.exe")],
        "vscode" => vec![
            local.join(r"Programs\Microsoft VS Code\Code.exe"),
            pf.join(r"Microsoft VS Code\Code.exe"),
        ],
        "photoshop" => vec![
            pf.join(r"Adobe\Adobe Photoshop 2025\Photoshop.exe"),
            pf.join(r"Adobe\Adobe Photoshop 2024\Photoshop.exe"),
        ],
        "lightroom" => vec![pf.join(r"Adobe\Adobe Lightroom Classic\Lightroom.exe")],
        "figma" => vec![local.join(r"Figma\Figma.exe")],
        "unity" => vec![pf.join(r"Unity Hub\Unity Hub.exe")],
        "steam" => {
            if let Some(detected) = crate::apps_indexer::detect_steam_exe() {
                return detected;
            }
            vec![
                pf86.join(r"Steam\steam.exe"),
                pf.join(r"Steam\steam.exe"),
                PathBuf::from(r"D:\steam\steam.exe"),
            ]
        }
        "battlenet" => vec![pf86.join(r"Battle.net\Battle.net Launcher.exe")],
        "warp" => vec![local.join(r"Programs\Warp\warp.exe")],
        "wt" => vec![
            local.join(r"Microsoft\WindowsApps\wt.exe"),
            PathBuf::from(r"C:\Windows\System32\cmd.exe"),
        ],
        "wiztree" => vec![pf.join(r"WizTree\WizTree64.exe")],
        _ => vec![],
    };

    find_first_existing(&candidates).unwrap_or_else(|| match name_key {
        "explorer" => r"C:\Windows\explorer.exe".to_string(),
        "steam" => "steam://open/main".to_string(),
        "ymusic" => "https://music.yandex.ru".to_string(),
        "claude" => "https://claude.ai".to_string(),
        "figma" => "https://figma.com".to_string(),
        _ => String::new(),
    })
}

fn strip_verbatim_prefix(s: &str) -> Cow<'_, str> {
    if let Some(rest) = s
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| s.strip_prefix(r"\\?\unc\"))
    {
        Cow::Owned(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        Cow::Borrowed(rest)
    } else {
        Cow::Borrowed(s)
    }
}

fn has_unquoted_cli_args(path: &str) -> bool {
    let lower = path.to_lowercase();
    let bytes = lower.as_bytes();
    // Reject executable followed by whitespace and additional arguments (e.g. "app.exe --flag foo.exe")
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find(".exe") {
        let end = search_from + rel + 4;
        if let Some(ch) = lower[end..].chars().next() {
            if ch.is_whitespace() {
                return true;
            }
        }
        search_from = end;
    }
    // Reject unquoted CLI flags such as " --flag" or " /flag"
    for i in 1..bytes.len() {
        if bytes[i - 1].is_ascii_whitespace() && (bytes[i] == b'-' || bytes[i] == b'/') {
            return true;
        }
    }
    false
}

/// Derives a lowercase executable filename (e.g. `"zen.exe"`) from an executable target path.
/// Supports Windows verbatim prefixes (`\\?\` / `\\?\UNC\`) and quoted executables with trailing CLI args.
/// Returns `None` for URLs, URI schemes, folders, empty strings, non-`.exe` targets, or unquoted CLI command lines.
pub fn derive_match_exe(target: &str) -> Option<String> {
    let s = target.trim();
    if s.is_empty() {
        return None;
    }

    let path_part = if let Some(after_open) = s.strip_prefix('"') {
        let close_idx = after_open.find('"')?;
        after_open[..close_idx].trim()
    } else {
        if s.contains('"') || has_unquoted_cli_args(s) {
            return None;
        }
        s
    };

    let stripped = strip_verbatim_prefix(path_part);
    let trimmed = stripped.trim();
    if trimmed.is_empty() || trimmed.contains("://") {
        return None;
    }
    if let Some(colon_idx) = trimmed.find(':') {
        let bytes = trimmed.as_bytes();
        let is_drive = colon_idx == 1
            && bytes[0].is_ascii_alphabetic()
            && bytes
                .get(2)
                .map(|&b| b == b'\\' || b == b'/')
                .unwrap_or(false);
        if !is_drive {
            return None;
        }
    }
    if trimmed.ends_with('\\') || trimmed.ends_with('/') {
        return None;
    }
    let file_name = trimmed
        .rsplit(|c| c == '\\' || c == '/')
        .next()
        .unwrap_or("")
        .trim();
    let lower = file_name.to_lowercase();
    if lower.len() > 4 && lower.ends_with(".exe") {
        Some(lower)
    } else {
        None
    }
}

pub fn is_discord_update_target(target: &str) -> bool {
    let norm = target
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .to_ascii_lowercase();
    norm.ends_with(r"\discord\update.exe")
}

fn pin(
    id: &str,
    kind: &str,
    title: &str,
    subtitle: &str,
    target: String,
    alt_action: &str,
    accent_rgb: &str,
    icon_key: &str,
) -> PinItem {
    let is_discord_squirrel = kind == "app" && is_discord_update_target(&target);
    let args = if is_discord_squirrel {
        Some("--processStart Discord.exe".to_string())
    } else {
        None
    };
    let match_exe = if is_discord_squirrel {
        Some("discord.exe".to_string())
    } else if kind == "app" {
        derive_match_exe(&target)
    } else {
        None
    };
    PinItem {
        id: id.to_string(),
        kind: kind.to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        target,
        args,
        alt_action: alt_action.to_string(),
        alt_label: None,
        accent_rgb: accent_rgb.to_string(),
        icon_key: icon_key.to_string(),
        icon_data: None,
        match_exe,
        match_aumid: None,
        combo_targets: None,
    }
}

fn combo_pin(
    id: &str,
    title: &str,
    subtitle: &str,
    targets: Vec<String>,
    accent_rgb: &str,
) -> PinItem {
    PinItem {
        id: id.to_string(),
        kind: "combo".to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        target: String::new(),
        args: None,
        alt_action: String::new(),
        alt_label: None,
        accent_rgb: accent_rgb.to_string(),
        icon_key: String::new(),
        icon_data: None,
        match_exe: None,
        match_aumid: None,
        combo_targets: Some(targets),
    }
}

fn retain_resolved_pins(items: Vec<PinItem>) -> Vec<PinItem> {
    items
        .into_iter()
        .filter(|p| {
            if p.kind == "combo" {
                p.combo_targets
                    .as_ref()
                    .is_some_and(|t| t.iter().any(|s| !s.trim().is_empty()))
            } else {
                !p.target.trim().is_empty()
            }
        })
        .collect()
}

pub fn normalize_pin(item: &mut PinItem, force_combo: bool) -> bool {
    let mut changed = false;

    let desired_kind = if force_combo {
        "combo".to_string()
    } else {
        let norm = item.kind.trim().to_lowercase();
        if norm.is_empty() {
            if item.combo_targets.is_some() {
                "combo".to_string()
            } else {
                "app".to_string()
            }
        } else {
            norm
        }
    };
    if item.kind != desired_kind {
        item.kind = desired_kind;
        changed = true;
    }

    if item.kind == "combo" {
        match &item.combo_targets {
            None => {
                let target_trimmed = item.target.trim();
                let targets = if target_trimmed.is_empty() {
                    Vec::new()
                } else {
                    vec![target_trimmed.to_string()]
                };
                item.combo_targets = Some(targets);
                changed = true;
            }
            Some(existing) => {
                let cleaned: Vec<String> = existing
                    .iter()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
                if cleaned != *existing {
                    item.combo_targets = Some(cleaned);
                    changed = true;
                }
            }
        }
    }

    if let Some(ref args) = item.args {
        if args.trim().is_empty() {
            item.args = None;
            changed = true;
        }
    }

    let is_discord_squirrel = item.kind == "app" && is_discord_update_target(&item.target);
    if is_discord_squirrel && item.args.is_none() {
        item.args = Some("--processStart Discord.exe".to_string());
        changed = true;
    }

    if let Some(ref aumid) = item.match_aumid {
        let trimmed = aumid.trim();
        if trimmed.is_empty() {
            item.match_aumid = None;
            changed = true;
        } else if trimmed != aumid {
            item.match_aumid = Some(trimmed.to_string());
            changed = true;
        }
    }

    let normalized_existing_exe = item
        .match_exe
        .as_deref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    let desired_exe = if is_discord_squirrel {
        match normalized_existing_exe.as_deref() {
            None | Some("update.exe") => Some("discord.exe".to_string()),
            Some(_) => normalized_existing_exe,
        }
    } else if item.kind == "app" {
        normalized_existing_exe.or_else(|| derive_match_exe(&item.target))
    } else {
        normalized_existing_exe
    };
    if item.match_exe != desired_exe {
        item.match_exe = desired_exe;
        changed = true;
    }

    changed
}

/// Losslessly upgrades legacy v0 config payloads to schema v1 and normalizes all pin lists.
/// Returns `true` if any field was upgraded or normalized.
pub fn normalize_and_migrate(cfg: &mut TestBarConfig) -> bool {
    let mut changed = false;

    if cfg.schema_version < CURRENT_SCHEMA_VERSION {
        cfg.schema_version = CURRENT_SCHEMA_VERSION;
        changed = true;
    }

    if cfg.bar_mode != "dock" && cfg.bar_mode != "edge" {
        cfg.bar_mode = "dock".to_string();
        changed = true;
    }

    let desired_secondary = normalize_secondary_taskbar_mode(&cfg.secondary_taskbar_mode);
    if cfg.secondary_taskbar_mode != desired_secondary {
        cfg.secondary_taskbar_mode = desired_secondary.to_string();
        changed = true;
    }

    let desired_glass = normalize_glass_depth(&cfg.glass_depth);
    if cfg.glass_depth != desired_glass {
        cfg.glass_depth = desired_glass.to_string();
        changed = true;
    }

    for list in [
        &mut cfg.taskbar_pins,
        &mut cfg.personal_pins,
        &mut cfg.personal_links,
        &mut cfg.work_folders,
        &mut cfg.work_apps,
        &mut cfg.studio_pins,
        &mut cfg.studio_tools,
        &mut cfg.game_launchers,
    ] {
        for item in list.iter_mut() {
            if normalize_pin(item, false) {
                changed = true;
            }
        }
    }

    for combo in cfg.game_combos.iter_mut() {
        if normalize_pin(combo, true) {
            changed = true;
        }
    }

    for space in cfg.custom_spaces.iter_mut() {
        let desired_preset = normalize_widget_preset(&space.widget_preset);
        if space.widget_preset != desired_preset {
            space.widget_preset = desired_preset.to_string();
            changed = true;
        }
        for item in space.items.iter_mut() {
            if normalize_pin(item, false) {
                changed = true;
            }
        }
    }

    // Deduplicate and drop blank entries in pinned_clipboard while preserving order
    let mut seen = HashSet::new();
    let mut cleaned = Vec::with_capacity(cfg.pinned_clipboard.len());
    for entry in &cfg.pinned_clipboard {
        let trimmed = entry.trim();
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            cleaned.push(trimmed.to_string());
        }
    }
    if cleaned != cfg.pinned_clipboard {
        cfg.pinned_clipboard = cleaned;
        changed = true;
    }

    // Deduplicate and normalize hidden_tray_keys (case-insensitive)
    let mut seen_tray = HashSet::new();
    let mut cleaned_tray = Vec::with_capacity(cfg.hidden_tray_keys.len());
    for entry in &cfg.hidden_tray_keys {
        let norm = entry.trim().to_ascii_lowercase();
        if !norm.is_empty() && seen_tray.insert(norm.clone()) {
            cleaned_tray.push(norm);
        }
    }
    if cleaned_tray != cfg.hidden_tray_keys {
        cfg.hidden_tray_keys = cleaned_tray;
        changed = true;
    }

    if trim_frecency_map(&mut cfg.frecency) {
        changed = true;
    }

    changed
}

fn trim_frecency_map(map: &mut HashMap<String, FrecencyEntry>) -> bool {
    let mut changed = false;
    let before = map.len();
    map.retain(|k, v| !k.trim().is_empty() && (v.count > 0 || v.last_used_ms > 0));
    if map.len() != before {
        changed = true;
    }
    if map.len() > MAX_FRECENCY_ENTRIES {
        let mut ranked: Vec<(String, u64, u32)> = map
            .iter()
            .map(|(k, v)| (k.clone(), v.last_used_ms, v.count))
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)).then_with(|| a.0.cmp(&b.0)));
        let keep: HashSet<String> = ranked
            .into_iter()
            .take(MAX_FRECENCY_ENTRIES)
            .map(|(k, _, _)| k)
            .collect();
        map.retain(|k, _| keep.contains(k));
        changed = true;
    }
    changed
}

impl Default for TestBarConfig {
    fn default() -> Self {
        let home = user_home();
        let downloads = dirs::download_dir()
            .unwrap_or_else(|| home.join("Downloads"))
            .to_string_lossy()
            .to_string();
        let documents = dirs::document_dir()
            .unwrap_or_else(|| home.join("Documents"))
            .to_string_lossy()
            .to_string();
        let desktop = dirs::desktop_dir()
            .unwrap_or_else(|| home.join("Desktop"))
            .to_string_lossy()
            .to_string();
        let pictures = dirs::picture_dir()
            .unwrap_or_else(|| home.join("Pictures"))
            .to_string_lossy()
            .to_string();
        let testbar_dir_path = PathBuf::from(&documents).join("testbar");
        let testbar_dir = if testbar_dir_path.exists() {
            testbar_dir_path.to_string_lossy().to_string()
        } else {
            String::new()
        };

        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: 0,
            bar_mode: "dock".to_string(),
            secondary_taskbar_mode: default_secondary_taskbar_mode(),
            autostart: false,
            glass_depth: default_glass_depth(),
            clock_show_seconds: false,
            clock_show_weekday: false,
            start_remember_space: false,
            hidden_tray_keys: vec![],
            frecency: HashMap::new(),
            taskbar_pins: retain_resolved_pins(vec![
                pin("explorer", "app", "Проводник", "Файлы и диски", detect_app_target("explorer"), "Новое окно", "245,158,11", "explorer"),
                pin("zen", "app", "Zen Browser", "Браузер", detect_app_target("zen"), "Новое окно", "249,115,22", "zen"),
                pin("telegram", "app", "Telegram", "Мессенджер", detect_app_target("telegram"), "", "56,189,248", "telegram"),
                pin("spark", "app", "Spark Mail", "Почта", detect_app_target("spark"), "", "59,130,246", "spark"),
                pin("antigravity", "app", "Antigravity", "IDE", detect_app_target("antigravity"), "Новое окно", "56,189,248", "antigravity"),
                pin("vscode", "app", "VS Code", "Редактор", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
                pin("ymusic", "app", "Яндекс Музыка", "Плеер", detect_app_target("ymusic"), "", "250,204,21", "ymusic"),
            ]),
            personal_pins: retain_resolved_pins(vec![
                pin("zen", "app", "Zen Browser", "Основной браузер", detect_app_target("zen"), "Приватное окно", "249,115,22", "zen"),
                pin("telegram", "app", "Telegram", "Мессенджер", detect_app_target("telegram"), "", "56,189,248", "telegram"),
                pin("spark", "app", "Spark Mail", "Почта", detect_app_target("spark"), "", "59,130,246", "spark"),
                pin("explorer", "app", "Проводник", "Файлы и диски", detect_app_target("explorer"), "Новое окно", "245,158,11", "explorer"),
                pin("obsidian", "app", "Второй мозг", "Obsidian", detect_app_target("obsidian"), "", "139,92,246", "obsidian"),
                pin("claude", "app", "Claude", "Ассистент", detect_app_target("claude"), "", "217,119,87", "claude"),
                pin("yandex", "app", "Яндекс", "Браузер", detect_app_target("yandex"), "Инкогнито", "239,68,68", "yandex"),
                pin("discord", "app", "Discord", "Голос и чаты", detect_app_target("discord"), "", "99,102,241", "discord"),
                pin("ymusic", "app", "Я.Музыка", "Плеер", detect_app_target("ymusic"), "", "250,204,21", "ymusic"),
                pin("obs", "app", "OBS Studio", "Запись экрана", detect_app_target("obs"), "От админа", "148,163,184", "obs"),
                pin("wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "warp"),
            ]),
            personal_links: retain_resolved_pins(vec![
                pin("lnk-downloads", "folder", "Загрузки", "Папка", downloads.clone(), "Скопировать путь", "56,189,248", "folder"),
                pin("lnk-docs", "folder", "Документы", "Папка", documents.clone(), "Скопировать путь", "245,158,11", "folder"),
                pin("lnk-pics", "folder", "Изображения", "Скриншоты", pictures.clone(), "Скопировать путь", "139,92,246", "image"),
                pin("lnk-desktop", "folder", "Рабочий стол", "Файлы", desktop.clone(), "Скопировать путь", "52,211,153", "folder"),
            ]),
            work_folders: retain_resolved_pins(vec![
                pin("wf-dl", "folder", "Загрузки", "Свежие файлы", downloads, "Открыть в терминале", "245,158,11", "folder"),
                pin("wf-docs", "folder", "Документы", "Основная папка", documents.clone(), "Открыть в терминале", "245,158,11", "folder"),
                pin("wf-testbar", "folder", "testbar", "Проект · сейчас", testbar_dir, "Открыть в терминале", "56,189,248", "folder"),
                pin("wf-desktop", "folder", "Рабочий стол", "Файлы и ярлыки", desktop, "Открыть в терминале", "56,189,248", "folder"),
                pin("wf-pics", "folder", "Скриншоты", "Изображения", pictures, "Открыть в терминале", "139,92,246", "folder"),
                pin("wf-home", "folder", "Профиль", "Домашняя папка", home.to_string_lossy().to_string(), "Открыть в терминале", "52,211,153", "folder"),
            ]),
            work_apps: retain_resolved_pins(vec![
                pin("wa-ag", "app", "Antigravity", "IDE", detect_app_target("antigravity"), "Новое окно", "56,189,248", "antigravity"),
                pin("wa-cur", "app", "Cursor", "Код", detect_app_target("cursor"), "Новое окно", "226,232,240", "cursor"),
                pin("wa-vsc", "app", "VS Code", "Редактор", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
                pin("wa-wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "warp"),
            ]),
            studio_pins: retain_resolved_pins(vec![
                pin("photoshop", "app", "Photoshop 2025", "Графика и ретушь", detect_app_target("photoshop"), "От админа", "49,168,255", "photoshop"),
                pin("obsidian-s", "app", "Obsidian", "База знаний", detect_app_target("obsidian"), "", "139,92,246", "obsidian"),
                pin("figma", "app", "Figma", "Интерфейсы", detect_app_target("figma"), "", "168,85,247", "figma"),
                pin("lightroom", "app", "Lightroom", "Classic", detect_app_target("lightroom"), "", "56,189,248", "lightroom"),
                pin("unity", "app", "Unity Hub", "Проекты 3D", detect_app_target("unity"), "", "226,232,240", "unity"),
                pin("obs-s", "app", "OBS Studio", "Захват видео", detect_app_target("obs"), "От админа", "148,163,184", "obs"),
                pin("antigravity-s", "app", "Antigravity", "IDE", detect_app_target("antigravity"), "Новое окно", "56,189,248", "antigravity"),
                pin("cursor-s", "app", "Cursor", "Редактор кода", detect_app_target("cursor"), "Новое окно", "226,232,240", "cursor"),
                pin("vscode-s", "app", "VS Code", "Workspace", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
                pin("claude-s", "app", "Claude", "AI-ассистент", detect_app_target("claude"), "", "217,119,87", "claude"),
                pin("warp-s", "app", "Warp", "Терминал", detect_app_target("warp"), "От админа", "52,211,153", "warp"),
            ]),
            studio_tools: retain_resolved_pins(vec![
                pin("st-snip", "app", "Ножницы", "Скриншот", "ms-screenclip:".to_string(), "", "52,211,153", "scissors"),
                pin("st-wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "warp"),
                pin("st-wiz", "app", "WizTree", "Диски", detect_app_target("wiztree"), "От админа", "56,189,248", "disk"),
                pin("st-tm", "app", "Диспетчер", "Процессы", r"C:\Windows\System32\Taskmgr.exe".to_string(), "От админа", "129,140,248", "cpu"),
            ]),
            game_launchers: retain_resolved_pins(vec![
                pin("gl-steam", "app", "Steam", "Библиотека", detect_app_target("steam"), "", "56,189,248", "steam"),
                pin("gl-bnet", "app", "Battle.net", "Лаунчер", detect_app_target("battlenet"), "", "14,165,233", "battlenet"),
                pin("gl-discord", "app", "Discord", "Голос", detect_app_target("discord"), "", "99,102,241", "discord"),
                pin("gl-obs", "app", "OBS Studio", "Запись", detect_app_target("obs"), "От админа", "148,163,184", "obs"),
            ]),
            game_combos: vec![
                combo_pin(
                    "combo-poe2",
                    "Path of Exile 2 + Steam",
                    "Steam · запуск игры",
                    vec!["steam://rungameid/2694490".to_string()],
                    "251,113,133",
                ),
                combo_pin(
                    "combo-dota2",
                    "Dota 2 + Discord",
                    "Steam + голосовой чат",
                    {
                        let mut targets = vec!["steam://rungameid/570".to_string()];
                        let discord_target = detect_app_target("discord");
                        if !discord_target.is_empty() {
                            if is_discord_update_target(&discord_target) {
                                targets.push(format!(
                                    "\"{discord_target}\" --processStart Discord.exe"
                                ));
                            } else {
                                targets.push(discord_target);
                            }
                        }
                        targets
                    },
                    "56,189,248",
                ),
            ],
            custom_spaces: vec![],
            pinned_clipboard: vec![],
        }
    }
}

pub fn config_path() -> PathBuf {
    roaming_appdata().join("TestBar").join("config.json")
}

pub fn corrupt_backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.json".to_string());
    path.with_file_name(format!("{file_name}.corrupt.bak"))
}

fn sibling_tmp_path(path: &Path) -> PathBuf {
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.json".to_string());
    path.with_file_name(format!("{file_name}.{pid}.{seq}.tmp"))
}

pub fn load_config_from(path: &Path) -> TestBarConfig {
    if path.exists() {
        if let Ok(raw) = fs::read_to_string(path) {
            let trimmed_bom = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
            if let Ok(mut cfg) = serde_json::from_str::<TestBarConfig>(trimmed_bom) {
                if normalize_and_migrate(&mut cfg) {
                    let _ = save_config_to(path, &cfg);
                }
                return cfg;
            }
            // Never silently destroy a corrupt user config: preserve an exact `.corrupt.bak` copy.
            if path.is_file() {
                let _ = fs::copy(path, corrupt_backup_path(path));
            }
        }
        return TestBarConfig::default();
    }
    let cfg = TestBarConfig::default();
    let _ = save_config_to(path, &cfg);
    cfg
}

pub fn load_config() -> TestBarConfig {
    load_config_from(&config_path())
}

fn is_retryable_rename_error(err: &std::io::Error) -> bool {
    if err.kind() == ErrorKind::PermissionDenied {
        return true;
    }
    // Windows ERROR_ACCESS_DENIED (5) or ERROR_SHARING_VIOLATION (32) from AV / Search Indexer
    matches!(err.raw_os_error(), Some(5) | Some(32))
}

pub fn save_config_to(path: &Path, cfg: &TestBarConfig) -> Result<(), String> {
    ensure_parent_dir(path)?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize: {e}"))?;
    let tmp = sibling_tmp_path(path);

    let write_res = (|| -> Result<(), String> {
        let mut file = fs::File::create(&tmp).map_err(|e| format!("create tmp: {e}"))?;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("write tmp: {e}"))?;
        file.sync_all().map_err(|e| format!("sync tmp: {e}"))?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }

    let mut last_err = None;
    for attempt in 0..=SAVE_RENAME_RETRIES {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(err) => {
                let retryable = is_retryable_rename_error(&err) && !path.is_dir();
                last_err = Some(err);
                if retryable && attempt < SAVE_RENAME_RETRIES {
                    std::thread::sleep(Duration::from_millis(SAVE_RENAME_RETRY_DELAY_MS));
                    continue;
                }
                break;
            }
        }
    }

    let _ = fs::remove_file(&tmp);
    Err(format!(
        "rename tmp: {}",
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown error".to_string())
    ))
}

pub fn set_pinned_clipboard(cfg: &mut TestBarConfig, text: &str, pinned: bool) -> bool {
    let value = text.trim();
    if value.is_empty() {
        return false;
    }
    if pinned {
        if cfg.pinned_clipboard.iter().any(|entry| entry == value) {
            return false;
        }
        cfg.pinned_clipboard.push(value.to_string());
        true
    } else {
        let before = cfg.pinned_clipboard.len();
        cfg.pinned_clipboard.retain(|entry| entry != value);
        cfg.pinned_clipboard.len() != before
    }
}

pub fn should_force_combo(space: &str, kind: &str) -> bool {
    space == "game_combos" || (space == "games" && kind.trim().eq_ignore_ascii_case("combo"))
}

pub fn add_pin_to_space(cfg: &mut TestBarConfig, space: &str, mut item: PinItem) -> bool {
    let force_combo = should_force_combo(space, &item.kind);
    normalize_pin(&mut item, force_combo);

    match space {
        "personal" | "personal_pins" => {
            cfg.personal_pins.push(item);
            true
        }
        "personal_links" => {
            cfg.personal_links.push(item);
            true
        }
        "work" => {
            if item.kind == "app" {
                cfg.work_apps.push(item);
            } else {
                cfg.work_folders.push(item);
            }
            true
        }
        "work_folders" => {
            cfg.work_folders.push(item);
            true
        }
        "work_apps" => {
            cfg.work_apps.push(item);
            true
        }
        "studio" | "studio_pins" => {
            cfg.studio_pins.push(item);
            true
        }
        "studio_tools" => {
            cfg.studio_tools.push(item);
            true
        }
        "games" => {
            if item.kind == "combo" {
                cfg.game_combos.push(item);
            } else {
                cfg.game_launchers.push(item);
            }
            true
        }
        "game_combos" => {
            cfg.game_combos.push(item);
            true
        }
        "game_launchers" => {
            cfg.game_launchers.push(item);
            true
        }
        "taskbar" | "taskbar_pins" => {
            let duplicate = (!item.target.is_empty()
                && cfg
                    .taskbar_pins
                    .iter()
                    .any(|p| p.target.eq_ignore_ascii_case(&item.target)))
                || item.match_aumid.as_deref().is_some_and(|aumid| {
                    cfg.taskbar_pins.iter().any(|p| {
                        p.match_aumid
                            .as_deref()
                            .is_some_and(|pa| pa.eq_ignore_ascii_case(aumid))
                    })
                });
            if duplicate {
                false
            } else {
                cfg.taskbar_pins.push(item);
                true
            }
        }
        custom_id => {
            if let Some(sp) = cfg.custom_spaces.iter_mut().find(|s| s.id == custom_id) {
                sp.items.push(item);
                true
            } else {
                false
            }
        }
    }
}

fn remove_by_id(list: &mut Vec<PinItem>, item_id: &str) -> bool {
    let before = list.len();
    list.retain(|i| i.id != item_id);
    list.len() != before
}

pub fn remove_pin_from_space(cfg: &mut TestBarConfig, space: &str, item_id: &str) -> bool {
    match space {
        "personal" | "personal_pins" => remove_by_id(&mut cfg.personal_pins, item_id),
        "personal_links" => remove_by_id(&mut cfg.personal_links, item_id),
        "work" => {
            let removed_folder = remove_by_id(&mut cfg.work_folders, item_id);
            let removed_app = remove_by_id(&mut cfg.work_apps, item_id);
            removed_folder || removed_app
        }
        "work_folders" => remove_by_id(&mut cfg.work_folders, item_id),
        "work_apps" => remove_by_id(&mut cfg.work_apps, item_id),
        "studio" | "studio_pins" => remove_by_id(&mut cfg.studio_pins, item_id),
        "studio_tools" => remove_by_id(&mut cfg.studio_tools, item_id),
        "games" => {
            let removed_combo = remove_by_id(&mut cfg.game_combos, item_id);
            let removed_launcher = remove_by_id(&mut cfg.game_launchers, item_id);
            removed_combo || removed_launcher
        }
        "game_combos" => remove_by_id(&mut cfg.game_combos, item_id),
        "game_launchers" => remove_by_id(&mut cfg.game_launchers, item_id),
        "taskbar" | "taskbar_pins" => remove_by_id(&mut cfg.taskbar_pins, item_id),
        custom_id => cfg
            .custom_spaces
            .iter_mut()
            .find(|s| s.id == custom_id)
            .map(|sp| remove_by_id(&mut sp.items, item_id))
            .unwrap_or(false),
    }
}

pub fn reorder_pins_in_space(
    cfg: &mut TestBarConfig,
    space: &str,
    from_idx: usize,
    to_idx: usize,
) -> bool {
    let list = match space {
        "personal" | "personal_pins" => Some(&mut cfg.personal_pins),
        "personal_links" => Some(&mut cfg.personal_links),
        "work" | "work_folders" => Some(&mut cfg.work_folders),
        "work_apps" => Some(&mut cfg.work_apps),
        "studio" | "studio_pins" => Some(&mut cfg.studio_pins),
        "studio_tools" => Some(&mut cfg.studio_tools),
        "games" | "game_combos" => Some(&mut cfg.game_combos),
        "game_launchers" => Some(&mut cfg.game_launchers),
        "taskbar" | "taskbar_pins" => Some(&mut cfg.taskbar_pins),
        custom_id => cfg
            .custom_spaces
            .iter_mut()
            .find(|s| s.id == custom_id)
            .map(|s| &mut s.items),
    };
    if let Some(vec) = list {
        if from_idx < vec.len() && to_idx < vec.len() && from_idx != to_idx {
            let item = vec.remove(from_idx);
            vec.insert(to_idx, item);
            return true;
        }
    }
    false
}

pub fn update_custom_space(
    cfg: &mut TestBarConfig,
    id: &str,
    name: Option<String>,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    widget_preset: Option<String>,
) -> Result<bool, String> {
    let Some(space) = cfg.custom_spaces.iter_mut().find(|s| s.id == id) else {
        return Err(format!("Пространство '{id}' не найдено"));
    };
    let mut changed = false;

    if let Some(raw_name) = name {
        let clean = raw_name.trim().to_string();
        if clean.is_empty() {
            return Err("Пустое название пространства".to_string());
        }
        if space.name != clean {
            space.name = clean;
            changed = true;
        }
    }

    if accent_hex.is_some() || accent_rgb.is_some() {
        let (hex, rgb) = sanitize_accent_hex_rgb(
            accent_hex.as_deref().or(Some(&space.accent_hex)),
            accent_rgb.as_deref(),
        );
        if space.accent_hex != hex {
            space.accent_hex = hex;
            changed = true;
        }
        if space.accent_rgb != rgb {
            space.accent_rgb = rgb;
            changed = true;
        }
    }

    if let Some(preset) = widget_preset {
        let valid = validate_widget_preset(&preset)?;
        if space.widget_preset != valid {
            space.widget_preset = valid.to_string();
            changed = true;
        }
    }

    Ok(changed)
}

pub fn delete_custom_space(cfg: &mut TestBarConfig, id: &str) -> bool {
    let Some(pos) = cfg.custom_spaces.iter().position(|s| s.id == id) else {
        return false;
    };
    let removed = cfg.custom_spaces.remove(pos);
    cfg.frecency.remove(id);
    for item in &removed.items {
        cfg.frecency.remove(&item.id);
        cfg.frecency.remove(&format!("pin:{}", item.id));
    }
    true
}

fn find_pin_mut<'a>(
    cfg: &'a mut TestBarConfig,
    space: &str,
    pin_id: &str,
) -> Option<(&'a mut PinItem, bool)> {
    match space {
        "personal" | "personal_pins" => cfg
            .personal_pins
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "personal_links" => cfg
            .personal_links
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "work" => {
            if let Some(p) = cfg.work_folders.iter_mut().find(|p| p.id == pin_id) {
                Some((p, false))
            } else {
                cfg.work_apps
                    .iter_mut()
                    .find(|p| p.id == pin_id)
                    .map(|p| (p, false))
            }
        }
        "work_folders" => cfg
            .work_folders
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "work_apps" => cfg
            .work_apps
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "studio" | "studio_pins" => cfg
            .studio_pins
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "studio_tools" => cfg
            .studio_tools
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "games" => {
            if let Some(p) = cfg.game_combos.iter_mut().find(|p| p.id == pin_id) {
                Some((p, true))
            } else {
                cfg.game_launchers
                    .iter_mut()
                    .find(|p| p.id == pin_id)
                    .map(|p| (p, false))
            }
        }
        "game_combos" => cfg
            .game_combos
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, true)),
        "game_launchers" => cfg
            .game_launchers
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "taskbar" | "taskbar_pins" => cfg
            .taskbar_pins
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        custom_id => cfg
            .custom_spaces
            .iter_mut()
            .find(|s| s.id == custom_id)
            .and_then(|s| s.items.iter_mut().find(|p| p.id == pin_id))
            .map(|p| (p, false)),
    }
}

pub fn update_pin(
    cfg: &mut TestBarConfig,
    space: &str,
    pin_id: &str,
    title: Option<String>,
    target: Option<String>,
    args: Option<String>,
    alt_action: Option<String>,
    alt_label: Option<String>,
) -> Result<bool, String> {
    let Some((pin_item, force_combo)) = find_pin_mut(cfg, space, pin_id) else {
        return Err(format!("Элемент '{pin_id}' не найден в '{space}'"));
    };
    let mut changed = false;

    if let Some(raw_title) = title {
        let clean = raw_title.trim().to_string();
        if clean.is_empty() {
            return Err("Название элемента не может быть пустым".to_string());
        }
        if pin_item.title != clean {
            pin_item.title = clean;
            changed = true;
        }
    }

    if let Some(raw_target) = target {
        let clean = raw_target.trim().to_string();
        if pin_item.target != clean {
            pin_item.target = clean;
            // Re-derive match_exe on target update for app pins
            if pin_item.kind == "app" {
                pin_item.match_exe = None;
            }
            changed = true;
        }
    }

    if let Some(raw_args) = args {
        let trimmed = raw_args.trim();
        let next_args = (!trimmed.is_empty()).then(|| trimmed.to_string());
        if pin_item.args != next_args {
            pin_item.args = next_args;
            changed = true;
        }
    }

    if let Some(raw_alt) = alt_action {
        let clean = raw_alt.trim().to_string();
        if pin_item.alt_action != clean {
            pin_item.alt_action = clean;
            changed = true;
        }
    }

    if let Some(raw_label) = alt_label {
        let trimmed = raw_label.trim();
        let next_label = (!trimmed.is_empty()).then(|| trimmed.to_string());
        if pin_item.alt_label != next_label {
            pin_item.alt_label = next_label;
            changed = true;
        }
    }

    if normalize_pin(pin_item, force_combo) {
        changed = true;
    }

    Ok(changed)
}

pub fn add_game_combo(
    cfg: &mut TestBarConfig,
    id: String,
    name: String,
    subtitle: String,
    accent: Option<String>,
    steps: Vec<String>,
) -> Result<bool, String> {
    let clean_name = name.trim().to_string();
    if clean_name.is_empty() {
        return Err("Название игровой связки не может быть пустым".to_string());
    }
    let clean_steps: Vec<String> = steps
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if clean_steps.is_empty() {
        return Err("Добавьте хотя бы один шаг для запуска связки".to_string());
    }
    let (_, accent_rgb) = match accent.as_deref().map(str::trim) {
        Some(a) if a.starts_with('#') => sanitize_accent_hex_rgb(Some(a), None),
        Some(a) if !a.is_empty() => sanitize_accent_hex_rgb(None, Some(a)),
        _ => sanitize_accent_hex_rgb(Some("#38BDF8"), None),
    };
    let clean_sub = subtitle.trim().to_string();
    let mut item = combo_pin(&id, &clean_name, &clean_sub, clean_steps, &accent_rgb);
    normalize_pin(&mut item, true);
    cfg.game_combos.push(item);
    Ok(true)
}

pub fn update_game_combo(
    cfg: &mut TestBarConfig,
    id: &str,
    name: Option<String>,
    subtitle: Option<String>,
    accent: Option<String>,
    steps: Option<Vec<String>>,
) -> Result<bool, String> {
    let Some(combo) = cfg.game_combos.iter_mut().find(|c| c.id == id) else {
        return Err(format!("Игровая связка '{id}' не найдена"));
    };
    let mut changed = false;

    if let Some(raw_name) = name {
        let clean = raw_name.trim().to_string();
        if clean.is_empty() {
            return Err("Название игровой связки не может быть пустым".to_string());
        }
        if combo.title != clean {
            combo.title = clean;
            changed = true;
        }
    }

    if let Some(raw_sub) = subtitle {
        let clean = raw_sub.trim().to_string();
        if combo.subtitle != clean {
            combo.subtitle = clean;
            changed = true;
        }
    }

    if let Some(raw_acc) = accent {
        let trimmed = raw_acc.trim();
        let (_, rgb) = if trimmed.starts_with('#') {
            sanitize_accent_hex_rgb(Some(trimmed), None)
        } else {
            sanitize_accent_hex_rgb(None, Some(trimmed))
        };
        if combo.accent_rgb != rgb {
            combo.accent_rgb = rgb;
            changed = true;
        }
    }

    if let Some(raw_steps) = steps {
        let clean_steps: Vec<String> = raw_steps
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if clean_steps.is_empty() {
            return Err("Добавьте хотя бы один шаг для запуска связки".to_string());
        }
        if combo.combo_targets.as_ref() != Some(&clean_steps) {
            combo.combo_targets = Some(clean_steps);
            changed = true;
        }
    }

    if normalize_pin(combo, true) {
        changed = true;
    }

    Ok(changed)
}

pub fn delete_game_combo(cfg: &mut TestBarConfig, id: &str) -> bool {
    remove_by_id(&mut cfg.game_combos, id)
}

pub fn set_appearance_settings(
    cfg: &mut TestBarConfig,
    glass_depth: Option<String>,
    clock_show_seconds: Option<bool>,
    clock_show_weekday: Option<bool>,
    start_remember_space: Option<bool>,
) -> Result<bool, String> {
    let mut changed = false;
    if let Some(depth) = glass_depth {
        let valid = validate_glass_depth(&depth)?;
        if cfg.glass_depth != valid {
            cfg.glass_depth = valid.to_string();
            changed = true;
        }
    }
    if let Some(sec) = clock_show_seconds {
        if cfg.clock_show_seconds != sec {
            cfg.clock_show_seconds = sec;
            changed = true;
        }
    }
    if let Some(wd) = clock_show_weekday {
        if cfg.clock_show_weekday != wd {
            cfg.clock_show_weekday = wd;
            changed = true;
        }
    }
    if let Some(rem) = start_remember_space {
        if cfg.start_remember_space != rem {
            cfg.start_remember_space = rem;
            changed = true;
        }
    }
    Ok(changed)
}

pub fn hide_tray_key(cfg: &mut TestBarConfig, key: &str) -> bool {
    let norm = key.trim().to_ascii_lowercase();
    if norm.is_empty() {
        return false;
    }
    if cfg.hidden_tray_keys.iter().any(|k| k.eq_ignore_ascii_case(&norm)) {
        return false;
    }
    cfg.hidden_tray_keys.push(norm);
    true
}

pub fn restore_tray_key(cfg: &mut TestBarConfig, key: &str) -> bool {
    let norm = key.trim().to_ascii_lowercase();
    if norm.is_empty() {
        return false;
    }
    let before = cfg.hidden_tray_keys.len();
    cfg.hidden_tray_keys.retain(|k| !k.eq_ignore_ascii_case(&norm));
    cfg.hidden_tray_keys.len() != before
}

pub fn restore_all_tray_keys(cfg: &mut TestBarConfig) -> bool {
    if cfg.hidden_tray_keys.is_empty() {
        return false;
    }
    cfg.hidden_tray_keys.clear();
    true
}

pub fn record_frecency(cfg: &mut TestBarConfig, item_id: &str, now_ms: u64) -> bool {
    let key = item_id.trim();
    if key.is_empty() {
        return false;
    }
    let entry = cfg.frecency.entry(key.to_string()).or_default();
    entry.count = entry.count.saturating_add(1);
    entry.last_used_ms = now_ms.max(entry.last_used_ms);
    trim_frecency_map(&mut cfg.frecency);
    true
}

pub fn export_config_json(cfg: &TestBarConfig) -> Result<String, String> {
    serde_json::to_string_pretty(cfg).map_err(|e| format!("Не удалось сериализовать конфиг: {e}"))
}

pub fn import_config_json(raw: &str) -> Result<TestBarConfig, String> {
    if raw.len() > MAX_IMPORT_CONFIG_BYTES {
        return Err("Файл конфигурации слишком большой (максимум 4 МБ)".to_string());
    }
    let trimmed = raw.strip_prefix('\u{feff}').unwrap_or(raw).trim();
    if trimmed.is_empty() {
        return Err("Файл конфигурации пуст".to_string());
    }
    let val: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|e| format!("Некорректный JSON конфигурации: {e}"))?;
    if !val.is_object() {
        return Err("Конфигурация должна быть JSON-объектом".to_string());
    }
    let mut parsed: TestBarConfig = serde_json::from_value(val)
        .map_err(|e| format!("Несовместимый формат конфигурации TestBar: {e}"))?;
    normalize_and_migrate(&mut parsed);
    Ok(parsed)
}

pub fn format_autostart_command(exe_path: &Path) -> Result<String, String> {
    let raw = exe_path.to_string_lossy();
    if raw.contains('\0') {
        return Err("autostart executable path contains NUL byte".to_string());
    }
    if raw.contains('"') {
        return Err("autostart executable path contains embedded quote".to_string());
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("autostart executable path is empty".to_string());
    }
    let normalized = strip_verbatim_prefix(trimmed);
    let cleaned = normalized.trim();
    if cleaned.is_empty() {
        return Err("autostart executable path is empty".to_string());
    }
    Ok(format!("\"{cleaned}\""))
}

pub fn classify_autostart_delete_error(code: WIN32_ERROR) -> Result<(), String> {
    if code == ERROR_SUCCESS || code == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!(
            "RegDeleteValueW failed (code {}): {}",
            code.0,
            windows::core::Error::from(code)
        ))
    }
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

fn to_wide_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn set_windows_autostart_at(
    subkey_str: &str,
    value_name_str: &str,
    exe_path: &Path,
    enable: bool,
) -> Result<(), String> {
    let subkey = to_wide_nul(subkey_str);
    let value_name = to_wide_nul(value_name_str);

    if enable {
        let command = format_autostart_command(exe_path)?;
        let wide_cmd = to_wide_nul(&command);
        let bytes = unsafe {
            std::slice::from_raw_parts(wide_cmd.as_ptr() as *const u8, wide_cmd.len() * 2)
        };

        let mut raw_key = HKEY::default();
        let create_err = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut raw_key,
                None,
            )
        };
        if create_err != ERROR_SUCCESS {
            return Err(format!(
                "RegCreateKeyExW failed (code {}): {}",
                create_err.0,
                windows::core::Error::from(create_err)
            ));
        }
        let _guard = RegKeyGuard(raw_key);

        let set_err = unsafe {
            RegSetValueExW(
                raw_key,
                PCWSTR(value_name.as_ptr()),
                0,
                REG_SZ,
                Some(bytes),
            )
        };
        if set_err != ERROR_SUCCESS {
            return Err(format!(
                "RegSetValueExW failed (code {}): {}",
                set_err.0,
                windows::core::Error::from(set_err)
            ));
        }
        Ok(())
    } else {
        let mut raw_key = HKEY::default();
        let open_err = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                0,
                KEY_SET_VALUE,
                &mut raw_key,
            )
        };
        if open_err == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if open_err != ERROR_SUCCESS {
            return Err(format!(
                "RegOpenKeyExW failed (code {}): {}",
                open_err.0,
                windows::core::Error::from(open_err)
            ));
        }
        let _guard = RegKeyGuard(raw_key);

        let del_err = unsafe { RegDeleteValueW(raw_key, PCWSTR(value_name.as_ptr())) };
        classify_autostart_delete_error(del_err)
    }
}

pub fn set_windows_autostart(enable: bool) -> Result<(), String> {
    let exe = if enable {
        std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?
    } else {
        PathBuf::new()
    };
    set_windows_autostart_at(AUTOSTART_RUN_SUBKEY, AUTOSTART_VALUE_NAME, &exe, enable)
}

fn ensure_parent_dir(p: &Path) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::ERROR_ACCESS_DENIED;

    fn make_temp_dir(tag: &str) -> PathBuf {
        static DIR_SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "testbar-config-test-{}-{}-{}",
            tag,
            std::process::id(),
            seq
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn count_tmp_files(dir: &Path) -> usize {
        fs::read_dir(dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count()
    }

    #[test]
    fn default_config_roundtrip_and_universal_pinning() {
        let mut cfg = TestBarConfig::default();
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.revision, 0);
        assert_eq!(cfg.bar_mode, "dock");
        assert!(!cfg.personal_pins.is_empty());

        let folder_pin = pin(
            "custom-folder-1",
            "folder",
            "Проекты",
            "Папка",
            r"C:\Temp".to_string(),
            "Открыть в терминале",
            "56,189,248",
            "folder",
        );
        assert!(add_pin_to_space(&mut cfg, "personal", folder_pin));
        assert!(cfg.personal_pins.iter().any(|p| p.id == "custom-folder-1"));

        let prev_first = cfg.personal_pins[0].id.clone();
        assert!(reorder_pins_in_space(&mut cfg, "personal", 0, 1));
        assert_eq!(cfg.personal_pins[1].id, prev_first);

        assert!(remove_pin_from_space(&mut cfg, "personal", "custom-folder-1"));
        assert!(!cfg.personal_pins.iter().any(|p| p.id == "custom-folder-1"));

        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: TestBarConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.bar_mode, "dock");
        assert_eq!(parsed.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn derive_match_exe_handles_executables_and_rejects_urls_folders_and_schemes() {
        assert_eq!(
            derive_match_exe(r"C:\Program Files\Zen Browser\Zen.EXE"),
            Some("zen.exe".to_string())
        );
        assert_eq!(
            derive_match_exe(r"C:\Program Files\Unity Hub\Unity Hub.exe"),
            Some("unity hub.exe".to_string())
        );
        assert_eq!(
            derive_match_exe(r#""C:\Windows\System32\Taskmgr.exe""#),
            Some("taskmgr.exe".to_string())
        );
        assert_eq!(
            derive_match_exe(r#""C:\Program Files\VS Code\Code.exe" --new-window "file.txt""#),
            Some("code.exe".to_string())
        );
        assert_eq!(
            derive_match_exe(r"\\?\C:\Program Files\Zen Browser\Zen.exe"),
            Some("zen.exe".to_string())
        );
        assert_eq!(
            derive_match_exe(r"\\?\UNC\server\share\Tools\Code.exe"),
            Some("code.exe".to_string())
        );
        assert_eq!(
            derive_match_exe(r#""\\?\C:\Tools\App.exe" --profile dev"#),
            Some("app.exe".to_string())
        );
        assert_eq!(
            derive_match_exe(r"\\server\share\Tools\Code.exe"),
            Some("code.exe".to_string())
        );
        assert_eq!(
            derive_match_exe("notepad.exe"),
            Some("notepad.exe".to_string())
        );
        // Reject unquoted CLI command lines with trailing args/flags
        assert_eq!(derive_match_exe(r"C:\Tools\app.exe --flag"), None);
        assert_eq!(derive_match_exe(r"C:\Tools\app.exe --child other.exe"), None);
        assert_eq!(derive_match_exe(r"cmd.exe /c notepad.exe"), None);
        assert_eq!(derive_match_exe(r"C:\Tools\runner --target other.exe"), None);
        assert_eq!(derive_match_exe(r"C:\Tools\runner /target other.exe"), None);
        assert_eq!(derive_match_exe("https://example.com/setup.exe"), None);
        assert_eq!(derive_match_exe("steam://rungameid/570"), None);
        assert_eq!(derive_match_exe("ms-screenclip:"), None);
        assert_eq!(derive_match_exe(r"C:\Users\loki2\Downloads"), None);
        assert_eq!(derive_match_exe(r"C:\Users\loki2\app.exe\"), None);
        assert_eq!(derive_match_exe(".exe"), None);
        assert_eq!(derive_match_exe("   "), None);
    }

    #[test]
    fn load_config_handles_utf8_bom_missing_bar_mode_and_preserves_newer_schema_version() {
        let dir = make_temp_dir("bom-partial-v2");

        // 1. Partial config missing barMode & CustomSpace accent fields
        let partial_path = dir.join("partial.json");
        let partial_json = r##"{
          "taskbarPins": [
            { "id": "p1", "kind": "app", "title": "MyApp", "target": "C:\\Apps\\MyApp.exe" }
          ],
          "customSpaces": [
            { "id": "cs1", "name": "Space1", "items": [] }
          ]
        }"##;
        fs::write(&partial_path, partial_json).expect("write partial");
        let loaded_partial = load_config_from(&partial_path);
        assert_eq!(loaded_partial.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded_partial.bar_mode, "dock");
        assert_eq!(loaded_partial.taskbar_pins.len(), 1);
        assert_eq!(loaded_partial.taskbar_pins[0].title, "MyApp");
        assert_eq!(loaded_partial.taskbar_pins[0].match_exe.as_deref(), Some("myapp.exe"));
        assert_eq!(loaded_partial.custom_spaces.len(), 1);
        assert_eq!(loaded_partial.custom_spaces[0].id, "cs1");

        // 2. UTF-8 BOM prefixed file loads cleanly without resetting pins
        let bom_path = dir.join("bom.json");
        let bom_json = format!("\u{feff}{}", r##"{"schemaVersion":1,"revision":3,"barMode":"edge","taskbarPins":[{"id":"b1","kind":"app","title":"BomApp","target":"C:\\Apps\\Bom.exe","matchExe":"bom.exe"}]}"##);
        fs::write(&bom_path, bom_json).expect("write bom");
        let loaded_bom = load_config_from(&bom_path);
        assert_eq!(loaded_bom.bar_mode, "edge");
        assert_eq!(loaded_bom.revision, 3);
        assert_eq!(loaded_bom.taskbar_pins.len(), 1);
        assert_eq!(loaded_bom.taskbar_pins[0].title, "BomApp");

        // 3. Newer schemaVersion (2) is preserved untouched and not downgraded
        let v2_path = dir.join("v2.json");
        let v2_json = r##"{"schemaVersion":2,"revision":10,"barMode":"dock","taskbarPins":[{"id":"v2p","kind":"app","title":"FutureApp","target":"C:\\Apps\\Future.exe","matchExe":"future.exe"}]}"##;
        fs::write(&v2_path, v2_json).expect("write v2");
        let mut loaded_v2 = load_config_from(&v2_path);
        assert_eq!(loaded_v2.schema_version, 2);
        assert_eq!(loaded_v2.revision, 10);
        assert!(!normalize_and_migrate(&mut loaded_v2));
        assert_eq!(loaded_v2.schema_version, 2);
        let on_disk_v2: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&v2_path).expect("read v2")).expect("parse");
        assert_eq!(on_disk_v2["schemaVersion"], 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_and_migrate_handles_kinds_combos_overrides_clip_and_is_idempotent() {
        let mut cfg = TestBarConfig::default();
        cfg.schema_version = 0;
        cfg.bar_mode = "invalid-mode".to_string();
        cfg.pinned_clipboard = vec![
            "  first  ".to_string(),
            "   ".to_string(),
            "first".to_string(),
            "second".to_string(),
        ];

        // App pin with explicit match_exe override + uppercase/padded kind + blank args/aumid
        cfg.personal_pins = vec![
            PinItem {
                id: "override-app".to_string(),
                kind: "  APP ".to_string(),
                title: "Slack".to_string(),
                subtitle: "Chat".to_string(),
                target: r"C:\Slack\Update.exe".to_string(),
                args: Some("   ".to_string()),
                alt_action: String::new(),
                alt_label: None,
                accent_rgb: String::new(),
                icon_key: String::new(),
                icon_data: None,
                match_exe: Some("  Slack.EXE ".to_string()),
                match_aumid: Some("   ".to_string()),
                combo_targets: None,
            },
            // Non-forced combo item with target and no combo_targets
            PinItem {
                id: "inline-combo".to_string(),
                kind: " CoMbO ".to_string(),
                title: "Solo Combo".to_string(),
                subtitle: String::new(),
                target: "  steam://rungameid/570  ".to_string(),
                args: None,
                alt_action: String::new(),
                alt_label: None,
                accent_rgb: String::new(),
                icon_key: String::new(),
                icon_data: None,
                match_exe: None,
                match_aumid: Some("  My.App!Id  ".to_string()),
                combo_targets: None,
            },
            // Non-app item with padded/uppercase match_exe
            PinItem {
                id: "non-app-match".to_string(),
                kind: " Folder ".to_string(),
                title: "Work".to_string(),
                subtitle: String::new(),
                target: r"C:\Work".to_string(),
                args: None,
                alt_action: String::new(),
                alt_label: None,
                accent_rgb: String::new(),
                icon_key: String::new(),
                icon_data: None,
                match_exe: Some(" Explorer.EXE ".to_string()),
                match_aumid: None,
                combo_targets: None,
            },
        ];

        // Combo with padded & blank targets
        cfg.game_combos = vec![PinItem {
            id: "dirty-combo".to_string(),
            kind: "combo".to_string(),
            title: "Dirty Combo".to_string(),
            subtitle: String::new(),
            target: String::new(),
            args: None,
            alt_action: String::new(),
            alt_label: None,
            accent_rgb: String::new(),
            icon_key: String::new(),
            icon_data: None,
            match_exe: None,
            match_aumid: None,
            combo_targets: Some(vec![
                "  steam://rungameid/1  ".to_string(),
                "   ".to_string(),
                "C:\\Apps\\Tool.exe".to_string(),
            ]),
        }];

        assert!(normalize_and_migrate(&mut cfg));
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.bar_mode, "dock");
        assert_eq!(cfg.pinned_clipboard, vec!["first", "second"]);

        assert_eq!(cfg.personal_pins[0].kind, "app");
        assert_eq!(
            cfg.personal_pins[0].match_exe.as_deref(),
            Some("slack.exe"),
            "explicit match_exe override must be preserved instead of replaced by update.exe"
        );
        assert_eq!(cfg.personal_pins[0].args, None);
        assert_eq!(cfg.personal_pins[0].match_aumid, None);

        assert_eq!(cfg.personal_pins[1].kind, "combo");
        assert_eq!(
            cfg.personal_pins[1].combo_targets,
            Some(vec!["steam://rungameid/570".to_string()])
        );
        assert_eq!(
            cfg.personal_pins[1].match_aumid.as_deref(),
            Some("My.App!Id")
        );

        assert_eq!(cfg.personal_pins[2].kind, "folder");
        assert_eq!(
            cfg.personal_pins[2].match_exe.as_deref(),
            Some("explorer.exe")
        );

        assert_eq!(
            cfg.game_combos[0].combo_targets,
            Some(vec![
                "steam://rungameid/1".to_string(),
                r"C:\Apps\Tool.exe".to_string()
            ])
        );

        // Idempotency: second call returns false
        assert!(!normalize_and_migrate(&mut cfg));
    }

    #[test]
    fn legacy_v0_migration_preserves_pins_and_persists_v1_atomically() {
        let dir = make_temp_dir("v0-migrate");
        let path = dir.join("config.json");

        let legacy_v0 = r##"{
          "barMode": "edge",
          "autostart": true,
          "taskbarPins": [
            {
              "id": "tb-zen",
              "kind": "app",
              "name": "Zen Browser",
              "subtitle": "Browser",
              "target": "C:\\Program Files\\Zen Browser\\Zen.EXE",
              "altAction": "New Window",
              "accentRgb": "249,115,22",
              "iconKey": "zen"
            },
            {
              "id": "tb-url",
              "kind": "url",
              "name": "Claude Web",
              "subtitle": "Site",
              "target": "https://claude.ai",
              "altAction": "Open",
              "accentRgb": "217,119,87",
              "iconKey": "claude"
            }
          ],
          "personalPins": [
            {
              "id": "p-snip",
              "kind": "app",
              "name": "Snipping Tool",
              "subtitle": "Capture",
              "target": "ms-screenclip:",
              "altAction": "Open",
              "accentRgb": "52,211,153",
              "iconKey": "scissors"
            }
          ],
          "personalLinks": [
            {
              "id": "lnk-1",
              "kind": "folder",
              "name": "Projects",
              "subtitle": "Folder",
              "target": "C:\\Projects",
              "altAction": "Terminal",
              "accentRgb": "56,189,248",
              "iconKey": "folder"
            }
          ],
          "workFolders": [],
          "workApps": [],
          "studioPins": [],
          "studioTools": [],
          "gameLaunchers": [],
          "gameCombos": [
            {
              "id": "combo-1",
              "name": "Dota + Discord",
              "subtitle": "Stack",
              "targets": ["steam://rungameid/570", "C:\\Discord\\Update.exe"],
              "accentRgb": "56,189,248"
            }
          ],
          "customSpaces": [
            {
              "id": "custom-1",
              "name": "Dev",
              "accentHex": "#A78BFA",
              "accentRgb": "167,139,250",
              "items": [
                {
                  "id": "c-code",
                  "kind": "app",
                  "name": "VS Code",
                  "subtitle": "Editor",
                  "target": "C:\\Tools\\Code.exe",
                  "altAction": "",
                  "accentRgb": "56,189,248",
                  "iconKey": "vscode"
                }
              ]
            }
          ]
        }"##;

        fs::write(&path, legacy_v0).expect("write legacy v0");

        let loaded = load_config_from(&path);
        assert_eq!(loaded.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.revision, 0);
        assert_eq!(loaded.bar_mode, "edge");
        assert!(loaded.autostart);
        assert!(loaded.pinned_clipboard.is_empty());

        // App pin migrated name -> title and derived lowercase match_exe
        assert_eq!(loaded.taskbar_pins.len(), 2);
        assert_eq!(loaded.taskbar_pins[0].title, "Zen Browser");
        assert_eq!(loaded.taskbar_pins[0].match_exe.as_deref(), Some("zen.exe"));

        // URL, URI scheme, and folder stay match_exe = None
        assert_eq!(loaded.taskbar_pins[1].title, "Claude Web");
        assert_eq!(loaded.taskbar_pins[1].match_exe, None);
        assert_eq!(loaded.personal_pins[0].match_exe, None);
        assert_eq!(loaded.personal_links[0].match_exe, None);

        // Legacy GameCombo migrated into polymorphic PinItem kind = "combo"
        assert_eq!(loaded.game_combos.len(), 1);
        assert_eq!(loaded.game_combos[0].kind, "combo");
        assert_eq!(loaded.game_combos[0].title, "Dota + Discord");
        assert_eq!(
            loaded.game_combos[0].combo_targets,
            Some(vec![
                "steam://rungameid/570".to_string(),
                r"C:\Discord\Update.exe".to_string()
            ])
        );

        // Custom space pin migrated too
        assert_eq!(
            loaded.custom_spaces[0].items[0].match_exe.as_deref(),
            Some("code.exe")
        );

        // Upgraded v1 file was persisted atomically with zero lingering .tmp files
        assert_eq!(count_tmp_files(&dir), 0);
        let persisted_raw = fs::read_to_string(&path).expect("read migrated file");
        let persisted_val: serde_json::Value =
            serde_json::from_str(&persisted_raw).expect("valid v1 json");
        assert_eq!(persisted_val["schemaVersion"], 1);
        assert_eq!(persisted_val["taskbarPins"][0]["title"], "Zen Browser");
        assert_eq!(persisted_val["taskbarPins"][0]["matchExe"], "zen.exe");
        assert_eq!(persisted_val["gameCombos"][0]["kind"], "combo");
        assert_eq!(persisted_val["gameCombos"][0]["title"], "Dota + Discord");
        assert_eq!(
            persisted_val["gameCombos"][0]["comboTargets"][0],
            "steam://rungameid/570"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_or_unreadable_config_falls_back_to_default_and_writes_corrupt_backup() {
        let dir = make_temp_dir("corrupt");
        let corrupt_path = dir.join("config.json");
        let raw_corrupt = "{ not valid json !!!";
        fs::write(&corrupt_path, raw_corrupt).expect("write corrupt");

        let cfg = load_config_from(&corrupt_path);
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.bar_mode, "dock");
        assert!(!cfg.taskbar_pins.is_empty());

        let bak_path = corrupt_backup_path(&corrupt_path);
        assert!(bak_path.exists(), "corrupt config must be backed up to .corrupt.bak");
        assert_eq!(fs::read_to_string(&bak_path).unwrap(), raw_corrupt);

        // Passing a directory path (unreadable as file) also falls back cleanly
        let cfg_dir = load_config_from(&dir);
        assert_eq!(cfg_dir.schema_version, CURRENT_SCHEMA_VERSION);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_save_replaces_target_and_cleans_up_tmp_on_error() {
        let dir = make_temp_dir("atomic-save");
        let file_path = dir.join("config.json");
        let mut cfg = TestBarConfig::default();
        cfg.revision = 7;

        save_config_to(&file_path, &cfg).expect("save should succeed");
        assert_eq!(count_tmp_files(&dir), 0);
        let reloaded = load_config_from(&file_path);
        assert_eq!(reloaded.revision, 7);

        // Overwrite existing file atomically
        cfg.revision = 8;
        cfg.bar_mode = "edge".to_string();
        save_config_to(&file_path, &cfg).expect("overwrite should succeed");
        assert_eq!(count_tmp_files(&dir), 0);
        let reloaded2 = load_config_from(&file_path);
        assert_eq!(reloaded2.revision, 8);
        assert_eq!(reloaded2.bar_mode, "edge");

        // Failure case: target path is an existing directory -> rename fails & .tmp is removed
        let dir_target = dir.join("is-a-dir");
        fs::create_dir_all(&dir_target).expect("create target dir");
        let err = save_config_to(&dir_target, &cfg);
        assert!(err.is_err(), "saving over directory must fail");
        assert_eq!(
            count_tmp_files(&dir),
            0,
            "failed save must clean up sibling .tmp file"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    fn target_slice<'a>(cfg: &'a TestBarConfig, key: &str) -> &'a [PinItem] {
        match key {
            "personal" | "personal_pins" => &cfg.personal_pins,
            "personal_links" => &cfg.personal_links,
            "work" | "work_folders" => &cfg.work_folders,
            "work_apps" => &cfg.work_apps,
            "studio" | "studio_pins" => &cfg.studio_pins,
            "studio_tools" => &cfg.studio_tools,
            "games" | "game_combos" => &cfg.game_combos,
            "game_launchers" => &cfg.game_launchers,
            "taskbar" | "taskbar_pins" => &cfg.taskbar_pins,
            custom_id => cfg
                .custom_spaces
                .iter()
                .find(|s| s.id == custom_id)
                .map(|s| s.items.as_slice())
                .expect("custom space must exist"),
        }
    }

    #[test]
    fn uniform_10_list_crud_reorder_and_edge_cases() {
        let mut cfg = TestBarConfig::default();
        cfg.custom_spaces.push(CustomSpace {
            id: "custom-lab".to_string(),
            name: "Lab".to_string(),
            accent_hex: "#A78BFA".to_string(),
            accent_rgb: "167,139,250".to_string(),
            widget_preset: default_widget_preset(),
            items: vec![],
        });

        let make_item = |id: &str, kind: &str, target: &str| {
            pin(id, kind, id, "sub", target.to_string(), "", "1,2,3", "app")
        };

        // Test all 10 explicit lists with exact target-slice presence, order, and removal assertions
        let lists: &[(&str, &str, &str)] = &[
            ("personal_pins", "app", r"C:\Apps\a.exe"),
            ("personal_links", "url", "https://example.com"),
            ("work_folders", "folder", r"C:\Work"),
            ("work_apps", "app", r"C:\Apps\ide.exe"),
            ("studio_pins", "app", r"C:\Apps\ps.exe"),
            ("studio_tools", "app", r"C:\Apps\tool.exe"),
            ("game_launchers", "app", r"C:\Apps\launcher.exe"),
            ("game_combos", "combo", "steam://rungameid/1"),
            ("taskbar_pins", "app", r"C:\Apps\unique-tb-1.exe"),
            ("custom-lab", "file", r"C:\Docs\spec.md"),
        ];

        for (space_key, kind, target) in lists {
            let id1 = format!("t1-{space_key}");
            let id2 = format!("t2-{space_key}");
            let t2 = format!("{target}-2");

            assert!(
                add_pin_to_space(&mut cfg, space_key, make_item(&id1, kind, target)),
                "add id1 to {space_key}"
            );
            assert!(
                add_pin_to_space(&mut cfg, space_key, make_item(&id2, kind, &t2)),
                "add id2 to {space_key}"
            );

            let before_slice = target_slice(&cfg, space_key);
            assert!(before_slice.iter().any(|p| p.id == id1));
            assert!(before_slice.iter().any(|p| p.id == id2));
            let orig_0 = before_slice[0].id.clone();
            let orig_1 = before_slice[1].id.clone();

            // Reorder first two items and verify exact swapped IDs on the target slice
            assert!(
                reorder_pins_in_space(&mut cfg, space_key, 0, 1),
                "reorder in {space_key}"
            );
            let after_reorder = target_slice(&cfg, space_key);
            assert_eq!(after_reorder[0].id, orig_1, "index 0 swapped in {space_key}");
            assert_eq!(after_reorder[1].id, orig_0, "index 1 swapped in {space_key}");

            // Out-of-bounds & same-index no-ops return false without mutating or panicking
            assert!(!reorder_pins_in_space(&mut cfg, space_key, 0, 0));
            assert!(!reorder_pins_in_space(&mut cfg, space_key, 999, 0));
            assert!(!reorder_pins_in_space(&mut cfg, space_key, 0, 999));
            assert_eq!(target_slice(&cfg, space_key)[0].id, orig_1);

            // Remove both items and verify absence on the exact target slice
            assert!(remove_pin_from_space(&mut cfg, space_key, &id1));
            assert!(remove_pin_from_space(&mut cfg, space_key, &id2));
            let after_remove = target_slice(&cfg, space_key);
            assert!(!after_remove.iter().any(|p| p.id == id1));
            assert!(!after_remove.iter().any(|p| p.id == id2));
            assert!(!remove_pin_from_space(&mut cfg, space_key, &id1));
        }

        // Alias routing: "work" routes apps to work_apps, folders to work_folders, reorders work_folders, removes from both
        assert!(add_pin_to_space(
            &mut cfg,
            "work",
            make_item("wa-alias", "app", r"C:\Apps\wa.exe")
        ));
        assert!(cfg.work_apps.iter().any(|p| p.id == "wa-alias"));
        assert!(!cfg.work_folders.iter().any(|p| p.id == "wa-alias"));
        assert!(add_pin_to_space(
            &mut cfg,
            "work",
            make_item("wf-alias", "folder", r"C:\Work\Dir")
        ));
        assert!(cfg.work_folders.iter().any(|p| p.id == "wf-alias"));
        assert!(!cfg.work_apps.iter().any(|p| p.id == "wf-alias"));

        let wf_0 = cfg.work_folders[0].id.clone();
        let wf_1 = cfg.work_folders[1].id.clone();
        let wa_0 = cfg.work_apps[0].id.clone();
        assert!(reorder_pins_in_space(&mut cfg, "work", 0, 1));
        assert_eq!(cfg.work_folders[0].id, wf_1);
        assert_eq!(cfg.work_folders[1].id, wf_0);
        assert_eq!(cfg.work_apps[0].id, wa_0, "work alias reorder must not mutate work_apps");

        assert!(remove_pin_from_space(&mut cfg, "work", "wa-alias"));
        assert!(remove_pin_from_space(&mut cfg, "work", "wf-alias"));
        assert!(!cfg.work_apps.iter().any(|p| p.id == "wa-alias"));
        assert!(!cfg.work_folders.iter().any(|p| p.id == "wf-alias"));

        // Alias routing: "games" routes combo to game_combos, app to game_launchers, reorders game_combos, removes from both
        assert!(add_pin_to_space(
            &mut cfg,
            "games",
            make_item("gc-alias", "combo", "steam://rungameid/99")
        ));
        assert!(cfg
            .game_combos
            .iter()
            .any(|p| p.id == "gc-alias" && p.kind == "combo" && p.combo_targets.is_some()));
        assert!(!cfg.game_launchers.iter().any(|p| p.id == "gc-alias"));
        assert!(add_pin_to_space(
            &mut cfg,
            "games",
            make_item("gl-alias", "app", r"C:\Apps\epic.exe")
        ));
        assert!(cfg.game_launchers.iter().any(|p| p.id == "gl-alias"));
        assert!(!cfg.game_combos.iter().any(|p| p.id == "gl-alias"));

        let gc_0 = cfg.game_combos[0].id.clone();
        let gc_1 = cfg.game_combos[1].id.clone();
        let gl_0 = cfg.game_launchers[0].id.clone();
        assert!(reorder_pins_in_space(&mut cfg, "games", 0, 1));
        assert_eq!(cfg.game_combos[0].id, gc_1);
        assert_eq!(cfg.game_combos[1].id, gc_0);
        assert_eq!(cfg.game_launchers[0].id, gl_0, "games alias reorder must not mutate game_launchers");

        assert!(remove_pin_from_space(&mut cfg, "games", "gc-alias"));
        assert!(remove_pin_from_space(&mut cfg, "games", "gl-alias"));
        assert!(!cfg.game_combos.iter().any(|p| p.id == "gc-alias"));
        assert!(!cfg.game_launchers.iter().any(|p| p.id == "gl-alias"));

        // Duplicate taskbar pin returns false without mutating
        let tb_len_before = cfg.taskbar_pins.len();
        assert!(add_pin_to_space(
            &mut cfg,
            "taskbar",
            make_item("tb-dup-1", "app", r"C:\Apps\DupApp.exe")
        ));
        assert_eq!(cfg.taskbar_pins.len(), tb_len_before + 1);
        assert!(!add_pin_to_space(
            &mut cfg,
            "taskbar",
            make_item("tb-dup-2", "app", r"c:\apps\dupapp.exe")
        ));
        assert_eq!(cfg.taskbar_pins.len(), tb_len_before + 1);

        // Unknown space returns false across all operations
        assert!(!add_pin_to_space(
            &mut cfg,
            "nonexistent-space",
            make_item("x", "app", r"C:\x.exe")
        ));
        assert!(!remove_pin_from_space(&mut cfg, "nonexistent-space", "x"));
        assert!(!reorder_pins_in_space(&mut cfg, "nonexistent-space", 0, 1));
    }

    #[test]
    fn pinned_clipboard_tracks_add_noop_and_remove() {
        let mut cfg = TestBarConfig::default();
        assert!(!set_pinned_clipboard(&mut cfg, "   ", true));
        assert!(set_pinned_clipboard(&mut cfg, "https://testbar.local", true));
        assert!(!set_pinned_clipboard(&mut cfg, "https://testbar.local", true));
        assert_eq!(cfg.pinned_clipboard, vec!["https://testbar.local"]);

        assert!(set_pinned_clipboard(&mut cfg, "https://testbar.local", false));
        assert!(!set_pinned_clipboard(&mut cfg, "https://testbar.local", false));
        assert!(cfg.pinned_clipboard.is_empty());
    }

    #[test]
    fn autostart_pure_helpers_format_paths_and_classify_registry_errors() {
        let valid = format_autostart_command(Path::new(r"C:\Program Files\TestBar\testbar.exe"))
            .expect("valid path");
        assert_eq!(valid, r#""C:\Program Files\TestBar\testbar.exe""#);

        let verbatim = format_autostart_command(Path::new(r"\\?\C:\Program Files\TestBar\testbar.exe"))
            .expect("verbatim drive path");
        assert_eq!(verbatim, r#""C:\Program Files\TestBar\testbar.exe""#);

        let verbatim_unc = format_autostart_command(Path::new(r"\\?\UNC\server\share\testbar.exe"))
            .expect("verbatim unc path");
        assert_eq!(verbatim_unc, r#""\\server\share\testbar.exe""#);

        assert!(format_autostart_command(Path::new("")).is_err());
        assert!(format_autostart_command(Path::new("   ")).is_err());
        assert!(format_autostart_command(Path::new(r"\\?\")).is_err());
        assert!(format_autostart_command(Path::new("C:\\bad\0path\\testbar.exe")).is_err());
        assert!(format_autostart_command(Path::new(r#"C:\bad"quote\testbar.exe"#)).is_err());
        assert!(format_autostart_command(Path::new(r#""C:\TestBar\testbar.exe""#)).is_err());

        assert!(classify_autostart_delete_error(ERROR_SUCCESS).is_ok());
        assert!(classify_autostart_delete_error(ERROR_FILE_NOT_FOUND).is_ok());
        assert!(classify_autostart_delete_error(ERROR_ACCESS_DENIED).is_err());
    }

    #[test]
    fn autostart_registry_roundtrip_on_isolated_temp_subkey() {
        use windows::Win32::System::Registry::{
            RegDeleteKeyW, RegQueryValueExW, KEY_QUERY_VALUE, REG_VALUE_TYPE,
        };

        struct TempRegSubkeyGuard(String);
        impl Drop for TempRegSubkeyGuard {
            fn drop(&mut self) {
                let w = to_wide_nul(&self.0);
                unsafe {
                    let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(w.as_ptr()));
                    let parent = to_wide_nul(r"Software\TestBar\TestAutostart");
                    let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(parent.as_ptr()));
                    let root = to_wide_nul(r"Software\TestBar");
                    let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(root.as_ptr()));
                }
            }
        }

        static REG_SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = REG_SEQ.fetch_add(1, Ordering::Relaxed);
        let subkey = format!(
            r"Software\TestBar\TestAutostart\{}-{}",
            std::process::id(),
            seq
        );
        let _cleanup = TempRegSubkeyGuard(subkey.clone());
        let val_name = "TestBar";
        let exe_path = Path::new(r"\\?\C:\Program Files\TestBar\testbar.exe");

        // 1. Disable on missing subkey is idempotent Ok(())
        set_windows_autostart_at(&subkey, val_name, exe_path, false)
            .expect("disable on missing subkey must succeed");

        // 2. Enable creates subkey and writes quoted REG_SZ without verbatim prefix
        set_windows_autostart_at(&subkey, val_name, exe_path, true)
            .expect("enable on temp subkey must succeed");

        // 3. Read back via RegQueryValueExW and verify REG_SZ payload
        let w_subkey = to_wide_nul(&subkey);
        let w_val = to_wide_nul(val_name);
        let mut hkey = HKEY::default();
        let open_err = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(w_subkey.as_ptr()),
                0,
                KEY_QUERY_VALUE,
                &mut hkey,
            )
        };
        assert_eq!(open_err, ERROR_SUCCESS);
        let _read_guard = RegKeyGuard(hkey);

        let mut reg_type = REG_VALUE_TYPE(0);
        let mut buf = vec![0u16; 256];
        let mut buf_len = (buf.len() * std::mem::size_of::<u16>()) as u32;
        let query_err = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(w_val.as_ptr()),
                None,
                Some(&mut reg_type),
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut buf_len),
            )
        };
        assert_eq!(query_err, ERROR_SUCCESS);
        assert_eq!(reg_type, REG_SZ);
        let u16_slice = &buf[..(buf_len as usize) / 2];
        let trimmed_u16 = u16_slice.strip_suffix(&[0]).unwrap_or(u16_slice);
        let read_back = String::from_utf16(trimmed_u16).expect("valid utf16");
        assert_eq!(read_back, r#""C:\Program Files\TestBar\testbar.exe""#);

        // 4. Disable deletes the value
        set_windows_autostart_at(&subkey, val_name, exe_path, false)
            .expect("disable must delete value");
        let mut check_len = (buf.len() * std::mem::size_of::<u16>()) as u32;
        let after_del = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(w_val.as_ptr()),
                None,
                None,
                Some(buf.as_mut_ptr() as *mut u8),
                Some(&mut check_len),
            )
        };
        assert_eq!(after_del, ERROR_FILE_NOT_FOUND);

        // 5. Repeat disable is idempotent Ok(())
        set_windows_autostart_at(&subkey, val_name, exe_path, false)
            .expect("repeat disable must succeed");
    }

    #[test]
    fn secondary_taskbar_mode_defaults_validation_and_persistence() {
        let dir = make_temp_dir("secondary-mode");
        let cfg_path = dir.join("config.json");

        // Default config uses "testbar"
        let def = TestBarConfig::default();
        assert_eq!(def.secondary_taskbar_mode, "testbar");

        // Validation accepts canonical values case-insensitively and rejects invalid ones
        assert_eq!(validate_secondary_taskbar_mode("testbar"), Ok("testbar"));
        assert_eq!(validate_secondary_taskbar_mode("  NATIVE "), Ok("native"));
        assert_eq!(validate_secondary_taskbar_mode("Hidden"), Ok("hidden"));
        assert!(validate_secondary_taskbar_mode("mirror").is_err());
        assert!(validate_secondary_taskbar_mode("").is_err());

        // Legacy JSON missing secondaryTaskbarMode defaults to "testbar"
        let legacy_json = r#"{"schemaVersion":1,"revision":2,"barMode":"dock"}"#;
        fs::write(&cfg_path, legacy_json).expect("write legacy");
        let loaded = load_config_from(&cfg_path);
        assert_eq!(loaded.secondary_taskbar_mode, "testbar");

        // Unknown value normalizes back to "testbar" on load/migration
        let invalid_json =
            r#"{"schemaVersion":1,"revision":3,"barMode":"dock","secondaryTaskbarMode":"bogus"}"#;
        fs::write(&cfg_path, invalid_json).expect("write invalid");
        let normalized = load_config_from(&cfg_path);
        assert_eq!(normalized.secondary_taskbar_mode, "testbar");

        // Valid modes ("native", "hidden") persist cleanly through save/load roundtrip
        for mode in ["native", "hidden", "testbar"] {
            let mut cfg = TestBarConfig::default();
            cfg.secondary_taskbar_mode = mode.to_string();
            save_config_to(&cfg_path, &cfg).expect("save mode");
            let back = load_config_from(&cfg_path);
            assert_eq!(back.secondary_taskbar_mode, mode);
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn discord_squirrel_update_exe_normalizes_args_and_match_exe() {
        let target = r"C:\Users\alice\AppData\Local\Discord\Update.exe".to_string();
        let created = pin(
            "discord",
            "app",
            "Discord",
            "Голос и чаты",
            target.clone(),
            "Каналы",
            "99,102,241",
            "discord",
        );
        assert_eq!(created.args.as_deref(), Some("--processStart Discord.exe"));
        assert_eq!(created.match_exe.as_deref(), Some("discord.exe"));

        // Legacy persisted Discord pin with Update.exe and no args / wrong match_exe migrates cleanly
        let mut legacy = PinItem {
            id: "discord".to_string(),
            kind: "app".to_string(),
            title: "Discord".to_string(),
            subtitle: "Голос".to_string(),
            target,
            args: None,
            alt_action: "Каналы".to_string(),
            alt_label: None,
            accent_rgb: "99,102,241".to_string(),
            icon_key: "discord".to_string(),
            icon_data: None,
            match_exe: Some("update.exe".to_string()),
            match_aumid: None,
            combo_targets: None,
        };
        assert!(normalize_pin(&mut legacy, false));
        assert_eq!(legacy.args.as_deref(), Some("--processStart Discord.exe"));
        assert_eq!(legacy.match_exe.as_deref(), Some("discord.exe"));
        // Idempotent second pass
        assert!(!normalize_pin(&mut legacy, false));

        // Non-Discord Update.exe is untouched
        let mut other = PinItem {
            target: r"C:\Users\alice\AppData\Local\OtherApp\Update.exe".to_string(),
            args: None,
            match_exe: None,
            ..legacy.clone()
        };
        assert!(normalize_pin(&mut other, false));
        assert_eq!(other.args, None);
        assert_eq!(other.match_exe.as_deref(), Some("update.exe"));
    }

    #[test]
    fn honest_defaults_omit_empty_targets_and_fake_alt_labels() {
        let cfg = TestBarConfig::default();
        let all_non_combo = [
            &cfg.taskbar_pins,
            &cfg.personal_pins,
            &cfg.personal_links,
            &cfg.work_folders,
            &cfg.work_apps,
            &cfg.studio_pins,
            &cfg.studio_tools,
            &cfg.game_launchers,
        ];
        let banned_fake_alts = [
            "Избранное",
            "Новое письмо",
            "Моя волна",
            "Новая заметка",
            "Новый чат",
            "Каналы",
            "Каталог",
            "Открыть",
        ];
        for list in all_non_combo {
            for p in list {
                assert!(
                    !p.target.trim().is_empty(),
                    "default pin '{}' must never have empty target",
                    p.id
                );
                assert!(
                    !banned_fake_alts.contains(&p.alt_action.as_str()),
                    "default pin '{}' must not carry unimplemented fake alt_action '{}'",
                    p.id,
                    p.alt_action
                );
            }
        }
        assert_eq!(cfg.glass_depth, GLASS_DEPTH_OBSIDIAN);
        assert!(!cfg.clock_show_seconds);
        assert!(!cfg.clock_show_weekday);
        assert!(!cfg.start_remember_space);
        assert!(cfg.hidden_tray_keys.is_empty());
        assert!(cfg.frecency.is_empty());
    }

    #[test]
    fn premium_mutations_spaces_pins_combos_appearance_tray_frecency_and_import_export() {
        let mut cfg = TestBarConfig::default();

        // 1. Custom space CRUD + widget_preset validation
        cfg.custom_spaces.push(CustomSpace {
            id: "custom-1".to_string(),
            name: "Музыка".to_string(),
            accent_hex: "#A78BFA".to_string(),
            accent_rgb: "167,139,250".to_string(),
            widget_preset: default_widget_preset(),
            items: vec![pin(
                "cp-1",
                "app",
                "App",
                "",
                r"C:\Apps\app.exe".to_string(),
                "",
                "167,139,250",
                "app",
            )],
        });
        assert!(update_custom_space(
            &mut cfg,
            "custom-1",
            Some("Медиа".to_string()),
            Some("#38bdf8".to_string()),
            None,
            Some("audio_mic".to_string()),
        )
        .unwrap());
        assert_eq!(cfg.custom_spaces[0].name, "Медиа");
        assert_eq!(cfg.custom_spaces[0].accent_hex, "#38BDF8");
        assert_eq!(cfg.custom_spaces[0].accent_rgb, "56,189,248");
        assert_eq!(cfg.custom_spaces[0].widget_preset, WIDGET_PRESET_AUDIO_MIC);
        assert!(update_custom_space(&mut cfg, "custom-1", None, None, None, Some("bogus".into())).is_err());

        // 2. Pin edit mutation
        assert!(update_pin(
            &mut cfg,
            "custom-1",
            "cp-1",
            Some("Renamed App".to_string()),
            Some(r"C:\Apps\Renamed.exe".to_string()),
            Some("--flag".to_string()),
            Some("От админа".to_string()),
            Some("Запуск от админа".to_string()),
        )
        .unwrap());
        assert_eq!(cfg.custom_spaces[0].items[0].title, "Renamed App");
        assert_eq!(cfg.custom_spaces[0].items[0].match_exe.as_deref(), Some("renamed.exe"));
        assert_eq!(cfg.custom_spaces[0].items[0].args.as_deref(), Some("--flag"));
        assert_eq!(cfg.custom_spaces[0].items[0].alt_label.as_deref(), Some("Запуск от админа"));
        assert!(update_pin(&mut cfg, "custom-1", "missing", Some("X".into()), None, None, None, None).is_err());

        // 3. Delete custom space cleans up its frecency entries
        assert!(record_frecency(&mut cfg, "cp-1", 1000));
        assert!(cfg.frecency.contains_key("cp-1"));
        assert!(delete_custom_space(&mut cfg, "custom-1"));
        assert!(cfg.custom_spaces.is_empty());
        assert!(!cfg.frecency.contains_key("cp-1"));

        // 4. Game combo CRUD
        assert!(add_game_combo(
            &mut cfg,
            "combo-test".to_string(),
            "CS2 Stack".to_string(),
            "Steam + Discord".to_string(),
            Some("#F97316".to_string()),
            vec!["steam://rungameid/730".to_string()],
        )
        .unwrap());
        assert!(cfg.game_combos.iter().any(|c| c.id == "combo-test" && c.accent_rgb == "249,115,22"));
        assert!(update_game_combo(
            &mut cfg,
            "combo-test",
            Some("CS2 Pro".to_string()),
            None,
            None,
            Some(vec!["steam://rungameid/730".to_string(), r"C:\Apps\Faceit.exe".to_string()]),
        )
        .unwrap());
        let updated_combo = cfg.game_combos.iter().find(|c| c.id == "combo-test").unwrap();
        assert_eq!(updated_combo.title, "CS2 Pro");
        assert_eq!(updated_combo.combo_targets.as_ref().unwrap().len(), 2);
        assert!(delete_game_combo(&mut cfg, "combo-test"));
        assert!(!cfg.game_combos.iter().any(|c| c.id == "combo-test"));

        // 5. Appearance settings
        assert!(set_appearance_settings(&mut cfg, Some("crystal".into()), Some(true), Some(true), Some(true)).unwrap());
        assert_eq!(cfg.glass_depth, GLASS_DEPTH_CRYSTAL);
        assert!(cfg.clock_show_seconds && cfg.clock_show_weekday && cfg.start_remember_space);
        assert!(set_appearance_settings(&mut cfg, Some("light".into()), None, None, None).is_err());

        // 6. Hidden tray keys
        assert!(hide_tray_key(&mut cfg, "Steam.EXE"));
        assert!(!hide_tray_key(&mut cfg, "steam.exe"));
        assert!(hide_tray_key(&mut cfg, "discord.exe"));
        assert_eq!(cfg.hidden_tray_keys, vec!["steam.exe", "discord.exe"]);
        assert!(restore_tray_key(&mut cfg, "STEAM.exe"));
        assert_eq!(cfg.hidden_tray_keys, vec!["discord.exe"]);
        assert!(restore_all_tray_keys(&mut cfg));
        assert!(cfg.hidden_tray_keys.is_empty());

        // 7. Frecency cap at MAX_FRECENCY_ENTRIES (200)
        for i in 0..240u64 {
            assert!(record_frecency(&mut cfg, &format!("item-{i}"), 10_000 + i));
        }
        assert_eq!(cfg.frecency.len(), MAX_FRECENCY_ENTRIES);
        assert!(cfg.frecency.contains_key("item-239"));
        assert!(!cfg.frecency.contains_key("item-0"));

        // 8. Export & import roundtrip + invalid rejection
        let exported = export_config_json(&cfg).expect("export json");
        let imported = import_config_json(&exported).expect("import json");
        assert_eq!(imported.glass_depth, GLASS_DEPTH_CRYSTAL);
        assert_eq!(imported.frecency.len(), MAX_FRECENCY_ENTRIES);
        assert!(import_config_json("").is_err());
        assert!(import_config_json("[]").is_err());
        assert!(import_config_json("{broken").is_err());
    }
}
