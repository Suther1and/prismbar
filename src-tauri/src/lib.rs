#[cfg(target_os = "windows")]
pub mod apps_indexer;
#[cfg(target_os = "windows")]
pub mod audio;
#[cfg(target_os = "windows")]
pub mod backdrop;
#[cfg(target_os = "windows")]
pub mod battery;
pub mod config;
#[cfg(target_os = "windows")]
pub mod coordinator;
#[cfg(target_os = "windows")]
pub mod file_explorer;
#[cfg(target_os = "windows")]
pub mod icon_cache;
#[cfg(target_os = "windows")]
pub mod media;
#[cfg(target_os = "windows")]
pub mod network_monitor;
#[cfg(target_os = "windows")]
pub mod quick_toggles;
#[cfg(target_os = "windows")]
pub mod system_widgets;
#[cfg(target_os = "windows")]
pub mod telemetry;
#[cfg(target_os = "windows")]
pub mod weather;
#[cfg(target_os = "windows")]
pub mod win_shell;
#[cfg(target_os = "windows")]
pub mod windows_tracker;

#[path = "macos/command_tap.rs"]
pub mod mac_command_tap;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub fn run(_hide_native: bool) {
    macos::app::run();
}

#[cfg(all(test, target_os = "windows"))]
pub(crate) static WIN32_TOPMOST_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(target_os = "windows")]
mod windows_desktop {
    use super::*;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use apps_indexer::{IndexedApp, IndexedFile, SteamGame};
use audio::{AudioMasterState, AudioPeaksSnapshot, AudioState};
use backdrop::BackdropInsets;
use config::{CustomSpace, PinItem, SpaceHeaderWidget, TestBarConfig};
use coordinator::{
    BarMode, CoordinatorActions, FlyoutKind, ModalPickerGuard, PixelRect, WindowCoordinator,
    DOCK_APPBAR_LOGICAL_HEIGHT, EDGE_APPBAR_LOGICAL_HEIGHT, FLYOUT_ROOT_PAD_LOGICAL,
    START_ROOT_PAD_LOGICAL,
};
use icon_cache::IconCache;
use media::MediaState;
use network_monitor::NetworkSnapshot;
use quick_toggles::QuickTogglesState;
use system_widgets::{ClipboardEntry, NetworkInfo, PeripheralBattery, SessionAlert, TrayAppItem};
use telemetry::TelemetryState;
use win_shell::{ShellAppBarEvent, ShellHookEvent, WatchdogArgs};
use windows_tracker::{AppWindowGroup, JumpListItem};

const DOCK_BAR_LOGICAL_HEIGHT: f64 = 58.0;
const DOCK_TOP_PAD_LOGICAL: f64 = 16.0;
const DOCK_SIDE_PAD_LOGICAL: f64 = 16.0;
const DOCK_BOTTOM_PAD_LOGICAL: f64 = 9.0;
const DOCK_WINDOW_LOGICAL_HEIGHT: f64 =
    DOCK_TOP_PAD_LOGICAL + DOCK_BAR_LOGICAL_HEIGHT + DOCK_BOTTOM_PAD_LOGICAL; // 83px
const DOCK_HORIZONTAL_PADDING_LOGICAL: i32 = (2.0 * DOCK_SIDE_PAD_LOGICAL) as i32; // 32px (16px L + 16px R)
const DOCK_MIN_CONTENT_WIDTH_LOGICAL: i32 = 110;
const SECONDARY_DOCK_DEFAULT_CONTENT_WIDTH_LOGICAL: i32 = 160;
const START_DEFAULT_LOGICAL_WIDTH: f64 = 1140.0 + 2.0 * START_ROOT_PAD_LOGICAL; // 1188px (24px shadow pad)
const START_DEFAULT_LOGICAL_HEIGHT: f64 = 680.0 + 2.0 * START_ROOT_PAD_LOGICAL; // 728px (24px shadow pad)
const FLYOUT_ROOT_PAD_TOTAL_LOGICAL: f64 = 2.0 * FLYOUT_ROOT_PAD_LOGICAL; // 32px (16px shadow pad on all 4 sides)
const FLYOUT_EXIT_ANIM_MS: u64 = 130;
const START_EXIT_ANIM_MS: u64 = 135;
const PREVIEW_GLIDE_DURATION_MS: u64 = 115;
const PERIODIC_REINDEX_INTERVAL_SECS: u64 = 15 * 60;
const MAX_ICON_SOURCES_ENTRIES: usize = 2048;
const MAX_JUMP_LIST_CACHE_ENTRIES: usize = 128;

static MONOTONIC_EPOCH: OnceLock<std::time::Instant> = OnceLock::new();

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlyoutHideEvent {
    pub gen: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveSurfaceMetrics {
    pub gen: u64,
    pub rect: PixelRect,
    pub scale_factor: f64,
    pub bottom_anchor_y: i32,
    pub monitor_top: i32,
    pub max_h_logical: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct PendingSurfaceExit {
    pub gen: u64,
}

/// Tracks the exact last-delivered payloads sent to the hidden/visible `"start"` WebView so
/// repeated `Win` presses skip serializing & emitting unchanged JSON payloads on frame 0.
#[derive(Debug, Default, Clone)]
pub struct StartSentSnapshot {
    pub config_rev: Option<u64>,
    pub(crate) windows_fp: Option<WindowsFingerprint>,
    pub clips_sig: Option<u64>,
    pub media_sig: Option<u64>,
    pub audio: Option<AudioState>,
}

pub struct AppState {
    pub config: Mutex<TestBarConfig>,
    pub icon_cache: Mutex<IconCache>,
    pub icon_sources: Mutex<HashMap<String, String>>,
    pub last_window_groups: Mutex<Vec<AppWindowGroup>>,
    pub cached_tray_apps: Mutex<Vec<TrayAppItem>>,
    pub tray_cache_warmed: Arc<AtomicBool>,
    pub jump_list_cache: Mutex<HashMap<String, Vec<JumpListItem>>>,
    pub last_audio_state: Mutex<Option<AudioState>>,
    pub installed_apps: Mutex<Vec<IndexedApp>>,
    pub indexed_files: Mutex<Vec<IndexedFile>>,
    pub steam_games: Mutex<Vec<SteamGame>>,
    pub hide_native: bool,
    pub coordinator: Mutex<WindowCoordinator>,
    pub overlay_open: Arc<AtomicBool>,
    pub fullscreen_suppressed: Arc<AtomicBool>,
    pub secondary_fullscreen_suppressed: Arc<AtomicBool>,
    pub taskbar_content_width: Mutex<i32>,
    pub secondary_taskbar_content_width: Mutex<Option<i32>>,
    pub secondary_taskbar_visible: Arc<AtomicBool>,
    pub primary_taskbar_raster_dpi: AtomicU32,
    pub secondary_taskbar_raster_dpi: AtomicU32,
    pub last_flyout_req: Mutex<Option<FlyoutRequest>>,
    pub last_flyout_bounds: Mutex<Option<(FlyoutKind, PixelRect)>>,
    pub active_start_metrics: Mutex<Option<ActiveSurfaceMetrics>>,
    pub active_flyout_metrics: Mutex<Option<ActiveSurfaceMetrics>>,
    pub last_start_rect: Mutex<Option<PixelRect>>,
    pub start_sent: Mutex<StartSentSnapshot>,
    pub flyout_sent_audio: Mutex<Option<AudioState>>,
    pub flyout_sent_media_sig: Mutex<Option<u64>>,
    pub pending_start_exit: Mutex<Option<PendingSurfaceExit>>,
    pub pending_flyout_exit: Mutex<Option<PendingSurfaceExit>>,
    pub flyout_anim_gen: Arc<AtomicU64>,
    pub start_anim_gen: Arc<AtomicU64>,
    pub start_backdrop_shown_gen: Arc<AtomicU64>,
    pub flyout_backdrop_shown_gen: Arc<AtomicU64>,
    pub flyout_glide_gen: Arc<AtomicU64>,
    pub dpi_override: AtomicU32,
    pub last_external_fg: Arc<AtomicIsize>,
    pub index_refresh_running: Arc<AtomicBool>,
    pub start_hydration_running: Arc<AtomicBool>,
}

impl AppState {
    fn sync_overlay_flag(&self, coord: &WindowCoordinator) {
        let overlay_open = coord.has_open_overlay();
        self.overlay_open.store(overlay_open, Ordering::Release);
        win_shell::set_overlay_open_hint(overlay_open);
        self.fullscreen_suppressed
            .store(coord.is_primary_fullscreen(), Ordering::Release);
        self.secondary_fullscreen_suppressed
            .store(coord.is_secondary_fullscreen(), Ordering::Release);
    }

    fn set_dpi_override(&self, dpi: u32) {
        self.dpi_override.store(dpi, Ordering::Release);
    }

    fn clear_dpi_override(&self) {
        self.dpi_override.store(0, Ordering::Release);
    }

    fn current_dpi_override(&self) -> Option<u32> {
        let dpi = self.dpi_override.load(Ordering::Acquire);
        (dpi > 0).then_some(dpi)
    }

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

fn capture_external_foreground_hwnd(state: &AppState) -> isize {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindow,
    };
    unsafe {
        let fg = GetForegroundWindow();
        if !fg.0.is_null() && IsWindow(fg).as_bool() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(fg, Some(&mut pid));
            if pid != 0 && pid != GetCurrentProcessId() {
                let raw = fg.0 as isize;
                state.last_external_fg.store(raw, Ordering::Release);
                return raw;
            }
        }
    }
    state.last_external_fg.load(Ordering::Acquire)
}

fn build_pid_to_hwnd_map(groups: &[AppWindowGroup]) -> HashMap<u32, isize> {
    let mut map = HashMap::new();
    for g in groups {
        for w in &g.windows {
            if w.pid != 0 && w.hwnd != 0 {
                if w.is_active {
                    map.insert(w.pid, w.hwnd);
                } else {
                    map.entry(w.pid).or_insert(w.hwnd);
                }
            }
        }
    }
    map
}

fn sample_audio_state_with_icon_sources(state: &AppState) -> AudioState {
    let aud = match audio::get_audio_state_with_icons_mutex(&state.icon_cache) {
        Ok((aud, sources)) => {
            state.register_icon_sources(sources);
            aud
        }
        Err(_) => audio::get_audio_state(),
    };
    *lock_unpoisoned(&state.last_audio_state) = Some(aud.clone());
    aud
}

/// Computes exact compact 6-column Tray Overflow card logical height matching `flyoutLogic.ts`.
fn compute_tray_flyout_height_logical(app_count: usize) -> i32 {
    let rows = ((app_count + 5) / 6).max(1) as i32;
    let grid_h = (rows * 34 + (rows - 1).max(0) * 4).min(224);
    (grid_h + 50).max(128)
}

/// Refreshes `state.cached_tray_apps` from Win11 `NotifyIconSettings` + live Session-1 processes,
/// emitting `tray-updated` to `"flyout"` whenever `emit_on_change` is true and the snapshot changed.
fn refresh_cached_tray_apps(app: &AppHandle, emit_on_change: bool) -> Vec<TrayAppItem> {
    let st = app.state::<AppState>();
    let open_groups = {
        let cached = lock_unpoisoned(&st.last_window_groups).clone();
        if cached.is_empty() {
            let (fresh, sources) =
                windows_tracker::list_open_window_groups_cached_with_sources(&st.icon_cache);
            st.register_icon_sources(sources);
            *lock_unpoisoned(&st.last_window_groups) = fresh.clone();
            fresh
        } else {
            cached
        }
    };
    let hidden_keys = lock_unpoisoned(&st.config).hidden_tray_keys.clone();
    let (tray_items, tray_sources) =
        system_widgets::list_tray_background_apps_filtered_with_open_mutex(
            &st.icon_cache,
            &open_groups,
            &hidden_keys,
        );
    st.register_icon_sources(tray_sources);

    let changed = {
        let mut slot = lock_unpoisoned(&st.cached_tray_apps);
        let was_warmed = st.tray_cache_warmed.swap(true, Ordering::AcqRel);
        let diff = !was_warmed || *slot != tray_items;
        if diff {
            *slot = tray_items.clone();
        }
        diff
    };
    if emit_on_change && changed {
        let _ = app.emit_to("flyout", "tray-updated", &tray_items);
    }
    tray_items
}

fn jump_list_cache_key(exe_path: &str, aumid: Option<&str>) -> String {
    let exe_norm = exe_path.trim().to_ascii_lowercase();
    let aumid_norm = aumid
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    format!("{exe_norm}\0{aumid_norm}")
}

/// Exact Path 1 DComp backdrop insets and corner radius for `taskbar` / `taskbar-secondary`.
fn taskbar_backdrop_insets_and_radius(bar_mode: &str, sf: f64) -> (BackdropInsets, f32) {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    if BarMode::from_config_str(bar_mode) == BarMode::Edge {
        (BackdropInsets::ZERO, 0.0)
    } else {
        let side = (DOCK_SIDE_PAD_LOGICAL * sf).round() as i32;
        let top = (DOCK_TOP_PAD_LOGICAL * sf).round() as i32;
        let bottom = (DOCK_BOTTOM_PAD_LOGICAL * sf).round() as i32;
        (BackdropInsets::new(side, top, side, bottom), (18.0 * sf) as f32)
    }
}

/// Exact Path 1 DComp backdrop insets and corner radius for `start`.
fn start_backdrop_insets_and_radius(sf: f64) -> (BackdropInsets, f32) {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    let pad = (START_ROOT_PAD_LOGICAL * sf).round() as i32;
    (BackdropInsets::uniform(pad), (26.0 * sf) as f32)
}

/// Exact Path 1 DComp backdrop insets and corner radius for `flyout` by `FlyoutKind`.
fn flyout_backdrop_insets_and_radius(kind: FlyoutKind, sf: f64) -> (BackdropInsets, f32) {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    let pad = (FLYOUT_ROOT_PAD_LOGICAL * sf).round() as i32;
    let radius_logical = match kind {
        FlyoutKind::Calendar | FlyoutKind::Weather => 18.0,
        FlyoutKind::Preview => 15.0,
        FlyoutKind::Osd => 21.0,
        FlyoutKind::Tray
        | FlyoutKind::Sound
        | FlyoutKind::Battery
        | FlyoutKind::Network
        | FlyoutKind::Telemetry
        | FlyoutKind::ContextApp
        | FlyoutKind::ContextBar => 16.0,
    };
    (BackdropInsets::uniform(pad), (radius_logical * sf) as f32)
}

/// Neutralizes Windows Accessibility `TextScaleFactor` on a WebView2 window by disabling
/// automatic monitor scale detection and locking `RasterizationScale` strictly to `dpi / 96.0`.
fn set_webview_rasterization_scale(win: &WebviewWindow, scale: f64) {
    let clamped = if scale.is_finite() && scale > 0.0 {
        scale.clamp(0.5, 4.0)
    } else {
        1.0
    };
    let _ = win.with_webview(move |wv| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller3;
        use windows_core_wv2::Interface;
        if let Ok(ctrl3) = wv.controller().cast::<ICoreWebView2Controller3>() {
            let _ = ctrl3.SetShouldDetectMonitorScaleChanges(false);
            let _ = ctrl3.SetRasterizationScale(clamped);
        }
    });
}

fn ensure_taskbar_dpi_only_scale(win: &WebviewWindow, cache: &AtomicU32, dpi: u32) {
    let effective_dpi = dpi.max(96);
    if cache.swap(effective_dpi, Ordering::AcqRel) != effective_dpi {
        set_webview_rasterization_scale(win, effective_dpi as f64 / 96.0);
    }
}

/// Switches a WebView2 window's memory usage target level between `LOW` (when hidden) and
/// `NORMAL` (when visible) via `ICoreWebView2_19::SetMemoryUsageTargetLevel`, aggressively
/// trimming Chromium/V8 heap on idle overlays without destroying pre-warmed state (AD-1).
fn set_webview_memory_low(win: &WebviewWindow, low: bool) {
    let _ = win.with_webview(move |wv| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core_wv2::Interface;
        if let Ok(core) = wv.controller().CoreWebView2() {
            if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                let level = if low {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                } else {
                    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                };
                let _ = core19.SetMemoryUsageTargetLevel(level);
            }
        }
    });
}

fn cancel_flyout_glide(state: &AppState) {
    state.flyout_glide_gen.fetch_add(1, Ordering::AcqRel);
    win_shell::cancel_window_x_glide();
}

