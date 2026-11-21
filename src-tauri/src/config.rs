use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
#[cfg(target_os = "windows")]
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
};

pub const CURRENT_SCHEMA_VERSION: u32 = 4;
pub const SECONDARY_TASKBAR_MODE_TESTBAR: &str = "testbar";
pub const SECONDARY_TASKBAR_MODE_NATIVE: &str = "native";
pub const SECONDARY_TASKBAR_MODE_HIDDEN: &str = "hidden";

pub const GLASS_DEPTH_CRYSTAL: &str = "crystal";
pub const GLASS_DEPTH_OBSIDIAN: &str = "obsidian";
pub const GLASS_DEPTH_MATTE: &str = "matte";
pub const GLASS_DEPTH_SOLID: &str = "solid";

pub const THEME_PRESET_OBSIDIAN: &str = "obsidian";
pub const THEME_PRESET_CARBON: &str = "carbon";
pub const THEME_PRESET_MIDNIGHT: &str = "midnight";
pub const THEME_PRESET_EMERALD: &str = "emerald";
pub const THEME_PRESET_AMETHYST: &str = "amethyst";
pub const THEME_PRESET_GRAPHITE: &str = "graphite";

pub const THEME_PRESETS: &[&str] = &[
    THEME_PRESET_OBSIDIAN,
    THEME_PRESET_CARBON,
    THEME_PRESET_MIDNIGHT,
    THEME_PRESET_EMERALD,
    THEME_PRESET_AMETHYST,
    THEME_PRESET_GRAPHITE,
];

pub const ACCENT_MODE_PER_SPACE: &str = "per_space";
pub const ACCENT_MODE_CUSTOM: &str = "custom";

pub const CLOCK_DATE_SHORT: &str = "short";
pub const CLOCK_DATE_NUMERIC: &str = "numeric";
pub const CLOCK_DATE_NONE: &str = "none";

pub const CLOCK_LAYOUT_CENTER: &str = "center";
pub const CLOCK_LAYOUT_RIGHT: &str = "right";
pub const CLOCK_LAYOUT_SINGLE: &str = "single";

pub const HEADER_WIDGET_SIDE_AUTO: &str = "auto";
pub const HEADER_WIDGET_SIDE_LEFT: &str = "left";
pub const HEADER_WIDGET_SIDE_RIGHT: &str = "right";

pub const TASKBAR_WIDGETS_POSITION_LEFT: &str = "left";
pub const TASKBAR_WIDGETS_POSITION_RIGHT: &str = "right";

pub const WIDGET_PRESET_MEDIA_CLIPBOARD: &str = "media_clipboard";
pub const WIDGET_PRESET_TELEMETRY_PROCESSES: &str = "telemetry_processes";
pub const WIDGET_PRESET_AUDIO_MIC: &str = "audio_mic";
pub const WIDGET_PRESET_FULL_GRID: &str = "full_grid";

pub const SPACE_LAYOUT_GRID: &str = "grid";
pub const SPACE_LAYOUT_GAMES: &str = "games";
pub const TILE_SIZE_SMALL: &str = "small";
pub const TILE_SIZE_HERO: &str = "hero";
pub const TILE_SIZE_COMPACT: &str = "compact";
pub const TILE_SECTION_MAIN: &str = "main";
pub const TILE_SECTION_BOTTOM: &str = "bottom";
pub const MAX_SPACES: usize = 12;

pub const SPACE_WIDGET_IDS: &[&str] = &[
    "media",
    "audio_mixer",
    "clipboard",
    "pc_health",
    "top_processes",
    "game_combos",
    "weather",
    "batteries",
    "notes",
    "timer",
    "network",
    "quick_toggles",
];

pub const TASKBAR_WIDGET_IDS: &[&str] = &[
    "weather",
    "telemetry",
    "batteries",
    "network",
    "media",
    "timer",
];

pub const MAX_QUICK_NOTES_CHARS: usize = 8_000;
pub const MAX_QUICK_TODOS: usize = 40;
pub const MAX_QUICK_TODO_CHARS: usize = 240;

pub const MAX_FRECENCY_ENTRIES: usize = 200;
pub const MAX_CUSTOM_ICON_DATA_BYTES: usize = 128 * 1024;
const SAVE_RENAME_RETRIES: usize = 3;
const SAVE_RENAME_RETRY_DELAY_MS: u64 = 15;
const MAX_IMPORT_CONFIG_BYTES: usize = 4 * 1024 * 1024;

pub fn validate_space_widget_id(id: &str) -> Result<&'static str, String> {
    let norm = id.trim().to_ascii_lowercase();
    for &valid in SPACE_WIDGET_IDS {
        if norm == valid {
            return Ok(valid);
        }
    }
    Err(format!(
        "Недопустимый виджет пространства: '{id}' (допустимы: {})",
        SPACE_WIDGET_IDS.join(", ")
    ))
}

pub fn normalize_space_widgets(widgets: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(widgets.len().min(SPACE_WIDGET_IDS.len()));
    let mut seen = HashSet::new();
    for w in widgets {
        if let Ok(valid) = validate_space_widget_id(w) {
            if seen.insert(valid) {
                out.push(valid.to_string());
            }
        }
    }
    out
}

pub fn validate_and_normalize_space_widgets(widgets: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(widgets.len().min(SPACE_WIDGET_IDS.len()));
    let mut seen = HashSet::new();
    for w in widgets {
        let valid = validate_space_widget_id(w)?;
        if seen.insert(valid) {
            out.push(valid.to_string());
        }
    }
    Ok(out)
}

pub fn default_space_layout_kind() -> String {
    SPACE_LAYOUT_GRID.to_string()
}

pub fn normalize_space_layout_kind(kind: &str) -> &'static str {
    match kind.trim().to_ascii_lowercase().as_str() {
        "games" => SPACE_LAYOUT_GAMES,
        _ => SPACE_LAYOUT_GRID,
    }
}

pub fn validate_space_layout_kind(kind: &str) -> Result<&'static str, String> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "grid" => Ok(SPACE_LAYOUT_GRID),
        "games" => Ok(SPACE_LAYOUT_GAMES),
        other => Err(format!(
            "Недопустимый тип сетки пространства: '{other}' (допустимы: grid, games)"
        )),
    }
}

pub fn normalize_tile_size(size: &str) -> Option<&'static str> {
    match size.trim().to_ascii_lowercase().as_str() {
        "small" => Some(TILE_SIZE_SMALL),
        "hero" => Some(TILE_SIZE_HERO),
        "compact" => Some(TILE_SIZE_COMPACT),
        _ => None,
    }
}

pub fn validate_tile_size(size: &str) -> Result<&'static str, String> {
    match size.trim().to_ascii_lowercase().as_str() {
        "small" => Ok(TILE_SIZE_SMALL),
        "hero" => Ok(TILE_SIZE_HERO),
        "compact" => Ok(TILE_SIZE_COMPACT),
        other => Err(format!(
            "Недопустимый размер плитки: '{other}' (допустимы: small, hero, compact)"
        )),
    }
}

pub fn default_tile_size() -> String {
    TILE_SIZE_COMPACT.to_string()
}

pub fn default_clock_date_format() -> String {
    CLOCK_DATE_SHORT.to_string()
}

pub fn normalize_clock_date_format(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "numeric" => CLOCK_DATE_NUMERIC,
        "none" => CLOCK_DATE_NONE,
        _ => CLOCK_DATE_SHORT,
    }
}

pub fn validate_clock_date_format(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "short" => Ok(CLOCK_DATE_SHORT),
        "numeric" => Ok(CLOCK_DATE_NUMERIC),
        "none" => Ok(CLOCK_DATE_NONE),
        other => Err(format!(
            "Недопустимый формат даты: '{other}' (допустимы: short, numeric, none)"
        )),
    }
}

pub fn default_clock_layout() -> String {
    CLOCK_LAYOUT_CENTER.to_string()
}

pub fn normalize_clock_layout(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "right" => CLOCK_LAYOUT_RIGHT,
        "single" => CLOCK_LAYOUT_SINGLE,
        _ => CLOCK_LAYOUT_CENTER,
    }
}

pub fn validate_clock_layout(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "center" => Ok(CLOCK_LAYOUT_CENTER),
        "right" => Ok(CLOCK_LAYOUT_RIGHT),
        "single" => Ok(CLOCK_LAYOUT_SINGLE),
        other => Err(format!(
            "Недопустимый вид часов: '{other}' (допустимы: center, right, single)"
        )),
    }
}

pub fn default_header_widget_side() -> String {
    HEADER_WIDGET_SIDE_AUTO.to_string()
}

pub fn normalize_header_widget_side(side: &str) -> &'static str {
    match side.trim().to_ascii_lowercase().as_str() {
        "left" => HEADER_WIDGET_SIDE_LEFT,
        "right" => HEADER_WIDGET_SIDE_RIGHT,
        _ => HEADER_WIDGET_SIDE_AUTO,
    }
}

pub fn validate_header_widget_side(side: &str) -> Result<&'static str, String> {
    match side.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(HEADER_WIDGET_SIDE_AUTO),
        "left" => Ok(HEADER_WIDGET_SIDE_LEFT),
        "right" => Ok(HEADER_WIDGET_SIDE_RIGHT),
        other => Err(format!(
            "Недопустимая сторона виджета шапки: '{other}' (допустимы: auto, left, right)"
        )),
    }
}

pub fn validate_taskbar_widget_id(id: &str) -> Result<&'static str, String> {
    let norm = id.trim().to_ascii_lowercase();
    for &valid in TASKBAR_WIDGET_IDS {
        if norm == valid {
            return Ok(valid);
        }
    }
    Err(format!(
        "Недопустимый виджет нижней панели: '{id}' (допустимы: {})",
        TASKBAR_WIDGET_IDS.join(", ")
    ))
}

pub fn normalize_taskbar_widget_list(list: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(list.len().min(TASKBAR_WIDGET_IDS.len()));
    let mut seen = HashSet::new();
    for w in list {
        if let Ok(valid) = validate_taskbar_widget_id(w) {
            if seen.insert(valid) {
                out.push(valid.to_string());
            }
        }
    }
    out
}

pub fn validate_taskbar_widget_list(list: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(list.len().min(TASKBAR_WIDGET_IDS.len()));
    let mut seen = HashSet::new();
    for w in list {
        let valid = validate_taskbar_widget_id(w)?;
        if seen.insert(valid) {
            out.push(valid.to_string());
        }
    }
    Ok(out)
}

pub fn default_taskbar_widgets_position() -> String {
    TASKBAR_WIDGETS_POSITION_LEFT.to_string()
}

pub fn normalize_taskbar_widgets_position(pos: &str) -> &'static str {
    match pos.trim().to_ascii_lowercase().as_str() {
        "right" => TASKBAR_WIDGETS_POSITION_RIGHT,
        _ => TASKBAR_WIDGETS_POSITION_LEFT,
    }
}

pub fn validate_taskbar_widgets_position(pos: &str) -> Result<&'static str, String> {
    match pos.trim().to_ascii_lowercase().as_str() {
        "left" => Ok(TASKBAR_WIDGETS_POSITION_LEFT),
        "right" => Ok(TASKBAR_WIDGETS_POSITION_RIGHT),
        other => Err(format!(
            "Недопустимое положение виджетов панели: '{other}' (допустимы: left, right)"
        )),
    }
}

pub fn default_theme_preset() -> String {
    THEME_PRESET_OBSIDIAN.to_string()
}

pub fn normalize_theme_preset(theme: &str) -> &'static str {
    let norm = theme.trim().to_ascii_lowercase();
    for &valid in THEME_PRESETS {
        if norm == valid {
            return valid;
        }
    }
    THEME_PRESET_OBSIDIAN
}

pub fn validate_theme_preset(theme: &str) -> Result<&'static str, String> {
    let norm = theme.trim().to_ascii_lowercase();
    for &valid in THEME_PRESETS {
        if norm == valid {
            return Ok(valid);
        }
    }
    Err(format!(
        "Недопустимая тема оформления: '{theme}' (допустимы: {})",
        THEME_PRESETS.join(", ")
    ))
}

pub fn default_accent_mode() -> String {
    ACCENT_MODE_PER_SPACE.to_string()
}

pub fn normalize_accent_mode(mode: &str) -> &'static str {
    match mode.trim().to_ascii_lowercase().as_str() {
        "custom" => ACCENT_MODE_CUSTOM,
        _ => ACCENT_MODE_PER_SPACE,
    }
}

pub fn validate_accent_mode(mode: &str) -> Result<&'static str, String> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "per_space" => Ok(ACCENT_MODE_PER_SPACE),
        "custom" => Ok(ACCENT_MODE_CUSTOM),
        other => Err(format!(
            "Недопустимый режим акцента: '{other}' (допустимы: per_space, custom)"
        )),
    }
}

pub fn default_global_accent_hex() -> String {
    "#818CF8".to_string()
}

pub fn default_global_accent_rgb() -> String {
    "129,140,248".to_string()
}

pub fn default_true() -> bool {
    true
}

pub fn normalize_tile_section(section: &str) -> Option<&'static str> {
    match section.trim().to_ascii_lowercase().as_str() {
        "main" => Some(TILE_SECTION_MAIN),
        "bottom" => Some(TILE_SECTION_BOTTOM),
        _ => None,
    }
}

pub fn validate_tile_section(section: &str) -> Result<&'static str, String> {
    match section.trim().to_ascii_lowercase().as_str() {
        "main" => Ok(TILE_SECTION_MAIN),
        "bottom" => Ok(TILE_SECTION_BOTTOM),
        other => Err(format!(
            "Недопустимая секция плитки: '{other}' (допустимы: main, bottom)"
        )),
    }
}

pub fn default_weather_units() -> String {
    "c".to_string()
}

pub fn normalize_weather_units(units: &str) -> &'static str {
    match units.trim().to_ascii_lowercase().as_str() {
        "f" => "f",
        _ => "c",
    }
}

pub fn validate_weather_units(units: &str) -> Result<&'static str, String> {
    match units.trim().to_ascii_lowercase().as_str() {
        "c" => Ok("c"),
        "f" => Ok("f"),
        other => Err(format!(
            "Недопустимые единицы температуры: '{other}' (допустимы: c, f)"
        )),
    }
}

pub fn widget_preset_to_widgets(preset: &str) -> Vec<String> {
    match normalize_widget_preset(preset) {
        WIDGET_PRESET_TELEMETRY_PROCESSES => {
            vec!["pc_health".to_string(), "top_processes".to_string()]
        }
        WIDGET_PRESET_AUDIO_MIC => vec!["audio_mixer".to_string(), "media".to_string()],
        WIDGET_PRESET_FULL_GRID => vec![],
        _ => vec![
            "media".to_string(),
            "audio_mixer".to_string(),
            "clipboard".to_string(),
        ],
    }
}

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
        "solid" => GLASS_DEPTH_SOLID,
        _ => GLASS_DEPTH_OBSIDIAN,
    }
}

pub fn validate_glass_depth(depth: &str) -> Result<&'static str, String> {
    match depth.trim().to_ascii_lowercase().as_str() {
        "crystal" => Ok(GLASS_DEPTH_CRYSTAL),
        "obsidian" => Ok(GLASS_DEPTH_OBSIDIAN),
        "matte" => Ok(GLASS_DEPTH_MATTE),
        "solid" => Ok(GLASS_DEPTH_SOLID),
        other => Err(format!(
            "Недопустимая глубина стекла: '{other}' (допустимы: crystal, obsidian, matte, solid)"
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

fn parse_hex_color(raw: &str) -> Option<(u8, u8, u8, String)> {
    let trimmed = raw.trim();
    if trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        let r = u8::from_str_radix(&trimmed[1..3], 16).ok()?;
        let g = u8::from_str_radix(&trimmed[3..5], 16).ok()?;
        let b = u8::from_str_radix(&trimmed[5..7], 16).ok()?;
        return Some((r, g, b, trimmed.to_uppercase()));
    }
    None
}

fn parse_rgb_color(raw: &str) -> Option<(u8, u8, u8, String)> {
    let parts: Vec<&str> = raw.trim().split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    Some((r, g, b, format!("{r},{g},{b}")))
}

pub fn sanitize_accent_hex_rgb(
    accent_hex: Option<&str>,
    accent_rgb: Option<&str>,
) -> (String, String) {
    let parsed_hex = accent_hex.and_then(parse_hex_color);
    let parsed_rgb = accent_rgb.and_then(parse_rgb_color);

    match (parsed_hex, parsed_rgb) {
        (Some((hr, hg, hb, hex)), Some((_, _, _, rgb))) => {
            let _ = (hr, hg, hb);
            (hex, rgb)
        }
        (Some((r, g, b, hex)), None) => (hex, format!("{r},{g},{b}")),
        (None, Some((r, g, b, rgb))) => (format!("#{r:02X}{g:02X}{b:02X}"), rgb),
        (None, None) => ("#A78BFA".to_string(), "167,139,250".to_string()),
    }
}

#[cfg(target_os = "windows")]
const AUTOSTART_RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const AUTOSTART_VALUE_NAME: &str = "TestBar";

pub const MAC_COMMAND_KEY_ANY: &str = "any";
pub const MAC_COMMAND_KEY_LEFT: &str = "left";
pub const MAC_COMMAND_KEY_RIGHT: &str = "right";
pub const MAC_COMMAND_KEY_DISABLED: &str = "disabled";

pub const MAC_WINDOW_POSITION_CENTER: &str = "center";
pub const MAC_WINDOW_POSITION_BOTTOM: &str = "bottom";

pub const MAC_TERMINAL_AUTO: &str = "auto";
pub const MAC_TERMINAL_TERMINAL: &str = "terminal";
pub const MAC_TERMINAL_ITERM: &str = "iterm";
pub const MAC_TERMINAL_WARP: &str = "warp";
pub const MAC_TERMINAL_GHOSTTY: &str = "ghostty";

pub fn default_mac_command_key() -> String {
    MAC_COMMAND_KEY_ANY.to_string()
}

pub fn normalize_mac_command_key(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "left" => MAC_COMMAND_KEY_LEFT,
        "right" => MAC_COMMAND_KEY_RIGHT,
        "disabled" => MAC_COMMAND_KEY_DISABLED,
        _ => MAC_COMMAND_KEY_ANY,
    }
}

pub fn validate_mac_command_key(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "any" => Ok(MAC_COMMAND_KEY_ANY),
        "left" => Ok(MAC_COMMAND_KEY_LEFT),
        "right" => Ok(MAC_COMMAND_KEY_RIGHT),
        "disabled" => Ok(MAC_COMMAND_KEY_DISABLED),
        other => Err(format!(
            "Недопустимый режим клавиши Command: '{other}' (допустимы: any, left, right, disabled)"
        )),
    }
}

pub fn default_mac_window_position() -> String {
    MAC_WINDOW_POSITION_CENTER.to_string()
}

pub fn normalize_mac_window_position(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "bottom" => MAC_WINDOW_POSITION_BOTTOM,
        _ => MAC_WINDOW_POSITION_CENTER,
    }
}

pub fn validate_mac_window_position(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "center" => Ok(MAC_WINDOW_POSITION_CENTER),
        "bottom" => Ok(MAC_WINDOW_POSITION_BOTTOM),
        other => Err(format!(
            "Недопустимая позиция окна: '{other}' (допустимы: center, bottom)"
        )),
    }
}

pub fn default_mac_terminal_app() -> String {
    MAC_TERMINAL_AUTO.to_string()
}

pub fn normalize_mac_terminal_app(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "terminal" => MAC_TERMINAL_TERMINAL,
        "iterm" => MAC_TERMINAL_ITERM,
        "warp" => MAC_TERMINAL_WARP,
        "ghostty" => MAC_TERMINAL_GHOSTTY,
        _ => MAC_TERMINAL_AUTO,
    }
}

pub fn validate_mac_terminal_app(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(MAC_TERMINAL_AUTO),
        "terminal" => Ok(MAC_TERMINAL_TERMINAL),
        "iterm" => Ok(MAC_TERMINAL_ITERM),
        "warp" => Ok(MAC_TERMINAL_WARP),
        "ghostty" => Ok(MAC_TERMINAL_GHOSTTY),
        other => Err(format!(
            "Недопустимый терминал: '{other}' (допустимы: auto, terminal, iterm, warp, ghostty)"
        )),
    }
}

pub fn set_mac_settings_in_config(
    cfg: &mut TestBarConfig,
    mac_command_key: Option<String>,
    mac_window_position: Option<String>,
    mac_terminal_app: Option<String>,
) -> Result<bool, String> {
    let mut changed = false;
    if let Some(cmd) = mac_command_key {
        let valid = validate_mac_command_key(&cmd)?;
        if cfg.mac_command_key != valid {
            cfg.mac_command_key = valid.to_string();
            changed = true;
        }
    }
    if let Some(pos) = mac_window_position {
        let valid = validate_mac_window_position(&pos)?;
        if cfg.mac_window_position != valid {
            cfg.mac_window_position = valid.to_string();
            changed = true;
        }
    }
    if let Some(term) = mac_terminal_app {
        let valid = validate_mac_terminal_app(&term)?;
        if cfg.mac_terminal_app != valid {
            cfg.mac_terminal_app = valid.to_string();
            changed = true;
        }
    }
    Ok(changed)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default)]
    pub run_as_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpaceHeaderWidget {
    pub id: String,
    /// "auto" | "left" | "right"
    #[serde(default = "default_header_widget_side")]
    pub side: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuickTodoItem {
    pub id: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct QuickNotesState {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub todos: Vec<QuickTodoItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpaceConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub accent_hex: String,
    #[serde(default)]
    pub accent_rgb: String,
    #[serde(default = "default_space_layout_kind")]
    pub layout_kind: String,
    #[serde(default)]
    pub show_hero_row: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom_section_title: Option<String>,
    #[serde(default)]
    pub widgets: Vec<String>,
    #[serde(default)]
    pub header_widgets: Vec<SpaceHeaderWidget>,
    #[serde(default)]
    pub items: Vec<PinItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarWidgetSettings {
    #[serde(default)]
    pub show_weather: bool,
    #[serde(default)]
    pub show_batteries: bool,
    #[serde(default)]
    pub show_telemetry: bool,
    #[serde(default)]
    pub left_widgets: Vec<String>,
    #[serde(default = "default_taskbar_widgets_position")]
    pub widgets_position: String,
    #[serde(default)]
    pub weather_city: String,
    #[serde(default)]
    pub weather_lat: Option<f64>,
    #[serde(default)]
    pub weather_lon: Option<f64>,
    #[serde(default = "default_weather_units")]
    pub weather_units: String,
}

impl Default for TaskbarWidgetSettings {
    fn default() -> Self {
        Self {
            show_weather: false,
            show_batteries: false,
            show_telemetry: false,
            left_widgets: Vec::new(),
            widgets_position: default_taskbar_widgets_position(),
            weather_city: String::new(),
            weather_lat: None,
            weather_lon: None,
            weather_units: default_weather_units(),
        }
    }
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
    pub glass_depth: String, // "crystal" | "obsidian" | "matte" | "solid"
    #[serde(default = "default_theme_preset")]
    pub theme_preset: String, // "obsidian" | "carbon" | "midnight" | "emerald" | "amethyst" | "graphite"
    #[serde(default = "default_accent_mode")]
    pub accent_mode: String, // "per_space" | "custom"
    #[serde(default = "default_global_accent_hex")]
    pub accent_hex: String,
    #[serde(default = "default_global_accent_rgb")]
    pub accent_rgb: String,
    #[serde(default = "default_true")]
    pub surface_gradients: bool,
    #[serde(default = "default_true")]
    pub hover_glow: bool,
    #[serde(default)]
    pub clock_show_seconds: bool,
    #[serde(default)]
    pub clock_show_weekday: bool,
    #[serde(default = "default_clock_date_format")]
    pub clock_date_format: String, // "short" | "numeric" | "none"
    #[serde(default = "default_clock_layout")]
    pub clock_layout: String, // "center" | "right" | "single"
    #[serde(default)]
    pub start_remember_space: bool,
    #[serde(default = "default_tile_size")]
    pub default_tile_size: String, // "small" | "compact" | "hero"
    #[serde(default)]
    pub quick_notes: QuickNotesState,
    #[serde(default)]
    pub hidden_tray_keys: Vec<String>,
    #[serde(default)]
    pub hidden_battery_ids: Vec<String>,
    #[serde(default)]
    pub frecency: HashMap<String, FrecencyEntry>,
    #[serde(default)]
    pub taskbar_pins: Vec<PinItem>,
    #[serde(default)]
    pub spaces: Vec<SpaceConfig>,
    #[serde(default)]
    pub taskbar_widgets: TaskbarWidgetSettings,
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
    #[serde(default = "default_mac_command_key")]
    pub mac_command_key: String, // "any" | "left" | "right" | "disabled"
    #[serde(default = "default_mac_window_position")]
    pub mac_window_position: String, // "center" | "bottom"
    #[serde(default = "default_mac_terminal_app")]
    pub mac_terminal_app: String, // "auto" | "terminal" | "iterm" | "warp" | "ghostty"
}

fn find_first_existing(candidates: &[PathBuf]) -> Option<String> {
    for p in candidates {
        let s = p.to_string_lossy();
        if path_exists_including_reparse(&s) {
            return Some(s.into_owned());
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn user_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"))
}

#[cfg(not(target_os = "windows"))]
fn user_home() -> PathBuf {
    dirs::home_dir()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

#[cfg(target_os = "windows")]
fn local_appdata() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| user_home().join(r"AppData\Local"))
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn local_appdata() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| user_home().join("Library").join("Application Support"))
}

#[cfg(target_os = "windows")]
fn roaming_appdata() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| user_home().join(r"AppData\Roaming"))
}

#[cfg(not(target_os = "windows"))]
fn roaming_appdata() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| user_home().join("Library").join("Application Support"))
}

