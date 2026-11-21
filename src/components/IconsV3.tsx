import React from 'react';
import type { WeatherConditionKey } from '../lib/ipc';

export interface TileIconOption {
  id: string;
  label: string;
  render: (className?: string) => React.ReactElement;
}

const FOLDER_BASE_D =
  'M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z';

function FolderGlyph({
  className = 'ic16',
  children,
}: {
  className?: string;
  children?: React.ReactNode;
}) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.65"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={FOLDER_BASE_D} />
      {children}
    </svg>
  );
}

export const TILE_ICON_CATALOG: readonly TileIconOption[] = [
  {
    id: 'folder',
    label: 'Папка',
    render: (className = 'ic16') => <FolderGlyph className={className} />,
  },
  {
    id: 'folder-code',
    label: 'Код',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <polyline points="10 11.5 8 13.5 10 15.5" />
        <polyline points="14 11.5 16 13.5 14 15.5" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-media',
    label: 'Медиа',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <polygon points="10.5 11 15 13.5 10.5 16 10.5 11" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-downloads',
    label: 'Загрузки',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <line x1="12" y1="10.5" x2="12" y2="16" />
        <polyline points="9.5 13.5 12 16 14.5 13.5" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-docs',
    label: 'Документы',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <line x1="8.5" y1="12" x2="15.5" y2="12" />
        <line x1="8.5" y1="15.5" x2="13.5" y2="15.5" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-games',
    label: 'Игры',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <line x1="9" y1="12" x2="9" y2="15" />
        <line x1="7.5" y1="13.5" x2="10.5" y2="13.5" />
        <circle cx="14.5" cy="12.5" r="0.6" fill="currentColor" stroke="none" />
        <circle cx="16.2" cy="14.2" r="0.6" fill="currentColor" stroke="none" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-star',
    label: 'Избранное',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <polygon points="12 10 12.95 12.1 15.2 12.35 13.5 13.85 13.95 16.1 12 14.95 10.05 16.1 10.5 13.85 8.8 12.35 11.05 12.1 12 10" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-work',
    label: 'Работа',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <rect x="8.5" y="11" width="7" height="5" rx="1" />
        <path d="M10.5 11v-1h3v1" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-music',
    label: 'Музыка',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <path d="M11 15.5v-4.2l3.5-.8v3.8" />
        <circle cx="9.8" cy="15.5" r="1.1" />
        <circle cx="13.3" cy="14.5" r="1.1" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-video',
    label: 'Видео',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <rect x="7.5" y="10.8" width="6" height="4.8" rx="1" />
        <polygon points="13.5 12.3 16.5 10.8 16.5 15.6 13.5 14.1" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-archive',
    label: 'Архив',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <line x1="12" y1="10" x2="12" y2="16.5" />
        <line x1="10" y1="11.5" x2="12" y2="11.5" />
        <line x1="12" y1="13.5" x2="14" y2="13.5" />
        <line x1="10" y1="15.5" x2="12" y2="15.5" />
      </FolderGlyph>
    ),
  },
  {
    id: 'folder-heart',
    label: 'Личное',
    render: (className = 'ic16') => (
      <FolderGlyph className={className}>
        <path d="M12 16.2s-3.2-2-3.2-3.9a1.8 1.8 0 0 1 3.2-1.1 1.8 1.8 0 0 1 3.2 1.1c0 1.9-3.2 3.9-3.2 3.9z" />
      </FolderGlyph>
    ),
  },
  {
    id: 'terminal',
    label: 'Терминал',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <rect x="3" y="4" width="18" height="16" rx="2.5" />
        <polyline points="7.5 9.5 10.5 12 7.5 14.5" />
        <line x1="12.5" y1="14.5" x2="16.5" y2="14.5" />
      </svg>
    ),
  },
  {
    id: 'globe',
    label: 'Веб',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.65"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="9.5" />
        <line x1="2.5" y1="12" x2="21.5" y2="12" />
        <path d="M12 2.5a14.5 14.5 0 0 1 3.8 9.5 14.5 14.5 0 0 1-3.8 9.5 14.5 14.5 0 0 1-3.8-9.5A14.5 14.5 0 0 1 12 2.5z" />
      </svg>
    ),
  },
  {
    id: 'gamepad',
    label: 'Геймпад',
    render: (className = 'ic16') => <GamepadIcon className={className} />,
  },
  {
    id: 'layers',
    label: 'Слои',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.65"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <polygon points="12 3 21 8 12 13 3 8 12 3" />
        <polyline points="3 12.5 12 17.5 21 12.5" />
        <polyline points="3 17 12 22 21 17" />
      </svg>
    ),
  },
  {
    id: 'cpu',
    label: 'Система',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.65"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <rect x="6" y="6" width="12" height="12" rx="2" />
        <rect x="9.5" y="9.5" width="5" height="5" rx="0.8" />
        <line x1="9" y1="2.5" x2="9" y2="6" />
        <line x1="15" y1="2.5" x2="15" y2="6" />
        <line x1="9" y1="18" x2="9" y2="21.5" />
        <line x1="15" y1="18" x2="15" y2="21.5" />
        <line x1="2.5" y1="9" x2="6" y2="9" />
        <line x1="2.5" y1="15" x2="6" y2="15" />
        <line x1="18" y1="9" x2="21.5" y2="9" />
        <line x1="18" y1="15" x2="21.5" y2="15" />
      </svg>
    ),
  },
  {
    id: 'bookmark',
    label: 'Закладка',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.65"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M6 4.5A1.5 1.5 0 0 1 7.5 3h9A1.5 1.5 0 0 1 18 4.5V21l-6-4-6 4V4.5z" />
      </svg>
    ),
  },
  {
    id: 'bolt',
    label: 'Молния',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.7"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <polygon points="13 2 4.5 13.5 11.5 13.5 10.5 22 19.5 10.5 12.5 10.5 13 2" />
      </svg>
    ),
  },
  {
    id: 'camera',
    label: 'Камера',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.65"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M21 18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8.5a2 2 0 0 1 2-2h3l1.6-2.5h4.8L16 6.5h3a2 2 0 0 1 2 2V18z" />
        <circle cx="12" cy="13" r="3.4" />
      </svg>
    ),
  },
  {
    id: 'wrench',
    label: 'Инструменты',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.65"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M14.7 6.3a4.5 4.5 0 0 0-5.8 5.8l-5.2 5.2a1.6 1.6 0 0 0 2.3 2.3l5.2-5.2a4.5 4.5 0 0 0 5.8-5.8l-2.6 2.6-2.3-2.3 2.6-2.6z" />
      </svg>
    ),
  },
  {
    id: 'briefcase',
    label: 'Портфель',
    render: (className = 'ic16') => (
      <svg
        className={className}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.65"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <rect x="3" y="7" width="18" height="13" rx="2.5" />
        <path d="M8.5 7V5a1.5 1.5 0 0 1 1.5-1.5h4A1.5 1.5 0 0 1 15.5 5v2" />
        <line x1="3" y1="12.5" x2="21" y2="12.5" />
      </svg>
    ),
  },
];

