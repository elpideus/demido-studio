// Demido's timeframe names, and what TradingView calls them.

import { RpcError } from './protocol.ts';

export type Timeframe = '1m' | '5m' | '15m' | '30m' | '1h' | '4h' | '1d' | '1w' | '1M';

interface TimeframeSpec {
  tradingview: string;
  /** Nominal length; months vary, so month boundaries come from store/aggregate.ts. */
  seconds: number;
}

export const TIMEFRAMES: Record<Timeframe, TimeframeSpec> = {
  '1m': { tradingview: '1', seconds: 60 },
  '5m': { tradingview: '5', seconds: 300 },
  '15m': { tradingview: '15', seconds: 900 },
  '30m': { tradingview: '30', seconds: 1800 },
  '1h': { tradingview: '60', seconds: 3600 },
  '4h': { tradingview: '240', seconds: 14400 },
  '1d': { tradingview: '1D', seconds: 86400 },
  '1w': { tradingview: '1W', seconds: 604800 },
  '1M': { tradingview: '1M', seconds: 2592000 },
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
