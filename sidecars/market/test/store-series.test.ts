// The read path over a real store in a temp dir, with synthetic buckets (no network).

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, test } from 'node:test';

import { type Bar } from '../src/protocol.ts';
import { aggregate } from '../src/store/aggregate.ts';
import { nextBucket } from '../src/store/buckets.ts';
import { DukascopyStore } from '../src/store/dukascopy-store.ts';
import { type ReadResult, Series } from '../src/store/series.ts';
import { TvStore, coverOf, toleratedGap } from '../src/store/tv-store.ts';
import { DAY, bucketBars, bucketBody, cleanup, fxOpen, iso, priceAt, putBucket, s, tempDir } from './store-helpers.ts';

afterEach(cleanup);

const NOW = s('2026-09-27T04:00:00Z');

function harness(now = NOW) {
  const cache = tempDir();
  const clock = () => now * 1000;
  const store = new DukascopyStore(cache, { now: clock, manifestDelayMs: 20 });
  const tv = new TvStore(cache, { now: clock, flushDelayMs: 20 });
  const series = new Series({ store, tv, now: clock });
  return { cache, store, tv, series };
}

/** Hourly TradingView bars (fx sessions) in [from, to), priced apart from Dukascopy's. */
function tvBars(from: number, to: number): Bar[] {
  const out: Bar[] = [];
  for (let t = from; t < to; t += 3600) {
    if (!fxOpen(t)) continue;
    const p = Math.round((priceAt(t) + 0.01) * 1e5) / 1e5;
    out.push({ t, o: p, h: p, l: p, c: p, v: 7 });
  }
  return out;
}

async function pageAll(series: Series, symbol: string, tf: '1h' | '1d', before: number, count: number) {
  const pages: ReadResult[] = [];
  let edge = before;
  for (let i = 0; i < 500; i += 1) {
    const page = await series.read({ symbol, tf, before: edge, count });
    pages.push(page);
    if (page.more !== 'cached') break;
    assert.ok(page.bars.length > 0, 'a cached page always has bars');
    edge = page.bars[0]!.t;
  }
  return pages;
}

function assertAscendingUnique(bars: readonly Bar[]): void {
  for (let i = 1; i < bars.length; i += 1) {
    assert.ok(bars[i]!.t > bars[i - 1]!.t, `bars ascend and are unique at ${iso(bars[i]!.t)}`);
  }
}

test('EURUSD: h1 history 2003-2026 plus a TradingView interval pages back to 2003, then none', async () => {
  const { store, tv, series } = harness();
  let expected = 0;
  for (let m = s('2003-05-01T00:00:00Z'); m < s('2026-09-01T00:00:00Z'); m = nextBucket('h1', m)) {
    // One bar a day keeps 23 years small.
    const bars = bucketBars('h1', m, { every: DAY, from: s('2003-05-05T00:00:00Z') });
    expected += bars.filter((b) => b.t < s('2024-01-01T00:00:00Z')).length;
    await putBucket(store, 'eurusd', 'h1', m, bars);
  }
  // The chart's last live stream: 500 hourly bars up to now.
  const live = tvBars(NOW - 40 * DAY, NOW).slice(-500);
  tv.record('FX:EURUSD', '1h', live, coverOf(live, '1h', NOW));

  const pages = await pageAll(series, 'FX:EURUSD', '1h', s('2024-01-01T00:00:00Z'), 500);
  const first = pages[0]!;
  assert.equal(first.bars.length, 500);
  assert.equal(first.more, 'cached');
  assert.deepEqual(first.keys, { dukascopy: 'eurusd', tradingview: 'FX:EURUSD' });
  assert.ok(first.bars.every((b) => b.t < s('2024-01-01T00:00:00Z')));
  assert.equal(pages.at(-1)!.more, 'none');
  assert.ok(pages.slice(0, -1).every((p) => p.more === 'cached'));
  const all = pages
    .slice()
    .reverse()
    .flatMap((p) => p.bars);
  assertAscendingUnique(all);
  assert.equal(all.length, expected);
  assert.equal(iso(all[0]!.t), '2003-05-05T00:00:00.000Z');
  assert.ok(pages.every((p) => p.spans.every((sp) => sp.source === 'dukascopy')));

  // The newest 700: TradingView's 500, then Dukascopy right before them, without a gap.
  const latest = await series.read({ symbol: 'FX:EURUSD', tf: '1h', count: 700, latest: true });
  assert.equal(latest.bars.length, 700);
  assert.equal(latest.more, 'cached');
  assert.deepEqual(
    latest.spans.map((sp) => [sp.source, sp.count]),
    [
      ['dukascopy', 200],
      ['tradingview', 500],
    ],
  );
  // Paging from the stream's oldest bar is Dukascopy's.
  const older = await series.read({ symbol: 'FX:EURUSD', tf: '1h', before: live[0]!.t, count: 50 });
  assert.equal(older.bars.length, 50);
  assert.ok(older.bars.every((b, i) => b.t < live[0]!.t && older.sources[i] === 'dukascopy'));
});

