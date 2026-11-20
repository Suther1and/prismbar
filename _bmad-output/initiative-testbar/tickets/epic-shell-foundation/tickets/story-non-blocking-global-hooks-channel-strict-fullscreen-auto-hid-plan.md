---
title: 'Non-blocking global hooks channel & strict fullscreen auto-hide integration'
type: 'feature'
ticket: '3'
created: '2026-11-20'
status: 'built'
baseline_revision: '98f50d76e5d072d4130a1158ec067de171daabac'
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
      Verify live Windows 11 WH_KEYBOARD_LL, WH_MOUSE_LL, GetAsyncKeyState Win+L unlock recovery, 0xE8 Start suppression, and Ctrl+Alt+Shift+Esc WM_HOTKEY teardown in compiled testbar.exe.
    evidence: |-
      Unit tests verified WinKeyStateMachine, HookEventChannel (capacity=64), and dispatch_ll_keyboard_event/dispatch_ll_mouse_event wiring without installing live global OS hooks; real SetWindowsHookExW delivery, 0xE8 mask suppression of native Windows 11 Start, Secure Desktop Win+L recovery, and Ctrl+Alt+Shift+Esc hotkey unhooking require an interactive Windows 11 desktop smoke test.
    location: >-
      src-tauri/src/win_shell.rs:1723
    severity: medium (unverified)
  - summary: >-
      Verify live Tauri WebView2 fullscreen auto-hide across real D3D/borderless games, ABN_FULLSCREENAPP, h_fast poller, and WM_DPICHANGED transitions.
    evidence: |-
      Unit tests verified WindowCoordinator::on_appbar_fullscreen_hint, apply_fullscreen_snapshot, apply_taskbar_visibility_action, and DPI-override persistence across fullscreen without spawning live WebView2 windows or fullscreen games; confirming zero visibility or Z-order flicker under real Windows 11 DWM composition requires interactive smoke testing.
    location: >-
      src-tauri/src/lib.rs:223
    severity: medium (unverified)
---

<intent-contract>

## Intent

**Problem:** `WH_KEYBOARD_LL` and `WH_MOUSE_LL` callbacks in `win_shell.rs` directly invoke `HOOK_CALLBACK` (running Tauri `run_on_main_thread` dispatch on the hook thread, risking Windows `LowLevelHooksTimeout` unhooking), track Win key state via two ad-hoc atomics that mishandle dual-Win chords and injected events, and `position_taskbar_window` still calls `SetWindowPos(..., HWND_TOPMOST, ...)` while fullscreen-suppressed without startup fullscreen reconciliation or repositioning on fullscreen exit.

**Approach:** Isolate `WH_KEYBOARD_LL` and `WH_MOUSE_LL` behind a bounded `sync_channel(64)` (`HookEventChannel` + non-blocking `try_send`) drained by a dedicated worker thread, extract a pure testable `WinKeyStateMachine` that ignores `LLKHF_INJECTED` and preserves every `Win+<key>` OS shortcut, and enforce single-authority strict fullscreen auto-hide across `ABN_FULLSCREENAPP`, `h_fast`, startup, `resize_taskbar_dock`, `set_bar_mode`, and `position_taskbar_window` with zero visibility or Z-order leaks while suppressed.

## Boundaries & Constraints

