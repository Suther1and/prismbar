import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { canOpenFileLocation, canRunAsAdmin } from '../lib/flyoutLogic';
import {
  applyConfigUpdate,
  fetchIconsBatch,
  iconCache,
  ipc,
  isCacheIconKey,
  onEvent,
  type AppWindowGroup,
  type AudioState,
  type ClipboardEntry,
  type IndexedApp,
  type IndexedFile,
  type MediaState,
  type PeripheralBattery,
  type PinItem,
  type SecondaryTaskbarMode,
  type SteamGame,
  type TelemetryState,
  type TestBarConfig,
} from '../lib/ipc';
import {
  buildSearchCorpus,
  isPrintableSearchTrigger,
  rankSearchResults,
  resolveSearchLaunchPayload,
  type SearchCorpusItem,
} from '../lib/searchLogic';
import {
  INITIAL_START_ANIM_STATE,
  reducePowerClick,
  reduceStartState,
  resolvePinAction,
  START_EXIT_ANIM_MS,
  type ArmedPowerKind,
  type PowerActionKind,
  type SpaceAccentOption,
  type StartAnimState,
} from '../lib/startLogic';
import { KillIcon } from './Icons';
import { StartCommandPalette } from './start/StartCommandPalette';
import { StartHeader } from './start/StartHeader';
import { StartPinDrawer } from './start/StartPinDrawer';
import { StartSettingsOverlay } from './start/StartSettingsOverlay';
import { StartSpaces } from './start/StartSpaces';

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

  const [anim, setAnim] = useState<StartAnimState>(INITIAL_START_ANIM_STATE);
  const [space, setSpace] = useState<string>('personal');
  const [editMode, setEditMode] = useState(false);
  const [shiftHeld, setShiftHeld] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [periphOpen, setPeriphOpen] = useState(false);
  const [powerOpen, setPowerOpen] = useState(false);
  const [armedPower, setArmedPower] = useState<ArmedPowerKind>(null);
  const [addingSpace, setAddingSpace] = useState(false);

  const [toastMsg, setToastMsg] = useState<string | null>(null);
  const [tileCtx, setTileCtx] = useState<{ x: number; y: number; item: PinItem; spaceKey: string } | null>(null);

  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [cmdIdx, setCmdIdx] = useState(0);
  const cmdInputRef = useRef<HTMLInputElement>(null);
  const closeCleanupTimerRef = useRef<number | null>(null);
  const toastTimerRef = useRef<number | null>(null);
  const resolvedTargetsRef = useRef<Set<string>>(new Set());

  const showToast = useCallback((msg: string) => {
    setToastMsg(msg);
    if (toastTimerRef.current !== null) {
      window.clearTimeout(toastTimerRef.current);
    }
    toastTimerRef.current = window.setTimeout(() => {
      toastTimerRef.current = null;
      setToastMsg(null);
    }, 1400);
  }, []);

  const hydrateLiveData = useCallback(() => {
    ipc<TestBarConfig>('get_config')
      .then((next) => setConfig((prev) => applyConfigUpdate(prev, next)))
      .catch(() => {});
    ipc<AppWindowGroup[]>('get_open_windows').then(setGroups).catch(() => {});
    ipc<AudioState>('get_audio_state').then(setAudio).catch(() => {});
    ipc<ClipboardEntry[]>('get_clipboard_history').then(setClips).catch(() => {});
    ipc<MediaState>('get_media_state').then(setMedia).catch(() => {});
    ipc<TelemetryState>('get_telemetry').then(setTelem).catch(() => {});
    ipc<PeripheralBattery[]>('get_peripherals').then(setPeriphs).catch(() => {});
  }, []);

  useEffect(() => {
    hydrateLiveData();
    ipc<IndexedApp[]>('get_installed_apps').then(setApps).catch(() => {});
    ipc<IndexedFile[]>('get_indexed_files').then(setFiles).catch(() => {});
    ipc<SteamGame[]>('get_steam_games').then(setGames).catch(() => {});

    const subs = [
      onEvent<TestBarConfig>('config-updated', (next) =>
        setConfig((prev) => applyConfigUpdate(prev, next)),
      ),
      onEvent<AppWindowGroup[]>('windows-updated', setGroups),
      onEvent<IndexedApp[]>('apps-indexed', setApps),
      onEvent<IndexedFile[]>('files-indexed', setFiles),
      onEvent<SteamGame[]>('games-indexed', setGames),
      onEvent<AudioState>('audio-mixer-updated', setAudio),
      onEvent<MediaState>('media-updated', setMedia),
      onEvent<TelemetryState>('telemetry-updated', setTelem),
      onEvent<ClipboardEntry[]>('clipboard-updated', setClips),
      onEvent<PeripheralBattery[]>('peripherals-updated', setPeriphs),
      onEvent<boolean>('start-state', (vis) => {
        if (closeCleanupTimerRef.current !== null) {
          window.clearTimeout(closeCleanupTimerRef.current);
          closeCleanupTimerRef.current = null;
        }
        setAnim((prev) => reduceStartState(prev, vis));
        if (vis) {
          setShiftHeld(false);
          hydrateLiveData();
        } else {
          closeCleanupTimerRef.current = window.setTimeout(() => {
            closeCleanupTimerRef.current = null;
            setSearchOpen(false);
            setQuery('');
            setPowerOpen(false);
            setArmedPower(null);
            setPeriphOpen(false);
            setAddingSpace(false);
            setSettingsOpen(false);
            setDrawerOpen(false);
            setTileCtx(null);
            setShiftHeld(false);
          }, START_EXIT_ANIM_MS);
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
        setTileCtx(null);
        setSettingsOpen(true);
      }),
    ];

    return () => {
      if (closeCleanupTimerRef.current !== null) {
        window.clearTimeout(closeCleanupTimerRef.current);
      }
      if (toastTimerRef.current !== null) {
        window.clearTimeout(toastTimerRef.current);
      }
      subs.forEach((p) => p.then((u) => u()));
    };
  }, [hydrateLiveData]);

  // Unified IconCache batch fetch for apps, window groups & cache-keyed pins (AD-6)
  useEffect(() => {
    const keysToFetch: string[] = [];
    const seen = new Set<string>();
    const consider = (key: string | null | undefined) => {
      if (!isCacheIconKey(key) || seen.has(key)) return;
      seen.add(key);
      if (iconCache.has(key)) return;
      keysToFetch.push(key);
    };

    groups.forEach((g) => consider(g.iconKey));
    apps.forEach((a) => consider(a.iconKey));
    if (config) {
      [
        ...config.personalPins,
        ...config.workApps,
        ...config.studioPins,
        ...config.studioTools,
        ...config.gameLaunchers,
        ...config.customSpaces.flatMap((s) => s.items),
      ].forEach((p) => consider(p.iconKey));
    }

    if (keysToFetch.length === 0) return;

    let cancelled = false;
    fetchIconsBatch(keysToFetch)
      .then((batch) => {
        if (!cancelled && Object.keys(batch).length > 0) {
          setIconMap((prev) => ({ ...prev, ...batch }));
        }
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [apps, groups, config]);

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
        const resolvable = p.target.includes(':\\') || p.target.startsWith('\\\\') || low.startsWith('shell:appsfolder\\');
        if (resolvable && !resolvedTargetsRef.current.has(p.target)) {
          resolvedTargetsRef.current.add(p.target);
          targetsToResolve.push(p.target);
        }
      }
    }

    if (targetsToResolve.length === 0) return;

    let cancelled = false;
    ipc<Record<string, string>>('resolve_pin_icons', { targets: targetsToResolve })
      .then((batch) => {
        if (!cancelled && batch && Object.keys(batch).length > 0) {
          setIconMap((prev) => ({ ...prev, ...batch }));
        }
      })
      .catch(() => {});

    return () => {
      cancelled = true;
    };
  }, [config]);

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
    () => buildSearchCorpus({ config, apps, games, files, iconMap }),
    [config, apps, games, files, iconMap],
  );

  const searchResults = useMemo(
    () => rankSearchResults(searchCorpus, query, 10),
    [searchCorpus, query],
  );

  const dispatchSearchLaunch = useCallback((item: SearchCorpusItem, ctrlKey: boolean) => {
    const payload = resolveSearchLaunchPayload(item, ctrlKey);
    ipc(payload.command, payload.args).catch(() => {});
    setSearchOpen(false);
    setQuery('');
  }, []);

  // Global keyboard handler: zero-loss type-anywhere search + Tab space switch + Esc + Shift
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Shift') setShiftHeld(true);

      const activeEl = document.activeElement as HTMLElement | null;
      const activeTag = activeEl?.tagName;
      const typingInField =
        (activeTag === 'INPUT' || activeTag === 'TEXTAREA') && activeEl !== cmdInputRef.current;
      if (typingInField) return;

      if (e.key === 'Escape') {
        if (searchOpen) {
          setSearchOpen(false);
          setQuery('');
        } else if (addingSpace) {
          setAddingSpace(false);
        } else if (drawerOpen) {
          setDrawerOpen(false);
        } else if (settingsOpen) {
          setSettingsOpen(false);
        } else if (periphOpen) {
          setPeriphOpen(false);
        } else if (tileCtx) {
          setTileCtx(null);
        } else if (powerOpen) {
          setPowerOpen(false);
          setArmedPower(null);
        } else {
          ipc('toggle_start', { force: false }).catch(() => {});
        }
        return;
      }

      if (e.key === 'Tab' && !searchOpen && !drawerOpen && !settingsOpen && !addingSpace) {
        e.preventDefault();
        const allSpaces = [
          'personal',
          'work',
          'studio',
          'games',
          ...(config?.customSpaces.map((s) => s.id) ?? []),
        ];
        const idx = allSpaces.indexOf(space);
        const next = allSpaces[(idx + (e.shiftKey ? allSpaces.length - 1 : 1)) % allSpaces.length];
        setSpace(next);
        return;
      }

      if (searchOpen) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          setCmdIdx((i) => Math.min(Math.max(0, searchResults.items.length - 1), i + 1));
        } else if (e.key === 'ArrowUp') {
          e.preventDefault();
          setCmdIdx((i) => Math.max(0, i - 1));
        } else if (e.key === 'Enter' && searchResults.items[cmdIdx]) {
          e.preventDefault();
          dispatchSearchLaunch(searchResults.items[cmdIdx], e.ctrlKey);
        }
        return;
      }

      if (!drawerOpen && !settingsOpen && !addingSpace && isPrintableSearchTrigger(e)) {
        e.preventDefault();
        setSearchOpen(true);
        setQuery(e.key);
        setCmdIdx(0);
        requestAnimationFrame(() => {
          cmdInputRef.current?.focus();
          const len = cmdInputRef.current?.value.length ?? 0;
          cmdInputRef.current?.setSelectionRange(len, len);
        });
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
    searchOpen,
    addingSpace,
    drawerOpen,
    settingsOpen,
    periphOpen,
    tileCtx,
    powerOpen,
    space,
    config,
    searchResults.items,
    cmdIdx,
    dispatchSearchLaunch,
  ]);

  const launchPin = useCallback(
    (item: PinItem) => {
      if (editMode) return;
      const action = resolvePinAction(item, shiftHeld);
      if (action.type === 'combo') {
        ipc('launch_combo', { targets: action.targets }).catch(() => {});
      } else if (action.type === 'terminal') {
        ipc('open_terminal_at', { path: action.path }).catch(() => {});
      } else if (action.type === 'copy_path') {
        ipc('copy_to_clipboard', { text: action.text })
          .then(() => showToast(action.toast))
          .catch(() => {});
      } else {
        ipc('launch_item', {
          target: action.target,
          args: action.args,
          asAdmin: action.asAdmin,
          closeStart: action.closeStart,
        }).catch(() => {});
      }
    },
    [editMode, shiftHeld, showToast],
  );

  const handlePinFolder = async () => {
    const picked = await ipc<string | null>('pick_folder').catch(() => null);
    if (!picked) return;
    const baseName = picked.split('\\').filter(Boolean).pop() || picked;
    const targetSpace = space === 'personal' ? 'personal_links' : space === 'work' ? 'work_folders' : space;
    await ipc('add_pin', {
      space: targetSpace,
      item: {
        id: `pin-${Date.now()}`,
        kind: 'folder',
        title: baseName,
        subtitle: 'Папка',
        target: picked,
        altAction: targetSpace === 'personal_links' ? 'Скопировать путь' : 'Открыть в терминале',
        accentRgb: '245,158,11',
        iconKey: 'folder',
        iconData: null,
      },
    }).catch(() => {});
    showToast(`Закреплено: ${baseName}`);
  };

  const handlePinFile = async () => {
    const picked = await ipc<string | null>('pick_file').catch(() => null);
    if (!picked) return;
    const baseName = picked.split('\\').filter(Boolean).pop() || picked;
    const isExe = picked.toLowerCase().endsWith('.exe');
    const targetSpace = space === 'personal' ? (isExe ? 'personal' : 'personal_links') : space;
    const title = isExe ? baseName.replace(/\.exe$/i, '') : baseName;
    await ipc('add_pin', {
      space: targetSpace,
      item: {
        id: `pin-${Date.now()}`,
        kind: isExe ? 'app' : 'file',
        title,
        subtitle: isExe ? 'Программа' : 'Файл',
        target: picked,
        altAction: isExe ? 'От админа' : 'Скопировать путь',
        accentRgb: '56,189,248',
        iconKey: isExe ? baseName.toLowerCase() : 'file',
        iconData: null,
      },
    }).catch(() => {});
    showToast(`Закреплено: ${title}`);
  };

  const handlePinUrl = async (url: string, title: string) => {
    const targetSpace = space === 'personal' ? 'personal_links' : space;
    await ipc('add_pin', {
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
    }).catch(() => {});
    showToast(`Закреплено: ${title}`);
  };

  const handleTogglePinApp = async (app: IndexedApp, existingPin: PinItem | null) => {
    if (existingPin) {
      await ipc('remove_pin', { space, itemId: existingPin.id }).catch(() => {});
      return;
    }
    const isUwp = Boolean(app.aumid) || app.target.toLowerCase().startsWith('shell:appsfolder\\');
    await ipc('add_pin', {
      space,
      item: {
        id: `app-${Date.now()}`,
        kind: 'app',
        title: app.name,
        subtitle: app.category,
        target: app.target,
        altAction: isUwp ? 'Открыть' : 'От админа',
        accentRgb: '129,140,248',
        iconKey: app.iconKey || app.name.toLowerCase(),
        iconData: app.iconData ?? null,
        matchAumid: app.aumid ?? null,
      },
    }).catch(() => {});
  };

  const handleCreateCustomSpace = async (name: string, accent: SpaceAccentOption) => {
    const updated = await ipc<TestBarConfig>('add_custom_space', {
      name,
      accentHex: accent.hex,
      accentRgb: accent.rgb,
    });
    setConfig((prev) => applyConfigUpdate(prev, updated));
    const newest = updated.customSpaces[updated.customSpaces.length - 1];
    if (newest) {
      setSpace(newest.id);
      setDrawerOpen(false);
      setSettingsOpen(false);
    }
  };

  const handlePowerClick = (kind: PowerActionKind) => {
    const res = reducePowerClick(armedPower, kind);
    setArmedPower(res.nextArmed);
    if (res.closePower) setPowerOpen(false);
    if (res.execute) {
      ipc('power_action', { action: res.execute }).catch(() => {});
    }
  };

  const openTileContextMenu = (e: React.MouseEvent, item: PinItem, spaceKey: string) => {
    e.preventDefault();
    e.stopPropagation();
    setTileCtx({
      x: Math.min(e.clientX, window.innerWidth - 256),
      y: Math.min(e.clientY, window.innerHeight - 240),
      item,
      spaceKey,
    });
  };

  const activeCustomSpace = config?.customSpaces.find((s) => s.id === space) ?? null;
  const spaceTheme = ['personal', 'work', 'studio', 'games'].includes(space) ? space : 'custom';
  const customSpaceStyle: React.CSSProperties | undefined = activeCustomSpace
    ? ({
        ['--accent' as string]: activeCustomSpace.accentHex || '#A78BFA',
        ['--accent-rgb' as string]: activeCustomSpace.accentRgb || '167, 139, 250',
      } as React.CSSProperties)
    : undefined;

  return (
    <div
      className={`start-root ${editMode ? 'edit-mode' : ''} ${shiftHeld ? 'shift-active' : ''}`}
      data-space={spaceTheme}
      style={customSpaceStyle}
    >
      <section
        key={anim.openSeq}
        className={`start-chassis ${anim.phase === 'closing' ? 'closing' : ''}`}
      >
        <div className="chassis-aura" />

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
          onSelectSpace={(id) => {
            setSpace(id);
            setDrawerOpen(false);
            setSettingsOpen(false);
            setAddingSpace(false);
          }}
          onTogglePeriph={() => setPeriphOpen((v) => !v)}
          onClosePeriph={() => setPeriphOpen(false)}
          onSelectAudioDevice={(deviceId) => {
            ipc<AudioState>('set_audio_device', { deviceId }).then(setAudio).catch(() => {});
          }}
          onOpenSearch={() => {
            setSearchOpen(true);
            setDrawerOpen(false);
            setSettingsOpen(false);
            setAddingSpace(false);
            setQuery('');
            setCmdIdx(0);
            requestAnimationFrame(() => cmdInputRef.current?.focus());
          }}
          onToggleEditMode={() => setEditMode((v) => !v)}
          onToggleDrawer={() => {
            setDrawerOpen((v) => !v);
            setSettingsOpen(false);
            setSearchOpen(false);
            setAddingSpace(false);
          }}
          onToggleSettings={() => {
            setSettingsOpen((v) => !v);
            setDrawerOpen(false);
            setSearchOpen(false);
            setAddingSpace(false);
          }}
          onTogglePower={() => {
            setPowerOpen((v) => !v);
            setArmedPower(null);
          }}
          onPowerAction={handlePowerClick}
          onSetAddingSpace={(open) => {
            setAddingSpace(open);
            if (open) {
              setPeriphOpen(false);
              setTileCtx(null);
            }
          }}
          onCreateCustomSpace={handleCreateCustomSpace}
        />

        <div
          className="space-stage"
          onClick={() => {
            if (periphOpen) setPeriphOpen(false);
            if (tileCtx) setTileCtx(null);
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
          />
        </div>

        {drawerOpen && (
          <StartPinDrawer
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
        )}

        {settingsOpen && (
          <StartSettingsOverlay
            config={config}
            onClose={() => setSettingsOpen(false)}
            onSetBarMode={async (mode) => {
              const next = await ipc<TestBarConfig>('set_bar_mode', { mode });
              setConfig((prev) => applyConfigUpdate(prev, next));
            }}
            onSetSecondaryMode={async (mode: SecondaryTaskbarMode) => {
              const next = await ipc<TestBarConfig>('set_secondary_taskbar_mode', { mode });
              setConfig((prev) => applyConfigUpdate(prev, next));
            }}
            onSetAutostart={async (enable) => {
              const next = await ipc<TestBarConfig>('set_autostart', { enable });
              setConfig((prev) => applyConfigUpdate(prev, next));
            }}
            onRestoreWindowsAndExit={() => {
              ipc('restore_windows_taskbar_and_exit').catch(() => {});
            }}
          />
        )}

        {searchOpen && (
          <StartCommandPalette
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
        )}

        {tileCtx && (
          <div
            className="flyout-card ctx-card"
            style={{ position: 'fixed', left: tileCtx.x, top: tileCtx.y, width: 236, zIndex: 120 }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="ctx-sec">{tileCtx.item.title}</div>
            <button
              type="button"
              className="ctx-item"
              onClick={() => {
                launchPin(tileCtx.item);
                setTileCtx(null);
              }}
            >
              Открыть
            </button>
            {tileCtx.item.kind === 'app' && canRunAsAdmin(tileCtx.item.target) && (
              <button
                type="button"
                className="ctx-item"
                onClick={() => {
                  ipc('launch_item', {
                    target: tileCtx.item.target,
                    args: tileCtx.item.args ?? null,
                    asAdmin: true,
                    closeStart: true,
                  }).catch(() => {});
                  setTileCtx(null);
                }}
              >
                От имени администратора
              </button>
            )}
            {tileCtx.item.kind === 'folder' && (
              <button
                type="button"
                className="ctx-item"
                onClick={() => {
                  ipc('open_terminal_at', { path: tileCtx.item.target }).catch(() => {});
                  setTileCtx(null);
                }}
              >
                Открыть в терминале
              </button>
            )}
            {canOpenFileLocation(tileCtx.item.target) && (
              <button
                type="button"
                className="ctx-item"
                onClick={() => {
                  ipc('open_location', { path: tileCtx.item.target }).catch(() => {});
                  setTileCtx(null);
                }}
              >
                Расположение файла
              </button>
            )}
            {tileCtx.item.target && (
              <button
                type="button"
                className="ctx-item"
                onClick={() => {
                  ipc('copy_to_clipboard', { text: tileCtx.item.target })
                    .then(() => showToast('Путь скопирован'))
                    .catch(() => {});
                  setTileCtx(null);
                }}
              >
                Скопировать путь
              </button>
            )}
            {tileCtx.item.kind === 'app' && (
              <button
                type="button"
                className="ctx-item"
                onClick={() => {
                  ipc('add_pin', {
                    space: 'taskbar',
                    item: { ...tileCtx.item, id: `tb-${Date.now()}` },
                  }).catch(() => {});
                  setTileCtx(null);
                }}
              >
                Закрепить на панели задач
              </button>
            )}
            <div className="ctx-sep" />
            <button
              type="button"
              className="ctx-item danger"
              onClick={() => {
                ipc('remove_pin', { space: tileCtx.spaceKey, itemId: tileCtx.item.id }).catch(() => {});
                setTileCtx(null);
              }}
            >
              <KillIcon />
              <span>Открепить</span>
            </button>
          </div>
        )}

        {toastMsg && (
          <div className="start-toast" role="status" aria-live="polite">
            {toastMsg}
          </div>
        )}
      </section>
    </div>
  );
};
