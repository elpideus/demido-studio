// Indicators computed on this computer from the chart's own bars, for charts without a TradingView
// session (signed out): TradingView's built-in SMA, EMA, Bollinger Bands and RSI. They use
// TradingView's formulas, plot ids and input ids, so a chart's indicators carry over both ways:
// signed in, TradingView computes the same indicator with the same settings.
//
// TradingView starts computing from the market's first bar; here the first loaded bar starts it.
// Moving averages need `length` bars before their first value, and the EMA and RSI settle within a
// few times `length` bars to TradingView's values.

import type { Bar, IndicatorEntry, IndicatorInput, IndicatorMeta, IndicatorPlot, IndicatorRow, IndicatorSetup } from '@/lib/types';

type Series = Array<number | null>;

/** Simple moving average; null until `length` values are in. */
export function sma(values: Series, length: number): Series {
  const out: Series = new Array(values.length).fill(null);
  let sum = 0;
  let count = 0;
  for (let i = 0; i < values.length; i += 1) {
    const v = values[i];
    if (v === null || v === undefined) {
      // Pine: any na in the window makes the average na.
      sum = 0;
      count = 0;
      continue;
    }
    sum += v;
    count += 1;
    if (count > length) {
      sum -= values[i - length]!;
      count = length;
    }
    if (count === length) out[i] = sum / length;
  }
  return out;
}

/** An exponential average with weight `alpha`, seeded with the SMA of its first `length` values
 *  (Pine's ta.ema and ta.rma). */
function smoothed(values: Series, length: number, alpha: number): Series {
  const seed = sma(values, length);
  const out: Series = new Array(values.length).fill(null);
  let prev: number | null = null;
  for (let i = 0; i < values.length; i += 1) {
    const v = values[i];
    if (prev === null) {
      prev = seed[i] ?? null;
    } else if (v !== null && v !== undefined) {
      prev = alpha * v + (1 - alpha) * prev;
    }
    out[i] = prev;
  }
  return out;
}

/** Exponential moving average (Pine's ta.ema). */
export function ema(values: Series, length: number): Series {
  return smoothed(values, length, 2 / (length + 1));
}

/** Wilder's moving average (Pine's ta.rma, "SMMA (RMA)"). */
export function rma(values: Series, length: number): Series {
  return smoothed(values, length, 1 / length);
}

/** Weighted moving average (Pine's ta.wma). */
export function wma(values: Series, length: number): Series {
  const out: Series = new Array(values.length).fill(null);
  const norm = (length * (length + 1)) / 2;
  for (let i = length - 1; i < values.length; i += 1) {
    let sum = 0;
    let ok = true;
    for (let k = 0; k < length; k += 1) {
      const v = values[i - k];
      if (v === null || v === undefined) {
        ok = false;
        break;
      }
      sum += v * (length - k);
    }
    if (ok) out[i] = sum / norm;
  }
  return out;
}

/** Volume-weighted moving average (Pine's ta.vwma). */
export function vwma(values: Series, volumes: Series, length: number): Series {
  const weighted = values.map((v, i) => (v === null || volumes[i] === null ? null : v * volumes[i]!));
  const num = sma(weighted, length);
  const den = sma(volumes, length);
  return num.map((n, i) => (n === null || !den[i] ? null : n / den[i]!));
}

/** Population standard deviation over `length` values (Pine's ta.stdev). */
export function stdev(values: Series, length: number): Series {
  const mean = sma(values, length);
  const out: Series = new Array(values.length).fill(null);
  for (let i = length - 1; i < values.length; i += 1) {
    const m = mean[i];
    if (m === null || m === undefined) continue;
    let sum = 0;
    for (let k = 0; k < length; k += 1) {
      const d = values[i - k]! - m;
      sum += d * d;
    }
    out[i] = Math.sqrt(sum / length);
  }
  return out;
}

