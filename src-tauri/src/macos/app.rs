use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::config::{self, CustomSpace, PinItem, SpaceHeaderWidget, TestBarConfig};

use super::apps_indexer::{self, IndexedApp, IndexedFile};
use super::battery::{self, PeripheralBattery};
use super::bridge_ffi;
use super::clipboard::{self, ClipboardEntry};
use super::command_tap::{CommandTapMachine, CommandTapMode};
use super::file_explorer::{self, DirectoryListing, FileEntry};
use super::icon_cache::{self, IconCache};
use super::launcher;
use super::network::{self, NetworkSnapshot};
use super::quick_toggles::{self, QuickTogglesState};
use super::telemetry::{self, TelemetryState};
use super::weather::{self, WeatherCityOption, WeatherSnapshot};

const START_WIDTH_LOGICAL: f64 = 1188.0;
const START_HEIGHT_LOGICAL: f64 = 728.0;
const START_EXIT_ANIM_MS: u64 = 140;
const START_BLUR_GRACE_MS: u64 = 180;
const PERIODIC_REINDEX_INTERVAL_SECS: u64 = 15 * 60;
const MAX_ICON_SOURCES_ENTRIES: usize = 2048;

const TRAY_ICON_PNG: &[u8] = include_bytes!("../../icons/32x32.png");

static MONOTONIC_EPOCH: OnceLock<std::time::Instant> = OnceLock::new();
static TAP_CALLBACK_CTX: OnceLock<TapCallbackContext> = OnceLock::new();

