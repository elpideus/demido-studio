// The chart's bookkeeping: bar sources from spans, which downloads belong to a chart, and the
// ranges the download popup offers.

import { describe, expect, it } from 'vitest';

import type { Bar, MarketJob, MarketSpan } from '@/lib/types';
import {
  chartJobs,
  coveringJob,
  formatDay,
  historySource,
  isBehind,
  isChartJob,
  keysMatch,
  latestView,
  mergeListed,
  mergeSpans,
  optionRange,
  parseDay,
  pickJob,
  shiftMonths,
  sourceAt,
  sourcesOf,
  spanOf,
  upsertJob,
} from './chartData';

const bar = (t: number): Bar => ({ t, o: 1, h: 1, l: 1, c: 1, v: 1 });
const utc = (iso: string) => Math.floor(Date.parse(iso) / 1000);

const job = (over: Partial<MarketJob> = {}): MarketJob => ({
  id: 'j1',
  source: 'dukascopy',
  key: 'eurusd',
  symbol: 'FX:EURUSD',
  name: 'EUR/USD',
  from: 0,
  to: 100,
  tiers: ['m1', 'h1', 'd1'],
  status: 'running',
  total: 10,
  done: 1,
  bytes: 0,
  etaSeconds: null,
  rate: 0,
  inFlight: 0,
  perTier: [],
  origin: 'chart',
  createdAt: 1,
  updatedAt: 1,
  ...over,
});

describe('spans', () => {
  const duka: MarketSpan = { source: 'dukascopy', from: 100, to: 400, count: 4 };
  const tv: MarketSpan = { source: 'tradingview', from: 500, to: 800, count: 4 };

  it('describes a stream as one TradingView span', () => {
    expect(spanOf([bar(10), bar(20), bar(30)], 'tradingview')).toEqual([
      { source: 'tradingview', from: 10, to: 30, count: 3 },
    ]);
    expect(spanOf([], 'tradingview')).toEqual([]);
  });

  it('joins an older page onto the run it continues', () => {
    const older: MarketSpan = { source: 'dukascopy', from: 0, to: 90, count: 2 };
    expect(mergeSpans([duka, tv], [older])).toEqual([{ source: 'dukascopy', from: 0, to: 400, count: 6 }, tv]);
  });

  it('keeps runs from different sources apart', () => {
    expect(mergeSpans([tv], [duka])).toEqual([duka, tv]);
  });

  it('finds the source of a bar, including live bars newer than every span', () => {
    const spans = [duka, tv];
    expect(sourceAt(spans, 100)).toBe('dukascopy');
    expect(sourceAt(spans, 400)).toBe('dukascopy');
    expect(sourceAt(spans, 500)).toBe('tradingview');
    expect(sourceAt(spans, 9_000)).toBe('tradingview');
    // Before the first span (should not happen): the oldest run's source rather than nothing.
    expect(sourceAt(spans, 50)).toBe('dukascopy');
    expect(sourceAt([], 50)).toBeNull();
  });

  it('labels every bar in one pass the same way sourceAt does', () => {
    const bars = [100, 200, 400, 500, 800, 860].map(bar);
    const spans = [duka, tv];
    expect(sourcesOf(bars, spans)).toEqual(bars.map((b) => sourceAt(spans, b.t)));
    expect(sourcesOf(bars, spans)).toEqual([
      'dukascopy',
      'dukascopy',
      'dukascopy',
      'tradingview',
      'tradingview',
      'tradingview',
    ]);
  });
});

