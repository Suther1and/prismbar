import { describe, expect, it } from 'vitest';
import type { MergedBarItem } from './ipc';
import {
  buildAppContextMenuRequest,
  buildBarContextMenuRequest,
  buildPinFromAppContext,
  buildPreviewFlyoutRequest,
  computeDockLogicalWidth,
  getTaskbarItemStateClass,
  getTaskbarRootClassName,
  resolveTaskbarMonitor,
  shouldTriggerPreviewHover,
  type AppContextPayload,
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

describe('TaskbarView Dock/Edge layout & multi-monitor helpers', () => {
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
    // Picks unconstrained scrollWidth when initial window width clips bounding rect
    expect(computeDockLogicalWidth(742.2, 604)).toBe(743);
    // Picks bounding rect when slightly larger
    expect(computeDockLogicalWidth(500, 512.4)).toBe(513);
    // Rejects unmounted / sub-200px transient measurements
    expect(computeDockLogicalWidth(0, 0)).toBeNull();
    expect(computeDockLogicalWidth(180, 200)).toBeNull();
    expect(computeDockLogicalWidth(Number.NaN, 420)).toBe(420);
  });

  it('computes item running/active/idle indicator classes', () => {
    const active = makeRunningItem();
    expect(getTaskbarItemStateClass(active)).toBe('active');

    const running = makeRunningItem({
      group: { ...active.group!, isActive: false },
    });
    expect(getTaskbarItemStateClass(running)).toBe('running');

    const idle = makeRunningItem({ group: undefined });
    expect(getTaskbarItemStateClass(idle)).toBe('idle');
  });
});

describe('Preview hover preemption & flyout payload builders', () => {
  it('preempts hover preview when Start or interactive click flyouts are open', () => {
    const running = makeRunningItem();
    const idle = makeRunningItem({ group: undefined });

    // Normal hover on running app -> allowed
    expect(shouldTriggerPreviewHover(running, false, '')).toBe(true);
    // Switching directly from one hover preview to another -> allowed
    expect(shouldTriggerPreviewHover(running, false, 'preview')).toBe(true);
    expect(shouldTriggerPreviewHover(running, false, 'osd')).toBe(true);

    // Idle pinned app -> blocked
    expect(shouldTriggerPreviewHover(idle, false, '')).toBe(false);
    // Start open -> blocked
    expect(shouldTriggerPreviewHover(running, true, '')).toBe(false);

    // Interactive click flyouts must NEVER be preempted by hover preview
    for (const clickFlyout of ['tray', 'sound', 'quick', 'calendar', 'context_app', 'context_bar']) {
      expect(shouldTriggerPreviewHover(running, false, clickFlyout)).toBe(false);
    }
  });

  it('builds gapless non-activating preview requests carrying monitor anchor', () => {
    const item = makeRunningItem();
    const req = buildPreviewFlyoutRequest(item, 320, 'secondary');
    expect(req.kind).toBe('preview');
    expect(req.noActivate).toBe(true);
    expect(req.anchorInTaskbar).toBe(true);
    expect(req.anchorXLogical).toBe(320);
    expect(req.monitor).toBe('secondary');
    expect(req.width).toBe(2 * 214 + 20);
    expect(req.height).toBe(166);
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

    // Pinning from context menu preserves canonical deduplication fields
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
