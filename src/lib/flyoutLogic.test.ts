import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import {
  BellIcon,
  ChargingBoltIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  DropletIcon,
  NativeTrayIcon,
  PeripheralDeviceIcon,
  WeatherConditionIcon,
  WindIcon,
} from '../components/Icons';
import { TaskbarView } from '../components/TaskbarView';
import { StartSettingsOverlay } from '../components/start/StartSettingsOverlay';
import {
  isCacheIconKey,
  type AppWindowGroup,
  type AudioDeviceInfo,
  type FlyoutRequest,
  type OpenWindowInfo,
} from './ipc';
import {
  BATTERY_LIST_MAX_HEIGHT,
  canOpenFileLocation,
  canRunAsAdmin,
  collectAppContextPids,
  computeCardOriginX,
  computeFlyoutCardHeight,
  computeNextContextFocusIdx,
  computeTrayFlyoutHeight,
  computeWeatherTempBarRange,
  CONTEXT_FLYOUT_EXIT_MS,
  FLYOUT_EXIT_MS,
  FLYOUT_MAX_CARD_HEIGHT,
  FLYOUT_ROOT_PAD_LOGICAL,
  formatCityOptionSubtitle,
  formatIpcError,
  formatMediaTime,
  formatPeripheralSourceLabel,
  formatTrayHeaderLabel,
  formatWeatherWindLabel,
  getActiveDeviceIndex,
  getFlyoutExitMs,
  INITIAL_FLYOUT_ANIM_STATE,
  interpolateMediaPosition,
  isAppContextKind,
  isBarContextKind,
  OSD_AUTO_CLOSE_MS,
  partitionAudioDevices,
  PREVIEW_LEAVE_COMMAND,
  QS_MIXER_MAX_HEIGHT,
  reduceFlyoutHideComplete,
  reduceFlyoutHideImmediate,
  reduceFlyoutHideStart,
  reduceFlyoutShow,
  removePreviewWindow,
  resolveTrayHideKey,
  shouldShowAppContextActionSeparator,
  stepVolume,
  syncPreviewWindowsFromGroups,
  TRAY_CARD_WIDTH,
  TRAY_COLUMNS,
  TRAY_CTX_OPEN_MIN_HEIGHT,
  TRAY_MIN_CARD_HEIGHT,
} from './flyoutLogic';

function makeReq(
  kind: FlyoutRequest['kind'],
  overrides: Partial<FlyoutRequest> = {},
): FlyoutRequest {
  return {
    kind,
    anchorXLogical: 200,
    anchorInTaskbar: true,
    width: 240,
    height: 180,
    noActivate: kind === 'preview' || kind === 'osd',
    payload: {},
    ...overrides,
  };
}

function makeWin(hwnd: number, title: string, pid = 1000 + hwnd): OpenWindowInfo {
  return {
    hwnd,
    pid,
    title,
    exePath: 'C:\\Apps\\Code.exe',
    exeName: 'VS Code',
    aumid: null,
    isActive: false,
    isMinimized: false,
  };
}