const TILE_ICON_MAP = new Map<string, TileIconOption>(
  TILE_ICON_CATALOG.map((item) => [item.id, item]),
);

export function renderCustomTileIcon(
  id: string | null | undefined,
  className = 'ic16',
): React.ReactElement | null {
  const key = id?.trim();
  if (!key || key === 'custom_image') return null;
  const entry = TILE_ICON_MAP.get(key);
  return entry ? entry.render(className) : null;
}

/* ===================== WEATHER ICONS ===================== */

export const WindIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M3 8h11.5a2.5 2.5 0 1 0-2.5-2.5" />
    <path d="M3 12h15.5a2.5 2.5 0 1 1-2.5 2.5" />
    <path d="M3 16h8.5a2 2 0 1 1-2 2" />
  </svg>
);

export const DropletIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M12 2.8S6 10 6 14.5a6 6 0 0 0 12 0C18 10 12 2.8 12 2.8z" />
  </svg>
);

export const WeatherConditionIcon: React.FC<{
  condition?: WeatherConditionKey | null;
  className?: string;
}> = ({ condition = 'cloud', className = 'ic16' }) => {
  switch (condition) {
    case 'sun':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <circle cx="12" cy="12" r="4.2" />
          <line x1="12" y1="2.5" x2="12" y2="4.8" />
          <line x1="12" y1="19.2" x2="12" y2="21.5" />
          <line x1="2.5" y1="12" x2="4.8" y2="12" />
          <line x1="19.2" y1="12" x2="21.5" y2="12" />
          <line x1="5.3" y1="5.3" x2="6.9" y2="6.9" />
          <line x1="17.1" y1="17.1" x2="18.7" y2="18.7" />
          <line x1="18.7" y1="5.3" x2="17.1" y2="6.9" />
          <line x1="6.9" y1="17.1" x2="5.3" y2="18.7" />
        </svg>
      );
    case 'moon':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M20.2 14.6A8.5 8.5 0 0 1 9.4 3.8a8.5 8.5 0 1 0 10.8 10.8z" />
        </svg>
      );
    case 'partly-cloudy-day':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M12.5 5.2a3.3 3.3 0 0 1 5.7 2.8" />
          <line x1="15" y1="2.3" x2="15" y2="3.5" />
          <line x1="19.6" y1="4.4" x2="18.7" y2="5.3" />
          <line x1="21.5" y1="9" x2="20.2" y2="9" />
          <path d="M17.5 18.5H7.2a4.2 4.2 0 0 1-.4-8.38 5.2 5.2 0 0 1 9.9-1.12 3.8 3.8 0 0 1 .8 7.5z" />
        </svg>
      );
    case 'partly-cloudy-night':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M18.6 9.8a3.6 3.6 0 0 1-4.3-4.6 3.7 3.7 0 0 0-2 3.2" />
          <path d="M17.5 18.5H7.2a4.2 4.2 0 0 1-.4-8.38 5.2 5.2 0 0 1 9.9-1.12 3.8 3.8 0 0 1 .8 7.5z" />
        </svg>
      );
    case 'fog':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M18 13.5H7a4 4 0 0 1-.4-7.98 5 5 0 0 1 9.5-1.02A3.6 3.6 0 0 1 18 11.5" />
          <line x1="4" y1="16.5" x2="20" y2="16.5" />
          <line x1="6" y1="20" x2="18" y2="20" />
        </svg>
      );
    case 'drizzle':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M17.5 14.5H7.2a4.2 4.2 0 0 1-.4-8.38 5.2 5.2 0 0 1 9.9-1.12 3.8 3.8 0 0 1 .8 7.5z" />
          <line x1="8.5" y1="17.5" x2="7.7" y2="19.8" />
          <line x1="12.5" y1="17.5" x2="11.7" y2="19.8" />
          <line x1="16.5" y1="17.5" x2="15.7" y2="19.8" />
        </svg>
      );
    case 'rain':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M17.5 14H7.2a4.2 4.2 0 0 1-.4-8.38 5.2 5.2 0 0 1 9.9-1.12 3.8 3.8 0 0 1 .8 7.5z" />
          <line x1="8.2" y1="16.8" x2="6.8" y2="21" />
          <line x1="12.5" y1="16.8" x2="11.1" y2="21" />
          <line x1="16.8" y1="16.8" x2="15.4" y2="21" />
        </svg>
      );
    case 'snow':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M17.5 14.5H7.2a4.2 4.2 0 0 1-.4-8.38 5.2 5.2 0 0 1 9.9-1.12 3.8 3.8 0 0 1 .8 7.5z" />
          <circle cx="8" cy="18" r="1" fill="currentColor" stroke="none" />
          <circle cx="12" cy="19.8" r="1" fill="currentColor" stroke="none" />
          <circle cx="16" cy="18" r="1" fill="currentColor" stroke="none" />
        </svg>
      );
    case 'storm':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M17.5 14.5H7.2a4.2 4.2 0 0 1-.4-8.38 5.2 5.2 0 0 1 9.9-1.12 3.8 3.8 0 0 1 .8 7.5z" />
          <polyline points="13 14.5 10.2 18.2 13.5 18.2 11 21.8" />
        </svg>
      );
    case 'cloud':
    default:
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M17.5 18H7.2a4.4 4.4 0 0 1-.4-8.78 5.4 5.4 0 0 1 10.3-1.12A3.9 3.9 0 0 1 17.5 18z" />
        </svg>
      );
  }
};

