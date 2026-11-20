import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  buildCalendarMonthGrid,
  formatAlertTime,
  formatSelectedDateHeader,
  isSameCalendarDay,
  isViewingTodayMonth,
  MONTHS_RU,
  shiftCalendarMonth,
  WEEKDAYS_SHORT_RU,
  type CalendarMonthRef,
} from '../lib/calendarLogic';
import {
  fetchIconsBatch,
  iconCache,
  ipc,
  isCacheIconKey,
  onEvent,
  type AppWindowGroup,
  type AudioMasterState,
  type AudioState,
  type FlyoutRequest,
  type JumpListItem,
  type NetworkInfo,
  type OpenWindowInfo,
  type SessionAlert,
  type TrayAppItem,
} from '../lib/ipc';
import {
  canOpenFileLocation,
  canRunAsAdmin,
  computeTrayFlyoutHeight,
  FLYOUT_EXIT_MS,
  formatIpcError,
  formatTrayHeaderLabel,
  getActiveDeviceIndex,
  INITIAL_FLYOUT_ANIM_STATE,
  isAppContextKind,
  isBarContextKind,
  OSD_AUTO_CLOSE_MS,
  PREVIEW_LEAVE_COMMAND,
  reduceFlyoutEntered,
  reduceFlyoutHideComplete,
  reduceFlyoutHideStart,
  reduceFlyoutShow,
  removePreviewWindow,
  stepVolume,
  syncPreviewWindowsFromGroups,
  TRAY_CARD_WIDTH,
  type FlyoutAnimState,
} from '../lib/flyoutLogic';
import {
  buildPinFromAppContext,
  type AppContextPayload,
  type BarContextPayload,
  type VolumeOsdPayload,
} from '../lib/taskbarLogic';
import {
  BellIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  ClockRecentIcon,
  CloseWindowIcon,
  ExternalWindowIcon,
  FallbackAppIcon,
  FolderSvg,
  KillIcon,
  LayoutBarIcon,
  NativeTrayIcon,
  PinIcon,
  SettingsGearIcon,
  ShieldIcon,
  TaskmgrIcon,
  VolIcon,
  XIcon10,
} from './Icons';

