import { useCallback, useEffect, useState } from 'react';

export const TIMER_STORAGE_KEY = 'testbar.focus_timer.v1';
export const TIMER_UPDATED_EVENT = 'testbar:focus-timer-updated';
export const TIMER_PRESETS = [5, 15, 25, 45] as const;
export const DEFAULT_TIMER_MINUTES = 25;
export const MIN_TIMER_MINUTES = 1;
export const MAX_TIMER_MINUTES = 180;

export interface FocusTimerState {
  running: boolean;
  totalSec: number;
  remainingSec: number;
  endEpochMs: number | null;
}

export const DEFAULT_TIMER_STATE: FocusTimerState = {
  running: false,
  totalSec: DEFAULT_TIMER_MINUTES * 60,
  remainingSec: DEFAULT_TIMER_MINUTES * 60,
  endEpochMs: null,
};

type TimerSubscriber = (state: FocusTimerState) => void;
const subscribers = new Set<TimerSubscriber>();
let lastChimedEndEpochMs: number | null = null;

function clampSeconds(sec: number, minSec = 60, maxSec = MAX_TIMER_MINUTES * 60): number {
  if (!Number.isFinite(sec)) return DEFAULT_TIMER_MINUTES * 60;
  return Math.min(maxSec, Math.max(minSec, Math.round(sec)));
}

/**
 * Formats a duration in seconds as tabular `MM:SS` (e.g. `25:00`, `04:09`).
 */
