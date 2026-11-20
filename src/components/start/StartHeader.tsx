import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import {
  type AudioState,
  type CustomSpace,
  type PeripheralBattery,
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
import {
  AppsGridIcon,
  EditPencilIcon,
  HeadphonesIcon,
  MouseIcon,
  PlusIcon,
  PowerIcon,
  SearchIcon,
  SettingsGearIcon,
  VolIcon,
} from '../Icons';

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
  onSelectSpace: (spaceId: string) => void;
  onTogglePeriph: () => void;
  onClosePeriph: () => void;
  onSelectAudioDevice: (deviceId: string) => void;
  onOpenSearch: () => void;
  onToggleEditMode: () => void;
  onToggleDrawer: () => void;
  onToggleSettings: () => void;
  onTogglePower: () => void;
  onPowerAction: (kind: PowerActionKind) => void;
  onSetAddingSpace: (open: boolean) => void;
  onCreateCustomSpace: (name: string, accent: SpaceAccentOption) => Promise<void>;
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
  onSelectSpace,
  onTogglePeriph,
  onClosePeriph,
  onSelectAudioDevice,
  onOpenSearch,
  onToggleEditMode,
  onToggleDrawer,
  onToggleSettings,
  onTogglePower,
  onPowerAction,
  onSetAddingSpace,
  onCreateCustomSpace,
}) => {
  const navRef = useRef<HTMLElement>(null);
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const createInputRef = useRef<HTMLInputElement>(null);
  const createPopRef = useRef<HTMLDivElement>(null);

  const [indicator, setIndicator] = useState<SlidingPillRect>({ left: 0, width: 0, visible: false });
  const [ghost, setGhost] = useState<SlidingPillRect>({ left: 0, width: 0, visible: false });

  const [newSpaceName, setNewSpaceName] = useState('');
  const [selectedAccent, setSelectedAccent] = useState<SpaceAccentOption>(CUSTOM_SPACE_ACCENTS[4]);
  const [createError, setCreateError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const measureActiveTab = () => {
    const btn = tabRefs.current[space];
    if (!btn) return;
    setIndicator(computeSlidingPillRect(btn.offsetLeft, btn.offsetWidth));
  };

  useLayoutEffect(() => {
    measureActiveTab();
  }, [space, customSpaces.length]);

  useEffect(() => {
    const nav = navRef.current;
    if (!nav || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => measureActiveTab());
    ro.observe(nav);
    return () => ro.disconnect();
  }, [space, customSpaces.length]);

  useEffect(() => {
    if (!addingSpace) {
      setNewSpaceName('');
      setCreateError(null);
      setCreating(false);
      return;
    }
    requestAnimationFrame(() => createInputRef.current?.focus());

    const onOutsideMouseDown = (e: MouseEvent) => {
      const target = e.target as Node | null;
      if (
        createPopRef.current &&
        target &&
        !createPopRef.current.contains(target) &&
        !(target as HTMLElement).closest?.('.space-add-btn')
      ) {
        onSetAddingSpace(false);
      }
    };
    window.addEventListener('mousedown', onOutsideMouseDown);
    return () => window.removeEventListener('mousedown', onOutsideMouseDown);
  }, [addingSpace, onSetAddingSpace]);

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
      await onCreateCustomSpace(check.name, selectedAccent);
      onSetAddingSpace(false);
    } catch {
      setCreateError('Не удалось создать пространство');
      setCreating(false);
    }
  };

  const mouseBat = periphs.find((p) => p.kind === 'mouse');
  const audioBat = periphs.find((p) => p.kind === 'audio');
  const activeDev = audio?.devices.find((d) => d.isDefault);

  const allTabs = [
    ...BUILTIN_SPACES.map((b) => ({ id: b.id, label: b.label })),
    ...customSpaces.map((cs) => ({ id: cs.id, label: cs.name })),
  ];

  return (
    <>
      <header className="start-header">
        <div className="hdr-left">
          <button
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
                    style={{ width: `${mouseBat.percent}%`, background: getBatteryColor(mouseBat.percent) }}
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
                  style={{ width: `${audioBat.percent}%`, background: getBatteryColor(audioBat.percent) }}
                />
              </span>
            )}
          </button>
        </div>

        <nav
          ref={navRef}
          className="space-nav"
          onMouseLeave={() => setGhost((prev) => ({ ...prev, visible: false }))}
        >
          <span
            className="space-ghost"
            aria-hidden="true"
            style={{
              width: ghost.width,
              transform: `translateX(${ghost.left}px)`,
              opacity: ghost.visible ? 1 : 0,
            }}
          />
          <span
            className="space-indicator"
            aria-hidden="true"
            style={{
              width: indicator.width,
              transform: `translateX(${indicator.left}px)`,
              opacity: indicator.visible ? 1 : 0,
            }}
          />
          {allTabs.map((t) => (
            <button
              key={t.id}
              ref={(el) => {
                tabRefs.current[t.id] = el;
              }}
              className={`space-tab ${space === t.id ? 'active' : ''}`}
              onMouseEnter={(e) => {
                if (space === t.id) {
                  setGhost((prev) => ({ ...prev, visible: false }));
                } else {
                  setGhost(computeSlidingPillRect(e.currentTarget.offsetLeft, e.currentTarget.offsetWidth));
                }
              }}
              onClick={() => onSelectSpace(t.id)}
            >
              <span className="s-dot" />
              {t.label}
            </button>
          ))}
          <button
            className={`space-add-btn ${addingSpace ? 'open' : ''}`}
            title="Создать пространство"
            aria-expanded={addingSpace}
            onClick={() => onSetAddingSpace(!addingSpace)}
          >
            <PlusIcon className="ic14" />
          </button>
        </nav>

        <div className={`hdr-right ${powerOpen ? 'power-open' : ''}`}>
          <div className="hdr-tools-group">
            <button
              className={`hdr-btn ${searchOpen ? 'active' : ''}`}
              title="Поиск"
              aria-label="Поиск"
              onClick={onOpenSearch}
            >
              <SearchIcon className="ic14" />
            </button>
            <button
              className={`hdr-btn ${editMode ? 'active' : ''}`}
              title="Редактировать"
              aria-label="Редактировать"
              onClick={onToggleEditMode}
            >
              <EditPencilIcon className="ic14" />
            </button>
            <button
              className={`hdr-btn ${drawerOpen ? 'active' : ''}`}
              title="Все программы и закрепление"
              aria-label="Все программы и закрепление"
              onClick={onToggleDrawer}
            >
              <AppsGridIcon className="ic14" />
            </button>
            <button
              className={`hdr-btn gear ${settingsOpen ? 'active' : ''}`}
              title="Настройки"
              aria-label="Настройки"
              onClick={onToggleSettings}
            >
              <SettingsGearIcon className="ic14" />
            </button>
            <div className="pwr-spacer" />
          </div>

          <div className={`power-capsule ${powerOpen ? 'open' : ''}`}>
            <div className="power-strip">
              <button className="pwr-item" onClick={() => onPowerAction('lock')}>Блок</button>
              <button className="pwr-item" onClick={() => onPowerAction('sleep')}>Сон</button>
              <button
                className={`pwr-item danger ${armedPower === 'reboot' ? 'armed' : ''}`}
                onClick={() => onPowerAction('reboot')}
              >
                {armedPower === 'reboot' ? 'Точно?' : 'Ребут'}
              </button>
              <button
                className={`pwr-item danger ${armedPower === 'shutdown' ? 'armed' : ''}`}
                onClick={() => onPowerAction('shutdown')}
              >
                {armedPower === 'shutdown' ? 'Выкл?' : 'Выкл'}
              </button>
            </div>
            <button
              className="pwr-trigger"
              title="Питание"
              aria-label="Питание"
              onClick={onTogglePower}
            >
              <PowerIcon className="ic14" />
            </button>
          </div>
        </div>
      </header>

      {/* FLOATING + CUSTOM SPACE POPOVER (anchored cleanly below .space-nav) */}
      {addingSpace && (
        <div
          ref={createPopRef}
          className="space-create-pop"
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

      {/* PERIPHERALS & AUDIO OUTPUT POPOVER */}
      {periphOpen && (
        <div className="periph-pop" onClick={(e) => e.stopPropagation()}>
          <div className="periph-pop-title">BLUETOOTH-УСТРОЙСТВА</div>
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

          <div className="pp-sec">ВЫВОД ЗВУКА</div>
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
