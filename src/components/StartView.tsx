import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { canOpenFileLocation, canRunAsAdmin, formatIpcError } from '../lib/flyoutLogic';
import {
  applyConfigUpdate,
  deleteCustomSpace,
  emptyRecycleBin,
  exportConfigDialog,
  fetchIconsBatch,
  iconCache,
  importConfigDialog,
  ipc,
  isCacheIconKey,
  isTauri,
  normalizeStartStatePayload,
  onEvent,
  openScreenSnip,
  pasteClipboardEntry,
  recordFrecency,
  refreshIndexes,
  restoreAllTrayApps,
  restoreTrayApp,
  setAppearanceSettings,
  surfaceCommitAck,
  surfaceExitAck,
  toggleStart,
  updateCustomSpace,
  updatePin,
  type AppWindowGroup,
  type AudioMasterState,
  type AudioState,
  type ClipboardEntry,
  type GlassDepth,
  type IndexedApp,
  type IndexedFile,
  type MediaState,
  type PeripheralBattery,
  type PinItem,
  type SecondaryTaskbarMode,
  type StartStateEvent,
  type SteamGame,
  type TelemetryState,
  type TestBarConfig,
  type WidgetPreset,
} from '../lib/ipc';
import {
  buildSearchCorpus,
  isPrintableSearchTrigger,
  rankSearchResults,
  resolveSearchLaunchPayload,
  resolveSearchSecondaryAction,
  type SearchCorpusItem,
} from '../lib/searchLogic';
import {
  buildGameComboLaunchPayload,
  completeStartClosing,
  INITIAL_START_ANIM_STATE,
  promoteStartEntering,
  reducePowerClick,
  reduceStartState,
  resolvePinAction,
  START_EXIT_ANIM_MS,
  type ArmedPowerKind,
  type PowerActionKind,
  type SpaceAccentOption,
  type StartAnimState,
} from '../lib/startLogic';
import { useAnimatedPresence } from '../lib/useAnimatedPresence';
import {
  CopyIcon,
  EditIcon,
  ExternalWindowIcon,
  FolderSvg,
  KillIcon,
  PinIcon,
  ShieldIcon,
  TerminalIcon,
  UnpinIcon,
} from './Icons';
import { StartCommandPalette } from './start/StartCommandPalette';
import { StartHeader } from './start/StartHeader';
import { StartPinDrawer } from './start/StartPinDrawer';
import { StartSettingsOverlay } from './start/StartSettingsOverlay';
import { StartSpaces } from './start/StartSpaces';

