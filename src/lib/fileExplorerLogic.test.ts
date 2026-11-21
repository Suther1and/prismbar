import { describe, expect, it } from 'vitest';
import {
  canGoBack,
  canGoForward,
  computeVirtualGridRange,
  computeVirtualListRange,
  createHistoryState,
  filterDirectoryEntries,
  formatFileDate,
  formatFileSize,
  getEntryTypeLabel,
  navigateHistoryBack,
  navigateHistoryForward,
  normalizeHistoryPath,
  pushHistoryPath,
  type FileEntry,
} from './fileExplorerLogic';

function makeEntry(overrides: Partial<FileEntry> & { name: string }): FileEntry {
  const isDir = overrides.isDir ?? false;
  return {
    name: overrides.name,
    path: overrides.path ?? `C:\\Test\\${overrides.name}`,
    isDir,
    isHidden: overrides.isHidden ?? false,
    sizeBytes: overrides.sizeBytes ?? (isDir ? 0 : 1024),
    modifiedMs: overrides.modifiedMs ?? 1_700_000_000_000,
    extension: overrides.extension ?? '',
    category: overrides.category ?? (isDir ? 'folder' : 'file'),
    hasPreview: overrides.hasPreview ?? false,
  };
}

describe('computeVirtualListRange', () => {
  it('returns zeroed range for empty lists', () => {
    expect(
      computeVirtualListRange({
        totalCount: 0,
        scrollTop: 200,
        viewportHeight: 400,
        rowHeight: 36,
        overscan: 4,
      }),
    ).toEqual({
      startIndex: 0,
      endIndex: 0,
      topSpacerPx: 0,
      bottomSpacerPx: 0,
      totalHeightPx: 0,
    });
  });

  it('computes exact visible window and spacers for 2000 items', () => {
    const totalCount = 2000;
    const rowHeight = 36;
    const range = computeVirtualListRange({
      totalCount,
      scrollTop: 36 * 500 + 14, // scrolled into row 500
      viewportHeight: 360, // 10 rows visible (touches row 510 too)
      rowHeight,
      overscan: 3,
    });

    expect(range.totalHeightPx).toBe(2000 * 36);
    expect(range.startIndex).toBe(497);
    expect(range.endIndex).toBe(514);
    expect(range.topSpacerPx).toBe(497 * 36);
    expect(range.bottomSpacerPx).toBe((2000 - 514) * 36);
    expect(
      range.topSpacerPx + (range.endIndex - range.startIndex) * rowHeight + range.bottomSpacerPx,
    ).toBe(range.totalHeightPx);
  });

  it('clamps stale scrollTop past the end after filtering down', () => {
    const range = computeVirtualListRange({
      totalCount: 20,
      scrollTop: 50_000,
      viewportHeight: 360, // 10 rows
      rowHeight: 36,
      overscan: 2,
    });

    expect(range.startIndex).toBe(8);
    expect(range.endIndex).toBe(20);
    expect(range.bottomSpacerPx).toBe(0);
    expect(range.topSpacerPx).toBe(8 * 36);
  });
});

describe('computeVirtualGridRange', () => {
  it('calculates responsive columns and aligned row slices across 1500 items', () => {
    const range = computeVirtualGridRange({
      totalCount: 1500,
      scrollTop: 120 * 40, // row 40
      viewportHeight: 480, // 4 rows
      containerWidth: 500, // fits 4 * 110 + 3 * 12 = 476 <= 500
      cellMinWidth: 110,
      cellGap: 12,
      rowHeight: 120,
      overscan: 1,
    });

    expect(range.columns).toBe(4);
    // 1500 / 4 = 375 rows
    expect(range.totalHeightPx).toBe(375 * 120);
    // visible rows 40..44 -> with overscan 1 -> rows 39..45
    expect(range.startIndex).toBe(39 * 4);
    expect(range.endIndex).toBe(45 * 4);
    expect(range.topSpacerPx).toBe(39 * 120);
    expect(range.bottomSpacerPx).toBe((375 - 45) * 120);
  });

  it('handles partial final grid row cleanly without exceeding totalCount', () => {
    const range = computeVirtualGridRange({
      totalCount: 503,
      scrollTop: 99_999,
      viewportHeight: 300,
      containerWidth: 400,
      cellMinWidth: 100,
      cellGap: 0,
      rowHeight: 100,
      overscan: 1,
    });

    expect(range.columns).toBe(4);
    // 503 / 4 = 126 rows (last row has 3 items)
    expect(range.totalHeightPx).toBe(126 * 100);
    expect(range.endIndex).toBe(503);
    expect(range.bottomSpacerPx).toBe(0);
    expect(range.startIndex % 4).toBe(0);
  });

  it('falls back to 1 column when container is narrower than cellMinWidth', () => {
    const range = computeVirtualGridRange({
      totalCount: 10,
      scrollTop: 0,
      viewportHeight: 200,
      containerWidth: 64,
      cellMinWidth: 120,
      cellGap: 12,
      rowHeight: 100,
      overscan: 0,
    });

    expect(range.columns).toBe(1);
    expect(range.startIndex).toBe(0);
    expect(range.endIndex).toBe(2);
  });
});

