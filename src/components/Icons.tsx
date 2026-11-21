import React from 'react';

export interface IconProps {
  className?: string;
  color?: string;
}

/**
 * Razor-sharp 4-pane Start mark with luminous frosted silver/white specular panes.
 * Base panes render crisp high-contrast silver-white; `.start-logo-accent` illuminates
 * smoothly with `var(--accent)` on hover and `.start-on` via CSS opacity (GPU-only).
 */
export const StartLogoIcon: React.FC<{ className?: string }> = ({ className = 'ic24' }) => (
  <svg className={`${className} start-logo-svg`} viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <defs>
      <linearGradient id="tb_pane_tl" x1="3.5" y1="3.5" x2="11.1" y2="11.1" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stopColor="#FFFFFF" stopOpacity="1" />
        <stop offset="100%" stopColor="#E6EDF9" stopOpacity="0.96" />
      </linearGradient>
      <linearGradient id="tb_pane_mid" x1="3.5" y1="3.5" x2="20.5" y2="20.5" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stopColor="#F4F7FD" stopOpacity="0.96" />
        <stop offset="100%" stopColor="#D3DDF0" stopOpacity="0.88" />
      </linearGradient>
      <linearGradient id="tb_pane_br" x1="12.9" y1="12.9" x2="20.5" y2="20.5" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stopColor="#E9EFFA" stopOpacity="0.94" />
        <stop offset="100%" stopColor="#C4D1E7" stopOpacity="0.86" />
      </linearGradient>
    </defs>

    {/* Luminous frosted silver-white precision panes */}
    <g className="start-logo-base">
      <rect x="3.5" y="3.5" width="7.6" height="7.6" rx="1.5" fill="url(#tb_pane_tl)" />
      <rect x="12.9" y="3.5" width="7.6" height="7.6" rx="1.5" fill="url(#tb_pane_mid)" />
      <rect x="3.5" y="12.9" width="7.6" height="7.6" rx="1.5" fill="url(#tb_pane_mid)" />
      <rect x="12.9" y="12.9" width="7.6" height="7.6" rx="1.5" fill="url(#tb_pane_br)" />
    </g>

    {/* Luminous Space Accent illumination overlay (faded in via CSS on hover / start-on) */}
    <g className="start-logo-accent">
      <rect x="3.5" y="3.5" width="7.6" height="7.6" rx="1.5" fill="var(--accent)" fillOpacity="1" />
      <rect x="12.9" y="3.5" width="7.6" height="7.6" rx="1.5" fill="var(--accent)" fillOpacity="0.88" />
      <rect x="3.5" y="12.9" width="7.6" height="7.6" rx="1.5" fill="var(--accent)" fillOpacity="0.84" />
      <rect x="12.9" y="12.9" width="7.6" height="7.6" rx="1.5" fill="var(--accent)" fillOpacity="0.96" />
    </g>
  </svg>
);

export const StartLogo = StartLogoIcon;

export const XIcon10: React.FC = () => (
  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round">
    <line x1="2" y1="2" x2="8" y2="8" />
    <line x1="8" y1="2" x2="2" y2="8" />
  </svg>
);

export const KillIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round">
    <circle cx="12" cy="12" r="9" />
    <line x1="15" y1="9" x2="9" y2="15" />
    <line x1="9" y1="9" x2="15" y2="15" />
  </svg>
);

export const ChevronLeftIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="15 18 9 12 15 6" />
  </svg>
);

export const ChevronRightIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="9 18 15 12 9 6" />
  </svg>
);

export const ChevronDownIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="6 9 12 15 18 9" />
  </svg>
);

export const ChevronUpIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="18 15 12 9 6 15" />
  </svg>
);

export const BellIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9" />
    <path d="M13.73 21a2 2 0 0 1-3.46 0" />
  </svg>
);

export const NativeTrayIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <line x1="3" y1="15" x2="21" y2="15" />
    <polyline points="9 11 12 8 15 11" />
  </svg>
);

export const ExternalWindowIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <line x1="3" y1="9" x2="21" y2="9" />
    <line x1="12" y1="12.5" x2="12" y2="16.5" />
    <line x1="10" y1="14.5" x2="14" y2="14.5" />
  </svg>
);

export const ShieldIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 3l7 3v5c0 4.6-3 8.5-7 10-4-1.5-7-5.4-7-10V6l7-3z" />
    <polyline points="9.5 12.2 11.2 14 14.8 10.2" />
  </svg>
);

export const PinIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 17v5" />
    <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a1 1 0 0 0 0-2H8a1 1 0 0 0 0 2h1v4.76z" />
  </svg>
);