test('1d reaches the d1 start, reading d1 years and aggregating h1 months', async () => {
  const { store, series } = harness();
  for (let y = s('1973-01-01T00:00:00Z'); y < s('2004-01-01T00:00:00Z'); y = nextBucket('d1', y)) {
    await putBucket(store, 'eurusd', 'd1', y, bucketBars('d1', y, { from: s('1973-03-01T00:00:00Z') }));
  }
  for (let m = s('2004-01-01T00:00:00Z'); m < s('2004-07-01T00:00:00Z'); m = nextBucket('h1', m)) {
    await putBucket(store, 'eurusd', 'h1', m, bucketBars('h1', m));
  }
  const pages = await pageAll(series, 'EURUSD', '1d', s('2004-07-01T00:00:00Z'), 500);
  assert.equal(pages.at(-1)!.more, 'none');
  const all = pages
    .slice()
    .reverse()
    .flatMap((p) => p.bars);
  assertAscendingUnique(all);
  assert.equal(iso(all[0]!.t), '1973-03-01T00:00:00.000Z');
  // Session days: Sunday's bars fold into Monday, so no bar is labelled a weekend day.
  assert.ok(all.every((b) => ![0, 6].includes(new Date(b.t * 1000).getUTCDay())));
  // A 2004 daily bar is built from that day's h1 bars (Sunday evening included for Mondays).
  const monday = all.find((b) => b.t === s('2004-03-01T00:00:00Z'))!;
  const hours = bucketBars('h1', s('2004-02-01T00:00:00Z'))
    .concat(bucketBars('h1', s('2004-03-01T00:00:00Z')))
    .filter((b) => b.t >= s('2004-02-29T00:00:00Z') && b.t < s('2004-03-02T00:00:00Z'));
  assert.equal(monday.o, hours[0]!.o);
  assert.equal(monday.c, hours.at(-1)!.c);
  assert.equal(monday.h, Math.max(...hours.map((b) => b.h)));
});

