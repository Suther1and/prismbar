import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { applyAppearanceToDocument } from '../lib/appearance';
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
  getAccessibilityStatus,
  getNetworkSnapshot,
  getPeripheralBatteries,
  getPlatformInfo,
  getQuickTogglesState,
  getWeather,
  hideBatteryDevice,
  iconCache,
  importConfigDialog,
  ipc,
  isCacheIconKey,
  isNonLaunchableUwpHostExe,
  isTauri,
  normalizeStartStatePayload,
  onEvent,
  openAccessibilitySettings,
  openScreenSnip,
  pasteClipboardEntry,
  pickCustomIconImage,
  quitApp,
  recordFrecency,
  refreshIndexes,
  refreshPeripheralBatteries,
  reorderSpaces,
  restoreAllTrayApps,
  restoreTrayApp,
  setAppearanceSettings,
  setMacSettings,
  setSpaceWidgetLayout,
  setSpaceWidgets,
  setTaskbarWidgets,
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
  type NetworkSnapshot,
  type PeripheralBattery,
  type PinItem,
  type QuickTogglesState,
  type SecondaryTaskbarMode,
  type SpaceHeaderWidget,
  type SpaceLayoutKind,
  type SpaceWidgetId,
  type StartShownEvent,
  type StartStateEvent,
  type SteamGame,
  type TelemetryState,
  type TestBarConfig,
  type TileSection,
  type TileSize,
  type WeatherCityOption,
  type WeatherSnapshot,
} from '../lib/ipc';
import {
  detectPlatform,
  extractTargetBasename,
  isMacAppBundle,
  isNativeIconTarget,
} from '../lib/platform';
import {
  buildPinFromSearchItem,
  buildSearchCorpus,
  isPrintableSearchTrigger,
  rankSearchResults,
  resolveSearchLaunchPayload,
  resolveSearchSecondaryAction,
  type RankedSearchResult,
  type SearchCorpusItem,
} from '../lib/searchLogic';
import {
  applyOptimisticMovePinInSpace,
  applyOptimisticPinReorder,
  buildGameComboLaunchPayload,
  completeStartClosing,
  EXPLORER_RESUME_TTL_MS,
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
  resolveSpaceHeaderWidgets,
  settleStartOpen,
  shouldResetExplorerOnReopen,
  START_ENTER_ANIM_MS,
  START_EXIT_ANIM_MS,
  START_OPACITY_ANIM_MS,
  TILE_COLOR_PALETTE,
  type ArmedPowerKind,
  type ExplorerSessionSnapshot,
  type MovePinInSpaceParams,
  type PowerActionKind,
  type SpaceTemplateId,
  type StartAnimState,
} from '../lib/startLogic';
import { useAnimatedPresence } from '../lib/useAnimatedPresence';
import { useReducedMotion } from '../lib/useReducedMotion';
import {
  AppsGridIcon,
  ArrowDownIcon,
  ArrowUpIcon,
  CloseWindowIcon,
  CopyIcon,
  EditIcon,
  ExternalWindowIcon,
  FallbackAppIcon,
  FolderSvg,
  KillIcon,
  LayersIcon,
  MaximizeIcon,
  PinIcon,
  PlusIcon,
  renderCustomTileIcon,
  RestoreIcon,
  SettingsGearIcon,
  ShieldIcon,
  SlidersIcon,
  TerminalIcon,
  TILE_ICON_CATALOG,
  UnpinIcon,
} from './Icons';
import { StartCommandPalette } from './start/StartCommandPalette';
import { StartFileExplorer } from './start/StartFileExplorer';
import { StartHeader } from './start/StartHeader';
import { StartPinDrawer } from './start/StartPinDrawer';
import { StartSettingsOverlay } from './start/StartSettingsOverlay';
import { renderStandaloneWidgetCard, StartSpaces } from './start/StartSpaces';

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
  item: PinItem;
  spaceKey: string;
}

interface EmptyCtxState {
  x: number;
  y: number;
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

function extractTargetAumid(target: string | null | undefined): string {
  const trimmed = (target ?? '').trim().toLowerCase();
  const prefix = 'shell:appsfolder\\';
  return trimmed.startsWith(prefix) ? trimmed.slice(prefix.length).trim() : '';
}

function findMatchingPinInList(item: PinItem, pins: readonly PinItem[]): PinItem | null {
  const normTarget = (item.target ?? '').trim().replace(/\//g, '\\').toLowerCase();
  const normAumid =
    (item.matchAumid ?? '').trim().toLowerCase() || extractTargetAumid(item.target);
  return (
    pins.find((tp) => {
      const tpAumid =
        (tp.matchAumid ?? '').trim().toLowerCase() || extractTargetAumid(tp.target);
      if (normAumid && tpAumid && tpAumid === normAumid) {
        return true;
      }
      if (normTarget && tp.target.trim().replace(/\//g, '\\').toLowerCase() === normTarget) {
        return true;
      }
      return false;
    }) ?? null
  );
}

function findExistingTaskbarPin(item: PinItem, taskbarPins: PinItem[]): PinItem | null {
  return findMatchingPinInList(item, taskbarPins);
}

export const StartView: React.FC = () => {
  const platform = useMemo(() => detectPlatform(), []);
  const isMac = platform === 'macos';
  const [accessibilityTrusted, setAccessibilityTrusted] = useState<boolean | null>(null);
  const [accessBannerDismissed, setAccessBannerDismissed] = useState(false);
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
  const [network, setNetwork] = useState<NetworkSnapshot | null>(null);
  const [quickToggles, setQuickToggles] = useState<QuickTogglesState | null>(null);
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
  const [weatherOpen, setWeatherOpen] = useState(false);
  const [activeHeaderWidgetId, setActiveHeaderWidgetId] = useState<SpaceWidgetId | null>(null);
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

  // Empty-Space Context Menu (rendered outside .start-chassis!)
  const [emptyCtx, setEmptyCtx] = useState<EmptyCtxState | null>(null);
  const [emptyCtxIdx, setEmptyCtxIdx] = useState<number>(0);
  const lastEmptyCtxRef = useRef<EmptyCtxState | null>(null);
  if (emptyCtx) lastEmptyCtxRef.current = emptyCtx;
  const emptyCtxPresence = useAnimatedPresence(Boolean(emptyCtx), 95);

  // Centered Tile Edit Modal (rendered inside .start-chassis!)
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
  const [editTileSize, setEditTileSize] = useState<TileSize>('compact');
  const [editSection, setEditSection] = useState<TileSection>('main');
  const [editRunAsAdmin, setEditRunAsAdmin] = useState<boolean>(false);
  const [pickingCustomImg, setPickingCustomImg] = useState(false);
  const [savingPinEdit, setSavingPinEdit] = useState(false);
  const editTitleInputRef = useRef<HTMLInputElement>(null);

  // Explorer 3-minute resume tracking
  const lastStartClosedAtRef = useRef<number | null>(null);
  const explorerSessionRef = useRef<ExplorerSessionSnapshot | null>(null);
  const activeExplorerPathRef = useRef<string | null>(activeExplorerPath);
  activeExplorerPathRef.current = activeExplorerPath;

  // Command Palette & Search Context Menu
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchTrigger, setSearchTrigger] = useState<'keyboard' | 'click'>('keyboard');
  const [query, setQuery] = useState('');
  const [cmdIdx, setCmdIdx] = useState(0);
  const cmdInputRef = useRef<HTMLInputElement>(null);
  const [searchCtx, setSearchCtx] = useState<{
    x: number;
    y: number;
    item: SearchCorpusItem;
  } | null>(null);
  const [searchCtxIdx, setSearchCtxIdx] = useState<number>(0);
  const lastSearchCtxRef = useRef<{ x: number; y: number; item: SearchCorpusItem } | null>(null);
  if (searchCtx) lastSearchCtxRef.current = searchCtx;
  const searchCtxPresence = useAnimatedPresence(Boolean(searchCtx), 95);

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

  const resolvedSpaces = useMemo(
    () => resolveConfiguredSpaces(config, platform),
    [config, platform],
  );
  const activeSpace = useMemo(
    () => resolvedSpaces.find((s) => s.id === space) ?? resolvedSpaces[0],
    [resolvedSpaces, space],
  );

  // 2-layer ambient aura crossfade state
  const spaceTheme = ['personal', 'work', 'studio', 'games'].includes(space) ? space : 'custom';
  const customAuraRgb =
    config?.accentMode === 'custom' ? normalizeRgbTriplet(config?.accentRgb) : null;
  const targetAuraRgb =
    customAuraRgb ??
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
    if (typeof document !== 'undefined') {
      applyAppearanceToDocument(next);
    }
  }, []);