export const UnpinIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <line x1="2" y1="2" x2="22" y2="22" />
    <path d="M12 17v5" />
    <path d="M9 9v1.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h12" />
    <path d="M15 9.34V6h1a1 1 0 0 0 0-2H9.34" />
  </svg>
);

export const CloseWindowIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round">
    <line x1="6" y1="6" x2="18" y2="18" />
    <line x1="18" y1="6" x2="6" y2="18" />
  </svg>
);

export const ClockRecentIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="9" />
    <polyline points="12 7 12 12 15.5 14" />
  </svg>
);

export const TaskmgrIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <polyline points="6 14 9.5 14 11.5 9.5 14 16 15.8 12.5 18 12.5" />
  </svg>
);

export const LayoutBarIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <line x1="6" y1="16.5" x2="18" y2="16.5" />
  </svg>
);

export const SettingsGearIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.08a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.08a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.08a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </svg>
);

export const VolIcon: React.FC<{ muted?: boolean; level?: number; className?: string }> = ({
  muted,
  level,
  className = 'ic14',
}) => {
  const isSilent = muted || (typeof level === 'number' && level <= 0);
  const showOuterWave = typeof level === 'number' ? level > 50 : false;
  if (isSilent) {
    return (
      <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
        <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
        <line x1="22" y1="9" x2="16" y2="15" />
        <line x1="16" y1="9" x2="22" y2="15" />
      </svg>
    );
  }
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
      <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
      {showOuterWave && <path d="M18.8 5.2a9.5 9.5 0 0 1 0 13.6" opacity="0.72" />}
    </svg>
  );
};

/* ===================== NETWORK ICONS ===================== */

export const EthernetIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="14" rx="2.5" />
    <line x1="7.5" y1="4" x2="7.5" y2="8.5" />
    <line x1="10.5" y1="4" x2="10.5" y2="8.5" />
    <line x1="13.5" y1="4" x2="13.5" y2="8.5" />
    <line x1="16.5" y1="4" x2="16.5" y2="8.5" />
    <path d="M9 18v2.5h6V18" />
  </svg>
);

export const WifiIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
    <path d="M4.5 10.5a11.5 11.5 0 0 1 15 0" />
    <path d="M8.2 14.3a6.2 6.2 0 0 1 7.6 0" />
    <circle cx="12" cy="18.5" r="1.2" fill="currentColor" stroke="none" />
  </svg>
);

export const OfflineIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="9" />
    <line x1="4.5" y1="4.5" x2="19.5" y2="19.5" />
    <path d="M8.5 12c0-2.8 1.2-5.4 3.5-7" opacity="0.6" />
  </svg>
);

export const NetIcon: React.FC<{ kind?: string; connected?: boolean; className?: string }> = ({
  kind = 'ethernet',
  connected = true,
  className = 'ic14',
}) => {
  if (!connected) return <OfflineIcon className={className} />;
  const low = kind.toLowerCase();
  if (low.includes('wi-fi') || low.includes('wifi') || low.includes('wireless')) {
    return <WifiIcon className={className} />;
  }
  return <EthernetIcon className={className} />;
};

/* ===================== WINDOW MANAGEMENT ICONS ===================== */

export const SnapLeftIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <rect x="5.5" y="6.5" width="5.5" height="11" rx="1" fill="currentColor" fillOpacity="0.3" />
    <line x1="12" y1="4" x2="12" y2="20" />
  </svg>
);

export const SnapRightIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <rect x="13" y="6.5" width="5.5" height="11" rx="1" fill="currentColor" fillOpacity="0.3" />
    <line x1="12" y1="4" x2="12" y2="20" />
  </svg>
);

export const MaximizeIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="4" y="4" width="16" height="16" rx="2.5" />
  </svg>
);

export const RestoreIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="4" y="8" width="12" height="12" rx="2" />
    <path d="M8 8V6a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-2" />
  </svg>
);

export const TopmostIcon: React.FC<{ className?: string; active?: boolean }> = ({
  className = 'ic14',
  active = false,
}) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill={active ? 'currentColor' : 'none'}
    fillOpacity={active ? 0.22 : 0}
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <path d="M12 17v5" />
    <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a1 1 0 0 0 0-2H8a1 1 0 0 0 0 2h1v4.76z" />
  </svg>
);

export const MinimizeOthersIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="4" y="4" width="16" height="12" rx="2.5" />
    <line x1="6" y1="20" x2="18" y2="20" />
    <polyline points="9 11 12 14 15 11" />
  </svg>
);

/* ===================== ACTION, MEDIA & SYSTEM ICONS ===================== */

