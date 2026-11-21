import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { canOpenFileLocation, canRunAsAdmin, formatIpcError } from '../lib/flyoutLogic';
import {
  animateSurfaceIn,
  animateSurfaceOut,
  applyConfigUpdate,
  createSpace,
  deleteSpace,
  emptyRecycleBin,
  exportConfigDialog,
  fetchIconsBatch,
  findMatchingGroupForPin,
  fsOpenInExplorer,
  getPeripheralBatteries,
  getWeather,
  iconCache,
  importConfigDialog,
  ipc,
  isCacheIconKey,
  isTauri,
  normalizeStartStatePayload,
  onEvent,
  openScreenSnip,
  pasteClipboardEntry,
  pickCustomIconImage,
  recordFrecency,
  refreshIndexes,
  refreshPeripheralBatteries,
  reorderSpaces,
  restoreAllTrayApps,
  restoreTrayApp,
  setAppearanceSettings,
  setSpaceWidgets,
  surfaceExitAck,
  toggleStart,
  updatePin,
  updateSpace,
  type AppWindowGroup,
  type AudioMasterState,
  type AudioState,
  type ClipboardEntry,
  type IndexedApp,
  type IndexedFile,
  type MediaState,
  type PeripheralBattery,
  type PinItem,
  type SecondaryTaskbarMode,
  type SpaceLayoutKind,
  type SpaceWidgetId,
  type StartShownEvent,
  type StartStateEvent,
  type SteamGame,
  type TelemetryState,
  type TestBarConfig,
  type WeatherSnapshot,
} from '../lib/ipc';
import {
  buildSearchCorpus,
  isPrintableSearchTrigger,
  rankSearchResults,
  resolveSearchLaunchPayload,
  resolveSearchSecondaryAction,
  type RankedSearchResult,
  type SearchCorpusItem,
} from '../lib/searchLogic';
import {
  applyOptimisticPinReorder,
  buildGameComboLaunchPayload,
  completeStartClosing,
  findMatchingInstalledAppForPin,
  getStartChassisAnimClass,
  INITIAL_START_ANIM_STATE,
  mergeAudioStateRespectingLocks,
  normalizeRgbTriplet,
  reducePowerClick,
  reduceStartState,
  resolveConfiguredSpaces,
  resolveCustomColorRgb,
  resolvePinAction,
  settleStartOpen,
  START_ENTER_ANIM_MS,
  START_EXIT_ANIM_MS,
  START_OPACITY_ANIM_MS,
  TILE_COLOR_PALETTE,
  type ArmedPowerKind,
  type PowerActionKind,
  type SpaceTemplateId,
  type StartAnimState,
} from '../lib/startLogic';
import { useAnimatedPresence } from '../lib/useAnimatedPresence';
import { useReducedMotion } from '../lib/useReducedMotion';
import {
  ArrowLeftIcon,
  CopyIcon,
  EditIcon,
  ExternalWindowIcon,
  FallbackAppIcon,
  FolderSvg,
  KillIcon,
  PinIcon,
  renderCustomTileIcon,
  ShieldIcon,
  TerminalIcon,
  TILE_ICON_CATALOG,
  UnpinIcon,
} from './Icons';
import { StartCommandPalette } from './start/StartCommandPalette';
import { StartHeader } from './start/StartHeader';
import { StartPinDrawer } from './start/StartPinDrawer';
import { StartSettingsOverlay } from './start/StartSettingsOverlay';
import { StartSpaces } from './start/StartSpaces';

const EMPTY_RANKED_SEARCH: RankedSearchResult = {
  items: [],
  swappedHint: null,
  totalMatches: 0,
  matchIndices: {},
};

const BUILTIN_SPACE_AURA_RGB: Record<string, string> = {
  personal: '129, 140, 248',
  work: '56, 189, 248',
  studio: '52, 211, 153',
  games: '251, 113, 133',
};

const ALT_ACTION_OPTIONS: ReadonlyArray<{ value: string; label: string }> = [
  { value: '', label: 'Нет' },
  { value: 'Новое окно', label: 'Новое окно' },
  { value: 'Приватное окно', label: 'Приватное окно' },
  { value: 'От имени администратора', label: 'От имени администратора' },
  { value: 'Открыть в терминале', label: 'Открыть в терминале' },
  { value: 'Скопировать путь', label: 'Скопировать путь' },
];

interface TileCtxState {
  x: number;
  y: number;
  item: PinItem;
  spaceKey: string;
}

interface TileEditState {
  x: number;
  y: number;
  item: PinItem;
  spaceKey: string;
}

interface TileCtxAction {
  id: string;
  label: string;
  danger?: boolean;
  icon: React.ReactNode;
  run: () => void;
}

function clampPopoverPos(x: number, y: number, width: number, estHeight: number): { x: number; y: number } {
  const vw = typeof window !== 'undefined' ? window.innerWidth : 1172;
  const vh = typeof window !== 'undefined' ? window.innerHeight : 712;
  return {
    x: Math.max(16, Math.min(x, vw - width - 16)),
    y: Math.max(16, Math.min(y, vh - estHeight - 16)),
  };
}