/* ===================== FILE EXPLORER ICONS ===================== */

export const ChevronRightIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.9"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <polyline points="9 18 15 12 9 6" />
  </svg>
);

export const ArrowLeftIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.8"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <line x1="19" y1="12" x2="5" y2="12" />
    <polyline points="11 6 5 12 11 18" />
  </svg>
);

export const ArrowRightIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.8"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <line x1="5" y1="12" x2="19" y2="12" />
    <polyline points="13 6 19 12 13 18" />
  </svg>
);

export const GridViewIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="3.5" y="3.5" width="7" height="7" rx="1.5" />
    <rect x="13.5" y="3.5" width="7" height="7" rx="1.5" />
    <rect x="3.5" y="13.5" width="7" height="7" rx="1.5" />
    <rect x="13.5" y="13.5" width="7" height="7" rx="1.5" />
  </svg>
);

export const ListViewIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.75"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <line x1="8.5" y1="6.5" x2="20" y2="6.5" />
    <line x1="8.5" y1="12" x2="20" y2="12" />
    <line x1="8.5" y1="17.5" x2="20" y2="17.5" />
    <circle cx="4.5" cy="6.5" r="1" fill="currentColor" stroke="none" />
    <circle cx="4.5" cy="12" r="1" fill="currentColor" stroke="none" />
    <circle cx="4.5" cy="17.5" r="1" fill="currentColor" stroke="none" />
  </svg>
);

