// What the assistant puts on the Market window's chart (`chart_add_indicator`, `chart_draw`), as
// changes to the window's props; and its drawings: named sets drawn like an indicator computed
// here, series as plots and the rest as a script's labels, lines and boxes, on the chart's bars.
// Pure, so it is unit-tested.

import type {
  ChartDrawing,
  ChartDrawingItem,
  IndicatorBox,
  IndicatorGraphics,
  IndicatorLabel,
  IndicatorLine,
  IndicatorMeta,
  IndicatorPlot,
  IndicatorRow,
} from '@/lib/types';
import type { ChartCommand } from '@/lib/events';
import { fade } from './indicatorView';
import { fromScript, savedIndicators } from './savedIndicators';

/** Legend keys of drawing sets; indicator keys never contain a colon. */
export const DRAWING_KEY = 'draw:';
export const drawingKey = (name: string) => `${DRAWING_KEY}${name}`;

const SERIES_COLORS = ['#2962ff', '#ff9800', '#e91e63', '#00bcd4', '#9c27b0', '#4caf50'];
const BLUE = '#2962ff';
const GREEN = '#089981';
const RED = '#f23645';
const TYPES = new Set(['hline', 'line', 'box', 'label', 'marker', 'series']);

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);

/** The drawing sets in a window's props; anything malformed is left out. */
export function drawingsOf(value: unknown): ChartDrawing[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const out: ChartDrawing[] = [];
  for (const v of value) {
    if (!isObj(v) || typeof v.name !== 'string' || !v.name || seen.has(v.name) || !Array.isArray(v.items)) continue;
    seen.add(v.name);
    out.push({
      name: v.name,
      pane: v.pane === 'separate' ? 'separate' : 'overlay',
      items: v.items.filter((i): i is ChartDrawingItem => isObj(i) && TYPES.has(String(i.type))),
      ...(typeof v.symbol === 'string' && v.symbol ? { symbol: v.symbol } : {}),
      ...(v.hidden === true ? { hidden: true } : {}),
    });
  }
  return out;
}

/** The sets with `drawing` in place of the one of its name (shown again), or added last. */
export function withDrawing(list: readonly ChartDrawing[], drawing: ChartDrawing): ChartDrawing[] {
  const fresh: ChartDrawing = {
    name: drawing.name,
    pane: drawing.pane,
    items: drawing.items,
    ...(drawing.symbol ? { symbol: drawing.symbol } : {}),
  };
  const at = list.findIndex((d) => d.name === drawing.name);
  if (at < 0) return [...list, fresh];
  return list.map((d, i) => (i === at ? fresh : d));
}

/** The sets without the one named `name` ("*": without any). */
export function withoutDrawing(list: readonly ChartDrawing[], name: string): ChartDrawing[] {
  return name === '*' ? [] : list.filter((d) => d.name !== name);
}

/** Whether two symbols name the same market (`FX:EURUSD` and `EURUSD` do). */
export function sameMarket(a: string, b: string): boolean {
  const ticker = (s: string) => (s.includes(':') ? s.slice(s.indexOf(':') + 1) : s).trim().toUpperCase();
  return a.trim().toUpperCase() === b.trim().toUpperCase() || ticker(a) === ticker(b);
}

/** Whether a set shows on the chart of `symbol`. */
export function drawnOn(d: ChartDrawing, symbol: string): boolean {
  return !d.symbol || sameMarket(d.symbol, symbol);
}

/**
 * The Market window's props after one of the assistant's commands: the chart shown, on the
 * command's symbol and timeframe, with the indicator added (or shown again, if it was there
 * already as it is set up now) or the drawings replaced.
 */
export function chartPatch(props: Record<string, unknown>, cmd: ChartCommand): Record<string, unknown> {
  if (cmd.action === 'undraw') return { drawings: withoutDrawing(drawingsOf(props.drawings), cmd.name) };
  const patch: Record<string, unknown> = { tab: 'chart' };
  if (cmd.symbol) patch.symbol = cmd.symbol;
  if (cmd.timeframe) patch.timeframe = cmd.timeframe;
  if (cmd.action === 'draw') {
    const symbol = cmd.symbol ?? (typeof props.symbol === 'string' ? props.symbol : undefined);
    patch.drawings = withDrawing(drawingsOf(props.drawings), { ...cmd.drawing, ...(symbol ? { symbol } : {}) });
    return patch;
  }
  const saved = savedIndicators(props.indicators);
  const inputs = cmd.inputs && Object.keys(cmd.inputs).length ? cmd.inputs : undefined;
  const setup = inputs ? { inputs } : {};
  const same = saved.find((s) => s.script === cmd.script && JSON.stringify(s.setup) === JSON.stringify(setup));
  if (same) {
    patch.indicators = saved.map((s) => (s === same ? { ...s, hidden: undefined } : s));
    return patch;
  }
  const taken = new Set(saved.map((s) => s.key));
  patch.indicators = [...saved, fromScript(cmd.script, cmd.name ?? cmd.script, taken, inputs)];
  return patch;
}

