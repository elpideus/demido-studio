// The Data tab's timeline geometry: interval arithmetic, segments, simplification and the axis.

import { describe, expect, it } from 'vitest';

import type { MarketCoverageItem, MarketJob, MarketPlan, MarketTierCoverage } from '@/lib/types';
import {
  axisTicks,
  chartSymbol,
  coveredShare,
  jobsFor,
  leadJob,
  marketDomain,
  normalize,
  planIdle,
  rangeText,
  segmentAt,
  segmentsOf,
  shareText,
  simplify,
  sortSources,
  sortTiers,
  storedExtent,
  subtract,
  tierGaps,
  tierSpan,
  unfinishedJobs,
  type Segment,
} from './coverage';

const DAY = 86_400;
const utc = (y: number, m = 1, d = 1) => Date.UTC(y, m - 1, d) / 1000;

const tier = (over: Partial<MarketTierCoverage> = {}): MarketTierCoverage => ({
  tier: 'm1',
  intervals: [],
  available: null,
  bytes: 0,
  ...over,
});

const item = (over: Partial<MarketCoverageItem> = {}): MarketCoverageItem => ({
  market: 'eurusd',
  name: 'EUR/USD',
  symbols: [],
  sources: [],
  jobs: [],
  ...over,
});

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
  origin: 'data',
  createdAt: 1,
  updatedAt: 1,
  ...over,
});

const plan = (over: Partial<MarketPlan> = {}): MarketPlan => ({
  source: 'dukascopy',
  key: 'eurusd',
  name: 'EUR/USD',
  from: 0,
  to: 100,
  tiers: ['m1', 'h1', 'd1'],
  requests: 0,
  bytes: 0,
  seconds: 0,
  queuedAhead: 0,
  complete: false,
  approximate: false,
  perTier: [],
  job: null,
  ...over,
});

describe('interval arithmetic', () => {
  it('merges overlapping and touching ranges and drops empty ones', () => {
    expect(
      normalize([
        [5, 7],
        [0, 2],
        [2, 3],
        [6, 9],
        [4, 4],
      ]),
    ).toEqual([
      [0, 3],
      [5, 9],
    ]);
    expect(normalize(undefined)).toEqual([]);
  });

  it('subtracts ranges that cut, split and swallow', () => {
    expect(
      subtract(
        [
          [0, 10],
          [20, 30],
        ],
        [
          [2, 4],
          [8, 22],
          [25, 26],
        ],
      ),
    ).toEqual([
      [0, 2],
      [4, 8],
      [22, 25],
      [26, 30],
    ]);
    expect(subtract([[0, 10]], [[0, 10]])).toEqual([]);
    expect(subtract([[0, 10]], [])).toEqual([[0, 10]]);
  });
});

