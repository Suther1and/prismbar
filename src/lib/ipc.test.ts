import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.hoisted(() => vi.fn());
const globalListenMock = vi.hoisted(() => vi.fn());
const windowListenMock = vi.hoisted(() => vi.fn());
const getCurrentWebviewWindowMock = vi.hoisted(() =>
  vi.fn(() => ({ listen: windowListenMock })),
);

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: globalListenMock,
}));
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: getCurrentWebviewWindowMock,
}));

import {
  addGameCombo,
  addQuickTodo,
  applyConfigUpdate,
  checkForUpdates,
  clearDoneQuickTodos,
  clearIconCache,
  createSpace,
  deleteClipboardEntry,
  deleteCustomSpace,
  deleteGameCombo,
  deleteQuickTodo,
  deleteSpace,
  dismissOverlays,
  emptyRecycleBin,
  exportConfigDialog,
  fetchIconsBatch,
  fsCopyFilesToClipboard,
  fsCreateFolder,
  fsOpenInExplorer,
  fsOpenItem,
  fsRenameItem,
  fsTrashItems,
  getAppVersion,
  getAudioPeaks,
  getClipboardPaused,
  getFileThumbnails,
  getNetworkSnapshot,
  getPeripheralBatteries,
  getQuickTogglesState,
  getWeather,
  hideBatteryDevice,
  hideTrayApp,
  ICON_CACHE_MAX_ENTRIES,
  iconCache,
  importConfigDialog,
  installUpdate,
  isCacheIconKey,
  isNonLaunchableUwpHostExe,
  launchNewWindow,
  listDirectory,
  loadGameCover,
  LruIconCache,
  mergeTaskbarItems,
  minimizeOtherWindows,
  movePinInSpace,
  normalizeStartStatePayload,
  onEvent,
  openScreenSnip,
  pasteClipboardEntry,
  pickCustomIconImage,
  recordFrecency,
  refreshIndexes,
  refreshPeripheralBatteries,
  reorderSpaces,
  restoreAllBatteryDevices,
  restoreAllTrayApps,
  restoreBatteryDevice,
  restoreTrayApp,
  saveQuickNotesText,
  searchWeatherCities,
  seekMedia,
  selectMediaSession,
  setAppearanceSettings,
  setClipboardPaused,
  setInputDevice,
  setKeepAwake,
  setPowerPlan,
  setSessionState,
  setSpaceWidgetLayout,
  setSpaceWidgets,
  setTaskbarWidgets,
  shouldApplyConfigUpdate,
  snapWindow,
  surfaceCommitAck,
  surfaceExitAck,
  toggleQuickTodo,
  toggleStart,
  toggleWindowTopmost,
  triggerScreenshot,
  updateCustomSpace,
  updateGameCombo,
  updatePin,
  updateSpace,
  type AppWindowGroup,
  type PinItem,
  type TestBarConfig,
  type TileSection,
  type TileSize,
} from './ipc';

function makeConfig(revision: number, barMode: 'dock' | 'edge' = 'dock'): TestBarConfig {
  return {
    schemaVersion: 1,
    revision,
    barMode,
    secondaryTaskbarMode: 'testbar',
    autostart: false,
    taskbarPins: [],
    spaces: [],
    personalPins: [],
    personalLinks: [],
    workFolders: [],
    workApps: [],
    studioPins: [],
    studioTools: [],
    gameLaunchers: [],
    gameCombos: [],
    customSpaces: [],
    pinnedClipboard: [],
  };
}

describe('shouldApplyConfigUpdate', () => {
  it('accepts initial snapshot with revision 0 when prev is null', () => {
    expect(shouldApplyConfigUpdate(null, makeConfig(0))).toBe(true);
    expect(shouldApplyConfigUpdate(undefined, makeConfig(3))).toBe(true);
  });

  it('accepts strictly newer revisions', () => {
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(6))).toBe(true);
    expect(shouldApplyConfigUpdate(makeConfig(0), makeConfig(1))).toBe(true);
  });

  it('rejects older and duplicate revisions', () => {
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(4))).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(5))).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(0), makeConfig(0))).toBe(false);
  });

  it('rejects invalid revisions without throwing', () => {
    expect(shouldApplyConfigUpdate(null, null)).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), null)).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), undefined)).toBe(false);
    expect(shouldApplyConfigUpdate(null, makeConfig(Number.NaN))).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(Number.NaN))).toBe(false);
    expect(shouldApplyConfigUpdate(null, makeConfig(-1))).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(-1))).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(Number.POSITIVE_INFINITY))).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(5.5))).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(Number.MAX_SAFE_INTEGER + 1))).toBe(false);
    expect(shouldApplyConfigUpdate(makeConfig(5), makeConfig(Number.MAX_SAFE_INTEGER))).toBe(true);
  });
});

