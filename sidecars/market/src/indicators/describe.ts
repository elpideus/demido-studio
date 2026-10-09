// What a TradingView indicator draws, described for Demido's chart.
//
// TradingView runs every indicator itself (studies.ts). What comes back is a value per plot and
// bar, plus the labels, lines, boxes and tables of scripts that draw. How to draw them lives in
// the script's metadata (pine-facade "translate" → metaInfo) and, for an indicator imported from a
// saved chart layout, in that layout's state. This file turns both into plain data the chart
// works from. It is pure, so it is unit-tested.

type Obj = Record<string, unknown>;

const obj = (v: unknown): Obj => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Obj) : {});
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const str = (v: unknown): string => (typeof v === 'string' ? v : '');
const num = (v: unknown): number | undefined => (typeof v === 'number' && Number.isFinite(v) ? v : undefined);

/** A plot value, with TradingView's "na" (1e100) and anything non-finite as null. */
export function value(v: unknown): number | null {
  return typeof v === 'number' && Number.isFinite(v) && Math.abs(v) < 1e99 ? v : null;
}

export type PlotKind = 'line' | 'step' | 'area' | 'histogram' | 'columns' | 'circles' | 'cross' | 'shapes';

export interface PlotInfo {
  id: string;
  title: string;
  kind: PlotKind;
  color: string;
  width: number;
  /** 0 solid, 1 dotted, 2 dashed (TradingView's numbering). */
  dash: number;
  /** Plot whose value picks this plot's color on each bar (a Pine `color=` expression). */
  colorer?: string;
  /** Colorer value → color. */
  colors?: Record<string, string>;
  /** Shapes: TradingView's shape name (`triangle_up`, `label_down`, `circle`, `char`, `arrow`…). */
  shape?: string;
  /** Shapes: where they sit; `absolute` puts them at the plot value. */
  location?: 'abovebar' | 'belowbar' | 'top' | 'bottom' | 'absolute';
  text?: string;
  textColor?: string;
  /** Arrows: the color of negative values. */
  downColor?: string;
  /** Histograms and columns: the value bars grow from. */
  base?: number;
  /** Not drawn (the script's or the layout's `display` is off); its values still come. */
  hidden?: true;
}

export interface BandInfo {
  id: string;
  title: string;
  value: number;
  color: string;
  width: number;
  dash: number;
  /** Not drawn (switched off in the layout). */
  hidden?: true;
}

export interface InputInfo {
  id: string;
  name: string;
  /** TradingView's input type: integer, float, bool, text, source, resolution, color, symbol… */
  type: string;
  value: unknown;
  defval: unknown;
  /** Not shown in TradingView's settings either (internal inputs). */
  hidden: boolean;
  /** TradingView's `isFake` flag, sent back with the value. */
  fake: boolean;
  min?: number;
  max?: number;
  step?: number;
  options?: string[];
  group?: string;
  tooltip?: string;
  /** Shown after the name in the chart's legend, as in TradingView's status line (default: yes). */
  legend?: boolean;
}

export interface IndicatorMeta {
  id: string;
  version: string;
  name: string;
  short: string;
  kind: 'study' | 'strategy';
  /** Drawn over the candles (true) or in a pane of its own. */
  overlay: boolean;
  /** Decimals of its values; null uses the chart's. */
  precision: number | null;
  /** What it draws, in TradingView's order; hidden ones are marked so (the settings show them). */
  plots: PlotInfo[];
  bands: BandInfo[];
  inputs: InputInfo[];
  /** The plot ids each data row carries after its time, in order. */
  columns: string[];
  /** Colors of the script's labels, lines, boxes and tables, by index. */
  palette: string[];
}

/** How a saved chart layout (or the user, in Demido) set an indicator up. */
export interface LayoutState {
  inputs?: Record<string, unknown>;
  styles?: Record<string, Obj>;
  palettes?: Record<string, Obj>;
  bands?: unknown[];
  /** Pane placement, when it differs from the script's own. */
  overlay?: boolean;
}