describe('Flyout instant-open animation, height measurement & preview scrub preservation', () => {
  it('opens immediately at frame 0, bumps renderSeq on kind switches, and preserves card mount during preview scrub', () => {
    const p1 = makeReq('preview', {
      anchorXLogical: 180,
      payload: { group: { key: 'app-a', windows: [makeWin(1, 'A')] } },
    });
    const s1 = reduceFlyoutShow(INITIAL_FLYOUT_ANIM_STATE, p1);
    expect(s1.phase).toBe('open');
    expect(s1.renderSeq).toBe(1);
    expect(s1.scrubSeq).toBe(0);
    expect(s1.scrubDir).toBe(0);

    // Scrubbing rightward to App B keeps renderSeq = 1 (no flyIn re-bounce) and bumps scrubSeq with dir +1
    const p2 = makeReq('preview', {
      anchorXLogical: 260,
      payload: { group: { key: 'app-b', windows: [makeWin(2, 'B')] } },
    });
    const sScrubRight = reduceFlyoutShow(s1, p2);
    expect(sScrubRight.renderSeq).toBe(1);
    expect(sScrubRight.phase).toBe('open');
    expect(sScrubRight.scrubSeq).toBe(1);
    expect(sScrubRight.scrubDir).toBe(1);

    // Scrubbing leftward back to App A bumps scrubSeq with dir -1
    const sScrubLeft = reduceFlyoutShow(sScrubRight, p1);
    expect(sScrubLeft.renderSeq).toBe(1);
    expect(sScrubLeft.scrubSeq).toBe(2);
    expect(sScrubLeft.scrubDir).toBe(-1);

    // Switching straight to a different flyout kind bumps renderSeq and replays flyIn cleanly
    const s3 = reduceFlyoutShow(sScrubLeft, makeReq('context_app'));
    expect(s3.phase).toBe('open');
    expect(s3.renderSeq).toBe(2);
    expect(s3.scrubSeq).toBe(0);
    expect(s3.req?.kind).toBe('context_app');
  });

  it('plays closing phase with surface-specific exit timing and handles immediate preemption', () => {
    expect(FLYOUT_EXIT_MS).toBe(105);
    expect(CONTEXT_FLYOUT_EXIT_MS).toBe(95);
    expect(getFlyoutExitMs('sound')).toBe(105);
    expect(getFlyoutExitMs('context_app')).toBe(95);
    expect(getFlyoutExitMs('context_bar')).toBe(95);

    const open = reduceFlyoutShow(INITIAL_FLYOUT_ANIM_STATE, makeReq('sound'));
    const closing = reduceFlyoutHideStart(open);
    expect(closing.phase).toBe('closing');
    expect(closing.req?.kind).toBe('sound');
    expect(closing.renderSeq).toBe(1);

    // Duplicate hide during closing is idempotent
    expect(reduceFlyoutHideStart(closing)).toBe(closing);

    // Exit timer completes cleanly
    const closed = reduceFlyoutHideComplete(closing, 1);
    expect(closed.phase).toBe('closed');
    expect(closed.req).toBeNull();
    expect(closed.renderSeq).toBe(1);

    // Immediate preemption clears synchronously with zero stale content
    const immediate = reduceFlyoutHideImmediate(open);
    expect(immediate.phase).toBe('closed');
    expect(immediate.req).toBeNull();
  });

  it('rapid reopen during closing cancels pending exit cleanly (stale seq ignored)', () => {
    const open = reduceFlyoutShow(INITIAL_FLYOUT_ANIM_STATE, makeReq('preview'));
    const closing = reduceFlyoutHideStart(open);
    expect(closing.renderSeq).toBe(1);

    const reopened = reduceFlyoutShow(closing, makeReq('calendar'));
    expect(reopened.phase).toBe('open');
    expect(reopened.renderSeq).toBe(2);
    expect(reopened.req?.kind).toBe('calendar');

    // Stale hide timer for seq 1 MUST NOT wipe the reopened flyout!
    const afterStaleTimer = reduceFlyoutHideComplete(reopened, 1);
    expect(afterStaleTimer).toBe(reopened);
    expect(afterStaleTimer.phase).toBe('open');
    expect(afterStaleTimer.req?.kind).toBe('calendar');
  });

  it('computes clamped card-local transform-origin X from Rust anchorOriginX & natural card height up to 580px', () => {
    expect(FLYOUT_ROOT_PAD_LOGICAL).toBe(16);
    expect(FLYOUT_MAX_CARD_HEIGHT).toBe(580);
    // Window left has 16px DComp root pad; anchorOriginX = 116 -> local 100px
    expect(computeCardOriginX(116, 300)).toBe(100);
    // Clamps near left/right card edges
    expect(computeCardOriginX(10, 300)).toBe(16);
    expect(computeCardOriginX(500, 300)).toBe(284);
    expect(computeCardOriginX(null, 300)).toBeNull();

    // Expands Quick Sound card when inner mixer is constrained, capped at QS_MIXER_MAX_HEIGHT (220)
    expect(
      computeFlyoutCardHeight({
        cardScrollHeight: 338,
        borderY: 2,
        innerScrollHeight: 180,
        innerClientHeight: 80,
        innerMaxHeight: QS_MIXER_MAX_HEIGHT,
      }),
    ).toBe(440);
    expect(
      computeFlyoutCardHeight({
        cardScrollHeight: 338,
        borderY: 2,
        innerScrollHeight: 400,
        innerClientHeight: 80,
        innerMaxHeight: QS_MIXER_MAX_HEIGHT,
      }),
    ).toBe(480);
    // Clamps tall context menus cleanly to 580px ceiling
    expect(
      computeFlyoutCardHeight({
        cardScrollHeight: 318,
        borderY: 2,
        innerScrollHeight: 650,
        innerClientHeight: 280,
      }),
    ).toBe(580);
    expect(computeFlyoutCardHeight({ cardScrollHeight: 20 })).toBeNull();
  });
});

