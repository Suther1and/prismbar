---
title: 'Refactor sweep — Epic 1 shell foundation cleanup'
type: 'refactor'
ticket: '5'
created: '2026-11-21'
status: done
baseline_revision: 'e0d39f84d25941fd6e2954683f471b30cf3bfede'
route: 'full'
route_source: 'auto'
risk: 'low'
review: 'quick'
review_source: 'pinned'
lenses_ran: ['quick']
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: []
deferred: []
---

<intent-contract>

## Intent

**Problem:** Stories 1.1–1.4 left dead legacy helpers (including `#[allow(dead_code)]` Win32 stubs, unused `point_inside_hwnd`, and unused `config::save_config`) plus repeated local boilerplate across the Epic 1 shell foundation (`coordinator.rs`, `win_shell.rs`, `config.rs`, `lib.rs`).

**Approach:** Remove all dead/unused legacy helpers and consolidate repeated internal Win32, mutex-locking, coordinator-dispatch, and config-persistence boilerplate across `win_shell.rs`, `config.rs`, `coordinator.rs`, and `lib.rs` (while verifying `main.rs` and `src/lib/ipc.ts` stay clean) with zero behavior change, zero compiler warnings, and 100% green Rust and Vitest suites.

## Boundaries & Constraints

**Always:**
- Strictly preserve all Epic 1 runtime behaviors, public IPC command signatures, event names, and invariants (`AD-1`..`AD-4`, `AD-6`, `AD-7`): non-resizable transparent windows, visibility-only show/hide, single-instance mutex, top-level hidden AppBar/`TaskbarCreated`/HiDPI watchdog recovery, non-blocking bounded hook channel, single-authority fullscreen classifier, and transactional config mutations with zero `save_config` IPC exposure.
- Keep `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`, `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` passing with zero errors and zero Rust/TypeScript warnings.
- Only modify Epic 1 shell foundation files (`src-tauri/src/win_shell.rs`, `src-tauri/src/config.rs`, `src-tauri/src/coordinator.rs`, `src-tauri/src/lib.rs`, and `src-tauri/src/main.rs` / `src/lib/ipc.ts` only if a real warning or dead helper surfaces).

