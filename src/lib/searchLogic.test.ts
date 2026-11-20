import { describe, expect, it } from 'vitest';
import type { IndexedApp, IndexedFile, PinItem, SteamGame, TestBarConfig } from './ipc';
import {
  buildSearchCorpus,
  isPrintableSearchTrigger,
  rankSearchResults,
  resolveSearchLaunchPayload,
} from './searchLogic';

function makePin(overrides: Partial<PinItem> & Pick<PinItem, 'id' | 'title' | 'target'>): PinItem {
  return {
    kind: 'app',
    subtitle: 'App',
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

function makeConfig(): TestBarConfig {
  return {
    schemaVersion: 1,
    revision: 1,
    barMode: 'dock',
    secondaryTaskbarMode: 'testbar',
    autostart: false,
    taskbarPins: [],
    personalPins: [
      makePin({
        id: 'tg',
        title: 'Telegram',
        subtitle: 'Мессенджер',
        target: 'C:\\Users\\user\\AppData\\Roaming\\Telegram Desktop\\Telegram.exe',
      }),
    ],
    personalLinks: [
      makePin({
        id: 'lnk-dl',
        kind: 'folder',
        title: 'Загрузки',
        subtitle: 'Папка',
        target: 'C:\\Users\\user\\Downloads',
      }),
    ],
    workFolders: [
      makePin({
        id: 'wf-tb',
        kind: 'folder',
        title: 'testbar-workspace',
        subtitle: 'Проект',
        target: 'C:\\Users\\user\\Documents\\testbar',
      }),
    ],
    workApps: [
      makePin({
        id: 'wa-cur',
        title: 'Cursor',
        subtitle: 'Редактор',
        target: 'C:\\Users\\user\\AppData\\Local\\Programs\\cursor\\Cursor.exe',
      }),
    ],
    studioPins: [
      makePin({
        id: 'ps',
        title: 'Photoshop 2025',
        subtitle: 'Графика',
        target: 'C:\\Program Files\\Adobe\\Photoshop.exe',
      }),
    ],
    studioTools: [
      makePin({
        id: 'wiz',
        title: 'WizTree',
        subtitle: 'Диски',
        target: 'C:\\Program Files\\WizTree\\WizTree64.exe',
      }),
    ],
    gameLaunchers: [
      makePin({
        id: 'bnet',
        title: 'Battle.net',
        subtitle: 'Лаунчер',
        target: 'C:\\Program Files (x86)\\Battle.net\\Battle.net Launcher.exe',
      }),
    ],
    gameCombos: [
      makePin({
        id: 'combo-poe',
        kind: 'combo',
        title: 'PoE 2 + Trade',
        subtitle: 'Связка',
        target: '',
        comboTargets: ['steam://rungameid/2694490', 'C:\\Tools\\ExiledExchange.exe'],
      }),
    ],
    customSpaces: [
      {
        id: 'custom-1',
        name: 'Финансы',
        accentHex: '#34D399',
        accentRgb: '52,211,153',
        items: [
          makePin({
            id: 'fin-url',
            kind: 'url',
            title: 'Портфель Акций',
            subtitle: 'Ссылка',
            target: 'https://portfolio.local',
          }),
        ],
      },
    ],
    pinnedClipboard: [],
  };
}

const sampleApps: IndexedApp[] = [
  {
    id: 'app-1',
    name: 'Notion',
    category: 'Программы',
    target: 'C:\\Users\\user\\AppData\\Local\\Programs\\Notion\\Notion.exe',
    aumid: null,
    iconKey: 'exe:0000000000000001',
    iconData: null,
  },
  {
    id: 'uwp-1',
    name: 'Calculator',
    category: 'Система',
    target: 'shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
    aumid: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
    iconKey: 'aumid:0000000000000002',
    iconData: null,
  },
  {
    id: 'uwp-2',
    name: 'Windows Terminal',
    category: 'Разработка',
    target: 'shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
    aumid: 'Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
    iconKey: 'aumid:0000000000000003',
    iconData: null,
  },
];

const sampleGames: SteamGame[] = [
  {
    appId: '2694490',
    name: 'Path of Exile 2',
    launcher: 'Steam',
    target: 'steam://rungameid/2694490',
    coverData: null,
    lastUpdated: 1730000000,
  },
];

const sampleFiles: IndexedFile[] = [
  {
    id: 'ufile-0',
    name: 'Quarterly-Report-2026.pdf',
    kind: 'file',
    location: 'Документы',
    target: 'C:\\Users\\user\\Documents\\Quarterly-Report-2026.pdf',
    modifiedTs: 1730000100,
  },
  {
    id: 'ufile-1',
    name: 'Дизайн-Макеты',
    kind: 'folder',
    location: 'Рабочий стол',
    target: 'C:\\Users\\user\\Desktop\\Дизайн-Макеты',
    modifiedTs: 1730000050,
  },
];

describe('searchLogic corpus, RU<->EN fuzzy ranking & keyboard dispatch', () => {
  it('builds a unified corpus across ALL Spaces, customSpaces, UWP apps, Steam games & user files', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {
        'aumid:0000000000000002': 'data:image/png;base64,CALC',
      },
    });

    const names = corpus.map((c) => c.name);
    expect(names).toContain('Telegram'); // personalPins
    expect(names).toContain('Загрузки'); // personalLinks
    expect(names).toContain('testbar-workspace'); // workFolders
    expect(names).toContain('Cursor'); // workApps
    expect(names).toContain('Photoshop 2025'); // studioPins
    expect(names).toContain('WizTree'); // studioTools
    expect(names).toContain('Battle.net'); // gameLaunchers
    expect(names).toContain('PoE 2 + Trade'); // gameCombos
    expect(names).toContain('Портфель Акций'); // customSpaces
    expect(names).toContain('Path of Exile 2'); // Steam games
    expect(names).toContain('Notion'); // Win32 installed app
    expect(names).toContain('Calculator'); // UWP packaged app
    expect(names).toContain('Quarterly-Report-2026.pdf'); // User file
    expect(names).toContain('Дизайн-Макеты'); // User folder

    const calc = corpus.find((c) => c.name === 'Calculator');
    expect(calc?.source).toBe('uwp');
    expect(calc?.iconData).toBe('data:image/png;base64,CALC');
  });

  it('matches RU<->EN wrong-keyboard-layout queries including ntkt -> Telegram and rfkmr -> Calculator', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
    });

    // "ntkt" (EN layout for "теле") -> Telegram
    const tgHit = rankSearchResults(corpus, 'ntkt');
    expect(tgHit.items.length).toBeGreaterThan(0);
    expect(tgHit.items[0].name).toBe('Telegram');
    expect(tgHit.swappedHint).toBe('теле');

    // "rfkmr" (EN layout for "кальк") -> UWP Calculator
    const calcHit = rankSearchResults(corpus, 'rfkmr');
    expect(calcHit.items.length).toBeGreaterThan(0);
    expect(calcHit.items[0].name).toBe('Calculator');
    expect(calcHit.swappedHint).toBe('кальк');

    // "йгфкеукдн" (RU layout for "quarterly") -> user file Quarterly-Report-2026.pdf
    const fileHit = rankSearchResults(corpus, 'йгфкеукдн');
    expect(fileHit.items.length).toBeGreaterThan(0);
    expect(fileHit.items[0].name).toBe('Quarterly-Report-2026.pdf');
    expect(fileHit.swappedHint).toBe('quarterly');

    // "gjhnаtkm" or "gjhn" (EN layout for "порт") -> customSpace URL "Портфель Акций"
    const customHit = rankSearchResults(corpus, 'gjhn');
    expect(customHit.items.some((i) => i.name === 'Портфель Акций')).toBe(true);
  });

  it('ranks prefix and pinned matches above substring matches', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: [
        ...sampleApps,
        {
          id: 'app-sub',
          name: 'My Cursor Helper',
          category: 'Программы',
          target: 'C:\\Tools\\MyCursorHelper.exe',
          aumid: null,
          iconKey: null,
          iconData: null,
        },
        {
          id: 'app-sub2',
          name: 'Cursorless Extension',
          category: 'Разработка',
          target: 'C:\\Tools\\Cursorless.exe',
          aumid: null,
          iconKey: null,
          iconData: null,
        },
      ],
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
    });

    const ranked = rankSearchResults(corpus, 'cursor');
    expect(ranked.items[0].name).toBe('Cursor'); // exact + pinned
    expect(ranked.items[1].name).toBe('Cursorless Extension'); // prefix
    expect(ranked.items[2].name).toBe('My Cursor Helper'); // word-start
  });

  it('dispatches Enter, Ctrl+Enter (admin-safe), and combo payloads accurately', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
    });

    const tg = corpus.find((c) => c.name === 'Telegram')!;
    expect(resolveSearchLaunchPayload(tg, false)).toEqual({
      command: 'launch_item',
      args: { target: tg.target, args: null, asAdmin: false, closeStart: true },
    });
    expect(resolveSearchLaunchPayload(tg, true)).toEqual({
      command: 'launch_item',
      args: { target: tg.target, args: null, asAdmin: true, closeStart: true },
    });

    // UWP shell:AppsFolder target must never pass runas verb on Ctrl+Enter
    const calc = corpus.find((c) => c.name === 'Calculator')!;
    expect(resolveSearchLaunchPayload(calc, true)).toEqual({
      command: 'launch_item',
      args: { target: calc.target, args: null, asAdmin: false, closeStart: true },
    });

    // Combo item dispatches launch_combo with all targets
    const combo = corpus.find((c) => c.name === 'PoE 2 + Trade')!;
    expect(resolveSearchLaunchPayload(combo, false)).toEqual({
      command: 'launch_combo',
      args: { targets: ['steam://rungameid/2694490', 'C:\\Tools\\ExiledExchange.exe'] },
    });
  });

  it('activates type-anywhere search on single printable keys without hijacking shortcuts or Space', () => {
    expect(isPrintableSearchTrigger({ key: 't', ctrlKey: false, altKey: false, metaKey: false })).toBe(true);
    expect(isPrintableSearchTrigger({ key: 'т', ctrlKey: false, altKey: false, metaKey: false })).toBe(true);
    expect(isPrintableSearchTrigger({ key: '2', ctrlKey: false, altKey: false, metaKey: false })).toBe(true);
    expect(isPrintableSearchTrigger({ key: ' ', ctrlKey: false, altKey: false, metaKey: false })).toBe(false);
    expect(isPrintableSearchTrigger({ key: 'c', ctrlKey: true, altKey: false, metaKey: false })).toBe(false);
    expect(isPrintableSearchTrigger({ key: 'Tab', ctrlKey: false, altKey: false, metaKey: false })).toBe(false);
    expect(isPrintableSearchTrigger({ key: 'Shift', ctrlKey: false, altKey: false, metaKey: false })).toBe(false);
  });
});
