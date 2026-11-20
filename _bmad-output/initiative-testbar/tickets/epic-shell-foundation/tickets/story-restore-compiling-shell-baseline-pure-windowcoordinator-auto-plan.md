---
title: 'Restore compiling shell baseline, pure WindowCoordinator & automated test harness'
type: 'feature'
ticket: '1'
created: '2026-11-20'
status: 'built'
baseline_revision: '5de4961acf2b23e2202220af3e1f613ba7adb4ea'
route: 'full'
route_source: 'auto'
risk: 'high'
review: 'thorough'
review_source: 'pinned'
lenses_ran: ['blind-hunter', 'edge-case-hunter', 'verification-gap', 'intent-alignment']
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md'
warnings: []
deferred:
  - summary: >-
      Pre-existing Preview hover-leave handler in FlyoutView calls close_flyout unconditionally instead of close_preview_if_outside.
    evidence: |-
      src/components/FlyoutView.tsx:324 already had onMouseLeave={close} before this diff; leaving a hover preview directly onto a taskbar click-flyout trigger could close a newly opened interactive flyout. Owned by Epic 2 hover-preview polish.
    location: >-
      src/components/FlyoutView.tsx:324
    severity: low
  - summary: >-
      Verify live WebView2 child Chromium HWND WM_MOUSEACTIVATE routing and strict local CSP enforcement under interactive testbar.exe execution.
    evidence: |-
      Unit tests verified top-level HWND WM_MOUSEACTIVATE subclassing and static tauri.conf.json CSP, but WebView2 child Chromium HWNDs (Chrome_WidgetWin_*) and runtime WebView2 resource loading under the strict local CSP can only be confirmed by running testbar.exe --no-hide-native interactively on Windows 11.
    location: >-
      src-tauri/src/win_shell.rs:212
    severity: medium (unverified)
---

<intent-contract>

## Intent

**Problem:** The brownfield TestBar Rust crate fails to compile (`win_shell::set_window_bounds` missing, `ShellHookEvent::GlobalMouseDown` unhandled), hides windows by shrinking to `1×1` at `(0,0)`, toggles `GWL_EXSTYLE` per flyout open, uses `resizable: true` and `csp: null` in `tauri.conf.json`, over-triggers fullscreen on `QUNS_BUSY` / normal windows, and lacks a frontend test harness.

**Approach:** Introduce a Win32-independent `WindowCoordinator` state machine with strict fullscreen classification and DPI AppBar rect math in `src-tauri/src/coordinator.rs`, wire `lib.rs` and `win_shell.rs` to show/hide via visibility and `WM_MOUSEACTIVATE` subclassing with off-hook `GlobalMouseDown` dispatch, harden `tauri.conf.json`, preserve canonical + brownfield flyout/event compatibility, and install Vitest with `layoutMap.ts` tests.

## Boundaries & Constraints

**Always:**
- Keep `coordinator.rs` 100% Win32-independent (zero `windows::` imports) and unit-tested.
- Keep `resizable: false`, `decorations: false`, `transparent: true`, `shadow: false`, `skipTaskbar: true` on all 3 windows (`taskbar`, `start`, `flyout`) and `security.csp` set to `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'`.
- Show/hide `start` and `flyout` via visibility (`SW_SHOW` / `SW_SHOWNOACTIVATE` / `SW_HIDE`) — zero `1×1` shrinking.
- Handle non-activating `flyout` (`preview`/`osd`) via subclassed `WM_MOUSEACTIVATE` returning `MA_NOACTIVATE` without flipping `GWL_EXSTYLE` on every open.
- Dispatch `ShellHookEvent::GlobalMouseDown` off the `WH_MOUSE_LL` callback thread (zero blocking IPC/Win32 on hook thread).
- Preserve serde aliases on `FlyoutKind` (`quick` -> `Sound`, `ctx_app` -> `ContextApp`, `ctx_taskbar` -> `ContextBar`) and compatible event handling across `TaskbarView`, `StartView`, and `FlyoutView`.

