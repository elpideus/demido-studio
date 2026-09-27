import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  PLACEHOLDER_START,
  TIERS,
  activeUrl,
  allInstruments,
  bucketKey,
  bucketStart,
  bucketUrl,
  bucketsIn,
  completedUrl,
  instrumentCode,
  instrumentMeta,
  isActive,
  isFinalBuild,
  nextBucket,
  parseBucketKey,
  tierStart,
  tierStartFromMeta,
  type InstrumentMeta,
} from '../src/store/buckets.ts';

const s = (iso: string) => Date.parse(iso) / 1000;

test('bucket starts and ends per tier, including leap days and year ends', () => {
  const t = s('2024-02-29T13:45:10Z');
  assert.equal(bucketStart('m1', t), s('2024-02-29T00:00:00Z'));
  assert.equal(nextBucket('m1', t), s('2024-03-01T00:00:00Z'));
  assert.equal(bucketStart('h1', t), s('2024-02-01T00:00:00Z'));
  assert.equal(nextBucket('h1', t), s('2024-03-01T00:00:00Z'));
  assert.equal(bucketStart('d1', t), s('2024-01-01T00:00:00Z'));
  assert.equal(nextBucket('d1', t), s('2025-01-01T00:00:00Z'));
  assert.equal(nextBucket('h1', s('2023-12-31T23:59:59Z')), s('2024-01-01T00:00:00Z'));
  // A bucket start is its own start; the instant before belongs to the previous bucket.
  for (const tier of TIERS) {
    const start = bucketStart(tier, t);
    assert.equal(bucketStart(tier, start), start);
    assert.equal(bucketStart(tier, start - 1) < start, true);
    assert.equal(nextBucket(tier, start - 1), start);
  }
  // Before 1970 too (EURUSD daily data starts in 1973, some start earlier).
  assert.equal(bucketStart('m1', s('1969-12-31T12:00:00Z')), s('1969-12-31T00:00:00Z'));
  assert.equal(bucketStart('d1', s('1965-06-01T00:00:00Z')), s('1965-01-01T00:00:00Z'));
});

test('bucketsIn lists every bucket overlapping [from, to)', () => {
  assert.deepEqual(bucketsIn('m1', s('2024-03-01T12:00:00Z'), s('2024-03-03T00:00:00Z')), [
    s('2024-03-01T00:00:00Z'),
    s('2024-03-02T00:00:00Z'),
  ]);
  assert.deepEqual(bucketsIn('m1', s('2024-03-01T12:00:00Z'), s('2024-03-03T00:00:01Z')).length, 3);
  assert.deepEqual(bucketsIn('h1', s('2023-11-15T00:00:00Z'), s('2024-02-01T00:00:00Z')), [
    s('2023-11-01T00:00:00Z'),
    s('2023-12-01T00:00:00Z'),
    s('2024-01-01T00:00:00Z'),
  ]);
  assert.deepEqual(bucketsIn('d1', s('2020-05-01T00:00:00Z'), s('2022-01-01T00:00:00Z')), [
    s('2020-01-01T00:00:00Z'),
    s('2021-01-01T00:00:00Z'),
  ]);
  assert.deepEqual(bucketsIn('m1', 100, 100), []);
  assert.deepEqual(bucketsIn('m1', 200, 100), []);
});

test('bucket keys round-trip and reject impossible dates', () => {
  const start = s('2024-03-05T00:00:00Z');
  assert.equal(bucketKey('m1', start), '2024-03-05');
  assert.equal(bucketKey('h1', bucketStart('h1', start)), '2024-03');
  assert.equal(bucketKey('d1', bucketStart('d1', start)), '2024');
  for (const tier of TIERS) {
    for (let t = s('2019-12-25T00:00:00Z'); t < s('2021-02-10T00:00:00Z'); t += 86400 * 3) {
      const b = bucketStart(tier, t);
      assert.equal(parseBucketKey(tier, bucketKey(tier, b)), b);
    }
  }
  assert.equal(parseBucketKey('m1', '2024-02-30'), null);
  assert.equal(parseBucketKey('m1', '2023-02-29'), null);
  assert.equal(parseBucketKey('m1', '2024-02-29'), s('2024-02-29T00:00:00Z'));
  assert.equal(parseBucketKey('h1', '2024-13'), null);
  assert.equal(parseBucketKey('h1', '2024-3'), null);
  assert.equal(parseBucketKey('d1', '2024-03'), null);
  assert.equal(parseBucketKey('m1', '2024-03'), null);
  assert.equal(parseBucketKey('d1', 'x'), null);
});

