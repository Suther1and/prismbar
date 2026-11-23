import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

export type TileSize = 'small' | 'hero' | 'compact';
export type TileSection = 'main' | 'bottom';
export type HeaderWidgetSide = 'auto' | 'left' | 'right';
export type TaskbarWidgetId =
  | 'weather'
  | 'telemetry'
  | 'batteries'
  | 'network'
  | 'media'
  | 'timer';
export type TaskbarWidgetsPosition = 'left' | 'right';
export type ThemePreset =
  | 'obsidian'
  | 'carbon'
  | 'midnight'
  | 'emerald'
  | 'amethyst'
  | 'graphite';
export type AccentMode = 'per_space' | 'custom';
export type ClockDateFormat = 'short' | 'numeric' | 'none';
export type ClockLayout = 'center' | 'right' | 'single';

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
  customIcon?: string | null;
  customColor?: string | null;
  tileSize?: TileSize | null;
  section?: TileSection | null;
  runAsAdmin?: boolean;
}

export type WidgetPreset =
  | 'media_clipboard'
  | 'telemetry_processes'
  | 'audio_mic'
  | 'full_grid';

export type SpaceWidgetId =
  | 'media'
  | 'audio_mixer'
  | 'clipboard'
  | 'pc_health'
  | 'top_processes'
  | 'game_combos'
  | 'weather'
  | 'batteries'
  | 'notes'
  | 'timer'
  | 'network'
  | 'quick_toggles';

export interface SpaceHeaderWidget {
  id: SpaceWidgetId;
  side: HeaderWidgetSide;
}

export interface QuickTodoItem {
  id: string;
  text: string;
  done: boolean;
}

export interface QuickNotesState {
  text: string;
  todos: QuickTodoItem[];
}

export interface NetworkSnapshot {
  online: boolean;
  kind: 'ethernet' | 'wifi' | 'offline' | string;
  adapterName: string;
  localIp: string;
  rxBytesPerSec: number;
  txBytesPerSec: number;
  pingMs: number | null;
  pingHost: string;
}

export type PowerPlanKind = 'balanced' | 'high_performance' | 'power_saver' | 'custom';

export interface QuickTogglesState {
  keepAwake: boolean;
  powerPlan: PowerPlanKind | string;
  powerPlanLabel: string;
  recycleBinItems: number;
  recycleBinBytes: number;
}

export type SpaceLayoutKind = 'grid' | 'games';

export interface SpaceConfig {
  id: string;
  name: string;
  accentHex: string;
  accentRgb: string;
  layoutKind: SpaceLayoutKind;
  showHeroRow: boolean;
  bottomSectionTitle?: string | null;
  widgets: SpaceWidgetId[];
  headerWidgets?: SpaceHeaderWidget[];
  items: PinItem[];
}

export interface TaskbarWidgetSettings {
  showWeather: boolean;
  showBatteries: boolean;
  showTelemetry: boolean;
  leftWidgets?: TaskbarWidgetId[];
  widgetsPosition?: TaskbarWidgetsPosition;
  weatherCity: string;
  weatherLat?: number | null;
  weatherLon?: number | null;
  weatherUnits: 'c' | 'f';
}

export interface CustomSpace {
  id: string;
  name: string;
  accentHex: string;
  accentRgb: string;
  widgetPreset?: WidgetPreset;
  items: PinItem[];
}

export type SecondaryTaskbarMode = 'testbar' | 'native' | 'hidden';
export type GlassDepth = 'crystal' | 'obsidian' | 'matte' | 'solid';
export type BackdropMode = 'dcomp' | 'swca' | 'none';
export type SnapZone = 'left' | 'right' | 'maximize' | 'restore' | 'center';
export type MacCommandKeyMode = 'any' | 'left' | 'right' | 'disabled';
export type MacWindowPosition = 'center' | 'bottom';
export type MacTerminalApp = 'auto' | 'terminal' | 'iterm' | 'warp' | 'ghostty';

