import React from 'react';

export interface SpotCardProps extends React.HTMLAttributes<HTMLDivElement> {
  children: React.ReactNode;
}

export const SpotCard: React.FC<SpotCardProps> = ({
  className = '',
  onMouseMove,
  children,
  ...rest
}) => {
  const handleMove = (e: React.MouseEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    e.currentTarget.style.setProperty('--mx', `${e.clientX - rect.left}px`);
    e.currentTarget.style.setProperty('--my', `${e.clientY - rect.top}px`);
    onMouseMove?.(e);
  };

  return (
    <div
      className={className ? `spot-card ${className}` : 'spot-card'}
      onMouseMove={handleMove}
      {...rest}
    >
      {children}
    </div>
  );
};

export function trackTilePointer(e: React.MouseEvent<HTMLElement>): void {
  const rect = e.currentTarget.getBoundingClientRect();
  e.currentTarget.style.setProperty('--tx', `${e.clientX - rect.left}px`);
  e.currentTarget.style.setProperty('--ty', `${e.clientY - rect.top}px`);
}
