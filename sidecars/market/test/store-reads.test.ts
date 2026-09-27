// What reads fetch before they read: bars.latest walking back over closed markets, tool reads
// refreshing the due provisional buckets they show, and the sweep. Real store, planner and series in
// a temp dir; the fetcher double answers from synthetic forex sessions (no network).

import assert from 'node:assert/strict';
import { afterEach, test } from 'node:test';

import { type Tier } from '../src/store/buckets.ts';
import { DukascopyStore } from '../src/store/dukascopy-store.ts';
import { Planner } from '../src/store/planner.ts';
import { INTERACTIVE_BUDGET, Reads } from '../src/store/reads.ts';
import { Series } from '../src/store/series.ts';
import { TvStore } from '../src/store/tv-store.ts';
import {
  DAY,
  FakeFetcher,
  bucketBars,
  bucketBody,
  cleanup,
  iso,
  putBucket,
  s,
  tempDir,
  until,
} from './store-helpers.ts';

afterEach(cleanup);

type Data = (instrument: string, tier: Tier, start: number) => ReturnType<typeof bucketBars> | 'unavailable';

function harness(nowSec: number, data?: Data) {
  const clock = { now: nowSec };
  const ms = () => clock.now * 1000;
  const cache = tempDir();
  const store = new DukascopyStore(cache, { now: ms, manifestDelayMs: 20 });
  const tv = new TvStore(cache, { now: ms, flushDelayMs: 20 });
  const series = new Series({ store, tv, now: ms });
  // Every bucket as Dukascopy would serve it at the harness's clock (the open one up to now).
  const fetcher = new FakeFetcher(store, data ?? ((_i, tier, start) => bucketBars(tier, start, { to: clock.now })), ms);
  const planner = new Planner({ store, fetcher, tv, series, now: ms });
  const reads = new Reads({ store, fetcher, planner, series, now: ms });
  return { clock, store, tv, series, fetcher, reads };
}

const asked = (fetcher: FakeFetcher) =>
  fetcher.requests.map((k) => {
    const [, tier, start] = k.split('/');
    return `${tier} ${iso(Number(start)).slice(0, 10)}`;
  });

const SUNDAY = s('2026-09-27T05:15:00Z');

test('bars.latest on a Sunday walks back over the empty weekend to Friday (empty cache)', async () => {
  const h = harness(SUNDAY);
  const read = await h.reads.latest('FX:EURUSD', '1m', 500);
  assert.deepEqual(asked(h.fetcher), ['m1 2026-09-27', 'm1 2026-09-26', 'm1 2026-09-25']);
  assert.equal(read.bars.length, 500);
  assert.equal(iso(read.bars.at(-1)!.t), '2026-09-25T20:59:00.000Z');
  assert.equal(read.more, 'cached');

  // Opening the chart again asks for nothing and serves the same bars.
  h.fetcher.requests = [];
  const again = await h.reads.latest('FX:EURUSD', '1m', 500);
  assert.deepEqual(h.fetcher.requests, []);
  assert.deepEqual(again.bars, read.bars);
});

test('bars.latest on a Sunday fetches Friday between stored history and the fetched weekend', async () => {
  const h = harness(SUNDAY);
  for (let d = s('2026-09-21T00:00:00Z'); d < s('2026-09-25T00:00:00Z'); d += DAY) {
    await putBucket(h.store, 'eurusd', 'm1', d, bucketBars('m1', d));
  }
  const read = await h.reads.latest('FX:EURUSD', '1m', 500);
  assert.deepEqual(asked(h.fetcher), ['m1 2026-09-27', 'm1 2026-09-26', 'm1 2026-09-25']);
  assert.equal(read.bars.length, 500);
  assert.equal(iso(read.bars.at(-1)!.t), '2026-09-25T20:59:00.000Z');
});

test('bars.latest serves what is stored when Dukascopy cannot be reached', async () => {
  const h = harness(SUNDAY, () => {
    throw new Error('offline');
  });
  for (let d = s('2026-09-21T00:00:00Z'); d < s('2026-09-25T00:00:00Z'); d += DAY) {
    await putBucket(h.store, 'eurusd', 'm1', d, bucketBars('m1', d));
  }
  const read = await h.reads.latest('FX:EURUSD', '1m', 500);
  // The first window failed: no walk back after it.
  assert.deepEqual(asked(h.fetcher), ['m1 2026-09-27', 'm1 2026-09-26']);
  assert.equal(read.bars.length, 500);
  assert.equal(iso(read.bars.at(-1)!.t), '2026-09-24T23:59:00.000Z');
});

