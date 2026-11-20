import type { AppWindowGroup, AudioDeviceInfo, FlyoutKind, FlyoutRequest, OpenWindowInfo } from './ipc';

export type FlyoutPhase = 'closed' | 'entering' | 'open' | 'closing';

export interface FlyoutAnimState {
  phase: FlyoutPhase;
  renderSeq: number;
  req: FlyoutRequest | null;
}

export const FLYOUT_EXIT_MS = 120;
export const PREVIEW_LEAVE_COMMAND = 'close_preview_if_outside' as const;
export const OSD_AUTO_CLOSE_MS = 1100;

export const TRAY_COLUMNS = 6;
export const TRAY_CARD_WIDTH = 244;
export const TRAY_CELL_SIZE = 34;
export const TRAY_GRID_GAP = 4;
export const TRAY_HEADER_HEIGHT = 24;
export const TRAY_GRID_MAX_HEIGHT = 224;
export const TRAY_MIN_CARD_HEIGHT = 160;

export const INITIAL_FLYOUT_ANIM_STATE: FlyoutAnimState = {
  phase: 'closed',
  renderSeq: 0,
  req: null,
};

/**
 * Transitions flyout into `'entering'` with an incremented `renderSeq` so CSS spring `flyIn`
 * replays cleanly on every open (including rapid reopen cancelling an in-flight exit).
 * Consecutive `'osd'` updates while OSD is already visible update `req` in place without
 * bumping `renderSeq` so wheel ticks glide smoothly instead of re-bouncing.
 */
export function reduceFlyoutShow(state: FlyoutAnimState, req: FlyoutRequest): FlyoutAnimState {
  if (
    req.kind === 'osd' &&
    state.req?.kind === 'osd' &&
    (state.phase === 'entering' || state.phase === 'open')
  ) {
    return {
      ...state,
      req,
    };
  }
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

/**
 * Always returns a non-empty label for the permanently-mounted 24px Tray Overflow header
 * so hovering icons NEVER inserts/removes DOM height or shakes the 6-column grid.
 */
export function formatTrayHeaderLabel(hoveredName: string, totalCount: number): string {
  const trimmed = hoveredName.trim();
  if (trimmed) return trimmed;
  return totalCount > 0 ? `Фоновые · ${totalCount}` : 'Фоновые приложения';
}

/**
 * Computes compact 6-column tray flyout height strictly from item count (completely independent
 * of hover state or right-click overlay), guaranteeing zero vertical jump during interaction.
 */
export function computeTrayFlyoutHeight(appCount: number): number {
  const safeCount = Math.max(0, Math.floor(appCount));
  const rows = Math.max(1, Math.ceil(safeCount / TRAY_COLUMNS));
  const gridHeight = Math.min(
    TRAY_GRID_MAX_HEIGHT,
    rows * TRAY_CELL_SIZE + Math.max(0, rows - 1) * TRAY_GRID_GAP,
  );
  // 8px top + 24px fixed header + 6px gap + grid + 8px bottom = grid + 46px
  return Math.max(TRAY_MIN_CARD_HEIGHT, gridHeight + 46);
}

/**
 * Computes active device index for the spring-animated sliding output indicator pill.
 */
export function getActiveDeviceIndex(devices: Pick<AudioDeviceInfo, 'isDefault'>[]): number {
  const idx = devices.findIndex((d) => d.isDefault);
  return idx >= 0 ? idx : 0;
}

/**
 * Computes next clamped 0..100 volume from a mouse wheel delta (`deltaY < 0` scrolls up).
 */
export function stepVolume(current: number, deltaY: number, step = 4): number {
  const base = Number.isFinite(current) ? Math.round(current) : 0;
  const delta = deltaY < 0 ? step : deltaY > 0 ? -step : 0;
  return Math.max(0, Math.min(100, base + delta));
}