/** Relative strength index (TradingView's built-in RSI: Wilder's averages of gains and losses). */
export function rsi(values: Series, length: number): Series {
  const gains: Series = [null];
  const losses: Series = [null];
  for (let i = 1; i < values.length; i += 1) {
    const a = values[i - 1];
    const b = values[i];
    if (a === null || a === undefined || b === null || b === undefined) {
      gains.push(null);
      losses.push(null);
      continue;
    }
    gains.push(Math.max(b - a, 0));
    losses.push(Math.max(a - b, 0));
  }
  const up = rma(gains, length);
  const down = rma(losses, length);
  return up.map((u, i) => {
    const d = down[i];
    if (u === null || d === null || d === undefined) return null;
    if (d === 0) return 100;
    if (u === 0) return 0;
    return 100 - 100 / (1 + u / d);
  });
}

export const SOURCES = ['open', 'high', 'low', 'close', 'hl2', 'hlc3', 'hlcc4', 'ohlc4'] as const;

/** A bar's price by TradingView's source name. */
export function sourceOf(bars: readonly Bar[], source: unknown): number[] {
  switch (source) {
    case 'open':
      return bars.map((b) => b.o);
    case 'high':
      return bars.map((b) => b.h);
    case 'low':
      return bars.map((b) => b.l);
    case 'hl2':
      return bars.map((b) => (b.h + b.l) / 2);
    case 'hlc3':
      return bars.map((b) => (b.h + b.l + b.c) / 3);
    case 'hlcc4':
      return bars.map((b) => (b.h + b.l + 2 * b.c) / 4);
    case 'ohlc4':
      return bars.map((b) => (b.o + b.h + b.l + b.c) / 4);
    default:
      return bars.map((b) => b.c);
  }
}

const MA_TYPES = ['SMA', 'EMA', 'SMMA (RMA)', 'WMA', 'VWMA'];

function average(type: unknown, values: Series, volumes: Series, length: number): Series {
  switch (type) {
    case 'EMA':
      return ema(values, length);
    case 'SMMA (RMA)':
      return rma(values, length);
    case 'WMA':
      return wma(values, length);
    case 'VWMA':
      return vwma(values, volumes, length);
    default:
      return sma(values, length);
  }
}

/** Moves values `offset` bars to the right (Pine's `offset=`), dropping what leaves the chart. */
function shifted(values: Series, offset: number): Series {
  if (!offset) return values;
  const out: Series = new Array(values.length).fill(null);
  for (let i = 0; i < values.length; i += 1) {
    const j = i + offset;
    if (j >= 0 && j < values.length) out[j] = values[i]!;
  }
  return out;
}

// -------------------------------------------------------------------------------------------
// The indicators, described like TradingView describes its built-ins (same ids and defaults).

const lengthInput = (id: string, name: string, defval: number): IndicatorInput => ({
  id,
  name,
  type: 'integer',
  value: defval,
  defval,
  hidden: false,
  fake: false,
  min: 1,
  max: 5000,
  step: 1,
});

const sourceInput = (id: string): IndicatorInput => ({
  id,
  name: 'Source',
  type: 'source',
  value: 'close',
  defval: 'close',
  hidden: false,
  fake: false,
  options: [...SOURCES],
});

const offsetInput = (id: string): IndicatorInput => ({
  id,
  name: 'Offset',
  type: 'integer',
  value: 0,
  defval: 0,
  hidden: false,
  fake: false,
  min: -500,
  max: 500,
  step: 1,
  legend: false,
});

const line = (id: string, title: string, color: string): IndicatorPlot => ({
  id,
  title,
  kind: 'line',
  color,
  width: 1,
  dash: 0,
});

interface Local {
  meta: Omit<IndicatorMeta, 'columns' | 'palette' | 'version' | 'kind'>;
  /** One series per plot of the meta, in its order. */
  compute: (bars: readonly Bar[], inputs: Record<string, unknown>) => Series[];
}

