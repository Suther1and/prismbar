import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import '../styles/widgets.css';
import {
  buildCalendarMonthGrid,
  computeCalendarSlideDir,
  formatAlertTime,
  formatSelectedDateHeader,
  isSameCalendarDay,
  isViewingTodayMonth,
  isWeekendColumn,
  MONTHS_RU,
  shiftCalendarMonth,
  WEEKDAYS_SHORT_RU,
  type CalendarMonthRef,
} from '../lib/calendarLogic';
import {
  AUDIO_PEAK_POLL_MS,
  BATTERY_LIST_MAX_HEIGHT,
  canOpenFileLocation,
  canRunAsAdmin,
  collectAppContextPids,
  computeCardOriginX,
  computeFlyoutCardHeight,
  computeNextContextFocusIdx,
  computeTrayFlyoutHeight,
  computeWeatherTempBarRange,
  formatCityOptionSubtitle,
  formatIpcError,
  formatMediaTime,
  formatPeripheralSourceLabel,
  formatTrayHeaderLabel,
  formatWeatherWindLabel,
  getFlyoutExitMs,
  INITIAL_FLYOUT_ANIM_STATE,
  interpolateMediaPosition,
  isAppContextKind,
  isBarContextKind,
  OSD_AUTO_CLOSE_MS,
  partitionAudioDevices,
  PREVIEW_CARD_CLOSE_MS,
  PREVIEW_LEAVE_COMMAND,
  QS_MIXER_MAX_HEIGHT,
  reduceFlyoutHideComplete,
  reduceFlyoutHideImmediate,
  reduceFlyoutHideStart,
  reduceFlyoutShow,
  removePreviewWindow,
  resolveTrayHideKey,
  shouldShowAppContextActionSeparator,
  syncPreviewWindowsFromGroups,
  TRAY_CARD_WIDTH,
  TRAY_CTX_OPEN_MIN_HEIGHT,
  type FlyoutAnimState,
} from '../lib/flyoutLogic';
import {
  animateSurfaceIn,
  animateSurfaceOut,
  applyConfigUpdate,
  fetchIconsBatch,
  getAudioPeaks,
  getNetworkSnapshot,
  getPeripheralBatteries,
  getWeather,
  hideBatteryDevice,
  hideTrayApp,
  iconCache,
  ipc,
  isCacheIconKey,
  launchNewWindow,
  minimizeOtherWindows,
  onEvent,
  refreshPeripheralBatteries,
  searchWeatherCities,
  seekMedia,
  setInputDevice,
  setSessionState,
  setTaskbarWidgets,
  snapWindow,
  surfaceCommitAck,
  surfaceExitAck,
  toggleWindowTopmost,
  type AppWindowGroup,
  type AudioMasterState,
  type AudioPeaksSnapshot,
  type AudioState,
  type FlyoutHideEvent,
  type FlyoutKind,
  type FlyoutRequest,
  type JumpListItem,
  type MediaState,
  type NetworkInfo,
  type NetworkSnapshot,
  type OpenWindowInfo,
  type PeripheralBattery,
  type SessionAlert,
  type TelemetryState,
  type TestBarConfig,
  type TrayAppItem,
  type WeatherCityOption,
  type WeatherSnapshot,
} from '../lib/ipc';
import { mergeAudioStateRespectingLocks } from '../lib/startLogic';
import {
  buildPinFromAppContext,
  computePreviewCardWidth,
  formatNetworkRateCompact,
  formatTelemetryGb,
  formatWeatherTempLabel,
  getTelemetryUsageTone,
  PREVIEW_CARD_HEIGHT,
  PREVIEW_MAX_VISIBLE_CARDS,
  type AppContextPayload,
  type BarContextPayload,
  type PreviewFlyoutPayload,
  type VolumeOsdPayload,
} from '../lib/taskbarLogic';
import { useAnimatedPresence } from '../lib/useAnimatedPresence';
import { useReducedMotion } from '../lib/useReducedMotion';
import {
  ArrowDownIcon,
  ArrowUpIcon,
  BellIcon,
  BluetoothIcon,
  ChargingBoltIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  ClockRecentIcon,
  CloseWindowIcon,
  CopyIcon,
  DiskIcon,
  DropletIcon,
  EthernetIcon,
  ExternalWindowIcon,
  EyeOffIcon,
  FallbackAppIcon,
  FolderSvg,
  KillIcon,
  LayoutBarIcon,
  MaximizeIcon,
  MicIcon,
  MinimizeOthersIcon,
  MusicNoteIcon,
  NativeTrayIcon,
  NextTrackIcon,
  OfflineIcon,
  PauseIcon,
  PeripheralDeviceIcon,
  PinIcon,
  PlaySmallIcon,
  PrevTrackIcon,
  RefreshIcon,
  RestoreIcon,
  SearchIcon,
  SettingsGearIcon,
  ShieldIcon,
  SnapLeftIcon,
  SnapRightIcon,
  TaskmgrIcon,
  TopmostIcon,
  UnpinIcon,
  VolIcon,
  WeatherConditionIcon,
  WifiIcon,
  WindIcon,
  XIcon10,
} from './Icons';
import { ObsidianSlider } from './ObsidianSlider';

const MAX_THUMB_CACHE_ENTRIES = 24;

interface ContextActionEntry {
  key: string;
  label: string;
  title?: string;
  icon: React.ReactNode;
  danger?: boolean;
  onActivate: () => void;
}

type ContextBlock =
  | { kind: 'subsec'; key: string; label: string }
  | { kind: 'sep'; key: string }
  | { kind: 'action'; entry: ContextActionEntry; actionIdx: number };

function formatCompactNetworkLabel(net: NetworkInfo | null): string {
  if (!net) return 'Сеть';
  if (!net.connected) return 'Нет сети';
  const raw = (net.kind ?? '').trim().toLowerCase();
  let kindLabel = 'Сеть';
  if (raw === 'ethernet' || raw.includes('ether') || raw === 'lan') {
    kindLabel = 'Ethernet';
  } else if (raw === 'wifi' || raw === 'wi-fi' || raw.includes('wi-fi') || raw.includes('wlan')) {
    kindLabel = 'Wi-Fi';
  } else if (raw && raw !== 'offline' && raw !== 'none' && raw !== 'unknown') {
    const trimmed = net.kind.trim();
    kindLabel = trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
  }
  const speed = (net.speedLabel ?? '').trim();
  return speed ? `${kindLabel} · ${speed}` : kindLabel;
}

function formatBatterySourceLabel(source: string | null | undefined): string {
  const norm = (source ?? '').trim().toLowerCase();
  if (!norm) return 'Беспроводное';
  switch (norm) {
    case 'sony_hid':
      return 'PlayStation HID';
    case 'razer_hid':
      return 'USB HID';
    case 'logitech_hidpp':
      return 'Logitech HID++';
    case 'bluetooth_pnp':
    case 'bluetooth':
    case 'bt':
      return 'Bluetooth';
    case 'xinput':
      return 'Xbox / XInput';
    default:
      return formatPeripheralSourceLabel(source);
  }
}

function measureCardHeight(el: HTMLElement, kind: FlyoutKind): number | null {
  let borderY = 2;
  if (typeof getComputedStyle === 'function') {
    const cs = getComputedStyle(el);
    const parsed = (parseFloat(cs.borderTopWidth) || 0) + (parseFloat(cs.borderBottomWidth) || 0);
    if (Number.isFinite(parsed) && parsed > 0) borderY = parsed;
  }

  if (isAppContextKind(kind) || isBarContextKind(kind)) {
    const inner = el.querySelector<HTMLElement>('.ctx-scroll');
    return computeFlyoutCardHeight({
      cardScrollHeight: el.scrollHeight,
      borderY,
      innerScrollHeight: inner?.scrollHeight,
      innerClientHeight: inner?.clientHeight,
    });
  }

  if (kind === 'sound' || kind === 'quick') {
    const inner = el.querySelector<HTMLElement>('.qs-mixer-scroll');
    return computeFlyoutCardHeight({
      cardScrollHeight: el.scrollHeight,
      borderY,
      innerScrollHeight: inner?.scrollHeight,
      innerClientHeight: inner?.clientHeight,
      innerMaxHeight: QS_MIXER_MAX_HEIGHT,
    });
  }

  if (kind === 'battery') {
    const inner = el.querySelector<HTMLElement>('.bat-list');
    return computeFlyoutCardHeight({
      cardScrollHeight: el.scrollHeight,
      borderY,
      innerScrollHeight: inner?.scrollHeight,
      innerClientHeight: inner?.clientHeight,
      innerMaxHeight: BATTERY_LIST_MAX_HEIGHT,
    });
  }

  if (kind === 'telemetry') {
    const inner = el.querySelector<HTMLElement>('.telem-disks');
    return computeFlyoutCardHeight({
      cardScrollHeight: el.scrollHeight,
      borderY,
      innerScrollHeight: inner?.scrollHeight,
      innerClientHeight: inner?.clientHeight,
      innerMaxHeight: 160,
    });
  }

  return computeFlyoutCardHeight({
    cardScrollHeight: el.scrollHeight,
    borderY,
  });
}