describe('applyConfigUpdate', () => {
  it('returns incoming snapshot on initial load and strictly newer revision', () => {
    const rev0 = makeConfig(0, 'dock');
    const rev1 = makeConfig(1, 'edge');

    expect(applyConfigUpdate(null, rev0)).toBe(rev0);
    expect(applyConfigUpdate(rev0, rev1)).toBe(rev1);
  });

  it('preserves previous snapshot on older, duplicate, or invalid incoming revisions', () => {
    const rev5 = makeConfig(5, 'edge');
    const rev4 = makeConfig(4, 'dock');
    const rev5Dup = makeConfig(5, 'dock');
    const revNaN = makeConfig(Number.NaN, 'dock');
    const revNeg = makeConfig(-1, 'dock');
    const revUnsafe = makeConfig(Number.MAX_SAFE_INTEGER + 1, 'dock');

    expect(applyConfigUpdate(rev5, rev4)).toBe(rev5);
    expect(applyConfigUpdate(rev5, rev5Dup)).toBe(rev5);
    expect(applyConfigUpdate(rev5, revNaN)).toBe(rev5);
    expect(applyConfigUpdate(rev5, revNeg)).toBe(rev5);
    expect(applyConfigUpdate(rev5, revUnsafe)).toBe(rev5);
    expect(applyConfigUpdate(rev5, null)).toBe(rev5);
    expect(applyConfigUpdate(null, revNaN)).toBeNull();
  });
});

function makePin(overrides: Partial<PinItem> & { id: string; title: string }): PinItem {
  return {
    kind: 'app',
    subtitle: '',
    target: '',
    altAction: '',
    accentRgb: '56,189,248',
    iconKey: 'app',
    iconData: null,
    ...overrides,
  };
}

function makeGroup(overrides: Partial<AppWindowGroup> & { key: string; name: string }): AppWindowGroup {
  return {
    exePath: '',
    iconData: null,
    isActive: false,
    windows: [],
    ...overrides,
  };
}

