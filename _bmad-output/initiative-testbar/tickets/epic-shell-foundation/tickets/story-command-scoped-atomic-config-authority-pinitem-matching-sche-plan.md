---
title: 'Command-scoped atomic config authority, PinItem matching schema & autostart'
type: 'feature'
ticket: '4'
created: '2026-11-20'
status: done
baseline_revision: '865e8b6649ca9f59ee4e2fa01ff09e75dcafccf0'
route: 'full'
route_source: 'auto'
risk: 'medium'
review: 'thorough'
review_source: 'pinned'
lenses_ran: ['blind-hunter', 'edge-case-hunter', 'verification-gap', 'intent-alignment']
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md'
warnings: ['oversized']
deferred:
  - summary: >-
      Verify live cross-window `config-updated` event delivery and mounted `TaskbarView`/`StartView` stale-revision handling plus real HKCU `Run` autostart toggling in compiled `testbar.exe`.
    evidence: |-
      Pure `shouldApplyConfigUpdate`/`applyConfigUpdate`/`mergeTaskbarItems` helpers (Vitest), `mutate_config_with_emitter` (Rust), and isolated HKCU test-subkey Registry roundtrip (Rust) all pass, but mounted Tauri WebView2 event subscriptions and live `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` writes require an interactive Windows 11 desktop smoke test.
    location: >-
      src/components/TaskbarView.tsx:50
    severity: medium (unverified)
  - summary: >-
      Hydrate `system_widgets.rs` in-memory clipboard history from `config.pinned_clipboard` on startup so pinned clipboard entries reappear in `StartView` after relaunch.
    evidence: |-
      Story 1.4 persists `pinned_clipboard` on `toggle_clip_pin` and loads it into `AppState::config`, while Story 1.4 Boundaries explicitly prohibit modifying `system_widgets.rs` beyond keeping it compiling; startup hydration belongs to the Epic 3 `system_widgets.rs` clipboard upgrade.
    location: >-
      src-tauri/src/lib.rs:1146
    severity: medium
---

<intent-contract>

## Intent

**Problem:** `%APPDATA%\TestBar\config.json` lacks schema versioning, monotonic revision tracking, durable temp-file sync/cleanup, uniform CRUD/reorder across all 10 `PinItem` lists, polymorphic `PinItem` matching fields (`title`, `args`, `match_exe`, `match_aumid`, `combo_targets`), `pinned_clipboard` persistence, frontend stale-revision filtering, and console-free HKCU `Run` autostart with real OS error propagation.

**Approach:** Upgrade `config.rs` into a versioned (`schema_version = 1`), revision-tracked, transactionally persisted config authority with lossless v0->v1 migration (`name` -> `title`, legacy `GameCombo` -> polymorphic `kind = "combo"`), uniform 10-list pin operations returning change flags, and Win32 Registry API autostart; route all config IPC mutations in `lib.rs` through one transactional mutate-bump-save-emit helper (exposing zero full-document `save_config` IPC); and update `src/lib/ipc.ts` plus minimal view references with `shouldApplyConfigUpdate` stale-revision protection.

## Boundaries & Constraints