struct TapCallbackContext {
    app: AppHandle,
    machine: Arc<Mutex<CommandTapMachine>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub platform: &'static str,
    pub accessibility_trusted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartStateEvent {
    pub open: bool,
    pub gen: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub immediate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartShownEvent {
    pub open: bool,
    pub gen: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MacAudioState {
    pub available: bool,
    pub master_volume: u32,
    pub master_muted: bool,
    pub mic_muted: bool,
    pub master_peak: f32,
    pub mic_peak: f32,
    pub devices: Vec<serde_json::Value>,
    pub input_devices: Vec<serde_json::Value>,
    pub sessions: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MacAudioPeaksSnapshot {
    pub master_peak: f32,
    pub mic_peak: f32,
    pub session_peaks: Vec<(String, f32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacMediaState {
    pub has_session: bool,
    pub title: String,
    pub artist: String,
    pub source: String,
    pub is_playing: bool,
    pub position_sec: f64,
    pub duration_sec: f64,
    pub can_seek: bool,
    pub last_updated_ms: u64,
    pub thumbnail: Option<String>,
    pub sessions: Vec<serde_json::Value>,
    pub active_session_id: String,
}

impl Default for MacMediaState {
    fn default() -> Self {
        Self {
            has_session: false,
            title: "Ничего не воспроизводится".to_string(),
            artist: "Запусти музыку или видео".to_string(),
            source: String::new(),
            is_playing: false,
            position_sec: 0.0,
            duration_sec: 0.0,
            can_seek: false,
            last_updated_ms: 0,
            thumbnail: None,
            sessions: Vec::new(),
            active_session_id: String::new(),
        }
    }
}

/// Tri-state IPC argument distinguishing omitted (`Unset` -> keep unchanged),
/// explicit JSON `null` (`Null` -> clear), and a provided value (`Value(T)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchArg<T> {
    Unset,
    Null,
    Value(T),
}

impl<T> PatchArg<T> {
    pub fn into_double_option(self) -> Option<Option<T>> {
        match self {
            PatchArg::Unset => None,
            PatchArg::Null => Some(None),
            PatchArg::Value(v) => Some(Some(v)),
        }
    }
}

impl<'de, T: serde::de::DeserializeOwned, R: tauri::Runtime> tauri::ipc::CommandArg<'de, R>
    for PatchArg<T>
{
    fn from_command(
        command: tauri::ipc::CommandItem<'de, R>,
    ) -> Result<Self, tauri::ipc::InvokeError> {
        let name = command.name;
        let arg = command.key;
        match command.message.payload() {
            tauri::ipc::InvokeBody::Json(v) => match v.get(arg) {
                None => Ok(PatchArg::Unset),
                Some(serde_json::Value::Null) => Ok(PatchArg::Null),
                Some(val) => T::deserialize(val)
                    .map(PatchArg::Value)
                    .map_err(|e| tauri::Error::InvalidArgs(name, arg, e).into()),
            },
            tauri::ipc::InvokeBody::Raw(_) => Err(tauri::Error::InvalidArgs(
                name,
                arg,
                serde_json::de::Error::custom("expected json body"),
            )
            .into()),
        }
    }
}

pub struct MacAppState {
    pub config: Mutex<TestBarConfig>,
    pub icon_cache: Mutex<IconCache>,
    pub icon_sources: Mutex<HashMap<String, String>>,
    pub installed_apps: Mutex<Vec<IndexedApp>>,
    pub indexed_files: Mutex<Vec<IndexedFile>>,
    pub index_refresh_running: Arc<AtomicBool>,
    pub start_open: Arc<AtomicBool>,
    pub start_gen: Arc<AtomicU64>,
    pub last_show_ms: Arc<AtomicU64>,
    pub modal_picker_depth: Arc<AtomicU32>,
    pub prev_frontmost_pid: Arc<AtomicI32>,
    pub command_tap: Arc<Mutex<CommandTapMachine>>,
    pub command_tap_started: Arc<AtomicBool>,
    pub tray_icon: Mutex<Option<TrayIcon>>,
}

impl MacAppState {
    fn register_icon_sources(&self, sources: Vec<(String, String)>) {
        if sources.is_empty() {
            return;
        }
        let mut map = lock_unpoisoned(&self.icon_sources);
        if map.len() + sources.len() > MAX_ICON_SOURCES_ENTRIES {
            map.clear();
        }
        for (k, src) in sources {
            if !k.is_empty() && !src.is_empty() {
                map.insert(k, src);
            }
        }
    }
}

struct MacModalPickerGuard(Arc<AtomicU32>);

impl MacModalPickerGuard {
    fn acquire(depth: &Arc<AtomicU32>) -> Self {
        depth.fetch_add(1, Ordering::AcqRel);
        Self(Arc::clone(depth))
    }
}

impl Drop for MacModalPickerGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn monotonic_ms() -> u64 {
    MONOTONIC_EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

fn decode_png_rgba(bytes: &[u8]) -> Option<tauri::image::Image<'static>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::EXPAND
            | png::Transformations::STRIP_16
            | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .chunks_exact(3)
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks_exact(2)
            .flat_map(|ga| [ga[0], ga[0], ga[0], ga[1]])
            .collect(),
        png::ColorType::Grayscale => buf.into_iter().flat_map(|g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some(tauri::image::Image::new_owned(rgba, info.width, info.height))
}

fn filter_visible_batteries(
    list: Vec<PeripheralBattery>,
    hidden_ids: &[String],
) -> Vec<PeripheralBattery> {
    if hidden_ids.is_empty() {
        return list;
    }
    list.into_iter()
        .filter(|b| {
            let norm = b.id.trim().to_ascii_lowercase();
            !hidden_ids.iter().any(|h| h == &norm)
        })
        .collect()
}

fn normalize_bump_and_save(
    candidate: &mut TestBarConfig,
    path: &std::path::Path,
) -> Result<(), String> {
    config::normalize_and_migrate(candidate);
    candidate.revision = candidate
        .revision
        .checked_add(1)
        .ok_or_else(|| "config revision overflow".to_string())?;
    config::save_config_to(path, candidate)
}

fn mutate_config_at<F>(
    config_mutex: &Mutex<TestBarConfig>,
    path: &std::path::Path,
    mutate: F,
) -> Result<(TestBarConfig, bool), String>
where
    F: FnOnce(&mut TestBarConfig) -> bool,
{
    let mut guard = lock_unpoisoned(config_mutex);
    let mut candidate = guard.clone();
    if !mutate(&mut candidate) {
        return Ok((guard.clone(), false));
    }
    normalize_bump_and_save(&mut candidate, path)?;
    *guard = candidate.clone();
    Ok((candidate, true))
}

fn mutate_config_and_emit<F>(
    app: &AppHandle,
    state: &MacAppState,
    mutate: F,
) -> Result<(TestBarConfig, bool), String>
where
    F: FnOnce(&mut TestBarConfig) -> bool,
{
    let path = config::config_path();
    let (updated, changed) = mutate_config_at(&state.config, &path, mutate)?;
    if changed {
        let _ = app.emit("config-updated", &updated);
    }
    Ok((updated, changed))
}

fn apply_autostart_transaction<FReg>(
    config_mutex: &Mutex<TestBarConfig>,
    path: &std::path::Path,
    enable: bool,
    mut apply_agent: FReg,
) -> Result<(TestBarConfig, bool), String>
where
    FReg: FnMut(bool) -> Result<(), String>,
{
    let mut guard = lock_unpoisoned(config_mutex);
    let prev_enabled = guard.autostart;
    apply_agent(enable)?;
    if prev_enabled == enable {
        return Ok((guard.clone(), false));
    }
    let mut candidate = guard.clone();
    candidate.autostart = enable;
    if let Err(err) = normalize_bump_and_save(&mut candidate, path) {
        let _ = apply_agent(prev_enabled);
        return Err(err);
    }
    *guard = candidate.clone();
    Ok((candidate, true))
}

fn position_start_window(app: &AppHandle, position_mode: &str) {
    if let Some(win) = app.get_webview_window("start") {
        if let Ok(ns_win) = win.ns_window() {
            bridge_ffi::configure_start_window(ns_win);
            bridge_ffi::position_start_window(
                ns_win,
                START_WIDTH_LOGICAL,
                START_HEIGHT_LOGICAL,
                position_mode,
            );
        }
    }
}

fn show_start(app: &AppHandle) {
    let st = app.state::<MacAppState>();
    let own_pid = std::process::id() as i32;
    if let Some(fg_pid) = bridge_ffi::frontmost_app_pid() {
        if fg_pid > 0 && fg_pid != own_pid {
            st.prev_frontmost_pid.store(fg_pid, Ordering::Release);
        }
    }

    let show_gen = st.start_gen.fetch_add(1, Ordering::SeqCst) + 1;
    st.start_open.store(true, Ordering::Release);
    st.last_show_ms.store(monotonic_ms(), Ordering::Release);

    let (cfg_snapshot, position_mode, hidden_bats) = {
        let cfg = lock_unpoisoned(&st.config);
        (
            cfg.clone(),
            cfg.mac_window_position.clone(),
            cfg.hidden_battery_ids.clone(),
        )
    };
    position_start_window(app, &position_mode);

    let _ = app.emit(
        "start-state",
        StartStateEvent {
            open: true,
            gen: show_gen,
            monitor: Some("primary".to_string()),
            immediate: false,
        },
    );
    let _ = app.emit_to("start", "config-updated", &cfg_snapshot);
    let clips = clipboard::get_clipboard_history();
    let _ = app.emit_to("start", "clipboard-updated", &clips);

    if let Some(win) = app.get_webview_window("start") {
        let _ = win.show();
        let _ = win.set_focus();
    }

    let _ = app.emit_to(
        "start",
        "start-shown",
        StartShownEvent {
            open: true,
            gen: show_gen,
            monitor: Some("primary".to_string()),
        },
    );

    let h_hyd = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let st_h = h_hyd.state::<MacAppState>();
        if !st_h.start_open.load(Ordering::Acquire)
            || st_h.start_gen.load(Ordering::SeqCst) != show_gen
        {
            return;
        }
        let tel = telemetry::get_telemetry_cached(&st_h.icon_cache);
        if st_h.start_open.load(Ordering::Acquire)
            && st_h.start_gen.load(Ordering::SeqCst) == show_gen
        {
            let _ = h_hyd.emit_to("start", "telemetry-updated", &tel);
        } else {
            return;
        }
        let periphs = filter_visible_batteries(battery::get_peripheral_batteries(), &hidden_bats);
        if st_h.start_open.load(Ordering::Acquire)
            && st_h.start_gen.load(Ordering::SeqCst) == show_gen
        {
            let _ = h_hyd.emit_to("start", "peripherals-updated", &periphs);
        }
    });
}

fn hide_start(app: &AppHandle, immediate: bool) {
    let st = app.state::<MacAppState>();
    let was_open = st.start_open.swap(false, Ordering::AcqRel);
    let hide_gen = st.start_gen.fetch_add(1, Ordering::SeqCst) + 1;

    if immediate {
        let _ = app.emit(
            "start-state",
            StartStateEvent {
                open: false,
                gen: hide_gen,
                monitor: None,
                immediate: true,
            },
        );
        if let Some(win) = app.get_webview_window("start") {
            let _ = win.hide();
        }
        return;
    }

    if !was_open {
        return;
    }

    let _ = app.emit(
        "start-state",
        StartStateEvent {
            open: false,
            gen: hide_gen,
            monitor: None,
            immediate: false,
        },
    );

    let h_hide = app.clone();
    let gen_ref = Arc::clone(&st.start_gen);
    let open_ref = Arc::clone(&st.start_open);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(START_EXIT_ANIM_MS)).await;
        if gen_ref.load(Ordering::SeqCst) != hide_gen || open_ref.load(Ordering::Acquire) {
            return;
        }
        let h = h_hide.clone();
        let _ = h_hide.run_on_main_thread(move || {
            let st_m = h.state::<MacAppState>();
            if st_m.start_gen.load(Ordering::SeqCst) == hide_gen
                && !st_m.start_open.load(Ordering::Acquire)
            {
                if let Some(win) = h.get_webview_window("start") {
                    let _ = win.hide();
                }
            }
        });
    });
}

fn apply_toggle_start(app: &AppHandle, force: Option<bool>) {
    let st = app.state::<MacAppState>();
    let currently_open = st.start_open.load(Ordering::Acquire);
    let want_open = force.unwrap_or(!currently_open);
    if want_open == currently_open {
        if want_open {
            if let Some(win) = app.get_webview_window("start") {
                let _ = win.set_focus();
            }
        }
        return;
    }
    if want_open {
        show_start(app);
    } else {
        hide_start(app, false);
    }
}

extern "C" fn on_cg_tap_event(
    event_kind: i32,
    key_code: u16,
    cg_flags: u64,
    timestamp_ms: u64,
    _ctx: *mut std::ffi::c_void,
) {
    let Some(tap_ctx) = TAP_CALLBACK_CTX.get() else {
        return;
    };
    let triggered = {
        let mut machine = lock_unpoisoned(&tap_ctx.machine);
        match event_kind {
            1 => machine.on_flags_changed(key_code, cg_flags, timestamp_ms),
            2 => {
                machine.on_key_down();
                false
            }
            3 => {
                machine.on_mouse_down();
                false
            }
            _ => false,
        }
    };
    if triggered {
        let h = tap_ctx.app.clone();
        let _ = tap_ctx.app.run_on_main_thread(move || {
            apply_toggle_start(&h, None);
        });
    }
}

fn run_background_index_refresh(app: &AppHandle) {
    let st = app.state::<MacAppState>();
    if st
        .index_refresh_running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    struct ResetGuard(Arc<AtomicBool>);
    impl Drop for ResetGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let _reset = ResetGuard(Arc::clone(&st.index_refresh_running));

    let apps = apps_indexer::scan_installed_apps_cached(&st.icon_cache, Some(&st.icon_sources));
    *lock_unpoisoned(&st.installed_apps) = apps.clone();
    let _ = app.emit_to("start", "apps-indexed", &apps);

    let files = apps_indexer::scan_user_files();
    *lock_unpoisoned(&st.indexed_files) = files.clone();
    let _ = app.emit_to("start", "files-indexed", &files);
}

async fn run_dialog_on_main<T, F>(app: &AppHandle, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let st = app.state::<MacAppState>();
    let _guard = MacModalPickerGuard::acquire(&st.modal_picker_depth);
    let (tx, rx) = std::sync::mpsc::sync_channel::<T>(1);
    let h_after = app.clone();
    app.run_on_main_thread(move || {
        let res = f();
        let st_inner = h_after.state::<MacAppState>();
        if st_inner.start_open.load(Ordering::Acquire) {
            st_inner.last_show_ms.store(monotonic_ms(), Ordering::Release);
            if let Some(win) = h_after.get_webview_window("start") {
                let _ = win.set_focus();
            }
        }
        let _ = tx.send(res);
    })
    .map_err(|e| format!("main thread dispatch failed: {e}"))?;

    tauri::async_runtime::spawn_blocking(move || {
        rx.recv().map_err(|_| "dialog canceled unexpectedly".to_string())
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

fn load_custom_icon_image_data_uri(path: &std::path::Path) -> Result<String, String> {
    let path_str = path.to_string_lossy().to_string();
    let mut thumbs = file_explorer::get_file_thumbnails(vec![path_str.clone()], 64);
    if let Some(uri) = thumbs.remove(&path_str) {
        if let Some(valid) = config::sanitize_custom_icon_data(&uri) {
            return Ok(valid);
        }
    }

    let is_png = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    if is_png {
        if let Ok(bytes) = std::fs::read(path) {
            const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
            if bytes.len() <= 92 * 1024 && bytes.starts_with(PNG_MAGIC) {
                let b64 =
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                let uri = format!("data:image/png;base64,{b64}");
                if let Some(valid) = config::sanitize_custom_icon_data(&uri) {
                    return Ok(valid);
                }
            }
        }
    }

    Err("Не удалось загрузить изображение иконки (максимум 128 КБ)".to_string())
}

fn resolve_system_settings_icon_path() -> Option<&'static str> {
    const CANDIDATES: &[&str] = &[
        "/System/Applications/System Settings.app",
        "/System/Applications/System Preferences.app",
    ];
    CANDIDATES
        .iter()
        .copied()
        .find(|p| std::path::Path::new(p).exists())
}

// ======================= TAURI IPC COMMANDS =======================

#[tauri::command]
fn get_platform_info() -> PlatformInfo {
    PlatformInfo {
        platform: "macos",
        accessibility_trusted: bridge_ffi::is_accessibility_trusted(false),
    }
}

#[tauri::command]
fn get_accessibility_status() -> bool {
    bridge_ffi::is_accessibility_trusted(false)
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    let _ = bridge_ffi::is_accessibility_trusted(true);
    bridge_ffi::open_accessibility_settings();
    Ok(())
}

#[tauri::command]
fn set_mac_settings(
    app: AppHandle,
    state: State<MacAppState>,
    mac_command_key: Option<String>,
    mac_window_position: Option<String>,
    mac_terminal_app: Option<String>,
) -> Result<TestBarConfig, String> {
    let pos_requested = mac_window_position.is_some();
    let mut inner_err: Option<String> = None;
    let (updated, changed) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::set_mac_settings_in_config(
            cfg,
            mac_command_key.clone(),
            mac_window_position.clone(),
            mac_terminal_app.clone(),
        ) {
            Ok(ch) => ch,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }

    if changed {
        let mode = CommandTapMode::from_config_str(&updated.mac_command_key);
        lock_unpoisoned(&state.command_tap).set_mode(mode);
        if pos_requested {
            let h = app.clone();
            let pos = updated.mac_window_position.clone();
            let _ = app.run_on_main_thread(move || {
                position_start_window(&h, &pos);
            });
        }
    }
    Ok(updated)
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    quick_toggles::shutdown();
    app.exit(0);
}

#[tauri::command]
fn get_config(state: State<MacAppState>) -> TestBarConfig {
    lock_unpoisoned(&state.config).clone()
}

#[tauri::command]
fn set_autostart(
    app: AppHandle,
    state: State<MacAppState>,
    enable: bool,
) -> Result<TestBarConfig, String> {
    let path = config::config_path();
    let (updated, changed) =
        apply_autostart_transaction(&state.config, &path, enable, config::set_macos_autostart)?;
    if changed {
        let _ = app.emit("config-updated", &updated);
    }
    Ok(updated)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_appearance_settings(
    app: AppHandle,
    state: State<MacAppState>,
    glass_depth: Option<String>,
    theme_preset: Option<String>,
    accent_mode: Option<String>,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    surface_gradients: Option<bool>,
    hover_glow: Option<bool>,
    clock_show_seconds: Option<bool>,
    clock_show_weekday: Option<bool>,
    clock_date_format: Option<String>,
    clock_layout: Option<String>,
    start_remember_space: Option<bool>,
    default_tile_size: Option<String>,
    reset_visuals: Option<bool>,
) -> Result<TestBarConfig, String> {
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        let patch = config::AppearancePatch {
            glass_depth: glass_depth.clone(),
            theme_preset: theme_preset.clone(),
            accent_mode: accent_mode.clone(),
            accent_hex: accent_hex.clone(),
            accent_rgb: accent_rgb.clone(),
            surface_gradients,
            hover_glow,
            clock_show_seconds,
            clock_show_weekday,
            clock_date_format: clock_date_format.clone(),
            clock_layout: clock_layout.clone(),
            start_remember_space,
            default_tile_size: default_tile_size.clone(),
            reset_visuals: reset_visuals.unwrap_or(false),
        };
        match config::set_appearance_settings_ext(cfg, patch) {
            Ok(ch) => ch,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn toggle_start(app: AppHandle, force: Option<bool>, _monitor: Option<String>) {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        apply_toggle_start(&h, force);
    });
}

#[tauri::command]
fn open_start_settings(app: AppHandle) {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        show_start(&h);
        let _ = h.emit_to("start", "open-start-settings", ());
    });
}

#[tauri::command]
fn surface_commit_ack(_surface: String, _gen: u64, _measured_height: Option<f64>) {}

#[tauri::command]
fn surface_exit_ack(app: AppHandle, surface: String, gen: u64) {
    if surface != "start" {
        return;
    }
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        let st = h.state::<MacAppState>();
        if st.start_gen.load(Ordering::SeqCst) == gen && !st.start_open.load(Ordering::Acquire) {
            if let Some(win) = h.get_webview_window("start") {
                let _ = win.hide();
            }
        }
    });
}

