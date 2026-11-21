import { describe, expect, it } from 'vitest';
import type { MergedBarItem } from './ipc';
import {
  buildAppContextMenuRequest,
  buildBarContextMenuRequest,
  buildPinFromAppContext,
  buildPreviewFlyoutRequest,
  buildVolumeOsdRequest,
  computeDockLogicalWidth,
  computeNextWheelVolume,
  computePreviewCardWidth,
  formatSoundNetworkTooltip,
  formatTaskbarClock,
  getPreviewHoverDelayMs,
  getTaskbarItemStateClass,
  getTaskbarRootClassName,
  isSoundFlyoutActive,
  planSoundFeedbackAction,
  PREVIEW_HOVER_COLD_MS,
  PREVIEW_HOVER_WARM_MS,
  resolvePinnedReorderIndices,
  resolveTaskbarMonitor,
  selectWindowToFocusOnTaskbarClick,
  shouldOpenVolumeOsd,
  shouldTriggerPreviewHover,
  VOLUME_OSD_HEIGHT,
  VOLUME_OSD_WIDTH,
  type AppContextPayload,
  type PreviewFlyoutPayload,
} from './taskbarLogic';

function makeRunningItem(overrides: Partial<MergedBarItem> = {}): MergedBarItem {
  return {
    id: 'run-code',
    name: 'VS Code',
    target: 'C:\\Program Files\\VS Code\\Code.exe',
    args: '--new-window',
    matchExe: 'code.exe',
    matchAumid: null,
    iconKey: 'exe:1111222233334444',
    cacheIconKey: 'exe:1111222233334444',
    iconData: null,
    pinned: false,
    group: {
      key: 'c:\\program files\\vs code\\code.exe',
      name: 'VS Code',
      exePath: 'C:\\Program Files\\VS Code\\Code.exe',
      aumid: null,
      iconKey: 'exe:1111222233334444',
      iconData: null,
      isActive: true,
      windows: [
        {
          hwnd: 101,
          pid: 5500,
          title: 'testbar — VS Code',
          exePath: 'C:\\Program Files\\VS Code\\Code.exe',
          exeName: 'VS Code',
          aumid: null,
          isActive: true,
          isMinimized: false,
        },
        {
          hwnd: 102,
          pid: 5500,
          title: 'notes — VS Code',
          exePath: 'C:\\Program Files\\VS Code\\Code.exe',
          exeName: 'VS Code',
          aumid: null,
          isActive: false,
          isMinimized: false,
        },
      ],
    },
    ...overrides,
  };
}

