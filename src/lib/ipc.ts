import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

export interface PinItem {
  id: string;
  kind: 'app' | 'folder' | 'file' | 'url' | 'combo';
  title: string;
  subtitle: string;
  target: string;
  args?: string | null;
  altAction: string;
  altLabel?: string | null;
  accentRgb: string;
  iconKey: string;
  iconData?: string | null;
  matchExe?: string | null;
  matchAumid?: string | null;
  comboTargets?: string[] | null;
}

export type WidgetPreset =
  | 'media_clipboard'
  | 'telemetry_processes'
  | 'audio_mic'
  | 'full_grid';

export interface CustomSpace {
  id: string;
  name: string;
  accentHex: string;
  accentRgb: string;
  widgetPreset?: WidgetPreset;
  items: PinItem[];
}

export type SecondaryTaskbarMode = 'testbar' | 'native' | 'hidden';
export type GlassDepth = 'crystal' | 'obsidian' | 'matte';
export type BackdropMode = 'dcomp' | 'swca' | 'none';
export type SnapZone = 'left' | 'right' | 'maximize' | 'restore' | 'center';

export interface FrecencyEntry {
  count: number;
  lastUsedMs: number;
}

export interface TestBarConfig {
  schemaVersion: number;
  revision: number;
  barMode: 'dock' | 'edge';
  secondaryTaskbarMode: SecondaryTaskbarMode;
  autostart: boolean;
  glassDepth?: GlassDepth;
  clockShowSeconds?: boolean;
  clockShowWeekday?: boolean;
  startRememberSpace?: boolean;
  hiddenTrayKeys?: string[];
  frecency?: Record<string, FrecencyEntry>;
  taskbarPins: PinItem[];
  personalPins: PinItem[];
  personalLinks: PinItem[];
  workFolders: PinItem[];
  workApps: PinItem[];
  studioPins: PinItem[];
  studioTools: PinItem[];
  gameLaunchers: PinItem[];
  gameCombos: PinItem[];
  customSpaces: CustomSpace[];
  pinnedClipboard: string[];
}

export interface OpenWindowInfo {
  hwnd: number;
  pid: number;
  title: string;
  exePath: string;
  exeName: string;
  aumid?: string | null;
  isActive: boolean;
  isMinimized: boolean;
  isTopmost?: boolean;
  monitor?: 'primary' | 'secondary' | string | null;
}

export interface AppWindowGroup {
  key: string;
  name: string;
  exePath: string;
  aumid?: string | null;
  iconKey?: string | null;
  iconData?: string | null;
  isActive: boolean;
  windows: OpenWindowInfo[];
}

export interface IndexedApp {
  id: string;
  name: string;
  category: string;
  target: string;
  aumid?: string | null;
  iconKey?: string | null;
  iconData?: string | null;
}

export interface IndexedFile {
  id: string;
  name: string;
  kind: 'file' | 'folder';
  location: string;
  target: string;
  modifiedTs: number;
}

export interface SteamGame {
  appId: string;
  name: string;
  launcher: string;
  target: string;
  coverData?: string | null;
  coverPath?: string | null;
  lastUpdated: number;
}

export interface AudioDeviceInfo {
  id: string;
  name: string;
  shortName: string;
  isDefault: boolean;
}

export interface AudioSessionInfo {
  id: string;
  pid: number;
  pids: number[];
  name: string;
  volume: number;
  muted: boolean;
  isActive: boolean;
  peak?: number;
  iconKey?: string | null;
}

export interface AudioMasterState {
  available: boolean;
  masterVolume: number;
  masterMuted: boolean;
  micMuted: boolean;
  defaultDeviceShortName: string;
}

export interface AudioState {
  available: boolean;
  masterVolume: number;
  masterMuted: boolean;
  micMuted: boolean;
  masterPeak?: number;
  micPeak?: number;
  devices: AudioDeviceInfo[];
  inputDevices?: AudioDeviceInfo[];
  sessions: AudioSessionInfo[];
}

export interface AudioPeaksSnapshot {
  masterPeak: number;
  micPeak: number;
  sessionPeaks: Array<[string, number]>;
}