**Never:**
- Do not implement single-instance mutex, top-level `TaskbarCreated` message window, or `--watchdog --workarea` rewrite (Story 1.2).
- Do not rewrite the full `WH_KEYBOARD_LL`/`WH_MOUSE_LL` bounded `sync_channel(64)` architecture (Story 1.3).
- Do not change `config.rs` schema or add `PinItem` fields (Story 1.4).
- Do not modify `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, or `system_widgets.rs` beyond keeping them compiling.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Hover/OSD preemption | Click flyout open (`Tray`/`Sound`/`Calendar`/`ContextApp`/`ContextBar`); request `Preview` or `Osd` | Request ignored; active click flyout stays open | No error |
| Focus-loss anti-double-toggle | `start` or interactive `flyout` dismissed on focus-loss at `t`; re-click arrives at `t + 100ms` (`< 180ms`) | Re-open suppressed; re-click at `t + 180ms` succeeds | No error |
| Modal picker focus guard | `start` open; `begin_modal_picker()` active (`pick_folder`/`pick_file`); `start` loses focus | Dismissal suppressed; `start` stays open; RAII guard ends modal state even on panic | Always calls `end_modal_picker` on drop |
| Fullscreen suppression leak | `fullscreen_suppressed == true`; taskbar bounds update arrives (`resize_taskbar_dock`/`set_bar_mode`) | Bounds updated without forcing `taskbar` visible | No error |
| Strict fullscreen classifier | `QUNS_RUNNING_D3D_FULL_SCREEN` or `QUNS_PRESENTATION_MODE`, OR non-shell non-TestBar frameless window (no `WS_CAPTION`, no `WS_THICKFRAME`) covering primary monitor | Returns `true`; `QUNS_BUSY`, `Progman`, `WorkerW`, own process, or windows with `WS_CAPTION`/`WS_THICKFRAME` return `false` | Missing/invisible foreground window returns `false` |
| DPI AppBar rect math | Primary monitor rect + DPI (`96`, `120`, `144`, `192`) + `Dock`/`Edge` mode | Dock reserves `64px` logical height scaled by `dpi/96`; Edge reserves `52px` logical height | `dpi == 0` falls back to `96` |
| Flyout kind alias & layout swap | Deserialize `"quick"`, `"ctx_app"`, `"ctx_taskbar"`; `matchesQuery("ntkt", "Telegram")` | Deserializes to `Sound`, `ContextApp`, `ContextBar`; `matchesQuery` returns `{ matched: true, swapped: "теле" }` | Invalid kind returns serde error |

</intent-contract>

## Code Map

- `src-tauri/src/coordinator.rs` (new) -- Pure Win32-independent `WindowCoordinator`, `FlyoutKind` (canonical snake_case + serde aliases), `TriggerId`, `BarMode`, `NotificationState`, `PixelRect`, `ForegroundWindowInfo`, `classify_fullscreen`, `compute_appbar_rect`, and unit tests (plus `tauri.conf.json` validation test).
- `src-tauri/src/win_shell.rs` -- Add `position_window(hwnd, x, y, w, h)` (uses `SetWindowPos` with `SWP_NOACTIVATE`, never `SWP_SHOWWINDOW`), `install_flyout_noactivate_subclass(hwnd)` + `set_flyout_non_activating(bool)` using `GWLP_WNDPROC` + `CallWindowProcW` returning `MA_NOACTIVATE` on `WM_MOUSEACTIVATE`; update `is_fullscreen_active()` to delegate to `coordinator::classify_fullscreen()` using `GWL_STYLE` + `SHQueryUserNotificationState`; keep existing `set_window_visible`, `style_overlay_window`, `point_inside_hwnd`, `register_appbar`, and hook thread intact (do not rewrite Story 1.2/1.3 items).
- `src-tauri/src/lib.rs` -- Register `pub mod coordinator;`, store `Mutex<WindowCoordinator>` in `AppState`, replace all 5 broken `set_window_bounds` calls (removing both `1×1` shrinks at lines 99 & 109 and removing per-open `style_overlay_window` at line 225), install `flyout` subclass once at setup, wire `pick_folder`/`pick_file` with an RAII modal guard, wire `open_flyout`/`close_flyout`/`toggle_start`/`on_window_event`/fullscreen poller through `WindowCoordinator`, use `compute_appbar_rect` for AppBar reserve height, handle `ShellHookEvent::GlobalMouseDown(x, y)` (and `ToggleStart`) off the hook thread via `run_on_main_thread`, and emit canonical (`start-state`, `flyout-hide`) alongside legacy (`start-visibility`, `flyout-hidden`) events.
- `src-tauri/tauri.conf.json` -- Change `resizable` to `false` on `taskbar`, `start`, and `flyout` (lines 26, 41, 56); set `app.security.csp` (line 62) to `"default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'"`.
- `src/lib/ipc.ts` -- Expand `FlyoutRequest.kind` union to include canonical (`'sound' | 'context_app' | 'context_bar' | 'osd'`) alongside legacy aliases (`'quick' | 'ctx_app' | 'ctx_taskbar'`).
- `src/components/TaskbarView.tsx`, `src/components/StartView.tsx`, `src/components/FlyoutView.tsx` -- Listen to canonical `'start-state'` and `'flyout-hide'` events and match both canonical and legacy `FlyoutKind` values (`sound`/`quick`, `context_app`/`ctx_app`, `context_bar`/`ctx_taskbar`).
- `package.json` & `src/lib/layoutMap.test.ts` (new) -- Add `vitest` devDependency, `"test": "vitest run"` script, and unit tests for `src/lib/layoutMap.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/src/coordinator.rs` -- Create pure `WindowCoordinator` (visibility, preemption, `180ms` per-trigger debounce, modal guard, fullscreen suppression, global click-outside), `FlyoutKind` with serde aliases, `classify_fullscreen`, `compute_appbar_rect`, and comprehensive unit tests (including `tauri.conf.json` assertions) -- satisfies R2, R3, R5 with zero Win32 coupling.
- [x] `src-tauri/src/win_shell.rs` -- Add `position_window`, `install_flyout_noactivate_subclass` / `set_flyout_non_activating` (`WM_MOUSEACTIVATE` -> `MA_NOACTIVATE`), and refactor `is_fullscreen_active()` to gather Win32 state and delegate to `coordinator::classify_fullscreen` -- eliminates `1×1` shrinking, per-open `GWL_EXSTYLE` flips, and `QUNS_BUSY` false positives.
- [x] `src-tauri/src/lib.rs` -- Wire `AppState` to `WindowCoordinator`, replace all `set_window_bounds` calls with `position_window`/`set_window_visible`, install `flyout` subclass once, wrap `pick_folder`/`pick_file` in RAII modal guard, dispatch `ShellHookEvent::GlobalMouseDown` off the hook thread onto the main GUI thread, and emit canonical + legacy events -- restores clean compilation and full coordinator authority.
- [x] `src-tauri/tauri.conf.json` -- Set `"resizable": false` for `taskbar`, `start`, and `flyout`, and set strict local `security.csp` -- removes `WS_THICKFRAME` border artifacts and locks WebView2 CSP.
- [x] `src/lib/ipc.ts`, `src/components/TaskbarView.tsx`, `src/components/StartView.tsx`, `src/components/FlyoutView.tsx` -- Update `FlyoutRequest.kind` types, canonical event listeners (`start-state`, `flyout-hide`), and canonical+legacy kind matching -- keeps brownfield views working cleanly with canonical Rust `FlyoutKind` serialization.
- [x] `package.json`, `src/lib/layoutMap.test.ts` -- Install `vitest`, add `"test": "vitest run"`, and add unit tests covering `swapLayout` and `matchesQuery` (including `ntkt` -> Telegram) -- establishes automated frontend test harness.

**Acceptance Criteria:**
- Given `src-tauri/tauri.conf.json` and `src-tauri/src/`, when checked statically and by unit tests, then zero `"resizable": true` entries and zero `set_window_bounds(..., 1, 1, ...)` calls exist, `security.csp` equals `"default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'"`, and `flyout` handles `preview`/`osd` non-activation via `WM_MOUSEACTIVATE` without calling `style_overlay_window`/`GWL_EXSTYLE` per open.
- Given the workspace after all edits, when `pnpm build`, `pnpm test`, and `cargo test --manifest-path src-tauri/Cargo.toml` are executed non-interactively, then all three commands exit `0` with passing Rust tests (`coordinator`, `config`, `integration_tests`) and passing Vitest tests (`layoutMap.test.ts`).

## Implementation Notes

- Implemented `src-tauri/src/coordinator.rs` as a pure, Win32-independent state machine with `WindowCoordinator`, `CoordinatorActions`, `ModalPickerGuard`, `FlyoutKind` (with serde aliases `quick`, `ctx_app`, `ctx_taskbar`), `classify_fullscreen`, `compute_appbar_rect`, and 19 unit tests (including `tauri.conf.json` validation).
- Wired `src-tauri/src/win_shell.rs` (`position_window`, `get_window_pixel_rect`, `install_flyout_noactivate_subclass`, `set_flyout_non_activating`, `is_fullscreen_active` delegating to `classify_fullscreen`) and added 2 Win32 unit tests covering `position_window` non-leak visibility and `WM_MOUSEACTIVATE` subclass behavior.
- Wired `src-tauri/src/lib.rs` through `WindowCoordinator`, marshaling hook/fullscreen background events via `app.run_on_main_thread`, gating idle `GlobalMouseDown` clicks with `Arc<AtomicBool>`, anchoring coordinator timestamps to monotonic `Instant`, and dual-emitting canonical (`start-state`, `flyout-hide`) and legacy (`start-visibility`, `flyout-hidden`) events.
- Installed `vitest` (`^5.1.1`) in `package.json` with `"test": "vitest run"` and added 12 unit tests in `src/lib/layoutMap.test.ts`.
- All 7 I/O & Edge-Case Matrix rows verified by executed passing tests.

## Plan Change Log

## Review Triage Log

### 2026-11-20 — Review pass
- verdicts: 33 findings — high 7, medium 15, low 8, false 2, maybe-false 1
- findings:
  - `[high]` `[patch]` (blind-hunter) Unlocked state-to-Win32 race and cross-thread UI calls in `lib.rs` — Marshaled `ToggleStart`, `GlobalMouseDown`, `EmergencyRestore`, and `h_fast` fullscreen transitions onto the main GUI thread via `app.run_on_main_thread(...)`.
  - `[medium]` `[patch]` (blind-hunter) Non-monotonic `SystemTime` clock for anti-double-toggle debounce — Added `monotonic_ms()` anchored to `OnceLock<Instant>` in `lib.rs` and fixed `is_debounced` in `coordinator.rs` to check `now_ms >= dismissed_at`.
  - `[medium]` `[patch]` (blind-hunter) Async `GlobalMouseDown` racing past `ModalPickerGuard` and Start losing focus after dialog — Marshaled `GlobalMouseDown` via `run_on_main_thread` and re-focused `"start"` (if open) before `_modal_guard` drops in `pick_folder`/`pick_file`.
  - `[high]` `[patch]` (blind-hunter) Fullscreen suppression does not block opening `start` or `flyout` — Blocked `request_toggle_start` (when opening) and `request_open_flyout` when `fullscreen_suppressed` is `true`, with unit tests.
  - `[medium]` `[patch]` (blind-hunter) Flyout reopen debounce bypassed while a non-activating flyout is active — Changed `request_open_flyout` guard to `self.active_flyout.map_or(true, |a| a.is_non_activating()) && self.is_debounced(...)`, with unit test.
  - `[low]` `[defer]` (blind-hunter) Context menus debounced on dismissal and `Preview` `onMouseLeave` calls `close_flyout` unconditionally — Per-trigger debounce on `flyout:<kind>` matches AD-1; `FlyoutView.tsx:324` `onMouseLeave={close}` is pre-existing (not modified in this diff) and deferred to Epic 2.
  - `[false]` `[reject]` (blind-hunter) Missing `FlyoutKind::Osd` UI in `FlyoutView.tsx` and `Osd`+`Start` overlap — Refuted: OSD UI belongs to Epic 3 (`epic-tray-audio-calendar`, out of Story 1.1 scope) with zero current callers, and AD-1 only forbids `Osd` from preempting interactive click flyouts.
  - `[medium]` `[patch]` (blind-hunter) `position_flyout_window` `gap_logical` ignores `.flyout-root`'s `12px` bottom padding — Adjusted `gap_logical` to `40.0`/`50.0` (Edge) and `52.0`/`62.0` (Dock) so Preview sits flush and click flyouts sit `10px` above taskbar.
  - `[low]` `[patch]` (blind-hunter) `install_flyout_noactivate_subclass` failure cleanup, `hwnd` check, and unreset `FLYOUT_NON_ACTIVATING` on hide — Cleared `FLYOUT_ORIG_WNDPROC` on `prev == 0`, verified `hwnd` matches `FLYOUT_SUBCLASSED_HWND` in `flyout_wnd_proc`, and reset `set_flyout_non_activating(false)` on hide.
  - `[low]` `[patch]` (blind-hunter) Inclusive `PixelRect::contains` and per-click Tokio task spawn when idle — Switched `PixelRect::contains` to half-open `[left, right) × [top, bottom)` and added `Arc<AtomicBool>` (`overlay_open`) fast-path so idle clicks do zero dispatch; taskbar-rect inclusion matches AD-1.
  - `[low]` `[patch]` (blind-hunter) Duplicate `ABM_NEW` in `register_appbar` when called from `set_bar_mode` with same HWND — Guarded `SHAppBarMessage(ABM_NEW, ...)` with `if prev != hwnd_raw`.
  - `[medium]` `[patch]` (blind-hunter) `is_fullscreen_active` 64-char class buffer, `IsIconic` exclusion, and missing unit tests — Widened `cls_buf` to `[0u16; 256]`, excluded `IsIconic` foreground windows, and added missing Rust and Vitest unit tests.
  - `[high]` `[patch]` (edge-case-hunter) `request_open_flyout` while `fullscreen_suppressed` is true — Fixed alongside blind-hunter finding #4; returns `CoordinatorActions::default()`.
  - `[high]` `[patch]` (edge-case-hunter) `request_toggle_start` opens Start while `fullscreen_suppressed` is true — Fixed alongside blind-hunter finding #4; returns `CoordinatorActions::default()`.
  - `[medium]` `[patch]` (edge-case-hunter) `now_ms < dismissed_at` backward clock step locks out debounce — Fixed alongside blind-hunter finding #2 with `now_ms >= dismissed_at` and monotonic clock.
  - `[medium]` `[patch]` (edge-case-hunter) `Preview`/`Osd` opens within `180ms` bypassing interactive flyout debounce — Fixed alongside blind-hunter finding #5.
  - `[low]` `[patch]` (edge-case-hunter) `compute_appbar_rect` `reserve_px` unclamped against monitor height — Clamped `reserve_px` to `0..=primary_monitor.height()` and added unit test.
  - `[high]` `[patch]` (edge-case-hunter) Concurrent threads releasing coordinator lock before `apply_coordinator_actions` — Fixed alongside blind-hunter finding #1 via `app.run_on_main_thread`.
  - `[high]` `[patch]` (edge-case-hunter) `GlobalMouseDown` snapshotting `IsWindowVisible` rects out of order across threads — Fixed alongside blind-hunter finding #1 via `app.run_on_main_thread`.
  - `[low]` `[patch]` (edge-case-hunter) `position_flyout_window` unvalidated non-finite `anchor_x_logical` and out-of-range `width`/`height` — Guarded `!req.anchor_x_logical.is_finite()` and clamped `width`/`height` to `1..=2000`.
  - `[low]` `[patch]` (edge-case-hunter) Case-sensitive `bar_mode == "edge"` in `position_flyout_window` — Replaced with `BarMode::from_config_str` in both `position_flyout_window` and `position_taskbar_window`.
  - `[false]` `[reject]` (edge-case-hunter) Missing `'osd'` branch in `FlyoutView.tsx` — Refuted alongside blind-hunter finding #7: OSD UI is owned by Epic 3 and unreachable in Story 1.1.
  - `[high]` `[patch]` (edge-case-hunter) Removed brownfield `220ms` post-open focus-loss grace guard on Start — Added private `last_start_opened_ms` with `200ms` grace window in `on_start_focus_lost` (preserving `on_global_mouse_down` and explicit close) plus unit test.
  - `[medium]` `[patch]` (verification-gap) `request_toggle_start` missing tests for closing Start, flyout preemption, and `force: Some(true)` — Added `start_toggle_close_preemption_force_and_fullscreen_block` test in `coordinator.rs`.
  - `[medium]` `[patch]` (verification-gap) `request_open_flyout` missing tests for `Preview` suppression and `Osd` allowance while Start open — Added `flyout_open_start_interaction_debounce_and_fullscreen_block` test in `coordinator.rs`.
  - `[medium]` `[patch]` (verification-gap) `request_close_preview` untested — Added `close_preview_and_non_activating_focus_loss` test in `coordinator.rs`.
  - `[medium]` `[patch]` (verification-gap) `on_flyout_focus_lost` missing tests verifying `Preview`/`Osd` ignore focus loss — Covered in `close_preview_and_non_activating_focus_loss` test in `coordinator.rs`.
  - `[medium]` `[patch]` (verification-gap) `on_global_mouse_down` missing tests for dismissing active flyouts and recording flyout debounce — Added `global_mouse_down_dismisses_flyout_debounces_and_uses_half_open_rect` test in `coordinator.rs`.
  - `[medium]` `[patch]` (verification-gap) `on_fullscreen_changed` missing tests for hiding active flyouts and deduplicating repeated states — Added `fullscreen_hides_active_flyout_and_deduplicates_repeated_states` test in `coordinator.rs`.
  - `[medium]` `[patch]` (verification-gap) Fullscreen bounds-update test did not verify `win_shell::position_window` — Added Win32 unit test `win_shell::tests::position_window_updates_rect_without_showing_hidden_window`.
  - `[low]` `[patch]` (verification-gap) `BarMode::from_config_str` untested — Added `bar_mode_from_config_str_parsing` test in `coordinator.rs`.
  - `[medium]` `[patch]` (verification-gap) Flyout `WM_MOUSEACTIVATE` non-activating subclass untested — Added Win32 unit test `win_shell::tests::flyout_subclass_handles_wm_mouseactivate_by_mode`.
  - `[maybe-false]` `[defer]` (intent-alignment) Live WebView2 child Chromium HWND `WM_MOUSEACTIVATE` routing and runtime strict CSP loading cannot be proven by unit tests alone — Deferred for interactive runtime smoke test of `testbar.exe --no-hide-native` on Windows 11.

## Design Notes

- **Flyout `WM_MOUSEACTIVATE` subclassing:** Subclass `flyout` once during Tauri `setup` via `SetWindowLongPtrW(hwnd, GWLP_WNDPROC, flyout_wnd_proc as isize)` storing the original proc pointer in an `AtomicIsize` (guarded so re-install is idempotent). Control non-activation via an `AtomicBool` (`FLYOUT_NON_ACTIVATING`) toggled before showing `flyout`: `true` for `Preview | Osd`, `false` otherwise. In `flyout_wnd_proc`, return `LRESULT(MA_NOACTIVATE as isize)` on `WM_MOUSEACTIVATE` when the flag is `true`; otherwise forward to `CallWindowProcW`.
- **Canonical vs legacy event compatibility:** Emit both `"start-state"` + `"start-visibility"` (payload `bool`) and `"flyout-hide"` + `"flyout-hidden"` (payload `()`). Frontend views listen only to the canonical events (`"start-state"`, `"flyout-hide"`) so they don't process duplicates, while any legacy listener remains supported.
- **Fullscreen `WS_CAPTION` check:** In Win32 `WS_CAPTION` (`0x00C0_0000`) is `WS_BORDER | WS_DLGFRAME`. Test `(style & WS_CAPTION_FLAG) == WS_CAPTION_FLAG || (style & WS_THICKFRAME_FLAG) != 0` to detect titled/resizable windows so borderless windows with only `WS_BORDER` are classified accurately.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml` -- expected: exits `0`, all unit and integration tests pass with zero compilation errors.
- `pnpm build` -- expected: exits `0`, TypeScript check and Vite bundle succeed cleanly.
- `pnpm test` -- expected: exits `0` non-interactively (`vitest run`), all `layoutMap.test.ts` cases pass.
- Grep check on `src-tauri/` -- expected: zero matches for `set_window_bounds`, zero matches for `"resizable": true`, and `style_overlay_window` called only during startup `setup` (never inside `open_flyout`).