describe('segments', () => {
  it('tiles the span with data, empty and gap, whether or not intervals include the empties', () => {
    const expected: Segment[] = [
      { kind: 'gap', from: 0, to: 10 },
      { kind: 'data', from: 10, to: 20 },
      { kind: 'empty', from: 20, to: 25 },
      { kind: 'data', from: 25, to: 30 },
      { kind: 'gap', from: 30, to: 40 },
    ];
    const withEmpties = tier({ intervals: [[10, 30]], empty: [[20, 25]], available: [0, 40] });
    const withoutEmpties = tier({
      intervals: [
        [10, 20],
        [25, 30],
      ],
      empty: [[20, 25]],
      available: [0, 40],
    });
    expect(segmentsOf(withEmpties, [0, 40])).toEqual(expected);
    expect(segmentsOf(withoutEmpties, [0, 40])).toEqual(expected);
  });

  it('folds weekly weekday/weekend stripes into one stored run but keeps a real hole', () => {
    // Four weeks of five stored days and an empty weekend, a missing week, then two more weeks.
    const intervals: Array<[number, number]> = [];
    const empty: Array<[number, number]> = [];
    const week = (w: number) => {
      intervals.push([w * 7 * DAY, (w * 7 + 5) * DAY]);
      empty.push([(w * 7 + 5) * DAY, (w + 1) * 7 * DAY]);
    };
    [0, 1, 2, 3, 5, 6].forEach(week);
    const t = tier({ intervals, empty, available: [0, 7 * 7 * DAY] });
    const raw = segmentsOf(t, [0, 7 * 7 * DAY]);
    expect(raw.length).toBeGreaterThan(10);
    const simple = simplify(raw, 10 * DAY);
    expect(simple.map((s) => s.kind)).toEqual(['data', 'gap', 'data']);
    expect(simple[0]!.from).toBe(0);
    expect(simple[simple.length - 1]!.to).toBe(7 * 7 * DAY);
    // Precise boundaries survive: the hole starts where the fourth week's weekend ends.
    expect(simple[1]!.from).toBe(28 * DAY);
    expect(simple[1]!.to).toBe(35 * DAY);
  });

  it('keeps long segments exactly and finds the segment under a time', () => {
    const segs: Segment[] = [
      { kind: 'data', from: 0, to: 100 },
      { kind: 'empty', from: 100, to: 200 },
    ];
    expect(simplify(segs, 10)).toEqual(segs);
    expect(segmentAt(segs, 150)?.kind).toBe('empty');
    expect(segmentAt(segs, 200)).toBeNull();
  });

  it('reports the covered share, counting known-empty time as covered', () => {
    const t = tier({ intervals: [[0, 50]], empty: [[50, 75]], available: [0, 100] });
    expect(coveredShare(t, [0, 100])).toBeCloseTo(0.75);
    expect(shareText(0.75)).toBe('75%');
    expect(shareText(0.999)).toBe('99%');
    expect(shareText(1)).toBe('100%');
    expect(shareText(0.001)).toBe('<1%');
    expect(shareText(0)).toBe('0%');
  });
});

describe('domain', () => {
  it('spans the union of every tier, so daily data from 1973 widens the minute timeline axis', () => {
    const m = item({
      sources: [
        {
          source: 'dukascopy',
          key: 'eurusd',
          bytes: 10,
          tiers: [
            tier({ tier: 'm1', available: [utc(2003, 5), utc(2026, 9)] }),
            tier({ tier: 'd1', available: [utc(1973, 1), utc(2026, 9)] }),
          ],
        },
        {
          source: 'tradingview',
          key: 'FX:EURUSD',
          bytes: 5,
          tiers: [tier({ tier: '1h', intervals: [[utc(2020), utc(2026, 10)]] })],
        },
      ],
    });
    expect(marketDomain(m)).toEqual([utc(1973, 1), utc(2026, 10)]);
  });

  it('widens a tier span to data held outside its available range and falls back to held data', () => {
    expect(tierSpan(tier({ intervals: [[5, 20]], available: [10, 30] }))).toEqual([5, 30]);
    expect(tierSpan(tier({ intervals: [[5, 20]], empty: [[20, 40]] }))).toEqual([5, 40]);
    expect(tierSpan(tier())).toBeNull();
  });

  it('finds the stored extent from data only, not from empties', () => {
    const m = item({
      sources: [
        {
          source: 'dukascopy',
          key: 'eurusd',
          bytes: 1,
          tiers: [tier({ intervals: [[0, 100]], empty: [[0, 10]] }), tier({ tier: 'h1', intervals: [[50, 200]] })],
        },
      ],
    });
    expect(storedExtent(m)).toEqual([10, 200]);
    expect(storedExtent(item())).toBeNull();
  });
});