**Always:**
- `ll_keyboard_proc` and `ll_mouse_proc` perform zero Tauri IPC, zero `SetWindowPos`/`ShowWindow`, zero blocking I/O, and zero mutex locks waiting on GUI/IPC work; they enqueue `ShellHookEvent` strictly via non-blocking `try_send` into a bounded `sync_channel(64)` (`HOOK_EVENT_CHANNEL_CAPACITY = 64`), dropping excess events cleanly instead of blocking.
- A dedicated worker thread drains `Receiver<ShellHookEvent>` into the caller's callback (catching callback panics so the worker never dies silently) and exits cleanly when the sender drops.
- `WinKeyStateMachine` ignores `LLKHF_INJECTED` / `LLKHF_LOWER_IL_INJECTED` and `VK_DUMMY_MASK` (`0xE8`), tracks `VK_LWIN` and `VK_RWIN` independently, preserves auto-repeat semantics (repeat after a combo never clears `combo_used`), treats dual-Win chords and any intervening non-Win key as a non-lone press, and emits `ToggleStart` only on a true lone Win press-and-release (suppressing native Windows Start via `0xE8` mask key).
- `Ctrl+Alt+Shift+Esc` (`WM_HOTKEY`) continues to unhook `WH_KEYBOARD_LL`, `WH_MOUSE_LL`, and the hotkey BEFORE calling `restore_native_taskbar_nonblocking()`, and reliably delivers `ShellHookEvent::EmergencyRestore` to the worker thread after unhooking; the hook loop breaks on `GetMessageW(...).0 <= 0`.
- `coordinator::classify_fullscreen` remains the sole fullscreen authority across `ABN_FULLSCREENAPP` (never trusting raw `lparam`), `h_fast`, and startup; `position_taskbar_window` skips Win32 `SetWindowPos`/`ShowWindow` while fullscreen-suppressed and `apply_coordinator_actions` repositions the taskbar with latest `(bar_mode, taskbar_content_width)` before showing it upon fullscreen exit.