export const TerminalIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <polyline points="7.5 9.5 10.5 12 7.5 14.5" />
    <line x1="12.5" y1="14.5" x2="16.5" y2="14.5" />
  </svg>
);

export const CopyIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="9" y="9" width="11" height="11" rx="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  </svg>
);

export const PasteIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M9 5H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2" />
    <rect x="9" y="3" width="6" height="4" rx="1.2" />
    <line x1="9" y1="12" x2="15" y2="12" />
    <line x1="9" y1="16" x2="13" y2="16" />
  </svg>
);

export const TrashIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="3 6 5 6 21 6" />
    <path d="M8 6V4a1.5 1.5 0 0 1 1.5-1.5h5A1.5 1.5 0 0 1 16 4v2" />
    <path d="M19 6l-1 13.5a2 2 0 0 1-2 1.85H8a2 2 0 0 1-2-1.85L5 6" />
    <line x1="10" y1="11" x2="10" y2="16.5" />
    <line x1="14" y1="11" x2="14" y2="16.5" />
  </svg>
);

export const PauseIcon: React.FC<{ className?: string; size?: number }> = ({ className = 'ic14', size }) => (
  <svg
    className={size ? undefined : className}
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="currentColor"
  >
    <rect x="6" y="4.5" width="4" height="15" rx="1.2" />
    <rect x="14" y="4.5" width="4" height="15" rx="1.2" />
  </svg>
);

export const PlaySmallIcon: React.FC<{ className?: string; size?: number }> = ({ className = 'ic12', size }) => (
  <svg
    className={size ? undefined : className}
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="currentColor"
  >
    <path d="M7 5.2c0-.9.98-1.45 1.75-.98l10.2 6.3a1.15 1.15 0 0 1 0 1.96l-10.2 6.3A1.15 1.15 0 0 1 7 17.8V5.2z" />
  </svg>
);

export const PrevTrackIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="currentColor">
    <polygon points="19 5 9.5 12 19 19 19 5" />
    <rect x="5" y="5.5" width="2.4" height="13" rx="1" />
  </svg>
);

export const NextTrackIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="currentColor">
    <polygon points="5 5 14.5 12 5 19 5 5" />
    <rect x="16.6" y="5.5" width="2.4" height="13" rx="1" />
  </svg>
);

export const MusicNoteIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M9 18V6.2l11-2V16" />
    <circle cx="6.5" cy="18" r="2.5" />
    <circle cx="17.5" cy="16" r="2.5" />
  </svg>
);

export const EditIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" />
  </svg>
);

export const SparkIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 3l1.9 5.6L19.5 10.5l-5.6 1.9L12 18l-1.9-5.6L4.5 10.5l5.6-1.9L12 3z" />
    <path d="M18.5 15.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8.8-2.2z" />
  </svg>
);

export const DiskIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="5" width="18" height="14" rx="2.5" />
    <line x1="3" y1="14" x2="21" y2="14" />
    <circle cx="17" cy="16.5" r="0.9" fill="currentColor" stroke="none" />
  </svg>
);

export const DownloadIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 3v12" />
    <polyline points="7 11 12 16 17 11" />
    <path d="M4 19h16" />
  </svg>
);

export const UploadIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 16V4" />
    <polyline points="7 9 12 4 17 9" />
    <path d="M4 19h16" />
  </svg>
);

export const EyeOffIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M17.94 17.94A10.1 10.1 0 0 1 12 20c-6 0-9.5-5.2-10-8a16.8 16.8 0 0 1 4.1-5.94" />
    <path d="M9.9 4.24A9.4 9.4 0 0 1 12 4c6 0 9.5 5.2 10 8a17 17 0 0 1-1.92 3.55" />
    <line x1="3" y1="3" x2="21" y2="21" />
  </svg>
);

export const RefreshIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 12a9 9 0 1 1-2.64-6.36" />
    <polyline points="21 3 21 9 15 9" />
  </svg>
);

export const ScissorsIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="6.5" cy="7" r="2.75" />
    <circle cx="6.5" cy="17" r="2.75" />
    <line x1="8.8" y1="8.8" x2="20" y2="17" />
    <line x1="8.8" y1="15.2" x2="20" y2="7" />
  </svg>
);

export const BroomIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M19 3l-8 8" />
    <path d="M9.2 10.8L4 16l4 4 5.2-5.2a2.5 2.5 0 0 0 0-3.54l-.46-.46a2.5 2.5 0 0 0-3.54 0z" />
    <line x1="6.5" y1="18.5" x2="9" y2="16" />
  </svg>
);

