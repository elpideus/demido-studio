import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';
import {
  AreaSeries,
  CandlestickSeries,
  ColorType,
  CrosshairMode,
  HistogramSeries,
  LineSeries,
  LineType,
  createChart,
  createSeriesMarkers,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type ISeriesMarkersPluginApi,
  type LineWidth,
  type SeriesMarker,
  type SeriesType,
  type Time,
  type UTCTimestamp,
} from 'lightweight-charts';
import { Eye, EyeOff, RotateCcw, Settings2, TriangleAlert, X } from 'lucide-react';
import { Spinner, cx } from '@demido/ui';

import { formatPrice } from '@/lib/format';
import type {
  Bar,
  IndicatorGraphics,
  IndicatorMeta,
  IndicatorPlot,
  IndicatorRow,
  IndicatorTable,
  MarketSource,
  MarketSpan,
} from '@/lib/types';
import { mergeSpans, sourceAt, sourcesOf } from './chartData';
import { Drawings } from './drawings';
import { SOURCE_NAMES } from './DownloadProgress';
import { localRows } from './indicators';
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
  pointAt,
  precisionOf,
  sameShape,
  shapeMarkers,
  type PlotPoint,
} from './indicatorView';
import { ScriptTable } from './ScriptTable';
import styles from './MarketWindow.module.css';

export interface PriceChartHandle {
  /** Replaces all data and fits it, prices included. `spans` say which source each bar came from.
   *  TradingView indicators lose their values (they belong to the previous chart). */
  setBars: (bars: Bar[], spans: MarketSpan[]) => void;
  /** Adds older bars on the left without moving the view. Returns how many were older than the
   *  chart's oldest bar and so were actually added. */
  prependBars: (older: Bar[], spans: MarketSpan[]) => number;
  /** Applies a live update of the last (or a new) bar; it belongs to the newest bar's source. */
  updateBar: (bar: Bar) => void;
  /** Whether the view shows (or nearly shows) the oldest loaded bar. */
  nearOldest: () => boolean;
  /** Draws an indicator, or redraws it for a new description. `local` ones are computed from the
   *  chart's bars (by `compute` when given, else as the classics of market/indicators.ts); the
   *  others get their values from `indicatorData`. */
  showIndicator: (key: string, meta: IndicatorMeta, local: boolean, compute?: Compute) => void;
  /** Draws an indicator in a new look (its settings' Style tab), keeping the values it has. */
  restyleIndicator: (key: string, meta: IndicatorMeta) => void;
  /** TradingView rows for an indicator: `full` replaces them, otherwise they merge by time. */
  indicatorData: (key: string, rows: IndicatorRow[], full: boolean) => void;
  indicatorGraphics: (key: string, graphics: IndicatorGraphics | null) => void;
  /** Takes an indicator off the chart (its legend row stays while it is listed). */
  clearIndicator: (key: string) => void;
}

/** An indicator as the legend lists it, drawn or not. */
export interface IndicatorView {
  key: string;
  /** Shown until its description is known. */
  name: string;
  status: 'loading' | 'ready' | 'error' | 'signin';
  message?: string;
  /** Its eye is closed. */
  hidden: boolean;
  /** Not shown on the chart's timeframe (its Visibility settings). */
  offTimeframe?: boolean;
  /** Computed on this computer rather than by TradingView. */
  local: boolean;
  /** The assistant's drawings: no settings. */
  drawing?: boolean;
  /** Its inputs after its name, and its values (default: both). */
  legendInputs?: boolean;
  legendValues?: boolean;
}

export type IndicatorAction = 'toggle' | 'settings' | 'remove' | 'retry';

/** Values and drawings computed here from the chart's bars (oldest first). */
export type Compute = (
  meta: IndicatorMeta,
  bars: readonly Bar[],
) => { rows: IndicatorRow[]; graphics: IndicatorGraphics | null };

/** The classics, computed again on every update of the last bar. */
const classic: Compute = (meta, bars) => ({ rows: localRows(meta, bars), graphics: null });

const concealed = (view: IndicatorView | undefined) => !!view && (view.hidden || !!view.offTimeframe);

/** How close (in bars) the view's left edge gets to the oldest bar before more are asked for. */
const NEAR_OLDEST = 15;

