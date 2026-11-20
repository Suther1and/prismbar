import React, { useEffect, useRef, useState } from 'react';
import {
  ipc,
  onEvent,
  type AppWindowGroup,
  type AudioState,
  type FlyoutRequest,
  type JumpListItem,
  type NetworkInfo,
  type OpenWindowInfo,
  type TrayAppItem,
} from '../lib/ipc';
import {
  canOpenFileLocation,
  canRunAsAdmin,
  FLYOUT_EXIT_MS,
  formatIpcError,
  INITIAL_FLYOUT_ANIM_STATE,
  isAppContextKind,
  isBarContextKind,
  PREVIEW_LEAVE_COMMAND,
  reduceFlyoutEntered,
  reduceFlyoutHideComplete,
  reduceFlyoutHideStart,
  reduceFlyoutShow,
  removePreviewWindow,
  syncPreviewWindowsFromGroups,
  type FlyoutAnimState,
} from '../lib/flyoutLogic';
import {
  buildPinFromAppContext,
  type AppContextPayload,
  type BarContextPayload,
} from '../lib/taskbarLogic';
import {
  ChevronLeftIcon,
  ClockRecentIcon,
  CloseWindowIcon,
  ExternalWindowIcon,
  FallbackAppIcon,
  FolderSvg,
  KillIcon,
  LayoutBarIcon,
  PinIcon,
  SettingsGearIcon,
  ShieldIcon,
  TaskmgrIcon,
  VolIcon,
  XIcon10,
} from './Icons';

const MONTHS = [
  'Январь', 'Февраль', 'Март', 'Апрель', 'Май', 'Июнь',
  'Июль', 'Август', 'Сентябрь', 'Октябрь', 'Ноябрь', 'Декабрь',
];