#[tauri::command]
fn get_backdrop_mode() -> &'static str {
    "dcomp"
}

#[tauri::command]
fn animate_surface_in(
    _surface: String,
    _gen: u64,
    _duration_ms: Option<u32>,
    _offset_y: Option<f32>,
    _from_scale: Option<f32>,
) {
}

#[tauri::command]
fn animate_surface_out(
    _surface: String,
    _gen: u64,
    _duration_ms: Option<u32>,
    _offset_y: Option<f32>,
    _to_scale: Option<f32>,
) {
}

#[tauri::command]
fn dismiss_overlays(app: AppHandle) {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        hide_start(&h, false);
    });
}

#[tauri::command]
async fn get_icons(app: AppHandle, keys: Vec<String>) -> Result<HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MacAppState>();
        let mut resolved = lock_unpoisoned(&state.icon_cache).get_batch(&keys);
        let missing: Vec<(String, String)> = {
            let sources = lock_unpoisoned(&state.icon_sources);
            keys.iter()
                .filter(|k| !k.is_empty() && !resolved.contains_key(*k))
                .filter_map(|k| sources.get(k).map(|src| (k.clone(), src.clone())))
                .collect()
        };
        for (key, src) in missing {
            if let Some(uri) = icon_cache::extract_icon_for_path_cached(&src, &state.icon_cache) {
                resolved.insert(key, uri);
            }
        }
        resolved
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn resolve_pin_icons(
    app: AppHandle,
    targets: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MacAppState>();
        let mut out = HashMap::new();
        let mut seen = std::collections::HashSet::new();
        let mut new_sources = Vec::new();

        for raw in targets {
            let trimmed = raw.trim();
            if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
                continue;
            }
            let low = trimmed.to_ascii_lowercase();
            let resolved_path: Option<String> = if trimmed.starts_with('/') {
                Some(trimmed.to_string())
            } else if low.starts_with("x-apple.systempreferences:") {
                resolve_system_settings_icon_path().map(str::to_string)
            } else {
                None
            };

            if let Some(path_str) = resolved_path {
                if let Some(uri) =
                    icon_cache::extract_icon_for_path_cached(&path_str, &state.icon_cache)
                {
                    let key = icon_cache::path_icon_key(&path_str);
                    new_sources.push((key, path_str));
                    out.insert(trimmed.to_string(), uri);
                }
            }
        }

        state.register_icon_sources(new_sources);
        out
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_icon_for_path(app: AppHandle, path: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MacAppState>();
        Ok(icon_cache::extract_icon_for_path_cached(
            &path,
            &state.icon_cache,
        ))
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
fn get_installed_apps(state: State<MacAppState>) -> Vec<IndexedApp> {
    lock_unpoisoned(&state.installed_apps).clone()
}

#[tauri::command]
fn get_indexed_files(state: State<MacAppState>) -> Vec<IndexedFile> {
    lock_unpoisoned(&state.indexed_files).clone()
}

#[tauri::command]
async fn refresh_indexes(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_background_index_refresh(&app);
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
fn get_open_windows(_monitor: Option<String>) -> Vec<serde_json::Value> {
    Vec::new()
}

#[tauri::command]
fn get_steam_games() -> Vec<serde_json::Value> {
    Vec::new()
}

#[tauri::command]
fn get_audio_state() -> MacAudioState {
    MacAudioState::default()
}

#[tauri::command]
fn get_media_state() -> MacMediaState {
    MacMediaState::default()
}

#[tauri::command]
fn get_audio_peaks() -> MacAudioPeaksSnapshot {
    MacAudioPeaksSnapshot::default()
}

#[tauri::command]
async fn add_pin(
    app: AppHandle,
    space: String,
    mut item: PinItem,
) -> Result<TestBarConfig, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<MacAppState>();
        let force_combo = config::should_force_combo(&space, &item.kind);
        config::normalize_pin(&mut item, force_combo);

        let target_trimmed = item.target.trim().to_string();
        if (item.kind == "app" || item.kind == "file") && target_trimmed.starts_with('/') {
            let key = icon_cache::path_icon_key(&target_trimmed);
            if icon_cache::extract_icon_for_path_cached(&target_trimmed, &state.icon_cache)
                .is_some()
            {
                state.register_icon_sources(vec![(key.clone(), target_trimmed)]);
                if item.icon_key.trim().is_empty()
                    || item.icon_key == "app"
                    || item.icon_key == "file"
                    || item.icon_key == "settings"
                {
                    item.icon_key = key;
                }
            }
        }

        if item.custom_icon.as_deref() != Some("custom_image") {
            item.icon_data = None;
        } else {
            item.icon_data = item
                .icon_data
                .as_deref()
                .and_then(config::sanitize_custom_icon_data);
        }

        let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
            config::add_pin_to_space(cfg, &space, item)
        })?;
        Ok(updated)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn update_pin(
    app: AppHandle,
    state: State<MacAppState>,
    space: String,
    pin_id: String,
    title: Option<String>,
    target: Option<String>,
    args: Option<String>,
    alt_action: Option<String>,
    alt_label: Option<String>,
    custom_icon: PatchArg<String>,
    custom_color: PatchArg<String>,
    icon_data: PatchArg<String>,
    tile_size: PatchArg<String>,
    section: PatchArg<String>,
    run_as_admin: Option<bool>,
) -> Result<TestBarConfig, String> {
    let custom_icon_opt = custom_icon.into_double_option();
    let custom_color_opt = custom_color.into_double_option();
    let icon_data_opt = icon_data.into_double_option();
    let tile_size_opt = tile_size.into_double_option();
    let section_opt = section.into_double_option();
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::update_pin_v2(
            cfg,
            &space,
            &pin_id,
            title.clone(),
            target.clone(),
            args.clone(),
            alt_action.clone(),
            alt_label.clone(),
            custom_icon_opt.clone(),
            custom_color_opt.clone(),
            icon_data_opt.clone(),
            tile_size_opt.clone(),
            section_opt.clone(),
            run_as_admin,
        ) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn remove_pin(
    app: AppHandle,
    state: State<MacAppState>,
    space: String,
    item_id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::remove_pin_from_space(cfg, &space, &item_id)
    })?;
    Ok(updated)
}

