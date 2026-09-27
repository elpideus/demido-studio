import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react';
import {
  CandlestickSeries,
  ColorType,
  CrosshairMode,
  HistogramSeries,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type Time,
  type UTCTimestamp,
} from 'lightweight-charts';

import { formatPrice } from '@/lib/format';
import type { Bar, MarketSource, MarketSpan } from '@/lib/types';
import { mergeSpans, sourceAt, sourcesOf } from './chartData';
import { SOURCE_NAMES } from './DownloadProgress';
import styles from './MarketWindow.module.css';

export interface PriceChartHandle {
  /** Replaces all data and fits it, prices included. `spans` say which source each bar came from. */
  setBars: (bars: Bar[], spans: MarketSpan[]) => void;
  /** Adds older bars on the left without moving the view. Returns how many were older than the
   *  chart's oldest bar and so were actually added. */
  prependBars: (older: Bar[], spans: MarketSpan[]) => number;
  /** Applies a live update of the last (or a new) bar; it belongs to the newest bar's source. */
  updateBar: (bar: Bar) => void;
  /** Whether the view shows (or nearly shows) the oldest loaded bar. */
  nearOldest: () => boolean;
}

/** How close (in bars) the view's left edge gets to the oldest bar before more are asked for. */
const NEAR_OLDEST = 15;

interface Props {
  pricescale: number;
  /** Called when the view nears the oldest bar, to load more history. */
  onNeedMore: () => void;
  /** The oldest loaded bar's screen position, recomputed on every pan/zoom (null when there is
   *  no data or it is off-screen) — lets an overlay track that edge as the chart moves. */
  onEdgeAnchor?: (pos: { x: number; y: number } | null) => void;
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

/** Candlesticks and volume, in local time, drawn with TradingView's lightweight-charts. */
export const PriceChart = forwardRef<PriceChartHandle, Props>(function PriceChart(
  { pricescale, onNeedMore, onEdgeAnchor },
  ref,
) {
  const container = useRef<HTMLDivElement>(null);
  const chart = useRef<IChartApi | null>(null);
  const candles = useRef<ISeriesApi<'Candlestick'> | null>(null);
  const volume = useRef<ISeriesApi<'Histogram'> | null>(null);
  const data = useRef<Bar[]>([]);
  const spans = useRef<MarketSpan[]>([]);
  const needMore = useRef(onNeedMore);
  needMore.current = onNeedMore;
  const edgeAnchor = useRef(onEdgeAnchor);
  edgeAnchor.current = onEdgeAnchor;
  const [legend, setLegend] = useState<Bar | null>(null);

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
      setLegend(data.current.find((b) => b.t === param.time) ?? null);
    });
    chart.current = c;
    candles.current = cs;
    volume.current = vs;
    return () => {
      c.remove();
      chart.current = null;
    };
  }, []);

  useEffect(() => {
    const precision = Math.min(8, Math.max(0, Math.round(Math.log10(pricescale || 100))));
    candles.current?.applyOptions({
      priceFormat: { type: 'price', precision, minMove: 1 / 10 ** precision },
    });
  }, [pricescale]);

  useImperativeHandle(ref, () => ({
    setBars: (bars, sourceSpans) => {
      data.current = [...bars].sort((a, b) => a.t - b.t);
      spans.current = mergeSpans([], sourceSpans);
      // Dragging the price axis or the chart turns auto-scaling off, which would keep the old
      // symbol's price range on screen: another symbol can sit thousands of times higher.
      candles.current?.priceScale().applyOptions({ autoScale: true });
      volume.current?.priceScale().applyOptions({ autoScale: true });
      candles.current?.setData(data.current.map(toCandle));
      volume.current?.setData(volumes(data.current, spans.current));
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
      // Spans reaching into bars the chart already had describe those bars too; they agree.
      spans.current = mergeSpans(spans.current, sourceSpans);
      candles.current?.setData(data.current.map(toCandle));
      volume.current?.setData(volumes(data.current, spans.current));
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
      if (last && bar.t === last.t) data.current[data.current.length - 1] = bar;
      else data.current.push(bar);
      candles.current?.update(toCandle(bar));
      volume.current?.update(toVolume(bar, true));
    },
    nearOldest: () => {
      const range = chart.current?.timeScale().getVisibleLogicalRange();
      return !!range && data.current.length > 0 && range.from < NEAR_OLDEST;
    },
  }));

  const shown = legend ?? data.current[data.current.length - 1] ?? null;
  const source: MarketSource | null = shown ? sourceAt(spans.current, shown.t) : null;
  return (
    <div className={styles.chartWrap}>
      <div ref={container} className={styles.chart} />
      {shown && (
        <div className={styles.legend}>
          <span>
            O <b>{formatPrice(shown.o, pricescale)}</b>
          </span>
          <span>
            H <b>{formatPrice(shown.h, pricescale)}</b>
          </span>
          <span>
            L <b>{formatPrice(shown.l, pricescale)}</b>
          </span>
          <span>
            C <b className={shown.c >= shown.o ? styles.up : styles.down}>{formatPrice(shown.c, pricescale)}</b>
          </span>
          {shown.v > 0 && (
            <span>
              V <b>{shown.v.toLocaleString(undefined, { maximumFractionDigits: 0 })}</b>
            </span>
          )}
          {source && <span className={styles.legendSource}>{SOURCE_NAMES[source]}</span>}
        </div>
      )}
    </div>
  );
});