export const FlyoutView: React.FC = () => {
  const [anim, setAnim] = useState<FlyoutAnimState>(INITIAL_FLYOUT_ANIM_STATE);
  const [trayApps, setTrayApps] = useState<TrayAppItem[]>([]);
  const [trayHover, setTrayHover] = useState<string>('');
  const [trayCtxItem, setTrayCtxItem] = useState<TrayAppItem | null>(null);
  const [audio, setAudio] = useState<AudioState | null>(null);
  const [net, setNet] = useState<NetworkInfo | null>(null);
  const [thumbs, setThumbs] = useState<Record<number, string>>({});
  const [previewGroupKey, setPreviewGroupKey] = useState<string>('');
  const [previewWindows, setPreviewWindows] = useState<OpenWindowInfo[]>([]);
  const [jumpList, setJumpList] = useState<JumpListItem[]>([]);
  const [actionError, setActionError] = useState<string | null>(null);
  const [calDate, setCalDate] = useState(() => new Date());

  const exitTimerRef = useRef<number | null>(null);
  const renderSeqRef = useRef<number>(0);
  const activeKindRef = useRef<string>('');
  const previewGroupKeyRef = useRef<string>('');
  const ctxCardRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    renderSeqRef.current = anim.renderSeq;
    activeKindRef.current = anim.phase === 'closed' ? '' : (anim.req?.kind ?? '');
  }, [anim.renderSeq, anim.phase, anim.req?.kind]);

  useEffect(() => {
    previewGroupKeyRef.current = previewGroupKey;
  }, [previewGroupKey]);

  useEffect(() => {
    const subs = [
      onEvent<FlyoutRequest>('flyout-show', (r) => {
        if (exitTimerRef.current !== null) {
          window.clearTimeout(exitTimerRef.current);
          exitTimerRef.current = null;
        }
        const nextSeq = renderSeqRef.current + 1;
        renderSeqRef.current = nextSeq;
        activeKindRef.current = r.kind;
        setAnim((prev) => reduceFlyoutShow(prev, r));
        setTrayCtxItem(null);
        setTrayHover('');
        setActionError(null);
        setJumpList([]);

        if (r.kind !== 'preview') {
          previewGroupKeyRef.current = '';
          setPreviewGroupKey('');
          setPreviewWindows([]);
        }

        if (r.kind === 'tray') {
          ipc<TrayAppItem[]>('get_tray_apps').then(setTrayApps).catch(() => {});
        } else if (r.kind === 'sound' || r.kind === 'quick') {
          ipc<AudioState>('get_audio_state').then(setAudio).catch(() => {});
          ipc<NetworkInfo>('get_network_info').then(setNet).catch(() => {});
        } else if (r.kind === 'preview') {
          const payload = r.payload as { group?: AppWindowGroup };
          const wins = payload.group?.windows ?? [];
          const gKey = payload.group?.key ?? '';
          previewGroupKeyRef.current = gKey;
          setPreviewGroupKey(gKey);
          setPreviewWindows(wins);
          setThumbs({});
          wins.slice(0, 4).forEach((w) => {
            ipc<string | null>('get_window_thumbnail', { hwnd: w.hwnd })
              .then((img) => {
                if (img && renderSeqRef.current === nextSeq) {
                  setThumbs((prev) => ({ ...prev, [w.hwnd]: img }));
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
      onEvent<void>('flyout-hide', () => {
        if (exitTimerRef.current !== null) {
          window.clearTimeout(exitTimerRef.current);
          exitTimerRef.current = null;
        }
        const hideSeq = renderSeqRef.current;
        previewGroupKeyRef.current = '';
        setPreviewGroupKey('');
        setAnim((prev) => reduceFlyoutHideStart(prev));
        exitTimerRef.current = window.setTimeout(() => {
          exitTimerRef.current = null;
          setAnim((prev) => reduceFlyoutHideComplete(prev, hideSeq));
          setTrayCtxItem(null);
          setActionError(null);
        }, FLYOUT_EXIT_MS);
      }),
      onEvent<AudioState>('audio-updated', setAudio),
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
      if (e.key === 'Escape') ipc('close_flyout').catch(() => {});
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      if (exitTimerRef.current !== null) window.clearTimeout(exitTimerRef.current);
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  const req = anim.req;
  const isClosing = anim.phase === 'closing';
  const cardCls = isClosing ? 'closing' : '';

  // Auto-grow context menu flyout cleanly when Jump List items or an OS error banner mount
  useEffect(() => {
    if (!req || isClosing) return;
    if (!isAppContextKind(req.kind) && !isBarContextKind(req.kind) && !trayCtxItem) return;
    const el = ctxCardRef.current;
    if (!el) return;
    const syncHeight = () => {
      const h = Math.ceil(el.scrollHeight) + 6;
      if (h > 80) {
        ipc('resize_flyout', { width: null, height: h }).catch(() => {});
      }
    };
    syncHeight();
    const ro = new ResizeObserver(syncHeight);
    ro.observe(el);
    return () => ro.disconnect();
  }, [req?.kind, anim.renderSeq, isClosing, jumpList.length, actionError, trayCtxItem]);

  if (!req || anim.phase === 'closed') return null;

  const close = () => ipc('close_flyout').catch(() => {});
  const onCardAnimEnd = () => setAnim((prev) => reduceFlyoutEntered(prev));

  const runActionAndClose = (promise: Promise<unknown>) => {
    setActionError(null);
    promise
      .then(() => close())
      .catch((err) => setActionError(formatIpcError(err)));
  };

  // 1. COMPACT 40-ICON TRAY OVERFLOW (^)
  if (req.kind === 'tray') {
    if (trayCtxItem) {
      return (
        <div className="flyout-root" data-space="personal">
          <div
            key={`${anim.renderSeq}-tray-ctx`}
            ref={ctxCardRef}
            className={`flyout-card ctx-card ${cardCls}`}
            onAnimationEnd={onCardAnimEnd}
          >
            <div className="ctx-sec">{trayCtxItem.name}</div>
            <button
              className="ctx-item"
              onClick={() =>
                runActionAndClose(
                  ipc('launch_item', {
                    target: trayCtxItem.exePath,
                    args: null,
                    asAdmin: false,
                    closeStart: false,
                  }),
                )
              }
            >
              <ExternalWindowIcon />
              <span className="ctx-item-label">Открыть</span>
            </button>
            <button
              className="ctx-item"
              onClick={() =>
                runActionAndClose(ipc('open_location', { path: trayCtxItem.exePath }))
              }
            >
              <FolderSvg className="ic14" />
              <span className="ctx-item-label">Открыть расположение файла</span>
            </button>
            <div className="ctx-sep" />
            <button
              className="ctx-item danger"
              onClick={() => {
                setActionError(null);
                ipc('kill_process_pid', { pid: trayCtxItem.pid })
                  .then(() => {
                    setTrayApps((prev) => prev.filter((x) => x.pid !== trayCtxItem.pid));
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
            <button
              className="ctx-item"
              onClick={() => {
                setActionError(null);
                setTrayCtxItem(null);
              }}
            >
              <ChevronLeftIcon />
              <span className="ctx-item-label">Назад к иконкам</span>
            </button>
          </div>
        </div>
      );
    }

    return (
      <div className="flyout-root" data-space="personal">
        <div
          key={anim.renderSeq}
          className={`flyout-card tray-card ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
        >
          {trayHover && <div className="tray-tip">{trayHover}</div>}
          <div className="tray-grid">
            {trayApps.map((t) => (
              <button
                key={t.pid}
                className="tray-cell"
                onMouseEnter={() => setTrayHover(t.name)}
                onMouseLeave={() => setTrayHover('')}
                onClick={() => {
                  ipc('launch_item', {
                    target: t.exePath,
                    args: null,
                    asAdmin: false,
                    closeStart: false,
                  }).catch(() => {});
                  close();
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setActionError(null);
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
        <div
          key={anim.renderSeq}
          className={`flyout-card qs-card ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
        >
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
                  onClick={() => ipc<AudioState>('set_audio_device', { deviceId: d.id }).then(setAudio).catch(() => {})}
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
                })
                  .then(setAudio)
                  .catch(() => {})
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
                ipc<AudioState>('set_master_volume', { volume: v, muted: false })
                  .then(setAudio)
                  .catch(() => {});
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
                      ipc<AudioState>('set_session_volume', { pid: s.pid, volume: s.volume, muted: !s.muted })
                        .then(setAudio)
                        .catch(() => {})
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
                      ipc<AudioState>('set_session_volume', { pid: s.pid, volume: v, muted: false })
                        .then(setAudio)
                        .catch(() => {});
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
        <div
          key={anim.renderSeq}
          className={`flyout-card cal-card ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
        >
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

  // 4. GAPLESS ANIMATED HOVER WINDOW PREVIEW
  if (req.kind === 'preview') {
    const wins = previewWindows.slice(0, 4);
    return (
      <div
        className="flyout-root"
        data-space="personal"
        onMouseLeave={() => {
          ipc(PREVIEW_LEAVE_COMMAND).catch(() => {});
        }}
      >
        <div
          key={anim.renderSeq}
          className={`flyout-card preview-card ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
        >
          {wins.map((w) => (
            <div
              key={w.hwnd}
              className="wp-item"
              onClick={() => ipc('focus_window', { hwnd: w.hwnd, toggleMinimize: false }).catch(() => {})}
            >
              <div className="wp-head">
                <span style={{ fontSize: 11.5, fontWeight: 600, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {w.title}
                </span>
                <button
                  className="wp-close"
                  title="Закрыть окно"
                  onClick={(e) => {
                    e.stopPropagation();
                    ipc('close_window_hwnd', { hwnd: w.hwnd }).catch(() => {});
                    const nextWins = removePreviewWindow(previewWindows, w.hwnd);
                    setPreviewWindows(nextWins);
                    if (nextWins.length === 0) {
                      close();
                    } else {
                      const nextW = Math.min(nextWins.length, 4) * 214 + 20;
                      ipc('resize_flyout', { width: nextW, height: 166 }).catch(() => {});
                    }
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

  // 5. RIGHT-CLICK CONTEXT MENU ON TASKBAR APP (JUMP LIST + ADMIN + LOCATION + PIN + END TASK)
  if (isAppContextKind(req.kind)) {
    const p = req.payload as AppContextPayload;
    const pid = p.windows[0]?.pid;
    const launchTarget = p.target || (p.aumid ? `shell:AppsFolder\\${p.aumid}` : '');
    const showAdmin = canRunAsAdmin(p.target);
    const showLocation = canOpenFileLocation(p.target);
    const tasks = jumpList.filter((i) => i.kind === 'task');
    const recents = jumpList.filter((i) => i.kind === 'recent');

    return (
      <div className="flyout-root" data-space="personal">
        <div
          key={anim.renderSeq}
          ref={ctxCardRef}
          className={`flyout-card ctx-card ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
        >
          <div className="ctx-sec">{p.name}</div>

          {tasks.length > 0 && (
            <>
              <div className="ctx-subsec">Задачи</div>
              {tasks.map((item, idx) => (
                <button
                  key={`task-${idx}-${item.title}`}
                  className="ctx-item"
                  title={item.title}
                  onClick={() =>
                    runActionAndClose(
                      ipc('launch_item', {
                        target: item.target,
                        args: item.args ?? null,
                        asAdmin: false,
                        closeStart: false,
                      }),
                    )
                  }
                >
                  <ExternalWindowIcon />
                  <span className="ctx-item-label">{item.title}</span>
                </button>
              ))}
            </>
          )}

          {recents.length > 0 && (
            <>
              <div className="ctx-subsec">Недавние</div>
              {recents.map((item, idx) => (
                <button
                  key={`recent-${idx}-${item.target}`}
                  className="ctx-item"
                  title={item.target}
                  onClick={() =>
                    runActionAndClose(
                      ipc('launch_item', {
                        target: item.target,
                        args: item.args ?? null,
                        asAdmin: false,
                        closeStart: false,
                      }),
                    )
                  }
                >
                  <ClockRecentIcon />
                  <span className="ctx-item-label">{item.title}</span>
                </button>
              ))}
            </>
          )}

          {jumpList.length > 0 && <div className="ctx-sep" />}

          {launchTarget && (
            <button
              className="ctx-item"
              onClick={() =>
                runActionAndClose(
                  ipc('launch_item', {
                    target: launchTarget,
                    args: p.args ?? null,
                    asAdmin: false,
                    closeStart: false,
                  }),
                )
              }
            >
              <ExternalWindowIcon />
              <span className="ctx-item-label">Открыть новое окно</span>
            </button>
          )}

          {showAdmin && (
            <button
              className="ctx-item"
              onClick={() =>
                runActionAndClose(
                  ipc('launch_item', {
                    target: p.target,
                    args: p.args ?? null,
                    asAdmin: true,
                    closeStart: false,
                  }),
                )
              }
            >
              <ShieldIcon />
              <span className="ctx-item-label">Запустить от администратора</span>
            </button>
          )}

          {showLocation && (
            <button
              className="ctx-item"
              onClick={() => runActionAndClose(ipc('open_location', { path: p.target }))}
            >
              <FolderSvg className="ic14" />
              <span className="ctx-item-label">Открыть расположение файла</span>
            </button>
          )}

          <div className="ctx-sep" />

          {p.pinned && p.pinId ? (
            <button
              className="ctx-item"
              onClick={() =>
                runActionAndClose(ipc('remove_pin', { space: 'taskbar', itemId: p.pinId }))
              }
            >
              <PinIcon />
              <span className="ctx-item-label">Открепить с панели</span>
            </button>
          ) : (
            launchTarget && (
              <button
                className="ctx-item"
                onClick={() =>
                  runActionAndClose(
                    ipc('add_pin', {
                      space: 'taskbar',
                      item: buildPinFromAppContext(p),
                    }),
                  )
                }
              >
                <PinIcon />
                <span className="ctx-item-label">Закрепить на панели</span>
              </button>
            )
          )}

          {p.windows.length > 0 && (
            <>
              <div className="ctx-sep" />
              <button
                className="ctx-item"
                onClick={() => {
                  p.windows.forEach((w) => {
                    ipc('close_window_hwnd', { hwnd: w.hwnd }).catch(() => {});
                  });
                  close();
                }}
              >
                <CloseWindowIcon />
                <span className="ctx-item-label">
                  {p.windows.length > 1
                    ? `Закрыть все окна (${p.windows.length})`
                    : 'Закрыть окно'}
                </span>
              </button>
              {pid && (
                <button
                  className="ctx-item danger"
                  onClick={() => runActionAndClose(ipc('kill_process_pid', { pid }))}
                >
                  <KillIcon />
                  <span className="ctx-item-label">Завершить задачу</span>
                </button>
              )}
            </>
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

  // 6. RIGHT-CLICK ON EMPTY TASKBAR
  if (isBarContextKind(req.kind)) {
    const p = req.payload as BarContextPayload;
    const isEdge = p.barMode === 'edge';
    const nextMode = isEdge ? 'dock' : 'edge';
    return (
      <div className="flyout-root" data-space="personal">
        <div
          key={anim.renderSeq}
          ref={ctxCardRef}
          className={`flyout-card ctx-card ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
        >
          <div className="ctx-sec">Панель задач</div>
          <button
            className="ctx-item"
            onClick={() =>
              runActionAndClose(
                ipc('launch_item', {
                  target: 'C:\\Windows\\System32\\Taskmgr.exe',
                  args: null,
                  asAdmin: false,
                  closeStart: false,
                }),
              )
            }
          >
            <TaskmgrIcon />
            <span className="ctx-item-label">Диспетчер задач</span>
          </button>
          <button
            className="ctx-item"
            onClick={() => runActionAndClose(ipc('set_bar_mode', { mode: nextMode }))}
          >
            <LayoutBarIcon />
            <span className="ctx-item-label">
              Режим панели: {isEdge ? 'Парящий док' : 'Во всю ширину'}
            </span>
          </button>
          <button
            className="ctx-item"
            onClick={() => {
              ipc('open_start_settings').catch(() => {});
            }}
          >
            <SettingsGearIcon />
            <span className="ctx-item-label">Настройки TestBar</span>
          </button>
          <div className="ctx-sep" />
          <button
            className="ctx-item danger"
            onClick={() => ipc('restore_windows_taskbar_and_exit').catch(() => {})}
          >
            <KillIcon />
            <span className="ctx-item-label">Вернуть панель Windows</span>
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