export interface MediaSessionSummary {
  id: string;
  appName: string;
  title: string;
  artist: string;
  isPlaying: boolean;
  isCurrent: boolean;
}

export interface MediaState {
  hasSession: boolean;
  title: string;
  artist: string;
  source: string;
  isPlaying: boolean;
  positionSec: number;
  durationSec: number;
  canSeek?: boolean;
  lastUpdatedMs?: number;
  thumbnail?: string | null;
  sessions?: MediaSessionSummary[];
  activeSessionId?: string;
}

export interface DiskHeadroomInfo {
  mount: string;
  name: string;
  usedGb: number;
  totalGb: number;
  freeGb: number;
  usedPct: number;
}

export interface TopProcessInfo {
  pid: number;
  pids: number[];
  name: string;
  memGb: number;
  vramGb: number;
  usesGpu: boolean;
  cpuPct: number;
  iconKey?: string | null;
  hwnd?: number | null;
  iconData?: string | null;
}

export interface TelemetryState {
  cpuName: string;
  cpuPct: number;
  ramUsedGb: number;
  ramTotalGb: number;
  ramPct: number;
  gpuAvailable: boolean;
  gpuName: string;
  gpuPct: number;
  gpuTempC: number;
  vramUsedGb: number;
  vramTotalGb: number;
  vramPct: number;
  topProcesses: TopProcessInfo[];
  gpuProcesses: TopProcessInfo[];
  disks?: DiskHeadroomInfo[];
}

export type ClipboardKind = 'url' | 'path' | 'hex' | 'code' | 'text';

export interface ClipboardEntry {
  id: string;
  tag: string;
  text: string;
  preview?: string;
  lineCount?: number;
  charCount?: number;
  kind?: ClipboardKind | string;
  color?: string | null;
  pinned: boolean;
}

export interface PeripheralBattery {
  name: string;
  kind: 'mouse' | 'audio' | 'other';
  percent: number;
}

export interface TrayAppItem {
  id: string;
  hwnd?: number | null;
  pid: number;
  allPids: number[];
  name: string;
  exePath: string;
  iconKey?: string | null;
  iconData?: string | null;
}

export interface SessionAlert {
  id: string;
  level: 'info' | 'warn' | 'error';
  title: string;
  detail?: string | null;
  timestampMs: number;
}

export interface NetworkInfo {
  connected: boolean;
  kind: string;
  speedLabel: string;
}

export type FlyoutKind =
  | 'preview'
  | 'tray'
  | 'sound'
  | 'calendar'
  | 'context_app'
  | 'context_bar'
  | 'osd'
  | 'quick'
  | 'ctx_app'
  | 'ctx_taskbar';

export interface JumpListItem {
  title: string;
  target: string;
  args?: string | null;
  kind: 'task' | 'recent';
}

export interface FlyoutRequest {
  kind: FlyoutKind;
  anchorXLogical: number;
  anchorInTaskbar: boolean;
  width: number;
  height: number;
  noActivate: boolean;
  monitor?: 'primary' | 'secondary';
  payload: unknown;
  gen?: number;
  anchorOriginX?: number | null;
}

export interface StartStateEvent {
  open: boolean;
  gen: number;
  monitor?: 'primary' | 'secondary' | string | null;
  immediate?: boolean;
}

export interface StartShownEvent {
  open: boolean;
  gen: number;
  monitor?: 'primary' | 'secondary' | string | null;
}

export interface FlyoutHideEvent {
  gen: number;
}

export function normalizeStartStatePayload(payload: boolean | StartStateEvent): StartStateEvent {
  if (typeof payload === 'boolean') {
    return { open: payload, gen: 0, monitor: null, immediate: false };
  }
  return {
    open: Boolean(payload?.open),
    gen: typeof payload?.gen === 'number' && Number.isFinite(payload.gen) ? payload.gen : 0,
    monitor: payload?.monitor ?? null,
    immediate: Boolean(payload?.immediate),
  };
}