#[tauri::command]
fn reorder_pins(
    app: AppHandle,
    state: State<MacAppState>,
    space: String,
    from_idx: usize,
    to_idx: usize,
) -> Result<TestBarConfig, String> {
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::reorder_pins_in_space(cfg, &space, from_idx, to_idx)
    })?;
    Ok(updated)
}

#[tauri::command]
fn move_pin_in_space(
    app: AppHandle,
    state: State<MacAppState>,
    space: String,
    from_id: String,
    to_id: Option<String>,
    target_section: Option<String>,
) -> Result<TestBarConfig, String> {
    if let Some(ref sec) = target_section {
        config::validate_tile_section(sec)?;
    }
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::reorder_pins_by_id(
            cfg,
            &space,
            &from_id,
            to_id.as_deref(),
            target_section.as_deref(),
        )
    })?;
    Ok(updated)
}

#[tauri::command]
fn add_custom_space(
    app: AppHandle,
    state: State<MacAppState>,
    name: String,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    widget_preset: Option<String>,
) -> Result<TestBarConfig, String> {
    let clean = name.trim().to_string();
    if clean.is_empty() {
        return Err("Пустое название".to_string());
    }
    let preset = match widget_preset {
        Some(ref p) if !p.trim().is_empty() => config::validate_widget_preset(p)?.to_string(),
        _ => config::default_widget_preset(),
    };
    let (hex, rgb) = config::sanitize_accent_hex_rgb(accent_hex.as_deref(), accent_rgb.as_deref());
    let id = format!("custom-{}", now_ms());
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        cfg.custom_spaces.push(CustomSpace {
            id,
            name: clean,
            accent_hex: hex,
            accent_rgb: rgb,
            widget_preset: preset,
            items: vec![],
        });
        true
    })?;
    Ok(updated)
}

#[tauri::command]
fn update_custom_space(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
    name: Option<String>,
    accent: Option<String>,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    widget_preset: Option<String>,
) -> Result<TestBarConfig, String> {
    let eff_hex = accent_hex.or_else(|| {
        accent
            .as_deref()
            .map(str::trim)
            .filter(|a| a.starts_with('#'))
            .map(str::to_string)
    });
    let eff_rgb = accent_rgb.or_else(|| {
        accent
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty() && !a.starts_with('#'))
            .map(str::to_string)
    });
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::update_custom_space(
            cfg,
            &id,
            name.clone(),
            eff_hex.clone(),
            eff_rgb.clone(),
            widget_preset.clone(),
        ) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn delete_custom_space(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::delete_custom_space(cfg, &id)
    })?;
    Ok(updated)
}

