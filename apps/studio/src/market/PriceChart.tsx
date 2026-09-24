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
import type { Bar } from '@/lib/types';
import styles from './MarketWindow.module.css';

export interface PriceChartHandle {
  /** Replaces all data and fits it. */
  setBars: (bars: Bar[]) => void;
  /** Adds older bars on the left without moving the view. */
  prependBars: (older: Bar[]) => void;
  /** Applies a live update of the last (or a new) bar. */
  updateBar: (bar: Bar) => void;
}

interface Props {
  pricescale: number;
  /** Called when the view nears the oldest bar, to load more history. */
  onNeedMore: () => void;
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

function toVolume(b: Bar) {
  return { time: b.t as UTCTimestamp, value: b.v, color: b.c >= b.o ? 'rgba(79, 207, 138, 0.28)' : 'rgba(239, 107, 107, 0.28)' };
}

/** Candlesticks and volume, in local time, drawn with TradingView's lightweight-charts. */
export const PriceChart = forwardRef<PriceChartHandle, Props>(function PriceChart({ pricescale, onNeedMore }, ref) {
  const container = useRef<HTMLDivElement>(null);
  const chart = useRef<IChartApi | null>(null);
  const candles = useRef<ISeriesApi<'Candlestick'> | null>(null);
  const volume = useRef<ISeriesApi<'Histogram'> | null>(null);
  const data = useRef<Bar[]>([]);
  const needMore = useRef(onNeedMore);
  needMore.current = onNeedMore;
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
      if (range && range.from < 15 && data.current.length > 0) needMore.current();
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
    setBars: (bars) => {
      data.current = [...bars].sort((a, b) => a.t - b.t);
      candles.current?.setData(data.current.map(toCandle));
      volume.current?.setData(data.current.map(toVolume));
      chart.current?.timeScale().fitContent();
      if (data.current.length > 150) {
        chart.current?.timeScale().setVisibleLogicalRange({ from: data.current.length - 150, to: data.current.length + 5 });
      }
    },
    prependBars: (older) => {
      const first = data.current[0]?.t ?? Number.MAX_SAFE_INTEGER;
      const fresh = older.filter((b) => b.t < first).sort((a, b) => a.t - b.t);
      if (!fresh.length || !chart.current) return;
      const range = chart.current.timeScale().getVisibleLogicalRange();
      data.current = [...fresh, ...data.current];
      candles.current?.setData(data.current.map(toCandle));
      volume.current?.setData(data.current.map(toVolume));
      if (range) {
        chart.current.timeScale().setVisibleLogicalRange({ from: range.from + fresh.length, to: range.to + fresh.length });
      }
    },
    updateBar: (bar) => {
      const last = data.current[data.current.length - 1];
      if (last && bar.t < last.t) return;
      if (last && bar.t === last.t) data.current[data.current.length - 1] = bar;
      else data.current.push(bar);
      candles.current?.update(toCandle(bar));
      volume.current?.update(toVolume(bar));
    },
  }));

  const shown = legend ?? data.current[data.current.length - 1] ?? null;
  return (
    <div className={styles.chartWrap}>
      <div ref={container} className={styles.chart} />
      {shown && (
        <div className={styles.legend}>
          <span>O <b>{formatPrice(shown.o, pricescale)}</b></span>
          <span>H <b>{formatPrice(shown.h, pricescale)}</b></span>
          <span>L <b>{formatPrice(shown.l, pricescale)}</b></span>
          <span>C <b className={shown.c >= shown.o ? styles.up : styles.down}>{formatPrice(shown.c, pricescale)}</b></span>
          {shown.v > 0 && <span>V <b>{shown.v.toLocaleString(undefined, { maximumFractionDigits: 0 })}</b></span>}
        </div>
      )}
    </div>
  );
});
