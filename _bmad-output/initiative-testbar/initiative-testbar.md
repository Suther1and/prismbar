---
type: initiative
title: "TestBar — Windows 11 Taskbar & Start Menu Replacement"
parent: none
status: in-progress
sources: [_bmad-output/initiative-testbar/brief-testbar/brief-testbar.md, _bmad-output/initiative-testbar/prd-testbar/prd-testbar.md]
companions: [_bmad-output/initiative-testbar/ux-testbar/ux-testbar.md, _bmad-output/initiative-testbar/ux-testbar/DESIGN.md, _bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md, _bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md]
risk: high
---

# TestBar — Windows 11 Taskbar & Start Menu Replacement

## Description

Deliver a standalone Windows 11 desktop application (`testbar.exe`) that cleanly replaces the stock bottom taskbar and Start Menu in full fidelity to the approved Obsidian Layered Glass UX (`DESIGN.md`, `EXPERIENCE.md`, and `mockups/complete-ux.html`) while preserving rock-solid OS stability, instant response, and guaranteed native taskbar recovery.

## Outcome

A Windows 11 daily user runs TestBar as their primary bottom taskbar and Start Menu with <50 ms Start response, zero visual glitches, and guaranteed automatic restoration of the stock Windows taskbar on exit, crash, or emergency hotkey.

## Requirements

- **R1 (prd FR-01, FR-02, NFR-04):** Reserve the bottom desktop work area on Windows 11 across DPI and monitor changes, safely hide native Windows taskbars without modifying `explorer.exe`, and guarantee automatic native taskbar restoration on normal exit, crash, `explorer.exe` restart, or `Ctrl+Alt+Shift+Esc`.
- **R2 (prd FR-03, FR-04, FR-05, FR-06, FR-20):** Display pinned and running Windows applications on the bottom taskbar with crisp native icons (including UWP), state indicators, strictly contained hover feedback, live window hover previews (focus and close), Dock / Edge-to-Edge layout switching, and full right-click menus (Jump List actions, New Window, Run as Admin, Pin/Unpin, Close, End Task, and empty-bar menu).
- **R3 (prd FR-07):** Automatically hide the taskbar when a true fullscreen game or video is active without false-hiding on normal maximized or borderless desktop windows, and restore it cleanly upon leaving fullscreen.
- **R4 (prd FR-14, FR-15, FR-16, FR-18):** Provide a compact 6-column icon-only tray overflow (`^`) sized for 30–40+ background apps with stable hover tooltips and right-click actions, live foreground keyboard layout indicator and toggle (`RU`/`EN`), scroll-wheel volume adjustment with non-jittering OSD plus synced Quick Sound & per-app Mixer and output device switcher, and an interactive Clock & Calendar with notifications.
- **R5 (prd FR-08, FR-10, FR-11, FR-12, FR-13, FR-23):** Open Start Menu via Start button or lone `Win` key without breaking `Win+<key>` OS shortcuts; provide 4 adaptive Spaces (**Личное · Работа · Студия · Игры**) plus custom spaces, drag-and-drop Edit Mode with centered unpin badges, universal pinning for apps/folders/files/URLs, All Apps browser, Power actions with 2-click armed restart/shutdown, `Shift` alternate action layer, and persistent local configuration.
- **R6 (prd FR-09, NFR-02, NFR-06):** Provide instant type-anywhere local search inside Start Menu with zero dropped leading keystrokes and RU↔EN wrong-keyboard-layout fuzzy matching across installed apps, UWP apps, games, pinned spaces, folders, and user files, with keyboard navigation, `Enter` launch, and `Ctrl+Enter` run-as-admin.
- **R7 (prd FR-17, FR-19):** Provide live system integrations inside Start Menu: real media playback controls (Now Playing), live clipboard history (pin/copy/clear), human-readable PC hardware telemetry (CPU %, RAM/64 GB, GPU temp/load/VRAM) with top memory/GPU consumers and reliable process termination, Bluetooth peripheral battery indicator, and dynamic Steam library auto-discovery with real cover art and 1-click game combos.
- **R8 (prd NFR-01, NFR-03, NFR-05, FR-23):** Deliver a locally built standalone Windows 11 `.exe` with smooth GPU-composited animations (60–240 FPS), low idle CPU/RAM footprint, optional Windows autostart, and automated build/test verification.

## Constraints

- **C1 (prd NFR-04):** Zero DLL injections, hooks into `explorer.exe` memory, or system binary patches.
- **C2 (brief Границы версии 1.0 (MVP), prd NFR-06):** Strictly dark Obsidian Layered Glass theme only; zero light theme, accent color pickers, VPN toggle, weather widget, calculator, quick notes, monitor layout buttons, Focus/DND filler toggles, «Режимы работы», raw GPU RPM/MHz numbers, emoji, or inline UI tutorial hints (per approved `DESIGN.md` and `EXPERIENCE.md`).
- **C3 (prd NFR-05):** Standalone local Windows 11 build relying solely on the OS-provided WebView2 Runtime without external runtime installers.

## Done when

