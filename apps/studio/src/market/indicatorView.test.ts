// Turning indicator rows into chart points, markers, legend text and script tables.

import { describe, expect, it } from 'vitest';

import type { IndicatorInput, IndicatorPlot, IndicatorRow } from '@/lib/types';
import {
  TRANSPARENT,
  anchorPoints,
  colorAt,
  columnIndex,
  fade,
  formatValue,
  legendInputs,
  mergeRows,
  plotPoints,
  shapeMarkers,
  tableGrid,
  tablePlacement,
} from './indicatorView';

const plot = (over: Partial<IndicatorPlot> = {}): IndicatorPlot => ({
  id: 'plot_0',
  title: 'Plot',
  kind: 'line',
  color: '#2962ff',
  width: 1,
  dash: 0,
  ...over,
});

const input = (over: Partial<IndicatorInput>): IndicatorInput => ({
  id: 'in_0',
  name: 'Length',
  type: 'integer',
  value: 14,
  defval: 14,
  hidden: false,
  fake: false,
  ...over,
});

describe('mergeRows', () => {
  const rows: IndicatorRow[] = [
    [10, 1],
    [20, 2],
    [30, 3],
  ];

  it('replaces rows by time and inserts new ones in order', () => {
    expect(
      mergeRows(
        rows,
        [
          [40, 4],
          [20, 9],
          [5, 0],
          [25, 7],
        ],
        false,
      ),
    ).toEqual([
      [5, 0],
      [10, 1],
      [20, 9],
      [25, 7],
      [30, 3],
      [40, 4],
    ]);
  });

  it('takes a full update as all there is', () => {
    expect(
      mergeRows(
        rows,
        [
          [2, 1],
          [1, 0],
        ],
        true,
      ),
    ).toEqual([
      [1, 0],
      [2, 1],
    ]);
  });
});

describe('colors and points', () => {
  const columns = columnIndex({ columns: ['plot_0', 'plot_0_color'] });
  const colored = plot({ colorer: 'plot_0_color', colors: { '0': '#00ff00', '1': '#ff0000' } });

  it('finds each column after the time', () => {
    expect(columns.get('plot_0')).toBe(1);
    expect(columns.get('plot_0_color')).toBe(2);
  });

  it("picks the colorer's color, and none where the colorer has no value", () => {
    expect(colorAt(colored, [0, 5, 1], columns)).toBe('#ff0000');
    expect(colorAt(colored, [0, 5, 7], columns)).toBe('#2962ff');
    expect(colorAt(colored, [0, 5, null], columns)).toBe(TRANSPARENT);
    expect(colorAt(plot(), [0, 5, 1], columns)).toBe('#2962ff');
  });

  it('gives every bar a point: a value, or a gap', () => {
    const rows = new Map<number, IndicatorRow>([
      [1, [1, 5, 0]],
      [3, [3, null, 0]],
      [4, [4, 6, null]],
    ]);
    expect(plotPoints(colored, (t) => rows.get(t), [1, 2, 3, 4], columns)).toEqual([
      { time: 1, value: 5, color: '#00ff00' },
      { time: 2 },
      { time: 3 },
      // An unset color hides a line's bar...
      { time: 4 },
    ]);
    // ...but a histogram keeps its bar, drawn transparent.
    expect(plotPoints({ ...colored, kind: 'histogram' }, (t) => rows.get(t), [4], columns)).toEqual([
      { time: 4, value: 6, color: TRANSPARENT },
    ]);
  });

  it('anchors a pane on its first plotted value, carried over gaps', () => {
    const meta = { plots: [plot({ id: 'plot_0', kind: 'shapes' }), plot({ id: 'plot_1' })] };
    const cols = columnIndex({ columns: ['plot_0', 'plot_1'] });
    const rows = new Map<number, IndicatorRow>([
      [1, [1, 1, 10]],
      [3, [3, 1, null]],
    ]);
    expect(anchorPoints(meta, (t) => rows.get(t), [1, 2, 3], cols)).toEqual([
      { time: 1, value: 10 },
      { time: 2, value: 10 },
      { time: 3, value: 10 },
    ]);
  });
});

