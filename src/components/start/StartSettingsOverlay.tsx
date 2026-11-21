import React, { useEffect, useRef, useState } from 'react';
import '../../styles/widgets.css';
import { formatCityOptionSubtitle, formatIpcError } from '../../lib/flyoutLogic';
import {
  restoreAllBatteryDevices,
  restoreBatteryDevice,
  searchWeatherCities,
  setTaskbarWidgets,
  type GlassDepth,
  type SecondaryTaskbarMode,
  type TestBarConfig,
  type WeatherCityOption,
} from '../../lib/ipc';
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
  onSetTaskbarWidgets?: (params: Parameters<typeof setTaskbarWidgets>[0]) => Promise<void>;
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

function formatHiddenBatteryLabel(rawId: string): string {
  const trimmed = rawId.trim();
  if (!trimmed) return rawId;
  const colonIdx = trimmed.indexOf(':');
  if (colonIdx <= 0) return trimmed;

  const prefix = trimmed.slice(0, colonIdx).toLowerCase();
  const rest = trimmed.slice(colonIdx + 1).trim();
  if (!rest) return trimmed;

  if (prefix === 'xinput' && /^\d+$/.test(rest)) {
    return `Xbox #${Number(rest) + 1}`;
  }

  const brandMap: Record<string, string> = {
    razer: 'Razer',
    sony: 'PlayStation',
    logitech: 'Logitech',
    bt: 'Bluetooth',
    bluetooth: 'Bluetooth',
    xinput: 'Xbox',
  };
  const brand = brandMap[prefix] ?? (prefix.charAt(0).toUpperCase() + prefix.slice(1));
  const looksHexOrMac = /^[0-9a-f:.-]+$/i.test(rest) && /[0-9a-f]/i.test(rest);
  const formattedRest = looksHexOrMac
    ? rest.toUpperCase()
    : rest
        .replace(/[-_]+/g, ' ')
        .replace(/\b\w/g, (ch) => ch.toUpperCase());
  return `${brand} (${formattedRest})`;
}

