---
type: epic
title: "Bottom Taskbar, Real Window Tracking, UWP Icons, Previews & Context Menus"
parent: initiative-testbar
covers: [R2]
sources: [_bmad-output/initiative-testbar/initiative-testbar.md]
companions: [_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md, _bmad-output/initiative-testbar/ux-testbar/DESIGN.md, _bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md]
after: []
refined: true
risk: high
---

# Bottom Taskbar, Real Window Tracking, UWP Icons, Previews & Context Menus

## Description

Deliver the core Windows 11 bottom taskbar experience: unified cached icon extraction (fixing broken `ApplicationFrameHost.exe` UWP icons), diff-only window broadcasts to eliminate background IPC bloat, accurate pinned+running deduplication, strictly contained capsule hover (`42×40px` inside `52px`), Dock / Edge-to-Edge switching, gapless non-activating live window hover previews, and full right-click menus (Jump Lists, New Window, Run as Admin, raw-arg file location, Pin/Unpin, Close, End Task, and empty-bar menu).

## Outcome

Every pinned and running Win32 or UWP application appears once on the bottom taskbar with its real crisp icon, clear state indicators, zero hover bleed or flickering previews, and complete Windows-grade window control.

## Requirements

- **R1 (parent R2):** Provide one Rust `IconCache` service (256-entry LRU, namespaced keys `exe:`, `aumid:`, `hwnd:`, `file:`) with batch `get_icons(keys)` IPC and diff-only `windows-updated` broadcasts targeting visible windows so hidden WebViews never receive periodic base64 icon dumps.
- **R2 (parent R2):** Resolve authentic icons for both Win32 apps and packaged UWP apps (`ApplicationFrameHost.exe` via child `CoreWindow` -> `GetApplicationUserModelId` -> `IShellItemImageFactory` behind an RAII COM guard per AD-5) and merge pinned taskbar items with running windows cleanly via canonical `match_exe`/`match_aumid` without duplicates.
- **R3 (parent R2):** Render the `52px` bottom taskbar in Floating Dock (`17px` radius, `12px` bottom margin) and Edge-to-Edge Bar (`0px` radius, full width) modes with stationary `42×40px` hover slots strictly inside the capsule, inner `24×24px` icon lift, subtle cursor-tracking radial glow, running/active/multi-window state pills, and gapless non-activating live window hover previews (`PrintWindow` focus and `×` close).
- **R4 (parent R2):** Deliver full right-click context menus on taskbar icons (real app Jump List actions, Open New Window, Run as Admin, Open File Location via raw unescaped `explorer.exe /select`, Pin/Unpin, Close Window(s), and red «Завершить задачу» killing the target PID cleanly) plus right-click on empty taskbar (Task Manager, Dock/Edge toggle, TestBar Settings, Emergency «Вернуть панель Windows»).

## Constraints

- **C1 (parent C1):** Zero DLL injections or `explorer.exe` memory patching.
- **C2 (parent C2):** Strictly dark Obsidian Layered Glass styling (`DESIGN.md` §2), zero hover bleed outside the `52px` capsule, zero emoji, zero tutorial hints.

## Done when

1. Running UWP apps (Settings, Store, Terminal, Calculator) and Win32 apps display their authentic icons and merge cleanly with pinned entries without duplicates.
2. `windows-updated` transmits lightweight `icon_key` references only on real list/focus diffs, and `IconCache` LRU/key tests pass in `cargo test`.
3. Hovering a running app shows a gapless non-activating preview card that does not steal focus or clobber open click flyouts, allowing 1-click focus and `×` close.
4. Right-clicking an app icon or empty taskbar opens the full `DESIGN.md` context menu with working Jump List / New Window / Admin / File Location / Pin / Close / End Task / Settings / Restore actions.
5. `pnpm build`, `pnpm test`, and `cargo test --manifest-path src-tauri/Cargo.toml` pass cleanly.

## Boundaries

- **Owns:** `icon_cache.rs`, `windows_tracker.rs`, `TaskbarView.tsx` (left/center app slots, Dock/Edge styling, hover triggers), and `FlyoutView.tsx` (`preview`, `context_app`, `context_bar` modes).
- **Belongs to siblings:** AppBar/watchdog/coordinator base (`epic-shell-foundation`), right-cluster Tray/Audio/OSD/Layout/Calendar (`epic-tray-audio-calendar`), Start Menu (`epic-start-spaces-search`), and hardware/media/Steam (`epic-hardware-media-games`).

## References

- parent — `_bmad-output/initiative-testbar/initiative-testbar.md`, section Requirements (R2)
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, sections AD-1, AD-4, AD-6, AD-7
- design — `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, section 2 (Bottom Taskbar, Flyouts — Context Menu)
- design — `_bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md`, section 1 (Bottom Taskbar)

## Notes

- Decision: 2026-11-20 — Tracer bullet is entry `1` ("Unified IconCache service & diff-only window-list broadcasts"): it establishes the cross-epic icon transport (`icon_key` + `get_icons`) that Epics 2, 3, and 4 all depend on.
- Decision: 2026-11-20 — Stories `2.1 -> 2.2 -> 2.3 -> 2.4 -> 2.5` run sequentially after `1.5` to prevent collisions in `windows_tracker.rs`, `TaskbarView.tsx`, `FlyoutView.tsx`, and `lib.rs`.
- Decision: 2026-11-20 — Includes closing `Refactor sweep` (entry `5`) per BMAD slicing policy.
- High-risk check: Verify UWP windows (Settings, Store, Terminal) render distinct real app icons (not `ApplicationFrameHost.exe`) and RAII COM guard never calls `CoUninitialize` on `RPC_E_CHANGED_MODE`.
