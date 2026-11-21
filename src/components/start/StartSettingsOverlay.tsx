import React, { useEffect, useState } from 'react';
import { formatIpcError } from '../../lib/flyoutLogic';
import type { GlassDepth, SecondaryTaskbarMode, TestBarConfig } from '../../lib/ipc';
import { useAnimatedPresence } from '../../lib/useAnimatedPresence';
import {
  AppsGridIcon,
  CheckIcon,
  CloseWindowIcon,
  DownloadIcon,
  LayoutBarIcon,
  RefreshIcon,
  RestoreWindowsIcon,
  SettingsGearIcon,
  SlidersIcon,
  UploadIcon,
  XIcon10,
} from '../Icons';

export interface StartSettingsOverlayProps {
  open: boolean;
  config: TestBarConfig | null;
  onClose: () => void;
  onSetBarMode: (mode: 'dock' | 'edge') => Promise<void>;
  onSetSecondaryMode: (mode: SecondaryTaskbarMode) => Promise<void>;
  onSetAutostart: (enable: boolean) => Promise<void>;
  onSetAppearance: (params: {
    glassDepth?: GlassDepth;
    clockShowSeconds?: boolean;
    clockShowWeekday?: boolean;
    startRememberSpace?: boolean;
  }) => Promise<void>;
  onRefreshIndexes: () => Promise<void>;
  onRestoreTrayApp: (key: string) => Promise<void>;
  onRestoreAllTrayApps: () => Promise<void>;
  onExportConfig: () => Promise<void>;
  onImportConfig: () => Promise<void>;
  onRestoreWindowsAndExit: () => void;
}

type SettingsTabId = 'appearance' | 'bar' | 'start' | 'system';

const SETTINGS_TABS: ReadonlyArray<{
  id: SettingsTabId;
  label: string;
  title: string;
  subtitle: string;
  icon: React.ReactNode;
}> = [
  {
    id: 'appearance',
    label: 'Внешний вид',
    title: 'Внешний вид',
    subtitle: 'Глубина акрилового стекла и формат часов',
    icon: <SlidersIcon className="ic14" />,
  },
  {
    id: 'bar',
    label: 'Панель и мониторы',
    title: 'Панель и мониторы',
    subtitle: 'Форма нижней панели и режим второго экрана',
    icon: <LayoutBarIcon className="ic14" />,
  },
  {
    id: 'start',
    label: 'Меню «Пуск»',
    title: 'Меню «Пуск»',
    subtitle: 'Пространства, индексация и горячие клавиши',
    icon: <AppsGridIcon className="ic14" />,
  },
  {
    id: 'system',
    label: 'Система и трей',
    title: 'Система и трей',
    subtitle: 'Автозапуск, профиль, скрытые значки и возврат',
    icon: <SettingsGearIcon className="ic14" />,
  },
];

const GLASS_OPTIONS: ReadonlyArray<{
  id: GlassDepth;
  label: string;
  hint: string;
}> = [
  { id: 'crystal', label: 'Кристалл', hint: 'Воздушное стекло' },
  { id: 'obsidian', label: 'Обсидиан', hint: 'Сбалансированный' },
  { id: 'matte', label: 'Матовый', hint: 'Плотный контраст' },
];

const HOTKEY_ROWS: ReadonlyArray<{ keys: string[]; label: string }> = [
  { keys: ['Win'], label: 'Открыть или закрыть «Пуск»' },
  { keys: ['Ctrl', '1–9'], label: 'Переход к пространству по номеру' },
  { keys: ['Tab'], label: 'Переключение пространств' },
  { keys: ['Shift'], label: 'Альтернативные действия на плитках' },
  { keys: ['Ctrl', 'Alt', 'Shift', 'Esc'], label: 'Возврат штатной панели Windows' },
];

