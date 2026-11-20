import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  applyConfigUpdate,
  clearIconCache,
  fetchIconsBatch,
  iconCache,
  isCacheIconKey,
  mergeTaskbarItems,
  shouldApplyConfigUpdate,
  type AppWindowGroup,
  type PinItem,
  type TestBarConfig,
} from './ipc';

function makeConfig(revision: number, barMode: 'dock' | 'edge' = 'dock'): TestBarConfig {
  return {
    schemaVersion: 1,
    revision,
    barMode,
    secondaryTaskbarMode: 'testbar',
    autostart: false,
    taskbarPins: [],
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
    expect(merged[2].pinned).toBe(false);
    expect(merged[2].name).toBe('Параметры');
    expect(merged[2].iconData).toBe('data:image/png;base64,SETTINGS');
    expect(merged[2].cacheIconKey).toBe('aumid:settings');
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
});