const BUILTIN_SPACE_AURA_RGB: Record<string, string> = {
  personal: '139, 150, 248',
  work: '82, 184, 242',
  studio: '62, 213, 152',
  games: '242, 110, 132',
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
  const vw = typeof window !== 'undefined' ? window.innerWidth : 1188;
  const vh = typeof window !== 'undefined' ? window.innerHeight : 728;
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
  const [savingPinEdit, setSavingPinEdit] = useState(false);
  const editTitleInputRef = useRef<HTMLInputElement>(null);

  // Command Palette
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchTrigger, setSearchTrigger] = useState<'keyboard' | 'click'>('keyboard');
  const [query, setQuery] = useState('');
  const [cmdIdx, setCmdIdx] = useState(0);
  const cmdInputRef = useRef<HTMLInputElement>(null);

  // Refs for lifecycle & Paint-Sync coordination
  const mountedRef = useRef<boolean>(true);
  const latestGenRef = useRef<number>(0);
  const pendingCommitGenRef = useRef<number | null>(null);
  const enterRaf1Ref = useRef<number | null>(null);
  const enterRaf2Ref = useRef<number | null>(null);
  const closeCleanupTimerRef = useRef<number | null>(null);
  const toastTimerRef = useRef<number | null>(null);
  const lastSpaceSwitchTsRef = useRef<number>(0);
  const configRef = useRef<TestBarConfig | null>(null);
  const spaceRef = useRef<string>(space);
  const secondaryHydratedRef = useRef<boolean>(false);
  const resolvedTargetsRef = useRef<Set<string>>(new Set());

  const cancelEnterRafs = useCallback(() => {
    if (typeof cancelAnimationFrame !== 'function') {
      enterRaf1Ref.current = null;
      enterRaf2Ref.current = null;
      return;
    }
    if (enterRaf1Ref.current !== null) {
      cancelAnimationFrame(enterRaf1Ref.current);
      enterRaf1Ref.current = null;
    }
    if (enterRaf2Ref.current !== null) {
      cancelAnimationFrame(enterRaf2Ref.current);
      enterRaf2Ref.current = null;
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  configRef.current = config;
  spaceRef.current = space;

  // 2-layer ambient aura crossfade state
  const activeCustomSpace = config?.customSpaces.find((s) => s.id === space) ?? null;
  const spaceTheme = ['personal', 'work', 'studio', 'games'].includes(space) ? space : 'custom';
  const targetAuraRgb =
    BUILTIN_SPACE_AURA_RGB[space] ?? activeCustomSpace?.accentRgb ?? '167, 139, 250';

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

  const resetTransientOverlays = useCallback(() => {
    setSearchOpen(false);
    setQuery('');
    setPowerOpen(false);
    setArmedPower(null);
    setPeriphOpen(false);
    setAddingSpace(false);
    setManagingSpaceId(null);
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
    ipc<PeripheralBattery[]>('get_peripherals').then(setPeriphs).catch(() => {});
  }, []);

  // Paint-Sync commit ACK + double-rAF promotion from 'entering' -> 'open'
  useLayoutEffect(() => {
    if (anim.phase !== 'entering') return;
    if (pendingCommitGenRef.current !== anim.gen) return;
    const gen = anim.gen;
    pendingCommitGenRef.current = null;

    surfaceCommitAck('start', gen).catch(() => {});

    cancelEnterRafs();

    if (typeof requestAnimationFrame === 'function') {
      enterRaf1Ref.current = requestAnimationFrame(() => {
        enterRaf1Ref.current = null;
        if (latestGenRef.current !== gen) return;
        enterRaf2Ref.current = requestAnimationFrame(() => {
          enterRaf2Ref.current = null;
          if (latestGenRef.current === gen) {
            setAnim((prev) => promoteStartEntering(prev));
          }
        });
      });
    } else if (latestGenRef.current === gen) {
      setAnim((prev) => promoteStartEntering(prev));
    }
  }, [anim.phase, anim.gen, anim.openSeq, cancelEnterRafs]);

  useEffect(() => {
    hydrateCoreData();
    const idleTimer = window.setTimeout(() => {
      hydrateSecondaryData();
    }, 1400);

    const subs = [
      onEvent<TestBarConfig>('config-updated', (next) =>
        setConfig((prev) => applyConfigUpdate(prev, next)),
      ),
      onEvent<AppWindowGroup[]>('windows-updated', setGroups),
      onEvent<IndexedApp[]>('apps-indexed', setApps),
      onEvent<IndexedFile[]>('files-indexed', setFiles),
      onEvent<SteamGame[]>('games-indexed', setGames),
      onEvent<AudioState>('audio-mixer-updated', setAudio),
      onEvent<AudioMasterState>('audio-master-updated', (master) => {
        setAudio((prev) =>
          prev
            ? {
                ...prev,
                available: master.available,
                masterVolume: master.masterVolume,
                masterMuted: master.masterMuted,
                micMuted: master.micMuted,
              }
            : prev,
        );
      }),
      onEvent<MediaState>('media-updated', setMedia),
      onEvent<TelemetryState>('telemetry-updated', setTelem),
      onEvent<ClipboardEntry[]>('clipboard-updated', setClips),
      onEvent<PeripheralBattery[]>('peripherals-updated', setPeriphs),
      onEvent<boolean | StartStateEvent>('start-state', (raw) => {
        const ev = normalizeStartStatePayload(raw);
        latestGenRef.current = ev.gen;

        if (closeCleanupTimerRef.current !== null) {
          window.clearTimeout(closeCleanupTimerRef.current);
          closeCleanupTimerRef.current = null;
        }
        cancelEnterRafs();

        if (ev.open) {
          hydrateSecondaryData();
          setShiftHeld(false);
          const remember = configRef.current?.startRememberSpace ?? false;
          if (!remember && spaceRef.current !== 'personal') {
            setSpaceDir(0);
            setRapidSwitch(false);
            setSpace('personal');
          }
          pendingCommitGenRef.current = ev.gen;
          setAnim((prev) => reduceStartState(prev, ev));
        } else {
          pendingCommitGenRef.current = null;
          setAnim((prev) => reduceStartState(prev, ev));
          if (ev.immediate) {
            resetTransientOverlays();
            surfaceExitAck('start', ev.gen).catch(() => {});
          } else {
            const exitGen = ev.gen;
            closeCleanupTimerRef.current = window.setTimeout(() => {
              closeCleanupTimerRef.current = null;
              if (latestGenRef.current !== exitGen) return;
              setAnim((prev) => completeStartClosing(prev, exitGen));
              resetTransientOverlays();
              surfaceExitAck('start', exitGen).catch(() => {});
            }, START_EXIT_ANIM_MS);
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
        setTileCtx(null);
        setTileEdit(null);
        setSettingsOpen(true);
      }),
    ];

    return () => {
      window.clearTimeout(idleTimer);
      if (closeCleanupTimerRef.current !== null) {
        window.clearTimeout(closeCleanupTimerRef.current);
      }
      cancelEnterRafs();
      if (toastTimerRef.current !== null) {
        window.clearTimeout(toastTimerRef.current);
      }
      subs.forEach((p) => p.then((u) => u()));
    };
  }, [hydrateCoreData, hydrateSecondaryData, resetTransientOverlays, cancelEnterRafs]);

  // If active custom space was deleted remotely, fall back to 'personal'
  useEffect(() => {
    if (!config) return;
    if (['personal', 'work', 'studio', 'games'].includes(space)) return;
    if (!config.customSpaces.some((s) => s.id === space)) {
      setSpaceDir(0);
      setSpace('personal');
    }
  }, [config, space]);

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
        ...config.personalPins,
        ...config.workApps,
        ...config.studioPins,
        ...config.studioTools,
        ...config.gameLaunchers,
        ...config.customSpaces.flatMap((s) => s.items),
      ].forEach((p) => {
        if (p.iconKey) keys.push(p.iconKey);
      });
    }
    if (keys.length > 0) {
      handleResolveIconKeys(keys);
    }
  }, [apps, groups, config, handleResolveIconKeys]);

  // Batched native icon hydration for path/UWP pinned tiles through unified Rust IconCache
  useEffect(() => {
    if (!config) return;
    const allPins = [
      ...config.personalPins,
      ...config.personalLinks,
      ...config.workApps,
      ...config.studioPins,
      ...config.studioTools,
      ...config.gameLaunchers,
      ...config.customSpaces.flatMap((s) => s.items),
    ];

    const targetsToResolve: string[] = [];
    for (const p of allPins) {
      if ((p.kind === 'app' || p.kind === 'file') && p.target && !p.iconData) {
        const low = p.target.toLowerCase();
        const resolvable =
          p.target.includes(':\\') || p.target.startsWith('\\\\') || low.startsWith('shell:appsfolder\\');
        if (resolvable && !resolvedTargetsRef.current.has(p.target)) {
          resolvedTargetsRef.current.add(p.target);
          targetsToResolve.push(p.target);
        }
      }
    }

    if (targetsToResolve.length === 0) return;

    ipc<Record<string, string>>('resolve_pin_icons', { targets: targetsToResolve })
      .then((batch) => {
        if (!mountedRef.current) {
          for (const t of targetsToResolve) {
            resolvedTargetsRef.current.delete(t);
          }
          return;
        }
        if (batch && Object.keys(batch).length > 0) {
          setIconMap((prev) => ({ ...prev, ...batch }));
        }
      })
      .catch(() => {
        for (const t of targetsToResolve) {
          resolvedTargetsRef.current.delete(t);
        }
      });
  }, [config]);

  const allSpaceIds = useMemo(
    () => [
      'personal',
      'work',
      'studio',
      'games',
      ...(config?.customSpaces.map((s) => s.id) ?? []),
    ],
    [config?.customSpaces],
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
      setPeriphOpen(false);
      setPowerOpen(false);
      setArmedPower(null);
      setTileCtx(null);
      setTileEdit(null);
    },
    [allSpaceIds],
  );

  const runningExeSet = useMemo(() => {
    const s = new Set<string>();
    groups.forEach((g) => {
      if (g.exePath) s.add(g.exePath.toLowerCase());
      if (g.aumid) s.add(`shell:appsfolder\\${g.aumid.toLowerCase()}`);
    });
    return s;
  }, [groups]);

  const isRunning = useCallback(
    (item: PinItem) => {
      if (item.kind !== 'app') return false;
      if (item.matchAumid) {
        const aumidLow = item.matchAumid.toLowerCase();
        if (groups.some((g) => g.aumid?.toLowerCase() === aumidLow)) return true;
      }
      if (item.matchExe) {
        const matchBase = item.matchExe.toLowerCase();
        if (groups.some((g) => g.exePath.toLowerCase().endsWith('\\' + matchBase))) return true;
      }
      if (!item.target) return false;
      const low = item.target.toLowerCase();
      if (runningExeSet.has(low)) return true;
      const base = low.split('\\').pop() || '';
      return Boolean(base) && groups.some((g) => g.exePath.toLowerCase().endsWith('\\' + base));
    },
    [groups, runningExeSet],
  );

  const resolveIconForPin = useCallback(
    (item: PinItem): string | null => {
      if (isCacheIconKey(item.iconKey)) {
        const fromMap = iconMap[item.iconKey] || iconCache.get(item.iconKey);
        if (fromMap) return fromMap;
      }
      return item.iconData || iconMap[item.target] || null;
    },
    [iconMap],
  );

  const searchCorpus = useMemo(
    () => buildSearchCorpus({ config, apps, games, files, iconMap, groups, audio, clips }),
    [config, apps, games, files, iconMap, groups, audio, clips],
  );

  const searchResults = useMemo(
    () => rankSearchResults(searchCorpus, query, 12, config?.frecency),
    [searchCorpus, query, config?.frecency],
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
      const vw = typeof window !== 'undefined' ? window.innerWidth : 1188;
      const vh = typeof window !== 'undefined' ? window.innerHeight : 728;
      const rawX = coords ? coords.x : Math.round((vw - 316) / 2);
      const rawY = coords ? coords.y : Math.round((vh - 360) / 2);
      const pos = clampPopoverPos(rawX, rawY, 316, 360);

      setTileCtx(null);
      setEditPinTitle(item.title);
      setEditPinArgs(item.args ?? '');
      setEditPinAltAction(item.altAction ?? '');
      setEditPinAltLabel(item.altLabel ?? '');
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
      const updated = await updatePin({
        space: tileEdit.spaceKey,
        pinId: tileEdit.item.id,
        title: trimmedTitle,
        args: tileEdit.item.kind === 'app' ? editPinArgs.trim() : undefined,
        altAction: editPinAltAction,
        altLabel: editPinAltAction ? editPinAltLabel.trim() : '',
      });
      handleApplyConfig(updated);
      setTileEdit(null);
      showToast('Плитка обновлена');
    } catch (err) {
      showToast(formatIpcError(err));
      setSavingPinEdit(false);
    }
  };

  // Build dynamic Tile Context Menu actions (all 7+ items with 14px SVG icons!)
  const activeCtxSource = tileCtx ?? lastTileCtxRef.current;
  const tileCtxActions = useMemo<TileCtxAction[]>(() => {
    if (!activeCtxSource) return [];
    const { item, spaceKey } = activeCtxSource;
    const actions: TileCtxAction[] = [];

    actions.push({
      id: 'open',
      label: 'Открыть',
      icon: <ExternalWindowIcon className="ic14" />,
      run: () => {
        launchPin(item);
        setTileCtx(null);
      },
    });

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

    if (canOpenFileLocation(item.target)) {
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
        label: 'Настроить плитку',
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
        } else if (drawerOpen) {
          setDrawerOpen(false);
        } else if (settingsOpen) {
          setSettingsOpen(false);
        } else if (periphOpen) {
          setPeriphOpen(false);
        } else if (powerOpen) {
          setPowerOpen(false);
          setArmedPower(null);
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
      const targetSpace = space === 'personal' ? 'personal_links' : space === 'work' ? 'work_folders' : space;
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
      const targetSpace = space === 'personal' ? (isExe ? 'personal' : 'personal_links') : space;
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
    const targetSpace = space === 'personal' ? 'personal_links' : space;
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

  const handleCreateCustomSpace = async (
    name: string,
    accent: SpaceAccentOption,
    widgetPreset: WidgetPreset,
  ) => {
    const updated = await ipc<TestBarConfig>('add_custom_space', {
      name,
      accentHex: accent.hex,
      accentRgb: accent.rgb,
      widgetPreset,
    });
    handleApplyConfig(updated);
    const newest = updated.customSpaces[updated.customSpaces.length - 1];
    if (newest) {
      selectSpace(newest.id);
    }
    showToast(`Создано пространство «${name}»`);
  };

  const handleUpdateCustomSpace = async (params: {
    id: string;
    name: string;
    accent: SpaceAccentOption;
    widgetPreset: WidgetPreset;
  }) => {
    const updated = await updateCustomSpace({
      id: params.id,
      name: params.name,
      accentHex: params.accent.hex,
      accentRgb: params.accent.rgb,
      widgetPreset: params.widgetPreset,
    });
    handleApplyConfig(updated);
    showToast('Пространство обновлено');
  };

  const handleDeleteCustomSpace = async (id: string) => {
    const updated = await deleteCustomSpace(id);
    handleApplyConfig(updated);
    if (spaceRef.current === id) {
      setSpaceDir(-1);
      setSpace('personal');
    }
    showToast('Пространство удалено');
  };

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

  const customSpaceStyle: React.CSSProperties | undefined = activeCustomSpace
    ? ({
        ['--accent' as string]: activeCustomSpace.accentHex || '#A78BFA',
        ['--accent-rgb' as string]: activeCustomSpace.accentRgb || '167, 139, 250',
      } as React.CSSProperties)
    : undefined;

  const startVisible = anim.phase !== 'hidden';
  const glassDepth: GlassDepth = config?.glassDepth ?? 'obsidian';

  return (
    <div
      className={`start-root ${editMode ? 'edit-mode' : ''} ${shiftHeld ? 'shift-active' : ''}`}
      data-space={spaceTheme}
      data-glass={glassDepth}
      data-visible={startVisible ? 'true' : 'false'}
      style={customSpaceStyle}
      onClick={() => {
        if (tileCtx) setTileCtx(null);
        if (tileEdit) setTileEdit(null);
        if (powerOpen) {
          setPowerOpen(false);
          setArmedPower(null);
        }
      }}
    >
      {/* NON-REMOUNTING START CHASSIS (NO key={anim.openSeq}!) */}
      <section className="start-chassis" data-phase={anim.phase}>
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
          customSpaces={config?.customSpaces ?? []}
          periphs={periphs}
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
            setTileCtx(null);
            setTileEdit(null);
          }}
          onToggleSettings={() => {
            setSettingsOpen((v) => !v);
            setDrawerOpen(false);
            setSearchOpen(false);
            setAddingSpace(false);
            setManagingSpaceId(null);
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
          onCreateCustomSpace={handleCreateCustomSpace}
          onUpdateCustomSpace={handleUpdateCustomSpace}
          onDeleteCustomSpace={handleDeleteCustomSpace}
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
          <StartSpaces
            space={space}
            activeCustomSpace={activeCustomSpace}
            config={config}
            games={games}
            audio={audio}
            media={media}
            telem={telem}
            clips={clips}
            editMode={editMode}
            shiftHeld={shiftHeld}
            isRunning={isRunning}
            resolveIconForPin={resolveIconForPin}
            onLaunchPin={launchPin}
            onOpenDrawer={() => setDrawerOpen(true)}
            onOpenTileContext={openTileContextMenu}
            onSetAudio={setAudio}
            onSetMedia={setMedia}
            onSetTelem={setTelem}
            onSetClips={setClips}
            onShowToast={showToast}
            startVisible={startVisible}
            spaceDir={spaceDir}
            rapidSwitch={rapidSwitch}
            iconMap={iconMap}
            apps={apps}
            onResolveIconKeys={handleResolveIconKeys}
            onSetConfig={handleApplyConfig}
            onEditPin={(item, spaceKey) => openTileEditPopover(item, spaceKey)}
          />
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
          className="tile-edit-pop"
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
