/**
 * TestBar always runs its own lightweight `complete-ux.html` spring & glass animations regardless
 * of Windows' global `MinAnimate=0` / `SPI_GETCLIENTAREAANIMATION=0` setting (which otherwise
 * causes WebView2 to report `prefers-reduced-motion: reduce` and kill all UI motion).
 */
export function getReducedMotionSnapshot(): boolean {
  return false;
}

export function useReducedMotion(): boolean {
  return false;
}