#[tauri::command]
fn create_space(
    app: AppHandle,
    state: State<MacAppState>,
    name: String,
    template: Option<String>,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    widgets: Option<Vec<String>>,
) -> Result<TestBarConfig, String> {
    let tpl = template.unwrap_or_else(|| "blank".to_string());
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::create_space_from_template(
            cfg,
            name.clone(),
            &tpl,
            accent_hex.clone(),
            accent_rgb.clone(),
            widgets.clone(),
        ) {
            Ok(_) => true,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn update_space(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
    name: Option<String>,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
    layout_kind: Option<String>,
    show_hero_row: Option<bool>,
    widgets: Option<Vec<String>>,
    bottom_section_title: PatchArg<String>,
) -> Result<TestBarConfig, String> {
    let bottom_title_opt = bottom_section_title.into_double_option();
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::update_space_config(
            cfg,
            &id,
            name.clone(),
            accent_hex.clone(),
            accent_rgb.clone(),
            layout_kind.clone(),
            show_hero_row,
            widgets.clone(),
            bottom_title_opt.clone(),
        ) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn delete_space(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::delete_space_by_id(cfg, &id) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn reorder_spaces(
    app: AppHandle,
    state: State<MacAppState>,
    ordered_ids: Vec<String>,
) -> Result<TestBarConfig, String> {
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::reorder_spaces_by_ids(cfg, ordered_ids.clone()) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn set_space_widgets(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
    widgets: Vec<String>,
) -> Result<TestBarConfig, String> {
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::set_space_widgets_by_id(cfg, &id, widgets.clone()) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn set_space_widget_layout(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
    widgets: Vec<String>,
    header_widgets: Vec<SpaceHeaderWidget>,
) -> Result<TestBarConfig, String> {
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::set_space_widget_layout_in_config(
            cfg,
            &id,
            widgets.clone(),
            header_widgets.clone(),
        ) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_taskbar_widgets(
    app: AppHandle,
    state: State<MacAppState>,
    show_weather: Option<bool>,
    show_batteries: Option<bool>,
    show_telemetry: Option<bool>,
    left_widgets: Option<Vec<String>>,
    widgets_position: Option<String>,
    weather_city: Option<String>,
    weather_lat: PatchArg<f64>,
    weather_lon: PatchArg<f64>,
    weather_units: Option<String>,
) -> Result<TestBarConfig, String> {
    let lat_patch = weather_lat.into_double_option();
    let lon_patch = weather_lon.into_double_option();
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        let mut next = cfg.taskbar_widgets.clone();
        if let Some(ref lw) = left_widgets {
            match config::validate_taskbar_widget_list(lw) {
                Ok(clean) => {
                    next.left_widgets = clean;
                    config::sync_legacy_taskbar_booleans_from_left_widgets(&mut next);
                }
                Err(err) => {
                    inner_err = Some(err);
                    return false;
                }
            }
        } else {
            if let Some(v) = show_weather {
                next.show_weather = v;
                config::apply_taskbar_widget_boolean_to_left_widgets(
                    &mut next.left_widgets,
                    "weather",
                    v,
                );
            }
            if let Some(v) = show_batteries {
                next.show_batteries = v;
                config::apply_taskbar_widget_boolean_to_left_widgets(
                    &mut next.left_widgets,
                    "batteries",
                    v,
                );
            }
            if let Some(v) = show_telemetry {
                next.show_telemetry = v;
                config::apply_taskbar_widget_boolean_to_left_widgets(
                    &mut next.left_widgets,
                    "telemetry",
                    v,
                );
            }
        }
        if let Some(ref pos) = widgets_position {
            next.widgets_position = pos.clone();
        }
        if let Some(ref city) = weather_city {
            next.weather_city = city.clone();
        }
        if let Some(lat) = lat_patch {
            next.weather_lat = lat;
        }
        if let Some(lon) = lon_patch {
            next.weather_lon = lon;
        }
        if let Some(ref units) = weather_units {
            next.weather_units = units.clone();
        }
        match config::set_taskbar_widgets(cfg, next) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn save_quick_notes_text(
    app: AppHandle,
    state: State<MacAppState>,
    text: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::set_quick_notes_text(cfg, &text)
    })?;
    Ok(updated)
}

#[tauri::command]
fn add_quick_todo(
    app: AppHandle,
    state: State<MacAppState>,
    text: String,
) -> Result<TestBarConfig, String> {
    let id = format!("todo-{}", now_ms());
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::add_quick_todo(cfg, id.clone(), &text) {
            Ok(changed) => changed,
            Err(err) => {
                inner_err = Some(err);
                false
            }
        }
    })?;
    if let Some(err) = inner_err {
        return Err(err);
    }
    Ok(updated)
}

#[tauri::command]
fn toggle_quick_todo(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, |cfg| config::toggle_quick_todo(cfg, &id))?;
    Ok(updated)
}

#[tauri::command]
fn delete_quick_todo(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, |cfg| config::delete_quick_todo(cfg, &id))?;
    Ok(updated)
}

#[tauri::command]
fn clear_done_quick_todos(
    app: AppHandle,
    state: State<MacAppState>,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, config::clear_done_quick_todos)?;
    Ok(updated)
}

#[tauri::command]
fn hide_tray_app(
    app: AppHandle,
    state: State<MacAppState>,
    key: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, |cfg| config::hide_tray_key(cfg, &key))?;
    Ok(updated)
}

#[tauri::command]
fn restore_tray_app(
    app: AppHandle,
    state: State<MacAppState>,
    key: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, |cfg| config::restore_tray_key(cfg, &key))?;
    Ok(updated)
}

#[tauri::command]
fn restore_all_tray_apps(
    app: AppHandle,
    state: State<MacAppState>,
) -> Result<TestBarConfig, String> {
    let (updated, _) = mutate_config_and_emit(&app, &state, config::restore_all_tray_keys)?;
    Ok(updated)
}

#[tauri::command]
fn hide_battery_device(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, changed) = mutate_config_and_emit(&app, &state, |cfg| {
        config::hide_battery_device(cfg, &id)
    })?;
    if changed {
        let h = app.clone();
        let hidden = updated.hidden_battery_ids.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let list = filter_visible_batteries(battery::get_peripheral_batteries(), &hidden);
            let _ = h.emit_to("start", "peripherals-updated", &list);
        });
    }
    Ok(updated)
}

#[tauri::command]
fn restore_battery_device(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, changed) = mutate_config_and_emit(&app, &state, |cfg| {
        config::restore_battery_device(cfg, &id)
    })?;
    if changed {
        let h = app.clone();
        let hidden = updated.hidden_battery_ids.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let list = filter_visible_batteries(battery::get_peripheral_batteries(), &hidden);
            let _ = h.emit_to("start", "peripherals-updated", &list);
        });
    }
    Ok(updated)
}

#[tauri::command]
fn restore_all_battery_devices(
    app: AppHandle,
    state: State<MacAppState>,
) -> Result<TestBarConfig, String> {
    let (updated, changed) =
        mutate_config_and_emit(&app, &state, config::restore_all_battery_devices)?;
    if changed {
        let h = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let list = battery::get_peripheral_batteries();
            let _ = h.emit_to("start", "peripherals-updated", &list);
        });
    }
    Ok(updated)
}

