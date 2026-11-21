import { matchesQuery } from './layoutMap';

export type FileCategory =
  | 'folder'
  | 'image'
  | 'video'
  | 'audio'
  | 'code'
  | 'archive'
  | 'document'
  | 'app'
  | 'file';

export type ExplorerSortBy = 'name' | 'date' | 'size' | 'type';
export type ExplorerSortDir = 'asc' | 'desc';
export type ExplorerViewMode = 'grid' | 'list';

export interface BreadcrumbPart {
  name: string;
  path: string;
}

export interface DriveEntry {
  letter: string;
  path: string;
  label: string;
  totalBytes: number;
  freeBytes: number;
}

export interface QuickRoot {
  id: string;
  label: string;
  path: string;
  kind: string;
}

export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  isHidden: boolean;
  sizeBytes: number;
  modifiedMs: number;
  extension: string;
  category: FileCategory | string;
  hasPreview: boolean;
}

export interface DirectoryListing {
  currentPath: string;
  parentPath: string | null;
  breadcrumbs: BreadcrumbPart[];
  entries: FileEntry[];
  totalCount: number;
  drives: DriveEntry[];
  quickRoots: QuickRoot[];
}

export interface VirtualListRangeParams {
  totalCount: number;
  scrollTop: number;
  viewportHeight: number;
  rowHeight: number;
  overscan?: number;
}

export interface VirtualListRange {
  startIndex: number;
  endIndex: number;
  topSpacerPx: number;
  bottomSpacerPx: number;
  totalHeightPx: number;
}

export interface VirtualGridRangeParams {
  totalCount: number;
  scrollTop: number;
  viewportHeight: number;
  containerWidth: number;
  cellMinWidth: number;
  cellGap: number;
  rowHeight: number;
  overscan?: number;
}

export interface VirtualGridRange {
  columns: number;
  startIndex: number;
  endIndex: number;
  topSpacerPx: number;
  bottomSpacerPx: number;
  totalHeightPx: number;
}

export interface ExplorerHistoryState {
  entries: string[];
  index: number;
}

export interface HistoryNavigationResult {
  state: ExplorerHistoryState;
  path: string | null;
}

const MAX_HISTORY_ENTRIES = 100;

function sanitizeNonNegInt(value: number, fallback = 0): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.max(0, Math.floor(value));
}

function sanitizePositive(value: number, fallback = 1): number {
  if (!Number.isFinite(value) || value <= 0) return fallback;
  return value;
}

function sanitizeNonNeg(value: number, fallback = 0): number {
  if (!Number.isFinite(value) || value < 0) return fallback;
  return value;
}

/**
 * Computes the visible `[startIndex, endIndex)` slice and spacer heights for a virtualized
 * vertical list. Preserves `topSpacerPx + renderedRows * rowHeight + bottomSpacerPx === totalHeightPx`.
 */
export function computeVirtualListRange(params: VirtualListRangeParams): VirtualListRange {
  const totalCount = sanitizeNonNegInt(params.totalCount, 0);
  const rowHeight = sanitizePositive(params.rowHeight, 1);
  const viewportHeight = sanitizeNonNeg(params.viewportHeight, 0);
  const scrollTop = sanitizeNonNeg(params.scrollTop, 0);
  const overscan = sanitizeNonNegInt(params.overscan ?? 4, 4);

  if (totalCount === 0) {
    return {
      startIndex: 0,
      endIndex: 0,
      topSpacerPx: 0,
      bottomSpacerPx: 0,
      totalHeightPx: 0,
    };
  }

  const totalHeightPx = totalCount * rowHeight;
  const maxScroll = Math.max(0, totalHeightPx - viewportHeight);
  const effectiveScroll = Math.min(scrollTop, maxScroll);

  const firstVisibleRow = Math.floor(effectiveScroll / rowHeight);
  const lastVisibleRowExcl =
    viewportHeight > 0 ? Math.ceil((effectiveScroll + viewportHeight) / rowHeight) : firstVisibleRow;

  const startIndex = Math.max(0, Math.min(totalCount, firstVisibleRow - overscan));
  const endIndex = Math.max(startIndex, Math.min(totalCount, lastVisibleRowExcl + overscan));

  const topSpacerPx = startIndex * rowHeight;
  const bottomSpacerPx = Math.max(0, (totalCount - endIndex) * rowHeight);

  return {
    startIndex,
    endIndex,
    topSpacerPx,
    bottomSpacerPx,
    totalHeightPx,
  };
}

