// The assistant's drawings, as the chart draws them.

import { describe, expect, it } from 'vitest';

import type { ChartDrawing } from '@/lib/types';
import {
  chartPatch,
  drawingMeta,
  drawingView,
  drawingsOf,
  drawnOn,
  sameMarket,
  withDrawing,
  withoutDrawing,
} from './chartCommands';
import { savedIndicators } from './savedIndicators';

// Hourly bars.
const TIMES = [3600, 7200, 10800, 14400];

const set = (items: ChartDrawing['items'], pane: ChartDrawing['pane'] = 'overlay'): ChartDrawing => ({
  name: 'Levels',
  pane,
  items,
});

describe('drawing sets', () => {
  it('are read from the props, malformed ones left out', () => {
    expect(
      drawingsOf([
        { name: 'A', pane: 'separate', items: [{ type: 'hline', price: 1 }, { type: 'nope' }], symbol: 'FX:EURUSD', hidden: true },
        { name: 'A', items: [] },
        { name: '', items: [] },
        { name: 'B', items: 'x' },
        { name: 'C', items: [] },
      ]),
    ).toEqual([
      { name: 'A', pane: 'separate', items: [{ type: 'hline', price: 1 }], symbol: 'FX:EURUSD', hidden: true },
      { name: 'C', pane: 'overlay', items: [] },
    ]);
    expect(drawingsOf(null)).toEqual([]);
  });

  it('replace the set of the same name in place, shown again, and are removed by name', () => {
    const a = { name: 'A', pane: 'overlay' as const, items: [], hidden: true };
    const b = { name: 'B', pane: 'overlay' as const, items: [] };
    const next = withDrawing([a, b], { name: 'A', pane: 'separate', items: [{ type: 'hline', price: 2 }] });
    expect(next.map((d) => [d.name, d.pane, !!d.hidden])).toEqual([
      ['A', 'separate', false],
      ['B', 'overlay', false],
    ]);
    expect(withDrawing([a], b).map((d) => d.name)).toEqual(['A', 'B']);
    expect(withoutDrawing([a, b], 'A')).toEqual([b]);
    expect(withoutDrawing([a, b], '*')).toEqual([]);
  });
});

describe('markets', () => {
  it('are the same with or without their exchange', () => {
    expect(sameMarket('FX:EURUSD', 'eurusd')).toBe(true);
    expect(sameMarket('FX:EURUSD', 'OANDA:EURUSD')).toBe(true);
    expect(sameMarket('FX:EURUSD', 'FX:GBPUSD')).toBe(false);
    expect(drawnOn({ name: 'a', pane: 'overlay', items: [] }, 'X:Y')).toBe(true);
    expect(drawnOn({ name: 'a', pane: 'overlay', items: [], symbol: 'BINANCE:BTCUSDT' }, 'FX:EURUSD')).toBe(false);
  });
});

describe('chartPatch', () => {
  const props = {
    symbol: 'FX:EURUSD',
    indicators: [{ key: 'k1', script: 'STD;RSI', version: null, name: 'RSI', setup: {}, hidden: true }],
  };

  it('adds an indicator and shows the chart, on the asked symbol and timeframe', () => {
    const patch = chartPatch(props, {
      action: 'indicator',
      script: 'DEMIDO;abc',
      name: 'My cross',
      inputs: { Length: 9 },
      symbol: 'BINANCE:BTCUSDT',
      timeframe: '4h',
    });
    expect(patch).toMatchObject({ tab: 'chart', symbol: 'BINANCE:BTCUSDT', timeframe: '4h' });
    const list = savedIndicators(patch.indicators);
    expect(list.map((s) => [s.script, s.name, s.setup])).toEqual([
      ['STD;RSI', 'RSI', {}],
      ['DEMIDO;abc', 'My cross', { inputs: { Length: 9 } }],
    ]);
    expect(new Set(list.map((s) => s.key)).size).toBe(2);
  });

  it('shows an indicator again instead of adding it twice', () => {
    const patch = chartPatch(props, { action: 'indicator', script: 'STD;RSI', name: null, inputs: {}, symbol: null, timeframe: null });
    expect(patch).toEqual({ tab: 'chart', indicators: [{ key: 'k1', script: 'STD;RSI', version: null, name: 'RSI', setup: {} }] });
  });

  it("names a TradingView script by its id until its description arrives", () => {
    const patch = chartPatch({}, { action: 'indicator', script: 'PUB;xyz', name: null, inputs: {}, symbol: null, timeframe: null });
    expect(savedIndicators(patch.indicators)[0]).toMatchObject({ script: 'PUB;xyz', name: 'PUB;xyz', version: null });
  });

  it("draws on the chart's market, and removes drawings without showing the chart", () => {
    const drawn = chartPatch(props, {
      action: 'draw',
      drawing: { name: 'Levels', pane: 'overlay', items: [{ type: 'hline', price: 1 }] },
      symbol: null,
      timeframe: null,
    });
    expect(drawn).toEqual({
      tab: 'chart',
      drawings: [{ name: 'Levels', pane: 'overlay', items: [{ type: 'hline', price: 1 }], symbol: 'FX:EURUSD' }],
    });
    expect(chartPatch({ ...props, ...drawn }, { action: 'undraw', name: 'Levels' })).toEqual({ drawings: [] });
  });
});