/// Smoothly glides the Preview flyout AND its DComp underlay horizontally in lockstep on DWM vblank.
fn glide_flyout_with_backdrop(
    glide_gen: Arc<AtomicU64>,
    hwnd_raw: isize,
    from_x: i32,
    to_x: i32,
    y: i32,
    duration_ms: u64,
) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::DwmFlush;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsWindow, IsWindowVisible, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };

    let my_gen = glide_gen.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    win_shell::cancel_window_x_glide();
    if hwnd_raw == 0 {
        return;
    }
    if duration_ms == 0 || from_x == to_x {
        unsafe {
            let hwnd = HWND(hwnd_raw as *mut _);
            if IsWindow(hwnd).as_bool() {
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    to_x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        backdrop::sync_position("flyout", hwnd_raw, to_x, y);
        return;
    }

    let duration_f = duration_ms.clamp(1, 1000) as f64;
    std::thread::spawn(move || unsafe {
        let hwnd = HWND(hwnd_raw as *mut _);
        let started = std::time::Instant::now();
        let mut last_x = from_x;
        loop {
            if glide_gen.load(Ordering::Acquire) != my_gen {
                break;
            }
            if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
                break;
            }
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let progress = (elapsed_ms / duration_f).min(1.0);
            let cur_x = win_shell::interpolate_glide_x(from_x, to_x, progress);
            if cur_x != last_x || progress >= 1.0 {
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    cur_x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
                backdrop::sync_position("flyout", hwnd_raw, cur_x, y);
                last_x = cur_x;
            }
            if progress >= 1.0 {
                break;
            }
            if DwmFlush().is_err() {
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
        }
    });
}

/// Double-lock against cross-monitor clobbering: emit the complete `all_groups` snapshot to every
/// targeted webview (`taskbar`, `taskbar-secondary`, `start`, `flyout`) while JS listeners are
/// strictly window-scoped (`getCurrentWebviewWindow().listen`) and each `TaskbarView` filters
/// strictly for its own monitor via `filterGroupsForMonitor(groups, monitor)`.
fn emit_windows_to_targets(app: &AppHandle, targets: &[&str], all_groups: &[AppWindowGroup]) {
    let mut start_sent = false;
    for &target in targets {
        if target == "start" {
            start_sent = true;
        }
        let _ = app.emit_to(target, "windows-updated", all_groups);
    }
    if start_sent {
        let fp = compute_windows_fingerprint(all_groups);
        lock_unpoisoned(&app.state::<AppState>().start_sent).windows_fp = Some(fp);
    }
}

fn hash_clipboard_entries(entries: &[ClipboardEntry]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    entries.len().hash(&mut h);
    for e in entries {
        e.id.hash(&mut h);
        e.tag.hash(&mut h);
        e.pinned.hash(&mut h);
        e.text.hash(&mut h);
        e.kind.hash(&mut h);
        e.color.hash(&mut h);
    }
    h.finish()
}

fn hash_media_state(media: &MediaState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    media.has_session.hash(&mut h);
    media.is_playing.hash(&mut h);
    media.title.hash(&mut h);
    media.artist.hash(&mut h);
    media.source.hash(&mut h);
    media.active_session_id.hash(&mut h);
    media.can_seek.hash(&mut h);
    ((media.position_sec * 2.0).round() as i64).hash(&mut h);
    ((media.duration_sec * 2.0).round() as i64).hash(&mut h);
    media.thumbnail.as_deref().map(str::len).hash(&mut h);
    media.thumbnail.as_deref().and_then(|s| s.get(..48)).hash(&mut h);
    media.sessions.len().hash(&mut h);
    for s in &media.sessions {
        s.id.hash(&mut h);
        s.app_name.hash(&mut h);
        s.title.hash(&mut h);
        s.artist.hash(&mut h);
        s.is_playing.hash(&mut h);
        s.is_current.hash(&mut h);
    }
    h.finish()
}

fn emit_start_config_if_dirty(app: &AppHandle, state: &AppState, cfg: &TestBarConfig) {
    let dirty = {
        let mut sent = lock_unpoisoned(&state.start_sent);
        if sent.config_rev == Some(cfg.revision) {
            false
        } else {
            sent.config_rev = Some(cfg.revision);
            true
        }
    };
    if dirty {
        let _ = app.emit_to("start", "config-updated", cfg);
    }
}

fn emit_start_windows_if_dirty(app: &AppHandle, state: &AppState, groups: &[AppWindowGroup]) {
    let fp = compute_windows_fingerprint(groups);
    let dirty = {
        let mut sent = lock_unpoisoned(&state.start_sent);
        if sent.windows_fp.as_ref() == Some(&fp) {
            false
        } else {
            sent.windows_fp = Some(fp);
            true
        }
    };
    if dirty {
        let _ = app.emit_to("start", "windows-updated", groups);
    }
}

fn emit_start_clips_if_dirty(app: &AppHandle, state: &AppState, clips: &[ClipboardEntry]) {
    let sig = hash_clipboard_entries(clips);
    let dirty = {
        let mut sent = lock_unpoisoned(&state.start_sent);
        if sent.clips_sig == Some(sig) {
            false
        } else {
            sent.clips_sig = Some(sig);
            true
        }
    };
    if dirty {
        let _ = app.emit_to("start", "clipboard-updated", clips);
    }
}

fn emit_start_media_if_dirty(app: &AppHandle, state: &AppState, media: &MediaState) {
    let sig = hash_media_state(media);
    let dirty = {
        let mut sent = lock_unpoisoned(&state.start_sent);
        if sent.media_sig == Some(sig) {
            false
        } else {
            sent.media_sig = Some(sig);
            true
        }
    };
    if dirty {
        let _ = app.emit_to("start", "media-updated", media);
    }
}

fn emit_start_audio_if_dirty(app: &AppHandle, state: &AppState, aud: &AudioState) {
    let dirty = {
        let mut sent = lock_unpoisoned(&state.start_sent);
        if sent.audio.as_ref().is_some_and(|prev| prev.structurally_eq(aud)) {
            false
        } else {
            sent.audio = Some(aud.clone());
            true
        }
    };
    if dirty {
        let _ = app.emit_to("start", "audio-mixer-updated", aud);
    }
}

fn emit_flyout_audio_if_dirty(app: &AppHandle, state: &AppState, aud: &AudioState) {
    let dirty = {
        let mut sent = lock_unpoisoned(&state.flyout_sent_audio);
        if sent.as_ref().is_some_and(|prev| prev.structurally_eq(aud)) {
            false
        } else {
            *sent = Some(aud.clone());
            true
        }
    };
    if dirty {
        let _ = app.emit_to("flyout", "audio-mixer-updated", aud);
    }
}

fn emit_flyout_media_if_dirty(app: &AppHandle, state: &AppState, media: &MediaState) {
    let sig = hash_media_state(media);
    let dirty = {
        let mut sent = lock_unpoisoned(&state.flyout_sent_media_sig);
        if *sent == Some(sig) {
            false
        } else {
            *sent = Some(sig);
            true
        }
    };
    if dirty {
        let _ = app.emit_to("flyout", "media-updated", media);
    }
}

fn sample_telemetry_with_icon_sources(state: &AppState) -> TelemetryState {
    let pid_to_hwnd = {
        let groups = lock_unpoisoned(&state.last_window_groups);
        build_pid_to_hwnd_map(&groups)
    };
    let (telem, sources) =
        telemetry::sample_telemetry_with_icons_mutex(&state.icon_cache, &pid_to_hwnd);
    state.register_icon_sources(sources);
    telem
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

fn win_hwnd(win: &WebviewWindow) -> Option<isize> {
    win.hwnd().ok().map(|h| h.0 as isize)
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn primary_taskbar_dpi(app: &AppHandle) -> u32 {
    app.state::<AppState>()
        .current_dpi_override()
        .unwrap_or_else(win_shell::primary_monitor_dpi)
        .max(96)
}

/// DPI-only scale factor for `taskbar` (neutralizes Windows Accessibility `TextScaleFactor`).
fn primary_taskbar_scale_factor(app: &AppHandle) -> f64 {
    effective_taskbar_dpi_scale(
        app.state::<AppState>().current_dpi_override(),
        win_shell::primary_monitor_dpi() as f64 / 96.0,
    )
}

/// Effective scale factor for `start` and `flyout` (preserves Windows Accessibility `TextScaleFactor`).
fn scale_factor(app: &AppHandle) -> f64 {
    let dpi_override = app.state::<AppState>().current_dpi_override();
    let fallback_sf = win_shell::primary_monitor_dpi() as f64 / 96.0;
    let text_scale = win_shell::windows_text_scale_factor();
    effective_taskbar_scale_factor(dpi_override, fallback_sf, text_scale)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowItemDigest {
    hwnd: isize,
    title: String,
    is_active: bool,
    is_minimized: bool,
    is_topmost: bool,
    monitor: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowGroupDigest {
    key: String,
    name: String,
    aumid: Option<String>,
    icon_key: Option<String>,
    is_active: bool,
    windows: Vec<WindowItemDigest>,
}

type WindowsFingerprint = Vec<WindowGroupDigest>;

fn compute_windows_fingerprint(groups: &[AppWindowGroup]) -> WindowsFingerprint {
    groups
        .iter()
        .map(|g| WindowGroupDigest {
            key: g.key.clone(),
            name: g.name.clone(),
            aumid: g.aumid.clone(),
            icon_key: g.icon_key.clone(),
            is_active: g.is_active,
            windows: g
                .windows
                .iter()
                .map(|w| WindowItemDigest {
                    hwnd: w.hwnd,
                    title: w.title.clone(),
                    is_active: w.is_active,
                    is_minimized: w.is_minimized,
                    is_topmost: w.is_topmost,
                    monitor: w.monitor.clone(),
                })
                .collect(),
        })
        .collect()
}

fn diff_window_update_targets_with_secondary(
    last_fp: &mut Option<WindowsFingerprint>,
    groups: &[AppWindowGroup],
    coord: &WindowCoordinator,
    secondary_taskbar_visible: bool,
) -> Option<Vec<&'static str>> {
    let fp = compute_windows_fingerprint(groups);
    if last_fp.as_ref() == Some(&fp) {
        return None;
    }
    *last_fp = Some(fp);
    Some(coord.windows_update_targets_with_secondary(secondary_taskbar_visible))
}

#[cfg(test)]
fn diff_window_update_targets(
    last_fp: &mut Option<WindowsFingerprint>,
    groups: &[AppWindowGroup],
    coord: &WindowCoordinator,
) -> Option<Vec<&'static str>> {
    diff_window_update_targets_with_secondary(last_fp, groups, coord, false)
}

/// Diff-gates lightweight `audio-master-updated` emissions and targets ONLY visible surfaces (AD-6).
fn diff_audio_master_targets(
    last: &mut Option<AudioMasterState>,
    current: &AudioMasterState,
    coord: &WindowCoordinator,
    secondary_taskbar_visible: bool,
) -> Option<Vec<&'static str>> {
    if last.as_ref() == Some(current) {
        return None;
    }
    *last = Some(current.clone());
    Some(coord.audio_master_targets_with_secondary(secondary_taskbar_visible))
}

/// Diff-gates full `audio-mixer-updated` emissions and targets ONLY `"start"` / `"flyout(sound)"` (AD-6).
/// Returns `None` without advancing `last` when no mixer-capable surface is visible.
fn diff_audio_mixer_targets(
    last: &mut Option<AudioState>,
    current: &AudioState,
    coord: &WindowCoordinator,
) -> Option<Vec<&'static str>> {
    let targets = coord.audio_mixer_targets();
    if targets.is_empty() {
        *last = None;
        return None;
    }
    if last.as_ref().is_some_and(|prev| prev.structurally_eq(current)) {
        return None;
    }
    *last = Some(current.clone());
    Some(targets)
}

fn emit_audio_state_targeted(app: &AppHandle, audio_state: &AudioState) {
    let st = app.state::<AppState>();
    let sec_vis = st.secondary_taskbar_visible.load(Ordering::Acquire);
    let (master_targets, mixer_targets) = {
        let coord = lock_unpoisoned(&st.coordinator);
        (
            coord.audio_master_targets_with_secondary(sec_vis),
            coord.audio_mixer_targets(),
        )
    };
    let master = audio_state.to_master_state();
    for target in master_targets {
        let _ = app.emit_to(target, "audio-master-updated", &master);
    }
    for target in mixer_targets {
        if target == "start" {
            lock_unpoisoned(&st.start_sent).audio = Some(audio_state.clone());
        } else if target == "flyout" {
            *lock_unpoisoned(&st.flyout_sent_audio) = Some(audio_state.clone());
        }
        let _ = app.emit_to(target, "audio-mixer-updated", audio_state);
    }
}

fn emit_session_alerts_if_calendar_open(app: &AppHandle) {
    let is_cal = lock_unpoisoned(&app.state::<AppState>().coordinator).active_flyout()
        == Some(FlyoutKind::Calendar);
    if is_cal {
        let alerts = system_widgets::get_session_alerts();
        let _ = app.emit_to("flyout", "session-alerts-updated", &alerts);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlowPollPlan {
    poll_media: bool,
    poll_telemetry: bool,
    poll_peripherals: bool,
    enforce_hide_native: bool,
}

/// Pure gating policy for hidden-window slow-poller suppression (AD-6, NFR-03):
/// Start-only live widgets (`media`, `telemetry`, `peripherals`) are ONLY polled and emitted
/// while the `start` window is actually visible.
fn should_poll_start_widgets(coord: &WindowCoordinator) -> bool {
    coord.is_start_visible()
}

/// Clipboard listener stays active continuously in Rust, but only emits `clipboard-updated`
/// over IPC when `"start"` is currently visible (show-time hydration pushes the full vault).
fn should_emit_start_clipboard_update(coord: &WindowCoordinator) -> bool {
    coord.is_start_visible()
}

fn plan_slow_poll_tick(coord: &WindowCoordinator, tick: u64, hide_native: bool) -> SlowPollPlan {
    let start_visible = should_poll_start_widgets(coord);
    SlowPollPlan {
        poll_media: start_visible,
        poll_telemetry: start_visible && tick % 2 == 0,
        poll_peripherals: start_visible && tick % 10 == 0,
        enforce_hide_native: hide_native && tick % 10 == 0,
    }
}

fn dispatch_coordinator<F>(
    app: &AppHandle,
    flyout_req: Option<&FlyoutRequest>,
    transition: F,
) -> CoordinatorActions
where
    F: FnOnce(&mut WindowCoordinator) -> CoordinatorActions,
{
    let actions = {
        let state = app.state::<AppState>();
        let mut coord = lock_unpoisoned(&state.coordinator);
        let act = transition(&mut coord);
        state.sync_overlay_flag(&coord);
        act
    };
    apply_coordinator_actions(app, &actions, flyout_req);
    actions
}

#[cfg(test)]
fn apply_fullscreen_snapshot(
    state: &AppState,
    hint_entering: Option<bool>,
    snap: &win_shell::FullscreenSnapshot,
) -> CoordinatorActions {
    let mut coord = lock_unpoisoned(&state.coordinator);
    let act = match hint_entering {
        Some(entering) => coord.on_appbar_fullscreen_hint(
            entering,
            snap.notification_state,
            snap.foreground.as_ref(),
            snap.primary_monitor,
        ),
        None => coord.reevaluate_fullscreen(
            snap.notification_state,
            snap.foreground.as_ref(),
            snap.primary_monitor,
        ),
    };
    state.sync_overlay_flag(&coord);
    act
}

fn reevaluate_fullscreen(app: &AppHandle, _hint_entering: Option<bool>) -> CoordinatorActions {
    let (per_mon, fg_is_own) = win_shell::detect_fullscreen_per_monitor_with_fg();
    let actions = {
        let state = app.state::<AppState>();
        let mut coord = lock_unpoisoned(&state.coordinator);
        let act = coord.reevaluate_fullscreen_per_monitor_with_fg(per_mon, fg_is_own, monotonic_ms());
        state.sync_overlay_flag(&coord);
        act
    };
    apply_coordinator_actions(app, &actions, None);
    actions
}

fn active_shell_window_rects(app: &AppHandle) -> Vec<PixelRect> {
    let st = app.state::<AppState>();
    let (start_active, flyout_active) = {
        let coord = lock_unpoisoned(&st.coordinator);
        (coord.is_start_visible(), coord.is_flyout_visible())
    };
    let secondary_visible = st.secondary_taskbar_visible.load(Ordering::Acquire);
    let mut rects = Vec::with_capacity(4);
    for (label, include) in [
        ("taskbar", true),
        ("taskbar-secondary", secondary_visible),
        ("start", start_active),
        ("flyout", flyout_active),
    ] {
        if !include {
            continue;
        }
        if let Some(win) = app.get_webview_window(label) {
            if let Some(hwnd) = win_hwnd(&win) {
                if let Some(r) = win_shell::get_window_pixel_rect(hwnd) {
                    rects.push(r);
                }
            }
        }
    }
    rects
}

/// DPI-only scale factor (used by `taskbar` & `taskbar-secondary` where `TextScaleFactor` is neutralized).
fn effective_taskbar_dpi_scale(dpi_override: Option<u32>, fallback_sf: f64) -> f64 {
    if let Some(dpi) = dpi_override.filter(|&d| d > 0) {
        dpi as f64 / 96.0
    } else if fallback_sf.is_finite() && fallback_sf > 0.0 {
        fallback_sf
    } else {
        1.0
    }
}

/// Effective scale factor including Windows Accessibility `TextScaleFactor` (used by `start` & `flyout`).
fn effective_taskbar_scale_factor(
    dpi_override: Option<u32>,
    fallback_sf: f64,
    text_scale: f64,
) -> f64 {
    let base_dpi_sf = effective_taskbar_dpi_scale(dpi_override, fallback_sf);
    let ts = if text_scale.is_finite() && text_scale > 0.0 {
        text_scale.clamp(1.0, 2.25)
    } else {
        1.0
    };
    base_dpi_sf * ts
}

fn compute_taskbar_bounds(
    mon: win_shell::MonitorRect,
    sf: f64,
    bar_mode: &str,
    logical_content_w: i32,
) -> PixelRect {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    let mon_w = mon.width.max(0);
    let mon_h = mon.height.max(0);
    if BarMode::from_config_str(bar_mode) == BarMode::Edge {
        let h = ((DOCK_BAR_LOGICAL_HEIGHT * sf).round() as i32).clamp(0, mon_h);
        PixelRect::new(mon.left, mon.bottom - h, mon.left + mon_w, mon.bottom)
    } else {
        // Dock mode: unconstrained capsule width + 32px (16px left + 16px right) transparent shadow padding;
        // 83px logical height = 16px top + 58px bar + 9px bottom float.
        let desired_w = ((logical_content_w.clamp(DOCK_MIN_CONTENT_WIDTH_LOGICAL, 2400)
            + DOCK_HORIZONTAL_PADDING_LOGICAL) as f64
            * sf)
            .round() as i32;
        let max_w = (mon_w - 16).max(0);
        let w = desired_w.clamp(0, max_w);
        let h = ((DOCK_WINDOW_LOGICAL_HEIGHT * sf).round() as i32).clamp(0, mon_h);
        let x = mon.left + (mon_w - w) / 2;
        let y = mon.bottom - h;
        PixelRect::new(x, y, x + w, y + h)
    }
}

/// Computes physical screen bounds for a secondary monitor taskbar using that monitor's
/// pure physical DPI (`dpi / 96.0`, neutralizing Windows Accessibility `TextScaleFactor`),
/// clamped cleanly within `mon.width - 16`.
pub fn compute_secondary_taskbar_bounds(
    mon: win_shell::MonitorRect,
    _text_scale: f64,
    bar_mode: &str,
    logical_content_w: i32,
) -> PixelRect {
    let sf = (mon.dpi.max(96) as f64) / 96.0;
    compute_taskbar_bounds(mon, sf, bar_mode, logical_content_w)
}

fn compute_start_bounds(mon: win_shell::MonitorRect, sf: f64) -> PixelRect {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    let tb_sf = (mon.dpi.max(96) as f64) / 96.0;
    let mon_w = mon.width.max(0);
    let mon_h = mon.height.max(0);
    let avail_w = ((mon_w as f64 / sf) - 24.0).max(320.0);
    let avail_h = ((mon_h as f64 / sf) - 80.0).max(320.0);
    let logical_w = avail_w.min(START_DEFAULT_LOGICAL_WIDTH);
    let logical_h = avail_h.min(START_DEFAULT_LOGICAL_HEIGHT);
    let max_w_px = (mon_w - 16).max(0);
    let max_h_px = (mon_h - 16).max(0);
    let w_px = ((logical_w * sf).round() as i32).clamp(0, max_w_px);
    let h_px = ((logical_h * sf).round() as i32).clamp(0, max_h_px);
    // Keep outer HWND bottom right above the 46px taskbar button band (0px click overlap)
    // while floating cleanly above the balanced 58px + 9px Dock.
    let bar_gap = (61.0 * tb_sf).round() as i32;
    let x_px = mon.left + (mon_w - w_px) / 2;
    let y_px = (mon.bottom - bar_gap - h_px).max(mon.top + 8);
    PixelRect::new(x_px, y_px, x_px + w_px, y_px + h_px)
}

/// Applies Win32 HWND bounds and topmost Z-order in a single atomic `SetWindowPos` call
/// without activating or forcing hidden windows visible (AD-1).
fn apply_webview_bounds(_win: &WebviewWindow, hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    let w_clamped = w.max(1);
    let h_clamped = h.max(1);
    win_shell::set_window_bounds_atomic(hwnd, x, y, w_clamped, h_clamped, true);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskbarLayoutInput {
    bar_mode: String,
    glass_depth: String,
    logical_content_w: i32,
    dpi_override: Option<u32>,
}

fn read_taskbar_layout_if_allowed(state: &AppState) -> Option<TaskbarLayoutInput> {
    if !lock_unpoisoned(&state.coordinator).should_apply_taskbar_bounds() {
        return None;
    }
    let (bar_mode, glass_depth) = {
        let cfg = lock_unpoisoned(&state.config);
        (cfg.bar_mode.clone(), cfg.glass_depth.clone())
    };
    let logical_content_w = *lock_unpoisoned(&state.taskbar_content_width);
    Some(TaskbarLayoutInput {
        bar_mode,
        glass_depth,
        logical_content_w,
        dpi_override: state.current_dpi_override(),
    })
}

#[cfg(test)]
fn plan_taskbar_bounds_update(
    state: &AppState,
    mon: win_shell::MonitorRect,
    fallback_sf: f64,
    _text_scale: f64,
) -> Option<PixelRect> {
    let input = read_taskbar_layout_if_allowed(state)?;
    let sf = effective_taskbar_dpi_scale(input.dpi_override, fallback_sf);
    Some(compute_taskbar_bounds(
        mon,
        sf,
        &input.bar_mode,
        input.logical_content_w,
    ))
}

fn position_taskbar_window(app: &AppHandle) {
    let st = app.state::<AppState>();
    let Some(input) = read_taskbar_layout_if_allowed(&st) else {
        return;
    };
    let Some(tb) = app.get_webview_window("taskbar") else { return };
    let Some(hwnd) = win_hwnd(&tb) else { return };
    let dpi = primary_taskbar_dpi(app);
    ensure_taskbar_dpi_only_scale(&tb, &st.primary_taskbar_raster_dpi, dpi);
    let sf = effective_taskbar_dpi_scale(input.dpi_override, dpi as f64 / 96.0);
    let mon = win_shell::primary_monitor_rect();

    let rect = compute_taskbar_bounds(mon, sf, &input.bar_mode, input.logical_content_w);

    // Never force visibility here so fullscreen suppression cannot leak on bounds updates.
    apply_webview_bounds(&tb, hwnd, rect.left, rect.top, rect.width(), rect.height());
    let (insets, radius) = taskbar_backdrop_insets_and_radius(&input.bar_mode, sf);
    let visible_now = win_shell::get_window_pixel_rect(hwnd).is_some();
    backdrop::attach_or_sync(
        "taskbar",
        hwnd,
        rect,
        insets,
        radius,
        &input.glass_depth,
        visible_now,
    );
}

fn resolve_secondary_taskbar_content_width(state: &AppState) -> i32 {
    lock_unpoisoned(&state.secondary_taskbar_content_width)
        .unwrap_or(SECONDARY_DOCK_DEFAULT_CONTENT_WIDTH_LOGICAL)
}

fn position_secondary_taskbar_window(app: &AppHandle) {
    let st = app.state::<AppState>();
    if !lock_unpoisoned(&st.coordinator).should_apply_secondary_taskbar_bounds() {
        return;
    }
    let (bar_mode, sec_mode, glass_depth) = {
        let cfg = lock_unpoisoned(&st.config);
        (
            cfg.bar_mode.clone(),
            cfg.secondary_taskbar_mode.clone(),
            cfg.glass_depth.clone(),
        )
    };
    if sec_mode != config::SECONDARY_TASKBAR_MODE_TESTBAR {
        return;
    }
    let Some(sec_mon) = win_shell::secondary_monitor_rects().into_iter().next() else {
        return;
    };
    let Some(tb2) = app.get_webview_window("taskbar-secondary") else {
        return;
    };
    let Some(hwnd) = win_hwnd(&tb2) else { return };
    ensure_taskbar_dpi_only_scale(&tb2, &st.secondary_taskbar_raster_dpi, sec_mon.dpi);
    let logical_w = resolve_secondary_taskbar_content_width(&st);
    let sf = (sec_mon.dpi.max(96) as f64) / 96.0;
    let rect = compute_secondary_taskbar_bounds(sec_mon, 1.0, &bar_mode, logical_w);
    apply_webview_bounds(&tb2, hwnd, rect.left, rect.top, rect.width(), rect.height());
    let (insets, radius) = taskbar_backdrop_insets_and_radius(&bar_mode, sf);
    let visible_now = win_shell::get_window_pixel_rect(hwnd).is_some();
    backdrop::attach_or_sync(
        "taskbar-secondary",
        hwnd,
        rect,
        insets,
        radius,
        &glass_depth,
        visible_now,
    );
}

fn sync_secondary_taskbar(app: &AppHandle) {
    let st = app.state::<AppState>();
    let (cfg_snapshot, bar_mode, sec_mode, glass_depth) = {
        let cfg = lock_unpoisoned(&st.config);
        (
            cfg.clone(),
            cfg.bar_mode.clone(),
            cfg.secondary_taskbar_mode.clone(),
            cfg.glass_depth.clone(),
        )
    };
    win_shell::apply_secondary_taskbar_mode_to_native(&sec_mode, st.hide_native);

    let Some(tb2) = app.get_webview_window("taskbar-secondary") else {
        st.secondary_taskbar_visible.store(false, Ordering::Release);
        backdrop::set_visible("taskbar-secondary", false);
        return;
    };
    let Some(hwnd) = win_hwnd(&tb2) else {
        st.secondary_taskbar_visible.store(false, Ordering::Release);
        backdrop::set_visible("taskbar-secondary", false);
        return;
    };

    let sec_allowed = lock_unpoisoned(&st.coordinator).should_show_secondary_taskbar();
    let sec_mon = win_shell::secondary_monitor_rects().into_iter().next();
    if sec_mode == config::SECONDARY_TASKBAR_MODE_TESTBAR && sec_allowed {
        if let Some(mon) = sec_mon {
            ensure_taskbar_dpi_only_scale(&tb2, &st.secondary_taskbar_raster_dpi, mon.dpi);
            let logical_w = resolve_secondary_taskbar_content_width(&st);
            let sf = (mon.dpi.max(96) as f64) / 96.0;
            let rect = compute_secondary_taskbar_bounds(mon, 1.0, &bar_mode, logical_w);
            apply_webview_bounds(&tb2, hwnd, rect.left, rect.top, rect.width(), rect.height());
            let was_visible = st.secondary_taskbar_visible.swap(true, Ordering::AcqRel);
            if !was_visible {
                set_webview_memory_low(&tb2, false);
            }
            win_shell::set_window_visible(hwnd, true, true);
            let (insets, radius) = taskbar_backdrop_insets_and_radius(&bar_mode, sf);
            backdrop::attach_or_sync(
                "taskbar-secondary",
                hwnd,
                rect,
                insets,
                radius,
                &glass_depth,
                true,
            );
            backdrop::sync_z_order("taskbar-secondary", hwnd);
            if !was_visible {
                let win_snap = lock_unpoisoned(&st.last_window_groups).clone();
                let (start_open, start_gen, start_mon) = {
                    let c = lock_unpoisoned(&st.coordinator);
                    (
                        c.is_start_open(),
                        c.start_anim_gen(),
                        c.start_target_monitor().map(str::to_string),
                    )
                };
                let _ = app.emit_to("taskbar-secondary", "config-updated", &cfg_snapshot);
                let _ = app.emit_to("taskbar-secondary", "windows-updated", &win_snap);
                let _ = app.emit_to(
                    "taskbar-secondary",
                    "start-state",
                    StartStateEvent {
                        open: start_open,
                        gen: start_gen,
                        monitor: start_mon,
                        immediate: false,
                    },
                );
            }
            return;
        }
    }

    let was_vis = st.secondary_taskbar_visible.swap(false, Ordering::AcqRel);
    backdrop::set_visible("taskbar-secondary", false);
    win_shell::set_window_visible(hwnd, false, true);
    if was_vis {
        set_webview_memory_low(&tb2, true);
    }
}

fn resolve_start_monitor_and_scale(
    app: &AppHandle,
    monitor: Option<&str>,
) -> (win_shell::MonitorRect, f64, Option<String>) {
    if monitor == Some("secondary") {
        if let Some(sec_mon) = win_shell::secondary_monitor_rects().into_iter().next() {
            let text_scale = win_shell::windows_text_scale_factor();
            let sec_sf = coordinator::compute_effective_scale(sec_mon.dpi, text_scale);
            return (sec_mon, sec_sf, Some("secondary".to_string()));
        }
    }
    (
        win_shell::primary_monitor_rect(),
        scale_factor(app),
        Some("primary".to_string()),
    )
}

fn position_start_window_on_monitor(
    app: &AppHandle,
    win: &WebviewWindow,
    hwnd: isize,
    monitor: Option<&str>,
) -> (PixelRect, f64, i32, Option<String>) {
    let (mon, sf, resolved_monitor) = resolve_start_monitor_and_scale(app, monitor);
    let rect = compute_start_bounds(mon, sf);
    apply_webview_bounds(win, hwnd, rect.left, rect.top, rect.width(), rect.height());
    *lock_unpoisoned(&app.state::<AppState>().last_start_rect) = Some(rect);
    (rect, sf, mon.top, resolved_monitor)
}

#[derive(Debug, Clone, Copy)]
struct ComputedFlyoutLayout {
    rect: PixelRect,
    scale_factor: f64,
    anchor_origin_x: f64,
    bottom_anchor_y: i32,
    monitor_top: i32,
    max_h_logical: f64,
}

fn compute_flyout_max_height_logical(mon_h: i32, sf: f64) -> f64 {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    ((mon_h.max(0) as f64 / sf) - 88.0).clamp(240.0, 620.0)
}

fn compute_flyout_anchor_origin_x(
    anchor_screen_x_px: i32,
    win_left_px: i32,
    total_w_logical: f64,
    sf: f64,
) -> f64 {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    let rel = (anchor_screen_x_px - win_left_px) as f64 / sf;
    let pad = FLYOUT_ROOT_PAD_LOGICAL;
    let max_x = (total_w_logical - pad).max(pad);
    rel.clamp(pad, max_x)
}

fn compute_flyout_layout(app: &AppHandle, req: &FlyoutRequest) -> ComputedFlyoutLayout {
    let use_secondary = req.monitor.as_deref() == Some("secondary");
    let (mon, sf, tb_sf, tb_label) = if use_secondary {
        if let Some(sec_mon) = win_shell::secondary_monitor_rects().into_iter().next() {
            let text_scale = win_shell::windows_text_scale_factor();
            let sec_tb_sf = (sec_mon.dpi.max(96) as f64) / 96.0;
            let sec_sf = coordinator::compute_effective_scale(sec_mon.dpi, text_scale);
            (sec_mon, sec_sf, sec_tb_sf, "taskbar-secondary")
        } else {
            (
                win_shell::primary_monitor_rect(),
                scale_factor(app),
                primary_taskbar_scale_factor(app),
                "taskbar",
            )
        }
    } else {
        (
            win_shell::primary_monitor_rect(),
            scale_factor(app),
            primary_taskbar_scale_factor(app),
            "taskbar",
        )
    };

    let anchor_screen_x_px = if !req.anchor_x_logical.is_finite() {
        mon.left + mon.width / 2
    } else if req.anchor_in_taskbar {
        if let Some(tb) = app.get_webview_window(tb_label) {
            let pos = tb.outer_position().map(|p| p.x).unwrap_or(mon.left);
            // Taskbar WebView2 uses DPI-only `tb_sf` (TextScaleFactor neutralized).
            pos + (req.anchor_x_logical * tb_sf).round() as i32
        } else {
            mon.left + mon.width / 2
        }
    } else {
        mon.left + (req.anchor_x_logical * sf).round() as i32
    };

    let max_h_logical = compute_flyout_max_height_logical(mon.height, sf);
    let req_w = req.width.clamp(1, 2000) as f64;
    let req_h = (req.height as f64).clamp(48.0, max_h_logical);
    let total_w_logical = req_w + FLYOUT_ROOT_PAD_TOTAL_LOGICAL;
    let total_h_logical = req_h + FLYOUT_ROOT_PAD_TOTAL_LOGICAL;
    let w_px = (total_w_logical * sf).round() as i32;
    let h_px = (total_h_logical * sf).round() as i32;
    let is_edge =
        BarMode::from_config_str(&lock_unpoisoned(&app.state::<AppState>().config).bar_mode)
            == BarMode::Edge;
    let taskbar_top_px = (if is_edge {
        EDGE_APPBAR_LOGICAL_HEIGHT
    } else {
        DOCK_APPBAR_LOGICAL_HEIGHT
    }) as f64
        * tb_sf;
    let desired_air_gap_px = match req.kind {
        FlyoutKind::Preview => 6.0 * tb_sf,
        _ => 10.0 * tb_sf,
    };
    let flyout_pad_px = FLYOUT_ROOT_PAD_LOGICAL * sf;
    let max_bezel_overlap_px = if is_edge { 5.0 * tb_sf } else { 6.0 * tb_sf };
    let bezel_overlap_px = (flyout_pad_px - desired_air_gap_px).clamp(0.0, max_bezel_overlap_px);
    let bar_gap = (taskbar_top_px - bezel_overlap_px).round() as i32;

    let mut x_px = anchor_screen_x_px - w_px / 2;
    x_px = x_px.clamp(mon.left + 8, (mon.right - w_px - 8).max(mon.left + 8));
    let bottom_anchor_y = mon.bottom - bar_gap;
    let y_px = (bottom_anchor_y - h_px).max(mon.top + 8);
    let anchor_origin_x =
        compute_flyout_anchor_origin_x(anchor_screen_x_px, x_px, total_w_logical, sf);

    ComputedFlyoutLayout {
        rect: PixelRect::new(x_px, y_px, x_px + w_px, y_px + h_px),
        scale_factor: sf,
        anchor_origin_x,
        bottom_anchor_y,
        monitor_top: mon.top,
        max_h_logical,
    }
}

fn position_flyout_window(
    app: &AppHandle,
    win: &WebviewWindow,
    hwnd: isize,
    req: &FlyoutRequest,
) -> ComputedFlyoutLayout {
    let layout = compute_flyout_layout(app, req);
    apply_webview_bounds(
        win,
        hwnd,
        layout.rect.left,
        layout.rect.top,
        layout.rect.width(),
        layout.rect.height(),
    );
    layout
}

/// Clean fast-resize handler for `surface_commit_ack`: windows are shown immediately at 0ms
/// (zero Paint-Sync stall!), and when the frontend reports a `measured_height` differing by
/// >2px we atomically resize the HWND while keeping its bottom edge anchored.
fn apply_surface_measured_height(
    app: &AppHandle,
    surface: &str,
    gen: u64,
    measured_height: Option<f64>,
) {
    let st = app.state::<AppState>();
    // Never prematurely reveal the flyout backdrop on `surface_commit_ack` — wait for
    // `animate_surface_in` so the flyout opens in ONE synchronized spring at exact final height
    // (backed by the 160ms safety fallback so it can never remain hidden).
    let Some(mh) = measured_height.filter(|h| h.is_finite()) else {
        return;
    };
    let glass_depth = lock_unpoisoned(&st.config).glass_depth.clone();
    match surface {
        "start" => {
            if mh < 240.0 || st.start_anim_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            if !lock_unpoisoned(&st.coordinator).is_start_visible() {
                return;
            }
            let metrics = {
                let slot = lock_unpoisoned(&st.active_start_metrics);
                slot.filter(|m| m.gen == gen)
            };
            let Some(mut m) = metrics else { return };
            let new_h = ((mh.clamp(240.0, m.max_h_logical) + 2.0 * START_ROOT_PAD_LOGICAL)
                * m.scale_factor)
                .round() as i32;
            if (new_h - m.rect.height()).abs() <= 2 {
                return;
            }
            let new_y = (m.bottom_anchor_y - new_h).max(m.monitor_top + 8);
            let new_rect = PixelRect::new(m.rect.left, new_y, m.rect.right, new_y + new_h);
            m.rect = new_rect;
            *lock_unpoisoned(&st.active_start_metrics) = Some(m);
            *lock_unpoisoned(&st.last_start_rect) = Some(new_rect);
            if let Some(win) = app.get_webview_window("start") {
                if let Some(hwnd) = win_hwnd(&win) {
                    apply_webview_bounds(
                        &win,
                        hwnd,
                        new_rect.left,
                        new_rect.top,
                        new_rect.width(),
                        new_rect.height(),
                    );
                    let (insets, radius) = start_backdrop_insets_and_radius(m.scale_factor);
                    let shown = st.start_backdrop_shown_gen.load(Ordering::SeqCst) == gen;
                    backdrop::attach_or_sync(
                        "start",
                        hwnd,
                        new_rect,
                        insets,
                        radius,
                        &glass_depth,
                        shown,
                    );
                }
            }
        }
        "flyout" => {
            if mh < 36.0 || st.flyout_anim_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            let Some(kind) = lock_unpoisoned(&st.coordinator).active_flyout() else {
                return;
            };
            let metrics = {
                let slot = lock_unpoisoned(&st.active_flyout_metrics);
                slot.filter(|m| m.gen == gen)
            };
            let Some(mut m) = metrics else { return };
            let clamped_h = mh.clamp(48.0, m.max_h_logical);
            let new_h = ((clamped_h + FLYOUT_ROOT_PAD_TOTAL_LOGICAL) * m.scale_factor)
                .round() as i32;
            if (new_h - m.rect.height()).abs() <= 2 {
                return;
            }
            let new_y = (m.bottom_anchor_y - new_h).max(m.monitor_top + 8);
            let new_rect = PixelRect::new(m.rect.left, new_y, m.rect.right, new_y + new_h);
            m.rect = new_rect;
            *lock_unpoisoned(&st.active_flyout_metrics) = Some(m);
            *lock_unpoisoned(&st.last_flyout_bounds) = Some((kind, new_rect));
            if let Some(req) = lock_unpoisoned(&st.last_flyout_req).as_mut() {
                req.height = clamped_h.round() as i32;
            }
            if let Some(fly) = app.get_webview_window("flyout") {
                if let Some(hwnd) = win_hwnd(&fly) {
                    apply_webview_bounds(
                        &fly,
                        hwnd,
                        new_rect.left,
                        new_rect.top,
                        new_rect.width(),
                        new_rect.height(),
                    );
                    let (insets, radius) = flyout_backdrop_insets_and_radius(kind, m.scale_factor);
                    let shown = st.flyout_backdrop_shown_gen.load(Ordering::SeqCst) == gen;
                    backdrop::attach_or_sync(
                        "flyout",
                        hwnd,
                        new_rect,
                        insets,
                        radius,
                        &glass_depth,
                        shown,
                    );
                }
            }
        }
        _ => {}
    }
}

fn schedule_post_close_fullscreen_check(app: &AppHandle) {
    let h_fs = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        let h = h_fs.clone();
        let _ = h_fs.run_on_main_thread(move || {
            let st = h.state::<AppState>();
            if !lock_unpoisoned(&st.coordinator).has_open_overlay() {
                let _ = reevaluate_fullscreen(&h, None);
            }
        });
    });
}

fn commit_pending_surface_exit(app: &AppHandle, surface: &str, gen: u64) {
    let st = app.state::<AppState>();
    match surface {
        "start" => {
            let pending = {
                let mut slot = lock_unpoisoned(&st.pending_start_exit);
                if slot.as_ref().is_some_and(|p| p.gen == gen) {
                    slot.take()
                } else {
                    None
                }
            };
            if pending.is_none() || st.start_anim_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            if lock_unpoisoned(&st.coordinator).is_start_visible() {
                return;
            }
            backdrop::set_visible("start", false);
            if let Some(win) = app.get_webview_window("start") {
                if let Some(hwnd) = win_hwnd(&win) {
                    win_shell::set_window_visible(hwnd, false, true);
                    win_shell::set_window_hit_transparent(hwnd, false);
                }
            }
            schedule_post_close_fullscreen_check(app);
        }
        "flyout" => {
            let pending = {
                let mut slot = lock_unpoisoned(&st.pending_flyout_exit);
                if slot.as_ref().is_some_and(|p| p.gen == gen) {
                    slot.take()
                } else {
                    None
                }
            };
            if pending.is_none() || st.flyout_anim_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            if lock_unpoisoned(&st.coordinator).is_flyout_visible() {
                return;
            }
            backdrop::set_visible("flyout", false);
            win_shell::set_flyout_non_activating(false);
            if let Some(fly) = app.get_webview_window("flyout") {
                if let Some(hwnd) = win_hwnd(&fly) {
                    win_shell::set_window_visible(hwnd, false, true);
                    win_shell::set_window_hit_transparent(hwnd, false);
                }
            }
            schedule_post_close_fullscreen_check(app);
        }
        _ => {}
    }
}

fn apply_taskbar_visibility_action<FPos, FVis>(
    taskbar_visible: Option<bool>,
    reposition: FPos,
    set_visible: FVis,
) where
    FPos: FnOnce(),
    FVis: FnOnce(bool),
{
    if let Some(tb_vis) = taskbar_visible {
        if tb_vis {
            reposition();
        }
        set_visible(tb_vis);
    }
}

fn embed_tray_apps_into_payload(payload: &mut serde_json::Value, tray_apps: &[TrayAppItem]) {
    let serialized = serde_json::to_value(tray_apps).unwrap_or(serde_json::Value::Array(vec![]));
    match payload {
        serde_json::Value::Object(map) => {
            map.insert("trayApps".to_string(), serialized);
        }
        _ => {
            let mut map = serde_json::Map::new();
            map.insert("trayApps".to_string(), serialized);
            *payload = serde_json::Value::Object(map);
        }
    }
}

fn embed_jump_list_into_payload(payload: &mut serde_json::Value, items: &[JumpListItem]) {
    let serialized = serde_json::to_value(items).unwrap_or(serde_json::Value::Array(vec![]));
    match payload {
        serde_json::Value::Object(map) => {
            map.insert("jumpList".to_string(), serialized);
        }
        _ => {
            let mut map = serde_json::Map::new();
            map.insert("jumpList".to_string(), serialized);
            *payload = serde_json::Value::Object(map);
        }
    }
}

fn apply_coordinator_actions(
    app: &AppHandle,
    actions: &CoordinatorActions,
    flyout_req: Option<&FlyoutRequest>,
) {
    let st = app.state::<AppState>();

    // 1. Hide / preempt outgoing surfaces first (with click-through exit or immediate atomic preemption)
    let hide_flyout_now = actions.hide_flyout_immediate
        || (actions.flyout_visible == Some(false)
            && (actions.start_visible == Some(true)
                || actions.taskbar_visible == Some(false)
                || actions.secondary_taskbar_visible == Some(false)));

    if hide_flyout_now {
        *lock_unpoisoned(&st.last_flyout_req) = None;
        *lock_unpoisoned(&st.last_flyout_bounds) = None;
        *lock_unpoisoned(&st.active_flyout_metrics) = None;
        *lock_unpoisoned(&st.pending_flyout_exit) = None;
        cancel_flyout_glide(&st);
        let hide_gen = actions
            .flyout_anim_gen
            .unwrap_or_else(|| st.flyout_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.flyout_anim_gen.store(hide_gen, Ordering::SeqCst);
        backdrop::set_visible("flyout", false);
        let _ = app.emit("flyout-hide-immediate", FlyoutHideEvent { gen: hide_gen });
        win_shell::set_flyout_non_activating(false);
        if let Some(fly) = app.get_webview_window("flyout") {
            if let Some(hwnd) = win_hwnd(&fly) {
                win_shell::set_window_visible(hwnd, false, true);
                win_shell::set_window_hit_transparent(hwnd, false);
            }
        }
        schedule_post_close_fullscreen_check(app);
    } else if actions.flyout_visible == Some(false) {
        *lock_unpoisoned(&st.last_flyout_req) = None;
        *lock_unpoisoned(&st.last_flyout_bounds) = None;
        *lock_unpoisoned(&st.active_flyout_metrics) = None;
        cancel_flyout_glide(&st);
        let hide_gen = actions
            .flyout_anim_gen
            .unwrap_or_else(|| st.flyout_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.flyout_anim_gen.store(hide_gen, Ordering::SeqCst);
        *lock_unpoisoned(&st.pending_flyout_exit) = Some(PendingSurfaceExit { gen: hide_gen });

        win_shell::set_flyout_non_activating(true);
        if let Some(fly) = app.get_webview_window("flyout") {
            if let Some(hwnd) = win_hwnd(&fly) {
                win_shell::set_window_hit_transparent(hwnd, true);
            }
        }
        if actions.emit_flyout_hide {
            let _ = app.emit("flyout-hide", FlyoutHideEvent { gen: hide_gen });
        }

        let h_hide = app.clone();
        let anim_gen = Arc::clone(&st.flyout_anim_gen);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(FLYOUT_EXIT_ANIM_MS)).await;
            if anim_gen.load(Ordering::SeqCst) != hide_gen {
                return;
            }
            let h = h_hide.clone();
            let _ = h_hide.run_on_main_thread(move || {
                commit_pending_surface_exit(&h, "flyout", hide_gen);
            });
        });
    }

    let hide_start_now = actions.hide_start_immediate
        || (actions.start_visible == Some(false)
            && (actions.flyout_visible == Some(true)
                || actions.taskbar_visible == Some(false)
                || actions.secondary_taskbar_visible == Some(false)));

    if hide_start_now {
        *lock_unpoisoned(&st.active_start_metrics) = None;
        *lock_unpoisoned(&st.pending_start_exit) = None;
        let hide_gen = actions
            .start_anim_gen
            .unwrap_or_else(|| st.start_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.start_anim_gen.store(hide_gen, Ordering::SeqCst);
        backdrop::set_visible("start", false);
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
            if let Some(hwnd) = win_hwnd(&win) {
                win_shell::set_window_visible(hwnd, false, true);
                win_shell::set_window_hit_transparent(hwnd, false);
            }
        }
        schedule_post_close_fullscreen_check(app);
    } else if actions.start_visible == Some(false) {
        *lock_unpoisoned(&st.active_start_metrics) = None;
        let hide_gen = actions
            .start_anim_gen
            .unwrap_or_else(|| st.start_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.start_anim_gen.store(hide_gen, Ordering::SeqCst);
        *lock_unpoisoned(&st.pending_start_exit) = Some(PendingSurfaceExit { gen: hide_gen });

        if let Some(win) = app.get_webview_window("start") {
            if let Some(hwnd) = win_hwnd(&win) {
                win_shell::set_window_hit_transparent(hwnd, true);
            }
        }
        if actions.emit_start_state == Some(false) {
            let _ = app.emit(
                "start-state",
                StartStateEvent {
                    open: false,
                    gen: hide_gen,
                    monitor: None,
                    immediate: false,
                },
            );
        }

        let h_hide = app.clone();
        let anim_gen = Arc::clone(&st.start_anim_gen);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(START_EXIT_ANIM_MS)).await;
            if anim_gen.load(Ordering::SeqCst) != hide_gen {
                return;
            }
            let h = h_hide.clone();
            let _ = h_hide.run_on_main_thread(move || {
                commit_pending_surface_exit(&h, "start", hide_gen);
            });
        });
    }

    // 2. Per-monitor taskbar visibility (runs synchronously BEFORE Start show for atomic fullscreen overlay)
    apply_taskbar_visibility_action(
        actions.taskbar_visible,
        || position_taskbar_window(app),
        |tb_vis| {
            if !tb_vis {
                backdrop::set_visible("taskbar", false);
            }
            if let Some(tb) = app.get_webview_window("taskbar") {
                if let Some(hwnd) = win_hwnd(&tb) {
                    win_shell::set_window_visible(hwnd, tb_vis, true);
                    if tb_vis {
                        backdrop::set_visible("taskbar", true);
                        backdrop::sync_z_order("taskbar", hwnd);
                    }
                }
            }
        },
    );

    if let Some(sec_vis) = actions.secondary_taskbar_visible {
        if sec_vis {
            sync_secondary_taskbar(app);
        } else {
            let was_vis = st.secondary_taskbar_visible.swap(false, Ordering::AcqRel);
            backdrop::set_visible("taskbar-secondary", false);
            if let Some(tb2) = app.get_webview_window("taskbar-secondary") {
                if let Some(hwnd) = win_hwnd(&tb2) {
                    win_shell::set_window_visible(hwnd, false, true);
                }
                if was_vis {
                    set_webview_memory_low(&tb2, true);
                }
            }
        }
    }

    // 3. Zero-Lag Start Menu Open: pre-hydrate cached data (including audio) -> SW_SHOW -> emit `start-shown`
    if actions.start_visible == Some(true) {
        capture_external_foreground_hwnd(&st);
        *lock_unpoisoned(&st.pending_start_exit) = None;
        let show_gen = actions
            .start_anim_gen
            .unwrap_or_else(|| st.start_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.start_anim_gen.store(show_gen, Ordering::SeqCst);

        let target_mon_req = actions.start_target_monitor.as_deref();
        let (mon, sf, resolved_monitor) = resolve_start_monitor_and_scale(app, target_mon_req);
        let rect = compute_start_bounds(mon, sf);
        let glass_depth = lock_unpoisoned(&st.config).glass_depth.clone();
        let (start_insets, start_radius) = start_backdrop_insets_and_radius(sf);

        *lock_unpoisoned(&st.active_start_metrics) = Some(ActiveSurfaceMetrics {
            gen: show_gen,
            rect,
            scale_factor: sf,
            bottom_anchor_y: rect.bottom,
            monitor_top: mon.top,
            max_h_logical: 960.0,
        });

        let start_win = app.get_webview_window("start");
        let start_hwnd = start_win.as_ref().and_then(win_hwnd);
        if let (Some(win), Some(hwnd)) = (&start_win, start_hwnd) {
            win_shell::set_window_hit_transparent(hwnd, false);
            let bounds_changed = {
                let mut slot = lock_unpoisoned(&st.last_start_rect);
                if *slot == Some(rect) {
                    false
                } else {
                    *slot = Some(rect);
                    true
                }
            };
            if bounds_changed {
                apply_webview_bounds(win, hwnd, rect.left, rect.top, rect.width(), rect.height());
            }
            // Arm underlay hidden; `animate_surface_in` reveals it on the exact visible rAF frame.
            backdrop::attach_or_sync(
                "start",
                hwnd,
                rect,
                start_insets,
                start_radius,
                &glass_depth,
                false,
            );
        }

        // Emit start-state and dirty-gated cached hydration BEFORE SW_SHOW (0 redundant JSON when unchanged!)
        let _ = app.emit(
            "start-state",
            StartStateEvent {
                open: true,
                gen: show_gen,
                monitor: resolved_monitor.clone(),
                immediate: false,
            },
        );
        let cfg_snapshot = lock_unpoisoned(&st.config).clone();
        let win_snapshot = lock_unpoisoned(&st.last_window_groups).clone();
        let clips_snapshot = system_widgets::get_clipboard_history();
        let media_snapshot = media::get_media_snapshot();
        let cached_audio = lock_unpoisoned(&st.last_audio_state).clone();
        emit_start_config_if_dirty(app, &st, &cfg_snapshot);
        emit_start_windows_if_dirty(app, &st, &win_snapshot);
        emit_start_clips_if_dirty(app, &st, &clips_snapshot);
        emit_start_media_if_dirty(app, &st, &media_snapshot);
        if let Some(ref aud_cached) = cached_audio {
            emit_start_audio_if_dirty(app, &st, aud_cached);
        }

        if let (Some(win), Some(hwnd)) = (&start_win, start_hwnd) {
            win_shell::set_window_visible(hwnd, true, false);
            let _ = win.set_focus();
        }

        // Strictly AFTER SW_SHOW: trigger StartView's post-visible double-rAF WAAPI + DComp spring!
        let _ = app.emit_to(
            "start",
            "start-shown",
            StartShownEvent {
                open: true,
                gen: show_gen,
                monitor: resolved_monitor,
            },
        );

        // Safety net (Tokio timer wheel — 0 OS threads spawned): ensure Start underlay never stays invisible
        if let Some(hwnd) = start_hwnd {
            let h_guard = app.clone();
            let anim_gen = Arc::clone(&st.start_anim_gen);
            let shown_gen = Arc::clone(&st.start_backdrop_shown_gen);
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(160)).await;
                if anim_gen.load(Ordering::SeqCst) == show_gen
                    && shown_gen.load(Ordering::SeqCst) != show_gen
                    && lock_unpoisoned(&h_guard.state::<AppState>().coordinator).is_start_visible()
                {
                    shown_gen.store(show_gen, Ordering::SeqCst);
                    backdrop::set_visible("start", true);
                    backdrop::sync_z_order("start", hwnd);
                }
            });
        }

        // Single-flight coalesced Start hydration: never stacks parallel audio/media/telemetry/BT scans
        // across rapid `Win` key presses, and cancels immediately if Start closes or gen advances.
        if st
            .start_hydration_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let h_hyd = app.clone();
            let hyd_flag = Arc::clone(&st.start_hydration_running);
            let anim_gen = Arc::clone(&st.start_anim_gen);
            let _ = tauri::async_runtime::spawn_blocking(move || {
                struct HydGuard(Arc<AtomicBool>);
                impl Drop for HydGuard {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release);
                    }
                }
                let _guard = HydGuard(hyd_flag);
                let still_current = |st_hyd: &AppState| -> bool {
                    anim_gen.load(Ordering::SeqCst) == show_gen
                        && lock_unpoisoned(&st_hyd.coordinator).is_start_visible()
                };

                let st_hyd = h_hyd.state::<AppState>();
                if !still_current(&st_hyd) {
                    return;
                }
                let aud = sample_audio_state_with_icon_sources(&st_hyd);
                if still_current(&st_hyd) {
                    emit_start_audio_if_dirty(&h_hyd, &st_hyd, &aud);
                    let _ = h_hyd.emit_to("start", "audio-master-updated", &aud.to_master_state());
                } else {
                    return;
                }

                let med = media::poll_media_state();
                if still_current(&st_hyd) {
                    emit_start_media_if_dirty(&h_hyd, &st_hyd, &med);
                } else {
                    return;
                }

                let tel = sample_telemetry_with_icon_sources(&st_hyd);
                if still_current(&st_hyd) {
                    let _ = h_hyd.emit_to("start", "telemetry-updated", &tel);
                } else {
                    return;
                }

                let periphs = system_widgets::get_peripheral_batteries();
                if still_current(&st_hyd) {
                    let _ = h_hyd.emit_to("start", "peripherals-updated", &periphs);
                }
            });
        }
    }

    // 4. Zero-Lag Instant Show + Preview Horizontal Glide for Flyout
    if actions.flyout_visible == Some(true) {
        *lock_unpoisoned(&st.pending_flyout_exit) = None;
        cancel_flyout_glide(&st);
        let show_gen = actions
            .flyout_anim_gen
            .unwrap_or_else(|| st.flyout_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.flyout_anim_gen.store(show_gen, Ordering::SeqCst);
        let non_activating = actions.flyout_non_activating.unwrap_or(false);

        if let Some(req) = flyout_req {
            let mut enriched = req.clone();
            if req.kind == FlyoutKind::Tray {
                let cached_tray = lock_unpoisoned(&st.cached_tray_apps).clone();
                let tray_warmed = st.tray_cache_warmed.load(Ordering::Acquire);
                if tray_warmed || !cached_tray.is_empty() {
                    enriched.height = compute_tray_flyout_height_logical(cached_tray.len());
                    embed_tray_apps_into_payload(&mut enriched.payload, &cached_tray);
                }
            } else if req.kind == FlyoutKind::ContextApp {
                let target = enriched
                    .payload
                    .get("target")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let aumid = enriched.payload.get("aumid").and_then(|v| v.as_str());
                let key = jump_list_cache_key(target, aumid);
                if let Some(cached_jl) = lock_unpoisoned(&st.jump_list_cache).get(&key).cloned() {
                    embed_jump_list_into_payload(&mut enriched.payload, &cached_jl);
                }
            }

            let layout = compute_flyout_layout(app, &enriched);
            enriched.gen = show_gen;
            enriched.anchor_origin_x = Some(layout.anchor_origin_x);
            *lock_unpoisoned(&st.last_flyout_req) = Some(enriched.clone());

            let glass_depth = lock_unpoisoned(&st.config).glass_depth.clone();
            let (fly_insets, fly_radius) =
                flyout_backdrop_insets_and_radius(req.kind, layout.scale_factor);

            let fly_win = app.get_webview_window("flyout");
            let fly_hwnd = fly_win.as_ref().and_then(win_hwnd);
            let was_visible =
                fly_hwnd.is_some_and(|hwnd| win_shell::get_window_pixel_rect(hwnd).is_some());
            let prev_bounds = *lock_unpoisoned(&st.last_flyout_bounds);

            let is_same_width_preview_scrub = was_visible
                && req.kind == FlyoutKind::Preview
                && matches!(
                    prev_bounds,
                    Some((FlyoutKind::Preview, prev_rect))
                        if (layout.rect.width() - prev_rect.width()).abs() <= 2
                            && (layout.rect.height() - prev_rect.height()).abs() <= 2
                );

            *lock_unpoisoned(&st.active_flyout_metrics) = Some(ActiveSurfaceMetrics {
                gen: show_gen,
                rect: layout.rect,
                scale_factor: layout.scale_factor,
                bottom_anchor_y: layout.bottom_anchor_y,
                monitor_top: layout.monitor_top,
                max_h_logical: layout.max_h_logical,
            });
            *lock_unpoisoned(&st.last_flyout_bounds) = Some((req.kind, layout.rect));

            let cached_audio_for_sound = if req.kind == FlyoutKind::Sound {
                lock_unpoisoned(&st.last_audio_state).clone()
            } else {
                None
            };

            if is_same_width_preview_scrub {
                st.flyout_backdrop_shown_gen.store(show_gen, Ordering::SeqCst);
                if let (Some(hwnd), Some((_, prev_rect))) = (fly_hwnd, prev_bounds) {
                    win_shell::set_flyout_non_activating(non_activating);
                    win_shell::set_window_hit_transparent(hwnd, false);
                    glide_flyout_with_backdrop(
                        Arc::clone(&st.flyout_glide_gen),
                        hwnd,
                        prev_rect.left,
                        layout.rect.left,
                        layout.rect.top,
                        PREVIEW_GLIDE_DURATION_MS,
                    );
                }
                if actions.emit_flyout_show {
                    let _ = app.emit("flyout-show", &enriched);
                }
            } else {
                if let (Some(fly), Some(hwnd)) = (&fly_win, fly_hwnd) {
                    win_shell::set_flyout_non_activating(non_activating);
                    win_shell::set_window_hit_transparent(hwnd, false);
                    apply_webview_bounds(
                        fly,
                        hwnd,
                        layout.rect.left,
                        layout.rect.top,
                        layout.rect.width(),
                        layout.rect.height(),
                    );
                    // Keep backdrop visible across in-place OSD / kind switches; arm hidden on cold open.
                    backdrop::attach_or_sync(
                        "flyout",
                        hwnd,
                        layout.rect,
                        fly_insets,
                        fly_radius,
                        &glass_depth,
                        was_visible,
                    );
                    if was_visible {
                        st.flyout_backdrop_shown_gen.store(show_gen, Ordering::SeqCst);
                    }
                }
                if req.kind == FlyoutKind::Sound {
                    if let Some(ref aud_cached) = cached_audio_for_sound {
                        emit_flyout_audio_if_dirty(app, &st, aud_cached);
                    }
                    let media_snap = media::get_media_snapshot();
                    emit_flyout_media_if_dirty(app, &st, &media_snap);
                }
                if actions.emit_flyout_show {
                    let _ = app.emit("flyout-show", &enriched);
                }
                if let (Some(fly), Some(hwnd)) = (&fly_win, fly_hwnd) {
                    win_shell::set_window_visible(hwnd, true, non_activating);
                    if !non_activating {
                        let _ = fly.set_focus();
                    }
                    if was_visible {
                        backdrop::sync_z_order("flyout", hwnd);
                    } else {
                        let h_guard = app.clone();
                        let anim_gen = Arc::clone(&st.flyout_anim_gen);
                        let shown_gen = Arc::clone(&st.flyout_backdrop_shown_gen);
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(160)).await;
                            if anim_gen.load(Ordering::SeqCst) == show_gen
                                && shown_gen.load(Ordering::SeqCst) != show_gen
                                && lock_unpoisoned(&h_guard.state::<AppState>().coordinator)
                                    .is_flyout_visible()
                            {
                                shown_gen.store(show_gen, Ordering::SeqCst);
                                backdrop::set_visible("flyout", true);
                                backdrop::sync_z_order("flyout", hwnd);
                            }
                        });
                    }
                }
            }

            // Hydrate Flyout immediately on hidden -> visible transition (AD-6)
            let snapshot = lock_unpoisoned(&st.last_window_groups).clone();
            let _ = app.emit_to("flyout", "windows-updated", &snapshot);
            match req.kind {
                FlyoutKind::Tray => {
                    let h_tray = app.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        let _ = refresh_cached_tray_apps(&h_tray, true);
                    });
                }
                FlyoutKind::Sound => {
                    let h_aud = app.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        let st_aud = h_aud.state::<AppState>();
                        let aud = sample_audio_state_with_icon_sources(&st_aud);
                        if lock_unpoisoned(&st_aud.coordinator).active_flyout()
                            == Some(FlyoutKind::Sound)
                        {
                            emit_flyout_audio_if_dirty(&h_aud, &st_aud, &aud);
                            let _ = h_aud.emit_to(
                                "flyout",
                                "audio-master-updated",
                                &aud.to_master_state(),
                            );
                        }
                        let med = media::poll_media_state();
                        if lock_unpoisoned(&st_aud.coordinator).active_flyout()
                            == Some(FlyoutKind::Sound)
                        {
                            emit_flyout_media_if_dirty(&h_aud, &st_aud, &med);
                        }
                        let net = system_widgets::get_network_info();
                        if lock_unpoisoned(&st_aud.coordinator).active_flyout()
                            == Some(FlyoutKind::Sound)
                        {
                            let _ = h_aud.emit_to("flyout", "network-updated", &net);
                        }
                    });
                }
                FlyoutKind::Calendar => {
                    let alerts = system_widgets::get_session_alerts();
                    let _ = app.emit_to("flyout", "session-alerts-updated", &alerts);
                }
                FlyoutKind::Weather => {
                    let h_weather = app.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        let st_w = h_weather.state::<AppState>();
                        let (city_opt, lat_opt, lon_opt) = {
                            let cfg = lock_unpoisoned(&st_w.config);
                            let trimmed = cfg.taskbar_widgets.weather_city.trim();
                            let city = (!trimmed.is_empty()).then(|| trimmed.to_string());
                            (
                                city,
                                cfg.taskbar_widgets.weather_lat,
                                cfg.taskbar_widgets.weather_lon,
                            )
                        };
                        let snap = weather::get_weather(city_opt.as_deref(), lat_opt, lon_opt, false);
                        if lock_unpoisoned(&st_w.coordinator).active_flyout()
                            == Some(FlyoutKind::Weather)
                        {
                            let _ = h_weather.emit_to("flyout", "weather-updated", &snap);
                        }
                    });
                }
                FlyoutKind::Battery => {
                    let h_bat = app.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        let periphs = battery::get_peripheral_batteries();
                        let st_b = h_bat.state::<AppState>();
                        if lock_unpoisoned(&st_b.coordinator).active_flyout()
                            == Some(FlyoutKind::Battery)
                        {
                            let _ = h_bat.emit_to("flyout", "peripherals-updated", &periphs);
                        }
                    });
                }
                _ => {}
            }
        }
    }
}