test('GBP-CHF 2013-02: an empty h1 month is a gap until m1 is fetched, then m1 fills it', async () => {
  const { store, series } = harness();
  await putBucket(store, 'gbpchf', 'h1', s('2013-01-01T00:00:00Z'), bucketBars('h1', s('2013-01-01T00:00:00Z')));
  await putBucket(store, 'gbpchf', 'h1', s('2013-02-01T00:00:00Z'), null);
  await putBucket(store, 'gbpchf', 'h1', s('2013-03-01T00:00:00Z'), bucketBars('h1', s('2013-03-01T00:00:00Z')));
  const before = s('2013-03-05T00:00:00Z');

  const gap = await series.read({ symbol: 'SAXO:GBPCHF', tf: '1h', before, count: 5000 });
  assert.equal(gap.more, 'gap');
  assert.deepEqual(gap.gap, { from: s('2013-02-01T00:00:00Z'), to: s('2013-03-01T00:00:00Z'), source: 'dukascopy' });
  assert.ok(gap.bars.every((b) => b.t >= s('2013-03-01T00:00:00Z')));
  assert.deepEqual(series.missing('GBPCHF', '1h', s('2013-01-15T00:00:00Z'), s('2013-03-10T00:00:00Z')), [
    [s('2013-02-01T00:00:00Z'), s('2013-03-01T00:00:00Z')],
  ]);

  for (let d = s('2013-02-01T00:00:00Z'); d < s('2013-03-01T00:00:00Z'); d += DAY) {
    await putBucket(store, 'gbpchf', 'm1', d, bucketBars('m1', d, { every: 600 }));
  }
  const filled = await series.read({ symbol: 'SAXO:GBPCHF', tf: '1h', before, count: 5000 });
  assertAscendingUnique(filled.bars);
  const feb = filled.bars.filter((b) => b.t >= s('2013-02-01T00:00:00Z') && b.t < s('2013-03-01T00:00:00Z'));
  assert.equal(feb.length, bucketBars('h1', s('2013-02-01T00:00:00Z')).length);
  assert.ok(
    filled.bars.some((b) => b.t < s('2013-02-01T00:00:00Z')),
    'reaches January through February',
  );
  assert.equal(filled.more, 'gap');
  assert.equal(filled.gap!.to, s('2013-01-01T00:00:00Z'));
  assert.deepEqual(series.missing('GBPCHF', '1h', s('2013-01-15T00:00:00Z'), s('2013-03-10T00:00:00Z')), []);
});

test('a week straddling h1 and m1 aggregates once, with ascending unique times', async () => {
  const { store, series } = harness();
  await putBucket(store, 'eurusd', 'h1', s('2024-03-01T00:00:00Z'), bucketBars('h1', s('2024-03-01T00:00:00Z')));
  const minutes: Bar[] = [];
  for (let d = s('2024-04-01T00:00:00Z'); d < s('2024-04-07T00:00:00Z'); d += DAY) {
    const bars = bucketBars('m1', d, { every: 300 });
    minutes.push(...bars);
    await putBucket(store, 'eurusd', 'm1', d, bars);
  }
  // Week of Monday 2024-04-01: Sunday 03-31 (h1, March) through Saturday 04-06 (m1).
  const week = await series.read({
    symbol: 'FX:EURUSD',
    tf: '1w',
    from: s('2024-03-25T00:00:00Z'),
    to: s('2024-04-06T00:00:00Z'),
  });
  const bar = week.bars.find((b) => b.t === s('2024-04-01T00:00:00Z'));
  assert.ok(bar, 'the straddling week has a bar');
  const sunday = bucketBars('h1', s('2024-03-01T00:00:00Z')).filter((b) => b.t >= s('2024-03-31T00:00:00Z'));
  const [expected] = aggregate([...sunday, ...minutes], '1w', { week: 'session' });
  assert.deepEqual(bar, expected);
  // The week before is all h1 (March), and is complete too.
  assert.ok(week.bars.some((b) => b.t === s('2024-03-25T00:00:00Z')));

  const hours = await series.read({
    symbol: 'FX:EURUSD',
    tf: '1h',
    from: s('2024-03-31T18:00:00Z'),
    to: s('2024-04-01T03:00:00Z'),
  });
  assertAscendingUnique(hours.bars);
  assert.deepEqual(
    hours.bars.map((b) => iso(b.t).slice(0, 13)),
    [
      '2024-03-31T21',
      '2024-03-31T22',
      '2024-03-31T23',
      '2024-04-01T00',
      '2024-04-01T01',
      '2024-04-01T02',
      '2024-04-01T03',
    ],
  );
});

