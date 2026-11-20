---
title: "TestBar — Design System (DESIGN.md)"
status: final
created: 2026-11-19
updated: 2026-11-20
---

# TestBar — Design System

## 1. Visual Direction
- **Strict Dark Mode Only ("Obsidian Layered Glass"):** Deep obsidian translucent panels with subtle specular top edge, cursor-tracking radial spotlight sheen, and space-adaptive ambient aura. No light theme, no accent color picker clutter, no branding logos.
- **Iconography:** Unified crisp 1.5px–1.9px vector SVG icons for all system controls; real native `.exe`/UWP/Steam art for apps and games. Zero emoji anywhere in UI.
- **Typography:** `Segoe UI Variable Display / Text` (fallback `Segoe UI`, sans-serif) for UI; `Cascadia Code` monospace with `tabular-nums` for telemetry, percentages, clock, and keycaps.

## 2. Core Tokens
- **Start Menu Chassis:** `1060×636px` centered floating glass surface (`border-radius: 26px`, `rgba(11, 13, 19, 0.90)`, `backdrop-filter: blur(52px) saturate(195%)`, `border: 1px solid rgba(255,255,255,0.10)`).
- **Space Accents:**
  - Личное: `#818CF8` (`129, 140, 248`)
  - Работа: `#38BDF8` (`56, 189, 248`)
  - Студия: `#34D399` (`52, 211, 153`)
  - Игры: `#FB7185` (`251, 113, 133`)
- **Bottom Taskbar:**
  - Height: `52px`, Floating Dock default (`border-radius: 17px`, centered with `12px` bottom margin) + Edge-to-Edge Bar toggle (`border-radius: 0`, full monitor width).
  - App button slot: `42×40px` (`border-radius: 11px`) strictly stationary inside the `52px` bar (`6px` top/bottom margin — zero border bleed on hover). Only inner `24×24px` icon lifts (`translateY(-1.5px) scale(1.07)`).
  - Running indicator: `6×2.5px` neutral pill; Active indicator: `16×2.5px` accent pill with soft glow; Multi-window secondary dot (`3×2.5px`).
  - Subtle cursor-tracking horizontal radial glow inside the taskbar capsule.
- **Flyouts (`10px–12px` gap above taskbar):**
  - Compact Tray Overflow (`^`): `244px` wide, 6-column grid of `34×34px` icon-only cells (`18×18px` icons), max-height `224px` scrollable (supports 30–40+ tray apps effortlessly), floating name tooltip above panel, zero tutorial hints.
  - Quick Sound & Mixer: `324px` wide, 3-device sliding output selector, master volume slider + mute, synced per-app mixer sliders.
  - Clock & Calendar: `310px` wide, interactive month calendar with pixel-centered SVG chevrons + notifications list.
  - Context Menu: `min-width: 236px`, Jump List actions, Open New Window, Run as Admin, Pin/Unpin, Close Window(s), and red «Завершить задачу».

## 3. Motion Tokens
- Spring open/close: `cubic-bezier(0.22, 1, 0.36, 1)` (`180–260ms`).
- Micro-elastic controls: `cubic-bezier(0.34, 1.45, 0.64, 1)` (`140–220ms`).
- Edit-mode delete badge (`×`): `20×20px` pill with mathematically centered `10×10px` inline SVG cross (`dx=0, dy=0`).
