import { describe, expect, it } from 'vitest';
import { mergeTaskbarItems, type AppWindowGroup, type MergedBarItem } from './ipc';
import {
  BATTERY_FLYOUT_HEIGHT,
  BATTERY_FLYOUT_WIDTH,
  buildAppContextMenuRequest,
  buildBarContextMenuRequest,
  buildPinFromAppContext,
  buildPreviewFlyoutRequest,
  buildVolumeOsdRequest,
  computeDockLogicalWidth,
  computeNaturalDockWidth,
  computeNextWheelVolume,
  computePreviewCardWidth,
  createTaskbarOrderMemory,
  EDGE_CENTERED_MIN_WIDTH,
  filterGroupsForMonitor,
  formatBatteryPillSummary,
  formatMiniTelemetryLabel,
  formatSoundNetworkTooltip,
  formatTaskbarClock,
  formatWeatherTempLabel,
  getPreviewHoverDelayMs,
  getTaskbarItemStateClass,
  getTaskbarRootClassName,
  isInteractiveFlyoutActive,
  isSoundFlyoutActive,
  planSoundFeedbackAction,
  PREVIEW_CARD_HEIGHT,
  PREVIEW_HOVER_COLD_MS,
  PREVIEW_HOVER_WARM_MS,
  resolvePinnedReorderIndices,
  resolveTaskbarItemClick,
  resolveTaskbarMonitor,
  selectTaskbarPinsForMonitor,
  selectWindowToFocusOnTaskbarClick,
  shouldOpenVolumeOsd,
  shouldTriggerPreviewHover,
  shouldUseEdgeCenteredLayout,
  SOUND_FLYOUT_WIDTH,
  stabilizeTaskbarItemsOrder,
  VOLUME_OSD_HEIGHT,
  VOLUME_OSD_WIDTH,
  WEATHER_FLYOUT_HEIGHT,
  WEATHER_FLYOUT_WIDTH,
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

  it('computes unconstrained dock logical width from max(scrollWidth, rectWidth) & child groups', () => {
    expect(SOUND_FLYOUT_WIDTH).toBe(336);
    expect(computeDockLogicalWidth(742.2, 604)).toBe(743);
    expect(computeDockLogicalWidth(500, 512.4)).toBe(513);
    expect(computeDockLogicalWidth(134, 130)).toBe(134); // compact secondary monitor dock
    expect(computeDockLogicalWidth(0, 0)).toBeNull();
    expect(computeDockLogicalWidth(80, 96)).toBeNull();
    expect(computeDockLogicalWidth(Number.NaN, 420)).toBe(420);

    // Unconstrained apps + tray + chrome prevents yellow-sliver clipping even if bar was narrower
    expect(computeNaturalDockWidth(598, 600, 540, 225)).toBe(786);
    expect(computeNaturalDockWidth(700, 702, 400, 200)).toBe(702);
  });

  it('enforces Option A secondary monitor contract (no primary pins, strict per-monitor window filtering)', () => {
    const samplePin = {
      id: 'p1',
      kind: 'app' as const,
      title: 'VS Code',
      subtitle: 'Code',
      target: 'C:\\Apps\\Code.exe',
      altAction: '',
      accentRgb: '129,140,248',
      iconKey: 'code',
    };
    expect(selectTaskbarPinsForMonitor([samplePin], 'primary')).toHaveLength(1);
    expect(selectTaskbarPinsForMonitor([samplePin], 'secondary')).toEqual([]);

    const baseGroup = makeRunningItem().group!;
    const mixedGroups = [
      {
        ...baseGroup,
        isActive: true,
        windows: [
          { ...baseGroup.windows[0], hwnd: 101, isActive: true, monitor: 'primary' },
          { ...baseGroup.windows[1], hwnd: 102, isActive: false, monitor: 'secondary' },
        ],
      },
      {
        ...baseGroup,
        key: 'legacy-untagged',
        isActive: false,
        windows: [{ ...baseGroup.windows[1], hwnd: 201, isActive: false, monitor: undefined }],
      },
    ];
    const primaryOnly = filterGroupsForMonitor(mixedGroups, 'primary');
    expect(primaryOnly).toHaveLength(2);
    expect(primaryOnly[0].windows.map((w) => w.hwnd)).toEqual([101]);
    expect(primaryOnly[0].isActive).toBe(true);

    const secondaryOnly = filterGroupsForMonitor(mixedGroups, 'secondary');
    expect(secondaryOnly).toHaveLength(1);
    expect(secondaryOnly[0].windows.map((w) => w.hwnd)).toEqual([102]);
    expect(secondaryOnly[0].isActive).toBe(false);
  });

  it('applies edge-centered layout strictly on wide primary displays (never secondary/narrow vertical)', () => {
    expect(EDGE_CENTERED_MIN_WIDTH).toBe(1180);
    expect(shouldUseEdgeCenteredLayout('edge', 'primary', 1920)).toBe(true);
    expect(shouldUseEdgeCenteredLayout('edge', 'primary', 1180)).toBe(true);
    expect(shouldUseEdgeCenteredLayout('edge', 'primary', 1080)).toBe(false);
    expect(shouldUseEdgeCenteredLayout('edge', 'secondary', 1920)).toBe(false);
    expect(shouldUseEdgeCenteredLayout('edge', 'secondary', 1080)).toBe(false);
    expect(shouldUseEdgeCenteredLayout('dock', 'primary', 1920)).toBe(false);
  });

  it('stabilizes unpinned icon slots and multi-window preview order across Z-order clicks', () => {
    const mem = createTaskbarOrderMemory();
    const pinA = makeRunningItem({ id: 'pin-a', name: 'Pin A', pinned: true, group: undefined });
    const unpinned1 = makeRunningItem({
      id: 'run-settings',
      name: 'Settings',
      pinned: false,
      group: {
        ...makeRunningItem().group!,
        key: 'settings',
        windows: [
          { ...makeRunningItem().group!.windows[0], hwnd: 10, isActive: false },
          { ...makeRunningItem().group!.windows[1], hwnd: 20, isActive: true },
        ],
      },
    });
    const unpinned2 = makeRunningItem({
      id: 'run-spotify',
      name: 'Spotify',
      pinned: false,
      group: { ...makeRunningItem().group!, key: 'spotify', windows: [makeRunningItem().group!.windows[0]] },
    });

    // Initial order: pinA, Settings (hw 10, 20), Spotify
    const first = stabilizeTaskbarItemsOrder([pinA, unpinned1, unpinned2], mem);
    expect(first.map((i) => i.id)).toEqual(['pin-a', 'run-settings', 'run-spotify']);
    expect(first[1].group?.windows.map((w) => w.hwnd)).toEqual([10, 20]);

    // Simulated Z-order flip after clicking Spotify + flipping Settings window Z-order:
    // Order MUST NOT shuffle!
    const unpinned1Flipped = {
      ...unpinned1,
      group: {
        ...unpinned1.group!,
        windows: [unpinned1.group!.windows[1], unpinned1.group!.windows[0]],
      },
    };
    const afterClick = stabilizeTaskbarItemsOrder([pinA, unpinned2, unpinned1Flipped], mem);
    expect(afterClick.map((i) => i.id)).toEqual(['pin-a', 'run-settings', 'run-spotify']);
    expect(afterClick[1].group?.windows.map((w) => w.hwnd)).toEqual([10, 20]);

    // Closing Settings & opening Telegram appends Telegram strictly at the END
    const unpinned3 = makeRunningItem({
      id: 'run-telegram',
      name: 'Telegram',
      pinned: false,
      group: { ...makeRunningItem().group!, key: 'telegram', windows: [makeRunningItem().group!.windows[0]] },
    });
    const afterCloseAndOpen = stabilizeTaskbarItemsOrder([pinA, unpinned3, unpinned2], mem);
    expect(afterCloseAndOpen.map((i) => i.id)).toEqual(['pin-a', 'run-spotify', 'run-telegram']);
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
    expect(PREVIEW_CARD_HEIGHT).toBe(172);
    expect(req.width).toBe(2 * 214 + 20);
    expect(req.height).toBe(PREVIEW_CARD_HEIGHT);
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

  it('resolves cross-monitor pinned apps (e.g. Spark pinned on Monitor 1, running on Monitor 2) without duplicating unpinned slots', () => {
    const sparkPin = {
      id: 'spark',
      kind: 'app' as const,
      title: 'Spark Mail',
      subtitle: 'Почта',
      target: '',
      altAction: '',
      accentRgb: '56,189,248',
      iconKey: 'spark',
    };
    const sparkSecondaryGroup: AppWindowGroup = {
      key: 'c:\\users\\loki2\\appdata\\local\\programs\\sparkdesktop\\spark desktop.exe',
      name: 'Spark Desktop',
      exePath: 'C:\\Users\\loki2\\AppData\\Local\\Programs\\SparkDesktop\\Spark Desktop.exe',
      aumid: null,
      iconKey: 'exe:spark1234',
      iconData: null,
      isActive: false,
      windows: [
        {
          hwnd: 909,
          pid: 4242,
          title: 'Inbox — Spark',
          exePath: 'C:\\Users\\loki2\\AppData\\Local\\Programs\\SparkDesktop\\Spark Desktop.exe',
          exeName: 'Spark Desktop',
          aumid: null,
          isActive: false,
          isMinimized: false,
          monitor: 'secondary',
        },
      ],
    };
    const secondaryUnpinned: AppWindowGroup = {
      key: 'c:\\apps\\discord.exe',
      name: 'Discord',
      exePath: 'C:\\Apps\\Discord.exe',
      aumid: null,
      iconKey: 'exe:disc999',
      iconData: null,
      isActive: false,
      windows: [
        {
          hwnd: 910,
          pid: 4243,
          title: 'Discord',
          exePath: 'C:\\Apps\\Discord.exe',
          exeName: 'Discord',
          aumid: null,
          isActive: false,
          isMinimized: false,
          monitor: 'secondary',
        },
      ],
    };

    const allGroups = [sparkSecondaryGroup, secondaryUnpinned];
    const primaryVisible = filterGroupsForMonitor(allGroups, 'primary');
    expect(primaryVisible).toEqual([]);

    const merged = mergeTaskbarItems(
      [sparkPin],
      primaryVisible,
      { 'exe:spark1234': 'data:image/png;base64,SPARK' },
      allGroups,
    );

    // Monitor 2 unpinned Discord MUST NOT duplicate on Monitor 1!
    expect(merged).toHaveLength(1);
    const sparkItem = merged[0];
    expect(sparkItem.id).toBe('spark');
    expect(sparkItem.group).toBeUndefined();
    expect(sparkItem.remoteGroup?.key).toBe(sparkSecondaryGroup.key);
    expect(sparkItem.iconData).toBe('data:image/png;base64,SPARK');
    expect(sparkItem.target).toBe(sparkSecondaryGroup.exePath);

    // Left-clicking Spark on Monitor 1 focuses Spark's running window on Monitor 2 (without minimizing it)
    expect(resolveTaskbarItemClick(sparkItem)).toEqual({
      type: 'focus',
      hwnd: 909,
      toggleMinimize: false,
    });

    // Right-clicking Spark on Monitor 1 includes resolved target & remote windows so full menu appears
    const ctxReq = buildAppContextMenuRequest(sparkItem, 220, 'primary');
    const ctxPayload = ctxReq.payload as AppContextPayload;
    expect(ctxPayload.target).toBe(sparkSecondaryGroup.exePath);
    expect(ctxPayload.windows.map((w) => w.hwnd)).toEqual([909]);
  });
});

describe('Taskbar optional widgets: weather, peripheral batteries, mini telemetry & flyout preemption', () => {
  it('registers weather and battery as interactive flyouts with fixed dimensions', () => {
    expect(WEATHER_FLYOUT_WIDTH).toBe(340);
    expect(WEATHER_FLYOUT_HEIGHT).toBe(404);
    expect(BATTERY_FLYOUT_WIDTH).toBe(304);
    expect(BATTERY_FLYOUT_HEIGHT).toBe(250);

    expect(isInteractiveFlyoutActive('weather')).toBe(true);
    expect(isInteractiveFlyoutActive('battery')).toBe(true);
    expect(shouldOpenVolumeOsd('weather')).toBe(false);
    expect(shouldOpenVolumeOsd('battery')).toBe(false);

    const running = makeRunningItem();
    expect(shouldTriggerPreviewHover(running, false, 'weather')).toBe(false);
    expect(shouldTriggerPreviewHover(running, false, 'battery')).toBe(false);
  });

  it('formats Celsius and Fahrenheit temperature labels cleanly without -0 or jitter', () => {
    expect(formatWeatherTempLabel(14.2, 'c')).toBe('14°');
    expect(formatWeatherTempLabel(14.0, 'f')).toBe('57°');
    expect(formatWeatherTempLabel(0, 'f')).toBe('32°');
    expect(formatWeatherTempLabel(-0.3, 'c')).toBe('0°');
    expect(formatWeatherTempLabel(-10.6, 'c')).toBe('-11°');
    expect(formatWeatherTempLabel(null, 'c')).toBe('—°');
    expect(formatWeatherTempLabel(undefined, 'f')).toBe('—°');
    expect(formatWeatherTempLabel(Number.NaN, 'c')).toBe('—°');
  });

  it('summarizes peripheral batteries picking lowest active level with low/critical/charging flags', () => {
    expect(formatBatteryPillSummary(null)).toBeNull();
    expect(formatBatteryPillSummary([])).toBeNull();
    expect(
      formatBatteryPillSummary([
        {
          id: 'bt-offline',
          name: 'Headset',
          kind: 'headphones',
          percent: 50,
          charging: false,
          source: 'bluetooth',
          connected: false,
        },
      ]),
    ).toBeNull();

    const single = formatBatteryPillSummary([
      {
        id: 'razer-mouse',
        name: 'Razer Basilisk V3 Pro 35K',
        kind: 'mouse',
        percent: 78.6,
        charging: true,
        source: 'razer_hid',
        connected: true,
      },
    ]);
    expect(single).not.toBeNull();
    expect(single?.percent).toBe(79);
    expect(single?.percentStr).toBe('79%');
    expect(single?.charging).toBe(true);
    expect(single?.low).toBe(false);
    expect(single?.critical).toBe(false);
    expect(single?.count).toBe(1);
    expect(single?.ariaLabel).toContain('Razer Basilisk V3 Pro 35K: 79% (заряжается)');

    // Picks lowest % across connected devices, breaking ties in favor of non-charging
    const multi = formatBatteryPillSummary([
      {
        id: 'airpods',
        name: 'AirPods Max',
        kind: 'headphones',
        percent: 82,
        charging: false,
        source: 'bluetooth',
        connected: true,
      },
      {
        id: 'kbd',
        name: 'Keychron K2',
        kind: 'keyboard',
        percent: 15,
        charging: true,
        source: 'bluetooth',
        connected: true,
      },
      {
        id: 'mouse',
        name: 'Razer Mouse',
        kind: 'mouse',
        percent: 15,
        charging: false,
        source: 'razer_hid',
        connected: true,
      },
    ]);
    expect(multi?.device.id).toBe('mouse');
    expect(multi?.percentStr).toBe('15%');
    expect(multi?.charging).toBe(false);
    expect(multi?.low).toBe(true);
    expect(multi?.critical).toBe(false);
    expect(multi?.count).toBe(3);

    const critical = formatBatteryPillSummary([
      {
        id: 'pad',
        name: 'Xbox Controller',
        kind: 'gamepad',
        percent: 8,
        charging: false,
        source: 'xinput',
        connected: true,
      },
    ]);
    expect(critical?.low).toBe(true);
    expect(critical?.critical).toBe(true);
  });

  it('formats mini PC telemetry label with GPU temperature or RAM fallback', () => {
    expect(formatMiniTelemetryLabel(null)).toBe('CPU —%');
    expect(
      formatMiniTelemetryLabel({
        cpuPct: 14.4,
        ramPct: 48,
        gpuAvailable: true,
        gpuTempC: 46.2,
      }),
    ).toBe('CPU 14% · 46°');
    expect(
      formatMiniTelemetryLabel({
        cpuPct: 22.8,
        ramPct: 61.4,
        gpuAvailable: false,
        gpuTempC: 0,
      }),
    ).toBe('CPU 23% · RAM 61%');
  });
});