1. `pnpm build`, frontend unit tests, `cargo test`, and `pnpm tauri build` succeed cleanly on Windows 11 producing a working standalone `testbar.exe`.
2. Native Windows 11 taskbar hides cleanly on startup, work area reserves without clipping maximized windows, and native taskbar + original HiDPI work area restore automatically on exit, forced termination (watchdog), `explorer.exe` restart, and `Ctrl+Alt+Shift+Esc`.
3. Bottom taskbar displays real pinned + running windows (including UWP icons), gapless hover previews, native right-click menus, non-jittering volume OSD, compact tray overflow, layout toggle, and calendar without double-toggle or fullscreen false-hiding bugs.
4. Start Menu opens cleanly on Start button and lone `Win` key (preserving all `Win+<key>` shortcuts), renders all 4 adaptive Spaces + custom spaces with universal pinning, Edit Mode, instant RU↔EN search, live SMTC/clipboard/NVML/Steam/BT integrations, and persists configuration reliably.
5. Visual implementation adheres strictly to `DESIGN.md` and `EXPERIENCE.md` with zero excluded widgets, zero emoji, and zero tutorial hints.

## Boundaries

- **Out of scope (v1.0):** Tiling window management (GlazeWM/Komorebi style), third-party plugin or custom CSS theme store, top/left/right taskbar edge placement, light theme, Split Islands layout mode, and multi-monitor independent taskbars beyond primary monitor + clean secondary taskbar hiding.
- **Touch point:** Windows Shell (`Shell_TrayWnd` / `Shell_SecondaryTrayWnd`, `SHAppBarMessage`, `TaskbarCreated`) — consumed only for hide/show and work-area reservation; owner: `epic-shell-foundation`.
- **Touch point:** Windows Registry (`HKCU\...\Run`, `HKCU\Software\Valve\Steam`) — consumed for autostart and Steam library discovery; owner: `epic-shell-foundation` (autostart), `epic-hardware-media-games` (Steam).
- **Touch point:** `%APPDATA%\TestBar\config.json` — single atomic configuration store shared across all views; owner: `epic-shell-foundation`.
- **Tracer path:** `epic-shell-foundation` establishes a compiling, tested 3-window host with safe AppBar/watchdog/hooks -> `epic-taskbar-windows` proves real window tracking, icons, previews, and context menus -> `epic-tray-audio-calendar` completes right-side system flyouts -> `epic-start-spaces-search` completes Start Spaces, universal pinning, and RU↔EN search -> `epic-hardware-media-games` finishes live hardware/media/Steam integrations and release `.exe` delivery.

## References

- brief — `_bmad-output/initiative-testbar/brief-testbar/brief-testbar.md`, sections Суть продукта, Границы версии 1.0, Критерии успеха
- prd — `_bmad-output/initiative-testbar/prd-testbar/prd-testbar.md`, sections 2–4 (UJ-1..UJ-4, FR-01..FR-24, NFR-01..NFR-06)
- design — `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, sections 1–3 (Visual Direction, Core Tokens, Motion Tokens)
- design — `_bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md`, sections 1–3 (Bottom Taskbar, Start Menu, Hard Exclusions)
- design — `_bmad-output/initiative-testbar/ux-testbar/ux-testbar.md`, UX Specification index
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, Invariants AD-1..AD-7 & Consistency Conventions

## Notes

- Decision: 2026-11-20 — User-approved UX (`DESIGN.md`, `EXPERIENCE.md`, 2026-11-20) supersedes earlier PRD v1 drafts (`FR-16`, `FR-19`, `FR-20`, `FR-21`, `FR-22`) by locking dark-only Obsidian Layered Glass theme, fixed `52px` taskbar height with fixed modules, 2 layout modes (Floating Dock and Edge-to-Edge Bar, dropping Split Islands), and explicitly excluding weather, VPN, Focus/DND, calculator, quick notes, monitor buttons, and brightness filler widgets.
- Decision: 2026-11-20 — The premature prototype committed at git baseline `772e23c` is retained as a brownfield workspace; its broken compilation, Win32/WebView2 lifecycle defects, fake tray module, and UX regressions are fixed systematically through the planned BMAD epics and stories.
- Source conflict: `prd FR-18` vs brownfield `src-tauri/src/system_widgets.rs` — prototype implemented tray overflow as a process list instead of real tray icons; resolved via `epic-tray-audio-calendar` architectural contract.
- Decision: 2026-11-20 — Windows 11 24H2/25H2 unpackaged `.exe` constraints settled during Architecture Reviewer Gate (`architecture-testbar.md` AD-7): Tray Overflow (`^`) uses zero-injection Two-Tier background app inspection (activating real hidden app windows) plus 1-click native Windows overflow (`Win+B`) escape hatch; Calendar Notifications displays local TestBar session alerts plus 1-click native Windows Notification Center toggle (`Win+N`) because WinRT `UserNotificationListener` requires MSIX Package Identity.
- High-risk check: Verify forced-kill watchdog restoration (`taskkill /F /PID`) restores `Shell_TrayWnd` and original HiDPI `SPI_GETWORKAREA` cleanly without requiring `explorer.exe` restart.