test('URLs follow the API: 1-based unpadded months and days, ?from= for the open bucket', () => {
  const day = s('2024-03-05T00:00:00Z');
  assert.equal(
    completedUrl('EUR-USD', 'm1', day),
    'https://jetta.dukascopy.com/v1/candles/minute/EUR-USD/BID/2024/3/5',
  );
  assert.equal(
    completedUrl('EUR-USD', 'h1', bucketStart('h1', day)),
    'https://jetta.dukascopy.com/v1/candles/hour/EUR-USD/BID/2024/3',
  );
  assert.equal(
    completedUrl('EUR-USD', 'd1', bucketStart('d1', day)),
    'https://jetta.dukascopy.com/v1/candles/day/EUR-USD/BID/2024',
  );
  assert.equal(
    activeUrl('EUR-USD', 'm1', day),
    `https://jetta.dukascopy.com/v1/candles/minute/EUR-USD/BID?from=${day * 1000}`,
  );
  assert.equal(bucketUrl('EUR-USD', 'm1', day, day + 3600), activeUrl('EUR-USD', 'm1', day));
  assert.equal(bucketUrl('EUR-USD', 'm1', day, day + 86400), completedUrl('EUR-USD', 'm1', day));
  assert.equal(instrumentCode('eurusd'), 'EUR-USD');
  assert.equal(instrumentCode('nope'), null);
  assert.equal(instrumentMeta('constructor'), null);
});

test('active and final builds', () => {
  const day = s('2024-03-05T00:00:00Z');
  assert.equal(isActive('m1', day, day - 1), false);
  assert.equal(isActive('m1', day, day), true);
  assert.equal(isActive('m1', day, day + 86399), true);
  assert.equal(isActive('m1', day, day + 86400), false);
  // Final once built an hour after the close; CloudFront can serve a copy built a second after it.
  assert.equal(isFinalBuild('m1', day, day + 86400 + 1), false);
  assert.equal(isFinalBuild('m1', day, day + 86400 + 3599), false);
  assert.equal(isFinalBuild('m1', day, day + 86400 + 3600), true);
  const month = bucketStart('h1', day);
  assert.equal(isFinalBuild('h1', month, s('2024-04-01T01:00:00Z')), true);
  assert.equal(isFinalBuild('h1', month, s('2024-04-01T00:59:59Z')), false);
});

const meta = (patch: Partial<InstrumentMeta>): InstrumentMeta => ({
  name: 'X/Y',
  code: 'X-Y',
  description: '',
  startHourForTicks: '2010-01-04T10:00:00.123Z',
  startDayForMinuteCandles: '2010-01-04T10:00:00.000Z',
  startMonthForHourlyCandles: '2010-01-04T10:00:00.000Z',
  startYearForDailyCandles: '2005-01-01T00:00:00.000Z',
  ...patch,
});