**Always:**
- `CURRENT_SCHEMA_VERSION = 1`; `load_config_from(path)` losslessly migrates legacy v0 JSON (missing `schemaVersion`, `revision`, `pinnedClipboard`, using `"name"` or `GameCombo` `"targets"`) without resetting user pins, derives lowercase `match_exe` from `.exe` targets on `kind == "app"` pins when omitted, normalizes `game_combos` entries to `kind = "combo"`, and persists upgraded v1 JSON atomically.
- `save_config_to(path, cfg)` writes to a sibling `.tmp` file, calls `sync_all()`, atomically renames over `path`, and deletes the `.tmp` file on any write/sync/rename error.
- `lib.rs` routes all config mutations (`set_bar_mode`, `set_autostart`, `add_pin`, `remove_pin`, `reorder_pins`, `add_custom_space`, `toggle_clip_pin`) through one transactional helper that bumps `revision` monotonically, writes atomically, updates in-memory state only after disk write succeeds, skips disk write/bump/emit on no-op mutations, and emits `config-updated`. No `save_config` Tauri command exists in `lib.rs`.
- `add_pin_to_space`, `remove_pin_from_space`, and `reorder_pins_in_space` return `bool` (whether state changed) and uniformly support all 10 `PinItem` lists (`personal`/`personal_pins`, `personal_links`, `work`/`work_folders`, `work_apps`, `studio`/`studio_pins`, `studio_tools`, `games`/`game_combos`, `game_launchers`, `taskbar`/`taskbar_pins`, and any `custom_spaces` id), ignoring out-of-bounds or no-op moves without mutating or panicking.
- `set_windows_autostart` uses the Win32 Registry API (`Win32_System_Registry`, zero console flash) against `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `TestBar`, treats `ERROR_FILE_NOT_FOUND` on disable as `Ok(())`, and propagates all other real OS errors. Unit tests never mutate the real HKCU `Run` key.
- Frontend views apply config snapshots strictly via `shouldApplyConfigUpdate(prev, next)` / `applyConfigUpdate(prev, next)`, ignoring snapshots with invalid or non-newer revisions.

**Never:**
- Do not modify `win_shell.rs`, `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, or `system_widgets.rs`.
- Do not implement `IconCache` or UWP extraction (Epic 2), and do not redesign `StartView.tsx` layout, Edit Mode, or Search UI (Epic 4) beyond minimal `title`/`comboTargets`/revision-guard compatibility.
- Do not expose any full-document `save_config` overwrite IPC command.
- Do not mutate `%APPDATA%\TestBar\config.json` or real HKCU `Run` during `cargo test`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Legacy v0 -> v1 migration | Brownfield JSON missing `schemaVersion`/`revision`/`pinnedClipboard`, `PinItem` with `"name"`, `gameCombos` with `GameCombo` (`name`, `targets`, no `kind`) | Loads all user pins intact; `schema_version = 1`; `name` -> `title`; `game_combos` items get `kind = "combo"` and `combo_targets`; app pins with `.exe` target derive lowercase `match_exe` (URLs/folders/schemes stay `None`); upgraded file persisted atomically | Corrupt/unreadable file falls back to `TestBarConfig::default()` without panicking |
| Atomic save & `.tmp` cleanup | `save_config_to(path, cfg)` succeeds on normal temp dir; fails when target path is an existing directory | Success atomically replaces target with valid JSON and leaves zero `.tmp` files; rename failure returns `Err(...)` and removes `.tmp` | Propagates I/O error string cleanly |
| Uniform 10-list CRUD & reorder | `add_pin_to_space`, `remove_pin_from_space`, `reorder_pins_in_space` across all 10 list keys & aliases + unknown space + out-of-bounds/equal indices + duplicate taskbar pin | Every valid list adds, reorders, removes and returns `true`; out-of-bounds, `from == to`, missing id, unknown space, and duplicate taskbar pin return `false` without mutating | Never panics |
| Transactional mutation helper | Valid mutation, no-op mutation, and mutation where `save_config_to` fails | Valid mutation bumps `revision` by 1 and returns `changed = true`; no-op keeps `revision` unchanged and skips write; failed save rolls back in-memory state and `revision` | Returns `Err(...)` without diverging in-memory config |
| Autostart formatting & error handling | Valid exe path, empty/NUL path; delete error codes (`ERROR_SUCCESS`, `ERROR_FILE_NOT_FOUND`, `ERROR_ACCESS_DENIED`) | Valid path formats quoted `REG_SZ`; `ERROR_SUCCESS`/`ERROR_FILE_NOT_FOUND` map to `Ok(())`; `ERROR_ACCESS_DENIED` maps to `Err(...)` | Pure helpers tested without touching real HKCU |
| Frontend stale-revision filter | `prev = null` + `rev = 0`; `prev = 5` + `next = 6`; `prev = 5` + `next = 4`; `prev = 5` + `next = 5`; `next = NaN / -1` | Accepts initial (`null -> 0`) and strictly newer (`5 -> 6`); ignores older (`5 -> 4`), duplicate (`5 -> 5`), and invalid (`NaN`, `-1`, `null`) | Never throws |

</intent-contract>

## Code Map

