import React, { useEffect, useRef, useState } from 'react';
import {
  addQuickTodo,
  clearDoneQuickTodos,
  deleteQuickTodo,
  emptyRecycleBin,
  getNetworkSnapshot,
  getQuickTogglesState,
  ipc,
  isTauri,
  saveQuickNotesText,
  setKeepAwake,
  setPowerPlan,
  toggleQuickTodo,
  triggerScreenshot,
  type AudioState,
  type NetworkSnapshot,
  type PowerPlanKind,
  type QuickNotesState,
  type QuickTogglesState,
  type TestBarConfig,
} from '../../lib/ipc';
import { formatIpcError } from '../../lib/flyoutLogic';
import { detectPlatform, type AppPlatform } from '../../lib/platform';
import { TIMER_PRESETS, useFocusTimer } from '../../lib/timerStore';
import {
  CaffeineIcon,
  CameraSnipIcon,
  CheckIcon,
  CloseWindowIcon,
  CopyIcon,
  EthernetIcon,
  MicIcon,
  OfflineIcon,
  PauseIcon,
  PlaySmallIcon,
  PlusIcon,
  RefreshIcon,
  TrashIcon,
  WifiIcon,
} from '../Icons';
import { SpotCard } from './SpotCard';

export type WidgetCardMode = 'card' | 'popover';

const EMPTY_NOTES: QuickNotesState = { text: '', todos: [] };

