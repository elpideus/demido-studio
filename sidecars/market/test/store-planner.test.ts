// Download plans and interactive needs over a real store in a temp dir (no network).

import assert from 'node:assert/strict';
import { afterEach, test } from 'node:test';

import { nextBucket } from '../src/store/buckets.ts';
import { DukascopyStore } from '../src/store/dukascopy-store.ts';
import { type JobView, Planner } from '../src/store/planner.ts';
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

test('weekend m1 days with no h1 bars in a final, non-empty h1 month are inferred empty', async () => {
  const { store, planner } = harness(NOW);
  const march = s('2024-03-01T00:00:00Z');
  const range = { symbol: 'EURUSD', from: march, to: nextBucket('h1', march), tiers: ['m1'] };
  assert.equal((await planner.plan(range)).requests, 31, 'nothing inferred without h1');

  // Provisional h1: days after its build would look empty, so nothing is inferred.
  await putBucket(store, 'eurusd', 'h1', march, bucketBars('h1', march), false);
  assert.equal((await planner.plan(range)).requests, 31);

  await putBucket(store, 'eurusd', 'h1', march, bucketBars('h1', march), true);
  // Saturdays have no h1 bars; Sundays trade from 21:00, so they are still asked for.
  const saturdays = [2, 9, 16, 23, 30].map((d) => march + (d - 1) * DAY);
  const inferred = await planner.inferableDays(
    'eurusd',
    [...Array(31).keys()].map((i) => march + i * DAY),
  );
  assert.deepEqual([...inferred].sort(), saturdays);
  assert.equal((await planner.plan(range)).requests, 31 - 5);

  // Never from an empty h1 month.
  const feb = s('2024-02-01T00:00:00Z');
  await putBucket(store, 'eurusd', 'h1', feb, null);
  assert.equal((await planner.inferableDays('eurusd', [s('2024-02-03T00:00:00Z')])).size, 0);

  // Recording them makes them fetched-empty m1 buckets: covered, never requested.
  await planner.recordInferred('eurusd', inferred);
  assert.equal(store.status('eurusd', 'm1', saturdays[0]!), 'final');
  assert.ok(store.sets('eurusd', 'm1').empty.contains(saturdays[0]!));
  assert.equal((await planner.plan(range)).requests, 31 - 5);
});

test('the tiers filter restricts the plan; complete when only provisional and active buckets remain', async () => {
  const { store, planner } = harness(NOW);
  const from = s('2024-05-29T00:00:00Z');
  await putBucket(store, 'eurusd', 'm1', from, bucketBars('m1', from));
  // A provisional copy not yet due counts as covered.
  await store.put('eurusd', 'm1', from + DAY, null, { builtAt: from + 2 * DAY + 60, final: false });
  await putBucket(store, 'eurusd', 'm1', from + 2 * DAY, bucketBars('m1', from + 2 * DAY));
  // The active bucket, stored a while ago: stale but covered.
  await store.put('eurusd', 'm1', from + 3 * DAY, null, { builtAt: NOW - 600, final: false });
  const plan = await planner.plan({ symbol: 'EURUSD', from, tiers: ['m1'] });
  assert.equal(plan.requests, 0);
  assert.equal(plan.complete, true);
  assert.equal(plan.seconds, 0);
  assert.equal(plan.to, NOW);

  const h1 = await planner.plan({ symbol: 'EURUSD', from, tiers: ['h1'] });
  assert.deepEqual(h1.tiers, ['h1']);
  assert.deepEqual(
    h1.perTier.map((t) => [t.tier, t.requests]),
    [['h1', 2]],
  );
  assert.equal(h1.complete, false);

  // Everything, by default: every tier from its effective start.
  const all = await planner.plan({ symbol: 'EURUSD' });
  assert.deepEqual(all.tiers, ['m1', 'h1', 'd1']);
  assert.equal(all.from, s('1973-03-01T00:00:00Z'));
  assert.equal(all.perTier.find((t) => t.tier === 'h1')!.from, s('2003-05-04T19:00:00Z'));
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

test('interactive needs: the coarsest usable tier first, finer where it is known empty', async () => {
  const { store, planner } = harness(s('2013-03-10T00:00:00Z'));
  const feb = s('2013-02-01T00:00:00Z');
  const march = s('2013-03-01T00:00:00Z');
  await putBucket(store, 'gbpchf', 'h1', march, bucketBars('h1', march), false);
  let needs = await planner.interactive('GBPCHF', '1h', feb, march, 60);
  assert.deepEqual(needs, [{ tier: 'h1', start: feb }]);

  // The h1 month came back empty: m1 has to answer for it (nothing inferred from an empty month).
  await putBucket(store, 'gbpchf', 'h1', feb, null);
  needs = await planner.interactive('GBPCHF', '1h', feb, march, 60);
  assert.equal(needs.length, 28);
  assert.ok(needs.every((n) => n.tier === 'm1'));
  assert.equal(needs[0]!.start, march - DAY, 'newest first');
  assert.equal((await planner.interactive('GBPCHF', '1h', feb, march, 10)).length, 10);

  // A 5m chart only has m1; Saturdays of a final, non-empty h1 month are recorded, not requested.
  await putBucket(store, 'gbpchf', 'h1', s('2013-01-01T00:00:00Z'), bucketBars('h1', s('2013-01-01T00:00:00Z')));
  needs = await planner.interactive('GBPCHF', '5m', s('2013-01-01T00:00:00Z'), s('2013-01-08T00:00:00Z'), 60);
  assert.equal(needs.length, 6);
  assert.equal(store.status('gbpchf', 'm1', s('2013-01-05T00:00:00Z')), 'final');

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
  // Fri 10:00: Thursday's m1 day, this month's h1 and this year's d1 were stored Thu 15:00.
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
    // The open h1 month and d1 year are not due: reads keep them fresh.
    assert.equal(plan.requests, 1);
  }
});
