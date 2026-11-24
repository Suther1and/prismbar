import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import '../styles/widgets.css';
import { TRAY_CARD_WIDTH, TRAY_MIN_CARD_HEIGHT } from '../lib/flyoutLogic';
import {
  applyConfigUpdate,
  dismissOverlays,
  fetchIconsBatch,
  getNetworkSnapshot,
  getOpenWindows,
  getPeripheralBatteries,
  getWeather,
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
  type MediaState,
  type MergedBarItem,
  type NetworkInfo,
  type NetworkSnapshot,
  type PeripheralBattery,
  type StartStateEvent,
  type TaskbarWidgetId,
  type TelemetryState,
  type TestBarConfig,
  type WeatherSnapshot,
} from '../lib/ipc';
import {
  BATTERY_FLYOUT_HEIGHT,
  BATTERY_FLYOUT_WIDTH,
  buildAppContextMenuRequest,
  buildBarContextMenuRequest,
  buildPreviewFlyoutRequest,
  buildStartContextMenuRequest,
  CALENDAR_FLYOUT_INITIAL_HEIGHT,
  CALENDAR_FLYOUT_WIDTH,
  computeNaturalDockWidth,
  computeNextWheelVolume,
  createTaskbarOrderMemory,
  filterGroupsForMonitor,
  formatBatteryPillSummary,
  formatFocusTimerMmSs,
  formatMiniTelemetryLabel,
  formatSoundNetworkTooltip,
  formatTaskbarClock,
  formatTaskbarMediaLabel,
  formatTaskbarNetworkBadge,
  formatWeatherTempLabel,
  getFocusTimerEffectiveRemainingSec,
  getPreviewHoverDelayMs,
  getTaskbarItemStateClass,
  getTaskbarRootClassName,
  NETWORK_FLYOUT_HEIGHT,
  NETWORK_FLYOUT_WIDTH,
  parseFocusTimerSnapshot,
  planSoundFeedbackAction,
  reorderUnpinnedIds,
  resolveActiveTaskbarWidgets,
  resolveHorizontalDropTarget,
  resolvePinnedReorderIndices,
  resolveTaskbarItemClick,
  resolveTaskbarMonitor,
  resolveTaskbarWidgetsPosition,
  selectTaskbarPinsForMonitor,
  shouldTriggerPreviewHover,
  shouldUseEdgeCenteredLayout,
  SOUND_FLYOUT_INITIAL_HEIGHT,
  SOUND_FLYOUT_WIDTH,
  stabilizeTaskbarItemsOrder,
  TASKBAR_LAUNCH_HOP_MS,
  TELEMETRY_FLYOUT_HEIGHT,
  TELEMETRY_FLYOUT_WIDTH,
  TIMER_STORAGE_KEY,
  TIMER_UPDATED_EVENT,
  WEATHER_FLYOUT_HEIGHT,
  WEATHER_FLYOUT_WIDTH,
  type FocusTimerSnapshot,
  type TaskbarMonitor,
} from '../lib/taskbarLogic';
import { hasExceededDragThreshold } from '../lib/startLogic';
import { useReducedMotion } from '../lib/useReducedMotion';
import {
  ChargingBoltIcon,
  ChevronUpIcon,
  FallbackAppIcon,
  MusicNoteIcon,
  NetworkSpeedIcon,
  OfflineIcon,
  PauseIcon,
  PeripheralDeviceIcon,
  PlaySmallIcon,
  StartLogoIcon,
  TimerIcon,
  VolIcon,
  WeatherConditionIcon,
  WifiIcon,
} from './Icons';

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

export interface TaskbarViewProps {
  monitorOverride?: TaskbarMonitor;
}