describe('TaskbarView Dock/Edge layout, states, cycling & pin DnD helpers', () => {
  it('resolves primary vs secondary monitor from query string', () => {
    expect(resolveTaskbarMonitor('?view=taskbar')).toBe('primary');
    expect(resolveTaskbarMonitor('?view=taskbar&monitor=primary')).toBe('primary');
    expect(resolveTaskbarMonitor('?view=taskbar&monitor=secondary')).toBe('secondary');
    expect(resolveTaskbarMonitor('')).toBe('primary');
  });

  it('computes Dock and Edge root classes accurately', () => {
    expect(getTaskbarRootClassName('dock')).toBe('tb-root');
    expect(getTaskbarRootClassName(undefined)).toBe('tb-root');
    expect(getTaskbarRootClassName('edge')).toBe('tb-root edge');
  });

  it('computes unconstrained dock logical width from max(scrollWidth, rectWidth)', () => {
    expect(computeDockLogicalWidth(742.2, 604)).toBe(743);
    expect(computeDockLogicalWidth(500, 512.4)).toBe(513);
    expect(computeDockLogicalWidth(0, 0)).toBeNull();
    expect(computeDockLogicalWidth(180, 200)).toBeNull();
    expect(computeDockLogicalWidth(Number.NaN, 420)).toBe(420);
  });

  it('computes active, running, minimized & idle indicator classes', () => {
    const active = makeRunningItem();
    expect(getTaskbarItemStateClass(active)).toBe('active');

    const running = makeRunningItem({
      group: { ...active.group!, isActive: false },
    });
    expect(getTaskbarItemStateClass(running)).toBe('running');

    const allMinimized = makeRunningItem({
      group: {
        ...active.group!,
        isActive: false,
        windows: active.group!.windows.map((w) => ({ ...w, isActive: false, isMinimized: true })),
      },
    });
    expect(getTaskbarItemStateClass(allMinimized)).toBe('minimized');

    const idle = makeRunningItem({ group: undefined });
    expect(getTaskbarItemStateClass(idle)).toBe('idle');
  });

  it('cycles multi-window groups cleanly on taskbar click without stranding windows #2..N', () => {
    const item = makeRunningItem();
    const wins = item.group!.windows;

    // Single window -> toggles focus/minimize
    expect(selectWindowToFocusOnTaskbarClick([wins[0]])).toEqual({
      hwnd: 101,
      toggleMinimize: true,
    });

    // Multi-window with #0 active -> cycles to #1 (no minimize)
    expect(selectWindowToFocusOnTaskbarClick(wins)).toEqual({
      hwnd: 102,
      toggleMinimize: false,
    });

    // Multi-window with last window active -> wraps back to #0
    const lastActive = [
      { ...wins[0], isActive: false },
      { ...wins[1], isActive: true },
    ];
    expect(selectWindowToFocusOnTaskbarClick(lastActive)).toEqual({
      hwnd: 101,
      toggleMinimize: false,
    });

    // Multi-window with none active -> picks first non-minimized window
    const noneActive = [
      { ...wins[0], isActive: false, isMinimized: true },
      { ...wins[1], isActive: false, isMinimized: false },
    ];
    expect(selectWindowToFocusOnTaskbarClick(noneActive)).toEqual({
      hwnd: 102,
      toggleMinimize: false,
    });

    expect(selectWindowToFocusOnTaskbarClick([])).toBeNull();
  });

  it('resolves drag-and-drop pin indices strictly across pinned taskbar items', () => {
    const items = [
      { id: 'pin-a', pinned: true },
      { id: 'pin-b', pinned: true },
      { id: 'pin-c', pinned: true },
      { id: 'run-x', pinned: false },
    ];

    expect(resolvePinnedReorderIndices(items, 'pin-a', 'pin-c')).toEqual({
      fromIdx: 0,
      toIdx: 2,
    });
    expect(resolvePinnedReorderIndices(items, 'pin-b', 'pin-a')).toEqual({
      fromIdx: 1,
      toIdx: 0,
    });
    // Rejects same item or unpinned running items
    expect(resolvePinnedReorderIndices(items, 'pin-a', 'pin-a')).toBeNull();
    expect(resolvePinnedReorderIndices(items, 'run-x', 'pin-a')).toBeNull();
    expect(resolvePinnedReorderIndices(items, 'pin-a', 'run-x')).toBeNull();
  });

  it('formats taskbar clock with optional seconds, weekday & full Russian date tooltip', () => {
    // 2026-11-21 09:05:07 is Saturday
    const d = new Date(2026, 10, 21, 9, 5, 7);
    expect(formatTaskbarClock(d)).toEqual({
      timeStr: '09:05',
      dateStr: '21.11.2026',
      fullTitle: 'Суббота, 21 ноября 2026',
    });
    expect(formatTaskbarClock(d, { showSeconds: true, showWeekday: true })).toEqual({
      timeStr: '09:05:07',
      dateStr: 'Сб, 21.11.2026',
      fullTitle: 'Суббота, 21 ноября 2026',
    });

    expect(
      formatSoundNetworkTooltip(
        {
          available: true,
          masterVolume: 68,
          masterMuted: false,
          micMuted: false,
          defaultDeviceShortName: 'Наушники',
        },
        { connected: true, kind: 'Ethernet', speedLabel: '1 Гбит/с' },
      ),
    ).toBe('Ethernet · 1 Гбит/с · Звук (Наушники): 68%');

    expect(
      formatSoundNetworkTooltip(
        {
          available: true,
          masterVolume: 40,
          masterMuted: true,
          micMuted: false,
          defaultDeviceShortName: '',
        },
        { connected: false, kind: 'Offline', speedLabel: '' },
      ),
    ).toBe('Нет сети · Звук: Выкл');
  });
});