export interface MergedBarItem {
  id: string;
  name: string;
  target: string;
  args?: string | null;
  matchExe?: string | null;
  matchAumid?: string | null;
  iconKey: string;
  cacheIconKey?: string | null;
  iconData?: string | null;
  pinned: boolean;
  pinId?: string;
  group?: AppWindowGroup;
  /** Matched running window group residing on another monitor (used to focus & resolve icons without duplicating unpinned slots). */
  remoteGroup?: AppWindowGroup;
}

export const ICON_CACHE_MAX_ENTRIES = 384;

export class LruIconCache extends Map<string, string> {
  readonly maxEntries: number;

  constructor(maxEntries: number = ICON_CACHE_MAX_ENTRIES) {
    super();
    this.maxEntries = Math.max(1, Math.floor(maxEntries));
  }

  override get(key: string): string | undefined {
    const val = super.get(key);
    if (val !== undefined) {
      super.delete(key);
      super.set(key, val);
    }
    return val;
  }

  override set(key: string, value: string): this {
    if (super.has(key)) {
      super.delete(key);
    }
    super.set(key, value);
    while (this.size > this.maxEntries) {
      const oldest = this.keys().next().value;
      if (oldest === undefined) break;
      super.delete(oldest);
    }
    return this;
  }
}

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const iconCache: Map<string, string> = new LruIconCache(ICON_CACHE_MAX_ENTRIES);
const inFlightIconRequests = new Map<string, Promise<string | undefined>>();

export function isCacheIconKey(key: string | null | undefined): key is string {
  if (!key) return false;
  return (
    key.startsWith('exe:') ||
    key.startsWith('aumid:') ||
    key.startsWith('hwnd:') ||
    key.startsWith('file:') ||
    key.startsWith('tray:')
  );
}

export function clearIconCache(): void {
  iconCache.clear();
  inFlightIconRequests.clear();
}

export type IconBatchFetcher = (keys: string[]) => Promise<Record<string, string>>;

const defaultIconFetcher: IconBatchFetcher = (keys) => {
  if (!isTauri() || keys.length === 0) {
    return Promise.resolve({});
  }
  return ipc<Record<string, string>>('get_icons', { keys });
};

/**
 * Fetches icon PNG data URIs in batches via `get_icons`, deduplicating both already-cached
 * entries in `iconCache` and concurrent in-flight requests across overlapping calls.
 */
export async function fetchIconsBatch(
  keys: string[],
  fetcher: IconBatchFetcher = defaultIconFetcher,
): Promise<Record<string, string>> {
  const uniqueKeys: string[] = [];
  const seen = new Set<string>();
  for (const raw of keys) {
    const trimmed = raw.trim();
    if (!trimmed || seen.has(trimmed)) continue;
    seen.add(trimmed);
    uniqueKeys.push(trimmed);
  }

  const result: Record<string, string> = {};
  if (uniqueKeys.length === 0) {
    return result;
  }

  const toFetch: string[] = [];
  const pendingWaiters: Array<[string, Promise<string | undefined>]> = [];

  for (const key of uniqueKeys) {
    const cached = iconCache.get(key);
    if (cached !== undefined) {
      result[key] = cached;
      continue;
    }
    const inflight = inFlightIconRequests.get(key);
    if (inflight) {
      pendingWaiters.push([key, inflight]);
    } else {
      toFetch.push(key);
    }
  }

  if (toFetch.length > 0) {
    let rawPromise: Promise<Record<string, string>>;
    try {
      rawPromise = Promise.resolve(fetcher(toFetch));
    } catch {
      rawPromise = Promise.resolve({});
    }
    const batchPromise = rawPromise
      .then((fetched) => fetched ?? {})
      .catch(() => ({} as Record<string, string>));

    for (const key of toFetch) {
      const keyPromise = batchPromise
        .then((map) => {
          const uri = map[key];
          if (typeof uri === 'string' && uri.length > 0) {
            iconCache.set(key, uri);
            return uri;
          }
          return undefined;
        })
        .finally(() => {
          if (inFlightIconRequests.get(key) === keyPromise) {
            inFlightIconRequests.delete(key);
          }
        });
      inFlightIconRequests.set(key, keyPromise);
      pendingWaiters.push([key, keyPromise]);
    }
  }

  if (pendingWaiters.length > 0) {
    const settled = await Promise.all(
      pendingWaiters.map(async ([key, promise]) => [key, await promise] as const),
    );
    for (const [key, uri] of settled) {
      if (uri) {
        result[key] = uri;
      }
    }
  }

  return result;
}