/** TradingView's plot styles (the charting library's LineStudyPlotStyle). */
const PLOT_TYPES: Record<number, PlotKind> = {
  0: 'line',
  1: 'histogram',
  3: 'cross',
  4: 'area',
  5: 'columns',
  6: 'circles',
  7: 'line',
  8: 'area',
  9: 'step',
  10: 'step',
  11: 'step',
};

const FALLBACK_COLOR = '#2962ff';

/** Inputs Demido never shows or sends: the script itself and its id. */
const SCRIPT_INPUTS = new Set(['text', 'pineId', 'pineVersion']);

function inputsOf(info: Obj, values: Record<string, unknown> | undefined): InputInfo[] {
  const out: InputInfo[] = [];
  for (const raw of arr(info.inputs)) {
    const i = obj(raw);
    const id = str(i.id);
    if (!id || SCRIPT_INPUTS.has(id)) continue;
    const input: InputInfo = {
      id,
      name: str(i.name) || id,
      type: str(i.type) || 'text',
      value: values && id in values ? values[id] : i.defval,
      defval: i.defval,
      hidden: i.isHidden === true,
      fake: i.isFake === true,
    };
    const min = num(i.min);
    const max = num(i.max);
    const step = num(i.step);
    if (min !== undefined) input.min = min;
    if (max !== undefined) input.max = max;
    if (step !== undefined) input.step = step;
    const options = arr(i.options).filter((o) => typeof o === 'string' || typeof o === 'number');
    if (options.length) input.options = options.map(String);
    if (str(i.group)) input.group = str(i.group);
    if (str(i.tooltip)) input.tooltip = str(i.tooltip);
    // Pine's `display = display.none` keeps an input out of the status line.
    if (i.display === 0) input.legend = false;
    out.push(input);
  }
  return out;
}

/** Coerces values from the UI to each input's type; unknown ids and bad numbers are dropped. */
export function inputValues(inputs: readonly InputInfo[], values: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  // The assistant may name inputs by their title ("Length"); an id wins over a title.
  const byTitle = new Map(Object.entries(values).map(([k, v]) => [k.trim().toLowerCase(), v]));
  for (const input of inputs) {
    const v = input.id in values ? values[input.id] : byTitle.get(input.name.trim().toLowerCase());
    if (v === undefined) continue;
    if (input.type === 'integer' || input.type === 'float' || input.type === 'price') {
      let n = Number(v);
      if (!Number.isFinite(n)) continue;
      if (input.type === 'integer') n = Math.round(n);
      if (input.min !== undefined) n = Math.max(input.min, n);
      if (input.max !== undefined) n = Math.min(input.max, n);
      out[input.id] = n;
    } else if (input.type === 'bool') {
      out[input.id] = v === true || v === 'true';
    } else if (input.type === 'time') {
      const n = Number(v);
      if (Number.isFinite(n)) out[input.id] = n;
    } else if (typeof v === 'string' || typeof v === 'number') {
      out[input.id] = String(v);
    }
  }
  return out;
}

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** The input id behind a metadata expression such as `rm_ic_0`, when it is a plain input. */
function inputBehind(tree: string, root: string): string | null {
  const r = new RegExp(`##root\\(root_metainfo,${escapeRe(root)},([A-Za-z_]\\w*),`).exec(tree);
  if (!r) return null;
  const v = new RegExp(`(?:^|\\n)${escapeRe(r[1]!)}=##input\\([^\\n]*?##id='([^']+)'`).exec(tree);
  return v ? v[1]! : null;
}

function setPath(target: Obj, path: string[], v: unknown): void {
  let at: Obj = target;
  for (const key of path.slice(0, -1)) {
    if (!at[key] || typeof at[key] !== 'object') at[key] = {};
    at = at[key] as Obj;
  }
  at[path[path.length - 1]!] = v;
}

/**
 * The script's defaults with its color inputs applied. Scripts declare a color input once and use
 * it in their palettes and plot styles; the metadata says where (TVScriptMetaInfoExprs).
 */
