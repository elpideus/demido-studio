import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import {
  colorOf,
  describe,
  graphicsOf,
  inputValues,
  parseFavorites,
  parseLayout,
  rowsOf,
  value,
  withTransparency,
} from '../src/indicators/describe.ts';
import { faultOf, studyError } from '../src/indicators/studies.ts';

// TradingView's own metadata for two built-ins, as pine-facade "translate" returns it (the
// compiled script body is left out).
const fixture = (name: string) =>
  JSON.parse(readFileSync(new URL(`./fixtures/${name}`, import.meta.url), 'utf8')) as Record<string, unknown>;
const RSI = fixture('tv-meta-rsi.json');
const MACD = fixture('tv-meta-macd.json');

test('RSI goes in its own pane with its two visible lines and its bands', () => {
  const m = describe(RSI);
  assert.equal(m.id, 'STD;RSI');
  assert.equal(m.short, 'RSI');
  assert.equal(m.kind, 'study');
  assert.equal(m.overlay, false);
  assert.equal(m.precision, 2);
  const titles = m.plots.filter((p) => !p.hidden).map((p) => p.title);
  // "Plot" and the Bollinger bands of the RSI are display:none by default: kept, but hidden, so
  // the settings can switch them on.
  assert.ok(titles.includes('RSI'));
  assert.ok(titles.includes('RSI-based MA'));
  assert.ok(!titles.includes('Plot'));
  assert.ok(!titles.includes('Upper Bollinger Band'));
  assert.equal(m.plots.find((p) => p.title === 'Upper Bollinger Band')?.hidden, true);
  // Hidden plots still carry their values.
  assert.equal(m.columns.length >= m.plots.length, true);
  const rsi = m.plots.find((p) => p.title === 'RSI')!;
  assert.deepEqual([rsi.kind, rsi.color, rsi.width], ['line', '#7E57C2', 1]);
  assert.deepEqual(
    m.bands.map((b) => b.value),
    [70, 50, 30],
  );
});

test('a colorer maps its values to palette colors through valToIndex', () => {
  const m = describe(RSI);
  const bullish = m.plots.find((p) => p.title === 'Regular Bullish')!;
  assert.equal(bullish.colorer, 'plot_7');
  // valToIndex {"1": 0, "2": 1}: value 1 is the green, value 2 the transparent white.
  assert.deepEqual(bullish.colors, { 1: 'rgba(76,175,80,1)', 2: 'rgba(255,255,255,0)' });
  // Every drawn plot is a column, followed by its colorer.
  assert.equal(m.columns[m.columns.indexOf('plot_6') + 1], 'plot_7');
});

test('plotshape plots become shapes with their location and text', () => {
  const m = describe(RSI);
  const label = m.plots.find((p) => p.title === 'Regular Bullish Label')!;
  assert.equal(label.kind, 'shapes');
  assert.equal(label.shape, 'label_up');
  assert.equal(label.location, 'absolute');
  assert.equal(label.text, 'Bull');
  assert.equal(label.textColor, '#FFFFFF');
});

test('MACD draws its histogram as columns colored per bar', () => {
  const m = describe(MACD);
  const hist = m.plots.find((p) => p.title === 'Histogram')!;
  assert.equal(hist.kind, 'columns');
  assert.equal(hist.colorer, 'plot_1');
  assert.equal(Object.keys(hist.colors!).length, 4);
  // No color in the metadata: the default blue.
  assert.equal(m.plots.find((p) => p.title === 'MACD')!.color, '#2962ff');
  assert.equal(m.precision, null);
});