/**
 * Computes responsive column count, visible `[startIndex, endIndex)` item slice (row-aligned),
 * and spacer heights for a virtualized icon/thumbnail grid.
 */
export function computeVirtualGridRange(params: VirtualGridRangeParams): VirtualGridRange {
  const totalCount = sanitizeNonNegInt(params.totalCount, 0);
  const rowHeight = sanitizePositive(params.rowHeight, 1);
  const viewportHeight = sanitizeNonNeg(params.viewportHeight, 0);
  const scrollTop = sanitizeNonNeg(params.scrollTop, 0);
  const cellMinWidth = sanitizePositive(params.cellMinWidth, 96);
  const cellGap = sanitizeNonNeg(params.cellGap, 0);
  const containerWidth = sanitizePositive(params.containerWidth, cellMinWidth);
  const overscan = sanitizeNonNegInt(params.overscan ?? 2, 2);

  const columns = Math.max(1, Math.floor((containerWidth + cellGap) / (cellMinWidth + cellGap)));

  if (totalCount === 0) {
    return {
      columns,
      startIndex: 0,
      endIndex: 0,
      topSpacerPx: 0,
      bottomSpacerPx: 0,
      totalHeightPx: 0,
    };
  }

  const totalRows = Math.ceil(totalCount / columns);
  const totalHeightPx = totalRows * rowHeight;
  const maxScroll = Math.max(0, totalHeightPx - viewportHeight);
  const effectiveScroll = Math.min(scrollTop, maxScroll);

  const firstVisibleRow = Math.floor(effectiveScroll / rowHeight);
  const lastVisibleRowExcl =
    viewportHeight > 0 ? Math.ceil((effectiveScroll + viewportHeight) / rowHeight) : firstVisibleRow;

  const startRow = Math.max(0, Math.min(totalRows, firstVisibleRow - overscan));
  const endRow = Math.max(startRow, Math.min(totalRows, lastVisibleRowExcl + overscan));

  const startIndex = Math.min(totalCount, startRow * columns);
  const endIndex = Math.min(totalCount, endRow * columns);

  const topSpacerPx = startRow * rowHeight;
  const bottomSpacerPx = Math.max(0, (totalRows - endRow) * rowHeight);

  return {
    columns,
    startIndex,
    endIndex,
    topSpacerPx,
    bottomSpacerPx,
    totalHeightPx,
  };
}

