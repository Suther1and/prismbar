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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use apps_indexer::{IndexedApp, IndexedFile, SteamGame};
use audio::{AudioMasterState, AudioState};
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
const START_DEFAULT_LOGICAL_WIDTH: f64 = 1172.0;
const START_DEFAULT_LOGICAL_HEIGHT: f64 = 712.0;
const FLYOUT_EXIT_ANIM_MS: u64 = 130;
const START_EXIT_ANIM_MS: u64 = 135;

static MONOTONIC_EPOCH: OnceLock<std::time::Instant> = OnceLock::new();

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
    pub taskbar_content_width: Mutex<i32>,
    pub secondary_taskbar_content_width: Mutex<i32>,
    pub secondary_taskbar_visible: Arc<AtomicBool>,
    pub last_flyout_req: Mutex<Option<FlyoutRequest>>,
    pub flyout_anim_gen: Arc<AtomicU64>,
    pub start_anim_gen: Arc<AtomicU64>,
    pub dpi_override: AtomicU32,
}

impl AppState {
    fn sync_overlay_flag(&self, coord: &WindowCoordinator) {
        let overlay_open = coord.has_open_overlay();
        self.overlay_open.store(overlay_open, Ordering::Release);
        win_shell::set_overlay_open_hint(overlay_open);
        self.fullscreen_suppressed
            .store(coord.is_fullscreen_suppressed(), Ordering::Release);
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

fn reevaluate_fullscreen(app: &AppHandle, hint_entering: Option<bool>) -> CoordinatorActions {
    let snap = win_shell::query_fullscreen_snapshot();
    let actions = apply_fullscreen_snapshot(&app.state::<AppState>(), hint_entering, &snap);
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
    let avail_w = ((mon_w as f64 / sf) - 24.0).max(320.0);
    let avail_h = ((mon_h as f64 / sf) - 88.0).max(320.0);
    let logical_w = avail_w.min(START_DEFAULT_LOGICAL_WIDTH);
    let logical_h = avail_h.min(START_DEFAULT_LOGICAL_HEIGHT);
    let max_w_px = (mon_w - 16).max(0);
    let max_h_px = (mon_h - 16).max(0);
    let w_px = ((logical_w * sf).round() as i32).clamp(0, max_w_px);
    let h_px = ((logical_h * sf).round() as i32).clamp(0, max_h_px);
    let bar_gap = (76.0 * sf).round() as i32;
    let x_px = mon.left + (mon_w - w_px) / 2;
    let y_px = (mon.bottom - bar_gap - h_px).max(mon.top + 8);
    PixelRect::new(x_px, y_px, x_px + w_px, y_px + h_px)
}

/// Synchronizes both Tauri/WebView2 inner controller bounds and synchronous Win32 HWND bounds
/// without activating or forcing hidden windows visible (AD-1).
fn apply_webview_bounds(win: &WebviewWindow, hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    let w_clamped = w.max(1);
    let h_clamped = h.max(1);
    let _ = win.set_size(tauri::PhysicalSize::new(w_clamped as u32, h_clamped as u32));
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    win_shell::position_window(hwnd, x, y, w_clamped, h_clamped);
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

    let sec_mon = win_shell::secondary_monitor_rects().into_iter().next();
    if sec_mode == config::SECONDARY_TASKBAR_MODE_TESTBAR {
        if let Some(mon) = sec_mon {
            let logical_w = *lock_unpoisoned(&st.secondary_taskbar_content_width);
            let text_scale = win_shell::windows_text_scale_factor();
            let rect = compute_secondary_taskbar_bounds(mon, text_scale, &bar_mode, logical_w);
            apply_webview_bounds(&tb2, hwnd, rect.left, rect.top, rect.width(), rect.height());
            let was_visible = st.secondary_taskbar_visible.swap(true, Ordering::AcqRel);
            win_shell::set_window_visible(hwnd, true, true);
            if !was_visible {
                let win_snap = lock_unpoisoned(&st.last_window_groups).clone();
                let start_open = lock_unpoisoned(&st.coordinator).is_start_open();
                let layout = system_widgets::get_keyboard_layout_code();
                let _ = app.emit_to("taskbar-secondary", "config-updated", &cfg_snapshot);
                let _ = app.emit_to("taskbar-secondary", "windows-updated", &win_snap);
                let _ = app.emit_to("taskbar-secondary", "layout-updated", &layout);
                let _ = app.emit_to("taskbar-secondary", "start-state", start_open);
                let h_sec = app.clone();
                std::thread::spawn(move || {
                    let master = audio::get_audio_master_state();
                    let _ = h_sec.emit_to("taskbar-secondary", "audio-master-updated", &master);
                });
            }
            return;
        }
    }

    st.secondary_taskbar_visible.store(false, Ordering::Release);
    win_shell::set_window_visible(hwnd, false, true);
}

fn position_start_window(app: &AppHandle, win: &WebviewWindow, hwnd: isize) {
    let sf = scale_factor(app);
    let mon = win_shell::primary_monitor_rect();
    let rect = compute_start_bounds(mon, sf);
    apply_webview_bounds(win, hwnd, rect.left, rect.top, rect.width(), rect.height());
}

fn position_flyout_window(
    app: &AppHandle,
    win: &WebviewWindow,
    hwnd: isize,
    req: &FlyoutRequest,
) {
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

    let req_w = req.width.clamp(1, 2000);
    let req_h = req.height.clamp(1, 2000);
    let w_px = ((req_w + 24) as f64 * sf).round() as i32;
    let h_px = ((req_h + 24) as f64 * sf).round() as i32;
    let is_edge =
        BarMode::from_config_str(&lock_unpoisoned(&app.state::<AppState>().config).bar_mode)
            == BarMode::Edge;
    // Account for `.flyout-root`'s 12px bottom padding so Preview sits flush and click flyouts sit 10px above.
    let gap_logical = match (is_edge, req.kind) {
        (true, FlyoutKind::Preview) => 40.0,
        (true, _) => 50.0,
        (false, FlyoutKind::Preview) => 52.0,
        (false, _) => 62.0,
    };
    let bar_gap = (gap_logical * sf).round() as i32;

    let mut x_px = anchor_screen_x_px - w_px / 2;
    x_px = x_px.clamp(mon.left + 8, (mon.right - w_px - 8).max(mon.left + 8));
    let y_px = (mon.bottom - bar_gap - h_px).max(mon.top + 8);

    apply_webview_bounds(win, hwnd, x_px, y_px, w_px, h_px);
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

    // 1. Hide outgoing surfaces first (with animated flyout exit unless Start/fullscreen takes over)
    if actions.flyout_visible == Some(false) {
        *lock_unpoisoned(&st.last_flyout_req) = None;
        let hide_gen = st.flyout_anim_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let hide_immediately =
            actions.start_visible == Some(true) || actions.taskbar_visible == Some(false);

        if actions.emit_flyout_hide {
            let _ = app.emit("flyout-hide", ());
        }

        if hide_immediately {
            win_shell::set_flyout_non_activating(false);
            if let Some(fly) = app.get_webview_window("flyout") {
                if let Some(hwnd) = win_hwnd(&fly) {
                    win_shell::set_window_visible(hwnd, false, true);
                }
            }
        } else {
            win_shell::set_flyout_non_activating(true);
            let h_hide = app.clone();
            let anim_gen = Arc::clone(&st.flyout_anim_gen);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(FLYOUT_EXIT_ANIM_MS));
                if anim_gen.load(Ordering::SeqCst) != hide_gen {
                    return;
                }
                let h = h_hide.clone();
                let _ = h_hide.run_on_main_thread(move || {
                    if anim_gen.load(Ordering::SeqCst) != hide_gen {
                        return;
                    }
                    let still_hidden =
                        !lock_unpoisoned(&h.state::<AppState>().coordinator).is_flyout_visible();
                    if still_hidden {
                        win_shell::set_flyout_non_activating(false);
                        if let Some(fly) = h.get_webview_window("flyout") {
                            if let Some(hwnd) = win_hwnd(&fly) {
                                win_shell::set_window_visible(hwnd, false, true);
                            }
                        }
                    }
                });
            });
        }
    }
    let mut start_state_emitted_early = false;
    if actions.start_visible == Some(false) {
        let hide_gen = st.start_anim_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let hide_immediately =
            actions.flyout_visible == Some(true) || actions.taskbar_visible == Some(false);

        if actions.emit_start_state == Some(false) {
            let _ = app.emit("start-state", false);
            start_state_emitted_early = true;
        }

        if hide_immediately {
            if let Some(win) = app.get_webview_window("start") {
                if let Some(hwnd) = win_hwnd(&win) {
                    win_shell::set_window_visible(hwnd, false, true);
                }
            }
        } else {
            let h_hide = app.clone();
            let anim_gen = Arc::clone(&st.start_anim_gen);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(START_EXIT_ANIM_MS));
                if anim_gen.load(Ordering::SeqCst) != hide_gen {
                    return;
                }
                let h = h_hide.clone();
                let _ = h_hide.run_on_main_thread(move || {
                    if anim_gen.load(Ordering::SeqCst) != hide_gen {
                        return;
                    }
                    let still_hidden =
                        !lock_unpoisoned(&h.state::<AppState>().coordinator).is_start_visible();
                    if still_hidden {
                        if let Some(win) = h.get_webview_window("start") {
                            if let Some(hwnd) = win_hwnd(&win) {
                                win_shell::set_window_visible(hwnd, false, true);
                            }
                        }
                    }
                });
            });
        }
    }

    // 2. Taskbar visibility (fullscreen auto-hide / restore)
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

    // 3. Show incoming surfaces via visibility only (never 1x1 shrink or per-open GWL_EXSTYLE flip)
    if actions.start_visible == Some(true) {
        // Cancel any pending delayed hide from a prior Start exit animation
        st.start_anim_gen.fetch_add(1, Ordering::SeqCst);
        if let Some(win) = app.get_webview_window("start") {
            if let Some(hwnd) = win_hwnd(&win) {
                position_start_window(app, &win, hwnd);
                win_shell::set_window_visible(hwnd, true, false);
                let _ = win.set_focus();
            }
        }
        // Hydrate Start immediately on hidden -> visible transition (AD-6)
        let cfg_snapshot = lock_unpoisoned(&st.config).clone();
        let win_snapshot = lock_unpoisoned(&st.last_window_groups).clone();
        let clips_snapshot = system_widgets::get_clipboard_history();
        let _ = app.emit_to("start", "config-updated", &cfg_snapshot);
        let _ = app.emit_to("start", "windows-updated", &win_snapshot);
        let _ = app.emit_to("start", "clipboard-updated", &clips_snapshot);
        let h_hyd = app.clone();
        std::thread::spawn(move || {
            let aud = audio::get_audio_state();
            if lock_unpoisoned(&h_hyd.state::<AppState>().coordinator).is_start_visible() {
                let _ = h_hyd.emit_to("start", "audio-mixer-updated", &aud);
                let _ = h_hyd.emit_to("start", "audio-master-updated", &aud.to_master_state());
            }
            let med = media::get_media_state();
            let tel = telemetry::get_telemetry();
            if lock_unpoisoned(&h_hyd.state::<AppState>().coordinator).is_start_visible() {
                let _ = h_hyd.emit_to("start", "media-updated", &med);
                let _ = h_hyd.emit_to("start", "telemetry-updated", &tel);
            }
        });
    }

    if actions.flyout_visible == Some(true) {
        // Cancel any pending delayed hide from a prior exit animation
        st.flyout_anim_gen.fetch_add(1, Ordering::SeqCst);
        if let Some(req) = flyout_req {
            *lock_unpoisoned(&st.last_flyout_req) = Some(req.clone());
        }
        if let Some(fly) = app.get_webview_window("flyout") {
            if let Some(hwnd) = win_hwnd(&fly) {
                let non_activating = actions.flyout_non_activating.unwrap_or(false);
                win_shell::set_flyout_non_activating(non_activating);
                if let Some(req) = flyout_req {
                    position_flyout_window(app, &fly, hwnd, req);
                }
                win_shell::set_window_visible(hwnd, true, non_activating);
                if !non_activating {
                    let _ = fly.set_focus();
                }
            }
        }
        // Hydrate Flyout immediately on hidden -> visible transition (AD-6)
        let snapshot = lock_unpoisoned(&st.last_window_groups).clone();
        let _ = app.emit_to("flyout", "windows-updated", &snapshot);
        if let Some(req) = flyout_req {
            match req.kind {
                FlyoutKind::Sound => {
                    let h_aud = app.clone();
                    std::thread::spawn(move || {
                        let aud = audio::get_audio_state();
                        if lock_unpoisoned(&h_aud.state::<AppState>().coordinator).active_flyout()
                            == Some(FlyoutKind::Sound)
                        {
                            let _ = h_aud.emit_to("flyout", "audio-mixer-updated", &aud);
                            let _ = h_aud.emit_to(
                                "flyout",
                                "audio-master-updated",
                                &aud.to_master_state(),
                            );
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

    // 4. Emit canonical state events
    if let Some(vis) = actions.emit_start_state {
        if !(start_state_emitted_early && !vis) {
            let _ = app.emit("start-state", vis);
        }
    }
    if actions.emit_flyout_hide && actions.flyout_visible != Some(false) {
        let _ = app.emit("flyout-hide", ());
    }
    if actions.emit_flyout_show {
        if let Some(req) = flyout_req {
            let _ = app.emit("flyout-show", req);
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
fn toggle_start(app: AppHandle, force: Option<bool>) {
    dispatch_coordinator(&app, None, |coord| {
        coord.request_toggle_start(force, monotonic_ms())
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
    dispatch_coordinator(&app, Some(&req), |coord| {
        coord.request_open_flyout(req.kind, monotonic_ms())
    });
}

#[tauri::command]
fn resize_flyout(app: AppHandle, state: State<AppState>, width: Option<i32>, height: i32) {
    let is_open = lock_unpoisoned(&state.coordinator).is_flyout_visible();
    if !is_open {
        return;
    }
    let updated_req = {
        let mut slot = lock_unpoisoned(&state.last_flyout_req);
        let Some(req) = slot.as_mut() else {
            return;
        };
        if let Some(w) = width {
            req.width = w.clamp(120, 1200);
        }
        req.height = height.clamp(80, 720);
        req.clone()
    };
    if let Some(fly) = app.get_webview_window("flyout") {
        if let Some(hwnd) = win_hwnd(&fly) {
            position_flyout_window(&app, &fly, hwnd, &updated_req);
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
fn get_open_windows(state: State<AppState>) -> Vec<AppWindowGroup> {
    let groups = windows_tracker::list_open_window_groups_cached(&state.icon_cache);
    *lock_unpoisoned(&state.last_window_groups) = groups.clone();
    groups
}

#[tauri::command]
fn get_icons(state: State<AppState>, keys: Vec<String>) -> HashMap<String, String> {
    let mut resolved = lock_unpoisoned(&state.icon_cache).get_batch(&keys);
    let missing: Vec<(String, String)> = {
        let sources = lock_unpoisoned(&state.icon_sources);
        keys.iter()
            .filter(|k| !k.is_empty() && !resolved.contains_key(*k))
            .filter_map(|k| sources.get(k).map(|src| (k.clone(), src.clone())))
            .collect()
    };
    for (key, src) in missing {
        if let Some(uri) = windows_tracker::extract_icon_for_path_cached(&src, &state.icon_cache) {
            resolved.insert(key, uri);
        }
    }
    resolved
}

#[tauri::command]
fn resolve_pin_icons(state: State<AppState>, targets: Vec<String>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for raw in targets {
        let trimmed = raw.trim();
        if trimmed.is_empty() || !seen.insert(trimmed.to_string()) {
            continue;
        }
        let low = trimmed.to_ascii_lowercase();
        let is_local_or_uwp =
            trimmed.contains(":\\") || trimmed.starts_with("\\\\") || low.starts_with("shell:appsfolder\\");
        if !is_local_or_uwp {
            continue;
        }
        if let Some(uri) = windows_tracker::extract_icon_for_path_cached(trimmed, &state.icon_cache) {
            let key = if low.starts_with("shell:appsfolder\\") {
                icon_cache::aumid_key(trimmed)
            } else {
                icon_cache::path_icon_key(trimmed)
            };
            lock_unpoisoned(&state.icon_sources).insert(key, trimmed.to_string());
            out.insert(trimmed.to_string(), uri);
        }
    }
    out
}

#[tauri::command]
fn get_jump_list(exe_path: String, aumid: Option<String>) -> Vec<JumpListItem> {
    windows_tracker::get_jump_list(&exe_path, aumid.as_deref())
}

#[tauri::command]
fn get_window_thumbnail(hwnd: isize) -> Option<String> {
    windows_tracker::capture_window_thumbnail(hwnd, 240, 140)
}

#[tauri::command]
fn focus_window(app: AppHandle, hwnd: isize, toggle_minimize: bool) {
    close_start_and_flyout(&app, false, true);
    windows_tracker::focus_or_toggle_window(hwnd, toggle_minimize);
}

#[tauri::command]
fn close_window_hwnd(hwnd: isize) {
    windows_tracker::close_window(hwnd);
}

#[tauri::command]
fn kill_process_pid(pid: u32) -> Result<(), String> {
    windows_tracker::terminate_process_by_pid(pid)
}

#[tauri::command]
fn launch_item(
    app: AppHandle,
    target: String,
    args: Option<String>,
    as_admin: bool,
    close_start: bool,
) -> Result<(), String> {
    if close_start {
        close_start_and_flyout(&app, true, true);
    }
    windows_tracker::launch_target_with_args(&target, args.as_deref(), as_admin)
}

#[tauri::command]
fn launch_combo(app: AppHandle, targets: Vec<String>) -> Result<(), String> {
    close_start_and_flyout(&app, true, false);
    for t in targets {
        if !t.trim().is_empty() {
            let _ = windows_tracker::launch_target(&t, false);
        }
    }
    Ok(())
}

#[tauri::command]
fn open_location(path: String) -> Result<(), String> {
    windows_tracker::open_file_location(&path)
}

#[tauri::command]
fn open_terminal_at(path: String) -> Result<(), String> {
    windows_tracker::open_in_terminal(&path)
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
fn add_pin(
    app: AppHandle,
    state: State<AppState>,
    space: String,
    mut item: PinItem,
) -> Result<TestBarConfig, String> {
    let force_combo = config::should_force_combo(&space, &item.kind);
    config::normalize_pin(&mut item, force_combo);
    if item.icon_data.is_none() && (item.kind == "app" || item.kind == "file") {
        item.icon_data =
            windows_tracker::extract_icon_for_path_cached(&item.target, &state.icon_cache);
    }
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        config::add_pin_to_space(cfg, &space, item)
    })?;
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
    let hex = accent_hex
        .map(str::trim)
        .filter(|h| h.len() == 7 && h.starts_with('#') && h[1..].chars().all(|c| c.is_ascii_hexdigit()))
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

#[tauri::command]
fn add_custom_space(
    app: AppHandle,
    state: State<AppState>,
    name: String,
    accent_hex: Option<String>,
    accent_rgb: Option<String>,
) -> Result<TestBarConfig, String> {
    let clean = name.trim().to_string();
    if clean.is_empty() {
        return Err("Пустое название".to_string());
    }
    let (hex, rgb) = sanitize_custom_space_accent(accent_hex.as_deref(), accent_rgb.as_deref());
    let id = format!("custom-{}", now_ms());
    let (updated, _) = mutate_config_and_emit(&app, &state, |cfg| {
        cfg.custom_spaces.push(CustomSpace {
            id,
            name: clean,
            accent_hex: hex,
            accent_rgb: rgb,
            items: vec![],
        });
        true
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
fn pick_folder(app: AppHandle, state: State<AppState>) -> Option<String> {
    let _modal_guard = ModalPickerGuard::acquire(&state.coordinator);
    let picked = apps_indexer::pick_folder_dialog();
    refocus_start_if_open(&app, &state);
    picked
}

#[tauri::command]
fn pick_file(app: AppHandle, state: State<AppState>) -> Option<String> {
    let _modal_guard = ModalPickerGuard::acquire(&state.coordinator);
    let picked = apps_indexer::pick_file_dialog();
    refocus_start_if_open(&app, &state);
    picked
}

#[tauri::command]
fn get_audio_state() -> AudioState {
    audio::get_audio_state()
}

#[tauri::command]
fn get_audio_master_state() -> AudioMasterState {
    audio::get_audio_master_state()
}

#[tauri::command]
fn set_master_volume(
    app: AppHandle,
    volume: u32,
    muted: Option<bool>,
) -> Result<AudioState, String> {
    audio::set_master_volume(volume, muted)?;
    let st = audio::get_audio_state();
    emit_audio_state_targeted(&app, &st);
    Ok(st)
}

#[tauri::command]
fn set_mic_muted(app: AppHandle, muted: bool) -> Result<AudioState, String> {
    audio::set_mic_muted(muted)?;
    let st = audio::get_audio_state();
    emit_audio_state_targeted(&app, &st);
    Ok(st)
}

#[tauri::command]
fn set_session_volume(
    app: AppHandle,
    pid: u32,
    pids: Option<Vec<u32>>,
    app_key: Option<String>,
    volume: u32,
    muted: Option<bool>,
) -> Result<AudioState, String> {
    audio::set_session_volume_for_app(
        pid,
        pids.as_deref(),
        app_key.as_deref(),
        volume,
        muted,
    )?;
    let st = audio::get_audio_state();
    emit_audio_state_targeted(&app, &st);
    Ok(st)
}

#[tauri::command]
fn set_audio_device(app: AppHandle, device_id: String) -> Result<AudioState, String> {
    if let Err(err) = audio::set_default_audio_device(&device_id) {
        let _ = system_widgets::push_session_alert(
            "warn",
            "Ошибка переключения звука",
            Some(err.clone()),
        );
        emit_session_alerts_if_calendar_open(&app);
        return Err(err);
    }
    let st = audio::get_audio_state();
    emit_audio_state_targeted(&app, &st);
    Ok(st)
}

#[tauri::command]
fn get_media_state() -> MediaState {
    media::get_media_state()
}

#[tauri::command]
fn media_action(app: AppHandle, action: String) -> Result<MediaState, String> {
    media::media_command(&action)?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    let st = media::get_media_state();
    let _ = app.emit("media-updated", &st);
    Ok(st)
}

#[tauri::command]
fn get_telemetry() -> TelemetryState {
    telemetry::get_telemetry()
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
    let _ = app.emit("clipboard-updated", &list);
    list
}

#[tauri::command]
fn clear_unpinned_clips(app: AppHandle) -> Vec<ClipboardEntry> {
    let list = system_widgets::clear_unpinned_clipboard();
    let _ = app.emit("clipboard-updated", &list);
    list
}

#[tauri::command]
fn get_keyboard_layout() -> String {
    system_widgets::get_keyboard_layout_code()
}

#[tauri::command]
fn switch_keyboard_layout(app: AppHandle) -> String {
    let code = system_widgets::switch_keyboard_layout();
    let _ = app.emit("layout-updated", &code);
    code
}

#[tauri::command]
fn get_peripherals() -> Vec<PeripheralBattery> {
    system_widgets::get_peripheral_batteries()
}

#[tauri::command]
fn get_tray_apps(state: State<AppState>) -> Vec<TrayAppItem> {
    let open_groups = windows_tracker::list_open_window_groups_cached(&state.icon_cache);
    *lock_unpoisoned(&state.last_window_groups) = open_groups.clone();
    system_widgets::list_tray_background_apps_cached(&state.icon_cache, &open_groups)
}

#[tauri::command]
fn activate_tray_app(
    app: AppHandle,
    hwnd: Option<isize>,
    exe_path: String,
) -> Result<(), String> {
    close_start_and_flyout(&app, false, true);
    system_widgets::activate_tray_app(hwnd, &exe_path)
}

#[tauri::command]
fn end_tray_app_task(pids: Vec<u32>) -> Result<(), String> {
    system_widgets::end_tray_app_task(&pids)
}

#[tauri::command]
fn open_native_windows_tray(app: AppHandle) -> Result<(), String> {
    close_start_and_flyout(&app, false, true);
    system_widgets::trigger_native_tray_overflow()
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
fn open_windows_notifications(app: AppHandle) -> Result<(), String> {
    close_start_and_flyout(&app, false, true);
    system_widgets::open_windows_notifications()
}

#[tauri::command]
fn get_network_info() -> NetworkInfo {
    system_widgets::get_network_info()
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
fn get_icon_for_path(state: State<AppState>, path: String) -> Option<String> {
    windows_tracker::extract_icon_for_path_cached(&path, &state.icon_cache)
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
    win_shell::set_keep_native_secondary_taskbar(
        win_shell::secondary_native_taskbar_should_stay_visible(&initial_cfg.secondary_taskbar_mode),
    );

    let overlay_open = Arc::new(AtomicBool::new(false));
    let fullscreen_suppressed = Arc::new(AtomicBool::new(false));
    let secondary_taskbar_visible = Arc::new(AtomicBool::new(false));
    let flyout_anim_gen = Arc::new(AtomicU64::new(0));
    let start_anim_gen = Arc::new(AtomicU64::new(0));
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
        taskbar_content_width: Mutex::new(620),
        secondary_taskbar_content_width: Mutex::new(620),
        secondary_taskbar_visible: Arc::clone(&secondary_taskbar_visible),
        last_flyout_req: Mutex::new(None),
        flyout_anim_gen: Arc::clone(&flyout_anim_gen),
        start_anim_gen: Arc::clone(&start_anim_gen),
        dpi_override: AtomicU32::new(0),
    };

    let app = tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_bar_mode,
            set_secondary_taskbar_mode,
            set_autostart,
            resize_taskbar_dock,
            toggle_start,
            open_start_settings,
            open_flyout,
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
            kill_process_pid,
            launch_item,
            launch_combo,
            open_location,
            open_terminal_at,
            get_installed_apps,
            get_indexed_files,
            get_steam_games,
            add_pin,
            remove_pin,
            reorder_pins,
            add_custom_space,
            pick_folder,
            pick_file,
            get_audio_state,
            get_audio_master_state,
            set_master_volume,
            set_mic_muted,
            set_session_volume,
            set_audio_device,
            get_media_state,
            media_action,
            get_telemetry,
            get_clipboard_history,
            copy_to_clipboard,
            toggle_clip_pin,
            clear_unpinned_clips,
            get_keyboard_layout,
            switch_keyboard_layout,
            get_peripherals,
            get_tray_apps,
            activate_tray_app,
            end_tray_app_task,
            open_native_windows_tray,
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
                                        sync_secondary_taskbar(&h);
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

            // Live Clipboard History listener
            let h_clip = handle.clone();
            system_widgets::start_clipboard_listener(move |entries| {
                let _ = h_clip.emit("clipboard-updated", &entries);
            });

            // Async background app (Start .lnk + UWP), Steam library & user files indexer (non-blocking startup)
            let h_idx = handle.clone();
            std::thread::spawn(move || {
                let apps = {
                    let st = h_idx.state::<AppState>();
                    apps_indexer::scan_installed_apps_cached(&st.icon_cache, Some(&st.icon_sources))
                };
                {
                    let st = h_idx.state::<AppState>();
                    *lock_unpoisoned(&st.installed_apps) = apps.clone();
                }
                let _ = h_idx.emit("apps-indexed", &apps);

                let games = apps_indexer::scan_steam_games();
                {
                    let st = h_idx.state::<AppState>();
                    *lock_unpoisoned(&st.steam_games) = games.clone();
                }
                let _ = h_idx.emit("games-indexed", &games);

                let files = apps_indexer::scan_user_files();
                {
                    let st = h_idx.state::<AppState>();
                    *lock_unpoisoned(&st.indexed_files) = files.clone();
                }
                let _ = h_idx.emit("files-indexed", &files);
            });

            // Fast lightweight poller for open windows (diff-only visible-window broadcast),
            // keyboard layout, native taskbar hide guard, and fullscreen auto-hide.
            let h_fast = handle.clone();
            let fast_fs_suppressed = Arc::clone(&fullscreen_suppressed);
            std::thread::spawn(move || {
                let mut last_layout = String::new();
                let mut last_windows_fp: Option<WindowsFingerprint> = None;
                loop {
                    let groups = {
                        let st = h_fast.state::<AppState>();
                        windows_tracker::list_open_window_groups_cached(&st.icon_cache)
                    };
                    let targets = {
                        let st = h_fast.state::<AppState>();
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

                    let fs = win_shell::is_fullscreen_active();
                    if fs != fast_fs_suppressed.load(Ordering::Acquire) {
                        let h = h_fast.clone();
                        let _ = h_fast.run_on_main_thread(move || {
                            let _ = reevaluate_fullscreen(&h, None);
                        });
                    }

                    std::thread::sleep(std::time::Duration::from_millis(650));
                }
            });

            // Slower poller for split audio (diff-gated, visible-surface targeted),
            // media, telemetry, and Bluetooth peripherals
            let h_slow = handle.clone();
            std::thread::spawn(move || {
                let mut tick = 0u64;
                let mut last_audio_master: Option<AudioMasterState> = None;
                let mut last_audio_mixer: Option<AudioState> = None;
                loop {
                    let mixer_needed = !lock_unpoisoned(&h_slow.state::<AppState>().coordinator)
                        .audio_mixer_targets()
                        .is_empty();

                    if mixer_needed {
                        let audio_st = audio::get_audio_state();
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

                    let media_st = media::get_media_state();
                    let _ = h_slow.emit("media-updated", &media_st);

                    if tick % 2 == 0 {
                        let telem = telemetry::get_telemetry();
                        let _ = h_slow.emit("telemetry-updated", &telem);
                    }

                    if tick % 10 == 0 {
                        let periphs = system_widgets::get_peripheral_batteries();
                        let _ = h_slow.emit("peripherals-updated", &periphs);
                        if hide_native && !win_shell::is_restore_in_progress() {
                            win_shell::hide_native_taskbar();
                        }
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
            taskbar_content_width: Mutex::new(width),
            secondary_taskbar_content_width: Mutex::new(width),
            secondary_taskbar_visible: Arc::new(AtomicBool::new(false)),
            last_flyout_req: Mutex::new(None),
            flyout_anim_gen: Arc::new(AtomicU64::new(0)),
            start_anim_gen: Arc::new(AtomicU64::new(0)),
            dpi_override: AtomicU32::new(0),
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

        // Start bounds at 1.0x on 1920x1080 -> full 1172x712 logical
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
        assert_eq!(start_100.width(), 1172);
        assert_eq!(start_100.height(), 712);
        assert!(start_100.left >= mon_1080.left && start_100.right <= mon_1080.right);
        assert!(start_100.top >= mon_1080.top && start_100.bottom <= mon_1080.bottom);

        // Start bounds at 1.4x on 2560x1440 -> full 1172x712 scaled to 1641x997
        let start_1440_140 = compute_start_bounds(mon_1440, 1.4);
        assert_eq!(start_1440_140.width(), (1172.0_f64 * 1.4).round() as i32);
        assert_eq!(start_1440_140.height(), (712.0_f64 * 1.4).round() as i32);
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
            devices: vec![audio::AudioDeviceInfo {
                id: "d1".into(),
                name: "Speakers (Realtek)".into(),
                short_name: "Speakers".into(),
                is_default: true,
            }],
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
}