test('inputs keep their metadata and take saved values', () => {
  const m = describe(RSI, { inputs: { in_0: 7 } });
  const length = m.inputs.find((i) => i.name === 'RSI Length')!;
  assert.equal(length.type, 'integer');
  assert.equal(length.value, 7);
  assert.equal(length.defval, 14);
  assert.equal(length.min, 1);
  const smoothing = m.inputs.find((i) => i.name === 'Type')!;
  assert.ok(smoothing.options!.includes('EMA'));
  assert.equal(smoothing.group, 'Smoothing');
  // Inputs the script keeps out of the status line stay out of the legend.
  assert.equal(length.legend, undefined);
  assert.equal(smoothing.legend, false);
  // The script body and its id are never inputs.
  assert.ok(!m.inputs.some((i) => ['text', 'pineId', 'pineVersion'].includes(i.id)));
  assert.ok(m.inputs.find((i) => i.id === '__fast_calc')!.hidden);
});

test('input values from the UI are coerced and clamped to their type', () => {
  const inputs = describe(RSI).inputs;
  assert.deepEqual(inputValues(inputs, { in_0: '21.6', in_2: 'true', in_3: 'EMA', nope: 1 }), {
    in_0: 22,
    in_2: true,
    in_3: 'EMA',
  });
  assert.deepEqual(inputValues(inputs, { in_0: 0 }), { in_0: 1 });
  assert.deepEqual(inputValues(inputs, { in_0: 'abc' }), {});
  // By title too, as the assistant may give them; the id wins.
  const length = inputs.find((i) => i.id === 'in_0')!.name;
  assert.deepEqual(inputValues(inputs, { [length.toUpperCase()]: 9 }), { in_0: 9 });
  assert.deepEqual(inputValues(inputs, { [length]: 9, in_0: 5 }), { in_0: 5 });
});

test('a layout overrides styles, palettes, bands and the pane', () => {
  const m = describe(RSI, {
    styles: { plot_0: { color: '#ff0000', linewidth: 3, title: 'plot_0' }, plot_2: { display: 0 } },
    palettes: { palette_1: { colors: { 0: { color: '#00ff00' } } } },
    bands: [{ value: 80 }, { value: null }, { visible: false }],
    overlay: true,
  });
  const rsi = m.plots.find((p) => p.id === 'plot_0')!;
  assert.deepEqual([rsi.color, rsi.width, rsi.title], ['#ff0000', 3, 'RSI']);
  assert.equal(m.plots.find((p) => p.id === 'plot_2')?.hidden, true);
  assert.equal(m.plots.find((p) => p.id === 'plot_6')!.colors![1], '#00ff00');
  assert.deepEqual(
    m.bands.map((b) => [b.value, b.hidden ?? false]),
    [
      [80, false],
      [50, false],
      [30, true],
    ],
  );
  assert.equal(m.overlay, true);
});

test('color inputs reach the palette through the metadata expressions', () => {
  const info = {
    scriptIdPart: 'USER;abc',
    pine: { version: '3.0' },
    description: 'Sessions',
    is_price_study: true,
    plots: [],
    inputs: [{ id: 'in_9', name: 'Border Color', type: 'color', defval: 'rgba(41,98,255,0.6)' }],
    palettes: { palette_common: { colors: { 0: { name: 'Color 0' }, 1: { name: 'Color 1' } } } },
    defaults: {
      palettes: { palette_common: { colors: { 0: { color: 'rgba(41,98,255,0.6)' }, 1: { color: '#ffffff' } } } },
    },
    TVScriptMetaInfoExprs: {
      patchMap: { 'defaults.palettes.palette_common.colors.0.color': 'rm_ic_0' },
      tree: "//@version=6\ni_border=##input(defval=#2962ff99,type='INPUT_COLOR',##id='in_9')\n##root(root_metainfo,rm_ic_0,i_border,\"input&&color\")\n",
    },
  };
  assert.deepEqual(describe(info).palette, ['rgba(41,98,255,0.6)', '#ffffff']);
  assert.deepEqual(describe(info, { inputs: { in_9: '#ff9800' } }).palette, ['#ff9800', '#ffffff']);
});

test('strategies are recognised', () => {
  assert.equal(describe({ ...MACD, isTVScriptStrategy: true }).kind, 'strategy');
});

