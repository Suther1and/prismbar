import { describe, expect, it } from 'vitest';
import { matchesQuery, swapLayout } from './layoutMap';

describe('swapLayout', () => {
  it('swaps EN QWERTY characters to RU ЙЦУКЕН', () => {
    expect(swapLayout('ntkt')).toBe('теле');
    expect(swapLayout('ghbdtn')).toBe('привет');
    expect(swapLayout('NTKT')).toBe('теле');
  });

  it('swaps RU ЙЦУКЕН characters to EN QWERTY including uppercase Russian', () => {
    expect(swapLayout('сгкыщк')).toBe('cursor');
    expect(swapLayout('СГКЫЩК')).toBe('cursor');
    expect(swapLayout('ыеуфь')).toBe('steam');
  });

  it('swaps ЙЦУКЕН punctuation keys mapped to Cyrillic letters', () => {
    expect(swapLayout(',hfepth')).toBe('браузер');
    expect(swapLayout(".;',[]")).toBe('южэбхъ');
  });

  it('preserves digits, spaces, and unmapped punctuation', () => {
    expect(swapLayout('poe 2!')).toBe('зщу 2!');
  });
});

describe('matchesQuery', () => {
  it('matches wrong-layout EN input against Russian phonetic alias for Telegram', () => {
    expect(matchesQuery('ntkt', 'Telegram')).toEqual({
      matched: true,
      swapped: 'теле',
    });
  });

  it('matches wrong-layout punctuation-to-Cyrillic input against phonetic alias', () => {
    expect(matchesQuery(',hfepth', 'Zen')).toEqual({
      matched: true,
      swapped: 'браузер',
    });
  });

  it('matches uppercase Russian input directly and via layout swap', () => {
    expect(matchesQuery('ТЕЛЕГА', 'Telegram')).toEqual({
      matched: true,
      swapped: null,
    });
    expect(matchesQuery('СГКЫЩК', 'Cursor')).toEqual({
      matched: true,
      swapped: 'cursor',
    });
  });

  it('matches direct query without reporting layout swap', () => {
    expect(matchesQuery('tele', 'Telegram')).toEqual({
      matched: true,
      swapped: null,
    });
  });

  it('matches empty or whitespace query immediately', () => {
    expect(matchesQuery('   ', 'Telegram')).toEqual({
      matched: true,
      swapped: null,
    });
  });

  it('matches Russian phonetic aliases directly', () => {
    expect(matchesQuery('телега', 'Telegram')).toEqual({
      matched: true,
      swapped: null,
    });
    expect(matchesQuery('дс', 'Discord')).toEqual({
      matched: true,
      swapped: null,
    });
    expect(matchesQuery('пое2', 'Path of Exile')).toEqual({
      matched: true,
      swapped: null,
    });
  });

  it('matches wrong-layout RU input against EN app names', () => {
    expect(matchesQuery('сгкыщк', 'Cursor')).toEqual({
      matched: true,
      swapped: 'cursor',
    });
  });

  it('searches across multiple fields and returns false on miss', () => {
    expect(matchesQuery('студия', 'Photoshop', 'Студия')).toEqual({
      matched: true,
      swapped: null,
    });
    expect(matchesQuery('zzzznotfound', 'Telegram', 'Личное')).toEqual({
      matched: false,
      swapped: null,
    });
  });
});
