// The calendar behind the Mail window's date search: month grids, ranges of days and times, and
// how a range reads in the search bar.

import type { MailDateRange } from '@/lib/types';

/** The time a range starts at, and ends at, when none is chosen: the whole of both days. */
export const DAY_START = '00:00';
export const DAY_END = '23:59';

/** The day a moment falls on, at midnight local time. */
export function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

/** A number that sorts days in calendar order. */
export function dayKey(date: Date): number {
  return date.getFullYear() * 10_000 + date.getMonth() * 100 + date.getDate();
}

export function sameDay(a: Date | null, b: Date | null): boolean {
  return !!a && !!b && dayKey(a) === dayKey(b);
}

/** The 42 days a month view shows: six weeks from the first `weekStart` (0 is Sunday) on or before the 1st. */
export function monthGrid(year: number, month: number, weekStart: number): Date[] {
  const offset = (new Date(year, month, 1).getDay() - weekStart + 7) % 7;
  return Array.from({ length: 42 }, (_, i) => new Date(year, month, 1 - offset + i));
}

/** The day the week starts on in the user's locale (0 is Sunday), Monday when the locale does not say. */
export function firstWeekday(locale = typeof navigator === 'undefined' ? 'en' : navigator.language): number {
  try {
    type WeekInfo = { firstDay: number };
    const loc = new Intl.Locale(locale) as Intl.Locale & { getWeekInfo?: () => WeekInfo; weekInfo?: WeekInfo };
    const info = loc.getWeekInfo?.() ?? loc.weekInfo;
    if (info) return info.firstDay % 7;
  } catch {
    // An unknown locale falls back to Monday.
  }
  return 1;
}

/** One-letter weekday names, in grid order. */
export function weekdayNames(weekStart: number): string[] {
  // 7 January 2024 was a Sunday.
  return Array.from({ length: 7 }, (_, i) =>
    new Date(2024, 0, 7 + ((weekStart + i) % 7)).toLocaleDateString(undefined, { weekday: 'narrow' }),
  );
}

/** Hours and minutes from an `HH:MM` time, or null when it is not one. */
export function parseTime(time: string): [number, number] | null {
  const m = /^(\d{1,2}):(\d{2})/.exec(time);
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  return h < 24 && min < 60 ? [h, min] : null;
}

/** A moment's time as `HH:MM`. */
export function timeOf(date: Date): string {
  return `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

function at(day: Date, time: string, fallback: string, endOfMinute: boolean): number {
  const [h, m] = parseTime(time) ?? parseTime(fallback)!;
  return new Date(
    day.getFullYear(),
    day.getMonth(),
    day.getDate(),
    h,
    m,
    endOfMinute ? 59 : 0,
    endOfMinute ? 999 : 0,
  ).getTime();
}

/**
 * The range from the start of `fromTime` on one day to the end of `toTime`'s minute on the
 * other. The days may come in either order; times that would end before they start swap.
 */
export function toRange(a: Date, b: Date, fromTime = DAY_START, toTime = DAY_END): MailDateRange {
  const [first, last] = dayKey(a) <= dayKey(b) ? [a, b] : [b, a];
  const since = at(first, fromTime, DAY_START, false);
  const until = at(last, toTime, DAY_END, true);
  if (since <= until) return { since, until };
  return { since: at(first, toTime, DAY_START, false), until: at(last, fromTime, DAY_END, true) };
}

/** What a picker shows for a range: its days and times. */
export function fromRange(range: MailDateRange | null): {
  start: Date | null;
  end: Date | null;
  fromTime: string;
  toTime: string;
} {
  const start = range?.since != null ? new Date(range.since) : null;
  const end = range?.until != null ? new Date(range.until) : null;
  return {
    start: start && startOfDay(start),
    end: end && startOfDay(end),
    fromTime: start ? timeOf(start) : DAY_START,
    toTime: end ? timeOf(end) : DAY_END,
  };
}

export interface Preset {
  label: string;
  range: MailDateRange;
}

/** Ranges one click away, each made of whole days up to today. */
export function presets(now = new Date()): Preset[] {
  const today = startOfDay(now);
  const back = (days: number) => new Date(today.getFullYear(), today.getMonth(), today.getDate() - days);
  return [
    { label: 'Today', range: toRange(today, today) },
    { label: 'Yesterday', range: toRange(back(1), back(1)) },
    { label: 'Last 7 days', range: toRange(back(6), today) },
    { label: 'Last 30 days', range: toRange(back(29), today) },
    { label: 'This year', range: toRange(new Date(today.getFullYear(), 0, 1), today) },
  ];
}

function day(date: Date, now: Date): string {
  return date.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    year: date.getFullYear() === now.getFullYear() ? undefined : 'numeric',
  });
}

function time(date: Date): string {
  return date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
}

/** A range the way the search bar names it: "Mar 3 – Mar 9", with times only when not whole days. */
export function rangeLabel(range: MailDateRange, now = new Date()): string {
  const since = range.since != null ? new Date(range.since) : null;
  const until = range.until != null ? new Date(range.until) : null;
  const timed = (since && timeOf(since) !== DAY_START) || (until && timeOf(until) !== DAY_END);
  const moment = (d: Date) => (timed ? `${day(d, now)}, ${time(d)}` : day(d, now));
  if (since && until) {
    if (!sameDay(since, until)) return `${moment(since)} – ${moment(until)}`;
    return timed ? `${day(since, now)}, ${time(since)} – ${time(until)}` : day(since, now);
  }
  if (since) return `Since ${moment(since)}`;
  if (until) return `Until ${moment(until)}`;
  return 'Any date';
}