fn close_start_and_flyout(app: &AppHandle, close_start: bool, close_flyout: bool) {
    let state = app.state::<AppState>();
    let now = monotonic_ms();
    let (start_act, flyout_act) = {
        let mut coord = lock_unpoisoned(&state.coordinator);
        let s = if close_start {
            coord.request_toggle_start(Some(false), now)
        } else {
            CoordinatorActions::default()
        };
        let f = if close_flyout {
            coord.request_close_flyout()
        } else {
            CoordinatorActions::default()
        };
        state.sync_overlay_flag(&coord);
        (s, f)
    };
    apply_coordinator_actions(app, &start_act, None);
    apply_coordinator_actions(app, &flyout_act, None);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlyoutRequest {
    pub kind: FlyoutKind,
    pub anchor_x_logical: f64, // relative to taskbar window left, or screen logical X
    pub anchor_in_taskbar: bool,
    pub width: i32,
    pub height: i32,
    #[serde(default)]
    pub no_activate: bool,
    #[serde(default)]
    pub monitor: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default)]
    pub gen: u64,
    #[serde(default)]
    pub anchor_origin_x: Option<f64>,
}

/// Atomically repairs any broken/empty app pin targets across all pin lists using the latest
/// indexed installed apps & live open window groups, persisting and emitting `config-updated`
/// ONLY if at least one pin actually changed.
fn maybe_heal_broken_pins(app: &AppHandle) {
    let st = app.state::<AppState>();
    let (apps, groups) = (
        lock_unpoisoned(&st.installed_apps).clone(),
        lock_unpoisoned(&st.last_window_groups).clone(),
    );
    let _ = mutate_config_and_emit(app, &st, |cfg| {
        config::heal_broken_pins(cfg, &apps, &groups)
    });
}

fn run_background_index_refresh(app: &AppHandle) {
    let st = app.state::<AppState>();
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
    maybe_heal_broken_pins(app);

    let games = apps_indexer::scan_games_lightweight();
    *lock_unpoisoned(&st.steam_games) = games.clone();
    let _ = app.emit_to("start", "games-indexed", &games);

    let files = apps_indexer::scan_user_files();
    *lock_unpoisoned(&st.indexed_files) = files.clone();
    let _ = app.emit_to("start", "files-indexed", &files);
}

fn emit_media_to_visible_surfaces(app: &AppHandle, st: &MediaState) {
    let app_state = app.state::<AppState>();
    let (start_vis, sound_vis) = {
        let coord = lock_unpoisoned(&app_state.coordinator);
        (
            coord.is_start_visible(),
            coord.active_flyout() == Some(FlyoutKind::Sound),
        )
    };
    if start_vis {
        emit_start_media_if_dirty(app, &app_state, st);
    }
    if sound_vis {
        emit_flyout_media_if_dirty(app, &app_state, st);
    }
}

// ======================= CONFIG TRANSACTION AUTHORITY =======================

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

fn mutate_config_with_emitter<F, FEmit>(
    config_mutex: &Mutex<TestBarConfig>,
    path: &std::path::Path,
    mutate: F,
    emit: FEmit,
) -> Result<(TestBarConfig, bool), String>
where
    F: FnOnce(&mut TestBarConfig) -> bool,
    FEmit: FnOnce(&TestBarConfig),
{
    let (updated, changed) = mutate_config_at(config_mutex, path, mutate)?;
    if changed {
        emit(&updated);
    }
    Ok((updated, changed))
}

fn mutate_config_and_emit<F>(
    app: &AppHandle,
    state: &AppState,
    mutate: F,
) -> Result<(TestBarConfig, bool), String>
where
    F: FnOnce(&mut TestBarConfig) -> bool,
{
    let path = config::config_path();
    mutate_config_with_emitter(&state.config, &path, mutate, |updated| {
        lock_unpoisoned(&state.start_sent).config_rev = Some(updated.revision);
        let _ = app.emit("config-updated", updated);
    })
}