describe('shapeMarkers', () => {
  const columns = columnIndex({ columns: ['plot_0'] });

  it('draws series bools where they are true, above or below the bar', () => {
    const meta = { plots: [plot({ kind: 'shapes', shape: 'triangle_up', location: 'belowbar', text: 'Buy' })] };
    expect(
      shapeMarkers(
        meta,
        [
          [1, 1],
          [2, 0],
          [3, null],
        ],
        columns,
      ),
    ).toEqual([{ time: 1, position: 'belowBar', shape: 'arrowUp', color: '#2962ff', text: 'Buy' }]);
  });

  it('puts absolute shapes at their value', () => {
    const meta = { plots: [plot({ kind: 'shapes', shape: 'label_down', location: 'absolute' })] };
    expect(shapeMarkers(meta, [[1, 1.5]], columns)).toEqual([
      { time: 1, position: 'atPriceBottom', shape: 'arrowDown', color: '#2962ff', price: 1.5 },
    ]);
  });

  it('points arrows by their sign, with the down color for negative values', () => {
    const meta = { plots: [plot({ kind: 'shapes', shape: 'arrow', downColor: '#f00' })] };
    expect(
      shapeMarkers(
        meta,
        [
          [1, 2],
          [2, 0],
          [3, -1],
        ],
        columns,
      ),
    ).toEqual([
      { time: 1, position: 'belowBar', shape: 'arrowUp', color: '#2962ff' },
      { time: 3, position: 'aboveBar', shape: 'arrowDown', color: '#f00' },
    ]);
  });

  it('starts at `from` and ignores plots that are not shapes', () => {
    const meta = { plots: [plot({ kind: 'shapes', shape: 'circle' })] };
    expect(
      shapeMarkers(
        meta,
        [
          [1, 1],
          [5, 1],
        ],
        columns,
        3,
      ).map((m) => m.time),
    ).toEqual([5]);
    expect(shapeMarkers({ plots: [plot()] }, [[1, 1]], columns)).toEqual([]);
  });
});

describe('legend', () => {
  it("lists the inputs TradingView's status line shows", () => {
    expect(
      legendInputs([
        input({ id: 'in_0', value: 14 }),
        input({ id: 'in_1', type: 'source', value: 'close' }),
        input({ id: 'in_2', type: 'bool', value: true }),
        input({ id: 'in_3', type: 'color', value: '#fff' }),
        input({ id: 'in_4', value: 0, legend: false }),
        input({ id: 'in_5', value: 3, hidden: true }),
        input({ id: 'in_6', type: 'float', value: 0.1 + 0.2 }),
      ]),
    ).toEqual(['14', 'close', '0.3']);
  });

  it('formats values with the indicator precision, and none as ∅', () => {
    expect(formatValue(null, 2)).toBe('∅');
    expect(formatValue(Number.NaN, 2)).toBe('∅');
    expect(formatValue(1.23456, 2)).toBe((1.23).toLocaleString(undefined, { minimumFractionDigits: 2 }));
  });
});

describe('fade', () => {
  it('scales the opacity of hex and rgb colors', () => {
    expect(fade('#ff0000', 0.5)).toBe('rgba(255,0,0,0.5)');
    expect(fade('#f00', 0.5)).toBe('rgba(255,0,0,0.5)');
    expect(fade('#ff000080', 0.5)).toBe('rgba(255,0,0,0.251)');
    expect(fade('rgba(1, 2, 3, 0.4)', 0.5)).toBe('rgba(1,2,3,0.2)');
    expect(fade('rgb(1,2,3)', 0.3)).toBe('rgba(1,2,3,0.3)');
    expect(fade('red', 0.5)).toBe('red');
  });
});

describe('script tables', () => {
  it("places them where Pine's position says", () => {
    expect(tablePlacement('top_right')).toEqual({ top: 6, right: 6 });
    expect(tablePlacement('bottom_left')).toEqual({ bottom: 6, left: 6 });
    expect(tablePlacement('middle_center')).toEqual({
      top: '50%',
      left: '50%',
      transform: 'translateY(-50%) translateX(-50%)',
    });
  });

  it('lays out merged cells once, leaving out what they cover', () => {
    const cells = [
      { row: 0, col: 0, colspan: 2, rowspan: 1, text: 'a' },
      { row: 1, col: 1, colspan: 1, rowspan: 1, text: 'b' },
    ];
    const grid = tableGrid(2, 2, cells);
    expect(grid.map((line) => line.map((c) => [c.col, c.cell?.text ?? null]))).toEqual([
      [[0, 'a']],
      [
        [0, null],
        [1, 'b'],
      ],
    ]);
  });

  it('grows to fit cells beyond the declared size', () => {
    const grid = tableGrid(1, 1, [{ row: 1, col: 2, colspan: 1, rowspan: 1 }]);
    expect(grid).toHaveLength(2);
    expect(grid[1]).toHaveLength(3);
  });
});
