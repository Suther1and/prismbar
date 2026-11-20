import React, { useEffect, useState } from 'react';
import {
  ipc,
  onEvent,
  type AppWindowGroup,
  type AudioState,
  type FlyoutRequest,
  type NetworkInfo,
  type OpenWindowInfo,
  type TrayAppItem,
} from '../lib/ipc';
import { FallbackAppIcon, KillIcon, VolIcon, XIcon10 } from './Icons';

const MONTHS = [
  'Январь', 'Февраль', 'Март', 'Апрель', 'Май', 'Июнь',
  'Июль', 'Август', 'Сентябрь', 'Октябрь', 'Ноябрь', 'Декабрь',
];

export const FlyoutView: React.FC = () => {
  const [req, setReq] = useState<FlyoutRequest | null>(null);
  const [trayApps, setTrayApps] = useState<TrayAppItem[]>([]);
  const [trayHover, setTrayHover] = useState<string>('');
  const [trayCtxItem, setTrayCtxItem] = useState<TrayAppItem | null>(null);
  const [audio, setAudio] = useState<AudioState | null>(null);
  const [net, setNet] = useState<NetworkInfo | null>(null);
  const [thumbs, setThumbs] = useState<Record<number, string>>({});
  const [calDate, setCalDate] = useState(() => new Date());

  useEffect(() => {
    const subs = [
      onEvent<FlyoutRequest>('flyout-show', (r) => {
        setReq(r);
        setTrayCtxItem(null);
        setTrayHover('');
        if (r.kind === 'tray') {
          ipc<TrayAppItem[]>('get_tray_apps').then(setTrayApps).catch(() => {});
        } else if (r.kind === 'sound' || r.kind === 'quick') {
          ipc<AudioState>('get_audio_state').then(setAudio).catch(() => {});
          ipc<NetworkInfo>('get_network_info').then(setNet).catch(() => {});
        } else if (r.kind === 'preview') {
          const payload = r.payload as { group?: AppWindowGroup };
          setThumbs({});
          (payload.group?.windows ?? []).slice(0, 4).forEach((w) => {
            ipc<string | null>('get_window_thumbnail', { hwnd: w.hwnd })
              .then((img) => {
                if (img) setThumbs((prev) => ({ ...prev, [w.hwnd]: img }));
              })
              .catch(() => {});
          });
        }
      }),
      onEvent<void>('flyout-hide', () => {
        setReq(null);
        setTrayCtxItem(null);
      }),
      onEvent<AudioState>('audio-updated', setAudio),
    ];

    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') ipc('close_flyout').catch(() => {});
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  if (!req) return null;

  const close = () => ipc('close_flyout').catch(() => {});

  // 1. COMPACT 40-ICON TRAY OVERFLOW (^)
  if (req.kind === 'tray') {
    if (trayCtxItem) {
      return (
        <div className="flyout-root" data-space="personal">
          <div className="flyout-card ctx-card">
            <div className="ctx-sec">{trayCtxItem.name}</div>
            <button
              className="ctx-item"
              onClick={() => {
                ipc('launch_item', { target: trayCtxItem.exePath, asAdmin: false, closeStart: false });
                close();
              }}
            >
              Открыть
            </button>
            <button
              className="ctx-item"
              onClick={() => {
                ipc('open_location', { path: trayCtxItem.exePath });
                close();
              }}
            >
              Расположение файла
            </button>
            <div className="ctx-sep" />
            <button
              className="ctx-item danger"
              onClick={() => {
                ipc('kill_process_pid', { pid: trayCtxItem.pid }).then(() => {
                  setTrayApps((prev) => prev.filter((x) => x.pid !== trayCtxItem.pid));
                  setTrayCtxItem(null);
                });
              }}
            >
              <KillIcon />
              <span>Завершить задачу</span>
            </button>
            <button className="ctx-item" onClick={() => setTrayCtxItem(null)}>
              ← Назад к иконкам
            </button>
          </div>
        </div>
      );
    }

    return (
      <div className="flyout-root" data-space="personal">
        <div className="flyout-card tray-card">
          {trayHover && <div className="tray-tip">{trayHover}</div>}
          <div className="tray-grid">
            {trayApps.map((t) => (
              <button
                key={t.pid}
                className="tray-cell"
                onMouseEnter={() => setTrayHover(t.name)}
                onMouseLeave={() => setTrayHover('')}
                onClick={() => {
                  ipc('launch_item', { target: t.exePath, asAdmin: false, closeStart: false });
                  close();
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setTrayCtxItem(t);
                }}
              >
                {t.iconData ? (
                  <img src={t.iconData} alt="" className="native-ic-sm" draggable={false} />
                ) : (
                  <FallbackAppIcon iconKey={t.name} size={18} />
                )}
              </button>
            ))}
          </div>
        </div>
      </div>
    );
  }

  // 2. QUICK SOUND & MIXER
  if (req.kind === 'sound' || req.kind === 'quick') {
    const devs = (audio?.devices ?? []).slice(0, 3);
    const sessions = (audio?.sessions ?? []).slice(0, 4);
    return (
      <div className="flyout-root" data-space="personal">
        <div className="flyout-card qs-card">
          <div className="sec-row" style={{ marginBottom: 8 }}>
            <span className="sec-title">Звук и сеть</span>
            <span style={{ fontSize: 10.5, color: net?.connected ? '#34D399' : '#FB7185' }}>
              {net ? `${net.kind} · ${net.speedLabel}` : 'Сеть'}
            </span>
          </div>

          {devs.length > 0 && (
            <div className="seg-bar" style={{ gridTemplateColumns: `repeat(${devs.length}, 1fr)`, marginBottom: 10 }}>
              {devs.map((d) => (
                <button
                  key={d.id}
                  className={`seg-opt ${d.isDefault ? 'active' : ''}`}
                  title={d.name}
                  onClick={() => ipc<AudioState>('set_audio_device', { deviceId: d.id }).then(setAudio)}
                >
                  {d.shortName}
                </button>
              ))}
            </div>
          )}

          <div
            className={`mix-row ${audio?.masterMuted ? 'muted' : ''}`}
            style={{
              marginBottom: 10,
              background: 'rgba(var(--accent-rgb), 0.1)',
              borderColor: 'rgba(var(--accent-rgb), 0.3)',
            }}
          >
            <button
              className="mix-mute"
              onClick={() =>
                ipc<AudioState>('set_master_volume', {
                  volume: audio?.masterVolume ?? 50,
                  muted: !(audio?.masterMuted ?? false),
                }).then(setAudio)
              }
            >
              <VolIcon muted={audio?.masterMuted} />
            </button>
            <span className="mix-name" style={{ fontWeight: 600 }}>Общий звук</span>
            <input
              type="range"
              className="mix-range"
              min={0}
              max={100}
              value={audio?.masterMuted ? 0 : (audio?.masterVolume ?? 50)}
              onChange={(e) => {
                const v = Number(e.target.value);
                setAudio((prev) => (prev ? { ...prev, masterVolume: v, masterMuted: false } : prev));
                ipc<AudioState>('set_master_volume', { volume: v, muted: false }).then(setAudio);
              }}
            />
            <span className="mix-val">{audio?.masterMuted ? '0%' : `${audio?.masterVolume ?? 0}%`}</span>
          </div>

          <div className="mixer-list">
            {sessions.length === 0 ? (
              <div style={{ fontSize: 11.5, color: 'var(--t3)', padding: '6px 4px' }}>Нет активных аудиопотоков</div>
            ) : (
              sessions.map((s) => (
                <div key={s.pid} className={`mix-row ${s.muted ? 'muted' : ''}`}>
                  <button
                    className="mix-mute"
                    onClick={() =>
                      ipc<AudioState>('set_session_volume', { pid: s.pid, volume: s.volume, muted: !s.muted }).then(setAudio)
                    }
                  >
                    <VolIcon muted={s.muted} />
                  </button>
                  <span className="mix-name" title={s.name}>{s.name}</span>
                  <input
                    type="range"
                    className="mix-range"
                    min={0}
                    max={100}
                    value={s.muted ? 0 : s.volume}
                    onChange={(e) => {
                      const v = Number(e.target.value);
                      ipc<AudioState>('set_session_volume', { pid: s.pid, volume: v, muted: false }).then(setAudio);
                    }}
                  />
                  <span className="mix-val">{s.muted ? '0%' : `${s.volume}%`}</span>
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    );
  }

  // 3. CALENDAR
  if (req.kind === 'calendar') {
    const today = new Date();
    const y = calDate.getFullYear();
    const m = calDate.getMonth();
    const first = new Date(y, m, 1);
    const offset = (first.getDay() + 6) % 7;
    const daysInMonth = new Date(y, m + 1, 0).getDate();
    const prevMonthDays = new Date(y, m, 0).getDate();
    const cells: { d: number; muted: boolean; isToday: boolean }[] = [];
    for (let i = offset - 1; i >= 0; i--) {
      cells.push({ d: prevMonthDays - i, muted: true, isToday: false });
    }
    for (let d = 1; d <= daysInMonth; d++) {
      cells.push({
        d,
        muted: false,
        isToday: y === today.getFullYear() && m === today.getMonth() && d === today.getDate(),
      });
    }

    return (
      <div className="flyout-root" data-space="personal">
        <div className="flyout-card cal-card">
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <strong style={{ fontSize: 13.5 }}>{MONTHS[m]} {y}</strong>
            <div style={{ display: 'flex', gap: 4 }}>
              <button
                className="hdr-btn"
                style={{ width: 26, height: 26 }}
                onClick={() => setCalDate(new Date(y, m - 1, 1))}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="15 18 9 12 15 6" />
                </svg>
              </button>
              <button
                className="hdr-btn"
                style={{ width: 26, height: 26 }}
                onClick={() => setCalDate(new Date(y, m + 1, 1))}
              >
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="9 18 15 12 9 6" />
                </svg>
              </button>
            </div>
          </div>

          <div className="cal-grid">
            {['Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб', 'Вс'].map((wd) => (
              <div key={wd} className="cal-wd">{wd}</div>
            ))}
            {cells.map((c, idx) => (
              <div
                key={idx}
                className={`cal-day ${c.muted ? 'muted' : ''} ${c.isToday ? 'today' : ''}`}
                onClick={() => setCalDate(new Date())}
              >
                {c.d}
              </div>
            ))}
          </div>
        </div>
      </div>
    );
  }

  // 4. HOVER WINDOW PREVIEW
  if (req.kind === 'preview') {
    const payload = req.payload as { group?: AppWindowGroup };
    const wins = payload.group?.windows ?? [];
    return (
      <div className="flyout-root" data-space="personal" onMouseLeave={close}>
        <div className="flyout-card preview-card">
          {wins.slice(0, 4).map((w) => (
            <div
              key={w.hwnd}
              className="wp-item"
              onClick={() => ipc('focus_window', { hwnd: w.hwnd, toggleMinimize: false })}
            >
              <div className="wp-head">
                <span style={{ fontSize: 11.5, fontWeight: 600, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {w.title}
                </span>
                <button
                  className="wp-close"
                  onClick={(e) => {
                    e.stopPropagation();
                    ipc('close_window_hwnd', { hwnd: w.hwnd });
                    close();
                  }}
                >
                  <XIcon10 />
                </button>
              </div>
              <div className="wp-thumb">
                {thumbs[w.hwnd] ? (
                  <img src={thumbs[w.hwnd]} alt="" draggable={false} />
                ) : (
                  <span style={{ fontSize: 11, color: 'var(--t3)' }}>
                    {w.isMinimized ? 'Свёрнуто' : w.exeName}
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>
    );
  }

  // 5. RIGHT-CLICK CONTEXT MENU ON TASKBAR APP (NATIVE-GRADE + END TASK)
  if (req.kind === 'context_app' || req.kind === 'ctx_app') {
    const p = req.payload as {
      id: string;
      pinId?: string;
      name: string;
      target: string;
      pinned: boolean;
      windows: OpenWindowInfo[];
    };
    const pid = p.windows[0]?.pid;
    return (
      <div className="flyout-root" data-space="personal">
        <div className="flyout-card ctx-card">
          <div className="ctx-sec">{p.name}</div>
          {p.target && (
            <>
              <button
                className="ctx-item"
                onClick={() => {
                  ipc('launch_item', { target: p.target, asAdmin: false, closeStart: false });
                  close();
                }}
              >
                Открыть новое окно
              </button>
              <button
                className="ctx-item"
                onClick={() => {
                  ipc('launch_item', { target: p.target, asAdmin: true, closeStart: false });
                  close();
                }}
              >
                Запустить от имени администратора
              </button>
              <button
                className="ctx-item"
                onClick={() => {
                  ipc('open_location', { path: p.target });
                  close();
                }}
              >
                Расположение файла
              </button>
            </>
          )}
          <div className="ctx-sep" />
          {p.pinned && p.pinId ? (
            <button
              className="ctx-item"
              onClick={() => {
                ipc('remove_pin', { space: 'taskbar', itemId: p.pinId }).catch(() => {});
                close();
              }}
            >
              Открепить от панели задач
            </button>
          ) : (
            p.target && (
              <button
                className="ctx-item"
                onClick={() => {
                  ipc('add_pin', {
                    space: 'taskbar',
                    item: {
                      id: `tb-${Date.now()}`,
                      kind: 'app',
                      title: p.name,
                      subtitle: 'Приложение',
                      target: p.target,
                      altAction: 'Новое окно',
                      accentRgb: '129,140,248',
                      iconKey: p.name.toLowerCase(),
                      iconData: null,
                    },
                  }).catch(() => {});
                  close();
                }}
              >
                Закрепить на панели задач
              </button>
            )
          )}
          {p.windows.length > 0 && (
            <>
              <div className="ctx-sep" />
              <button
                className="ctx-item"
                onClick={() => {
                  p.windows.forEach((w) => ipc('close_window_hwnd', { hwnd: w.hwnd }));
                  close();
                }}
              >
                Закрыть {p.windows.length > 1 ? `все окна (${p.windows.length})` : 'окно'}
              </button>
              {pid && (
                <button
                  className="ctx-item danger"
                  onClick={() => {
                    ipc('kill_process_pid', { pid });
                    close();
                  }}
                >
                  <KillIcon />
                  <span>Завершить задачу</span>
                </button>
              )}
            </>
          )}
        </div>
      </div>
    );
  }

  // 6. RIGHT-CLICK ON EMPTY TASKBAR
  if (req.kind === 'context_bar' || req.kind === 'ctx_taskbar') {
    const p = req.payload as { barMode?: string };
    const nextMode = p.barMode === 'edge' ? 'dock' : 'edge';
    return (
      <div className="flyout-root" data-space="personal">
        <div className="flyout-card ctx-card">
          <div className="ctx-sec">Панель задач</div>
          <button
            className="ctx-item"
            onClick={() => {
              ipc('launch_item', { target: 'C:\\Windows\\System32\\Taskmgr.exe', asAdmin: false, closeStart: false });
              close();
            }}
          >
            Диспетчер задач
          </button>
          <button
            className="ctx-item"
            onClick={() => {
              ipc('set_bar_mode', { mode: nextMode }).catch(() => {});
              close();
            }}
          >
            Стиль: {nextMode === 'edge' ? 'На всю ширину' : 'Плавающий док'}
          </button>
          <button
            className="ctx-item"
            onClick={() => {
              ipc('toggle_start', { force: true });
              close();
            }}
          >
            Открыть «Пуск»
          </button>
          <div className="ctx-sep" />
          <button
            className="ctx-item danger"
            onClick={() => ipc('restore_windows_taskbar_and_exit')}
          >
            <KillIcon />
            <span>Вернуть панель Windows и выйти</span>
          </button>
        </div>
      </div>
    );
  }

  return null;
};