#[cfg(target_os = "windows")]
fn read_registry_sz(root: HKEY, subkey: &str, value_name: &str) -> Option<String> {
    let subkey_w = to_wide_nul(subkey);
    let value_w = if value_name.is_empty() {
        Vec::new()
    } else {
        to_wide_nul(value_name)
    };
    let value_ptr = if value_name.is_empty() {
        PCWSTR::null()
    } else {
        PCWSTR(value_w.as_ptr())
    };
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut byte_len: u32 = 0;
    let status = unsafe {
        RegGetValueW(
            root,
            PCWSTR(subkey_w.as_ptr()),
            value_ptr,
            flags,
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
            value_ptr,
            flags,
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

pub fn parse_browser_command_exe(cmd: &str) -> Option<String> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return None;
    }
    let raw_exe = if let Some(after_open) = trimmed.strip_prefix('"') {
        let close_idx = after_open.find('"')?;
        after_open[..close_idx].trim()
    } else {
        let lower = trimmed.to_ascii_lowercase();
        if let Some(pos) = lower.find(".exe") {
            let end = pos + 4;
            trimmed[..end].trim()
        } else {
            trimmed.split_whitespace().next().unwrap_or("").trim()
        }
    };
    let stripped = strip_verbatim_prefix(raw_exe);
    let cleaned = stripped.trim();
    let lower = cleaned.to_ascii_lowercase();
    if !lower.ends_with(".exe")
        || lower.ends_with("\\launchwinapp.exe")
        || lower.ends_with("\\rundll32.exe")
        || lower.ends_with("\\cmd.exe")
        || lower.ends_with("\\wscript.exe")
    {
        return None;
    }
    Some(cleaned.to_string())
}

pub fn browser_display_info(
    exe_path: &str,
    prog_id: Option<&str>,
) -> (String, &'static str, &'static str) {
    let exe_low = derive_match_exe(exe_path).unwrap_or_default();
    let prog_low = prog_id.unwrap_or("").trim().to_ascii_lowercase();
    match exe_low.as_str() {
        "msedge.exe" => ("Microsoft Edge".to_string(), "edge", "56,189,248"),
        "chrome.exe" => ("Google Chrome".to_string(), "chrome", "56,189,248"),
        "firefox.exe" => ("Firefox".to_string(), "firefox", "249,115,22"),
        "brave.exe" => ("Brave".to_string(), "brave", "249,115,22"),
        "opera.exe" | "launcher.exe" if prog_low.contains("opera") || exe_low == "opera.exe" => {
            ("Opera".to_string(), "opera", "239,68,68")
        }
        "browser.exe" if prog_low.contains("yandex") || exe_path.to_ascii_lowercase().contains("yandex") => {
            ("Яндекс Браузер".to_string(), "yandex", "239,68,68")
        }
        "vivaldi.exe" => ("Vivaldi".to_string(), "browser", "239,68,68"),
        "arc.exe" => ("Arc".to_string(), "browser", "129,140,248"),
        _ => {
            let stem = exe_low.trim_end_matches(".exe");
            let title = if stem.is_empty() {
                "Браузер".to_string()
            } else {
                let mut chars = stem.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => "Браузер".to_string(),
                }
            };
            (title, "browser", "56,189,248")
        }
    }
}

#[cfg(target_os = "windows")]
struct DetectedBrowser {
    title: String,
    target: String,
    icon_key: String,
    accent_rgb: String,
}