export function formatRate(bytesPerSec: number): string {
  if (!Number.isFinite(bytesPerSec) || bytesPerSec <= 0) return '0 Б/с';
  if (bytesPerSec < 1024) return `${Math.round(bytesPerSec)} Б/с`;
  const kb = bytesPerSec / 1024;
  if (kb < 1024) return `${kb >= 100 ? kb.toFixed(0) : kb.toFixed(1)} КБ/с`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb >= 10 ? mb.toFixed(1) : mb.toFixed(2)} МБ/с`;
  const gb = mb / 1024;
  return `${gb.toFixed(2)} ГБ/с`;
}

export function formatRecycleSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 Б';
  if (bytes < 1024) return `${Math.round(bytes)} Б`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${Math.round(kb)} КБ`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb >= 100 ? mb.toFixed(0) : mb.toFixed(1)} МБ`;
  const gb = mb / 1024;
  return `${gb.toFixed(2)} ГБ`;
}

function WidgetCardShell({
  mode = 'card',
  bentoIndex,
  className,
  children,
}: {
  mode?: WidgetCardMode;
  bentoIndex?: number;
  className: string;
  children: React.ReactNode;
}) {
  if (mode === 'popover') {
    return <div className={`nw-popover-card ${className}`}>{children}</div>;
  }
  return (
    <SpotCard
      className={`compact-card ${className}`}
      {...(bentoIndex !== undefined ? { bentoIndex } : {})}
    >
      {children}
    </SpotCard>
  );
}

/* ===================== 1. NOTES & TODOS WIDGET ===================== */

export interface NotesWidgetCardProps {
  quickNotes?: QuickNotesState | null;
  onConfigUpdated?: (config: TestBarConfig) => void;
  onShowToast?: (msg: string) => void;
  mode?: WidgetCardMode;
  bentoIndex?: number;
}

export const NotesWidgetCard: React.FC<NotesWidgetCardProps> = ({
  quickNotes,
  onConfigUpdated,
  onShowToast,
  mode = 'card',
  bentoIndex,
}) => {
  const [fallbackNotes, setFallbackNotes] = useState<QuickNotesState>(EMPTY_NOTES);
  const effectiveNotes = quickNotes ?? fallbackNotes;

  const [draftText, setDraftText] = useState<string>(effectiveNotes.text);
  const [todoInput, setTodoInput] = useState('');
  const [savingText, setSavingText] = useState(false);
  const [todoBusy, setTodoBusy] = useState(false);

  const dirtyRef = useRef(false);
  const latestDraftRef = useRef(draftText);
  latestDraftRef.current = draftText;
  const saveTimerRef = useRef<number | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Hydrate standalone popover when quickNotes prop is omitted
  useEffect(() => {
    if (quickNotes !== undefined || !isTauri()) return;
    let active = true;
    ipc<TestBarConfig>('get_config')
      .then((cfg) => {
        if (!active || !cfg?.quickNotes) return;
        setFallbackNotes(cfg.quickNotes);
        if (!dirtyRef.current) {
          setDraftText(cfg.quickNotes.text);
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [quickNotes]);

  // Sync incoming external text updates when not actively dirty
  useEffect(() => {
    if (!dirtyRef.current && effectiveNotes.text !== draftText) {
      setDraftText(effectiveNotes.text);
    }
  }, [effectiveNotes.text]);

  const commitNotesText = (textToSave: string) => {
    if (saveTimerRef.current !== null && typeof window !== 'undefined') {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }
    setSavingText(true);
    saveQuickNotesText(textToSave)
      .then((cfg) => {
        if (!mountedRef.current) return;
        if (latestDraftRef.current === textToSave) {
          dirtyRef.current = false;
        }
        if (cfg?.quickNotes) setFallbackNotes(cfg.quickNotes);
        onConfigUpdated?.(cfg);
      })
      .catch((err) => {
        if (mountedRef.current) {
          onShowToast?.(formatIpcError(err));
        }
      })
      .finally(() => {
        if (mountedRef.current) setSavingText(false);
      });
  };

  useEffect(() => {
    return () => {
      if (saveTimerRef.current !== null && typeof window !== 'undefined') {
        window.clearTimeout(saveTimerRef.current);
        saveTimerRef.current = null;
      }
      if (dirtyRef.current && isTauri()) {
        saveQuickNotesText(latestDraftRef.current).catch(() => {});
      }
    };
  }, []);

  const handleTextChange = (next: string) => {
    dirtyRef.current = true;
    setDraftText(next);
    if (typeof window === 'undefined') return;
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
    }
    saveTimerRef.current = window.setTimeout(() => {
      saveTimerRef.current = null;
      commitNotesText(next);
    }, 450);
  };

  const applyNotesConfigResult = (cfg: TestBarConfig) => {
    if (cfg?.quickNotes) setFallbackNotes(cfg.quickNotes);
    onConfigUpdated?.(cfg);
  };

  const handleAddTodo = async () => {
    const trimmed = todoInput.trim();
    if (!trimmed || todoBusy) return;
    setTodoBusy(true);
    try {
      const cfg = await addQuickTodo(trimmed);
      applyNotesConfigResult(cfg);
      setTodoInput('');
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setTodoBusy(false);
    }
  };

  const handleToggleTodo = async (id: string) => {
    if (todoBusy) return;
    setTodoBusy(true);
    try {
      const cfg = await toggleQuickTodo(id);
      applyNotesConfigResult(cfg);
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setTodoBusy(false);
    }
  };

  const handleDeleteTodo = async (id: string) => {
    if (todoBusy) return;
    setTodoBusy(true);
    try {
      const cfg = await deleteQuickTodo(id);
      applyNotesConfigResult(cfg);
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setTodoBusy(false);
    }
  };

  const handleClearDone = async () => {
    if (todoBusy) return;
    setTodoBusy(true);
    try {
      const cfg = await clearDoneQuickTodos();
      applyNotesConfigResult(cfg);
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setTodoBusy(false);
    }
  };

  const todos = effectiveNotes.todos ?? [];
  const hasDoneTodos = todos.some((t) => t.done);
  const openCount = todos.filter((t) => !t.done).length;

  return (
    <WidgetCardShell mode={mode} bentoIndex={bentoIndex} className="nw-notes-card">
      <div className="sec-row" style={{ marginBottom: 7 }}>
        <div className="nw-head-left">
          <span className="sec-title">Заметки и дела</span>
          {openCount > 0 && <span className="nw-count-pill">{openCount}</span>}
        </div>
        <div className="sec-actions">
          {savingText && <span className="nw-sync-lbl">Сохранение…</span>}
          {hasDoneTodos && (
            <button
              type="button"
              className="sec-act"
              disabled={todoBusy}
              onClick={() => void handleClearDone()}
            >
              Очистить готовые
            </button>
          )}
        </div>
      </div>

      <textarea
        className="nw-notes-area"
        rows={mode === 'popover' ? 3 : 2}
        placeholder="Быстрая заметка…"
        aria-label="Быстрая заметка"
        value={draftText}
        onChange={(e) => handleTextChange(e.target.value)}
        onBlur={() => {
          if (dirtyRef.current) {
            commitNotesText(latestDraftRef.current);
          }
        }}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.currentTarget.blur();
          }
        }}
      />

      <div className="nw-todo-add-row">
        <input
          type="text"
          className="nw-todo-input"
          placeholder="Новая задача…"
          aria-label="Новая задача"
          maxLength={240}
          value={todoInput}
          onChange={(e) => setTodoInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              void handleAddTodo();
            } else if (e.key === 'Escape') {
              e.currentTarget.blur();
            }
          }}
        />
        <button
          type="button"
          className="nw-todo-add-btn"
          aria-label="Добавить задачу"
          disabled={todoBusy || !todoInput.trim()}
          onClick={() => void handleAddTodo()}
        >
          <PlusIcon className="ic12" />
        </button>
      </div>

      <div className="nw-todo-list">
        {todos.length === 0 ? (
          <div className="nw-empty-sub">Список задач пуст</div>
        ) : (
          todos.map((item) => (
            <div key={item.id} className={`nw-todo-row ${item.done ? 'done' : ''}`}>
              <button
                type="button"
                role="checkbox"
                aria-checked={item.done}
                aria-label={item.text}
                className={`nw-todo-check ${item.done ? 'checked' : ''}`}
                disabled={todoBusy}
                onClick={() => void handleToggleTodo(item.id)}
              >
                {item.done && <CheckIcon size={10} />}
              </button>
              <span className="nw-todo-txt" title={item.text}>
                {item.text}
              </span>
              <button
                type="button"
                className="nw-todo-del"
                aria-label={`Удалить задачу ${item.text}`}
                disabled={todoBusy}
                onClick={() => void handleDeleteTodo(item.id)}
              >
                <CloseWindowIcon className="ic12" />
              </button>
            </div>
          ))
        )}
      </div>
    </WidgetCardShell>
  );
};

/* ===================== 2. FOCUS TIMER WIDGET ===================== */

export interface TimerWidgetCardProps {
  mode?: WidgetCardMode;
  bentoIndex?: number;
  onShowToast?: (msg: string) => void;
}

const TIMER_RING_R = 23;
const TIMER_RING_CIRC = 2 * Math.PI * TIMER_RING_R;

export const TimerWidgetCard: React.FC<TimerWidgetCardProps> = ({
  mode = 'card',
  bentoIndex,
}) => {
  const {
    state,
    remainingSec,
    formatted,
    progress,
    toggle,
    reset,
    setMinutes,
    adjustMinutes,
  } = useFocusTimer();

  const statusLabel = state.running
    ? 'Идёт'
    : remainingSec === 0
      ? 'Время вышло'
      : remainingSec < state.totalSec
        ? 'Пауза'
        : 'Готов';
  const statusClass = state.running
    ? 'st-mid'
    : remainingSec === 0
      ? 'st-ok'
      : '';
  const dashOffset = TIMER_RING_CIRC * (1 - progress);

  return (
    <WidgetCardShell mode={mode} bentoIndex={bentoIndex} className="nw-timer-card">
      <div className="sec-row" style={{ marginBottom: 8 }}>
        <span className="sec-title">Таймер фокуса</span>
        <span className={`status-pill ${statusClass || 'nw-pill-muted'}`}>{statusLabel}</span>
      </div>

      <div className="nw-timer-hero">
        <div className="nw-timer-ring-wrap" aria-hidden="true">
          <svg className="nw-timer-ring" viewBox="0 0 56 56">
            <circle className="nw-timer-ring-bg" cx="28" cy="28" r={TIMER_RING_R} />
            <circle
              className="nw-timer-ring-fg"
              cx="28"
              cy="28"
              r={TIMER_RING_R}
              strokeDasharray={TIMER_RING_CIRC}
              strokeDashoffset={dashOffset}
            />
          </svg>
        </div>

        <div className="nw-timer-readout-col">
          <div className={`nw-timer-digits ${state.running ? 'running' : ''}`}>{formatted}</div>
          <div className="nw-timer-bar">
            <div
              className="nw-timer-bar-fill"
              style={{ ['--pct' as string]: progress } as React.CSSProperties}
            />
          </div>
        </div>

        <div className="nw-timer-actions">
          <button
            type="button"
            className={`m-btn play nw-timer-play ${state.running ? 'running' : ''}`}
            aria-label={state.running ? 'Пауза таймера' : 'Старт таймера'}
            title={state.running ? 'Пауза' : 'Старт'}
            onClick={() => toggle()}
          >
            {state.running ? <PauseIcon className="ic14" /> : <PlaySmallIcon className="ic14" />}
          </button>
          <button
            type="button"
            className="m-btn nw-timer-reset"
            aria-label="Сбросить таймер"
            title="Сбросить"
            onClick={() => reset()}
          >
            <RefreshIcon className="ic12" />
          </button>
        </div>
      </div>

      <div className="nw-timer-controls">
        <div className="nw-timer-presets" role="group" aria-label="Пресеты таймера">
          {TIMER_PRESETS.map((min) => {
            const isSelected = state.totalSec === min * 60 && !state.running && remainingSec === state.totalSec;
            return (
              <button
                key={min}
                type="button"
                className={`nw-preset-btn ${isSelected ? 'active' : ''}`}
                onClick={() => setMinutes(min)}
              >
                {min}м
              </button>
            );
          })}
        </div>
        <div className="nw-timer-adjust">
          <button
            type="button"
            className="nw-step-btn"
            aria-label="Уменьшить на 1 минуту"
            onClick={() => adjustMinutes(-1)}
          >
            -1м
          </button>
          <button
            type="button"
            className="nw-step-btn"
            aria-label="Увеличить на 1 минуту"
            onClick={() => adjustMinutes(1)}
          >
            +1м
          </button>
        </div>
      </div>
    </WidgetCardShell>
  );
};

/* ===================== 3. NETWORK & PING WIDGET ===================== */

export interface NetworkWidgetCardProps {
  network?: NetworkSnapshot | null;
  onSetNetwork?: (snapshot: NetworkSnapshot) => void;
  onShowToast?: (msg: string) => void;
  mode?: WidgetCardMode;
  bentoIndex?: number;
}

export const NetworkWidgetCard: React.FC<NetworkWidgetCardProps> = ({
  network,
  onSetNetwork,
  onShowToast,
  mode = 'card',
  bentoIndex,
}) => {
  const [fallbackNet, setFallbackNet] = useState<NetworkSnapshot | null>(null);
  const [probing, setProbing] = useState(false);
  const [copiedIp, setCopiedIp] = useState(false);
  const mountedRef = useRef(true);
  const copyTimerRef = useRef<number | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (copyTimerRef.current !== null && typeof window !== 'undefined') {
        window.clearTimeout(copyTimerRef.current);
      }
    };
  }, []);

  // Self-poll if parent doesn't supply network snapshot
  useEffect(() => {
    if (network !== undefined || !isTauri()) return;
    let active = true;
    const poll = () => {
      getNetworkSnapshot(false)
        .then((snap) => {
          if (active) setFallbackNet(snap);
        })
        .catch(() => {});
    };
    poll();
    const id = window.setInterval(poll, 2000);
    return () => {
      active = false;
      window.clearInterval(id);
    };
  }, [network]);

  const snap = network ?? fallbackNet;
  const online = snap?.online ?? false;
  const kindLow = (snap?.kind ?? '').toLowerCase();
  const isWifi = kindLow.includes('wifi') || kindLow.includes('wi-fi');
  const kindLabel = !online ? 'Офлайн' : isWifi ? 'Wi-Fi' : 'Ethernet';

  const pingMs = snap?.pingMs ?? null;
  const pingClass =
    pingMs === null ? 'nw-pill-muted' : pingMs < 60 ? 'st-ok' : pingMs < 140 ? 'st-mid' : 'st-hot';

  const handleProbe = async () => {
    if (probing) return;
    setProbing(true);
    try {
      const fresh = await getNetworkSnapshot(true);
      if (!mountedRef.current) return;
      setFallbackNet(fresh);
      onSetNetwork?.(fresh);
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setProbing(false);
    }
  };

  const handleCopyIp = () => {
    const ip = snap?.localIp?.trim();
    if (!ip) return;
    const finish = () => {
      setCopiedIp(true);
      onShowToast?.(`Скопировано: ${ip}`);
      if (copyTimerRef.current !== null && typeof window !== 'undefined') {
        window.clearTimeout(copyTimerRef.current);
      }
      if (typeof window !== 'undefined') {
        copyTimerRef.current = window.setTimeout(() => {
          copyTimerRef.current = null;
          if (mountedRef.current) setCopiedIp(false);
        }, 1200);
      }
    };

    if (isTauri()) {
      ipc('copy_to_clipboard', { text: ip })
        .then(finish)
        .catch(() => onShowToast?.('Не удалось скопировать IP'));
    } else if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      navigator.clipboard
        .writeText(ip)
        .then(finish)
        .catch(() => onShowToast?.('Не удалось скопировать IP'));
    }
  };

  return (
    <WidgetCardShell mode={mode} bentoIndex={bentoIndex} className="nw-network-card">
      <div className="sec-row" style={{ marginBottom: 8 }}>
        <div className="nw-head-left">
          <span className="sec-title">Сеть и пинг</span>
          <span className={`status-pill ${online ? 'st-ok' : 'st-hot'}`}>
            {online ? 'Онлайн' : 'Офлайн'}
          </span>
        </div>
        <button
          type="button"
          className={`periph-refresh-btn ${probing ? 'spinning' : ''}`}
          title="Замерить пинг"
          aria-label="Замерить пинг"
          disabled={probing}
          onClick={() => void handleProbe()}
        >
          <RefreshIcon className="ic12" />
        </button>
      </div>

      <div className="nw-net-adapter-row">
        <span className={`nw-net-ic ${!online ? 'offline' : ''}`}>
          {!online ? (
            <OfflineIcon className="ic14" />
          ) : isWifi ? (
            <WifiIcon className="ic14" />
          ) : (
            <EthernetIcon className="ic14" />
          )}
        </span>
        <span className="nw-net-adapter-name" title={snap?.adapterName || 'Сетевой адаптер'}>
          {snap?.adapterName || (snap ? 'Нет активного адаптера' : 'Определение сети…')}
        </span>
        <span className="periph-src-badge">{kindLabel}</span>
      </div>

      <div className="nw-net-rates">
        <div className="nw-rate-box">
          <span className="nw-rate-lbl">↓ Приём</span>
          <span className="nw-rate-val">{formatRate(snap?.rxBytesPerSec ?? 0)}</span>
        </div>
        <div className="nw-rate-box">
          <span className="nw-rate-lbl">↑ Отдача</span>
          <span className="nw-rate-val">{formatRate(snap?.txBytesPerSec ?? 0)}</span>
        </div>
      </div>

      <div className="nw-net-foot">
        <div className="nw-ip-pill">
          <span className="nw-ip-lbl">IPv4</span>
          <span className="nw-ip-val">{snap?.localIp || '—'}</span>
          {snap?.localIp && (
            <button
              type="button"
              className="nw-copy-btn"
              title="Скопировать IPv4"
              aria-label="Скопировать IPv4"
              onClick={handleCopyIp}
            >
              {copiedIp ? <CheckIcon size={11} /> : <CopyIcon className="ic12" />}
            </button>
          )}
        </div>

        <button
          type="button"
          className={`nw-ping-btn status-pill ${pingClass}`}
          title={snap?.pingHost ? `Пинг до ${snap.pingHost} · нажмите для замера` : 'Нажмите для замера пинга'}
          disabled={probing}
          onClick={() => void handleProbe()}
        >
          {pingMs !== null ? `${pingMs} мс` : probing ? '…' : 'Пинг'}
        </button>
      </div>
    </WidgetCardShell>
  );
};

/* ===================== 4. QUICK TOGGLES WIDGET ===================== */

export interface QuickTogglesWidgetCardProps {
  audio?: AudioState | null;
  quickToggles?: QuickTogglesState | null;
  platform?: AppPlatform;
  onSetAudio?: (audio: AudioState, fromServer?: boolean) => void;
  onSetQuickToggles?: (state: QuickTogglesState) => void;
  onShowToast?: (msg: string) => void;
  mode?: WidgetCardMode;
  bentoIndex?: number;
}

const POWER_PLANS: ReadonlyArray<{ id: PowerPlanKind; label: string }> = [
  { id: 'power_saver', label: 'Экономия' },
  { id: 'balanced', label: 'Баланс' },
  { id: 'high_performance', label: 'Макс' },
];

export const QuickTogglesWidgetCard: React.FC<QuickTogglesWidgetCardProps> = ({
  audio,
  quickToggles,
  platform,
  onSetAudio,
  onSetQuickToggles,
  onShowToast,
  mode = 'card',
  bentoIndex,
}) => {
  const isMac = (platform ?? detectPlatform()) === 'macos';
  const [fallbackToggles, setFallbackToggles] = useState<QuickTogglesState | null>(null);
  const [fallbackAudio, setFallbackAudio] = useState<AudioState | null>(null);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [trashArmed, setTrashArmed] = useState(false);

  const trashTimerRef = useRef<number | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (trashTimerRef.current !== null && typeof window !== 'undefined') {
        window.clearTimeout(trashTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let active = true;
    if (quickToggles === undefined) {
      getQuickTogglesState()
        .then((st) => {
          if (active) setFallbackToggles(st);
        })
        .catch(() => {});
    }
    if (!isMac && audio === undefined) {
      ipc<AudioState>('get_audio_state')
        .then((st) => {
          if (active) setFallbackAudio(st);
        })
        .catch(() => {});
    }
    return () => {
      active = false;
    };
  }, [quickToggles, audio, isMac]);

  const effectiveToggles = quickToggles ?? fallbackToggles;
  const effectiveAudio = audio ?? fallbackAudio;
  const micMuted = effectiveAudio?.micMuted ?? false;
  const keepAwake = effectiveToggles?.keepAwake ?? false;
  const activePlan = effectiveToggles?.powerPlan ?? 'balanced';
  const recycleItems = effectiveToggles?.recycleBinItems ?? 0;
  const recycleBytes = effectiveToggles?.recycleBinBytes ?? 0;

  const applyToggles = (next: QuickTogglesState) => {
    setFallbackToggles(next);
    onSetQuickToggles?.(next);
  };

  const handleToggleMic = async () => {
    if (busyKey) return;
    setBusyKey('mic');
    try {
      const next = await ipc<AudioState>('set_mic_muted', { muted: !micMuted });
      if (!mountedRef.current) return;
      setFallbackAudio(next);
      onSetAudio?.(next, true);
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setBusyKey(null);
    }
  };

  const handleToggleAwake = async () => {
    if (busyKey) return;
    setBusyKey('awake');
    try {
      const next = await setKeepAwake(!keepAwake);
      if (!mountedRef.current) return;
      applyToggles(next);
      onShowToast?.(next.keepAwake ? 'Экран не уснёт' : 'Режим бодрствования выключен');
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setBusyKey(null);
    }
  };

  const handleSelectPlan = async (plan: PowerPlanKind) => {
    if (busyKey || activePlan === plan) return;
    setBusyKey(`plan:${plan}`);
    try {
      const next = await setPowerPlan(plan);
      if (!mountedRef.current) return;
      applyToggles(next);
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setBusyKey(null);
    }
  };

  const handleScreenshot = async () => {
    if (busyKey) return;
    setBusyKey('snip');
    try {
      await triggerScreenshot();
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setBusyKey(null);
    }
  };

  const handleRecycleClick = async () => {
    if (busyKey || recycleItems === 0) return;
    if (!trashArmed) {
      setTrashArmed(true);
      if (trashTimerRef.current !== null && typeof window !== 'undefined') {
        window.clearTimeout(trashTimerRef.current);
      }
      if (typeof window !== 'undefined') {
        trashTimerRef.current = window.setTimeout(() => {
          trashTimerRef.current = null;
          if (mountedRef.current) setTrashArmed(false);
        }, 2800);
      }
      return;
    }

    setTrashArmed(false);
    if (trashTimerRef.current !== null && typeof window !== 'undefined') {
      window.clearTimeout(trashTimerRef.current);
      trashTimerRef.current = null;
    }

    setBusyKey('trash');
    try {
      const rawRes = (await emptyRecycleBin()) as unknown;
      const next =
        rawRes && typeof rawRes === 'object' && 'recycleBinItems' in rawRes
          ? (rawRes as QuickTogglesState)
          : await getQuickTogglesState();
      if (!mountedRef.current) return;
      applyToggles(next);
      onShowToast?.('Корзина очищена');
    } catch (err) {
      onShowToast?.(formatIpcError(err));
    } finally {
      if (mountedRef.current) setBusyKey(null);
    }
  };

  return (
    <WidgetCardShell mode={mode} bentoIndex={bentoIndex} className="nw-toggles-card">
      <div className="sec-row" style={{ marginBottom: 8 }}>
        <span className="sec-title">Быстрые тумблеры</span>
        {effectiveToggles?.powerPlanLabel && (
          <span className="nw-plan-lbl" title={effectiveToggles.powerPlanLabel}>
            {effectiveToggles.powerPlanLabel}
          </span>
        )}
      </div>

      <div className="nw-toggle-grid" style={isMac ? { gridTemplateColumns: '1fr' } : undefined}>
        {!isMac && (
          <button
            type="button"
            className={`sys-tgl nw-tgl-btn ${!micMuted ? 'on' : ''}`}
            disabled={busyKey === 'mic'}
            onClick={() => void handleToggleMic()}
          >
            <MicIcon className="ic14" muted={micMuted} />
            <div className="nw-tgl-meta">
              <div className="sys-tgl-t">Микрофон</div>
              <div className="sys-tgl-s">{micMuted ? 'Выключен' : 'Активен'}</div>
            </div>
            <span className={`mic-status-dot ${micMuted ? 'muted' : ''}`} aria-hidden="true" />
          </button>
        )}

        <button
          type="button"
          className={`sys-tgl nw-tgl-btn ${keepAwake ? 'on' : ''}`}
          title={isMac ? 'Не давать Mac уснуть (Caffeine)' : undefined}
          disabled={busyKey === 'awake'}
          onClick={() => void handleToggleAwake()}
        >
          <CaffeineIcon className="ic14" />
          <div className="nw-tgl-meta">
            <div className="sys-tgl-t">{isMac ? 'Не спать (Caffeine)' : 'Не спать'}</div>
            <div className="sys-tgl-s">{keepAwake ? 'Активен' : 'Выкл'}</div>
          </div>
        </button>
      </div>

      {!isMac && (
        <div className="nw-power-seg" role="radiogroup" aria-label="Схема питания">
          {POWER_PLANS.map((plan) => {
            const isActive = activePlan === plan.id;
            return (
              <button
                key={plan.id}
                type="button"
                role="radio"
                aria-checked={isActive}
                disabled={Boolean(busyKey)}
                className={`nw-power-btn ${isActive ? 'active' : ''}`}
                onClick={() => void handleSelectPlan(plan.id)}
              >
                {plan.label}
              </button>
            );
          })}
        </div>
      )}

      <div className="nw-toggle-actions">
        <button
          type="button"
          className="nw-act-btn"
          disabled={busyKey === 'snip'}
          onClick={() => void handleScreenshot()}
        >
          <CameraSnipIcon className="ic14" />
          <span>Скриншот</span>
        </button>

        <button
          type="button"
          className={`nw-act-btn nw-trash-btn ${trashArmed ? 'armed' : ''}`}
          disabled={busyKey === 'trash' || (effectiveToggles !== null && recycleItems === 0)}
          title={
            recycleItems > 0
              ? `Корзина: ${recycleItems} элем. (${formatRecycleSize(recycleBytes)})`
              : 'Корзина пуста'
          }
          onClick={() => void handleRecycleClick()}
        >
          <TrashIcon className="ic14" />
          <span>
            {trashArmed
              ? 'Подтвердить?'
              : recycleItems > 0
                ? `Корзина · ${formatRecycleSize(recycleBytes)}`
                : 'Корзина пуста'}
          </span>
        </button>
      </div>
    </WidgetCardShell>
  );
};