fn apply_autostart_transaction<FReg>(
    config_mutex: &Mutex<TestBarConfig>,
    path: &std::path::Path,
    enable: bool,
    mut apply_registry: FReg,
) -> Result<(TestBarConfig, bool), String>
where
    FReg: FnMut(bool) -> Result<(), String>,
{
    let mut guard = lock_unpoisoned(config_mutex);
    let prev_enabled = guard.autostart;
    apply_registry(enable)?;
    if prev_enabled == enable {
        return Ok((guard.clone(), false));
    }
    let mut candidate = guard.clone();
    candidate.autostart = enable;
    if let Err(err) = normalize_bump_and_save(&mut candidate, path) {
        let _ = apply_registry(prev_enabled);
        return Err(err);
    }
    *guard = candidate.clone();
    Ok((candidate, true))
}

// ======================= TAURI IPC COMMANDS =======================

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
                serde::de::Error::custom("expected json body"),
            )
            .into()),
        }
    }
}

#[tauri::command]
fn get_config(state: State<AppState>) -> TestBarConfig {
    lock_unpoisoned(&state.config).clone()
}

#[tauri::command]
fn set_bar_mode(
    app: AppHandle,
    state: State<AppState>,
    mode: String,
) -> Result<TestBarConfig, String> {
    let target_mode = if mode == "edge" { "edge" } else { "dock" };
    let path = config::config_path();
    let (updated, changed) = mutate_config_at(&state.config, &path, |cfg| {
        if cfg.bar_mode == target_mode {
            return false;
        }
        cfg.bar_mode = target_mode.to_string();
        true
    })?;
    if changed {
        if state.hide_native {
            win_shell::update_appbar_mode(BarMode::from_config_str(&updated.bar_mode));
        }
        position_taskbar_window(&app);
        sync_secondary_taskbar(&app);
        let actions = lock_unpoisoned(&state.coordinator).on_taskbar_bounds_updated();
        apply_coordinator_actions(&app, &actions, None);
        lock_unpoisoned(&state.start_sent).config_rev = Some(updated.revision);
        let _ = app.emit("config-updated", &updated);
    }
    Ok(updated)
}

#[tauri::command]
fn set_secondary_taskbar_mode(
    app: AppHandle,
    state: State<AppState>,
    mode: String,
) -> Result<TestBarConfig, String> {
    let target_mode = config::validate_secondary_taskbar_mode(&mode)?;
    let path = config::config_path();
    let (updated, changed) = mutate_config_at(&state.config, &path, |cfg| {
        if cfg.secondary_taskbar_mode == target_mode {
            return false;
        }
        cfg.secondary_taskbar_mode = target_mode.to_string();
        true
    })?;
    sync_secondary_taskbar(&app);
    if changed {
        lock_unpoisoned(&state.start_sent).config_rev = Some(updated.revision);
        let _ = app.emit("config-updated", &updated);
    }
    Ok(updated)
}

#[tauri::command]
fn set_autostart(
    app: AppHandle,
    state: State<AppState>,
    enable: bool,
) -> Result<TestBarConfig, String> {
    let path = config::config_path();
    let (updated, changed) =
        apply_autostart_transaction(&state.config, &path, enable, config::set_windows_autostart)?;
    if changed {
        lock_unpoisoned(&state.start_sent).config_rev = Some(updated.revision);
        let _ = app.emit("config-updated", &updated);
    }
    Ok(updated)
}

#[tauri::command]
fn resize_taskbar_dock(
    app: AppHandle,
    state: State<AppState>,
    logical_width: i32,
    monitor: Option<String>,
) {
    if monitor.as_deref() == Some("secondary") {
        *lock_unpoisoned(&state.secondary_taskbar_content_width) = Some(logical_width);
        position_secondary_taskbar_window(&app);
        return;
    }
    *lock_unpoisoned(&state.taskbar_content_width) = logical_width;
    position_taskbar_window(&app);
    let actions = lock_unpoisoned(&state.coordinator).on_taskbar_bounds_updated();
    apply_coordinator_actions(&app, &actions, None);
}

#[tauri::command]
fn toggle_start(app: AppHandle, force: Option<bool>, monitor: Option<String>) {
    dispatch_coordinator(&app, None, |coord| {
        coord.request_toggle_start_on_monitor(force, monitor, monotonic_ms())
    });
}

#[tauri::command]
fn open_start_settings(app: AppHandle) {
    dispatch_coordinator(&app, None, |coord| {
        coord.request_toggle_start(Some(true), monotonic_ms())
    });
    let _ = app.emit_to("start", "open-start-settings", ());
}

#[tauri::command]
fn open_flyout(app: AppHandle, mut req: FlyoutRequest) {
    req.no_activate = req.kind.is_non_activating();
    let monitor = req.monitor.clone();
    dispatch_coordinator(&app, Some(&req), |coord| {
        coord.request_open_flyout_on_monitor(req.kind, monitor.as_deref(), monotonic_ms())
    });
}

#[tauri::command]
fn surface_commit_ack(app: AppHandle, surface: String, gen: u64, measured_height: Option<f64>) {
    apply_surface_measured_height(&app, &surface, gen, measured_height);
}

#[tauri::command]
fn surface_exit_ack(app: AppHandle, surface: String, gen: u64) {
    commit_pending_surface_exit(&app, &surface, gen);
}

#[tauri::command]
fn get_backdrop_mode() -> &'static str {
    if backdrop::is_active() {
        "dcomp"
    } else {
        "none"
    }
}

/// Atomically claims the one-shot DComp entrance animation for `gen`.
/// Returns `false` if the surface is hidden, `gen` is stale, or the backdrop for `gen` was
/// already shown (e.g. in-place preview horizontal glide, repeated OSD tick, or duplicate call).
fn claim_backdrop_enter_gen(
    current_anim_gen: &AtomicU64,
    shown_gen: &AtomicU64,
    gen: u64,
    is_visible: bool,
) -> bool {
    if !is_visible || current_anim_gen.load(Ordering::SeqCst) != gen {
        return false;
    }
    shown_gen.swap(gen, Ordering::SeqCst) != gen
}

#[tauri::command]
fn animate_surface_in(
    app: AppHandle,
    state: State<AppState>,
    surface: String,
    gen: u64,
    duration_ms: u32,
    offset_y: f32,
    from_scale: f32,
) {
    // Always run TestBar's crisp DComp spring entrance regardless of Windows'
    // `SPI_GETCLIENTAREAANIMATION` setting (still honors explicit `duration_ms == 0`).
    match surface.as_str() {
        "start" => {
            let is_vis = lock_unpoisoned(&state.coordinator).is_start_visible();
            if !claim_backdrop_enter_gen(
                &state.start_anim_gen,
                &state.start_backdrop_shown_gen,
                gen,
                is_vis,
            ) {
                return;
            }
            let sf = lock_unpoisoned(&state.active_start_metrics)
                .map(|m| m.scale_factor)
                .unwrap_or_else(|| scale_factor(&app));
            backdrop::animate_in("start", duration_ms, offset_y * sf as f32, from_scale);
        }
        "flyout" => {
            let is_vis = lock_unpoisoned(&state.coordinator).is_flyout_visible();
            if !claim_backdrop_enter_gen(
                &state.flyout_anim_gen,
                &state.flyout_backdrop_shown_gen,
                gen,
                is_vis,
            ) {
                return;
            }
            let sf = lock_unpoisoned(&state.active_flyout_metrics)
                .map(|m| m.scale_factor)
                .unwrap_or_else(|| scale_factor(&app));
            backdrop::animate_in("flyout", duration_ms, offset_y * sf as f32, from_scale);
        }
        _ => {}
    }
}

#[tauri::command]
fn animate_surface_out(
    app: AppHandle,
    state: State<AppState>,
    surface: String,
    gen: u64,
    duration_ms: u32,
    offset_y: f32,
    to_scale: f32,
) {
    // Always run TestBar's DComp exit transition regardless of Windows'
    // `SPI_GETCLIENTAREAANIMATION` setting (still honors explicit `duration_ms == 0`).
    match surface.as_str() {
        "start" => {
            if state.start_anim_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            let sf = lock_unpoisoned(&state.active_start_metrics)
                .map(|m| m.scale_factor)
                .unwrap_or_else(|| scale_factor(&app));
            backdrop::animate_out("start", duration_ms, offset_y * sf as f32, to_scale);
        }
        "flyout" => {
            if state.flyout_anim_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            let sf = lock_unpoisoned(&state.active_flyout_metrics)
                .map(|m| m.scale_factor)
                .unwrap_or_else(|| scale_factor(&app));
            backdrop::animate_out("flyout", duration_ms, offset_y * sf as f32, to_scale);
        }
        _ => {}
    }
}

#[tauri::command]
fn dismiss_overlays(app: AppHandle) {
    close_start_and_flyout(&app, true, true);
}

#[tauri::command]
fn resize_flyout(app: AppHandle, state: State<AppState>, width: Option<i32>, height: i32) {
    let active_kind = lock_unpoisoned(&state.coordinator).active_flyout();
    let Some(kind) = active_kind else {
        return;
    };
    let updated_req = {
        let mut slot = lock_unpoisoned(&state.last_flyout_req);
        let Some(req) = slot.as_mut() else {
            return;
        };
        if let Some(w) = width {
            req.width = w.clamp(120, 1200);
        }
        req.height = height.clamp(48, 620);
        req.clone()
    };
    let glass_depth = lock_unpoisoned(&state.config).glass_depth.clone();
    if let Some(fly) = app.get_webview_window("flyout") {
        if let Some(hwnd) = win_hwnd(&fly) {
            let layout = position_flyout_window(&app, &fly, hwnd, &updated_req);
            *lock_unpoisoned(&state.last_flyout_bounds) = Some((kind, layout.rect));
            if let Some(metrics) = lock_unpoisoned(&state.active_flyout_metrics).as_mut() {
                metrics.rect = layout.rect;
                metrics.bottom_anchor_y = layout.bottom_anchor_y;
                metrics.monitor_top = layout.monitor_top;
                metrics.max_h_logical = layout.max_h_logical;
            }
            let (insets, radius) = flyout_backdrop_insets_and_radius(kind, layout.scale_factor);
            let shown = state.flyout_backdrop_shown_gen.load(Ordering::SeqCst)
                == state.flyout_anim_gen.load(Ordering::SeqCst);
            backdrop::attach_or_sync(
                "flyout",
                hwnd,
                layout.rect,
                insets,
                radius,
                &glass_depth,
                shown,
            );
        }
    }
}

#[tauri::command]
fn close_flyout(app: AppHandle) {
    dispatch_coordinator(&app, None, |coord| coord.request_close_flyout());
}

#[tauri::command]
fn close_osd(app: AppHandle) {
    dispatch_coordinator(&app, None, |coord| coord.request_close_osd());
}

#[tauri::command]
fn close_preview_if_outside(app: AppHandle, state: State<AppState>) {
    let is_preview =
        lock_unpoisoned(&state.coordinator).active_flyout() == Some(FlyoutKind::Preview);
    if !is_preview {
        return;
    }
    let Some(fly) = app.get_webview_window("flyout") else { return };
    let Some(hwnd_raw) = win_hwnd(&fly) else { return };
    if win_shell::is_cursor_near_window(hwnd_raw, 6, 6, 18) {
        return;
    }
    dispatch_coordinator(&app, None, |coord| coord.request_close_preview());
}

#[tauri::command]
async fn get_open_windows(
    app: AppHandle,
    monitor: Option<String>,
) -> Result<Vec<AppWindowGroup>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (groups, sources) =
            windows_tracker::list_open_window_groups_cached_with_sources(&state.icon_cache);
        state.register_icon_sources(sources);
        *lock_unpoisoned(&state.last_window_groups) = groups.clone();
        match monitor.as_deref() {
            Some(mon) if !mon.trim().is_empty() => {
                windows_tracker::filter_window_groups_for_monitor(&groups, mon)
            }
            _ => groups,
        }
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_icons(app: AppHandle, keys: Vec<String>) -> Result<HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut resolved = lock_unpoisoned(&state.icon_cache).get_batch(&keys);
        let missing: Vec<(String, String)> = {
            let sources = lock_unpoisoned(&state.icon_sources);
            keys.iter()
                .filter(|k| !k.is_empty() && !resolved.contains_key(*k))
                .filter_map(|k| sources.get(k).map(|src| (k.clone(), src.clone())))
                .collect()
        };
        for (key, src) in missing {
            if let Some(uri) =
                windows_tracker::extract_icon_for_path_cached(&src, &state.icon_cache)
            {
                resolved.insert(key, uri);
            }
        }
        resolved
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[derive(Debug, Clone, Default)]
struct PinIconLookupHint {
    title: String,
    icon_key: String,
    match_exe: Option<String>,
    match_aumid: Option<String>,
}

fn collect_pin_hints_for_target(cfg: &TestBarConfig, target: &str) -> Vec<PinIconLookupHint> {
    let needle = target.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hints = Vec::new();
    let lists: [&[PinItem]; 8] = [
        &cfg.taskbar_pins,
        &cfg.personal_pins,
        &cfg.personal_links,
        &cfg.work_folders,
        &cfg.work_apps,
        &cfg.studio_pins,
        &cfg.studio_tools,
        &cfg.game_launchers,
    ];
    for list in lists {
        for pin in list {
            if pin.target.trim().eq_ignore_ascii_case(needle) {
                hints.push(PinIconLookupHint {
                    title: pin.title.clone(),
                    icon_key: pin.icon_key.clone(),
                    match_exe: pin.match_exe.clone(),
                    match_aumid: pin.match_aumid.clone(),
                });
            }
        }
    }
    for sp in &cfg.spaces {
        for pin in &sp.items {
            if pin.target.trim().eq_ignore_ascii_case(needle) {
                hints.push(PinIconLookupHint {
                    title: pin.title.clone(),
                    icon_key: pin.icon_key.clone(),
                    match_exe: pin.match_exe.clone(),
                    match_aumid: pin.match_aumid.clone(),
                });
            }
        }
    }
    for sp in &cfg.custom_spaces {
        for pin in &sp.items {
            if pin.target.trim().eq_ignore_ascii_case(needle) {
                hints.push(PinIconLookupHint {
                    title: pin.title.clone(),
                    icon_key: pin.icon_key.clone(),
                    match_exe: pin.match_exe.clone(),
                    match_aumid: pin.match_aumid.clone(),
                });
            }
        }
    }
    hints
}

fn extract_exe_basename_lower(path: &str) -> Option<String> {
    let norm = path.trim().trim_matches('"').replace('/', "\\");
    if norm.is_empty() || norm.contains("://") {
        return None;
    }
    let file_name = norm.rsplit('\\').next().unwrap_or(&norm).trim();
    let low = file_name.to_lowercase();
    if low.ends_with(".exe") && low.len() > 4 {
        Some(low)
    } else {
        None
    }
}

fn extract_target_stem_lower(target: &str) -> String {
    let t = target.trim().trim_matches('"');
    let low = t.to_lowercase();
    if low.starts_with("http://") || low.starts_with("https://") {
        let after = low
            .split_once("://")
            .map(|(_, r)| r)
            .unwrap_or(&low)
            .split('/')
            .next()
            .unwrap_or("");
        if after.contains("music.yandex") {
            return "ymusic".to_string();
        }
        if after.contains("claude.ai") {
            return "claude".to_string();
        }
        if after.contains("figma.com") {
            return "figma".to_string();
        }
        return after
            .strip_prefix("www.")
            .unwrap_or(after)
            .split('.')
            .next()
            .unwrap_or("")
            .to_string();
    }
    if low.starts_with("steam://") {
        return "steam".to_string();
    }
    if low.starts_with("ms-screenclip:") {
        return "snipping".to_string();
    }
    let norm = t.replace('/', "\\");
    let base = norm.rsplit('\\').next().unwrap_or(&norm);
    base.strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .or_else(|| base.strip_suffix(".lnk"))
        .or_else(|| base.strip_suffix(".LNK"))
        .unwrap_or(base)
        .trim()
        .to_lowercase()
}

fn is_brand_icon_key(key: &str) -> bool {
    matches!(
        key,
        "spark"
            | "claude"
            | "yandex"
            | "ymusic"
            | "discord"
            | "telegram"
            | "obsidian"
            | "steam"
            | "figma"
            | "photoshop"
            | "lightroom"
            | "obs"
            | "zen"
            | "vscode"
            | "cursor"
            | "antigravity"
            | "battlenet"
            | "unity"
            | "wiztree"
    )
}

fn expand_app_aliases(token: &str, out: &mut Vec<String>) {
    match token {
        "wt" | "windowsterminal" | "терминал" | "windows terminal" => {
            for a in ["windows terminal", "терминал", "windowsterminal"] {
                out.push(a.to_string());
            }
        }
        "spark" | "sparkdesktop" | "spark mail" => {
            for a in ["spark", "spark mail", "spark desktop", "sparkdesktop"] {
                out.push(a.to_string());
            }
        }
        "claude" | "anthropicclaude" => {
            for a in ["claude", "anthropicclaude"] {
                out.push(a.to_string());
            }
        }
        "yandex" | "яндекс" | "яндекс браузер" | "yandex browser" | "browser" => {
            for a in ["яндекс браузер", "yandex browser", "yandex", "яндекс"] {
                out.push(a.to_string());
            }
        }
        "ymusic" | "я.музыка" | "яндекс музыка" | "yandex music" | "yandexmusic" => {
            for a in ["яндекс музыка", "yandex music", "yandexmusic"] {
                out.push(a.to_string());
            }
        }
        "discord" => {
            out.push("discord".to_string());
        }
        "snipping" | "ножницы" => {
            for a in ["ножницы", "snipping tool", "screensketch"] {
                out.push(a.to_string());
            }
        }
        _ => {}
    }
}

/// Matches a pin target (e.g. bare exe name, WindowsApps alias, Squirrel Update.exe, UWP app,
/// or web fallback like `https://claude.ai`) against indexed installed apps (`state.installed_apps`),
/// returning the best reloadable icon source (`shell:AppsFolder\<AUMID>` or real `.lnk`/`.exe`).
fn find_matching_indexed_app_icon_source(
    target: &str,
    hints: &[PinIconLookupHint],
    installed_apps: &[IndexedApp],
) -> Option<String> {
    if installed_apps.is_empty() {
        return None;
    }

    let mut target_exes: Vec<String> = Vec::new();
    let mut target_aumids: Vec<String> = Vec::new();
    let mut exact_names: Vec<String> = Vec::new();
    let mut alias_tokens: Vec<String> = Vec::new();

    if let Some(exe) = extract_exe_basename_lower(target) {
        if exe != "update.exe" {
            target_exes.push(exe);
        }
    }
    let stem = extract_target_stem_lower(target);
    if !stem.is_empty() && stem != "update" {
        exact_names.push(stem.clone());
        expand_app_aliases(&stem, &mut alias_tokens);
    }

    // If target path contains parent app folder name (e.g. `...\Discord\Update.exe` or
    // `...\SparkDesktop\SparkDesktop.exe`), capture that folder name as an extra signal.
    let norm_target = target.trim().trim_matches('"').replace('/', "\\");
    if let Some(parent_name) = std::path::Path::new(&norm_target)
        .parent()
        .and_then(|p| p.file_name())
        .map(|s| s.to_string_lossy().to_lowercase())
    {
        if !matches!(
            parent_name.as_str(),
            "" | "application" | "bin" | "64bit" | "windowsapps" | "system32" | "windows" | "programs"
        ) {
            exact_names.push(parent_name.clone());
            expand_app_aliases(&parent_name, &mut alias_tokens);
        }
    }

    for h in hints {
        if let Some(ref a) = h.match_aumid {
            let clean = a.trim().to_lowercase();
            if !clean.is_empty() {
                target_aumids.push(clean);
            }
        }
        if let Some(ref e) = h.match_exe {
            let clean = e.trim().to_lowercase();
            if !clean.is_empty() && clean != "update.exe" {
                target_exes.push(clean.clone());
                let estem = clean.strip_suffix(".exe").unwrap_or(&clean);
                expand_app_aliases(estem, &mut alias_tokens);
            }
        }
        let title_low = h.title.trim().to_lowercase();
        if !title_low.is_empty() {
            exact_names.push(title_low.clone());
            expand_app_aliases(&title_low, &mut alias_tokens);
        }
        let ik = h.icon_key.trim().to_lowercase();
        if is_brand_icon_key(&ik) {
            exact_names.push(ik.clone());
            expand_app_aliases(&ik, &mut alias_tokens);
        }
    }

    let wants_music = exact_names
        .iter()
        .chain(alias_tokens.iter())
        .any(|t| t.contains("музык") || t.contains("music") || t == "ymusic");
    let wants_yandex_browser = !wants_music
        && (target_exes.iter().any(|e| e == "browser.exe")
            || exact_names
                .iter()
                .chain(alias_tokens.iter())
                .any(|t| t.contains("yandex") || t.contains("яндекс")));

    let mut best_score: i32 = 0;
    let mut best_source: Option<String> = None;

    for app in installed_apps {
        let app_name_low = app.name.trim().to_lowercase();
        let app_aumid_low = app
            .aumid
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_default();
        let app_exe_base = extract_exe_basename_lower(&app.target);
        let app_target_stem = extract_target_stem_lower(&app.target);

        let app_is_music =
            app_name_low.contains("музык") || app_name_low.contains("music") || app_target_stem.contains("music");
        if wants_yandex_browser && app_is_music {
            continue;
        }
        if wants_music && !app_is_music && (app_name_low.contains("yandex") || app_name_low.contains("яндекс")) {
            continue;
        }

        let mut score: i32 = 0;

        // 1. Exact AUMID match
        if !app_aumid_low.is_empty() && target_aumids.iter().any(|a| a == &app_aumid_low) {
            score = score.max(120);
        }

        // 2. Exact .exe filename match (ignoring generic launcher stubs like update.exe)
        if let Some(ref app_exe) = app_exe_base {
            if app_exe != "update.exe" && target_exes.iter().any(|e| e == app_exe) {
                score = score.max(100);
            }
        }

        // 3. Exact display name match
        if exact_names.iter().any(|n| n == &app_name_low)
            || alias_tokens.iter().any(|a| a == &app_name_low)
        {
            score = score.max(92);
        }

        // 4. Strong AUMID package token match (e.g. Microsoft.WindowsTerminal, Claude, Readdle.Spark)
        if !app_aumid_low.is_empty() {
            for tok in alias_tokens.iter().chain(exact_names.iter()) {
                if tok.len() >= 4 && !tok.contains(' ') && app_aumid_low.contains(tok.as_str()) {
                    score = score.max(85);
                }
            }
        }

        // 5. Prefix / word-level match on display name or target stem
        for tok in alias_tokens.iter().chain(exact_names.iter()) {
            if tok.len() < 3 {
                continue;
            }
            if app_name_low == *tok
                || app_name_low.starts_with(&format!("{tok} "))
                || app_name_low.ends_with(&format!(" {tok}"))
                || app_target_stem == *tok
            {
                score = score.max(76);
            } else if tok.len() >= 5
                && (app_name_low.contains(tok.as_str()) || app_target_stem.contains(tok.as_str()))
            {
                score = score.max(68);
            }
        }

        if score < 65 {
            continue;
        }
        // Slight tie-breaker favoring packaged UWP apps (`shell:AppsFolder\<aumid>`) for crisp 96px tiles
        if app.aumid.is_some() {
            score += 2;
        }
        if score > best_score {
            best_score = score;
            best_source = if let Some(ref aumid) = app.aumid {
                if aumid.to_ascii_lowercase().starts_with("shell:appsfolder\\") {
                    Some(aumid.clone())
                } else {
                    Some(format!("shell:AppsFolder\\{aumid}"))
                }
            } else if !app.target.trim().is_empty() {
                Some(app.target.clone())
            } else {
                None
            };
        }
    }

    best_source
}

fn find_matching_running_group_icon_source(
    target: &str,
    hints: &[PinIconLookupHint],
    groups: &[AppWindowGroup],
) -> Option<String> {
    let mut want_exes: Vec<String> = Vec::new();
    if let Some(exe) = extract_exe_basename_lower(target) {
        if exe != "update.exe" {
            want_exes.push(exe);
        }
    }
    for h in hints {
        if let Some(ref e) = h.match_exe {
            let low = e.trim().to_lowercase();
            if !low.is_empty() && low != "update.exe" {
                want_exes.push(low);
            }
        }
    }
    for g in groups {
        if let Some(ref aumid) = g.aumid {
            if hints.iter().any(|h| {
                h.match_aumid
                    .as_deref()
                    .is_some_and(|ma| ma.eq_ignore_ascii_case(aumid))
            }) {
                return Some(format!("shell:AppsFolder\\{aumid}"));
            }
        }
        if let Some(g_exe) = extract_exe_basename_lower(&g.exe_path) {
            if want_exes.iter().any(|w| w == &g_exe) {
                if let Some(ref aumid) = g.aumid {
                    return Some(format!("shell:AppsFolder\\{aumid}"));
                }
                if !g.exe_path.trim().is_empty() {
                    return Some(g.exe_path.clone());
                }
            }
        }
    }
    None
}

fn icon_cache_key_for_source(src: &str) -> String {
    let low = src.trim().to_ascii_lowercase();
    if low.starts_with("shell:appsfolder\\") {
        icon_cache::aumid_key(src)
    } else if low.starts_with("ms-settings:") {
        icon_cache::aumid_key(windows_tracker::WINDOWS_SETTINGS_AUMID)
    } else {
        icon_cache::path_icon_key(src)
    }
}

