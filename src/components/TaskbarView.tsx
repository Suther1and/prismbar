import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { TRAY_CARD_WIDTH, TRAY_MIN_CARD_HEIGHT } from '../lib/flyoutLogic';
import {
  applyConfigUpdate,
  dismissOverlays,
  fetchIconsBatch,
  iconCache,
  ipc,
  isCacheIconKey,
  launchNewWindow,
  mergeTaskbarItems,
  normalizeStartStatePayload,
  onEvent,
  toggleStart,
  type AppWindowGroup,
  type AudioMasterState,
  type AudioState,
  type FlyoutKind,
  type MergedBarItem,
  type NetworkInfo,
  type StartStateEvent,
  type TestBarConfig,
} from '../lib/ipc';
import {
  buildAppContextMenuRequest,
  buildBarContextMenuRequest,
  buildPreviewFlyoutRequest,
  CALENDAR_FLYOUT_INITIAL_HEIGHT,
  CALENDAR_FLYOUT_WIDTH,
  computeNaturalDockWidth,
  computeNextWheelVolume,
  createTaskbarOrderMemory,
  formatSoundNetworkTooltip,
  formatTaskbarClock,
  getPreviewHoverDelayMs,
  getTaskbarItemStateClass,
  getTaskbarRootClassName,
  planSoundFeedbackAction,
  resolvePinnedReorderIndices,
  resolveTaskbarMonitor,
  selectWindowToFocusOnTaskbarClick,
  shouldTriggerPreviewHover,
  shouldUseEdgeCenteredLayout,
  SOUND_FLYOUT_INITIAL_HEIGHT,
  SOUND_FLYOUT_WIDTH,
  stabilizeTaskbarItemsOrder,
  TASKBAR_LAUNCH_HOP_MS,
} from '../lib/taskbarLogic';
import { useReducedMotion } from '../lib/useReducedMotion';
import { ChevronUpIcon, FallbackAppIcon, NetIcon, StartLogoIcon, VolIcon } from './Icons';

function audioStateToMaster(st: AudioState): AudioMasterState {
  const active = st.devices.find((d) => d.isDefault);
  return {
    available: st.available,
    masterVolume: st.masterVolume,
    masterMuted: st.masterMuted,
    micMuted: st.micMuted,
    defaultDeviceShortName: active?.shortName ?? '',
  };
}

