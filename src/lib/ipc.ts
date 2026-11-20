import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface PinItem {
  id: string;
  kind: 'app' | 'folder' | 'file' | 'url';
  name: string;
  subtitle: string;
  target: string;
  altAction: string;
  accentRgb: string;
  iconKey: string;
  iconData?: string | null;
}

export interface GameCombo {
  id: string;
  name: string;
  subtitle: string;
  targets: string[];
  accentRgb: string;
}

export interface CustomSpace {
  id: string;
  name: string;
  accentHex: string;
  accentRgb: string;
  items: PinItem[];
}

export interface TestBarConfig {
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
  gameCombos: GameCombo[];
  customSpaces: CustomSpace[];
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

export interface FlyoutRequest {
  kind: 'tray' | 'quick' | 'calendar' | 'preview' | 'ctx_app' | 'ctx_taskbar' | 'ctx_tray';
  anchorXLogical: number;
  anchorInTaskbar: boolean;
  width: number;
  height: number;
  noActivate: boolean;
  payload: unknown;
}

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export async function ipc<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

export async function onEvent<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<T>(event, (e) => cb(e.payload));
}
