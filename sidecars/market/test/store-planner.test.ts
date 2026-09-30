// Download plans and interactive needs over a real store in a temp dir (no network).

import assert from 'node:assert/strict';
import { afterEach, test } from 'node:test';

import { nextBucket } from '../src/store/buckets.ts';
import { DukascopyStore } from '../src/store/dukascopy-store.ts';
import { type JobView, Planner, backFrom } from '../src/store/planner.ts';
import { Series } from '../src/store/series.ts';
import { TvStore } from '../src/store/tv-store.ts';
import { DAY, FakeFetcher, bucketBars, bucketBody, cleanup, putBucket, s, tempDir } from './store-helpers.ts';

afterEach(cleanup);

function harness(now: number, jobs: JobView[] = []) {
  const cache = tempDir();
  const clock = () => now * 1000;
  const store = new DukascopyStore(cache, { now: clock, manifestDelayMs: 20 });
  const tv = new TvStore(cache, { now: clock, flushDelayMs: 20 });
  const series = new Series({ store, tv, now: clock });
  const fetcher = new FakeFetcher(store, () => [], clock);
  const planner = new Planner({ store, fetcher, tv, series, jobs: () => jobs, now: clock });
  return { store, tv, series, planner, jobs };
}

const NOW = s('2024-06-01T12:00:00Z');
const job = (over: Partial<JobView>): JobView => ({
  id: 'j1',
  status: 'running',
  source: 'dukascopy',
  key: 'eurusd',
  from: 0,
  to: 0,
  openEnd: false,
  tiers: ['m1', 'h1', 'd1'],
  remaining: 0,
  ...over,
});

test('final, provisional (not yet due), unavailable and queued buckets are never planned', async () => {
  const jobs = [
    job({ from: s('2024-03-11T00:00:00Z'), to: s('2024-03-13T00:00:00Z'), remaining: 2 }),
    // A paused job fetches nothing: its buckets are this plan's to fetch.
    job({ id: 'j2', status: 'paused', from: s('2024-03-13T00:00:00Z'), to: s('2024-03-15T00:00:00Z') }),
  ];
  const { store, planner } = harness(NOW, jobs);
  await putBucket(store, 'eurusd', 'm1', s('2024-03-04T00:00:00Z'), bucketBars('m1', s('2024-03-04T00:00:00Z')));
  await putBucket(store, 'eurusd', 'm1', s('2024-03-05T00:00:00Z'), bucketBars('m1', s('2024-03-05T00:00:00Z')));
  // Built a minute after it closed, a day ago: its re-fetch is not due yet.
  await store.put('eurusd', 'm1', s('2024-03-06T00:00:00Z'), null, { builtAt: NOW - DAY, final: false });
  store.markUnavailable('eurusd', 'm1', s('2024-03-07T00:00:00Z'));
  const plan = await planner.plan({
    symbol: 'FX:EURUSD',
    from: s('2024-03-01T00:00:00Z'),
    to: s('2024-03-16T00:00:00Z'),
    tiers: ['m1'],
  });
  assert.equal(plan.source, 'dukascopy');
  assert.equal(plan.key, 'eurusd');
  assert.equal(plan.name, 'EUR/USD');
  assert.deepEqual(plan.tiers, ['m1']);
  assert.equal(plan.requests, 15 - 4 - 2);
  assert.equal(plan.queuedAhead, 2);
  assert.equal(plan.complete, false);
  assert.equal(plan.approximate, false);
  assert.deepEqual(plan.perTier, [
    { tier: 'm1', from: s('2024-03-01T00:00:00Z'), to: s('2024-03-16T00:00:00Z'), requests: 9 },
  ]);
  assert.equal(plan.job, null);
  // Seconds: own requests plus the backlog ahead, at the fetcher's rate (3/s).
  assert.equal(plan.seconds, Math.ceil((9 + 2) / 3));
  assert.ok(plan.bytes > 0);
});

