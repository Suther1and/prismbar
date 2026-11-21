import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  type AudioState,
  type CustomSpace,
  type PeripheralBattery,
  type WidgetPreset,
} from '../../lib/ipc';
import {
  BUILTIN_SPACES,
  computeSlidingPillRect,
  CUSTOM_SPACE_ACCENTS,
  getBatteryColor,
  validateCustomSpaceName,
  type ArmedPowerKind,
  type PowerActionKind,
  type SlidingPillRect,
  type SpaceAccentOption,
} from '../../lib/startLogic';
import { useAnimatedPresence } from '../../lib/useAnimatedPresence';
import { useReducedMotion } from '../../lib/useReducedMotion';
import {
  AppsGridIcon,
  EditIcon,
  EditPencilIcon,
  HeadphonesIcon,
  LockIcon,
  MouseIcon,
  PlusIcon,
  PowerIcon,
  RebootIcon,
  SearchIcon,
  SettingsGearIcon,
  SleepIcon,
  VolIcon,
} from '../Icons';

const WIDGET_PRESET_OPTIONS: ReadonlyArray<{ id: WidgetPreset; label: string }> = [
  { id: 'media_clipboard', label: 'Медиа и буфер' },
  { id: 'telemetry_processes', label: 'Телеметрия и процессы' },
  { id: 'audio_mic', label: 'Звук и микрофон' },
  { id: 'full_grid', label: 'Полная сетка' },
];

export interface StartHeaderProps {
  space: string;
  customSpaces: CustomSpace[];
  periphs: PeripheralBattery[];
  audio: AudioState | null;
  periphOpen: boolean;
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
  onCreateCustomSpace: (
    name: string,
    accent: SpaceAccentOption,
    widgetPreset: WidgetPreset,
  ) => Promise<void>;
  onUpdateCustomSpace: (params: {
    id: string;
    name: string;
    accent: SpaceAccentOption;
    widgetPreset: WidgetPreset;
  }) => Promise<void>;
  onDeleteCustomSpace: (id: string) => Promise<void>;
}

function findMatchingAccent(hex: string | undefined): SpaceAccentOption {
  const norm = (hex ?? '').trim().toUpperCase();
  return (
    CUSTOM_SPACE_ACCENTS.find((a) => a.hex.toUpperCase() === norm) ?? CUSTOM_SPACE_ACCENTS[4]
  );
}

