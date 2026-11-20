---
id: 1
type: story
title: "Restore compiling shell baseline, pure WindowCoordinator & automated test harness"
parent: epic-shell-foundation
covers: [R2, R3, R5]
after: []
refined: true
hitl: false
risk: high
---

# Restore compiling shell baseline, pure WindowCoordinator & automated test harness

## Description

This tracer-bullet story brings the broken brownfield TestBar workspace back to a clean, compiling, and test-verified baseline across Rust, Tauri config, and React/TypeScript. It establishes the Win32-independent `WindowCoordinator` state machine and fullscreen/AppBar pure helpers, removes `WS_THICKFRAME` and `1×1` window-shrinking bugs, repairs the broken `lib.rs`/`win_shell.rs` interface cleanly with brownfield event compatibility, and installs the Vitest frontend test harness.

## Requirements

1. **Pure `WindowCoordinator` visibility, preemption, debounce & modal guard** (R3)
   **Given** a `WindowCoordinator` managing the 3 shell windows (`taskbar`, `start`, `flyout`) with closed `FlyoutKind` (`preview | tray | sound | calendar | context_app | context_bar | osd`)
   **When** interactive click flyouts, hover previews, volume OSD, focus-loss dismissals, rapid re-clicks within `180ms`, or native modal picker guards occur
   **Then** hover `preview` and transient `osd` never overwrite an open click flyout (`tray`, `sound`, `calendar`, `context_*`), focus-loss dismissal triggers a `180ms` anti-double-toggle debounce for that trigger, an active modal picker suppresses `start` focus-loss dismissal, and taskbar bounds updates never force `taskbar` visible while fullscreen-suppressed.

2. **Single-authority strict fullscreen classification & DPI AppBar rect calculation** (R2, R3)
   **Given** window geometry, Win32 style flags, window class names, `QUERY_USER_NOTIFICATION_STATE`, and monitor DPI/bar mode inputs
   **When** the pure fullscreen classifier and AppBar rect calculator evaluate them
   **Then** `QUNS_RUNNING_D3D_FULL_SCREEN` / `QUNS_PRESENTATION_MODE` or a non-shell, non-TestBar frameless window covering the primary monitor (lacking `WS_CAPTION` and `WS_THICKFRAME`) classifies as fullscreen, while `Progman`/`WorkerW` and normal maximized/borderless desktop windows with caption or thick frame classify as non-fullscreen; and DPI-scaled AppBar rects compute `64px` logical height for Dock mode and `52px` logical height for Edge mode.

3. **Non-resizable transparent window config & strict local CSP** (R3, R5)
   **Given** the Tauri v2 window and security configuration
   **When** the app configuration is validated and loaded
   **Then** all three pre-warmed windows (`taskbar`, `start`, `flyout`) have `resizable: false`, `decorations: false`, `transparent: true`, `shadow: false`, `skipTaskbar: true`, and `security.csp` enforces a strict local policy (`default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'`) instead of `null`.

4. **Compiling shell host without `1×1` shrinking, with non-activating preview/OSD & safe click-outside dispatch** (R2, R3)
   **Given** the brownfield Rust backend and React frontend
   **When** `lib.rs` and `win_shell.rs` are repaired and wired through `WindowCoordinator`
   **Then** `src-tauri` compiles cleanly with zero errors, `start` and `flyout` show/hide via window visibility (`SW_SHOW` / `SW_SHOWNOACTIVATE` / `SW_HIDE`) instead of shrinking to `1×1` at `(0, 0)`, `flyout` returns `MA_NOACTIVATE` on `WM_MOUSEACTIVATE` when in `preview`/`osd` mode without flipping `GWL_EXSTYLE` per open, `ShellHookEvent::GlobalMouseDown` is dispatched off the hook thread (never blocking `WH_MOUSE_LL`), and canonical `FlyoutKind`/event names preserve brownfield compatibility via serde aliases (`quick` -> `sound`, `ctx_app` -> `context_app`, `ctx_taskbar` -> `context_bar`) and compatible event listeners.

5. **Automated Rust and Vitest verification harness** (R5)
   **Given** the repository workspace
   **When** `pnpm build`, `pnpm test` (`vitest run`), and `cargo test --manifest-path src-tauri/Cargo.toml` execute
   **Then** all three commands exit `0` non-interactively with passing unit tests covering `WindowCoordinator` (preemption, debounce, modal guard, fullscreen suppression), fullscreen classification, DPI AppBar rect math, existing `config.rs` tests, and `src/lib/layoutMap.ts` (RU↔EN wrong-keyboard-layout fuzzy matching including `ntkt` -> Telegram).

## Boundaries

- **Must not change:** Do not implement single-instance mutex, top-level `TaskbarCreated` message window, or `--watchdog --workarea` rewrite (owned by Story `1.2`); do not rewrite the full `WH_KEYBOARD_LL`/`WH_MOUSE_LL` bounded `sync_channel(64)` architecture (owned by Story `1.3`); do not change `config.rs` schema or add `PinItem` fields (owned by Story `1.4`); do not modify `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, or `system_widgets.rs` beyond keeping them compiling.
- **Must preserve:** Existing brownfield IPC commands and views (`TaskbarView`, `StartView`, `FlyoutView`) continue working seamlessly with canonical + aliased `FlyoutKind` and event names; launching `testbar --no-hide-native` displays the borderless non-resizable taskbar without `WS_THICKFRAME` artifacts or `1×1` shrinking.

## References

- parent — `_bmad-output/initiative-testbar/tickets/epic-shell-foundation/epic-shell-foundation.md`, sections Requirements (R2, R3, R5) & Done when
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, sections AD-1, AD-6, AD-7 & Consistency Conventions
- design — `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, section 2 (Core Tokens — Window Chassis & Taskbar Geometry)

## Notes

- Implementation guide: Pure state machine and calculators live in `src-tauri/src/coordinator.rs`; `src-tauri/src/lib.rs` and `src-tauri/src/win_shell.rs` adopt `WindowCoordinator`; `src-tauri/tauri.conf.json` locks `resizable: false` and local CSP; `package.json` adds `vitest` and `"test": "vitest run"` with `src/lib/layoutMap.test.ts`.
- Brownfield compatibility: Keep serde aliases on `FlyoutKind` (`quick` -> `Sound`, `ctx_app` -> `ContextApp`, `ctx_taskbar` -> `ContextBar`) and ensure `src/lib/ipc.ts` + views handle canonical (`flyout-hide`, `start-state`) alongside any legacy listeners cleanly so zero regressions occur before Epics 2–4 migrate each view.
- `GlobalMouseDown` dispatch: Dispatch `ShellHookEvent::GlobalMouseDown(x, y)` off the `WH_MOUSE_LL` callback thread (e.g., via `tauri::async_runtime::spawn` or worker thread) so mouse input never stalls; full bounded `sync_channel(64)` hook architecture follows in Story `1.3`.
- High-risk check: Confirm via grep/inspection that zero `set_window_bounds(..., 1, 1, ...)` calls and zero `"resizable": true` entries remain in `src-tauri/`, and that `flyout` `WM_MOUSEACTIVATE` handles `preview`/`osd` without toggling `GWL_EXSTYLE` on every open.