test('rows run oldest first with "na" as null', () => {
  const rows = rowsOf(
    [{ $time: 200, plot_0: 2, plot_1: 1e100 }, { $time: 100, plot_0: 1, plot_1: 3 }, { plot_0: 9 }],
    ['plot_0', 'plot_1'],
  );
  assert.deepEqual(rows, [
    [100, 1, 3],
    [200, 2, null],
  ]);
  assert.equal(value(Number.NaN), null);
  assert.equal(value(-1e100), null);
  assert.equal(value(0), 0);
});

test('transparency turns into an alpha', () => {
  assert.equal(withTransparency('#ff0000', 50), 'rgba(255,0,0,0.5)');
  assert.equal(withTransparency('#f00', 0), '#f00');
  assert.equal(withTransparency('rgba(1,2,3,0.5)', 50), 'rgba(1,2,3,0.25)');
  assert.equal(withTransparency('#ff000080', 50), 'rgba(255,0,0,0.251)');
});

test('drawings are placed in time and colored from the palette', () => {
  const palette = ['#000000', '#111111', '#222222'];
  const times = [1000, 900, 800, 700]; // bars back → time
  const timeAt = (back: number) => times[back];
  const g = graphicsOf(
    {
      labels: [
        { id: 1, x: 1, y: 1.5, yLoc: 'price', text: 'Long', style: 'label_up', color: 2, textColor: 0, size: 'normal' },
        { id: 2, y: 1.6, yLoc: 'price', text: 'off the chart' },
        { id: 3, x: 0, y: 1e100, yLoc: 'abovebar', text: 'above', style: 'label_down', color: 1 },
      ],
      lines: [
        { id: 4, x1: 2, y1: 1, x2: 0, y2: 2, extend: 'none', style: 'solid', color: 0, width: 2 },
        { id: 5, x1: 3, y1: 1.2, y2: 1.2, extend: 'none', color: 1 },
        { id: 6, x1: 3, y1: 1.2, y2: 1.3, color: 1 },
      ],
      boxes: [{ id: 7, x1: 2, y1: 1.1, y2: 1.4, color: 0, bgColor: 1, extend: 'none', style: 'solid', width: 1 }],
    },
    {
      dwgtables: { 8: { id: 8, pos: 'top_right', rows: 1, cols: 2, bgc: null, frmc: 0, frmw: 1, brdc: 1, brdw: 1 } },
      dwgtablecells: {
        9: { id: 9, tid: 8, col: 1, row: 0, t: 'Both', tc: 2, bgc: 1, ts: 'small', colspan: 1, rowspan: 1 },
        10: { id: 10, tid: 8, col: 0, row: 0, t: 'SSA', tc: 2, bgc: 1, ts: 'small' },
      },
    },
    palette,
    timeAt,
    700,
    1000,
  );
  assert.deepEqual(
    g.labels.map((l) => [l.id, l.t, l.y, l.yloc, l.color]),
    [
      [3, 1000, null, 'abovebar', '#111111'],
      [1, 900, 1.5, 'price', '#222222'],
    ],
  );
  // A level whose right end is in the future runs to the edge; a sloped line with both ends
  // stays as it is; a sloped line with a missing end is dropped. Newest (then latest made) first.
  assert.deepEqual(
    g.lines.map((l) => [l.id, l.t1, l.t2, l.extend]),
    [
      [5, 700, 1000, 'right'],
      [4, 800, 1000, 'none'],
    ],
  );
  assert.deepEqual(
    g.boxes.map((b) => [b.t1, b.y1, b.t2, b.y2, b.color, b.bg, b.extend]),
    [[800, 1.4, 1000, 1.1, '#000000', '#111111', 'right']],
  );
  assert.equal(g.tables.length, 1);
  assert.deepEqual(
    g.tables[0]!.cells.map((c) => [c.col, c.text]),
    [
      [0, 'SSA'],
      [1, 'Both'],
    ],
  );
  assert.equal(g.tables[0]!.border, '#111111');
});