- `src-tauri/Cargo.toml` -- Add `"Win32_System_Registry"` to `[dependencies.windows].features` (lines 27–64) so `config.rs` can manage HKCU `Run` via Win32 Registry API without spawning `reg.exe`.
- `src-tauri/src/config.rs` -- Replace `GameCombo` with polymorphic `PinItem` (`kind` supporting `"combo"`, `title` with `#[serde(alias = "name")]`, `args`, `match_exe`, `match_aumid`, `combo_targets` with `#[serde(alias = "targets")]`). Add `CURRENT_SCHEMA_VERSION = 1`, `schema_version`, `revision`, `pinned_clipboard` on `TestBarConfig`; add `derive_match_exe`, `normalize_and_migrate`, `load_config_from`, `save_config_to` (sibling `.tmp` + `sync_all` + `rename` + cleanup on error), `set_pinned_clipboard`, uniform 10-list `add_pin_to_space`/`remove_pin_from_space`/`reorder_pins_in_space` returning `bool`, and Win32 Registry `set_windows_autostart` with pure testable `format_autostart_command`/`classify_autostart_delete_error`. Preserve existing `default_config_roundtrip_and_universal_pinning` test and add full unit test suite.
- `src-tauri/src/lib.rs` -- Add transactional `mutate_config_at` / `mutate_config_and_emit` helpers (poison-safe mutex, clone-mutate-bump-save-commit, emit `config-updated` only when changed). Update `set_bar_mode` (lines 435–454), `set_autostart` (lines 457–467), `add_pin` (lines 622–634), `remove_pin` (lines 637–646), `reorder_pins` (lines 649–658), `add_custom_space` (lines 661–681), and `toggle_clip_pin` (lines 782–786, syncing toggled entry to `config.pinned_clipboard` without modifying `system_widgets.rs`). Ensure zero `save_config` Tauri command exists and add integration tests in `integration_tests`.
- `src/lib/ipc.ts` -- Update `PinItem` (`kind: 'app' | 'folder' | 'file' | 'url' | 'combo'`, `title`, optional `args`/`matchExe`/`matchAumid`/`comboTargets`), remove `GameCombo`, update `TestBarConfig` (`schemaVersion`, `revision`, `gameCombos: PinItem[]`, `pinnedClipboard: string[]`), and export `shouldApplyConfigUpdate` + `applyConfigUpdate`.
- `src/lib/ipc.test.ts` -- New Vitest suite verifying `shouldApplyConfigUpdate` and `applyConfigUpdate` across initial, newer, older, duplicate, and invalid revisions.
- `src/components/TaskbarView.tsx` -- Lines 46, 52, 86–97: use `applyConfigUpdate` on `get_config`/`config-updated` and read `p.title` / `p.matchExe`.
- `src/components/FlyoutView.tsx` -- Line 430: send `title: p.name` in `add_pin` payload.
- `src/components/StartView.tsx` -- Lines 75, 86, 156–158, 270, 296, 380, 664, 848, 893, 1013, 1169, 1231, 1237, 1338, 1377, 1384, 1398, 1482: minimal compatibility updates to `title`, `comboTargets`, and `applyConfigUpdate` (no layout/UI redesign).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml` -- Enable `Win32_System_Registry` feature on `windows` 0.58 -- allows console-free Win32 HKCU `Run` updates without subprocesses.
- [x] `src-tauri/src/config.rs` -- Implement schema v1 `TestBarConfig`, polymorphic `PinItem` + `derive_match_exe` + lossless `normalize_and_migrate`, atomic `load_config_from`/`save_config_to` with `.tmp` cleanup, `set_pinned_clipboard`, uniform 10-list add/remove/reorder returning `bool`, Win32 Registry `set_windows_autostart` with pure helpers, and unit tests covering every I/O Matrix scenario -- establishes the persistent config authority.
- [x] `src-tauri/src/lib.rs` -- Route all config mutations (`set_bar_mode`, `set_autostart`, `add_pin`, `remove_pin`, `reorder_pins`, `add_custom_space`, `toggle_clip_pin`) through one transactional helper bumping `revision`, saving atomically, and emitting `config-updated` only on real changes, plus integration tests verifying transactional rollback, no-op skip, and zero `save_config` IPC exposure -- prevents lost-update races and in-memory/disk divergence.
- [x] `src/lib/ipc.ts` -- Update `PinItem` and `TestBarConfig` TypeScript interfaces to canonical v1 schema and export pure `shouldApplyConfigUpdate` / `applyConfigUpdate` helpers -- aligns frontend contract and guards against out-of-order events.
- [x] `src/lib/ipc.test.ts` -- Add Vitest tests for `shouldApplyConfigUpdate` and `applyConfigUpdate` -- verifies stale-revision filtering non-interactively.
- [x] `src/components/TaskbarView.tsx`, `src/components/FlyoutView.tsx`, `src/components/StartView.tsx` -- Apply minimal TypeScript schema compatibility (`title`, `comboTargets`, `applyConfigUpdate`) without redesigning UI -- keeps `pnpm build` green and protects all views from stale config snapshots.

**Acceptance Criteria:**
- Given a legacy brownfield `config.json` (v0, missing `schemaVersion`/`revision`/`pinnedClipboard`, using `"name"` and `GameCombo` `"targets"`), when loaded via `load_config_from` and mutated through the command mutation path, then all user pins are preserved under `schemaVersion: 1`, `gameCombos` entries have `kind: "combo"` and `comboTargets`, app pins derive lowercase `matchExe`, `revision` increments monotonically only on real state changes, and atomic persistence leaves zero `.tmp` files on both success and failure.
- Given the compiled backend IPC registry and frontend views after all edits, when `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` run non-interactively, then all three commands exit `0` with zero Rust warnings, zero `save_config` IPC command exposed, zero modifications to `win_shell.rs` or real HKCU `Run` during tests, and all existing + new Rust and Vitest tests passing.

## Implementation Notes

- Enabled `Win32_System_Registry` in `src-tauri/Cargo.toml` so `config.rs` manages HKCU `Run` via Win32 Registry API (`RegCreateKeyExW`, `RegSetValueExW`, `RegOpenKeyExW`, `RegDeleteValueW`, RAII `RegKeyGuard`) with zero subprocesses or console flashes.
- Upgraded `src-tauri/src/config.rs` with `CURRENT_SCHEMA_VERSION = 1`, polymorphic `PinItem` (`title` with `#[serde(alias = "name")]`, `args`, `match_exe`, `match_aumid`, `combo_targets` with `#[serde(alias = "targets")]`), `schema_version`, `revision`, `pinned_clipboard`, `derive_match_exe`, `normalize_pin`, `normalize_and_migrate`, `load_config_from`, atomic `save_config_to` (sibling `.tmp` + `sync_all` + `rename` + cleanup on failure), `set_pinned_clipboard`, uniform 10-list CRUD/reorder returning `bool`, pure autostart helpers (`format_autostart_command`, `classify_autostart_delete_error`), and 7 Rust unit tests.
- Upgraded `src-tauri/src/lib.rs` with transactional `mutate_config_at` and `mutate_config_and_emit`, routing `set_bar_mode`, `set_autostart`, `add_pin`, `remove_pin`, `reorder_pins`, `add_custom_space`, and `toggle_clip_pin` through it with poison-safe locking, no-op write skipping, rollback on disk failure, zero `save_config` IPC exposure, and 3 integration tests.
- Updated `src/lib/ipc.ts` with canonical v1 `PinItem`/`TestBarConfig` interfaces plus `shouldApplyConfigUpdate`/`applyConfigUpdate`, added 6 Vitest tests in `src/lib/ipc.test.ts`, and applied minimal schema/revision-guard compatibility in `src/components/TaskbarView.tsx`, `src/components/FlyoutView.tsx`, and `src/components/StartView.tsx`.
- Verification passed before review: `cargo test --manifest-path src-tauri/Cargo.toml` (52/52), `pnpm test` (18/18), `pnpm build` (`tsc && vite build`), and `git diff --name-only` confirmed zero out-of-scope file modifications.