function isValidRevision(rev: unknown): rev is number {
  return typeof rev === 'number' && Number.isSafeInteger(rev) && rev >= 0;
}

export function shouldApplyConfigUpdate(
  prev: Pick<TestBarConfig, 'revision'> | null | undefined,
  next: Pick<TestBarConfig, 'revision'> | null | undefined,
): boolean {
  if (!next || !isValidRevision(next.revision)) {
    return false;
  }
  if (!prev || !isValidRevision(prev.revision)) {
    return true;
  }
  return next.revision > prev.revision;
}

export function applyConfigUpdate(
  prev: TestBarConfig | null,
  next: TestBarConfig | null | undefined,
): TestBarConfig | null {
  if (!next || !shouldApplyConfigUpdate(prev, next)) {
    return prev;
  }
  return next;
}

function normalizePath(p: string | null | undefined): string {
  if (!p) return '';
  return p.trim().replace(/\//g, '\\').toLowerCase();
}

function extractBasename(p: string | null | undefined): string {
  const norm = normalizePath(p);
  if (!norm || norm.endsWith('\\')) return '';
  const idx = norm.lastIndexOf('\\');
  return idx >= 0 ? norm.slice(idx + 1) : norm;
}

function normalizeAumid(raw: string | null | undefined): string {
  if (!raw) return '';
  const trimmed = raw.trim().toLowerCase();
  const prefix = 'shell:appsfolder\\';
  return trimmed.startsWith(prefix) ? trimmed.slice(prefix.length).trim() : trimmed;
}

function extractAumidFromTarget(target: string | null | undefined): string {
  if (!target) return '';
  const trimmed = target.trim().toLowerCase();
  const prefix = 'shell:appsfolder\\';
  return trimmed.startsWith(prefix) ? trimmed.slice(prefix.length).trim() : '';
}

function resolveCachedIcon(
  iconMap: Record<string, string>,
  ...keys: Array<string | null | undefined>
): string | null {
  for (const key of keys) {
    if (!key) continue;
    const fromMap = iconMap[key];
    if (fromMap) return fromMap;
    const fromCache = iconCache.get(key);
    if (fromCache) return fromCache;
  }
  return null;
}

function extractBrandLeadToken(raw: string | null | undefined): string {
  if (!raw) return '';
  const cleaned = raw
    .trim()
    .toLowerCase()
    .replace(/\.exe$/i, '')
    .split(/[\s\-_.:/\\]+/)
    .find((tok) => tok.length >= 4);
  return cleaned ?? '';
}

export function findMatchingGroupForPin(
  p: PinItem,
  groups: readonly AppWindowGroup[],
  excludeKeys?: ReadonlySet<string>,
): AppWindowGroup | undefined {
  const pinAumid = normalizeAumid(p.matchAumid) || extractAumidFromTarget(p.target);
  const matchExeBase = extractBasename(p.matchExe);
  const targetNorm = normalizePath(p.target);
  const targetBase = extractBasename(p.target);
  const titleLow = p.title.trim().toLowerCase();

  const available = excludeKeys
    ? groups.filter((g) => !excludeKeys.has(g.key))
    : groups;

  // Priority 1: Exact case-insensitive UWP AUMID match
  let matchedGroup = pinAumid
    ? available.find((g) => {
        const groupAumid = normalizeAumid(g.aumid);
        return Boolean(groupAumid) && groupAumid === pinAumid;
      })
    : undefined;

  // Priority 2: Exact case-insensitive Win32 matchExe === basename(group.exePath)
  if (!matchedGroup && matchExeBase) {
    matchedGroup = available.find((g) => {
      const groupBase = extractBasename(g.exePath);
      return Boolean(groupBase) && groupBase === matchExeBase;
    });
  }

  // Priority 3: Normalized target path / basename fallback
  if (!matchedGroup && (targetNorm || targetBase)) {
    matchedGroup = available.find((g) => {
      const groupKeyNorm = normalizePath(g.key);
      const groupExeNorm = normalizePath(g.exePath);
      const groupBase = extractBasename(g.exePath);
      return (
        (Boolean(targetNorm) && (groupKeyNorm === targetNorm || groupExeNorm === targetNorm)) ||
        (Boolean(targetBase) && Boolean(groupBase) && groupBase === targetBase)
      );
    });
  }

  // Priority 4: Exact case-insensitive display title fallback
  if (!matchedGroup && titleLow) {
    matchedGroup = available.find((g) => {
      const groupNameLow = g.name.trim().toLowerCase();
      return Boolean(groupNameLow) && groupNameLow === titleLow;
    });
  }

  // Priority 5: Safe lead-brand token fallback for broken/empty-target pins (e.g. "Spark Mail" <-> "Spark Desktop")
  if (!matchedGroup && !pinAumid && !matchExeBase && !targetNorm) {
    const lead = extractBrandLeadToken(p.title) || extractBrandLeadToken(p.iconKey);
    if (lead) {
      matchedGroup = available.find((g) => {
        const gLead = extractBrandLeadToken(g.name);
        const exeLead = extractBrandLeadToken(extractBasename(g.exePath));
        return gLead === lead || exeLead === lead;
      });
    }
  }

  return matchedGroup;
}

export function mergeTaskbarItems(
  pins: PinItem[],
  groups: AppWindowGroup[],
  iconMap: Record<string, string>,
  allGroups?: AppWindowGroup[],
): MergedBarItem[] {
  const usedGroupKeys = new Set<string>();
  const merged: MergedBarItem[] = [];

  for (const p of pins) {
    const matchedGroup = findMatchingGroupForPin(p, groups, usedGroupKeys);

    if (matchedGroup) {
      usedGroupKeys.add(matchedGroup.key);
    }

    // If pinned app is running on another monitor (e.g. Monitor 2 while pinned on Monitor 1),
    // bind `remoteGroup` for icon/target resolution and click-to-focus without duplicating slots.
    const remoteGroup =
      !matchedGroup && allGroups && allGroups.length > 0
        ? findMatchingGroupForPin(p, allGroups)
        : undefined;
    const effectiveGroup = matchedGroup ?? remoteGroup;

    const cacheKey = effectiveGroup?.iconKey ?? (isCacheIconKey(p.iconKey) ? p.iconKey : null);
    const resolvedIconData =
      resolveCachedIcon(
        iconMap,
        effectiveGroup?.iconKey,
        isCacheIconKey(p.iconKey) ? p.iconKey : null,
      ) ||
      effectiveGroup?.iconData ||
      p.iconData ||
      iconMap[p.target] ||
      (p.target ? iconMap[p.target.toLowerCase()] : undefined) ||
      null;

    merged.push({
      id: p.id,
      name: p.title,
      target:
        p.target ||
        effectiveGroup?.exePath ||
        (effectiveGroup?.aumid ? `shell:AppsFolder\\${effectiveGroup.aumid}` : ''),
      args: p.args ?? null,
      matchExe:
        p.matchExe ??
        (effectiveGroup?.exePath ? extractBasename(effectiveGroup.exePath) || null : null),
      matchAumid: p.matchAumid ?? effectiveGroup?.aumid ?? null,
      iconKey: p.iconKey,
      cacheIconKey: cacheKey,
      iconData: resolvedIconData,
      pinned: true,
      pinId: p.id,
      group: matchedGroup,
      remoteGroup,
    });
  }

  for (const g of groups) {
    if (!usedGroupKeys.has(g.key)) {
      const resolvedIconData = resolveCachedIcon(iconMap, g.iconKey) || g.iconData || null;
      const baseExe = extractBasename(g.exePath);
      merged.push({
        id: 'run-' + g.key,
        name: g.name,
        target: g.exePath || (g.aumid ? `shell:AppsFolder\\${g.aumid}` : ''),
        args: null,
        matchExe: baseExe && baseExe.endsWith('.exe') ? baseExe : null,
        matchAumid: g.aumid ?? null,
        iconKey: g.iconKey || g.name.toLowerCase(),
        cacheIconKey: g.iconKey ?? null,
        iconData: resolvedIconData,
        pinned: false,
        group: g,
      });
    }
  }

  return merged;
}

export async function ipc<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

export async function onEvent<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  try {
    const win = getCurrentWebviewWindow();
    if (win && typeof win.listen === 'function') {
      return await win.listen<T>(event, (e) => cb(e.payload));
    }
  } catch {
    // Fallback for test/browser environments without a Tauri WebviewWindow handle
  }
  return listen<T>(event, (e) => cb(e.payload));
}