describe('history stack reducer', () => {
  it('supports push, back, forward, duplicate skip, and branch truncation', () => {
    let state = createHistoryState('C:\\Users\\Test');
    expect(canGoBack(state)).toBe(false);
    expect(canGoForward(state)).toBe(false);

    // Same-path push (case/slash variant) must be a no-op
    state = pushHistoryPath(state, 'c:/users/test/');
    expect(state.entries).toHaveLength(1);

    state = pushHistoryPath(state, 'C:\\Users\\Test\\Documents');
    state = pushHistoryPath(state, 'C:\\Users\\Test\\Downloads');
    expect(canGoBack(state)).toBe(true);
    expect(canGoForward(state)).toBe(false);

    const back1 = navigateHistoryBack(state);
    state = back1.state;
    expect(back1.path).toBe('C:\\Users\\Test\\Documents');
    expect(canGoBack(state)).toBe(true);
    expect(canGoForward(state)).toBe(true);

    const fwd1 = navigateHistoryForward(state);
    state = fwd1.state;
    expect(fwd1.path).toBe('C:\\Users\\Test\\Downloads');
    expect(canGoForward(state)).toBe(false);

    // Go back once then push a new branch -> truncates forward history
    state = navigateHistoryBack(state).state;
    state = pushHistoryPath(state, 'C:\\Windows');
    expect(canGoForward(state)).toBe(false);
    expect(state.entries).toEqual([
      'C:\\Users\\Test',
      'C:\\Users\\Test\\Documents',
      'C:\\Windows',
    ]);

    // Forward past end returns null cleanly
    const noopFwd = navigateHistoryForward(state);
    expect(noopFwd.path).toBeNull();
  });
});

describe('filterDirectoryEntries with RU<->EN layout mapping', () => {
  const sample: FileEntry[] = [
    makeEntry({ name: 'Projects', isDir: true, category: 'folder' }),
    makeEntry({ name: 'README.md', extension: 'md', category: 'document' }),
    makeEntry({ name: 'Отчет_2026_финал.pdf', extension: 'pdf', category: 'document' }),
    makeEntry({ name: 'screenshot.png', extension: 'png', category: 'image', hasPreview: true }),
  ];

  it('returns full list for empty/whitespace queries', () => {
    expect(filterDirectoryEntries(sample, '   ')).toHaveLength(4);
  });

  it('matches wrong-layout RU query against EN file names and extensions', () => {
    // "куфвьу" on RU keyboard == "readme"
    expect(filterDirectoryEntries(sample, 'куфвьу').map((e) => e.name)).toEqual(['README.md']);
    // "зтп" on RU keyboard == "png"
    expect(filterDirectoryEntries(sample, 'зтп').map((e) => e.name)).toEqual(['screenshot.png']);
  });

  it('matches wrong-layout EN query against Russian file names', () => {
    // "jnxtn" on EN keyboard == "отчет"
    expect(filterDirectoryEntries(sample, 'jnxtn').map((e) => e.name)).toEqual([
      'Отчет_2026_финал.pdf',
    ]);
  });

  it('matches multi-word queries and type labels', () => {
    expect(filterDirectoryEntries(sample, 'отчет 2026').map((e) => e.name)).toEqual([
      'Отчет_2026_финал.pdf',
    ]);
    expect(filterDirectoryEntries(sample, 'папка').map((e) => e.name)).toEqual(['Projects']);
  });
});

