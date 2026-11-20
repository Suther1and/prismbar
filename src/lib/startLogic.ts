import { canRunAsAdmin } from './flyoutLogic';
import type { IndexedApp, PinItem, TestBarConfig } from './ipc';

export type StartAnimPhase = 'open' | 'closing';

export interface StartAnimState {
  phase: StartAnimPhase;
  openSeq: number;
}

export const START_EXIT_ANIM_MS = 130;

export const INITIAL_START_ANIM_STATE: StartAnimState = {
  phase: 'open',
  openSeq: 0,
};

/**
 * Tracks Start Menu open/close animation phase. Every `start-state(true)` bumps `openSeq`
 * so `.start-chassis` replays its crisp spring entrance animation and cancels any in-flight close.
 */
export function reduceStartState(state: StartAnimState, visible: boolean): StartAnimState {
  if (visible) {
    return {
      phase: 'open',
      openSeq: state.openSeq + 1,
    };
  }
  return {
    phase: 'closing',
    openSeq: state.openSeq,
  };
}

export interface SpaceAccentOption {
  hex: string;
  rgb: string;
  label: string;
}

export const CUSTOM_SPACE_ACCENTS: readonly SpaceAccentOption[] = [
  { hex: '#818CF8', rgb: '129, 140, 248', label: 'Индиго' },
  { hex: '#38BDF8', rgb: '56, 189, 248', label: 'Небесный' },
  { hex: '#34D399', rgb: '52, 211, 153', label: 'Изумруд' },
  { hex: '#FB7185', rgb: '251, 113, 133', label: 'Коралл' },
  { hex: '#A78BFA', rgb: '167, 139, 250', label: 'Аметист' },
] as const;

export const BUILTIN_SPACES = [
  { id: 'personal', label: 'Личное' },
  { id: 'work', label: 'Работа' },
  { id: 'studio', label: 'Студия' },
  { id: 'games', label: 'Игры' },
] as const;

export function getSpaceDisplayName(spaceId: string, config: TestBarConfig | null): string {
  const builtin = BUILTIN_SPACES.find((s) => s.id === spaceId);
  if (builtin) return builtin.label;
  const custom = config?.customSpaces.find((s) => s.id === spaceId);
  return custom?.name ?? 'Пространство';
}

export function validateCustomSpaceName(
  raw: string,
  existingCustomNames: string[],
): { ok: true; name: string } | { ok: false; error: string } {
  const name = raw.trim().replace(/\s+/g, ' ');
  if (!name) {
    return { ok: false, error: 'Введите название пространства' };
  }
  if (name.length > 18) {
    return { ok: false, error: 'Максимум 18 символов' };
  }
  const low = name.toLowerCase();
  const reserved = [
    ...BUILTIN_SPACES.map((s) => s.label.toLowerCase()),
    ...existingCustomNames.map((n) => n.trim().toLowerCase()),
  ];
  if (reserved.includes(low)) {
    return { ok: false, error: 'Такое пространство уже есть' };
  }
  return { ok: true, name };
}

/**
 * Splits a Space's pins into 2 hero tiles and ALL remaining compact tiles with zero truncation.
 */
export function partitionHeroAndCompact(pins: PinItem[]): {
  heroes: PinItem[];
  compact: PinItem[];
} {
  return {
    heroes: pins.slice(0, 2),
    compact: pins.slice(2),
  };
}

