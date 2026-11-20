import type { AppWindowGroup, FlyoutKind, FlyoutRequest, OpenWindowInfo } from './ipc';

export type FlyoutPhase = 'closed' | 'entering' | 'open' | 'closing';

export interface FlyoutAnimState {
  phase: FlyoutPhase;
  renderSeq: number;
  req: FlyoutRequest | null;
}

export const FLYOUT_EXIT_MS = 120;
export const PREVIEW_LEAVE_COMMAND = 'close_preview_if_outside' as const;

export const INITIAL_FLYOUT_ANIM_STATE: FlyoutAnimState = {
  phase: 'closed',
  renderSeq: 0,
  req: null,
};

/**
 * Transitions flyout into `'entering'` with a incremented `renderSeq` so CSS spring `flyIn`
 * replays cleanly on every open (including rapid reopen cancelling an in-flight exit).
 */
export function reduceFlyoutShow(state: FlyoutAnimState, req: FlyoutRequest): FlyoutAnimState {
  return {
    phase: 'entering',
    renderSeq: state.renderSeq + 1,
    req,
  };
}

export function reduceFlyoutEntered(state: FlyoutAnimState): FlyoutAnimState {
  if (state.phase !== 'entering' || !state.req) {
    return state;
  }
  return { ...state, phase: 'open' };
}

/**
 * Begins the `120ms` exit animation while keeping `req` mounted so `flyOut` plays visibly.
 */
export function reduceFlyoutHideStart(state: FlyoutAnimState): FlyoutAnimState {
  if (!state.req || state.phase === 'closed' || state.phase === 'closing') {
    return state;
  }
  return { ...state, phase: 'closing' };
}

/**
 * Completes the exit transition ONLY if `seq` still matches the active `renderSeq`,
 * guaranteeing that a rapid reopen during exit never gets clobbered by a stale timer.
 */
export function reduceFlyoutHideComplete(state: FlyoutAnimState, seq: number): FlyoutAnimState {
  if (state.phase !== 'closing' || state.renderSeq !== seq) {
    return state;
  }
  return {
    phase: 'closed',
    renderSeq: state.renderSeq,
    req: null,
  };
}

export function isAppContextKind(kind: FlyoutKind): boolean {
  return kind === 'context_app' || kind === 'ctx_app';
}

export function isBarContextKind(kind: FlyoutKind): boolean {
  return kind === 'context_bar' || kind === 'ctx_taskbar';
}

export function removePreviewWindow(windows: OpenWindowInfo[], hwnd: number): OpenWindowInfo[] {
  return windows.filter((w) => w.hwnd !== hwnd);
}

export function syncPreviewWindowsFromGroups(
  groupKey: string | undefined,
  groups: AppWindowGroup[],
): OpenWindowInfo[] | null {
  if (!groupKey) return null;
  const match = groups.find((g) => g.key === groupKey);
  return match ? match.windows : [];
}

export function canOpenFileLocation(target: string | null | undefined): boolean {
  const t = (target ?? '').trim();
  if (!t) return false;
  const low = t.toLowerCase();
  if (low.includes('://') || low.startsWith('shell:') || low.startsWith('ms-')) {
    return false;
  }
  return t.includes(':\\') || t.includes(':/') || t.startsWith('\\\\');
}

export function canRunAsAdmin(target: string | null | undefined): boolean {
  const t = (target ?? '').trim();
  if (!t) return false;
  const low = t.toLowerCase();
  return !(
    low.startsWith('http://') ||
    low.startsWith('https://') ||
    low.startsWith('shell:') ||
    low.startsWith('ms-')
  );
}

export function formatIpcError(err: unknown): string {
  if (typeof err === 'string' && err.trim()) {
    return err.trim();
  }
  if (err instanceof Error && err.message.trim()) {
    return err.message.trim();
  }
  return 'Не удалось выполнить действие';
}
