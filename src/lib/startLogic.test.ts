/// <reference types="vite/client" />
import { describe, expect, it } from 'vitest';
import spotCardSrc from '../components/start/SpotCard.tsx?raw';
import startCommandPaletteSrc from '../components/start/StartCommandPalette.tsx?raw';
import startHeaderSrc from '../components/start/StartHeader.tsx?raw';
import startPinDrawerSrc from '../components/start/StartPinDrawer.tsx?raw';
import startSettingsSrc from '../components/start/StartSettingsOverlay.tsx?raw';
import startSpacesSrc from '../components/start/StartSpaces.tsx?raw';
import startViewSrc from '../components/StartView.tsx?raw';
import type { ClipboardEntry, PinItem, TestBarConfig } from './ipc';
import {
  buildGameComboLaunchPayload,
  buildSparklinePath,
  completeStartClosing,
  computeGameCardTilt,
  computeInterpolatedMediaSec,
  computeSlidingPillRect,
  CUSTOM_SPACE_ACCENTS,
  EMPTY_TELEMETRY_HISTORY,
  filterClipboardEntries,
  findBannedStartArtifacts,
  formatDiskSummary,
  formatKillProcessError,
  formatMediaDuration,
  formatMemoryRatio,
  formatProcessMetricLine,
  GAME_CARD_MAX_TILT_DEG,
  getBatteryColor,
  getClipboardKindLabel,
  getGpuTempStatusPill,
  getHonestShiftBadge,
  getTelemetryLevelClass,
  getTelemetryStatusPill,
  hasHonestShiftAction,
  hexColorToRgbString,
  INITIAL_START_ANIM_STATE,
  isAppPinnedInSpace,
  normalizeUrlPinInput,
  partitionHeroAndCompact,
  promoteStartEntering,
  pushTelemetryHistory,
  pushTelemetrySample,
  RECENT_GAMES_LIMIT,
  reducePowerClick,
  reduceProcessKillClick,
  reduceStartState,
  resolveClipboardKind,
  resolvePinAction,
  resolveProcessKillPids,
  selectDisplayedGames,
  START_EXIT_ANIM_MS,
  TELEMETRY_HISTORY_CAP,
  validateCustomSpaceName,
  validateGameComboInput,
} from './startLogic';

function makePin(overrides: Partial<PinItem> & Pick<PinItem, 'id' | 'title' | 'target'>): PinItem {
  return {
    kind: 'app',
    subtitle: 'Sub',
    altAction: 'От админа',
    accentRgb: '139,150,248',
    iconKey: overrides.id,
    iconData: null,
    args: null,
    matchExe: null,
    matchAumid: null,
    comboTargets: null,
    ...overrides,
  };
}