describe('mergeTaskbarItems', () => {
  it('matches case-insensitive matchExe without colliding on suffix substrings', () => {
    const pins = [
      makePin({
        id: 'pin-code',
        title: 'Editor',
        target: 'C:\\Tools\\launcher.exe',
        matchExe: 'Code.EXE',
      }),
    ];
    const groups = [
      makeGroup({
        key: 'c:\\apps\\vscode.exe',
        name: 'VSCode',
        exePath: 'C:\\Apps\\vscode.exe',
      }),
      makeGroup({
        key: 'c:\\apps\\code.exe',
        name: 'Code',
        exePath: 'C:\\Apps\\code.exe',
      }),
    ];

    const merged = mergeTaskbarItems(pins, groups, {});
    expect(merged).toHaveLength(2);
    expect(merged[0].id).toBe('pin-code');
    expect(merged[0].group?.key).toBe('c:\\apps\\code.exe');
    expect(merged[1].id).toBe('run-c:\\apps\\vscode.exe');
    expect(merged[1].pinned).toBe(false);
  });

  it('matches forward-slash and bare exePath values', () => {
    const pins = [
      makePin({ id: 'p-slash', title: 'Zen', target: 'C:\\Program Files\\Zen\\zen.exe', matchExe: 'zen.exe' }),
      makePin({ id: 'p-bare', title: 'Notepad', target: 'C:\\Windows\\notepad.exe', matchExe: 'notepad.exe' }),
    ];
    const groups = [
      makeGroup({ key: 'grp-zen', name: 'Browser', exePath: 'C:/Program Files/Zen/Zen.exe' }),
      makeGroup({ key: 'grp-note', name: 'Editor', exePath: 'Notepad.EXE' }),
    ];

    const merged = mergeTaskbarItems(pins, groups, {});
    expect(merged).toHaveLength(2);
    expect(merged[0].group?.key).toBe('grp-zen');
    expect(merged[1].group?.key).toBe('grp-note');
  });

  it('binds a running group to at most one pin when duplicate pins exist', () => {
    const pins = [
      makePin({ id: 'p1', title: 'Telegram', target: 'C:\\Apps\\Telegram.exe', matchExe: 'telegram.exe' }),
      makePin({ id: 'p2', title: 'Telegram', target: 'C:\\Apps\\Telegram.exe', matchExe: 'telegram.exe' }),
    ];
    const groups = [
      makeGroup({ key: 'c:\\apps\\telegram.exe', name: 'Telegram', exePath: 'C:\\Apps\\Telegram.exe' }),
    ];

    const merged = mergeTaskbarItems(pins, groups, {});
    expect(merged).toHaveLength(2);
    expect(merged[0].group?.key).toBe('c:\\apps\\telegram.exe');
    expect(merged[1].group).toBeUndefined();
  });

  it('matches by title and does not falsely match empty titles or trailing-slash targets', () => {
    const pins = [
      makePin({ id: 'p-title', title: 'Spotify', target: '' }),
      makePin({ id: 'p-empty', title: '   ', target: 'C:\\Folders\\' }),
    ];
    const groups = [
      makeGroup({ key: 'grp-spot', name: 'spotify', exePath: 'C:\\Store\\app.exe' }),
      makeGroup({ key: 'grp-blank', name: '   ', exePath: 'C:\\Other\\' }),
    ];

    const merged = mergeTaskbarItems(pins, groups, {});
    expect(merged[0].group?.key).toBe('grp-spot');
    expect(merged[1].group).toBeUndefined();
    expect(merged.some((m) => m.id === 'run-grp-blank')).toBe(true);
  });

  it('deduplicates pinned UWP apps via case-insensitive matchAumid and resolves icons via iconKey', () => {
    clearIconCache();
    iconCache.set('aumid:settings', 'data:image/png;base64,SETTINGS');

    const pins = [
      makePin({
        id: 'pin-calc',
        title: 'Калькулятор',
        target: 'shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
        matchAumid: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
        iconKey: 'calc',
      }),
      makePin({
        id: 'pin-term',
        title: 'Терминал',
        target: 'shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
        iconKey: 'warp',
      }),
    ];
    const groups = [
      makeGroup({
        key: 'aumid:microsoft.windowscalculator_8wekyb3d8bbwe!app',
        name: 'Calculator',
        exePath: 'C:\\Program Files\\WindowsApps\\Calc\\CalculatorApp.exe',
        aumid: 'microsoft.windowscalculator_8wekyb3d8bbwe!app',
        iconKey: 'aumid:calc',
        isActive: true,
      }),
      makeGroup({
        key: 'aumid:microsoft.windowsterminal_8wekyb3d8bbwe!app',
        name: 'Windows Terminal',
        exePath: 'C:\\Program Files\\WindowsApps\\WT\\WindowsTerminal.exe',
        aumid: 'Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
        iconKey: 'aumid:wt',
      }),
      makeGroup({
        key: 'aumid:windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
        name: 'Параметры',
        exePath: 'C:\\Windows\\ImmersiveControlPanel\\SystemSettings.exe',
        aumid: 'windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
        iconKey: 'aumid:settings',
      }),
    ];

    const merged = mergeTaskbarItems(pins, groups, {
      'aumid:calc': 'data:image/png;base64,CALC',
      'aumid:wt': 'data:image/png;base64,WT',
    });

    // 2 pinned UWP apps merged + 1 unpinned UWP Settings app = 3 total slots, zero duplicates
    expect(merged).toHaveLength(3);
    expect(merged[0].id).toBe('pin-calc');
    expect(merged[0].pinned).toBe(true);
    expect(merged[0].group?.aumid).toBe('microsoft.windowscalculator_8wekyb3d8bbwe!app');
    expect(merged[0].iconData).toBe('data:image/png;base64,CALC');
    expect(merged[0].cacheIconKey).toBe('aumid:calc');

    expect(merged[1].id).toBe('pin-term');
    expect(merged[1].pinned).toBe(true);
    expect(merged[1].group?.aumid).toBe('Microsoft.WindowsTerminal_8wekyb3d8bbwe!App');
    expect(merged[1].iconData).toBe('data:image/png;base64,WT');

    // Unpinned UWP Settings resolves icon straight from iconCache via iconKey
    // and prefers shell:AppsFolder\<aumid> over non-launchable SystemSettings.exe
    expect(merged[2].pinned).toBe(false);
    expect(merged[2].name).toBe('Параметры');
    expect(merged[2].target).toBe(
      'shell:AppsFolder\\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
    );
    expect(merged[2].iconData).toBe('data:image/png;base64,SETTINGS');
    expect(merged[2].cacheIconKey).toBe('aumid:settings');
    expect(isNonLaunchableUwpHostExe('C:\\Windows\\ImmersiveControlPanel\\SystemSettings.exe')).toBe(true);
    expect(isNonLaunchableUwpHostExe('C:/Windows/System32/ApplicationFrameHost.exe')).toBe(true);
    expect(isNonLaunchableUwpHostExe('C:\\Program Files\\WindowsApps\\Calc\\Calc.exe')).toBe(true);
    expect(isNonLaunchableUwpHostExe('C:\\Windows\\notepad.exe')).toBe(false);
  });

  it('prioritizes exact matchAumid over matchExe and title fallback', () => {
    const pins = [
      makePin({
        id: 'pin-uwp',
        title: 'Store',
        target: 'C:\\Tools\\store.exe',
        matchExe: 'store.exe',
        matchAumid: 'Microsoft.WindowsStore_8wekyb3d8bbwe!App',
      }),
    ];
    const groups = [
      makeGroup({
        key: 'c:\\tools\\store.exe',
        name: 'Store',
        exePath: 'C:\\Tools\\store.exe',
        aumid: null,
      }),
      makeGroup({
        key: 'aumid:microsoft.windowsstore_8wekyb3d8bbwe!app',
        name: 'Microsoft Store',
        exePath: 'C:\\Program Files\\WindowsApps\\Store\\WinStore.App.exe',
        aumid: 'Microsoft.WindowsStore_8wekyb3d8bbwe!App',
      }),
    ];

    const merged = mergeTaskbarItems(pins, groups, {});
    expect(merged).toHaveLength(2);
    expect(merged[0].group?.key).toBe('aumid:microsoft.windowsstore_8wekyb3d8bbwe!app');
    expect(merged[1].id).toBe('run-c:\\tools\\store.exe');
  });
});