#[cfg(target_os = "windows")]
fn detect_default_browser() -> Option<DetectedBrowser> {
    let prog_id = read_registry_sz(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
        "ProgId",
    );

    if let Some(ref pid) = prog_id {
        let subkey = format!(r"Software\Classes\{pid}\shell\open\command");
        let cmd = read_registry_sz(HKEY_CURRENT_USER, &subkey, "")
            .or_else(|| read_registry_sz(HKEY_LOCAL_MACHINE, &subkey, ""));
        if let Some(cmd_str) = cmd {
            if let Some(exe) = parse_browser_command_exe(&cmd_str) {
                if path_exists_including_reparse(&exe) {
                    let (title, icon_key, accent_rgb) = browser_display_info(&exe, Some(pid));
                    return Some(DetectedBrowser {
                        title,
                        target: exe,
                        icon_key: icon_key.to_string(),
                        accent_rgb: accent_rgb.to_string(),
                    });
                }
            }
        }

        let pid_low = pid.to_ascii_lowercase();
        let hinted_key = if pid_low.contains("edge") {
            Some("edge")
        } else if pid_low.contains("chrome") {
            Some("chrome")
        } else if pid_low.contains("firefox") {
            Some("firefox")
        } else if pid_low.contains("brave") {
            Some("brave")
        } else if pid_low.contains("opera") {
            Some("opera")
        } else if pid_low.contains("yandex") {
            Some("yandex")
        } else {
            None
        };
        if let Some(key) = hinted_key {
            let target = detect_app_target(key);
            if !target.is_empty() {
                let (title, icon_key, accent_rgb) = browser_display_info(&target, Some(pid));
                return Some(DetectedBrowser {
                    title,
                    target,
                    icon_key: icon_key.to_string(),
                    accent_rgb: accent_rgb.to_string(),
                });
            }
        }
    }

    for key in ["edge", "chrome", "firefox", "brave", "opera", "yandex"] {
        let target = detect_app_target(key);
        if !target.is_empty() {
            let (title, icon_key, accent_rgb) = browser_display_info(&target, None);
            return Some(DetectedBrowser {
                title,
                target,
                icon_key: icon_key.to_string(),
                accent_rgb: accent_rgb.to_string(),
            });
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn detect_adobe_photoshop(pf: &Path) -> Vec<PathBuf> {
    let mut dirs_found = Vec::new();
    let adobe_dir = pf.join("Adobe");
    if let Ok(entries) = fs::read_dir(&adobe_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().starts_with("adobe photoshop") {
                let exe = entry.path().join("Photoshop.exe");
                if exe.is_file() {
                    dirs_found.push(exe);
                }
            }
        }
    }
    dirs_found.sort_by(|a, b| b.cmp(a));
    dirs_found
}

#[cfg(target_os = "windows")]
fn detect_app_target(name_key: &str) -> String {
    let local = local_appdata();
    let roaming = roaming_appdata();
    let pf = PathBuf::from(r"C:\Program Files");
    let pf86 = PathBuf::from(r"C:\Program Files (x86)");

    if name_key == "snipping" {
        return "ms-screenclip:".to_string();
    }

    let candidates: Vec<PathBuf> = match name_key {
        "explorer" => vec![PathBuf::from(r"C:\Windows\explorer.exe")],
        "wt" => vec![
            local.join(r"Microsoft\WindowsApps\wt.exe"),
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            PathBuf::from(r"C:\Windows\System32\cmd.exe"),
        ],
        "notepad" => vec![
            PathBuf::from(r"C:\Windows\System32\notepad.exe"),
            PathBuf::from(r"C:\Windows\notepad.exe"),
        ],
        "taskmgr" => vec![PathBuf::from(r"C:\Windows\System32\Taskmgr.exe")],
        "edge" => vec![
            pf86.join(r"Microsoft\Edge\Application\msedge.exe"),
            pf.join(r"Microsoft\Edge\Application\msedge.exe"),
        ],
        "chrome" => vec![
            pf.join(r"Google\Chrome\Application\chrome.exe"),
            pf86.join(r"Google\Chrome\Application\chrome.exe"),
            local.join(r"Google\Chrome\Application\chrome.exe"),
        ],
        "firefox" => vec![
            pf.join(r"Mozilla Firefox\firefox.exe"),
            pf86.join(r"Mozilla Firefox\firefox.exe"),
        ],
        "brave" => vec![
            pf.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"),
            local.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"),
            pf86.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"),
        ],
        "opera" => vec![
            local.join(r"Programs\Opera\opera.exe"),
            local.join(r"Programs\Opera GX\opera.exe"),
            pf.join(r"Opera\opera.exe"),
        ],
        "yandex" => vec![
            local.join(r"Yandex\YandexBrowser\Application\browser.exe"),
            pf.join(r"Yandex\YandexBrowser\Application\browser.exe"),
            pf86.join(r"Yandex\YandexBrowser\Application\browser.exe"),
        ],
        "vscode" => vec![
            local.join(r"Programs\Microsoft VS Code\Code.exe"),
            pf.join(r"Microsoft VS Code\Code.exe"),
            pf86.join(r"Microsoft VS Code\Code.exe"),
        ],
        "telegram" => vec![
            roaming.join(r"Telegram Desktop\Telegram.exe"),
            local.join(r"Programs\Telegram Desktop\Telegram.exe"),
            local.join(r"Telegram Desktop\Telegram.exe"),
        ],
        "discord" => vec![local.join(r"Discord\Update.exe")],
        "steam" => {
            if let Some(detected) = crate::apps_indexer::detect_steam_exe() {
                return detected;
            }
            vec![
                pf86.join(r"Steam\steam.exe"),
                pf.join(r"Steam\steam.exe"),
            ]
        }
        "obs" => vec![
            pf.join(r"obs-studio\bin\64bit\obs64.exe"),
            pf86.join(r"obs-studio\bin\64bit\obs64.exe"),
        ],
        "figma" => vec![local.join(r"Figma\Figma.exe")],
        "photoshop" => detect_adobe_photoshop(&pf),
        "vlc" => vec![
            pf.join(r"VideoLAN\VLC\vlc.exe"),
            pf86.join(r"VideoLAN\VLC\vlc.exe"),
        ],
        "spotify" => vec![
            roaming.join(r"Spotify\Spotify.exe"),
            local.join(r"Microsoft\WindowsApps\Spotify.exe"),
        ],
        _ => vec![],
    };

    find_first_existing(&candidates).unwrap_or_else(|| match name_key {
        "explorer" => r"C:\Windows\explorer.exe".to_string(),
        "notepad" => r"C:\Windows\System32\notepad.exe".to_string(),
        "taskmgr" => r"C:\Windows\System32\Taskmgr.exe".to_string(),
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
        custom_icon: None,
        custom_color: None,
        tile_size: None,
        section: None,
        run_as_admin: false,
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
        custom_icon: None,
        custom_color: None,
        tile_size: None,
        section: None,
        run_as_admin: false,
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

fn sanitize_custom_color(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((_, _, _, hex)) = parse_hex_color(trimmed) {
        return Some(hex);
    }
    if let Some((_, _, _, rgb)) = parse_rgb_color(trimmed) {
        return Some(rgb);
    }
    Some(trimmed.to_string())
}

fn sanitize_optional_nonempty(opt: &Option<String>) -> Option<String> {
    opt.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub fn sanitize_custom_icon_data(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_CUSTOM_ICON_DATA_BYTES {
        return None;
    }
    let low = trimmed.to_ascii_lowercase();
    let prefix = [
        "data:image/png;base64,",
        "data:image/jpeg;base64,",
        "data:image/jpg;base64,",
        "data:image/webp;base64,",
        "data:image/gif;base64,",
        "data:image/bmp;base64,",
        "data:image/x-icon;base64,",
        "data:image/vnd.microsoft.icon;base64,",
    ]
    .iter()
    .copied()
    .find(|p| low.starts_with(p))?;
    let payload = &trimmed[prefix.len()..];
    if payload.is_empty()
        || !payload
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    {
        return None;
    }
    Some(trimmed.to_string())
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

    // Auto-heal broken non-launchable UWP host pins (`SystemSettings.exe`, `ApplicationFrameHost.exe`, `\WindowsApps\`)
    let mut healed_uwp_host = false;
    if item.kind != "combo" {
        let target_norm = item
            .target
            .trim()
            .trim_matches('"')
            .replace('/', "\\")
            .to_ascii_lowercase();
        if target_norm.ends_with(r"\systemsettings.exe") || target_norm == "systemsettings.exe" {
            const SETTINGS_AUMID: &str =
                "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel";
            let healed_target = format!(r"shell:AppsFolder\{SETTINGS_AUMID}");
            if item.target != healed_target {
                item.target = healed_target;
                changed = true;
            }
            if item.match_aumid.as_deref() != Some(SETTINGS_AUMID) {
                item.match_aumid = Some(SETTINGS_AUMID.to_string());
                changed = true;
            }
            healed_uwp_host = true;
        } else if target_norm.ends_with(r"\applicationframehost.exe")
            || target_norm == "applicationframehost.exe"
            || target_norm.contains(r"\windowsapps\")
        {
            if let Some(ref aumid) = item.match_aumid {
                let trimmed_aumid = aumid.trim();
                let clean_aumid = if trimmed_aumid
                    .get(..17)
                    .is_some_and(|p| p.eq_ignore_ascii_case("shell:appsfolder\\"))
                {
                    trimmed_aumid[17..].trim()
                } else {
                    trimmed_aumid
                };
                if !clean_aumid.is_empty() {
                    let healed_target = format!(r"shell:AppsFolder\{clean_aumid}");
                    if item.target != healed_target {
                        item.target = healed_target;
                        changed = true;
                    }
                    if item.match_aumid.as_deref() != Some(clean_aumid) {
                        item.match_aumid = Some(clean_aumid.to_string());
                        changed = true;
                    }
                    healed_uwp_host = true;
                }
            }
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
        let filtered = if healed_uwp_host
            && matches!(
                normalized_existing_exe.as_deref(),
                Some("applicationframehost.exe")
            ) {
            None
        } else {
            normalized_existing_exe
        };
        filtered.or_else(|| derive_match_exe(&item.target))
    } else {
        normalized_existing_exe
    };
    if item.match_exe != desired_exe {
        item.match_exe = desired_exe;
        changed = true;
    }

    let clean_custom_icon = sanitize_optional_nonempty(&item.custom_icon);
    if item.custom_icon != clean_custom_icon {
        item.custom_icon = clean_custom_icon;
        changed = true;
    }

    let clean_custom_color = item.custom_color.as_deref().and_then(sanitize_custom_color);
    if item.custom_color != clean_custom_color {
        item.custom_color = clean_custom_color;
        changed = true;
    }

    let clean_icon_data = if item.custom_icon.as_deref() == Some("custom_image") {
        item.icon_data.as_deref().and_then(sanitize_custom_icon_data)
    } else {
        None
    };
    if item.icon_data != clean_icon_data {
        item.icon_data = clean_icon_data;
        changed = true;
    }

    let clean_tile_size = item
        .tile_size
        .as_deref()
        .and_then(normalize_tile_size)
        .map(str::to_string);
    if item.tile_size != clean_tile_size {
        item.tile_size = clean_tile_size;
        changed = true;
    }

    let clean_section = item
        .section
        .as_deref()
        .and_then(normalize_tile_section)
        .map(str::to_string);
    if item.section != clean_section {
        item.section = clean_section;
        changed = true;
    }

    changed
}

pub fn normalize_space_header_widgets(
    header_widgets: &[SpaceHeaderWidget],
    excluded_side_widgets: &[String],
) -> Vec<SpaceHeaderWidget> {
    let mut out = Vec::with_capacity(header_widgets.len().min(SPACE_WIDGET_IDS.len()));
    let mut seen = HashSet::new();
    for entry in header_widgets {
        if let Ok(valid_id) = validate_space_widget_id(&entry.id) {
            if excluded_side_widgets.iter().any(|s| s == valid_id) {
                continue;
            }
            if seen.insert(valid_id) {
                out.push(SpaceHeaderWidget {
                    id: valid_id.to_string(),
                    side: normalize_header_widget_side(&entry.side).to_string(),
                });
            }
        }
    }
    out
}

pub fn validate_and_normalize_space_header_widgets(
    header_widgets: &[SpaceHeaderWidget],
    excluded_side_widgets: &[String],
) -> Result<Vec<SpaceHeaderWidget>, String> {
    let mut out = Vec::with_capacity(header_widgets.len().min(SPACE_WIDGET_IDS.len()));
    let mut seen = HashSet::new();
    for entry in header_widgets {
        let valid_id = validate_space_widget_id(&entry.id)?;
        let valid_side = validate_header_widget_side(&entry.side)?;
        if excluded_side_widgets.iter().any(|s| s == valid_id) {
            continue;
        }
        if seen.insert(valid_id) {
            out.push(SpaceHeaderWidget {
                id: valid_id.to_string(),
                side: valid_side.to_string(),
            });
        }
    }
    Ok(out)
}

pub fn default_space_header_widgets(side_widgets: &[String]) -> Vec<SpaceHeaderWidget> {
    let mut out = Vec::with_capacity(2);
    for id in ["batteries", "weather"] {
        if !side_widgets.iter().any(|w| w == id) {
            out.push(SpaceHeaderWidget {
                id: id.to_string(),
                side: HEADER_WIDGET_SIDE_LEFT.to_string(),
            });
        }
    }
    out
}

pub fn sync_legacy_taskbar_booleans_from_left_widgets(settings: &mut TaskbarWidgetSettings) -> bool {
    let has_weather = settings.left_widgets.iter().any(|w| w == "weather");
    let has_batteries = settings.left_widgets.iter().any(|w| w == "batteries");
    let has_telemetry = settings.left_widgets.iter().any(|w| w == "telemetry");
    let mut changed = false;
    if settings.show_weather != has_weather {
        settings.show_weather = has_weather;
        changed = true;
    }
    if settings.show_batteries != has_batteries {
        settings.show_batteries = has_batteries;
        changed = true;
    }
    if settings.show_telemetry != has_telemetry {
        settings.show_telemetry = has_telemetry;
        changed = true;
    }
    changed
}

pub fn apply_taskbar_widget_boolean_to_left_widgets(
    left_widgets: &mut Vec<String>,
    id: &str,
    enabled: bool,
) -> bool {
    if enabled {
        if left_widgets.iter().any(|w| w == id) {
            false
        } else {
            left_widgets.push(id.to_string());
            true
        }
    } else {
        let before = left_widgets.len();
        left_widgets.retain(|w| w != id);
        left_widgets.len() != before
    }
}

pub fn normalize_quick_notes(notes: &mut QuickNotesState) -> bool {
    let mut changed = false;
    if notes.text.chars().count() > MAX_QUICK_NOTES_CHARS {
        notes.text = notes.text.chars().take(MAX_QUICK_NOTES_CHARS).collect();
        changed = true;
    }
    let mut clean_todos = Vec::with_capacity(notes.todos.len().min(MAX_QUICK_TODOS));
    let mut seen_ids = HashSet::new();
    for (idx, item) in notes.todos.iter().enumerate() {
        if clean_todos.len() >= MAX_QUICK_TODOS {
            changed = true;
            break;
        }
        let text: String = item.text.trim().chars().take(MAX_QUICK_TODO_CHARS).collect();
        if text.is_empty() {
            changed = true;
            continue;
        }
        let base_id = if item.id.trim().is_empty() {
            format!("todo-{}", idx + 1)
        } else {
            item.id.trim().to_string()
        };
        let unique_id = ensure_unique_space_id(&base_id, &mut seen_ids);
        if unique_id != item.id || text != item.text {
            changed = true;
        }
        clean_todos.push(QuickTodoItem {
            id: unique_id,
            text,
            done: item.done,
        });
    }
    if clean_todos != notes.todos {
        notes.todos = clean_todos;
        changed = true;
    }
    changed
}

pub fn normalize_taskbar_widgets(settings: &mut TaskbarWidgetSettings) -> bool {
    let mut changed = false;

    let clean_city = settings.weather_city.trim().to_string();
    if settings.weather_city != clean_city {
        settings.weather_city = clean_city;
        changed = true;
    }

    let clean_units = normalize_weather_units(&settings.weather_units);
    if settings.weather_units != clean_units {
        settings.weather_units = clean_units.to_string();
        changed = true;
    }

    let clean_pos = normalize_taskbar_widgets_position(&settings.widgets_position);
    if settings.widgets_position != clean_pos {
        settings.widgets_position = clean_pos.to_string();
        changed = true;
    }

    let clean_left = normalize_taskbar_widget_list(&settings.left_widgets);
    if settings.left_widgets != clean_left {
        settings.left_widgets = clean_left;
        changed = true;
    }

    let coords_valid = match (settings.weather_lat, settings.weather_lon) {
        (Some(lat), Some(lon)) => {
            lat.is_finite()
                && lon.is_finite()
                && (-90.0..=90.0).contains(&lat)
                && (-180.0..=180.0).contains(&lon)
        }
        (None, None) => true,
        _ => false,
    };
    if !coords_valid {
        settings.weather_lat = None;
        settings.weather_lon = None;
        changed = true;
    }

    changed
}

fn ensure_unique_space_id(base: &str, seen: &mut HashSet<String>) -> String {
    let trimmed = base.trim();
    let root = if trimmed.is_empty() { "space" } else { trimmed };
    if seen.insert(root.to_string()) {
        return root.to_string();
    }
    let mut suffix = 2usize;
    loop {
        let candidate = format!("{root}-{suffix}");
        if seen.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn normalize_space_config(
    space: &mut SpaceConfig,
    seen_ids: &mut HashSet<String>,
    idx: usize,
) -> bool {
    let mut changed = false;

    let fallback_id = format!("space-{}", idx + 1);
    let desired_base = if space.id.trim().is_empty() {
        fallback_id.as_str()
    } else {
        space.id.trim()
    };
    let unique_id = ensure_unique_space_id(desired_base, seen_ids);
    if space.id != unique_id {
        space.id = unique_id;
        changed = true;
    }

    let trimmed_name = space.name.trim();
    let desired_name = if trimmed_name.is_empty() {
        "Пространство".to_string()
    } else {
        trimmed_name.to_string()
    };
    if space.name != desired_name {
        space.name = desired_name;
        changed = true;
    }

    let (hex, rgb) =
        sanitize_accent_hex_rgb(Some(&space.accent_hex), Some(&space.accent_rgb));
    if space.accent_hex != hex {
        space.accent_hex = hex;
        changed = true;
    }
    if space.accent_rgb != rgb {
        space.accent_rgb = rgb;
        changed = true;
    }

    let desired_layout = normalize_space_layout_kind(&space.layout_kind);
    if space.layout_kind != desired_layout {
        space.layout_kind = desired_layout.to_string();
        changed = true;
    }

    let desired_widgets = normalize_space_widgets(&space.widgets);
    if space.widgets != desired_widgets {
        space.widgets = desired_widgets;
        changed = true;
    }

    let desired_header_widgets = normalize_space_header_widgets(&space.header_widgets, &space.widgets);
    if space.header_widgets != desired_header_widgets {
        space.header_widgets = desired_header_widgets;
        changed = true;
    }

    let clean_bottom_title = sanitize_optional_nonempty(&space.bottom_section_title);
    if space.bottom_section_title != clean_bottom_title {
        space.bottom_section_title = clean_bottom_title;
        changed = true;
    }

    for item in space.items.iter_mut() {
        if normalize_pin(item, false) {
            changed = true;
        }
    }

    changed
}

fn default_bottom_section_title(space_id: &str, layout_kind: &str) -> &'static str {
    match space_id {
        "personal" => "Быстрый доступ",
        "work" => "Приложения",
        "studio" => "Инструменты",
        "games" => "Лаунчеры и закреплённое",
        _ if layout_kind == SPACE_LAYOUT_GAMES => "Лаунчеры и закреплённое",
        _ => "Закреплённое",
    }
}

fn build_migrated_spaces_from_legacy(cfg: &TestBarConfig) -> Vec<SpaceConfig> {
    let mut spaces = Vec::with_capacity(4 + cfg.custom_spaces.len());

    let mut personal_items = Vec::with_capacity(cfg.personal_pins.len() + cfg.personal_links.len());
    for (idx, mut item) in cfg.personal_pins.iter().cloned().enumerate() {
        if item.section.is_none() {
            item.section = Some(TILE_SECTION_MAIN.to_string());
        }
        if idx < 2 && item.tile_size.is_none() {
            item.tile_size = Some(TILE_SIZE_HERO.to_string());
        }
        personal_items.push(item);
    }
    for mut item in cfg.personal_links.iter().cloned() {
        if item.section.is_none() {
            item.section = Some(TILE_SECTION_BOTTOM.to_string());
        }
        personal_items.push(item);
    }
    spaces.push(SpaceConfig {
        id: "personal".to_string(),
        name: "Личное".to_string(),
        accent_hex: "#818CF8".to_string(),
        accent_rgb: "129,140,248".to_string(),
        layout_kind: SPACE_LAYOUT_GRID.to_string(),
        show_hero_row: true,
        bottom_section_title: Some("Быстрый доступ".to_string()),
        widgets: vec![
            "media".to_string(),
            "audio_mixer".to_string(),
            "clipboard".to_string(),
        ],
        header_widgets: default_space_header_widgets(&[
            "media".to_string(),
            "audio_mixer".to_string(),
            "clipboard".to_string(),
        ]),
        items: personal_items,
    });

    let mut work_items = Vec::with_capacity(cfg.work_folders.len() + cfg.work_apps.len());
    for mut item in cfg.work_folders.iter().cloned() {
        if item.section.is_none() {
            item.section = Some(TILE_SECTION_MAIN.to_string());
        }
        work_items.push(item);
    }
    for mut item in cfg.work_apps.iter().cloned() {
        if item.section.is_none() {
            item.section = Some(TILE_SECTION_BOTTOM.to_string());
        }
        work_items.push(item);
    }
    spaces.push(SpaceConfig {
        id: "work".to_string(),
        name: "Работа".to_string(),
        accent_hex: "#38BDF8".to_string(),
        accent_rgb: "56,189,248".to_string(),
        layout_kind: SPACE_LAYOUT_GRID.to_string(),
        show_hero_row: false,
        bottom_section_title: Some("Приложения".to_string()),
        widgets: vec!["pc_health".to_string(), "top_processes".to_string()],
        header_widgets: default_space_header_widgets(&[
            "pc_health".to_string(),
            "top_processes".to_string(),
        ]),
        items: work_items,
    });

    let mut studio_items = Vec::with_capacity(cfg.studio_pins.len() + cfg.studio_tools.len());
    for (idx, mut item) in cfg.studio_pins.iter().cloned().enumerate() {
        if item.section.is_none() {
            item.section = Some(TILE_SECTION_MAIN.to_string());
        }
        if idx < 2 && item.tile_size.is_none() {
            item.tile_size = Some(TILE_SIZE_HERO.to_string());
        }
        studio_items.push(item);
    }
    for mut item in cfg.studio_tools.iter().cloned() {
        if item.section.is_none() {
            item.section = Some(TILE_SECTION_BOTTOM.to_string());
        }
        studio_items.push(item);
    }
    spaces.push(SpaceConfig {
        id: "studio".to_string(),
        name: "Студия".to_string(),
        accent_hex: "#34D399".to_string(),
        accent_rgb: "52,211,153".to_string(),
        layout_kind: SPACE_LAYOUT_GRID.to_string(),
        show_hero_row: true,
        bottom_section_title: Some("Инструменты".to_string()),
        widgets: vec![
            "pc_health".to_string(),
            "audio_mixer".to_string(),
            "top_processes".to_string(),
        ],
        header_widgets: default_space_header_widgets(&[
            "pc_health".to_string(),
            "audio_mixer".to_string(),
            "top_processes".to_string(),
        ]),
        items: studio_items,
    });

    let mut game_items = Vec::with_capacity(cfg.game_launchers.len());
    for mut item in cfg.game_launchers.iter().cloned() {
        if item.section.is_none() {
            item.section = Some(TILE_SECTION_BOTTOM.to_string());
        }
        game_items.push(item);
    }
    spaces.push(SpaceConfig {
        id: "games".to_string(),
        name: "Игры".to_string(),
        accent_hex: "#FB7185".to_string(),
        accent_rgb: "251,113,133".to_string(),
        layout_kind: SPACE_LAYOUT_GAMES.to_string(),
        show_hero_row: false,
        bottom_section_title: Some("Лаунчеры и закреплённое".to_string()),
        widgets: vec![
            "audio_mixer".to_string(),
            "game_combos".to_string(),
            "pc_health".to_string(),
        ],
        header_widgets: default_space_header_widgets(&[
            "audio_mixer".to_string(),
            "game_combos".to_string(),
            "pc_health".to_string(),
        ]),
        items: game_items,
    });

    for cs in &cfg.custom_spaces {
        let (hex, rgb) = sanitize_accent_hex_rgb(Some(&cs.accent_hex), Some(&cs.accent_rgb));
        let mut custom_items = Vec::with_capacity(cs.items.len());
        for mut item in cs.items.iter().cloned() {
            if item.section.is_none() {
                item.section = Some(TILE_SECTION_MAIN.to_string());
            }
            custom_items.push(item);
        }
        let custom_widgets = widget_preset_to_widgets(&cs.widget_preset);
        spaces.push(SpaceConfig {
            id: cs.id.clone(),
            name: cs.name.clone(),
            accent_hex: hex,
            accent_rgb: rgb,
            layout_kind: SPACE_LAYOUT_GRID.to_string(),
            show_hero_row: false,
            bottom_section_title: Some("Закреплённое".to_string()),
            header_widgets: default_space_header_widgets(&custom_widgets),
            widgets: custom_widgets,
            items: custom_items,
        });
    }

    spaces
}

fn upgrade_v2_spaces_to_v3(cfg: &mut TestBarConfig) -> bool {
    let mut changed = false;
    let personal_bottom_ids: HashSet<String> =
        cfg.personal_links.iter().map(|i| i.id.clone()).collect();
    let work_bottom_ids: HashSet<String> = cfg.work_apps.iter().map(|i| i.id.clone()).collect();
    let studio_bottom_ids: HashSet<String> =
        cfg.studio_tools.iter().map(|i| i.id.clone()).collect();

    for sp in &mut cfg.spaces {
        if sp.bottom_section_title.is_none() {
            sp.bottom_section_title =
                Some(default_bottom_section_title(&sp.id, &sp.layout_kind).to_string());
            changed = true;
        }

        if sp.items.iter().any(|i| i.section.is_some()) {
            continue;
        }

        match sp.id.as_str() {
            "personal" => {
                let trailing_folder_start = if personal_bottom_ids.is_empty() {
                    let has_non_folder = sp.items.iter().any(|i| i.kind != "folder");
                    let has_folder = sp.items.iter().any(|i| i.kind == "folder");
                    if has_non_folder && has_folder {
                        sp.items
                            .iter()
                            .rposition(|i| i.kind != "folder")
                            .map(|pos| pos + 1)
                            .filter(|&start| start < sp.items.len())
                    } else {
                        None
                    }
                } else {
                    None
                };

                let mut main_count = 0usize;
                for (idx, item) in sp.items.iter_mut().enumerate() {
                    let is_bottom = if !personal_bottom_ids.is_empty() {
                        personal_bottom_ids.contains(&item.id)
                    } else {
                        trailing_folder_start.is_some_and(|start| idx >= start)
                    };
                    if is_bottom {
                        item.section = Some(TILE_SECTION_BOTTOM.to_string());
                    } else {
                        item.section = Some(TILE_SECTION_MAIN.to_string());
                        if main_count < 2 && item.tile_size.is_none() {
                            item.tile_size = Some(TILE_SIZE_HERO.to_string());
                        }
                        main_count += 1;
                    }
                }
                if !sp.items.is_empty() {
                    changed = true;
                }
            }
            "work" => {
                for item in &mut sp.items {
                    let is_bottom = if !work_bottom_ids.is_empty() {
                        work_bottom_ids.contains(&item.id)
                    } else {
                        item.kind == "app"
                    };
                    item.section = Some(
                        if is_bottom {
                            TILE_SECTION_BOTTOM
                        } else {
                            TILE_SECTION_MAIN
                        }
                        .to_string(),
                    );
                }
                if !sp.items.is_empty() {
                    changed = true;
                }
            }
            "studio" => {
                let mut main_count = 0usize;
                for item in &mut sp.items {
                    if !studio_bottom_ids.is_empty() && studio_bottom_ids.contains(&item.id) {
                        item.section = Some(TILE_SECTION_BOTTOM.to_string());
                    } else {
                        item.section = Some(TILE_SECTION_MAIN.to_string());
                        if main_count < 2 && item.tile_size.is_none() {
                            item.tile_size = Some(TILE_SIZE_HERO.to_string());
                        }
                        main_count += 1;
                    }
                }
                if !sp.items.is_empty() {
                    changed = true;
                }
            }
            id if id == "games" || sp.layout_kind == SPACE_LAYOUT_GAMES => {
                for item in &mut sp.items {
                    item.section = Some(TILE_SECTION_BOTTOM.to_string());
                }
                if !sp.items.is_empty() {
                    changed = true;
                }
            }
            _ => {
                let mut main_count = 0usize;
                for item in &mut sp.items {
                    item.section = Some(TILE_SECTION_MAIN.to_string());
                    if sp.show_hero_row && main_count < 2 && item.tile_size.is_none() {
                        item.tile_size = Some(TILE_SIZE_HERO.to_string());
                    }
                    main_count += 1;
                }
                if !sp.items.is_empty() {
                    changed = true;
                }
            }
        }
    }

    changed
}

/// Losslessly upgrades legacy v0/v1/v2 config payloads to schema v3 and normalizes all pin lists & spaces.
/// Returns `true` if any field was upgraded or normalized.
pub fn normalize_and_migrate(cfg: &mut TestBarConfig) -> bool {
    let mut changed = false;
    let prev_schema = cfg.schema_version;

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

    let desired_theme = normalize_theme_preset(&cfg.theme_preset);
    if cfg.theme_preset != desired_theme {
        cfg.theme_preset = desired_theme.to_string();
        changed = true;
    }

    let desired_accent_mode = normalize_accent_mode(&cfg.accent_mode);
    if cfg.accent_mode != desired_accent_mode {
        cfg.accent_mode = desired_accent_mode.to_string();
        changed = true;
    }

    let (clean_acc_hex, clean_acc_rgb) =
        sanitize_accent_hex_rgb(Some(&cfg.accent_hex), Some(&cfg.accent_rgb));
    if cfg.accent_hex != clean_acc_hex {
        cfg.accent_hex = clean_acc_hex;
        changed = true;
    }
    if cfg.accent_rgb != clean_acc_rgb {
        cfg.accent_rgb = clean_acc_rgb;
        changed = true;
    }

    let desired_date_fmt = normalize_clock_date_format(&cfg.clock_date_format);
    if cfg.clock_date_format != desired_date_fmt {
        cfg.clock_date_format = desired_date_fmt.to_string();
        changed = true;
    }

    let desired_clock_layout = normalize_clock_layout(&cfg.clock_layout);
    if cfg.clock_layout != desired_clock_layout {
        cfg.clock_layout = desired_clock_layout.to_string();
        changed = true;
    }

    let desired_default_tile = normalize_tile_size(&cfg.default_tile_size).unwrap_or(TILE_SIZE_COMPACT);
    if cfg.default_tile_size != desired_default_tile {
        cfg.default_tile_size = desired_default_tile.to_string();
        changed = true;
    }

    if normalize_quick_notes(&mut cfg.quick_notes) {
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

    if cfg.spaces.is_empty() {
        let has_legacy_spaces = !cfg.personal_pins.is_empty()
            || !cfg.personal_links.is_empty()
            || !cfg.work_folders.is_empty()
            || !cfg.work_apps.is_empty()
            || !cfg.studio_pins.is_empty()
            || !cfg.studio_tools.is_empty()
            || !cfg.game_launchers.is_empty()
            || !cfg.custom_spaces.is_empty();
        cfg.spaces = if has_legacy_spaces {
            build_migrated_spaces_from_legacy(cfg)
        } else {
            default_modular_spaces()
        };
        changed = true;
    } else if prev_schema < 3 && upgrade_v2_spaces_to_v3(cfg) {
        changed = true;
    }

    if cfg.spaces.is_empty() {
        cfg.spaces = default_modular_spaces();
        changed = true;
    }

    if prev_schema < 4 {
        for space in &mut cfg.spaces {
            if space.header_widgets.is_empty() {
                let seeded = default_space_header_widgets(&space.widgets);
                if !seeded.is_empty() {
                    space.header_widgets = seeded;
                    changed = true;
                }
            }
        }
    }

    let mut seen_space_ids = HashSet::new();
    for (idx, space) in cfg.spaces.iter_mut().enumerate() {
        if normalize_space_config(space, &mut seen_space_ids, idx) {
            changed = true;
        }
    }

    if prev_schema < 3 && cfg.taskbar_widgets.show_weather {
        cfg.taskbar_widgets.show_weather = false;
        changed = true;
    }

    if prev_schema < 4 && cfg.taskbar_widgets.left_widgets.is_empty() {
        if cfg.taskbar_widgets.show_weather {
            cfg.taskbar_widgets.left_widgets.push("weather".to_string());
        }
        if cfg.taskbar_widgets.show_batteries {
            cfg.taskbar_widgets
                .left_widgets
                .push("batteries".to_string());
        }
        if cfg.taskbar_widgets.show_telemetry {
            cfg.taskbar_widgets
                .left_widgets
                .push("telemetry".to_string());
        }
        if !cfg.taskbar_widgets.left_widgets.is_empty() {
            changed = true;
        }
    }

    if normalize_taskbar_widgets(&mut cfg.taskbar_widgets) {
        changed = true;
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

    // Deduplicate and normalize hidden_battery_ids (case-insensitive)
    let mut seen_bats = HashSet::new();
    let mut cleaned_bats = Vec::with_capacity(cfg.hidden_battery_ids.len());
    for entry in &cfg.hidden_battery_ids {
        let norm = entry.trim().to_ascii_lowercase();
        if !norm.is_empty() && seen_bats.insert(norm.clone()) {
            cleaned_bats.push(norm);
        }
    }
    if cleaned_bats != cfg.hidden_battery_ids {
        cfg.hidden_battery_ids = cleaned_bats;
        changed = true;
    }

    if trim_frecency_map(&mut cfg.frecency) {
        changed = true;
    }

    let desired_mac_cmd = normalize_mac_command_key(&cfg.mac_command_key);
    if cfg.mac_command_key != desired_mac_cmd {
        cfg.mac_command_key = desired_mac_cmd.to_string();
        changed = true;
    }
    let desired_mac_pos = normalize_mac_window_position(&cfg.mac_window_position);
    if cfg.mac_window_position != desired_mac_pos {
        cfg.mac_window_position = desired_mac_pos.to_string();
        changed = true;
    }
    let desired_mac_term = normalize_mac_terminal_app(&cfg.mac_terminal_app);
    if cfg.mac_terminal_app != desired_mac_term {
        cfg.mac_terminal_app = desired_mac_term.to_string();
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

#[cfg(target_os = "windows")]
fn default_modular_spaces() -> Vec<SpaceConfig> {
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

    let browser_pin_personal = detect_default_browser().map(|b| {
        pin(
            "p-browser",
            "app",
            &b.title,
            "Браузер по умолчанию",
            b.target,
            "Новое окно",
            &b.accent_rgb,
            &b.icon_key,
        )
    });

    let mut personal_items = Vec::new();
    if let Some(browser) = browser_pin_personal {
        personal_items.push(browser);
    }
    personal_items.extend([
        pin("p-explorer", "app", "Проводник", "Файлы и диски", detect_app_target("explorer"), "Новое окно", "245,158,11", "explorer"),
        pin("p-telegram", "app", "Telegram", "Мессенджер", detect_app_target("telegram"), "", "56,189,248", "telegram"),
        pin("p-discord", "app", "Discord", "Голос и чаты", detect_app_target("discord"), "", "99,102,241", "discord"),
        pin("p-spotify", "app", "Spotify", "Музыка", detect_app_target("spotify"), "", "52,211,153", "spotify"),
        pin("p-vlc", "app", "VLC", "Медиаплеер", detect_app_target("vlc"), "", "249,115,22", "vlc"),
        pin("p-downloads", "folder", "Загрузки", "Папка", downloads.clone(), "Скопировать путь", "56,189,248", "folder"),
        pin("p-docs", "folder", "Документы", "Папка", documents.clone(), "Скопировать путь", "245,158,11", "folder"),
        pin("p-pics", "folder", "Изображения", "Скриншоты", pictures.clone(), "Скопировать путь", "139,92,246", "image"),
        pin("p-desktop", "folder", "Рабочий стол", "Файлы", desktop.clone(), "Скопировать путь", "52,211,153", "folder"),
    ]);
    let mut personal_items = retain_resolved_pins(personal_items);
    let mut personal_main_idx = 0usize;
    for item in &mut personal_items {
        if item.kind == "folder" {
            item.section = Some(TILE_SECTION_BOTTOM.to_string());
        } else {
            item.section = Some(TILE_SECTION_MAIN.to_string());
            if personal_main_idx < 2 {
                item.tile_size = Some(TILE_SIZE_HERO.to_string());
            }
            personal_main_idx += 1;
        }
    }

    let mut work_items = retain_resolved_pins(vec![
        pin("w-dl", "folder", "Загрузки", "Свежие файлы", downloads, "Открыть в терминале", "245,158,11", "folder"),
        pin("w-docs", "folder", "Документы", "Основная папка", documents, "Открыть в терминале", "245,158,11", "folder"),
        pin("w-desktop", "folder", "Рабочий стол", "Файлы и ярлыки", desktop, "Открыть в терминале", "56,189,248", "folder"),
        pin("w-pics", "folder", "Изображения", "Скриншоты", pictures, "Открыть в терминале", "139,92,246", "folder"),
        pin("w-vscode", "app", "VS Code", "Редактор", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
        pin("w-wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "terminal"),
        pin("w-notepad", "app", "Блокнот", "Заметки", detect_app_target("notepad"), "", "148,163,184", "notepad"),
        pin("w-taskmgr", "app", "Диспетчер задач", "Процессы", detect_app_target("taskmgr"), "От админа", "129,140,248", "cpu"),
    ]);
    for item in &mut work_items {
        item.section = Some(
            if item.kind == "app" {
                TILE_SECTION_BOTTOM
            } else {
                TILE_SECTION_MAIN
            }
            .to_string(),
        );
    }

    let mut studio_items = retain_resolved_pins(vec![
        pin("s-photoshop", "app", "Photoshop", "Графика и ретушь", detect_app_target("photoshop"), "От админа", "49,168,255", "photoshop"),
        pin("s-figma", "app", "Figma", "Интерфейсы", detect_app_target("figma"), "", "168,85,247", "figma"),
        pin("s-obs", "app", "OBS Studio", "Захват видео", detect_app_target("obs"), "От админа", "148,163,184", "obs"),
        pin("s-vscode", "app", "VS Code", "Редактор", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
        pin("s-snip", "app", "Ножницы", "Скриншот", detect_app_target("snipping"), "", "52,211,153", "scissors"),
        pin("s-wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "terminal"),
        pin("s-taskmgr", "app", "Диспетчер задач", "Процессы", detect_app_target("taskmgr"), "От админа", "129,140,248", "cpu"),
    ]);
    let mut studio_main_idx = 0usize;
    for item in &mut studio_items {
        if matches!(item.id.as_str(), "s-snip" | "s-wt" | "s-taskmgr") {
            item.section = Some(TILE_SECTION_BOTTOM.to_string());
        } else {
            item.section = Some(TILE_SECTION_MAIN.to_string());
            if studio_main_idx < 2 {
                item.tile_size = Some(TILE_SIZE_HERO.to_string());
            }
            studio_main_idx += 1;
        }
    }

    let mut game_items = retain_resolved_pins(vec![
        pin("g-steam", "app", "Steam", "Библиотека", detect_app_target("steam"), "", "56,189,248", "steam"),
        pin("g-discord", "app", "Discord", "Голос", detect_app_target("discord"), "", "99,102,241", "discord"),
        pin("g-obs", "app", "OBS Studio", "Запись", detect_app_target("obs"), "От админа", "148,163,184", "obs"),
    ]);
    for item in &mut game_items {
        item.section = Some(TILE_SECTION_BOTTOM.to_string());
    }

    vec![
        SpaceConfig {
            id: "personal".to_string(),
            name: "Личное".to_string(),
            accent_hex: "#818CF8".to_string(),
            accent_rgb: "129,140,248".to_string(),
            layout_kind: SPACE_LAYOUT_GRID.to_string(),
            show_hero_row: true,
            bottom_section_title: Some("Быстрый доступ".to_string()),
            widgets: vec![
                "media".to_string(),
                "audio_mixer".to_string(),
                "clipboard".to_string(),
            ],
            header_widgets: default_space_header_widgets(&[
                "media".to_string(),
                "audio_mixer".to_string(),
                "clipboard".to_string(),
            ]),
            items: personal_items,
        },
        SpaceConfig {
            id: "work".to_string(),
            name: "Работа".to_string(),
            accent_hex: "#38BDF8".to_string(),
            accent_rgb: "56,189,248".to_string(),
            layout_kind: SPACE_LAYOUT_GRID.to_string(),
            show_hero_row: false,
            bottom_section_title: Some("Приложения".to_string()),
            widgets: vec!["pc_health".to_string(), "top_processes".to_string()],
            header_widgets: default_space_header_widgets(&[
                "pc_health".to_string(),
                "top_processes".to_string(),
            ]),
            items: work_items,
        },
        SpaceConfig {
            id: "studio".to_string(),
            name: "Студия".to_string(),
            accent_hex: "#34D399".to_string(),
            accent_rgb: "52,211,153".to_string(),
            layout_kind: SPACE_LAYOUT_GRID.to_string(),
            show_hero_row: true,
            bottom_section_title: Some("Инструменты".to_string()),
            widgets: vec![
                "pc_health".to_string(),
                "audio_mixer".to_string(),
                "top_processes".to_string(),
            ],
            header_widgets: default_space_header_widgets(&[
                "pc_health".to_string(),
                "audio_mixer".to_string(),
                "top_processes".to_string(),
            ]),
            items: studio_items,
        },
        SpaceConfig {
            id: "games".to_string(),
            name: "Игры".to_string(),
            accent_hex: "#FB7185".to_string(),
            accent_rgb: "251,113,133".to_string(),
            layout_kind: SPACE_LAYOUT_GAMES.to_string(),
            show_hero_row: false,
            bottom_section_title: Some("Лаунчеры и закреплённое".to_string()),
            widgets: vec![
                "audio_mixer".to_string(),
                "game_combos".to_string(),
                "pc_health".to_string(),
            ],
            header_widgets: default_space_header_widgets(&[
                "audio_mixer".to_string(),
                "game_combos".to_string(),
                "pc_health".to_string(),
            ]),
            items: game_items,
        },
    ]
}

#[cfg(target_os = "macos")]
fn detect_mac_app_bundle(names: &[&str]) -> String {
    let home = user_home();
    let roots = [
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
        PathBuf::from("/System/Library/CoreServices"),
        home.join("Applications"),
    ];
    let mut candidates = Vec::new();
    for name in names {
        let p = Path::new(name);
        if p.is_absolute() {
            candidates.push(p.to_path_buf());
        } else {
            for root in &roots {
                candidates.push(root.join(name));
            }
        }
    }
    find_first_existing(&candidates).unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn detect_mac_browser_pin() -> Option<PinItem> {
    let candidates: &[(&str, &str, &str, &str)] = &[
        ("Arc.app", "Arc", "129,140,248", "browser"),
        ("Google Chrome.app", "Google Chrome", "56,189,248", "chrome"),
        ("Brave Browser.app", "Brave", "249,115,22", "brave"),
        ("Firefox.app", "Firefox", "249,115,22", "firefox"),
        ("Safari.app", "Safari", "56,189,248", "browser"),
    ];
    for &(bundle, title, rgb, icon_key) in candidates {
        let target = detect_mac_app_bundle(&[bundle]);
        if !target.is_empty() {
            return Some(pin(
                "p-browser",
                "app",
                title,
                "Браузер",
                target,
                "Новое окно",
                rgb,
                icon_key,
            ));
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn default_modular_spaces() -> Vec<SpaceConfig> {
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

    let finder_target = detect_mac_app_bundle(&["Finder.app"]);
    let terminal_target =
        detect_mac_app_bundle(&["Warp.app", "Ghostty.app", "iTerm.app", "Terminal.app"]);
    let vscode_target = detect_mac_app_bundle(&["Visual Studio Code.app"]);
    let cursor_target = detect_mac_app_bundle(&["Cursor.app"]);
    let telegram_target = detect_mac_app_bundle(&["Telegram.app"]);
    let discord_target = detect_mac_app_bundle(&["Discord.app"]);
    let obsidian_target = detect_mac_app_bundle(&["Obsidian.app"]);
    let figma_target = detect_mac_app_bundle(&["Figma.app"]);
    let activity_target = detect_mac_app_bundle(&["Activity Monitor.app"]);
    let settings_target =
        detect_mac_app_bundle(&["System Settings.app", "System Preferences.app"]);

    let mut personal_items = Vec::new();
    if let Some(browser) = detect_mac_browser_pin() {
        personal_items.push(browser);
    }
    personal_items.extend([
        pin(
            "p-finder",
            "app",
            "Finder",
            "Файлы и диски",
            finder_target,
            "Новое окно",
            "56,189,248",
            "explorer",
        ),
        pin(
            "p-terminal",
            "app",
            "Терминал",
            "Консоль",
            terminal_target.clone(),
            "Новое окно",
            "52,211,153",
            "terminal",
        ),
        pin(
            "p-vscode",
            "app",
            "VS Code",
            "Редактор",
            vscode_target.clone(),
            "Новое окно",
            "56,189,248",
            "vscode",
        ),
        pin(
            "p-cursor",
            "app",
            "Cursor",
            "AI-редактор",
            cursor_target.clone(),
            "",
            "129,140,248",
            "cursor",
        ),
        pin(
            "p-telegram",
            "app",
            "Telegram",
            "Мессенджер",
            telegram_target,
            "",
            "56,189,248",
            "telegram",
        ),
        pin(
            "p-discord",
            "app",
            "Discord",
            "Голос и чаты",
            discord_target,
            "",
            "99,102,241",
            "discord",
        ),
        pin(
            "p-obsidian",
            "app",
            "Obsidian",
            "Заметки",
            obsidian_target,
            "",
            "167,139,250",
            "obsidian",
        ),
        pin(
            "p-desktop",
            "folder",
            "Рабочий стол",
            "Файлы",
            desktop.clone(),
            "Скопировать путь",
            "52,211,153",
            "folder",
        ),
        pin(
            "p-downloads",
            "folder",
            "Загрузки",
            "Папка",
            downloads.clone(),
            "Скопировать путь",
            "56,189,248",
            "folder",
        ),
        pin(
            "p-docs",
            "folder",
            "Документы",
            "Папка",
            documents.clone(),
            "Скопировать путь",
            "245,158,11",
            "folder",
        ),
        pin(
            "p-apps",
            "folder",
            "Программы",
            "Приложения",
            "/Applications".to_string(),
            "Скопировать путь",
            "129,140,248",
            "folder",
        ),
    ]);
    let mut personal_items = retain_resolved_pins(personal_items);
    let mut personal_main_idx = 0usize;
    for item in &mut personal_items {
        if item.kind == "folder" {
            item.section = Some(TILE_SECTION_BOTTOM.to_string());
        } else {
            item.section = Some(TILE_SECTION_MAIN.to_string());
            if personal_main_idx < 2 {
                item.tile_size = Some(TILE_SIZE_HERO.to_string());
            }
            personal_main_idx += 1;
        }
    }

    let mut work_items = retain_resolved_pins(vec![
        pin(
            "w-dl",
            "folder",
            "Загрузки",
            "Свежие файлы",
            downloads,
            "Открыть в терминале",
            "245,158,11",
            "folder",
        ),
        pin(
            "w-docs",
            "folder",
            "Документы",
            "Основная папка",
            documents,
            "Открыть в терминале",
            "245,158,11",
            "folder",
        ),
        pin(
            "w-desktop",
            "folder",
            "Рабочий стол",
            "Файлы и ярлыки",
            desktop,
            "Открыть в терминале",
            "56,189,248",
            "folder",
        ),
        pin(
            "w-vscode",
            "app",
            "VS Code",
            "Редактор",
            vscode_target,
            "Новое окно",
            "56,189,248",
            "vscode",
        ),
        pin(
            "w-cursor",
            "app",
            "Cursor",
            "AI-редактор",
            cursor_target,
            "",
            "129,140,248",
            "cursor",
        ),
        pin(
            "w-terminal",
            "app",
            "Терминал",
            "Консоль",
            terminal_target.clone(),
            "Новое окно",
            "52,211,153",
            "terminal",
        ),
        pin(
            "w-activity",
            "app",
            "Мониторинг системы",
            "Процессы и нагрузка",
            activity_target.clone(),
            "",
            "129,140,248",
            "cpu",
        ),
    ]);
    for item in &mut work_items {
        item.section = Some(
            if item.kind == "app" {
                TILE_SECTION_BOTTOM
            } else {
                TILE_SECTION_MAIN
            }
            .to_string(),
        );
    }

    let mut studio_items = retain_resolved_pins(vec![
        pin(
            "s-figma",
            "app",
            "Figma",
            "Интерфейсы",
            figma_target,
            "",
            "168,85,247",
            "figma",
        ),
        pin(
            "s-settings",
            "app",
            "Системные настройки",
            "Параметры macOS",
            settings_target,
            "",
            "52,211,153",
            "settings",
        ),
        pin(
            "s-terminal",
            "app",
            "Терминал",
            "Консоль",
            terminal_target,
            "Новое окно",
            "52,211,153",
            "terminal",
        ),
        pin(
            "s-activity",
            "app",
            "Мониторинг системы",
            "Процессы и память",
            activity_target,
            "",
            "129,140,248",
            "cpu",
        ),
    ]);
    let mut studio_main_idx = 0usize;
    for item in &mut studio_items {
        if matches!(item.id.as_str(), "s-terminal" | "s-activity") {
            item.section = Some(TILE_SECTION_BOTTOM.to_string());
        } else {
            item.section = Some(TILE_SECTION_MAIN.to_string());
            if studio_main_idx < 2 {
                item.tile_size = Some(TILE_SIZE_HERO.to_string());
            }
            studio_main_idx += 1;
        }
    }

    let personal_widgets = vec!["clipboard".to_string(), "notes".to_string()];
    let work_widgets = vec![
        "pc_health".to_string(),
        "top_processes".to_string(),
        "timer".to_string(),
    ];
    let studio_widgets = vec![
        "quick_toggles".to_string(),
        "weather".to_string(),
        "batteries".to_string(),
    ];

    vec![
        SpaceConfig {
            id: "personal".to_string(),
            name: "Личное".to_string(),
            accent_hex: "#818CF8".to_string(),
            accent_rgb: "129,140,248".to_string(),
            layout_kind: SPACE_LAYOUT_GRID.to_string(),
            show_hero_row: true,
            bottom_section_title: Some("Быстрый доступ".to_string()),
            header_widgets: default_space_header_widgets(&personal_widgets),
            widgets: personal_widgets,
            items: personal_items,
        },
        SpaceConfig {
            id: "work".to_string(),
            name: "Работа".to_string(),
            accent_hex: "#38BDF8".to_string(),
            accent_rgb: "56,189,248".to_string(),
            layout_kind: SPACE_LAYOUT_GRID.to_string(),
            show_hero_row: false,
            bottom_section_title: Some("Приложения".to_string()),
            widgets: work_widgets,
            header_widgets: vec![SpaceHeaderWidget {
                id: "network".to_string(),
                side: HEADER_WIDGET_SIDE_LEFT.to_string(),
            }],
            items: work_items,
        },
        SpaceConfig {
            id: "studio".to_string(),
            name: "Утилиты".to_string(),
            accent_hex: "#34D399".to_string(),
            accent_rgb: "52,211,153".to_string(),
            layout_kind: SPACE_LAYOUT_GRID.to_string(),
            show_hero_row: true,
            bottom_section_title: Some("Инструменты".to_string()),
            widgets: studio_widgets,
            header_widgets: vec![],
            items: studio_items,
        },
    ]
}

#[cfg(target_os = "windows")]
fn default_taskbar_pins_for_platform() -> Vec<PinItem> {
    let mut taskbar = vec![pin(
        "tb-explorer",
        "app",
        "Проводник",
        "Файлы и диски",
        detect_app_target("explorer"),
        "Новое окно",
        "245,158,11",
        "explorer",
    )];
    if let Some(browser) = detect_default_browser() {
        taskbar.push(pin(
            "tb-browser",
            "app",
            &browser.title,
            "Браузер",
            browser.target,
            "Новое окно",
            &browser.accent_rgb,
            &browser.icon_key,
        ));
    }
    taskbar.extend([
        pin("tb-wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "terminal"),
        pin("tb-vscode", "app", "VS Code", "Редактор", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
        pin("tb-telegram", "app", "Telegram", "Мессенджер", detect_app_target("telegram"), "", "56,189,248", "telegram"),
        pin("tb-discord", "app", "Discord", "Голос и чаты", detect_app_target("discord"), "", "99,102,241", "discord"),
        pin("tb-spotify", "app", "Spotify", "Музыка", detect_app_target("spotify"), "", "52,211,153", "spotify"),
    ]);
    retain_resolved_pins(taskbar)
}

#[cfg(not(target_os = "windows"))]
fn default_taskbar_pins_for_platform() -> Vec<PinItem> {
    Vec::new()
}

impl Default for TestBarConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: 0,
            bar_mode: "dock".to_string(),
            secondary_taskbar_mode: default_secondary_taskbar_mode(),
            autostart: false,
            glass_depth: default_glass_depth(),
            theme_preset: default_theme_preset(),
            accent_mode: default_accent_mode(),
            accent_hex: default_global_accent_hex(),
            accent_rgb: default_global_accent_rgb(),
            surface_gradients: true,
            hover_glow: true,
            clock_show_seconds: false,
            clock_show_weekday: false,
            clock_date_format: default_clock_date_format(),
            clock_layout: default_clock_layout(),
            start_remember_space: false,
            default_tile_size: default_tile_size(),
            quick_notes: QuickNotesState::default(),
            hidden_tray_keys: vec![],
            hidden_battery_ids: vec![],
            frecency: HashMap::new(),
            taskbar_pins: default_taskbar_pins_for_platform(),
            spaces: default_modular_spaces(),
            taskbar_widgets: TaskbarWidgetSettings::default(),
            personal_pins: vec![],
            personal_links: vec![],
            work_folders: vec![],
            work_apps: vec![],
            studio_pins: vec![],
            studio_tools: vec![],
            game_launchers: vec![],
            game_combos: vec![],
            custom_spaces: vec![],
            pinned_clipboard: vec![],
            mac_command_key: default_mac_command_key(),
            mac_window_position: default_mac_window_position(),
            mac_terminal_app: default_mac_terminal_app(),
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

fn legacy_parent_space_id(space: &str) -> Option<&'static str> {
    match space {
        "personal_pins" | "personal_links" => Some("personal"),
        "work_folders" | "work_apps" => Some("work"),
        "studio_pins" | "studio_tools" => Some("studio"),
        "game_launchers" => Some("games"),
        _ => None,
    }
}

pub fn add_pin_to_space(cfg: &mut TestBarConfig, space: &str, mut item: PinItem) -> bool {
    let force_combo = should_force_combo(space, &item.kind);
    normalize_pin(&mut item, force_combo);

    match space {
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
        "game_combos" => {
            cfg.game_combos.push(item);
            true
        }
        "games" if item.kind == "combo" => {
            cfg.game_combos.push(item);
            true
        }
        "personal_pins" => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "personal") {
                sp.items.push(item.clone());
            }
            cfg.personal_pins.push(item);
            true
        }
        "personal_links" => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "personal") {
                sp.items.push(item.clone());
            }
            cfg.personal_links.push(item);
            true
        }
        "work_folders" => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "work") {
                sp.items.push(item.clone());
            }
            cfg.work_folders.push(item);
            true
        }
        "work_apps" => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "work") {
                sp.items.push(item.clone());
            }
            cfg.work_apps.push(item);
            true
        }
        "studio_pins" => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "studio") {
                sp.items.push(item.clone());
            }
            cfg.studio_pins.push(item);
            true
        }
        "studio_tools" => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "studio") {
                sp.items.push(item.clone());
            }
            cfg.studio_tools.push(item);
            true
        }
        "game_launchers" => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "games") {
                sp.items.push(item.clone());
            }
            cfg.game_launchers.push(item);
            true
        }
        space_id => {
            let mut added = false;
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == space_id) {
                sp.items.push(item.clone());
                added = true;
            }
            if let Some(cs) = cfg.custom_spaces.iter_mut().find(|s| s.id == space_id) {
                cs.items.push(item);
                added = true;
            }
            added
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
        "taskbar" | "taskbar_pins" => remove_by_id(&mut cfg.taskbar_pins, item_id),
        "game_combos" => remove_by_id(&mut cfg.game_combos, item_id),
        "personal_pins" => {
            let mut removed = remove_by_id(&mut cfg.personal_pins, item_id);
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "personal") {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            removed
        }
        "personal_links" => {
            let mut removed = remove_by_id(&mut cfg.personal_links, item_id);
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "personal") {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            removed
        }
        "work_folders" => {
            let mut removed = remove_by_id(&mut cfg.work_folders, item_id);
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "work") {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            removed
        }
        "work_apps" => {
            let mut removed = remove_by_id(&mut cfg.work_apps, item_id);
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "work") {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            removed
        }
        "studio_pins" => {
            let mut removed = remove_by_id(&mut cfg.studio_pins, item_id);
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "studio") {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            removed
        }
        "studio_tools" => {
            let mut removed = remove_by_id(&mut cfg.studio_tools, item_id);
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "studio") {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            removed
        }
        "game_launchers" => {
            let mut removed = remove_by_id(&mut cfg.game_launchers, item_id);
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "games") {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            removed
        }
        "games" => {
            let mut removed = remove_by_id(&mut cfg.game_combos, item_id);
            removed |= remove_by_id(&mut cfg.game_launchers, item_id);
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == "games") {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            removed
        }
        space_id => {
            let mut removed = false;
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == space_id) {
                removed |= remove_by_id(&mut sp.items, item_id);
            }
            if let Some(cs) = cfg.custom_spaces.iter_mut().find(|s| s.id == space_id) {
                removed |= remove_by_id(&mut cs.items, item_id);
            }
            match space_id {
                "personal" => {
                    removed |= remove_by_id(&mut cfg.personal_pins, item_id);
                    removed |= remove_by_id(&mut cfg.personal_links, item_id);
                }
                "work" => {
                    removed |= remove_by_id(&mut cfg.work_folders, item_id);
                    removed |= remove_by_id(&mut cfg.work_apps, item_id);
                }
                "studio" => {
                    removed |= remove_by_id(&mut cfg.studio_pins, item_id);
                    removed |= remove_by_id(&mut cfg.studio_tools, item_id);
                }
                _ => {}
            }
            removed
        }
    }
}

fn reorder_vec(vec: &mut Vec<PinItem>, from_idx: usize, to_idx: usize) -> bool {
    if from_idx < vec.len() && to_idx < vec.len() && from_idx != to_idx {
        let item = vec.remove(from_idx);
        vec.insert(to_idx, item);
        true
    } else {
        false
    }
}

fn resolve_space_pins_mut<'a>(
    cfg: &'a mut TestBarConfig,
    space: &str,
) -> Option<&'a mut Vec<PinItem>> {
    match space {
        "taskbar" | "taskbar_pins" => Some(&mut cfg.taskbar_pins),
        "game_combos" => Some(&mut cfg.game_combos),
        "personal_pins" => Some(&mut cfg.personal_pins),
        "personal_links" => Some(&mut cfg.personal_links),
        "work_folders" => Some(&mut cfg.work_folders),
        "work_apps" => Some(&mut cfg.work_apps),
        "studio_pins" => Some(&mut cfg.studio_pins),
        "studio_tools" => Some(&mut cfg.studio_tools),
        "game_launchers" => Some(&mut cfg.game_launchers),
        space_id => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == space_id) {
                Some(&mut sp.items)
            } else {
                cfg.custom_spaces
                    .iter_mut()
                    .find(|s| s.id == space_id)
                    .map(|cs| &mut cs.items)
            }
        }
    }
}

pub fn reorder_pins_in_space(
    cfg: &mut TestBarConfig,
    space: &str,
    from_idx: usize,
    to_idx: usize,
) -> bool {
    let Some(list) = resolve_space_pins_mut(cfg, space) else {
        return false;
    };
    reorder_vec(list, from_idx, to_idx)
}

pub fn reorder_pins_by_id(
    cfg: &mut TestBarConfig,
    space: &str,
    from_id: &str,
    to_id: Option<&str>,
    target_section: Option<&str>,
) -> bool {
    let normalized_section = match target_section {
        Some(raw) => match normalize_tile_section(raw) {
            Some(valid) => Some(valid),
            None => return false,
        },
        None => None,
    };

    let Some(list) = resolve_space_pins_mut(cfg, space) else {
        return false;
    };

    let Some(from_idx) = list.iter().position(|p| p.id == from_id) else {
        return false;
    };

    if let Some(tid) = to_id {
        let Some(to_idx) = list.iter().position(|p| p.id == tid) else {
            return false;
        };
        let mut item = list.remove(from_idx);
        let mut changed = from_idx != to_idx;
        if let Some(sec) = normalized_section {
            if item.section.as_deref() != Some(sec) {
                item.section = Some(sec.to_string());
                changed = true;
            }
        }
        list.insert(to_idx, item);
        return changed;
    }

    let before = list.clone();
    let mut item = list.remove(from_idx);
    if let Some(sec) = normalized_section {
        item.section = Some(sec.to_string());
    }
    let insert_idx = match normalized_section {
        Some(TILE_SECTION_MAIN) => list
            .iter()
            .rposition(|p| p.section.as_deref() != Some(TILE_SECTION_BOTTOM))
            .map(|pos| pos + 1)
            .unwrap_or(0),
        Some(TILE_SECTION_BOTTOM) => list
            .iter()
            .rposition(|p| p.section.as_deref() == Some(TILE_SECTION_BOTTOM))
            .map(|pos| pos + 1)
            .unwrap_or(list.len()),
        _ => list.len(),
    };
    list.insert(insert_idx, item);
    *list != before
}

fn transliterate_cyrillic_char(ch: char) -> &'static str {
    match ch {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' | 'ё' => "e",
        'ж' => "zh",
        'з' => "z",
        'и' | 'й' => "i",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "h",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "sch",
        'ъ' | 'ь' => "",
        'ы' => "y",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        _ => "",
    }
}

fn slugify_space_id(name: &str, cfg: &TestBarConfig) -> String {
    let mut slug = String::new();
    let mut prev_dash = true;
    for ch in name.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            prev_dash = false;
        } else {
            let tr = transliterate_cyrillic_char(ch);
            if !tr.is_empty() {
                slug.push_str(tr);
                prev_dash = false;
            } else if !prev_dash && (ch == '-' || ch == '_' || ch.is_whitespace()) {
                slug.push('-');
                prev_dash = true;
            }
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    let base = if slug.is_empty() {
        "space".to_string()
    } else {
        slug.chars().take(32).collect()
    };
    let is_taken = |cand: &str| {
        cfg.spaces.iter().any(|s| s.id == cand)
            || cfg.custom_spaces.iter().any(|s| s.id == cand)
    };
    if !is_taken(&base) {
        return base;
    }
    let mut suffix = 2usize;
    loop {
        let cand = format!("{base}-{suffix}");
        if !is_taken(&cand) {
            return cand;
        }
        suffix += 1;
    }
}

struct SpaceTemplateSpec {
    accent_hex: &'static str,
    accent_rgb: &'static str,
    layout_kind: &'static str,
    show_hero_row: bool,
    widgets: &'static [&'static str],
}

fn space_template_spec(template: &str) -> Result<SpaceTemplateSpec, String> {
    match template.trim().to_ascii_lowercase().as_str() {
        "blank" | "" => Ok(SpaceTemplateSpec {
            accent_hex: "#A78BFA",
            accent_rgb: "167,139,250",
            layout_kind: SPACE_LAYOUT_GRID,
            show_hero_row: false,
            widgets: &[],
        }),
        "personal" => Ok(SpaceTemplateSpec {
            accent_hex: "#818CF8",
            accent_rgb: "129,140,248",
            layout_kind: SPACE_LAYOUT_GRID,
            show_hero_row: true,
            widgets: &["media", "audio_mixer", "clipboard"],
        }),
        "work" => Ok(SpaceTemplateSpec {
            accent_hex: "#38BDF8",
            accent_rgb: "56,189,248",
            layout_kind: SPACE_LAYOUT_GRID,
            show_hero_row: false,
            widgets: &["pc_health", "top_processes"],
        }),
        "studio" => Ok(SpaceTemplateSpec {
            accent_hex: "#34D399",
            accent_rgb: "52,211,153",
            layout_kind: SPACE_LAYOUT_GRID,
            show_hero_row: true,
            widgets: &["pc_health", "audio_mixer", "top_processes"],
        }),
        "games" => Ok(SpaceTemplateSpec {
            accent_hex: "#FB7185",
            accent_rgb: "251,113,133",
            layout_kind: SPACE_LAYOUT_GAMES,
            show_hero_row: false,
            widgets: &["audio_mixer", "game_combos", "pc_health"],
        }),
        other => Err(format!(
            "Неизвестный шаблон пространства: '{other}' (допустимы: blank, personal, work, studio, games)"
        )),
    }
}

pub fn create_space_from_template(
    cfg: &mut TestBarConfig,
    name: String,
    template: &str,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    widgets: Option<Vec<String>>,
) -> Result<SpaceConfig, String> {
    if cfg.spaces.len() >= MAX_SPACES {
        return Err(format!(
            "Достигнуто максимальное количество пространств ({MAX_SPACES})"
        ));
    }
    let clean_name = name.trim().to_string();
    if clean_name.is_empty() {
        return Err("Пустое название пространства".to_string());
    }
    let spec = space_template_spec(template)?;
    let (hex, rgb) = if accent_hex.is_some() || accent_rgb.is_some() {
        sanitize_accent_hex_rgb(
            accent_hex.as_deref().or(Some(spec.accent_hex)),
            accent_rgb.as_deref(),
        )
    } else {
        (spec.accent_hex.to_string(), spec.accent_rgb.to_string())
    };
    let final_widgets = match widgets {
        Some(w) => validate_and_normalize_space_widgets(&w)?,
        None => spec.widgets.iter().map(|s| (*s).to_string()).collect(),
    };
    let final_header_widgets = default_space_header_widgets(&final_widgets);
    let id = slugify_space_id(&clean_name, cfg);
    let bottom_title = default_bottom_section_title(template.trim(), spec.layout_kind).to_string();
    let space = SpaceConfig {
        id,
        name: clean_name,
        accent_hex: hex,
        accent_rgb: rgb,
        layout_kind: spec.layout_kind.to_string(),
        show_hero_row: spec.show_hero_row,
        bottom_section_title: Some(bottom_title),
        widgets: final_widgets,
        header_widgets: final_header_widgets,
        items: vec![],
    };
    cfg.spaces.push(space.clone());
    Ok(space)
}

#[allow(clippy::too_many_arguments)]
pub fn update_space_config(
    cfg: &mut TestBarConfig,
    id: &str,
    name: Option<String>,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    layout_kind: Option<String>,
    show_hero_row: Option<bool>,
    widgets: Option<Vec<String>>,
    bottom_section_title: Option<Option<String>>,
) -> Result<bool, String> {
    let next_name = match name {
        Some(raw) => {
            let clean = raw.trim().to_string();
            if clean.is_empty() {
                return Err("Пустое название пространства".to_string());
            }
            Some(clean)
        }
        None => None,
    };
    let next_layout = match layout_kind {
        Some(kind) => Some(validate_space_layout_kind(&kind)?.to_string()),
        None => None,
    };
    let next_widgets = match widgets {
        Some(w) => Some(validate_and_normalize_space_widgets(&w)?),
        None => None,
    };
    let next_bottom_title = bottom_section_title.map(|opt| sanitize_optional_nonempty(&opt));

    let Some(space) = cfg.spaces.iter_mut().find(|s| s.id == id) else {
        return Err(format!("Пространство '{id}' не найдено"));
    };

    let mut changed = false;
    if let Some(clean) = next_name {
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
    if let Some(layout) = next_layout {
        if space.layout_kind != layout {
            space.layout_kind = layout;
            changed = true;
        }
    }
    if let Some(hero) = show_hero_row {
        if space.show_hero_row != hero {
            space.show_hero_row = hero;
            changed = true;
        }
    }
    if let Some(w) = next_widgets {
        if space.widgets != w {
            space.widgets = w;
            changed = true;
        }
        let filtered_header =
            normalize_space_header_widgets(&space.header_widgets, &space.widgets);
        if space.header_widgets != filtered_header {
            space.header_widgets = filtered_header;
            changed = true;
        }
    }
    if let Some(title_opt) = next_bottom_title {
        if space.bottom_section_title != title_opt {
            space.bottom_section_title = title_opt;
            changed = true;
        }
    }

    Ok(changed)
}

pub fn delete_space_by_id(cfg: &mut TestBarConfig, id: &str) -> Result<bool, String> {
    let Some(pos) = cfg.spaces.iter().position(|s| s.id == id) else {
        return Ok(false);
    };
    if cfg.spaces.len() <= 1 {
        return Err("Нельзя удалить последнее пространство".to_string());
    }
    let removed = cfg.spaces.remove(pos);
    cfg.custom_spaces.retain(|s| s.id != id);
    cfg.frecency.remove(id);
    for item in &removed.items {
        cfg.frecency.remove(&item.id);
        cfg.frecency.remove(&format!("pin:{}", item.id));
    }
    Ok(true)
}

pub fn reorder_spaces_by_ids(
    cfg: &mut TestBarConfig,
    ordered_ids: Vec<String>,
) -> Result<bool, String> {
    if ordered_ids.len() != cfg.spaces.len() {
        return Err(format!(
            "Некорректный список пространств для сортировки: получено {}, ожидалось {}",
            ordered_ids.len(),
            cfg.spaces.len()
        ));
    }
    let mut seen = HashSet::with_capacity(ordered_ids.len());
    for id in &ordered_ids {
        if !seen.insert(id.as_str()) {
            return Err(format!("Дубликат идентификатора пространства: '{id}'"));
        }
        if !cfg.spaces.iter().any(|s| s.id == *id) {
            return Err(format!("Неизвестное пространство при сортировке: '{id}'"));
        }
    }
    if cfg
        .spaces
        .iter()
        .map(|s| s.id.as_str())
        .eq(ordered_ids.iter().map(String::as_str))
    {
        return Ok(false);
    }

    let mut by_id: HashMap<String, SpaceConfig> = cfg
        .spaces
        .drain(..)
        .map(|sp| (sp.id.clone(), sp))
        .collect();
    for id in ordered_ids {
        if let Some(sp) = by_id.remove(&id) {
            cfg.spaces.push(sp);
        }
    }
    Ok(true)
}

pub fn set_space_widgets_by_id(
    cfg: &mut TestBarConfig,
    id: &str,
    widgets: Vec<String>,
) -> Result<bool, String> {
    let normalized = validate_and_normalize_space_widgets(&widgets)?;
    let Some(space) = cfg.spaces.iter_mut().find(|s| s.id == id) else {
        return Err(format!("Пространство '{id}' не найдено"));
    };
    let filtered_header = normalize_space_header_widgets(&space.header_widgets, &normalized);
    if space.widgets == normalized && space.header_widgets == filtered_header {
        return Ok(false);
    }
    space.widgets = normalized;
    space.header_widgets = filtered_header;
    Ok(true)
}

pub fn set_space_widget_layout_in_config(
    cfg: &mut TestBarConfig,
    space_id: &str,
    widgets: Vec<String>,
    header_widgets: Vec<SpaceHeaderWidget>,
) -> Result<bool, String> {
    let normalized_widgets = validate_and_normalize_space_widgets(&widgets)?;
    let normalized_header =
        validate_and_normalize_space_header_widgets(&header_widgets, &normalized_widgets)?;
    let Some(space) = cfg.spaces.iter_mut().find(|s| s.id == space_id) else {
        return Err(format!("Пространство '{space_id}' не найдено"));
    };
    if space.widgets == normalized_widgets && space.header_widgets == normalized_header {
        return Ok(false);
    }
    space.widgets = normalized_widgets;
    space.header_widgets = normalized_header;
    Ok(true)
}

pub fn set_taskbar_widgets(
    cfg: &mut TestBarConfig,
    mut settings: TaskbarWidgetSettings,
) -> Result<bool, String> {
    settings.weather_city = settings.weather_city.trim().to_string();
    settings.weather_units = validate_weather_units(&settings.weather_units)?.to_string();
    settings.widgets_position =
        validate_taskbar_widgets_position(&settings.widgets_position)?.to_string();
    settings.left_widgets = validate_taskbar_widget_list(&settings.left_widgets)?;

    match (settings.weather_lat, settings.weather_lon) {
        (Some(lat), Some(lon)) => {
            if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
                return Err("Некорректная широта погоды".to_string());
            }
            if !lon.is_finite() || !(-180.0..=180.0).contains(&lon) {
                return Err("Некорректная долгота погоды".to_string());
            }
        }
        (None, None) => {}
        _ => return Err("Укажите обе координаты (широту и долготу)".to_string()),
    }

    if cfg.taskbar_widgets == settings {
        return Ok(false);
    }
    cfg.taskbar_widgets = settings;
    Ok(true)
}

pub fn update_custom_space(
    cfg: &mut TestBarConfig,
    id: &str,
    name: Option<String>,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    widget_preset: Option<String>,
) -> Result<bool, String> {
    let next_name = match name {
        Some(raw) => {
            let clean = raw.trim().to_string();
            if clean.is_empty() {
                return Err("Пустое название пространства".to_string());
            }
            Some(clean)
        }
        None => None,
    };
    let next_preset = match widget_preset {
        Some(preset) => Some(validate_widget_preset(&preset)?.to_string()),
        None => None,
    };

    let mut found = false;
    let mut changed = false;

    if let Some(space) = cfg.custom_spaces.iter_mut().find(|s| s.id == id) {
        found = true;
        if let Some(ref clean) = next_name {
            if space.name != *clean {
                space.name = clean.clone();
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
        if let Some(ref valid) = next_preset {
            if space.widget_preset != *valid {
                space.widget_preset = valid.clone();
                changed = true;
            }
        }
    }

    if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == id) {
        found = true;
        if let Some(ref clean) = next_name {
            if sp.name != *clean {
                sp.name = clean.clone();
                changed = true;
            }
        }
        if accent_hex.is_some() || accent_rgb.is_some() {
            let (hex, rgb) = sanitize_accent_hex_rgb(
                accent_hex.as_deref().or(Some(&sp.accent_hex)),
                accent_rgb.as_deref(),
            );
            if sp.accent_hex != hex {
                sp.accent_hex = hex;
                changed = true;
            }
            if sp.accent_rgb != rgb {
                sp.accent_rgb = rgb;
                changed = true;
            }
        }
        if let Some(ref valid) = next_preset {
            let widgets = widget_preset_to_widgets(valid);
            if sp.widgets != widgets {
                sp.widgets = widgets;
                changed = true;
            }
        }
    }

    if !found {
        return Err(format!("Пространство '{id}' не найдено"));
    }

    Ok(changed)
}

pub fn delete_custom_space(cfg: &mut TestBarConfig, id: &str) -> bool {
    let Some(pos) = cfg.custom_spaces.iter().position(|s| s.id == id) else {
        return false;
    };
    let removed = cfg.custom_spaces.remove(pos);
    if cfg.spaces.len() > 1 {
        cfg.spaces.retain(|s| s.id != id);
    }
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
        "taskbar" | "taskbar_pins" => cfg
            .taskbar_pins
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "game_combos" => cfg
            .game_combos
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, true)),
        "personal_pins" => cfg
            .personal_pins
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "personal_links" => cfg
            .personal_links
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
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
        "studio_pins" => cfg
            .studio_pins
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "studio_tools" => cfg
            .studio_tools
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        "game_launchers" => cfg
            .game_launchers
            .iter_mut()
            .find(|p| p.id == pin_id)
            .map(|p| (p, false)),
        space_id => {
            if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == space_id) {
                if let Some(p) = sp.items.iter_mut().find(|p| p.id == pin_id) {
                    return Some((p, false));
                }
            }
            if space_id == "games" {
                if let Some(p) = cfg.game_combos.iter_mut().find(|p| p.id == pin_id) {
                    return Some((p, true));
                }
            }
            if let Some(cs) = cfg.custom_spaces.iter_mut().find(|s| s.id == space_id) {
                if let Some(p) = cs.items.iter_mut().find(|p| p.id == pin_id) {
                    return Some((p, false));
                }
            }
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_pin_v2(
    cfg: &mut TestBarConfig,
    space: &str,
    pin_id: &str,
    title: Option<String>,
    target: Option<String>,
    args: Option<String>,
    alt_action: Option<String>,
    alt_label: Option<String>,
    custom_icon: Option<Option<String>>,
    custom_color: Option<Option<String>>,
    icon_data: Option<Option<String>>,
    tile_size: Option<Option<String>>,
    section: Option<Option<String>>,
    run_as_admin: Option<bool>,
) -> Result<bool, String> {
    let next_title = match title {
        Some(raw) => {
            let clean = raw.trim().to_string();
            if clean.is_empty() {
                return Err("Название элемента не может быть пустым".to_string());
            }
            Some(clean)
        }
        None => None,
    };

    let next_tile_size = match tile_size {
        None => None,
        Some(None) => Some(None),
        Some(Some(raw)) => Some(Some(validate_tile_size(&raw)?.to_string())),
    };

    let next_section = match section {
        None => None,
        Some(None) => Some(None),
        Some(Some(raw)) => Some(Some(validate_tile_section(&raw)?.to_string())),
    };

    let Some((pin_item, force_combo)) = find_pin_mut(cfg, space, pin_id) else {
        return Err(format!("Элемент '{pin_id}' не найден в '{space}'"));
    };
    let before = pin_item.clone();

    if let Some(clean) = next_title {
        pin_item.title = clean;
    }

    if let Some(raw_target) = target {
        let clean = raw_target.trim().to_string();
        if pin_item.target != clean {
            pin_item.target = clean;
            if pin_item.kind == "app" {
                pin_item.match_exe = None;
            }
        }
    }

    if let Some(raw_args) = args {
        let trimmed = raw_args.trim();
        pin_item.args = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    if let Some(raw_alt) = alt_action {
        pin_item.alt_action = raw_alt.trim().to_string();
    }

    if let Some(raw_label) = alt_label {
        let trimmed = raw_label.trim();
        pin_item.alt_label = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    if let Some(next_icon_opt) = custom_icon {
        pin_item.custom_icon = next_icon_opt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }

    if let Some(next_color_opt) = custom_color {
        pin_item.custom_color = next_color_opt.as_deref().and_then(sanitize_custom_color);
    }

    if let Some(next_data_opt) = icon_data {
        pin_item.icon_data = next_data_opt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }

    if let Some(next_size_opt) = next_tile_size {
        pin_item.tile_size = next_size_opt;
    }

    if let Some(next_sec_opt) = next_section {
        pin_item.section = next_sec_opt;
    }

    if let Some(admin) = run_as_admin {
        pin_item.run_as_admin = admin;
    }

    normalize_pin(pin_item, force_combo);
    let mut changed = *pin_item != before;

    // Mirror edits made through legacy sub-list keys into the corresponding v2 space if present.
    if let Some(parent_id) = legacy_parent_space_id(space) {
        let updated_clone = pin_item.clone();
        if let Some(sp) = cfg.spaces.iter_mut().find(|s| s.id == parent_id) {
            if let Some(mirror) = sp.items.iter_mut().find(|p| p.id == pin_id) {
                if *mirror != updated_clone {
                    *mirror = updated_clone;
                    changed = true;
                }
            }
        }
    }

    Ok(changed)
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
    update_pin_v2(
        cfg, space, pin_id, title, target, args, alt_action, alt_label, None, None, None, None,
        None, None,
    )
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

#[derive(Debug, Default, Clone)]
pub struct AppearancePatch {
    pub glass_depth: Option<String>,
    pub theme_preset: Option<String>,
    pub accent_mode: Option<String>,
    pub accent_hex: Option<String>,
    pub accent_rgb: Option<String>,
    pub surface_gradients: Option<bool>,
    pub hover_glow: Option<bool>,
    pub clock_show_seconds: Option<bool>,
    pub clock_show_weekday: Option<bool>,
    pub clock_date_format: Option<String>,
    pub clock_layout: Option<String>,
    pub start_remember_space: Option<bool>,
    pub default_tile_size: Option<String>,
    pub reset_visuals: bool,
}

pub fn set_appearance_settings_ext(
    cfg: &mut TestBarConfig,
    patch: AppearancePatch,
) -> Result<bool, String> {
    let mut changed = false;

    if patch.reset_visuals {
        if cfg.glass_depth != GLASS_DEPTH_OBSIDIAN {
            cfg.glass_depth = GLASS_DEPTH_OBSIDIAN.to_string();
            changed = true;
        }
        if cfg.theme_preset != THEME_PRESET_OBSIDIAN {
            cfg.theme_preset = THEME_PRESET_OBSIDIAN.to_string();
            changed = true;
        }
        if cfg.accent_mode != ACCENT_MODE_PER_SPACE {
            cfg.accent_mode = ACCENT_MODE_PER_SPACE.to_string();
            changed = true;
        }
        let def_hex = default_global_accent_hex();
        let def_rgb = default_global_accent_rgb();
        if cfg.accent_hex != def_hex {
            cfg.accent_hex = def_hex;
            changed = true;
        }
        if cfg.accent_rgb != def_rgb {
            cfg.accent_rgb = def_rgb;
            changed = true;
        }
        if !cfg.surface_gradients {
            cfg.surface_gradients = true;
            changed = true;
        }
        if !cfg.hover_glow {
            cfg.hover_glow = true;
            changed = true;
        }
    }

    if let Some(depth) = patch.glass_depth {
        let valid = validate_glass_depth(&depth)?;
        if cfg.glass_depth != valid {
            cfg.glass_depth = valid.to_string();
            changed = true;
        }
    }
    if let Some(theme) = patch.theme_preset {
        let valid = validate_theme_preset(&theme)?;
        if cfg.theme_preset != valid {
            cfg.theme_preset = valid.to_string();
            changed = true;
        }
    }
    if let Some(mode) = patch.accent_mode {
        let valid = validate_accent_mode(&mode)?;
        if cfg.accent_mode != valid {
            cfg.accent_mode = valid.to_string();
            changed = true;
        }
    }
    if patch.accent_hex.is_some() || patch.accent_rgb.is_some() {
        let (hex, rgb) = sanitize_accent_hex_rgb(
            patch.accent_hex.as_deref().or(Some(&cfg.accent_hex)),
            patch.accent_rgb.as_deref(),
        );
        if cfg.accent_hex != hex {
            cfg.accent_hex = hex;
            changed = true;
        }
        if cfg.accent_rgb != rgb {
            cfg.accent_rgb = rgb;
            changed = true;
        }
    }
    if let Some(grad) = patch.surface_gradients {
        if cfg.surface_gradients != grad {
            cfg.surface_gradients = grad;
            changed = true;
        }
    }
    if let Some(glow) = patch.hover_glow {
        if cfg.hover_glow != glow {
            cfg.hover_glow = glow;
            changed = true;
        }
    }
    if let Some(sec) = patch.clock_show_seconds {
        if cfg.clock_show_seconds != sec {
            cfg.clock_show_seconds = sec;
            changed = true;
        }
    }
    if let Some(wd) = patch.clock_show_weekday {
        if cfg.clock_show_weekday != wd {
            cfg.clock_show_weekday = wd;
            changed = true;
        }
    }
    if let Some(fmt) = patch.clock_date_format {
        let valid = validate_clock_date_format(&fmt)?;
        if cfg.clock_date_format != valid {
            cfg.clock_date_format = valid.to_string();
            changed = true;
        }
    }
    if let Some(layout) = patch.clock_layout {
        let valid = validate_clock_layout(&layout)?;
        if cfg.clock_layout != valid {
            cfg.clock_layout = valid.to_string();
            changed = true;
        }
    }
    if let Some(rem) = patch.start_remember_space {
        if cfg.start_remember_space != rem {
            cfg.start_remember_space = rem;
            changed = true;
        }
    }
    if let Some(size) = patch.default_tile_size {
        let valid = validate_tile_size(&size)?;
        if cfg.default_tile_size != valid {
            cfg.default_tile_size = valid.to_string();
            changed = true;
        }
    }
    Ok(changed)
}

pub fn set_appearance_settings(
    cfg: &mut TestBarConfig,
    glass_depth: Option<String>,
    clock_show_seconds: Option<bool>,
    clock_show_weekday: Option<bool>,
    start_remember_space: Option<bool>,
) -> Result<bool, String> {
    set_appearance_settings_ext(
        cfg,
        AppearancePatch {
            glass_depth,
            clock_show_seconds,
            clock_show_weekday,
            start_remember_space,
            ..Default::default()
        },
    )
}

pub fn set_quick_notes_text(cfg: &mut TestBarConfig, raw_text: &str) -> bool {
    let clamped: String = raw_text.chars().take(MAX_QUICK_NOTES_CHARS).collect();
    if cfg.quick_notes.text == clamped {
        return false;
    }
    cfg.quick_notes.text = clamped;
    true
}

pub fn add_quick_todo(
    cfg: &mut TestBarConfig,
    id: String,
    raw_text: &str,
) -> Result<bool, String> {
    if cfg.quick_notes.todos.len() >= MAX_QUICK_TODOS {
        return Err(format!("Максимум {MAX_QUICK_TODOS} задач в списке"));
    }
    let clean: String = raw_text.trim().chars().take(MAX_QUICK_TODO_CHARS).collect();
    if clean.is_empty() {
        return Err("Текст задачи не может быть пустым".to_string());
    }
    cfg.quick_notes.todos.push(QuickTodoItem {
        id,
        text: clean,
        done: false,
    });
    Ok(true)
}

pub fn toggle_quick_todo(cfg: &mut TestBarConfig, id: &str) -> bool {
    let Some(item) = cfg.quick_notes.todos.iter_mut().find(|t| t.id == id) else {
        return false;
    };
    item.done = !item.done;
    true
}

pub fn delete_quick_todo(cfg: &mut TestBarConfig, id: &str) -> bool {
    let before = cfg.quick_notes.todos.len();
    cfg.quick_notes.todos.retain(|t| t.id != id);
    cfg.quick_notes.todos.len() != before
}

pub fn clear_done_quick_todos(cfg: &mut TestBarConfig) -> bool {
    let before = cfg.quick_notes.todos.len();
    cfg.quick_notes.todos.retain(|t| !t.done);
    cfg.quick_notes.todos.len() != before
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

pub fn hide_battery_device(cfg: &mut TestBarConfig, id: &str) -> bool {
    let norm = id.trim().to_ascii_lowercase();
    if norm.is_empty() {
        return false;
    }
    if cfg
        .hidden_battery_ids
        .iter()
        .any(|k| k.eq_ignore_ascii_case(&norm))
    {
        return false;
    }
    cfg.hidden_battery_ids.push(norm);
    true
}

pub fn restore_battery_device(cfg: &mut TestBarConfig, id: &str) -> bool {
    let norm = id.trim().to_ascii_lowercase();
    if norm.is_empty() {
        return false;
    }
    let before = cfg.hidden_battery_ids.len();
    cfg.hidden_battery_ids
        .retain(|k| !k.eq_ignore_ascii_case(&norm));
    cfg.hidden_battery_ids.len() != before
}

pub fn restore_all_battery_devices(cfg: &mut TestBarConfig) -> bool {
    if cfg.hidden_battery_ids.is_empty() {
        return false;
    }
    cfg.hidden_battery_ids.clear();
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

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
struct RegKeyGuard(HKEY);

#[cfg(target_os = "windows")]
impl Drop for RegKeyGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn to_wide_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
pub fn set_windows_autostart(enable: bool) -> Result<(), String> {
    let exe = if enable {
        std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?
    } else {
        PathBuf::new()
    };
    set_windows_autostart_at(AUTOSTART_RUN_SUBKEY, AUTOSTART_VALUE_NAME, &exe, enable)
}

pub fn resolve_macos_bundle_executable(exe_path: &Path) -> PathBuf {
    let raw = exe_path.to_string_lossy();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return PathBuf::new();
    }
    let components: Vec<std::path::Component<'_>> = exe_path.components().collect();
    for (idx, comp) in components.iter().enumerate() {
        let name = comp.as_os_str().to_string_lossy();
        if name.to_ascii_lowercase().ends_with(".app") {
            let mut app_root = PathBuf::new();
            for c in &components[..=idx] {
                app_root.push(c.as_os_str());
            }
            let exe_name = exe_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "TestBar".to_string());
            let candidate = app_root.join("Contents").join("MacOS").join(exe_name);
            if candidate == exe_path || candidate.is_file() {
                return candidate;
            }
        }
    }
    exe_path.to_path_buf()
}

fn escape_xml_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn build_macos_launch_agent_plist(exe_path: &Path) -> Result<String, String> {
    let resolved = resolve_macos_bundle_executable(exe_path);
    let raw = resolved.to_string_lossy();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("autostart executable path is empty".to_string());
    }
    if trimmed.contains('\0') {
        return Err("autostart executable path contains NUL byte".to_string());
    }
    let escaped = escape_xml_text(trimmed);
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.testbar.shell</string>
  <key>ProgramArguments</key>
  <array>
    <string>{escaped}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#
    ))
}

#[cfg(target_os = "macos")]
pub fn macos_launch_agent_plist_path() -> PathBuf {
    user_home()
        .join("Library")
        .join("LaunchAgents")
        .join("com.testbar.shell.plist")
}

#[cfg(target_os = "macos")]
pub fn set_macos_autostart_at(
    plist_path: &Path,
    exe_path: &Path,
    enable: bool,
) -> Result<(), String> {
    if enable {
        let plist = build_macos_launch_agent_plist(exe_path)?;
        ensure_parent_dir(plist_path)?;
        let tmp = sibling_tmp_path(plist_path);
        fs::write(&tmp, plist.as_bytes()).map_err(|e| format!("write LaunchAgent tmp: {e}"))?;
        if let Err(err) = fs::rename(&tmp, plist_path) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("rename LaunchAgent plist: {err}"));
        }
        Ok(())
    } else {
        match fs::remove_file(plist_path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("remove LaunchAgent plist: {err}")),
        }
    }
}