describe('Warm preview scrubbing, hover preemption & flyout payload builders', () => {
  it('uses 180ms cold delay and drops to 25ms warm delay during preview scrub & 300ms post-close grace', () => {
    expect(PREVIEW_HOVER_COLD_MS).toBe(180);
    expect(PREVIEW_HOVER_WARM_MS).toBe(25);

    // Cold hover -> 180ms
    expect(getPreviewHoverDelayMs('', null, 1000)).toBe(180);
    expect(getPreviewHoverDelayMs('', 500, 1000)).toBe(180);

    // Active preview -> 25ms instant scrub
    expect(getPreviewHoverDelayMs('preview', null, 1000)).toBe(25);

    // Within 300ms after preview closed -> 25ms warm handoff
    expect(getPreviewHoverDelayMs('', 800, 1000)).toBe(25);
    expect(getPreviewHoverDelayMs('', 700, 1000)).toBe(25);
    expect(getPreviewHoverDelayMs('', 699, 1000)).toBe(180);
  });

  it('preempts hover preview when Start or interactive click flyouts are open', () => {
    const running = makeRunningItem();
    const idle = makeRunningItem({ group: undefined });

    expect(shouldTriggerPreviewHover(running, false, '')).toBe(true);
    expect(shouldTriggerPreviewHover(running, false, 'preview')).toBe(true);
    expect(shouldTriggerPreviewHover(running, false, 'osd')).toBe(true);

    expect(shouldTriggerPreviewHover(idle, false, '')).toBe(false);
    expect(shouldTriggerPreviewHover(running, true, '')).toBe(false);

    for (const clickFlyout of ['tray', 'sound', 'quick', 'calendar', 'context_app', 'context_bar']) {
      expect(shouldTriggerPreviewHover(running, false, clickFlyout)).toBe(false);
    }
  });

  it('builds gapless non-activating preview requests carrying monitor anchor & +N overflow width', () => {
    const item = makeRunningItem();
    const req = buildPreviewFlyoutRequest(item, 320, 'secondary');
    expect(req.kind).toBe('preview');
    expect(req.noActivate).toBe(true);
    expect(req.anchorInTaskbar).toBe(true);
    expect(req.anchorXLogical).toBe(320);
    expect(req.monitor).toBe('secondary');
    expect(req.width).toBe(2 * 214 + 20);
    expect(req.height).toBe(166);
    expect((req.payload as PreviewFlyoutPayload).iconKey).toBe('exe:1111222233334444');

    // 4 cards vs 6 cards (4 visible + 53px for +2 overflow badge)
    expect(computePreviewCardWidth(4)).toBe(4 * 214 + 20);
    expect(computePreviewCardWidth(6)).toBe(4 * 214 + 20 + 53);
  });

  it('builds app context menu payload preserving matchExe, matchAumid, iconKey & args', () => {
    const uwpItem = makeRunningItem({
      id: 'run-uwp',
      name: 'Терминал',
      target: 'C:\\Program Files\\WindowsApps\\WT\\WindowsTerminal.exe',
      args: null,
      matchExe: 'windowsterminal.exe',
      matchAumid: 'Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
      iconKey: 'aumid:abcd1234',
      cacheIconKey: 'aumid:abcd1234',
      pinned: false,
      group: {
        key: 'aumid:microsoft.windowsterminal_8wekyb3d8bbwe!app',
        name: 'Терминал',
        exePath: 'C:\\Program Files\\WindowsApps\\WT\\WindowsTerminal.exe',
        aumid: 'Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
        iconKey: 'aumid:abcd1234',
        iconData: null,
        isActive: false,
        windows: [
          {
            hwnd: 500,
            pid: 777,
            title: 'PowerShell',
            exePath: 'C:\\Program Files\\WindowsApps\\WT\\WindowsTerminal.exe',
            exeName: 'Терминал',
            aumid: 'Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
            isActive: false,
            isMinimized: false,
          },
        ],
      },
    });

    const req = buildAppContextMenuRequest(uwpItem, 180, 'primary');
    expect(req.kind).toBe('context_app');
    expect(req.noActivate).toBe(false);
    expect(req.monitor).toBe('primary');

    const payload = req.payload as AppContextPayload;
    expect(payload.matchExe).toBe('windowsterminal.exe');
    expect(payload.aumid).toBe('Microsoft.WindowsTerminal_8wekyb3d8bbwe!App');
    expect(payload.iconKey).toBe('aumid:abcd1234');
    expect(payload.windows).toHaveLength(1);

    const pin = buildPinFromAppContext(payload, 'tb-fixed');
    expect(pin.id).toBe('tb-fixed');
    expect(pin.kind).toBe('app');
    expect(pin.title).toBe('Терминал');
    expect(pin.matchExe).toBe('windowsterminal.exe');
    expect(pin.matchAumid).toBe('Microsoft.WindowsTerminal_8wekyb3d8bbwe!App');
    expect(pin.iconKey).toBe('aumid:abcd1234');
  });

  it('builds empty-bar context menu payload for both dock and edge modes', () => {
    const dockReq = buildBarContextMenuRequest('dock', 512, 'primary');
    expect(dockReq.kind).toBe('context_bar');
    expect(dockReq.noActivate).toBe(false);
    expect(dockReq.payload).toEqual({ barMode: 'dock' });

    const edgeReq = buildBarContextMenuRequest('edge', 900, 'secondary');
    expect(edgeReq.monitor).toBe('secondary');
    expect(edgeReq.payload).toEqual({ barMode: 'edge' });
  });
});