describe('Preview mouse-leave contract, TaskbarView SSR smoke & context menu guards', () => {
  it('uses close_preview_if_outside and renders upgraded TaskbarView capsule with zero native title tooltips', () => {
    expect(PREVIEW_LEAVE_COMMAND).toBe('close_preview_if_outside');

    const chevronHtml = renderToStaticMarkup(createElement(ChevronLeftIcon));
    expect(chevronHtml).toContain('<svg');
    expect(chevronHtml).toContain('<polyline');
    expect(chevronHtml).not.toContain('←');

    const tbHtml = renderToStaticMarkup(createElement(TaskbarView));
    expect(tbHtml).toContain('class="tb-root"');
    expect(tbHtml).not.toContain('data-glass=');
    expect(tbHtml).not.toContain('title="');
    expect(tbHtml).toContain('aria-label="Пуск"');
    expect(tbHtml).toContain('data-monitor="primary"');
    expect(tbHtml).toContain('tb-sheen');
    expect(tbHtml).toContain('tb-hover-pill');
    expect(tbHtml).toContain('tb-sep');
    expect(tbHtml).toContain('tb-weather-pill');
    expect(tbHtml).toContain('tb-widget-sep');
    expect(tbHtml).not.toContain('tb-tray-sep');
    expect(tbHtml).not.toContain('tb-multi');

    // Monitor 2 Option A: Start + Clock only (no tray chevron, lang pill, sound/net pill, or optional widgets)
    const secHtml = renderToStaticMarkup(
      createElement(TaskbarView, { monitorOverride: 'secondary' }),
    );
    expect(secHtml).toContain('data-monitor="secondary"');
    expect(secHtml).toContain('aria-label="Пуск"');
    expect(secHtml).toContain('tb-clock');
    expect(secHtml).not.toContain('tb-lang-btn');
    expect(secHtml).not.toContain('tb-sound-btn');
    expect(secHtml).not.toContain('tb-weather-pill');
    expect(secHtml).not.toContain('tb-bat-pill');
    expect(secHtml).not.toContain('tb-telem-pill');
    expect(secHtml).not.toContain('Фоновые приложения');

    // Win11 NotifyIconSettings tray icon keys resolve through IconCache batch fetcher
    expect(isCacheIconKey('tray:deadbeef1234')).toBe(true);
    expect(isCacheIconKey('exe:abcd')).toBe(true);
    expect(isCacheIconKey('unknown:1')).toBe(false);
  });

  it('updates preview window list on per-window close and monitor-aware windows-updated sync', () => {
    const wins = [makeWin(1, 'Tab 1'), makeWin(2, 'Tab 2')];
    const afterCloseOne = removePreviewWindow(wins, 1);
    expect(afterCloseOne).toHaveLength(1);
    expect(afterCloseOne[0].hwnd).toBe(2);

    const afterCloseLast = removePreviewWindow(afterCloseOne, 2);
    expect(afterCloseLast).toHaveLength(0);

    const groups: AppWindowGroup[] = [
      {
        key: 'c:\\apps\\code.exe',
        name: 'VS Code',
        exePath: 'C:\\Apps\\Code.exe',
        aumid: null,
        iconKey: 'exe:1',
        iconData: null,
        isActive: true,
        windows: [
          { ...makeWin(2, 'Tab 2'), monitor: 'primary' },
          { ...makeWin(3, 'Secondary Tab'), monitor: 'secondary' },
        ],
      },
    ];
    expect(syncPreviewWindowsFromGroups('c:\\apps\\code.exe', groups)).toHaveLength(2);
    expect(syncPreviewWindowsFromGroups('c:\\apps\\code.exe', groups, 'primary')?.map((w) => w.hwnd)).toEqual([2]);
    expect(syncPreviewWindowsFromGroups('c:\\apps\\code.exe', groups, 'secondary')?.map((w) => w.hwnd)).toEqual([3]);
    expect(syncPreviewWindowsFromGroups('c:\\apps\\gone.exe', groups)).toEqual([]);
    expect(syncPreviewWindowsFromGroups('', groups)).toBeNull();
  });

  it('validates file-location, run-as-admin, multi-PID group kill, dangling-separator guard & keyboard nav', () => {
    expect(canOpenFileLocation('C:\\Program Files\\VS Code\\Code.exe')).toBe(true);
    expect(canOpenFileLocation('\\\\server\\share\\app.exe')).toBe(true);
    expect(canOpenFileLocation('shell:AppsFolder\\Microsoft.Store!App')).toBe(false);
    expect(canOpenFileLocation('https://claude.ai')).toBe(false);
    expect(canOpenFileLocation('')).toBe(false);

    expect(canRunAsAdmin('C:\\Program Files\\VS Code\\Code.exe')).toBe(true);
    expect(canRunAsAdmin('https://claude.ai')).toBe(false);
    expect(canRunAsAdmin('shell:AppsFolder\\Microsoft.WindowsCalculator!App')).toBe(false);
    expect(canRunAsAdmin('ms-screenclip:')).toBe(false);

    expect(isAppContextKind('context_app')).toBe(true);
    expect(isAppContextKind('ctx_app')).toBe(true);
    expect(isAppContextKind('context_bar')).toBe(false);

    expect(isBarContextKind('context_bar')).toBe(true);
    expect(isBarContextKind('ctx_taskbar')).toBe(true);
    expect(isBarContextKind('preview')).toBe(false);

    expect(formatIpcError('Отказано в доступе')).toBe('Отказано в доступе');
    expect(formatIpcError(new Error('Access denied'))).toBe('Access denied');
    expect(formatIpcError(null)).toBe('Не удалось выполнить действие');

    // Collects unique valid PIDs across all windows in group
    expect(
      collectAppContextPids([
        makeWin(1, 'A', 400),
        makeWin(2, 'B', 401),
        makeWin(3, 'C', 400),
        makeWin(4, 'D', 0),
      ]),
    ).toEqual([400, 401]);

    // No dangling separator when top actions are empty
    expect(
      shouldShowAppContextActionSeparator({
        hasJumpList: false,
        hasLaunch: false,
        hasAdmin: false,
        hasLocation: false,
        hasPinSection: true,
      }),
    ).toBe(false);
    expect(
      shouldShowAppContextActionSeparator({
        hasJumpList: false,
        hasLaunch: true,
        hasAdmin: false,
        hasLocation: false,
        hasPinSection: true,
      }),
    ).toBe(true);

    // Circular ArrowUp / ArrowDown navigation
    expect(computeNextContextFocusIdx(-1, 1, 4)).toBe(0);
    expect(computeNextContextFocusIdx(-1, -1, 4)).toBe(3);
    expect(computeNextContextFocusIdx(3, 1, 4)).toBe(0);
    expect(computeNextContextFocusIdx(0, -1, 4)).toBe(3);
    expect(computeNextContextFocusIdx(0, 1, 0)).toBe(-1);
  });
});

