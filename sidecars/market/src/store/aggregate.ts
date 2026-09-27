// Builds chart timeframes from native bars (m1/h1/d1, possibly mixed: h1 for one stretch and m1 for
// the next). Intraday timeframes align to UTC multiples of their length. Days, weeks and months
// depend on the week mode:
//   'utc'      plain UTC days, Monday weeks, calendar months (24/7 markets: crypto)
//   'session'  markets closed on Saturday (FX, metals, indices, commodities): Sunday evening opens
//              the new week, so a Sunday belongs to the following Monday. The daily bar labelled
//              Monday spans Sunday 00:00 -> Tuesday 00:00, the week runs Sunday 00:00 -> Sunday
//              00:00 labelled Monday 00:00, and a month holds the session days whose (folded) date
//              falls in it (a month ending on a Sunday hands that Sunday to the next month).
// Every label is the bucket's start as the chart shows it; `bucketSpan` gives the real time span.
//
// API:
//   type WeekMode, interface AggregateOptions { week }
//   bucketOf(t, tf, opts)            label of the timeframe bucket containing t
//   bucketSpan(label, tf, opts)      [start, end) the bucket actually covers
//   aggregate(bars, tf, opts)        bars sorted ascending (any native mix finer than tf) -> tf bars
//   wholeBuckets(covered, tf, opts, now?)   covered time snapped inward to whole buckets; the
//                                    bucket containing now counts when covered from its start to now
//   dropPartial(bars, tf, opts, covered, now?)   aggregated bars whose bucket is fully covered
//   weekModeFor(instrument, sample?) 'utc' when the sample has bars on most Saturdays, 'session'
//                                    when it spans two weekends without; else by name (crypto)
//   isCryptoInstrument(instrument)

import { type Bar } from '../protocol.ts';
import { TIMEFRAMES, type Timeframe } from '../timeframes.ts';
import { instrumentMeta } from './buckets.ts';
import { IntervalSet } from './intervals.ts';

export type WeekMode = 'session' | 'utc';

export interface AggregateOptions {
  week: WeekMode;
}

const DAY = 86400;

/** 0 = Sunday ... 6 = Saturday, for a day number (days since 1970-01-01, a Thursday). */
function weekday(day: number): number {
  return (((day + 4) % 7) + 7) % 7;
}

/** The day number a time trades under: Sunday folds into Monday for session markets. */
function sessionDay(t: number, week: WeekMode): number {
  const d = Math.floor(t / DAY);
  return week === 'session' && weekday(d) === 0 ? d + 1 : d;
}

function monthStart(day: number, offset = 0): number {
  const date = new Date(day * DAY * 1000);
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + offset, 1) / 1000;
}

export function bucketOf(t: number, tf: Timeframe, opts: AggregateOptions): number {
  switch (tf) {
    case '1d':
      return sessionDay(t, opts.week) * DAY;
    case '1w': {
      const d = sessionDay(t, opts.week);
      return (d - ((weekday(d) + 6) % 7)) * DAY;
    }
    case '1M':
      return monthStart(sessionDay(t, opts.week));
    default: {
      const s = TIMEFRAMES[tf].seconds;
      return Math.floor(t / s) * s;
    }
  }
}

export function bucketSpan(label: number, tf: Timeframe, opts: AggregateOptions): [number, number] {
  const session = opts.week === 'session';
  const isMonday = (t: number) => weekday(Math.floor(t / DAY)) === 1;
  switch (tf) {
    case '1d':
      return session && isMonday(label) ? [label - DAY, label + DAY] : [label, label + DAY];
    case '1w':
      return session ? [label - DAY, label + 6 * DAY] : [label, label + 7 * DAY];
    case '1M': {
      const first = monthStart(Math.floor(label / DAY));
      const next = monthStart(Math.floor(label / DAY), 1);
      if (!session) return [first, next];
      return [isMonday(first) ? first - DAY : first, isMonday(next) ? next - DAY : next];
    }
    default:
      return [label, label + TIMEFRAMES[tf].seconds];
  }
}

const roundVolume = (v: number) => Math.round(v * 1e6) / 1e6;