test('a Dukascopy plan is 1-minute days, whatever detail it asks for', async () => {
  const { store, planner } = harness(NOW);
  const march = s('2024-03-01T00:00:00Z');
  const april = nextBucket('h1', march);
  for (const tiers of [undefined, ['m1'], ['h1'], ['d1'], ['h1', 'd1']]) {
    const plan = await planner.plan({ symbol: 'EURUSD', from: march, to: april, tiers });
    assert.deepEqual(plan.tiers, ['m1'], `tiers ${tiers}`);
    // Every day, weekends included: nothing is inferred from coarser data.
    assert.equal(plan.requests, 31, `tiers ${tiers}`);
    assert.deepEqual(
      plan.perTier.map((t) => [t.tier, t.requests]),
      [['m1', 31]],
    );
  }
  // Hourly buckets an earlier version stored change nothing.
  await putBucket(store, 'eurusd', 'h1', march, bucketBars('h1', march));
  assert.equal((await planner.plan({ symbol: 'EURUSD', from: march, to: april })).requests, 31);
});

test('back: a month or a year more than the stored 1-minute history, up to now', async () => {
  const { store, planner } = harness(NOW);
  // Nothing stored: counted back from now.
  let plan = await planner.plan({ symbol: 'EURUSD', back: 'month' });
  assert.equal(plan.storedFrom, null);
  assert.equal(plan.from, s('2024-05-01T00:00:00Z'));
  assert.equal(plan.to, NOW);

  // May is stored: a month more starts in April, and the download reaches now.
  for (let d = s('2024-05-01T00:00:00Z'); d < s('2024-06-01T00:00:00Z'); d += DAY) {
    await putBucket(store, 'eurusd', 'm1', d, bucketBars('m1', d, { every: 3600 }));
  }
  plan = await planner.plan({ symbol: 'EURUSD', back: 'month' });
  assert.equal(plan.storedFrom, s('2024-05-01T00:00:00Z'));
  assert.equal(plan.from, s('2024-04-01T00:00:00Z'));
  assert.equal(plan.to, NOW);
  // April's 30 days and today's.
  assert.equal(plan.requests, 31);
  plan = await planner.plan({ symbol: 'EURUSD', back: 'year' });
  assert.equal(plan.from, s('2023-05-01T00:00:00Z'));
  assert.equal(plan.requests, 366 + 1);

  // Never before the history's start.
  store.setLearnedStart('eurusd', 'm1', { t: s('2024-04-15T00:00:00Z'), evidence: 'test' });
  plan = await planner.plan({ symbol: 'EURUSD', back: 'year' });
  assert.equal(plan.from, s('2024-04-15T00:00:00Z'));
  assert.equal(plan.requests, 16 + 1);

  // TradingView symbols have no 1-minute history to count from.
  assert.equal((await planner.plan({ symbol: 'NASDAQ:AAPL', back: 'month' })).storedFrom, null);

  // Calendar months: the same day of the month, or the last one it has.
  assert.equal(backFrom(s('2024-03-31T00:00:00Z'), 'month'), s('2024-02-29T00:00:00Z'));
  assert.equal(backFrom(s('2024-02-29T00:00:00Z'), 'year'), s('2023-02-28T00:00:00Z'));
  assert.equal(backFrom(s('2024-01-15T13:30:00Z'), 'month'), s('2023-12-15T00:00:00Z'));
});

test('complete when only provisional (not yet due) and active days remain; everything starts at the 1-minute start', async () => {
  const { store, planner } = harness(NOW);
  const from = s('2024-05-29T00:00:00Z');
  await putBucket(store, 'eurusd', 'm1', from, bucketBars('m1', from));
  // A provisional copy not yet due counts as covered.
  await store.put('eurusd', 'm1', from + DAY, null, { builtAt: from + 2 * DAY + 60, final: false });
  await putBucket(store, 'eurusd', 'm1', from + 2 * DAY, bucketBars('m1', from + 2 * DAY));
  // The active bucket, stored a while ago: stale but covered.
  await store.put('eurusd', 'm1', from + 3 * DAY, null, { builtAt: NOW - 600, final: false });
  const plan = await planner.plan({ symbol: 'EURUSD', from });
  assert.equal(plan.requests, 0);
  assert.equal(plan.complete, true);
  assert.equal(plan.seconds, 0);
  assert.equal(plan.to, NOW);

  // Everything, by default: 1-minute days from their start.
  const all = await planner.plan({ symbol: 'EURUSD' });
  assert.deepEqual(all.tiers, ['m1']);
  assert.equal(all.from, s('2003-05-04T19:00:00Z'));
  assert.deepEqual(
    all.perTier.map((t) => t.tier),
    ['m1'],
  );
});