describe('formatting helpers', () => {
  it('formats sizes cleanly across B/KB/MB/GB/TB and folders', () => {
    expect(formatFileSize(4096, true)).toBe('—');
    expect(formatFileSize(0, false)).toBe('0 Б');
    expect(formatFileSize(-10, false)).toBe('0 Б');
    expect(formatFileSize(512, false)).toBe('512 Б');
    expect(formatFileSize(1536, false)).toBe('1.5 КБ');
    expect(formatFileSize(2 * 1024 * 1024, false)).toBe('2 МБ');
    expect(formatFileSize(2.5 * 1024 * 1024 * 1024, false)).toBe('2.5 ГБ');
  });

  it('formats Russian/numeric modified dates deterministically', () => {
    const now = new Date(2026, 10, 21, 18, 0, 0).getTime(); // 21.11.2026 18:00
    const todayTs = new Date(2026, 10, 21, 9, 5, 0).getTime();
    const yesterdayTs = new Date(2026, 10, 20, 22, 15, 0).getTime();
    const olderTs = new Date(2025, 0, 7, 8, 3, 0).getTime();

    expect(formatFileDate(0, now)).toBe('—');
    expect(formatFileDate(todayTs, now)).toBe('Сегодня, 09:05');
    expect(formatFileDate(yesterdayTs, now)).toBe('Вчера, 22:15');
    expect(formatFileDate(olderTs, now)).toBe('07.01.2025 08:03');
  });

  it('formats entry type labels cleanly', () => {
    expect(getEntryTypeLabel({ isDir: true, category: 'folder', extension: '' })).toBe('Папка');
    expect(getEntryTypeLabel({ isDir: false, category: 'image', extension: 'png' })).toBe(
      'Изображение PNG',
    );
    expect(getEntryTypeLabel({ isDir: false, category: 'code', extension: 'ts' })).toBe('Код TS');
    expect(getEntryTypeLabel({ isDir: false, category: 'file', extension: '' })).toBe('Файл');
  });
});

describe('POSIX history normalization and navigation', () => {
  it('preserves POSIX forward slashes and strips trailing slashes except root', () => {
    expect(normalizeHistoryPath('/')).toBe('/');
    expect(normalizeHistoryPath('///')).toBe('/');
    expect(normalizeHistoryPath('/Users/alex/Desktop/')).toBe('/Users/alex/Desktop');
    expect(normalizeHistoryPath('/Volumes/WorkSSD///')).toBe('/Volumes/WorkSSD');
    expect(normalizeHistoryPath('C:\\Users\\alex\\')).toBe('c:\\users\\alex');
    expect(normalizeHistoryPath('C:\\')).toBe('c:\\');
    expect(normalizeHistoryPath('\\\\server\\share\\')).toBe('\\\\server\\share');
  });

  it('pushes and navigates POSIX paths without turning / into \\', () => {
    let h = createHistoryState('/');
    h = pushHistoryPath(h, '/'); // duplicate root ignored
    h = pushHistoryPath(h, '///'); // duplicate normalized root ignored
    expect(h.entries).toEqual(['/']);

    h = pushHistoryPath(h, '/Users/alex/Desktop/');
    h = pushHistoryPath(h, '/Users/alex/Desktop'); // trailing-slash duplicate ignored
    h = pushHistoryPath(h, '/Volumes/WorkSSD');
    expect(h.entries).toEqual(['/', '/Users/alex/Desktop/', '/Volumes/WorkSSD']);
    expect(h.index).toBe(2);

    const back1 = navigateHistoryBack(h);
    expect(back1.path).toBe('/Users/alex/Desktop/');
    h = back1.state;

    const back2 = navigateHistoryBack(h);
    expect(back2.path).toBe('/');
    h = back2.state;

    const fwd = navigateHistoryForward(h);
    expect(fwd.path).toBe('/Users/alex/Desktop/');
  });
});
