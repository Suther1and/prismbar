import type { FlyoutRequest, MergedBarItem, OpenWindowInfo, PinItem } from './ipc';

export type TaskbarMonitor = 'primary' | 'secondary';

export interface AppContextPayload {
  id: string;
  pinId?: string;
  name: string;
  target: string;
  args?: string | null;
  matchExe?: string | null;
  aumid?: string | null;
  iconKey: string;
  pinned: boolean;
  windows: OpenWindowInfo[];
}

export interface BarContextPayload {
  barMode: 'dock' | 'edge';
}

const INTERACTIVE_FLYOUT_KINDS = new Set([
  'tray',
  'sound',
  'quick',
  'calendar',
  'context_app',
  'context_bar',
  'ctx_app',
  'ctx_taskbar',
]);

export function resolveTaskbarMonitor(search: string): TaskbarMonitor {
  const params = new URLSearchParams(search);
  return params.get('monitor') === 'secondary' ? 'secondary' : 'primary';
}

export function getTaskbarRootClassName(barMode?: string | null): string {
  return barMode === 'edge' ? 'tb-root edge' : 'tb-root';
}

export function getTaskbarItemStateClass(item: Pick<MergedBarItem, 'group'>): 'active' | 'running' | 'idle' {
  const winCount = item.group?.windows.length ?? 0;
  if (item.group?.isActive) return 'active';
  if (winCount > 0) return 'running';
  return 'idle';
}

/**
 * Computes unconstrained logical dock width from `scrollWidth` and `getBoundingClientRect().width`,
 * guarding against zero/sub-threshold measurements before initial paint.
 */
export function computeDockLogicalWidth(scrollWidth: number, rectWidth: number): number | null {
  const safeScroll = Number.isFinite(scrollWidth) ? scrollWidth : 0;
  const safeRect = Number.isFinite(rectWidth) ? rectWidth : 0;
  const measured = Math.ceil(Math.max(safeScroll, safeRect));
  return measured > 200 ? measured : null;
}

export function isInteractiveFlyoutActive(activeFlyout: string): boolean {
  return INTERACTIVE_FLYOUT_KINDS.has(activeFlyout);
}

export const VOLUME_OSD_WIDTH = 176;
export const VOLUME_OSD_HEIGHT = 44;

export interface VolumeOsdPayload {
  volume: number;
  muted: boolean;
}

export interface SoundFeedbackPlan {
  openOsd: boolean;
  volume: number;
  muted: boolean;
  req: FlyoutRequest | null;
}

export function isSoundFlyoutActive(activeFlyout: string): boolean {
  return activeFlyout === 'sound' || activeFlyout === 'quick';
}

/**
 * Per AD-1 preemption: floating Volume OSD must NEVER overwrite an already-open Quick Sound
 * flyout (which updates live in place) or any other interactive click flyout.
 */
export function shouldOpenVolumeOsd(activeFlyout: string): boolean {
  if (isSoundFlyoutActive(activeFlyout)) return false;
  if (isInteractiveFlyoutActive(activeFlyout)) return false;
  return true;
}

export function computeNextWheelVolume(currentVolume: number, deltaY: number, step = 4): number {
  const safeCurrent = Number.isFinite(currentVolume) ? Math.round(currentVolume) : 0;
  const delta = deltaY < 0 ? step : deltaY > 0 ? -step : 0;
  return Math.max(0, Math.min(100, safeCurrent + delta));
}

export function buildVolumeOsdRequest(
  volume: number,
  muted: boolean,
  anchorXLogical: number,
  monitor: TaskbarMonitor,
): FlyoutRequest {
  const clampedVolume = Math.max(0, Math.min(100, Math.round(Number.isFinite(volume) ? volume : 0)));
  const payload: VolumeOsdPayload = {
    volume: clampedVolume,
    muted,
  };
  return {
    kind: 'osd',
    anchorXLogical,
    anchorInTaskbar: true,
    width: VOLUME_OSD_WIDTH,
    height: VOLUME_OSD_HEIGHT,
    noActivate: true,
    monitor,
    payload,
  };
}

