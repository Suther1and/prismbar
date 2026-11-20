# AGENTS.md — TestBar Repository Guide

- **Stack:** Tauri v2 (Rust `windows` 0.58 + `sysinfo` + dynamic `nvml.dll`) + React 19 + TypeScript + Vite.
- **Architecture:** 3 pre-warmed transparent frameless WebView2 windows (`taskbar`, `start`, `flyout`) from one Vite bundle (`?view=...`). Never inject DLLs into `explorer.exe`; always preserve watchdog & `Ctrl+Alt+Shift+Esc` native taskbar restoration.
- **UX Authority:** `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, `EXPERIENCE.md`, and `mockups/complete-ux.html`. Strictly dark mode only, zero emoji, zero inline tutorial hints, taskbar hover strictly inside capsule bounds, compact 6-column icon-only tray overflow (`^`).
- **Build & Verify:**
  - `pnpm build` — TypeScript check + Vite production bundle
  - `cargo test --manifest-path src-tauri/Cargo.toml` — Rust unit tests
  - `pnpm tauri build` — Builds standalone `src-tauri/target/release/testbar.exe`