export const SortIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.75"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <polyline points="7 7 4 4 4 20" />
    <line x1="11" y1="6" x2="20" y2="6" />
    <line x1="11" y1="12" x2="17.5" y2="12" />
    <line x1="11" y1="18" x2="15" y2="18" />
  </svg>
);

export const FolderPlusIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d={FOLDER_BASE_D} />
    <line x1="12" y1="10.5" x2="12" y2="16.5" />
    <line x1="9" y1="13.5" x2="15" y2="13.5" />
  </svg>
);

export const RenameIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M16 4h2a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-2" />
    <path d="M8 20H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
    <line x1="12" y1="3" x2="12" y2="21" />
    <line x1="9" y1="3" x2="15" y2="3" />
    <line x1="9" y1="21" x2="15" y2="21" />
  </svg>
);

export const DriveIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="3" y="5" width="18" height="14" rx="2.5" />
    <line x1="3" y1="14.5" x2="21" y2="14.5" />
    <circle cx="17" cy="17" r="0.9" fill="currentColor" stroke="none" />
  </svg>
);

export const HomeIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M3 10.5L12 3l9 7.5" />
    <path d="M5.5 9.5V20a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1V9.5" />
    <path d="M9.5 21v-6.5h5V21" />
  </svg>
);

export const FileImageIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="3.5" y="4" width="17" height="16" rx="2.5" />
    <circle cx="8.8" cy="9.2" r="1.4" />
    <polyline points="20.5 15.5 15.5 10.5 6.5 19.5" />
  </svg>
);

