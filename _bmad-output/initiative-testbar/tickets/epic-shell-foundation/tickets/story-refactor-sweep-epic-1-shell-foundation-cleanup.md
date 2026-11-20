---
id: 5
type: story
title: "Refactor sweep — Epic 1 shell foundation cleanup"
parent: epic-shell-foundation
covers: [R1, R2, R3, R4, R5]
after: [4]
hitl: false
risk: low
---

# Refactor sweep — Epic 1 shell foundation cleanup

## Description

Cleanup-only sweep across the Epic 1 shell foundation (`coordinator.rs`, `win_shell.rs`, `config.rs`, `lib.rs`, `main.rs`, `src/lib/ipc.ts`): removes dead/unused legacy helpers (such as unused `#[allow(dead_code)]` functions left behind by Stories 1.1–1.4), deduplicates repeated boilerplate where clearly safer/cleaner, and confirms zero compiler warnings and 100% green automated tests without adding new feature scope or altering behavior.

## Requirements

Verify: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`, `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` all pass with zero errors and zero Rust/TypeScript compiler warnings across `coordinator.rs`, `win_shell.rs`, `config.rs`, `lib.rs`, `main.rs`, and `src/lib/ipc.ts`.

## References

- parent — _bmad-output/initiative-testbar/tickets/epic-shell-foundation/epic-shell-foundation.md
- architecture — _bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md, sections AD-1..AD-4, AD-6, AD-7

## Notes

- Cleanup only: Do not pull in Epic 2–5 features (do not touch `FlyoutView.tsx` hover-leave Epic 2 item or `system_widgets.rs` clipboard hydration Epic 3/5 item). Focus strictly on dead code, unused `#[allow(dead_code)]` helpers, minor hygiene/clarity, and ensuring every Epic 1 invariant holds cleanly.