function formatHiddenTrayLabel(rawKey: string): string {
  const norm = rawKey.trim().replace(/\//g, '\\');
  const base = norm.split('\\').pop() || norm;
  return base.replace(/\.exe$/i, '') || rawKey;
}

export const StartSettingsOverlay: React.FC<StartSettingsOverlayProps> = ({
  open,
  config,
  onClose,
  onSetBarMode,
  onSetSecondaryMode,
  onSetAutostart,
  onSetAppearance,
  onRefreshIndexes,
  onRestoreTrayApp,
  onRestoreAllTrayApps,
  onExportConfig,
  onImportConfig,
  onRestoreWindowsAndExit,
}) => {
  const { mounted, phase } = useAnimatedPresence(open, 115);

  const [activeTab, setActiveTab] = useState<SettingsTabId>('appearance');
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setSettingsError(null);
    }
  }, [open]);

  if (!mounted) return null;

  const barMode = config?.barMode === 'edge' ? 'edge' : 'dock';
  const glassDepth: GlassDepth =
    config?.glassDepth === 'crystal' || config?.glassDepth === 'matte'
      ? config.glassDepth
      : 'obsidian';
  const secondaryMode: SecondaryTaskbarMode = config?.secondaryTaskbarMode ?? 'testbar';
  const autostart = config?.autostart ?? false;
  const clockShowSeconds = config?.clockShowSeconds ?? false;
  const clockShowWeekday = config?.clockShowWeekday ?? false;
  const startRememberSpace = config?.startRememberSpace ?? false;
  const hiddenTrayKeys = config?.hiddenTrayKeys ?? [];

  const activeMeta = SETTINGS_TABS.find((t) => t.id === activeTab) ?? SETTINGS_TABS[0];

  const runAction = async (key: string, fn: () => Promise<void>) => {
    if (busyKey !== null) return;
    setBusyKey(key);
    setSettingsError(null);
    try {
      await fn();
    } catch (err) {
      setSettingsError(formatIpcError(err));
    } finally {
      setBusyKey(null);
    }
  };

  return (
    <div
      className="settings-backdrop"
      data-phase={phase}
      onClick={onClose}
    >
      <div
        className="settings-modal"
        data-phase={phase}
        role="dialog"
        aria-label="Настройки TestBar"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Left Navigation Sidebar */}
        <aside className="set-sidebar" aria-label="Разделы настроек">
          <div className="set-brand">
            <div className="set-brand-title">Настройки</div>
            <div className="set-brand-sub">TestBar</div>
          </div>

          <nav className="set-nav" role="tablist" aria-orientation="vertical">
            {SETTINGS_TABS.map((tab) => {
              const isActive = tab.id === activeTab;
              return (
                <button
                  key={tab.id}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  className={`set-nav-item ${isActive ? 'active' : ''}`}
                  onClick={() => setActiveTab(tab.id)}
                >
                  <span className="set-nav-ic">{tab.icon}</span>
                  <span className="set-nav-lbl">{tab.label}</span>
                </button>
              );
            })}
          </nav>
        </aside>

        {/* Right Category Studio Pane */}
        <section className="set-pane" role="tabpanel" aria-label={activeMeta.title}>
          <header className="set-pane-head">
            <div>
              <h2 className="set-pane-title">{activeMeta.title}</h2>
              <p className="set-pane-sub">{activeMeta.subtitle}</p>
            </div>
            <button
              type="button"
              className="hdr-btn"
              aria-label="Закрыть настройки"
              onClick={onClose}
            >
              <CloseWindowIcon className="ic14" />
            </button>
          </header>

          {settingsError && (
            <div className="set-error-banner" role="alert">
              {settingsError}
            </div>
          )}

          <div className="set-pane-body" key={activeTab}>
            {activeTab === 'appearance' && (
              <>
                <div className="set-group">
                  <div className="set-group-title">Глубина стекла</div>
                  <div className="set-glass-grid" role="radiogroup" aria-label="Глубина стекла">
                    {GLASS_OPTIONS.map((opt) => {
                      const active = glassDepth === opt.id;
                      return (
                        <button
                          key={opt.id}
                          type="button"
                          role="radio"
                          aria-checked={active}
                          disabled={busyKey === 'glassDepth'}
                          className={`set-glass-card ${active ? 'active' : ''}`}
                          onClick={() =>
                            void runAction('glassDepth', () =>
                              onSetAppearance({ glassDepth: opt.id }),
                            )
                          }
                        >
                          {active && (
                            <span className="lp-check" aria-hidden="true">
                              <CheckIcon size={10} />
                            </span>
                          )}
                          <div className={`set-glass-swatch sw-${opt.id}`}>
                            <span className="set-glass-bar" />
                          </div>
                          <div className="set-glass-name">{opt.label}</div>
                          <div className="set-glass-hint">{opt.hint}</div>
                        </button>
                      );
                    })}
                  </div>
                </div>

                <div className="set-group">
                  <div className="set-group-title">Часы на панели</div>
                  <div className="set-toggle-list">
                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">Секунды в часах</div>
                        <div className="set-row-desc">Показывать точное время с секундами</div>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-checked={clockShowSeconds}
                        aria-label="Секунды в часах"
                        disabled={busyKey === 'clockShowSeconds'}
                        className={`glass-switch ${clockShowSeconds ? 'on' : ''}`}
                        onClick={() =>
                          void runAction('clockShowSeconds', () =>
                            onSetAppearance({ clockShowSeconds: !clockShowSeconds }),
                          )
                        }
                      >
                        <span className="glass-switch-thumb" />
                      </button>
                    </div>

                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">День недели рядом с датой</div>
                        <div className="set-row-desc">Короткое обозначение дня недели в часах</div>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-checked={clockShowWeekday}
                        aria-label="День недели рядом с датой"
                        disabled={busyKey === 'clockShowWeekday'}
                        className={`glass-switch ${clockShowWeekday ? 'on' : ''}`}
                        onClick={() =>
                          void runAction('clockShowWeekday', () =>
                            onSetAppearance({ clockShowWeekday: !clockShowWeekday }),
                          )
                        }
                      >
                        <span className="glass-switch-thumb" />
                      </button>
                    </div>
                  </div>
                </div>
              </>
            )}

            {activeTab === 'bar' && (
              <>
                <div className="set-group">
                  <div className="set-group-title">Стиль панели</div>
                  <div className="layout-preview-grid">
                    <button
                      type="button"
                      className={`layout-preview-card ${barMode === 'dock' ? 'active' : ''}`}
                      aria-pressed={barMode === 'dock'}
                      disabled={busyKey === 'barMode'}
                      onClick={() => void runAction('barMode', () => onSetBarMode('dock'))}
                    >
                      {barMode === 'dock' && (
                        <span className="lp-check" aria-hidden="true">
                          <CheckIcon size={10} />
                        </span>
                      )}
                      <div className="lp-screen">
                        <div className="lp-dock-pill">
                          <i className="lp-dot accent" />
                          <i className="lp-dot" />
                          <i className="lp-dot" />
                          <i className="lp-dot" />
                          <span className="lp-sep" />
                          <i className="lp-pill-mini" />
                        </div>
                      </div>
                      <div className="lp-label">Плавающий док</div>
                    </button>

                    <button
                      type="button"
                      className={`layout-preview-card ${barMode === 'edge' ? 'active' : ''}`}
                      aria-pressed={barMode === 'edge'}
                      disabled={busyKey === 'barMode'}
                      onClick={() => void runAction('barMode', () => onSetBarMode('edge'))}
                    >
                      {barMode === 'edge' && (
                        <span className="lp-check" aria-hidden="true">
                          <CheckIcon size={10} />
                        </span>
                      )}
                      <div className="lp-screen">
                        <div className="lp-edge-bar">
                          <div className="lp-edge-group">
                            <i className="lp-dot accent" />
                            <i className="lp-dot" />
                            <i className="lp-dot" />
                          </div>
                          <div className="lp-edge-group">
                            <i className="lp-pill-mini" />
                          </div>
                        </div>
                      </div>
                      <div className="lp-label">На всю ширину</div>
                    </button>
                  </div>
                </div>

                <div className="set-group">
                  <div className="set-group-title">Второй монитор</div>
                  <div className="set-seg-3" role="radiogroup" aria-label="Режим второго монитора">
                    {(
                      [
                        ['testbar', 'Панель TestBar', 'Окна второго экрана'],
                        ['native', 'Штатная Windows', 'Панель Windows'],
                        ['hidden', 'Скрывать', 'Чистый экран'],
                      ] as const
                    ).map(([modeKey, label, hint]) => (
                      <button
                        key={modeKey}
                        type="button"
                        role="radio"
                        aria-checked={secondaryMode === modeKey}
                        className={`set-seg-card ${secondaryMode === modeKey ? 'active' : ''}`}
                        disabled={busyKey === 'secondaryMode'}
                        onClick={() =>
                          void runAction('secondaryMode', () => onSetSecondaryMode(modeKey))
                        }
                      >
                        <span className="set-seg-title">{label}</span>
                        <span className="set-seg-hint">{hint}</span>
                      </button>
                    ))}
                  </div>
                </div>
              </>
            )}

            {activeTab === 'start' && (
              <>
                <div className="set-group">
                  <div className="set-group-title">Поведение и индекс</div>
                  <div className="set-toggle-list">
                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">Запоминать активное пространство</div>
                        <div className="set-row-desc">Открывать последнюю выбранную вкладку при нажатии Win</div>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-checked={startRememberSpace}
                        aria-label="Запоминать активное пространство"
                        disabled={busyKey === 'startRememberSpace'}
                        className={`glass-switch ${startRememberSpace ? 'on' : ''}`}
                        onClick={() =>
                          void runAction('startRememberSpace', () =>
                            onSetAppearance({ startRememberSpace: !startRememberSpace }),
                          )
                        }
                      >
                        <span className="glass-switch-thumb" />
                      </button>
                    </div>

                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">Индекс программ и игр</div>
                        <div className="set-row-desc">Пересканировать установленные приложения и библиотеки</div>
                      </div>
                      <button
                        type="button"
                        className="pin-any-btn"
                        disabled={busyKey === 'refreshIndexes'}
                        onClick={() => void runAction('refreshIndexes', onRefreshIndexes)}
                      >
                        <RefreshIcon className="ic14" />
                        <span>{busyKey === 'refreshIndexes' ? 'Обновление…' : 'Обновить'}</span>
                      </button>
                    </div>
                  </div>
                </div>

                <div className="set-group">
                  <div className="set-group-title">Горячие клавиши</div>
                  <div className="hotkey-table">
                    {HOTKEY_ROWS.map((row) => (
                      <div key={row.label} className="hotkey-row">
                        <span className="hotkey-desc">{row.label}</span>
                        <span className="hotkey-keys">
                          {row.keys.map((k) => (
                            <span key={k} className="kbd">{k}</span>
                          ))}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              </>
            )}

            {activeTab === 'system' && (
              <>
                <div className="set-group">
                  <div className="set-group-title">Запуск и конфигурация</div>
                  <div className="set-toggle-list">
                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">Автозапуск с Windows</div>
                        <div className="set-row-desc">Запускать TestBar автоматически при входе в систему</div>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-checked={autostart}
                        aria-label="Автозапуск с Windows"
                        disabled={busyKey === 'autostart'}
                        className={`glass-switch ${autostart ? 'on' : ''}`}
                        onClick={() => void runAction('autostart', () => onSetAutostart(!autostart))}
                      >
                        <span className="glass-switch-thumb" />
                      </button>
                    </div>

                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">Резервная копия профиля</div>
                        <div className="set-row-desc">Сохранить или восстановить все плитки и настройки</div>
                      </div>
                      <div className="set-io-row">
                        <button
                          type="button"
                          className="pin-any-btn"
                          disabled={busyKey === 'exportConfig'}
                          onClick={() => void runAction('exportConfig', onExportConfig)}
                        >
                          <DownloadIcon className="ic14" />
                          <span>Экспорт</span>
                        </button>
                        <button
                          type="button"
                          className="pin-any-btn"
                          disabled={busyKey === 'importConfig'}
                          onClick={() => void runAction('importConfig', onImportConfig)}
                        >
                          <UploadIcon className="ic14" />
                          <span>Импорт</span>
                        </button>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="set-group">
                  <div className="sec-row" style={{ marginBottom: 4 }}>
                    <span className="set-group-title" style={{ marginBottom: 0 }}>
                      Скрытые значки трея{hiddenTrayKeys.length > 0 ? ` (${hiddenTrayKeys.length})` : ''}
                    </span>
                    {hiddenTrayKeys.length > 0 && (
                      <button
                        type="button"
                        className="sec-act"
                        disabled={busyKey === 'restoreAllTray'}
                        onClick={() => void runAction('restoreAllTray', onRestoreAllTrayApps)}
                      >
                        Показать все
                      </button>
                    )}
                  </div>
                  {hiddenTrayKeys.length === 0 ? (
                    <div className="set-row-desc">Все фоновые значки отображаются в трее</div>
                  ) : (
                    <div className="tray-hidden-chips">
                      {hiddenTrayKeys.map((key) => (
                        <button
                          key={key}
                          type="button"
                          className="tray-hidden-chip"
                          title={`Вернуть ${key}`}
                          disabled={busyKey === `tray:${key}`}
                          onClick={() => void runAction(`tray:${key}`, () => onRestoreTrayApp(key))}
                        >
                          <span>{formatHiddenTrayLabel(key)}</span>
                          <XIcon10 />
                        </button>
                      ))}
                    </div>
                  )}
                </div>

                <div className="set-recovery-row">
                  <div className="set-recovery-meta">
                    <RestoreWindowsIcon className="ic14" />
                    <div>
                      <div className="set-row-title">Восстановление</div>
                      <div className="set-row-desc">Вернуть штатную панель Windows и закрыть TestBar</div>
                    </div>
                  </div>
                  <button
                    type="button"
                    className="set-recovery-btn"
                    onClick={onRestoreWindowsAndExit}
                  >
                    Вернуть Windows
                  </button>
                </div>
              </>
            )}
          </div>
        </section>
      </div>
    </div>
  );
};
