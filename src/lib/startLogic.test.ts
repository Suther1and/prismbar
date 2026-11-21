/// <reference types="vite/client" />
import { describe, expect, it } from 'vitest';
import spotCardSrc from '../components/start/SpotCard.tsx?raw';
import startCommandPaletteSrc from '../components/start/StartCommandPalette.tsx?raw';
import startHeaderSrc from '../components/start/StartHeader.tsx?raw';
import startPinDrawerSrc from '../components/start/StartPinDrawer.tsx?raw';
import startSettingsSrc from '../components/start/StartSettingsOverlay.tsx?raw';
import startSpacesSrc from '../components/start/StartSpaces.tsx?raw';
import startViewSrc from '../components/StartView.tsx?raw';
import type { ClipboardEntry, PinItem, SpaceConfig, TestBarConfig } from './ipc';
import {
  applyOptimisticMovePinInSpace,
  applyOptimisticPinReorder,
  buildGameComboLaunchPayload,
  buildSparklinePath,
  completeStartClosing,
  computeGameCardTilt,
  computeInterpolatedMediaSec,
  computePointerReorderRequest,
  computeSlidingPillRect,
  convertTempFromC,
  CUSTOM_SPACE_ACCENTS,
  EMPTY_TELEMETRY_HISTORY,
  EXPLORER_RESUME_TTL_MS,
  filterClipboardEntries,
  filterVisibleBatteries,
  findBannedStartArtifacts,
  findMatchingInstalledAppForPin,
  formatDiskSummary,
  formatHardwareSummaryLine,
  formatKillProcessError,
  formatMediaDuration,
  formatMemoryRatio,
  formatPeripheralSourceLabel,
  formatProcessMetricLine,
  formatStartTemperature,
  GAME_CARD_MAX_TILT_DEG,
  getBatteryColor,
  getClipboardKindLabel,
  getGpuTempStatusPill,
  getHonestShiftBadge,
  getPinnedItemInSpace,
  getSpaceDisplayName,
  getSpacePinList,
  getSpaceWidgetPlacement,
  getStartChassisAnimClass,
  getTelemetryLevelClass,
  getTelemetryStatusPill,
  hasExceededDragThreshold,
  hasHonestShiftAction,
  HEADER_OVERFLOW_BADGE_PX,
  HEADER_WIDGET_GAP_PX,
  hexColorToRgbString,
  INITIAL_START_ANIM_STATE,
  isAppPinnedInSpace,
  isLowBatteryPercent,
  mergeAudioStateRespectingLocks,
  MODULAR_SPACE_ACCENTS,
  moveHeaderWidget,
  moveSpaceIdInOrder,
  moveSpaceWidget,
  normalizeRgbTriplet,
  normalizeSessionVolumeScalar,
  normalizeUrlPinInput,
  partitionHeroAndCompact,
  partitionSpaceItemsBySection,
  planHeaderWidgetSlots,
  pushTelemetryHistory,
  pushTelemetrySample,
  RECENT_GAMES_LIMIT,
  reducePowerClick,
  reduceProcessKillClick,
  reduceStartState,
  resolveClipboardKind,
  resolveConfiguredSpaces,
  resolveCustomColorRgb,
  resolvePinAction,
  resolveProcessKillPids,
  resolveSpaceHeaderWidgets,
  selectDisplayedGames,
  setHeaderWidgetSide,
  setSpaceWidgetPlacement,
  settleStartOpen,
  shouldResetExplorerOnReopen,
  SPACE_TEMPLATES,
  SPACE_WIDGET_CATALOG,
  START_DRAG_THRESHOLD_PX,
  START_ENTER_ANIM_MS,
  START_EXIT_ANIM_MS,
  START_OPACITY_ANIM_MS,
  TELEMETRY_HISTORY_CAP,
  TILE_COLOR_PALETTE,
  toggleSpaceWidget,
  validateCustomSpaceName,
  validateGameComboInput,
  validateSpaceName,
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
  it('drives WAAPI-ready StartAnimState phases (hidden -> arming -> open -> closing -> hidden) & generation guards', () => {
    expect(START_ENTER_ANIM_MS).toBe(240);
    expect(START_OPACITY_ANIM_MS).toBe(170);
    expect(START_EXIT_ANIM_MS).toBe(120);
    let st = INITIAL_START_ANIM_STATE;
    expect(st).toEqual({
      phase: 'hidden',
      open: false,
      closing: false,
      openSeq: 0,
      gen: 0,
    });
    expect(getStartChassisAnimClass(st)).toBe('');

    // Opening enters 'arming' first (hidden at translateY(20px) scale(0.96) for zero flash before WAAPI spring)
    st = reduceStartState(st, { open: true, gen: 1 });
    expect(st).toEqual({
      phase: 'arming',
      open: true,
      closing: false,
      openSeq: 1,
      gen: 1,
    });
    expect(getStartChassisAnimClass(st)).toBe('open-arming');

    // Stale gen settle is ignored; matching gen settles to 'open'
    expect(settleStartOpen(st, 999)).toBe(st);
    st = settleStartOpen(st, 1);
    expect(st.phase).toBe('open');
    expect(getStartChassisAnimClass(st)).toBe('open-anim open-a');

    // Normal close enters 'closing'
    st = reduceStartState(st, false, { gen: 1 });
    expect(st).toEqual({
      phase: 'closing',
      open: false,
      closing: true,
      openSeq: 1,
      gen: 1,
    });
    expect(getStartChassisAnimClass(st)).toBe('closing');

    // Stale gen exit completion is ignored
    expect(completeStartClosing(st, 999)).toBe(st);

    // Reopening mid-close immediately cancels closing, bumps openSeq, and settles to open-b
    st = reduceStartState(st, true, { gen: 2 });
    expect(st).toEqual({
      phase: 'arming',
      open: true,
      closing: false,
      openSeq: 2,
      gen: 2,
    });
    st = settleStartOpen(st, 2);
    expect(getStartChassisAnimClass(st)).toBe('open-anim open-b');

    // Immediate preemption goes straight to hidden
    st = reduceStartState(st, { open: false, gen: 3, immediate: true });
    expect(st).toEqual({
      phase: 'hidden',
      open: false,
      closing: false,
      openSeq: 2,
      gen: 3,
    });

    // Normal close from arming or open -> completeStartClosing settles to hidden
    st = reduceStartState(st, { open: true, gen: 4 });
    st = settleStartOpen(st, 4);
    expect(getStartChassisAnimClass(st)).toBe('open-anim open-a');
    st = reduceStartState(st, { open: false, gen: 4 });
    st = completeStartClosing(st, 4);
    expect(st.phase).toBe('hidden');
    expect(st.closing).toBe(false);

    // Drag-echo guard preserves actively dragged slider values across incoming IPC pushes
    const prevAudio = {
      available: true,
      masterVolume: 74,
      masterMuted: false,
      micMuted: false,
      devices: [],
      inputDevices: [],
      sessions: [
        { id: 'chrome.exe', pid: 1, pids: [1], name: 'Chrome', volume: 42, muted: false, isActive: true },
        { id: 'discord.exe', pid: 2, pids: [2], name: 'Discord', volume: 80, muted: false, isActive: false },
      ],
    };
    const incomingAudio = {
      ...prevAudio,
      masterVolume: 60,
      sessions: [
        { ...prevAudio.sessions[0], volume: 55 },
        { ...prevAudio.sessions[1], volume: 65 },
      ],
    };
    const locks = new Map<string, number>([
      ['__master__', Infinity],
      ['chrome.exe', 1200],
      ['discord.exe', 900],
    ]);
    const merged = mergeAudioStateRespectingLocks(prevAudio, incomingAudio, locks, 1000);
    expect(merged.masterVolume).toBe(74); // locked
    expect(merged.sessions[0].volume).toBe(42); // within settle window
    expect(merged.sessions[1].volume).toBe(65); // expired lock accepts incoming
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
      title: 'workspace',
      target: 'C:\\Users\\User\\Documents\\workspace',
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
      '#818CF8',
      '#38BDF8',
      '#34D399',
      '#FB7185',
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
      spaces: [],
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
    expect(startSpacesSrc).not.toMatch(/setSessionState\(\{[\s\S]{0,160}volume:\s*nextVol\b/);
    expect(startSpacesSrc).toContain('volume: normalizeSessionVolumeScalar(nextVol)');
    expect(startSpacesSrc).not.toContain('[startVisible, space, displayedGames, loadedCovers]');
    expect(startSpacesSrc).toContain('hasSession && durationSec > 0');
    expect(startSpacesSrc).toContain('has-open-pop');
    expect(startSpacesSrc).not.toContain('+ Закрепить');
    expect(startSpacesSrc).not.toContain('+ Добавить');
    expect(startSpacesSrc).not.toContain('+ Папка / Файл / URL');
    expect(startSpacesSrc).not.toContain("style={{ position: 'absolute', inset: 0, zIndex: 20 }}");
    expect(startViewSrc).not.toContain('data-glass');
    expect(startSettingsSrc).not.toContain('glass-preset-grid');
  });

  it('normalizes per-app session volume percentages strictly to 0.0..=1.0 WASAPI scalars', () => {
    expect(normalizeSessionVolumeScalar(0)).toBe(0);
    expect(normalizeSessionVolumeScalar(1)).toBeCloseTo(0.01, 5);
    expect(normalizeSessionVolumeScalar(50)).toBeCloseTo(0.5, 5);
    expect(normalizeSessionVolumeScalar(100)).toBe(1);
    expect(normalizeSessionVolumeScalar(-15)).toBe(0);
    expect(normalizeSessionVolumeScalar(140)).toBe(1);
    expect(normalizeSessionVolumeScalar(Number.NaN)).toBe(0);
  });

  it('formats single-line telemetry ratios, sparkline ring buffer, SVG paths, disk strip & 2-click safe process kill', () => {
    expect(formatMemoryRatio(18.4, 32)).toBe('18.4 / 32 ГБ');
    expect(formatMemoryRatio(6.0, 16.0)).toBe('6 / 16 ГБ');
    expect(formatMemoryRatio(18.4, 32)).not.toContain('\n');

    expect(formatDiskSummary({ usedPct: 68, freeGb: 142.4 })).toBe('142 ГБ своб.');
    expect(formatDiskSummary({ usedPct: 95, freeGb: -1 })).toBe('0 ГБ своб.');
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
    expect(validateGameComboInput('  Dota 2 + Discord ', ['steam://rungameid/570', 'C:\\Discord.exe'])).toEqual({
      ok: true,
      name: 'Dota 2 + Discord',
      steps: ['steam://rungameid/570', 'C:\\Discord.exe'],
    });

    // Game Combo multi-target launch dispatch
    const dotaCombo = makePin({
      id: 'combo-dota2',
      kind: 'combo',
      title: 'Dota 2 + Discord',
      target: '',
      comboTargets: [
        'steam://rungameid/570',
        '"C:\\Users\\User\\AppData\\Local\\Discord\\Update.exe" --processStart Discord.exe',
        '   ',
      ],
    });
    expect(buildGameComboLaunchPayload(dotaCombo)).toEqual({
      command: 'launch_combo',
      args: {
        targets: [
          'steam://rungameid/570',
          '"C:\\Users\\User\\AppData\\Local\\Discord\\Update.exe" --processStart Discord.exe',
        ],
      },
    });
    expect(resolvePinAction(dotaCombo, false)).toEqual({
      type: 'combo',
      targets: [
        'steam://rungameid/570',
        '"C:\\Users\\User\\AppData\\Local\\Discord\\Update.exe" --processStart Discord.exe',
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

  it('supports always-on pointer drag-and-drop pin reordering across Hero <-> Compact & custom spaces and matches installed app icons', () => {
    expect(START_DRAG_THRESHOLD_PX).toBe(6);
    expect(hasExceededDragThreshold(100, 100, 103, 104)).toBe(false); // dist = 5 < 6
    expect(hasExceededDragThreshold(100, 100, 106, 100)).toBe(true); // dist = 6
    expect(hasExceededDragThreshold(100, 100, 105, 105)).toBe(true);

    // Same-space Hero <-> Compact reorder (e.g. Antigravity at idx 2 dragged onto Zen at idx 0)
    expect(
      computePointerReorderRequest({ spaceKey: 'personal', idx: 2 }, { spaceKey: 'personal', idx: 0 }),
    ).toEqual({ space: 'personal', fromIdx: 2, toIdx: 0 });
    // Compact <-> Compact reorder (e.g. Yandex Music idx 3 <-> Terminal idx 4)
    expect(
      computePointerReorderRequest({ spaceKey: 'personal', idx: 3 }, { spaceKey: 'personal', idx: 4 }),
    ).toEqual({ space: 'personal', fromIdx: 3, toIdx: 4 });
    // Cross-space or same-slot drops are ignored safely
    expect(
      computePointerReorderRequest({ spaceKey: 'personal', idx: 0 }, { spaceKey: 'personal_links', idx: 1 }),
    ).toBeNull();
    expect(
      computePointerReorderRequest({ spaceKey: 'personal', idx: 2 }, { spaceKey: 'personal', idx: 2 }),
    ).toBeNull();

    const cfg: TestBarConfig = {
      schemaVersion: 1,
      revision: 12,
      barMode: 'dock',
      secondaryTaskbarMode: 'testbar',
      autostart: false,
      taskbarPins: [],
      spaces: [],
      personalPins: [
        makePin({ id: 'zen', title: 'Zen', target: 'C:\\Apps\\zen.exe' }),
        makePin({ id: 'tg', title: 'Telegram', target: 'C:\\Apps\\Telegram.exe' }),
        makePin({ id: 'agy', title: 'Antigravity', target: 'C:\\Apps\\Antigravity.exe' }),
        makePin({ id: 'ym', title: 'Яндекс Музыка', target: 'C:\\Apps\\ym.exe' }),
        makePin({ id: 'wt', title: 'Терминал', target: 'C:\\Apps\\wt.exe' }),
      ],
      personalLinks: [],
      workFolders: [],
      workApps: [],
      studioPins: [],
      studioTools: [],
      gameLaunchers: [],
      gameCombos: [],
      customSpaces: [
        {
          id: 'cs-1',
          name: 'Dev',
          accentHex: '#A78BFA',
          accentRgb: '167,139,250',
          widgetPreset: 'full_grid',
          items: [
            makePin({ id: 'a', title: 'A', target: 'C:\\a.exe' }),
            makePin({ id: 'b', title: 'B', target: 'C:\\b.exe' }),
          ],
        },
      ],
      pinnedClipboard: [],
    };

    const reorderedHero = applyOptimisticPinReorder(cfg, 'personal', 2, 0);
    expect(reorderedHero?.personalPins.map((p) => p.id)).toEqual(['agy', 'zen', 'tg', 'ym', 'wt']);
    expect(reorderedHero?.revision).toBe(12); // Keeps revision intact for authoritative Rust IPC response

    const reorderedCompact = applyOptimisticPinReorder(reorderedHero, 'personal', 3, 4);
    expect(reorderedCompact?.personalPins.map((p) => p.id)).toEqual(['agy', 'zen', 'tg', 'wt', 'ym']);

    const reorderedCustom = applyOptimisticPinReorder(cfg, 'cs-1', 1, 0);
    expect(reorderedCustom?.customSpaces[0].items.map((p) => p.id)).toEqual(['b', 'a']);

    // Installed app icon matching fallback for Start tiles
    const installed = [
      {
        id: 'app-wt',
        name: 'Terminal',
        target: 'shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
        aumid: 'Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
        category: 'Система',
        iconKey: 'uwp:wt123',
      },
    ];
    expect(
      findMatchingInstalledAppForPin(
        makePin({
          id: 'wt',
          title: 'Терминал',
          target: 'shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
        }),
        installed,
      )?.iconKey,
    ).toBe('uwp:wt123');
  });

  it('resolves modular config.spaces with legacy fallback, pin lookup, unified mixed reorder & validation', () => {
    const modularSpaces: SpaceConfig[] = [
      {
        id: 'personal',
        name: 'Дом',
        accentHex: '#818CF8',
        accentRgb: '129, 140, 248',
        layoutKind: 'grid',
        showHeroRow: true,
        widgets: ['media', 'weather', 'batteries'],
        items: [
          makePin({ id: 'app-1', kind: 'app', title: 'Zen', target: 'C:\\Apps\\zen.exe' }),
          makePin({ id: 'fld-1', kind: 'folder', title: 'Проекты', target: 'D:\\Projects' }),
          makePin({ id: 'url-1', kind: 'url', title: 'Docs', target: 'https://example.com' }),
        ],
      },
      {
        id: 'sp-custom',
        name: 'Лаборатория',
        accentHex: '#34D399',
        accentRgb: '52, 211, 153',
        layoutKind: 'grid',
        showHeroRow: false,
        widgets: [],
        items: [],
      },
    ];

    const v2Cfg: TestBarConfig = {
      schemaVersion: 2,
      revision: 25,
      barMode: 'dock',
      secondaryTaskbarMode: 'native',
      autostart: true,
      taskbarPins: [],
      spaces: modularSpaces,
      personalPins: [makePin({ id: 'legacy', title: 'Legacy', target: 'C:\\legacy.exe' })],
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

    const resolved = resolveConfiguredSpaces(v2Cfg);
    expect(resolved).toHaveLength(2);
    expect(resolved[0].name).toBe('Дом');
    expect(getSpaceDisplayName('personal', v2Cfg)).toBe('Дом');
    expect(getSpacePinList('personal', v2Cfg).map((p) => p.id)).toEqual(['app-1', 'fld-1', 'url-1']);
    expect(
      getPinnedItemInSpace({ name: 'Zen', target: 'c:\\apps\\zen.exe' }, 'personal', v2Cfg)?.id,
    ).toBe('app-1');

    // Mixed grid reorder across folder + app + url pins directly updates config.spaces
    const reordered = applyOptimisticPinReorder(v2Cfg, 'personal', 2, 0);
    expect(reordered?.spaces[0].items.map((p) => p.id)).toEqual(['url-1', 'app-1', 'fld-1']);
    expect(reordered?.revision).toBe(25);

    // Fallback when config.spaces is empty synthesizes built-ins + customSpaces
    const legacyCfg: TestBarConfig = {
      ...v2Cfg,
      spaces: [],
      customSpaces: [
        {
          id: 'cs-9',
          name: 'Музыка',
          accentHex: '#F472B6',
          accentRgb: '244, 114, 182',
          widgetPreset: 'audio_mic',
          items: [makePin({ id: 'daw', title: 'DAW', target: 'C:\\daw.exe' })],
        },
      ],
    };
    const fallbackSpaces = resolveConfiguredSpaces(legacyCfg);
    expect(fallbackSpaces.map((s) => s.id)).toEqual(['personal', 'work', 'studio', 'games', 'cs-9']);
    expect(fallbackSpaces[0].items.map((p) => p.id)).toEqual(['legacy']);
    expect(fallbackSpaces[4].widgets).toEqual(['audio_mixer', 'media']);

    expect(SPACE_TEMPLATES).toHaveLength(5);
    expect(SPACE_WIDGET_CATALOG).toHaveLength(12);
    expect(SPACE_WIDGET_CATALOG.map((w) => w.id)).toEqual([
      'media',
      'audio_mixer',
      'clipboard',
      'pc_health',
      'top_processes',
      'game_combos',
      'weather',
      'batteries',
      'notes',
      'timer',
      'network',
      'quick_toggles',
    ]);
    expect(MODULAR_SPACE_ACCENTS.length).toBeGreaterThanOrEqual(6);

    expect(validateSpaceName('  Код  ', ['Дом', 'Работа'])).toEqual({ ok: true, name: 'Код' });
    expect(validateSpaceName('   ', ['Дом']).ok).toBe(false);
    expect(validateSpaceName('дом', ['Дом']).ok).toBe(false);
    expect(validateSpaceName('Дом', ['Дом', 'Работа'], 'Дом')).toEqual({ ok: true, name: 'Дом' });
    expect(validateSpaceName('Слишком длинное имя пространства', [])).toEqual({
      ok: false,
      error: 'Максимум 18 символов',
    });

    expect(moveSpaceIdInOrder(['personal', 'work', 'studio'], 'work', 'left')).toEqual([
      'work',
      'personal',
      'studio',
    ]);
    expect(moveSpaceIdInOrder(['personal', 'work'], 'personal', 'left')).toBeNull();
    expect(moveSpaceIdInOrder(['personal', 'work'], 'work', 'right')).toBeNull();
  });

  it('handles custom tile RGB color parsing, widget builder toggles/order, weather/battery & dynamic PC health', () => {
    expect(TILE_COLOR_PALETTE).toHaveLength(10);
    expect(resolveCustomColorRgb('#52B8F2')).toBe('82, 184, 242');
    expect(resolveCustomColorRgb('#fff')).toBe('255, 255, 255');
    expect(resolveCustomColorRgb(' 167, 139, 250 ')).toBe('167, 139, 250');
    expect(resolveCustomColorRgb('999, 0, 0')).toBeNull();
    expect(resolveCustomColorRgb('not-a-color')).toBeNull();
    expect(normalizeRgbTriplet('52, 211, 153')).toBe('52, 211, 153');

    const w0: TestBarConfig['spaces'][number]['widgets'] = ['media', 'weather'];
    expect(toggleSpaceWidget(w0, 'batteries')).toEqual(['media', 'weather', 'batteries']);
    expect(toggleSpaceWidget(w0, 'media')).toEqual(['weather']);
    expect(w0).toEqual(['media', 'weather']); // Pure / non-mutating

    expect(moveSpaceWidget(['media', 'weather', 'batteries'], 'batteries', 'up')).toEqual([
      'media',
      'batteries',
      'weather',
    ]);
    expect(moveSpaceWidget(['media', 'weather'], 'media', 'up')).toEqual(['media', 'weather']);

    expect(isLowBatteryPercent(19)).toBe(true);
    expect(isLowBatteryPercent(20)).toBe(false);
    expect(isLowBatteryPercent(Number.NaN)).toBe(false);
    expect(formatPeripheralSourceLabel('hidpp')).toBe('Logitech HID');
    expect(formatPeripheralSourceLabel('bluetooth')).toBe('Bluetooth');
    expect(formatPeripheralSourceLabel('razer_hid')).toBe('Razer HID');

    expect(convertTempFromC(0, 'f')).toBe(32);
    expect(convertTempFromC(25, 'c')).toBe(25);
    expect(formatStartTemperature(21.6, 'c')).toBe('22°C');
    expect(formatStartTemperature(20, 'f')).toBe('68°F');
    expect(formatStartTemperature(Number.NaN, 'f')).toBe('—°F');

    expect(
      formatHardwareSummaryLine({
        ramTotalGb: 31.8,
        gpuAvailable: true,
        gpuName: 'AMD Radeon RX 7800 XT',
        vramTotalGb: 16,
      }),
    ).toBe('32 ГБ ОЗУ · AMD Radeon RX 7800 XT · 16 ГБ');
    expect(
      formatHardwareSummaryLine({
        ramTotalGb: 16,
        gpuAvailable: false,
        gpuName: '',
        vramTotalGb: 0,
      }),
    ).toBe('16 ГБ ОЗУ');
    expect(formatHardwareSummaryLine(null)).toBe('Система');
  });

  it('keeps V3 Start sources free of personal names, hardcoded GPU/VRAM/RAM literals, and wires explorer slot + styles', () => {
    expect(startSpacesSrc).toContain("import '../../styles/spaces-v3.css'");
    expect(startViewSrc).toContain('v3-explorer-slot');
    expect(startViewSrc).toContain('data-explorer-path');
    expect(startViewSrc).toContain('<StartFileExplorer');
    for (const [relPath, content] of [
      ['StartView.tsx', startViewSrc],
      ['StartHeader.tsx', startHeaderSrc],
      ['StartSpaces.tsx', startSpacesSrc],
    ] as const) {
      expect(content, `${relPath} must not contain hardcoded RTX 4090`).not.toContain('RTX 4090');
      expect(content, `${relPath} must not contain hardcoded PoE 2`).not.toContain('PoE 2');
      expect(content, `${relPath} must not contain Exiled`).not.toContain('Exiled');
      expect(content, `${relPath} must not hardcode 64 ГБ or 24 ГБ`).not.toMatch(/\b(64|24)\s*ГБ\b/);
    }
  });

  it('partitions space items into mainItems and bottomItems while preserving relative order', () => {
    const items: PinItem[] = [
      makePin({ id: 'a', title: 'Alpha', target: 'alpha.exe' }),
      makePin({ id: 'b', title: 'Beta', target: 'beta.exe', section: 'bottom' }),
      makePin({ id: 'c', title: 'Gamma', target: 'gamma.exe', section: 'main', tileSize: 'hero' }),
      makePin({ id: 'd', title: 'Delta', target: 'delta.exe', section: 'bottom' }),
    ];

    const res = partitionSpaceItemsBySection(items);
    expect(res.mainItems.map((p) => p.id)).toEqual(['a', 'c']);
    expect(res.bottomItems.map((p) => p.id)).toEqual(['b', 'd']);
    expect(partitionSpaceItemsBySection([])).toEqual({ mainItems: [], bottomItems: [] });
  });

  it('evaluates 3-minute Explorer resume TTL boundaries cleanly', () => {
    expect(EXPLORER_RESUME_TTL_MS).toBe(180_000);
    expect(shouldResetExplorerOnReopen(null, 500_000)).toBe(false);
    expect(shouldResetExplorerOnReopen(undefined, 500_000)).toBe(false);
    expect(shouldResetExplorerOnReopen(Number.NaN, 500_000)).toBe(false);

    const closedAt = 1_000_000;
    expect(shouldResetExplorerOnReopen(closedAt, closedAt + 179_999)).toBe(false);
    expect(shouldResetExplorerOnReopen(closedAt, closedAt + 180_000)).toBe(true);
    expect(shouldResetExplorerOnReopen(closedAt, closedAt + 240_000)).toBe(true);
    expect(shouldResetExplorerOnReopen(closedAt, closedAt + 1000, 0)).toBe(true);
  });

  it('filters hidden battery devices case-insensitively without mutating source order', () => {
    const devices = [
      { id: 'Logi-GPRO', name: 'Mouse', percent: 82 },
      { id: 'BT-Headset-1', name: 'Headset', percent: 45 },
      { id: 'KB-915', name: 'Keyboard', percent: 91 },
    ];

    expect(filterVisibleBatteries(devices, undefined).map((d) => d.id)).toEqual([
      'Logi-GPRO',
      'BT-Headset-1',
      'KB-915',
    ]);
    expect(filterVisibleBatteries(devices, []).map((d) => d.id)).toEqual([
      'Logi-GPRO',
      'BT-Headset-1',
      'KB-915',
    ]);
    expect(filterVisibleBatteries(devices, ['  bt-headset-1  ']).map((d) => d.id)).toEqual([
      'Logi-GPRO',
      'KB-915',
    ]);
    expect(filterVisibleBatteries([], ['Logi-GPRO'])).toEqual([]);
  });

  it('optimistically moves pins within and across main/bottom sections by ID and honors runAsAdmin', () => {
    const baseConfig: TestBarConfig = {
      schemaVersion: 2,
      revision: 1,
      barMode: 'dock',
      secondaryTaskbarMode: 'native',
      autostart: true,
      taskbarPins: [],
      spaces: [
        {
          id: 'personal',
          name: 'Личное',
          accentHex: '#818CF8',
          accentRgb: '129, 140, 248',
          layoutKind: 'grid',
          showHeroRow: false,
          widgets: [],
          items: [
            makePin({ id: 'p1', title: 'Code', target: 'code.exe', section: 'main', tileSize: 'hero' }),
            makePin({ id: 'p2', title: 'Notes', target: 'notes.exe', section: 'main' }),
            makePin({ id: 'p3', title: 'Docs', target: 'C:\\Docs', kind: 'folder', section: 'bottom' }),
          ],
        },
      ],
      personalPins: [],
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

    const movedToBottom = applyOptimisticMovePinInSpace(baseConfig, {
      space: 'personal',
      fromId: 'p2',
      toId: null,
      targetSection: 'bottom',
    });
    expect(movedToBottom).not.toBeNull();
    const afterBottom = movedToBottom!.spaces[0].items;
    expect(afterBottom.find((p) => p.id === 'p2')?.section).toBe('bottom');
    expect(afterBottom.map((p) => p.id)).toEqual(['p1', 'p3', 'p2']);

    const movedBackToMain = applyOptimisticMovePinInSpace(movedToBottom, {
      space: 'personal',
      fromId: 'p3',
      toId: 'p1',
      targetSection: 'main',
    });
    expect(movedBackToMain).not.toBeNull();
    expect(movedBackToMain!.spaces[0].items.map((p) => p.id)).toEqual(['p3', 'p1', 'p2']);
    expect(movedBackToMain!.spaces[0].items[0].section).toBe('main');

    expect(
      applyOptimisticMovePinInSpace(baseConfig, {
        space: 'personal',
        fromId: 'missing',
        toId: null,
        targetSection: 'bottom',
      }),
    ).toBeNull();

    const adminPin = makePin({
      id: 'adm',
      title: 'Regedit',
      target: 'C:\\Windows\\regedit.exe',
      runAsAdmin: true,
    });
    const action = resolvePinAction(adminPin, false);
    expect(action.type).toBe('launch');
    if (action.type === 'launch') {
      expect(Boolean(action.asAdmin || adminPin.runAsAdmin)).toBe(true);
    }
  });

  it('wires header weather pill, empty-space context menu, mixed grid, and centered tile edit modal', () => {
    expect(startHeaderSrc).toContain('weather-hdr-pill');
    expect(startHeaderSrc).toContain('weather-hdr-pop');
    expect(startHeaderSrc).toContain('periph-hide-btn');
    expect(startSpacesSrc).toContain('mixed-tile-grid');
    expect(startSpacesSrc).toContain('space-dock-foot');
    expect(startSpacesSrc).toContain('periph-hide-btn');
    expect(startViewSrc).toContain('empty-ctx-menu');
    expect(startViewSrc).toContain('tile-edit-backdrop');
    expect(startViewSrc).toContain('tile-edit-modal');
    expect(startViewSrc).toContain('shouldResetExplorerOnReopen');
  });

  it('plans header widget slots without overflowing left/right pockets and reserves 38px on overflow', () => {
    const widths = {
      batteries: 90,
      weather: 64,
      timer: 76,
      network: 96,
      notes: 84,
    };

    // Exact fit with zero overflow (no 38px badge reservation)
    const exact = planHeaderWidgetSlots(
      [
        { id: 'batteries', side: 'left' },
        { id: 'weather', side: 'left' },
        { id: 'timer', side: 'right' },
      ],
      90 + HEADER_WIDGET_GAP_PX + 64,
      76,
      widths,
    );
    expect(exact.left.map((e) => e.id)).toEqual(['batteries', 'weather']);
    expect(exact.right.map((e) => e.id)).toEqual(['timer']);
    expect(exact.overflow).toEqual([]);
    expect(exact.overflowSide).toBeNull();
    expect(exact.leftUsedPx).toBe(160);
    expect(exact.rightUsedPx).toBe(76);

    // 'auto' spills left-first then right without overflowing either pocket
    const autoSplit = planHeaderWidgetSlots(
      [
        { id: 'batteries', side: 'auto' },
        { id: 'weather', side: 'auto' },
        { id: 'timer', side: 'auto' },
        { id: 'network', side: 'auto' },
      ],
      170,
      190,
      widths,
    );
    expect(autoSplit.left.map((e) => e.id)).toEqual(['batteries', 'weather']);
    expect(autoSplit.right.map((e) => e.id)).toEqual(['timer', 'network']);
    expect(autoSplit.overflow).toEqual([]);

    // Explicit 'left' never jumps to right pocket; overflow reserves 38px badge inside pocket
    const leftOverflow = planHeaderWidgetSlots(
      [
        { id: 'batteries', side: 'left' },
        { id: 'weather', side: 'left' },
        { id: 'timer', side: 'left' },
      ],
      165,
      200,
      widths,
    );
    // 90 + 64 + 6 = 160 fits without badge, but 3rd widget overflows so 38px+6px must be reserved:
    // 90 + 6 + 64 + 6 + 38 = 204 > 165 -> weather also moves to overflow, leaving batteries (90) + gap (6) + badge (38) = 134 <= 165
    expect(leftOverflow.left.map((e) => e.id)).toEqual(['batteries']);
    expect(leftOverflow.right).toEqual([]);
    expect(leftOverflow.overflow.map((e) => e.id)).toEqual(['weather', 'timer']);
    expect(leftOverflow.overflowSide).toBe('left');
    expect(leftOverflow.leftUsedPx).toBe(90 + HEADER_WIDGET_GAP_PX + HEADER_OVERFLOW_BADGE_PX);
    expect(leftOverflow.leftUsedPx).toBeLessThanOrEqual(165);

    // Auto overflow reserves 38px on right when right holds items
    const autoOverflow = planHeaderWidgetSlots(
      [
        { id: 'batteries', side: 'auto' },
        { id: 'weather', side: 'auto' },
        { id: 'timer', side: 'auto' },
        { id: 'network', side: 'auto' },
        { id: 'notes', side: 'auto' },
      ],
      180,
      190,
      widths,
    );
    expect(autoOverflow.leftUsedPx).toBeLessThanOrEqual(180);
    expect(autoOverflow.rightUsedPx).toBeLessThanOrEqual(190);
    expect(autoOverflow.overflowSide).toBe('right');
    expect(autoOverflow.overflow.length).toBeGreaterThanOrEqual(2);
    expect(autoOverflow.left.length + autoOverflow.right.length + autoOverflow.overflow.length).toBe(5);

    // Zero / negative available px never overflows
    const zeroSpace = planHeaderWidgetSlots(
      [{ id: 'weather', side: 'auto' }],
      -10,
      0,
      widths,
    );
    expect(zeroSpace.left).toEqual([]);
    expect(zeroSpace.right).toEqual([]);
    expect(zeroSpace.overflow.map((e) => e.id)).toEqual(['weather']);
    expect(zeroSpace.overflowSide).toBeNull();
    expect(zeroSpace.leftUsedPx).toBe(0);
    expect(zeroSpace.rightUsedPx).toBe(0);
  });

  it('maintains mutual exclusivity between side widgets and headerWidgets and reorders header widgets', () => {
    const startSide: TestBarConfig['spaces'][number]['widgets'] = ['media', 'audio_mixer'];
    const startHeader = [{ id: 'batteries' as const, side: 'left' as const }];

    // Placing 'media' in top header removes it from side widgets
    const movedToTop = setSpaceWidgetPlacement(startSide, startHeader, 'media', 'top', 'right');
    expect(movedToTop.widgets).toEqual(['audio_mixer']);
    expect(movedToTop.headerWidgets).toEqual([
      { id: 'batteries', side: 'left' },
      { id: 'media', side: 'right' },
    ]);
    expect(getSpaceWidgetPlacement(movedToTop.widgets, movedToTop.headerWidgets, 'media')).toEqual({
      placement: 'top',
      side: 'right',
    });

    // Updating side on already-top widget preserves its position in headerWidgets
    const changedSide = setSpaceWidgetPlacement(
      movedToTop.widgets,
      movedToTop.headerWidgets,
      'batteries',
      'top',
      'auto',
    );
    expect(changedSide.headerWidgets).toEqual([
      { id: 'batteries', side: 'auto' },
      { id: 'media', side: 'right' },
    ]);

    // Reordering inside headerWidgets
    const reorderedHeader = moveHeaderWidget(changedSide.headerWidgets, 'media', 'up');
    expect(reorderedHeader.map((h) => h.id)).toEqual(['media', 'batteries']);
    expect(setHeaderWidgetSide(reorderedHeader, 'media', 'left')).toEqual([
      { id: 'media', side: 'left' },
      { id: 'batteries', side: 'auto' },
    ]);

    // Placing 'batteries' into side rail removes it from headerWidgets
    const movedToSide = setSpaceWidgetPlacement(
      changedSide.widgets,
      reorderedHeader,
      'batteries',
      'side',
    );
    expect(movedToSide.widgets).toEqual(['audio_mixer', 'batteries']);
    expect(movedToSide.headerWidgets).toEqual([{ id: 'media', side: 'right' }]);

    // Turning off removes from both
    const turnedOff = setSpaceWidgetPlacement(
      movedToSide.widgets,
      movedToSide.headerWidgets,
      'media',
      'off',
    );
    expect(turnedOff.widgets).toEqual(['audio_mixer', 'batteries']);
    expect(turnedOff.headerWidgets).toEqual([]);

    // resolveSpaceHeaderWidgets respects explicit empty v4 array vs pre-v4 undefined fallback
    expect(resolveSpaceHeaderWidgets({ widgets: ['weather'], headerWidgets: undefined })).toEqual([
      { id: 'batteries', side: 'left' },
    ]);
    expect(resolveSpaceHeaderWidgets({ widgets: ['audio_mixer'], headerWidgets: [] })).toEqual([]);
  });
});
