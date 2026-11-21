import { canRunAsAdmin } from './flyoutLogic';
import type {
  AudioState,
  ClipboardEntry,
  ClipboardKind,
  DiskHeadroomInfo,
  HeaderWidgetSide,
  IndexedApp,
  MediaState,
  PeripheralBattery,
  PinItem,
  SpaceConfig,
  SpaceHeaderWidget,
  SpaceLayoutKind,
  SpaceWidgetId,
  TelemetryState,
  TestBarConfig,
  TileSection,
  TopProcessInfo,
  WidgetPreset,
} from './ipc';

export type StartAnimPhase = 'hidden' | 'arming' | 'open' | 'closing';

export interface StartAnimState {
  phase: StartAnimPhase;
  open: boolean;
  closing: boolean;
  openSeq: number;
  gen: number;
}

export const START_ENTER_ANIM_MS = 240;
export const START_OPACITY_ANIM_MS = 170;
export const START_EXIT_ANIM_MS = 120;

export const INITIAL_START_ANIM_STATE: StartAnimState = {
  phase: 'hidden',
  open: false,
  closing: false,
  openSeq: 0,
  gen: 0,
};

export interface StartStateReduceInput {
  open: boolean;
  gen?: number;
  immediate?: boolean;
}

/**
 * Transitions Start Menu animation phase deterministically without unmounting DOM:
 * - `open: true` -> `'arming'` (locks chassis at opacity 0 / translateY(20px) scale(0.96)
 *   until post-SW_SHOW visible frame + double rAF triggers the WAAPI spring, preventing any
 *   1-frame pop-in flash or Chromium SW_HIDE->SW_SHOW animation collapse!)
 * - `open: false, immediate: true` -> `'hidden'` immediately
 * - `open: false` -> `'closing'` (or stays `'hidden'` if already hidden)
 */
export function reduceStartState(
  state: StartAnimState,
  visibleOrEvent: boolean | StartStateReduceInput,
  opts?: { gen?: number; immediate?: boolean },
): StartAnimState {
  const isObj = typeof visibleOrEvent === 'object' && visibleOrEvent !== null;
  const visible = isObj ? Boolean(visibleOrEvent.open) : Boolean(visibleOrEvent);
  const rawGen = isObj ? visibleOrEvent.gen : opts?.gen;
  const immediate = isObj ? Boolean(visibleOrEvent.immediate) : Boolean(opts?.immediate);
  const nextGen = typeof rawGen === 'number' && Number.isFinite(rawGen) ? rawGen : state.gen;

  if (visible) {
    return {
      phase: 'arming',
      open: true,
      closing: false,
      openSeq: state.openSeq + 1,
      gen: nextGen,
    };
  }

  if (immediate || state.phase === 'hidden') {
    return {
      phase: 'hidden',
      open: false,
      closing: false,
      openSeq: state.openSeq,
      gen: nextGen,
    };
  }

  return {
    phase: 'closing',
    open: false,
    closing: true,
    openSeq: state.openSeq,
    gen: nextGen,
  };
}

/**
 * Commits an `'arming'` Start Menu surface into its settled `'open'` state once the WAAPI
 * spring finishes (or immediately under `prefers-reduced-motion`). Ignores stale generations.
 */
export function settleStartOpen(
  state: StartAnimState,
  expectedGen?: number,
): StartAnimState {
  if (state.phase !== 'arming') {
    return state;
  }
  if (typeof expectedGen === 'number' && state.gen !== expectedGen) {
    return state;
  }
  return {
    ...state,
    phase: 'open',
    open: true,
    closing: false,
  };
}

/**
 * Class marker matching the current Start animation phase (`open-arming`, `open-a/b`, `closing`).
 */
export function getStartChassisAnimClass(
  anim: Pick<StartAnimState, 'phase' | 'openSeq'>,
): string {
  if (anim.phase === 'arming') {
    return 'open-arming';
  }
  if (anim.phase === 'open') {
    return anim.openSeq % 2 === 1 ? 'open-anim open-a' : 'open-anim open-b';
  }
  if (anim.phase === 'closing') {
    return 'closing';
  }
  return '';
}

/**
 * Settles a `'closing'` Start Menu surface to `'hidden'` if `expectedGen` still matches.
 */
export function completeStartClosing(
  state: StartAnimState,
  expectedGen?: number,
): StartAnimState {
  if (state.phase !== 'closing') {
    return state;
  }
  if (typeof expectedGen === 'number' && state.gen !== expectedGen) {
    return state;
  }
  return {
    ...state,
    phase: 'hidden',
    open: false,
    closing: false,
  };
}

/**
 * Merges an incoming WASAPI `AudioState` push while preserving the local optimistic volume of
 * any slider currently being dragged (or within its post-commit settle window) so IPC echoes
 * never yank the slider thumb backward mid-drag.
 */
export function mergeAudioStateRespectingLocks(
  prev: AudioState | null,
  incoming: AudioState,
  lockedUntilByKey: ReadonlyMap<string, number>,
  nowMs: number,
): AudioState {
  if (!prev || lockedUntilByKey.size === 0) {
    return incoming;
  }
  const masterLock = lockedUntilByKey.get('__master__');
  const preserveMaster = masterLock !== undefined && masterLock > nowMs;
  let sessionsChanged = false;
  const nextSessions = incoming.sessions.map((sess) => {
    const lockUntil = lockedUntilByKey.get(sess.id);
    if (lockUntil !== undefined && lockUntil > nowMs) {
      const prevSess = prev.sessions.find((p) => p.id === sess.id);
      if (prevSess && prevSess.volume !== sess.volume) {
        sessionsChanged = true;
        return { ...sess, volume: prevSess.volume };
      }
    }
    return sess;
  });

  if (!preserveMaster && !sessionsChanged) {
    return incoming;
  }
  return {
    ...incoming,
    masterVolume: preserveMaster ? prev.masterVolume : incoming.masterVolume,
    sessions: sessionsChanged ? nextSessions : incoming.sessions,
  };
}

export interface SpaceAccentOption {
  hex: string;
  rgb: string;
  label: string;
}

export const MAX_MODULAR_SPACES = 12;

export const CUSTOM_SPACE_ACCENTS: readonly SpaceAccentOption[] = [
  { hex: '#818CF8', rgb: '129, 140, 248', label: 'Индиго' },
  { hex: '#38BDF8', rgb: '56, 189, 248', label: 'Кобальт' },
  { hex: '#34D399', rgb: '52, 211, 153', label: 'Изумруд' },
  { hex: '#FB7185', rgb: '251, 113, 133', label: 'Коралл' },
  { hex: '#A78BFA', rgb: '167, 139, 250', label: 'Аметист' },
] as const;

export const MODULAR_SPACE_ACCENTS: readonly SpaceAccentOption[] = [
  ...CUSTOM_SPACE_ACCENTS,
  { hex: '#2DD4BF', rgb: '45, 212, 191', label: 'Бирюза' },
  { hex: '#FBBF24', rgb: '251, 191, 36', label: 'Янтарь' },
  { hex: '#F472B6', rgb: '244, 114, 182', label: 'Роза' },
] as const;

export interface TileColorSwatch {
  id: string;
  label: string;
  hex: string;
  rgb: string;
}