describe('drawingMeta', () => {
  it('has a plot per series, in its pane', () => {
    const m = drawingMeta(
      set(
        [
          { type: 'hline', price: 1 },
          { type: 'series', points: [], text: 'Fair value', color: '#fff' },
          { type: 'series', points: [], style: 'dashed', width: 3 },
        ],
        'separate',
      ),
    );
    expect(m.overlay).toBe(false);
    expect(m.plots.map((p) => [p.id, p.title, p.color, p.width, p.dash])).toEqual([
      ['series_1', 'Fair value', '#fff', 2, 0],
      ['series_2', 'Levels 2', '#ff9800', 3, 2],
    ]);
    expect(m.columns).toEqual(['series_1', 'series_2']);
  });
});

describe('drawingView', () => {
  it('puts series values on the bars they fall in', () => {
    const { rows } = drawingView(
      set([
        {
          type: 'series',
          points: [
            [3600, 1],
            [7300, 2],
            [10800, null],
            [99999, 9],
          ],
        },
      ]),
      TIMES,
    );
    expect(rows).toEqual([
      [3600, 1],
      [7200, 2],
      [10800, null],
      [14400, null],
    ]);
  });

  it('draws a level across the chart, with its text at the right', () => {
    const { graphics } = drawingView(set([{ type: 'hline', price: 1.1, text: 'R1', color: '#f00', style: 'dashed' }]), TIMES);
    expect(graphics.lines[0]).toMatchObject({ t1: 3600, t2: 14400, y1: 1.1, y2: 1.1, extend: 'both', style: 'dashed', color: '#f00' });
    expect(graphics.labels[0]).toMatchObject({ t: 14400, y: 1.1, text: 'R1', style: 'label_left' });
  });

  it('cuts a line where the bars end, along the line, and snaps its ends to bars', () => {
    const { graphics } = drawingView(set([{ type: 'line', time: 0, price: 0, time2: 7300, price2: 73 }]), TIMES);
    expect(graphics.lines[0]).toMatchObject({ t1: 3600, y1: 36, t2: 7200, y2: 73 });
    // Wholly before the bars: nothing.
    expect(drawingView(set([{ type: 'line', time: 0, price: 0, time2: 100, price2: 1 }]), TIMES).graphics.lines).toEqual([]);
  });

  it('draws a box between its corners, top first', () => {
    const { graphics } = drawingView(set([{ type: 'box', time: 10900, price: 1, time2: 3700, price2: 2, text: 'Zone' }]), TIMES);
    expect(graphics.boxes[0]).toMatchObject({ t1: 3600, t2: 10800, y1: 2, y2: 1, text: 'Zone', color: '#2962ff' });
    expect(graphics.boxes[0]!.bg).toMatch(/^rgba\(41,98,255,/);
  });

  it('puts labels at a price or over a bar, and markers above or below bars', () => {
    const { graphics } = drawingView(
      set([
        { type: 'label', time: 7200, price: 5, text: 'Here' },
        { type: 'label', time: 7200, position: 'below', text: 'Low' },
        { type: 'marker', time: 10800, shape: 'arrow_up', text: 'Buy' },
        { type: 'marker', time: 14400, position: 'above' },
        { type: 'marker', time: 100 },
      ]),
      TIMES,
    );
    expect(graphics.labels.map((l) => [l.t, l.y, l.yloc, l.style, l.text, l.color])).toEqual([
      [7200, 5, 'price', 'label_down', 'Here', '#2962ff'],
      [7200, null, 'belowbar', 'label_up', 'Low', '#2962ff'],
      [10800, null, 'belowbar', 'arrowup', 'Buy', '#089981'],
      [14400, null, 'abovebar', 'triangledown', '', '#f23645'],
    ]);
  });

  it('has nothing to draw without bars', () => {
    expect(drawingView(set([{ type: 'hline', price: 1 }]), [])).toEqual({
      rows: [],
      graphics: { labels: [], lines: [], boxes: [], tables: [] },
    });
  });
});
