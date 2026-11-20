export const MONTHS_RU = [
  'Январь',
  'Февраль',
  'Март',
  'Апрель',
  'Май',
  'Июнь',
  'Июль',
  'Август',
  'Сентябрь',
  'Октябрь',
  'Ноябрь',
  'Декабрь',
] as const;

const MONTHS_GENITIVE_RU = [
  'января',
  'февраля',
  'марта',
  'апреля',
  'мая',
  'июня',
  'июля',
  'августа',
  'сентября',
  'октября',
  'ноября',
  'декабря',
] as const;

const WEEKDAYS_FULL_RU = [
  'Воскресенье',
  'Понедельник',
  'Вторник',
  'Среда',
  'Четверг',
  'Пятница',
  'Суббота',
] as const;

export const WEEKDAYS_SHORT_RU = ['Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб', 'Вс'] as const;

export const CALENDAR_GRID_CELLS = 42; // 6 rows x 7 columns (invariant height across all months)

export interface CalendarMonthRef {
  year: number;
  month: number; // 0..11
}

export interface CalendarDayCell {
  key: string;
  year: number;
  month: number;
  day: number;
  inCurrentMonth: boolean;
  isToday: boolean;
  isSelected: boolean;
}

export function isSameCalendarDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export function isViewingTodayMonth(view: CalendarMonthRef, today: Date): boolean {
  return view.year === today.getFullYear() && view.month === today.getMonth();
}

export function shiftCalendarMonth(
  year: number,
  month: number,
  delta: number,
): CalendarMonthRef {
  const shifted = new Date(year, month + delta, 1);
  return {
    year: shifted.getFullYear(),
    month: shifted.getMonth(),
  };
}

function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

function makeDayKey(year: number, month: number, day: number): string {
  return `${year}-${pad2(month + 1)}-${pad2(day)}`;
}

/**
 * Generates a fixed 42-cell (6x7) Monday-first Russian calendar grid for `(viewYear, viewMonth)`.
 * Fixed row count guarantees zero vertical layout jitter when stepping between months.
 */
export function buildCalendarMonthGrid(
  viewYear: number,
  viewMonth: number,
  today: Date,
  selectedDate: Date,
): CalendarDayCell[] {
  const firstOfMonth = new Date(viewYear, viewMonth, 1);
  // JS getDay(): 0 = Sun, 1 = Mon, ..., 6 = Sat -> convert to Monday-first 0..6
  const mondayOffset = (firstOfMonth.getDay() + 6) % 7;
  const daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate();
  const prevRef = shiftCalendarMonth(viewYear, viewMonth, -1);
  const nextRef = shiftCalendarMonth(viewYear, viewMonth, 1);
  const daysInPrevMonth = new Date(viewYear, viewMonth, 0).getDate();

  const todayY = today.getFullYear();
  const todayM = today.getMonth();
  const todayD = today.getDate();

  const selY = selectedDate.getFullYear();
  const selM = selectedDate.getMonth();
  const selD = selectedDate.getDate();

  const cells: CalendarDayCell[] = [];

  for (let i = mondayOffset - 1; i >= 0; i--) {
    const day = daysInPrevMonth - i;
    cells.push({
      key: makeDayKey(prevRef.year, prevRef.month, day),
      year: prevRef.year,
      month: prevRef.month,
      day,
      inCurrentMonth: false,
      isToday: prevRef.year === todayY && prevRef.month === todayM && day === todayD,
      isSelected: prevRef.year === selY && prevRef.month === selM && day === selD,
    });
  }

  for (let day = 1; day <= daysInMonth; day++) {
    cells.push({
      key: makeDayKey(viewYear, viewMonth, day),
      year: viewYear,
      month: viewMonth,
      day,
      inCurrentMonth: true,
      isToday: viewYear === todayY && viewMonth === todayM && day === todayD,
      isSelected: viewYear === selY && viewMonth === selM && day === selD,
    });
  }

  let nextDay = 1;
  while (cells.length < CALENDAR_GRID_CELLS) {
    cells.push({
      key: makeDayKey(nextRef.year, nextRef.month, nextDay),
      year: nextRef.year,
      month: nextRef.month,
      day: nextDay,
      inCurrentMonth: false,
      isToday: nextRef.year === todayY && nextRef.month === todayM && nextDay === todayD,
      isSelected: nextRef.year === selY && nextRef.month === selM && nextDay === selD,
    });
    nextDay += 1;
  }

  return cells;
}

export function formatSelectedDateHeader(date: Date): string {
  const weekday = WEEKDAYS_FULL_RU[date.getDay()] ?? '';
  const monthGen = MONTHS_GENITIVE_RU[date.getMonth()] ?? '';
  return `${weekday}, ${date.getDate()} ${monthGen} ${date.getFullYear()}`;
}

export function formatAlertTime(timestampMs: number): string {
  if (!Number.isFinite(timestampMs) || timestampMs <= 0) return '';
  const d = new Date(timestampMs);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}