**Never:**
- Do not modify `config.rs` schema or `PinItem` fields (Story `1.4`).
- Do not alter `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, `system_widgets.rs`, `main.rs`, `tauri.conf.json`, or frontend React/TS components.
- Do not break any Story `1.1` (`WindowCoordinator`, Vitest) or Story `1.2` (`SingleInstanceGuard`, top-level `WS_OVERLAPPED` AppBar/`TaskbarCreated`, HiDPI `recovery-workarea.json`, watchdog, panic hook) invariants or tests.
- Never install live global OS hooks or mutate the host desktop taskbar inside unit tests.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Lone Win tap | `LWin Down -> LWin Up` or `RWin Down -> RWin Up` (incl. `LWin` key repeat alone) | Emits `WinKeyAction::ToggleStart` on keyup; resets cleanly to idle | Stray keyup without prior down returns `None` |
| `Win+<key>` OS chords | `Win+E/D/R/L/V/Tab`, `Win+Shift+S`, `LWin` released before `E Up`, `LWin` auto-repeat after `E Down` | Returns `WinKeyAction::None` throughout; OS shortcut passes through untouched | Next lone Win tap after chord succeeds |
| Injected & dual-Win keys | `LLKHF_INJECTED` / `LLKHF_LOWER_IL_INJECTED` Win or non-Win events, `0xE8` mask key, `LWin+RWin` overlapping chords (any release order) | Injected & `0xE8` events ignored completely; dual-Win chords never trigger `ToggleStart` and reset only when both Win keys are up | Never panics or wedges into permanent combo state |
| Bounded channel (`capacity = 64`) | Burst of `> 64` events (`try_send`) with paused/unconsumed receiver, then worker drains | First `64` return `Sent`, excess return `DroppedFull` immediately without blocking and increment `dropped_count`; worker drains all `64` in FIFO order and exits on sender drop | Worker survives panicking callback via `catch_unwind`; disconnected receiver returns `Disconnected` |
| Fullscreen single authority & bounds suppression | `ABN_FULLSCREENAPP(true)` over maximized desktop app; `ABN_FULLSCREENAPP(false)` while D3D/borderless game active; `resize_taskbar_dock`/`set_bar_mode`/`DisplayChanged` while suppressed; game exit | Maximized desktop app stays visible; true fullscreen stays hidden regardless of `lparam`; bounds updates record state without showing or calling `SetWindowPos` while suppressed; exit fullscreen repositions with latest width/mode and restores taskbar | Startup checks fullscreen before showing taskbar |

</intent-contract>

## Code Map

- `src-tauri/src/win_shell.rs` -- Remove global `WIN_DOWN`, `WIN_COMBO_USED`, and `HOOK_CALLBACK` statics (lines 78–80). Add `HOOK_EVENT_CHANNEL_CAPACITY` (`64`), `VK_DUMMY_MASK` (`0xE8`), `KeyTransition`, `WinKeyAction`, `WinKeyStateMachine`, `HookEnqueueResult`, `HookEventSender`, `HookEventChannel` (`sync_channel(64)`, non-blocking `try_send`, post-unhook `send_blocking`, `spawn_worker`). Add `PartialEq, Eq` to `ShellHookEvent` (line 97). Refactor `start_shell_hook_thread` (lines 1434–1478), `ll_mouse_proc` (lines 1480–1491), and `ll_keyboard_proc` (lines 1522–1551) to use thread-local `Cell<WinKeyStateMachine>` and `RefCell<Option<HookEventSender>>`, check `GetMessageW(...).0 <= 0`, unhook before restoring, and deliver `EmergencyRestore` after unhooking. Preserve all Story `1.1`/`1.2` AppBar, mutex, workarea, and watchdog logic untouched.
- `src-tauri/src/coordinator.rs` -- Reuse `classify_fullscreen` (line 141) and `WindowCoordinator::on_fullscreen_changed` (line 421). Add `WindowCoordinator::reevaluate_fullscreen(&mut self, notification_state, foreground, primary_monitor) -> CoordinatorActions` (and `on_appbar_fullscreen_hint(&mut self, _hint_entering: bool, ...)` delegating strictly to `reevaluate_fullscreen`) plus `should_apply_taskbar_bounds(&self) -> bool` (`!self.fullscreen_suppressed`) so single-authority fullscreen integration is pure and directly unit-testable.
- `src-tauri/src/lib.rs` -- Guard `position_taskbar_window` (lines 100–129) so it returns early without calling `SetWindowPos` whenever `coordinator.is_fullscreen_suppressed()` is true; in `apply_coordinator_actions` (lines 211–217), when `actions.taskbar_visible == Some(true)`, reposition taskbar with current `(bar_mode, taskbar_content_width)` right before `set_window_visible(hwnd, true, true)`. In `.setup()` (lines 805–856), run `reevaluate_fullscreen` before `position_taskbar_window` on `ShellAppBarEvent` display/DPI/position events and before initial taskbar show; keep `ShellAppBarEvent::FullscreenHint(_)` and `h_fast` (lines 980–986) strictly routing through `reevaluate_fullscreen`. Keep all non-shell commands/pollers untouched.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/src/coordinator.rs` -- Add `reevaluate_fullscreen`, `on_appbar_fullscreen_hint`, and `should_apply_taskbar_bounds` methods on `WindowCoordinator` with unit tests for `ABN_FULLSCREENAPP(true/false)` single-authority re-evaluation and bounds suppression -- makes the single fullscreen authority contract explicit and directly testable without Win32 side effects.
- [x] `src-tauri/src/win_shell.rs` -- Implement pure `WinKeyStateMachine` and `HookEventChannel` (`capacity = 64`), replace `WIN_DOWN`/`WIN_COMBO_USED`/`HOOK_CALLBACK` with thread-local non-blocking `try_send` + dedicated worker thread in `start_shell_hook_thread`/`ll_keyboard_proc`/`ll_mouse_proc`, fix `GetMessageW` error check (`<= 0`), and add deterministic unit tests covering all I/O & Edge-Case Matrix rows for `WinKeyStateMachine` and `HookEventChannel` -- eliminates low-level hook stalls and `Win+<key>` regressions.
- [x] `src-tauri/src/lib.rs` -- Guard `position_taskbar_window` on `should_apply_taskbar_bounds()`, reposition taskbar before showing on `taskbar_visible == Some(true)` in `apply_coordinator_actions`, evaluate fullscreen before initial taskbar show and before repositioning on AppBar geometry events, and add unit tests covering taskbar bounds suppression and `ABN_FULLSCREENAPP` re-evaluation integration -- guarantees zero visibility or Z-order leaks over fullscreen apps.

**Acceptance Criteria:**
- Given `src-tauri/src/win_shell.rs` after all edits, when `ll_keyboard_proc` and `ll_mouse_proc` are inspected statically, then both callbacks contain zero Tauri IPC calls, zero `SetWindowPos`/`ShowWindow` calls, zero mutex locks waiting on GUI/IPC work, and strictly enqueue events via non-blocking `try_send` onto the `capacity = 64` channel drained by a dedicated worker thread.
- Given the full workspace after all edits, when `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` run non-interactively, then all three commands exit `0` with passing tests for Story `1.1`, Story `1.2`, and Story `1.3` and zero Rust compiler warnings.

