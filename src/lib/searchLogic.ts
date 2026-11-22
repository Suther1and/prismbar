import { canOpenFileLocation, canRunAsAdmin } from './flyoutLogic';
import {
  formatAppsFolderTarget,
  iconCache,
  isCacheIconKey,
  isNonLaunchableUwpHostExe,
  type AppWindowGroup,
  type AudioState,
  type ClipboardEntry,
  type FrecencyEntry,
  type IndexedApp,
  type IndexedFile,
  type PinItem,
  type SteamGame,
  type TestBarConfig,
} from './ipc';
import { collectPhoneticAliases, swapLayout } from './layoutMap';
import { detectPlatform, type AppPlatform } from './platform';

export type SearchSourceKind =
  | 'window'
  | 'command'
  | 'win_settings'
  | 'audio_device'
  | 'clipboard'
  | 'web'
  | 'pinned'
  | 'app'
  | 'uwp'
  | 'game'
  | 'folder'
  | 'file'
  | 'url'
  | 'combo';

export type SystemCommandId =
  | 'audio_device'
  | 'mic_toggle'
  | 'empty_recycle_bin'
  | 'screenshot'
  | 'refresh_indexes'
  | 'task_manager'
  | 'lock'
  | 'sleep'
  | 'reboot'
  | 'shutdown'
  | 'settings';

export interface SearchBadge {
  text: string;
  variant: 'open' | 'cmd' | 'clip';
}

export interface SearchCorpusItem {
  id: string;
  name: string;
  subtitle: string;
  category: string;
  target: string;
  args?: string | null;
  kind: 'app' | 'folder' | 'file' | 'url' | 'combo';
  source: SearchSourceKind;
  iconData?: string | null;
  iconKey?: string | null;
  cacheIconKey?: string | null;
  matchAumid?: string | null;
  comboTargets?: string[] | null;
  pinned: boolean;
  badge?: SearchBadge | null;
  hwnd?: number | null;
  commandId?: SystemCommandId | null;
  deviceId?: string | null;
  micMuted?: boolean;
  clipText?: string | null;
  clipColor?: string | null;
  keywords?: string[];
  frecencyKey?: string;
}

export interface SearchCorpusInput {
  config: TestBarConfig | null;
  apps: IndexedApp[];
  games: SteamGame[];
  files: IndexedFile[];
  iconMap: Record<string, string>;
  groups?: AppWindowGroup[];
  audio?: AudioState | null;
  clips?: ClipboardEntry[];
  platform?: AppPlatform;
}

export interface RankedSearchResult {
  items: SearchCorpusItem[];
  swappedHint: string | null;
  totalMatches: number;
  matchIndices: Record<string, number[]>;
  mode?: 'default' | 'clipboard' | 'web';
}

