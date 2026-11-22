import React, { useEffect, useMemo, useRef, useState } from 'react';
import '../../styles/widgets.css';
import '../../styles/settings-studio.css';
import {
  applyAppearanceToDocument,
  GLASS_DEPTH_OPTIONS,
  GLOBAL_ACCENT_PALETTE,
  resolveAppearanceState,
  THEME_PRESETS,
} from '../../lib/appearance';
import { formatCityOptionSubtitle, formatIpcError } from '../../lib/flyoutLogic';
import {
  openAccessibilitySettings,
  quitApp,
  restoreAllBatteryDevices,
  restoreBatteryDevice,
  searchWeatherCities,
  setAppearanceSettings,
  setMacSettings,
  setSpaceWidgets,
  setTaskbarWidgets,
  type ClockDateFormat,
  type ClockLayout,
  type MacCommandKeyMode,
  type MacTerminalApp,
  type MacWindowPosition,
  type SecondaryTaskbarMode,
  type SpaceWidgetId,
  type TaskbarWidgetId,
  type TaskbarWidgetsPosition,
  type TestBarConfig,
  type TileSize,
  type UpdateCheckResult,
  type UpdateProgressEvent,
  type WeatherCityOption,
} from '../../lib/ipc';
import {
  detectPlatform,
  getAvailableSpaceWidgetIds,
  type AppPlatform,
} from '../../lib/platform';
import { resolveConfiguredSpaces } from '../../lib/startLogic';
import { useAnimatedPresence } from '../../lib/useAnimatedPresence';
import {
  AppsGridIcon,
  BluetoothIcon,
  CheckIcon,
  CloseWindowIcon,
  DownloadIcon,
  LayoutBarIcon,
  MusicNoteIcon,
  NetworkSpeedIcon,
  PaletteIcon,
  PeripheralDeviceIcon,
  RefreshIcon,
  RestoreWindowsIcon,
  SearchIcon,
  SettingsGearIcon,
  SlidersIcon,
  TimerIcon,
  UploadIcon,
  WeatherConditionIcon,
  XIcon10,
} from '../Icons';

export interface StartSettingsOverlayProps {
  open: boolean;
  config: TestBarConfig | null;
  onClose: () => void;
  onSetBarMode: (mode: 'dock' | 'edge') => Promise<void>;
  onSetSecondaryMode: (mode: SecondaryTaskbarMode) => Promise<void>;
  onSetAutostart: (enable: boolean) => Promise<void>;
  onSetAppearance: (params: Parameters<typeof setAppearanceSettings>[0]) => Promise<void>;
  onSetTaskbarWidgets?: (params: Parameters<typeof setTaskbarWidgets>[0]) => Promise<void>;
  onOpenSpaceWidgets?: (spaceId: string) => void;
  onRefreshIndexes: () => Promise<void>;
  onRestoreTrayApp: (key: string) => Promise<void>;
  onRestoreAllTrayApps: () => Promise<void>;
  onExportConfig: () => Promise<void>;
  onImportConfig: () => Promise<void>;
  onRestoreWindowsAndExit: () => void;
  platform?: AppPlatform;
  accessibilityTrusted?: boolean | null;
  onOpenAccessibilitySettings?: () => Promise<void>;
  onSetMacSettings?: (params: Parameters<typeof setMacSettings>[0]) => Promise<void>;
  onQuitApp?: () => void;
  appVersion?: string;
  updateInfo?: UpdateCheckResult | null;
  updateStatus?:
    | 'idle'
    | 'checking'
    | 'available'
    | 'up-to-date'
    | 'downloading'
    | 'installing'
    | 'error';
  updateProgress?: UpdateProgressEvent | null;
  updateError?: string | null;
  onCheckForUpdates?: () => Promise<void>;
  onInstallUpdate?: () => Promise<void>;
}

type SettingsTabId = 'appearance' | 'bar' | 'start' | 'devices' | 'system';

const SETTINGS_TABS: ReadonlyArray<{
  id: SettingsTabId;
  label: string;
  title: string;
  subtitle: string;
  tooltip?: string;
  icon: React.ReactNode;
}> = [
  {
    id: 'appearance',
    label: 'Внешний вид',
    title: 'Внешний вид',
    subtitle: 'Темы, глубина фона, единый акцент и эффекты подсветки',
    icon: <PaletteIcon className="ic14" />,
  },
  {
    id: 'bar',
    label: 'Нижняя панель',
    title: 'Нижняя панель',
    subtitle: 'Форма панели, виджеты, погода, часы и второй монитор',
    tooltip: 'Панель и мониторы',
    icon: <LayoutBarIcon className="ic14" />,
  },
  {
    id: 'start',
    label: 'Меню «Пуск»',
    title: 'Меню «Пуск»',
    subtitle: 'Плитки, пространства, индексация и горячие клавиши',
    icon: <AppsGridIcon className="ic14" />,
  },
  {
    id: 'devices',
    label: 'Устройства и трей',
    title: 'Устройства и трей',
    subtitle: 'Управление скрытыми значками трея и батареями периферии',
    icon: <BluetoothIcon className="ic14" />,
  },
  {
    id: 'system',
    label: 'Система',
    title: 'Система',
    subtitle: 'Автозапуск, профиль конфигурации и возврат панели Windows',
    icon: <SettingsGearIcon className="ic14" />,
  },
];

const TASKBAR_WIDGET_ITEMS: ReadonlyArray<{
  id: TaskbarWidgetId;
  label: string;
  desc: string;
  icon: React.ReactNode;
}> = [
  {
    id: 'weather',
    label: 'Погода',
    desc: 'Температура и прогноз',
    icon: <WeatherConditionIcon condition="partly-cloudy-day" className="ic14" />,
  },
  {
    id: 'telemetry',
    label: 'Монитор ПК',
    desc: 'CPU, RAM и температура GPU',
    icon: <SlidersIcon className="ic14" />,
  },
  {
    id: 'batteries',
    label: 'Батареи',
    desc: 'Заряд мыши и наушников',
    icon: <PeripheralDeviceIcon kind="mouse" className="ic14" />,
  },
  {
    id: 'network',
    label: 'Сеть',
    desc: 'Скорость RX/TX и пинг',
    icon: <NetworkSpeedIcon className="ic14" />,
  },
  {
    id: 'media',
    label: 'Медиа',
    desc: 'Текущий трек и пауза',
    icon: <MusicNoteIcon className="ic14" />,
  },
  {
    id: 'timer',
    label: 'Таймер',
    desc: 'Фокус-отсчёт на панели',
    icon: <TimerIcon className="ic14" />,
  },
];

const CLOCK_DATE_OPTIONS: ReadonlyArray<{
  id: ClockDateFormat;
  label: string;
  hint: string;
}> = [
  { id: 'short', label: 'Краткий · 21 ноя', hint: 'Число и месяц' },
  { id: 'numeric', label: 'Цифрами · 21.11.2026', hint: 'Полная дата' },
  { id: 'none', label: 'Только время', hint: 'Компактный режим' },
];

const CLOCK_LAYOUT_OPTIONS: ReadonlyArray<{
  id: ClockLayout;
  label: string;
  hint: string;
}> = [
  { id: 'center', label: 'По центру', hint: '2 ровные строки' },
  { id: 'right', label: 'По краю', hint: 'Правый край' },
  { id: 'single', label: 'В одну строку', hint: 'Дата и время рядом' },
];

