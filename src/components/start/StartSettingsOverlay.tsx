import React, { useState } from 'react';
import { formatIpcError } from '../../lib/flyoutLogic';
import type { GlassDepth, SecondaryTaskbarMode, TestBarConfig } from '../../lib/ipc';
import { useAnimatedPresence } from '../../lib/useAnimatedPresence';
import {
  CheckIcon,
  CloseWindowIcon,
  DownloadIcon,
  RefreshIcon,
  RestoreWindowsIcon,
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

const GLASS_PRESETS: ReadonlyArray<{ id: GlassDepth; label: string; desc: string }> = [
  { id: 'crystal', label: 'Кристалл', desc: 'Лёгкая глубина' },
  { id: 'obsidian', label: 'Обсидиан', desc: 'Баланс стекла' },
  { id: 'matte', label: 'Матовый', desc: 'Плотный фон' },
];

const HOTKEY_ROWS: ReadonlyArray<{ keys: string[]; label: string }> = [
  { keys: ['Win'], label: 'Открыть или закрыть «Пуск»' },
  { keys: ['Ctrl', '1–9'], label: 'Переход к пространству по номеру' },
  { keys: ['Tab'], label: 'Переключение пространств' },
  { keys: ['Shift'], label: 'Альтернативные действия на плитках' },
  { keys: ['Ctrl', 'Alt', 'Shift', 'Esc'], label: 'Аварийный возврат панели Windows' },
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

  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [settingsError, setSettingsError] = useState<string | null>(null);

  if (!mounted) return null;

  const barMode = config?.barMode === 'edge' ? 'edge' : 'dock';
  const secondaryMode: SecondaryTaskbarMode = config?.secondaryTaskbarMode ?? 'testbar';
  const autostart = config?.autostart ?? false;
  const glassDepth: GlassDepth = config?.glassDepth ?? 'obsidian';
  const clockShowSeconds = config?.clockShowSeconds ?? false;
  const clockShowWeekday = config?.clockShowWeekday ?? false;
  const startRememberSpace = config?.startRememberSpace ?? false;
  const hiddenTrayKeys = config?.hiddenTrayKeys ?? [];

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
      className="settings-drawer"
      data-phase={phase}
      role="dialog"
      aria-label="Настройки TestBar"
      onClick={(e) => e.stopPropagation()}
    >
      <div className="settings-head">
        <div>
          <div className="settings-title">Настройки TestBar</div>
          <div className="settings-sub">Компоновка панели, материал поверхности, мониторы и системная интеграция</div>
        </div>
        <button
          type="button"
          className="hdr-btn"
          title="Закрыть"
          aria-label="Закрыть настройки"
          onClick={onClose}
        >
          <CloseWindowIcon className="ic14" />
        </button>
      </div>

      {settingsError && (
        <div className="set-error-banner" role="alert">
          {settingsError}
        </div>
      )}

      <div className="settings-bento">
        {/* LEFT COLUMN */}
        <div className="set-stack">
          {/* Card 1: Taskbar Silhouette */}
          <div className="set-card">
            <div className="set-card-head">
              <div className="set-card-title">Расположение панели</div>
              <div className="set-card-desc">Парящая капсула по центру или классическая полоса на всю ширину</div>
            </div>
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
                    <CheckIcon size={11} />
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
                <div className="lp-label">Парящий док</div>
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
                    <CheckIcon size={11} />
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
                <div className="lp-label">Во всю ширину</div>
              </button>
            </div>
          </div>

          {/* Card 2: Strictly Dark Glass Depth & Clock / Start Behavior */}
          <div className="set-card">
            <div className="set-card-head">
              <div className="set-card-title">Внешний вид и часы</div>
              <div className="set-card-desc">Глубина тёмного стекла, формат времени и поведение «Пуска»</div>
            </div>

            <div className="glass-preset-grid" role="radiogroup" aria-label="Глубина стекла">
              {GLASS_PRESETS.map((preset) => {
                const active = glassDepth === preset.id;
                return (
                  <button
                    key={preset.id}
                    type="button"
                    role="radio"
                    aria-checked={active}
                    className={`glass-preset-card ${active ? 'active' : ''}`}
                    disabled={busyKey === 'glassDepth'}
                    onClick={() =>
                      void runAction('glassDepth', () => onSetAppearance({ glassDepth: preset.id }))
                    }
                  >
                    <span className={`gp-swatch ${preset.id}`} aria-hidden="true" />
                    <span className="set-seg-title">{preset.label}</span>
                    <span className="set-seg-hint">{preset.desc}</span>
                  </button>
                );
              })}
            </div>

            <div className="set-toggle-list">
              <div className="set-toggle-row">
                <div>
                  <div className="set-card-title">Секунды в часах</div>
                  <div className="set-card-desc">Показывать точное время с секундами на нижней панели</div>
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
                  <div className="set-card-title">День недели в часах</div>
                  <div className="set-card-desc">Короткий день недели рядом с датой на панели задач</div>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={clockShowWeekday}
                  aria-label="День недели в часах"
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

              <div className="set-toggle-row">
                <div>
                  <div className="set-card-title">Запоминать пространство в «Пуске»</div>
                  <div className="set-card-desc">Открывать последнюю вкладку вместо возврата в «Личное»</div>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={startRememberSpace}
                  aria-label="Запоминать пространство в Пуске"
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
            </div>
          </div>
        </div>

        {/* RIGHT COLUMN */}
        <div className="set-stack">
          {/* Card 3: Secondary Monitor, Autostart, Index Refresh, Hidden Tray, Backup */}
          <div className="set-card">
            <div className="set-card-head">
              <div className="set-card-title">Мониторы и система</div>
              <div className="set-card-desc">Дополнительный экран, автозапуск и управление конфигурацией</div>
            </div>

            <div className="set-seg-3" role="radiogroup" aria-label="Режим второго монитора">
              {(
                [
                  ['testbar', 'Панель TestBar', 'Отображать копию TestBar'],
                  ['native', 'Штатная Windows', 'Не скрывать штатную панель'],
                  ['hidden', 'Скрывать', 'Чистый второй экран'],
                ] as const
              ).map(([modeKey, label, hint]) => (
                <button
                  key={modeKey}
                  type="button"
                  role="radio"
                  aria-checked={secondaryMode === modeKey}
                  className={`set-seg-card ${secondaryMode === modeKey ? 'active' : ''}`}
                  disabled={busyKey === 'secondaryMode'}
                  onClick={() => void runAction('secondaryMode', () => onSetSecondaryMode(modeKey))}
                >
                  <span className="set-seg-title">{label}</span>
                  <span className="set-seg-hint">{hint}</span>
                </button>
              ))}
            </div>

            <div className="set-toggle-list">
              <div className="set-toggle-row">
                <div>
                  <div className="set-card-title">Автозапуск Windows</div>
                  <div className="set-card-desc">Запускать TestBar при входе в систему через реестр пользователя</div>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-checked={autostart}
                  aria-label="Автозапуск Windows"
                  disabled={busyKey === 'autostart'}
                  className={`glass-switch ${autostart ? 'on' : ''}`}
                  onClick={() => void runAction('autostart', () => onSetAutostart(!autostart))}
                >
                  <span className="glass-switch-thumb" />
                </button>
              </div>
            </div>

            <div className="set-io-row">
              <button
                type="button"
                className="pin-any-btn"
                disabled={busyKey === 'refreshIndexes'}
                onClick={() => void runAction('refreshIndexes', onRefreshIndexes)}
              >
                <RefreshIcon className="ic14" />
                <span>{busyKey === 'refreshIndexes' ? 'Обновление…' : 'Обновить индекс'}</span>
              </button>
              <button
                type="button"
                className="pin-any-btn"
                disabled={busyKey === 'exportConfig'}
                onClick={() => void runAction('exportConfig', onExportConfig)}
              >
                <DownloadIcon className="ic14" />
                <span>Экспорт JSON</span>
              </button>
              <button
                type="button"
                className="pin-any-btn"
                disabled={busyKey === 'importConfig'}
                onClick={() => void runAction('importConfig', onImportConfig)}
              >
                <UploadIcon className="ic14" />
                <span>Импорт JSON</span>
              </button>
            </div>

            {hiddenTrayKeys.length > 0 && (
              <div>
                <div className="sec-row" style={{ marginBottom: 6 }}>
                  <span className="sec-title">Скрытые иконки трея</span>
                  <button
                    type="button"
                    className="sec-act"
                    disabled={busyKey === 'restoreAllTray'}
                    onClick={() => void runAction('restoreAllTray', onRestoreAllTrayApps)}
                  >
                    Показать все
                  </button>
                </div>
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
              </div>
            )}
          </div>

          {/* Card 4: Keyboard Shortcuts */}
          <div className="set-card">
            <div className="set-card-head">
              <div className="set-card-title">Горячие клавиши</div>
            </div>
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
        </div>

        {/* Full-Width Emergency Restore Danger Card */}
        <div className="set-card set-danger-card">
          <div className="set-danger-info">
            <span className="set-danger-ic">
              <RestoreWindowsIcon className="ic18" />
            </span>
            <div>
              <div className="set-card-title">Восстановление</div>
              <div className="set-card-desc">Мгновенно вернуть стандартную панель задач Windows, рабочую область и завершить TestBar</div>
            </div>
          </div>
          <button
            type="button"
            className="set-danger-btn"
            onClick={onRestoreWindowsAndExit}
          >
            Вернуть стандартную панель Windows и выйти
          </button>
        </div>
      </div>
    </div>
  );
};