function patchedDefaults(info: Obj, inputs: readonly InputInfo[]): Obj {
  const defaults = obj(info.defaults);
  const exprs = obj(info.TVScriptMetaInfoExprs);
  const map = obj(exprs.patchMap);
  const tree = str(exprs.tree);
  if (!tree || !Object.keys(map).length) return defaults;
  const out = structuredClone(defaults);
  const byId = new Map(inputs.map((i) => [i.id, i.value]));
  for (const [path, root] of Object.entries(map)) {
    if (!path.startsWith('defaults.')) continue;
    const id = inputBehind(tree, String(root));
    const v = id ? byId.get(id) : undefined;
    if (typeof v !== 'string' && typeof v !== 'number' && typeof v !== 'boolean') continue;
    setPath(out, path.slice('defaults.'.length).split('.'), v);
  }
  return out;
}

/** Override fields that are set (layouts store null for "as the script says"). */
function present(o: Obj): Obj {
  return Object.fromEntries(Object.entries(o).filter(([, v]) => v !== null && v !== undefined));
}

function mergePalettes(defaults: Obj, overrides: Record<string, Obj> | undefined): Record<string, Record<string, Obj>> {
  const out: Record<string, Record<string, Obj>> = {};
  for (const name of new Set([...Object.keys(defaults), ...Object.keys(overrides ?? {})])) {
    const base = obj(obj(defaults[name]).colors);
    const over = obj(obj(overrides?.[name]).colors);
    const colors: Record<string, Obj> = {};
    for (const n of new Set([...Object.keys(base), ...Object.keys(over)])) {
      colors[n] = { ...obj(base[n]), ...present(obj(over[n])) };
    }
    out[name] = colors;
  }
  return out;
}

/** Applies a 0–100 transparency (older scripts store it next to the color). */
export function withTransparency(color: string, transparency: number | undefined): string {
  if (!transparency || transparency <= 0) return color;
  const alpha = Math.max(0, Math.min(1, 1 - transparency / 100));
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.exec(color);
  if (hex) {
    let h = hex[1]!;
    if (h.length === 3) h = [...h].map((c) => c + c).join('');
    const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
    const a = h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1;
    return `rgba(${r},${g},${b},${+(a * alpha).toFixed(3)})`;
  }
  const rgba = /^rgba?\(([^)]+)\)$/i.exec(color);
  if (rgba) {
    const parts = rgba[1]!.split(',').map((p) => p.trim());
    const a = parts[3] !== undefined ? Number(parts[3]) : 1;
    return `rgba(${parts.slice(0, 3).join(',')},${+(a * alpha).toFixed(3)})`;
  }
  return color;
}

const SHAPE_LOCATIONS = new Set(['abovebar', 'belowbar', 'top', 'bottom', 'absolute']);