test('TradingView wins inside its coverage; tolerated seams and weekends are not gaps', async () => {
  const { store, tv, series } = harness();
  await putBucket(store, 'eurusd', 'h1', s('2024-04-01T00:00:00Z'), bucketBars('h1', s('2024-04-01T00:00:00Z')));
  // TradingView from 02:00 on May 1: a 2-hour seam after Dukascopy's April (under 3 bars).
  const a = tvBars(s('2024-05-01T02:00:00Z'), s('2024-05-10T21:00:00Z'));
  tv.record('FX:EURUSD', '1h', a, [a[0]!.t, s('2024-05-10T21:00:00Z')]);
  // Another interval after the weekend.
  const b = tvBars(s('2024-05-12T21:00:00Z'), s('2024-05-15T00:00:00Z'));
  tv.record('FX:EURUSD', '1h', b, [b[0]!.t, s('2024-05-15T00:00:00Z')]);

  const page = await series.read({ symbol: 'FX:EURUSD', tf: '1h', before: s('2024-05-15T00:00:00Z'), count: 400 });
  assert.equal(page.bars.length, 400);
  assert.equal(page.more, 'cached');
  assert.deepEqual(
    page.spans.map((sp) => sp.source),
    ['dukascopy', 'tradingview'],
  );
  const tvSpan = page.spans[1]!;
  assert.equal(tvSpan.from, a[0]!.t);
  assert.equal(tvSpan.count, a.length + b.length);
  const all = await pageAll(series, 'FX:EURUSD', '1h', s('2024-05-15T00:00:00Z'), 400);
  assert.equal(all.at(-1)!.more, 'gap');
  assert.equal(all.at(-1)!.gap!.to, s('2024-04-01T00:00:00Z'));

  // Dukascopy for May too: TradingView still wins where it covers; the seam hours are Dukascopy's.
  await putBucket(store, 'eurusd', 'h1', s('2024-05-01T00:00:00Z'), bucketBars('h1', s('2024-05-01T00:00:00Z')));
  const both = await series.read({
    symbol: 'FX:EURUSD',
    tf: '1h',
    from: s('2024-04-30T22:00:00Z'),
    to: s('2024-05-01T04:00:00Z'),
  });
  assert.deepEqual(
    both.bars.map((bar, i) => `${iso(bar.t).slice(11, 13)} ${both.sources[i]}`),
    [
      '22 dukascopy',
      '23 dukascopy',
      '00 dukascopy',
      '01 dukascopy',
      '02 tradingview',
      '03 tradingview',
      '04 tradingview',
    ],
  );
  assert.equal(both.bars[4]!.v, 7);
});

test('a gap between TradingView intervals longer than 3 bars on a weekday is a gap', async () => {
  const { tv, series } = harness();
  const a = tvBars(s('2024-05-20T00:00:00Z'), s('2024-05-22T12:00:00Z'));
  tv.record('NSE:RELIANCE', '1h', a, [a[0]!.t, s('2024-05-22T12:00:00Z')]);
  const b = tvBars(s('2024-05-22T18:00:00Z'), s('2024-05-24T00:00:00Z'));
  tv.record('NSE:RELIANCE', '1h', b, [b[0]!.t, s('2024-05-24T00:00:00Z')]);
  const page = await series.read({ symbol: 'NSE:RELIANCE', tf: '1h', before: s('2024-05-24T00:00:00Z'), count: 400 });
  assert.deepEqual(page.keys, { tradingview: 'NSE:RELIANCE' });
  assert.equal(page.bars.length, b.length);
  assert.equal(page.more, 'gap');
  assert.deepEqual(page.gap, { from: s('2024-05-22T12:00:00Z'), to: s('2024-05-22T18:00:00Z'), source: 'tradingview' });

  // The tolerance itself.
  assert.equal(toleratedGap(s('2024-05-22T12:00:00Z'), s('2024-05-22T14:30:00Z'), '1h'), true);
  assert.equal(toleratedGap(s('2024-05-22T12:00:00Z'), s('2024-05-22T18:00:00Z'), '1h'), false);
  assert.equal(toleratedGap(s('2024-05-24T21:00:00Z'), s('2024-05-26T21:00:00Z'), '1m'), true);
  assert.equal(toleratedGap(s('2024-05-24T21:00:00Z'), s('2024-05-26T21:00:00Z'), '1m', false), false);
  assert.equal(toleratedGap(s('2024-12-24T21:00:00Z'), s('2024-12-25T22:00:00Z'), '1h'), true);
});