interface Props {
  pricescale: number;
  /** Called when the view nears the oldest bar, to load more history. */
  onNeedMore: () => void;
  /** The oldest loaded bar's screen position, recomputed on every pan/zoom (null when there is
   *  no data or it is off-screen) — lets an overlay track that edge as the chart moves. */
  onEdgeAnchor?: (pos: { x: number; y: number } | null) => void;
  indicators?: IndicatorView[];
  onIndicator?: (key: string, action: IndicatorAction) => void;
}

const UP = '#4fcf8a';
const DOWN = '#ef6b6b';

const timeFormat = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: 'short',
  day: 'numeric',
  hour: '2-digit',
  minute: '2-digit',
});

function toCandle(b: Bar) {
  return { time: b.t as UTCTimestamp, open: b.o, high: b.h, low: b.l, close: b.c };
}

// Volume units differ between sources (Dukascopy counts differently from TradingView), so only bars
// from the newest bar's source get a column; the rest are left blank rather than drawn misleadingly.
function toVolume(b: Bar, shown: boolean) {
  if (!shown) return { time: b.t as UTCTimestamp };
  return {
    time: b.t as UTCTimestamp,
    value: b.v,
    color: b.c >= b.o ? 'rgba(79, 207, 138, 0.28)' : 'rgba(239, 107, 107, 0.28)',
  };
}

function volumes(bars: Bar[], spans: MarketSpan[]) {
  const sources = sourcesOf(bars, spans);
  const newest = sources[sources.length - 1] ?? null;
  return bars.map((b, i) => toVolume(b, sources[i] === newest));
}

function precisionOfScale(pricescale: number): number {
  return Math.min(8, Math.max(0, Math.round(Math.log10(pricescale || 100))));
}

const lineWidth = (w: number) => Math.min(4, Math.max(1, Math.round(w))) as LineWidth;

/** One indicator on the chart. */
interface Shown {
  meta: IndicatorMeta;
  local: boolean;
  /** What computes a local indicator's values. */
  compute: Compute | null;
  hidden: boolean;
  columns: Map<string, number>;
  rows: IndicatorRow[];
  byTime: Map<number, IndicatorRow>;
  plotted: Array<{ plot: IndicatorPlot; api: ISeriesApi<SeriesType> }>;
  /** What bands, markers and drawings hang on: the candles, or an invisible series in its pane. */
  anchor: ISeriesApi<SeriesType>;
  ownAnchor: boolean;
  markers: ISeriesMarkersPluginApi<Time> | null;
  bands: IPriceLine[];
  drawings: Drawings;
  graphics: IndicatorGraphics | null;
}

/** A point in the shape its series takes. */
function seriesPoint(plot: IndicatorPlot, p: PlotPoint) {
  const time = p.time as UTCTimestamp;
  if (p.value === undefined) return { time };
  if (plot.kind === 'area') {
    return p.color
      ? { time, value: p.value, lineColor: p.color, topColor: fade(p.color, 0.28), bottomColor: fade(p.color, 0.02) }
      : { time, value: p.value };
  }
  return p.color ? { time, value: p.value, color: p.color } : { time, value: p.value };
}

/** Candlesticks and volume, in local time, drawn with TradingView's lightweight-charts; indicators
 *  over them or in panes below, each pane with its legend. */
