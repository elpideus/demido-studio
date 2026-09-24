import assert from 'node:assert/strict';
import { test } from 'node:test';

import { resolveInstrument, weekly } from '../src/dukascopy.ts';
import { stitch } from '../src/candles.ts';
import { parseDate } from '../src/timeframes.ts';

test('TradingView symbols map to Dukascopy instruments', () => {
  assert.equal(resolveInstrument('FX:EURUSD'), 'eurusd');
  assert.equal(resolveInstrument('OANDA:XAUUSD'), 'xauusd');
  assert.equal(resolveInstrument('BINANCE:BTCUSDT'), 'btcusd');
  assert.equal(resolveInstrument('NASDAQ:AAPL'), 'aaplususd');
  assert.equal(resolveInstrument('SP:SPX'), 'usa500idxusd');
  assert.equal(resolveInstrument('US500'), 'usa500idxusd');
  assert.equal(resolveInstrument('TVC:GOLD'), 'xauusd');
  assert.equal(resolveInstrument('NSE:RELIANCE'), null);
});

test('stitching never overlaps the newer source', () => {
  const bar = (t: number) => ({ t, o: 1, h: 1, l: 1, c: 1, v: 0 });
  const out = stitch([bar(1), bar(2), bar(3)], [bar(3), bar(4)]);
  assert.deepEqual(out.map((b) => b.t), [1, 2, 3, 4]);
});

test('daily candles roll up into Monday weeks', () => {
  const day = (iso: string, c: number) => ({ t: Date.parse(iso) / 1000, o: c, h: c + 1, l: c - 1, c, v: 1 });
  const weeks = weekly([day('2026-09-14T00:00:00Z', 10), day('2026-09-16T00:00:00Z', 12), day('2026-09-21T00:00:00Z', 9)]);
  assert.equal(weeks.length, 2);
  assert.equal(weeks[0]!.c, 12);
  assert.equal(weeks[0]!.h, 13);
  assert.equal(weeks[0]!.v, 2);
});

test('dates parse to seconds, end of day inclusive', () => {
  assert.equal(parseDate('2026-01-01'), 1767225600);
  assert.equal(parseDate('2026-01-01', true), 1767225600 + 86399);
  assert.equal(parseDate(undefined), null);
});