function normKey(v: string | null | undefined): string {
  return (v ?? '').trim().replace(/\//g, '\\').toLowerCase();
}

function resolveCachedIconData(
  iconMap: Record<string, string>,
  cacheIconKey: string | null | undefined,
  fallbackTarget?: string,
  inlineData?: string | null,
): string | null {
  if (cacheIconKey) {
    const mapped = iconMap[cacheIconKey] || iconCache.get(cacheIconKey);
    if (mapped) return mapped;
  }
  if (inlineData) return inlineData;
  if (fallbackTarget && iconMap[fallbackTarget]) return iconMap[fallbackTarget];
  return null;
}

function resolvePinIcon(pin: PinItem, iconMap: Record<string, string>): {
  iconData: string | null;
  cacheIconKey: string | null;
} {
  const cacheIconKey = isCacheIconKey(pin.iconKey) ? pin.iconKey : null;
  const iconData = resolveCachedIconData(iconMap, cacheIconKey, pin.target, pin.iconData);
  return { iconData, cacheIconKey };
}

function buildSystemCommandItems(
  audio?: AudioState | null,
  platform: AppPlatform = 'windows',
): SearchCorpusItem[] {
  const isMac = platform === 'macos';
  const commands: SearchCorpusItem[] = [];

  if (!isMac) {
    for (const dev of audio?.devices ?? []) {
      commands.push({
        id: `cmd:audio:${dev.id}`,
        name: `Звук: ${dev.shortName}`,
        subtitle: dev.isDefault ? `Активный вывод · ${dev.name}` : `Переключить вывод · ${dev.name}`,
        category: 'Аудиовывод',
        target: dev.id,
        args: null,
        kind: 'app',
        source: 'audio_device',
        iconData: null,
        iconKey: 'cmd:audio_device',
        cacheIconKey: null,
        pinned: false,
        badge: { text: 'Команда', variant: 'cmd' },
        commandId: 'audio_device',
        deviceId: dev.id,
        keywords: [
          'звук',
          'аудио',
          'вывод',
          'наушники',
          'колонки',
          'динамики',
          'audio',
          'sound',
          'output',
          'headphones',
          'speakers',
          dev.shortName,
          dev.name,
        ],
        frecencyKey: `cmd:audio:${dev.id}`,
      });
    }

    const micMuted = Boolean(audio?.micMuted);
    commands.push({
      id: 'cmd:mic_toggle',
      name: micMuted ? 'Включить микрофон' : 'Выключить микрофон',
      subtitle: micMuted ? 'Микрофон сейчас выключен' : 'Микрофон сейчас активен',
      category: 'Аудио',
      target: 'cmd:mic_toggle',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:mic_toggle',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'mic_toggle',
      micMuted,
      keywords: ['микрофон', 'mic', 'mute', 'выключить микрофон', 'включить микрофон', 'голос'],
      frecencyKey: 'cmd:mic_toggle',
    });
  }

  commands.push(
    {
      id: 'cmd:empty_recycle_bin',
      name: 'Очистить корзину',
      subtitle: isMac ? 'Удалить файлы из Корзины' : 'Удалить файлы из корзины Windows',
      category: 'Система',
      target: 'cmd:empty_recycle_bin',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:empty_recycle_bin',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'empty_recycle_bin',
      keywords: ['корзина', 'очистить корзину', 'recycle', 'recycle bin', 'trash', 'empty bin'],
      frecencyKey: 'cmd:empty_recycle_bin',
    },
    {
      id: 'cmd:screenshot',
      name: 'Сделать скриншот',
      subtitle: isMac ? 'Выбрать область экрана и скопировать' : 'Открыть инструмент «Ножницы»',
      category: 'Система',
      target: isMac ? 'cmd:screenshot' : 'ms-screenclip:',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:screenshot',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'screenshot',
      keywords: ['скриншот', 'снимок', 'экран', 'ножницы', 'screenshot', 'snip', 'capture'],
      frecencyKey: 'cmd:screenshot',
    },
    {
      id: 'cmd:refresh_indexes',
      name: 'Обновить индекс приложений',
      subtitle: isMac ? 'Пересканировать программы и файлы' : 'Пересканировать программы, игры и файлы',
      category: 'PrismBar',
      target: 'cmd:refresh_indexes',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:refresh_indexes',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'refresh_indexes',
      keywords: ['обновить индекс', 'пересканировать', 'индекс', 'reindex', 'refresh', 'rescan'],
      frecencyKey: 'cmd:refresh_indexes',
    },
  );

  if (!isMac) {
    commands.push({
      id: 'cmd:task_manager',
      name: 'Диспетчер задач',
      subtitle: 'Процессы и производительность Windows',
      category: 'Система',
      target: 'C:\\Windows\\System32\\Taskmgr.exe',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:task_manager',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'task_manager',
      keywords: ['диспетчер задач', 'диспетчер', 'процессы', 'taskmgr', 'task manager'],
      frecencyKey: 'cmd:task_manager',
    });
  }

  commands.push(
    {
      id: 'cmd:settings',
      name: 'Настройки PrismBar',
      subtitle: isMac
        ? 'Оформление, клавиша Command, позиция и профиль'
        : 'Панель, стекло, мониторы и резервная копия',
      category: 'PrismBar',
      target: 'cmd:settings',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:settings',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'settings',
      keywords: [
        'настройки prismbar',
        'настройки testbar',
        'настройки',
        'параметры',
        'settings',
        'preferences',
      ],
      frecencyKey: 'cmd:settings',
    },
    {
      id: 'cmd:lock',
      name: 'Блокировка экрана',
      subtitle: isMac ? 'Заблокировать экран Mac' : 'Заблокировать сеанс Windows',
      category: 'Питание',
      target: 'cmd:lock',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:lock',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'lock',
      keywords: ['блокировка', 'заблокировать', 'lock', 'экран'],
      frecencyKey: 'cmd:lock',
    },
    {
      id: 'cmd:sleep',
      name: 'Режим сна',
      subtitle: isMac ? 'Перевести Mac в режим сна' : 'Перевести компьютер в сон',
      category: 'Питание',
      target: 'cmd:sleep',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:sleep',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'sleep',
      keywords: ['сон', 'спящий режим', 'усыпить', 'sleep', 'suspend'],
      frecencyKey: 'cmd:sleep',
    },
    {
      id: 'cmd:reboot',
      name: 'Перезагрузка компьютера',
      subtitle: isMac ? 'Перезагрузить Mac' : 'Перезагрузить Windows',
      category: 'Питание',
      target: 'cmd:reboot',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:reboot',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'reboot',
      keywords: ['перезагрузка', 'перезагрузить', 'reboot', 'restart'],
      frecencyKey: 'cmd:reboot',
    },
    {
      id: 'cmd:shutdown',
      name: 'Выключение компьютера',
      subtitle: isMac ? 'Завершить работу Mac' : 'Завершить работу Windows',
      category: 'Питание',
      target: 'cmd:shutdown',
      args: null,
      kind: 'app',
      source: 'command',
      iconData: null,
      iconKey: 'cmd:shutdown',
      cacheIconKey: null,
      pinned: false,
      badge: { text: 'Команда', variant: 'cmd' },
      commandId: 'shutdown',
      keywords: ['выключение', 'выключить', 'завершение работы', 'shutdown', 'power off'],
      frecencyKey: 'cmd:shutdown',
    },
  );

  return commands;
}

const WIN_SETTINGS_APP_AUMID =
  'windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel';

export function buildWindowsSettingsItems(): SearchCorpusItem[] {
  const specs: Array<{
    id: string;
    name: string;
    subtitle: string;
    target: string;
    keywords: string[];
  }> = [
    {
      id: 'winset:settings',
      name: 'Параметры Windows',
      subtitle: 'Системные параметры Windows 11',
      target: 'ms-settings:',
      keywords: [
        'параметры windows',
        'параметры',
        'настройки windows',
        'windows settings',
        'settings',
        'systemsettings',
      ],
    },
    {
      id: 'winset:network',
      name: 'Сеть и интернет',
      subtitle: 'Подключения, состояние сети и параметры',
      target: 'ms-settings:network',
      keywords: [
        'сеть',
        'интернет',
        'настройка сети',
        'сеть и интернет',
        'подключение',
        'network',
        'internet',
        'lan',
      ],
    },
    {
      id: 'winset:wifi',
      name: 'Wi-Fi',
      subtitle: 'Беспроводные сети и адаптер Wi-Fi',
      target: 'ms-settings:network-wifi',
      keywords: ['wifi', 'wi-fi', 'вайфай', 'вай-фай', 'беспроводная сеть', 'сеть'],
    },
    {
      id: 'winset:ethernet',
      name: 'Ethernet и проводная сеть',
      subtitle: 'Проводное подключение и параметры адаптера',
      target: 'ms-settings:network-ethernet',
      keywords: ['ethernet', 'проводная сеть', 'кабель', 'lan', 'сеть'],
    },
    {
      id: 'winset:vpn',
      name: 'VPN',
      subtitle: 'Профили и подключения VPN',
      target: 'ms-settings:network-vpn',
      keywords: ['vpn', 'впн', 'сеть'],
    },
    {
      id: 'winset:proxy',
      name: 'Прокси-сервер',
      subtitle: 'Параметры прокси-сервера Windows',
      target: 'ms-settings:network-proxy',
      keywords: ['прокси', 'proxy', 'сеть'],
    },
    {
      id: 'winset:ncpa',
      name: 'Сетевые подключения (адаптеры)',
      subtitle: 'Панель сетевых адаптеров, DNS и IP',
      target: 'ncpa.cpl',
      keywords: [
        'сетевые подключения',
        'адаптеры',
        'сетевая карта',
        'dns',
        'ip',
        'ncpa',
        'сеть и интернет',
        'настройка сети',
      ],
    },
    {
      id: 'winset:bluetooth',
      name: 'Bluetooth и устройства',
      subtitle: 'Беспроводные устройства, мыши, наушники',
      target: 'ms-settings:bluetooth',
      keywords: ['bluetooth', 'bt', 'блютуз', 'блютус', 'устройства', 'наушники', 'мышь'],
    },
    {
      id: 'winset:sound',
      name: 'Звук (Параметры Windows)',
      subtitle: 'Устройства вывода, микрофон и громкость',
      target: 'ms-settings:sound',
      keywords: ['звук windows', 'звук', 'аудио', 'микрофон', 'устройства воспроизведения', 'sound'],
    },
    {
      id: 'winset:display',
      name: 'Дисплей и мониторы',
      subtitle: 'Разрешение, масштаб, частота и HDR',
      target: 'ms-settings:display',
      keywords: ['дисплей', 'экран', 'монитор', 'разрешение', 'герцовка', 'hdr', 'масштаб', 'display'],
    },
    {
      id: 'winset:personalization',
      name: 'Персонализация и обои',
      subtitle: 'Фон рабочего стола, цвета и темы',
      target: 'ms-settings:personalization',
      keywords: ['персонализация', 'обои', 'тема', 'фон', 'wallpaper', 'personalization'],
    },
    {
      id: 'winset:appsfeatures',
      name: 'Приложения и удаление программ',
      subtitle: 'Установленные приложения и компоненты',
      target: 'ms-settings:appsfeatures',
      keywords: ['приложения', 'программы', 'удалить программу', 'apps', 'uninstall'],
    },
    {
      id: 'winset:startupapps',
      name: 'Автозагрузка Windows',
      subtitle: 'Программы при входе в систему',
      target: 'ms-settings:startupapps',
      keywords: ['автозагрузка', 'автозапуск', 'startup'],
    },
    {
      id: 'winset:defaultapps',
      name: 'Приложения по умолчанию',
      subtitle: 'Браузер и программы по умолчанию',
      target: 'ms-settings:defaultapps',
      keywords: ['приложения по умолчанию', 'браузер по умолчанию', 'default apps'],
    },
    {
      id: 'winset:storagesense',
      name: 'Хранилище и диски',
      subtitle: 'Свободное место и очистка дисков',
      target: 'ms-settings:storagesense',
      keywords: ['хранилище', 'диски', 'место на диске', 'storage'],
    },
    {
      id: 'winset:powersleep',
      name: 'Питание и батарея',
      subtitle: 'Сон, экран и схема питания',
      target: 'ms-settings:powersleep',
      keywords: ['питание', 'батарея', 'энергосбережение', 'power', 'battery'],
    },
    {
      id: 'winset:windowsupdate',
      name: 'Обновление Windows',
      subtitle: 'Центр обновления Windows 11',
      target: 'ms-settings:windowsupdate',
      keywords: ['обновления', 'центр обновления', 'windows update', 'update'],
    },
    {
      id: 'winset:regionlanguage',
      name: 'Время и язык',
      subtitle: 'Языки, ввод и раскладка клавиатуры',
      target: 'ms-settings:regionlanguage',
      keywords: ['язык', 'раскладка', 'время и язык', 'регион', 'language', 'keyboard'],
    },
    {
      id: 'winset:dateandtime',
      name: 'Дата и время',
      subtitle: 'Часовой пояс и синхронизация времени',
      target: 'ms-settings:dateandtime',
      keywords: ['время', 'дата', 'часовой пояс', 'date', 'time'],
    },
    {
      id: 'winset:windowsdefender',
      name: 'Безопасность и Защитник Windows',
      subtitle: 'Защита от вирусов и брандмауэр',
      target: 'ms-settings:windowsdefender',
      keywords: ['безопасность', 'защитник', 'антивирус', 'брандмауэр', 'defender', 'security', 'firewall'],
    },
    {
      id: 'winset:gaming',
      name: 'Игровой режим',
      subtitle: 'Оптимизация Windows для игр',
      target: 'ms-settings:gaming-gamemode',
      keywords: ['игровой режим', 'игры', 'game mode', 'gaming'],
    },
    {
      id: 'winset:about',
      name: 'О системе',
      subtitle: 'Характеристики ПК и версия Windows',
      target: 'ms-settings:about',
      keywords: ['о системе', 'характеристики', 'версия windows', 'about', 'system'],
    },
    {
      id: 'winset:control',
      name: 'Панель управления',
      subtitle: 'Классическая панель управления Windows',
      target: 'control.exe',
      keywords: ['панель управления', 'control panel', 'control'],
    },
    {
      id: 'winset:devmgmt',
      name: 'Диспетчер устройств',
      subtitle: 'Оборудование и драйверы устройств',
      target: 'devmgmt.msc',
      keywords: ['диспетчер устройств', 'драйверы', 'устройства', 'device manager', 'devmgmt'],
    },
    {
      id: 'winset:diskmgmt',
      name: 'Управление дисками',
      subtitle: 'Разделы, тома и буквы дисков',
      target: 'diskmgmt.msc',
      keywords: ['управление дисками', 'разделы', 'диски', 'disk management', 'diskmgmt'],
    },
    {
      id: 'winset:services',
      name: 'Службы Windows',
      subtitle: 'Системные службы и их запуск',
      target: 'services.msc',
      keywords: ['службы', 'сервисы', 'services', 'services.msc'],
    },
    {
      id: 'winset:mmsys',
      name: 'Классические параметры звука',
      subtitle: 'Устройства воспроизведения и записи (mmsys)',
      target: 'mmsys.cpl',
      keywords: ['классические параметры звука', 'звук', 'динамики', 'микрофон', 'mmsys'],
    },
  ];

  return specs.map((s) => ({
    id: s.id,
    name: s.name,
    subtitle: s.subtitle,
    category: 'Параметры Windows',
    target: s.target,
    args: null,
    kind: 'app',
    source: 'win_settings',
    iconData: null,
    iconKey: s.id,
    cacheIconKey: null,
    matchAumid: null,
    pinned: false,
    badge: { text: 'Параметры', variant: 'cmd' },
    keywords: s.keywords,
    frecencyKey: s.id,
  }));
}

/**
 * Builds the complete Start Menu Command Palette corpus spanning live open windows,
 * bilingual system & WASAPI audio commands, Windows 11 Settings, lossless clipboard vault entries,
 * ALL Spaces, Steam games, installed `.lnk` + UWP apps, and indexed user files.
 */
export function buildSearchCorpus({
  config,
  apps,
  games,
  files,
  iconMap,
  groups = [],
  audio = null,
  clips = [],
  platform = detectPlatform(),
}: SearchCorpusInput): SearchCorpusItem[] {
  const isMac = platform === 'macos';
  const pool: SearchCorpusItem[] = [];
  const seenWinHwnds = new Set<number>();
  const seenTargets = new Set<string>();
  const seenNameKind = new Set<string>();

  // 1. Live open windows (kept separate from pin/app dedupe so active windows always jump cleanly)
  for (const g of groups) {
    const cacheIconKey = isCacheIconKey(g.iconKey) ? g.iconKey : null;
    const groupIcon = resolveCachedIconData(iconMap, cacheIconKey, g.exePath, g.iconData);
    for (const w of g.windows) {
      if (!w.hwnd || seenWinHwnds.has(w.hwnd)) continue;
      seenWinHwnds.add(w.hwnd);
      const cleanTitle = w.title.trim() || g.name.trim();
      if (!cleanTitle) continue;
      pool.push({
        id: `win:${w.hwnd}`,
        name: cleanTitle,
        subtitle: g.name,
        category: g.name,
        target: w.exePath || g.exePath || '',
        args: null,
        kind: 'app',
        source: 'window',
        iconData: groupIcon,
        iconKey: g.iconKey || g.name.toLowerCase(),
        cacheIconKey,
        matchAumid: g.aumid ?? null,
        pinned: false,
        badge: { text: 'Открыто', variant: 'open' },
        hwnd: w.hwnd,
        keywords: [g.name, w.exeName, w.isMinimized ? 'свернуто' : 'активно'],
        frecencyKey: `app:${normKey(w.exePath || g.exePath || g.name)}`,
      });
    }
  }

  const pushUnique = (item: SearchCorpusItem) => {
    const cleanName = item.name.trim();
    const cleanTarget = item.target.trim();
    if (!cleanName) return;
    if (item.kind !== 'combo' && !cleanTarget) return;
    if (item.kind === 'combo' && (!item.comboTargets || item.comboTargets.length === 0)) return;

    const tKey = item.kind === 'combo'
      ? `combo:${(item.comboTargets ?? []).map(normKey).join('|')}`
      : `${item.kind}:${normKey(cleanTarget)}`;
    const nkKey = `${item.kind}:${cleanName.toLowerCase()}`;

    if (seenTargets.has(tKey) || seenNameKind.has(nkKey)) {
      return;
    }
    seenTargets.add(tKey);
    seenNameKind.add(nkKey);
    pool.push(item);
  };

  const addPinList = (pins: PinItem[] | undefined, category: string) => {
    for (const p of pins ?? []) {
      const { iconData, cacheIconKey } = resolvePinIcon(p, iconMap);
      pushUnique({
        id: `pin:${category}:${p.id}`,
        name: p.title,
        subtitle: p.subtitle || category,
        category,
        target: p.target,
        args: p.args ?? null,
        kind: p.kind,
        source: p.kind === 'combo' ? 'combo' : 'pinned',
        iconData,
        iconKey: p.iconKey,
        cacheIconKey,
        matchAumid: p.matchAumid ?? null,
        comboTargets: p.comboTargets ?? null,
        pinned: true,
        frecencyKey: p.kind === 'combo' ? `combo:${p.id}` : `target:${normKey(p.target) || p.id}`,
      });
    }
  };

  if (config) {
    if (config.spaces && config.spaces.length > 0) {
      for (const sp of config.spaces) {
        addPinList(sp.items, sp.name);
      }
      if (!isMac) {
        addPinList(config.gameCombos, 'Игры · Связка');
      }
    } else {
      addPinList(config.personalPins, 'Личное');
      addPinList(config.personalLinks, 'Быстрый доступ');
      addPinList(config.workFolders, 'Работа · Папка');
      addPinList(config.workApps, 'Работа');
      addPinList(config.studioPins, 'Студия');
      addPinList(config.studioTools, 'Студия · Инструмент');
      if (!isMac) {
        addPinList(config.gameLaunchers, 'Игры · Лаунчер');
        addPinList(config.gameCombos, 'Игры · Связка');
      }
      for (const cs of config.customSpaces) {
        addPinList(cs.items, cs.name);
      }
    }
  }

  if (!isMac) {
    for (const g of games) {
      pushUnique({
        id: `game:${g.appId}`,
        name: g.name,
        subtitle: g.launcher || 'Steam',
        category: 'Игра · Steam',
        target: g.target,
        args: null,
        kind: 'app',
        source: 'game',
        iconData: g.coverData ?? null,
        iconKey: 'steam',
        cacheIconKey: null,
        matchAumid: null,
        pinned: false,
        frecencyKey: `game:${g.appId}`,
      });
    }
  }

  for (const a of apps) {
    const aumidLow = (a.aumid ?? '').trim().toLowerCase();
    const targetLow = a.target.trim().toLowerCase();
    // Skip raw unpinned UWP Settings entry so curated `Параметры Windows` (ms-settings:) doesn't duplicate
    if (
      !isMac &&
      (aumidLow === WIN_SETTINGS_APP_AUMID ||
        targetLow === `shell:appsfolder\\${WIN_SETTINGS_APP_AUMID}`)
    ) {
      continue;
    }
    const isMacPrefs = isMac && targetLow.startsWith('x-apple.systempreferences:');
    const isUwp = !isMac && (Boolean(a.aumid) || targetLow.startsWith('shell:appsfolder\\'));
    const cacheIconKey = isCacheIconKey(a.iconKey) ? a.iconKey : null;
    const iconData = resolveCachedIconData(iconMap, cacheIconKey, a.target, a.iconData);
    pushUnique({
      id: `app:${a.id}`,
      name: a.name,
      subtitle: isMacPrefs ? 'Системные настройки macOS' : isUwp ? `${a.category} · UWP` : a.category,
      category: isMacPrefs ? 'Настройки macOS' : isUwp ? `${a.category} · UWP` : a.category,
      target: a.target,
      args: null,
      kind: 'app',
      source: isMacPrefs ? 'win_settings' : isUwp ? 'uwp' : 'app',
      iconData,
      iconKey: a.iconKey || a.name,
      cacheIconKey,
      matchAumid: a.aumid ?? null,
      pinned: false,
      badge: isMacPrefs ? { text: 'Параметры', variant: 'cmd' } : null,
      keywords: isMacPrefs
        ? ['настройки', 'системные настройки', 'system settings', 'preferences', a.name, a.category]
        : undefined,
      frecencyKey: `target:${normKey(a.target)}`,
    });
  }

  if (isMac) {
    pushUnique({
      id: 'cmd:activity_monitor',
      name: 'Мониторинг системы',
      subtitle: 'Процессы, память и нагрузка Mac',
      category: 'Утилиты',
      target: '/System/Applications/Utilities/Activity Monitor.app',
      args: null,
      kind: 'app',
      source: 'app',
      iconData: null,
      iconKey: 'cmd:task_manager',
      cacheIconKey: null,
      pinned: false,
      keywords: [
        'мониторинг системы',
        'мониторинг',
        'activity monitor',
        'диспетчер задач',
        'процессы',
        'память',
        'task manager',
      ],
      frecencyKey: 'target:/system/applications/utilities/activity monitor.app',
    });
  }

  for (const f of files) {
    const isFolder = f.kind === 'folder';
    pushUnique({
      id: `file:${f.id}`,
      name: f.name,
      subtitle: f.location,
      category: `${isFolder ? 'Папка' : 'Файл'} · ${f.location}`,
      target: f.target,
      args: null,
      kind: isFolder ? 'folder' : 'file',
      source: isFolder ? 'folder' : 'file',
      iconData: null,
      iconKey: isFolder ? 'folder' : 'file',
      cacheIconKey: null,
      pinned: false,
      frecencyKey: `target:${normKey(f.target)}`,
    });
  }

  // System & Audio commands
  for (const cmd of buildSystemCommandItems(audio, platform)) {
    pool.push(cmd);
  }

  // Windows 11 Settings, Network & System management tools
  if (!isMac) {
    const hasPinnedWinSettings =
      seenTargets.has(`app:shell:appsfolder\\${WIN_SETTINGS_APP_AUMID}`) ||
      seenTargets.has('app:ms-settings:');
    for (const ws of buildWindowsSettingsItems()) {
      if (ws.id === 'winset:settings' && hasPinnedWinSettings) {
        continue;
      }
      pushUnique(ws);
    }
  }

  // Clipboard Vault entries (up to 40)
  for (const c of clips.slice(0, 40)) {
    const preview = (c.preview ?? c.text).replace(/\s+/g, ' ').trim();
    if (!preview) continue;
    const lineMeta = (c.lineCount ?? 1) > 1 ? ` · ${c.lineCount} стр.` : '';
    pool.push({
      id: `clip:${c.id}`,
      name: preview,
      subtitle: `${c.pinned ? 'Закреплено' : 'Буфер обмена'}${lineMeta}`,
      category: c.tag || 'Буфер',
      target: c.text,
      args: null,
      kind: 'file',
      source: 'clipboard',
      iconData: null,
      iconKey: 'cmd:clipboard',
      cacheIconKey: null,
      pinned: c.pinned,
      badge: { text: 'Буфер', variant: 'clip' },
      clipText: c.text,
      clipColor: c.color ?? null,
      keywords: [c.tag, String(c.kind ?? ''), 'буфер', 'clip', 'clipboard'],
      frecencyKey: `clip:${c.id}`,
    });
  }

  return pool;
}

const WORD_BOUNDARY_RE = /[\s\-_./\\·:()[\]]/;

function isWordBoundaryAt(text: string, idx: number): boolean {
  if (idx <= 0) return true;
  const prev = text[idx - 1];
  const cur = text[idx];
  if (WORD_BOUNDARY_RE.test(prev)) return true;
  // CamelCase boundary (e.g. "VSCode", "TestBar")
  return prev === prev.toLowerCase() && prev !== prev.toUpperCase() && cur === cur.toUpperCase() && cur !== cur.toLowerCase();
}

function hasWordStartMatch(text: string, term: string): boolean {
  const words = text.split(/[\s\-_./\\·:()[\]]+/).filter(Boolean);
  return words.some((w) => w.startsWith(term));
}

export interface SubsequenceMatch {
  score: number;
  indices: number[];
}

/**
 * Character-subsequence fuzzy matcher prioritizing word-boundary acronyms (`vsc` -> VS Code,
 * `tg` -> Telegram) and tight consecutive spans, returning exact matched character indices.
 */
export function matchSubsequence(originalName: string, termLower: string): SubsequenceMatch | null {
  if (termLower.length < 2) return null;
  const nameLower = originalName.toLowerCase();
  if (termLower.length > nameLower.length) return null;

  // Prefer greedy word-boundary alignment first so "vsc" hits V-S-C initials instead of random middle letters.
  const indices: number[] = [];
  let cursor = 0;

  for (let ti = 0; ti < termLower.length; ti++) {
    const ch = termLower[ti];
    let boundaryPick = -1;
    let firstPick = -1;

    for (let ni = cursor; ni < nameLower.length; ni++) {
      if (nameLower[ni] === ch) {
        if (firstPick === -1) firstPick = ni;
        if (isWordBoundaryAt(originalName, ni)) {
          boundaryPick = ni;
          break;
        }
      }
    }

    // Also allow immediate continuation of previous matched char
    const prevIdx = indices.length > 0 ? indices[indices.length - 1] : -1;
    let chosen = firstPick;
    if (prevIdx >= 0 && prevIdx + 1 < nameLower.length && nameLower[prevIdx + 1] === ch) {
      chosen = prevIdx + 1;
    } else if (boundaryPick !== -1) {
      chosen = boundaryPick;
    }

    if (chosen === -1) return null;
    indices.push(chosen);
    cursor = chosen + 1;
  }

  let boundaryHits = 0;
  let consecutiveRuns = 0;
  let totalGap = 0;

  for (let i = 0; i < indices.length; i++) {
    const idx = indices[i];
    if (isWordBoundaryAt(originalName, idx)) boundaryHits += 1;
    if (i > 0) {
      const gap = idx - indices[i - 1] - 1;
      if (gap === 0) {
        consecutiveRuns += 1;
      } else {
        totalGap += gap;
      }
    }
  }

  const allBoundaryOrConsecutive = boundaryHits + consecutiveRuns >= indices.length;
  // Reject wildly scattered false positives on long titles/paths
  if (!allBoundaryOrConsecutive && totalGap > Math.max(8, termLower.length * 3)) {
    return null;
  }

  let score = 210 + boundaryHits * 46 + consecutiveRuns * 28 - totalGap * 5;
  if (indices[0] === 0) score += 35;
  if (boundaryHits === indices.length) score += 65; // Acronym match (e.g., "vsc" -> Visual Studio Code)
  return {
    score: Math.max(150, Math.min(440, score)),
    indices,
  };
}

function findSubstringIndices(textLower: string, termLower: string): number[] | null {
  if (!termLower) return null;
  const idx = textLower.indexOf(termLower);
  if (idx < 0) return null;
  const out: number[] = [];
  for (let i = 0; i < termLower.length; i++) {
    out.push(idx + i);
  }
  return out;
}

export function computeFrecencyBonus(
  entry: FrecencyEntry | undefined,
  nowMs: number = Date.now(),
): number {
  if (!entry || entry.count <= 0) return 0;
  const ageMs = Math.max(0, nowMs - (entry.lastUsedMs || 0));
  const ageDays = ageMs / 86_400_000;
  const decay = ageDays <= 1 ? 1.0 : ageDays <= 7 ? 0.78 : ageDays <= 30 ? 0.55 : 0.35;
  return Math.min(200, Math.round(Math.log2(entry.count + 1) * 48 * decay));
}

interface WebBangSpec {
  prefix: string;
  label: string;
  buildUrl: (query: string) => string;
}

const WEB_BANGS: readonly WebBangSpec[] = [
  {
    prefix: 'g ',
    label: 'Google',
    buildUrl: (q) => `https://www.google.com/search?q=${encodeURIComponent(q)}`,
  },
  {
    prefix: 'yt ',
    label: 'YouTube',
    buildUrl: (q) => `https://www.youtube.com/results?search_query=${encodeURIComponent(q)}`,
  },
  {
    prefix: 'gh ',
    label: 'GitHub',
    buildUrl: (q) => `https://github.com/search?q=${encodeURIComponent(q)}`,
  },
];

export function parseWebBangQuery(rawQuery: string): SearchCorpusItem | null {
  const trimmedStart = rawQuery.trimStart();
  const low = trimmedStart.toLowerCase();
  for (const bang of WEB_BANGS) {
    if (low.startsWith(bang.prefix)) {
      const rest = trimmedStart.slice(bang.prefix.length).trim();
      if (!rest) return null;
      return {
        id: `web:${bang.prefix.trim()}:${rest}`,
        name: `Найти в ${bang.label}: «${rest}»`,
        subtitle: bang.buildUrl(rest),
        category: bang.label,
        target: bang.buildUrl(rest),
        args: null,
        kind: 'url',
        source: 'web',
        iconData: null,
        iconKey: 'url',
        cacheIconKey: null,
        pinned: false,
        badge: { text: bang.label, variant: 'cmd' },
      };
    }
  }
  return null;
}

const CLIPBOARD_PREFIXES = ['clip', 'clipboard', 'буфер'];

function parseClipboardPrefixQuery(rawQuery: string): { active: boolean; subQuery: string; swappedPrefix: string | null } {
  const q = rawQuery.trim().toLowerCase();
  if (!q) return { active: false, subQuery: '', swappedPrefix: null };

  const matchPrefix = (candidate: string): { matched: boolean; rest: string } => {
    for (const p of CLIPBOARD_PREFIXES) {
      if (candidate === p) return { matched: true, rest: '' };
      if (candidate.startsWith(`${p} `)) return { matched: true, rest: candidate.slice(p.length).trim() };
    }
    return { matched: false, rest: '' };
  };

  const direct = matchPrefix(q);
  if (direct.matched) {
    return { active: true, subQuery: direct.rest, swappedPrefix: null };
  }

  const firstWord = q.split(/\s+/)[0] ?? '';
  const swappedFirst = swapLayout(firstWord);
  if (swappedFirst !== firstWord) {
    const rest = q.slice(firstWord.length).trim();
    const swappedCandidate = rest ? `${swappedFirst} ${rest}` : swappedFirst;
    const sw = matchPrefix(swappedCandidate);
    if (sw.matched) {
      return { active: true, subQuery: rest, swappedPrefix: swappedFirst };
    }
  }

  return { active: false, subQuery: '', swappedPrefix: null };
}

interface ScoreTermOutcome {
  score: number;
  indices: number[];
}

function scoreTerm(
  item: SearchCorpusItem,
  nameLow: string,
  aliases: string[],
  metaLow: string,
  targetLow: string,
  term: string,
): ScoreTermOutcome {
  let score = 0;
  let indices: number[] = [];

  if (nameLow === term) {
    score = 1000;
    indices = findSubstringIndices(nameLow, term) ?? [];
  } else if (item.source === 'window' && aliases.some((a) => a === term)) {
    score = 1040;
    indices = findSubstringIndices(nameLow, term) ?? matchSubsequence(item.name, term)?.indices ?? [];
  } else if (nameLow.startsWith(term)) {
    score = 660;
    indices = findSubstringIndices(nameLow, term) ?? [];
  } else if (item.source === 'window' && aliases.some((a) => a.startsWith(term))) {
    score = 720;
    indices = findSubstringIndices(nameLow, term) ?? matchSubsequence(item.name, term)?.indices ?? [];
  } else if (aliases.some((a) => a === term)) {
    score = 600;
    indices = findSubstringIndices(nameLow, term) ?? matchSubsequence(item.name, term)?.indices ?? [];
  } else if (aliases.some((a) => a.startsWith(term))) {
    score = 540;
    indices = findSubstringIndices(nameLow, term) ?? matchSubsequence(item.name, term)?.indices ?? [];
  } else if (hasWordStartMatch(nameLow, term)) {
    score = item.source === 'window' ? 620 : 460;
    indices = findSubstringIndices(nameLow, term) ?? [];
  } else {
    const subIdx = findSubstringIndices(nameLow, term);
    if (subIdx) {
      score = 290;
      indices = subIdx;
    } else {
      const fuzzy = matchSubsequence(item.name, term);
      if (fuzzy) {
        score = fuzzy.score;
        indices = fuzzy.indices;
      } else if (aliases.some((a) => a.includes(term))) {
        score = 220;
        indices = matchSubsequence(item.name, term)?.indices ?? [];
      } else if (metaLow.includes(term)) {
        score = 130;
      } else if (item.source !== 'clipboard' && targetLow.includes(term)) {
        score = 70;
      }
    }
  }

  if (score === 0) return { score: 0, indices: [] };

  // Source priority bonuses (Live open windows first!)
  if (item.source === 'window') {
    score += 140;
  } else if (item.source === 'audio_device' || item.source === 'command') {
    score += 90;
  } else if (item.pinned) {
    score += 80;
  } else if (item.source === 'win_settings') {
    score += 50;
  }

  if (item.source === 'app' || item.source === 'uwp' || item.source === 'pinned') {
    score += 30;
  } else if (item.source === 'game') {
    score += 24;
  } else if (item.source === 'folder' || item.source === 'combo') {
    score += 14;
  }

  return { score, indices };
}

/**
 * Ranks Command Palette results across open windows, bilingual system/audio commands,
 * clipboard entries, web bangs (`g `, `yt `, `gh `), pinned spaces, apps, games, and files
 * with RU<->EN wrong-layout awareness, subsequence fuzzy matching, and frecency boosting.
 * Strictly excludes any math/calculator evaluation per EXPERIENCE.md Section 3.
 */
export function rankSearchResults(
  corpus: SearchCorpusItem[],
  rawQuery: string,
  limit = 12,
  frecency?: Record<string, FrecencyEntry>,
  nowMs: number = Date.now(),
): RankedSearchResult {
  const q = rawQuery.trim().toLowerCase();

  // 1. Explicit web bangs (`g `, `yt `, `gh `)
  const webBang = parseWebBangQuery(rawQuery);
  if (webBang) {
    return {
      items: [webBang],
      swappedHint: null,
      totalMatches: 1,
      matchIndices: {},
      mode: 'web',
    };
  }

  // 2. Empty query: curated overview (live windows first + top frecency/pinned apps; skip noise commands/clips/settings)
  if (!q) {
    const openWins = corpus.filter((c) => c.source === 'window').slice(0, 3);
    const launchables = corpus
      .filter(
        (c) =>
          c.source !== 'window' &&
          c.source !== 'command' &&
          c.source !== 'win_settings' &&
          c.source !== 'audio_device' &&
          c.source !== 'clipboard',
      )
      .map((item) => ({
        item,
        weight:
          computeFrecencyBonus(frecency?.[item.frecencyKey ?? item.id], nowMs) +
          (item.pinned ? 60 : 0),
      }))
      .sort((a, b) => b.weight - a.weight)
      .map((e) => e.item);

    const combined = [...openWins, ...launchables].slice(0, limit);
    return {
      items: combined,
      swappedHint: null,
      totalMatches: openWins.length + launchables.length,
      matchIndices: {},
      mode: 'default',
    };
  }

  // 3. Explicit clipboard prefix (`clip `, `буфер `)
  const clipPrefix = parseClipboardPrefixQuery(rawQuery);
  if (clipPrefix.active) {
    const clipPool = corpus.filter((c) => c.source === 'clipboard');
    if (!clipPrefix.subQuery) {
      return {
        items: clipPool.slice(0, limit),
        swappedHint: clipPrefix.swappedPrefix,
        totalMatches: clipPool.length,
        matchIndices: {},
        mode: 'clipboard',
      };
    }

    const subQ = clipPrefix.subQuery.toLowerCase();
    const subSwapped = swapLayout(subQ);
    const canSubSwap = subSwapped !== subQ;
    const matchIndices: Record<string, number[]> = {};
    const scoredClips: Array<{ item: SearchCorpusItem; score: number; usedSwapped: boolean }> = [];

    for (const item of clipPool) {
      const nameLow = item.name.toLowerCase();
      const fullLow = (item.clipText ?? item.target).toLowerCase();
      const directIdx = findSubstringIndices(nameLow, subQ);
      if (directIdx || fullLow.includes(subQ)) {
        if (directIdx) matchIndices[item.id] = directIdx;
        scoredClips.push({ item, score: (directIdx ? 600 : 380) + (item.pinned ? 50 : 0), usedSwapped: false });
        continue;
      }
      if (canSubSwap) {
        const swIdx = findSubstringIndices(nameLow, subSwapped);
        if (swIdx || fullLow.includes(subSwapped)) {
          if (swIdx) matchIndices[item.id] = swIdx;
          scoredClips.push({ item, score: (swIdx ? 540 : 320) + (item.pinned ? 50 : 0), usedSwapped: true });
        }
      }
    }

    scoredClips.sort((a, b) => b.score - a.score);
    const top = scoredClips.slice(0, limit);
    return {
      items: top.map((e) => e.item),
      swappedHint: clipPrefix.swappedPrefix ?? (top.some((e) => e.usedSwapped) ? subSwapped : null),
      totalMatches: scoredClips.length,
      matchIndices,
      mode: 'clipboard',
    };
  }

  // 4. General multi-source ranked search
  const swapped = swapLayout(q);
  const canSwap = swapped !== q;

  const scored: Array<{
    item: SearchCorpusItem;
    score: number;
    usedSwapped: boolean;
    indices: number[];
  }> = [];

  for (const item of corpus) {
    // Keep general search clean from noisy 1-char clipboard hits
    if (item.source === 'clipboard' && q.length < 3) {
      continue;
    }

    const nameLow = item.name.toLowerCase();
    const metaLow = `${item.category} ${item.subtitle}`.toLowerCase();
    const targetLow = item.target.toLowerCase();
    const itemKeywords = (item.keywords ?? []).map((k) => k.toLowerCase());
    const aliases = [...collectPhoneticAliases(nameLow, metaLow, targetLow), ...itemKeywords];
    const frecBonus = computeFrecencyBonus(frecency?.[item.frecencyKey ?? item.id], nowMs);

    const direct = scoreTerm(item, nameLow, aliases, metaLow, targetLow, q);
    if (direct.score > 0) {
      scored.push({
        item,
        score: direct.score + 40 + frecBonus,
        usedSwapped: false,
        indices: direct.indices,
      });
      continue;
    }

    if (canSwap) {
      const sw = scoreTerm(item, nameLow, aliases, metaLow, targetLow, swapped);
      if (sw.score > 0) {
        scored.push({
          item,
          score: sw.score + frecBonus,
          usedSwapped: true,
          indices: sw.indices,
        });
      }
    }
  }

  scored.sort((a, b) => {
    if (b.score !== a.score) return b.score - a.score;
    if (a.item.name.length !== b.item.name.length) return a.item.name.length - b.item.name.length;
    return a.item.name.localeCompare(b.item.name, 'ru');
  });

  const top = scored.slice(0, limit);
  const swappedHint = top.length > 0 && top.some((entry) => entry.usedSwapped) ? swapped : null;
  const matchIndices: Record<string, number[]> = {};
  for (const entry of top) {
    if (entry.indices.length > 0) {
      matchIndices[entry.item.id] = entry.indices;
    }
  }

  return {
    items: top.map((entry) => entry.item),
    swappedHint,
    totalMatches: scored.length,
    matchIndices,
    mode: 'default',
  };
}

export type SearchLaunchDispatch =
  | { command: 'launch_combo'; args: { targets: string[] } }
  | {
      command: 'launch_item';
      args: { target: string; args: string | null; asAdmin: boolean; closeStart: boolean };
    }
  | { command: 'focus_window'; args: { hwnd: number; toggleMinimize: boolean } }
  | { command: 'set_audio_device'; args: { deviceId: string } }
  | { command: 'set_mic_muted'; args: { muted: boolean } }
  | { command: 'empty_recycle_bin'; args: Record<string, never> }
  | { command: 'open_screen_snip'; args: Record<string, never> }
  | { command: 'refresh_indexes'; args: Record<string, never> }
  | { command: 'power_action'; args: { action: 'lock' | 'sleep' | 'reboot' | 'shutdown' } }
  | { command: 'open_settings'; args: Record<string, never> }
  | { command: 'paste_clipboard_entry'; args: { text: string } };

export function resolveSearchLaunchPayload(
  item: SearchCorpusItem,
  ctrlKey: boolean,
  platform: AppPlatform = detectPlatform(),
): SearchLaunchDispatch {
  if (item.source === 'window' && item.hwnd) {
    return {
      command: 'focus_window',
      args: { hwnd: item.hwnd, toggleMinimize: false },
    };
  }

  if (item.source === 'clipboard') {
    return {
      command: 'paste_clipboard_entry',
      args: { text: item.clipText ?? item.target },
    };
  }

  if (item.source === 'audio_device' && item.deviceId) {
    return {
      command: 'set_audio_device',
      args: { deviceId: item.deviceId },
    };
  }

  if (item.source === 'command' && item.commandId) {
    switch (item.commandId) {
      case 'mic_toggle':
        return { command: 'set_mic_muted', args: { muted: !item.micMuted } };
      case 'empty_recycle_bin':
        return { command: 'empty_recycle_bin', args: {} };
      case 'screenshot':
        return { command: 'open_screen_snip', args: {} };
      case 'refresh_indexes':
        return { command: 'refresh_indexes', args: {} };
      case 'settings':
        return { command: 'open_settings', args: {} };
      case 'lock':
      case 'sleep':
      case 'reboot':
      case 'shutdown':
        return { command: 'power_action', args: { action: item.commandId } };
      case 'task_manager':
        return {
          command: 'launch_item',
          args: {
            target: item.target,
            args: null,
            asAdmin: platform !== 'macos' && ctrlKey,
            closeStart: true,
          },
        };
      default:
        break;
    }
  }

  if (item.kind === 'combo') {
    return {
      command: 'launch_combo',
      args: { targets: item.comboTargets ?? [] },
    };
  }

  return {
    command: 'launch_item',
    args: {
      target: item.target,
      args: item.args ?? null,
      asAdmin: ctrlKey && item.source !== 'win_settings' && canRunAsAdmin(item.target, platform),
      closeStart: true,
    },
  };
}

function extractBasenameLower(path: string): string {
  const norm = normKey(path);
  if (!norm || norm.endsWith('\\')) return '';
  const idx = norm.lastIndexOf('\\');
  return idx >= 0 ? norm.slice(idx + 1) : norm;
}

function extractAppsFolderAumid(target: string): string {
  const trimmed = target.trim();
  const prefix = 'shell:appsfolder\\';
  return trimmed.toLowerCase().startsWith(prefix)
    ? trimmed.slice(prefix.length).trim()
    : '';
}

/**
 * Converts a pinnable search result into a canonical `PinItem` (for Taskbar or active Space),
 * preferring `shell:AppsFolder\<aumid>` for non-launchable UWP host executables and returning
 * `null` for transient non-pinnable entries (clipboard, audio devices, web bangs, power actions).
 */
export function buildPinFromSearchItem(
  item: SearchCorpusItem,
  id = `pin-${Date.now()}`,
  platform: AppPlatform = detectPlatform(),
): PinItem | null {
  if (
    item.source === 'clipboard' ||
    item.source === 'audio_device' ||
    item.source === 'web' ||
    item.kind === 'combo'
  ) {
    return null;
  }
  if (
    item.source === 'command' &&
    item.commandId !== 'task_manager' &&
    item.commandId !== 'settings'
  ) {
    return null;
  }

  const trimmedAumid = item.matchAumid?.trim() || '';
  const rawTarget = item.target.trim();
  const resolvedTarget =
    trimmedAumid && (!rawTarget || isNonLaunchableUwpHostExe(rawTarget))
      ? formatAppsFolderTarget(trimmedAumid)
      : rawTarget || (trimmedAumid ? formatAppsFolderTarget(trimmedAumid) : '');
  if (!resolvedTarget) return null;

  const title =
    (item.source === 'window' ? item.subtitle.trim() || item.name.trim() : item.name.trim()) ||
    'Элемент';

  if (item.kind === 'folder') {
    return {
      id,
      kind: 'folder',
      title,
      subtitle: 'Папка',
      target: resolvedTarget,
      args: null,
      altAction: 'Открыть в терминале',
      accentRgb: '242,180,75',
      iconKey: item.iconKey || 'folder',
      iconData: null,
      matchExe: null,
      matchAumid: null,
    };
  }

  if (item.kind === 'file') {
    return {
      id,
      kind: 'file',
      title,
      subtitle: 'Файл',
      target: resolvedTarget,
      args: null,
      altAction: 'Скопировать путь',
      accentRgb: '82,184,242',
      iconKey: item.iconKey || 'file',
      iconData: null,
      matchExe: null,
      matchAumid: null,
    };
  }

  if (item.kind === 'url') {
    return {
      id,
      kind: 'url',
      title,
      subtitle: 'Ссылка',
      target: resolvedTarget,
      args: null,
      altAction: 'Скопировать путь',
      accentRgb: '167,139,250',
      iconKey: item.iconKey || 'url',
      iconData: null,
      matchExe: null,
      matchAumid: null,
    };
  }

  const resolvedAumid = trimmedAumid || extractAppsFolderAumid(resolvedTarget) || null;
  const exeBase =
    !resolvedTarget.toLowerCase().startsWith('shell:appsfolder\\') &&
    !isNonLaunchableUwpHostExe(resolvedTarget)
      ? extractBasenameLower(resolvedTarget)
      : '';
  const matchExe =
    exeBase && exeBase.endsWith('.exe') && exeBase !== 'applicationframehost.exe'
      ? exeBase
      : null;
  const canElevate =
    platform !== 'macos' &&
    item.source !== 'win_settings' &&
    item.commandId !== 'settings' &&
    canRunAsAdmin(resolvedTarget, platform);

  return {
    id,
    kind: 'app',
    title,
    subtitle:
      item.source === 'win_settings'
        ? 'Параметры'
        : item.source === 'game'
          ? item.subtitle || 'Игра'
          : item.commandId === 'settings'
            ? 'PrismBar'
            : item.commandId === 'task_manager'
              ? 'Система'
              : item.subtitle || 'Приложение',
    target: resolvedTarget,
    args: item.args ?? null,
    altAction: canElevate ? 'От имени администратора' : '',
    accentRgb: '129,140,248',
    iconKey: item.cacheIconKey || item.iconKey || title.toLowerCase(),
    iconData: null,
    matchExe,
    matchAumid: resolvedAumid,
  };
}

export type SearchSecondaryAction =
  | { type: 'close_window'; hwnd: number; label: string }
  | { type: 'open_terminal'; path: string; label: string }
  | { type: 'open_location'; path: string; label: string }
  | { type: 'copy_clipboard'; text: string; label: string };

export function resolveSearchSecondaryAction(
  item: SearchCorpusItem | undefined,
  platform: AppPlatform = detectPlatform(),
): SearchSecondaryAction | null {
  if (!item) return null;
  if (item.source === 'window' && item.hwnd) {
    return { type: 'close_window', hwnd: item.hwnd, label: 'Закрыть окно' };
  }
  if (item.source === 'clipboard') {
    return { type: 'copy_clipboard', text: item.clipText ?? item.target, label: 'Копировать' };
  }
  if (item.kind === 'folder' && canOpenFileLocation(item.target)) {
    return { type: 'open_terminal', path: item.target, label: 'Терминал' };
  }
  if (
    item.source !== 'command' &&
    item.source !== 'win_settings' &&
    item.source !== 'audio_device' &&
    item.source !== 'web' &&
    canOpenFileLocation(item.target)
  ) {
    return {
      type: 'open_location',
      path: item.target,
      label: platform === 'macos' ? 'Показать в Finder' : 'Расположение',
    };
  }
  return null;
}

export function getPrimaryActionLabel(item: SearchCorpusItem | undefined): string {
  if (!item) return 'Открыть';
  if (item.source === 'window') return 'Перейти';
  if (item.source === 'clipboard') return 'Вставить';
  if (item.source === 'audio_device') return 'Переключить';
  if (item.source === 'command') return 'Выполнить';
  if (item.source === 'web') return 'Найти';
  return 'Открыть';
}

export function isPrintableSearchTrigger(
  e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'altKey' | 'metaKey'>,
): boolean {
  if (e.ctrlKey || e.altKey || e.metaKey) return false;
  if (e.key.length !== 1) return false;
  // Ignore plain Space so Space can still activate a focused button without hijacking
  return e.key !== ' ';
}