export const LayersIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <polygon points="12 3 21 8 12 13 3 8 12 3" />
    <polyline points="3 12.5 12 17.5 21 12.5" />
    <polyline points="3 17 12 22 21 17" />
  </svg>
);

export const SlidersIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <line x1="4" y1="7" x2="20" y2="7" />
    <circle cx="9" cy="7" r="2" fill="var(--bg-solid, #11131B)" />
    <line x1="4" y1="12" x2="20" y2="12" />
    <circle cx="15" cy="12" r="2" fill="var(--bg-solid, #11131B)" />
    <line x1="4" y1="17" x2="20" y2="17" />
    <circle cx="10" cy="17" r="2" fill="var(--bg-solid, #11131B)" />
  </svg>
);

export const KeyboardIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="2.5" y="5.5" width="19" height="13" rx="2.5" />
    <line x1="6.5" y1="9.5" x2="8" y2="9.5" />
    <line x1="11.25" y1="9.5" x2="12.75" y2="9.5" />
    <line x1="16" y1="9.5" x2="17.5" y2="9.5" />
    <line x1="8" y1="14.5" x2="16" y2="14.5" />
  </svg>
);

export const LockIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <rect x="5" y="11" width="14" height="9.5" rx="2.2" />
    <path d="M8 11V7.8a4 4 0 0 1 8 0V11" />
  </svg>
);

export const SleepIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z" />
  </svg>
);

export const RebootIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
    <polyline points="3 3 3 8 8 8" />
  </svg>
);

/* ===================== PRESERVED CORE ICONS ===================== */

export const FolderSvg: React.FC<{ className?: string; color?: string }> = ({
  className = 'ic16',
  color = 'currentColor',
}) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
  </svg>
);

export const FileSvg: React.FC<{ className?: string; color?: string }> = ({
  className = 'ic16',
  color = 'currentColor',
}) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
  </svg>
);

export const GlobeSvg: React.FC<{ className?: string; color?: string }> = ({
  className = 'ic16',
  color = 'currentColor',
}) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.65" strokeLinecap="round">
    <circle cx="12" cy="12" r="10" />
    <line x1="2" y1="12" x2="22" y2="12" />
    <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
  </svg>
);

export const SearchIcon: React.FC<{ className?: string; color?: string }> = ({
  className = 'ic14',
  color = 'currentColor',
}) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.75" strokeLinecap="round">
    <circle cx="11" cy="11" r="7" />
    <line x1="21" y1="21" x2="16.65" y2="16.65" />
  </svg>
);

export const PlayTriangleIcon: React.FC<{ className?: string; size?: number }> = ({ className, size = 11 }) => (
  <svg className={className} width={size} height={size} viewBox="0 0 24 24" fill="currentColor">
    <polygon points="6 4 20 12 6 20 6 4" />
  </svg>
);

export const ExternalArrowIcon: React.FC<{ className?: string }> = ({ className = 'ic12' }) => (
  <svg className={className} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round" strokeLinejoin="round">
    <line x1="7" y1="17" x2="17" y2="7" />
    <polyline points="8 7 17 7 17 16" />
  </svg>
);

export const CheckIcon: React.FC<{ className?: string; size?: number }> = ({ className, size = 12 }) => (
  <svg className={className} width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="20 6 9 17 4 12" />
  </svg>
);

export const PlusIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round">
    <line x1="12" y1="5" x2="12" y2="19" />
    <line x1="5" y1="12" x2="19" y2="12" />
  </svg>
);

export const EditPencilIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <EditIcon className={className} />
);

export const AppsGridIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round">
    <rect x="3" y="3" width="7" height="7" rx="1.6" />
    <rect x="14" y="3" width="7" height="7" rx="1.6" />
    <rect x="3" y="14" width="7" height="7" rx="1.6" />
    <rect x="14" y="14" width="7" height="7" rx="1.6" />
  </svg>
);

export const PowerIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
    <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
    <line x1="12" y1="2" x2="12" y2="12" />
  </svg>
);

export const MouseIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65">
    <rect x="6" y="3" width="12" height="18" rx="6" />
    <line x1="12" y1="7" x2="12" y2="10" strokeLinecap="round" />
  </svg>
);

export const HeadphonesIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
    <path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3zM3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3z" />
  </svg>
);

export const MicIcon: React.FC<{ className?: string; muted?: boolean }> = ({
  className = 'ic16',
  muted = false,
}) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 1.5a3 3 0 0 0-3 3v7.5a3 3 0 0 0 6 0V4.5a3 3 0 0 0-3-3z" />
    <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
    <line x1="12" y1="19" x2="12" y2="22.5" />
    {muted && <line x1="3" y1="3" x2="21" y2="21" stroke="var(--danger, #F26378)" strokeWidth="1.9" />}
  </svg>
);

