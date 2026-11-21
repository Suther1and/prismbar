import React, { useEffect, useRef } from 'react';
import { useReducedMotion } from '../../lib/useReducedMotion';

export interface SpotCardProps extends React.HTMLAttributes<HTMLDivElement> {
  bentoIndex?: number;
  children: React.ReactNode;
}

const SHEEN_HALF = 160;

function isHoverGlowOff(): boolean {
  return (
    typeof document !== 'undefined' &&
    document.documentElement.dataset.hoverGlow === 'off'
  );
}

export const SpotCard: React.FC<SpotCardProps> = ({
  className = '',
  bentoIndex,
  style,
  onPointerEnter,
  onPointerMove,
  onPointerLeave,
  children,
  ...rest
}) => {
  const reducedMotion = useReducedMotion();
  const cardRef = useRef<HTMLDivElement>(null);
  const sheenRef = useRef<HTMLSpanElement>(null);
  const rectRef = useRef<{ left: number; top: number } | null>(null);
  const pendingCoordsRef = useRef<{ x: number; y: number } | null>(null);
  const rafRef = useRef<number | null>(null);

  useEffect(() => {
    const el = cardRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      rectRef.current = { left: r.left, top: r.top };
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (rafRef.current !== null && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
    };
  }, []);

  const scheduleSheen = (clientX: number, clientY: number) => {
    if (reducedMotion || isHoverGlowOff() || !sheenRef.current) return;
    const rect = rectRef.current;
    if (!rect) return;
    pendingCoordsRef.current = {
      x: clientX - rect.left - SHEEN_HALF,
      y: clientY - rect.top - SHEEN_HALF,
    };
    if (rafRef.current !== null) return;
    if (typeof requestAnimationFrame === 'function') {
      rafRef.current = requestAnimationFrame(() => {
        rafRef.current = null;
        const coords = pendingCoordsRef.current;
        if (coords && sheenRef.current && !isHoverGlowOff()) {
          sheenRef.current.style.transform = `translate3d(${coords.x}px, ${coords.y}px, 0)`;
        }
      });
    } else if (pendingCoordsRef.current) {
      sheenRef.current.style.transform = `translate3d(${pendingCoordsRef.current.x}px, ${pendingCoordsRef.current.y}px, 0)`;
    }
  };

  const handlePointerEnter = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!reducedMotion && !isHoverGlowOff() && cardRef.current) {
      const r = cardRef.current.getBoundingClientRect();
      rectRef.current = { left: r.left, top: r.top };
      scheduleSheen(e.clientX, e.clientY);
    }
    onPointerEnter?.(e);
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!reducedMotion && !isHoverGlowOff()) {
      if (!rectRef.current && cardRef.current) {
        const r = cardRef.current.getBoundingClientRect();
        rectRef.current = { left: r.left, top: r.top };
      }
      scheduleSheen(e.clientX, e.clientY);
    }
    onPointerMove?.(e);
  };

  const handlePointerLeave = (e: React.PointerEvent<HTMLDivElement>) => {
    rectRef.current = null;
    pendingCoordsRef.current = null;
    if (rafRef.current !== null && typeof cancelAnimationFrame === 'function') {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    onPointerLeave?.(e);
  };

  const classes = ['spot-card', !reducedMotion ? 'has-sheen' : '', className]
    .filter(Boolean)
    .join(' ');

  const mergedStyle: React.CSSProperties | undefined =
    bentoIndex !== undefined
      ? ({
          ...style,
          ['--bento-i' as string]: bentoIndex,
        } as React.CSSProperties)
      : style;

  return (
    <div
      ref={cardRef}
      className={classes}
      data-bento={bentoIndex !== undefined ? '' : undefined}
      style={mergedStyle}
      onPointerEnter={handlePointerEnter}
      onPointerMove={handlePointerMove}
      onPointerLeave={handlePointerLeave}
      {...rest}
    >
      {!reducedMotion && <span ref={sheenRef} className="spot-sheen" aria-hidden="true" />}
      {children}
    </div>
  );
};

export function trackTilePointer(e: React.MouseEvent<HTMLElement>): void {
  const rect = e.currentTarget.getBoundingClientRect();
  e.currentTarget.style.setProperty('--tx', `${e.clientX - rect.left}px`);
  e.currentTarget.style.setProperty('--ty', `${e.clientY - rect.top}px`);
}