const TILE_SIZE_OPTIONS: ReadonlyArray<{
  id: TileSize;
  label: string;
  hint: string;
}> = [
  { id: 'small', label: 'Маленький', hint: '6 в ряд' },
  { id: 'compact', label: 'Средний', hint: '3 в ряд' },
  { id: 'hero', label: 'Большой', hint: '2 в ряд' },
];

const INLINE_SPACE_WIDGET_LABELS: ReadonlyArray<{ id: SpaceWidgetId; label: string }> = [
  { id: 'media', label: 'Медиа' },
  { id: 'audio_mixer', label: 'Микшер' },
  { id: 'clipboard', label: 'Буфер' },
  { id: 'pc_health', label: 'Нагрузка ПК' },
  { id: 'top_processes', label: 'Процессы' },
  { id: 'weather', label: 'Погода' },
  { id: 'batteries', label: 'Батареи' },
  { id: 'notes', label: 'Заметки' },
  { id: 'timer', label: 'Таймер' },
  { id: 'network', label: 'Сеть' },
  { id: 'quick_toggles', label: 'Переключатели' },
];

const HOTKEY_ROWS: ReadonlyArray<{ keys: string[]; label: string }> = [
  { keys: ['Win'], label: 'Открыть или закрыть «Пуск»' },
  { keys: ['Ctrl', '1–9'], label: 'Переход к пространству по номеру' },
  { keys: ['Tab'], label: 'Переключение пространств' },
  { keys: ['Shift'], label: 'Альтернативные действия на плитках' },
  { keys: ['Ctrl', 'Alt', 'Shift', 'Esc'], label: 'Возврат штатной панели Windows' },
];

const MAC_HOTKEY_ROWS: ReadonlyArray<{ keys: string[]; label: string }> = [
  { keys: ['⌘'], label: 'Открыть или закрыть «Пуск»' },
  { keys: ['⌘ / Ctrl', '1–9'], label: 'Переход к пространству по номеру' },
  { keys: ['Tab'], label: 'Переключение пространств' },
  { keys: ['Shift'], label: 'Альтернативные действия на плитках' },
  { keys: ['Esc'], label: 'Закрыть окно или модалку' },
];

const MAC_COMMAND_KEY_OPTIONS: ReadonlyArray<{
  id: MacCommandKeyMode;
  label: string;
  hint: string;
}> = [
  { id: 'any', label: 'Любой ⌘', hint: 'Левый или правый' },
  { id: 'left', label: 'Левый ⌘', hint: 'Только левый' },
  { id: 'right', label: 'Правый ⌘', hint: 'Только правый' },
  { id: 'disabled', label: 'Выкл', hint: 'Не перехватывать' },
];

const MAC_WINDOW_POSITION_OPTIONS: ReadonlyArray<{
  id: MacWindowPosition;
  label: string;
  hint: string;
}> = [
  { id: 'center', label: 'По центру', hint: 'Как Spotlight' },
  { id: 'bottom', label: 'Снизу над Dock', hint: 'Классический вид' },
];

const MAC_TERMINAL_OPTIONS: ReadonlyArray<{
  id: MacTerminalApp;
  label: string;
}> = [
  { id: 'auto', label: 'Авто' },
  { id: 'terminal', label: 'Terminal' },
  { id: 'iterm', label: 'iTerm' },
  { id: 'warp', label: 'Warp' },
  { id: 'ghostty', label: 'Ghostty' },
];

const EN_TO_RU_MAP: Record<string, string> = {
  q: 'й', w: 'ц', e: 'у', r: 'к', t: 'е', y: 'н', u: 'г', i: 'ш', o: 'щ', p: 'з',
  '[': 'х', ']': 'ъ', a: 'ф', s: 'ы', d: 'в', f: 'а', g: 'п', h: 'р', j: 'о', k: 'л',
  l: 'д', ';': 'ж', "'": 'э', z: 'я', x: 'ч', c: 'с', v: 'м', b: 'и', n: 'т', m: 'ь',
  ',': 'б', '.': 'ю',
};

function swapEnLayoutToRu(text: string): string {
  let out = '';
  for (const ch of text) {
    out += EN_TO_RU_MAP[ch] ?? ch;
  }
  return out;
}

function matchesSearchKeywords(haystack: string, rawQuery: string): boolean {
  const q = rawQuery.trim().toLowerCase();
  if (!q) return true;
  const hay = haystack.toLowerCase();
  const swapped = swapEnLayoutToRu(q);
  const terms = q.split(/\s+/).filter(Boolean);
  if (terms.every((t) => hay.includes(t))) return true;
  const swappedTerms = swapped.split(/\s+/).filter(Boolean);
  return swappedTerms.length > 0 && swappedTerms.every((t) => hay.includes(t));
}

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

function formatClockSamplePreview(
  dateFormat: ClockDateFormat,
  layout: ClockLayout,
  showSeconds: boolean,
  showWeekday: boolean,
): string {
  const time = showSeconds ? '22:41:08' : '22:41';
  let date = '';
  if (dateFormat === 'short') {
    date = showWeekday ? 'Сб, 21 ноя' : '21 ноя';
  } else if (dateFormat === 'numeric') {
    date = showWeekday ? 'Сб, 21.11' : '21.11.2026';
  }
  if (!date) return time;
  if (layout === 'single') return `${date} · ${time}`;
  return `${time} / ${date}`;
}

function hexToRgbTriplet(hex: string): string {
  const clean = hex.replace('#', '');
  if (clean.length !== 6) return '16, 19, 28';
  const r = Number.parseInt(clean.slice(0, 2), 16);
  const g = Number.parseInt(clean.slice(2, 4), 16);
  const b = Number.parseInt(clean.slice(4, 6), 16);
  return `${r}, ${g}, ${b}`;
}