export function formatTimerMmSs(totalSeconds: number): string {
  const safe = Number.isFinite(totalSeconds) ? Math.max(0, Math.floor(totalSeconds)) : 0;
  const mins = Math.floor(safe / 60);
  const secs = safe % 60;
  return `${String(mins).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
}

/**
 * Computes the monotonic remaining whole seconds for a timer snapshot at `nowMs`.
 */
export function getEffectiveRemainingSec(
  state: Pick<FocusTimerState, 'running' | 'remainingSec' | 'endEpochMs'>,
  nowMs: number = Date.now(),
): number {
  if (!state.running || state.endEpochMs === null || !Number.isFinite(state.endEpochMs)) {
    return Number.isFinite(state.remainingSec) ? Math.max(0, Math.round(state.remainingSec)) : 0;
  }
  const diffMs = state.endEpochMs - nowMs;
  if (diffMs <= 0) return 0;
  return Math.ceil(diffMs / 1000);
}

/**
 * Normalizes raw parsed storage JSON into a valid `FocusTimerState`.
 */
export function normalizeTimerState(raw: unknown): FocusTimerState {
  if (!raw || typeof raw !== 'object') {
    return { ...DEFAULT_TIMER_STATE };
  }
  const obj = raw as Record<string, unknown>;
  const totalSec =
    typeof obj.totalSec === 'number' && Number.isFinite(obj.totalSec) && obj.totalSec > 0
      ? clampSeconds(obj.totalSec, 1, MAX_TIMER_MINUTES * 60)
      : DEFAULT_TIMER_STATE.totalSec;
  const remainingSec =
    typeof obj.remainingSec === 'number' && Number.isFinite(obj.remainingSec)
      ? Math.max(0, Math.min(MAX_TIMER_MINUTES * 60, Math.round(obj.remainingSec)))
      : totalSec;
  const endEpochMs =
    typeof obj.endEpochMs === 'number' && Number.isFinite(obj.endEpochMs) && obj.endEpochMs > 0
      ? obj.endEpochMs
      : null;
  const running = Boolean(obj.running) && endEpochMs !== null;

  return {
    running,
    totalSec,
    remainingSec,
    endEpochMs: running ? endEpochMs : null,
  };
}

export function parseTimerState(rawJson: string | null | undefined): FocusTimerState {
  if (!rawJson) return { ...DEFAULT_TIMER_STATE };
  try {
    return normalizeTimerState(JSON.parse(rawJson));
  } catch {
    return { ...DEFAULT_TIMER_STATE };
  }
}

export function readTimerState(): FocusTimerState {
  if (typeof localStorage === 'undefined') {
    return { ...DEFAULT_TIMER_STATE };
  }
  try {
    return parseTimerState(localStorage.getItem(TIMER_STORAGE_KEY));
  } catch {
    return { ...DEFAULT_TIMER_STATE };
  }
}

function notifySubscribers(state: FocusTimerState): void {
  subscribers.forEach((cb) => {
    try {
      cb(state);
    } catch {
      // Ignore subscriber errors
    }
  });
}

export function writeTimerState(next: FocusTimerState): FocusTimerState {
  const normalized = normalizeTimerState(next);
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.setItem(TIMER_STORAGE_KEY, JSON.stringify(normalized));
    } catch {
      // Ignore storage quota errors
    }
  }
  if (typeof window !== 'undefined' && typeof window.dispatchEvent === 'function') {
    try {
      if (typeof CustomEvent === 'function') {
        window.dispatchEvent(new CustomEvent(TIMER_UPDATED_EVENT, { detail: normalized }));
      } else {
        const ev = new Event(TIMER_UPDATED_EVENT);
        window.dispatchEvent(ev);
      }
    } catch {
      // Ignore event dispatch errors
    }
  }
  notifySubscribers(normalized);
  return normalized;
}

export function reduceStartTimer(
  state: FocusTimerState,
  nowMs: number = Date.now(),
): FocusTimerState {
  const curRemaining = getEffectiveRemainingSec(state, nowMs);
  if (state.running && curRemaining > 0) {
    return state;
  }
  const baseTotal = state.totalSec > 0 ? state.totalSec : DEFAULT_TIMER_STATE.totalSec;
  const startFromSec = curRemaining > 0 ? curRemaining : baseTotal;
  return {
    running: true,
    totalSec: Math.max(baseTotal, startFromSec),
    remainingSec: startFromSec,
    endEpochMs: nowMs + startFromSec * 1000,
  };
}

export function reducePauseTimer(
  state: FocusTimerState,
  nowMs: number = Date.now(),
): FocusTimerState {
  if (!state.running) return state;
  const remainingSec = getEffectiveRemainingSec(state, nowMs);
  return {
    running: false,
    totalSec: state.totalSec,
    remainingSec,
    endEpochMs: null,
  };
}

export function reduceResetTimer(state: FocusTimerState): FocusTimerState {
  const totalSec = state.totalSec > 0 ? state.totalSec : DEFAULT_TIMER_STATE.totalSec;
  return {
    running: false,
    totalSec,
    remainingSec: totalSec,
    endEpochMs: null,
  };
}

export function reduceSetTimerMinutes(minutes: number): FocusTimerState {
  const clampedMin = Number.isFinite(minutes)
    ? Math.min(MAX_TIMER_MINUTES, Math.max(MIN_TIMER_MINUTES, Math.round(minutes)))
    : DEFAULT_TIMER_MINUTES;
  const totalSec = clampedMin * 60;
  return {
    running: false,
    totalSec,
    remainingSec: totalSec,
    endEpochMs: null,
  };
}

export function reduceAdjustTimerMinutes(
  state: FocusTimerState,
  deltaMinutes: number,
  nowMs: number = Date.now(),
): FocusTimerState {
  if (!Number.isFinite(deltaMinutes) || deltaMinutes === 0) {
    return state;
  }
  const deltaSec = Math.round(deltaMinutes) * 60;
  const curRemaining = getEffectiveRemainingSec(state, nowMs);

  if (state.running) {
    const nextRemaining = clampSeconds(curRemaining + deltaSec);
    const nextTotal = Math.max(state.totalSec, nextRemaining);
    return {
      running: true,
      totalSec: nextTotal,
      remainingSec: nextRemaining,
      endEpochMs: nowMs + nextRemaining * 1000,
    };
  }

  if (curRemaining === state.totalSec) {
    const nextTotal = clampSeconds(state.totalSec + deltaSec);
    return {
      running: false,
      totalSec: nextTotal,
      remainingSec: nextTotal,
      endEpochMs: null,
    };
  }

  const nextRemaining = clampSeconds(curRemaining + deltaSec);
  return {
    running: false,
    totalSec: Math.max(state.totalSec, nextRemaining),
    remainingSec: nextRemaining,
    endEpochMs: null,
  };
}

export function startTimer(nowMs: number = Date.now()): FocusTimerState {
  return writeTimerState(reduceStartTimer(readTimerState(), nowMs));
}

export function pauseTimer(nowMs: number = Date.now()): FocusTimerState {
  return writeTimerState(reducePauseTimer(readTimerState(), nowMs));
}

export function toggleTimer(nowMs: number = Date.now()): FocusTimerState {
  const current = readTimerState();
  const remaining = getEffectiveRemainingSec(current, nowMs);
  if (current.running && remaining > 0) {
    return writeTimerState(reducePauseTimer(current, nowMs));
  }
  return writeTimerState(reduceStartTimer(current, nowMs));
}

export function resetTimer(): FocusTimerState {
  return writeTimerState(reduceResetTimer(readTimerState()));
}

export function setTimerMinutes(minutes: number): FocusTimerState {
  return writeTimerState(reduceSetTimerMinutes(minutes));
}

export function adjustTimerMinutes(
  deltaMinutes: number,
  nowMs: number = Date.now(),
): FocusTimerState {
  return writeTimerState(reduceAdjustTimerMinutes(readTimerState(), deltaMinutes, nowMs));
}

/**
 * Plays a subtle, gentle two-note sine chime when a running focus timer reaches 00:00.
 */
export function playTimerChime(): void {
  if (typeof window === 'undefined') return;
  try {
    const w = window as unknown as {
      AudioContext?: typeof AudioContext;
      webkitAudioContext?: typeof AudioContext;
    };
    const Ctor = w.AudioContext || w.webkitAudioContext;
    if (!Ctor) return;
    const ctx = new Ctor();
    const now = ctx.currentTime;

    const scheduleNote = (freq: number, startOffset: number, duration: number, peakGain: number) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = 'sine';
      osc.frequency.setValueAtTime(freq, now + startOffset);
      gain.gain.setValueAtTime(0.0001, now + startOffset);
      gain.gain.exponentialRampToValueAtTime(peakGain, now + startOffset + 0.025);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + startOffset + duration);
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start(now + startOffset);
      osc.stop(now + startOffset + duration + 0.02);
    };

    scheduleNote(587.33, 0, 0.42, 0.065);
    scheduleNote(880.0, 0.14, 0.56, 0.055);

    if (typeof window.setTimeout === 'function') {
      window.setTimeout(() => {
        ctx.close().catch(() => {});
      }, 900);
    }
  } catch {
    // Ignore audio context restrictions in headless/locked environments
  }
}

function finalizeExpiredTimerIfNeeded(current: FocusTimerState, nowMs: number): FocusTimerState {
  if (!current.running) return current;
  if (getEffectiveRemainingSec(current, nowMs) > 0) return current;

  const endMs = current.endEpochMs;
  const shouldChime = endMs !== null && lastChimedEndEpochMs !== endMs;
  if (endMs !== null) {
    lastChimedEndEpochMs = endMs;
  }

  const completed: FocusTimerState = {
    running: false,
    totalSec: current.totalSec,
    remainingSec: 0,
    endEpochMs: null,
  };
  writeTimerState(completed);
  if (shouldChime) {
    playTimerChime();
  }
  return completed;
}

export interface UseFocusTimerResult {
  state: FocusTimerState;
  remainingSec: number;
  formatted: string;
  progress: number;
  start: () => FocusTimerState;
  pause: () => FocusTimerState;
  toggle: () => FocusTimerState;
  reset: () => FocusTimerState;
  setMinutes: (minutes: number) => FocusTimerState;
  adjustMinutes: (deltaMinutes: number) => FocusTimerState;
}

export function useFocusTimer(): UseFocusTimerResult {
  const [state, setState] = useState<FocusTimerState>(() => readTimerState());
  const [nowMs, setNowMs] = useState<number>(() => Date.now());

  useEffect(() => {
    const syncFromStorage = () => {
      const latest = readTimerState();
      const now = Date.now();
      setNowMs(now);
      setState(finalizeExpiredTimerIfNeeded(latest, now));
    };

    syncFromStorage();
    subscribers.add(setState);

    const onStorage = (e: StorageEvent) => {
      if (e.key === null || e.key === TIMER_STORAGE_KEY) {
        syncFromStorage();
      }
    };
    const onCustomUpdate = (e: Event) => {
      const detail = (e as CustomEvent<FocusTimerState> | undefined)?.detail;
      const now = Date.now();
      setNowMs(now);
      if (detail && typeof detail === 'object') {
        setState(normalizeTimerState(detail));
      } else {
        syncFromStorage();
      }
    };

    if (typeof window !== 'undefined') {
      window.addEventListener('storage', onStorage);
      window.addEventListener(TIMER_UPDATED_EVENT, onCustomUpdate);
    }

    return () => {
      subscribers.delete(setState);
      if (typeof window !== 'undefined') {
        window.removeEventListener('storage', onStorage);
        window.removeEventListener(TIMER_UPDATED_EVENT, onCustomUpdate);
      }
    };
  }, []);

  useEffect(() => {
    if (!state.running) return;
    const tick = () => {
      const now = Date.now();
      setNowMs(now);
      const current = readTimerState();
      if (current.running && getEffectiveRemainingSec(current, now) <= 0) {
        setState(finalizeExpiredTimerIfNeeded(current, now));
      }
    };
    tick();
    const id = window.setInterval(tick, 250);
    return () => window.clearInterval(id);
  }, [state.running, state.endEpochMs]);

  const remainingSec = getEffectiveRemainingSec(state, nowMs);
  const progress =
    state.totalSec > 0 ? Math.max(0, Math.min(1, remainingSec / state.totalSec)) : 0;

  const start = useCallback(() => startTimer(), []);
  const pause = useCallback(() => pauseTimer(), []);
  const toggle = useCallback(() => toggleTimer(), []);
  const reset = useCallback(() => resetTimer(), []);
  const setMinutes = useCallback((m: number) => setTimerMinutes(m), []);
  const adjustMinutes = useCallback((d: number) => adjustTimerMinutes(d), []);

  return {
    state,
    remainingSec,
    formatted: formatTimerMmSs(remainingSec),
    progress,
    start,
    pause,
    toggle,
    reset,
    setMinutes,
    adjustMinutes,
  };
}