export const FileVideoIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="3" y="5" width="13.5" height="14" rx="2.2" />
    <polygon points="16.5 9.5 21 7 21 17 16.5 14.5" />
  </svg>
);

export const FileAudioIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M9 17.5V6l11-2v11.5" />
    <circle cx="6.5" cy="17.5" r="2.5" />
    <circle cx="17.5" cy="15.5" r="2.5" />
  </svg>
);

export const FileCodeIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <polyline points="8.5 8 3.5 12 8.5 16" />
    <polyline points="15.5 8 20.5 12 15.5 16" />
    <line x1="13.5" y1="5.5" x2="10.5" y2="18.5" />
  </svg>
);

export const FileArchiveIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="4" y="3.5" width="16" height="17" rx="2.5" />
    <line x1="12" y1="3.5" x2="12" y2="15" />
    <line x1="10" y1="7" x2="12" y2="7" />
    <line x1="12" y1="10" x2="14" y2="10" />
    <line x1="10" y1="13" x2="12" y2="13" />
    <rect x="10" y="15" width="4" height="3" rx="0.8" />
  </svg>
);

export const FileDocIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M14 2.5H6.5A2 2 0 0 0 4.5 4.5v15a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2.5 14 8 19.5 8" />
    <line x1="8.5" y1="12.5" x2="15.5" y2="12.5" />
    <line x1="8.5" y1="16.5" x2="13.5" y2="16.5" />
  </svg>
);

export const FileExeIcon: React.FC<{ className?: string }> = ({ className = 'ic16' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="3.5" y="4.5" width="17" height="15" rx="2.5" />
    <line x1="3.5" y1="9.5" x2="20.5" y2="9.5" />
    <polyline points="8.5 13 11 15 8.5 17" />
    <line x1="13" y1="17" x2="16" y2="17" />
  </svg>
);

export const EyeIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M2 12s3.8-6.5 10-6.5S22 12 22 12s-3.8 6.5-10 6.5S2 12 2 12z" />
    <circle cx="12" cy="12" r="2.7" />
  </svg>
);

/* ===================== PERIPHERAL ICONS ===================== */

export const KeyboardPeripheralIcon: React.FC<{ className?: string }> = ({
  className = 'ic14',
}) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="2.5" y="5.5" width="19" height="13" rx="2.5" />
    <line x1="6.5" y1="9.5" x2="8" y2="9.5" />
    <line x1="11.25" y1="9.5" x2="12.75" y2="9.5" />
    <line x1="16" y1="9.5" x2="17.5" y2="9.5" />
    <line x1="8" y1="14.5" x2="16" y2="14.5" />
  </svg>
);

export const GamepadIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M6.5 8h11a4.2 4.2 0 0 1 4.1 5.1l-.8 3.6a2.3 2.3 0 0 1-4 1l-1.9-2.1H9.1l-1.9 2.1a2.3 2.3 0 0 1-4-1l-.8-3.6A4.2 4.2 0 0 1 6.5 8z" />
    <line x1="8.5" y1="10.8" x2="8.5" y2="13.8" />
    <line x1="7" y1="12.3" x2="10" y2="12.3" />
    <circle cx="15.3" cy="11.4" r="0.8" fill="currentColor" stroke="none" />
    <circle cx="17.1" cy="13.1" r="0.8" fill="currentColor" stroke="none" />
  </svg>
);

export const BluetoothIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.7"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <polyline points="7 7.5 17 16.5 12 21 12 3 17 7.5 7 16.5" />
  </svg>
);