describe('chart keys and jobs', () => {
  const keys = { dukascopy: 'eurusd', tradingview: 'FX:EURUSD' };

  it('matches store keys per source, ignoring case', () => {
    expect(keysMatch(keys, 'dukascopy', 'EURUSD')).toBe(true);
    expect(keysMatch(keys, 'tradingview', 'fx:eurusd')).toBe(true);
    expect(keysMatch(keys, 'dukascopy', 'FX:EURUSD')).toBe(false);
    expect(keysMatch({}, 'dukascopy', 'eurusd')).toBe(false);
  });

  it('claims a job by key, or by symbol before the keys are known', () => {
    expect(isChartJob(job({ symbol: 'EURUSD' }), keys, 'FX:EURUSD')).toBe(true);
    expect(isChartJob(job({ key: 'gbpusd', symbol: 'FX:GBPUSD' }), keys, 'FX:EURUSD')).toBe(false);
    expect(isChartJob(job(), {}, 'FX:EURUSD')).toBe(true);
    expect(isChartJob(job(), {}, 'OANDA:XAUUSD')).toBe(false);
  });

  it('follows a moving job before a paused one before a failed one', () => {
    const paused = job({ id: 'p', status: 'paused', updatedAt: 9 });
    const failed = job({ id: 'e', status: 'error', updatedAt: 10 });
    const waiting = job({ id: 'w', status: 'waiting', updatedAt: 2 });
    const running = job({ id: 'r', status: 'running', updatedAt: 3 });
    expect(pickJob([failed, paused, waiting, running])?.id).toBe('r');
    expect(pickJob([failed, paused])?.id).toBe('p');
    expect(pickJob([failed])?.id).toBe('e');
    expect(pickJob([job({ status: 'done' })])).toBeNull();
    expect(pickJob([])).toBeNull();
  });

  it('adds, replaces and drops finished jobs, never going back to an older copy', () => {
    const a = job({ id: 'a', updatedAt: 5, done: 3 });
    expect(upsertJob([], a)).toEqual([a]);
    expect(upsertJob([a], job({ id: 'a', updatedAt: 6, done: 4 }))[0]?.done).toBe(4);
    expect(upsertJob([a], job({ id: 'a', updatedAt: 4, done: 1 }))).toEqual([a]);
    expect(upsertJob([a], job({ id: 'a', updatedAt: 7, status: 'done' }))).toEqual([]);
    expect(upsertJob([a], job({ id: 'b', status: 'done' }))).toEqual([a]);
  });

  it('takes a fresh listing as the truth, except for copies newer than it', () => {
    const held = [job({ id: 'a', updatedAt: 9, done: 7 }), job({ id: 'gone', updatedAt: 9 })];
    const listed = [job({ id: 'a', updatedAt: 5, done: 3 }), job({ id: 'b', updatedAt: 5 })];
    expect(mergeListed(held, listed).map((j) => [j.id, j.done])).toEqual([
      ['a', 7],
      ['b', 1],
    ]);
    expect(mergeListed(held, [job({ id: 'a', updatedAt: 10, status: 'done' })])).toEqual([]);
  });

  it('takes older history from Dukascopy whenever the symbol has a Dukascopy key', () => {
    expect(historySource(keys, null)).toBe('dukascopy');
    expect(historySource({ tradingview: 'NASDAQ:AAPL' }, null)).toBe('tradingview');
    expect(historySource(keys, { from: 1, to: 2, source: 'tradingview' })).toBe('tradingview');
  });
});

describe('downloads in the toolbar and popup', () => {
  const hour = 3600;
  // The migrated real cache: a paused GBP/CHF job over 2003-08-03..2025-07-27, while 1m paging stops
  // at 2025-09-25 with the gap reaching back past the job's end.
  const gbpchf = { key: 'gbpchf', symbol: 'SAXO:GBPCHF', name: 'GBP/CHF' };
  const paused = job({
    ...gbpchf,
    id: 'old',
    status: 'paused',
    from: utc('2003-08-03T00:00:00Z'),
    to: utc('2025-07-27T00:00:00Z'),
  });
  const gap = { from: utc('2003-08-03T00:00:00Z'), to: utc('2025-09-25T00:00:00Z'), source: 'dukascopy' as const };
  const strip = { from: utc('2025-07-27T00:00:00Z'), to: utc('2025-09-25T00:00:00Z'), source: 'dukascopy' as const };

  it('keeps the picker and the Download button while the only download is paused', () => {
    const view = chartJobs([paused], gap, '1m', 60);
    expect(view).toEqual({ lead: paused, moving: false, covering: null, other: paused });
    // Even a paused job that does contain the gap fills nothing until resumed.
    expect(chartJobs([paused], strip, '1m', 60).covering).toBeNull();
    const failed = job({ ...gbpchf, id: 'e', status: 'error', from: 0, to: gap.to + 1 });
    expect(chartJobs([failed], gap, '1m', 60)).toMatchObject({ moving: false, covering: null, other: failed });
  });

  it('lets a moving download replace the picker only when it already fetches the whole gap', () => {
    const everything = job({ ...gbpchf, id: 'all', status: 'running', from: gap.from, to: gap.to + hour });
    expect(chartJobs([everything], gap, '1m', 60)).toEqual({
      lead: everything,
      moving: true,
      covering: everything,
      other: null,
    });
    // Over another range: it moves (the toolbar shows it) but the picker stays, with the job above it.
    const running = { ...paused, id: 'r', status: 'running' as const };
    expect(chartJobs([running], gap, '1m', 60)).toEqual({
      lead: running,
      moving: true,
      covering: null,
      other: running,
    });
    expect(coveringJob([running], null, '1m', 60)).toBeNull();
  });

  it('counts only the detail that serves the timeframe', () => {
    const hourly = job({ id: 'h', status: 'running', tiers: ['h1'], from: gap.from, to: gap.to });
    expect(coveringJob([hourly], gap, '1m', 60)).toBeNull();
    expect(coveringJob([hourly], gap, '4h', 4 * hour)?.id).toBe('h');
    const daily = job({ id: 'd', status: 'queued', tiers: ['d1'], from: gap.from, to: gap.to });
    expect(coveringJob([daily], gap, '1h', hour)).toBeNull();
    expect(coveringJob([daily], gap, '1d', 86_400)?.id).toBe('d');
    const tv = job({ id: 't', source: 'tradingview', status: 'running', tiers: ['1h'], from: 0, to: gap.to });
    expect(coveringJob([tv], gap, '1h', hour)).toBeNull();
    expect(coveringJob([tv], { ...gap, source: 'tradingview' }, '1h', hour)?.id).toBe('t');
    expect(coveringJob([tv], { ...gap, source: 'tradingview' }, '1d', 86_400)).toBeNull();
  });
});