export const PriceChart = forwardRef<PriceChartHandle, Props>(function PriceChart(
  { pricescale, onNeedMore, onEdgeAnchor, indicators = [], onIndicator },
  ref,
) {
  const container = useRef<HTMLDivElement>(null);
  const chart = useRef<IChartApi | null>(null);
  const candles = useRef<ISeriesApi<'Candlestick'> | null>(null);
  const volume = useRef<ISeriesApi<'Histogram'> | null>(null);
  const data = useRef<Bar[]>([]);
  const times = useRef<number[]>([]);
  const timeSet = useRef<Set<number>>(new Set());
  const spans = useRef<MarketSpan[]>([]);
  const shown = useRef(new Map<string, Shown>());
  const precision = useRef(precisionOfScale(pricescale));
  const needMore = useRef(onNeedMore);
  needMore.current = onNeedMore;
  const edgeAnchor = useRef(onEdgeAnchor);
  edgeAnchor.current = onEdgeAnchor;
  const [legend, setLegend] = useState<Bar | null>(null);
  // Bumped when indicator values change, so the legend shows them.
  const [, setTick] = useState(0);
  const tickPending = useRef(false);
  const [panes, setPanes] = useState<Array<{ top: number; height: number }>>([]);
  const [axisWidth, setAxisWidth] = useState(0);
  const [tables, setTables] = useState<Record<string, IndicatorTable[]>>({});

  const indicatorsRef = useRef(indicators);
  indicatorsRef.current = indicators;

  const refreshLegend = useCallback(() => {
    if (tickPending.current) return;
    tickPending.current = true;
    requestAnimationFrame(() => {
      tickPending.current = false;
      setTick((n) => n + 1);
    });
  }, []);

  const measurePanes = useCallback(() => {
    const c = chart.current;
    const box = container.current?.getBoundingClientRect();
    if (!c || !box) return;
    const next = c.panes().map((p) => {
      const r = p.getHTMLElement()?.getBoundingClientRect();
      return r ? { top: r.top - box.top, height: r.height } : { top: 0, height: 0 };
    });
    setPanes((prev) => (JSON.stringify(prev) === JSON.stringify(next) ? prev : next));
    setAxisWidth(c.priceScale('right').width());
  }, []);

  // Panes come and go with indicators, and the chart lays them out on its next frame: their
  // legends and tables follow once it has.
  const measureSoon = useCallback(() => {
    requestAnimationFrame(() => requestAnimationFrame(measurePanes));
  }, [measurePanes]);

  const barAt = useCallback((t: number): Bar | undefined => {
    const bars = data.current;
    let lo = 0;
    let hi = bars.length - 1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const bt = bars[mid]!.t;
      if (bt === t) return bars[mid];
      if (bt < t) lo = mid + 1;
      else hi = mid - 1;
    }
    return undefined;
  }, []);

  useEffect(() => {
    if (!container.current) return undefined;
    const c = createChart(container.current, {
      autoSize: true,
      layout: {
        background: { type: ColorType.Solid, color: '#1c1c1c' },
        textColor: 'rgba(255,255,255,0.55)',
        fontFamily: "'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif",
        fontSize: 11,
        attributionLogo: true,
        panes: { separatorColor: 'rgba(255,255,255,0.08)', separatorHoverColor: 'rgba(255,255,255,0.14)' },
      },
      grid: {
        vertLines: { color: 'rgba(255,255,255,0.04)' },
        horzLines: { color: 'rgba(255,255,255,0.04)' },
      },
      crosshair: {
        mode: CrosshairMode.Normal,
        vertLine: { color: 'rgba(255,255,255,0.25)', labelBackgroundColor: '#333' },
        horzLine: { color: 'rgba(255,255,255,0.25)', labelBackgroundColor: '#333' },
      },
      rightPriceScale: { borderColor: 'rgba(255,255,255,0.08)' },
      timeScale: {
        borderColor: 'rgba(255,255,255,0.08)',
        timeVisible: true,
        secondsVisible: false,
        rightOffset: 6,
      },
      localization: {
        timeFormatter: (t: Time) => timeFormat.format(new Date((t as number) * 1000)),
      },
    });
    const cs = c.addSeries(CandlestickSeries, {
      upColor: UP,
      downColor: DOWN,
      borderVisible: false,
      wickUpColor: UP,
      wickDownColor: DOWN,
    });
    const vs = c.addSeries(HistogramSeries, { priceFormat: { type: 'volume' }, priceScaleId: '' });
    vs.priceScale().applyOptions({ scaleMargins: { top: 0.82, bottom: 0 } });
    c.timeScale().subscribeVisibleLogicalRangeChange((range) => {
      if (range && range.from < NEAR_OLDEST && data.current.length > 0) needMore.current();
      const oldest = data.current[0];
      const x = oldest ? c.timeScale().timeToCoordinate(oldest.t as UTCTimestamp) : null;
      const y = oldest ? (candles.current?.priceToCoordinate(oldest.c) ?? null) : null;
      edgeAnchor.current?.(x !== null && y !== null ? { x, y } : null);
    });
    c.subscribeCrosshairMove((param) => {
      if (!param.time) {
        setLegend(null);
        return;
      }
      setLegend(barAt(param.time as number) ?? null);
    });
    chart.current = c;
    candles.current = cs;
    volume.current = vs;
    // Pane heights change with the window and when a pane separator is dragged.
    const observer = new ResizeObserver(() => measurePanes());
    observer.observe(container.current);
    const panesEl = container.current;
    panesEl.addEventListener('pointerup', measurePanes);
    return () => {
      observer.disconnect();
      panesEl.removeEventListener('pointerup', measurePanes);
      shown.current.clear();
      c.remove();
      chart.current = null;
    };
  }, [barAt, measurePanes]);

  useEffect(() => {
    precision.current = precisionOfScale(pricescale);
    const p = precision.current;
    candles.current?.applyOptions({
      priceFormat: { type: 'price', precision: p, minMove: 1 / 10 ** p },
    });
    // Indicators without decimals of their own follow the chart's.
    for (const s of shown.current.values()) {
      if (s.meta.precision !== null) continue;
      const format = { type: 'price' as const, precision: p, minMove: 1 / 10 ** p };
      for (const { api } of s.plotted) api.applyOptions({ priceFormat: format });
      if (s.ownAnchor) s.anchor.applyOptions({ priceFormat: format });
    }
  }, [pricescale]);

  // -------------------------------------------------------------------------------------------
  // Indicators

  const rowAtOf = (s: Shown) => (t: number) => s.byTime.get(t);

  const markersOf = (s: Shown) =>
    shapeMarkers(s.meta, s.rows, s.columns, times.current[0])
      .filter((m) => timeSet.current.has(m.time))
      .map((m) => ({ ...m, time: m.time as UTCTimestamp })) as unknown as SeriesMarker<Time>[];

  /** Draws every value of an indicator again (after new bars or a full replacement). */
  const render = (s: Shown) => {
    if (s.compute) {
      const out = s.compute(s.meta, data.current);
      s.rows = out.rows;
      s.byTime = new Map(s.rows.map((r) => [r[0], r]));
      if (s.compute !== classic) {
        s.graphics = out.graphics;
        s.drawings.set(out.graphics);
      }
    }
    const rowAt = rowAtOf(s);
    for (const { plot, api } of s.plotted) {
      api.setData(plotPoints(plot, rowAt, times.current, s.columns).map((p) => seriesPoint(plot, p)) as never);
    }
    if (s.ownAnchor) {
      s.anchor.setData(
        anchorPoints(s.meta, rowAt, times.current, s.columns).map((p) => ({ ...p, time: p.time as UTCTimestamp })) as never,
      );
    }
    s.markers?.setMarkers(s.hidden ? [] : markersOf(s));
  };

  /** Redraws the bars at `at` (chart times, oldest first) from the indicator's rows. */
  const apply = (s: Shown, at: number[]) => {
    const last = times.current[times.current.length - 1];
    if (last === undefined) return;
    const rowAt = rowAtOf(s);
    for (const t of at) {
      if (!timeSet.current.has(t)) continue;
      const historical = t < last;
      const row = rowAt(t);
      for (const { plot, api } of s.plotted) {
        api.update(seriesPoint(plot, pointAt(plot, row, t, s.columns.get(plot.id) ?? -1, s.columns)) as never, historical);
      }
      if (s.ownAnchor) {
        const [p] = anchorPoints(s.meta, rowAt, [t], s.columns);
        if (p) s.anchor.update({ time: t as UTCTimestamp, value: p.value } as never, historical);
      }
    }
    if (s.markers) s.markers.setMarkers(s.hidden ? [] : markersOf(s));
  };

  const setVisible = (s: Shown, visible: boolean) => {
    s.hidden = !visible;
    for (const { api } of s.plotted) api.applyOptions({ visible });
    if (s.ownAnchor) s.anchor.applyOptions({ visible });
    for (const band of s.bands) band.applyOptions({ lineVisible: visible });
    s.markers?.setMarkers(visible ? markersOf(s) : []);
    s.drawings.setVisible(visible);
  };

  const destroy = (s: Shown) => {
    const c = chart.current;
    if (!c) return;
    s.markers?.detach();
    s.anchor.detachPrimitive(s.drawings);
    for (const band of s.bands) s.anchor.removePriceLine(band);
    for (const { api } of s.plotted) c.removeSeries(api);
    if (s.ownAnchor) c.removeSeries(s.anchor);
  };

  const build = (key: string, meta: IndicatorMeta, compute: Compute | null, hidden: boolean): Shown | null => {
    const c = chart.current;
    const cs = candles.current;
    if (!c || !cs) return null;
    const old = shown.current.get(key);
    // A redrawn indicator keeps its pane: the new series join it before the old ones leave.
    const paneIndex = meta.overlay
      ? 0
      : old && old.ownAnchor
        ? old.anchor.getPane().paneIndex()
        : c.panes().length;
    const newPane = paneIndex >= c.panes().length;
    const p = precisionOf(meta, precision.current);
    const priceFormat = { type: 'price' as const, precision: p, minMove: 1 / 10 ** p };
    let anchor: ISeriesApi<SeriesType> = cs;
    if (!meta.overlay) {
      anchor = c.addSeries(
        LineSeries,
        {
          lineVisible: false,
          pointMarkersVisible: false,
          crosshairMarkerVisible: false,
          lastValueVisible: false,
          priceLineVisible: false,
          autoscaleInfoProvider: () => null,
          priceFormat,
        },
        paneIndex,
      );
    }
    const pane = meta.overlay ? 0 : anchor.getPane().paneIndex();
    const plotted = meta.plots
      .filter((plot) => plot.kind !== 'shapes' && !plot.hidden)
      .map((plot) => {
        const common = {
          priceFormat,
          lastValueVisible: plot.axisLabel !== false,
          priceLineVisible: false,
          visible: !hidden,
          title: '',
        };
        let api: ISeriesApi<SeriesType>;
        if (plot.kind === 'histogram' || plot.kind === 'columns') {
          api = c.addSeries(HistogramSeries, { ...common, color: plot.color, base: plot.base ?? 0 }, pane);
        } else if (plot.kind === 'area') {
          api = c.addSeries(
            AreaSeries,
            {
              ...common,
              lineColor: plot.color,
              topColor: fade(plot.color, 0.28),
              bottomColor: fade(plot.color, 0.02),
              lineWidth: lineWidth(plot.width),
              lineStyle: plot.dash,
            },
            pane,
          );
        } else {
          const points = plot.kind === 'circles' || plot.kind === 'cross';
          api = c.addSeries(
            LineSeries,
            {
              ...common,
              color: plot.color,
              lineWidth: lineWidth(plot.width),
              lineStyle: plot.dash,
              lineType: plot.kind === 'step' ? LineType.WithSteps : LineType.Simple,
              lineVisible: !points,
              pointMarkersVisible: points,
              pointMarkersRadius: points ? Math.max(1.5, plot.width + 0.5) : undefined,
              crosshairMarkerVisible: !points,
            },
            pane,
          );
        }
        return { plot, api };
      });
    const bands = meta.bands
      .filter((b) => !b.hidden)
      .map((b) =>
        anchor.createPriceLine({
          price: b.value,
          color: b.color,
          lineWidth: lineWidth(b.width),
          lineStyle: b.dash,
          lineVisible: !hidden,
          axisLabelVisible: false,
          title: '',
        }),
      );
    const markers = meta.plots.some((plot) => plot.kind === 'shapes' && !plot.hidden)
      ? createSeriesMarkers(anchor, [])
      : null;
    const drawings = new Drawings(barAt);
    anchor.attachPrimitive(drawings);
    if (old) destroy(old);
    if (newPane && c.panes().length > 1) {
      // The price pane keeps most of the height, as on TradingView.
      const main = c.panes()[0];
      if (main && main.getStretchFactor() === 1) main.setStretchFactor(3);
    }
    const s: Shown = {
      meta,
      local: compute !== null,
      compute,
      hidden,
      columns: columnIndex(meta),
      rows: [],
      byTime: new Map(),
      plotted,
      anchor,
      ownAnchor: anchor !== cs,
      markers,
      bands,
      drawings,
      graphics: null,
    };
    drawings.setVisible(!hidden);
    shown.current.set(key, s);
    measureSoon();
    return s;
  };

  const removeShown = (key: string) => {
    const s = shown.current.get(key);
    if (!s) return;
    destroy(s);
    shown.current.delete(key);
    measureSoon();
    setTables((prev) => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  };

  // Indicators no longer listed leave the chart; the listed ones follow their eye.
  const listed = indicators.map((i) => `${i.key}:${concealed(i) ? 1 : 0}`).join(',');
  useEffect(() => {
    const keep = new Map(indicators.map((i) => [i.key, i]));
    for (const key of [...shown.current.keys()]) {
      const view = keep.get(key);
      if (!view) removeShown(key);
      else {
        const s = shown.current.get(key)!;
        if (s.hidden !== concealed(view)) setVisible(s, !concealed(view));
      }
    }
    // `listed` changes exactly when the keys or eyes do.
  }, [listed]);

  const setTimes = () => {
    times.current = data.current.map((b) => b.t);
    timeSet.current = new Set(times.current);
  };

  useImperativeHandle(ref, () => ({
    setBars: (bars, sourceSpans) => {
      data.current = [...bars].sort((a, b) => a.t - b.t);
      setTimes();
      spans.current = mergeSpans([], sourceSpans);
      // Dragging the price axis or the chart turns auto-scaling off, which would keep the old
      // symbol's price range on screen: another symbol can sit thousands of times higher.
      candles.current?.priceScale().applyOptions({ autoScale: true });
      volume.current?.priceScale().applyOptions({ autoScale: true });
      candles.current?.setData(data.current.map(toCandle));
      volume.current?.setData(volumes(data.current, spans.current));
      for (const s of shown.current.values()) {
        if (!s.compute) {
          s.rows = [];
          s.byTime = new Map();
          s.graphics = null;
          s.drawings.set(null);
        }
        if (s.ownAnchor) s.anchor.priceScale().applyOptions({ autoScale: true });
        render(s);
      }
      setTables({});
      chart.current?.timeScale().fitContent();
      if (data.current.length > 150) {
        chart.current
          ?.timeScale()
          .setVisibleLogicalRange({ from: data.current.length - 150, to: data.current.length + 5 });
      }
      setLegend(null);
    },
    prependBars: (older, sourceSpans) => {
      const first = data.current[0]?.t ?? Number.MAX_SAFE_INTEGER;
      const fresh = older.filter((b) => b.t < first).sort((a, b) => a.t - b.t);
      if (!fresh.length || !chart.current) return 0;
      const range = chart.current.timeScale().getVisibleLogicalRange();
      data.current = [...fresh, ...data.current];
      setTimes();
      // Spans reaching into bars the chart already had describe those bars too; they agree.
      spans.current = mergeSpans(spans.current, sourceSpans);
      candles.current?.setData(data.current.map(toCandle));
      volume.current?.setData(volumes(data.current, spans.current));
      for (const s of shown.current.values()) render(s);
      if (range) {
        chart.current
          .timeScale()
          .setVisibleLogicalRange({ from: range.from + fresh.length, to: range.to + fresh.length });
      }
      return fresh.length;
    },
    updateBar: (bar) => {
      const last = data.current[data.current.length - 1];
      if (last && bar.t < last.t) return;
      const added = !last || bar.t !== last.t;
      if (added) {
        data.current.push(bar);
        times.current.push(bar.t);
        timeSet.current.add(bar.t);
      } else data.current[data.current.length - 1] = bar;
      candles.current?.update(toCandle(bar));
      volume.current?.update(toVolume(bar, true));
      for (const s of shown.current.values()) {
        // Drawings only move on with a new bar.
        if (s.compute && s.compute !== classic) {
          if (added) render(s);
          continue;
        }
        if (s.compute) {
          s.rows = s.compute(s.meta, data.current).rows;
          const row = s.rows[s.rows.length - 1];
          if (row) s.byTime.set(row[0], row);
        }
        // A new bar gets a point in every indicator, with the value TradingView may already have sent.
        if (s.compute || added) apply(s, [bar.t]);
      }
      if (shown.current.size) refreshLegend();
    },
    nearOldest: () => {
      const range = chart.current?.timeScale().getVisibleLogicalRange();
      return !!range && data.current.length > 0 && range.from < NEAR_OLDEST;
    },
    showIndicator: (key, meta, local, compute) => {
      const old = shown.current.get(key);
      const hidden = concealed(indicatorsRef.current.find((i) => i.key === key));
      const how = local ? (compute ?? classic) : null;
      if (old && old.local === local && sameShape(old.meta, meta)) {
        // Same series: only the settings (and so the values) differ. TradingView sends the new
        // values; the old ones must not stay up meanwhile.
        old.meta = meta;
        old.compute = how;
        if (!local) {
          old.rows = [];
          old.byTime = new Map();
          old.graphics = null;
          old.drawings.set(null);
        }
        render(old);
        refreshLegend();
        return;
      }
      const s = build(key, meta, how, hidden);
      if (!s) return;
      render(s);
      setTables((prev) => {
        if (!(key in prev)) return prev;
        const next = { ...prev };
        delete next[key];
        return next;
      });
      refreshLegend();
    },
    restyleIndicator: (key, meta) => {
      const old = shown.current.get(key);
      if (!old) return;
      if (sameShape(old.meta, meta)) {
        old.meta = meta;
        refreshLegend();
        return;
      }
      const { rows, byTime, graphics, compute, hidden } = old;
      const s = build(key, meta, compute, hidden);
      if (!s) return;
      s.rows = rows;
      s.byTime = byTime;
      s.graphics = graphics;
      s.drawings.set(graphics);
      render(s);
      refreshLegend();
    },
    indicatorData: (key, rows, full) => {
      const s = shown.current.get(key);
      if (!s || s.local) return;
      s.rows = mergeRows(s.rows, rows, full);
      if (full) {
        s.byTime = new Map(s.rows.map((r) => [r[0], r]));
        render(s);
      } else {
        for (const r of rows) s.byTime.set(r[0], r);
        apply(
          s,
          rows.map((r) => r[0]).sort((a, b) => a - b),
        );
      }
      refreshLegend();
    },
    indicatorGraphics: (key, graphics) => {
      const s = shown.current.get(key);
      if (!s) return;
      s.graphics = graphics;
      s.drawings.set(graphics);
      const list = graphics?.tables ?? [];
      setTables((prev) => {
        if (!list.length && !(key in prev)) return prev;
        return { ...prev, [key]: list };
      });
    },
    clearIndicator: (key) => {
      removeShown(key);
      refreshLegend();
    },
  }));

  const current = legend ?? data.current[data.current.length - 1] ?? null;
  const source: MarketSource | null = current ? sourceAt(spans.current, current.t) : null;

  // Legend rows by pane: the price pane lists overlays and whatever is not drawn (yet).
  const byPane = new Map<number, IndicatorView[]>();
  for (const view of indicators) {
    const s = shown.current.get(view.key);
    const pane = s?.ownAnchor ? s.anchor.getPane().paneIndex() : 0;
    byPane.set(pane, [...(byPane.get(pane) ?? []), view]);
  }

  const legendRow = (view: IndicatorView) => {
    const s = shown.current.get(view.key);
    return (
      <IndicatorLegend
        key={view.key}
        view={view}
        shown={s}
        time={current?.t ?? null}
        chartPrecision={precision.current}
        onAction={(action) => onIndicator?.(view.key, action)}
      />
    );
  };

  const paneWidth = (container.current?.clientWidth ?? 0) - axisWidth;
  return (
    <div className={styles.chartWrap}>
      <div ref={container} className={styles.chart} />
      <div className={styles.legends} style={{ top: (panes[0]?.top ?? 0) + 8 }}>
        {current && (
          <div className={styles.legend}>
            <span>
              O <b>{formatPrice(current.o, pricescale)}</b>
            </span>
            <span>
              H <b>{formatPrice(current.h, pricescale)}</b>
            </span>
            <span>
              L <b>{formatPrice(current.l, pricescale)}</b>
            </span>
            <span>
              C <b className={current.c >= current.o ? styles.up : styles.down}>{formatPrice(current.c, pricescale)}</b>
            </span>
            {current.v > 0 && (
              <span>
                V <b>{current.v.toLocaleString(undefined, { maximumFractionDigits: 0 })}</b>
              </span>
            )}
            {source && <span className={styles.legendSource}>{SOURCE_NAMES[source]}</span>}
          </div>
        )}
        {(byPane.get(0) ?? []).map(legendRow)}
      </div>
      {[...byPane.entries()]
        .filter(([pane]) => pane > 0 && panes[pane])
        .map(([pane, views]) => (
          <div key={pane} className={styles.legends} style={{ top: panes[pane]!.top + 4 }}>
            {views.map(legendRow)}
          </div>
        ))}
      {Object.entries(tables).map(([key, list]) => {
        const s = shown.current.get(key);
        if (!s || s.hidden || !list.length) return null;
        const pane = panes[s.ownAnchor ? s.anchor.getPane().paneIndex() : 0];
        if (!pane) return null;
        return (
          <div
            key={key}
            className={styles.tables}
            style={{ top: pane.top, height: pane.height, width: Math.max(0, paneWidth) }}
          >
            {list.map((t) => (
              <ScriptTable key={t.id} table={t} />
            ))}
          </div>
        );
      })}
    </div>
  );
});

