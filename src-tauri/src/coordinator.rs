use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Minimum delay in milliseconds after a focus-loss or click-outside dismissal
/// before the same trigger can re-open the surface (prevents double-toggle).
pub const REOPEN_DEBOUNCE_MS: u64 = 180;

/// Grace window in milliseconds immediately after opening Start during which
/// transient WebView2/Win-key `Focused(false)` jitter does not dismiss Start.
pub const START_OPEN_FOCUS_GRACE_MS: u64 = 200;

/// Win32 `WS_CAPTION` (`WS_BORDER | WS_DLGFRAME`) bitmask.
pub const WS_CAPTION_FLAG: u32 = 0x00C0_0000;
/// Win32 `WS_THICKFRAME` bitmask.
pub const WS_THICKFRAME_FLAG: u32 = 0x0004_0000;

/// Logical reserved AppBar heights (AD-2).
pub const DOCK_APPBAR_LOGICAL_HEIGHT: i32 = 64;
pub const EDGE_APPBAR_LOGICAL_HEIGHT: i32 = 52;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlyoutKind {
    Preview,
    Tray,
    #[serde(alias = "quick")]
    Sound,
    Calendar,
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
        matches!(self, FlyoutKind::Tray | FlyoutKind::Sound | FlyoutKind::Calendar)
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

/// Single authoritative fullscreen classifier (AD-6).
///
/// Returns `true` ONLY when:
/// - `SHQueryUserNotificationState` reports `QUNS_RUNNING_D3D_FULL_SCREEN` or `QUNS_PRESENTATION_MODE`, OR
/// - The visible foreground window belongs to another non-shell process, has neither
///   full `WS_CAPTION` nor `WS_THICKFRAME`, and covers the entire primary monitor.
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
    if matches!(
        fg.class_name.as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    ) {
        return false;
    }

    let has_caption = (fg.style & WS_CAPTION_FLAG) == WS_CAPTION_FLAG;
    let has_thickframe = (fg.style & WS_THICKFRAME_FLAG) != 0;
    if has_caption || has_thickframe {
        return false;
    }

    fg.rect.covers(primary_monitor)
}

