import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  ipc,
  onEvent,
  type AppWindowGroup,
  type AudioState,
  type ClipboardEntry,
  type IndexedApp,
  type MediaState,
  type PeripheralBattery,
  type PinItem,
  type SteamGame,
  type TelemetryState,
  type TestBarConfig,
} from '../lib/ipc';
import { matchesQuery } from '../lib/layoutMap';
import { FallbackAppIcon, FolderSvg, KillIcon, VolIcon, XIcon10 } from './Icons';

function fmtSec(s: number): string {
  const sec = Math.max(0, Math.floor(s));
  return `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, '0')}`;
}

function statusPill(pct: number): { cls: string; label: string } {
  if (pct < 45) return { cls: 'st-ok', label: 'Свободно' };
  if (pct < 75) return { cls: 'st-mid', label: 'Норма' };
  return { cls: 'st-hot', label: 'Высокая' };
}

function tempStatus(temp: number): { cls: string; label: string } {
  if (temp < 60) return { cls: 'st-ok', label: 'Холодная' };
  if (temp < 78) return { cls: 'st-mid', label: 'Рабочая' };
  return { cls: 'st-hot', label: 'Горячая' };
}

export const StartView: React.FC = () => {
  const [config, setConfig] = useState<TestBarConfig | null>(null);
  const [groups, setGroups] = useState<AppWindowGroup[]>([]);
  const [apps, setApps] = useState<IndexedApp[]>([]);
  const [games, setGames] = useState<SteamGame[]>([]);
  const [audio, setAudio] = useState<AudioState | null>(null);
  const [media, setMedia] = useState<MediaState | null>(null);
  const [telem, setTelem] = useState<TelemetryState | null>(null);
  const [clips, setClips] = useState<ClipboardEntry[]>([]);
  const [periphs, setPeriphs] = useState<PeripheralBattery[]>([]);
  const [iconMap, setIconMap] = useState<Record<string, string>>({});

  const [space, setSpace] = useState<string>('personal');
  const [editMode, setEditMode] = useState(false);
  const [shiftHeld, setShiftHeld] = useState(false);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [periphOpen, setPeriphOpen] = useState(false);
  const [powerOpen, setPowerOpen] = useState(false);
  const [armedPower, setArmedPower] = useState<'reboot' | 'shutdown' | null>(null);
  const [addingSpace, setAddingSpace] = useState(false);
  const [newSpaceName, setNewSpaceName] = useState('');

  // Universal pinning URL input
  const [urlPromptOpen, setUrlPromptOpen] = useState(false);
  const [urlValue, setUrlValue] = useState('');
  const [drawerCat, setDrawerCat] = useState('Все');
  const [gameFilter, setGameFilter] = useState<'recent' | 'all'>('recent');
  const [copiedClipId, setCopiedClipId] = useState<string | null>(null);
  const [dragIdx, setDragIdx] = useState<number | null>(null);
  const [tileCtx, setTileCtx] = useState<{ x: number; y: number; item: PinItem; spaceKey: string } | null>(null);

  // Instant Type-Anywhere Command Search
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [cmdIdx, setCmdIdx] = useState(0);
  const cmdInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    ipc<TestBarConfig>('get_config').then(setConfig).catch(() => {});
    ipc<AppWindowGroup[]>('get_open_windows').then(setGroups).catch(() => {});
    ipc<IndexedApp[]>('get_installed_apps').then(setApps).catch(() => {});
    ipc<SteamGame[]>('get_steam_games').then(setGames).catch(() => {});
    ipc<AudioState>('get_audio_state').then(setAudio).catch(() => {});
    ipc<MediaState>('get_media_state').then(setMedia).catch(() => {});
    ipc<TelemetryState>('get_telemetry').then(setTelem).catch(() => {});
    ipc<ClipboardEntry[]>('get_clipboard_history').then(setClips).catch(() => {});
    ipc<PeripheralBattery[]>('get_peripherals').then(setPeriphs).catch(() => {});

    const subs = [
      onEvent<TestBarConfig>('config-updated', setConfig),
      onEvent<AppWindowGroup[]>('windows-updated', setGroups),
      onEvent<IndexedApp[]>('apps-indexed', setApps),
      onEvent<SteamGame[]>('games-indexed', setGames),
      onEvent<AudioState>('audio-updated', setAudio),
      onEvent<MediaState>('media-updated', setMedia),
      onEvent<TelemetryState>('telemetry-updated', setTelem),
      onEvent<ClipboardEntry[]>('clipboard-updated', setClips),
      onEvent<PeripheralBattery[]>('peripherals-updated', setPeriphs),
      onEvent<boolean>('start-visibility', (vis) => {
        if (!vis) {
          setSearchOpen(false);
          setQuery('');
          setPowerOpen(false);
          setArmedPower(null);
          setPeriphOpen(false);
        }
      }),
    ];
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  // Load real native .exe icons for pinned tiles across spaces
  useEffect(() => {
    if (!config) return;
    const allPins = [
      ...config.personalPins,
      ...config.workApps,
      ...config.studioPins,
      ...config.studioTools,
      ...config.gameLaunchers,
      ...config.customSpaces.flatMap((s) => s.items),
    ];
    allPins.forEach((p) => {
      if (p.kind === 'app' && p.target && !p.iconData && !iconMap[p.target] && p.target.includes(':\\')) {
        ipc<string | null>('get_icon_for_path', { path: p.target })
          .then((data) => {
            if (data) setIconMap((prev) => ({ ...prev, [p.target]: data }));
          })
          .catch(() => {});
      }
    });
  }, [config]);

  const runningExeSet = useMemo(() => {
    const s = new Set<string>();
    groups.forEach((g) => s.add(g.exePath.toLowerCase()));
    return s;
  }, [groups]);

  const isRunning = (target: string) => {
    if (!target) return false;
    const low = target.toLowerCase();
    return runningExeSet.has(low) || groups.some((g) => g.exePath.toLowerCase().endsWith('\\' + (low.split('\\').pop() || '')));
  };

  // Search corpus combining apps, Steam games, and all pinned items
  const searchResults = useMemo(() => {
    const pool: { name: string; cat: string; target: string; iconData?: string | null; iconKey?: string; kind?: string }[] = [];
    const seen = new Set<string>();

    const addItem = (name: string, cat: string, target: string, iconData?: string | null, iconKey?: string, kind = 'app') => {
      const k = name.toLowerCase();
      if (!target || seen.has(k)) return;
      seen.add(k);
      pool.push({ name, cat, target, iconData, iconKey, kind });
    };

    (config?.personalPins ?? []).forEach((p) => addItem(p.name, 'Личное', p.target, p.iconData || iconMap[p.target], p.iconKey, p.kind));
    (config?.workFolders ?? []).forEach((p) => addItem(p.name, 'Папка', p.target, null, p.iconKey, 'folder'));
    (config?.studioPins ?? []).forEach((p) => addItem(p.name, 'Студия', p.target, p.iconData || iconMap[p.target], p.iconKey, p.kind));
    games.forEach((g) => addItem(g.name, 'Игра · Steam', g.target, null, 'steam', 'app'));
    apps.forEach((a) => addItem(a.name, a.category, a.target, a.iconData, a.name, 'app'));

    let swappedHint: string | null = null;
    const filtered = pool.filter((item) => {
      const res = matchesQuery(query, item.name, item.cat, item.target);
      if (res.matched && res.swapped && !swappedHint) swappedHint = res.swapped;
      return res.matched;
    });
    return { items: filtered.slice(0, 8), swappedHint };
  }, [query, config, apps, games, iconMap]);

  // Global keyboard handler: synchronous type-anywhere search + Tab space switch + Esc + Shift
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Shift') setShiftHeld(true);

      const activeTag = (document.activeElement as HTMLElement)?.tagName;
      const typingInField = activeTag === 'INPUT' && document.activeElement !== cmdInputRef.current;
      if (typingInField) return;

      if (e.key === 'Escape') {
        if (searchOpen) {
          setSearchOpen(false);
          setQuery('');
        } else if (drawerOpen) {
          setDrawerOpen(false);
        } else if (settingsOpen) {
          setSettingsOpen(false);
        } else if (powerOpen) {
          setPowerOpen(false);
          setArmedPower(null);
        } else {
          ipc('toggle_start', { force: false }).catch(() => {});
        }
        return;
      }

      if (e.key === 'Tab' && !searchOpen && !drawerOpen && !settingsOpen) {
        e.preventDefault();
        const allSpaces = ['personal', 'work', 'studio', 'games', ...(config?.customSpaces.map((s) => s.id) ?? [])];
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
          const hit = searchResults.items[cmdIdx];
          ipc('launch_item', { target: hit.target, asAdmin: e.ctrlKey, closeStart: true });
          setSearchOpen(false);
          setQuery('');
        }
        return;
      }

      // Synchronous type-anywhere activation: zero dropped first chars!
      if (!e.ctrlKey && !e.altKey && !e.metaKey && e.key.length === 1 && !drawerOpen && !settingsOpen) {
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

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
    };
  }, [searchOpen, drawerOpen, settingsOpen, powerOpen, space, config, searchResults.items, cmdIdx]);

  const launchPin = (item: PinItem) => {
    if (editMode) return;
    if (shiftHeld && item.kind === 'folder') {
      ipc('open_terminal_at', { path: item.target });
      return;
    }
    ipc('launch_item', { target: item.target, asAdmin: shiftHeld && item.kind === 'app', closeStart: true });
  };

  const handleUniversalPin = async (kind: 'folder' | 'file' | 'url') => {
    if (kind === 'url') {
      setUrlPromptOpen(true);
      return;
    }
    const picked = await ipc<string | null>(kind === 'folder' ? 'pick_folder' : 'pick_file').catch(() => null);
    if (!picked) return;
    const baseName = picked.split('\\').filter(Boolean).pop() || picked;
    const targetSpace = space === 'personal' ? 'personal_links' : space;
    const item: PinItem = {
      id: `pin-${Date.now()}`,
      kind,
      name: baseName,
      subtitle: kind === 'folder' ? 'Папка' : 'Файл',
      target: picked,
      altAction: kind === 'folder' ? 'Открыть в терминале' : 'Открыть',
      accentRgb: kind === 'folder' ? '245,158,11' : '56,189,248',
      iconKey: kind,
      iconData: null,
    };
    await ipc('add_pin', { space: targetSpace, item });
    setDrawerOpen(false);
  };

  const submitUrlPin = async () => {
    let raw = urlValue.trim();
    if (!raw) return;
    if (!/^https?:\/\//i.test(raw)) raw = 'https://' + raw;
    let label = raw;
    try {
      label = new URL(raw).hostname.replace(/^www\./, '');
    } catch {}
    const targetSpace = space === 'personal' ? 'personal_links' : space;
    await ipc('add_pin', {
      space: targetSpace,
      item: {
        id: `url-${Date.now()}`,
        kind: 'url',
        name: label,
        subtitle: 'Ссылка',
        target: raw,
        altAction: 'Открыть',
        accentRgb: '167,139,250',
        iconKey: 'url',
        iconData: null,
      },
    });
    setUrlValue('');
    setUrlPromptOpen(false);
    setDrawerOpen(false);
  };

  const handlePowerClick = (kind: 'lock' | 'sleep' | 'reboot' | 'shutdown') => {
    if (kind === 'reboot' || kind === 'shutdown') {
      if (armedPower !== kind) {
        setArmedPower(kind);
        return;
      }
    }
    setPowerOpen(false);
    setArmedPower(null);
    ipc('power_action', { action: kind }).catch(() => {});
  };

  const renderTile = (item: PinItem, idx: number, spaceKey: string, isHero: boolean) => {
    const running = item.kind === 'app' && isRunning(item.target);
    const nativeIc = item.iconData || iconMap[item.target];
    return (
      <div
        key={item.id}
        className={`app-tile ${isHero ? 'hero' : 'compact'}`}
        style={{ ['--tile-rgb' as string]: item.accentRgb || '129,140,248' }}
        draggable={editMode}
        onDragStart={() => setDragIdx(idx)}
        onDragOver={(e) => {
          if (editMode) {
            e.preventDefault();
            e.currentTarget.classList.add('drag-over');
          }
        }}
        onDragLeave={(e) => e.currentTarget.classList.remove('drag-over')}
        onDrop={(e) => {
          e.preventDefault();
          e.currentTarget.classList.remove('drag-over');
          if (editMode && dragIdx !== null && dragIdx !== idx) {
            ipc('reorder_pins', { space: spaceKey, fromIdx: dragIdx, toIdx: idx });
          }
          setDragIdx(null);
        }}
        onClick={() => launchPin(item)}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setTileCtx({
            x: Math.min(e.clientX, window.innerWidth - 250),
            y: Math.min(e.clientY, window.innerHeight - 230),
            item,
            spaceKey,
          });
        }}
      >
        {editMode && (
          <button
            className="tile-del"
            title="Открепить"
            onClick={(e) => {
              e.stopPropagation();
              ipc('remove_pin', { space: spaceKey, itemId: item.id });
            }}
          >
            <XIcon10 />
          </button>
        )}
        <div className="tile-ic">
          {nativeIc ? (
            <img src={nativeIc} alt="" className={isHero ? 'native-ic-lg' : 'native-ic'} draggable={false} />
          ) : (
            <FallbackAppIcon iconKey={item.iconKey} kind={item.kind} size={isHero ? 24 : 20} />
          )}
          {running && <span className="run-led" />}
        </div>
        <div className="tile-meta">
          <div className="tile-name">{item.name}</div>
          <div className={`tile-sub ${item.altAction ? 'has-alt' : ''}`}>
            {shiftHeld && item.altAction ? item.altAction : item.subtitle}
          </div>
        </div>
      </div>
    );
  };

  const spaceTheme = ['personal', 'work', 'studio', 'games'].includes(space) ? space : 'custom';
  const mouseBat = periphs.find((p) => p.kind === 'mouse');
  const audioBat = periphs.find((p) => p.kind === 'audio');
  const activeDev = audio?.devices.find((d) => d.isDefault);
  const audioDevs = (audio?.devices ?? []).slice(0, 3);
  const mixerSessions = (audio?.sessions ?? []).slice(0, 3);
  const categories = useMemo(() => {
    const set = new Set<string>(['Все']);
    apps.forEach((a) => set.add(a.category));
    return Array.from(set);
  }, [apps]);
  const filteredDrawerApps = useMemo(
    () => (drawerCat === 'Все' ? apps : apps.filter((a) => a.category === drawerCat)),
    [apps, drawerCat],
  );
  const displayedGames = gameFilter === 'recent' ? games.slice(0, 4) : games;

  return (
    <div className={`start-root ${editMode ? 'edit-mode' : ''} ${shiftHeld ? 'shift-active' : ''}`} data-space={spaceTheme}>
      <section className="start-chassis">
        <div className="chassis-aura" />

        {/* HEADER */}
        <header className="start-header">
          <div className="hdr-left">
            <button
              className={`periph-pill ${periphOpen ? 'open' : ''}`}
              title="Устройства и вывод звука"
              onClick={() => setPeriphOpen((v) => !v)}
            >
              {mouseBat && (
                <>
                  <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                    <rect x="6" y="3" width="12" height="18" rx="6" />
                    <line x1="12" y1="7" x2="12" y2="10" />
                  </svg>
                  <span>{mouseBat.percent}%</span>
                  <span className="bat-track">
                    <i className="bat-fill" style={{ width: `${mouseBat.percent}%`, background: '#34D399' }} />
                  </span>
                  <span className="periph-sep" />
                </>
              )}
              <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
                <path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3zM3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3z" />
              </svg>
              <span>{audioBat ? `${audioBat.percent}%` : (activeDev?.shortName || 'Звук')}</span>
              {audioBat && (
                <span className="bat-track">
                  <i className="bat-fill" style={{ width: `${audioBat.percent}%`, background: '#818CF8' }} />
                </span>
              )}
            </button>
          </div>

          <nav className="space-nav">
            {[
              { id: 'personal', label: 'Личное' },
              { id: 'work', label: 'Работа' },
              { id: 'studio', label: 'Студия' },
              { id: 'games', label: 'Игры' },
            ].map((t) => (
              <button
                key={t.id}
                className={`space-tab ${space === t.id ? 'active' : ''}`}
                onClick={() => {
                  setSpace(t.id);
                  setDrawerOpen(false);
                  setSettingsOpen(false);
                }}
              >
                <span className="s-dot" />
                {t.label}
              </button>
            ))}
            {(config?.customSpaces ?? []).map((cs) => (
              <button
                key={cs.id}
                className={`space-tab ${space === cs.id ? 'active' : ''}`}
                onClick={() => {
                  setSpace(cs.id);
                  setDrawerOpen(false);
                  setSettingsOpen(false);
                }}
              >
                <span className="s-dot" />
                {cs.name}
              </button>
            ))}
            {addingSpace ? (
              <input
                className="space-new-input"
                autoFocus
                placeholder="Название..."
                maxLength={14}
                value={newSpaceName}
                onChange={(e) => setNewSpaceName(e.target.value)}
                onBlur={() => {
                  setAddingSpace(false);
                  setNewSpaceName('');
                }}
                onKeyDown={async (e) => {
                  if (e.key === 'Enter' && newSpaceName.trim()) {
                    const updated = await ipc<TestBarConfig>('add_custom_space', { name: newSpaceName.trim() });
                    const newest = updated.customSpaces[updated.customSpaces.length - 1];
                    if (newest) setSpace(newest.id);
                    setAddingSpace(false);
                    setNewSpaceName('');
                  } else if (e.key === 'Escape') {
                    setAddingSpace(false);
                    setNewSpaceName('');
                  }
                }}
              />
            ) : (
              <button className="space-add-btn" title="Создать пространство" onClick={() => setAddingSpace(true)}>
                <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <line x1="12" y1="5" x2="12" y2="19" />
                  <line x1="5" y1="12" x2="19" y2="12" />
                </svg>
              </button>
            )}
          </nav>

          <div className={`hdr-right ${powerOpen ? 'power-open' : ''}`}>
            <div className="hdr-tools-group">
              <button
                className={`hdr-btn ${editMode ? 'active' : ''}`}
                title="Редактировать"
                onClick={() => setEditMode((v) => !v)}
              >
                <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M12 20h9" />
                  <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" />
                </svg>
              </button>
              <button
                className={`hdr-btn ${drawerOpen ? 'active' : ''}`}
                title="Все программы и закрепление"
                onClick={() => {
                  setDrawerOpen((v) => !v);
                  setSettingsOpen(false);
                }}
              >
                <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
                  <rect x="3" y="3" width="7" height="7" rx="1.5" />
                  <rect x="14" y="3" width="7" height="7" rx="1.5" />
                  <rect x="3" y="14" width="7" height="7" rx="1.5" />
                  <rect x="14" y="14" width="7" height="7" rx="1.5" />
                </svg>
              </button>
              <button
                className={`hdr-btn gear ${settingsOpen ? 'active' : ''}`}
                title="Настройки"
                onClick={() => {
                  setSettingsOpen((v) => !v);
                  setDrawerOpen(false);
                }}
              >
                <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                  <circle cx="12" cy="12" r="3" />
                  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9c.26.6.85 1 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
                </svg>
              </button>
              <div className="pwr-spacer" />
            </div>

            <div className={`power-capsule ${powerOpen ? 'open' : ''}`}>
              <div className="power-strip">
                <button className="pwr-item" onClick={() => handlePowerClick('lock')}>Блок</button>
                <button className="pwr-item" onClick={() => handlePowerClick('sleep')}>Сон</button>
                <button
                  className={`pwr-item danger ${armedPower === 'reboot' ? 'armed' : ''}`}
                  onClick={() => handlePowerClick('reboot')}
                >
                  {armedPower === 'reboot' ? 'Точно?' : 'Ребут'}
                </button>
                <button
                  className={`pwr-item danger ${armedPower === 'shutdown' ? 'armed' : ''}`}
                  onClick={() => handlePowerClick('shutdown')}
                >
                  {armedPower === 'shutdown' ? 'Выкл?' : 'Выкл'}
                </button>
              </div>
              <button
                className="pwr-trigger"
                title="Питание"
                onClick={() => {
                  setPowerOpen((v) => !v);
                  setArmedPower(null);
                }}
              >
                <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
                  <line x1="12" y1="2" x2="12" y2="12" />
                </svg>
              </button>
            </div>
          </div>
        </header>

        {/* PERIPHERALS & AUDIO OUTPUT POPOVER */}
        {periphOpen && (
          <div className="periph-pop">
            <div style={{ fontSize: 10.5, fontWeight: 600, color: 'var(--t2)', letterSpacing: '0.06em', marginBottom: 8 }}>
              УСТРОЙСТВА И ВЫВОД
            </div>
            {periphs.map((p) => (
              <div key={p.name} className="pp-row">
                <span className="pp-ic">
                  <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                    <rect x="6" y="3" width="12" height="18" rx="6" />
                    <line x1="12" y1="7" x2="12" y2="10" />
                  </svg>
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 12, fontWeight: 500, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</div>
                  <div style={{ fontSize: 10.5, color: 'var(--t3)' }}>Bluetooth</div>
                </div>
                <strong style={{ fontFamily: 'monospace', color: p.percent > 30 ? '#34D399' : '#FB7185' }}>{p.percent}%</strong>
              </div>
            ))}
            {(audio?.devices ?? []).map((d) => (
              <div
                key={d.id}
                className={`pp-row ${d.isDefault ? 'active' : ''}`}
                onClick={() => {
                  ipc<AudioState>('set_audio_device', { deviceId: d.id }).then(setAudio);
                  setPeriphOpen(false);
                }}
              >
                <span className="pp-ic"><VolIcon /></span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 12, fontWeight: 500, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{d.shortName}</div>
                  <div style={{ fontSize: 10.5, color: d.isDefault ? 'var(--accent)' : 'var(--t3)' }}>
                    {d.isDefault ? 'Активный вывод' : 'Переключить'}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* MAIN STAGE */}
        <div className="space-stage" onClick={() => { if (periphOpen) setPeriphOpen(false); if (tileCtx) setTileCtx(null); }}>

          {/* 1. ЛИЧНОЕ */}
          {space === 'personal' && (
            <div className="space-panel">
              <div className="bento-col">
                <div className="spot-card" style={{ flex: 1, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
                  <div>
                    <div className="sec-row">
                      <span className="sec-title">Закреплено</span>
                      <button className="sec-act" onClick={() => setDrawerOpen(true)}>+ Закрепить</button>
                    </div>
                    <div className="hero-pair">
                      {(config?.personalPins ?? []).slice(0, 2).map((p, i) => renderTile(p, i, 'personal', true))}
                    </div>
                    <div className="sub-grid">
                      {(config?.personalPins ?? []).slice(2, 11).map((p, i) => renderTile(p, i + 2, 'personal', false))}
                    </div>
                  </div>

                  <div style={{ paddingTop: 10, borderTop: '1px solid rgba(255,255,255,0.065)' }}>
                    <div className="sec-row" style={{ marginBottom: 8 }}>
                      <span className="sec-title">Быстрый доступ</span>
                      <button className="sec-act" onClick={() => setDrawerOpen(true)}>+ Папка / Файл / URL</button>
                    </div>
                    <div className="qlinks-grid">
                      {(config?.personalLinks ?? []).slice(0, 4).map((l) => (
                        <button key={l.id} className="qlink" onClick={() => launchPin(l)}>
                          <FallbackAppIcon iconKey={l.iconKey} kind={l.kind} size={16} />
                          <div style={{ minWidth: 0 }}>
                            <div className="qlink-name">{l.name}</div>
                            <div className="qlink-sub">{l.subtitle}</div>
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                </div>
              </div>

              <div className="bento-col">
                <div className={`spot-card media-card ${!media?.isPlaying ? 'paused' : ''}`}>
                  <div className="media-top">
                    <div className="album-box">
                      <div className="eq-wave"><i /><i /><i /><i /></div>
                    </div>
                    <div style={{ minWidth: 0, flex: 1 }}>
                      <div style={{ fontSize: 13, fontWeight: 600, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                        {media?.title || 'Ничего не воспроизводится'}
                      </div>
                      <div style={{ fontSize: 11, color: 'var(--t2)', marginTop: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                        {media?.hasSession ? [media.artist, media.source].filter(Boolean).join(' · ') : 'Запусти музыку или видео'}
                      </div>
                    </div>
                    <div className="m-ctrls">
                      <button className="m-btn" onClick={() => ipc<MediaState>('media_action', { action: 'prev' }).then(setMedia)}>
                        <svg className="ic14" viewBox="0 0 24 24" fill="currentColor"><polygon points="19 4 9 12 19 20 19 4" /><rect x="5" y="5" width="2.4" height="14" rx="1" /></svg>
                      </button>
                      <button className="m-btn play" onClick={() => ipc<MediaState>('media_action', { action: 'toggle' }).then(setMedia)}>
                        {media?.isPlaying ? (
                          <svg className="ic14" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1" /><rect x="14" y="4" width="4" height="16" rx="1" /></svg>
                        ) : (
                          <svg className="ic14" viewBox="0 0 24 24" fill="currentColor"><polygon points="6 4 20 12 6 20 6 4" /></svg>
                        )}
                      </button>
                      <button className="m-btn" onClick={() => ipc<MediaState>('media_action', { action: 'next' }).then(setMedia)}>
                        <svg className="ic14" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 4 15 12 5 20 5 4" /><rect x="17" y="5" width="2.4" height="14" rx="1" /></svg>
                      </button>
                    </div>
                  </div>
                  <div className="scrub-wrap">
                    <span className="scrub-time">{fmtSec(media?.positionSec ?? 0)}</span>
                    <div className="scrub-bar">
                      <div
                        className="scrub-fill"
                        style={{
                          width: media && media.durationSec > 0
                            ? `${Math.min(100, (media.positionSec / media.durationSec) * 100)}%`
                            : '0%',
                        }}
                      />
                    </div>
                    <span className="scrub-time">{fmtSec(media?.durationSec ?? 0)}</span>
                  </div>
                </div>

                <div className="spot-card" style={{ padding: '12px 14px' }}>
                  <div className="sec-row" style={{ marginBottom: 7 }}><span className="sec-title">Звук и микшер</span></div>
                  {audioDevs.length > 0 && (
                    <div className="seg-bar" style={{ gridTemplateColumns: `repeat(${audioDevs.length}, 1fr)` }}>
                      {audioDevs.map((d) => (
                        <button
                          key={d.id}
                          className={`seg-opt ${d.isDefault ? 'active' : ''}`}
                          onClick={() => ipc<AudioState>('set_audio_device', { deviceId: d.id }).then(setAudio)}
                        >
                          {d.shortName}
                        </button>
                      ))}
                    </div>
                  )}
                  <div className="mixer-list">
                    {mixerSessions.length === 0 ? (
                      <div className={`mix-row ${audio?.masterMuted ? 'muted' : ''}`}>
                        <button
                          className="mix-mute"
                          onClick={() => ipc<AudioState>('set_master_volume', { volume: audio?.masterVolume ?? 50, muted: !(audio?.masterMuted ?? false) }).then(setAudio)}
                        >
                          <VolIcon muted={audio?.masterMuted} />
                        </button>
                        <span className="mix-name">Общий звук</span>
                        <input
                          type="range"
                          className="mix-range"
                          min={0}
                          max={100}
                          value={audio?.masterMuted ? 0 : (audio?.masterVolume ?? 50)}
                          onChange={(e) => ipc<AudioState>('set_master_volume', { volume: Number(e.target.value), muted: false }).then(setAudio)}
                        />
                        <span className="mix-val">{audio?.masterMuted ? '0%' : `${audio?.masterVolume ?? 0}%`}</span>
                      </div>
                    ) : (
                      mixerSessions.map((s) => (
                        <div key={s.pid} className={`mix-row ${s.muted ? 'muted' : ''}`}>
                          <button
                            className="mix-mute"
                            onClick={() => ipc<AudioState>('set_session_volume', { pid: s.pid, volume: s.volume, muted: !s.muted }).then(setAudio)}
                          >
                            <VolIcon muted={s.muted} />
                          </button>
                          <span className="mix-name">{s.name}</span>
                          <input
                            type="range"
                            className="mix-range"
                            min={0}
                            max={100}
                            value={s.muted ? 0 : s.volume}
                            onChange={(e) => ipc<AudioState>('set_session_volume', { pid: s.pid, volume: Number(e.target.value), muted: false }).then(setAudio)}
                          />
                          <span className="mix-val">{s.muted ? '0%' : `${s.volume}%`}</span>
                        </div>
                      ))
                    )}
                  </div>
                </div>

                <div className="spot-card" style={{ flex: 1, padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 8 }}>
                  <div className="sec-row" style={{ marginBottom: 0 }}>
                    <span className="sec-title">Буфер обмена</span>
                    <button className="sec-act" onClick={() => ipc<ClipboardEntry[]>('clear_unpinned_clips').then(setClips)}>Очистить</button>
                  </div>
                  <div className="clip-list">
                    {clips.length === 0 ? (
                      <div style={{ fontSize: 11.5, color: 'var(--t3)', padding: '10px 4px' }}>Скопируй текст или ссылку — появится здесь</div>
                    ) : (
                      clips.slice(0, 3).map((c) => (
                        <div
                          key={c.id}
                          className={`clip-item ${copiedClipId === c.id ? 'copied' : ''}`}
                          onClick={() => {
                            ipc('copy_to_clipboard', { text: c.text });
                            setCopiedClipId(c.id);
                            setTimeout(() => setCopiedClipId(null), 900);
                          }}
                        >
                          {c.color ? <span className="clip-swatch" style={{ background: c.color }} /> : <span className="clip-tag">{c.tag}</span>}
                          <span className="clip-txt">{c.text}</span>
                          <button
                            className={`clip-pin ${c.pinned ? 'pinned' : ''}`}
                            onClick={(e) => {
                              e.stopPropagation();
                              ipc<ClipboardEntry[]>('toggle_clip_pin', { id: c.id }).then(setClips);
                            }}
                          >
                            <svg width="12" height="12" viewBox="0 0 24 24" fill={c.pinned ? 'currentColor' : 'none'} stroke="currentColor" strokeWidth="2">
                              <path d="M12 17v5" />
                              <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z" />
                            </svg>
                          </button>
                        </div>
                      ))
                    )}
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* 2. РАБОТА */}
          {space === 'work' && (
            <div className="space-panel">
              <div className="bento-col">
                <div className="spot-card" style={{ flex: 1, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
                  <div>
                    <div className="sec-row">
                      <span className="sec-title">Проекты, папки и файлы</span>
                      <button className="sec-act" onClick={() => setDrawerOpen(true)}>+ Закрепить</button>
                    </div>
                    <div className="folder-hub-grid">
                      {(config?.workFolders ?? []).slice(0, 8).map((f) => (
                        <div key={f.id} className="folder-card" onClick={() => launchPin(f)}>
                          {editMode && (
                            <button
                              className="tile-del"
                              onClick={(e) => {
                                e.stopPropagation();
                                ipc('remove_pin', { space: 'work', itemId: f.id });
                              }}
                            >
                              <XIcon10 />
                            </button>
                          )}
                          <div className="folder-ic-box"><FolderSvg className="ic18" /></div>
                          <div className="tile-meta">
                            <div className="tile-name">{f.name}</div>
                            <div className="tile-sub">{f.subtitle}</div>
                          </div>
                          {!editMode && (
                            <div className="folder-acts">
                              <button
                                className="f-mini"
                                title="Открыть в терминале"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  ipc('open_terminal_at', { path: f.target });
                                }}
                              >
                                Терм.
                              </button>
                              <button
                                className="f-mini"
                                title="Скопировать путь"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  ipc('copy_to_clipboard', { text: f.target });
                                }}
                              >
                                Путь
                              </button>
                            </div>
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                  <div style={{ paddingTop: 10, borderTop: '1px solid rgba(255,255,255,0.065)' }}>
                    <div className="sec-row" style={{ marginBottom: 8 }}>
                      <span className="sec-title">Рабочие программы</span>
                      <button className="sec-act" onClick={() => setDrawerOpen(true)}>+ Добавить</button>
                    </div>
                    <div className="qlinks-grid">
                      {(config?.workApps ?? []).slice(0, 4).map((a) => (
                        <button key={a.id} className="qlink" onClick={() => launchPin(a)}>
                          {a.iconData || iconMap[a.target] ? (
                            <img src={(a.iconData || iconMap[a.target])!} alt="" className="native-ic-sm" />
                          ) : (
                            <FallbackAppIcon iconKey={a.iconKey} size={18} />
                          )}
                          <div style={{ minWidth: 0 }}>
                            <div className="qlink-name">{a.name}</div>
                            <div className="qlink-sub">{a.subtitle}</div>
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                </div>
              </div>

              <div className="bento-col">
                <div className="spot-card" style={{ padding: '13px 14px' }}>
                  <div className="sec-row" style={{ marginBottom: 9 }}>
                    <span className="sec-title">Нагрузка ПК</span>
                    <span style={{ fontSize: 10.5, color: 'var(--t3)' }}>
                      {telem ? `${telem.ramTotalGb} ГБ${telem.gpuAvailable ? ` · ${telem.gpuName}` : ''}` : 'Система'}
                    </span>
                  </div>
                  <div className="health-list">
                    <div className="health-row">
                      <div className="health-top">
                        <span className="health-lbl">Процессор <span className="health-sub">· {telem?.cpuName || 'CPU'}</span></span>
                        <span className="health-val">
                          {telem?.cpuPct ?? 0}% <span className={`status-pill ${statusPill(telem?.cpuPct ?? 0).cls}`}>{statusPill(telem?.cpuPct ?? 0).label}</span>
                        </span>
                      </div>
                      <div className="health-bar"><div className="health-fill" style={{ width: `${telem?.cpuPct ?? 0}%`, background: '#34D399' }} /></div>
                    </div>

                    <div className="health-row">
                      <div className="health-top">
                        <span className="health-lbl">ОЗУ <span className="health-sub">· RAM</span></span>
                        <span className="health-val">
                          {telem ? `${telem.ramUsedGb} / ${telem.ramTotalGb} ГБ` : '—'} <span className={`status-pill ${statusPill(telem?.ramPct ?? 0).cls}`}>{statusPill(telem?.ramPct ?? 0).label}</span>
                        </span>
                      </div>
                      <div className="health-bar"><div className="health-fill" style={{ width: `${telem?.ramPct ?? 0}%`, background: '#38BDF8' }} /></div>
                    </div>

                    {telem?.gpuAvailable && (
                      <>
                        <div className="health-row">
                          <div className="health-top">
                            <span className="health-lbl">{telem.gpuName} <span className="health-sub">· {telem.gpuTempC}°C</span></span>
                            <span className="health-val">
                              {telem.gpuPct}% <span className={`status-pill ${tempStatus(telem.gpuTempC).cls}`}>{tempStatus(telem.gpuTempC).label}</span>
                            </span>
                          </div>
                          <div className="health-bar"><div className="health-fill" style={{ width: `${telem.gpuPct}%`, background: '#818CF8' }} /></div>
                        </div>

                        <div className="health-row">
                          <div className="health-top">
                            <span className="health-lbl">Видеопамять <span className="health-sub">· VRAM</span></span>
                            <span className="health-val">
                              {telem.vramUsedGb} / {telem.vramTotalGb} ГБ <span className={`status-pill ${statusPill(telem.vramPct).cls}`}>{statusPill(telem.vramPct).label}</span>
                            </span>
                          </div>
                          <div className="health-bar"><div className="health-fill" style={{ width: `${telem.vramPct}%`, background: '#F59E0B' }} /></div>
                        </div>
                      </>
                    )}
                  </div>
                </div>

                <div className="spot-card" style={{ flex: 1, padding: '13px 14px', display: 'flex', flexDirection: 'column', gap: 8 }}>
                  <div className="sec-row" style={{ marginBottom: 0 }}>
                    <span className="sec-title">Кто сейчас ест память</span>
                    <button className="sec-act" onClick={() => ipc('launch_item', { target: 'C:\\Windows\\System32\\Taskmgr.exe', asAdmin: false, closeStart: true })}>
                      Диспетчер ↗
                    </button>
                  </div>
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
                    {(telem?.topProcesses ?? []).slice(0, 3).map((pr) => (
                      <div key={pr.pid} className="consumer-row">
                        {pr.iconData ? <img src={pr.iconData} alt="" className="native-ic-sm" /> : <FallbackAppIcon iconKey={pr.name} size={18} />}
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <div style={{ fontSize: 12, fontWeight: 600, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{pr.name}</div>
                          <div style={{ fontSize: 10, color: 'var(--t3)' }}>{pr.cpuPct}% CPU</div>
                        </div>
                        <span style={{ fontFamily: '"Cascadia Code", monospace', fontSize: 11.5, fontWeight: 600 }}>{pr.memGb} ГБ</span>
                        <button
                          className="consumer-kill"
                          title="Завершить процесс"
                          onClick={() => ipc('kill_process_pid', { pid: pr.pid }).then(() => ipc<TelemetryState>('get_telemetry').then(setTelem))}
                        >
                          <KillIcon />
                        </button>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* 3. СТУДИЯ (1-LINE MEMORY ROWS, NO "РЕЖИМЫ РАБОТЫ") */}
          {space === 'studio' && (
            <div className="space-panel">
              <div className="bento-col">
                <div className="spot-card" style={{ flex: 1, display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
                  <div>
                    <div className="sec-row">
                      <span className="sec-title">Творчество и AI</span>
                      <button className="sec-act" onClick={() => setDrawerOpen(true)}>+ Закрепить</button>
                    </div>
                    <div className="hero-pair">
                      {(config?.studioPins ?? []).slice(0, 2).map((p, i) => renderTile(p, i, 'studio', true))}
                    </div>
                    <div className="sub-grid">
                      {(config?.studioPins ?? []).slice(2, 11).map((p, i) => renderTile(p, i + 2, 'studio', false))}
                    </div>
                  </div>
                  <div style={{ paddingTop: 10, borderTop: '1px solid rgba(255,255,255,0.065)' }}>
                    <div className="sec-row" style={{ marginBottom: 8 }}><span className="sec-title">Инструменты</span></div>
                    <div className="qlinks-grid">
                      {(config?.studioTools ?? []).slice(0, 4).map((t) => (
                        <button key={t.id} className="qlink" onClick={() => launchPin(t)}>
                          <FallbackAppIcon iconKey={t.iconKey} size={16} />
                          <div style={{ minWidth: 0 }}>
                            <div className="qlink-name">{t.name}</div>
                            <div className="qlink-sub">{t.subtitle}</div>
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                </div>
              </div>

              <div className="bento-col">
                <div className="spot-card" style={{ padding: '13px 14px' }}>
                  <div className="sec-row" style={{ marginBottom: 9 }}>
                    <span className="sec-title">Запас памяти</span>
                    <span className={`status-pill ${statusPill(telem?.ramPct ?? 0).cls}`}>{statusPill(telem?.ramPct ?? 0).label}</span>
                  </div>
                  <div className="health-list">
                    {telem?.gpuAvailable && (
                      <div className="health-row">
                        <div className="health-top">
                          <span className="health-lbl">VRAM <span className="health-sub">· {telem.gpuName} · {telem.gpuTempC}°C</span></span>
                          <span className="health-val">
                            {telem.vramUsedGb} / {telem.vramTotalGb} ГБ <span className="status-pill st-mid">{telem.vramPct}%</span>
                          </span>
                        </div>
                        <div className="health-bar"><div className="health-fill" style={{ width: `${telem.vramPct}%`, background: '#34D399' }} /></div>
                      </div>
                    )}
                    <div className="health-row">
                      <div className="health-top">
                        <span className="health-lbl">ОЗУ <span className="health-sub">· RAM</span></span>
                        <span className="health-val">
                          {telem ? `${telem.ramUsedGb} / ${telem.ramTotalGb} ГБ` : '—'} <span className="status-pill st-ok">{telem?.ramPct ?? 0}%</span>
                        </span>
                      </div>
                      <div className="health-bar"><div className="health-fill" style={{ width: `${telem?.ramPct ?? 0}%`, background: '#38BDF8' }} /></div>
                    </div>
                  </div>
                </div>

                <div className="spot-card" style={{ padding: '12px 14px' }}>
                  <div className="sec-row" style={{ marginBottom: 8 }}><span className="sec-title">Аудио и микрофон</span></div>
                  {audioDevs.length > 0 && (
                    <div className="seg-bar" style={{ gridTemplateColumns: `repeat(${audioDevs.length}, 1fr)`, marginBottom: 8 }}>
                      {audioDevs.map((d) => (
                        <button
                          key={d.id}
                          className={`seg-opt ${d.isDefault ? 'active' : ''}`}
                          onClick={() => ipc<AudioState>('set_audio_device', { deviceId: d.id }).then(setAudio)}
                        >
                          {d.shortName}
                        </button>
                      ))}
                    </div>
                  )}
                  <button
                    className={`sys-tgl ${!(audio?.micMuted ?? false) ? 'on' : ''}`}
                    style={{ width: '100%' }}
                    onClick={() => ipc<AudioState>('set_mic_muted', { muted: !(audio?.micMuted ?? false) }).then(setAudio)}
                  >
                    <svg className="ic16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
                      <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
                      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                      <line x1="12" y1="19" x2="12" y2="23" />
                    </svg>
                    <div>
                      <div style={{ fontSize: 12, fontWeight: 600 }}>Микрофон</div>
                      <div style={{ fontSize: 10, color: 'var(--t2)' }}>{audio?.micMuted ? 'Замьючен' : 'Активен'}</div>
                    </div>
                    <span className="vu-bars"><i /><i /><i /></span>
                  </button>
                </div>

                <div className="spot-card" style={{ flex: 1, padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 8 }}>
                  <div className="sec-row" style={{ marginBottom: 0 }}><span className="sec-title">Активные процессы GPU / ОЗУ</span></div>
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
                    {(telem?.topProcesses ?? []).slice(0, 3).map((pr) => (
                      <div key={pr.pid} className="consumer-row">
                        {pr.iconData ? <img src={pr.iconData} alt="" className="native-ic-sm" /> : <FallbackAppIcon iconKey={pr.name} size={18} />}
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <div style={{ fontSize: 12, fontWeight: 600, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{pr.name}</div>
                          <div style={{ fontSize: 10, color: 'var(--t3)' }}>{pr.cpuPct}% CPU</div>
                        </div>
                        <span style={{ fontFamily: '"Cascadia Code", monospace', fontSize: 11.5, fontWeight: 600 }}>{pr.memGb} ГБ</span>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* 4. ИГРЫ */}
          {space === 'games' && (
            <div className="space-panel">
              <div className="bento-col">
                <div className="spot-card" style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
                  <div className="sec-row" style={{ marginBottom: 9 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                      <span className="sec-title">Игры</span>
                      <div className="mini-tabs">
                        <button className={`mini-tab ${gameFilter === 'recent' ? 'active' : ''}`} onClick={() => setGameFilter('recent')}>
                          Недавние ({Math.min(4, games.length)})
                        </button>
                        <button className={`mini-tab ${gameFilter === 'all' ? 'active' : ''}`} onClick={() => setGameFilter('all')}>
                          Все игры ({games.length})
                        </button>
                      </div>
                    </div>
                    <button className="sec-act" onClick={() => setDrawerOpen(true)}>+ Добавить</button>
                  </div>

                  <div className="games-scroll">
                    {displayedGames.length === 0 ? (
                      <div style={{ padding: 24, textAlign: 'center', color: 'var(--t3)', fontSize: 12.5 }}>
                        Библиотека Steam сканируется или игры не найдены
                      </div>
                    ) : (
                      <div className="games-grid">
                        {displayedGames.map((g) => (
                          <div
                            key={g.appId}
                            className="game-card"
                            onClick={() => ipc('launch_item', { target: g.target, asAdmin: false, closeStart: true })}
                          >
                            <div
                              className="game-bg"
                              style={{
                                backgroundImage: g.coverData
                                  ? `url("${g.coverData}")`
                                  : 'linear-gradient(135deg, #1E293B, #0F172A)',
                              }}
                            />
                            <div className="game-grad" />
                            <span className="game-play">▶ Играть</span>
                            <div className="game-info">
                              <div className="game-title">{g.name}</div>
                              <div className="game-meta">{g.launcher}</div>
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>

                  <div style={{ marginTop: 10, paddingTop: 10, borderTop: '1px solid rgba(255,255,255,0.065)' }}>
                    <div className="sec-row" style={{ marginBottom: 7 }}><span className="sec-title">Лаунчеры и компаньоны</span></div>
                    <div className="qlinks-grid">
                      {(config?.gameLaunchers ?? []).slice(0, 4).map((l) => (
                        <button key={l.id} className="qlink" onClick={() => launchPin(l)}>
                          {l.iconData || iconMap[l.target] ? (
                            <img src={(l.iconData || iconMap[l.target])!} alt="" className="native-ic-sm" />
                          ) : (
                            <FallbackAppIcon iconKey={l.iconKey} size={18} />
                          )}
                          <div style={{ minWidth: 0 }}>
                            <div className="qlink-name">{l.name}</div>
                            <div className="qlink-sub">{l.subtitle}</div>
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>
                </div>
              </div>

              <div className="bento-col">
                <div className="spot-card" style={{ padding: '13px 14px' }}>
                  <div className="sec-row" style={{ marginBottom: 8 }}><span className="sec-title">Игровой звук и голос</span></div>
                  <button
                    className={`sys-tgl ${!(audio?.micMuted ?? false) ? 'on' : ''}`}
                    style={{ width: '100%', marginBottom: 9 }}
                    onClick={() => ipc<AudioState>('set_mic_muted', { muted: !(audio?.micMuted ?? false) }).then(setAudio)}
                  >
                    <svg className="ic16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
                      <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
                      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                      <line x1="12" y1="19" x2="12" y2="23" />
                    </svg>
                    <div>
                      <div style={{ fontSize: 12, fontWeight: 600 }}>Микрофон</div>
                      <div style={{ fontSize: 10, color: 'var(--t2)' }}>{audio?.micMuted ? 'Замьючен' : 'Активен'}</div>
                    </div>
                    <span className="vu-bars"><i /><i /><i /></span>
                  </button>
                  <div className="mixer-list">
                    {mixerSessions.slice(0, 3).map((s) => (
                      <div key={s.pid} className={`mix-row ${s.muted ? 'muted' : ''}`}>
                        <button
                          className="mix-mute"
                          onClick={() => ipc<AudioState>('set_session_volume', { pid: s.pid, volume: s.volume, muted: !s.muted }).then(setAudio)}
                        >
                          <VolIcon muted={s.muted} />
                        </button>
                        <span className="mix-name">{s.name}</span>
                        <input
                          type="range"
                          className="mix-range"
                          min={0}
                          max={100}
                          value={s.muted ? 0 : s.volume}
                          onChange={(e) => ipc<AudioState>('set_session_volume', { pid: s.pid, volume: Number(e.target.value), muted: false }).then(setAudio)}
                        />
                        <span className="mix-val">{s.muted ? '0%' : `${s.volume}%`}</span>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="spot-card" style={{ flex: 1, padding: '13px 14px', display: 'flex', flexDirection: 'column', justifyContent: 'space-between' }}>
                  <div>
                    <div className="sec-row" style={{ marginBottom: 8 }}><span className="sec-title">Игровые связки</span></div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: 7 }}>
                      {(config?.gameCombos ?? []).map((cb) => (
                        <button
                          key={cb.id}
                          className="consumer-row"
                          style={{ width: '100%', cursor: 'pointer', textAlign: 'left', color: 'var(--t1)', fontFamily: 'inherit' }}
                          onClick={() => ipc('launch_combo', { targets: cb.targets })}
                        >
                          <span style={{ width: 28, height: 28, borderRadius: 8, background: `rgba(${cb.accentRgb}, 0.16)`, color: `rgb(${cb.accentRgb})`, display: 'grid', placeItems: 'center', flexShrink: 0 }}>
                            <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3" /></svg>
                          </span>
                          <div style={{ flex: 1, minWidth: 0 }}>
                            <div style={{ fontSize: 12, fontWeight: 600 }}>{cb.name}</div>
                            <div style={{ fontSize: 10.5, color: 'var(--t3)' }}>{cb.subtitle}</div>
                          </div>
                        </button>
                      ))}
                    </div>
                  </div>

                  {telem?.gpuAvailable && (
                    <div className="health-row" style={{ marginTop: 8 }}>
                      <div className="health-top" style={{ marginBottom: 5 }}>
                        <span className="health-lbl">{telem.gpuName} <span className="health-sub">· {telem.gpuTempC}°C</span></span>
                        <span className="health-val">
                          {telem.vramUsedGb} / {telem.vramTotalGb} ГБ <span className={`status-pill ${tempStatus(telem.gpuTempC).cls}`}>{tempStatus(telem.gpuTempC).label}</span>
                        </span>
                      </div>
                      <div className="health-bar"><div className="health-fill" style={{ width: `${telem.vramPct}%`, background: '#34D399' }} /></div>
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}

          {/* CUSTOM SPACE */}
          {!['personal', 'work', 'studio', 'games'].includes(space) && (() => {
            const cs = config?.customSpaces.find((s) => s.id === space);
            if (!cs) return null;
            return (
              <div className="space-panel" style={{ gridTemplateColumns: '1fr' }}>
                <div className="spot-card" style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 12 }}>
                  <div className="sec-row" style={{ marginBottom: 0 }}>
                    <span className="sec-title">{cs.name}</span>
                    <button className="sec-act" onClick={() => setDrawerOpen(true)}>+ Закрепить</button>
                  </div>
                  {cs.items.length === 0 ? (
                    <div style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 10, color: 'var(--t2)' }}>
                      <div style={{ fontSize: 14, fontWeight: 600 }}>Пока пусто</div>
                      <div style={{ fontSize: 12, color: 'var(--t3)' }}>Закрепи сюда любые программы, папки, файлы или ссылки.</div>
                      <button className="pin-any-btn" onClick={() => setDrawerOpen(true)}>+ Закрепить элементы</button>
                    </div>
                  ) : (
                    <div className="sub-grid" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
                      {cs.items.map((it, i) => renderTile(it, i, cs.id, false))}
                    </div>
                  )}
                </div>
              </div>
            );
          })()}
        </div>

        {/* UNIVERSAL PINNING & ALL APPS DRAWER */}
        {drawerOpen && (
          <div className="all-apps-drawer">
            <div className="pin-any-bar">
              <span style={{ fontSize: 11, fontWeight: 600, color: 'var(--t2)', marginRight: 4 }}>ЗАКРЕПИТЬ В ПРОСТРАНСТВО:</span>
              <button className="pin-any-btn" onClick={() => handleUniversalPin('folder')}>+ Папку</button>
              <button className="pin-any-btn" onClick={() => handleUniversalPin('file')}>+ Файл</button>
              {urlPromptOpen ? (
                <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <input
                    className="url-input"
                    autoFocus
                    placeholder="https://example.com"
                    value={urlValue}
                    onChange={(e) => setUrlValue(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') submitUrlPin();
                      if (e.key === 'Escape') setUrlPromptOpen(false);
                    }}
                  />
                  <button className="pin-any-btn" onClick={submitUrlPin}>Добавить</button>
                </div>
              ) : (
                <button className="pin-any-btn" onClick={() => handleUniversalPin('url')}>+ Ссылку / Сайт</button>
              )}
              <button className="sec-act" style={{ marginLeft: 'auto', color: 'var(--t1)' }} onClick={() => setDrawerOpen(false)}>
                Закрыть · Esc
              </button>
            </div>

            <div className="cat-pills">
              {categories.map((c) => (
                <button key={c} className={`cat-pill ${drawerCat === c ? 'active' : ''}`} onClick={() => setDrawerCat(c)}>
                  {c}
                </button>
              ))}
            </div>

            <div className="drawer-grid">
              {filteredDrawerApps.map((a) => (
                <button
                  key={a.id}
                  className="drawer-app"
                  onClick={() => {
                    ipc('add_pin', {
                      space,
                      item: {
                        id: `app-${Date.now()}`,
                        kind: 'app',
                        name: a.name,
                        subtitle: a.category,
                        target: a.target,
                        altAction: 'От админа',
                        accentRgb: '129,140,248',
                        iconKey: a.name.toLowerCase(),
                        iconData: a.iconData ?? null,
                      },
                    });
                    setDrawerOpen(false);
                  }}
                >
                  {a.iconData ? <img src={a.iconData} alt="" className="native-ic" /> : <FallbackAppIcon iconKey={a.name} size={20} />}
                  <div style={{ minWidth: 0 }}>
                    <div style={{ fontWeight: 600, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{a.name}</div>
                    <div style={{ fontSize: 10.5, color: 'var(--t3)' }}>{a.category}</div>
                  </div>
                </button>
              ))}
            </div>
          </div>
        )}

        {/* SETTINGS DRAWER */}
        {settingsOpen && (
          <div className="settings-drawer">
            <div className="sec-row" style={{ marginBottom: 4 }}>
              <span className="sec-title">Настройки TestBar</span>
              <button className="sec-act" style={{ color: 'var(--t1)' }} onClick={() => setSettingsOpen(false)}>Закрыть · Esc</button>
            </div>
            <div className="set-row">
              <div>
                <div style={{ fontSize: 13.5, fontWeight: 600 }}>Стиль нижней панели</div>
                <div style={{ fontSize: 11.5, color: 'var(--t2)' }}>Плавающая капсула по центру или классическая полоса на всю ширину</div>
              </div>
              <div className="mini-tabs">
                <button
                  className={`mini-tab ${config?.barMode !== 'edge' ? 'active' : ''}`}
                  style={{ padding: '6px 12px', fontSize: 12 }}
                  onClick={() => ipc<TestBarConfig>('set_bar_mode', { mode: 'dock' }).then(setConfig)}
                >
                  Плавающий док
                </button>
                <button
                  className={`mini-tab ${config?.barMode === 'edge' ? 'active' : ''}`}
                  style={{ padding: '6px 12px', fontSize: 12 }}
                  onClick={() => ipc<TestBarConfig>('set_bar_mode', { mode: 'edge' }).then(setConfig)}
                >
                  На всю ширину
                </button>
              </div>
            </div>

            <div className="set-row">
              <div>
                <div style={{ fontSize: 13.5, fontWeight: 600 }}>Автозапуск с Windows</div>
                <div style={{ fontSize: 11.5, color: 'var(--t2)' }}>Запускать TestBar автоматически при входе в систему</div>
              </div>
              <button
                className="pin-any-btn"
                onClick={() => ipc<TestBarConfig>('set_autostart', { enable: !(config?.autostart ?? false) }).then(setConfig)}
              >
                {config?.autostart ? 'Включён' : 'Выключен'}
              </button>
            </div>

            <div className="set-row">
              <div>
                <div style={{ fontSize: 13.5, fontWeight: 600 }}>Аварийный возврат панели Windows</div>
                <div style={{ fontSize: 11.5, color: 'var(--t2)' }}>Также всегда работает глобальная комбинация Ctrl + Alt + Shift + Esc</div>
              </div>
              <button
                className="pin-any-btn"
                style={{ borderColor: 'rgba(244,63,94,0.45)', color: '#FDA4AF' }}
                onClick={() => ipc('restore_windows_taskbar_and_exit')}
              >
                Вернуть панель Windows и выйти
              </button>
            </div>
          </div>
        )}

        {/* TYPE-ANYWHERE COMMAND SEARCH */}
        {searchOpen && (
          <div className="search-backdrop" onClick={() => { setSearchOpen(false); setQuery(''); }}>
            <div className="cmd-palette" onClick={(e) => e.stopPropagation()}>
              <div className="cmd-top">
                <svg className="ic16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="2" strokeLinecap="round">
                  <circle cx="11" cy="11" r="7" />
                  <line x1="21" y1="21" x2="16.65" y2="16.65" />
                </svg>
                <input
                  ref={cmdInputRef}
                  className="cmd-input"
                  autoFocus
                  placeholder="Поиск программ, папок, файлов, игр…"
                  value={query}
                  onChange={(e) => {
                    setQuery(e.target.value);
                    setCmdIdx(0);
                  }}
                />
                {searchResults.swappedHint && (
                  <span className="cmd-layout-hint">Раскладка → «{searchResults.swappedHint}»</span>
                )}
                <span className="kbd">ESC</span>
              </div>
              <div className="cmd-list-wrap">
                {searchResults.items.length === 0 ? (
                  <div style={{ padding: 20, textAlign: 'center', fontSize: 12.5, color: 'var(--t3)' }}>Ничего не найдено</div>
                ) : (
                  searchResults.items.map((it, i) => (
                    <button
                      key={`${it.name}-${i}`}
                      className={`cmd-row ${i === cmdIdx ? 'selected' : ''}`}
                      onMouseEnter={() => setCmdIdx(i)}
                      onClick={() => {
                        ipc('launch_item', { target: it.target, asAdmin: false, closeStart: true });
                        setSearchOpen(false);
                        setQuery('');
                      }}
                    >
                      {it.iconData ? <img src={it.iconData} alt="" className="native-ic-sm" /> : <FallbackAppIcon iconKey={it.iconKey} kind={it.kind} size={18} />}
                      <span style={{ fontWeight: 500 }}>{it.name}</span>
                      <span className="cmd-cat">{it.cat}</span>
                    </button>
                  ))
                )}
              </div>
              <div className="cmd-foot">
                <span><span className="kbd">Enter</span> Открыть · <span className="kbd">Ctrl+Enter</span> От админа</span>
                <span><span className="kbd">↑</span> <span className="kbd">↓</span> Выбор</span>
              </div>
            </div>
          </div>
        )}

        {/* TILE RIGHT-CLICK MENU */}
        {tileCtx && (
          <div
            className="flyout-card ctx-card"
            style={{ position: 'fixed', left: tileCtx.x, top: tileCtx.y, width: 236, zIndex: 120 }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="ctx-sec">{tileCtx.item.name}</div>
            <button
              className="ctx-item"
              onClick={() => {
                ipc('launch_item', { target: tileCtx.item.target, asAdmin: false, closeStart: true });
                setTileCtx(null);
              }}
            >
              Открыть
            </button>
            {tileCtx.item.kind === 'app' && (
              <button
                className="ctx-item"
                onClick={() => {
                  ipc('launch_item', { target: tileCtx.item.target, asAdmin: true, closeStart: true });
                  setTileCtx(null);
                }}
              >
                От имени администратора
              </button>
            )}
            {tileCtx.item.target.includes(':\\') && (
              <button
                className="ctx-item"
                onClick={() => {
                  ipc('open_location', { path: tileCtx.item.target });
                  setTileCtx(null);
                }}
              >
                Расположение файла
              </button>
            )}
            {tileCtx.item.kind === 'app' && (
              <button
                className="ctx-item"
                onClick={() => {
                  ipc('add_pin', { space: 'taskbar', item: { ...tileCtx.item, id: `tb-${Date.now()}` } });
                  setTileCtx(null);
                }}
              >
                Закрепить на панели задач
              </button>
            )}
            <div className="ctx-sep" />
            <button
              className="ctx-item danger"
              onClick={() => {
                ipc('remove_pin', { space: tileCtx.spaceKey, itemId: tileCtx.item.id });
                setTileCtx(null);
              }}
            >
              <KillIcon />
              <span>Открепить</span>
            </button>
          </div>
        )}

        {/* FOOTER */}
        <footer className="start-footer">
          <div
            className="whisper-search"
            onClick={() => {
              setSearchOpen(true);
              setQuery('');
              requestAnimationFrame(() => cmdInputRef.current?.focus());
            }}
          >
            <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9">
              <circle cx="11" cy="11" r="7" />
              <line x1="21" y1="21" x2="16.65" y2="16.65" />
            </svg>
            <span>Начни печатать для поиска…</span>
          </div>
          <div className="footer-hints">
            <button className={`shift-chip ${shiftHeld ? 'on' : ''}`} onClick={() => setShiftHeld((v) => !v)}>
              <span className="kbd">Shift</span>
              <span>Второй слой</span>
            </button>
            <span><span className="kbd">Tab</span> Пространства</span>
          </div>
        </footer>
      </section>
    </div>
  );
};
