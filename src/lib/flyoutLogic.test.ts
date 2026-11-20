import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { BellIcon, ChevronLeftIcon, ChevronRightIcon, NativeTrayIcon } from '../components/Icons';
import { TaskbarView } from '../components/TaskbarView';
import type { AppWindowGroup, FlyoutRequest, OpenWindowInfo } from './ipc';
import {
  canOpenFileLocation,
  canRunAsAdmin,
  computeTrayFlyoutHeight,
  FLYOUT_EXIT_MS,
  formatIpcError,
  formatTrayHeaderLabel,
  getActiveDeviceIndex,
  INITIAL_FLYOUT_ANIM_STATE,
  isAppContextKind,
  isBarContextKind,
  OSD_AUTO_CLOSE_MS,
  PREVIEW_LEAVE_COMMAND,
  reduceFlyoutEntered,
  reduceFlyoutHideComplete,
  reduceFlyoutHideStart,
  reduceFlyoutShow,
  removePreviewWindow,
  stepVolume,
  syncPreviewWindowsFromGroups,
  TRAY_CARD_WIDTH,
  TRAY_COLUMNS,
  TRAY_MIN_CARD_HEIGHT,
} from './flyoutLogic';

function makeReq(kind: FlyoutRequest['kind']): FlyoutRequest {
  return {
    kind,
    anchorXLogical: 200,
    anchorInTaskbar: true,
    width: 240,
    height: 180,
    noActivate: kind === 'preview',
    payload: {},
  };
}

function makeWin(hwnd: number, title: string): OpenWindowInfo {
  return {
    hwnd,
    pid: 1000 + hwnd,
    title,
    exePath: 'C:\\Apps\\Code.exe',
    exeName: 'VS Code',
    aumid: null,
    isActive: false,
    isMinimized: false,
  };
}

describe('Flyout enter/exit animation state machine', () => {
  it('increments renderSeq on every show so CSS flyIn replays on every open', () => {
    const s1 = reduceFlyoutShow(INITIAL_FLYOUT_ANIM_STATE, makeReq('preview'));
    expect(s1.phase).toBe('entering');
    expect(s1.renderSeq).toBe(1);
    expect(s1.req?.kind).toBe('preview');

    const s2 = reduceFlyoutEntered(s1);
    expect(s2.phase).toBe('open');
    expect(s2.renderSeq).toBe(1);

    // Switching straight to another flyout replays enter with renderSeq = 2
    const s3 = reduceFlyoutShow(s2, makeReq('context_app'));
    expect(s3.phase).toBe('entering');
    expect(s3.renderSeq).toBe(2);
    expect(s3.req?.kind).toBe('context_app');
  });

  it('plays closing phase keeping req mounted until matching renderSeq completes', () => {
    expect(FLYOUT_EXIT_MS).toBe(120);

    const open = reduceFlyoutEntered(reduceFlyoutShow(INITIAL_FLYOUT_ANIM_STATE, makeReq('sound')));
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
  });

  it('rapid reopen during closing cancels pending exit cleanly (stale seq ignored)', () => {
    const open = reduceFlyoutEntered(reduceFlyoutShow(INITIAL_FLYOUT_ANIM_STATE, makeReq('preview')));
    const closing = reduceFlyoutHideStart(open);
    expect(closing.renderSeq).toBe(1);

    // Rapid reopen before 120ms exit completes
    const reopened = reduceFlyoutShow(closing, makeReq('calendar'));
    expect(reopened.phase).toBe('entering');
    expect(reopened.renderSeq).toBe(2);
    expect(reopened.req?.kind).toBe('calendar');

    // Stale hide timer for seq 1 MUST NOT wipe the reopened flyout!
    const afterStaleTimer = reduceFlyoutHideComplete(reopened, 1);
    expect(afterStaleTimer).toBe(reopened);
    expect(afterStaleTimer.phase).toBe('entering');
    expect(afterStaleTimer.req?.kind).toBe('calendar');
  });
});