  const handleOptimisticReorder = useCallback(
    (spaceKey: string, fromIdx: number, toIdx: number) => {
      setConfig((prev) => applyOptimisticPinReorder(prev, spaceKey, fromIdx, toIdx) ?? prev);
    },
    [],
  );

  const handleOptimisticMovePin = useCallback((params: MovePinInSpaceParams) => {
    setConfig((prev) => applyOptimisticMovePinInSpace(prev, params) ?? prev);
  }, []);

  const handleHideBatteryDevice = useCallback(
    async (id: string) => {
      try {
        const updated = await hideBatteryDevice(id);
        handleApplyConfig(updated);
        showToast('Устройство скрыто; вернуть можно в Настройках');
      } catch (err) {
        showToast(formatIpcError(err));
      }
    },
    [handleApplyConfig, showToast],
  );

  const resetTransientOverlays = useCallback(() => {
    setSearchOpen(false);
    setQuery('');
    setSearchCtx(null);
    setPowerOpen(false);
    setArmedPower(null);
    setPeriphOpen(false);
    setWeatherOpen(false);
    setActiveHeaderWidgetId(null);
    setAddingSpace(false);
    setManagingSpaceId(null);
    setWidgetBuilderOpen(false);
    setSettingsOpen(false);
    setDrawerOpen(false);
    setTileCtx(null);
    setEmptyCtx(null);
    setTileEdit(null);
    setShiftHeld(false);
  }, []);

  // Core startup hydration (lightweight, zero thundering herd)
  const hydrateCoreData = useCallback(() => {
    ipc<TestBarConfig>('get_config')
      .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
      .catch(() => {});
    if (!isMac) {
      ipc<AppWindowGroup[]>('get_open_windows').then(setGroups).catch(() => {});
    } else {
      getPlatformInfo()
        .then((info) => setAccessibilityTrusted(info.accessibilityTrusted))
        .catch(() => {
          getAccessibilityStatus()
            .then(setAccessibilityTrusted)
            .catch(() => {});
        });
    }
    ipc<IndexedApp[]>('get_installed_apps').then(setApps).catch(() => {});
  }, [isMac]);

  // Deferred secondary widget hydration (runs on first Start open or calm idle)
  const hydrateSecondaryData = useCallback(() => {
    if (secondaryHydratedRef.current) return;
    secondaryHydratedRef.current = true;
    ipc<IndexedFile[]>('get_indexed_files').then(setFiles).catch(() => {});
    if (!isMac) {
      ipc<SteamGame[]>('get_steam_games').then(setGames).catch(() => {});
      ipc<MediaState>('get_media_state').then(setMedia).catch(() => {});
      ipc<AudioState>('get_audio_state').then(setAudio).catch(() => {});
    }
    ipc<ClipboardEntry[]>('get_clipboard_history').then(setClips).catch(() => {});
    ipc<TelemetryState>('get_telemetry').then(setTelem).catch(() => {});
    getPeripheralBatteries()
      .then(setPeriphs)
      .catch(() => ipc<PeripheralBattery[]>('get_peripherals').then(setPeriphs).catch(() => {}));
  }, [isMac]);