/** Describes a script from its metaInfo and, optionally, how a layout or the user set it up. */
export function describe(info: Obj, state: LayoutState = {}): IndicatorMeta {
  const inputs = inputsOf(info, state.inputs);
  const defaults = patchedDefaults(info, inputs);
  const baseStyles = obj(defaults.styles);
  const plotStyle = (id: string): Obj => ({ ...obj(baseStyles[id]), ...present(obj(state.styles?.[id])) });
  const scriptStyles = obj(info.styles);
  const palettes = mergePalettes(obj(defaults.palettes), state.palettes);
  const paletteInfo = obj(info.palettes);

  const plotsRaw = arr(info.plots).map(obj);
  // A colorer plot targets the plot it colors; its values go through the palette's valToIndex.
  const colorers = new Map<string, Obj>();
  for (const p of plotsRaw) {
    if (p.type === 'colorer' && str(p.target)) colorers.set(str(p.target), p);
  }
  const colorsOf = (palette: string): Record<string, string> => {
    const colors = palettes[palette] ?? {};
    const valToIndex = obj(obj(paletteInfo[palette]).valToIndex);
    const out: Record<string, string> = {};
    const pairs = Object.keys(valToIndex).length
      ? Object.entries(valToIndex).map(([v, i]) => [v, String(i)] as const)
      : Object.keys(colors).map((i) => [i, i] as const);
    for (const [v, i] of pairs) {
      const c = str(colors[i]?.color);
      if (c) out[v] = c;
    }
    return out;
  };

  const plots: PlotInfo[] = [];
  for (const p of plotsRaw) {
    const id = str(p.id);
    const type = str(p.type);
    if (!id || !['line', 'shapes', 'chars', 'arrows'].includes(type)) continue;
    const s = plotStyle(id);
    const display = num(s.display) ?? 15;
    const title = str(obj(scriptStyles[id]).title) || id;
    const transparency = num(s.transparency);
    const color = withTransparency(str(s.color) || FALLBACK_COLOR, transparency);
    const plot: PlotInfo = {
      id,
      title,
      kind: 'line',
      color,
      width: num(s.linewidth) ?? 1,
      dash: num(s.linestyle) ?? 0,
    };
    if (type === 'line') {
      plot.kind = PLOT_TYPES[num(s.plottype) ?? 0] ?? 'line';
      const base = num(obj(scriptStyles[id]).histogramBase);
      if (base !== undefined && base !== 0) plot.base = base;
    } else {
      plot.kind = 'shapes';
      const location = str(s.location).toLowerCase();
      plot.location = (SHAPE_LOCATIONS.has(location) ? location : 'abovebar') as PlotInfo['location'];
      if (type === 'chars') {
        plot.shape = 'char';
        plot.text = str(s.char) || '★';
      } else if (type === 'arrows') {
        plot.shape = 'arrow';
        plot.color = withTransparency(str(s.colorup) || '#089981', transparency);
        plot.downColor = withTransparency(str(s.colordown) || '#f23645', transparency);
        plot.location = 'abovebar';
      } else {
        plot.shape = str(s.plottype).replace(/^shape_/, '') || 'circle';
      }
      const text = str(obj(scriptStyles[id]).text).trim();
      if (text && type !== 'chars') plot.text = text;
      if (str(s.textColor)) plot.textColor = str(s.textColor);
    }
    const colorer = colorers.get(id);
    if (colorer && str(colorer.palette)) {
      plot.colorer = str(colorer.id);
      plot.colors = colorsOf(str(colorer.palette));
    }
    if ((display & 1) === 0) plot.hidden = true;
    plots.push(plot);
  }

  const bandDefaults = arr(defaults.bands).map(obj);
  const bands: BandInfo[] = [];
  arr(info.bands).forEach((raw, i) => {
    const b = obj(raw);
    const d = { ...bandDefaults[i], ...present(obj(arr(state.bands)[i])) };
    const v = num(d.value);
    if (v === undefined || b.isHidden === true) return;
    const band: BandInfo = {
      id: str(b.id) || `hline_${i}`,
      title: str(b.name),
      value: v,
      color: str(d.color) || 'rgba(120,123,134,0.6)',
      width: num(d.linewidth) ?? 1,
      dash: num(d.linestyle) ?? 2,
    };
    if (d.visible === false) band.hidden = true;
    bands.push(band);
  });

  const common = palettes.palette_common ?? {};
  const palette: string[] = [];
  for (const [n, c] of Object.entries(common)) palette[Number(n)] = str(c.color);

  const format = obj(info.format);
  const precision =
    (format.type === 'price' || format.type === 'percent') && num(format.precision) !== undefined
      ? num(format.precision)!
      : format.type === 'volume'
        ? 0
        : null;

  const columns: string[] = [];
  for (const p of plots) {
    columns.push(p.id);
    if (p.colorer) columns.push(p.colorer);
  }

  const pine = obj(info.pine);
  return {
    id: str(info.scriptIdPart) || str(info.id),
    version: str(pine.version) || str(info.version),
    name: str(info.description) || str(info.shortDescription) || 'Indicator',
    short: str(info.shortDescription) || str(info.description),
    kind: info.isTVScriptStrategy === true ? 'strategy' : 'study',
    overlay: state.overlay ?? info.is_price_study === true,
    precision,
    plots,
    bands,
    inputs,
    columns,
    palette,
  };
}

/** A data row: bar time (seconds), then one value per column. */
export type Row = [number, ...(number | null)[]];