export const TILE_COLOR_PALETTE: readonly TileColorSwatch[] = [
  { id: 'indigo', label: 'Индиго', hex: '#818CF8', rgb: '129, 140, 248' },
  { id: 'sky', label: 'Кобальт', hex: '#38BDF8', rgb: '56, 189, 248' },
  { id: 'teal', label: 'Бирюза', hex: '#2DD4BF', rgb: '45, 212, 191' },
  { id: 'emerald', label: 'Изумруд', hex: '#34D399', rgb: '52, 211, 153' },
  { id: 'amber', label: 'Янтарь', hex: '#FBBF24', rgb: '251, 191, 36' },
  { id: 'orange', label: 'Оранж', hex: '#FB923C', rgb: '251, 146, 60' },
  { id: 'coral', label: 'Коралл', hex: '#FB7185', rgb: '251, 113, 133' },
  { id: 'rose', label: 'Роза', hex: '#F472B6', rgb: '244, 114, 182' },
  { id: 'violet', label: 'Аметист', hex: '#A78BFA', rgb: '167, 139, 250' },
  { id: 'slate', label: 'Графит', hex: '#94A3B8', rgb: '148, 163, 184' },
] as const;

export type SpaceTemplateId = 'blank' | 'personal' | 'work' | 'studio' | 'games';

export interface SpaceTemplateOption {
  id: SpaceTemplateId;
  label: string;
  accentHex: string;
  accentRgb: string;
  layoutKind: SpaceLayoutKind;
  showHeroRow: boolean;
  widgets: SpaceWidgetId[];
}

export const SPACE_TEMPLATES: readonly SpaceTemplateOption[] = [
  {
    id: 'blank',
    label: 'Пустое',
    accentHex: '#A78BFA',
    accentRgb: '167, 139, 250',
    layoutKind: 'grid',
    showHeroRow: false,
    widgets: [],
  },
  {
    id: 'personal',
    label: 'Личное',
    accentHex: '#818CF8',
    accentRgb: '129, 140, 248',
    layoutKind: 'grid',
    showHeroRow: true,
    widgets: ['media', 'audio_mixer', 'clipboard'],
  },
  {
    id: 'work',
    label: 'Работа',
    accentHex: '#38BDF8',
    accentRgb: '56, 189, 248',
    layoutKind: 'grid',
    showHeroRow: false,
    widgets: ['pc_health', 'top_processes'],
  },
  {
    id: 'studio',
    label: 'Студия',
    accentHex: '#34D399',
    accentRgb: '52, 211, 153',
    layoutKind: 'grid',
    showHeroRow: true,
    widgets: ['pc_health', 'audio_mixer', 'top_processes'],
  },
  {
    id: 'games',
    label: 'Игры',
    accentHex: '#FB7185',
    accentRgb: '251, 113, 133',
    layoutKind: 'games',
    showHeroRow: false,
    widgets: ['audio_mixer', 'game_combos', 'pc_health'],
  },
] as const;

export interface SpaceWidgetOption {
  id: SpaceWidgetId;
  label: string;
  description: string;
}

export const SPACE_WIDGET_CATALOG: readonly SpaceWidgetOption[] = [
  { id: 'media', label: 'Медиаплеер', description: 'Управление треками и прогресс' },
  { id: 'audio_mixer', label: 'Звук и микшер', description: 'Устройства вывода, микрофон и потоки' },
  { id: 'clipboard', label: 'Буфер обмена', description: 'История копирования и закрепления' },
  { id: 'pc_health', label: 'Нагрузка ПК', description: 'CPU, RAM, GPU, VRAM и диски' },
  { id: 'top_processes', label: 'Потребители памяти', description: 'Процессы по ОЗУ и VRAM' },
  { id: 'game_combos', label: 'Игровые связки', description: 'Запуск наборов программ в 1 клик' },
  { id: 'weather', label: 'Погода', description: 'Текущая погода и прогноз на 3 дня' },
  { id: 'batteries', label: 'Батареи устройств', description: 'Заряд мыши, наушников и геймпадов' },
  { id: 'notes', label: 'Заметки и дела', description: 'Быстрые записи и чек-лист задач' },
  { id: 'timer', label: 'Таймер фокуса', description: 'Обратный отсчёт и пресеты времени' },
  { id: 'network', label: 'Сеть и пинг', description: 'Скорость трафика, IP и задержка' },
  { id: 'quick_toggles', label: 'Быстрые тумблеры', description: 'Микрофон, питание, скриншот и корзина' },
] as const;

export const BUILTIN_SPACES = [
  { id: 'personal', label: 'Личное' },
  { id: 'work', label: 'Работа' },
  { id: 'studio', label: 'Студия' },
  { id: 'games', label: 'Игры' },
] as const;

function mapLegacyPresetToWidgets(preset: WidgetPreset | undefined): SpaceWidgetId[] {
  switch (preset) {
    case 'telemetry_processes':
      return ['pc_health', 'top_processes'];
    case 'audio_mic':
      return ['audio_mixer', 'media'];
    case 'full_grid':
      return [];
    default:
      return ['media', 'audio_mixer', 'clipboard'];
  }
}

/**
 * Resolves the authoritative modular `SpaceConfig[]` list from `config.spaces`,
 * synthesizing equivalent entries from legacy v1 fields only when `config.spaces` is missing or empty.
 */
export function resolveConfiguredSpaces(config: TestBarConfig | null): SpaceConfig[] {
  if (config?.spaces && config.spaces.length > 0) {
    return config.spaces;
  }
  const legacySpaces: SpaceConfig[] = [
    {
      id: 'personal',
      name: 'Личное',
      accentHex: '#818CF8',
      accentRgb: '129, 140, 248',
      layoutKind: 'grid',
      showHeroRow: true,
      widgets: ['media', 'audio_mixer', 'clipboard'],
      items: [...(config?.personalPins ?? []), ...(config?.personalLinks ?? [])],
    },
    {
      id: 'work',
      name: 'Работа',
      accentHex: '#38BDF8',
      accentRgb: '56, 189, 248',
      layoutKind: 'grid',
      showHeroRow: false,
      widgets: ['pc_health', 'top_processes'],
      items: [...(config?.workFolders ?? []), ...(config?.workApps ?? [])],
    },
    {
      id: 'studio',
      name: 'Студия',
      accentHex: '#34D399',
      accentRgb: '52, 211, 153',
      layoutKind: 'grid',
      showHeroRow: true,
      widgets: ['pc_health', 'audio_mixer', 'top_processes'],
      items: [...(config?.studioPins ?? []), ...(config?.studioTools ?? [])],
    },
    {
      id: 'games',
      name: 'Игры',
      accentHex: '#FB7185',
      accentRgb: '251, 113, 133',
      layoutKind: 'games',
      showHeroRow: false,
      widgets: ['audio_mixer', 'game_combos', 'pc_health'],
      items: [...(config?.gameLaunchers ?? [])],
    },
  ];

  for (const cs of config?.customSpaces ?? []) {
    legacySpaces.push({
      id: cs.id,
      name: cs.name,
      accentHex: cs.accentHex || '#A78BFA',
      accentRgb: cs.accentRgb || '167, 139, 250',
      layoutKind: 'grid',
      showHeroRow: false,
      widgets: mapLegacyPresetToWidgets(cs.widgetPreset),
      items: cs.items ?? [],
    });
  }

  return legacySpaces;
}

export function getSpaceDisplayName(spaceId: string, config: TestBarConfig | null): string {
  const modular = config?.spaces?.find((s) => s.id === spaceId);
  if (modular) return modular.name;
  const builtin = BUILTIN_SPACES.find((s) => s.id === spaceId);
  if (builtin) return builtin.label;
  const custom = config?.customSpaces.find((s) => s.id === spaceId);
  return custom?.name ?? 'Пространство';
}