const LOCAL: Record<string, Local> = {
  'STD;SMA': {
    meta: {
      id: 'STD;SMA',
      name: 'Simple moving average',
      short: 'SMA',
      overlay: true,
      precision: null,
      plots: [line('plot_0', 'MA', '#2962ff')],
      bands: [],
      inputs: [lengthInput('in_0', 'Length', 9), sourceInput('in_1'), offsetInput('in_2')],
    },
    compute: (bars, i) => [shifted(sma(sourceOf(bars, i.in_1), len(i.in_0, 9)), offset(i.in_2))],
  },
  'STD;EMA': {
    meta: {
      id: 'STD;EMA',
      name: 'Exponential moving average',
      short: 'EMA',
      overlay: true,
      precision: null,
      plots: [line('plot_0', 'EMA', '#2962ff')],
      bands: [],
      inputs: [lengthInput('in_0', 'Length', 9), sourceInput('in_1'), offsetInput('in_2')],
    },
    compute: (bars, i) => [shifted(ema(sourceOf(bars, i.in_1), len(i.in_0, 9)), offset(i.in_2))],
  },
  'STD;Bollinger_Bands': {
    meta: {
      id: 'STD;Bollinger_Bands',
      name: 'Bollinger Bands',
      short: 'BB',
      overlay: true,
      precision: null,
      plots: [line('plot_0', 'Basis', '#2962FF'), line('plot_1', 'Upper', '#F23645'), line('plot_2', 'Lower', '#089981')],
      bands: [],
      inputs: [
        lengthInput('in_0', 'Length', 20),
        {
          id: 'in_1',
          name: 'Basis MA Type',
          type: 'text',
          value: 'SMA',
          defval: 'SMA',
          hidden: false,
          fake: false,
          options: MA_TYPES,
        },
        sourceInput('in_2'),
        { id: 'in_3', name: 'StdDev', type: 'float', value: 2, defval: 2, hidden: false, fake: false, min: 0.001, max: 50, step: 0.5 },
        offsetInput('in_4'),
      ],
    },
    compute: (bars, i) => {
      const src = sourceOf(bars, i.in_2);
      const length = len(i.in_0, 20);
      const basis = average(i.in_1, src, bars.map((b) => b.v), length);
      const dev = stdev(src, length);
      const mult = typeof i.in_3 === 'number' ? i.in_3 : 2;
      const shift = offset(i.in_4);
      return [
        shifted(basis, shift),
        shifted(basis.map((b, k) => (b === null || dev[k] === null ? null : b + mult * dev[k]!)), shift),
        shifted(basis.map((b, k) => (b === null || dev[k] === null ? null : b - mult * dev[k]!)), shift),
      ];
    },
  },
  'STD;RSI': {
    meta: {
      id: 'STD;RSI',
      name: 'Relative strength index',
      short: 'RSI',
      overlay: false,
      precision: 2,
      plots: [line('plot_0', 'RSI', '#7E57C2'), line('plot_2', 'RSI-based MA', '#FDD835')],
      bands: [
        { id: 'hline_0', title: 'RSI Upper Band', value: 70, color: '#787B86', width: 1, dash: 2 },
        { id: 'hline_1', title: 'RSI Middle Band', value: 50, color: 'rgba(120,123,134,0.5)', width: 1, dash: 2 },
        { id: 'hline_2', title: 'RSI Lower Band', value: 30, color: '#787B86', width: 1, dash: 2 },
      ],
      inputs: [
        lengthInput('in_0', 'RSI Length', 14),
        sourceInput('in_1'),
        {
          id: 'in_3',
          name: 'Smoothing type',
          type: 'text',
          value: 'SMA',
          defval: 'SMA',
          hidden: false,
          fake: false,
          options: ['None', ...MA_TYPES],
          group: 'Smoothing',
          legend: false,
        },
        { ...lengthInput('in_4', 'Smoothing length', 14), group: 'Smoothing', legend: false },
      ],
    },
    compute: (bars, i) => {
      const value = rsi(sourceOf(bars, i.in_1), len(i.in_0, 14));
      const ma =
        i.in_3 === 'None' ? value.map(() => null) : average(i.in_3, value, bars.map((b) => b.v), len(i.in_4, 14));
      return [value, ma];
    },
  },
};

/** A length input: a whole number of bars, at least one. */
function len(v: unknown, fallback = 14): number {
  const n = typeof v === 'number' && Number.isFinite(v) ? Math.round(v) : fallback;
  return Math.max(1, Math.min(5000, n));
}