export const RestoreWindowsIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round">
    <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
    <polyline points="3 3 3 8 8 8" />
  </svg>
);

export const ArrowUpIcon: React.FC<{ size?: number }> = ({ size = 10 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.1" strokeLinecap="round" strokeLinejoin="round">
    <line x1="12" y1="19" x2="12" y2="5" />
    <polyline points="5 12 12 5 19 12" />
  </svg>
);

export const ArrowDownIcon: React.FC<{ size?: number }> = ({ size = 10 }) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.1" strokeLinecap="round" strokeLinejoin="round">
    <line x1="12" y1="5" x2="12" y2="19" />
    <polyline points="19 12 12 19 5 12" />
  </svg>
);

export const FallbackAppIcon: React.FC<{ iconKey?: string; kind?: string; size?: number }> = ({
  iconKey = '',
  kind = 'app',
  size = 22,
}) => {
  if (kind === 'folder') return <FolderSvg className="ic18" color="#F2B44B" />;
  if (kind === 'file') return <FileSvg className="ic18" color="#52B8F2" />;
  if (kind === 'url') return <GlobeSvg className="ic18" color="#A78BFA" />;

  const k = iconKey.toLowerCase();
  if (k.includes('zen')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <circle cx="12" cy="12" r="9" stroke="#F97316" strokeWidth="2.1" />
        <circle cx="12" cy="12" r="4.8" stroke="#FB923C" strokeWidth="2" />
        <circle cx="12" cy="12" r="1.8" fill="#FDBA74" />
      </svg>
    );
  }
  if (k.includes('telegram')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <circle cx="12" cy="12" r="9.5" fill="#29A9EB" />
        <path d="M16.8 8.2L6.6 12.1c-.7.3-.7 1.1-.1 1.3l2.6.8 6-3.8-4.6 4.4-.2 2.4c.4 0 .6-.2.9-.4l1.4-1.3 2.7 2c.5.3.9.1 1-.5l1.8-8.1c.2-.7-.3-1.1-1.3-.7z" fill="#fff" />
      </svg>
    );
  }
  if (k.includes('explorer')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <path d="M3 7a2 2 0 0 1 2-2h4.2l2 2H19a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" fill="#E59A2B" />
        <rect x="3" y="9" width="18" height="10" rx="2" fill="#F2B44B" />
      </svg>
    );
  }
  if (k.includes('vscode') || k.includes('code')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <path d="M17.5 3.5L7.8 12l9.7 8.5L21 18.8V5.2l-3.5-1.7z" fill="#0EA5E9" />
        <path d="M7.8 12L4 8.8 2.8 10l2.8 2-2.8 2L4 15.2 7.8 12z" fill="#52B8F2" />
      </svg>
    );
  }
  if (k.includes('steam')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <circle cx="12" cy="12" r="9.5" fill="#172554" />
        <circle cx="15" cy="9.5" r="2.7" stroke="#60A5FA" strokeWidth="1.7" />
        <circle cx="8.5" cy="14.8" r="2.1" fill="#60A5FA" />
        <line x1="10.2" y1="13.8" x2="13.1" y2="11.3" stroke="#60A5FA" strokeWidth="2" />
      </svg>
    );
  }
  if (k.includes('discord')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <rect x="3" y="4" width="18" height="16" rx="5" fill="#5865F2" />
        <circle cx="9.2" cy="12.2" r="1.5" fill="#fff" />
        <circle cx="14.8" cy="12.2" r="1.5" fill="#fff" />
      </svg>
    );
  }
  if (k.includes('obsidian')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <path d="M12 2.5L18.5 7l-1.8 11.5-4.7 3-5.2-3.8L6.2 8.2 12 2.5z" fill="#8B5CF6" />
        <path d="M12 2.5L18.5 7l-6.5 4.2V2.5z" fill="#C4B5FD" />
      </svg>
    );
  }
  if (k.includes('ymusic') || k.includes('music')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <circle cx="12" cy="12" r="9.5" fill="#F2C94C" />
        <path d="M14.5 7.5v7a2 2 0 1 1-1.5-1.9V9.3l-3.5 1v4.7a2 2 0 1 1-1.5-1.9V8.8l6.5-1.3z" fill="#14161D" />
      </svg>
    );
  }
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
      <rect x="3.5" y="3.5" width="17" height="17" rx="5" fill="rgba(139,150,248,0.16)" stroke="#8B96F8" strokeWidth="1.6" />
      <circle cx="12" cy="12" r="3" fill="#8B96F8" />
    </svg>
  );
};
