pub mod apps_indexer;
pub mod audio;
pub mod config;
pub mod coordinator;
pub mod icon_cache;
pub mod media;
pub mod system_widgets;
pub mod telemetry;
pub mod win_shell;
pub mod windows_tracker;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use apps_indexer::{IndexedApp, IndexedFile, SteamGame};
use audio::{AudioMasterState, AudioPeaksSnapshot, AudioState};
use config::{CustomSpace, PinItem, TestBarConfig};
use coordinator::{
    BarMode, CoordinatorActions, FlyoutKind, ModalPickerGuard, PixelRect, WindowCoordinator,
};
use icon_cache::IconCache;
use media::MediaState;
use system_widgets::{ClipboardEntry, NetworkInfo, PeripheralBattery, SessionAlert, TrayAppItem};
use telemetry::TelemetryState;
use win_shell::{ShellAppBarEvent, ShellHookEvent, WatchdogArgs};
use windows_tracker::{AppWindowGroup, JumpListItem};

const DOCK_WINDOW_LOGICAL_HEIGHT: f64 = 84.0;
const DOCK_HORIZONTAL_PADDING_LOGICAL: i32 = 36; // 18px left + 18px right transparent shadow margin
const START_DEFAULT_LOGICAL_WIDTH: f64 = 1140.0 + 2.0 * win_shell::START_ROOT_PAD_LOGICAL; // 1188px (24px shadow pad)
const START_DEFAULT_LOGICAL_HEIGHT: f64 = 680.0 + 2.0 * win_shell::START_ROOT_PAD_LOGICAL; // 728px (24px shadow pad)
const FLYOUT_ROOT_PAD_TOTAL_LOGICAL: f64 = 2.0 * win_shell::FLYOUT_ROOT_PAD_LOGICAL; // 36px (18px shadow pad)
const PAINT_SYNC_SHOW_FAILSAFE_MS: u64 = 22;
const FLYOUT_EXIT_ANIM_MS: u64 = 135;
const START_EXIT_ANIM_MS: u64 = 145;
const PREVIEW_GLIDE_DURATION_MS: u64 = 115;
const PERIODIC_REINDEX_INTERVAL_SECS: u64 = 15 * 60;
const MAX_ICON_SOURCES_ENTRIES: usize = 2048;

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
pub struct FlyoutHideEvent {
    pub gen: u64,
}