## Auto Run Result

- **Summary:** Restored a clean, compiling, test-verified TestBar shell baseline with a pure Win32-independent `WindowCoordinator`, single-authority fullscreen classifier, DPI AppBar rect calculator, visibility-only window show/hide (eliminating `1×1` shrinking), `WM_MOUSEACTIVATE` non-activating flyout subclassing, main-thread off-hook `GlobalMouseDown` dispatch, non-resizable strict-CSP `tauri.conf.json`, canonical+legacy event/kind compatibility, and a Vitest frontend test harness.
- **Files Changed:**
  - `src-tauri/src/coordinator.rs` — Pure `WindowCoordinator`, `FlyoutKind` with serde aliases, `ModalPickerGuard`, `classify_fullscreen`, `compute_appbar_rect`, and 19 unit tests.
  - `src-tauri/src/win_shell.rs` — `position_window`, `get_window_pixel_rect`, idempotent `WM_MOUSEACTIVATE` flyout subclass, `is_fullscreen_active` delegating to `classify_fullscreen`, `ABM_NEW` re-register guard, and 2 Win32 unit tests.
  - `src-tauri/src/lib.rs` — Wired `AppState` and all window lifecycle/hook/fullscreen paths through `WindowCoordinator` on the main GUI thread with monotonic timestamps and dual canonical+legacy event emission.
  - `src-tauri/tauri.conf.json` — Locked `resizable: false` on all 3 windows and enforced strict local `security.csp`.
  - `src/lib/ipc.ts` — Expanded `FlyoutKind` / `FlyoutRequest.kind` with canonical and legacy names.
  - `src/components/TaskbarView.tsx`, `src/components/StartView.tsx`, `src/components/FlyoutView.tsx` — Migrated listeners to canonical `start-state` / `flyout-hide` and matched canonical + legacy flyout kinds.
  - `package.json`, `pnpm-lock.yaml`, `src/lib/layoutMap.test.ts` — Installed `vitest`, added `"test": "vitest run"`, and added 12 RU↔EN `layoutMap` unit tests.
