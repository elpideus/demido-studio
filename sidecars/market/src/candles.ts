// One series from two sources: TradingView for recent and live candles, Dukascopy for the
// older part TradingView does not reach. The two never overlap: Dukascopy only fills the time
// before TradingView's first candle, so every instant comes from exactly one source.

import * as dukascopy from './dukascopy.ts';
import { type Bar } from './protocol.ts';
import { TIMEFRAMES, type Timeframe, estimateBars } from './timeframes.ts';
import * as tv from './tradingview.ts';

export interface SourceSpan {
  source: 'tradingview' | 'dukascopy';
  count: number;
  from: string | null;
  to: string | null;
}

export interface CandlesResult {
  symbol: string;
  info: tv.ChartInfo;
  timeframe: Timeframe;
  bars: Bar[];
  sources: SourceSpan[];
}

const MAX_BARS = 20_000;

function iso(seconds: number | undefined): string | null {
  return seconds === undefined ? null : new Date(seconds * 1000).toISOString();
}

export function span(source: SourceSpan['source'], bars: Bar[]): SourceSpan {
  return { source, count: bars.length, from: iso(bars[0]?.t), to: iso(bars[bars.length - 1]?.t) };
}

/** Joins older Dukascopy bars in front of TradingView bars without overlap. */
export function stitch(older: Bar[], newer: Bar[]): Bar[] {
  if (newer.length === 0) return older;
  const first = newer[0]!.t;
  return [...older.filter((b) => b.t < first), ...newer];
}

export async function candles(
  symbol: string,
  tf: Timeframe,
  count: number,
  from: number | null,
  to: number | null,
): Promise<CandlesResult> {
  const now = Math.floor(Date.now() / 1000);
  const end = Math.min(to ?? now, now);
  const wanted = Math.min(from ? estimateBars(from, end, tf) : count, MAX_BARS);
  const live = await tv.candles(symbol, tf, wanted, to ? end : undefined);
  let bars = live.bars.filter((b) => b.t <= end && (from === null || b.t >= from));
  const sources: SourceSpan[] = bars.length ? [span('tradingview', bars)] : [];

  // Fill the gap before TradingView's history from Dukascopy, when it knows the instrument.
  const instrument = dukascopy.resolveInstrument(live.info.symbol) ?? dukascopy.resolveInstrument(symbol);
  const firstLive = live.bars[0]?.t ?? end;
  const step = TIMEFRAMES[tf].seconds;
  const gapStart = from ?? (bars.length < count ? firstLive - (count - bars.length) * step * 1.5 : null);
  if (instrument && gapStart !== null && gapStart < firstLive - step) {
    try {
      const maxSpan = TIMEFRAMES[tf].maxDays * 86400;
      const start = Math.max(gapStart, firstLive - maxSpan);
      const older = await dukascopy.history(instrument, tf, Math.floor(start), firstLive);
      let fill = older.bars.filter((b) => b.t < firstLive);
      if (from === null) fill = fill.slice(-(count - bars.length));
      if (fill.length) {
        bars = stitch(fill, bars);
        sources.unshift(span('dukascopy', fill));
      }
    } catch {
      // History is a bonus: the live part stands on its own.
    }
  }
  return { symbol: live.info.symbol, info: live.info, timeframe: tf, bars, sources };
}