  useEffect(() => {
    hydrateCoreData();
    const idleTimer = window.setTimeout(() => {
      hydrateSecondaryData();
    }, 1400);

    const subs = [
      onEvent<TestBarConfig>('config-updated', (next) =>
        setConfig((prev) => applyConfigUpdate(prev, next)),
      ),
      onEvent<IndexedApp[]>('apps-indexed', setApps),
      onEvent<IndexedFile[]>('files-indexed', setFiles),
      ...(isMac
        ? [
            onEvent<{ trusted?: boolean } | boolean>('accessibility-status-updated', (payload) => {
              const trusted =
                typeof payload === 'boolean' ? payload : Boolean(payload?.trusted);
              setAccessibilityTrusted(trusted);
            }),
          ]
        : [
            onEvent<AppWindowGroup[]>('windows-updated', (next) => {
              if (!startActiveRef.current) {
                pendingHiddenRef.current.groups = next;
                hasPendingHiddenRef.current = true;
                return;
              }
              setGroups(next);
            }),
            onEvent<SteamGame[]>('games-indexed', setGames),
            onEvent<AudioState>('audio-mixer-updated', (incoming) => {
              if (!startActiveRef.current) {
                pendingHiddenRef.current.audio = incoming;
                pendingHiddenRef.current.audioMaster = undefined;
                hasPendingHiddenRef.current = true;
                return;
              }
              setAudio((prev) =>
                mergeAudioStateRespectingLocks(
                  prev,
                  incoming,
                  audioDragLocksRef.current,
                  Date.now(),
                ),
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
          ]),
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
          if (isMac) {
            getAccessibilityStatus()
              .then(setAccessibilityTrusted)
              .catch(() => {});
          }
          setShiftHeld((prev) => (prev ? false : prev));
          const closedAt = lastStartClosedAtRef.current;
          lastStartClosedAtRef.current = null;
          const explorerExpired = shouldResetExplorerOnReopen(
            closedAt,
            Date.now(),
            EXPLORER_RESUME_TTL_MS,
          );
          if (activeExplorerPathRef.current !== null && explorerExpired) {
            setActiveExplorerPath(null);
            explorerSessionRef.current = null;
          }
          const explorerResuming = activeExplorerPathRef.current !== null && !explorerExpired;
          const remember = configRef.current?.startRememberSpace ?? false;
          const configured = resolveConfiguredSpaces(configRef.current, platform);
          const defaultSpaceId = configured[0]?.id ?? 'personal';
          if (!remember && spaceRef.current !== defaultSpaceId && !explorerResuming) {
            setSpaceDir(0);
            setRapidSwitch(false);
            setSpace(defaultSpaceId);
            setWidgetBuilderOpen(false);
            setActiveExplorerPath(null);
            explorerSessionRef.current = null;
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
          lastStartClosedAtRef.current = Date.now();
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
        setWeatherOpen(false);
        setAddingSpace(false);
        setManagingSpaceId(null);
        setWidgetBuilderOpen(false);
        setActiveExplorerPath(null);
        explorerSessionRef.current = null;
        setTileCtx(null);
        setEmptyCtx(null);
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
    isMac,
    platform,
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

  // Always keep header weather fresh while Start is open
  const needsWeather = anim.phase !== 'hidden';
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

  const activeHeaderWidgets = useMemo(
    () => (activeSpace ? resolveSpaceHeaderWidgets(activeSpace) : []),
    [activeSpace],
  );
  const activeSpaceHasNetwork = useMemo(
    () =>
      Boolean(
        activeSpace?.widgets?.includes('network') ||
          activeHeaderWidgets.some((w) => w.id === 'network') ||
          activeHeaderWidgetId === 'network',
      ),
    [activeSpace?.widgets, activeHeaderWidgets, activeHeaderWidgetId],
  );
  const activeSpaceHasQuickToggles = useMemo(
    () =>
      Boolean(
        activeSpace?.widgets?.includes('quick_toggles') ||
          activeHeaderWidgets.some((w) => w.id === 'quick_toggles') ||
          activeHeaderWidgetId === 'quick_toggles',
      ),
    [activeSpace?.widgets, activeHeaderWidgets, activeHeaderWidgetId],
  );

  // Lazy-poll network throughput + TCP ping ONLY while Start is visible & network widget is active
  useEffect(() => {
    if (anim.phase === 'hidden' || !activeSpaceHasNetwork) return;
    let cancelled = false;
    let tick = 0;
    const poll = () => {
      const probePing = tick % 2 === 0;
      tick += 1;
      getNetworkSnapshot(probePing)
        .then((snap) => {
          if (!cancelled && mountedRef.current) {
            setNetwork(snap);
          }
        })
        .catch(() => {});
    };
    poll();
    const timer = window.setInterval(poll, 2500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [anim.phase, activeSpaceHasNetwork]);

  // Lazy-fetch system quick toggles ONLY while Start is visible & quick_toggles widget is active
  useEffect(() => {
    if (anim.phase === 'hidden' || !activeSpaceHasQuickToggles) return;
    let cancelled = false;
    getQuickTogglesState()
      .then((st) => {
        if (!cancelled && mountedRef.current) {
          setQuickToggles(st);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [anim.phase, activeSpaceHasQuickToggles]);

  const handleSelectWeatherCity = useCallback(
    async (city: WeatherCityOption) => {
      try {
        const next = await setTaskbarWidgets({
          weatherCity: city.name,
          weatherLat: city.lat,
          weatherLon: city.lon,
        });
        handleApplyConfig(next);
        lastWeatherKeyRef.current = '';
        const fresh = await getWeather({ force: true });
        if (mountedRef.current) {
          lastWeatherFetchMsRef.current = Date.now();
          setWeather(fresh);
        }
        showToast(`Город погоды: ${city.name}`);
      } catch (err) {
        showToast(formatIpcError(err));
      }
    },
    [handleApplyConfig, showToast],
  );

  const handleResetWeatherCity = useCallback(async () => {
    try {
      const next = await setTaskbarWidgets({
        weatherCity: '',
        weatherLat: null,
        weatherLon: null,
      });
      handleApplyConfig(next);
      lastWeatherKeyRef.current = '';
      const fresh = await getWeather({ force: true });
      if (mountedRef.current) {
        lastWeatherFetchMsRef.current = Date.now();
        setWeather(fresh);
      }
      showToast('Автоопределение города включено');
    } catch (err) {
      showToast(formatIpcError(err));
    }
  }, [handleApplyConfig, showToast]);

  const handleChangeWeatherUnits = useCallback(
    async (units: 'c' | 'f') => {
      try {
        const next = await setTaskbarWidgets({ weatherUnits: units });
        handleApplyConfig(next);
        lastWeatherKeyRef.current = '';
        const fresh = await getWeather({ force: true });
        if (mountedRef.current) {
          lastWeatherFetchMsRef.current = Date.now();
          setWeather(fresh);
        }
      } catch (err) {
        showToast(formatIpcError(err));
      }
    },
    [handleApplyConfig, showToast],
  );

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
        isNativeIconTarget(target) ||
        (!isMac &&
          (low.startsWith('ms-settings:') ||
            low.startsWith('http://') ||
            low.startsWith('https://')));
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
      setSearchCtx(null);
      setDrawerOpen(false);
      setSettingsOpen(false);
      setAddingSpace(false);
      setManagingSpaceId(null);
      setWidgetBuilderOpen(false);
      setActiveExplorerPath(null);
      explorerSessionRef.current = null;
      setPeriphOpen(false);
      setWeatherOpen(false);
      setActiveHeaderWidgetId(null);
      setPowerOpen(false);
      setArmedPower(null);
      setTileCtx(null);
      setEmptyCtx(null);
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
        ? buildSearchCorpus({ config, apps, games, files, iconMap, groups, audio, clips, platform })
        : [],
    [searchOpen, config, apps, games, files, iconMap, groups, audio, clips, platform],
  );

  const searchResults = useMemo<RankedSearchResult>(
    () => (searchOpen ? rankSearchResults(searchCorpus, query, 12, config?.frecency) : EMPTY_RANKED_SEARCH),
    [searchOpen, searchCorpus, query, config?.frecency],
  );

  const dispatchSearchSecondary = useCallback(
    (item: SearchCorpusItem) => {
      const secondary = resolveSearchSecondaryAction(item, platform);
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
    [platform, showToast],
  );

  const dispatchSearchLaunch = useCallback(
    (item: SearchCorpusItem, ctrlKey: boolean, altKey = false) => {
      if (altKey) {
        dispatchSearchSecondary(item);
        return;
      }

      const payload = resolveSearchLaunchPayload(item, ctrlKey, platform);
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

      setSearchCtx(null);
      setSearchOpen(false);
      setQuery('');
    },
    [dispatchSearchSecondary, handleApplyConfig, platform, showToast],
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
      const action = resolvePinAction(item, shiftHeld, platform);
      if (action.type === 'terminal') {
        ipc('open_terminal_at', { path: action.path }).catch((err) =>
          showToast(formatIpcError(err)),
        );
      } else if (action.type === 'copy_path') {
        ipc('copy_to_clipboard', { text: action.text })
          .then(() => showToast(action.toast))
          .catch((err) => showToast(formatIpcError(err)));
      } else if (action.type === 'launch') {
        if (action.target === 'cmd:settings') {
          setSettingsOpen(true);
          setDrawerOpen(false);
          return;
        }
        ipc('launch_item', {
          target: action.target,
          args: action.args,
          asAdmin: !isMac && Boolean(action.asAdmin || item.runAsAdmin),
          closeStart: action.closeStart,
        })
          .then(() => {
            const normTarget = isMac
              ? item.target.trim().toLowerCase()
              : item.target.trim().replace(/\//g, '\\').toLowerCase();
            recordFrecency(`target:${normTarget || item.id}`).then(handleApplyConfig).catch(() => {});
          })
          .catch((err) => showToast(formatIpcError(err)));
      }
    },
    [editMode, shiftHeld, showToast, handleApplyConfig, isMac, platform],
  );

  // Centered Tile Edit Modal handlers
  const openTileEditPopover = useCallback(
    (item: PinItem, spaceKey: string, _coords?: { x: number; y: number }) => {
      setTileCtx(null);
      setEmptyCtx(null);
      setEditPinTitle(item.title);
      setEditPinArgs(item.args ?? '');
      setEditPinAltAction(item.altAction ?? '');
      setEditPinAltLabel(item.altLabel ?? '');
      setEditCustomColor(item.customColor ?? null);
      setEditCustomIcon(item.customIcon ?? null);
      setEditIconData(item.customIcon === 'custom_image' ? (item.iconData ?? null) : null);
      setEditTileSize(
        item.tileSize === 'hero'
          ? 'hero'
          : item.tileSize === 'small'
            ? 'small'
            : 'compact',
      );
      setEditSection(item.section === 'bottom' ? 'bottom' : 'main');
      setEditRunAsAdmin(Boolean(item.runAsAdmin));
      setPickingCustomImg(false);
      setSavingPinEdit(false);
      setTileEdit({ item, spaceKey });
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
        tileSize: editTileSize,
        section: editSection,
        runAsAdmin: tileEdit.item.kind === 'app' ? (isMac ? false : editRunAsAdmin) : undefined,
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
          explorerSessionRef.current = null;
          setActiveExplorerPath(item.target);
          setTileCtx(null);
        },
      });
      actions.push({
        id: 'open-explorer',
        label: isMac ? 'Открыть в Finder' : 'Открыть в Проводнике Windows',
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

    if (item.kind === 'app' && canRunAsAdmin(item.target, platform)) {
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
        label: isMac ? 'Показать в Finder' : 'Расположение файла',
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
          openTileEditPopover(item, spaceKey);
        },
      });
    }

    if (spaceKey !== 'game_combos' && item.kind !== 'combo') {
      const curSize =
        item.tileSize === 'hero'
          ? 'hero'
          : item.tileSize === 'small'
            ? 'small'
            : 'compact';
      const sizeOptions = [
        {
          size: 'small' as const,
          label: 'Сделать маленькой',
          toast: 'Маленькая плитка',
          icon: <AppsGridIcon className="ic14" />,
        },
        {
          size: 'compact' as const,
          label: 'Сделать средней',
          toast: 'Средняя плитка',
          icon: <RestoreIcon className="ic14" />,
        },
        {
          size: 'hero' as const,
          label: 'Сделать большой',
          toast: 'Большая плитка',
          icon: <MaximizeIcon className="ic14" />,
        },
      ];
      for (const opt of sizeOptions) {
        if (opt.size === curSize) continue;
        actions.push({
          id: `size-${opt.size}`,
          label: opt.label,
          icon: opt.icon,
          run: () => {
            updatePin({
              space: spaceKey,
              pinId: item.id,
              tileSize: opt.size,
            })
              .then((next) => {
                handleApplyConfig(next);
                showToast(opt.toast);
              })
              .catch((err) => showToast(formatIpcError(err)));
            setTileCtx(null);
          },
        });
      }

      const isBottomTile = item.section === 'bottom';
      actions.push({
        id: 'toggle-section',
        label: isBottomTile ? 'Перенести в верхний блок' : 'Перенести в нижний блок',
        icon: isBottomTile ? <ArrowUpIcon size={14} /> : <ArrowDownIcon size={14} />,
        run: () => {
          updatePin({
            space: spaceKey,
            pinId: item.id,
            section: isBottomTile ? 'main' : 'bottom',
          })
            .then((next) => {
              handleApplyConfig(next);
              showToast(
                isBottomTile ? 'Перенесено в верхний блок' : 'Перенесено в нижний блок',
              );
            })
            .catch((err) => showToast(formatIpcError(err)));
          setTileCtx(null);
        },
      });
    }

    if (!isMac && item.kind === 'app') {
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
  }, [
    activeCtxSource,
    config?.taskbarPins,
    handleApplyConfig,
    isMac,
    launchPin,
    openTileEditPopover,
    platform,
    showToast,
  ]);

  const emptyCtxActions = useMemo<TileCtxAction[]>(() => {
    const currentSpaceName = activeSpace?.name ?? 'Пространство';
    return [
      {
        id: 'add-pin',
        label: 'Добавить программу или папку',
        icon: <PlusIcon className="ic14" />,
        run: () => {
          setEmptyCtx(null);
          setDrawerOpen(true);
        },
      },
      {
        id: 'toggle-edit',
        label: editMode ? 'Выключить режим редактирования' : 'Режим редактирования',
        icon: <EditIcon className="ic14" />,
        run: () => {
          setEmptyCtx(null);
          setEditMode((v) => !v);
        },
      },
      {
        id: 'widgets',
        label: 'Настроить виджеты',
        icon: <SlidersIcon className="ic14" />,
        run: () => {
          setEmptyCtx(null);
          setWidgetBuilderOpen((v) => !v);
        },
      },
      {
        id: 'manage-space',
        label: `Настроить пространство «${currentSpaceName}»`,
        icon: <LayersIcon className="ic14" />,
        run: () => {
          setEmptyCtx(null);
          setAddingSpace(false);
          setManagingSpaceId(space);
        },
      },
      {
        id: 'create-space',
        label: 'Создать пространство',
        icon: <PlusIcon className="ic14" />,
        run: () => {
          setEmptyCtx(null);
          setManagingSpaceId(null);
          setAddingSpace(true);
        },
      },
      {
        id: 'settings',
        label: 'Настройки TestBar',
        icon: <SettingsGearIcon className="ic14" />,
        run: () => {
          setEmptyCtx(null);
          setSettingsOpen(true);
        },
      },
    ];
  }, [activeSpace?.name, editMode, space]);

  // Dismiss Search Context Menu when clicking anywhere outside it
  useEffect(() => {
    if (!searchCtx) return;
    const onPointerDown = (e: PointerEvent) => {
      if ((e.target as Element | null)?.closest('.search-ctx-menu')) return;
      setSearchCtx(null);
    };
    window.addEventListener('pointerdown', onPointerDown, true);
    return () => window.removeEventListener('pointerdown', onPointerDown, true);
  }, [searchCtx]);

  // Search Result Context Menu actions
  const activeSearchCtxSource = searchCtx ?? lastSearchCtxRef.current;
  const searchCtxActions = useMemo<TileCtxAction[]>(() => {
    if (!activeSearchCtxSource) return [];
    const { item } = activeSearchCtxSource;
    const actions: TileCtxAction[] = [];

    const primaryLabel =
      item.source === 'window'
        ? 'Перейти к окну'
        : item.source === 'clipboard'
          ? 'Вставить'
          : item.source === 'audio_device'
            ? 'Переключить'
            : item.source === 'command'
              ? 'Выполнить'
              : 'Открыть';

    actions.push({
      id: 'open',
      label: primaryLabel,
      icon: item.kind === 'folder' ? <FolderSvg className="ic14" /> : <ExternalWindowIcon className="ic14" />,
      run: () => {
        dispatchSearchLaunch(item, false, false);
      },
    });

    const pinCandidate = buildPinFromSearchItem(item, undefined, platform);
    if (pinCandidate) {
      if (!isMac) {
        const existingTbPin = findMatchingPinInList(pinCandidate, config?.taskbarPins ?? []);
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
              setSearchCtx(null);
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
                item: { ...pinCandidate, id: `tb-${Date.now()}`, iconData: null },
              })
                .then((next) => {
                  handleApplyConfig(next);
                  showToast('Закреплено на панели задач');
                })
                .catch((err) => showToast(formatIpcError(err)));
              setSearchCtx(null);
            },
          });
        }
      }

      if (activeSpace && !findMatchingPinInList(pinCandidate, activeSpace.items)) {
        actions.push({
          id: 'pin-space',
          label: `Закрепить в пространстве «${activeSpace.name}»`,
          icon: <PinIcon className="ic14" />,
          run: () => {
            ipc<TestBarConfig>('add_pin', {
              space: activeSpace.id,
              item: {
                ...pinCandidate,
                id: `pin-${Date.now()}`,
                iconData: null,
                tileSize: config?.defaultTileSize ?? 'compact',
              },
            })
              .then((next) => {
                handleApplyConfig(next);
                showToast(`Закреплено: ${pinCandidate.title}`);
              })
              .catch((err) => showToast(formatIpcError(err)));
            setSearchCtx(null);
          },
        });
      }
    }

    if (
      item.source !== 'win_settings' &&
      item.source !== 'command' &&
      item.source !== 'clipboard' &&
      item.source !== 'audio_device' &&
      item.source !== 'web' &&
      item.kind !== 'folder' &&
      item.kind !== 'combo' &&
      !isNonLaunchableUwpHostExe(item.target) &&
      canRunAsAdmin(item.target, platform)
    ) {
      actions.push({
        id: 'admin',
        label: 'Запустить от администратора',
        icon: <ShieldIcon className="ic14" />,
        run: () => {
          ipc('launch_item', {
            target: item.target,
            args: item.args ?? null,
            asAdmin: true,
            closeStart: true,
          }).catch((err) => showToast(formatIpcError(err)));
          setSearchCtx(null);
          setSearchOpen(false);
          setQuery('');
        },
      });
    }

    if (item.kind === 'folder') {
      actions.push({
        id: 'open-testbar',
        label: 'Открыть в TestBar',
        icon: <FolderSvg className="ic14" />,
        run: () => {
          setSearchCtx(null);
          setSearchOpen(false);
          setQuery('');
          setWidgetBuilderOpen(false);
          setActiveExplorerPath(item.target);
        },
      });

      if (canOpenFileLocation(item.target)) {
        actions.push({
          id: 'open-explorer',
          label: isMac ? 'Открыть в Finder' : 'Открыть в Проводнике Windows',
          icon: <ExternalWindowIcon className="ic14" />,
          run: () => {
            fsOpenInExplorer(item.target).catch((err) => showToast(formatIpcError(err)));
            setSearchCtx(null);
            setSearchOpen(false);
            setQuery('');
          },
        });
        actions.push({
          id: 'terminal',
          label: 'Открыть в терминале',
          icon: <TerminalIcon className="ic14" />,
          run: () => {
            ipc('open_terminal_at', { path: item.target }).catch((err) =>
              showToast(formatIpcError(err)),
            );
            setSearchCtx(null);
            setSearchOpen(false);
            setQuery('');
          },
        });
      }
    } else if (
      item.source !== 'win_settings' &&
      item.source !== 'command' &&
      item.source !== 'clipboard' &&
      item.source !== 'audio_device' &&
      item.source !== 'web' &&
      canOpenFileLocation(item.target)
    ) {
      actions.push({
        id: 'location',
        label: isMac ? 'Показать в Finder' : 'Расположение файла',
        icon: <FolderSvg className="ic14" />,
        run: () => {
          ipc('open_location', { path: item.target }).catch((err) =>
            showToast(formatIpcError(err)),
          );
          setSearchCtx(null);
          setSearchOpen(false);
          setQuery('');
        },
      });
    }

    if (
      item.source !== 'clipboard' &&
      item.source !== 'audio_device' &&
      item.target.trim() &&
      !item.target.trim().toLowerCase().startsWith('cmd:')
    ) {
      actions.push({
        id: 'copy',
        label: 'Скопировать путь',
        icon: <CopyIcon className="ic14" />,
        run: () => {
          ipc('copy_to_clipboard', { text: item.target })
            .then(() => showToast('Путь скопирован'))
            .catch((err) => showToast(formatIpcError(err)));
          setSearchCtx(null);
        },
      });
    }

    return actions;
  }, [
    activeSearchCtxSource,
    activeSpace,
    config?.taskbarPins,
    dispatchSearchLaunch,
    handleApplyConfig,
    isMac,
    platform,
    showToast,
  ]);

  // Global keyboard handler: zero-loss type-anywhere search, Ctrl+1..9, Tab, tileCtx nav, Esc, Shift
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Shift') setShiftHeld(true);
      if (e.defaultPrevented) return;

      // Ctrl+1..9 direct Space switcher (works unless actively typing inside a modal input)
      const activeEl = document.activeElement as HTMLElement | null;
      const activeTag = activeEl?.tagName;
      const typingInField =
        (activeTag === 'INPUT' || activeTag === 'TEXTAREA' || activeTag === 'SELECT') &&
        activeEl !== cmdInputRef.current;

      const spaceShortcutModifier =
        (e.ctrlKey && !e.metaKey) || (isMac && e.metaKey && !e.ctrlKey);
      if (
        spaceShortcutModifier &&
        !e.altKey &&
        e.key >= '1' &&
        e.key <= '9' &&
        !typingInField
      ) {
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

      // Search Result Context Menu keyboard navigation
      if (searchCtx) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setSearchCtxIdx((i) =>
            searchCtxActions.length > 0 ? (i + 1) % searchCtxActions.length : 0,
          );
          return;
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          setSearchCtxIdx((i) =>
            searchCtxActions.length > 0
              ? (i - 1 + searchCtxActions.length) % searchCtxActions.length
              : 0,
          );
          return;
        }
        if (e.key === 'Enter' && searchCtxActions[searchCtxIdx]) {
          e.preventDefault();
          searchCtxActions[searchCtxIdx].run();
          return;
        }
        if (e.key === 'Escape') {
          e.preventDefault();
          setSearchCtx(null);
          return;
        }
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

      // Empty-Space Context Menu keyboard navigation
      if (emptyCtx) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setEmptyCtxIdx((i) => (emptyCtxActions.length > 0 ? (i + 1) % emptyCtxActions.length : 0));
          return;
        }
        if (e.key === 'ArrowUp') {
          e.preventDefault();
          setEmptyCtxIdx((i) =>
            emptyCtxActions.length > 0
              ? (i - 1 + emptyCtxActions.length) % emptyCtxActions.length
              : 0,
          );
          return;
        }
        if (e.key === 'Enter' && emptyCtxActions[emptyCtxIdx]) {
          e.preventDefault();
          emptyCtxActions[emptyCtxIdx].run();
          return;
        }
        if (e.key === 'Escape') {
          e.preventDefault();
          setEmptyCtx(null);
          return;
        }
      }

      if (e.key === 'Escape') {
        if (tileEdit) {
          setTileEdit(null);
        } else if (emptyCtx) {
          setEmptyCtx(null);
        } else if (searchCtx) {
          setSearchCtx(null);
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
        } else if (activeHeaderWidgetId) {
          setActiveHeaderWidgetId(null);
        } else if (weatherOpen) {
          setWeatherOpen(false);
        } else if (periphOpen) {
          setPeriphOpen(false);
        } else if (powerOpen) {
          setPowerOpen(false);
          setArmedPower(null);
        } else if (activeExplorerPath !== null) {
          explorerSessionRef.current = null;
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
        !activeHeaderWidgetId &&
        !periphOpen &&
        !weatherOpen &&
        !powerOpen &&
        !tileCtx &&
        !emptyCtx &&
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
        !activeHeaderWidgetId &&
        !weatherOpen &&
        !emptyCtx &&
        activeExplorerPath === null &&
        !tileEdit &&
        !insideDialog &&
        isPrintableSearchTrigger(e)
      ) {
        e.preventDefault();
        setTileCtx(null);
        setEmptyCtx(null);
        setPeriphOpen(false);
        setWeatherOpen(false);
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
    weatherOpen,
    searchCtx,
    searchCtxIdx,
    searchCtxActions,
    tileCtx,
    tileCtxIdx,
    tileCtxActions,
    emptyCtx,
    emptyCtxIdx,
    emptyCtxActions,
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
      const baseName = picked.split(/[\\/]/).filter(Boolean).pop() || picked;
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
          tileSize: config?.defaultTileSize ?? 'compact',
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
      const baseName = picked.split(/[\\/]/).filter(Boolean).pop() || picked;
      const lower = picked.toLowerCase();
      const isExe = lower.endsWith('.exe');
      const isMacApp = isMac && isMacAppBundle(picked);
      const isApp = isExe || isMacApp;
      const hasSpaces = Boolean(config?.spaces && config.spaces.length > 0);
      const targetSpace = hasSpaces
        ? space
        : space === 'personal'
          ? isApp
            ? 'personal'
            : 'personal_links'
          : space;
      const title = isApp ? extractTargetBasename(picked) || baseName : baseName;
      const updated = await ipc<TestBarConfig>('add_pin', {
        space: targetSpace,
        item: {
          id: `pin-${Date.now()}`,
          kind: isApp ? 'app' : 'file',
          title,
          subtitle: isApp ? 'Программа' : 'Файл',
          target: picked,
          altAction: isMacApp
            ? ''
            : isExe
              ? isMac
                ? ''
                : 'От имени администратора'
              : 'Скопировать путь',
          accentRgb: '82,184,242',
          iconKey: isApp ? baseName.toLowerCase() : 'file',
          iconData: null,
          tileSize: config?.defaultTileSize ?? 'compact',
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
        tileSize: config?.defaultTileSize ?? 'compact',
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
          altAction: isUwp || isMac ? '' : 'От имени администратора',
          accentRgb: '139,150,248',
          iconKey: app.iconKey || app.name.toLowerCase(),
          iconData: null,
          matchAumid: app.aumid ?? null,
          tileSize: config?.defaultTileSize ?? 'compact',
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
    bottomSectionTitle?: string | null;
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
      const fallback = resolveConfiguredSpaces(updated, platform)[0]?.id ?? 'personal';
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

  const handleSetSpaceWidgetLayout = async (
    spaceId: string,
    widgets: SpaceWidgetId[],
    headerWidgets: SpaceHeaderWidget[],
  ) => {
    try {
      const updated = await setSpaceWidgetLayout(spaceId, widgets, headerWidgets);
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
      explorerSessionRef.current = null;
      setManagingSpaceId(null);
      setAddingSpace(false);
      setActiveHeaderWidgetId(null);
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
      const pos = clampPopoverPos(e.clientX, e.clientY, 252, 380);
      setTileEdit(null);
      setEmptyCtx(null);
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

  const openSearchContextMenu = useCallback(
    (e: React.MouseEvent, item: SearchCorpusItem) => {
      e.preventDefault();
      e.stopPropagation();
      const pos = clampPopoverPos(e.clientX, e.clientY, 244, 320);
      setTileCtx(null);
      setEmptyCtx(null);
      setTileEdit(null);
      setSearchCtxIdx(0);
      setSearchCtx({
        x: pos.x,
        y: pos.y,
        item,
      });
    },
    [],
  );

  const isCustomAccent = config?.accentMode === 'custom';
  const activeAccentHex = activeSpace?.accentHex?.trim() || '#52B8F2';
  const activeAccentRgb = normalizeRgbTriplet(activeSpace?.accentRgb) ?? '82, 184, 242';
  const spaceAccentStyle = useMemo(
    () =>
      isCustomAccent
        ? ({} as React.CSSProperties)
        : ({
            ['--accent' as string]: activeAccentHex,
            ['--accent-rgb' as string]: activeAccentRgb,
          } as React.CSSProperties),
    [isCustomAccent, activeAccentHex, activeAccentRgb],
  );

  const startVisible = anim.phase !== 'hidden';
  const startChassisAnimCls = getStartChassisAnimClass(anim);

  return (
    <div
      className={`start-root ${editMode ? 'edit-mode' : ''} ${shiftHeld ? 'shift-active' : ''}`}
      data-space={spaceTheme}
      data-visible={startVisible ? 'true' : 'false'}
      style={spaceAccentStyle}
      onClick={(e) => {
        if (searchCtx) setSearchCtx(null);
        if (tileCtx) setTileCtx(null);
        if (emptyCtx) setEmptyCtx(null);
        if (tileEdit) setTileEdit(null);
        const target = e.target as HTMLElement | null;
        if (
          activeHeaderWidgetId &&
          !target?.closest('.hdr-widget-pop, .hdr-overflow-pop, .hdr-widget-pill')
        ) {
          setActiveHeaderWidgetId(null);
        }
        if (powerOpen) {
          setPowerOpen(false);
          setArmedPower(null);
        }
      }}
      onContextMenu={(e) => {
        if (e.defaultPrevented) return;
        const target = e.target as HTMLElement | null;
        if (
          target?.closest(
            'input, textarea, select, [contenteditable="true"], [contenteditable=""]',
          )
        ) {
          return;
        }
        e.preventDefault();
        if (
          target?.closest(
            '.app-tile, .qlink, .game-card, .combo-row, .space-tab, .v3-explorer-slot, .search-backdrop, .settings-backdrop, .tile-edit-backdrop, .ctx-card, .tile-ctx-menu, .periph-pop, .weather-hdr-pop, .power-pop, .space-pop, .hdr-widget-pop, .hdr-overflow-pop, .hdr-widget-pill, .drawer-sheet, .widget-builder-sheet, .combo-builder-sheet',
          )
        ) {
          return;
        }
        const pos = clampPopoverPos(e.clientX, e.clientY, 248, 250);
        setTileCtx(null);
        setSearchCtx(null);
        setPeriphOpen(false);
        setWeatherOpen(false);
        setPowerOpen(false);
        setArmedPower(null);
        setActiveHeaderWidgetId(null);
        setEmptyCtxIdx(0);
        setEmptyCtx({ x: pos.x, y: pos.y });
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
          platform={platform}
          space={space}
          spaces={resolvedSpaces}
          headerWidgets={activeHeaderWidgets}
          periphs={periphs}
          periphsRefreshing={periphsRefreshing}
          hiddenBatteryIds={config?.hiddenBatteryIds}
          audio={audio}
          media={media}
          telem={telem}
          clips={clips}
          network={network}
          quickNotes={config?.quickNotes ?? null}
          quickToggles={quickToggles}
          activeHeaderWidgetId={activeHeaderWidgetId}
          periphOpen={periphOpen}
          weather={weather}
          weatherRefreshing={weatherRefreshing}
          weatherUnits={config?.taskbarWidgets?.weatherUnits ?? 'c'}
          weatherCityConfig={{
            city: config?.taskbarWidgets?.weatherCity ?? '',
            lat: config?.taskbarWidgets?.weatherLat ?? null,
            lon: config?.taskbarWidgets?.weatherLon ?? null,
          }}
          weatherOpen={weatherOpen}
          editMode={editMode}
          drawerOpen={drawerOpen}
          settingsOpen={settingsOpen}
          searchOpen={searchOpen}
          powerOpen={powerOpen}
          armedPower={armedPower}
          addingSpace={addingSpace}
          managingSpaceId={managingSpaceId}
          onSelectSpace={selectSpace}
          onToggleHeaderWidget={(wid) => {
            setActiveHeaderWidgetId((cur) => (cur === wid ? null : wid));
            setPeriphOpen(false);
            setWeatherOpen(false);
            setPowerOpen(false);
            setArmedPower(null);
            setAddingSpace(false);
            setManagingSpaceId(null);
          }}
          onTogglePeriph={() => {
            setPeriphOpen((v) => !v);
            setWeatherOpen(false);
            setActiveHeaderWidgetId(null);
          }}
          onClosePeriph={() => setPeriphOpen(false)}
          onRefreshPeriphs={handleRefreshPeriphs}
          onHideBatteryDevice={handleHideBatteryDevice}
          onToggleWeather={() => {
            setWeatherOpen((v) => !v);
            setPeriphOpen(false);
            setActiveHeaderWidgetId(null);
          }}
          onCloseWeather={() => setWeatherOpen(false)}
          onRefreshWeather={handleRefreshWeather}
          onSelectWeatherCity={handleSelectWeatherCity}
          onResetWeatherCity={handleResetWeatherCity}
          onChangeWeatherUnits={handleChangeWeatherUnits}
          onSelectAudioDevice={(deviceId) => {
            ipc<AudioState>('set_audio_device', { deviceId })
              .then(setAudio)
              .catch((err) => showToast(formatIpcError(err)));
          }}
          onSetMedia={setMedia}
          onSetAudio={handleSetAudioFromChild}
          onSetNetwork={setNetwork}
          onSetQuickToggles={setQuickToggles}
          onConfigUpdated={handleApplyConfig}
          onShowToast={showToast}
          renderHeaderWidgetPopover={(wid) =>
            renderStandaloneWidgetCard(wid, {
              mode: 'popover',
              spaceId: space,
              activeSpace,
              config,
              games,
              apps,
              audio,
              media,
              telem,
              clips,
              periphs,
              periphsRefreshing,
              weather,
              weatherRefreshing,
              network,
              quickToggles,
              startVisible,
              startAnimSettled: anim.phase === 'open',
              iconMap,
              onSetAudio: handleSetAudioFromChild,
              onSetMedia: setMedia,
              onSetTelem: setTelem,
              onSetClips: setClips,
              onSetNetwork: setNetwork,
              onSetQuickToggles: setQuickToggles,
              onRefreshWeather: handleRefreshWeather,
              onRefreshPeriphs: handleRefreshPeriphs,
              onResolveIconKeys: handleResolveIconKeys,
              onSetConfig: handleApplyConfig,
              onAudioDragStateChange: handleAudioDragStateChange,
              onHideBatteryDevice: handleHideBatteryDevice,
              onShowToast: showToast,
              onLaunchPin: launchPin,
              onOpenTileContext: openTileContextMenu,
            })
          }
          onOpenSearch={() => {
            setSearchTrigger('click');
            setSearchOpen(true);
            setDrawerOpen(false);
            setSettingsOpen(false);
            setAddingSpace(false);
            setManagingSpaceId(null);
            setWidgetBuilderOpen(false);
            setWeatherOpen(false);
            setPeriphOpen(false);
            setActiveHeaderWidgetId(null);
            setTileCtx(null);
            setEmptyCtx(null);
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
            setWeatherOpen(false);
            setPeriphOpen(false);
            setActiveHeaderWidgetId(null);
            setTileCtx(null);
            setEmptyCtx(null);
            setTileEdit(null);
          }}
          onToggleSettings={() => {
            setSettingsOpen((v) => !v);
            setDrawerOpen(false);
            setSearchOpen(false);
            setAddingSpace(false);
            setManagingSpaceId(null);
            setWidgetBuilderOpen(false);
            setWeatherOpen(false);
            setPeriphOpen(false);
            setActiveHeaderWidgetId(null);
            setTileCtx(null);
            setEmptyCtx(null);
            setTileEdit(null);
          }}
          onTogglePower={() => {
            setPowerOpen((v) => !v);
            setArmedPower(null);
            setWeatherOpen(false);
            setPeriphOpen(false);
            setActiveHeaderWidgetId(null);
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
              setWeatherOpen(false);
              setActiveHeaderWidgetId(null);
              setTileCtx(null);
              setEmptyCtx(null);
              setTileEdit(null);
            }
          }}
          onSetManagingSpaceId={(id) => {
            setManagingSpaceId(id);
            if (id) {
              setAddingSpace(false);
              setPeriphOpen(false);
              setWeatherOpen(false);
              setActiveHeaderWidgetId(null);
              setTileCtx(null);
              setEmptyCtx(null);
              setTileEdit(null);
            }
          }}
          onCreateSpace={handleCreateSpace}
          onUpdateSpace={handleUpdateSpace}
          onReorderSpaces={handleReorderSpaces}
          onDeleteSpace={handleDeleteSpace}
          onOpenWidgetBuilder={handleOpenWidgetBuilder}
        />

        {isMac && accessibilityTrusted === false && !accessBannerDismissed && (
          <div className="mac-ax-banner" role="status">
            <div className="mac-ax-banner-lead">
              <ShieldIcon className="ic14" />
              <span>
                Чтобы открывать TestBar одиночным нажатием ⌘ Command, разреши доступ в Настройках
              </span>
            </div>
            <div className="mac-ax-banner-actions">
              <button
                type="button"
                className="mac-ax-banner-btn"
                onClick={() => {
                  openAccessibilitySettings().catch((err) => showToast(formatIpcError(err)));
                }}
              >
                Открыть настройки
              </button>
              <button
                type="button"
                className="mac-ax-banner-close"
                aria-label="Скрыть уведомление"
                onClick={() => setAccessBannerDismissed(true)}
              >
                <CloseWindowIcon className="ic12" />
              </button>
            </div>
          </div>
        )}

        <div
          className="space-stage"
          onClick={() => {
            if (periphOpen) setPeriphOpen(false);
            if (weatherOpen) setWeatherOpen(false);
            if (activeHeaderWidgetId) setActiveHeaderWidgetId(null);
            if (powerOpen) {
              setPowerOpen(false);
              setArmedPower(null);
            }
            if (tileCtx) setTileCtx(null);
            if (emptyCtx) setEmptyCtx(null);
            if (tileEdit) setTileEdit(null);
          }}
        >
          {activeExplorerPath !== null ? (
            <div className="v3-explorer-slot" data-explorer-path={activeExplorerPath}>
              <StartFileExplorer
                platform={platform}
                initialPath={activeExplorerPath}
                initialSession={explorerSessionRef.current}
                onSessionChange={(snap) => {
                  explorerSessionRef.current = snap;
                }}
                activeSpaceId={space}
                onClose={() => {
                  explorerSessionRef.current = null;
                  setActiveExplorerPath(null);
                }}
                onPinFolder={async (folderPath, folderName) => {
                  try {
                    const next = await ipc<TestBarConfig>('add_pin', {
                      space,
                      item: {
                        id: `pin-${Date.now()}`,
                        kind: 'folder',
                        title: folderName || folderPath,
                        subtitle: 'Папка',
                        target: folderPath,
                        altAction: 'Открыть в терминале',
                        accentRgb: activeSpace?.accentRgb || '56,189,248',
                        iconKey: 'folder',
                        iconData: null,
                        tileSize: config?.defaultTileSize ?? 'compact',
                      },
                    });
                    handleApplyConfig(next);
                    showToast(`Закреплено: ${folderName || folderPath}`);
                  } catch (err) {
                    showToast(formatIpcError(err));
                  }
                }}
                onShowToast={showToast}
              />
            </div>
          ) : activeSpace ? (
            <StartSpaces
              platform={platform}
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
              network={network}
              quickToggles={quickToggles}
              editMode={editMode}
              shiftHeld={shiftHeld}
              widgetBuilderOpen={widgetBuilderOpen}
              isRunning={isRunning}
              resolveIconForPin={resolveIconForPin}
              onLaunchPin={launchPin}
              onOpenFolder={(path) => {
                setWidgetBuilderOpen(false);
                explorerSessionRef.current = null;
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
              onSetNetwork={setNetwork}
              onSetQuickToggles={setQuickToggles}
              onShowToast={showToast}
              onRefreshWeather={handleRefreshWeather}
              onRefreshPeriphs={handleRefreshPeriphs}
              onToggleWidgetBuilder={(open) => {
                if (open) setActiveHeaderWidgetId(null);
                setWidgetBuilderOpen(open);
              }}
              onSetSpaceWidgets={handleSetSpaceWidgets}
              onSetSpaceWidgetLayout={handleSetSpaceWidgetLayout}
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
              onOptimisticMovePin={handleOptimisticMovePin}
              onHideBatteryDevice={handleHideBatteryDevice}
              onEditPin={(item, spaceKey) => openTileEditPopover(item, spaceKey)}
            />
          ) : null}
        </div>

        <StartPinDrawer
          platform={platform}
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
          platform={platform}
          accessibilityTrusted={accessibilityTrusted}
          onOpenAccessibilitySettings={openAccessibilitySettings}
          onSetMacSettings={async (params) => {
            const next = await setMacSettings(params);
            handleApplyConfig(next);
          }}
          onQuitApp={() => {
            quitApp().catch((err) => showToast(formatIpcError(err)));
          }}
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
          onSetTaskbarWidgets={async (params) => {
            const next = await setTaskbarWidgets(params);
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
          onOpenSpaceWidgets={(targetId) => {
            setSettingsOpen(false);
            if (spaceRef.current !== targetId) selectSpace(targetId);
            setActiveExplorerPath(null);
            explorerSessionRef.current = null;
            setManagingSpaceId(null);
            setAddingSpace(false);
            setActiveHeaderWidgetId(null);
            setWidgetBuilderOpen(true);
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
          onQueryChange={(next) => {
            setQuery(next);
            setSearchCtx(null);
          }}
          onSelectIdx={setCmdIdx}
          onLaunch={dispatchSearchLaunch}
          onItemContextMenu={openSearchContextMenu}
          onClose={() => {
            setSearchCtx(null);
            setSearchOpen(false);
            setQuery('');
          }}
        />

        {/* CENTERED TILE EDIT MODAL — Rendered INSIDE .start-chassis, fixed header + scrollable body + sticky footer! */}
        {tileEditPresence.mounted && (tileEdit ?? lastTileEditRef.current) && (
          <div
            className="tile-edit-backdrop"
            data-phase={tileEditPresence.phase}
            onClick={(e) => {
              e.stopPropagation();
              if (e.target === e.currentTarget) setTileEdit(null);
            }}
          >
            <div
              className="tile-edit-modal"
              data-phase={tileEditPresence.phase}
              role="dialog"
              aria-modal="true"
              aria-label="Настроить плитку"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="tile-edit-head">
                <span className="tile-edit-head-title">Настроить плитку</span>
                <button
                  type="button"
                  className="tile-edit-close-btn"
                  aria-label="Закрыть"
                  onClick={() => setTileEdit(null)}
                >
                  <CloseWindowIcon className="ic14" />
                </button>
              </div>

              <div className="tile-edit-body">
                {(() => {
                  const editCtx = (tileEdit ?? lastTileEditRef.current)!;
                  const previewRgb = resolveCustomColorRgb(editCustomColor);
                  const isFolder = editCtx.item.kind === 'folder';
                  const isFile = editCtx.item.kind === 'file';
                  const isCustomImg = editCustomIcon === 'custom_image' && Boolean(editIconData);
                  const previewIcCls = [
                    'tile-edit-preview-ic',
                    isFolder && !isCustomImg ? 'is-folder-ic' : '',
                    isFile && !isCustomImg ? 'is-file-ic' : '',
                  ]
                    .filter(Boolean)
                    .join(' ');
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
                      <div className={previewIcCls}>
                        {isCustomImg && editIconData ? (
                          <img className="custom-tile-img" src={editIconData} alt="" />
                        ) : editCustomIcon && TILE_ICON_CATALOG.some((c) => c.id === editCustomIcon) ? (
                          renderCustomTileIcon(editCustomIcon, 'ic18')
                        ) : fallbackIconSrc ? (
                          <img src={fallbackIconSrc} alt="" />
                        ) : (
                          <FallbackAppIcon
                            iconKey={editCtx.item.iconKey}
                            kind={editCtx.item.kind}
                            target={editCtx.item.target}
                            size={18}
                            color={editCustomColor ?? undefined}
                          />
                        )}
                      </div>
                      <div className="tile-edit-preview-meta">
                        <span className="tile-edit-preview-name">
                          {editPinTitle.trim() || editCtx.item.title}
                        </span>
                        <span className="tile-edit-preview-sub">
                          {editTileSize === 'hero'
                            ? 'Большая'
                            : editTileSize === 'small'
                              ? 'Маленькая'
                              : 'Средняя'}{' '}
                          · {editSection === 'bottom' ? 'Нижний блок' : 'Верхний блок'}
                        </span>
                      </div>
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

                <div className="tile-edit-row-2">
                  <div className="tile-edit-field">
                    <span className="tile-edit-lbl">Размер плитки</span>
                    <div className="space-seg" role="radiogroup" aria-label="Размер плитки">
                      <button
                        type="button"
                        role="radio"
                        aria-checked={editTileSize === 'small'}
                        className={`space-seg-btn ${editTileSize === 'small' ? 'active' : ''}`}
                        onClick={() => setEditTileSize('small')}
                      >
                        Маленькая
                      </button>
                      <button
                        type="button"
                        role="radio"
                        aria-checked={editTileSize === 'compact'}
                        className={`space-seg-btn ${editTileSize === 'compact' ? 'active' : ''}`}
                        onClick={() => setEditTileSize('compact')}
                      >
                        Средняя
                      </button>
                      <button
                        type="button"
                        role="radio"
                        aria-checked={editTileSize === 'hero'}
                        className={`space-seg-btn ${editTileSize === 'hero' ? 'active' : ''}`}
                        onClick={() => setEditTileSize('hero')}
                      >
                        Большая
                      </button>
                    </div>
                  </div>

                  <div className="tile-edit-field">
                    <span className="tile-edit-lbl">Расположение</span>
                    <div className="space-seg" role="radiogroup" aria-label="Расположение">
                      <button
                        type="button"
                        role="radio"
                        aria-checked={editSection === 'main'}
                        className={`space-seg-btn ${editSection === 'main' ? 'active' : ''}`}
                        onClick={() => setEditSection('main')}
                      >
                        Верхний блок
                      </button>
                      <button
                        type="button"
                        role="radio"
                        aria-checked={editSection === 'bottom'}
                        className={`space-seg-btn ${editSection === 'bottom' ? 'active' : ''}`}
                        onClick={() => setEditSection('bottom')}
                      >
                        Нижний блок
                      </button>
                    </div>
                  </div>
                </div>

                <div className="tile-edit-field">
                  <div className="tile-edit-lbl-row">
                    <span className="tile-edit-lbl">Цвет плитки и иконки</span>
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
                    <span className="tile-edit-lbl">Иконка</span>
                    <button
                      type="button"
                      className="tile-reset-mini-btn"
                      onClick={() => {
                        setEditCustomIcon(null);
                        setEditIconData(null);
                      }}
                    >
                      Сбросить иконку
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
                  <>
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

                    {!isMac && (
                      <div className="tile-edit-switch-row">
                        <div className="tile-edit-switch-meta">
                          <span className="tile-edit-switch-title">
                            Запускать от имени администратора
                          </span>
                          <span className="tile-edit-switch-sub">
                            Повышенные права Windows при открытии
                          </span>
                        </div>
                        <button
                          type="button"
                          role="switch"
                          aria-checked={editRunAsAdmin}
                          aria-label="Запускать от имени администратора"
                          className={`glass-switch ${editRunAsAdmin ? 'on' : ''}`}
                          onClick={() => setEditRunAsAdmin((v) => !v)}
                        >
                          <span className="glass-switch-thumb" />
                        </button>
                      </div>
                    )}
                  </>
                )}

                <div className="tile-edit-field">
                  <label className="tile-edit-lbl" htmlFor="tb-edit-pin-alt">
                    Второе действие (Shift)
                  </label>
                  <select
                    id="tb-edit-pin-alt"
                    className="space-create-input"
                    value={editPinAltAction}
                    onChange={(e) => setEditPinAltAction(e.target.value)}
                  >
                    {ALT_ACTION_OPTIONS.filter(
                      (opt) => !(isMac && opt.value === 'От имени администратора'),
                    ).map((opt) => (
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
              </div>

              <div className="tile-edit-foot">
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
          </div>
        )}

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
          <div className="ctx-scroll">
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
        </div>
      )}

      {/* EMPTY-SPACE CONTEXT MENU — Rendered OUTSIDE .start-chassis overflow: hidden! */}
      {emptyCtxPresence.mounted && (emptyCtx ?? lastEmptyCtxRef.current) && (
        <div
          className="flyout-card ctx-card tile-ctx-menu empty-ctx-menu"
          data-phase={emptyCtxPresence.phase}
          role="menu"
          aria-label={activeSpace?.name ?? 'Start'}
          style={{
            left: (emptyCtx ?? lastEmptyCtxRef.current)!.x,
            top: (emptyCtx ?? lastEmptyCtxRef.current)!.y,
          }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="ctx-sec">{activeSpace?.name ?? 'Пространство'}</div>
          <div className="ctx-scroll">
            {emptyCtxActions.map((act, idx) => (
              <React.Fragment key={act.id}>
                {act.id === 'settings' && <div className="ctx-sep" />}
                <button
                  type="button"
                  role="menuitem"
                  className={`ctx-item ${idx === emptyCtxIdx ? 'focused' : ''}`}
                  onMouseEnter={() => setEmptyCtxIdx(idx)}
                  onClick={act.run}
                >
                  {act.icon}
                  <span className="ctx-item-label">{act.label}</span>
                </button>
              </React.Fragment>
            ))}
          </div>
        </div>
      )}

      {/* SEARCH CONTEXT MENU — Rendered OUTSIDE .start-chassis overflow: hidden above .search-backdrop */}
      {searchCtxPresence.mounted && activeSearchCtxSource && (
        <div
          className="flyout-card ctx-card tile-ctx-menu search-ctx-menu"
          data-phase={searchCtxPresence.phase}
          role="menu"
          aria-label={activeSearchCtxSource.item.name}
          style={{ left: activeSearchCtxSource.x, top: activeSearchCtxSource.y, zIndex: 95 }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="ctx-sec">{activeSearchCtxSource.item.name}</div>
          <div className="ctx-scroll">
            {searchCtxActions.map((act, idx) => (
              <React.Fragment key={act.id}>
                {act.danger && <div className="ctx-sep" />}
                <button
                  type="button"
                  role="menuitem"
                  className={`ctx-item ${act.danger ? 'danger' : ''} ${idx === searchCtxIdx ? 'focused' : ''}`}
                  onMouseEnter={() => setSearchCtxIdx(idx)}
                  onClick={act.run}
                >
                  {act.icon}
                  <span className="ctx-item-label">{act.label}</span>
                </button>
              </React.Fragment>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};