export const TaskbarView: React.FC = () => {
  const monitor = useMemo(
    () => (typeof window !== 'undefined' ? resolveTaskbarMonitor(window.location.search) : 'primary'),
    [],
  );
  const reducedMotion = useReducedMotion();

  const [config, setConfig] = useState<TestBarConfig | null>(null);
  const [groups, setGroups] = useState<AppWindowGroup[]>([]);
  const [audioMaster, setAudioMaster] = useState<AudioMasterState | null>(null);
  const [net, setNet] = useState<NetworkInfo | null>(null);
  const [layout, setLayout] = useState('RU');
  const [startOpen, setStartOpen] = useState(false);
  const [activeFlyout, setActiveFlyout] = useState<string>('');
  const [now, setNow] = useState(() => new Date());
  const [iconMap, setIconMap] = useState<Record<string, string>>({});
  const [launchingIds, setLaunchingIds] = useState<Record<string, true>>({});
  const [dragPinId, setDragPinId] = useState<string | null>(null);
  const [dragOverPinId, setDragOverPinId] = useState<string | null>(null);
  const [viewportWidth, setViewportWidth] = useState<number>(() =>
    typeof window !== 'undefined' ? window.innerWidth : 1920,
  );

  const barRef = useRef<HTMLElement>(null);
  const appsScrollRef = useRef<HTMLDivElement>(null);
  const trayGroupRef = useRef<HTMLDivElement>(null);
  const barRectRef = useRef<DOMRect | null>(null);
  const orderMemoryRef = useRef(createTaskbarOrderMemory());
  const sheenRef = useRef<HTMLSpanElement>(null);
  const sheenRafRef = useRef<number | null>(null);
  const sheenClientXRef = useRef<number>(0);

  const hoverPillRef = useRef<HTMLSpanElement>(null);
  const pillVisibleRef = useRef<boolean>(false);
  const pillSnapRafRef = useRef<number | null>(null);

  const hoverTimer = useRef<number | null>(null);
  const leaveTimer = useRef<number | null>(null);
  const launchTimersRef = useRef<Map<string, number>>(new Map());
  const activeFlyoutRef = useRef<string>('');
  const lastPreviewCloseMsRef = useRef<number | null>(null);
  const requestedTargetsRef = useRef<Set<string>>(new Set());
  const iconMapRef = useRef<Record<string, string>>({});
  const mountedRef = useRef<boolean>(true);

  iconMapRef.current = iconMap;

  const showSeconds = Boolean(config?.clockShowSeconds);
  const showWeekday = Boolean(config?.clockShowWeekday);
  const isEdge = config?.barMode === 'edge';
  const useEdgeCentered = shouldUseEdgeCenteredLayout(config?.barMode, monitor, viewportWidth);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    const onResize = () => setViewportWidth(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  // Tick every 1s ONLY when clockShowSeconds is enabled; otherwise align cleanly to minute rollovers.
  useEffect(() => {
    setNow(new Date());
    if (showSeconds) {
      const intervalId = window.setInterval(() => setNow(new Date()), 1000);
      return () => window.clearInterval(intervalId);
    }

    let timeoutId: number | null = null;
    const scheduleNextMinute = () => {
      const current = new Date();
      const msUntilNextMinute = Math.max(250, (60 - current.getSeconds()) * 1000 - current.getMilliseconds() + 40);
      timeoutId = window.setTimeout(() => {
        setNow(new Date());
        scheduleNextMinute();
      }, msUntilNextMinute);
    };
    scheduleNextMinute();

    return () => {
      if (timeoutId !== null) window.clearTimeout(timeoutId);
    };
  }, [showSeconds]);

  // Global lifecycle & timer/rAF cleanup on unmount (StrictMode-safe mountedRef)
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
      if (leaveTimer.current !== null) window.clearTimeout(leaveTimer.current);
      if (sheenRafRef.current !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(sheenRafRef.current);
      }
      if (pillSnapRafRef.current !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(pillSnapRafRef.current);
      }
      launchTimersRef.current.forEach((timerId) => window.clearTimeout(timerId));
      launchTimersRef.current.clear();
    };
  }, []);

  useEffect(() => {
    ipc<TestBarConfig>('get_config')
      .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
      .catch(() => {});
    ipc<AppWindowGroup[]>('get_open_windows').then(setGroups).catch(() => {});
    ipc<AudioMasterState>('get_audio_master_state').then(setAudioMaster).catch(() => {});
    ipc<NetworkInfo>('get_network_info').then(setNet).catch(() => {});
    ipc<string>('get_keyboard_layout').then(setLayout).catch(() => {});

    const clearActiveFlyout = () => {
      if (activeFlyoutRef.current === 'preview') {
        lastPreviewCloseMsRef.current = performance.now();
      }
      activeFlyoutRef.current = '';
      setActiveFlyout('');
    };

    const subs = [
      onEvent<TestBarConfig>('config-updated', (next) =>
        setConfig((prev) => applyConfigUpdate(prev, next)),
      ),
      onEvent<AppWindowGroup[]>('windows-updated', setGroups),
      onEvent<AudioMasterState>('audio-master-updated', setAudioMaster),
      onEvent<NetworkInfo>('network-updated', setNet),
      onEvent<string>('layout-updated', setLayout),
      onEvent<boolean | StartStateEvent>('start-state', (ev) => {
        setStartOpen(normalizeStartStatePayload(ev).open);
      }),
      onEvent<{ kind: string }>('flyout-show', (f) => {
        activeFlyoutRef.current = f.kind;
        setActiveFlyout(f.kind);
      }),
      onEvent<unknown>('flyout-hide', clearActiveFlyout),
      onEvent<unknown>('flyout-hide-immediate', clearActiveFlyout),
    ];
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  // Load real native .exe and shell:AppsFolder\ UWP icons for pinned taskbar apps without stranding on config churn
  useEffect(() => {
    if (!config) return;

    for (const p of config.taskbarPins) {
      const target = p.target?.trim() ?? '';
      const canResolve =
        target.includes(':\\') || target.toLowerCase().startsWith('shell:appsfolder\\');
      if (
        !target ||
        !canResolve ||
        p.iconData ||
        iconMapRef.current[target] ||
        requestedTargetsRef.current.has(target)
      ) {
        continue;
      }
      requestedTargetsRef.current.add(target);
      ipc<string | null>('get_icon_for_path', { path: target })
        .then((data) => {
          if (!mountedRef.current) {
            requestedTargetsRef.current.delete(target);
            return;
          }
          if (data) {
            setIconMap((prev) => (prev[target] === data ? prev : { ...prev, [target]: data }));
          } else {
            requestedTargetsRef.current.delete(target);
          }
        })
        .catch(() => {
          requestedTargetsRef.current.delete(target);
        });
    }
  }, [config]);

  // Batch-fetch missing cached icon keys from Rust IconCache for running windows & cache-keyed pins
  useEffect(() => {
    const missing = new Set<string>();
    const immediate: Record<string, string> = {};
    const curIcons = iconMapRef.current;

    const checkKey = (key: string | null | undefined) => {
      if (!isCacheIconKey(key) || curIcons[key]) return;
      const cached = iconCache.get(key);
      if (cached) {
        immediate[key] = cached;
      } else {
        missing.add(key);
      }
    };

    for (const g of groups) {
      checkKey(g.iconKey);
    }
    for (const p of config?.taskbarPins ?? []) {
      checkKey(p.iconKey);
    }

    if (Object.keys(immediate).length > 0) {
      setIconMap((prev) => ({ ...prev, ...immediate }));
    }
    if (missing.size === 0) return;

    fetchIconsBatch(Array.from(missing))
      .then((fetched) => {
        if (!mountedRef.current) return;
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
  }, [groups, config]);

  // Merge pinned apps + running window groups cleanly without duplicates, preserving stable
  // first-seen order for unpinned apps & multi-window groups so clicks NEVER shuffle icons
  const barItems = useMemo<MergedBarItem[]>(
    () =>
      stabilizeTaskbarItemsOrder(
        mergeTaskbarItems(config?.taskbarPins ?? [], groups, iconMap),
        orderMemoryRef.current,
      ),
    [config, groups, iconMap],
  );

  // Immediately end bounded launch hop once the app's window group appears
  useEffect(() => {
    setLaunchingIds((prev) => {
      const keys = Object.keys(prev);
      if (keys.length === 0) return prev;
      let changed = false;
      const next = { ...prev };
      for (const id of keys) {
        const item = barItems.find((b) => b.id === id);
        if (!item || (item.group && item.group.windows.length > 0)) {
          delete next[id];
          const timerId = launchTimersRef.current.get(id);
          if (timerId !== undefined) {
            window.clearTimeout(timerId);
            launchTimersRef.current.delete(id);
          }
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [barItems]);

  // Auto-size floating dock window to exact unconstrained capsule width & keep barRectRef fresh
  useEffect(() => {
    const el = barRef.current;
    if (!el) return;
    const syncSize = () => {
      const rect = el.getBoundingClientRect();
      barRectRef.current = rect;
      if (config?.barMode === 'edge') return;
      const scrollEl = appsScrollRef.current;
      const trayEl = trayGroupRef.current;
      const appsNatural = scrollEl ? scrollEl.scrollWidth : 0;
      const trayNatural = trayEl ? trayEl.scrollWidth : 0;
      const w = computeNaturalDockWidth(el.scrollWidth, rect.width, appsNatural, trayNatural);
      if (w !== null) {
        ipc('resize_taskbar_dock', { logicalWidth: w, monitor }).catch(() => {});
      }
    };
    syncSize();
    const rafId =
      typeof requestAnimationFrame === 'function' ? requestAnimationFrame(syncSize) : null;
    if (typeof ResizeObserver === 'undefined') {
      return () => {
        if (rafId !== null && typeof cancelAnimationFrame === 'function') {
          cancelAnimationFrame(rafId);
        }
      };
    }
    const ro = new ResizeObserver(syncSize);
    ro.observe(el);
    if (appsScrollRef.current) ro.observe(appsScrollRef.current);
    if (trayGroupRef.current) ro.observe(trayGroupRef.current);
    return () => {
      if (rafId !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(rafId);
      }
      ro.disconnect();
    };
  }, [barItems.length, config?.barMode, showSeconds, showWeekday, layout, monitor]);

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

  const triggerLaunchHop = useCallback((id: string) => {
    const existing = launchTimersRef.current.get(id);
    if (existing !== undefined) {
      window.clearTimeout(existing);
    }
    setLaunchingIds((prev) => (prev[id] ? prev : { ...prev, [id]: true }));
    const timerId = window.setTimeout(() => {
      launchTimersRef.current.delete(id);
      setLaunchingIds((prev) => {
        if (!prev[id]) return prev;
        const next = { ...prev };
        delete next[id];
        return next;
      });
    }, TASKBAR_LAUNCH_HOP_MS);
    launchTimersRef.current.set(id, timerId);
  }, []);

  const hideHoverPill = useCallback(() => {
    pillVisibleRef.current = false;
    const pill = hoverPillRef.current;
    if (pill) {
      pill.style.opacity = '0';
    }
  }, []);

  const moveHoverPillTo = useCallback((btn: HTMLElement) => {
    if (dragPinId) return;
    const pill = hoverPillRef.current;
    if (!pill) return;

    const targetX = btn.offsetLeft;
    if (!pillVisibleRef.current) {
      pill.classList.add('snap');
      pill.style.transform = `translate3d(${targetX}px, 0, 0)`;
      pill.style.opacity = '1';
      pillVisibleRef.current = true;
      if (pillSnapRafRef.current !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(pillSnapRafRef.current);
      }
      if (typeof requestAnimationFrame === 'function') {
        pillSnapRafRef.current = requestAnimationFrame(() => {
          pillSnapRafRef.current = null;
          hoverPillRef.current?.classList.remove('snap');
        });
      } else {
        pill.classList.remove('snap');
      }
    } else {
      pill.classList.remove('snap');
      pill.style.transform = `translate3d(${targetX}px, 0, 0)`;
      pill.style.opacity = '1';
    }
  }, [dragPinId]);

  const handleBarPointerEnter = useCallback((e: React.PointerEvent<HTMLElement>) => {
    barRectRef.current = e.currentTarget.getBoundingClientRect();
  }, []);

  const handleBarPointerMove = useCallback(
    (e: React.PointerEvent<HTMLElement>) => {
      if (reducedMotion) return;
      sheenClientXRef.current = e.clientX;
      if (sheenRafRef.current !== null) return;
      if (typeof requestAnimationFrame !== 'function') return;
      sheenRafRef.current = requestAnimationFrame(() => {
        sheenRafRef.current = null;
        const sheen = sheenRef.current;
        const bar = barRef.current;
        if (!sheen || !bar) return;
        const rect = barRectRef.current ?? bar.getBoundingClientRect();
        barRectRef.current = rect;
        const localX = sheenClientXRef.current - rect.left;
        sheen.style.transform = `translate3d(${Math.round(localX - 120)}px, 0, 0)`;
      });
    },
    [reducedMotion],
  );

  const handleAppClick = (item: MergedBarItem) => {
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    const windows = item.group?.windows ?? [];
    const targetWin = selectWindowToFocusOnTaskbarClick(windows);
    if (targetWin) {
      ipc('focus_window', {
        hwnd: targetWin.hwnd,
        toggleMinimize: targetWin.toggleMinimize,
      }).catch(() => {});
    } else if (item.target) {
      triggerLaunchHop(item.id);
      ipc('launch_item', {
        target: item.target,
        args: item.args ?? null,
        asAdmin: false,
        closeStart: false,
      }).catch(() => {});
    }
  };

  const handleAppAuxClick = (e: React.MouseEvent<HTMLButtonElement>, item: MergedBarItem) => {
    if (e.button !== 1) return;
    e.preventDefault();
    e.stopPropagation();
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    const launchTarget =
      item.target ||
      item.group?.exePath ||
      (item.matchAumid ? `shell:AppsFolder\\${item.matchAumid}` : '');
    if (!launchTarget) return;
    if (!item.group || item.group.windows.length === 0) {
      triggerLaunchHop(item.id);
    }
    launchNewWindow(launchTarget, item.args ?? null, false).catch(() => {
      ipc('launch_item', {
        target: launchTarget,
        args: item.args ?? null,
        asAdmin: false,
        closeStart: false,
      }).catch(() => {});
    });
  };

  const handleAppHover = (item: MergedBarItem, el: HTMLElement) => {
    moveHoverPillTo(el);
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    if (leaveTimer.current !== null) {
      window.clearTimeout(leaveTimer.current);
      leaveTimer.current = null;
    }
    if (dragPinId) return;
    if (!shouldTriggerPreviewHover(item, startOpen, activeFlyoutRef.current)) return;

    const delayMs = getPreviewHoverDelayMs(
      activeFlyoutRef.current,
      lastPreviewCloseMsRef.current,
      performance.now(),
    );
    const anchor = anchorX(el);
    hoverTimer.current = window.setTimeout(() => {
      hoverTimer.current = null;
      const req = buildPreviewFlyoutRequest(item, anchor, monitor);
      ipc('open_flyout', { req }).catch(() => {});
    }, delayMs);
  };

  const handleAppLeave = () => {
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    if (leaveTimer.current !== null) {
      window.clearTimeout(leaveTimer.current);
      leaveTimer.current = null;
    }
    leaveTimer.current = window.setTimeout(() => {
      leaveTimer.current = null;
      ipc('close_preview_if_outside').catch(() => {});
    }, 260);
  };

  const handleAppCtx = (e: React.MouseEvent, item: MergedBarItem) => {
    e.preventDefault();
    e.stopPropagation();
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    if (leaveTimer.current !== null) {
      window.clearTimeout(leaveTimer.current);
      leaveTimer.current = null;
    }
    const req = buildAppContextMenuRequest(item, anchorX(e.currentTarget as HTMLElement), monitor);
    ipc('open_flyout', { req }).catch(() => {});
  };

  const handleBarCtx = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('.tb-btn')) return;
    e.preventDefault();
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    const req = buildBarContextMenuRequest(config?.barMode, e.clientX, monitor);
    ipc('open_flyout', { req }).catch(() => {});
  };

  const handleBarClick = (e: React.MouseEvent<HTMLElement>) => {
    if ((e.target as HTMLElement).closest('button')) return;
    if (startOpen || activeFlyoutRef.current !== '') {
      dismissOverlays().catch(() => {});
    }
  };

  const handlePinDragStart = (e: React.DragEvent<HTMLButtonElement>, item: MergedBarItem) => {
    if (!item.pinned) {
      e.preventDefault();
      return;
    }
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    hideHoverPill();
    setDragPinId(item.id);
    setDragOverPinId(null);
    e.dataTransfer.effectAllowed = 'move';
    try {
      e.dataTransfer.setData('text/plain', item.id);
    } catch {
      // Ignore synthetic environments without dataTransfer backing store
    }
  };

  const handlePinDragOver = (e: React.DragEvent<HTMLButtonElement>, item: MergedBarItem) => {
    if (!dragPinId || !item.pinned || item.id === dragPinId) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    if (dragOverPinId !== item.id) {
      setDragOverPinId(item.id);
    }
  };

  const handlePinDragLeave = (item: MergedBarItem) => {
    if (dragOverPinId === item.id) {
      setDragOverPinId(null);
    }
  };

  const handlePinDrop = (e: React.DragEvent<HTMLButtonElement>, targetItem: MergedBarItem) => {
    e.preventDefault();
    const sourceId = dragPinId;
    setDragPinId(null);
    setDragOverPinId(null);
    if (!sourceId || !targetItem.pinned) return;
    const indices = resolvePinnedReorderIndices(barItems, sourceId, targetItem.id);
    if (!indices) return;
    ipc('reorder_pins', {
      space: 'taskbar',
      fromIdx: indices.fromIdx,
      toIdx: indices.toIdx,
    }).catch(() => {});
  };

  const handlePinDragEnd = () => {
    setDragPinId(null);
    setDragOverPinId(null);
  };

  const applyVolumeChange = (
    el: HTMLElement,
    nextVolume: number,
    nextMuted: boolean,
  ) => {
    const plan = planSoundFeedbackAction(
      activeFlyoutRef.current,
      nextVolume,
      nextMuted,
      anchorX(el),
      monitor,
    );
    setAudioMaster((prev) => ({
      available: prev?.available ?? true,
      masterVolume: plan.volume,
      masterMuted: plan.muted,
      micMuted: prev?.micMuted ?? false,
      defaultDeviceShortName: prev?.defaultDeviceShortName ?? '',
    }));
    if (plan.openOsd && plan.req) {
      ipc('open_flyout', { req: plan.req }).catch(() => {});
    }
    ipc<AudioState>('set_master_volume', { volume: plan.volume, muted: plan.muted })
      .then((st) => setAudioMaster(audioStateToMaster(st)))
      .catch(() => {});
  };

  const handleSoundWheel = (e: React.WheelEvent<HTMLButtonElement>) => {
    e.preventDefault();
    const cur = audioMaster?.masterVolume ?? 0;
    const next = computeNextWheelVolume(cur, e.deltaY, 4);
    applyVolumeChange(e.currentTarget, next, false);
  };

  const handleSoundAux = (e: React.MouseEvent<HTMLButtonElement>) => {
    if (e.button === 1) {
      e.preventDefault();
      const curVol = audioMaster?.masterVolume ?? 0;
      const nextMute = !(audioMaster?.masterMuted ?? false);
      applyVolumeChange(e.currentTarget, curVol, nextMute);
    }
  };

  const clock = formatTaskbarClock(now, { showSeconds, showWeekday });
  const soundTooltip = formatSoundNetworkTooltip(audioMaster, net);
  const netConnected = net ? net.connected : true;

  return (
    <div
      className={getTaskbarRootClassName(config?.barMode)}
      data-space="personal"
      data-monitor={monitor}
    >
      <nav
        ref={barRef}
        className={`taskbar has-sheen ${useEdgeCentered ? 'edge-centered' : ''}`}
        onClick={handleBarClick}
        onContextMenu={handleBarCtx}
        onPointerEnter={handleBarPointerEnter}
        onPointerMove={handleBarPointerMove}
      >
        <span ref={sheenRef} className="tb-sheen" aria-hidden="true" />

        {useEdgeCentered && <div className="tb-edge-left" aria-hidden="true" />}

        <div className="tb-group tb-apps has-hover-pill">
          <div ref={appsScrollRef} className="tb-apps-scroll" onMouseLeave={hideHoverPill}>
            <span ref={hoverPillRef} className="tb-hover-pill" aria-hidden="true" />

            <button
              type="button"
              className={`tb-btn ${startOpen ? 'start-on' : ''}`}
              aria-label="Пуск"
              onMouseEnter={(e) => moveHoverPillTo(e.currentTarget)}
              onClick={() => toggleStart(undefined, monitor).catch(() => {})}
            >
              <span className="tb-ic">
                <StartLogoIcon />
              </span>
            </button>

            {barItems.map((item) => {
              const winCount = item.group?.windows.length ?? 0;
              const stateCls = getTaskbarItemStateClass(item);
              const isLaunching = Boolean(launchingIds[item.id]);
              const isDragging = dragPinId === item.id;
              const isDragOver = dragOverPinId === item.id;
              const btnCls = [
                'tb-btn',
                stateCls,
                isLaunching ? 'launching' : '',
                isDragging ? 'dragging' : '',
                isDragOver ? 'drag-over' : '',
              ]
                .filter(Boolean)
                .join(' ');

              return (
                <button
                  key={item.id}
                  type="button"
                  className={btnCls}
                  aria-label={item.name}
                  draggable={item.pinned}
                  onClick={() => handleAppClick(item)}
                  onAuxClick={(e) => handleAppAuxClick(e, item)}
                  onContextMenu={(e) => handleAppCtx(e, item)}
                  onMouseEnter={(e) => handleAppHover(item, e.currentTarget)}
                  onMouseLeave={handleAppLeave}
                  onDragStart={(e) => handlePinDragStart(e, item)}
                  onDragOver={(e) => handlePinDragOver(e, item)}
                  onDragLeave={() => handlePinDragLeave(item)}
                  onDrop={(e) => handlePinDrop(e, item)}
                  onDragEnd={handlePinDragEnd}
                >
                  <span className="tb-ic">
                    {item.iconData ? (
                      <img src={item.iconData} alt="" className="native-ic" draggable={false} />
                    ) : (
                      <FallbackAppIcon iconKey={item.iconKey} size={22} />
                    )}
                  </span>
                  <span className="tb-indicators" aria-hidden="true">
                    <span className="tb-pill" />
                    {winCount > 1 && <span className="tb-multi" />}
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        <div ref={trayGroupRef} className="tb-group tb-tray">
          {!isEdge && <div className="tb-sep" />}

          <button
            type="button"
            className={`tb-tray-btn chevron ${activeFlyout === 'tray' ? 'open' : ''}`}
            aria-label="Фоновые приложения"
            onClick={(e) => openFlyoutAt('tray', e.currentTarget, TRAY_CARD_WIDTH, TRAY_MIN_CARD_HEIGHT, false)}
          >
            <ChevronUpIcon className="ic14" />
          </button>

          <button
            type="button"
            className="tb-tray-btn tb-lang-btn"
            aria-label={`Раскладка клавиатуры: ${layout}`}
            onClick={() => ipc<string>('switch_keyboard_layout').then(setLayout).catch(() => {})}
          >
            {layout}
          </button>

          <button
            type="button"
            className={`tb-tray-btn tb-sound-btn ${activeFlyout === 'sound' || activeFlyout === 'quick' ? 'open' : ''} ${audioMaster?.masterMuted ? 'muted' : ''}`}
            aria-label={soundTooltip}
            onClick={(e) =>
              openFlyoutAt('sound', e.currentTarget, SOUND_FLYOUT_WIDTH, SOUND_FLYOUT_INITIAL_HEIGHT, false)
            }
            onWheel={handleSoundWheel}
            onAuxClick={handleSoundAux}
          >
            <span className={!netConnected ? 'tb-net-offline' : undefined}>
              <NetIcon kind={net?.kind} connected={netConnected} className="ic14" />
            </span>
            <span className="tb-vol-ic">
              <VolIcon muted={audioMaster?.masterMuted} level={audioMaster?.masterVolume} className="ic14" />
            </span>
          </button>

          <div className="tb-tray-sep" aria-hidden="true" />

          <button
            type="button"
            className={`tb-tray-btn tb-clock ${activeFlyout === 'calendar' ? 'open' : ''}`}
            aria-label={clock.fullTitle}
            onClick={(e) =>
              openFlyoutAt('calendar', e.currentTarget, CALENDAR_FLYOUT_WIDTH, CALENDAR_FLYOUT_INITIAL_HEIGHT, false)
            }
          >
            <span className="tb-clock-time">{clock.timeStr}</span>
            <span className="tb-clock-date">{clock.dateStr}</span>
          </button>
        </div>
      </nav>
    </div>
  );
};
