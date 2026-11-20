import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  ipc,
  onEvent,
  type AppWindowGroup,
  type AudioState,
  type FlyoutKind,
  type PinItem,
  type TestBarConfig,
} from '../lib/ipc';
import { FallbackAppIcon, StartLogoIcon, VolIcon } from './Icons';

interface MergedBarItem {
  id: string;
  name: string;
  target: string;
  iconKey: string;
  iconData?: string | null;
  pinned: boolean;
  pinId?: string;
  group?: AppWindowGroup;
}

export const TaskbarView: React.FC = () => {
  const [config, setConfig] = useState<TestBarConfig | null>(null);
  const [groups, setGroups] = useState<AppWindowGroup[]>([]);
  const [audio, setAudio] = useState<AudioState | null>(null);
  const [layout, setLayout] = useState('RU');
  const [startOpen, setStartOpen] = useState(false);
  const [activeFlyout, setActiveFlyout] = useState<string>('');
  const [now, setNow] = useState(() => new Date());
  const [iconMap, setIconMap] = useState<Record<string, string>>({});
  const [volToast, setVolToast] = useState<string | null>(null);

  const barRef = useRef<HTMLElement>(null);
  const hoverTimer = useRef<number | null>(null);
  const leaveTimer = useRef<number | null>(null);
  const volToastTimer = useRef<number | null>(null);

  useEffect(() => {
    const t = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    ipc<TestBarConfig>('get_config').then(setConfig).catch(() => {});
    ipc<AppWindowGroup[]>('get_open_windows').then(setGroups).catch(() => {});
    ipc<AudioState>('get_audio_state').then(setAudio).catch(() => {});
    ipc<string>('get_keyboard_layout').then(setLayout).catch(() => {});

    const subs = [
      onEvent<TestBarConfig>('config-updated', setConfig),
      onEvent<AppWindowGroup[]>('windows-updated', setGroups),
      onEvent<AudioState>('audio-updated', setAudio),
      onEvent<string>('layout-updated', setLayout),
      onEvent<boolean>('start-state', setStartOpen),
      onEvent<{ kind: string }>('flyout-show', (f) => setActiveFlyout(f.kind)),
      onEvent<void>('flyout-hide', () => setActiveFlyout('')),
    ];
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  // Load real native .exe icons for pinned taskbar apps
  useEffect(() => {
    if (!config) return;
    config.taskbarPins.forEach((p) => {
      if (p.target && !p.iconData && !iconMap[p.target] && p.target.includes(':\\')) {
        ipc<string | null>('get_icon_for_path', { path: p.target })
          .then((data) => {
            if (data) setIconMap((prev) => ({ ...prev, [p.target]: data }));
          })
          .catch(() => {});
      }
    });
  }, [config]);

  // Merge pinned apps + running window groups cleanly without duplicates
  const barItems = useMemo<MergedBarItem[]>(() => {
    const pins: PinItem[] = config?.taskbarPins ?? [];
    const usedGroupKeys = new Set<string>();
    const merged: MergedBarItem[] = [];

    for (const p of pins) {
      const targetLow = p.target.toLowerCase();
      const matchedGroup = groups.find(
        (g) =>
          (targetLow && g.key === targetLow) ||
          (targetLow && g.exePath.toLowerCase().endsWith('\\' + targetLow.split('\\').pop())) ||
          g.name.toLowerCase() === p.name.toLowerCase(),
      );
      if (matchedGroup) usedGroupKeys.add(matchedGroup.key);
      merged.push({
        id: p.id,
        name: p.name,
        target: p.target || matchedGroup?.exePath || '',
        iconKey: p.iconKey,
        iconData: matchedGroup?.iconData || p.iconData || iconMap[p.target],
        pinned: true,
        pinId: p.id,
        group: matchedGroup,
      });
    }

    for (const g of groups) {
      if (!usedGroupKeys.has(g.key)) {
        merged.push({
          id: 'run-' + g.key,
          name: g.name,
          target: g.exePath,
          iconKey: g.name.toLowerCase(),
          iconData: g.iconData,
          pinned: false,
          group: g,
        });
      }
    }
    return merged;
  }, [config, groups, iconMap]);

  // Auto-size floating dock window to exact capsule width so desktop left/right stays clickable
  useEffect(() => {
    const el = barRef.current;
    if (!el || config?.barMode === 'edge') return;
    const syncSize = () => {
      const w = Math.ceil(el.getBoundingClientRect().width);
      if (w > 200) {
        ipc('resize_taskbar_dock', { logicalWidth: w }).catch(() => {});
      }
    };
    syncSize();
    const ro = new ResizeObserver(syncSize);
    ro.observe(el);
    return () => ro.disconnect();
  }, [barItems.length, config?.barMode]);

  const anchorX = (el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    return r.left + r.width / 2;
  };

  const openFlyoutAt = (
    kind: FlyoutKind,
    el: HTMLElement,
    width: number,
    height: number,
    noActivate: boolean,
    payload: unknown = {},
  ) => {
    ipc('open_flyout', {
      req: {
        kind,
        anchorXLogical: anchorX(el),
        anchorInTaskbar: true,
        width,
        height,
        noActivate,
        payload,
      },
    }).catch(() => {});
  };

  const handleAppClick = (item: MergedBarItem) => {
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    if (item.group && item.group.windows.length > 0) {
      const activeWin = item.group.windows.find((w) => w.isActive) || item.group.windows[0];
      ipc('focus_window', { hwnd: activeWin.hwnd, toggleMinimize: true }).catch(() => {});
    } else if (item.target) {
      ipc('launch_item', { target: item.target, asAdmin: false, closeStart: false }).catch(() => {});
    }
  };

  const handleAppHover = (item: MergedBarItem, el: HTMLElement) => {
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    if (leaveTimer.current) window.clearTimeout(leaveTimer.current);
    if (!item.group || item.group.windows.length === 0 || startOpen) return;
    hoverTimer.current = window.setTimeout(() => {
      const count = Math.min(item.group!.windows.length, 4);
      const w = count * 214 + 20;
      openFlyoutAt('preview', el, w, 166, true, { group: item.group });
    }, 220);
  };

  const handleAppLeave = () => {
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    if (leaveTimer.current) window.clearTimeout(leaveTimer.current);
    leaveTimer.current = window.setTimeout(() => {
      ipc('close_preview_if_outside').catch(() => {});
    }, 280);
  };

  const handleAppCtx = (e: React.MouseEvent, item: MergedBarItem) => {
    e.preventDefault();
    e.stopPropagation();
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    const hasWins = Boolean(item.group && item.group.windows.length > 0);
    openFlyoutAt('context_app', e.currentTarget as HTMLElement, 248, hasWins ? 270 : 196, false, {
      id: item.id,
      pinId: item.pinId,
      name: item.name,
      target: item.target,
      pinned: item.pinned,
      windows: item.group?.windows ?? [],
    });
  };

  const handleBarCtx = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('.tb-btn')) return;
    e.preventDefault();
    ipc('open_flyout', {
      req: {
        kind: 'context_bar',
        anchorXLogical: e.clientX,
        anchorInTaskbar: true,
        width: 244,
        height: 190,
        noActivate: false,
        payload: { barMode: config?.barMode ?? 'dock' },
      },
    }).catch(() => {});
  };

  const flashVol = (text: string) => {
    setVolToast(text);
    if (volToastTimer.current) window.clearTimeout(volToastTimer.current);
    volToastTimer.current = window.setTimeout(() => setVolToast(null), 1100);
  };

  const handleSoundWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    const cur = audio?.masterVolume ?? 50;
    const next = Math.max(0, Math.min(100, cur + (e.deltaY < 0 ? 4 : -4)));
    flashVol(`${next}%`);
    ipc<AudioState>('set_master_volume', { volume: next, muted: false })
      .then(setAudio)
      .catch(() => {});
  };

  const handleSoundAux = (e: React.MouseEvent) => {
    if (e.button === 1) {
      e.preventDefault();
      const nextMute = !(audio?.masterMuted ?? false);
      flashVol(nextMute ? 'Выкл' : `${audio?.masterVolume ?? 50}%`);
      ipc<AudioState>('set_master_volume', {
        volume: audio?.masterVolume ?? 50,
        muted: nextMute,
      })
        .then(setAudio)
        .catch(() => {});
    }
  };

  const pad2 = (n: number) => String(n).padStart(2, '0');
  const timeStr = `${pad2(now.getHours())}:${pad2(now.getMinutes())}`;
  const dateStr = `${pad2(now.getDate())}.${pad2(now.getMonth() + 1)}.${now.getFullYear()}`;
  const isEdge = config?.barMode === 'edge';

  return (
    <div className={`tb-root ${isEdge ? 'edge' : ''}`} data-space="personal">
      <nav
        ref={barRef}
        className="taskbar"
        onContextMenu={handleBarCtx}
        onMouseMove={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          e.currentTarget.style.setProperty('--tbx', `${e.clientX - r.left}px`);
        }}
      >
        <div className="tb-group">
          <button
            className={`tb-btn ${startOpen ? 'start-on' : ''}`}
            title="Пуск"
            onClick={() => ipc('toggle_start', { force: null }).catch(() => {})}
          >
            <span className="tb-ic">
              <StartLogoIcon />
            </span>
          </button>

          {barItems.map((item) => {
            const winCount = item.group?.windows.length ?? 0;
            const cls = item.group?.isActive ? 'active' : winCount > 0 ? 'running' : 'idle';
            return (
              <button
                key={item.id}
                className={`tb-btn ${cls}`}
                title={item.name}
                onClick={() => handleAppClick(item)}
                onContextMenu={(e) => handleAppCtx(e, item)}
                onMouseEnter={(e) => handleAppHover(item, e.currentTarget)}
                onMouseLeave={handleAppLeave}
              >
                <span className="tb-ic">
                  {item.iconData ? (
                    <img src={item.iconData} alt="" className="native-ic" draggable={false} />
                  ) : (
                    <FallbackAppIcon iconKey={item.iconKey} size={22} />
                  )}
                </span>
                <span className="tb-pill" />
                {winCount > 1 && <span className="tb-multi" />}
              </button>
            );
          })}
        </div>

        <div className="tb-group">
          <div className="tb-sep" />

          <button
            className={`tb-tray-btn chevron ${activeFlyout === 'tray' ? 'open' : ''}`}
            style={{ padding: '0 7px' }}
            title="Фоновые приложения"
            onClick={(e) => openFlyoutAt('tray', e.currentTarget, 248, 224, false)}
          >
            <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round">
              <polyline points="18 15 12 9 6 15" />
            </svg>
          </button>

          <button
            className="tb-tray-btn"
            style={{ fontFamily: '"Cascadia Code", monospace', fontSize: 11.5, fontWeight: 600, padding: '0 7px' }}
            title="Раскладка клавиатуры"
            onClick={() => ipc<string>('switch_keyboard_layout').then(setLayout).catch(() => {})}
          >
            {layout}
          </button>

          <button
            className={`tb-tray-btn ${activeFlyout === 'sound' || activeFlyout === 'quick' ? 'open' : ''}`}
            title={`Звук: ${audio?.masterMuted ? 'Выкл' : `${audio?.masterVolume ?? 0}%`}`}
            onClick={(e) => openFlyoutAt('sound', e.currentTarget, 330, 290, false)}
            onWheel={handleSoundWheel}
            onAuxClick={handleSoundAux}
          >
            <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
              <path d="M5 12.55a11 11 0 0 1 14.08 0" />
              <path d="M8.53 16.11a6 6 0 0 1 6.95 0" />
              <line x1="12" y1="20" x2="12.01" y2="20" />
            </svg>
            <VolIcon muted={audio?.masterMuted} />
            {volToast && (
              <span style={{ fontFamily: '"Cascadia Code", monospace', fontSize: 11, fontWeight: 600, color: 'var(--accent)' }}>
                {volToast}
              </span>
            )}
          </button>

          <button
            className={`tb-tray-btn tb-clock ${activeFlyout === 'calendar' ? 'open' : ''}`}
            onClick={(e) => openFlyoutAt('calendar', e.currentTarget, 310, 292, false)}
          >
            <span style={{ fontSize: 12, fontWeight: 600, fontVariantNumeric: 'tabular-nums' }}>{timeStr}</span>
            <span style={{ fontSize: 10, color: 'var(--t2)', fontVariantNumeric: 'tabular-nums' }}>{dateStr}</span>
          </button>
        </div>
      </nav>
    </div>
  );
};
