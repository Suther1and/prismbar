import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  computeVirtualGridRange,
  computeVirtualListRange,
  createHistoryState,
  filterDirectoryEntries,
  formatFileDate,
  formatFileSize,
  getEntryTypeLabel,
  navigateHistoryBack,
  navigateHistoryForward,
  pushHistoryPath,
  canGoBack,
  canGoForward,
  type ExplorerHistoryState,
  type ExplorerSortBy,
  type ExplorerSortDir,
  type ExplorerViewMode,
} from '../../lib/fileExplorerLogic';
import { formatIpcError } from '../../lib/flyoutLogic';
import type { ExplorerSessionSnapshot } from '../../lib/startLogic';
import {
  fsCopyFilesToClipboard,
  fsCreateFolder,
  fsOpenInExplorer,
  fsOpenItem,
  fsRenameItem,
  fsTrashItems,
  getFileThumbnails,
  ipc,
  listDirectory,
  LruIconCache,
  type DirectoryListing,
  type DriveEntry,
  type FileEntry,
} from '../../lib/ipc';
import '../../styles/explorer.css';
import {
  ArrowLeftIcon,
  ArrowRightIcon,
  ArrowUpIcon,
  ChevronRightIcon,
  CloseWindowIcon,
  CopyIcon,
  DownloadIcon,
  DriveIcon,
  EditPencilIcon,
  ExternalWindowIcon,
  EyeIcon,
  EyeOffIcon,
  FileArchiveIcon,
  FileAudioIcon,
  FileCodeIcon,
  FileDocIcon,
  FileExeIcon,
  FileImageIcon,
  FileSvg,
  FileVideoIcon,
  FolderPlusIcon,
  FolderSvg,
  GridViewIcon,
  HomeIcon,
  ListViewIcon,
  PinIcon,
  RefreshIcon,
  RenameIcon,
  SearchIcon,
  SortIcon,
  TerminalIcon,
  TrashIcon,
} from '../Icons';

export type { ExplorerSessionSnapshot };

export interface StartFileExplorerProps {
  initialPath: string;
  activeSpaceId: string;
  initialSession?: ExplorerSessionSnapshot | null;
  onSessionChange?: (snap: ExplorerSessionSnapshot) => void;
  onClose: () => void;
  onPinFolder: (path: string, name: string) => Promise<void> | void;
  onShowToast: (msg: string) => void;
}

const LIST_ROW_HEIGHT = 36;
const LIST_OVERSCAN = 5;
const GRID_CELL_MIN_WIDTH = 112;
const GRID_CELL_GAP = 8;
const GRID_ROW_HEIGHT = 116;
const GRID_OVERSCAN = 2;
const VIEWPORT_PADDING_PX = 12;

const THUMB_CACHE_MAX = 400;
const THUMB_BATCH_LIMIT = 40;
const THUMB_DEBOUNCE_MS = 40;
const FAILED_THUMB_MAX = 800;

const SORT_OPTIONS: ReadonlyArray<{ value: ExplorerSortBy; label: string }> = [
  { value: 'name', label: 'Имя' },
  { value: 'date', label: 'Дата' },
  { value: 'size', label: 'Размер' },
  { value: 'type', label: 'Тип' },
];

interface ExplorerCtxMenuState {
  x: number;
  y: number;
  entry: FileEntry;
}

interface ExplorerCtxAction {
  id: string;
  label: string;
  danger?: boolean;
  icon: React.ReactNode;
  run: () => void;
}

function normalizePathKey(p: string | null | undefined): string {
  if (!p) return '';
  let norm = p.trim().replace(/\//g, '\\').toLowerCase();
  while (norm.length > 3 && norm.endsWith('\\')) {
    norm = norm.slice(0, -1);
  }
  return norm;
}

function makeThumbCacheKey(
  entry: Pick<FileEntry, 'path' | 'modifiedMs'>,
  thumbSize: number,
): string {
  return `${entry.path}|${entry.modifiedMs}|${thumbSize}`;
}

function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable;
}

function deriveFolderDisplayName(path: string, listing: DirectoryListing | null): string {
  if (listing && normalizePathKey(listing.currentPath) === normalizePathKey(path)) {
    const lastCrumb = listing.breadcrumbs[listing.breadcrumbs.length - 1]?.name;
    if (lastCrumb) return lastCrumb;
  }
  const parts = path.trim().replace(/\//g, '\\').split('\\').filter(Boolean);
  return parts[parts.length - 1] || path;
}

function getDriveUsageLevel(drive: DriveEntry): { ratio: number; levelClass: string } {
  if (drive.totalBytes <= 0) {
    return { ratio: 0, levelClass: '' };
  }
  const used = Math.max(0, drive.totalBytes - drive.freeBytes);
  const ratio = Math.min(1, Math.max(0, used / drive.totalBytes));
  const levelClass = ratio >= 0.92 ? 'lvl-hot' : ratio >= 0.8 ? 'lvl-warn' : '';
  return { ratio, levelClass };
}

function renderQuickRootIcon(kind: string): React.ReactNode {
  switch (kind) {
    case 'home':
      return <HomeIcon className="ic14" />;
    case 'downloads':
      return <DownloadIcon className="ic14" />;
    default:
      return <FolderSvg className="ic14" />;
  }
}

function renderEntryFallbackIcon(entry: FileEntry, className: string): React.ReactNode {
  if (entry.isDir || entry.category === 'folder') {
    return <FolderSvg className={`${className} fx-ic-folder`} />;
  }
  switch (entry.category) {
    case 'image':
      return <FileImageIcon className={`${className} fx-ic-image`} />;
    case 'video':
      return <FileVideoIcon className={`${className} fx-ic-video`} />;
    case 'audio':
      return <FileAudioIcon className={`${className} fx-ic-audio`} />;
    case 'code':
      return <FileCodeIcon className={`${className} fx-ic-code`} />;
    case 'archive':
      return <FileArchiveIcon className={`${className} fx-ic-archive`} />;
    case 'document':
      return <FileDocIcon className={`${className} fx-ic-doc`} />;
    case 'app':
      return <FileExeIcon className={`${className} fx-ic-app`} />;
    default:
      return <FileSvg className={`${className} fx-ic-file`} />;
  }
}

function chunkItems<T>(items: readonly T[], chunkSize: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < items.length; i += chunkSize) {
    out.push(items.slice(i, i + chunkSize));
  }
  return out;
}

async function copyTextToClipboard(text: string): Promise<void> {
  try {
    await ipc('copy_to_clipboard', { text });
    return;
  } catch (err) {
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return;
    }
    throw err;
  }
}

