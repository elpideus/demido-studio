// An indicator's look on this chart (its settings' Style and Visibility tabs): applied over what
// TradingView describes, so changing it redraws the indicator without computing it again. Pure,
// so it is unit-tested.

import type {
  BandLook,
  IndicatorBand,
  IndicatorLook,
  IndicatorMeta,
  IndicatorPlot,
  IndicatorPlotKind,
  PlotLook,
} from '@/lib/types';

/** The plot types the Style tab offers (shapes keep theirs). */
export const PLOT_KINDS: ReadonlyArray<{ value: Exclude<IndicatorPlotKind, 'shapes'>; label: string }> = [
  { value: 'line', label: 'Line' },
  { value: 'step', label: 'Step line' },
  { value: 'histogram', label: 'Histogram' },
  { value: 'columns', label: 'Columns' },
  { value: 'area', label: 'Area' },
  { value: 'circles', label: 'Circles' },
  { value: 'cross', label: 'Cross' },
];

const KINDS = new Set<string>(PLOT_KINDS.map((k) => k.value));

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v);
const color = (v: unknown) => (typeof v === 'string' && v.trim() ? v.trim() : undefined);
const width = (v: unknown) =>
  typeof v === 'number' && Number.isFinite(v) ? Math.min(4, Math.max(1, Math.round(v))) : undefined;
const dash = (v: unknown) => (v === 0 || v === 1 || v === 2 ? v : undefined);
const bool = (v: unknown) => (typeof v === 'boolean' ? v : undefined);

function defined<T extends object>(o: T): T {
  return Object.fromEntries(Object.entries(o).filter(([, v]) => v !== undefined)) as T;
}

function plotLookOf(v: unknown): PlotLook | null {
  if (!isObj(v)) return null;
  const colors = isObj(v.colors)
    ? Object.fromEntries(Object.entries(v.colors).filter((e): e is [string, string] => !!color(e[1])))
    : undefined;
  const out = defined<PlotLook>({
    color: color(v.color),
    colors: colors && Object.keys(colors).length ? colors : undefined,
    width: width(v.width),
    dash: dash(v.dash),
    kind: typeof v.kind === 'string' && KINDS.has(v.kind) ? (v.kind as IndicatorPlotKind) : undefined,
    hidden: bool(v.hidden),
  });
  return Object.keys(out).length ? out : null;
}

function bandLookOf(v: unknown): BandLook | null {
  if (!isObj(v)) return null;
  const out = defined<BandLook>({
    color: color(v.color),
    width: width(v.width),
    dash: dash(v.dash),
    value: typeof v.value === 'number' && Number.isFinite(v.value) ? v.value : undefined,
    hidden: bool(v.hidden),
  });
  return Object.keys(out).length ? out : null;
}

function records<T>(v: unknown, of: (x: unknown) => T | null): Record<string, T> | undefined {
  if (!isObj(v)) return undefined;
  const out: Record<string, T> = {};
  for (const [k, x] of Object.entries(v)) {
    const r = of(x);
    if (r) out[k] = r;
  }
  return Object.keys(out).length ? out : undefined;
}

/** A look kept in a window's props, checked; undefined when there is none worth keeping. */
export function lookOf(value: unknown): IndicatorLook | undefined {
  if (!isObj(value)) return undefined;
  const precision =
    typeof value.precision === 'number' &&
    Number.isInteger(value.precision) &&
    value.precision >= 0 &&
    value.precision <= 10
      ? value.precision
      : undefined;
  const out = defined<IndicatorLook>({
    plots: records(value.plots, plotLookOf),
    bands: records(value.bands, bandLookOf),
    precision,
    timeframes: Array.isArray(value.timeframes)
      ? value.timeframes.filter((t): t is string => typeof t === 'string')
      : undefined,
    legendInputs: bool(value.legendInputs),
    legendValues: bool(value.legendValues),
    scaleLabels: bool(value.scaleLabels),
  });
  return Object.keys(out).length ? out : undefined;
}

/** What the chart draws: the description with the look over it. */
export function applyLook(meta: IndicatorMeta, look: IndicatorLook | undefined): IndicatorMeta {
  if (!look) return meta;
  const plots = meta.plots.map((p): IndicatorPlot => {
    const l = look.plots?.[p.id];
    const out: IndicatorPlot = { ...p };
    if (l) {
      if (l.color) out.color = l.color;
      if (l.colors && p.colors) out.colors = { ...p.colors, ...l.colors };
      if (l.width !== undefined) out.width = l.width;
      if (l.dash !== undefined) out.dash = l.dash;
      if (l.kind && p.kind !== 'shapes' && l.kind !== 'shapes') out.kind = l.kind;
      if (l.hidden === true) out.hidden = true;
      else if (l.hidden === false) delete out.hidden;
    }
    if (look.scaleLabels === false) out.axisLabel = false;
    return out;
  });
  const bands = meta.bands.map((b): IndicatorBand => {
    const l = look.bands?.[b.id];
    if (!l) return b;
    const out: IndicatorBand = { ...b };
    if (l.color) out.color = l.color;
    if (l.width !== undefined) out.width = l.width;
    if (l.dash !== undefined) out.dash = l.dash;
    if (l.value !== undefined) out.value = l.value;
    if (l.hidden === true) out.hidden = true;
    else if (l.hidden === false) delete out.hidden;
    return out;
  });
  return { ...meta, plots, bands, precision: look.precision ?? meta.precision };
}

