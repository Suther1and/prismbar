import type {
  AppWindowGroup,
  AudioDeviceInfo,
  FlyoutKind,
  FlyoutRequest,
  MediaState,
  OpenWindowInfo,
  TrayAppItem,
} from './ipc';

export type FlyoutPhase = 'closed' | 'entering' | 'open' | 'closing';

export interface FlyoutAnimState {
  phase: FlyoutPhase;
  renderSeq: number;
  scrubSeq: number;
  scrubDir: -1 | 0 | 1;
  req: FlyoutRequest | null;
}

export const FLYOUT_EXIT_MS = 110;
export const CONTEXT_FLYOUT_EXIT_MS = 95;
export const PREVIEW_LEAVE_COMMAND = 'close_preview_if_outside' as const;
export const OSD_AUTO_CLOSE_MS = 1100;
export const PREVIEW_CARD_CLOSE_MS = 105;
export const AUDIO_PEAK_POLL_MS = 130;
export const FLYOUT_ROOT_PAD_LOGICAL = 18;

export const TRAY_COLUMNS = 6;
export const TRAY_CARD_WIDTH = 248;
export const TRAY_CELL_SIZE = 34;
export const TRAY_GRID_GAP = 4;
export const TRAY_HEADER_HEIGHT = 24;
export const TRAY_GRID_MAX_HEIGHT = 224;
export const TRAY_MIN_CARD_HEIGHT = 204;

export const INITIAL_FLYOUT_ANIM_STATE: FlyoutAnimState = {
  phase: 'closed',
  renderSeq: 0,
  scrubSeq: 0,
  scrubDir: 0,
  req: null,
};

export function getFlyoutExitMs(kind: FlyoutKind | undefined): number {
  if (kind && (isAppContextKind(kind) || isBarContextKind(kind))) {
    return CONTEXT_FLYOUT_EXIT_MS;
  }
  return FLYOUT_EXIT_MS;
}

function extractPreviewGroupKey(req: FlyoutRequest | null): string {
  if (!req || req.kind !== 'preview') return '';
  const payload = req.payload as { group?: { key?: string } } | null | undefined;
  return payload?.group?.key ?? '';
}

function computeScrubDirection(prevAnchorX: number, nextAnchorX: number): -1 | 0 | 1 {
  if (!Number.isFinite(prevAnchorX) || !Number.isFinite(nextAnchorX)) return 0;
  const delta = nextAnchorX - prevAnchorX;
  if (delta > 1) return 1;
  if (delta < -1) return -1;
  return 0;
}

/**
 * Transitions flyout into `'entering'` with an incremented `renderSeq` so enter choreography
 * plays cleanly on every cold open or kind switch (including rapid reopen cancelling an exit).
 * - Consecutive `'osd'` updates update in place without bumping `renderSeq`.
 * - Consecutive `'preview'` scrubs keep `.flyout-card` mounted (no vertical re-bounce!),
 *   bumping `scrubSeq` with directional `scrubDir` when moving across taskbar apps.
 */
export function reduceFlyoutShow(state: FlyoutAnimState, req: FlyoutRequest): FlyoutAnimState {
  const isLive = state.phase === 'entering' || state.phase === 'open';

  if (req.kind === 'osd' && state.req?.kind === 'osd' && isLive) {
    return {
      ...state,
      req,
    };
  }

  if (req.kind === 'preview' && state.req?.kind === 'preview' && isLive) {
    const prevKey = extractPreviewGroupKey(state.req);
    const nextKey = extractPreviewGroupKey(req);
    const dir = computeScrubDirection(state.req.anchorXLogical, req.anchorXLogical);
    const changedGroup = Boolean(nextKey) && nextKey !== prevKey;
    return {
      ...state,
      req,
      scrubSeq: changedGroup ? state.scrubSeq + 1 : state.scrubSeq,
      scrubDir: changedGroup ? dir : state.scrubDir,
    };
  }

  return {
    phase: 'entering',
    renderSeq: state.renderSeq + 1,
    scrubSeq: 0,
    scrubDir: 0,
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
 * Begins the crisp exit animation while keeping `req` mounted so exit transitions visibly.
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
    scrubSeq: 0,
    scrubDir: 0,
    req: null,
  };
}

/**
 * Immediately clears the flyout to `'closed'` (zero stale frame on preemption / fullscreen).
 */
export function reduceFlyoutHideImmediate(state: FlyoutAnimState): FlyoutAnimState {
  return {
    phase: 'closed',
    renderSeq: state.renderSeq,
    scrubSeq: 0,
    scrubDir: 0,
    req: null,
  };
}

/**
 * Converts Rust's window-relative `anchorOriginX` (which includes `.flyout-root`'s 18px left pad)
 * into a clamped card-local `transform-origin` X coordinate in logical px.
 */