export const StartFileExplorer: React.FC<StartFileExplorerProps> = ({
  initialPath,
  activeSpaceId,
  initialSession = null,
  onSessionChange,
  onClose,
  onPinFolder,
  onShowToast,
}) => {
  const bootPath = initialSession?.path?.trim() || initialPath;
  const [listing, setListing] = useState<DirectoryListing | null>(null);
  const [history, setHistory] = useState<ExplorerHistoryState>(() =>
    createHistoryState(bootPath),
  );
  const [viewMode, setViewMode] = useState<ExplorerViewMode>(
    () => initialSession?.viewMode ?? 'list',
  );
  const [sortBy, setSortBy] = useState<ExplorerSortBy>(() => initialSession?.sortBy ?? 'name');
  const [sortDir, setSortDir] = useState<ExplorerSortDir>(() => initialSession?.sortDir ?? 'asc');
  const [showHidden, setShowHidden] = useState(false);
  const [filterQuery, setFilterQuery] = useState('');
  const [selectedPath, setSelectedPath] = useState<string | null>(
    () => initialSession?.selectedPath ?? null,
  );

  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  // Quick path jump editor
  const [pathEditing, setPathEditing] = useState(false);
  const [pathDraft, setPathDraft] = useState('');
  const pathSubmittingRef = useRef(false);

  // Inline + Папка bar
  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [newFolderName, setNewFolderName] = useState('');
  const [newFolderError, setNewFolderError] = useState<string | null>(null);
  const [creatingFolder, setCreatingFolder] = useState(false);

  // Inline F2 rename
  const [renamingPath, setRenamingPath] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState('');
  const [renamingBusy, setRenamingBusy] = useState(false);
  const renameCommitLockRef = useRef(false);

  // Context menu
  const [ctxMenu, setCtxMenu] = useState<ExplorerCtxMenuState | null>(null);
  const [ctxIdx, setCtxIdx] = useState(0);

  // Pinning guard
  const [pinningFolder, setPinningFolder] = useState(false);

  // Viewport virtualization measurements
  const [viewportSize, setViewportSize] = useState<{ width: number; height: number }>({
    width: 880,
    height: 450,
  });
  const [scrollTop, setScrollTop] = useState(() =>
    initialSession && Number.isFinite(initialSession.scrollTop)
      ? Math.max(0, initialSession.scrollTop)
      : 0,
  );

  // Thumbnail LRU cache & reactive lookup map
  const thumbLruRef = useRef<LruIconCache>(new LruIconCache(THUMB_CACHE_MAX));
  const failedThumbKeysRef = useRef<Set<string>>(new Set());
  const inFlightThumbKeysRef = useRef<Set<string>>(new Set());
  const [thumbMap, setThumbMap] = useState<Record<string, string>>({});

  // Refs for lifecycle & stale async guards
  const mountedRef = useRef(true);
  const navSeqRef = useRef(0);
  const activePathRef = useRef(bootPath);
  const thumbSizeRef = useRef<number>(32);
  const prevInitialPathRef = useRef(initialPath);
  const pendingRestoreRef = useRef<{
    scrollTop: number;
    selectedPath: string | null;
  } | null>(
    initialSession
      ? {
          scrollTop: Number.isFinite(initialSession.scrollTop)
            ? Math.max(0, initialSession.scrollTop)
            : 0,
          selectedPath: initialSession.selectedPath ?? null,
        }
      : null,
  );
  const onSessionChangeRef = useRef(onSessionChange);
  onSessionChangeRef.current = onSessionChange;
  const historyRef = useRef(history);
  historyRef.current = history;
  const listingRef = useRef(listing);
  listingRef.current = listing;

  const rootRef = useRef<HTMLDivElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const breadcrumbsRef = useRef<HTMLDivElement>(null);
  const pathInputRef = useRef<HTMLInputElement>(null);
  const filterInputRef = useRef<HTMLInputElement>(null);
  const newFolderInputRef = useRef<HTMLInputElement>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);
  const ctxMenuRef = useRef<HTMLDivElement>(null);

  const currentPath = listing?.currentPath ?? activePathRef.current;
  const thumbSize = viewMode === 'grid' ? 72 : 32;
  thumbSizeRef.current = thumbSize;

  const focusRoot = useCallback(() => {
    requestAnimationFrame(() => {
      rootRef.current?.focus({ preventScroll: true });
    });
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    focusRoot();
    return () => {
      mountedRef.current = false;
    };
  }, [focusRoot]);

  const loadDirectory = useCallback(
    async (
      targetPath: string,
      opts: {
        mode: 'init' | 'push' | 'history' | 'refresh';
        nextHistory?: ExplorerHistoryState;
        sortBy: ExplorerSortBy;
        sortDir: ExplorerSortDir;
        showHidden: boolean;
        selectAfterLoad?: string | null;
      },
    ): Promise<boolean> => {
      const seq = ++navSeqRef.current;
      const hadListing = listingRef.current !== null;
      if (!hadListing || opts.mode !== 'refresh') {
        setLoading(true);
      }
      if (opts.mode === 'init') {
        setLoadError(null);
      }

      try {
        const res = await listDirectory(targetPath, {
          sortBy: opts.sortBy,
          sortDir: opts.sortDir,
          showHidden: opts.showHidden,
        });
        if (!mountedRef.current || navSeqRef.current !== seq) {
          return false;
        }

        activePathRef.current = res.currentPath;
        setListing(res);
        setLoadError(null);
        setLoading(false);
        setCtxMenu(null);
        setRenamingPath(null);

        if (opts.mode === 'init') {
          const restore = pendingRestoreRef.current;
          pendingRestoreRef.current = null;
          setHistory(createHistoryState(res.currentPath));
          setFilterQuery('');
          setNewFolderOpen(false);
          setPathEditing(false);
          if (restore) {
            const matchedSel =
              restore.selectedPath && res.entries.some((e) => e.path === restore.selectedPath)
                ? restore.selectedPath
                : null;
            const restoredScroll = Math.max(0, restore.scrollTop);
            setSelectedPath(opts.selectAfterLoad ?? matchedSel);
            setScrollTop(restoredScroll);
            if (typeof requestAnimationFrame === 'function') {
              requestAnimationFrame(() => {
                if (viewportRef.current) {
                  viewportRef.current.scrollTop = restoredScroll;
                }
              });
            } else if (viewportRef.current) {
              viewportRef.current.scrollTop = restoredScroll;
            }
          } else {
            setSelectedPath(opts.selectAfterLoad ?? null);
            setScrollTop(0);
            if (viewportRef.current) viewportRef.current.scrollTop = 0;
          }
        } else if (opts.mode === 'push') {
          pendingRestoreRef.current = null;
          setHistory((prev) => pushHistoryPath(prev, res.currentPath));
          setFilterQuery('');
          setSelectedPath(opts.selectAfterLoad ?? null);
          setNewFolderOpen(false);
          setPathEditing(false);
          setScrollTop(0);
          if (viewportRef.current) viewportRef.current.scrollTop = 0;
        } else if (opts.mode === 'history' && opts.nextHistory) {
          pendingRestoreRef.current = null;
          setHistory(opts.nextHistory);
          setFilterQuery('');
          setSelectedPath(opts.selectAfterLoad ?? null);
          setNewFolderOpen(false);
          setPathEditing(false);
          setScrollTop(0);
          if (viewportRef.current) viewportRef.current.scrollTop = 0;
        } else if (opts.mode === 'refresh') {
          if (opts.selectAfterLoad !== undefined) {
            setSelectedPath(opts.selectAfterLoad);
          } else {
            setSelectedPath((prevSel) =>
              prevSel && res.entries.some((e) => e.path === prevSel) ? prevSel : null,
            );
          }
        }

        return true;
      } catch (err) {
        if (!mountedRef.current || navSeqRef.current !== seq) {
          return false;
        }
        const msg = formatIpcError(err);
        setLoading(false);
        if (!listingRef.current) {
          setLoadError(msg);
        }
        onShowToast(msg);
        return false;
      }
    },
    [onShowToast],
  );

  // Initial load & reload when initialPath prop changes externally
  useEffect(() => {
    const isInitialMount = prevInitialPathRef.current === initialPath && listingRef.current === null;
    const pathChanged = prevInitialPathRef.current !== initialPath;
    if (isInitialMount || pathChanged) {
      prevInitialPathRef.current = initialPath;
      if (pathChanged) {
        pendingRestoreRef.current = null;
      }
      const targetToLoad =
        isInitialMount && initialSession?.path?.trim() ? initialSession.path.trim() : initialPath;
      activePathRef.current = targetToLoad;
      void loadDirectory(targetToLoad, {
        mode: 'init',
        sortBy,
        sortDir,
        showHidden,
      });
    }
    // Intentionally trigger only on initialPath changes; sort/hidden changes use dedicated refresh below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialPath, loadDirectory]);

  // Emit live session snapshot for 3-minute Start resume
  useEffect(() => {
    if (!listing?.currentPath || pendingRestoreRef.current !== null) {
      return;
    }
    onSessionChangeRef.current?.({
      path: listing.currentPath,
      viewMode,
      sortBy,
      sortDir,
      scrollTop,
      selectedPath,
    });
  }, [listing?.currentPath, viewMode, sortBy, sortDir, scrollTop, selectedPath]);

  // Reload when sort / hidden toggle changes (StrictMode-safe key diff)
  const sortHiddenKey = `${sortBy}|${sortDir}|${showHidden}`;
  const prevSortHiddenKeyRef = useRef(sortHiddenKey);
  useEffect(() => {
    if (prevSortHiddenKeyRef.current === sortHiddenKey) {
      return;
    }
    prevSortHiddenKeyRef.current = sortHiddenKey;
    const pathToReload = listingRef.current?.currentPath ?? activePathRef.current;
    if (!pathToReload && !listingRef.current) return;
    void loadDirectory(pathToReload, {
      mode: 'refresh',
      sortBy,
      sortDir,
      showHidden,
    });
  }, [sortHiddenKey, sortBy, sortDir, showHidden, loadDirectory]);

  // Auto-scroll breadcrumbs tail into view when directory changes
  useEffect(() => {
    const el = breadcrumbsRef.current;
    if (!el || pathEditing) return;
    el.scrollLeft = el.scrollWidth;
  }, [listing?.currentPath, pathEditing]);

  // Measure scroll viewport with ResizeObserver
  useLayoutEffect(() => {
    const el = viewportRef.current;
    if (!el) return;

    const measure = () => {
      const w = el.clientWidth;
      const h = el.clientHeight;
      if (w > 0 || h > 0) {
        setViewportSize((prev) =>
          prev.width === w && prev.height === h ? prev : { width: w, height: h },
        );
      }
    };
    measure();

    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => measure());
    ro.observe(el);
    return () => ro.disconnect();
  }, [viewMode]);

  // Keep focus inside Explorer if a focused toolbar button becomes disabled
  useEffect(() => {
    if (typeof document === 'undefined') return;
    if (
      document.activeElement === document.body &&
      !document.querySelector('.settings-backdrop, .search-backdrop, .all-apps-drawer')
    ) {
      rootRef.current?.focus({ preventScroll: true });
    }
  }, [listing, history.index]);

  const navigatePush = useCallback(
    (targetPath: string) => {
      return loadDirectory(targetPath, {
        mode: 'push',
        sortBy,
        sortDir,
        showHidden,
      });
    },
    [loadDirectory, sortBy, sortDir, showHidden],
  );

  const handleGoBack = useCallback(() => {
    const nav = navigateHistoryBack(historyRef.current);
    if (!nav.path) return;
    void loadDirectory(nav.path, {
      mode: 'history',
      nextHistory: nav.state,
      sortBy,
      sortDir,
      showHidden,
    });
  }, [loadDirectory, sortBy, sortDir, showHidden]);

  const handleGoForward = useCallback(() => {
    const nav = navigateHistoryForward(historyRef.current);
    if (!nav.path) return;
    void loadDirectory(nav.path, {
      mode: 'history',
      nextHistory: nav.state,
      sortBy,
      sortDir,
      showHidden,
    });
  }, [loadDirectory, sortBy, sortDir, showHidden]);

  const handleGoUp = useCallback(() => {
    const parent = listingRef.current?.parentPath;
    if (!parent) return;
    void navigatePush(parent);
  }, [navigatePush]);

  const handleRefresh = useCallback(() => {
    failedThumbKeysRef.current.clear();
    const target = listingRef.current?.currentPath ?? activePathRef.current;
    void loadDirectory(target, {
      mode: 'refresh',
      sortBy,
      sortDir,
      showHidden,
    });
  }, [loadDirectory, sortBy, sortDir, showHidden]);

  const allEntries = listing?.entries ?? [];
  const filteredEntries = useMemo(
    () => filterDirectoryEntries(allEntries, filterQuery),
    [allEntries, filterQuery],
  );

  const selectedEntry = useMemo(
    () => (selectedPath ? filteredEntries.find((e) => e.path === selectedPath) ?? null : null),
    [filteredEntries, selectedPath],
  );

  const listRange = useMemo(
    () =>
      computeVirtualListRange({
        totalCount: filteredEntries.length,
        scrollTop,
        viewportHeight: Math.max(0, viewportSize.height - VIEWPORT_PADDING_PX),
        rowHeight: LIST_ROW_HEIGHT,
        overscan: LIST_OVERSCAN,
      }),
    [filteredEntries.length, scrollTop, viewportSize.height],
  );

  const gridRange = useMemo(
    () =>
      computeVirtualGridRange({
        totalCount: filteredEntries.length,
        scrollTop,
        viewportHeight: Math.max(0, viewportSize.height - VIEWPORT_PADDING_PX),
        containerWidth: Math.max(GRID_CELL_MIN_WIDTH, viewportSize.width - VIEWPORT_PADDING_PX),
        cellMinWidth: GRID_CELL_MIN_WIDTH,
        cellGap: GRID_CELL_GAP,
        rowHeight: GRID_ROW_HEIGHT,
        overscan: GRID_OVERSCAN,
      }),
    [filteredEntries.length, scrollTop, viewportSize.height, viewportSize.width],
  );

  const activeRange = viewMode === 'grid' ? gridRange : listRange;
  const visibleEntries = useMemo(
    () => filteredEntries.slice(activeRange.startIndex, activeRange.endIndex),
    [filteredEntries, activeRange.startIndex, activeRange.endIndex],
  );

  // Lazy debounced (~40ms) batched thumbnail & native shell icon loader
  useEffect(() => {
    const dirPath = listing?.currentPath;
    if (!dirPath || visibleEntries.length === 0) return;

    const missing: FileEntry[] = [];
    const seenKeys = new Set<string>();
    const cache = thumbLruRef.current;

    for (const entry of visibleEntries) {
      const key = makeThumbCacheKey(entry, thumbSize);
      if (seenKeys.has(key)) continue;
      seenKeys.add(key);

      if (cache.get(key) !== undefined) {
        continue;
      }
      if (failedThumbKeysRef.current.has(key) || inFlightThumbKeysRef.current.has(key)) {
        continue;
      }
      missing.push(entry);
    }

    if (missing.length === 0) return;

    const timerId = window.setTimeout(() => {
      const reqPath = dirPath;
      const reqSeq = navSeqRef.current;
      const reqSize = thumbSize;
      const chunks = chunkItems(missing, THUMB_BATCH_LIMIT);

      void (async () => {
        for (const chunk of chunks) {
          if (
            !mountedRef.current ||
            navSeqRef.current !== reqSeq ||
            activePathRef.current !== reqPath
          ) {
            return;
          }

          const chunkKeys = chunk.map((e) => makeThumbCacheKey(e, reqSize));
          chunkKeys.forEach((k) => inFlightThumbKeysRef.current.add(k));

          let batch: Record<string, string> = {};
          try {
            batch = (await getFileThumbnails(
              chunk.map((e) => e.path),
              reqSize,
            )) ?? {};
          } catch {
            batch = {};
          } finally {
            chunkKeys.forEach((k) => inFlightThumbKeysRef.current.delete(k));
          }

          // Ignore stale async responses if user navigated away or unmounted
          if (
            !mountedRef.current ||
            navSeqRef.current !== reqSeq ||
            activePathRef.current !== reqPath
          ) {
            return;
          }

          let updatedCount = 0;
          for (const entry of chunk) {
            const key = makeThumbCacheKey(entry, reqSize);
            const uri = batch[entry.path];
            if (typeof uri === 'string' && uri.length > 0) {
              thumbLruRef.current.set(key, uri);
              updatedCount++;
            } else {
              if (failedThumbKeysRef.current.size >= FAILED_THUMB_MAX) {
                failedThumbKeysRef.current.clear();
              }
              failedThumbKeysRef.current.add(key);
            }
          }

          if (updatedCount > 0 && thumbSizeRef.current === reqSize) {
            setThumbMap(Object.fromEntries(thumbLruRef.current.entries()));
          }
        }
      })();
    }, THUMB_DEBOUNCE_MS);

    return () => window.clearTimeout(timerId);
  }, [listing?.currentPath, visibleEntries, thumbSize]);

  const ensureIndexVisible = useCallback(
    (idx: number) => {
      const el = viewportRef.current;
      if (!el || idx < 0) return;
      const itemTop =
        viewMode === 'grid'
          ? Math.floor(idx / Math.max(1, gridRange.columns)) * GRID_ROW_HEIGHT
          : idx * LIST_ROW_HEIGHT;
      const itemHeight = viewMode === 'grid' ? GRID_ROW_HEIGHT : LIST_ROW_HEIGHT;
      const itemBottom = itemTop + itemHeight;
      const viewHeight = Math.max(0, el.clientHeight - VIEWPORT_PADDING_PX);

      let nextScroll = el.scrollTop;
      if (itemTop < nextScroll) {
        nextScroll = itemTop;
      } else if (itemBottom > nextScroll + viewHeight) {
        nextScroll = Math.max(0, itemBottom - viewHeight);
      }

      if (nextScroll !== el.scrollTop) {
        el.scrollTop = nextScroll;
        setScrollTop(nextScroll);
      }
    },
    [viewMode, gridRange.columns],
  );

  // Open directory or launch file
  const openEntry = useCallback(
    (entry: FileEntry) => {
      setCtxMenu(null);
      if (entry.isDir) {
        void navigatePush(entry.path);
      } else {
        fsOpenItem(entry.path).catch((err) => onShowToast(formatIpcError(err)));
      }
    },
    [navigatePush, onShowToast],
  );

  // Start inline F2 rename
  const startRenameEntry = useCallback((entry: FileEntry) => {
    setCtxMenu(null);
    setNewFolderOpen(false);
    setPathEditing(false);
    setSelectedPath(entry.path);
    setRenamingPath(entry.path);
    setRenameDraft(entry.name);
    setRenamingBusy(false);

    requestAnimationFrame(() => {
      const input = renameInputRef.current;
      if (!input) return;
      input.focus();
      const dotIdx = !entry.isDir ? entry.name.lastIndexOf('.') : -1;
      if (dotIdx > 0) {
        try {
          input.setSelectionRange(0, dotIdx);
        } catch {
          input.select();
        }
      } else {
        input.select();
      }
    });
  }, []);

  const commitRename = useCallback(
    async (entry: FileEntry) => {
      if (renamingBusy || renameCommitLockRef.current) return;
      const trimmed = renameDraft.trim();
      if (!trimmed) {
        onShowToast('Имя не может быть пустым');
        renameInputRef.current?.focus();
        return;
      }
      if (trimmed === entry.name) {
        setRenamingPath(null);
        focusRoot();
        return;
      }

      renameCommitLockRef.current = true;
      setRenamingBusy(true);
      try {
        const renamed = await fsRenameItem(entry.path, trimmed);
        if (!mountedRef.current) return;
        setRenamingPath(null);
        setRenamingBusy(false);
        renameCommitLockRef.current = false;
        await loadDirectory(activePathRef.current, {
          mode: 'refresh',
          sortBy,
          sortDir,
          showHidden,
          selectAfterLoad: renamed.path,
        });
        onShowToast(`Переименовано: ${renamed.name}`);
        focusRoot();
      } catch (err) {
        if (!mountedRef.current) return;
        setRenamingBusy(false);
        renameCommitLockRef.current = false;
        onShowToast(formatIpcError(err));
        requestAnimationFrame(() => renameInputRef.current?.focus());
      }
    },
    [renamingBusy, renameDraft, onShowToast, loadDirectory, sortBy, sortDir, showHidden, focusRoot],
  );

  // Create new folder
  const openNewFolderBar = useCallback(() => {
    setCtxMenu(null);
    setRenamingPath(null);
    setPathEditing(false);
    setNewFolderError(null);
    setNewFolderName('');
    setNewFolderOpen(true);
    requestAnimationFrame(() => newFolderInputRef.current?.focus());
  }, []);

  const submitNewFolder = useCallback(async () => {
    if (creatingFolder) return;
    const trimmed = newFolderName.trim();
    if (!trimmed) {
      setNewFolderError('Введите имя папки');
      newFolderInputRef.current?.focus();
      return;
    }

    setCreatingFolder(true);
    setNewFolderError(null);
    try {
      const created = await fsCreateFolder(activePathRef.current, trimmed);
      if (!mountedRef.current) return;
      setNewFolderOpen(false);
      setNewFolderName('');
      setCreatingFolder(false);
      await loadDirectory(activePathRef.current, {
        mode: 'refresh',
        sortBy,
        sortDir,
        showHidden,
        selectAfterLoad: created.path,
      });
      onShowToast(`Создана папка: ${created.name}`);
      focusRoot();
    } catch (err) {
      if (!mountedRef.current) return;
      setCreatingFolder(false);
      setNewFolderError(formatIpcError(err));
      requestAnimationFrame(() => newFolderInputRef.current?.focus());
    }
  }, [creatingFolder, newFolderName, loadDirectory, sortBy, sortDir, showHidden, onShowToast, focusRoot]);

  // Copy file / Copy path / Trash
  const copyEntryFile = useCallback(
    (entry: FileEntry) => {
      fsCopyFilesToClipboard([entry.path])
        .then(() => onShowToast(`Скопировано: ${entry.name}`))
        .catch((err) => onShowToast(formatIpcError(err)));
    },
    [onShowToast],
  );

  const copyEntryPath = useCallback(
    (entry: FileEntry) => {
      copyTextToClipboard(entry.path)
        .then(() => onShowToast('Путь скопирован'))
        .catch((err) => onShowToast(formatIpcError(err)));
    },
    [onShowToast],
  );

  const trashEntry = useCallback(
    async (entry: FileEntry) => {
      const idx = filteredEntries.findIndex((e) => e.path === entry.path);
      const nextCandidate =
        filteredEntries[idx + 1]?.path ?? filteredEntries[idx - 1]?.path ?? null;

      try {
        await fsTrashItems([entry.path]);
        if (!mountedRef.current) return;
        await loadDirectory(activePathRef.current, {
          mode: 'refresh',
          sortBy,
          sortDir,
          showHidden,
          selectAfterLoad: nextCandidate,
        });
        onShowToast(`Удалено в Корзину: ${entry.name}`);
      } catch (err) {
        if (!mountedRef.current) return;
        onShowToast(formatIpcError(err));
      }
    },
    [filteredEntries, loadDirectory, sortBy, sortDir, showHidden, onShowToast],
  );

  // Quick path edit bar
  const startPathEdit = useCallback(() => {
    setCtxMenu(null);
    setPathDraft(currentPath);
    setPathEditing(true);
    requestAnimationFrame(() => {
      const input = pathInputRef.current;
      if (!input) return;
      input.focus();
      input.select();
    });
  }, [currentPath]);

  const submitPathJump = useCallback(async () => {
    const trimmed = pathDraft.trim();
    if (!trimmed) {
      setPathEditing(false);
      focusRoot();
      return;
    }
    pathSubmittingRef.current = true;
    const ok = await navigatePush(trimmed);
    pathSubmittingRef.current = false;
    if (ok) {
      setPathEditing(false);
      focusRoot();
    } else {
      requestAnimationFrame(() => {
        pathInputRef.current?.focus();
        pathInputRef.current?.select();
      });
    }
  }, [pathDraft, navigatePush, focusRoot]);

  // Toolbar actions
  const handleOpenTerminalHere = useCallback(() => {
    if (!currentPath) return;
    ipc('open_terminal_at', { path: currentPath }).catch((err) =>
      onShowToast(formatIpcError(err)),
    );
  }, [currentPath, onShowToast]);

  const handleOpenInExplorerHere = useCallback(() => {
    if (!currentPath) return;
    fsOpenInExplorer(currentPath).catch((err) => onShowToast(formatIpcError(err)));
  }, [currentPath, onShowToast]);

  const handlePinCurrentFolder = useCallback(async () => {
    if (!currentPath || pinningFolder) return;
    const folderName = deriveFolderDisplayName(currentPath, listingRef.current);
    setPinningFolder(true);
    try {
      await Promise.resolve(onPinFolder(currentPath, folderName));
    } catch (err) {
      if (mountedRef.current) {
        onShowToast(formatIpcError(err));
      }
    } finally {
      if (mountedRef.current) {
        setPinningFolder(false);
      }
    }
  }, [currentPath, pinningFolder, onPinFolder, onShowToast]);

  // Context menu positioning & actions
  const openContextMenu = useCallback((e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    e.stopPropagation();
    setSelectedPath(entry.path);
    setRenamingPath(null);
    setCtxIdx(0);

    const rect = rootRef.current?.getBoundingClientRect();
    const menuW = 248;
    const estH = entry.isDir ? 264 : 228;
    const relX = rect ? e.clientX - rect.left : e.clientX;
    const relY = rect ? e.clientY - rect.top : e.clientY;
    const maxW = rect ? rect.width : window.innerWidth;
    const maxH = rect ? rect.height : window.innerHeight;

    setCtxMenu({
      x: Math.max(8, Math.min(relX, maxW - menuW - 8)),
      y: Math.max(8, Math.min(relY, maxH - estH - 8)),
      entry,
    });
  }, []);

  // Re-clamp context menu precisely after layout measurement
  useLayoutEffect(() => {
    if (!ctxMenu || !ctxMenuRef.current || !rootRef.current) return;
    const rootRect = rootRef.current.getBoundingClientRect();
    const menuW = ctxMenuRef.current.offsetWidth || 248;
    const menuH = ctxMenuRef.current.offsetHeight || 240;
    const clampedX = Math.max(8, Math.min(ctxMenu.x, rootRect.width - menuW - 8));
    const clampedY = Math.max(8, Math.min(ctxMenu.y, rootRect.height - menuH - 8));
    if (clampedX !== ctxMenu.x || clampedY !== ctxMenu.y) {
      setCtxMenu((prev) => (prev ? { ...prev, x: clampedX, y: clampedY } : prev));
    }
  }, [ctxMenu]);

  // Close context menu on outside mousedown
  useEffect(() => {
    if (!ctxMenu) return;
    const onOutsideDown = (e: MouseEvent) => {
      if (ctxMenuRef.current && !ctxMenuRef.current.contains(e.target as Node)) {
        setCtxMenu(null);
      }
    };
    window.addEventListener('mousedown', onOutsideDown);
    return () => window.removeEventListener('mousedown', onOutsideDown);
  }, [ctxMenu]);

  const ctxActions = useMemo<ExplorerCtxAction[]>(() => {
    if (!ctxMenu) return [];
    const target = ctxMenu.entry;
    const items: ExplorerCtxAction[] = [
      {
        id: 'open',
        label: 'Открыть',
        icon: <ExternalWindowIcon className="ic14" />,
        run: () => openEntry(target),
      },
      {
        id: 'rename',
        label: 'Переименовать',
        icon: <RenameIcon className="ic14" />,
        run: () => startRenameEntry(target),
      },
      {
        id: 'copy-file',
        label: 'Копировать файл',
        icon: <CopyIcon className="ic14" />,
        run: () => {
          setCtxMenu(null);
          copyEntryFile(target);
        },
      },
      {
        id: 'copy-path',
        label: 'Скопировать путь',
        icon: <CopyIcon className="ic14" />,
        run: () => {
          setCtxMenu(null);
          copyEntryPath(target);
        },
      },
    ];

    if (target.isDir) {
      items.push({
        id: 'terminal',
        label: 'Открыть в терминале',
        icon: <TerminalIcon className="ic14" />,
        run: () => {
          setCtxMenu(null);
          ipc('open_terminal_at', { path: target.path }).catch((err) =>
            onShowToast(formatIpcError(err)),
          );
        },
      });
    }

    items.push({
      id: 'reveal',
      label: 'Показать в Проводнике Windows',
      icon: <FolderSvg className="ic14" />,
      run: () => {
        setCtxMenu(null);
        fsOpenInExplorer(target.path).catch((err) => onShowToast(formatIpcError(err)));
      },
    });

    items.push({
      id: 'trash',
      label: 'Удалить в Корзину',
      danger: true,
      icon: <TrashIcon className="ic14" />,
      run: () => {
        setCtxMenu(null);
        void trashEntry(target);
      },
    });

    return items;
  }, [ctxMenu, openEntry, startRenameEntry, copyEntryFile, copyEntryPath, trashEntry, onShowToast]);

  // Unified Escape handler obeying strict priority chain
  const handleEscapePriority = useCallback(() => {
    if (ctxMenu) {
      setCtxMenu(null);
      focusRoot();
      return;
    }
    if (renamingPath !== null) {
      setRenamingPath(null);
      focusRoot();
      return;
    }
    if (newFolderOpen) {
      setNewFolderOpen(false);
      setNewFolderError(null);
      focusRoot();
      return;
    }
    if (pathEditing) {
      setPathEditing(false);
      focusRoot();
      return;
    }
    if (filterQuery.length > 0) {
      setFilterQuery('');
      return;
    }
    onClose();
  }, [ctxMenu, renamingPath, newFolderOpen, pathEditing, filterQuery, onClose, focusRoot]);

  // Safety guard so Escape never leaks to StartView if focus fell back to document.body
  useEffect(() => {
    const onWindowKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (rootRef.current && rootRef.current.contains(e.target as Node)) return;
      if (
        document.activeElement === document.body &&
        !document.querySelector(
          '.settings-backdrop, .search-backdrop, .all-apps-drawer, .space-create-pop, .space-manage-pop, .periph-pop, .weather-hdr-pop, .tile-edit-backdrop, .empty-ctx-menu',
        )
      ) {
        e.preventDefault();
        e.stopPropagation();
        handleEscapePriority();
      }
    };
    window.addEventListener('keydown', onWindowKeyDown, true);
    return () => window.removeEventListener('keydown', onWindowKeyDown, true);
  }, [handleEscapePriority]);

  const handleContainerKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      e.nativeEvent.stopPropagation();
      handleEscapePriority();
      return;
    }

    // Alt+ArrowLeft / Alt+ArrowRight history navigation
    if (e.altKey && !e.ctrlKey && !e.metaKey) {
      if (e.key === 'ArrowLeft') {
        e.preventDefault();
        e.stopPropagation();
        handleGoBack();
        return;
      }
      if (e.key === 'ArrowRight') {
        e.preventDefault();
        e.stopPropagation();
        handleGoForward();
        return;
      }
    }

    // Context menu keyboard navigation
    if (ctxMenu && ctxActions.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        e.stopPropagation();
        setCtxIdx((i) => (i + 1) % ctxActions.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        e.stopPropagation();
        setCtxIdx((i) => (i - 1 + ctxActions.length) % ctxActions.length);
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        e.stopPropagation();
        ctxActions[ctxIdx]?.run();
        return;
      }
    }

    const inInput = isEditableTarget(e.target);

    // Quick keyboard shortcuts for filter/path focus
    if ((e.ctrlKey || e.metaKey) && !e.altKey && (e.code === 'KeyF' || e.key.toLowerCase() === 'f')) {
      e.preventDefault();
      e.stopPropagation();
      filterInputRef.current?.focus();
      filterInputRef.current?.select();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && !e.altKey && (e.code === 'KeyL' || e.key.toLowerCase() === 'l')) {
      e.preventDefault();
      e.stopPropagation();
      startPathEdit();
      return;
    }

    if (inInput) return;

    if (e.key === 'Backspace') {
      e.preventDefault();
      e.stopPropagation();
      handleGoUp();
      return;
    }

    if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && (e.code === 'KeyR' || e.key.toLowerCase() === 'r'))) {
      e.preventDefault();
      e.stopPropagation();
      handleRefresh();
      return;
    }

    if (e.key === 'F2' && selectedEntry) {
      e.preventDefault();
      e.stopPropagation();
      startRenameEntry(selectedEntry);
      return;
    }

    if (e.key === 'Delete' && selectedEntry) {
      e.preventDefault();
      e.stopPropagation();
      void trashEntry(selectedEntry);
      return;
    }

    if (
      (e.ctrlKey || e.metaKey) &&
      !e.altKey &&
      !e.shiftKey &&
      (e.code === 'KeyC' || e.key.toLowerCase() === 'c' || e.key.toLowerCase() === 'с') &&
      selectedEntry
    ) {
      e.preventDefault();
      e.stopPropagation();
      copyEntryFile(selectedEntry);
      return;
    }

    if (e.key === 'Enter' && selectedEntry) {
      e.preventDefault();
      e.stopPropagation();
      openEntry(selectedEntry);
      return;
    }

    // List/Grid Arrow navigation
    if (filteredEntries.length > 0) {
      const curIdx = selectedEntry
        ? filteredEntries.findIndex((item) => item.path === selectedEntry.path)
        : -1;
      const cols = viewMode === 'grid' ? Math.max(1, gridRange.columns) : 1;
      let nextIdx: number | null = null;

      if (e.key === 'ArrowDown') {
        nextIdx = curIdx < 0 ? 0 : Math.min(filteredEntries.length - 1, curIdx + cols);
      } else if (e.key === 'ArrowUp') {
        nextIdx = curIdx < 0 ? 0 : Math.max(0, curIdx - cols);
      } else if (e.key === 'ArrowRight' && viewMode === 'grid') {
        nextIdx = curIdx < 0 ? 0 : Math.min(filteredEntries.length - 1, curIdx + 1);
      } else if (e.key === 'ArrowLeft' && viewMode === 'grid') {
        nextIdx = curIdx < 0 ? 0 : Math.max(0, curIdx - 1);
      } else if (e.key === 'Home') {
        nextIdx = 0;
      } else if (e.key === 'End') {
        nextIdx = filteredEntries.length - 1;
      }

      if (nextIdx !== null && filteredEntries[nextIdx]) {
        e.preventDefault();
        e.stopPropagation();
        setSelectedPath(filteredEntries[nextIdx].path);
        ensureIndexVisible(nextIdx);
      }
    }
  };

  const handleSortHeaderClick = (field: ExplorerSortBy) => {
    if (sortBy === field) {
      setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'));
    } else {
      setSortBy(field);
      setSortDir('asc');
    }
  };

  const currentKey = normalizePathKey(currentPath);
  const backEnabled = canGoBack(history) && !loading;
  const forwardEnabled = canGoForward(history) && !loading;
  const upEnabled = Boolean(listing?.parentPath) && !loading;
  const totalCount = listing?.totalCount ?? allEntries.length;
  const isFiltered = filterQuery.trim().length > 0;

  return (
    <div
      ref={rootRef}
      className="fx-root"
      role="dialog"
      aria-label={`Проводник: ${currentFolderNameLabel(currentPath, listing)}`}
      data-space-id={activeSpaceId}
      tabIndex={-1}
      onKeyDown={handleContainerKeyDown}
      onClick={() => {
        if (ctxMenu) setCtxMenu(null);
      }}
    >
      <div className="fx-shell">
        {/* TOP TOOLBAR + FILTER BAR */}
        <header className="fx-topbar">
          <div className="fx-toolbar-row">
            <button
              type="button"
              className="fx-back-pill"
              onClick={onClose}
            >
              <ArrowLeftIcon className="ic14" />
              <span>Пространство</span>
            </button>

            <div className="fx-nav-group" role="group" aria-label="Навигация">
              <button
                type="button"
                className="fx-ic-btn"
                title="Назад (Alt+Стрелка влево)"
                aria-label="Назад"
                disabled={!backEnabled}
                onClick={handleGoBack}
              >
                <ArrowLeftIcon className="ic14" />
              </button>
              <button
                type="button"
                className="fx-ic-btn"
                title="Вперёд (Alt+Стрелка вправо)"
                aria-label="Вперёд"
                disabled={!forwardEnabled}
                onClick={handleGoForward}
              >
                <ArrowRightIcon className="ic14" />
              </button>
              <button
                type="button"
                className="fx-ic-btn"
                title="Вверх (Backspace)"
                aria-label="Вверх"
                disabled={!upEnabled}
                onClick={handleGoUp}
              >
                <ArrowUpIcon size={14} />
              </button>
            </div>

            <div
              className={`fx-path-wrap ${pathEditing ? 'editing' : ''}`}
              onDoubleClick={() => {
                if (!pathEditing) startPathEdit();
              }}
            >
              {pathEditing ? (
                <input
                  ref={pathInputRef}
                  className="fx-path-input"
                  aria-label="Путь к папке"
                  value={pathDraft}
                  onChange={(e) => setPathDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      e.stopPropagation();
                      void submitPathJump();
                    } else if (e.key === 'Escape') {
                      e.preventDefault();
                      e.stopPropagation();
                      setPathEditing(false);
                      focusRoot();
                    }
                  }}
                  onBlur={() => {
                    if (!pathSubmittingRef.current) {
                      setPathEditing(false);
                    }
                  }}
                />
              ) : (
                <nav
                  ref={breadcrumbsRef}
                  className="fx-breadcrumbs"
                  aria-label="Путь"
                >
                  {(listing?.breadcrumbs ?? []).map((part, idx, arr) => {
                    const isLast = idx === arr.length - 1;
                    return (
                      <React.Fragment key={part.path}>
                        {idx > 0 && (
                          <span className="fx-crumb-sep" aria-hidden="true">
                            <ChevronRightIcon className="ic12" />
                          </span>
                        )}
                        <button
                          type="button"
                          className={`fx-crumb-btn ${isLast ? 'active' : ''}`}
                          title={part.path}
                          onDoubleClick={(e) => {
                            if (!isLast) e.stopPropagation();
                          }}
                          onClick={() => {
                            if (!isLast) void navigatePush(part.path);
                          }}
                        >
                          {part.name}
                        </button>
                      </React.Fragment>
                    );
                  })}
                </nav>
              )}

              <button
                type="button"
                className={`fx-ic-btn fx-path-edit-btn ${pathEditing ? 'active' : ''}`}
                title="Ввести путь"
                aria-label="Ввести путь"
                onMouseDown={(e) => {
                  if (pathEditing) e.preventDefault();
                }}
                onClick={() => {
                  if (pathEditing) {
                    setPathEditing(false);
                    focusRoot();
                  } else {
                    startPathEdit();
                  }
                }}
              >
                <EditPencilIcon className="ic12" />
              </button>
            </div>

            <div className="fx-action-group">
              <button
                type="button"
                className={`fx-pill-btn ${newFolderOpen ? 'active' : ''}`}
                disabled={!listing?.currentPath}
                onClick={() => {
                  if (newFolderOpen) {
                    setNewFolderOpen(false);
                    setNewFolderError(null);
                  } else {
                    openNewFolderBar();
                  }
                }}
              >
                <FolderPlusIcon className="ic14" />
                <span>+ Папка</span>
              </button>

              <button
                type="button"
                className="fx-pill-btn"
                disabled={!currentPath}
                onClick={handleOpenTerminalHere}
              >
                <TerminalIcon className="ic14" />
                <span>Терминал</span>
              </button>

              <button
                type="button"
                className="fx-pill-btn"
                disabled={!currentPath}
                onClick={handleOpenInExplorerHere}
              >
                <ExternalWindowIcon className="ic14" />
                <span>Проводник</span>
              </button>

              <button
                type="button"
                className="fx-pill-btn pin-btn"
                disabled={!currentPath || pinningFolder}
                onClick={() => void handlePinCurrentFolder()}
              >
                <PinIcon className="ic14" />
                <span>Закрепить</span>
              </button>

              <button
                type="button"
                className="fx-pill-btn icon-only"
                title="Обновить (F5)"
                aria-label="Обновить"
                disabled={loading}
                onClick={handleRefresh}
              >
                <RefreshIcon className="ic14" />
              </button>
            </div>
          </div>

          <div className="fx-filter-row">
            <div className="fx-search-wrap">
              <SearchIcon className="ic14" color="var(--t3)" />
              <input
                ref={filterInputRef}
                className="fx-search-input"
                aria-label="Фильтр файлов"
                placeholder="Фильтр по папке…"
                value={filterQuery}
                onChange={(e) => {
                  setFilterQuery(e.target.value);
                  setScrollTop(0);
                  if (viewportRef.current) viewportRef.current.scrollTop = 0;
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Escape') {
                    e.preventDefault();
                    e.stopPropagation();
                    e.nativeEvent.stopPropagation();
                    handleEscapePriority();
                  }
                }}
              />
              {filterQuery.length > 0 && (
                <button
                  type="button"
                  className="fx-search-clear"
                  aria-label="Очистить фильтр"
                  onClick={() => {
                    setFilterQuery('');
                    filterInputRef.current?.focus();
                  }}
                >
                  <CloseWindowIcon className="ic12" />
                </button>
              )}
            </div>

            <div className="fx-view-controls">
              <div className="fx-segmented" role="group" aria-label="Режим просмотра">
                <button
                  type="button"
                  className={`fx-seg-btn ${viewMode === 'list' ? 'active' : ''}`}
                  aria-pressed={viewMode === 'list'}
                  onClick={() => {
                    if (viewMode !== 'list') {
                      setViewMode('list');
                      setScrollTop(0);
                      if (viewportRef.current) viewportRef.current.scrollTop = 0;
                    }
                  }}
                >
                  <ListViewIcon className="ic14" />
                  <span>Список</span>
                </button>
                <button
                  type="button"
                  className={`fx-seg-btn ${viewMode === 'grid' ? 'active' : ''}`}
                  aria-pressed={viewMode === 'grid'}
                  onClick={() => {
                    if (viewMode !== 'grid') {
                      setViewMode('grid');
                      setScrollTop(0);
                      if (viewportRef.current) viewportRef.current.scrollTop = 0;
                    }
                  }}
                >
                  <GridViewIcon className="ic14" />
                  <span>Сетка</span>
                </button>
              </div>

              <div className="fx-sort-box">
                <select
                  className="fx-sort-select"
                  aria-label="Сортировка"
                  value={sortBy}
                  onChange={(e) => setSortBy(e.target.value as ExplorerSortBy)}
                >
                  {SORT_OPTIONS.map((opt) => (
                    <option key={opt.value} value={opt.value}>
                      {opt.label}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className="fx-ic-btn fx-sort-dir-btn"
                  data-dir={sortDir}
                  title={sortDir === 'asc' ? 'По возрастанию' : 'По убыванию'}
                  aria-label={sortDir === 'asc' ? 'По возрастанию' : 'По убыванию'}
                  onClick={() => setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'))}
                >
                  <SortIcon className="ic14" />
                </button>
              </div>

              <button
                type="button"
                className={`fx-pill-btn fx-hidden-btn ${showHidden ? 'active' : ''}`}
                aria-pressed={showHidden}
                title={showHidden ? 'Скрывать скрытые файлы' : 'Показать скрытые файлы'}
                onClick={() => setShowHidden((v) => !v)}
              >
                {showHidden ? <EyeIcon className="ic14" /> : <EyeOffIcon className="ic14" />}
                <span>Скрытые</span>
              </button>
            </div>
          </div>
        </header>

        {/* INLINE + НОВАЯ ПАПКА BAR */}
        {newFolderOpen && (
          <div className="fx-new-folder-bar">
            <FolderPlusIcon className="ic16" />
            <input
              ref={newFolderInputRef}
              className="fx-new-folder-input"
              placeholder="Имя новой папки…"
              maxLength={255}
              value={newFolderName}
              onChange={(e) => {
                setNewFolderName(e.target.value);
                if (newFolderError) setNewFolderError(null);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  e.stopPropagation();
                  void submitNewFolder();
                } else if (e.key === 'Escape') {
                  e.preventDefault();
                  e.stopPropagation();
                  e.nativeEvent.stopPropagation();
                  setNewFolderOpen(false);
                  setNewFolderError(null);
                  focusRoot();
                }
              }}
            />
            {newFolderError && <span className="fx-inline-err">{newFolderError}</span>}
            <button
              type="button"
              className="fx-inline-btn primary"
              disabled={creatingFolder}
              onClick={() => void submitNewFolder()}
            >
              Создать
            </button>
            <button
              type="button"
              className="fx-inline-btn"
              onClick={() => {
                setNewFolderOpen(false);
                setNewFolderError(null);
                focusRoot();
              }}
            >
              Отмена
            </button>
          </div>
        )}

        {/* BODY: LEFT SIDEBAR + VIRTUALIZED MAIN VIEWPORT */}
        <div className="fx-body">
          <aside className="fx-sidebar" aria-label="Разделы и диски">
            {listing && listing.quickRoots.length > 0 && (
              <div className="fx-side-sec">
                <div className="fx-side-title">Быстрый доступ</div>
                {listing.quickRoots.map((root) => {
                  const isActive = normalizePathKey(root.path) === currentKey;
                  return (
                    <button
                      key={root.id}
                      type="button"
                      className={`fx-side-btn ${isActive ? 'active' : ''}`}
                      title={root.path}
                      onClick={() => void navigatePush(root.path)}
                    >
                      <span className="fx-side-ic">{renderQuickRootIcon(root.kind)}</span>
                      <span className="fx-side-lbl">{root.label}</span>
                    </button>
                  );
                })}
              </div>
            )}

            {listing && listing.drives.length > 0 && (
              <div className="fx-side-sec">
                <div className="fx-side-title">Диски</div>
                {listing.drives.map((drive) => {
                  const isActive = normalizePathKey(drive.path) === currentKey;
                  const { ratio, levelClass } = getDriveUsageLevel(drive);
                  return (
                    <button
                      key={drive.path}
                      type="button"
                      className={`fx-side-btn fx-drive-btn ${isActive ? 'active' : ''}`}
                      title={`${drive.letter} (${drive.label})`}
                      onClick={() => void navigatePush(drive.path)}
                    >
                      <div className="fx-drive-top">
                        <span className="fx-side-ic">
                          <DriveIcon className="ic14" />
                        </span>
                        <span className="fx-drive-letter">{drive.letter}</span>
                        <span className="fx-drive-name">{drive.label}</span>
                      </div>
                      {drive.totalBytes > 0 && (
                        <>
                          <div className="fx-drive-bar">
                            <div
                              className={`fx-drive-fill ${levelClass}`.trim()}
                              style={{ ['--pct' as string]: ratio } as React.CSSProperties}
                            />
                          </div>
                          <div className="fx-drive-meta">
                            {formatFileSize(drive.freeBytes)} / {formatFileSize(drive.totalBytes)}
                          </div>
                        </>
                      )}
                    </button>
                  );
                })}
              </div>
            )}
          </aside>

          <main className="fx-main">
            {viewMode === 'list' && !loadError && (
              <div className="fx-list-head" role="row">
                <button
                  type="button"
                  className={`fx-col-btn ${sortBy === 'name' ? 'active' : ''}`}
                  onClick={() => handleSortHeaderClick('name')}
                >
                  <span>Имя</span>
                  {sortBy === 'name' && (
                    <span className="fx-col-dir">{sortDir === 'asc' ? '↑' : '↓'}</span>
                  )}
                </button>
                <button
                  type="button"
                  className={`fx-col-btn ${sortBy === 'date' ? 'active' : ''}`}
                  onClick={() => handleSortHeaderClick('date')}
                >
                  <span>Изменён</span>
                  {sortBy === 'date' && (
                    <span className="fx-col-dir">{sortDir === 'asc' ? '↑' : '↓'}</span>
                  )}
                </button>
                <button
                  type="button"
                  className={`fx-col-btn right ${sortBy === 'size' ? 'active' : ''}`}
                  onClick={() => handleSortHeaderClick('size')}
                >
                  <span>Размер</span>
                  {sortBy === 'size' && (
                    <span className="fx-col-dir">{sortDir === 'asc' ? '↑' : '↓'}</span>
                  )}
                </button>
                <button
                  type="button"
                  className={`fx-col-btn ${sortBy === 'type' ? 'active' : ''}`}
                  onClick={() => handleSortHeaderClick('type')}
                >
                  <span>Тип</span>
                  {sortBy === 'type' && (
                    <span className="fx-col-dir">{sortDir === 'asc' ? '↑' : '↓'}</span>
                  )}
                </button>
              </div>
            )}

            <div
              ref={viewportRef}
              className="fx-viewport"
              onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
              onClick={(e) => {
                if (e.target === e.currentTarget) {
                  setSelectedPath(null);
                  setCtxMenu(null);
                }
              }}
            >
              {loadError && !listing ? (
                <div className="fx-state-box">
                  <div className="fx-state-title">Не удалось открыть папку</div>
                  <div className="fx-state-sub">{loadError}</div>
                  <div className="fx-state-actions">
                    <button
                      type="button"
                      className="fx-inline-btn primary"
                      onClick={() =>
                        void loadDirectory('', {
                          mode: 'init',
                          sortBy,
                          sortDir,
                          showHidden,
                        })
                      }
                    >
                      Домашняя папка
                    </button>
                    <button
                      type="button"
                      className="fx-inline-btn"
                      onClick={() =>
                        void loadDirectory(initialPath, {
                          mode: 'init',
                          sortBy,
                          sortDir,
                          showHidden,
                        })
                      }
                    >
                      Повторить
                    </button>
                  </div>
                </div>
              ) : loading && !listing ? (
                <div className="fx-state-box">
                  <div className="fx-state-title">Загрузка…</div>
                </div>
              ) : filteredEntries.length === 0 ? (
                <div className="fx-state-box">
                  <div className="fx-state-title">
                    {isFiltered ? 'По фильтру ничего не найдено' : 'Папка пуста'}
                  </div>
                  {isFiltered && (
                    <button
                      type="button"
                      className="fx-inline-btn"
                      onClick={() => setFilterQuery('')}
                    >
                      Сбросить фильтр
                    </button>
                  )}
                </div>
              ) : viewMode === 'list' ? (
                <div className="fx-list-slice" role="listbox" aria-label="Список файлов">
                  {listRange.topSpacerPx > 0 && (
                    <div
                      className="fx-spacer"
                      style={{ height: listRange.topSpacerPx }}
                      aria-hidden="true"
                    />
                  )}
                  {visibleEntries.map((entry) => {
                    const isSelected = selectedPath === entry.path;
                    const isRenaming = renamingPath === entry.path;
                    const thumbKey = makeThumbCacheKey(entry, thumbSize);
                    const thumbSrc = thumbMap[thumbKey] ?? null;

                    return (
                      <div key={entry.path} className="fx-row-slot">
                        <div
                          role="option"
                          aria-selected={isSelected}
                          className={`fx-list-row ${entry.isDir ? 'is-dir' : ''} ${entry.isHidden ? 'is-hidden' : ''} ${isSelected ? 'selected' : ''}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            setCtxMenu(null);
                            setSelectedPath(entry.path);
                          }}
                          onDoubleClick={(e) => {
                            e.stopPropagation();
                            if (!isRenaming) openEntry(entry);
                          }}
                          onContextMenu={(e) => openContextMenu(e, entry)}
                        >
                          <div className="fx-row-name-cell">
                            <span className="fx-row-ic-box">
                              {thumbSrc ? (
                                <img
                                  src={thumbSrc}
                                  alt=""
                                  className="fx-item-thumb-sm"
                                  draggable={false}
                                  decoding="async"
                                />
                              ) : (
                                renderEntryFallbackIcon(entry, 'ic16')
                              )}
                            </span>

                            {isRenaming ? (
                              <input
                                ref={renameInputRef}
                                className="fx-rename-input"
                                aria-label="Новое имя"
                                value={renameDraft}
                                disabled={renamingBusy}
                                onClick={(e) => e.stopPropagation()}
                                onDoubleClick={(e) => e.stopPropagation()}
                                onChange={(e) => setRenameDraft(e.target.value)}
                                onKeyDown={(e) => {
                                  if (e.key === 'Enter') {
                                    e.preventDefault();
                                    e.stopPropagation();
                                    void commitRename(entry);
                                  } else if (e.key === 'Escape') {
                                    e.preventDefault();
                                    e.stopPropagation();
                                    e.nativeEvent.stopPropagation();
                                    setRenamingPath(null);
                                    focusRoot();
                                  }
                                }}
                                onBlur={() => {
                                  if (!renamingBusy && !renameCommitLockRef.current) {
                                    setRenamingPath(null);
                                  }
                                }}
                              />
                            ) : (
                              <span className="fx-row-name" title={entry.name}>
                                {entry.name}
                              </span>
                            )}
                          </div>

                          <span className="fx-row-date">
                            {formatFileDate(entry.modifiedMs)}
                          </span>
                          <span className="fx-row-size">
                            {formatFileSize(entry.sizeBytes, entry.isDir)}
                          </span>
                          <span className="fx-row-type" title={getEntryTypeLabel(entry)}>
                            {getEntryTypeLabel(entry)}
                          </span>
                        </div>
                      </div>
                    );
                  })}
                  {listRange.bottomSpacerPx > 0 && (
                    <div
                      className="fx-spacer"
                      style={{ height: listRange.bottomSpacerPx }}
                      aria-hidden="true"
                    />
                  )}
                </div>
              ) : (
                <div role="listbox" aria-label="Сетка файлов">
                  {gridRange.topSpacerPx > 0 && (
                    <div
                      className="fx-spacer"
                      style={{ height: gridRange.topSpacerPx }}
                      aria-hidden="true"
                    />
                  )}
                  <div
                    className="fx-grid-slice"
                    style={{
                      gridTemplateColumns: `repeat(${gridRange.columns}, minmax(0, 1fr))`,
                    }}
                  >
                    {visibleEntries.map((entry) => {
                      const isSelected = selectedPath === entry.path;
                      const isRenaming = renamingPath === entry.path;
                      const thumbKey = makeThumbCacheKey(entry, thumbSize);
                      const thumbSrc = thumbMap[thumbKey] ?? null;

                      return (
                        <div key={entry.path} className="fx-grid-slot">
                          <div
                            role="option"
                            aria-selected={isSelected}
                            className={`fx-grid-card ${entry.isDir ? 'is-dir' : ''} ${entry.isHidden ? 'is-hidden' : ''} ${isSelected ? 'selected' : ''}`}
                            title={entry.name}
                            onClick={(e) => {
                              e.stopPropagation();
                              setCtxMenu(null);
                              setSelectedPath(entry.path);
                            }}
                            onDoubleClick={(e) => {
                              e.stopPropagation();
                              if (!isRenaming) openEntry(entry);
                            }}
                            onContextMenu={(e) => openContextMenu(e, entry)}
                          >
                            <div className="fx-grid-thumb-box">
                              {thumbSrc ? (
                                <img
                                  src={thumbSrc}
                                  alt=""
                                  className="fx-grid-thumb-img"
                                  draggable={false}
                                  decoding="async"
                                />
                              ) : (
                                renderEntryFallbackIcon(entry, 'fx-grid-fallback-ic')
                              )}
                            </div>

                            {isRenaming ? (
                              <input
                                ref={renameInputRef}
                                className="fx-rename-input"
                                aria-label="Новое имя"
                                value={renameDraft}
                                disabled={renamingBusy}
                                onClick={(e) => e.stopPropagation()}
                                onDoubleClick={(e) => e.stopPropagation()}
                                onChange={(e) => setRenameDraft(e.target.value)}
                                onKeyDown={(e) => {
                                  if (e.key === 'Enter') {
                                    e.preventDefault();
                                    e.stopPropagation();
                                    void commitRename(entry);
                                  } else if (e.key === 'Escape') {
                                    e.preventDefault();
                                    e.stopPropagation();
                                    e.nativeEvent.stopPropagation();
                                    setRenamingPath(null);
                                    focusRoot();
                                  }
                                }}
                                onBlur={() => {
                                  if (!renamingBusy && !renameCommitLockRef.current) {
                                    setRenamingPath(null);
                                  }
                                }}
                              />
                            ) : (
                              <div className="fx-grid-label">{entry.name}</div>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                  {gridRange.bottomSpacerPx > 0 && (
                    <div
                      className="fx-spacer"
                      style={{ height: gridRange.bottomSpacerPx }}
                      aria-hidden="true"
                    />
                  )}
                </div>
              )}
            </div>
          </main>
        </div>

        {/* STATUS FOOTER */}
        <footer className="fx-footer">
          <div className="fx-footer-left">
            <span>Элементов: {totalCount}</span>
            {isFiltered && (
              <>
                <span className="fx-footer-sep">·</span>
                <span className="fx-footer-filtered">Найдено: {filteredEntries.length}</span>
              </>
            )}
          </div>

          <div className="fx-footer-right">
            {selectedEntry ? (
              <>
                <span className="fx-footer-sel-name" title={selectedEntry.name}>
                  {selectedEntry.name}
                </span>
                <span className="fx-footer-sel-meta">
                  {selectedEntry.isDir
                    ? 'Папка'
                    : formatFileSize(selectedEntry.sizeBytes, false)}
                  {selectedEntry.modifiedMs > 0
                    ? ` · ${formatFileDate(selectedEntry.modifiedMs)}`
                    : ''}
                </span>
              </>
            ) : (
              <span className="fx-footer-path" title={currentPath}>
                {currentPath}
              </span>
            )}
          </div>
        </footer>
      </div>

      {/* RIGHT-CLICK CONTEXT MENU (clamped inside Explorer bounds) */}
      {ctxMenu && (
        <div
          ref={ctxMenuRef}
          className="fx-ctx-menu"
          role="menu"
          aria-label={ctxMenu.entry.name}
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="ctx-sec">{ctxMenu.entry.name}</div>
          {ctxActions.map((act, idx) => (
            <React.Fragment key={act.id}>
              {act.danger && <div className="ctx-sep" />}
              <button
                type="button"
                role="menuitem"
                className={`ctx-item ${act.danger ? 'danger' : ''} ${idx === ctxIdx ? 'focused' : ''}`}
                onMouseEnter={() => setCtxIdx(idx)}
                onClick={act.run}
              >
                {act.icon}
                <span className="ctx-item-label">{act.label}</span>
              </button>
            </React.Fragment>
          ))}
        </div>
      )}
    </div>
  );
};

function currentFolderNameLabel(path: string, listing: DirectoryListing | null): string {
  return deriveFolderDisplayName(path, listing);
}
