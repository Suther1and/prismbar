import { describe, expect, it } from 'vitest';
import {
  applyConfigUpdate,
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
});
