import React, { useEffect, useMemo, useRef, useState } from 'react';
import { formatIpcError } from '../../lib/flyoutLogic';
import {
  iconCache,
  isCacheIconKey,
  type IndexedApp,
  type PinItem,
  type TestBarConfig,
} from '../../lib/ipc';
import { matchesQuery } from '../../lib/layoutMap';
import { detectPlatform, type AppPlatform } from '../../lib/platform';
import {
  getPinnedItemInSpace,
  getSpaceDisplayName,
  normalizeUrlPinInput,
} from '../../lib/startLogic';
import { useAnimatedPresence } from '../../lib/useAnimatedPresence';
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
  open: boolean;
  space: string;
  config: TestBarConfig | null;
  apps: IndexedApp[];
  iconMap: Record<string, string>;
  platform?: AppPlatform;
  onClose: () => void;
  onPinFolder: () => Promise<void>;
  onPinFile: () => Promise<void>;
  onPinUrl: (url: string, title: string) => Promise<void>;
  onTogglePinApp: (app: IndexedApp, existingPin: PinItem | null) => Promise<void>;
}

export const StartPinDrawer: React.FC<StartPinDrawerProps> = ({
  open,
  space,
  config,
  apps,
  iconMap,
  platform,
  onClose,
  onPinFolder,
  onPinFile,
  onPinUrl,
  onTogglePinApp,
}) => {
  const isMac = (platform ?? detectPlatform()) === 'macos';
  const { mounted, phase } = useAnimatedPresence(open, 115);

  const [filterText, setFilterText] = useState('');
  const [drawerCat, setDrawerCat] = useState('Все');
  const [urlPromptOpen, setUrlPromptOpen] = useState(false);
  const urlFormPresence = useAnimatedPresence(urlPromptOpen, 100);
  const [urlValue, setUrlValue] = useState('');
  const [urlTitle, setUrlTitle] = useState('');
  const [urlError, setUrlError] = useState<string | null>(null);
  const [submittingUrl, setSubmittingUrl] = useState(false);
  const filterInputRef = useRef<HTMLInputElement>(null);
  const urlInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) {
      setUrlPromptOpen(false);
      setUrlError(null);
      setSubmittingUrl(false);
    }
  }, [open]);

  useEffect(() => {
    if (urlPromptOpen) {
      requestAnimationFrame(() => urlInputRef.current?.focus());
    }
  }, [urlPromptOpen]);

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
    if (submittingUrl) return;
    const normalized = normalizeUrlPinInput(urlValue, urlTitle);
    if (!normalized) {
      setUrlError('Введите корректный адрес сайта');
      urlInputRef.current?.focus();
      return;
    }
    setSubmittingUrl(true);
    setUrlError(null);
    try {
      await onPinUrl(normalized.url, normalized.title);
      setUrlValue('');
      setUrlTitle('');
      setUrlPromptOpen(false);
    } catch (err) {
      setUrlError(formatIpcError(err));
    } finally {
      setSubmittingUrl(false);
    }
  };

  if (!mounted) return null;

  return (
    <div
      className="all-apps-drawer"
      data-phase={phase}
      role="dialog"
      aria-label="Все программы и закрепление"
      onClick={(e) => e.stopPropagation()}
    >
      {/* Top Bar: Target Space Badge + Live Filter + Close */}
      <div className="drawer-top-bar">
        <div className="drawer-space-badge">
          <span className="drawer-space-dot" />
          <span>Пространство: <strong>{spaceName}</strong></span>
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

      {/* Crisp 3-Card Universal Pinning Strip (SVG icon badges, no "+" text prefix) */}
      <div className="pin-strip-grid">
        <button
          type="button"
          className="pin-source-card"
          onClick={() => void onPinFolder()}
        >
          <span className="pin-source-ic amber"><FolderSvg className="ic18" color="#F2B44B" /></span>
          <div className="pin-source-meta">
            <div className="pin-source-title">Папка</div>
            <div className="pin-source-sub">
              {isMac ? 'Любая папка на Mac' : 'Любая директория на диске'}
            </div>
          </div>
        </button>

        <button
          type="button"
          className="pin-source-card"
          onClick={() => void onPinFile()}
        >
          <span className="pin-source-ic sky"><FileSvg className="ic18" color="#52B8F2" /></span>
          <div className="pin-source-meta">
            <div className="pin-source-title">{isMac ? 'Файл / Программа (.app)' : 'Файл'}</div>
            <div className="pin-source-sub">
              {isMac ? 'Приложение .app, документ или скрипт' : 'Программа, документ или скрипт'}
            </div>
          </div>
        </button>

        <button
          type="button"
          className={`pin-source-card ${urlPromptOpen ? 'active' : ''}`}
          aria-expanded={urlPromptOpen}
          onClick={() => {
            setUrlPromptOpen((v) => !v);
            setUrlError(null);
          }}
        >
          <span className="pin-source-ic violet"><GlobeSvg className="ic18" color="#A78BFA" /></span>
          <div className="pin-source-meta">
            <div className="pin-source-title">Ссылка</div>
            <div className="pin-source-sub">Веб-сервис или локальный адрес</div>
          </div>
        </button>
      </div>

      {/* Animated Inline URL Form (unmounted when closed -> zero flex gap, zero layout transition) */}
      {urlFormPresence.mounted && (
        <div
          className={`url-inline-collapse ${urlPromptOpen ? 'open' : ''}`}
          data-phase={urlFormPresence.phase}
        >
          <div className="url-inline-collapse-inner">
            <div className="url-inline-form">
              <input
                ref={urlInputRef}
                className="url-input url-main"
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
              <button
                type="button"
                className="pin-any-btn primary"
                disabled={submittingUrl}
                onClick={() => void handleSubmitUrl()}
              >
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
          </div>
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

      {/* 3-Column Scrollable All Apps Grid */}
      <div className="drawer-grid">
        {filteredApps.length === 0 ? (
          <div className="drawer-empty">Программы по этому фильтру не найдены</div>
        ) : (
          filteredApps.map((a) => {
            const existingPin = getPinnedItemInSpace(a, space, config);
            const isPinned = existingPin !== null;
            const cacheKey = isCacheIconKey(a.iconKey) ? a.iconKey : null;
            const resolvedIcon =
              (cacheKey ? iconMap[cacheKey] || iconCache.get(cacheKey) : null) ||
              a.iconData ||
              iconMap[a.target] ||
              iconMap[a.target.toLowerCase()] ||
              null;
            const isUwp =
              !isMac && (Boolean(a.aumid) || a.target.toLowerCase().startsWith('shell:appsfolder\\'));
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
