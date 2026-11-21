import { formatSelectedDateHeader } from './calendarLogic';
import type {
  AppWindowGroup,
  AudioMasterState,
  FlyoutRequest,
  MergedBarItem,
  NetworkInfo,
  OpenWindowInfo,
  PinItem,
} from './ipc';

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

export interface PreviewFlyoutPayload {
  group?: MergedBarItem['group'];
  iconData?: string | null;
  iconKey?: string;
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

export const PREVIEW_HOVER_COLD_MS = 180;
export const PREVIEW_HOVER_WARM_MS = 25;
export const PREVIEW_WARM_WINDOW_MS = 300;
export const PREVIEW_MAX_VISIBLE_CARDS = 4;
export const PREVIEW_CARD_HEIGHT = 172;
export const TASKBAR_LAUNCH_HOP_MS = 880;

export const VOLUME_OSD_WIDTH = 184;
export const VOLUME_OSD_HEIGHT = 42;
export const SOUND_FLYOUT_WIDTH = 336;
export const SOUND_FLYOUT_INITIAL_HEIGHT = 340;
export const CALENDAR_FLYOUT_WIDTH = 310;
export const CALENDAR_FLYOUT_INITIAL_HEIGHT = 408;
export const EDGE_CENTERED_MIN_WIDTH = 1180;
export const DOCK_CHROME_EXTRA_PX = 21; // 16px horizontal padding (8px*2) + 2px border + 3px group gap

const WEEKDAYS_JS_SHORT_RU = ['Вс', 'Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб'] as const;

export function resolveTaskbarMonitor(search: string): TaskbarMonitor {
  const params = new URLSearchParams(search);
  return params.get('monitor') === 'secondary' ? 'secondary' : 'primary';
}

export function getTaskbarRootClassName(barMode?: string | null): string {
  return barMode === 'edge' ? 'tb-root edge' : 'tb-root';
}

/**
 * Applies `.edge-centered` (3-column centered apps layout) strictly on wide primary displays.
 * Secondary or narrow/vertical monitors render a clean left-to-right flex bar so icons never
 * stretch awkwardly across narrow screens.
 */
export function shouldUseEdgeCenteredLayout(
  barMode: string | null | undefined,
  monitor: TaskbarMonitor,
  viewportWidth: number,
): boolean {
  if (barMode !== 'edge') return false;
  if (monitor === 'secondary') return false;
  return Number.isFinite(viewportWidth) ? viewportWidth >= EDGE_CENTERED_MIN_WIDTH : true;
}

export type TaskbarItemStateClass = 'active' | 'running' | 'minimized' | 'idle';

export function getTaskbarItemStateClass(item: Pick<MergedBarItem, 'group'>): TaskbarItemStateClass {
  const windows = item.group?.windows ?? [];
  if (item.group?.isActive) return 'active';
  if (windows.length > 0) {
    if (windows.every((w) => w.isMinimized)) {
      return 'minimized';
    }
    return 'running';
  }
  return 'idle';
}

/**
 * Option A secondary monitor contract: Monitor 2 shows Start + ONLY windows currently on
 * Monitor 2 (never duplicating Monitor 1 `taskbarPins`).
 */
export function selectTaskbarPinsForMonitor(
  pins: PinItem[] | null | undefined,
  monitor: TaskbarMonitor,
): PinItem[] {
  if (monitor === 'secondary') return [];
  return pins ?? [];
}

/**
 * Filters `AppWindowGroup[]` strictly to windows residing on `monitor` when per-window `monitor`
 * metadata is present. Untagged windows (legacy payloads) stay on `'primary'` only.
 */
export function filterGroupsForMonitor(
  groups: readonly AppWindowGroup[],
  monitor: TaskbarMonitor,
): AppWindowGroup[] {
  const result: AppWindowGroup[] = [];
  for (const g of groups) {
    const filteredWins = g.windows.filter((w) => {
      if (!w.monitor) return monitor === 'primary';
      return w.monitor === monitor;
    });
    if (filteredWins.length === 0) continue;
    if (filteredWins.length === g.windows.length) {
      result.push(g);
    } else {
      result.push({
        ...g,
        isActive: filteredWins.some((w) => w.isActive),
        windows: filteredWins,
      });
    }
  }
  return result;
}

/**
 * Computes unconstrained logical dock width from `scrollWidth` and `getBoundingClientRect().width`,
 * guarding against zero/sub-threshold measurements before initial paint while supporting compact
 * secondary monitor docks (Start + Clock = ~130px).
 */
export function computeDockLogicalWidth(scrollWidth: number, rectWidth: number): number | null {
  const safeScroll = Number.isFinite(scrollWidth) ? scrollWidth : 0;
  const safeRect = Number.isFinite(rectWidth) ? rectWidth : 0;
  const measured = Math.ceil(Math.max(safeScroll, safeRect));
  return measured > 96 ? measured : null;
}

/**
 * Computes unconstrained border-box dock width combining the capsule's `scrollWidth` (+2px border),
 * rendered rect width, and unconstrained child group scroll widths so icons never clip into slivers.
 */
export function computeNaturalDockWidth(
  barScrollWidth: number,
  barRectWidth: number,
  appsNaturalWidth = 0,
  trayNaturalWidth = 0,
): number | null {
  const safeBarScroll = Number.isFinite(barScrollWidth) && barScrollWidth > 0 ? barScrollWidth + 2 : 0;
  const safeRect = Number.isFinite(barRectWidth) && barRectWidth > 0 ? barRectWidth : 0;
  const safeApps = Number.isFinite(appsNaturalWidth) && appsNaturalWidth > 0 ? appsNaturalWidth : 0;
  const safeTray = Number.isFinite(trayNaturalWidth) && trayNaturalWidth > 0 ? trayNaturalWidth : 0;
  const groupsTotal = safeApps > 0 && safeTray > 0 ? safeApps + safeTray + DOCK_CHROME_EXTRA_PX : 0;
  return computeDockLogicalWidth(Math.max(safeBarScroll, groupsTotal), safeRect);
}

export interface TaskbarOrderMemory {
  unpinnedIds: string[];
  windowHwndsByGroup: Map<string, number[]>;
}

export function createTaskbarOrderMemory(): TaskbarOrderMemory {
  return {
    unpinnedIds: [],
    windowHwndsByGroup: new Map(),
  };
}

/**
 * Stabilizes unpinned taskbar icons AND per-group window order against Z-order churn on click:
 * - Pinned items always remain first in exact `config.taskbarPins` order.
 * - Unpinned items preserve their first-seen slot order until closed; new apps append at the end.
 * - Multi-window groups preserve first-seen HWND order so preview cards and click-cycling never jump.
 */
export function stabilizeTaskbarItemsOrder(
  items: readonly MergedBarItem[],
  memory: TaskbarOrderMemory,
): MergedBarItem[] {
  const pinned: MergedBarItem[] = [];
  const unpinnedById = new Map<string, MergedBarItem>();
  const activeGroupKeys = new Set<string>();

  const stabilizeItemWindows = (item: MergedBarItem): MergedBarItem => {
    const group = item.group;
    if (!group || group.windows.length === 0) return item;
    activeGroupKeys.add(group.key);

    if (group.windows.length === 1) {
      memory.windowHwndsByGroup.set(group.key, [group.windows[0].hwnd]);
      return item;
    }

    const byHwnd = new Map<number, OpenWindowInfo>();
    for (const w of group.windows) {
      byHwnd.set(w.hwnd, w);
    }

    const prevOrder = memory.windowHwndsByGroup.get(group.key) ?? [];
    const nextOrder: number[] = [];
    const seen = new Set<number>();

    for (const hwnd of prevOrder) {
      if (byHwnd.has(hwnd) && !seen.has(hwnd)) {
        nextOrder.push(hwnd);
        seen.add(hwnd);
      }
    }
    for (const w of group.windows) {
      if (!seen.has(w.hwnd)) {
        nextOrder.push(w.hwnd);
        seen.add(w.hwnd);
      }
    }

    memory.windowHwndsByGroup.set(group.key, nextOrder);
    const orderedWindows = nextOrder.map((h) => byHwnd.get(h)!);
    const unchanged = orderedWindows.every((w, idx) => w === group.windows[idx]);
    if (unchanged) return item;
    return {
      ...item,
      group: {
        ...group,
        windows: orderedWindows,
      },
    };
  };

  for (const raw of items) {
    const item = stabilizeItemWindows(raw);
    if (item.pinned) {
      pinned.push(item);
    } else {
      unpinnedById.set(item.id, item);
    }
  }

  for (const key of Array.from(memory.windowHwndsByGroup.keys())) {
    if (!activeGroupKeys.has(key)) {
      memory.windowHwndsByGroup.delete(key);
    }
  }

  const nextUnpinnedOrder: string[] = [];
  const seenUnpinned = new Set<string>();

  for (const id of memory.unpinnedIds) {
    if (unpinnedById.has(id) && !seenUnpinned.has(id)) {
      nextUnpinnedOrder.push(id);
      seenUnpinned.add(id);
    }
  }
  for (const id of unpinnedById.keys()) {
    if (!seenUnpinned.has(id)) {
      nextUnpinnedOrder.push(id);
      seenUnpinned.add(id);
    }
  }

  memory.unpinnedIds = nextUnpinnedOrder;
  const stableUnpinned = nextUnpinnedOrder.map((id) => unpinnedById.get(id)!);
  return [...pinned, ...stableUnpinned];
}

export function isInteractiveFlyoutActive(activeFlyout: string): boolean {
  return INTERACTIVE_FLYOUT_KINDS.has(activeFlyout);
}

/**
 * Adaptive preview hover delay: `180ms` on cold hover, dropping to `25ms` when scrubbing
 * horizontally while preview is already open or within the `300ms` post-close warm window.
 */
export function getPreviewHoverDelayMs(
  activeFlyout: string,
  lastPreviewCloseMs: number | null,
  nowMs: number,
): number {
  if (activeFlyout === 'preview') {
    return PREVIEW_HOVER_WARM_MS;
  }
  if (
    lastPreviewCloseMs !== null &&
    Number.isFinite(lastPreviewCloseMs) &&
    Number.isFinite(nowMs) &&
    nowMs >= lastPreviewCloseMs &&
    nowMs - lastPreviewCloseMs <= PREVIEW_WARM_WINDOW_MS
  ) {
    return PREVIEW_HOVER_WARM_MS;
  }
  return PREVIEW_HOVER_COLD_MS;
}

/**
 * Selects the window to focus when clicking a running taskbar app:
 * - Single window: focuses or toggles minimize (`toggleMinimize: true`).
 * - Multiple windows: cycles cleanly to the next window after the active one (or first
 *   non-minimized window when unfocused) without stranding windows #2..N.
 */
export function selectWindowToFocusOnTaskbarClick(
  windows: readonly OpenWindowInfo[],
): { hwnd: number; toggleMinimize: boolean } | null {
  if (windows.length === 0) return null;
  if (windows.length === 1) {
    return { hwnd: windows[0].hwnd, toggleMinimize: true };
  }
  const activeIdx = windows.findIndex((w) => w.isActive);
  if (activeIdx >= 0) {
    const next = windows[(activeIdx + 1) % windows.length];
    return { hwnd: next.hwnd, toggleMinimize: false };
  }
  const firstVisible = windows.find((w) => !w.isMinimized) ?? windows[0];
  return { hwnd: firstVisible.hwnd, toggleMinimize: false };
}

/**
 * Maps two taskbar item IDs onto `config.taskbarPins` indices for drag-and-drop pin reordering.
 * Unpinned running apps cannot be reordered into the pinned list.
 */
export function resolvePinnedReorderIndices(
  barItems: readonly Pick<MergedBarItem, 'id' | 'pinned'>[],
  fromId: string,
  toId: string,
): { fromIdx: number; toIdx: number } | null {
  if (!fromId || !toId || fromId === toId) return null;
  let pinIdx = 0;
  let fromIdx = -1;
  let toIdx = -1;
  for (const item of barItems) {
    if (!item.pinned) continue;
    if (item.id === fromId) fromIdx = pinIdx;
    if (item.id === toId) toIdx = pinIdx;
    pinIdx += 1;
  }
  if (fromIdx < 0 || toIdx < 0 || fromIdx === toIdx) return null;
  return { fromIdx, toIdx };
}

export interface TaskbarClockDisplay {
  timeStr: string;
  dateStr: string;
  fullTitle: string;
}

export function formatTaskbarClock(
  date: Date,
  options?: { showSeconds?: boolean; showWeekday?: boolean },
): TaskbarClockDisplay {
  const pad2 = (n: number) => String(n).padStart(2, '0');
  const hh = pad2(date.getHours());
  const mm = pad2(date.getMinutes());
  const ss = pad2(date.getSeconds());
  const timeStr = options?.showSeconds ? `${hh}:${mm}:${ss}` : `${hh}:${mm}`;

  const dd = pad2(date.getDate());
  const mo = pad2(date.getMonth() + 1);
  const yyyy = date.getFullYear();
  const baseDate = `${dd}.${mo}.${yyyy}`;
  const weekday = WEEKDAYS_JS_SHORT_RU[date.getDay()] ?? '';
  const dateStr = options?.showWeekday && weekday ? `${weekday}, ${baseDate}` : baseDate;

  return {
    timeStr,
    dateStr,
    fullTitle: formatSelectedDateHeader(date),
  };
}

export function formatSoundNetworkTooltip(
  audioMaster: AudioMasterState | null,
  net: NetworkInfo | null,
): string {
  const soundPart = audioMaster?.defaultDeviceShortName
    ? `Звук (${audioMaster.defaultDeviceShortName}): ${audioMaster.masterMuted ? 'Выкл' : `${audioMaster.masterVolume}%`}`
    : `Звук: ${audioMaster?.masterMuted ? 'Выкл' : `${audioMaster?.masterVolume ?? 0}%`}`;
  if (!net) return soundPart;
  const netPart = net.connected
    ? net.speedLabel
      ? `${net.kind} · ${net.speedLabel}`
      : net.kind
    : 'Нет сети';
  return `${netPart} · ${soundPart}`;
}

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

/**
 * Computes preview card logical width for `windowCount` open windows (4 visible cards +
 * optional 44px `+N` overflow badge so windows #5+ are never silently hidden).
 */
export function computePreviewCardWidth(windowCount: number): number {
  const safe = Math.max(1, Math.floor(Number.isFinite(windowCount) ? windowCount : 1));
  const visible = Math.min(safe, PREVIEW_MAX_VISIBLE_CARDS);
  const overflowExtra = safe > PREVIEW_MAX_VISIBLE_CARDS ? 53 : 0;
  return visible * 214 + 20 + overflowExtra;
}

export function buildPreviewFlyoutRequest(
  item: MergedBarItem,
  anchorXLogical: number,
  monitor: TaskbarMonitor,
): FlyoutRequest {
  const winCount = item.group?.windows.length ?? 1;
  const payload: PreviewFlyoutPayload = {
    group: item.group,
    iconData: item.iconData ?? null,
    iconKey: item.cacheIconKey || item.iconKey,
  };
  return {
    kind: 'preview',
    anchorXLogical,
    anchorInTaskbar: true,
    width: computePreviewCardWidth(winCount),
    height: PREVIEW_CARD_HEIGHT,
    noActivate: true,
    monitor,
    payload,
  };
}

export type TaskbarClickAction =
  | { type: 'focus'; hwnd: number; toggleMinimize: boolean }
  | { type: 'launch'; target: string; args: string | null };

/**
 * Resolves left-click behavior for a taskbar slot:
 * 1. Running window(s) on this monitor -> focus / cycle / toggle minimize.
 * 2. Pinned app currently running on another monitor (`remoteGroup`) -> focus that running window
 *    without minimizing it (`toggleMinimize: false`) so clicking e.g. Spark on Monitor 1 activates
 *    Spark on Monitor 2 cleanly.
 * 3. Otherwise -> launch `item.target`.
 */
export function resolveTaskbarItemClick(
  item: Pick<MergedBarItem, 'group' | 'remoteGroup' | 'target' | 'args'>,
): TaskbarClickAction | null {
  const localWin = selectWindowToFocusOnTaskbarClick(item.group?.windows ?? []);
  if (localWin) {
    return { type: 'focus', hwnd: localWin.hwnd, toggleMinimize: localWin.toggleMinimize };
  }
  const remoteWins = item.remoteGroup?.windows ?? [];
  if (remoteWins.length > 0) {
    const preferred =
      remoteWins.find((w) => w.isActive) ??
      remoteWins.find((w) => !w.isMinimized) ??
      remoteWins[0];
    return { type: 'focus', hwnd: preferred.hwnd, toggleMinimize: false };
  }
  if (item.target) {
    return { type: 'launch', target: item.target, args: item.args ?? null };
  }
  return null;
}

export function buildAppContextMenuRequest(
  item: MergedBarItem,
  anchorXLogical: number,
  monitor: TaskbarMonitor,
): FlyoutRequest {
  const localWins = item.group?.windows ?? [];
  const windows = localWins.length > 0 ? localWins : (item.remoteGroup?.windows ?? []);
  const hasWins = windows.length > 0;
  const payload: AppContextPayload = {
    id: item.id,
    pinId: item.pinId,
    name: item.name,
    target: item.target,
    args: item.args ?? null,
    matchExe: item.matchExe ?? null,
    aumid: item.group?.aumid ?? item.remoteGroup?.aumid ?? item.matchAumid ?? null,
    iconKey: item.cacheIconKey || item.iconKey,
    pinned: item.pinned,
    windows,
  };
  return {
    kind: 'context_app',
    anchorXLogical,
    anchorInTaskbar: true,
    width: 264,
    height: hasWins ? 320 : 212,
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