// ======================= TYPED IPC COMMAND WRAPPERS =======================

export function surfaceCommitAck(
  surface: 'start' | 'flyout',
  gen: number,
  measuredHeight?: number,
): Promise<void> {
  return ipc<void>('surface_commit_ack', {
    surface,
    gen,
    measuredHeight: measuredHeight ?? null,
  });
}

export function surfaceExitAck(surface: 'start' | 'flyout', gen: number): Promise<void> {
  return ipc<void>('surface_exit_ack', { surface, gen });
}

export function animateSurfaceIn(
  surface: 'start' | 'flyout',
  gen: number,
  opts?: { durationMs?: number; offsetY?: number; fromScale?: number },
): Promise<void> {
  return ipc<void>('animate_surface_in', {
    surface,
    gen,
    durationMs: opts?.durationMs ?? 240,
    offsetY: opts?.offsetY ?? 20,
    fromScale: opts?.fromScale ?? 0.96,
  });
}

export function animateSurfaceOut(
  surface: 'start' | 'flyout',
  gen: number,
  opts?: { durationMs?: number; offsetY?: number; toScale?: number },
): Promise<void> {
  return ipc<void>('animate_surface_out', {
    surface,
    gen,
    durationMs: opts?.durationMs ?? 120,
    offsetY: opts?.offsetY ?? 12,
    toScale: opts?.toScale ?? 0.975,
  });
}