## Implementation Notes

- Added `WindowCoordinator::should_apply_taskbar_bounds`, `reevaluate_fullscreen`, and `on_appbar_fullscreen_hint` in `src-tauri/src/coordinator.rs` plus 2 unit tests proving single-authority fullscreen classification across `ABN_FULLSCREENAPP` hints and startup.
- Replaced global `WIN_DOWN`/`WIN_COMBO_USED`/`HOOK_CALLBACK` statics in `src-tauri/src/win_shell.rs` with `WinKeyStateMachine`, `HookEventChannel`/`HookEventSender` (`HOOK_EVENT_CHANNEL_CAPACITY = 64`), thread-local `Cell<WinKeyStateMachine>` & `RefCell<Option<HookEventSender>>`, non-blocking `try_send` inside `ll_keyboard_proc`/`ll_mouse_proc`, panic-resilient dedicated worker thread (`catch_unwind`), `GetMessageW(...).0 <= 0` loop guard, post-unhook `send_blocking(EmergencyRestore)`, `FullscreenSnapshot`/`query_fullscreen_snapshot()`, and 4 deterministic unit tests.
- Updated `src-tauri/src/lib.rs` to route fullscreen checks through `apply_fullscreen_snapshot` / `reevaluate_fullscreen` / `reevaluate_fullscreen_with_hint`, gate `position_taskbar_window` on `should_apply_taskbar_bounds()` (extracted `compute_taskbar_bounds` + `plan_taskbar_bounds_update`), reposition taskbar before showing when `actions.taskbar_visible == Some(true)`, evaluate fullscreen before initial taskbar show and before repositioning on AppBar geometry events, and added 2 unit tests in `integration_tests`.

## Plan Change Log

## Review Triage Log