test('bars.latest on a weekday asks only for the window the newest bars span', async () => {
  const h = harness(s('2026-09-23T12:00:00Z'));
  const read = await h.reads.latest('FX:EURUSD', '1m', 500);
  assert.deepEqual(asked(h.fetcher), ['m1 2026-09-23', 'm1 2026-09-22']);
  assert.equal(read.bars.length, 500);
  assert.equal(iso(read.bars.at(-1)!.t), '2026-09-23T11:59:00.000Z');
});

test('bars.latest stops at the request budget when nothing has bars', async () => {
  const h = harness(SUNDAY, () => []);
  const read = await h.reads.latest('FX:EURUSD', '1m', 500);
  assert.equal(read.bars.length, 0);
  assert.equal(read.more, 'gap');
  assert.ok(h.fetcher.requests.length > 10, 'it walked back');
  assert.ok(h.fetcher.requests.length <= INTERACTIVE_BUDGET, `${h.fetcher.requests.length} requests`);
});

test('a tool read re-fetches the due provisional buckets it shows, never missing ones', async () => {
  // Thursday 15:00: the chart stored the open day (bars up to 14:59) and the open month.
  const h = harness(s('2026-09-24T15:00:00Z'));
  const thu = s('2026-09-24T00:00:00Z');
  const partial = bucketBars('m1', thu, { to: h.clock.now });
  await h.store.put('eurusd', 'm1', thu, bucketBody('m1', thu, partial), { builtAt: h.clock.now, final: false });
  // Friday 10:00, same process.
  h.clock.now = s('2026-09-25T10:00:00Z');
  const to = thu + DAY - 1;
  const stale = await h.series.read({ symbol: 'EURUSD', tf: '1m', from: thu, to });
  assert.equal(iso(stale.bars.at(-1)!.t), '2026-09-24T14:59:00.000Z', 'the partial day is what the store has');
  assert.deepEqual(h.series.missing('EURUSD', '1m', thu, to), [], 'and it counts as covered');

  // A range with an unfetched day (Wednesday) and today's open day: only Thursday is asked for.
  assert.equal(await h.reads.refreshDue('EURUSD', '1m', thu - DAY, h.clock.now), 1);
  assert.deepEqual(asked(h.fetcher), ['m1 2026-09-24']);
  const fresh = await h.series.read({ symbol: 'EURUSD', tf: '1m', from: thu, to });
  assert.equal(iso(fresh.bars.at(-1)!.t), '2026-09-24T23:59:00.000Z');
  assert.equal(h.store.status('eurusd', 'm1', thu), 'final');
  assert.equal(await h.reads.refreshDue('EURUSD', '1m', thu, to), 0);
});

test('a tool read at 1h re-fetches the h1 month it reads, not the m1 days under it', async () => {
  const h = harness(s('2026-08-31T15:00:00Z'));
  const aug = s('2026-08-01T00:00:00Z');
  const month = bucketBars('h1', aug, { to: h.clock.now });
  await h.store.put('eurusd', 'h1', aug, bucketBody('h1', aug, month), { builtAt: h.clock.now, final: false });
  const day = s('2026-08-31T00:00:00Z');
  await h.store.put('eurusd', 'm1', day, bucketBody('m1', day, bucketBars('m1', day, { to: h.clock.now })), {
    builtAt: h.clock.now,
    final: false,
  });
  h.clock.now = s('2026-09-02T10:00:00Z');
  assert.equal(await h.reads.refreshDue('EURUSD', '1h', day, day + DAY - 1), 1);
  assert.deepEqual(asked(h.fetcher), ['h1 2026-08-01']);
  const read = await h.series.read({ symbol: 'EURUSD', tf: '1h', from: day, to: day + DAY - 1 });
  assert.equal(iso(read.bars.at(-1)!.t), '2026-08-31T23:00:00.000Z');
});

test('the sweep asks for every due provisional bucket on the bulk lane, never an open one', async () => {
  const h = harness(s('2026-09-24T15:00:00Z'));
  const thu = s('2026-09-24T00:00:00Z');
  await h.store.put('eurusd', 'm1', thu, bucketBody('m1', thu, bucketBars('m1', thu, { to: h.clock.now })), {
    builtAt: h.clock.now,
    final: false,
  });
  h.clock.now = s('2026-09-25T10:00:00Z');
  const fri = s('2026-09-25T00:00:00Z');
  await h.store.put('eurusd', 'm1', fri, bucketBody('m1', fri, bucketBars('m1', fri, { to: fri + 3600 })), {
    builtAt: fri + 3600,
    final: false,
  });
  assert.equal(h.reads.sweepDue(), 1);
  assert.deepEqual(h.fetcher.lanes, [`bulk eurusd/m1/${thu}`]);
  await until(() => h.store.status('eurusd', 'm1', thu) === 'final', 5000, 'the re-fetch');
});

