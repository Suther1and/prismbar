/// <reference types="vite/client" />
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it, vi } from 'vitest';
import mainSrc from '../main.tsx?raw';
import spotCardSrc from '../components/start/SpotCard.tsx?raw';
import startSettingsSrc from '../components/start/StartSettingsOverlay.tsx?raw';
import { StartSettingsOverlay } from '../components/start/StartSettingsOverlay';

async function readTextAsset(relPath: string): Promise<string> {
  // @ts-expect-error -- tsconfig omits @types/node; Vitest resolves node:fs at test runtime
  const fsMod = await import('node:fs');
  return fsMod.readFileSync(new URL(relPath, import.meta.url), 'utf8');
}
import {
  applyAppearanceToDocument,
  DEFAULT_GLOBAL_ACCENT_HEX,
  DEFAULT_GLOBAL_ACCENT_RGB,
  GLASS_DEPTH_OPTIONS,
  GLOBAL_ACCENT_PALETTE,
  normalizeAccentMode,
  normalizeGlassDepth,
  normalizeThemePreset,
  resolveAppearanceState,
  resolveGlobalAccent,
  THEME_PRESETS,
  type AppearanceRootTarget,
} from './appearance';

function createMockRoot(): {
  root: AppearanceRootTarget;
  cssVars: Record<string, string>;
} {
  const cssVars: Record<string, string> = {};
  const dataset: Record<string, string | undefined> = {};
  const root: AppearanceRootTarget = {
    dataset,
    style: {
      setProperty: (name: string, value: string | null) => {
        if (value === null) {
          delete cssVars[name];
        } else {
          cssVars[name] = value;
        }
      },
    },
  };
  return { root, cssVars };
}