#[cfg(target_os = "macos")]
pub fn set_macos_autostart(enable: bool) -> Result<(), String> {
    let exe = if enable {
        std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?
    } else {
        PathBuf::new()
    };
    set_macos_autostart_at(&macos_launch_agent_plist_path(), &exe, enable)
}

fn ensure_parent_dir(p: &Path) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
        }
    }
    Ok(())
}

fn path_exists_including_reparse(path: &str) -> bool {
    let trimmed = path.trim().trim_matches('"');
    if trimmed.is_empty() {
        return false;
    }
    let p = Path::new(trimmed);
    fs::symlink_metadata(p).is_ok() || p.exists()
}

#[cfg(target_os = "windows")]
fn looks_like_local_fs_path(target: &str) -> bool {
    let t = target.trim().trim_matches('"');
    let bytes = t.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/'))
        || t.starts_with(r"\\")
}

#[cfg(target_os = "windows")]
fn is_non_app_uri(target: &str) -> bool {
    let low = target.trim().to_ascii_lowercase();
    low.starts_with("http://")
        || low.starts_with("https://")
        || low.starts_with("steam://")
        || low.starts_with("ms-")
        || low.starts_with("mailto:")
}

#[cfg(target_os = "windows")]
fn pin_target_needs_healing(target: &str) -> bool {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed
        .to_ascii_lowercase()
        .starts_with("shell:appsfolder\\")
        || is_non_app_uri(trimmed)
    {
        return false;
    }
    if looks_like_local_fs_path(trimmed) {
        return !path_exists_including_reparse(trimmed);
    }
    false
}