- **Review Findings Breakdown:**
  - **Patched (grouped entries by entry verdict):** 3 `high` entries (main-thread marshaling of Win32/coordinator calls, fullscreen suppression gating opens, Start `200ms` post-open focus-loss grace guard), 12 `medium` entries (monotonic debounce clock + backward-step guard, modal-picker Start re-focus, debounce preservation across non-activating flyouts, `.flyout-root` padding offset in flyout positioning, `is_fullscreen_active` buffer/`IsIconic`, and 8 verification-gap unit test suites), and 6 `low` entries (`compute_appbar_rect` clamping, half-open `PixelRect::contains` + idle click fast-path, `flyout_wnd_proc` safety/reset, `position_flyout_window` clamping + case-insensitive `bar_mode`, `register_appbar` `ABM_NEW` guard, `BarMode::from_config_str` test).
  - **Deferred (2):** Pre-existing `FlyoutView.tsx:324` `Preview` `onMouseLeave={close}` (low, Epic 2); unverified live WebView2 child HWND `WM_MOUSEACTIVATE` routing and runtime CSP rendering (medium unverified, interactive smoke test).
  - **Rejected (2):** Missing `'osd'` rendering branch in `FlyoutView.tsx` / `Osd`+`Start` overlap (both `false` — OSD UI is owned by Epic 3 with no callers in Story 1.1, and AD-1 only forbids preempting interactive click flyouts).