/** Rows, oldest first, from the library's periods (newest first, keyed `$time`, `plot_N`). */
export function rowsOf(periods: readonly Obj[], columns: readonly string[]): Row[] {
  const rows: Row[] = [];
  for (const p of periods) {
    const t = num(p.$time);
    if (t === undefined) continue;
    rows.push([t, ...columns.map((c) => value(p[c]))]);
  }
  return rows.sort((a, b) => a[0] - b[0]);
}

// -------------------------------------------------------------------------------------------
// Drawings (Pine's label.new, line.new, box.new, table.new)

export interface Label {
  id: number;
  t: number;
  y: number | null;
  yloc: 'price' | 'abovebar' | 'belowbar';
  text: string;
  style: string;
  color: string | null;
  textColor: string | null;
  size: string;
  tooltip?: string;
}

export interface Segment {
  id: number;
  t1: number;
  y1: number;
  t2: number;
  y2: number;
  extend: 'none' | 'left' | 'right' | 'both';
  style: string;
  color: string | null;
  width: number;
}

export interface Box {
  id: number;
  t1: number;
  y1: number;
  t2: number;
  y2: number;
  color: string | null;
  bg: string | null;
  extend: 'none' | 'left' | 'right' | 'both';
  style: string;
  width: number;
  text: string;
  textColor: string | null;
  textSize: string;
  halign: string;
  valign: string;
}

export interface TableCell {
  row: number;
  col: number;
  text: string;
  textColor: string | null;
  bg: string | null;
  size: string;
  halign: string;
  valign: string;
  colspan: number;
  rowspan: number;
  tooltip?: string;
}

export interface Table {
  id: number;
  position: string;
  rows: number;
  columns: number;
  bg: string | null;
  frame: string | null;
  frameWidth: number;
  border: string | null;
  borderWidth: number;
  cells: TableCell[];
}

export interface Graphics {
  labels: Label[];
  lines: Segment[];
  boxes: Box[];
  tables: Table[];
}

/** Most drawings of one kind sent to the chart (Pine's own limit is 500). */
const MAX_DRAWINGS = 500;

/** A drawing color: an index into the script's palette, a color string, or none. */
export function colorOf(v: unknown, palette: readonly string[]): string | null {
  if (typeof v === 'number') return palette[v] || null;
  if (typeof v === 'string' && v) return v;
  return null;
}

const EXTENDS = new Set(['none', 'left', 'right', 'both']);
const extendOf = (v: unknown): Segment['extend'] => (EXTENDS.has(str(v)) ? (str(v) as Segment['extend']) : 'none');

/**
 * Places a two-point drawing in time. `x1`/`x2` are bar positions counted back from the newest
 * bar (what TradingView-API's parser produces); `timeAt` turns them into bar times. An end that
 * falls outside the loaded bars is pinned to the nearest loaded bar when the drawing is flat (a
 * level or a box), so it still shows; a sloped line with a missing end is dropped.
 */
function span(
  x1: unknown,
  x2: unknown,
  flat: boolean,
  timeAt: (barsBack: number) => number | undefined,
  oldest: number,
  newest: number,
): { t1: number; t2: number; pinnedRight: boolean } | null {
  const a = typeof x1 === 'number' ? timeAt(x1) : undefined;
  const b = typeof x2 === 'number' ? timeAt(x2) : undefined;
  if (a !== undefined && b !== undefined) return { t1: Math.min(a, b), t2: Math.max(a, b), pinnedRight: false };
  if (!flat || (a === undefined && b === undefined)) return null;
  // Drawings run left to right, so a missing right end lies in the future and a missing left
  // end before the loaded history.
  if (a !== undefined) return { t1: a, t2: newest, pinnedRight: true };
  return { t1: oldest, t2: b!, pinnedRight: false };
}

/**
 * The drawings of a study in chart terms. `parsed` is TradingView-API's `study.graphic` (bar
 * positions as "bars back"); `raw` its `graphic.raw()`, read for table cells.
 */
