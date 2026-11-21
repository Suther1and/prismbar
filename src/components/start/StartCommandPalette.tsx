import React, { useEffect, useRef } from 'react';
import { canRunAsAdmin } from '../../lib/flyoutLogic';
import {
  getPrimaryActionLabel,
  resolveSearchSecondaryAction,
  type RankedSearchResult,
  type SearchCorpusItem,
} from '../../lib/searchLogic';
import { useAnimatedPresence } from '../../lib/useAnimatedPresence';
import {
  ArrowDownIcon,
  ArrowUpIcon,
  BluetoothIcon,
  BroomIcon,
  DiskIcon,
  FallbackAppIcon,
  GamepadIcon,
  GlobeSvg,
  LockIcon,
  MicIcon,
  NetIcon,
  PasteIcon,
  PowerIcon,
  RebootIcon,
  RefreshIcon,
  ScissorsIcon,
  SearchIcon,
  SettingsGearIcon,
  ShieldIcon,
  SleepIcon,
  TaskmgrIcon,
  VolIcon,
} from '../Icons';

export interface StartCommandPaletteProps {
  open: boolean;
  trigger: 'keyboard' | 'click';
  query: string;
  results: RankedSearchResult;
  selectedIdx: number;
  inputRef: React.RefObject<HTMLInputElement | null>;
  onQueryChange: (next: string) => void;
  onSelectIdx: (idx: number) => void;
  onLaunch: (item: SearchCorpusItem, ctrlKey: boolean, altKey?: boolean) => void;
  onItemContextMenu?: (e: React.MouseEvent, item: SearchCorpusItem) => void;
  onClose: () => void;
}

function renderHighlightedName(name: string, indices?: number[]): React.ReactNode {
  if (!indices || indices.length === 0) return name;
  const set = new Set(indices);
  const parts: React.ReactNode[] = [];
  let buf = '';
  let inMark = false;

  for (let i = 0; i < name.length; i++) {
    const hit = set.has(i);
    if (hit !== inMark) {
      if (buf) {
        parts.push(
          inMark ? (
            <span key={`m-${i}`} className="cmd-match-hl">
              {buf}
            </span>
          ) : (
            <React.Fragment key={`t-${i}`}>{buf}</React.Fragment>
          ),
        );
      }
      buf = name[i];
      inMark = hit;
    } else {
      buf += name[i];
    }
  }
  if (buf) {
    parts.push(
      inMark ? (
        <span key="m-end" className="cmd-match-hl">
          {buf}
        </span>
      ) : (
        <React.Fragment key="t-end">{buf}</React.Fragment>
      ),
    );
  }
  return parts;
}

function renderCommandIcon(item: SearchCorpusItem): React.ReactNode {
  if (item.source === 'clipboard') {
    if (item.clipColor) {
      return <span className="clip-swatch" style={{ background: item.clipColor }} />;
    }
    return <PasteIcon className="ic16" />;
  }
  if (item.source === 'web') {
    return <GlobeSvg className="ic16" />;
  }
  if (item.source === 'audio_device') {
    return <VolIcon className="ic16" />;
  }
  if (item.source === 'win_settings') {
    const idOrTarget = `${item.id} ${item.target}`.toLowerCase();
    if (idOrTarget.includes('wifi')) {
      return <NetIcon kind="wifi" className="ic16" />;
    }
    if (
      idOrTarget.includes('network') ||
      idOrTarget.includes('ethernet') ||
      idOrTarget.includes('vpn') ||
      idOrTarget.includes('proxy') ||
      idOrTarget.includes('ncpa')
    ) {
      return <NetIcon className="ic16" />;
    }
    if (idOrTarget.includes('bluetooth')) {
      return <BluetoothIcon className="ic16" />;
    }
    if (idOrTarget.includes('sound') || idOrTarget.includes('mmsys')) {
      return <VolIcon className="ic16" />;
    }
    if (idOrTarget.includes('storage') || idOrTarget.includes('diskmgmt')) {
      return <DiskIcon className="ic16" />;
    }
    if (idOrTarget.includes('defender')) {
      return <ShieldIcon className="ic16" />;
    }
    if (idOrTarget.includes('gaming')) {
      return <GamepadIcon className="ic16" />;
    }
    return <SettingsGearIcon className="ic16" />;
  }
  if (item.source === 'command') {
    switch (item.commandId) {
      case 'mic_toggle':
        return <MicIcon className="ic16" muted={Boolean(item.micMuted)} />;
      case 'empty_recycle_bin':
        return <BroomIcon className="ic16" />;
      case 'screenshot':
        return <ScissorsIcon className="ic16" />;
      case 'refresh_indexes':
        return <RefreshIcon className="ic16" />;
      case 'task_manager':
        return <TaskmgrIcon className="ic16" />;
      case 'settings':
        return <SettingsGearIcon className="ic16" />;
      case 'lock':
        return <LockIcon className="ic16" />;
      case 'sleep':
        return <SleepIcon className="ic16" />;
      case 'reboot':
        return <RebootIcon className="ic16" />;
      case 'shutdown':
        return <PowerIcon className="ic16" />;
      default:
        break;
    }
  }
  if (item.iconData) {
    return <img src={item.iconData} alt="" className="native-ic-sm" draggable={false} />;
  }
  return <FallbackAppIcon iconKey={item.iconKey ?? item.name} kind={item.kind} size={18} />;
}

