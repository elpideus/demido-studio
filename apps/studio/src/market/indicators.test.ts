// Indicators computed on this computer while signed out: TradingView's formulas, ids and defaults.

import { describe, expect, it } from 'vitest';

import type { Bar } from '@/lib/types';
import {
  LOCAL_ENTRIES,
  ema,
  isLocal,
  localMeta,
  localRows,
  rma,
  rsi,
  sma,
  sourceOf,
  stdev,
  vwma,
  wma,
} from './indicators';

const bars = (closes: number[]): Bar[] =>
  closes.map((c, i) => ({ t: 1_700_000_000 + i * 60, o: c, h: c + 1, l: c - 1, c, v: 10 + i }));

const near = (values: Array<number | null>, expected: Array<number | null>) => {
  expect(values).toHaveLength(expected.length);
  values.forEach((v, i) => {
    const e = expected[i];
    if (e === null || e === undefined) expect(v).toBeNull();
    else expect(v).toBeCloseTo(e, 9);
  });
};

describe('moving averages', () => {
  it('averages the last `length` values once there are that many', () => {
    near(sma([1, 2, 3, 4, 5], 3), [null, null, 2, 3, 4]);
  });

  it('starts over after a missing value, as Pine does', () => {
    near(sma([1, 2, null, 4, 5, 6], 2), [null, 1.5, null, null, 4.5, 5.5]);
  });

  it('seeds the EMA and RMA with the SMA of their first values', () => {
    // alpha 2/(3+1) = 0.5: 2, then 0.5*4 + 0.5*2 = 3, then 0.5*5 + 0.5*3 = 4
    near(ema([1, 2, 3, 4, 5], 3), [null, null, 2, 3, 4]);
    // alpha 1/3: 2, then 4/3 + 2*2/3 = 8/3
    near(rma([1, 2, 3, 4], 3), [null, null, 2, 8 / 3]);
  });

  it('weights the newest value most in the WMA', () => {
    // (1*1 + 2*2 + 3*3) / 6
    near(wma([1, 2, 3], 3), [null, null, 14 / 6]);
  });

  it('weights by volume in the VWMA', () => {
    near(vwma([10, 20], [1, 3], 2), [null, (10 + 60) / 4]);
  });

  it('takes the population standard deviation', () => {
    near(stdev([2, 4, 4, 4, 5, 5, 7, 9], 8), [null, null, null, null, null, null, null, 2]);
  });
});

describe('rsi', () => {
  it('is 100 when prices only rise and 0 when they only fall', () => {
    expect(rsi([1, 2, 3, 4, 5], 3).slice(3)).toEqual([100, 100]);
    expect(rsi([5, 4, 3, 2, 1], 3).slice(3)).toEqual([0, 0]);
  });

  it("uses Wilder's averages of gains and losses", () => {
    // Changes +1, -1, +2: up = rma([1,0,2]) = 1, down = rma([0,1,0]) = 1/3 at the first value.
    const out = rsi([10, 11, 10, 12], 3);
    near(out, [null, null, null, 100 - 100 / (1 + 1 / (1 / 3))]);
  });
});

describe('sourceOf', () => {
  it("reads TradingView's sources off the bars", () => {
    const b: Bar[] = [{ t: 0, o: 1, h: 4, l: 0, c: 3, v: 0 }];
    expect(sourceOf(b, 'hl2')).toEqual([2]);
    expect(sourceOf(b, 'hlc3')).toEqual([7 / 3]);
    expect(sourceOf(b, 'ohlc4')).toEqual([2]);
    expect(sourceOf(b, 'hlcc4')).toEqual([2.5]);
    expect(sourceOf(b, 'something else')).toEqual([3]);
  });
});

describe('localMeta', () => {
  it("knows TradingView's built-in ids and nothing else", () => {
    expect(LOCAL_ENTRIES.map((e) => e.id).sort()).toEqual(['STD;Bollinger_Bands', 'STD;EMA', 'STD;RSI', 'STD;SMA']);
    expect(isLocal('STD;RSI')).toBe(true);
    expect(isLocal('PUB;abc')).toBe(false);
    expect(localMeta('PUB;abc')).toBeNull();
  });

  it("has TradingView's defaults", () => {
    const meta = localMeta('STD;RSI')!;
    expect(meta.overlay).toBe(false);
    expect(meta.precision).toBe(2);
    expect(meta.columns).toEqual(['plot_0', 'plot_2']);
    expect(meta.inputs.find((i) => i.id === 'in_0')?.value).toBe(14);
    expect(meta.bands.map((b) => b.value)).toEqual([70, 50, 30]);
  });

  it('takes input values, checked against what each input takes', () => {
    const meta = localMeta('STD;Bollinger_Bands', {
      inputs: { in_0: '30.4', in_1: 'EMA', in_2: 'nonsense', in_3: 99 },
    })!;
    const value = (id: string) => meta.inputs.find((i) => i.id === id)?.value;
    expect(value('in_0')).toBe(30);
    expect(value('in_1')).toBe('EMA');
    expect(value('in_2')).toBe('close');
    expect(value('in_3')).toBe(50);
  });

  it("takes a saved layout's look: colors, widths, hidden plots and moved bands", () => {
    const meta = localMeta('STD;RSI', {
      styles: { plot_0: { color: '#ff0000', linewidth: 2 }, plot_2: { visible: false } },
      bands: [{ value: 80 }, { visible: false }],
    })!;
    expect(meta.plots[0]).toMatchObject({ id: 'plot_0', color: '#ff0000', width: 2 });
    expect(meta.plots[0]!.hidden).toBeUndefined();
    // Hidden ones stay, marked, so the settings can show them again; their values are still computed.
    expect(meta.plots[1]).toMatchObject({ id: 'plot_2', hidden: true });
    expect(meta.columns).toEqual(['plot_0', 'plot_2']);
    expect(meta.bands.map((b) => [b.value, !!b.hidden])).toEqual([
      [80, false],
      [50, true],
      [30, false],
    ]);
  });
});

describe('localRows', () => {
  it('gives one row per bar with a value per column', () => {
    const b = bars([1, 2, 3, 4, 5]);
    const rows = localRows(localMeta('STD;SMA', { inputs: { in_0: 3 } })!, b);
    expect(rows.map((r) => r[0])).toEqual(b.map((x) => x.t));
    expect(rows.map((r) => r[1])).toEqual([null, null, 2, 3, 4]);
  });

  it('shifts by the offset input', () => {
    const rows = localRows(localMeta('STD;SMA', { inputs: { in_0: 2, in_2: 1 } })!, bars([1, 3, 5, 7]));
    expect(rows.map((r) => r[1])).toEqual([null, null, 2, 4]);
  });

  it('computes the three Bollinger lines', () => {
    const rows = localRows(localMeta('STD;Bollinger_Bands', { inputs: { in_0: 2, in_3: 1 } })!, bars([1, 3]));
    // Basis 2, deviation 1.
    expect(rows[1]).toEqual([rows[1]![0], 2, 3, 1]);
  });

  it('leaves the RSI smoothing out when its type is None', () => {
    const rows = localRows(localMeta('STD;RSI', { inputs: { in_0: 2, in_3: 'None' } })!, bars([1, 2, 3, 4]));
    expect(rows.map((r) => r[1])).toEqual([null, null, 100, 100]);
    expect(rows.every((r) => r[2] === null)).toBe(true);
  });

  it('has nothing for no bars', () => {
    expect(localRows(localMeta('STD;EMA')!, [])).toEqual([]);
  });
});
