---
type: epic
title: "Live Media, Clipboard, Telemetry, Peripherals, Steam Games & Standalone .exe Delivery"
parent: initiative-testbar
covers: [R7, R8]
sources: [_bmad-output/initiative-testbar/initiative-testbar.md]
companions: [_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md, _bmad-output/initiative-testbar/ux-testbar/DESIGN.md, _bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md]
after: []
refined: true
risk: medium
---

# Live Media, Clipboard, Telemetry, Peripherals, Steam Games & Standalone .exe Delivery

## Description

Complete all live hardware, media, clipboard, peripheral, and Steam integrations inside Start Menu Spaces and deliver the verified standalone Windows 11 `testbar.exe`: non-blocking MTA WinRT SMTC media controls, continuous live clipboard vault with persistent pins, Bluetooth peripheral battery popover synced with audio outputs, human-readable CPU/RAM/RTX 4090 NVML telemetry with multi-PID process-group termination and real Studio GPU/RAM active processes, dynamic Registry Steam library scanner with real hero covers, 3D hover tilt, and 1-click game combos, and final release `.exe` packaging + UX compliance sign-off.

## Outcome

Every widget in **Личное**, **Работа**, **Студия**, and **Игры** displays live real Windows 11 hardware/media/Steam data without blocking the UI, and `pnpm tauri build` produces a verified standalone `testbar.exe` passing all automated checks and UX exclusions.

## Requirements

- **R1 (parent R7):** Run WinRT SMTC media observation and transport controls (Play/Pause, Next, Prev) on a dedicated MTA worker thread with zero `thread::sleep` in IPC; maintain continuous `AddClipboardFormatListener` history with persistent pinned items in `config.pinned_clipboard`, click-to-copy, and clear-unpinned; sync Start Menu audio/mic controls and Bluetooth peripheral battery popover with live WASAPI state.
- **R2 (parent R7):** Provide human-readable PC Health telemetry (CPU %, RAM GB/64, dynamic `nvml.dll` RTX 4090 temp+%, VRAM GB/24) in strictly single-line rows (`WORK` and `STUDIO`), real «Кто сейчас ест память» and Studio «Активные процессы GPU / ОЗУ» lists, and multi-PID process-group termination that kills all PIDs of a multi-process app and surfaces OS permission errors cleanly to the user.
- **R3 (parent R7):** Discover Steam libraries dynamically via `HKCU\Software\Valve\Steam\SteamPath` (with fallback paths), parse `libraryfolders.vdf` and `appmanifest_*.acf`, load real local hero covers with 3D hover tilt, support toggleable `Недавние (4)` / `Все игры` scrollable library, and execute 1-click Game Combos (`kind: "combo"`).
- **R4 (parent R8):** Verify final standalone Windows 11 release packaging (`pnpm tauri build` -> `src-tauri/target/release/testbar.exe`), confirm strict adherence to `DESIGN.md` and `EXPERIENCE.md` §3 Hard Exclusions, and pass full `pnpm build`, `pnpm test`, and `cargo test` suites.

## Constraints

- **C1 (parent C2):** Strictly dark Obsidian Layered Glass only; strictly single-line memory/GPU rows in Work and Studio; zero «Режимы работы», zero raw GPU RPM/MHz numbers, zero emoji, zero tutorial hints.
- **C2 (parent C3):** Dynamic `nvml.dll` loading only (zero CUDA SDK link dependency) and standalone portable `.exe` build using OS WebView2 Runtime.

## Done when

1. SMTC media controls, clipboard history (capturing while Start is hidden and persisting pins), and Bluetooth peripheral battery + audio sync operate without blocking IPC threads.
2. Telemetry shows real CPU/RAM/NVML GPU metrics in single-line rows, Studio shows active GPU/RAM processes with working termination, and terminating a multi-process app kills all its PIDs or reports exact OS error feedback.
3. Steam games are discovered dynamically from Registry without hardcoded drive letters, render real hero covers with 3D hover tilt, toggle Recent/All, and launch 1-click combos.
4. `pnpm build`, `pnpm test`, `cargo test --manifest-path src-tauri/Cargo.toml`, and `pnpm tauri build` succeed cleanly, producing a verified standalone `src-tauri/target/release/testbar.exe`.

## Boundaries

- **Owns:** `media.rs`, `telemetry.rs`, `system_widgets.rs` (clipboard listener, BT battery, network, power), `apps_indexer.rs` (Steam registry + covers), Start Menu right panels & Games space widgets, and release build verification.
- **Belongs to siblings:** Shell foundation & config authority (`epic-shell-foundation`), taskbar & IconCache (`epic-taskbar-windows`), WASAPI audio backend (`epic-tray-audio-calendar`), Start Spaces chassis & search (`epic-start-spaces-search`).

## References

- parent — `_bmad-output/initiative-testbar/initiative-testbar.md`, section Requirements (R7, R8)
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, sections AD-4, AD-5, AD-6, AD-7
- design — `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, sections 1–3
- design — `_bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md`, sections 2–3

## Notes

- Decision: 2026-11-20 — Tracer bullet is entry `1` ("Dedicated MTA WinRT SMTC worker, continuous clipboard vault & synced peripherals/audio"): proves non-blocking COM/WinRT background workers and hidden-window capture + hydration first.
- Decision: 2026-11-20 — Stories `5.1 -> 5.2 -> 5.3 -> 5.4 -> 5.5` run sequentially after `3.4` and `4.4` to prevent collisions across Start panels, `system_widgets.rs`, and `lib.rs`.
- Decision: 2026-11-20 — Epic 5 completes the `R8` standalone release `.exe` packaging and full UX/NFR verification (building on Epic 1's build/test/autostart/CSP foundation).
- Decision: 2026-11-20 — Entry `4` is the cleanup-only `Refactor sweep`; entry `5` is the dedicated closing release packaging & UX/NFR sign-off story waiting on all prior epic sweeps (`1.5`, `2.5`, `3.4`, `4.4`, `5.4`).
