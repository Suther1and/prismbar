import { useEffect, useState } from 'react';

const REDUCED_MOTION_QUERY = '(prefers-reduced-motion: reduce)';

/**
 * Safely reads the current `prefers-reduced-motion` media query.
 * Works cleanly in SSR and Vitest/jsdom environments where `matchMedia` may be absent.
 */
export function getReducedMotionSnapshot(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return false;
  }
  try {
    return window.matchMedia(REDUCED_MOTION_QUERY).matches;
  } catch {
    return false;
  }
}

/**
 * React hook subscribing to `prefers-reduced-motion: reduce` via a single cached MediaQueryList
 * listener (never polling `matchMedia` on pointermove or render hot paths).
 */
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState<boolean>(getReducedMotionSnapshot);

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
      return;
    }

    let mql: MediaQueryList;
    try {
      mql = window.matchMedia(REDUCED_MOTION_QUERY);
    } catch {
      return;
    }

    setReduced(mql.matches);

    const onChange = (event: MediaQueryListEvent) => {
      setReduced(event.matches);
    };

    if (typeof mql.addEventListener === 'function') {
      mql.addEventListener('change', onChange);
      return () => mql.removeEventListener('change', onChange);
    }

    // Legacy WebView fallback
    if (typeof mql.addListener === 'function') {
      mql.addListener(onChange);
      return () => mql.removeListener(onChange);
    }
  }, []);

  return reduced;
}