/** An offset input: whole bars, either way. */
function offset(v: unknown): number {
  return typeof v === 'number' && Number.isFinite(v) ? Math.max(-500, Math.min(500, Math.round(v))) : 0;
}

/** The indicators this computer can compute, as the Indicators menu lists them. */
export const LOCAL_ENTRIES: IndicatorEntry[] = Object.values(LOCAL)
  .map((l) => ({ id: l.meta.id, version: null, name: l.meta.name, short: l.meta.short, overlay: l.meta.overlay, kind: 'study' as const }))
  .sort((a, b) => a.name.localeCompare(b.name));

export function isLocal(script: string): boolean {
  return script in LOCAL;
}

/** An input's value as the setup gives it, checked against what the input takes. */
function inputValue(input: IndicatorInput, given: unknown): unknown {
  if (given === undefined || given === null) return input.defval;
  if (input.type === 'integer' || input.type === 'float') {
    const n = typeof given === 'number' ? given : Number(given);
    if (!Number.isFinite(n)) return input.defval;
    const clamped = Math.max(input.min ?? -Infinity, Math.min(input.max ?? Infinity, n));
    return input.type === 'integer' ? Math.round(clamped) : clamped;
  }
  if (input.options && !input.options.includes(String(given))) return input.defval;
  return String(given);
}

/** Look settings a saved TradingView layout gave a plot or band (color, width, style, visibility).
 *  A hidden one is kept, marked hidden, so its settings can show it again. */
function styled<T extends { color: string; width: number; dash: number; hidden?: boolean }>(
  item: T,
  style: Record<string, unknown> | undefined,
): T {
  if (!style) return item;
  const out = { ...item };
  if ((typeof style.display === 'number' && (style.display & 1) === 0) || style.visible === false) out.hidden = true;
  if (typeof style.color === 'string' && style.color) out.color = style.color;
  if (typeof style.linewidth === 'number') out.width = style.linewidth;
  if (typeof style.linestyle === 'number') out.dash = style.linestyle;
  return out;
}

/** The description of a local indicator set up as `setup` says, or null for any other script. */
export function localMeta(script: string, setup: IndicatorSetup = {}): IndicatorMeta | null {
  const local = LOCAL[script];
  if (!local) return null;
  const inputs = local.meta.inputs.map((i) => ({ ...i, value: inputValue(i, setup.inputs?.[i.id]) }));
  const plots: IndicatorPlot[] = local.meta.plots.map((p) => styled(p, setup.styles?.[p.id]));
  const bands = local.meta.bands.map((b, i) => {
    const s = setup.bands?.[i];
    const out = styled(b, s && typeof s === 'object' ? (s as Record<string, unknown>) : undefined);
    if (s && typeof s === 'object' && typeof (s as { value?: unknown }).value === 'number') {
      out.value = (s as { value: number }).value;
    }
    return out;
  });
  return {
    ...local.meta,
    version: 'local',
    kind: 'study',
    overlay: setup.overlay ?? local.meta.overlay,
    inputs,
    plots,
    bands,
    columns: plots.map((p) => p.id),
    palette: [],
  };
}

/** A local indicator's rows over `bars` (oldest first), one value per column of `meta`. */
export function localRows(meta: IndicatorMeta, bars: readonly Bar[]): IndicatorRow[] {
  const local = LOCAL[meta.id];
  if (!local || !bars.length) return [];
  const inputs: Record<string, unknown> = {};
  for (const i of meta.inputs) inputs[i.id] = i.value;
  const series = local.compute(bars, inputs);
  const byPlot = new Map(local.meta.plots.map((p, k) => [p.id, series[k] ?? []]));
  const columns = meta.columns.map((c) => byPlot.get(c) ?? []);
  return bars.map((b, k): IndicatorRow => {
    const values = columns.map((s) => {
      const v = s[k];
      return v === null || v === undefined || !Number.isFinite(v) ? null : v;
    });
    return [b.t, ...values];
  });
}
