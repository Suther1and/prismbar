import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';

export interface ObsidianSliderProps {
  value: number;
  min?: number;
  max?: number;
  step?: number;
  wheelStep?: number;
  muted?: boolean;
  /** Optional live audio peak level (0..1) rendered as a subtle GPU live energy overlay bar. */
  peak?: number;
  /** Soft magnetic detent stops in value units (default `[0, 50, 100]` when range is `0..100`). */
  magneticDetents?: number[];
  /** Magnetic snap radius in percent of total span (default `2` -> ±2%). */
  detentThresholdPct?: number;
  /** Show subtle notch tick(s) for interior magnetic detents (default `true`). */
  showCenterNotch?: boolean;
  onChange: (value: number) => void;
  onCommit?: (value: number) => void;
  disabled?: boolean;
  ariaLabel: string;
  className?: string;
}

const DEFAULT_DETENTS: number[] = [0, 50, 100];

function clamp(v: number, lo: number, hi: number): number {
  if (!Number.isFinite(v)) return lo;
  return Math.min(hi, Math.max(lo, v));
}

function quantizeStep(v: number, min: number, max: number, step: number): number {
  if (!Number.isFinite(step) || step <= 0) return clamp(v, min, max);
  const snapped = Math.round((v - min) / step) * step + min;
  // Avoid IEEE-754 trailing decimals when step is fractional (e.g. 0.1)
  const precision = step < 1 ? Math.min(6, Math.ceil(-Math.log10(step))) : 0;
  const rounded = precision > 0 ? Number(snapped.toFixed(precision)) : Math.round(snapped);
  return clamp(rounded, min, max);
}

