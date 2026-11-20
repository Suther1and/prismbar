# AGENTS.md — TestBar Repository Guide

<!-- bmad-project-context: managed block (refreshed 2026-11-20) -->

## Stack & Architecture
- **Stack:** Tauri v2 (`tauri` 2.x, Rust `windows` 0.58, `sysinfo` 0.33, dynamic `nvml.dll`) + React 19 + TypeScript + Vite 6 + Vitest.
- **Architecture authority:** `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md` (`AD-1`..`AD-7`).
- **UX authority:** `_bmad-output/initiative-testbar/ux-testbar/DESIGN.md`, `EXPERIENCE.md`, and `mockups/complete-ux.html`. Strictly dark mode only ("Obsidian Layered Glass"), zero emoji, zero inline tutorial hints, taskbar hover strictly inside `52px` capsule bounds, compact 6-column icon-only tray overflow (`^`).
- **BMAD ticket tree:** `_bmad-output/initiative-testbar/tickets/`. Check next ready work with `uv run _bmad/method/scripts/tickets.py next`.

## Verified Commands
- `pnpm build` — TypeScript check (`tsc`) + Vite production bundle.
- `pnpm test` — Vitest non-interactive frontend unit tests (`vitest run`, wired in Story 1.1).
- `cargo test --manifest-path src-tauri/Cargo.toml` — Rust unit tests.
- `pnpm tauri build` — Builds standalone local `src-tauri/target/release/testbar.exe`.

## Critical Pitfalls & Invariants (Windows 11 Build 26200 + WebView2)
- **Never inject DLLs or call `ReadProcessMemory` on `explorer.exe`.** Always preserve watchdog restoration and `Ctrl+Alt+Shift+Esc` native taskbar recovery (`AD-2`).
- **All 3 pre-warmed WebView2 windows (`taskbar`, `start`, `flyout`) MUST keep `"resizable": false` in `tauri.conf.json`** — `resizable: true` adds `WS_THICKFRAME` border artifacts on transparent Win11 windows (`AD-1`).
- **Never hide `start` or `flyout` by shrinking to `1×1` at `(0,0)`.** Use visibility (`SW_SHOW` / `SW_SHOWNOACTIVATE` / `SW_HIDE`) through `WindowCoordinator`; never flip `GWL_EXSTYLE` + `SWP_FRAMECHANGED` on every flyout open (`AD-1`).
- **Never do Tauri IPC, `SetWindowPos`, or blocking I/O inside `WH_KEYBOARD_LL` / `WH_MOUSE_LL` callbacks** — Windows silently unhooks slow low-level hooks (`AD-3`).
- **Never use `HWND_MESSAGE` for `TaskbarCreated`** — message-only windows never receive broadcasts; use a hidden top-level `WS_OVERLAPPED` window (`AD-2`).
- **Never hardcode `mon.bottom - 48` on work-area restore** — capture and restore DPI-aware `SPI_GETWORKAREA` (`AD-2`).
- **Never use WinRT `UserNotificationListener` in an unpackaged `.exe`** — requires MSIX Package Identity and deterministically fails (`AD-7`).
- **Never allow full-document `save_config` overwrites from frontend views** — use command-scoped atomic Rust mutations (`AD-4`).
- **Always use an RAII COM guard checking `CoInitializeEx` result** — never call `CoUninitialize` when `RPC_E_CHANGED_MODE` was returned (`AD-5`).
- **Always factor in Windows `HKCU\Software\Microsoft\Accessibility\TextScaleFactor` (`effective_scale = (dpi / 96.0) * (text_scale_factor / 100.0)`) when computing WebView2 window pixel dimensions** — WebView2 `RasterizationScale` scales by `dpi * TextScaleFactor` even when `LogPixels = 96`, causing severe clipping if Rust sizes windows at `1.0×`.
- **Never inject `0xE8` and return `CallNextHookEx` on physical lone-Win `WM_KEYUP`** — `SendInput` queues after the current `KEYUP`; always inject `[0xE8 down, 0xE8 up, Win up]` and return `LRESULT(1)` to swallow the physical `KEYUP` so native Windows Start never pops over TestBar.
- **Always style custom dark scrollbars (`color-scheme: dark` + `::-webkit-scrollbar`) globally** — never let white Chromium scrollbars appear.

<!-- /bmad-project-context -->