export const TaskbarView: React.FC<TaskbarViewProps> = ({ monitorOverride }) => {
  const monitor = useMemo<TaskbarMonitor>(
    () =>
      monitorOverride ??
      (typeof window !== 'undefined' ? resolveTaskbarMonitor(window.location.search) : 'primary'),
    [monitorOverride],
  );
  const isSecondary = monitor === 'secondary';
  const reducedMotion = useReducedMotion();

  const [config, setConfig] = useState<TestBarConfig | null>(null);
  const [groups, setGroups] = useState<AppWindowGroup[]>([]);
  const [audioMaster, setAudioMaster] = useState<AudioMasterState | null>(null);
  const [net, setNet] = useState<NetworkInfo | null>(null);
  const [netSnap, setNetSnap] = useState<NetworkSnapshot | null>(null);
  const [media, setMedia] = useState<MediaState | null>(null);
  const [layout, setLayout] = useState('RU');
  const [weather, setWeather] = useState<WeatherSnapshot | null>(null);
  const [periphs, setPeriphs] = useState<PeripheralBattery[]>([]);
  const [telemetry, setTelemetry] = useState<TelemetryState | null>(null);
  const [timerSnap, setTimerSnap] = useState<FocusTimerSnapshot>(() =>
    typeof window !== 'undefined'
      ? parseFocusTimerSnapshot(window.localStorage?.getItem(TIMER_STORAGE_KEY))
      : parseFocusTimerSnapshot(null),
  );
  const [timerNowMs, setTimerNowMs] = useState<number>(() => Date.now());
  const [startOpen, setStartOpen] = useState(false);
  const [startFullscreenOverlay, setStartFullscreenOverlay] = useState(false);
  const [activeFlyout, setActiveFlyout] = useState<string>('');
  const [now, setNow] = useState(() => new Date());
  const [iconMap, setIconMap] = useState<Record<string, string>>({});
  const [launchingIds, setLaunchingIds] = useState<Record<string, true>>({});
  const [dragPinId, setDragPinId] = useState<string | null>(null);
  const [dragOverPinId, setDragOverPinId] = useState<string | null>(null);
  const [dragOverSide, setDragOverSide] = useState<'before' | 'after' | null>(null);
  const [unpinnedOrderRev, setUnpinnedOrderRev] = useState(0);
  const [tbHidden, setTbHidden] = useState(false);
  const [viewportWidth, setViewportWidth] = useState<number>(() =>
    typeof window !== 'undefined' ? window.innerWidth : 1920,
  );

  const barRef = useRef<HTMLElement>(null);
  const appsScrollRef = useRef<HTMLDivElement>(null);
  const trayGroupRef = useRef<HTMLDivElement>(null);
  const leftWidgetsGroupRef = useRef<HTMLDivElement>(null);
  const mediaPillRef = useRef<HTMLDivElement>(null);
  const barRectRef = useRef<DOMRect | null>(null);
  const orderMemoryRef = useRef(createTaskbarOrderMemory());
  const hydratedOrderMonitorRef = useRef<string | null>(null);
  if (hydratedOrderMonitorRef.current !== monitor) {
    hydratedOrderMonitorRef.current = monitor;
    if (typeof window !== 'undefined' && window.localStorage) {
      try {
        const raw = window.localStorage.getItem(`prismbar-open-order-${monitor}`);
        if (raw) {
          const parsed = JSON.parse(raw);
          if (Array.isArray(parsed)) {
            orderMemoryRef.current.unpinnedIds = parsed.filter(
              (id): id is string => typeof id === 'string' && id.length > 0,
            );
          }
        }
      } catch {
        // Ignore storage read errors
      }
    }
  }
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
  const pointerDragCleanupRef = useRef<(() => void) | null>(null);
  const suppressClickUntilRef = useRef<number>(0);
  const barItemsRef = useRef<MergedBarItem[]>([]);

  iconMapRef.current = iconMap;

  const showSeconds = Boolean(config?.clockShowSeconds);
  const showWeekday = Boolean(config?.clockShowWeekday);
  const clockDateFormat = config?.clockDateFormat ?? 'short';
  const clockLayout = config?.clockLayout ?? 'center';
  const isEdge = config?.barMode === 'edge';
  const useEdgeCentered = shouldUseEdgeCenteredLayout(config?.barMode, monitor, viewportWidth);

  const legacyWeatherEnabled = config?.taskbarWidgets?.showWeather === true;
  const activeWidgets = useMemo<TaskbarWidgetId[]>(() => {
    if (isSecondary || !config?.taskbarWidgets) return [];
    return resolveActiveTaskbarWidgets({
      ...config.taskbarWidgets,
      showWeather: legacyWeatherEnabled,
    });
  }, [isSecondary, config?.taskbarWidgets, legacyWeatherEnabled]);
  const widgetsPosition = resolveTaskbarWidgetsPosition(config?.taskbarWidgets);

  const showWeather = !isSecondary && activeWidgets.includes('weather');
  const showBatteries = !isSecondary && activeWidgets.includes('batteries');
  const showTelemetry = !isSecondary && activeWidgets.includes('telemetry');
  const showNetwork = !isSecondary && activeWidgets.includes('network');
  const showMedia = !isSecondary && activeWidgets.includes('media');
  const showTimer = !isSecondary && activeWidgets.includes('timer');

  const weatherUnits: 'c' | 'f' = config?.taskbarWidgets?.weatherUnits === 'f' ? 'f' : 'c';
  const weatherCityKey = config?.taskbarWidgets?.weatherCity ?? '';
  const weatherLatKey = config?.taskbarWidgets?.weatherLat ?? null;
  const weatherLonKey = config?.taskbarWidgets?.weatherLon ?? null;

  const weatherTempStr = showWeather
    ? formatWeatherTempLabel(weather?.available ? weather.tempC : null, weatherUnits)
    : '';
  const visiblePeriphList = useMemo(() => {
    const hiddenIds = config?.hiddenBatteryIds;
    if (!hiddenIds || hiddenIds.length === 0) return periphs;
    const hiddenSet = new Set(hiddenIds.map((id) => id.trim().toLowerCase()));
    return periphs.filter((p) => !hiddenSet.has(p.id.trim().toLowerCase()));
  }, [periphs, config?.hiddenBatteryIds]);
  const periphSummary = useMemo(
    () => (showBatteries ? formatBatteryPillSummary(visiblePeriphList) : null),
    [showBatteries, visiblePeriphList],
  );
  const periphBadgeKey = periphSummary
    ? `${periphSummary.device.kind}:${periphSummary.percent}:${periphSummary.charging ? 1 : 0}:${periphSummary.count}`
    : '';
  const telemetryBadgeStr = showTelemetry ? formatMiniTelemetryLabel(telemetry) : '';
  const netBadge = useMemo(
    () => (showNetwork ? formatTaskbarNetworkBadge(netSnap) : null),
    [showNetwork, netSnap],
  );
  const mediaSummary = useMemo(
    () => (showMedia ? formatTaskbarMediaLabel(media) : null),
    [showMedia, media],
  );
  const effectiveTimerSec = showTimer
    ? getFocusTimerEffectiveRemainingSec(timerSnap, timerNowMs)
    : 0;
  const timerLabel = showTimer ? formatFocusTimerMmSs(effectiveTimerSec) : '';
  const hasAnyWidget =
    !isSecondary &&
    activeWidgets.some((id) => id !== 'batteries' || periphSummary !== null);

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
      if (pointerDragCleanupRef.current) {
        pointerDragCleanupRef.current();
        pointerDragCleanupRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    ipc<TestBarConfig>('get_config')
      .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
      .catch(() => {});
    // Primary monitor fetches all open windows so pinned items running on Monitor 2
    // can resolve native icons and focus across monitors without duplicating unpinned slots.
    getOpenWindows(isSecondary ? monitor : undefined).then(setGroups).catch(() => {});
    if (!isSecondary) {
      ipc<AudioMasterState>('get_audio_master_state').then(setAudioMaster).catch(() => {});
      ipc<NetworkInfo>('get_network_info').then(setNet).catch(() => {});
      ipc<string>('get_keyboard_layout').then(setLayout).catch(() => {});
    }

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
      ...(!isSecondary
        ? [
            onEvent<AudioMasterState>('audio-master-updated', setAudioMaster),
            onEvent<NetworkInfo>('network-updated', setNet),
            onEvent<string>('layout-updated', setLayout),
          ]
        : []),
      onEvent<boolean | StartStateEvent>('start-state', (ev) => {
        const norm = normalizeStartStatePayload(ev);
        setStartOpen(norm.open);
        setStartFullscreenOverlay(
          Boolean(norm.open && norm.fullscreenOverlay && (!norm.monitor || norm.monitor === monitor)),
        );
      }),
      onEvent<{ kind: string }>('flyout-show', (f) => {
        activeFlyoutRef.current = f.kind;
        setActiveFlyout(f.kind);
      }),
      onEvent<unknown>('flyout-hide', clearActiveFlyout),
      onEvent<unknown>('flyout-hide-immediate', clearActiveFlyout),
      onEvent<{ visible: boolean; gen: number }>('taskbar-visibility', (ev) => {
        if (ev && typeof ev.visible === 'boolean') {
          setTbHidden(!ev.visible);
        }
      }),
    ];
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, [monitor, isSecondary]);

  // Primary Weather widget lazy fetch + 15-min refresh + live event updates
  useEffect(() => {
    if (!showWeather) {
      setWeather((prev) => (prev === null ? prev : null));
      return;
    }
    let cancelled = false;
    const loadWeather = () => {
      getWeather({ force: false })
        .then((snap) => {
          if (!cancelled) setWeather(snap);
        })
        .catch(() => {});
    };
    loadWeather();
    const unlistenPromise = onEvent<WeatherSnapshot>('weather-updated', (snap) => {
      if (!cancelled && snap) setWeather(snap);
    });
    const timer = window.setInterval(loadWeather, 15 * 60 * 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      unlistenPromise.then((u) => u());
    };
  }, [showWeather, weatherCityKey, weatherLatKey, weatherLonKey]);

  // Primary Batteries widget: fetch + poll ONLY when enabled
  useEffect(() => {
    if (!showBatteries) {
      setPeriphs((prev) => (prev.length === 0 ? prev : []));
      return;
    }
    let cancelled = false;
    let inflight = false;
    const loadPeriphs = () => {
      if (cancelled || inflight) return;
      inflight = true;
      getPeripheralBatteries()
        .then((items) => {
          if (!cancelled && Array.isArray(items)) setPeriphs(items);
        })
        .catch(() => {})
        .finally(() => {
          inflight = false;
        });
    };
    loadPeriphs();
    const unlistenPromise = onEvent<PeripheralBattery[]>('peripherals-updated', (items) => {
      if (!cancelled && Array.isArray(items)) setPeriphs(items);
    });
    const timer = window.setInterval(loadPeriphs, 30 * 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      unlistenPromise.then((u) => u());
    };
  }, [showBatteries]);

  // Primary Mini PC Status widget: fetch & poll get_telemetry ONLY when enabled
  useEffect(() => {
    if (!showTelemetry) {
      setTelemetry((prev) => (prev === null ? prev : null));
      return;
    }
    let cancelled = false;
    let inflight = false;
    const loadTelemetry = () => {
      if (cancelled || inflight) return;
      inflight = true;
      ipc<TelemetryState>('get_telemetry')
        .then((snap) => {
          if (!cancelled && snap) setTelemetry(snap);
        })
        .catch(() => {})
        .finally(() => {
          inflight = false;
        });
    };
    loadTelemetry();
    const unlistenPromise = onEvent<TelemetryState>('telemetry-updated', (snap) => {
      if (!cancelled && snap) setTelemetry(snap);
    });
    const timer = window.setInterval(loadTelemetry, 3000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      unlistenPromise.then((u) => u());
    };
  }, [showTelemetry]);

  // Primary Network Speed & Ping widget: poll get_network_snapshot ONLY when enabled
  useEffect(() => {
    if (!showNetwork) {
      setNetSnap((prev) => (prev === null ? prev : null));
      return;
    }
    let cancelled = false;
    let inflight = false;
    let tick = 0;
    const loadNetSnap = (probePing: boolean) => {
      if (cancelled || inflight) return;
      inflight = true;
      getNetworkSnapshot(probePing)
        .then((snap) => {
          if (!cancelled && snap) setNetSnap(snap);
        })
        .catch(() => {})
        .finally(() => {
          inflight = false;
        });
    };
    loadNetSnap(true);
    const timer = window.setInterval(() => {
      tick += 1;
      loadNetSnap(tick % 12 === 0);
    }, 2500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [showNetwork]);

  // Primary Media widget: fetch + poll get_media_state ONLY when enabled
  useEffect(() => {
    if (!showMedia) {
      setMedia((prev) => (prev === null ? prev : null));
      return;
    }
    let cancelled = false;
    let inflight = false;
    const loadMedia = () => {
      if (cancelled || inflight) return;
      inflight = true;
      ipc<MediaState>('get_media_state')
        .then((snap) => {
          if (!cancelled && snap) setMedia(snap);
        })
        .catch(() => {})
        .finally(() => {
          inflight = false;
        });
    };
    loadMedia();
    const unlistenPromise = onEvent<MediaState>('media-updated', (snap) => {
      if (!cancelled && snap) setMedia(snap);
    });
    const timer = window.setInterval(loadMedia, 2500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      unlistenPromise.then((u) => u());
    };
  }, [showMedia]);

  // Primary Focus Timer widget: self-contained localStorage + custom event subscription
  useEffect(() => {
    if (!showTimer || typeof window === 'undefined') return;
    const syncFromStorage = () => {
      try {
        const next = parseFocusTimerSnapshot(window.localStorage?.getItem(TIMER_STORAGE_KEY));
        setTimerSnap(next);
        setTimerNowMs(Date.now());
      } catch {
        // Ignore storage read errors
      }
    };
    syncFromStorage();
    const onStorage = (e: StorageEvent) => {
      if (e.key === null || e.key === TIMER_STORAGE_KEY) {
        syncFromStorage();
      }
    };
    const onCustom = (e: Event) => {
      const detail = (e as CustomEvent<unknown>).detail;
      if (detail && typeof detail === 'object') {
        try {
          setTimerSnap(parseFocusTimerSnapshot(JSON.stringify(detail)));
          setTimerNowMs(Date.now());
          return;
        } catch {
          // Fallback to storage read
        }
      }
      syncFromStorage();
    };
    window.addEventListener('storage', onStorage);
    window.addEventListener(TIMER_UPDATED_EVENT, onCustom);
    return () => {
      window.removeEventListener('storage', onStorage);
      window.removeEventListener(TIMER_UPDATED_EVENT, onCustom);
    };
  }, [showTimer]);

  useEffect(() => {
    if (!showTimer || !timerSnap.running || typeof window === 'undefined') return;
    setTimerNowMs(Date.now());
    const timer = window.setInterval(() => {
      setTimerNowMs(Date.now());
    }, 1000);
    return () => window.clearInterval(timer);
  }, [showTimer, timerSnap.running, timerSnap.endEpochMs]);

  // Load real native .exe, WindowsApps aliases, and shell:AppsFolder\ UWP icons for pinned taskbar apps (primary only)
  useEffect(() => {
    if (!config || isSecondary) return;

    const targetsToResolve: string[] = [];
    for (const p of config.taskbarPins) {
      const target = p.target?.trim() ?? '';
      const low = target.toLowerCase();
      const canResolve =
        target.includes(':\\') ||
        target.startsWith('\\\\') ||
        low.startsWith('shell:appsfolder\\') ||
        low.startsWith('ms-settings:') ||
        low.startsWith('http://') ||
        low.startsWith('https://');
      if (
        !target ||
        !canResolve ||
        p.iconData ||
        iconMapRef.current[target] ||
        iconMapRef.current[low] ||
        requestedTargetsRef.current.has(low)
      ) {
        continue;
      }
      requestedTargetsRef.current.add(low);
      targetsToResolve.push(target);
    }

    if (targetsToResolve.length === 0) return;

    ipc<Record<string, string>>('resolve_pin_icons', { targets: targetsToResolve })
      .then((batch) => {
        if (!mountedRef.current) {
          for (const t of targetsToResolve) {
            requestedTargetsRef.current.delete(t.toLowerCase());
          }
          return;
        }
        const updates: Record<string, string> = {};
        const unresolved: string[] = [];
        for (const t of targetsToResolve) {
          const hit = batch?.[t] ?? batch?.[t.toLowerCase()];
          if (hit) {
            updates[t] = hit;
            updates[t.toLowerCase()] = hit;
          } else {
            unresolved.push(t);
          }
        }
        if (Object.keys(updates).length > 0) {
          setIconMap((prev) => ({ ...prev, ...updates }));
        }
        // Fallback per-path lookup for any local exe/UWP target missed by batch
        for (const t of unresolved) {
          const low = t.toLowerCase();
          if (low.startsWith('http://') || low.startsWith('https://')) {
            continue;
          }
          ipc<string | null>('get_icon_for_path', { path: t })
            .then((data) => {
              if (!mountedRef.current) {
                requestedTargetsRef.current.delete(low);
                return;
              }
              if (data) {
                setIconMap((prev) =>
                  prev[t] === data ? prev : { ...prev, [t]: data, [low]: data },
                );
              } else {
                requestedTargetsRef.current.delete(low);
              }
            })
            .catch(() => {
              requestedTargetsRef.current.delete(low);
            });
        }
      })
      .catch(() => {
        for (const t of targetsToResolve) {
          requestedTargetsRef.current.delete(t.toLowerCase());
        }
      });
  }, [config, isSecondary]);

  const visibleGroups = useMemo<AppWindowGroup[]>(
    () => filterGroupsForMonitor(groups, monitor),
    [groups, monitor],
  );

  // Batch-fetch missing cached icon keys from Rust IconCache for running windows (including cross-monitor pins) & cache-keyed pins
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

    const groupsForIcons = isSecondary ? visibleGroups : groups;
    for (const g of groupsForIcons) {
      checkKey(g.iconKey);
    }
    if (!isSecondary) {
      for (const p of config?.taskbarPins ?? []) {
        checkKey(p.iconKey);
      }
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
  }, [groups, visibleGroups, config, isSecondary]);

  // Merge monitor-appropriate pins (Option A: none on secondary) + monitor-filtered open windows,
  // passing full `groups` on primary so cross-monitor pinned apps (e.g. Spark on Monitor 2) resolve icons & click-to-focus.
  const barItems = useMemo<MergedBarItem[]>(
    () =>
      stabilizeTaskbarItemsOrder(
        mergeTaskbarItems(
          selectTaskbarPinsForMonitor(config?.taskbarPins, monitor),
          visibleGroups,
          iconMap,
          !isSecondary ? groups : undefined,
        ),
        orderMemoryRef.current,
      ),
    [config, visibleGroups, groups, iconMap, monitor, isSecondary, unpinnedOrderRev],
  );
  barItemsRef.current = barItems;

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
      const leftEl = leftWidgetsGroupRef.current;
      const appsNatural = scrollEl ? scrollEl.scrollWidth : 0;
      const trayNatural = trayEl ? trayEl.scrollWidth : 0;
      const leftNatural =
        widgetsPosition === 'left' && leftEl ? leftEl.scrollWidth : 0;
      const w = computeNaturalDockWidth(
        el.scrollWidth,
        rect.width,
        appsNatural,
        trayNatural,
        leftNatural,
      );
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
    if (leftWidgetsGroupRef.current) ro.observe(leftWidgetsGroupRef.current);
    return () => {
      if (rafId !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(rafId);
      }
      ro.disconnect();
    };
  }, [
    barItems.length,
    config?.barMode,
    showSeconds,
    showWeekday,
    clockDateFormat,
    clockLayout,
    layout,
    monitor,
    widgetsPosition,
    activeWidgets,
    config?.taskbarWidgets?.weatherUnits,
    weatherTempStr,
    periphBadgeKey,
    telemetryBadgeStr,
    netBadge?.text,
    mediaSummary?.title,
    mediaSummary?.hasSession,
    timerLabel,
    net,
  ]);

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

  const clearLaunchHop = useCallback((id: string) => {
    const timerId = launchTimersRef.current.get(id);
    if (timerId !== undefined) {
      window.clearTimeout(timerId);
      launchTimersRef.current.delete(id);
    }
    setLaunchingIds((prev) => {
      if (!prev[id]) return prev;
      const next = { ...prev };
      delete next[id];
      return next;
    });
  }, []);

  const triggerLaunchHop = useCallback(
    (id: string) => {
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
    },
    [],
  );

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

    // Never double-stack sliding hover pill behind an active app or open Start button
    if (btn.classList.contains('active') || btn.classList.contains('start-on')) {
      hideHoverPill();
      return;
    }

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
  }, [dragPinId, hideHoverPill]);

  const isHoverGlowOff = () =>
    typeof document !== 'undefined' && document.documentElement.dataset.hoverGlow === 'off';

  const handleBarPointerEnter = useCallback(
    (e: React.PointerEvent<HTMLElement>) => {
      const rect = e.currentTarget.getBoundingClientRect();
      barRectRef.current = rect;
      if (!reducedMotion && !isHoverGlowOff() && sheenRef.current) {
        const localX = e.clientX - rect.left;
        sheenRef.current.style.transform = `translate3d(${Math.round(localX - 120)}px, 0, 0)`;
      }
    },
    [reducedMotion],
  );

  const handleBarPointerMove = useCallback(
    (e: React.PointerEvent<HTMLElement>) => {
      if (reducedMotion || isHoverGlowOff()) return;
      sheenClientXRef.current = e.clientX;
      if (sheenRafRef.current !== null) return;
      if (typeof requestAnimationFrame !== 'function') return;
      sheenRafRef.current = requestAnimationFrame(() => {
        sheenRafRef.current = null;
        if (isHoverGlowOff()) return;
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
    if (typeof performance !== 'undefined' && performance.now() < suppressClickUntilRef.current) {
      return;
    }
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    const action = resolveTaskbarItemClick(item);
    if (!action) return;
    if (action.type === 'focus') {
      ipc('focus_window', {
        hwnd: action.hwnd,
        toggleMinimize: action.toggleMinimize,
      }).catch(() => {});
    } else if (action.target === 'cmd:settings') {
      ipc('open_start_settings').catch(() => {});
    } else {
      triggerLaunchHop(item.id);
      const matchedPin = item.pinId
        ? config?.taskbarPins?.find((pin) => pin.id === item.pinId)
        : undefined;
      ipc('launch_item', {
        target: action.target,
        args: action.args,
        asAdmin: Boolean(matchedPin?.runAsAdmin),
        closeStart: true,
      }).catch(() => {
        clearLaunchHop(item.id);
      });
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
      item.remoteGroup?.exePath ||
      (item.matchAumid ? `shell:AppsFolder\\${item.matchAumid}` : '');
    if (!launchTarget) return;
    if (launchTarget === 'cmd:settings') {
      ipc('open_start_settings').catch(() => {});
      return;
    }
    if (!item.group || item.group.windows.length === 0) {
      triggerLaunchHop(item.id);
    }
    launchNewWindow(launchTarget, item.args ?? null, false).catch(() => {
      ipc('launch_item', {
        target: launchTarget,
        args: item.args ?? null,
        asAdmin: false,
        closeStart: true,
      }).catch(() => {
        clearLaunchHop(item.id);
      });
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
    if (!shouldTriggerPreviewHover(item, startOpen, activeFlyoutRef.current, startFullscreenOverlay)) {
      return;
    }

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

  const handleStartCtx = (e: React.MouseEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    const req = buildStartContextMenuRequest(anchorX(e.currentTarget), monitor);
    ipc('open_flyout', { req }).catch(() => {});
  };

  const handleBarClick = (e: React.MouseEvent<HTMLElement>) => {
    if ((e.target as HTMLElement).closest('button')) return;
    dismissOverlays().catch(() => {});
  };

  // Pointer-based icon reordering for BOTH pinned & unpinned open taskbar items
  const handlePinPointerDown = (e: React.PointerEvent<HTMLButtonElement>, item: MergedBarItem) => {
    if (e.button !== 0 || typeof window === 'undefined') return;
    if (pointerDragCleanupRef.current) {
      pointerDragCleanupRef.current();
      pointerDragCleanupRef.current = null;
    }

    const startX = e.clientX;
    const startY = e.clientY;
    const sourceId = item.id;
    const isPinnedSource = item.pinned;
    const pointerId = e.pointerId;
    const btnEl = e.currentTarget;
    let active = false;
    let currentOverId: string | null = null;
    let currentOverSide: 'before' | 'after' | null = null;

    const releaseCapture = () => {
      try {
        if (btnEl.hasPointerCapture?.(pointerId)) {
          btnEl.releasePointerCapture(pointerId);
        }
      } catch {
        // Ignore capture release errors
      }
    };

    const onMove = (ev: PointerEvent) => {
      if (!active) {
        if (!hasExceededDragThreshold(startX, startY, ev.clientX, ev.clientY)) {
          return;
        }
        active = true;
        window.getSelection?.()?.removeAllRanges();
        try {
          btnEl.setPointerCapture?.(pointerId);
        } catch {
          // Ignore pointer capture errors
        }
        if (hoverTimer.current !== null) {
          window.clearTimeout(hoverTimer.current);
          hoverTimer.current = null;
        }
        hideHoverPill();
        setDragPinId(sourceId);
      }

      const container = appsScrollRef.current;
      const buttons = container
        ? Array.from(
            container.querySelectorAll<HTMLElement>(
              `[data-bar-id][data-bar-pinned="${isPinnedSource ? '1' : '0'}"]`,
            ),
          )
        : [];
      const candidates = buttons
        .map((el) => {
          const rect = el.getBoundingClientRect();
          return {
            id: el.dataset.barId ?? '',
            left: rect.left,
            right: rect.right,
          };
        })
        .filter((c) => Boolean(c.id));

      const resolved = resolveHorizontalDropTarget(candidates, ev.clientX, sourceId);
      const nextOver = resolved?.targetId ?? null;
      const nextSide = resolved?.side ?? null;
      if (nextOver !== currentOverId || nextSide !== currentOverSide) {
        currentOverId = nextOver;
        currentOverSide = nextSide;
        setDragOverPinId(nextOver);
        setDragOverSide(nextSide);
      }
    };

    const cleanup = () => {
      releaseCapture();
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onCancel);
      pointerDragCleanupRef.current = null;
    };

    const onCancel = () => {
      cleanup();
      if (active) {
        setDragPinId(null);
        setDragOverPinId(null);
        setDragOverSide(null);
      }
    };

    const onUp = () => {
      cleanup();
      if (!active) return;
      suppressClickUntilRef.current = performance.now() + 240;
      const targetId = currentOverId;
      setDragPinId(null);
      setDragOverPinId(null);
      setDragOverSide(null);
      if (!targetId) return;

      if (isPinnedSource) {
        const indices = resolvePinnedReorderIndices(barItemsRef.current, sourceId, targetId);
        if (indices) {
          setConfig((prev) => {
            if (!prev?.taskbarPins) return prev;
            const pins = [...prev.taskbarPins];
            if (
              indices.fromIdx < 0 ||
              indices.toIdx < 0 ||
              indices.fromIdx >= pins.length ||
              indices.toIdx >= pins.length
            ) {
              return prev;
            }
            const [moved] = pins.splice(indices.fromIdx, 1);
            pins.splice(indices.toIdx, 0, moved);
            return { ...prev, taskbarPins: pins };
          });
          ipc<TestBarConfig>('reorder_pins', {
            space: 'taskbar',
            fromIdx: indices.fromIdx,
            toIdx: indices.toIdx,
          })
            .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
            .catch(() => {
              ipc<TestBarConfig>('get_config')
                .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
                .catch(() => {});
            });
        }
      } else {
        const currentUnpinned =
          orderMemoryRef.current.unpinnedIds.length > 0
            ? orderMemoryRef.current.unpinnedIds
            : barItemsRef.current.filter((b) => !b.pinned).map((b) => b.id);
        const nextOrder = reorderUnpinnedIds(currentUnpinned, sourceId, targetId);
        orderMemoryRef.current.unpinnedIds = nextOrder;
        if (window.localStorage) {
          try {
            window.localStorage.setItem(
              `prismbar-open-order-${monitor}`,
              JSON.stringify(nextOrder),
            );
          } catch {
            // Ignore storage write errors
          }
        }
        setUnpinnedOrderRev((v) => v + 1);
      }
    };

    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
    pointerDragCleanupRef.current = cleanup;
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

  const clock = formatTaskbarClock(now, {
    showSeconds,
    showWeekday,
    dateFormat: clockDateFormat,
    layout: clockLayout,
  });
  const soundTooltip = formatSoundNetworkTooltip(audioMaster, net);
  const netConnected = net ? net.connected : true;
  const telemHot = Boolean(
    telemetry && (telemetry.cpuPct >= 85 || (telemetry.gpuAvailable && telemetry.gpuTempC >= 80)),
  );
  const telemWarm = Boolean(
    !telemHot &&
      telemetry &&
      (telemetry.cpuPct >= 60 || (telemetry.gpuAvailable && telemetry.gpuTempC >= 68)),
  );
  const timerDone = showTimer && timerSnap.running && effectiveTimerSec === 0;

  const renderWidgetPills = () =>
    activeWidgets.map((widgetId) => {
      switch (widgetId) {
        case 'weather':
          return (
            <button
              key="weather"
              type="button"
              className={`tb-tray-btn tb-widget-pill tb-weather-pill ${activeFlyout === 'weather' ? 'open' : ''} ${weather?.stale ? 'stale' : ''}`.trim()}
              data-condition={weather?.available ? weather.conditionIcon : 'partly-cloudy-day'}
              aria-label={
                weather?.available
                  ? `Погода: ${weather.city}, ${weatherTempStr}, ${weather.conditionLabel}`
                  : 'Погода'
              }
              onClick={(e) =>
                openFlyoutAt(
                  'weather',
                  e.currentTarget,
                  WEATHER_FLYOUT_WIDTH,
                  WEATHER_FLYOUT_HEIGHT,
                  false,
                )
              }
            >
              <WeatherConditionIcon
                condition={weather?.available ? weather.conditionIcon : 'partly-cloudy-day'}
                className="ic16 tb-weather-ic"
              />
              <span className="tb-widget-val">{weatherTempStr}</span>
            </button>
          );
        case 'telemetry':
          return (
            <button
              key="telemetry"
              type="button"
              className={`tb-tray-btn tb-widget-pill tb-telem-pill ${activeFlyout === 'telemetry' ? 'open' : ''} ${telemHot ? 'hot' : telemWarm ? 'warm' : ''}`.trim()}
              aria-label={`Статус ПК: ${telemetryBadgeStr}`}
              onClick={(e) =>
                openFlyoutAt(
                  'telemetry',
                  e.currentTarget,
                  TELEMETRY_FLYOUT_WIDTH,
                  TELEMETRY_FLYOUT_HEIGHT,
                  false,
                )
              }
            >
              <span className="tb-telem-dot" aria-hidden="true" />
              <span className="tb-telem-text">{telemetryBadgeStr}</span>
            </button>
          );
        case 'batteries':
          if (!periphSummary) return null;
          return (
            <button
              key="batteries"
              type="button"
              className={`tb-tray-btn tb-widget-pill tb-bat-pill ${activeFlyout === 'battery' ? 'open' : ''} ${periphSummary.critical ? 'critical' : periphSummary.low ? 'low' : ''}`.trim()}
              aria-label={periphSummary.ariaLabel}
              onClick={(e) =>
                openFlyoutAt(
                  'battery',
                  e.currentTarget,
                  BATTERY_FLYOUT_WIDTH,
                  BATTERY_FLYOUT_HEIGHT,
                  false,
                )
              }
            >
              <PeripheralDeviceIcon
                kind={periphSummary.device.kind}
                className="ic16 tb-bat-ic"
              />
              <span className="tb-widget-val tb-bat-val">{periphSummary.percentStr}</span>
              {periphSummary.charging && <ChargingBoltIcon className="ic12 tb-bat-bolt" />}
            </button>
          );
        case 'network':
          if (!netBadge) return null;
          return (
            <button
              key="network"
              type="button"
              className={`tb-tray-btn tb-widget-pill tb-net-pill ${activeFlyout === 'network' ? 'open' : ''} ${!netBadge.online ? 'offline' : ''}`.trim()}
              aria-label={netBadge.ariaLabel}
              onClick={(e) =>
                openFlyoutAt(
                  'network',
                  e.currentTarget,
                  NETWORK_FLYOUT_WIDTH,
                  NETWORK_FLYOUT_HEIGHT,
                  false,
                )
              }
            >
              {!netBadge.online ? (
                <OfflineIcon className="ic16 tb-net-ic-badge" />
              ) : netBadge.kind === 'wifi' ? (
                <WifiIcon className="ic16 tb-net-ic-badge" />
              ) : (
                <NetworkSpeedIcon className="ic16 tb-net-ic-badge" />
              )}
              <span className="tb-widget-val tb-net-val">{netBadge.text}</span>
            </button>
          );
        case 'media':
          if (!mediaSummary) return null;
          return (
            <div
              key="media"
              ref={mediaPillRef}
              className={`tb-widget-pill tb-media-pill ${activeFlyout === 'sound' || activeFlyout === 'quick' ? 'open' : ''} ${mediaSummary.hasSession ? '' : 'idle'} ${mediaSummary.isPlaying ? 'playing' : ''}`.trim()}
            >
              <button
                type="button"
                className="tb-media-open-btn"
                aria-label={mediaSummary.ariaLabel}
                onClick={(e) =>
                  openFlyoutAt(
                    'sound',
                    mediaPillRef.current ?? e.currentTarget,
                    SOUND_FLYOUT_WIDTH,
                    SOUND_FLYOUT_INITIAL_HEIGHT,
                    false,
                  )
                }
              >
                {media?.thumbnail && mediaSummary.hasSession ? (
                  <img
                    src={media.thumbnail}
                    alt=""
                    className="tb-media-pill-art"
                    draggable={false}
                  />
                ) : (
                  <span className="tb-media-ic">
                    <MusicNoteIcon className="ic14" />
                  </span>
                )}
                <span className="tb-media-pill-title">{mediaSummary.title}</span>
              </button>
              {mediaSummary.hasSession && (
                <button
                  type="button"
                  className="tb-media-play-btn"
                  aria-label={mediaSummary.isPlaying ? 'Пауза' : 'Воспроизвести'}
                  onClick={(e) => {
                    e.stopPropagation();
                    ipc<MediaState>('media_action', { action: 'toggle' })
                      .then((next) => {
                        if (mountedRef.current && next) setMedia(next);
                      })
                      .catch(() => {});
                  }}
                >
                  {mediaSummary.isPlaying ? <PauseIcon size={11} /> : <PlaySmallIcon size={11} />}
                </button>
              )}
            </div>
          );
        case 'timer':
          return (
            <button
              key="timer"
              type="button"
              className={`tb-tray-btn tb-widget-pill tb-timer-pill ${timerSnap.running ? 'running' : ''} ${timerDone ? 'done' : ''}`.trim()}
              aria-label={`Таймер фокуса: ${timerLabel}`}
              onClick={() => toggleStart(undefined, monitor).catch(() => {})}
            >
              <TimerIcon className="ic16 tb-timer-ic" />
              <span className="tb-widget-val tb-timer-val">{timerLabel}</span>
            </button>
          );
        default:
          return null;
      }
    });

  const handleRootContextMenu = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!(e.target as HTMLElement)?.closest?.('input, textarea, select, [contenteditable="true"]')) {
      e.preventDefault();
    }
  };

  return (
    <div
      className={getTaskbarRootClassName(config?.barMode)}
      data-space="personal"
      data-monitor={monitor}
      onContextMenu={handleRootContextMenu}
    >
      <nav
        ref={barRef}
        className={`taskbar has-sheen ${useEdgeCentered ? 'edge-centered' : ''}`}
        data-tb-hidden={tbHidden ? 'true' : 'false'}
        onClick={handleBarClick}
        onContextMenu={handleBarCtx}
        onPointerEnter={handleBarPointerEnter}
        onPointerMove={handleBarPointerMove}
      >
        <span ref={sheenRef} className="tb-sheen" aria-hidden="true" />

        {useEdgeCentered && (
          <div
            className="tb-edge-left"
            aria-hidden={!(widgetsPosition === 'left' && hasAnyWidget)}
          >
            {widgetsPosition === 'left' && hasAnyWidget && (
              <div ref={leftWidgetsGroupRef} className="tb-group tb-left-widgets">
                {renderWidgetPills()}
              </div>
            )}
          </div>
        )}

        {!useEdgeCentered && widgetsPosition === 'left' && hasAnyWidget && (
          <>
            <div ref={leftWidgetsGroupRef} className="tb-group tb-left-widgets">
              {renderWidgetPills()}
            </div>
            <div className="tb-sep tb-widgets-left-sep" aria-hidden="true" />
          </>
        )}

        <div className="tb-group tb-apps has-hover-pill">
          <div ref={appsScrollRef} className="tb-apps-scroll" onMouseLeave={hideHoverPill}>
            <span ref={hoverPillRef} className="tb-hover-pill" aria-hidden="true" />

            <button
              type="button"
              className={`tb-btn tb-start ${startOpen ? 'start-on' : ''}`}
              aria-label="Пуск"
              onMouseEnter={(e) => moveHoverPillTo(e.currentTarget)}
              onClick={() => toggleStart(undefined, monitor).catch(() => {})}
              onContextMenu={handleStartCtx}
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
                isDragOver && dragOverSide === 'before' ? 'drag-over-before' : '',
                isDragOver && dragOverSide === 'after' ? 'drag-over-after' : '',
              ]
                .filter(Boolean)
                .join(' ');

              return (
                <button
                  key={item.id}
                  type="button"
                  className={btnCls}
                  aria-label={item.name}
                  data-bar-id={item.id}
                  data-bar-pinned={item.pinned ? '1' : '0'}
                  data-pin-id={item.pinned ? item.id : undefined}
                  onPointerDown={(e) => handlePinPointerDown(e, item)}
                  onClick={() => handleAppClick(item)}
                  onAuxClick={(e) => handleAppAuxClick(e, item)}
                  onContextMenu={(e) => handleAppCtx(e, item)}
                  onMouseEnter={(e) => handleAppHover(item, e.currentTarget)}
                  onMouseLeave={handleAppLeave}
                >
                  <span className="tb-ic">
                    {item.iconData ? (
                      <img src={item.iconData} alt="" className="native-ic" draggable={false} />
                    ) : (
                      <FallbackAppIcon iconKey={item.iconKey} target={item.target} size={32} />
                    )}
                  </span>
                  <span
                    className={`tb-indicators ${winCount > 1 ? 'has-multi' : ''}`.trim()}
                    aria-hidden="true"
                  >
                    <span className="tb-pill" />
                  </span>
                </button>
              );
            })}
          </div>
        </div>

        <div ref={trayGroupRef} className="tb-group tb-tray">
          {!isEdge && <div className="tb-sep" />}

          {!isSecondary && (
            <>
              {widgetsPosition === 'right' && hasAnyWidget && (
                <>
                  <div ref={leftWidgetsGroupRef} className="tb-group tb-left-widgets">
                    {renderWidgetPills()}
                  </div>
                  <div className="tb-sep tb-widget-sep" aria-hidden="true" />
                </>
              )}

              <button
                type="button"
                className={`tb-tray-btn chevron ${activeFlyout === 'tray' ? 'open' : ''}`}
                aria-label="Фоновые приложения"
                onClick={(e) =>
                  openFlyoutAt('tray', e.currentTarget, TRAY_CARD_WIDTH, TRAY_MIN_CARD_HEIGHT, false)
                }
              >
                <ChevronUpIcon className="ic20" />
              </button>

              <button
                type="button"
                className="tb-tray-btn tb-lang-btn"
                aria-label={`Раскладка клавиатуры: ${layout}`}
                onClick={() => ipc<string>('switch_keyboard_layout').then(setLayout).catch(() => {})}
              >
                {layout}
              </button>

              {!netConnected && (
                <button
                  type="button"
                  className={`tb-tray-btn tb-net-offline-btn ${activeFlyout === 'sound' || activeFlyout === 'quick' ? 'open' : ''}`}
                  aria-label="Нет подключения к сети"
                  onClick={(e) =>
                    openFlyoutAt(
                      'sound',
                      e.currentTarget,
                      SOUND_FLYOUT_WIDTH,
                      SOUND_FLYOUT_INITIAL_HEIGHT,
                      false,
                    )
                  }
                >
                  <span className="tb-net-offline">
                    <OfflineIcon className="ic18" />
                  </span>
                </button>
              )}

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
                <span className="tb-vol-ic">
                  <VolIcon muted={audioMaster?.masterMuted} level={audioMaster?.masterVolume} className="ic20" />
                </span>
              </button>
            </>
          )}

          <button
            type="button"
            className={`tb-tray-btn tb-clock ${activeFlyout === 'calendar' ? 'open' : ''} ${clock.timeOnly ? 'time-only' : ''}`.trim()}
            data-clock-layout={clock.layout}
            aria-label={clock.fullTitle}
            onClick={(e) =>
              openFlyoutAt('calendar', e.currentTarget, CALENDAR_FLYOUT_WIDTH, CALENDAR_FLYOUT_INITIAL_HEIGHT, false)
            }
          >
            {clock.layout === 'single' ? (
              clock.dateStr ? (
                <>
                  <span className="tb-clock-date">{clock.dateStr}</span>
                  <span className="tb-clock-sep" aria-hidden="true">·</span>
                  <span className="tb-clock-time">{clock.timeStr}</span>
                </>
              ) : (
                <span className="tb-clock-time time-only">{clock.timeStr}</span>
              )
            ) : (
              <>
                <span className={`tb-clock-time ${clock.timeOnly ? 'time-only' : ''}`.trim()}>
                  {clock.timeStr}
                </span>
                {clock.dateStr && <span className="tb-clock-date">{clock.dateStr}</span>}
              </>
            )}
          </button>
        </div>
      </nav>
    </div>
  );
};
