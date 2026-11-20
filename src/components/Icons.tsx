import React from 'react';

export const StartLogoIcon: React.FC<{ className?: string }> = ({ className = 'ic24' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none">
    <rect x="3" y="3" width="8.2" height="8.2" rx="2.6" fill="url(#tb_g1)" />
    <rect x="12.8" y="3" width="8.2" height="8.2" rx="2.6" fill="url(#tb_g2)" />
    <rect x="3" y="12.8" width="8.2" height="8.2" rx="2.6" fill="url(#tb_g3)" />
    <rect x="12.8" y="12.8" width="8.2" height="8.2" rx="2.6" fill="url(#tb_g4)" />
    <defs>
      <linearGradient id="tb_g1" x1="3" y1="3" x2="11" y2="11" gradientUnits="userSpaceOnUse"><stop stopColor="#A5B4FC"/><stop offset="1" stopColor="#6366F1"/></linearGradient>
      <linearGradient id="tb_g2" x1="13" y1="3" x2="21" y2="11" gradientUnits="userSpaceOnUse"><stop stopColor="#7DD3FC"/><stop offset="1" stopColor="#38BDF8"/></linearGradient>
      <linearGradient id="tb_g3" x1="3" y1="13" x2="11" y2="21" gradientUnits="userSpaceOnUse"><stop stopColor="#818CF8"/><stop offset="1" stopColor="#A855F7"/></linearGradient>
      <linearGradient id="tb_g4" x1="13" y1="13" x2="21" y2="21" gradientUnits="userSpaceOnUse"><stop stopColor="#38BDF8"/><stop offset="1" stopColor="#6366F1"/></linearGradient>
    </defs>
  </svg>
);

export const XIcon10: React.FC = () => (
  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round">
    <line x1="2" y1="2" x2="8" y2="8" />
    <line x1="8" y1="2" x2="2" y2="8" />
  </svg>
);

export const KillIcon: React.FC = () => (
  <svg className="ic14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
    <circle cx="12" cy="12" r="9" />
    <line x1="15" y1="9" x2="9" y2="15" />
    <line x1="9" y1="9" x2="15" y2="15" />
  </svg>
);

export const ChevronLeftIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.1" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="15 18 9 12 15 6" />
  </svg>
);

export const ChevronRightIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.1" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="9 18 15 12 9 6" />
  </svg>
);

export const BellIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.85" strokeLinecap="round" strokeLinejoin="round">
    <path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9" />
    <path d="M13.73 21a2 2 0 0 1-3.46 0" />
  </svg>
);

export const NativeTrayIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <line x1="3" y1="15" x2="21" y2="15" />
    <polyline points="9 11 12 8 15 11" />
  </svg>
);

export const ExternalWindowIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <line x1="3" y1="9" x2="21" y2="9" />
    <line x1="12" y1="12.5" x2="12" y2="16.5" />
    <line x1="10" y1="14.5" x2="14" y2="14.5" />
  </svg>
);

export const ShieldIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 3l7 3v5c0 4.6-3 8.5-7 10-4-1.5-7-5.4-7-10V6l7-3z" />
    <polyline points="9.5 12.2 11.2 14 14.8 10.2" />
  </svg>
);

export const PinIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M12 17v5" />
    <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a1 1 0 0 0 0-2H8a1 1 0 0 0 0 2h1v4.76z" />
  </svg>
);

export const CloseWindowIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round">
    <line x1="6" y1="6" x2="18" y2="18" />
    <line x1="18" y1="6" x2="6" y2="18" />
  </svg>
);

export const ClockRecentIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="9" />
    <polyline points="12 7 12 12 15.5 14" />
  </svg>
);

export const TaskmgrIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <polyline points="6 14 9.5 14 11.5 9.5 14 16 15.8 12.5 18 12.5" />
  </svg>
);

export const LayoutBarIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" />
    <line x1="6" y1="16.5" x2="18" y2="16.5" />
  </svg>
);

export const SettingsGearIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.08a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.08a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.08a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </svg>
);

export const VolIcon: React.FC<{ muted?: boolean; className?: string }> = ({ muted, className = 'ic14' }) =>
  muted ? (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
      <line x1="23" y1="9" x2="17" y2="15" />
      <line x1="17" y1="9" x2="23" y2="15" />
    </svg>
  ) : (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
      <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
    </svg>
  );

export const FolderSvg: React.FC<{ className?: string; color?: string }> = ({ className = 'ic16', color = 'currentColor' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
  </svg>
);

export const FileSvg: React.FC<{ className?: string; color?: string }> = ({ className = 'ic16', color = 'currentColor' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
  </svg>
);

export const GlobeSvg: React.FC<{ className?: string; color?: string }> = ({ className = 'ic16', color = 'currentColor' }) => (
  <svg className={className} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.8" strokeLinecap="round">
    <circle cx="12" cy="12" r="10" />
    <line x1="2" y1="12" x2="22" y2="12" />
    <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
  </svg>
);

export const FallbackAppIcon: React.FC<{ iconKey?: string; kind?: string; size?: number }> = ({
  iconKey = '',
  kind = 'app',
  size = 22,
}) => {
  if (kind === 'folder') return <FolderSvg className="ic18" color="#FBBF24" />;
  if (kind === 'file') return <FileSvg className="ic18" color="#38BDF8" />;
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
        <path d="M3 7a2 2 0 0 1 2-2h4.2l2 2H19a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" fill="#F59E0B" />
        <rect x="3" y="9" width="18" height="10" rx="2" fill="#FBBF24" />
      </svg>
    );
  }
  if (k.includes('vscode') || k.includes('code')) {
    return (
      <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
        <path d="M17.5 3.5L7.8 12l9.7 8.5L21 18.8V5.2l-3.5-1.7z" fill="#0EA5E9" />
        <path d="M7.8 12L4 8.8 2.8 10l2.8 2-2.8 2L4 15.2 7.8 12z" fill="#38BDF8" />
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
        <circle cx="12" cy="12" r="9.5" fill="#FACC15" />
        <path d="M14.5 7.5v7a2 2 0 1 1-1.5-1.9V9.3l-3.5 1v4.7a2 2 0 1 1-1.5-1.9V8.8l6.5-1.3z" fill="#18181B" />
      </svg>
    );
  }
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none">
      <rect x="3.5" y="3.5" width="17" height="17" rx="5" fill="rgba(129,140,248,0.18)" stroke="#818CF8" strokeWidth="1.6" />
      <circle cx="12" cy="12" r="3.2" fill="#818CF8" />
    </svg>
  );
};
