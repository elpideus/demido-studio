// Our decoder against dukascopy-node's own processData, on real bucket responses (truncated to a few
// dozen candles each, see fixtures/dukascopy-buckets.json) for m1, h1 and d1, plus synthetic edge cases.

import assert from 'node:assert/strict';
import fs from 'node:fs';
import { test } from 'node:test';

import dukascopyNodeImport from 'dukascopy-node';

import { DecodeError, decode, isEmpty, parseResponse } from '../src/store/decode.ts';

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

const fixtures = JSON.parse(
  fs.readFileSync(new URL('./fixtures/dukascopy-buckets.json', import.meta.url), 'utf8'),
) as Record<string, Record<string, unknown> & { timestamp: number; shift: number; times: number[] }>;

const tierOf = (url: string) => (url.includes('/minute/') ? 'm1' : url.includes('/hour/') ? 'h1' : 'd1');

for (const [url, response] of Object.entries(fixtures)) {
  const tier = tierOf(url);
  test(`decode matches processData on ${url.replace(/^.*candles\//, '')}`, () => {
    const buffer = Buffer.from(JSON.stringify(response));
    const rows = dukascopyNode
      .processData({
        instrument: 'x',
        requestedTimeframe: tier,
        bufferObjects: [{ url, buffer }],
        priceType: 'bid',
        volumes: true,
        volumeUnits: 'millions',
        ignoreFlats: false,
      })
      .filter((row) => row.length > 0);
    // processData adds synthetic flat rows for skipped steps; keep only the real candle times.
    const real = new Set<number>();
    let ms = response.timestamp;
    for (const delta of response.times) real.add((ms += delta * response.shift));
    const expected = rows.filter((row) => real.has(row[0]!));
    const bars = decode(buffer);
    assert.equal(bars.length, response.times.length);
    assert.equal(expected.length, bars.length);
    assert.equal(isEmpty(buffer), bars.length === 0);
    bars.forEach((bar, i) => {
      const [t, o, h, l, c, v] = expected[i]!;
      assert.deepEqual([bar.t * 1000, bar.o, bar.h, bar.l, bar.c], [t, o, h, l, c], `${url} row ${i}`);
      // processData rounds h1/d1 volumes to 4 decimals while aggregating; we keep the raw value.
      assert.equal(tier === 'm1' ? bar.v : Number(bar.v.toFixed(4)), v, `${url} row ${i} volume`);
    });
    for (let i = 1; i < bars.length; i += 1) assert.ok(bars[i]!.t > bars[i - 1]!.t, 'ascending');
  });
}

test('the fixtures cover m1, h1 and d1, gaps, and empty responses', () => {
  const urls = Object.keys(fixtures);
  for (const tier of ['m1', 'h1', 'd1']) assert.ok(urls.some((u) => tierOf(u) === tier && fixtures[u]!.times.length));
  assert.ok(
    Object.values(fixtures).some((r) => r.times.slice(1).some((d) => d > 1)),
    'a skipped step',
  );
  assert.ok(
    Object.values(fixtures).some((r) => r.times.length === 0),
    'an empty bucket',
  );
});

const response = (patch: Record<string, unknown> = {}) => ({
  timestamp: Date.UTC(2024, 2, 5),
  multiplier: 0.001,
  shift: 60_000,
  open: 150.123,
  high: 150.2,
  low: 150.1,
  close: 150.15,
  times: [0, 1, 3],
  opens: [0, 10, -5],
  highs: [0, 7, 0],
  lows: [0, 2, -9],
  closes: [0, -1, 4],
  volumes: [12.5, 0, 3.25],
  ...patch,
});

test('decode: seconds, prices at the multiplier scale, no synthetic rows, zero volume kept', () => {
  const bars = decode(response());
  const t0 = Date.UTC(2024, 2, 5) / 1000;
  assert.deepEqual(bars, [
    { t: t0, o: 150.123, h: 150.2, l: 150.1, c: 150.15, v: 12.5 },
    { t: t0 + 60, o: 150.133, h: 150.207, l: 150.102, c: 150.149, v: 0 },
    // Two skipped minutes: no flat rows for them.
    { t: t0 + 240, o: 150.128, h: 150.207, l: 150.093, c: 150.153, v: 3.25 },
  ]);
  // Every input form gives the same bars.
  const json = JSON.stringify(response());
  assert.deepEqual(decode(json), bars);
  assert.deepEqual(decode(Buffer.from(json)), bars);
  assert.deepEqual(decode(new Uint8Array(Buffer.from(json))), bars);
});

test('decode: long runs of deltas never drift from the multiplier grid', () => {
  const n = 5000;
  const bars = decode(
    response({
      multiplier: 1e-5,
      open: 1.1,
      high: 1.1,
      low: 1.1,
      close: 1.1,
      times: Array.from({ length: n }, (_, i) => (i === 0 ? 0 : 1)),
      opens: Array.from({ length: n }, (_, i) => (i % 2 ? 3 : -2)),
      highs: new Array(n).fill(0),
      lows: new Array(n).fill(0),
      closes: new Array(n).fill(1),
      volumes: new Array(n).fill(1),
    }),
  );
  // 110000 units of 1e-5, plus one per candle for closes and +3/-2 alternating for opens.
  const last = bars[n - 1]!;
  assert.equal(last.c, 1.15);
  assert.equal(last.o, 1.125);
  bars.forEach((b, i) => {
    assert.equal(b.c, Number(((110000 + i + 1) / 1e5).toFixed(5)));
    assert.equal(Number(b.o.toFixed(5)), b.o);
  });
});

test('empty responses decode to no bars, even with null base prices', () => {
  const empty = response({
    open: null,
    high: null,
    low: null,
    close: null,
    times: [],
    opens: [],
    highs: [],
    lows: [],
    closes: [],
    volumes: [],
  });
  assert.deepEqual(decode(empty), []);
  assert.equal(isEmpty(empty), true);
  assert.equal(isEmpty(response()), false);
});

test('malformed responses throw DecodeError', () => {
  const bad: unknown[] = [
    'not json',
    '[]',
    'null',
    response({ timestamp: 'x' }),
    response({ volumes: [1, 2] }),
    response({ opens: undefined }),
    response({ highs: [0, Infinity, 0] }),
    response({ times: [0, -1, 1] }),
    response({ times: [0, 1.5, 1] }),
    response({ multiplier: 0 }),
    response({ multiplier: undefined }),
    response({ shift: 0 }),
    response({ open: null }),
  ];
  for (const raw of bad) {
    assert.throws(() => decode(raw as object), DecodeError, JSON.stringify(raw).slice(0, 80));
    assert.throws(() => isEmpty(raw as object), DecodeError);
  }
  assert.equal(parseResponse(response()).times.length, 3);
});
