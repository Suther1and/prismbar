import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ChevronLeftIcon } from '../components/Icons';
import { TaskbarView } from '../components/TaskbarView';
import type { AppWindowGroup, FlyoutRequest, OpenWindowInfo } from './ipc';
import {
  canOpenFileLocation,
  canRunAsAdmin,
  FLYOUT_EXIT_MS,
  formatIpcError,
  INITIAL_FLYOUT_ANIM_STATE,
  isAppContextKind,
  isBarContextKind,
  PREVIEW_LEAVE_COMMAND,
  reduceFlyoutEntered,
  reduceFlyoutHideComplete,
  reduceFlyoutHideStart,
  reduceFlyoutShow,
  removePreviewWindow,
  syncPreviewWindowsFromGroups,
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
