---
title: 'Single-instance mutex, top-level AppBar/TaskbarCreated window & HiDPI watchdog recovery'
type: 'feature'
ticket: '2'
created: '2026-11-20'
status: 'built'
baseline_revision: '537eb082e434518f38207439ee8a29d2dd74055e'
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
      Verify live multi-process single-instance rejection on Local\TestBar_SingleInstance_v1 and --watchdog mutex bypass with compiled testbar.exe.
    evidence: |-
      Unit tests verified SingleInstanceGuard::acquire_named duplicate rejection and drop re-acquisition intra-process, but launching two real testbar.exe processes (and confirming --watchdog bypasses the startup mutex while checking it after parent exit) requires an interactive Windows 11 smoke test.
    location: >-
      src-tauri/src/lib.rs:718
    severity: medium (unverified)
  - summary: >-
      Verify live Windows 11 shell recovery across real explorer.exe restarts (TaskbarCreated), taskkill /F watchdog recovery, Ctrl+Alt+Shift+Esc, and HiDPI SPI_SETWORKAREA scaling.
    evidence: |-
      Unit tests verified all AppBar reconciliation paths via FakeAppBarShellOps and pure/isolated Win32 primitives without mutating the host desktop; live end-to-end desktop behavior (real SHAppBarMessage, Per-Monitor V2 SPI_GETWORKAREA/SPI_SETWORKAREA at 125%-200% DPI, Explorer restart broadcasts, and force-kill watchdog restoration) can only be confirmed by interactive smoke testing on Windows 11.
    location: >-
      src-tauri/src/win_shell.rs:1357
    severity: medium (unverified)
---

<intent-contract>

## Intent

**Problem:** TestBar currently allows duplicate instances that corrupt shell state, discards `TaskbarCreated` broadcasts, registers the AppBar on the Tauri `taskbar` HWND without receiving `ABN_POSCHANGED` / `ABN_FULLSCREENAPP` / `WM_DISPLAYCHANGE` / `WM_DPICHANGED`, hardcodes `mon.bottom - 48` on work-area restore (breaking HiDPI displays), and spawns a watchdog that neither flushes orphaned AppBars nor restores a captured work area.

**Approach:** Add an RAII Win32 single-instance mutex (`Local\TestBar_SingleInstance_v1`) acquired before any shell mutation, capture and atomically persist the original HiDPI `SPI_GETWORKAREA` snapshot to `%LOCALAPPDATA%\TestBar\recovery-workarea.json`, host AppBar registration and shell broadcasts on a dedicated hidden top-level `WS_OVERLAPPED` message window, and upgrade the chained Rust panic hook and `--watchdog <pid> --workarea <l,t,r,b>` recovery path with dummy-HWND AppBar flushing and exact HiDPI work-area restoration.

## Boundaries & Constraints