export interface PlatformInfo {
  platform: 'windows' | 'macos';
  accessibilityTrusted: boolean;
}

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
  themePreset?: ThemePreset;
  accentMode?: AccentMode;
  accentHex?: string;
  accentRgb?: string;
  surfaceGradients?: boolean;
  hoverGlow?: boolean;
  clockShowSeconds?: boolean;
  clockShowWeekday?: boolean;
  clockDateFormat?: ClockDateFormat;
  clockLayout?: ClockLayout;
  startRememberSpace?: boolean;
  defaultTileSize?: TileSize;
  macCommandKey?: MacCommandKeyMode;
  macWindowPosition?: MacWindowPosition;
  macTerminalApp?: MacTerminalApp;
  quickNotes?: QuickNotesState;
  hiddenTrayKeys?: string[];
  hiddenBatteryIds?: string[];
  frecency?: Record<string, FrecencyEntry>;
  taskbarPins: PinItem[];
  spaces: SpaceConfig[];
  taskbarWidgets?: TaskbarWidgetSettings;
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

export interface BatterySubLevel {
  label: string;
  percent: number;
  charging: boolean;
}

export interface PeripheralBattery {
  id: string;
  name: string;
  kind: 'mouse' | 'headphones' | 'audio' | 'keyboard' | 'gamepad' | 'other' | string;
  percent: number;
  charging: boolean;
  source: string;
  connected: boolean;
  subLevels?: BatterySubLevel[] | null;
}

export interface BreadcrumbPart {
  name: string;
  path: string;
}

export interface DriveEntry {
  letter: string;
  path: string;
  label: string;
  totalBytes: number;
  freeBytes: number;
}

export interface QuickRoot {
  id: string;
  label: string;
  path: string;
  kind: string;
}

export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  isHidden: boolean;
  sizeBytes: number;
  modifiedMs: number;
  extension: string;
  category:
    | 'folder'
    | 'image'
    | 'video'
    | 'audio'
    | 'code'
    | 'archive'
    | 'document'
    | 'app'
    | 'file'
    | string;
  hasPreview: boolean;
}

export interface DirectoryListing {
  currentPath: string;
  parentPath: string | null;
  breadcrumbs: BreadcrumbPart[];
  entries: FileEntry[];
  totalCount: number;
  drives: DriveEntry[];
  quickRoots: QuickRoot[];
}

export type WeatherConditionKey =
  | 'sun'
  | 'moon'
  | 'partly-cloudy-day'
  | 'partly-cloudy-night'
  | 'cloud'
  | 'fog'
  | 'drizzle'
  | 'rain'
  | 'snow'
  | 'storm'
  | string;

export interface WeatherCityOption {
  name: string;
  country: string;
  admin1: string;
  lat: number;
  lon: number;
}

export interface HourlyForecast {
  time: string;
  tempC: number;
  conditionIcon: WeatherConditionKey;
  precipProb: number;
}

export interface DailyForecast {
  date: string;
  label: string;
  tempMinC: number;
  tempMaxC: number;
  conditionIcon: WeatherConditionKey;
  precipProb: number;
}

