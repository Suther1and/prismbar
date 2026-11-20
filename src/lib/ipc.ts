import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface PinItem {
  id: string;
  kind: 'app' | 'folder' | 'file' | 'url' | 'combo';
  title: string;
  subtitle: string;
  target: string;
  args?: string | null;
  altAction: string;
  accentRgb: string;
  iconKey: string;
  iconData?: string | null;
  matchExe?: string | null;
  matchAumid?: string | null;
  comboTargets?: string[] | null;
}

export interface CustomSpace {
  id: string;
  name: string;
  accentHex: string;
  accentRgb: string;
  items: PinItem[];
}

export type SecondaryTaskbarMode = 'testbar' | 'native' | 'hidden';

export interface TestBarConfig {
  schemaVersion: number;
  revision: number;
  barMode: 'dock' | 'edge';
  secondaryTaskbarMode: SecondaryTaskbarMode;
  autostart: boolean;
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
  iconData?: string | null;
}

export interface SteamGame {
  appId: string;
  name: string;
  launcher: string;
  target: string;
  coverData?: string | null;
  lastUpdated: number;
}

export interface AudioDeviceInfo {
  id: string;
  name: string;
  shortName: string;
  isDefault: boolean;
}

export interface AudioSessionInfo {
  pid: number;
  name: string;
  volume: number;
  muted: boolean;
}

export interface AudioState {
  masterVolume: number;
  masterMuted: boolean;
  micMuted: boolean;
  devices: AudioDeviceInfo[];
  sessions: AudioSessionInfo[];
}

export interface MediaState {
  hasSession: boolean;
  title: string;
  artist: string;
  source: string;
  isPlaying: boolean;
  positionSec: number;
  durationSec: number;
}

export interface TopProcessInfo {
  pid: number;
  name: string;
  memGb: number;
  cpuPct: number;
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
}

export interface ClipboardEntry {
  id: string;
  tag: string;
  text: string;
  color?: string | null;
  pinned: boolean;
}

export interface PeripheralBattery {
  name: string;
  kind: 'mouse' | 'audio' | 'other';
  percent: number;
}

export interface TrayAppItem {
  pid: number;
  name: string;
  exePath: string;
  iconData?: string | null;
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
}

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const iconCache = new Map<string, string>();
const inFlightIconRequests = new Map<string, Promise<string | undefined>>();

export function isCacheIconKey(key: string | null | undefined): key is string {
  if (!key) return false;
  return (
    key.startsWith('exe:') ||
    key.startsWith('aumid:') ||
    key.startsWith('hwnd:') ||
    key.startsWith('file:')
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

export function mergeTaskbarItems(
  pins: PinItem[],
  groups: AppWindowGroup[],
  iconMap: Record<string, string>,
): MergedBarItem[] {
  const usedGroupKeys = new Set<string>();
  const merged: MergedBarItem[] = [];

  for (const p of pins) {
    const pinAumid = normalizeAumid(p.matchAumid) || extractAumidFromTarget(p.target);
    const matchExeBase = extractBasename(p.matchExe);
    const targetNorm = normalizePath(p.target);
    const targetBase = extractBasename(p.target);
    const titleLow = p.title.trim().toLowerCase();

    const available = groups.filter((g) => !usedGroupKeys.has(g.key));

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

    if (matchedGroup) {
      usedGroupKeys.add(matchedGroup.key);
    }

    const cacheKey = matchedGroup?.iconKey ?? (isCacheIconKey(p.iconKey) ? p.iconKey : null);
    const resolvedIconData =
      resolveCachedIcon(iconMap, matchedGroup?.iconKey, isCacheIconKey(p.iconKey) ? p.iconKey : null) ||
      matchedGroup?.iconData ||
      p.iconData ||
      iconMap[p.target] ||
      null;

    merged.push({
      id: p.id,
      name: p.title,
      target:
        p.target ||
        matchedGroup?.exePath ||
        (matchedGroup?.aumid ? `shell:AppsFolder\\${matchedGroup.aumid}` : ''),
      args: p.args ?? null,
      matchExe: p.matchExe ?? (matchedGroup?.exePath ? extractBasename(matchedGroup.exePath) || null : null),
      matchAumid: p.matchAumid ?? matchedGroup?.aumid ?? null,
      iconKey: p.iconKey,
      cacheIconKey: cacheKey,
      iconData: resolvedIconData,
      pinned: true,
      pinId: p.id,
      group: matchedGroup,
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
  return listen<T>(event, (e) => cb(e.payload));
}