## Plan Change Log

## Review Triage Log

### 2026-11-20 — Review pass
- verdicts: 40 findings — high 7, medium 23, low 6, false 3, maybe-false 1
- findings:
  - `[medium]` `[patch]` (blind-hunter) `toggle_clip_pin` silently swallows `mutate_config_and_emit` errors (and `system_widgets.rs` does not hydrate `pinned_clipboard` on startup) — Reverted in-memory clipboard pin via `system_widgets::toggle_clipboard_pin(&id)` when `mutate_config_and_emit` fails in `lib.rs`; startup hydration in `system_widgets.rs` is explicitly out of scope per Story 1.4 Boundaries and recorded in `deferred`.
  - `[medium]` `[patch]` (blind-hunter) `set_bar_mode` emits `config-updated` before updating AppBar mode and repositioning taskbar bounds — Reordered `set_bar_mode` to call `mutate_config_at`, update AppBar + `position_taskbar_window` + coordinator bounds, and emit `config-updated` afterward.
  - `[medium]` `[patch]` (blind-hunter) `set_autostart` calls `set_windows_autostart` outside config mutex with no rollback on disk save failure — Extracted `apply_autostart_transaction` holding the config mutex across registry + save with compensating `apply_registry(prev_enabled)` rollback on save error, verified by unit test.
  - `[medium]` `[patch]` (blind-hunter) Missing `.catch`/error handling on `StartView.tsx` and `FlyoutView.tsx` config mutations — Added `.catch(() => {})` / `try/catch` to `set_bar_mode`, `set_autostart`, `add_custom_space`, `add_pin`, `remove_pin`, and `reorder_pins` calls.
  - `[high]` `[patch]` (blind-hunter) Missing `#[serde(default)]` on `TestBarConfig.bar_mode`/`CustomSpace` accents and missing UTF-8 BOM stripping — Added `#[serde(default)]` to `bar_mode`, `accent_hex`, and `accent_rgb`, stripped `\u{feff}` in `load_config_from`, and added unit test coverage.
  - `[medium]` `[patch]` (blind-hunter) `normalize_and_migrate` silently downgrades `schema_version > CURRENT_SCHEMA_VERSION` on load — Guarded migration with `cfg.schema_version < CURRENT_SCHEMA_VERSION` and added unit test for `schemaVersion: 2`.
  - `[false]` `[reject]` (blind-hunter) Asymmetric space alias routing for `"work"` and `"games"` across add/remove/reorder — Refuted: routing matches Story 1.4 Requirement 3 (`work` / `work_folders`, `games` / `game_combos`) and preserves existing `StartView.tsx` add-drawer calls.
  - `[medium]` `[patch]` (blind-hunter) Incomplete `normalize_pin` handling for whitespace/uppercase `kind`, non-forced combos, blank `combo_targets`, and non-app `match_exe` — Normalized `kind`, populated and trimmed/filtered `combo_targets` for all `kind == "combo"` items, and trimmed/lowercased `match_exe` consistently with unit tests.
  - `[medium]` `[patch]` (blind-hunter) `derive_match_exe` rejects `\\?\` verbatim paths and mishandles targets with CLI arguments — Stripped `\\?\UNC\`/`\\?\` prefixes, parsed quoted executables before trailing args, rejected unquoted trailing flags, and added unit tests.
  - `[medium]` `[patch]` (blind-hunter) `TaskbarView.tsx` deduplication edge cases (`!usedGroupKeys`, bare/slash paths, empty `title`, trailing `\`) — Extracted pure `mergeTaskbarItems` into `src/lib/ipc.ts` fixing all matching edge cases and wired `TaskbarView.tsx` to it with Vitest coverage (`StartView.tsx` `isRunning` left untouched per Epic 4 boundary).
  - `[low]` `[patch]` (blind-hunter) `saturating_add(1)` at `u64::MAX` and `Number.isInteger` vs `Number.isSafeInteger` — Switched `mutate_config_at` to `checked_add(1)` and `isValidRevision` to `Number.isSafeInteger(rev)` with tests.
  - `[low]` `[patch]` (blind-hunter) `add_pin` runs icon extraction before `normalize_pin` defaults empty `kind` — Invoked `config::normalize_pin(&mut item, force_combo)` prior to icon extraction check in `add_pin`.
  - `[false]` `[reject]` (blind-hunter) Holding `AppState::config` mutex across `save_config_to` stalls readers — Refuted: holding the config mutex across atomic save is required to serialize monotonic revision increments and prevent concurrent out-of-order disk renames; mutations are low-frequency user actions.
  - `[medium]` `[patch]` (blind-hunter) `format_autostart_command` does not strip `\\?\` prefixes or reject embedded `"` quotes — Stripped `\\?\UNC\`/`\\?\` prefixes, rejected embedded `"`, and added unit tests.
  - `[low]` `[patch]` (blind-hunter) Brittle source-text assertion in `no_full_document_save_config_ipc_command_is_exposed` and missing edge-case tests — Hardened test to parse `generate_handler![...]` identifiers and reject any `#[tauri::command]` taking `TestBarConfig`, plus added BOM/missing-`barMode`/v2 schema tests.
  - `[medium]` `[patch]` (edge-case-hunter) `set_autostart` registry/config divergence and concurrency race — Fixed via `apply_autostart_transaction` with config lock and compensating registry rollback on save failure.
  - `[medium]` `[patch]` (edge-case-hunter) `toggle_clip_pin` ignores `mutate_config_and_emit` error after flipping clipboard state — Fixed by reverting in-memory clipboard state via `system_widgets::toggle_clipboard_pin(&id)` on save error.
  - `[medium]` `[patch]` (edge-case-hunter) `set_bar_mode` emits `config-updated` before updating AppBar/bounds — Fixed by emitting `config-updated` after AppBar and taskbar window repositioning.
  - `[low]` `[patch]` (edge-case-hunter) `add_pin` checks `item.kind` for icon extraction before `normalize_pin` — Fixed by calling `config::normalize_pin` before the icon extraction check.
  - `[low]` `[patch]` (edge-case-hunter) `mutate_config_at` `saturating_add(1)` stalls revision at `u64::MAX` — Fixed via `checked_add(1)` returning overflow error.
  - `[medium]` `[patch]` (edge-case-hunter) `normalize_and_migrate` downgrades `schemaVersion > CURRENT_SCHEMA_VERSION` — Fixed by guarding with `cfg.schema_version < CURRENT_SCHEMA_VERSION`.
  - `[medium]` `[patch]` (edge-case-hunter) `derive_match_exe` mishandles `\\?\` paths and CLI args ending in `.exe` — Fixed verbatim prefix stripping and quoted/unquoted arg handling.
  - `[medium]` `[patch]` (edge-case-hunter) `normalize_pin` whitespace-padded `kind`, non-forced combo `combo_targets`, and `match_exe` normalization — Fixed in `normalize_pin` with unit tests.
  - `[high]` `[patch]` (edge-case-hunter) Missing `#[serde(default)]` on `TestBarConfig.bar_mode` resets user pins — Fixed by adding `#[serde(default)]` to `bar_mode` and `CustomSpace` accent fields.
  - `[high]` `[patch]` (edge-case-hunter) UTF-8 BOM in `config.json` fails `serde_json::from_str` and resets user pins — Fixed by stripping leading `\u{feff}` in `load_config_from`.
  - `[medium]` `[patch]` (edge-case-hunter) `TaskbarView.tsx` `matchExe` matching fails on bare exe names or `/` separators — Fixed in extracted `mergeTaskbarItems` using normalized basename comparison.
  - `[low]` `[patch]` (edge-case-hunter) `isValidRevision` accepts unsafe integers above `Number.MAX_SAFE_INTEGER` — Fixed via `Number.isSafeInteger(rev)`.
  - `[medium]` `[patch]` (edge-case-hunter) Unhandled promise rejection on `set_bar_mode`/`set_autostart` in `StartView.tsx` — Fixed with `.catch(() => {})` handlers.
  - `[high]` `[patch]` (verification-gap) Win32 Registry autostart write/delete paths in `set_windows_autostart` untested — Factored `set_windows_autostart_at` and added `autostart_registry_roundtrip_on_isolated_temp_subkey` exercising real Win32 Registry APIs on an isolated temporary subkey under `HKCU\Software\TestBar\TestAutostart\<pid>-<seq>` with guaranteed cleanup (never touching HKCU `Run`).
  - `[high]` `[patch]` (verification-gap) 10-list CRUD/reorder test only checked boolean returns on pre-populated default lists — Strengthened `uniform_10_list_crud_reorder_and_edge_cases` with `target_slice` asserting exact target list mutation and post-reorder element order across all 10 lists and aliases.
  - `[medium]` `[patch]` (verification-gap) Explicit `match_exe` override preservation, blank `args`/`match_aumid`, invalid `bar_mode`, `pinned_clipboard` cleanup, and `normalize_and_migrate` idempotency untested — Added `normalize_and_migrate_handles_kinds_combos_overrides_clip_and_is_idempotent`.
  - `[medium]` `[patch]` (verification-gap) `TaskbarView.tsx` `matchExe` deduplication untested — Extracted `mergeTaskbarItems` into `src/lib/ipc.ts` and added Vitest unit tests in `src/lib/ipc.test.ts`.
  - `[medium]` `[patch]` (verification-gap) `toggle_clip_pin` discards save errors (and `system_widgets.rs` lacks startup hydration) — Patched `toggle_clip_pin` rollback in `lib.rs`; deferred `system_widgets.rs` startup hydration per Story 1.4 Boundaries.
  - `[medium]` `[patch]` (verification-gap) `set_autostart` lacks rollback on config save failure — Fixed via `apply_autostart_transaction` with compensating rollback and unit test.
  - `[medium]` `[patch]` (intent-alignment) `pinned_clipboard` write-side error handling and restart hydration gap — Patched `toggle_clip_pin` error rollback in `lib.rs`; deferred `system_widgets.rs` startup hydration per Story 1.4 Boundaries.
  - `[medium]` `[patch]` (intent-alignment) `mutate_config_and_emit` emitter gating untested and brittle `save_config` substring check — Factored `mutate_config_with_emitter` with unit test and hardened `no_full_document_save_config_ipc_command_is_exposed`.
  - `[high]` `[patch]` (intent-alignment) Win32 Registry autostart execution untested, missing `set_autostart` rollback, and missing `StartView.tsx` `.catch` — Patched via isolated temp-subkey Win32 Registry roundtrip test, `apply_autostart_transaction` rollback, and frontend `.catch` handlers.
  - `[high]` `[patch]` (intent-alignment) `bar_mode` missing `#[serde(default)]` and `args`/`match_aumid` normalization untested — Patched `#[serde(default)]` on `bar_mode` and added normalization tests covering `args`/`match_aumid`.
  - `[false]` `[reject]` (intent-alignment) Multi-list alias routing and unchanged `StartView.tsx` drag-and-drop wiring — Refuted: alias routing matches Story 1.4 Requirement 3 and `StartView.tsx` UI/Edit Mode redesign is explicitly excluded by Story 1.4 Boundaries.
  - `[maybe-false]` `[defer]` (intent-alignment) Mounted React `TaskbarView`/`StartView` `config-updated` event listener integration not exercised in live WebView2 — Pure `shouldApplyConfigUpdate`/`applyConfigUpdate`/`mergeTaskbarItems` and `mutate_config_with_emitter` are unit-tested; live cross-window Tauri event delivery deferred to interactive Windows 11 desktop smoke testing (`medium (unverified)`).

