---
type: epic
title: "Start Menu 4 Adaptive Spaces, Universal Pinning, Edit Mode & Type-Anywhere Search"
parent: initiative-testbar
status: in-progress
covers: [R5, R6]
sources: [_bmad-output/initiative-testbar/initiative-testbar.md]
companions: [_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md, _bmad-output/initiative-testbar/ux-testbar/DESIGN.md, _bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md]
after: []
refined: true
risk: medium
---

# Start Menu 4 Adaptive Spaces, Universal Pinning, Edit Mode & Type-Anywhere Search

## Description

Deliver the full Start Menu experience in exact fidelity to `DESIGN.md` and `EXPERIENCE.md`: instant hydration on show (via Start button or lone `Win` key without breaking `Win+<key>`), all 4 adaptive Spaces (**Личное · Работа · Студия · Игры**) plus custom spaces with sliding spring tab indicator, hover ghost pill, cursor spotlight, and zero-clipping dynamic grids; complete Edit Mode (drag-and-drop reorder across all lists + mathematically centered SVG `×`); modal-safe Universal Pinning for apps/folders/files/URLs and full All Apps (including UWP `shell:AppsFolder`); overlay Power capsule (2-click armed Restart/Shutdown); functional `Shift` alternate layer; and zero-loss type-anywhere RU↔EN fuzzy command search across apps, UWP, games, all Spaces, folders, and user files.

## Outcome

Pressing `Win` or clicking Start opens the `1060×636px` Obsidian Layered Glass Start Menu in <50 ms where every pinned item is visible, editable, and re-orderable, universal pinning never closes Start while picking files, and typing immediately finds any app, game, folder, URL, or user file even in the wrong keyboard layout.

## Requirements

- **R1 (parent R5):** Render the `1060×636px` Obsidian Layered Glass Start Menu hydrated cleanly on open, featuring the 4 adaptive Spaces (**Личное · Работа · Студия · Игры**) + `+` custom spaces, spring sliding Space selector + hover ghost pill, cursor-tracking radial spotlight on cards, and unlimited scrollable/wrapping tile and link grids (removing `.slice()` clipping of user-pinned items) with strictly inline SVG icons and zero tutorial hints.
- **R2 (parent R5):** Provide full Edit Mode (drag-and-drop reordering across Personal, Work hubs & apps, Personal links, Studio, Games, and Custom Spaces; pixel-centered `20×20px` pill with `10×10px` inline SVG `×`), modal-safe Universal Pinning drawer (`+ Программу`, `+ Папку`, `+ Файл`, `+ Ссылку / Сайт` with `rfd` focus guard + categorized All Apps including Start Menu `.lnk` and UWP `shell:AppsFolder` with no 180-item truncation), overlay Power capsule (Lock, Sleep, 2-click armed Restart & Shutdown without shifting header tabs), and functional `Shift` alternate action layer matching every shown badge.
- **R3 (parent R6):** Provide zero-loss type-anywhere floating Command Search inside Start Menu with RU↔EN wrong-keyboard-layout fuzzy matching across installed `.lnk` apps, UWP apps, Steam games, all Spaces/links/launchers/custom spaces, folders, and user files (`Desktop`, `Documents`, `Downloads`), full keyboard navigation, `Enter` launch, `Ctrl+Enter` run-as-admin, and `Esc` close.

## Constraints

- **C1 (parent C2):** Strictly dark Obsidian Layered Glass only; zero emoji, zero Unicode arrow/play glyph hacks where SVG belongs, zero tutorial hint texts, zero rejected widgets («Режимы работы», VPN, calculator, quick notes, monitor buttons, light theme).
- **C2 (parent C1):** Never modify `explorer.exe` or break `Win+<key>` system shortcuts.

## Done when

1. Pinning 12+ tiles or 5+ links in any Space displays all of them without silent `.slice()` clipping and persists across restarts via command-scoped config mutations.
2. Opening `+ Папку` or `+ Файл` keeps Start Menu open while the native dialog is active and immediately pins the selected item.
3. Holding `Shift` shows alternate action badges and clicking executes the exact displayed alternate action (Admin, New Window, Copy Path, Open Terminal).
4. Typing immediately on Start open (including wrong RU/EN layout like `ntkt` -> Telegram) finds installed apps, UWP apps, games, pinned items across all Spaces, and user files with zero dropped leading keystrokes.
5. `pnpm build`, `pnpm test`, and `cargo test --manifest-path src-tauri/Cargo.toml` pass cleanly.

## Boundaries

- **Owns:** `StartView.tsx`, `apps_indexer.rs` (Start `.lnk` + UWP `shell:AppsFolder` + user file index + `rfd` modal-guarded pickers), `layoutMap.ts`, and `styles/tokens.css` Start visual/motion polish.
- **Belongs to siblings:** Lone `Win` hook & modal focus guard (`epic-shell-foundation`), unified `IconCache` (`epic-taskbar-windows`), live SMTC/clipboard/NVML/BT/Steam right-side integrations (`epic-hardware-media-games`).

## References

- parent — `_bmad-output/initiative-testbar/initiative-testbar.md`, section Requirements (R5, R6)
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, sections AD-1, AD-3, AD-4, AD-6, AD-7
- design — `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, sections 1–3
- design — `_bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md`, sections 2–3

## Notes

- Decision: 2026-11-20 — Tracer bullet is entry `1` ("Start Menu shell, show-time hydration, 4 Spaces + custom & zero-clipping dynamic grids"): fixes show-time hydration, spotlight/sliding motion, and removes silent `.slice()` pin clipping across all Spaces first.
- Decision: 2026-11-20 — Stories `4.1 -> 4.2 -> 4.3 -> 4.4` run sequentially after `3.4` to avoid `StartView.tsx`, `tokens.css`, and `lib.rs` collisions and guarantee IconCache/UWP icons and lone Win hook are ready.
- Decision: 2026-11-20 — Epic 4 owns the Start Menu UI/Spaces/Edit/Pinning/Settings/Power/Shift portion of `R5` (with OS hook/modal-guard/config authority delivered by Epic 1).
- Decision: 2026-11-20 — Includes closing `Refactor sweep` (entry `4`) per BMAD slicing policy.