const dashOf = (style: string | undefined) => (style === 'dotted' ? 1 : style === 'dashed' ? 2 : 0);
const widthOf = (w: number | undefined, fallback: number) =>
  typeof w === 'number' && Number.isFinite(w) ? Math.min(4, Math.max(1, Math.round(w))) : fallback;

/** A set described like an indicator: one plot per series. */
export function drawingMeta(d: ChartDrawing): IndicatorMeta {
  const plots: IndicatorPlot[] = [];
  d.items.forEach((item, i) => {
    if (item.type !== 'series') return;
    plots.push({
      id: `series_${i}`,
      title: item.text || (plots.length ? `${d.name} ${plots.length + 1}` : d.name),
      kind: 'line',
      color: item.color || SERIES_COLORS[plots.length % SERIES_COLORS.length]!,
      width: widthOf(item.width, 2),
      dash: dashOf(item.style),
    });
  });
  return {
    id: drawingKey(d.name),
    version: '',
    name: d.name,
    short: d.name,
    kind: 'study',
    overlay: d.pane !== 'separate',
    precision: null,
    plots,
    bands: [],
    inputs: [],
    columns: plots.map((p) => p.id),
    palette: [],
  };
}

/** The bar a time falls in (bars open at their time): its index, or -1 before the first bar. */
function barIndex(times: readonly number[], t: number): number {
  let lo = 0;
  let hi = times.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (times[mid]! <= t) {
      found = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  return found;
}

/** A time on the chart: the bar it falls in, kept within the chart's bars. */
function snap(times: readonly number[], t: number): number {
  const i = barIndex(times, t);
  return times[Math.max(0, i)]!;
}

/** The marker shape names the chart's drawings know. */
function markerStyle(shape: string | undefined, below: boolean): string {
  switch (shape) {
    case 'arrow_up':
      return 'arrowup';
    case 'arrow_down':
      return 'arrowdown';
    case 'triangle_up':
      return 'triangleup';
    case 'triangle_down':
      return 'triangledown';
    case 'circle':
    case 'square':
    case 'cross':
    case 'diamond':
    case 'flag':
      return shape;
    default:
      return below ? 'triangleup' : 'triangledown';
  }
}

/**
 * What a set draws on bars at `times` (the chart's, oldest first): rows for its series, and its
 * levels, lines, boxes, labels and markers as a script's drawings. Times fall in the bar they are
 * in; what lies wholly outside the bars is left out, and lines are cut where the bars end.
 */
export function drawingView(
  d: ChartDrawing,
  times: readonly number[],
): { rows: IndicatorRow[]; graphics: IndicatorGraphics } {
  const graphics: IndicatorGraphics = { labels: [], lines: [], boxes: [], tables: [] };
  if (!times.length) return { rows: [], graphics };
  const first = times[0]!;
  const last = times[times.length - 1]!;
  // The end of the last bar, as far as it can be told: a time in it is still in it.
  const end = last + (times.length > 1 ? last - times[times.length - 2]! : 0);
  const inside = (t: number) => t >= first && t <= end;

  const series: Array<Array<number | null>> = [];
  let id = 0;
  for (const item of d.items) {
    const color = item.color || undefined;
    const width = widthOf(item.width, 1);
    const style = item.style ?? 'solid';
    const text = item.text ?? '';
    switch (item.type) {
      case 'series': {
        const values: Array<number | null> = times.map(() => null);
        for (const [t, v] of item.points ?? []) {
          if (typeof t !== 'number' || !inside(t)) continue;
          values[barIndex(times, t)] = typeof v === 'number' && Number.isFinite(v) ? v : null;
        }
        series.push(values);
        break;
      }
      case 'hline': {
        if (typeof item.price !== 'number') break;
        const line: IndicatorLine = {
          id: id++,
          t1: first,
          y1: item.price,
          t2: last,
          y2: item.price,
          extend: 'both',
          style,
          color: color ?? BLUE,
          width,
        };
        graphics.lines.push(line);
        if (text)
          graphics.labels.push(label(id++, last, item.price, 'price', text, 'label_left', color ?? BLUE, 'small'));
        break;
      }
      case 'line': {
        const { time: t1, price: y1, time2: t2, price2: y2 } = item;
        if (typeof t1 !== 'number' || typeof t2 !== 'number' || typeof y1 !== 'number' || typeof y2 !== 'number') break;
        const [a, b] =
          t1 <= t2
            ? [
                { t: t1, y: y1 },
                { t: t2, y: y2 },
              ]
            : [
                { t: t2, y: y2 },
                { t: t1, y: y1 },
              ];
        if (b.t < first || a.t > end) break;
        // Cut where the bars end, along the line.
        const at = (t: number) => (b.t === a.t ? a.y : a.y + ((b.y - a.y) * (t - a.t)) / (b.t - a.t));
        const from = a.t < first ? { t: first, y: at(first) } : a;
        const to = b.t > last ? { t: last, y: at(last) } : b;
        graphics.lines.push({
          id: id++,
          t1: snap(times, from.t),
          y1: from.y,
          t2: snap(times, to.t),
          y2: to.y,
          extend: item.extend ?? 'none',
          style,
          color: color ?? BLUE,
          width,
        });
        if (text)
          graphics.labels.push(
            label(id++, snap(times, to.t), to.y, 'price', text, 'label_left', color ?? BLUE, 'small'),
          );
        break;
      }
      case 'box': {
        const { time: t1, price: y1, time2: t2, price2: y2 } = item;
        if (typeof t1 !== 'number' || typeof t2 !== 'number' || typeof y1 !== 'number' || typeof y2 !== 'number') break;
        const left = Math.min(t1, t2);
        const right = Math.max(t1, t2);
        if (right < first || left > end) break;
        const c = color ?? BLUE;
        const box: IndicatorBox = {
          id: id++,
          t1: snap(times, Math.max(left, first)),
          y1: Math.max(y1, y2),
          t2: snap(times, Math.min(right, last)),
          y2: Math.min(y1, y2),
          color: c,
          bg: fade(c, 0.18),
          extend: item.extend ?? 'none',
          style,
          width,
          text,
          textColor: c,
          textSize: 'small',
          halign: 'center',
          valign: 'top',
        };
        graphics.boxes.push(box);
        break;
      }
      case 'label': {
        if (typeof item.time !== 'number' || !inside(item.time) || !text) break;
        const t = snap(times, item.time);
        const c = color ?? BLUE;
        if (item.position === 'above' || item.position === 'below') {
          const above = item.position === 'above';
          graphics.labels.push(
            label(id++, t, null, above ? 'abovebar' : 'belowbar', text, above ? 'label_down' : 'label_up', c, 'normal'),
          );
        } else if (typeof item.price === 'number') {
          graphics.labels.push(label(id++, t, item.price, 'price', text, 'label_down', c, 'normal'));
        }
        break;
      }
      case 'marker': {
        if (typeof item.time !== 'number' || !inside(item.time)) break;
        const up = /_up$/.test(item.shape ?? '');
        const down = /_down$/.test(item.shape ?? '');
        const below = item.position ? item.position === 'below' : up;
        // Green for a buy-like marker (pointing up, or below the bar), red otherwise.
        const c = color ?? (up || (!down && below) ? GREEN : RED);
        const marker = label(
          id++,
          snap(times, item.time),
          null,
          below ? 'belowbar' : 'abovebar',
          text,
          markerStyle(item.shape, below),
          c,
          'small',
        );
        marker.textColor = c;
        graphics.labels.push(marker);
        break;
      }
      default:
        break;
    }
  }
  const rows: IndicatorRow[] = series.length
    ? times.map((t, k): IndicatorRow => [t, ...series.map((s) => s[k] ?? null)])
    : [];
  return { rows, graphics };
}

function label(
  id: number,
  t: number,
  y: number | null,
  yloc: IndicatorLabel['yloc'],
  text: string,
  style: string,
  color: string,
  size: string,
): IndicatorLabel {
  return { id, t, y, yloc, text, style, color, textColor: '#ffffff', size };
}
