import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
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
  canOpenFileLocation,
  canRunAsAdmin,
  collectAppContextPids,
  computeCardOriginX,
  computeNextContextFocusIdx,
  computeTrayFlyoutHeight,
  formatIpcError,
  formatMediaTime,
  formatTrayHeaderLabel,
  getFlyoutExitMs,
  INITIAL_FLYOUT_ANIM_STATE,
  interpolateMediaPosition,
  isAppContextKind,
  isBarContextKind,
  OSD_AUTO_CLOSE_MS,
  partitionAudioDevices,
  PREVIEW_CARD_CLOSE_MS,
  PREVIEW_LEAVE_COMMAND,
  reduceFlyoutEntered,
  reduceFlyoutHideComplete,
  reduceFlyoutHideImmediate,
  reduceFlyoutHideStart,
  reduceFlyoutShow,
  removePreviewWindow,
  resolveTrayHideKey,
  shouldShowAppContextActionSeparator,
  syncPreviewWindowsFromGroups,
  TRAY_CARD_WIDTH,
  type FlyoutAnimState,
} from '../lib/flyoutLogic';
import {
  applyConfigUpdate,
  fetchIconsBatch,
  getAudioPeaks,
  hideTrayApp,
  iconCache,
  ipc,
  isCacheIconKey,
  launchNewWindow,
  minimizeOtherWindows,
  onEvent,
  seekMedia,
  setInputDevice,
  setSessionState,
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
  type OpenWindowInfo,
  type SessionAlert,
  type TestBarConfig,
  type TrayAppItem,
} from '../lib/ipc';
import {
  buildPinFromAppContext,
  computePreviewCardWidth,
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
  BellIcon,
  CheckIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  ClockRecentIcon,
  CloseWindowIcon,
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
  PauseIcon,
  PinIcon,
  PlaySmallIcon,
  PrevTrackIcon,
  RestoreIcon,
  SettingsGearIcon,
  ShieldIcon,
  SnapLeftIcon,
  SnapRightIcon,
  TaskmgrIcon,
  TopmostIcon,
  UnpinIcon,
  VolIcon,
  XIcon10,
} from './Icons';
import { ObsidianSlider } from './ObsidianSlider';

const MAX_THUMB_CACHE_ENTRIES = 48;

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