export function computeCardOriginX(
  anchorOriginX: number | null | undefined,
  cardWidth: number,
  rootPad = FLYOUT_ROOT_PAD_LOGICAL,
): number | null {
  if (typeof anchorOriginX !== 'number' || !Number.isFinite(anchorOriginX)) {
    return null;
  }
  const safeW = Math.max(28, Number.isFinite(cardWidth) ? cardWidth : 240);
  const local = anchorOriginX - rootPad;
  const edge = Math.min(16, safeW / 2);
  return Math.round(Math.max(edge, Math.min(safeW - edge, local)) * 10) / 10;
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
 * Resolves a concise, stable hide key for `hide_tray_app` matching Rust `filter_hidden_tray_apps`.
 */
export function resolveTrayHideKey(item: Pick<TrayAppItem, 'id' | 'name' | 'exePath'>): string {
  const normExe = (item.exePath ?? '').trim().replace(/\//g, '\\').toLowerCase();
  if (normExe) {
    const idx = normExe.lastIndexOf('\\');
    const base = idx >= 0 ? normExe.slice(idx + 1) : normExe;
    if (base) return base;
  }
  const id = (item.id ?? '').trim().toLowerCase();
  if (id) return id;
  return (item.name ?? '').trim().toLowerCase();
}

/**
 * Computes active device index for the spring-animated sliding output indicator pill.
 * Kept exported for StartSpaces cross-file compatibility.
 */
export function getActiveDeviceIndex(devices: Pick<AudioDeviceInfo, 'isDefault'>[]): number {
  const idx = devices.findIndex((d) => d.isDefault);
  return idx >= 0 ? idx : 0;
}

export interface PartitionedAudioDevices {
  primary: AudioDeviceInfo[];
  overflow: AudioDeviceInfo[];
  activeInPrimaryIdx: number;
  activeInOverflow: boolean;
}

/**
 * Splits audio output devices into the top `maxPills` sliding pill slots plus overflow dropdown
 * entries so users with 4+ endpoints can switch any endpoint in one click.
 */
export function partitionAudioDevices(
  devices: readonly AudioDeviceInfo[],
  maxPills = 3,
): PartitionedAudioDevices {
  const limit = Math.max(1, Math.floor(maxPills));
  const primary = devices.slice(0, limit);
  const overflow = devices.slice(limit);
  const activeInPrimaryIdx = primary.findIndex((d) => d.isDefault);
  const activeInOverflow = overflow.some((d) => d.isDefault);
  return {
    primary,
    overflow,
    activeInPrimaryIdx,
    activeInOverflow,
  };
}

/**
 * Computes next clamped 0..100 volume from a mouse wheel delta (`deltaY < 0` scrolls up).
 */
export function stepVolume(current: number, deltaY: number, step = 4): number {
  const base = Number.isFinite(current) ? Math.round(current) : 0;
  const delta = deltaY < 0 ? step : deltaY > 0 ? -step : 0;
  return Math.max(0, Math.min(100, base + delta));
}

/**
 * Interpolates live SMTC track position between Rust polls while playing, clamped to `[0, durationSec]`.
 */
export function interpolateMediaPosition(
  media: Pick<MediaState, 'isPlaying' | 'positionSec' | 'durationSec' | 'lastUpdatedMs'> | null,
  nowMs: number,
): number {
  if (!media) return 0;
  const base = Number.isFinite(media.positionSec) ? Math.max(0, media.positionSec) : 0;
  const duration = Number.isFinite(media.durationSec) ? Math.max(0, media.durationSec) : 0;
  if (!media.isPlaying || !media.lastUpdatedMs || !Number.isFinite(nowMs)) {
    return duration > 0 ? Math.min(duration, base) : base;
  }
  const elapsedSec = Math.max(0, (nowMs - media.lastUpdatedMs) / 1000);
  const next = base + elapsedSec;
  return duration > 0 ? Math.min(duration, next) : next;
}

export function formatMediaTime(seconds: number): string {
  const s = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  const mins = Math.floor(s / 60);
  const rem = s % 60;
  return `${mins}:${String(rem).padStart(2, '0')}`;
}

/**
 * Collects unique positive PIDs across all open windows in an app group for `kill_process_group`.
 */
export function collectAppContextPids(windows: readonly Pick<OpenWindowInfo, 'pid'>[]): number[] {
  const seen = new Set<number>();
  const out: number[] = [];
  for (const w of windows) {
    if (Number.isInteger(w.pid) && w.pid > 0 && !seen.has(w.pid)) {
      seen.add(w.pid);
      out.push(w.pid);
    }
  }
  return out;
}

/**
 * Guards against a dangling top divider in the taskbar app context menu when no launch/admin/
 * location/jump-list items precede the Pin section.
 */
export function shouldShowAppContextActionSeparator(params: {
  hasJumpList: boolean;
  hasLaunch: boolean;
  hasAdmin: boolean;
  hasLocation: boolean;
  hasPinSection: boolean;
}): boolean {
  const hasTopActions = params.hasJumpList || params.hasLaunch || params.hasAdmin || params.hasLocation;
  return hasTopActions && params.hasPinSection;
}

/**
 * Circular keyboard navigation index step for context menus (`ArrowUp`/`ArrowDown`).
 */
export function computeNextContextFocusIdx(
  currentIdx: number,
  delta: 1 | -1,
  itemCount: number,
): number {
  if (itemCount <= 0) return -1;
  if (currentIdx < 0 || currentIdx >= itemCount) {
    return delta > 0 ? 0 : itemCount - 1;
  }
  return (currentIdx + delta + itemCount) % itemCount;
}
