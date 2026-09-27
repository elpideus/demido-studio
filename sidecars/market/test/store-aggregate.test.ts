import assert from 'node:assert/strict';
import { test } from 'node:test';

import { type Bar } from '../src/protocol.ts';
import {
  aggregate,
  bucketOf,
  bucketSpan,
  dropPartial,
  isCryptoInstrument,
  weekModeFor,
  wholeBuckets,
  type AggregateOptions,
} from '../src/store/aggregate.ts';
import { IntervalSet } from '../src/store/intervals.ts';
import { TIMEFRAMES, type Timeframe } from '../src/timeframes.ts';

const s = (iso: string) => Date.parse(iso) / 1000;
const SESSION: AggregateOptions = { week: 'session' };
const UTC: AggregateOptions = { week: 'utc' };
const bar = (t: number, c: number, v = 1): Bar => ({ t, o: c, h: c + 0.5, l: c - 0.5, c, v });
/** Native bars every `step` seconds over [from, to), close = index. */
const series = (from: number, to: number, step: number, first = 0): Bar[] => {
  const out: Bar[] = [];
  for (let t = from, i = first; t < to; t += step, i += 1) out.push(bar(t, i));
  return out;
};
const ALL: Timeframe[] = ['1m', '5m', '15m', '30m', '1h', '4h', '1d', '1w', '1M'];

test('intraday timeframes align to UTC multiples of their length', () => {
  const t = s('2024-03-05T13:47:30Z');
  assert.equal(bucketOf(t, '1m', SESSION), s('2024-03-05T13:47:00Z'));
  assert.equal(bucketOf(t, '5m', SESSION), s('2024-03-05T13:45:00Z'));
  assert.equal(bucketOf(t, '15m', UTC), s('2024-03-05T13:45:00Z'));
  assert.equal(bucketOf(t, '30m', UTC), s('2024-03-05T13:30:00Z'));
  assert.equal(bucketOf(t, '1h', UTC), s('2024-03-05T13:00:00Z'));
  assert.equal(bucketOf(t, '4h', SESSION), s('2024-03-05T12:00:00Z'));
  const bars = aggregate(series(s('2024-03-05T12:00:00Z'), s('2024-03-05T14:00:00Z'), 60), '1h', SESSION);
  assert.deepEqual(bars, [
    { t: s('2024-03-05T12:00:00Z'), o: 0, h: 59.5, l: -0.5, c: 59, v: 60 },
    { t: s('2024-03-05T13:00:00Z'), o: 60, h: 119.5, l: 59.5, c: 119, v: 60 },
  ]);
});

test('daily bars: Sunday folds into Monday for session markets, plain days for 24/7', () => {
  const sun = s('2024-03-03T00:00:00Z');
  const mon = s('2024-03-04T00:00:00Z');
  assert.equal(bucketOf(sun + 22 * 3600, '1d', SESSION), mon);
  assert.equal(bucketOf(sun + 22 * 3600, '1d', UTC), sun);
  assert.deepEqual(bucketSpan(mon, '1d', SESSION), [sun, mon + 86400]);
  assert.deepEqual(bucketSpan(mon + 86400, '1d', SESSION), [mon + 86400, mon + 2 * 86400]);
  assert.deepEqual(bucketSpan(mon, '1d', UTC), [mon, mon + 86400]);
  // Dukascopy d1 has Sunday bars for FX: folded into Monday's.
  const daily = [
    bar(s('2024-03-01T00:00:00Z'), 10, 5),
    bar(sun, 11, 1),
    { t: mon, o: 11.2, h: 13, l: 10, c: 12, v: 7 },
    bar(s('2024-03-05T00:00:00Z'), 13, 6),
  ];
  assert.deepEqual(aggregate(daily, '1d', SESSION), [
    bar(s('2024-03-01T00:00:00Z'), 10, 5),
    { t: mon, o: 11, h: 13, l: 10, c: 12, v: 8 },
    bar(s('2024-03-05T00:00:00Z'), 13, 6),
  ]);
  assert.equal(aggregate(daily, '1d', UTC).length, 4);
});

test('weeks: Sunday to Sunday labelled Monday for session markets, Monday weeks for 24/7', () => {
  const mon = s('2024-03-04T00:00:00Z');
  for (const t of [s('2024-03-03T21:00:00Z'), mon, s('2024-03-08T21:59:00Z'), s('2024-03-09T12:00:00Z')]) {
    assert.equal(bucketOf(t, '1w', SESSION), mon, new Date(t * 1000).toISOString());
  }
  assert.equal(bucketOf(s('2024-03-10T00:00:00Z'), '1w', SESSION), s('2024-03-11T00:00:00Z'));
  assert.deepEqual(bucketSpan(mon, '1w', SESSION), [s('2024-03-03T00:00:00Z'), s('2024-03-10T00:00:00Z')]);
  assert.equal(bucketOf(s('2024-03-03T21:00:00Z'), '1w', UTC), s('2024-02-26T00:00:00Z'));
  assert.equal(bucketOf(s('2024-03-10T23:59:59Z'), '1w', UTC), mon);
  assert.deepEqual(bucketSpan(mon, '1w', UTC), [mon, s('2024-03-11T00:00:00Z')]);
});

