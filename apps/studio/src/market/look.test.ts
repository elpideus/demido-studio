// An indicator's look (Style and Visibility settings) over its description.

import { describe, expect, it } from 'vitest';

import type { IndicatorMeta } from '@/lib/types';
import { applyLook, joinColor, lookOf, shownOn, splitColor, tidyLook } from './look';

const TFS = ['1m', '5m', '15m', '1h', '4h', '1d', '1w'];

const meta = (): IndicatorMeta => ({
  id: 'STD;RSI',
  version: '1',
  name: 'Relative Strength Index',
  short: 'RSI',
  kind: 'study',
  overlay: false,
  precision: 2,
  plots: [
    { id: 'plot_0', title: 'RSI', kind: 'line', color: '#7E57C2', width: 1, dash: 0 },
    { id: 'plot_1', title: 'MA', kind: 'line', color: '#FDD835', width: 1, dash: 0, hidden: true },
    { id: 'plot_2', title: 'Hist', kind: 'histogram', color: '#26a69a', width: 1, dash: 0, colorer: 'plot_3', colors: { '0': '#26a69a', '1': '#ef5350' } },
  ],
  bands: [{ id: 'hline_0', title: 'Upper', value: 70, color: '#787B86', width: 1, dash: 2 }],
  inputs: [],
  columns: ['plot_0', 'plot_1', 'plot_2', 'plot_3'],
  palette: [],
});

describe('applyLook', () => {
  it('leaves the description as it is without a look', () => {
    const m = meta();
    expect(applyLook(m, undefined)).toBe(m);
  });

  it('restyles plots and bands, and shows or hides them', () => {
    const m = applyLook(meta(), {
      plots: {
        plot_0: { color: '#ff0000', width: 3, dash: 1, kind: 'step', hidden: true },
        plot_1: { hidden: false },
        plot_2: { colors: { '1': '#000000' } },
      },
      bands: { hline_0: { value: 80, color: '#ffffff', hidden: true } },
      precision: 4,
    });
    expect(m.plots[0]).toMatchObject({ color: '#ff0000', width: 3, dash: 1, kind: 'step', hidden: true });
    expect(m.plots[1]!.hidden).toBeUndefined();
    expect(m.plots[2]!.colors).toEqual({ '0': '#26a69a', '1': '#000000' });
    expect(m.bands[0]).toMatchObject({ value: 80, color: '#ffffff', hidden: true });
    expect(m.precision).toBe(4);
    // The description itself is untouched.
    expect(meta().plots[1]!.hidden).toBe(true);
  });

  it('takes the price scale labels off every plot', () => {
    const m = applyLook(meta(), { scaleLabels: false });
    expect(m.plots.every((p) => p.axisLabel === false)).toBe(true);
  });

  it('keeps a shape plot a shape', () => {
    const base = meta();
    base.plots[0]!.kind = 'shapes';
    expect(applyLook(base, { plots: { plot_0: { kind: 'line' } } }).plots[0]!.kind).toBe('shapes');
  });
});

describe('shownOn', () => {
  it('shows on every timeframe unless some are picked', () => {
    expect(shownOn(undefined, '1h')).toBe(true);
    expect(shownOn({ timeframes: ['1d', '1w'] }, '1h')).toBe(false);
    expect(shownOn({ timeframes: ['1d', '1w'] }, '1d')).toBe(true);
  });
});

describe('tidyLook', () => {
  it('keeps only what differs from the description', () => {
    const m = meta();
    expect(
      tidyLook(
        m,
        {
          plots: {
            plot_0: { color: '#7E57C2', width: 2 },
            plot_1: { hidden: true },
            plot_2: { colors: { '0': '#26a69a', '1': '#111111' } },
          },
          bands: { hline_0: { value: 70, dash: 0 } },
          precision: 2,
          timeframes: [...TFS],
          legendInputs: true,
          legendValues: false,
        },
        TFS,
      ),
    ).toEqual({
      plots: { plot_0: { width: 2 }, plot_2: { colors: { '1': '#111111' } } },
      bands: { hline_0: { dash: 0 } },
      legendValues: false,
    });
  });

  it('is nothing when nothing differs', () => {
    expect(tidyLook(meta(), { plots: { plot_0: { color: '#7E57C2' } }, timeframes: TFS }, TFS)).toBeUndefined();
  });

  it('keeps the picked timeframes in order', () => {
    expect(tidyLook(meta(), { timeframes: ['1w', '1h'] }, TFS)).toEqual({ timeframes: ['1h', '1w'] });
  });
});

describe('colors', () => {
  it('split into a hex and an opacity, and back', () => {
    expect(splitColor('#ABC')).toEqual({ hex: '#aabbcc', alpha: 1 });
    expect(splitColor('#2962ff80')).toEqual({ hex: '#2962ff', alpha: 0.5 });
    expect(splitColor('rgba(41, 98, 255, 0.25)')).toEqual({ hex: '#2962ff', alpha: 0.25 });
    expect(splitColor('rgb(0,0,0)')).toEqual({ hex: '#000000', alpha: 1 });
    expect(splitColor('blue')).toBeNull();
    expect(joinColor('#2962ff', 1)).toBe('#2962ff');
    expect(joinColor('#2962ff', 0.25)).toBe('rgba(41,98,255,0.25)');
  });
});

describe('lookOf', () => {
  it('keeps what is well formed and drops the rest', () => {
    expect(
      lookOf({
        plots: { plot_0: { color: '#fff', width: 9, dash: 5, kind: 'pie', hidden: 'no' }, plot_1: 3 },
        bands: { hline_0: { value: 'x' } },
        precision: 30,
        timeframes: ['1h', 4],
        legendValues: false,
      }),
    ).toEqual({ plots: { plot_0: { color: '#fff', width: 4 } }, timeframes: ['1h'], legendValues: false });
    expect(lookOf(null)).toBeUndefined();
    expect(lookOf({ plots: {} })).toBeUndefined();
  });
});
