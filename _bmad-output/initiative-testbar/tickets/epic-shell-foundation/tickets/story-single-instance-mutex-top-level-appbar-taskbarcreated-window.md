---
id: 2
type: story
title: "Single-instance mutex, top-level AppBar/TaskbarCreated window & HiDPI watchdog recovery"
parent: epic-shell-foundation
covers: [R1]
after: [1]
refined: true
hitl: false
risk: high
---

# Single-instance mutex, top-level AppBar/TaskbarCreated window & HiDPI watchdog recovery

## Description

Hardens TestBar's Windows 11 OS shell lifecycle so a single instance safely hides native taskbars, reserves DPI-aware bottom work area via a dedicated hidden top-level `WS_OVERLAPPED` message window (surviving `explorer.exe` restarts, display/DPI changes, and Dock/Edge switches), and guarantees exact HiDPI work-area and native-taskbar restoration on normal exit, Rust panic, `Ctrl+Alt+Shift+Esc`, or forced process termination via the companion watchdog.

## Requirements

1. **Named single-instance mutex guard** (R1)
   **Given** TestBar starting in normal shell mode (not `--watchdog`)
   **When** `Local\TestBar_SingleInstance_v1` is acquired at startup before touching native taskbars, capturing work area, or spawning the watchdog
   **Then** the first instance holds the OS mutex for its entire lifetime (RAII guard), and any second instance detects `ERROR_ALREADY_EXISTS` and exits cleanly without modifying `Shell_TrayWnd`, `SPI_GETWORKAREA`, or AppBar state.

2. **Captured & persisted HiDPI `SPI_GETWORKAREA` snapshot** (R1)
   **Given** a primary monitor at any DPI scale (100%, 125%, 150%, 200%)
   **When** TestBar initializes native taskbar hiding
   **Then** it captures the exact original `SPI_GETWORKAREA` rect before hiding `Shell_TrayWnd` or registering an AppBar, persists it atomically to `%LOCALAPPDATA%\TestBar\recovery-workarea.json`, and uses that captured rect (never a hardcoded `mon.bottom - 48` offset) on every restoration path.

3. **Hidden top-level `WS_OVERLAPPED` AppBar & `TaskbarCreated` message window** (R1)
   **Given** TestBar running with native taskbar replacement enabled
   **When** the AppBar is registered, the user toggles Dock (`64px` logical) vs Edge (`52px` logical), display resolution/DPI changes (`WM_DISPLAYCHANGE`, `WM_DPICHANGED`), `ABN_POSCHANGED` / `ABN_FULLSCREENAPP` arrives on `WM_APP + 10`, or `explorer.exe` restarts broadcasting `TaskbarCreated`
   **Then** a dedicated hidden top-level `WS_OVERLAPPED` window (never `HWND_MESSAGE`) receives those messages, scales AppBar height via `coordinator::compute_appbar_rect`, re-hides `Shell_TrayWnd` / `Shell_SecondaryTrayWnd` and re-registers the AppBar on `TaskbarCreated`, updates taskbar bounds on mode/display/DPI changes, and forwards `ABN_FULLSCREENAPP` notifications for fullscreen re-evaluation.

4. **Rust panic hook & upgraded HiDPI watchdog recovery** (R1)
   **Given** TestBar running with native taskbar replacement enabled
   **When** the main process exits cleanly, panics, triggers `Ctrl+Alt+Shift+Esc`, or is force-killed (`taskkill /F`)
   **Then** a chained Rust panic hook restores native taskbars immediately on panic, the spawned companion watchdog (`testbar.exe --watchdog <pid> --workarea <l,t,r,b>`) waits on the parent PID, flushes orphaned AppBar reservations using a local dummy-HWND `ABM_NEW` + `ABM_REMOVE` sequence, restores `Shell_TrayWnd` and all `Shell_SecondaryTrayWnd` windows (`SW_SHOW`), and restores the captured HiDPI `SPI_SETWORKAREA` (`SPIF_SENDCHANGE`) from `--workarea` (falling back to `recovery-workarea.json`).

5. **Deterministic unit tests & clean verification** (R1)
   **Given** the pure helper functions and Win32 tests in `src-tauri`
   **When** `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` run
   **Then** all three commands exit `0` with passing unit tests covering `recovery-workarea.json` atomic serialization/roundtrip, `--watchdog <pid> --workarea <l,t,r,b>` CLI parsing (valid, missing workarea, malformed args), single-instance duplicate mutex rejection, and hidden top-level `WS_OVERLAPPED` window creation/cleanup.

## Boundaries

- **Must not change:** Do not rewrite `WH_KEYBOARD_LL` / `WH_MOUSE_LL` into a bounded `sync_channel(64)` worker thread (owned by Story `1.3`); do not change `config.rs` schema or `PinItem` fields (owned by Story `1.4`); do not alter `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, `system_widgets.rs`, or frontend React UI components.
- **Must preserve:** Zero DLL injection or `ReadProcessMemory` on `explorer.exe`; `--no-hide-native` mode continues to skip hiding native taskbars, AppBar registration, and watchdog spawning; all Story `1.1` `WindowCoordinator`, `tauri.conf.json`, and Vitest checks stay green.

## References

- parent — `_bmad-output/initiative-testbar/tickets/epic-shell-foundation/epic-shell-foundation.md`, sections Requirements (R1) & Done when
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, section AD-2
- design — `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, section 2 (Bottom Taskbar — 52px bar + 12px Dock margin)

## Notes

- Implementation guide: Keep `main.rs` watchdog CLI dispatch parsing `--watchdog <pid>` plus optional `--workarea <l,t,r,b>`; place pure snapshot/arg parsing helpers and Win32 `SingleInstanceGuard`, top-level `WS_OVERLAPPED` AppBar message window, panic hook helper, and dummy-HWND AppBar flush in `src-tauri/src/win_shell.rs`; wire single-instance acquisition, panic hook, message window startup, and live `set_bar_mode` AppBar height updates in `src-tauri/src/lib.rs`.
- Win11 Build 26200 invariant: Never create the `TaskbarCreated` / AppBar receiver as `HWND_MESSAGE` (message-only windows never receive top-level broadcasts); use a hidden top-level `WS_OVERLAPPED` window with `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`.
- High-risk check: Confirm via grep/inspection that zero hardcoded `mon.bottom - 48` offsets remain in `src-tauri/`, zero `HWND_MESSAGE` is used for `TaskbarCreated`, and `SingleInstanceGuard` is acquired before any native taskbar hide or work-area capture.