describe('Volume OSD preemption vs open Quick Sound & wheel volume math', () => {
  it('never opens OSD when Quick Sound or another interactive flyout is already open (AD-1)', () => {
    expect(isSoundFlyoutActive('sound')).toBe(true);
    expect(isSoundFlyoutActive('quick')).toBe(true);
    expect(isSoundFlyoutActive('osd')).toBe(false);

    expect(shouldOpenVolumeOsd('')).toBe(true);
    expect(shouldOpenVolumeOsd('preview')).toBe(true);
    expect(shouldOpenVolumeOsd('osd')).toBe(true);

    for (const active of ['sound', 'quick', 'tray', 'calendar', 'context_app', 'context_bar']) {
      expect(shouldOpenVolumeOsd(active)).toBe(false);
    }

    const inPlace = planSoundFeedbackAction('sound', 72, false, 640, 'primary');
    expect(inPlace).toEqual({ openOsd: false, volume: 72, muted: false, req: null });

    const osdPlan = planSoundFeedbackAction('', 72, false, 640, 'secondary');
    expect(osdPlan.openOsd).toBe(true);
    expect(osdPlan.req).toEqual({
      kind: 'osd',
      anchorXLogical: 640,
      anchorInTaskbar: true,
      width: VOLUME_OSD_WIDTH,
      height: VOLUME_OSD_HEIGHT,
      noActivate: true,
      monitor: 'secondary',
      payload: { volume: 72, muted: false },
    });
  });

  it('clamps OSD volume and computes mouse-wheel steps cleanly', () => {
    const clampedHigh = buildVolumeOsdRequest(140, false, 300, 'primary');
    expect(clampedHigh.payload).toEqual({ volume: 100, muted: false });

    const clampedLow = buildVolumeOsdRequest(-15, true, 300, 'primary');
    expect(clampedLow.payload).toEqual({ volume: 0, muted: true });

    expect(computeNextWheelVolume(40, -120)).toBe(44);
    expect(computeNextWheelVolume(40, 120)).toBe(36);
    expect(computeNextWheelVolume(0, 120)).toBe(0);
    expect(computeNextWheelVolume(100, -120)).toBe(100);
  });
});