test('months: calendar months for 24/7; session months hand a closing Sunday to the next month', () => {
  // June 2024 ends on Sunday the 30th, which opens July's first session.
  const jun30 = s('2024-06-30T22:00:00Z');
  assert.equal(bucketOf(jun30, '1M', SESSION), s('2024-07-01T00:00:00Z'));
  assert.equal(bucketOf(jun30, '1M', UTC), s('2024-06-01T00:00:00Z'));
  assert.deepEqual(bucketSpan(s('2024-07-01T00:00:00Z'), '1M', SESSION), [
    s('2024-06-30T00:00:00Z'),
    s('2024-08-01T00:00:00Z'),
  ]);
  assert.deepEqual(bucketSpan(s('2024-06-01T00:00:00Z'), '1M', SESSION), [
    s('2024-06-01T00:00:00Z'),
    s('2024-06-30T00:00:00Z'),
  ]);
  // September 2024 starts on a Sunday: that Sunday is still September's.
  assert.equal(bucketOf(s('2024-09-01T21:00:00Z'), '1M', SESSION), s('2024-09-01T00:00:00Z'));
  assert.equal(bucketOf(s('2024-08-31T12:00:00Z'), '1M', SESSION), s('2024-08-01T00:00:00Z'));
  assert.deepEqual(bucketSpan(s('2024-02-01T00:00:00Z'), '1M', UTC), [
    s('2024-02-01T00:00:00Z'),
    s('2024-03-01T00:00:00Z'),
  ]);
  const bars = aggregate(
    [bar(s('2024-06-28T00:00:00Z'), 1), bar(jun30, 2), bar(s('2024-07-01T00:00:00Z'), 3)],
    '1M',
    SESSION,
  );
  assert.deepEqual(
    bars.map((b) => [b.t, b.o, b.c]),
    [
      [s('2024-06-01T00:00:00Z'), 1, 1],
      [s('2024-07-01T00:00:00Z'), 2, 3],
    ],
  );
});

test('every label spans the instants that map to it, for every timeframe and week mode', () => {
  let t = s('2023-12-20T00:00:00Z');
  const end = s('2024-03-15T00:00:00Z');
  for (; t < end; t += 3371) {
    for (const opts of [SESSION, UTC]) {
      for (const tf of ALL) {
        const label = bucketOf(t, tf, opts);
        const [from, to] = bucketSpan(label, tf, opts);
        assert.ok(from <= t && t < to, `${tf} ${opts.week} ${new Date(t * 1000).toISOString()}`);
        assert.equal(bucketOf(from, tf, opts), label);
        assert.equal(bucketOf(to - 1, tf, opts), label);
        assert.notEqual(bucketOf(to, tf, opts), label);
      }
    }
  }
});

test('a week straddling a month end, h1 on one side and m1 on the other, aggregates once', () => {
  // Session week of Monday 2024-04-29 runs Sunday 04-28 -> Sunday 05-05. April from h1, May from m1.
  const h1 = series(s('2024-04-28T21:00:00Z'), s('2024-05-01T00:00:00Z'), 3600, 0);
  const m1 = series(s('2024-05-01T00:00:00Z'), s('2024-05-03T21:00:00Z'), 60, 1000);
  const native = [...h1, ...m1];
  const weeks = aggregate(native, '1w', SESSION);
  assert.equal(weeks.length, 1);
  assert.equal(weeks[0]!.t, s('2024-04-29T00:00:00Z'));
  assert.equal(weeks[0]!.o, 0);
  assert.equal(weeks[0]!.c, m1[m1.length - 1]!.c);
  assert.equal(weeks[0]!.v, h1.length + m1.length);
  assert.equal(weeks[0]!.h, m1[m1.length - 1]!.h);
  assert.equal(weeks[0]!.l, -0.5);
  const months = aggregate(native, '1M', SESSION);
  assert.deepEqual(
    months.map((b) => [b.t, b.v]),
    [
      [s('2024-04-01T00:00:00Z'), h1.length],
      [s('2024-05-01T00:00:00Z'), m1.length],
    ],
  );
  const fourHours = aggregate(native, '4h', SESSION);
  assert.equal(fourHours[0]!.t, s('2024-04-28T20:00:00Z'));
  assert.equal(fourHours.find((b) => b.t === s('2024-05-01T00:00:00Z'))!.v, 240);
});

