import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  applyConfigUpdate,
  fetchIconsBatch,
  iconCache,
  ipc,
  isCacheIconKey,
  mergeTaskbarItems,
  onEvent,
  type AppWindowGroup,
  type AudioState,
  type FlyoutKind,
  type MergedBarItem,
  type TestBarConfig,
} from '../lib/ipc';
import {
  buildAppContextMenuRequest,
  buildBarContextMenuRequest,
  buildPreviewFlyoutRequest,
  computeDockLogicalWidth,
  getTaskbarItemStateClass,
  getTaskbarRootClassName,
  resolveTaskbarMonitor,
  shouldTriggerPreviewHover,
} from '../lib/taskbarLogic';
import { FallbackAppIcon, StartLogoIcon, VolIcon } from './Icons';

export const TaskbarView: React.FC = () => {
  const monitor = useMemo(
    () => (typeof window !== 'undefined' ? resolveTaskbarMonitor(window.location.search) : 'primary'),
    [],
  );

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
    return () => {
      window.clearInterval(t);
      if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
      if (leaveTimer.current) window.clearTimeout(leaveTimer.current);
      if (volToastTimer.current) window.clearTimeout(volToastTimer.current);
    };
  }, []);

  useEffect(() => {
    ipc<TestBarConfig>('get_config')
      .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
      .catch(() => {});
    ipc<AppWindowGroup[]>('get_open_windows').then(setGroups).catch(() => {});
    ipc<AudioState>('get_audio_state').then(setAudio).catch(() => {});
    ipc<string>('get_keyboard_layout').then(setLayout).catch(() => {});

    const subs = [
      onEvent<TestBarConfig>('config-updated', (next) =>
        setConfig((prev) => applyConfigUpdate(prev, next)),
      ),
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

  // Load real native .exe and shell:AppsFolder\ UWP icons for pinned taskbar apps
  useEffect(() => {
    if (!config) return;
    config.taskbarPins.forEach((p) => {
      const target = p.target?.trim() ?? '';
      const canResolve =
        target.includes(':\\') || target.toLowerCase().startsWith('shell:appsfolder\\');
      if (target && !p.iconData && !iconMap[target] && canResolve) {
        ipc<string | null>('get_icon_for_path', { path: target })
          .then((data) => {
            if (data) setIconMap((prev) => (prev[target] ? prev : { ...prev, [target]: data }));
          })
          .catch(() => {});
      }
    });
  }, [config]);

  // Batch-fetch missing cached icon keys from Rust IconCache for running windows & cache-keyed pins
  useEffect(() => {
    const missing = new Set<string>();
    for (const g of groups) {
      if (g.iconKey && !iconMap[g.iconKey]) {
        const cached = iconCache.get(g.iconKey);
        if (cached) {
          setIconMap((prev) => (prev[g.iconKey!] ? prev : { ...prev, [g.iconKey!]: cached }));
        } else {
          missing.add(g.iconKey);
        }
      }
    }
    for (const p of config?.taskbarPins ?? []) {
      if (isCacheIconKey(p.iconKey) && !iconMap[p.iconKey]) {
        const cached = iconCache.get(p.iconKey);
        if (cached) {
          setIconMap((prev) => (prev[p.iconKey] ? prev : { ...prev, [p.iconKey]: cached }));
        } else {
          missing.add(p.iconKey);
        }
      }
    }
    if (missing.size === 0) return;

    let cancelled = false;
    fetchIconsBatch(Array.from(missing))
      .then((fetched) => {
        if (cancelled) return;
        const keys = Object.keys(fetched);
        if (keys.length === 0) return;
        setIconMap((prev) => {
          let changed = false;
          for (const k of keys) {
            if (prev[k] !== fetched[k]) {
              changed = true;
              break;
            }
          }
          return changed ? { ...prev, ...fetched } : prev;
        });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [groups, config]);

  // Merge pinned apps + running window groups cleanly without duplicates
  const barItems = useMemo<MergedBarItem[]>(
    () => mergeTaskbarItems(config?.taskbarPins ?? [], groups, iconMap),
    [config, groups, iconMap],
  );

  // Auto-size floating dock window to exact unconstrained capsule width so desktop left/right stays clickable
  useEffect(() => {
    const el = barRef.current;
    if (!el || config?.barMode === 'edge') return;
    const syncSize = () => {
      const w = computeDockLogicalWidth(el.scrollWidth, el.getBoundingClientRect().width);
      if (w !== null) {
        ipc('resize_taskbar_dock', { logicalWidth: w, monitor }).catch(() => {});
      }
    };
    syncSize();
    const ro = new ResizeObserver(syncSize);
    ro.observe(el);
    return () => ro.disconnect();
  }, [barItems.length, config?.barMode, layout, monitor]);

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
        monitor,
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
      ipc('launch_item', {
        target: item.target,
        args: item.args ?? null,
        asAdmin: false,
        closeStart: false,
      }).catch(() => {});
    }
  };

  const handleAppHover = (item: MergedBarItem, el: HTMLElement) => {
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    if (leaveTimer.current) window.clearTimeout(leaveTimer.current);
    if (!shouldTriggerPreviewHover(item, startOpen, activeFlyout)) return;
    hoverTimer.current = window.setTimeout(() => {
      const req = buildPreviewFlyoutRequest(item, anchorX(el), monitor);
      ipc('open_flyout', { req }).catch(() => {});
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
    if (leaveTimer.current) window.clearTimeout(leaveTimer.current);
    const req = buildAppContextMenuRequest(item, anchorX(e.currentTarget as HTMLElement), monitor);
    ipc('open_flyout', { req }).catch(() => {});
  };

  const handleBarCtx = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('.tb-btn')) return;
    e.preventDefault();
    if (hoverTimer.current) window.clearTimeout(hoverTimer.current);
    const req = buildBarContextMenuRequest(config?.barMode, e.clientX, monitor);
    ipc('open_flyout', { req }).catch(() => {});
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

  return (
    <div
      className={getTaskbarRootClassName(config?.barMode)}
      data-space="personal"
      data-monitor={monitor}
    >
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
            const cls = getTaskbarItemStateClass(item);
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
