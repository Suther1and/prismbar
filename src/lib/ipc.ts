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

export interface TestBarConfig {
  schemaVersion: number;
  revision: number;
  barMode: 'dock' | 'edge';
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
  isActive: boolean;
  isMinimized: boolean;
}

export interface AppWindowGroup {
  key: string;
  name: string;
  exePath: string;
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

export interface FlyoutRequest {
  kind: FlyoutKind;
  anchorXLogical: number;
  anchorInTaskbar: boolean;
  width: number;
  height: number;
  noActivate: boolean;
  payload: unknown;
}

export interface MergedBarItem {
  id: string;
  name: string;
  target: string;
  iconKey: string;
  iconData?: string | null;
  pinned: boolean;
  pinId?: string;
  group?: AppWindowGroup;
}

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

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

export function mergeTaskbarItems(
  pins: PinItem[],
  groups: AppWindowGroup[],
  iconMap: Record<string, string>,
): MergedBarItem[] {
  const usedGroupKeys = new Set<string>();
  const merged: MergedBarItem[] = [];

  for (const p of pins) {
    const targetNorm = normalizePath(p.target);
    const targetBase = extractBasename(p.target);
    const matchExeBase = extractBasename(p.matchExe);
    const titleLow = p.title.trim().toLowerCase();

    const matchedGroup = groups.find((g) => {
      if (usedGroupKeys.has(g.key)) return false;
      const groupKeyNorm = normalizePath(g.key);
      const groupBase = extractBasename(g.exePath);
      const groupNameLow = g.name.trim().toLowerCase();

      return (
        (Boolean(targetNorm) && groupKeyNorm === targetNorm) ||
        (Boolean(matchExeBase) && Boolean(groupBase) && groupBase === matchExeBase) ||
        (Boolean(targetBase) && Boolean(groupBase) && groupBase === targetBase) ||
        (Boolean(titleLow) && Boolean(groupNameLow) && groupNameLow === titleLow)
      );
    });

    if (matchedGroup) {
      usedGroupKeys.add(matchedGroup.key);
    }

    merged.push({
      id: p.id,
      name: p.title,
      target: p.target || matchedGroup?.exePath || '',
      iconKey: p.iconKey,
      iconData: matchedGroup?.iconData || p.iconData || iconMap[p.target],
      pinned: true,
      pinId: p.id,
      group: matchedGroup,
    });
  }

  for (const g of groups) {
    if (!usedGroupKeys.has(g.key)) {
      merged.push({
        id: 'run-' + g.key,
        name: g.name,
        target: g.exePath,
        iconKey: g.name.toLowerCase(),
        iconData: g.iconData,
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