/** One indicator's legend row: its name and settings, its values at the crosshair, its controls. */
function IndicatorLegend({
  view,
  shown,
  time,
  chartPrecision,
  onAction,
}: {
  view: IndicatorView;
  shown: Shown | undefined;
  time: number | null;
  chartPrecision: number;
  onAction: (action: IndicatorAction) => void;
}) {
  const meta = shown?.meta;
  const args = meta && view.legendInputs !== false ? legendInputs(meta.inputs) : [];
  const row = time !== null ? shown?.byTime.get(time) : undefined;
  const p = meta ? precisionOf(meta, chartPrecision) : chartPrecision;
  return (
    <div className={cx(styles.indicator, view.hidden && styles.indicatorHidden)}>
      <span className={styles.indicatorName} title={meta?.name ?? view.name}>
        {meta?.short || view.name}
        {args.length > 0 && <span className={styles.indicatorArgs}> {args.join(' ')}</span>}
      </span>
      {view.status === 'loading' && <Spinner size={10} />}
      {view.status === 'error' && (
        <span className={styles.indicatorError} title={view.message}>
          <TriangleAlert size={12} aria-hidden /> {view.message}
        </span>
      )}
      {view.status === 'signin' && <span className={styles.indicatorNote}>Sign in to TradingView to show it</span>}
      {view.offTimeframe && !view.hidden && <span className={styles.indicatorNote}>Not shown on this timeframe</span>}
      {view.status === 'ready' &&
        !concealed(view) &&
        view.legendValues !== false &&
        meta?.plots
          .filter((plot) => plot.kind !== 'shapes' && !plot.hidden)
          .map((plot) => {
            const v = row?.[shown!.columns.get(plot.id) ?? -1];
            const color = row ? colorAt(plot, row, shown!.columns) : plot.color;
            return (
              <b key={plot.id} title={plot.title} style={{ color: color === TRANSPARENT ? plot.color : color }}>
                {formatValue(v, p)}
              </b>
            );
          })}
      <span className={styles.indicatorActions}>
        {view.status === 'error' && (
          <button type="button" aria-label="Try again" title="Try again" onClick={() => onAction('retry')}>
            <RotateCcw size={12} />
          </button>
        )}
        {view.status !== 'error' && (
          <button
            type="button"
            aria-label={view.hidden ? 'Show' : 'Hide'}
            title={view.hidden ? 'Show' : 'Hide'}
            onClick={() => onAction('toggle')}
          >
            {view.hidden ? <EyeOff size={12} /> : <Eye size={12} />}
          </button>
        )}
        {meta && !view.drawing && (
          <button type="button" aria-label="Settings" title="Settings" onClick={() => onAction('settings')}>
            <Settings2 size={12} />
          </button>
        )}
        <button type="button" aria-label="Remove" title="Remove" onClick={() => onAction('remove')}>
          <X size={12} />
        </button>
      </span>
    </div>
  );
}