export function validateSpaceName(
  raw: string,
  existingNames: readonly string[],
  currentName?: string,
): { ok: true; name: string } | { ok: false; error: string } {
  const name = raw.trim().replace(/\s+/g, ' ');
  if (!name) {
    return { ok: false, error: 'Введите название пространства' };
  }
  if (name.length > 18) {
    return { ok: false, error: 'Максимум 18 символов' };
  }
  const low = name.toLowerCase();
  const currentLow = (currentName ?? '').trim().toLowerCase();
  const duplicate = existingNames.some((n) => {
    const clean = n.trim().toLowerCase();
    return clean === low && clean !== currentLow;
  });
  if (duplicate) {
    return { ok: false, error: 'Такое пространство уже есть' };
  }
  return { ok: true, name };
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

/**
 * Splits a Space's items into Upper Main Grid (`section !== 'bottom'`) and Lower Docked
 * Section (`section === 'bottom'`) preserving relative order within each section.
 */
export function partitionSpaceItemsBySection(items: readonly PinItem[]): {
  mainItems: PinItem[];
  bottomItems: PinItem[];
} {
  const mainItems: PinItem[] = [];
  const bottomItems: PinItem[] = [];
  for (const item of items) {
    if (item.section === 'bottom') {
      bottomItems.push(item);
    } else {
      mainItems.push(item);
    }
  }
  return { mainItems, bottomItems };
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
  if (config.spaces && config.spaces.length > 0) {
    const sp = config.spaces.find((s) => s.id === spaceId);
    if (sp) return sp.items;
  }
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

export const START_DRAG_THRESHOLD_PX = 6;

export interface StartPinDragSlot {
  spaceKey: string;
  idx: number;
}

export function hasExceededDragThreshold(
  startX: number,
  startY: number,
  curX: number,
  curY: number,
  thresholdPx: number = START_DRAG_THRESHOLD_PX,
): boolean {
  const dx = curX - startX;
  const dy = curY - startY;
  return dx * dx + dy * dy >= thresholdPx * thresholdPx;
}

export function computePointerReorderRequest(
  origin: StartPinDragSlot | null,
  target: StartPinDragSlot | null,
): { space: string; fromIdx: number; toIdx: number } | null {
  if (!origin || !target) return null;
  if (!origin.spaceKey || origin.spaceKey !== target.spaceKey) return null;
  if (!Number.isInteger(origin.idx) || !Number.isInteger(target.idx)) return null;
  if (origin.idx < 0 || target.idx < 0 || origin.idx === target.idx) return null;
  return {
    space: origin.spaceKey,
    fromIdx: origin.idx,
    toIdx: target.idx,
  };
}

function reorderArrayCopy<T>(list: readonly T[], fromIdx: number, toIdx: number): T[] | null {
  if (fromIdx < 0 || toIdx < 0 || fromIdx >= list.length || toIdx >= list.length || fromIdx === toIdx) {
    return null;
  }
  const next = list.slice();
  const [moved] = next.splice(fromIdx, 1);
  next.splice(toIdx, 0, moved);
  return next;
}

/**
 * Optimistically reorders a Start space pin list in memory (supporting modular `config.spaces`,
 * Hero 0..1 <-> Compact 2..N, and legacy spaces) so pointer drag-and-drop feels instantaneous.
 */
export function applyOptimisticPinReorder(
  config: TestBarConfig | null,
  spaceKey: string,
  fromIdx: number,
  toIdx: number,
): TestBarConfig | null {
  if (!config) return null;
  if (config.spaces && config.spaces.length > 0) {
    const spIdx = config.spaces.findIndex((s) => s.id === spaceKey);
    if (spIdx >= 0) {
      const nextItems = reorderArrayCopy(config.spaces[spIdx].items, fromIdx, toIdx);
      if (!nextItems) return null;
      const nextSpaces = config.spaces.slice();
      nextSpaces[spIdx] = { ...config.spaces[spIdx], items: nextItems };
      return { ...config, spaces: nextSpaces };
    }
  }
  switch (spaceKey) {
    case 'taskbar': {
      const next = reorderArrayCopy(config.taskbarPins, fromIdx, toIdx);
      return next ? { ...config, taskbarPins: next } : null;
    }
    case 'personal': {
      const next = reorderArrayCopy(config.personalPins, fromIdx, toIdx);
      return next ? { ...config, personalPins: next } : null;
    }
    case 'personal_links': {
      const next = reorderArrayCopy(config.personalLinks, fromIdx, toIdx);
      return next ? { ...config, personalLinks: next } : null;
    }
    case 'work_folders': {
      const next = reorderArrayCopy(config.workFolders, fromIdx, toIdx);
      return next ? { ...config, workFolders: next } : null;
    }
    case 'work_apps': {
      const next = reorderArrayCopy(config.workApps, fromIdx, toIdx);
      return next ? { ...config, workApps: next } : null;
    }
    case 'studio': {
      const next = reorderArrayCopy(config.studioPins, fromIdx, toIdx);
      return next ? { ...config, studioPins: next } : null;
    }
    case 'studio_tools': {
      const next = reorderArrayCopy(config.studioTools, fromIdx, toIdx);
      return next ? { ...config, studioTools: next } : null;
    }
    case 'game_launchers': {
      const next = reorderArrayCopy(config.gameLaunchers, fromIdx, toIdx);
      return next ? { ...config, gameLaunchers: next } : null;
    }
    case 'game_combos': {
      const next = reorderArrayCopy(config.gameCombos, fromIdx, toIdx);
      return next ? { ...config, gameCombos: next } : null;
    }
    default: {
      const csIdx = config.customSpaces.findIndex((s) => s.id === spaceKey);
      if (csIdx < 0) return null;
      const nextItems = reorderArrayCopy(config.customSpaces[csIdx].items, fromIdx, toIdx);
      if (!nextItems) return null;
      const nextSpaces = config.customSpaces.slice();
      nextSpaces[csIdx] = { ...config.customSpaces[csIdx], items: nextItems };
      return { ...config, customSpaces: nextSpaces };
    }
  }
}

export interface MovePinInSpaceParams {
  space: string;
  fromId: string;
  toId?: string | null;
  targetSection?: TileSection | null;
}

function movePinByIdInList(
  list: readonly PinItem[],
  fromId: string,
  toId: string | null | undefined,
  targetSection: TileSection | null | undefined,
): PinItem[] | null {
  const fromIdx = list.findIndex((p) => p.id === fromId);
  if (fromIdx < 0) return null;

  const normSec: TileSection | undefined =
    targetSection === 'main' || targetSection === 'bottom' ? targetSection : undefined;

  if (toId) {
    const toIdx = list.findIndex((p) => p.id === toId);
    if (toIdx < 0) return null;
    const next = list.slice();
    const [moved] = next.splice(fromIdx, 1);
    const updatedItem: PinItem =
      normSec && moved.section !== normSec ? { ...moved, section: normSec } : moved;
    if (fromIdx === toIdx && updatedItem === moved) return null;
    next.splice(toIdx, 0, updatedItem);
    return next;
  }

  const next = list.slice();
  const [moved] = next.splice(fromIdx, 1);
  const updatedItem: PinItem =
    normSec && moved.section !== normSec ? { ...moved, section: normSec } : moved;

  let insertIdx = next.length;
  if (normSec === 'main') {
    let lastMain = -1;
    for (let i = 0; i < next.length; i++) {
      if (next[i].section !== 'bottom') lastMain = i;
    }
    insertIdx = lastMain >= 0 ? lastMain + 1 : 0;
  } else if (normSec === 'bottom') {
    let lastBottom = -1;
    for (let i = 0; i < next.length; i++) {
      if (next[i].section === 'bottom') lastBottom = i;
    }
    insertIdx = lastBottom >= 0 ? lastBottom + 1 : next.length;
  }
  next.splice(insertIdx, 0, updatedItem);

  const unchanged =
    next.length === list.length &&
    next.every((it, idx) => it.id === list[idx].id && it.section === list[idx].section);
  return unchanged ? null : next;
}

/**
 * Optimistically moves/reorders a pin by ID across Main <-> Bottom sections
 * (mirroring Rust `reorder_pins_by_id` semantics).
 */
export function applyOptimisticMovePinInSpace(
  config: TestBarConfig | null,
  params: MovePinInSpaceParams,
): TestBarConfig | null {
  if (!config || !params.fromId) return null;
  if (config.spaces && config.spaces.length > 0) {
    const spIdx = config.spaces.findIndex((s) => s.id === params.space);
    if (spIdx >= 0) {
      const nextItems = movePinByIdInList(
        config.spaces[spIdx].items,
        params.fromId,
        params.toId,
        params.targetSection,
      );
      if (!nextItems) return null;
      const nextSpaces = config.spaces.slice();
      nextSpaces[spIdx] = { ...config.spaces[spIdx], items: nextItems };
      return { ...config, spaces: nextSpaces };
    }
  }
  if (params.space === 'game_combos') {
    const next = movePinByIdInList(
      config.gameCombos,
      params.fromId,
      params.toId,
      params.targetSection,
    );
    return next ? { ...config, gameCombos: next } : null;
  }
  const csIdx = config.customSpaces.findIndex((s) => s.id === params.space);
  if (csIdx >= 0) {
    const nextItems = movePinByIdInList(
      config.customSpaces[csIdx].items,
      params.fromId,
      params.toId,
      params.targetSection,
    );
    if (!nextItems) return null;
    const nextSpaces = config.customSpaces.slice();
    nextSpaces[csIdx] = { ...config.customSpaces[csIdx], items: nextItems };
    return { ...config, customSpaces: nextSpaces };
  }
  return null;
}

export function findMatchingInstalledAppForPin(
  pin: PinItem,
  apps: readonly IndexedApp[],
): IndexedApp | undefined {
  if (apps.length === 0) return undefined;
  const pTarget = normTarget(pin.target);
  const pBase = pTarget && !pTarget.endsWith('\\') ? pTarget.split('\\').pop() ?? '' : '';
  const pAumid = normAumid(pin.matchAumid || (pTarget.startsWith('shell:appsfolder\\') ? pTarget : ''));
  const pMatchExe = (pin.matchExe ?? '').trim().toLowerCase();
  const pTitle = pin.title.trim().toLowerCase();

  if (pAumid) {
    const byAumid = apps.find((a) => {
      const aAumid = normAumid(a.aumid || (normTarget(a.target).startsWith('shell:appsfolder\\') ? a.target : ''));
      return Boolean(aAumid) && aAumid === pAumid;
    });
    if (byAumid) return byAumid;
  }
  if (pTarget) {
    const byTarget = apps.find((a) => normTarget(a.target) === pTarget);
    if (byTarget) return byTarget;
  }
  if (pMatchExe || (pBase && pBase.endsWith('.exe'))) {
    const wantedExe = pMatchExe || pBase;
    const byExe = apps.find((a) => {
      const aTarget = normTarget(a.target);
      return aTarget.endsWith('\\' + wantedExe) || aTarget === wantedExe;
    });
    if (byExe) return byExe;
  }
  if (pTitle) {
    const byName = apps.find((a) => a.name.trim().toLowerCase() === pTitle);
    if (byName) return byName;
  }
  return undefined;
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
  const baseAsAdmin = item.kind === 'app' && Boolean(item.runAsAdmin) && canRunAsAdmin(item.target);

  if (!shiftHeld) {
    return {
      type: 'launch',
      target: item.target,
      args: baseArgs,
      asAdmin: baseAsAdmin,
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
      asAdmin: baseAsAdmin,
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
      asAdmin: baseAsAdmin,
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

  // 6. Plain launch fallback
  return {
    type: 'launch',
    target: item.target,
    args: baseArgs,
    asAdmin: baseAsAdmin,
    closeStart: true,
  };
}

/**
 * Returns the displayed Shift badge label ONLY when `resolvePinAction(item, true)` produces a
 * genuinely distinct real action (different type, elevated UAC, or distinct CLI flag).
 * Eliminates deceptive/decorative Shift labels on tiles whose fallback is an identical launch.
 */
export function getHonestShiftBadge(item: PinItem): string | null {
  if (item.kind === 'combo') return null;
  const label = (item.altLabel?.trim() || item.altAction?.trim() || '');
  if (!label) return null;

  const base = resolvePinAction(item, false);
  const alt = resolvePinAction(item, true);

  if (alt.type === 'copy_path' || alt.type === 'terminal') {
    return label;
  }

  if (alt.type === 'launch' && base.type === 'launch') {
    const distinctAdmin = alt.asAdmin && !base.asAdmin;
    const distinctArgs = (alt.args ?? null) !== (base.args ?? null);
    if (distinctAdmin || distinctArgs) {
      return label;
    }
  }

  return null;
}

export function hasHonestShiftAction(item: PinItem): boolean {
  return getHonestShiftBadge(item) !== null;
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

// ===================== EPIC 5 + V2: TELEMETRY, SPARKLINES, SAFE KILL, CLIPBOARD, GAMES =====================

export function getBatteryColor(percent: number): string {
  if (percent > 30) return '#34D399';
  if (percent > 15) return '#F59E0B';
  return '#FB7185';
}

export function getTelemetryStatusPill(pct: number): {
  cls: 'st-ok' | 'st-mid' | 'st-hot';
  label: string;
} {
  if (pct < 45) return { cls: 'st-ok', label: 'Свободно' };
  if (pct < 75) return { cls: 'st-mid', label: 'Норма' };
  return { cls: 'st-hot', label: 'Высокая' };
}

export function getTelemetryLevelClass(pct: number): '' | 'lvl-warn' | 'lvl-hot' {
  if (pct >= 90) return 'lvl-hot';
  if (pct >= 75) return 'lvl-warn';
  return '';
}

export function getGpuTempStatusPill(tempC: number): {
  cls: 'st-ok' | 'st-mid' | 'st-hot';
  label: string;
} {
  if (tempC < 60) return { cls: 'st-ok', label: 'Холодная' };
  if (tempC < 78) return { cls: 'st-mid', label: 'Рабочая' };
  return { cls: 'st-hot', label: 'Горячая' };
}

export function formatGbValue(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '0';
  const rounded = Math.round(value * 10) / 10;
  return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
}

/**
 * Formats strictly single-line memory usage (e.g. `18.4 / 32 ГБ`) with zero raw RPM/MHz numbers.
 */
export function formatMemoryRatio(usedGb: number, totalGb: number): string {
  return `${formatGbValue(usedGb)} / ${formatGbValue(totalGb)} ГБ`;
}

export function formatDiskSummary(disk: Pick<DiskHeadroomInfo, 'usedPct' | 'freeGb'>): string {
  const safeFree = Number.isFinite(disk.freeGb) ? Math.max(0, disk.freeGb) : 0;
  return `${safeFree.toFixed(0)} ГБ своб.`;
}

/**
 * Converts a 0..100 UI volume slider percentage into a strictly normalized 0.0..=1.0 WASAPI scalar.
 */
export function normalizeSessionVolumeScalar(volumePct: number): number {
  if (!Number.isFinite(volumePct)) return 0;
  return clampNum(volumePct, 0, 100) / 100;
}

export const TELEMETRY_HISTORY_CAP = 30;

export interface TelemetryHistory {
  cpu: number[];
  ram: number[];
  gpu: number[];
  vram: number[];
}

export const EMPTY_TELEMETRY_HISTORY: TelemetryHistory = {
  cpu: [],
  ram: [],
  gpu: [],
  vram: [],
};

function clampNum(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

function round2(v: number): number {
  const r = Math.round(v * 100) / 100;
  return Object.is(r, -0) ? 0 : r;
}

export function pushTelemetrySample(
  series: readonly number[],
  value: number,
  cap = TELEMETRY_HISTORY_CAP,
): number[] {
  const safeCap = Math.max(1, Math.floor(cap));
  const safeVal = Number.isFinite(value) ? clampNum(Math.round(value), 0, 100) : 0;
  const next = [...series, safeVal];
  return next.length > safeCap ? next.slice(next.length - safeCap) : next;
}

export function pushTelemetryHistory(
  history: TelemetryHistory,
  sample: {
    cpuPct: number;
    ramPct: number;
    gpuPct?: number;
    vramPct?: number;
    gpuAvailable?: boolean;
  },
  cap = TELEMETRY_HISTORY_CAP,
): TelemetryHistory {
  return {
    cpu: pushTelemetrySample(history.cpu, sample.cpuPct, cap),
    ram: pushTelemetrySample(history.ram, sample.ramPct, cap),
    gpu: sample.gpuAvailable === false
      ? history.gpu
      : pushTelemetrySample(history.gpu, sample.gpuPct ?? 0, cap),
    vram: sample.gpuAvailable === false
      ? history.vram
      : pushTelemetrySample(history.vram, sample.vramPct ?? 0, cap),
  };
}

export interface SparklineGeometry {
  linePath: string;
  areaPath: string;
}

/**
 * Generates crisp SVG polyline + soft area paths from a rolling 0..100 percentage buffer.
 */
export function buildSparklinePath(
  values: readonly number[],
  width = 56,
  height = 16,
): SparklineGeometry {
  if (values.length === 0 || !Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return { linePath: '', areaPath: '' };
  }

  const padY = 1.5;
  const usableH = Math.max(1, height - padY * 2);
  const toY = (v: number) => {
    const clamped = Number.isFinite(v) ? clampNum(v, 0, 100) : 0;
    return round2(height - padY - (clamped / 100) * usableH);
  };

  if (values.length === 1) {
    const y = toY(values[0]);
    const w = round2(width);
    const h = round2(height);
    return {
      linePath: `M 0 ${y} L ${w} ${y}`,
      areaPath: `M 0 ${y} L ${w} ${y} L ${w} ${h} L 0 ${h} Z`,
    };
  }

  const stepX = width / (values.length - 1);
  const coords = values.map((v, idx) => ({
    x: round2(idx * stepX),
    y: toY(v),
  }));

  const lineParts = coords.map((pt, idx) => `${idx === 0 ? 'M' : 'L'} ${pt.x} ${pt.y}`);
  const linePath = lineParts.join(' ');
  const lastX = coords[coords.length - 1].x;
  const h = round2(height);
  const areaPath = `${linePath} L ${lastX} ${h} L 0 ${h} Z`;

  return { linePath, areaPath };
}

export interface ProcessMetricDisplay {
  primary: string;
  gpuBadge: string | null;
}

/**
 * Formats strictly single-line process memory / GPU VRAM readouts for Work and Studio cards.
 */
export function formatProcessMetricLine(
  proc: Pick<TopProcessInfo, 'memGb' | 'vramGb' | 'usesGpu'>,
  mode: 'ram' | 'gpu',
): ProcessMetricDisplay {
  const ramLabel = `${formatGbValue(proc.memGb)} ГБ`;
  if (mode === 'ram') {
    return { primary: ramLabel, gpuBadge: null };
  }
  if (Number.isFinite(proc.vramGb) && proc.vramGb > 0) {
    return {
      primary: ramLabel,
      gpuBadge: `VRAM ${formatGbValue(proc.vramGb)} ГБ`,
    };
  }
  if (proc.usesGpu) {
    return {
      primary: ramLabel,
      gpuBadge: 'GPU',
    };
  }
  return { primary: ramLabel, gpuBadge: null };
}

export function resolveProcessKillPids(proc: Pick<TopProcessInfo, 'pid' | 'pids'>): number[] {
  const source = Array.isArray(proc.pids) && proc.pids.length > 0 ? proc.pids : [proc.pid];
  const unique: number[] = [];
  const seen = new Set<number>();
  for (const raw of source) {
    if (Number.isInteger(raw) && raw > 0 && !seen.has(raw)) {
      seen.add(raw);
      unique.push(raw);
    }
  }
  return unique;
}

export const PROCESS_KILL_ARM_MS = 2500;

export interface ProcessKillClickResult {
  execute: boolean;
  nextArmedKey: string | null;
}

/**
 * 2-click armed process-kill state machine: prevents accidental single-click data loss
 * matching Power capsule's error-prevention grammar.
 */
export function reduceProcessKillClick(
  armedKey: string | null,
  targetKey: string,
): ProcessKillClickResult {
  if (!targetKey) {
    return { execute: false, nextArmedKey: null };
  }
  if (armedKey === targetKey) {
    return { execute: true, nextArmedKey: null };
  }
  return { execute: false, nextArmedKey: targetKey };
}

export function formatKillProcessError(procName: string, err: unknown): string {
  const raw =
    typeof err === 'string'
      ? err.trim()
      : err instanceof Error
        ? err.message.trim()
        : String(err ?? '').trim();
  const cleanName = procName.trim() || 'процесс';
  if (!raw) {
    return `Не удалось завершить ${cleanName}`;
  }
  const low = raw.toLowerCase();
  if (low.includes('отказано в доступе') || low.includes('access is denied') || low.includes('access denied')) {
    return `Отказано в доступе: ${cleanName} требует прав администратора`;
  }
  return raw;
}

/**
 * Converts a `#RGB`, `#RRGGBB`, or `#RRGGBBAA` color string into canonical CSS `rgb(...)` / `rgba(...)`.
 */
export function hexColorToRgbString(raw: string | null | undefined): string | null {
  const trimmed = (raw ?? '').trim();
  if (!trimmed) return null;
  const hex = trimmed.startsWith('#') ? trimmed.slice(1) : trimmed;
  if (!/^[0-9a-fA-F]+$/.test(hex)) return null;

  let r = 0;
  let g = 0;
  let b = 0;
  let a: number | null = null;

  if (hex.length === 3) {
    r = parseInt(hex[0] + hex[0], 16);
    g = parseInt(hex[1] + hex[1], 16);
    b = parseInt(hex[2] + hex[2], 16);
  } else if (hex.length === 6) {
    r = parseInt(hex.slice(0, 2), 16);
    g = parseInt(hex.slice(2, 4), 16);
    b = parseInt(hex.slice(4, 6), 16);
  } else if (hex.length === 8) {
    r = parseInt(hex.slice(0, 2), 16);
    g = parseInt(hex.slice(2, 4), 16);
    b = parseInt(hex.slice(4, 6), 16);
    const alphaByte = parseInt(hex.slice(6, 8), 16);
    if (alphaByte < 255) {
      a = Math.round((alphaByte / 255) * 100) / 100;
    }
  } else {
    return null;
  }

  return a !== null ? `rgba(${r}, ${g}, ${b}, ${a})` : `rgb(${r}, ${g}, ${b})`;
}

export function resolveClipboardKind(
  entry: Pick<ClipboardEntry, 'kind' | 'tag' | 'color' | 'text'>,
): ClipboardKind {
  const k = (entry.kind ?? '').toString().trim().toLowerCase();
  if (k === 'url' || k === 'path' || k === 'hex' || k === 'code' || k === 'text') {
    return k;
  }
  if (entry.color || hexColorToRgbString(entry.text)) {
    return 'hex';
  }
  const tagLow = (entry.tag ?? '').trim().toLowerCase();
  if (tagLow === 'url') return 'url';
  if (tagLow === 'path') return 'path';
  if (tagLow === 'code') return 'code';
  if (tagLow === 'hex') return 'hex';
  return 'text';
}

export function getClipboardKindLabel(kind: ClipboardKind): string {
  switch (kind) {
    case 'url':
      return 'URL';
    case 'path':
      return 'PATH';
    case 'hex':
      return 'HEX';
    case 'code':
      return 'CODE';
    default:
      return 'TXT';
  }
}

export function filterClipboardEntries(
  entries: readonly ClipboardEntry[],
  rawQuery: string,
): ClipboardEntry[] {
  const q = rawQuery.trim().toLowerCase();
  if (!q) return [...entries];
  return entries.filter((c) => {
    const hay = `${c.text} ${c.preview ?? ''} ${c.tag} ${c.kind ?? ''}`.toLowerCase();
    return hay.includes(q);
  });
}

export function computeInterpolatedMediaSec(
  media: Pick<MediaState, 'positionSec' | 'durationSec' | 'isPlaying' | 'lastUpdatedMs'> | null,
  nowMs: number,
): number {
  if (!media) return 0;
  const base = Number.isFinite(media.positionSec) ? Math.max(0, media.positionSec) : 0;
  const max = Number.isFinite(media.durationSec) && media.durationSec > 0 ? media.durationSec : Infinity;

  if (!media.isPlaying || !media.lastUpdatedMs || !Number.isFinite(media.lastUpdatedMs)) {
    return Math.min(base, max);
  }
  const elapsed = Math.max(0, (nowMs - media.lastUpdatedMs) / 1000);
  return Math.min(base + elapsed, max);
}

export function formatMediaDuration(rawSec: number): string {
  const total = Math.max(0, Math.floor(Number.isFinite(rawSec) ? rawSec : 0));
  const hrs = Math.floor(total / 3600);
  const mins = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  if (hrs > 0) {
    return `${hrs}:${String(mins).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
  }
  return `${mins}:${String(secs).padStart(2, '0')}`;
}

export type GameLibraryFilter = 'recent' | 'all';
export const RECENT_GAMES_LIMIT = 4;

export function selectDisplayedGames<T>(games: readonly T[], filter: GameLibraryFilter): T[] {
  if (filter === 'recent') {
    return games.slice(0, RECENT_GAMES_LIMIT);
  }
  return [...games];
}

export const GAME_CARD_MAX_TILT_DEG = 4.5;

export interface GameCardTilt {
  gxPct: number;
  gyPct: number;
  rotateXDeg: number;
  rotateYDeg: number;
}

/**
 * Computes pointer-tracked 3D hover tilt angles (`perspective(900px) rotateX(...) rotateY(...)`)
 * clamped strictly to `[-maxDeg, +maxDeg]` (default `±4.5°` for zero Chromium text blur)
 * and zeroed cleanly under `prefers-reduced-motion`.
 */
export function computeGameCardTilt(
  clientX: number,
  clientY: number,
  rect: { left: number; top: number; width: number; height: number },
  reducedMotion = false,
  maxDeg = GAME_CARD_MAX_TILT_DEG,
): GameCardTilt {
  if (
    reducedMotion ||
    !Number.isFinite(clientX) ||
    !Number.isFinite(clientY) ||
    !Number.isFinite(rect.width) ||
    !Number.isFinite(rect.height) ||
    rect.width <= 0 ||
    rect.height <= 0
  ) {
    return { gxPct: 50, gyPct: 50, rotateXDeg: 0, rotateYDeg: 0 };
  }

  const clampedMax = Math.max(0, Math.min(20, Number.isFinite(maxDeg) ? maxDeg : GAME_CARD_MAX_TILT_DEG));
  const nx = clampNum((clientX - rect.left) / rect.width, 0, 1);
  const ny = clampNum((clientY - rect.top) / rect.height, 0, 1);

  const rotateXDeg = round2(clampNum((0.5 - ny) * 2 * clampedMax, -clampedMax, clampedMax));
  const rotateYDeg = round2(clampNum((nx - 0.5) * 2 * clampedMax, -clampedMax, clampedMax));

  return {
    gxPct: round2(nx * 100),
    gyPct: round2(ny * 100),
    rotateXDeg,
    rotateYDeg,
  };
}

export function buildGameComboLaunchPayload(
  item: Pick<PinItem, 'kind' | 'target' | 'comboTargets'>,
): { command: 'launch_combo'; args: { targets: string[] } } | null {
  if (item.kind !== 'combo') return null;
  const rawTargets =
    Array.isArray(item.comboTargets) && item.comboTargets.length > 0
      ? item.comboTargets
      : item.target
        ? [item.target]
        : [];
  const targets = rawTargets.map((t) => t.trim()).filter(Boolean);
  if (targets.length === 0) return null;
  return {
    command: 'launch_combo',
    args: { targets },
  };
}

export function validateGameComboInput(
  rawName: string,
  rawSteps: readonly string[],
): { ok: true; name: string; steps: string[] } | { ok: false; error: string } {
  const name = rawName.trim().replace(/\s+/g, ' ');
  if (!name) {
    return { ok: false, error: 'Введите название связки' };
  }
  if (name.length > 36) {
    return { ok: false, error: 'Максимум 36 символов' };
  }
  const steps = rawSteps.map((s) => s.trim()).filter(Boolean);
  if (steps.length < 2) {
    return { ok: false, error: 'Добавьте минимум 2 цели для связки' };
  }
  return { ok: true, name, steps };
}

// ===================== WAVE 3B: MODULAR SPACES, CUSTOM TILES & WIDGET BUILDER =====================

export function normalizeRgbTriplet(raw: string | null | undefined): string | null {
  const trimmed = (raw ?? '').trim();
  if (!trimmed) return null;
  const parts = trimmed.split(',').map((p) => Number.parseInt(p.trim(), 10));
  if (parts.length !== 3 || parts.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) {
    return null;
  }
  return `${parts[0]}, ${parts[1]}, ${parts[2]}`;
}

/**
 * Resolves a pin's `customColor` (`#RGB`, `#RRGGBB`, or `r,g,b`) into a CSS RGB triplet (`r, g, b`).
 */
export function resolveCustomColorRgb(raw: string | null | undefined): string | null {
  const trimmed = (raw ?? '').trim();
  if (!trimmed) return null;
  const asTriplet = normalizeRgbTriplet(trimmed);
  if (asTriplet) return asTriplet;

  const hex = trimmed.startsWith('#') ? trimmed.slice(1) : trimmed;
  if (!/^[0-9a-fA-F]+$/.test(hex)) return null;
  if (hex.length === 3) {
    const r = Number.parseInt(hex[0] + hex[0], 16);
    const g = Number.parseInt(hex[1] + hex[1], 16);
    const b = Number.parseInt(hex[2] + hex[2], 16);
    return `${r}, ${g}, ${b}`;
  }
  if (hex.length === 6) {
    const r = Number.parseInt(hex.slice(0, 2), 16);
    const g = Number.parseInt(hex.slice(2, 4), 16);
    const b = Number.parseInt(hex.slice(4, 6), 16);
    return `${r}, ${g}, ${b}`;
  }
  return null;
}

export function toggleSpaceWidget(
  current: readonly SpaceWidgetId[],
  widgetId: SpaceWidgetId,
): SpaceWidgetId[] {
  if (current.includes(widgetId)) {
    return current.filter((w) => w !== widgetId);
  }
  return [...current, widgetId];
}

export function moveSpaceWidget(
  current: readonly SpaceWidgetId[],
  widgetId: SpaceWidgetId,
  direction: 'up' | 'down',
): SpaceWidgetId[] {
  const idx = current.indexOf(widgetId);
  if (idx < 0) return [...current];
  const targetIdx = direction === 'up' ? idx - 1 : idx + 1;
  if (targetIdx < 0 || targetIdx >= current.length) return [...current];
  const next = current.slice();
  const [moved] = next.splice(idx, 1);
  next.splice(targetIdx, 0, moved);
  return next;
}

export function moveSpaceIdInOrder(
  orderedIds: readonly string[],
  spaceId: string,
  direction: 'left' | 'right',
): string[] | null {
  const idx = orderedIds.indexOf(spaceId);
  if (idx < 0) return null;
  const targetIdx = direction === 'left' ? idx - 1 : idx + 1;
  if (targetIdx < 0 || targetIdx >= orderedIds.length) return null;
  const next = orderedIds.slice();
  const [moved] = next.splice(idx, 1);
  next.splice(targetIdx, 0, moved);
  return next;
}

export function isLowBatteryPercent(percent: number): boolean {
  return Number.isFinite(percent) && percent < 20;
}

export function formatPeripheralSourceLabel(source: string | null | undefined): string {
  const s = (source ?? '').trim().toLowerCase();
  if (!s || s === 'bluetooth' || s === 'bt') return 'Bluetooth';
  if (s.includes('razer')) return 'Razer HID';
  if (s.includes('logitech') || s.includes('hidpp')) return 'Logitech HID';
  if (s.includes('xinput')) return 'XInput';
  if (s.includes('usb') || s.includes('hid')) return 'USB HID';
  return source?.trim() || 'Беспроводное';
}

export function convertTempFromC(tempC: number, units: 'c' | 'f' = 'c'): number {
  if (!Number.isFinite(tempC)) return 0;
  return units === 'f' ? (tempC * 9) / 5 + 32 : tempC;
}

export function formatStartTemperature(
  tempC: number,
  units: 'c' | 'f' = 'c',
  withUnit = true,
): string {
  if (!Number.isFinite(tempC)) {
    return withUnit ? `—°${units === 'f' ? 'F' : 'C'}` : '—°';
  }
  const rounded = Math.round(convertTempFromC(tempC, units));
  const val = Object.is(rounded, -0) ? 0 : rounded;
  if (!withUnit) return `${val}°`;
  return `${val}°${units === 'f' ? 'F' : 'C'}`;
}

/**
 * Formats dynamic hardware summary in PC Health header strictly from live `TelemetryState`.
 */
export function formatHardwareSummaryLine(
  telem: Pick<TelemetryState, 'ramTotalGb' | 'gpuAvailable' | 'gpuName' | 'vramTotalGb'> | null,
): string {
  if (!telem) return 'Система';
  const parts: string[] = [];
  if (Number.isFinite(telem.ramTotalGb) && telem.ramTotalGb > 0) {
    parts.push(`${Math.round(telem.ramTotalGb)} ГБ ОЗУ`);
  }
  if (telem.gpuAvailable && telem.gpuName?.trim()) {
    const vramPart =
      Number.isFinite(telem.vramTotalGb) && telem.vramTotalGb > 0
        ? ` · ${formatGbValue(telem.vramTotalGb)} ГБ`
        : '';
    parts.push(`${telem.gpuName.trim()}${vramPart}`);
  }
  return parts.length > 0 ? parts.join(' · ') : 'Система';
}

// ===================== WAVE 5B: BATTERY HIDING & EXPLORER 3-MIN RESUME =====================

export function filterVisibleBatteries<T extends Pick<PeripheralBattery, 'id'>>(
  periphs: readonly T[],
  hiddenBatteryIds?: readonly string[] | null,
): T[] {
  if (!Array.isArray(periphs) || periphs.length === 0) {
    return [];
  }
  if (!Array.isArray(hiddenBatteryIds) || hiddenBatteryIds.length === 0) {
    return [...periphs];
  }
  const hiddenSet = new Set(
    hiddenBatteryIds.map((id) => id.trim().toLowerCase()).filter(Boolean),
  );
  if (hiddenSet.size === 0) {
    return [...periphs];
  }
  return periphs.filter((p) => !hiddenSet.has(p.id.trim().toLowerCase()));
}

export const EXPLORER_RESUME_TTL_MS = 180_000;

export interface ExplorerSessionSnapshot {
  path: string;
  viewMode: 'grid' | 'list';
  sortBy: 'name' | 'date' | 'size' | 'type';
  sortDir: 'asc' | 'desc';
  scrollTop: number;
  selectedPath: string | null;
}

export function shouldResetExplorerOnReopen(
  closedAtMs: number | null | undefined,
  nowMs: number,
  ttlMs: number = EXPLORER_RESUME_TTL_MS,
): boolean {
  if (closedAtMs === null || closedAtMs === undefined || !Number.isFinite(closedAtMs)) {
    return false;
  }
  if (!Number.isFinite(nowMs) || !Number.isFinite(ttlMs) || ttlMs <= 0) {
    return true;
  }
  return nowMs - closedAtMs >= ttlMs;
}

// ===================== V3.2 DUAL-MODE HEADER & SIDE WIDGET PLANNING =====================

export const HEADER_WIDGET_GAP_PX = 6;
export const HEADER_OVERFLOW_BADGE_PX = 38;
export const DEFAULT_HEADER_WIDGET_WIDTH_PX = 92;

export type SpaceWidgetPlacement = 'off' | 'top' | 'side';

export interface SpaceWidgetLayoutState {
  widgets: SpaceWidgetId[];
  headerWidgets: SpaceHeaderWidget[];
}

export interface PlannedHeaderWidgetSlots {
  left: SpaceHeaderWidget[];
  right: SpaceHeaderWidget[];
  overflow: SpaceHeaderWidget[];
  overflowSide: 'left' | 'right' | null;
  leftUsedPx: number;
  rightUsedPx: number;
}

function normalizeHeaderSide(side: unknown): HeaderWidgetSide {
  return side === 'left' || side === 'right' || side === 'auto' ? side : 'auto';
}

/**
 * Resolves a Space's top-header widget list, preserving explicit empty arrays in v4
 * while falling back to `batteries` + `weather` on pre-v4 configs without `headerWidgets`.
 */
export function resolveSpaceHeaderWidgets(
  space: Pick<SpaceConfig, 'widgets' | 'headerWidgets'> | null | undefined,
): SpaceHeaderWidget[] {
  const sideSet = new Set<SpaceWidgetId>(space?.widgets ?? []);
  if (Array.isArray(space?.headerWidgets)) {
    const out: SpaceHeaderWidget[] = [];
    const seen = new Set<SpaceWidgetId>();
    for (const entry of space.headerWidgets) {
      if (!entry || !entry.id || sideSet.has(entry.id) || seen.has(entry.id)) continue;
      seen.add(entry.id);
      out.push({ id: entry.id, side: normalizeHeaderSide(entry.side) });
    }
    return out;
  }
  const fallbackIds: SpaceWidgetId[] = ['batteries', 'weather'];
  return fallbackIds
    .filter((id) => !sideSet.has(id))
    .map((id) => ({ id, side: 'left' as const }));
}

export function getSpaceWidgetPlacement(
  widgets: readonly SpaceWidgetId[],
  headerWidgets: readonly SpaceHeaderWidget[],
  widgetId: SpaceWidgetId,
): { placement: SpaceWidgetPlacement; side: HeaderWidgetSide } {
  if (widgets.includes(widgetId)) {
    return { placement: 'side', side: 'auto' };
  }
  const headerEntry = headerWidgets.find((h) => h.id === widgetId);
  if (headerEntry) {
    return { placement: 'top', side: normalizeHeaderSide(headerEntry.side) };
  }
  return { placement: 'off', side: 'auto' };
}

/**
 * Assigns a widget exclusively to `'off'`, `'top'` (header), or `'side'` (right rail),
 * enforcing mutual exclusivity between `widgets` and `headerWidgets`.
 */
export function setSpaceWidgetPlacement(
  widgets: readonly SpaceWidgetId[],
  headerWidgets: readonly SpaceHeaderWidget[],
  widgetId: SpaceWidgetId,
  placement: SpaceWidgetPlacement,
  side: HeaderWidgetSide = 'auto',
): SpaceWidgetLayoutState {
  const cleanSide = normalizeHeaderSide(side);
  const withoutSide = widgets.filter((w) => w !== widgetId);
  const withoutHeader = headerWidgets.filter((h) => h.id !== widgetId);

  if (placement === 'off') {
    return {
      widgets: withoutSide,
      headerWidgets: withoutHeader,
    };
  }

  if (placement === 'side') {
    return {
      widgets: widgets.includes(widgetId) ? [...widgets] : [...withoutSide, widgetId],
      headerWidgets: withoutHeader,
    };
  }

  const existingIdx = headerWidgets.findIndex((h) => h.id === widgetId);
  if (existingIdx >= 0) {
    const nextHeader = headerWidgets.map((h, idx) =>
      idx === existingIdx ? { ...h, side: cleanSide } : h,
    );
    return {
      widgets: withoutSide,
      headerWidgets: nextHeader,
    };
  }

  return {
    widgets: withoutSide,
    headerWidgets: [...withoutHeader, { id: widgetId, side: cleanSide }],
  };
}

export function setHeaderWidgetSide(
  headerWidgets: readonly SpaceHeaderWidget[],
  widgetId: SpaceWidgetId,
  side: HeaderWidgetSide,
): SpaceHeaderWidget[] {
  const cleanSide = normalizeHeaderSide(side);
  return headerWidgets.map((entry) =>
    entry.id === widgetId ? { ...entry, side: cleanSide } : entry,
  );
}

export function moveHeaderWidget(
  headerWidgets: readonly SpaceHeaderWidget[],
  widgetId: SpaceWidgetId,
  direction: 'up' | 'down',
): SpaceHeaderWidget[] {
  const idx = headerWidgets.findIndex((h) => h.id === widgetId);
  if (idx < 0) return [...headerWidgets];
  const targetIdx = direction === 'up' ? idx - 1 : idx + 1;
  if (targetIdx < 0 || targetIdx >= headerWidgets.length) return [...headerWidgets];
  const next = headerWidgets.slice();
  const [moved] = next.splice(idx, 1);
  next.splice(targetIdx, 0, moved);
  return next;
}

/**
 * Allocates compact header pills into left and right pockets without overflowing either pocket.
 * Honors explicit `'left'` / `'right'` sides and `'auto'` (left-first then right), reserving
 * `38px` for the `+N` badge whenever any widget overflows.
 */
export function planHeaderWidgetSlots(
  entries: readonly SpaceHeaderWidget[],
  leftAvailPx: number,
  rightAvailPx: number,
  widthMap: Partial<Record<SpaceWidgetId, number>> = {},
): PlannedHeaderWidgetSlots {
  const safeLeft = Number.isFinite(leftAvailPx) ? Math.max(0, Math.floor(leftAvailPx)) : 0;
  const safeRight = Number.isFinite(rightAvailPx) ? Math.max(0, Math.floor(rightAvailPx)) : 0;

  const deduped: SpaceHeaderWidget[] = [];
  const seen = new Set<SpaceWidgetId>();
  for (const entry of entries) {
    if (!entry || !entry.id || seen.has(entry.id)) continue;
    seen.add(entry.id);
    deduped.push({ id: entry.id, side: normalizeHeaderSide(entry.side) });
  }

  if (deduped.length === 0) {
    return {
      left: [],
      right: [],
      overflow: [],
      overflowSide: null,
      leftUsedPx: 0,
      rightUsedPx: 0,
    };
  }

  const getPillWidth = (id: SpaceWidgetId): number => {
    const raw = widthMap[id];
    return typeof raw === 'number' && Number.isFinite(raw) && raw > 0
      ? Math.ceil(raw)
      : DEFAULT_HEADER_WIDGET_WIDTH_PX;
  };

  const runPass = (badgeSide: 'left' | 'right' | null): PlannedHeaderWidgetSlots => {
    const left: SpaceHeaderWidget[] = [];
    const right: SpaceHeaderWidget[] = [];
    const overflow: SpaceHeaderWidget[] = [];
    let leftPillsPx = 0;
    let rightPillsPx = 0;

    const badgeTotalForPocket = (side: 'left' | 'right', pillCount: number, pillsPx: number) => {
      if (badgeSide !== side) return pillsPx;
      return pillCount === 0
        ? HEADER_OVERFLOW_BADGE_PX
        : pillsPx + HEADER_WIDGET_GAP_PX + HEADER_OVERFLOW_BADGE_PX;
    };

    const tryPushLeft = (entry: SpaceHeaderWidget): boolean => {
      const w = getPillWidth(entry.id);
      const nextPillsPx = left.length === 0 ? w : leftPillsPx + HEADER_WIDGET_GAP_PX + w;
      const totalNeeded = badgeTotalForPocket('left', left.length + 1, nextPillsPx);
      if (totalNeeded <= safeLeft) {
        left.push(entry);
        leftPillsPx = nextPillsPx;
        return true;
      }
      return false;
    };

    const tryPushRight = (entry: SpaceHeaderWidget): boolean => {
      const w = getPillWidth(entry.id);
      const nextPillsPx = right.length === 0 ? w : rightPillsPx + HEADER_WIDGET_GAP_PX + w;
      const totalNeeded = badgeTotalForPocket('right', right.length + 1, nextPillsPx);
      if (totalNeeded <= safeRight) {
        right.push(entry);
        rightPillsPx = nextPillsPx;
        return true;
      }
      return false;
    };

    for (const entry of deduped) {
      if (entry.side === 'left') {
        if (!tryPushLeft(entry)) overflow.push(entry);
      } else if (entry.side === 'right') {
        if (!tryPushRight(entry)) overflow.push(entry);
      } else if (!tryPushLeft(entry) && !tryPushRight(entry)) {
        overflow.push(entry);
      }
    }

    const leftUsedPx =
      badgeSide === 'left'
        ? badgeTotalForPocket('left', left.length, leftPillsPx)
        : leftPillsPx;
    const rightUsedPx =
      badgeSide === 'right'
        ? badgeTotalForPocket('right', right.length, rightPillsPx)
        : rightPillsPx;

    return {
      left,
      right,
      overflow,
      overflowSide: overflow.length > 0 ? badgeSide : null,
      leftUsedPx,
      rightUsedPx,
    };
  };

  const pass1 = runPass(null);
  if (pass1.overflow.length === 0) {
    return pass1;
  }

  const preferRight = pass1.overflow.some((e) => e.side === 'right' || e.side === 'auto');
  let badgeSide: 'left' | 'right' | null = null;
  if (preferRight) {
    if (safeRight >= HEADER_OVERFLOW_BADGE_PX) {
      badgeSide = 'right';
    } else if (safeLeft >= HEADER_OVERFLOW_BADGE_PX) {
      badgeSide = 'left';
    }
  } else if (safeLeft >= HEADER_OVERFLOW_BADGE_PX) {
    badgeSide = 'left';
  } else if (safeRight >= HEADER_OVERFLOW_BADGE_PX) {
    badgeSide = 'right';
  }

  return runPass(badgeSide);
}