describe('Tray 6-column layout, Hide key, OSD, 4+ Audio Devices & Media interpolation', () => {
  it('computes compact 6-column tray dimensions (128px floor, 184px while right-click menu open) and resolves hide keys', () => {
    expect(TRAY_COLUMNS).toBe(6);
    expect(TRAY_CARD_WIDTH).toBe(244);

    expect(formatTrayHeaderLabel('', 0)).toBe('Фоновые приложения');
    expect(formatTrayHeaderLabel('   ', 14)).toBe('Фоновые · 14');
    expect(formatTrayHeaderLabel('Telegram', 14)).toBe('Telegram');

    expect(TRAY_MIN_CARD_HEIGHT).toBe(128);
    expect(TRAY_CTX_OPEN_MIN_HEIGHT).toBe(184);
    expect(computeTrayFlyoutHeight(0)).toBe(TRAY_MIN_CARD_HEIGHT);
    expect(computeTrayFlyoutHeight(6)).toBe(TRAY_MIN_CARD_HEIGHT);
    expect(computeTrayFlyoutHeight(12)).toBe(TRAY_MIN_CARD_HEIGHT);
    expect(computeTrayFlyoutHeight(18)).toBe(160);
    expect(computeTrayFlyoutHeight(24)).toBe(198);
    expect(computeTrayFlyoutHeight(30)).toBe(236);
    expect(computeTrayFlyoutHeight(42)).toBe(274);
    expect(computeTrayFlyoutHeight(80)).toBe(274);

    // Dynamic expansion ONLY while tray right-click menu is open
    expect(computeTrayFlyoutHeight(6, true)).toBe(184);
    expect(computeTrayFlyoutHeight(18, true)).toBe(184);
    expect(computeTrayFlyoutHeight(24, true)).toBe(198);

    expect(
      resolveTrayHideKey({
        id: 'tray-1',
        name: 'Discord',
        exePath: 'C:\\Users\\user\\AppData\\Local\\Discord\\Discord.exe',
      }),
    ).toBe('discord.exe');
    expect(resolveTrayHideKey({ id: 'tray-helper', name: 'Helper', exePath: '' })).toBe('tray-helper');
  });

  it('updates consecutive OSD wheel ticks in place without bumping renderSeq, but replays enter after closing', () => {
    expect(OSD_AUTO_CLOSE_MS).toBe(1100);

    const openOsd = reduceFlyoutShow(INITIAL_FLYOUT_ANIM_STATE, makeReq('osd'));
    expect(openOsd.renderSeq).toBe(1);
    expect(openOsd.phase).toBe('open');

    const nextTickReq = { ...makeReq('osd'), payload: { volume: 64, muted: false } };
    const updated = reduceFlyoutShow(openOsd, nextTickReq);
    expect(updated.renderSeq).toBe(1);
    expect(updated.phase).toBe('open');
    expect(updated.req?.payload).toEqual({ volume: 64, muted: false });

    const closing = reduceFlyoutHideStart(updated);
    const reopened = reduceFlyoutShow(closing, nextTickReq);
    expect(reopened.renderSeq).toBe(2);
    expect(reopened.phase).toBe('open');
  });

  it('partitions 4+ audio output devices cleanly without dropping devices #4..N', () => {
    const devs: AudioDeviceInfo[] = [
      { id: 'd1', name: 'Speakers', shortName: 'Колонки', isDefault: false },
      { id: 'd2', name: 'Headset', shortName: 'Наушники', isDefault: false },
      { id: 'd3', name: 'Monitor HDMI', shortName: 'Монитор', isDefault: false },
      { id: 'd4', name: 'Bluetooth DAC', shortName: 'BT DAC', isDefault: true },
      { id: 'd5', name: 'Virtual Cable', shortName: 'Cable', isDefault: false },
    ];

    const part = partitionAudioDevices(devs, 3);
    expect(part.primary).toHaveLength(3);
    expect(part.overflow).toHaveLength(2);
    expect(part.activeInPrimaryIdx).toBe(-1);
    expect(part.activeInOverflow).toBe(true);

    // Cross-file compatible getActiveDeviceIndex
    expect(
      getActiveDeviceIndex([
        { isDefault: false },
        { isDefault: true },
        { isDefault: false },
      ]),
    ).toBe(1);
    expect(getActiveDeviceIndex([{ isDefault: false }, { isDefault: false }])).toBe(0);
  });

  it('interpolates live SMTC media position, formats times & steps wheel volume cleanly', () => {
    expect(
      interpolateMediaPosition(
        { isPlaying: true, positionSec: 40, durationSec: 180, lastUpdatedMs: 10000 },
        14500,
      ),
    ).toBe(44.5);

    // Paused stays at exact reported position
    expect(
      interpolateMediaPosition(
        { isPlaying: false, positionSec: 40, durationSec: 180, lastUpdatedMs: 10000 },
        20000,
      ),
    ).toBe(40);

    // Clamps to durationSec
    expect(
      interpolateMediaPosition(
        { isPlaying: true, positionSec: 175, durationSec: 180, lastUpdatedMs: 10000 },
        25000,
      ),
    ).toBe(180);

    expect(formatMediaTime(0)).toBe('0:00');
    expect(formatMediaTime(65.9)).toBe('1:05');
    expect(formatMediaTime(3612)).toBe('60:12');

    expect(stepVolume(50, -100, 4)).toBe(54);
    expect(stepVolume(50, 100, 4)).toBe(46);
    expect(stepVolume(98, -100, 4)).toBe(100);
    expect(stepVolume(2, 100, 4)).toBe(0);

    for (const Icon of [ChevronRightIcon, BellIcon, NativeTrayIcon]) {
      const html = renderToStaticMarkup(createElement(Icon));
      expect(html).toContain('<svg');
    }
  });
});

