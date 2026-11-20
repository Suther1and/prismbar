const EN_TO_RU: Record<string, string> = {
  q: 'й', w: 'ц', e: 'у', r: 'к', t: 'е', y: 'н', u: 'г', i: 'ш', o: 'щ', p: 'з', '[': 'х', ']': 'ъ',
  a: 'ф', s: 'ы', d: 'в', f: 'а', g: 'п', h: 'р', j: 'о', k: 'л', l: 'д', ';': 'ж', "'": 'э',
  z: 'я', x: 'ч', c: 'с', v: 'м', b: 'и', n: 'т', m: 'ь', ',': 'б', '.': 'ю',
};

const RU_TO_EN: Record<string, string> = {};
Object.entries(EN_TO_RU).forEach(([en, ru]) => {
  RU_TO_EN[ru] = en;
});

export const PHONETIC_ALIASES: Record<string, string[]> = {
  telegram: ['телеграм', 'телега', 'тг'],
  zen: ['зен', 'браузер'],
  spark: ['спарк', 'почта'],
  photoshop: ['фотошоп', 'фш'],
  lightroom: ['лайтрум'],
  obsidian: ['обсидиан', 'второй мозг'],
  discord: ['дискорд', 'дс'],
  explorer: ['проводник', 'файлы'],
  steam: ['стим', 'игры'],
  'path of exile': ['пое', 'poe', 'пое2', 'poe2'],
  dota: ['дота', 'dota'],
  vscode: ['вскод', 'код', 'vs code'],
  'vs code': ['вскод', 'код', 'vscode'],
  cursor: ['курсор'],
  antigravity: ['антигравити'],
  yandex: ['яндекс', 'музыка'],
  calculator: ['калькулятор', 'кальк'],
  калькулятор: ['calculator', 'calc'],
  settings: ['параметры', 'настройки'],
  параметры: ['settings', 'control panel'],
  terminal: ['терминал', 'консоль', 'wt'],
  терминал: ['terminal', 'console', 'wt'],
  paint: ['пейнт', 'рисование'],
  notepad: ['блокнот'],
  блокнот: ['notepad'],
  store: ['магазин', 'стор', 'microsoft store'],
  chrome: ['хром', 'браузер'],
  edge: ['эдж', 'браузер'],
  figma: ['фигма'],
  blender: ['блендер'],
  obs: ['обс', 'запись'],
  snipping: ['ножницы', 'скриншот'],
  ножницы: ['snipping', 'screenshot'],
  photos: ['фотографии', 'фото'],
  фотографии: ['photos'],
};

export function swapLayout(str: string): string {
  return [...str.toLowerCase()]
    .map((ch) => EN_TO_RU[ch] || RU_TO_EN[ch] || ch)
    .join('');
}

export function collectPhoneticAliases(...fields: string[]): string[] {
  const hay = fields.join(' ').toLowerCase();
  const out: string[] = [];
  for (const [k, aliases] of Object.entries(PHONETIC_ALIASES)) {
    if (hay.includes(k)) {
      out.push(...aliases);
    }
  }
  return out;
}

export function matchesQuery(rawQuery: string, ...fields: string[]): { matched: boolean; swapped: string | null } {
  const q = rawQuery.trim().toLowerCase();
  if (!q) return { matched: true, swapped: null };

  const hay = fields.join(' ').toLowerCase();
  const aliases = collectPhoneticAliases(hay);
  const enriched = aliases.length > 0 ? `${hay} ${aliases.join(' ')}` : hay;

  if (enriched.includes(q)) return { matched: true, swapped: null };
  const sw = swapLayout(q);
  if (sw !== q && enriched.includes(sw)) return { matched: true, swapped: sw };
  return { matched: false, swapped: null };
}