export interface WeatherSnapshot {
  available: boolean;
  stale: boolean;
  city: string;
  lat: number;
  lon: number;
  tempC: number;
  feelsLikeC: number;
  humidity: number;
  windKmh: number;
  conditionCode: number;
  conditionIcon: WeatherConditionKey;
  conditionLabel: string;
  isDay: boolean;
  hourly: HourlyForecast[];
  daily: DailyForecast[];
  updatedAtMs: number;
  error: string | null;
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
  | 'weather'
  | 'battery'
  | 'network'
  | 'telemetry'
  | 'context_app'
  | 'context_bar'
  | 'context_start'
  | 'osd'
  | 'quick'
  | 'ctx_app'
  | 'ctx_taskbar'
  | 'ctx_start';

export type WinXAction =
  | 'apps'
  | 'system'
  | 'device_manager'
  | 'disk_management'
  | 'computer_management'
  | 'event_viewer'
  | 'network_connections'
  | 'terminal'
  | 'terminal_admin'
  | 'task_manager'
  | 'settings'
  | 'explorer'
  | 'run'
  | 'desktop'
  | 'lock'
  | 'sleep'
  | 'reboot'
  | 'shutdown';

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

export function isNonLaunchableUwpHostExe(path: string | null | undefined): boolean {
  const norm = normalizePath(path).replace(/^"+|"+$/g, '').trim();
  if (!norm) return false;
  return (
    norm.endsWith('\\systemsettings.exe') ||
    norm === 'systemsettings.exe' ||
    norm.endsWith('\\applicationframehost.exe') ||
    norm === 'applicationframehost.exe' ||
    norm.includes('\\windowsapps\\')
  );
}

export function formatAppsFolderTarget(aumid: string): string {
  const trimmed = aumid.trim();
  const prefix = 'shell:appsfolder\\';
  const clean = trimmed.toLowerCase().startsWith(prefix)
    ? trimmed.slice(prefix.length).trim()
    : trimmed;
  return clean ? `shell:AppsFolder\\${clean}` : '';
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

    const pinnedAumid = effectiveGroup?.aumid?.trim() || p.matchAumid?.trim() || '';
    const pinnedRawTarget = p.target?.trim() || effectiveGroup?.exePath?.trim() || '';
    const resolvedPinnedTarget =
      pinnedAumid && (!pinnedRawTarget || isNonLaunchableUwpHostExe(pinnedRawTarget))
        ? formatAppsFolderTarget(pinnedAumid)
        : pinnedRawTarget || (pinnedAumid ? formatAppsFolderTarget(pinnedAumid) : '');
    const pinnedExeBase = effectiveGroup?.exePath ? extractBasename(effectiveGroup.exePath) || null : null;

    merged.push({
      id: p.id,
      name: p.title,
      target: resolvedPinnedTarget,
      args: p.args ?? null,
      matchExe:
        p.matchExe ??
        (pinnedExeBase && pinnedExeBase !== 'applicationframehost.exe' ? pinnedExeBase : null),
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
      const groupAumid = g.aumid?.trim() || '';
      const rawExePath = g.exePath?.trim() || '';
      const resolvedGroupTarget =
        groupAumid && (!rawExePath || isNonLaunchableUwpHostExe(rawExePath))
          ? formatAppsFolderTarget(groupAumid)
          : rawExePath || (groupAumid ? formatAppsFolderTarget(groupAumid) : '');
      merged.push({
        id: 'run-' + g.key,
        name: g.name,
        target: resolvedGroupTarget,
        args: null,
        matchExe:
          baseExe && baseExe.endsWith('.exe') && baseExe !== 'applicationframehost.exe'
            ? baseExe
            : null,
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
  customIcon?: string | null;
  customColor?: string | null;
  iconData?: string | null;
  tileSize?: TileSize | null;
  section?: TileSection | null;
  runAsAdmin?: boolean;
}): Promise<TestBarConfig> {
  const payload: Record<string, unknown> = {
    space: params.space,
    pinId: params.pinId,
    title: params.title ?? null,
    target: params.target ?? null,
    args: params.args ?? null,
    altAction: params.altAction ?? null,
    altLabel: params.altLabel ?? null,
  };
  if (params.customIcon !== undefined) payload.customIcon = params.customIcon;
  if (params.customColor !== undefined) payload.customColor = params.customColor;
  if (params.iconData !== undefined) payload.iconData = params.iconData;
  if (params.tileSize !== undefined) payload.tileSize = params.tileSize;
  if (params.section !== undefined) payload.section = params.section;
  if (params.runAsAdmin !== undefined) payload.runAsAdmin = params.runAsAdmin;
  return ipc<TestBarConfig>('update_pin', payload);
}

export function movePinInSpace(params: {
  space: string;
  fromId: string;
  toId?: string | null;
  targetSection?: TileSection | null;
}): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('move_pin_in_space', {
    space: params.space,
    fromId: params.fromId,
    toId: params.toId ?? null,
    targetSection: params.targetSection ?? null,
  });
}

export function createSpace(params: {
  name: string;
  template?: 'blank' | 'personal' | 'work' | 'studio' | 'games' | string;
  accentHex?: string;
  accentRgb?: string;
  widgets?: SpaceWidgetId[];
}): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('create_space', {
    name: params.name,
    template: params.template ?? null,
    accentHex: params.accentHex ?? null,
    accentRgb: params.accentRgb ?? null,
    widgets: params.widgets ?? null,
  });
}

export function updateSpace(params: {
  id: string;
  name?: string;
  accentHex?: string;
  accentRgb?: string;
  layoutKind?: SpaceLayoutKind;
  showHeroRow?: boolean;
  widgets?: SpaceWidgetId[];
  bottomSectionTitle?: string | null;
}): Promise<TestBarConfig> {
  const payload: Record<string, unknown> = {
    id: params.id,
    name: params.name ?? null,
    accentHex: params.accentHex ?? null,
    accentRgb: params.accentRgb ?? null,
    layoutKind: params.layoutKind ?? null,
    showHeroRow: params.showHeroRow ?? null,
    widgets: params.widgets ?? null,
  };
  if (params.bottomSectionTitle !== undefined) {
    payload.bottomSectionTitle = params.bottomSectionTitle;
  }
  return ipc<TestBarConfig>('update_space', payload);
}

export function deleteSpace(id: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('delete_space', { id });
}

export function reorderSpaces(orderedIds: string[]): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('reorder_spaces', { orderedIds });
}

export function setSpaceWidgets(id: string, widgets: SpaceWidgetId[]): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('set_space_widgets', { id, widgets });
}