export const StartCommandPalette: React.FC<StartCommandPaletteProps> = ({
  open,
  trigger,
  query,
  results,
  selectedIdx,
  inputRef,
  onQueryChange,
  onSelectIdx,
  onLaunch,
  onItemContextMenu,
  onClose,
}) => {
  const { mounted, phase } = useAnimatedPresence(open, 90);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    requestAnimationFrame(() => {
      const input = inputRef.current;
      if (!input) return;
      input.focus();
      const len = input.value.length;
      try {
        input.setSelectionRange(len, len);
      } catch {
        // Ignore if selection range unavailable
      }
    });
  }, [open, inputRef]);

  useEffect(() => {
    if (!mounted) return;
    const activeRow = listRef.current?.querySelector<HTMLButtonElement>('.cmd-row.selected');
    activeRow?.scrollIntoView({ block: 'nearest' });
  }, [selectedIdx, mounted]);

  if (!mounted) return null;

  const safeSelectedIdx =
    results.items.length > 0 ? Math.max(0, Math.min(selectedIdx, results.items.length - 1)) : 0;
  const selectedItem = results.items[safeSelectedIdx];
  const primaryLabel = getPrimaryActionLabel(selectedItem);
  const secondaryAction = resolveSearchSecondaryAction(selectedItem);
  const showAdminHint =
    selectedItem &&
    selectedItem.source !== 'window' &&
    selectedItem.source !== 'win_settings' &&
    selectedItem.source !== 'clipboard' &&
    selectedItem.source !== 'audio_device' &&
    selectedItem.source !== 'web' &&
    (selectedItem.source !== 'command' || selectedItem.commandId === 'task_manager') &&
    canRunAsAdmin(selectedItem.target);

  return (
    <div className="search-backdrop" data-phase={phase} onClick={onClose}>
      <div
        className="cmd-palette"
        data-trigger={trigger}
        role="dialog"
        aria-label="Поиск и команды"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="cmd-top">
          <SearchIcon className="ic16" color="var(--accent)" />
          <input
            ref={inputRef}
            className="cmd-input"
            autoFocus
            placeholder="Окна, программы, команды, буфер (clip), поиск (g / yt / gh)…"
            value={query}
            onChange={(e) => {
              onQueryChange(e.target.value);
              onSelectIdx(0);
            }}
          />
          {results.swappedHint && (
            <span className="cmd-layout-hint">Раскладка: «{results.swappedHint}»</span>
          )}
          <span className="kbd">ESC</span>
        </div>

        <div ref={listRef} className="cmd-list-wrap">
          {results.items.length === 0 ? (
            <div className="cmd-empty">Ничего не найдено</div>
          ) : (
            results.items.map((it, i) => {
              const showSubline =
                it.source === 'window' ||
                it.source === 'win_settings' ||
                it.source === 'file' ||
                it.source === 'folder' ||
                it.source === 'clipboard' ||
                it.source === 'audio_device' ||
                it.source === 'command';
              const sublineText =
                it.source === 'file' || it.source === 'folder' ? it.target : it.subtitle;

              return (
                <button
                  key={it.id}
                  type="button"
                  className={`cmd-row ${i === safeSelectedIdx ? 'selected' : ''}`}
                  onMouseEnter={() => onSelectIdx(i)}
                  onClick={(e) => onLaunch(it, e.ctrlKey, e.altKey)}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    onSelectIdx(i);
                    onItemContextMenu?.(e, it);
                  }}
                >
                  <span className="cmd-ic-slot">{renderCommandIcon(it)}</span>
                  <div className="cmd-row-text">
                    <span className="cmd-row-name">
                      {renderHighlightedName(it.name, results.matchIndices[it.id])}
                    </span>
                    {showSubline && sublineText && (
                      <span className="cmd-row-path">{sublineText}</span>
                    )}
                  </div>
                  {it.badge ? (
                    <span className={`cmd-badge badge-${it.badge.variant}`}>
                      {it.badge.text}
                    </span>
                  ) : (
                    <span className="cmd-cat">{it.category}</span>
                  )}
                </button>
              );
            })
          )}
        </div>

        <div className="cmd-foot">
          <span className="cmd-foot-group">
            <span className="kbd">Enter</span> {primaryLabel}
            {showAdminHint && (
              <>
                <span className="cmd-foot-sep">·</span>
                <span className="kbd">Ctrl+Enter</span> Администратор
              </>
            )}
            {secondaryAction && (
              <>
                <span className="cmd-foot-sep">·</span>
                <span className="kbd">Alt+Enter</span> {secondaryAction.label}
              </>
            )}
          </span>
          <span className="cmd-foot-group">
            <span className="kbd cmd-kbd-ic"><ArrowUpIcon size={9} /></span>
            <span className="kbd cmd-kbd-ic"><ArrowDownIcon size={9} /></span>
            <span>Выбор</span>
          </span>
        </div>
      </div>
    </div>
  );
};