export const ObsidianSlider: React.FC<ObsidianSliderProps> = ({
  value,
  min = 0,
  max = 100,
  step = 1,
  wheelStep = 4,
  muted = false,
  peak,
  magneticDetents,
  detentThresholdPct = 2,
  showCenterNotch = true,
  onChange,
  onCommit,
  disabled = false,
  ariaLabel,
  className = '',
}) => {
  const span = Math.max(1e-6, max - min);
  const clampedValue = clamp(value, min, max);
  // Preserve stored level visually when muted (rendered in dimmed danger tint via CSS)
  const pct = clamp((clampedValue - min) / span, 0, 1);
  const peakNorm = typeof peak === 'number' && Number.isFinite(peak) ? clamp(peak, 0, 1) : null;

  const detents = useMemo<number[]>(() => {
    if (magneticDetents !== undefined) return magneticDetents;
    if (min === 0 && max === 100) return DEFAULT_DETENTS;
    return [min, min + span / 2, max];
  }, [magneticDetents, min, max, span]);

  const interiorNotchPcts = useMemo<number[]>(() => {
    if (!showCenterNotch || detents.length === 0) return [];
    return detents
      .filter((d) => d > min && d < max)
      .map((d) => clamp((d - min) / span, 0, 1));
  }, [showCenterNotch, detents, min, max, span]);

  const rootRef = useRef<HTMLDivElement>(null);
  const trackRef = useRef<HTMLDivElement>(null);
  const dragRectRef = useRef<DOMRect | null>(null);
  const activePointerIdRef = useRef<number | null>(null);
  const lastEmittedRef = useRef<number>(clampedValue);
  const wheelStateRef = useRef({
    disabled,
    wheelStep,
    clampedValue,
    min,
    max,
    step,
    muted,
    onChange,
    onCommit,
  });
  wheelStateRef.current = {
    disabled,
    wheelStep,
    clampedValue,
    min,
    max,
    step,
    muted,
    onChange,
    onCommit,
  };

  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const onNativeWheel = (e: WheelEvent) => {
      const s = wheelStateRef.current;
      if (s.disabled || e.deltaY === 0) return;
      e.preventDefault();
      e.stopPropagation();
      const delta = e.deltaY < 0 ? s.wheelStep : -s.wheelStep;
      const next = quantizeStep(s.clampedValue + delta, s.min, s.max, s.step);
      if (next !== s.clampedValue || s.muted) {
        lastEmittedRef.current = next;
        s.onChange(next);
        s.onCommit?.(next);
      }
    };
    el.addEventListener('wheel', onNativeWheel, { passive: false });
    return () => {
      el.removeEventListener('wheel', onNativeWheel);
    };
  }, []);

  const [dragging, setDragging] = useState(false);
  const [detentHit, setDetentHit] = useState(false);

  const resolveFromClientX = useCallback(
    (clientX: number, rect: DOMRect, applyMagnet: boolean): { next: number; snappedDetent: boolean } => {
      if (rect.width <= 0) {
        return { next: clampedValue, snappedDetent: false };
      }
      const ratio = clamp((clientX - rect.left) / rect.width, 0, 1);
      const raw = min + ratio * span;

      if (applyMagnet && detents.length > 0 && detentThresholdPct > 0) {
        const thresholdVal = (detentThresholdPct / 100) * span;
        for (const stop of detents) {
          if (Math.abs(raw - stop) <= thresholdVal) {
            return { next: clamp(stop, min, max), snappedDetent: true };
          }
        }
      }

      return { next: quantizeStep(raw, min, max, step), snappedDetent: false };
    },
    [clampedValue, min, max, span, detents, detentThresholdPct, step],
  );

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (disabled || e.button !== 0) return;
    const track = trackRef.current;
    if (!track) return;

    e.preventDefault();
    e.currentTarget.focus({ preventScroll: true });
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      // Ignore synthetic events in tests without active pointer IDs
    }

    const rect = track.getBoundingClientRect();
    dragRectRef.current = rect;
    activePointerIdRef.current = e.pointerId;
    setDragging(true);

    const { next, snappedDetent } = resolveFromClientX(e.clientX, rect, true);
    setDetentHit(snappedDetent);
    lastEmittedRef.current = next;
    onChange(next);
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging || disabled) return;
    if (activePointerIdRef.current !== null && e.pointerId !== activePointerIdRef.current) return;

    const rect = dragRectRef.current ?? trackRef.current?.getBoundingClientRect();
    if (!rect) return;

    const { next, snappedDetent } = resolveFromClientX(e.clientX, rect, true);
    setDetentHit(snappedDetent);
    if (next !== lastEmittedRef.current) {
      lastEmittedRef.current = next;
      onChange(next);
    }
  };

  const finishDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!dragging) return;
    if (activePointerIdRef.current !== null && e.pointerId !== activePointerIdRef.current) return;

    try {
      if (e.currentTarget.hasPointerCapture(e.pointerId)) {
        e.currentTarget.releasePointerCapture(e.pointerId);
      }
    } catch {
      // Safe no-op in non-capturing environments
    }

    activePointerIdRef.current = null;
    dragRectRef.current = null;
    setDragging(false);
    setDetentHit(false);
    onCommit?.(lastEmittedRef.current);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (disabled) return;
    const fineStep = e.shiftKey ? step * 5 : step;
    const pageStep = Math.max(step, Math.round(span * 0.1));
    let next: number | null = null;

    switch (e.key) {
      case 'ArrowRight':
      case 'ArrowUp':
        next = quantizeStep(clampedValue + fineStep, min, max, step);
        break;
      case 'ArrowLeft':
      case 'ArrowDown':
        next = quantizeStep(clampedValue - fineStep, min, max, step);
        break;
      case 'PageUp':
        next = quantizeStep(clampedValue + pageStep, min, max, step);
        break;
      case 'PageDown':
        next = quantizeStep(clampedValue - pageStep, min, max, step);
        break;
      case 'Home':
        next = min;
        break;
      case 'End':
        next = max;
        break;
      default:
        return;
    }

    e.preventDefault();
    lastEmittedRef.current = next;
    onChange(next);
    onCommit?.(next);
  };

  const rootCls = className ? `obs-slider ${className}` : 'obs-slider';

  return (
    <div
      ref={rootRef}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={ariaLabel}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={clampedValue}
      aria-disabled={disabled || undefined}
      data-dragging={dragging ? 'true' : 'false'}
      data-muted={muted ? 'true' : 'false'}
      data-detent={detentHit ? 'true' : 'false'}
      data-disabled={disabled ? 'true' : 'false'}
      className={rootCls}
      style={
        {
          ['--slider-pct' as string]: pct,
          ['--slider-peak' as string]: peakNorm ?? 0,
        } as React.CSSProperties
      }
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={finishDrag}
      onPointerCancel={finishDrag}
      onKeyDown={handleKeyDown}
    >
      <div ref={trackRef} className="obs-slider-track-wrap">
        <div className="obs-slider-track">
          <div className="obs-slider-fill" aria-hidden="true" />
          {peakNorm !== null && <div className="obs-slider-peak" aria-hidden="true" />}
        </div>
        {interiorNotchPcts.map((notchPct, idx) => (
          <span
            key={idx}
            className="obs-slider-notch"
            aria-hidden="true"
            style={{ left: `${notchPct * 100}%` }}
          />
        ))}
        <div className="obs-slider-thumb-rail" aria-hidden="true">
          <span className="obs-slider-thumb" />
        </div>
      </div>
    </div>
  );
};
