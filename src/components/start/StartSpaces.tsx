import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { getActiveDeviceIndex } from '../../lib/flyoutLogic';
import {
  addGameCombo,
  deleteClipboardEntry,
  deleteGameCombo,
  getAudioPeaks,
  getClipboardPaused,
  iconCache,
  ipc,
  isCacheIconKey,
  loadGameCover,
  pasteClipboardEntry,
  seekMedia,
  selectMediaSession,
  setClipboardPaused,
  setInputDevice,
  setSessionState,
  type AudioDeviceInfo,
  type AudioState,
  type ClipboardEntry,
  type CustomSpace,
  type IndexedApp,
  type MediaState,
  type PinItem,
  type SteamGame,
  type TelemetryState,
  type TestBarConfig,
  type TopProcessInfo,
  type WidgetPreset,
} from '../../lib/ipc';
import {
  buildSparklinePath,
  computeGameCardTilt,
  computeInterpolatedMediaSec,
  computePointerReorderRequest,
  EMPTY_TELEMETRY_HISTORY,
  filterClipboardEntries,
  formatDiskSummary,
  formatGbValue,
  formatKillProcessError,
  formatMediaDuration,
  formatMemoryRatio,
  formatProcessMetricLine,
  getClipboardKindLabel,
  getGpuTempStatusPill,
  getHonestShiftBadge,
  getTelemetryLevelClass,
  getTelemetryStatusPill,
  hasExceededDragThreshold,
  hexColorToRgbString,
  normalizeSessionVolumeScalar,
  partitionHeroAndCompact,
  PROCESS_KILL_ARM_MS,
  pushTelemetryHistory,
  reduceProcessKillClick,
  resolveClipboardKind,
  resolveProcessKillPids,
  selectDisplayedGames,
  validateGameComboInput,
  type GameLibraryFilter,
  type TelemetryHistory,
} from '../../lib/startLogic';
import { useAnimatedPresence } from '../../lib/useAnimatedPresence';
import { useReducedMotion } from '../../lib/useReducedMotion';
import {
  ChevronDownIcon,
  CloseWindowIcon,
  CopyIcon,
  DiskIcon,
  ExternalArrowIcon,
  FallbackAppIcon,
  FolderSvg,
  KillIcon,
  MicIcon,
  MusicNoteIcon,
  NextTrackIcon,
  PauseIcon,
  PinIcon,
  PlaySmallIcon,
  PlayTriangleIcon,
  PlusIcon,
  PrevTrackIcon,
  SearchIcon,
  TerminalIcon,
  TrashIcon,
  VolIcon,
  XIcon10,
} from '../Icons';
import { ObsidianSlider } from '../ObsidianSlider';
import { SpotCard, trackTilePointer, type SpotCardProps } from './SpotCard';

type BentoSpotCardProps = SpotCardProps & { bentoIndex?: number };
const BentoSpotCard = SpotCard as React.ComponentType<BentoSpotCardProps>;

interface ActivePointerDrag {
  spaceKey: string;
  fromIdx: number;
  overIdx: number | null;
  label: string;
}

interface LivePeaksState {
  masterPeak: number;
  micPeak: number;
  sessionPeaks: Record<string, number>;
}

interface ComboDraftStep {
  id: string;
  label: string;
  target: string;
}

// Bounded cache of sampled ambient RGB triplets keyed by album art data URI
const ambientRgbCache = new Map<string, string>();
const AMBIENT_CACHE_MAX = 24;

function sampleAmbientRgbFromDataUrl(dataUrl: string): Promise<string | null> {
  if (!dataUrl || typeof document === 'undefined' || typeof Image === 'undefined') {
    return Promise.resolve(null);
  }
  const cached = ambientRgbCache.get(dataUrl);
  if (cached) {
    return Promise.resolve(cached);
  }

  return new Promise((resolve) => {
    try {
      const img = new Image();
      img.onload = () => {
        try {
          const size = 16;
          const canvas = document.createElement('canvas');
          canvas.width = size;
          canvas.height = size;
          const ctx = canvas.getContext('2d', { willReadFrequently: true });
          if (!ctx) {
            resolve(null);
            return;
          }
          ctx.drawImage(img, 0, 0, size, size);
          const { data } = ctx.getImageData(0, 0, size, size);

          let sumR = 0;
          let sumG = 0;
          let sumB = 0;
          let weightSum = 0;

          for (let i = 0; i < data.length; i += 4) {
            const a = data[i + 3];
            if (a < 96) continue;
            const r = data[i];
            const g = data[i + 1];
            const b = data[i + 2];
            const max = Math.max(r, g, b);
            const min = Math.min(r, g, b);
            const chroma = max - min;
            const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            if (lum < 22 || lum > 242) continue;
            // Favor moderately chromatic pixels for subtle obsidian ambient tint
            const w = 1 + (chroma / 255) * 3.5;
            sumR += r * w;
            sumG += g * w;
            sumB += b * w;
            weightSum += w;
          }

          if (weightSum <= 0) {
            resolve(null);
            return;
          }

          let r = Math.round(sumR / weightSum);
          let g = Math.round(sumG / weightSum);
          let b = Math.round(sumB / weightSum);

          // Lift overly dark samples gently so the 16% radial wash reads cleanly on obsidian
          const avgLum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
          if (avgLum > 1 && avgLum < 96) {
            const boost = Math.min(2.2, 96 / avgLum);
            r = Math.min(235, Math.round(r * boost));
            g = Math.min(235, Math.round(g * boost));
            b = Math.min(235, Math.round(b * boost));
          }

          const triplet = `${r}, ${g}, ${b}`;
          if (ambientRgbCache.size >= AMBIENT_CACHE_MAX) {
            const oldest = ambientRgbCache.keys().next().value;
            if (oldest !== undefined) ambientRgbCache.delete(oldest);
          }
          ambientRgbCache.set(dataUrl, triplet);
          resolve(triplet);
        } catch {
          resolve(null);
        }
      };
      img.onerror = () => resolve(null);
      img.src = dataUrl;
    } catch {
      resolve(null);
    }
  });
}

function isActivateKey(e: React.KeyboardEvent): boolean {
  return e.key === 'Enter' || e.key === ' ';
}

export interface StartSpacesProps {
  space: string;
  activeCustomSpace: CustomSpace | null;
  config: TestBarConfig | null;
  games: SteamGame[];
  audio: AudioState | null;
  media: MediaState | null;
  telem: TelemetryState | null;
  clips: ClipboardEntry[];
  editMode: boolean;
  shiftHeld: boolean;
  isRunning: (item: PinItem) => boolean;
  resolveIconForPin: (item: PinItem) => string | null;
  onLaunchPin: (item: PinItem) => void;
  onOpenDrawer: () => void;
  onOpenTileContext: (e: React.MouseEvent, item: PinItem, spaceKey: string) => void;
  onSetAudio: (audio: AudioState, fromServer?: boolean) => void;
  onSetMedia: (media: MediaState) => void;
  onSetTelem: (telem: TelemetryState) => void;
  onSetClips: (clips: ClipboardEntry[]) => void;
  onShowToast: (msg: string) => void;
  // Shared Wave 2B optional props
  startVisible?: boolean;
  startAnimSettled?: boolean;
  onAudioDragStateChange?: (sliderKey: string, dragging: boolean) => void;
  spaceDir?: number;
  rapidSwitch?: boolean;
  iconMap?: Record<string, string>;
  apps?: IndexedApp[];
  onResolveIconKeys?: (keys: string[]) => void;
  onSetConfig?: (config: TestBarConfig) => void;
  onOptimisticReorder?: (spaceKey: string, fromIdx: number, toIdx: number) => void;
  onEditPin?: (item: PinItem, spaceKey: string) => void;
}