describe('appearance engine & presets', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('defines all 6 dark theme presets with Russian labels, hints, and surface hexes', () => {
    expect(THEME_PRESETS.map((t) => t.id)).toEqual([
      'obsidian',
      'carbon',
      'midnight',
      'emerald',
      'amethyst',
      'graphite',
    ]);
    for (const preset of THEME_PRESETS) {
      expect(preset.label.trim().length).toBeGreaterThan(0);
      expect(preset.hint.trim().length).toBeGreaterThan(0);
      expect(preset.baseHex).toMatch(/^#[0-9A-F]{6}$/i);
      expect(preset.elevatedHex).toMatch(/^#[0-9A-F]{6}$/i);
      expect(preset.borderHex).toMatch(/^#[0-9A-F]{6}$/i);
    }
    const carbon = THEME_PRESETS.find((t) => t.id === 'carbon');
    expect(carbon?.baseHex).toBe('#0A0A0C');
    expect(carbon?.elevatedHex).toBe('#121214');
  });

  it('defines all 4 glass depth options including solid («Сплошной»)', () => {
    expect(GLASS_DEPTH_OPTIONS.map((g) => g.id)).toEqual([
      'crystal',
      'obsidian',
      'matte',
      'solid',
    ]);
    const solid = GLASS_DEPTH_OPTIONS.find((g) => g.id === 'solid');
    expect(solid?.label).toBe('Сплошной');
  });

  it('defines all 10 global accent swatches including #E2E8F0 Сталь', () => {
    expect(GLOBAL_ACCENT_PALETTE.map((s) => s.hex)).toEqual([
      '#818CF8',
      '#38BDF8',
      '#2DD4BF',
      '#34D399',
      '#FBBF24',
      '#FB923C',
      '#FB7185',
      '#F472B6',
      '#A78BFA',
      '#E2E8F0',
    ]);
    const steel = GLOBAL_ACCENT_PALETTE.find((s) => s.hex === '#E2E8F0');
    expect(steel?.label).toBe('Сталь');
    expect(steel?.rgb).toBe('226, 232, 240');
  });

  it('normalizes valid and invalid theme, glass depth, accent mode, and color fallbacks', () => {
    expect(normalizeThemePreset(' CARBON ')).toBe('carbon');
    expect(normalizeThemePreset('midnight')).toBe('midnight');
    expect(normalizeThemePreset('emerald')).toBe('emerald');
    expect(normalizeThemePreset('amethyst')).toBe('amethyst');
    expect(normalizeThemePreset('graphite')).toBe('graphite');
    expect(normalizeThemePreset('light-white')).toBe('obsidian');
    expect(normalizeThemePreset(null)).toBe('obsidian');
    expect(normalizeThemePreset(undefined)).toBe('obsidian');

    expect(normalizeGlassDepth('crystal')).toBe('crystal');
    expect(normalizeGlassDepth('OBSIDIAN')).toBe('obsidian');
    expect(normalizeGlassDepth('matte')).toBe('matte');
    expect(normalizeGlassDepth(' solid ')).toBe('solid');
    expect(normalizeGlassDepth('frosted')).toBe('obsidian');
    expect(normalizeGlassDepth(undefined)).toBe('obsidian');

    expect(normalizeAccentMode('custom')).toBe('custom');
    expect(normalizeAccentMode(' CUSTOM ')).toBe('custom');
    expect(normalizeAccentMode('per_space')).toBe('per_space');
    expect(normalizeAccentMode('bogus')).toBe('per_space');
    expect(normalizeAccentMode(null)).toBe('per_space');

    expect(resolveGlobalAccent('#34d399')).toEqual({ hex: '#34D399', rgb: '52, 211, 153' });
    expect(resolveGlobalAccent('#38b')).toEqual({ hex: '#3388BB', rgb: '51, 136, 187' });
    expect(resolveGlobalAccent(null, '226, 232, 240')).toEqual({
      hex: '#E2E8F0',
      rgb: '226, 232, 240',
    });
    expect(resolveGlobalAccent('invalid', '999, 0, 0')).toEqual({
      hex: DEFAULT_GLOBAL_ACCENT_HEX,
      rgb: DEFAULT_GLOBAL_ACCENT_RGB,
    });
  });

  it('resolves defaults cleanly when config is null or omits fields', () => {
    expect(resolveAppearanceState(null)).toEqual({
      themePreset: 'obsidian',
      glassDepth: 'obsidian',
      accentMode: 'per_space',
      accentHex: '#818CF8',
      accentRgb: '129, 140, 248',
      surfaceGradients: true,
      hoverGlow: true,
    });
  });

  it('applies dataset attributes and CSS custom properties to documentElement on valid and fallback configs', () => {
    const { root, cssVars } = createMockRoot();

    // 1. Default / null config
    applyAppearanceToDocument(null, root);
    expect(root.dataset).toEqual({
      theme: 'obsidian',
      glass: 'obsidian',
      gradients: 'on',
      hoverGlow: 'on',
      accentMode: 'per_space',
    });
    expect(cssVars).toEqual({
      '--global-accent': '#818CF8',
      '--global-accent-rgb': '129, 140, 248',
    });

    // 2. Full custom configuration (OLED carbon + solid + no gradients + no hover glow + custom accent)
    applyAppearanceToDocument(
      {
        themePreset: 'carbon',
        glassDepth: 'solid',
        accentMode: 'custom',
        accentHex: '#2dd4bf',
        accentRgb: '45,212,191',
        surfaceGradients: false,
        hoverGlow: false,
      },
      root,
    );
    expect(root.dataset).toEqual({
      theme: 'carbon',
      glass: 'solid',
      gradients: 'off',
      hoverGlow: 'off',
      accentMode: 'custom',
    });
    expect(cssVars).toEqual({
      '--global-accent': '#2DD4BF',
      '--global-accent-rgb': '45, 212, 191',
    });

    // 3. Invalid values fall back safely while preserving explicit booleans
    applyAppearanceToDocument(
      {
        themePreset: 'unknown' as never,
        glassDepth: 'broken' as never,
        accentMode: 'bogus' as never,
        accentHex: 'not-a-hex',
        accentRgb: 'bad',
        surfaceGradients: false,
        hoverGlow: true,
      },
      root,
    );
    expect(root.dataset).toEqual({
      theme: 'obsidian',
      glass: 'obsidian',
      gradients: 'off',
      hoverGlow: 'on',
      accentMode: 'per_space',
    });
    expect(cssVars).toEqual({
      '--global-accent': '#818CF8',
      '--global-accent-rgb': '129, 140, 248',
    });
  });

  it('targets global document.documentElement by default and is SSR-safe when document is missing', () => {
    const { root, cssVars } = createMockRoot();
    vi.stubGlobal('document', { documentElement: root });

    applyAppearanceToDocument({
      themePreset: 'emerald',
      glassDepth: 'crystal',
      accentMode: 'custom',
      accentHex: '#E2E8F0',
      surfaceGradients: true,
      hoverGlow: false,
    });

    expect(root.dataset).toEqual({
      theme: 'emerald',
      glass: 'crystal',
      gradients: 'on',
      hoverGlow: 'off',
      accentMode: 'custom',
    });
    expect(cssVars).toEqual({
      '--global-accent': '#E2E8F0',
      '--global-accent-rgb': '226, 232, 240',
    });

    vi.unstubAllGlobals();
    expect(() => applyAppearanceToDocument(null, null)).not.toThrow();
    expect(applyAppearanceToDocument({ themePreset: 'amethyst' }).themePreset).toBe('amethyst');
  });

  it('wires themes.css, no-gradient/no-glow rules, SpotCard guard, and Settings Studio cleanly', async () => {
    const themesCssSrc = await readTextAsset('../styles/themes.css');
    const settingsStudioCssSrc = await readTextAsset('../styles/settings-studio.css');

    // 1. main.tsx imports themes.css right after tokens.css and calls applyAppearanceToDocument
    const tokensIdx = mainSrc.indexOf("import './styles/tokens.css'");
    const themesIdx = mainSrc.indexOf("import './styles/themes.css'");
    expect(tokensIdx).toBeGreaterThanOrEqual(0);
    expect(themesIdx).toBeGreaterThan(tokensIdx);
    expect(mainSrc).toContain('applyAppearanceToDocument');

    // 2. SpotCard guards pointer sheen transforms when hoverGlow === 'off'
    expect(spotCardSrc).toContain("dataset.hoverGlow === 'off'");

    // 3. themes.css covers all 6 themes, 4 glass levels, custom accent override, no-gradients, no-hover-glow
    for (const theme of ['obsidian', 'carbon', 'midnight', 'emerald', 'amethyst', 'graphite']) {
      expect(themesCssSrc).toContain(`:root[data-theme="${theme}"]`);
    }
    for (const glass of ['crystal', 'obsidian', 'matte', 'solid']) {
      expect(themesCssSrc).toContain(`:root[data-glass="${glass}"]`);
    }
    expect(themesCssSrc).toContain(':root[data-accent-mode="custom"]');
    expect(themesCssSrc).toContain('--global-accent');
    expect(themesCssSrc).toContain(':root[data-gradients="off"]');
    expect(themesCssSrc).toContain('.chassis-aura-layer');
    expect(themesCssSrc).toContain(':root[data-hover-glow="off"]');
    expect(themesCssSrc).toContain('.tb-sheen');
    expect(themesCssSrc).toContain('.spot-sheen');
    expect(themesCssSrc).toContain('.media-ambient');

    // 4. settings-studio.css sets Studio dimensions & custom scrollbars
    expect(settingsStudioCssSrc).toContain('min(940px, calc(100% - 36px))');
    expect(settingsStudioCssSrc).toContain('min(604px, calc(100% - 32px))');
    expect(settingsStudioCssSrc).toContain('::-webkit-scrollbar');

    // 5. StartSettingsOverlay renders Studio tabs, themes, glass options, swatches & controls cleanly
    const html = renderToStaticMarkup(
      createElement(StartSettingsOverlay, {
        open: true,
        config: null,
        onClose: () => {},
        onSetBarMode: async () => {},
        onSetSecondaryMode: async () => {},
        onSetAutostart: async () => {},
        onSetAppearance: async () => {},
        onRefreshIndexes: async () => {},
        onRestoreTrayApp: async () => {},
        onRestoreAllTrayApps: async () => {},
        onExportConfig: async () => {},
        onImportConfig: async () => {},
        onRestoreWindowsAndExit: () => {},
      }),
    );
    expect(html).toContain('Внешний вид');
    expect(html).toContain('Нижняя панель');
    expect(html).toContain('Меню «Пуск»');
    expect(html).toContain('Устройства и трей');
    expect(html).toContain('Система');
    expect(html).toContain('Поиск настроек');
    expect(html).toContain('Карбон');
    expect(html).toContain('Сплошной');
    expect(html).toContain('Сталь');
    expect(html).toContain('Градиенты и фоновая аура');
    expect(html).toContain('Подсветка при наведении и от курсора');
    expect(html).toContain('Сбросить вид');

    // 6. Source contracts on StartSettingsOverlay (search + bar + start sections)
    expect(startSettingsSrc).toContain('onOpenSpaceWidgets');
    expect(startSettingsSrc).toContain('resetVisuals: true');
    expect(startSettingsSrc).toContain('widgetsPosition');
    expect(startSettingsSrc).toContain('clockDateFormat');
    expect(startSettingsSrc).toContain('clockLayout');
    expect(startSettingsSrc).toContain('defaultTileSize');
  });
});