describe('latestView', () => {
  const keys = { dukascopy: 'eurusd' };

  it('shows an empty Dukascopy read as history with the download popup, not an error', () => {
    // Sunday morning at 1m: the read reached only the weekend's empty days.
    const weekend = { bars: [], more: 'gap' as const, keys };
    expect(latestView(weekend)).toEqual({ mode: 'history', offer: true });
    expect(latestView({ ...weekend, more: 'cached' })).toEqual({ mode: 'history', offer: false });
    expect(latestView({ bars: [bar(1)], more: 'gap', keys })).toEqual({ mode: 'history', offer: false });
  });

  it('says Dukascopy could not be reached only when a fetch failed', () => {
    const view = latestView({ bars: [], more: 'gap', keys, fetchError: 'Offline' });
    expect(view).toMatchObject({ mode: 'error' });
    expect(view.mode === 'error' && view.message).toMatch(/could not be reached: Offline/);
    expect(latestView({ bars: [], more: 'none', keys })).toEqual({
      mode: 'error',
      message: 'Dukascopy has no history for this market.',
    });
  });

  it('asks to sign in for a market only TradingView has', () => {
    expect(latestView({ bars: [], more: 'gap', keys: { tradingview: 'NASDAQ:AAPL' } })).toEqual({ mode: 'signin' });
    expect(latestView({ bars: [], more: 'none', keys, stale: true })).toEqual({ mode: 'signin' });
  });
});

describe('popup ranges', () => {
  const oldest = utc('2024-03-31T00:00:00Z');
  const now = utc('2026-09-26T12:00:00Z');
  const ctx = { oldest, gap: null, fromDate: '', now };

  it('moves by calendar months in UTC, clamping to the month end', () => {
    expect(shiftMonths(oldest, -1)).toBe(utc('2024-02-29T00:00:00Z'));
    expect(shiftMonths(oldest, -12)).toBe(utc('2023-03-31T00:00:00Z'));
    expect(shiftMonths(utc('2024-01-15T06:30:00Z'), -1)).toBe(utc('2023-12-15T06:30:00Z'));
  });

  it('ends month, year and from-date at the oldest bar; Everything has no bounds', () => {
    expect(optionRange('month', ctx)).toEqual({ from: utc('2024-02-29T00:00:00Z'), to: oldest });
    expect(optionRange('year', ctx)).toEqual({ from: utc('2023-03-31T00:00:00Z'), to: oldest });
    expect(optionRange('all', ctx)).toEqual({});
    expect(optionRange('from', { ...ctx, fromDate: '2020-01-01' })).toEqual({
      from: utc('2020-01-01T00:00:00Z'),
      to: oldest,
    });
  });

  it('refuses a from-date that is missing or not before the oldest bar', () => {
    expect(optionRange('from', ctx)).toBeNull();
    expect(optionRange('from', { ...ctx, fromDate: '2024-04-02' })).toBeNull();
    expect(optionRange('from', { ...ctx, fromDate: 'soon' })).toBeNull();
  });

  it('fills exactly the gap when the store knows it', () => {
    const gap = { from: utc('2019-03-12T00:00:00Z'), to: oldest, source: 'dukascopy' as const };
    expect(optionRange('gap', { ...ctx, gap })).toEqual({ from: gap.from, to: gap.to });
    expect(optionRange('gap', ctx)).toBeNull();
  });

  it('counts back from now when the chart has no bars', () => {
    expect(optionRange('month', { ...ctx, oldest: null })).toEqual({ from: shiftMonths(now, -1), to: now });
  });

  it('reads and writes UTC days', () => {
    expect(parseDay('2024-03-05')).toBe(utc('2024-03-05T00:00:00Z'));
    expect(parseDay('2024-3-5')).toBeNull();
    expect(formatDay(utc('2024-03-05T23:59:00Z'))).toBe('2024-03-05');
  });
});

describe('isBehind', () => {
  const hour = 3600;
  const day = 86_400;

  it('lets a weekend pass without offering an update', () => {
    // Friday 20:59 UTC close, looked at on Sunday 20:00 UTC.
    expect(isBehind(utc('2026-09-25T20:59:00Z'), 60, utc('2026-09-27T20:00:00Z'))).toBe(false);
  });

  it('offers an update once the gap is longer than any closure', () => {
    expect(isBehind(utc('2026-09-20T00:00:00Z'), hour, utc('2026-09-26T00:00:00Z'))).toBe(true);
  });

  it('allows a whole bar on top for long timeframes', () => {
    const monday = utc('2026-09-21T00:00:00Z');
    expect(isBehind(monday, 7 * day, monday + 9 * day)).toBe(false);
    expect(isBehind(monday, 7 * day, monday + 11 * day)).toBe(true);
  });
});
