import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  computeWeatherTempBarRange,
  formatCityOptionSubtitle,
  formatIpcError,
  formatWeatherWindLabel,
} from '../../lib/flyoutLogic';
import {
  searchWeatherCities,
  type AudioState,
  type PeripheralBattery,
  type SpaceConfig,
  type SpaceLayoutKind,
  type SpaceWidgetId,
  type WeatherCityOption,
  type WeatherSnapshot,
} from '../../lib/ipc';
import {
  computeSlidingPillRect,
  filterVisibleBatteries,
  formatPeripheralSourceLabel,
  formatStartTemperature,
  getBatteryColor,
  isLowBatteryPercent,
  MAX_MODULAR_SPACES,
  MODULAR_SPACE_ACCENTS,
  moveSpaceIdInOrder,
  SPACE_TEMPLATES,
  SPACE_WIDGET_CATALOG,
  toggleSpaceWidget,
  validateSpaceName,
  type ArmedPowerKind,
  type PowerActionKind,
  type SlidingPillRect,
  type SpaceAccentOption,
  type SpaceTemplateId,
} from '../../lib/startLogic';
import { useAnimatedPresence } from '../../lib/useAnimatedPresence';
import { useReducedMotion } from '../../lib/useReducedMotion';
import {
  AppsGridIcon,
  ArrowLeftIcon,
  ArrowRightIcon,
  ChargingBoltIcon,
  CloseWindowIcon,
  DropletIcon,
  EditIcon,
  EditPencilIcon,
  EyeOffIcon,
  HeadphonesIcon,
  LockIcon,
  PeripheralDeviceIcon,
  PlusIcon,
  PowerIcon,
  RebootIcon,
  RefreshIcon,
  SearchIcon,
  SettingsGearIcon,
  SleepIcon,
  SlidersIcon,
  VolIcon,
  WeatherConditionIcon,
  WindIcon,
} from '../Icons';

export interface StartHeaderProps {
  space: string;
  spaces: SpaceConfig[];
  periphs: PeripheralBattery[];
  periphsRefreshing?: boolean;
  hiddenBatteryIds?: readonly string[] | null;
  audio: AudioState | null;
  periphOpen: boolean;
  weather: WeatherSnapshot | null;
  weatherRefreshing?: boolean;
  weatherUnits?: 'c' | 'f';
  weatherCityConfig?: { city: string; lat?: number | null; lon?: number | null };
  weatherOpen: boolean;
  editMode: boolean;
  drawerOpen: boolean;
  settingsOpen: boolean;
  searchOpen: boolean;
  powerOpen: boolean;
  armedPower: ArmedPowerKind;
  addingSpace: boolean;
  managingSpaceId: string | null;
  onSelectSpace: (spaceId: string) => void;
  onTogglePeriph: () => void;
  onClosePeriph: () => void;
  onRefreshPeriphs: () => Promise<void>;
  onHideBatteryDevice: (deviceId: string) => Promise<void>;
  onToggleWeather: () => void;
  onCloseWeather: () => void;
  onRefreshWeather: () => Promise<void>;
  onSelectWeatherCity: (opt: WeatherCityOption) => Promise<void>;
  onResetWeatherCity: () => Promise<void>;
  onChangeWeatherUnits: (units: 'c' | 'f') => Promise<void>;
  onSelectAudioDevice: (deviceId: string) => void;
  onOpenSearch: () => void;
  onToggleEditMode: () => void;
  onToggleDrawer: () => void;
  onToggleSettings: () => void;
  onTogglePower: () => void;
  onClosePower: () => void;
  onPowerAction: (kind: PowerActionKind) => void;
  onSetAddingSpace: (open: boolean) => void;
  onSetManagingSpaceId: (spaceId: string | null) => void;
  onCreateSpace: (params: {
    name: string;
    template: SpaceTemplateId;
    accentHex: string;
    accentRgb: string;
    widgets: SpaceWidgetId[];
  }) => Promise<void>;
  onUpdateSpace: (params: {
    id: string;
    name: string;
    accentHex: string;
    accentRgb: string;
    layoutKind: SpaceLayoutKind;
    showHeroRow: boolean;
    bottomSectionTitle?: string | null;
  }) => Promise<void>;
  onReorderSpaces: (orderedIds: string[]) => Promise<void>;
  onDeleteSpace: (id: string) => Promise<void>;
  onOpenWidgetBuilder: (spaceId: string) => void;
}

function findMatchingAccent(hex: string | undefined): SpaceAccentOption {
  const norm = (hex ?? '').trim().toUpperCase();
  return (
    MODULAR_SPACE_ACCENTS.find((a) => a.hex.toUpperCase() === norm) ?? MODULAR_SPACE_ACCENTS[4]
  );
}