test('drawing colors may be indexes, strings or nothing', () => {
  assert.equal(colorOf(1, ['a', 'b']), 'b');
  assert.equal(colorOf(5, ['a']), null);
  assert.equal(colorOf('#fff', []), '#fff');
  assert.equal(colorOf(null, []), null);
});

test('favorites are read from the chart page settings', () => {
  const html = `<script>var user = {"settings":{"a":"1","studyMarket.favorites":"[\\"USER;dda9\\",\\"PUB;GOYN\\",\\"STD;RSI\\"]","b":"x"}};</script>`;
  assert.deepEqual(parseFavorites(html), ['USER;dda9', 'PUB;GOYN', 'STD;RSI']);
  assert.deepEqual(parseFavorites('<html></html>'), []);
  assert.deepEqual(parseFavorites('"studyMarket.favorites":"not json"'), []);
});

test('a chart layout yields its script studies with their state and pane', () => {
  const content = {
    name: 'My layout',
    charts: [
      {
        panes: [
          {
            sources: [
              { type: 'MainSeries', state: { symbol: 'FX:EURUSD', interval: '60' } },
              {
                type: 'Study',
                metaInfo: 'Script$USER;abc@tv-scripting-101[v.110.0]',
                state: {
                  visible: false,
                  inputs: { in_0: 5, text: 'body', in_1: { nested: 1 } },
                  styles: {},
                  palettes: {},
                },
              },
              { type: 'Study', metaInfo: 'Volume@tv-basicstudies-251', state: {} },
            ],
          },
          {
            sources: [
              { type: 'Study', metaInfo: 'Script$STD;RSI@tv-scripting-101[v.47.0]', state: { inputs: { in_0: 9 } } },
              { type: 'Study', metaInfo: 'MACD@tv-basicstudies-1', state: {} },
            ],
          },
        ],
      },
    ],
  };
  const metaMap = { 'Script$USER;abc@tv-scripting-101[v.110.0]': { description: 'Sessions' } };
  const html = `<script>\n\t\tinitData.content = ${JSON.stringify(content)};\n\t\tinitData.study_meta_info_map = ${JSON.stringify(metaMap)};\n</script>`;
  const layout = parseLayout(html)!;
  assert.equal(layout.name, 'My layout');
  assert.equal(layout.symbol, 'FX:EURUSD');
  assert.equal(layout.interval, '60');
  assert.deepEqual(
    layout.studies.map((s) => [s.id, s.version, s.name, s.hidden, s.state.overlay, s.state.inputs]),
    [
      ['USER;abc', '110.0', 'Sessions', true, true, { in_0: 5 }],
      ['STD;RSI', '47.0', 'STD;RSI', false, false, { in_0: 9 }],
    ],
  );
  assert.deepEqual(layout.skipped, ['MACD']);
  assert.equal(parseLayout('<html></html>'), null);
});

test('TradingView refusals are explained', () => {
  const limit = studyError([
    'some id',
    { ctx: { code: 'study_limit_exceeded' }, error: 'The maximum number of studies per chart has been reached' },
  ]);
  assert.equal(limit.code, 'STUDY_LIMIT');
  assert.match(limit.message, /plan/);
  assert.equal(studyError(['Script compile error']).code, 'STUDY_ERROR');
});

test('a runtime error names the line and the bar, without the server', () => {
  const args = [
    {
      ctx: { funcName: 'get', index: 3, code: 'RE10045', size: 0, bar_index: 0 },
      error:
        "Error on bar {bar_index}: In 'array.{funcName}()' function. Index {index} is out of bounds, array size is {size}.",
      stack_trace: [{ n: '#main', p: 4 }],
    },
    'some-server@some-server',
  ];
  assert.deepEqual(faultOf(args), {
    message: "Error on bar 0: In 'array.get()' function. Index 3 is out of bounds, array size is 0.",
    code: 'RE10045',
    bar: 0,
    line: 4,
  });
  const error = studyError(args);
  assert.equal(error.code, 'SCRIPT_RUNTIME');
  assert.match(error.message, /line 4/);
  assert.doesNotMatch(error.message, /some-server/);
});