#[tauri::command]
fn record_frecency(
    app: AppHandle,
    state: State<MacAppState>,
    item_id: String,
) -> Result<TestBarConfig, String> {
    let ts = now_ms();
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::record_frecency(cfg, &item_id, ts)
    })?;
    Ok(updated)
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    let picked = run_dialog_on_main(&app, || {
        rfd::FileDialog::new()
            .set_title("Выбери папку")
            .pick_folder()
    })
    .await?;
    Ok(picked.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
async fn pick_file(app: AppHandle) -> Result<Option<String>, String> {
    // Do NOT filter extensions on macOS so `.app` bundles remain selectable alongside regular files.
    let picked = run_dialog_on_main(&app, || {
        rfd::FileDialog::new()
            .set_title("Выбери программу или файл")
            .pick_file()
    })
    .await?;
    Ok(picked.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
async fn pick_custom_icon_image(app: AppHandle) -> Result<Option<String>, String> {
    let picked = run_dialog_on_main(&app, || {
        rfd::FileDialog::new()
            .set_title("Выбери изображение для иконки")
            .add_filter("Изображения", &["png", "jpg", "jpeg", "webp", "icns", "bmp"])
            .pick_file()
    })
    .await?;
    let Some(path) = picked else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || load_custom_icon_image_data_uri(&path).map(Some))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn export_config_dialog(app: AppHandle) -> Result<Option<String>, String> {
    let cfg_snapshot = lock_unpoisoned(&app.state::<MacAppState>().config).clone();
    let picked = run_dialog_on_main(&app, || {
        rfd::FileDialog::new()
            .set_title("Экспорт конфигурации TestBar")
            .add_filter("JSON", &["json"])
            .set_file_name("testbar-config.json")
            .save_file()
    })
    .await?;
    let Some(path) = picked else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || {
        let payload = config::export_config_json(&cfg_snapshot)?;
        std::fs::write(&path, payload)
            .map_err(|e| format!("Не удалось записать файл конфигурации: {e}"))?;
        Ok(Some(path.to_string_lossy().to_string()))
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn import_config_dialog(app: AppHandle) -> Result<Option<TestBarConfig>, String> {
    let picked = run_dialog_on_main(&app, || {
        rfd::FileDialog::new()
            .set_title("Импорт конфигурации TestBar")
            .add_filter("JSON", &["json"])
            .pick_file()
    })
    .await?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let h_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("Не удалось прочитать файл конфигурации: {e}"))?;
        let imported = config::import_config_json(&raw)?;
        let st = h_app.state::<MacAppState>();
        let (updated, _) = mutate_config_and_emit(&h_app, &st, |cfg| {
            let prev_rev = cfg.revision;
            *cfg = imported;
            cfg.revision = prev_rev;
            true
        })?;
        lock_unpoisoned(&st.command_tap)
            .set_mode(CommandTapMode::from_config_str(&updated.mac_command_key));
        let clips = clipboard::hydrate_pinned_clipboard(&updated.pinned_clipboard);
        if st.start_open.load(Ordering::Acquire) {
            let _ = h_app.emit_to("start", "clipboard-updated", &clips);
        }
        let h_pos = h_app.clone();
        let pos = updated.mac_window_position.clone();
        let _ = h_app.run_on_main_thread(move || {
            position_start_window(&h_pos, &pos);
        });
        Ok(Some(updated))
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_telemetry(app: AppHandle) -> Result<TelemetryState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<MacAppState>();
        telemetry::get_telemetry_cached(&st.icon_cache)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn kill_process_pid(pid: u32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || telemetry::kill_process_pid(pid))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn kill_process_group(pids: Vec<u32>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || telemetry::kill_process_group(&pids))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn launch_item(
    app: AppHandle,
    target: String,
    args: Option<String>,
    _as_admin: Option<bool>,
    close_start: Option<bool>,
) -> Result<(), String> {
    if close_start.unwrap_or(false) {
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            hide_start(&h, false);
        });
    }
    tauri::async_runtime::spawn_blocking(move || launcher::launch_item(&target, args.as_deref()))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn launch_new_window(
    app: AppHandle,
    exe_path: String,
    args: Option<String>,
    close_start: Option<bool>,
) -> Result<(), String> {
    if close_start.unwrap_or(true) {
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            hide_start(&h, false);
        });
    }
    tauri::async_runtime::spawn_blocking(move || {
        launcher::launch_new_window(&exe_path, args.as_deref())
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn launch_combo(app: AppHandle, targets: Vec<String>) -> Result<(), String> {
    if targets.iter().any(|t| !t.trim().is_empty()) {
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            hide_start(&h, false);
        });
    }
    tauri::async_runtime::spawn_blocking(move || launcher::launch_combo(&targets))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn open_location(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || launcher::open_location(&path))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn open_terminal_at(app: AppHandle, path: String) -> Result<(), String> {
    let terminal_pref = lock_unpoisoned(&app.state::<MacAppState>().config)
        .mac_terminal_app
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        launcher::open_terminal_at(&path, &terminal_pref)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn power_action(action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || launcher::power_action(&action))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
fn get_clipboard_history() -> Vec<ClipboardEntry> {
    clipboard::get_clipboard_history()
}

#[tauri::command]
fn copy_to_clipboard(text: String) -> Result<(), String> {
    clipboard::copy_to_clipboard(&text)
}

#[tauri::command]
fn toggle_clip_pin(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
) -> Vec<ClipboardEntry> {
    let mut list = clipboard::toggle_clipboard_pin(&id);
    if let Some(entry) = list.iter().find(|c| c.id == id) {
        let text = entry.text.clone();
        let pinned = entry.pinned;
        if mutate_config_and_emit(&app, &state, |cfg| {
            config::set_pinned_clipboard(cfg, &text, pinned)
        })
        .is_err()
        {
            list = clipboard::toggle_clipboard_pin(&id);
        }
    }
    if state.start_open.load(Ordering::Acquire) {
        let _ = app.emit_to("start", "clipboard-updated", &list);
    }
    list
}

#[tauri::command]
fn delete_clipboard_entry(
    app: AppHandle,
    state: State<MacAppState>,
    id: String,
) -> Vec<ClipboardEntry> {
    let removed_text = clipboard::get_clipboard_history()
        .into_iter()
        .find(|c| c.id == id && c.pinned)
        .map(|c| c.text);
    let list = clipboard::delete_clipboard_entry(&id);
    if let Some(text) = removed_text {
        let _ = mutate_config_and_emit(&app, &state, |cfg| {
            config::set_pinned_clipboard(cfg, &text, false)
        });
    }
    if state.start_open.load(Ordering::Acquire) {
        let _ = app.emit_to("start", "clipboard-updated", &list);
    }
    list
}

#[tauri::command]
fn set_clipboard_paused(paused: bool) -> bool {
    clipboard::set_clipboard_paused(paused)
}

#[tauri::command]
fn get_clipboard_paused() -> bool {
    clipboard::get_clipboard_paused()
}

#[tauri::command]
async fn paste_clipboard_entry(app: AppHandle, text: String) -> Result<(), String> {
    let target_pid = app
        .state::<MacAppState>()
        .prev_frontmost_pid
        .load(Ordering::Acquire);
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);
    let h = app.clone();
    let dispatched = app.run_on_main_thread(move || {
        hide_start(&h, true);
        let _ = tx.send(());
    });
    if dispatched.is_ok() {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _ = rx.recv_timeout(std::time::Duration::from_millis(200));
        })
        .await;
    }

    tauri::async_runtime::spawn_blocking(move || {
        clipboard::copy_to_clipboard(&text)?;
        std::thread::sleep(std::time::Duration::from_millis(70));
        if target_pid > 0 && target_pid != std::process::id() as i32 {
            let _ = bridge_ffi::activate_pid(target_pid);
            std::thread::sleep(std::time::Duration::from_millis(35));
        }
        bridge_ffi::send_cmd_v();
        Ok(())
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
fn clear_unpinned_clips(app: AppHandle, state: State<MacAppState>) -> Vec<ClipboardEntry> {
    let list = clipboard::clear_unpinned_clips();
    if state.start_open.load(Ordering::Acquire) {
        let _ = app.emit_to("start", "clipboard-updated", &list);
    }
    list
}

#[tauri::command]
async fn list_directory(
    path: String,
    sort_by: Option<String>,
    sort_dir: Option<String>,
    show_hidden: Option<bool>,
) -> Result<DirectoryListing, String> {
    tauri::async_runtime::spawn_blocking(move || {
        file_explorer::list_directory(
            &path,
            sort_by.as_deref(),
            sort_dir.as_deref(),
            show_hidden.unwrap_or(false),
        )
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_file_thumbnails(
    paths: Vec<String>,
    size: Option<u32>,
) -> Result<HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        file_explorer::get_file_thumbnails(paths, size.unwrap_or(48))
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn fs_open_item(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || file_explorer::fs_open_item(&path))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn fs_create_folder(parent_path: String, name: String) -> Result<FileEntry, String> {
    tauri::async_runtime::spawn_blocking(move || {
        file_explorer::fs_create_folder(&parent_path, &name)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn fs_rename_item(path: String, new_name: String) -> Result<FileEntry, String> {
    tauri::async_runtime::spawn_blocking(move || file_explorer::fs_rename_item(&path, &new_name))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn fs_copy_files_to_clipboard(paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || file_explorer::fs_copy_files_to_clipboard(paths))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn fs_trash_items(paths: Vec<String>) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || file_explorer::fs_trash_items(paths))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn fs_open_in_explorer(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || file_explorer::fs_open_in_explorer(&path))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_weather(
    app: AppHandle,
    city: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    force: Option<bool>,
) -> Result<WeatherSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let has_explicit_city = city
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty());
        let has_explicit_coords =
            lat.is_some_and(f64::is_finite) && lon.is_some_and(f64::is_finite);
        let (eff_city, eff_lat, eff_lon) = if has_explicit_city || has_explicit_coords {
            (city, lat, lon)
        } else {
            let st = app.state::<MacAppState>();
            let cfg = lock_unpoisoned(&st.config);
            let trimmed = cfg.taskbar_widgets.weather_city.trim();
            let c_opt = (!trimmed.is_empty()).then(|| trimmed.to_string());
            (
                c_opt,
                cfg.taskbar_widgets.weather_lat,
                cfg.taskbar_widgets.weather_lon,
            )
        };
        weather::get_weather(eff_city.as_deref(), eff_lat, eff_lon, force.unwrap_or(false))
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn search_weather_cities(query: String) -> Result<Vec<WeatherCityOption>, String> {
    tauri::async_runtime::spawn_blocking(move || weather::search_weather_cities(&query))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_network_snapshot(probe_ping: Option<bool>) -> Result<NetworkSnapshot, String> {
    let do_probe = probe_ping.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || network::get_network_snapshot(do_probe))
        .await
        .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_quick_toggles_state() -> Result<QuickTogglesState, String> {
    tauri::async_runtime::spawn_blocking(quick_toggles::get_quick_toggles_state)
        .await
        .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn set_keep_awake(enable: bool) -> Result<QuickTogglesState, String> {
    tauri::async_runtime::spawn_blocking(move || quick_toggles::set_keep_awake(enable))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn open_screen_snip(app: AppHandle) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(1);
    let h = app.clone();
    let dispatched = app.run_on_main_thread(move || {
        hide_start(&h, true);
        let _ = tx.send(());
    });
    if dispatched.is_ok() {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _ = rx.recv_timeout(std::time::Duration::from_millis(200));
        })
        .await;
    }
    tauri::async_runtime::spawn_blocking(|| {
        std::thread::sleep(std::time::Duration::from_millis(160));
        quick_toggles::trigger_screenshot()
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn trigger_screenshot(app: AppHandle) -> Result<(), String> {
    open_screen_snip(app).await
}

#[tauri::command]
async fn empty_recycle_bin() -> Result<QuickTogglesState, String> {
    tauri::async_runtime::spawn_blocking(quick_toggles::empty_recycle_bin_and_refresh)
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_peripherals(app: AppHandle) -> Result<Vec<PeripheralBattery>, String> {
    let hidden = lock_unpoisoned(&app.state::<MacAppState>().config)
        .hidden_battery_ids
        .clone();
    tauri::async_runtime::spawn_blocking(move || {
        filter_visible_batteries(battery::get_peripheral_batteries(), &hidden)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_peripheral_batteries(app: AppHandle) -> Result<Vec<PeripheralBattery>, String> {
    get_peripherals(app).await
}

#[tauri::command]
async fn refresh_peripheral_batteries(app: AppHandle) -> Result<Vec<PeripheralBattery>, String> {
    let hidden = lock_unpoisoned(&app.state::<MacAppState>().config)
        .hidden_battery_ids
        .clone();
    let h = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let list = filter_visible_batteries(battery::refresh_peripheral_batteries(), &hidden);
        let _ = h.emit_to("start", "peripherals-updated", &list);
        list
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

pub fn run() {
    let initial_cfg = config::load_config();
    let initial_pinned_clipboard = initial_cfg.pinned_clipboard.clone();
    let initial_tap_mode = CommandTapMode::from_config_str(&initial_cfg.mac_command_key);
    let initial_pos_mode = initial_cfg.mac_window_position.clone();

    let start_open = Arc::new(AtomicBool::new(false));
    let start_gen = Arc::new(AtomicU64::new(0));
    let last_show_ms = Arc::new(AtomicU64::new(0));
    let modal_picker_depth = Arc::new(AtomicU32::new(0));
    let prev_frontmost_pid = Arc::new(AtomicI32::new(0));
    let index_refresh_running = Arc::new(AtomicBool::new(false));
    let command_tap = Arc::new(Mutex::new(CommandTapMachine::new(initial_tap_mode)));
    let command_tap_started = Arc::new(AtomicBool::new(false));

    let app_state = MacAppState {
        config: Mutex::new(initial_cfg),
        icon_cache: Mutex::new(IconCache::default()),
        icon_sources: Mutex::new(HashMap::new()),
        installed_apps: Mutex::new(Vec::new()),
        indexed_files: Mutex::new(Vec::new()),
        index_refresh_running: Arc::clone(&index_refresh_running),
        start_open: Arc::clone(&start_open),
        start_gen: Arc::clone(&start_gen),
        last_show_ms: Arc::clone(&last_show_ms),
        modal_picker_depth: Arc::clone(&modal_picker_depth),
        prev_frontmost_pid: Arc::clone(&prev_frontmost_pid),
        command_tap: Arc::clone(&command_tap),
        command_tap_started: Arc::clone(&command_tap_started),
        tray_icon: Mutex::new(None),
    };

    let app = tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_platform_info,
            get_accessibility_status,
            open_accessibility_settings,
            set_mac_settings,
            quit_app,
            get_config,
            set_autostart,
            set_appearance_settings,
            toggle_start,
            open_start_settings,
            surface_commit_ack,
            surface_exit_ack,
            get_backdrop_mode,
            animate_surface_in,
            animate_surface_out,
            dismiss_overlays,
            get_icons,
            resolve_pin_icons,
            get_icon_for_path,
            get_installed_apps,
            get_indexed_files,
            refresh_indexes,
            get_open_windows,
            get_steam_games,
            get_audio_state,
            get_media_state,
            get_audio_peaks,
            add_pin,
            update_pin,
            remove_pin,
            reorder_pins,
            move_pin_in_space,
            add_custom_space,
            update_custom_space,
            delete_custom_space,
            create_space,
            update_space,
            delete_space,
            reorder_spaces,
            set_space_widgets,
            set_space_widget_layout,
            set_taskbar_widgets,
            save_quick_notes_text,
            add_quick_todo,
            toggle_quick_todo,
            delete_quick_todo,
            clear_done_quick_todos,
            hide_tray_app,
            restore_tray_app,
            restore_all_tray_apps,
            hide_battery_device,
            restore_battery_device,
            restore_all_battery_devices,
            record_frecency,
            pick_folder,
            pick_file,
            pick_custom_icon_image,
            export_config_dialog,
            import_config_dialog,
            get_telemetry,
            kill_process_pid,
            kill_process_group,
            launch_item,
            launch_new_window,
            launch_combo,
            open_location,
            open_terminal_at,
            power_action,
            get_clipboard_history,
            copy_to_clipboard,
            toggle_clip_pin,
            delete_clipboard_entry,
            set_clipboard_paused,
            get_clipboard_paused,
            paste_clipboard_entry,
            clear_unpinned_clips,
            list_directory,
            get_file_thumbnails,
            fs_open_item,
            fs_create_folder,
            fs_rename_item,
            fs_copy_files_to_clipboard,
            fs_trash_items,
            fs_open_in_explorer,
            get_weather,
            search_weather_cities,
            get_network_snapshot,
            get_quick_toggles_state,
            set_keep_awake,
            trigger_screenshot,
            open_screen_snip,
            empty_recycle_bin,
            get_peripherals,
            get_peripheral_batteries,
            refresh_peripheral_batteries,
        ])
        .setup(move |app| {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();

            // Build native macOS Menu Bar TrayIcon
            let open_item =
                MenuItem::with_id(app, "tray-open", "Открыть TestBar (⌘)", true, None::<&str>)?;
            let settings_item =
                MenuItem::with_id(app, "tray-settings", "Настройки", true, None::<&str>)?;
            let ax_item = MenuItem::with_id(
                app,
                "tray-accessibility",
                "Проверить доступ к клавише ⌘",
                true,
                None::<&str>,
            )?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "tray-quit", "Выход", true, None::<&str>)?;
            let tray_menu =
                Menu::with_items(app, &[&open_item, &settings_item, &ax_item, &sep, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::with_id("testbar-tray")
                .menu(&tray_menu)
                .tooltip("TestBar")
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "tray-open" => {
                        let h = app.clone();
                        let _ = app.run_on_main_thread(move || show_start(&h));
                    }
                    "tray-settings" => {
                        let h = app.clone();
                        let _ = app.run_on_main_thread(move || {
                            show_start(&h);
                            let _ = h.emit_to("start", "open-start-settings", ());
                        });
                    }
                    "tray-accessibility" => {
                        let trusted = bridge_ffi::is_accessibility_trusted(true);
                        let _ = app.emit_to(
                            "start",
                            "accessibility-status-updated",
                            serde_json::json!({ "trusted": trusted }),
                        );
                        if !trusted {
                            bridge_ffi::open_accessibility_settings();
                        }
                        let h = app.clone();
                        let _ = app.run_on_main_thread(move || show_start(&h));
                    }
                    "tray-quit" => {
                        quick_toggles::shutdown();
                        app.exit(0);
                    }
                    _ => {}
                });

            if let Some(img) = decode_png_rgba(TRAY_ICON_PNG) {
                tray_builder = tray_builder.icon(img);
            } else if let Some(def_img) = app.default_window_icon() {
                tray_builder = tray_builder.icon(def_img.clone());
            }
            if let Ok(tray) = tray_builder.build(app) {
                *lock_unpoisoned(&app.state::<MacAppState>().tray_icon) = Some(tray);
            }

            // Configure pre-warmed transparent floating Start window
            if let Some(start_win) = app.get_webview_window("start") {
                if let Ok(ns_win) = start_win.ns_window() {
                    bridge_ffi::configure_start_window(ns_win);
                    bridge_ffi::position_start_window(
                        ns_win,
                        START_WIDTH_LOGICAL,
                        START_HEIGHT_LOGICAL,
                        &initial_pos_mode,
                    );
                }
                let h_blur = handle.clone();
                start_win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::Focused(false) = ev {
                        let st = h_blur.state::<MacAppState>();
                        if st.modal_picker_depth.load(Ordering::Acquire) > 0 {
                            return;
                        }
                        if !st.start_open.load(Ordering::Acquire) {
                            return;
                        }
                        let elapsed =
                            monotonic_ms().saturating_sub(st.last_show_ms.load(Ordering::Acquire));
                        if elapsed < START_BLUR_GRACE_MS {
                            return;
                        }
                        hide_start(&h_blur, false);
                    }
                });
            }

            // Start lone-Command CGEventTap listener (re-tried every 2s until Accessibility granted)
            let _ = TAP_CALLBACK_CTX.set(TapCallbackContext {
                app: handle.clone(),
                machine: Arc::clone(&command_tap),
            });
            let initial_trusted = bridge_ffi::is_accessibility_trusted(false);
            if initial_trusted
                && bridge_ffi::start_command_tap(on_cg_tap_event, std::ptr::null_mut())
            {
                command_tap_started.store(true, Ordering::Release);
            }

            if !initial_trusted {
                let h_first = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                    let h = h_first.clone();
                    let _ = h_first.run_on_main_thread(move || {
                        show_start(&h);
                    });
                });
            }

            // Accessibility permission monitor (starts CGEventTap immediately once granted)
            let h_ax = handle.clone();
            let tap_started_flag = Arc::clone(&command_tap_started);
            std::thread::spawn(move || {
                let mut was_trusted = initial_trusted;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let now_trusted = bridge_ffi::is_accessibility_trusted(false);
                    if now_trusted && !tap_started_flag.load(Ordering::Acquire) {
                        if bridge_ffi::start_command_tap(on_cg_tap_event, std::ptr::null_mut()) {
                            tap_started_flag.store(true, Ordering::Release);
                        }
                    }
                    if now_trusted != was_trusted {
                        was_trusted = now_trusted;
                        let _ = h_ax.emit_to(
                            "start",
                            "accessibility-status-updated",
                            serde_json::json!({ "trusted": now_trusted }),
                        );
                    }
                }
            });

            // Background apps & user files indexer (startup + every 15 minutes)
            let h_idx = handle.clone();
            std::thread::spawn(move || loop {
                run_background_index_refresh(&h_idx);
                std::thread::sleep(std::time::Duration::from_secs(
                    PERIODIC_REINDEX_INTERVAL_SECS,
                ));
            });

            // Background clipboard watcher
            clipboard::hydrate_pinned_clipboard(&initial_pinned_clipboard);
            let h_clip = handle.clone();
            let clip_open_flag = Arc::clone(&start_open);
            std::thread::spawn(move || loop {
                if let Some(entries) = clipboard::poll_clipboard_into_history() {
                    if clip_open_flag.load(Ordering::Acquire) {
                        let _ = h_clip.emit_to("start", "clipboard-updated", &entries);
                    }
                }
                let delay_ms = if clip_open_flag.load(Ordering::Acquire) {
                    700
                } else {
                    1500
                };
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            });

            // Visibility-gated telemetry watcher (2s only when Start is open)
            let h_tel = handle.clone();
            let tel_open_flag = Arc::clone(&start_open);
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if tel_open_flag.load(Ordering::Acquire) {
                    let st = h_tel.state::<MacAppState>();
                    let tel = telemetry::get_telemetry_cached(&st.icon_cache);
                    if tel_open_flag.load(Ordering::Acquire) {
                        let _ = h_tel.emit_to("start", "telemetry-updated", &tel);
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error building TestBar macOS");

    app.run(|_handle, event| {
        if let tauri::RunEvent::Exit = event {
            quick_toggles::shutdown();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_app_never_exposes_save_config_or_accepts_full_config() {
        let src = include_str!("app.rs");
        let marker = "tauri::generate_handler![";
        let start = src.find(marker).expect("generate_handler! must exist") + marker.len();
        let tail = &src[start..];
        let end = tail.find("])").expect("end of generate_handler!");
        let block = &tail[..end];
        let handlers: Vec<&str> = block
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .flat_map(|l| l.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        assert!(!handlers.contains(&"save_config"));

        let mut cursor = 0usize;
        while let Some(rel) = src[cursor..].find("#[tauri::command]") {
            let attr_pos = cursor + rel;
            let line_end = src[attr_pos..]
                .find('\n')
                .map(|i| attr_pos + i)
                .unwrap_or(src.len());
            cursor = line_end;
            let after = &src[line_end..];
            let Some(fn_rel) = after.find("fn ") else {
                continue;
            };
            let fn_slice = &after[fn_rel..];
            let Some(open_p) = fn_slice.find('(') else {
                continue;
            };
            let mut depth = 0usize;
            let mut close_p = None;
            for (idx, ch) in fn_slice[open_p..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close_p = Some(open_p + idx);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close_p = close_p.expect("closing paren");
            let params = &fn_slice[open_p + 1..close_p];
            assert!(
                !params.contains("TestBarConfig"),
                "command must not accept TestBarConfig: {params}"
            );
        }
    }
}