export const ChargingBoltIcon: React.FC<{ className?: string }> = ({ className = 'ic12' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.9"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <polygon points="13 2.5 5.5 13 11.8 13 10.8 21.5 18.5 11 12.2 11 13 2.5" />
  </svg>
);

export const PeripheralDeviceIcon: React.FC<{
  kind?: string | null;
  className?: string;
}> = ({ kind, className = 'ic14' }) => {
  switch ((kind ?? '').toLowerCase()) {
    case 'mouse':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <rect x="6" y="3" width="12" height="18" rx="6" />
          <line x1="12" y1="7" x2="12" y2="10" />
        </svg>
      );
    case 'headphones':
    case 'audio':
      return (
        <svg
          className={className}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.65"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
          <path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3zM3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3z" />
        </svg>
      );
    case 'keyboard':
      return <KeyboardPeripheralIcon className={className} />;
    case 'gamepad':
      return <GamepadIcon className={className} />;
    default:
      return <BluetoothIcon className={className} />;
  }
};

/* ===================== V3.2 WIDGET & SETTINGS STUDIO ICONS ===================== */

export const NoteChecklistIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="4" y="3.5" width="16" height="17" rx="2.8" />
    <polyline points="8 9 9.5 10.5 12 7.8" />
    <line x1="14" y1="9.2" x2="16.8" y2="9.2" />
    <line x1="8" y1="13.5" x2="16.8" y2="13.5" />
    <line x1="8" y1="17" x2="13.5" y2="17" />
  </svg>
);

export const TimerIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <line x1="9.5" y1="2.5" x2="14.5" y2="2.5" />
    <line x1="12" y1="2.5" x2="12" y2="5" />
    <circle cx="12" cy="13.5" r="7.5" />
    <polyline points="12 9.5 12 13.5 15 15.2" />
  </svg>
);

export const NetworkSpeedIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.7"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M8 5v14" />
    <polyline points="4.8 15.5 8 19 11.2 15.5" />
    <path d="M16 19V5" />
    <polyline points="12.8 8.5 16 5 19.2 8.5" />
  </svg>
);

export const BoltToggleIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <rect x="2.5" y="6.5" width="19" height="11" rx="5.5" />
    <circle cx="15.8" cy="12" r="2.8" fill="currentColor" stroke="none" />
  </svg>
);

export const CaffeineIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M17 8h1.5a3 3 0 0 1 0 6H17" />
    <path d="M4.5 8h12.5v6.2a4.5 4.5 0 0 1-4.5 4.5H9a4.5 4.5 0 0 1-4.5-4.5V8z" />
    <line x1="8" y1="3" x2="8" y2="5.2" />
    <line x1="12" y1="2.5" x2="12" y2="5.2" />
  </svg>
);

export const CameraSnipIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M4 8.5V5.5A1.5 1.5 0 0 1 5.5 4h3" />
    <path d="M15.5 4h3A1.5 1.5 0 0 1 20 5.5v3" />
    <path d="M20 15.5v3a1.5 1.5 0 0 1-1.5 1.5h-3" />
    <path d="M8.5 20h-3A1.5 1.5 0 0 1 4 18.5v-3" />
    <circle cx="12" cy="12" r="3.2" />
  </svg>
);

export const PaletteIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M12 3a9 9 0 0 0 0 18 1.8 1.8 0 0 0 1.8-1.8c0-.5-.2-.9-.5-1.2-.3-.3-.5-.7-.5-1.2a1.8 1.8 0 0 1 1.8-1.8H17a4 4 0 0 0 4-4c0-4.97-4.03-8-9-8z" />
    <circle cx="8" cy="10.5" r="1" fill="currentColor" stroke="none" />
    <circle cx="11.5" cy="7.5" r="1" fill="currentColor" stroke="none" />
    <circle cx="15.5" cy="9.5" r="1" fill="currentColor" stroke="none" />
  </svg>
);

export const SparklesOffIcon: React.FC<{ className?: string }> = ({ className = 'ic14' }) => (
  <svg
    className={className}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.65"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M12 3l1.6 4.7L18 9.2l-4.4 1.5L12 15.5l-1.6-4.8L6 9.2l4.4-1.5L12 3z" />
    <line x1="3.5" y1="3.5" x2="20.5" y2="20.5" />
  </svg>
);