#[cfg(target_os = "windows")]
fn normalize_match_token(raw: &str) -> String {
    raw.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[cfg(target_os = "windows")]
fn is_uninstaller_or_helper_label(name_or_target: &str) -> bool {
    let low = name_or_target.to_lowercase();
    low.contains("uninstall")
        || low.contains("удален")
        || low.contains("деинсталл")
        || low.contains("report a problem")
        || low.contains("crashpad")
        || low.contains("installer")
        || low.contains("setup.exe")
}

#[cfg(target_os = "windows")]
fn canonical_detect_key_for_pin(pin: &PinItem) -> Option<&'static str> {
    let id_norm = normalize_match_token(&pin.id);
    let title_norm = normalize_match_token(&pin.title);
    let candidates = [id_norm.as_str(), title_norm.as_str()];
    for c in candidates {
        match c {
            "spark" | "sparkmail" | "sparkdesktop" => return Some("spark"),
            "cursor" | "wacur" | "cursors" => return Some("cursor"),
            "yandex" | "яндекс" | "yandexbrowser" | "яндексбраузер" => return Some("yandex"),
            "unity" | "unityhub" => return Some("unity"),
            "zen" | "zenbrowser" => return Some("zen"),
            "telegram" | "telegramdesktop" => return Some("telegram"),
            "ymusic" | "ямузыка" | "яндексмузыка" | "yandexmusic" => return Some("ymusic"),
            "discord" | "gldiscord" => return Some("discord"),
            "obs" | "obss" | "globs" | "obsstudio" => return Some("obs"),
            "antigravity" | "waag" | "antigravitys" => return Some("antigravity"),
            "vscode" | "wavsc" | "vscodes" | "visualstudiocode" => return Some("vscode"),
            "obsidian" | "obsidians" => return Some("obsidian"),
            "photoshop" | "photoshop2025" => return Some("photoshop"),
            "lightroom" => return Some("lightroom"),
            "figma" => return Some("figma"),
            "steam" | "glsteam" => return Some("steam"),
            "battlenet" | "glbnet" => return Some("battlenet"),
            "warp" | "warps" => return Some("warp"),
            "wt" | "wawt" | "терминал" | "windowsterminal" => return Some("wt"),
            _ => {}
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn alias_tokens_for_pin(pin: &PinItem) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |s: &str| {
        let norm = normalize_match_token(s);
        if norm.len() >= 2 && !out.contains(&norm) {
            out.push(norm);
        }
    };
    push(&pin.title);
    // Avoid noisy generic auto-generated pin ids like `app-1795267558157`
    if !pin.id.to_ascii_lowercase().starts_with("app-") {
        push(&pin.id);
    }
    if let Some(ref exe) = pin.match_exe {
        push(exe.trim_end_matches(".exe"));
    }

    match canonical_detect_key_for_pin(pin) {
        Some("spark") => {
            push("spark desktop");
            push("sparkdesktop");
            push("spark mail");
            push("spark");
        }
        Some("cursor") => {
            push("cursor");
        }
        Some("yandex") => {
            push("yandex");
            push("yandex browser");
            push("яндекс");
            push("яндекс браузер");
        }
        Some("unity") => {
            push("unity hub");
            push("unityhub");
            push("unity");
        }
        Some("ymusic") => {
            push("яндекс музыка");
            push("yandex music");
            push("yandexmusic");
        }
        Some("vscode") => {
            push("visual studio code");
            push("vs code");
            push("code");
        }
        _ => {}
    }
    out
}

#[cfg(target_os = "windows")]
fn apply_healed_target(
    pin: &mut PinItem,
    new_target: String,
    new_aumid: Option<String>,
) -> bool {
    let new_target = new_target.trim().to_string();
    if new_target.is_empty() {
        return false;
    }
    let mut changed = false;
    if pin.target != new_target {
        pin.target = new_target.clone();
        changed = true;
    }
    if let Some(aumid) = new_aumid
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
    {
        if pin.match_aumid.as_deref() != Some(aumid.as_str()) {
            pin.match_aumid = Some(aumid);
            changed = true;
        }
    } else if let Some(rest) = new_target
        .strip_prefix("shell:AppsFolder\\")
        .or_else(|| new_target.strip_prefix("shell:appsfolder\\"))
    {
        let cleaned = rest.trim().to_string();
        if !cleaned.is_empty() && pin.match_aumid.as_deref() != Some(cleaned.as_str()) {
            pin.match_aumid = Some(cleaned);
            changed = true;
        }
    } else if let Some(exe) = derive_match_exe(&new_target) {
        if pin.match_exe.as_deref() != Some(exe.as_str()) {
            pin.match_exe = Some(exe);
            changed = true;
        }
    }
    if changed {
        let valid_ns = pin.icon_key.starts_with("exe:")
            || pin.icon_key.starts_with("aumid:")
            || pin.icon_key.starts_with("file:")
            || pin.icon_key.starts_with("hwnd:");
        if !valid_ns && !pin.icon_key.is_empty() {
            pin.icon_key.clear();
        }
        if pin.custom_icon.as_deref() != Some("custom_image") {
            pin.icon_data = None;
        }
    }
    changed
}

#[cfg(target_os = "windows")]
fn try_heal_single_pin(
    pin: &mut PinItem,
    installed_apps: &[crate::apps_indexer::IndexedApp],
    open_groups: &[crate::windows_tracker::AppWindowGroup],
) -> bool {
    if !pin.kind.is_empty() && !pin.kind.eq_ignore_ascii_case("app") {
        return false;
    }

    // Backfill missing matchAumid / matchExe on already-valid targets so taskbar/start matching works cleanly.
    if !pin_target_needs_healing(&pin.target) {
        let mut metadata_updated = false;
        let t_low = pin.target.trim().to_ascii_lowercase();
        if t_low.starts_with("shell:appsfolder\\") && pin.match_aumid.is_none() {
            let aumid = pin.target.trim()["shell:appsfolder\\".len()..].trim().to_string();
            if !aumid.is_empty() {
                pin.match_aumid = Some(aumid);
                metadata_updated = true;
            }
        } else if pin.match_exe.is_none() {
            if let Some(exe) = derive_match_exe(&pin.target) {
                pin.match_exe = Some(exe);
                metadata_updated = true;
            }
        }
        return metadata_updated;
    }

    let aliases = alias_tokens_for_pin(pin);
    let pin_match_exe = pin
        .match_exe
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let pin_match_aumid = pin
        .match_aumid
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    // 1. Live running windows (ground truth — e.g. Spark Desktop running on Monitor 2!)
    let mut best_group: Option<(u32, String, Option<String>)> = None;
    for g in open_groups {
        let norm_exe = crate::windows_tracker::normalize_jump_list_exe_path(&g.exe_path);
        let is_afh = norm_exe
            .to_ascii_lowercase()
            .ends_with("applicationframehost.exe");
        let g_target = if is_afh || norm_exe.is_empty() {
            match g.aumid.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(aumid) => format!("shell:AppsFolder\\{aumid}"),
                None => continue,
            }
        } else if is_uninstaller_or_helper_label(&norm_exe) {
            continue;
        } else {
            norm_exe.clone()
        };

        let g_exe_low = derive_match_exe(&norm_exe).unwrap_or_default();
        let g_aumid_low = g
            .aumid
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let g_name_norm = normalize_match_token(&g.name);
        let g_stem_norm = normalize_match_token(g_exe_low.trim_end_matches(".exe"));

        let mut score = 0u32;
        if pin_match_aumid.as_deref() == Some(g_aumid_low.as_str()) && !g_aumid_low.is_empty() {
            score = 100;
        } else if pin_match_exe.as_deref() == Some(g_exe_low.as_str()) && !g_exe_low.is_empty() {
            score = 95;
        } else {
            for a in &aliases {
                if !g_name_norm.is_empty() && (g_name_norm == *a || g_stem_norm == *a) {
                    score = score.max(90);
                } else if a.len() >= 4
                    && ((!g_name_norm.is_empty()
                        && (g_name_norm.starts_with(a.as_str()) || a.starts_with(&g_name_norm)))
                        || (!g_stem_norm.is_empty()
                            && (g_stem_norm.starts_with(a.as_str())
                                || a.starts_with(&g_stem_norm))))
                {
                    score = score.max(75);
                }
            }
        }
        if score > best_group.as_ref().map(|b| b.0).unwrap_or(0) {
            best_group = Some((score, g_target, g.aumid.clone()));
        }
    }
    if let Some((_, target, aumid)) = best_group {
        return apply_healed_target(pin, target, aumid);
    }

    // 2. Installed apps (Start Menu shortcuts + UWP AppsFolder)
    let mut best_app: Option<(u32, String, Option<String>)> = None;
    for app in installed_apps {
        let target = app.target.trim();
        if target.is_empty()
            || is_uninstaller_or_helper_label(&app.name)
            || is_uninstaller_or_helper_label(target)
        {
            continue;
        }
        let app_aumid_low = app
            .aumid
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let app_exe_low = derive_match_exe(target).unwrap_or_default();
        let app_name_norm = normalize_match_token(&app.name);
        let app_stem_norm = normalize_match_token(app_exe_low.trim_end_matches(".exe"));

        let mut score = 0u32;
        if pin_match_aumid.as_deref() == Some(app_aumid_low.as_str()) && !app_aumid_low.is_empty() {
            score = 100;
        } else if pin_match_exe.as_deref() == Some(app_exe_low.as_str()) && !app_exe_low.is_empty()
        {
            score = 95;
        } else {
            for a in &aliases {
                if !app_name_norm.is_empty() && (app_name_norm == *a || app_stem_norm == *a) {
                    score = score.max(90);
                } else if a.len() >= 4
                    && ((!app_name_norm.is_empty() && app_name_norm.starts_with(a.as_str()))
                        || (!app_stem_norm.is_empty() && app_stem_norm.starts_with(a.as_str())))
                {
                    // Avoid matching e.g. "Yandex Telemost" / "Yandex Disk" for bare "yandex" unless exact
                    let is_subproduct = (a == "yandex" || a == "яндекс")
                        && (app_name_norm.contains("disk")
                            || app_name_norm.contains("telemost")
                            || app_name_norm.contains("телемост")
                            || app_name_norm.contains("music")
                            || app_name_norm.contains("музык"));
                    if !is_subproduct {
                        score = score.max(75);
                    }
                }
            }
        }
        if score > 0 && app.target.to_ascii_lowercase().ends_with(".exe") {
            score += 2; // slightly prefer direct .exe over .lnk when tied
        }
        if score > best_app.as_ref().map(|b| b.0).unwrap_or(0) {
            best_app = Some((score, target.to_string(), app.aumid.clone()));
        }
    }
    if let Some((_, target, aumid)) = best_app {
        return apply_healed_target(pin, target, aumid);
    }

    // 3. Built-in known filesystem candidate probe (only if target actually exists on disk!)
    if let Some(key) = canonical_detect_key_for_pin(pin) {
        let detected = detect_app_target(key);
        if !detected.is_empty()
            && !is_non_app_uri(&detected)
            && path_exists_including_reparse(&detected)
        {
            return apply_healed_target(pin, detected, None);
        }
    }

    false
}

/// Auto-heals empty or broken app pin targets across ALL TestBar pin lists (`taskbarPins`,
/// v2 `spaces`, legacy lists, and `customSpaces`) by matching against live running windows,
/// indexed installed apps, and canonical disk locations.
/// Returns `true` iff at least one pin was repaired or enriched.
#[cfg(target_os = "windows")]
pub fn heal_broken_pins(
    cfg: &mut TestBarConfig,
    installed_apps: &[crate::apps_indexer::IndexedApp],
    open_groups: &[crate::windows_tracker::AppWindowGroup],
) -> bool {
    let mut changed = false;
    for list in [
        &mut cfg.taskbar_pins,
        &mut cfg.personal_pins,
        &mut cfg.work_apps,
        &mut cfg.studio_pins,
        &mut cfg.game_launchers,
    ] {
        for pin in list.iter_mut() {
            if try_heal_single_pin(pin, installed_apps, open_groups) {
                changed = true;
            }
        }
    }
    for sp in &mut cfg.spaces {
        for pin in &mut sp.items {
            if try_heal_single_pin(pin, installed_apps, open_groups) {
                changed = true;
            }
        }
    }
    for sp in &mut cfg.custom_spaces {
        for pin in &mut sp.items {
            if try_heal_single_pin(pin, installed_apps, open_groups) {
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "windows")]
    use windows::Win32::Foundation::ERROR_ACCESS_DENIED;

    #[cfg(target_os = "macos")]
    const DEFAULT_SPACES_COUNT: usize = 3;
    #[cfg(not(target_os = "macos"))]
    const DEFAULT_SPACES_COUNT: usize = 4;

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
        assert_eq!(cfg.spaces.len(), DEFAULT_SPACES_COUNT);
        assert!(!cfg.spaces[0].items.is_empty());
        assert!(!normalize_and_migrate(&mut cfg));

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
        assert!(cfg.spaces[0].items.iter().any(|p| p.id == "custom-folder-1"));

        let prev_first = cfg.spaces[0].items[0].id.clone();
        assert!(reorder_pins_in_space(&mut cfg, "personal", 0, 1));
        assert_eq!(cfg.spaces[0].items[1].id, prev_first);

        assert!(remove_pin_from_space(&mut cfg, "personal", "custom-folder-1"));
        assert!(!cfg.spaces[0].items.iter().any(|p| p.id == "custom-folder-1"));

        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: TestBarConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.bar_mode, "dock");
        assert_eq!(parsed.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(parsed.spaces.len(), DEFAULT_SPACES_COUNT);
    }

    #[test]
    fn derive_match_exe_handles_executables_and_rejects_urls_folders_and_schemes() {
        assert_eq!(
            derive_match_exe(r"C:\Program Files\Firefox\Firefox.EXE"),
            Some("firefox.exe".to_string())
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
            derive_match_exe(r"\\?\C:\Program Files\Mozilla Firefox\firefox.exe"),
            Some("firefox.exe".to_string())
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
        assert_eq!(derive_match_exe(r"C:\Users\alice\Downloads"), None);
        assert_eq!(derive_match_exe(r"C:\Users\alice\app.exe\"), None);
        assert_eq!(derive_match_exe(".exe"), None);
        assert_eq!(derive_match_exe("   "), None);
    }

    #[test]
    fn load_config_handles_utf8_bom_missing_bar_mode_and_preserves_newer_schema_version() {
        let dir = make_temp_dir("bom-partial-v3");

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
        assert!(loaded_partial.spaces.iter().any(|s| s.id == "cs1"));

        // 2. UTF-8 BOM prefixed file loads cleanly without resetting pins
        let bom_path = dir.join("bom.json");
        let bom_json = format!("\u{feff}{}", r##"{"schemaVersion":1,"revision":3,"barMode":"edge","taskbarPins":[{"id":"b1","kind":"app","title":"BomApp","target":"C:\\Apps\\Bom.exe","matchExe":"bom.exe"}]}"##);
        fs::write(&bom_path, bom_json).expect("write bom");
        let loaded_bom = load_config_from(&bom_path);
        assert_eq!(loaded_bom.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded_bom.bar_mode, "edge");
        assert_eq!(loaded_bom.revision, 3);
        assert_eq!(loaded_bom.taskbar_pins.len(), 1);
        assert_eq!(loaded_bom.taskbar_pins[0].title, "BomApp");
        assert!(!loaded_bom.spaces.is_empty());

        // 3. Newer schemaVersion (99) is preserved untouched and not downgraded
        let v99_path = dir.join("v99.json");
        let v99_json = r##"{"schemaVersion":99,"revision":10,"barMode":"dock","taskbarPins":[{"id":"v3p","kind":"app","title":"FutureApp","target":"C:\\Apps\\Future.exe","matchExe":"future.exe"}],"spaces":[{"id":"personal","name":"Личное","accentHex":"#818CF8","accentRgb":"129,140,248","layoutKind":"grid","showHeroRow":true,"widgets":["media"],"items":[]}]}"##;
        fs::write(&v99_path, v99_json).expect("write v99");
        let mut loaded_v99 = load_config_from(&v99_path);
        assert_eq!(loaded_v99.schema_version, 99);
        assert_eq!(loaded_v99.revision, 10);
        assert!(!normalize_and_migrate(&mut loaded_v99));
        assert_eq!(loaded_v99.schema_version, 99);
        let on_disk_v99: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&v99_path).expect("read v99")).expect("parse");
        assert_eq!(on_disk_v99["schemaVersion"], 99);

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
                custom_icon: Some("  chat  ".to_string()),
                custom_color: Some(" #38bdf8 ".to_string()),
                tile_size: None,
                section: None,
                run_as_admin: false,
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
                custom_icon: Some("   ".to_string()),
                custom_color: Some("   ".to_string()),
                tile_size: None,
                section: None,
                run_as_admin: false,
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
                custom_icon: None,
                custom_color: None,
                tile_size: None,
                section: None,
                run_as_admin: false,
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
            custom_icon: None,
            custom_color: None,
            tile_size: None,
            section: None,
            run_as_admin: false,
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
        assert_eq!(cfg.personal_pins[0].custom_icon.as_deref(), Some("chat"));
        assert_eq!(cfg.personal_pins[0].custom_color.as_deref(), Some("#38BDF8"));

        assert_eq!(cfg.personal_pins[1].kind, "combo");
        assert_eq!(
            cfg.personal_pins[1].combo_targets,
            Some(vec!["steam://rungameid/570".to_string()])
        );
        assert_eq!(
            cfg.personal_pins[1].match_aumid.as_deref(),
            Some("My.App!Id")
        );
        assert_eq!(cfg.personal_pins[1].custom_icon, None);
        assert_eq!(cfg.personal_pins[1].custom_color, None);

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
    fn legacy_v0_migration_preserves_pins_and_persists_v2_atomically() {
        let dir = make_temp_dir("v0-migrate");
        let path = dir.join("config.json");

        let legacy_v0 = r##"{
          "barMode": "edge",
          "autostart": true,
          "taskbarPins": [
            {
              "id": "tb-ff",
              "kind": "app",
              "name": "Firefox",
              "subtitle": "Browser",
              "target": "C:\\Program Files\\Mozilla Firefox\\firefox.EXE",
              "altAction": "New Window",
              "accentRgb": "249,115,22",
              "iconKey": "firefox"
            },
            {
              "id": "tb-url",
              "kind": "url",
              "name": "Docs Web",
              "subtitle": "Site",
              "target": "https://example.org",
              "altAction": "Open",
              "accentRgb": "217,119,87",
              "iconKey": "url"
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
              "name": "Game + Discord",
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
              "widgetPreset": "telemetry_processes",
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
        assert_eq!(loaded.taskbar_pins[0].title, "Firefox");
        assert_eq!(loaded.taskbar_pins[0].match_exe.as_deref(), Some("firefox.exe"));

        // URL, URI scheme, and folder stay match_exe = None
        assert_eq!(loaded.taskbar_pins[1].title, "Docs Web");
        assert_eq!(loaded.taskbar_pins[1].match_exe, None);
        assert_eq!(loaded.personal_pins[0].match_exe, None);
        assert_eq!(loaded.personal_links[0].match_exe, None);

        // Legacy GameCombo migrated into polymorphic PinItem kind = "combo"
        assert_eq!(loaded.game_combos.len(), 1);
        assert_eq!(loaded.game_combos[0].kind, "combo");
        assert_eq!(loaded.game_combos[0].title, "Game + Discord");
        assert_eq!(
            loaded.game_combos[0].combo_targets,
            Some(vec![
                "steam://rungameid/570".to_string(),
                r"C:\Discord\Update.exe".to_string()
            ])
        );

        // v2 Spaces migrated cleanly (4 core + 1 custom = 5)
        assert_eq!(loaded.spaces.len(), 5);
        assert_eq!(loaded.spaces[0].id, "personal");
        assert_eq!(loaded.spaces[0].items.len(), 2);
        assert_eq!(loaded.spaces[0].items[0].id, "p-snip");
        assert_eq!(loaded.spaces[0].items[1].id, "lnk-1");
        assert_eq!(loaded.spaces[4].id, "custom-1");
        assert_eq!(loaded.spaces[4].widgets, vec!["pc_health", "top_processes"]);
        assert_eq!(
            loaded.spaces[4].items[0].match_exe.as_deref(),
            Some("code.exe")
        );

        // Custom space pin migrated too
        assert_eq!(
            loaded.custom_spaces[0].items[0].match_exe.as_deref(),
            Some("code.exe")
        );

        // Upgraded v2 file was persisted atomically with zero lingering .tmp files
        assert_eq!(count_tmp_files(&dir), 0);
        let persisted_raw = fs::read_to_string(&path).expect("read migrated file");
        let persisted_val: serde_json::Value =
            serde_json::from_str(&persisted_raw).expect("valid v3 json");
        assert_eq!(persisted_val["schemaVersion"], CURRENT_SCHEMA_VERSION);
        assert_eq!(persisted_val["taskbarPins"][0]["title"], "Firefox");
        assert_eq!(persisted_val["taskbarPins"][0]["matchExe"], "firefox.exe");
        assert_eq!(persisted_val["gameCombos"][0]["kind"], "combo");
        assert_eq!(persisted_val["gameCombos"][0]["title"], "Game + Discord");
        assert_eq!(
            persisted_val["gameCombos"][0]["comboTargets"][0],
            "steam://rungameid/570"
        );
        assert_eq!(persisted_val["spaces"][0]["id"], "personal");
        assert_eq!(persisted_val["spaces"][4]["id"], "custom-1");

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
        assert_eq!(cfg.spaces.len(), 4);

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
            "personal_pins" => &cfg.personal_pins,
            "personal_links" => &cfg.personal_links,
            "work_folders" => &cfg.work_folders,
            "work_apps" => &cfg.work_apps,
            "studio_pins" => &cfg.studio_pins,
            "studio_tools" => &cfg.studio_tools,
            "game_combos" => &cfg.game_combos,
            "game_launchers" => &cfg.game_launchers,
            "taskbar" | "taskbar_pins" => &cfg.taskbar_pins,
            space_id => {
                if let Some(sp) = cfg.spaces.iter().find(|s| s.id == space_id) {
                    return sp.items.as_slice();
                }
                cfg.custom_spaces
                    .iter()
                    .find(|s| s.id == space_id)
                    .map(|s| s.items.as_slice())
                    .expect("space must exist")
            }
        }
    }

    #[test]
    fn uniform_list_crud_reorder_and_edge_cases() {
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

        // Test legacy slices + v2 spaces + taskbar + custom space
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
            ("personal", "file", r"C:\Docs\notes.md"),
            ("work", "app", r"C:\Apps\work-app.exe"),
            ("studio", "url", "https://figma.com"),
            ("games", "app", r"C:\Apps\epic.exe"),
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

        // Games combo routing via "games" still routes combo to game_combos
        assert!(add_pin_to_space(
            &mut cfg,
            "games",
            make_item("gc-alias", "combo", "steam://rungameid/99")
        ));
        assert!(cfg
            .game_combos
            .iter()
            .any(|p| p.id == "gc-alias" && p.kind == "combo" && p.combo_targets.is_some()));
        assert!(remove_pin_from_space(&mut cfg, "games", "gc-alias"));
        assert!(!cfg.game_combos.iter().any(|p| p.id == "gc-alias"));

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

        #[cfg(target_os = "windows")]
        {
            assert!(classify_autostart_delete_error(ERROR_SUCCESS).is_ok());
            assert!(classify_autostart_delete_error(ERROR_FILE_NOT_FOUND).is_ok());
            assert!(classify_autostart_delete_error(ERROR_ACCESS_DENIED).is_err());
        }
    }

    #[cfg(target_os = "windows")]
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
            custom_icon: None,
            custom_color: None,
            tile_size: None,
            section: None,
            run_as_admin: false,
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
        for p in cfg
            .taskbar_pins
            .iter()
            .chain(cfg.spaces.iter().flat_map(|s| s.items.iter()))
        {
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
        assert_eq!(cfg.glass_depth, GLASS_DEPTH_OBSIDIAN);
        assert!(!cfg.clock_show_seconds);
        assert!(!cfg.clock_show_weekday);
        assert!(!cfg.start_remember_space);
        assert!(cfg.hidden_tray_keys.is_empty());
        assert!(cfg.frecency.is_empty());
        assert!(cfg.game_combos.is_empty());
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

    #[cfg(target_os = "windows")]
    #[test]
    fn heal_broken_pins_repairs_spark_cursor_yandex_unity_and_spaces() {
        use crate::apps_indexer::IndexedApp;
        use crate::windows_tracker::AppWindowGroup;

        let make_pin = |id: &str, kind: &str, title: &str, target: &str| PinItem {
            id: id.into(),
            kind: kind.into(),
            title: title.into(),
            subtitle: String::new(),
            target: target.into(),
            args: None,
            alt_action: String::new(),
            alt_label: None,
            accent_rgb: String::new(),
            icon_key: "placeholder".into(),
            icon_data: None,
            match_exe: None,
            match_aumid: None,
            combo_targets: None,
            custom_icon: None,
            custom_color: None,
            tile_size: None,
            section: None,
            run_as_admin: false,
        };

        let mut cfg = TestBarConfig::default();
        cfg.taskbar_pins = vec![
            make_pin("spark", "app", "Spark Mail", ""),
            make_pin("claude-url", "app", "Claude", "https://claude.ai"),
        ];
        cfg.personal_pins = vec![
            make_pin("yandex", "app", "Яндекс", ""),
            make_pin(
                "chatgpt",
                "app",
                "ChatGPT",
                r"shell:AppsFolder\OpenAI.Codex_2p2nqsd0c76g0!App",
            ),
        ];
        cfg.work_apps = vec![make_pin("wa-cur", "app", "Cursor", "")];
        cfg.studio_pins = vec![
            make_pin("unity", "app", "Unity Hub", ""),
            make_pin("cursor-s", "app", "Cursor", r"C:\NonExistent\OldCursor\Cursor.exe"),
        ];
        cfg.spaces[0].items = vec![make_pin("sp2-cur", "app", "Cursor", "")];
        cfg.custom_spaces = vec![CustomSpace {
            id: "sp-1".into(),
            name: "Dev".into(),
            accent_hex: "#818CF8".into(),
            accent_rgb: "129,140,248".into(),
            widget_preset: WIDGET_PRESET_MEDIA_CLIPBOARD.into(),
            items: vec![make_pin("sp-spark", "app", "Spark Mail", "")],
        }];

        let open_groups = vec![AppWindowGroup {
            key: "spark desktop.exe".into(),
            name: "Spark Desktop".into(),
            exe_path: r"C:\Users\alice\AppData\Local\Programs\SparkDesktop\Spark Desktop.exe"
                .into(),
            aumid: None,
            icon_key: None,
            icon_data: None,
            is_active: false,
            windows: vec![],
        }];

        let installed_apps = vec![
            IndexedApp {
                id: "uninstall-spark".into(),
                name: "Uninstall Spark Desktop".into(),
                category: "App".into(),
                target: r"C:\Users\alice\AppData\Local\Programs\SparkDesktop\Uninstall Spark Desktop.exe".into(),
                aumid: None,
                icon_key: None,
                icon_data: None,
            },
            IndexedApp {
                id: "yandex-disk".into(),
                name: "Yandex Disk".into(),
                category: "App".into(),
                target: r"C:\Users\alice\AppData\Local\Programs\Yandex\YandexDisk4\YandexDisk4.exe".into(),
                aumid: None,
                icon_key: None,
                icon_data: None,
            },
            IndexedApp {
                id: "yandex-browser".into(),
                name: "Yandex".into(),
                category: "App".into(),
                target: r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Yandex.lnk".into(),
                aumid: None,
                icon_key: None,
                icon_data: None,
            },
            IndexedApp {
                id: "cursor".into(),
                name: "Cursor".into(),
                category: "App".into(),
                target: r"C:\Program Files\cursor\Cursor.exe".into(),
                aumid: None,
                icon_key: None,
                icon_data: None,
            },
            IndexedApp {
                id: "unity-hub".into(),
                name: "Unity Hub".into(),
                category: "App".into(),
                target: r"C:\Program Files\Unity Hub\Unity Hub.exe".into(),
                aumid: None,
                icon_key: None,
                icon_data: None,
            },
        ];

        assert!(heal_broken_pins(&mut cfg, &installed_apps, &open_groups));

        // 1. Spark Mail healed from running Spark Desktop.exe (not Uninstall!)
        assert_eq!(
            cfg.taskbar_pins[0].target,
            r"C:\Users\alice\AppData\Local\Programs\SparkDesktop\Spark Desktop.exe"
        );
        assert_eq!(
            cfg.taskbar_pins[0].match_exe.as_deref(),
            Some("spark desktop.exe")
        );
        assert!(cfg.taskbar_pins[0].icon_key.is_empty());

        // 2. Valid https URL pin untouched
        assert_eq!(cfg.taskbar_pins[1].target, "https://claude.ai");

        // 3. Yandex healed to Yandex.lnk (NOT Yandex Disk!)
        assert_eq!(
            cfg.personal_pins[0].target,
            r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Yandex.lnk"
        );

        // 4. UWP ChatGPT pin backfilled match_aumid cleanly without altering target
        assert_eq!(
            cfg.personal_pins[1].target,
            r"shell:AppsFolder\OpenAI.Codex_2p2nqsd0c76g0!App"
        );
        assert_eq!(
            cfg.personal_pins[1].match_aumid.as_deref(),
            Some("OpenAI.Codex_2p2nqsd0c76g0!App")
        );

        // 5. Cursor (empty & stale path) + Unity Hub + v2 Space + CustomSpace healed
        assert_eq!(cfg.work_apps[0].target, r"C:\Program Files\cursor\Cursor.exe");
        assert_eq!(cfg.work_apps[0].match_exe.as_deref(), Some("cursor.exe"));
        assert_eq!(
            cfg.studio_pins[0].target,
            r"C:\Program Files\Unity Hub\Unity Hub.exe"
        );
        assert_eq!(
            cfg.studio_pins[1].target,
            r"C:\Program Files\cursor\Cursor.exe"
        );
        assert_eq!(
            cfg.spaces[0].items[0].target,
            r"C:\Program Files\cursor\Cursor.exe"
        );
        assert_eq!(
            cfg.custom_spaces[0].items[0].target,
            r"C:\Users\alice\AppData\Local\Programs\SparkDesktop\Spark Desktop.exe"
        );

        // 6. Second pass is idempotent (returns false)
        assert!(!heal_broken_pins(&mut cfg, &installed_apps, &open_groups));
    }

    #[test]
    fn v1_to_v2_migration_preserves_all_spaces_custom_presets_and_is_idempotent() {
        let v1_json = r##"{
          "schemaVersion": 1,
          "revision": 42,
          "barMode": "dock",
          "personalPins": [
            { "id": "p1", "kind": "app", "title": "App1", "target": "C:\\Apps\\One.exe" }
          ],
          "personalLinks": [
            { "id": "pl1", "kind": "url", "title": "Site", "target": "https://example.com" }
          ],
          "workFolders": [
            { "id": "wf1", "kind": "folder", "title": "Work", "target": "C:\\Work" }
          ],
          "workApps": [
            { "id": "wa1", "kind": "app", "title": "IDE", "target": "C:\\Apps\\Code.exe" }
          ],
          "studioPins": [
            { "id": "sp1", "kind": "app", "title": "Figma", "target": "C:\\Apps\\Figma.exe" }
          ],
          "studioTools": [
            { "id": "st1", "kind": "app", "title": "Taskmgr", "target": "C:\\Windows\\System32\\Taskmgr.exe" }
          ],
          "gameLaunchers": [
            { "id": "gl1", "kind": "app", "title": "Steam", "target": "C:\\Steam\\steam.exe" }
          ],
          "customSpaces": [
            { "id": "cs-media", "name": "Музыка", "accentHex": "#34d399", "widgetPreset": "media_clipboard", "items": [] },
            { "id": "cs-tel", "name": "Монитор", "accentRgb": "56, 189, 248", "widgetPreset": "telemetry_processes", "items": [] },
            { "id": "cs-aud", "name": "Звук", "widgetPreset": "audio_mic", "items": [] },
            { "id": "cs-full", "name": "Сетка", "widgetPreset": "full_grid", "items": [] }
          ]
        }"##;

        let mut cfg: TestBarConfig = serde_json::from_str(v1_json).expect("parse v1");
        assert!(normalize_and_migrate(&mut cfg));
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.revision, 42);
        assert_eq!(cfg.spaces.len(), 8);
        assert!(!cfg.taskbar_widgets.show_weather);
        assert_eq!(cfg.spaces[0].bottom_section_title.as_deref(), Some("Быстрый доступ"));
        assert_eq!(cfg.spaces[0].items[0].section.as_deref(), Some("main"));
        assert_eq!(cfg.spaces[0].items[0].tile_size.as_deref(), Some("hero"));
        assert_eq!(cfg.spaces[0].items[1].section.as_deref(), Some("bottom"));
        assert_eq!(cfg.spaces[1].bottom_section_title.as_deref(), Some("Приложения"));
        assert_eq!(cfg.spaces[1].items[0].section.as_deref(), Some("main"));
        assert_eq!(cfg.spaces[1].items[1].section.as_deref(), Some("bottom"));
        assert_eq!(cfg.spaces[2].bottom_section_title.as_deref(), Some("Инструменты"));
        assert_eq!(cfg.spaces[2].items[0].section.as_deref(), Some("main"));
        assert_eq!(cfg.spaces[2].items[0].tile_size.as_deref(), Some("hero"));
        assert_eq!(cfg.spaces[2].items[1].section.as_deref(), Some("bottom"));
        assert_eq!(cfg.spaces[3].bottom_section_title.as_deref(), Some("Лаунчеры и закреплённое"));
        assert_eq!(cfg.spaces[3].items[0].section.as_deref(), Some("bottom"));

        assert_eq!(cfg.spaces[0].id, "personal");
        assert_eq!(cfg.spaces[0].name, "Личное");
        assert_eq!(cfg.spaces[0].accent_hex, "#818CF8");
        assert_eq!(cfg.spaces[0].accent_rgb, "129,140,248");
        assert_eq!(cfg.spaces[0].layout_kind, "grid");
        assert!(cfg.spaces[0].show_hero_row);
        assert_eq!(cfg.spaces[0].widgets, vec!["media", "audio_mixer", "clipboard"]);
        assert_eq!(
            cfg.spaces[0].items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["p1", "pl1"]
        );

        assert_eq!(cfg.spaces[1].id, "work");
        assert_eq!(cfg.spaces[1].name, "Работа");
        assert_eq!(cfg.spaces[1].accent_hex, "#38BDF8");
        assert_eq!(cfg.spaces[1].accent_rgb, "56,189,248");
        assert!(!cfg.spaces[1].show_hero_row);
        assert_eq!(cfg.spaces[1].widgets, vec!["pc_health", "top_processes"]);
        assert_eq!(
            cfg.spaces[1].items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["wf1", "wa1"]
        );

        assert_eq!(cfg.spaces[2].id, "studio");
        assert_eq!(cfg.spaces[2].name, "Студия");
        assert_eq!(cfg.spaces[2].accent_hex, "#34D399");
        assert_eq!(cfg.spaces[2].accent_rgb, "52,211,153");
        assert!(cfg.spaces[2].show_hero_row);
        assert_eq!(cfg.spaces[2].widgets, vec!["pc_health", "audio_mixer", "top_processes"]);
        assert_eq!(
            cfg.spaces[2].items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["sp1", "st1"]
        );

        assert_eq!(cfg.spaces[3].id, "games");
        assert_eq!(cfg.spaces[3].name, "Игры");
        assert_eq!(cfg.spaces[3].accent_hex, "#FB7185");
        assert_eq!(cfg.spaces[3].accent_rgb, "251,113,133");
        assert_eq!(cfg.spaces[3].layout_kind, "games");
        assert!(!cfg.spaces[3].show_hero_row);
        assert_eq!(cfg.spaces[3].widgets, vec!["audio_mixer", "game_combos", "pc_health"]);
        assert_eq!(
            cfg.spaces[3].items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["gl1"]
        );

        assert_eq!(cfg.spaces[4].widgets, vec!["media", "audio_mixer", "clipboard"]);
        assert_eq!(cfg.spaces[5].accent_hex, "#38BDF8");
        assert_eq!(cfg.spaces[5].widgets, vec!["pc_health", "top_processes"]);
        assert_eq!(cfg.spaces[6].widgets, vec!["audio_mixer", "media"]);
        assert!(cfg.spaces[7].widgets.is_empty());

        assert!(!normalize_and_migrate(&mut cfg));

        // Blank/empty spaces never remain empty after normalize_and_migrate
        let mut blank: TestBarConfig = serde_json::from_str("{}").expect("empty object");
        assert!(normalize_and_migrate(&mut blank));
        assert_eq!(blank.spaces.len(), DEFAULT_SPACES_COUNT);
    }

    #[test]
    fn space_crud_templates_last_space_guard_cap_and_reorder() {
        let mut cfg = TestBarConfig::default();
        assert_eq!(cfg.spaces.len(), DEFAULT_SPACES_COUNT);

        // Create from templates with deduped widgets & unique slug id
        let created_work = create_space_from_template(
            &mut cfg,
            "  Dev Проекты  ".to_string(),
            "work",
            None,
            None,
            None,
        )
        .expect("create work space");
        assert_eq!(created_work.id, "dev-proekty");
        assert_eq!(created_work.accent_hex, "#38BDF8");
        assert_eq!(created_work.widgets, vec!["pc_health", "top_processes"]);

        let created_dup = create_space_from_template(
            &mut cfg,
            "Dev Проекты".to_string(),
            "games",
            Some("#f97316".to_string()),
            None,
            Some(vec![
                "weather".to_string(),
                "WEATHER".to_string(),
                "batteries".to_string(),
            ]),
        )
        .expect("create second space with unique slug");
        assert_eq!(created_dup.id, "dev-proekty-2");
        assert_eq!(created_dup.layout_kind, "games");
        assert_eq!(created_dup.accent_hex, "#F97316");
        assert_eq!(created_dup.accent_rgb, "249,115,22");
        assert_eq!(created_dup.widgets, vec!["weather", "batteries"]);

        assert!(create_space_from_template(&mut cfg, "Bad".into(), "unknown", None, None, None).is_err());
        assert!(create_space_from_template(&mut cfg, "   ".into(), "blank", None, None, None).is_err());
        assert!(create_space_from_template(
            &mut cfg,
            "BadW".into(),
            "blank",
            None,
            None,
            Some(vec!["invalid_widget".to_string()])
        )
        .is_err());

        // Update space config + set_space_widgets_by_id
        assert!(update_space_config(
            &mut cfg,
            "dev-proekty",
            Some("Разработка".to_string()),
            Some("#a78bfa".to_string()),
            None,
            Some("games".to_string()),
            Some(true),
            Some(vec!["weather".to_string(), "media".to_string()]),
            None,
        )
        .unwrap());
        let sp = cfg.spaces.iter().find(|s| s.id == "dev-proekty").unwrap();
        assert_eq!(sp.name, "Разработка");
        assert_eq!(sp.accent_hex, "#A78BFA");
        assert_eq!(sp.layout_kind, "games");
        assert!(sp.show_hero_row);
        assert_eq!(sp.widgets, vec!["weather", "media"]);

        assert!(set_space_widgets_by_id(
            &mut cfg,
            "dev-proekty",
            vec!["batteries".into(), "batteries".into(), "clipboard".into()],
        )
        .unwrap());
        assert!(!set_space_widgets_by_id(
            &mut cfg,
            "dev-proekty",
            vec!["batteries".into(), "clipboard".into()],
        )
        .unwrap());
        assert!(set_space_widgets_by_id(&mut cfg, "dev-proekty", vec!["bogus".into()]).is_err());

        // Reorder spaces
        let mut ids: Vec<String> = cfg.spaces.iter().map(|s| s.id.clone()).collect();
        assert!(!reorder_spaces_by_ids(&mut cfg, ids.clone()).unwrap());
        ids.reverse();
        assert!(reorder_spaces_by_ids(&mut cfg, ids.clone()).unwrap());
        assert_eq!(
            cfg.spaces.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ids.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert!(reorder_spaces_by_ids(&mut cfg, vec!["personal".into()]).is_err());
        let mut dup_ids = ids.clone();
        dup_ids[1] = dup_ids[0].clone();
        assert!(reorder_spaces_by_ids(&mut cfg, dup_ids).is_err());

        // Max spaces cap
        while cfg.spaces.len() < MAX_SPACES {
            let extra_name = format!("Extra {}", cfg.spaces.len());
            create_space_from_template(
                &mut cfg,
                extra_name,
                "blank",
                None,
                None,
                None,
            )
            .expect("within cap");
        }
        assert!(create_space_from_template(&mut cfg, "Overflow".into(), "blank", None, None, None).is_err());

        // Delete down to 1 space, then last-space delete guard rejects deletion
        while cfg.spaces.len() > 1 {
            let last_id = cfg.spaces.last().unwrap().id.clone();
            assert!(delete_space_by_id(&mut cfg, &last_id).unwrap());
        }
        let only_id = cfg.spaces[0].id.clone();
        assert!(delete_space_by_id(&mut cfg, &only_id).is_err());
        assert_eq!(cfg.spaces.len(), 1);
    }

    #[test]
    fn mixed_items_reorder_and_pin_v2_custom_icon_color_roundtrip() {
        let mut cfg = TestBarConfig::default();
        let space = create_space_from_template(
            &mut cfg,
            "Mixed".to_string(),
            "blank",
            None,
            None,
            None,
        )
        .expect("create blank");

        assert!(add_pin_to_space(
            &mut cfg,
            &space.id,
            pin("m-app", "app", "Notepad", "", r"C:\Windows\System32\notepad.exe".into(), "", "56,189,248", "notepad")
        ));
        assert!(add_pin_to_space(
            &mut cfg,
            &space.id,
            pin("m-dir", "folder", "Temp", "", r"C:\Temp".into(), "", "245,158,11", "folder")
        ));
        assert!(add_pin_to_space(
            &mut cfg,
            &space.id,
            pin("m-url", "url", "Docs", "", "https://example.com".into(), "", "129,140,248", "url")
        ));

        // Mixed grid reorder: move url (idx 2) before app (idx 0)
        assert!(reorder_pins_in_space(&mut cfg, &space.id, 2, 0));
        let sp = cfg.spaces.iter().find(|s| s.id == space.id).unwrap();
        assert_eq!(
            sp.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec!["m-url", "m-app", "m-dir"]
        );

        // update_pin_v2 preserves safe icon_data when custom_icon == "custom_image"
        assert!(update_pin_v2(
            &mut cfg,
            &space.id,
            "m-app",
            Some("Editor".to_string()),
            None,
            None,
            None,
            None,
            Some(Some("  custom_image  ".to_string())),
            Some(Some(" #34d399 ".to_string())),
            Some(Some("data:image/png;base64,AAA".to_string())),
            None,
            None,
            None,
        )
        .unwrap());
        let edited = cfg
            .spaces
            .iter()
            .find(|s| s.id == space.id)
            .unwrap()
            .items
            .iter()
            .find(|i| i.id == "m-app")
            .unwrap();
        assert_eq!(edited.title, "Editor");
        assert_eq!(edited.custom_icon.as_deref(), Some("custom_image"));
        assert_eq!(edited.custom_color.as_deref(), Some("#34D399"));
        assert_eq!(edited.icon_data.as_deref(), Some("data:image/png;base64,AAA"));

        // Roundtrip JSON preserves customIcon / customColor / custom_image iconData
        let json = export_config_json(&cfg).expect("export");
        let back = import_config_json(&json).expect("import");
        let edited_back = back
            .spaces
            .iter()
            .find(|s| s.id == space.id)
            .unwrap()
            .items
            .iter()
            .find(|i| i.id == "m-app")
            .unwrap();
        assert_eq!(edited_back.custom_icon.as_deref(), Some("custom_image"));
        assert_eq!(edited_back.custom_color.as_deref(), Some("#34D399"));
        assert_eq!(
            edited_back.icon_data.as_deref(),
            Some("data:image/png;base64,AAA")
        );

        // Unsafe SVG or oversized >128KB icon_data is rejected even for custom_image
        assert!(update_pin_v2(
            &mut cfg,
            &space.id,
            "m-app",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("data:image/svg+xml;base64,PHN2Zz4=".to_string())),
            None,
            None,
            None,
        )
        .unwrap());
        assert_eq!(
            cfg.spaces
                .iter()
                .find(|s| s.id == space.id)
                .unwrap()
                .items
                .iter()
                .find(|i| i.id == "m-app")
                .unwrap()
                .icon_data,
            None
        );

        let huge = format!("data:image/png;base64,{}", "A".repeat(MAX_CUSTOM_ICON_DATA_BYTES + 1));
        assert!(!update_pin_v2(
            &mut cfg,
            &space.id,
            "m-app",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some(huge)),
            None,
            None,
            None,
        )
        .unwrap());

        // Restore valid icon_data, then switch custom_icon to "terminal" -> clears icon_data automatically
        assert!(update_pin_v2(
            &mut cfg,
            &space.id,
            "m-app",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("data:image/png;base64,BBB".to_string())),
            None,
            None,
            None,
        )
        .unwrap());
        assert!(update_pin_v2(
            &mut cfg,
            &space.id,
            "m-app",
            None,
            None,
            None,
            None,
            None,
            Some(Some("  terminal  ".to_string())),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap());

        // Leave custom_icon untouched (None), clear custom_color (Some(None))
        assert!(update_pin_v2(
            &mut cfg,
            &space.id,
            "m-app",
            None,
            None,
            None,
            None,
            None,
            None,
            Some(None),
            None,
            None,
            None,
            None,
        )
        .unwrap());
        let cleared = cfg
            .spaces
            .iter()
            .find(|s| s.id == space.id)
            .unwrap()
            .items
            .iter()
            .find(|i| i.id == "m-app")
            .unwrap();
        assert_eq!(cleared.custom_icon.as_deref(), Some("terminal"));
        assert_eq!(cleared.custom_color, None);
        assert_eq!(cleared.icon_data, None);
        let serialized_item = serde_json::to_value(cleared).unwrap();
        assert!(serialized_item.get("customColor").is_none());
        assert_eq!(serialized_item["customIcon"], "terminal");
    }

    #[test]
    fn taskbar_widgets_validation_normalization_and_persistence() {
        let mut cfg = TestBarConfig::default();
        assert!(!cfg.taskbar_widgets.show_weather);
        assert!(!cfg.taskbar_widgets.show_batteries);
        assert!(!cfg.taskbar_widgets.show_telemetry);
        assert_eq!(cfg.taskbar_widgets.weather_units, "c");

        assert!(set_taskbar_widgets(
            &mut cfg,
            TaskbarWidgetSettings {
                show_weather: true,
                show_batteries: true,
                show_telemetry: true,
                weather_city: "  Москва  ".to_string(),
                weather_lat: Some(55.7558),
                weather_lon: Some(37.6173),
                weather_units: "F".to_string(),
                ..Default::default()
            }
        )
        .unwrap());
        assert_eq!(cfg.taskbar_widgets.weather_city, "Москва");
        assert_eq!(cfg.taskbar_widgets.weather_units, "f");
        let same_widgets = cfg.taskbar_widgets.clone();
        assert!(!set_taskbar_widgets(&mut cfg, same_widgets).unwrap());

        assert!(set_taskbar_widgets(
            &mut cfg,
            TaskbarWidgetSettings {
                weather_units: "k".to_string(),
                ..Default::default()
            }
        )
        .is_err());
        assert!(set_taskbar_widgets(
            &mut cfg,
            TaskbarWidgetSettings {
                weather_lat: Some(55.0),
                weather_lon: None,
                ..Default::default()
            }
        )
        .is_err());

        // Dirty JSON normalizes units & resets unpaired/out-of-range coords
        let dirty_json = r#"{"taskbarWidgets":{"showWeather":false,"weatherCity":"  Berlin ","weatherLat":120.0,"weatherLon":13.4,"weatherUnits":"kelvin"}}"#;
        let parsed = import_config_json(dirty_json).expect("import dirty taskbarWidgets");
        assert!(!parsed.taskbar_widgets.show_weather);
        assert_eq!(parsed.taskbar_widgets.weather_city, "Berlin");
        assert_eq!(parsed.taskbar_widgets.weather_units, "c");
        assert_eq!(parsed.taskbar_widgets.weather_lat, None);
        assert_eq!(parsed.taskbar_widgets.weather_lon, None);
    }

    #[test]
    fn browser_command_parser_and_clean_defaults_have_zero_personal_hardcodes() {
        assert_eq!(
            parse_browser_command_exe(
                r#""C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe" --single-argument %1"#
            ),
            Some(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".to_string())
        );
        assert_eq!(
            parse_browser_command_exe(r"C:\Program Files\Mozilla Firefox\firefox.exe -osint -url %1"),
            Some(r"C:\Program Files\Mozilla Firefox\firefox.exe".to_string())
        );
        assert_eq!(
            parse_browser_command_exe(r"C:\Windows\System32\LaunchWinApp.exe %1"),
            None
        );

        let src = include_str!("config.rs");
        let detect_start = src
            .find("fn detect_app_target(")
            .expect("detect_app_target must exist");
        let detect_end = src[detect_start..]
            .find("\nfn strip_verbatim_prefix(")
            .map(|rel| detect_start + rel)
            .expect("end of detect_app_target");
        let defaults_start = src
            .find("fn default_modular_spaces()")
            .expect("default_modular_spaces must exist");
        let defaults_end = src[defaults_start..]
            .find("\npub fn config_path()")
            .map(|rel| defaults_start + rel)
            .expect("end of TestBarConfig::default");
        let source_corpus = format!(
            "{}\n{}",
            &src[detect_start..detect_end],
            &src[defaults_start..defaults_end]
        )
        .to_lowercase();

        let banned_source = [
            "loki2",
            r"d:\steam",
            "antigravity",
            "spark",
            "zen",
            "wiztree",
            "poe",
            "dota",
            "\"testbar\"",
            "второй мозг",
            "wf-testbar",
        ];
        for needle in banned_source {
            assert!(
                !source_corpus.contains(needle),
                "detect_app_target / default spaces must not hardcode '{needle}'"
            );
        }

        let cfg = TestBarConfig::default();
        assert!(cfg.game_combos.is_empty());
        let banned_runtime_labels = [
            "loki2",
            r"d:\steam",
            "antigravity",
            "spark",
            "zen browser",
            "wiztree",
            "poe",
            "dota",
            "testbar",
            "второй мозг",
            "wf-testbar",
        ];
        let mut label_corpus = String::new();
        for p in cfg
            .taskbar_pins
            .iter()
            .chain(cfg.spaces.iter().flat_map(|s| s.items.iter()))
            .chain(cfg.game_combos.iter())
        {
            label_corpus.push_str(&p.id);
            label_corpus.push('|');
            label_corpus.push_str(&p.title);
            label_corpus.push('|');
            label_corpus.push_str(&p.subtitle);
            label_corpus.push('|');
        }
        let lower_labels = label_corpus.to_lowercase();
        for needle in banned_runtime_labels {
            assert!(
                !lower_labels.contains(needle),
                "TestBarConfig::default() pin metadata must not contain personal hardcode '{needle}', got: {lower_labels}"
            );
        }
    }

    #[test]
    fn normalize_pin_auto_heals_system_settings_and_uwp_host_targets() {
        // 1. SystemSettings.exe -> shell:AppsFolder\windows.immersivecontrolpanel_... + match_aumid
        let mut settings_pin = PinItem {
            id: "set-1".into(),
            kind: "app".into(),
            title: "Параметры".into(),
            target: r"C:\Windows\ImmersiveControlPanel\SystemSettings.exe".into(),
            ..Default::default()
        };
        assert!(normalize_pin(&mut settings_pin, false));
        assert_eq!(
            settings_pin.target,
            r"shell:AppsFolder\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"
        );
        assert_eq!(
            settings_pin.match_aumid.as_deref(),
            Some("windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel")
        );
        assert!(!normalize_pin(&mut settings_pin, false));

        // 2. ApplicationFrameHost.exe with match_aumid -> shell:AppsFolder\<aumid>
        let mut afh_pin = PinItem {
            id: "calc-1".into(),
            kind: "app".into(),
            title: "Калькулятор".into(),
            target: r"C:/Windows/System32/ApplicationFrameHost.exe".into(),
            match_exe: Some("ApplicationFrameHost.exe".into()),
            match_aumid: Some("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into()),
            ..Default::default()
        };
        assert!(normalize_pin(&mut afh_pin, false));
        assert_eq!(
            afh_pin.target,
            r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        );
        assert_eq!(afh_pin.match_exe, None);
        assert!(!normalize_pin(&mut afh_pin, false));

        // 3. \WindowsApps\ exe with match_aumid -> shell:AppsFolder\<aumid>
        let mut winapps_pin = PinItem {
            id: "wt-1".into(),
            kind: "app".into(),
            title: "Terminal".into(),
            target: r"C:\Program Files\WindowsApps\Microsoft.WindowsTerminal_1.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe".into(),
            match_aumid: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
            ..Default::default()
        };
        assert!(normalize_pin(&mut winapps_pin, false));
        assert_eq!(
            winapps_pin.target,
            r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"
        );
        assert!(!normalize_pin(&mut winapps_pin, false));
    }

    #[test]
    fn v2_to_v3_migration_preserves_user_pins_and_restores_sections_and_hero_tiles() {
        let v2_json = r##"{
          "schemaVersion": 2,
          "revision": 19,
          "barMode": "dock",
          "taskbarWidgets": {
            "showWeather": true,
            "showBatteries": true,
            "showTelemetry": false,
            "weatherCity": "Москва",
            "weatherUnits": "c"
          },
          "personalLinks": [
            { "id": "pl-legacy", "kind": "url", "title": "Docs", "target": "https://example.com" }
          ],
          "spaces": [
            {
              "id": "personal",
              "name": "Личное",
              "accentHex": "#818CF8",
              "accentRgb": "129,140,248",
              "layoutKind": "grid",
              "showHeroRow": true,
              "widgets": ["media"],
              "items": [
                { "id": "p-a1", "kind": "app", "title": "Browser", "target": "C:\\Apps\\Browser.exe" },
                { "id": "p-a2", "kind": "app", "title": "Explorer", "target": "C:\\Windows\\explorer.exe" },
                { "id": "p-a3", "kind": "app", "title": "Telegram", "target": "C:\\Apps\\Telegram.exe" },
                { "id": "pl-legacy", "kind": "url", "title": "Docs", "target": "https://example.com" }
              ]
            },
            {
              "id": "work",
              "name": "Работа",
              "accentHex": "#38BDF8",
              "accentRgb": "56,189,248",
              "layoutKind": "grid",
              "showHeroRow": false,
              "widgets": ["pc_health"],
              "items": [
                { "id": "w-f1", "kind": "folder", "title": "Projects", "target": "C:\\Projects" },
                { "id": "w-a1", "kind": "app", "title": "Code", "target": "C:\\Apps\\Code.exe" },
                { "id": "w-a2", "kind": "app", "title": "Terminal", "target": "C:\\Windows\\System32\\cmd.exe" }
              ]
            },
            {
              "id": "studio",
              "name": "Студия",
              "accentHex": "#34D399",
              "accentRgb": "52,211,153",
              "layoutKind": "grid",
              "showHeroRow": true,
              "widgets": ["pc_health"],
              "items": [
                { "id": "s-1", "kind": "app", "title": "Figma", "target": "C:\\Apps\\Figma.exe" },
                { "id": "s-2", "kind": "app", "title": "OBS", "target": "C:\\Apps\\obs64.exe" },
                { "id": "s-3", "kind": "app", "title": "VLC", "target": "C:\\Apps\\vlc.exe" }
              ]
            },
            {
              "id": "games",
              "name": "Игры",
              "accentHex": "#FB7185",
              "accentRgb": "251,113,133",
              "layoutKind": "games",
              "showHeroRow": false,
              "widgets": ["game_combos"],
              "items": [
                { "id": "g-1", "kind": "app", "title": "Steam", "target": "C:\\Steam\\steam.exe" }
              ]
            },
            {
              "id": "custom-hero",
              "name": "Медиа",
              "accentHex": "#A78BFA",
              "accentRgb": "167,139,250",
              "layoutKind": "grid",
              "showHeroRow": true,
              "widgets": ["media"],
              "items": [
                { "id": "c-1", "kind": "app", "title": "App1", "target": "C:\\Apps\\One.exe" },
                { "id": "c-2", "kind": "app", "title": "App2", "target": "C:\\Apps\\Two.exe" },
                { "id": "c-3", "kind": "app", "title": "App3", "target": "C:\\Apps\\Three.exe" }
              ]
            }
          ]
        }"##;

        let mut cfg: TestBarConfig = serde_json::from_str(v2_json).expect("parse v2");
        assert!(normalize_and_migrate(&mut cfg));
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.revision, 19);
        assert!(!cfg.taskbar_widgets.show_weather);
        assert!(cfg.taskbar_widgets.show_batteries);

        // Personal: all 4 pins kept, legacy personalLinks item -> bottom, first 2 main -> hero
        let personal = &cfg.spaces[0];
        assert_eq!(personal.items.len(), 4);
        assert_eq!(personal.bottom_section_title.as_deref(), Some("Быстрый доступ"));
        assert_eq!(personal.items[0].section.as_deref(), Some("main"));
        assert_eq!(personal.items[0].tile_size.as_deref(), Some("hero"));
        assert_eq!(personal.items[1].section.as_deref(), Some("main"));
        assert_eq!(personal.items[1].tile_size.as_deref(), Some("hero"));
        assert_eq!(personal.items[2].section.as_deref(), Some("main"));
        assert_eq!(personal.items[2].tile_size, None);
        assert_eq!(personal.items[3].section.as_deref(), Some("bottom"));

        // Work with empty workApps: folder -> main, apps -> bottom
        let work = &cfg.spaces[1];
        assert_eq!(work.items.len(), 3);
        assert_eq!(work.bottom_section_title.as_deref(), Some("Приложения"));
        assert_eq!(work.items[0].section.as_deref(), Some("main"));
        assert_eq!(work.items[1].section.as_deref(), Some("bottom"));
        assert_eq!(work.items[2].section.as_deref(), Some("bottom"));

        // Studio: first 2 main -> hero
        let studio = &cfg.spaces[2];
        assert_eq!(studio.bottom_section_title.as_deref(), Some("Инструменты"));
        assert_eq!(studio.items[0].tile_size.as_deref(), Some("hero"));
        assert_eq!(studio.items[1].tile_size.as_deref(), Some("hero"));
        assert_eq!(studio.items[2].tile_size, None);
        assert_eq!(studio.items[2].section.as_deref(), Some("main"));

        // Games: all -> bottom
        let games = &cfg.spaces[3];
        assert_eq!(games.bottom_section_title.as_deref(), Some("Лаунчеры и закреплённое"));
        assert_eq!(games.items[0].section.as_deref(), Some("bottom"));

        // Custom with showHeroRow: true -> main + first 2 hero
        let custom = &cfg.spaces[4];
        assert_eq!(custom.bottom_section_title.as_deref(), Some("Закреплённое"));
        assert_eq!(custom.items[0].section.as_deref(), Some("main"));
        assert_eq!(custom.items[0].tile_size.as_deref(), Some("hero"));
        assert_eq!(custom.items[1].tile_size.as_deref(), Some("hero"));
        assert_eq!(custom.items[2].tile_size, None);

        // Second pass is idempotent AND user re-enabling showWeather in v3 is preserved
        assert!(!normalize_and_migrate(&mut cfg));
        cfg.taskbar_widgets.show_weather = true;
        assert!(!normalize_and_migrate(&mut cfg));
        assert!(cfg.taskbar_widgets.show_weather);

        // v2 personal with empty personalLinks splits trailing folders into bottom
        let v2_mixed_personal = r##"{
          "schemaVersion": 2,
          "spaces": [
            {
              "id": "personal",
              "name": "Личное",
              "accentHex": "#818CF8",
              "accentRgb": "129,140,248",
              "layoutKind": "grid",
              "showHeroRow": true,
              "items": [
                { "id": "a1", "kind": "app", "title": "App", "target": "C:\\Apps\\One.exe" },
                { "id": "f1", "kind": "folder", "title": "Downloads", "target": "C:\\Downloads" },
                { "id": "f2", "kind": "folder", "title": "Docs", "target": "C:\\Docs" }
              ]
            }
          ]
        }"##;
        let mut mixed_cfg: TestBarConfig = serde_json::from_str(v2_mixed_personal).expect("parse mixed v2");
        assert!(normalize_and_migrate(&mut mixed_cfg));
        assert_eq!(mixed_cfg.spaces[0].items[0].section.as_deref(), Some("main"));
        assert_eq!(mixed_cfg.spaces[0].items[0].tile_size.as_deref(), Some("hero"));
        assert_eq!(mixed_cfg.spaces[0].items[1].section.as_deref(), Some("bottom"));
        assert_eq!(mixed_cfg.spaces[0].items[2].section.as_deref(), Some("bottom"));
    }

    #[test]
    fn v3_pin_tile_size_section_admin_and_space_bottom_title_roundtrip() {
        let mut cfg = TestBarConfig::default();
        assert_eq!(
            cfg.spaces[0].bottom_section_title.as_deref(),
            Some("Быстрый доступ")
        );
        let first_id = cfg.spaces[0].items[0].id.clone();

        assert!(update_pin_v2(
            &mut cfg,
            "personal",
            &first_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("compact".to_string())),
            Some(Some("bottom".to_string())),
            Some(true),
        )
        .unwrap());

        assert!(update_pin_v2(
            &mut cfg,
            "personal",
            &first_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("giant".to_string())),
            None,
            None,
        )
        .is_err());

        assert!(update_pin_v2(
            &mut cfg,
            "personal",
            &first_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("sidebar".to_string())),
            None,
        )
        .is_err());

        assert!(update_space_config(
            &mut cfg,
            "personal",
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("  Свои ссылки  ".to_string())),
        )
        .unwrap());

        let exported = export_config_json(&cfg).expect("export v3");
        let back = import_config_json(&exported).expect("import v3");
        let item_back = back.spaces[0]
            .items
            .iter()
            .find(|i| i.id == first_id)
            .unwrap();
        assert_eq!(item_back.tile_size.as_deref(), Some("compact"));
        assert_eq!(item_back.section.as_deref(), Some("bottom"));
        assert!(item_back.run_as_admin);
        assert_eq!(
            back.spaces[0].bottom_section_title.as_deref(),
            Some("Свои ссылки")
        );

        // Clear tile_size, section, bottom_section_title, and run_as_admin
        assert!(update_pin_v2(
            &mut cfg,
            "personal",
            &first_id,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(None),
            Some(None),
            Some(false),
        )
        .unwrap());
        assert!(update_space_config(
            &mut cfg,
            "personal",
            None,
            None,
            None,
            None,
            None,
            None,
            Some(None),
        )
        .unwrap());
        let cleared = cfg.spaces[0]
            .items
            .iter()
            .find(|i| i.id == first_id)
            .unwrap();
        assert_eq!(cleared.tile_size, None);
        assert_eq!(cleared.section, None);
        assert!(!cleared.run_as_admin);
        assert_eq!(cfg.spaces[0].bottom_section_title, None);
    }

    #[test]
    fn reorder_pins_by_id_handles_intra_section_and_cross_section_moves() {
        let mut cfg = TestBarConfig::default();
        let space = create_space_from_template(
            &mut cfg,
            "Sections".to_string(),
            "blank",
            None,
            None,
            None,
        )
        .unwrap();

        for (id, sec) in [
            ("m1", "main"),
            ("m2", "main"),
            ("b1", "bottom"),
            ("b2", "bottom"),
        ] {
            let mut item = pin(
                id,
                "app",
                id,
                "",
                format!(r"C:\Apps\{id}.exe"),
                "",
                "56,189,248",
                "app",
            );
            item.section = Some(sec.to_string());
            assert!(add_pin_to_space(&mut cfg, &space.id, item));
        }

        // Intra-section move: m2 -> m1
        assert!(reorder_pins_by_id(
            &mut cfg,
            &space.id,
            "m2",
            Some("m1"),
            Some("main"),
        ));
        let ids = |cfg: &TestBarConfig| -> Vec<(String, Option<String>)> {
            cfg.spaces
                .iter()
                .find(|s| s.id == space.id)
                .unwrap()
                .items
                .iter()
                .map(|i| (i.id.clone(), i.section.clone()))
                .collect()
        };
        assert_eq!(
            ids(&cfg),
            vec![
                ("m2".into(), Some("main".into())),
                ("m1".into(), Some("main".into())),
                ("b1".into(), Some("bottom".into())),
                ("b2".into(), Some("bottom".into())),
            ]
        );

        // Cross-section Main -> Bottom onto specific target b1
        assert!(reorder_pins_by_id(
            &mut cfg,
            &space.id,
            "m2",
            Some("b1"),
            Some("bottom"),
        ));
        assert_eq!(
            ids(&cfg),
            vec![
                ("m1".into(), Some("main".into())),
                ("b1".into(), Some("bottom".into())),
                ("m2".into(), Some("bottom".into())),
                ("b2".into(), Some("bottom".into())),
            ]
        );

        // Cross-section Bottom -> Main dropzone (to_id = None)
        assert!(reorder_pins_by_id(
            &mut cfg,
            &space.id,
            "b2",
            None,
            Some("main"),
        ));
        assert_eq!(
            ids(&cfg),
            vec![
                ("m1".into(), Some("main".into())),
                ("b2".into(), Some("main".into())),
                ("b1".into(), Some("bottom".into())),
                ("m2".into(), Some("bottom".into())),
            ]
        );

        // No-op when already at end of main
        assert!(!reorder_pins_by_id(
            &mut cfg,
            &space.id,
            "b2",
            None,
            Some("main"),
        ));
        // Invalid section / missing pin / missing space return false
        assert!(!reorder_pins_by_id(
            &mut cfg,
            &space.id,
            "m1",
            None,
            Some("invalid"),
        ));
        assert!(!reorder_pins_by_id(
            &mut cfg,
            &space.id,
            "missing",
            None,
            Some("bottom"),
        ));
        assert!(!reorder_pins_by_id(
            &mut cfg,
            "no-such-space",
            "m1",
            None,
            Some("bottom"),
        ));
    }

    #[test]
    fn hidden_battery_devices_hide_restore_restore_all_and_normalize() {
        let mut cfg = TestBarConfig::default();
        assert!(cfg.hidden_battery_ids.is_empty());

        assert!(hide_battery_device(&mut cfg, "  Sony:0CE6 "));
        assert!(!hide_battery_device(&mut cfg, "sony:0ce6"));
        assert!(!hide_battery_device(&mut cfg, "   "));
        assert!(hide_battery_device(&mut cfg, "Razer:00CC"));
        assert_eq!(cfg.hidden_battery_ids, vec!["sony:0ce6", "razer:00cc"]);

        assert!(restore_battery_device(&mut cfg, "SONY:0ce6"));
        assert!(!restore_battery_device(&mut cfg, "sony:0ce6"));
        assert_eq!(cfg.hidden_battery_ids, vec!["razer:00cc"]);

        assert!(restore_all_battery_devices(&mut cfg));
        assert!(!restore_all_battery_devices(&mut cfg));
        assert!(cfg.hidden_battery_ids.is_empty());

        // normalize_and_migrate deduplicates and lowercases hidden_battery_ids
        cfg.hidden_battery_ids = vec![
            "  BT:AABB  ".to_string(),
            "".to_string(),
            "bt:aabb".to_string(),
            "Sony:0DF2".to_string(),
        ];
        assert!(normalize_and_migrate(&mut cfg));
        assert_eq!(cfg.hidden_battery_ids, vec!["bt:aabb", "sony:0df2"]);
        assert!(!normalize_and_migrate(&mut cfg));
    }

    #[test]
    fn v3_to_v4_migration_preserves_pins_and_populates_header_and_taskbar_widgets() {
        let mut cfg = TestBarConfig::default();
        cfg.schema_version = 3;
        cfg.taskbar_widgets.show_weather = true;
        cfg.taskbar_widgets.show_telemetry = true;
        cfg.taskbar_widgets.left_widgets.clear();
        for sp in &mut cfg.spaces {
            sp.header_widgets.clear();
            sp.items.push(pin(
                &format!("keep-{}", sp.id),
                "app",
                "Keep",
                "Sub",
                "notepad.exe".to_string(),
                "",
                "129,140,248",
                "app",
            ));
        }
        // Put weather into personal sidebar to verify mutual exclusivity during seeding
        cfg.spaces[0].widgets.push("weather".to_string());

        assert!(normalize_and_migrate(&mut cfg));
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(cfg.taskbar_widgets.show_weather);
        assert_eq!(
            cfg.taskbar_widgets.left_widgets,
            vec!["weather".to_string(), "telemetry".to_string()]
        );
        assert_eq!(cfg.taskbar_widgets.widgets_position, "left");
        assert!(cfg.spaces.iter().all(|s| s.items.iter().any(|i| i.id.starts_with("keep-"))));

        // Personal had weather in right rail so header got only batteries
        assert_eq!(
            cfg.spaces[0]
                .header_widgets
                .iter()
                .map(|w| w.id.as_str())
                .collect::<Vec<_>>(),
            vec!["batteries"]
        );
        // Work got both batteries and weather
        assert_eq!(
            cfg.spaces[1]
                .header_widgets
                .iter()
                .map(|w| w.id.as_str())
                .collect::<Vec<_>>(),
            vec!["batteries", "weather"]
        );
        assert!(!normalize_and_migrate(&mut cfg));
    }

    #[test]
    fn small_tile_dual_widget_exclusivity_themes_clock_and_notes_work() {
        let mut cfg = TestBarConfig::default();
        let space_id = cfg.spaces[0].id.clone();

        // Small tile size accepted
        assert_eq!(validate_tile_size("small").unwrap(), "small");
        cfg.spaces[0].items.push(pin(
            "small-one",
            "app",
            "Mini",
            "",
            "notepad.exe".to_string(),
            "",
            "129,140,248",
            "app",
        ));
        assert!(update_pin_v2(
            &mut cfg,
            &space_id,
            "small-one",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Some("small".into())),
            None,
            None,
        )
        .unwrap());
        assert_eq!(
            cfg.spaces[0]
                .items
                .iter()
                .find(|p| p.id == "small-one")
                .and_then(|p| p.tile_size.as_deref()),
            Some("small")
        );

        // Dual widget mutual exclusivity: media in side strips media from header
        assert!(set_space_widget_layout_in_config(
            &mut cfg,
            &space_id,
            vec!["media".into(), "notes".into()],
            vec![
                SpaceHeaderWidget {
                    id: "media".into(),
                    side: "right".into(),
                },
                SpaceHeaderWidget {
                    id: "timer".into(),
                    side: "left".into(),
                },
                SpaceHeaderWidget {
                    id: "network".into(),
                    side: "right".into(),
                },
            ],
        )
        .unwrap());
        assert_eq!(cfg.spaces[0].widgets, vec!["media", "notes"]);
        assert_eq!(
            cfg.spaces[0].header_widgets,
            vec![
                SpaceHeaderWidget {
                    id: "timer".into(),
                    side: "left".into(),
                },
                SpaceHeaderWidget {
                    id: "network".into(),
                    side: "right".into(),
                },
            ]
        );

        // Theme, solid background, no-gradient, no-glow, clock formats, reset_visuals
        assert!(set_appearance_settings_ext(
            &mut cfg,
            AppearancePatch {
                glass_depth: Some("solid".into()),
                theme_preset: Some("carbon".into()),
                accent_mode: Some("custom".into()),
                accent_hex: Some("#34D399".into()),
                surface_gradients: Some(false),
                hover_glow: Some(false),
                clock_date_format: Some("none".into()),
                clock_layout: Some("single".into()),
                default_tile_size: Some("small".into()),
                ..Default::default()
            }
        )
        .unwrap());
        assert_eq!(cfg.glass_depth, "solid");
        assert_eq!(cfg.theme_preset, "carbon");
        assert_eq!(cfg.accent_mode, "custom");
        assert!(!cfg.surface_gradients);
        assert!(!cfg.hover_glow);
        assert_eq!(cfg.clock_date_format, "none");
        assert_eq!(cfg.clock_layout, "single");
        assert_eq!(cfg.default_tile_size, "small");

        assert!(set_appearance_settings_ext(
            &mut cfg,
            AppearancePatch {
                reset_visuals: true,
                ..Default::default()
            }
        )
        .unwrap());
        assert_eq!(cfg.glass_depth, "obsidian");
        assert_eq!(cfg.theme_preset, "obsidian");
        assert_eq!(cfg.accent_mode, "per_space");
        assert!(cfg.surface_gradients);
        assert!(cfg.hover_glow);

        // Quick notes & todos CRUD
        assert!(set_quick_notes_text(&mut cfg, "Купить кофе"));
        assert!(add_quick_todo(&mut cfg, "t-1".into(), "Проверить билд").unwrap());
        assert!(toggle_quick_todo(&mut cfg, "t-1"));
        assert!(clear_done_quick_todos(&mut cfg));
        assert!(cfg.quick_notes.todos.is_empty());
    }

    #[test]
    fn mac_settings_defaults_validation_normalization_and_launch_agent_plist() {
        let mut cfg = TestBarConfig::default();
        assert_eq!(cfg.mac_command_key, "any");
        assert_eq!(cfg.mac_window_position, "center");
        assert_eq!(cfg.mac_terminal_app, "auto");

        assert!(set_mac_settings_in_config(
            &mut cfg,
            Some("  LEFT ".to_string()),
            Some("bottom".to_string()),
            Some("Warp".to_string()),
        )
        .unwrap());
        assert_eq!(cfg.mac_command_key, "left");
        assert_eq!(cfg.mac_window_position, "bottom");
        assert_eq!(cfg.mac_terminal_app, "warp");

        // Idempotent no-op
        assert!(!set_mac_settings_in_config(
            &mut cfg,
            Some("left".to_string()),
            Some("bottom".to_string()),
            Some("warp".to_string()),
        )
        .unwrap());

        // Reject invalid values
        assert!(set_mac_settings_in_config(&mut cfg, Some("ctrl".into()), None, None).is_err());
        assert!(set_mac_settings_in_config(&mut cfg, None, Some("top".into()), None).is_err());
        assert!(set_mac_settings_in_config(&mut cfg, None, None, Some("kitty".into())).is_err());

        // Dirty JSON normalization
        let dirty = r#"{"macCommandKey":"bogus","macWindowPosition":"floating","macTerminalApp":"cmd"}"#;
        let parsed = import_config_json(dirty).expect("import dirty mac fields");
        assert_eq!(parsed.mac_command_key, "any");
        assert_eq!(parsed.mac_window_position, "center");
        assert_eq!(parsed.mac_terminal_app, "auto");

        // LaunchAgent bundle path & XML escaping
        let resolved = resolve_macos_bundle_executable(Path::new(
            "/Applications/TestBar & Co.app/Contents/MacOS/TestBar",
        ));
        assert!(resolved
            .to_string_lossy()
            .ends_with("TestBar & Co.app/Contents/MacOS/TestBar")
            || resolved
                .to_string_lossy()
                .ends_with(r"TestBar & Co.app\Contents\MacOS\TestBar"));

        let plist = build_macos_launch_agent_plist(Path::new(
            "/Applications/TestBar & <Dev>.app/Contents/MacOS/TestBar",
        ))
        .expect("build plist");
        assert!(plist.contains("<string>com.testbar.shell</string>"));
        assert!(plist.contains("&amp; &lt;Dev&gt;.app"));
        assert!(plist.contains("<true/>"));
        assert!(build_macos_launch_agent_plist(Path::new("   ")).is_err());
    }
}
