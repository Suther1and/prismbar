import React, { useMemo, useRef, useState } from 'react';
import {
  isCacheIconKey,
  type IndexedApp,
  type PinItem,
  type TestBarConfig,
} from '../../lib/ipc';
import { matchesQuery } from '../../lib/layoutMap';
import {
  getPinnedItemInSpace,
  getSpaceDisplayName,
  normalizeUrlPinInput,
} from '../../lib/startLogic';
import {
  CheckIcon,
  CloseWindowIcon,
  FallbackAppIcon,
  FileSvg,
  FolderSvg,
  GlobeSvg,
  PlusIcon,
  SearchIcon,
} from '../Icons';

export interface StartPinDrawerProps {
  space: string;
  config: TestBarConfig | null;
  apps: IndexedApp[];
  iconMap: Record<string, string>;
  onClose: () => void;
  onPinFolder: () => Promise<void>;
  onPinFile: () => Promise<void>;
  onPinUrl: (url: string, title: string) => Promise<void>;
  onTogglePinApp: (app: IndexedApp, existingPin: PinItem | null) => Promise<void>;
}

export const StartPinDrawer: React.FC<StartPinDrawerProps> = ({
  space,
  config,
  apps,
  iconMap,
  onClose,
  onPinFolder,
  onPinFile,
  onPinUrl,
  onTogglePinApp,
}) => {
  const [filterText, setFilterText] = useState('');
  const [drawerCat, setDrawerCat] = useState('Все');
  const [urlPromptOpen, setUrlPromptOpen] = useState(false);
  const [urlValue, setUrlValue] = useState('');
  const [urlTitle, setUrlTitle] = useState('');
  const [urlError, setUrlError] = useState<string | null>(null);
  const filterInputRef = useRef<HTMLInputElement>(null);

  const spaceName = getSpaceDisplayName(space, config);

  const categories = useMemo(() => {
    const set = new Set<string>(['Все']);
    apps.forEach((a) => set.add(a.category));
    return Array.from(set);
  }, [apps]);

  const filteredApps = useMemo(() => {
    const byCat = drawerCat === 'Все' ? apps : apps.filter((a) => a.category === drawerCat);
    const q = filterText.trim();
    if (!q) return byCat;
    return byCat.filter((a) => matchesQuery(q, a.name, a.category, a.target, a.aumid ?? '').matched);
  }, [apps, drawerCat, filterText]);

  const handleSubmitUrl = async () => {
    const normalized = normalizeUrlPinInput(urlValue, urlTitle);
    if (!normalized) {
      setUrlError('Введите корректный адрес сайта');
      return;
    }
    setUrlError(null);
    await onPinUrl(normalized.url, normalized.title);
    setUrlValue('');
    setUrlTitle('');
    setUrlPromptOpen(false);
  };

  return (
    <div className="all-apps-drawer" onClick={(e) => e.stopPropagation()}>
      {/* Top Bar: Target Space Badge + Live Filter + Clean SVG Close */}
      <div className="drawer-top-bar">
        <div className="drawer-space-badge">
          <span className="drawer-space-dot" />
          <span>Закрепить в: <strong>{spaceName}</strong></span>
        </div>

        <div className="drawer-filter-wrap">
          <SearchIcon className="ic14" color="var(--t3)" />
          <input
            ref={filterInputRef}
            className="drawer-filter-input"
            placeholder="Фильтр программ…"
            value={filterText}
            onChange={(e) => setFilterText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                e.preventDefault();
                e.stopPropagation();
                if (filterText) {
                  setFilterText('');
                } else {
                  onClose();
                }
              }
            }}
          />
          {filterText && (
            <button
              type="button"
              className="drawer-filter-clear"
              aria-label="Очистить фильтр"
              onClick={() => {
                setFilterText('');
                filterInputRef.current?.focus();
              }}
            >
              <CloseWindowIcon className="ic12" />
            </button>
          )}
        </div>

        <span className="drawer-count">{filteredApps.length}</span>

        <button
          type="button"
          className="hdr-btn drawer-close-btn"
          title="Закрыть"
          aria-label="Закрыть"
          onClick={onClose}
        >
          <CloseWindowIcon className="ic14" />
        </button>
      </div>

      {/* Crisp 3-Card Universal Pinning Strip */}
      <div className="pin-strip-grid">
        <button
          type="button"
          className="pin-source-card"
          onClick={() => void onPinFolder()}
        >
          <span className="pin-source-ic amber"><FolderSvg className="ic18" color="#FBBF24" /></span>
          <div className="pin-source-meta">
            <div className="pin-source-title">+ Папку</div>
            <div className="pin-source-sub">Любая директория на диске</div>
          </div>
        </button>

        <button
          type="button"
          className="pin-source-card"
          onClick={() => void onPinFile()}
        >
          <span className="pin-source-ic sky"><FileSvg className="ic18" color="#38BDF8" /></span>
          <div className="pin-source-meta">
            <div className="pin-source-title">+ Файл</div>
            <div className="pin-source-sub">Программа, документ или скрипт</div>
          </div>
        </button>

        <button
          type="button"
          className={`pin-source-card ${urlPromptOpen ? 'active' : ''}`}
          onClick={() => {
            setUrlPromptOpen((v) => !v);
            setUrlError(null);
          }}
        >
          <span className="pin-source-ic violet"><GlobeSvg className="ic18" color="#A78BFA" /></span>
          <div className="pin-source-meta">
            <div className="pin-source-title">+ Ссылку / Сайт</div>
            <div className="pin-source-sub">Веб-сервис или локальный URL</div>
          </div>
        </button>
      </div>

      {/* Clean inline URL + optional Title form */}
      {urlPromptOpen && (
        <div className="url-inline-form">
          <input
            className="url-input url-main"
            autoFocus
            placeholder="https://example.com"
            value={urlValue}
            onChange={(e) => {
              setUrlValue(e.target.value);
              if (urlError) setUrlError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                void handleSubmitUrl();
              } else if (e.key === 'Escape') {
                e.preventDefault();
                e.stopPropagation();
                setUrlPromptOpen(false);
              }
            }}
          />
          <input
            className="url-input url-title"
            placeholder="Название (необязательно)"
            maxLength={32}
            value={urlTitle}
            onChange={(e) => setUrlTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                void handleSubmitUrl();
              } else if (e.key === 'Escape') {
                e.preventDefault();
                e.stopPropagation();
                setUrlPromptOpen(false);
              }
            }}
          />
          <button type="button" className="pin-any-btn primary" onClick={() => void handleSubmitUrl()}>
            Добавить
          </button>
          <button
            type="button"
            className="pin-any-btn"
            onClick={() => {
              setUrlPromptOpen(false);
              setUrlError(null);
            }}
          >
            Отмена
          </button>
          {urlError && <span className="url-inline-err">{urlError}</span>}
        </div>
      )}

      {/* Category Filter Pills */}
      <div className="cat-pills">
        {categories.map((c) => (
          <button
            key={c}
            type="button"
            className={`cat-pill ${drawerCat === c ? 'active' : ''}`}
            onClick={() => setDrawerCat(c)}
          >
            {c}
          </button>
        ))}
      </div>

      {/* 3-Column Scrollable All Apps Grid (Win32 + UWP, dark slim scrollbar, zero horizontal overflow) */}
      <div className="drawer-grid">
        {filteredApps.length === 0 ? (
          <div className="drawer-empty">Программы по этому фильтру не найдены</div>
        ) : (
          filteredApps.map((a) => {
            const existingPin = getPinnedItemInSpace(a, space, config);
            const isPinned = existingPin !== null;
            const cacheKey = isCacheIconKey(a.iconKey) ? a.iconKey : null;
            const resolvedIcon = (cacheKey ? iconMap[cacheKey] : null) || a.iconData || iconMap[a.target] || null;
            const isUwp = Boolean(a.aumid) || a.target.toLowerCase().startsWith('shell:appsfolder\\');
            return (
              <button
                key={a.id}
                type="button"
                className={`drawer-app ${isPinned ? 'pinned' : ''}`}
                onClick={() => void onTogglePinApp(a, existingPin)}
              >
                <span className="drawer-app-ic">
                  {resolvedIcon ? (
                    <img src={resolvedIcon} alt="" className="native-ic" draggable={false} />
                  ) : (
                    <FallbackAppIcon iconKey={a.name} size={20} />
                  )}
                </span>
                <div className="drawer-app-meta">
                  <div className="drawer-app-name">{a.name}</div>
                  <div className="drawer-app-sub">
                    <span>{a.category}</span>
                    {isUwp && <span className="drawer-uwp-tag">UWP</span>}
                  </div>
                </div>
                <span className={`drawer-pin-pill ${isPinned ? 'pinned' : ''}`}>
                  {isPinned ? (
                    <>
                      <span>Закреплено</span>
                      <CheckIcon size={11} />
                    </>
                  ) : (
                    <>
                      <PlusIcon className="ic12" />
                      <span>Закрепить</span>
                    </>
                  )}
                </span>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
};