export const FlyoutView: React.FC = () => {
  const reducedMotion = useReducedMotion();

  const [anim, setAnim] = useState<FlyoutAnimState>(INITIAL_FLYOUT_ANIM_STATE);
  const [enterReady, setEnterReady] = useState<boolean>(false);
  const [jumpListReady, setJumpListReady] = useState<boolean>(true);
  const [trayApps, setTrayApps] = useState<TrayAppItem[]>([]);
  const [trayHydrated, setTrayHydrated] = useState<boolean>(false);
  const [trayIconMap, setTrayIconMap] = useState<Record<string, string>>({});
  const [trayHover, setTrayHover] = useState<string>('');
  const [trayCtxItem, setTrayCtxItem] = useState<TrayAppItem | null>(null);
  const [audio, setAudio] = useState<AudioState | null>(null);
  const [peaks, setPeaks] = useState<AudioPeaksSnapshot | null>(null);
  const [sessionIconMap, setSessionIconMap] = useState<Record<string, string>>({});
  const [media, setMedia] = useState<MediaState | null>(null);
  const [mediaNowMs, setMediaNowMs] = useState<number>(() => Date.now());
  const [scrubOverrideSec, setScrubOverrideSec] = useState<number | null>(null);
  const [devMenuOpen, setDevMenuOpen] = useState(false);
  const [segAnimReady, setSegAnimReady] = useState(false);
  const [osdState, setOsdState] = useState<VolumeOsdPayload>({ volume: 0, muted: false });
  const [net, setNet] = useState<NetworkInfo | null>(null);
  const [thumbs, setThumbs] = useState<Record<number, string>>({});
  const [previewGroupKey, setPreviewGroupKey] = useState<string>('');
  const [previewIconUri, setPreviewIconUri] = useState<string | null>(null);
  const [previewWindows, setPreviewWindows] = useState<OpenWindowInfo[]>([]);
  const [closingHwnds, setClosingHwnds] = useState<Record<number, true>>({});
  const [jumpList, setJumpList] = useState<JumpListItem[]>([]);
  const [alerts, setAlerts] = useState<SessionAlert[]>([]);
  const [config, setConfig] = useState<TestBarConfig | null>(null);
  const [weather, setWeather] = useState<WeatherSnapshot | null>(null);
  const [weatherLoading, setWeatherLoading] = useState<boolean>(false);
  const [weatherCityOpen, setWeatherCityOpen] = useState<boolean>(false);
  const [weatherCityQuery, setWeatherCityQuery] = useState<string>('');
  const [weatherCityResults, setWeatherCityResults] = useState<WeatherCityOption[]>([]);
  const [weatherCitySearching, setWeatherCitySearching] = useState<boolean>(false);
  const [weatherCityError, setWeatherCityError] = useState<string | null>(null);
  const [periphBatteries, setPeriphBatteries] = useState<PeripheralBattery[]>([]);
  const [periphHydrated, setPeriphHydrated] = useState<boolean>(false);
  const [periphRefreshing, setPeriphRefreshing] = useState<boolean>(false);
  const [netSnap, setNetSnap] = useState<NetworkSnapshot | null>(null);
  const [netPinging, setNetPinging] = useState<boolean>(false);
  const [netIpCopied, setNetIpCopied] = useState<boolean>(false);
  const [telemSnap, setTelemSnap] = useState<TelemetryState | null>(null);
  const [telemRefreshing, setTelemRefreshing] = useState<boolean>(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [ctxFocusIdx, setCtxFocusIdx] = useState<number>(-1);
  const [calDir, setCalDir] = useState<-1 | 0 | 1>(0);
  const [calView, setCalView] = useState<CalendarMonthRef>(() => {
    const now = new Date();
    return { year: now.getFullYear(), month: now.getMonth() };
  });
  const [selectedDate, setSelectedDate] = useState<Date>(() => new Date());

  const exitTimerRef = useRef<number | null>(null);
  const osdTimerRef = useRef<number | null>(null);
  const netCopyTimerRef = useRef<number | null>(null);
  const segReadyRafRef = useRef<number | null>(null);
  const enterRafRef = useRef<number | null>(null);
  const jumpListTimerRef = useRef<number | null>(null);
  const activeFlyoutAnimsRef = useRef<Animation[]>([]);
  const previewCloseTimersRef = useRef<Map<number, number>>(new Map());
  const thumbCacheRef = useRef<Map<number, string>>(new Map());
  const lastTrayCtxRef = useRef<TrayAppItem | null>(null);

  const renderSeqRef = useRef<number>(0);
  const activeKindRef = useRef<string>('');
  const phaseRef = useRef<FlyoutAnimState['phase']>('closed');
  const currentGenRef = useRef<number>(0);
  const ackedGenRef = useRef<number>(-1);
  const animatedRenderSeqRef = useRef<number>(0);
  const lastResizedHeightRef = useRef<number>(0);
  const previewGroupKeyRef = useRef<string>('');
  const previewMonitorRef = useRef<'primary' | 'secondary' | undefined>(undefined);
  const trayCtxItemRef = useRef<TrayAppItem | null>(null);
  const devMenuOpenRef = useRef<boolean>(false);
  const weatherCityOpenRef = useRef<boolean>(false);
  const weatherCitySearchSeqRef = useRef<number>(0);
  const cardRef = useRef<HTMLDivElement | null>(null);
  const ctxActionsRef = useRef<ContextActionEntry[]>([]);
  const ctxFocusIdxRef = useRef<number>(-1);
  const seekSeqRef = useRef<number>(0);
  const audioDragLocksRef = useRef<Map<string, number>>(new Map());
  const draggingAudioKeysRef = useRef<Set<string>>(new Set());
  const audioReqSeqRef = useRef<Map<string, number>>(new Map());
  const audioThrottleTimersRef = useRef<Map<string, number>>(new Map());
  const audioPendingSendRef = useRef<Map<string, () => void>>(new Map());
  const reducedMotionRef = useRef<boolean>(reducedMotion);
  reducedMotionRef.current = reducedMotion;

  if (trayCtxItem) {
    lastTrayCtxRef.current = trayCtxItem;
  }

  renderSeqRef.current = anim.renderSeq;
  activeKindRef.current = anim.phase === 'closed' ? '' : (anim.req?.kind ?? '');
  phaseRef.current = anim.phase;
  previewGroupKeyRef.current = previewGroupKey;
  trayCtxItemRef.current = trayCtxItem;
  devMenuOpenRef.current = devMenuOpen;
  weatherCityOpenRef.current = weatherCityOpen;
  ctxFocusIdxRef.current = ctxFocusIdx;

  const trayCtxPresence = useAnimatedPresence(Boolean(trayCtxItem), 95);
  const devMenuPresence = useAnimatedPresence(devMenuOpen, 95);

  const clearPreviewCloseTimers = useCallback(() => {
    previewCloseTimersRef.current.forEach((timerId) => window.clearTimeout(timerId));
    previewCloseTimersRef.current.clear();
  }, []);

  const cancelFlyoutAnims = useCallback(() => {
    for (const a of activeFlyoutAnimsRef.current) {
      try {
        a.cancel();
      } catch {
        // Ignore finished animations
      }
    }
    activeFlyoutAnimsRef.current = [];
    if (cardRef.current) {
      cardRef.current.style.pointerEvents = '';
    }
  }, []);

  const clearPendingFlyoutEnter = useCallback(() => {
    if (enterRafRef.current !== null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(enterRafRef.current);
      enterRafRef.current = null;
    }
    if (jumpListTimerRef.current !== null) {
      window.clearTimeout(jumpListTimerRef.current);
      jumpListTimerRef.current = null;
    }
  }, []);

  // Release entrance WAAPI fill:forwards strictly post-commit once [data-enter-ready="true"] is on DOM
  useLayoutEffect(() => {
    if (enterReady && anim.phase === 'open') {
      cancelFlyoutAnims();
    }
  }, [enterReady, anim.phase, cancelFlyoutAnims]);

  const clearOsdTimer = useCallback(() => {
    if (osdTimerRef.current !== null) {
      window.clearTimeout(osdTimerRef.current);
      osdTimerRef.current = null;
    }
  }, []);

  const armOsdAutoClose = useCallback(() => {
    clearOsdTimer();
    osdTimerRef.current = window.setTimeout(() => {
      osdTimerRef.current = null;
      if (activeKindRef.current === 'osd') {
        ipc('close_osd').catch(() => {});
      }
    }, OSD_AUTO_CLOSE_MS);
  }, [clearOsdTimer]);

  const cacheThumb = useCallback((hwnd: number, uri: string) => {
    const map = thumbCacheRef.current;
    if (map.has(hwnd)) {
      map.delete(hwnd);
    }
    map.set(hwnd, uri);
    while (map.size > MAX_THUMB_CACHE_ENTRIES) {
      const oldest = map.keys().next().value;
      if (oldest === undefined) break;
      map.delete(oldest);
    }
  }, []);

  useEffect(() => {
    ipc<TestBarConfig>('get_config')
      .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
      .catch(() => {});

    // Background idle pre-warm for Tray apps & icons so first `^` click renders instantly with zero empty flash
    const trayPrewarmTimer = window.setTimeout(() => {
      ipc<TrayAppItem[]>('get_tray_apps')
        .then((items) => {
          setTrayApps(items);
          setTrayHydrated(true);
        })
        .catch(() => {});
    }, 1600);

    const subs = [
      onEvent<TrayAppItem[]>('tray-updated', (items) => {
        if (!Array.isArray(items)) return;
        setTrayApps(items);
        setTrayHydrated(true);
        if (activeKindRef.current === 'tray' && phaseRef.current === 'open') {
          const nextH = computeTrayFlyoutHeight(items.length, Boolean(trayCtxItemRef.current));
          if (Math.abs(nextH - lastResizedHeightRef.current) > 2) {
            lastResizedHeightRef.current = nextH;
            ipc('resize_flyout', { width: TRAY_CARD_WIDTH, height: nextH }).catch(() => {});
          }
        }
      }),
      onEvent<FlyoutRequest>('flyout-show', (r) => {
        if (exitTimerRef.current !== null) {
          window.clearTimeout(exitTimerRef.current);
          exitTimerRef.current = null;
        }
        clearPendingFlyoutEnter();

        const prevKind = activeKindRef.current;
        const isLive = phaseRef.current === 'open';
        const isInPlaceOsd = r.kind === 'osd' && prevKind === 'osd' && isLive;
        const isInPlacePreview = r.kind === 'preview' && prevKind === 'preview' && isLive;

        if (!isInPlacePreview) {
          clearPreviewCloseTimers();
        }
        if (!isInPlaceOsd && !isInPlacePreview) {
          cancelFlyoutAnims();
          setEnterReady(false);
        } else {
          setEnterReady(true);
        }

        const nextSeq = isInPlaceOsd || isInPlacePreview ? renderSeqRef.current : renderSeqRef.current + 1;
        renderSeqRef.current = nextSeq;
        activeKindRef.current = r.kind;
        currentGenRef.current = r.gen ?? 0;
        if (!isInPlaceOsd) {
          ackedGenRef.current = -1;
        }
        lastResizedHeightRef.current = r.height;

        setAnim((prev) => reduceFlyoutShow(prev, r));

        if (r.kind !== 'osd') {
          clearOsdTimer();
        }
        setTrayCtxItem((prev) => (prev === null ? prev : null));
        setTrayHover((prev) => (prev === '' ? prev : ''));
        setDevMenuOpen((prev) => (prev ? false : prev));
        setActionError((prev) => (prev === null ? prev : null));
        setCtxFocusIdx((prev) => (prev === -1 ? prev : -1));
        setJumpList((prev) => (prev.length === 0 ? prev : []));
        if (!isAppContextKind(r.kind)) {
          setJumpListReady((prev) => (prev ? prev : true));
        }
        seekSeqRef.current += 1;
        setScrubOverrideSec((prev) => (prev === null ? prev : null));
        setClosingHwnds((prev) => (Object.keys(prev).length === 0 ? prev : {}));

        if (r.kind === 'sound' || r.kind === 'quick') {
          if (segReadyRafRef.current !== null && typeof cancelAnimationFrame === 'function') {
            cancelAnimationFrame(segReadyRafRef.current);
            segReadyRafRef.current = null;
          }
          setSegAnimReady((prev) => (prev ? false : prev));
        }

        if (r.kind !== 'preview') {
          previewGroupKeyRef.current = '';
          previewMonitorRef.current = undefined;
          setPreviewGroupKey((prev) => (prev === '' ? prev : ''));
          setPreviewIconUri((prev) => (prev === null ? prev : null));
          setPreviewWindows((prev) => (prev.length === 0 ? prev : []));
        }

        if (r.kind === 'tray') {
          const payload = (r.payload ?? {}) as { trayApps?: TrayAppItem[] };
          if (Array.isArray(payload.trayApps)) {
            // Instant zero-flash hydration directly from Wave 2B Rust cached tray snapshot
            setTrayApps(payload.trayApps);
            setTrayHydrated(true);
            const nextH = computeTrayFlyoutHeight(payload.trayApps.length);
            if (Math.abs(nextH - lastResizedHeightRef.current) > 2) {
              lastResizedHeightRef.current = nextH;
              ipc('resize_flyout', { width: TRAY_CARD_WIDTH, height: nextH }).catch(() => {});
            }
          } else {
            ipc<TrayAppItem[]>('get_tray_apps')
              .then((items) => {
                setTrayApps(items);
                setTrayHydrated(true);
                if (renderSeqRef.current !== nextSeq || activeKindRef.current !== 'tray') return;
                const nextH = computeTrayFlyoutHeight(items.length);
                lastResizedHeightRef.current = nextH;
                ipc('resize_flyout', {
                  width: TRAY_CARD_WIDTH,
                  height: nextH,
                }).catch(() => {});
              })
              .catch(() => {});
          }
        } else if (r.kind === 'osd') {
          const payload = (r.payload ?? {}) as Partial<VolumeOsdPayload>;
          setOsdState({
            volume:
              typeof payload.volume === 'number' && Number.isFinite(payload.volume)
                ? Math.max(0, Math.min(100, Math.round(payload.volume)))
                : 0,
            muted: Boolean(payload.muted),
          });
          armOsdAutoClose();
        } else if (r.kind === 'calendar') {
          const now = new Date();
          setCalDir(0);
          setCalView({ year: now.getFullYear(), month: now.getMonth() });
          setSelectedDate(now);
        } else if (r.kind === 'weather') {
          setWeatherCityOpen(false);
          setWeatherCityQuery('');
          setWeatherCityResults([]);
          setWeatherCityError(null);
          setWeatherLoading(true);
          ipc<TestBarConfig>('get_config')
            .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
            .catch(() => {});
          getWeather({ force: false })
            .then((snap) => {
              if (renderSeqRef.current === nextSeq || activeKindRef.current === 'weather') {
                setWeather(snap);
              }
            })
            .catch(() => {})
            .finally(() => {
              if (renderSeqRef.current === nextSeq || activeKindRef.current === 'weather') {
                setWeatherLoading(false);
              }
            });
        } else if (r.kind === 'battery') {
          ipc<TestBarConfig>('get_config')
            .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
            .catch(() => {});
          getPeripheralBatteries()
            .then((items) => {
              if (Array.isArray(items)) {
                setPeriphBatteries(items);
              }
              setPeriphHydrated(true);
            })
            .catch(() => {
              setPeriphHydrated(true);
            });
        } else if (r.kind === 'network') {
          setNetIpCopied(false);
          setNetPinging(true);
          getNetworkSnapshot(true)
            .then((snap) => {
              if (renderSeqRef.current === nextSeq || activeKindRef.current === 'network') {
                setNetSnap(snap);
              }
            })
            .catch(() => {})
            .finally(() => {
              if (renderSeqRef.current === nextSeq || activeKindRef.current === 'network') {
                setNetPinging(false);
              }
            });
        } else if (r.kind === 'telemetry') {
          setTelemRefreshing(true);
          ipc<TelemetryState>('get_telemetry')
            .then((snap) => {
              if (renderSeqRef.current === nextSeq || activeKindRef.current === 'telemetry') {
                setTelemSnap(snap);
              }
            })
            .catch(() => {})
            .finally(() => {
              if (renderSeqRef.current === nextSeq || activeKindRef.current === 'telemetry') {
                setTelemRefreshing(false);
              }
            });
        } else if (r.kind === 'preview') {
          const payload = (r.payload ?? {}) as PreviewFlyoutPayload;
          const wins = payload.group?.windows ?? [];
          const gKey = payload.group?.key ?? '';
          const iconKey = payload.iconKey ?? payload.group?.iconKey ?? null;
          const resolvedIcon =
            payload.iconData ||
            payload.group?.iconData ||
            (iconKey ? iconCache.get(iconKey) ?? null : null);

          previewGroupKeyRef.current = gKey;
          previewMonitorRef.current = r.monitor;
          setPreviewGroupKey(gKey);
          setPreviewWindows(wins);
          setPreviewIconUri(resolvedIcon);

          if (!resolvedIcon && isCacheIconKey(iconKey)) {
            fetchIconsBatch([iconKey])
              .then((batch) => {
                if (previewGroupKeyRef.current === gKey && batch[iconKey]) {
                  setPreviewIconUri(batch[iconKey]);
                }
              })
              .catch(() => {});
          }

          // Seed immediately from LRU thumb cache to avoid blank placeholder flashes on scrub
          const seeded: Record<number, string> = {};
          for (const w of wins.slice(0, PREVIEW_MAX_VISIBLE_CARDS)) {
            const cached = thumbCacheRef.current.get(w.hwnd);
            if (cached) seeded[w.hwnd] = cached;
          }
          setThumbs(seeded);

          wins.slice(0, PREVIEW_MAX_VISIBLE_CARDS).forEach((w) => {
            ipc<string | null>('get_window_thumbnail', { hwnd: w.hwnd })
              .then((img) => {
                if (img) {
                  cacheThumb(w.hwnd, img);
                  if (previewGroupKeyRef.current === gKey && activeKindRef.current === 'preview') {
                    setThumbs((prev) => (prev[w.hwnd] === img ? prev : { ...prev, [w.hwnd]: img }));
                  }
                }
              })
              .catch(() => {});
          });
        } else if (isAppContextKind(r.kind)) {
          const payload = r.payload as AppContextPayload & { jumpList?: JumpListItem[] };
          if (Array.isArray(payload.jumpList)) {
            setJumpList(payload.jumpList);
            setJumpListReady(true);
          } else {
            // Wait up to 45ms for jump_list before measuring & launching the single-step spring
            setJumpListReady(false);
            let settled = false;
            const settleJumpList = () => {
              if (settled) return;
              settled = true;
              if (jumpListTimerRef.current !== null) {
                window.clearTimeout(jumpListTimerRef.current);
                jumpListTimerRef.current = null;
              }
              if (renderSeqRef.current === nextSeq) {
                setJumpListReady(true);
              }
            };
            jumpListTimerRef.current = window.setTimeout(settleJumpList, 45);
            ipc<JumpListItem[]>('get_jump_list', {
              exePath: payload.target ?? '',
              aumid: payload.aumid ?? null,
            })
              .then((items) => {
                if (renderSeqRef.current === nextSeq) {
                  setJumpList(items);
                }
                settleJumpList();
              })
              .catch(() => {
                settleJumpList();
              });
          }
        }
      }),
      onEvent<FlyoutHideEvent | void>('flyout-hide', (ev) => {
        clearOsdTimer();
        clearPreviewCloseTimers();
        clearPendingFlyoutEnter();
        if (exitTimerRef.current !== null) {
          window.clearTimeout(exitTimerRef.current);
          exitTimerRef.current = null;
        }
        const hideGen =
          ev && typeof ev === 'object' && typeof ev.gen === 'number'
            ? ev.gen
            : currentGenRef.current;
        const hideSeq = renderSeqRef.current;
        const exitMs = getFlyoutExitMs(activeKindRef.current as FlyoutKind);

        const el = cardRef.current;
        let liveOpacity = 1;
        let liveTransform = 'translate3d(0, 0, 0) scale(1)';
        if (el && typeof window !== 'undefined') {
          const cs = window.getComputedStyle(el);
          const parsedOp = Number.parseFloat(cs.opacity);
          if (Number.isFinite(parsedOp)) liveOpacity = parsedOp;
          if (cs.transform && cs.transform !== 'none') liveTransform = cs.transform;
        }

        cancelFlyoutAnims();
        previewGroupKeyRef.current = '';
        previewMonitorRef.current = undefined;
        setPreviewGroupKey('');
        setAnim((prev) => reduceFlyoutHideStart(prev));

        animateSurfaceOut('flyout', hideGen, {
          durationMs: reducedMotionRef.current ? 0 : exitMs,
          offsetY: 6,
          toScale: 0.975,
        }).catch(() => {});

        if (!reducedMotionRef.current && el && typeof el.animate === 'function') {
          const exitAnim = el.animate(
            [
              { opacity: liveOpacity, transform: liveTransform },
              { opacity: 0, transform: 'translate3d(0, 6px, 0) scale(0.975)' },
            ],
            {
              duration: exitMs,
              easing: 'cubic-bezier(0.4, 0, 1, 1)',
              fill: 'forwards',
            },
          );
          activeFlyoutAnimsRef.current = [exitAnim];
        }

        exitTimerRef.current = window.setTimeout(() => {
          exitTimerRef.current = null;
          cancelFlyoutAnims();
          setEnterReady(false);
          setJumpListReady(true);
          setAnim((prev) => reduceFlyoutHideComplete(prev, hideSeq));
          setTrayCtxItem(null);
          setDevMenuOpen(false);
          setWeatherCityOpen(false);
          setActionError(null);
          setClosingHwnds({});
          setCtxFocusIdx(-1);
          surfaceExitAck('flyout', hideGen).catch(() => {});
        }, exitMs);
      }),
      onEvent<FlyoutHideEvent | void>('flyout-hide-immediate', () => {
        clearOsdTimer();
        clearPreviewCloseTimers();
        clearPendingFlyoutEnter();
        cancelFlyoutAnims();
        if (exitTimerRef.current !== null) {
          window.clearTimeout(exitTimerRef.current);
          exitTimerRef.current = null;
        }
        previewGroupKeyRef.current = '';
        previewMonitorRef.current = undefined;
        setPreviewGroupKey('');
        setEnterReady(false);
        setJumpListReady(true);
        setTrayCtxItem(null);
        setDevMenuOpen(false);
        setWeatherCityOpen(false);
        setActionError(null);
        setClosingHwnds({});
        setCtxFocusIdx(-1);
        setAnim((prev) => reduceFlyoutHideImmediate(prev));
      }),
      onEvent<AudioState>('audio-mixer-updated', (st) => {
        setAudio((prev) =>
          mergeAudioStateRespectingLocks(prev, st, audioDragLocksRef.current, Date.now()),
        );
      }),
      onEvent<AudioMasterState>('audio-master-updated', (m) => {
        setAudio((prev) => {
          if (!prev) return prev;
          const masterLock = audioDragLocksRef.current.get('__master__');
          const keepMasterVol = masterLock !== undefined && masterLock > Date.now();
          return {
            ...prev,
            available: m.available,
            masterVolume: keepMasterVol ? prev.masterVolume : m.masterVolume,
            masterMuted: m.masterMuted,
            micMuted: m.micMuted,
          };
        });
        if (activeKindRef.current === 'osd') {
          setOsdState({ volume: m.masterVolume, muted: m.masterMuted });
        }
      }),
      onEvent<MediaState>('media-updated', (m) => {
        setMedia(m);
        setMediaNowMs(Date.now());
      }),
      onEvent<NetworkInfo>('network-updated', setNet),
      onEvent<SessionAlert[]>('session-alerts-updated', setAlerts),
      onEvent<TestBarConfig>('config-updated', (next) =>
        setConfig((prev) => applyConfigUpdate(prev, next)),
      ),
      onEvent<WeatherSnapshot>('weather-updated', (snap) => {
        if (snap) {
          setWeather(snap);
          setWeatherLoading(false);
        }
      }),
      onEvent<PeripheralBattery[]>('peripherals-updated', (items) => {
        if (!Array.isArray(items)) return;
        setPeriphBatteries(items);
        setPeriphHydrated(true);
        setPeriphRefreshing(false);
      }),
      onEvent<TelemetryState>('telemetry-updated', (snap) => {
        if (snap) {
          setTelemSnap(snap);
          setTelemRefreshing(false);
        }
      }),
      onEvent<AppWindowGroup[]>('windows-updated', (groups) => {
        if (activeKindRef.current !== 'preview') return;
        const key = previewGroupKeyRef.current;
        if (!key) return;
        const synced = syncPreviewWindowsFromGroups(key, groups, previewMonitorRef.current);
        if (synced === null) return;
        setPreviewWindows(synced);
        if (synced.length === 0) {
          ipc('close_flyout').catch(() => {});
        }
      }),
    ];

    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        if (trayCtxItemRef.current) {
          e.preventDefault();
          setActionError(null);
          setTrayCtxItem(null);
        } else if (devMenuOpenRef.current) {
          e.preventDefault();
          setDevMenuOpen(false);
        } else if (weatherCityOpenRef.current) {
          e.preventDefault();
          setWeatherCityOpen(false);
        } else {
          ipc('close_flyout').catch(() => {});
        }
        return;
      }

      const kind = activeKindRef.current as FlyoutKind;
      if (isAppContextKind(kind) || isBarContextKind(kind)) {
        const actions = ctxActionsRef.current;
        if (actions.length === 0) return;
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setCtxFocusIdx((cur) => computeNextContextFocusIdx(cur, 1, actions.length));
        } else if (e.key === 'ArrowUp') {
          e.preventDefault();
          setCtxFocusIdx((cur) => computeNextContextFocusIdx(cur, -1, actions.length));
        } else if (e.key === 'Enter') {
          const idx = ctxFocusIdxRef.current;
          if (idx >= 0 && idx < actions.length) {
            e.preventDefault();
            actions[idx].onActivate();
          }
        }
      }
    };

    window.addEventListener('keydown', onKey);
    return () => {
      window.clearTimeout(trayPrewarmTimer);
      window.removeEventListener('keydown', onKey);
      clearOsdTimer();
      if (exitTimerRef.current !== null) window.clearTimeout(exitTimerRef.current);
      if (netCopyTimerRef.current !== null) window.clearTimeout(netCopyTimerRef.current);
      if (segReadyRafRef.current !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(segReadyRafRef.current);
      }
      audioThrottleTimersRef.current.forEach((id) => window.clearTimeout(id));
      audioThrottleTimersRef.current.clear();
      audioPendingSendRef.current.clear();
      clearPendingFlyoutEnter();
      cancelFlyoutAnims();
      clearPreviewCloseTimers();
      subs.forEach((p) => p.then((u) => u()));
    };
  }, [
    armOsdAutoClose,
    cacheThumb,
    cancelFlyoutAnims,
    clearOsdTimer,
    clearPendingFlyoutEnter,
    clearPreviewCloseTimers,
  ]);

  // Single-step commit-then-spring entrance: measures final height, waits for `surfaceCommitAck`,
  // then launches ONE synchronized WAAPI + DComp spring (zero 2-step resize jump!).
  useLayoutEffect(() => {
    const req = anim.req;
    if (!req || anim.phase !== 'open' || !jumpListReady) return;
    const gen = req.gen ?? 0;
    if (ackedGenRef.current === gen) return;
    ackedGenRef.current = gen;

    const shouldMeasure =
      isAppContextKind(req.kind) ||
      isBarContextKind(req.kind) ||
      req.kind === 'sound' ||
      req.kind === 'quick' ||
      req.kind === 'calendar' ||
      req.kind === 'weather' ||
      req.kind === 'battery' ||
      req.kind === 'network' ||
      req.kind === 'telemetry';

    let measuredCardHeight: number | undefined;
    if (shouldMeasure && cardRef.current) {
      const h = measureCardHeight(cardRef.current, req.kind);
      if (h !== null) {
        measuredCardHeight = h;
        lastResizedHeightRef.current = h;
      }
    }

    const shouldAnimateIn =
      !reducedMotionRef.current && animatedRenderSeqRef.current !== anim.renderSeq;
    if (shouldAnimateIn) {
      animatedRenderSeqRef.current = anim.renderSeq;
    }

    void surfaceCommitAck('flyout', gen, measuredCardHeight)
      .catch(() => {})
      .finally(() => {
        if (currentGenRef.current !== gen || phaseRef.current !== 'open') {
          return;
        }
        if (!shouldAnimateIn) {
          setEnterReady(true);
          return;
        }

        const runSpring = () => {
          enterRafRef.current = null;
          if (currentGenRef.current !== gen || phaseRef.current !== 'open') return;
          const el = cardRef.current;

          animateSurfaceIn('flyout', gen, {
            durationMs: 185,
            offsetY: 10,
            fromScale: 0.97,
          }).catch(() => {});

          if (!el || typeof el.animate !== 'function') {
            setEnterReady(true);
            return;
          }

          cancelFlyoutAnims();
          el.style.pointerEvents = 'auto';

          const transformAnim = el.animate(
            [
              { transform: 'translate3d(0, 10px, 0) scale(0.97)' },
              { transform: 'translate3d(0, 0, 0) scale(1)' },
            ],
            {
              duration: 185,
              easing: 'cubic-bezier(0.16, 1, 0.3, 1)',
              fill: 'forwards',
            },
          );
          const opacityAnim = el.animate(
            [{ opacity: 0 }, { opacity: 1 }],
            {
              duration: 140,
              easing: 'ease-out',
              fill: 'forwards',
            },
          );
          activeFlyoutAnimsRef.current = [transformAnim, opacityAnim];

          transformAnim.finished
            .then(() => {
              if (currentGenRef.current === gen && phaseRef.current === 'open') {
                setEnterReady(true);
              }
            })
            .catch(() => {
              // Cancelled by rapid reopen or hide
            });
        };

        if (typeof requestAnimationFrame === 'function') {
          enterRafRef.current = requestAnimationFrame(runSpring);
        } else {
          runSpring();
        }
      });
  }, [anim.req, anim.phase, anim.renderSeq, anim.scrubSeq, jumpListReady, cancelFlyoutAnims]);

  // Enable smooth slider/segmented-pill transitions & live WASAPI peaks STRICTLY AFTER the 185ms
  // entrance spring settles (zero mid-spring IPC/re-render stalls & 0% idle cost!).
  const isSoundVisiblyOpen =
    enterReady &&
    anim.phase === 'open' &&
    (anim.req?.kind === 'sound' || anim.req?.kind === 'quick');

  useEffect(() => {
    if (!isSoundVisiblyOpen) {
      setSegAnimReady((prev) => (prev ? false : prev));
      return;
    }
    setSegAnimReady(true);
  }, [isSoundVisiblyOpen]);

  useEffect(() => {
    if (!isSoundVisiblyOpen) {
      setPeaks(null);
      return;
    }
    let cancelled = false;
    let inflight = false;

    const poll = () => {
      if (cancelled || inflight) return;
      inflight = true;
      getAudioPeaks()
        .then((snap) => {
          if (!cancelled) setPeaks(snap);
        })
        .catch(() => {})
        .finally(() => {
          inflight = false;
        });
    };

    poll();
    const timer = window.setInterval(poll, AUDIO_PEAK_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [isSoundVisiblyOpen]);

  // Smooth 1s SMTC timeline interpolation ONLY while Sound flyout is visibly open and media is playing
  useEffect(() => {
    if (!isSoundVisiblyOpen || !media?.hasSession || !media.isPlaying) return;
    setMediaNowMs(Date.now());
    const id = window.setInterval(() => setMediaNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [isSoundVisiblyOpen, media?.hasSession, media?.isPlaying]);

  // Live 1.5s RX/TX network polling ONLY while Network flyout is open
  useEffect(() => {
    if (anim.phase !== 'open' || anim.req?.kind !== 'network') return;
    let cancelled = false;
    let inflight = false;
    const poll = () => {
      if (cancelled || inflight) return;
      inflight = true;
      getNetworkSnapshot(false)
        .then((snap) => {
          if (!cancelled && snap) setNetSnap(snap);
        })
        .catch(() => {})
        .finally(() => {
          inflight = false;
        });
    };
    const timer = window.setInterval(poll, 1500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [anim.phase, anim.req?.kind]);

  // Live 2s PC telemetry polling ONLY while Telemetry flyout is open
  useEffect(() => {
    if (anim.phase !== 'open' || anim.req?.kind !== 'telemetry') return;
    let cancelled = false;
    let inflight = false;
    const poll = () => {
      if (cancelled || inflight) return;
      inflight = true;
      ipc<TelemetryState>('get_telemetry')
        .then((snap) => {
          if (!cancelled && snap) setTelemSnap(snap);
        })
        .catch(() => {})
        .finally(() => {
          inflight = false;
        });
    };
    const timer = window.setInterval(poll, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [anim.phase, anim.req?.kind]);

  // Hydrate per-app audio session native icons via bounded Rust IconCache
  useEffect(() => {
    const sessions = audio?.sessions ?? [];
    if (sessions.length === 0) return;
    const missing = new Set<string>();
    const immediate: Record<string, string> = {};

    for (const s of sessions) {
      const k = s.iconKey;
      if (!isCacheIconKey(k) || sessionIconMap[k]) continue;
      const cached = iconCache.get(k);
      if (cached) {
        immediate[k] = cached;
      } else {
        missing.add(k);
      }
    }

    if (Object.keys(immediate).length > 0) {
      setSessionIconMap((prev) => ({ ...prev, ...immediate }));
    }
    if (missing.size === 0) return;

    let cancelled = false;
    fetchIconsBatch(Array.from(missing))
      .then((fetched) => {
        if (cancelled || Object.keys(fetched).length === 0) return;
        setSessionIconMap((prev) => ({ ...prev, ...fetched }));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [audio?.sessions]);

  // Lazy batch-fetch missing tray icon keys from Rust IconCache
  useEffect(() => {
    if (trayApps.length === 0) return;
    const missing = new Set<string>();
    const immediate: Record<string, string> = {};

    for (const item of trayApps) {
      const k = item.iconKey;
      if (!isCacheIconKey(k) || trayIconMap[k]) continue;
      const cached = iconCache.get(k);
      if (cached) {
        immediate[k] = cached;
      } else {
        missing.add(k);
      }
    }

    if (Object.keys(immediate).length > 0) {
      setTrayIconMap((prev) => ({ ...prev, ...immediate }));
    }
    if (missing.size === 0) return;

    let cancelled = false;
    fetchIconsBatch(Array.from(missing))
      .then((fetched) => {
        if (cancelled || Object.keys(fetched).length === 0) return;
        setTrayIconMap((prev) => ({ ...prev, ...fetched }));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [trayApps]);

  const req = anim.req;
  const isClosing = anim.phase === 'closing';

  // Debounced city search when Weather Flyout inline city picker is open
  useEffect(() => {
    if (anim.phase !== 'open' || anim.req?.kind !== 'weather' || !weatherCityOpen) return;
    const trimmed = weatherCityQuery.trim();
    if (trimmed.length < 2) {
      weatherCitySearchSeqRef.current += 1;
      setWeatherCityResults([]);
      setWeatherCitySearching(false);
      setWeatherCityError(null);
      return;
    }
    const seq = ++weatherCitySearchSeqRef.current;
    setWeatherCitySearching(true);
    setWeatherCityError(null);
    const timer = window.setTimeout(() => {
      searchWeatherCities(trimmed)
        .then((res) => {
          if (weatherCitySearchSeqRef.current !== seq) return;
          setWeatherCityResults(Array.isArray(res) ? res : []);
        })
        .catch((err) => {
          if (weatherCitySearchSeqRef.current !== seq) return;
          setWeatherCityResults([]);
          setWeatherCityError(formatIpcError(err));
        })
        .finally(() => {
          if (weatherCitySearchSeqRef.current === seq) {
            setWeatherCitySearching(false);
          }
        });
    }, 260);
    return () => window.clearTimeout(timer);
  }, [anim.phase, anim.req?.kind, weatherCityOpen, weatherCityQuery]);

  const hiddenBatterySet = useMemo(() => {
    const ids = config?.hiddenBatteryIds ?? [];
    return new Set(ids.map((id) => id.trim().toLowerCase()));
  }, [config?.hiddenBatteryIds]);

  const visibleBatteries = useMemo(
    () => periphBatteries.filter((dev) => !hiddenBatterySet.has(dev.id.trim().toLowerCase())),
    [periphBatteries, hiddenBatterySet],
  );

  // Auto-fit flyout height for dynamic post-open content (Jump List, audio sessions, media strip, alerts, weather, battery)
  useEffect(() => {
    if (!req || isClosing || anim.phase === 'closed' || !jumpListReady) return;
    const needsAutoHeight =
      isAppContextKind(req.kind) ||
      isBarContextKind(req.kind) ||
      req.kind === 'sound' ||
      req.kind === 'quick' ||
      req.kind === 'calendar' ||
      req.kind === 'weather' ||
      req.kind === 'battery' ||
      req.kind === 'network' ||
      req.kind === 'telemetry';
    if (!needsAutoHeight) return;
    const el = cardRef.current;
    if (!el) return;

    const syncHeight = () => {
      if (ackedGenRef.current !== currentGenRef.current) return;
      const h = measureCardHeight(el, req.kind);
      if (h !== null && Math.abs(h - lastResizedHeightRef.current) > 2) {
        lastResizedHeightRef.current = h;
        ipc('resize_flyout', { width: null, height: h }).catch(() => {});
      }
    };
    syncHeight();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(syncHeight);
    ro.observe(el);
    const innerScroll = el.querySelector<HTMLElement>(
      '.ctx-scroll, .qs-mixer-scroll, .bat-list, .telem-disks',
    );
    if (innerScroll) ro.observe(innerScroll);
    return () => ro.disconnect();
  }, [
    req?.kind,
    anim.renderSeq,
    anim.phase,
    isClosing,
    jumpListReady,
    jumpList.length,
    actionError,
    audio?.sessions.length,
    audio?.devices.length,
    audio?.inputDevices?.length,
    devMenuOpen,
    devMenuPresence.mounted,
    media?.hasSession,
    media?.canSeek,
    media?.durationSec,
    alerts.length,
    weather?.available,
    weather?.hourly.length,
    weather?.daily.length,
    weather?.stale,
    weatherCityOpen,
    weatherCityResults.length,
    weatherCitySearching,
    weatherCityError,
    visibleBatteries.length,
    periphHydrated,
    netSnap?.online,
    netSnap?.localIp,
    telemSnap?.gpuAvailable,
    telemSnap?.disks?.length,
  ]);

  // Dynamic compact tray height (128px min normally, smoothly expands to 184px+ ONLY while tray right-click menu is open)
  useEffect(() => {
    if (!req || req.kind !== 'tray' || anim.phase !== 'open') return;
    const ctxOpen = Boolean(trayCtxItem);
    let targetH = computeTrayFlyoutHeight(trayApps.length, ctxOpen);
    if (ctxOpen && cardRef.current) {
      const sheet = cardRef.current.querySelector<HTMLElement>('.tray-ctx-sheet');
      if (sheet && sheet.scrollHeight > 0) {
        targetH = Math.max(targetH, Math.ceil(sheet.scrollHeight) + 18);
      }
    }
    if (Math.abs(targetH - lastResizedHeightRef.current) > 2) {
      lastResizedHeightRef.current = targetH;
      ipc('resize_flyout', { width: TRAY_CARD_WIDTH, height: targetH }).catch(() => {});
    }
  }, [req?.kind, anim.phase, anim.renderSeq, trayApps.length, trayCtxItem, actionError]);

  const isCalendarOpen = anim.phase !== 'closed' && anim.req?.kind === 'calendar';
  const today = useMemo(() => new Date(), [anim.renderSeq]);
  const calCells = useMemo(
    () =>
      isCalendarOpen
        ? buildCalendarMonthGrid(calView.year, calView.month, today, selectedDate)
        : [],
    [isCalendarOpen, calView.year, calView.month, today, selectedDate],
  );

  const sessionPeakMap = useMemo(() => {
    const m = new Map<string, number>();
    if (peaks) {
      for (const [key, val] of peaks.sessionPeaks) {
        m.set(key, val);
      }
    }
    return m;
  }, [peaks]);

  // Clear context keyboard action list unless rendering a context menu
  ctxActionsRef.current = [];

  if (!req || anim.phase === 'closed') return null;

  const dataPhase = anim.phase;
  const originX = computeCardOriginX(req.anchorOriginX, req.width);
  const cardOriginStyle: React.CSSProperties | undefined =
    originX !== null
      ? ({ ['--flyout-origin-x' as string]: `${originX}px` } as React.CSSProperties)
      : undefined;

  const close = () => ipc('close_flyout').catch(() => {});

  const handleRootContextMenu = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!(e.target as HTMLElement)?.closest?.('input, textarea, select, [contenteditable="true"]')) {
      e.preventDefault();
    }
  };

  const runActionAndClose = (promise: Promise<unknown>) => {
    setActionError(null);
    promise
      .then(() => close())
      .catch((err) => setActionError(formatIpcError(err)));
  };

  // 1. COMPACT 6-COLUMN TRAY OVERFLOW (^) — ZERO JUMPING / HIDE FROM LIST
  if (req.kind === 'tray') {
    const headerLabel = formatTrayHeaderLabel(trayHover, trayApps.length);
    const activeCtxTray = trayCtxItem ?? lastTrayCtxRef.current;

    const trayCardStyle: React.CSSProperties | undefined = trayCtxItem
      ? { ...cardOriginStyle, minHeight: TRAY_CTX_OPEN_MIN_HEIGHT }
      : cardOriginStyle;

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card tray-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={trayCardStyle}
        >
          <div className="tray-head">
            <span className={`tray-tip-label ${trayHover ? 'hovering' : ''}`} title={headerLabel}>
              {headerLabel}
            </span>
            <button
              type="button"
              className="tray-native-btn"
              title="Системный трей Windows (Win+B)"
              onClick={() => {
                ipc('open_native_windows_tray').catch(() => {});
              }}
            >
              <NativeTrayIcon className="ic14" />
            </button>
          </div>

          <div className="tray-grid">
            {trayApps.length === 0 ? (
              trayHydrated ? (
                <div className="tray-empty">Нет фоновых приложений</div>
              ) : null
            ) : (
              trayApps.map((t, idx) => {
                const iconUri =
                  (t.iconKey ? trayIconMap[t.iconKey] || iconCache.get(t.iconKey) : undefined) ||
                  t.iconData ||
                  null;
                return (
                  <button
                    key={t.id || `tray-${t.pid ?? idx}-${t.name}-${idx}`}
                    type="button"
                    className="tray-cell"
                    onMouseEnter={() => setTrayHover(t.name)}
                    onMouseLeave={() => setTrayHover('')}
                    onClick={() => {
                      ipc('activate_tray_app', {
                        hwnd: t.hwnd ?? null,
                        exePath: t.exePath,
                      }).catch((err) => setActionError(formatIpcError(err)));
                    }}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      setActionError(null);
                      setTrayCtxItem(t);
                    }}
                  >
                    {iconUri ? (
                      <img src={iconUri} alt="" className="native-ic-sm" draggable={false} />
                    ) : (
                      <FallbackAppIcon iconKey={t.name} size={18} />
                    )}
                  </button>
                );
              })
            )}
          </div>

          {trayCtxPresence.mounted && activeCtxTray && (
            <div
              className="tray-ctx-overlay"
              data-phase={trayCtxPresence.phase}
              onClick={() => {
                setActionError(null);
                setTrayCtxItem(null);
              }}
            >
              <div className="tray-ctx-sheet" onClick={(e) => e.stopPropagation()}>
                <div className="tray-ctx-head">
                  <span className="ctx-sec" style={{ padding: 0 }}>{activeCtxTray.name}</span>
                  <button
                    type="button"
                    className="tray-ctx-close"
                    title="Закрыть меню"
                    onClick={() => {
                      setActionError(null);
                      setTrayCtxItem(null);
                    }}
                  >
                    <XIcon10 />
                  </button>
                </div>

                <button
                  type="button"
                  className="ctx-item"
                  onClick={() => {
                    setActionError(null);
                    ipc('activate_tray_app', {
                      hwnd: activeCtxTray.hwnd ?? null,
                      exePath: activeCtxTray.exePath,
                    }).catch((err) => setActionError(formatIpcError(err)));
                  }}
                >
                  <ExternalWindowIcon />
                  <span className="ctx-item-label">Открыть</span>
                </button>

                {canOpenFileLocation(activeCtxTray.exePath) && (
                  <button
                    type="button"
                    className="ctx-item"
                    onClick={() =>
                      runActionAndClose(ipc('open_location', { path: activeCtxTray.exePath }))
                    }
                  >
                    <FolderSvg className="ic14" />
                    <span className="ctx-item-label">Открыть расположение</span>
                  </button>
                )}

                <button
                  type="button"
                  className="ctx-item"
                  onClick={() => {
                    setActionError(null);
                    const hideKey = resolveTrayHideKey(activeCtxTray);
                    hideTrayApp(hideKey)
                      .then(() => {
                        setTrayApps((prev) => {
                          const next = prev.filter((x) => x.id !== activeCtxTray.id);
                          const nextH = computeTrayFlyoutHeight(next.length);
                          lastResizedHeightRef.current = nextH;
                          ipc('resize_flyout', { width: TRAY_CARD_WIDTH, height: nextH }).catch(() => {});
                          return next;
                        });
                        setTrayCtxItem(null);
                      })
                      .catch((err) => setActionError(formatIpcError(err)));
                  }}
                >
                  <EyeOffIcon className="ic14" />
                  <span className="ctx-item-label">Скрыть из списка</span>
                </button>

                <div className="ctx-sep" />

                <button
                  type="button"
                  className="ctx-item danger"
                  onClick={() => {
                    setActionError(null);
                    const pids =
                      activeCtxTray.allPids.length > 0 ? activeCtxTray.allPids : [activeCtxTray.pid];
                    ipc('end_tray_app_task', { pids })
                      .then(() => {
                        setTrayApps((prev) => {
                          const next = prev.filter((x) => x.id !== activeCtxTray.id);
                          const nextH = computeTrayFlyoutHeight(next.length);
                          lastResizedHeightRef.current = nextH;
                          ipc('resize_flyout', { width: TRAY_CARD_WIDTH, height: nextH }).catch(() => {});
                          return next;
                        });
                        setTrayCtxItem(null);
                      })
                      .catch((err) => setActionError(formatIpcError(err)));
                  }}
                >
                  <KillIcon />
                  <span className="ctx-item-label">Завершить задачу</span>
                </button>

                {actionError && (
                  <div className="ctx-err" role="alert">
                    {actionError}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    );
  }

  // 2. FLOATING NON-ACTIVATING PILL VOLUME OSD (GPU scaleX)
  if (req.kind === 'osd') {
    const pct = osdState.muted ? 0 : osdState.volume;
    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className={`flyout-card osd-card ${osdState.muted ? 'muted' : ''}`}
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
        >
          <span className="osd-ic">
            <VolIcon muted={osdState.muted} level={osdState.volume} className="ic16" />
          </span>
          <div className="osd-track">
            <div
              className="osd-fill"
              style={{ ['--vol-pct' as string]: pct / 100 } as React.CSSProperties}
            />
          </div>
          <span className="osd-val">{osdState.muted ? 'Выкл' : `${osdState.volume}%`}</span>
        </div>
      </div>
    );
  }

  // 3. EARTUMPET-PRO QUICK SOUND, MEDIA & PER-APP MIXER (332px)
  if (req.kind === 'sound' || req.kind === 'quick') {
    const allDevs = audio?.devices ?? [];
    const { primary: primaryDevs, overflow: overflowDevs, activeInPrimaryIdx, activeInOverflow } =
      partitionAudioDevices(allDevs, 3);
    const inputDevices = audio?.inputDevices ?? [];
    const activeInputId = inputDevices.find((d) => d.isDefault)?.id ?? inputDevices[0]?.id ?? '';
    const sessions = audio?.sessions ?? [];
    const netConnected = net ? net.connected : true;

    const livePosSec =
      scrubOverrideSec !== null
        ? scrubOverrideSec
        : interpolateMediaPosition(media, mediaNowMs);
    const mediaDuration = media?.durationSec ?? 0;

    const scheduleThrottledFlyoutAudio = (key: string, fn: () => void, flushNow: boolean) => {
      if (flushNow) {
        const existing = audioThrottleTimersRef.current.get(key);
        if (existing !== undefined) {
          window.clearTimeout(existing);
          audioThrottleTimersRef.current.delete(key);
        }
        audioPendingSendRef.current.delete(key);
        fn();
        return;
      }
      audioPendingSendRef.current.set(key, fn);
      if (!audioThrottleTimersRef.current.has(key)) {
        const id = window.setTimeout(() => {
          audioThrottleTimersRef.current.delete(key);
          const pending = audioPendingSendRef.current.get(key);
          audioPendingSendRef.current.delete(key);
          pending?.();
        }, 36);
        audioThrottleTimersRef.current.set(key, id);
      }
    };

    const handleSliderDrag = (sliderKey: string, dragging: boolean) => {
      if (dragging) {
        draggingAudioKeysRef.current.add(sliderKey);
        audioDragLocksRef.current.set(sliderKey, Number.POSITIVE_INFINITY);
      } else {
        draggingAudioKeysRef.current.delete(sliderKey);
        audioDragLocksRef.current.set(sliderKey, Date.now() + 280);
      }
    };

    const setMaster = (vol: number, muted: boolean, commit = false) => {
      setActionError(null);
      const reqSeq = (audioReqSeqRef.current.get('__master__') ?? 0) + 1;
      audioReqSeqRef.current.set('__master__', reqSeq);
      setAudio((prev) => (prev ? { ...prev, masterVolume: vol, masterMuted: muted } : prev));
      scheduleThrottledFlyoutAudio(
        '__master__',
        () => {
          ipc<AudioState>('set_master_volume', { volume: vol, muted })
            .then((nextState) => {
              if (
                audioReqSeqRef.current.get('__master__') === reqSeq &&
                !draggingAudioKeysRef.current.has('__master__')
              ) {
                setAudio((prev) =>
                  mergeAudioStateRespectingLocks(prev, nextState, audioDragLocksRef.current, Date.now()),
                );
              }
            })
            .catch((err) => setActionError(formatIpcError(err)));
        },
        commit,
      );
    };

    const setSessionVolume = (
      s: AudioState['sessions'][number],
      vol: number,
      commit = false,
    ) => {
      setActionError(null);
      const reqSeq = (audioReqSeqRef.current.get(s.id) ?? 0) + 1;
      audioReqSeqRef.current.set(s.id, reqSeq);
      const nextMuted = s.muted ? false : undefined;
      setAudio((prev) =>
        prev
          ? {
              ...prev,
              sessions: prev.sessions.map((item) =>
                item.id === s.id
                  ? { ...item, volume: vol, muted: nextMuted ?? item.muted }
                  : item,
              ),
            }
          : prev,
      );
      scheduleThrottledFlyoutAudio(
        s.id,
        () => {
          setSessionState({
            appKey: s.id,
            volume: vol / 100,
            muted: nextMuted,
            pid: s.pid,
            pids: s.pids,
          })
            .then((nextState) => {
              if (
                audioReqSeqRef.current.get(s.id) === reqSeq &&
                !draggingAudioKeysRef.current.has(s.id)
              ) {
                setAudio((prev) =>
                  mergeAudioStateRespectingLocks(prev, nextState, audioDragLocksRef.current, Date.now()),
                );
              }
            })
            .catch((err) => setActionError(formatIpcError(err)));
        },
        commit,
      );
    };

    const toggleSessionMute = (s: AudioState['sessions'][number]) => {
      setActionError(null);
      const nextMuted = !s.muted;
      setAudio((prev) =>
        prev
          ? {
              ...prev,
              sessions: prev.sessions.map((item) =>
                item.id === s.id ? { ...item, muted: nextMuted } : item,
              ),
            }
          : prev,
      );
      setSessionState({
        appKey: s.id,
        muted: nextMuted,
        pid: s.pid,
        pids: s.pids,
      })
        .then(setAudio)
        .catch((err) => setActionError(formatIpcError(err)));
    };

    const selectOutputDevice = (deviceId: string) => {
      setActionError(null);
      setDevMenuOpen(false);
      ipc<AudioState>('set_audio_device', { deviceId })
        .then(setAudio)
        .catch((err) => setActionError(formatIpcError(err)));
    };

    const toggleMicMute = () => {
      setActionError(null);
      const nextMuted = !(audio?.micMuted ?? false);
      setAudio((prev) => (prev ? { ...prev, micMuted: nextMuted } : prev));
      ipc<AudioState>('set_mic_muted', { muted: nextMuted })
        .then(setAudio)
        .catch((err) => setActionError(formatIpcError(err)));
    };

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card qs-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
          onClick={() => {
            if (devMenuOpen) setDevMenuOpen(false);
          }}
        >
          <div className="sec-row" style={{ marginBottom: 8 }}>
            <span className="sec-title">Звук и сеть</span>
            <div className="qs-head-pills">
              {inputDevices.length <= 1 && (
                <button
                  type="button"
                  className={`qs-mic-btn ${audio?.micMuted ? 'muted' : ''}`}
                  aria-label={audio?.micMuted ? 'Включить микрофон' : 'Отключить микрофон'}
                  aria-pressed={Boolean(audio?.micMuted)}
                  onClick={toggleMicMute}
                >
                  <MicIcon className="ic12" muted={audio?.micMuted ?? false} />
                  <span>{audio?.micMuted ? 'Выкл' : 'Мик'}</span>
                </button>
              )}
              <button
                type="button"
                className={`qs-net-pill ${!netConnected ? 'offline' : ''}`}
                aria-label="Открыть параметры сети Windows"
                onClick={() => {
                  ipc('launch_item', {
                    target: 'ms-settings:network',
                    args: null,
                    asAdmin: false,
                    closeStart: false,
                  }).catch(() => {});
                  close();
                }}
              >
                <span className="qs-net-dot" aria-hidden="true" />
                <span>{formatCompactNetworkLabel(net)}</span>
              </button>
            </div>
          </div>

          {media?.hasSession && (
            <div className="qs-now-playing">
              <div className="qs-np-top">
                <div className="qs-np-art">
                  {media.thumbnail ? (
                    <img src={media.thumbnail} alt="" draggable={false} />
                  ) : (
                    <MusicNoteIcon className="ic14" />
                  )}
                </div>
                <div className="qs-np-meta">
                  <div className="qs-np-title">{media.title || 'Воспроизведение'}</div>
                  <div className="qs-np-artist">
                    {[media.artist, media.source].filter(Boolean).join(' · ') || 'Медиа'}
                  </div>
                </div>
                <div className="m-ctrls">
                  <button
                    type="button"
                    className="m-btn"
                    aria-label="Предыдущий трек"
                    onClick={() =>
                      ipc<MediaState>('media_action', { action: 'prev' })
                        .then(setMedia)
                        .catch((err) => setActionError(formatIpcError(err)))
                    }
                  >
                    <PrevTrackIcon className="ic12" />
                  </button>
                  <button
                    type="button"
                    className="m-btn qs-play-btn"
                    aria-label={media.isPlaying ? 'Пауза' : 'Воспроизвести'}
                    onClick={() =>
                      ipc<MediaState>('media_action', { action: 'toggle' })
                        .then(setMedia)
                        .catch((err) => setActionError(formatIpcError(err)))
                    }
                  >
                    {media.isPlaying ? (
                      <PauseIcon size={12} />
                    ) : (
                      <PlaySmallIcon size={12} />
                    )}
                  </button>
                  <button
                    type="button"
                    className="m-btn"
                    aria-label="Следующий трек"
                    onClick={() =>
                      ipc<MediaState>('media_action', { action: 'next' })
                        .then(setMedia)
                        .catch((err) => setActionError(formatIpcError(err)))
                    }
                  >
                    <NextTrackIcon className="ic12" />
                  </button>
                </div>
              </div>

              {mediaDuration > 0 && (
                <div className="scrub-wrap">
                  <span className="scrub-time">{formatMediaTime(livePosSec)}</span>
                  {media.canSeek ? (
                    <ObsidianSlider
                      className="media-scrub-slider"
                      min={0}
                      max={Math.max(1, Math.round(mediaDuration))}
                      step={1}
                      wheelStep={5}
                      value={Math.round(livePosSec)}
                      magneticDetents={[]}
                      showCenterNotch={false}
                      ariaLabel="Позиция трека"
                      onChange={(v) => {
                        seekSeqRef.current += 1;
                        setScrubOverrideSec(v);
                      }}
                      onCommit={(v) => {
                        const seq = ++seekSeqRef.current;
                        setScrubOverrideSec(v);
                        seekMedia(v)
                          .then((next) => {
                            if (seekSeqRef.current === seq) {
                              setMedia(next);
                              setMediaNowMs(Date.now());
                              setScrubOverrideSec(null);
                            }
                          })
                          .catch((err) => {
                            if (seekSeqRef.current === seq) {
                              setScrubOverrideSec(null);
                            }
                            setActionError(formatIpcError(err));
                          });
                      }}
                    />
                  ) : (
                    <div className="scrub-bar">
                      <div
                        className="scrub-fill"
                        style={
                          {
                            ['--pct' as string]: Math.min(1, Math.max(0, livePosSec / mediaDuration)),
                          } as React.CSSProperties
                        }
                      />
                    </div>
                  )}
                  <span className="scrub-time">{formatMediaTime(mediaDuration)}</span>
                </div>
              )}
            </div>
          )}

          {primaryDevs.length > 0 && (
            <>
              <div
                className="qs-dev-row"
                style={devMenuPresence.mounted ? { zIndex: 35 } : undefined}
                onClick={(e) => e.stopPropagation()}
              >
                <div
                  className="seg-bar sliding"
                  style={
                    {
                      gridTemplateColumns: `repeat(${primaryDevs.length}, 1fr)`,
                      ['--seg-count' as string]: primaryDevs.length,
                      ['--seg-idx' as string]: Math.max(0, activeInPrimaryIdx),
                    } as React.CSSProperties
                  }
                >
                  <span
                    className={`seg-indicator ${segAnimReady ? '' : 'no-anim'}`}
                    aria-hidden="true"
                    style={activeInOverflow ? { opacity: 0 } : undefined}
                  />
                  {primaryDevs.map((d, idx) => (
                    <button
                      key={d.id || `dev-${d.name}-${idx}`}
                      type="button"
                      className={`seg-opt ${d.isDefault ? 'active' : ''}`}
                      title={d.name}
                      onClick={() => selectOutputDevice(d.id)}
                    >
                      {d.shortName}
                    </button>
                  ))}
                </div>

                {overflowDevs.length > 0 && (
                  <button
                    type="button"
                    className={`qs-dev-more-btn ${devMenuOpen || activeInOverflow ? 'open' : ''}`}
                    aria-label={`Все устройства вывода (${allDevs.length})`}
                    aria-expanded={devMenuOpen}
                    onClick={() => setDevMenuOpen((v) => !v)}
                  >
                    <ChevronDownIcon className="ic14" />
                  </button>
                )}
              </div>

              {devMenuPresence.mounted && overflowDevs.length > 0 && (
                <div
                  className="qs-dev-list-inflow"
                  data-phase={devMenuPresence.phase}
                  onClick={(e) => e.stopPropagation()}
                >
                  {allDevs.map((d, idx) => (
                    <button
                      key={d.id || `all-dev-${d.name}-${idx}`}
                      type="button"
                      className={`ctx-item ${d.isDefault ? 'focused' : ''}`}
                      title={d.name}
                      onClick={() => selectOutputDevice(d.id)}
                    >
                      {d.isDefault ? <CheckIcon size={13} /> : <VolIcon className="ic14" />}
                      <span className="ctx-item-label">{d.name}</span>
                    </button>
                  ))}
                </div>
              )}
            </>
          )}

          {inputDevices.length > 1 && (
            <div className="qs-mic-row">
              <button
                type="button"
                className={`qs-mic-btn ${audio?.micMuted ? 'muted' : ''}`}
                aria-label={audio?.micMuted ? 'Включить микрофон' : 'Отключить микрофон'}
                onClick={toggleMicMute}
              >
                <MicIcon className="ic14" muted={audio?.micMuted ?? false} />
                <span>{audio?.micMuted ? 'Выкл' : 'Микрофон'}</span>
              </button>
              <select
                className="qs-input-select"
                aria-label="Устройство записи"
                value={activeInputId}
                onChange={(e) => {
                  const id = e.target.value;
                  setActionError(null);
                  setInputDevice(id)
                    .then(setAudio)
                    .catch((err) => setActionError(formatIpcError(err)));
                }}
              >
                {inputDevices.map((d, idx) => (
                  <option key={d.id || `in-dev-${d.name}-${idx}`} value={d.id}>
                    {d.shortName || d.name}
                  </option>
                ))}
              </select>
            </div>
          )}

          <div className={`mix-row qs-master-row ${audio?.masterMuted ? 'muted' : ''}`}>
            <button
              type="button"
              className="mix-mute"
              title={audio?.masterMuted ? 'Включить звук' : 'Выключить звук'}
              onClick={() => setMaster(audio?.masterVolume ?? 0, !(audio?.masterMuted ?? false), true)}
            >
              <VolIcon muted={audio?.masterMuted} level={audio?.masterVolume} />
            </button>
            <span className="mix-name" style={{ fontWeight: 600 }}>Общий звук</span>
            <ObsidianSlider
              value={audio?.masterVolume ?? 0}
              muted={audio?.masterMuted ?? false}
              peak={peaks ? peaks.masterPeak : 0}
              noTransition={!segAnimReady}
              magneticDetents={[0, 50, 100]}
              ariaLabel="Общий звук"
              onDragStateChange={(d) => handleSliderDrag('__master__', d)}
              onChange={(v) => setMaster(v, false, false)}
              onCommit={(v) => setMaster(v, false, true)}
            />
            <span className="mix-val">
              {audio?.masterMuted ? '0%' : `${audio?.masterVolume ?? 0}%`}
            </span>
          </div>

          <div className="mixer-list qs-mixer-scroll">
            {sessions.length === 0 ? (
              <div className="qs-empty">Нет активных аудиопотоков</div>
            ) : (
              sessions.map((s, idx) => {
                const iconUri = s.iconKey
                  ? sessionIconMap[s.iconKey] || iconCache.get(s.iconKey) || null
                  : null;
                const livePeak = s.muted || !peaks ? 0 : (sessionPeakMap.get(s.id) ?? 0);
                const isActiveAudio = !s.muted && livePeak > 0.02;
                return (
                  <div
                    key={s.id || `sess-${s.pid ?? idx}-${s.name}-${idx}`}
                    className={`mix-row ${s.muted ? 'muted' : ''} ${isActiveAudio ? 'active-audio' : ''}`}
                  >
                    <button
                      type="button"
                      className="mix-mute"
                      title={s.muted ? 'Включить звук приложения' : 'Отключить звук приложения'}
                      onClick={() => toggleSessionMute(s)}
                    >
                      <VolIcon muted={s.muted} level={s.volume} />
                    </button>
                    {iconUri ? (
                      <img src={iconUri} alt="" className="mix-app-ic" draggable={false} />
                    ) : (
                      <FallbackAppIcon iconKey={s.name} size={16} />
                    )}
                    <span className="mix-name" title={s.name}>{s.name}</span>
                    <ObsidianSlider
                      value={s.volume}
                      muted={s.muted}
                      peak={livePeak}
                      noTransition={!segAnimReady}
                      magneticDetents={[0, 50, 100]}
                      ariaLabel={`Громкость ${s.name}`}
                      onDragStateChange={(d) => handleSliderDrag(s.id, d)}
                      onChange={(v) => setSessionVolume(s, v, false)}
                      onCommit={(v) => setSessionVolume(s, v, true)}
                    />
                    <span className="mix-val">{s.muted ? '0%' : `${s.volume}%`}</span>
                  </div>
                );
              })
            )}
          </div>

          {actionError && (
            <div className="ctx-err" role="alert" style={{ marginTop: 8 }}>
              {actionError}
            </div>
          )}
        </div>
      </div>
    );
  }

  // 4. INTERACTIVE CLOCK & CALENDAR (310px, DIRECTIONAL MONTH SLIDE, WEEKEND TINT, AA CONTRAST)
  if (req.kind === 'calendar') {
    const awayFromToday =
      !isSameCalendarDay(selectedDate, today) || !isViewingTodayMonth(calView, today);

    const navigateMonth = (next: CalendarMonthRef) => {
      setCalDir(computeCalendarSlideDir(calView, next));
      setCalView(next);
    };

    const jumpToToday = () => {
      const fresh = new Date();
      const targetRef = { year: fresh.getFullYear(), month: fresh.getMonth() };
      setCalDir(computeCalendarSlideDir(calView, targetRef));
      setCalView(targetRef);
      setSelectedDate(fresh);
    };

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card cal-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
        >
          <div className="cal-selected-hdr">{formatSelectedDateHeader(selectedDate)}</div>

          <div className="cal-top-bar">
            <strong className="cal-month-title">
              {MONTHS_RU[calView.month]} {calView.year}
            </strong>
            <div className="cal-nav-group">
              <button
                type="button"
                className={`cal-today-btn ${awayFromToday ? 'active' : ''}`}
                onClick={jumpToToday}
              >
                Сегодня
              </button>
              <button
                type="button"
                className="hdr-btn cal-nav-btn"
                title="Предыдущий месяц"
                aria-label="Предыдущий месяц"
                onClick={() => navigateMonth(shiftCalendarMonth(calView.year, calView.month, -1))}
              >
                <ChevronLeftIcon className="ic14" />
              </button>
              <button
                type="button"
                className="hdr-btn cal-nav-btn"
                title="Следующий месяц"
                aria-label="Следующий месяц"
                onClick={() => navigateMonth(shiftCalendarMonth(calView.year, calView.month, 1))}
              >
                <ChevronRightIcon className="ic14" />
              </button>
            </div>
          </div>

          <div className="cal-wd-row" aria-hidden="true">
            {WEEKDAYS_SHORT_RU.map((wd, idx) => (
              <div key={wd} className={`cal-wd ${isWeekendColumn(idx) ? 'weekend' : ''}`}>
                {wd}
              </div>
            ))}
          </div>

          <div className="cal-days-viewport">
            <div
              key={`${calView.year}-${calView.month}`}
              className="cal-days-grid"
              style={{ ['--cal-dir' as string]: calDir } as React.CSSProperties}
            >
              {calCells.map((c) => (
                <button
                  key={c.key}
                  type="button"
                  className={[
                    'cal-day',
                    c.isWeekend ? 'weekend' : '',
                    c.inCurrentMonth ? '' : 'muted',
                    c.isToday ? 'today' : '',
                    c.isSelected ? 'selected' : '',
                  ]
                    .filter(Boolean)
                    .join(' ')}
                  onClick={() => {
                    setSelectedDate(new Date(c.year, c.month, c.day));
                    if (!c.inCurrentMonth) {
                      navigateMonth({ year: c.year, month: c.month });
                    }
                  }}
                >
                  {c.day}
                </button>
              ))}
            </div>
          </div>

          <div className="cal-alerts-section">
            <div className="sec-row" style={{ marginBottom: 6 }}>
              <span className="sec-title">События TestBar</span>
              {alerts.length > 0 && (
                <button
                  type="button"
                  className="sec-act"
                  onClick={() => ipc<SessionAlert[]>('clear_session_alerts').then(setAlerts).catch(() => {})}
                >
                  Очистить
                </button>
              )}
            </div>

            {alerts.length === 0 ? (
              <div className="cal-alerts-empty">Нет системных событий</div>
            ) : (
              <div className="cal-alerts-list">
                {alerts.slice(0, 4).map((a, idx) => (
                  <div key={a.id || `alert-${a.timestampMs ?? idx}-${idx}`} className="cal-alert-row">
                    <span className={`cal-alert-dot ${a.level}`} />
                    <div className="cal-alert-body">
                      <div className="cal-alert-top">
                        <span className="cal-alert-title">{a.title}</span>
                        <span className="cal-alert-time">{formatAlertTime(a.timestampMs)}</span>
                      </div>
                      {a.detail && <div className="cal-alert-detail">{a.detail}</div>}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          <button
            type="button"
            className="cal-notif-btn"
            onClick={() => {
              ipc('open_windows_notifications').catch(() => {});
            }}
          >
            <BellIcon className="ic14" />
            <span>Уведомления Windows</span>
          </button>
        </div>
      </div>
    );
  }

  // 4B. WEATHER FORECAST FLYOUT (340px, 18px radius, inline city search, 12h + 5d)
  if (req.kind === 'weather') {
    const units: 'c' | 'f' = config?.taskbarWidgets?.weatherUnits === 'f' ? 'f' : 'c';
    const unitLetter = units === 'f' ? 'F' : 'C';
    const configuredCity = config?.taskbarWidgets?.weatherCity?.trim() ?? '';
    const isAutoLocation = !configuredCity && config?.taskbarWidgets?.weatherLat == null;
    const headerCity = weather?.city?.trim() || configuredCity || 'Погода';

    const dailyRows = weather?.daily.slice(0, 5) ?? [];
    const weekMinC =
      dailyRows.length > 0 ? Math.min(...dailyRows.map((d) => d.tempMinC)) : 0;
    const weekMaxC =
      dailyRows.length > 0 ? Math.max(...dailyRows.map((d) => d.tempMaxC)) : 0;

    const refreshWeatherNow = () => {
      if (weatherLoading) return;
      setWeatherLoading(true);
      getWeather({ force: true })
        .then((snap) => setWeather(snap))
        .catch(() => {})
        .finally(() => setWeatherLoading(false));
    };

    const selectWeatherCity = (opt: WeatherCityOption) => {
      setWeatherCityError(null);
      setWeatherCityOpen(false);
      setWeatherLoading(true);
      setTaskbarWidgets({
        weatherCity: opt.name,
        weatherLat: opt.lat,
        weatherLon: opt.lon,
      })
        .then((updated) => {
          setConfig((prev) => applyConfigUpdate(prev, updated));
          return getWeather({
            city: opt.name,
            lat: opt.lat,
            lon: opt.lon,
            force: true,
          });
        })
        .then((snap) => setWeather(snap))
        .catch((err) => setWeatherCityError(formatIpcError(err)))
        .finally(() => setWeatherLoading(false));
    };

    const resetWeatherToAuto = () => {
      setWeatherCityError(null);
      setWeatherCityOpen(false);
      setWeatherLoading(true);
      setTaskbarWidgets({
        weatherCity: '',
        weatherLat: null,
        weatherLon: null,
      })
        .then((updated) => {
          setConfig((prev) => applyConfigUpdate(prev, updated));
          return getWeather({ force: true });
        })
        .then((snap) => setWeather(snap))
        .catch((err) => setWeatherCityError(formatIpcError(err)))
        .finally(() => setWeatherLoading(false));
    };

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card weather-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
        >
          <div className="wf-head">
            <div className="wf-head-meta">
              <span className="wf-title" title={headerCity}>
                {headerCity}
              </span>
              {weather?.available && weather.stale && (
                <span className="wf-badge stale">Офлайн</span>
              )}
            </div>
            <div className="wf-head-actions">
              <button
                type="button"
                className={`wf-icon-btn ${weatherCityOpen ? 'active' : ''}`}
                aria-label="Выбрать город"
                aria-expanded={weatherCityOpen}
                onClick={() => setWeatherCityOpen((v) => !v)}
              >
                <SearchIcon className="ic14" />
              </button>
              <button
                type="button"
                className={`wf-icon-btn ${weatherLoading ? 'spinning' : ''}`}
                aria-label="Обновить прогноз"
                disabled={weatherLoading}
                onClick={refreshWeatherNow}
              >
                <RefreshIcon className="ic14" />
              </button>
            </div>
          </div>

          {weatherCityOpen && (
            <div className="weather-city-drawer">
              <div className="weather-city-search-row">
                <input
                  type="text"
                  className="weather-city-input"
                  value={weatherCityQuery}
                  onChange={(e) => setWeatherCityQuery(e.target.value)}
                  placeholder="Поиск города…"
                  aria-label="Поиск города"
                  autoFocus
                />
                <button
                  type="button"
                  className={`weather-auto-btn ${isAutoLocation ? 'active' : ''}`}
                  onClick={resetWeatherToAuto}
                >
                  Авто
                </button>
              </div>

              {weatherCitySearching && (
                <div className="weather-city-msg">Поиск городов…</div>
              )}

              {weatherCityError && (
                <div className="weather-city-msg err" role="alert">
                  {weatherCityError}
                </div>
              )}

              {!weatherCitySearching &&
                !weatherCityError &&
                weatherCityQuery.trim().length >= 2 &&
                weatherCityResults.length === 0 && (
                  <div className="weather-city-msg">Ничего не найдено</div>
                )}

              {weatherCityResults.length > 0 && (
                <div className="weather-city-results">
                  {weatherCityResults.map((opt, idx) => {
                    const sub = formatCityOptionSubtitle(opt);
                    return (
                      <button
                        key={`${opt.name}-${opt.lat}-${opt.lon}-${idx}`}
                        type="button"
                        className="weather-city-opt"
                        onClick={() => selectWeatherCity(opt)}
                      >
                        <span className="weather-city-opt-name">{opt.name}</span>
                        {sub && <span className="weather-city-opt-sub">{sub}</span>}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          )}

          {weather?.available ? (
            <>
              <div className="weather-hero" data-condition={weather.conditionIcon}>
                <div className="weather-hero-left">
                  <WeatherConditionIcon
                    condition={weather.conditionIcon}
                    className="ic34 weather-hero-ic"
                  />
                  <div className="weather-temp-big">
                    {formatWeatherTempLabel(weather.tempC, units)}
                    <span className="weather-unit-tag">{unitLetter}</span>
                  </div>
                </div>
                <div className="weather-hero-right">
                  <div className="weather-cond-label" title={weather.conditionLabel}>
                    {weather.conditionLabel}
                  </div>
                  <div className="weather-feels">
                    Ощущается как {formatWeatherTempLabel(weather.feelsLikeC, units)}
                  </div>
                  <div className="weather-metrics">
                    <span className="weather-metric" title="Влажность">
                      <DropletIcon className="ic12" />
                      <span>{weather.humidity}%</span>
                    </span>
                    <span className="weather-metric" title="Ветер">
                      <WindIcon className="ic12" />
                      <span>{formatWeatherWindLabel(weather.windKmh)}</span>
                    </span>
                  </div>
                </div>
              </div>

              {weather.hourly.length > 0 && (
                <div className="weather-hourly-strip" aria-label="Почасовой прогноз">
                  {weather.hourly.slice(0, 12).map((h, idx) => (
                    <div key={`${h.time}-${idx}`} className="weather-hour-cell">
                      <span className="weather-hour-time">{h.time}</span>
                      <WeatherConditionIcon condition={h.conditionIcon} className="ic16" />
                      <span className="weather-hour-temp">
                        {formatWeatherTempLabel(h.tempC, units)}
                      </span>
                      <span className={`weather-hour-precip ${h.precipProb >= 20 ? 'wet' : ''}`}>
                        {h.precipProb}%
                      </span>
                    </div>
                  ))}
                </div>
              )}

              {dailyRows.length > 0 && (
                <div className="weather-daily-list" aria-label="Прогноз на 5 дней">
                  {dailyRows.map((d, idx) => {
                    const bar = computeWeatherTempBarRange(
                      d.tempMinC,
                      d.tempMaxC,
                      weekMinC,
                      weekMaxC,
                    );
                    return (
                      <div key={d.date || `day-${idx}`} className="weather-day-row">
                        <span className="weather-day-label">{d.label}</span>
                        <WeatherConditionIcon condition={d.conditionIcon} className="ic14" />
                        <span className={`weather-day-precip ${d.precipProb >= 20 ? 'wet' : ''}`}>
                          {d.precipProb > 0 ? `${d.precipProb}%` : '—'}
                        </span>
                        <span className="weather-day-min">
                          {formatWeatherTempLabel(d.tempMinC, units)}
                        </span>
                        <div className="weather-range-track" aria-hidden="true">
                          <div
                            className="weather-range-fill"
                            style={{ left: `${bar.leftPct}%`, width: `${bar.widthPct}%` }}
                          />
                        </div>
                        <span className="weather-day-max">
                          {formatWeatherTempLabel(d.tempMaxC, units)}
                        </span>
                      </div>
                    );
                  })}
                </div>
              )}
            </>
          ) : (
            <div className="weather-empty">
              <WeatherConditionIcon condition="cloud" className="ic24" />
              <div>{weatherLoading ? 'Загрузка прогноза…' : 'Нет данных о погоде'}</div>
              {weather?.error && <div className="weather-empty-err">{weather.error}</div>}
            </div>
          )}
        </div>
      </div>
    );
  }

  // 4C. WIRELESS PERIPHERAL BATTERIES FLYOUT (304px, 16px radius)
  if (req.kind === 'battery') {
    const handleRefreshBatteries = () => {
      if (periphRefreshing) return;
      setActionError(null);
      setPeriphRefreshing(true);
      refreshPeripheralBatteries()
        .then((items) => {
          if (Array.isArray(items)) setPeriphBatteries(items);
          setPeriphHydrated(true);
        })
        .catch(() => {})
        .finally(() => setPeriphRefreshing(false));
    };

    const handleHideBattery = (id: string) => {
      setActionError(null);
      hideBatteryDevice(id)
        .then((updated) => setConfig((prev) => applyConfigUpdate(prev, updated)))
        .catch((err) => setActionError(formatIpcError(err)));
    };

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card battery-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
        >
          <div className="wf-head">
            <div className="wf-head-meta">
              <span className="wf-title">Беспроводные устройства</span>
              {visibleBatteries.length > 0 && (
                <span className="wf-badge count">{visibleBatteries.length}</span>
              )}
            </div>
            <button
              type="button"
              className={`wf-icon-btn ${periphRefreshing ? 'spinning' : ''}`}
              aria-label="Обновить устройства"
              disabled={periphRefreshing}
              onClick={handleRefreshBatteries}
            >
              <RefreshIcon className="ic14" />
            </button>
          </div>

          {visibleBatteries.length === 0 ? (
            <div className="bat-empty">
              <div className="bat-empty-ic">
                <BluetoothIcon className="ic18" />
              </div>
              <span>
                {!periphHydrated
                  ? 'Поиск подключённых устройств…'
                  : periphBatteries.length > 0
                    ? 'Все устройства скрыты'
                    : 'Беспроводные устройства не найдены'}
              </span>
            </div>
          ) : (
            <div className="bat-list">
              {visibleBatteries.map((dev, idx) => {
                const pct = Math.max(0, Math.min(100, Math.round(dev.percent)));
                const isLow = pct < 20;
                const itemCls = [
                  'bat-item',
                  isLow ? 'low' : '',
                  dev.charging ? 'charging' : '',
                ]
                  .filter(Boolean)
                  .join(' ');
                const fillCls = [
                  'bat-bar-fill',
                  dev.charging ? 'charging' : isLow ? 'low' : '',
                ]
                  .filter(Boolean)
                  .join(' ');

                return (
                  <div key={dev.id || `bat-${dev.name}-${idx}`} className={itemCls}>
                    <div className="bat-item-top">
                      <div className="bat-ic-box">
                        <PeripheralDeviceIcon kind={dev.kind} className="ic16" />
                      </div>
                      <div className="bat-meta">
                        <div className="bat-name" title={dev.name}>
                          {dev.name}
                        </div>
                        <div className="bat-badges-row">
                          <span className="bat-source-badge">
                            {formatBatterySourceLabel(dev.source)}
                          </span>
                        </div>
                      </div>
                      <div className="bat-pct-wrap">
                        {dev.charging && <ChargingBoltIcon className="ic12 bat-bolt" />}
                        <span className="bat-pct">{pct}%</span>
                      </div>
                      <button
                        type="button"
                        className="bat-hide-btn"
                        title="Скрыть устройство"
                        aria-label={`Скрыть устройство ${dev.name}`}
                        onClick={() => handleHideBattery(dev.id)}
                      >
                        <EyeOffIcon className="ic12" />
                      </button>
                    </div>

                    <div className="bat-bar-track" aria-hidden="true">
                      <div
                        className={fillCls}
                        style={{ transform: `scaleX(${pct / 100})` }}
                      />
                    </div>

                    {dev.subLevels && dev.subLevels.length > 0 && (
                      <div className="bat-sublevels">
                        {dev.subLevels.map((sub, sIdx) => {
                          const subPct = Math.max(0, Math.min(100, Math.round(sub.percent)));
                          return (
                            <span
                              key={`${sub.label}-${sIdx}`}
                              className={`bat-sub-chip ${subPct < 20 ? 'low' : ''}`}
                            >
                              <span className="bat-sub-lbl">{sub.label}</span>
                              <span>{subPct}%</span>
                              {sub.charging && <ChargingBoltIcon className="ic12" />}
                            </span>
                          );
                        })}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}

          {actionError && (
            <div className="ctx-err" role="alert">
              {actionError}
            </div>
          )}
        </div>
      </div>
    );
  }

  // 4D. NETWORK MONITOR FLYOUT (316px, 16px radius)
  if (req.kind === 'network') {
    const isOnline = netSnap ? netSnap.online : true;
    const rawKind = (netSnap?.kind ?? '').trim().toLowerCase();
    const isWifi = rawKind.includes('wi-fi') || rawKind.includes('wifi') || rawKind.includes('wireless');
    const kindLabel = !isOnline ? 'Офлайн' : isWifi ? 'Wi-Fi' : 'Ethernet';
    const adapterTitle = netSnap?.adapterName?.trim() || (isOnline ? 'Сетевой адаптер' : 'Нет подключения');
    const localIp = netSnap?.localIp?.trim() || '';
    const pingMs = netSnap?.pingMs ?? null;
    const pingHost = netSnap?.pingHost?.trim() || '1.1.1.1';
    const pingTone =
      pingMs === null ? '' : pingMs < 40 ? 'ok' : pingMs < 100 ? 'accent' : 'warn';

    const handleProbePing = () => {
      if (netPinging) return;
      setActionError(null);
      setNetPinging(true);
      getNetworkSnapshot(true)
        .then((snap) => {
          if (snap) setNetSnap(snap);
        })
        .catch((err) => setActionError(formatIpcError(err)))
        .finally(() => setNetPinging(false));
    };

    const handleCopyIp = () => {
      if (!localIp) return;
      setActionError(null);
      ipc('copy_to_clipboard', { text: localIp })
        .then(() => {
          setNetIpCopied(true);
          if (netCopyTimerRef.current !== null) window.clearTimeout(netCopyTimerRef.current);
          netCopyTimerRef.current = window.setTimeout(() => setNetIpCopied(false), 1600);
        })
        .catch((err) => setActionError(formatIpcError(err)));
    };

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card net-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
        >
          <div className="wf-head">
            <div className="wf-head-meta">
              <span className="net-head-ic">
                {!isOnline ? (
                  <OfflineIcon className="ic16" />
                ) : isWifi ? (
                  <WifiIcon className="ic16" />
                ) : (
                  <EthernetIcon className="ic16" />
                )}
              </span>
              <span className="wf-title">Сеть</span>
              <span className={`net-status-badge ${isOnline ? 'online' : 'offline'}`}>
                <span className="net-status-dot" aria-hidden="true" />
                <span>{isOnline ? 'Онлайн' : 'Офлайн'}</span>
              </span>
            </div>
            <div className="wf-head-actions">
              <button
                type="button"
                className="wf-icon-btn"
                title="Параметры сети Windows"
                aria-label="Параметры сети Windows"
                onClick={() => {
                  ipc('launch_item', {
                    target: 'ms-settings:network',
                    args: null,
                    asAdmin: false,
                    closeStart: false,
                  }).catch(() => {});
                  close();
                }}
              >
                <SettingsGearIcon className="ic14" />
              </button>
            </div>
          </div>

          <div className="net-adapter-box">
            <div className="net-adapter-meta">
              <span className="net-adapter-lbl">Адаптер</span>
              <span className="net-adapter-name" title={adapterTitle}>
                {adapterTitle}
              </span>
            </div>
            <span className="wf-badge count">{kindLabel}</span>
          </div>

          <div className="net-rate-grid">
            <div className="net-rate-box rx">
              <span className="net-rate-lbl">
                <ArrowDownIcon size={11} />
                <span>Скачивание</span>
              </span>
              <span className="net-rate-val">
                {formatNetworkRateCompact(isOnline ? netSnap?.rxBytesPerSec : 0)}
              </span>
            </div>
            <div className="net-rate-box tx">
              <span className="net-rate-lbl">
                <ArrowUpIcon size={11} />
                <span>Отдача</span>
              </span>
              <span className="net-rate-val">
                {formatNetworkRateCompact(isOnline ? netSnap?.txBytesPerSec : 0)}
              </span>
            </div>
          </div>

          <div className="net-info-list">
            <div className="net-info-row">
              <span className="net-info-lbl">Локальный IPv4</span>
              <div className="net-info-val-wrap">
                <span className="net-info-mono">{localIp || '—'}</span>
                <button
                  type="button"
                  className={`net-act-btn ${netIpCopied ? 'copied' : ''}`}
                  disabled={!localIp}
                  aria-label="Скопировать IPv4"
                  title="Скопировать IPv4"
                  onClick={handleCopyIp}
                >
                  {netIpCopied ? <CheckIcon size={12} /> : <CopyIcon className="ic12" />}
                  <span>{netIpCopied ? 'Скопировано' : 'Копировать'}</span>
                </button>
              </div>
            </div>

            <div className="net-info-row">
              <span className="net-info-lbl" title={`TCP-проверка ${pingHost}`}>
                Пинг · {pingHost}
              </span>
              <div className="net-info-val-wrap">
                <span className={`net-ping-pill ${pingTone}`}>
                  {pingMs !== null ? `${pingMs} мс` : '—'}
                </span>
                <button
                  type="button"
                  className={`net-act-btn ${netPinging ? 'spinning' : ''}`}
                  disabled={netPinging || !isOnline}
                  onClick={handleProbePing}
                >
                  <RefreshIcon className="ic12" />
                  <span>Проверить</span>
                </button>
              </div>
            </div>
          </div>

          {actionError && (
            <div className="ctx-err" role="alert">
              {actionError}
            </div>
          )}
        </div>
      </div>
    );
  }

  // 4E. PC TELEMETRY FLYOUT (324px, 16px radius)
  if (req.kind === 'telemetry') {
    const cpuPct = Math.max(0, Math.min(100, Math.round(telemSnap?.cpuPct ?? 0)));
    const ramPct = Math.max(0, Math.min(100, Math.round(telemSnap?.ramPct ?? 0)));
    const gpuPct = Math.max(0, Math.min(100, Math.round(telemSnap?.gpuPct ?? 0)));
    const vramPct = Math.max(0, Math.min(100, Math.round(telemSnap?.vramPct ?? 0)));
    const gpuTemp = Math.round(telemSnap?.gpuTempC ?? 0);
    const gpuTempTone = gpuTemp >= 80 ? 'hot' : gpuTemp >= 65 ? 'warn' : 'ok';
    const disks = telemSnap?.disks ?? [];

    const handleRefreshTelemetry = () => {
      if (telemRefreshing) return;
      setActionError(null);
      setTelemRefreshing(true);
      ipc<TelemetryState>('get_telemetry')
        .then((snap) => {
          if (snap) setTelemSnap(snap);
        })
        .catch((err) => setActionError(formatIpcError(err)))
        .finally(() => setTelemRefreshing(false));
    };

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card telem-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
        >
          <div className="wf-head">
            <div className="wf-head-meta">
              <span className="wf-title">Монитор ПК</span>
              {telemSnap && (
                <span className="wf-badge count">
                  {Math.round(telemSnap.ramTotalGb)} ГБ ОЗУ
                </span>
              )}
            </div>
            <div className="wf-head-actions">
              <button
                type="button"
                className="wf-icon-btn"
                title="Диспетчер задач"
                aria-label="Диспетчер задач"
                onClick={() => {
                  ipc('launch_item', {
                    target: 'C:\\Windows\\System32\\Taskmgr.exe',
                    args: null,
                    asAdmin: false,
                    closeStart: false,
                  }).catch(() => {});
                  close();
                }}
              >
                <TaskmgrIcon className="ic14" />
              </button>
              <button
                type="button"
                className={`wf-icon-btn ${telemRefreshing ? 'spinning' : ''}`}
                aria-label="Обновить телеметрию"
                disabled={telemRefreshing}
                onClick={handleRefreshTelemetry}
              >
                <RefreshIcon className="ic14" />
              </button>
            </div>
          </div>

          <div className="telem-rows">
            <div className="telem-row">
              <div className="telem-row-top">
                <span className="telem-lbl" title={telemSnap?.cpuName || 'Процессор'}>
                  Процессор
                  {telemSnap?.cpuName && <span className="telem-sub"> · {telemSnap.cpuName}</span>}
                </span>
                <span className="telem-val">{telemSnap ? `${cpuPct}%` : '—'}</span>
              </div>
              <div className="telem-bar-track" aria-hidden="true">
                <div
                  className={`telem-bar-fill ${getTelemetryUsageTone(cpuPct)}`}
                  style={{ transform: `scaleX(${cpuPct / 100})` }}
                />
              </div>
            </div>

            <div className="telem-row">
              <div className="telem-row-top">
                <span className="telem-lbl">ОЗУ</span>
                <span className="telem-val">
                  {telemSnap
                    ? `${formatTelemetryGb(telemSnap.ramUsedGb)} / ${formatTelemetryGb(telemSnap.ramTotalGb)} ГБ · ${ramPct}%`
                    : '—'}
                </span>
              </div>
              <div className="telem-bar-track" aria-hidden="true">
                <div
                  className={`telem-bar-fill ${getTelemetryUsageTone(ramPct)}`}
                  style={{ transform: `scaleX(${ramPct / 100})` }}
                />
              </div>
            </div>

            {telemSnap?.gpuAvailable && (
              <>
                <div className="telem-row">
                  <div className="telem-row-top">
                    <span className="telem-lbl" title={telemSnap.gpuName || 'GPU'}>
                      GPU
                      {telemSnap.gpuName && <span className="telem-sub"> · {telemSnap.gpuName}</span>}
                    </span>
                    <span className="telem-val">
                      {gpuTemp > 0 && (
                        <span className={`telem-temp-chip ${gpuTempTone}`}>{gpuTemp}°C</span>
                      )}
                      <span>{gpuPct}%</span>
                    </span>
                  </div>
                  <div className="telem-bar-track" aria-hidden="true">
                    <div
                      className={`telem-bar-fill ${getTelemetryUsageTone(gpuPct)}`}
                      style={{ transform: `scaleX(${gpuPct / 100})` }}
                    />
                  </div>
                </div>

                <div className="telem-row">
                  <div className="telem-row-top">
                    <span className="telem-lbl">Видеопамять (VRAM)</span>
                    <span className="telem-val">
                      {`${formatTelemetryGb(telemSnap.vramUsedGb)} / ${formatTelemetryGb(telemSnap.vramTotalGb)} ГБ · ${vramPct}%`}
                    </span>
                  </div>
                  <div className="telem-bar-track" aria-hidden="true">
                    <div
                      className={`telem-bar-fill ${getTelemetryUsageTone(vramPct)}`}
                      style={{ transform: `scaleX(${vramPct / 100})` }}
                    />
                  </div>
                </div>
              </>
            )}
          </div>

          {disks.length > 0 && (
            <div className="telem-disks">
              {disks.map((d, idx) => {
                const usedPct = Math.max(0, Math.min(100, Math.round(d.usedPct)));
                return (
                  <div key={d.mount || `disk-${idx}`} className="telem-disk-row">
                    <div className="telem-disk-top">
                      <span className="telem-disk-mount">
                        <DiskIcon className="ic12" />
                        <span>{d.mount}</span>
                        {d.name && <span className="telem-sub">{d.name}</span>}
                      </span>
                      <span className="telem-disk-val">
                        {`${formatTelemetryGb(d.freeGb)} / ${formatTelemetryGb(d.totalGb)} ГБ своб.`}
                      </span>
                    </div>
                    <div className="telem-bar-track sm" aria-hidden="true">
                      <div
                        className={`telem-bar-fill ${getTelemetryUsageTone(usedPct)}`}
                        style={{ transform: `scaleX(${usedPct / 100})` }}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {actionError && (
            <div className="ctx-err" role="alert">
              {actionError}
            </div>
          )}
        </div>
      </div>
    );
  }

  // 5. SILKY SCRUBBING WINDOW PREVIEW WITH SNAP / TOPMOST / MIDDLE-CLICK CLOSE
  if (req.kind === 'preview') {
    const visibleWins = previewWindows.slice(0, PREVIEW_MAX_VISIBLE_CARDS);
    const overflowCount = Math.max(0, previewWindows.length - PREVIEW_MAX_VISIBLE_CARDS);

    const closePreviewWindowAnimated = (hwnd: number) => {
      if (closingHwnds[hwnd]) return;
      const seq = renderSeqRef.current;
      const gKey = previewGroupKeyRef.current;
      setClosingHwnds((prev) => ({ ...prev, [hwnd]: true }));
      ipc('close_window_hwnd', { hwnd }).catch(() => {});

      const finish = () => {
        previewCloseTimersRef.current.delete(hwnd);
        if (
          activeKindRef.current !== 'preview' ||
          renderSeqRef.current !== seq ||
          previewGroupKeyRef.current !== gKey
        ) {
          return;
        }
        setClosingHwnds((prev) => {
          const next = { ...prev };
          delete next[hwnd];
          return next;
        });
        setPreviewWindows((prev) => {
          const nextWins = removePreviewWindow(prev, hwnd);
          if (nextWins.length === 0) {
            close();
          } else {
            ipc('resize_flyout', {
              width: computePreviewCardWidth(nextWins.length),
              height: PREVIEW_CARD_HEIGHT,
            }).catch(() => {});
          }
          return nextWins;
        });
      };

      const delay = reducedMotion ? 20 : PREVIEW_CARD_CLOSE_MS;
      const timerId = window.setTimeout(finish, delay);
      previewCloseTimersRef.current.set(hwnd, timerId);
    };

    return (
      <div
        className="flyout-root"
        data-space="personal"
        data-visible="true"
        onContextMenu={handleRootContextMenu}
        onMouseLeave={() => {
          ipc(PREVIEW_LEAVE_COMMAND).catch(() => {});
        }}
      >
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card preview-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
        >
          <div
            key={anim.scrubSeq}
            className="preview-track"
            style={{ ['--scrub-dir' as string]: anim.scrubDir } as React.CSSProperties}
          >
            {visibleWins.map((w, idx) => (
              <div
                key={w.hwnd ?? `win-${idx}`}
                className={`wp-item ${w.isActive ? 'active' : ''} ${closingHwnds[w.hwnd] ? 'closing' : ''}`}
                onClick={() =>
                  ipc('focus_window', { hwnd: w.hwnd, toggleMinimize: false }).catch(() => {})
                }
                onAuxClick={(e) => {
                  if (e.button === 1) {
                    e.preventDefault();
                    e.stopPropagation();
                    closePreviewWindowAnimated(w.hwnd);
                  }
                }}
              >
                <div className="wp-head">
                  <div className="wp-head-left">
                    {w.isActive && <span className="wp-active-dot" aria-hidden="true" />}
                    <span className="wp-title" title={w.title}>
                      {w.title}
                    </span>
                  </div>

                  <div className="wp-actions" onClick={(e) => e.stopPropagation()}>
                    <button
                      type="button"
                      className="wp-act-btn"
                      title="Прикрепить влево"
                      aria-label="Прикрепить влево"
                      onClick={() => {
                        snapWindow(w.hwnd, 'left').catch(() => {});
                        close();
                      }}
                    >
                      <SnapLeftIcon className="ic12" />
                    </button>
                    <button
                      type="button"
                      className="wp-act-btn"
                      title="Прикрепить вправо"
                      aria-label="Прикрепить вправо"
                      onClick={() => {
                        snapWindow(w.hwnd, 'right').catch(() => {});
                        close();
                      }}
                    >
                      <SnapRightIcon className="ic12" />
                    </button>
                    <button
                      type="button"
                      className={`wp-act-btn ${w.isTopmost ? 'pinned-top' : ''}`}
                      title={w.isTopmost ? 'Убрать поверх всех окон' : 'Поверх всех окон'}
                      aria-label="Поверх всех окон"
                      onClick={() => {
                        toggleWindowTopmost(w.hwnd)
                          .then((nextTop) => {
                            setPreviewWindows((prev) =>
                              prev.map((item) =>
                                item.hwnd === w.hwnd ? { ...item, isTopmost: nextTop } : item,
                              ),
                            );
                          })
                          .catch(() => {});
                      }}
                    >
                      <TopmostIcon className="ic12" active={Boolean(w.isTopmost)} />
                    </button>
                    <button
                      type="button"
                      className="wp-act-btn"
                      title={w.isMinimized ? 'Восстановить окно' : 'Развернуть окно'}
                      aria-label={w.isMinimized ? 'Восстановить окно' : 'Развернуть окно'}
                      onClick={() => {
                        snapWindow(w.hwnd, w.isMinimized ? 'restore' : 'maximize').catch(() => {});
                        close();
                      }}
                    >
                      {w.isMinimized ? (
                        <RestoreIcon className="ic12" />
                      ) : (
                        <MaximizeIcon className="ic12" />
                      )}
                    </button>
                    <button
                      type="button"
                      className="wp-act-btn danger"
                      title="Закрыть окно"
                      aria-label="Закрыть окно"
                      onClick={() => closePreviewWindowAnimated(w.hwnd)}
                    >
                      <XIcon10 />
                    </button>
                  </div>
                </div>

                <div className="wp-thumb">
                  {thumbs[w.hwnd] ? (
                    <img src={thumbs[w.hwnd]} alt="" className="wp-thumb-img" draggable={false} />
                  ) : (
                    <div className="wp-placeholder">
                      <div className="wp-placeholder-badge">
                        {previewIconUri ? (
                          <img src={previewIconUri} alt="" className="native-ic" draggable={false} />
                        ) : (
                          <FallbackAppIcon iconKey={w.exeName} size={28} />
                        )}
                      </div>
                      <span>{w.isMinimized ? 'Свёрнуто' : w.exeName}</span>
                    </div>
                  )}
                </div>
              </div>
            ))}

            {overflowCount > 0 && (
              <div className="wp-overflow-badge" title={`Ещё открытых окон: ${overflowCount}`}>
                +{overflowCount}
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  // 6. RIGHT-CLICK CONTEXT MENU ON TASKBAR APP (JUMP LIST + NEW WINDOW + TOPMOST + MINIMIZE OTHERS + KILL GROUP + KEYBOARD NAV)
  if (isAppContextKind(req.kind)) {
    const p = req.payload as AppContextPayload;
    const allPids = collectAppContextPids(p.windows);
    const primaryWin = p.windows.find((w) => w.isActive) ?? p.windows[0] ?? null;
    const launchTarget = p.target || (p.aumid ? `shell:AppsFolder\\${p.aumid}` : '');
    const showAdmin = canRunAsAdmin(p.target);
    const showLocation = canOpenFileLocation(p.target);
    const tasks = jumpList.filter((i) => i.kind === 'task');
    const recents = jumpList.filter((i) => i.kind === 'recent');
    const hasPinSection = Boolean((p.pinned && p.pinId) || launchTarget);

    const blocks: ContextBlock[] = [];
    const actions: ContextActionEntry[] = [];
    const pushAction = (entry: ContextActionEntry) => {
      const actionIdx = actions.length;
      actions.push(entry);
      blocks.push({ kind: 'action', entry, actionIdx });
    };

    if (tasks.length > 0) {
      blocks.push({ kind: 'subsec', key: 'sub-tasks', label: 'Задачи' });
      tasks.forEach((item, idx) => {
        pushAction({
          key: `task-${idx}-${item.title}`,
          label: item.title,
          title: item.title,
          icon: <ExternalWindowIcon />,
          onActivate: () =>
            runActionAndClose(
              ipc('launch_item', {
                target: item.target,
                args: item.args ?? null,
                asAdmin: false,
                closeStart: false,
              }),
            ),
        });
      });
    }

    if (recents.length > 0) {
      blocks.push({ kind: 'subsec', key: 'sub-recents', label: 'Недавние' });
      recents.forEach((item, idx) => {
        pushAction({
          key: `recent-${idx}-${item.target}`,
          label: item.title,
          title: item.target,
          icon: <ClockRecentIcon />,
          onActivate: () =>
            runActionAndClose(
              ipc('launch_item', {
                target: item.target,
                args: item.args ?? null,
                asAdmin: false,
                closeStart: false,
              }),
            ),
        });
      });
    }

    if (jumpList.length > 0 && (launchTarget || showAdmin || showLocation)) {
      blocks.push({ kind: 'sep', key: 'sep-jump' });
    }

    if (launchTarget) {
      pushAction({
        key: 'open-app',
        label: 'Открыть',
        icon: <PlaySmallIcon className="ic14" />,
        onActivate: () =>
          runActionAndClose(
            primaryWin
              ? ipc('focus_window', { hwnd: primaryWin.hwnd, toggleMinimize: false })
              : ipc('launch_item', {
                  target: launchTarget,
                  args: p.args ?? null,
                  asAdmin: false,
                  closeStart: false,
                }),
          ),
      });
      pushAction({
        key: 'new-window',
        label: 'Новое окно',
        icon: <ExternalWindowIcon />,
        onActivate: () =>
          runActionAndClose(launchNewWindow(launchTarget, p.args ?? null, false)),
      });
    }

    if (showAdmin) {
      pushAction({
        key: 'run-admin',
        label: 'Запустить от имени администратора',
        icon: <ShieldIcon />,
        onActivate: () =>
          runActionAndClose(
            ipc('launch_item', {
              target: p.target,
              args: p.args ?? null,
              asAdmin: true,
              closeStart: false,
            }),
          ),
      });
    }

    if (showLocation) {
      pushAction({
        key: 'open-location',
        label: 'Открыть расположение файла',
        icon: <FolderSvg className="ic14" />,
        onActivate: () => runActionAndClose(ipc('open_location', { path: p.target })),
      });
    }

    if (
      shouldShowAppContextActionSeparator({
        hasJumpList: jumpList.length > 0,
        hasLaunch: Boolean(launchTarget),
        hasAdmin: showAdmin,
        hasLocation: showLocation,
        hasPinSection,
      })
    ) {
      blocks.push({ kind: 'sep', key: 'sep-pin' });
    }

    if (p.pinned && p.pinId) {
      pushAction({
        key: 'unpin',
        label: 'Открепить с панели',
        icon: <UnpinIcon />,
        onActivate: () =>
          runActionAndClose(ipc('remove_pin', { space: 'taskbar', itemId: p.pinId })),
      });
    } else if (launchTarget) {
      pushAction({
        key: 'pin',
        label: 'Закрепить на панели',
        icon: <PinIcon />,
        onActivate: () =>
          runActionAndClose(
            ipc('add_pin', {
              space: 'taskbar',
              item: buildPinFromAppContext(p),
            }),
          ),
      });
    }

    if (p.windows.length > 0 && primaryWin) {
      if (actions.length > 0) {
        blocks.push({ kind: 'sep', key: 'sep-win-prod' });
      }
      pushAction({
        key: 'toggle-topmost',
        label: primaryWin.isTopmost ? 'Убрать поверх всех окон' : 'Поверх всех окон',
        icon: <TopmostIcon active={Boolean(primaryWin.isTopmost)} />,
        onActivate: () => runActionAndClose(toggleWindowTopmost(primaryWin.hwnd)),
      });
      pushAction({
        key: 'minimize-others',
        label: 'Свернуть остальные',
        icon: <MinimizeOthersIcon />,
        onActivate: () => runActionAndClose(minimizeOtherWindows(primaryWin.hwnd)),
      });

      blocks.push({ kind: 'sep', key: 'sep-close' });

      pushAction({
        key: 'close-windows',
        label:
          p.windows.length > 1
            ? `Закрыть все окна (${p.windows.length})`
            : 'Закрыть окно',
        icon: <CloseWindowIcon />,
        onActivate: () => {
          p.windows.forEach((w) => {
            ipc('close_window_hwnd', { hwnd: w.hwnd }).catch(() => {});
          });
          close();
        },
      });

      if (allPids.length > 0) {
        pushAction({
          key: 'kill-group',
          label: 'Завершить задачу',
          icon: <KillIcon />,
          danger: true,
          onActivate: () => runActionAndClose(ipc('kill_process_group', { pids: allPids })),
        });
      }
    }

    ctxActionsRef.current = actions;

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card ctx-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
          role="menu"
        >
          <div className="ctx-sec">{p.name}</div>

          <div className="ctx-scroll">
            {blocks.map((block) => {
              if (block.kind === 'subsec') {
                return (
                  <div key={block.key} className="ctx-subsec">
                    {block.label}
                  </div>
                );
              }
              if (block.kind === 'sep') {
                return <div key={block.key} className="ctx-sep" />;
              }
              const { entry, actionIdx } = block;
              return (
                <button
                  key={entry.key}
                  type="button"
                  role="menuitem"
                  className={`ctx-item ${entry.danger ? 'danger' : ''} ${ctxFocusIdx === actionIdx ? 'focused' : ''}`}
                  title={entry.title}
                  onMouseEnter={() => setCtxFocusIdx(actionIdx)}
                  onClick={entry.onActivate}
                >
                  {entry.icon}
                  <span className="ctx-item-label">{entry.label}</span>
                </button>
              );
            })}

            {actionError && (
              <div className="ctx-err" role="alert">
                {actionError}
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  // 7. RIGHT-CLICK ON EMPTY TASKBAR (WITH KEYBOARD NAVIGATION)
  if (isBarContextKind(req.kind)) {
    const p = req.payload as BarContextPayload;
    const isEdge = p.barMode === 'edge';
    const nextMode = isEdge ? 'dock' : 'edge';

    const barActions: ContextActionEntry[] = [
      {
        key: 'taskmgr',
        label: 'Диспетчер задач',
        icon: <TaskmgrIcon />,
        onActivate: () =>
          runActionAndClose(
            ipc('launch_item', {
              target: 'C:\\Windows\\System32\\Taskmgr.exe',
              args: null,
              asAdmin: false,
              closeStart: false,
            }),
          ),
      },
      {
        key: 'bar-mode',
        label: isEdge ? 'Переключить: Парящий док' : 'Переключить: Во всю ширину',
        icon: <LayoutBarIcon />,
        onActivate: () => runActionAndClose(ipc('set_bar_mode', { mode: nextMode })),
      },
      {
        key: 'settings',
        label: 'Настройки TestBar',
        icon: <SettingsGearIcon />,
        onActivate: () => {
          ipc('open_start_settings').catch(() => {});
        },
      },
      {
        key: 'restore-windows',
        label: 'Вернуть панель Windows',
        icon: <KillIcon />,
        danger: true,
        onActivate: () => {
          ipc('restore_windows_taskbar_and_exit').catch(() => {});
        },
      },
    ];

    ctxActionsRef.current = barActions;

    return (
      <div className="flyout-root" data-space="personal" data-visible="true" onContextMenu={handleRootContextMenu}>
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card ctx-card"
          data-phase={dataPhase}
          data-enter-ready={enterReady ? 'true' : 'false'}
          style={cardOriginStyle}
          role="menu"
        >
          <div className="ctx-sec">Панель задач</div>

          <div className="ctx-scroll">
            {barActions.slice(0, 3).map((entry, idx) => (
              <button
                key={entry.key}
                type="button"
                role="menuitem"
                className={`ctx-item ${ctxFocusIdx === idx ? 'focused' : ''}`}
                onMouseEnter={() => setCtxFocusIdx(idx)}
                onClick={entry.onActivate}
              >
                {entry.icon}
                <span className="ctx-item-label">{entry.label}</span>
              </button>
            ))}

            <div className="ctx-sep" />

            <button
              type="button"
              role="menuitem"
              className={`ctx-item danger ${ctxFocusIdx === 3 ? 'focused' : ''}`}
              onMouseEnter={() => setCtxFocusIdx(3)}
              onClick={barActions[3].onActivate}
            >
              {barActions[3].icon}
              <span className="ctx-item-label">{barActions[3].label}</span>
            </button>

            {actionError && (
              <div className="ctx-err" role="alert">
                {actionError}
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }

  return null;
};