test('the TradingView bar still forming when stored is served for a TradingView-only market', async () => {
  const { tv, series } = harness(s('2024-05-22T10:30:00Z'));
  const bars = tvBars(s('2024-05-21T00:00:00Z'), s('2024-05-22T11:00:00Z'));
  const cover = coverOf(bars, '1h', s('2024-05-22T10:30:00Z'))!;
  assert.equal(cover[1], s('2024-05-22T10:00:00Z'), 'the forming bar is not covered');
  tv.record('NSE:RELIANCE', '1h', bars, cover);
  const latest = await series.read({ symbol: 'NSE:RELIANCE', tf: '1h', count: 10, latest: true });
  assert.equal(latest.bars.at(-1)!.t, s('2024-05-22T10:00:00Z'));
});

test('latest serves the newest stored bars past a fetched, empty weekend', async () => {
  const { store, series } = harness(s('2026-09-27T05:15:00Z'));
  for (let d = s('2026-09-21T00:00:00Z'); d < s('2026-09-25T00:00:00Z'); d += DAY) {
    await putBucket(store, 'eurusd', 'm1', d, bucketBars('m1', d));
  }
  // The chart fetched Saturday (empty, final) and the open Sunday (empty so far); Friday is not stored.
  await putBucket(store, 'eurusd', 'm1', s('2026-09-26T00:00:00Z'), null);
  await store.put('eurusd', 'm1', s('2026-09-27T00:00:00Z'), null, {
    builtAt: s('2026-09-27T05:15:00Z'),
    final: false,
  });
  const latest = await series.read({ symbol: 'FX:EURUSD', tf: '1m', count: 500, latest: true });
  assert.equal(latest.bars.length, 500);
  assert.equal(iso(latest.bars.at(-1)!.t), '2026-09-24T23:59:00.000Z');
  assert.equal(latest.more, 'cached');
  // Without `latest`, paging stops at the uncovered Friday as before.
  const paged = await series.read({ symbol: 'FX:EURUSD', tf: '1m', count: 500 });
  assert.equal(paged.bars.length, 0);
  assert.deepEqual(paged.gap, { from: s('2026-09-25T00:00:00Z'), to: s('2026-09-26T00:00:00Z'), source: 'dukascopy' });
});