export function getBackdropMode(): Promise<BackdropMode> {
  return ipc<BackdropMode>('get_backdrop_mode');
}

export function getOpenWindows(
  monitor?: 'primary' | 'secondary',
): Promise<AppWindowGroup[]> {
  return ipc<AppWindowGroup[]>(
    'get_open_windows',
    monitor ? { monitor } : undefined,
  );
}

export function dismissOverlays(): Promise<void> {
  return ipc<void>('dismiss_overlays');
}

export function toggleStart(
  force?: boolean,
  monitor?: 'primary' | 'secondary',
): Promise<void> {
  return ipc<void>('toggle_start', {
    force: force ?? null,
    monitor: monitor ?? null,
  });
}

export function getAudioPeaks(): Promise<AudioPeaksSnapshot> {
  return ipc<AudioPeaksSnapshot>('get_audio_peaks');
}

export function setInputDevice(deviceId: string): Promise<AudioState> {
  return ipc<AudioState>('set_input_device', { deviceId });
}

export function setSessionState(params: {
  appKey: string;
  /** Normalized WASAPI volume scalar in `0.0..=1.0` (e.g. `0.01` = 1%, `1.0` = 100%). */
  volume?: number;
  muted?: boolean;
  pid?: number;
  pids?: number[];
}): Promise<AudioState> {
  return ipc<AudioState>('set_session_state', {
    appKey: params.appKey,
    volume: params.volume ?? null,
    muted: params.muted ?? null,
    pid: params.pid ?? null,
    pids: params.pids ?? null,
  });
}

export function seekMedia(positionSec: number): Promise<MediaState> {
  return ipc<MediaState>('seek_media', { positionSec });
}

export function selectMediaSession(sessionId: string): Promise<MediaState> {
  return ipc<MediaState>('select_media_session', { sessionId });
}

export function snapWindow(hwnd: number, zone: SnapZone): Promise<void> {
  return ipc<void>('snap_window', { hwnd, zone });
}

export function toggleWindowTopmost(hwnd: number): Promise<boolean> {
  return ipc<boolean>('toggle_window_topmost', { hwnd });
}