export function graphicsOf(
  parsed: Obj,
  raw: Obj,
  palette: readonly string[],
  timeAt: (barsBack: number) => number | undefined,
  oldest: number,
  newest: number,
): Graphics {
  const labels: Label[] = [];
  for (const l of arr(parsed.labels).map(obj)) {
    const t = typeof l.x === 'number' ? timeAt(l.x) : undefined;
    if (t === undefined) continue;
    const yloc = str(l.yLoc);
    const text = str(l.text);
    const label: Label = {
      id: num(l.id) ?? 0,
      t,
      y: value(l.y),
      yloc: yloc === 'abovebar' || yloc === 'belowbar' ? yloc : 'price',
      text,
      style: str(l.style) || 'label_down',
      color: colorOf(l.color, palette),
      textColor: colorOf(l.textColor, palette),
      size: str(l.size) || 'normal',
    };
    if (str(l.toolTip)) label.tooltip = str(l.toolTip);
    if (label.yloc === 'price' && label.y === null) continue;
    labels.push(label);
  }

  const lines: Segment[] = [];
  for (const l of arr(parsed.lines).map(obj)) {
    const y1 = value(l.y1);
    const y2 = value(l.y2);
    if (y1 === null || y2 === null) continue;
    const at = span(l.x1, l.x2, y1 === y2, timeAt, oldest, newest);
    if (!at) continue;
    const ext = extendOf(l.extend);
    lines.push({
      id: num(l.id) ?? 0,
      t1: at.t1,
      y1,
      t2: at.t2,
      y2,
      extend: at.pinnedRight ? (ext === 'left' || ext === 'both' ? 'both' : 'right') : ext,
      style: str(l.style) || 'solid',
      color: colorOf(l.color, palette),
      width: num(l.width) ?? 1,
    });
  }

  const boxes: Box[] = [];
  for (const b of arr(parsed.boxes).map(obj)) {
    const y1 = value(b.y1);
    const y2 = value(b.y2);
    if (y1 === null || y2 === null) continue;
    const at = span(b.x1, b.x2, true, timeAt, oldest, newest);
    if (!at) continue;
    const ext = extendOf(b.extend);
    boxes.push({
      id: num(b.id) ?? 0,
      t1: at.t1,
      y1: Math.max(y1, y2),
      t2: at.t2,
      y2: Math.min(y1, y2),
      color: colorOf(b.color, palette),
      bg: colorOf(b.bgColor, palette),
      extend: at.pinnedRight ? (ext === 'left' || ext === 'both' ? 'both' : 'right') : ext,
      style: str(b.style) || 'solid',
      width: num(b.width) ?? 1,
      text: str(b.text),
      textColor: colorOf(b.textColor, palette),
      textSize: str(b.textSize) || 'auto',
      halign: str(b.textHAlign) || 'center',
      valign: str(b.textVAlign) || 'center',
    });
  }

  const cellsByTable = new Map<number, TableCell[]>();
  for (const c of Object.values(obj(raw.dwgtablecells)).map(obj)) {
    const tid = num(c.tid);
    if (tid === undefined) continue;
    const cell: TableCell = {
      row: num(c.row) ?? 0,
      col: num(c.col) ?? 0,
      text: str(c.t),
      textColor: colorOf(c.tc, palette),
      bg: colorOf(c.bgc, palette),
      size: str(c.ts) || 'normal',
      halign: str(c.tha) || 'center',
      valign: str(c.tva) || 'center',
      colspan: num(c.colspan) ?? 1,
      rowspan: num(c.rowspan) ?? 1,
    };
    if (str(c.tt)) cell.tooltip = str(c.tt);
    const list = cellsByTable.get(tid) ?? [];
    list.push(cell);
    cellsByTable.set(tid, list);
  }
  const tables: Table[] = [];
  for (const t of Object.values(obj(raw.dwgtables)).map(obj)) {
    const id = num(t.id) ?? 0;
    tables.push({
      id,
      position: str(t.pos) || 'top_right',
      rows: num(t.rows) ?? 0,
      columns: num(t.cols) ?? 0,
      bg: colorOf(t.bgc, palette),
      frame: colorOf(t.frmc, palette),
      frameWidth: num(t.frmw) ?? 0,
      border: colorOf(t.brdc, palette),
      borderWidth: num(t.brdw) ?? 0,
      cells: (cellsByTable.get(id) ?? []).sort((a, b) => a.row - b.row || a.col - b.col),
    });
  }

  const newestFirst = <T extends { id: number }>(list: T[], at: (x: T) => number) =>
    list.sort((a, b) => at(b) - at(a) || b.id - a.id).slice(0, MAX_DRAWINGS);
  return {
    labels: newestFirst(labels, (l) => l.t),
    lines: newestFirst(lines, (l) => l.t2),
    boxes: newestFirst(boxes, (b) => b.t2),
    tables,
  };
}