#[tauri::command]
async fn resolve_pin_icons(
    app: AppHandle,
    targets: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut out = HashMap::new();
        let mut seen = std::collections::HashSet::new();
        let mut new_sources = Vec::new();
        let mut needs_index_lookup: Vec<(String, bool)> = Vec::new();

        for raw in targets {
            let trimmed = raw.trim();
            if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
                continue;
            }
            let low = trimmed.to_ascii_lowercase();
            let is_winapps_alias = windows_tracker::is_windows_apps_execution_alias(trimmed);
            let is_ms_settings = low.starts_with("ms-settings:");
            let is_uri_scheme = trimmed.contains("://")
                || (low.starts_with("ms-") && !is_ms_settings)
                || low.starts_with("mailto:");

            // Prefer UWP / installed_apps match FIRST for 0-byte WindowsApps aliases (e.g. wt.exe)
            // and URL fallbacks (e.g. https://claude.ai when Claude Desktop is installed) so they
            // never get stuck with a blank/missing icon.
            if is_winapps_alias || is_uri_scheme {
                needs_index_lookup.push((trimmed.to_string(), is_winapps_alias));
                continue;
            }

            if let Some(uri) =
                windows_tracker::extract_icon_for_path_cached(trimmed, &state.icon_cache)
            {
                let key = icon_cache_key_for_source(trimmed);
                let registered_src = if is_ms_settings {
                    windows_tracker::WINDOWS_SETTINGS_AUMID.to_string()
                } else {
                    trimmed.to_string()
                };
                new_sources.push((key, registered_src));
                out.insert(trimmed.to_string(), uri);
            } else {
                needs_index_lookup.push((trimmed.to_string(), false));
            }
        }

        if !needs_index_lookup.is_empty() {
            // If startup background app indexing is still running and installed_apps is empty,
            // wait briefly (on this blocking worker thread) so UWP/Squirrel/bare pins resolve on first paint.
            if lock_unpoisoned(&state.installed_apps).is_empty() {
                for _ in 0..30 {
                    if !lock_unpoisoned(&state.installed_apps).is_empty()
                        || !state.index_refresh_running.load(Ordering::Acquire)
                    {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }

            let apps_snap = lock_unpoisoned(&state.installed_apps).clone();
            let cfg_snap = lock_unpoisoned(&state.config).clone();
            let groups_snap = lock_unpoisoned(&state.last_window_groups).clone();

            for (trimmed, is_winapps_alias) in needs_index_lookup {
                let hints = collect_pin_hints_for_target(&cfg_snap, &trimmed);
                let matched_source =
                    find_matching_indexed_app_icon_source(&trimmed, &hints, &apps_snap)
                        .or_else(|| {
                            find_matching_running_group_icon_source(&trimmed, &hints, &groups_snap)
                        });

                if let Some(src) = matched_source {
                    if let Some(uri) =
                        windows_tracker::extract_icon_for_path_cached(&src, &state.icon_cache)
                    {
                        let key = icon_cache_key_for_source(&src);
                        new_sources.push((key, src));
                        out.insert(trimmed, uri);
                        continue;
                    }
                }

                // Last-resort direct extraction for WindowsApps alias if no indexed UWP match existed
                if is_winapps_alias {
                    if let Some(uri) =
                        windows_tracker::extract_icon_for_path_cached(&trimmed, &state.icon_cache)
                    {
                        let key = icon_cache_key_for_source(&trimmed);
                        new_sources.push((key, trimmed.clone()));
                        out.insert(trimmed, uri);
                    }
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
async fn get_jump_list(
    app: AppHandle,
    exe_path: String,
    aumid: Option<String>,
) -> Result<Vec<JumpListItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let items = windows_tracker::get_jump_list(&exe_path, aumid.as_deref());
        let key = jump_list_cache_key(&exe_path, aumid.as_deref());
        let st = app.state::<AppState>();
        let mut cache = lock_unpoisoned(&st.jump_list_cache);
        if cache.len() >= MAX_JUMP_LIST_CACHE_ENTRIES && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.insert(key, items.clone());
        items
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_window_thumbnail(hwnd: isize) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        windows_tracker::capture_window_thumbnail(hwnd, 440, 260)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn focus_window(app: AppHandle, hwnd: isize, toggle_minimize: bool) -> Result<(), String> {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        close_start_and_flyout(&h, false, true);
    });
    tauri::async_runtime::spawn_blocking(move || {
        windows_tracker::focus_or_toggle_window(hwnd, toggle_minimize);
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
fn close_window_hwnd(hwnd: isize) {
    windows_tracker::close_window(hwnd);
}

#[tauri::command]
async fn snap_window(hwnd: isize, zone: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || windows_tracker::snap_window(hwnd, &zone))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn toggle_window_topmost(hwnd: isize) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || windows_tracker::toggle_window_topmost(hwnd))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn minimize_other_windows(keep_hwnd: isize) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        windows_tracker::minimize_other_windows(keep_hwnd)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn kill_process_pid(pid: u32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || windows_tracker::terminate_process_by_pid(pid))
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
    as_admin: bool,
    close_start: bool,
) -> Result<(), String> {
    if close_start {
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            close_start_and_flyout(&h, true, true);
        });
    }
    tauri::async_runtime::spawn_blocking(move || {
        windows_tracker::launch_target_with_args(&target, args.as_deref(), as_admin)
    })
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
    if close_start.unwrap_or(false) {
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            close_start_and_flyout(&h, true, true);
        });
    } else {
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            close_start_and_flyout(&h, false, true);
        });
    }
    tauri::async_runtime::spawn_blocking(move || {
        windows_tracker::launch_new_window_instance(&exe_path, args.as_deref())
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn launch_combo(app: AppHandle, targets: Vec<String>) -> Result<(), String> {
    let has_target = targets.iter().any(|t| !t.trim().is_empty());
    if has_target {
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            close_start_and_flyout(&h, true, false);
        });
    }
    tauri::async_runtime::spawn_blocking(move || windows_tracker::launch_combo_targets(&targets))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn open_location(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || windows_tracker::open_file_location(&path))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn open_terminal_at(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || windows_tracker::open_in_terminal(&path))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
fn get_installed_apps(state: State<AppState>) -> Vec<IndexedApp> {
    lock_unpoisoned(&state.installed_apps).clone()
}

#[tauri::command]
fn get_indexed_files(state: State<AppState>) -> Vec<IndexedFile> {
    lock_unpoisoned(&state.indexed_files).clone()
}

#[tauri::command]
fn get_steam_games(state: State<AppState>) -> Vec<SteamGame> {
    lock_unpoisoned(&state.steam_games).clone()
}

#[tauri::command]
async fn load_game_cover(cover_path: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || apps_indexer::load_game_cover_data(&cover_path))
        .await
        .map_err(|e| format!("task join error: {e}"))
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
async fn add_pin(
    app: AppHandle,
    space: String,
    mut item: PinItem,
) -> Result<TestBarConfig, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let force_combo = config::should_force_combo(&space, &item.kind);
        config::normalize_pin(&mut item, force_combo);

        // Resolve icon into unified IconCache & register icon_sources; NEVER store inline base64 icon_data in config.json!
        let target_trimmed = item.target.trim().to_string();
        if (item.kind == "app" || item.kind == "file") && !target_trimmed.is_empty() {
            let low = target_trimmed.to_ascii_lowercase();
            let is_ms_settings = low.starts_with("ms-settings:");
            let is_resolvable = target_trimmed.contains(":\\")
                || target_trimmed.starts_with("\\\\")
                || low.starts_with("shell:appsfolder\\")
                || is_ms_settings;
            if is_resolvable {
                let key = icon_cache_key_for_source(&target_trimmed);
                if windows_tracker::extract_icon_for_path_cached(&target_trimmed, &state.icon_cache)
                    .is_some()
                {
                    let source_to_register = if is_ms_settings {
                        windows_tracker::WINDOWS_SETTINGS_AUMID.to_string()
                    } else {
                        target_trimmed
                    };
                    state.register_icon_sources(vec![(key.clone(), source_to_register)]);
                    if item.icon_key.trim().is_empty()
                        || item.icon_key == "app"
                        || item.icon_key == "file"
                        || item.icon_key == "settings"
                    {
                        item.icon_key = key;
                    }
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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

fn sanitize_custom_space_accent(
    accent_hex: Option<&str>,
    accent_rgb: Option<&str>,
) -> (String, String) {
    config::sanitize_accent_hex_rgb(accent_hex, accent_rgb)
}

#[tauri::command]
fn add_custom_space(
    app: AppHandle,
    state: State<AppState>,
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
    let (hex, rgb) = sanitize_custom_space_accent(accent_hex.as_deref(), accent_rgb.as_deref());
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
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
fn add_game_combo(
    app: AppHandle,
    state: State<AppState>,
    name: String,
    subtitle: Option<String>,
    accent: Option<String>,
    steps: Vec<String>,
) -> Result<TestBarConfig, String> {
    let id = format!("combo-{}", now_ms());
    let sub = subtitle.unwrap_or_default();
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::add_game_combo(
            cfg,
            id.clone(),
            name.clone(),
            sub.clone(),
            accent.clone(),
            steps.clone(),
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
fn update_game_combo(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    name: Option<String>,
    subtitle: Option<String>,
    accent: Option<String>,
    steps: Option<Vec<String>>,
) -> Result<TestBarConfig, String> {
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::update_game_combo(
            cfg,
            &id,
            name.clone(),
            subtitle.clone(),
            accent.clone(),
            steps.clone(),
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
fn delete_game_combo(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::delete_game_combo(cfg, &id)
    })?;
    Ok(updated)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn set_appearance_settings(
    app: AppHandle,
    state: State<AppState>,
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
    let glass_changed_hint = glass_depth.is_some() || reset_visuals.unwrap_or(false);
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
    if glass_changed_hint {
        backdrop::set_glass_depth(&updated.glass_depth);
    }
    Ok(updated)
}

#[tauri::command]
fn save_quick_notes_text(
    app: AppHandle,
    state: State<AppState>,
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
    state: State<AppState>,
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
    state: State<AppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, |cfg| config::toggle_quick_todo(cfg, &id))?;
    Ok(updated)
}

#[tauri::command]
fn delete_quick_todo(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, |cfg| config::delete_quick_todo(cfg, &id))?;
    Ok(updated)
}

#[tauri::command]
fn clear_done_quick_todos(app: AppHandle, state: State<AppState>) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, config::clear_done_quick_todos)?;
    Ok(updated)
}

#[tauri::command]
fn hide_tray_app(
    app: AppHandle,
    state: State<AppState>,
    key: String,
) -> Result<TestBarConfig, String> {
    let (updated, changed) =
        mutate_config_and_emit(&app, &state, |cfg| config::hide_tray_key(cfg, &key))?;
    if changed {
        let h = app.clone();
        std::thread::spawn(move || {
            let _ = refresh_cached_tray_apps(&h, true);
        });
    }
    Ok(updated)
}

#[tauri::command]
fn restore_tray_app(
    app: AppHandle,
    state: State<AppState>,
    key: String,
) -> Result<TestBarConfig, String> {
    let (updated, changed) =
        mutate_config_and_emit(&app, &state, |cfg| config::restore_tray_key(cfg, &key))?;
    if changed {
        let h = app.clone();
        std::thread::spawn(move || {
            let _ = refresh_cached_tray_apps(&h, true);
        });
    }
    Ok(updated)
}

#[tauri::command]
fn restore_all_tray_apps(
    app: AppHandle,
    state: State<AppState>,
) -> Result<TestBarConfig, String> {
    let (updated, changed) =
        mutate_config_and_emit(&app, &state, config::restore_all_tray_keys)?;
    if changed {
        let h = app.clone();
        std::thread::spawn(move || {
            let _ = refresh_cached_tray_apps(&h, true);
        });
    }
    Ok(updated)
}

#[tauri::command]
fn hide_battery_device(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::hide_battery_device(cfg, &id)
    })?;
    Ok(updated)
}

#[tauri::command]
fn restore_battery_device(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::restore_battery_device(cfg, &id)
    })?;
    Ok(updated)
}

#[tauri::command]
fn restore_all_battery_devices(
    app: AppHandle,
    state: State<AppState>,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, config::restore_all_battery_devices)?;
    Ok(updated)
}

#[tauri::command]
fn record_frecency(
    app: AppHandle,
    state: State<AppState>,
    item_id: String,
) -> Result<TestBarConfig, String> {
    let ts = now_ms();
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::record_frecency(cfg, &item_id, ts)
    })?;
    Ok(updated)
}

fn refocus_start_if_open(app: &AppHandle, state: &AppState) {
    if lock_unpoisoned(&state.coordinator).is_start_open() {
        if let Some(win) = app.get_webview_window("start") {
            if let Some(hwnd) = win_hwnd(&win) {
                win_shell::force_focus_window(hwnd);
                backdrop::sync_z_order("start", hwnd);
            }
            let _ = win.set_focus();
        }
    }
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _modal_guard = ModalPickerGuard::acquire(&state.coordinator);
        let picked = apps_indexer::pick_folder_dialog();
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            refocus_start_if_open(&h, &h.state::<AppState>());
        });
        picked
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn pick_file(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _modal_guard = ModalPickerGuard::acquire(&state.coordinator);
        let picked = apps_indexer::pick_file_dialog();
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            refocus_start_if_open(&h, &h.state::<AppState>());
        });
        picked
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
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

