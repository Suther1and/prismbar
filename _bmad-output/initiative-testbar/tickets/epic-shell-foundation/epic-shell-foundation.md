---
type: epic
title: "Core Shell Host, Safe AppBar/Watchdog & 3-Window Lifecycle"
parent: initiative-testbar
status: in-progress
covers: [R1, R3, R5, R8]
sources: [_bmad-output/initiative-testbar/initiative-testbar.md]
companions: [_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md, _bmad-output/initiative-testbar/ux-testbar/DESIGN.md, _bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md]
after: []
refined: true
risk: high
---

# Core Shell Host, Safe AppBar/Watchdog & 3-Window Lifecycle

## Description

Establish the rock-solid Windows 11 shell foundation for TestBar: restore clean Rust + TypeScript compilation and automated tests (Rust unit tests + Vitest), enforce non-resizable transparent 3-window lifecycle under one `WindowCoordinator`, implement single-instance protection and top-level hidden AppBar/`TaskbarCreated`/HiDPI watchdog restoration, isolate global hooks onto a bounded non-blocking channel, apply strict single-authority fullscreen auto-hide, and lock down command-scoped atomic configuration persistence.

## Outcome

TestBar launches cleanly as a single instance on Windows 11, reserves HiDPI work area and hides native taskbars safely, never stalls system input or false-hides over desktop windows, restores the native taskbar reliably on every exit or crash path, and passes `pnpm build`, `pnpm test`, and `cargo test`.

## Requirements

- **R1 (parent R1):** Acquire single-instance protection (`Local\TestBar_SingleInstance_v1`), reserve bottom desktop work area across DPI/monitor changes and Dock/Edge modes, hide native Windows taskbars without modifying `explorer.exe`, re-hide on `TaskbarCreated` Explorer restarts, and guarantee automatic restoration of native taskbars and captured HiDPI work area on normal exit, crash (watchdog), panic, or `Ctrl+Alt+Shift+Esc`.
- **R2 (parent R3):** Automatically suppress taskbar visibility only when a true fullscreen game or presentation is active (single classifier combining `SHQueryUserNotificationState` and frameless full-monitor checks), never false-hiding on maximized/borderless desktop apps and never allowing resize updates to leak the taskbar over fullscreen apps.
- **R3 (parent R1, R8):** Manage the 3 pre-warmed transparent frameless non-resizable windows (`taskbar`, `start`, `flyout`) through one testable `WindowCoordinator` state machine that prevents `1×1` window shrinking, double-toggle races, click-flyout preemption by hover previews, focus loss during modal file pickers, and focus theft by preview/OSD flyouts.
- **R4 (parent R1, R5):** Keep `WH_KEYBOARD_LL` and `WH_MOUSE_LL` callbacks strictly non-blocking via a bounded channel to a worker thread so Windows never unhooks them, suppressing lone `Win` key for Start while preserving all `Win+<key>` OS shortcuts and forwarding `GlobalMouseDown` for click-outside dismissal.
- **R5 (parent R8):** Own `%APPDATA%\TestBar\config.json` with `schema_version`, monotonic `revision`, atomic writes, command-scoped mutations (no full-document overwrite IPC), `PinItem` matching fields (`match_exe`, `match_aumid`, `combo_targets`), HKCU autostart, strict local CSP, and automated Rust + Vitest verification (`pnpm test` + `cargo test`).

## Constraints

- **C1 (parent C1):** Zero DLL injections, hooks into `explorer.exe` memory, or system binary patches.
- **C2 (parent C3):** Local standalone Windows 11 execution relying solely on built-in WebView2 Runtime.

## Done when

1. `pnpm build`, `pnpm test`, and `cargo test --manifest-path src-tauri/Cargo.toml` pass with zero errors.
2. `WindowCoordinator`, fullscreen classifier, DPI AppBar rect calculation, `config.json` atomic CRUD/reorder, and `layoutMap.ts` are covered by automated tests.
3. Launching `testbar` acquires the single-instance mutex, hides native taskbars, registers DPI-aware AppBar via a top-level hidden window handling `TaskbarCreated` and `WM_DISPLAYCHANGE`, and restores both native taskbars and original HiDPI work area on normal exit, `Ctrl+Alt+Shift+Esc`, and forced kill (`--watchdog`).
4. Global keyboard and mouse hooks perform zero IPC or window calls on the hook thread, preserve `Win+<key>` chords, dismiss open popups on click-outside without double-toggle, and never hide the taskbar over normal maximized windows.

## Boundaries

- **Owns:** Compiling baseline, `coordinator.rs`, `win_shell.rs`, `main.rs`, `config.rs`, `tauri.conf.json` window/CSP baseline, Vitest setup, and core `lib.rs` coordinator wiring.
- **Belongs to siblings:** Window enumeration/UWP icons/previews/context menus (`epic-taskbar-windows`), tray/audio/calendar UI & backends (`epic-tray-audio-calendar`), Start Spaces/Edit Mode/Universal Pinning/Search (`epic-start-spaces-search`), and SMTC/NVML/Steam/release `.exe` packaging (`epic-hardware-media-games`).

## References

- parent — `_bmad-output/initiative-testbar/initiative-testbar.md`, section Requirements (R1, R3, R8)
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, sections AD-1, AD-2, AD-3, AD-4, AD-6, AD-7
- design — `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, section 2 (Core Tokens — Window Chassis & Taskbar Geometry)

## Notes

- Decision: 2026-11-20 — Tracer bullet is entry `1` ("Restore compiling shell baseline, pure WindowCoordinator & automated test harness"): it proves the end-to-end Rust + Tauri config + TypeScript + test loop first so every later story builds on green compilation and automated verification.
- Decision: 2026-11-20 — Stories `1.1 -> 1.2 -> 1.3 -> 1.4 -> 1.5` run sequentially because `1.2`, `1.3`, and `1.4` all modify shared shell orchestration (`win_shell.rs`, `lib.rs`).
- Decision: 2026-11-20 — Epic 1 owns the `R5` OS foundation (lone `Win` key hook, modal focus guard, and `PinItem` config authority); Epic 4 owns the `R5` Start Menu Spaces UI, Edit Mode, Universal Pinning, Power, and `Shift` layer.
- Decision: 2026-11-20 — Epic 1 delivers the R8 build/test/autostart/CSP foundation; final standalone release `.exe` packaging and full UX regression sign-off under R8 live in Epic 5.
- Decision: 2026-11-20 — Includes closing `Refactor sweep` (entry `5`) per BMAD slicing policy for epics exceeding 3 entries.
- High-risk check: Verify no `1×1` window shrinking, no `resizable: true` flags, and no synchronous IPC inside `WH_KEYBOARD_LL`/`WH_MOUSE_LL` callbacks exist after Epic 1 completes.