**Never:**
- Never pull in Epic 2–5 scope (do not touch `FlyoutView.tsx`, `StartView.tsx`, `TaskbarView.tsx`, `system_widgets.rs`, `windows_tracker.rs`, `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, or `tauri.conf.json`).
- Never leave `#[allow(dead_code)]` attributes in any of the 6 Epic 1 files.
- Never alter observable behavior, error handling, or public APIs tested by existing unit/integration tests unless removing confirmed-unused dead legacy functions.

</intent-contract>

## Code Map

- `src-tauri/src/win_shell.rs` -- Remove unused `#[allow(dead_code)] pub fn create_hidden_message_window` (lines 1963–1983), `#[allow(dead_code)] pub fn post_close_to_hwnd` (lines 1985–1990), and unused legacy `pub fn point_inside_hwnd` (lines 612–624). Extract private `set_native_taskbars_visibility(cmd: SHOW_WINDOW_CMD)` shared by `hide_native_taskbar` (lines 730–758) and `show_native_taskbars_and_restore_workarea` (lines 1270–1308). Add `MonitorRect::to_pixel_rect(self) -> PixelRect` to deduplicate 4 inline `PixelRect::new(mon.left, ...)` conversions (lines 348, 483, 777, 1412). Import `AttachThreadInput`, `GetCursorPos`, `SHOW_WINDOW_CMD` cleanly and extract `pub fn is_cursor_near_window(hwnd_raw: isize, pad_x: i32, pad_top: i32, pad_bottom: i32) -> bool` for `lib.rs::close_preview_if_outside`.
- `src-tauri/src/config.rs` -- Remove unused legacy `pub fn save_config` (lines 665–667). Narrow `ensure_parent_dir` (line 974) from `pub fn` to private `fn`. Extract `pub fn should_force_combo(space: &str, kind: &str) -> bool` shared by `add_pin_to_space` (lines 688–689) and `lib.rs::add_pin`.
- `src-tauri/src/coordinator.rs` -- Extract private `WindowCoordinator::close_start(&mut self, actions: &mut CoordinatorActions)` and `WindowCoordinator::close_flyout(&mut self, actions: &mut CoordinatorActions) -> Option<FlyoutKind>` to deduplicate repeated Start/flyout closing transitions across lines 281–448 without changing any transition outcome.
- `src-tauri/src/lib.rs` -- Add private `lock_unpoisoned<T>` (replacing ~18 repeated `.lock().unwrap_or_else(|e| e.into_inner())` chains and making `installed_apps`/`steam_games` poison-safe), `dispatch_coordinator` (deduplicating 8 lock-sync-apply blocks), unified `reevaluate_fullscreen(app, hint_entering: Option<bool>)`, and `normalize_bump_and_save` (shared by `mutate_config_at` and `apply_autostart_transaction`). Call `config::should_force_combo` in `add_pin` and `win_shell::is_cursor_near_window` in `close_preview_if_outside` (removing the lone `unsafe` Win32 block from `lib.rs`). Remove redundant `win_shell::stop_appbar_message_window()` calls immediately before `win_shell::restore_native_taskbar()` (which already stops the AppBar message window first).
- `src-tauri/src/main.rs` -- Read-only verified clean (23 lines, zero dead code or warnings).
- `src/lib/ipc.ts` -- Read-only verified clean (all 26 exports actively used across views/tests, zero warnings).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/src/win_shell.rs` -- Remove `create_hidden_message_window`, `post_close_to_hwnd`, and `point_inside_hwnd`; extract `set_native_taskbars_visibility`, `MonitorRect::to_pixel_rect`, and `is_cursor_near_window`; tidy `AttachThreadInput` imports in `force_focus_window` -- eliminates dead `#[allow(dead_code)]` stubs and Win32 boilerplate while consolidating all raw Win32 calls inside `win_shell.rs`.
- [x] `src-tauri/src/config.rs` -- Remove unused `save_config`, narrow `ensure_parent_dir` to private, and extract `should_force_combo` -- removes dead full-document save wrapper and shares combo-forcing predicate with `lib.rs`.
- [x] `src-tauri/src/coordinator.rs` -- Extract private `close_start` and `close_flyout` helpers on `WindowCoordinator` and reuse them across all Start/flyout dismissal paths -- eliminates repeated state-clearing boilerplate and guarantees `last_start_opened_ms` is always cleared on Start close.
- [x] `src-tauri/src/lib.rs` -- Add private `lock_unpoisoned`, `dispatch_coordinator`, unified `reevaluate_fullscreen(app, Option<bool>)`, and `normalize_bump_and_save`; wire `config::should_force_combo` and `win_shell::is_cursor_near_window`; drop redundant `stop_appbar_message_window()` calls before `restore_native_taskbar()` -- deduplicates shell orchestration boilerplate and removes the only raw `unsafe` Win32 block in `lib.rs`.

**Acceptance Criteria:**
- Given the refactored Epic 1 shell foundation files, when `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`, `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` are executed, then all four commands exit `0` with zero errors, zero Rust/TypeScript compiler warnings, and 100% of existing Rust (57/57) and Vitest (22/22) tests passing.
- Given the completed cleanup diff against `baseline_revision`, when inspecting `git diff --name-only` and scanning the 6 Epic 1 files for `#[allow(dead_code)]`, `create_hidden_message_window`, `post_close_to_hwnd`, `point_inside_hwnd`, and `config::save_config`, then zero out-of-scope files were modified and zero of those dead helpers or `#[allow(dead_code)]` attributes remain.

## Implementation Notes

- Completed all 4 refactor tasks across `src-tauri/src/win_shell.rs`, `src-tauri/src/config.rs`, `src-tauri/src/coordinator.rs`, and `src-tauri/src/lib.rs` (net `-161` lines: 193 insertions, 354 deletions).
- Verified `src-tauri/src/main.rs` and `src/lib/ipc.ts` read-only: clean, zero dead code or warnings.
- Removed unused `state: State<AppState>` parameters from `toggle_start`, `open_flyout`, and `close_flyout` (Tauri injects `State` on the Rust side, so frontend IPC signatures are unchanged) after routing them through `dispatch_coordinator(&app, ...)`.
- `apply_autostart_transaction` now routes through `normalize_bump_and_save`, which also rolls back the registry write if revision overflow occurs in addition to disk save failure.
- Verified with `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` (0 warnings), `cargo test --manifest-path src-tauri/Cargo.toml` (57/57 passed), `pnpm test` (22/22 passed), and `pnpm build` (clean `tsc && vite build`).

## Plan Change Log

## Review Triage Log

### 2026-11-21 — Review pass
- verdicts: 0 findings — high 0, medium 0, low 0, false 0, maybe-false 0
- findings:
  - None (Quick review found no unmet acceptance criteria, broken rules, or bugs).

## Auto Run Result

- **Summary:** Completed a strictly cleanup-only refactor sweep across the Epic 1 shell foundation (`win_shell.rs`, `config.rs`, `coordinator.rs`, `lib.rs`, with `main.rs` and `src/lib/ipc.ts` verified clean), removing all dead/unused legacy helpers and `#[allow(dead_code)]` attributes and consolidating repeated Win32, mutex-locking, coordinator-dispatch, and config-persistence boilerplate (net `-161` lines: `+193 / -354`) with zero behavior change and zero compiler warnings.
- **Files Changed:**
  - `src-tauri/src/win_shell.rs` — Removed dead `#[allow(dead_code)]` helpers (`create_hidden_message_window`, `post_close_to_hwnd`) and unused `point_inside_hwnd`; extracted `set_native_taskbars_visibility`, `MonitorRect::to_pixel_rect`, and `is_cursor_near_window`; tidied Win32 imports in `force_focus_window`.
  - `src-tauri/src/config.rs` — Removed unused `save_config` wrapper, narrowed `ensure_parent_dir` to private, and extracted `should_force_combo`.
  - `src-tauri/src/coordinator.rs` — Extracted private `WindowCoordinator::close_start` and `WindowCoordinator::close_flyout` helpers and reused them across all Start/flyout dismissal paths.
  - `src-tauri/src/lib.rs` — Added private `lock_unpoisoned`, `dispatch_coordinator`, unified `reevaluate_fullscreen(app, Option<bool>)`, and `normalize_bump_and_save`; wired `config::should_force_combo` and `win_shell::is_cursor_near_window` (eliminating raw `unsafe` Win32 code from `lib.rs`); removed redundant `stop_appbar_message_window()` calls immediately before `restore_native_taskbar()`.
  - `src-tauri/src/main.rs` & `src/lib/ipc.ts` — Verified read-only; clean with zero dead code or warnings.
- **Review Findings Breakdown:**
  - **Patched (0):** high 0, medium 0, low 0.
  - **Deferred (0):** None.
  - **Rejected (0):** None.
- **Follow-up Review Recommended:** `false` (patched counts by verdict: high 0, medium 0, low 0; Quick review reported 0 findings).
- **Verification Performed:**
  - `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`: **PASS (exit 0)** — zero Rust compiler warnings.
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **PASS (exit 0)** — 57/57 Rust unit/integration tests passed, zero Rust compiler warnings.
  - `pnpm test`: **PASS (exit 0)** — 22/22 Vitest tests passed (`layoutMap.test.ts`: 12, `ipc.test.ts`: 10).
  - `pnpm build`: **PASS (exit 0)** — `tsc && vite build` completed cleanly with zero warnings/errors.
  - Dead-code & scope scan: **PASS** — zero out-of-scope files modified; zero `#[allow(dead_code)]`, `create_hidden_message_window`, `post_close_to_hwnd`, `point_inside_hwnd`, or `config::save_config` remain across all 6 Epic 1 files.
- **Residual Risks:** None.

## Verification

**Commands:**
- `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` -- expected: exits `0` with zero Rust compiler warnings.
- `cargo test --manifest-path src-tauri/Cargo.toml` -- expected: exits `0`, all 57+ Rust unit/integration tests pass with zero warnings.
- `pnpm test` -- expected: exits `0`, all 22 Vitest tests pass non-interactively.
- `pnpm build` -- expected: exits `0`, `tsc && vite build` completes with zero TypeScript/Vite errors or warnings.
- `git diff --name-only` -- expected: only Epic 1 shell foundation files (plus plan file) modified, zero Epic 2–5 files touched.