/** A color as the Style tab edits it: `#rrggbb` and an opacity (0–1); null for what it cannot read. */
export function splitColor(color: string): { hex: string; alpha: number } | null {
  const c = color.trim();
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i.exec(c);
  if (hex) {
    let h = hex[1]!.toLowerCase();
    if (h.length === 3) h = [...h].map((x) => x + x).join('');
    return { hex: `#${h.slice(0, 6)}`, alpha: h.length === 8 ? +(parseInt(h.slice(6), 16) / 255).toFixed(2) : 1 };
  }
  const rgb = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(c);
  if (!rgb) return null;
  const part = (s: string) => Math.min(255, Number(s)).toString(16).padStart(2, '0');
  const alpha = rgb[4] === undefined ? 1 : Math.min(1, Math.max(0, Number(rgb[4])));
  return { hex: `#${part(rgb[1]!)}${part(rgb[2]!)}${part(rgb[3]!)}`, alpha };
}

/** The color of a hex and an opacity: the hex itself when opaque. */
export function joinColor(hex: string, alpha: number): string {
  if (alpha >= 1) return hex;
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  return `rgba(${r},${g},${b},${+Math.max(0, alpha).toFixed(2)})`;
}

/** Whether an indicator shows on a timeframe (its Visibility tab). */
export function shownOn(look: IndicatorLook | undefined, timeframe: string): boolean {
  return !look?.timeframes || look.timeframes.includes(timeframe);
}

/**
 * The look worth keeping: only what differs from the indicator's own description, so a later
 * change of the script's defaults still shows through. Undefined when nothing differs.
 */
export function tidyLook(
  meta: IndicatorMeta,
  look: IndicatorLook,
  allTimeframes: readonly string[],
): IndicatorLook | undefined {
  const plots: Record<string, PlotLook> = {};
  for (const p of meta.plots) {
    const l = look.plots?.[p.id];
    if (!l) continue;
    const colors =
      l.colors && p.colors ? Object.fromEntries(Object.entries(l.colors).filter(([k, c]) => p.colors![k] !== c)) : {};
    const kept = defined<PlotLook>({
      color: l.color && l.color !== p.color ? l.color : undefined,
      colors: Object.keys(colors).length ? colors : undefined,
      width: l.width !== undefined && l.width !== p.width ? l.width : undefined,
      dash: l.dash !== undefined && l.dash !== p.dash ? l.dash : undefined,
      kind: l.kind && l.kind !== p.kind ? l.kind : undefined,
      hidden: l.hidden !== undefined && l.hidden !== !!p.hidden ? l.hidden : undefined,
    });
    if (Object.keys(kept).length) plots[p.id] = kept;
  }
  const bands: Record<string, BandLook> = {};
  for (const b of meta.bands) {
    const l = look.bands?.[b.id];
    if (!l) continue;
    const kept = defined<BandLook>({
      color: l.color && l.color !== b.color ? l.color : undefined,
      width: l.width !== undefined && l.width !== b.width ? l.width : undefined,
      dash: l.dash !== undefined && l.dash !== b.dash ? l.dash : undefined,
      value: l.value !== undefined && l.value !== b.value ? l.value : undefined,
      hidden: l.hidden !== undefined && l.hidden !== !!b.hidden ? l.hidden : undefined,
    });
    if (Object.keys(kept).length) bands[b.id] = kept;
  }
  const every = !look.timeframes || allTimeframes.every((t) => look.timeframes!.includes(t));
  const out = defined<IndicatorLook>({
    plots: Object.keys(plots).length ? plots : undefined,
    bands: Object.keys(bands).length ? bands : undefined,
    precision: look.precision !== undefined && look.precision !== meta.precision ? look.precision : undefined,
    timeframes: every ? undefined : allTimeframes.filter((t) => look.timeframes!.includes(t)),
    legendInputs: look.legendInputs === false ? false : undefined,
    legendValues: look.legendValues === false ? false : undefined,
    scaleLabels: look.scaleLabels === false ? false : undefined,
  });
  return Object.keys(out).length ? out : undefined;
}