describe('fetchIconsBatch and iconCache', () => {
  beforeEach(() => {
    clearIconCache();
  });

  it('identifies namespaced cache icon keys accurately', () => {
    expect(isCacheIconKey('exe:0123456789abcdef')).toBe(true);
    expect(isCacheIconKey('aumid:fedcba9876543210')).toBe(true);
    expect(isCacheIconKey('hwnd:1234:0')).toBe(true);
    expect(isCacheIconKey('file:abcdef0123456789')).toBe(true);
    expect(isCacheIconKey('explorer')).toBe(false);
    expect(isCacheIconKey('')).toBe(false);
    expect(isCacheIconKey(null)).toBe(false);
  });

  it('batches missing keys, caches results, and skips IPC on subsequent hits', async () => {
    const fetcher = vi.fn(async (keys: string[]) => {
      const out: Record<string, string> = {};
      for (const k of keys) {
        if (k !== 'exe:missing') {
          out[k] = `data:image/png;base64,${k}`;
        }
      }
      return out;
    });

    const first = await fetchIconsBatch(['exe:a', 'exe:b', 'exe:a', '  ', 'exe:missing'], fetcher);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(fetcher).toHaveBeenCalledWith(['exe:a', 'exe:b', 'exe:missing']);
    expect(first).toEqual({
      'exe:a': 'data:image/png;base64,exe:a',
      'exe:b': 'data:image/png;base64,exe:b',
    });
    expect(iconCache.get('exe:a')).toBe('data:image/png;base64,exe:a');
    expect(iconCache.has('exe:missing')).toBe(false);

    // Subsequent request for cached keys makes ZERO fetcher calls
    const second = await fetchIconsBatch(['exe:a', 'exe:b'], fetcher);
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(second).toEqual(first);
  });

  it('deduplicates concurrent in-flight keys across overlapping calls', async () => {
    let resolveFirst!: (val: Record<string, string>) => void;
    const firstDeferred = new Promise<Record<string, string>>((res) => {
      resolveFirst = res;
    });

    const calls: string[][] = [];
    const fetcher = vi.fn((keys: string[]) => {
      calls.push(keys);
      if (calls.length === 1) {
        return firstDeferred;
      }
      return Promise.resolve({ 'exe:c': 'data:image/png;base64,C' });
    });

    const p1 = fetchIconsBatch(['exe:a', 'exe:b'], fetcher);
    const p2 = fetchIconsBatch(['exe:b', 'exe:c'], fetcher);

    // Second call must NOT re-request in-flight exe:b — only exe:c
    expect(calls).toEqual([
      ['exe:a', 'exe:b'],
      ['exe:c'],
    ]);

    resolveFirst({
      'exe:a': 'data:image/png;base64,A',
      'exe:b': 'data:image/png;base64,B',
    });

    const [r1, r2] = await Promise.all([p1, p2]);
    expect(r1).toEqual({
      'exe:a': 'data:image/png;base64,A',
      'exe:b': 'data:image/png;base64,B',
    });
    expect(r2).toEqual({
      'exe:b': 'data:image/png;base64,B',
      'exe:c': 'data:image/png;base64,C',
    });
  });

  it('recovers cleanly on fetch failure so later retries succeed', async () => {
    const failFetcher = vi.fn(async () => {
      throw new Error('ipc down');
    });
    const failed = await fetchIconsBatch(['aumid:retry'], failFetcher);
    expect(failed).toEqual({});
    expect(iconCache.has('aumid:retry')).toBe(false);

    const okFetcher = vi.fn(async () => ({ 'aumid:retry': 'data:image/png;base64,OK' }));
    const retried = await fetchIconsBatch(['aumid:retry'], okFetcher);
    expect(retried).toEqual({ 'aumid:retry': 'data:image/png;base64,OK' });
    expect(iconCache.get('aumid:retry')).toBe('data:image/png;base64,OK');
  });

  it('bounds iconCache to 384 entries and refreshes recency on read/write (LRU)', () => {
    expect(ICON_CACHE_MAX_ENTRIES).toBe(384);

    // Small LRU eviction behavior: touching key 'a' keeps it alive when 'd' arrives
    const lru = new LruIconCache(3);
    lru.set('a', 'A');
    lru.set('b', 'B');
    lru.set('c', 'C');
    expect(lru.get('a')).toBe('A'); // 'a' becomes MRU; 'b' is now oldest LRU
    lru.set('d', 'D');
    expect(lru.size).toBe(3);
    expect(lru.has('b')).toBe(false);
    expect(lru.get('a')).toBe('A');
    expect(lru.get('c')).toBe('C');
    expect(lru.get('d')).toBe('D');

    // Global iconCache stays strictly bounded at 384 entries
    clearIconCache();
    for (let i = 0; i < 420; i++) {
      iconCache.set(`exe:k${i}`, `data:image/png;base64,${i}`);
    }
    expect(iconCache.size).toBe(ICON_CACHE_MAX_ENTRIES);
    expect(iconCache.has('exe:k0')).toBe(false);
    expect(iconCache.has('exe:k35')).toBe(false);
    expect(iconCache.has('exe:k36')).toBe(true);
    expect(iconCache.has('exe:k419')).toBe(true);
  });
});

