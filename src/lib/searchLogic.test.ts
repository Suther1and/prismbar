import { describe, expect, it } from 'vitest';
import type {
  AppWindowGroup,
  AudioState,
  ClipboardEntry,
  FrecencyEntry,
  IndexedApp,
  IndexedFile,
  PinItem,
  SteamGame,
  TestBarConfig,
} from './ipc';
import {
  buildPinFromSearchItem,
  buildSearchCorpus,
  getPrimaryActionLabel,
  isPrintableSearchTrigger,
  matchSubsequence,
  rankSearchResults,
  resolveSearchLaunchPayload,
  resolveSearchSecondaryAction,
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
    spaces: [],
    personalPins: [
      makePin({
        id: 'tg',
        title: 'Telegram',
        subtitle: 'Мессенджер',
        target: 'C:\\Users\\user\\AppData\\Roaming\\Telegram Desktop\\Telegram.exe',
      }),
      makePin({
        id: 'figma-pin',
        title: 'Figma',
        subtitle: 'Дизайн',
        target: 'C:\\Users\\user\\AppData\\Local\\Figma\\Figma.exe',
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
        title: 'Workspace',
        subtitle: 'Проект',
        target: 'C:\\Users\\User\\Documents\\Workspace',
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
        id: 'tool-7z',
        title: '7-Zip',
        subtitle: 'Архиватор',
        target: 'C:\\Program Files\\7-Zip\\7zFM.exe',
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
        id: 'combo-cs2',
        kind: 'combo',
        title: 'Counter-Strike 2 + Discord',
        subtitle: 'Связка',
        target: '',
        comboTargets: ['steam://rungameid/730', 'C:\\Tools\\Discord.exe'],
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
    id: 'app-vsc',
    name: 'Visual Studio Code',
    category: 'Разработка',
    target: 'C:\\Users\\user\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe',
    aumid: null,
    iconKey: null,
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
    appId: '730',
    name: 'Counter-Strike 2',
    launcher: 'Steam',
    target: 'steam://rungameid/730',
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

const sampleGroups: AppWindowGroup[] = [
  {
    key: 'figma.exe',
    name: 'Figma',
    exePath: 'C:\\Users\\user\\AppData\\Local\\Figma\\Figma.exe',
    isActive: false,
    iconKey: null,
    iconData: null,
    windows: [
      {
        hwnd: 9001,
        pid: 412,
        title: 'Design System v2 — Figma',
        exePath: 'C:\\Users\\user\\AppData\\Local\\Figma\\Figma.exe',
        exeName: 'Figma',
        isActive: false,
        isMinimized: false,
      },
    ],
  },
];

const sampleAudio: AudioState = {
  available: true,
  masterVolume: 72,
  masterMuted: false,
  micMuted: false,
  devices: [
    { id: 'dev-speakers', name: 'Realtek Digital Speakers', shortName: 'Колонки', isDefault: true },
    { id: 'dev-hp', name: 'Sony WH-1000XM5 Headphones', shortName: 'Sony XM5', isDefault: false },
  ],
  sessions: [],
};

const sampleClips: ClipboardEntry[] = [
  {
    id: 'c-1',
    tag: 'HEX',
    kind: 'hex',
    text: '#8B96F8',
    preview: '#8B96F8',
    color: '#8B96F8',
    pinned: true,
    lineCount: 1,
    charCount: 7,
  },
  {
    id: 'c-2',
    tag: 'CODE',
    kind: 'code',
    text: 'const secret = "obsidian";\nconsole.log(secret);',
    preview: 'const secret = "obsidian"; console.log(secret);',
    pinned: false,
    lineCount: 2,
    charCount: 47,
  },
];

describe('searchLogic corpus, Raycast ranking, open windows, commands, clipboard & bangs', () => {
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
    expect(names).toContain('Telegram');
    expect(names).toContain('Загрузки');
    expect(names).toContain('Workspace');
    expect(names).toContain('Cursor');
    expect(names).toContain('Photoshop 2025');
    expect(names).toContain('7-Zip');
    expect(names).toContain('Battle.net');
    expect(names).toContain('Counter-Strike 2 + Discord');
    expect(names).toContain('Портфель Акций');
    expect(names).toContain('Counter-Strike 2');
    expect(names).toContain('Notion');
    expect(names).toContain('Calculator');
    expect(names).toContain('Quarterly-Report-2026.pdf');
    expect(names).toContain('Дизайн-Макеты');

    const calc = corpus.find((c) => c.name === 'Calculator');
    expect(calc?.source).toBe('uwp');
    expect(calc?.iconData).toBe('data:image/png;base64,CALC');
  });

  it('prioritizes live open windows ahead of launching duplicate apps and supports focus + Alt+Enter close', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
      groups: sampleGroups,
    });

    const ranked = rankSearchResults(corpus, 'figma');
    expect(ranked.items.length).toBeGreaterThan(0);
    expect(ranked.items[0].source).toBe('window');
    expect(ranked.items[0].hwnd).toBe(9001);
    expect(ranked.items[0].badge).toEqual({ text: 'Открыто', variant: 'open' });
    expect(getPrimaryActionLabel(ranked.items[0])).toBe('Перейти');

    expect(resolveSearchLaunchPayload(ranked.items[0], false)).toEqual({
      command: 'focus_window',
      args: { hwnd: 9001, toggleMinimize: false },
    });
    expect(resolveSearchSecondaryAction(ranked.items[0])).toEqual({
      type: 'close_window',
      hwnd: 9001,
      label: 'Закрыть окно',
    });
  });

  it('supports subsequence fuzzy matching (vsc -> Visual Studio Code) with exact highlight indices', () => {
    const sub = matchSubsequence('Visual Studio Code', 'vsc');
    expect(sub).not.toBeNull();
    expect(sub?.indices).toEqual([0, 7, 14]);

    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
    });

    const ranked = rankSearchResults(corpus, 'vsc');
    expect(ranked.items[0]?.name).toBe('Visual Studio Code');
    expect(ranked.matchIndices[ranked.items[0].id]).toEqual([0, 7, 14]);
  });

  it('boosts frequently launched items via frecency scoring', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: [
        ...sampleApps,
        {
          id: 'app-alpha',
          name: 'Note Alpha',
          category: 'Программы',
          target: 'C:\\Tools\\NoteAlpha.exe',
          aumid: null,
          iconKey: null,
          iconData: null,
        },
        {
          id: 'app-beta',
          name: 'Note Beta',
          category: 'Программы',
          target: 'C:\\Tools\\NoteBeta.exe',
          aumid: null,
          iconKey: null,
          iconData: null,
        },
      ],
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
    });

    const now = 1_750_000_000_000;
    const frecency: Record<string, FrecencyEntry> = {
      'target:c:\\tools\\notebeta.exe': { count: 24, lastUsedMs: now - 60_000 },
    };

    const ranked = rankSearchResults(corpus, 'note', 10, frecency, now);
    expect(ranked.items[0]?.name).toBe('Note Beta');
  });

  it('matches RU<->EN wrong-keyboard-layout queries across apps, files, and system commands', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
      audio: sampleAudio,
    });

    // "ntkt" (EN layout for "теле") -> Telegram
    const tgHit = rankSearchResults(corpus, 'ntkt');
    expect(tgHit.items[0]?.name).toBe('Telegram');
    expect(tgHit.swappedHint).toBe('теле');

    // "rjhpb" (EN layout for "корзи") -> Очистить корзину command
    const binHit = rankSearchResults(corpus, 'rjhpb');
    expect(binHit.items[0]?.commandId).toBe('empty_recycle_bin');
    expect(binHit.swappedHint).toBe('корзи');
    expect(resolveSearchLaunchPayload(binHit.items[0], false)).toEqual({
      command: 'empty_recycle_bin',
      args: {},
    });

    // "йгфкеукдн" (RU layout for "quarterly") -> user file
    const fileHit = rankSearchResults(corpus, 'йгфкеукдн');
    expect(fileHit.items[0]?.name).toBe('Quarterly-Report-2026.pdf');
    expect(fileHit.swappedHint).toBe('quarterly');

    // "gjhn" (EN layout for "порт") -> customSpace URL
    const customHit = rankSearchResults(corpus, 'gjhn');
    expect(customHit.items.some((i) => i.name === 'Портфель Акций')).toBe(true);
  });

  it('exposes WASAPI audio output switching and bilingual system commands', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
      audio: sampleAudio,
    });

    const hpHit = rankSearchResults(corpus, 'sony');
    expect(hpHit.items[0]?.source).toBe('audio_device');
    expect(resolveSearchLaunchPayload(hpHit.items[0], false)).toEqual({
      command: 'set_audio_device',
      args: { deviceId: 'dev-hp' },
    });

    const micHit = rankSearchResults(corpus, 'микрофон');
    expect(micHit.items[0]?.commandId).toBe('mic_toggle');
    expect(resolveSearchLaunchPayload(micHit.items[0], false)).toEqual({
      command: 'set_mic_muted',
      args: { muted: true },
    });

    const snipHit = rankSearchResults(corpus, 'скриншот');
    expect(snipHit.items[0]?.commandId).toBe('screenshot');
    expect(resolveSearchLaunchPayload(snipHit.items[0], false)).toEqual({
      command: 'open_screen_snip',
      args: {},
    });

    const reindexHit = rankSearchResults(corpus, 'reindex');
    expect(reindexHit.items[0]?.commandId).toBe('refresh_indexes');
    expect(resolveSearchLaunchPayload(reindexHit.items[0], false)).toEqual({
      command: 'refresh_indexes',
      args: {},
    });
  });

  it('supports clipboard prefix search (clip / буфер) and dispatches instant paste', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
      clips: sampleClips,
    });

    const allClips = rankSearchResults(corpus, 'clip');
    expect(allClips.mode).toBe('clipboard');
    expect(allClips.items.length).toBe(2);

    const filtered = rankSearchResults(corpus, 'буфер obsidian');
    expect(filtered.mode).toBe('clipboard');
    expect(filtered.items.length).toBe(1);
    expect(filtered.items[0].source).toBe('clipboard');
    expect(resolveSearchLaunchPayload(filtered.items[0], false)).toEqual({
      command: 'paste_clipboard_entry',
      args: { text: 'const secret = "obsidian";\nconsole.log(secret);' },
    });
    expect(resolveSearchSecondaryAction(filtered.items[0])).toEqual({
      type: 'copy_clipboard',
      text: 'const secret = "obsidian";\nconsole.log(secret);',
      label: 'Копировать',
    });
  });

  it('supports explicit web bangs (g, yt, gh) and strictly excludes any math calculator', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
    });

    const google = rankSearchResults(corpus, 'g tauri v2');
    expect(google.mode).toBe('web');
    expect(google.items[0].target).toBe('https://www.google.com/search?q=tauri%20v2');

    const yt = rankSearchResults(corpus, 'yt synthwave');
    expect(yt.items[0].target).toBe('https://www.youtube.com/results?search_query=synthwave');

    const gh = rankSearchResults(corpus, 'gh windows-rs');
    expect(gh.items[0].target).toBe('https://github.com/search?q=windows-rs');

    // User exclusion guardrail: ZERO inline math calculator!
    expect(rankSearchResults(corpus, '2+2').items).toHaveLength(0);
    expect(rankSearchResults(corpus, '144 / 12').items).toHaveLength(0);
    expect(rankSearchResults(corpus, '(18 + 7) * 4').items).toHaveLength(0);
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
    expect(ranked.items[0].name).toBe('Cursor');
    expect(ranked.items[1].name).toBe('Cursorless Extension');
    expect(ranked.items[2].name).toBe('My Cursor Helper');
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

    const calc = corpus.find((c) => c.name === 'Calculator')!;
    expect(resolveSearchLaunchPayload(calc, true)).toEqual({
      command: 'launch_item',
      args: { target: calc.target, args: null, asAdmin: false, closeStart: true },
    });

    const combo = corpus.find((c) => c.name === 'Counter-Strike 2 + Discord')!;
    expect(resolveSearchLaunchPayload(combo, false)).toEqual({
      command: 'launch_combo',
      args: { targets: ['steam://rungameid/730', 'C:\\Tools\\Discord.exe'] },
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

  it('indexes Windows 11 Settings & Network items cleanly with RU/EN + wrong-layout queries and avoids UWP Settings duplication', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: [
        ...sampleApps,
        {
          id: 'uwp-settings-dup',
          name: 'Параметры',
          category: 'Система',
          target: 'shell:AppsFolder\\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
          aumid: 'windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
          iconKey: 'aumid:settings',
          iconData: null,
        },
      ],
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
      audio: sampleAudio,
    });

    // Only one Settings root entry (`Параметры Windows`, no duplicate UWP `Параметры`)
    const settingsRoots = corpus.filter(
      (c) =>
        c.target === 'ms-settings:' ||
        c.matchAumid ===
          'windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel',
    );
    expect(settingsRoots).toHaveLength(1);
    expect(settingsRoots[0].name).toBe('Параметры Windows');

    const netRu = rankSearchResults(corpus, 'сеть и интернет');
    expect(netRu.items[0]?.id).toBe('winset:network');
    expect(netRu.items[0]?.source).toBe('win_settings');
    expect(resolveSearchLaunchPayload(netRu.items[0], false)).toEqual({
      command: 'launch_item',
      args: { target: 'ms-settings:network', args: null, asAdmin: false, closeStart: true },
    });

    const wifiHit = rankSearchResults(corpus, 'вайфай');
    expect(wifiHit.items[0]?.id).toBe('winset:wifi');
    expect(wifiHit.items[0]?.target).toBe('ms-settings:network-wifi');

    const netEn = rankSearchResults(corpus, 'network');
    expect(netEn.items[0]?.id).toBe('winset:network');

    const btShort = rankSearchResults(corpus, 'bt');
    expect(btShort.items[0]?.id).toBe('winset:bluetooth');

    const btRu = rankSearchResults(corpus, 'блютуз');
    expect(btRu.items[0]?.id).toBe('winset:bluetooth');

    const settingsHit = rankSearchResults(corpus, 'параметры');
    expect(settingsHit.items[0]?.id).toBe('winset:settings');
    expect(settingsHit.items[0]?.target).toBe('ms-settings:');

    // Wrong layout "ctnm" ("сеть") hits Network & Internet cleanly
    const swappedNet = rankSearchResults(corpus, 'ctnm');
    expect(swappedNet.items.some((i) => i.id === 'winset:network')).toBe(true);
  });

  it('indexes config.spaces pins first and propagates matchAumid across sources', () => {
    const cfgV2: TestBarConfig = {
      ...makeConfig(),
      schemaVersion: 2,
      spaces: [
        {
          id: 'custom-dev',
          name: 'Разработка',
          accentHex: '#38BDF8',
          accentRgb: '56,189,248',
          layoutKind: 'grid',
          showHeroRow: true,
          widgets: ['media'],
          items: [
            makePin({
              id: 'sp-wt',
              title: 'My Terminal',
              subtitle: 'Терминал',
              target: 'shell:AppsFolder\\Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
              matchAumid: 'Microsoft.WindowsTerminal_8wekyb3d8bbwe!App',
            }),
            makePin({
              id: 'sp-dir',
              kind: 'folder',
              title: 'Исходники',
              subtitle: 'Папка',
              target: 'D:\\Code\\Workspace',
            }),
          ],
        },
      ],
    };

    const corpus = buildSearchCorpus({
      config: cfgV2,
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
      groups: [
        {
          ...sampleGroups[0],
          aumid: 'Figma.Desktop!App',
        },
      ],
    });

    // Spaces pins are indexed; legacy fields are NOT indexed when config.spaces is non-empty
    const myTerm = corpus.find((c) => c.name === 'My Terminal');
    expect(myTerm?.source).toBe('pinned');
    expect(myTerm?.matchAumid).toBe('Microsoft.WindowsTerminal_8wekyb3d8bbwe!App');
    expect(corpus.some((c) => c.name === 'Исходники' && c.kind === 'folder')).toBe(true);
    expect(corpus.some((c) => c.name === 'Портфель Акций')).toBe(false);

    // Propagates matchAumid on open windows and indexed UWP apps
    expect(corpus.find((c) => c.source === 'window')?.matchAumid).toBe('Figma.Desktop!App');
    expect(corpus.find((c) => c.name === 'Calculator')?.matchAumid).toBe(
      'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
    );
  });

  it('buildPinFromSearchItem builds valid pins for apps, UWP hosts, folders, win_settings and rejects transient items', () => {
    const corpus = buildSearchCorpus({
      config: makeConfig(),
      apps: sampleApps,
      games: sampleGames,
      files: sampleFiles,
      iconMap: {},
      audio: sampleAudio,
      clips: sampleClips,
      groups: [
        {
          key: 'aumid:calc',
          name: 'Калькулятор',
          exePath: 'C:\\Windows\\System32\\ApplicationFrameHost.exe',
          aumid: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
          iconKey: 'aumid:calc',
          iconData: null,
          isActive: true,
          windows: [
            {
              hwnd: 777,
              pid: 888,
              title: 'Калькулятор — Вычисления',
              exePath: 'C:\\Windows\\System32\\ApplicationFrameHost.exe',
              exeName: 'ApplicationFrameHost',
              aumid: 'Microsoft.WindowsCalculator_8wekyb3d8bbwe!App',
              isActive: true,
              isMinimized: false,
            },
          ],
        },
      ],
    });

    // UWP window hosted by ApplicationFrameHost.exe resolves cleanly to shell:AppsFolder\<aumid>
    const calcWin = corpus.find((c) => c.source === 'window' && c.hwnd === 777)!;
    const calcPin = buildPinFromSearchItem(calcWin, 'pin-calc-win');
    expect(calcPin).not.toBeNull();
    expect(calcPin?.title).toBe('Калькулятор');
    expect(calcPin?.target).toBe('shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App');
    expect(calcPin?.matchAumid).toBe('Microsoft.WindowsCalculator_8wekyb3d8bbwe!App');
    expect(calcPin?.matchExe).toBeNull();

    // Windows Settings ms-settings:* item builds valid pin
    const netSetting = corpus.find((c) => c.id === 'winset:network')!;
    const netPin = buildPinFromSearchItem(netSetting, 'pin-net');
    expect(netPin).not.toBeNull();
    expect(netPin?.kind).toBe('app');
    expect(netPin?.target).toBe('ms-settings:network');
    expect(netPin?.altAction).toBe('');

    // Folder item builds folder pin
    const folderItem = corpus.find((c) => c.name === 'Дизайн-Макеты')!;
    const folderPin = buildPinFromSearchItem(folderItem, 'pin-dir');
    expect(folderPin?.kind).toBe('folder');
    expect(folderPin?.target).toBe('C:\\Users\\user\\Desktop\\Дизайн-Макеты');

    // task_manager and settings commands are pinnable; lock / clipboard / audio_device / web are rejected
    const taskmgr = corpus.find((c) => c.commandId === 'task_manager')!;
    expect(buildPinFromSearchItem(taskmgr, 'pin-tm')?.target).toBe('C:\\Windows\\System32\\Taskmgr.exe');
    const tbSettings = corpus.find((c) => c.commandId === 'settings')!;
    expect(buildPinFromSearchItem(tbSettings, 'pin-tbs')?.target).toBe('cmd:settings');

    const lockCmd = corpus.find((c) => c.commandId === 'lock')!;
    const clipItem = corpus.find((c) => c.source === 'clipboard')!;
    const audioItem = corpus.find((c) => c.source === 'audio_device')!;
    const webItem = rankSearchResults(corpus, 'g tauri').items[0];
    expect(buildPinFromSearchItem(lockCmd)).toBeNull();
    expect(buildPinFromSearchItem(clipItem)).toBeNull();
    expect(buildPinFromSearchItem(audioItem)).toBeNull();
    expect(buildPinFromSearchItem(webItem)).toBeNull();
  });
});
