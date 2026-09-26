// Regression test for a dukascopy-node bug (fixed via patches/dukascopy-node@1.50.0.patch):
// getOHLC's default `startTs = input[0][0]` was evaluated before its own `input.length === 0`
// guard, so any request whose raw remote bucket (a month of h1 candles, a day of m1 candles, a
// year of d1 candles) came back with zero rows crashed with "Cannot read properties of undefined
// (reading '0')" instead of contributing zero bars. That surfaced to users as "Load more history"
// failing outright on Dukascopy's own closed-market / pre-listing gaps. This calls dukascopy-node's
// real processData pipeline directly with a synthetic empty bucket, so it exercises the actual bug
// pattern without a live network dependency.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import dukascopyNodeImport from 'dukascopy-node';

const dukascopyNode = dukascopyNodeImport as unknown as {
  processData: (input: {
    instrument: string;
    requestedTimeframe: string;
    bufferObjects: Array<{ url: string; buffer: Buffer }>;
    priceType: string;
    volumes: boolean;
    volumeUnits: string;
    ignoreFlats: boolean;
  }) => number[][];
};

function emptyBucketBuffer(): Buffer {
  return Buffer.from(
    JSON.stringify({
      timestamp: 1700000000000,
      multiplier: 0.00001,
      shift: 3600000,
      open: 0,
      high: 0,
      low: 0,
      close: 0,
      times: [],
      opens: [],
      highs: [],
      lows: [],
      closes: [],
      volumes: [],
    }),
    'utf8',
  );
}

// Only timeframes dukascopy-node fetches at their own native granularity hit the buggy path
// (aggregate()'s generic `fromTimeframe === toTimeframe` branch, the one that never passes an
// explicit startTs into getOHLC). m1 is special-cased earlier without calling getOHLC at all, and
// every other timeframe (m5/m15/m30/h4/mn1/1w) is built by aggregating from a finer source, which
// always passes startTs explicitly — so h1 and d1 are the only two exposed to this crash.
for (const [timeframe, url] of [
  ['h1', 'https://jetta.dukascopy.com/v1/candles/hour/GBP-CHF/BID/2026/6'],
  ['d1', 'https://jetta.dukascopy.com/v1/candles/day/GBP-CHF/BID/2026'],
] as const) {
  test(`a fully empty remote bucket does not crash ${timeframe} aggregation`, () => {
    const result = dukascopyNode.processData({
      instrument: 'gbpchf',
      requestedTimeframe: timeframe,
      bufferObjects: [{ url, buffer: emptyBucketBuffer() }],
      priceType: 'bid',
      volumes: true,
      volumeUnits: 'units',
      ignoreFlats: true,
    });
    // No timestamped candles come out of a bucket with zero raw rows.
    assert.deepEqual(
      result.filter((candle) => candle.length > 0),
      [],
    );
  });
}
