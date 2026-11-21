import React, { useState } from 'react';
import { formatIpcError } from '../../lib/flyoutLogic';
import type { SecondaryTaskbarMode, TestBarConfig } from '../../lib/ipc';
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
          <div className="settings-sub">Компоновка панели, поведение, второй монитор и система</div>
        </div>
        <button
          type="button"
          className="hdr-btn"
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
          {/* Card 1: Taskbar Layout */}
          <div className="set-card">
            <div className="set-card-head">
              <div className="set-card-title">Расположение панели</div>
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

          {/* Card 2: Clock & Behavior Switches */}
          <div className="set-card">
            <div className="set-card-head">
              <div className="set-card-title">Часы и поведение</div>
            </div>

            <div className="set-toggle-list">
              <div className="set-toggle-row">
                <div>
                  <div className="set-card-title">Секунды в часах</div>
                  <div className="set-card-desc">Точное время с секундами на панели</div>
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
                  <div className="set-card-desc">Короткий день рядом с датой</div>
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
                  <div className="set-card-desc">Открывать последнюю активную вкладку</div>
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

              <div className="set-toggle-row">
                <div>
                  <div className="set-card-title">Автозапуск с Windows</div>
                  <div className="set-card-desc">Запускать TestBar при входе в систему</div>
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
            </div>
          </div>
        </div>

        {/* RIGHT COLUMN */}
        <div className="set-stack">
          {/* Card 3: Secondary Monitor & Data */}
          <div className="set-card">
            <div className="set-card-head">
              <div className="set-card-title">Второй монитор и данные</div>
            </div>

            <div className="set-seg-3" role="radiogroup" aria-label="Режим второго монитора">
              {(
                [
                  ['testbar', 'Панель TestBar', 'Копия TestBar'],
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
                  onClick={() => void runAction('secondaryMode', () => onSetSecondaryMode(modeKey))}
                >
                  <span className="set-seg-title">{label}</span>
                  <span className="set-seg-hint">{hint}</span>
                </button>
              ))}
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

            {hiddenTrayKeys.length > 0 && (
              <div className="tray-hidden-wrap">
                <div className="sec-row" style={{ marginBottom: 4 }}>
                  <span className="sec-title">Скрытые в трее ({hiddenTrayKeys.length})</span>
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

        {/* Full-Width Compact Bottom Strip: Emergency Windows Taskbar Restore */}
        <div className="set-card set-danger-card">
          <div className="set-danger-info">
            <span className="set-danger-ic">
              <RestoreWindowsIcon className="ic16" />
            </span>
            <div className="set-card-desc">
              Мгновенно вернуть штатную панель задач Windows, рабочую область и завершить TestBar
            </div>
          </div>
          <button
            type="button"
            className="set-danger-btn"
            onClick={onRestoreWindowsAndExit}
          >
            Аварийный возврат штатной панели Windows
          </button>
        </div>
      </div>
    </div>
  );
};
