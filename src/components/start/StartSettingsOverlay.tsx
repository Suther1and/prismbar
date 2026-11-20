import React, { useState } from 'react';
import { formatIpcError } from '../../lib/flyoutLogic';
import type { SecondaryTaskbarMode, TestBarConfig } from '../../lib/ipc';
import { CloseWindowIcon, RestoreWindowsIcon } from '../Icons';

export interface StartSettingsOverlayProps {
  config: TestBarConfig | null;
  onClose: () => void;
  onSetBarMode: (mode: 'dock' | 'edge') => Promise<void>;
  onSetSecondaryMode: (mode: SecondaryTaskbarMode) => Promise<void>;
  onSetAutostart: (enable: boolean) => Promise<void>;
  onRestoreWindowsAndExit: () => void;
}

const HOTKEY_ROWS: ReadonlyArray<{ keys: string[]; label: string }> = [
  { keys: ['Win'], label: 'Открыть или закрыть меню Пуск TestBar' },
  { keys: ['Shift'], label: 'Альтернативные действия на плитках' },
  { keys: ['Tab'], label: 'Быстрое переключение пространств' },
  { keys: ['Ctrl', 'Alt', 'Shift', 'Esc'], label: 'Аварийный возврат панели Windows' },
];

export const StartSettingsOverlay: React.FC<StartSettingsOverlayProps> = ({
  config,
  onClose,
  onSetBarMode,
  onSetSecondaryMode,
  onSetAutostart,
  onRestoreWindowsAndExit,
}) => {
  const [autostartBusy, setAutostartBusy] = useState(false);
  const [autostartError, setAutostartError] = useState<string | null>(null);

  const barMode = config?.barMode === 'edge' ? 'edge' : 'dock';
  const secondaryMode: SecondaryTaskbarMode = config?.secondaryTaskbarMode ?? 'testbar';
  const autostart = config?.autostart ?? false;

  const handleAutostartToggle = async () => {
    if (autostartBusy) return;
    setAutostartBusy(true);
    setAutostartError(null);
    try {
      await onSetAutostart(!autostart);
    } catch (err) {
      setAutostartError(formatIpcError(err));
    } finally {
      setAutostartBusy(false);
    }
  };

  return (
    <div className="settings-drawer" onClick={(e) => e.stopPropagation()}>
      <div className="settings-head">
        <div>
          <div className="settings-title">Настройки TestBar</div>
          <div className="settings-sub">Внешний вид панели, поведение мониторов и системная интеграция</div>
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

      <div className="settings-bento">
        {/* Card 1: Bottom Bar Layout Visual Cards */}
        <div className="set-card">
          <div className="set-card-head">
            <div className="set-card-title">Стиль нижней панели</div>
            <div className="set-card-desc">Парящая капсула по центру или классическая полоса на всю ширину</div>
          </div>
          <div className="layout-preview-grid">
            <button
              type="button"
              className={`layout-preview-card ${barMode === 'dock' ? 'active' : ''}`}
              aria-pressed={barMode === 'dock'}
              onClick={() => void onSetBarMode('dock')}
            >
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
              onClick={() => void onSetBarMode('edge')}
            >
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

        {/* Card 2: Secondary Monitor Taskbar */}
        <div className="set-card">
          <div className="set-card-head">
            <div className="set-card-title">Второй монитор</div>
            <div className="set-card-desc">Управление панелью задач на дополнительном дисплее</div>
          </div>
          <div className="set-seg-3" role="radiogroup" aria-label="Режим второго монитора">
            {(
              [
                ['testbar', 'Панель TestBar', 'Синхронная панель TestBar'],
                ['native', 'Штатная Windows', 'Оставить панель Windows'],
                ['hidden', 'Скрывать', 'Чистый второй экран'],
              ] as const
            ).map(([modeKey, label, hint]) => (
              <button
                key={modeKey}
                type="button"
                role="radio"
                aria-checked={secondaryMode === modeKey}
                className={`set-seg-card ${secondaryMode === modeKey ? 'active' : ''}`}
                onClick={() => void onSetSecondaryMode(modeKey)}
              >
                <span className="set-seg-title">{label}</span>
                <span className="set-seg-hint">{hint}</span>
              </button>
            ))}
          </div>
        </div>

        {/* Card 3: Windows Autostart Switch */}
        <div className="set-card">
          <div className="set-switch-row">
            <div>
              <div className="set-card-title">Автозапуск Windows</div>
              <div className="set-card-desc">Автоматически запускать TestBar при входе в систему через реестр пользователя</div>
            </div>
            <button
              type="button"
              role="switch"
              aria-checked={autostart}
              aria-label="Автозапуск Windows"
              disabled={autostartBusy}
              className={`glass-switch ${autostart ? 'on' : ''}`}
              onClick={() => void handleAutostartToggle()}
            >
              <span className="glass-switch-thumb" />
            </button>
          </div>
          {autostartError && <div className="set-error-banner">{autostartError}</div>}
        </div>

        {/* Card 4: Keyboard Shortcuts */}
        <div className="set-card">
          <div className="set-card-head" style={{ marginBottom: 10 }}>
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

        {/* Card 5: Emergency Restore Danger Card */}
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