#[tauri::command]
async fn pick_custom_icon_image(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _modal_guard = ModalPickerGuard::acquire(&state.coordinator);
        let picked = rfd::FileDialog::new()
            .set_title("Выбери изображение для иконки")
            .add_filter("Изображения", &["png", "jpg", "jpeg", "webp", "ico", "bmp"])
            .pick_file();
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            refocus_start_if_open(&h, &h.state::<AppState>());
        });
        let Some(path) = picked else {
            return Ok(None);
        };
        load_custom_icon_image_data_uri(&path).map(Some)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn export_config_dialog(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _modal_guard = ModalPickerGuard::acquire(&state.coordinator);
        let cfg_snapshot = lock_unpoisoned(&state.config).clone();
        let picked = rfd::FileDialog::new()
            .set_title("Экспорт конфигурации TestBar")
            .add_filter("JSON", &["json"])
            .set_file_name("testbar-config.json")
            .save_file();
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            refocus_start_if_open(&h, &h.state::<AppState>());
        });
        let Some(path) = picked else {
            return Ok(None);
        };
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
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _modal_guard = ModalPickerGuard::acquire(&state.coordinator);
        let picked = rfd::FileDialog::new()
            .set_title("Импорт конфигурации TestBar")
            .add_filter("JSON", &["json"])
            .pick_file();
        let h_refocus = app.clone();
        let _ = app.run_on_main_thread(move || {
            refocus_start_if_open(&h_refocus, &h_refocus.state::<AppState>());
        });
        let Some(path) = picked else {
            return Ok(None);
        };
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("Не удалось прочитать файл конфигурации: {e}"))?;
        let imported = config::import_config_json(&raw)?;
        let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
            let prev_rev = cfg.revision;
            *cfg = imported;
            // Preserve strictly monotonic revision across imports so frontend revision gates accept it
            cfg.revision = prev_rev;
            true
        })?;
        let clips = system_widgets::hydrate_pinned_clipboard(&updated.pinned_clipboard);
        if should_emit_start_clipboard_update(&lock_unpoisoned(&state.coordinator)) {
            emit_start_clips_if_dirty(&app, &state, &clips);
        }
        let h_sync = app.clone();
        let bar_mode = updated.bar_mode.clone();
        let glass_depth = updated.glass_depth.clone();
        let hide_native = state.hide_native;
        let _ = app.run_on_main_thread(move || {
            backdrop::set_glass_depth(&glass_depth);
            if hide_native {
                win_shell::update_appbar_mode(BarMode::from_config_str(&bar_mode));
            }
            position_taskbar_window(&h_sync);
            sync_secondary_taskbar(&h_sync);
        });
        Ok(Some(updated))
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_audio_state(app: AppHandle) -> Result<AudioState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        sample_audio_state_with_icon_sources(&state)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_audio_peaks() -> Result<AudioPeaksSnapshot, String> {
    tauri::async_runtime::spawn_blocking(audio::get_audio_peaks)
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_audio_master_state() -> Result<AudioMasterState, String> {
    tauri::async_runtime::spawn_blocking(audio::get_audio_master_state)
        .await
        .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn set_master_volume(
    app: AppHandle,
    volume: u32,
    muted: Option<bool>,
) -> Result<AudioState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        audio::set_master_volume(volume, muted)?;
        let st = sample_audio_state_with_icon_sources(&app.state::<AppState>());
        emit_audio_state_targeted(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn set_mic_muted(app: AppHandle, muted: bool) -> Result<AudioState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        audio::set_mic_muted(muted)?;
        let st = sample_audio_state_with_icon_sources(&app.state::<AppState>());
        emit_audio_state_targeted(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn set_session_volume(
    app: AppHandle,
    pid: u32,
    pids: Option<Vec<u32>>,
    app_key: Option<String>,
    volume: u32,
    muted: Option<bool>,
) -> Result<AudioState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        audio::set_session_volume_for_app(
            pid,
            pids.as_deref(),
            app_key.as_deref(),
            volume,
            muted,
        )?;
        let st = sample_audio_state_with_icon_sources(&app.state::<AppState>());
        emit_audio_state_targeted(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

/// Strictly clamps a WASAPI per-app session volume scalar into `0.0..=1.0` (non-finite -> `0.0`).
/// Removes any `> 1.0` heuristic so `0.01` always maps to 1% and never collides with `1.0` (100%).
fn normalize_session_volume_scalar(v: f32) -> f32 {
    if !v.is_finite() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

#[tauri::command]
async fn set_session_state(
    app: AppHandle,
    app_key: String,
    volume: Option<f32>,
    muted: Option<bool>,
    pid: Option<u32>,
    pids: Option<Vec<u32>>,
) -> Result<AudioState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let normalized_vol = volume.map(normalize_session_volume_scalar);
        if pid.unwrap_or(0) != 0 || pids.as_ref().is_some_and(|p| !p.is_empty()) {
            let vol_pct = normalized_vol.map(|s| (s * 100.0).round() as u32);
            audio::set_session_state_for_app_pct(
                pid.unwrap_or(0),
                pids.as_deref(),
                (!app_key.trim().is_empty()).then_some(app_key.as_str()),
                vol_pct,
                muted,
            )?;
        } else {
            audio::set_session_state_for_app(&app_key, normalized_vol, muted)?;
        }
        let st = sample_audio_state_with_icon_sources(&app.state::<AppState>());
        emit_audio_state_targeted(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn set_audio_device(app: AppHandle, device_id: String) -> Result<AudioState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = audio::set_default_audio_device(&device_id) {
            let _ = system_widgets::push_session_alert(
                "warn",
                "Ошибка переключения звука",
                Some(err.clone()),
            );
            emit_session_alerts_if_calendar_open(&app);
            return Err(err);
        }
        let st = sample_audio_state_with_icon_sources(&app.state::<AppState>());
        emit_audio_state_targeted(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn set_input_device(app: AppHandle, device_id: String) -> Result<AudioState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = audio::set_default_input_device(&device_id) {
            let _ = system_widgets::push_session_alert(
                "warn",
                "Ошибка переключения микрофона",
                Some(err.clone()),
            );
            emit_session_alerts_if_calendar_open(&app);
            return Err(err);
        }
        let st = sample_audio_state_with_icon_sources(&app.state::<AppState>());
        emit_audio_state_targeted(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_media_state() -> Result<MediaState, String> {
    tauri::async_runtime::spawn_blocking(media::get_media_state)
        .await
        .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn media_action(app: AppHandle, action: String) -> Result<MediaState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = media::media_action(&action)?;
        emit_media_to_visible_surfaces(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn seek_media(app: AppHandle, position_sec: f64) -> Result<MediaState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = media::seek_media(position_sec)?;
        emit_media_to_visible_surfaces(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn select_media_session(app: AppHandle, session_id: String) -> Result<MediaState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let st = media::select_media_session(session_id)?;
        emit_media_to_visible_surfaces(&app, &st);
        Ok(st)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_telemetry(app: AppHandle) -> Result<TelemetryState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        sample_telemetry_with_icon_sources(&state)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
fn get_clipboard_history() -> Vec<ClipboardEntry> {
    system_widgets::get_clipboard_history()
}

#[tauri::command]
fn copy_to_clipboard(text: String) -> Result<(), String> {
    system_widgets::write_clipboard_text(&text)
}

#[tauri::command]
fn toggle_clip_pin(app: AppHandle, state: State<AppState>, id: String) -> Vec<ClipboardEntry> {
    let mut list = system_widgets::toggle_clipboard_pin(&id);
    if let Some(entry) = list.iter().find(|c| c.id == id) {
        let text = entry.text.clone();
        let pinned = entry.pinned;
        if mutate_config_and_emit(&app, &state, |cfg| {
            config::set_pinned_clipboard(cfg, &text, pinned)
        })
        .is_err()
        {
            list = system_widgets::toggle_clipboard_pin(&id);
        }
    }
    if should_emit_start_clipboard_update(&lock_unpoisoned(&state.coordinator)) {
        emit_start_clips_if_dirty(&app, &state, &list);
    }
    list
}

#[tauri::command]
fn delete_clipboard_entry(
    app: AppHandle,
    state: State<AppState>,
    id: String,
) -> Vec<ClipboardEntry> {
    let removed_text = system_widgets::get_clipboard_history()
        .into_iter()
        .find(|c| c.id == id && c.pinned)
        .map(|c| c.text);
    let list = system_widgets::delete_clipboard_entry(&id);
    if let Some(text) = removed_text {
        let _ = mutate_config_and_emit(&app, &state, |cfg| {
            config::set_pinned_clipboard(cfg, &text, false)
        });
    }
    if should_emit_start_clipboard_update(&lock_unpoisoned(&state.coordinator)) {
        emit_start_clips_if_dirty(&app, &state, &list);
    }
    list
}

#[tauri::command]
fn set_clipboard_paused(paused: bool) -> bool {
    system_widgets::set_clipboard_paused(paused)
}

#[tauri::command]
fn get_clipboard_paused() -> bool {
    system_widgets::is_clipboard_paused()
}

#[tauri::command]
async fn paste_clipboard_entry(app: AppHandle, text: String) -> Result<(), String> {
    let target_hwnd = app.state::<AppState>().last_external_fg.load(Ordering::Acquire);
    let (hide_tx, hide_rx) = std::sync::mpsc::sync_channel::<()>(1);
    let h = app.clone();
    let dispatch_res = app.run_on_main_thread(move || {
        dispatch_coordinator(&h, None, |coord| coord.hide_start_immediate());
        let _ = hide_tx.send(());
    });
    if dispatch_res.is_err() {
        // Main thread unavailable; avoid blocking on channel forever
        drop(hide_rx);
        return tauri::async_runtime::spawn_blocking(move || {
            system_widgets::paste_clipboard_text(target_hwnd, &text)
        })
        .await
        .map_err(|e| format!("task join error: {e}"))?;
    }
    tauri::async_runtime::spawn_blocking(move || {
        // Wait deterministically for main-thread SW_HIDE of Start before restoring target & pasting
        let _ = hide_rx.recv_timeout(std::time::Duration::from_millis(250));
        system_widgets::paste_clipboard_text(target_hwnd, &text)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
fn clear_unpinned_clips(app: AppHandle, state: State<AppState>) -> Vec<ClipboardEntry> {
    let list = system_widgets::clear_unpinned_clipboard();
    if should_emit_start_clipboard_update(&lock_unpoisoned(&state.coordinator)) {
        emit_start_clips_if_dirty(&app, &state, &list);
    }
    list
}

#[tauri::command]
fn get_keyboard_layout() -> String {
    system_widgets::get_keyboard_layout_code()
}

#[tauri::command]
async fn switch_keyboard_layout(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let code = system_widgets::switch_keyboard_layout();
        let _ = app.emit("layout-updated", &code);
        code
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_peripherals() -> Result<Vec<PeripheralBattery>, String> {
    tauri::async_runtime::spawn_blocking(system_widgets::get_peripheral_batteries)
        .await
        .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_peripheral_batteries() -> Result<Vec<battery::PeripheralBattery>, String> {
    tauri::async_runtime::spawn_blocking(battery::get_peripheral_batteries)
        .await
        .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn refresh_peripheral_batteries(
    app: AppHandle,
) -> Result<Vec<battery::PeripheralBattery>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let list = battery::refresh_peripheral_batteries();
        let _ = app.emit_to("start", "peripherals-updated", &list);
        let _ = app.emit_to("flyout", "peripherals-updated", &list);
        list
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn list_directory(
    path: String,
    sort_by: Option<String>,
    sort_dir: Option<String>,
    show_hidden: Option<bool>,
) -> Result<file_explorer::DirectoryListing, String> {
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
async fn fs_create_folder(
    parent_path: String,
    name: String,
) -> Result<file_explorer::FileEntry, String> {
    tauri::async_runtime::spawn_blocking(move || {
        file_explorer::fs_create_folder(&parent_path, &name)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn fs_rename_item(
    path: String,
    new_name: String,
) -> Result<file_explorer::FileEntry, String> {
    tauri::async_runtime::spawn_blocking(move || file_explorer::fs_rename_item(&path, &new_name))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn fs_copy_files_to_clipboard(paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        file_explorer::fs_copy_files_to_clipboard(paths)
    })
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
) -> Result<weather::WeatherSnapshot, String> {
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
            let st = app.state::<AppState>();
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
async fn search_weather_cities(query: String) -> Result<Vec<weather::WeatherCityOption>, String> {
    tauri::async_runtime::spawn_blocking(move || weather::search_weather_cities(&query))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_tray_apps(app: AppHandle) -> Result<Vec<TrayAppItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (open_groups, win_sources) =
            windows_tracker::list_open_window_groups_cached_with_sources(&state.icon_cache);
        state.register_icon_sources(win_sources);
        *lock_unpoisoned(&state.last_window_groups) = open_groups.clone();

        let hidden_keys = lock_unpoisoned(&state.config).hidden_tray_keys.clone();
        let (tray_items, tray_sources) =
            system_widgets::list_tray_background_apps_filtered_with_open_mutex(
                &state.icon_cache,
                &open_groups,
                &hidden_keys,
            );
        state.register_icon_sources(tray_sources);
        *lock_unpoisoned(&state.cached_tray_apps) = tray_items.clone();
        state.tray_cache_warmed.store(true, Ordering::Release);
        tray_items
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn activate_tray_app(
    app: AppHandle,
    hwnd: Option<isize>,
    exe_path: String,
) -> Result<(), String> {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        close_start_and_flyout(&h, false, true);
    });
    tauri::async_runtime::spawn_blocking(move || system_widgets::activate_tray_app(hwnd, &exe_path))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn end_tray_app_task(pids: Vec<u32>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || system_widgets::end_tray_app_task(&pids))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn open_native_windows_tray(app: AppHandle) -> Result<(), String> {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        close_start_and_flyout(&h, false, true);
    });
    tauri::async_runtime::spawn_blocking(system_widgets::trigger_native_tray_overflow)
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn empty_recycle_bin() -> Result<QuickTogglesState, String> {
    tauri::async_runtime::spawn_blocking(quick_toggles::empty_recycle_bin_and_refresh)
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn open_screen_snip(app: AppHandle) -> Result<(), String> {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        close_start_and_flyout(&h, true, true);
    });
    std::thread::sleep(std::time::Duration::from_millis(120));
    tauri::async_runtime::spawn_blocking(system_widgets::open_screen_snip)
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn trigger_screenshot(app: AppHandle) -> Result<(), String> {
    open_screen_snip(app).await
}

#[tauri::command]
async fn get_network_snapshot(probe_ping: Option<bool>) -> Result<NetworkSnapshot, String> {
    let do_probe = probe_ping.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || network_monitor::get_network_snapshot(do_probe))
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
async fn set_power_plan(plan: String) -> Result<QuickTogglesState, String> {
    tauri::async_runtime::spawn_blocking(move || quick_toggles::set_power_plan(&plan))
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
fn get_session_alerts() -> Vec<SessionAlert> {
    system_widgets::get_session_alerts()
}

#[tauri::command]
fn clear_session_alerts(app: AppHandle) -> Vec<SessionAlert> {
    let list = system_widgets::clear_session_alerts();
    let _ = app.emit_to("flyout", "session-alerts-updated", &list);
    list
}

#[tauri::command]
async fn open_windows_notifications(app: AppHandle) -> Result<(), String> {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        close_start_and_flyout(&h, false, true);
    });
    tauri::async_runtime::spawn_blocking(system_widgets::open_windows_notifications)
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn get_network_info() -> Result<NetworkInfo, String> {
    tauri::async_runtime::spawn_blocking(system_widgets::get_network_info)
        .await
        .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
fn power_action(action: String) -> Result<(), String> {
    system_widgets::system_power_action(&action)
}

#[tauri::command]
fn restore_windows_taskbar_and_exit(app: AppHandle, state: State<AppState>) {
    backdrop::shutdown();
    win_shell::stop_shell_hook_thread();
    if state.hide_native {
        win_shell::restore_native_taskbar();
    }
    app.exit(0);
}

#[tauri::command]
async fn get_icon_for_path(app: AppHandle, path: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        windows_tracker::extract_icon_for_path_cached(&path, &state.icon_cache)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

pub fn run_watchdog_mode(args: WatchdogArgs) {
    win_shell::run_watchdog(args);
}

pub fn run(hide_native: bool) {
    // Acquire single-instance mutex BEFORE any shell mutation, work-area capture,
    // panic hook installation, or watchdog spawning (AD-2).
    let _single_instance = match win_shell::SingleInstanceGuard::acquire() {
        Ok(Some(guard)) => guard,
        Ok(None) | Err(_) => return,
    };

    if hide_native {
        win_shell::ensure_process_dpi_aware();
        let captured_wa =
            win_shell::capture_and_persist_original_workarea().map(|snap| snap.work_area);
        win_shell::install_panic_restore_hook();
        win_shell::spawn_watchdog(captured_wa);
    }

    let initial_cfg = config::load_config();
    let initial_pinned_clipboard = initial_cfg.pinned_clipboard.clone();
    win_shell::set_keep_native_secondary_taskbar(
        win_shell::secondary_native_taskbar_should_stay_visible(&initial_cfg.secondary_taskbar_mode),
    );

    let overlay_open = Arc::new(AtomicBool::new(false));
    let fullscreen_suppressed = Arc::new(AtomicBool::new(false));
    let secondary_fullscreen_suppressed = Arc::new(AtomicBool::new(false));
    let secondary_taskbar_visible = Arc::new(AtomicBool::new(false));
    let tray_cache_warmed = Arc::new(AtomicBool::new(false));
    let flyout_anim_gen = Arc::new(AtomicU64::new(0));
    let start_anim_gen = Arc::new(AtomicU64::new(0));
    let start_backdrop_shown_gen = Arc::new(AtomicU64::new(u64::MAX));
    let flyout_backdrop_shown_gen = Arc::new(AtomicU64::new(u64::MAX));
    let flyout_glide_gen = Arc::new(AtomicU64::new(0));
    let last_external_fg = Arc::new(AtomicIsize::new(0));
    let index_refresh_running = Arc::new(AtomicBool::new(false));
    let start_hydration_running = Arc::new(AtomicBool::new(false));
    let initial_glass_depth = initial_cfg.glass_depth.clone();
    win_shell::set_overlay_open_hint(false);
    let app_state = AppState {
        config: Mutex::new(initial_cfg),
        icon_cache: Mutex::new(IconCache::default()),
        icon_sources: Mutex::new(HashMap::new()),
        last_window_groups: Mutex::new(Vec::new()),
        cached_tray_apps: Mutex::new(Vec::new()),
        tray_cache_warmed: Arc::clone(&tray_cache_warmed),
        jump_list_cache: Mutex::new(HashMap::new()),
        last_audio_state: Mutex::new(None),
        installed_apps: Mutex::new(Vec::new()),
        indexed_files: Mutex::new(Vec::new()),
        steam_games: Mutex::new(Vec::new()),
        hide_native,
        coordinator: Mutex::new(WindowCoordinator::new()),
        overlay_open: Arc::clone(&overlay_open),
        fullscreen_suppressed: Arc::clone(&fullscreen_suppressed),
        secondary_fullscreen_suppressed: Arc::clone(&secondary_fullscreen_suppressed),
        taskbar_content_width: Mutex::new(620),
        secondary_taskbar_content_width: Mutex::new(None),
        secondary_taskbar_visible: Arc::clone(&secondary_taskbar_visible),
        primary_taskbar_raster_dpi: AtomicU32::new(0),
        secondary_taskbar_raster_dpi: AtomicU32::new(0),
        last_flyout_req: Mutex::new(None),
        last_flyout_bounds: Mutex::new(None),
        active_start_metrics: Mutex::new(None),
        active_flyout_metrics: Mutex::new(None),
        last_start_rect: Mutex::new(None),
        start_sent: Mutex::new(StartSentSnapshot::default()),
        flyout_sent_audio: Mutex::new(None),
        flyout_sent_media_sig: Mutex::new(None),
        pending_start_exit: Mutex::new(None),
        pending_flyout_exit: Mutex::new(None),
        flyout_anim_gen: Arc::clone(&flyout_anim_gen),
        start_anim_gen: Arc::clone(&start_anim_gen),
        start_backdrop_shown_gen: Arc::clone(&start_backdrop_shown_gen),
        flyout_backdrop_shown_gen: Arc::clone(&flyout_backdrop_shown_gen),
        flyout_glide_gen: Arc::clone(&flyout_glide_gen),
        dpi_override: AtomicU32::new(0),
        last_external_fg: Arc::clone(&last_external_fg),
        index_refresh_running: Arc::clone(&index_refresh_running),
        start_hydration_running: Arc::clone(&start_hydration_running),
    };

    let app = tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_bar_mode,
            set_secondary_taskbar_mode,
            set_autostart,
            set_appearance_settings,
            resize_taskbar_dock,
            toggle_start,
            open_start_settings,
            open_flyout,
            surface_commit_ack,
            surface_exit_ack,
            get_backdrop_mode,
            animate_surface_in,
            animate_surface_out,
            dismiss_overlays,
            resize_flyout,
            close_flyout,
            close_osd,
            close_preview_if_outside,
            get_open_windows,
            get_icons,
            resolve_pin_icons,
            get_jump_list,
            get_window_thumbnail,
            focus_window,
            close_window_hwnd,
            snap_window,
            toggle_window_topmost,
            minimize_other_windows,
            kill_process_pid,
            kill_process_group,
            launch_item,
            launch_new_window,
            launch_combo,
            open_location,
            open_terminal_at,
            get_installed_apps,
            get_indexed_files,
            get_steam_games,
            load_game_cover,
            refresh_indexes,
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
            add_game_combo,
            update_game_combo,
            delete_game_combo,
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
            get_audio_state,
            get_audio_peaks,
            get_audio_master_state,
            set_master_volume,
            set_mic_muted,
            set_session_volume,
            set_session_state,
            set_audio_device,
            set_input_device,
            get_media_state,
            media_action,
            seek_media,
            select_media_session,
            get_telemetry,
            get_clipboard_history,
            copy_to_clipboard,
            toggle_clip_pin,
            delete_clipboard_entry,
            set_clipboard_paused,
            get_clipboard_paused,
            paste_clipboard_entry,
            clear_unpinned_clips,
            get_keyboard_layout,
            switch_keyboard_layout,
            get_peripherals,
            get_peripheral_batteries,
            refresh_peripheral_batteries,
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
            get_tray_apps,
            activate_tray_app,
            end_tray_app_task,
            open_native_windows_tray,
            empty_recycle_bin,
            open_screen_snip,
            trigger_screenshot,
            get_network_snapshot,
            get_quick_toggles_state,
            set_keep_awake,
            set_power_plan,
            get_session_alerts,
            clear_session_alerts,
            open_windows_notifications,
            get_network_info,
            power_action,
            restore_windows_taskbar_and_exit,
            get_icon_for_path,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            backdrop::set_glass_depth(&initial_glass_depth);

            // Style & subclass all 4 pre-warmed transparent borderless shell windows
            if let Some(tb) = app.get_webview_window("taskbar") {
                if let Some(hwnd) = win_hwnd(&tb) {
                    win_shell::style_overlay_window(hwnd, true);
                    win_shell::install_overlay_borderless_subclass(hwnd);
                    ensure_taskbar_dpi_only_scale(
                        &tb,
                        &app.state::<AppState>().primary_taskbar_raster_dpi,
                        primary_taskbar_dpi(&handle),
                    );
                    let mode = lock_unpoisoned(&app.state::<AppState>().config).bar_mode.clone();
                    if hide_native {
                        win_shell::hide_native_taskbar();
                        let h_appbar = handle.clone();
                        if let Err(err) = win_shell::start_appbar_message_window(
                            BarMode::from_config_str(&mode),
                            move |ev| {
                                let h = h_appbar.clone();
                                let _ = h_appbar.run_on_main_thread(move || match ev {
                                    ShellAppBarEvent::FullscreenHint(entering) => {
                                        let _ = reevaluate_fullscreen(&h, Some(entering));
                                    }
                                    ShellAppBarEvent::PositionChanged
                                    | ShellAppBarEvent::TaskbarCreated
                                    | ShellAppBarEvent::DisplayChanged
                                    | ShellAppBarEvent::DpiChanged(_) => {
                                        match ev {
                                            ShellAppBarEvent::DpiChanged(dpi) => {
                                                let st = h.state::<AppState>();
                                                st.set_dpi_override(dpi);
                                                st.primary_taskbar_raster_dpi.store(0, Ordering::Release);
                                                st.secondary_taskbar_raster_dpi.store(0, Ordering::Release);
                                                *lock_unpoisoned(&st.last_start_rect) = None;
                                            }
                                            ShellAppBarEvent::DisplayChanged => {
                                                let st = h.state::<AppState>();
                                                st.clear_dpi_override();
                                                st.primary_taskbar_raster_dpi.store(0, Ordering::Release);
                                                st.secondary_taskbar_raster_dpi.store(0, Ordering::Release);
                                                *lock_unpoisoned(&st.last_start_rect) = None;
                                            }
                                            ShellAppBarEvent::TaskbarCreated => {
                                                let _ = system_widgets::push_session_alert(
                                                    "info",
                                                    "Проводник перезапущен",
                                                    Some("Панель TestBar и рабочая область восстановлены".to_string()),
                                                );
                                                emit_session_alerts_if_calendar_open(&h);
                                            }
                                            _ => {}
                                        }
                                        let actions = reevaluate_fullscreen(&h, None);
                                        // If fullscreen just exited, apply_coordinator_actions already
                                        // repositioned the taskbar with the latest DPI/mode/width before showing it.
                                        if actions.taskbar_visible != Some(true) {
                                            position_taskbar_window(&h);
                                        }
                                        if actions.secondary_taskbar_visible != Some(true) {
                                            sync_secondary_taskbar(&h);
                                        }
                                    }
                                });
                            },
                        ) {
                            backdrop::shutdown();
                            win_shell::restore_native_taskbar();
                            return Err(err.into());
                        }
                    }
                    let _ = reevaluate_fullscreen(&handle, None);
                    position_taskbar_window(&handle);
                    if lock_unpoisoned(&app.state::<AppState>().coordinator).should_show_taskbar() {
                        win_shell::set_window_visible(hwnd, true, true);
                        backdrop::set_visible("taskbar", true);
                        backdrop::sync_z_order("taskbar", hwnd);
                    }
                }
            }

            if let Some(tb2) = app.get_webview_window("taskbar-secondary") {
                if let Some(hwnd) = win_hwnd(&tb2) {
                    win_shell::style_overlay_window(hwnd, true);
                    win_shell::install_overlay_borderless_subclass(hwnd);
                    if let Some(sec_mon) = win_shell::secondary_monitor_rects().into_iter().next() {
                        ensure_taskbar_dpi_only_scale(
                            &tb2,
                            &app.state::<AppState>().secondary_taskbar_raster_dpi,
                            sec_mon.dpi,
                        );
                    }
                }
                set_webview_memory_low(&tb2, true);
            }
            sync_secondary_taskbar(&handle);

            if let Some(start_win) = app.get_webview_window("start") {
                if let Some(hwnd) = win_hwnd(&start_win) {
                    win_shell::style_overlay_window(hwnd, false);
                    win_shell::install_overlay_borderless_subclass(hwnd);
                    let _ = position_start_window_on_monitor(&handle, &start_win, hwnd, None);
                }
                let h_blur = handle.clone();
                start_win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::Focused(false) = ev {
                        let _ = win_shell::refresh_foreground_elevated_state();
                        dispatch_coordinator(&h_blur, None, |coord| {
                            coord.on_start_focus_lost(monotonic_ms())
                        });
                    }
                });
            }

            if let Some(fly_win) = app.get_webview_window("flyout") {
                if let Some(hwnd) = win_hwnd(&fly_win) {
                    win_shell::style_overlay_window(hwnd, false);
                    win_shell::install_overlay_borderless_subclass(hwnd);
                    win_shell::install_flyout_noactivate_subclass(hwnd);
                }
                let h_blur = handle.clone();
                fly_win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::Focused(false) = ev {
                        let _ = win_shell::refresh_foreground_elevated_state();
                        dispatch_coordinator(&h_blur, None, |coord| {
                            coord.on_flyout_focus_lost(monotonic_ms())
                        });
                    }
                });
            }

            // Global lone-Win key hook, Win+L lock, global click-outside & Ctrl+Alt+Shift+Esc emergency restore.
            // Dispatch off the low-level hook thread onto the main GUI thread.
            let h_hook = handle.clone();
            let hook_overlay_open = Arc::clone(&overlay_open);
            win_shell::start_shell_hook_thread(move |ev| match ev {
                ShellHookEvent::ToggleStart => {
                    let h = h_hook.clone();
                    let _ = h_hook.run_on_main_thread(move || {
                        dispatch_coordinator(&h, None, |coord| {
                            coord.request_toggle_start(None, monotonic_ms())
                        });
                    });
                }
                ShellHookEvent::LockWorkstation => {
                    win_shell::lock_workstation();
                }
                ShellHookEvent::NativeStartSuppressed(start_menu_hwnd) => {
                    let h = h_hook.clone();
                    let _ = h_hook.run_on_main_thread(move || {
                        let st = h.state::<AppState>();
                        if lock_unpoisoned(&st.coordinator).is_start_open() {
                            refocus_start_if_open(&h, &st);
                        } else {
                            let stored = st.last_external_fg.load(Ordering::Acquire);
                            let fallback = if stored == start_menu_hwnd { 0 } else { stored };
                            win_shell::dismiss_native_start_menu(start_menu_hwnd, fallback);
                        }
                    });
                }
                ShellHookEvent::GlobalMouseDown(x, y) => {
                    if !hook_overlay_open.load(Ordering::Acquire) {
                        return;
                    }
                    let h = h_hook.clone();
                    let _ = h_hook.run_on_main_thread(move || {
                        let rects = active_shell_window_rects(&h);
                        dispatch_coordinator(&h, None, |coord| {
                            coord.on_global_mouse_down(x, y, &rects, monotonic_ms())
                        });
                    });
                }
                ShellHookEvent::EmergencyRestore => {
                    let h = h_hook.clone();
                    let _ = h_hook.run_on_main_thread(move || {
                        backdrop::shutdown();
                        if h.state::<AppState>().hide_native {
                            win_shell::restore_native_taskbar();
                        }
                        h.exit(0);
                    });
                }
            });

            // Dedicated MTA WinRT SMTC worker thread (AD-5)
            media::start_media_worker();

            // Pre-warm AudioState, Win11 NotifyIconSettings Tray cache, & auto-heal broken pins at startup
            let h_warm = handle.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || {
                let st = h_warm.state::<AppState>();
                let _ = sample_audio_state_with_icon_sources(&st);
                let _ = refresh_cached_tray_apps(&h_warm, true);
                maybe_heal_broken_pins(&h_warm);
            });

            // Live Clipboard History listener: hydrates persisted pins at startup, stays
            // continuously active even while Start is hidden, and gates IPC emission to visible Start.
            let h_clip = handle.clone();
            system_widgets::start_clipboard_listener(&initial_pinned_clipboard, move |entries| {
                let st_clip = h_clip.state::<AppState>();
                let emit = should_emit_start_clipboard_update(&lock_unpoisoned(&st_clip.coordinator));
                if emit {
                    emit_start_clips_if_dirty(&h_clip, &st_clip, &entries);
                }
            });

            // Async background app (Start .lnk + UWP), lightweight games (Steam/Epic/GOG), & user files indexer
            // Runs at startup and repeats cleanly every 15 minutes so new installs/downloads appear automatically.
            let h_idx = handle.clone();
            std::thread::spawn(move || loop {
                run_background_index_refresh(&h_idx);
                std::thread::sleep(std::time::Duration::from_secs(PERIODIC_REINDEX_INTERVAL_SECS));
            });

            // Event-driven desktop monitor: wakes immediately (~40ms coalesced) on OS foreground,
            // window create/destroy/show/hide/minimize events, and falls back to a low-overhead
            // 2.5s idle safety tick for keyboard layout, native taskbar guard, and fullscreen sync.
            let h_fast = handle.clone();
            let fast_fs_primary = Arc::clone(&fullscreen_suppressed);
            let fast_fs_secondary = Arc::clone(&secondary_fullscreen_suppressed);
            std::thread::spawn(move || {
                let mut last_layout = String::new();
                let mut last_windows_fp: Option<WindowsFingerprint> = None;
                let mut desktop_seq = win_shell::current_desktop_activity_seq();
                loop {
                    let _ = win_shell::refresh_foreground_elevated_state();
                    let (groups, win_sources) = {
                        let st = h_fast.state::<AppState>();
                        capture_external_foreground_hwnd(&st);
                        windows_tracker::list_open_window_groups_cached_with_sources(&st.icon_cache)
                    };
                    let targets = {
                        let st = h_fast.state::<AppState>();
                        st.register_icon_sources(win_sources);
                        let sec_vis = st.secondary_taskbar_visible.load(Ordering::Acquire);
                        let coord = lock_unpoisoned(&st.coordinator);
                        let maybe_targets = diff_window_update_targets_with_secondary(
                            &mut last_windows_fp,
                            &groups,
                            &coord,
                            sec_vis,
                        );
                        if maybe_targets.is_some() {
                            *lock_unpoisoned(&st.last_window_groups) = groups.clone();
                        }
                        maybe_targets
                    };
                    if let Some(targets) = targets {
                        emit_windows_to_targets(&h_fast, &targets, &groups);
                    }

                    let layout = system_widgets::get_keyboard_layout_code();
                    if layout != last_layout {
                        last_layout = layout.clone();
                        let _ = h_fast.emit("layout-updated", &layout);
                    }

                    if hide_native && !win_shell::is_restore_in_progress() {
                        win_shell::ensure_native_taskbar_hidden_if_needed();
                    }

                    let (fs, fg_is_own) = win_shell::detect_fullscreen_per_monitor_with_fg();
                    if !fg_is_own
                        && (fs.primary != fast_fs_primary.load(Ordering::Acquire)
                            || fs.secondary != fast_fs_secondary.load(Ordering::Acquire))
                    {
                        let h = h_fast.clone();
                        let _ = h_fast.run_on_main_thread(move || {
                            let _ = reevaluate_fullscreen(&h, None);
                        });
                    }

                    let woken = win_shell::wait_for_desktop_activity_or_timeout(
                        &mut desktop_seq,
                        std::time::Duration::from_millis(2500),
                    );
                    if woken {
                        std::thread::sleep(std::time::Duration::from_millis(40));
                        desktop_seq = win_shell::current_desktop_activity_seq();
                    }
                }
            });

            // Slower poller for split audio (diff-gated, visible-surface targeted with icon_sources),
            // diff-gated network info, 5s cached tray refresh, and hidden-window-gated Start/Sound widgets.
            let h_slow = handle.clone();
            std::thread::spawn(move || {
                let mut tick = 0u64;
                let mut last_audio_master: Option<AudioMasterState> = None;
                let mut last_audio_mixer: Option<AudioState> = None;
                let mut last_media: Option<MediaState> = None;
                let mut last_periphs: Option<Vec<PeripheralBattery>> = None;
                let mut last_network: Option<NetworkInfo> = None;
                loop {
                    let (mixer_needed, sound_flyout_open, slow_plan) = {
                        let st = h_slow.state::<AppState>();
                        let coord = lock_unpoisoned(&st.coordinator);
                        (
                            !coord.audio_mixer_targets().is_empty(),
                            coord.active_flyout() == Some(FlyoutKind::Sound),
                            plan_slow_poll_tick(&coord, tick, hide_native),
                        )
                    };

                    if mixer_needed {
                        let audio_st = sample_audio_state_with_icon_sources(&h_slow.state::<AppState>());
                        let master = audio_st.to_master_state();
                        let (master_targets, mixer_targets) = {
                            let st = h_slow.state::<AppState>();
                            let sec_vis = st.secondary_taskbar_visible.load(Ordering::Acquire);
                            let coord = lock_unpoisoned(&st.coordinator);
                            (
                                diff_audio_master_targets(
                                    &mut last_audio_master,
                                    &master,
                                    &coord,
                                    sec_vis,
                                ),
                                diff_audio_mixer_targets(&mut last_audio_mixer, &audio_st, &coord),
                            )
                        };
                        if let Some(targets) = master_targets {
                            for target in targets {
                                let _ = h_slow.emit_to(target, "audio-master-updated", &master);
                            }
                        }
                        if let Some(targets) = mixer_targets {
                            let st_slow = h_slow.state::<AppState>();
                            for target in targets {
                                if target == "start" {
                                    emit_start_audio_if_dirty(&h_slow, &st_slow, &audio_st);
                                } else if target == "flyout" {
                                    emit_flyout_audio_if_dirty(&h_slow, &st_slow, &audio_st);
                                } else {
                                    let _ = h_slow.emit_to(target, "audio-mixer-updated", &audio_st);
                                }
                            }
                        }
                    } else {
                        last_audio_mixer = None;
                        let master = audio::get_audio_master_state();
                        let master_targets = {
                            let st = h_slow.state::<AppState>();
                            let sec_vis = st.secondary_taskbar_visible.load(Ordering::Acquire);
                            let coord = lock_unpoisoned(&st.coordinator);
                            diff_audio_master_targets(
                                &mut last_audio_master,
                                &master,
                                &coord,
                                sec_vis,
                            )
                        };
                        if let Some(targets) = master_targets {
                            for target in targets {
                                let _ = h_slow.emit_to(target, "audio-master-updated", &master);
                            }
                        }
                    }

                    if tick % 3 == 0 {
                        let net = system_widgets::get_network_info();
                        if last_network.as_ref() != Some(&net) {
                            last_network = Some(net.clone());
                            let flyout_vis = lock_unpoisoned(&h_slow.state::<AppState>().coordinator)
                                .is_flyout_visible();
                            let _ = h_slow.emit_to("taskbar", "network-updated", &net);
                            if flyout_vis {
                                let _ = h_slow.emit_to("flyout", "network-updated", &net);
                            }
                        }
                    }

                    if tick % 6 == 0 {
                        // ~9.6s idle background tray cache refresh (diff-only emit `tray-updated` to flyout;
                        // tray also refreshes immediately on open and metadata is cached in system_widgets.rs)
                        let _ = refresh_cached_tray_apps(&h_slow, true);
                    }

                    if slow_plan.poll_media || sound_flyout_open {
                        let media_st = media::poll_media_state();
                        if last_media.as_ref() != Some(&media_st) {
                            last_media = Some(media_st.clone());
                            emit_media_to_visible_surfaces(&h_slow, &media_st);
                        }
                    } else {
                        last_media = None;
                    }

                    if !slow_plan.poll_media {
                        last_periphs = None;
                    }

                    if slow_plan.poll_telemetry {
                        let telem = sample_telemetry_with_icon_sources(&h_slow.state::<AppState>());
                        if should_poll_start_widgets(
                            &lock_unpoisoned(&h_slow.state::<AppState>().coordinator),
                        ) {
                            let _ = h_slow.emit_to("start", "telemetry-updated", &telem);
                        }
                    }

                    if slow_plan.poll_peripherals {
                        let periphs = system_widgets::get_peripheral_batteries();
                        if last_periphs.as_ref() != Some(&periphs)
                            && should_poll_start_widgets(
                                &lock_unpoisoned(&h_slow.state::<AppState>().coordinator),
                            )
                        {
                            last_periphs = Some(periphs.clone());
                            let _ = h_slow.emit_to("start", "peripherals-updated", &periphs);
                        }
                    }

                    if slow_plan.enforce_hide_native && !win_shell::is_restore_in_progress() {
                        win_shell::hide_native_taskbar();
                    }

                    tick += 1;
                    std::thread::sleep(std::time::Duration::from_millis(1600));
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error building TestBar");

    app.run(move |_handle, event| {
        if let tauri::RunEvent::Exit = event {
            quick_toggles::shutdown();
            backdrop::shutdown();
            win_shell::stop_shell_hook_thread();
            if hide_native {
                win_shell::restore_native_taskbar();
            }
        }
    });

    drop(_single_instance);
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn real_windows_subsystems_probe_cleanly() {
        let telem = telemetry::get_telemetry();
        assert!(telem.ram_total_gb > 0.0);
        println!(
            "Telemetry OK: CPU={} ({}%), RAM={}/{} GB, GPU={} ({}C, VRAM={}/{} GB)",
            telem.cpu_name, telem.cpu_pct, telem.ram_used_gb, telem.ram_total_gb,
            telem.gpu_name, telem.gpu_temp_c, telem.vram_used_gb, telem.vram_total_gb
        );

        let aud = audio::get_audio_state();
        let master = audio::get_audio_master_state();
        assert_eq!(aud.master_volume, master.master_volume);
        println!("Audio OK: vol={}%, muted={}, devices={}, sessions={}", aud.master_volume, aud.master_muted, aud.devices.len(), aud.sessions.len());

        let cache = Mutex::new(IconCache::default());
        let wins = windows_tracker::list_open_window_groups_cached(&cache);
        println!("Windows OK: {} groups", wins.len());

        let tray = system_widgets::list_tray_background_apps_cached(&cache, &wins);
        let tray_with_icons = tray.iter().filter(|t| t.icon_key.is_some()).count();
        println!("Tray OK: {} background apps ({} with cached icon_key)", tray.len(), tray_with_icons);

        let games = apps_indexer::scan_steam_games();
        println!("Steam OK: {} games found", games.len());

        let layout = system_widgets::get_keyboard_layout_code();
        assert!(!layout.is_empty());
        println!("Layout OK: {}", layout);
    }

    fn make_test_app_state(bar_mode: &str, width: i32) -> AppState {
        let mut cfg = TestBarConfig::default();
        cfg.bar_mode = bar_mode.to_string();
        AppState {
            config: Mutex::new(cfg),
            icon_cache: Mutex::new(IconCache::default()),
            icon_sources: Mutex::new(HashMap::new()),
            last_window_groups: Mutex::new(Vec::new()),
            cached_tray_apps: Mutex::new(Vec::new()),
            tray_cache_warmed: Arc::new(AtomicBool::new(false)),
            jump_list_cache: Mutex::new(HashMap::new()),
            last_audio_state: Mutex::new(None),
            installed_apps: Mutex::new(Vec::new()),
            indexed_files: Mutex::new(Vec::new()),
            steam_games: Mutex::new(Vec::new()),
            hide_native: false,
            coordinator: Mutex::new(WindowCoordinator::new()),
            overlay_open: Arc::new(AtomicBool::new(false)),
            fullscreen_suppressed: Arc::new(AtomicBool::new(false)),
            secondary_fullscreen_suppressed: Arc::new(AtomicBool::new(false)),
            taskbar_content_width: Mutex::new(width),
            secondary_taskbar_content_width: Mutex::new(None),
            secondary_taskbar_visible: Arc::new(AtomicBool::new(false)),
            primary_taskbar_raster_dpi: AtomicU32::new(0),
            secondary_taskbar_raster_dpi: AtomicU32::new(0),
            last_flyout_req: Mutex::new(None),
            last_flyout_bounds: Mutex::new(None),
            active_start_metrics: Mutex::new(None),
            active_flyout_metrics: Mutex::new(None),
            last_start_rect: Mutex::new(None),
            start_sent: Mutex::new(StartSentSnapshot::default()),
            flyout_sent_audio: Mutex::new(None),
            flyout_sent_media_sig: Mutex::new(None),
            pending_start_exit: Mutex::new(None),
            pending_flyout_exit: Mutex::new(None),
            flyout_anim_gen: Arc::new(AtomicU64::new(0)),
            start_anim_gen: Arc::new(AtomicU64::new(0)),
            start_backdrop_shown_gen: Arc::new(AtomicU64::new(u64::MAX)),
            flyout_backdrop_shown_gen: Arc::new(AtomicU64::new(u64::MAX)),
            flyout_glide_gen: Arc::new(AtomicU64::new(0)),
            dpi_override: AtomicU32::new(0),
            last_external_fg: Arc::new(AtomicIsize::new(0)),
            index_refresh_running: Arc::new(AtomicBool::new(false)),
            start_hydration_running: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn taskbar_bounds_suppressed_during_fullscreen_and_repositioned_on_exit() {
        let mon_rect = PixelRect::new(0, 0, 1920, 1080);
        let mon = win_shell::MonitorRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
            width: 1920,
            height: 1080,
            dpi: 96,
        };
        let borderless_game = coordinator::ForegroundWindowInfo {
            visible: true,
            is_own_process: false,
            class_name: "UnrealWindow".to_string(),
            style: 0x9000_0000,
            rect: mon_rect,
        };
        let state = make_test_app_state("dock", 620);

        // Normal desktop: bounds update allowed with initial Dock width 620 (+ 32px transparent margin = 652, 83px height)
        let initial_bounds = plan_taskbar_bounds_update(&state, mon, 1.0, 1.0)
            .expect("bounds should be allowed on normal desktop");
        assert_eq!(initial_bounds.width(), 652);
        assert_eq!(initial_bounds.height(), 83);
        assert_eq!(initial_bounds.bottom, 1080);

        // Enter fullscreen -> bounds updates must be suppressed (None)
        let fs_snap = win_shell::FullscreenSnapshot {
            notification_state: coordinator::NotificationState::AcceptsNotifications,
            foreground: Some(borderless_game),
            primary_monitor: mon_rect,
        };
        let enter_act = apply_fullscreen_snapshot(&state, None, &fs_snap);
        assert_eq!(enter_act.taskbar_visible, Some(false));
        assert!(state.fullscreen_suppressed.load(Ordering::Acquire));
        assert_eq!(plan_taskbar_bounds_update(&state, mon, 1.0, 1.0), None);

        // Simulate resize_taskbar_dock and set_bar_mode mutations while suppressed
        *state.taskbar_content_width.lock().unwrap() = 840;
        assert_eq!(plan_taskbar_bounds_update(&state, mon, 1.0, 1.0), None);
        state.config.lock().unwrap().bar_mode = "edge".to_string();
        assert_eq!(plan_taskbar_bounds_update(&state, mon, 1.0, 1.0), None);

        // Exit fullscreen -> taskbar_visible == Some(true) and bounds reflect latest Edge mode (58px height)
        let desktop_snap = win_shell::FullscreenSnapshot {
            notification_state: coordinator::NotificationState::AcceptsNotifications,
            foreground: None,
            primary_monitor: mon_rect,
        };
        let exit_act = apply_fullscreen_snapshot(&state, None, &desktop_snap);
        assert_eq!(exit_act.taskbar_visible, Some(true));
        assert!(!state.fullscreen_suppressed.load(Ordering::Acquire));

        let restored_edge = plan_taskbar_bounds_update(&state, mon, 1.0, 1.0)
            .expect("bounds should apply on fullscreen exit");
        assert_eq!(restored_edge, PixelRect::new(0, 1080 - 58, 1920, 1080));

        // Switch back to dock -> reflects recorded logical width 840 (+ 32 shadow margin = 872, 83px height)
        state.config.lock().unwrap().bar_mode = "dock".to_string();
        let restored_dock = plan_taskbar_bounds_update(&state, mon, 1.0, 1.0)
            .expect("bounds should apply in dock mode");
        assert_eq!(restored_dock.width(), 872);
        assert_eq!(restored_dock.height(), 83);
    }

    #[test]
    fn abn_fullscreenapp_hint_reevaluates_via_single_authority_and_syncs_state() {
        let mon_rect = PixelRect::new(0, 0, 1920, 1080);
        let mon = win_shell::MonitorRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
            width: 1920,
            height: 1080,
            dpi: 96,
        };
        let maximized_app = coordinator::ForegroundWindowInfo {
            visible: true,
            is_own_process: false,
            class_name: "Chrome_WidgetWin_1".to_string(),
            style: coordinator::WS_CAPTION_FLAG | coordinator::WS_THICKFRAME_FLAG,
            rect: mon_rect,
        };
        let state = make_test_app_state("dock", 620);

        // ABN_FULLSCREENAPP(true) over maximized desktop app keeps taskbar visible & bounds active
        let max_snap = win_shell::FullscreenSnapshot {
            notification_state: coordinator::NotificationState::Busy,
            foreground: Some(maximized_app.clone()),
            primary_monitor: mon_rect,
        };
        let act_max = apply_fullscreen_snapshot(&state, Some(true), &max_snap);
        assert_eq!(act_max, CoordinatorActions::default());
        assert!(!state.fullscreen_suppressed.load(Ordering::Acquire));
        assert!(plan_taskbar_bounds_update(&state, mon, 1.0, 1.0).is_some());

        // ABN_FULLSCREENAPP(false) while D3D fullscreen is active still suppresses taskbar
        let d3d_snap = win_shell::FullscreenSnapshot {
            notification_state: coordinator::NotificationState::RunningD3dFullScreen,
            foreground: Some(maximized_app.clone()),
            primary_monitor: mon_rect,
        };
        let act_d3d = apply_fullscreen_snapshot(&state, Some(false), &d3d_snap);
        assert_eq!(act_d3d.taskbar_visible, Some(false));
        assert!(state.fullscreen_suppressed.load(Ordering::Acquire));
        assert_eq!(plan_taskbar_bounds_update(&state, mon, 1.0, 1.0), None);

        // ABN_FULLSCREENAPP(true) once D3D exits back to maximized app restores taskbar
        let act_restore = apply_fullscreen_snapshot(&state, Some(true), &max_snap);
        assert_eq!(act_restore.taskbar_visible, Some(true));
        assert!(!state.fullscreen_suppressed.load(Ordering::Acquire));
        assert!(plan_taskbar_bounds_update(&state, mon, 1.0, 1.0).is_some());
    }

    #[test]
    fn apply_taskbar_visibility_action_repositions_before_showing_and_skips_on_hide() {
        let trace = std::cell::RefCell::new(Vec::new());
        apply_taskbar_visibility_action(
            None,
            || trace.borrow_mut().push("reposition"),
            |_| trace.borrow_mut().push("visible"),
        );
        assert!(trace.borrow().is_empty());

        apply_taskbar_visibility_action(
            Some(false),
            || trace.borrow_mut().push("reposition"),
            |v| trace.borrow_mut().push(if v { "show" } else { "hide" }),
        );
        assert_eq!(*trace.borrow(), vec!["hide"]);

        trace.borrow_mut().clear();
        apply_taskbar_visibility_action(
            Some(true),
            || trace.borrow_mut().push("reposition"),
            |v| trace.borrow_mut().push(if v { "show" } else { "hide" }),
        );
        assert_eq!(*trace.borrow(), vec!["reposition", "show"]);
    }

    #[test]
    fn dpi_changed_during_fullscreen_persists_until_exit_and_bounds_guard_narrow_and_invalid_sf() {
        let mon_rect = PixelRect::new(0, 0, 2560, 1440);
        let mon = win_shell::MonitorRect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
            width: 2560,
            height: 1440,
            dpi: 96,
        };
        let state = make_test_app_state("dock", 620);

        // Enter fullscreen
        let fs_snap = win_shell::FullscreenSnapshot {
            notification_state: coordinator::NotificationState::RunningD3dFullScreen,
            foreground: None,
            primary_monitor: mon_rect,
        };
        assert_eq!(
            apply_fullscreen_snapshot(&state, None, &fs_snap).taskbar_visible,
            Some(false)
        );

        // DpiChanged(144 = 150%) arrives while fullscreen-suppressed: persisted but bounds stay suppressed
        state.set_dpi_override(144);
        assert_eq!(state.current_dpi_override(), Some(144));
        assert_eq!(plan_taskbar_bounds_update(&state, mon, 1.0, 1.0), None);

        // Exit fullscreen: bounds immediately use persisted 144 DPI (1.5x), not stale fallback 1.0x
        let desktop_snap = win_shell::FullscreenSnapshot {
            notification_state: coordinator::NotificationState::AcceptsNotifications,
            foreground: None,
            primary_monitor: mon_rect,
        };
        assert_eq!(
            apply_fullscreen_snapshot(&state, None, &desktop_snap).taskbar_visible,
            Some(true)
        );
        let restored = plan_taskbar_bounds_update(&state, mon, 1.0, 1.0)
            .expect("bounds should apply on fullscreen exit");
        // (620 + 32) * 1.5 = 978, height = 83 * 1.5 = 124.5 -> 125
        assert_eq!(restored.width(), 978);
        assert_eq!(restored.height(), 125);

        // DisplayChanged clears dpi_override so fallback scale_factor applies again
        state.clear_dpi_override();
        assert_eq!(state.current_dpi_override(), None);
        let after_display_change = plan_taskbar_bounds_update(&state, mon, 1.25, 1.0).unwrap();
        // (620 + 32) * 1.25 = 815, height = 83 * 1.25 = 103.75 -> 104
        assert_eq!(after_display_change.width(), 815);
        assert_eq!(after_display_change.height(), 104);

        // Narrow monitors (< 16px) and non-finite/non-positive scale factors never invert bounds
        let narrow = win_shell::MonitorRect {
            left: 0,
            top: 0,
            right: 10,
            bottom: 40,
            width: 10,
            height: 40,
            dpi: 96,
        };
        for bad_sf in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -2.0] {
            let dock = compute_taskbar_bounds(narrow, bad_sf, "dock", 620);
            assert!(dock.width() >= 0 && dock.right >= dock.left);
            assert!(dock.height() >= 0 && dock.bottom >= dock.top);
            assert_eq!(dock.width(), 0);
            assert_eq!(dock.height(), 40);

            let edge = compute_taskbar_bounds(narrow, bad_sf, "edge", 620);
            assert_eq!(edge.width(), 10);
            assert_eq!(edge.height(), 40);
        }

        // Valid HiDPI 200% (sf = 2.0) on normal monitor -> 58 * 2.0 = 116px Edge height
        let hidpi_edge = compute_taskbar_bounds(mon, 2.0, "edge", 620);
        assert_eq!(hidpi_edge, PixelRect::new(0, 1440 - 116, 2560, 1440));
    }

    #[test]
    fn effective_scale_with_text_scale_factor_and_start_dock_bounds() {
        // Overlay scale (Start & Flyout) preserves TextScaleFactor: 96 DPI (1.0) * 140% = 1.4
        let eff_140 = effective_taskbar_scale_factor(Some(96), 1.0, 1.4);
        assert!((eff_140 - 1.4).abs() < 1e-9);
        assert!((effective_taskbar_scale_factor(None, 1.0, 1.4) - 1.4).abs() < 1e-9);
        assert!((effective_taskbar_scale_factor(Some(144), 1.0, 1.4) - 2.1).abs() < 1e-9);
        // Clamping out-of-range text scale factors
        assert!((effective_taskbar_scale_factor(Some(96), 1.0, 0.5) - 1.0).abs() < 1e-9);
        assert!((effective_taskbar_scale_factor(Some(96), 1.0, 4.0) - 2.25).abs() < 1e-9);
        assert!((effective_taskbar_scale_factor(Some(96), 1.0, f64::NAN) - 1.0).abs() < 1e-9);

        let mon_1440 = win_shell::MonitorRect {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
            width: 2560,
            height: 1440,
            dpi: 96,
        };
        let state = make_test_app_state("dock", 620);
        // Taskbar neutralizes TextScaleFactor=1.4 -> stays pure DPI 1.0x: (620 + 32) = 652, height = 83
        let dock_140 = plan_taskbar_bounds_update(&state, mon_1440, 1.0, 1.4).unwrap();
        assert_eq!(dock_140.width(), 652);
        assert_eq!(dock_140.height(), 83);

        // Start bounds at 1.0x on 1920x1080 -> full 1188x728 logical (1140x680 + 24px shadow pad on all 4 sides)
        let mon_1080 = win_shell::MonitorRect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
            width: 1920,
            height: 1080,
            dpi: 96,
        };
        let start_100 = compute_start_bounds(mon_1080, 1.0);
        assert_eq!(start_100.width(), 1188);
        assert_eq!(start_100.height(), 728);
        assert!(start_100.left >= mon_1080.left && start_100.right <= mon_1080.right);
        assert!(start_100.top >= mon_1080.top && start_100.bottom <= mon_1080.bottom);

        // Start bounds at 1.4x on 2560x1440 -> full 1188x728 scaled cleanly (Start keeps TextScaleFactor!)
        let start_1440_140 = compute_start_bounds(mon_1440, 1.4);
        assert_eq!(start_1440_140.width(), (1188.0_f64 * 1.4).round() as i32);
        assert_eq!(start_1440_140.height(), (728.0_f64 * 1.4).round() as i32);
        assert!(start_1440_140.top >= mon_1440.top + 8);

        // Start bounds at 1.4x on 1920x1080 -> clamped cleanly inside monitor bounds
        let start_1080_140 = compute_start_bounds(mon_1080, 1.4);
        assert!(start_1080_140.left >= mon_1080.left + 8);
        assert!(start_1080_140.right <= mon_1080.right - 8);
        assert!(start_1080_140.top >= mon_1080.top + 8);
        assert!(start_1080_140.bottom <= mon_1080.bottom);
    }

    #[test]
    fn windows_diff_fingerprint_skips_identical_and_targets_only_visible_windows() {
        let make_sample =
            |title: &str, active: bool, minimized: bool, icon_key: Option<&str>, monitor: &str| {
                vec![AppWindowGroup {
                    key: "c:\\apps\\zen.exe".to_string(),
                    name: "Zen Browser".to_string(),
                    exe_path: r"C:\Apps\zen.exe".to_string(),
                    aumid: None,
                    icon_key: icon_key.map(str::to_string),
                    icon_data: None,
                    is_active: active,
                    windows: vec![windows_tracker::OpenWindowInfo {
                        hwnd: 1001,
                        pid: 42,
                        title: title.to_string(),
                        exe_path: r"C:\Apps\zen.exe".to_string(),
                        exe_name: "Zen Browser".to_string(),
                        aumid: None,
                        is_active: active,
                        is_minimized: minimized,
                        is_topmost: false,
                        monitor: monitor.to_string(),
                    }],
                }]
            };

        let mut coord = WindowCoordinator::new();
        let mut last_fp: Option<WindowsFingerprint> = None;

        // First poll always emits -> only taskbar is visible
        let g1 = make_sample("Tab 1", true, false, Some("exe:aaa"), "primary");
        assert_eq!(
            diff_window_update_targets(&mut last_fp, &g1, &coord),
            Some(vec!["taskbar"])
        );

        // Identical poll -> NO emit (None)
        assert_eq!(diff_window_update_targets(&mut last_fp, &g1, &coord), None);

        // Open Start -> title change emits to taskbar + start (not flyout)
        let _ = coord.request_toggle_start(Some(true), 100);
        let g2 = make_sample("Tab 2", true, false, Some("exe:aaa"), "primary");
        assert_eq!(
            diff_window_update_targets(&mut last_fp, &g2, &coord),
            Some(vec!["taskbar", "start"])
        );
        assert_eq!(diff_window_update_targets(&mut last_fp, &g2, &coord), None);

        // Open Sound flyout (closes Start) -> minimize change emits to taskbar + flyout
        let _ = coord.request_open_flyout(FlyoutKind::Sound, 200);
        let g3 = make_sample("Tab 2", false, true, Some("exe:aaa"), "primary");
        assert_eq!(
            diff_window_update_targets(&mut last_fp, &g3, &coord),
            Some(vec!["taskbar", "flyout"])
        );

        // Icon key change also triggers diff
        let g4 = make_sample("Tab 2", false, true, Some("exe:bbb"), "primary");
        assert_eq!(
            diff_window_update_targets(&mut last_fp, &g4, &coord),
            Some(vec!["taskbar", "flyout"])
        );

        // Secondary taskbar receives windows-updated ONLY when visible
        let g5 = make_sample("Tab 3", false, false, Some("exe:bbb"), "primary");
        assert_eq!(
            diff_window_update_targets_with_secondary(&mut last_fp, &g5, &coord, true),
            Some(vec!["taskbar", "taskbar-secondary", "flyout"])
        );
        // Identical poll skips broadcast even with secondary visible
        assert_eq!(
            diff_window_update_targets_with_secondary(&mut last_fp, &g5, &coord, true),
            None
        );

        // Dragging a window between monitors ("primary" -> "secondary") MUST invalidate fingerprint!
        let g6 = make_sample("Tab 3", false, false, Some("exe:bbb"), "secondary");
        assert_eq!(
            diff_window_update_targets_with_secondary(&mut last_fp, &g6, &coord, true),
            Some(vec!["taskbar", "taskbar-secondary", "flyout"])
        );
    }

    #[test]
    fn compute_secondary_taskbar_bounds_handles_vertical_display2_scaling_and_clamping() {
        // DISPLAY2 vertical 1440x2560 placed right of 4K primary (3840,0 -> 5280,2560), 96 DPI (TextScaleFactor=1.4 neutralized!)
        let display2 = win_shell::MonitorRect {
            left: 3840,
            top: 0,
            right: 5280,
            bottom: 2560,
            width: 1440,
            height: 2560,
            dpi: 96,
        };

        // 1. Compact Option A secondary dock width 160 (+ 32 padding = 192px at 1.0x DPI-only, 83px height)
        let dock_compact = compute_secondary_taskbar_bounds(display2, 1.4, "dock", 160);
        assert_eq!(dock_compact.width(), 192);
        assert_eq!(dock_compact.height(), 83);
        assert_eq!(dock_compact.left, 3840 + (1440 - 192) / 2);
        assert_eq!(dock_compact.right, dock_compact.left + 192);
        assert_eq!(dock_compact.bottom, 2560);
        assert_eq!(dock_compact.top, 2560 - 83);

        // 2. Wide Dock (1600 logical -> 1632px) clamps cleanly to mon.width - 16 = 1424px
        let dock_clamped = compute_secondary_taskbar_bounds(display2, 1.4, "dock", 1600);
        assert_eq!(dock_clamped.width(), 1424);
        assert_eq!(dock_clamped.left, 3840 + 8);
        assert_eq!(dock_clamped.right, 5280 - 8);
        assert_eq!(dock_clamped.bottom, 2560);

        // 3. Edge mode fills exact DISPLAY2 width and uses pure DPI 58px bar height (NOT 1.4x!)
        let edge_sec = compute_secondary_taskbar_bounds(display2, 1.4, "edge", 700);
        assert_eq!(edge_sec, PixelRect::new(3840, 2560 - 58, 5280, 2560));

        // 4. Secondary HiDPI monitor (144 DPI = 1.5x, TextScaleFactor=1.2 ignored!), left-of-primary negative coordinates
        let left_hidpi = win_shell::MonitorRect {
            left: -2560,
            top: 0,
            right: 0,
            bottom: 1440,
            width: 2560,
            height: 1440,
            dpi: 144,
        };
        let left_dock = compute_secondary_taskbar_bounds(left_hidpi, 1.2, "dock", 620);
        // (620 + 32) * 1.5 = 978; 83 * 1.5 = 124.5 -> 125
        assert_eq!(left_dock.width(), 978);
        assert_eq!(left_dock.height(), 125);
        assert!(left_dock.left >= -2560 + 8 && left_dock.right <= -8);
        assert_eq!(left_dock.bottom, 1440);
    }

    fn make_lib_temp_dir(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::AtomicU64;
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "testbar-lib-test-{}-{}-{}",
            tag,
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn transactional_config_mutation_bumps_revision_skips_noops_and_rolls_back_on_save_error() {
        let dir = make_lib_temp_dir("tx-mutate");
        let cfg_path = dir.join("config.json");
        let state = make_test_app_state("dock", 620);

        // 1. Valid mutation bumps revision 0 -> 1 and writes atomically
        let (updated, changed) = mutate_config_at(&state.config, &cfg_path, |cfg| {
            if cfg.bar_mode == "edge" {
                return false;
            }
            cfg.bar_mode = "edge".to_string();
            true
        })
        .expect("valid mutation must succeed");
        assert!(changed);
        assert_eq!(updated.revision, 1);
        assert_eq!(updated.bar_mode, "edge");
        assert!(cfg_path.exists());
        assert_eq!(config::load_config_from(&cfg_path).revision, 1);

        // 2. No-op mutation returns changed = false, keeps revision = 1, and skips disk write
        std::fs::remove_file(&cfg_path).expect("remove file to prove no-op skips write");
        let (same, changed_noop) = mutate_config_at(&state.config, &cfg_path, |cfg| {
            if cfg.bar_mode == "edge" {
                return false;
            }
            cfg.bar_mode = "edge".to_string();
            true
        })
        .expect("no-op mutation must succeed");
        assert!(!changed_noop);
        assert_eq!(same.revision, 1);
        assert!(!cfg_path.exists(), "no-op mutation must skip disk write");

        // 3. Failed disk write (target is a directory) rolls back in-memory state and revision
        let dir_target = dir.join("existing-dir");
        std::fs::create_dir_all(&dir_target).expect("create dir target");
        let err = mutate_config_at(&state.config, &dir_target, |cfg| {
            cfg.bar_mode = "dock".to_string();
            cfg.autostart = true;
            true
        });
        assert!(err.is_err(), "writing over a directory must return Err");

        let in_mem = state.config.lock().unwrap().clone();
        assert_eq!(in_mem.revision, 1, "revision must not advance on failed save");
        assert_eq!(in_mem.bar_mode, "edge", "bar_mode must roll back on failed save");
        assert!(!in_mem.autostart, "autostart must roll back on failed save");

        let tmp_count = std::fs::read_dir(&dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(tmp_count, 0, "no .tmp files may remain after failure");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_v0_migration_and_command_mutations_persist_monotonically() {
        let dir = make_lib_temp_dir("v0-e2e");
        let cfg_path = dir.join("config.json");
        let v0_json = r#"{
          "barMode": "dock",
          "autostart": false,
          "taskbarPins": [
            {
              "id": "p-term",
              "kind": "app",
              "name": "Terminal",
              "subtitle": "Shell",
              "target": "C:\\Windows\\System32\\cmd.exe",
              "altAction": "",
              "accentRgb": "52,211,153",
              "iconKey": "warp"
            }
          ],
          "personalPins": [],
          "personalLinks": [],
          "workFolders": [],
          "workApps": [],
          "studioPins": [],
          "studioTools": [],
          "gameLaunchers": [],
          "gameCombos": [
            {
              "id": "g-1",
              "name": "Game Combo",
              "subtitle": "Steam",
              "targets": ["steam://rungameid/730"],
              "accentRgb": "251,113,133"
            }
          ],
          "customSpaces": []
        }"#;
        std::fs::write(&cfg_path, v0_json).expect("write v0");

        let loaded = config::load_config_from(&cfg_path);
        assert_eq!(loaded.schema_version, config::CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.revision, 0);
        assert_eq!(loaded.taskbar_pins[0].title, "Terminal");
        assert_eq!(loaded.taskbar_pins[0].match_exe.as_deref(), Some("cmd.exe"));
        assert_eq!(loaded.game_combos[0].kind, "combo");

        let cfg_mutex = Mutex::new(loaded);

        // Add clipboard pin -> revision 1
        let (c1, ch1) = mutate_config_at(&cfg_mutex, &cfg_path, |cfg| {
            config::set_pinned_clipboard(cfg, "pinned-snippet", true)
        })
        .expect("pin clip");
        assert!(ch1);
        assert_eq!(c1.revision, 1);
        assert_eq!(c1.pinned_clipboard, vec!["pinned-snippet"]);

        // Duplicate clipboard pin -> no-op, revision stays 1
        let (c2, ch2) = mutate_config_at(&cfg_mutex, &cfg_path, |cfg| {
            config::set_pinned_clipboard(cfg, "pinned-snippet", true)
        })
        .expect("dup clip");
        assert!(!ch2);
        assert_eq!(c2.revision, 1);

        // Add pin to studio_tools -> revision 2
        let (c3, ch3) = mutate_config_at(&cfg_mutex, &cfg_path, |cfg| {
            config::add_pin_to_space(
                cfg,
                "studio_tools",
                PinItem {
                    id: "tool-1".to_string(),
                    kind: "app".to_string(),
                    title: "7-Zip".to_string(),
                    subtitle: "Archive".to_string(),
                    target: r"C:\Program Files\7-Zip\7zFM.exe".to_string(),
                    args: None,
                    alt_action: String::new(),
                    alt_label: None,
                    accent_rgb: "56,189,248".to_string(),
                    icon_key: "disk".to_string(),
                    icon_data: None,
                    match_exe: None,
                    match_aumid: None,
                    combo_targets: None,
                    custom_icon: None,
                    custom_color: None,
                    tile_size: None,
                    section: None,
                    run_as_admin: false,
                },
            )
        })
        .expect("add studio tool");
        assert!(ch3);
        assert_eq!(c3.revision, 2);
        assert_eq!(
            c3.studio_tools[0].match_exe.as_deref(),
            Some("7zfm.exe")
        );

        let disk_cfg = config::load_config_from(&cfg_path);
        assert_eq!(disk_cfg.revision, 2);
        assert_eq!(disk_cfg.pinned_clipboard, vec!["pinned-snippet"]);
        assert_eq!(disk_cfg.studio_tools.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mutate_config_with_emitter_emits_only_on_real_change_and_rejects_revision_overflow() {
        let dir = make_lib_temp_dir("tx-emitter");
        let cfg_path = dir.join("config.json");
        let state = make_test_app_state("dock", 620);

        // 1. Real change emits exactly once with updated revision
        let mut emitted = Vec::new();
        let (c1, ch1) = mutate_config_with_emitter(
            &state.config,
            &cfg_path,
            |cfg| {
                cfg.bar_mode = "edge".to_string();
                true
            },
            |updated| emitted.push(updated.revision),
        )
        .expect("real change");
        assert!(ch1);
        assert_eq!(c1.revision, 1);
        assert_eq!(emitted, vec![1]);

        // 2. No-op emits zero times
        emitted.clear();
        let (_, ch2) = mutate_config_with_emitter(
            &state.config,
            &cfg_path,
            |_| false,
            |updated| emitted.push(updated.revision),
        )
        .expect("no-op");
        assert!(!ch2);
        assert!(emitted.is_empty());

        // 3. Save error emits zero times and rolls back
        emitted.clear();
        let dir_target = dir.join("bad-dir");
        std::fs::create_dir_all(&dir_target).expect("create dir");
        let err = mutate_config_with_emitter(
            &state.config,
            &dir_target,
            |cfg| {
                cfg.bar_mode = "dock".to_string();
                true
            },
            |updated| emitted.push(updated.revision),
        );
        assert!(err.is_err());
        assert!(emitted.is_empty());
        assert_eq!(state.config.lock().unwrap().bar_mode, "edge");

        // 4. u64::MAX revision overflow returns Err and emits zero times
        state.config.lock().unwrap().revision = u64::MAX;
        emitted.clear();
        let overflow = mutate_config_with_emitter(
            &state.config,
            &cfg_path,
            |cfg| {
                cfg.bar_mode = "dock".to_string();
                true
            },
            |updated| emitted.push(updated.revision),
        );
        assert!(overflow.is_err());
        assert!(emitted.is_empty());
        assert_eq!(state.config.lock().unwrap().revision, u64::MAX);
        assert_eq!(state.config.lock().unwrap().bar_mode, "edge");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_autostart_transaction_rolls_back_registry_on_save_failure() {
        let dir = make_lib_temp_dir("autostart-tx");
        let cfg_path = dir.join("config.json");
        let state = make_test_app_state("dock", 620);

        // 1. Successful enable calls registry(true), bumps revision, persists config
        let mut reg_calls = Vec::new();
        let (c1, ch1) = apply_autostart_transaction(&state.config, &cfg_path, true, |en| {
            reg_calls.push(en);
            Ok(())
        })
        .expect("enable autostart");
        assert!(ch1);
        assert!(c1.autostart);
        assert_eq!(c1.revision, 1);
        assert_eq!(reg_calls, vec![true]);

        // 2. No-op enable syncs registry but skips revision bump
        reg_calls.clear();
        let (c2, ch2) = apply_autostart_transaction(&state.config, &cfg_path, true, |en| {
            reg_calls.push(en);
            Ok(())
        })
        .expect("noop autostart");
        assert!(!ch2);
        assert_eq!(c2.revision, 1);
        assert_eq!(reg_calls, vec![true]);

        // 3. Disk failure triggers compensating registry(prev_enabled = true) rollback
        let dir_target = dir.join("dir-fail");
        std::fs::create_dir_all(&dir_target).expect("create dir");
        reg_calls.clear();
        let err = apply_autostart_transaction(&state.config, &dir_target, false, |en| {
            reg_calls.push(en);
            Ok(())
        });
        assert!(err.is_err(), "save over dir must fail");
        assert_eq!(
            reg_calls,
            vec![false, true],
            "failed save must issue compensating rollback call with previous state"
        );
        let in_mem = state.config.lock().unwrap().clone();
        assert!(in_mem.autostart, "in-memory autostart must remain unchanged");
        assert_eq!(in_mem.revision, 1, "revision must remain unchanged");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_full_document_save_config_ipc_command_is_exposed() {
        let src = include_str!("lib.rs");

        // 1. Parse all handler identifiers registered in tauri::generate_handler![...]
        let marker = "tauri::generate_handler![";
        let handler_start = src.find(marker).expect("generate_handler! must exist") + marker.len();
        let handler_tail = &src[handler_start..];
        let handler_end = handler_tail.find("])").expect("end of generate_handler!");
        let handler_block = &handler_tail[..handler_end];

        let handlers: Vec<String> = handler_block
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .flat_map(|line| line.split(','))
            .map(|tok| tok.trim())
            .filter(|tok| !tok.is_empty())
            .map(|tok| tok.rsplit("::").next().unwrap_or(tok).trim().to_string())
            .collect();

        assert!(!handlers.is_empty(), "generate_handler! must list commands");
        assert!(
            !handlers.iter().any(|h| h == "save_config"),
            "generate_handler! must not register a save_config command: {handlers:?}"
        );

        // 2. Inspect every #[tauri::command] parameter list to ensure none accepts TestBarConfig
        let mut cursor = 0usize;
        let mut checked_commands = 0usize;
        while let Some(rel) = src[cursor..].find("#[tauri::command]") {
            let attr_pos = cursor + rel;
            let line_start = src[..attr_pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let line_end = src[attr_pos..]
                .find('\n')
                .map(|i| attr_pos + i)
                .unwrap_or(src.len());
            cursor = line_end;
            if src[line_start..line_end].trim() != "#[tauri::command]" {
                continue;
            }
            let after_attr = &src[line_end..];
            let Some(fn_rel) = after_attr.find("fn ") else {
                continue;
            };
            let fn_slice = &after_attr[fn_rel..];
            let Some(open_paren) = fn_slice.find('(') else {
                continue;
            };
            let mut depth = 0usize;
            let mut close_paren = None;
            for (idx, ch) in fn_slice[open_paren..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close_paren = Some(open_paren + idx);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close_paren = close_paren.expect("command signature closing paren");
            let fn_name = fn_slice[3..open_paren].trim();
            let params = &fn_slice[open_paren + 1..close_paren];
            assert!(
                !params.contains("TestBarConfig"),
                "#[tauri::command] `{fn_name}` must not accept a full TestBarConfig parameter"
            );
            checked_commands += 1;
        }
        assert!(
            checked_commands >= 10,
            "expected to inspect all tauri commands, found {checked_commands}"
        );
    }

    #[test]
    fn audio_split_diff_gating_targets_only_visible_surfaces_and_skips_unchanged() {
        let mut coord = WindowCoordinator::new();
        let mut last_master: Option<AudioMasterState> = None;
        let mut last_mixer: Option<AudioState> = None;

        let mut full = AudioState {
            available: true,
            master_volume: 52,
            master_muted: false,
            mic_muted: false,
            master_peak: 0.0,
            mic_peak: 0.0,
            devices: vec![audio::AudioDeviceInfo {
                id: "d1".into(),
                name: "Speakers (Realtek)".into(),
                short_name: "Speakers".into(),
                is_default: true,
            }],
            input_devices: vec![],
            sessions: vec![],
        };
        let master1 = full.to_master_state();

        // 1. Idle desktop: first master poll targets taskbar only; mixer is NEVER emitted
        assert_eq!(
            diff_audio_master_targets(&mut last_master, &master1, &coord, false),
            Some(vec!["taskbar"])
        );
        assert_eq!(
            diff_audio_master_targets(&mut last_master, &master1, &coord, false),
            None,
            "identical master state must not re-emit"
        );
        assert_eq!(
            diff_audio_mixer_targets(&mut last_mixer, &full, &coord),
            None,
            "mixer must never emit while Start and Quick Sound are hidden"
        );

        // 2. Open Tray flyout: still no mixer emission, master skips hidden flyout
        let _ = coord.request_open_flyout(FlyoutKind::Tray, 100);
        full.master_volume = 56;
        let master2 = full.to_master_state();
        assert_eq!(
            diff_audio_master_targets(&mut last_master, &master2, &coord, true),
            Some(vec!["taskbar", "taskbar-secondary"])
        );
        assert_eq!(diff_audio_mixer_targets(&mut last_mixer, &full, &coord), None);

        // 3. Open Sound flyout: mixer emits to flyout only; identical poll skips
        let _ = coord.request_open_flyout(FlyoutKind::Sound, 200);
        assert_eq!(
            diff_audio_mixer_targets(&mut last_mixer, &full, &coord),
            Some(vec!["flyout"])
        );
        assert_eq!(
            diff_audio_mixer_targets(&mut last_mixer, &full, &coord),
            None
        );

        // 4. Master change while Sound open targets taskbar + secondary + flyout
        full.master_volume = 60;
        let master3 = full.to_master_state();
        assert_eq!(
            diff_audio_master_targets(&mut last_master, &master3, &coord, true),
            Some(vec!["taskbar", "taskbar-secondary", "flyout"])
        );

        // 5. Open Start (closes Sound): mixer emits to start only
        let _ = coord.request_toggle_start(Some(true), 300);
        assert_eq!(
            diff_audio_mixer_targets(&mut last_mixer, &full, &coord),
            Some(vec!["start"])
        );
        assert_eq!(
            diff_audio_mixer_targets(&mut last_mixer, &full, &coord),
            None
        );
    }

    #[test]
    fn custom_space_accent_sanitization_validates_hex_and_derives_rgb() {
        assert_eq!(
            sanitize_custom_space_accent(Some("#38bdf8"), Some("56, 189, 248")),
            ("#38BDF8".to_string(), "56,189,248".to_string())
        );
        assert_eq!(
            sanitize_custom_space_accent(Some("#34d399"), None),
            ("#34D399".to_string(), "52,211,153".to_string())
        );
        assert_eq!(
            sanitize_custom_space_accent(Some("not-a-color"), Some("999,0,0")),
            ("#A78BFA".to_string(), "167,139,250".to_string())
        );
        assert_eq!(
            sanitize_custom_space_accent(None, None),
            ("#A78BFA".to_string(), "167,139,250".to_string())
        );
    }

    #[test]
    fn hidden_window_slow_poller_suppression_gates_media_telemetry_peripherals_and_clipboard() {
        let mut coord = WindowCoordinator::new();

        // 1. Start hidden (idle desktop): ZERO media/telemetry/peripherals/clipboard IPC
        assert!(!should_poll_start_widgets(&coord));
        assert!(!should_emit_start_clipboard_update(&coord));
        assert_eq!(
            plan_slow_poll_tick(&coord, 0, true),
            SlowPollPlan {
                poll_media: false,
                poll_telemetry: false,
                poll_peripherals: false,
                enforce_hide_native: true,
            }
        );

        // 2. Flyout open (e.g. Sound) while Start stays hidden: still suppresses Start widgets!
        let _ = coord.request_open_flyout(FlyoutKind::Sound, 100);
        assert!(!should_poll_start_widgets(&coord));
        assert!(!should_emit_start_clipboard_update(&coord));
        assert_eq!(
            plan_slow_poll_tick(&coord, 0, false),
            SlowPollPlan {
                poll_media: false,
                poll_telemetry: false,
                poll_peripherals: false,
                enforce_hide_native: false,
            }
        );

        // 3. Start visible: tick 0 polls all three, tick 1 polls media only, tick 2 polls media + telemetry
        let _ = coord.request_toggle_start(Some(true), 200);
        assert!(should_poll_start_widgets(&coord));
        assert!(should_emit_start_clipboard_update(&coord));
        assert_eq!(
            plan_slow_poll_tick(&coord, 0, true),
            SlowPollPlan {
                poll_media: true,
                poll_telemetry: true,
                poll_peripherals: true,
                enforce_hide_native: true,
            }
        );
        assert_eq!(
            plan_slow_poll_tick(&coord, 1, true),
            SlowPollPlan {
                poll_media: true,
                poll_telemetry: false,
                poll_peripherals: false,
                enforce_hide_native: false,
            }
        );
        assert_eq!(
            plan_slow_poll_tick(&coord, 2, true),
            SlowPollPlan {
                poll_media: true,
                poll_telemetry: true,
                poll_peripherals: false,
                enforce_hide_native: false,
            }
        );

        // 4. Closing Start immediately suppresses Start slow-poller widgets again
        let _ = coord.request_toggle_start(Some(false), 300);
        assert!(!should_poll_start_widgets(&coord));
        assert!(!should_emit_start_clipboard_update(&coord));
    }

    #[test]
    fn media_action_ipc_contains_zero_thread_sleep() {
        let src = include_str!("lib.rs");
        let start = src
            .find("fn media_action(")
            .expect("media_action command must exist");
        let tail = &src[start..];
        let end = tail.find("\n#[tauri::command]").unwrap_or(tail.len());
        let body = &tail[..end];
        assert!(
            !body.contains("sleep"),
            "media_action IPC must never call thread::sleep: {body}"
        );
    }

    #[test]
    fn combo_target_parser_supports_uris_quoted_unquoted_args_and_discord_squirrel() {
        let steam = windows_tracker::parse_combo_target("steam://rungameid/730").unwrap();
        assert_eq!(steam.target, "steam://rungameid/730");
        assert_eq!(steam.args, None);

        let quoted = windows_tracker::parse_combo_target(
            r#""C:\Users\Alex\AppData\Local\Discord\Update.exe" --processStart Discord.exe"#,
        )
        .unwrap();
        assert_eq!(
            quoted.target,
            r"C:\Users\Alex\AppData\Local\Discord\Update.exe"
        );
        assert_eq!(quoted.args.as_deref(), Some("--processStart Discord.exe"));

        let unquoted = windows_tracker::parse_combo_target(
            r"C:\Users\Alex Doe\AppData\Local\Discord\Update.exe --processStart Discord.exe",
        )
        .unwrap();
        assert_eq!(
            unquoted.target,
            r"C:\Users\Alex Doe\AppData\Local\Discord\Update.exe"
        );
        assert_eq!(unquoted.args.as_deref(), Some("--processStart Discord.exe"));

        // Legacy bare Discord\Update.exe auto-supplies --processStart Discord.exe
        let bare_discord = windows_tracker::parse_combo_target(
            r"C:\Users\Alex\AppData\Local\Discord\Update.exe",
        )
        .unwrap();
        assert_eq!(
            bare_discord.target,
            r"C:\Users\Alex\AppData\Local\Discord\Update.exe"
        );
        assert_eq!(
            bare_discord.args.as_deref(),
            Some("--processStart Discord.exe")
        );

        assert!(windows_tracker::parse_combo_target("   ").is_err());
    }

    #[test]
    fn paint_sync_anchor_origin_event_shapes_pid_hwnd_map_and_icon_sources_work_cleanly() {
        // 1. Anchor origin X clamps cleanly within [16, total_w - 16] logical px (16px flyout shadow pad)
        assert!((compute_flyout_anchor_origin_x(500, 400, 320.0, 1.0) - 100.0).abs() < 1e-6);
        assert!((compute_flyout_anchor_origin_x(405, 400, 320.0, 1.0) - 16.0).abs() < 1e-6);
        assert!((compute_flyout_anchor_origin_x(999, 400, 320.0, 1.0) - 304.0).abs() < 1e-6);
        assert!((compute_flyout_anchor_origin_x(700, 400, 320.0, 1.5) - 200.0).abs() < 1e-6);

        // 2. StartStateEvent, StartShownEvent, and FlyoutHideEvent serialize in camelCase
        let start_ev = StartStateEvent {
            open: true,
            gen: 7,
            monitor: Some("secondary".to_string()),
            immediate: false,
        };
        let start_json = serde_json::to_value(&start_ev).unwrap();
        assert_eq!(start_json["open"], true);
        assert_eq!(start_json["gen"], 7);
        assert_eq!(start_json["monitor"], "secondary");
        assert!(start_json.get("immediate").is_none());

        let shown_ev = StartShownEvent {
            open: true,
            gen: 7,
            monitor: Some("primary".to_string()),
        };
        let shown_json = serde_json::to_value(&shown_ev).unwrap();
        assert_eq!(shown_json["open"], true);
        assert_eq!(shown_json["gen"], 7);
        assert_eq!(shown_json["monitor"], "primary");

        let hide_ev = FlyoutHideEvent { gen: 12 };
        assert_eq!(serde_json::to_value(&hide_ev).unwrap()["gen"], 12);

        // 3. build_pid_to_hwnd_map prefers active window for multi-window PIDs
        let groups = vec![AppWindowGroup {
            key: "code".into(),
            name: "VS Code".into(),
            exe_path: r"C:\Apps\Code.exe".into(),
            aumid: None,
            icon_key: Some("exe:123".into()),
            icon_data: None,
            is_active: true,
            windows: vec![
                windows_tracker::OpenWindowInfo {
                    hwnd: 100,
                    pid: 777,
                    title: "Old".into(),
                    is_active: false,
                    ..Default::default()
                },
                windows_tracker::OpenWindowInfo {
                    hwnd: 200,
                    pid: 777,
                    title: "Active".into(),
                    is_active: true,
                    ..Default::default()
                },
            ],
        }];
        let map = build_pid_to_hwnd_map(&groups);
        assert_eq!(map.get(&777), Some(&200));

        // 4. register_icon_sources stores valid pairs and skips blanks
        let state = make_test_app_state("dock", 620);
        state.register_icon_sources(vec![
            ("exe:abc".into(), r"C:\Apps\Code.exe".into()),
            ("".into(), r"C:\Bad.exe".into()),
            ("exe:empty".into(), "".into()),
        ]);
        let sources = state.icon_sources.lock().unwrap();
        assert_eq!(sources.get("exe:abc").map(String::as_str), Some(r"C:\Apps\Code.exe"));
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn normalize_session_volume_scalar_clamps_strictly_without_percent_heuristic() {
        assert!((normalize_session_volume_scalar(0.0) - 0.0).abs() < 1e-6);
        assert!((normalize_session_volume_scalar(0.01) - 0.01).abs() < 1e-6);
        assert_eq!((normalize_session_volume_scalar(0.01) * 100.0).round() as u32, 1);
        assert!((normalize_session_volume_scalar(0.5) - 0.5).abs() < 1e-6);
        assert!((normalize_session_volume_scalar(1.0) - 1.0).abs() < 1e-6);
        // No `> 1.0` heuristic: out-of-range values strictly clamp to 1.0, never rescale by /100
        assert!((normalize_session_volume_scalar(2.0) - 1.0).abs() < 1e-6);
        assert!((normalize_session_volume_scalar(50.0) - 1.0).abs() < 1e-6);
        assert!((normalize_session_volume_scalar(-0.25) - 0.0).abs() < 1e-6);
        assert!((normalize_session_volume_scalar(f32::NAN) - 0.0).abs() < 1e-6);
        assert!((normalize_session_volume_scalar(f32::INFINITY) - 0.0).abs() < 1e-6);
        assert!((normalize_session_volume_scalar(f32::NEG_INFINITY) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn pin_icon_fallback_matches_installed_apps_flyout_height_clamp_and_secondary_width_default() {
        // 1. Secondary taskbar Option A content width NEVER inherits primary's 7-pin width (defaults to compact 160px)
        let state = make_test_app_state("dock", 740);
        assert_eq!(
            resolve_secondary_taskbar_content_width(&state),
            SECONDARY_DOCK_DEFAULT_CONTENT_WIDTH_LOGICAL
        );
        *state.taskbar_content_width.lock().unwrap() = 510;
        assert_eq!(
            resolve_secondary_taskbar_content_width(&state),
            SECONDARY_DOCK_DEFAULT_CONTENT_WIDTH_LOGICAL
        );
        *state.secondary_taskbar_content_width.lock().unwrap() = Some(220);
        assert_eq!(resolve_secondary_taskbar_content_width(&state), 220);

        // 2. Flyout max height clamps cleanly in [240, 620] logical px
        assert!((compute_flyout_max_height_logical(1080, 1.0) - 620.0).abs() < 1e-6);
        assert!((compute_flyout_max_height_logical(1440, 1.4) - 620.0).abs() < 1e-6);
        assert!((compute_flyout_max_height_logical(768, 1.25) - 526.4).abs() < 1e-6);
        assert!((compute_flyout_max_height_logical(300, 1.0) - 240.0).abs() < 1e-6);

        // 3. Pin fallback icon matching against installed_apps covers Spark, Claude, Yandex Browser
        //    (without colliding with Yandex Music!), Discord, and Windows Terminal (wt.exe)
        let apps = vec![
            IndexedApp {
                id: "uwp-0".into(),
                name: "Windows Terminal".into(),
                category: "Разработка".into(),
                target: r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into(),
                aumid: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".into()),
                icon_key: Some("aumid:wt".into()),
                icon_data: None,
            },
            IndexedApp {
                id: "uwp-1".into(),
                name: "Spark".into(),
                category: "Общение".into(),
                target: r"shell:AppsFolder\Readdle.SparkDesktop_abc!App".into(),
                aumid: Some("Readdle.SparkDesktop_abc!App".into()),
                icon_key: Some("aumid:spark".into()),
                icon_data: None,
            },
            IndexedApp {
                id: "uwp-2".into(),
                name: "Claude".into(),
                category: "AI".into(),
                target: r"shell:AppsFolder\Claude_pzs8sxrjxfjjc!Claude".into(),
                aumid: Some("Claude_pzs8sxrjxfjjc!Claude".into()),
                icon_key: Some("aumid:claude".into()),
                icon_data: None,
            },
            IndexedApp {
                id: "app-0".into(),
                name: "Яндекс Браузер".into(),
                category: "Интернет".into(),
                target: r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Yandex.lnk".into(),
                aumid: None,
                icon_key: Some("file:yandex".into()),
                icon_data: None,
            },
            IndexedApp {
                id: "app-1".into(),
                name: "Яндекс Музыка".into(),
                category: "Медиа".into(),
                target: r"C:\Users\me\AppData\Local\Programs\YandexMusic\Яндекс Музыка.exe".into(),
                aumid: None,
                icon_key: Some("exe:ymusic".into()),
                icon_data: None,
            },
            IndexedApp {
                id: "app-2".into(),
                name: "Discord".into(),
                category: "Общение".into(),
                target: r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Discord Inc\Discord.lnk".into(),
                aumid: None,
                icon_key: Some("file:discord".into()),
                icon_data: None,
            },
        ];

        assert_eq!(
            find_matching_indexed_app_icon_source(
                r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\wt.exe",
                &[PinIconLookupHint {
                    title: "Терминал".into(),
                    icon_key: "warp".into(),
                    match_exe: Some("wt.exe".into()),
                    match_aumid: None,
                }],
                &apps,
            )
            .as_deref(),
            Some(r"shell:AppsFolder\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
        );

        assert_eq!(
            find_matching_indexed_app_icon_source(
                "https://claude.ai",
                &[PinIconLookupHint {
                    title: "Claude".into(),
                    icon_key: "claude".into(),
                    match_exe: None,
                    match_aumid: None,
                }],
                &apps,
            )
            .as_deref(),
            Some(r"shell:AppsFolder\Claude_pzs8sxrjxfjjc!Claude")
        );

        assert_eq!(
            find_matching_indexed_app_icon_source(
                "Spark",
                &[PinIconLookupHint {
                    title: "Spark Mail".into(),
                    icon_key: "spark".into(),
                    match_exe: None,
                    match_aumid: None,
                }],
                &apps,
            )
            .as_deref(),
            Some(r"shell:AppsFolder\Readdle.SparkDesktop_abc!App")
        );

        assert_eq!(
            find_matching_indexed_app_icon_source(
                "browser.exe",
                &[PinIconLookupHint {
                    title: "Яндекс".into(),
                    icon_key: "yandex".into(),
                    match_exe: Some("browser.exe".into()),
                    match_aumid: None,
                }],
                &apps,
            )
            .as_deref(),
            Some(r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Yandex.lnk")
        );

        assert_eq!(
            find_matching_indexed_app_icon_source(
                "https://music.yandex.ru",
                &[PinIconLookupHint {
                    title: "Я.Музыка".into(),
                    icon_key: "ymusic".into(),
                    match_exe: None,
                    match_aumid: None,
                }],
                &apps,
            )
            .as_deref(),
            Some(r"C:\Users\me\AppData\Local\Programs\YandexMusic\Яндекс Музыка.exe")
        );

        assert_eq!(
            find_matching_indexed_app_icon_source(
                r"C:\Users\me\AppData\Local\Discord\Update.exe",
                &[PinIconLookupHint {
                    title: "Discord".into(),
                    icon_key: "discord".into(),
                    match_exe: Some("discord.exe".into()),
                    match_aumid: None,
                }],
                &apps,
            )
            .as_deref(),
            Some(r"C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Discord Inc\Discord.lnk")
        );
    }

    #[test]
    fn wave2b_backdrop_insets_tray_height_payload_and_structural_audio_diff() {
        // 1. Path 1 DComp backdrop insets & radii match CSS contract
        assert_eq!(
            taskbar_backdrop_insets_and_radius("dock", 1.0),
            (BackdropInsets::new(16, 16, 16, 9), 18.0)
        );
        assert_eq!(
            taskbar_backdrop_insets_and_radius("edge", 1.5),
            (BackdropInsets::ZERO, 0.0)
        );
        let (start_insets, start_radius) = start_backdrop_insets_and_radius(1.4);
        assert_eq!(start_insets, BackdropInsets::uniform(34));
        assert!((start_radius - (26.0 * 1.4_f64) as f32).abs() < 1e-4);
        assert_eq!(
            flyout_backdrop_insets_and_radius(FlyoutKind::Tray, 1.0),
            (BackdropInsets::uniform(16), 16.0)
        );
        assert_eq!(
            flyout_backdrop_insets_and_radius(FlyoutKind::Calendar, 1.0),
            (BackdropInsets::uniform(16), 18.0)
        );
        assert_eq!(
            flyout_backdrop_insets_and_radius(FlyoutKind::Weather, 1.0),
            (BackdropInsets::uniform(16), 18.0)
        );
        assert_eq!(
            flyout_backdrop_insets_and_radius(FlyoutKind::Battery, 1.0),
            (BackdropInsets::uniform(16), 16.0)
        );
        assert_eq!(
            flyout_backdrop_insets_and_radius(FlyoutKind::Preview, 1.0),
            (BackdropInsets::uniform(16), 15.0)
        );
        assert_eq!(
            flyout_backdrop_insets_and_radius(FlyoutKind::Osd, 1.0),
            (BackdropInsets::uniform(16), 21.0)
        );

        // 2. Tray flyout initial height matches flyoutLogic.ts computeTrayFlyoutHeight
        assert_eq!(compute_tray_flyout_height_logical(0), 128);
        assert_eq!(compute_tray_flyout_height_logical(6), 128);
        assert_eq!(compute_tray_flyout_height_logical(12), 128);
        assert_eq!(compute_tray_flyout_height_logical(18), 160); // 3 rows = 110 + 50
        assert_eq!(compute_tray_flyout_height_logical(24), 198); // 4 rows = 148 + 50
        assert_eq!(compute_tray_flyout_height_logical(60), 274); // capped at 224 + 50

        // 3. embed_tray_apps_into_payload preserves existing object fields and replaces non-object payloads
        let sample_tray = vec![TrayAppItem {
            id: "steam".into(),
            hwnd: Some(1234),
            pid: 99,
            all_pids: vec![99],
            name: "Steam".into(),
            exe_path: r"C:\Program Files (x86)\Steam\steam.exe".into(),
            icon_key: Some("tray:abc".into()),
            icon_data: None,
        }];
        let mut obj_payload = serde_json::json!({ "source": "taskbar" });
        embed_tray_apps_into_payload(&mut obj_payload, &sample_tray);
        assert_eq!(obj_payload["source"], "taskbar");
        assert_eq!(obj_payload["trayApps"].as_array().unwrap().len(), 1);
        assert_eq!(obj_payload["trayApps"][0]["name"], "Steam");

        let mut null_payload = serde_json::Value::Null;
        embed_tray_apps_into_payload(&mut null_payload, &sample_tray);
        assert_eq!(null_payload["trayApps"].as_array().unwrap().len(), 1);

        // 4. diff_audio_mixer_targets ignores volatile peak & session is_active jitter (AudioState::structurally_eq)
        let mut coord = WindowCoordinator::new();
        let _ = coord.request_toggle_start(Some(true), 100);
        let mut last_mixer: Option<AudioState> = None;
        let mut aud = AudioState {
            available: true,
            master_volume: 65,
            master_muted: false,
            mic_muted: false,
            master_peak: 0.12,
            mic_peak: 0.04,
            devices: vec![],
            input_devices: vec![],
            sessions: vec![audio::AudioSessionInfo {
                id: "s1".into(),
                pid: 42,
                pids: vec![42],
                name: "Discord".into(),
                volume: 80,
                muted: false,
                is_active: true,
                peak: 0.25,
                icon_key: None,
            }],
        };
        assert_eq!(
            diff_audio_mixer_targets(&mut last_mixer, &aud, &coord),
            Some(vec!["start"])
        );
        // Volatile peak / is_active changes MUST NOT re-emit audio-mixer-updated!
        aud.master_peak = 0.89;
        aud.mic_peak = 0.55;
        aud.sessions[0].peak = 0.91;
        aud.sessions[0].is_active = false;
        assert_eq!(
            diff_audio_mixer_targets(&mut last_mixer, &aud, &coord),
            None
        );
        // Structural volume change MUST re-emit
        aud.sessions[0].volume = 72;
        assert_eq!(
            diff_audio_mixer_targets(&mut last_mixer, &aud, &coord),
            Some(vec!["start"])
        );

        // 5. claim_backdrop_enter_gen suppresses duplicate/in-place scrub & OSD animations
        let anim_gen = AtomicU64::new(5);
        let shown_gen = AtomicU64::new(u64::MAX);
        assert!(!claim_backdrop_enter_gen(&anim_gen, &shown_gen, 4, true)); // stale gen
        assert!(!claim_backdrop_enter_gen(&anim_gen, &shown_gen, 5, false)); // hidden
        assert!(claim_backdrop_enter_gen(&anim_gen, &shown_gen, 5, true)); // cold open -> claims once
        assert!(!claim_backdrop_enter_gen(&anim_gen, &shown_gen, 5, true)); // duplicate -> suppressed
        // In-place scrub/OSD already marks shown_gen == anim_gen -> must NOT re-animate
        anim_gen.store(6, Ordering::SeqCst);
        shown_gen.store(6, Ordering::SeqCst);
        assert!(!claim_backdrop_enter_gen(&anim_gen, &shown_gen, 6, true));

        // 6. Jump list cache key normalization & ContextApp payload embedding
        assert_eq!(
            jump_list_cache_key("  C:\\Program Files\\App.EXE ", Some("App.AUMID")),
            jump_list_cache_key("c:\\program files\\app.exe", Some(" app.aumid "))
        );
        let sample_jl = vec![JumpListItem {
            title: "New Window".into(),
            target: "C:\\app.exe".into(),
            args: Some("--new-window".into()),
            kind: "task".into(),
        }];
        let mut ctx_payload = serde_json::json!({ "target": "C:\\app.exe", "title": "App" });
        embed_jump_list_into_payload(&mut ctx_payload, &sample_jl);
        assert_eq!(ctx_payload["title"], "App");
        assert_eq!(ctx_payload["jumpList"].as_array().unwrap().len(), 1);
        assert_eq!(ctx_payload["jumpList"][0]["title"], "New Window");

        // 7. Wave 7 Start/Flyout dirty-gating hashes are deterministic & change-sensitive
        let clip_a = vec![ClipboardEntry {
            id: "c1".into(),
            tag: "TXT".into(),
            text: "hello".into(),
            color: None,
            pinned: false,
            preview: "hello".into(),
            line_count: 1,
            char_count: 5,
            kind: "text".into(),
        }];
        let mut clip_b = clip_a.clone();
        assert_eq!(hash_clipboard_entries(&clip_a), hash_clipboard_entries(&clip_b));
        clip_b[0].pinned = true;
        assert_ne!(hash_clipboard_entries(&clip_a), hash_clipboard_entries(&clip_b));

        let mut med_a = MediaState::default();
        med_a.has_session = true;
        med_a.title = "Track A".into();
        let mut med_b = med_a.clone();
        assert_eq!(hash_media_state(&med_a), hash_media_state(&med_b));
        med_b.title = "Track B".into();
        assert_ne!(hash_media_state(&med_a), hash_media_state(&med_b));
    }

    #[test]
    fn wave3a_patch_arg_pin_hints_spaces_and_custom_icon_loader() {
        assert_eq!(PatchArg::<String>::Unset.into_double_option(), None);
        assert_eq!(PatchArg::<String>::Null.into_double_option(), Some(None));
        assert_eq!(
            PatchArg::Value("terminal".to_string()).into_double_option(),
            Some(Some("terminal".to_string()))
        );
        assert_eq!(PatchArg::<f64>::Null.into_double_option(), Some(None));
        assert_eq!(
            PatchArg::Value(55.75).into_double_option(),
            Some(Some(55.75))
        );

        let mut cfg = TestBarConfig::default();
        cfg.spaces[0].items.push(PinItem {
            id: "sp-custom".into(),
            kind: "app".into(),
            title: "Custom App".into(),
            target: r"C:\Apps\Custom\custom.exe".into(),
            icon_key: "custom-k".into(),
            match_exe: Some("custom.exe".into()),
            ..Default::default()
        });
        let hints = collect_pin_hints_for_target(&cfg, r"C:\Apps\Custom\custom.exe");
        assert!(hints.iter().any(|h| h.icon_key == "custom-k"));

        // Verify load_custom_icon_image_data_uri returns valid data:image/png;base64,... for a tiny PNG
        let dir = make_lib_temp_dir("custom-icon");
        let png_path = dir.join("tiny.png");
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255])
                .unwrap();
        }
        std::fs::write(&png_path, &png_bytes).unwrap();
        let data_uri = load_custom_icon_image_data_uri(&png_path).expect("tiny PNG should load");
        assert!(data_uri.starts_with("data:image/png;base64,"));
        assert!(config::sanitize_custom_icon_data(&data_uri).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
}

#[cfg(target_os = "windows")]
pub use windows_desktop::{
    compute_secondary_taskbar_bounds, run, run_watchdog_mode, ActiveSurfaceMetrics, AppState,
    FlyoutHideEvent, FlyoutRequest, PatchArg, PendingSurfaceExit, StartSentSnapshot,
    StartShownEvent, StartStateEvent,
};
