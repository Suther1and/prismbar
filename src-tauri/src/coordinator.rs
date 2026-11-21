use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Minimum delay in milliseconds after a focus-loss or click-outside dismissal
/// before the same trigger can re-open the surface (prevents double-toggle).
pub const REOPEN_DEBOUNCE_MS: u64 = 180;

/// Grace window in milliseconds immediately after opening Start during which
/// transient WebView2/Win-key `Focused(false)` jitter does not dismiss Start.
pub const START_OPEN_FOCUS_GRACE_MS: u64 = 200;

/// Grace window in milliseconds immediately after opening an interactive click flyout
/// during which transient WebView2 `Focused(false)` jitter does not dismiss the flyout.
pub const FLYOUT_OPEN_FOCUS_GRACE_MS: u64 = 160;

/// Non-clipped logical shadow root padding for Start and Flyout transparent WebView2 windows (AD-1).
pub const START_ROOT_PAD_LOGICAL: f64 = 24.0;
pub const FLYOUT_ROOT_PAD_LOGICAL: f64 = 16.0;

/// Win32 `WS_CAPTION` (`WS_BORDER | WS_DLGFRAME`) bitmask.
pub const WS_CAPTION_FLAG: u32 = 0x00C0_0000;
/// Win32 `WS_THICKFRAME` bitmask.
pub const WS_THICKFRAME_FLAG: u32 = 0x0004_0000;

/// Logical reserved AppBar heights (AD-2): 58px taskbar + 9px bottom gap in Dock mode, 58px in Edge mode.
pub const DOCK_APPBAR_LOGICAL_HEIGHT: i32 = 67;
pub const EDGE_APPBAR_LOGICAL_HEIGHT: i32 = 58;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlyoutKind {
    Preview,
    Tray,
    #[serde(alias = "quick")]
    Sound,
    Calendar,
    Weather,
    Battery,
    Network,
    Telemetry,
    #[serde(alias = "ctx_app")]
    ContextApp,
    #[serde(alias = "ctx_taskbar")]
    ContextBar,
    Osd,
}

impl FlyoutKind {
    pub fn is_non_activating(self) -> bool {
        matches!(self, FlyoutKind::Preview | FlyoutKind::Osd)
    }

    pub fn is_interactive_click(self) -> bool {
        !self.is_non_activating()
    }

