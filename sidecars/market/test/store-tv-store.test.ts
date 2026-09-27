// TradingView bars and coverage on disk.

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, test } from 'node:test';

import { type Bar } from '../src/protocol.ts';
import { TvStore, barEnd, coverOf, fileKey, intervalsOf } from '../src/store/tv-store.ts';
import { cleanup, s, tempDir } from './store-helpers.ts';

afterEach(cleanup);

const bar = (t: number, c = 1.1): Bar => ({ t, o: c, h: c, l: c, c, v: 1 });
const H = 3600;

test('pages splice in by time, live bars replace or append, and coverage follows closed bars', async () => {
  const cache = tempDir();
  const tv = new TvStore(cache, { flushDelayMs: 10_000 });
  const t0 = s('2024-05-06T00:00:00Z');
  tv.record('FX:EURUSD', '1h', [bar(t0 + 5 * H), bar(t0 + 6 * H), bar(t0 + 7 * H)], [t0 + 5 * H, t0 + 8 * H]);
  // An older page, overlapping one stored bar: the page wins for its span.
  tv.record('FX:EURUSD', '1h', [bar(t0 + 3 * H), bar(t0 + 4 * H), bar(t0 + 5 * H, 2)], [t0 + 3 * H, t0 + 6 * H]);
  assert.deepEqual(
    tv.bars('FX:EURUSD', '1h').map((b) => [(b.t - t0) / H, b.c]),
    [
      [3, 1.1],
      [4, 1.1],
      [5, 2],
      [6, 1.1],
      [7, 1.1],
    ],
  );
  tv.live('FX:EURUSD', '1h', bar(t0 + 7 * H, 3));
  tv.live('FX:EURUSD', '1h', bar(t0 + 8 * H, 4));
  tv.cover('FX:EURUSD', '1h', t0 + 3 * H, t0 + 8 * H);
  const bars = tv.bars('FX:EURUSD', '1h');
  assert.equal(bars.length, 6);
  assert.equal(bars[4]!.c, 3);
  assert.equal(bars[5]!.c, 4);
  assert.deepEqual(tv.coverage('fx:eurusd', '1h').intervals.toJSON(), [[t0 + 3 * H, t0 + 8 * H]]);

  // Aliases and infos: the chart resolved EURUSD to FX:EURUSD.
  tv.setAlias('eurusd', 'FX:EURUSD');
  tv.setInfo('FX:EURUSD', { symbol: 'FX:EURUSD', pricescale: 100000 });
  assert.equal(tv.canonical('EURUSD'), 'FX:EURUSD');
  assert.equal(tv.bars('EURUSD', '1h').length, 6);
  assert.equal(tv.info('eurusd')?.pricescale, 100000);

  await tv.flush();
  const again = new TvStore(cache);
  assert.equal(again.bars('EURUSD', '1h').length, 6);
  assert.deepEqual(again.coverage('FX:EURUSD', '1h').intervals.toJSON(), [[t0 + 3 * H, t0 + 8 * H]]);
  assert.deepEqual(
    again.series().map((sr) => [sr.symbol, sr.tf, sr.count]),
    [['FX:EURUSD', '1h', 6]],
  );
  assert.ok(fs.existsSync(path.join(cache, 'tradingview', `${fileKey('FX:EURUSD', '1h')}.json`)));
});

test('the index never claims bars that were not saved, and a flush saves bars before the index', async () => {
  const cache = tempDir();
  const tv = new TvStore(cache, { flushDelayMs: 60_000 });
  const t0 = s('2024-05-06T00:00:00Z');
  tv.record('OANDA:XAUUSD', '1h', [bar(t0), bar(t0 + H)], [t0, t0 + 2 * H]);
  await tv.flush();
  tv.record('OANDA:XAUUSD', '1h', [bar(t0 + 2 * H), bar(t0 + 3 * H)], [t0 + 2 * H, t0 + 4 * H]);
  // Not flushed: a reader of the disk sees the old coverage with the old bars.
  const disk = new TvStore(cache);
  assert.deepEqual(disk.coverage('OANDA:XAUUSD', '1h').intervals.toJSON(), [[t0, t0 + 2 * H]]);
  assert.equal(disk.bars('OANDA:XAUUSD', '1h').length, 2);
  await tv.flush();
  const later = new TvStore(cache);
  assert.deepEqual(later.coverage('OANDA:XAUUSD', '1h').intervals.toJSON(), [[t0, t0 + 4 * H]]);
  assert.equal(later.bars('OANDA:XAUUSD', '1h').length, 4);
  // Removing a symbol frees its files.
  assert.ok((await later.remove('OANDA:XAUUSD')) > 0);
  assert.equal(new TvStore(cache).series().length, 0);
});

test('coverage of a snapshot ends at its last closed bar; rebuilt coverage splits at real gaps', () => {
  const t0 = s('2024-05-08T10:00:00Z');
  const bars = [bar(t0), bar(t0 + H), bar(t0 + 2 * H)];
  assert.deepEqual(coverOf(bars, '1h', t0 + 2 * H + 1800), [t0, t0 + 2 * H]);
  assert.deepEqual(coverOf(bars, '1h', t0 + 3 * H), [t0, t0 + 3 * H]);
  assert.equal(barEnd(s('2024-01-01T00:00:00Z'), '1M'), s('2024-02-01T00:00:00Z'));
  // Friday 20:00 to Sunday 22:00 is a weekend, not a gap; five hours on a Wednesday is.
  const fri = s('2024-05-10T20:00:00Z');
  const sun = s('2024-05-12T22:00:00Z');
  assert.deepEqual(intervalsOf([bar(fri), bar(sun)], '1h').toJSON(), [[fri, sun + H]]);
  assert.equal(intervalsOf([bar(fri), bar(sun)], '1h', false).count, 2);
  assert.equal(intervalsOf([bar(t0), bar(t0 + 5 * H)], '1h').count, 2);
  assert.equal(fileKey('FX:EURUSD', '1M'), 'FX_EURUSD_1mo');
});