export const StartHeader: React.FC<StartHeaderProps> = ({
  space,
  customSpaces,
  periphs,
  audio,
  periphOpen,
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
  onCreateCustomSpace,
  onUpdateCustomSpace,
  onDeleteCustomSpace,
}) => {
  const reducedMotion = useReducedMotion();
  const navRef = useRef<HTMLElement>(null);
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const createInputRef = useRef<HTMLInputElement>(null);
  const manageInputRef = useRef<HTMLInputElement>(null);
  const spacePopRef = useRef<HTMLDivElement>(null);
  const periphPopRef = useRef<HTMLDivElement>(null);
  const powerCapsuleRef = useRef<HTMLDivElement>(null);
  const deleteArmTimerRef = useRef<number | null>(null);

  const [indicator, setIndicator] = useState<SlidingPillRect>({ left: 0, width: 0, visible: false });
  const [indicatorReady, setIndicatorReady] = useState(false);
  const [ghost, setGhost] = useState<SlidingPillRect>({ left: 0, width: 0, visible: false });

  // Create Space popover state
  const [newSpaceName, setNewSpaceName] = useState('');
  const [selectedAccent, setSelectedAccent] = useState<SpaceAccentOption>(CUSTOM_SPACE_ACCENTS[4]);
  const [newWidgetPreset, setNewWidgetPreset] = useState<WidgetPreset>('media_clipboard');
  const [createError, setCreateError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  // Manage Custom Space popover state
  const managingSpace = customSpaces.find((s) => s.id === managingSpaceId) ?? null;
  const lastManagedSpaceRef = useRef<CustomSpace | null>(null);
  if (managingSpace) {
    lastManagedSpaceRef.current = managingSpace;
  }
  const activeManagedSpace = managingSpace ?? lastManagedSpaceRef.current;

  const [editSpaceName, setEditSpaceName] = useState('');
  const [editSpaceAccent, setEditSpaceAccent] = useState<SpaceAccentOption>(CUSTOM_SPACE_ACCENTS[4]);
  const [editWidgetPreset, setEditWidgetPreset] = useState<WidgetPreset>('media_clipboard');
  const [manageError, setManageError] = useState<string | null>(null);
  const [savingSpace, setSavingSpace] = useState(false);
  const [deleteArmed, setDeleteArmed] = useState(false);
  const [deletingSpace, setDeletingSpace] = useState(false);

  const periphPresence = useAnimatedPresence(periphOpen, 105);
  const createPresence = useAnimatedPresence(addingSpace, 105);
  const managePresence = useAnimatedPresence(Boolean(managingSpace), 105);

  const customSpacesKey = customSpaces.map((s) => `${s.id}:${s.name}`).join('|');

  const measureActiveTab = () => {
    const btn = tabRefs.current[space];
    if (!btn) return;
    setIndicator(computeSlidingPillRect(btn.offsetLeft, btn.offsetWidth));
  };

  useLayoutEffect(() => {
    measureActiveTab();
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
  }, [space, customSpacesKey, editMode, indicatorReady]);

  useEffect(() => {
    const nav = navRef.current;
    if (!nav || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => measureActiveTab());
    ro.observe(nav);
    return () => ro.disconnect();
  }, [space, customSpacesKey, editMode]);

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
      setNewSpaceName('');
      setNewWidgetPreset('media_clipboard');
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
    setEditWidgetPreset(managingSpace.widgetPreset ?? 'media_clipboard');
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

  const handleSubmitSpace = async () => {
    if (creating) return;
    const check = validateCustomSpaceName(
      newSpaceName,
      customSpaces.map((c) => c.name),
    );
    if (!check.ok) {
      setCreateError(check.error);
      createInputRef.current?.focus();
      return;
    }
    setCreating(true);
    setCreateError(null);
    try {
      await onCreateCustomSpace(check.name, selectedAccent, newWidgetPreset);
      onSetAddingSpace(false);
    } catch {
      setCreateError('Не удалось создать пространство');
      setCreating(false);
    }
  };

  const handleSaveManagedSpace = async () => {
    if (!managingSpace || savingSpace) return;
    const otherNames = customSpaces
      .filter((c) => c.id !== managingSpace.id)
      .map((c) => c.name);
    const check = validateCustomSpaceName(editSpaceName, otherNames);
    if (!check.ok) {
      setManageError(check.error);
      manageInputRef.current?.focus();
      return;
    }
    setSavingSpace(true);
    setManageError(null);
    try {
      await onUpdateCustomSpace({
        id: managingSpace.id,
        name: check.name,
        accent: editSpaceAccent,
        widgetPreset: editWidgetPreset,
      });
      onSetManagingSpaceId(null);
    } catch {
      setManageError('Не удалось сохранить изменения');
      setSavingSpace(false);
    }
  };

  const handleDeleteClick = async () => {
    if (!managingSpace || deletingSpace) return;
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
      await onDeleteCustomSpace(managingSpace.id);
      onSetManagingSpaceId(null);
    } catch {
      setManageError('Не удалось удалить пространство');
      setDeletingSpace(false);
      setDeleteArmed(false);
    }
  };

  const mouseBat = periphs.find((p) => p.kind === 'mouse');
  const audioBat = periphs.find((p) => p.kind === 'audio');
  const activeDev = audio?.devices.find((d) => d.isDefault);

  const allTabs: Array<{ id: string; label: string; isCustom: boolean }> = [
    ...BUILTIN_SPACES.map((b) => ({ id: b.id, label: b.label, isCustom: false })),
    ...customSpaces.map((cs) => ({ id: cs.id, label: cs.name, isCustom: true })),
  ];

  return (
    <>
      <header className="start-header">
        <div className="hdr-left">
          <button
            type="button"
            className={`periph-pill ${periphOpen ? 'open' : ''}`}
            title="Устройства и вывод звука"
            onClick={onTogglePeriph}
          >
            {mouseBat && (
              <>
                <MouseIcon className="ic14" />
                <span>{mouseBat.percent}%</span>
                <span className="bat-track">
                  <i
                    className="bat-fill"
                    style={
                      {
                        ['--pct' as string]: Math.max(0, Math.min(1, mouseBat.percent / 100)),
                        background: getBatteryColor(mouseBat.percent),
                      } as React.CSSProperties
                    }
                  />
                </span>
                <span className="periph-sep" />
              </>
            )}
            <HeadphonesIcon className="ic14" />
            <span>{audioBat ? `${audioBat.percent}%` : activeDev?.shortName || 'Звук'}</span>
            {audioBat && (
              <span className="bat-track">
                <i
                  className="bat-fill"
                  style={
                    {
                      ['--pct' as string]: Math.max(0, Math.min(1, audioBat.percent / 100)),
                      background: getBatteryColor(audioBat.percent),
                    } as React.CSSProperties
                  }
                />
              </span>
            )}
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
          {allTabs.map((t) => (
            <button
              key={t.id}
              type="button"
              ref={(el) => {
                tabRefs.current[t.id] = el;
              }}
              className={`space-tab ${space === t.id ? 'active' : ''}`}
              title={
                t.isCustom
                  ? 'Правый клик — настроить пространство'
                  : undefined
              }
              onMouseEnter={(e) => {
                if (space === t.id) {
                  setGhost((prev) => ({ ...prev, visible: false }));
                } else {
                  setGhost(computeSlidingPillRect(e.currentTarget.offsetLeft, e.currentTarget.offsetWidth));
                }
              }}
              onClick={() => {
                if (t.isCustom && editMode && space === t.id) {
                  onSetManagingSpaceId(managingSpaceId === t.id ? null : t.id);
                  return;
                }
                onSelectSpace(t.id);
              }}
              onContextMenu={(e) => {
                if (!t.isCustom) return;
                e.preventDefault();
                e.stopPropagation();
                onSelectSpace(t.id);
                onSetManagingSpaceId(t.id);
              }}
            >
              <span className="s-dot" />
              <span>{t.label}</span>
              {t.isCustom && editMode && <EditIcon className="ic12" />}
            </button>
          ))}
          <button
            type="button"
            className={`space-add-btn ${addingSpace ? 'open' : ''}`}
            title="Создать пространство"
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

      {/* FLOATING CREATE / MANAGE CUSTOM SPACE POPOVER (inside .space-pop-anchor) */}
      {(createPresence.mounted || managePresence.mounted) && (
        <div className="space-pop-anchor" ref={spacePopRef}>
          {createPresence.mounted && (
            <div
              className="space-create-pop"
              data-phase={createPresence.phase}
              role="dialog"
              aria-label="Новое пространство"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="space-create-head">Новое пространство</div>
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
                <div className="space-swatches">
                  {CUSTOM_SPACE_ACCENTS.map((sw) => (
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
                <span className="tile-edit-lbl">Правая колонка</span>
                <div className="space-preset-grid">
                  {WIDGET_PRESET_OPTIONS.map((opt) => (
                    <button
                      key={opt.id}
                      type="button"
                      className={`space-preset-btn ${newWidgetPreset === opt.id ? 'active' : ''}`}
                      aria-pressed={newWidgetPreset === opt.id}
                      onClick={() => setNewWidgetPreset(opt.id)}
                    >
                      {opt.label}
                    </button>
                  ))}
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
              className="space-manage-pop"
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
                <div className="space-swatches">
                  {CUSTOM_SPACE_ACCENTS.map((sw) => (
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
                <span className="tile-edit-lbl">Правая колонка</span>
                <div className="space-preset-grid">
                  {WIDGET_PRESET_OPTIONS.map((opt) => (
                    <button
                      key={opt.id}
                      type="button"
                      className={`space-preset-btn ${editWidgetPreset === opt.id ? 'active' : ''}`}
                      aria-pressed={editWidgetPreset === opt.id}
                      onClick={() => setEditWidgetPreset(opt.id)}
                    >
                      {opt.label}
                    </button>
                  ))}
                </div>
              </div>
              {manageError && <div className="space-create-err">{manageError}</div>}
              <div className="space-create-actions split">
                <button
                  type="button"
                  className={`space-delete-btn ${deleteArmed ? 'armed' : ''}`}
                  disabled={deletingSpace}
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
          <div className="periph-pop-title">Bluetooth-устройства</div>
          {periphs.length === 0 ? (
            <div className="pp-empty">Нет устройств с датчиком заряда</div>
          ) : (
            periphs.map((p) => (
              <div key={p.name} className="pp-row static">
                <span className="pp-ic">
                  {p.kind === 'audio' ? <HeadphonesIcon className="ic14" /> : <MouseIcon className="ic14" />}
                </span>
                <div className="pp-meta">
                  <div className="pp-name">{p.name}</div>
                  <div className="pp-sub">Bluetooth</div>
                </div>
                <strong className="pp-pct" style={{ color: getBatteryColor(p.percent) }}>
                  {p.percent}%
                </strong>
              </div>
            ))
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
    </>
  );
};
