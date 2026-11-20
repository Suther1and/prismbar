---
title: "TestBar — Experience & Behavior (EXPERIENCE.md)"
status: final
created: 2026-11-19
updated: 2026-11-20
---

# TestBar — Information Architecture & Behavior

## 1. Bottom Taskbar
- **Start Button:** Opens/closes Start Menu (same as tapping lone `Win` key without breaking `Win+<key>` OS shortcuts).
- **Pinned + Running Apps:** Native icons, single click launches/focuses/minimizes; hover shows live window preview cards with click-to-focus and `×` close per window; right-click opens full native-grade menu (app Jump List actions, New Window, Run as Admin, Pin/Unpin, Close Window(s), **Завершить задачу**).
- **Tray Overflow (`^`):** Compact 6-column icon-only grid sized for 30–40+ background apps; hover displays app name tooltip; left-click activates/opens; right-click offers Open / Settings / End Task.
- **Keyboard Layout (`RU` / `EN`):** Live active foreground window layout indicator; click toggles layout.
- **Sound & Network Pill:** Scroll wheel adjusts master volume with on-screen OSD; middle-click toggles mute; left-click opens Quick Sound & Mixer (synced with Start Menu audio controls).
- **Clock & Date:** Live local time/date; left-click opens interactive Calendar & Notifications flyout.
- **Right-click on empty Taskbar:** Task Manager, Dock / Edge-to-Edge toggle, TestBar Settings, Emergency «Вернуть панель Windows».
- **Fullscreen Auto-Hide:** Automatically hides taskbar when a fullscreen game or video is active; restores when leaving fullscreen.

## 2. Start Menu (4 Adaptive Spaces + Custom Spaces)
- **Header:**
  - Left: Peripherals battery pill (mouse + active wireless headphones charge; click opens compact popover & audio device switcher).
  - Center: Segmented Space selector (**Личное · Работа · Студия · Игры** + `+` custom space) with spring sliding indicator + hover ghost pill.
  - Right: Edit Mode toggle (drag-and-drop reorder + centered SVG `×` unpin), Universal Pinning / All Apps drawer, Settings, and overlay Power capsule (Lock / Sleep / 2-click armed Restart & Shutdown — overlays right tools without shifting center tabs).
- **Space 1 — Личное:** 2 Hero tiles + 9 Compact tiles + 4 Quick Access links (folders/files/URLs) | Now Playing SMTC card + Audio Device Switcher & Per-App Mixer + Live Clipboard History (pin, click-to-copy, clear unpinned).
- **Space 2 — Работа:** 8 Project/Folder hub cards (with hover Open / Terminal / Copy path actions) + 4 Work Apps | Human-readable PC Health (CPU %, RAM GB/64, RTX 4090 temp+%, VRAM GB/24 — strictly 1-line rows with plain status pills) + «Кто сейчас ест память» with 1-click process terminate.
- **Space 3 — Студия:** 2 Creative/AI Hero tiles + 9 Compact tiles + 4 Tools | Strictly 1-line Memory Headroom (VRAM + RAM) + Audio & Mic toggle (with live indicator) + «Активные процессы GPU / ОЗУ». (No «Режимы работы» widget.)
- **Space 4 — Игры:** Toggleable `Недавние (4)` / `Все игры` scrollable library with real Steam hero covers + 3D hover tilt + Launchers & Companions row | Gaming Mic + Game/Discord/Music mixer + 1-click Game Combos (e.g., PoE 2 + Exiled Exchange) + single-line RTX 4090 temp/VRAM status.
- **Universal Pinning Drawer:** Allows pinning ANY app, folder, file, or URL into the active space, plus categorized All Apps browser.
- **Type-Anywhere Command Search:** No permanent search box; typing any character immediately focuses the floating command palette with zero lost keystrokes and **RU↔EN wrong-keyboard-layout fuzzy matching** (`ntkt` → Telegram). `Enter` launches, `Ctrl+Enter` runs as admin, `Esc` closes.
- **Shift Second Layer:** Holding `Shift` highlights alternate actions on tiles (`От админа`, `Новое окно`, `Скопировать путь`, etc.).

## 3. Hard Exclusions (User-Rejected)
- No light theme, no accent color dots, no VPN toggle, no calculator, no quick notes widget, no monitor layout buttons, no Focus/DND filler toggles, no «Режимы работы», no cryptic raw GPU RPM/MHz numbers, no inline UI tutorial hints, no emoji.
