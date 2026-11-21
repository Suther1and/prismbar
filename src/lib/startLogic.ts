import { canRunAsAdmin } from './flyoutLogic';
import type {
  ClipboardEntry,
  ClipboardKind,
  DiskHeadroomInfo,
  IndexedApp,
  MediaState,
  PinItem,
  TestBarConfig,
  TopProcessInfo,
} from './ipc';

export type StartAnimPhase = 'hidden' | 'entering' | 'open' | 'closing';

export interface StartAnimState {
  phase: StartAnimPhase;
  open: boolean;
  closing: boolean;
  openSeq: number;
  gen: number;
}

export const START_EXIT_ANIM_MS = 115;

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
 * Transitions Start Menu animation phase without unmounting DOM:
 * - `open: true` -> `'entering'` (with incremented `openSeq` and synced `gen`)
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
      phase: 'entering',
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
 * Promotes an `'entering'` Start Menu surface to `'open'` on the first awake rAF
 * after `surface_commit_ack` + `ShowWindow`.
 */
export function promoteStartEntering(state: StartAnimState): StartAnimState {
  if (state.phase !== 'entering') {
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

export interface SpaceAccentOption {
  hex: string;
  rgb: string;
  label: string;
}

export const CUSTOM_SPACE_ACCENTS: readonly SpaceAccentOption[] = [
  { hex: '#8B96F8', rgb: '139, 150, 248', label: 'Индиго' },
  { hex: '#52B8F2', rgb: '82, 184, 242', label: 'Кобальт' },
  { hex: '#3ED598', rgb: '62, 213, 152', label: 'Изумруд' },
  { hex: '#F26E84', rgb: '242, 110, 132', label: 'Коралл' },
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

  // 6. Plain launch fallback
  return {
    type: 'launch',
    target: item.target,
    args: baseArgs,
    asAdmin: false,
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
 * Formats strictly single-line memory usage (e.g. `18.4 / 64 ГБ`) with zero raw RPM/MHz numbers.
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