test('due: the provisional buckets a read shows that need their re-fetch, per tier read', async () => {
  const opened = s('2026-08-31T15:00:00Z');
  let now = opened;
  const cache = tempDir();
  const clock = () => now * 1000;
  const store = new DukascopyStore(cache, { now: clock, manifestDelayMs: 20 });
  const tv = new TvStore(cache, { now: clock, flushDelayMs: 20 });
  const series = new Series({ store, tv, now: clock });
  const aug = s('2026-08-01T00:00:00Z');
  const day = s('2026-08-31T00:00:00Z');
  const put = (tier: 'm1' | 'h1', start: number, bars: Bar[] | null, builtAt: number) =>
    store.put('eurusd', tier, start, bars ? bucketBody(tier, start, bars) : null, { builtAt, final: false });
  // Stored while open: the August month and the last August day, up to 15:00.
  await put('h1', aug, bucketBars('h1', aug, { to: opened }), opened);
  await put('m1', day, bucketBars('m1', day, { to: opened }), opened);
  // An m1 day fetched right after it opened: empty, and no tier has bars for it at 1m.
  const early = s('2026-08-28T00:00:30Z');
  await put('m1', s('2026-08-28T00:00:00Z'), null, early);
  assert.deepEqual(series.due('FX:EURUSD', '1h', day, day + DAY - 1), [], 'not due while open');

  now = s('2026-09-02T10:00:00Z');
  // The September day open now is provisional but active: reads near now refresh it.
  await put('m1', s('2026-09-02T00:00:00Z'), bucketBars('m1', s('2026-09-02T00:00:00Z'), { to: now }), now - 120);
  const end = s('2026-09-02T23:59:59Z');
  const due = (tf: '1m' | '1h', from: number, to: number) =>
    series.due('FX:EURUSD', tf, from, to).map((d) => `${d.tier} ${iso(d.start).slice(0, 10)}`);
  assert.deepEqual(due('1h', day, day + DAY - 1), ['h1 2026-08-01']);
  assert.deepEqual(due('1m', day, day + DAY - 1), ['m1 2026-08-31']);
  assert.deepEqual(due('1m', s('2026-08-27T00:00:00Z'), end), ['m1 2026-08-31', 'm1 2026-08-28']);

  // Where TradingView covers the time, its bars are shown: nothing of Dukascopy's is due there.
  const hours = tvBars(day, day + DAY);
  tv.record('FX:EURUSD', '1h', hours, [day, day + DAY]);
  assert.deepEqual(due('1h', day, day + DAY - 1), []);
  await store.close();
});

test('ranges cap at the limit keeping the newest bars; the CSV export streams with a source column', async () => {
  const { cache, store, series } = harness();
  for (let d = s('2024-03-04T00:00:00Z'); d < s('2024-03-09T00:00:00Z'); d += DAY) {
    await putBucket(store, 'eurusd', 'm1', d, bucketBars('m1', d));
  }
  const from = s('2024-03-04T00:00:00Z');
  const to = s('2024-03-08T23:59:59Z');
  const capped = await series.read({ symbol: 'EURUSD', tf: '1m', from, to, limit: 1000 });
  assert.equal(capped.bars.length, 1000);
  assert.equal(capped.truncated, true);
  assert.equal(capped.bars.at(-1)!.t, s('2024-03-08T20:59:00Z'));
  const whole = await series.read({ symbol: 'EURUSD', tf: '5m', from, to });
  assert.equal(whole.truncated, false);
  assert.equal(whole.bars.length, 5 * 24 * 12 - 3 * 12);

  const file = path.join(cache, 'out.csv');
  const summary = await series.exportCsv({ symbol: 'EURUSD', tf: '5m', from, to, path: file });
  const lines = fs.readFileSync(file, 'utf8').trim().split('\n');
  assert.equal(lines[0], 'time,open,high,low,close,volume,source');
  assert.equal(lines.length - 1, whole.bars.length);
  assert.equal(summary.count, whole.bars.length);
  assert.match(lines[1]!, /^2024-03-04T00:00:00Z,[\d.]+,[\d.]+,[\d.]+,[\d.]+,[\d.]+,dukascopy$/);
  assert.equal(summary.from, whole.bars[0]!.t);
  assert.equal(summary.lastClose, whole.bars.at(-1)!.c);
  assert.equal(summary.high, Math.max(...whole.bars.map((b) => b.h)));
  assert.ok(summary.closes.length <= 160 && summary.closes.length > 100);
  assert.equal(summary.lastRows.length, 10);
  assert.deepEqual(summary.spans, [{ source: 'dukascopy', from: summary.from, to: summary.to, count: summary.count }]);
  assert.deepEqual(summary.missing, []);

  // Capped export keeps the newest rows.
  const small = await series.exportCsv({ symbol: 'EURUSD', tf: '5m', from, to, path: file, limit: 100 });
  assert.equal(small.count, 100);
  assert.equal(small.truncated, true);
  assert.equal(small.to, whole.bars.at(-1)!.t);
});
