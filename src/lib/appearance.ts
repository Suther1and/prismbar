import type {
  AccentMode,
  GlassDepth,
  TestBarConfig,
  ThemePreset,
} from './ipc';

export interface ThemePresetOption {
  id: ThemePreset;
  label: string;
  hint: string;
  baseHex: string;
  elevatedHex: string;
  borderHex: string;
  accentHintHex: string;
}

export interface GlassDepthOption {
  id: GlassDepth;
  label: string;
  hint: string;
}

export interface GlobalAccentSwatch {
  id: string;
  label: string;
  hex: string;
  rgb: string;
}

export interface ResolvedAppearanceState {
  themePreset: ThemePreset;
  glassDepth: GlassDepth;
  accentMode: AccentMode;
  accentHex: string;
  accentRgb: string;
  surfaceGradients: boolean;
  hoverGlow: boolean;
}

export interface AppearanceRootTarget {
  dataset: Record<string, string | undefined>;
  style: {
    setProperty: (name: string, value: string | null) => void;
  };
}

export const DEFAULT_GLOBAL_ACCENT_HEX = '#818CF8';
export const DEFAULT_GLOBAL_ACCENT_RGB = '129, 140, 248';

export const THEME_PRESETS: readonly ThemePresetOption[] = [
  {
    id: 'obsidian',
    label: 'Обсидиан',
    hint: 'Глубокое тёмное стекло',
    baseHex: '#0D1017',
    elevatedHex: '#151A26',
    borderHex: '#252C3D',
    accentHintHex: '#818CF8',
  },
  {
    id: 'carbon',
    label: 'Карбон',
    hint: 'OLED чёрный контраст',
    baseHex: '#0A0A0C',
    elevatedHex: '#121214',
    borderHex: '#242429',
    accentHintHex: '#A1A1AA',
  },
  {
    id: 'midnight',
    label: 'Полночь',
    hint: 'Ночной сапфир',
    baseHex: '#080E1E',
    elevatedHex: '#101932',
    borderHex: '#1E2D52',
    accentHintHex: '#38BDF8',
  },
  {
    id: 'emerald',
    label: 'Изумруд',
    hint: 'Хвойный сланец',
    baseHex: '#081310',
    elevatedHex: '#10211C',
    borderHex: '#1D3930',
    accentHintHex: '#34D399',
  },
  {
    id: 'amethyst',
    label: 'Аметист',
    hint: 'Сумеречный фиолет',
    baseHex: '#100B19',
    elevatedHex: '#1A1328',
    borderHex: '#2E2246',
    accentHintHex: '#A78BFA',
  },
  {
    id: 'graphite',
    label: 'Графит',
    hint: 'Нейтральный тёмно-серый',
    baseHex: '#121417',
    elevatedHex: '#1B1E24',
    borderHex: '#2C313A',
    accentHintHex: '#94A3B8',
  },
] as const;

export const GLASS_DEPTH_OPTIONS: readonly GlassDepthOption[] = [
  { id: 'crystal', label: 'Кристалл', hint: 'Воздушное стекло' },
  { id: 'obsidian', label: 'Обсидиан', hint: 'Сбалансированный' },
  { id: 'matte', label: 'Матовый', hint: 'Плотный контраст' },
  { id: 'solid', label: 'Сплошной', hint: 'Без прозрачности' },
] as const;

export const GLOBAL_ACCENT_PALETTE: readonly GlobalAccentSwatch[] = [
  { id: 'indigo', label: 'Индиго', hex: '#818CF8', rgb: '129, 140, 248' },
  { id: 'sky', label: 'Кобальт', hex: '#38BDF8', rgb: '56, 189, 248' },
  { id: 'teal', label: 'Бирюза', hex: '#2DD4BF', rgb: '45, 212, 191' },
  { id: 'emerald', label: 'Изумруд', hex: '#34D399', rgb: '52, 211, 153' },
  { id: 'amber', label: 'Янтарь', hex: '#FBBF24', rgb: '251, 191, 36' },
  { id: 'orange', label: 'Оранж', hex: '#FB923C', rgb: '251, 146, 60' },
  { id: 'coral', label: 'Коралл', hex: '#FB7185', rgb: '251, 113, 133' },
  { id: 'rose', label: 'Роза', hex: '#F472B6', rgb: '244, 114, 182' },
  { id: 'violet', label: 'Аметист', hex: '#A78BFA', rgb: '167, 139, 250' },
  { id: 'steel', label: 'Сталь', hex: '#E2E8F0', rgb: '226, 232, 240' },
] as const;

const THEME_PRESET_SET = new Set<string>(THEME_PRESETS.map((t) => t.id));
const GLASS_DEPTH_SET = new Set<string>(GLASS_DEPTH_OPTIONS.map((g) => g.id));

export function normalizeThemePreset(raw: unknown): ThemePreset {
  if (typeof raw === 'string') {
    const norm = raw.trim().toLowerCase();
    if (THEME_PRESET_SET.has(norm)) {
      return norm as ThemePreset;
    }
  }
  return 'obsidian';
}

