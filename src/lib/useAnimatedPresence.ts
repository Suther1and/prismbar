import { useEffect, useRef, useState } from 'react';
import { useReducedMotion } from './useReducedMotion';

export type PresencePhase = 'hidden' | 'entering' | 'open' | 'closing';

export interface AnimatedPresenceState {
  mounted: boolean;
  phase: PresencePhase;
}

/**
 * Drives interruptible enter/exit lifecycle transitions (`data-phase`) for overlays, drawers,
 * popovers, and context menus so surfaces animate smoothly both in AND out before unmounting.
 *
 * - Opening from unmounted: mounts in `'entering'`, then promotes to `'open'` on the next rAF.
 * - Closing: transitions to `'closing'` for `exitMs`, then unmounts to `'hidden'`.
 * - Reopening mid-exit: cancels the pending unmount and smoothly retargets directly to `'open'`
 *   from the live interpolated CSS frame with zero blink or jump.
 */
export function useAnimatedPresence(open: boolean, exitMs = 115): AnimatedPresenceState {
  const reducedMotion = useReducedMotion();
  const [mounted, setMounted] = useState<boolean>(open);
  const [phase, setPhase] = useState<PresencePhase>(open ? 'entering' : 'hidden');

  const raf1Ref = useRef<number | null>(null);
  const raf2Ref = useRef<number | null>(null);
  const exitTimerRef = useRef<number | null>(null);
  const mountedRef = useRef<boolean>(mounted);
  const phaseRef = useRef<PresencePhase>(phase);

  mountedRef.current = mounted;
  phaseRef.current = phase;

  useEffect(() => {
    const cancelPending = () => {
      if (raf1Ref.current !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(raf1Ref.current);
        raf1Ref.current = null;
      }
      if (raf2Ref.current !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(raf2Ref.current);
        raf2Ref.current = null;
      }
      if (exitTimerRef.current !== null) {
        window.clearTimeout(exitTimerRef.current);
        exitTimerRef.current = null;
      }
    };

    cancelPending();

    if (open) {
      if (mountedRef.current && phaseRef.current === 'closing') {
        // Interrupt exit in flight: retarget directly to settled open state.
        setPhase('open');
        return cancelPending;
      }

      if (!mountedRef.current) {
        setMounted(true);
        setPhase('entering');
      } else if (phaseRef.current === 'hidden') {
        setPhase('entering');
      }

      if (typeof requestAnimationFrame === 'function') {
        raf1Ref.current = requestAnimationFrame(() => {
          raf1Ref.current = null;
          raf2Ref.current = requestAnimationFrame(() => {
            raf2Ref.current = null;
            setPhase('open');
          });
        });
      } else {
        setPhase('open');
      }
    } else if (mountedRef.current) {
      setPhase('closing');
      const effectiveExitMs = reducedMotion ? Math.min(exitMs, 110) : Math.max(0, exitMs);
      exitTimerRef.current = window.setTimeout(() => {
        exitTimerRef.current = null;
        setMounted(false);
        setPhase('hidden');
      }, effectiveExitMs);
    } else {
      setPhase('hidden');
    }

    return cancelPending;
  }, [open, exitMs, reducedMotion]);

  return { mounted, phase };
}