interface StudioBlock {
  id: string;
  section: SettingsTabId;
  keywords: string;
  render: () => React.ReactNode;
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
  onOpenSpaceWidgets,
  onRefreshIndexes,
  onRestoreTrayApp,
  onRestoreAllTrayApps,
  onExportConfig,
  onImportConfig,
  onRestoreWindowsAndExit,
  platform,
  accessibilityTrusted = null,
  onOpenAccessibilitySettings = openAccessibilitySettings,
  onSetMacSettings = (params) => setMacSettings(params).then(() => {}),
  onQuitApp,
  appVersion = '3.3.0',
  updateInfo = null,
  updateStatus = 'idle',
  updateProgress = null,
  updateError = null,
  onCheckForUpdates,
  onInstallUpdate,
}) => {
  const effectivePlatform = platform ?? detectPlatform();
  const isMac = effectivePlatform === 'macos';
  const tabsList = useMemo(
    () =>
      SETTINGS_TABS.filter((tab) => !(isMac && tab.id === 'bar')).map((tab) => {
        if (!isMac) return tab;
        if (tab.id === 'devices') {
          return {
            ...tab,
            label: 'Устройства',
            title: 'Устройства',
            subtitle: 'Управление скрытыми устройствами и батареями периферии',
          };
        }
        if (tab.id === 'system') {
          return {
            ...tab,
            subtitle: 'Обновления, автозапуск, профиль конфигурации и выход из PrismBar',
          };
        }
        return tab;
      }),
    [isMac],
  );
  const { mounted, phase } = useAnimatedPresence(open, 115);

  const [activeTab, setActiveTab] = useState<SettingsTabId>('appearance');
  const [searchQuery, setSearchQuery] = useState('');
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [cityQuery, setCityQuery] = useState('');
  const [cityResults, setCityResults] = useState<WeatherCityOption[]>([]);
  const [citySearching, setCitySearching] = useState(false);
  const [citySearchError, setCitySearchError] = useState<string | null>(null);
  const [expandedSpaceId, setExpandedSpaceId] = useState<string | null>(null);
  const citySearchSeqRef = useRef(0);

  useEffect(() => {
    if (config) {
      applyAppearanceToDocument(config);
    }
  }, [config]);

  useEffect(() => {
    if (open) {
      setSettingsError(null);
    } else {
      setSearchQuery('');
      setCityQuery('');
      setCityResults([]);
      setCitySearching(false);
      setCitySearchError(null);
      setExpandedSpaceId(null);
    }
  }, [open]);

  useEffect(() => {
    const weatherTab: SettingsTabId = isMac ? 'start' : 'bar';
    if (!open || (activeTab !== weatherTab && !searchQuery.trim())) return;
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
  }, [open, activeTab, searchQuery, cityQuery, isMac]);

  const appearance = resolveAppearanceState(config);
  const barMode = config?.barMode === 'edge' ? 'edge' : 'dock';
  const secondaryMode: SecondaryTaskbarMode = config?.secondaryTaskbarMode ?? 'testbar';
  const autostart = config?.autostart ?? false;
  const clockShowSeconds = config?.clockShowSeconds ?? false;
  const clockShowWeekday = config?.clockShowWeekday ?? false;
  const clockDateFormat: ClockDateFormat =
    config?.clockDateFormat === 'numeric' || config?.clockDateFormat === 'none'
      ? config.clockDateFormat
      : 'short';
  const clockLayout: ClockLayout =
    config?.clockLayout === 'right' || config?.clockLayout === 'single'
      ? config.clockLayout
      : 'center';
  const startRememberSpace = config?.startRememberSpace ?? false;
  const defaultTileSize: TileSize =
    config?.defaultTileSize === 'small' || config?.defaultTileSize === 'hero'
      ? config.defaultTileSize
      : 'compact';

  const hiddenTrayKeys = config?.hiddenTrayKeys ?? [];
  const hiddenBatteryIds = config?.hiddenBatteryIds ?? [];
  const tw = config?.taskbarWidgets;
  const legacyShowWeather = Boolean(tw?.showWeather);
  const legacyShowBatteries = tw?.showBatteries === true;
  const legacyShowTelemetry = tw?.showTelemetry === true;
  const activeTaskbarWidgets: TaskbarWidgetId[] =
    tw?.leftWidgets && tw.leftWidgets.length > 0
      ? tw.leftWidgets
      : [
          ...(legacyShowWeather ? (['weather'] as const) : []),
          ...(legacyShowTelemetry ? (['telemetry'] as const) : []),
          ...(legacyShowBatteries ? (['batteries'] as const) : []),
        ];
  const widgetsPosition: TaskbarWidgetsPosition =
    tw?.widgetsPosition === 'right' ? 'right' : 'left';
  const weatherCity = tw?.weatherCity?.trim() ?? '';
  const weatherUnits: 'c' | 'f' = tw?.weatherUnits === 'f' ? 'f' : 'c';
  const macCommandKey: MacCommandKeyMode = config?.macCommandKey ?? 'any';
  const macWindowPosition: MacWindowPosition = config?.macWindowPosition ?? 'center';
  const macTerminalApp: MacTerminalApp = config?.macTerminalApp ?? 'auto';
  const supportedSpaceWidgetLabels = useMemo(() => {
    const allowed = new Set(getAvailableSpaceWidgetIds(effectivePlatform));
    return INLINE_SPACE_WIDGET_LABELS.filter((w) => allowed.has(w.id));
  }, [effectivePlatform]);

  const spacesList = useMemo(
    () => resolveConfiguredSpaces(config, effectivePlatform),
    [config, effectivePlatform],
  );

  const activeThemeMeta =
    THEME_PRESETS.find((t) => t.id === appearance.themePreset) ?? THEME_PRESETS[0];
  const activeGlassMeta =
    GLASS_DEPTH_OPTIONS.find((g) => g.id === appearance.glassDepth) ?? GLASS_DEPTH_OPTIONS[1];
  const activeSwatchMeta = GLOBAL_ACCENT_PALETTE.find(
    (s) => s.hex.toUpperCase() === appearance.accentHex.toUpperCase(),
  );

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

  const toggleTaskbarWidget = (widgetId: TaskbarWidgetId) => {
    const active = activeTaskbarWidgets.includes(widgetId);
    const nextRaw = active
      ? activeTaskbarWidgets.filter((id) => id !== widgetId)
      : [...activeTaskbarWidgets, widgetId];
    const nextList = TASKBAR_WIDGET_ITEMS.map((item) => item.id).filter((id) =>
      nextRaw.includes(id),
    );
    void runAction(`tw:${widgetId}`, () =>
      onSetTaskbarWidgets({
        leftWidgets: nextList,
        showWeather: nextList.includes('weather'),
        showBatteries: nextList.includes('batteries'),
        showTelemetry: nextList.includes('telemetry'),
      }),
    );
  };

  // Preview inline CSS vars reflecting live theme/glass/accent/gradient/glow state
  const pvAlphaMap = {
    crystal: 0.58,
    obsidian: 0.76,
    matte: 0.90,
    solid: 1,
  } as const;
  const pvAlpha = pvAlphaMap[appearance.glassDepth];
  const pvBaseRgb = hexToRgbTriplet(activeThemeMeta.baseHex);
  const pvElevRgb = hexToRgbTriplet(activeThemeMeta.elevatedHex);
  const pvSurface =
    appearance.glassDepth === 'solid' && !appearance.surfaceGradients
      ? activeThemeMeta.baseHex
      : appearance.surfaceGradients
        ? `linear-gradient(180deg, rgba(${pvElevRgb}, ${pvAlpha}) 0%, rgba(${pvBaseRgb}, ${pvAlpha}) 100%)`
        : `rgba(${pvElevRgb}, ${pvAlpha})`;
  const pvAccentHex =
    appearance.accentMode === 'custom' ? appearance.accentHex : activeThemeMeta.accentHintHex;
  const pvAccentRgb =
    appearance.accentMode === 'custom'
      ? appearance.accentRgb
      : hexToRgbTriplet(activeThemeMeta.accentHintHex);
  const pvHeroBg = appearance.surfaceGradients
    ? `linear-gradient(135deg, rgba(${pvAccentRgb}, 0.24), rgba(255, 255, 255, 0.04))`
    : `rgba(${pvAccentRgb}, 0.16)`;

  const studioBlocks: StudioBlock[] = [
    {
      id: 'appearance-preview',
      section: 'appearance',
      keywords:
        'предпросмотр превью внешний вид тема стекло акцент градиент подсветка preview appearance',
      render: () => (
        <div className="set-group" key="appearance-preview">
          <div className="studio-group-head">
            <span className="set-group-title">Живой предпросмотр</span>
            <button
              type="button"
              className="studio-reset-btn"
              disabled={busyKey === 'resetVisuals'}
              onClick={() =>
                void runAction('resetVisuals', () => onSetAppearance({ resetVisuals: true }))
              }
            >
              <RefreshIcon className="ic12" />
              <span>Сбросить вид</span>
            </button>
          </div>
          <div className="studio-live-preview">
            <div
              className="studio-pv-stage"
              style={
                {
                  '--pv-surface': pvSurface,
                  '--pv-border': activeThemeMeta.borderHex,
                  '--pv-accent': pvAccentHex,
                  '--pv-accent-rgb': pvAccentRgb,
                  '--pv-hero-bg': pvHeroBg,
                } as React.CSSProperties
              }
            >
              <div className="studio-pv-backdrop-grid" aria-hidden="true" />
              <div className="studio-pv-start">
                {appearance.surfaceGradients && (
                  <div className="studio-pv-aura" aria-hidden="true" />
                )}
                <div className="studio-pv-header">
                  <div className="studio-pv-pills">
                    <span className="studio-pv-tab active" />
                    <span className="studio-pv-tab" />
                    <span className="studio-pv-tab" />
                  </div>
                  <span className="studio-pv-clock" />
                </div>
                <div className="studio-pv-tiles">
                  <div
                    className={`studio-pv-tile hero ${appearance.hoverGlow ? 'glow' : ''}`}
                  >
                    <span className="studio-pv-tile-ic" />
                    <div className="studio-pv-tile-lines">
                      <span className="studio-pv-line" />
                      <span className="studio-pv-line sub" />
                    </div>
                  </div>
                  <div className="studio-pv-tile">
                    <span className="studio-pv-tile-ic" />
                    <div className="studio-pv-tile-lines">
                      <span className="studio-pv-line" />
                      <span className="studio-pv-line sub" />
                    </div>
                  </div>
                  <div className="studio-pv-tile">
                    <span className="studio-pv-tile-ic" />
                    <div className="studio-pv-tile-lines">
                      <span className="studio-pv-line" />
                      <span className="studio-pv-line sub" />
                    </div>
                  </div>
                </div>
              </div>
              {!isMac && (
                <div className="studio-pv-dock" aria-hidden="true">
                  <span className="studio-pv-dot accent" />
                  <span className="studio-pv-dot" />
                  <span className="studio-pv-dot" />
                  <span className="studio-pv-sep" />
                  <span className="studio-pv-clock" />
                </div>
              )}
            </div>

            <div className="studio-pv-meta">
              <div className="studio-pv-summary-title">
                {activeThemeMeta.label} · {activeGlassMeta.label}
              </div>
              <div className="studio-pv-badges">
                <span className="studio-pv-badge">
                  <span
                    className="studio-pv-swatch-dot"
                    style={{ background: pvAccentHex }}
                  />
                  {appearance.accentMode === 'custom'
                    ? `Единый · ${activeSwatchMeta?.label ?? appearance.accentHex}`
                    : 'Акцент по вкладке'}
                </span>
                <span className="studio-pv-badge">
                  Градиенты: {appearance.surfaceGradients ? 'Вкл' : 'Выкл'}
                </span>
                <span className="studio-pv-badge">
                  Подсветка: {appearance.hoverGlow ? 'Вкл' : 'Выкл'}
                </span>
              </div>
            </div>
          </div>
        </div>
      ),
    },
    {
      id: 'appearance-theme',
      section: 'appearance',
      keywords:
        'тема оформление палитра тёмная обсидиан карбон oled полночь изумруд аметист графит theme obsidian carbon midnight emerald amethyst graphite',
      render: () => (
        <div className="set-group" key="appearance-theme">
          <div className="set-group-title">Тема оформления</div>
          <div className="studio-theme-grid" role="radiogroup" aria-label="Тема оформления">
            {THEME_PRESETS.map((opt) => {
              const active = appearance.themePreset === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  role="radio"
                  aria-checked={active}
                  disabled={busyKey === 'themePreset'}
                  className={`studio-theme-card ${active ? 'active' : ''}`}
                  onClick={() =>
                    void runAction('themePreset', () =>
                      onSetAppearance({ themePreset: opt.id }),
                    )
                  }
                >
                  {active && (
                    <span className="lp-check" aria-hidden="true">
                      <CheckIcon size={10} />
                    </span>
                  )}
                  <div
                    className="studio-theme-thumb"
                    style={{
                      background: opt.baseHex,
                      border: `1px solid ${opt.borderHex}`,
                    }}
                  >
                    <div
                      className="studio-theme-side"
                      style={{ background: opt.elevatedHex }}
                    />
                    <div
                      className="studio-theme-main"
                      style={{ background: opt.elevatedHex }}
                    >
                      <span
                        className="studio-theme-accent-bar"
                        style={{ background: opt.accentHintHex }}
                      />
                      <div className="studio-theme-lines">
                        <span />
                        <span />
                      </div>
                    </div>
                  </div>
                  <div className="studio-theme-name">{opt.label}</div>
                  <div className="studio-theme-hint">{opt.hint}</div>
                </button>
              );
            })}
          </div>
        </div>
      ),
    },
    {
      id: 'appearance-glass',
      section: 'appearance',
      keywords:
        'фон глубина стекла прозрачность акрил кристалл обсидиан матовый сплошной без прозрачности glass crystal obsidian matte solid',
      render: () => (
        <div className="set-group" key="appearance-glass">
          <div className="set-group-title">Фон и глубина стекла</div>
          <div
            className="studio-glass-grid"
            role="radiogroup"
            aria-label="Фон и глубина стекла"
          >
            {GLASS_DEPTH_OPTIONS.map((opt) => {
              const active = appearance.glassDepth === opt.id;
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
      ),
    },
    {
      id: 'appearance-accent',
      section: 'appearance',
      keywords:
        'акцент цвет палитра по вкладке единый акцент индиго кобальт бирюза изумруд янтарь оранж коралл роза аметист сталь accent color',
      render: () => (
        <div className="set-group" key="appearance-accent">
          <div className="set-group-title">Акцентный цвет</div>
          <div className="studio-seg-2" role="radiogroup" aria-label="Режим акцента">
            {(
              [
                ['per_space', 'По вкладке', 'У каждой вкладки свой цвет'],
                ['custom', 'Единый акцент', 'Один цвет для всей оболочки'],
              ] as const
            ).map(([modeKey, label, hint]) => (
              <button
                key={modeKey}
                type="button"
                role="radio"
                aria-checked={appearance.accentMode === modeKey}
                disabled={busyKey === 'accentMode'}
                className={`set-seg-card ${appearance.accentMode === modeKey ? 'active' : ''}`}
                onClick={() =>
                  void runAction('accentMode', () =>
                    onSetAppearance({ accentMode: modeKey }),
                  )
                }
              >
                <span className="set-seg-title">{label}</span>
                <span className="set-seg-hint">{hint}</span>
              </button>
            ))}
          </div>

          <div className="studio-swatch-row">
            <div className="studio-swatches" role="radiogroup" aria-label="Палитра акцента">
              {GLOBAL_ACCENT_PALETTE.map((swatch) => {
                const selected =
                  appearance.accentMode === 'custom' &&
                  appearance.accentHex.toUpperCase() === swatch.hex.toUpperCase();
                return (
                  <button
                    key={swatch.id}
                    type="button"
                    role="radio"
                    aria-checked={selected}
                    aria-label={swatch.label}
                    title={swatch.label}
                    disabled={busyKey === 'accentColor'}
                    className={`studio-accent-swatch ${selected ? 'active' : ''} ${
                      appearance.accentMode === 'per_space' ? 'muted' : ''
                    }`}
                    onClick={() =>
                      void runAction('accentColor', () =>
                        onSetAppearance({
                          accentMode: 'custom',
                          accentHex: swatch.hex,
                          accentRgb: swatch.rgb,
                        }),
                      )
                    }
                  >
                    <span
                      className="studio-accent-fill"
                      style={{ background: swatch.hex }}
                    >
                      {selected && <CheckIcon size={11} />}
                    </span>
                  </button>
                );
              })}
            </div>
            <span className="studio-accent-name">
              {appearance.accentMode === 'custom' ? (
                <>
                  Цвет: <strong>{activeSwatchMeta?.label ?? appearance.accentHex}</strong>
                </>
              ) : (
                'Выберите цвет для единого акцента'
              )}
            </span>
          </div>
        </div>
      ),
    },
    {
      id: 'appearance-effects',
      section: 'appearance',
      keywords:
        'эффекты градиенты фоновая аура подсветка наведение курсор блик свечение сбросить вид effects gradients aura hover glow reset',
      render: () => (
        <div className="set-group" key="appearance-effects">
          <div className="set-group-title">Эффекты</div>
          <div className="set-toggle-list">
            <div className="set-toggle-row">
              <div>
                <div className="set-row-title">Градиенты и фоновая аура</div>
                <div className="set-row-desc">
                  Мягкие градиентные заливки карточек, ползунков и свечение шапки «Пуска»
                </div>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={appearance.surfaceGradients}
                aria-label="Градиенты и фоновая аура"
                disabled={busyKey === 'surfaceGradients'}
                className={`glass-switch ${appearance.surfaceGradients ? 'on' : ''}`}
                onClick={() =>
                  void runAction('surfaceGradients', () =>
                    onSetAppearance({ surfaceGradients: !appearance.surfaceGradients }),
                  )
                }
              >
                <span className="glass-switch-thumb" />
              </button>
            </div>

            <div className="set-toggle-row">
              <div>
                <div className="set-row-title">Подсветка при наведении и от курсора</div>
                <div className="set-row-desc">
                  Световой блик за указателем мыши на плитках{isMac ? ' и карточках' : ', карточках и панели задач'}
                </div>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={appearance.hoverGlow}
                aria-label="Подсветка при наведении и от курсора"
                disabled={busyKey === 'hoverGlow'}
                className={`glass-switch ${appearance.hoverGlow ? 'on' : ''}`}
                onClick={() =>
                  void runAction('hoverGlow', () =>
                    onSetAppearance({ hoverGlow: !appearance.hoverGlow }),
                  )
                }
              >
                <span className="glass-switch-thumb" />
              </button>
            </div>
          </div>
        </div>
      ),
    },
    {
      id: 'bar-style',
      section: 'bar',
      keywords: 'стиль панели плавающий док на всю ширину dock edge taskbar',
      render: () => (
        <div className="set-group" key="bar-style">
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
      ),
    },
    {
      id: 'bar-widgets',
      section: 'bar',
      keywords:
        'виджеты нижней панели расположение слева справа погода монитор пк батареи сеть медиа таймер widgets position left right weather telemetry batteries network media timer',
      render: () => (
        <div className="set-group" key="bar-widgets">
          <div className="studio-group-head">
            <span className="set-group-title">Виджеты на нижней панели</span>
            <span className="studio-accent-name">
              Активно: <strong>{activeTaskbarWidgets.length}</strong> из 6
            </span>
          </div>

          <div className="studio-seg-2" role="radiogroup" aria-label="Расположение виджетов панели">
            {(
              [
                ['left', 'Слева на панели', 'Отдельная группа слева от приложений'],
                ['right', 'Справа у трея', 'Компактно рядом с часами и звуком'],
              ] as const
            ).map(([posKey, label, hint]) => (
              <button
                key={posKey}
                type="button"
                role="radio"
                aria-checked={widgetsPosition === posKey}
                disabled={busyKey === 'widgetsPosition'}
                className={`set-seg-card ${widgetsPosition === posKey ? 'active' : ''}`}
                onClick={() =>
                  void runAction('widgetsPosition', () =>
                    onSetTaskbarWidgets({ widgetsPosition: posKey }),
                  )
                }
              >
                <span className="set-seg-title">{label}</span>
                <span className="set-seg-hint">{hint}</span>
              </button>
            ))}
          </div>

          <div className="studio-widget-grid">
            {TASKBAR_WIDGET_ITEMS.map((item) => {
              const enabled = activeTaskbarWidgets.includes(item.id);
              return (
                <div
                  key={item.id}
                  className={`studio-widget-card ${enabled ? 'active' : ''}`}
                >
                  <div className="studio-widget-card-meta">
                    <span className="studio-widget-ic">{item.icon}</span>
                    <div className="studio-widget-text">
                      <div className="studio-widget-title">{item.label}</div>
                      <div className="studio-widget-desc">{item.desc}</div>
                    </div>
                  </div>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={enabled}
                    aria-label={item.label}
                    disabled={busyKey === `tw:${item.id}`}
                    className={`glass-switch ${enabled ? 'on' : ''}`}
                    onClick={() => toggleTaskbarWidget(item.id)}
                  >
                    <span className="glass-switch-thumb" />
                  </button>
                </div>
              );
            })}
          </div>
        </div>
      ),
    },
    {
      id: 'bar-weather',
      section: isMac ? 'start' : 'bar',
      keywords:
        'погода город поиск автоопределение цельсий фаренгейт температура градусы weather city units celsius fahrenheit',
      render: () => (
        <div className="set-group" key="bar-weather">
          <div className="studio-group-head">
            <span className="set-group-title">Погода и город</span>
            <span className="studio-accent-name">
              {isMac ? 'Для шапки и виджета «Погода»' : 'Для шапки «Пуска» и панели задач'}
            </span>
          </div>
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

            {citySearching && <div className="weather-city-msg">Поиск городов…</div>}
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
        </div>
      ),
    },
    {
      id: 'bar-clock',
      section: 'bar',
      keywords:
        'часы дата время формат краткий цифрами только время расположение по центру по краю в одну строку секунды день недели clock date time',
      render: () => (
        <div className="set-group" key="bar-clock">
          <div className="studio-group-head">
            <span className="set-group-title">Часы и дата</span>
            <span className="studio-clock-preview-pill">
              {formatClockSamplePreview(
                clockDateFormat,
                clockLayout,
                clockShowSeconds,
                clockShowWeekday,
              )}
            </span>
          </div>

          <div className="studio-sublabel">Формат даты</div>
          <div className="studio-seg-3" role="radiogroup" aria-label="Формат даты">
            {CLOCK_DATE_OPTIONS.map((opt) => (
              <button
                key={opt.id}
                type="button"
                role="radio"
                aria-checked={clockDateFormat === opt.id}
                disabled={busyKey === 'clockDateFormat'}
                className={`set-seg-card ${clockDateFormat === opt.id ? 'active' : ''}`}
                onClick={() =>
                  void runAction('clockDateFormat', () =>
                    onSetAppearance({ clockDateFormat: opt.id }),
                  )
                }
              >
                <span className="set-seg-title">{opt.label}</span>
                <span className="set-seg-hint">{opt.hint}</span>
              </button>
            ))}
          </div>

          <div className="studio-sublabel">Выравнивание и вид</div>
          <div className="studio-seg-3" role="radiogroup" aria-label="Расположение часов">
            {CLOCK_LAYOUT_OPTIONS.map((opt) => (
              <button
                key={opt.id}
                type="button"
                role="radio"
                aria-checked={clockLayout === opt.id}
                disabled={busyKey === 'clockLayout'}
                className={`set-seg-card ${clockLayout === opt.id ? 'active' : ''}`}
                onClick={() =>
                  void runAction('clockLayout', () =>
                    onSetAppearance({ clockLayout: opt.id }),
                  )
                }
              >
                <span className="set-seg-title">{opt.label}</span>
                <span className="set-seg-hint">{opt.hint}</span>
              </button>
            ))}
          </div>

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
                <div className="set-row-title">День недели</div>
                <div className="set-row-desc">Короткое обозначение дня недели рядом с датой</div>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={clockShowWeekday}
                aria-label="День недели"
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
      ),
    },
    {
      id: 'bar-monitors',
      section: 'bar',
      keywords: 'второй монитор экран дисплей панель windows скрывать secondary monitor display',
      render: () => (
        <div className="set-group" key="bar-monitors">
          <div className="set-group-title">Второй монитор</div>
          <div className="set-seg-3" role="radiogroup" aria-label="Режим второго монитора">
            {(
              [
                ['testbar', 'Панель PrismBar', 'Окна второго экрана'],
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
      ),
    },
    {
      id: 'mac-command-key',
      section: 'start',
      keywords:
        'command cmd клавиша открытия универсальный доступ accessibility позиция окна по центру снизу dock терминал terminal iterm warp ghostty macos',
      render: () => (
        <div className="set-group" key="mac-command-key">
          <div className="set-group-title">Управление на Mac</div>
          <div className="set-toggle-list">
            <div>
              <div className="set-row-title" style={{ marginBottom: 7 }}>
                Клавиша открытия (⌘ Command)
              </div>
              <div
                className="studio-seg-4"
                role="radiogroup"
                aria-label="Клавиша открытия Command"
              >
                {MAC_COMMAND_KEY_OPTIONS.map((opt) => (
                  <button
                    key={opt.id}
                    type="button"
                    role="radio"
                    aria-checked={macCommandKey === opt.id}
                    disabled={busyKey === 'mac:commandKey'}
                    className={`set-seg-card ${macCommandKey === opt.id ? 'active' : ''}`}
                    onClick={() =>
                      void runAction('mac:commandKey', () =>
                        onSetMacSettings({ macCommandKey: opt.id }),
                      )
                    }
                  >
                    <span className="set-seg-title">{opt.label}</span>
                    <span className="set-seg-hint">{opt.hint}</span>
                  </button>
                ))}
              </div>
            </div>

            <div className="set-toggle-row">
              <div>
                <div className="set-row-title">Разрешение ⌘ (Универсальный доступ)</div>
                <div className="set-row-desc">
                  Нужно для одиночного нажатия ⌘ Command без удержания сочетаний
                </div>
              </div>
              <div className="mac-ax-status-controls">
                <span
                  className={`mac-ax-status-pill ${accessibilityTrusted === true ? 'trusted' : 'pending'}`}
                >
                  {accessibilityTrusted === true ? 'Разрешено' : 'Нужен доступ'}
                </span>
                <button
                  type="button"
                  className="pin-any-btn"
                  disabled={busyKey === 'mac:axSettings'}
                  onClick={() =>
                    void runAction('mac:axSettings', onOpenAccessibilitySettings)
                  }
                >
                  Открыть настройки
                </button>
              </div>
            </div>

            <div>
              <div className="set-row-title" style={{ marginBottom: 7 }}>
                Позиция окна
              </div>
              <div
                className="studio-seg-2"
                role="radiogroup"
                aria-label="Позиция окна PrismBar"
              >
                {MAC_WINDOW_POSITION_OPTIONS.map((opt) => (
                  <button
                    key={opt.id}
                    type="button"
                    role="radio"
                    aria-checked={macWindowPosition === opt.id}
                    disabled={busyKey === 'mac:windowPosition'}
                    className={`set-seg-card ${macWindowPosition === opt.id ? 'active' : ''}`}
                    onClick={() =>
                      void runAction('mac:windowPosition', () =>
                        onSetMacSettings({ macWindowPosition: opt.id }),
                      )
                    }
                  >
                    <span className="set-seg-title">{opt.label}</span>
                    <span className="set-seg-hint">{opt.hint}</span>
                  </button>
                ))}
              </div>
            </div>

            <div>
              <div className="set-row-title" style={{ marginBottom: 7 }}>
                Терминал по умолчанию
              </div>
              <div
                className="studio-seg-5"
                role="radiogroup"
                aria-label="Терминал по умолчанию"
              >
                {MAC_TERMINAL_OPTIONS.map((opt) => (
                  <button
                    key={opt.id}
                    type="button"
                    role="radio"
                    aria-checked={macTerminalApp === opt.id}
                    disabled={busyKey === 'mac:terminalApp'}
                    className={`set-seg-card ${macTerminalApp === opt.id ? 'active' : ''}`}
                    onClick={() =>
                      void runAction('mac:terminalApp', () =>
                        onSetMacSettings({ macTerminalApp: opt.id }),
                      )
                    }
                  >
                    <span className="set-seg-title">{opt.label}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>
      ),
    },
    {
      id: 'start-behavior',
      section: 'start',
      keywords:
        'пуск запоминать активное пространство размер плиток по умолчанию маленький средний большой индекс программ игр обновить start remember space tile size index refresh',
      render: () => (
        <div className="set-group" key="start-behavior">
          <div className="set-group-title">Поведение и плитки</div>
          <div className="set-toggle-list">
            <div className="set-toggle-row">
              <div>
                <div className="set-row-title">Запоминать активное пространство</div>
                <div className="set-row-desc">
                  Открывать последнюю выбранную вкладку при нажатии {isMac ? '⌘' : 'Win'}
                </div>
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

            <div>
              <div className="set-row-title" style={{ marginBottom: 7 }}>
                Размер плиток по умолчанию
              </div>
              <div
                className="studio-seg-3"
                role="radiogroup"
                aria-label="Размер плиток по умолчанию"
              >
                {TILE_SIZE_OPTIONS.map((opt) => (
                  <button
                    key={opt.id}
                    type="button"
                    role="radio"
                    aria-checked={defaultTileSize === opt.id}
                    disabled={busyKey === 'defaultTileSize'}
                    className={`set-seg-card ${defaultTileSize === opt.id ? 'active' : ''}`}
                    onClick={() =>
                      void runAction('defaultTileSize', () =>
                        onSetAppearance({ defaultTileSize: opt.id }),
                      )
                    }
                  >
                    <span className="set-seg-title">{opt.label}</span>
                    <span className="set-seg-hint">{opt.hint}</span>
                  </button>
                ))}
              </div>
            </div>

            <div className="set-toggle-row">
              <div>
                <div className="set-row-title">
                  {isMac ? 'Индекс программ и файлов' : 'Индекс программ и игр'}
                </div>
                <div className="set-row-desc">
                  {isMac
                    ? 'Пересканировать установленные приложения и индексируемые папки'
                    : 'Пересканировать установленные приложения и библиотеки'}
                </div>
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
      ),
    },
    {
      id: 'start-spaces',
      section: 'start',
      keywords: `активные пространства вкладки настроить виджеты ${spacesList.map((s) => s.name).join(' ')}`,
      render: () => (
        <div className="set-group" key="start-spaces">
          <div className="set-group-title">Активные пространства</div>
          <div className="studio-spaces-list">
            {spacesList.map((sp) => {
              const headerCount = sp.headerWidgets?.length ?? 0;
              const sideCount = sp.widgets?.length ?? 0;
              const isExpanded = expandedSpaceId === sp.id;
              return (
                <div key={sp.id} className="studio-space-row">
                  <div className="studio-space-row-top">
                    <div className="studio-space-info">
                      <span
                        className="studio-space-dot"
                        style={{ background: sp.accentHex || '#818CF8' }}
                      />
                      <div style={{ minWidth: 0 }}>
                        <div className="studio-space-name">{sp.name}</div>
                        <div className="studio-space-counts">
                          Элементов: {sp.items.length} · Виджетов справа: {sideCount}
                          {headerCount > 0 ? ` · В шапке: ${headerCount}` : ''}
                        </div>
                      </div>
                    </div>
                    <button
                      type="button"
                      className="pin-any-btn"
                      onClick={() => {
                        if (onOpenSpaceWidgets) {
                          onOpenSpaceWidgets?.(sp.id);
                        } else {
                          setExpandedSpaceId((prev) => (prev === sp.id ? null : sp.id));
                        }
                      }}
                    >
                      <SlidersIcon className="ic14" />
                      <span>Настроить виджеты</span>
                    </button>
                  </div>

                  {!onOpenSpaceWidgets && isExpanded && (
                    <div className="studio-space-widget-chips">
                      {supportedSpaceWidgetLabels.map((w) => {
                        const active = sp.widgets.includes(w.id);
                        return (
                          <button
                            key={w.id}
                            type="button"
                            disabled={busyKey === `spw:${sp.id}:${w.id}`}
                            className={`studio-space-chip ${active ? 'active' : ''}`}
                            onClick={() => {
                              const nextWidgets = active
                                ? sp.widgets.filter((id) => id !== w.id)
                                : [...sp.widgets, w.id];
                              void runAction(`spw:${sp.id}:${w.id}`, () =>
                                setSpaceWidgets(sp.id, nextWidgets).then(() => {}),
                              );
                            }}
                          >
                            {w.label}
                          </button>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ),
    },
    {
      id: 'start-hotkeys',
      section: 'start',
      keywords: 'горячие клавиши сочетания win cmd ctrl tab shift esc hotkeys shortcuts',
      render: () => (
        <div className="set-group" key="start-hotkeys">
          <div className="set-group-title">Горячие клавиши</div>
          <div className="hotkey-table">
            {(isMac ? MAC_HOTKEY_ROWS : HOTKEY_ROWS).map((row) => (
              <div key={row.label} className="hotkey-row">
                <span className="hotkey-desc">{row.label}</span>
                <span className="hotkey-keys">
                  {row.keys.map((k) => (
                    <span key={k} className="kbd">
                      {k}
                    </span>
                  ))}
                </span>
              </div>
            ))}
          </div>
        </div>
      ),
    },
    {
      id: 'devices-tray',
      section: 'devices',
      keywords: `скрытые значки трея показать все восстановить tray hidden ${hiddenTrayKeys.join(' ')}`,
      render: () => (
        <div className="set-group" key="devices-tray">
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
      ),
    },
    {
      id: 'devices-batteries',
      section: 'devices',
      keywords: `скрытые устройства батарей беспроводные мышь наушники геймпад bluetooth razer logitech playstation xbox battery ${hiddenBatteryIds.join(' ')}`,
      render: () => (
        <div className="set-group" key="devices-batteries">
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
      ),
    },
    {
      id: 'system-updates',
      section: 'system',
      keywords:
        'обновление версия скачать проверить актуальная релиз update version check download',
      render: () => {
        const isChecking = updateStatus === 'checking';
        const isDownloading = updateStatus === 'downloading';
        const isInstalling = updateStatus === 'installing';
        const isUpdating = isChecking || isDownloading || isInstalling;
        const pct = updateProgress?.percent ?? null;
        const statusText = isInstalling
          ? 'Устанавливаю обновление и перезапускаю...'
          : isDownloading
            ? `Скачивание обновления v${updateInfo?.version ?? ''}${pct != null ? ` (${pct}%)` : '...'}`
            : isChecking
              ? 'Проверяю наличие обновлений...'
              : updateStatus === 'available' && updateInfo?.version
                ? `Доступна новая версия v${updateInfo.version}`
                : updateStatus === 'up-to-date'
                  ? 'Установлена актуальная версия'
                  : updateStatus === 'error'
                    ? updateError || 'Не удалось проверить обновления'
                    : 'Облачные обновления из GitHub Releases';
        const statusToneClass =
          updateStatus === 'available'
            ? 'available'
            : updateStatus === 'error'
              ? 'error'
              : '';

        return (
          <div className="set-group" key="system-updates">
            <div className="set-group-title">Обновления</div>
            <div className="studio-update-card">
              <div className="studio-update-head">
                <div>
                  <div className="studio-update-version">PrismBar v{appVersion || '3.3.0'}</div>
                  <div className={`studio-update-status ${statusToneClass}`.trim()}>
                    {statusText}
                  </div>
                </div>
                <div className="studio-update-actions">
                  <button
                    type="button"
                    className="pin-any-btn"
                    disabled={isUpdating || !onCheckForUpdates}
                    onClick={() => {
                      if (onCheckForUpdates) {
                        void runAction('updates:check', onCheckForUpdates);
                      }
                    }}
                  >
                    <RefreshIcon className="ic14" />
                    <span>{isChecking ? 'Проверка...' : 'Проверить'}</span>
                  </button>
                  {updateInfo?.available && updateInfo.version && onInstallUpdate && (
                    <button
                      type="button"
                      className="start-update-banner-btn"
                      disabled={isUpdating}
                      onClick={() => {
                        void runAction('updates:install', onInstallUpdate);
                      }}
                    >
                      {isInstalling
                        ? 'Установка...'
                        : isDownloading
                          ? 'Загрузка...'
                          : 'Обновить и перезапустить'}
                    </button>
                  )}
                </div>
              </div>
              {(isDownloading || isInstalling) && (
                <div className="studio-update-progress">
                  <div className="studio-update-progress-meta">
                    <span>
                      {isInstalling ? 'Установка новой версии...' : 'Загрузка обновления...'}
                    </span>
                    <span>{pct != null ? `${pct}%` : ''}</span>
                  </div>
                  <div className="studio-update-bar">
                    <div
                      className={`studio-update-bar-fill ${pct == null && !isInstalling ? 'indeterminate' : ''}`.trim()}
                      style={
                        pct != null || isInstalling
                          ? { width: `${isInstalling ? 100 : pct}%` }
                          : undefined
                      }
                    />
                  </div>
                </div>
              )}
            </div>
          </div>
        );
      },
    },
    {
      id: 'system-autostart',
      section: 'system',
      keywords:
        'автозапуск windows резервная копия профиля экспорт импорт конфигурация autostart export import backup config',
      render: () => (
        <div className="set-group" key="system-autostart">
          <div className="set-group-title">Запуск и конфигурация</div>
          <div className="set-toggle-list">
            <div className="set-toggle-row">
              <div>
                <div className="set-row-title">
                  {isMac ? 'Автозапуск PrismBar' : 'Автозапуск с Windows'}
                </div>
                <div className="set-row-desc">
                  Запускать PrismBar автоматически при входе в систему
                </div>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={autostart}
                aria-label={isMac ? 'Автозапуск PrismBar' : 'Автозапуск с Windows'}
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
                <div className="set-row-desc">
                  Сохранить или восстановить все плитки, пространства и настройки
                </div>
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
      ),
    },
    {
      id: 'system-recovery',
      section: 'system',
      keywords: isMac
        ? 'выход закрыть завершить приложение quit exit'
        : 'восстановление вернуть штатную панель windows закрыть выход recovery exit',
      render: () => (
        <div className="set-recovery-row" key="system-recovery">
          {isMac ? (
            <>
              <div className="set-recovery-meta">
                <CloseWindowIcon className="ic14" />
                <div>
                  <div className="set-row-title">Выход из приложения</div>
                  <div className="set-row-desc">Полностью завершить фоновую работу PrismBar</div>
                </div>
              </div>
              <button
                type="button"
                className="set-recovery-btn danger"
                onClick={() => (onQuitApp ? onQuitApp() : void quitApp())}
              >
                Выйти из PrismBar
              </button>
            </>
          ) : (
            <>
              <div className="set-recovery-meta">
                <RestoreWindowsIcon className="ic14" />
                <div>
                  <div className="set-row-title">Восстановление</div>
                  <div className="set-row-desc">Вернуть штатную панель Windows и закрыть PrismBar</div>
                </div>
              </div>
              <button
                type="button"
                className="set-recovery-btn"
                onClick={onRestoreWindowsAndExit}
              >
                Вернуть Windows
              </button>
            </>
          )}
        </div>
      ),
    },
  ];

  const availableBlocks = isMac
    ? studioBlocks.filter((block) => block.section !== 'bar' && block.id !== 'devices-tray')
    : studioBlocks.filter((block) => block.id !== 'mac-command-key');

  const trimmedSearch = searchQuery.trim();
  const isSearching = trimmedSearch.length > 0;
  const matchedBlocks = isSearching
    ? availableBlocks.filter((block) => matchesSearchKeywords(block.keywords, trimmedSearch))
    : availableBlocks.filter((block) => block.section === activeTab);

  const matchCountsBySection = new Map<SettingsTabId, number>();
  if (isSearching) {
    for (const block of matchedBlocks) {
      matchCountsBySection.set(
        block.section,
        (matchCountsBySection.get(block.section) ?? 0) + 1,
      );
    }
  }

  const activeMeta = tabsList.find((t) => t.id === activeTab) ?? tabsList[0];

  if (!mounted) return null;

  return (
    <div className="settings-backdrop" data-phase={phase} onClick={onClose}>
      <div
        className="settings-modal studio-modal"
        data-phase={phase}
        role="dialog"
        aria-label="Настройки PrismBar"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Sticky Left Navigation Sidebar */}
        <aside className="set-sidebar" aria-label="Разделы настроек">
          <div className="set-brand">
            <div className="set-brand-title">Настройки</div>
            <div className="set-brand-sub">PrismBar</div>
          </div>

          <div className="studio-search-wrap">
            <SearchIcon className="ic14" />
            <input
              type="text"
              className="studio-search-input"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Escape' && searchQuery) {
                  e.preventDefault();
                  e.stopPropagation();
                  setSearchQuery('');
                }
              }}
              placeholder="Поиск настроек…"
              aria-label="Поиск настроек"
            />
            {searchQuery && (
              <button
                type="button"
                className="studio-search-clear"
                aria-label="Очистить поиск"
                onClick={() => setSearchQuery('')}
              >
                <XIcon10 />
              </button>
            )}
          </div>

          <nav className="set-nav" role="tablist" aria-orientation="vertical">
            {tabsList.map((tab) => {
              const isActive = !isSearching && tab.id === activeTab;
              const hitCount = matchCountsBySection.get(tab.id) ?? 0;
              return (
                <button
                  key={tab.id}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  title={tab.tooltip ?? tab.title}
                  className={`set-nav-item ${isActive ? 'active' : ''}`}
                  onClick={() => {
                    setSearchQuery('');
                    setActiveTab(tab.id);
                  }}
                >
                  <span className="set-nav-ic">{tab.icon}</span>
                  <span className="set-nav-lbl">{tab.label}</span>
                  {isSearching && hitCount > 0 && (
                    <span className="studio-nav-count">{hitCount}</span>
                  )}
                </button>
              );
            })}
          </nav>

          <div className="studio-sidebar-foot">
            <span>{activeThemeMeta.label}</span>
            <span>{activeGlassMeta.label}</span>
          </div>
        </aside>

        {/* Right Studio Content Pane */}
        <section
          className="set-pane"
          role="tabpanel"
          aria-label={isSearching ? 'Результаты поиска' : activeMeta.title}
        >
          <header className="set-pane-head">
            <div>
              <h2 className="set-pane-title">
                {isSearching ? 'Результаты поиска' : activeMeta.title}
              </h2>
              <p className="set-pane-sub">
                {isSearching
                  ? `Найдено разделов: ${matchedBlocks.length}`
                  : activeMeta.subtitle}
              </p>
            </div>
            <div className="studio-head-actions">
              {isSearching && (
                <button
                  type="button"
                  className="pin-any-btn"
                  onClick={() => setSearchQuery('')}
                >
                  Сбросить поиск
                </button>
              )}
              <button
                type="button"
                className="hdr-btn"
                aria-label="Закрыть настройки"
                onClick={onClose}
              >
                <CloseWindowIcon className="ic14" />
              </button>
            </div>
          </header>

          {settingsError && (
            <div className="set-error-banner" role="alert">
              {settingsError}
            </div>
          )}

          <div className="set-pane-body" key={isSearching ? 'search' : activeTab}>
            {isSearching && matchedBlocks.length === 0 ? (
              <div className="studio-search-empty">
                <div>По запросу «{trimmedSearch}» настройки не найдены</div>
                <button
                  type="button"
                  className="pin-any-btn"
                  onClick={() => setSearchQuery('')}
                >
                  Показать все настройки
                </button>
              </div>
            ) : (
              matchedBlocks.map((block) => {
                if (!isSearching) {
                  return block.render();
                }
                const tabMeta = tabsList.find((t) => t.id === block.section);
                return (
                  <div key={block.id} style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
                    {tabMeta && (
                      <div>
                        <span className="studio-search-section-badge">{tabMeta.label}</span>
                      </div>
                    )}
                    {block.render()}
                  </div>
                );
              })
            )}
          </div>
        </section>
      </div>
    </div>
  );
};
