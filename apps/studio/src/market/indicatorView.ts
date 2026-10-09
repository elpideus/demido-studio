// Turning an indicator's rows into what the chart draws: one point per bar and plot, colored per
// bar where the script says so, shapes as series markers, and the legend's text. Pure, so it is
// unit-tested; PriceChart applies the results to lightweight-charts.

import type { IndicatorInput, IndicatorMeta, IndicatorPlot, IndicatorRow } from '@/lib/types';

/** Where each column's value sits in a row (after the time). */
export function columnIndex(meta: Pick<IndicatorMeta, 'columns'>): Map<string, number> {
  return new Map(meta.columns.map((c, i) => [c, i + 1]));
}

/** Rows after an update: `full` replaces them, otherwise rows replace or join by time. Oldest first. */
export function mergeRows(rows: readonly IndicatorRow[], incoming: readonly IndicatorRow[], full: boolean): IndicatorRow[] {
  const sorted = [...incoming].sort((a, b) => a[0] - b[0]);
  if (full || !rows.length) return sorted;
  const out = [...rows];
  for (const row of sorted) {
    const at = search(out, row[0]);
    if (at < out.length && out[at]![0] === row[0]) out[at] = row;
    else out.splice(at, 0, row);
  }
  return out;
}

/** The first index whose time is at or after `t` (rows are oldest first). */
function search(rows: readonly IndicatorRow[], t: number): number {
  let lo = 0;
  let hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (rows[mid]![0] < t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export const TRANSPARENT = 'rgba(0,0,0,0)';

/** A plot's color on one bar: what its colorer picks, else its own. An unset color draws nothing. */
export function colorAt(plot: IndicatorPlot, row: IndicatorRow, columns: ReadonlyMap<string, number>): string {
  if (!plot.colorer) return plot.color;
  const v = row[columns.get(plot.colorer) ?? -1];
  if (v === null || v === undefined) return TRANSPARENT;
  return plot.colors?.[String(v)] ?? plot.color;
}

export interface PlotPoint {
  time: number;
  value?: number;
  color?: string;
}

/** One point per bar for a plot: its value (with its color when the script colors it per bar),
 *  or a gap. `times` are the chart's bars; bars without a row get a gap too. */
export function plotPoints(
  plot: IndicatorPlot,
  rowAt: (t: number) => IndicatorRow | undefined,
  times: readonly number[],
  columns: ReadonlyMap<string, number>,
): PlotPoint[] {
  const col = columns.get(plot.id) ?? -1;
  return times.map((time) => pointAt(plot, rowAt(time), time, col, columns));
}

export function pointAt(
  plot: IndicatorPlot,
  row: IndicatorRow | undefined,
  time: number,
  col: number,
  columns: ReadonlyMap<string, number>,
): PlotPoint {
  const v = row?.[col];
  if (v === null || v === undefined) return { time };
  if (!plot.colorer) return { time, value: v };
  const color = colorAt(plot, row!, columns);
  // An "na" color hides the bar; a gap draws that best (a transparent line still joins).
  if (color === TRANSPARENT && plot.kind !== 'histogram' && plot.kind !== 'columns') return { time };
  return { time, value: v, color };
}

/** A value for each bar to hang a pane's markers on: its first drawn plot's value, carried over gaps. */
export function anchorPoints(
  meta: Pick<IndicatorMeta, 'plots'>,
  rowAt: (t: number) => IndicatorRow | undefined,
  times: readonly number[],
  columns: ReadonlyMap<string, number>,
): Array<{ time: number; value: number }> {
  const cols = meta.plots.filter((p) => p.kind !== 'shapes' && !p.hidden).map((p) => columns.get(p.id) ?? -1);
  const fallback = meta.plots.map((p) => columns.get(p.id) ?? -1);
  let last = 0;
  return times.map((time) => {
    const row = rowAt(time);
    if (row) {
      for (const c of cols.length ? cols : fallback) {
        const v = row[c];
        if (typeof v === 'number') {
          last = v;
          break;
        }
      }
    }
    return { time, value: last };
  });
}

export type MarkerPosition = 'aboveBar' | 'belowBar' | 'inBar' | 'atPriceTop' | 'atPriceBottom' | 'atPriceMiddle';
export type MarkerShape = 'circle' | 'square' | 'arrowUp' | 'arrowDown';

export interface Marker {
  time: number;
  position: MarkerPosition;
  shape: MarkerShape;
  color: string;
  text?: string;
  size?: number;
  price?: number;
}

/** TradingView's shapes in the four lightweight-charts has. */
function markerShape(shape: string | undefined): MarkerShape {
  const s = (shape ?? '').replace(/_/g, '');
  if (/up$/.test(s)) return 'arrowUp';
  if (/down$/.test(s)) return 'arrowDown';
  if (s === 'circle' || s === 'char') return 'circle';
  return 'square';
}

/** The markers of an indicator's shape plots (plotshape, plotchar, plotarrow), oldest first. */
export function shapeMarkers(
  meta: Pick<IndicatorMeta, 'plots'>,
  rows: readonly IndicatorRow[],
  columns: ReadonlyMap<string, number>,
  from = -Infinity,
): Marker[] {
  const plots = meta.plots.filter((p) => p.kind === 'shapes' && !p.hidden);
  if (!plots.length) return [];
  const out: Marker[] = [];
  for (const row of rows) {
    const time = row[0];
    if (time < from) continue;
    for (const plot of plots) {
      const v = row[columns.get(plot.id) ?? -1];
      if (v === null || v === undefined) continue;
      const color = colorAt(plot, row, columns);
      if (color === TRANSPARENT) continue;
      if (plot.shape === 'arrow') {
        if (v === 0) continue;
        out.push({
          time,
          position: v > 0 ? 'belowBar' : 'aboveBar',
          shape: v > 0 ? 'arrowUp' : 'arrowDown',
          color: v > 0 ? color : (plot.downColor ?? color),
        });
        continue;
      }
      const shape = markerShape(plot.shape);
      const marker: Marker = { time, position: 'aboveBar', shape, color };
      if (plot.location === 'absolute') {
        // Labels point at the value: an up label sits below it, a down label above it.
        marker.position = shape === 'arrowUp' ? 'atPriceTop' : shape === 'arrowDown' ? 'atPriceBottom' : 'atPriceMiddle';
        marker.price = v;
      } else {
        // A series bool: drawn where it is true.
        if (v === 0) continue;
        marker.position = plot.location === 'belowbar' || plot.location === 'bottom' ? 'belowBar' : 'aboveBar';
      }
      if (plot.text) marker.text = plot.text;
      if (plot.shape === 'char') marker.size = 0.6;
      out.push(marker);
    }
  }
  return out;
}

export const NUMERIC_INPUTS: ReadonlySet<string> = new Set(['integer', 'float', 'price']);
/** Input types the settings edit; the rest (symbols, sessions, internal objects) keep their value. */
const EDITABLE = new Set([...NUMERIC_INPUTS, 'bool', 'source', 'text', 'string', 'color', 'resolution', 'time']);

/** The inputs an indicator's settings show. */
export function editableInputs(meta: Pick<IndicatorMeta, 'inputs'>): IndicatorInput[] {
  return meta.inputs.filter((i) => !i.hidden && !i.fake && (EDITABLE.has(i.type) || !!i.options?.length));
}

/** The input values TradingView shows after an indicator's name ("RSI 14 close"). */
export function legendInputs(inputs: readonly IndicatorInput[], max = 8): string[] {
  const out: string[] = [];
  for (const i of inputs) {
    if (out.length >= max) break;
    if (i.hidden || i.legend === false || i.type === 'bool' || i.type === 'color') continue;
    const v = i.value;
    if (v === null || v === undefined || v === '' || typeof v === 'object') continue;
    out.push(typeof v === 'number' ? String(+v.toFixed(6)) : String(v));
  }
  return out;
}

/** Decimals for an indicator's values: its own, or the chart's price precision. */
export function precisionOf(meta: Pick<IndicatorMeta, 'precision'>, chartPrecision: number): number {
  return meta.precision ?? chartPrecision;
}

export function formatValue(v: number | null | undefined, precision: number): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return '∅';
  const abs = Math.abs(v);
  // Large values (volume-like) read better grouped and without decimals they do not need.
  if (abs >= 1e6) return v.toLocaleString(undefined, { maximumFractionDigits: 0 });
  return v.toLocaleString(undefined, { minimumFractionDigits: precision, maximumFractionDigits: precision });
}

/** Whether two descriptions draw the same series (only values or input settings differ). */
export function sameShape(a: IndicatorMeta, b: IndicatorMeta): boolean {
  const shape = (m: IndicatorMeta) => JSON.stringify([m.overlay, m.precision, m.plots, m.bands, m.columns, m.palette]);
  return shape(a) === shape(b);
}

/** A color with its opacity scaled by `alpha` (hex or rgb[a]; anything else is kept). */
export function fade(color: string, alpha: number): string {
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

/** Where a script table sits in its pane (Pine's `position.top_right` and friends), as CSS. */
export function tablePlacement(position: string): Record<string, string | number> {
  const [v = 'top', h = 'right'] = position.split('_');
  const style: Record<string, string | number> = {};
  const shift: string[] = [];
  if (v === 'bottom') style.bottom = 6;
  else if (v === 'middle') {
    style.top = '50%';
    shift.push('translateY(-50%)');
  } else style.top = 6;
  if (h === 'left') style.left = 6;
  else if (h === 'center') {
    style.left = '50%';
    shift.push('translateX(-50%)');
  } else style.right = 6;
  if (shift.length) style.transform = shift.join(' ');
  return style;
}

/** A table's cells laid out for HTML, merged cells included: one entry per drawn cell, by row. */
export function tableGrid<T extends { row: number; col: number; colspan: number; rowspan: number }>(
  rows: number,
  columns: number,
  cells: readonly T[],
): Array<Array<{ col: number; cell: T | null }>> {
  const at = new Map(cells.map((c) => [`${c.row}:${c.col}`, c]));
  const covered = new Set<string>();
  const out: Array<Array<{ col: number; cell: T | null }>> = [];
  const maxRow = Math.max(rows, ...cells.map((c) => c.row + 1));
  const maxCol = Math.max(columns, ...cells.map((c) => c.col + 1));
  for (let r = 0; r < maxRow; r += 1) {
    const line: Array<{ col: number; cell: T | null }> = [];
    for (let c = 0; c < maxCol; c += 1) {
      if (covered.has(`${r}:${c}`)) continue;
      const cell = at.get(`${r}:${c}`) ?? null;
      if (cell) {
        for (let dr = 0; dr < Math.max(1, cell.rowspan); dr += 1) {
          for (let dc = 0; dc < Math.max(1, cell.colspan); dc += 1) {
            if (dr || dc) covered.add(`${r + dr}:${c + dc}`);
          }
        }
      }
      line.push({ col: c, cell });
    }
    out.push(line);
  }
  return out;
}