describe('startLogic animations, unlimited grids, Shift layer, Power & custom spaces', () => {
  it('drives Paint-Sync StartAnimState phases (hidden -> entering -> open -> closing -> hidden) cleanly', () => {
    expect(START_EXIT_ANIM_MS).toBe(115);
    let st = INITIAL_START_ANIM_STATE;
    expect(st).toEqual({
      phase: 'hidden',
      open: false,
      closing: false,
      openSeq: 0,
      gen: 0,
    });

    // Opening enters 'entering' phase with incremented openSeq & synced gen
    st = reduceStartState(st, { open: true, gen: 1 });
    expect(st).toEqual({
      phase: 'entering',
      open: true,
      closing: false,
      openSeq: 1,
      gen: 1,
    });

    // Promoting on awake rAF flips entering -> open
    st = promoteStartEntering(st);
    expect(st).toEqual({
      phase: 'open',
      open: true,
      closing: false,
      openSeq: 1,
      gen: 1,
    });
    // Idempotent when already open
    expect(promoteStartEntering(st)).toBe(st);

    // Normal close enters 'closing'
    st = reduceStartState(st, false, { gen: 1 });
    expect(st).toEqual({
      phase: 'closing',
      open: false,
      closing: true,
      openSeq: 1,
      gen: 1,
    });

    // Stale gen exit completion is ignored
    expect(completeStartClosing(st, 999)).toBe(st);

    // Reopening mid-close immediately cancels closing and bumps openSeq
    st = reduceStartState(st, true, { gen: 2 });
    expect(st).toEqual({
      phase: 'entering',
      open: true,
      closing: false,
      openSeq: 2,
      gen: 2,
    });

    // Immediate preemption goes straight to hidden
    st = reduceStartState(st, { open: false, gen: 3, immediate: true });
    expect(st).toEqual({
      phase: 'hidden',
      open: false,
      closing: false,
      openSeq: 2,
      gen: 3,
    });

    // Normal close -> completeStartClosing settles to hidden
    st = promoteStartEntering(reduceStartState(st, { open: true, gen: 4 }));
    st = reduceStartState(st, { open: false, gen: 4 });
    st = completeStartClosing(st, 4);
    expect(st.phase).toBe('hidden');
    expect(st.closing).toBe(false);
  });

  it('partitions 15+ pins into 2 heroes + all remaining compact tiles with zero .slice() clipping', () => {
    const pins = Array.from({ length: 16 }, (_, i) =>
      makePin({ id: `p-${i}`, title: `App ${i}`, target: `C:\\Apps\\app${i}.exe` }),
    );
    const { heroes, compact } = partitionHeroAndCompact(pins);
    expect(heroes).toHaveLength(2);
    expect(compact).toHaveLength(14);
    expect(heroes.length + compact.length).toBe(16);
  });

  it('resolves Shift alternate actions faithfully and gates honest Shift badges strictly', () => {
    // 1. Zen Browser -> Приватное окно (--private-window)
    const zen = makePin({
      id: 'zen',
      title: 'Zen Browser',
      target: 'C:\\Program Files\\Zen Browser\\zen.exe',
      altAction: 'Приватное окно',
      iconKey: 'zen',
    });
    expect(resolvePinAction(zen, false)).toEqual({
      type: 'launch',
      target: zen.target,
      args: null,
      asAdmin: false,
      closeStart: true,
    });
    expect(resolvePinAction(zen, true)).toEqual({
      type: 'launch',
      target: zen.target,
      args: '--private-window',
      asAdmin: false,
      closeStart: true,
    });
    expect(getHonestShiftBadge(zen)).toBe('Приватное окно');
    expect(hasHonestShiftAction(zen)).toBe(true);

    // 2. Yandex Browser -> Инкогнито (--incognito)
    const yandex = makePin({
      id: 'yandex',
      title: 'Яндекс',
      target: 'C:\\Users\\u\\AppData\\Local\\Yandex\\browser.exe',
      altAction: 'Инкогнито',
      iconKey: 'yandex',
    });
    expect(resolvePinAction(yandex, true)).toEqual({
      type: 'launch',
      target: yandex.target,
      args: '--incognito',
      asAdmin: false,
      closeStart: true,
    });
    expect(getHonestShiftBadge(yandex)).toBe('Инкогнито');

    // 3. VS Code -> Новое окно (--new-window) & Explorer -> /n
    const vsc = makePin({
      id: 'vscode',
      title: 'VS Code',
      target: 'C:\\Program Files\\Microsoft VS Code\\Code.exe',
      altAction: 'Новое окно',
    });
    expect(resolvePinAction(vsc, true)).toEqual({
      type: 'launch',
      target: vsc.target,
      args: '--new-window',
      asAdmin: false,
      closeStart: true,
    });
    expect(getHonestShiftBadge(vsc)).toBe('Новое окно');

    const exp = makePin({
      id: 'explorer',
      title: 'Проводник',
      target: 'C:\\Windows\\explorer.exe',
      altAction: 'Новое окно',
    });
    expect(resolvePinAction(exp, true)).toEqual({
      type: 'launch',
      target: exp.target,
      args: '/n',
      asAdmin: false,
      closeStart: true,
    });
    expect(getHonestShiftBadge(exp)).toBe('Новое окно');

    // 4. Work folder -> Открыть в терминале
    const folder = makePin({
      id: 'wf',
      kind: 'folder',
      title: 'testbar',
      target: 'C:\\Users\\u\\Documents\\testbar',
      altAction: 'Открыть в терминале',
    });
    expect(resolvePinAction(folder, true)).toEqual({
      type: 'terminal',
      path: folder.target,
    });
    expect(getHonestShiftBadge(folder)).toBe('Открыть в терминале');

    // 5. Personal quick link -> Скопировать путь
    const qlink = makePin({
      id: 'dl',
      kind: 'folder',
      title: 'Загрузки',
      target: 'C:\\Users\\u\\Downloads',
      altAction: 'Скопировать путь',
    });
    expect(resolvePinAction(qlink, true)).toEqual({
      type: 'copy_path',
      text: qlink.target,
      toast: 'Путь скопирован',
    });
    expect(getHonestShiftBadge(qlink)).toBe('Скопировать путь');

    // 6. Terminal / OBS -> От админа (real on Win32 .exe, hidden on UWP/URL)
    const obs = makePin({
      id: 'obs',
      title: 'OBS Studio',
      target: 'C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe',
      altAction: 'От админа',
    });
    expect(resolvePinAction(obs, true)).toEqual({
      type: 'launch',
      target: obs.target,
      args: null,
      asAdmin: true,
      closeStart: true,
    });
    expect(getHonestShiftBadge(obs)).toBe('От админа');

    const uwpAdmin = makePin({
      id: 'uwp',
      title: 'Calculator',
      target: 'shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
      altAction: 'От админа',
    });
    expect(getHonestShiftBadge(uwpAdmin)).toBeNull();

    // 7. Fake decorative Shift badges (e.g. 'Каналы', 'Моя волна', 'Избранное') are suppressed
    const discord = makePin({
      id: 'discord',
      title: 'Discord',
      target: 'C:\\Users\\u\\AppData\\Local\\Discord\\Update.exe',
      args: '--processStart Discord.exe',
      altAction: 'Каналы',
    });
    expect(resolvePinAction(discord, true)).toEqual({
      type: 'launch',
      target: discord.target,
      args: '--processStart Discord.exe',
      asAdmin: false,
      closeStart: true,
    });
    expect(getHonestShiftBadge(discord)).toBeNull();
    expect(hasHonestShiftAction(discord)).toBe(false);

    // Custom altLabel is shown when underlying Shift action is real
    const customLabeled = makePin({
      id: 'wt',
      title: 'Терминал',
      target: 'C:\\Windows\\System32\\cmd.exe',
      altAction: 'От админа',
      altLabel: 'Администратор',
    });
    expect(getHonestShiftBadge(customLabeled)).toBe('Администратор');
  });

  it('enforces 2-click armed Power semantics for Reboot and Shutdown while executing Lock/Sleep immediately', () => {
    expect(reducePowerClick(null, 'lock')).toEqual({
      execute: 'lock',
      nextArmed: null,
      closePower: true,
    });
    expect(reducePowerClick(null, 'sleep')).toEqual({
      execute: 'sleep',
      nextArmed: null,
      closePower: true,
    });

    const armReboot = reducePowerClick(null, 'reboot');
    expect(armReboot).toEqual({
      execute: null,
      nextArmed: 'reboot',
      closePower: false,
    });

    const switchArm = reducePowerClick(armReboot.nextArmed, 'shutdown');
    expect(switchArm).toEqual({
      execute: null,
      nextArmed: 'shutdown',
      closePower: false,
    });

    expect(reducePowerClick(switchArm.nextArmed, 'shutdown')).toEqual({
      execute: 'shutdown',
      nextArmed: null,
      closePower: true,
    });
  });

  it('validates floating custom-space creation, curated obsidian space accents, sliding rect & URL input', () => {
    expect(CUSTOM_SPACE_ACCENTS).toHaveLength(5);
    expect(CUSTOM_SPACE_ACCENTS.map((c) => c.hex)).toEqual([
      '#8B96F8',
      '#52B8F2',
      '#3ED598',
      '#F26E84',
      '#A78BFA',
    ]);

    expect(validateCustomSpaceName('   ', []).ok).toBe(false);
    expect(validateCustomSpaceName('Личное', []).ok).toBe(false);
    expect(validateCustomSpaceName('Крипто', ['крипто']).ok).toBe(false);
    expect(validateCustomSpaceName('Очень Длинное Название Пространства', []).ok).toBe(false);
    expect(validateCustomSpaceName('  Трейдинг  ', ['Учёба'])).toEqual({
      ok: true,
      name: 'Трейдинг',
    });

    expect(computeSlidingPillRect(112.4, 86.6)).toEqual({ left: 112, width: 87, visible: true });
    expect(computeSlidingPillRect(0, 0)).toEqual({ left: 0, width: 0, visible: false });

    expect(normalizeUrlPinInput('github.com/openai')).toEqual({
      url: 'https://github.com/openai',
      title: 'github.com',
    });
    expect(normalizeUrlPinInput('https://www.linear.app', 'Linear')).toEqual({
      url: 'https://www.linear.app',
      title: 'Linear',
    });
    expect(normalizeUrlPinInput('   ')).toBeNull();
  });

  it('detects pinned Win32 and UWP apps accurately inside the active Space', () => {
    const cfg: TestBarConfig = {
      schemaVersion: 1,
      revision: 1,
      barMode: 'dock',
      secondaryTaskbarMode: 'testbar',
      autostart: false,
      taskbarPins: [],
      personalPins: [
        makePin({
          id: 'calc',
          title: 'Калькулятор',
          target: 'shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
          matchAumid: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
        }),
      ],
      personalLinks: [],
      workFolders: [],
      workApps: [],
      studioPins: [],
      studioTools: [],
      gameLaunchers: [],
      gameCombos: [],
      customSpaces: [],
      pinnedClipboard: [],
    };

    expect(
      isAppPinnedInSpace(
        {
          name: 'Calculator',
          target: 'shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
          aumid: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
        },
        'personal',
        cfg,
      ),
    ).toBe(true);
    expect(
      isAppPinnedInSpace(
        {
          name: 'Paint',
          target: 'shell:AppsFolder\\Microsoft.Paint_8wekyb3d8bbwe!App',
          aumid: 'Microsoft.Paint_8wekyb3d8bbwe!App',
        },
        'personal',
        cfg,
      ),
    ).toBe(false);
  });

  it('enforces zero footer hints, zero banned emoji/glyph hacks, zero slang, and zero pin .slice() clipping across Start components', () => {
    const startSources: Array<[string, string]> = [
      ['src/components/StartView.tsx', startViewSrc],
      ['src/components/start/StartHeader.tsx', startHeaderSrc],
      ['src/components/start/StartSpaces.tsx', startSpacesSrc],
      ['src/components/start/StartPinDrawer.tsx', startPinDrawerSrc],
      ['src/components/start/StartSettingsOverlay.tsx', startSettingsSrc],
      ['src/components/start/StartCommandPalette.tsx', startCommandPaletteSrc],
      ['src/components/start/SpotCard.tsx', spotCardSrc],
    ];

    for (const [relPath, content] of startSources) {
      expect(findBannedStartArtifacts(content), `${relPath} contains banned hint/glyph`).toEqual([]);
      expect(content, `${relPath} must not render start-footer`).not.toContain('start-footer');
      expect(content, `${relPath} must not render space-new-input`).not.toContain('space-new-input');
      expect(content, `${relPath} must not truncate compact pins with .slice(2, 11)`).not.toContain('.slice(2, 11)');
      expect(content, `${relPath} must not truncate work folders with .slice(0, 8)`).not.toContain('.slice(0, 8)');
      expect(content, `${relPath} must never expose raw GPU RPM/MHz numbers`).not.toMatch(/\b(RPM|MHz)\b/i);
    }

    expect(startSpacesSrc).not.toContain('Замьючен');
    expect(startSpacesSrc).not.toContain('vu-bars');
  });

  it('formats single-line telemetry ratios, sparkline ring buffer, SVG paths, disk strip & 2-click safe process kill', () => {
    expect(formatMemoryRatio(18.4, 64)).toBe('18.4 / 64 ГБ');
    expect(formatMemoryRatio(6.0, 24.0)).toBe('6 / 24 ГБ');
    expect(formatMemoryRatio(18.4, 64)).not.toContain('\n');

    expect(formatDiskSummary({ usedPct: 68, freeGb: 142.4 })).toBe('68% · 142.4 ГБ свободно');
    expect(getTelemetryLevelClass(42)).toBe('');
    expect(getTelemetryLevelClass(75)).toBe('lvl-warn');
    expect(getTelemetryLevelClass(92)).toBe('lvl-hot');

    expect(getTelemetryStatusPill(22)).toEqual({ cls: 'st-ok', label: 'Свободно' });
    expect(getTelemetryStatusPill(58)).toEqual({ cls: 'st-mid', label: 'Норма' });
    expect(getTelemetryStatusPill(88)).toEqual({ cls: 'st-hot', label: 'Высокая' });

    expect(getGpuTempStatusPill(45)).toEqual({ cls: 'st-ok', label: 'Холодная' });
    expect(getGpuTempStatusPill(68)).toEqual({ cls: 'st-mid', label: 'Рабочая' });
    expect(getGpuTempStatusPill(84)).toEqual({ cls: 'st-hot', label: 'Горячая' });

    expect(getBatteryColor(85)).toBe('#34D399');
    expect(getBatteryColor(24)).toBe('#F59E0B');
    expect(getBatteryColor(10)).toBe('#FB7185');

    // Rolling 30-point ring buffer
    let hist = EMPTY_TELEMETRY_HISTORY;
    for (let i = 0; i < TELEMETRY_HISTORY_CAP + 5; i++) {
      hist = pushTelemetryHistory(hist, {
        cpuPct: i * 3,
        ramPct: 50,
        gpuPct: 20,
        vramPct: 35,
        gpuAvailable: true,
      });
    }
    expect(hist.cpu).toHaveLength(TELEMETRY_HISTORY_CAP);
    expect(hist.cpu[hist.cpu.length - 1]).toBe(100); // clamped
    expect(pushTelemetrySample([10, 20], -5, 3)).toEqual([10, 20, 0]);

    // SVG sparkline path generation
    expect(buildSparklinePath([])).toEqual({ linePath: '', areaPath: '' });
    const single = buildSparklinePath([50], 56, 16);
    expect(single.linePath).toBe('M 0 8 L 56 8');
    expect(single.areaPath).toBe('M 0 8 L 56 8 L 56 16 L 0 16 Z');
    const multi = buildSparklinePath([0, 50, 100], 56, 16);
    expect(multi.linePath).toBe('M 0 14.5 L 28 8 L 56 1.5');
    expect(multi.areaPath).toContain('L 56 16 L 0 16 Z');

    // 2-click armed process kill
    const armA = reduceProcessKillClick(null, 'work:chrome:100');
    expect(armA).toEqual({ execute: false, nextArmedKey: 'work:chrome:100' });
    const switchB = reduceProcessKillClick(armA.nextArmedKey, 'work:code:200');
    expect(switchB).toEqual({ execute: false, nextArmedKey: 'work:code:200' });
    const confirmB = reduceProcessKillClick(switchB.nextArmedKey, 'work:code:200');
    expect(confirmB).toEqual({ execute: true, nextArmedKey: null });

    expect(formatProcessMetricLine({ memGb: 4.5, vramGb: 1.2, usesGpu: true }, 'ram')).toEqual({
      primary: '4.5 ГБ',
      gpuBadge: null,
    });
    expect(formatProcessMetricLine({ memGb: 2.0, vramGb: 8.5, usesGpu: true }, 'gpu')).toEqual({
      primary: '2 ГБ',
      gpuBadge: 'VRAM 8.5 ГБ',
    });
    expect(formatProcessMetricLine({ memGb: 3.1, vramGb: 0, usesGpu: true }, 'gpu')).toEqual({
      primary: '3.1 ГБ',
      gpuBadge: 'GPU',
    });
    expect(formatProcessMetricLine({ memGb: 1.4, vramGb: 0, usesGpu: false }, 'gpu')).toEqual({
      primary: '1.4 ГБ',
      gpuBadge: null,
    });

    expect(resolveProcessKillPids({ pid: 102, pids: [101, 102, 101, 0, -5, 103] })).toEqual([101, 102, 103]);
    expect(resolveProcessKillPids({ pid: 777, pids: [] })).toEqual([777]);

    expect(
      formatKillProcessError('Anticheat', 'Отказано в доступе при завершении процесса 444 (нужны права администратора)'),
    ).toBe('Отказано в доступе: Anticheat требует прав администратора');
    expect(formatKillProcessError('Docker', new Error('Access is denied'))).toBe(
      'Отказано в доступе: Docker требует прав администратора',
    );
    expect(formatKillProcessError('Chrome', '')).toBe('Не удалось завершить Chrome');
  });

  it('parses HEX->RGB clipboard colors, classifies/filters clipboard vault & interpolates media timeline', () => {
    expect(hexColorToRgbString('#8B96F8')).toBe('rgb(139, 150, 248)');
    expect(hexColorToRgbString('#fff')).toBe('rgb(255, 255, 255)');
    expect(hexColorToRgbString('#10203080')).toBe('rgba(16, 32, 48, 0.5)');
    expect(hexColorToRgbString('#102030FF')).toBe('rgb(16, 32, 48)');
    expect(hexColorToRgbString('not-a-color')).toBeNull();

    const entries: ClipboardEntry[] = [
      { id: '1', tag: 'URL', kind: 'url', text: 'https://linear.app', preview: 'https://linear.app', pinned: false },
      { id: '2', tag: 'HEX', kind: 'hex', text: '#3ED598', color: '#3ED598', pinned: true },
      { id: '3', tag: 'CODE', kind: 'code', text: 'const x = 1;\nconst y = 2;', preview: 'const x = 1; ...', lineCount: 2, pinned: false },
    ];
    expect(resolveClipboardKind(entries[0])).toBe('url');
    expect(getClipboardKindLabel('path')).toBe('PATH');
    expect(filterClipboardEntries(entries, 'linear')).toHaveLength(1);
    expect(filterClipboardEntries(entries, 'CODE')).toHaveLength(1);
    expect(filterClipboardEntries(entries, '')).toHaveLength(3);

    // Media timeline interpolation
    expect(
      computeInterpolatedMediaSec(
        { positionSec: 40, durationSec: 120, isPlaying: true, lastUpdatedMs: 10_000 },
        15_500,
      ),
    ).toBeCloseTo(45.5, 2);
    expect(
      computeInterpolatedMediaSec(
        { positionSec: 118, durationSec: 120, isPlaying: true, lastUpdatedMs: 10_000 },
        20_000,
      ),
    ).toBe(120);
    expect(
      computeInterpolatedMediaSec(
        { positionSec: 40, durationSec: 120, isPlaying: false, lastUpdatedMs: 10_000 },
        25_000,
      ),
    ).toBe(40);
    expect(formatMediaDuration(65)).toBe('1:05');
    expect(formatMediaDuration(3665)).toBe('1:01:05');
  });

  it('toggles Recent(4)/All Games cleanly, clamps ±4.5° 3D tilt, validates combo builder & dispatches combos', () => {
    expect(GAME_CARD_MAX_TILT_DEG).toBe(4.5);

    const library = Array.from({ length: 9 }, (_, i) => ({ appId: String(100 + i), name: `Game ${i + 1}` }));
    const recent = selectDisplayedGames(library, 'recent');
    const all = selectDisplayedGames(library, 'all');
    expect(recent).toHaveLength(RECENT_GAMES_LIMIT);
    expect(recent.map((g) => g.name)).toEqual(['Game 1', 'Game 2', 'Game 3', 'Game 4']);
    expect(all).toHaveLength(9);

    const rect = { left: 100, top: 200, width: 200, height: 100 };
    expect(computeGameCardTilt(200, 250, rect, false)).toEqual({
      gxPct: 50,
      gyPct: 50,
      rotateXDeg: 0,
      rotateYDeg: 0,
    });

    const topRightExtreme = computeGameCardTilt(999, -500, rect, false);
    expect(topRightExtreme.gxPct).toBe(100);
    expect(topRightExtreme.gyPct).toBe(0);
    expect(topRightExtreme.rotateXDeg).toBe(4.5);
    expect(topRightExtreme.rotateYDeg).toBe(4.5);

    const bottomLeftExtreme = computeGameCardTilt(-100, 999, rect, false);
    expect(bottomLeftExtreme.gxPct).toBe(0);
    expect(bottomLeftExtreme.gyPct).toBe(100);
    expect(bottomLeftExtreme.rotateXDeg).toBe(-4.5);
    expect(bottomLeftExtreme.rotateYDeg).toBe(-4.5);

    expect(computeGameCardTilt(300, 200, rect, true)).toEqual({
      gxPct: 50,
      gyPct: 50,
      rotateXDeg: 0,
      rotateYDeg: 0,
    });
    expect(computeGameCardTilt(200, 250, { left: 0, top: 0, width: 0, height: 100 }, false)).toEqual({
      gxPct: 50,
      gyPct: 50,
      rotateXDeg: 0,
      rotateYDeg: 0,
    });

    // Game Combo builder validation
    expect(validateGameComboInput('   ', ['a', 'b']).ok).toBe(false);
    expect(validateGameComboInput('Solo', ['steam://rungameid/570']).ok).toBe(false);
    expect(validateGameComboInput('  PoE 2 + Discord ', ['steam://rungameid/2694490', 'C:\\Discord.exe'])).toEqual({
      ok: true,
      name: 'PoE 2 + Discord',
      steps: ['steam://rungameid/2694490', 'C:\\Discord.exe'],
    });

    // Game Combo multi-target launch dispatch
    const dotaCombo = makePin({
      id: 'combo-dota2',
      kind: 'combo',
      title: 'Dota 2 + Discord',
      target: '',
      comboTargets: [
        'steam://rungameid/570',
        '"C:\\Users\\loki2\\AppData\\Local\\Discord\\Update.exe" --processStart Discord.exe',
        '   ',
      ],
    });
    expect(buildGameComboLaunchPayload(dotaCombo)).toEqual({
      command: 'launch_combo',
      args: {
        targets: [
          'steam://rungameid/570',
          '"C:\\Users\\loki2\\AppData\\Local\\Discord\\Update.exe" --processStart Discord.exe',
        ],
      },
    });
    expect(resolvePinAction(dotaCombo, false)).toEqual({
      type: 'combo',
      targets: [
        'steam://rungameid/570',
        '"C:\\Users\\loki2\\AppData\\Local\\Discord\\Update.exe" --processStart Discord.exe',
      ],
    });

    const emptyCombo = makePin({
      id: 'combo-empty',
      kind: 'combo',
      title: 'Empty',
      target: '',
      comboTargets: ['   '],
    });
    expect(buildGameComboLaunchPayload(emptyCombo)).toBeNull();
  });
});
