import { describe, expect, it } from 'vitest';
import {
  buildCalendarMonthGrid,
  CALENDAR_GRID_CELLS,
  formatAlertTime,
  formatSelectedDateHeader,
  isSameCalendarDay,
  isViewingTodayMonth,
  shiftCalendarMonth,
  WEEKDAYS_SHORT_RU,
} from './calendarLogic';

describe('Calendar month grid generation, leap years, navigation & date selection', () => {
  it('uses Monday-first Russian weekdays and fixed 42-cell grid for zero height jump', () => {
    expect(WEEKDAYS_SHORT_RU).toEqual(['Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб', 'Вс']);

    // November 2026 starts on Sunday (offset = 6) -> needs 6 leading Oct days + 30 Nov days + 6 trailing Dec days = 42
    const today = new Date(2026, 10, 21); // Nov 21, 2026
    const selected = new Date(2026, 10, 25);
    const cells = buildCalendarMonthGrid(2026, 10, today, selected);

    expect(cells).toHaveLength(CALENDAR_GRID_CELLS);
    // Leading October days: Oct 26 (Mon) .. Oct 31 (Sat)
    expect(cells[0]).toMatchObject({ year: 2026, month: 9, day: 26, inCurrentMonth: false });
    expect(cells[5]).toMatchObject({ year: 2026, month: 9, day: 31, inCurrentMonth: false });
    // Nov 1 is Sunday -> index 6
    expect(cells[6]).toMatchObject({ year: 2026, month: 10, day: 1, inCurrentMonth: true });
    // Nov 30 is index 35
    expect(cells[35]).toMatchObject({ year: 2026, month: 10, day: 30, inCurrentMonth: true });
    // Trailing December days: Dec 1 .. Dec 6
    expect(cells[36]).toMatchObject({ year: 2026, month: 11, day: 1, inCurrentMonth: false });
    expect(cells[41]).toMatchObject({ year: 2026, month: 11, day: 6, inCurrentMonth: false });

    // Today (Nov 21) and Selected (Nov 25) are independently flagged
    const todayCell = cells.find((c) => c.key === '2026-11-21');
    const selectedCell = cells.find((c) => c.key === '2026-11-25');
    expect(todayCell?.isToday).toBe(true);
    expect(todayCell?.isSelected).toBe(false);
    expect(selectedCell?.isToday).toBe(false);
    expect(selectedCell?.isSelected).toBe(true);
  });

  it('handles leap-year February (2024, 2000) vs non-leap February (2025, 1900)', () => {
    const ref = new Date(2024, 1, 29);

    const feb2024 = buildCalendarMonthGrid(2024, 1, ref, ref).filter((c) => c.inCurrentMonth);
    expect(feb2024).toHaveLength(29);
    expect(feb2024[28]).toMatchObject({ day: 29, isToday: true, isSelected: true });

    const feb2025 = buildCalendarMonthGrid(2025, 1, ref, ref).filter((c) => c.inCurrentMonth);
    expect(feb2025).toHaveLength(28);

    const feb2000 = buildCalendarMonthGrid(2000, 1, ref, ref).filter((c) => c.inCurrentMonth);
    expect(feb2000).toHaveLength(29);

    const feb1900 = buildCalendarMonthGrid(1900, 1, ref, ref).filter((c) => c.inCurrentMonth);
    expect(feb1900).toHaveLength(28);
  });

  it('shifts months cleanly across year boundaries and tracks Today navigation', () => {
    expect(shiftCalendarMonth(2026, 0, -1)).toEqual({ year: 2025, month: 11 });
    expect(shiftCalendarMonth(2026, 11, 1)).toEqual({ year: 2027, month: 0 });
    expect(shiftCalendarMonth(2026, 5, 1)).toEqual({ year: 2026, month: 6 });

    const today = new Date(2026, 10, 21);
    const otherDay = new Date(2026, 11, 5);
    expect(isSameCalendarDay(today, new Date(2026, 10, 21, 23, 59))).toBe(true);
    expect(isSameCalendarDay(today, otherDay)).toBe(false);
    expect(isViewingTodayMonth({ year: 2026, month: 10 }, today)).toBe(true);
    expect(isViewingTodayMonth({ year: 2026, month: 11 }, today)).toBe(false);
  });

  it('formats Russian selected-date header and alert timestamps cleanly', () => {
    // 2026-11-21 is Saturday
    expect(formatSelectedDateHeader(new Date(2026, 10, 21))).toBe('Суббота, 21 ноября 2026');
    // 2024-02-29 is Thursday
    expect(formatSelectedDateHeader(new Date(2024, 1, 29))).toBe('Четверг, 29 февраля 2024');

    const ts = new Date(2026, 10, 21, 9, 5).getTime();
    expect(formatAlertTime(ts)).toBe('09:05');
    expect(formatAlertTime(0)).toBe('');
  });
});
