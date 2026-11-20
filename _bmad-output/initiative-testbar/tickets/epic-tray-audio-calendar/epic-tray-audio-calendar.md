---
type: epic
title: "Compact Tray Overflow, WASAPI Audio/Mixer, OSD, Layout & Calendar"
parent: initiative-testbar
status: in-progress
covers: [R4]
sources: [_bmad-output/initiative-testbar/initiative-testbar.md]
companions: [_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md, _bmad-output/initiative-testbar/ux-testbar/DESIGN.md, _bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md]
after: []
refined: true
risk: medium
---

# Compact Tray Overflow, WASAPI Audio/Mixer, OSD, Layout & Calendar

## Description

Complete the right-side system cluster of the bottom taskbar and its flyouts in full compliance with `DESIGN.md`, `EXPERIENCE.md`, and Windows 11 24H2 constraints: replace the fake process-list tray stub with a real Two-Tier 6-column Tray Overflow (`^`), decouple volume OSD into a non-jittering `FlyoutKind::Osd`, harden WASAPI master/per-app mixer and 3-role output switching, verify live RU/EN layout switching, and finish the interactive Calendar & Notifications flyout.

## Outcome

Users manage background tray apps, audio volume/devices/per-app mixer, keyboard layout, and calendar directly from the taskbar right cluster with zero width jitter, zero jumping tooltips, and zero fake process-spawning clicks.

## Requirements

- **R1 (parent R4):** Implement the compact `244px` 6-column icon-only Tray Overflow (`^`, `34×34px` cells, scrollable `224px` max-height for 30–40+ apps) using Windows 11 24H2-compatible Two-Tier inspection: enumerate active background user apps owning hidden/tray-capable windows with cached real icons, render a non-shifting floating hover tooltip above the panel, activate/restore the app's hidden window on left-click, provide Open / Location / End Task on right-click (overlay menu, not replacing the grid), and offer a 1-click native Windows overflow (`Win+B`) trigger.
- **R2 (parent R4):** Move scroll-wheel volume feedback out of the taskbar button into non-activating `FlyoutKind::Osd` (zero taskbar width jitter), support middle-click mute, split `audio-master-updated` and `audio-mixer-updated` events, filter expired/inactive WASAPI sessions and group multi-process apps cleanly, switch all 3 `IPolicyConfig` roles (`eConsole`, `eMultimedia`, `eCommunications`) with real `HRESULT` error propagation, and render the `324px` Quick Sound & Mixer flyout with a sliding 3-device indicator.
- **R3 (parent R4):** Display the active foreground window's keyboard layout (`RU`/`EN`) in real time with click-to-toggle, and deliver the `310px` interactive Clock & Calendar flyout with pixel-centered SVG chevrons, month navigation, real date selection + quick return to «Сегодня», local TestBar session alerts, and a 1-click button triggering Windows 11 Notification Center (`Win+N`).

## Constraints

- **C1 (parent C1):** Zero DLL injection or `ReadProcessMemory` scraping of `explorer.exe`.
- **C2 (parent C2, C3):** Strictly dark Obsidian Layered Glass UI with zero emoji, zero tutorial hints, and zero MSIX-only API dependencies (`UserNotificationListener` is out of scope for unpackaged `.exe`).

## Done when

1. Tray Overflow (`^`) renders a 6-column icon-only grid whose tooltip never shifts the grid vertically, activates hidden app windows instead of blindly launching duplicates, and includes a working right-click menu + native `Win+B` trigger.
2. Scrolling over the Sound pill changes master volume smoothly via floating `FlyoutKind::Osd` without shifting taskbar width; Quick Sound & Mixer switches all 3 audio roles, filters dead sessions, and propagates errors honestly.
3. Keyboard layout indicator updates from the foreground window and switches on click; Calendar supports month browsing, date selection, «Сегодня», local alerts, and `Win+N` Notification Center toggle.
4. `pnpm build`, `pnpm test`, and `cargo test --manifest-path src-tauri/Cargo.toml` pass cleanly.

## Boundaries

- **Owns:** `audio.rs`, `system_widgets.rs` (tray inspector, keyboard layout, `Win+B`/`Win+N` helpers), `TaskbarView.tsx` right cluster (tray trigger, layout, sound/network, clock), and `FlyoutView.tsx` (`tray`, `sound`, `osd`, `calendar` modes).
- **Belongs to siblings:** WindowCoordinator/FlyoutKind infrastructure (`epic-shell-foundation`), IconCache & taskbar app list (`epic-taskbar-windows`), Start Menu audio/peripherals sync (`epic-hardware-media-games`).

## References

- parent — `_bmad-output/initiative-testbar/initiative-testbar.md`, section Requirements (R4)
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, sections AD-1, AD-5, AD-6, AD-7
- design — `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, section 2 (Flyouts — Tray, Sound & Mixer, Clock & Calendar)
- design — `_bmad-output/initiative-testbar/ux-testbar/EXPERIENCE.md`, section 1 (Tray, Layout, Sound, Clock)

## Notes

- Decision: 2026-11-20 — Tracer bullet is entry `1` ("Two-tier compact 6-column Tray Overflow (`^`)"): replaces the single biggest fake module from the premature prototype first.
- Decision: 2026-11-20 — Stories `3.1 -> 3.2 -> 3.3 -> 3.4` run sequentially after `2.5` because they share `FlyoutView.tsx`, `TaskbarView.tsx`, `system_widgets.rs`, and `lib.rs`.
- Decision: 2026-11-20 — Tray right-click menu offers `Open / Location / End Task` (adapting `EXPERIENCE.md` §1 `"Open / Settings / End Task"` because arbitrary third-party background apps have no universal Win32 "Settings" verb without DLL injection; proprietary app settings remain reachable via the 1-click native Windows `Win+B` overflow button).
- Decision: 2026-11-20 — Includes closing `Refactor sweep` (entry `4`) per BMAD slicing policy.