export function setSpaceWidgetLayout(
  id: string,
  widgets: SpaceWidgetId[],
  headerWidgets: SpaceHeaderWidget[],
): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('set_space_widget_layout', { id, widgets, headerWidgets });
}

export function setTaskbarWidgets(params: {
  showWeather?: boolean;
  showBatteries?: boolean;
  showTelemetry?: boolean;
  leftWidgets?: TaskbarWidgetId[];
  widgetsPosition?: TaskbarWidgetsPosition;
  weatherCity?: string;
  weatherLat?: number | null;
  weatherLon?: number | null;
  weatherUnits?: 'c' | 'f';
}): Promise<TestBarConfig> {
  const payload: Record<string, unknown> = {
    showWeather: params.showWeather ?? null,
    showBatteries: params.showBatteries ?? null,
    showTelemetry: params.showTelemetry ?? null,
    weatherCity: params.weatherCity ?? null,
    weatherUnits: params.weatherUnits ?? null,
  };
  if (params.leftWidgets !== undefined) payload.leftWidgets = params.leftWidgets;
  if (params.widgetsPosition !== undefined) payload.widgetsPosition = params.widgetsPosition;
  if (params.weatherLat !== undefined) payload.weatherLat = params.weatherLat;
  if (params.weatherLon !== undefined) payload.weatherLon = params.weatherLon;
  return ipc<TestBarConfig>('set_taskbar_widgets', payload);
}

export function pickCustomIconImage(): Promise<string | null> {
  return ipc<string | null>('pick_custom_icon_image');
}

export function listDirectory(
  path: string,
  opts?: {
    sortBy?: 'name' | 'date' | 'size' | 'type';
    sortDir?: 'asc' | 'desc';
    showHidden?: boolean;
  },
): Promise<DirectoryListing> {
  return ipc<DirectoryListing>('list_directory', {
    path,
    sortBy: opts?.sortBy ?? null,
    sortDir: opts?.sortDir ?? null,
    showHidden: opts?.showHidden ?? null,
  });
}

export function getFileThumbnails(
  paths: string[],
  size?: number,
): Promise<Record<string, string>> {
  return ipc<Record<string, string>>('get_file_thumbnails', {
    paths,
    size: size ?? null,
  });
}

export function fsOpenItem(path: string): Promise<void> {
  return ipc<void>('fs_open_item', { path });
}

export function fsCreateFolder(parentPath: string, name: string): Promise<FileEntry> {
  return ipc<FileEntry>('fs_create_folder', { parentPath, name });
}

export function fsRenameItem(path: string, newName: string): Promise<FileEntry> {
  return ipc<FileEntry>('fs_rename_item', { path, newName });
}

export function fsCopyFilesToClipboard(paths: string[]): Promise<void> {
  return ipc<void>('fs_copy_files_to_clipboard', { paths });
}

export function fsTrashItems(paths: string[]): Promise<number> {
  return ipc<number>('fs_trash_items', { paths });
}

export function fsOpenInExplorer(path: string): Promise<void> {
  return ipc<void>('fs_open_in_explorer', { path });
}

export function getWeather(params?: {
  city?: string;
  lat?: number;
  lon?: number;
  force?: boolean;
}): Promise<WeatherSnapshot> {
  return ipc<WeatherSnapshot>('get_weather', {
    city: params?.city ?? null,
    lat: params?.lat ?? null,
    lon: params?.lon ?? null,
    force: params?.force ?? null,
  });
}

export function searchWeatherCities(query: string): Promise<WeatherCityOption[]> {
  return ipc<WeatherCityOption[]>('search_weather_cities', { query });
}

export function getPeripheralBatteries(): Promise<PeripheralBattery[]> {
  return ipc<PeripheralBattery[]>('get_peripheral_batteries');
}