describe('Preview mouse-leave contract, per-window close & context menu guards', () => {
  it('uses close_preview_if_outside (never close_flyout) on preview mouse leave and SVG icons instead of Unicode arrows', () => {
    expect(PREVIEW_LEAVE_COMMAND).toBe('close_preview_if_outside');

    // ChevronLeftIcon renders crisp inline SVG with zero Unicode glyph hacks
    const chevronHtml = renderToStaticMarkup(createElement(ChevronLeftIcon));
    expect(chevronHtml).toContain('<svg');
    expect(chevronHtml).toContain('<polyline');
    expect(chevronHtml).not.toContain('←');

    // TaskbarView renders clean Dock root & stationary capsule markup
    const tbHtml = renderToStaticMarkup(createElement(TaskbarView));
    expect(tbHtml).toContain('class="tb-root"');
    expect(tbHtml).toContain('class="taskbar"');
    expect(tbHtml).toContain('data-monitor="primary"');
  });

  it('updates preview window list on per-window close and live windows-updated sync', () => {
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
        windows: [makeWin(2, 'Tab 2')],
      },
    ];
    expect(syncPreviewWindowsFromGroups('c:\\apps\\code.exe', groups)).toHaveLength(1);
    expect(syncPreviewWindowsFromGroups('c:\\apps\\gone.exe', groups)).toEqual([]);
    expect(syncPreviewWindowsFromGroups('', groups)).toBeNull();
  });

  it('validates file-location, run-as-admin eligibility, context kinds & error formatting', () => {
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
  });
});

describe('Epic 3: Tray 6-column layout, non-shifting tooltip, OSD & Quick Sound helpers', () => {
  it('computes 6-column tray dimensions invariant to hover/overlay and always non-empty header', () => {
    expect(TRAY_COLUMNS).toBe(6);
    expect(TRAY_CARD_WIDTH).toBe(244);

    // Header text is ALWAYS non-empty so fixed 24px bar never collapses or shifts grid vertically
    expect(formatTrayHeaderLabel('', 0)).toBe('Фоновые приложения');
    expect(formatTrayHeaderLabel('   ', 14)).toBe('Фоновые · 14');
    expect(formatTrayHeaderLabel('Telegram', 14)).toBe('Telegram');

    // Minimum height accommodates the right-click overlay menu even with 0..6 apps
    expect(computeTrayFlyoutHeight(0)).toBe(TRAY_MIN_CARD_HEIGHT);
    expect(computeTrayFlyoutHeight(6)).toBe(TRAY_MIN_CARD_HEIGHT);
    // 18 apps -> 3 rows (3*34 + 2*4 = 110) + 46 = 156 -> still >= TRAY_MIN_CARD_HEIGHT
    expect(computeTrayFlyoutHeight(18)).toBe(TRAY_MIN_CARD_HEIGHT);
    // 24 apps -> 4 rows (4*34 + 3*4 = 148) + 46 = 194
    expect(computeTrayFlyoutHeight(24)).toBe(194);
    // 42 apps -> capped at 224px scrollable grid + 46 = 270px
    expect(computeTrayFlyoutHeight(42)).toBe(270);
    expect(computeTrayFlyoutHeight(80)).toBe(270);
  });

  it('updates consecutive OSD wheel ticks in place without bumping renderSeq, but replays enter after closing', () => {
    expect(OSD_AUTO_CLOSE_MS).toBe(1100);

    const openOsd = reduceFlyoutEntered(reduceFlyoutShow(INITIAL_FLYOUT_ANIM_STATE, makeReq('osd')));
    expect(openOsd.renderSeq).toBe(1);
    expect(openOsd.phase).toBe('open');

    // Next wheel tick updates payload in place without replaying flyIn bounce
    const nextTickReq = { ...makeReq('osd'), payload: { volume: 64, muted: false } };
    const updated = reduceFlyoutShow(openOsd, nextTickReq);
    expect(updated.renderSeq).toBe(1);
    expect(updated.phase).toBe('open');
    expect(updated.req?.payload).toEqual({ volume: 64, muted: false });

    // Reopening OSD while closing DOES bump renderSeq to cancel exit smoothly
    const closing = reduceFlyoutHideStart(updated);
    const reopened = reduceFlyoutShow(closing, nextTickReq);
    expect(reopened.renderSeq).toBe(2);
    expect(reopened.phase).toBe('entering');
  });

  it('resolves sliding device index, wheel volume steps & crisp inline SVG icons', () => {
    expect(
      getActiveDeviceIndex([
        { isDefault: false },
        { isDefault: true },
        { isDefault: false },
      ]),
    ).toBe(1);
    expect(getActiveDeviceIndex([{ isDefault: false }, { isDefault: false }])).toBe(0);

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
