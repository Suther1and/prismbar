import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  adjustTimerMinutes,
  DEFAULT_TIMER_STATE,
  formatTimerMmSs,
  getEffectiveRemainingSec,
  normalizeTimerState,
  parseTimerState,
  pauseTimer,
  readTimerState,
  reduceAdjustTimerMinutes,
  reducePauseTimer,
  reduceResetTimer,
  reduceSetTimerMinutes,
  reduceStartTimer,
  resetTimer,
  setTimerMinutes,
  startTimer,
  TIMER_PRESETS,
  TIMER_STORAGE_KEY,
  TIMER_UPDATED_EVENT,
  toggleTimer,
  type FocusTimerState,
} from './timerStore';

function installMockWindowAndStorage() {
  const store = new Map<string, string>();
  const target = new EventTarget();

  const mockStorage: Storage = {
    get length() {
      return store.size;
    },
    clear() {
      store.clear();
    },
    getItem(key: string) {
      return store.get(key) ?? null;
    },
    key(index: number) {
      return Array.from(store.keys())[index] ?? null;
    },
    removeItem(key: string) {
      store.delete(key);
    },
    setItem(key: string, value: string) {
      store.set(key, String(value));
    },
  };

  const dispatched: Event[] = [];
  const mockWindow = {
    addEventListener: target.addEventListener.bind(target),
    removeEventListener: target.removeEventListener.bind(target),
    dispatchEvent: (ev: Event) => {
      dispatched.push(ev);
      return target.dispatchEvent(ev);
    },
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
  };

  vi.stubGlobal('localStorage', mockStorage);
  vi.stubGlobal('window', mockWindow);

  return { store, dispatched, target };
}

describe('timerStore monotonic focus countdown & storage sync', () => {
  beforeEach(() => {
    installMockWindowAndStorage();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('formats MM:SS cleanly and exposes standard presets', () => {
    expect([...TIMER_PRESETS]).toEqual([5, 15, 25, 45]);
    expect(formatTimerMmSs(1500)).toBe('25:00');
    expect(formatTimerMmSs(65)).toBe('01:05');
    expect(formatTimerMmSs(0)).toBe('00:00');
    expect(formatTimerMmSs(-12)).toBe('00:00');
    expect(formatTimerMmSs(Number.NaN)).toBe('00:00');
    expect(formatTimerMmSs(3900)).toBe('65:00');
  });

  it('computes monotonic remaining seconds across start, tick, pause, resume, reset & adjust', () => {
    const t0 = 1_000_000;
    let st: FocusTimerState = reduceSetTimerMinutes(25);
    expect(st).toEqual({
      running: false,
      totalSec: 1500,
      remainingSec: 1500,
      endEpochMs: null,
    });

    st = reduceStartTimer(st, t0);
    expect(st.running).toBe(true);
    expect(st.endEpochMs).toBe(t0 + 1_500_000);
    expect(getEffectiveRemainingSec(st, t0)).toBe(1500);
    expect(getEffectiveRemainingSec(st, t0 + 100)).toBe(1500);
    expect(getEffectiveRemainingSec(st, t0 + 1000)).toBe(1499);
    expect(getEffectiveRemainingSec(st, t0 + 60_500)).toBe(1440);

    // Pause snapshots exact remaining seconds
    st = reducePauseTimer(st, t0 + 60_000);
    expect(st).toEqual({
      running: false,
      totalSec: 1500,
      remainingSec: 1440,
      endEpochMs: null,
    });
    expect(getEffectiveRemainingSec(st, t0 + 500_000)).toBe(1440);

    // Adjust +1m while paused & partially elapsed
    st = reduceAdjustTimerMinutes(st, 1, t0 + 500_000);
    expect(st.remainingSec).toBe(1500);
    expect(st.totalSec).toBe(1500);

    // Adjust +5m while running extends endEpochMs monotonically
    st = reduceStartTimer(st, t0 + 500_000);
    st = reduceAdjustTimerMinutes(st, 5, t0 + 510_000);
    expect(st.running).toBe(true);
    expect(st.remainingSec).toBe(1790);
    expect(st.totalSec).toBe(1790);
    expect(st.endEpochMs).toBe(t0 + 510_000 + 1790 * 1000);

    // Clamp to minimum 1 minute
    const clampedMin = reduceAdjustTimerMinutes(reduceSetTimerMinutes(1), -5);
    expect(clampedMin.remainingSec).toBe(60);
    expect(clampedMin.totalSec).toBe(60);

    // Reset restores full totalSec and stops timer
    const reset = reduceResetTimer(st);
    expect(reset).toEqual({
      running: false,
      totalSec: 1790,
      remainingSec: 1790,
      endEpochMs: null,
    });

    // Starting from 00:00 restarts full totalSec
    const expired: FocusTimerState = {
      running: false,
      totalSec: 900,
      remainingSec: 0,
      endEpochMs: null,
    };
    const restarted = reduceStartTimer(expired, t0 + 900_000);
    expect(restarted).toEqual({
      running: true,
      totalSec: 900,
      remainingSec: 900,
      endEpochMs: t0 + 900_000 + 900_000,
    });

    // Elapsed past endEpochMs returns 0 without negative values
    expect(
      getEffectiveRemainingSec(
        { running: true, remainingSec: 300, endEpochMs: t0 + 300_000 },
        t0 + 999_999,
      ),
    ).toBe(0);
  });

  it('persists state in localStorage under testbar.focus_timer.v1 and emits sync events', () => {
    const events: FocusTimerState[] = [];
    window.addEventListener(TIMER_UPDATED_EVENT, (ev) => {
      events.push((ev as CustomEvent<FocusTimerState>).detail);
    });

    expect(readTimerState()).toEqual(DEFAULT_TIMER_STATE);

    const preset = setTimerMinutes(15);
    expect(preset.totalSec).toBe(900);
    expect(JSON.parse(localStorage.getItem(TIMER_STORAGE_KEY)!)).toEqual(preset);

    const started = startTimer(2_000_000);
    expect(started.running).toBe(true);
    expect(started.endEpochMs).toBe(2_000_000 + 900_000);

    const adjusted = adjustTimerMinutes(-5, 2_010_000);
    expect(adjusted.remainingSec).toBe(590);
    expect(adjusted.endEpochMs).toBe(2_010_000 + 590_000);

    const paused = pauseTimer(2_020_000);
    expect(paused.running).toBe(false);
    expect(paused.remainingSec).toBe(580);
    expect(paused.endEpochMs).toBeNull();

    const toggledOn = toggleTimer(2_030_000);
    expect(toggledOn.running).toBe(true);
    expect(toggledOn.endEpochMs).toBe(2_030_000 + 580_000);

    const reset = resetTimer();
    expect(reset).toEqual({
      running: false,
      totalSec: 900,
      remainingSec: 900,
      endEpochMs: null,
    });

    expect(events).toHaveLength(6);
    expect(events[events.length - 1]).toEqual(reset);
  });

  it('handles corrupt or malformed localStorage JSON gracefully', () => {
    expect(parseTimerState(null)).toEqual(DEFAULT_TIMER_STATE);
    expect(parseTimerState('{bad json')).toEqual(DEFAULT_TIMER_STATE);
    expect(normalizeTimerState({ running: true, totalSec: -5, endEpochMs: null })).toEqual({
      running: false,
      totalSec: DEFAULT_TIMER_STATE.totalSec,
      remainingSec: DEFAULT_TIMER_STATE.totalSec,
      endEpochMs: null,
    });
  });
});