**Always:**
- Acquire `Local\TestBar_SingleInstance_v1` at normal startup before hiding native taskbars, capturing work area, installing the taskbar-restoring panic hook, or spawning the watchdog; exit cleanly on `ERROR_ALREADY_EXISTS` with zero shell mutations.
- Capture original `SPI_GETWORKAREA`, primary monitor rect, and DPI before hiding `Shell_TrayWnd` or registering an AppBar; persist atomically (temp file + rename) to `%LOCALAPPDATA%\TestBar\recovery-workarea.json`.
- Use a dedicated hidden top-level `WS_OVERLAPPED` window (`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, parent `HWND(0)`, never `HWND_MESSAGE`) for AppBar registration (`uCallbackMessage = WM_APP + 10`), `TaskbarCreated`, `WM_DISPLAYCHANGE`, and `WM_DPICHANGED`.
- Scale AppBar height via `coordinator::compute_appbar_rect` (`64px` logical Dock, `52px` logical Edge) using live primary monitor DPI (`GetDpiForMonitor`).
- Treat `ABN_FULLSCREENAPP` solely as a trigger to re-evaluate `win_shell::is_fullscreen_active()` -> `coordinator::classify_fullscreen()` (never bypassing the single fullscreen authority).
- Flush orphaned AppBar reservations in watchdog mode via a local hidden top-level dummy-HWND `ABM_NEW` + `ABM_REMOVE` sequence, show `Shell_TrayWnd` and all `Shell_SecondaryTrayWnd`, and restore exact `SPI_SETWORKAREA` (`SPIF_SENDCHANGE`) from `--workarea` falling back to `recovery-workarea.json` (zero hardcoded `mon.bottom - 48`).
- Preserve `--no-hide-native` mode (skips hiding native taskbars, AppBar registration, panic taskbar restore, and watchdog spawning) and zero DLL injection / `ReadProcessMemory` on `explorer.exe`.

**Never:**
- Do not rewrite `WH_KEYBOARD_LL` / `WH_MOUSE_LL` into a bounded `sync_channel(64)` worker thread (Story `1.3`).
- Do not modify `config.rs` schema or `PinItem` fields (Story `1.4`).
- Do not modify `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, `system_widgets.rs`, or frontend React/TS components.
- Never let unit tests mutate the live host desktop's native taskbar or work area.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Single-instance mutex | First `acquire_named(name)`, then second `acquire_named(name)` while first is alive, then third after first drops | First returns `Ok(Some(guard))`, second returns `Ok(None)` (`ERROR_ALREADY_EXISTS`), third returns `Ok(Some(guard))` | Win32 failure returns `Err`, never panics; closes duplicate handle immediately |
| Work-area snapshot atomic roundtrip | `WorkAreaSnapshot` at 100%/125%/150%/200% DPI written via `save_workarea_snapshot_to(path)` | Writes sibling temp file + atomic rename; `load_workarea_snapshot_from(path)` returns identical snapshot | Invalid JSON or inverted rect (`r <= l \|\| b <= t`) returns `None`/`Err` |
| Watchdog CLI parsing | `["testbar.exe"]`, `["testbar.exe", "--watchdog", "4242"]`, `["testbar.exe", "--watchdog", "4242", "--workarea", "0,0,2560,1368"]`, malformed PID / missing or malformed `--workarea` / inverted rect | Returns `NotWatchdog`, `Run { parent_pid: 4242, work_area: None }`, `Run { parent_pid: 4242, work_area: Some(rect) }`, or `Invalid(reason)` | `Invalid` exits cleanly in `main.rs` without starting GUI or mutating shell |
| Top-level `WS_OVERLAPPED` message window | Spawn hidden window (test mode without shell `SHAppBarMessage`), send `TaskbarCreated` / `WM_APP + 10` (`ABN_FULLSCREENAPP`) / `WM_DISPLAYCHANGE` / mode update, then stop | Window is hidden, top-level (`GetParent == 0`, not `HWND_MESSAGE`), `WS_EX_TOOLWINDOW \| WS_EX_NOACTIVATE`, dispatches callbacks, and destroys cleanly (`!IsWindow`) on stop | Cleanup runs on stop/drop; never panics inside `WndProc` |
| Watchdog recovery & panic hook | Parent exits/crashes or panics with `hide_native == true` | Watchdog waits on PID (`CloseHandle` after wait), flushes orphaned AppBars via dummy-HWND `ABM_NEW`+`ABM_REMOVE`, shows primary+secondary native taskbars, restores exact HiDPI `SPI_SETWORKAREA` from `--workarea` or `recovery-workarea.json` | Missing `--workarea` falls back to JSON; if both missing, skips `SPI_SETWORKAREA` instead of using `- 48`; panic hook never double-panics |

</intent-contract>

## Code Map

- `src-tauri/Cargo.toml` -- Add `"Win32_UI_HiDpi"` to `windows` dependency features for `GetDpiForMonitor` / `MDT_EFFECTIVE_DPI`.
- `src-tauri/src/coordinator.rs` -- Derive `Serialize, Deserialize` on `PixelRect` (line 86) so `win_shell.rs` reuses it directly in `WorkAreaSnapshot` and `WatchdogArgs`; keep module 100% Win32-independent.
- `src-tauri/src/win_shell.rs` -- Replace hardcoded `mon.bottom - 48` (line 341) and unused `TaskbarCreated` registration (line 500); add `SingleInstanceGuard` (`Local\TestBar_SingleInstance_v1`), `WorkAreaSnapshot` + atomic `%LOCALAPPDATA%\TestBar\recovery-workarea.json` persistence, `primary_monitor_dpi()`, `WatchdogArgs`/`WatchdogCli`/`parse_watchdog_args()`, `install_panic_restore_hook()`, dedicated hidden top-level `WS_OVERLAPPED` AppBar/`TaskbarCreated` message window (`ShellAppBarEvent`, `start_appbar_message_window`, `update_appbar_mode`, `stop_appbar_message_window`), `flush_orphaned_appbars()`, upgraded `spawn_watchdog()` / `run_watchdog(WatchdogArgs)` / `restore_native_taskbar()`, and deterministic unit tests. Leave `start_shell_hook_thread`, `ll_keyboard_proc`, and `ll_mouse_proc` hook architecture intact (Story `1.3`).
- `src-tauri/src/main.rs` -- Replace lines 7–12 with `testbar_lib::win_shell::parse_watchdog_args(&args)` dispatch (`WatchdogCli::Run` -> `run_watchdog_mode`, `WatchdogCli::Invalid` -> clean exit, `WatchdogCli::NotWatchdog` -> normal `run(hide_native)`).
- `src-tauri/src/lib.rs` -- Update `run_watchdog_mode(args: WatchdogArgs)`, acquire `SingleInstanceGuard` first in `run(hide_native)`, capture+persist HiDPI work area before hiding/spawning watchdog, install chained panic hook only when `hide_native`, start top-level AppBar message window in `.setup()` instead of registering AppBar on the `taskbar` WebView HWND, dispatch `ShellAppBarEvent` to main thread (`position_taskbar_window` + fullscreen re-eval with synced `fullscreen_suppressed` flag), update `set_bar_mode` to call `win_shell::update_appbar_mode`, and cleanly stop AppBar window + restore native taskbar on Exit / `EmergencyRestore` / `restore_windows_taskbar_and_exit` when `hide_native`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml` & `src-tauri/src/coordinator.rs` -- Enable `"Win32_UI_HiDpi"` feature and derive `Serialize, Deserialize` on `PixelRect` -- provides live primary monitor DPI lookup and shared rect serialization with zero Win32 leakage into `coordinator.rs`.
- [x] `src-tauri/src/win_shell.rs` -- Implement `SingleInstanceGuard`, `WorkAreaSnapshot` atomic JSON persistence (`%LOCALAPPDATA%\TestBar\recovery-workarea.json`), `primary_monitor_dpi()`, `WatchdogCli` parser, chained `install_panic_restore_hook()`, hidden top-level `WS_OVERLAPPED` AppBar/`TaskbarCreated` message window (`ABN_POSCHANGED`, `ABN_FULLSCREENAPP`, `TaskbarCreated`, `WM_DISPLAYCHANGE`, `WM_DPICHANGED`, live `BarMode` updates), dummy-HWND `flush_orphaned_appbars()`, and upgraded HiDPI `spawn_watchdog`/`run_watchdog`/`restore_native_taskbar` removing `mon.bottom - 48` -- delivers all R1 OS shell lifecycle primitives safely.
- [x] `src-tauri/src/main.rs` & `src-tauri/src/lib.rs` -- Wire `parse_watchdog_args` in `main.rs`, acquire `SingleInstanceGuard` before any shell mutation in `lib::run`, capture/persist work area before hide/watchdog, run AppBar on the dedicated hidden top-level message window, route `ShellAppBarEvent` to the main thread for taskbar repositioning and fullscreen re-evaluation, update `set_bar_mode` via `update_appbar_mode`, and clean up on exit/emergency restore -- integrates all lifecycle invariants end-to-end while preserving `--no-hide-native` and Story `1.1` coordinator behavior.
- [x] `src-tauri/src/win_shell.rs` (tests) -- Add deterministic unit tests covering all I/O & Edge-Case Matrix rows (duplicate single-instance mutex rejection + drop re-acquire, atomic `recovery-workarea.json` roundtrip + invalid rejection, `--watchdog` / `--workarea` CLI parsing across valid/missing/malformed cases, and hidden top-level `WS_OVERLAPPED` message window creation, style/hierarchy verification, event delivery, and cleanup without mutating the live desktop taskbar) -- proves R1 behaviors reliably in `cargo test`.

**Acceptance Criteria:**
- Given `src-tauri/` after all edits, when audited statically, then zero hardcoded `mon.bottom - 48` offsets exist in `src-tauri/`, zero `HWND_MESSAGE` is used for `TaskbarCreated` or AppBar hosting, and `SingleInstanceGuard` is acquired in `lib::run` before any `hide_native_taskbar`, `capture_and_persist_original_workarea`, panic-hook, or `spawn_watchdog` call.
- Given the full workspace after all edits, when `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` run non-interactively, then all three commands exit `0` with passing tests for Story `1.1` and Story `1.2` and zero Rust warnings.

## Implementation Notes

- Added `"Win32_Security"` alongside `"Win32_UI_HiDpi"` in `src-tauri/Cargo.toml` because `windows` 0.58 gates `CreateMutexW` behind `Win32_Security`.
- Derived `Serialize, Deserialize` on `PixelRect` in `src-tauri/src/coordinator.rs` while keeping the module 100% Win32-independent.
- Implemented `SingleInstanceGuard`, `ensure_process_dpi_aware()`, `WorkAreaSnapshot` atomic persistence (`%LOCALAPPDATA%\TestBar\recovery-workarea.json`), `WatchdogCli` parser, chained non-blocking panic restore hook, hidden top-level `WS_OVERLAPPED` AppBar/`TaskbarCreated` message window (`AppBarMessageWindow` + internal `AppBarShellOps` trait with `Cell`-based interior mutability), dummy-HWND `flush_orphaned_appbars()`, and upgraded HiDPI watchdog recovery in `src-tauri/src/win_shell.rs`.
- Wired `main.rs` watchdog CLI dispatch (exit code `2` + stderr on invalid args) and `lib.rs` startup ordering (`SingleInstanceGuard` -> `ensure_process_dpi_aware` -> `capture_and_persist_original_workarea` -> `install_panic_restore_hook` -> `spawn_watchdog` -> `hide_native_taskbar` -> `start_appbar_message_window` with restore-on-error), synced `AppState::fullscreen_suppressed` between `ABN_FULLSCREENAPP` and the fast poller, guarded `h_slow` re-hiding with `is_restore_in_progress()`, and preserved `--no-hide-native`.
- Added 7 new deterministic unit tests in `src-tauri/src/win_shell.rs` (30 total Rust tests passing) covering all I/O & Edge-Case Matrix rows without mutating the live desktop taskbar or work area.

## Plan Change Log

## Review Triage Log

### 2026-11-20 — Review pass
- verdicts: 29 findings — high 13, medium 12, low 2, false 0, maybe-false 2
- findings:
  - `[high]` `[patch]` (blind-hunter) Missing Per-Monitor V2 DPI awareness before `tauri::Builder::build()` work-area capture and throughout `--watchdog` mode — Added `ensure_process_dpi_aware()` calling `SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)` before `capture_and_persist_original_workarea()` in `lib::run()` and at start of `run_watchdog()`.
  - `[high]` `[patch]` (blind-hunter) Aliased `&mut AppBarWindowContext` UB on re-entrant `DestroyWindow`/`SHAppBarMessage`/callback invocations — Converted mutable fields in `AppBarWindowContext` to `std::cell::Cell` and borrowed only shared `&*ctx_ptr` across all Win32 calls and callbacks.
  - `[high]` `[patch]` (blind-hunter) Watchdog races with quick restarts and clobbers newly launched TestBar instance — Acquired `SingleInstanceGuard::acquire()` in `run_watchdog` after `WaitForSingleObject`, exiting immediately on `Ok(None)` and holding the guard during restoration.
  - `[medium]` `[patch]` (blind-hunter) Missing `primary_monitor.covers(work_area)` containment check and stale work-area restore after display/DPI changes — Enforced containment in `WorkAreaSnapshot::is_valid()` and verified current monitor/DPI compatibility in `resolve_recovery_workarea` before calling `SPI_SETWORKAREA`.
  - `[medium]` `[patch]` (blind-hunter) Startup unconditionally overwrites `recovery-workarea.json` even when `Shell_TrayWnd` is already hidden after an unclean crash — Added `select_capture_snapshot()` in `capture_and_persist_original_workarea()` to preserve a compatible existing on-disk snapshot whenever `Shell_TrayWnd` is already hidden.
  - `[high]` `[patch]` (blind-hunter) Panic/hotkey deadlock on `APPBAR_MSG_WINDOW` lock / unbounded `jh.join()`, hook-thread stall before unhooking, and `h_slow` poller re-hiding race — Added `RESTORE_IN_PROGRESS` atomic checked by `h_slow` and `hide_native_taskbar()`, unhooked `WH_KEYBOARD_LL`/`WH_MOUSE_LL` and hotkey before restoring in `start_shell_hook_thread`, split non-blocking restore for panic/hotkey paths, and guarded `stop()` join on `PostMessageW` success.
  - `[high]` `[patch]` (blind-hunter) Ignored `ABM_NEW` failure, leaked AppBar/HWND on abnormal `GetMessageW` exit, and swallowed `start_appbar_message_window` error — Checked `abm_new` return status before setting `appbar_registered`, added post-loop `remove_appbar_on_hwnd` + `DestroyWindow` cleanup in `spawn_with_ops`, and restored native taskbars + returned `Err` from `lib.rs` `.setup()` on spawn failure.
  - `[medium]` `[patch]` (blind-hunter) `WM_DPICHANGED` DPI payload ignored by `apply_appbar_on_hwnd` and `position_taskbar_window` — Threaded `dpi_override: Option<u32>` through both `apply_appbar_on_hwnd` and `position_taskbar_window` on `WM_DPICHANGED`.
  - `[medium]` `[patch]` (blind-hunter) `parse_watchdog_args` ignores preceding args, misses `--watchdog=<pid>`, overwrites duplicate `--workarea`, and `main.rs` exits `0` on invalid CLI — Rewrote `parse_watchdog_args` to parse all `args[1..]` in any order with duplicate/unknown rejection and `--watchdog=<pid>` support; `main.rs` logs to stderr and exits `2` on `Invalid`.
  - `[medium]` `[patch]` (blind-hunter) `start_appbar_message_window` concurrent start/stop race, drop-under-lock, and missing unit tests — Checked `RESTORE_IN_PROGRESS` around spawn/store, dropped replaced windows outside `APPBAR_MSG_WINDOW` lock, and added unit tests for panic-hook helper, CLI edge cases, and AppBar reconciliation.
  - `[high]` `[patch]` (edge-case-hunter) Re-entrant `&mut *ctx_ptr` UB in `appbar_msg_wnd_proc` — Fixed via `Cell` interior mutability and shared `&*ctx_ptr` borrows.
  - `[high]` `[patch]` (edge-case-hunter) `SHAppBarMessage(ABM_NEW)` returning `0` falsely sets `appbar_registered = true` — Checked `abm_new` boolean return and returned early on failure so subsequent events retry.
  - `[high]` `[patch]` (edge-case-hunter) Watchdog races with newly started TestBar instance — Guarded `run_watchdog` restoration with `SingleInstanceGuard::acquire()`.
  - `[high]` `[patch]` (edge-case-hunter) `PostMessageW(WM_CLOSE)` failure or blocked AppBar thread hangs `jh.join()` — Used non-blocking restore on panic/hotkey paths and gated `stop()` `jh.join()` on `PostMessageW` success.
  - `[medium]` `[patch]` (edge-case-hunter) Stale work-area coordinates restored after display resolution/bounds change — Verified monitor/DPI compatibility before calling `SPI_SETWORKAREA`.
  - `[medium]` `[patch]` (edge-case-hunter) Arguments including `--workarea` or invalid flags before `--watchdog` silently ignored — Parsed all `args[1..]` cleanly in `parse_watchdog_args`.
  - `[low]` `[patch]` (edge-case-hunter) `save_workarea_snapshot_to` fails `create_dir_all("")` on relative filename and leaks `.tmp` on write/sync error — Filtered empty parent paths and removed `tmp_path` on `create`/`write_all`/`sync_all` failure, with unit test.
  - `[high]` `[patch]` (edge-case-hunter) Abnormal `GetMessageW` exit skips `ABM_REMOVE` and `DestroyWindow` — Added post-loop AppBar removal and `IsWindow`/`DestroyWindow` cleanup before freeing `ctx_ptr`.
  - `[medium]` `[patch]` (edge-case-hunter) `start_appbar_message_window` drops previous `AppBarMessageWindow` under `APPBAR_MSG_WINDOW` lock — Replaced slot and dropped previous window outside the lock.
  - `[low]` `[patch]` (edge-case-hunter) `SingleInstanceGuard::acquire_named` accepts interior NUL byte — Rejected `name.contains('\0')` with unit test.
  - `[high]` `[patch]` (edge-case-hunter) `start_appbar_message_window` error discarded after `hide_native_taskbar()` — Restored native taskbars and propagated `Err` in `lib.rs` `.setup()`.
  - `[medium]` `[patch]` (edge-case-hunter) `WM_DPICHANGED` DPI payload not passed to `apply_appbar_on_hwnd` — Threaded `Some(dpi)` into `apply_appbar_on_hwnd`.
  - `[medium]` `[patch]` (verification-gap) `resolve_recovery_workarea` in-memory snapshot fallback tier never exercised — Extended `recovery_workarea_resolution_priority_geometry_checks_and_crash_capture` to assert in-memory priority over disk, invalid explicit fallback to in-memory, and invalid/mismatched in-memory fallback to disk.
  - `[high]` `[patch]` (verification-gap) `AppBarMessageWindow` test bypassed all AppBar reconciliation, mode-switch, recursion-guard, and teardown logic — Introduced internal `AppBarShellOps` trait with `FakeAppBarShellOps` recording fake and tested `ABM_NEW` retry, `ABN_POSCHANGED` dedup vs shift, Dock/Edge mode switches, `WM_DPICHANGED` DPI override, `TaskbarCreated` re-hide + `force_new`, and `ABM_REMOVE` cleanup.
  - `[high]` `[patch]` (intent-alignment) AppBar shell mutations bypassed in tests; `ABM_NEW` status and `start_appbar_message_window` errors discarded — Covered by `AppBarShellOps` test suite, `abm_new` retry check, and `.setup()` error propagation.
  - `[medium]` `[patch]` (intent-alignment) `WM_DPICHANGED` DPI payload parsed but unused by recalculation paths — Passed `Some(dpi)` to `apply_appbar_on_hwnd` and `position_taskbar_window`.
  - `[maybe-false]` `[defer]` (intent-alignment) Live multi-process single-instance rejection on `Local\TestBar_SingleInstance_v1` and `--watchdog` bypass — Deferred to interactive Windows 11 smoke testing with compiled `testbar.exe`.
  - `[medium]` `[patch]` (intent-alignment) Static startup snapshot applied verbatim after display/DPI change — Added monitor/DPI compatibility check in `resolve_recovery_workarea`.
  - `[maybe-false]` `[defer]` (intent-alignment) Live OS shell recovery (`taskkill /F` watchdog, `explorer.exe` `TaskbarCreated` broadcast, `Ctrl+Alt+Shift+Esc`, live `SPI_SETWORKAREA`) — Deferred to interactive Windows 11 smoke testing to avoid mutating the host desktop during `cargo test`.

## Design Notes

- **Single-instance mutex ownership:** Create `Local\TestBar_SingleInstance_v1` via `CreateMutexW(None, false, name)` (unowned kernel mutex object) and check `GetLastError() == ERROR_ALREADY_EXISTS`. Because ownership is handle-lifetime-based rather than thread-owned, dropping `SingleInstanceGuard` (`CloseHandle`) works safely regardless of thread affinity while preventing any duplicate process launch.
- **Hidden top-level `WS_OVERLAPPED` window thread:** Run the dedicated window on a lightweight thread (`start_appbar_message_window`) that registers class `TestBarAppBarMsgWnd`, creates the window with `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, `WS_OVERLAPPED`, `hWndParent = HWND(0)` (never `HWND_MESSAGE`), registers `TaskbarCreated`, registers the bottom AppBar (`uCallbackMessage = WM_APP + 10`), and pumps `GetMessageW`. Live `set_bar_mode` updates post a custom `WM_APP + 12` message to that HWND so all `SHAppBarMessage` updates run on the owning window thread; `stop_appbar_message_window` posts `WM_CLOSE` (which calls `ABM_REMOVE` + `DestroyWindow` + `PostQuitMessage(0)`).
- **Watchdog `--workarea` + AppBar flush:** Format `--workarea l,t,r,b` from the captured `SPI_GETWORKAREA` snapshot. On parent exit, the watchdog closes the process handle, creates a short-lived hidden top-level `WS_OVERLAPPED` dummy window to run `ABM_NEW` + `ABM_REMOVE` (prompting Explorer to prune dead AppBar reservations), calls `ShowWindow(..., SW_SHOW)` on `Shell_TrayWnd` and all `Shell_SecondaryTrayWnd`, and calls `SPI_SETWORKAREA` (`SPIF_SENDCHANGE`) with `--workarea` or `recovery-workarea.json`.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml` -- expected: exits `0`, all Rust unit/integration tests pass with zero warnings.
- `pnpm test` -- expected: exits `0`, all Vitest tests pass non-interactively.
- `pnpm build` -- expected: exits `0`, `tsc && vite build` succeeds cleanly.
- Static grep check on `src-tauri/` -- expected: zero matches for `bottom - 48`, zero `HWND_MESSAGE` usage for `TaskbarCreated` or AppBar, and `SingleInstanceGuard` acquired before any shell mutation.

## Auto Run Result

- **Summary:** Hardened TestBar's Windows 11 OS shell lifecycle with an RAII named single-instance mutex (`Local\TestBar_SingleInstance_v1`), Per-Monitor V2 DPI awareness before work-area capture and in watchdog mode, atomic HiDPI `SPI_GETWORKAREA` snapshot persistence to `%LOCALAPPDATA%\TestBar\recovery-workarea.json` (with zero hardcoded `mon.bottom - 48` offsets), a dedicated hidden top-level `WS_OVERLAPPED` AppBar & `TaskbarCreated` message window (`ABN_POSCHANGED`, `ABN_FULLSCREENAPP`, `WM_DISPLAYCHANGE`, `WM_DPICHANGED`, live Dock/Edge mode switching), non-blocking chained Rust panic and `Ctrl+Alt+Shift+Esc` recovery, and an upgraded `--watchdog <pid> --workarea <l,t,r,b>` recovery path with dummy-HWND orphan AppBar flushing and quick-restart mutex safety.
- **Files Changed:**
  - `src-tauri/Cargo.toml` — Enabled `"Win32_UI_HiDpi"` and `"Win32_Security"` features in `windows` 0.58.
  - `src-tauri/src/coordinator.rs` — Derived `Serialize, Deserialize` on `PixelRect` while keeping `coordinator.rs` 100% Win32-independent.
  - `src-tauri/src/win_shell.rs` — Added `SingleInstanceGuard`, `ensure_process_dpi_aware()`, `WorkAreaSnapshot` atomic persistence & geometry/crash-preservation checks, `WatchdogCli` full-argv parser, non-blocking chained panic restore hook, hidden top-level `WS_OVERLAPPED` `AppBarMessageWindow` (`AppBarShellOps` trait + `Cell` interior mutability), `flush_orphaned_appbars()`, upgraded `spawn_watchdog`/`run_watchdog`/`restore_native_taskbar` (removing `mon.bottom - 48`), and 7 deterministic unit tests.
  - `src-tauri/src/main.rs` — Switched CLI dispatch to `parse_watchdog_args(&args)` with non-zero exit (`2`) on invalid watchdog arguments.
  - `src-tauri/src/lib.rs` — Acquired `SingleInstanceGuard` first in `run()`, enabled DPI awareness and captured/persisted work area before hiding/spawning watchdog, hosted AppBar on the dedicated hidden top-level message window, routed `ShellAppBarEvent` to the main thread with synced `fullscreen_suppressed` and `WM_DPICHANGED` DPI override, guarded `h_slow` with `is_restore_in_progress()`, and preserved `--no-hide-native`.
- **Review Findings Breakdown:**
  - **Patched (14 grouped entries by entry verdict):** 6 `high` entries (Per-Monitor V2 DPI awareness before `tao` & in watchdog; aliased `&mut *ctx_ptr` UB eliminated via `Cell`; watchdog quick-restart `SingleInstanceGuard` race guard; non-blocking panic/hotkey restore + unhook ordering + `h_slow` re-hide guard; `ABM_NEW` failure retry + post-loop HWND/AppBar cleanup + `.setup()` error restore; `AppBarShellOps` trait & AppBar reconciliation test suite), 6 `medium` entries (`WorkAreaSnapshot` containment + monitor/DPI compatibility check; startup crash-preservation of `recovery-workarea.json` when `Shell_TrayWnd` is hidden; `WM_DPICHANGED` DPI propagation to AppBar & `position_taskbar_window`; `parse_watchdog_args` full-argv / `--watchdog=<pid>` / duplicate rejection & exit code `2`; `start_appbar_message_window` concurrency & drop-outside-lock + panic-hook test; `resolve_recovery_workarea` in-memory priority & fallback tests), and 2 `low` entries (`acquire_named` interior NUL rejection; `save_workarea_snapshot_to` relative-path parent filter & `.tmp` error cleanup).
  - **Deferred (2):** Live multi-process single-instance rejection on `Local\TestBar_SingleInstance_v1` and `--watchdog` bypass (`medium (unverified)`); live Windows 11 OS shell recovery across real `explorer.exe` restarts, `taskkill /F` watchdog recovery, `Ctrl+Alt+Shift+Esc`, and HiDPI `SPI_SETWORKAREA` (`medium (unverified)`).
  - **Rejected (0):** None.
- **Follow-up Review Recommended:** `true` (first pass patched 6 `high`, 6 `medium`, and 2 `low` entries; specific unverified risk: live Windows 11 desktop execution of `testbar.exe` verifying real Per-Monitor V2 `SPI_GETWORKAREA`/`SPI_SETWORKAREA` at 125%–200% scaling, real `explorer.exe` `TaskbarCreated` broadcasts, `taskkill /F` companion watchdog recovery, and `Ctrl+Alt+Shift+Esc` emergency restore).
- **Verification Performed:**
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **PASS (exit 0)** — 30/30 tests passed (19 `coordinator::tests`, 9 `win_shell::tests`, 1 `config::tests`, 1 `integration_tests`), zero Rust compiler warnings.
  - `pnpm test`: **PASS (exit 0)** — 12/12 Vitest tests in `src/lib/layoutMap.test.ts` passed non-interactively.
  - `pnpm build`: **PASS (exit 0)** — `tsc && vite build` succeeded cleanly.
  - Static grep audit on `src-tauri/src/*.rs`: **PASS** — zero matches for `bottom - 48`, zero `HWND_MESSAGE` usage for `TaskbarCreated` or AppBar, `SingleInstanceGuard::acquire()` precedes `ensure_process_dpi_aware()`, `capture_and_persist_original_workarea()`, `install_panic_restore_hook()`, `spawn_watchdog()`, and `hide_native_taskbar()`.
- **Residual Risks:** Live multi-process and live desktop OS shell recovery (`testbar.exe` duplicate launch, `taskkill /F` watchdog restore, `explorer.exe` restart `TaskbarCreated`, and HiDPI scaling transitions) remain to be smoke-tested interactively on Windows 11 once Epic 1 completes.