export const StartSettingsOverlay: React.FC<StartSettingsOverlayProps> = ({
  open,
  config,
  onClose,
  onSetBarMode,
  onSetSecondaryMode,
  onSetAutostart,
  onSetAppearance,
  onSetTaskbarWidgets = (params) => setTaskbarWidgets(params).then(() => {}),
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
  const [cityQuery, setCityQuery] = useState('');
  const [cityResults, setCityResults] = useState<WeatherCityOption[]>([]);
  const [citySearching, setCitySearching] = useState(false);
  const [citySearchError, setCitySearchError] = useState<string | null>(null);
  const citySearchSeqRef = useRef(0);

  useEffect(() => {
    if (open) {
      setSettingsError(null);
    } else {
      setCityQuery('');
      setCityResults([]);
      setCitySearching(false);
      setCitySearchError(null);
    }
  }, [open]);

  useEffect(() => {
    if (!open || activeTab !== 'bar') return;
    const trimmed = cityQuery.trim();
    if (trimmed.length < 2) {
      citySearchSeqRef.current += 1;
      setCityResults([]);
      setCitySearching(false);
      setCitySearchError(null);
      return;
    }
    const seq = ++citySearchSeqRef.current;
    setCitySearching(true);
    setCitySearchError(null);
    const timer = window.setTimeout(() => {
      searchWeatherCities(trimmed)
        .then((res) => {
          if (citySearchSeqRef.current !== seq) return;
          setCityResults(Array.isArray(res) ? res : []);
        })
        .catch((err) => {
          if (citySearchSeqRef.current !== seq) return;
          setCityResults([]);
          setCitySearchError(formatIpcError(err));
        })
        .finally(() => {
          if (citySearchSeqRef.current === seq) {
            setCitySearching(false);
          }
        });
    }, 260);
    return () => window.clearTimeout(timer);
  }, [open, activeTab, cityQuery]);

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
  const hiddenBatteryIds = config?.hiddenBatteryIds ?? [];
  const tw = config?.taskbarWidgets;
  const showWeather = Boolean(tw?.showWeather);
  const showBatteries = tw?.showBatteries === true;
  const showTelemetry = tw?.showTelemetry === true;
  const weatherCity = tw?.weatherCity?.trim() ?? '';
  const weatherUnits: 'c' | 'f' = tw?.weatherUnits === 'f' ? 'f' : 'c';

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

                <div className="set-group">
                  <div className="set-group-title">Виджеты панели задач</div>
                  <div className="set-toggle-list">
                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">Погода на панели</div>
                        <div className="set-row-desc">
                          Дублировать погоду на нижней панели (в шапке «Пуска» показывается всегда)
                        </div>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-checked={showWeather}
                        aria-label="Погода на панели"
                        disabled={busyKey === 'tw:showWeather'}
                        className={`glass-switch ${showWeather ? 'on' : ''}`}
                        onClick={() =>
                          void runAction('tw:showWeather', () =>
                            onSetTaskbarWidgets({ showWeather: !showWeather }),
                          )
                        }
                      >
                        <span className="glass-switch-thumb" />
                      </button>
                    </div>

                    {showWeather && (
                      <div className="set-weather-config">
                        <div className="set-weather-row">
                          <span className="set-weather-city-badge">
                            Город: <strong>{weatherCity || 'Автоопределение'}</strong>
                          </span>
                          <div className="set-unit-seg" role="radiogroup" aria-label="Единицы температуры">
                            <button
                              type="button"
                              role="radio"
                              aria-checked={weatherUnits === 'c'}
                              className={`set-unit-btn ${weatherUnits === 'c' ? 'active' : ''}`}
                              disabled={busyKey === 'tw:units'}
                              onClick={() =>
                                void runAction('tw:units', () =>
                                  onSetTaskbarWidgets({ weatherUnits: 'c' }),
                                )
                              }
                            >
                              °C
                            </button>
                            <button
                              type="button"
                              role="radio"
                              aria-checked={weatherUnits === 'f'}
                              className={`set-unit-btn ${weatherUnits === 'f' ? 'active' : ''}`}
                              disabled={busyKey === 'tw:units'}
                              onClick={() =>
                                void runAction('tw:units', () =>
                                  onSetTaskbarWidgets({ weatherUnits: 'f' }),
                                )
                              }
                            >
                              °F
                            </button>
                          </div>
                        </div>

                        <div className="set-city-search-row">
                          <input
                            type="text"
                            className="weather-city-input"
                            value={cityQuery}
                            onChange={(e) => setCityQuery(e.target.value)}
                            placeholder="Выбрать город…"
                            aria-label="Поиск города для погоды"
                          />
                          <button
                            type="button"
                            className="pin-any-btn"
                            disabled={busyKey === 'tw:autoCity' || (!weatherCity && tw?.weatherLat == null)}
                            onClick={() => {
                              setCityQuery('');
                              setCityResults([]);
                              void runAction('tw:autoCity', () =>
                                onSetTaskbarWidgets({
                                  weatherCity: '',
                                  weatherLat: null,
                                  weatherLon: null,
                                }),
                              );
                            }}
                          >
                            Автоопределение
                          </button>
                        </div>

                        {citySearching && (
                          <div className="weather-city-msg">Поиск городов…</div>
                        )}
                        {citySearchError && (
                          <div className="weather-city-msg err" role="alert">
                            {citySearchError}
                          </div>
                        )}
                        {!citySearching &&
                          !citySearchError &&
                          cityQuery.trim().length >= 2 &&
                          cityResults.length === 0 && (
                            <div className="weather-city-msg">Город не найден</div>
                          )}
                        {cityResults.length > 0 && (
                          <div className="set-city-results">
                            {cityResults.map((opt, idx) => {
                              const sub = formatCityOptionSubtitle(opt);
                              return (
                                <button
                                  key={`${opt.name}-${opt.lat}-${opt.lon}-${idx}`}
                                  type="button"
                                  className="weather-city-opt"
                                  disabled={busyKey === 'tw:city'}
                                  onClick={() => {
                                    setCityQuery('');
                                    setCityResults([]);
                                    void runAction('tw:city', () =>
                                      onSetTaskbarWidgets({
                                        weatherCity: opt.name,
                                        weatherLat: opt.lat,
                                        weatherLon: opt.lon,
                                      }),
                                    );
                                  }}
                                >
                                  <span className="weather-city-opt-name">{opt.name}</span>
                                  {sub && <span className="weather-city-opt-sub">{sub}</span>}
                                </button>
                              );
                            })}
                          </div>
                        )}
                      </div>
                    )}

                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">Заряд беспроводных устройств</div>
                        <div className="set-row-desc">Мышь, наушники и геймпады в области трея</div>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-checked={showBatteries}
                        aria-label="Заряд беспроводных устройств"
                        disabled={busyKey === 'tw:showBatteries'}
                        className={`glass-switch ${showBatteries ? 'on' : ''}`}
                        onClick={() =>
                          void runAction('tw:showBatteries', () =>
                            onSetTaskbarWidgets({ showBatteries: !showBatteries }),
                          )
                        }
                      >
                        <span className="glass-switch-thumb" />
                      </button>
                    </div>

                    <div className="set-toggle-row">
                      <div>
                        <div className="set-row-title">Мини-статус ПК</div>
                        <div className="set-row-desc">Загрузка CPU и температура GPU на панели</div>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-checked={showTelemetry}
                        aria-label="Мини-статус ПК"
                        disabled={busyKey === 'tw:showTelemetry'}
                        className={`glass-switch ${showTelemetry ? 'on' : ''}`}
                        onClick={() =>
                          void runAction('tw:showTelemetry', () =>
                            onSetTaskbarWidgets({ showTelemetry: !showTelemetry }),
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

                <div className="set-group">
                  <div className="sec-row" style={{ marginBottom: 4 }}>
                    <span className="set-group-title" style={{ marginBottom: 0 }}>
                      Скрытые устройства батарей
                      {hiddenBatteryIds.length > 0 ? ` (${hiddenBatteryIds.length})` : ''}
                    </span>
                    {hiddenBatteryIds.length > 0 && (
                      <button
                        type="button"
                        className="sec-act"
                        disabled={busyKey === 'restoreAllBatteries'}
                        onClick={() =>
                          void runAction('restoreAllBatteries', async () => {
                            await restoreAllBatteryDevices();
                          })
                        }
                      >
                        Показать все
                      </button>
                    )}
                  </div>
                  {hiddenBatteryIds.length === 0 ? (
                    <div className="set-row-desc">Нет скрытых устройств</div>
                  ) : (
                    <div className="tray-hidden-chips">
                      {hiddenBatteryIds.map((id) => {
                        const label = formatHiddenBatteryLabel(id);
                        return (
                          <button
                            key={id}
                            type="button"
                            className="tray-hidden-chip"
                            title={`Показать ${label}`}
                            aria-label={`Показать ${label}`}
                            disabled={busyKey === `battery:${id}`}
                            onClick={() =>
                              void runAction(`battery:${id}`, async () => {
                                await restoreBatteryDevice(id);
                              })
                            }
                          >
                            <span>{label}</span>
                            <XIcon10 />
                          </button>
                        );
                      })}
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