describe('normalizeStartStatePayload and typed Wave 1B IPC wrappers', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it('normalizes both legacy boolean and structured StartStateEvent payloads', () => {
    expect(normalizeStartStatePayload(true)).toEqual({
      open: true,
      gen: 0,
      monitor: null,
      immediate: false,
    });
    expect(normalizeStartStatePayload(false)).toEqual({
      open: false,
      gen: 0,
      monitor: null,
      immediate: false,
    });
    expect(
      normalizeStartStatePayload({ open: true, gen: 9, monitor: 'secondary', immediate: false }),
    ).toEqual({
      open: true,
      gen: 9,
      monitor: 'secondary',
      immediate: false,
    });
    expect(normalizeStartStatePayload({ open: false, gen: 10, immediate: true })).toEqual({
      open: false,
      gen: 10,
      monitor: null,
      immediate: true,
    });
  });

  it('serializes Paint-Sync, audio/media, window productivity, clipboard, and config IPC commands with exact camelCase args', async () => {
    await surfaceCommitAck('flyout', 7, 240);
    expect(invokeMock).toHaveBeenLastCalledWith('surface_commit_ack', {
      surface: 'flyout',
      gen: 7,
      measuredHeight: 240,
    });

    await surfaceExitAck('start', 8);
    expect(invokeMock).toHaveBeenLastCalledWith('surface_exit_ack', { surface: 'start', gen: 8 });

    await dismissOverlays();
    expect(invokeMock).toHaveBeenLastCalledWith('dismiss_overlays', undefined);

    await toggleStart(true, 'secondary');
    expect(invokeMock).toHaveBeenLastCalledWith('toggle_start', {
      force: true,
      monitor: 'secondary',
    });

    await getAudioPeaks();
    expect(invokeMock).toHaveBeenLastCalledWith('get_audio_peaks', undefined);

    await setInputDevice('mic-1');
    expect(invokeMock).toHaveBeenLastCalledWith('set_input_device', { deviceId: 'mic-1' });

    await setSessionState({ appKey: 'chrome.exe', muted: true });
    expect(invokeMock).toHaveBeenLastCalledWith('set_session_state', {
      appKey: 'chrome.exe',
      volume: null,
      muted: true,
      pid: null,
      pids: null,
    });

    await setSessionState({ appKey: 'chrome.exe', volume: 0.01, pid: 4242, pids: [4242, 4243] });
    expect(invokeMock).toHaveBeenLastCalledWith('set_session_state', {
      appKey: 'chrome.exe',
      volume: 0.01,
      muted: null,
      pid: 4242,
      pids: [4242, 4243],
    });

    await seekMedia(42.5);
    expect(invokeMock).toHaveBeenLastCalledWith('seek_media', { positionSec: 42.5 });

    await selectMediaSession('spotify.exe');
    expect(invokeMock).toHaveBeenLastCalledWith('select_media_session', {
      sessionId: 'spotify.exe',
    });

    await snapWindow(1234, 'left');
    expect(invokeMock).toHaveBeenLastCalledWith('snap_window', { hwnd: 1234, zone: 'left' });

    await toggleWindowTopmost(1234);
    expect(invokeMock).toHaveBeenLastCalledWith('toggle_window_topmost', { hwnd: 1234 });

    await minimizeOtherWindows(1234);
    expect(invokeMock).toHaveBeenLastCalledWith('minimize_other_windows', { keepHwnd: 1234 });

    await launchNewWindow('C:\\Apps\\Code.exe', '--profile Dev', true);
    expect(invokeMock).toHaveBeenLastCalledWith('launch_new_window', {
      exePath: 'C:\\Apps\\Code.exe',
      args: '--profile Dev',
      closeStart: true,
    });

    await deleteClipboardEntry('clip-1');
    expect(invokeMock).toHaveBeenLastCalledWith('delete_clipboard_entry', { id: 'clip-1' });

    await setClipboardPaused(true);
    expect(invokeMock).toHaveBeenLastCalledWith('set_clipboard_paused', { paused: true });

    await getClipboardPaused();
    expect(invokeMock).toHaveBeenLastCalledWith('get_clipboard_paused', undefined);

    await pasteClipboardEntry('hello\nworld');
    expect(invokeMock).toHaveBeenLastCalledWith('paste_clipboard_entry', { text: 'hello\nworld' });

    await emptyRecycleBin();
    expect(invokeMock).toHaveBeenLastCalledWith('empty_recycle_bin', undefined);

    await openScreenSnip();
    expect(invokeMock).toHaveBeenLastCalledWith('open_screen_snip', undefined);

    await loadGameCover('C:\\Steam\\cover.jpg');
    expect(invokeMock).toHaveBeenLastCalledWith('load_game_cover', {
      coverPath: 'C:\\Steam\\cover.jpg',
    });

    await refreshIndexes();
    expect(invokeMock).toHaveBeenLastCalledWith('refresh_indexes', undefined);

    await updateCustomSpace({
      id: 'custom-1',
      name: 'Media',
      accentHex: '#38BDF8',
      widgetPreset: 'audio_mic',
    });
    expect(invokeMock).toHaveBeenLastCalledWith('update_custom_space', {
      id: 'custom-1',
      name: 'Media',
      accent: null,
      accentHex: '#38BDF8',
      accentRgb: null,
      widgetPreset: 'audio_mic',
    });

    await deleteCustomSpace('custom-1');
    expect(invokeMock).toHaveBeenLastCalledWith('delete_custom_space', { id: 'custom-1' });

    await updatePin({ space: 'personal', pinId: 'zen', title: 'Zen', altAction: 'Приватное окно' });
    expect(invokeMock).toHaveBeenLastCalledWith('update_pin', {
      space: 'personal',
      pinId: 'zen',
      title: 'Zen',
      target: null,
      args: null,
      altAction: 'Приватное окно',
      altLabel: null,
    });

    await addGameCombo({ name: 'Game Combo', steps: ['steam://rungameid/730'] });
    expect(invokeMock).toHaveBeenLastCalledWith('add_game_combo', {
      name: 'Game Combo',
      subtitle: null,
      accent: null,
      steps: ['steam://rungameid/730'],
    });

    await updateGameCombo({ id: 'combo-1', name: 'Game Combo Updated' });
    expect(invokeMock).toHaveBeenLastCalledWith('update_game_combo', {
      id: 'combo-1',
      name: 'Game Combo Updated',
      subtitle: null,
      accent: null,
      steps: null,
    });

    await deleteGameCombo('combo-1');
    expect(invokeMock).toHaveBeenLastCalledWith('delete_game_combo', { id: 'combo-1' });

    await setAppearanceSettings({ glassDepth: 'crystal', clockShowSeconds: true });
    expect(invokeMock).toHaveBeenLastCalledWith('set_appearance_settings', {
      glassDepth: 'crystal',
      clockShowSeconds: true,
      clockShowWeekday: null,
      startRememberSpace: null,
    });

    await hideTrayApp('steam.exe');
    expect(invokeMock).toHaveBeenLastCalledWith('hide_tray_app', { key: 'steam.exe' });

    await restoreTrayApp('steam.exe');
    expect(invokeMock).toHaveBeenLastCalledWith('restore_tray_app', { key: 'steam.exe' });

    await restoreAllTrayApps();
    expect(invokeMock).toHaveBeenLastCalledWith('restore_all_tray_apps', undefined);

    await recordFrecency('app:vscode');
    expect(invokeMock).toHaveBeenLastCalledWith('record_frecency', { itemId: 'app:vscode' });

    await exportConfigDialog();
    expect(invokeMock).toHaveBeenLastCalledWith('export_config_dialog', undefined);

    await importConfigDialog();
    expect(invokeMock).toHaveBeenLastCalledWith('import_config_dialog', undefined);
  });

  it('serializes Wave 3 V3 space, taskbar widget, pin v2, explorer, weather, and battery IPC commands', async () => {
    await createSpace({
      name: 'Design',
      template: 'studio',
      accentHex: '#34D399',
      widgets: ['weather', 'batteries'],
    });
    expect(invokeMock).toHaveBeenLastCalledWith('create_space', {
      name: 'Design',
      template: 'studio',
      accentHex: '#34D399',
      accentRgb: null,
      widgets: ['weather', 'batteries'],
    });

    await updateSpace({
      id: 'design',
      name: 'Design Pro',
      layoutKind: 'grid',
      showHeroRow: true,
    });
    expect(invokeMock).toHaveBeenLastCalledWith('update_space', {
      id: 'design',
      name: 'Design Pro',
      accentHex: null,
      accentRgb: null,
      layoutKind: 'grid',
      showHeroRow: true,
      widgets: null,
    });

    await deleteSpace('design');
    expect(invokeMock).toHaveBeenLastCalledWith('delete_space', { id: 'design' });

    await reorderSpaces(['work', 'personal']);
    expect(invokeMock).toHaveBeenLastCalledWith('reorder_spaces', {
      orderedIds: ['work', 'personal'],
    });

    await setSpaceWidgets('work', ['pc_health', 'weather']);
    expect(invokeMock).toHaveBeenLastCalledWith('set_space_widgets', {
      id: 'work',
      widgets: ['pc_health', 'weather'],
    });

    // Omitted weatherLat/weatherLon stay omitted so Rust PatchArg::Unset preserves coords
    await setTaskbarWidgets({ showBatteries: true, showTelemetry: true });
    expect(invokeMock).toHaveBeenLastCalledWith('set_taskbar_widgets', {
      showWeather: null,
      showBatteries: true,
      showTelemetry: true,
      weatherCity: null,
      weatherUnits: null,
    });

    // Explicit null clears saved coordinates
    await setTaskbarWidgets({ weatherCity: '', weatherLat: null, weatherLon: null });
    expect(invokeMock).toHaveBeenLastCalledWith('set_taskbar_widgets', {
      showWeather: null,
      showBatteries: null,
      showTelemetry: null,
      weatherCity: '',
      weatherLat: null,
      weatherLon: null,
      weatherUnits: null,
    });

    await updatePin({
      space: 'personal',
      pinId: 'pin-1',
      customIcon: 'custom_image',
      customColor: null,
      iconData: 'data:image/png;base64,AAA',
    });
    expect(invokeMock).toHaveBeenLastCalledWith('update_pin', {
      space: 'personal',
      pinId: 'pin-1',
      title: null,
      target: null,
      args: null,
      altAction: null,
      altLabel: null,
      customIcon: 'custom_image',
      customColor: null,
      iconData: 'data:image/png;base64,AAA',
    });

    await pickCustomIconImage();
    expect(invokeMock).toHaveBeenLastCalledWith('pick_custom_icon_image', undefined);

    await listDirectory('C:\\Users', { sortBy: 'date', sortDir: 'desc', showHidden: true });
    expect(invokeMock).toHaveBeenLastCalledWith('list_directory', {
      path: 'C:\\Users',
      sortBy: 'date',
      sortDir: 'desc',
      showHidden: true,
    });

    await getFileThumbnails(['C:\\a.png'], 64);
    expect(invokeMock).toHaveBeenLastCalledWith('get_file_thumbnails', {
      paths: ['C:\\a.png'],
      size: 64,
    });

    await fsOpenItem('C:\\a.png');
    expect(invokeMock).toHaveBeenLastCalledWith('fs_open_item', { path: 'C:\\a.png' });

    await fsCreateFolder('C:\\Temp', 'Sub');
    expect(invokeMock).toHaveBeenLastCalledWith('fs_create_folder', {
      parentPath: 'C:\\Temp',
      name: 'Sub',
    });

    await fsRenameItem('C:\\Temp\\Sub', 'Sub2');
    expect(invokeMock).toHaveBeenLastCalledWith('fs_rename_item', {
      path: 'C:\\Temp\\Sub',
      newName: 'Sub2',
    });

    await fsCopyFilesToClipboard(['C:\\a.png']);
    expect(invokeMock).toHaveBeenLastCalledWith('fs_copy_files_to_clipboard', {
      paths: ['C:\\a.png'],
    });

    await fsTrashItems(['C:\\a.png']);
    expect(invokeMock).toHaveBeenLastCalledWith('fs_trash_items', { paths: ['C:\\a.png'] });

    await fsOpenInExplorer('C:\\a.png');
    expect(invokeMock).toHaveBeenLastCalledWith('fs_open_in_explorer', { path: 'C:\\a.png' });

    await getWeather({ city: 'Москва', lat: 55.75, lon: 37.62, force: true });
    expect(invokeMock).toHaveBeenLastCalledWith('get_weather', {
      city: 'Москва',
      lat: 55.75,
      lon: 37.62,
      force: true,
    });

    await searchWeatherCities('Казань');
    expect(invokeMock).toHaveBeenLastCalledWith('search_weather_cities', { query: 'Казань' });

    await getPeripheralBatteries();
    expect(invokeMock).toHaveBeenLastCalledWith('get_peripheral_batteries', undefined);

    await refreshPeripheralBatteries();
    expect(invokeMock).toHaveBeenLastCalledWith('refresh_peripheral_batteries', undefined);
  });

  it('serializes Wave 5A v3 pin layout/admin, space bottomSectionTitle, movePinInSpace, and battery hide/restore commands', async () => {
    const size: TileSize = 'hero';
    const section: TileSection = 'main';
    await updatePin({
      space: 'personal',
      pinId: 'pin-v3',
      tileSize: size,
      section,
      runAsAdmin: true,
    });
    expect(invokeMock).toHaveBeenLastCalledWith('update_pin', {
      space: 'personal',
      pinId: 'pin-v3',
      title: null,
      target: null,
      args: null,
      altAction: null,
      altLabel: null,
      tileSize: 'hero',
      section: 'main',
      runAsAdmin: true,
    });

    await updatePin({
      space: 'personal',
      pinId: 'pin-v3',
      tileSize: null,
      section: null,
      runAsAdmin: false,
    });
    expect(invokeMock).toHaveBeenLastCalledWith('update_pin', {
      space: 'personal',
      pinId: 'pin-v3',
      title: null,
      target: null,
      args: null,
      altAction: null,
      altLabel: null,
      tileSize: null,
      section: null,
      runAsAdmin: false,
    });

    await updateSpace({
      id: 'personal',
      bottomSectionTitle: 'Быстрые папки',
    });
    expect(invokeMock).toHaveBeenLastCalledWith('update_space', {
      id: 'personal',
      name: null,
      accentHex: null,
      accentRgb: null,
      layoutKind: null,
      showHeroRow: null,
      widgets: null,
      bottomSectionTitle: 'Быстрые папки',
    });

    await updateSpace({
      id: 'personal',
      bottomSectionTitle: null,
    });
    expect(invokeMock).toHaveBeenLastCalledWith('update_space', {
      id: 'personal',
      name: null,
      accentHex: null,
      accentRgb: null,
      layoutKind: null,
      showHeroRow: null,
      widgets: null,
      bottomSectionTitle: null,
    });

    await movePinInSpace({
      space: 'personal',
      fromId: 'p-1',
      toId: 'p-2',
      targetSection: 'bottom',
    });
    expect(invokeMock).toHaveBeenLastCalledWith('move_pin_in_space', {
      space: 'personal',
      fromId: 'p-1',
      toId: 'p-2',
      targetSection: 'bottom',
    });

    await movePinInSpace({
      space: 'personal',
      fromId: 'p-2',
    });
    expect(invokeMock).toHaveBeenLastCalledWith('move_pin_in_space', {
      space: 'personal',
      fromId: 'p-2',
      toId: null,
      targetSection: null,
    });

    await hideBatteryDevice('sony:0ce6');
    expect(invokeMock).toHaveBeenLastCalledWith('hide_battery_device', { id: 'sony:0ce6' });

    await restoreBatteryDevice('sony:0ce6');
    expect(invokeMock).toHaveBeenLastCalledWith('restore_battery_device', { id: 'sony:0ce6' });

    await restoreAllBatteryDevices();
    expect(invokeMock).toHaveBeenLastCalledWith('restore_all_battery_devices', undefined);

    // Wave 1 V3.2 IPC commands
    await setSpaceWidgetLayout('work', ['media', 'notes'], [{ id: 'weather', side: 'left' }]);
    expect(invokeMock).toHaveBeenLastCalledWith('set_space_widget_layout', {
      id: 'work',
      widgets: ['media', 'notes'],
      headerWidgets: [{ id: 'weather', side: 'left' }],
    });

    await setTaskbarWidgets({
      leftWidgets: ['weather', 'network', 'timer'],
      widgetsPosition: 'left',
    });
    expect(invokeMock).toHaveBeenLastCalledWith('set_taskbar_widgets', {
      showWeather: null,
      showBatteries: null,
      showTelemetry: null,
      leftWidgets: ['weather', 'network', 'timer'],
      widgetsPosition: 'left',
      weatherCity: null,
      weatherUnits: null,
    });

    await setAppearanceSettings({
      themePreset: 'carbon',
      glassDepth: 'solid',
      accentMode: 'custom',
      accentHex: '#38BDF8',
      surfaceGradients: false,
      hoverGlow: false,
      clockDateFormat: 'short',
      clockLayout: 'center',
      defaultTileSize: 'small',
      resetVisuals: false,
    });
    expect(invokeMock).toHaveBeenLastCalledWith('set_appearance_settings', {
      glassDepth: 'solid',
      clockShowSeconds: null,
      clockShowWeekday: null,
      startRememberSpace: null,
      themePreset: 'carbon',
      accentMode: 'custom',
      accentHex: '#38BDF8',
      surfaceGradients: false,
      hoverGlow: false,
      clockDateFormat: 'short',
      clockLayout: 'center',
      defaultTileSize: 'small',
      resetVisuals: false,
    });

    await saveQuickNotesText('hello');
    expect(invokeMock).toHaveBeenLastCalledWith('save_quick_notes_text', { text: 'hello' });
    await addQuickTodo('task');
    expect(invokeMock).toHaveBeenLastCalledWith('add_quick_todo', { text: 'task' });
    await toggleQuickTodo('todo-1');
    expect(invokeMock).toHaveBeenLastCalledWith('toggle_quick_todo', { id: 'todo-1' });
    await deleteQuickTodo('todo-1');
    expect(invokeMock).toHaveBeenLastCalledWith('delete_quick_todo', { id: 'todo-1' });
    await clearDoneQuickTodos();
    expect(invokeMock).toHaveBeenLastCalledWith('clear_done_quick_todos', undefined);

    await getNetworkSnapshot(true);
    expect(invokeMock).toHaveBeenLastCalledWith('get_network_snapshot', { probePing: true });
    await getQuickTogglesState();
    expect(invokeMock).toHaveBeenLastCalledWith('get_quick_toggles_state', undefined);
    await setKeepAwake(true);
    expect(invokeMock).toHaveBeenLastCalledWith('set_keep_awake', { enable: true });
    await setPowerPlan('balanced');
    expect(invokeMock).toHaveBeenLastCalledWith('set_power_plan', { plan: 'balanced' });
    await triggerScreenshot();
    expect(invokeMock).toHaveBeenLastCalledWith('trigger_screenshot', undefined);
    await getAppVersion();
    expect(invokeMock).toHaveBeenLastCalledWith('get_app_version', undefined);
    await checkForUpdates();
    expect(invokeMock).toHaveBeenLastCalledWith('check_for_updates', undefined);
    await installUpdate();
    expect(invokeMock).toHaveBeenLastCalledWith('install_update', undefined);
  });

  it('binds onEvent strictly to getCurrentWebviewWindow().listen() to prevent cross-window event clobbering', async () => {
    windowListenMock.mockReset();
    globalListenMock.mockReset();
    getCurrentWebviewWindowMock.mockReset();

    const unlistenWin = vi.fn();
    windowListenMock.mockResolvedValue(unlistenWin);
    getCurrentWebviewWindowMock.mockReturnValue({ listen: windowListenMock });

    const g = globalThis as unknown as { window?: Record<string, unknown> };
    const hadWindow = typeof g.window !== 'undefined';
    if (!hadWindow) {
      g.window = { __TAURI_INTERNALS__: {} };
    } else {
      g.window!.__TAURI_INTERNALS__ = {};
    }

    try {
      const received: string[] = [];
      const unlisten = await onEvent<string>('windows-updated', (p) => received.push(p));
      expect(getCurrentWebviewWindowMock).toHaveBeenCalledTimes(1);
      expect(windowListenMock).toHaveBeenCalledWith('windows-updated', expect.any(Function));
      expect(globalListenMock).not.toHaveBeenCalled();
      expect(unlisten).toBe(unlistenWin);

      // Verify payload unwrapping
      const handler = windowListenMock.mock.calls[0][1] as (e: { payload: string }) => void;
      handler({ payload: 'primary-only' });
      expect(received).toEqual(['primary-only']);

      // Fallback cleanly to global listen only if getCurrentWebviewWindow() throws
      const unlistenGlobal = vi.fn();
      globalListenMock.mockResolvedValue(unlistenGlobal);
      getCurrentWebviewWindowMock.mockImplementation(() => {
        throw new Error('no webview window');
      });
      const fallbackUnlisten = await onEvent<string>('layout-updated', () => {});
      expect(globalListenMock).toHaveBeenCalledWith('layout-updated', expect.any(Function));
      expect(fallbackUnlisten).toBe(unlistenGlobal);
    } finally {
      if (!hadWindow) {
        delete g.window;
      } else {
        delete g.window!.__TAURI_INTERNALS__;
      }
    }
  });
});
