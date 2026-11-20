/// <reference types="vite/client" />
import { describe, expect, it } from 'vitest';
import spotCardSrc from '../components/start/SpotCard.tsx?raw';
import startCommandPaletteSrc from '../components/start/StartCommandPalette.tsx?raw';
import startHeaderSrc from '../components/start/StartHeader.tsx?raw';
import startPinDrawerSrc from '../components/start/StartPinDrawer.tsx?raw';
import startSettingsSrc from '../components/start/StartSettingsOverlay.tsx?raw';
import startSpacesSrc from '../components/start/StartSpaces.tsx?raw';
import startViewSrc from '../components/StartView.tsx?raw';
import type { PinItem, TestBarConfig } from './ipc';
import {
  computeSlidingPillRect,
  CUSTOM_SPACE_ACCENTS,
  findBannedStartArtifacts,
  INITIAL_START_ANIM_STATE,
  isAppPinnedInSpace,
  normalizeUrlPinInput,
  partitionHeroAndCompact,
  reducePowerClick,
  reduceStartState,
  resolvePinAction,
  validateCustomSpaceName,
} from './startLogic';

function makePin(overrides: Partial<PinItem> & Pick<PinItem, 'id' | 'title' | 'target'>): PinItem {
  return {
    kind: 'app',
    subtitle: 'Sub',
    altAction: 'От админа',
    accentRgb: '129,140,248',
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
  it('replays enter spring animation via openSeq on every open and enters closing phase cleanly', () => {
    let st = INITIAL_START_ANIM_STATE;
    st = reduceStartState(st, true);
    expect(st).toEqual({ phase: 'open', openSeq: 1 });

    st = reduceStartState(st, false);
    expect(st).toEqual({ phase: 'closing', openSeq: 1 });

    // Reopening mid-close immediately cancels closing and bumps openSeq for fresh spring animation
    st = reduceStartState(st, true);
    expect(st).toEqual({ phase: 'open', openSeq: 2 });
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

  it('resolves Shift alternate actions faithfully for every displayed badge', () => {
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

    // 6. Terminal / OBS -> От админа
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

    // 7. Discord Squirrel args preserved on normal & Shift fallback launch
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

    // First click on reboot arms it without executing
    const armReboot = reducePowerClick(null, 'reboot');
    expect(armReboot).toEqual({
      execute: null,
      nextArmed: 'reboot',
      closePower: false,
    });

    // Clicking shutdown while reboot is armed switches arm to shutdown instead of executing!
    const switchArm = reducePowerClick(armReboot.nextArmed, 'shutdown');
    expect(switchArm).toEqual({
      execute: null,
      nextArmed: 'shutdown',
      closePower: false,
    });

    // Second click on shutdown executes and closes capsule
    expect(reducePowerClick(switchArm.nextArmed, 'shutdown')).toEqual({
      execute: 'shutdown',
      nextArmed: null,
      closePower: true,
    });
  });

  it('validates floating custom-space creation, curated dark accents, sliding rect & URL input', () => {
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

  it('enforces zero footer hints, zero banned emoji/glyph hacks, and zero pin .slice() clipping across Start components', () => {
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
    }
  });
});