#[derive(Debug, Clone)]
pub struct PendingSurfaceShow {
    pub gen: u64,
    pub rect: PixelRect,
    pub scale_factor: f64,
    pub bottom_anchor_y: i32,
    pub monitor_top: i32,
    pub activate: bool,
    pub anchor_x: f64,
    pub monitor: Option<String>,
    pub was_already_visible: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PendingSurfaceExit {
    pub gen: u64,
}

pub struct AppState {
    pub config: Mutex<TestBarConfig>,
    pub icon_cache: Mutex<IconCache>,
    pub icon_sources: Mutex<HashMap<String, String>>,
    pub last_window_groups: Mutex<Vec<AppWindowGroup>>,
    pub installed_apps: Mutex<Vec<IndexedApp>>,
    pub indexed_files: Mutex<Vec<IndexedFile>>,
    pub steam_games: Mutex<Vec<SteamGame>>,
    pub hide_native: bool,
    pub coordinator: Mutex<WindowCoordinator>,
    pub overlay_open: Arc<AtomicBool>,
    pub fullscreen_suppressed: Arc<AtomicBool>,
    pub secondary_fullscreen_suppressed: Arc<AtomicBool>,
    pub taskbar_content_width: Mutex<i32>,
    pub secondary_taskbar_content_width: Mutex<i32>,
    pub secondary_taskbar_visible: Arc<AtomicBool>,
    pub last_flyout_req: Mutex<Option<FlyoutRequest>>,
    pub last_flyout_bounds: Mutex<Option<(FlyoutKind, PixelRect)>>,
    pub pending_start_show: Mutex<Option<PendingSurfaceShow>>,
    pub pending_flyout_show: Mutex<Option<PendingSurfaceShow>>,
    pub pending_start_exit: Mutex<Option<PendingSurfaceExit>>,
    pub pending_flyout_exit: Mutex<Option<PendingSurfaceExit>>,
    pub flyout_anim_gen: Arc<AtomicU64>,
    pub start_anim_gen: Arc<AtomicU64>,
    pub dpi_override: AtomicU32,
    pub last_external_fg: Arc<AtomicIsize>,
    pub index_refresh_running: Arc<AtomicBool>,
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
    let mut cache = lock_unpoisoned(&state.icon_cache);
    let res = audio::get_audio_state_with_icons(&mut cache);
    drop(cache);
    match res {
        Ok((aud, sources)) => {
            state.register_icon_sources(sources);
            aud
        }
        Err(_) => audio::get_audio_state(),
    }
}

fn sample_telemetry_with_icon_sources(state: &AppState) -> TelemetryState {
    let pid_to_hwnd = {
        let groups = lock_unpoisoned(&state.last_window_groups);
        build_pid_to_hwnd_map(&groups)
    };
    let mut cache = lock_unpoisoned(&state.icon_cache);
    let (telem, sources) = telemetry::sample_telemetry_with_icons(&mut cache, &pid_to_hwnd);
    drop(cache);
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

fn scale_factor(app: &AppHandle) -> f64 {
    let dpi_override = app.state::<AppState>().current_dpi_override();
    let fallback_sf = app
        .get_webview_window("taskbar")
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or_else(|| win_shell::primary_monitor_dpi() as f64 / 96.0);
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
    if last.as_ref() == Some(current) {
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
    let per_mon = win_shell::detect_fullscreen_per_monitor();
    let actions = {
        let state = app.state::<AppState>();
        let mut coord = lock_unpoisoned(&state.coordinator);
        let act = coord.reevaluate_fullscreen_per_monitor(per_mon, monotonic_ms());
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

fn effective_taskbar_scale_factor(
    dpi_override: Option<u32>,
    fallback_sf: f64,
    text_scale: f64,
) -> f64 {
    let base_dpi_sf = if let Some(dpi) = dpi_override.filter(|&d| d > 0) {
        dpi as f64 / 96.0
    } else if fallback_sf.is_finite() && fallback_sf > 0.0 {
        fallback_sf
    } else {
        1.0
    };
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
        let h = ((52.0 * sf).round() as i32).clamp(0, mon_h);
        PixelRect::new(mon.left, mon.bottom - h, mon.left + mon_w, mon.bottom)
    } else {
        // Dock mode: unconstrained capsule width + 36px (18px left + 18px right) transparent shadow padding;
        // 84px logical height = 18px top + 52px bar + 12px bottom (+ 2px headroom).
        let desired_w =
            ((logical_content_w.clamp(380, 2400) + DOCK_HORIZONTAL_PADDING_LOGICAL) as f64 * sf)
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
/// own effective DPI multiplied by Windows Accessibility `TextScaleFactor`, clamped cleanly
/// within `mon.width - 16`.
pub fn compute_secondary_taskbar_bounds(
    mon: win_shell::MonitorRect,
    text_scale: f64,
    bar_mode: &str,
    logical_content_w: i32,
) -> PixelRect {
    let sf = coordinator::compute_effective_scale(mon.dpi, text_scale);
    compute_taskbar_bounds(mon, sf, bar_mode, logical_content_w)
}

fn compute_start_bounds(mon: win_shell::MonitorRect, sf: f64) -> PixelRect {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    let mon_w = mon.width.max(0);
    let mon_h = mon.height.max(0);
    let avail_w = ((mon_w as f64 / sf) - 16.0).max(320.0);
    let avail_h = ((mon_h as f64 / sf) - 80.0).max(320.0);
    let logical_w = avail_w.min(START_DEFAULT_LOGICAL_WIDTH);
    let logical_h = avail_h.min(START_DEFAULT_LOGICAL_HEIGHT);
    let max_w_px = (mon_w - 16).max(0);
    let max_h_px = (mon_h - 16).max(0);
    let w_px = ((logical_w * sf).round() as i32).clamp(0, max_w_px);
    let h_px = ((logical_h * sf).round() as i32).clamp(0, max_h_px);
    // Keep visible chassis at the exact 92px logical clearance above monitor bottom (68 + 24px shadow pad).
    let bar_gap = (68.0 * sf).round() as i32;
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
    logical_content_w: i32,
    dpi_override: Option<u32>,
}

fn read_taskbar_layout_if_allowed(state: &AppState) -> Option<TaskbarLayoutInput> {
    if !lock_unpoisoned(&state.coordinator).should_apply_taskbar_bounds() {
        return None;
    }
    let bar_mode = lock_unpoisoned(&state.config).bar_mode.clone();
    let logical_content_w = *lock_unpoisoned(&state.taskbar_content_width);
    Some(TaskbarLayoutInput {
        bar_mode,
        logical_content_w,
        dpi_override: state.current_dpi_override(),
    })
}

#[cfg(test)]
fn plan_taskbar_bounds_update(
    state: &AppState,
    mon: win_shell::MonitorRect,
    fallback_sf: f64,
    text_scale: f64,
) -> Option<PixelRect> {
    let input = read_taskbar_layout_if_allowed(state)?;
    let sf = effective_taskbar_scale_factor(input.dpi_override, fallback_sf, text_scale);
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
    let fallback_sf = tb.scale_factor().unwrap_or(1.0);
    let text_scale = win_shell::windows_text_scale_factor();
    let sf = effective_taskbar_scale_factor(input.dpi_override, fallback_sf, text_scale);
    let mon = win_shell::primary_monitor_rect();

    let rect = compute_taskbar_bounds(mon, sf, &input.bar_mode, input.logical_content_w);

    // Never force visibility here so fullscreen suppression cannot leak on bounds updates.
    apply_webview_bounds(&tb, hwnd, rect.left, rect.top, rect.width(), rect.height());
}

fn position_secondary_taskbar_window(app: &AppHandle) {
    let st = app.state::<AppState>();
    if !lock_unpoisoned(&st.coordinator).should_apply_secondary_taskbar_bounds() {
        return;
    }
    let (bar_mode, sec_mode) = {
        let cfg = lock_unpoisoned(&st.config);
        (cfg.bar_mode.clone(), cfg.secondary_taskbar_mode.clone())
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
    let logical_w = *lock_unpoisoned(&st.secondary_taskbar_content_width);
    let text_scale = win_shell::windows_text_scale_factor();
    let rect = compute_secondary_taskbar_bounds(sec_mon, text_scale, &bar_mode, logical_w);
    apply_webview_bounds(&tb2, hwnd, rect.left, rect.top, rect.width(), rect.height());
}

fn sync_secondary_taskbar(app: &AppHandle) {
    let st = app.state::<AppState>();
    let (cfg_snapshot, bar_mode, sec_mode) = {
        let cfg = lock_unpoisoned(&st.config);
        (
            cfg.clone(),
            cfg.bar_mode.clone(),
            cfg.secondary_taskbar_mode.clone(),
        )
    };
    win_shell::apply_secondary_taskbar_mode_to_native(&sec_mode, st.hide_native);

    let Some(tb2) = app.get_webview_window("taskbar-secondary") else {
        st.secondary_taskbar_visible.store(false, Ordering::Release);
        return;
    };
    let Some(hwnd) = win_hwnd(&tb2) else {
        st.secondary_taskbar_visible.store(false, Ordering::Release);
        return;
    };

    let sec_allowed = lock_unpoisoned(&st.coordinator).should_show_secondary_taskbar();
    let sec_mon = win_shell::secondary_monitor_rects().into_iter().next();
    if sec_mode == config::SECONDARY_TASKBAR_MODE_TESTBAR && sec_allowed {
        if let Some(mon) = sec_mon {
            let logical_w = *lock_unpoisoned(&st.secondary_taskbar_content_width);
            let text_scale = win_shell::windows_text_scale_factor();
            let rect = compute_secondary_taskbar_bounds(mon, text_scale, &bar_mode, logical_w);
            apply_webview_bounds(&tb2, hwnd, rect.left, rect.top, rect.width(), rect.height());
            let was_visible = st.secondary_taskbar_visible.swap(true, Ordering::AcqRel);
            win_shell::set_window_visible(hwnd, true, true);
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
                let layout = system_widgets::get_keyboard_layout_code();
                let _ = app.emit_to("taskbar-secondary", "config-updated", &cfg_snapshot);
                let _ = app.emit_to("taskbar-secondary", "windows-updated", &win_snap);
                let _ = app.emit_to("taskbar-secondary", "layout-updated", &layout);
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
                let h_sec = app.clone();
                std::thread::spawn(move || {
                    let master = audio::get_audio_master_state();
                    let _ = h_sec.emit_to("taskbar-secondary", "audio-master-updated", &master);
                    let net = system_widgets::get_network_info();
                    let _ = h_sec.emit_to("taskbar-secondary", "network-updated", &net);
                });
            }
            return;
        }
    }

    st.secondary_taskbar_visible.store(false, Ordering::Release);
    win_shell::set_window_visible(hwnd, false, true);
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
    (rect, sf, mon.top, resolved_monitor)
}

#[derive(Debug, Clone, Copy)]
struct ComputedFlyoutLayout {
    rect: PixelRect,
    scale_factor: f64,
    anchor_origin_x: f64,
    bottom_anchor_y: i32,
    monitor_top: i32,
}

fn compute_flyout_anchor_origin_x(
    anchor_screen_x_px: i32,
    win_left_px: i32,
    total_w_logical: f64,
    sf: f64,
) -> f64 {
    let sf = if sf.is_finite() && sf > 0.0 { sf } else { 1.0 };
    let rel = (anchor_screen_x_px - win_left_px) as f64 / sf;
    let pad = win_shell::FLYOUT_ROOT_PAD_LOGICAL;
    let max_x = (total_w_logical - pad).max(pad);
    rel.clamp(pad, max_x)
}

fn compute_flyout_layout(app: &AppHandle, req: &FlyoutRequest) -> ComputedFlyoutLayout {
    let use_secondary = req.monitor.as_deref() == Some("secondary");
    let (mon, sf, tb_label) = if use_secondary {
        if let Some(sec_mon) = win_shell::secondary_monitor_rects().into_iter().next() {
            let text_scale = win_shell::windows_text_scale_factor();
            let sec_sf = coordinator::compute_effective_scale(sec_mon.dpi, text_scale);
            (sec_mon, sec_sf, "taskbar-secondary")
        } else {
            (win_shell::primary_monitor_rect(), scale_factor(app), "taskbar")
        }
    } else {
        (win_shell::primary_monitor_rect(), scale_factor(app), "taskbar")
    };

    let anchor_screen_x_px = if !req.anchor_x_logical.is_finite() {
        mon.left + mon.width / 2
    } else if req.anchor_in_taskbar {
        if let Some(tb) = app.get_webview_window(tb_label) {
            let pos = tb.outer_position().map(|p| p.x).unwrap_or(mon.left);
            pos + (req.anchor_x_logical * sf).round() as i32
        } else {
            mon.left + mon.width / 2
        }
    } else {
        mon.left + (req.anchor_x_logical * sf).round() as i32
    };

    let req_w = req.width.clamp(1, 2000) as f64;
    let req_h = req.height.clamp(1, 2000) as f64;
    let total_w_logical = req_w + FLYOUT_ROOT_PAD_TOTAL_LOGICAL;
    let total_h_logical = req_h + FLYOUT_ROOT_PAD_TOTAL_LOGICAL;
    let w_px = (total_w_logical * sf).round() as i32;
    let h_px = (total_h_logical * sf).round() as i32;
    let is_edge =
        BarMode::from_config_str(&lock_unpoisoned(&app.state::<AppState>().config).bar_mode)
            == BarMode::Edge;
    // Account for `.flyout-root`'s 18px bottom shadow pad so Preview sits flush and click flyouts sit 10px above.
    let gap_logical = match (is_edge, req.kind) {
        (true, FlyoutKind::Preview) => 34.0,
        (true, _) => 44.0,
        (false, FlyoutKind::Preview) => 46.0,
        (false, _) => 56.0,
    };
    let bar_gap = (gap_logical * sf).round() as i32;

    let mut x_px = anchor_screen_x_px - w_px / 2;
    x_px = x_px.clamp(mon.left + 8, (mon.right - w_px - 8).max(mon.left + 8));
    let bottom_anchor_y = mon.bottom - bar_gap;
    let y_px = (bottom_anchor_y - h_px).max(mon.top + 8);
    let anchor_origin_x = compute_flyout_anchor_origin_x(anchor_screen_x_px, x_px, total_w_logical, sf);

    ComputedFlyoutLayout {
        rect: PixelRect::new(x_px, y_px, x_px + w_px, y_px + h_px),
        scale_factor: sf,
        anchor_origin_x,
        bottom_anchor_y,
        monitor_top: mon.top,
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

fn commit_pending_surface_show(
    app: &AppHandle,
    surface: &str,
    gen: u64,
    measured_height: Option<f64>,
) {
    let st = app.state::<AppState>();
    match surface {
        "start" => {
            let pending = {
                let mut slot = lock_unpoisoned(&st.pending_start_show);
                if slot.as_ref().is_some_and(|p| p.gen == gen) {
                    slot.take()
                } else {
                    None
                }
            };
            let Some(p) = pending else { return };
            if st.start_anim_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            if !lock_unpoisoned(&st.coordinator).is_start_visible() {
                return;
            }
            if let Some(win) = app.get_webview_window("start") {
                if let Some(hwnd) = win_hwnd(&win) {
                    let mut rect = p.rect;
                    if let Some(mh) = measured_height.filter(|h| h.is_finite() && *h >= 240.0) {
                        let new_h = ((mh.clamp(240.0, 960.0) + 2.0 * win_shell::START_ROOT_PAD_LOGICAL)
                            * p.scale_factor)
                            .round() as i32;
                        if (new_h - rect.height()).abs() > 2 {
                            let new_y = (p.bottom_anchor_y - new_h).max(p.monitor_top + 8);
                            rect = PixelRect::new(rect.left, new_y, rect.right, new_y + new_h);
                            apply_webview_bounds(&win, hwnd, rect.left, rect.top, rect.width(), rect.height());
                        }
                    } else if p.was_already_visible {
                        apply_webview_bounds(&win, hwnd, rect.left, rect.top, rect.width(), rect.height());
                    }
                    win_shell::set_window_hit_transparent(hwnd, false);
                    win_shell::set_window_visible(hwnd, true, !p.activate);
                    if p.activate {
                        let _ = win.set_focus();
                    }
                }
            }
        }
        "flyout" => {
            let pending = {
                let mut slot = lock_unpoisoned(&st.pending_flyout_show);
                if slot.as_ref().is_some_and(|p| p.gen == gen) {
                    slot.take()
                } else {
                    None
                }
            };
            let Some(p) = pending else { return };
            if st.flyout_anim_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            let active_kind = lock_unpoisoned(&st.coordinator).active_flyout();
            let Some(kind) = active_kind else { return };

            if let Some(fly) = app.get_webview_window("flyout") {
                if let Some(hwnd) = win_hwnd(&fly) {
                    let mut rect = p.rect;
                    let mut resized = false;
                    if let Some(mh) = measured_height.filter(|h| h.is_finite() && *h >= 36.0) {
                        let clamped_h = mh.clamp(48.0, 820.0);
                        let new_h = ((clamped_h + FLYOUT_ROOT_PAD_TOTAL_LOGICAL) * p.scale_factor)
                            .round() as i32;
                        if (new_h - rect.height()).abs() > 2 {
                            let new_y = (p.bottom_anchor_y - new_h).max(p.monitor_top + 8);
                            rect = PixelRect::new(rect.left, new_y, rect.right, new_y + new_h);
                            resized = true;
                            if let Some(req) = lock_unpoisoned(&st.last_flyout_req).as_mut() {
                                req.height = clamped_h.round() as i32;
                            }
                        }
                    }
                    if p.was_already_visible || resized {
                        apply_webview_bounds(&fly, hwnd, rect.left, rect.top, rect.width(), rect.height());
                    }
                    *lock_unpoisoned(&st.last_flyout_bounds) = Some((kind, rect));
                    win_shell::set_flyout_non_activating(!p.activate);
                    win_shell::set_window_hit_transparent(hwnd, false);
                    win_shell::set_window_visible(hwnd, true, !p.activate);
                    if p.activate {
                        let _ = fly.set_focus();
                    }
                }
            }
        }
        _ => {}
    }
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
            if let Some(win) = app.get_webview_window("start") {
                if let Some(hwnd) = win_hwnd(&win) {
                    win_shell::set_window_visible(hwnd, false, true);
                    win_shell::set_window_hit_transparent(hwnd, false);
                }
            }
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
            win_shell::set_flyout_non_activating(false);
            if let Some(fly) = app.get_webview_window("flyout") {
                if let Some(hwnd) = win_hwnd(&fly) {
                    win_shell::set_window_visible(hwnd, false, true);
                    win_shell::set_window_hit_transparent(hwnd, false);
                }
            }
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

fn apply_coordinator_actions(
    app: &AppHandle,
    actions: &CoordinatorActions,
    flyout_req: Option<&FlyoutRequest>,
) {
    let st = app.state::<AppState>();

    // 1. Hide / preempt outgoing surfaces first (with click-through exit or immediate preemption)
    let hide_flyout_now = actions.hide_flyout_immediate
        || (actions.flyout_visible == Some(false)
            && (actions.start_visible == Some(true) || actions.taskbar_visible == Some(false)));

    if hide_flyout_now {
        *lock_unpoisoned(&st.last_flyout_req) = None;
        *lock_unpoisoned(&st.last_flyout_bounds) = None;
        *lock_unpoisoned(&st.pending_flyout_show) = None;
        *lock_unpoisoned(&st.pending_flyout_exit) = None;
        win_shell::cancel_window_x_glide();
        let hide_gen = actions
            .flyout_anim_gen
            .unwrap_or_else(|| st.flyout_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.flyout_anim_gen.store(hide_gen, Ordering::SeqCst);
        let _ = app.emit("flyout-hide-immediate", FlyoutHideEvent { gen: hide_gen });
        win_shell::set_flyout_non_activating(false);
        if let Some(fly) = app.get_webview_window("flyout") {
            if let Some(hwnd) = win_hwnd(&fly) {
                win_shell::set_window_visible(hwnd, false, true);
                win_shell::set_window_hit_transparent(hwnd, false);
            }
        }
    } else if actions.flyout_visible == Some(false) {
        *lock_unpoisoned(&st.last_flyout_req) = None;
        *lock_unpoisoned(&st.last_flyout_bounds) = None;
        *lock_unpoisoned(&st.pending_flyout_show) = None;
        win_shell::cancel_window_x_glide();
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
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(FLYOUT_EXIT_ANIM_MS));
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
            && (actions.flyout_visible == Some(true) || actions.taskbar_visible == Some(false)));

    if hide_start_now {
        *lock_unpoisoned(&st.pending_start_show) = None;
        *lock_unpoisoned(&st.pending_start_exit) = None;
        let hide_gen = actions
            .start_anim_gen
            .unwrap_or_else(|| st.start_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.start_anim_gen.store(hide_gen, Ordering::SeqCst);
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
    } else if actions.start_visible == Some(false) {
        *lock_unpoisoned(&st.pending_start_show) = None;
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
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(START_EXIT_ANIM_MS));
            if anim_gen.load(Ordering::SeqCst) != hide_gen {
                return;
            }
            let h = h_hide.clone();
            let _ = h_hide.run_on_main_thread(move || {
                commit_pending_surface_exit(&h, "start", hide_gen);
            });
        });
    }

    // 2. Per-monitor taskbar visibility (fullscreen auto-hide / restore independently per display)
    apply_taskbar_visibility_action(
        actions.taskbar_visible,
        || position_taskbar_window(app),
        |tb_vis| {
            if let Some(tb) = app.get_webview_window("taskbar") {
                if let Some(hwnd) = win_hwnd(&tb) {
                    win_shell::set_window_visible(hwnd, tb_vis, true);
                }
            }
        },
    );

    if let Some(sec_vis) = actions.secondary_taskbar_visible {
        if sec_vis {
            sync_secondary_taskbar(app);
        } else {
            st.secondary_taskbar_visible.store(false, Ordering::Release);
            if let Some(tb2) = app.get_webview_window("taskbar-secondary") {
                if let Some(hwnd) = win_hwnd(&tb2) {
                    win_shell::set_window_visible(hwnd, false, true);
                }
            }
        }
    }

    // 3. Emit-Before-Show Paint-Sync for Start Menu
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
        let was_visible = app
            .get_webview_window("start")
            .and_then(|w| win_hwnd(&w))
            .is_some_and(|hwnd| win_shell::get_window_pixel_rect(hwnd).is_some());

        if let Some(win) = app.get_webview_window("start") {
            if let Some(hwnd) = win_hwnd(&win) {
                win_shell::set_window_hit_transparent(hwnd, false);
                if !was_visible {
                    apply_webview_bounds(&win, hwnd, rect.left, rect.top, rect.width(), rect.height());
                }
            }
        }

        *lock_unpoisoned(&st.pending_start_show) = Some(PendingSurfaceShow {
            gen: show_gen,
            rect,
            scale_factor: sf,
            bottom_anchor_y: rect.bottom,
            monitor_top: mon.top,
            activate: true,
            anchor_x: 0.0,
            monitor: resolved_monitor.clone(),
            was_already_visible: was_visible,
        });

        // Emit start-state BEFORE ShowWindow (Paint-Sync Protocol)
        let _ = app.emit(
            "start-state",
            StartStateEvent {
                open: true,
                gen: show_gen,
                monitor: resolved_monitor,
                immediate: false,
            },
        );

        // Hydrate Start immediately on hidden -> visible transition (AD-6)
        let cfg_snapshot = lock_unpoisoned(&st.config).clone();
        let win_snapshot = lock_unpoisoned(&st.last_window_groups).clone();
        let clips_snapshot = system_widgets::get_clipboard_history();
        let media_snapshot = media::get_media_snapshot();
        let _ = app.emit_to("start", "config-updated", &cfg_snapshot);
        let _ = app.emit_to("start", "windows-updated", &win_snapshot);
        let _ = app.emit_to("start", "clipboard-updated", &clips_snapshot);
        let _ = app.emit_to("start", "media-updated", &media_snapshot);

        // 22ms max Paint-Sync failsafe timer so opening NEVER stalls
        let h_fail = app.clone();
        let anim_gen = Arc::clone(&st.start_anim_gen);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(PAINT_SYNC_SHOW_FAILSAFE_MS));
            if anim_gen.load(Ordering::SeqCst) != show_gen {
                return;
            }
            let h = h_fail.clone();
            let _ = h_fail.run_on_main_thread(move || {
                commit_pending_surface_show(&h, "start", show_gen, None);
            });
        });

        let h_hyd = app.clone();
        std::thread::spawn(move || {
            let st_hyd = h_hyd.state::<AppState>();
            let aud = sample_audio_state_with_icon_sources(&st_hyd);
            if lock_unpoisoned(&st_hyd.coordinator).is_start_visible() {
                let _ = h_hyd.emit_to("start", "audio-mixer-updated", &aud);
                let _ = h_hyd.emit_to("start", "audio-master-updated", &aud.to_master_state());
            } else {
                return;
            }
            let med = media::poll_media_state();
            if lock_unpoisoned(&st_hyd.coordinator).is_start_visible() {
                let _ = h_hyd.emit_to("start", "media-updated", &med);
            } else {
                return;
            }
            let tel = sample_telemetry_with_icon_sources(&st_hyd);
            if lock_unpoisoned(&st_hyd.coordinator).is_start_visible() {
                let _ = h_hyd.emit_to("start", "telemetry-updated", &tel);
            } else {
                return;
            }
            let periphs = system_widgets::get_peripheral_batteries();
            if lock_unpoisoned(&st_hyd.coordinator).is_start_visible() {
                let _ = h_hyd.emit_to("start", "peripherals-updated", &periphs);
            }
        });
    }

    // 4. Emit-Before-Show Paint-Sync + Preview Horizontal Glide for Flyout
    if actions.flyout_visible == Some(true) {
        *lock_unpoisoned(&st.pending_flyout_exit) = None;
        win_shell::cancel_window_x_glide();
        let show_gen = actions
            .flyout_anim_gen
            .unwrap_or_else(|| st.flyout_anim_gen.fetch_add(1, Ordering::SeqCst) + 1);
        st.flyout_anim_gen.store(show_gen, Ordering::SeqCst);
        let non_activating = actions.flyout_non_activating.unwrap_or(false);

        if let Some(req) = flyout_req {
            let layout = compute_flyout_layout(app, req);
            let mut enriched = req.clone();
            enriched.gen = show_gen;
            enriched.anchor_origin_x = Some(layout.anchor_origin_x);
            *lock_unpoisoned(&st.last_flyout_req) = Some(enriched.clone());

            let fly_hwnd = app.get_webview_window("flyout").and_then(|w| win_hwnd(&w));
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

            if is_same_width_preview_scrub {
                *lock_unpoisoned(&st.pending_flyout_show) = None;
                if let (Some(hwnd), Some((_, prev_rect))) = (fly_hwnd, prev_bounds) {
                    win_shell::set_flyout_non_activating(non_activating);
                    win_shell::set_window_hit_transparent(hwnd, false);
                    win_shell::glide_window_x_dwm(
                        hwnd,
                        prev_rect.left,
                        layout.rect.left,
                        layout.rect.top,
                        PREVIEW_GLIDE_DURATION_MS,
                    );
                }
                *lock_unpoisoned(&st.last_flyout_bounds) = Some((req.kind, layout.rect));
                if actions.emit_flyout_show {
                    let _ = app.emit("flyout-show", &enriched);
                }
            } else {
                if let Some(fly) = app.get_webview_window("flyout") {
                    if let Some(hwnd) = win_hwnd(&fly) {
                        win_shell::set_flyout_non_activating(non_activating);
                        win_shell::set_window_hit_transparent(hwnd, false);
                        if !was_visible {
                            apply_webview_bounds(
                                &fly,
                                hwnd,
                                layout.rect.left,
                                layout.rect.top,
                                layout.rect.width(),
                                layout.rect.height(),
                            );
                        }
                    }
                }

                *lock_unpoisoned(&st.pending_flyout_show) = Some(PendingSurfaceShow {
                    gen: show_gen,
                    rect: layout.rect,
                    scale_factor: layout.scale_factor,
                    bottom_anchor_y: layout.bottom_anchor_y,
                    monitor_top: layout.monitor_top,
                    activate: !non_activating,
                    anchor_x: layout.anchor_origin_x,
                    monitor: req.monitor.clone(),
                    was_already_visible: was_visible,
                });

                // Emit flyout-show BEFORE ShowWindow (Paint-Sync Protocol)
                if actions.emit_flyout_show {
                    let _ = app.emit("flyout-show", &enriched);
                }

                // 22ms max Paint-Sync failsafe timer
                let h_fail = app.clone();
                let anim_gen = Arc::clone(&st.flyout_anim_gen);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(PAINT_SYNC_SHOW_FAILSAFE_MS));
                    if anim_gen.load(Ordering::SeqCst) != show_gen {
                        return;
                    }
                    let h = h_fail.clone();
                    let _ = h_fail.run_on_main_thread(move || {
                        commit_pending_surface_show(&h, "flyout", show_gen, None);
                    });
                });
            }

            // Hydrate Flyout immediately on hidden -> visible transition (AD-6)
            let snapshot = lock_unpoisoned(&st.last_window_groups).clone();
            let _ = app.emit_to("flyout", "windows-updated", &snapshot);
            match req.kind {
                FlyoutKind::Sound => {
                    let media_snap = media::get_media_snapshot();
                    let _ = app.emit_to("flyout", "media-updated", &media_snap);
                    let h_aud = app.clone();
                    std::thread::spawn(move || {
                        let st_aud = h_aud.state::<AppState>();
                        let aud = sample_audio_state_with_icon_sources(&st_aud);
                        if lock_unpoisoned(&st_aud.coordinator).active_flyout()
                            == Some(FlyoutKind::Sound)
                        {
                            let _ = h_aud.emit_to("flyout", "audio-mixer-updated", &aud);
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
                            let _ = h_aud.emit_to("flyout", "media-updated", &med);
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

    let games = apps_indexer::scan_games_lightweight();
    *lock_unpoisoned(&st.steam_games) = games.clone();
    let _ = app.emit_to("start", "games-indexed", &games);

    let files = apps_indexer::scan_user_files();
    *lock_unpoisoned(&st.indexed_files) = files.clone();
    let _ = app.emit_to("start", "files-indexed", &files);
}

fn emit_media_to_visible_surfaces(app: &AppHandle, st: &MediaState) {
    let (start_vis, sound_vis) = {
        let app_state = app.state::<AppState>();
        let coord = lock_unpoisoned(&app_state.coordinator);
        (
            coord.is_start_visible(),
            coord.active_flyout() == Some(FlyoutKind::Sound),
        )
    };
    if start_vis {
        let _ = app.emit_to("start", "media-updated", st);
    }
    if sound_vis {
        let _ = app.emit_to("flyout", "media-updated", st);
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
        *lock_unpoisoned(&state.secondary_taskbar_content_width) = logical_width;
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
    commit_pending_surface_show(&app, &surface, gen, measured_height);
}

#[tauri::command]
fn surface_exit_ack(app: AppHandle, surface: String, gen: u64) {
    commit_pending_surface_exit(&app, &surface, gen);
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
        req.height = height.clamp(48, 820);
        req.clone()
    };
    if let Some(fly) = app.get_webview_window("flyout") {
        if let Some(hwnd) = win_hwnd(&fly) {
            let layout = position_flyout_window(&app, &fly, hwnd, &updated_req);
            *lock_unpoisoned(&state.last_flyout_bounds) = Some((kind, layout.rect));
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
async fn get_open_windows(app: AppHandle) -> Result<Vec<AppWindowGroup>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (groups, sources) =
            windows_tracker::list_open_window_groups_cached_with_sources(&state.icon_cache);
        state.register_icon_sources(sources);
        *lock_unpoisoned(&state.last_window_groups) = groups.clone();
        groups
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
        for raw in targets {
            let trimmed = raw.trim();
            if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
                continue;
            }
            let low = trimmed.to_ascii_lowercase();
            let is_local_or_uwp = trimmed.contains(":\\")
                || trimmed.starts_with("\\\\")
                || low.starts_with("shell:appsfolder\\");
            if !is_local_or_uwp {
                continue;
            }
            if let Some(uri) =
                windows_tracker::extract_icon_for_path_cached(trimmed, &state.icon_cache)
            {
                let key = if low.starts_with("shell:appsfolder\\") {
                    icon_cache::aumid_key(trimmed)
                } else {
                    icon_cache::path_icon_key(trimmed)
                };
                new_sources.push((key, trimmed.to_string()));
                out.insert(trimmed.to_string(), uri);
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
    exe_path: String,
    aumid: Option<String>,
) -> Result<Vec<JumpListItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        windows_tracker::get_jump_list(&exe_path, aumid.as_deref())
    })
    .await
    .map_err(|e| format!("task join error: {e}"))
}

#[tauri::command]
async fn get_window_thumbnail(hwnd: isize) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        windows_tracker::capture_window_thumbnail(hwnd, 240, 140)
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
            let is_resolvable = target_trimmed.contains(":\\")
                || target_trimmed.starts_with("\\\\")
                || low.starts_with("shell:appsfolder\\");
            if is_resolvable {
                let key = if low.starts_with("shell:appsfolder\\") {
                    icon_cache::aumid_key(&target_trimmed)
                } else {
                    icon_cache::path_icon_key(&target_trimmed)
                };
                if windows_tracker::extract_icon_for_path_cached(&target_trimmed, &state.icon_cache)
                    .is_some()
                {
                    state.register_icon_sources(vec![(key.clone(), target_trimmed)]);
                    if item.icon_key.trim().is_empty()
                        || item.icon_key == "app"
                        || item.icon_key == "file"
                    {
                        item.icon_key = key;
                    }
                }
            }
        }
        item.icon_data = None;

        let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
            config::add_pin_to_space(cfg, &space, item)
        })?;
        Ok(updated)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
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
) -> Result<TestBarConfig, String> {
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::update_pin(
            cfg,
            &space,
            &pin_id,
            title.clone(),
            target.clone(),
            args.clone(),
            alt_action.clone(),
            alt_label.clone(),
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
fn set_appearance_settings(
    app: AppHandle,
    state: State<AppState>,
    glass_depth: Option<String>,
    clock_show_seconds: Option<bool>,
    clock_show_weekday: Option<bool>,
    start_remember_space: Option<bool>,
) -> Result<TestBarConfig, String> {
    let mut inner_err: Option<String> = None;
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        match config::set_appearance_settings(
            cfg,
            glass_depth.clone(),
            clock_show_seconds,
            clock_show_weekday,
            start_remember_space,
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
fn hide_tray_app(
    app: AppHandle,
    state: State<AppState>,
    key: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, |cfg| config::hide_tray_key(cfg, &key))?;
    Ok(updated)
}

#[tauri::command]
fn restore_tray_app(
    app: AppHandle,
    state: State<AppState>,
    key: String,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, |cfg| config::restore_tray_key(cfg, &key))?;
    Ok(updated)
}

#[tauri::command]
fn restore_all_tray_apps(
    app: AppHandle,
    state: State<AppState>,
) -> Result<TestBarConfig, String> {
    let (updated, _) =
        mutate_config_and_emit(&app, &state, config::restore_all_tray_keys)?;
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
            let _ = app.emit_to("start", "clipboard-updated", &clips);
        }
        let h_sync = app.clone();
        let bar_mode = updated.bar_mode.clone();
        let hide_native = state.hide_native;
        let _ = app.run_on_main_thread(move || {
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
        let normalized_vol = volume.map(|v| {
            if !v.is_finite() {
                0.0
            } else if v > 1.0 {
                (v.clamp(0.0, 100.0)) / 100.0
            } else {
                v.clamp(0.0, 1.0)
            }
        });
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
        let _ = app.emit_to("start", "clipboard-updated", &list);
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
        let _ = app.emit_to("start", "clipboard-updated", &list);
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
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        dispatch_coordinator(&h, None, |coord| coord.hide_start_immediate());
    });
    tauri::async_runtime::spawn_blocking(move || {
        // Give Start SW_HIDE a brief moment to release foreground before restoring target & injecting Ctrl+V
        std::thread::sleep(std::time::Duration::from_millis(45));
        system_widgets::paste_clipboard_text(target_hwnd, &text)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
fn clear_unpinned_clips(app: AppHandle, state: State<AppState>) -> Vec<ClipboardEntry> {
    let list = system_widgets::clear_unpinned_clipboard();
    if should_emit_start_clipboard_update(&lock_unpoisoned(&state.coordinator)) {
        let _ = app.emit_to("start", "clipboard-updated", &list);
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
async fn get_tray_apps(app: AppHandle) -> Result<Vec<TrayAppItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (open_groups, win_sources) =
            windows_tracker::list_open_window_groups_cached_with_sources(&state.icon_cache);
        state.register_icon_sources(win_sources);
        *lock_unpoisoned(&state.last_window_groups) = open_groups.clone();

        let hidden_keys = lock_unpoisoned(&state.config).hidden_tray_keys.clone();
        let (tray_items, tray_sources) = {
            let mut cache = lock_unpoisoned(&state.icon_cache);
            system_widgets::list_tray_background_apps_filtered_with_open(
                &mut cache,
                &open_groups,
                &hidden_keys,
            )
        };
        state.register_icon_sources(tray_sources);
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
async fn empty_recycle_bin() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(system_widgets::empty_recycle_bin)
        .await
        .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
async fn open_screen_snip(app: AppHandle) -> Result<(), String> {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        close_start_and_flyout(&h, true, true);
    });
    tauri::async_runtime::spawn_blocking(system_widgets::open_screen_snip)
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
    let flyout_anim_gen = Arc::new(AtomicU64::new(0));
    let start_anim_gen = Arc::new(AtomicU64::new(0));
    let last_external_fg = Arc::new(AtomicIsize::new(0));
    let index_refresh_running = Arc::new(AtomicBool::new(false));
    win_shell::set_overlay_open_hint(false);
    let app_state = AppState {
        config: Mutex::new(initial_cfg),
        icon_cache: Mutex::new(IconCache::default()),
        icon_sources: Mutex::new(HashMap::new()),
        last_window_groups: Mutex::new(Vec::new()),
        installed_apps: Mutex::new(Vec::new()),
        indexed_files: Mutex::new(Vec::new()),
        steam_games: Mutex::new(Vec::new()),
        hide_native,
        coordinator: Mutex::new(WindowCoordinator::new()),
        overlay_open: Arc::clone(&overlay_open),
        fullscreen_suppressed: Arc::clone(&fullscreen_suppressed),
        secondary_fullscreen_suppressed: Arc::clone(&secondary_fullscreen_suppressed),
        taskbar_content_width: Mutex::new(620),
        secondary_taskbar_content_width: Mutex::new(620),
        secondary_taskbar_visible: Arc::clone(&secondary_taskbar_visible),
        last_flyout_req: Mutex::new(None),
        last_flyout_bounds: Mutex::new(None),
        pending_start_show: Mutex::new(None),
        pending_flyout_show: Mutex::new(None),
        pending_start_exit: Mutex::new(None),
        pending_flyout_exit: Mutex::new(None),
        flyout_anim_gen: Arc::clone(&flyout_anim_gen),
        start_anim_gen: Arc::clone(&start_anim_gen),
        dpi_override: AtomicU32::new(0),
        last_external_fg: Arc::clone(&last_external_fg),
        index_refresh_running: Arc::clone(&index_refresh_running),
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
            add_custom_space,
            update_custom_space,
            delete_custom_space,
            add_game_combo,
            update_game_combo,
            delete_game_combo,
            hide_tray_app,
            restore_tray_app,
            restore_all_tray_apps,
            record_frecency,
            pick_folder,
            pick_file,
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
            get_tray_apps,
            activate_tray_app,
            end_tray_app_task,
            open_native_windows_tray,
            empty_recycle_bin,
            open_screen_snip,
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

            // Style & position pre-warmed transparent borderless shell windows
            if let Some(tb) = app.get_webview_window("taskbar") {
                if let Some(hwnd) = win_hwnd(&tb) {
                    win_shell::style_overlay_window(hwnd, true);
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
                                                h.state::<AppState>().set_dpi_override(dpi);
                                            }
                                            ShellAppBarEvent::DisplayChanged => {
                                                h.state::<AppState>().clear_dpi_override();
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
                            win_shell::restore_native_taskbar();
                            return Err(err.into());
                        }
                    }
                    let _ = reevaluate_fullscreen(&handle, None);
                    position_taskbar_window(&handle);
                    if lock_unpoisoned(&app.state::<AppState>().coordinator).should_show_taskbar() {
                        win_shell::set_window_visible(hwnd, true, true);
                    }
                }
            }

            if let Some(tb2) = app.get_webview_window("taskbar-secondary") {
                if let Some(hwnd) = win_hwnd(&tb2) {
                    win_shell::style_overlay_window(hwnd, true);
                }
            }
            sync_secondary_taskbar(&handle);

            if let Some(start_win) = app.get_webview_window("start") {
                if let Some(hwnd) = win_hwnd(&start_win) {
                    win_shell::style_overlay_window(hwnd, false);
                    let _ = position_start_window_on_monitor(&handle, &start_win, hwnd, None);
                }
                let h_blur = handle.clone();
                start_win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::Focused(false) = ev {
                        dispatch_coordinator(&h_blur, None, |coord| {
                            coord.on_start_focus_lost(monotonic_ms())
                        });
                    }
                });
            }

            if let Some(fly_win) = app.get_webview_window("flyout") {
                if let Some(hwnd) = win_hwnd(&fly_win) {
                    win_shell::style_overlay_window(hwnd, false);
                    win_shell::install_flyout_noactivate_subclass(hwnd);
                }
                let h_blur = handle.clone();
                fly_win.on_window_event(move |ev| {
                    if let tauri::WindowEvent::Focused(false) = ev {
                        dispatch_coordinator(&h_blur, None, |coord| {
                            coord.on_flyout_focus_lost(monotonic_ms())
                        });
                    }
                });
            }

            // Global lone-Win key hook, global click-outside & Ctrl+Alt+Shift+Esc emergency restore.
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
                        if h.state::<AppState>().hide_native {
                            win_shell::restore_native_taskbar();
                        }
                        h.exit(0);
                    });
                }
            });

            // Dedicated MTA WinRT SMTC worker thread (AD-5)
            media::start_media_worker();

            // Live Clipboard History listener: hydrates persisted pins at startup, stays
            // continuously active even while Start is hidden, and gates IPC emission to visible Start.
            let h_clip = handle.clone();
            system_widgets::start_clipboard_listener(&initial_pinned_clipboard, move |entries| {
                let emit = should_emit_start_clipboard_update(
                    &lock_unpoisoned(&h_clip.state::<AppState>().coordinator),
                );
                if emit {
                    let _ = h_clip.emit_to("start", "clipboard-updated", &entries);
                }
            });

            // Async background app (Start .lnk + UWP), lightweight games (Steam/Epic/GOG), & user files indexer
            // Runs at startup and repeats cleanly every 15 minutes so new installs/downloads appear automatically.
            let h_idx = handle.clone();
            std::thread::spawn(move || loop {
                run_background_index_refresh(&h_idx);
                std::thread::sleep(std::time::Duration::from_secs(PERIODIC_REINDEX_INTERVAL_SECS));
            });

            // Fast lightweight poller for open windows (diff-only visible-window broadcast + icon_sources),
            // keyboard layout, external foreground tracking, native taskbar hide guard, and per-monitor fullscreen.
            let h_fast = handle.clone();
            let fast_fs_primary = Arc::clone(&fullscreen_suppressed);
            let fast_fs_secondary = Arc::clone(&secondary_fullscreen_suppressed);
            std::thread::spawn(move || {
                let mut last_layout = String::new();
                let mut last_windows_fp: Option<WindowsFingerprint> = None;
                loop {
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
                        for target in targets {
                            let _ = h_fast.emit_to(target, "windows-updated", &groups);
                        }
                    }

                    let layout = system_widgets::get_keyboard_layout_code();
                    if layout != last_layout {
                        last_layout = layout.clone();
                        let _ = h_fast.emit("layout-updated", &layout);
                    }

                    if hide_native && !win_shell::is_restore_in_progress() {
                        win_shell::ensure_native_taskbar_hidden_if_needed();
                    }

                    let fs = win_shell::detect_fullscreen_per_monitor();
                    if fs.primary != fast_fs_primary.load(Ordering::Acquire)
                        || fs.secondary != fast_fs_secondary.load(Ordering::Acquire)
                    {
                        let h = h_fast.clone();
                        let _ = h_fast.run_on_main_thread(move || {
                            let _ = reevaluate_fullscreen(&h, None);
                        });
                    }

                    std::thread::sleep(std::time::Duration::from_millis(650));
                }
            });

            // Slower poller for split audio (diff-gated, visible-surface targeted with icon_sources),
            // diff-gated network info, and hidden-window-gated Start/Sound live widgets.
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
                            for target in targets {
                                let _ = h_slow.emit_to(target, "audio-mixer-updated", &audio_st);
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
                            let (sec_vis, flyout_vis) = {
                                let st = h_slow.state::<AppState>();
                                let sv = st.secondary_taskbar_visible.load(Ordering::Acquire);
                                let fv = lock_unpoisoned(&st.coordinator).is_flyout_visible();
                                (sv, fv)
                            };
                            let _ = h_slow.emit_to("taskbar", "network-updated", &net);
                            if sec_vis {
                                let _ = h_slow.emit_to("taskbar-secondary", "network-updated", &net);
                            }
                            if flyout_vis {
                                let _ = h_slow.emit_to("flyout", "network-updated", &net);
                            }
                        }
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
            installed_apps: Mutex::new(Vec::new()),
            indexed_files: Mutex::new(Vec::new()),
            steam_games: Mutex::new(Vec::new()),
            hide_native: false,
            coordinator: Mutex::new(WindowCoordinator::new()),
            overlay_open: Arc::new(AtomicBool::new(false)),
            fullscreen_suppressed: Arc::new(AtomicBool::new(false)),
            secondary_fullscreen_suppressed: Arc::new(AtomicBool::new(false)),
            taskbar_content_width: Mutex::new(width),
            secondary_taskbar_content_width: Mutex::new(width),
            secondary_taskbar_visible: Arc::new(AtomicBool::new(false)),
            last_flyout_req: Mutex::new(None),
            last_flyout_bounds: Mutex::new(None),
            pending_start_show: Mutex::new(None),
            pending_flyout_show: Mutex::new(None),
            pending_start_exit: Mutex::new(None),
            pending_flyout_exit: Mutex::new(None),
            flyout_anim_gen: Arc::new(AtomicU64::new(0)),
            start_anim_gen: Arc::new(AtomicU64::new(0)),
            dpi_override: AtomicU32::new(0),
            last_external_fg: Arc::new(AtomicIsize::new(0)),
            index_refresh_running: Arc::new(AtomicBool::new(false)),
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

        // Normal desktop: bounds update allowed with initial Dock width 620 (+ 36px transparent margin = 656, 84px height)
        let initial_bounds = plan_taskbar_bounds_update(&state, mon, 1.0, 1.0)
            .expect("bounds should be allowed on normal desktop");
        assert_eq!(initial_bounds.width(), 656);
        assert_eq!(initial_bounds.height(), 84);
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

        // Exit fullscreen -> taskbar_visible == Some(true) and bounds reflect latest Edge mode
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
        assert_eq!(restored_edge, PixelRect::new(0, 1080 - 52, 1920, 1080));

        // Switch back to dock -> reflects recorded logical width 840 (+ 36 shadow margin = 876)
        state.config.lock().unwrap().bar_mode = "dock".to_string();
        let restored_dock = plan_taskbar_bounds_update(&state, mon, 1.0, 1.0)
            .expect("bounds should apply in dock mode");
        assert_eq!(restored_dock.width(), 876);
        assert_eq!(restored_dock.height(), 84);
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
        // (620 + 36) * 1.5 = 984, height = 84 * 1.5 = 126
        assert_eq!(restored.width(), 984);
        assert_eq!(restored.height(), 126);

        // DisplayChanged clears dpi_override so fallback scale_factor applies again
        state.clear_dpi_override();
        assert_eq!(state.current_dpi_override(), None);
        let after_display_change = plan_taskbar_bounds_update(&state, mon, 1.25, 1.0).unwrap();
        // (620 + 36) * 1.25 = 820, height = 84 * 1.25 = 105
        assert_eq!(after_display_change.width(), 820);
        assert_eq!(after_display_change.height(), 105);

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

        // Valid HiDPI 200% (sf = 2.0) on normal monitor
        let hidpi_edge = compute_taskbar_bounds(mon, 2.0, "edge", 620);
        assert_eq!(hidpi_edge, PixelRect::new(0, 1440 - 104, 2560, 1440));
    }

    #[test]
    fn effective_scale_with_text_scale_factor_and_start_dock_bounds() {
        // 96 DPI (1.0) * 140% TextScaleFactor (1.4) = 1.4
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
        let dock_140 = plan_taskbar_bounds_update(&state, mon_1440, 1.0, 1.4).unwrap();
        // (620 + 36) * 1.4 = 918.4 -> 918; 84 * 1.4 = 117.6 -> 118
        assert_eq!(dock_140.width(), 918);
        assert_eq!(dock_140.height(), 118);

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

        // Start bounds at 1.4x on 2560x1440 -> full 1188x728 scaled cleanly
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
        let make_sample = |title: &str, active: bool, minimized: bool, icon_key: Option<&str>| {
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
                }],
            }]
        };

        let mut coord = WindowCoordinator::new();
        let mut last_fp: Option<WindowsFingerprint> = None;

        // First poll always emits -> only taskbar is visible
        let g1 = make_sample("Tab 1", true, false, Some("exe:aaa"));
        assert_eq!(
            diff_window_update_targets(&mut last_fp, &g1, &coord),
            Some(vec!["taskbar"])
        );

        // Identical poll -> NO emit (None)
        assert_eq!(diff_window_update_targets(&mut last_fp, &g1, &coord), None);

        // Open Start -> title change emits to taskbar + start (not flyout)
        let _ = coord.request_toggle_start(Some(true), 100);
        let g2 = make_sample("Tab 2", true, false, Some("exe:aaa"));
        assert_eq!(
            diff_window_update_targets(&mut last_fp, &g2, &coord),
            Some(vec!["taskbar", "start"])
        );
        assert_eq!(diff_window_update_targets(&mut last_fp, &g2, &coord), None);

        // Open Sound flyout (closes Start) -> minimize change emits to taskbar + flyout
        let _ = coord.request_open_flyout(FlyoutKind::Sound, 200);
        let g3 = make_sample("Tab 2", false, true, Some("exe:aaa"));
        assert_eq!(
            diff_window_update_targets(&mut last_fp, &g3, &coord),
            Some(vec!["taskbar", "flyout"])
        );

        // Icon key change also triggers diff
        let g4 = make_sample("Tab 2", false, true, Some("exe:bbb"));
        assert_eq!(
            diff_window_update_targets(&mut last_fp, &g4, &coord),
            Some(vec!["taskbar", "flyout"])
        );

        // Secondary taskbar receives windows-updated ONLY when visible
        let g5 = make_sample("Tab 3", false, false, Some("exe:bbb"));
        assert_eq!(
            diff_window_update_targets_with_secondary(&mut last_fp, &g5, &coord, true),
            Some(vec!["taskbar", "taskbar-secondary", "flyout"])
        );
        // Identical poll skips broadcast even with secondary visible
        assert_eq!(
            diff_window_update_targets_with_secondary(&mut last_fp, &g5, &coord, true),
            None
        );
    }

    #[test]
    fn compute_secondary_taskbar_bounds_handles_vertical_display2_scaling_and_clamping() {
        // DISPLAY2 vertical 1440x2560 placed right of 4K primary (3840,0 -> 5280,2560), 96 DPI, 1.4x TextScaleFactor
        let display2 = win_shell::MonitorRect {
            left: 3840,
            top: 0,
            right: 5280,
            bottom: 2560,
            width: 1440,
            height: 2560,
            dpi: 96,
        };

        // 1. Standard Dock width 700 (+ 36 padding = 736) * 1.4 = 1030px, centered inside DISPLAY2
        let dock_fit = compute_secondary_taskbar_bounds(display2, 1.4, "dock", 700);
        assert_eq!(dock_fit.width(), 1030);
        assert_eq!(dock_fit.height(), 118); // round(84 * 1.4)
        assert_eq!(dock_fit.left, 3840 + (1440 - 1030) / 2);
        assert_eq!(dock_fit.right, dock_fit.left + 1030);
        assert_eq!(dock_fit.bottom, 2560);
        assert_eq!(dock_fit.top, 2560 - 118);

        // 2. Wide Dock (1200 logical -> (1200 + 36) * 1.4 = 1730px) clamps cleanly to mon.width - 16 = 1424px
        let dock_clamped = compute_secondary_taskbar_bounds(display2, 1.4, "dock", 1200);
        assert_eq!(dock_clamped.width(), 1424);
        assert_eq!(dock_clamped.left, 3840 + 8);
        assert_eq!(dock_clamped.right, 5280 - 8);
        assert_eq!(dock_clamped.bottom, 2560);

        // 3. Edge mode fills exact DISPLAY2 width and scales 52px bar height by 1.4x -> 73px
        let edge_sec = compute_secondary_taskbar_bounds(display2, 1.4, "edge", 700);
        assert_eq!(edge_sec, PixelRect::new(3840, 2560 - 73, 5280, 2560));

        // 4. Secondary HiDPI monitor (144 DPI = 1.5x) * 1.2x TextScaleFactor = 1.8x, left-of-primary negative coordinates
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
        // (620 + 36) * 1.8 = 1180.8 -> 1181; 84 * 1.8 = 151.2 -> 151
        assert_eq!(left_dock.width(), 1181);
        assert_eq!(left_dock.height(), 151);
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
              "name": "PoE2",
              "subtitle": "Steam",
              "targets": ["steam://rungameid/2694490"],
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
                    title: "WizTree".to_string(),
                    subtitle: "Disk".to_string(),
                    target: r"C:\Program Files\WizTree\WizTree64.exe".to_string(),
                    args: None,
                    alt_action: String::new(),
                    alt_label: None,
                    accent_rgb: "56,189,248".to_string(),
                    icon_key: "disk".to_string(),
                    icon_data: None,
                    match_exe: None,
                    match_aumid: None,
                    combo_targets: None,
                },
            )
        })
        .expect("add studio tool");
        assert!(ch3);
        assert_eq!(c3.revision, 2);
        assert_eq!(
            c3.studio_tools[0].match_exe.as_deref(),
            Some("wiztree64.exe")
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
        let steam = windows_tracker::parse_combo_target("steam://rungameid/2694490").unwrap();
        assert_eq!(steam.target, "steam://rungameid/2694490");
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
        // 1. Anchor origin X clamps cleanly within [18, total_w - 18] logical px
        assert!((compute_flyout_anchor_origin_x(500, 400, 320.0, 1.0) - 100.0).abs() < 1e-6);
        assert!((compute_flyout_anchor_origin_x(405, 400, 320.0, 1.0) - 18.0).abs() < 1e-6);
        assert!((compute_flyout_anchor_origin_x(999, 400, 320.0, 1.0) - 302.0).abs() < 1e-6);
        assert!((compute_flyout_anchor_origin_x(700, 400, 320.0, 1.5) - 200.0).abs() < 1e-6);

        // 2. StartStateEvent and FlyoutHideEvent serialize in camelCase
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
}