export const StartSpaces: React.FC<StartSpacesProps> = ({
  space,
  activeCustomSpace,
  config,
  games,
  audio,
  media,
  telem,
  clips,
  editMode,
  shiftHeld,
  isRunning,
  resolveIconForPin,
  onLaunchPin,
  onOpenDrawer,
  onOpenTileContext,
  onSetAudio,
  onSetMedia,
  onSetTelem,
  onSetClips,
  onShowToast,
  startVisible = true,
  startAnimSettled = true,
  onAudioDragStateChange,
  spaceDir = 0,
  rapidSwitch = false,
  iconMap,
  apps = [],
  onResolveIconKeys,
  onSetConfig,
  onOptimisticReorder,
}) => {
  const reducedMotion = useReducedMotion();

  const [gameFilter, setGameFilter] = useState<GameLibraryFilter>('recent');
  const [copiedClipId, setCopiedClipId] = useState<string | null>(null);
  const [activeDrag, setActiveDrag] = useState<ActivePointerDrag | null>(null);
  const pointerDragCleanupRef = useRef<(() => void) | null>(null);
  const suppressClickUntilRef = useRef<number>(0);
  const dragGhostRef = useRef<HTMLDivElement | null>(null);
  const [killError, setKillError] = useState<{ space: string; message: string } | null>(null);

  // Safe 2-click armed process kill & combo delete
  const [armedKillKey, setArmedKillKey] = useState<string | null>(null);
  const armedKillTimerRef = useRef<number | null>(null);
  const [armedComboDeleteId, setArmedComboDeleteId] = useState<string | null>(null);
  const armedComboTimerRef = useRef<number | null>(null);

  // Audio switcher overflow, initial no-anim gate, drag coalescing & live WASAPI peaks
  const [devOverflowOpen, setDevOverflowOpen] = useState(false);
  const devDropdownPresence = useAnimatedPresence(devOverflowOpen, 95);
  const devRowRef = useRef<HTMLDivElement>(null);
  const [segReady, setSegReady] = useState(false);
  const [sliderAnimReady, setSliderAnimReady] = useState(false);
  const [peaks, setPeaks] = useState<LivePeaksState | null>(null);
  const draggingAudioKeysRef = useRef<Set<string>>(new Set());
  const audioReqSeqRef = useRef<Map<string, number>>(new Map());
  const audioThrottleTimersRef = useRef<Map<string, number>>(new Map());
  const audioPendingSendRef = useRef<Map<string, () => void>>(new Map());

  // Album-art ambient RGB & smooth 500ms media timeline interpolation
  const [mediaAmbientRgb, setMediaAmbientRgb] = useState<string | null>(null);
  const [mediaNowMs, setMediaNowMs] = useState<number>(() => Date.now());
  const [scrubOverrideSec, setScrubOverrideSec] = useState<number | null>(null);

  // Lossless Clipboard Vault search & pause capture state
  const [clipSearchOpen, setClipSearchOpen] = useState(false);
  const [clipQuery, setClipQuery] = useState('');
  const [clipPaused, setClipPaused] = useState(false);
  const clipSearchRef = useRef<HTMLInputElement>(null);

  // Telemetry 60s rolling sparkline ring buffer
  const [telemHistory, setTelemHistory] = useState<TelemetryHistory>(EMPTY_TELEMETRY_HISTORY);

  // Lazy Steam/Epic/GOG cover art cache
  const mountedRef = useRef(true);
  const [loadedCovers, setLoadedCovers] = useState<Record<string, string>>({});
  const requestedCoversRef = useRef<Set<string>>(new Set());
  const requestedIconsRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Inline Game Combo builder sheet
  const [comboBuilderOpen, setComboBuilderOpen] = useState(false);
  const comboBuilderPresence = useAnimatedPresence(comboBuilderOpen, 105);
  const [comboName, setComboName] = useState('');
  const [comboSteps, setComboSteps] = useState<ComboDraftStep[]>([]);
  const [comboPickKey, setComboPickKey] = useState('');
  const [comboCustomTarget, setComboCustomTarget] = useState('');
  const [comboError, setComboError] = useState<string | null>(null);
  const [comboSaving, setComboSaving] = useState(false);

  const customPreset: WidgetPreset = activeCustomSpace?.widgetPreset ?? 'media_clipboard';

  const hasAudioDevices = (audio?.devices.length ?? 0) > 0;
  const hasAudioHydrated = audio !== null;

  // Reset transient space-local states on space switch or hide
  useEffect(() => {
    setKillError(null);
    setArmedKillKey(null);
    setArmedComboDeleteId(null);
    setDevOverflowOpen(false);
    setComboBuilderOpen(false);
    if (pointerDragCleanupRef.current) {
      pointerDragCleanupRef.current();
      pointerDragCleanupRef.current = null;
    }
    setActiveDrag(null);
  }, [space, startVisible]);

  // Suppress segmented-pill & slider mount/hydration glide on space switch, Start open, or first audio hydration
  useEffect(() => {
    setSegReady(false);
    setSliderAnimReady(false);
    if (!startVisible || !startAnimSettled || !hasAudioHydrated) {
      return;
    }
    let raf2 = 0;
    const raf1 =
      typeof requestAnimationFrame === 'function'
        ? requestAnimationFrame(() => {
            raf2 = requestAnimationFrame(() => {
              setSegReady(true);
              setSliderAnimReady(true);
            });
          })
        : null;
    if (raf1 === null) {
      setSegReady(true);
      setSliderAnimReady(true);
    }
    return () => {
      if (raf1 !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(raf1);
      }
      if (raf2 && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(raf2);
      }
    };
  }, [space, startVisible, startAnimSettled, hasAudioDevices, hasAudioHydrated]);

  // Cleanup arming, drag & audio throttle timers on unmount
  useEffect(() => {
    return () => {
      if (armedKillTimerRef.current !== null) window.clearTimeout(armedKillTimerRef.current);
      if (armedComboTimerRef.current !== null) window.clearTimeout(armedComboTimerRef.current);
      if (pointerDragCleanupRef.current) {
        pointerDragCleanupRef.current();
        pointerDragCleanupRef.current = null;
      }
      audioThrottleTimersRef.current.forEach((id) => window.clearTimeout(id));
      audioThrottleTimersRef.current.clear();
      audioPendingSendRef.current.clear();
    };
  }, []);

  // Dismiss audio output dropdown on outside pointerdown or Escape
  useEffect(() => {
    if (!devOverflowOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (devRowRef.current && !devRowRef.current.contains(e.target as Node)) {
        setDevOverflowOpen(false);
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        setDevOverflowOpen(false);
      }
    };
    window.addEventListener('mousedown', onPointerDown);
    window.addEventListener('keydown', onKeyDown, true);
    return () => {
      window.removeEventListener('mousedown', onPointerDown);
      window.removeEventListener('keydown', onKeyDown, true);
    };
  }, [devOverflowOpen]);

  // Dismiss inline combo builder on Escape
  useEffect(() => {
    if (!comboBuilderOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        setComboBuilderOpen(false);
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [comboBuilderOpen]);

  // Sync initial clipboard pause state when visible
  useEffect(() => {
    if (!startVisible) return;
    let active = true;
    getClipboardPaused()
      .then((paused) => {
        if (active) setClipPaused(Boolean(paused));
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [startVisible]);

  // Append telemetry sample to 60s rolling history ONLY while Start is visible
  useEffect(() => {
    if (!telem || !startVisible) return;
    setTelemHistory((prev) =>
      pushTelemetryHistory(prev, {
        cpuPct: telem.cpuPct,
        ramPct: telem.ramPct,
        gpuPct: telem.gpuPct,
        vramPct: telem.vramPct,
        gpuAvailable: telem.gpuAvailable,
      }),
    );
  }, [telem, startVisible]);

  // Sample tasteful ambient RGB from album art thumbnail (cached per data URI)
  useEffect(() => {
    const thumb = media?.thumbnail ?? null;
    if (!media?.hasSession || !thumb) {
      setMediaAmbientRgb(null);
      return;
    }
    let cancelled = false;
    sampleAmbientRgbFromDataUrl(thumb).then((rgb) => {
      if (!cancelled) setMediaAmbientRgb(rgb);
    });
    return () => {
      cancelled = true;
    };
  }, [media?.hasSession, media?.thumbnail]);

  // Smooth 500ms media timeline interpolation ONLY while visible & playing
  useEffect(() => {
    setMediaNowMs(Date.now());
    setScrubOverrideSec(null);
    if (!startVisible || !media?.hasSession || !media?.isPlaying) return;
    const id = window.setInterval(() => setMediaNowMs(Date.now()), 500);
    return () => window.clearInterval(id);
  }, [startVisible, media?.hasSession, media?.isPlaying, media?.positionSec, media?.lastUpdatedMs]);

  // Visibility-gated 140ms live WASAPI peak poller (waits until Start spring settles, 0% hidden CPU!)
  const needsAudioPeaks = Boolean(
    startVisible &&
      startAnimSettled &&
      (space === 'personal' ||
        space === 'studio' ||
        space === 'games' ||
        (activeCustomSpace !== null && customPreset === 'audio_mic')),
  );

  useEffect(() => {
    if (!needsAudioPeaks) {
      setPeaks(null);
      return;
    }
    let cancelled = false;
    let inFlight = false;

    const poll = () => {
      if (inFlight || cancelled) return;
      inFlight = true;
      getAudioPeaks()
        .then((snap) => {
          if (cancelled || !snap) return;
          const map: Record<string, number> = {};
          for (const [id, peak] of snap.sessionPeaks ?? []) {
            map[id] = peak;
          }
          setPeaks({
            masterPeak: snap.masterPeak ?? 0,
            micPeak: snap.micPeak ?? 0,
            sessionPeaks: map,
          });
        })
        .catch(() => {})
        .finally(() => {
          inFlight = false;
        });
    };

    poll();
    const timer = window.setInterval(poll, 140);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [needsAudioPeaks]);

  // Hydrate missing audio session & process icon keys via shared Rust IconCache
  useEffect(() => {
    if (!onResolveIconKeys) return;
    const missing: string[] = [];
    const seen = new Set<string>();

    const checkKey = (key: string | null | undefined) => {
      if (!isCacheIconKey(key) || seen.has(key) || requestedIconsRef.current.has(key)) return;
      if (iconMap?.[key] || iconCache.get(key)) return;
      seen.add(key);
      requestedIconsRef.current.add(key);
      missing.push(key);
    };

    (audio?.sessions ?? []).forEach((s) => checkKey(s.iconKey));
    (telem?.topProcesses ?? []).forEach((p) => checkKey(p.iconKey));
    (telem?.gpuProcesses ?? []).forEach((p) => checkKey(p.iconKey));

    if (missing.length > 0) {
      onResolveIconKeys(missing);
    }
  }, [audio?.sessions, telem?.topProcesses, telem?.gpuProcesses, iconMap, onResolveIconKeys]);

  const personalPartition = useMemo(
    () => partitionHeroAndCompact(config?.personalPins ?? []),
    [config?.personalPins],
  );
  const studioPartition = useMemo(
    () => partitionHeroAndCompact(config?.studioPins ?? []),
    [config?.studioPins],
  );
  const displayedGames = useMemo(
    () => selectDisplayedGames(games, gameFilter),
    [games, gameFilter],
  );
  const studioProcesses = useMemo(
    () => (telem?.gpuProcesses && telem.gpuProcesses.length > 0 ? telem.gpuProcesses : (telem?.topProcesses ?? [])),
    [telem?.gpuProcesses, telem?.topProcesses],
  );
  const filteredClips = useMemo(
    () => filterClipboardEntries(clips, clipQuery),
    [clips, clipQuery],
  );

  // Idle pre-warm top 6 recent Steam covers after boot so the VERY FIRST switch to Games has zero decode hitch
  useEffect(() => {
    if (games.length === 0) return;
    const timer = window.setTimeout(() => {
      if (!mountedRef.current) return;
      const topRecent = selectDisplayedGames(games, 'recent').slice(0, 6);
      for (const g of topRecent) {
        if (g.coverData || !g.coverPath || requestedCoversRef.current.has(g.coverPath)) continue;
        const path = g.coverPath;
        requestedCoversRef.current.add(path);
        loadGameCover(path)
          .then((uri) => {
            if (!uri || !mountedRef.current) {
              requestedCoversRef.current.delete(path);
              return;
            }
            setLoadedCovers((prev) => (prev[path] === uri ? prev : { ...prev, [path]: uri }));
          })
          .catch(() => {
            requestedCoversRef.current.delete(path);
          });
      }
    }, 2200);
    return () => window.clearTimeout(timer);
  }, [games]);

  // Lazy load remaining game covers AFTER the 190ms space slide finishes so first tab transition never hitches
  useEffect(() => {
    if (!startVisible || space !== 'games') return;
    const toLoad = displayedGames.filter(
      (g) =>
        !g.coverData &&
        Boolean(g.coverPath) &&
        !requestedCoversRef.current.has(g.coverPath!),
    );
    if (toLoad.length === 0) return;

    const deferTimer = window.setTimeout(() => {
      if (!mountedRef.current) return;
      toLoad.forEach((g) => {
        const path = g.coverPath!;
        if (requestedCoversRef.current.has(path)) return;
        requestedCoversRef.current.add(path);
        loadGameCover(path)
          .then((uri) => {
            if (!uri || !mountedRef.current) {
              requestedCoversRef.current.delete(path);
              return;
            }
            setLoadedCovers((prev) => (prev[path] === uri ? prev : { ...prev, [path]: uri }));
          })
          .catch(() => {
            requestedCoversRef.current.delete(path);
          });
      });
    }, 210);

    return () => window.clearTimeout(deferTimer);
  }, [startVisible, space, displayedGames]);

  // Combo picker catalog (installed apps + games)
  const comboPickerOptions = useMemo(() => {
    const opts: Array<{ key: string; label: string; target: string }> = [];
    const seen = new Set<string>();
    for (const g of games) {
      const t = g.target.trim();
      if (!t || seen.has(t.toLowerCase())) continue;
      seen.add(t.toLowerCase());
      opts.push({ key: `game:${g.appId}`, label: `${g.name} (${g.launcher || 'Игра'})`, target: t });
    }
    for (const a of apps) {
      const t = a.target.trim();
      if (!t || seen.has(t.toLowerCase())) continue;
      seen.add(t.toLowerCase());
      opts.push({ key: `app:${a.id}`, label: a.name, target: t });
    }
    return opts;
  }, [apps, games]);

  const resolveIconKeyUri = useCallback(
    (iconKey: string | null | undefined, fallbackData?: string | null): string | null => {
      if (isCacheIconKey(iconKey)) {
        const hit = iconMap?.[iconKey] || iconCache.get(iconKey);
        if (hit) return hit;
      }
      return fallbackData || null;
    },
    [iconMap],
  );

  const handleKillProcessClick = (proc: TopProcessInfo, targetSpace: string) => {
    const procKey = `${targetSpace}:${proc.name}:${proc.pid}`;
    const res = reduceProcessKillClick(armedKillKey, procKey);
    if (armedKillTimerRef.current !== null) {
      window.clearTimeout(armedKillTimerRef.current);
      armedKillTimerRef.current = null;
    }

    if (!res.execute) {
      setArmedKillKey(res.nextArmedKey);
      armedKillTimerRef.current = window.setTimeout(() => {
        armedKillTimerRef.current = null;
        setArmedKillKey(null);
      }, PROCESS_KILL_ARM_MS);
      return;
    }

    setArmedKillKey(null);
    const pids = resolveProcessKillPids(proc);
    setKillError(null);
    ipc('kill_process_group', { pids })
      .then(() => {
        onShowToast(`Завершено: ${proc.name}`);
        return ipc<TelemetryState>('get_telemetry').then(onSetTelem);
      })
      .catch((err) => {
        const message = formatKillProcessError(proc.name, err);
        setKillError({ space: targetSpace, message });
        onShowToast(message);
      });
  };

  const handleDeleteComboClick = (combo: PinItem) => {
    if (armedComboTimerRef.current !== null) {
      window.clearTimeout(armedComboTimerRef.current);
      armedComboTimerRef.current = null;
    }
    if (armedComboDeleteId !== combo.id) {
      setArmedComboDeleteId(combo.id);
      armedComboTimerRef.current = window.setTimeout(() => {
        armedComboTimerRef.current = null;
        setArmedComboDeleteId(null);
      }, PROCESS_KILL_ARM_MS);
      return;
    }
    setArmedComboDeleteId(null);
    deleteGameCombo(combo.id)
      .then((updated) => {
        onSetConfig?.(updated);
        onShowToast(`Удалено: ${combo.title}`);
      })
      .catch(() => onShowToast('Не удалось удалить связку'));
  };

  const handleSaveCombo = async () => {
    if (comboSaving) return;
    const check = validateGameComboInput(
      comboName,
      comboSteps.map((s) => s.target),
    );
    if (!check.ok) {
      setComboError(check.error);
      return;
    }
    setComboSaving(true);
    setComboError(null);
    const subtitle = comboSteps.map((s) => s.label).slice(0, 3).join(' + ');
    try {
      const updated = await addGameCombo({
        name: check.name,
        subtitle,
        accent: '242,110,132',
        steps: check.steps,
      });
      onSetConfig?.(updated);
      setComboName('');
      setComboSteps([]);
      setComboPickKey('');
      setComboCustomTarget('');
      setComboBuilderOpen(false);
      onShowToast(`Связка создана: ${check.name}`);
    } catch (err) {
      setComboError(typeof err === 'string' && err.trim() ? err : 'Не удалось сохранить связку');
    } finally {
      setComboSaving(false);
    }
  };

  const handlePinTileClick = useCallback(
    (item: PinItem) => {
      if (typeof performance !== 'undefined' && performance.now() < suppressClickUntilRef.current) {
        return;
      }
      onLaunchPin(item);
    },
    [onLaunchPin],
  );

  // Always-on pointer-based pin reordering (6px threshold, Hero <-> Compact, immune to Tauri v2 dragDropEnabled)
  const handlePinPointerDown = (
    e: React.PointerEvent<HTMLElement>,
    spaceKey: string,
    idx: number,
    label: string,
  ) => {
    if (e.button !== 0 || typeof window === 'undefined') return;
    const targetEl = e.target as HTMLElement | null;
    if (targetEl && targetEl.closest('button, input, select, textarea, a')) {
      return;
    }
    if (pointerDragCleanupRef.current) {
      pointerDragCleanupRef.current();
      pointerDragCleanupRef.current = null;
    }

    const startX = e.clientX;
    const startY = e.clientY;
    let active = false;
    let currentOverIdx: number | null = null;
    let lastX = startX;
    let lastY = startY;

    const positionGhost = (x: number, y: number) => {
      const ghost = dragGhostRef.current;
      if (ghost) {
        ghost.style.transform = `translate3d(${Math.round(x + 12)}px, ${Math.round(y + 12)}px, 0)`;
      }
    };

    const onMove = (ev: PointerEvent) => {
      lastX = ev.clientX;
      lastY = ev.clientY;
      if (!active) {
        if (!hasExceededDragThreshold(startX, startY, ev.clientX, ev.clientY)) {
          return;
        }
        active = true;
        if (typeof document !== 'undefined') {
          document.body.classList.add('is-reordering-pins');
        }
        setActiveDrag({ spaceKey, fromIdx: idx, overIdx: null, label });
        requestAnimationFrame(() => positionGhost(lastX, lastY));
      } else {
        positionGhost(ev.clientX, ev.clientY);
      }

      const hit =
        typeof document !== 'undefined' ? document.elementFromPoint(ev.clientX, ev.clientY) : null;
      const dropTile = hit?.closest?.<HTMLElement>('[data-drag-space][data-drag-idx]');
      let nextOver: number | null = null;
      if (dropTile && dropTile.dataset.dragSpace === spaceKey) {
        const parsed = Number.parseInt(dropTile.dataset.dragIdx ?? '', 10);
        if (Number.isInteger(parsed) && parsed !== idx) {
          nextOver = parsed;
        }
      }
      if (nextOver !== currentOverIdx) {
        currentOverIdx = nextOver;
        setActiveDrag({ spaceKey, fromIdx: idx, overIdx: nextOver, label });
      }
    };

    const cleanup = () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onCancel);
      if (typeof document !== 'undefined') {
        document.body.classList.remove('is-reordering-pins');
      }
      pointerDragCleanupRef.current = null;
    };

    const onCancel = () => {
      cleanup();
      if (active) {
        setActiveDrag(null);
      }
    };

    const onUp = () => {
      cleanup();
      if (!active) return;
      suppressClickUntilRef.current =
        typeof performance !== 'undefined' ? performance.now() + 260 : 0;
      const req = computePointerReorderRequest(
        { spaceKey, idx },
        currentOverIdx !== null ? { spaceKey, idx: currentOverIdx } : null,
      );
      setActiveDrag(null);
      if (req) {
        onOptimisticReorder?.(req.space, req.fromIdx, req.toIdx);
        ipc<TestBarConfig>('reorder_pins', req)
          .then((updated) => {
            if (updated) onSetConfig?.(updated);
          })
          .catch(() => {
            ipc<TestBarConfig>('get_config')
              .then((fresh) => {
                if (fresh) onSetConfig?.(fresh);
              })
              .catch(() => {});
            onShowToast('Не удалось изменить порядок');
          });
      }
    };

    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onCancel);
    pointerDragCleanupRef.current = cleanup;
  };

  const getDragClasses = (spaceKey: string, idx: number): string => {
    if (!activeDrag || activeDrag.spaceKey !== spaceKey) return '';
    if (activeDrag.fromIdx === idx) return ' is-dragging';
    if (activeDrag.overIdx === idx) return ' drag-over';
    return '';
  };

  const makeDragProps = (spaceKey: string, idx: number, label: string) => ({
    'data-drag-space': spaceKey,
    'data-drag-idx': idx,
    onPointerDown: (e: React.PointerEvent<HTMLElement>) =>
      handlePinPointerDown(e, spaceKey, idx, label),
  });

  const renderEmptyStrip = (message: string) => (
    <div className="space-empty-inline">
      <span>{message}</span>
      <button type="button" className="pin-any-btn" onClick={onOpenDrawer}>
        <PlusIcon className="ic12" />
        <span>Закрепить</span>
      </button>
    </div>
  );

  const renderTile = (item: PinItem, idx: number, spaceKey: string, isHero: boolean) => {
    const running = isRunning(item);
    const nativeIc = resolveIconForPin(item);
    const honestAlt = getHonestShiftBadge(item);
    return (
      <div
        key={item.id || `tile-${spaceKey}-${item.title}-${idx}`}
        role="button"
        tabIndex={0}
        className={`app-tile ${isHero ? 'hero' : 'compact'}${getDragClasses(spaceKey, idx)}`}
        style={{ ['--tile-rgb' as string]: item.accentRgb || '139,150,248' }}
        onMouseMove={trackTilePointer}
        {...makeDragProps(spaceKey, idx, item.title)}
        onClick={() => handlePinTileClick(item)}
        onKeyDown={(e) => {
          if (e.target === e.currentTarget && isActivateKey(e)) {
            e.preventDefault();
            onLaunchPin(item);
          }
        }}
        onContextMenu={(e) => onOpenTileContext(e, item, spaceKey)}
      >
        {editMode && (
          <button
            type="button"
            className="tile-del"
            title="Открепить"
            aria-label={`Открепить ${item.title}`}
            onClick={(e) => {
              e.stopPropagation();
              ipc('remove_pin', { space: spaceKey, itemId: item.id }).catch(() => {
                onShowToast('Не удалось открепить элемент');
              });
            }}
          >
            <XIcon10 />
          </button>
        )}
        <div className="tile-ic">
          {nativeIc ? (
            <img src={nativeIc} alt="" className={isHero ? 'native-ic-lg' : 'native-ic'} draggable={false} />
          ) : (
            <FallbackAppIcon iconKey={item.iconKey} kind={item.kind} target={item.target} size={isHero ? 24 : 20} />
          )}
          {running && <span className="run-pill" aria-hidden="true" />}
        </div>
        <div className="tile-meta">
          <div className="tile-name">{item.title}</div>
          <div className={`tile-sub ${honestAlt ? 'has-alt' : ''}`}>
            {shiftHeld && honestAlt ? honestAlt : item.subtitle}
          </div>
        </div>
      </div>
    );
  };

  const renderQuickLink = (item: PinItem, idx: number, spaceKey: string) => {
    const nativeIc = resolveIconForPin(item);
    const honestAlt = getHonestShiftBadge(item);
    return (
      <div
        key={item.id || `tile-${spaceKey}-${item.title}-${idx}`}
        role="button"
        tabIndex={0}
        className={`qlink${getDragClasses(spaceKey, idx)}`}
        {...makeDragProps(spaceKey, idx, item.title)}
        onClick={() => handlePinTileClick(item)}
        onKeyDown={(e) => {
          if (e.target === e.currentTarget && isActivateKey(e)) {
            e.preventDefault();
            onLaunchPin(item);
          }
        }}
        onContextMenu={(e) => onOpenTileContext(e, item, spaceKey)}
      >
        {editMode && (
          <button
            type="button"
            className="tile-del tile-del-sm"
            title="Открепить"
            aria-label={`Открепить ${item.title}`}
            onClick={(e) => {
              e.stopPropagation();
              ipc('remove_pin', { space: spaceKey, itemId: item.id }).catch(() => {
                onShowToast('Не удалось открепить элемент');
              });
            }}
          >
            <XIcon10 />
          </button>
        )}
        {nativeIc ? (
          <img src={nativeIc} alt="" className="native-ic-sm" draggable={false} />
        ) : (
          <FallbackAppIcon iconKey={item.iconKey} kind={item.kind} target={item.target} size={17} />
        )}
        <div className="qlink-meta">
          <div className="qlink-name">{item.title}</div>
          <div className={`qlink-sub ${shiftHeld && honestAlt ? 'alt-on' : ''}`}>
            {shiftHeld && honestAlt ? honestAlt : item.subtitle}
          </div>
        </div>
      </div>
    );
  };

  const renderAudioDeviceBar = () => {
    if (!audio) {
      return <div className="qs-dev-placeholder" aria-hidden="true" />;
    }
    const allDevices = audio.devices ?? [];
    if (allDevices.length === 0) return null;

    let visibleDevs: AudioDeviceInfo[] = allDevices.slice(0, 3);
    const fullDefaultIdx = allDevices.findIndex((d) => d.isDefault);
    if (fullDefaultIdx >= 3) {
      visibleDevs = [allDevices[0], allDevices[1], allDevices[fullDefaultIdx]];
    }
    const activeIdx = getActiveDeviceIndex(visibleDevs);
    const hasOverflow = allDevices.length > 3;

    const switchOutput = (deviceId: string) => {
      ipc<AudioState>('set_audio_device', { deviceId })
        .then((next) => {
          onSetAudio(next);
          setDevOverflowOpen(false);
        })
        .catch((err) => {
          onShowToast(
            typeof err === 'string' && err.trim()
              ? err
              : 'Не удалось переключить вывод звука',
          );
        });
    };

    return (
      <div
        ref={devRowRef}
        className="qs-dev-row"
        style={devDropdownPresence.mounted ? { zIndex: 35 } : undefined}
      >
        <div
          className="seg-bar sliding"
          style={
            {
              gridTemplateColumns: `repeat(${visibleDevs.length}, 1fr)`,
              ['--seg-count' as string]: visibleDevs.length,
              ['--seg-idx' as string]: activeIdx,
            } as React.CSSProperties
          }
        >
          <span className={`seg-indicator ${segReady ? '' : 'no-anim'}`} aria-hidden="true" />
          {visibleDevs.map((d, idx) => (
            <button
              key={d.id || `dev-${d.name}-${idx}`}
              type="button"
              className={`seg-opt ${d.isDefault ? 'active' : ''}`}
              title={d.name}
              onClick={() => switchOutput(d.id)}
            >
              {d.shortName}
            </button>
          ))}
        </div>

        {hasOverflow && (
          <button
            type="button"
            className={`qs-dev-more-btn ${devOverflowOpen ? 'open' : ''}`}
            title={`Все устройства вывода (${allDevices.length})`}
            aria-label="Все устройства вывода"
            aria-expanded={devOverflowOpen}
            onClick={() => setDevOverflowOpen((v) => !v)}
          >
            <ChevronDownIcon className="ic12" />
          </button>
        )}

        {hasOverflow && devDropdownPresence.mounted && (
          <div className="qs-dev-dropdown" data-phase={devDropdownPresence.phase}>
            {allDevices.map((d, idx) => (
              <button
                key={d.id || `all-dev-${d.name}-${idx}`}
                type="button"
                className={`pp-row ${d.isDefault ? 'active' : ''}`}
                onClick={() => switchOutput(d.id)}
              >
                <span className="pp-ic"><VolIcon /></span>
                <div className="pp-meta">
                  <div className="pp-name">{d.shortName}</div>
                  <div className="pp-sub">{d.name}</div>
                </div>
              </button>
            ))}
          </div>
        )}
      </div>
    );
  };

  const renderMicToggleRow = (marginBottom = 0) => {
    const micMuted = audio?.micMuted ?? false;
    const liveMicPeak = micMuted || !peaks ? 0 : Math.max(0, Math.min(1, peaks.micPeak));
    const inputDevices = audio?.inputDevices ?? [];
    const activeInput = inputDevices.find((d) => d.isDefault);

    return (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6, marginBottom }}>
        <button
          type="button"
          className={`sys-tgl ${!micMuted ? 'on' : ''}`}
          style={{ width: '100%' }}
          onClick={() =>
            ipc<AudioState>('set_mic_muted', { muted: !micMuted })
              .then(onSetAudio)
              .catch((err) => {
                onShowToast(typeof err === 'string' && err.trim() ? err : 'Не удалось переключить микрофон');
              })
          }
        >
          <MicIcon className="ic16" muted={micMuted} />
          <div style={{ minWidth: 0, flex: 1 }}>
            <div className="sys-tgl-t">Микрофон</div>
            <div className="sys-tgl-s">{micMuted ? 'Выключен' : 'Активен'}</div>
          </div>
          {!micMuted && (
            <div
              className="bat-track"
              style={{ width: 42, height: 4 }}
              aria-hidden="true"
              title="Уровень сигнала микрофона"
            >
              <span
                className="bat-fill"
                style={
                  {
                    width: '100%',
                    background: 'var(--ok)',
                    ['--pct' as string]: liveMicPeak,
                  } as React.CSSProperties
                }
              />
            </div>
          )}
          <span className={`mic-status-dot ${micMuted ? 'muted' : ''}`} aria-hidden="true" />
        </button>

        {inputDevices.length > 1 && (
          <select
            className="qs-input-select"
            aria-label="Устройство ввода"
            value={activeInput?.id ?? inputDevices[0]?.id ?? ''}
            onChange={(e) => {
              setInputDevice(e.target.value)
                .then(onSetAudio)
                .catch(() => onShowToast('Не удалось переключить микрофон'));
            }}
          >
            {inputDevices.map((d, idx) => (
              <option key={d.id || `in-dev-${d.name}-${idx}`} value={d.id}>
                {d.shortName || d.name}
              </option>
            ))}
          </select>
        )}
      </div>
    );
  };

  const scheduleThrottledAudioIpc = (key: string, fn: () => void, flushNow: boolean) => {
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

  const handleSliderDragToggle = (sliderKey: string, dragging: boolean) => {
    if (dragging) {
      draggingAudioKeysRef.current.add(sliderKey);
    } else {
      draggingAudioKeysRef.current.delete(sliderKey);
    }
    onAudioDragStateChange?.(sliderKey, dragging);
  };

  const renderAudioMixerCard = (
    bentoIndex: number,
    options: { title: string; showDeviceSwitcher: boolean; showMicToggle: boolean; showSessions: boolean },
  ) => {
    const masterVol = audio?.masterVolume ?? 0;
    const masterMuted = audio?.masterMuted ?? false;
    const liveMasterPeak = masterMuted || !peaks ? 0 : peaks.masterPeak;
    const sessions = audio?.sessions ?? [];

    const updateMaster = (nextVol: number, nextMuted: boolean, commit = false) => {
      const reqSeq = (audioReqSeqRef.current.get('__master__') ?? 0) + 1;
      audioReqSeqRef.current.set('__master__', reqSeq);
      if (audio) {
        onSetAudio({ ...audio, masterVolume: nextVol, masterMuted: nextMuted });
      }
      scheduleThrottledAudioIpc(
        '__master__',
        () => {
          ipc<AudioState>('set_master_volume', { volume: nextVol, muted: nextMuted })
            .then((nextState) => {
              if (
                audioReqSeqRef.current.get('__master__') === reqSeq &&
                !draggingAudioKeysRef.current.has('__master__')
              ) {
                onSetAudio(nextState, true);
              }
            })
            .catch(() => onShowToast('Не удалось изменить громкость'));
        },
        commit,
      );
    };

    const updateSessionVolume = (
      s: AudioState['sessions'][number],
      nextVol: number,
      commit = false,
    ) => {
      const reqSeq = (audioReqSeqRef.current.get(s.id) ?? 0) + 1;
      audioReqSeqRef.current.set(s.id, reqSeq);
      if (audio) {
        onSetAudio({
          ...audio,
          sessions: audio.sessions.map((item) =>
            item.id === s.id ? { ...item, volume: nextVol, muted: false } : item,
          ),
        });
      }
      scheduleThrottledAudioIpc(
        s.id,
        () => {
          setSessionState({
            appKey: s.id,
            pid: s.pid,
            pids: s.pids,
            volume: normalizeSessionVolumeScalar(nextVol),
            muted: s.muted ? false : undefined,
          })
            .then((nextState) => {
              if (
                audioReqSeqRef.current.get(s.id) === reqSeq &&
                !draggingAudioKeysRef.current.has(s.id)
              ) {
                onSetAudio(nextState, true);
              }
            })
            .catch(() => onShowToast(`Не удалось изменить громкость ${s.name}`));
        },
        commit,
      );
    };

    const toggleSessionMute = (s: AudioState['sessions'][number]) => {
      const nextMuted = !s.muted;
      if (audio) {
        onSetAudio({
          ...audio,
          sessions: audio.sessions.map((item) =>
            item.id === s.id ? { ...item, muted: nextMuted } : item,
          ),
        });
      }
      setSessionState({
        appKey: s.id,
        pid: s.pid,
        pids: s.pids,
        muted: nextMuted,
      })
        .then((nextState) => onSetAudio(nextState, true))
        .catch(() => onShowToast(`Не удалось переключить звук ${s.name}`));
    };

    const audioDropdownActive = Boolean(options.showDeviceSwitcher && devDropdownPresence.mounted);

    return (
      <BentoSpotCard
        bentoIndex={bentoIndex}
        className={`compact-card${audioDropdownActive ? ' has-open-pop' : ''}`}
        style={audioDropdownActive ? { overflow: 'visible', zIndex: 25 } : undefined}
      >
        <div className="sec-row" style={{ marginBottom: 6 }}>
          <span className="sec-title">{options.title}</span>
        </div>

        {options.showDeviceSwitcher && renderAudioDeviceBar()}
        {options.showMicToggle && renderMicToggleRow(options.showSessions ? 6 : 0)}

        {options.showSessions && (
          <>
            {/* Master volume ALWAYS accessible above per-app mixer */}
            <div className={`mix-row qs-master-row ${masterMuted ? 'muted' : ''}`} style={{ marginBottom: 5 }}>
              <button
                type="button"
                className="mix-mute"
                title={masterMuted ? 'Включить общий звук' : 'Выключить общий звук'}
                aria-label={masterMuted ? 'Включить общий звук' : 'Выключить общий звук'}
                onClick={() => updateMaster(masterVol, !masterMuted, true)}
              >
                <VolIcon muted={masterMuted} level={masterVol} />
              </button>
              <span className="mix-name" style={{ fontWeight: 600 }} title="Общий звук">
                Общий звук
              </span>
              <ObsidianSlider
                value={masterVol}
                muted={masterMuted}
                peak={liveMasterPeak}
                noTransition={!sliderAnimReady}
                ariaLabel="Общий звук"
                onDragStateChange={(d) => handleSliderDragToggle('__master__', d)}
                onChange={(next) => updateMaster(next, false, false)}
                onCommit={(next) => updateMaster(next, false, true)}
              />
              <span className="mix-val">{masterMuted ? '0%' : `${masterVol}%`}</span>
            </div>

            <div className="mixer-list mixer-scroll">
              {sessions.length === 0 ? (
                <div className="qs-empty">Нет активных аудиопотоков</div>
              ) : (
                sessions.map((s, idx) => {
                  const iconUri = resolveIconKeyUri(s.iconKey, null);
                  const livePeak = s.muted || !peaks ? 0 : (peaks.sessionPeaks[s.id] ?? 0);
                  const isEmitting = !s.muted && livePeak > 0.02;
                  return (
                    <div
                      key={s.id || `sess-${s.pid ?? idx}-${s.name}-${idx}`}
                      className={`mix-row ${s.muted ? 'muted' : ''} ${isEmitting ? 'active-audio' : ''}`}
                    >
                      <button
                        type="button"
                        className="mix-mute"
                        title={s.muted ? `Включить звук ${s.name}` : `Выключить звук ${s.name}`}
                        aria-label={s.muted ? `Включить звук ${s.name}` : `Выключить звук ${s.name}`}
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
                        noTransition={!sliderAnimReady}
                        ariaLabel={`Громкость ${s.name}`}
                        onDragStateChange={(d) => handleSliderDragToggle(s.id, d)}
                        onChange={(next) => updateSessionVolume(s, next, false)}
                        onCommit={(next) => updateSessionVolume(s, next, true)}
                      />
                      <span className="mix-val">{s.muted ? '0%' : `${s.volume}%`}</span>
                    </div>
                  );
                })
              )}
            </div>
          </>
        )}
      </BentoSpotCard>
    );
  };

  const renderMediaCard = (bentoIndex: number) => {
    const hasSession = Boolean(media?.hasSession);
    const isPlaying = Boolean(media?.isPlaying);
    const sessions = media?.sessions ?? [];
    const durationSec = media?.durationSec ?? 0;
    const interpolatedSec = computeInterpolatedMediaSec(media, mediaNowMs);
    const displayedPosSec = scrubOverrideSec !== null ? scrubOverrideSec : interpolatedSec;
    const canSeek = Boolean(hasSession && media?.canSeek && durationSec > 0);
    const progressRatio = durationSec > 0 ? Math.min(1, Math.max(0, displayedPosSec / durationSec)) : 0;

    return (
      <BentoSpotCard
        bentoIndex={bentoIndex}
        className={`media-card ${hasSession ? 'playing' : 'idle'} ${!isPlaying ? 'paused' : ''}`}
        style={
          hasSession && mediaAmbientRgb
            ? ({ ['--media-ambient-rgb' as string]: mediaAmbientRgb } as React.CSSProperties)
            : undefined
        }
      >
        {hasSession && sessions.length > 1 && (
          <div className="media-sessions" role="tablist" aria-label="Источники воспроизведения">
            {sessions.map((s, idx) => (
              <button
                key={s.id || `msess-${s.appName}-${idx}`}
                type="button"
                role="tab"
                aria-selected={s.isCurrent}
                className={`media-session-pill ${s.isCurrent ? 'active' : ''}`}
                title={`${s.appName}: ${s.title}`}
                onClick={() => {
                  selectMediaSession(s.id)
                    .then(onSetMedia)
                    .catch(() => onShowToast('Не удалось переключить источник'));
                }}
              >
                {s.appName}
              </button>
            ))}
          </div>
        )}

        <div className="media-top" style={hasSession && durationSec <= 0 ? { marginBottom: 0 } : undefined}>
          <div className="album-box">
            {hasSession && media?.thumbnail ? (
              <img src={media.thumbnail} alt="" className="album-art-img" draggable={false} />
            ) : hasSession ? (
              <div className="eq-wave"><i /><i /><i /><i /></div>
            ) : (
              <MusicNoteIcon className="ic18" />
            )}
          </div>
          <div className="media-meta">
            <div className="media-title">{hasSession ? (media?.title || 'Без названия') : 'Ничего не воспроизводится'}</div>
            <div className="media-sub">
              {hasSession
                ? [media?.artist, media?.source].filter(Boolean).join(' · ') || 'Медиапоток'
                : 'Нет активного медиапотока'}
            </div>
          </div>
          <div className="m-ctrls">
            <button
              type="button"
              className="m-btn"
              disabled={!hasSession}
              aria-label="Предыдущий трек"
              onClick={() =>
                ipc<MediaState>('media_action', { action: 'prev' })
                  .then(onSetMedia)
                  .catch((err) => {
                    if (typeof err === 'string' && err.trim()) onShowToast(err);
                  })
              }
            >
              <PrevTrackIcon className="ic14" />
            </button>
            <button
              type="button"
              className="m-btn play qs-play-btn"
              disabled={!hasSession}
              aria-label={isPlaying ? 'Пауза' : 'Воспроизвести'}
              onClick={() =>
                ipc<MediaState>('media_action', { action: 'toggle' })
                  .then(onSetMedia)
                  .catch((err) => {
                    if (typeof err === 'string' && err.trim()) onShowToast(err);
                  })
              }
            >
              {isPlaying ? <PauseIcon className="ic14" /> : <PlaySmallIcon className="ic14" />}
            </button>
            <button
              type="button"
              className="m-btn"
              disabled={!hasSession}
              aria-label="Следующий трек"
              onClick={() =>
                ipc<MediaState>('media_action', { action: 'next' })
                  .then(onSetMedia)
                  .catch((err) => {
                    if (typeof err === 'string' && err.trim()) onShowToast(err);
                  })
              }
            >
              <NextTrackIcon className="ic14" />
            </button>
          </div>
        </div>

        {hasSession && durationSec > 0 && (
          <div className="scrub-wrap">
            <span className="scrub-time">{formatMediaDuration(displayedPosSec)}</span>
            {canSeek ? (
              <ObsidianSlider
                className="media-scrub-slider"
                min={0}
                max={durationSec}
                step={1}
                wheelStep={5}
                value={displayedPosSec}
                magneticDetents={[]}
                showCenterNotch={false}
                ariaLabel="Позиция воспроизведения"
                onChange={(sec) => setScrubOverrideSec(sec)}
                onCommit={(sec) => {
                  setScrubOverrideSec(sec);
                  seekMedia(sec)
                    .then((next) => {
                      setScrubOverrideSec(null);
                      onSetMedia(next);
                    })
                    .catch(() => {
                      setScrubOverrideSec(null);
                      onShowToast('Перемотка не поддерживается плеером');
                    });
                }}
              />
            ) : (
              <div className="scrub-bar">
                <div
                  className="scrub-fill"
                  style={{ ['--pct' as string]: progressRatio } as React.CSSProperties}
                />
              </div>
            )}
            <span className="scrub-time">{formatMediaDuration(durationSec)}</span>
          </div>
        )}
      </BentoSpotCard>
    );
  };

  const renderClipboardCard = (bentoIndex: number) => (
    <BentoSpotCard bentoIndex={bentoIndex} className="clip-card">
      <div className="sec-row" style={{ marginBottom: 6 }}>
        <span className="sec-title">Буфер обмена</span>
        <div className="sec-actions">
          <button
            type="button"
            className="sec-act"
            title="Поиск по буферу обмена"
            aria-label="Поиск по буферу обмена"
            onClick={() => {
              setClipSearchOpen((v) => {
                const next = !v;
                if (next) requestAnimationFrame(() => clipSearchRef.current?.focus());
                else setClipQuery('');
                return next;
              });
            }}
          >
            <SearchIcon className="ic12" />
          </button>
          <button
            type="button"
            className={`clip-pause-btn ${clipPaused ? 'paused' : ''}`}
            title={clipPaused ? 'Возобновить запись буфера' : 'Приостановить запись буфера'}
            aria-pressed={clipPaused}
            onClick={() => {
              const next = !clipPaused;
              setClipPaused(next);
              setClipboardPaused(next)
                .then((confirmed) => {
                  setClipPaused(confirmed);
                  onShowToast(confirmed ? 'Запись буфера на паузе' : 'Запись буфера возобновлена');
                })
                .catch(() => {
                  setClipPaused(!next);
                  onShowToast('Не удалось изменить статус буфера');
                });
            }}
          >
            <PauseIcon size={10} />
            <span>{clipPaused ? 'На паузе' : 'Пауза'}</span>
          </button>
          <button
            type="button"
            className="sec-act"
            onClick={() =>
              ipc<ClipboardEntry[]>('clear_unpinned_clips')
                .then(onSetClips)
                .catch(() => onShowToast('Не удалось очистить буфер'))
            }
          >
            Очистить
          </button>
        </div>
      </div>

      {clipSearchOpen && (
        <div className="clip-toolbar">
          <div className="clip-search-wrap">
            <SearchIcon className="ic12" color="var(--t3)" />
            <input
              ref={clipSearchRef}
              className="clip-search-input"
              placeholder="Фильтр записей буфера…"
              value={clipQuery}
              onChange={(e) => setClipQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  e.stopPropagation();
                  if (clipQuery) setClipQuery('');
                  else setClipSearchOpen(false);
                }
              }}
            />
            {clipQuery && (
              <button
                type="button"
                className="drawer-filter-clear"
                aria-label="Очистить поиск"
                onClick={() => {
                  setClipQuery('');
                  clipSearchRef.current?.focus();
                }}
              >
                <CloseWindowIcon className="ic12" />
              </button>
            )}
          </div>
        </div>
      )}

      <div className="clip-list">
        {filteredClips.length === 0 ? (
          <div className="clip-empty">
            {clipQuery.trim() ? 'Записи по запросу не найдены' : 'Буфер обмена пока пуст'}
          </div>
        ) : (
          filteredClips.map((c, idx) => {
            const kind = resolveClipboardKind(c);
            const hexRgb = kind === 'hex' ? hexColorToRgbString(c.color || c.text) : null;
            const swatchColor = c.color || (kind === 'hex' ? c.text.trim() : null);
            const displayPreview = c.preview || c.text;
            const lines = c.lineCount ?? 1;

            const triggerClipPrimary = (shiftCopyOnly: boolean) => {
              if (shiftCopyOnly) {
                ipc('copy_to_clipboard', { text: c.text })
                  .then(() => {
                    setCopiedClipId(c.id);
                    onShowToast('Скопировано');
                    window.setTimeout(() => setCopiedClipId((cur) => (cur === c.id ? null : cur)), 900);
                  })
                  .catch(() => onShowToast('Не удалось скопировать'));
                return;
              }
              pasteClipboardEntry(c.text)
                .then(() => {
                  setCopiedClipId(c.id);
                  window.setTimeout(() => setCopiedClipId((cur) => (cur === c.id ? null : cur)), 700);
                })
                .catch(() => {
                  // Fallback to plain copy if paste target unavailable
                  ipc('copy_to_clipboard', { text: c.text })
                    .then(() => onShowToast('Скопировано в буфер'))
                    .catch(() => onShowToast('Не удалось вставить запись'));
                });
            };

            return (
              <div
                key={c.id || `clip-${idx}`}
                role="button"
                tabIndex={0}
                title={c.text}
                className={`clip-item ${copiedClipId === c.id ? 'copied' : ''}`}
                onClick={(e) => triggerClipPrimary(e.shiftKey)}
                onKeyDown={(e) => {
                  if (e.target === e.currentTarget && isActivateKey(e)) {
                    e.preventDefault();
                    triggerClipPrimary(e.shiftKey);
                  }
                }}
              >
                {swatchColor ? (
                  <span className="clip-swatch" style={{ background: swatchColor }} aria-hidden="true" />
                ) : (
                  <span className={`clip-kind-tag k-${kind}`}>{getClipboardKindLabel(kind)}</span>
                )}
                <span className="clip-txt">{displayPreview}</span>
                {lines > 1 && <span className="clip-lines-meta">{lines} стр.</span>}

                <div className="clip-actions">
                  {kind === 'url' && (
                    <button
                      type="button"
                      className="clip-act-btn"
                      title="Открыть ссылку"
                      aria-label="Открыть ссылку"
                      onClick={(e) => {
                        e.stopPropagation();
                        ipc('launch_item', {
                          target: c.text.trim(),
                          asAdmin: false,
                          closeStart: true,
                        }).catch(() => onShowToast('Не удалось открыть ссылку'));
                      }}
                    >
                      <ExternalArrowIcon className="ic12" />
                    </button>
                  )}
                  {kind === 'path' && (
                    <button
                      type="button"
                      className="clip-act-btn"
                      title="Показать расположение"
                      aria-label="Показать расположение"
                      onClick={(e) => {
                        e.stopPropagation();
                        ipc('open_location', { path: c.text.trim() }).catch(() => {
                          onShowToast('Не удалось открыть расположение');
                        });
                      }}
                    >
                      <FolderSvg className="ic12" />
                    </button>
                  )}
                  {kind === 'hex' && hexRgb && (
                    <button
                      type="button"
                      className="clip-act-btn"
                      title={`Скопировать ${hexRgb}`}
                      aria-label="Скопировать RGB"
                      onClick={(e) => {
                        e.stopPropagation();
                        ipc('copy_to_clipboard', { text: hexRgb })
                          .then(() => onShowToast(`Скопировано: ${hexRgb}`))
                          .catch(() => onShowToast('Не удалось скопировать RGB'));
                      }}
                    >
                      <CopyIcon className="ic12" />
                    </button>
                  )}
                  <button
                    type="button"
                    className={`clip-pin ${c.pinned ? 'pinned' : ''}`}
                    title={c.pinned ? 'Открепить запись' : 'Закрепить запись'}
                    aria-label={c.pinned ? 'Открепить запись' : 'Закрепить запись'}
                    onClick={(e) => {
                      e.stopPropagation();
                      ipc<ClipboardEntry[]>('toggle_clip_pin', { id: c.id })
                        .then(onSetClips)
                        .catch(() => onShowToast('Не удалось обновить закрепление'));
                    }}
                  >
                    <PinIcon className="ic12" />
                  </button>
                  <button
                    type="button"
                    className="clip-act-btn danger"
                    title="Удалить запись"
                    aria-label="Удалить запись"
                    onClick={(e) => {
                      e.stopPropagation();
                      deleteClipboardEntry(c.id)
                        .then(onSetClips)
                        .catch(() => onShowToast('Не удалось удалить запись'));
                    }}
                  >
                    <TrashIcon className="ic12" />
                  </button>
                </div>
              </div>
            );
          })
        )}
      </div>
    </BentoSpotCard>
  );

  const renderSparklineSvg = (series: readonly number[], pct: number) => {
    const geom = buildSparklinePath(series, 56, 16);
    if (!geom.linePath) return null;
    const lvlCls = getTelemetryLevelClass(pct);
    return (
      <svg
        className={`health-spark-svg ${lvlCls}`}
        viewBox="0 0 56 16"
        preserveAspectRatio="none"
        aria-hidden="true"
      >
        <path className="health-spark-area" d={geom.areaPath} />
        <path className="health-spark-line" d={geom.linePath} />
      </svg>
    );
  };

  const renderDiskStrip = () => {
    const disks = telem?.disks ?? [];
    if (disks.length === 0) return null;
    return (
      <div className="disk-strip">
        {disks.map((d, idx) => {
          const lvl = getTelemetryLevelClass(d.usedPct);
          const pct = Math.max(0, Math.min(100, Math.round(Number.isFinite(d.usedPct) ? d.usedPct : 0)));
          return (
            <div
              key={d.mount || `disk-${idx}`}
              className="disk-chip"
              title={`${d.mount} (${d.name || 'Диск'}): ${pct}% · ${formatGbValue(d.freeGb)} ГБ свободно`}
            >
              <div className="disk-chip-top">
                <span className="disk-mount">
                  <DiskIcon className="ic12" />
                  <span>{d.mount}</span>
                </span>
                <span className="disk-free">{formatDiskSummary(d)}</span>
              </div>
              <div className="disk-bar">
                <div
                  className={`disk-fill ${lvl}`}
                  style={{ ['--pct' as string]: Math.min(1, Math.max(0, d.usedPct / 100)) } as React.CSSProperties}
                />
              </div>
            </div>
          );
        })}
      </div>
    );
  };

  const renderTelemetryCard = (bentoIndex: number, variant: 'full' | 'headroom') => {
    const cpuPct = telem?.cpuPct ?? 0;
    const ramPct = telem?.ramPct ?? 0;
    const gpuPct = telem?.gpuPct ?? 0;
    const vramPct = telem?.vramPct ?? 0;

    if (variant === 'headroom') {
      return (
        <BentoSpotCard bentoIndex={bentoIndex} className="compact-card">
          <div className="sec-row" style={{ marginBottom: 9 }}>
            <span className="sec-title">Запас памяти</span>
            <span className={`status-pill ${getTelemetryStatusPill(ramPct).cls}`}>
              {getTelemetryStatusPill(ramPct).label}
            </span>
          </div>
          <div className="health-list">
            {telem?.gpuAvailable && (
              <div className="health-row">
                <div className="health-top">
                  <span className="health-lbl">
                    VRAM <span className="health-sub">· {telem.gpuName} · {telem.gpuTempC}°C</span>
                  </span>
                  <span className="health-val">
                    {formatMemoryRatio(telem.vramUsedGb, telem.vramTotalGb)}{' '}
                    <span className={`status-pill ${getTelemetryStatusPill(vramPct).cls}`}>
                      {vramPct}%
                    </span>
                  </span>
                </div>
                <div className="health-bar-row">
                  <div className="health-bar">
                    <div
                      className={`health-fill ${getTelemetryLevelClass(vramPct)}`}
                      style={{ width: '100%', ['--pct' as string]: Math.min(1, vramPct / 100) } as React.CSSProperties}
                    />
                  </div>
                  {renderSparklineSvg(telemHistory.vram, vramPct)}
                </div>
              </div>
            )}

            <div className="health-row">
              <div className="health-top">
                <span className="health-lbl">
                  ОЗУ <span className="health-sub">· RAM</span>
                </span>
                <span className="health-val">
                  {telem ? formatMemoryRatio(telem.ramUsedGb, telem.ramTotalGb) : '—'}{' '}
                  <span className={`status-pill ${getTelemetryStatusPill(ramPct).cls}`}>
                    {ramPct}%
                  </span>
                </span>
              </div>
              <div className="health-bar-row">
                <div className="health-bar">
                  <div
                    className={`health-fill ${getTelemetryLevelClass(ramPct)}`}
                    style={{ width: '100%', ['--pct' as string]: Math.min(1, ramPct / 100) } as React.CSSProperties}
                  />
                </div>
                {renderSparklineSvg(telemHistory.ram, ramPct)}
              </div>
            </div>
          </div>
          {renderDiskStrip()}
        </BentoSpotCard>
      );
    }

    return (
      <BentoSpotCard bentoIndex={bentoIndex} className="compact-card">
        <div className="sec-row" style={{ marginBottom: 9 }}>
          <span className="sec-title">Нагрузка ПК</span>
          <span className="health-hw-meta">
            {telem ? `${Math.round(telem.ramTotalGb)} ГБ${telem.gpuAvailable ? ` · ${telem.gpuName}` : ''}` : 'Система'}
          </span>
        </div>
        <div className="health-list">
          <div className="health-row">
            <div className="health-top">
              <span className="health-lbl">
                Процессор <span className="health-sub">· {telem?.cpuName || 'CPU'}</span>
              </span>
              <span className="health-val">
                {cpuPct}%{' '}
                <span className={`status-pill ${getTelemetryStatusPill(cpuPct).cls}`}>
                  {getTelemetryStatusPill(cpuPct).label}
                </span>
              </span>
            </div>
            <div className="health-bar-row">
              <div className="health-bar">
                <div
                  className={`health-fill ${getTelemetryLevelClass(cpuPct)}`}
                  style={{ width: '100%', ['--pct' as string]: Math.min(1, cpuPct / 100) } as React.CSSProperties}
                />
              </div>
              {renderSparklineSvg(telemHistory.cpu, cpuPct)}
            </div>
          </div>

          <div className="health-row">
            <div className="health-top">
              <span className="health-lbl">
                ОЗУ <span className="health-sub">· RAM</span>
              </span>
              <span className="health-val">
                {telem ? formatMemoryRatio(telem.ramUsedGb, telem.ramTotalGb) : '—'}{' '}
                <span className={`status-pill ${getTelemetryStatusPill(ramPct).cls}`}>
                  {getTelemetryStatusPill(ramPct).label}
                </span>
              </span>
            </div>
            <div className="health-bar-row">
              <div className="health-bar">
                <div
                  className={`health-fill ${getTelemetryLevelClass(ramPct)}`}
                  style={{ width: '100%', ['--pct' as string]: Math.min(1, ramPct / 100) } as React.CSSProperties}
                />
              </div>
              {renderSparklineSvg(telemHistory.ram, ramPct)}
            </div>
          </div>

          {telem?.gpuAvailable && (
            <>
              <div className="health-row">
                <div className="health-top">
                  <span className="health-lbl">
                    {telem.gpuName} <span className="health-sub">· {telem.gpuTempC}°C</span>
                  </span>
                  <span className="health-val">
                    {gpuPct}%{' '}
                    <span className={`status-pill ${getGpuTempStatusPill(telem.gpuTempC).cls}`}>
                      {getGpuTempStatusPill(telem.gpuTempC).label}
                    </span>
                  </span>
                </div>
                <div className="health-bar-row">
                  <div className="health-bar">
                    <div
                      className={`health-fill ${getTelemetryLevelClass(gpuPct)}`}
                      style={{ width: '100%', ['--pct' as string]: Math.min(1, gpuPct / 100) } as React.CSSProperties}
                    />
                  </div>
                  {renderSparklineSvg(telemHistory.gpu, gpuPct)}
                </div>
              </div>

              <div className="health-row">
                <div className="health-top">
                  <span className="health-lbl">
                    Видеопамять <span className="health-sub">· VRAM</span>
                  </span>
                  <span className="health-val">
                    {formatMemoryRatio(telem.vramUsedGb, telem.vramTotalGb)}{' '}
                    <span className={`status-pill ${getTelemetryStatusPill(vramPct).cls}`}>
                      {getTelemetryStatusPill(vramPct).label}
                    </span>
                  </span>
                </div>
                <div className="health-bar-row">
                  <div className="health-bar">
                    <div
                      className={`health-fill ${getTelemetryLevelClass(vramPct)}`}
                      style={{ width: '100%', ['--pct' as string]: Math.min(1, vramPct / 100) } as React.CSSProperties}
                    />
                  </div>
                  {renderSparklineSvg(telemHistory.vram, vramPct)}
                </div>
              </div>
            </>
          )}
        </div>
        {renderDiskStrip()}
      </BentoSpotCard>
    );
  };

  const renderProcessConsumersCard = (
    bentoIndex: number,
    mode: 'ram' | 'gpu',
    targetSpace: string,
  ) => {
    const list = mode === 'gpu' ? studioProcesses : (telem?.topProcesses ?? []);
    return (
      <BentoSpotCard bentoIndex={bentoIndex} className="consumer-card">
        <div className="sec-row" style={{ marginBottom: 8 }}>
          <span className="sec-title">
            {mode === 'gpu' ? 'Активные процессы GPU / ОЗУ' : 'Кто сейчас ест память'}
          </span>
          {mode === 'ram' && (
            <button
              type="button"
              className="sec-act sec-act-ic"
              onClick={() =>
                ipc('launch_item', {
                  target: 'C:\\Windows\\System32\\Taskmgr.exe',
                  asAdmin: false,
                  closeStart: true,
                }).catch(() => onShowToast('Не удалось открыть диспетчер задач'))
              }
            >
              <span>Диспетчер</span>
              <ExternalArrowIcon />
            </button>
          )}
        </div>
        {killError?.space === targetSpace && (
          <div className="consumer-err" role="alert">
            <span>{killError.message}</span>
            <button
              type="button"
              className="consumer-err-close"
              aria-label="Скрыть ошибку"
              onClick={() => setKillError(null)}
            >
              <XIcon10 />
            </button>
          </div>
        )}
        <div className="consumer-list">
          {list.length === 0 ? (
            <div className="clip-empty">Нет активных процессов</div>
          ) : (
            list.map((pr, idx) => {
              const metrics = formatProcessMetricLine(pr, mode);
              const iconUri = resolveIconKeyUri(pr.iconKey, pr.iconData);
              const procKey = `${targetSpace}:${pr.name}:${pr.pid}`;
              const isArmed = armedKillKey === procKey;
              const canFocus = typeof pr.hwnd === 'number' && pr.hwnd > 0;

              const focusProcWindow = () => {
                if (!canFocus) return;
                ipc('focus_window', { hwnd: pr.hwnd, toggleMinimize: false }).catch(() => {
                  onShowToast(`Не удалось перейти к ${pr.name}`);
                });
              };

              return (
                <div
                  key={`${pr.name}-${pr.pid ?? idx}-${idx}`}
                  role={canFocus ? 'button' : undefined}
                  tabIndex={canFocus ? 0 : undefined}
                  title={canFocus ? `Перейти к окну ${pr.name}` : pr.name}
                  className={`consumer-row ${canFocus ? 'focusable' : ''}`}
                  onClick={canFocus ? focusProcWindow : undefined}
                  onKeyDown={
                    canFocus
                      ? (e) => {
                          if (e.target === e.currentTarget && isActivateKey(e)) {
                            e.preventDefault();
                            focusProcWindow();
                          }
                        }
                      : undefined
                  }
                >
                  {iconUri ? (
                    <img src={iconUri} alt="" className="native-ic-sm" draggable={false} />
                  ) : (
                    <FallbackAppIcon iconKey={pr.name} size={18} />
                  )}
                  <div className="consumer-meta">
                    <div className="consumer-name">{pr.name}</div>
                    <div className="consumer-sub">
                      {pr.cpuPct}% CPU{pr.pids.length > 1 ? ` · ${pr.pids.length} проц.` : ''}
                    </div>
                  </div>
                  <div className="consumer-metrics">
                    {metrics.gpuBadge && (
                      <span className="consumer-vram-pill">{metrics.gpuBadge}</span>
                    )}
                    <span className="consumer-mem">{metrics.primary}</span>
                  </div>
                  <button
                    type="button"
                    className={`consumer-kill ${isArmed ? 'armed' : ''}`}
                    title={
                      isArmed
                        ? 'Нажмите ещё раз для завершения'
                        : pr.pids.length > 1
                          ? `Завершить все (${pr.pids.length})`
                          : 'Завершить процесс'
                    }
                    aria-label={isArmed ? `Подтвердить завершение ${pr.name}` : `Завершить ${pr.name}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      handleKillProcessClick(pr, targetSpace);
                    }}
                  >
                    {isArmed ? <span>Подтвердить</span> : <KillIcon />}
                  </button>
                </div>
              );
            })
          )}
        </div>
      </BentoSpotCard>
    );
  };

  const dragGhost =
    activeDrag && typeof document !== 'undefined'
      ? createPortal(
          <div ref={dragGhostRef} className="pin-drag-ghost" aria-hidden="true">
            {activeDrag.label}
          </div>,
          document.body,
        )
      : null;

  const panelClass = `space-panel${rapidSwitch ? ' rapid' : ''}`;
  const panelStyle = { ['--space-dir' as string]: spaceDir } as React.CSSProperties;

  /* ===================== SPACE 1: ЛИЧНОЕ ===================== */
  if (space === 'personal') {
    const hasPersonalPins = (config?.personalPins.length ?? 0) > 0;
    const hasPersonalLinks = (config?.personalLinks.length ?? 0) > 0;

    return (
      <>
      <div key={space} className={panelClass} style={panelStyle}>
        <div className="bento-col">
          <BentoSpotCard bentoIndex={0} className="space-card-flex">
            <div className="sec-row">
              <span className="sec-title">Закреплено</span>
              <button type="button" className="sec-act sec-act-ic" onClick={onOpenDrawer}>
                <PlusIcon className="ic12" />
                <span>Закрепить</span>
              </button>
            </div>

            <div className="space-scroll-main">
              {!hasPersonalPins ? (
                renderEmptyStrip('Нет закреплённых программ')
              ) : (
                <>
                  {personalPartition.heroes.length > 0 && (
                    <div className="hero-pair">
                      {personalPartition.heroes.map((p, i) => renderTile(p, i, 'personal', true))}
                    </div>
                  )}
                  {personalPartition.compact.length > 0 && (
                    <div className="sub-grid">
                      {personalPartition.compact.map((p, i) => renderTile(p, i + 2, 'personal', false))}
                    </div>
                  )}
                </>
              )}
            </div>

            <div className="space-dock-foot">
              <div className="sec-row" style={{ marginBottom: 8 }}>
                <span className="sec-title">Быстрый доступ</span>
                <button type="button" className="sec-act sec-act-ic" onClick={onOpenDrawer}>
                  <PlusIcon className="ic12" />
                  <span>Папка / Файл / URL</span>
                </button>
              </div>
              {!hasPersonalLinks ? (
                renderEmptyStrip('Нет папок или ссылок быстрого доступа')
              ) : (
                <div className="qlinks-grid qlinks-scroll">
                  {(config?.personalLinks ?? []).map((l, idx) =>
                    renderQuickLink(l, idx, 'personal_links'),
                  )}
                </div>
              )}
            </div>
          </BentoSpotCard>
        </div>

        <div className="bento-col">
          {renderMediaCard(1)}
          {renderAudioMixerCard(2, {
            title: 'Звук и микшер',
            showDeviceSwitcher: true,
            showMicToggle: false,
            showSessions: true,
          })}
          {renderClipboardCard(3)}
        </div>
      </div>
      {dragGhost}
      </>
    );
  }

  /* ===================== SPACE 2: РАБОТА ===================== */
  if (space === 'work') {
    const workFolders = config?.workFolders ?? [];
    const workApps = config?.workApps ?? [];

    return (
      <>
      <div key={space} className={panelClass} style={panelStyle}>
        <div className="bento-col">
          <BentoSpotCard bentoIndex={0} className="space-card-flex">
            <div className="sec-row">
              <span className="sec-title">Проекты, папки и файлы</span>
              <button type="button" className="sec-act sec-act-ic" onClick={onOpenDrawer}>
                <PlusIcon className="ic12" />
                <span>Закрепить</span>
              </button>
            </div>

            <div className="space-scroll-main">
              {workFolders.length === 0 ? (
                renderEmptyStrip('Нет закреплённых папок и проектов')
              ) : (
                <div className="folder-hub-grid">
                  {workFolders.map((f, idx) => {
                    const honestAlt = getHonestShiftBadge(f);
                    return (
                      <div
                        key={f.id || `wf-${idx}`}
                        role="button"
                        tabIndex={0}
                        className={`folder-card${getDragClasses('work_folders', idx)}`}
                        {...makeDragProps('work_folders', idx, f.title)}
                        onClick={() => handlePinTileClick(f)}
                        onKeyDown={(e) => {
                          if (e.target === e.currentTarget && isActivateKey(e)) {
                            e.preventDefault();
                            onLaunchPin(f);
                          }
                        }}
                        onContextMenu={(e) => onOpenTileContext(e, f, 'work_folders')}
                      >
                        {editMode && (
                          <button
                            type="button"
                            className="tile-del"
                            title="Открепить"
                            aria-label={`Открепить ${f.title}`}
                            onClick={(e) => {
                              e.stopPropagation();
                              ipc('remove_pin', { space: 'work_folders', itemId: f.id }).catch(() => {
                                onShowToast('Не удалось открепить папку');
                              });
                            }}
                          >
                            <XIcon10 />
                          </button>
                        )}
                        <div className="folder-ic-box">
                          <FolderSvg className="ic18" />
                        </div>
                        <div className="tile-meta">
                          <div className="tile-name">{f.title}</div>
                          <div className={`tile-sub ${honestAlt ? 'has-alt' : ''}`}>
                            {shiftHeld && honestAlt ? honestAlt : f.subtitle}
                          </div>
                        </div>
                        {!editMode && (
                          <div className="folder-acts">
                            <button
                              type="button"
                              className="f-ic-btn"
                              title="Открыть папку"
                              aria-label={`Открыть ${f.title}`}
                              onClick={(e) => {
                                e.stopPropagation();
                                ipc('launch_item', {
                                  target: f.target,
                                  asAdmin: false,
                                  closeStart: true,
                                }).catch(() => onShowToast('Не удалось открыть папку'));
                              }}
                            >
                              <FolderSvg className="ic14" />
                            </button>
                            <button
                              type="button"
                              className="f-ic-btn"
                              title="Открыть в терминале"
                              aria-label={`Открыть ${f.title} в терминале`}
                              onClick={(e) => {
                                e.stopPropagation();
                                ipc('open_terminal_at', { path: f.target }).catch(() => {
                                  onShowToast('Не удалось открыть терминал');
                                });
                              }}
                            >
                              <TerminalIcon className="ic14" />
                            </button>
                            <button
                              type="button"
                              className="f-ic-btn"
                              title="Скопировать путь"
                              aria-label={`Скопировать путь ${f.title}`}
                              onClick={(e) => {
                                e.stopPropagation();
                                ipc('copy_to_clipboard', { text: f.target })
                                  .then(() => onShowToast('Путь скопирован'))
                                  .catch(() => onShowToast('Не удалось скопировать путь'));
                              }}
                            >
                              <CopyIcon className="ic14" />
                            </button>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            <div className="space-dock-foot">
              <div className="sec-row" style={{ marginBottom: 8 }}>
                <span className="sec-title">Рабочие программы</span>
                <button type="button" className="sec-act sec-act-ic" onClick={onOpenDrawer}>
                  <PlusIcon className="ic12" />
                  <span>Добавить</span>
                </button>
              </div>
              {workApps.length === 0 ? (
                renderEmptyStrip('Нет закреплённых рабочих программ')
              ) : (
                <div className="qlinks-grid qlinks-scroll">
                  {workApps.map((a, idx) => renderQuickLink(a, idx, 'work_apps'))}
                </div>
              )}
            </div>
          </BentoSpotCard>
        </div>

        <div className="bento-col">
          {renderTelemetryCard(1, 'full')}
          {renderProcessConsumersCard(2, 'ram', 'work')}
        </div>
      </div>
      {dragGhost}
      </>
    );
  }

  /* ===================== SPACE 3: СТУДИЯ ===================== */
  if (space === 'studio') {
    const hasStudioPins = (config?.studioPins.length ?? 0) > 0;
    const studioTools = config?.studioTools ?? [];

    return (
      <>
      <div key={space} className={panelClass} style={panelStyle}>
        <div className="bento-col">
          <BentoSpotCard bentoIndex={0} className="space-card-flex">
            <div className="sec-row">
              <span className="sec-title">Творчество и AI</span>
              <button type="button" className="sec-act sec-act-ic" onClick={onOpenDrawer}>
                <PlusIcon className="ic12" />
                <span>Закрепить</span>
              </button>
            </div>

            <div className="space-scroll-main">
              {!hasStudioPins ? (
                renderEmptyStrip('Нет программ студии')
              ) : (
                <>
                  {studioPartition.heroes.length > 0 && (
                    <div className="hero-pair">
                      {studioPartition.heroes.map((p, i) => renderTile(p, i, 'studio', true))}
                    </div>
                  )}
                  {studioPartition.compact.length > 0 && (
                    <div className="sub-grid">
                      {studioPartition.compact.map((p, i) => renderTile(p, i + 2, 'studio', false))}
                    </div>
                  )}
                </>
              )}
            </div>

            <div className="space-dock-foot">
              <div className="sec-row" style={{ marginBottom: 8 }}>
                <span className="sec-title">Инструменты</span>
                <button type="button" className="sec-act sec-act-ic" onClick={onOpenDrawer}>
                  <PlusIcon className="ic12" />
                  <span>Добавить</span>
                </button>
              </div>
              {studioTools.length === 0 ? (
                renderEmptyStrip('Нет закреплённых инструментов')
              ) : (
                <div className="qlinks-grid qlinks-scroll">
                  {studioTools.map((t, idx) => renderQuickLink(t, idx, 'studio_tools'))}
                </div>
              )}
            </div>
          </BentoSpotCard>
        </div>

        <div className="bento-col">
          {renderTelemetryCard(1, 'headroom')}
          {renderAudioMixerCard(2, {
            title: 'Аудио и микрофон',
            showDeviceSwitcher: true,
            showMicToggle: true,
            showSessions: false,
          })}
          {renderProcessConsumersCard(3, 'gpu', 'studio')}
        </div>
      </div>
      {dragGhost}
      </>
    );
  }

  /* ===================== SPACE 4: ИГРЫ ===================== */
  if (space === 'games') {
    const launchers = config?.gameLaunchers ?? [];
    const combos = config?.gameCombos ?? [];

    return (
      <>
      <div key={space} className={panelClass} style={panelStyle}>
        <div className="bento-col">
          <BentoSpotCard bentoIndex={0} className="space-card-flex">
            <div className="sec-row" style={{ marginBottom: 9 }}>
              <div className="games-head-left">
                <span className="sec-title">Игры</span>
                <div className="mini-tabs">
                  <button
                    type="button"
                    className={`mini-tab ${gameFilter === 'recent' ? 'active' : ''}`}
                    onClick={() => setGameFilter('recent')}
                  >
                    Недавние ({Math.min(4, games.length)})
                  </button>
                  <button
                    type="button"
                    className={`mini-tab ${gameFilter === 'all' ? 'active' : ''}`}
                    onClick={() => setGameFilter('all')}
                  >
                    Все игры ({games.length})
                  </button>
                </div>
              </div>
              <button type="button" className="sec-act sec-act-ic" onClick={onOpenDrawer}>
                <PlusIcon className="ic12" />
                <span>Добавить</span>
              </button>
            </div>

            <div className="games-scroll">
              {displayedGames.length === 0 ? (
                <div className="games-empty">
                  Библиотека игр сканируется или игры не найдены
                </div>
              ) : (
                <div className="games-grid">
                  {displayedGames.map((g, idx) => {
                    const coverSrc = g.coverData || (g.coverPath ? loadedCovers[g.coverPath] : null) || null;
                    const launchGame = () => {
                      ipc('launch_item', {
                        target: g.target,
                        asAdmin: false,
                        closeStart: true,
                      }).catch(() => onShowToast(`Не удалось запустить ${g.name}`));
                    };

                    return (
                      <div
                        key={g.appId || `game-${g.name}-${idx}`}
                        role="button"
                        tabIndex={0}
                        className="game-card"
                        onPointerEnter={(e) => {
                          if (!reducedMotion) e.currentTarget.classList.add('tilting');
                        }}
                        onPointerMove={(e) => {
                          if (reducedMotion) return;
                          const rect = e.currentTarget.getBoundingClientRect();
                          const tilt = computeGameCardTilt(e.clientX, e.clientY, rect, false);
                          e.currentTarget.style.setProperty('--gx', `${tilt.gxPct}%`);
                          e.currentTarget.style.setProperty('--gy', `${tilt.gyPct}%`);
                          e.currentTarget.style.setProperty('--rx', `${tilt.rotateXDeg}deg`);
                          e.currentTarget.style.setProperty('--ry', `${tilt.rotateYDeg}deg`);
                        }}
                        onPointerLeave={(e) => {
                          e.currentTarget.classList.remove('tilting');
                          e.currentTarget.style.setProperty('--rx', '0deg');
                          e.currentTarget.style.setProperty('--ry', '0deg');
                        }}
                        onClick={launchGame}
                        onKeyDown={(e) => {
                          if (e.target === e.currentTarget && isActivateKey(e)) {
                            e.preventDefault();
                            launchGame();
                          }
                        }}
                      >
                        <div
                          className="game-bg"
                          style={{
                            backgroundImage: coverSrc
                              ? `url("${coverSrc}")`
                              : 'linear-gradient(135deg, #1E2434, #0F121A)',
                          }}
                        />
                        <div className="game-grad" />
                        <div className="game-sheen" />
                        <span className="game-play">
                          <PlayTriangleIcon size={10} />
                          <span>Играть</span>
                        </span>
                        <div className="game-info">
                          <div className="game-title">{g.name}</div>
                          <div className="game-meta">{g.launcher}</div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            <div className="space-dock-foot">
              <div className="sec-row" style={{ marginBottom: 7 }}>
                <span className="sec-title">Лаунчеры и компаньоны</span>
              </div>
              {launchers.length === 0 ? (
                renderEmptyStrip('Нет закреплённых лаунчеров')
              ) : (
                <div className="qlinks-grid qlinks-scroll">
                  {launchers.map((l, idx) => renderQuickLink(l, idx, 'game_launchers'))}
                </div>
              )}
            </div>
          </BentoSpotCard>
        </div>

        <div className="bento-col">
          {renderAudioMixerCard(1, {
            title: 'Игровой звук и голос',
            showDeviceSwitcher: false,
            showMicToggle: true,
            showSessions: true,
          })}

          <BentoSpotCard bentoIndex={2} className="combos-card">
            <div className="sec-row" style={{ marginBottom: 8 }}>
              <span className="sec-title">Игровые связки</span>
              <button
                type="button"
                className="sec-act sec-act-ic"
                onClick={() => {
                  setComboError(null);
                  setComboBuilderOpen(true);
                }}
              >
                <PlusIcon className="ic12" />
                <span>Связка</span>
              </button>
            </div>

            <div className="combos-list">
              {combos.length === 0 ? (
                <div className="space-empty-inline">
                  <span>Нет игровых связок</span>
                  <button
                    type="button"
                    className="pin-any-btn"
                    onClick={() => {
                      setComboError(null);
                      setComboBuilderOpen(true);
                    }}
                  >
                    <PlusIcon className="ic12" />
                    <span>Создать</span>
                  </button>
                </div>
              ) : (
                combos.map((cb, idx) => {
                  const isDelArmed = armedComboDeleteId === cb.id;
                  return (
                    <div
                      key={cb.id || `combo-${idx}`}
                      role="button"
                      tabIndex={0}
                      className={`consumer-row combo-row${getDragClasses('game_combos', idx)}`}
                      {...makeDragProps('game_combos', idx, cb.title)}
                      onClick={() => handlePinTileClick(cb)}
                      onKeyDown={(e) => {
                        if (e.target === e.currentTarget && isActivateKey(e)) {
                          e.preventDefault();
                          onLaunchPin(cb);
                        }
                      }}
                      onContextMenu={(e) => onOpenTileContext(e, cb, 'game_combos')}
                    >
                      <span
                        className="combo-ic"
                        style={{
                          background: `rgba(${cb.accentRgb || '242,110,132'}, 0.16)`,
                          color: `rgb(${cb.accentRgb || '242,110,132'})`,
                        }}
                      >
                        <PlayTriangleIcon size={12} />
                      </span>
                      <div className="consumer-meta">
                        <div className="consumer-name">{cb.title}</div>
                        <div className="consumer-sub">{cb.subtitle}</div>
                      </div>
                      {editMode && (
                        <button
                          type="button"
                          className={`consumer-kill ${isDelArmed ? 'armed' : ''}`}
                          style={{ opacity: 1 }}
                          title={isDelArmed ? 'Подтвердить удаление связки' : 'Удалить связку'}
                          aria-label={isDelArmed ? `Подтвердить удаление ${cb.title}` : `Удалить ${cb.title}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            handleDeleteComboClick(cb);
                          }}
                        >
                          {isDelArmed ? <span>Подтвердить</span> : <XIcon10 />}
                        </button>
                      )}
                    </div>
                  );
                })
              )}
            </div>

            {telem?.gpuAvailable && (
              <div className="health-row" style={{ marginTop: 'auto', flexShrink: 0 }}>
                <div className="health-top" style={{ marginBottom: 5 }}>
                  <span className="health-lbl">
                    {telem.gpuName} <span className="health-sub">· {telem.gpuTempC}°C</span>
                  </span>
                  <span className="health-val">
                    {formatMemoryRatio(telem.vramUsedGb, telem.vramTotalGb)}{' '}
                    <span className={`status-pill ${getGpuTempStatusPill(telem.gpuTempC).cls}`}>
                      {getGpuTempStatusPill(telem.gpuTempC).label}
                    </span>
                  </span>
                </div>
                <div className="health-bar-row">
                  <div className="health-bar">
                    <div
                      className={`health-fill ${getTelemetryLevelClass(telem.vramPct)}`}
                      style={{ width: '100%', ['--pct' as string]: Math.min(1, telem.vramPct / 100) } as React.CSSProperties}
                    />
                  </div>
                  {renderSparklineSvg(telemHistory.vram, telem.vramPct)}
                </div>
              </div>
            )}

            {/* Inline Game Combo Builder Sheet */}
            {comboBuilderPresence.mounted && (
              <div
                className="combo-builder-sheet"
                data-phase={comboBuilderPresence.phase}
                role="dialog"
                aria-label="Новая игровая связка"
                onClick={(e) => e.stopPropagation()}
              >
                <div className="space-create-head">
                  <span>Новая связка</span>
                  <button
                    type="button"
                    className="tray-ctx-close"
                    aria-label="Закрыть конструктор связки"
                    onClick={() => setComboBuilderOpen(false)}
                  >
                    <CloseWindowIcon className="ic12" />
                  </button>
                </div>

                <input
                  className="space-create-input"
                  placeholder="Название (например, PoE 2 + Discord)"
                  maxLength={36}
                  value={comboName}
                  onChange={(e) => {
                    setComboName(e.target.value);
                    if (comboError) setComboError(null);
                  }}
                />

                {comboPickerOptions.length > 0 && (
                  <div className="combo-picker-row">
                    <select
                      className="qs-input-select"
                      style={{ height: 28 }}
                      aria-label="Выбрать программу или игру"
                      value={comboPickKey}
                      onChange={(e) => setComboPickKey(e.target.value)}
                    >
                      <option value="">Выбрать игру или программу…</option>
                      {comboPickerOptions.map((opt) => (
                        <option key={opt.key} value={opt.key}>
                          {opt.label}
                        </option>
                      ))}
                    </select>
                    <button
                      type="button"
                      className="pin-any-btn"
                      style={{ height: 28, padding: '0 10px' }}
                      onClick={() => {
                        const found = comboPickerOptions.find((o) => o.key === comboPickKey);
                        if (!found) return;
                        if (comboSteps.some((s) => s.target.toLowerCase() === found.target.toLowerCase())) {
                          return;
                        }
                        setComboSteps((prev) => [
                          ...prev,
                          { id: `${Date.now()}-${prev.length}`, label: found.label, target: found.target },
                        ]);
                        setComboPickKey('');
                        if (comboError) setComboError(null);
                      }}
                    >
                      <PlusIcon className="ic12" />
                    </button>
                  </div>
                )}

                <div className="combo-picker-row">
                  <input
                    className="url-input"
                    style={{ flex: 1, height: 28 }}
                    placeholder="Путь к .exe или URL (https://…)"
                    value={comboCustomTarget}
                    onChange={(e) => setComboCustomTarget(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        const val = comboCustomTarget.trim();
                        if (!val) return;
                        const label = val.split(/[\\/]/).filter(Boolean).pop() || val;
                        setComboSteps((prev) => [
                          ...prev,
                          { id: `${Date.now()}-${prev.length}`, label, target: val },
                        ]);
                        setComboCustomTarget('');
                        if (comboError) setComboError(null);
                      }
                    }}
                  />
                  <button
                    type="button"
                    className="pin-any-btn"
                    style={{ height: 28, padding: '0 10px' }}
                    onClick={() => {
                      const val = comboCustomTarget.trim();
                      if (!val) return;
                      const label = val.split(/[\\/]/).filter(Boolean).pop() || val;
                      setComboSteps((prev) => [
                        ...prev,
                        { id: `${Date.now()}-${prev.length}`, label, target: val },
                      ]);
                      setComboCustomTarget('');
                      if (comboError) setComboError(null);
                    }}
                  >
                    Добавить
                  </button>
                </div>

                <div className="combo-step-list">
                  {comboSteps.length === 0 ? (
                    <div className="clip-empty">Добавьте минимум 2 программы, игры или ссылки</div>
                  ) : (
                    comboSteps.map((step, idx) => (
                      <div key={step.id || `step-${idx}`} className="combo-step-chip">
                        <span className="clip-txt" title={step.target}>{step.label}</span>
                        <button
                          type="button"
                          className="tray-ctx-close"
                          aria-label={`Удалить шаг ${step.label}`}
                          onClick={() => setComboSteps((prev) => prev.filter((x) => x.id !== step.id))}
                        >
                          <XIcon10 />
                        </button>
                      </div>
                    ))
                  )}
                </div>

                {comboError && <div className="space-create-err">{comboError}</div>}

                <div className="space-create-actions">
                  <button
                    type="button"
                    className="space-create-cancel"
                    onClick={() => setComboBuilderOpen(false)}
                  >
                    Отмена
                  </button>
                  <button
                    type="button"
                    className="space-create-submit"
                    disabled={comboSaving}
                    onClick={() => void handleSaveCombo()}
                  >
                    Сохранить
                  </button>
                </div>
              </div>
            )}
          </BentoSpotCard>
        </div>
      </div>
      {dragGhost}
      </>
    );
  }

  /* ===================== CUSTOM SPACES (4 Widget Presets) ===================== */
  if (activeCustomSpace) {
    const isFullGrid = customPreset === 'full_grid';

    const pinsCard = (
      <BentoSpotCard bentoIndex={0} className="space-card-flex">
        <div className="sec-row">
          <span className="sec-title">{activeCustomSpace.name}</span>
          <button type="button" className="sec-act sec-act-ic" onClick={onOpenDrawer}>
            <PlusIcon className="ic12" />
            <span>Закрепить</span>
          </button>
        </div>
        {activeCustomSpace.items.length === 0 ? (
          <div className="custom-empty">
            <div className="custom-empty-title">Пока пусто</div>
            <div className="custom-empty-sub">
              Закрепите сюда программы, папки, файлы или ссылки.
            </div>
            <button type="button" className="pin-any-btn primary" onClick={onOpenDrawer}>
              <PlusIcon className="ic12" />
              <span>Закрепить элементы</span>
            </button>
          </div>
        ) : (
          <div className="space-scroll-main">
            <div className={isFullGrid ? 'sub-grid custom-grid-4' : 'sub-grid'}>
              {activeCustomSpace.items.map((it, i) =>
                renderTile(it, i, activeCustomSpace.id, false),
              )}
            </div>
          </div>
        )}
      </BentoSpotCard>
    );

    if (isFullGrid) {
      return (
        <>
          <div key={space} className={`${panelClass} space-panel-single`} style={panelStyle}>
            {pinsCard}
          </div>
          {dragGhost}
        </>
      );
    }

    return (
      <>
        <div key={space} className={panelClass} style={panelStyle}>
          <div className="bento-col">{pinsCard}</div>
          <div className="bento-col">
            {customPreset === 'media_clipboard' && (
              <>
                {renderMediaCard(1)}
                {renderClipboardCard(2)}
              </>
            )}
            {customPreset === 'telemetry_processes' && (
              <>
                {renderTelemetryCard(1, 'full')}
                {renderProcessConsumersCard(2, 'ram', activeCustomSpace.id)}
              </>
            )}
            {customPreset === 'audio_mic' &&
              renderAudioMixerCard(1, {
                title: 'Звук и микрофон',
                showDeviceSwitcher: true,
                showMicToggle: true,
                showSessions: true,
              })}
          </div>
        </div>
        {dragGhost}
      </>
    );
  }

  return null;
};