describe('axis ticks', () => {
  it('uses years across decades, within the maximum', () => {
    const ticks = axisTicks(utc(1973, 3), utc(2026, 9), 7);
    expect(ticks.length).toBeLessThanOrEqual(7);
    expect(ticks.length).toBeGreaterThan(2);
    for (const tick of ticks) {
      expect(new Date(tick.t * 1000).getUTCMonth()).toBe(0);
      expect(new Date(tick.t * 1000).getUTCFullYear() % 10).toBe(0);
      expect(tick.label).toBe(String(new Date(tick.t * 1000).getUTCFullYear()));
    }
  });

  it('uses month starts for about a year and days for a few weeks', () => {
    const months = axisTicks(utc(2024, 2, 15), utc(2025, 1, 10), 7);
    expect(months.length).toBeGreaterThan(1);
    for (const tick of months) expect(new Date(tick.t * 1000).getUTCDate()).toBe(1);
    const days = axisTicks(utc(2024, 3, 1), utc(2024, 3, 20), 7);
    expect(days.length).toBeGreaterThan(1);
    expect(days.length).toBeLessThanOrEqual(7);
    for (const tick of days) expect(tick.t % DAY).toBe(0);
  });

  it('keeps ticks strictly inside the domain', () => {
    const ticks = axisTicks(utc(2000), utc(2010), 10);
    expect(ticks.every((t) => t.t > utc(2000) && t.t < utc(2010))).toBe(true);
    expect(axisTicks(10, 10)).toEqual([]);
  });
});

describe('ordering and naming', () => {
  it('sorts tiers finest first and Dukascopy before TradingView', () => {
    expect(sortTiers('dukascopy', [{ tier: 'd1' }, { tier: 'm1' }, { tier: 'h1' }]).map((t) => t.tier)).toEqual([
      'm1',
      'h1',
      'd1',
    ]);
    expect(
      sortTiers('tradingview', [{ tier: '1d' }, { tier: '15m' }, { tier: '1w' }, { tier: '4h' }, { tier: '1m' }]).map(
        (t) => t.tier,
      ),
    ).toEqual(['1m', '15m', '4h', '1d', '1w']);
    expect(sortSources([{ source: 'tradingview' }, { source: 'dukascopy' }]).map((s) => s.source)).toEqual([
      'dukascopy',
      'tradingview',
    ]);
  });

  it('opens the chart on the first TradingView symbol, else the Dukascopy key', () => {
    const duka = { source: 'dukascopy' as const, key: 'eurusd', bytes: 0, tiers: [] };
    expect(chartSymbol(item({ symbols: ['FX:EURUSD', 'OANDA:EURUSD'], sources: [duka] }))).toBe('FX:EURUSD');
    expect(chartSymbol(item({ market: 'x', sources: [duka] }))).toBe('eurusd');
    expect(chartSymbol(item({ market: 'NASDAQ:AAPL' }))).toBe('NASDAQ:AAPL');
  });

  it('formats an exclusive end as the last day', () => {
    expect(rangeText(utc(2024, 3, 1), utc(2024, 4, 1))).toContain('31');
    expect(rangeText(utc(2024, 3, 1), utc(2024, 3, 2))).not.toContain('–');
  });
});

describe('jobs', () => {
  it('merges listed and summary jobs, keeps the newer copy and drops finished ones', () => {
    const a = job({ id: 'a', createdAt: 1, updatedAt: 5, done: 3 });
    const aOld = job({ id: 'a', createdAt: 1, updatedAt: 2, done: 1 });
    const b = job({ id: 'b', createdAt: 2, status: 'done' });
    const c = job({ id: 'c', createdAt: 3, status: 'paused', key: 'gbpchf', symbol: 'FX:GBPCHF' });
    const jobs = unfinishedJobs([item({ jobs: [aOld, b] })], [a, c]);
    expect(jobs.map((j) => j.id)).toEqual(['c', 'a']);
    expect(jobs[1]!.done).toBe(3);
    expect(unfinishedJobs([item({ jobs: [aOld] })], null).map((j) => j.id)).toEqual(['a']);
  });

  it('matches jobs to a market by key or symbol and prefers a moving one', () => {
    const m = item({ symbols: ['FX:EURUSD'], sources: [{ source: 'dukascopy', key: 'eurusd', bytes: 0, tiers: [] }] });
    const paused = job({ id: 'p', status: 'paused' });
    const running = job({ id: 'r', status: 'running', key: 'other', symbol: 'FX:EURUSD' });
    const foreign = job({ id: 'f', key: 'gbpchf', symbol: 'FX:GBPCHF' });
    const mine = jobsFor(m, [paused, running, foreign]);
    expect(mine.map((j) => j.id)).toEqual(['p', 'r']);
    expect(leadJob(mine)?.id).toBe('r');
    expect(leadJob([])).toBeNull();
  });
});

