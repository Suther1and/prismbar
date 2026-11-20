import React, { useMemo, useState } from 'react';
import { getActiveDeviceIndex } from '../../lib/flyoutLogic';
import {
  ipc,
  type AudioState,
  type ClipboardEntry,
  type CustomSpace,
  type MediaState,
  type PinItem,
  type SteamGame,
  type TelemetryState,
  type TestBarConfig,
} from '../../lib/ipc';
import { partitionHeroAndCompact } from '../../lib/startLogic';
import {
  ExternalArrowIcon,
  FallbackAppIcon,
  FolderSvg,
  KillIcon,
  MicIcon,
  PlayTriangleIcon,
  VolIcon,
  XIcon10,
} from '../Icons';
import { SpotCard, trackTilePointer } from './SpotCard';

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

interface DragOrigin {
  spaceKey: string;
  idx: number;
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
  onSetAudio: (audio: AudioState) => void;
  onSetMedia: (media: MediaState) => void;
  onSetTelem: (telem: TelemetryState) => void;
  onSetClips: (clips: ClipboardEntry[]) => void;
  onShowToast: (msg: string) => void;
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
}) => {
  const [gameFilter, setGameFilter] = useState<'recent' | 'all'>('recent');
  const [copiedClipId, setCopiedClipId] = useState<string | null>(null);
  const [dragOrigin, setDragOrigin] = useState<DragOrigin | null>(null);

  const audioDevs = (audio?.devices ?? []).slice(0, 3);
  const personalPartition = useMemo(
    () => partitionHeroAndCompact(config?.personalPins ?? []),
    [config?.personalPins],
  );
  const studioPartition = useMemo(
    () => partitionHeroAndCompact(config?.studioPins ?? []),
    [config?.studioPins],
  );
  const displayedGames = gameFilter === 'recent' ? games.slice(0, 4) : games;

  const makeDragProps = (spaceKey: string, idx: number) => ({
    draggable: editMode,
    onDragStart: () => {
      if (editMode) setDragOrigin({ spaceKey, idx });
    },
    onDragOver: (e: React.DragEvent<HTMLElement>) => {
      if (editMode && dragOrigin?.spaceKey === spaceKey) {
        e.preventDefault();
        e.currentTarget.classList.add('drag-over');
      }
    },
    onDragLeave: (e: React.DragEvent<HTMLElement>) => {
      e.currentTarget.classList.remove('drag-over');
    },
    onDrop: (e: React.DragEvent<HTMLElement>) => {
      e.preventDefault();
      e.currentTarget.classList.remove('drag-over');
      if (editMode && dragOrigin && dragOrigin.spaceKey === spaceKey && dragOrigin.idx !== idx) {
        ipc('reorder_pins', { space: spaceKey, fromIdx: dragOrigin.idx, toIdx: idx }).catch(() => {});
      }
      setDragOrigin(null);
    },
    onDragEnd: () => setDragOrigin(null),
  });

  const renderTile = (item: PinItem, idx: number, spaceKey: string, isHero: boolean) => {
    const running = isRunning(item);
    const nativeIc = resolveIconForPin(item);
    return (
      <div
        key={item.id}
        className={`app-tile ${isHero ? 'hero' : 'compact'}`}
        style={{ ['--tile-rgb' as string]: item.accentRgb || '129,140,248' }}
        onMouseMove={trackTilePointer}
        {...makeDragProps(spaceKey, idx)}
        onClick={() => onLaunchPin(item)}
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
              ipc('remove_pin', { space: spaceKey, itemId: item.id }).catch(() => {});
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
          <div className="tile-name">{item.title}</div>
          <div className={`tile-sub ${item.altAction ? 'has-alt' : ''}`}>
            {shiftHeld && item.altAction ? item.altAction : item.subtitle}
          </div>
        </div>
      </div>
    );
  };

  const renderQuickLink = (item: PinItem, idx: number, spaceKey: string) => {
    const nativeIc = resolveIconForPin(item);
    return (
      <div
        key={item.id}
        role="button"
        tabIndex={0}
        className="qlink"
        {...makeDragProps(spaceKey, idx)}
        onClick={() => onLaunchPin(item)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
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
              ipc('remove_pin', { space: spaceKey, itemId: item.id }).catch(() => {});
            }}
          >
            <XIcon10 />
          </button>
        )}
        {nativeIc ? (
          <img src={nativeIc} alt="" className="native-ic-sm" draggable={false} />
        ) : (
          <FallbackAppIcon iconKey={item.iconKey} kind={item.kind} size={17} />
        )}
        <div className="qlink-meta">
          <div className="qlink-name">{item.title}</div>
          <div className={`qlink-sub ${shiftHeld && item.altAction ? 'alt-on' : ''}`}>
            {shiftHeld && item.altAction ? item.altAction : item.subtitle}
          </div>
        </div>
      </div>
    );
  };

  const renderAudioDeviceBar = () => {
    if (audioDevs.length === 0) return null;
    const activeIdx = getActiveDeviceIndex(audioDevs);
    return (
      <div
        className="seg-bar sliding"
        style={{
          gridTemplateColumns: `repeat(${audioDevs.length}, 1fr)`,
          ['--seg-count' as string]: audioDevs.length,
          ['--seg-idx' as string]: activeIdx,
        }}
      >
        <span className="seg-indicator" aria-hidden="true" />
        {audioDevs.map((d) => (
          <button
            key={d.id}
            type="button"
            className={`seg-opt ${d.isDefault ? 'active' : ''}`}
            onClick={() => ipc<AudioState>('set_audio_device', { deviceId: d.id }).then(onSetAudio).catch(() => {})}
          >
            {d.shortName}
          </button>
        ))}
      </div>
    );
  };

  const renderMixerRows = (showMasterFallback: boolean) => {
    const sessions = audio?.sessions ?? [];
    if (sessions.length === 0 && showMasterFallback) {
      return (
        <div className={`mix-row ${audio?.masterMuted ? 'muted' : ''}`}>
          <button
            type="button"
            className="mix-mute"
            onClick={() =>
              ipc<AudioState>('set_master_volume', {
                volume: audio?.masterVolume ?? 50,
                muted: !(audio?.masterMuted ?? false),
              })
                .then(onSetAudio)
                .catch(() => {})
            }
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
            onChange={(e) =>
              ipc<AudioState>('set_master_volume', {
                volume: Number(e.target.value),
                muted: false,
              })
                .then(onSetAudio)
                .catch(() => {})
            }
          />
          <span className="mix-val">{audio?.masterMuted ? '0%' : `${audio?.masterVolume ?? 0}%`}</span>
        </div>
      );
    }

    return sessions.map((s) => (
      <div key={s.id} className={`mix-row ${s.muted ? 'muted' : ''}`}>
        <button
          type="button"
          className="mix-mute"
          onClick={() =>
            ipc<AudioState>('set_session_volume', {
              pid: s.pid,
              pids: s.pids,
              appKey: s.id,
              volume: s.volume,
              muted: !s.muted,
            })
              .then(onSetAudio)
              .catch(() => {})
          }
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
          onChange={(e) =>
            ipc<AudioState>('set_session_volume', {
              pid: s.pid,
              pids: s.pids,
              appKey: s.id,
              volume: Number(e.target.value),
              muted: false,
            })
              .then(onSetAudio)
              .catch(() => {})
          }
        />
        <span className="mix-val">{s.muted ? '0%' : `${s.volume}%`}</span>
      </div>
    ));
  };

  if (space === 'personal') {
    return (
      <div className="space-panel">
        <div className="bento-col">
          <SpotCard className="space-card-flex">
            <div className="sec-row">
              <span className="sec-title">Закреплено</span>
              <button type="button" className="sec-act" onClick={onOpenDrawer}>
                + Закрепить
              </button>
            </div>

            <div className="space-scroll-main">
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
            </div>

            <div className="space-dock-foot">
              <div className="sec-row" style={{ marginBottom: 8 }}>
                <span className="sec-title">Быстрый доступ</span>
                <button type="button" className="sec-act" onClick={onOpenDrawer}>
                  + Папка / Файл / URL
                </button>
              </div>
              <div className="qlinks-grid qlinks-scroll">
                {(config?.personalLinks ?? []).map((l, idx) =>
                  renderQuickLink(l, idx, 'personal_links'),
                )}
              </div>
            </div>
          </SpotCard>
        </div>

        <div className="bento-col">
          <SpotCard className={`media-card ${!media?.isPlaying ? 'paused' : ''}`}>
            <div className="media-top">
              <div className="album-box">
                <div className="eq-wave"><i /><i /><i /><i /></div>
              </div>
              <div className="media-meta">
                <div className="media-title">{media?.title || 'Ничего не воспроизводится'}</div>
                <div className="media-sub">
                  {media?.hasSession
                    ? [media.artist, media.source].filter(Boolean).join(' · ')
                    : 'Запусти музыку или видео'}
                </div>
              </div>
              <div className="m-ctrls">
                <button
                  type="button"
                  className="m-btn"
                  aria-label="Предыдущий трек"
                  onClick={() => ipc<MediaState>('media_action', { action: 'prev' }).then(onSetMedia).catch(() => {})}
                >
                  <svg className="ic14" viewBox="0 0 24 24" fill="currentColor">
                    <polygon points="19 4 9 12 19 20 19 4" />
                    <rect x="5" y="5" width="2.4" height="14" rx="1" />
                  </svg>
                </button>
                <button
                  type="button"
                  className="m-btn play"
                  aria-label={media?.isPlaying ? 'Пауза' : 'Воспроизвести'}
                  onClick={() => ipc<MediaState>('media_action', { action: 'toggle' }).then(onSetMedia).catch(() => {})}
                >
                  {media?.isPlaying ? (
                    <svg className="ic14" viewBox="0 0 24 24" fill="currentColor">
                      <rect x="6" y="4" width="4" height="16" rx="1" />
                      <rect x="14" y="4" width="4" height="16" rx="1" />
                    </svg>
                  ) : (
                    <PlayTriangleIcon className="ic14" size={14} />
                  )}
                </button>
                <button
                  type="button"
                  className="m-btn"
                  aria-label="Следующий трек"
                  onClick={() => ipc<MediaState>('media_action', { action: 'next' }).then(onSetMedia).catch(() => {})}
                >
                  <svg className="ic14" viewBox="0 0 24 24" fill="currentColor">
                    <polygon points="5 4 15 12 5 20 5 4" />
                    <rect x="17" y="5" width="2.4" height="14" rx="1" />
                  </svg>
                </button>
              </div>
            </div>
            <div className="scrub-wrap">
              <span className="scrub-time">{fmtSec(media?.positionSec ?? 0)}</span>
              <div className="scrub-bar">
                <div
                  className="scrub-fill"
                  style={{
                    width:
                      media && media.durationSec > 0
                        ? `${Math.min(100, (media.positionSec / media.durationSec) * 100)}%`
                        : '0%',
                  }}
                />
              </div>
              <span className="scrub-time">{fmtSec(media?.durationSec ?? 0)}</span>
            </div>
          </SpotCard>

          <SpotCard className="compact-card">
            <div className="sec-row" style={{ marginBottom: 8 }}>
              <span className="sec-title">Звук и микшер</span>
            </div>
            {renderAudioDeviceBar()}
            <div className="mixer-list mixer-scroll">{renderMixerRows(true)}</div>
          </SpotCard>

          <SpotCard className="clip-card">
            <div className="sec-row" style={{ marginBottom: 8 }}>
              <span className="sec-title">Буфер обмена</span>
              <button
                type="button"
                className="sec-act"
                onClick={() => ipc<ClipboardEntry[]>('clear_unpinned_clips').then(onSetClips).catch(() => {})}
              >
                Очистить
              </button>
            </div>
            <div className="clip-list">
              {clips.length === 0 ? (
                <div className="clip-empty">Буфер обмена пока пуст</div>
              ) : (
                clips.map((c) => (
                  <div
                    key={c.id}
                    className={`clip-item ${copiedClipId === c.id ? 'copied' : ''}`}
                    onClick={() => {
                      ipc('copy_to_clipboard', { text: c.text }).catch(() => {});
                      setCopiedClipId(c.id);
                      setTimeout(() => setCopiedClipId(null), 900);
                    }}
                  >
                    {c.color ? (
                      <span className="clip-swatch" style={{ background: c.color }} />
                    ) : (
                      <span className="clip-tag">{c.tag}</span>
                    )}
                    <span className="clip-txt">{c.text}</span>
                    <button
                      type="button"
                      className={`clip-pin ${c.pinned ? 'pinned' : ''}`}
                      aria-label={c.pinned ? 'Открепить запись' : 'Закрепить запись'}
                      onClick={(e) => {
                        e.stopPropagation();
                        ipc<ClipboardEntry[]>('toggle_clip_pin', { id: c.id }).then(onSetClips).catch(() => {});
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
          </SpotCard>
        </div>
      </div>
    );
  }

  if (space === 'work') {
    return (
      <div className="space-panel">
        <div className="bento-col">
          <SpotCard className="space-card-flex">
            <div className="sec-row">
              <span className="sec-title">Проекты, папки и файлы</span>
              <button type="button" className="sec-act" onClick={onOpenDrawer}>
                + Закрепить
              </button>
            </div>

            <div className="space-scroll-main">
              <div className="folder-hub-grid">
                {(config?.workFolders ?? []).map((f, idx) => (
                  <div
                    key={f.id}
                    className="folder-card"
                    {...makeDragProps('work_folders', idx)}
                    onClick={() => onLaunchPin(f)}
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
                          ipc('remove_pin', { space: 'work_folders', itemId: f.id }).catch(() => {});
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
                      <div className={`tile-sub ${f.altAction ? 'has-alt' : ''}`}>
                        {shiftHeld && f.altAction ? f.altAction : f.subtitle}
                      </div>
                    </div>
                    {!editMode && (
                      <div className="folder-acts">
                        <button
                          type="button"
                          className="f-mini"
                          title="Открыть"
                          onClick={(e) => {
                            e.stopPropagation();
                            ipc('launch_item', { target: f.target, asAdmin: false, closeStart: true }).catch(() => {});
                          }}
                        >
                          Открыть
                        </button>
                        <button
                          type="button"
                          className="f-mini"
                          title="Открыть в терминале"
                          onClick={(e) => {
                            e.stopPropagation();
                            ipc('open_terminal_at', { path: f.target }).catch(() => {});
                          }}
                        >
                          Терминал
                        </button>
                        <button
                          type="button"
                          className="f-mini"
                          title="Скопировать путь"
                          onClick={(e) => {
                            e.stopPropagation();
                            ipc('copy_to_clipboard', { text: f.target })
                              .then(() => onShowToast('Путь скопирован'))
                              .catch(() => {});
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

            <div className="space-dock-foot">
              <div className="sec-row" style={{ marginBottom: 8 }}>
                <span className="sec-title">Рабочие программы</span>
                <button type="button" className="sec-act" onClick={onOpenDrawer}>
                  + Добавить
                </button>
              </div>
              <div className="qlinks-grid qlinks-scroll">
                {(config?.workApps ?? []).map((a, idx) => renderQuickLink(a, idx, 'work_apps'))}
              </div>
            </div>
          </SpotCard>
        </div>

        <div className="bento-col">
          <SpotCard className="compact-card">
            <div className="sec-row" style={{ marginBottom: 9 }}>
              <span className="sec-title">Нагрузка ПК</span>
              <span className="health-hw-meta">
                {telem ? `${telem.ramTotalGb} ГБ${telem.gpuAvailable ? ` · ${telem.gpuName}` : ''}` : 'Система'}
              </span>
            </div>
            <div className="health-list">
              <div className="health-row">
                <div className="health-top">
                  <span className="health-lbl">
                    Процессор <span className="health-sub">· {telem?.cpuName || 'CPU'}</span>
                  </span>
                  <span className="health-val">
                    {telem?.cpuPct ?? 0}%{' '}
                    <span className={`status-pill ${statusPill(telem?.cpuPct ?? 0).cls}`}>
                      {statusPill(telem?.cpuPct ?? 0).label}
                    </span>
                  </span>
                </div>
                <div className="health-bar">
                  <div className="health-fill" style={{ width: `${telem?.cpuPct ?? 0}%`, background: '#34D399' }} />
                </div>
              </div>

              <div className="health-row">
                <div className="health-top">
                  <span className="health-lbl">
                    ОЗУ <span className="health-sub">· RAM</span>
                  </span>
                  <span className="health-val">
                    {telem ? `${telem.ramUsedGb} / ${telem.ramTotalGb} ГБ` : '—'}{' '}
                    <span className={`status-pill ${statusPill(telem?.ramPct ?? 0).cls}`}>
                      {statusPill(telem?.ramPct ?? 0).label}
                    </span>
                  </span>
                </div>
                <div className="health-bar">
                  <div className="health-fill" style={{ width: `${telem?.ramPct ?? 0}%`, background: '#38BDF8' }} />
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
                        {telem.gpuPct}%{' '}
                        <span className={`status-pill ${tempStatus(telem.gpuTempC).cls}`}>
                          {tempStatus(telem.gpuTempC).label}
                        </span>
                      </span>
                    </div>
                    <div className="health-bar">
                      <div className="health-fill" style={{ width: `${telem.gpuPct}%`, background: '#818CF8' }} />
                    </div>
                  </div>

                  <div className="health-row">
                    <div className="health-top">
                      <span className="health-lbl">
                        Видеопамять <span className="health-sub">· VRAM</span>
                      </span>
                      <span className="health-val">
                        {telem.vramUsedGb} / {telem.vramTotalGb} ГБ{' '}
                        <span className={`status-pill ${statusPill(telem.vramPct).cls}`}>
                          {statusPill(telem.vramPct).label}
                        </span>
                      </span>
                    </div>
                    <div className="health-bar">
                      <div className="health-fill" style={{ width: `${telem.vramPct}%`, background: '#F59E0B' }} />
                    </div>
                  </div>
                </>
              )}
            </div>
          </SpotCard>

          <SpotCard className="consumer-card">
            <div className="sec-row" style={{ marginBottom: 8 }}>
              <span className="sec-title">Кто сейчас ест память</span>
              <button
                type="button"
                className="sec-act sec-act-ic"
                onClick={() =>
                  ipc('launch_item', {
                    target: 'C:\\Windows\\System32\\Taskmgr.exe',
                    asAdmin: false,
                    closeStart: true,
                  }).catch(() => {})
                }
              >
                <span>Диспетчер</span>
                <ExternalArrowIcon />
              </button>
            </div>
            <div className="consumer-list">
              {(telem?.topProcesses ?? []).map((pr) => (
                <div key={pr.pid} className="consumer-row">
                  {pr.iconData ? (
                    <img src={pr.iconData} alt="" className="native-ic-sm" draggable={false} />
                  ) : (
                    <FallbackAppIcon iconKey={pr.name} size={18} />
                  )}
                  <div className="consumer-meta">
                    <div className="consumer-name">{pr.name}</div>
                    <div className="consumer-sub">{pr.cpuPct}% CPU</div>
                  </div>
                  <span className="consumer-mem">{pr.memGb} ГБ</span>
                  <button
                    type="button"
                    className="consumer-kill"
                    title="Завершить процесс"
                    aria-label={`Завершить ${pr.name}`}
                    onClick={() =>
                      ipc('kill_process_pid', { pid: pr.pid })
                        .then(() => ipc<TelemetryState>('get_telemetry').then(onSetTelem))
                        .catch(() => {})
                    }
                  >
                    <KillIcon />
                  </button>
                </div>
              ))}
            </div>
          </SpotCard>
        </div>
      </div>
    );
  }

  if (space === 'studio') {
    return (
      <div className="space-panel">
        <div className="bento-col">
          <SpotCard className="space-card-flex">
            <div className="sec-row">
              <span className="sec-title">Творчество и AI</span>
              <button type="button" className="sec-act" onClick={onOpenDrawer}>
                + Закрепить
              </button>
            </div>

            <div className="space-scroll-main">
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
            </div>

            <div className="space-dock-foot">
              <div className="sec-row" style={{ marginBottom: 8 }}>
                <span className="sec-title">Инструменты</span>
                <button type="button" className="sec-act" onClick={onOpenDrawer}>
                  + Добавить
                </button>
              </div>
              <div className="qlinks-grid qlinks-scroll">
                {(config?.studioTools ?? []).map((t, idx) => renderQuickLink(t, idx, 'studio_tools'))}
              </div>
            </div>
          </SpotCard>
        </div>

        <div className="bento-col">
          <SpotCard className="compact-card">
            <div className="sec-row" style={{ marginBottom: 9 }}>
              <span className="sec-title">Запас памяти</span>
              <span className={`status-pill ${statusPill(telem?.ramPct ?? 0).cls}`}>
                {statusPill(telem?.ramPct ?? 0).label}
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
                      {telem.vramUsedGb} / {telem.vramTotalGb} ГБ{' '}
                      <span className="status-pill st-mid">{telem.vramPct}%</span>
                    </span>
                  </div>
                  <div className="health-bar">
                    <div className="health-fill" style={{ width: `${telem.vramPct}%`, background: '#34D399' }} />
                  </div>
                </div>
              )}
              <div className="health-row">
                <div className="health-top">
                  <span className="health-lbl">
                    ОЗУ <span className="health-sub">· RAM</span>
                  </span>
                  <span className="health-val">
                    {telem ? `${telem.ramUsedGb} / ${telem.ramTotalGb} ГБ` : '—'}{' '}
                    <span className="status-pill st-ok">{telem?.ramPct ?? 0}%</span>
                  </span>
                </div>
                <div className="health-bar">
                  <div className="health-fill" style={{ width: `${telem?.ramPct ?? 0}%`, background: '#38BDF8' }} />
                </div>
              </div>
            </div>
          </SpotCard>

          <SpotCard className="compact-card">
            <div className="sec-row" style={{ marginBottom: 8 }}>
              <span className="sec-title">Аудио и микрофон</span>
            </div>
            {renderAudioDeviceBar()}
            <button
              type="button"
              className={`sys-tgl ${!(audio?.micMuted ?? false) ? 'on' : ''}`}
              style={{ width: '100%' }}
              onClick={() =>
                ipc<AudioState>('set_mic_muted', { muted: !(audio?.micMuted ?? false) })
                  .then(onSetAudio)
                  .catch(() => {})
              }
            >
              <MicIcon className="ic16" muted={audio?.micMuted ?? false} />
              <div>
                <div className="sys-tgl-t">Микрофон</div>
                <div className="sys-tgl-s">{audio?.micMuted ? 'Замьючен' : 'Активен'}</div>
              </div>
              <span className="vu-bars"><i /><i /><i /></span>
            </button>
          </SpotCard>

          <SpotCard className="consumer-card">
            <div className="sec-row" style={{ marginBottom: 8 }}>
              <span className="sec-title">Активные процессы GPU / ОЗУ</span>
            </div>
            <div className="consumer-list">
              {(telem?.topProcesses ?? []).map((pr) => (
                <div key={pr.pid} className="consumer-row">
                  {pr.iconData ? (
                    <img src={pr.iconData} alt="" className="native-ic-sm" draggable={false} />
                  ) : (
                    <FallbackAppIcon iconKey={pr.name} size={18} />
                  )}
                  <div className="consumer-meta">
                    <div className="consumer-name">{pr.name}</div>
                    <div className="consumer-sub">{pr.cpuPct}% CPU</div>
                  </div>
                  <span className="consumer-mem">{pr.memGb} ГБ</span>
                </div>
              ))}
            </div>
          </SpotCard>
        </div>
      </div>
    );
  }

  if (space === 'games') {
    return (
      <div className="space-panel">
        <div className="bento-col">
          <SpotCard className="space-card-flex">
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
              <button type="button" className="sec-act" onClick={onOpenDrawer}>
                + Добавить
              </button>
            </div>

            <div className="games-scroll">
              {displayedGames.length === 0 ? (
                <div className="games-empty">
                  Библиотека Steam сканируется или игры не найдены
                </div>
              ) : (
                <div className="games-grid">
                  {displayedGames.map((g) => (
                    <div
                      key={g.appId}
                      className="game-card"
                      onMouseMove={(e) => {
                        const r = e.currentTarget.getBoundingClientRect();
                        const x = (e.clientX - r.left) / r.width;
                        const y = (e.clientY - r.top) / r.height;
                        e.currentTarget.style.setProperty('--gx', `${(x * 100).toFixed(1)}%`);
                        e.currentTarget.style.setProperty('--gy', `${(y * 100).toFixed(1)}%`);
                        e.currentTarget.style.setProperty('--rx', `${((0.5 - y) * 6).toFixed(2)}deg`);
                        e.currentTarget.style.setProperty('--ry', `${((x - 0.5) * 6).toFixed(2)}deg`);
                      }}
                      onMouseLeave={(e) => {
                        e.currentTarget.style.setProperty('--rx', '0deg');
                        e.currentTarget.style.setProperty('--ry', '0deg');
                      }}
                      onClick={() =>
                        ipc('launch_item', { target: g.target, asAdmin: false, closeStart: true }).catch(() => {})
                      }
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
                  ))}
                </div>
              )}
            </div>

            <div className="space-dock-foot">
              <div className="sec-row" style={{ marginBottom: 7 }}>
                <span className="sec-title">Лаунчеры и компаньоны</span>
              </div>
              <div className="qlinks-grid qlinks-scroll">
                {(config?.gameLaunchers ?? []).map((l, idx) =>
                  renderQuickLink(l, idx, 'game_launchers'),
                )}
              </div>
            </div>
          </SpotCard>
        </div>

        <div className="bento-col">
          <SpotCard className="compact-card">
            <div className="sec-row" style={{ marginBottom: 8 }}>
              <span className="sec-title">Игровой звук и голос</span>
            </div>
            <button
              type="button"
              className={`sys-tgl ${!(audio?.micMuted ?? false) ? 'on' : ''}`}
              style={{ width: '100%', marginBottom: 9 }}
              onClick={() =>
                ipc<AudioState>('set_mic_muted', { muted: !(audio?.micMuted ?? false) })
                  .then(onSetAudio)
                  .catch(() => {})
              }
            >
              <MicIcon className="ic16" muted={audio?.micMuted ?? false} />
              <div>
                <div className="sys-tgl-t">Микрофон</div>
                <div className="sys-tgl-s">{audio?.micMuted ? 'Замьючен' : 'Активен'}</div>
              </div>
              <span className="vu-bars"><i /><i /><i /></span>
            </button>
            <div className="mixer-list mixer-scroll">{renderMixerRows(false)}</div>
          </SpotCard>

          <SpotCard className="combos-card">
            <div className="sec-row" style={{ marginBottom: 8 }}>
              <span className="sec-title">Игровые связки</span>
            </div>
            <div className="combos-list">
              {(config?.gameCombos ?? []).map((cb, idx) => (
                <div
                  key={cb.id}
                  role="button"
                  tabIndex={0}
                  className="consumer-row combo-row"
                  {...makeDragProps('game_combos', idx)}
                  onClick={() => onLaunchPin(cb)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      onLaunchPin(cb);
                    }
                  }}
                  onContextMenu={(e) => onOpenTileContext(e, cb, 'game_combos')}
                >
                  {editMode && (
                    <button
                      type="button"
                      className="tile-del tile-del-sm"
                      title="Удалить связку"
                      aria-label={`Удалить ${cb.title}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        ipc('remove_pin', { space: 'game_combos', itemId: cb.id }).catch(() => {});
                      }}
                    >
                      <XIcon10 />
                    </button>
                  )}
                  <span
                    className="combo-ic"
                    style={{
                      background: `rgba(${cb.accentRgb || '251,113,133'}, 0.16)`,
                      color: `rgb(${cb.accentRgb || '251,113,133'})`,
                    }}
                  >
                    <PlayTriangleIcon size={12} />
                  </span>
                  <div className="consumer-meta">
                    <div className="consumer-name">{cb.title}</div>
                    <div className="consumer-sub">{cb.subtitle}</div>
                  </div>
                </div>
              ))}
            </div>

            {telem?.gpuAvailable && (
              <div className="health-row" style={{ marginTop: 'auto', flexShrink: 0 }}>
                <div className="health-top" style={{ marginBottom: 5 }}>
                  <span className="health-lbl">
                    {telem.gpuName} <span className="health-sub">· {telem.gpuTempC}°C</span>
                  </span>
                  <span className="health-val">
                    {telem.vramUsedGb} / {telem.vramTotalGb} ГБ{' '}
                    <span className={`status-pill ${tempStatus(telem.gpuTempC).cls}`}>
                      {tempStatus(telem.gpuTempC).label}
                    </span>
                  </span>
                </div>
                <div className="health-bar">
                  <div className="health-fill" style={{ width: `${telem.vramPct}%`, background: '#34D399' }} />
                </div>
              </div>
            )}
          </SpotCard>
        </div>
      </div>
    );
  }

  if (activeCustomSpace) {
    return (
      <div className="space-panel space-panel-single">
        <SpotCard className="space-card-flex">
          <div className="sec-row">
            <span className="sec-title">{activeCustomSpace.name}</span>
            <button type="button" className="sec-act" onClick={onOpenDrawer}>
              + Закрепить
            </button>
          </div>
          {activeCustomSpace.items.length === 0 ? (
            <div className="custom-empty">
              <div className="custom-empty-title">Пока пусто</div>
              <div className="custom-empty-sub">
                Закрепи сюда любые программы, папки, файлы или ссылки.
              </div>
              <button type="button" className="pin-any-btn primary" onClick={onOpenDrawer}>
                + Закрепить элементы
              </button>
            </div>
          ) : (
            <div className="space-scroll-main">
              <div className="sub-grid custom-grid-4">
                {activeCustomSpace.items.map((it, i) =>
                  renderTile(it, i, activeCustomSpace.id, false),
                )}
              </div>
            </div>
          )}
        </SpotCard>
      </div>
    );
  }

  return null;
};