export const FlyoutView: React.FC = () => {
  const reducedMotion = useReducedMotion();

  const [config, setConfig] = useState<TestBarConfig | null>(null);
  const [anim, setAnim] = useState<FlyoutAnimState>(INITIAL_FLYOUT_ANIM_STATE);
  const [trayApps, setTrayApps] = useState<TrayAppItem[]>([]);
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
  const enterRaf1Ref = useRef<number | null>(null);
  const enterRaf2Ref = useRef<number | null>(null);
  const segReadyRafRef = useRef<number | null>(null);
  const previewCloseTimersRef = useRef<Map<number, number>>(new Map());
  const thumbCacheRef = useRef<Map<number, string>>(new Map());
  const lastTrayCtxRef = useRef<TrayAppItem | null>(null);

  const renderSeqRef = useRef<number>(0);
  const activeKindRef = useRef<string>('');
  const phaseRef = useRef<FlyoutAnimState['phase']>('closed');
  const currentGenRef = useRef<number>(0);
  const ackedGenRef = useRef<number>(-1);
  const lastResizedHeightRef = useRef<number>(0);
  const previewGroupKeyRef = useRef<string>('');
  const trayCtxItemRef = useRef<TrayAppItem | null>(null);
  const devMenuOpenRef = useRef<boolean>(false);
  const cardRef = useRef<HTMLDivElement | null>(null);
  const ctxActionsRef = useRef<ContextActionEntry[]>([]);
  const ctxFocusIdxRef = useRef<number>(-1);
  const seekSeqRef = useRef<number>(0);

  if (trayCtxItem) {
    lastTrayCtxRef.current = trayCtxItem;
  }

  renderSeqRef.current = anim.renderSeq;
  activeKindRef.current = anim.phase === 'closed' ? '' : (anim.req?.kind ?? '');
  phaseRef.current = anim.phase;
  previewGroupKeyRef.current = previewGroupKey;
  trayCtxItemRef.current = trayCtxItem;
  devMenuOpenRef.current = devMenuOpen;
  ctxFocusIdxRef.current = ctxFocusIdx;

  const trayCtxPresence = useAnimatedPresence(Boolean(trayCtxItem), 95);
  const devMenuPresence = useAnimatedPresence(devMenuOpen, 95);

  const cancelEnterRafs = useCallback(() => {
    if (enterRaf1Ref.current !== null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(enterRaf1Ref.current);
      enterRaf1Ref.current = null;
    }
    if (enterRaf2Ref.current !== null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(enterRaf2Ref.current);
      enterRaf2Ref.current = null;
    }
  }, []);

  const clearPreviewCloseTimers = useCallback(() => {
    previewCloseTimersRef.current.forEach((timerId) => window.clearTimeout(timerId));
    previewCloseTimersRef.current.clear();
  }, []);

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

    const subs = [
      onEvent<TestBarConfig>('config-updated', (next) =>
        setConfig((prev) => applyConfigUpdate(prev, next)),
      ),
      onEvent<FlyoutRequest>('flyout-show', (r) => {
        if (exitTimerRef.current !== null) {
          window.clearTimeout(exitTimerRef.current);
          exitTimerRef.current = null;
        }

        const prevKind = activeKindRef.current;
        const prevPhase = phaseRef.current;
        const isLive = prevPhase === 'entering' || prevPhase === 'open';
        const isInPlaceOsd = r.kind === 'osd' && prevKind === 'osd' && isLive;
        const isInPlacePreview = r.kind === 'preview' && prevKind === 'preview' && isLive;

        // Keep entering -> open double-rAF gate intact during continuous fast OSD scroll or preview scrub
        if (!((isInPlaceOsd || isInPlacePreview) && prevPhase === 'entering')) {
          cancelEnterRafs();
        }
        if (!isInPlacePreview) {
          clearPreviewCloseTimers();
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
        setTrayCtxItem(null);
        setTrayHover('');
        setDevMenuOpen(false);
        setActionError(null);
        setCtxFocusIdx(-1);
        setJumpList([]);
        seekSeqRef.current += 1;
        setScrubOverrideSec(null);
        setClosingHwnds({});

        if (r.kind === 'sound' || r.kind === 'quick') {
          setSegAnimReady(false);
          if (segReadyRafRef.current !== null && typeof cancelAnimationFrame === 'function') {
            cancelAnimationFrame(segReadyRafRef.current);
          }
          if (typeof requestAnimationFrame === 'function') {
            segReadyRafRef.current = requestAnimationFrame(() => {
              segReadyRafRef.current = null;
              setSegAnimReady(true);
            });
          } else {
            setSegAnimReady(true);
          }
        }

        if (r.kind !== 'preview') {
          previewGroupKeyRef.current = '';
          setPreviewGroupKey('');
          setPreviewIconUri(null);
          setPreviewWindows([]);
        }

        if (r.kind === 'tray') {
          ipc<TrayAppItem[]>('get_tray_apps')
            .then((items) => {
              if (renderSeqRef.current !== nextSeq || activeKindRef.current !== 'tray') return;
              setTrayApps(items);
              const nextH = computeTrayFlyoutHeight(items.length);
              lastResizedHeightRef.current = nextH;
              ipc('resize_flyout', {
                width: TRAY_CARD_WIDTH,
                height: nextH,
              }).catch(() => {});
            })
            .catch(() => {});
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
          const payload = r.payload as AppContextPayload;
          ipc<JumpListItem[]>('get_jump_list', {
            exePath: payload.target ?? '',
            aumid: payload.aumid ?? null,
          })
            .then((items) => {
              if (renderSeqRef.current === nextSeq) {
                setJumpList(items);
              }
            })
            .catch(() => {});
        }
      }),
      onEvent<FlyoutHideEvent | void>('flyout-hide', (ev) => {
        clearOsdTimer();
        cancelEnterRafs();
        clearPreviewCloseTimers();
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

        previewGroupKeyRef.current = '';
        setPreviewGroupKey('');
        setAnim((prev) => reduceFlyoutHideStart(prev));

        exitTimerRef.current = window.setTimeout(() => {
          exitTimerRef.current = null;
          setAnim((prev) => reduceFlyoutHideComplete(prev, hideSeq));
          setTrayCtxItem(null);
          setDevMenuOpen(false);
          setActionError(null);
          setClosingHwnds({});
          setCtxFocusIdx(-1);
          surfaceExitAck('flyout', hideGen).catch(() => {});
        }, exitMs);
      }),
      onEvent<FlyoutHideEvent | void>('flyout-hide-immediate', () => {
        clearOsdTimer();
        cancelEnterRafs();
        clearPreviewCloseTimers();
        if (exitTimerRef.current !== null) {
          window.clearTimeout(exitTimerRef.current);
          exitTimerRef.current = null;
        }
        previewGroupKeyRef.current = '';
        setPreviewGroupKey('');
        setTrayCtxItem(null);
        setDevMenuOpen(false);
        setActionError(null);
        setClosingHwnds({});
        setCtxFocusIdx(-1);
        setAnim((prev) => reduceFlyoutHideImmediate(prev));
      }),
      onEvent<AudioState>('audio-mixer-updated', (st) => {
        setAudio(st);
      }),
      onEvent<AudioMasterState>('audio-master-updated', (m) => {
        setAudio((prev) =>
          prev
            ? {
                ...prev,
                available: m.available,
                masterVolume: m.masterVolume,
                masterMuted: m.masterMuted,
                micMuted: m.micMuted,
              }
            : prev,
        );
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
      onEvent<AppWindowGroup[]>('windows-updated', (groups) => {
        if (activeKindRef.current !== 'preview') return;
        const key = previewGroupKeyRef.current;
        if (!key) return;
        const synced = syncPreviewWindowsFromGroups(key, groups);
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
      window.removeEventListener('keydown', onKey);
      clearOsdTimer();
      cancelEnterRafs();
      if (exitTimerRef.current !== null) window.clearTimeout(exitTimerRef.current);
      if (segReadyRafRef.current !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(segReadyRafRef.current);
      }
      clearPreviewCloseTimers();
      subs.forEach((p) => p.then((u) => u()));
    };
  }, [armOsdAutoClose, cacheThumb, cancelEnterRafs, clearOsdTimer, clearPreviewCloseTimers]);

  // Paint-Sync Step 1: Synchronous DOM commit ACK + exact pre-show height measurement (before ShowWindow)
  useLayoutEffect(() => {
    const req = anim.req;
    if (!req || anim.phase === 'closed' || anim.phase === 'closing') return;
    const gen = req.gen ?? 0;
    if (ackedGenRef.current === gen) return;
    ackedGenRef.current = gen;

    const shouldMeasure =
      isAppContextKind(req.kind) ||
      isBarContextKind(req.kind) ||
      req.kind === 'sound' ||
      req.kind === 'quick' ||
      req.kind === 'calendar';

    let measuredCardHeight: number | undefined;
    if (shouldMeasure && cardRef.current) {
      const h = Math.ceil(cardRef.current.scrollHeight);
      if (h >= 48) {
        measuredCardHeight = h;
        lastResizedHeightRef.current = h;
      }
    }

    surfaceCommitAck('flyout', gen, measuredCardHeight).catch(() => {});
  }, [anim.req, anim.phase, anim.renderSeq, anim.scrubSeq]);

  // Paint-Sync Step 2: Double-rAF promotion from 'entering' -> 'open' after ShowWindow wakes Chromium compositor
  useEffect(() => {
    if (anim.phase !== 'entering') return;
    cancelEnterRafs();

    if (typeof requestAnimationFrame !== 'function') {
      setAnim((prev) => reduceFlyoutEntered(prev));
      return;
    }

    enterRaf1Ref.current = requestAnimationFrame(() => {
      enterRaf1Ref.current = null;
      enterRaf2Ref.current = requestAnimationFrame(() => {
        enterRaf2Ref.current = null;
        setAnim((prev) => reduceFlyoutEntered(prev));
      });
    });

    return cancelEnterRafs;
  }, [anim.phase, anim.renderSeq, cancelEnterRafs]);

  // Poll WASAPI live peaks every 130ms STRICTLY while Quick Sound flyout is visibly open (0% idle cost)
  const isSoundVisiblyOpen =
    (anim.phase === 'open' || anim.phase === 'entering') &&
    (anim.req?.kind === 'sound' || anim.req?.kind === 'quick');

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
  const glassDepth = config?.glassDepth ?? 'obsidian';

  // Auto-fit flyout height for dynamic post-open content (Jump List, audio sessions, media strip, alerts)
  useEffect(() => {
    if (!req || isClosing || anim.phase === 'closed') return;
    const needsAutoHeight =
      isAppContextKind(req.kind) ||
      isBarContextKind(req.kind) ||
      req.kind === 'sound' ||
      req.kind === 'quick' ||
      req.kind === 'calendar';
    if (!needsAutoHeight) return;
    const el = cardRef.current;
    if (!el) return;

    const syncHeight = () => {
      const h = Math.ceil(el.scrollHeight);
      if (h >= 48 && Math.abs(h - lastResizedHeightRef.current) > 2) {
        lastResizedHeightRef.current = h;
        ipc('resize_flyout', { width: null, height: h }).catch(() => {});
      }
    };
    syncHeight();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(syncHeight);
    ro.observe(el);
    return () => ro.disconnect();
  }, [
    req?.kind,
    anim.renderSeq,
    anim.phase,
    isClosing,
    jumpList.length,
    actionError,
    audio?.sessions.length,
    audio?.devices.length,
    audio?.inputDevices?.length,
    devMenuOpen,
    devMenuPresence.mounted,
    media?.hasSession,
    media?.canSeek,
    alerts.length,
  ]);

  const today = useMemo(() => new Date(), [anim.renderSeq]);
  const calCells = useMemo(
    () => buildCalendarMonthGrid(calView.year, calView.month, today, selectedDate),
    [calView.year, calView.month, today, selectedDate],
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

    return (
      <div className="flyout-root" data-space="personal" data-glass={glassDepth} data-visible="true">
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card tray-card"
          data-phase={dataPhase}
          style={cardOriginStyle}
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
              <div className="tray-empty">Нет фоновых приложений</div>
            ) : (
              trayApps.map((t) => {
                const iconUri =
                  (t.iconKey ? trayIconMap[t.iconKey] || iconCache.get(t.iconKey) : undefined) ||
                  t.iconData ||
                  null;
                return (
                  <button
                    key={t.id}
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
      <div className="flyout-root" data-space="personal" data-glass={glassDepth} data-visible="true">
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className={`flyout-card osd-card ${osdState.muted ? 'muted' : ''}`}
          data-phase={dataPhase}
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

    const setMaster = (vol: number, muted: boolean) => {
      setActionError(null);
      setAudio((prev) => (prev ? { ...prev, masterVolume: vol, masterMuted: muted } : prev));
      ipc<AudioState>('set_master_volume', { volume: vol, muted })
        .then(setAudio)
        .catch((err) => setActionError(formatIpcError(err)));
    };

    const setSessionVolume = (s: AudioState['sessions'][number], vol: number) => {
      setActionError(null);
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
      setSessionState({
        appKey: s.id,
        volume: vol / 100,
        muted: nextMuted,
        pid: s.pid,
        pids: s.pids,
      })
        .then(setAudio)
        .catch((err) => setActionError(formatIpcError(err)));
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

    return (
      <div className="flyout-root" data-space="personal" data-glass={glassDepth} data-visible="true">
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card qs-card"
          data-phase={dataPhase}
          style={cardOriginStyle}
          onClick={() => {
            if (devMenuOpen) setDevMenuOpen(false);
          }}
        >
          <div className="sec-row" style={{ marginBottom: 9 }}>
            <span className="sec-title">Звук и сеть</span>
            <button
              type="button"
              className={`qs-net-pill ${!netConnected ? 'offline' : ''}`}
              title="Открыть параметры сети Windows"
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
              <span>
                {net
                  ? net.connected
                    ? net.speedLabel
                      ? `${net.kind} · ${net.speedLabel}`
                      : net.kind
                    : 'Нет сети'
                  : 'Сеть'}
              </span>
            </button>
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
                    title="Предыдущий трек"
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
                    className="m-btn play"
                    title={media.isPlaying ? 'Пауза' : 'Воспроизвести'}
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
                    title="Следующий трек"
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
                  {primaryDevs.map((d) => (
                    <button
                      key={d.id}
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
                    title={`Все устройства вывода (${allDevs.length})`}
                    aria-label="Все устройства вывода"
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
                  {allDevs.map((d) => (
                    <button
                      key={d.id}
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

          <div className="qs-mic-row">
            <button
              type="button"
              className={`qs-mic-btn ${audio?.micMuted ? 'muted' : ''}`}
              title={audio?.micMuted ? 'Включить микрофон' : 'Отключить микрофон'}
              onClick={() => {
                setActionError(null);
                const nextMuted = !(audio?.micMuted ?? false);
                setAudio((prev) => (prev ? { ...prev, micMuted: nextMuted } : prev));
                ipc<AudioState>('set_mic_muted', { muted: nextMuted })
                  .then(setAudio)
                  .catch((err) => setActionError(formatIpcError(err)));
              }}
            >
              <MicIcon className="ic14" muted={audio?.micMuted ?? false} />
              <span>{audio?.micMuted ? 'Выкл' : 'Микрофон'}</span>
            </button>
            {inputDevices.length > 0 && (
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
                {inputDevices.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.shortName || d.name}
                  </option>
                ))}
              </select>
            )}
          </div>

          <div className={`mix-row qs-master-row ${audio?.masterMuted ? 'muted' : ''}`}>
            <button
              type="button"
              className="mix-mute"
              title={audio?.masterMuted ? 'Включить звук' : 'Выключить звук'}
              onClick={() => setMaster(audio?.masterVolume ?? 0, !(audio?.masterMuted ?? false))}
            >
              <VolIcon muted={audio?.masterMuted} level={audio?.masterVolume} />
            </button>
            <span className="mix-name" style={{ fontWeight: 600 }}>Общий звук</span>
            <ObsidianSlider
              value={audio?.masterVolume ?? 0}
              muted={audio?.masterMuted ?? false}
              peak={peaks?.masterPeak ?? audio?.masterPeak}
              magneticDetents={[0, 50, 100]}
              ariaLabel="Общий звук"
              onChange={(v) => setMaster(v, false)}
            />
            <span className="mix-val">
              {audio?.masterMuted ? '0%' : `${audio?.masterVolume ?? 0}%`}
            </span>
          </div>

          <div className="mixer-list qs-mixer-scroll">
            {sessions.length === 0 ? (
              <div className="qs-empty">Нет активных аудиопотоков</div>
            ) : (
              sessions.map((s) => {
                const iconUri = s.iconKey
                  ? sessionIconMap[s.iconKey] || iconCache.get(s.iconKey) || null
                  : null;
                const livePeak = sessionPeakMap.get(s.id) ?? s.peak ?? 0;
                const isActiveAudio = s.isActive || livePeak > 0.02;
                return (
                  <div
                    key={s.id}
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
                      magneticDetents={[0, 50, 100]}
                      ariaLabel={`Громкость ${s.name}`}
                      onChange={(v) => setSessionVolume(s, v)}
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
      <div className="flyout-root" data-space="personal" data-glass={glassDepth} data-visible="true">
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card cal-card"
          data-phase={dataPhase}
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
                {alerts.slice(0, 4).map((a) => (
                  <div key={a.id} className="cal-alert-row">
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
        data-glass={glassDepth}
        data-visible="true"
        onMouseLeave={() => {
          ipc(PREVIEW_LEAVE_COMMAND).catch(() => {});
        }}
      >
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card preview-card"
          data-phase={dataPhase}
          style={cardOriginStyle}
        >
          <div
            key={anim.scrubSeq}
            className="preview-track"
            style={{ ['--scrub-dir' as string]: anim.scrubDir } as React.CSSProperties}
          >
            {visibleWins.map((w) => (
              <div
                key={w.hwnd}
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
                    <img src={thumbs[w.hwnd]} alt="" draggable={false} />
                  ) : (
                    <div className="wp-placeholder">
                      {previewIconUri ? (
                        <img src={previewIconUri} alt="" className="native-ic" draggable={false} />
                      ) : (
                        <FallbackAppIcon iconKey={w.exeName} size={22} />
                      )}
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
        key: 'new-window',
        label: 'Открыть новое окно',
        icon: <ExternalWindowIcon />,
        onActivate: () =>
          runActionAndClose(launchNewWindow(launchTarget, p.args ?? null, false)),
      });
    }

    if (showAdmin) {
      pushAction({
        key: 'run-admin',
        label: 'Запустить от администратора',
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
      <div className="flyout-root" data-space="personal" data-glass={glassDepth} data-visible="true">
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card ctx-card"
          data-phase={dataPhase}
          style={cardOriginStyle}
          role="menu"
        >
          <div className="ctx-sec">{p.name}</div>

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
      <div className="flyout-root" data-space="personal" data-glass={glassDepth} data-visible="true">
        <div
          key={anim.renderSeq}
          ref={cardRef}
          className="flyout-card ctx-card"
          data-phase={dataPhase}
          style={cardOriginStyle}
          role="menu"
        >
          <div className="ctx-sec">Панель задач</div>

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
    );
  }

  return null;
};