test('bars.latest at the weekly reopen walks back to Friday instead of taking the Sunday bucket as history', async () => {
  // Sunday 21:30: 30 bars since the 21:00 open. The Sunday bucket holds data, but none before 21:00.
  const h = harness(s('2026-09-27T21:30:00Z'));
  const read = await h.reads.latest('FX:EURUSD', '1m', 500);
  assert.ok(asked(h.fetcher).includes('m1 2026-09-25'), `Friday was fetched: ${asked(h.fetcher).join(', ')}`);
  assert.equal(read.bars.length, 500);
  assert.equal(iso(read.bars.at(-1)!.t), '2026-09-27T21:29:00.000Z');
});

test('a page that ran out of bars reports the gap it stopped at, not "cached"', async () => {
  const h = harness(s('2026-09-27T21:30:00Z'));
  for (const d of ['2026-09-26', '2026-09-27']) {
    const t = s(`${d}T00:00:00Z`);
    await h.store.put('eurusd', 'm1', t, bucketBody('m1', t, bucketBars('m1', t, { to: h.clock.now })), {
      builtAt: h.clock.now,
      final: false,
    });
  }
  const read = await h.series.read({ symbol: 'FX:EURUSD', tf: '1m', count: 500, latest: true });
  assert.equal(read.bars.length, 30);
  assert.equal(read.more, 'gap');
  assert.equal(iso(read.gap!.to), '2026-09-26T00:00:00.000Z');
  assert.ok(read.missing.length > 0 && read.missing[0]![1] === s('2026-09-26T00:00:00Z'));
});

test('bars.latest at 15m on a Saturday fills its count from the weeks before', async () => {
  const h = harness(s('2026-09-26T23:00:00Z'));
  const read = await h.reads.latest('FX:EURUSD', '15m', 500);
  assert.equal(read.bars.length, 500, `${read.bars.length} bars, more=${read.more}`);
  assert.equal(iso(read.bars.at(-1)!.t), '2026-09-25T20:45:00.000Z');
  // Paging back from the oldest bar finds stored bars or a real gap, never a false "cached".
  const older = await h.series.read({ symbol: 'FX:EURUSD', tf: '15m', count: 1, before: read.bars[0]!.t });
  assert.ok(read.more !== 'cached' || older.bars.length === 1, `more=${read.more} but nothing older is stored`);
});

test('a tool read re-fetches a stale open month it reads past its last fetch', async () => {
  // Thursday 15:00: a 1h chart stored the open September month (bars up to 14:00).
  const h = harness(s('2026-09-24T15:00:00Z'));
  const sep = s('2026-09-01T00:00:00Z');
  await h.store.put('eurusd', 'h1', sep, bucketBody('h1', sep, bucketBars('h1', sep, { to: h.clock.now })), {
    builtAt: h.clock.now,
    final: false,
  });
  h.clock.now = s('2026-09-25T10:00:00Z');
  const thu = s('2026-09-24T00:00:00Z');
  // Days before its fetch: nothing to refresh.
  assert.equal(await h.reads.refreshDue('EURUSD', '1h', s('2026-09-10T00:00:00Z'), s('2026-09-11T00:00:00Z')), 0);
  assert.equal(await h.reads.refreshDue('EURUSD', '1h', thu, thu + DAY - 1), 1);
  assert.deepEqual(asked(h.fetcher), ['h1 2026-09-01']);
  const read = await h.series.read({ symbol: 'EURUSD', tf: '1h', from: thu, to: thu + DAY - 1 });
  assert.equal(read.bars.length, 24);
  assert.equal(iso(read.bars.at(-1)!.t), '2026-09-24T23:00:00.000Z');
});

test('bars.latest says Dukascopy could not be reached only when nothing is stored and a fetch failed', async () => {
  const offline = harness(SUNDAY, () => {
    throw new Error('offline');
  });
  const empty = await offline.reads.latest('FX:EURUSD', '1m', 500);
  assert.equal(empty.bars.length, 0);
  assert.match(empty.fetchError ?? '', /offline/);
  // A closed market whose weekend was fetched fine is no error.
  const closed = harness(SUNDAY, () => []);
  assert.equal((await closed.reads.latest('FX:EURUSD', '1m', 500)).fetchError, undefined);
});