export const FlyoutView: React.FC = () => {
  const [anim, setAnim] = useState<FlyoutAnimState>(INITIAL_FLYOUT_ANIM_STATE);
  const [trayApps, setTrayApps] = useState<TrayAppItem[]>([]);
  const [trayIconMap, setTrayIconMap] = useState<Record<string, string>>({});
  const [trayHover, setTrayHover] = useState<string>('');
  const [trayCtxItem, setTrayCtxItem] = useState<TrayAppItem | null>(null);
  const [audio, setAudio] = useState<AudioState | null>(null);
  const [osdState, setOsdState] = useState<VolumeOsdPayload>({ volume: 0, muted: false });
  const [net, setNet] = useState<NetworkInfo | null>(null);
  const [thumbs, setThumbs] = useState<Record<number, string>>({});
  const [previewGroupKey, setPreviewGroupKey] = useState<string>('');
  const [previewWindows, setPreviewWindows] = useState<OpenWindowInfo[]>([]);
  const [jumpList, setJumpList] = useState<JumpListItem[]>([]);
  const [alerts, setAlerts] = useState<SessionAlert[]>([]);
  const [actionError, setActionError] = useState<string | null>(null);
  const [calView, setCalView] = useState<CalendarMonthRef>(() => {
    const now = new Date();
    return { year: now.getFullYear(), month: now.getMonth() };
  });
  const [selectedDate, setSelectedDate] = useState<Date>(() => new Date());

  const exitTimerRef = useRef<number | null>(null);
  const osdTimerRef = useRef<number | null>(null);
  const renderSeqRef = useRef<number>(0);
  const activeKindRef = useRef<string>('');
  const previewGroupKeyRef = useRef<string>('');
  const trayCtxItemRef = useRef<TrayAppItem | null>(null);
  const autoHeightRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    renderSeqRef.current = anim.renderSeq;
    activeKindRef.current = anim.phase === 'closed' ? '' : (anim.req?.kind ?? '');
  }, [anim.renderSeq, anim.phase, anim.req?.kind]);

  useEffect(() => {
    previewGroupKeyRef.current = previewGroupKey;
  }, [previewGroupKey]);

  useEffect(() => {
    trayCtxItemRef.current = trayCtxItem;
  }, [trayCtxItem]);

  const clearOsdTimer = () => {
    if (osdTimerRef.current !== null) {
      window.clearTimeout(osdTimerRef.current);
      osdTimerRef.current = null;
    }
  };

  const armOsdAutoClose = () => {
    clearOsdTimer();
    osdTimerRef.current = window.setTimeout(() => {
      osdTimerRef.current = null;
      if (activeKindRef.current === 'osd') {
        ipc('close_osd').catch(() => {});
      }
    }, OSD_AUTO_CLOSE_MS);
  };

  useEffect(() => {
    const subs = [
      onEvent<FlyoutRequest>('flyout-show', (r) => {
        if (exitTimerRef.current !== null) {
          window.clearTimeout(exitTimerRef.current);
          exitTimerRef.current = null;
        }
        const prevKind = activeKindRef.current;
        const nextSeq =
          r.kind === 'osd' && prevKind === 'osd' ? renderSeqRef.current : renderSeqRef.current + 1;
        renderSeqRef.current = nextSeq;
        activeKindRef.current = r.kind;
        setAnim((prev) => reduceFlyoutShow(prev, r));

        if (r.kind !== 'osd') {
          clearOsdTimer();
        }
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
          ipc<TrayAppItem[]>('get_tray_apps')
            .then((items) => {
              if (renderSeqRef.current !== nextSeq) return;
              setTrayApps(items);
              ipc('resize_flyout', {
                width: TRAY_CARD_WIDTH,
                height: computeTrayFlyoutHeight(items.length),
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
        } else if (r.kind === 'sound' || r.kind === 'quick') {
          ipc<AudioState>('get_audio_state')
            .then((st) => {
              if (renderSeqRef.current === nextSeq) setAudio(st);
            })
            .catch(() => {});
          ipc<NetworkInfo>('get_network_info')
            .then((n) => {
              if (renderSeqRef.current === nextSeq) setNet(n);
            })
            .catch(() => {});
        } else if (r.kind === 'calendar') {
          const now = new Date();
          setCalView({ year: now.getFullYear(), month: now.getMonth() });
          setSelectedDate(now);
          ipc<SessionAlert[]>('get_session_alerts')
            .then((list) => {
              if (renderSeqRef.current === nextSeq) setAlerts(list);
            })
            .catch(() => {});
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
        clearOsdTimer();
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
          setActionError(null);
          setTrayCtxItem(null);
        } else {
          ipc('close_flyout').catch(() => {});
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      clearOsdTimer();
      if (exitTimerRef.current !== null) window.clearTimeout(exitTimerRef.current);
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  // Lazy batch-fetch missing tray icon keys from Rust IconCache into local Map
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
  const cardCls = isClosing ? 'closing' : '';

  // Auto-fit flyout height cleanly for context menus, Quick Sound, and Calendar (never Tray!)
  useEffect(() => {
    if (!req || isClosing) return;
    const needsAutoHeight =
      isAppContextKind(req.kind) ||
      isBarContextKind(req.kind) ||
      req.kind === 'sound' ||
      req.kind === 'quick' ||
      req.kind === 'calendar';
    if (!needsAutoHeight) return;
    const el = autoHeightRef.current;
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
  }, [
    req?.kind,
    anim.renderSeq,
    isClosing,
    jumpList.length,
    actionError,
    audio?.sessions.length,
    audio?.devices.length,
    alerts.length,
  ]);

  const today = useMemo(() => new Date(), [anim.renderSeq]);
  const calCells = useMemo(
    () => buildCalendarMonthGrid(calView.year, calView.month, today, selectedDate),
    [calView.year, calView.month, today, selectedDate],
  );

  if (!req || anim.phase === 'closed') return null;

  const close = () => ipc('close_flyout').catch(() => {});
  const onCardAnimEnd = () => setAnim((prev) => reduceFlyoutEntered(prev));

  const runActionAndClose = (promise: Promise<unknown>) => {
    setActionError(null);
    promise
      .then(() => close())
      .catch((err) => setActionError(formatIpcError(err)));
  };

  // 1. TWO-TIER COMPACT 6-COLUMN TRAY OVERFLOW (^) — ZERO JUMPING / ZERO SHAKING
  if (req.kind === 'tray') {
    const headerLabel = formatTrayHeaderLabel(trayHover, trayApps.length);
    return (
      <div className="flyout-root" data-space="personal">
        <div
          key={anim.renderSeq}
          className={`flyout-card tray-card ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
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
                      }).catch(() => {});
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

          {trayCtxItem && (
            <div
              className="tray-ctx-overlay"
              onClick={() => {
                setActionError(null);
                setTrayCtxItem(null);
              }}
            >
              <div className="tray-ctx-sheet" onClick={(e) => e.stopPropagation()}>
                <div className="tray-ctx-head">
                  <span className="ctx-sec" style={{ padding: 0 }}>{trayCtxItem.name}</span>
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
                      hwnd: trayCtxItem.hwnd ?? null,
                      exePath: trayCtxItem.exePath,
                    }).catch((err) => setActionError(formatIpcError(err)));
                  }}
                >
                  <ExternalWindowIcon />
                  <span className="ctx-item-label">Открыть</span>
                </button>

                {canOpenFileLocation(trayCtxItem.exePath) && (
                  <button
                    type="button"
                    className="ctx-item"
                    onClick={() =>
                      runActionAndClose(ipc('open_location', { path: trayCtxItem.exePath }))
                    }
                  >
                    <FolderSvg className="ic14" />
                    <span className="ctx-item-label">Открыть расположение</span>
                  </button>
                )}

                <div className="ctx-sep" />

                <button
                  type="button"
                  className="ctx-item danger"
                  onClick={() => {
                    setActionError(null);
                    const pids =
                      trayCtxItem.allPids.length > 0 ? trayCtxItem.allPids : [trayCtxItem.pid];
                    ipc('end_tray_app_task', { pids })
                      .then(() => {
                        setTrayApps((prev) => prev.filter((x) => x.id !== trayCtxItem.id));
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

  // 2. FLOATING NON-ACTIVATING VOLUME OSD
  if (req.kind === 'osd') {
    const pct = osdState.muted ? 0 : osdState.volume;
    return (
      <div className="flyout-root" data-space="personal">
        <div
          key={anim.renderSeq}
          className={`flyout-card osd-card ${osdState.muted ? 'muted' : ''} ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
        >
          <span className="osd-ic">
            <VolIcon muted={osdState.muted} className="ic16" />
          </span>
          <div className="osd-track">
            <div className="osd-fill" style={{ width: `${pct}%` }} />
          </div>
          <span className="osd-val">{osdState.muted ? 'Выкл' : `${osdState.volume}%`}</span>
        </div>
      </div>
    );
  }

  // 3. QUICK SOUND & MIXER (324px, SLIDING 3-DEVICE PILL, MULTI-PROCESS MIXER)
  if (req.kind === 'sound' || req.kind === 'quick') {
    const devs = (audio?.devices ?? []).slice(0, 3);
    const activeDevIdx = getActiveDeviceIndex(devs);
    const sessions = audio?.sessions ?? [];

    const setMaster = (vol: number, muted: boolean) => {
      setActionError(null);
      setAudio((prev) => (prev ? { ...prev, masterVolume: vol, masterMuted: muted } : prev));
      ipc<AudioState>('set_master_volume', { volume: vol, muted })
        .then(setAudio)
        .catch((err) => setActionError(formatIpcError(err)));
    };

    const setSession = (
      s: AudioState['sessions'][number],
      vol: number,
      muted: boolean,
    ) => {
      setActionError(null);
      setAudio((prev) =>
        prev
          ? {
              ...prev,
              sessions: prev.sessions.map((item) =>
                item.id === s.id ? { ...item, volume: vol, muted } : item,
              ),
            }
          : prev,
      );
      ipc<AudioState>('set_session_volume', {
        pid: s.pid,
        pids: s.pids,
        appKey: s.id,
        volume: vol,
        muted,
      })
        .then(setAudio)
        .catch((err) => setActionError(formatIpcError(err)));
    };

    return (
      <div className="flyout-root" data-space="personal">
        <div
          key={anim.renderSeq}
          ref={autoHeightRef}
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
            <div
              className="seg-bar sliding"
              style={
                {
                  gridTemplateColumns: `repeat(${devs.length}, 1fr)`,
                  marginBottom: 10,
                  ['--seg-count' as string]: devs.length,
                  ['--seg-idx' as string]: activeDevIdx,
                } as React.CSSProperties
              }
            >
              <span className="seg-indicator" aria-hidden="true" />
              {devs.map((d) => (
                <button
                  key={d.id}
                  type="button"
                  className={`seg-opt ${d.isDefault ? 'active' : ''}`}
                  title={d.name}
                  onClick={() => {
                    setActionError(null);
                    ipc<AudioState>('set_audio_device', { deviceId: d.id })
                      .then(setAudio)
                      .catch((err) => setActionError(formatIpcError(err)));
                  }}
                >
                  {d.shortName}
                </button>
              ))}
            </div>
          )}

          <div
            className={`mix-row qs-master-row ${audio?.masterMuted ? 'muted' : ''}`}
            onWheel={(e) => {
              e.preventDefault();
              const next = stepVolume(audio?.masterVolume ?? 0, e.deltaY, 4);
              setMaster(next, false);
            }}
          >
            <button
              type="button"
              className="mix-mute"
              title={audio?.masterMuted ? 'Включить звук' : 'Выключить звук'}
              onClick={() => setMaster(audio?.masterVolume ?? 0, !(audio?.masterMuted ?? false))}
            >
              <VolIcon muted={audio?.masterMuted} />
            </button>
            <span className="mix-name" style={{ fontWeight: 600 }}>Общий звук</span>
            <input
              type="range"
              className="mix-range"
              min={0}
              max={100}
              value={audio?.masterMuted ? 0 : (audio?.masterVolume ?? 0)}
              onChange={(e) => setMaster(Number(e.target.value), false)}
            />
            <span className="mix-val">
              {audio?.masterMuted ? '0%' : `${audio?.masterVolume ?? 0}%`}
            </span>
          </div>

          <div className="mixer-list qs-mixer-scroll">
            {sessions.length === 0 ? (
              <div className="qs-empty">Нет активных аудиопотоков</div>
            ) : (
              sessions.map((s) => (
                <div
                  key={s.id}
                  className={`mix-row ${s.muted ? 'muted' : ''}`}
                  onWheel={(e) => {
                    e.preventDefault();
                    const next = stepVolume(s.volume, e.deltaY, 4);
                    setSession(s, next, false);
                  }}
                >
                  <button
                    type="button"
                    className="mix-mute"
                    title={s.muted ? 'Включить звук приложения' : 'Заглушить приложение'}
                    onClick={() => setSession(s, s.volume, !s.muted)}
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
                    onChange={(e) => setSession(s, Number(e.target.value), false)}
                  />
                  <span className="mix-val">{s.muted ? '0%' : `${s.volume}%`}</span>
                </div>
              ))
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

  // 4. INTERACTIVE CLOCK & CALENDAR (310px, DATE SELECTION, "СЕГОДНЯ", LOCAL ALERTS, WIN+N)
  if (req.kind === 'calendar') {
    const awayFromToday =
      !isSameCalendarDay(selectedDate, today) || !isViewingTodayMonth(calView, today);
    const jumpToToday = () => {
      const fresh = new Date();
      setCalView({ year: fresh.getFullYear(), month: fresh.getMonth() });
      setSelectedDate(fresh);
    };

    return (
      <div className="flyout-root" data-space="personal">
        <div
          key={anim.renderSeq}
          ref={autoHeightRef}
          className={`flyout-card cal-card ${cardCls}`}
          onAnimationEnd={onCardAnimEnd}
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
                onClick={() => setCalView((prev) => shiftCalendarMonth(prev.year, prev.month, -1))}
              >
                <ChevronLeftIcon className="ic14" />
              </button>
              <button
                type="button"
                className="hdr-btn cal-nav-btn"
                title="Следующий месяц"
                onClick={() => setCalView((prev) => shiftCalendarMonth(prev.year, prev.month, 1))}
              >
                <ChevronRightIcon className="ic14" />
              </button>
            </div>
          </div>

          <div className="cal-grid">
            {WEEKDAYS_SHORT_RU.map((wd) => (
              <div key={wd} className="cal-wd">{wd}</div>
            ))}
            {calCells.map((c) => (
              <button
                key={c.key}
                type="button"
                className={`cal-day ${c.inCurrentMonth ? '' : 'muted'} ${c.isToday ? 'today' : ''} ${c.isSelected ? 'selected' : ''}`}
                onClick={() => {
                  setSelectedDate(new Date(c.year, c.month, c.day));
                  if (!c.inCurrentMonth) {
                    setCalView({ year: c.year, month: c.month });
                  }
                }}
              >
                {c.day}
              </button>
            ))}
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

  // 5. GAPLESS ANIMATED HOVER WINDOW PREVIEW
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
                  type="button"
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

  // 6. RIGHT-CLICK CONTEXT MENU ON TASKBAR APP (JUMP LIST + ADMIN + LOCATION + PIN + END TASK)
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
          ref={autoHeightRef}
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

  // 7. RIGHT-CLICK ON EMPTY TASKBAR
  if (isBarContextKind(req.kind)) {
    const p = req.payload as BarContextPayload;
    const isEdge = p.barMode === 'edge';
    const nextMode = isEdge ? 'dock' : 'edge';
    return (
      <div className="flyout-root" data-space="personal">
        <div
          key={anim.renderSeq}
          ref={autoHeightRef}
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