describe('Weather & Battery flyout logic, formatters, temperature range bars & icon SVG smoke', () => {
  it('handles weather and battery flyout lifecycle transitions and dynamic list height caps', () => {
    expect(getFlyoutExitMs('weather')).toBe(FLYOUT_EXIT_MS);
    expect(getFlyoutExitMs('battery')).toBe(FLYOUT_EXIT_MS);
    expect(BATTERY_LIST_MAX_HEIGHT).toBe(360);

    const weatherOpen = reduceFlyoutShow(
      INITIAL_FLYOUT_ANIM_STATE,
      makeReq('weather', { width: 340, height: 404 }),
    );
    expect(weatherOpen.phase).toBe('open');
    expect(weatherOpen.renderSeq).toBe(1);

    const batteryOpen = reduceFlyoutShow(
      weatherOpen,
      makeReq('battery', { width: 304, height: 250 }),
    );
    expect(batteryOpen.phase).toBe('open');
    expect(batteryOpen.renderSeq).toBe(2);
    expect(batteryOpen.req?.kind).toBe('battery');

    const closing = reduceFlyoutHideStart(batteryOpen);
    const closed = reduceFlyoutHideComplete(closing, 2);
    expect(closed.phase).toBe('closed');
    expect(closed.req).toBeNull();

    // Battery list expands up to BATTERY_LIST_MAX_HEIGHT without overflowing card ceiling
    expect(
      computeFlyoutCardHeight({
        cardScrollHeight: 248,
        borderY: 2,
        innerScrollHeight: 420,
        innerClientHeight: 180,
        innerMaxHeight: BATTERY_LIST_MAX_HEIGHT,
      }),
    ).toBe(430);
  });

  it('formats peripheral source badges, wind speed, city subtitles & daily temperature range bars', () => {
    expect(formatPeripheralSourceLabel('bluetooth')).toBe('Bluetooth');
    expect(formatPeripheralSourceLabel('razer_hid')).toBe('USB HID');
    expect(formatPeripheralSourceLabel('logitech_hidpp')).toBe('USB HID');
    expect(formatPeripheralSourceLabel('xinput')).toBe('XInput');
    expect(formatPeripheralSourceLabel('')).toBe('Беспроводное');

    expect(formatWeatherWindLabel(12.6)).toBe('13 км/ч');
    expect(formatWeatherWindLabel(null)).toBe('— км/ч');

    expect(
      formatCityOptionSubtitle({
        name: 'Москва',
        admin1: 'Москва',
        country: 'Россия',
      }),
    ).toBe('Россия');
    expect(
      formatCityOptionSubtitle({
        name: 'Алматы',
        admin1: 'Алматы',
        country: 'Казахстан',
      }),
    ).toBe('Казахстан');
    expect(
      formatCityOptionSubtitle({
        name: 'Остин',
        admin1: 'Техас',
        country: 'США',
      }),
    ).toBe('Техас, США');

    expect(computeWeatherTempBarRange(10, 20, 0, 20)).toEqual({
      leftPct: 50,
      widthPct: 50,
    });
    expect(computeWeatherTempBarRange(5, 5, 5, 5)).toEqual({
      leftPct: 0,
      widthPct: 100,
    });
    expect(computeWeatherTempBarRange(0, 1, 0, 40)).toEqual({
      leftPct: 0,
      widthPct: 12,
    });
  });

  it('renders crisp weather and peripheral SVG icons without emoji', () => {
    for (const cond of ['sun', 'moon', 'partly-cloudy-day', 'rain', 'snow', 'storm'] as const) {
      const html = renderToStaticMarkup(createElement(WeatherConditionIcon, { condition: cond }));
      expect(html).toContain('<svg');
    }
    for (const kind of ['mouse', 'headphones', 'keyboard', 'gamepad', 'other'] as const) {
      const html = renderToStaticMarkup(createElement(PeripheralDeviceIcon, { kind }));
      expect(html).toContain('<svg');
    }
    expect(renderToStaticMarkup(createElement(WindIcon))).toContain('<svg');
    expect(renderToStaticMarkup(createElement(DropletIcon))).toContain('<svg');
    expect(renderToStaticMarkup(createElement(ChargingBoltIcon))).toContain('<svg');

    const settingsHtml = renderToStaticMarkup(
      createElement(StartSettingsOverlay, {
        open: true,
        config: null,
        onClose: () => {},
        onSetBarMode: async () => {},
        onSetSecondaryMode: async () => {},
        onSetAutostart: async () => {},
        onSetAppearance: async () => {},
        onRefreshIndexes: async () => {},
        onRestoreTrayApp: async () => {},
        onRestoreAllTrayApps: async () => {},
        onExportConfig: async () => {},
        onImportConfig: async () => {},
        onRestoreWindowsAndExit: () => {},
      }),
    );
    expect(settingsHtml).toContain('Настройки');
    expect(settingsHtml).toContain('Панель и мониторы');
  });
});