test('volumes are summed and rounded to 6 decimals', () => {
  const bars = aggregate([bar(0, 1, 0.1), bar(60, 1, 0.2)], '5m', UTC);
  assert.equal(bars[0]!.v, 0.3);
  assert.deepEqual(aggregate([], '1d', SESSION), []);
});

test('partial edges: coverage snaps inward to whole buckets, the forming bucket counts', () => {
  // Covered from Wednesday noon to Tuesday 10:00 two weeks later.
  const covered = new IntervalSet([[s('2024-03-06T12:00:00Z'), s('2024-03-19T10:00:00Z')]]);
  assert.deepEqual(wholeBuckets(covered, '1w', SESSION).toJSON(), [
    [s('2024-03-10T00:00:00Z'), s('2024-03-17T00:00:00Z')],
  ]);
  assert.deepEqual(wholeBuckets(covered, '1d', SESSION).toJSON(), [
    [s('2024-03-07T00:00:00Z'), s('2024-03-19T00:00:00Z')],
  ]);
  assert.deepEqual(wholeBuckets(covered, '1h', UTC).toJSON(), covered.toJSON());
  // The bucket containing now is kept when covered from its start to now.
  const now = s('2024-03-19T10:00:00Z');
  assert.deepEqual(wholeBuckets(covered, '1w', SESSION, now).toJSON(), [[s('2024-03-10T00:00:00Z'), now]]);
  assert.deepEqual(wholeBuckets(covered, '1d', SESSION, now).toJSON(), [[s('2024-03-07T00:00:00Z'), now]]);
  // ...but not when the coverage starts inside it.
  const late = new IntervalSet([[s('2024-03-19T06:00:00Z'), now]]);
  assert.deepEqual(wholeBuckets(late, '1d', SESSION, now).toJSON(), []);

  const daily = series(s('2024-03-06T00:00:00Z'), s('2024-03-20T00:00:00Z'), 86400);
  const native = daily.filter((b) => covered.contains(b.t) || b.t === s('2024-03-06T00:00:00Z'));
  const weeks = aggregate(native, '1w', SESSION);
  assert.deepEqual(
    dropPartial(weeks, '1w', SESSION, covered, now).map((b) => b.t),
    [s('2024-03-11T00:00:00Z'), s('2024-03-18T00:00:00Z')],
  );
  assert.deepEqual(
    dropPartial(weeks, '1w', SESSION, covered).map((b) => b.t),
    [s('2024-03-11T00:00:00Z')],
  );
});

test('week mode from the data: Saturday bars mean 24/7, two dry weekends mean sessions', () => {
  const days = (from: string, n: number, skip: (weekday: number) => boolean) =>
    series(s(from), s(from) + n * 86400, 86400).filter((b) => !skip(new Date(b.t * 1000).getUTCDay()));
  assert.equal(
    weekModeFor(
      'eurusd',
      days('2024-03-01T00:00:00Z', 30, () => false),
    ),
    'utc',
  );
  assert.equal(
    weekModeFor(
      'btcusd',
      days('2024-03-01T00:00:00Z', 30, (d) => d === 6),
    ),
    'session',
  );
  // One stray Saturday print among four dry Saturdays does not make a market 24/7.
  const stray = days('2024-03-01T00:00:00Z', 30, (d) => d === 6);
  stray.push(bar(s('2024-03-09T01:00:00Z'), 1));
  stray.sort((a, b) => a.t - b.t);
  assert.equal(weekModeFor('eurusd', stray), 'session');
  // Too short to judge: fall back on the name.
  const short = days('2024-03-04T00:00:00Z', 4, () => false);
  assert.equal(weekModeFor('btcusd', short), 'utc');
  assert.equal(weekModeFor('eurusd', short), 'session');
  assert.equal(weekModeFor('btcusd'), 'utc');
  assert.equal(weekModeFor('ethchf', []), 'utc');
  assert.equal(weekModeFor('xauusd', null), 'session');
  assert.equal(weekModeFor('usa500idxusd'), 'session');
});

test('crypto detection by name, with a fallback on the key', () => {
  for (const k of ['btcusd', 'ethusd', 'ltceur', 'xlmchf', 'bchgbp', 'adausd', 'uniusd']) {
    assert.equal(isCryptoInstrument(k), true, k);
  }
  for (const k of ['eurusd', 'xauusd', 'usa500idxusd', 'aaplususd', 'sarjpy', 'bitoususd']) {
    assert.equal(isCryptoInstrument(k), false, k);
  }
  // Not in the metadata: judged by the key.
  assert.equal(isCryptoInstrument('solusd'), true);
  assert.equal(isCryptoInstrument('dogeeur'), true);
  assert.equal(isCryptoInstrument('foousd'), false);
  assert.equal(TIMEFRAMES['1w'].seconds, 604800);
});