describe('plan without work of its own', () => {
  it('says whether starting would still resume something', () => {
    expect(planIdle(plan({ requests: 40 }))).toBeNull();
    expect(planIdle(plan({ requests: 40, job: { id: 'a', status: 'paused' } }))).toBeNull();
    expect(planIdle(plan({ complete: true }))).toBe('complete');
    expect(planIdle(plan({ queuedAhead: 12, job: { id: 'a', status: 'paused' } }))).toBe('stopped');
    expect(planIdle(plan({ job: { id: 'a', status: 'error' } }))).toBe('stopped');
    expect(planIdle(plan({ queuedAhead: 12, job: { id: 'a', status: 'running' } }))).toBe('busy');
    expect(planIdle(plan({ queuedAhead: 12 }))).toBe('busy');
    expect(planIdle(plan())).toBe('nothing');
  });
});

describe('tier gaps', () => {
  it('lists the available time that is neither stored nor known empty', () => {
    const t = tier({
      available: [utc(2003), utc(2026)],
      intervals: [
        [utc(2003), utc(2010)],
        [utc(2012), utc(2026)],
      ],
      empty: [[utc(2010), utc(2011)]],
    });
    expect(tierGaps(t)).toEqual([[utc(2011), utc(2012)]]);
    expect(tierGaps(tier({ available: [utc(2003), utc(2004)] }))).toEqual([[utc(2003), utc(2004)]]);
  });

  it('reads a stretch not downloaded as one gap across the empty weekends inside it', () => {
    // Weekends inferred empty inside two missing weeks; one stored Monday splits them.
    const t = tier({
      available: [utc(2024, 1, 1), utc(2024, 2, 1)],
      intervals: [
        [utc(2024, 1, 1), utc(2024, 1, 8)],
        [utc(2024, 1, 20), utc(2024, 1, 23)],
        [utc(2024, 1, 29), utc(2024, 2, 1)],
      ],
      empty: [
        [utc(2024, 1, 13), utc(2024, 1, 15)],
        [utc(2024, 1, 20), utc(2024, 1, 22)],
      ],
    });
    expect(tierGaps(t)).toEqual([
      [utc(2024, 1, 8), utc(2024, 1, 20)],
      [utc(2024, 1, 23), utc(2024, 1, 29)],
    ]);
    // A long stretch the source has nothing for does split the gap: that time is fully known.
    const quiet = tier({
      available: [utc(2024, 1, 1), utc(2024, 3, 1)],
      intervals: [[utc(2024, 1, 10), utc(2024, 2, 10)]],
      empty: [[utc(2024, 1, 10), utc(2024, 2, 10)]],
    });
    expect(tierGaps(quiet)).toEqual([
      [utc(2024, 1, 1), utc(2024, 1, 10)],
      [utc(2024, 2, 10), utc(2024, 3, 1)],
    ]);
  });

  it('leaves out the newest hours, which the next read fetches anyway', () => {
    const now = utc(2026, 9, 27) + 5 * 3600;
    const recent = tier({ available: [utc(2025), now], intervals: [[utc(2025), utc(2026, 9, 27)]] });
    expect(tierGaps(recent)).toEqual([]);
    const behind = tier({ available: [utc(2025), now], intervals: [[utc(2025), utc(2026, 9, 20)]] });
    expect(tierGaps(behind)).toEqual([[utc(2026, 9, 20), now]]);
  });

  it('has nothing to say without an available range', () => {
    expect(tierGaps(tier({ intervals: [[utc(2003), utc(2004)]] }))).toEqual([]);
  });
});