### 2026-11-20 — Review pass
- verdicts: 26 findings — high 5, medium 12, low 5, false 2, maybe-false 2
- findings:
  - `[high]` `[patch]` (blind-hunter) `WinKeyStateMachine::on_key_event` sticks after `Win+L`, UAC, or `Ctrl+Alt+Del` losing `Win Up` on Secure Desktop, swallowing next `LWin` tap and blocking `RWin` — Added `reconcile_physical_win_keys` called via non-blocking `GetAsyncKeyState(VK_LWIN/VK_RWIN)` before processing `KeyTransition::Down` whenever a Win flag is down, with unit tests.
  - `[medium]` `[patch]` (blind-hunter) `WinKeyStateMachine::on_key_event` cancels lone Win on non-Win `KeyTransition::Up` during fast typing rollover — Restricted non-Win `combo_used = true` to `KeyTransition::Down` only, with rollover unit test.
  - `[medium]` `[patch]` (blind-hunter) `WinKeyStateMachine` + `ll_keyboard_proc` falsely emit `ToggleStart` on pre-held `Alt+Win`, `Ctrl+Win`, `Shift+Win` (`LLKHF_ALTDOWN` ignored) — Handled `LLKHF_ALTDOWN_FLAG` (`0x20`) in `WinKeyStateMachine` and checked pre-held `VK_CONTROL`/`VK_SHIFT`/`VK_MENU` via non-blocking `GetAsyncKeyState` at Win `Down` time, with unit tests.
  - `[medium]` `[patch]` (blind-hunter) `ll_mouse_proc` unconditionally enqueues every global mouse click even when overlays are closed, risking 64-slot channel starvation dropping `ToggleStart` — Added lock-free `HOOK_OVERLAY_OPEN` (`set_overlay_open_hint`) synced in `AppState::sync_overlay_flag` so `ll_mouse_proc` skips `try_send` when overlays are closed.
  - `[medium]` `[patch]` (blind-hunter) `ll_keyboard_proc` calls `send_dummy_mask_key()` before `try_send` and ignores `HookEnqueueResult` (eating `Win` on full/disconnected channel; fullscreen Start suppression is pre-existing Story 1.1 coordinator policy) — Reordered `dispatch_ll_keyboard_event` to `try_send` first and invoke `send_dummy_mask_key()` only on `HookEnqueueResult::Sent`.
  - `[medium]` `[patch]` (blind-hunter) `start_shell_hook_thread` lacks duplicate-start guard, leaks threads if hotkey+hooks fail, and lacks clean `stop_shell_hook_thread()` — Added `HOOK_THREAD_ACTIVE` CAS guard, `HOOK_THREAD_ID`, RAII `HookThreadGuard`, early return when all 3 registrations fail, and `stop_shell_hook_thread()` (`PostThreadMessageW(..., WM_QUIT)`) called on clean exit.
  - `[medium]` `[patch]` (blind-hunter) `ShellAppBarEvent::DpiChanged(dpi)` loses `dpi_override` during fullscreen suppression and double-repositions with wrong-scale flicker on simultaneous exit — Added `AppState::dpi_override: AtomicU32` (set on `DpiChanged`, cleared on `DisplayChanged`), passed through `position_taskbar_window`, and skipped duplicate reposition when `reevaluate_fullscreen` already returned `taskbar_visible == Some(true)`.
  - `[low]` `[patch]` (blind-hunter) Redundant double coordinator lock in `position_taskbar_window` and non-poison-safe `.lock().unwrap()` in `set_bar_mode`/`resize_taskbar_dock` (`on_taskbar_bounds_updated` preserved for Story 1.1 contract) — Consolidated layout read into single-lock `read_taskbar_layout_if_allowed` and switched locks to `.unwrap_or_else(|e| e.into_inner())`.
  - `[low]` `[patch]` (blind-hunter) `compute_taskbar_bounds` produces negative width on narrow monitors (`mon.width < 16`) or invalid `sf` — Guarded `effective_taskbar_scale_factor` / `compute_taskbar_bounds` with finite positive `sf` fallback and non-negative clamped bounds (`(mon_w - 16).max(0)`), with unit tests.
  - `[medium]` `[patch]` (blind-hunter) Missing `catch_unwind`/`try_with`/`HC_ACTION`/`lparam != 0` at hook FFI boundary and `HookEventChannel::with_capacity(0)` allows degenerate rendezvous channel — Wrapped `ll_keyboard_proc`/`ll_mouse_proc` in `catch_unwind`, switched TLS to `try_with`, checked `code == HC_ACTION && lparam.0 != 0`, and clamped `with_capacity` to `capacity.max(1)`.
  - `[medium]` `[patch]` (edge-case-hunter) Pre-held `Ctrl`/`Alt`/`Shift` or `LLKHF_ALTDOWN` falsely triggers `ToggleStart` — Fixed with `LLKHF_ALTDOWN_FLAG` handling and non-blocking `GetAsyncKeyState` modifier check on Win `Down`.
  - `[high]` `[patch]` (edge-case-hunter) Lost `Win Up` on `Win+L` or UAC leaves `lwin_down`/`rwin_down` stuck true — Fixed via `reconcile_physical_win_keys` using non-blocking `GetAsyncKeyState` on `KeyTransition::Down`.
  - `[medium]` `[patch]` (edge-case-hunter) Non-Win `KeyTransition::Up` during typing rollover sets `combo_used` and drops lone Win tap — Fixed by marking `combo_used` only on non-Win `KeyTransition::Down`.
  - `[medium]` `[patch]` (edge-case-hunter) Full 64-slot channel suppresses native Start before `try_send` drops `ToggleStart` — Fixed by enqueuing `ToggleStart` first and calling `send_dummy_mask_key()` only when `HookEnqueueResult::Sent`.
  - `[low]` `[patch]` (edge-case-hunter) `kb.vkCode as u16` truncation can alias out-of-range `vkCode` to `VK_LWIN`/`VK_RWIN`/`VK_DUMMY_MASK` — Replaced with `let Ok(vk) = u16::try_from(vk_code) else { return; }` and added unit test.
  - `[low]` `[patch]` (edge-case-hunter) `HookEventChannel::with_capacity(0)` creates rendezvous channel dropping events on `try_send` — Clamped `capacity.max(1)` with unit test.
  - `[medium]` `[patch]` (edge-case-hunter) Duplicate `start_shell_hook_thread` or all-failed registrations leak threads / duplicate hooks — Fixed with `HOOK_THREAD_ACTIVE` CAS guard, RAII cleanup, and early exit on total registration failure.
  - `[low]` `[patch]` (edge-case-hunter) Monitor width `<= 16` or non-finite/non-positive `sf` produces negative dock width or overflow — Fixed with finite positive `sf` guard and non-negative width/height clamping in `compute_taskbar_bounds`.
  - `[high]` `[patch]` (verification-gap) `WinKeyStateMachine::on_key_event` bitmask flag filtering untested with realistic Win32 non-injected (`LLKHF_EXTENDED`, `LLKHF_UP`, `LLKHF_ALTDOWN`) and combined injected flags — Added `win_key_state_machine_realistic_flags_rollover_altdown_and_reconcile` covering `LLKHF_EXTENDED`, `LLKHF_EXTENDED | LLKHF_UP`, `LLKHF_ALTDOWN`, and combined injected masks.
  - `[medium]` `[patch]` (verification-gap) Fullscreen-exit taskbar repositioning in `apply_coordinator_actions` bypassed by test — Extracted `apply_taskbar_visibility_action` called by `apply_coordinator_actions` and added `apply_taskbar_visibility_action_repositions_before_showing_and_skips_on_hide`.
  - `[high]` `[patch]` (verification-gap) Rewritten `ll_keyboard_proc` and `ll_mouse_proc` callback wiring unexercised by any test — Extracted `dispatch_ll_keyboard_event` and `dispatch_ll_mouse_event` and added `ll_keyboard_and_mouse_dispatch_wiring_mask_order_modifiers_reconcile_and_overlay_gate`.
  - `[maybe-false]` `[defer]` (intent-alignment) Live Win32 `SetWindowsHookExW`/`RegisterHotKey` (`Ctrl+Alt+Shift+Esc` teardown & real `0xE8` native Start suppression) not exercised end-to-end in headless unit tests — Hook callback wiring patched via `dispatch_ll_*` tests; live OS hook delivery deferred to interactive Windows 11 smoke testing.
  - `[high]` `[patch]` (intent-alignment) Real-OS `WinKeyStateMachine` semantics (`Win+L` lost `KEYUP` and typing rollover `KeyTransition::Up`) — Fixed via `reconcile_physical_win_keys` and `KeyTransition::Down`-only combo marking.
  - `[false]` `[reject]` (intent-alignment) Truly frameless full-monitor desktop app indistinguishable from borderless game — Refuted: `AD-6` explicitly defines `classify_fullscreen` via absence of `WS_CAPTION` and `WS_THICKFRAME` on non-shell, non-TestBar full-monitor windows; normal maximized desktop apps retain `WS_CAPTION`/`WS_THICKFRAME`.
  - `[maybe-false]` `[defer]` (intent-alignment) Live Tauri WebView2 fullscreen transitions (`ABN_FULLSCREENAPP`, `h_fast`, `SetWindowPos`/`ShowWindow`) not run end-to-end in headless `cargo test` — `apply_coordinator_actions` branch patched via `apply_taskbar_visibility_action` test; live DWM/WebView2 fullscreen smoke testing deferred.
  - `[false]` `[reject]` (intent-alignment) `pnpm test` and `pnpm build` not run by the read-only auditor — Refuted: not a code defect; both `pnpm test` (12/12) and `pnpm build` ran and exited `0` in verification.