function normTarget(value: string | null | undefined): string {
  return (value ?? '').trim().replace(/\//g, '\\').toLowerCase();
}

function normAumid(value: string | null | undefined): string {
  const low = (value ?? '').trim().toLowerCase();
  return low.startsWith('shell:appsfolder\\') ? low.slice('shell:appsfolder\\'.length).trim() : low;
}

export function getSpacePinList(spaceId: string, config: TestBarConfig | null): PinItem[] {
  if (!config) return [];
  switch (spaceId) {
    case 'personal':
      return [...config.personalPins, ...config.personalLinks];
    case 'work':
      return [...config.workFolders, ...config.workApps];
    case 'studio':
      return [...config.studioPins, ...config.studioTools];
    case 'games':
      return [...config.gameLaunchers, ...config.gameCombos];
    default:
      return config.customSpaces.find((s) => s.id === spaceId)?.items ?? [];
  }
}

export function getPinnedItemInSpace(
  app: Pick<IndexedApp, 'name' | 'target' | 'aumid'>,
  spaceId: string,
  config: TestBarConfig | null,
): PinItem | null {
  const list = getSpacePinList(spaceId, config);
  const appTarget = normTarget(app.target);
  const appAumid = normAumid(app.aumid || (appTarget.startsWith('shell:appsfolder\\') ? appTarget : ''));
  const appName = app.name.trim().toLowerCase();

  return (
    list.find((pin) => {
      const pTarget = normTarget(pin.target);
      const pAumid = normAumid(pin.matchAumid || (pTarget.startsWith('shell:appsfolder\\') ? pTarget : ''));
      if (appAumid && pAumid && appAumid === pAumid) return true;
      if (appTarget && pTarget && appTarget === pTarget) return true;
      if (appName && pin.title.trim().toLowerCase() === appName) return true;
      return false;
    }) ?? null
  );
}

export function isAppPinnedInSpace(
  app: Pick<IndexedApp, 'name' | 'target' | 'aumid'>,
  spaceId: string,
  config: TestBarConfig | null,
): boolean {
  return getPinnedItemInSpace(app, spaceId, config) !== null;
}

export type ResolvedPinAction =
  | { type: 'launch'; target: string; args: string | null; asAdmin: boolean; closeStart: boolean }
  | { type: 'terminal'; path: string }
  | { type: 'copy_path'; text: string; toast: string }
  | { type: 'combo'; targets: string[] };

function exeBasename(target: string): string {
  const norm = normTarget(target);
  const idx = norm.lastIndexOf('\\');
  return idx >= 0 ? norm.slice(idx + 1) : norm;
}

function mergeArgs(baseArgs: string | null | undefined, extraArg: string): string {
  const base = (baseArgs ?? '').trim();
  if (!base) return extraArg;
  if (base.toLowerCase().includes(extraArg.toLowerCase())) return base;
  return `${base} ${extraArg}`;
}

/**
 * Resolves the exact action for a Start Menu tile click, making the `Shift` second layer
 * 100% functional and faithful to every displayed `altAction` badge.
 */
export function resolvePinAction(item: PinItem, shiftHeld: boolean): ResolvedPinAction {
  if (item.kind === 'combo') {
    return { type: 'combo', targets: (item.comboTargets ?? []).filter((t) => Boolean(t.trim())) };
  }

  const baseArgs = item.args?.trim() || null;

  if (!shiftHeld) {
    return {
      type: 'launch',
      target: item.target,
      args: baseArgs,
      asAdmin: false,
      closeStart: true,
    };
  }

  const alt = (item.altAction || '').trim().toLowerCase();
  const exe = exeBasename(item.target);
  const iconLow = (item.iconKey || '').toLowerCase();

  // 1. Copy path
  if (alt.includes('скопировать путь') || alt.includes('copy path')) {
    return {
      type: 'copy_path',
      text: item.target,
      toast: 'Путь скопирован',
    };
  }

  // 2. Open in terminal (explicit badge or folder default)
  if (alt.includes('терминал') || (item.kind === 'folder' && !alt.includes('открыть'))) {
    return {
      type: 'terminal',
      path: item.target,
    };
  }

  // 3. Private / Incognito browser window
  if (alt.includes('приват') || alt.includes('инкогнито') || alt.includes('inprivate')) {
    let flag = '--incognito';
    if (exe === 'zen.exe' || exe === 'firefox.exe' || iconLow.includes('zen')) {
      flag = '--private-window';
    } else if (exe === 'msedge.exe') {
      flag = '--inprivate';
    }
    return {
      type: 'launch',
      target: item.target,
      args: mergeArgs(baseArgs, flag),
      asAdmin: false,
      closeStart: true,
    };
  }

  // 4. New window / New project instance
  if (alt.includes('новое окно') || alt.includes('новый проект')) {
    let flag: string | null = null;
    if (exe === 'explorer.exe' || iconLow.includes('explorer')) {
      flag = '/n';
    } else if (
      exe === 'code.exe' ||
      exe === 'cursor.exe' ||
      exe === 'antigravity.exe' ||
      iconLow.includes('vscode') ||
      iconLow.includes('cursor') ||
      iconLow.includes('antigravity')
    ) {
      flag = '--new-window';
    } else if (exe === 'zen.exe' || exe === 'firefox.exe' || iconLow.includes('zen')) {
      flag = '--new-window';
    }
    return {
      type: 'launch',
      target: item.target,
      args: flag ? mergeArgs(baseArgs, flag) : baseArgs,
      asAdmin: false,
      closeStart: true,
    };
  }

  // 5. Run as administrator
  if (alt.includes('админ')) {
    return {
      type: 'launch',
      target: item.target,
      args: baseArgs,
      asAdmin: canRunAsAdmin(item.target),
      closeStart: true,
    };
  }

  // 6. Clean fallback for app-specific secondary actions (Избранное, Новый чат, Новая заметка, etc.)
  return {
    type: 'launch',
    target: item.target,
    args: baseArgs,
    asAdmin: false,
    closeStart: true,
  };
}

export type PowerActionKind = 'lock' | 'sleep' | 'reboot' | 'shutdown';
export type ArmedPowerKind = 'reboot' | 'shutdown' | null;

export interface PowerClickResult {
  execute: PowerActionKind | null;
  nextArmed: ArmedPowerKind;
  closePower: boolean;
}

/**
 * 2-click armed Power state machine: Lock and Sleep execute immediately,
 * while Reboot and Shutdown arm on first click and execute only on matching second click.
 */
export function reducePowerClick(
  armed: ArmedPowerKind,
  kind: PowerActionKind,
): PowerClickResult {
  if (kind === 'reboot' || kind === 'shutdown') {
    if (armed !== kind) {
      return {
        execute: null,
        nextArmed: kind,
        closePower: false,
      };
    }
  }
  return {
    execute: kind,
    nextArmed: null,
    closePower: true,
  };
}

export interface SlidingPillRect {
  left: number;
  width: number;
  visible: boolean;
}

export function computeSlidingPillRect(offsetLeft: number, offsetWidth: number): SlidingPillRect {
  if (!Number.isFinite(offsetLeft) || !Number.isFinite(offsetWidth) || offsetWidth <= 0) {
    return { left: 0, width: 0, visible: false };
  }
  return {
    left: Math.max(0, Math.round(offsetLeft)),
    width: Math.max(0, Math.round(offsetWidth)),
    visible: true,
  };
}

export function normalizeUrlPinInput(
  rawUrl: string,
  customTitle?: string,
): { url: string; title: string } | null {
  let raw = rawUrl.trim();
  if (!raw) return null;
  if (!/^https?:\/\//i.test(raw)) {
    raw = `https://${raw}`;
  }
  let fallbackTitle = raw;
  try {
    const parsed = new URL(raw);
    fallbackTitle = parsed.hostname.replace(/^www\./i, '') || raw;
  } catch {
    return null;
  }
  const title = (customTitle ?? '').trim() || fallbackTitle;
  return { url: raw, title };
}

export const BANNED_START_HINTS = [
  'Начни печатать для поиска',
  'Второй слой',
  'Tab Пространства',
  'Режимы работы',
] as const;

export const BANNED_UNICODE_GLYPHS = ['▶', '↗', '⚡', '🔥', '🎮', '📁', '🔍', '⚙', '📌', '✨'] as const;

export function findBannedStartArtifacts(sourceText: string): string[] {
  const hits: string[] = [];
  for (const hint of BANNED_START_HINTS) {
    if (sourceText.includes(hint)) {
      hits.push(hint);
    }
  }
  for (const glyph of BANNED_UNICODE_GLYPHS) {
    if (sourceText.includes(glyph)) {
      hits.push(glyph);
    }
  }
  return hits;
}