## Auto Run Result

- **Summary:** Upgraded `%APPDATA%\TestBar\config.json` into a schema-v1, revision-tracked, transactionally persisted config authority with lossless legacy v0 migration (`name` -> `title`, `GameCombo` -> polymorphic `kind = "combo"`, lowercase `match_exe` derivation, UTF-8 BOM & missing-field tolerance, future schema preservation), uniform 10-list pin CRUD/reorder returning change flags, `pinned_clipboard` write-side persistence with rollback on save failure, console-free Win32 Registry API HKCU `Run` autostart with transactional rollback, zero full-document `save_config` IPC exposure, and frontend stale-revision filtering plus hardened `matchExe` taskbar deduplication.
- **Files Changed:**
  - `src-tauri/Cargo.toml` — Enabled `Win32_System_Registry` on `windows` 0.58 for console-free HKCU `Run` registry management.
  - `src-tauri/src/config.rs` — Implemented `CURRENT_SCHEMA_VERSION = 1`, polymorphic `PinItem`, `schema_version`/`revision`/`pinned_clipboard` on `TestBarConfig`, `derive_match_exe`, `normalize_pin`, `normalize_and_migrate`, BOM-tolerant `load_config_from`, atomic temp+sync+rename `save_config_to` with `.tmp` cleanup, `set_pinned_clipboard`, uniform 10-list add/remove/reorder returning `bool`, Win32 Registry `set_windows_autostart` / `set_windows_autostart_at` + pure helpers, and 11 Rust unit tests (including isolated temporary HKCU subkey readback/delete test never touching HKCU `Run`).
  - `src-tauri/src/lib.rs` — Added transactional `mutate_config_at`, `mutate_config_with_emitter`, `mutate_config_and_emit`, and `apply_autostart_transaction`; routed `set_bar_mode` (emitting after AppBar/bounds update), `set_autostart` (with registry rollback on save error), `add_pin` (normalizing before icon extraction), `remove_pin`, `reorder_pins`, `add_custom_space`, and `toggle_clip_pin` (reverting clipboard toggle on save error) through the authority with zero `save_config` IPC exposed; added 5 Rust integration tests.
  - `src/lib/ipc.ts` — Updated `PinItem` and `TestBarConfig` to canonical v1 schema, removed `GameCombo`, and exported `shouldApplyConfigUpdate`, `applyConfigUpdate`, `MergedBarItem`, and `mergeTaskbarItems`.
  - `src/lib/ipc.test.ts` — Added 10 Vitest unit tests covering stale/duplicate/invalid/unsafe-integer revision filtering and `mergeTaskbarItems` pinned-to-running deduplication (`matchExe`, boundary non-collision, `/` & bare paths, duplicate-pin single-binding, `title`).
  - `src/components/TaskbarView.tsx` — Wired `applyConfigUpdate` on `get_config`/`config-updated` and delegated taskbar pin merging to `mergeTaskbarItems`.
  - `src/components/FlyoutView.tsx` — Updated `add_pin` payload to canonical `title` and added `.catch(() => {})` on config IPC calls.
  - `src/components/StartView.tsx` — Applied minimal schema compatibility (`title`, `comboTargets`, `applyConfigUpdate`) and `.catch`/`try-catch` guards on config mutations without altering UI layout.