function findExistingTaskbarPin(item: PinItem, taskbarPins: PinItem[]): PinItem | null {
  const normTarget = (item.target ?? '').trim().replace(/\//g, '\\').toLowerCase();
  const normAumid = (item.matchAumid ?? '').trim().toLowerCase();
  return (
    taskbarPins.find((tp) => {
      if (normAumid && tp.matchAumid && tp.matchAumid.trim().toLowerCase() === normAumid) {
        return true;
      }
      if (normTarget && tp.target.trim().replace(/\//g, '\\').toLowerCase() === normTarget) {
        return true;
      }
      return false;
    }) ?? null
  );
}

export const StartView: React.FC = () => {
  const [config, setConfig] = useState<TestBarConfig | null>(null);
  const [groups, setGroups] = useState<AppWindowGroup[]>([]);
  const [apps, setApps] = useState<IndexedApp[]>([]);
  const [files, setFiles] = useState<IndexedFile[]>([]);
  const [games, setGames] = useState<SteamGame[]>([]);
  const [audio, setAudio] = useState<AudioState | null>(null);
  const [media, setMedia] = useState<MediaState | null>(null);
  const [telem, setTelem] = useState<TelemetryState | null>(null);
  const [clips, setClips] = useState<ClipboardEntry[]>([]);
  const [periphs, setPeriphs] = useState<PeripheralBattery[]>([]);
  const [periphsRefreshing, setPeriphsRefreshing] = useState(false);
  const [weather, setWeather] = useState<WeatherSnapshot | null>(null);
  const [weatherRefreshing, setWeatherRefreshing] = useState(false);
  const [iconMap, setIconMap] = useState<Record<string, string>>({});

  // Non-remounting Paint-Sync animation state (hidden at 0 opacity at boot inside Tauri)
  const [anim, setAnim] = useState<StartAnimState>(() =>
    isTauri()
      ? INITIAL_START_ANIM_STATE
      : { phase: 'open', open: true, closing: false, openSeq: 1, gen: 0 },
  );

  const [space, setSpace] = useState<string>('personal');
  const [spaceDir, setSpaceDir] = useState<number>(0);
  const [rapidSwitch, setRapidSwitch] = useState<boolean>(false);
  const [editMode, setEditMode] = useState(false);
  const [shiftHeld, setShiftHeld] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [periphOpen, setPeriphOpen] = useState(false);
  const [powerOpen, setPowerOpen] = useState(false);
  const [armedPower, setArmedPower] = useState<ArmedPowerKind>(null);
  const [addingSpace, setAddingSpace] = useState(false);
  const [managingSpaceId, setManagingSpaceId] = useState<string | null>(null);
  const [widgetBuilderOpen, setWidgetBuilderOpen] = useState(false);
  const [activeExplorerPath, setActiveExplorerPath] = useState<string | null>(null);

  // Toast with smooth enter/exit presence
  const [toastMsg, setToastMsg] = useState<string | null>(null);
  const lastToastRef = useRef<string>('');
  if (toastMsg) lastToastRef.current = toastMsg;
  const toastPresence = useAnimatedPresence(Boolean(toastMsg), 110);

  // Tile Context Menu (rendered outside .start-chassis!) & Keyboard focus
  const [tileCtx, setTileCtx] = useState<TileCtxState | null>(null);
  const [tileCtxIdx, setTileCtxIdx] = useState<number>(0);
  const lastTileCtxRef = useRef<TileCtxState | null>(null);
  if (tileCtx) lastTileCtxRef.current = tileCtx;
  const tileCtxPresence = useAnimatedPresence(Boolean(tileCtx), 95);

  // Tile Edit Popover (rendered outside .start-chassis!)
  const [tileEdit, setTileEdit] = useState<TileEditState | null>(null);
  const lastTileEditRef = useRef<TileEditState | null>(null);
  if (tileEdit) lastTileEditRef.current = tileEdit;
  const tileEditPresence = useAnimatedPresence(Boolean(tileEdit), 95);
  const [editPinTitle, setEditPinTitle] = useState('');
  const [editPinArgs, setEditPinArgs] = useState('');
  const [editPinAltAction, setEditPinAltAction] = useState('');
  const [editPinAltLabel, setEditPinAltLabel] = useState('');
  const [editCustomColor, setEditCustomColor] = useState<string | null>(null);
  const [editCustomIcon, setEditCustomIcon] = useState<string | null>(null);
  const [editIconData, setEditIconData] = useState<string | null>(null);
  const [pickingCustomImg, setPickingCustomImg] = useState(false);
  const [savingPinEdit, setSavingPinEdit] = useState(false);
  const editTitleInputRef = useRef<HTMLInputElement>(null);

  // Command Palette
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchTrigger, setSearchTrigger] = useState<'keyboard' | 'click'>('keyboard');
  const [query, setQuery] = useState('');
  const [cmdIdx, setCmdIdx] = useState(0);
  const cmdInputRef = useRef<HTMLInputElement>(null);

  // Refs for lifecycle, WAAPI spring & drag-lock audio coordination
  const reducedMotion = useReducedMotion();
  const reducedMotionRef = useRef<boolean>(reducedMotion);
  reducedMotionRef.current = reducedMotion;

  const chassisRef = useRef<HTMLElement>(null);
  const activeAnimsRef = useRef<Animation[]>([]);
  const openArmCleanupRef = useRef<(() => void) | null>(null);
  const pendingShowLaunchRef = useRef<{ gen: number; launch: () => void } | null>(null);
  const deferredHydrateTimerRef = useRef<number | null>(null);
  const audioDragLocksRef = useRef<Map<string, number>>(new Map());

  const mountedRef = useRef<boolean>(true);
  const latestGenRef = useRef<number>(0);
  const closeCleanupTimerRef = useRef<number | null>(null);
  const toastTimerRef = useRef<number | null>(null);
  const lastSpaceSwitchTsRef = useRef<number>(0);
  const lastWeatherFetchMsRef = useRef<number>(0);
  const configRef = useRef<TestBarConfig | null>(null);
  const spaceRef = useRef<string>(space);
  const secondaryHydratedRef = useRef<boolean>(false);
  const resolvedTargetsRef = useRef<Set<string>>(new Set());
  const startActiveRef = useRef<boolean>(anim.phase !== 'hidden');
  startActiveRef.current = anim.phase !== 'hidden';
  const hasPendingHiddenRef = useRef<boolean>(false);
  const pendingHiddenRef = useRef<{
    groups?: AppWindowGroup[];
    audio?: AudioState;
    audioMaster?: AudioMasterState;
    media?: MediaState;
    telem?: TelemetryState;
    clips?: ClipboardEntry[];
    periphs?: PeripheralBattery[];
    weather?: WeatherSnapshot;
  }>({});

  const flushPendingHiddenState = useCallback(() => {
    if (!hasPendingHiddenRef.current) return;
    hasPendingHiddenRef.current = false;
    const pending = pendingHiddenRef.current;
    pendingHiddenRef.current = {};

    if (pending.groups !== undefined) setGroups(pending.groups);
    if (pending.audio !== undefined) {
      const nextAudio = pending.audio;
      setAudio((prev) =>
        mergeAudioStateRespectingLocks(prev, nextAudio, audioDragLocksRef.current, Date.now()),
      );
    } else if (pending.audioMaster !== undefined) {
      const master = pending.audioMaster;
      setAudio((prev) => {
        if (!prev) return prev;
        const masterLock = audioDragLocksRef.current.get('__master__');
        const keepMasterVol = masterLock !== undefined && masterLock > Date.now();
        return {
          ...prev,
          available: master.available,
          masterVolume: keepMasterVol ? prev.masterVolume : master.masterVolume,
          masterMuted: master.masterMuted,
          micMuted: master.micMuted,
        };
      });
    }
    if (pending.media !== undefined) setMedia(pending.media);
    if (pending.telem !== undefined) setTelem(pending.telem);
    if (pending.clips !== undefined) setClips(pending.clips);
    if (pending.periphs !== undefined) setPeriphs(pending.periphs);
    if (pending.weather !== undefined) setWeather(pending.weather);
  }, []);

  const cancelChassisAnims = useCallback(() => {
    for (const a of activeAnimsRef.current) {
      try {
        a.cancel();
      } catch {
        // Ignore already-finished animations
      }
    }
    activeAnimsRef.current = [];
    if (chassisRef.current) {
      chassisRef.current.style.pointerEvents = '';
    }
  }, []);

  const cancelPendingOpenArm = useCallback(() => {
    pendingShowLaunchRef.current = null;
    if (openArmCleanupRef.current) {
      openArmCleanupRef.current();
      openArmCleanupRef.current = null;
    }
  }, []);

  const handleAudioDragStateChange = useCallback((sliderKey: string, dragging: boolean) => {
    if (dragging) {
      audioDragLocksRef.current.set(sliderKey, Number.POSITIVE_INFINITY);
    } else {
      audioDragLocksRef.current.set(sliderKey, Date.now() + 280);
    }
  }, []);

  const handleSetAudioFromChild = useCallback((next: AudioState, fromServer = false) => {
    if (fromServer) {
      setAudio((prev) =>
        mergeAudioStateRespectingLocks(prev, next, audioDragLocksRef.current, Date.now()),
      );
    } else {
      setAudio(next);
    }
  }, []);

  // Release entrance WAAPI fill:forwards strictly post-DOM-commit once [data-phase="open"] is painted
  useLayoutEffect(() => {
    if (anim.phase === 'open') {
      cancelChassisAnims();
    }
  }, [anim.phase, cancelChassisAnims]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      cancelPendingOpenArm();
      cancelChassisAnims();
    };
  }, [cancelPendingOpenArm, cancelChassisAnims]);

  configRef.current = config;
  spaceRef.current = space;

  const resolvedSpaces = useMemo(() => resolveConfiguredSpaces(config), [config]);
  const activeSpace = useMemo(
    () => resolvedSpaces.find((s) => s.id === space) ?? resolvedSpaces[0],
    [resolvedSpaces, space],
  );

  // 2-layer ambient aura crossfade state
  const spaceTheme = ['personal', 'work', 'studio', 'games'].includes(space) ? space : 'custom';
  const targetAuraRgb =
    normalizeRgbTriplet(activeSpace?.accentRgb) ??
    BUILTIN_SPACE_AURA_RGB[space] ??
    '167, 139, 250';

  const [auraSlots, setAuraSlots] = useState<[string, string]>([targetAuraRgb, targetAuraRgb]);
  const [activeAuraSlot, setActiveAuraSlot] = useState<0 | 1>(0);
  const lastAuraRgbRef = useRef<string>(targetAuraRgb);

  useEffect(() => {
    if (targetAuraRgb === lastAuraRgbRef.current) return;
    lastAuraRgbRef.current = targetAuraRgb;
    setActiveAuraSlot((prevSlot) => {
      const nextSlot: 0 | 1 = prevSlot === 0 ? 1 : 0;
      setAuraSlots((prev) => (nextSlot === 0 ? [targetAuraRgb, prev[1]] : [prev[0], targetAuraRgb]));
      return nextSlot;
    });
  }, [targetAuraRgb]);

  const showToast = useCallback((msg: string) => {
    setToastMsg(msg);
    if (toastTimerRef.current !== null) {
      window.clearTimeout(toastTimerRef.current);
    }
    const durationMs = msg.length > 36 ? 2600 : 1650;
    toastTimerRef.current = window.setTimeout(() => {
      toastTimerRef.current = null;
      setToastMsg(null);
    }, durationMs);
  }, []);

  const handleApplyConfig = useCallback((next: TestBarConfig) => {
    setConfig((prev) => applyConfigUpdate(prev, next));
  }, []);

  const handleOptimisticReorder = useCallback(
    (spaceKey: string, fromIdx: number, toIdx: number) => {
      setConfig((prev) => applyOptimisticPinReorder(prev, spaceKey, fromIdx, toIdx) ?? prev);
    },
    [],
  );

  const resetTransientOverlays = useCallback(() => {
    setSearchOpen(false);
    setQuery('');
    setPowerOpen(false);
    setArmedPower(null);
    setPeriphOpen(false);
    setAddingSpace(false);
    setManagingSpaceId(null);
    setWidgetBuilderOpen(false);
    setActiveExplorerPath(null);
    setSettingsOpen(false);
    setDrawerOpen(false);
    setTileCtx(null);
    setTileEdit(null);
    setShiftHeld(false);
  }, []);

  // Core startup hydration (lightweight, zero thundering herd)
  const hydrateCoreData = useCallback(() => {
    ipc<TestBarConfig>('get_config')
      .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
      .catch(() => {});
    ipc<AppWindowGroup[]>('get_open_windows').then(setGroups).catch(() => {});
    ipc<IndexedApp[]>('get_installed_apps').then(setApps).catch(() => {});
  }, []);

  // Deferred secondary widget hydration (runs on first Start open or calm idle)
  const hydrateSecondaryData = useCallback(() => {
    if (secondaryHydratedRef.current) return;
    secondaryHydratedRef.current = true;
    ipc<IndexedFile[]>('get_indexed_files').then(setFiles).catch(() => {});
    ipc<SteamGame[]>('get_steam_games').then(setGames).catch(() => {});
    ipc<ClipboardEntry[]>('get_clipboard_history').then(setClips).catch(() => {});
    ipc<MediaState>('get_media_state').then(setMedia).catch(() => {});
    ipc<AudioState>('get_audio_state').then(setAudio).catch(() => {});
    ipc<TelemetryState>('get_telemetry').then(setTelem).catch(() => {});
    getPeripheralBatteries()
      .then(setPeriphs)
      .catch(() => ipc<PeripheralBattery[]>('get_peripherals').then(setPeriphs).catch(() => {}));
  }, []);

  useEffect(() => {
    hydrateCoreData();
    const idleTimer = window.setTimeout(() => {
      hydrateSecondaryData();
    }, 1400);

    const subs = [
      onEvent<TestBarConfig>('config-updated', (next) =>
        setConfig((prev) => applyConfigUpdate(prev, next)),
      ),
      onEvent<AppWindowGroup[]>('windows-updated', (next) => {
        if (!startActiveRef.current) {
          pendingHiddenRef.current.groups = next;
          hasPendingHiddenRef.current = true;
          return;
        }
        setGroups(next);
      }),
      onEvent<IndexedApp[]>('apps-indexed', setApps),
      onEvent<IndexedFile[]>('files-indexed', setFiles),
      onEvent<SteamGame[]>('games-indexed', setGames),
      onEvent<AudioState>('audio-mixer-updated', (incoming) => {
        if (!startActiveRef.current) {
          pendingHiddenRef.current.audio = incoming;
          pendingHiddenRef.current.audioMaster = undefined;
          hasPendingHiddenRef.current = true;
          return;
        }
        setAudio((prev) =>
          mergeAudioStateRespectingLocks(prev, incoming, audioDragLocksRef.current, Date.now()),
        );
      }),
      onEvent<AudioMasterState>('audio-master-updated', (master) => {
        if (!startActiveRef.current) {
          if (pendingHiddenRef.current.audio) {
            pendingHiddenRef.current.audio = {
              ...pendingHiddenRef.current.audio,
              available: master.available,
              masterVolume: master.masterVolume,
              masterMuted: master.masterMuted,
              micMuted: master.micMuted,
            };
          } else {
            pendingHiddenRef.current.audioMaster = master;
          }
          hasPendingHiddenRef.current = true;
          return;
        }
        setAudio((prev) => {
          if (!prev) return prev;
          const masterLock = audioDragLocksRef.current.get('__master__');
          const keepMasterVol = masterLock !== undefined && masterLock > Date.now();
          return {
            ...prev,
            available: master.available,
            masterVolume: keepMasterVol ? prev.masterVolume : master.masterVolume,
            masterMuted: master.masterMuted,
            micMuted: master.micMuted,
          };
        });
      }),
      onEvent<MediaState>('media-updated', (next) => {
        if (!startActiveRef.current) {
          pendingHiddenRef.current.media = next;
          hasPendingHiddenRef.current = true;
          return;
        }
        setMedia(next);
      }),
      onEvent<TelemetryState>('telemetry-updated', (next) => {
        if (!startActiveRef.current) {
          pendingHiddenRef.current.telem = next;
          hasPendingHiddenRef.current = true;
          return;
        }
        setTelem(next);
      }),
      onEvent<ClipboardEntry[]>('clipboard-updated', (next) => {
        if (!startActiveRef.current) {
          pendingHiddenRef.current.clips = next;
          hasPendingHiddenRef.current = true;
          return;
        }
        setClips(next);
      }),
      onEvent<PeripheralBattery[]>('peripherals-updated', (next) => {
        if (!startActiveRef.current) {
          pendingHiddenRef.current.periphs = next;
          hasPendingHiddenRef.current = true;
          return;
        }
        setPeriphs(next);
      }),
      onEvent<WeatherSnapshot>('weather-updated', (next) => {
        if (!startActiveRef.current) {
          pendingHiddenRef.current.weather = next;
          hasPendingHiddenRef.current = true;
          return;
        }
        lastWeatherFetchMsRef.current = Date.now();
        setWeather(next);
      }),
      onEvent<StartShownEvent>('start-shown', (ev) => {
        const pending = pendingShowLaunchRef.current;
        if (pending && (ev?.gen === undefined || ev.gen === pending.gen)) {
          pending.launch();
        }
      }),
      onEvent<boolean | StartStateEvent>('start-state', (raw) => {
        const ev = normalizeStartStatePayload(raw);
        latestGenRef.current = ev.gen;

        cancelPendingOpenArm();
        if (closeCleanupTimerRef.current !== null) {
          window.clearTimeout(closeCleanupTimerRef.current);
          closeCleanupTimerRef.current = null;
        }
        if (deferredHydrateTimerRef.current !== null) {
          window.clearTimeout(deferredHydrateTimerRef.current);
          deferredHydrateTimerRef.current = null;
        }

        if (ev.open) {
          startActiveRef.current = true;
          flushPendingHiddenState();
          cancelChassisAnims();
          setShiftHeld((prev) => (prev ? false : prev));
          const remember = configRef.current?.startRememberSpace ?? false;
          const configured = resolveConfiguredSpaces(configRef.current);
          const defaultSpaceId = configured[0]?.id ?? 'personal';
          if (!remember && spaceRef.current !== defaultSpaceId) {
            setSpaceDir(0);
            setRapidSwitch(false);
            setSpace(defaultSpaceId);
            setWidgetBuilderOpen(false);
            setActiveExplorerPath(null);
          }
          setAnim((prev) => reduceStartState(prev, ev));

          const openGen = ev.gen;
          let launched = false;
          let rafId = 0;
          let fallbackTimer: number | null = null;

          const cleanupWaiters = () => {
            if (rafId) window.cancelAnimationFrame(rafId);
            if (fallbackTimer !== null) {
              window.clearTimeout(fallbackTimer);
              fallbackTimer = null;
            }
            if (typeof document !== 'undefined') {
              document.removeEventListener('visibilitychange', onVisChange);
            }
          };

          const launchSpring = () => {
            if (launched) return;
            launched = true;
            pendingShowLaunchRef.current = null;
            if (fallbackTimer !== null) {
              window.clearTimeout(fallbackTimer);
              fallbackTimer = null;
            }
            if (typeof document !== 'undefined') {
              document.removeEventListener('visibilitychange', onVisChange);
            }

            // Single rAF after `start-shown` (compositor stays permanently warm with
            // CalculateNativeWinOcclusion disabled), guaranteeing post-SW_SHOW & post-commit
            // while cutting perceived Win-key open latency in half!
            rafId = window.requestAnimationFrame(() => {
              rafId = 0;
              openArmCleanupRef.current = null;
              if (!mountedRef.current || latestGenRef.current !== openGen) return;

              animateSurfaceIn('start', openGen, {
                durationMs: START_ENTER_ANIM_MS,
                offsetY: 20,
                fromScale: 0.96,
              }).catch(() => {});

              const el = chassisRef.current;
              if (reducedMotionRef.current || !el || typeof el.animate !== 'function') {
                setAnim((prev) => settleStartOpen(prev, openGen));
                hydrateSecondaryData();
                return;
              }

              cancelChassisAnims();
              // Allow clicks mid-flight while WAAPI drives transform & opacity
              el.style.pointerEvents = 'auto';

              const transformAnim = el.animate(
                [
                  { transform: 'translate3d(0, 20px, 0) scale(0.96)' },
                  { transform: 'translate3d(0, 0, 0) scale(1)' },
                ],
                {
                  duration: START_ENTER_ANIM_MS,
                  easing: 'cubic-bezier(0.16, 1, 0.3, 1)',
                  fill: 'forwards',
                },
              );
              const opacityAnim = el.animate(
                [{ opacity: 0 }, { opacity: 1 }],
                {
                  duration: START_OPACITY_ANIM_MS,
                  easing: 'ease-out',
                  fill: 'forwards',
                },
              );
              activeAnimsRef.current = [transformAnim, opacityAnim];

              transformAnim.finished
                .then(() => {
                  if (!mountedRef.current || latestGenRef.current !== openGen) return;
                  setAnim((prev) => settleStartOpen(prev, openGen));
                })
                .catch(() => {
                  // Cancelled by immediate close/reopen
                });

              if (!secondaryHydratedRef.current) {
                deferredHydrateTimerRef.current = window.setTimeout(() => {
                  deferredHydrateTimerRef.current = null;
                  hydrateSecondaryData();
                }, START_ENTER_ANIM_MS + 20);
              }
            });
          };

          const onVisChange = () => {
            if (document.visibilityState === 'visible') {
              launchSpring();
            }
          };

          pendingShowLaunchRef.current = { gen: openGen, launch: launchSpring };
          openArmCleanupRef.current = cleanupWaiters;

          if (typeof document !== 'undefined') {
            document.addEventListener('visibilitychange', onVisChange);
          }
          // Safety timer: starts immediately on `start-shown` or `visibilitychange`,
          // or within 12ms if already visible / backend predates `start-shown`.
          const waitMs =
            typeof document !== 'undefined' && document.visibilityState === 'visible' ? 12 : 32;
          fallbackTimer = window.setTimeout(launchSpring, waitMs);
        } else {
          if (ev.immediate) {
            startActiveRef.current = false;
            cancelChassisAnims();
            setAnim((prev) => reduceStartState(prev, ev));
            resetTransientOverlays();
            surfaceExitAck('start', ev.gen).catch(() => {});
          } else {
            const exitGen = ev.gen;
            const el = chassisRef.current;
            let liveOpacity = '1';
            let liveTransform = 'translate3d(0, 0, 0) scale(1)';
            if (el && typeof window !== 'undefined') {
              const cs = window.getComputedStyle(el);
              liveOpacity = cs.opacity || '1';
              liveTransform = cs.transform && cs.transform !== 'none' ? cs.transform : liveTransform;
            }

            cancelChassisAnims();
            setAnim((prev) => reduceStartState(prev, ev));

            animateSurfaceOut('start', exitGen, {
              durationMs: START_EXIT_ANIM_MS,
              offsetY: 12,
              toScale: 0.975,
            }).catch(() => {});

            const exitDuration = reducedMotionRef.current ? 0 : START_EXIT_ANIM_MS;
            if (exitDuration > 0 && el && typeof el.animate === 'function') {
              const exitAnim = el.animate(
                [
                  { opacity: Number(liveOpacity), transform: liveTransform },
                  { opacity: 0, transform: 'translate3d(0, 12px, 0) scale(0.975)' },
                ],
                {
                  duration: exitDuration,
                  easing: 'cubic-bezier(0.4, 0, 1, 1)',
                  fill: 'forwards',
                },
              );
              activeAnimsRef.current = [exitAnim];
            }

            closeCleanupTimerRef.current = window.setTimeout(() => {
              closeCleanupTimerRef.current = null;
              if (latestGenRef.current !== exitGen) return;
              startActiveRef.current = false;
              cancelChassisAnims();
              setAnim((prev) => completeStartClosing(prev, exitGen));
              resetTransientOverlays();
              surfaceExitAck('start', exitGen).catch(() => {});
            }, exitDuration);
          }
        }
      }),
      onEvent<void>('open-start-settings', () => {
        if (closeCleanupTimerRef.current !== null) {
          window.clearTimeout(closeCleanupTimerRef.current);
          closeCleanupTimerRef.current = null;
        }
        setSearchOpen(false);
        setQuery('');
        setDrawerOpen(false);
        setPowerOpen(false);
        setArmedPower(null);
        setPeriphOpen(false);
        setAddingSpace(false);
        setManagingSpaceId(null);
        setWidgetBuilderOpen(false);
        setActiveExplorerPath(null);
        setTileCtx(null);
        setTileEdit(null);
        setSettingsOpen(true);
      }),
    ];

    return () => {
      window.clearTimeout(idleTimer);
      cancelPendingOpenArm();
      cancelChassisAnims();
      if (deferredHydrateTimerRef.current !== null) {
        window.clearTimeout(deferredHydrateTimerRef.current);
      }
      if (closeCleanupTimerRef.current !== null) {
        window.clearTimeout(closeCleanupTimerRef.current);
      }
      if (toastTimerRef.current !== null) {
        window.clearTimeout(toastTimerRef.current);
      }
      subs.forEach((p) => p.then((u) => u()));
    };
  }, [
    hydrateCoreData,
    hydrateSecondaryData,
    flushPendingHiddenState,
    resetTransientOverlays,
    cancelPendingOpenArm,
    cancelChassisAnims,
  ]);

  // If active space was removed or deleted remotely, fall back to first available space
  useEffect(() => {
    if (!config) return;
    if (resolvedSpaces.some((s) => s.id === space)) return;
    const fallbackId = resolvedSpaces[0]?.id ?? 'personal';
    if (fallbackId !== space) {
      setSpaceDir(0);
      setRapidSwitch(false);
      setSpace(fallbackId);
      setWidgetBuilderOpen(false);
      setActiveExplorerPath(null);
    }
  }, [config, resolvedSpaces, space]);

  const weatherKey = useMemo(() => {
    const tw = config?.taskbarWidgets;
    return `${tw?.weatherCity ?? ''}::${tw?.weatherLat ?? ''}::${tw?.weatherLon ?? ''}::${tw?.weatherUnits ?? 'c'}`;
  }, [config?.taskbarWidgets]);
  const lastWeatherKeyRef = useRef<string>(weatherKey);

  const handleRefreshWeather = useCallback(async () => {
    setWeatherRefreshing(true);
    try {
      const next = await getWeather({ force: true });
      if (mountedRef.current) {
        lastWeatherFetchMsRef.current = Date.now();
        lastWeatherKeyRef.current = weatherKey;
        setWeather(next);
      }
    } catch (err) {
      showToast(formatIpcError(err));
    } finally {
      if (mountedRef.current) setWeatherRefreshing(false);
    }
  }, [showToast, weatherKey]);

  const handleRefreshPeriphs = useCallback(async () => {
    setPeriphsRefreshing(true);
    try {
      const next = await refreshPeripheralBatteries();
      if (mountedRef.current) {
        setPeriphs(next);
      }
    } catch (err) {
      showToast(formatIpcError(err));
    } finally {
      if (mountedRef.current) setPeriphsRefreshing(false);
    }
  }, [showToast]);

  // Lazy weather hydration when Start opens on a space containing the weather widget
  const needsWeather = anim.phase !== 'hidden' && (activeSpace?.widgets ?? []).includes('weather');
  useEffect(() => {
    if (!needsWeather) return;
    const keyChanged = lastWeatherKeyRef.current !== weatherKey;
    const stale = Date.now() - lastWeatherFetchMsRef.current > 10 * 60_000;
    if (weather && !keyChanged && !stale) return;
    lastWeatherKeyRef.current = weatherKey;
    lastWeatherFetchMsRef.current = Date.now();
    getWeather({ force: keyChanged })
      .then((next) => {
        if (mountedRef.current) setWeather(next);
      })
      .catch(() => {});
  }, [needsWeather, weatherKey, weather]);

  // Shared batch icon hydration callback passed down to StartSpaces
  const handleResolveIconKeys = useCallback((keys: string[]) => {
    const missing: string[] = [];
    const cachedHits: Record<string, string> = {};
    const seen = new Set<string>();

    for (const raw of keys) {
      if (!isCacheIconKey(raw) || seen.has(raw)) continue;
      seen.add(raw);
      const hit = iconCache.get(raw);
      if (hit) {
        cachedHits[raw] = hit;
      } else {
        missing.push(raw);
      }
    }

    if (Object.keys(cachedHits).length > 0) {
      setIconMap((prev) => {
        let changed = false;
        for (const k of Object.keys(cachedHits)) {
          if (prev[k] !== cachedHits[k]) {
            changed = true;
            break;
          }
        }
        return changed ? { ...prev, ...cachedHits } : prev;
      });
    }

    if (missing.length === 0) return;
    fetchIconsBatch(missing)
      .then((batch) => {
        if (!mountedRef.current) return;
        const fetchedKeys = Object.keys(batch);
        if (fetchedKeys.length === 0) return;
        setIconMap((prev) => {
          let changed = false;
          for (const k of fetchedKeys) {
            if (prev[k] !== batch[k]) {
              changed = true;
              break;
            }
          }
          return changed ? { ...prev, ...batch } : prev;
        });
      })
      .catch(() => {});
  }, []);

  // Unified IconCache batch fetch for apps, window groups & cache-keyed pins
  useEffect(() => {
    const keys: string[] = [];
    groups.forEach((g) => {
      if (g.iconKey) keys.push(g.iconKey);
    });
    apps.forEach((a) => {
      if (a.iconKey) keys.push(a.iconKey);
    });
    if (config) {
      [
        ...resolvedSpaces.flatMap((s) => s.items),
        ...config.gameCombos,
      ].forEach((p) => {
        if (p.iconKey) keys.push(p.iconKey);
      });
    }
    if (keys.length > 0) {
      handleResolveIconKeys(keys);
    }
  }, [apps, groups, config, resolvedSpaces, handleResolveIconKeys]);

  // Batched native icon hydration for path, UWP shell:AppsFolder\, WindowsApps aliases, and web/installed pins
  useEffect(() => {
    if (!config) return;
    const allPins = [
      ...resolvedSpaces.flatMap((s) => s.items),
    ];

    const targetsToResolve: string[] = [];
    for (const p of allPins) {
      if (p.kind === 'folder' || p.kind === 'combo' || !p.target || p.iconData) continue;
      const target = p.target.trim();
      const low = target.toLowerCase();
      const resolvable =
        target.includes(':\\') ||
        target.startsWith('\\\\') ||
        low.startsWith('shell:appsfolder\\') ||
        low.startsWith('http://') ||
        low.startsWith('https://');
      if (resolvable && !resolvedTargetsRef.current.has(low)) {
        resolvedTargetsRef.current.add(low);
        targetsToResolve.push(target);
      }
    }

    if (targetsToResolve.length === 0) return;

    ipc<Record<string, string>>('resolve_pin_icons', { targets: targetsToResolve })
      .then((batch) => {
        if (!mountedRef.current) {
          for (const t of targetsToResolve) {
            resolvedTargetsRef.current.delete(t.toLowerCase());
          }
          return;
        }
        const updates: Record<string, string> = {};
        const unresolvedLocal: string[] = [];
        for (const t of targetsToResolve) {
          const low = t.toLowerCase();
          const hit = batch?.[t] ?? batch?.[low];
          if (hit) {
            updates[t] = hit;
            updates[low] = hit;
          } else if (!low.startsWith('http://') && !low.startsWith('https://')) {
            unresolvedLocal.push(t);
          }
        }
        if (Object.keys(updates).length > 0) {
          setIconMap((prev) => ({ ...prev, ...updates }));
        }
        // Per-path fallback matching TaskbarView for any local/UWP exe missed by batch
        for (const t of unresolvedLocal) {
          const low = t.toLowerCase();
          ipc<string | null>('get_icon_for_path', { path: t })
            .then((data) => {
              if (!mountedRef.current) {
                resolvedTargetsRef.current.delete(low);
                return;
              }
              if (data) {
                setIconMap((prev) =>
                  prev[t] === data ? prev : { ...prev, [t]: data, [low]: data },
                );
              } else {
                resolvedTargetsRef.current.delete(low);
              }
            })
            .catch(() => {
              resolvedTargetsRef.current.delete(low);
            });
        }
      })
      .catch(() => {
        for (const t of targetsToResolve) {
          resolvedTargetsRef.current.delete(t.toLowerCase());
        }
      });
  }, [config, resolvedSpaces]);

  const allSpaceIds = useMemo(
    () => resolvedSpaces.map((s) => s.id),
    [resolvedSpaces],
  );

  // Directional Space switching (also closes Command Palette & popovers!)
  const selectSpace = useCallback(
    (nextId: string) => {
      const now = Date.now();
      const prevIdx = allSpaceIds.indexOf(spaceRef.current);
      const nextIdx = allSpaceIds.indexOf(nextId);
      const dir = prevIdx >= 0 && nextIdx >= 0 ? Math.sign(nextIdx - prevIdx) : 0;
      const isRapid = now - lastSpaceSwitchTsRef.current < 200;
      lastSpaceSwitchTsRef.current = now;

      setSpaceDir(dir);
      setRapidSwitch(isRapid);
      setSpace(nextId);
      setSearchOpen(false);
      setQuery('');
      setDrawerOpen(false);
      setSettingsOpen(false);
      setAddingSpace(false);
      setManagingSpaceId(null);
      setWidgetBuilderOpen(false);
      setActiveExplorerPath(null);
      setPeriphOpen(false);
      setPowerOpen(false);
      setArmedPower(null);
      setTileCtx(null);
      setTileEdit(null);
    },
    [allSpaceIds],
  );

  const isRunning = useCallback(
    (item: PinItem) => {
      if (item.kind !== 'app') return false;
      return Boolean(findMatchingGroupForPin(item, groups));
    },
    [groups],
  );

  const resolveIconForPin = useCallback(
    (item: PinItem): string | null => {
      if (isCacheIconKey(item.iconKey)) {
        const fromMap = iconMap[item.iconKey] || iconCache.get(item.iconKey);
        if (fromMap) return fromMap;
      }
      if (item.iconData) return item.iconData;
      if (item.target) {
        const direct = iconMap[item.target] || iconMap[item.target.toLowerCase()];
        if (direct) return direct;
      }
      // Fallback 1: inherit native icon from any matching running window group (even on another monitor!)
      const runningGroup = findMatchingGroupForPin(item, groups);
      if (runningGroup) {
        if (isCacheIconKey(runningGroup.iconKey)) {
          const fromGroupCache =
            iconMap[runningGroup.iconKey] || iconCache.get(runningGroup.iconKey);
          if (fromGroupCache) return fromGroupCache;
        }
        if (runningGroup.iconData) return runningGroup.iconData;
      }
      // Fallback 2: inherit native icon from matching installed app index (UWP/Start Menu shortcuts)
      const installed = findMatchingInstalledAppForPin(item, apps);
      if (installed) {
        if (isCacheIconKey(installed.iconKey)) {
          const fromAppCache = iconMap[installed.iconKey] || iconCache.get(installed.iconKey);
          if (fromAppCache) return fromAppCache;
        }
        if (installed.iconData) return installed.iconData;
      }
      return null;
    },
    [iconMap, groups, apps],
  );

  // Lazy Command Palette corpus & ranking: 0% CPU when search is closed or Start is hidden!
  const searchCorpus = useMemo(
    () =>
      searchOpen
        ? buildSearchCorpus({ config, apps, games, files, iconMap, groups, audio, clips })
        : [],
    [searchOpen, config, apps, games, files, iconMap, groups, audio, clips],
  );

  const searchResults = useMemo<RankedSearchResult>(
    () => (searchOpen ? rankSearchResults(searchCorpus, query, 12, config?.frecency) : EMPTY_RANKED_SEARCH),
    [searchOpen, searchCorpus, query, config?.frecency],
  );

  const dispatchSearchSecondary = useCallback(
    (item: SearchCorpusItem) => {
      const secondary = resolveSearchSecondaryAction(item);
      if (!secondary) return;
      switch (secondary.type) {
        case 'close_window':
          ipc('close_window_hwnd', { hwnd: secondary.hwnd })
            .then(() => showToast('Окно закрыто'))
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'open_terminal':
          ipc('open_terminal_at', { path: secondary.path })
            .then(() => {
              setSearchOpen(false);
              setQuery('');
            })
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'open_location':
          ipc('open_location', { path: secondary.path })
            .then(() => {
              setSearchOpen(false);
              setQuery('');
            })
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'copy_clipboard':
          ipc('copy_to_clipboard', { text: secondary.text })
            .then(() => {
              showToast('Скопировано');
              setSearchOpen(false);
              setQuery('');
            })
            .catch((err) => showToast(formatIpcError(err)));
          break;
      }
    },
    [showToast],
  );

  const dispatchSearchLaunch = useCallback(
    (item: SearchCorpusItem, ctrlKey: boolean, altKey = false) => {
      if (altKey) {
        dispatchSearchSecondary(item);
        return;
      }

      const payload = resolveSearchLaunchPayload(item, ctrlKey);
      const frecKey = item.frecencyKey ?? item.id;
      const shouldRecordFrecency = item.source !== 'clipboard' && item.source !== 'web';

      const trackFrecency = () => {
        if (shouldRecordFrecency) {
          recordFrecency(frecKey).then(handleApplyConfig).catch(() => {});
        }
      };

      switch (payload.command) {
        case 'focus_window':
          ipc('focus_window', payload.args)
            .then(() => {
              trackFrecency();
              toggleStart(false).catch(() => {});
            })
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'paste_clipboard_entry':
          pasteClipboardEntry(payload.args.text).catch((err) => showToast(formatIpcError(err)));
          break;
        case 'set_audio_device':
          ipc<AudioState>('set_audio_device', payload.args)
            .then((nextAudio) => {
              setAudio(nextAudio);
              trackFrecency();
              showToast('Вывод звука переключён');
            })
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'set_mic_muted':
          ipc<AudioState>('set_mic_muted', payload.args)
            .then((nextAudio) => {
              setAudio(nextAudio);
              trackFrecency();
              showToast(payload.args.muted ? 'Микрофон выключен' : 'Микрофон включён');
            })
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'empty_recycle_bin':
          emptyRecycleBin()
            .then(() => {
              trackFrecency();
              showToast('Корзина очищена');
            })
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'open_screen_snip':
          openScreenSnip()
            .then(trackFrecency)
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'refresh_indexes':
          refreshIndexes()
            .then(() => {
              trackFrecency();
              showToast('Индекс обновлён');
            })
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'open_settings':
          setSettingsOpen(true);
          setDrawerOpen(false);
          break;
        case 'power_action':
          ipc('power_action', payload.args).catch((err) => showToast(formatIpcError(err)));
          break;
        case 'launch_combo':
          ipc('launch_combo', payload.args)
            .then(trackFrecency)
            .catch((err) => showToast(formatIpcError(err)));
          break;
        case 'launch_item':
          ipc('launch_item', payload.args)
            .then(trackFrecency)
            .catch((err) => showToast(formatIpcError(err)));
          break;
      }

      setSearchOpen(false);
      setQuery('');
    },
    [dispatchSearchSecondary, handleApplyConfig, showToast],
  );

  const launchPin = useCallback(
    (item: PinItem) => {
      if (editMode) return;
      if (item.kind === 'combo') {
        const comboPayload = buildGameComboLaunchPayload(item);
        if (!comboPayload) {
          showToast('В связке нет целей для запуска');
          return;
        }
        ipc(comboPayload.command, comboPayload.args).catch((err) => {
          showToast(formatIpcError(err));
        });
        return;
      }
      const action = resolvePinAction(item, shiftHeld);
      if (action.type === 'terminal') {
        ipc('open_terminal_at', { path: action.path }).catch((err) =>
          showToast(formatIpcError(err)),
        );
      } else if (action.type === 'copy_path') {
        ipc('copy_to_clipboard', { text: action.text })
          .then(() => showToast(action.toast))
          .catch((err) => showToast(formatIpcError(err)));
      } else if (action.type === 'launch') {
        ipc('launch_item', {
          target: action.target,
          args: action.args,
          asAdmin: action.asAdmin,
          closeStart: action.closeStart,
        })
          .then(() => {
            const normTarget = item.target.trim().replace(/\//g, '\\').toLowerCase();
            recordFrecency(`target:${normTarget || item.id}`).then(handleApplyConfig).catch(() => {});
          })
          .catch((err) => showToast(formatIpcError(err)));
      }
    },
    [editMode, shiftHeld, showToast, handleApplyConfig],
  );

  // Tile Edit Popover handlers
  const openTileEditPopover = useCallback(
    (item: PinItem, spaceKey: string, coords?: { x: number; y: number }) => {
      const vw = typeof window !== 'undefined' ? window.innerWidth : 1172;
      const vh = typeof window !== 'undefined' ? window.innerHeight : 712;
      const rawX = coords ? coords.x : Math.round((vw - 348) / 2);
      const rawY = coords ? coords.y : Math.round((vh - 540) / 2);
      const pos = clampPopoverPos(rawX, rawY, 348, 540);

      setTileCtx(null);
      setEditPinTitle(item.title);
      setEditPinArgs(item.args ?? '');
      setEditPinAltAction(item.altAction ?? '');
      setEditPinAltLabel(item.altLabel ?? '');
      setEditCustomColor(item.customColor ?? null);
      setEditCustomIcon(item.customIcon ?? null);
      setEditIconData(item.customIcon === 'custom_image' ? (item.iconData ?? null) : null);
      setPickingCustomImg(false);
      setSavingPinEdit(false);
      setTileEdit({ x: pos.x, y: pos.y, item, spaceKey });
      requestAnimationFrame(() => editTitleInputRef.current?.focus());
    },
    [],
  );

  const handleSaveTileEdit = async () => {
    if (!tileEdit || savingPinEdit) return;
    const trimmedTitle = editPinTitle.trim();
    if (!trimmedTitle) {
      showToast('Введите название плитки');
      editTitleInputRef.current?.focus();
      return;
    }
    setSavingPinEdit(true);
    try {
      const hadCustomImg = tileEdit.item.customIcon === 'custom_image';
      const nextIconData =
        editCustomIcon === 'custom_image' ? editIconData : hadCustomImg ? null : undefined;
      const updated = await updatePin({
        space: tileEdit.spaceKey,
        pinId: tileEdit.item.id,
        title: trimmedTitle,
        args: tileEdit.item.kind === 'app' ? editPinArgs.trim() : undefined,
        altAction: editPinAltAction,
        altLabel: editPinAltAction ? editPinAltLabel.trim() : '',
        customIcon: editCustomIcon,
        customColor: editCustomColor,
        iconData: nextIconData,
      });
      handleApplyConfig(updated);
      setTileEdit(null);
      showToast('Плитка обновлена');
    } catch (err) {
      showToast(formatIpcError(err));
      setSavingPinEdit(false);
    }
  };

  // Build dynamic Tile Context Menu actions (all 6+ items with 14px SVG icons!)
  const activeCtxSource = tileCtx ?? lastTileCtxRef.current;
  const tileCtxActions = useMemo<TileCtxAction[]>(() => {
    if (!activeCtxSource) return [];
    const { item, spaceKey } = activeCtxSource;
    const actions: TileCtxAction[] = [];

    if (item.kind === 'folder') {
      actions.push({
        id: 'open',
        label: 'Открыть в TestBar',
        icon: <FolderSvg className="ic14" />,
        run: () => {
          setWidgetBuilderOpen(false);
          setActiveExplorerPath(item.target);
          setTileCtx(null);
        },
      });
      actions.push({
        id: 'open-explorer',
        label: 'Открыть в Проводнике Windows',
        icon: <ExternalWindowIcon className="ic14" />,
        run: () => {
          fsOpenInExplorer(item.target).catch((err) => showToast(formatIpcError(err)));
          setTileCtx(null);
        },
      });
    } else {
      actions.push({
        id: 'open',
        label: 'Открыть',
        icon: <ExternalWindowIcon className="ic14" />,
        run: () => {
          launchPin(item);
          setTileCtx(null);
        },
      });
    }

    if (item.kind === 'app' && canRunAsAdmin(item.target)) {
      actions.push({
        id: 'admin',
        label: 'От имени администратора',
        icon: <ShieldIcon className="ic14" />,
        run: () => {
          ipc('launch_item', {
            target: item.target,
            args: item.args ?? null,
            asAdmin: true,
            closeStart: true,
          }).catch((err) => showToast(formatIpcError(err)));
          setTileCtx(null);
        },
      });
    }

    if (item.kind === 'folder') {
      actions.push({
        id: 'terminal',
        label: 'Открыть в терминале',
        icon: <TerminalIcon className="ic14" />,
        run: () => {
          ipc('open_terminal_at', { path: item.target }).catch((err) =>
            showToast(formatIpcError(err)),
          );
          setTileCtx(null);
        },
      });
    }

    if (item.kind !== 'folder' && canOpenFileLocation(item.target)) {
      actions.push({
        id: 'location',
        label: 'Расположение файла',
        icon: <FolderSvg className="ic14" />,
        run: () => {
          ipc('open_location', { path: item.target }).catch((err) =>
            showToast(formatIpcError(err)),
          );
          setTileCtx(null);
        },
      });
    }

    if (item.target) {
      actions.push({
        id: 'copy',
        label: 'Скопировать путь',
        icon: <CopyIcon className="ic14" />,
        run: () => {
          ipc('copy_to_clipboard', { text: item.target })
            .then(() => showToast('Путь скопирован'))
            .catch((err) => showToast(formatIpcError(err)));
          setTileCtx(null);
        },
      });
    }

    if (item.kind !== 'combo') {
      actions.push({
        id: 'edit',
        label: 'Настроить вид / изменить',
        icon: <EditIcon className="ic14" />,
        run: () => {
          openTileEditPopover(item, spaceKey, { x: activeCtxSource.x, y: activeCtxSource.y });
        },
      });
    }

    if (item.kind === 'app') {
      const existingTbPin = findExistingTaskbarPin(item, config?.taskbarPins ?? []);
      if (existingTbPin) {
        actions.push({
          id: 'unpin-tb',
          label: 'Открепить с панели задач',
          icon: <UnpinIcon className="ic14" />,
          run: () => {
            ipc<TestBarConfig>('remove_pin', { space: 'taskbar', itemId: existingTbPin.id })
              .then((next) => {
                handleApplyConfig(next);
                showToast('Откреплено с панели задач');
              })
              .catch((err) => showToast(formatIpcError(err)));
            setTileCtx(null);
          },
        });
      } else {
        actions.push({
          id: 'pin-tb',
          label: 'Закрепить на панели задач',
          icon: <PinIcon className="ic14" />,
          run: () => {
            ipc<TestBarConfig>('add_pin', {
              space: 'taskbar',
              item: { ...item, id: `tb-${Date.now()}`, iconData: null },
            })
              .then((next) => {
                handleApplyConfig(next);
                showToast('Закреплено на панели задач');
              })
              .catch((err) => showToast(formatIpcError(err)));
            setTileCtx(null);
          },
        });
      }
    }

    actions.push({
      id: 'unpin',
      label: 'Открепить',
      danger: true,
      icon: <KillIcon className="ic14" />,
      run: () => {
        ipc<TestBarConfig>('remove_pin', { space: spaceKey, itemId: item.id })
          .then((next) => {
            handleApplyConfig(next);
            showToast('Откреплено');
          })
          .catch((err) => showToast(formatIpcError(err)));
        setTileCtx(null);
      },
    });

    return actions;
  }, [activeCtxSource, config?.taskbarPins, handleApplyConfig, launchPin, openTileEditPopover, showToast]);

  // Global keyboard handler: zero-loss type-anywhere search, Ctrl+1..9, Tab, tileCtx nav, Esc, Shift
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Shift') setShiftHeld(true);

      // Ctrl+1..9 direct Space switcher (works unless actively typing inside a modal input)
      const activeEl = document.activeElement as HTMLElement | null;
      const activeTag = activeEl?.tagName;
      const typingInField =
        (activeTag === 'INPUT' || activeTag === 'TEXTAREA' || activeTag === 'SELECT') &&
        activeEl !== cmdInputRef.current;

      if (e.ctrlKey && !e.altKey && !e.metaKey && e.key >= '1' && e.key <= '9' && !typingInField) {
        const idx = Number(e.key) - 1;
        if (idx >= 0 && idx < allSpaceIds.length) {
          e.preventDefault();
          selectSpace(allSpaceIds[idx]);
          return;
        }
      }

      if (typingInField) {
        if (e.key === 'Escape' && tileEdit) {
          e.preventDefault();
          setTileEdit(null);
        }
        return;
      }

      // Tile Context Menu keyboard navigation
      if (tileCtx) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setTileCtxIdx((i) => (tileCtxActions.length > 0 ? (i + 1) % tileCtxActions.length : 0));
          return;
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          setTileCtxIdx((i) =>
            tileCtxActions.length > 0 ? (i - 1 + tileCtxActions.length) % tileCtxActions.length : 0,
          );
          return;
        }
        if (e.key === 'Enter' && tileCtxActions[tileCtxIdx]) {
          e.preventDefault();
          tileCtxActions[tileCtxIdx].run();
          return;
        }
        if (e.key === 'Escape') {
          e.preventDefault();
          setTileCtx(null);
          return;
        }
      }

      if (e.key === 'Escape') {
        if (tileEdit) {
          setTileEdit(null);
        } else if (searchOpen) {
          setSearchOpen(false);
          setQuery('');
        } else if (managingSpaceId) {
          setManagingSpaceId(null);
        } else if (addingSpace) {
          setAddingSpace(false);
        } else if (widgetBuilderOpen) {
          setWidgetBuilderOpen(false);
        } else if (drawerOpen) {
          setDrawerOpen(false);
        } else if (settingsOpen) {
          setSettingsOpen(false);
        } else if (periphOpen) {
          setPeriphOpen(false);
        } else if (powerOpen) {
          setPowerOpen(false);
          setArmedPower(null);
        } else if (activeExplorerPath !== null) {
          setActiveExplorerPath(null);
        } else {
          toggleStart(false).catch(() => {});
        }
        return;
      }

      const insideDialog =
        Boolean(activeEl?.closest('[role="dialog"]')) ||
        Boolean(document.querySelector('.combo-builder-sheet'));

      if (
        e.key === 'Tab' &&
        !searchOpen &&
        !drawerOpen &&
        !settingsOpen &&
        !addingSpace &&
        !managingSpaceId &&
        !widgetBuilderOpen &&
        activeExplorerPath === null &&
        !periphOpen &&
        !powerOpen &&
        !tileCtx &&
        !tileEdit &&
        !insideDialog
      ) {
        e.preventDefault();
        const idx = allSpaceIds.indexOf(space);
        const next = allSpaceIds[(idx + (e.shiftKey ? allSpaceIds.length - 1 : 1)) % allSpaceIds.length];
        if (next) selectSpace(next);
        return;
      }

      if (searchOpen) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setCmdIdx((i) => Math.min(Math.max(0, searchResults.items.length - 1), i + 1));
        } else if (e.key === 'ArrowUp') {
          e.preventDefault();
          setCmdIdx((i) => Math.max(0, i - 1));
        } else if (e.key === 'Enter' && searchResults.items.length > 0) {
          const clamped = Math.max(0, Math.min(cmdIdx, searchResults.items.length - 1));
          const targetItem = searchResults.items[clamped];
          if (targetItem) {
            e.preventDefault();
            dispatchSearchLaunch(targetItem, e.ctrlKey, e.altKey);
          }
        }
        return;
      }

      if (
        !drawerOpen &&
        !settingsOpen &&
        !addingSpace &&
        !managingSpaceId &&
        !widgetBuilderOpen &&
        activeExplorerPath === null &&
        !tileEdit &&
        !insideDialog &&
        isPrintableSearchTrigger(e)
      ) {
        e.preventDefault();
        setTileCtx(null);
        setPeriphOpen(false);
        setPowerOpen(false);
        setArmedPower(null);
        setSearchTrigger('keyboard');
        setSearchOpen(true);
        setQuery(e.key);
        setCmdIdx(0);
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === 'Shift') setShiftHeld(false);
    };

    const onBlur = () => setShiftHeld(false);

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onBlur);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onBlur);
    };
  }, [
    allSpaceIds,
    searchOpen,
    addingSpace,
    managingSpaceId,
    widgetBuilderOpen,
    activeExplorerPath,
    drawerOpen,
    settingsOpen,
    periphOpen,
    tileCtx,
    tileCtxIdx,
    tileCtxActions,
    tileEdit,
    powerOpen,
    space,
    searchResults.items,
    cmdIdx,
    selectSpace,
    dispatchSearchLaunch,
  ]);

  // Pinning handlers — honest success/error toasts AFTER IPC resolves!
  const handlePinFolder = async () => {
    try {
      const picked = await ipc<string | null>('pick_folder');
      if (!picked) return;
      const baseName = picked.split('\\').filter(Boolean).pop() || picked;
      const hasSpaces = Boolean(config?.spaces && config.spaces.length > 0);
      const targetSpace = hasSpaces
        ? space
        : space === 'personal'
          ? 'personal_links'
          : space === 'work'
            ? 'work_folders'
            : space;
      const updated = await ipc<TestBarConfig>('add_pin', {
        space: targetSpace,
        item: {
          id: `pin-${Date.now()}`,
          kind: 'folder',
          title: baseName,
          subtitle: 'Папка',
          target: picked,
          altAction: targetSpace === 'personal_links' ? 'Скопировать путь' : 'Открыть в терминале',
          accentRgb: '242,180,75',
          iconKey: 'folder',
          iconData: null,
        },
      });
      handleApplyConfig(updated);
      showToast(`Закреплено: ${baseName}`);
    } catch (err) {
      showToast(formatIpcError(err));
    }
  };

  const handlePinFile = async () => {
    try {
      const picked = await ipc<string | null>('pick_file');
      if (!picked) return;
      const baseName = picked.split('\\').filter(Boolean).pop() || picked;
      const isExe = picked.toLowerCase().endsWith('.exe');
      const hasSpaces = Boolean(config?.spaces && config.spaces.length > 0);
      const targetSpace = hasSpaces
        ? space
        : space === 'personal'
          ? isExe
            ? 'personal'
            : 'personal_links'
          : space;
      const title = isExe ? baseName.replace(/\.exe$/i, '') : baseName;
      const updated = await ipc<TestBarConfig>('add_pin', {
        space: targetSpace,
        item: {
          id: `pin-${Date.now()}`,
          kind: isExe ? 'app' : 'file',
          title,
          subtitle: isExe ? 'Программа' : 'Файл',
          target: picked,
          altAction: isExe ? 'От имени администратора' : 'Скопировать путь',
          accentRgb: '82,184,242',
          iconKey: isExe ? baseName.toLowerCase() : 'file',
          iconData: null,
        },
      });
      handleApplyConfig(updated);
      showToast(`Закреплено: ${title}`);
    } catch (err) {
      showToast(formatIpcError(err));
    }
  };

  const handlePinUrl = async (url: string, title: string) => {
    const hasSpaces = Boolean(config?.spaces && config.spaces.length > 0);
    const targetSpace = hasSpaces
      ? space
      : space === 'personal'
        ? 'personal_links'
        : space;
    const updated = await ipc<TestBarConfig>('add_pin', {
      space: targetSpace,
      item: {
        id: `url-${Date.now()}`,
        kind: 'url',
        title,
        subtitle: 'Ссылка',
        target: url,
        altAction: 'Скопировать путь',
        accentRgb: '167,139,250',
        iconKey: 'url',
        iconData: null,
      },
    });
    handleApplyConfig(updated);
    showToast(`Закреплено: ${title}`);
  };

  const handleTogglePinApp = async (app: IndexedApp, existingPin: PinItem | null) => {
    try {
      if (existingPin) {
        const updated = await ipc<TestBarConfig>('remove_pin', { space, itemId: existingPin.id });
        handleApplyConfig(updated);
        return;
      }
      const isUwp = Boolean(app.aumid) || app.target.toLowerCase().startsWith('shell:appsfolder\\');
      const updated = await ipc<TestBarConfig>('add_pin', {
        space,
        item: {
          id: `app-${Date.now()}`,
          kind: 'app',
          title: app.name,
          subtitle: app.category,
          target: app.target,
          altAction: isUwp ? '' : 'От имени администратора',
          accentRgb: '139,150,248',
          iconKey: app.iconKey || app.name.toLowerCase(),
          iconData: null,
          matchAumid: app.aumid ?? null,
        },
      });
      handleApplyConfig(updated);
    } catch (err) {
      showToast(formatIpcError(err));
    }
  };

  const handleCreateSpace = async (params: {
    name: string;
    template: SpaceTemplateId;
    accentHex: string;
    accentRgb: string;
    widgets: SpaceWidgetId[];
  }) => {
    const updated = await createSpace({
      name: params.name,
      template: params.template,
      accentHex: params.accentHex,
      accentRgb: params.accentRgb,
      widgets: params.widgets,
    });
    handleApplyConfig(updated);
    const newest = updated.spaces[updated.spaces.length - 1];
    if (newest) {
      selectSpace(newest.id);
    }
    showToast(`Создано пространство «${params.name}»`);
  };

  const handleUpdateSpace = async (params: {
    id: string;
    name?: string;
    accentHex?: string;
    accentRgb?: string;
    layoutKind?: SpaceLayoutKind;
    showHeroRow?: boolean;
  }) => {
    const updated = await updateSpace(params);
    handleApplyConfig(updated);
    showToast('Пространство обновлено');
  };

  const handleReorderSpaces = async (orderedIds: string[]) => {
    try {
      const updated = await reorderSpaces(orderedIds);
      handleApplyConfig(updated);
    } catch (err) {
      showToast(formatIpcError(err));
    }
  };

  const handleDeleteSpace = async (id: string) => {
    const updated = await deleteSpace(id);
    handleApplyConfig(updated);
    if (spaceRef.current === id) {
      const fallback = resolveConfiguredSpaces(updated)[0]?.id ?? 'personal';
      setSpaceDir(-1);
      setSpace(fallback);
    }
    showToast('Пространство удалено');
  };

  const handleSetSpaceWidgets = async (spaceId: string, widgets: SpaceWidgetId[]) => {
    try {
      const updated = await setSpaceWidgets(spaceId, widgets);
      handleApplyConfig(updated);
    } catch (err) {
      showToast(formatIpcError(err));
    }
  };

  const handleOpenWidgetBuilder = useCallback(
    (spaceId: string) => {
      if (spaceRef.current !== spaceId) {
        selectSpace(spaceId);
      }
      setActiveExplorerPath(null);
      setManagingSpaceId(null);
      setAddingSpace(false);
      setWidgetBuilderOpen(true);
    },
    [selectSpace],
  );

  const handlePowerClick = (kind: PowerActionKind) => {
    const res = reducePowerClick(armedPower, kind);
    setArmedPower(res.nextArmed);
    if (res.closePower) setPowerOpen(false);
    if (res.execute) {
      ipc('power_action', { action: res.execute }).catch((err) =>
        showToast(formatIpcError(err)),
      );
    }
  };

  const openTileContextMenu = useCallback(
    (e: React.MouseEvent, item: PinItem, spaceKey: string) => {
      e.preventDefault();
      e.stopPropagation();
      const pos = clampPopoverPos(e.clientX, e.clientY, 244, 300);
      setTileEdit(null);
      setTileCtxIdx(0);
      setTileCtx({
        x: pos.x,
        y: pos.y,
        item,
        spaceKey,
      });
    },
    [],
  );

  const activeAccentHex = activeSpace?.accentHex?.trim() || '#52B8F2';
  const activeAccentRgb = normalizeRgbTriplet(activeSpace?.accentRgb) ?? '82, 184, 242';
  const spaceAccentStyle = useMemo(
    () =>
      ({
        ['--accent' as string]: activeAccentHex,
        ['--accent-rgb' as string]: activeAccentRgb,
      }) as React.CSSProperties,
    [activeAccentHex, activeAccentRgb],
  );

  const startVisible = anim.phase !== 'hidden';
  const startChassisAnimCls = getStartChassisAnimClass(anim);

  return (
    <div
      className={`start-root ${editMode ? 'edit-mode' : ''} ${shiftHeld ? 'shift-active' : ''}`}
      data-space={spaceTheme}
      data-visible={startVisible ? 'true' : 'false'}
      style={spaceAccentStyle}
      onClick={() => {
        if (tileCtx) setTileCtx(null);
        if (tileEdit) setTileEdit(null);
        if (powerOpen) {
          setPowerOpen(false);
          setArmedPower(null);
        }
      }}
    >
      {/* NON-REMOUNTING START CHASSIS — WAAPI-driven spring synced with DComp underlay */}
      <section
        ref={chassisRef}
        className={`start-chassis ${startChassisAnimCls}`.trim()}
        data-phase={anim.phase}
      >
        {/* 2-Layer GPU Opacity Ambient Aura Crossfade */}
        <div
          className={`chassis-aura-layer ${activeAuraSlot === 0 ? 'active' : ''}`}
          style={{ ['--aura-rgb' as string]: auraSlots[0] } as React.CSSProperties}
          aria-hidden="true"
        />
        <div
          className={`chassis-aura-layer ${activeAuraSlot === 1 ? 'active' : ''}`}
          style={{ ['--aura-rgb' as string]: auraSlots[1] } as React.CSSProperties}
          aria-hidden="true"
        />

        <StartHeader
          space={space}
          spaces={resolvedSpaces}
          periphs={periphs}
          periphsRefreshing={periphsRefreshing}
          audio={audio}
          periphOpen={periphOpen}
          editMode={editMode}
          drawerOpen={drawerOpen}
          settingsOpen={settingsOpen}
          searchOpen={searchOpen}
          powerOpen={powerOpen}
          armedPower={armedPower}
          addingSpace={addingSpace}
          managingSpaceId={managingSpaceId}
          onSelectSpace={selectSpace}
          onTogglePeriph={() => setPeriphOpen((v) => !v)}
          onClosePeriph={() => setPeriphOpen(false)}
          onRefreshPeriphs={handleRefreshPeriphs}
          onSelectAudioDevice={(deviceId) => {
            ipc<AudioState>('set_audio_device', { deviceId })
              .then(setAudio)
              .catch((err) => showToast(formatIpcError(err)));
          }}
          onOpenSearch={() => {
            setSearchTrigger('click');
            setSearchOpen(true);
            setDrawerOpen(false);
            setSettingsOpen(false);
            setAddingSpace(false);
            setManagingSpaceId(null);
            setWidgetBuilderOpen(false);
            setTileCtx(null);
            setTileEdit(null);
            setQuery('');
            setCmdIdx(0);
          }}
          onToggleEditMode={() => setEditMode((v) => !v)}
          onToggleDrawer={() => {
            setDrawerOpen((v) => !v);
            setSettingsOpen(false);
            setSearchOpen(false);
            setAddingSpace(false);
            setManagingSpaceId(null);
            setWidgetBuilderOpen(false);
            setTileCtx(null);
            setTileEdit(null);
          }}
          onToggleSettings={() => {
            setSettingsOpen((v) => !v);
            setDrawerOpen(false);
            setSearchOpen(false);
            setAddingSpace(false);
            setManagingSpaceId(null);
            setWidgetBuilderOpen(false);
            setTileCtx(null);
            setTileEdit(null);
          }}
          onTogglePower={() => {
            setPowerOpen((v) => !v);
            setArmedPower(null);
          }}
          onClosePower={() => {
            setPowerOpen(false);
            setArmedPower(null);
          }}
          onPowerAction={handlePowerClick}
          onSetAddingSpace={(open) => {
            setAddingSpace(open);
            if (open) {
              setManagingSpaceId(null);
              setPeriphOpen(false);
              setTileCtx(null);
              setTileEdit(null);
            }
          }}
          onSetManagingSpaceId={(id) => {
            setManagingSpaceId(id);
            if (id) {
              setAddingSpace(false);
              setPeriphOpen(false);
              setTileCtx(null);
              setTileEdit(null);
            }
          }}
          onCreateSpace={handleCreateSpace}
          onUpdateSpace={handleUpdateSpace}
          onReorderSpaces={handleReorderSpaces}
          onDeleteSpace={handleDeleteSpace}
          onOpenWidgetBuilder={handleOpenWidgetBuilder}
        />

        <div
          className="space-stage"
          onClick={() => {
            if (periphOpen) setPeriphOpen(false);
            if (powerOpen) {
              setPowerOpen(false);
              setArmedPower(null);
            }
            if (tileCtx) setTileCtx(null);
            if (tileEdit) setTileEdit(null);
          }}
        >
          {activeExplorerPath !== null ? (
            <div className="v3-explorer-slot" data-explorer-path={activeExplorerPath}>
              <div className="v3-explorer-placeholder">
                <div className="v3-explorer-slot-head">
                  <div className="v3-explorer-slot-path" title={activeExplorerPath}>
                    <FolderSvg className="ic16" />
                    <span>{activeExplorerPath}</span>
                  </div>
                  <div className="hdr-actions">
                    <button
                      type="button"
                      className="pin-any-btn"
                      onClick={() => {
                        fsOpenInExplorer(activeExplorerPath).catch((err) =>
                          showToast(formatIpcError(err)),
                        );
                      }}
                    >
                      <ExternalWindowIcon className="ic14" />
                      <span>Проводник Windows</span>
                    </button>
                    <button
                      type="button"
                      className="pin-any-btn"
                      onClick={() => setActiveExplorerPath(null)}
                    >
                      <ArrowLeftIcon className="ic14" />
                      <span>Закрыть</span>
                    </button>
                  </div>
                </div>
              </div>
            </div>
          ) : activeSpace ? (
            <StartSpaces
              space={space}
              activeSpace={activeSpace}
              config={config}
              games={games}
              audio={audio}
              media={media}
              telem={telem}
              clips={clips}
              periphs={periphs}
              periphsRefreshing={periphsRefreshing}
              weather={weather}
              weatherRefreshing={weatherRefreshing}
              editMode={editMode}
              shiftHeld={shiftHeld}
              widgetBuilderOpen={widgetBuilderOpen}
              isRunning={isRunning}
              resolveIconForPin={resolveIconForPin}
              onLaunchPin={launchPin}
              onOpenFolder={(path) => {
                setWidgetBuilderOpen(false);
                setActiveExplorerPath(path);
              }}
              onOpenFolderInExplorer={(path) => {
                fsOpenInExplorer(path).catch((err) => showToast(formatIpcError(err)));
              }}
              onOpenDrawer={() => setDrawerOpen(true)}
              onOpenTileContext={openTileContextMenu}
              onSetAudio={handleSetAudioFromChild}
              onSetMedia={setMedia}
              onSetTelem={setTelem}
              onSetClips={setClips}
              onShowToast={showToast}
              onRefreshWeather={handleRefreshWeather}
              onRefreshPeriphs={handleRefreshPeriphs}
              onToggleWidgetBuilder={(open) => setWidgetBuilderOpen(open)}
              onSetSpaceWidgets={handleSetSpaceWidgets}
              startVisible={startVisible}
              startAnimSettled={anim.phase === 'open'}
              onAudioDragStateChange={handleAudioDragStateChange}
              spaceDir={spaceDir}
              rapidSwitch={rapidSwitch}
              iconMap={iconMap}
              apps={apps}
              onResolveIconKeys={handleResolveIconKeys}
              onSetConfig={handleApplyConfig}
              onOptimisticReorder={handleOptimisticReorder}
              onEditPin={(item, spaceKey) => openTileEditPopover(item, spaceKey)}
            />
          ) : null}
        </div>

        <StartPinDrawer
          open={drawerOpen}
          space={space}
          config={config}
          apps={apps}
          iconMap={iconMap}
          onClose={() => setDrawerOpen(false)}
          onPinFolder={handlePinFolder}
          onPinFile={handlePinFile}
          onPinUrl={handlePinUrl}
          onTogglePinApp={handleTogglePinApp}
        />

        <StartSettingsOverlay
          open={settingsOpen}
          config={config}
          onClose={() => setSettingsOpen(false)}
          onSetBarMode={async (mode) => {
            const next = await ipc<TestBarConfig>('set_bar_mode', { mode });
            handleApplyConfig(next);
          }}
          onSetSecondaryMode={async (mode: SecondaryTaskbarMode) => {
            const next = await ipc<TestBarConfig>('set_secondary_taskbar_mode', { mode });
            handleApplyConfig(next);
          }}
          onSetAutostart={async (enable) => {
            const next = await ipc<TestBarConfig>('set_autostart', { enable });
            handleApplyConfig(next);
          }}
          onSetAppearance={async (params) => {
            const next = await setAppearanceSettings(params);
            handleApplyConfig(next);
          }}
          onRefreshIndexes={async () => {
            await refreshIndexes();
            showToast('Индекс обновлён');
          }}
          onRestoreTrayApp={async (key) => {
            const next = await restoreTrayApp(key);
            handleApplyConfig(next);
            showToast('Значок возвращён в трей');
          }}
          onRestoreAllTrayApps={async () => {
            const next = await restoreAllTrayApps();
            handleApplyConfig(next);
            showToast('Все значки трея возвращены');
          }}
          onExportConfig={async () => {
            const path = await exportConfigDialog();
            if (path) {
              showToast('Конфигурация экспортирована');
            }
          }}
          onImportConfig={async () => {
            const next = await importConfigDialog();
            if (next) {
              handleApplyConfig(next);
              showToast('Конфигурация импортирована');
            }
          }}
          onRestoreWindowsAndExit={() => {
            ipc('restore_windows_taskbar_and_exit').catch((err) =>
              showToast(formatIpcError(err)),
            );
          }}
        />

        <StartCommandPalette
          open={searchOpen}
          trigger={searchTrigger}
          query={query}
          results={searchResults}
          selectedIdx={cmdIdx}
          inputRef={cmdInputRef}
          onQueryChange={setQuery}
          onSelectIdx={setCmdIdx}
          onLaunch={dispatchSearchLaunch}
          onClose={() => {
            setSearchOpen(false);
            setQuery('');
          }}
        />

        {toastPresence.mounted && (
          <div
            className="start-toast"
            data-phase={toastPresence.phase}
            role="status"
            aria-live="polite"
          >
            {lastToastRef.current}
          </div>
        )}
      </section>

      {/* TILE CONTEXT MENU — Rendered OUTSIDE .start-chassis overflow: hidden! */}
      {tileCtxPresence.mounted && activeCtxSource && (
        <div
          className="flyout-card ctx-card tile-ctx-menu"
          data-phase={tileCtxPresence.phase}
          role="menu"
          aria-label={activeCtxSource.item.title}
          style={{ left: activeCtxSource.x, top: activeCtxSource.y }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="ctx-sec">{activeCtxSource.item.title}</div>
          {tileCtxActions.map((act, idx) => (
            <React.Fragment key={act.id}>
              {act.danger && <div className="ctx-sep" />}
              <button
                type="button"
                role="menuitem"
                className={`ctx-item ${act.danger ? 'danger' : ''} ${idx === tileCtxIdx ? 'focused' : ''}`}
                onMouseEnter={() => setTileCtxIdx(idx)}
                onClick={act.run}
              >
                {act.icon}
                <span className="ctx-item-label">{act.label}</span>
              </button>
            </React.Fragment>
          ))}
        </div>
      )}

      {/* TILE EDIT POPOVER — Rendered OUTSIDE .start-chassis overflow: hidden! */}
      {tileEditPresence.mounted && (tileEdit ?? lastTileEditRef.current) && (
        <div
          className="tile-edit-pop v3-tile-edit"
          data-phase={tileEditPresence.phase}
          role="dialog"
          aria-label="Настроить плитку"
          style={{
            left: (tileEdit ?? lastTileEditRef.current)!.x,
            top: (tileEdit ?? lastTileEditRef.current)!.y,
          }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="space-create-head">Настроить плитку</div>

          {(() => {
            const editCtx = (tileEdit ?? lastTileEditRef.current)!;
            const previewRgb = resolveCustomColorRgb(editCustomColor);
            const fallbackIconSrc =
              editCustomIcon === 'custom_image' || editCustomIcon
                ? null
                : resolveIconForPin(editCtx.item);
            return (
              <div
                className={`tile-edit-preview ${previewRgb ? 'has-custom-color' : ''}`}
                style={
                  previewRgb
                    ? ({ ['--tile-custom-rgb' as string]: previewRgb } as React.CSSProperties)
                    : undefined
                }
              >
                <div className="tile-edit-preview-ic">
                  {editCustomIcon === 'custom_image' && editIconData ? (
                    <img className="custom-tile-img" src={editIconData} alt="" />
                  ) : editCustomIcon && TILE_ICON_CATALOG.some((c) => c.id === editCustomIcon) ? (
                    renderCustomTileIcon(editCustomIcon, 'ic18')
                  ) : fallbackIconSrc ? (
                    <img src={fallbackIconSrc} alt="" />
                  ) : editCtx.item.kind === 'folder' ? (
                    <FolderSvg className="ic18" />
                  ) : (
                    <FallbackAppIcon
                      iconKey={editCtx.item.iconKey}
                      kind={editCtx.item.kind}
                      target={editCtx.item.target}
                      size={18}
                    />
                  )}
                </div>
                <span className="tile-edit-preview-name">
                  {editPinTitle.trim() || editCtx.item.title}
                </span>
              </div>
            );
          })()}

          <div className="tile-edit-field">
            <label className="tile-edit-lbl" htmlFor="tb-edit-pin-title">
              Название
            </label>
            <input
              id="tb-edit-pin-title"
              ref={editTitleInputRef}
              className="space-create-input"
              maxLength={40}
              value={editPinTitle}
              onChange={(e) => setEditPinTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  void handleSaveTileEdit();
                } else if (e.key === 'Escape') {
                  e.preventDefault();
                  e.stopPropagation();
                  setTileEdit(null);
                }
              }}
            />
          </div>

          <div className="tile-edit-field">
            <div className="tile-edit-lbl-row">
              <span className="tile-edit-lbl">Цвет плитки</span>
              <button
                type="button"
                className="tile-reset-mini-btn"
                onClick={() => setEditCustomColor(null)}
              >
                По умолчанию
              </button>
            </div>
            <div className="tile-color-grid">
              {TILE_COLOR_PALETTE.map((swatch) => {
                const selected = editCustomColor?.toLowerCase() === swatch.hex.toLowerCase();
                return (
                  <button
                    key={swatch.id}
                    type="button"
                    className={`tile-color-swatch ${selected ? 'selected' : ''}`}
                    style={{ background: swatch.hex }}
                    title={swatch.label}
                    aria-label={swatch.label}
                    onClick={() => setEditCustomColor(selected ? null : swatch.hex)}
                  />
                );
              })}
            </div>
          </div>

          <div className="tile-edit-field">
            <div className="tile-edit-lbl-row">
              <span className="tile-edit-lbl">Иконка плитки</span>
              <button
                type="button"
                className="tile-reset-mini-btn"
                onClick={() => {
                  setEditCustomColor(null);
                  setEditCustomIcon(null);
                  setEditIconData(null);
                }}
              >
                Сбросить вид
              </button>
            </div>
            <div className="tile-icon-grid">
              {TILE_ICON_CATALOG.map((opt) => {
                const selected = editCustomIcon === opt.id;
                return (
                  <button
                    key={opt.id}
                    type="button"
                    className={`tile-icon-opt ${selected ? 'selected' : ''}`}
                    title={opt.label}
                    aria-label={opt.label}
                    onClick={() => {
                      setEditCustomIcon(selected ? null : opt.id);
                      if (!selected) setEditIconData(null);
                    }}
                  >
                    {renderCustomTileIcon(opt.id, 'ic16')}
                  </button>
                );
              })}
            </div>
            <div className="tile-icon-actions">
              <button
                type="button"
                className="pin-any-btn"
                disabled={pickingCustomImg}
                onClick={() => {
                  setPickingCustomImg(true);
                  pickCustomIconImage()
                    .then((dataUrl) => {
                      if (dataUrl && mountedRef.current) {
                        setEditCustomIcon('custom_image');
                        setEditIconData(dataUrl);
                      }
                    })
                    .catch((err) => showToast(formatIpcError(err)))
                    .finally(() => {
                      if (mountedRef.current) setPickingCustomImg(false);
                    });
                }}
              >
                {pickingCustomImg ? 'Выбор файла…' : 'Своя картинка…'}
              </button>
            </div>
          </div>

          {(tileEdit ?? lastTileEditRef.current)!.item.kind === 'app' && (
            <div className="tile-edit-field">
              <label className="tile-edit-lbl" htmlFor="tb-edit-pin-args">
                Аргументы запуска
              </label>
              <input
                id="tb-edit-pin-args"
                className="space-create-input"
                placeholder="Например, --new-window"
                value={editPinArgs}
                onChange={(e) => setEditPinArgs(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    void handleSaveTileEdit();
                  } else if (e.key === 'Escape') {
                    e.preventDefault();
                    e.stopPropagation();
                    setTileEdit(null);
                  }
                }}
              />
            </div>
          )}

          <div className="tile-edit-field">
            <label className="tile-edit-lbl" htmlFor="tb-edit-pin-alt">
              Действие при удержании Shift
            </label>
            <select
              id="tb-edit-pin-alt"
              className="space-create-input"
              value={editPinAltAction}
              onChange={(e) => setEditPinAltAction(e.target.value)}
            >
              {ALT_ACTION_OPTIONS.map((opt) => (
                <option key={opt.value || 'none'} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </div>

          {editPinAltAction && (
            <div className="tile-edit-field">
              <label className="tile-edit-lbl" htmlFor="tb-edit-pin-alt-label">
                Подпись действия (необязательно)
              </label>
              <input
                id="tb-edit-pin-alt-label"
                className="space-create-input"
                maxLength={28}
                placeholder={editPinAltAction}
                value={editPinAltLabel}
                onChange={(e) => setEditPinAltLabel(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    void handleSaveTileEdit();
                  } else if (e.key === 'Escape') {
                    e.preventDefault();
                    e.stopPropagation();
                    setTileEdit(null);
                  }
                }}
              />
            </div>
          )}

          <div className="space-create-actions">
            <button
              type="button"
              className="space-create-cancel"
              onClick={() => setTileEdit(null)}
            >
              Отмена
            </button>
            <button
              type="button"
              className="space-create-submit"
              disabled={savingPinEdit}
              onClick={() => void handleSaveTileEdit()}
            >
              Сохранить
            </button>
          </div>
        </div>
      )}
    </div>
  );
};