## Design Notes

- **Thread-local hook state:** Because Windows always invokes `WH_KEYBOARD_LL` and `WH_MOUSE_LL` callbacks on the exact thread that called `SetWindowsHookExW` and pumps `GetMessageW`, storing `Cell<WinKeyStateMachine>` and `RefCell<Option<HookEventSender>>` in `thread_local!` eliminates all mutex locks and global mutable hook state inside hook callbacks while guaranteeing the `SyncSender` drops (closing the worker cleanly) when the hook thread exits.
- **Mask key timing:** `send_dummy_mask_key()` (`SendInput` with unassigned VK `0xE8`) MUST execute on lone Win keyup before `CallNextHookEx` returns so Windows suppresses native Start while releasing `VK_LWIN`/`VK_RWIN`; because `WinKeyStateMachine` ignores `LLKHF_INJECTED` and `0xE8`, the injected mask key never perturbs hook state.
- **EmergencyRestore delivery:** `WM_HOTKEY` (`Ctrl+Alt+Shift+Esc`) runs in the `GetMessageW` loop after unhooking `WH_KEYBOARD_LL`/`WH_MOUSE_LL` and `UnregisterHotKey`, restores the native taskbar non-blockingly first, then delivers `EmergencyRestore` to the worker via `send_blocking` so a transient burst of 64 mouse events cannot drop the shutdown signal.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml` -- expected: exits `0`, all Rust unit and integration tests pass with zero compiler warnings.
- `pnpm test` -- expected: exits `0`, all Vitest frontend tests pass non-interactively.
- `pnpm build` -- expected: exits `0`, `tsc && vite build` succeeds cleanly.
- Static inspection of `ll_keyboard_proc` and `ll_mouse_proc` in `src-tauri/src/win_shell.rs` -- expected: zero Tauri IPC, zero `SetWindowPos`/`ShowWindow`, zero mutex locks, strictly non-blocking `try_send` into `capacity = 64` channel.

## Auto Run Result

- **Summary:** Isolated `WH_KEYBOARD_LL` and `WH_MOUSE_LL` callbacks onto a bounded `sync_channel(64)` (`HookEventChannel` + non-blocking `try_send`) drained by a panic-resilient dedicated worker thread, replaced ad-hoc Win-key atomics with a pure testable `WinKeyStateMachine` preserving `Win+<key>` chords, rollover typing, pre-held modifiers, `LLKHF_INJECTED` ignoring, and Secure Desktop (`Win+L`/UAC) key-state reconciliation, and hardened single-authority strict fullscreen auto-hide across `ABN_FULLSCREENAPP`, `h_fast`, startup, `WM_DPICHANGED`, `resize_taskbar_dock`, `set_bar_mode`, `position_taskbar_window`, and `apply_coordinator_actions` with zero visibility or Z-order leaks while suppressed.
- **Files Changed:**
  - `src-tauri/src/coordinator.rs` — Added `WindowCoordinator::should_apply_taskbar_bounds`, `reevaluate_fullscreen`, and `on_appbar_fullscreen_hint` delegating strictly to `classify_fullscreen`, plus 2 unit tests.
  - `src-tauri/src/win_shell.rs` — Replaced global `WIN_DOWN`/`WIN_COMBO_USED`/`HOOK_CALLBACK` statics with `WinKeyStateMachine`, `HookEventChannel`/`HookEventSender` (`capacity = 64`), thread-local `Cell`/`RefCell` dispatch, FFI `catch_unwind`/`try_with`/`HC_ACTION`/`u16::try_from` guards, `try_send`-before-mask ordering, lock-free `HOOK_OVERLAY_OPEN` mouse gating, `GetAsyncKeyState` Secure Desktop & modifier checks, `stop_shell_hook_thread` + duplicate-start guard, `FullscreenSnapshot`/`query_fullscreen_snapshot`, and 6 deterministic unit tests.
  - `src-tauri/src/lib.rs` — Routed fullscreen checks through `apply_fullscreen_snapshot`/`reevaluate_fullscreen`/`reevaluate_fullscreen_with_hint`, gated `position_taskbar_window` on `should_apply_taskbar_bounds()`, persisted `dpi_override` across fullscreen suppression without double repositioning on exit, guarded `compute_taskbar_bounds` against narrow monitors and invalid scale factors, synced `set_overlay_open_hint` and `stop_shell_hook_thread` on clean exit, extracted `apply_taskbar_visibility_action`, and added 4 unit tests.
- **Review Findings Breakdown:**
  - **Patched (14 grouped entries by entry verdict — 3 `high`, 8 `medium`, 3 `low`):** 3 `high` entries (Secure Desktop `Win+L`/UAC lost `Win Up` reconciliation via `reconcile_physical_win_keys` + non-blocking `GetAsyncKeyState`; realistic Win32 `LLKHF_EXTENDED`/`LLKHF_UP`/`LLKHF_ALTDOWN` & combined injected flag tests; `dispatch_ll_keyboard_event`/`dispatch_ll_mouse_event` callback wiring tests), 8 `medium` entries (fast typing rollover `KeyTransition::Up` fix; pre-held `Ctrl`/`Alt`/`Shift` & `LLKHF_ALTDOWN` chord detection; lock-free `HOOK_OVERLAY_OPEN` mouse gating in `ll_mouse_proc`; `try_send`-before-`send_dummy_mask_key` ordering; `start_shell_hook_thread` duplicate-start guard, total-failure exit, & `stop_shell_hook_thread`; `AppState::dpi_override` persistence across fullscreen & single reposition on exit; hook FFI `catch_unwind`/`try_with`/`HC_ACTION`/`lparam != 0` & `with_capacity(0)` clamp; `apply_taskbar_visibility_action` reposition-before-show test coverage), and 3 `low` entries (single-lock `read_taskbar_layout_if_allowed` & poison-safe locks; `compute_taskbar_bounds` narrow-monitor/invalid-`sf` guard; `u16::try_from(kb.vkCode)` overflow guard).
  - **Deferred (2):** Live Windows 11 `WH_KEYBOARD_LL`/`WH_MOUSE_LL`/`WM_HOTKEY` (`Ctrl+Alt+Shift+Esc`), `0xE8` native Start suppression, and real `Win+L` unlock smoke verification (`medium (unverified)`); live Tauri WebView2 + DWM fullscreen transitions across real D3D/borderless games, `ABN_FULLSCREENAPP`, `h_fast`, and `WM_DPICHANGED` (`medium (unverified)`).
  - **Rejected (2):** `AD-6` frameless full-monitor fullscreen classifier definition (`false` — matches `AD-6` verbatim; normal maximized desktop apps retain `WS_CAPTION`/`WS_THICKFRAME`); read-only auditor not running `pnpm test`/`pnpm build` (`false` — verified exiting `0`).
- **Follow-up Review Recommended:** `true` (first pass patched 3 `high`, 8 `medium`, and 3 `low` grouped entries; specific unverified risk: live interactive Windows 11 desktop verification of real `WH_KEYBOARD_LL`/`WH_MOUSE_LL`/`WM_HOTKEY` delivery, `GetAsyncKeyState` Secure Desktop `Win+L` unlock recovery, `0xE8` native Start suppression, and real D3D/borderless fullscreen transitions with `ABN_FULLSCREENAPP` and `WM_DPICHANGED`).
- **Verification Performed:**
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **PASS (exit 0)** — 42/42 tests passed (21 `coordinator::tests`, 15 `win_shell::tests`, 1 `config::tests`, 5 `integration_tests`), zero Rust compiler warnings.
  - `pnpm test`: **PASS (exit 0)** — 12/12 Vitest tests in `src/lib/layoutMap.test.ts` passed non-interactively.
  - `pnpm build`: **PASS (exit 0)** — `tsc && vite build` succeeded cleanly.
  - Static inspection of `ll_keyboard_proc` (`src-tauri/src/win_shell.rs:1947-1960`) and `ll_mouse_proc` (`src-tauri/src/win_shell.rs:1838-1851`): **PASS** — zero Tauri IPC, zero `SetWindowPos`/`ShowWindow`, zero blocking I/O, zero mutex locks, wrapped in `catch_unwind`, strictly non-blocking `try_send` into `capacity = 64` channel drained by dedicated worker thread.
- **Residual Risks:** Live OS-level hook delivery (`SetWindowsHookExW`, `GetAsyncKeyState` after real `Win+L` lock, `0xE8` native Start suppression, `Ctrl+Alt+Shift+Esc` teardown) and live DWM/WebView2 fullscreen auto-hide transitions remain to be smoke-tested interactively on Windows 11 once Epic 1 completes.