export function planSoundFeedbackAction(
  activeFlyout: string,
  nextVolume: number,
  nextMuted: boolean,
  anchorXLogical: number,
  monitor: TaskbarMonitor,
): SoundFeedbackPlan {
  const volume = Math.max(0, Math.min(100, Math.round(Number.isFinite(nextVolume) ? nextVolume : 0)));
  if (!shouldOpenVolumeOsd(activeFlyout)) {
    return {
      openOsd: false,
      volume,
      muted: nextMuted,
      req: null,
    };
  }
  return {
    openOsd: true,
    volume,
    muted: nextMuted,
    req: buildVolumeOsdRequest(volume, nextMuted, anchorXLogical, monitor),
  };
}

/**
 * Hover preview must never trigger for idle pins, while Start is open, or while an
 * interactive click flyout (tray, sound, calendar, context menu) is active (AD-1).
 */
export function shouldTriggerPreviewHover(
  item: Pick<MergedBarItem, 'group'>,
  startOpen: boolean,
  activeFlyout: string,
): boolean {
  if (startOpen) return false;
  if (isInteractiveFlyoutActive(activeFlyout)) return false;
  return Boolean(item.group && item.group.windows.length > 0);
}

export function buildPreviewFlyoutRequest(
  item: MergedBarItem,
  anchorXLogical: number,
  monitor: TaskbarMonitor,
): FlyoutRequest {
  const count = Math.min(Math.max(item.group?.windows.length ?? 1, 1), 4);
  const width = count * 214 + 20;
  return {
    kind: 'preview',
    anchorXLogical,
    anchorInTaskbar: true,
    width,
    height: 166,
    noActivate: true,
    monitor,
    payload: { group: item.group },
  };
}

export function buildAppContextMenuRequest(
  item: MergedBarItem,
  anchorXLogical: number,
  monitor: TaskbarMonitor,
): FlyoutRequest {
  const windows = item.group?.windows ?? [];
  const hasWins = windows.length > 0;
  const payload: AppContextPayload = {
    id: item.id,
    pinId: item.pinId,
    name: item.name,
    target: item.target,
    args: item.args ?? null,
    matchExe: item.matchExe ?? null,
    aumid: item.group?.aumid ?? item.matchAumid ?? null,
    iconKey: item.cacheIconKey || item.iconKey,
    pinned: item.pinned,
    windows,
  };
  return {
    kind: 'context_app',
    anchorXLogical,
    anchorInTaskbar: true,
    width: 262,
    height: hasWins ? 296 : 212,
    noActivate: false,
    monitor,
    payload,
  };
}

export function buildBarContextMenuRequest(
  barMode: string | undefined,
  anchorXLogical: number,
  monitor: TaskbarMonitor,
): FlyoutRequest {
  const normalizedMode: 'dock' | 'edge' = barMode === 'edge' ? 'edge' : 'dock';
  const payload: BarContextPayload = { barMode: normalizedMode };
  return {
    kind: 'context_bar',
    anchorXLogical,
    anchorInTaskbar: true,
    width: 256,
    height: 204,
    noActivate: false,
    monitor,
    payload,
  };
}

/**
 * Constructs a canonical `PinItem` when pinning an app from the taskbar right-click menu,
 * preserving `matchExe`, `matchAumid`, `iconKey`, and `args` for clean deduplication (AD-4).
 */
export function buildPinFromAppContext(payload: AppContextPayload, id = `tb-${Date.now()}`): PinItem {
  const resolvedTarget =
    payload.target || (payload.aumid ? `shell:AppsFolder\\${payload.aumid}` : '');
  const trimmedName = payload.name.trim() || 'Приложение';
  return {
    id,
    kind: 'app',
    title: trimmedName,
    subtitle: 'Приложение',
    target: resolvedTarget,
    args: payload.args ?? null,
    altAction: 'Новое окно',
    accentRgb: '129,140,248',
    iconKey: payload.iconKey || trimmedName.toLowerCase(),
    iconData: null,
    matchExe: payload.matchExe ?? null,
    matchAumid: payload.aumid ?? null,
  };
}