test('tier starts: the minute start is trusted only when plausible', () => {
  const h1 = s('2010-01-04T10:00:00Z');
  assert.equal(tierStartFromMeta(meta({}), 'm1'), h1);
  assert.equal(tierStartFromMeta(meta({}), 'h1'), h1);
  assert.equal(tierStartFromMeta(meta({}), 'd1'), s('2005-01-01T00:00:00Z'));
  // Placeholder: the later of the hourly start and the first tick (floored to its minute).
  assert.equal(
    tierStartFromMeta(
      meta({ startDayForMinuteCandles: PLACEHOLDER_START, startHourForTicks: '2010-01-04T10:30:12.5Z' }),
      'm1',
    ),
    s('2010-01-04T10:30:00Z'),
  );
  assert.equal(
    tierStartFromMeta(
      meta({ startDayForMinuteCandles: PLACEHOLDER_START, startHourForTicks: PLACEHOLDER_START }),
      'm1',
    ),
    h1,
  );
  assert.equal(
    tierStartFromMeta(
      meta({ startDayForMinuteCandles: PLACEHOLDER_START, startHourForTicks: '2008-01-01T00:00:00Z' }),
      'm1',
    ),
    h1,
  );
  // Seconds in the minute start (the bogus 2015 US stock starts): not trusted.
  assert.equal(tierStartFromMeta(meta({ startDayForMinuteCandles: '2009-08-26T16:36:38.000Z' }), 'm1'), h1);
  // Whole minutes but long before the hourly data: not trusted either.
  assert.equal(tierStartFromMeta(meta({ startDayForMinuteCandles: '2009-12-01T00:00:00.000Z' }), 'm1'), h1);
  // Up to a day before the hourly start (indices start minutes before their first hour): trusted.
  assert.equal(
    tierStartFromMeta(meta({ startDayForMinuteCandles: '2010-01-03T21:55:00.000Z' }), 'm1'),
    s('2010-01-03T21:55:00Z'),
  );
  // Later minute starts are trusted as they are.
  assert.equal(
    tierStartFromMeta(meta({ startDayForMinuteCandles: '2012-06-01T00:00:00.000Z' }), 'm1'),
    s('2012-06-01T00:00:00Z'),
  );
});

test('tier starts on real metadata: EURUSD, a bogus US stock, an index, a placeholder', () => {
  assert.equal(tierStart('eurusd', 'd1'), s('1973-03-01T00:00:00Z'));
  assert.equal(tierStart('eurusd', 'h1'), s('2003-05-04T19:00:00Z'));
  assert.equal(tierStart('eurusd', 'm1'), s('2003-05-04T19:00:00Z'));
  // AXP.US: minute start 2015-08-26T16:36:38 (seconds, 2 years early) -> the later of the hourly
  // start (13:00) and the first tick (13:30:00.157).
  assert.equal(tierStart('axpususd', 'm1'), s('2017-11-02T13:30:00Z'));
  // CHE.IDX: a genuine minute start well before the first tick.
  assert.equal(tierStart('cheidxchf', 'm1'), s('2011-09-18T21:55:00Z'));
  // AUS.US: placeholder minute start -> the first tick's minute.
  assert.equal(tierStart('aususd', 'm1'), s('2017-05-25T14:30:00Z'));
  assert.equal(tierStart('nope', 'm1'), null);
});

test('tier starts over the full instrumentMetaData: no minute start before hourly start - 1 day', () => {
  const keys = allInstruments();
  assert.ok(keys.length > 1000, `expected the full metadata, got ${keys.length}`);
  let placeholders = 0;
  for (const key of keys) {
    const m = instrumentMeta(key)!;
    if (m.startDayForMinuteCandles === PLACEHOLDER_START) placeholders += 1;
    const h1 = tierStart(key, 'h1');
    const d1 = tierStart(key, 'd1');
    const m1 = tierStart(key, 'm1');
    assert.ok(h1 !== null && d1 !== null && m1 !== null, `${key} has every start`);
    assert.ok(m1 >= h1 - 86400, `${key}: minute start ${m1} before hourly start ${h1} - 1 day`);
    assert.equal(m1 % 60, 0, `${key}: minute start ${m1} is whole minutes`);
    assert.notEqual(m1, s(PLACEHOLDER_START), `${key}: minute start is the placeholder`);
    assert.ok(Number.isFinite(m1) && Number.isFinite(h1) && Number.isFinite(d1));
  }
  assert.ok(placeholders > 400, 'the metadata still has its placeholders, so the rule is exercised');
});