test('a plan names the unfinished job that already covers its range', async () => {
  const jobs = [job({ from: s('2020-01-01T00:00:00Z'), to: NOW - DAY, openEnd: true, status: 'paused' })];
  const { planner } = harness(NOW, jobs);
  const plan = await planner.plan({ symbol: 'FX:EURUSD', from: s('2021-01-01T00:00:00Z') });
  assert.deepEqual(plan.job, { id: 'j1', status: 'paused' });
  const wider = await planner.plan({ symbol: 'FX:EURUSD', from: s('2019-01-01T00:00:00Z') });
  assert.equal(wider.job, null);
});

test('TradingView plans are approximate and complete once every timeframe reached its start', async () => {
  const { tv, planner } = harness(NOW);
  const plan = await planner.plan({ symbol: 'NASDAQ:AAPL' });
  assert.equal(plan.source, 'tradingview');
  assert.equal(plan.key, 'NASDAQ:AAPL');
  assert.equal(plan.approximate, true);
  assert.equal(plan.complete, false);
  assert.deepEqual(plan.tiers, ['1m', '5m', '15m', '1h', '4h', '1d', '1w']);
  assert.ok(plan.requests > 0);
  assert.equal(plan.seconds, Math.ceil(plan.requests * 3));
  for (const tf of plan.tiers) tv.setReachedStart('NASDAQ:AAPL', tf as '1m', s('2010-01-01T00:00:00Z'));
  const done = await planner.plan({ symbol: 'NASDAQ:AAPL' });
  assert.equal(done.complete, true);
  assert.equal(done.requests, 0);
  const one = await planner.plan({ symbol: 'NASDAQ:AAPL', tiers: ['1d'] });
  assert.deepEqual(one.tiers, ['1d']);
});

test('interactive needs are the 1-minute days a read lacks, newest first, at any timeframe', async () => {
  const { store, planner } = harness(s('2013-03-10T00:00:00Z'));
  const feb = s('2013-02-01T00:00:00Z');
  const march = s('2013-03-01T00:00:00Z');
  // An hourly March from an earlier version: not read, so it covers nothing.
  await putBucket(store, 'gbpchf', 'h1', march, bucketBars('h1', march));
  for (const tf of ['5m', '1h', '1d'] as const) {
    const needs = await planner.interactive('GBPCHF', tf, feb, march, 60);
    assert.equal(needs.length, 28, tf);
    assert.ok(
      needs.every((n) => n.tier === 'm1'),
      tf,
    );
    assert.equal(needs[0]!.start, march - DAY, `${tf}: newest first`);
  }
  assert.equal((await planner.interactive('GBPCHF', '1h', feb, march, 10)).length, 10);

  // A day fetched empty (a weekend) is covered: never asked for again.
  await putBucket(store, 'gbpchf', 'm1', s('2013-02-02T00:00:00Z'), null);
  const needs = await planner.interactive('GBPCHF', '1d', feb, march, 60);
  assert.equal(needs.length, 27);
  assert.ok(!needs.some((n) => n.start === s('2013-02-02T00:00:00Z')));

  // A stock CFD routes to TradingView: nothing to fetch from Dukascopy.
  assert.deepEqual(await planner.interactive('NASDAQ:AAPL', '1h', feb, march, 60), []);
});

