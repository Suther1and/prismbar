const EN_TO_RU: Record<string, string> = {
  q: 'й', w: 'ц', e: 'у', r: 'к', t: 'е', y: 'н', u: 'г', i: 'ш', o: 'щ', p: 'з', '[': 'х', ']': 'ъ',
  a: 'ф', s: 'ы', d: 'в', f: 'а', g: 'п', h: 'р', j: 'о', k: 'л', l: 'д', ';': 'ж', "'": 'э',
  z: 'я', x: 'ч', c: 'с', v: 'м', b: 'и', n: 'т', m: 'ь', ',': 'б', '.': 'ю',
};

const RU_TO_EN: Record<string, string> = {};
Object.entries(EN_TO_RU).forEach(([en, ru]) => {
  RU_TO_EN[ru] = en;
});

const PHONETIC_ALIASES: Record<string, string[]> = {
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
  vscode: ['вскод', 'код'],
  cursor: ['курсор'],
  antigravity: ['антигравити'],
  yandex: ['яндекс', 'музыка'],
};

export function swapLayout(str: string): string {
  return [...str.toLowerCase()]
    .map((ch) => EN_TO_RU[ch] || RU_TO_EN[ch] || ch)
    .join('');
}

export function matchesQuery(rawQuery: string, ...fields: string[]): { matched: boolean; swapped: string | null } {
  const q = rawQuery.trim().toLowerCase();
  if (!q) return { matched: true, swapped: null };

  const hay = fields.join(' ').toLowerCase();
  // Enrich with phonetic aliases
  let enriched = hay;
  for (const [k, aliases] of Object.entries(PHONETIC_ALIASES)) {
    if (hay.includes(k)) {
      enriched += ' ' + aliases.join(' ');
    }
  }

  if (enriched.includes(q)) return { matched: true, swapped: null };
  const sw = swapLayout(q);
  if (sw !== q && enriched.includes(sw)) return { matched: true, swapped: sw };
  return { matched: false, swapped: null };
}