/// Computes the DPI-scaled bottom AppBar reservation rect in physical pixels (AD-2).
pub fn compute_appbar_rect(primary_monitor: PixelRect, dpi: u32, bar_mode: BarMode) -> PixelRect {
    let effective_dpi = if dpi == 0 { 96 } else { dpi };
    let logical_height = match bar_mode {
        BarMode::Dock => DOCK_APPBAR_LOGICAL_HEIGHT,
        BarMode::Edge => EDGE_APPBAR_LOGICAL_HEIGHT,
    };
    let reserve_px = (((logical_height as f64) * (effective_dpi as f64) / 96.0).round() as i32)
        .clamp(0, primary_monitor.height());
    PixelRect {
        left: primary_monitor.left,
        top: primary_monitor.bottom - reserve_px,
        right: primary_monitor.right,
        bottom: primary_monitor.bottom,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoordinatorActions {
    pub taskbar_visible: Option<bool>,
    pub start_visible: Option<bool>,
    pub flyout_visible: Option<bool>,
    pub flyout_non_activating: Option<bool>,
    pub emit_flyout_show: bool,
    pub emit_flyout_hide: bool,
    pub emit_start_state: Option<bool>,
}

/// Pure, Win32-independent state machine owning lifecycle, focus, preemption,
/// anti-double-toggle debounce, modal picker guard, and fullscreen suppression
/// for the 3 pre-warmed TestBar windows (`taskbar`, `start`, `flyout`).
#[derive(Debug, Default)]
pub struct WindowCoordinator {
    start_open: bool,
    active_flyout: Option<FlyoutKind>,
    fullscreen_suppressed: bool,
    modal_picker_depth: u32,
    last_dismissed_ms: HashMap<TriggerId, u64>,
    last_start_opened_ms: Option<u64>,
}

impl WindowCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_start_open(&self) -> bool {
        self.start_open
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

    pub fn should_show_taskbar(&self) -> bool {
        !self.fullscreen_suppressed
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

    fn is_debounced(&self, trigger: TriggerId, now_ms: u64) -> bool {
        self.last_dismissed_ms
            .get(&trigger)
            .map(|&dismissed_at| now_ms >= dismissed_at && (now_ms - dismissed_at) < REOPEN_DEBOUNCE_MS)
            .unwrap_or(false)
    }

    pub fn request_toggle_start(&mut self, force: Option<bool>, now_ms: u64) -> CoordinatorActions {
        let target = force.unwrap_or(!self.start_open);
        if target == self.start_open {
            return CoordinatorActions::default();
        }
        if target && self.fullscreen_suppressed {
            return CoordinatorActions::default();
        }

        let mut actions = CoordinatorActions::default();
        if target {
            if force.is_none() && self.is_debounced(TriggerId::Start, now_ms) {
                return actions;
            }
            if self.active_flyout.is_some() {
                self.active_flyout = None;
                actions.flyout_visible = Some(false);
                actions.emit_flyout_hide = true;
            }
            self.start_open = true;
            self.last_start_opened_ms = Some(now_ms);
            actions.start_visible = Some(true);
            actions.emit_start_state = Some(true);
        } else {
            self.start_open = false;
            self.last_start_opened_ms = None;
            actions.start_visible = Some(false);
            actions.emit_start_state = Some(false);
        }
        actions
    }

    pub fn request_open_flyout(&mut self, kind: FlyoutKind, now_ms: u64) -> CoordinatorActions {
        if self.fullscreen_suppressed {
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
                self.active_flyout = None;
                actions.flyout_visible = Some(false);
                actions.emit_flyout_hide = true;
                return actions;
            }
            if self.active_flyout.map_or(true, |a| a.is_non_activating())
                && self.is_debounced(TriggerId::Flyout(kind), now_ms)
            {
                return actions;
            }
            if self.start_open {
                self.start_open = false;
                self.last_start_opened_ms = None;
                actions.start_visible = Some(false);
                actions.emit_start_state = Some(false);
            }
        }

        self.active_flyout = Some(kind);
        actions.flyout_visible = Some(true);
        actions.flyout_non_activating = Some(kind.is_non_activating());
        actions.emit_flyout_show = true;
        actions
    }

    pub fn request_close_flyout(&mut self) -> CoordinatorActions {
        if self.active_flyout.is_none() {
            return CoordinatorActions::default();
        }
        self.active_flyout = None;
        CoordinatorActions {
            flyout_visible: Some(false),
            emit_flyout_hide: true,
            ..Default::default()
        }
    }

    pub fn request_close_preview(&mut self) -> CoordinatorActions {
        if self.active_flyout == Some(FlyoutKind::Preview) {
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
        self.start_open = false;
        self.last_start_opened_ms = None;
        self.last_dismissed_ms.insert(TriggerId::Start, now_ms);
        CoordinatorActions {
            start_visible: Some(false),
            emit_start_state: Some(false),
            ..Default::default()
        }
    }

    pub fn on_flyout_focus_lost(&mut self, now_ms: u64) -> CoordinatorActions {
        match self.active_flyout {
            Some(kind) if kind.is_interactive_click() => {
                self.active_flyout = None;
                self.last_dismissed_ms
                    .insert(TriggerId::Flyout(kind), now_ms);
                CoordinatorActions {
                    flyout_visible: Some(false),
                    emit_flyout_hide: true,
                    ..Default::default()
                }
            }
            _ => CoordinatorActions::default(),
        }
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
            self.start_open = false;
            self.last_start_opened_ms = None;
            self.last_dismissed_ms.insert(TriggerId::Start, now_ms);
            actions.start_visible = Some(false);
            actions.emit_start_state = Some(false);
        }
        if let Some(kind) = self.active_flyout.take() {
            self.last_dismissed_ms
                .insert(TriggerId::Flyout(kind), now_ms);
            actions.flyout_visible = Some(false);
            actions.emit_flyout_hide = true;
        }
        actions
    }

    pub fn on_fullscreen_changed(&mut self, is_fullscreen: bool) -> CoordinatorActions {
        if is_fullscreen == self.fullscreen_suppressed {
            return CoordinatorActions::default();
        }
        self.fullscreen_suppressed = is_fullscreen;

        let mut actions = CoordinatorActions::default();
        if is_fullscreen {
            actions.taskbar_visible = Some(false);
            if self.start_open {
                self.start_open = false;
                self.last_start_opened_ms = None;
                actions.start_visible = Some(false);
                actions.emit_start_state = Some(false);
            }
            if self.active_flyout.is_some() {
                self.active_flyout = None;
                actions.flyout_visible = Some(false);
                actions.emit_flyout_hide = true;
            }
        } else {
            actions.taskbar_visible = Some(true);
        }
        actions
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
            (96, BarMode::Dock, 64),
            (120, BarMode::Dock, 80),
            (144, BarMode::Dock, 96),
            (192, BarMode::Dock, 128),
            (0, BarMode::Dock, 64),
            (96, BarMode::Edge, 52),
            (120, BarMode::Edge, 65),
            (144, BarMode::Edge, 78),
            (192, BarMode::Edge, 104),
            (0, BarMode::Edge, 52),
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
        assert_eq!(windows.len(), 3, "expected exactly 3 pre-warmed windows");

        for expected_label in ["taskbar", "start", "flyout"] {
            let win = windows
                .iter()
                .find(|w| w["label"].as_str() == Some(expected_label))
                .unwrap_or_else(|| panic!("missing window '{expected_label}'"));
            assert_eq!(win["resizable"].as_bool(), Some(false), "{expected_label} must have resizable: false");
            assert_eq!(win["decorations"].as_bool(), Some(false), "{expected_label} must have decorations: false");
            assert_eq!(win["transparent"].as_bool(), Some(true), "{expected_label} must have transparent: true");
            assert_eq!(win["shadow"].as_bool(), Some(false), "{expected_label} must have shadow: false");
            assert_eq!(win["skipTaskbar"].as_bool(), Some(true), "{expected_label} must have skipTaskbar: true");
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

        // Blocked while fullscreen_suppressed == true
        let _ = coord.on_fullscreen_changed(true);
        assert!(!coord.is_start_open());
        assert_eq!(
            coord.request_toggle_start(None, 2000),
            CoordinatorActions::default()
        );
        assert_eq!(
            coord.request_toggle_start(Some(true), 2000),
            CoordinatorActions::default()
        );
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

        // Transient Preview/Osd opening within 180ms after Sound blur does NOT bypass Sound debounce
        let _ = coord.request_open_flyout(FlyoutKind::Sound, 1000);
        let _ = coord.on_flyout_focus_lost(1100);
        let prev = coord.request_open_flyout(FlyoutKind::Preview, 1150);
        assert_eq!(prev.flyout_visible, Some(true));
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Preview));
        assert_eq!(
            coord.request_open_flyout(FlyoutKind::Sound, 1200),
            CoordinatorActions::default()
        );
        assert_eq!(coord.active_flyout(), Some(FlyoutKind::Preview));

        // After 180ms window, Sound preempts Preview
        let reopened = coord.request_open_flyout(FlyoutKind::Sound, 1280);
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
}