function normalizePathKey(path: string): string {
  let norm = path.trim().replace(/\//g, '\\').toLowerCase();
  while (norm.length > 3 && norm.endsWith('\\')) {
    norm = norm.slice(0, -1);
  }
  return norm;
}

export function createHistoryState(initialPath?: string): ExplorerHistoryState {
  const trimmed = initialPath?.trim() ?? '';
  if (!trimmed) {
    return { entries: [], index: -1 };
  }
  return { entries: [trimmed], index: 0 };
}

export function canGoBack(state: ExplorerHistoryState): boolean {
  return state.index > 0 && state.index < state.entries.length;
}

export function canGoForward(state: ExplorerHistoryState): boolean {
  return state.index >= 0 && state.index < state.entries.length - 1;
}

export function pushHistoryPath(
  state: ExplorerHistoryState,
  path: string,
): ExplorerHistoryState {
  const trimmed = path.trim();
  if (!trimmed) return state;

  const current = state.index >= 0 ? state.entries[state.index] : undefined;
  if (current !== undefined && normalizePathKey(current) === normalizePathKey(trimmed)) {
    return state;
  }

  const head = state.index >= 0 ? state.entries.slice(0, state.index + 1) : [];
  let nextEntries = [...head, trimmed];
  if (nextEntries.length > MAX_HISTORY_ENTRIES) {
    nextEntries = nextEntries.slice(nextEntries.length - MAX_HISTORY_ENTRIES);
  }

  return {
    entries: nextEntries,
    index: nextEntries.length - 1,
  };
}

export function navigateHistoryBack(state: ExplorerHistoryState): HistoryNavigationResult {
  if (!canGoBack(state)) {
    return { state, path: null };
  }
  const nextIndex = state.index - 1;
  return {
    state: {
      entries: state.entries,
      index: nextIndex,
    },
    path: state.entries[nextIndex] ?? null,
  };
}

export function navigateHistoryForward(state: ExplorerHistoryState): HistoryNavigationResult {
  if (!canGoForward(state)) {
    return { state, path: null };
  }
  const nextIndex = state.index + 1;
  return {
    state: {
      entries: state.entries,
      index: nextIndex,
    },
    path: state.entries[nextIndex] ?? null,
  };
}

const CATEGORY_LABELS_RU: Record<string, string> = {
  folder: 'Папка',
  image: 'Изображение',
  video: 'Видео',
  audio: 'Аудио',
  code: 'Код',
  archive: 'Архив',
  document: 'Документ',
  app: 'Приложение',
  file: 'Файл',
};

export function getEntryTypeLabel(
  entry: Pick<FileEntry, 'isDir' | 'category' | 'extension'>,
): string {
  if (entry.isDir || entry.category === 'folder') {
    return 'Папка';
  }
  const base = CATEGORY_LABELS_RU[entry.category] ?? 'Файл';
  const ext = entry.extension.trim().replace(/^\./, '').toUpperCase();
  return ext ? `${base} ${ext}` : base;
}

/**
 * Filters directory entries client-side using `matchesQuery` (`src/lib/layoutMap.ts`)
 * so wrong-keyboard-layout RU↔EN queries match seamlessly across name, extension, and type label.
 */
export function filterDirectoryEntries(entries: FileEntry[], rawQuery: string): FileEntry[] {
  const trimmed = rawQuery.trim();
  if (!trimmed || entries.length === 0) {
    return entries;
  }

  const terms = trimmed.split(/\s+/).filter(Boolean);

  return entries.filter((entry) => {
    const typeLabel = getEntryTypeLabel(entry);
    if (matchesQuery(trimmed, entry.name, entry.extension, typeLabel).matched) {
      return true;
    }
    if (terms.length > 1) {
      return terms.every(
        (term) => matchesQuery(term, entry.name, entry.extension, typeLabel).matched,
      );
    }
    return false;
  });
}

function formatScaledUnit(value: number): string {
  const rounded = Math.round(value * 10) / 10;
  return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
}

export function formatFileSize(bytes: number, isDir = false): string {
  if (isDir) return '—';
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 Б';
  if (bytes < 1024) return `${Math.floor(bytes)} Б`;

  const kb = bytes / 1024;
  if (kb < 1024) return `${formatScaledUnit(kb)} КБ`;

  const mb = kb / 1024;
  if (mb < 1024) return `${formatScaledUnit(mb)} МБ`;

  const gb = mb / 1024;
  if (gb < 1024) return `${formatScaledUnit(gb)} ГБ`;

  const tb = gb / 1024;
  return `${formatScaledUnit(tb)} ТБ`;
}

function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export function formatFileDate(modifiedMs: number, nowMs: number = Date.now()): string {
  if (!Number.isFinite(modifiedMs) || modifiedMs <= 0) {
    return '—';
  }
  const date = new Date(modifiedMs);
  if (Number.isNaN(date.getTime())) {
    return '—';
  }

  const hhmm = `${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  const now = new Date(nowMs);
  if (!Number.isNaN(now.getTime())) {
    if (isSameDay(date, now)) {
      return `Сегодня, ${hhmm}`;
    }
    const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
    if (isSameDay(date, yesterday)) {
      return `Вчера, ${hhmm}`;
    }
  }

  return `${pad2(date.getDate())}.${pad2(date.getMonth() + 1)}.${date.getFullYear()} ${hhmm}`;
}
