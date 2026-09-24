// Demido's timeframe names, and what TradingView and Dukascopy call them.

import { RpcError } from './protocol.ts';

export type Timeframe = '1m' | '5m' | '15m' | '30m' | '1h' | '4h' | '1d' | '1w' | '1M';

interface TimeframeSpec {
  tradingview: string;
  /** Dukascopy timeframe, or null when it has to be built from daily candles. */
  dukascopy: 'm1' | 'm5' | 'm15' | 'm30' | 'h1' | 'h4' | 'd1' | 'mn1' | null;
  seconds: number;
  /** Longest Dukascopy range fetched in one request, in days. */
  maxDays: number;
}

export const TIMEFRAMES: Record<Timeframe, TimeframeSpec> = {
  '1m': { tradingview: '1', dukascopy: 'm1', seconds: 60, maxDays: 62 },
  '5m': { tradingview: '5', dukascopy: 'm5', seconds: 300, maxDays: 186 },
  '15m': { tradingview: '15', dukascopy: 'm15', seconds: 900, maxDays: 366 },
  '30m': { tradingview: '30', dukascopy: 'm30', seconds: 1800, maxDays: 732 },
  '1h': { tradingview: '60', dukascopy: 'h1', seconds: 3600, maxDays: 365 * 5 },
  '4h': { tradingview: '240', dukascopy: 'h4', seconds: 14400, maxDays: 365 * 10 },
  '1d': { tradingview: '1D', dukascopy: 'd1', seconds: 86400, maxDays: 365 * 40 },
  '1w': { tradingview: '1W', dukascopy: null, seconds: 604800, maxDays: 365 * 40 },
  '1M': { tradingview: '1M', dukascopy: 'mn1', seconds: 2592000, maxDays: 365 * 50 },
};

export function timeframe(value: unknown): Timeframe {
  if (typeof value === 'string' && value in TIMEFRAMES) return value as Timeframe;
  throw new RpcError(
    'BAD_REQUEST',
    `Unknown timeframe ${String(value)}. Use one of ${Object.keys(TIMEFRAMES).join(', ')}.`,
  );
}

/** Parses `YYYY-MM-DD` (or any ISO date) to seconds. `endOfDay` moves a bare date to 23:59:59. */
export function parseDate(value: unknown, endOfDay = false): number | null {
  if (value === undefined || value === null || value === '') return null;
  if (typeof value === 'number') return value > 1e12 ? Math.floor(value / 1000) : value;
  const text = String(value).trim();
  const bare = /^\d{4}-\d{2}-\d{2}$/.test(text);
  const ms = Date.parse(bare ? `${text}T00:00:00Z` : text);
  if (Number.isNaN(ms)) {
    throw new RpcError('BAD_REQUEST', `Could not read the date "${text}". Use YYYY-MM-DD.`);
  }
  const seconds = Math.floor(ms / 1000);
  return bare && endOfDay ? seconds + 86399 : seconds;
}

/** Rough candle count between two instants for a timeframe (markets closed at weekends ignored). */
export function estimateBars(from: number, to: number, tf: Timeframe): number {
  return Math.max(1, Math.ceil((to - from) / TIMEFRAMES[tf].seconds));
}