export function aggregate(bars: readonly Bar[], tf: Timeframe, opts: AggregateOptions): Bar[] {
  const out: Bar[] = [];
  let cur: Bar | null = null;
  // Month labels need a Date; reuse the current bucket's span while bars stay inside it.
  let spanFrom = Infinity;
  let spanTo = -Infinity;
  for (const b of bars) {
    if (cur && b.t >= spanFrom && b.t < spanTo) {
      if (b.h > cur.h) cur.h = b.h;
      if (b.l < cur.l) cur.l = b.l;
      cur.c = b.c;
      cur.v += b.v;
      continue;
    }
    if (cur) {
      cur.v = roundVolume(cur.v);
      out.push(cur);
    }
    const label = bucketOf(b.t, tf, opts);
    [spanFrom, spanTo] = bucketSpan(label, tf, opts);
    cur = { t: label, o: b.o, h: b.h, l: b.l, c: b.c, v: b.v };
  }
  if (cur) {
    cur.v = roundVolume(cur.v);
    out.push(cur);
  }
  return out;
}

export function wholeBuckets(covered: IntervalSet, tf: Timeframe, opts: AggregateOptions, now?: number): IntervalSet {
  const out = new IntervalSet();
  const forming = now === undefined ? null : bucketSpan(bucketOf(now, tf, opts), tf, opts);
  for (const [f, rawTo] of covered) {
    const t = now === undefined ? rawTo : Math.min(rawTo, now);
    if (!(t > f)) continue;
    const [s0, e0] = bucketSpan(bucketOf(f, tf, opts), tf, opts);
    const start = s0 === f ? f : e0;
    const [s1] = bucketSpan(bucketOf(t, tf, opts), tf, opts);
    let end = s1 === t ? t : s1;
    // The forming bucket is complete as far as it goes once it is covered from its start to now.
    if (forming && now !== undefined && t === now && f <= forming[0] && forming[0] < t) end = t;
    if (end > start) out.add(start, end);
  }
  return out;
}

export function dropPartial(
  bars: readonly Bar[],
  tf: Timeframe,
  opts: AggregateOptions,
  covered: IntervalSet,
  now?: number,
): Bar[] {
  return bars.filter((b) => {
    const [s, e] = bucketSpan(b.t, tf, opts);
    const end = now !== undefined && now >= s && now < e ? now : e;
    return covered.covers(s, end);
  });
}

/** Crypto bases Dukascopy lists (BTC/USD, ETH/CHF, ...), plus common ones it may add. */
const CRYPTO_BASES = new Set(
  'ADA AVE BAT BCH BTC CMP DSH ETH EUC LNK LTC TRX UNI USC XLM YFI AVAX DOGE DOT EOS LINK MATIC SOL XMR XRP'.split(' '),
);

export function isCryptoInstrument(instrument: string): boolean {
  const name = instrumentMeta(instrument)?.name ?? '';
  const base = /^([A-Z0-9]+)\/[A-Z]+$/.exec(name)?.[1];
  if (base) return CRYPTO_BASES.has(base);
  // Unknown to the metadata: judge by the key (btcusd, ethchf).
  const key = instrument.toUpperCase();
  return [...CRYPTO_BASES].some((b) => key.startsWith(b) && /^[A-Z]{3}$/.test(key.slice(b.length)));
}

export function weekModeFor(instrument: string, sample?: readonly Bar[] | null): WeekMode {
  if (sample && sample.length > 0) {
    const firstDay = Math.floor(sample[0]!.t / DAY);
    const lastDay = Math.floor(sample[sample.length - 1]!.t / DAY);
    let saturdays = 0;
    for (let d = firstDay; d <= lastDay; d += 1) if (weekday(d) === 6) saturdays += 1;
    if (saturdays >= 2) {
      const traded = new Set<number>();
      for (const b of sample) {
        const d = Math.floor(b.t / DAY);
        if (weekday(d) === 6) traded.add(d);
      }
      // A stray Saturday print does not make a market 24/7; bars on most Saturdays do.
      return traded.size * 2 >= saturdays ? 'utc' : 'session';
    }
  }
  return isCryptoInstrument(instrument) ? 'utc' : 'session';
}
