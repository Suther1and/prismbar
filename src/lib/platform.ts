import type { SpaceWidgetId } from './ipc';

export type AppPlatform = 'windows' | 'macos';

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

const MACOS_UNSUPPORTED_WIDGETS: ReadonlySet<SpaceWidgetId> = new Set([
  'audio_mixer',
  'media',
  'game_combos',
]);

const ALL_SPACE_WIDGET_IDS: readonly SpaceWidgetId[] = SPACE_WIDGET_CATALOG.map((w) => w.id);
const MACOS_SPACE_WIDGET_IDS: readonly SpaceWidgetId[] = ALL_SPACE_WIDGET_IDS.filter(
  (id) => !MACOS_UNSUPPORTED_WIDGETS.has(id),
);

/**
 * Detects runtime UI platform (`'windows'` vs `'macos'`), honoring explicit `?platform=`
 * query override first and defaulting safely to `'windows'` in Node/SSR unit tests.
 */
export function detectPlatform(): AppPlatform {
  if (typeof window === 'undefined') {
    return 'windows';
  }
  try {
    const search = window.location?.search;
    if (typeof search === 'string' && search.length > 0) {
      const param = new URLSearchParams(search).get('platform');
      if (param === 'macos') return 'macos';
      if (param === 'windows') return 'windows';
    }
  } catch {
    // Ignore malformed window.location in synthetic test environments
  }
  const nav =
    (window as { navigator?: Navigator }).navigator ??
    (typeof navigator !== 'undefined' ? navigator : undefined);
  if (nav) {
    const ua = String(nav.userAgent ?? '');
    // Ignore Node's built-in `navigator.platform` so Vitest SSR tests on macOS CI stay deterministic
    if (!ua.startsWith('Node.js/')) {
      const plat = String(nav.platform ?? '');
      if (/mac/i.test(plat) || /macintosh|mac os x/i.test(ua)) {
        return 'macos';
      }
    }
  }
  return 'windows';
}

export function getAvailableSpaceWidgetIds(platform: AppPlatform): readonly SpaceWidgetId[] {
  return platform === 'macos' ? MACOS_SPACE_WIDGET_IDS : ALL_SPACE_WIDGET_IDS;
}

export function getSupportedSpaceWidgetCatalog(
  platform: AppPlatform,
): readonly SpaceWidgetOption[] {
  const allowed = new Set<SpaceWidgetId>(getAvailableSpaceWidgetIds(platform));
  return SPACE_WIDGET_CATALOG.filter((w) => allowed.has(w.id));
}

export function isNativeIconTarget(target: string): boolean {
  const t = target.trim();
  if (!t) return false;
  const low = t.toLowerCase();
  if (low.includes('://')) return false;
  if (t.startsWith('/')) return true;
  if (/^[a-z]:[\\/]/i.test(t) || t.startsWith('\\\\')) return true;
  if (low.startsWith('shell:appsfolder\\')) return true;
  return false;
}

export function isMacAppBundle(path: string | null | undefined): boolean {
  const trimmed = (path ?? '').trim().replace(/[\\/]+$/, '').toLowerCase();
  return trimmed.length > 4 && trimmed.endsWith('.app');
}

export function extractTargetBasename(path: string): string {
  const trimmed = path.trim();
  if (!trimmed) return '';
  const base = trimmed.split(/[\\/]/).filter(Boolean).pop() ?? trimmed;
  const stripped = base.replace(/\.(exe|app)$/i, '');
  return stripped || base;
}