    pub fn toggles_on_reclick(self) -> bool {
        matches!(
            self,
            FlyoutKind::Tray
                | FlyoutKind::Sound
                | FlyoutKind::Calendar
                | FlyoutKind::Weather
                | FlyoutKind::Battery
                | FlyoutKind::Network
                | FlyoutKind::Telemetry
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerId {
    Start,
    Flyout(FlyoutKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarMode {
    Dock,
    Edge,
}

impl BarMode {
    pub fn from_config_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("edge") {
            Self::Edge
        } else {
            Self::Dock
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationState {
    NotPresent,
    Busy,
    RunningD3dFullScreen,
    PresentationMode,
    AcceptsNotifications,
    QuietTime,
    App,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    pub fn covers(self, other: PixelRect) -> bool {
        other.width() > 0
            && other.height() > 0
            && self.left <= other.left
            && self.top <= other.top
            && self.right >= other.right
            && self.bottom >= other.bottom
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundWindowInfo {
    pub visible: bool,
    pub is_own_process: bool,
    pub class_name: String,
    pub style: u32,
    pub rect: PixelRect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailedForegroundWindow {
    pub visible: bool,
    pub is_own_process: bool,
    pub is_maximized: bool,
    pub class_name: String,
    pub style: u32,
    pub window_rect: PixelRect,
    pub extended_rect: PixelRect,
    pub monitor_rect: PixelRect,
    pub work_area_rect: PixelRect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MonitorFullscreenState {
    pub primary: bool,
    pub secondary: bool,
}

fn is_shell_desktop_class(class_name: &str) -> bool {
    matches!(
        class_name,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "MultitaskingViewFrame"
    )
}

/// Single authoritative fullscreen classifier (AD-6).
///
/// Returns `true` ONLY when:
/// - `SHQueryUserNotificationState` reports `QUNS_RUNNING_D3D_FULL_SCREEN` or `QUNS_PRESENTATION_MODE`, OR
/// - The visible foreground window belongs to another non-shell process and covers the entire primary monitor,
///   either without full `WS_CAPTION`/`WS_THICKFRAME` OR as a UWP `ApplicationFrameWindow` covering past the AppBar.
pub fn classify_fullscreen(
    notification_state: NotificationState,
    foreground: Option<&ForegroundWindowInfo>,
    primary_monitor: PixelRect,
) -> bool {
    if matches!(
        notification_state,
        NotificationState::RunningD3dFullScreen | NotificationState::PresentationMode
    ) {
        return true;
    }

    let Some(fg) = foreground else {
        return false;
    };
    if !fg.visible || fg.is_own_process {
        return false;
    }
    if is_shell_desktop_class(&fg.class_name) {
        return false;
    }

    if !fg.rect.covers(primary_monitor) {
        return false;
    }

    // Packaged UWP apps hosted in ApplicationFrameWindow (Netflix, Photos, Movies & TV,
    // Windows Terminal F11) keep WS_CAPTION | WS_THICKFRAME even in true fullscreen;
    // when maximized normally their DWM extended rect stops above the AppBar.
    if fg.class_name == "ApplicationFrameWindow" {
        return true;
    }

    let has_caption = (fg.style & WS_CAPTION_FLAG) == WS_CAPTION_FLAG;
    let has_thickframe = (fg.style & WS_THICKFRAME_FLAG) != 0;
    if has_caption || has_thickframe {
        return false;
    }

    true
}

/// Per-monitor authoritative fullscreen classifier using `DWMWA_EXTENDED_FRAME_BOUNDS` and
/// `MonitorFromWindow` attribution so:
/// - `QUNS_RUNNING_D3D_FULL_SCREEN` / `QUNS_PRESENTATION_MODE` only suppresses the monitor
///   actually hosting the foreground window (never hiding Monitor 1's taskbar when a game is on Monitor 2).
/// - UWP `ApplicationFrameWindow` and borderless engines retaining `WS_CAPTION`/`WS_THICKFRAME`
///   are detected accurately without false-triggering on maximized desktop apps (even on Monitor 2
///   where `work_area_rect == monitor_rect`).
pub fn classify_fullscreen_for_monitor(
    notification_state: NotificationState,
    foreground: Option<&DetailedForegroundWindow>,
    target_monitor: PixelRect,
    is_primary_target: bool,
) -> bool {
    if target_monitor.width() <= 0 || target_monitor.height() <= 0 {
        return false;
    }

    if matches!(
        notification_state,
        NotificationState::RunningD3dFullScreen | NotificationState::PresentationMode
    ) {
        return match foreground {
            Some(fg) if !fg.is_own_process && fg.monitor_rect.width() > 0 => {
                fg.monitor_rect == target_monitor || fg.extended_rect.covers(target_monitor)
            }
            _ => is_primary_target,
        };
    }

    let Some(fg) = foreground else {
        return false;
    };
    if !fg.visible || fg.is_own_process {
        return false;
    }
    if is_shell_desktop_class(&fg.class_name) {
        return false;
    }

    let covers_target = fg.extended_rect.covers(target_monitor);
    if !covers_target {
        return false;
    }

    let has_caption = (fg.style & WS_CAPTION_FLAG) == WS_CAPTION_FLAG;
    let has_thickframe = (fg.style & WS_THICKFRAME_FLAG) != 0;
    if !has_caption && !has_thickframe {
        return true;
    }

    let has_reserved_appbar = fg.work_area_rect.width() > 0
        && fg.work_area_rect.height() > 0
        && fg.work_area_rect != target_monitor;

    // UWP ApplicationFrameWindow in fullscreen covers past reserved work area (primary) or runs un-maximized full-monitor (secondary).
    if fg.class_name == "ApplicationFrameWindow" {
        return !fg.is_maximized || has_reserved_appbar;
    }

    // Borderless games/engines that retain WS_CAPTION/WS_THICKFRAME while placing an un-maximized
    // frame over the entire monitor (including the reserved AppBar strip).
    !fg.is_maximized && has_reserved_appbar && fg.extended_rect == target_monitor
}

/// Computes effective WebView2 / UI scale factor combining monitor DPI scale and
/// Windows Accessibility `TextScaleFactor` (`effective_scale = (dpi / 96.0) * text_scale`).
pub fn compute_effective_scale(dpi: u32, text_scale: f64) -> f64 {
    let dpi_scale = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };
    let ts = if text_scale.is_finite() && text_scale > 0.0 {
        text_scale.clamp(1.0, 2.25)
    } else {
        1.0
    };
    dpi_scale * ts
}

/// Computes the scaled bottom AppBar reservation rect in physical pixels (AD-2)
/// using `effective_scale = (dpi / 96.0) * (text_scale_factor / 100.0)`.
pub fn compute_appbar_rect_scaled(
    primary_monitor: PixelRect,
    effective_scale: f64,
    bar_mode: BarMode,
) -> PixelRect {
    let sf = if effective_scale.is_finite() && effective_scale > 0.0 {
        effective_scale
    } else {
        1.0
    };
    let logical_height = match bar_mode {
        BarMode::Dock => DOCK_APPBAR_LOGICAL_HEIGHT,
        BarMode::Edge => EDGE_APPBAR_LOGICAL_HEIGHT,
    };
    let reserve_px = (((logical_height as f64) * sf).round() as i32)
        .clamp(0, primary_monitor.height());
    PixelRect {
        left: primary_monitor.left,
        top: primary_monitor.bottom - reserve_px,
        right: primary_monitor.right,
        bottom: primary_monitor.bottom,
    }
}

/// Computes the DPI-scaled bottom AppBar reservation rect in physical pixels (AD-2)
/// assuming default 100% `TextScaleFactor`.
pub fn compute_appbar_rect(primary_monitor: PixelRect, dpi: u32, bar_mode: BarMode) -> PixelRect {
    compute_appbar_rect_scaled(primary_monitor, compute_effective_scale(dpi, 1.0), bar_mode)
}

/// Returns the WebView window labels that should receive `windows-updated` broadcasts (AD-6):
/// always `"taskbar"`, plus `"start"` only when visible and `"flyout"` only when visible.
pub fn windows_update_targets(start_visible: bool, flyout_visible: bool) -> Vec<&'static str> {
    windows_update_targets_with_secondary(start_visible, flyout_visible, false)
}

/// Returns the WebView window labels that should receive `windows-updated` broadcasts (AD-6),
/// including `"taskbar-secondary"` only when the secondary monitor taskbar is currently visible.
pub fn windows_update_targets_with_secondary(
    start_visible: bool,
    flyout_visible: bool,
    secondary_taskbar_visible: bool,
) -> Vec<&'static str> {
    let mut targets = Vec::with_capacity(4);
    targets.push("taskbar");
    if secondary_taskbar_visible {
        targets.push("taskbar-secondary");
    }
    if start_visible {
        targets.push("start");
    }
    if flyout_visible {
        targets.push("flyout");
    }
    targets
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoordinatorActions {
    pub taskbar_visible: Option<bool>,
    pub secondary_taskbar_visible: Option<bool>,
    pub start_visible: Option<bool>,
    pub flyout_visible: Option<bool>,
    pub flyout_non_activating: Option<bool>,
    pub emit_flyout_show: bool,
    pub emit_flyout_hide: bool,
    pub emit_start_state: Option<bool>,
    pub hide_flyout_immediate: bool,
    pub hide_start_immediate: bool,
    pub start_anim_gen: Option<u64>,
    pub flyout_anim_gen: Option<u64>,
    pub start_target_monitor: Option<String>,
}

/// Pure, Win32-independent state machine owning lifecycle, focus, preemption,
/// anti-double-toggle debounce, modal picker guard, per-monitor fullscreen suppression,
/// and monotonic animation generations for the pre-warmed TestBar windows.
#[derive(Debug, Default)]
pub struct WindowCoordinator {
    start_open: bool,
    active_flyout: Option<FlyoutKind>,
    fullscreen_suppressed: bool,
    primary_fullscreen: bool,
    secondary_fullscreen: bool,
    fullscreen_overlay_active: bool,
    secondary_fullscreen_overlay_active: bool,
    modal_picker_depth: u32,
    last_dismissed_ms: HashMap<TriggerId, u64>,
    last_start_opened_ms: Option<u64>,
    last_flyout_opened_ms: Option<u64>,
    start_anim_gen: u64,
    flyout_anim_gen: u64,
    start_target_monitor: Option<String>,
}

impl WindowCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_start_open(&self) -> bool {
        self.start_open
    }

    pub fn is_start_visible(&self) -> bool {
        self.start_open
    }

    pub fn is_flyout_visible(&self) -> bool {
        self.active_flyout.is_some()
    }

    pub fn start_anim_gen(&self) -> u64 {
        self.start_anim_gen
    }

    pub fn flyout_anim_gen(&self) -> u64 {
        self.flyout_anim_gen
    }

    pub fn start_target_monitor(&self) -> Option<&str> {
        self.start_target_monitor.as_deref()
    }

    pub fn windows_update_targets(&self) -> Vec<&'static str> {
        windows_update_targets(self.is_start_visible(), self.is_flyout_visible())
    }

    pub fn windows_update_targets_with_secondary(
        &self,
        secondary_taskbar_visible: bool,
    ) -> Vec<&'static str> {
        windows_update_targets_with_secondary(
            self.is_start_visible(),
            self.is_flyout_visible(),
            secondary_taskbar_visible,
        )
    }

    /// Returns visible WebView labels that should receive lightweight `audio-master-updated`
    /// events (AD-6): always `"taskbar"`, `"taskbar-secondary"` when visible, `"start"` when
    /// visible, and `"flyout"` only when showing `Sound` or `Osd`.
    pub fn audio_master_targets_with_secondary(
        &self,
        secondary_taskbar_visible: bool,
    ) -> Vec<&'static str> {
        let mut targets = Vec::with_capacity(4);
        targets.push("taskbar");
        if secondary_taskbar_visible {
            targets.push("taskbar-secondary");
        }
        if self.is_start_visible() {
            targets.push("start");
        }
        if matches!(
            self.active_flyout,
            Some(FlyoutKind::Sound | FlyoutKind::Osd)
        ) {
            targets.push("flyout");
        }
        targets
    }

    /// Returns visible WebView labels that should receive full `audio-mixer-updated`
    /// payloads with devices & per-app sessions (AD-6): ONLY `"start"` when visible and
    /// `"flyout"` when showing `FlyoutKind::Sound`.
    pub fn audio_mixer_targets(&self) -> Vec<&'static str> {
        let mut targets = Vec::with_capacity(2);
        if self.is_start_visible() {
            targets.push("start");
        }
        if self.active_flyout == Some(FlyoutKind::Sound) {
            targets.push("flyout");
        }
        targets
    }

    pub fn active_flyout(&self) -> Option<FlyoutKind> {
        self.active_flyout
    }

    pub fn has_open_overlay(&self) -> bool {
        self.start_open || self.active_flyout.is_some()
    }

    pub fn is_fullscreen_suppressed(&self) -> bool {
        self.fullscreen_suppressed
    }

    pub fn is_primary_fullscreen(&self) -> bool {
        self.primary_fullscreen
    }

    pub fn is_secondary_fullscreen(&self) -> bool {
        self.secondary_fullscreen
    }

    pub fn is_fullscreen_overlay_active(&self) -> bool {
        self.fullscreen_overlay_active
    }

    pub fn is_secondary_fullscreen_overlay_active(&self) -> bool {
        self.secondary_fullscreen_overlay_active
    }

    pub fn should_show_taskbar(&self) -> bool {
        !self.primary_fullscreen || self.fullscreen_overlay_active
    }

    pub fn should_show_secondary_taskbar(&self) -> bool {
        !self.secondary_fullscreen || self.secondary_fullscreen_overlay_active
    }

    /// Returns `true` only when `taskbar` bounds updates (`SetWindowPos`) may be applied.
    /// While fullscreen suppression is active without an active fullscreen Start/Flyout overlay,
    /// callers must record logical state without touching Win32 window position, Z-order, or visibility.
    pub fn should_apply_taskbar_bounds(&self) -> bool {
        !self.primary_fullscreen || self.fullscreen_overlay_active
    }

    pub fn should_apply_secondary_taskbar_bounds(&self) -> bool {
        !self.secondary_fullscreen || self.secondary_fullscreen_overlay_active
    }

    pub fn is_modal_picker_active(&self) -> bool {
        self.modal_picker_depth > 0
    }

    pub fn begin_modal_picker(&mut self) {
        self.modal_picker_depth = self.modal_picker_depth.saturating_add(1);
    }

    pub fn end_modal_picker(&mut self) {
        self.modal_picker_depth = self.modal_picker_depth.saturating_sub(1);
    }

    fn next_start_gen(&mut self) -> u64 {
        self.start_anim_gen = self.start_anim_gen.wrapping_add(1);
        self.start_anim_gen
    }

    fn next_flyout_gen(&mut self) -> u64 {
        self.flyout_anim_gen = self.flyout_anim_gen.wrapping_add(1);
        self.flyout_anim_gen
    }

    fn is_debounced(&self, trigger: TriggerId, now_ms: u64) -> bool {
        self.last_dismissed_ms
            .get(&trigger)
            .map(|&dismissed_at| now_ms >= dismissed_at && (now_ms - dismissed_at) < REOPEN_DEBOUNCE_MS)
            .unwrap_or(false)
    }

    fn close_start(&mut self, actions: &mut CoordinatorActions) {
        if self.start_open {
            self.start_open = false;
            self.last_start_opened_ms = None;
            let gen = self.next_start_gen();
            actions.start_visible = Some(false);
            actions.emit_start_state = Some(false);
            actions.start_anim_gen = Some(gen);
        }
    }

    fn close_flyout(&mut self, actions: &mut CoordinatorActions) -> Option<FlyoutKind> {
        let closed = self.active_flyout.take()?;
        self.last_flyout_opened_ms = None;
        let gen = self.next_flyout_gen();
        actions.flyout_visible = Some(false);
        actions.emit_flyout_hide = true;
        actions.flyout_anim_gen = Some(gen);
        Some(closed)
    }

    /// Synchronously hides temporarily revealed fullscreen taskbar(s) when the last overlay
    /// (Start or a Flyout opened from that revealed taskbar) closes.
    fn reconcile_fullscreen_overlay_after_close(&mut self, actions: &mut CoordinatorActions) {
        if !self.start_open && self.active_flyout.is_none() {
            if self.fullscreen_overlay_active {
                self.fullscreen_overlay_active = false;
                if self.primary_fullscreen {
                    actions.taskbar_visible = Some(false);
                }
            }
            if self.secondary_fullscreen_overlay_active {
                self.secondary_fullscreen_overlay_active = false;
                if self.secondary_fullscreen {
                    actions.secondary_taskbar_visible = Some(false);
                }
            }
        }
    }

    /// Immediately cancels any active or exiting flyout animation and instructs the host to `SW_HIDE`.
    pub fn hide_flyout_immediate(&mut self) -> CoordinatorActions {
        let mut actions = CoordinatorActions::default();
        let had_active = self.close_flyout(&mut actions).is_some();
        if !had_active {
            let gen = self.next_flyout_gen();
            actions.flyout_anim_gen = Some(gen);
        }
        actions.hide_flyout_immediate = true;
        self.reconcile_fullscreen_overlay_after_close(&mut actions);
        actions
    }

    /// Immediately cancels any active or exiting Start animation and instructs the host to `SW_HIDE`.
    pub fn hide_start_immediate(&mut self) -> CoordinatorActions {
        let mut actions = CoordinatorActions::default();
        let had_active = self.start_open;
        self.close_start(&mut actions);
        if !had_active {
            let gen = self.next_start_gen();
            actions.start_anim_gen = Some(gen);
        }
        actions.hide_start_immediate = true;
        self.reconcile_fullscreen_overlay_after_close(&mut actions);
        actions
    }

    /// Toggles Start Menu (explicitly overriding `fullscreen_suppressed` so pressing `Win`
    /// inside fullscreen games/video atomically reveals Start + Taskbar together).
    pub fn request_toggle_start(&mut self, force: Option<bool>, now_ms: u64) -> CoordinatorActions {
        self.request_toggle_start_on_monitor(force, None, now_ms)
    }

    /// Toggles Start Menu targeting an optional monitor (`"primary"` | `"secondary"`),
    /// atomically revealing Start + that monitor's Taskbar together when fullscreen-suppressed
    /// and synchronously hiding both together on close.
    pub fn request_toggle_start_on_monitor(
        &mut self,
        force: Option<bool>,
        monitor: Option<String>,
        now_ms: u64,
    ) -> CoordinatorActions {
        let target = force.unwrap_or(!self.start_open);
        if target == self.start_open {
            return CoordinatorActions::default();
        }

        let mut actions = CoordinatorActions::default();
        if target {
            if force.is_none() && self.is_debounced(TriggerId::Start, now_ms) {
                return actions;
            }
            let had_flyout = self.close_flyout(&mut actions).is_some();
            if !had_flyout {
                // Bump flyout_anim_gen even if flyout was mid-exit so stale exit timers cancel immediately
                actions.flyout_anim_gen = Some(self.next_flyout_gen());
            }
            actions.hide_flyout_immediate = true;

            let target_secondary = monitor.as_deref() == Some("secondary");
            if !target_secondary {
                if self.primary_fullscreen && !self.fullscreen_overlay_active {
                    self.fullscreen_overlay_active = true;
                    actions.taskbar_visible = Some(true);
                }
                if self.secondary_fullscreen_overlay_active {
                    self.secondary_fullscreen_overlay_active = false;
                    if self.secondary_fullscreen {
                        actions.secondary_taskbar_visible = Some(false);
                    }
                }
            } else {
                if self.secondary_fullscreen && !self.secondary_fullscreen_overlay_active {
                    self.secondary_fullscreen_overlay_active = true;
                    actions.secondary_taskbar_visible = Some(true);
                }
                if self.fullscreen_overlay_active {
                    self.fullscreen_overlay_active = false;
                    if self.primary_fullscreen {
                        actions.taskbar_visible = Some(false);
                    }
                }
            }

            self.start_open = true;
            self.last_start_opened_ms = Some(now_ms);
            self.start_target_monitor = monitor;
            let gen = self.next_start_gen();
            actions.start_visible = Some(true);
            actions.emit_start_state = Some(true);
            actions.start_anim_gen = Some(gen);
            actions.start_target_monitor = self.start_target_monitor.clone();
        } else {
            self.close_start(&mut actions);
            self.reconcile_fullscreen_overlay_after_close(&mut actions);
        }
        actions
    }

    pub fn request_open_flyout(&mut self, kind: FlyoutKind, now_ms: u64) -> CoordinatorActions {
        self.request_open_flyout_on_monitor(kind, None, now_ms)
    }

    pub fn request_open_flyout_on_monitor(
        &mut self,
        kind: FlyoutKind,
        monitor: Option<&str>,
        now_ms: u64,
    ) -> CoordinatorActions {
        let monitor_suppressed = if monitor == Some("secondary") {
            self.secondary_fullscreen && !self.secondary_fullscreen_overlay_active
        } else {
            self.primary_fullscreen && !self.fullscreen_overlay_active
        };
        if monitor_suppressed {
            return CoordinatorActions::default();
        }

        let mut actions = CoordinatorActions::default();

        if kind.is_non_activating() {
            if let Some(active) = self.active_flyout {
                if active.is_interactive_click() {
                    return actions;
                }
            }
            if kind == FlyoutKind::Preview && self.start_open {
                return actions;
            }
        } else {
            if self.active_flyout == Some(kind) && kind.toggles_on_reclick() {
                let _ = self.close_flyout(&mut actions);
                self.reconcile_fullscreen_overlay_after_close(&mut actions);
                return actions;
            }
            if self.active_flyout.map_or(true, |a| a.is_non_activating())
                && self.is_debounced(TriggerId::Flyout(kind), now_ms)
            {
                return actions;
            }
            let had_start = self.start_open;
            self.close_start(&mut actions);
            if !had_start {
                actions.start_anim_gen = Some(self.next_start_gen());
            }
            actions.hide_start_immediate = true;
            self.last_flyout_opened_ms = Some(now_ms);
        }

        self.active_flyout = Some(kind);
        let gen = self.next_flyout_gen();
        actions.flyout_visible = Some(true);
        actions.flyout_non_activating = Some(kind.is_non_activating());
        actions.emit_flyout_show = true;
        actions.flyout_anim_gen = Some(gen);
        actions
    }

    pub fn request_close_flyout(&mut self) -> CoordinatorActions {
        let mut actions = CoordinatorActions::default();
        let _ = self.close_flyout(&mut actions);
        self.reconcile_fullscreen_overlay_after_close(&mut actions);
        actions
    }

    pub fn request_close_preview(&mut self) -> CoordinatorActions {
        if self.active_flyout == Some(FlyoutKind::Preview) {
            self.request_close_flyout()
        } else {
            CoordinatorActions::default()
        }
    }

    pub fn request_close_osd(&mut self) -> CoordinatorActions {
        if self.active_flyout == Some(FlyoutKind::Osd) {
            self.request_close_flyout()
        } else {
            CoordinatorActions::default()
        }
    }

    pub fn on_start_focus_lost(&mut self, now_ms: u64) -> CoordinatorActions {
        if !self.start_open || self.is_modal_picker_active() {
            return CoordinatorActions::default();
        }
        if let Some(opened_at) = self.last_start_opened_ms {
            if now_ms >= opened_at && (now_ms - opened_at) < START_OPEN_FOCUS_GRACE_MS {
                return CoordinatorActions::default();
            }
        }
        let mut actions = CoordinatorActions::default();
        self.close_start(&mut actions);
        self.last_dismissed_ms.insert(TriggerId::Start, now_ms);
        self.reconcile_fullscreen_overlay_after_close(&mut actions);
        actions
    }

    pub fn on_flyout_focus_lost(&mut self, now_ms: u64) -> CoordinatorActions {
        if !self.active_flyout.is_some_and(FlyoutKind::is_interactive_click) {
            return CoordinatorActions::default();
        }
        if let Some(opened_at) = self.last_flyout_opened_ms {
            if now_ms >= opened_at && (now_ms - opened_at) < FLYOUT_OPEN_FOCUS_GRACE_MS {
                return CoordinatorActions::default();
            }
        }
        let mut actions = CoordinatorActions::default();
        if let Some(kind) = self.close_flyout(&mut actions) {
            self.last_dismissed_ms
                .insert(TriggerId::Flyout(kind), now_ms);
        }
        self.reconcile_fullscreen_overlay_after_close(&mut actions);
        actions
    }

    pub fn on_global_mouse_down(
        &mut self,
        x: i32,
        y: i32,
        active_shell_rects: &[PixelRect],
        now_ms: u64,
    ) -> CoordinatorActions {
        if !self.start_open && self.active_flyout.is_none() {
            return CoordinatorActions::default();
        }
        if active_shell_rects.iter().any(|r| r.contains(x, y)) {
            return CoordinatorActions::default();
        }

        let mut actions = CoordinatorActions::default();
        if self.start_open && !self.is_modal_picker_active() {
            self.close_start(&mut actions);
            self.last_dismissed_ms.insert(TriggerId::Start, now_ms);
        }
        if let Some(kind) = self.close_flyout(&mut actions) {
            self.last_dismissed_ms
                .insert(TriggerId::Flyout(kind), now_ms);
        }
        self.reconcile_fullscreen_overlay_after_close(&mut actions);
        actions
    }

    /// Updates per-monitor fullscreen suppression independently for primary and secondary displays.
    /// Explicitly opened fullscreen overlays (`fullscreen_overlay_active`) are preserved rather
    /// than force-closed when fullscreen remains/re-asserts active.
    pub fn set_monitor_fullscreen(
        &mut self,
        primary: bool,
        secondary: bool,
        _now_ms: u64,
    ) -> CoordinatorActions {
        if primary == self.primary_fullscreen && secondary == self.secondary_fullscreen {
            return CoordinatorActions::default();
        }

        let mut actions = CoordinatorActions::default();

        if primary != self.primary_fullscreen {
            self.primary_fullscreen = primary;
            self.fullscreen_suppressed = primary;
            if primary {
                if !self.fullscreen_overlay_active {
                    actions.taskbar_visible = Some(false);
                    self.close_start(&mut actions);
                    let _ = self.close_flyout(&mut actions);
                    actions.hide_start_immediate = true;
                    if actions.start_anim_gen.is_none() {
                        actions.start_anim_gen = Some(self.next_start_gen());
                    }
                    actions.hide_flyout_immediate = true;
                    if actions.flyout_anim_gen.is_none() {
                        actions.flyout_anim_gen = Some(self.next_flyout_gen());
                    }
                }
            } else {
                let was_overlay = self.fullscreen_overlay_active;
                self.fullscreen_overlay_active = false;
                if !was_overlay {
                    actions.taskbar_visible = Some(true);
                }
            }
        }

        if secondary != self.secondary_fullscreen {
            self.secondary_fullscreen = secondary;
            if secondary {
                if !self.secondary_fullscreen_overlay_active {
                    actions.secondary_taskbar_visible = Some(false);
                    if self.start_target_monitor.as_deref() == Some("secondary") {
                        self.close_start(&mut actions);
                        actions.hide_start_immediate = true;
                        if actions.start_anim_gen.is_none() {
                            actions.start_anim_gen = Some(self.next_start_gen());
                        }
                    }
                }
            } else {
                let was_overlay = self.secondary_fullscreen_overlay_active;
                self.secondary_fullscreen_overlay_active = false;
                if !was_overlay {
                    actions.secondary_taskbar_visible = Some(true);
                }
            }
        }

        actions
    }

    /// Single-authority per-monitor fullscreen re-evaluation (AD-6).
    pub fn reevaluate_fullscreen_per_monitor(
        &mut self,
        state: MonitorFullscreenState,
        now_ms: u64,
    ) -> CoordinatorActions {
        self.set_monitor_fullscreen(state.primary, state.secondary, now_ms)
    }

    /// Single-authority per-monitor fullscreen re-evaluation that freezes underlying fullscreen
    /// state while TestBar itself owns foreground focus (`fg_is_own_process == true`, e.g. Start
    /// or Flyout open or mid-exit animation), eliminating 650ms poller desync over fullscreen games.
    pub fn reevaluate_fullscreen_per_monitor_with_fg(
        &mut self,
        state: MonitorFullscreenState,
        fg_is_own_process: bool,
        now_ms: u64,
    ) -> CoordinatorActions {
        if fg_is_own_process {
            return CoordinatorActions::default();
        }
        self.set_monitor_fullscreen(state.primary, state.secondary, now_ms)
    }

    pub fn set_fullscreen(&mut self, is_fullscreen: bool) -> CoordinatorActions {
        self.on_fullscreen_changed(is_fullscreen)
    }

    pub fn on_fullscreen_changed(&mut self, is_fullscreen: bool) -> CoordinatorActions {
        self.set_monitor_fullscreen(is_fullscreen, self.secondary_fullscreen, 0)
    }

    /// Single-authority fullscreen re-evaluation (AD-6).
    /// Always classifies via `classify_fullscreen` and transitions suppression state cleanly.
    pub fn reevaluate_fullscreen(
        &mut self,
        notification_state: NotificationState,
        foreground: Option<&ForegroundWindowInfo>,
        primary_monitor: PixelRect,
    ) -> CoordinatorActions {
        let is_fullscreen = classify_fullscreen(notification_state, foreground, primary_monitor);
        self.on_fullscreen_changed(is_fullscreen)
    }

    /// Handles `ABN_FULLSCREENAPP` hints strictly as an immediate trigger to re-evaluate
    /// `classify_fullscreen`, never trusting raw `lparam` (`_hint_entering`) on its own.
    pub fn on_appbar_fullscreen_hint(
        &mut self,
        _hint_entering: bool,
        notification_state: NotificationState,
        foreground: Option<&ForegroundWindowInfo>,
        primary_monitor: PixelRect,
    ) -> CoordinatorActions {
        self.reevaluate_fullscreen(notification_state, foreground, primary_monitor)
    }

    /// Bounds updates (`resize_taskbar_dock`, `set_bar_mode`) must never force
    /// `taskbar` visible when fullscreen suppression is active.
    pub fn on_taskbar_bounds_updated(&self) -> CoordinatorActions {
        CoordinatorActions::default()
    }
}

/// RAII guard that suppresses Start Menu focus-loss dismissal while a native
/// modal file/folder picker dialog is open, guaranteeing cleanup even on panic.
pub struct ModalPickerGuard<'a> {
    coordinator: &'a Mutex<WindowCoordinator>,
}

impl<'a> ModalPickerGuard<'a> {
    pub fn acquire(coordinator: &'a Mutex<WindowCoordinator>) -> Self {
        let mut guard = coordinator.lock().unwrap_or_else(|e| e.into_inner());
        guard.begin_modal_picker();
        drop(guard);
        Self { coordinator }
    }
}

impl Drop for ModalPickerGuard<'_> {
    fn drop(&mut self) {
        let mut guard = self.coordinator.lock().unwrap_or_else(|e| e.into_inner());
        guard.end_modal_picker();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon_1080p() -> PixelRect {
        PixelRect::new(0, 0, 1920, 1080)
    }

    #[test]
    fn flyout_kind_serde_canonical_and_legacy_aliases() {
        assert_eq!(
            serde_json::from_str::<FlyoutKind>("\"quick\"").unwrap(),
            FlyoutKind::Sound
        );
        assert_eq!(
            serde_json::from_str::<FlyoutKind>("\"ctx_app\"").unwrap(),
            FlyoutKind::ContextApp
        );
        assert_eq!(
            serde_json::from_str::<FlyoutKind>("\"ctx_taskbar\"").unwrap(),
            FlyoutKind::ContextBar
        );

        let canonical_cases = [
            ("\"preview\"", FlyoutKind::Preview),
            ("\"tray\"", FlyoutKind::Tray),
            ("\"sound\"", FlyoutKind::Sound),
            ("\"calendar\"", FlyoutKind::Calendar),
            ("\"weather\"", FlyoutKind::Weather),
            ("\"battery\"", FlyoutKind::Battery),
            ("\"network\"", FlyoutKind::Network),
            ("\"telemetry\"", FlyoutKind::Telemetry),
            ("\"context_app\"", FlyoutKind::ContextApp),
            ("\"context_bar\"", FlyoutKind::ContextBar),
            ("\"osd\"", FlyoutKind::Osd),
        ];
        for (raw, expected) in canonical_cases {
            let parsed: FlyoutKind = serde_json::from_str(raw).unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(serde_json::to_string(&expected).unwrap(), raw);
        }

        assert!(serde_json::from_str::<FlyoutKind>("\"unknown_kind\"").is_err());
    }

    #[test]
    fn hover_and_osd_do_not_preempt_click_flyouts() {
        let click_kinds = [
            FlyoutKind::Tray,
            FlyoutKind::Sound,
            FlyoutKind::Calendar,
            FlyoutKind::Weather,
            FlyoutKind::Battery,
            FlyoutKind::Network,
            FlyoutKind::Telemetry,
            FlyoutKind::ContextApp,
            FlyoutKind::ContextBar,
        ];

        for active_kind in click_kinds {
            let mut coord = WindowCoordinator::new();
            let open_act = coord.request_open_flyout(active_kind, 1000);
            assert_eq!(open_act.flyout_visible, Some(true));
            assert_eq!(open_act.flyout_non_activating, Some(false));
            assert_eq!(coord.active_flyout(), Some(active_kind));

            let preview_act = coord.request_open_flyout(FlyoutKind::Preview, 1050);
            assert_eq!(preview_act, CoordinatorActions::default());
            assert_eq!(coord.active_flyout(), Some(active_kind));

            let osd_act = coord.request_open_flyout(FlyoutKind::Osd, 1060);
            assert_eq!(osd_act, CoordinatorActions::default());
            assert_eq!(coord.active_flyout(), Some(active_kind));
        }
    }

    #[test]
    fn preview_and_osd_non_activating_and_preempted_by_click_flyout() {
        let mut coord = WindowCoordinator::new();

        let prev = coord.request_open_flyout(FlyoutKind::Preview, 500);
        assert_eq!(prev.flyout_visible, Some(true));
        assert_eq!(prev.flyout_non_activating, Some(true));
        assert!(prev.emit_flyout_show);

        let sound = coord.request_open_flyout(FlyoutKind::Sound, 600);
        assert_eq!(sound.flyout_visible, Some(true));
        assert_eq!(sound.flyout_non_activating, Some(false));
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Sound));

        // Re-clicking same toggleable flyout closes it
        let closed = coord.request_open_flyout(FlyoutKind::Sound, 700);
        assert_eq!(closed.flyout_visible, Some(false));
        assert!(closed.emit_flyout_hide);
        assert_eq!(coord.active_flyout(), None);

        // Context menus stay open and reposition on re-request
        let ctx1 = coord.request_open_flyout(FlyoutKind::ContextApp, 900);
        assert!(ctx1.emit_flyout_show);
        let ctx2 = coord.request_open_flyout(FlyoutKind::ContextApp, 950);
        assert!(ctx2.emit_flyout_show);
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::ContextApp));
    }

    #[test]
    fn weather_and_battery_flyouts_are_interactive_and_toggle_on_reclick() {
        for kind in [
            FlyoutKind::Weather,
            FlyoutKind::Battery,
            FlyoutKind::Network,
            FlyoutKind::Telemetry,
        ] {
            assert!(!kind.is_non_activating());
            assert!(kind.is_interactive_click());
            assert!(kind.toggles_on_reclick());

            let mut coord = WindowCoordinator::new();
            let _ = coord.request_toggle_start(Some(true), 100);
            let opened = coord.request_open_flyout(kind, 200);
            assert_eq!(opened.start_visible, Some(false));
            assert_eq!(opened.flyout_visible, Some(true));
            assert_eq!(opened.flyout_non_activating, Some(false));
            assert!(opened.emit_flyout_show);
            assert_eq!(coord.active_flyout(), Some(kind));

            let toggled_off = coord.request_open_flyout(kind, 250);
            assert_eq!(toggled_off.flyout_visible, Some(false));
            assert!(toggled_off.emit_flyout_hide);
            assert_eq!(coord.active_flyout(), None);

            let reopened = coord.request_open_flyout(kind, 300);
            assert_eq!(reopened.flyout_visible, Some(true));
            assert_eq!(coord.active_flyout(), Some(kind));

            let blurred = coord.on_flyout_focus_lost(300 + FLYOUT_OPEN_FOCUS_GRACE_MS);
            assert_eq!(blurred.flyout_visible, Some(false));
            assert_eq!(coord.active_flyout(), None);

            assert_eq!(
                coord.request_open_flyout(kind, 300 + FLYOUT_OPEN_FOCUS_GRACE_MS + 50),
                CoordinatorActions::default()
            );
        }
    }

    #[test]
    fn focus_loss_anti_double_toggle_debounce() {
        let mut coord = WindowCoordinator::new();

        // Start focus-loss debounce
        let opened = coord.request_toggle_start(None, 1000);
        assert_eq!(opened.start_visible, Some(true));
        assert!(coord.is_start_open());

        let dismissed = coord.on_start_focus_lost(2000);
        assert_eq!(dismissed.start_visible, Some(false));
        assert_eq!(dismissed.emit_start_state, Some(false));
        assert!(!coord.is_start_open());

        // Re-click at t + 100ms (< 180ms) is suppressed
        assert_eq!(
            coord.request_toggle_start(None, 2100),
            CoordinatorActions::default()
        );
        assert!(!coord.is_start_open());

        // Re-click at t + 179ms (< 180ms) is still suppressed
        assert_eq!(
            coord.request_toggle_start(None, 2179),
            CoordinatorActions::default()
        );

        // Re-click at t + 180ms succeeds
        let reopened = coord.request_toggle_start(None, 2180);
        assert_eq!(reopened.start_visible, Some(true));
        assert!(coord.is_start_open());

        // Interactive flyout focus-loss debounce (per trigger)
        let sound_open = coord.request_open_flyout(FlyoutKind::Sound, 3000);
        assert_eq!(sound_open.start_visible, Some(false));
        assert_eq!(sound_open.flyout_visible, Some(true));

        let sound_blur = coord.on_flyout_focus_lost(4000);
        assert_eq!(sound_blur.flyout_visible, Some(false));
        assert!(sound_blur.emit_flyout_hide);

        // Re-click Sound at t + 100ms suppressed
        assert_eq!(
            coord.request_open_flyout(FlyoutKind::Sound, 4100),
            CoordinatorActions::default()
        );
        assert_eq!(coord.active_flyout(), None);

        // Different trigger (Calendar) at t + 100ms is NOT blocked by Sound debounce
        let cal = coord.request_open_flyout(FlyoutKind::Calendar, 4100);
        assert_eq!(cal.flyout_visible, Some(true));
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Calendar));
        let _ = coord.request_close_flyout();

        // Sound at t + 180ms succeeds
        let sound_again = coord.request_open_flyout(FlyoutKind::Sound, 4180);
        assert_eq!(sound_again.flyout_visible, Some(true));
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Sound));
    }

    #[test]
    fn modal_picker_guard_suppresses_start_dismissal_and_survives_panic() {
        let mutex = Mutex::new(WindowCoordinator::new());
        {
            let mut c = mutex.lock().unwrap();
            let _ = c.request_toggle_start(Some(true), 100);
            assert!(c.is_start_open());
        }

        // Normal guard lifetime (past 200ms post-open grace window)
        {
            let _guard = ModalPickerGuard::acquire(&mutex);
            let mut c = mutex.lock().unwrap();
            assert!(c.is_modal_picker_active());
            assert_eq!(c.on_start_focus_lost(400), CoordinatorActions::default());
            assert_eq!(
                c.on_global_mouse_down(10, 10, &[PixelRect::new(500, 500, 800, 800)], 410),
                CoordinatorActions::default()
            );
            assert!(c.is_start_open());
        }
        assert!(!mutex.lock().unwrap().is_modal_picker_active());

        // Panic inside guarded scope must still end modal state
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = ModalPickerGuard::acquire(&mutex);
            assert!(mutex.lock().unwrap().is_modal_picker_active());
            panic!("simulated picker panic");
        }));
        assert!(res.is_err());

        let mut c = mutex.lock().unwrap();
        assert!(!c.is_modal_picker_active());
        let dismissed = c.on_start_focus_lost(500);
        assert_eq!(dismissed.start_visible, Some(false));
        assert!(!c.is_start_open());
    }

    #[test]
    fn fullscreen_suppression_hides_surfaces_and_bounds_update_does_not_leak() {
        let mut coord = WindowCoordinator::new();
        let _ = coord.request_toggle_start(Some(true), 100);
        assert!(coord.should_show_taskbar());

        let enter_fs = coord.on_fullscreen_changed(true);
        assert_eq!(enter_fs.taskbar_visible, Some(false));
        assert_eq!(enter_fs.start_visible, Some(false));
        assert_eq!(enter_fs.emit_start_state, Some(false));
        assert!(coord.is_fullscreen_suppressed());
        assert!(!coord.should_show_taskbar());

        // Bounds update while fullscreen suppressed must NOT force taskbar visible
        let bounds_act = coord.on_taskbar_bounds_updated();
        assert_eq!(bounds_act.taskbar_visible, None);
        assert!(!coord.should_show_taskbar());

        // Leaving fullscreen restores taskbar visibility
        let exit_fs = coord.on_fullscreen_changed(false);
        assert_eq!(exit_fs.taskbar_visible, Some(true));
        assert!(!coord.is_fullscreen_suppressed());
        assert!(coord.should_show_taskbar());
    }

    #[test]
    fn global_mouse_down_dismisses_only_when_outside_active_shell_rects() {
        let mut coord = WindowCoordinator::new();
        let taskbar_rect = PixelRect::new(600, 1016, 1320, 1080);
        let start_rect = PixelRect::new(420, 340, 1500, 1000);

        let _ = coord.request_toggle_start(Some(true), 100);

        // Inside start -> ignored
        assert_eq!(
            coord.on_global_mouse_down(500, 500, &[taskbar_rect, start_rect], 150),
            CoordinatorActions::default()
        );
        assert!(coord.is_start_open());

        // Inside taskbar -> ignored
        assert_eq!(
            coord.on_global_mouse_down(700, 1050, &[taskbar_rect, start_rect], 160),
            CoordinatorActions::default()
        );
        assert!(coord.is_start_open());

        // Outside all shell rects -> dismissed + debounced
        let act = coord.on_global_mouse_down(50, 50, &[taskbar_rect, start_rect], 200);
        assert_eq!(act.start_visible, Some(false));
        assert!(!coord.is_start_open());
        assert_eq!(
            coord.request_toggle_start(None, 300),
            CoordinatorActions::default()
        );
    }

    #[test]
    fn classify_fullscreen_strict_matrix() {
        let mon = mon_1080p();

        // Explicit D3D fullscreen and presentation mode always return true
        assert!(classify_fullscreen(
            NotificationState::RunningD3dFullScreen,
            None,
            mon
        ));
        assert!(classify_fullscreen(
            NotificationState::PresentationMode,
            None,
            mon
        ));

        // QUNS_BUSY alone must NOT trigger fullscreen
        assert!(!classify_fullscreen(NotificationState::Busy, None, mon));

        let borderless_game = ForegroundWindowInfo {
            visible: true,
            is_own_process: false,
            class_name: "UnrealWindow".to_string(),
            style: 0x9000_0000, // WS_POPUP | WS_VISIBLE (no WS_CAPTION, no WS_THICKFRAME)
            rect: mon,
        };
        assert!(classify_fullscreen(
            NotificationState::AcceptsNotifications,
            Some(&borderless_game),
            mon
        ));

        // Borderless window with WS_BORDER only (not full WS_CAPTION) is still borderless fullscreen
        let ws_border_only = ForegroundWindowInfo {
            style: 0x0080_0000,
            ..borderless_game.clone()
        };
        assert!(classify_fullscreen(
            NotificationState::AcceptsNotifications,
            Some(&ws_border_only),
            mon
        ));

        // Normal maximized window with WS_CAPTION returns false
        let with_caption = ForegroundWindowInfo {
            style: WS_CAPTION_FLAG,
            ..borderless_game.clone()
        };
        assert!(!classify_fullscreen(
            NotificationState::Busy,
            Some(&with_caption),
            mon
        ));

        // Window with WS_THICKFRAME returns false
        let with_thickframe = ForegroundWindowInfo {
            style: WS_THICKFRAME_FLAG,
            ..borderless_game.clone()
        };
        assert!(!classify_fullscreen(
            NotificationState::AcceptsNotifications,
            Some(&with_thickframe),
            mon
        ));

        // Shell desktop / taskbar windows return false
        for shell_cls in ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
            let shell = ForegroundWindowInfo {
                class_name: shell_cls.to_string(),
                ..borderless_game.clone()
            };
            assert!(!classify_fullscreen(
                NotificationState::AcceptsNotifications,
                Some(&shell),
                mon
            ));
        }

        // Own process returns false
        let own = ForegroundWindowInfo {
            is_own_process: true,
            ..borderless_game.clone()
        };
        assert!(!classify_fullscreen(
            NotificationState::AcceptsNotifications,
            Some(&own),
            mon
        ));

        // Invisible window returns false
        let hidden = ForegroundWindowInfo {
            visible: false,
            ..borderless_game.clone()
        };
        assert!(!classify_fullscreen(
            NotificationState::AcceptsNotifications,
            Some(&hidden),
            mon
        ));

        // Non-covering window returns false
        let partial = ForegroundWindowInfo {
            rect: PixelRect::new(0, 0, 1280, 720),
            ..borderless_game
        };
        assert!(!classify_fullscreen(
            NotificationState::AcceptsNotifications,
            Some(&partial),
            mon
        ));
    }

    #[test]
    fn compute_appbar_rect_scales_across_dpis_and_modes() {
        let mon = mon_1080p();

        let cases = [
            (96, BarMode::Dock, 67),
            (120, BarMode::Dock, 84),
            (144, BarMode::Dock, 101),
            (192, BarMode::Dock, 134),
            (0, BarMode::Dock, 67),
            (96, BarMode::Edge, 58),
            (120, BarMode::Edge, 73),
            (144, BarMode::Edge, 87),
            (192, BarMode::Edge, 116),
            (0, BarMode::Edge, 58),
        ];

        for (dpi, mode, expected_h) in cases {
            let r = compute_appbar_rect(mon, dpi, mode);
            assert_eq!(r.left, mon.left);
            assert_eq!(r.right, mon.right);
            assert_eq!(r.bottom, mon.bottom);
            assert_eq!(r.top, mon.bottom - expected_h);
            assert_eq!(r.height(), expected_h);
        }
    }

    #[test]
    fn tauri_conf_json_hardening_invariants() {
        let raw = include_str!("../tauri.conf.json");
        let conf: serde_json::Value =
            serde_json::from_str(raw).expect("tauri.conf.json must be valid JSON");

        let windows = conf["app"]["windows"]
            .as_array()
            .expect("app.windows must be an array");
        assert_eq!(windows.len(), 4, "expected exactly 4 pre-warmed windows");

        for expected_label in ["taskbar", "taskbar-secondary", "start", "flyout"] {
            let win = windows
                .iter()
                .find(|w| w["label"].as_str() == Some(expected_label))
                .unwrap_or_else(|| panic!("missing window '{expected_label}'"));
            assert_eq!(win["resizable"].as_bool(), Some(false), "{expected_label} must have resizable: false");
            assert_eq!(win["decorations"].as_bool(), Some(false), "{expected_label} must have decorations: false");
            assert_eq!(win["transparent"].as_bool(), Some(true), "{expected_label} must have transparent: true");
            assert_eq!(win["shadow"].as_bool(), Some(false), "{expected_label} must have shadow: false");
            assert_eq!(win["skipTaskbar"].as_bool(), Some(true), "{expected_label} must have skipTaskbar: true");
            if expected_label == "taskbar-secondary" {
                assert_eq!(
                    win["url"].as_str(),
                    Some("index.html?view=taskbar&monitor=secondary")
                );
            }
        }

        assert_eq!(
            conf["app"]["security"]["csp"].as_str(),
            Some("default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'")
        );
    }

    #[test]
    fn start_toggle_close_preemption_force_and_fullscreen_block() {
        let mut coord = WindowCoordinator::new();

        // Opening Start preempts an active flyout
        let _ = coord.request_open_flyout(FlyoutKind::Sound, 100);
        let open_start = coord.request_toggle_start(None, 200);
        assert_eq!(open_start.flyout_visible, Some(false));
        assert!(open_start.emit_flyout_hide);
        assert_eq!(open_start.start_visible, Some(true));
        assert_eq!(open_start.emit_start_state, Some(true));
        assert!(coord.is_start_open());
        assert_eq!(coord.active_flyout(), None);

        // Closing via None
        let close_none = coord.request_toggle_start(None, 250);
        assert_eq!(close_none.start_visible, Some(false));
        assert_eq!(close_none.emit_start_state, Some(false));
        assert!(!coord.is_start_open());

        // Closing via Some(false)
        let _ = coord.request_toggle_start(Some(true), 300);
        let close_false = coord.request_toggle_start(Some(false), 320);
        assert_eq!(close_false.start_visible, Some(false));
        assert!(!coord.is_start_open());

        // force: Some(true) bypasses anti-double-toggle debounce
        let _ = coord.request_toggle_start(Some(true), 500);
        let _ = coord.on_start_focus_lost(800);
        assert_eq!(
            coord.request_toggle_start(None, 850),
            CoordinatorActions::default()
        );
        let forced = coord.request_toggle_start(Some(true), 850);
        assert_eq!(forced.start_visible, Some(true));
        assert!(coord.is_start_open());

        // Explicit Start toggle overrides fullscreen_suppressed so physical Win key atomically reveals Start + Taskbar
        let _ = coord.on_fullscreen_changed(true);
        assert!(!coord.is_start_open());
        let fs_open = coord.request_toggle_start(None, 2000);
        assert_eq!(fs_open.start_visible, Some(true));
        assert_eq!(fs_open.taskbar_visible, Some(true));
        assert_eq!(fs_open.emit_start_state, Some(true));
        assert!(fs_open.hide_flyout_immediate);
        assert!(coord.is_start_open());

        let fs_close = coord.request_toggle_start(Some(false), 2050);
        assert_eq!(fs_close.start_visible, Some(false));
        assert_eq!(fs_close.taskbar_visible, Some(false));
        assert!(!coord.is_start_open());
    }

    #[test]
    fn flyout_open_start_interaction_debounce_and_fullscreen_block() {
        let mut coord = WindowCoordinator::new();

        // While Start is open, Preview is suppressed but Osd is allowed without closing Start
        let _ = coord.request_toggle_start(Some(true), 100);
        assert_eq!(
            coord.request_open_flyout(FlyoutKind::Preview, 150),
            CoordinatorActions::default()
        );
        assert_eq!(coord.active_flyout(), None);
        assert!(coord.is_start_open());

        let osd = coord.request_open_flyout(FlyoutKind::Osd, 160);
        assert_eq!(osd.flyout_visible, Some(true));
        assert_eq!(osd.flyout_non_activating, Some(true));
        assert_eq!(osd.start_visible, None);
        assert!(coord.is_start_open());
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Osd));
        let _ = coord.request_toggle_start(Some(false), 170);
        let _ = coord.request_close_flyout();

        // Transient Preview/Osd opening within 180ms after Sound blur (past 160ms grace) does NOT bypass Sound debounce
        let _ = coord.request_open_flyout(FlyoutKind::Sound, 1000);
        let _ = coord.on_flyout_focus_lost(1200);
        let prev = coord.request_open_flyout(FlyoutKind::Preview, 1250);
        assert_eq!(prev.flyout_visible, Some(true));
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Preview));
        assert_eq!(
            coord.request_open_flyout(FlyoutKind::Sound, 1300),
            CoordinatorActions::default()
        );
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Preview));

        // After 180ms window, Sound preempts Preview
        let reopened = coord.request_open_flyout(FlyoutKind::Sound, 1380);
        assert_eq!(reopened.flyout_visible, Some(true));
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Sound));

        // Blocked while fullscreen_suppressed == true
        let _ = coord.on_fullscreen_changed(true);
        assert_eq!(coord.active_flyout(), None);
        for kind in [FlyoutKind::Preview, FlyoutKind::Osd, FlyoutKind::Sound] {
            assert_eq!(
                coord.request_open_flyout(kind, 2000),
                CoordinatorActions::default()
            );
            assert_eq!(coord.active_flyout(), None);
        }
    }

    #[test]
    fn close_preview_and_non_activating_focus_loss() {
        let mut coord = WindowCoordinator::new();

        // request_close_preview closes Preview
        let _ = coord.request_open_flyout(FlyoutKind::Preview, 100);
        assert_eq!(coord.on_flyout_focus_lost(150), CoordinatorActions::default());
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Preview));
        let closed = coord.request_close_preview();
        assert_eq!(closed.flyout_visible, Some(false));
        assert!(closed.emit_flyout_hide);
        assert_eq!(coord.active_flyout(), None);

        // request_close_preview preserves Osd and Sound; on_flyout_focus_lost ignores Osd
        let _ = coord.request_open_flyout(FlyoutKind::Osd, 200);
        assert_eq!(coord.on_flyout_focus_lost(250), CoordinatorActions::default());
        assert_eq!(coord.request_close_preview(), CoordinatorActions::default());
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Osd));

        let _ = coord.request_open_flyout(FlyoutKind::Sound, 300);
        assert_eq!(coord.request_close_preview(), CoordinatorActions::default());
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Sound));
    }

    #[test]
    fn global_mouse_down_dismisses_flyout_debounces_and_uses_half_open_rect() {
        let mut coord = WindowCoordinator::new();
        let fly_rect = PixelRect::new(100, 100, 300, 300);

        let _ = coord.request_open_flyout(FlyoutKind::Tray, 1000);

        // Half-open boundary: right (300) and bottom (300) are OUTSIDE
        assert!(fly_rect.contains(100, 100));
        assert!(fly_rect.contains(299, 299));
        assert!(!fly_rect.contains(300, 200));
        assert!(!fly_rect.contains(200, 300));

        // Click on exclusive right boundary dismisses Tray and records 180ms debounce
        let act = coord.on_global_mouse_down(300, 200, &[fly_rect], 1100);
        assert_eq!(act.flyout_visible, Some(false));
        assert!(act.emit_flyout_hide);
        assert_eq!(coord.active_flyout(), None);

        assert_eq!(
            coord.request_open_flyout(FlyoutKind::Tray, 1200),
            CoordinatorActions::default()
        );
        let reopened = coord.request_open_flyout(FlyoutKind::Tray, 1280);
        assert_eq!(reopened.flyout_visible, Some(true));
    }

    #[test]
    fn fullscreen_hides_active_flyout_and_deduplicates_repeated_states() {
        let mut coord = WindowCoordinator::new();
        let _ = coord.request_open_flyout(FlyoutKind::Calendar, 100);

        let enter = coord.on_fullscreen_changed(true);
        assert_eq!(enter.taskbar_visible, Some(false));
        assert_eq!(enter.flyout_visible, Some(false));
        assert!(enter.emit_flyout_hide);
        assert_eq!(coord.active_flyout(), None);

        // Repeated true is a noop
        assert_eq!(
            coord.on_fullscreen_changed(true),
            CoordinatorActions::default()
        );

        let exit = coord.on_fullscreen_changed(false);
        assert_eq!(exit.taskbar_visible, Some(true));

        // Repeated false is a noop
        assert_eq!(
            coord.on_fullscreen_changed(false),
            CoordinatorActions::default()
        );
    }

    #[test]
    fn bar_mode_from_config_str_parsing() {
        assert_eq!(BarMode::from_config_str("edge"), BarMode::Edge);
        assert_eq!(BarMode::from_config_str("EDGE"), BarMode::Edge);
        assert_eq!(BarMode::from_config_str("Edge"), BarMode::Edge);
        assert_eq!(BarMode::from_config_str("dock"), BarMode::Dock);
        assert_eq!(BarMode::from_config_str("unknown"), BarMode::Dock);
    }

    #[test]
    fn backward_clock_step_does_not_lock_out_debounce() {
        let mut coord = WindowCoordinator::new();
        let _ = coord.request_toggle_start(Some(true), 1000);
        let _ = coord.on_start_focus_lost(2000);
        assert!(!coord.is_start_open());

        // Clock stepped backward (1500 < 2000) -> must NOT lock out reopening
        let reopened = coord.request_toggle_start(None, 1500);
        assert_eq!(reopened.start_visible, Some(true));
        assert!(coord.is_start_open());
    }

    #[test]
    fn compute_appbar_rect_clamps_to_monitor_height() {
        let short_mon = PixelRect::new(0, 100, 1920, 150); // height = 50
        let r = compute_appbar_rect(short_mon, 384, BarMode::Dock);
        assert_eq!(r.top, short_mon.top);
        assert_eq!(r.bottom, short_mon.bottom);
        assert_eq!(r.height(), short_mon.height());
    }

    #[test]
    fn start_post_open_focus_loss_grace_window() {
        let mut coord = WindowCoordinator::new();
        let _ = coord.request_toggle_start(Some(true), 1000);

        // Transient focus-loss within 200ms grace window is ignored
        assert_eq!(coord.on_start_focus_lost(1050), CoordinatorActions::default());
        assert_eq!(coord.on_start_focus_lost(1199), CoordinatorActions::default());
        assert!(coord.is_start_open());

        // Global click-outside is NOT suppressed by post-open grace window
        let rect = PixelRect::new(500, 500, 900, 900);
        let clicked_out = coord.on_global_mouse_down(10, 10, &[rect], 1050);
        assert_eq!(clicked_out.start_visible, Some(false));
        assert!(!coord.is_start_open());

        // Explicit close is NOT suppressed by post-open grace window
        let _ = coord.request_toggle_start(Some(true), 2000);
        let closed = coord.request_toggle_start(Some(false), 2010);
        assert_eq!(closed.start_visible, Some(false));
        assert!(!coord.is_start_open());

        // Focus-loss at >= 200ms after open dismisses Start
        let _ = coord.request_toggle_start(Some(true), 3000);
        let dismissed = coord.on_start_focus_lost(3200);
        assert_eq!(dismissed.start_visible, Some(false));
        assert!(!coord.is_start_open());
    }

    #[test]
    fn appbar_fullscreen_hint_uses_single_authority_and_gates_bounds() {
        let mon = mon_1080p();
        let maximized_app = ForegroundWindowInfo {
            visible: true,
            is_own_process: false,
            class_name: "Chrome_WidgetWin_1".to_string(),
            style: WS_CAPTION_FLAG | WS_THICKFRAME_FLAG,
            rect: mon,
        };
        let borderless_game = ForegroundWindowInfo {
            visible: true,
            is_own_process: false,
            class_name: "UnrealWindow".to_string(),
            style: 0x9000_0000,
            rect: mon,
        };

        let mut coord = WindowCoordinator::new();
        assert!(coord.should_apply_taskbar_bounds());

        // ABN_FULLSCREENAPP(true) over a normal maximized desktop app must NOT hide taskbar
        let false_alarm = coord.on_appbar_fullscreen_hint(
            true,
            NotificationState::Busy,
            Some(&maximized_app),
            mon,
        );
        assert_eq!(false_alarm, CoordinatorActions::default());
        assert!(!coord.is_fullscreen_suppressed());
        assert!(coord.should_show_taskbar());
        assert!(coord.should_apply_taskbar_bounds());

        // Open Start before entering true fullscreen
        let _ = coord.request_toggle_start(Some(true), 100);
        assert!(coord.is_start_open());

        // ABN_FULLSCREENAPP(false) while D3D fullscreen is active still enters suppression (ignores raw lparam)
        let d3d_enter = coord.on_appbar_fullscreen_hint(
            false,
            NotificationState::RunningD3dFullScreen,
            Some(&maximized_app),
            mon,
        );
        assert_eq!(d3d_enter.taskbar_visible, Some(false));
        assert_eq!(d3d_enter.start_visible, Some(false));
        assert_eq!(d3d_enter.emit_start_state, Some(false));
        assert!(coord.is_fullscreen_suppressed());
        assert!(!coord.should_show_taskbar());
        assert!(!coord.should_apply_taskbar_bounds());

        // Bounds updates while suppressed remain blocked
        assert_eq!(coord.on_taskbar_bounds_updated(), CoordinatorActions::default());
        assert!(!coord.should_apply_taskbar_bounds());

        // ABN_FULLSCREENAPP(false) while a borderless game is still covering monitor stays suppressed
        let still_fs = coord.on_appbar_fullscreen_hint(
            false,
            NotificationState::AcceptsNotifications,
            Some(&borderless_game),
            mon,
        );
        assert_eq!(still_fs, CoordinatorActions::default());
        assert!(coord.is_fullscreen_suppressed());
        assert!(!coord.should_apply_taskbar_bounds());

        // Exiting to desktop/normal window restores taskbar & bounds updates even if hint claims true
        let exit = coord.on_appbar_fullscreen_hint(
            true,
            NotificationState::AcceptsNotifications,
            Some(&maximized_app),
            mon,
        );
        assert_eq!(exit.taskbar_visible, Some(true));
        assert!(!coord.is_fullscreen_suppressed());
        assert!(coord.should_show_taskbar());
        assert!(coord.should_apply_taskbar_bounds());
    }

    #[test]
    fn startup_reevaluate_fullscreen_suppresses_initial_bounds_and_visibility() {
        let mon = mon_1080p();
        let borderless_game = ForegroundWindowInfo {
            visible: true,
            is_own_process: false,
            class_name: "UnityWndClass".to_string(),
            style: 0x9000_0000,
            rect: mon,
        };

        let mut coord = WindowCoordinator::new();
        let startup_act = coord.reevaluate_fullscreen(
            NotificationState::AcceptsNotifications,
            Some(&borderless_game),
            mon,
        );
        assert_eq!(startup_act.taskbar_visible, Some(false));
        assert!(coord.is_fullscreen_suppressed());
        assert!(!coord.should_show_taskbar());
        assert!(!coord.should_apply_taskbar_bounds());

        // Once game exits, reevaluate_fullscreen restores taskbar
        let exit_act = coord.reevaluate_fullscreen(
            NotificationState::AcceptsNotifications,
            None,
            mon,
        );
        assert_eq!(exit_act.taskbar_visible, Some(true));
        assert!(!coord.is_fullscreen_suppressed());
        assert!(coord.should_show_taskbar());
        assert!(coord.should_apply_taskbar_bounds());
    }

    #[test]
    fn compute_effective_scale_and_scaled_appbar_respect_text_scale_factor() {
        let mon = mon_1080p();

        assert!((compute_effective_scale(96, 1.0) - 1.0).abs() < 1e-9);
        assert!((compute_effective_scale(96, 1.4) - 1.4).abs() < 1e-9);
        assert!((compute_effective_scale(144, 1.4) - 2.1).abs() < 1e-9);
        assert!((compute_effective_scale(0, 1.4) - 1.4).abs() < 1e-9);
        assert!((compute_effective_scale(96, 0.5) - 1.0).abs() < 1e-9);
        assert!((compute_effective_scale(96, 3.0) - 2.25).abs() < 1e-9);
        assert!((compute_effective_scale(96, f64::NAN) - 1.0).abs() < 1e-9);

        // 96 DPI + 140% TextScaleFactor -> Dock 67 * 1.4 = 93.8 -> 94px, Edge 58 * 1.4 = 81.2 -> 81px
        let dock_140 = compute_appbar_rect_scaled(mon, compute_effective_scale(96, 1.4), BarMode::Dock);
        assert_eq!(dock_140.height(), 94);
        assert_eq!(dock_140.top, 1080 - 94);
        assert_eq!(dock_140.bottom, 1080);

        let edge_140 = compute_appbar_rect_scaled(mon, compute_effective_scale(96, 1.4), BarMode::Edge);
        assert_eq!(edge_140.height(), 81);
        assert_eq!(edge_140.top, 1080 - 81);
        assert_eq!(edge_140.bottom, 1080);

        // Invalid effective scale falls back safely to 1.0x
        assert_eq!(compute_appbar_rect_scaled(mon, f64::NAN, BarMode::Dock).height(), 67);
        assert_eq!(compute_appbar_rect_scaled(mon, -1.0, BarMode::Edge).height(), 58);
    }

    #[test]
    fn windows_update_targets_tracks_visible_surfaces_only() {
        let mut coord = WindowCoordinator::new();
        assert!(!coord.is_start_visible());
        assert!(!coord.is_flyout_visible());
        assert_eq!(coord.windows_update_targets(), vec!["taskbar"]);

        let _ = coord.request_toggle_start(Some(true), 100);
        assert!(coord.is_start_visible());
        assert!(!coord.is_flyout_visible());
        assert_eq!(coord.windows_update_targets(), vec!["taskbar", "start"]);

        let _ = coord.request_open_flyout(FlyoutKind::Osd, 150);
        assert!(coord.is_start_visible());
        assert!(coord.is_flyout_visible());
        assert_eq!(
            coord.windows_update_targets(),
            vec!["taskbar", "start", "flyout"]
        );

        let _ = coord.request_open_flyout(FlyoutKind::Sound, 200);
        assert!(!coord.is_start_visible());
        assert!(coord.is_flyout_visible());
        assert_eq!(coord.windows_update_targets(), vec!["taskbar", "flyout"]);

        let _ = coord.request_close_flyout();
        assert_eq!(coord.windows_update_targets(), vec!["taskbar"]);
    }

    #[test]
    fn audio_master_and_mixer_targets_and_close_osd_guard() {
        let mut coord = WindowCoordinator::new();

        // Idle desktop: master -> taskbar (+ secondary if visible); mixer -> empty!
        assert_eq!(coord.audio_master_targets_with_secondary(false), vec!["taskbar"]);
        assert_eq!(
            coord.audio_master_targets_with_secondary(true),
            vec!["taskbar", "taskbar-secondary"]
        );
        assert!(coord.audio_mixer_targets().is_empty());

        // Tray or Calendar open: mixer stays empty, flyout does NOT receive audio spam
        let _ = coord.request_open_flyout(FlyoutKind::Tray, 100);
        assert_eq!(coord.audio_master_targets_with_secondary(false), vec!["taskbar"]);
        assert!(coord.audio_mixer_targets().is_empty());
        // close_osd must NOT dismiss Tray!
        assert_eq!(coord.request_close_osd(), CoordinatorActions::default());
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Tray));
        let _ = coord.request_close_flyout();

        // OSD open: master targets taskbar + flyout; mixer stays empty!
        let _ = coord.request_open_flyout(FlyoutKind::Osd, 200);
        assert_eq!(
            coord.audio_master_targets_with_secondary(false),
            vec!["taskbar", "flyout"]
        );
        assert!(coord.audio_mixer_targets().is_empty());
        // close_osd cleanly closes OSD
        let close_osd = coord.request_close_osd();
        assert_eq!(close_osd.flyout_visible, Some(false));
        assert_eq!(coord.active_flyout(), None);

        // Quick Sound open: both master and mixer target flyout (and OSD cannot preempt it)
        let _ = coord.request_open_flyout(FlyoutKind::Sound, 300);
        assert_eq!(
            coord.audio_master_targets_with_secondary(true),
            vec!["taskbar", "taskbar-secondary", "flyout"]
        );
        assert_eq!(coord.audio_mixer_targets(), vec!["flyout"]);
        assert_eq!(coord.request_close_osd(), CoordinatorActions::default());
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Sound));

        // Start open (closes Sound): mixer targets start only
        let _ = coord.request_toggle_start(Some(true), 400);
        assert_eq!(
            coord.audio_master_targets_with_secondary(false),
            vec!["taskbar", "start"]
        );
        assert_eq!(coord.audio_mixer_targets(), vec!["start"]);
    }

    #[test]
    fn classify_fullscreen_for_monitor_handles_per_monitor_d3d_uwp_and_borderless() {
        let primary = PixelRect::new(0, 0, 1920, 1080);
        let primary_wa = PixelRect::new(0, 0, 1920, 1016);
        let secondary = PixelRect::new(1920, 0, 3840, 1080);

        // 1. D3D exclusive fullscreen on Monitor 2 ONLY suppresses secondary, NOT primary!
        let d3d_on_sec = DetailedForegroundWindow {
            visible: true,
            is_own_process: false,
            is_maximized: false,
            class_name: "UnrealWindow".into(),
            style: 0x1400_0000,
            window_rect: secondary,
            extended_rect: secondary,
            monitor_rect: secondary,
            work_area_rect: secondary,
        };
        assert!(!classify_fullscreen_for_monitor(
            NotificationState::RunningD3dFullScreen,
            Some(&d3d_on_sec),
            primary,
            true
        ));
        assert!(classify_fullscreen_for_monitor(
            NotificationState::RunningD3dFullScreen,
            Some(&d3d_on_sec),
            secondary,
            false
        ));

        // 2. UWP ApplicationFrameWindow (Netflix / Photos / Terminal F11) keeps WS_CAPTION | WS_THICKFRAME:
        //    - Normally maximized on primary: extended_rect stops at work_area_rect (1016px) -> NOT fullscreen
        let uwp_max_primary = DetailedForegroundWindow {
            visible: true,
            is_own_process: false,
            is_maximized: true,
            class_name: "ApplicationFrameWindow".into(),
            style: WS_CAPTION_FLAG | WS_THICKFRAME_FLAG,
            window_rect: PixelRect::new(-8, -8, 1928, 1024),
            extended_rect: primary_wa,
            monitor_rect: primary,
            work_area_rect: primary_wa,
        };
        assert!(!classify_fullscreen_for_monitor(
            NotificationState::AcceptsNotifications,
            Some(&uwp_max_primary),
            primary,
            true
        ));

        //    - Fullscreen on primary: extended_rect covers full monitor (past AppBar) -> FULLSCREEN
        let uwp_fs_primary = DetailedForegroundWindow {
            visible: true,
            is_own_process: false,
            is_maximized: true,
            class_name: "ApplicationFrameWindow".into(),
            style: WS_CAPTION_FLAG | WS_THICKFRAME_FLAG,
            window_rect: primary,
            extended_rect: primary,
            monitor_rect: primary,
            work_area_rect: primary_wa,
        };
        assert!(classify_fullscreen_for_monitor(
            NotificationState::AcceptsNotifications,
            Some(&uwp_fs_primary),
            primary,
            true
        ));

        //    - Normally maximized UWP on Monitor 2 (where work_area == monitor_rect): is_maximized=true -> NOT fullscreen
        let uwp_max_secondary = DetailedForegroundWindow {
            visible: true,
            is_own_process: false,
            is_maximized: true,
            class_name: "ApplicationFrameWindow".into(),
            style: WS_CAPTION_FLAG | WS_THICKFRAME_FLAG,
            window_rect: PixelRect::new(1912, -8, 3848, 1088),
            extended_rect: secondary,
            monitor_rect: secondary,
            work_area_rect: secondary,
        };
        assert!(!classify_fullscreen_for_monitor(
            NotificationState::AcceptsNotifications,
            Some(&uwp_max_secondary),
            secondary,
            false
        ));

        //    - Fullscreen UWP on Monitor 2: un-maximized covering full monitor -> FULLSCREEN
        let uwp_fs_secondary = DetailedForegroundWindow {
            visible: true,
            is_own_process: false,
            is_maximized: false,
            class_name: "ApplicationFrameWindow".into(),
            style: WS_CAPTION_FLAG | WS_THICKFRAME_FLAG,
            window_rect: secondary,
            extended_rect: secondary,
            monitor_rect: secondary,
            work_area_rect: secondary,
        };
        assert!(classify_fullscreen_for_monitor(
            NotificationState::AcceptsNotifications,
            Some(&uwp_fs_secondary),
            secondary,
            false
        ));

        // 3. Borderless game retaining WS_CAPTION covering full primary monitor (including AppBar) -> FULLSCREEN
        let borderless_with_caption = DetailedForegroundWindow {
            visible: true,
            is_own_process: false,
            is_maximized: false,
            class_name: "UnityWndClass".into(),
            style: WS_CAPTION_FLAG,
            window_rect: primary,
            extended_rect: primary,
            monitor_rect: primary,
            work_area_rect: primary_wa,
        };
        assert!(classify_fullscreen_for_monitor(
            NotificationState::AcceptsNotifications,
            Some(&borderless_with_caption),
            primary,
            true
        ));

        // 4. Maximized Chrome on Monitor 2 (where work_area == monitor_rect) -> NOT fullscreen
        let chrome_max_sec = DetailedForegroundWindow {
            visible: true,
            is_own_process: false,
            is_maximized: true,
            class_name: "Chrome_WidgetWin_1".into(),
            style: WS_CAPTION_FLAG | WS_THICKFRAME_FLAG,
            window_rect: PixelRect::new(1912, -8, 3848, 1088),
            extended_rect: secondary,
            monitor_rect: secondary,
            work_area_rect: secondary,
        };
        assert!(!classify_fullscreen_for_monitor(
            NotificationState::AcceptsNotifications,
            Some(&chrome_max_sec),
            secondary,
            false
        ));

        // 5. Legacy classify_fullscreen also detects UWP ApplicationFrameWindow covering primary monitor
        let uwp_legacy = ForegroundWindowInfo {
            visible: true,
            is_own_process: false,
            class_name: "ApplicationFrameWindow".into(),
            style: WS_CAPTION_FLAG | WS_THICKFRAME_FLAG,
            rect: primary,
        };
        assert!(classify_fullscreen(
            NotificationState::AcceptsNotifications,
            Some(&uwp_legacy),
            primary
        ));
    }

    #[test]
    fn per_monitor_fullscreen_transitions_and_start_override() {
        let mut coord = WindowCoordinator::new();
        assert!(coord.should_show_taskbar());
        assert!(coord.should_show_secondary_taskbar());

        // Secondary enters fullscreen -> ONLY secondary taskbar hides; primary stays visible
        let sec_enter = coord.set_monitor_fullscreen(false, true, 100);
        assert_eq!(sec_enter.taskbar_visible, None);
        assert_eq!(sec_enter.secondary_taskbar_visible, Some(false));
        assert!(!coord.is_primary_fullscreen());
        assert!(coord.is_secondary_fullscreen());
        assert!(coord.should_show_taskbar());
        assert!(!coord.should_show_secondary_taskbar());
        assert!(coord.should_apply_taskbar_bounds());
        assert!(!coord.should_apply_secondary_taskbar_bounds());

        // Flyout on secondary is blocked while secondary is fullscreen, but primary flyout still opens!
        assert_eq!(
            coord.request_open_flyout_on_monitor(FlyoutKind::Sound, Some("secondary"), 120),
            CoordinatorActions::default()
        );
        let primary_flyout =
            coord.request_open_flyout_on_monitor(FlyoutKind::Sound, Some("primary"), 130);
        assert_eq!(primary_flyout.flyout_visible, Some(true));

        // Primary enters fullscreen -> hides primary taskbar and immediately dismisses open flyout
        let pri_enter = coord.set_monitor_fullscreen(true, true, 200);
        assert_eq!(pri_enter.taskbar_visible, Some(false));
        assert_eq!(pri_enter.secondary_taskbar_visible, None);
        assert_eq!(pri_enter.flyout_visible, Some(false));
        assert!(pri_enter.hide_flyout_immediate);
        assert!(pri_enter.hide_start_immediate);

        // Pressing Win inside primary fullscreen atomically opens Start + Taskbar together (Option A)!
        let start_override =
            coord.request_toggle_start_on_monitor(None, Some("primary".into()), 300);
        assert_eq!(start_override.start_visible, Some(true));
        assert_eq!(start_override.taskbar_visible, Some(true));
        assert_eq!(start_override.emit_start_state, Some(true));
        assert_eq!(start_override.start_target_monitor.as_deref(), Some("primary"));
        assert!(start_override.hide_flyout_immediate);
        assert!(coord.is_start_open());
        assert!(coord.is_fullscreen_overlay_active());
        assert!(coord.should_show_taskbar());
        assert!(coord.should_apply_taskbar_bounds());

        // Leaving secondary fullscreen restores only secondary taskbar; primary stays in fullscreen overlay
        let sec_exit = coord.reevaluate_fullscreen_per_monitor(
            MonitorFullscreenState {
                primary: true,
                secondary: false,
            },
            400,
        );
        assert_eq!(sec_exit.taskbar_visible, None);
        assert_eq!(sec_exit.secondary_taskbar_visible, Some(true));
        assert!(coord.should_show_secondary_taskbar());
        assert!(coord.should_show_taskbar());

        // Closing Start atomically hides BOTH Start and primary Taskbar together!
        let start_close = coord.request_toggle_start(Some(false), 450);
        assert_eq!(start_close.start_visible, Some(false));
        assert_eq!(start_close.taskbar_visible, Some(false));
        assert!(!coord.is_start_open());
        assert!(!coord.is_fullscreen_overlay_active());
        assert!(!coord.should_show_taskbar());
    }

    #[test]
    fn fullscreen_option_a_atomic_open_quick_win_win_fg_freeze_and_flyout_handoff() {
        let mut coord = WindowCoordinator::new();

        // 1. Enter primary fullscreen game
        let enter_fs = coord.set_monitor_fullscreen(true, false, 100);
        assert_eq!(enter_fs.taskbar_visible, Some(false));
        assert!(coord.is_primary_fullscreen());
        assert!(!coord.should_show_taskbar());

        // 2. Press Win -> Start + Taskbar open atomically in ONE CoordinatorActions
        let open = coord.request_toggle_start(None, 200);
        assert_eq!(open.start_visible, Some(true));
        assert_eq!(open.taskbar_visible, Some(true));
        assert!(coord.is_start_open());
        assert!(coord.is_fullscreen_overlay_active());
        assert!(coord.should_show_taskbar());
        assert!(coord.should_apply_taskbar_bounds());

        // 3. 650ms poller sees TestBar Start as foreground (fg_is_own_process=true, primary=false) -> FROZEN!
        let frozen = coord.reevaluate_fullscreen_per_monitor_with_fg(
            MonitorFullscreenState {
                primary: false,
                secondary: false,
            },
            true,
            300,
        );
        assert_eq!(frozen, CoordinatorActions::default());
        assert!(coord.is_primary_fullscreen());
        assert!(coord.is_fullscreen_overlay_active());

        // 4. Quick Win-Win close: both Start and Taskbar hide atomically, and poller tick during exit stays frozen!
        let quick_close = coord.request_toggle_start(None, 350);
        assert_eq!(quick_close.start_visible, Some(false));
        assert_eq!(quick_close.taskbar_visible, Some(false));
        assert!(!coord.is_start_open());
        assert!(!coord.is_fullscreen_overlay_active());
        assert!(!coord.should_show_taskbar());

        let mid_exit_poll = coord.reevaluate_fullscreen_per_monitor_with_fg(
            MonitorFullscreenState {
                primary: false,
                secondary: false,
            },
            true,
            400,
        );
        assert_eq!(mid_exit_poll, CoordinatorActions::default());
        assert!(!coord.should_show_taskbar());

        // 5. Re-open Start over fullscreen, then open Sound flyout from the revealed Taskbar:
        //    Start closes, Flyout opens, Taskbar STAYS visible until Flyout closes!
        let open2 = coord.request_toggle_start(None, 600);
        assert_eq!(open2.start_visible, Some(true));
        assert_eq!(open2.taskbar_visible, Some(true));

        let open_osd = coord.request_open_flyout(FlyoutKind::Osd, 620);
        assert_eq!(open_osd.flyout_visible, Some(true));
        let close_osd = coord.request_close_osd();
        assert_eq!(close_osd.flyout_visible, Some(false));
        assert_eq!(close_osd.taskbar_visible, None, "closing OSD while Start is still open must keep taskbar visible");
        assert!(coord.is_fullscreen_overlay_active());

        let open_sound = coord.request_open_flyout(FlyoutKind::Sound, 650);
        assert_eq!(open_sound.start_visible, Some(false));
        assert_eq!(open_sound.flyout_visible, Some(true));
        assert_eq!(open_sound.taskbar_visible, None, "switching Start -> Sound flyout keeps revealed taskbar");
        assert!(coord.is_fullscreen_overlay_active());

        // Closing Sound flyout (last overlay) atomically hides taskbar back into fullscreen game!
        let close_sound = coord.request_open_flyout(FlyoutKind::Sound, 700);
        assert_eq!(close_sound.flyout_visible, Some(false));
        assert_eq!(close_sound.taskbar_visible, Some(false));
        assert!(!coord.is_fullscreen_overlay_active());
        assert!(!coord.should_show_taskbar());

        // 6. Open Start over fullscreen, then user switches to a normal desktop app:
        //    focus returns to non-fullscreen external app -> immediate re-eval restores Taskbar!
        let open3 = coord.request_toggle_start(None, 900);
        assert_eq!(open3.start_visible, Some(true));
        assert_eq!(open3.taskbar_visible, Some(true));
        let hide3 = coord.hide_start_immediate();
        assert_eq!(hide3.start_visible, Some(false));
        assert_eq!(hide3.taskbar_visible, Some(false));

        let after_app_focus = coord.reevaluate_fullscreen_per_monitor_with_fg(
            MonitorFullscreenState {
                primary: false,
                secondary: false,
            },
            false,
            950,
        );
        assert_eq!(after_app_focus.taskbar_visible, Some(true));
        assert!(!coord.is_primary_fullscreen());
        assert!(coord.should_show_taskbar());
    }

    #[test]
    fn flyout_focus_grace_monotonic_anim_gens_and_immediate_preemption() {
        let mut coord = WindowCoordinator::new();
        assert_eq!(coord.start_anim_gen(), 0);
        assert_eq!(coord.flyout_anim_gen(), 0);

        // Open interactive flyout at t=1000
        let open = coord.request_open_flyout(FlyoutKind::Sound, 1000);
        assert_eq!(open.flyout_visible, Some(true));
        assert!(open.hide_start_immediate);
        let g1 = open.flyout_anim_gen.expect("flyout_anim_gen set on open");
        assert_eq!(g1, 1);

        // Transient WebView2 focus jitter within 160ms grace window is ignored
        assert_eq!(
            coord.on_flyout_focus_lost(1000),
            CoordinatorActions::default()
        );
        assert_eq!(
            coord.on_flyout_focus_lost(1000 + FLYOUT_OPEN_FOCUS_GRACE_MS - 1),
            CoordinatorActions::default()
        );
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Sound));

        // Explicit global outside click STILL dismisses immediately even inside grace window
        let click_out = coord.on_global_mouse_down(50, 50, &[], 1050);
        assert_eq!(click_out.flyout_visible, Some(false));
        assert_eq!(click_out.flyout_anim_gen, Some(2));
        assert_eq!(coord.active_flyout(), None);

        // Re-open after debounce and verify focus-loss succeeds right at FLYOUT_OPEN_FOCUS_GRACE_MS
        let reopen = coord.request_open_flyout(FlyoutKind::Calendar, 1500);
        assert_eq!(reopen.flyout_anim_gen, Some(3));
        let lost = coord.on_flyout_focus_lost(1500 + FLYOUT_OPEN_FOCUS_GRACE_MS);
        assert_eq!(lost.flyout_visible, Some(false));
        assert_eq!(lost.flyout_anim_gen, Some(4));

        // Immediate hide preemption helpers bump generation monotonically even when already closed
        let h_fly = coord.hide_flyout_immediate();
        assert!(h_fly.hide_flyout_immediate);
        assert_eq!(h_fly.flyout_anim_gen, Some(5));

        let s_open = coord.request_toggle_start_on_monitor(Some(true), Some("secondary".into()), 2000);
        assert_eq!(s_open.start_target_monitor.as_deref(), Some("secondary"));
        let sg1 = s_open.start_anim_gen.unwrap();
        let h_start = coord.hide_start_immediate();
        assert!(h_start.hide_start_immediate);
        assert_eq!(h_start.start_visible, Some(false));
        assert!(h_start.start_anim_gen.unwrap() > sg1);
        assert!(!coord.is_start_open());
    }

    #[test]
    fn set_monitor_fullscreen_advances_anim_gens_even_when_surfaces_already_closed() {
        let mut coord = WindowCoordinator::new();
        assert_eq!(coord.start_anim_gen(), 0);
        assert_eq!(coord.flyout_anim_gen(), 0);

        // Entering primary fullscreen while both Start and Flyout are ALREADY closed must still
        // advance both anim_gen counters so AppState and WindowCoordinator never desync.
        let fs_enter = coord.set_monitor_fullscreen(true, false, 1000);
        assert!(fs_enter.hide_start_immediate);
        assert!(fs_enter.hide_flyout_immediate);
        assert_eq!(fs_enter.start_anim_gen, Some(1));
        assert_eq!(fs_enter.flyout_anim_gen, Some(1));
        assert_eq!(coord.start_anim_gen(), 1);
        assert_eq!(coord.flyout_anim_gen(), 1);

        // Subsequent Start open (fullscreen Win override) and Flyout open (after fullscreen exit)
        // emit strictly greater generations without duplicating or regressing gen IDs.
        let start_open = coord.request_toggle_start_on_monitor(
            Some(true),
            Some("secondary".into()),
            1200,
        );
        assert_eq!(start_open.start_anim_gen, Some(2));
        assert_eq!(start_open.flyout_anim_gen, Some(2));

        // Close Start cleanly so start_open == false while start_target_monitor stays Some("secondary")
        let start_close = coord.request_toggle_start(Some(false), 1400);
        assert_eq!(start_close.start_anim_gen, Some(3));
        assert!(!coord.is_start_open());

        // Entering secondary fullscreen when Start was already closed on secondary must also bump start_anim_gen
        let sec_fs_enter = coord.set_monitor_fullscreen(true, true, 1600);
        assert!(sec_fs_enter.hide_start_immediate);
        assert_eq!(sec_fs_enter.start_anim_gen, Some(4));
        assert_eq!(coord.start_anim_gen(), 4);

        // Leave fullscreen and verify Flyout open continues strictly monotonically
        let _ = coord.set_monitor_fullscreen(false, false, 1800);
        let fly_open = coord.request_open_flyout(FlyoutKind::Sound, 2000);
        assert_eq!(fly_open.flyout_anim_gen, Some(3));
        assert!(fly_open.start_anim_gen.unwrap() > 4);
    }
}
