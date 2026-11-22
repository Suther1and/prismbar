import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import flyoutViewSrc from '../components/FlyoutView.tsx?raw';
import { TaskbarView } from '../components/TaskbarView';
import taskbarViewSrc from '../components/TaskbarView.tsx?raw';
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
  DEFAULT_FOCUS_TIMER_SNAPSHOT,
  EDGE_CENTERED_MIN_WIDTH,
  filterGroupsForMonitor,
  formatBatteryPillSummary,
  formatFocusTimerMmSs,
  formatMiniTelemetryLabel,
  formatNetworkRateCompact,
  formatSoundNetworkTooltip,
  formatTaskbarClock,
  formatTaskbarMediaLabel,
  formatTaskbarNetworkBadge,
  formatTelemetryGb,
  formatWeatherTempLabel,
  getFocusTimerEffectiveRemainingSec,
  getPreviewHoverDelayMs,
  getTaskbarItemStateClass,
  getTaskbarRootClassName,
  getTelemetryUsageTone,
  isInteractiveFlyoutActive,
  isSoundFlyoutActive,
  NETWORK_FLYOUT_HEIGHT,
  NETWORK_FLYOUT_WIDTH,
  parseFocusTimerSnapshot,
  planSoundFeedbackAction,
  PREVIEW_CARD_HEIGHT,
  reorderUnpinnedIds,
  resolveHorizontalDropTarget,
  PREVIEW_HOVER_COLD_MS,
  PREVIEW_HOVER_WARM_MS,
  resolveActiveTaskbarWidgets,
  resolvePinnedReorderIndices,
  resolveTaskbarItemClick,
  resolveTaskbarMonitor,
  resolveTaskbarWidgetsPosition,
  selectTaskbarPinsForMonitor,
  selectWindowToFocusOnTaskbarClick,
  shouldOpenVolumeOsd,
  shouldTriggerPreviewHover,
  shouldUseEdgeCenteredLayout,
  SOUND_FLYOUT_WIDTH,
  stabilizeTaskbarItemsOrder,
  TELEMETRY_FLYOUT_HEIGHT,
  TELEMETRY_FLYOUT_WIDTH,
  TIMER_STORAGE_KEY,
  TIMER_UPDATED_EVENT,
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
          title: 'workspace — VS Code',
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

  it('formats taskbar clock across all dateFormat x showWeekday x layout permutations', () => {
    // 2026-11-21 09:05:07 is Saturday
    const d = new Date(2026, 10, 21, 9, 5, 7);

    // Defaults: dateFormat='short', layout='center', showWeekday=false, showSeconds=false
    expect(formatTaskbarClock(d)).toEqual({
      timeStr: '09:05',
      dateStr: '21 ноя',
      singleStr: '21 ноя · 09:05',
      layout: 'center',
      dateFormat: 'short',
      timeOnly: false,
      fullTitle: 'Суббота, 21 ноября 2026',
    });

    const expectedDates: Record<
      'short' | 'numeric' | 'none',
      Record<'false' | 'true', string>
    > = {
      short: { false: '21 ноя', true: 'Сб, 21 ноя' },
      numeric: { false: '21.11.2026', true: 'Сб, 21.11' },
      none: { false: '', true: '' },
    };

    for (const dateFormat of ['short', 'numeric', 'none'] as const) {
      for (const showWeekday of [false, true] as const) {
        for (const layout of ['center', 'right', 'single'] as const) {
          const expectedDate = expectedDates[dateFormat][String(showWeekday) as 'false' | 'true'];
          const res = formatTaskbarClock(d, {
            showSeconds: dateFormat === 'none' && showWeekday,
            showWeekday,
            dateFormat,
            layout,
          });
          const expectedTime = dateFormat === 'none' && showWeekday ? '09:05:07' : '09:05';
          expect(res.timeStr).toBe(expectedTime);
          expect(res.dateStr).toBe(expectedDate);
          expect(res.layout).toBe(layout);
          expect(res.dateFormat).toBe(dateFormat);
          expect(res.timeOnly).toBe(dateFormat === 'none');
          expect(res.singleStr).toBe(
            expectedDate ? `${expectedDate} · ${expectedTime}` : expectedTime,
          );
          expect(res.fullTitle).toBe('Суббота, 21 ноября 2026');
        }
      }
    }

    // Specific May formatting check (month index 4 -> 'мая')
    expect(formatTaskbarClock(new Date(2026, 4, 5, 22, 41), { showWeekday: true }).singleStr).toBe(
      'Вт, 5 мая · 22:41',
    );

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
    expect(pin.target).toBe('shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App');
    expect(pin.matchExe).toBe('windowsterminal.exe');
    expect(pin.matchAumid).toBe('Microsoft.WindowsTerminal_8wekyb3d8bbwe!App');
    expect(pin.iconKey).toBe('aumid:abcd1234');

    // Non-launchable UWP host executables (SystemSettings.exe / ApplicationFrameHost.exe / \WindowsApps\)
    // must ALWAYS resolve to shell:AppsFolder\<aumid> when aumid is present
    const settingsPin = buildPinFromAppContext(
      {
        id: 'run-settings',
        name: 'Параметры',
        target: 'C:\\Windows\\ImmersiveControlPanel\\SystemSettings.exe',
        args: null,
        matchExe: 'systemsettings.exe',
        aumid: 'windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
        iconKey: 'aumid:settings',
        pinned: false,
        windows: [],
      },
      'tb-settings',
    );
    expect(settingsPin.target).toBe(
      'shell:AppsFolder\\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
    );
    expect(settingsPin.matchAumid).toBe(
      'windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
    );

    const afhPin = buildPinFromAppContext(
      {
        id: 'run-calc',
        name: 'Калькулятор',
        target: 'C:\\Windows\\System32\\ApplicationFrameHost.exe',
        args: null,
        matchExe: 'applicationframehost.exe',
        aumid: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
        iconKey: 'aumid:calc',
        pinned: false,
        windows: [],
      },
      'tb-calc',
    );
    expect(afhPin.target).toBe('shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App');
    expect(afhPin.matchExe).toBeNull();
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
      key: 'c:\\users\\user\\appdata\\local\\programs\\sparkdesktop\\spark desktop.exe',
      name: 'Spark Desktop',
      exePath: 'C:\\Users\\User\\AppData\\Local\\Programs\\SparkDesktop\\Spark Desktop.exe',
      aumid: null,
      iconKey: 'exe:spark1234',
      iconData: null,
      isActive: false,
      windows: [
        {
          hwnd: 909,
          pid: 4242,
          title: 'Inbox — Spark',
          exePath: 'C:\\Users\\User\\AppData\\Local\\Programs\\SparkDesktop\\Spark Desktop.exe',
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

describe('V3.2 Taskbar widgets zone, network/media/timer formatters & network/telemetry flyouts', () => {
  it('registers network and telemetry flyouts with exact dimensions and OSD/preview preemption', () => {
    expect(NETWORK_FLYOUT_WIDTH).toBe(316);
    expect(NETWORK_FLYOUT_HEIGHT).toBe(276);
    expect(TELEMETRY_FLYOUT_WIDTH).toBe(324);
    expect(TELEMETRY_FLYOUT_HEIGHT).toBe(336);

    expect(isInteractiveFlyoutActive('network')).toBe(true);
    expect(isInteractiveFlyoutActive('telemetry')).toBe(true);
    expect(shouldOpenVolumeOsd('network')).toBe(false);
    expect(shouldOpenVolumeOsd('telemetry')).toBe(false);

    const running = makeRunningItem();
    expect(shouldTriggerPreviewHover(running, false, 'network')).toBe(false);
    expect(shouldTriggerPreviewHover(running, false, 'telemetry')).toBe(false);
  });

  it('resolves taskbar widgets from leftWidgets or legacy boolean fallback and widgetsPosition', () => {
    expect(resolveActiveTaskbarWidgets(null)).toEqual([]);
    expect(resolveActiveTaskbarWidgets(undefined)).toEqual([]);
    expect(resolveTaskbarWidgetsPosition(null)).toBe('left');
    expect(resolveTaskbarWidgetsPosition({ widgetsPosition: 'left' })).toBe('left');
    expect(resolveTaskbarWidgetsPosition({ widgetsPosition: 'right' })).toBe('right');

    // Legacy boolean fallback when leftWidgets is missing or empty
    expect(
      resolveActiveTaskbarWidgets({
        showWeather: true,
        showBatteries: false,
        showTelemetry: true,
        leftWidgets: [],
      }),
    ).toEqual(['weather', 'telemetry']);

    // Non-empty leftWidgets overrides booleans, preserves order, deduplicates & drops unknown IDs
    expect(
      resolveActiveTaskbarWidgets({
        showWeather: true,
        showBatteries: true,
        showTelemetry: false,
        leftWidgets: [
          'network',
          'media',
          'timer',
          'network' as never,
          'invalid' as never,
        ],
      }),
    ).toEqual(['network', 'media', 'timer']);

    // computeNaturalDockWidth accounts for optional left widgets zone
    expect(computeNaturalDockWidth(598, 600, 400, 200, 180)).toBe(815);
  });

  it('formats compact network rates and taskbar network badge across online/offline/wifi/tx states', () => {
    expect(formatNetworkRateCompact(null)).toBe('0 Б/с');
    expect(formatNetworkRateCompact(-50)).toBe('0 Б/с');
    expect(formatNetworkRateCompact(Number.NaN)).toBe('0 Б/с');
    expect(formatNetworkRateCompact(512)).toBe('512 Б/с');
    expect(formatNetworkRateCompact(1536)).toBe('1.5 КБ/с');
    expect(formatNetworkRateCompact(245 * 1024)).toBe('245 КБ/с');
    expect(formatNetworkRateCompact(12.4 * 1024 * 1024)).toBe('12.4 МБ/с');
    expect(formatNetworkRateCompact(125 * 1024 * 1024)).toBe('125 МБ/с');
    expect(formatNetworkRateCompact(1.5 * 1024 * 1024 * 1024)).toBe('1.5 ГБ/с');

    expect(formatTaskbarNetworkBadge(null)).toEqual({
      online: true,
      kind: 'ethernet',
      text: 'Сеть',
      rateStr: '—',
      pingStr: null,
      ariaLabel: 'Состояние сети',
    });

    expect(
      formatTaskbarNetworkBadge({
        online: false,
        kind: 'offline',
        adapterName: 'Нет подключения',
        localIp: '',
        rxBytesPerSec: 1000,
        txBytesPerSec: 1000,
        pingMs: 12,
        pingHost: '1.1.1.1',
      }),
    ).toEqual({
      online: false,
      kind: 'offline',
      text: 'Нет сети',
      rateStr: '0 Б/с',
      pingStr: null,
      ariaLabel: 'Нет подключения к сети',
    });

    const ethBadge = formatTaskbarNetworkBadge({
      online: true,
      kind: 'ethernet',
      adapterName: 'Ethernet',
      localIp: '192.168.1.42',
      rxBytesPerSec: 2 * 1024 * 1024,
      txBytesPerSec: 256 * 1024,
      pingMs: 18,
      pingHost: '1.1.1.1',
    });
    expect(ethBadge.kind).toBe('ethernet');
    expect(ethBadge.text).toBe('↓ 2.0 МБ/с · 18 мс');
    expect(ethBadge.pingStr).toBe('18 мс');

    const wifiTxBadge = formatTaskbarNetworkBadge({
      online: true,
      kind: 'Wi-Fi',
      adapterName: 'Intel Wi-Fi 6E',
      localIp: '192.168.1.50',
      rxBytesPerSec: 120 * 1024,
      txBytesPerSec: 5 * 1024 * 1024,
      pingMs: null,
      pingHost: '1.1.1.1',
    });
    expect(wifiTxBadge.kind).toBe('wifi');
    expect(wifiTxBadge.text).toBe('↑ 5.0 МБ/с');
    expect(wifiTxBadge.pingStr).toBeNull();
  });

  it('formats compact media pill labels and self-contained focus timer snapshots', () => {
    expect(formatTaskbarMediaLabel(null)).toEqual({
      hasSession: false,
      isPlaying: false,
      title: 'Нет медиа',
      ariaLabel: 'Медиа не воспроизводится',
    });

    expect(
      formatTaskbarMediaLabel(
        {
          hasSession: true,
          isPlaying: true,
          title: '  Very   Long   Synthwave   Track  Title  Here  ',
          artist: 'Artist',
          source: 'Spotify',
        },
        18,
      ),
    ).toEqual({
      hasSession: true,
      isPlaying: true,
      title: 'Very Long Synthwa…',
      ariaLabel: 'Играет: Very Long Synthwave Track Title Here — Artist',
    });

    expect(
      formatTaskbarMediaLabel({
        hasSession: true,
        isPlaying: false,
        title: '',
        artist: 'Radiohead',
        source: 'Yandex Music',
      }),
    ).toEqual({
      hasSession: true,
      isPlaying: false,
      title: 'Radiohead',
      ariaLabel: 'Пауза: Radiohead — Radiohead',
    });

    expect(TIMER_STORAGE_KEY).toBe('testbar.focus_timer.v1');
    expect(TIMER_UPDATED_EVENT).toBe('testbar:focus-timer-updated');
    expect(parseFocusTimerSnapshot(null)).toEqual(DEFAULT_FOCUS_TIMER_SNAPSHOT);
    expect(parseFocusTimerSnapshot('{invalid')).toEqual(DEFAULT_FOCUS_TIMER_SNAPSHOT);

    const parsed = parseFocusTimerSnapshot(
      JSON.stringify({
        running: true,
        totalSec: 1500,
        remainingSec: 1200,
        endEpochMs: 100_000,
      }),
    );
    expect(getFocusTimerEffectiveRemainingSec(parsed, 75_500)).toBe(25);
    expect(getFocusTimerEffectiveRemainingSec(parsed, 120_000)).toBe(0);
    expect(formatFocusTimerMmSs(0)).toBe('00:00');
    expect(formatFocusTimerMmSs(65)).toBe('01:05');
    expect(formatFocusTimerMmSs(1500)).toBe('25:00');

    expect(formatTelemetryGb(15.96)).toBe('16');
    expect(formatTelemetryGb(12.34)).toBe('12.3');
    expect(getTelemetryUsageTone(45)).toBe('ok');
    expect(getTelemetryUsageTone(75)).toBe('warn');
    expect(getTelemetryUsageTone(90)).toBe('hot');
  });

  it('enforces clean single sound icon, refined clock attrs, hoverGlow guard & network/telemetry flyouts', () => {
    const html = renderToStaticMarkup(createElement(TaskbarView));
    expect(html).toContain('data-clock-layout="center"');
    expect(html).toContain('tb-clock-date');
    expect(html).toContain('tb-sound-btn');
    expect(html).not.toContain('tb-net-ic');
    expect(html).not.toContain('tb-net-offline-btn');
    expect(html).not.toContain('tb-timer-pill');

    expect(taskbarViewSrc).toContain('OfflineIcon');
    expect(taskbarViewSrc).toContain("dataset.hoverGlow === 'off'");
    expect(taskbarViewSrc).toContain('resolveActiveTaskbarWidgets');
    expect(taskbarViewSrc).toContain('resolveTaskbarWidgetsPosition');
    expect(taskbarViewSrc).toContain("'telemetry'");
    expect(taskbarViewSrc).toContain("'network'");
    expect(taskbarViewSrc).toContain('TIMER_STORAGE_KEY');
    expect(taskbarViewSrc).toContain('TIMER_UPDATED_EVENT');

    expect(flyoutViewSrc).toContain("req.kind === 'network'");
    expect(flyoutViewSrc).toContain("req.kind === 'telemetry'");
    expect(flyoutViewSrc).toContain('getNetworkSnapshot');
    expect(flyoutViewSrc).toContain('Проверить');
    expect(flyoutViewSrc).toContain('Локальный IPv4');
    expect(flyoutViewSrc).toContain('Монитор ПК');
  });

  it('reorders unpinned open app IDs and resolves horizontal drop targets with drift tolerance', () => {
    expect(reorderUnpinnedIds(['a', 'b', 'c', 'd'], 'a', 'c')).toEqual(['b', 'c', 'a', 'd']);
    expect(reorderUnpinnedIds(['a', 'b', 'c', 'd'], 'd', 'b')).toEqual(['a', 'd', 'b', 'c']);
    expect(reorderUnpinnedIds(['a', 'b'], 'a', 'a')).toEqual(['a', 'b']);
    expect(reorderUnpinnedIds(['a', 'b'], 'missing', 'b')).toEqual(['a', 'b']);

    const boxes = [
      { id: 'a', left: 10, right: 50 },
      { id: 'b', left: 54, right: 94 },
      { id: 'c', left: 98, right: 138 },
    ];
    // Staying inside source box cancels drop target
    expect(resolveHorizontalDropTarget(boxes, 70, 'b')).toBeNull();
    // Moving left past 'b' highlights 'a'
    expect(resolveHorizontalDropTarget(boxes, 20, 'b')).toEqual({
      targetId: 'a',
      side: 'before',
    });
    // Clamping outside right edge still snaps cleanly to rightmost candidate
    expect(resolveHorizontalDropTarget(boxes, 500, 'a')).toEqual({
      targetId: 'c',
      side: 'after',
    });
    // Clamping outside left edge snaps cleanly to leftmost candidate
    expect(resolveHorizontalDropTarget(boxes, -20, 'c')).toEqual({
      targetId: 'a',
      side: 'before',
    });
  });
});