- **Review Findings Breakdown:**
  - **Patched (15 grouped entries across 36 raw findings — 3 `high`, 10 `medium`, 2 `low`):** 3 `high` (BOM stripping + `#[serde(default)]` on `bar_mode`/`CustomSpace`; isolated temp-subkey Win32 Registry roundtrip test; exact-slice 10-list CRUD/reorder assertions), 10 `medium` (`toggle_clip_pin` error rollback; `set_bar_mode` emit-after-AppBar/bounds; `set_autostart` transactional lock + registry rollback + test; frontend `.catch`/`try-catch`; newer `schema_version` preservation; `normalize_pin`/`normalize_and_migrate` hardening + tests; `derive_match_exe` verbatim/CLI-arg handling; `format_autostart_command` verbatim/quote handling; extracted `mergeTaskbarItems` + Vitest tests; `mutate_config_with_emitter` test + hardened `no_full_document_save_config_ipc_command_is_exposed`), 2 `low` (`checked_add(1)` + `Number.isSafeInteger`; `add_pin` normalize before icon check).
  - **Deferred (2):** Live Windows 11 WebView2 cross-window `config-updated` delivery to mounted `TaskbarView`/`StartView` & real HKCU `Run` autostart smoke verification (`medium (unverified)`); startup hydration of `system_widgets.rs` clipboard history from `config.pinned_clipboard` (`medium`, out of scope per Story 1.4 Boundaries, owned by Epic 3).
  - **Rejected (3):** `"work"`/`"games"` alias routing (`false` — matches Story 1.4 Requirement 3 and `StartView.tsx`); holding config mutex across `save_config_to` (`false` — required to serialize monotonic revisions and atomic disk renames); `StartView.tsx` drag-and-drop wiring (`false` — explicitly excluded by Story 1.4 Boundaries).