export function refreshPeripheralBatteries(): Promise<PeripheralBattery[]> {
  return ipc<PeripheralBattery[]>('refresh_peripheral_batteries');
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
  themePreset?: ThemePreset;
  accentMode?: AccentMode;
  accentHex?: string;
  accentRgb?: string;
  surfaceGradients?: boolean;
  hoverGlow?: boolean;
  clockShowSeconds?: boolean;
  clockShowWeekday?: boolean;
  clockDateFormat?: ClockDateFormat;
  clockLayout?: ClockLayout;
  startRememberSpace?: boolean;
  defaultTileSize?: TileSize;
  resetVisuals?: boolean;
}): Promise<TestBarConfig> {
  const payload: Record<string, unknown> = {
    glassDepth: params.glassDepth ?? null,
    clockShowSeconds: params.clockShowSeconds ?? null,
    clockShowWeekday: params.clockShowWeekday ?? null,
    startRememberSpace: params.startRememberSpace ?? null,
  };
  if (params.themePreset !== undefined) payload.themePreset = params.themePreset;
  if (params.accentMode !== undefined) payload.accentMode = params.accentMode;
  if (params.accentHex !== undefined) payload.accentHex = params.accentHex;
  if (params.accentRgb !== undefined) payload.accentRgb = params.accentRgb;
  if (params.surfaceGradients !== undefined) payload.surfaceGradients = params.surfaceGradients;
  if (params.hoverGlow !== undefined) payload.hoverGlow = params.hoverGlow;
  if (params.clockDateFormat !== undefined) payload.clockDateFormat = params.clockDateFormat;
  if (params.clockLayout !== undefined) payload.clockLayout = params.clockLayout;
  if (params.defaultTileSize !== undefined) payload.defaultTileSize = params.defaultTileSize;
  if (params.resetVisuals !== undefined) payload.resetVisuals = params.resetVisuals;
  return ipc<TestBarConfig>('set_appearance_settings', payload);
}

export function saveQuickNotesText(text: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('save_quick_notes_text', { text });
}

export function addQuickTodo(text: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('add_quick_todo', { text });
}

export function toggleQuickTodo(id: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('toggle_quick_todo', { id });
}

export function deleteQuickTodo(id: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('delete_quick_todo', { id });
}

export function clearDoneQuickTodos(): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('clear_done_quick_todos');
}

export function getNetworkSnapshot(probePing = false): Promise<NetworkSnapshot> {
  return ipc<NetworkSnapshot>('get_network_snapshot', { probePing });
}

export function getQuickTogglesState(): Promise<QuickTogglesState> {
  return ipc<QuickTogglesState>('get_quick_toggles_state');
}

export function setKeepAwake(enable: boolean): Promise<QuickTogglesState> {
  return ipc<QuickTogglesState>('set_keep_awake', { enable });
}

export function setPowerPlan(plan: 'balanced' | 'high_performance' | 'power_saver' | string): Promise<QuickTogglesState> {
  return ipc<QuickTogglesState>('set_power_plan', { plan });
}

export function triggerScreenshot(): Promise<void> {
  return ipc<void>('trigger_screenshot');
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

export function hideBatteryDevice(id: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('hide_battery_device', { id });
}

export function restoreBatteryDevice(id: string): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('restore_battery_device', { id });
}

export function restoreAllBatteryDevices(): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('restore_all_battery_devices');
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

export function getPlatformInfo(): Promise<PlatformInfo> {
  return ipc<PlatformInfo>('get_platform_info');
}

export function getAccessibilityStatus(): Promise<boolean> {
  return ipc<boolean>('get_accessibility_status');
}

export function openAccessibilitySettings(): Promise<void> {
  return ipc<void>('open_accessibility_settings');
}

export function setMacSettings(params: {
  macCommandKey?: MacCommandKeyMode;
  macWindowPosition?: MacWindowPosition;
  macTerminalApp?: MacTerminalApp;
}): Promise<TestBarConfig> {
  return ipc<TestBarConfig>('set_mac_settings', {
    macCommandKey: params.macCommandKey ?? null,
    macWindowPosition: params.macWindowPosition ?? null,
    macTerminalApp: params.macTerminalApp ?? null,
  });
}

export function quitApp(): Promise<void> {
  return ipc<void>('quit_app');
}

export interface UpdateCheckResult {
  available: boolean;
  currentVersion: string;
  version: string | null;
  date: string | null;
  body: string | null;
}

export interface UpdateProgressEvent {
  stage: 'downloading' | 'installing';
  downloaded: number;
  total: number | null;
  percent: number | null;
}

export function getAppVersion(): Promise<string> {
  return ipc<string>('get_app_version');
}

export function checkForUpdates(): Promise<UpdateCheckResult> {
  return ipc<UpdateCheckResult>('check_for_updates');
}

export function installUpdate(): Promise<void> {
  return ipc<void>('install_update');
}
