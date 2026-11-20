---
id: 3
type: story
title: "Non-blocking global hooks channel & strict fullscreen auto-hide integration"
parent: epic-shell-foundation
covers: [R2, R4]
after: [2]
refined: true
hitl: false
risk: high
---

# Non-blocking global hooks channel & strict fullscreen auto-hide integration

## Description

Eliminates low-level hook stalls and Windows `LowLevelHooksTimeout` silent unhooking by moving all `WH_KEYBOARD_LL` and `WH_MOUSE_LL` event dispatch onto a bounded `sync_channel(64)` (`try_send`) drained by a dedicated worker thread, extracts a pure testable `WinKeyStateMachine` that preserves every `Win+<key>` OS shortcut, and locks down single-authority fullscreen auto-hide across both `ABN_FULLSCREENAPP` and periodic checks so maximized desktop apps never false-hide and taskbar resizes never leak over fullscreen games.

## Requirements

1. **Bounded non-blocking hook channel (`capacity = 64`) & dedicated worker thread** (R4)
   **Given** `WH_KEYBOARD_LL` and `WH_MOUSE_LL` hooks installed in `win_shell.rs`
   **When** keyboard or mouse events arrive on the hook thread, including bursts exceeding `64` unhandled events
   **Then** hook callbacks perform zero Tauri IPC, zero `SetWindowPos`/`ShowWindow`, and zero blocking calls; they only enqueue compact `ShellHookEvent` values via non-blocking `try_send` onto a bounded `sync_channel(64)` (dropping excess events cleanly instead of blocking), and a dedicated worker thread drains the receiver into the application handler.

2. **Pure testable `WinKeyStateMachine` preserving `Win+<key>` OS chords** (R4)
   **Given** keyboard events processed by the low-level keyboard hook
   **When** `VK_LWIN` or `VK_RWIN` is pressed and released alone vs combined with any non-Win key (`Win+E`, `Win+D`, `Win+R`, `Win+L`, `Win+V`, `Win+Shift+S`, `Win+Tab`), repeated while held, accompanied by injected events (`LLKHF_INJECTED`), or chorded with both Win keys
   **Then** only a true lone Win press-and-release with zero intervening non-Win keys triggers `ToggleStart` (suppressing native Windows Start), while all `Win+<key>` OS shortcuts pass through untouched and never falsely open TestBar Start.

3. **Single-authority strict fullscreen auto-hide across `ABN_FULLSCREENAPP` & poller** (R2)
   **Given** TestBar running with both the top-level AppBar message window (`ABN_FULLSCREENAPP`) and the fast poller active
   **When** `ABN_FULLSCREENAPP` arrives (with `lparam` true or false), a maximized/borderless desktop app is focused, a true fullscreen game/presentation starts or exits, or `resize_taskbar_dock` / `position_taskbar_window` fires while fullscreen-suppressed
   **Then** `ABN_FULLSCREENAPP` acts strictly as a re-evaluation trigger into `coordinator::classify_fullscreen` (never blindly overriding it with raw `lparam`), normal maximized desktop windows never trigger fullscreen hide, and taskbar bounds updates never show or leak the taskbar while fullscreen-suppressed.

4. **Deterministic unit tests & clean verification** (R2, R4)
   **Given** the pure hook state machine, bounded queue helper, and fullscreen integration logic
   **When** `cargo test --manifest-path src-tauri/Cargo.toml`, `pnpm test`, and `pnpm build` run
   **Then** all three commands exit `0` with passing unit tests covering lone Win tap vs `Win+<key>` chords/repeats/injected keys, bounded `sync_channel(64)` drop-on-full non-blocking behavior and worker drain, and fullscreen suppression blocking taskbar visibility on resize/reposition plus `ABN_FULLSCREENAPP` single-authority re-evaluation.

## Boundaries

- **Must not change:** Do not modify `config.rs` schema or `PinItem` fields (owned by Story `1.4`); do not alter `audio.rs`, `media.rs`, `telemetry.rs`, `apps_indexer.rs`, `system_widgets.rs`, or frontend React UI components.
- **Must preserve:** All Story `1.1` (`WindowCoordinator`, `tauri.conf.json`, Vitest) and Story `1.2` (`SingleInstanceGuard`, top-level `WS_OVERLAPPED` AppBar/`TaskbarCreated`, HiDPI `recovery-workarea.json`, watchdog, panic hook) invariants and tests remain green; `Ctrl+Alt+Shift+Esc` emergency restore continues to unhook hooks cleanly before restoring native taskbars.

## References

- parent — `_bmad-output/initiative-testbar/tickets/epic-shell-foundation/epic-shell-foundation.md`, sections Requirements (R2, R4) & Done when
- architecture — `_bmad-output/initiative-testbar/architecture-testbar/architecture-testbar.md`, sections AD-1, AD-3, AD-6

## Notes

- Implementation guide: Add a pure `WinKeyStateMachine` and `HookEventChannel` (wrapping `std::sync::mpsc::sync_channel::<ShellHookEvent>(64)` with `try_send`) in `src-tauri/src/win_shell.rs`; replace direct `HOOK_CALLBACK` invocation in `ll_keyboard_proc` and `ll_mouse_proc` with `try_send` + dedicated worker thread draining `Receiver<ShellHookEvent>` into the caller's callback; ensure `LLKHF_INJECTED` events are ignored by `WinKeyStateMachine`; verify `lib.rs` `ABN_FULLSCREENAPP`, `h_fast`, `resize_taskbar_dock`, and `position_taskbar_window` honor `WindowCoordinator`'s single fullscreen authority with zero visibility leaks.
- High-risk check: Confirm by inspection that `ll_keyboard_proc` and `ll_mouse_proc` contain zero Tauri IPC calls, zero `SetWindowPos`/`ShowWindow` calls, zero mutex locks that wait on GUI/IPC work, and strictly use non-blocking `try_send` into the `capacity = 64` channel.