export const StartHeader: React.FC<StartHeaderProps> = ({
  space,
  spaces,
  periphs,
  periphsRefreshing = false,
  hiddenBatteryIds,
  audio,
  periphOpen,
  weather,
  weatherRefreshing = false,
  weatherUnits = 'c',
  weatherCityConfig,
  weatherOpen,
  editMode,
  drawerOpen,
  settingsOpen,
  searchOpen,
  powerOpen,
  armedPower,
  addingSpace,
  managingSpaceId,
  onSelectSpace,
  onTogglePeriph,
  onClosePeriph,
  onRefreshPeriphs,
  onHideBatteryDevice,
  onToggleWeather,
  onCloseWeather,
  onRefreshWeather,
  onSelectWeatherCity,
  onResetWeatherCity,
  onChangeWeatherUnits,
  onSelectAudioDevice,
  onOpenSearch,
  onToggleEditMode,
  onToggleDrawer,
  onToggleSettings,
  onTogglePower,
  onClosePower,
  onPowerAction,
  onSetAddingSpace,
  onSetManagingSpaceId,
  onCreateSpace,
  onUpdateSpace,
  onReorderSpaces,
  onDeleteSpace,
  onOpenWidgetBuilder,
}) => {
  const reducedMotion = useReducedMotion();
  const navRef = useRef<HTMLElement>(null);
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const createInputRef = useRef<HTMLInputElement>(null);
  const manageInputRef = useRef<HTMLInputElement>(null);
  const spacePopRef = useRef<HTMLDivElement>(null);
  const periphPopRef = useRef<HTMLDivElement>(null);
  const weatherPopRef = useRef<HTMLDivElement>(null);
  const powerCapsuleRef = useRef<HTMLDivElement>(null);
  const deleteArmTimerRef = useRef<number | null>(null);
  const weatherSearchSeqRef = useRef(0);

  const [indicator, setIndicator] = useState<SlidingPillRect>({ left: 0, width: 0, visible: false });
  const [indicatorReady, setIndicatorReady] = useState(false);
  const [ghost, setGhost] = useState<SlidingPillRect>({ left: 0, width: 0, visible: false });

  // Create Space modal/popover state
  const [newSpaceTemplate, setNewSpaceTemplate] = useState<SpaceTemplateId>('blank');
  const [newSpaceName, setNewSpaceName] = useState('');
  const [selectedAccent, setSelectedAccent] = useState<SpaceAccentOption>(MODULAR_SPACE_ACCENTS[4]);
  const [newWidgets, setNewWidgets] = useState<SpaceWidgetId[]>([]);
  const [createError, setCreateError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  // Manage Space popover state
  const managingSpace = spaces.find((s) => s.id === managingSpaceId) ?? null;
  const lastManagedSpaceRef = useRef<SpaceConfig | null>(null);
  if (managingSpace) {
    lastManagedSpaceRef.current = managingSpace;
  }
  const activeManagedSpace = managingSpace ?? lastManagedSpaceRef.current;

  const [editSpaceName, setEditSpaceName] = useState('');
  const [editSpaceAccent, setEditSpaceAccent] = useState<SpaceAccentOption>(MODULAR_SPACE_ACCENTS[4]);
  const [editLayoutKind, setEditLayoutKind] = useState<SpaceLayoutKind>('grid');
  const [editShowHeroRow, setEditShowHeroRow] = useState<boolean>(false);
  const [editBottomTitle, setEditBottomTitle] = useState('');
  const [manageError, setManageError] = useState<string | null>(null);
  const [savingSpace, setSavingSpace] = useState(false);
  const [reorderingSpaces, setReorderingSpaces] = useState(false);
  const [deleteArmed, setDeleteArmed] = useState(false);
  const [deletingSpace, setDeletingSpace] = useState(false);

  // Header Weather popover search & busy state
  const [weatherCityQuery, setWeatherCityQuery] = useState('');
  const [weatherCityResults, setWeatherCityResults] = useState<WeatherCityOption[]>([]);
  const [weatherCitySearching, setWeatherCitySearching] = useState(false);
  const [weatherCityError, setWeatherCityError] = useState<string | null>(null);
  const [weatherBusy, setWeatherBusy] = useState(false);
  const [hidingDeviceId, setHidingDeviceId] = useState<string | null>(null);

  const periphPresence = useAnimatedPresence(periphOpen, 105);
  const weatherPresence = useAnimatedPresence(weatherOpen, 105);
  const createPresence = useAnimatedPresence(addingSpace, 105);
  const managePresence = useAnimatedPresence(Boolean(managingSpace), 105);

  const spacesKey = spaces.map((s) => `${s.id}:${s.name}:${s.accentHex}`).join('|');

  const measureActiveTab = () => {
    const btn = tabRefs.current[space];
    if (!btn) {
      setIndicator((prev) => (prev.visible ? { left: 0, width: 0, visible: false } : prev));
      return;
    }
    setIndicator(computeSlidingPillRect(btn.offsetLeft, btn.offsetWidth));
  };

  useLayoutEffect(() => {
    measureActiveTab();
    const btn = tabRefs.current[space];
    if (btn && typeof btn.scrollIntoView === 'function') {
      btn.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    }
    if (!indicatorReady) {
      const id =
        typeof requestAnimationFrame === 'function'
          ? requestAnimationFrame(() => setIndicatorReady(true))
          : window.setTimeout(() => setIndicatorReady(true), 0);
      return () => {
        if (typeof cancelAnimationFrame === 'function') {
          cancelAnimationFrame(id);
        } else {
          window.clearTimeout(id);
        }
      };
    }
  }, [space, spacesKey, editMode, indicatorReady]);

  useEffect(() => {
    const nav = navRef.current;
    if (!nav || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => measureActiveTab());
    ro.observe(nav);
    return () => ro.disconnect();
  }, [space, spacesKey, editMode]);

  useEffect(() => {
    return () => {
      if (deleteArmTimerRef.current !== null) {
        window.clearTimeout(deleteArmTimerRef.current);
      }
    };
  }, []);

  // Create Space popover lifecycle & outside click
  useEffect(() => {
    if (!addingSpace) {
      setNewSpaceTemplate('blank');
      setNewSpaceName('');
      setSelectedAccent(MODULAR_SPACE_ACCENTS[4]);
      setNewWidgets([]);
      setCreateError(null);
      setCreating(false);
      return;
    }
    requestAnimationFrame(() => createInputRef.current?.focus());

    const onOutsideMouseDown = (e: MouseEvent) => {
      const target = e.target as Node | null;
      if (
        spacePopRef.current &&
        target &&
        !spacePopRef.current.contains(target) &&
        !(target as HTMLElement).closest?.('.space-add-btn')
      ) {
        onSetAddingSpace(false);
      }
    };
    window.addEventListener('mousedown', onOutsideMouseDown);
    return () => window.removeEventListener('mousedown', onOutsideMouseDown);
  }, [addingSpace, onSetAddingSpace]);

  // Manage Space popover lifecycle & outside click
  useEffect(() => {
    if (!managingSpace) {
      setManageError(null);
      setSavingSpace(false);
      setReorderingSpaces(false);
      setDeleteArmed(false);
      setDeletingSpace(false);
      if (deleteArmTimerRef.current !== null) {
        window.clearTimeout(deleteArmTimerRef.current);
        deleteArmTimerRef.current = null;
      }
      return;
    }
    setEditSpaceName(managingSpace.name);
    setEditSpaceAccent(findMatchingAccent(managingSpace.accentHex));
    setEditLayoutKind(managingSpace.layoutKind === 'games' ? 'games' : 'grid');
    setEditShowHeroRow(Boolean(managingSpace.showHeroRow));
    setEditBottomTitle(managingSpace.bottomSectionTitle ?? '');
    setManageError(null);
    setDeleteArmed(false);
    setDeletingSpace(false);
    requestAnimationFrame(() => manageInputRef.current?.focus());

    const onOutsideMouseDown = (e: MouseEvent) => {
      const target = e.target as Node | null;
      if (
        spacePopRef.current &&
        target &&
        !spacePopRef.current.contains(target) &&
        !(target as HTMLElement).closest?.('.space-tab')
      ) {
        onSetManagingSpaceId(null);
      }
    };
    window.addEventListener('mousedown', onOutsideMouseDown);
    return () => window.removeEventListener('mousedown', onOutsideMouseDown);
  }, [managingSpace?.id, onSetManagingSpaceId]);

  // Close periph popover on outside click
  useEffect(() => {
    if (!periphOpen) return;
    const onOutsideMouseDown = (e: MouseEvent) => {
      const target = e.target as Node | null;
      if (
        periphPopRef.current &&
        target &&
        !periphPopRef.current.contains(target) &&
        !(target as HTMLElement).closest?.('.periph-pill')
      ) {
        onClosePeriph();
      }
    };
    window.addEventListener('mousedown', onOutsideMouseDown);
    return () => window.removeEventListener('mousedown', onOutsideMouseDown);
  }, [periphOpen, onClosePeriph]);

  // Weather popover outside click & debounced city search
  useEffect(() => {
    if (!weatherOpen) {
      weatherSearchSeqRef.current += 1;
      setWeatherCityQuery('');
      setWeatherCityResults([]);
      setWeatherCitySearching(false);
      setWeatherCityError(null);
      return;
    }
    const onOutsideMouseDown = (e: MouseEvent) => {
      const target = e.target as Node | null;
      if (
        weatherPopRef.current &&
        target &&
        !weatherPopRef.current.contains(target) &&
        !(target as HTMLElement).closest?.('.weather-hdr-pill')
      ) {
        onCloseWeather();
      }
    };
    window.addEventListener('mousedown', onOutsideMouseDown);
    return () => window.removeEventListener('mousedown', onOutsideMouseDown);
  }, [weatherOpen, onCloseWeather]);

  useEffect(() => {
    if (!weatherOpen) return;
    const trimmed = weatherCityQuery.trim();
    if (trimmed.length < 2) {
      weatherSearchSeqRef.current += 1;
      setWeatherCityResults([]);
      setWeatherCitySearching(false);
      setWeatherCityError(null);
      return;
    }
    const seq = ++weatherSearchSeqRef.current;
    setWeatherCitySearching(true);
    setWeatherCityError(null);
    const timer = window.setTimeout(() => {
      searchWeatherCities(trimmed)
        .then((res) => {
          if (weatherSearchSeqRef.current !== seq) return;
          setWeatherCityResults(Array.isArray(res) ? res : []);
        })
        .catch((err) => {
          if (weatherSearchSeqRef.current !== seq) return;
          setWeatherCityResults([]);
          setWeatherCityError(formatIpcError(err));
        })
        .finally(() => {
          if (weatherSearchSeqRef.current === seq) {
            setWeatherCitySearching(false);
          }
        });
    }, 260);
    return () => window.clearTimeout(timer);
  }, [weatherOpen, weatherCityQuery]);

  // Close power capsule on outside click
  useEffect(() => {
    if (!powerOpen) return;
    const onOutsideMouseDown = (e: MouseEvent) => {
      const target = e.target as Node | null;
      if (powerCapsuleRef.current && target && !powerCapsuleRef.current.contains(target)) {
        onClosePower();
      }
    };
    window.addEventListener('mousedown', onOutsideMouseDown);
    return () => window.removeEventListener('mousedown', onOutsideMouseDown);
  }, [powerOpen, onClosePower]);

  const handleSelectTemplate = (tplId: SpaceTemplateId) => {
    setNewSpaceTemplate(tplId);
    const spec = SPACE_TEMPLATES.find((t) => t.id === tplId);
    if (spec) {
      setSelectedAccent(findMatchingAccent(spec.accentHex));
      setNewWidgets([...spec.widgets]);
    }
  };

  const handleSubmitSpace = async () => {
    if (creating) return;
    if (spaces.length >= MAX_MODULAR_SPACES) {
      setCreateError(`Максимум ${MAX_MODULAR_SPACES} пространств`);
      return;
    }
    const check = validateSpaceName(
      newSpaceName,
      spaces.map((s) => s.name),
    );
    if (!check.ok) {
      setCreateError(check.error);
      createInputRef.current?.focus();
      return;
    }
    setCreating(true);
    setCreateError(null);
    try {
      await onCreateSpace({
        name: check.name,
        template: newSpaceTemplate,
        accentHex: selectedAccent.hex,
        accentRgb: selectedAccent.rgb,
        widgets: newWidgets,
      });
      onSetAddingSpace(false);
    } catch (err) {
      setCreateError(typeof err === 'string' && err.trim() ? err : 'Не удалось создать пространство');
      setCreating(false);
    }
  };

  const handleSaveManagedSpace = async () => {
    if (!managingSpace || savingSpace) return;
    const otherNames = spaces
      .filter((s) => s.id !== managingSpace.id)
      .map((s) => s.name);
    const check = validateSpaceName(editSpaceName, otherNames, managingSpace.name);
    if (!check.ok) {
      setManageError(check.error);
      manageInputRef.current?.focus();
      return;
    }
    setSavingSpace(true);
    setManageError(null);
    try {
      const cleanBottomTitle = editBottomTitle.trim();
      await onUpdateSpace({
        id: managingSpace.id,
        name: check.name,
        accentHex: editSpaceAccent.hex,
        accentRgb: editSpaceAccent.rgb,
        layoutKind: editLayoutKind,
        showHeroRow: editShowHeroRow,
        bottomSectionTitle: cleanBottomTitle ? cleanBottomTitle : null,
      });
      onSetManagingSpaceId(null);
    } catch (err) {
      setManageError(typeof err === 'string' && err.trim() ? err : 'Не удалось сохранить изменения');
      setSavingSpace(false);
    }
  };

  const handleMoveManagedSpace = async (direction: 'left' | 'right') => {
    if (!managingSpace || reorderingSpaces) return;
    const nextOrder = moveSpaceIdInOrder(
      spaces.map((s) => s.id),
      managingSpace.id,
      direction,
    );
    if (!nextOrder) return;
    setReorderingSpaces(true);
    setManageError(null);
    try {
      await onReorderSpaces(nextOrder);
    } catch (err) {
      setManageError(typeof err === 'string' && err.trim() ? err : 'Не удалось изменить порядок');
    } finally {
      setReorderingSpaces(false);
    }
  };

  const handleDeleteClick = async () => {
    if (!managingSpace || deletingSpace || spaces.length <= 1) return;
    if (!deleteArmed) {
      setDeleteArmed(true);
      if (deleteArmTimerRef.current !== null) {
        window.clearTimeout(deleteArmTimerRef.current);
      }
      deleteArmTimerRef.current = window.setTimeout(() => {
        deleteArmTimerRef.current = null;
        setDeleteArmed(false);
      }, 3000);
      return;
    }
    setDeletingSpace(true);
    setManageError(null);
    try {
      await onDeleteSpace(managingSpace.id);
      onSetManagingSpaceId(null);
    } catch (err) {
      setManageError(typeof err === 'string' && err.trim() ? err : 'Не удалось удалить пространство');
      setDeletingSpace(false);
      setDeleteArmed(false);
    }
  };

  const visiblePeriphs = useMemo(
    () => filterVisibleBatteries(periphs, hiddenBatteryIds),
    [periphs, hiddenBatteryIds],
  );
  const activeDev = audio?.devices.find((d) => d.isDefault);
  const inlinePeriphLimit = 2;
  const inlinePeriphs = visiblePeriphs.slice(0, inlinePeriphLimit);
  const extraPeriphCount = Math.max(0, visiblePeriphs.length - inlinePeriphLimit);
  const hasLowBat = visiblePeriphs.some((p) => isLowBatteryPercent(p.percent));
  const managedIdx = managingSpace ? spaces.findIndex((s) => s.id === managingSpace.id) : -1;
  const canDeleteSpace = spaces.length > 1;

  const configuredWeatherCity = weatherCityConfig?.city?.trim() ?? '';
  const isAutoWeatherLoc = !configuredWeatherCity && weatherCityConfig?.lat == null;
  const weatherHeaderCity = weather?.city?.trim() || configuredWeatherCity || 'Погода';
  const weatherPillTemp =
    weather && weather.available ? formatStartTemperature(weather.tempC, weatherUnits, false) : '—°';
  const weatherDailyRows = (weather?.daily ?? []).slice(0, 5);
  const weekMinC =
    weatherDailyRows.length > 0 ? Math.min(...weatherDailyRows.map((d) => d.tempMinC)) : 0;
  const weekMaxC =
    weatherDailyRows.length > 0 ? Math.max(...weatherDailyRows.map((d) => d.tempMaxC)) : 0;

  const handleHideDevice = async (deviceId: string) => {
    if (!deviceId || hidingDeviceId === deviceId) return;
    setHidingDeviceId(deviceId);
    try {
      await onHideBatteryDevice(deviceId);
    } finally {
      setHidingDeviceId(null);
    }
  };

  const handleSelectCity = async (opt: WeatherCityOption) => {
    if (weatherBusy) return;
    setWeatherBusy(true);
    setWeatherCityError(null);
    try {
      await onSelectWeatherCity(opt);
      setWeatherCityQuery('');
      setWeatherCityResults([]);
    } catch (err) {
      setWeatherCityError(formatIpcError(err));
    } finally {
      setWeatherBusy(false);
    }
  };

  const handleResetAutoCity = async () => {
    if (weatherBusy) return;
    setWeatherBusy(true);
    setWeatherCityError(null);
    try {
      await onResetWeatherCity();
      setWeatherCityQuery('');
      setWeatherCityResults([]);
    } catch (err) {
      setWeatherCityError(formatIpcError(err));
    } finally {
      setWeatherBusy(false);
    }
  };

  return (
    <>
      <header className="start-header">
        <div className="hdr-left">
          <button
            type="button"
            className={`periph-pill ${periphOpen ? 'open' : ''} ${hasLowBat ? 'has-low-bat' : ''}`}
            title="Беспроводные устройства и вывод звука"
            onClick={onTogglePeriph}
          >
            {inlinePeriphs.length === 0 ? (
              <>
                <HeadphonesIcon className="ic14" />
                <span>{activeDev?.shortName || 'Звук'}</span>
              </>
            ) : (
              <>
                {inlinePeriphs.map((p, idx) => {
                  const isLow = isLowBatteryPercent(p.percent);
                  return (
                    <React.Fragment key={p.id || `${p.name}-${idx}`}>
                      {idx > 0 && <span className="periph-sep" />}
                      <PeripheralDeviceIcon kind={p.kind} className="ic14" />
                      {p.charging && <ChargingBoltIcon className="ic12 periph-charge-ic" />}
                      <span className={isLow ? 'periph-low-txt' : undefined}>{p.percent}%</span>
                      <span className="bat-track">
                        <i
                          className="bat-fill"
                          style={
                            {
                              ['--pct' as string]: Math.max(0, Math.min(1, p.percent / 100)),
                              background: isLow ? '#FB7185' : getBatteryColor(p.percent),
                            } as React.CSSProperties
                          }
                        />
                      </span>
                    </React.Fragment>
                  );
                })}
                {extraPeriphCount > 0 && (
                  <span className="periph-more-badge">+{extraPeriphCount}</span>
                )}
              </>
            )}
          </button>

          <button
            type="button"
            className={`weather-hdr-pill ${weatherOpen ? 'open' : ''} ${weather?.available && weather.stale ? 'stale' : ''}`}
            data-condition={weather?.available ? weather.conditionIcon : undefined}
            title={
              weather?.available
                ? `${weatherHeaderCity}: ${weather.conditionLabel}`
                : 'Погода'
            }
            aria-label="Погода"
            aria-expanded={weatherOpen}
            onClick={onToggleWeather}
          >
            <WeatherConditionIcon
              condition={weather?.available ? weather.conditionIcon : 'cloud'}
              className="ic14 weather-hdr-ic"
            />
            <span>{weatherPillTemp}</span>
          </button>
        </div>

        <nav
          ref={navRef}
          className="space-nav"
          aria-label="Пространства"
          aria-hidden={powerOpen}
          style={
            powerOpen
              ? {
                  opacity: 0,
                  pointerEvents: 'none',
                  transform: reducedMotion ? undefined : 'translate3d(-6px, 0, 0) scale(0.985)',
                  transition: reducedMotion
                    ? 'opacity 110ms ease'
                    : 'opacity 140ms var(--ease-out), transform 140ms var(--ease-out)',
                }
              : {
                  transition: reducedMotion
                    ? 'opacity 110ms ease'
                    : 'opacity 160ms var(--ease-out), transform 160ms var(--ease-out)',
                }
          }
          onMouseLeave={() => setGhost((prev) => ({ ...prev, visible: false }))}
        >
          <span
            className="space-ghost"
            aria-hidden="true"
            style={{
              width: ghost.width,
              transform: `translate3d(${ghost.left}px, 0, 0)`,
              opacity: ghost.visible ? 1 : 0,
            }}
          />
          <span
            className={`space-indicator ${indicatorReady ? '' : 'no-anim'}`}
            aria-hidden="true"
            style={{
              width: indicator.width,
              transform: `translate3d(${indicator.left}px, 0, 0)`,
              opacity: indicator.visible ? 1 : 0,
            }}
          />
          {spaces.map((sp) => {
            const isActive = space === sp.id;
            return (
              <button
                key={sp.id}
                type="button"
                ref={(el) => {
                  tabRefs.current[sp.id] = el;
                }}
                className={`space-tab ${isActive ? 'active' : ''}`}
                title="Правый клик — настроить пространство"
                onMouseEnter={(e) => {
                  if (isActive) {
                    setGhost((prev) => ({ ...prev, visible: false }));
                  } else {
                    setGhost(
                      computeSlidingPillRect(e.currentTarget.offsetLeft, e.currentTarget.offsetWidth),
                    );
                  }
                }}
                onClick={() => {
                  if (editMode && isActive) {
                    onSetManagingSpaceId(managingSpaceId === sp.id ? null : sp.id);
                    return;
                  }
                  onSelectSpace(sp.id);
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  onSelectSpace(sp.id);
                  onSetManagingSpaceId(sp.id);
                }}
              >
                <span
                  className="s-dot"
                  style={
                    !isActive && sp.accentHex
                      ? { background: sp.accentHex, opacity: 0.5 }
                      : undefined
                  }
                />
                <span>{sp.name}</span>
                {editMode && <EditIcon className="ic12" />}
              </button>
            );
          })}
          <button
            type="button"
            className={`space-add-btn ${addingSpace ? 'open' : ''}`}
            disabled={spaces.length >= MAX_MODULAR_SPACES}
            title={
              spaces.length >= MAX_MODULAR_SPACES
                ? `Достигнут лимит (${MAX_MODULAR_SPACES})`
                : 'Создать пространство'
            }
            aria-label="Создать пространство"
            aria-expanded={addingSpace}
            onClick={() => onSetAddingSpace(!addingSpace)}
          >
            <PlusIcon className="ic14" />
          </button>
        </nav>

        <div className={`hdr-right ${powerOpen ? 'power-open' : ''}`}>
          <div className="hdr-tools-group">
            <button
              type="button"
              className={`hdr-btn ${searchOpen ? 'active' : ''}`}
              title="Поиск и команды"
              aria-label="Поиск и команды"
              onClick={onOpenSearch}
            >
              <SearchIcon className="ic14" />
            </button>
            <button
              type="button"
              className={`hdr-btn ${editMode ? 'active' : ''}`}
              title="Редактировать плитки"
              aria-label="Редактировать плитки"
              onClick={onToggleEditMode}
            >
              <EditPencilIcon className="ic14" />
            </button>
            <button
              type="button"
              className={`hdr-btn ${drawerOpen ? 'active' : ''}`}
              title="Все программы и закрепление"
              aria-label="Все программы и закрепление"
              onClick={onToggleDrawer}
            >
              <AppsGridIcon className="ic14" />
            </button>
            <button
              type="button"
              className={`hdr-btn gear ${settingsOpen ? 'active' : ''}`}
              title="Настройки"
              aria-label="Настройки"
              onClick={onToggleSettings}
            >
              <SettingsGearIcon className="ic14" />
            </button>
            <div className="pwr-spacer" />
          </div>

          <div
            ref={powerCapsuleRef}
            className={`power-capsule ${powerOpen ? 'open' : ''}`}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="power-strip">
              <button type="button" className="pwr-item" onClick={() => onPowerAction('lock')}>
                <LockIcon className="ic12" />
                <span>Блокировка</span>
              </button>
              <button type="button" className="pwr-item" onClick={() => onPowerAction('sleep')}>
                <SleepIcon className="ic12" />
                <span>Сон</span>
              </button>
              <button
                type="button"
                className={`pwr-item danger ${armedPower === 'reboot' ? 'armed' : ''}`}
                onClick={() => onPowerAction('reboot')}
              >
                <RebootIcon className="ic12" />
                <span>{armedPower === 'reboot' ? 'Подтвердить' : 'Перезагрузка'}</span>
              </button>
              <button
                type="button"
                className={`pwr-item danger ${armedPower === 'shutdown' ? 'armed' : ''}`}
                onClick={() => onPowerAction('shutdown')}
              >
                <PowerIcon className="ic12" />
                <span>{armedPower === 'shutdown' ? 'Подтвердить' : 'Выключение'}</span>
              </button>
            </div>
            <button
              type="button"
              className="pwr-trigger"
              title="Питание"
              aria-label="Питание"
              aria-expanded={powerOpen}
              onClick={onTogglePower}
            >
              <PowerIcon className="ic14" />
            </button>
          </div>
        </div>
      </header>

      {/* FLOATING CREATE / MANAGE MODULAR SPACE POPOVER */}
      {(createPresence.mounted || managePresence.mounted) && (
        <div className="space-pop-anchor" ref={spacePopRef}>
          {createPresence.mounted && (
            <div
              className="space-create-pop v3-space-pop"
              data-phase={createPresence.phase}
              role="dialog"
              aria-label="Новое пространство"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="space-create-head">Новое пространство</div>

              <div className="tile-edit-field">
                <span className="tile-edit-lbl">Шаблон</span>
                <div className="space-tpl-grid">
                  {SPACE_TEMPLATES.map((tpl) => (
                    <button
                      key={tpl.id}
                      type="button"
                      className={`space-tpl-btn ${newSpaceTemplate === tpl.id ? 'active' : ''}`}
                      aria-pressed={newSpaceTemplate === tpl.id}
                      onClick={() => handleSelectTemplate(tpl.id)}
                    >
                      <span className="space-tpl-dot" style={{ background: tpl.accentHex }} />
                      <span>{tpl.label}</span>
                    </button>
                  ))}
                </div>
              </div>

              <input
                ref={createInputRef}
                className="space-create-input"
                placeholder="Название пространства…"
                maxLength={18}
                value={newSpaceName}
                onChange={(e) => {
                  setNewSpaceName(e.target.value);
                  if (createError) setCreateError(null);
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    void handleSubmitSpace();
                  } else if (e.key === 'Escape') {
                    e.preventDefault();
                    e.stopPropagation();
                    onSetAddingSpace(false);
                  }
                }}
              />

              <div className="space-swatch-row">
                <span className="space-swatch-lbl">Акцент</span>
                <div className="space-swatches wrap">
                  {MODULAR_SPACE_ACCENTS.map((sw) => (
                    <button
                      key={sw.hex}
                      type="button"
                      className={`space-swatch ${selectedAccent.hex === sw.hex ? 'selected' : ''}`}
                      style={{ background: sw.hex }}
                      title={sw.label}
                      aria-label={sw.label}
                      aria-pressed={selectedAccent.hex === sw.hex}
                      onClick={() => setSelectedAccent(sw)}
                    />
                  ))}
                </div>
              </div>

              <div className="tile-edit-field">
                <span className="tile-edit-lbl">Виджеты</span>
                <div className="space-widget-chips">
                  {SPACE_WIDGET_CATALOG.map((w) => {
                    const active = newWidgets.includes(w.id);
                    return (
                      <button
                        key={w.id}
                        type="button"
                        className={`space-widget-chip ${active ? 'active' : ''}`}
                        aria-pressed={active}
                        onClick={() => setNewWidgets((prev) => toggleSpaceWidget(prev, w.id))}
                      >
                        {w.label}
                      </button>
                    );
                  })}
                </div>
              </div>

              {createError && <div className="space-create-err">{createError}</div>}

              <div className="space-create-actions">
                <button
                  type="button"
                  className="space-create-cancel"
                  onClick={() => onSetAddingSpace(false)}
                >
                  Отмена
                </button>
                <button
                  type="button"
                  className="space-create-submit"
                  disabled={creating}
                  onClick={() => void handleSubmitSpace()}
                >
                  Создать
                </button>
              </div>
            </div>
          )}

          {managePresence.mounted && activeManagedSpace && (
            <div
              className="space-manage-pop v3-space-pop"
              data-phase={managePresence.phase}
              role="dialog"
              aria-label="Настройка пространства"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="space-create-head">Настройка пространства</div>

              <input
                ref={manageInputRef}
                className="space-create-input"
                placeholder="Название пространства…"
                maxLength={18}
                value={editSpaceName}
                onChange={(e) => {
                  setEditSpaceName(e.target.value);
                  if (manageError) setManageError(null);
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    void handleSaveManagedSpace();
                  } else if (e.key === 'Escape') {
                    e.preventDefault();
                    e.stopPropagation();
                    onSetManagingSpaceId(null);
                  }
                }}
              />

              <div className="space-swatch-row">
                <span className="space-swatch-lbl">Акцент</span>
                <div className="space-swatches wrap">
                  {MODULAR_SPACE_ACCENTS.map((sw) => (
                    <button
                      key={sw.hex}
                      type="button"
                      className={`space-swatch ${editSpaceAccent.hex === sw.hex ? 'selected' : ''}`}
                      style={{ background: sw.hex }}
                      title={sw.label}
                      aria-label={sw.label}
                      aria-pressed={editSpaceAccent.hex === sw.hex}
                      onClick={() => setEditSpaceAccent(sw)}
                    />
                  ))}
                </div>
              </div>

              <div className="tile-edit-field">
                <span className="tile-edit-lbl">Макет пространства</span>
                <div className="space-seg-2" role="radiogroup" aria-label="Макет пространства">
                  <button
                    type="button"
                    role="radio"
                    aria-checked={editLayoutKind === 'grid'}
                    className={`space-preset-btn ${editLayoutKind === 'grid' ? 'active' : ''}`}
                    onClick={() => setEditLayoutKind('grid')}
                  >
                    Обычная сетка
                  </button>
                  <button
                    type="button"
                    role="radio"
                    aria-checked={editLayoutKind === 'games'}
                    className={`space-preset-btn ${editLayoutKind === 'games' ? 'active' : ''}`}
                    onClick={() => setEditLayoutKind('games')}
                  >
                    Библиотека игр
                  </button>
                </div>
              </div>

              <div className="tile-edit-field">
                <label className="tile-edit-lbl" htmlFor="tb-space-bottom-title">
                  Нижний блок
                </label>
                <input
                  id="tb-space-bottom-title"
                  className="space-create-input"
                  placeholder={editLayoutKind === 'games' ? 'Лаунчеры и закреплённое' : 'Быстрый доступ'}
                  maxLength={32}
                  value={editBottomTitle}
                  onChange={(e) => {
                    setEditBottomTitle(e.target.value);
                    if (manageError) setManageError(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      void handleSaveManagedSpace();
                    } else if (e.key === 'Escape') {
                      e.preventDefault();
                      e.stopPropagation();
                      onSetManagingSpaceId(null);
                    }
                  }}
                />
              </div>

              {editLayoutKind === 'grid' && (
                <div className="space-toggle-row">
                  <span className="space-toggle-lbl">Крупные плитки сверху</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={editShowHeroRow}
                    aria-label="Крупные плитки сверху"
                    className={`glass-switch ${editShowHeroRow ? 'on' : ''}`}
                    onClick={() => setEditShowHeroRow((v) => !v)}
                  >
                    <span className="glass-switch-thumb" />
                  </button>
                </div>
              )}

              <div className="space-order-row">
                <div className="space-order-group">
                  <button
                    type="button"
                    className="space-order-btn"
                    disabled={reorderingSpaces || managedIdx <= 0}
                    title="Сдвинуть влево"
                    aria-label="Сдвинуть пространство влево"
                    onClick={() => void handleMoveManagedSpace('left')}
                  >
                    <ArrowLeftIcon className="ic12" />
                    <span>Влево</span>
                  </button>
                  <button
                    type="button"
                    className="space-order-btn"
                    disabled={reorderingSpaces || managedIdx < 0 || managedIdx >= spaces.length - 1}
                    title="Сдвинуть вправо"
                    aria-label="Сдвинуть пространство вправо"
                    onClick={() => void handleMoveManagedSpace('right')}
                  >
                    <span>Вправо</span>
                    <ArrowRightIcon className="ic12" />
                  </button>
                </div>

                <button
                  type="button"
                  className="space-order-btn"
                  onClick={() => {
                    const targetId = activeManagedSpace.id;
                    onSetManagingSpaceId(null);
                    onOpenWidgetBuilder(targetId);
                  }}
                >
                  <SlidersIcon className="ic12" />
                  <span>Виджеты</span>
                </button>
              </div>

              {manageError && <div className="space-create-err">{manageError}</div>}

              <div className="space-create-actions split">
                <button
                  type="button"
                  className={`space-delete-btn ${deleteArmed ? 'armed' : ''}`}
                  disabled={!canDeleteSpace || deletingSpace}
                  title={!canDeleteSpace ? 'Нельзя удалить последнее пространство' : undefined}
                  onClick={() => void handleDeleteClick()}
                >
                  {deleteArmed ? 'Подтвердить' : 'Удалить'}
                </button>
                <div className="space-create-actions" style={{ marginTop: 0 }}>
                  <button
                    type="button"
                    className="space-create-cancel"
                    onClick={() => onSetManagingSpaceId(null)}
                  >
                    Отмена
                  </button>
                  <button
                    type="button"
                    className="space-create-submit"
                    disabled={savingSpace}
                    onClick={() => void handleSaveManagedSpace()}
                  >
                    Сохранить
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* PERIPHERALS & AUDIO OUTPUT POPOVER */}
      {periphPresence.mounted && (
        <div
          ref={periphPopRef}
          className="periph-pop"
          data-phase={periphPresence.phase}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="periph-pop-head">
            <div className="periph-pop-title">Беспроводные устройства</div>
            <button
              type="button"
              className={`periph-refresh-btn ${periphsRefreshing ? 'spinning' : ''}`}
              title="Обновить заряд устройств"
              aria-label="Обновить заряд устройств"
              disabled={periphsRefreshing}
              onClick={() => void onRefreshPeriphs()}
            >
              <RefreshIcon className="ic12" />
            </button>
          </div>

          {visiblePeriphs.length === 0 ? (
            <div className="pp-empty">
              {periphs.length > 0 ? 'Все устройства скрыты' : 'Беспроводные устройства не найдены'}
            </div>
          ) : (
            <div className="periph-pop-list">
              {visiblePeriphs.map((p, idx) => {
                const isLow = isLowBatteryPercent(p.percent);
                const subLevels = p.subLevels ?? [];
                return (
                  <div
                    key={p.id || `${p.name}-${idx}`}
                    className={`periph-pop-card ${isLow ? 'is-low' : ''}`}
                  >
                    <div className="periph-pop-row">
                      <span className="pp-ic">
                        <PeripheralDeviceIcon kind={p.kind} className="ic14" />
                      </span>
                      <div className="pp-meta">
                        <div className="pp-name">{p.name}</div>
                        <div className="periph-pop-badges">
                          <span className="periph-src-badge">
                            {formatPeripheralSourceLabel(p.source)}
                          </span>
                          {p.charging && (
                            <span className="periph-charge-badge">
                              <ChargingBoltIcon className="ic12" />
                              <span>Зарядка</span>
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="periph-pct-col">
                        {p.charging && <ChargingBoltIcon className="ic12 periph-charge-ic" />}
                        <strong
                          className="pp-pct"
                          style={{ color: isLow ? '#FB7185' : getBatteryColor(p.percent) }}
                        >
                          {p.percent}%
                        </strong>
                        <button
                          type="button"
                          className="periph-hide-btn"
                          title="Скрыть устройство"
                          aria-label={`Скрыть устройство ${p.name}`}
                          disabled={!p.id || hidingDeviceId === p.id}
                          onClick={() => void handleHideDevice(p.id)}
                        >
                          <EyeOffIcon className="ic12" />
                        </button>
                      </div>
                    </div>

                    <div className="bat-widget-bar">
                      <div
                        className="bat-widget-fill"
                        style={
                          {
                            ['--pct' as string]: Math.max(0, Math.min(1, p.percent / 100)),
                            background: isLow ? '#FB7185' : getBatteryColor(p.percent),
                          } as React.CSSProperties
                        }
                      />
                    </div>

                    {subLevels.length > 0 && (
                      <div className="periph-sub-levels">
                        {subLevels.map((sub, sIdx) => {
                          const subLow = isLowBatteryPercent(sub.percent);
                          return (
                            <span
                              key={`${sub.label}-${sIdx}`}
                              className={`periph-sub-chip ${subLow ? 'is-low' : ''}`}
                            >
                              {sub.charging && <ChargingBoltIcon className="ic12 periph-charge-ic" />}
                              <span>{sub.label}:</span>
                              <strong>{sub.percent}%</strong>
                            </span>
                          );
                        })}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}

          <div className="pp-sec">Вывод звука</div>
          {(audio?.devices ?? []).length === 0 ? (
            <div className="pp-empty">Устройства вывода не найдены</div>
          ) : (
            (audio?.devices ?? []).map((d) => (
              <button
                key={d.id}
                type="button"
                className={`pp-row ${d.isDefault ? 'active' : ''}`}
                onClick={() => {
                  onSelectAudioDevice(d.id);
                  onClosePeriph();
                }}
              >
                <span className="pp-ic"><VolIcon /></span>
                <div className="pp-meta">
                  <div className="pp-name">{d.shortName}</div>
                  <div className="pp-sub" style={{ color: d.isDefault ? 'var(--accent)' : 'var(--t3)' }}>
                    {d.isDefault ? 'Активный вывод' : 'Переключить'}
                  </div>
                </div>
              </button>
            ))
          )}
        </div>
      )}

      {/* HEADER WEATHER POPOVER */}
      {weatherPresence.mounted && (
        <div
          ref={weatherPopRef}
          className="weather-hdr-pop"
          data-phase={weatherPresence.phase}
          role="dialog"
          aria-label="Погода и прогноз"
          onClick={(e) => e.stopPropagation()}
        >
          <div className="whp-head">
            <div className="whp-head-meta">
              <span className="whp-city-title" title={weatherHeaderCity}>
                {weatherHeaderCity}
              </span>
              {weather?.available && weather.stale && (
                <span className="weather-stale-pill">Кэш</span>
              )}
            </div>
            <div className="whp-head-actions">
              <div className="whp-unit-seg" role="radiogroup" aria-label="Единицы температуры">
                <button
                  type="button"
                  role="radio"
                  aria-checked={weatherUnits === 'c'}
                  className={`whp-unit-btn ${weatherUnits === 'c' ? 'active' : ''}`}
                  disabled={weatherBusy}
                  onClick={() => void onChangeWeatherUnits('c')}
                >
                  °C
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={weatherUnits === 'f'}
                  className={`whp-unit-btn ${weatherUnits === 'f' ? 'active' : ''}`}
                  disabled={weatherBusy}
                  onClick={() => void onChangeWeatherUnits('f')}
                >
                  °F
                </button>
              </div>
              <button
                type="button"
                className={`weather-refresh-btn ${weatherRefreshing ? 'spinning' : ''}`}
                title="Обновить прогноз"
                aria-label="Обновить прогноз"
                disabled={weatherRefreshing}
                onClick={() => void onRefreshWeather()}
              >
                <RefreshIcon className="ic12" />
              </button>
            </div>
          </div>

          <div className="whp-city-box">
            <div className="whp-city-search-row">
              <div className="whp-city-input-wrap">
                <SearchIcon className="ic12" color="var(--t3)" />
                <input
                  type="text"
                  className="whp-city-input"
                  value={weatherCityQuery}
                  onChange={(e) => setWeatherCityQuery(e.target.value)}
                  placeholder="Выбрать город…"
                  aria-label="Поиск города"
                  onKeyDown={(e) => {
                    if (e.key === 'Escape') {
                      e.preventDefault();
                      e.stopPropagation();
                      if (weatherCityQuery) {
                        setWeatherCityQuery('');
                        setWeatherCityResults([]);
                      } else {
                        onCloseWeather();
                      }
                    }
                  }}
                />
                {weatherCityQuery && (
                  <button
                    type="button"
                    className="drawer-filter-clear"
                    aria-label="Очистить поиск города"
                    onClick={() => {
                      setWeatherCityQuery('');
                      setWeatherCityResults([]);
                    }}
                  >
                    <CloseWindowIcon className="ic12" />
                  </button>
                )}
              </div>
              <button
                type="button"
                className={`whp-auto-btn ${isAutoWeatherLoc ? 'active' : ''}`}
                disabled={weatherBusy}
                title="Автоопределение города"
                onClick={() => void handleResetAutoCity()}
              >
                Авто
              </button>
            </div>

            {weatherCitySearching && (
              <div className="whp-city-msg">Поиск городов…</div>
            )}
            {weatherCityError && (
              <div className="whp-city-msg err" role="alert">
                {weatherCityError}
              </div>
            )}
            {!weatherCitySearching &&
              !weatherCityError &&
              weatherCityQuery.trim().length >= 2 &&
              weatherCityResults.length === 0 && (
                <div className="whp-city-msg">Город не найден</div>
              )}
            {weatherCityResults.length > 0 && (
              <div className="whp-city-results">
                {weatherCityResults.map((opt, idx) => {
                  const sub = formatCityOptionSubtitle(opt);
                  return (
                    <button
                      key={`${opt.name}-${opt.lat}-${opt.lon}-${idx}`}
                      type="button"
                      className="whp-city-opt"
                      disabled={weatherBusy}
                      onClick={() => void handleSelectCity(opt)}
                    >
                      <span className="whp-city-opt-name">{opt.name}</span>
                      {sub && <span className="whp-city-opt-sub">{sub}</span>}
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          {weather?.available ? (
            <>
              <div className="whp-hero" data-condition={weather.conditionIcon}>
                <div className="whp-hero-left">
                  <WeatherConditionIcon
                    condition={weather.conditionIcon}
                    className="ic24 whp-hero-ic"
                  />
                  <div className="whp-temp-big">
                    {formatStartTemperature(weather.tempC, weatherUnits, false)}
                    <span className="whp-unit-tag">{weatherUnits === 'f' ? 'F' : 'C'}</span>
                  </div>
                </div>
                <div className="whp-hero-right">
                  <div className="whp-cond-label" title={weather.conditionLabel}>
                    {weather.conditionLabel}
                  </div>
                  <div className="whp-feels">
                    Ощущается как {formatStartTemperature(weather.feelsLikeC, weatherUnits, false)}
                  </div>
                  <div className="whp-metrics">
                    <span className="whp-metric" title="Влажность">
                      <DropletIcon className="ic12" />
                      <span>{weather.humidity}%</span>
                    </span>
                    <span className="whp-metric" title="Ветер">
                      <WindIcon className="ic12" />
                      <span>{formatWeatherWindLabel(weather.windKmh)}</span>
                    </span>
                  </div>
                </div>
              </div>

              {weather.hourly.length > 0 && (
                <div className="whp-hourly-strip" aria-label="Почасовой прогноз">
                  {weather.hourly.slice(0, 12).map((h, idx) => (
                    <div key={`${h.time}-${idx}`} className="whp-hour-cell">
                      <span className="whp-hour-time">{h.time}</span>
                      <WeatherConditionIcon condition={h.conditionIcon} className="ic14" />
                      <span className="whp-hour-temp">
                        {formatStartTemperature(h.tempC, weatherUnits, false)}
                      </span>
                      <span className={`whp-hour-precip ${h.precipProb >= 20 ? 'wet' : ''}`}>
                        {h.precipProb}%
                      </span>
                    </div>
                  ))}
                </div>
              )}

              {weatherDailyRows.length > 0 && (
                <div className="whp-daily-list" aria-label="Прогноз на 5 дней">
                  {weatherDailyRows.map((d, idx) => {
                    const bar = computeWeatherTempBarRange(
                      d.tempMinC,
                      d.tempMaxC,
                      weekMinC,
                      weekMaxC,
                    );
                    return (
                      <div key={d.date || `whp-day-${idx}`} className="whp-day-row">
                        <span className="whp-day-label">{d.label}</span>
                        <WeatherConditionIcon condition={d.conditionIcon} className="ic14" />
                        <span className={`whp-day-precip ${d.precipProb >= 20 ? 'wet' : ''}`}>
                          {d.precipProb > 0 ? `${d.precipProb}%` : '—'}
                        </span>
                        <span className="whp-day-min">
                          {formatStartTemperature(d.tempMinC, weatherUnits, false)}
                        </span>
                        <div className="whp-range-track" aria-hidden="true">
                          <div
                            className="whp-range-fill"
                            style={{ left: `${bar.leftPct}%`, width: `${bar.widthPct}%` }}
                          />
                        </div>
                        <span className="whp-day-max">
                          {formatStartTemperature(d.tempMaxC, weatherUnits, false)}
                        </span>
                      </div>
                    );
                  })}
                </div>
              )}
            </>
          ) : (
            <div className="weather-empty">
              {weatherRefreshing ? 'Загрузка прогноза…' : weather?.error || 'Погода недоступна'}
            </div>
          )}
        </div>
      )}
    </>
  );
};