// -------------------------------------------------------------------------------------------
// Pages of tradingview.com

/** The ids in the user's Favorites of TradingView's Indicators menu (from a chart page). */
export function parseFavorites(html: string): string[] {
  const m = /"studyMarket\.favorites"\s*:\s*("(?:[^"\\]|\\.)*")/.exec(html);
  if (!m) return [];
  try {
    const list: unknown = JSON.parse(JSON.parse(m[1]!) as string);
    return Array.isArray(list) ? list.filter((x): x is string => typeof x === 'string' && x.includes(';')) : [];
  } catch {
    return [];
  }
}

export interface LayoutStudy {
  id: string;
  version: string;
  name: string;
  /** Hidden in the layout (its eye was off). */
  hidden: boolean;
  state: LayoutState;
}

export interface Layout {
  name: string;
  symbol: string;
  interval: string;
  studies: LayoutStudy[];
  /** Names of what could not be brought over (TradingView's older built-in studies). */
  skipped: string[];
}

/** Reads the value of `initData.<key> = …;` from a chart page. */
function initData(html: string, key: string): unknown {
  const marker = `initData.${key} =`;
  const start = html.indexOf(marker);
  if (start < 0) return undefined;
  const end = html.indexOf('\n', start);
  let text = html.slice(start + marker.length, end < 0 ? undefined : end).trim();
  if (text.endsWith(';')) text = text.slice(0, -1);
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

const SCRIPT_META = /^Script\$(.+)@tv-scripting-101(?:\[v\.([\d.]+)\])?/;

/** The indicators of a saved chart layout, from its chart page. */
export function parseLayout(html: string): Layout | null {
  const content = obj(initData(html, 'content'));
  const chart = obj(arr(content.charts)[0]);
  const panes = arr(chart.panes).map(obj);
  if (!panes.length) return null;
  const metaMap = obj(initData(html, 'study_meta_info_map'));
  const mainPane = Math.max(
    0,
    panes.findIndex((p) => arr(p.sources).some((s) => obj(s).type === 'MainSeries')),
  );
  const studies: LayoutStudy[] = [];
  const skipped: string[] = [];
  let symbol = '';
  let interval = '';
  panes.forEach((pane, index) => {
    for (const source of arr(pane.sources).map(obj)) {
      const state = obj(source.state);
      if (source.type === 'MainSeries') {
        symbol = str(state.symbol);
        interval = str(state.interval);
        continue;
      }
      if (source.type !== 'Study') continue;
      const metaKey = str(source.metaInfo);
      const m = SCRIPT_META.exec(metaKey);
      const description = str(obj(metaMap[metaKey]).description) || str(obj(metaMap[metaKey]).shortDescription);
      if (!m) {
        // Volume and the like belong to TradingView's own chart; Demido draws volume itself.
        if (!/^Volume@/.test(metaKey)) skipped.push(description || metaKey.split('@')[0] || 'Unknown study');
        continue;
      }
      studies.push({
        id: m[1]!,
        version: m[2] ?? 'last',
        name: description || m[1]!,
        hidden: state.visible === false,
        state: {
          inputs: Object.fromEntries(
            Object.entries(obj(state.inputs)).filter(
              ([k, v]) => !SCRIPT_INPUTS.has(k) && v !== null && typeof v !== 'object',
            ),
          ),
          styles: obj(state.styles) as Record<string, Obj>,
          palettes: obj(state.palettes) as Record<string, Obj>,
          bands: arr(state.bands),
          overlay: index === mainPane,
        },
      });
    }
  });
  return { name: str(content.name), symbol, interval, studies, skipped };
}