export function normalizeGlassDepth(raw: unknown): GlassDepth {
  if (typeof raw === 'string') {
    const norm = raw.trim().toLowerCase();
    if (GLASS_DEPTH_SET.has(norm)) {
      return norm as GlassDepth;
    }
  }
  return 'obsidian';
}

export function normalizeAccentMode(raw: unknown): AccentMode {
  if (typeof raw === 'string' && raw.trim().toLowerCase() === 'custom') {
    return 'custom';
  }
  return 'per_space';
}

function parseHexToParts(raw: unknown): { hex: string; rgb: string } | null {
  if (typeof raw !== 'string') return null;
  const trimmed = raw.trim();
  const body = trimmed.startsWith('#') ? trimmed.slice(1) : trimmed;
  if (!/^[0-9a-fA-F]+$/.test(body)) return null;
  if (body.length === 3) {
    const r = Number.parseInt(body[0] + body[0], 16);
    const g = Number.parseInt(body[1] + body[1], 16);
    const b = Number.parseInt(body[2] + body[2], 16);
    const hex = `#${(body[0] + body[0] + body[1] + body[1] + body[2] + body[2]).toUpperCase()}`;
    return { hex, rgb: `${r}, ${g}, ${b}` };
  }
  if (body.length === 6) {
    const r = Number.parseInt(body.slice(0, 2), 16);
    const g = Number.parseInt(body.slice(2, 4), 16);
    const b = Number.parseInt(body.slice(4, 6), 16);
    return { hex: `#${body.toUpperCase()}`, rgb: `${r}, ${g}, ${b}` };
  }
  return null;
}

function parseRgbToParts(raw: unknown): { hex: string; rgb: string } | null {
  if (typeof raw !== 'string') return null;
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const parts = trimmed.split(',').map((p) => p.trim());
  if (parts.length !== 3 || parts.some((p) => !/^\d{1,3}$/.test(p))) {
    return null;
  }
  const nums = parts.map((p) => Number.parseInt(p, 10));
  if (nums.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) {
    return null;
  }
  const toHex2 = (n: number) => n.toString(16).padStart(2, '0').toUpperCase();
  return {
    hex: `#${toHex2(nums[0])}${toHex2(nums[1])}${toHex2(nums[2])}`,
    rgb: `${nums[0]}, ${nums[1]}, ${nums[2]}`,
  };
}

/**
 * Resolves global custom accent hex & RGB triplet with deterministic fallback to Indigo (`#818CF8`).
 */
export function resolveGlobalAccent(
  accentHex?: unknown,
  accentRgb?: unknown,
): { hex: string; rgb: string } {
  const fromHex = parseHexToParts(accentHex);
  const fromRgb = parseRgbToParts(accentRgb);

  if (fromHex) {
    return {
      hex: fromHex.hex,
      rgb: fromHex.rgb,
    };
  }
  if (fromRgb) {
    return fromRgb;
  }
  return {
    hex: DEFAULT_GLOBAL_ACCENT_HEX,
    rgb: DEFAULT_GLOBAL_ACCENT_RGB,
  };
}

export function resolveAppearanceState(
  cfg?: Partial<
    Pick<
      TestBarConfig,
      | 'themePreset'
      | 'glassDepth'
      | 'accentMode'
      | 'accentHex'
      | 'accentRgb'
      | 'surfaceGradients'
      | 'hoverGlow'
    >
  > | null,
): ResolvedAppearanceState {
  const accent = resolveGlobalAccent(cfg?.accentHex, cfg?.accentRgb);
  return {
    themePreset: normalizeThemePreset(cfg?.themePreset),
    glassDepth: normalizeGlassDepth(cfg?.glassDepth),
    accentMode: normalizeAccentMode(cfg?.accentMode),
    accentHex: accent.hex,
    accentRgb: accent.rgb,
    surfaceGradients: cfg?.surfaceGradients !== false,
    hoverGlow: cfg?.hoverGlow !== false,
  };
}

/**
 * Applies resolved TestBar appearance attributes & global accent CSS variables onto `:root`
 * (or a custom root element for tests).
 */
export function applyAppearanceToDocument(
  cfg?: Partial<
    Pick<
      TestBarConfig,
      | 'themePreset'
      | 'glassDepth'
      | 'accentMode'
      | 'accentHex'
      | 'accentRgb'
      | 'surfaceGradients'
      | 'hoverGlow'
    >
  > | null,
  root?: AppearanceRootTarget | null,
): ResolvedAppearanceState {
  const resolved = resolveAppearanceState(cfg);
  const target =
    root !== undefined
      ? root
      : typeof document !== 'undefined'
        ? document.documentElement
        : null;

  if (target) {
    target.dataset.theme = resolved.themePreset;
    target.dataset.glass = resolved.glassDepth;
    target.dataset.gradients = resolved.surfaceGradients ? 'on' : 'off';
    target.dataset.hoverGlow = resolved.hoverGlow ? 'on' : 'off';
    target.dataset.accentMode = resolved.accentMode;
    target.style.setProperty('--global-accent', resolved.accentHex);
    target.style.setProperty('--global-accent-rgb', resolved.accentRgb);
  }

  return resolved;
}
