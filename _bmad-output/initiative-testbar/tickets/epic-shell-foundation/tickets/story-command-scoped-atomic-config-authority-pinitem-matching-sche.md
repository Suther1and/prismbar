---
id: 4
type: story
title: "Command-scoped atomic config authority, PinItem matching schema & autostart"
parent: epic-shell-foundation
covers: [R5]
after: [3]
refined: true
hitl: false
risk: medium
---

# Command-scoped atomic config authority, PinItem matching schema & autostart

## Description

Upgrades `%APPDATA%\TestBar\config.json` into a versioned, revision-tracked, command-scoped single source of truth shared across all three windows. Adds `schema_version = 1`, monotonic `revision`, canonical `PinItem` fields (`title`, `args`, `match_exe`, `match_aumid`, `combo_targets`), lossless migration of legacy `config.json` files (`name` -> `title`, legacy `GameCombo` -> polymorphic `kind = "combo"`), `pinned_clipboard` persistence, uniform add/remove/reorder across all 10 `PinItem` lists, zero full-document overwrite IPC commands, and console-free HKCU `Run` autostart.

## Requirements

1. **Versioned, revision-tracked atomic `TestBarConfig` authority** (R5)
   **Given** `%APPDATA%\TestBar\config.json` owned behind the Rust `AppState::config` mutex
   **When** the config is loaded, migrated from a legacy v0 file (missing `schemaVersion`, `revision`, `pinnedClipboard`, or using `name`/`GameCombo`), or mutated through any Tauri command
   **Then** `schema_version` is normalized to `1`, every successful mutation increments `revision` monotonically, writes atomically via temp file + sync + rename (cleaning up `.tmp` on failure), and emits `config-updated` carrying the updated snapshot; no full-document `save_config` overwrite IPC command exists in `lib.rs`.

2. **Polymorphic `PinItem` matching schema & lossless legacy migration** (R5)
   **Given** pinned items across taskbar, all 4 Spaces, and custom spaces
   **When** `PinItem` is created, deserialized from legacy JSON, or persisted
   **Then** `PinItem` carries canonical `title` (accepting legacy `"name"` via serde alias), optional `args`, `match_exe` (auto-derived lowercase `.exe` name from `target` when omitted on app pins), `match_aumid`, and `combo_targets` (accepting legacy `"targets"` via serde alias); `game_combos` stores polymorphic `PinItem` entries with `kind = "combo"` (migrating legacy `GameCombo` records losslessly), `pinned_clipboard` persists pinned clipboard strings, and `src/lib/ipc.ts` + frontend views compile cleanly against the canonical schema.

3. **Uniform add/remove/reorder across all 10 `PinItem` lists** (R5)
   **Given** any Space or taskbar list key (`personal`, `personal_links`, `work` / `work_folders`, `work_apps`, `studio`, `studio_tools`, `games` / `game_combos`, `game_launchers`, `taskbar` / `taskbar_pins`, or any `custom_spaces` id)
   **When** `add_pin_to_space`, `remove_pin_from_space`, or `reorder_pins_in_space` is called
   **Then** every list supports adding, removing, and bounds-safe index reordering uniformly (ignoring out-of-bounds or no-op moves without mutating or panicking), and `lib.rs` only bumps `revision` and writes to disk when a mutation actually changes state.

4. **Console-free HKCU `Run` autostart & stale-revision protection** (R5)
   **Given** the `set_autostart` command and frontend `config-updated` listeners
   **When** autostart is toggled or `config-updated` events arrive out of order
   **Then** HKCU `Run` updates execute without flashing a console window (`CREATE_NO_WINDOW` or Win32 Registry API) and propagate real OS errors instead of silently succeeding on failure, and frontend config listeners ignore snapshots with a `revision` older than the latest applied revision.

5. **Comprehensive Rust & Vitest verification** (R5)
   **Given** the upgraded config module and frontend IPC types
   **When** `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` execute
   **Then** all three commands exit `0` with unit tests verifying legacy v0 -> v1 migration (`name` -> `title`, `GameCombo` -> `kind = "combo"`, `match_exe` derivation), monotonic `revision` increments, atomic tmp+rename persistence & error cleanup, uniform reorder across all 10 `PinItem` lists, `pinned_clipboard` persistence, zero `save_config` IPC exposure, and frontend stale-revision filtering.

## Boundaries

- **Must not change:** Do not modify `win_shell.rs` AppBar/watchdog/hook logic (completed in Stories `1.2`–`1.3`); do not implement `IconCache` or UWP icon extraction (Epic 2); do not redesign `StartView.tsx` layout/Edit Mode/Search UI (Epic 4) beyond minimal TypeScript schema compatibility (`title`, `comboTargets`, revision guard); do not modify `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, or `system_widgets.rs` beyond keeping them compiling.
- **Must preserve:** Existing `%APPDATA%\TestBar\config.json` files written by the brownfield prototype load cleanly without resetting user pins; all 42 existing Rust tests and 12 Vitest tests remain green.

## References

- parent — `_bmad-output/initiative-testbar/tickets/epic-shell-foundation/epic-shell-foundation.md`, sections Requirements (R5) & Done when
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, section AD-4 & Consistency Conventions

## Notes

- Implementation guide: Upgrade `src-tauri/src/config.rs` with `CURRENT_SCHEMA_VERSION = 1`, `schema_version`, `revision`, `pinned_clipboard`, upgraded `PinItem` (`title` with `#[serde(alias = "name")]`, `args`, `match_exe`, `match_aumid`, `combo_targets` with `#[serde(alias = "targets")]`), `normalize_and_migrate`, `save_config_to(path, cfg)` / `load_config_from(path)` for isolated temp-dir unit tests, and uniform 10-list CRUD/reorder. Update `src-tauri/src/lib.rs` to route all config mutations through a single helper bumping `revision`, persisting atomically, and emitting `config-updated` (never exposing a `save_config` Tauri command). Update `src/lib/ipc.ts` (and minimal field references in `TaskbarView.tsx`, `StartView.tsx`, `FlyoutView.tsx`) plus a pure `shouldApplyConfigUpdate(prev, next)` helper tested in Vitest.
- Keep `config.rs` autostart command tests isolated (test argument/registry-value formatting cleanly without mutating the user's real HKCU `Run` key during `cargo test`).
