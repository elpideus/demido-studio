import { describe, expect, it } from 'vitest';

import {
  dayKey,
  firstWeekday,
  fromRange,
  monthGrid,
  parseTime,
  presets,
  rangeLabel,
  sameDay,
  toRange,
  weekdayNames,
} from './dateRange';

const ms = (...parts: number[]) => new Date(...(parts as [number, number])).getTime();

describe('the date search calendar', () => {
  it('lays a month out in six weeks from the first day of the week', () => {
    // 1 October 2026 is a Thursday.
    const monday = monthGrid(2026, 9, 1);
    expect(monday).toHaveLength(42);
    expect(monday[0]).toEqual(new Date(2026, 8, 28));
    expect(monday[3]).toEqual(new Date(2026, 9, 1));
    expect(monday[41]).toEqual(new Date(2026, 10, 8));
    const sunday = monthGrid(2026, 9, 0);
    expect(sunday[0]).toEqual(new Date(2026, 8, 27));
    expect(sunday[4]).toEqual(new Date(2026, 9, 1));
    // A month starting on the first day of the week starts the grid.
    expect(monthGrid(2026, 5, 1)[0]).toEqual(new Date(2026, 5, 1));
  });

  it('names the weekdays in grid order, and finds the first day of the week', () => {
    expect(weekdayNames(1)).toHaveLength(7);
    expect(weekdayNames(0)[1]).toBe(weekdayNames(1)[0]);
    expect(firstWeekday('en-US')).toBe(0);
    expect(firstWeekday('it-IT')).toBe(1);
    expect(firstWeekday('not a locale')).toBe(1);
  });

  it('compares days, not moments', () => {
    expect(sameDay(new Date(2026, 2, 3, 1), new Date(2026, 2, 3, 23))).toBe(true);
    expect(sameDay(new Date(2026, 2, 3), new Date(2026, 2, 4))).toBe(false);
    expect(sameDay(null, new Date())).toBe(false);
    expect(dayKey(new Date(2026, 0, 31))).toBeLessThan(dayKey(new Date(2026, 1, 1)));
  });

  it('reads times', () => {
    expect(parseTime('09:30')).toEqual([9, 30]);
    expect(parseTime('23:59')).toEqual([23, 59]);
    expect(parseTime('24:00')).toBeNull();
    expect(parseTime('')).toBeNull();
  });

  it('spans whole days unless times are chosen, with the days in either order', () => {
    expect(toRange(new Date(2026, 2, 9), new Date(2026, 2, 3))).toEqual({
      since: ms(2026, 2, 3),
      until: ms(2026, 2, 9, 23, 59, 59, 999),
    });
    expect(toRange(new Date(2026, 2, 3), new Date(2026, 2, 3), '14:00', '18:30')).toEqual({
      since: ms(2026, 2, 3, 14, 0),
      until: ms(2026, 2, 3, 18, 30, 59, 999),
    });
    // Times that would end before they start swap.
    expect(toRange(new Date(2026, 2, 3), new Date(2026, 2, 3), '18:00', '14:00')).toEqual({
      since: ms(2026, 2, 3, 14, 0),
      until: ms(2026, 2, 3, 18, 0, 59, 999),
    });
    // An unreadable time falls back to the whole day.
    expect(toRange(new Date(2026, 2, 3), new Date(2026, 2, 3), '', '')).toEqual({
      since: ms(2026, 2, 3),
      until: ms(2026, 2, 3, 23, 59, 59, 999),
    });
  });

  it('opens on the range in use', () => {
    const range = toRange(new Date(2026, 2, 3), new Date(2026, 2, 9), '08:15', '17:45');
    expect(fromRange(range)).toEqual({
      start: new Date(2026, 2, 3),
      end: new Date(2026, 2, 9),
      fromTime: '08:15',
      toTime: '17:45',
    });
    expect(fromRange(null)).toEqual({ start: null, end: null, fromTime: '00:00', toTime: '23:59' });
  });

  it('offers whole-day ranges up to today', () => {
    const now = new Date(2026, 9, 9, 15, 20);
    const byLabel = Object.fromEntries(presets(now).map((p) => [p.label, p.range]));
    const endOfToday = ms(2026, 9, 9, 23, 59, 59, 999);
    expect(byLabel.Today).toEqual({ since: ms(2026, 9, 9), until: endOfToday });
    expect(byLabel.Yesterday).toEqual({ since: ms(2026, 9, 8), until: ms(2026, 9, 8, 23, 59, 59, 999) });
    expect(byLabel['Last 7 days']).toEqual({ since: ms(2026, 9, 3), until: endOfToday });
    expect(byLabel['Last 30 days']).toEqual({ since: ms(2026, 8, 10), until: endOfToday });
    expect(byLabel['This year']).toEqual({ since: ms(2026, 0, 1), until: endOfToday });
  });

  it('names a range by its days, with times only when it is not whole days', () => {
    const now = new Date(2026, 9, 9);
    const days = rangeLabel(toRange(new Date(2026, 2, 3), new Date(2026, 2, 9)), now);
    expect(days).toContain('–');
    expect(days).not.toMatch(/2026|:/);
    expect(rangeLabel(toRange(new Date(2026, 2, 3), new Date(2026, 2, 3)), now)).not.toContain('–');
    const timed = rangeLabel(toRange(new Date(2026, 2, 3), new Date(2026, 2, 3), '14:00', '18:00'), now);
    expect(timed).toMatch(/14.00|2.00/);
    expect(timed).toMatch(/18.00|6.00/);
    expect(rangeLabel(toRange(new Date(2024, 2, 3), new Date(2024, 2, 9)), now)).toMatch(/2024/);
    expect(rangeLabel({ since: ms(2026, 2, 3), until: null }, now)).toMatch(/^Since /);
    expect(rangeLabel({ since: null, until: ms(2026, 2, 3, 23, 59, 59, 999) }, now)).toMatch(/^Until /);
  });
});