export function minimizeOtherWindows(keepHwnd: number): Promise<number> {
  return ipc<number>('minimize_other_windows', { keepHwnd });
}

export function launchNewWindow(
  exePath: string,
  args?: string | null,
  closeStart?: boolean,
): Promise<void> {
  return ipc<void>('launch_new_window', {
    exePath,
    args: args ?? null,
    closeStart: closeStart ?? false,
  });
}

export function deleteClipboardEntry(id: string): Promise<ClipboardEntry[]> {
  return ipc<ClipboardEntry[]>('delete_clipboard_entry', { id });
}

export function setClipboardPaused(paused: boolean): Promise<boolean> {
  return ipc<boolean>('set_clipboard_paused', { paused });
}

export function getClipboardPaused(): Promise<boolean> {
  return ipc<boolean>('get_clipboard_paused');
}

export function pasteClipboardEntry(text: string): Promise<void> {
  return ipc<void>('paste_clipboard_entry', { text });
}

export function emptyRecycleBin(): Promise<void> {
  return ipc<void>('empty_recycle_bin');
}

export function openScreenSnip(): Promise<void> {
  return ipc<void>('open_screen_snip');
}

export function loadGameCover(coverPath: string): Promise<string | null> {
  return ipc<string | null>('load_game_cover', { coverPath });
}

export function refreshIndexes(): Promise<void> {
  return ipc<void>('refresh_indexes');
}

export function updateCustomSpace(params: {
  id: string;
  name?: string;
  accent?: string;
  accentHex?: string;
  accentRgb?: string;
  widgetPreset?: WidgetPreset;
}): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('update_custom_space', {
    id: params.id,
    name: params.name ?? null,
    accent: params.accent ?? null,
    accentHex: params.accentHex ?? null,
    accentRgb: params.accentRgb ?? null,
    widgetPreset: params.widgetPreset ?? null,
  });
}

export function deleteCustomSpace(id: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('delete_custom_space', { id });
}

export function updatePin(params: {
  space: string;
  pinId: string;
  title?: string;
  target?: string;
  args?: string;
  altAction?: string;
  altLabel?: string;
}): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('update_pin', {
    space: params.space,
    pinId: params.pinId,
    title: params.title ?? null,
    target: params.target ?? null,
    args: params.args ?? null,
    altAction: params.altAction ?? null,
    altLabel: params.altLabel ?? null,
  });
}

export function addGameCombo(params: {
  name: string;
  subtitle?: string;
  accent?: string;
  steps: string[];
}): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('add_game_combo', {
    name: params.name,
    subtitle: params.subtitle ?? null,
    accent: params.accent ?? null,
    steps: params.steps,
  });
}

export function updateGameCombo(params: {
  id: string;
  name?: string;
  subtitle?: string;
  accent?: string;
  steps?: string[];
}): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('update_game_combo', {
    id: params.id,
    name: params.name ?? null,
    subtitle: params.subtitle ?? null,
    accent: params.accent ?? null,
    steps: params.steps ?? null,
  });
}

export function deleteGameCombo(id: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('delete_game_combo', { id });
}

export function setAppearanceSettings(params: {
  glassDepth?: GlassDepth;
  clockShowSeconds?: boolean;
  clockShowWeekday?: boolean;
  startRememberSpace?: boolean;
}): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('set_appearance_settings', {
    glassDepth: params.glassDepth ?? null,
    clockShowSeconds: params.clockShowSeconds ?? null,
    clockShowWeekday: params.clockShowWeekday ?? null,
    startRememberSpace: params.startRememberSpace ?? null,
  });
}

export function hideTrayApp(key: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('hide_tray_app', { key });
}

export function restoreTrayApp(key: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('restore_tray_app', { key });
}

export function restoreAllTrayApps(): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('restore_all_tray_apps');
}

export function recordFrecency(itemId: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('record_frecency', { itemId });
}

export function exportConfigDialog(): Promise<string | null> {
  return ipc<string | null>('export_config_dialog');
}

export function importConfigDialog(): Promise<TestBarConfig | null> {
  return ipc<TestBarConfig | null>('import_config_dialog');
}
