import React, { useEffect, useRef } from 'react';
import type { RankedSearchResult, SearchCorpusItem } from '../../lib/searchLogic';
import { ArrowDownIcon, ArrowUpIcon, FallbackAppIcon, SearchIcon } from '../Icons';

export interface StartCommandPaletteProps {
  query: string;
  results: RankedSearchResult;
  selectedIdx: number;
  inputRef: React.RefObject<HTMLInputElement | null>;
  onQueryChange: (next: string) => void;
  onSelectIdx: (idx: number) => void;
  onLaunch: (item: SearchCorpusItem, ctrlKey: boolean) => void;
  onClose: () => void;
}

export const StartCommandPalette: React.FC<StartCommandPaletteProps> = ({
  query,
  results,
  selectedIdx,
  inputRef,
  onQueryChange,
  onSelectIdx,
  onLaunch,
  onClose,
}) => {
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const activeRow = listRef.current?.querySelector<HTMLButtonElement>('.cmd-row.selected');
    activeRow?.scrollIntoView({ block: 'nearest' });
  }, [selectedIdx]);

  return (
    <div className="search-backdrop" onClick={onClose}>
      <div className="cmd-palette" role="dialog" aria-label="Поиск" onClick={(e) => e.stopPropagation()}>
        <div className="cmd-top">
          <SearchIcon className="ic16" color="var(--accent)" />
          <input
            ref={inputRef}
            className="cmd-input"
            autoFocus
            placeholder="Поиск программ, UWP, игр, папок и файлов…"
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
            results.items.map((it, i) => (
              <button
                key={it.id}
                type="button"
                className={`cmd-row ${i === selectedIdx ? 'selected' : ''}`}
                onMouseEnter={() => onSelectIdx(i)}
                onClick={(e) => onLaunch(it, e.ctrlKey)}
              >
                <span className="cmd-ic-slot">
                  {it.iconData ? (
                    <img src={it.iconData} alt="" className="native-ic-sm" draggable={false} />
                  ) : (
                    <FallbackAppIcon iconKey={it.iconKey ?? it.name} kind={it.kind} size={18} />
                  )}
                </span>
                <div className="cmd-row-text">
                  <span className="cmd-row-name">{it.name}</span>
                  {(it.source === 'file' || it.source === 'folder') && (
                    <span className="cmd-row-path">{it.target}</span>
                  )}
                </div>
                <span className="cmd-cat">{it.category}</span>
              </button>
            ))
          )}
        </div>

        <div className="cmd-foot">
          <span className="cmd-foot-group">
            <span className="kbd">Enter</span> Открыть
            <span className="cmd-foot-sep">·</span>
            <span className="kbd">Ctrl+Enter</span> От админа
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
