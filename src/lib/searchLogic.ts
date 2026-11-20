import { canRunAsAdmin } from './flyoutLogic';
import {
  isCacheIconKey,
  type IndexedApp,
  type IndexedFile,
  type PinItem,
  type SteamGame,
  type TestBarConfig,
} from './ipc';
import { collectPhoneticAliases, swapLayout } from './layoutMap';

export type SearchSourceKind = 'pinned' | 'app' | 'uwp' | 'game' | 'folder' | 'file' | 'url' | 'combo';

export interface SearchCorpusItem {
  id: string;
  name: string;
  subtitle: string;
  category: string;
  target: string;
  args?: string | null;
  kind: 'app' | 'folder' | 'file' | 'url' | 'combo';
  source: SearchSourceKind;
  iconData?: string | null;
  iconKey?: string | null;
  cacheIconKey?: string | null;
  comboTargets?: string[] | null;
  pinned: boolean;
}

export interface SearchCorpusInput {
  config: TestBarConfig | null;
  apps: IndexedApp[];
  games: SteamGame[];
  files: IndexedFile[];
  iconMap: Record<string, string>;
}

export interface RankedSearchResult {
  items: SearchCorpusItem[];
  swappedHint: string | null;
  totalMatches: number;
}

function normKey(v: string | null | undefined): string {
  return (v ?? '').trim().replace(/\//g, '\\').toLowerCase();
}

function resolvePinIcon(pin: PinItem, iconMap: Record<string, string>): {
  iconData: string | null;
  cacheIconKey: string | null;
} {
  const cacheIconKey = isCacheIconKey(pin.iconKey) ? pin.iconKey : null;
  const iconData =
    (cacheIconKey ? iconMap[cacheIconKey] : null) ||
    pin.iconData ||
    iconMap[pin.target] ||
    null;
  return { iconData, cacheIconKey };
}

/**
 * Builds the complete Start Menu search corpus spanning ALL Spaces (including personalLinks,
 * workApps, studioTools, gameLaunchers, gameCombos, customSpaces), Steam games, installed
 * `.lnk` + UWP apps, and indexed user files (Desktop/Documents/Downloads).
 */
export function buildSearchCorpus({
  config,
  apps,
  games,
  files,
  iconMap,
}: SearchCorpusInput): SearchCorpusItem[] {
  const pool: SearchCorpusItem[] = [];
  const seenTargets = new Set<string>();
  const seenNameKind = new Set<string>();

  const pushUnique = (item: SearchCorpusItem) => {
    const cleanName = item.name.trim();
    const cleanTarget = item.target.trim();
    if (!cleanName) return;
    if (item.kind !== 'combo' && !cleanTarget) return;
    if (item.kind === 'combo' && (!item.comboTargets || item.comboTargets.length === 0)) return;

    const tKey = item.kind === 'combo'
      ? `combo:${(item.comboTargets ?? []).map(normKey).join('|')}`
      : `${item.kind}:${normKey(cleanTarget)}`;
    const nkKey = `${item.kind}:${cleanName.toLowerCase()}`;

    if (seenTargets.has(tKey) || seenNameKind.has(nkKey)) {
      return;
    }
    seenTargets.add(tKey);
    seenNameKind.add(nkKey);
    pool.push(item);
  };

  const addPinList = (pins: PinItem[] | undefined, category: string) => {
    for (const p of pins ?? []) {
      const { iconData, cacheIconKey } = resolvePinIcon(p, iconMap);
      pushUnique({
        id: `pin:${category}:${p.id}`,
        name: p.title,
        subtitle: p.subtitle || category,
        category,
        target: p.target,
        args: p.args ?? null,
        kind: p.kind,
        source: p.kind === 'combo' ? 'combo' : 'pinned',
        iconData,
        iconKey: p.iconKey,
        cacheIconKey,
        comboTargets: p.comboTargets ?? null,
        pinned: true,
      });
    }
  };

  if (config) {
    addPinList(config.personalPins, 'Личное');
    addPinList(config.personalLinks, 'Быстрый доступ');
    addPinList(config.workFolders, 'Работа · Папка');
    addPinList(config.workApps, 'Работа');
    addPinList(config.studioPins, 'Студия');
    addPinList(config.studioTools, 'Студия · Инструмент');
    addPinList(config.gameLaunchers, 'Игры · Лаунчер');
    addPinList(config.gameCombos, 'Игры · Связка');
    for (const cs of config.customSpaces) {
      addPinList(cs.items, cs.name);
    }
  }

  for (const g of games) {
    pushUnique({
      id: `game:${g.appId}`,
      name: g.name,
      subtitle: g.launcher || 'Steam',
      category: 'Игра · Steam',
      target: g.target,
      args: null,
      kind: 'app',
      source: 'game',
      iconData: g.coverData ?? null,
      iconKey: 'steam',
      cacheIconKey: null,
      pinned: false,
    });
  }

  for (const a of apps) {
    const isUwp = Boolean(a.aumid) || a.target.toLowerCase().startsWith('shell:appsfolder\\');
    const cacheIconKey = isCacheIconKey(a.iconKey) ? a.iconKey : null;
    const iconData = (cacheIconKey ? iconMap[cacheIconKey] : null) || a.iconData || iconMap[a.target] || null;
    pushUnique({
      id: `app:${a.id}`,
      name: a.name,
      subtitle: isUwp ? `${a.category} · UWP` : a.category,
      category: isUwp ? `${a.category} · UWP` : a.category,
      target: a.target,
      args: null,
      kind: 'app',
      source: isUwp ? 'uwp' : 'app',
      iconData,
      iconKey: a.iconKey || a.name,
      cacheIconKey,
      pinned: false,
    });
  }

  for (const f of files) {
    const isFolder = f.kind === 'folder';
    pushUnique({
      id: `file:${f.id}`,
      name: f.name,
      subtitle: f.location,
      category: `${isFolder ? 'Папка' : 'Файл'} · ${f.location}`,
      target: f.target,
      args: null,
      kind: isFolder ? 'folder' : 'file',
      source: isFolder ? 'folder' : 'file',
      iconData: null,
      iconKey: isFolder ? 'folder' : 'file',
      cacheIconKey: null,
      pinned: false,
    });
  }

  return pool;
}

function hasWordStartMatch(text: string, term: string): boolean {
  const words = text.split(/[\s\-_./\\·]+/).filter(Boolean);
  return words.some((w) => w.startsWith(term));
}

function scoreTerm(
  item: SearchCorpusItem,
  nameLow: string,
  aliases: string[],
  metaLow: string,
  targetLow: string,
  term: string,
): number {
  let score = 0;
  if (nameLow === term) {
    score = 1000;
  } else if (nameLow.startsWith(term)) {
    score = 620;
  } else if (aliases.some((a) => a === term)) {
    score = 560;
  } else if (aliases.some((a) => a.startsWith(term))) {
    score = 500;
  } else if (hasWordStartMatch(nameLow, term)) {
    score = 420;
  } else if (nameLow.includes(term)) {
    score = 260;
  } else if (aliases.some((a) => a.includes(term))) {
    score = 220;
  } else if (metaLow.includes(term)) {
    score = 120;
  } else if (targetLow.includes(term)) {
    score = 70;
  }

  if (score === 0) return 0;

  if (item.pinned) score += 80;
  if (item.source === 'app' || item.source === 'uwp' || item.source === 'pinned') {
    score += 30;
  } else if (item.source === 'game') {
    score += 24;
  } else if (item.source === 'folder' || item.source === 'combo') {
    score += 14;
  }

  return score;
}

/**
 * Ranks search results with RU<->EN wrong-keyboard-layout matching (`ntkt` -> Telegram),
 * prioritizing exact/prefix > word-start > substring matches and boosting pinned items.
 */
export function rankSearchResults(
  corpus: SearchCorpusItem[],
  rawQuery: string,
  limit = 10,
): RankedSearchResult {
  const q = rawQuery.trim().toLowerCase();
  if (!q) {
    return {
      items: corpus.slice(0, limit),
      swappedHint: null,
      totalMatches: corpus.length,
    };
  }

  const swapped = swapLayout(q);
  const canSwap = swapped !== q;

  const scored: Array<{ item: SearchCorpusItem; score: number; usedSwapped: boolean }> = [];

  for (const item of corpus) {
    const nameLow = item.name.toLowerCase();
    const metaLow = `${item.category} ${item.subtitle}`.toLowerCase();
    const targetLow = item.target.toLowerCase();
    const aliases = collectPhoneticAliases(nameLow, metaLow, targetLow);

    const directScore = scoreTerm(item, nameLow, aliases, metaLow, targetLow, q);
    if (directScore > 0) {
      // Direct layout match wins ties over swapped match
      scored.push({ item, score: directScore + 40, usedSwapped: false });
      continue;
    }

    if (canSwap) {
      const swappedScore = scoreTerm(item, nameLow, aliases, metaLow, targetLow, swapped);
      if (swappedScore > 0) {
        scored.push({ item, score: swappedScore, usedSwapped: true });
      }
    }
  }

  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    if (a.item.name.length !== b.item.name.length) return a.item.name.length - b.item.name.length;
    return a.item.name.localeCompare(b.item.name, 'ru');
  });

  const top = scored.slice(0, limit);
  const swappedHint = top.length > 0 && top.some((entry) => entry.usedSwapped) ? swapped : null;

  return {
    items: top.map((entry) => entry.item),
    swappedHint,
    totalMatches: scored.length,
  };
}

export type SearchLaunchDispatch =
  | { command: 'launch_combo'; args: { targets: string[] } }
  | {
      command: 'launch_item';
      args: { target: string; args: string | null; asAdmin: boolean; closeStart: boolean };
    };

export function resolveSearchLaunchPayload(
  item: SearchCorpusItem,
  ctrlKey: boolean,
): SearchLaunchDispatch {
  if (item.kind === 'combo') {
    return {
      command: 'launch_combo',
      args: { targets: item.comboTargets ?? [] },
    };
  }
  return {
    command: 'launch_item',
    args: {
      target: item.target,
      args: item.args ?? null,
      asAdmin: ctrlKey && canRunAsAdmin(item.target),
      closeStart: true,
    },
  };
}

export function isPrintableSearchTrigger(
  e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'altKey' | 'metaKey'>,
): boolean {
  if (e.ctrlKey || e.altKey || e.metaKey) return false;
  if (e.key.length !== 1) return false;
  // Ignore plain Space so Space can still activate a focused button without hijacking
  return e.key !== ' ';
}