test("a TradingView job covers any from: it pages back to TradingView's start whatever it recorded", async () => {
  // Started with nothing cached: its recorded range is [now, now].
  const jobs = [
    job({
      source: 'tradingview',
      key: 'NASDAQ:AAPL',
      from: NOW,
      to: NOW,
      openEnd: true,
      tiers: ['1m', '5m', '15m', '1h', '4h', '1d', '1w'],
    }),
  ];
  const { planner } = harness(NOW, jobs);
  assert.deepEqual((await planner.plan({ symbol: 'NASDAQ:AAPL', from: s('2020-01-01T00:00:00Z') })).job, {
    id: 'j1',
    status: 'running',
  });
  assert.equal((await planner.plan({ symbol: 'NASDAQ:AAPL' })).job?.id, 'j1');
  // Dukascopy jobs still need the range.
  const duka = harness(NOW, [job({ from: NOW - DAY, to: NOW, openEnd: true })]);
  assert.equal((await duka.planner.plan({ symbol: 'EURUSD', from: s('2020-01-01T00:00:00Z') })).job, null);
});

test('a day stored while still open is planned once its re-fetch is due, so the range is not complete', async () => {
  // Fri 10:00: Thursday's m1 day (and, from an earlier version, this month's h1 and this year's d1)
  // were stored Thu 15:00.
  const now = s('2026-09-25T10:00:00Z');
  const thu = s('2026-09-24T00:00:00Z');
  const { store, planner } = harness(now);
  const builtAt = s('2026-09-24T15:00:00Z');
  const opts = { to: builtAt - 3600 };
  const put = (tier: 'm1' | 'h1' | 'd1', start: number) =>
    store.put('eurusd', tier, start, bucketBody(tier, start, bucketBars(tier, start, opts)), { builtAt, final: false });
  await put('m1', thu);
  await put('h1', s('2026-09-01T00:00:00Z'));
  await put('d1', s('2026-01-01T00:00:00Z'));
  assert.equal(store.needsFetch('eurusd', 'm1', thu, now), true, 'the day is due');
  for (const tiers of [undefined, ['m1']]) {
    const plan = await planner.plan({ symbol: 'EURUSD', from: thu, to: thu + DAY, tiers });
    assert.equal(plan.complete, false, `tiers ${tiers}`);
    assert.equal(plan.perTier.find((t) => t.tier === 'm1')!.requests, 1);
    // Only the 1-minute day: coarser buckets are never planned.
    assert.equal(plan.requests, 1);
  }
});

test("the stored history's newest stretch bridges gaps shorter than two months", async () => {
  // July 1: May is stored, and the chart fetched today after a month away.
  const { store, planner } = harness(s('2024-07-01T12:00:00Z'));
  for (let d = s('2024-05-01T00:00:00Z'); d < s('2024-06-01T00:00:00Z'); d += DAY) {
    await putBucket(store, 'eurusd', 'm1', d, bucketBars('m1', d, { every: 3600 }));
  }
  await store.put('eurusd', 'm1', s('2024-07-01T00:00:00Z'), null, {
    builtAt: s('2024-07-01T12:00:00Z'),
    final: false,
  });
  // Today's day does not make the history start today: the June gap is bridged (the download fills it).
  assert.equal(planner.storedFrom('eurusd'), s('2024-05-01T00:00:00Z'));
  const plan = await planner.plan({ symbol: 'EURUSD', back: 'month' });
  assert.equal(plan.from, s('2024-04-01T00:00:00Z'));
  // April and June: one download leaves April to today stored.
  assert.equal(plan.requests, 30 + 30);
  // A stretch behind a longer gap is another one.
  await putBucket(store, 'eurusd', 'm1', s('2024-02-01T00:00:00Z'), bucketBars('m1', s('2024-02-01T00:00:00Z')));
  assert.equal(planner.storedFrom('eurusd'), s('2024-05-01T00:00:00Z'));
  // Gaps under two months chain back: 41 days to May, 47 back to February.
  await putBucket(store, 'eurusd', 'm1', s('2024-03-20T00:00:00Z'), bucketBars('m1', s('2024-03-20T00:00:00Z')));
  assert.equal(planner.storedFrom('eurusd'), s('2024-02-01T00:00:00Z'));
});
