import assert from 'node:assert/strict';
import { test } from 'node:test';

import { historyInstrument, isStockCfd, resolveInstrument, weekly } from '../src/dukascopy.ts';
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

test('stock and ETF CFDs are not a history source; FX, indices, metals and crypto are', () => {
  assert.equal(isStockCfd('aaplususd'), true);
  assert.equal(historyInstrument('NASDAQ:AAPL'), null);
  assert.equal(historyInstrument('FX:EURUSD'), 'eurusd');
  assert.equal(historyInstrument('US500'), 'usa500idxusd');
  assert.equal(historyInstrument('OANDA:XAUUSD'), 'xauusd');
  assert.equal(historyInstrument('BINANCE:BTCUSDT'), 'btcusd');
  assert.equal(historyInstrument('NSE:RELIANCE'), null);
});

test('index and commodity aliases do not apply to stocks and ETFs on stock exchanges', () => {
  // NASDAQ:DAX is the Global X DAX ETF, NYSE:WTI W&T Offshore: their history is TradingView's.
  for (const symbol of ['NASDAQ:DAX', 'NYSE:WTI', 'AMEX:SILVER', 'NYSE:GOLD']) {
    assert.equal(historyInstrument(symbol), null, symbol);
  }
  for (const symbol of ['TVC:GOLD', 'OANDA:XAUUSD', 'GOLD']) assert.equal(historyInstrument(symbol), 'xauusd', symbol);
  assert.equal(historyInstrument('XETR:DAX'), 'deuidxeur');
  assert.equal(historyInstrument('TVC:USOIL'), 'lightcmdusd');
  // Indices published under an exchange's own prefix keep their history.
  assert.equal(historyInstrument('CBOE:VIX'), 'volidxusd');
  assert.equal(historyInstrument('CBOE:SPX'), 'usa500idxusd');
  assert.equal(historyInstrument('NASDAQ:NDX'), 'usatechidxusd');
});

test('daily candles roll up into Monday weeks', () => {
  const day = (iso: string, c: number) => ({ t: Date.parse(iso) / 1000, o: c, h: c + 1, l: c - 1, c, v: 1 });
  const weeks = weekly([
    day('2026-09-14T00:00:00Z', 10),
    day('2026-09-16T00:00:00Z', 12),
    day('2026-09-21T00:00:00Z', 9),
  ]);
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