- **Follow-up Review Recommended:** `true` (first pass patched 3 `high`, 10 `medium`, and 2 `low` grouped entries; specific unverified risk: live interactive Windows 11 desktop smoke verification of cross-window WebView2 `config-updated` delivery into mounted `TaskbarView`/`StartView` instances and real `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` autostart toggling in compiled `testbar.exe`).
- **Verification Performed:**
  - `cargo test --manifest-path src-tauri/Cargo.toml`: **PASS (exit 0)** — 57/57 Rust tests passed (11 `config::tests`, 21 `coordinator::tests`, 15 `win_shell::tests`, 10 `integration_tests`), zero Rust compiler warnings.
  - `pnpm test`: **PASS (exit 0)** — 22/22 Vitest tests passed (`layoutMap.test.ts`: 12, `ipc.test.ts`: 10) non-interactively.
  - `pnpm build`: **PASS (exit 0)** — `tsc && vite build` succeeded cleanly.
  - `git diff --name-only`: **PASS** — zero modifications to `win_shell.rs`, `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, or `system_widgets.rs`.
- **Residual Risks:** Interactive Windows 11 smoke testing is still needed for live cross-window `config-updated` delivery and real HKCU `Run` autostart execution; startup hydration of `config.pinned_clipboard` into `system_widgets.rs` awaits Epic 3.

## Design Notes

- **Transactional config mutation (`lib.rs`):** Clone `TestBarConfig` under the mutex, run the mutation closure returning `bool`, normalize, bump `revision += 1`, persist via `save_config_to`, and commit `*cfg = candidate` only on `Ok(())`. Disk errors leave in-memory state and `revision` unchanged.
- **Space key routing (`config.rs`):** `"work"` adds `kind == "app"` to `work_apps` and non-apps to `work_folders`, removes from both, and reorders `work_folders` (`"work_apps"` targets `work_apps` directly); `"games"` adds `kind == "combo"` to `game_combos` and non-combos to `game_launchers`, removes from both, and reorders `game_combos` (`"game_launchers"` targets `game_launchers` directly). Explicit list keys (`personal`/`personal_pins`, `personal_links`, `work_folders`, `work_apps`, `studio`/`studio_pins`, `studio_tools`, `game_launchers`, `game_combos`, `taskbar`/`taskbar_pins`, and `<custom_space_id>`) operate directly on their exact list.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml` -- expected: exits `0`, all 42+ existing and new Rust unit/integration tests pass with zero compiler warnings.
- `pnpm test` -- expected: exits `0`, all Vitest suites (`layoutMap.test.ts` + `ipc.test.ts`) pass non-interactively.
- `pnpm build` -- expected: exits `0`, `tsc && vite build` succeeds cleanly.
- `git diff --name-only` -- expected: zero edits to `win_shell.rs`, `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, or `system_widgets.rs`.