- **Follow-up Review Recommended:** `true` (first pass patched 3 `high` and 12 `medium` entries; specific unverified risk: live Windows 11 WebView2 child HWND `WM_MOUSEACTIVATE` behavior, `run_on_main_thread` focus-handoff timing, and strict CSP runtime resource loading under `testbar.exe --no-hide-native`).
- **Verification Performed:**
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **PASS (exit 0)** — 23/23 tests passed (19 `coordinator::tests`, 2 `win_shell::tests`, 1 `config::tests`, 1 `integration_tests`), zero Rust warnings.
  - `pnpm build`: **PASS (exit 0)** — `tsc && vite build` succeeded cleanly.
  - `pnpm test`: **PASS (exit 0)** — 12/12 Vitest tests in `src/lib/layoutMap.test.ts` passed non-interactively.
  - High-risk static grep check on `src-tauri/`: **PASS** — zero `set_window_bounds` calls, zero `"resizable": true` entries, `style_overlay_window` called only in startup `setup`, zero `windows::` imports in `coordinator.rs`.
- **Residual Risks:** Live WebView2 child HWND `WM_MOUSEACTIVATE` activation and strict CSP runtime asset checks remain to be smoke-tested interactively via `testbar.exe --no-hide-native` once Epic 1 shell host stories complete.
