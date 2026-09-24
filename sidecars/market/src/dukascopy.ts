// Historical candles from Dukascopy's public data feed (no account needed), through
// dukascopy-node. Also maps TradingView symbols to Dukascopy instruments, which is how history
// from both sources is joined into one series.

import path from 'node:path';

import dukascopy from 'dukascopy-node';

import { type Bar, RpcError } from './protocol.ts';
import { TIMEFRAMES, type Timeframe } from './timeframes.ts';

const { getHistoricalRates, instrumentMetaData } = dukascopy as unknown as {
  getHistoricalRates: (config: Record<string, unknown>) => Promise<unknown>;
  instrumentMetaData: Record<string, InstrumentMeta>;
};

interface InstrumentMeta {
  name: string;
  description: string;
  startDayForMinuteCandles: string;
  startMonthForHourlyCandles: string;
  startYearForDailyCandles: string;
}

/** Common index, commodity and alias tickers, as TradingView and brokers spell them. */
const ALIASES: Record<string, string> = {
  SPX: 'usa500idxusd',
  SPX500: 'usa500idxusd',
  US500: 'usa500idxusd',
  SP500: 'usa500idxusd',
  'ES1!': 'usa500idxusd',
  NDX: 'usatechidxusd',
  NAS100: 'usatechidxusd',
  US100: 'usatechidxusd',
  USTEC: 'usatechidxusd',
  'NQ1!': 'usatechidxusd',
  DJI: 'usa30idxusd',
  US30: 'usa30idxusd',
  DJ30: 'usa30idxusd',
  'YM1!': 'usa30idxusd',
  RUT: 'ussc2000idxusd',
  US2000: 'ussc2000idxusd',
  DAX: 'deuidxeur',
  GER40: 'deuidxeur',
  DE40: 'deuidxeur',
  GER30: 'deuidxeur',
  UKX: 'gbridxgbp',
  UK100: 'gbridxgbp',
  FTSE: 'gbridxgbp',
  NI225: 'jpnidxjpy',
  JP225: 'jpnidxjpy',
  JPN225: 'jpnidxjpy',
  PX1: 'fraidxeur',
  FRA40: 'fraidxeur',
  FR40: 'fraidxeur',
  CAC40: 'fraidxeur',
  HSI: 'hkgidxhkd',
  HK50: 'hkgidxhkd',
  SX5E: 'eusidxeur',
  EU50: 'eusidxeur',
  STOXX50: 'eusidxeur',
  XJO: 'ausidxaud',
  AUS200: 'ausidxaud',
  DXY: 'dollaridxusd',
  VIX: 'volidxusd',
  GOLD: 'xauusd',
  SILVER: 'xagusd',
  PLATINUM: 'xptcmdusd',
  PALLADIUM: 'xpdcmdusd',
  COPPER: 'coppercmdusd',
  USOIL: 'lightcmdusd',
  WTI: 'lightcmdusd',
  'CL1!': 'lightcmdusd',
  UKOIL: 'brentcmdusd',
  BRENT: 'brentcmdusd',
  'BZ1!': 'brentcmdusd',
  NATGAS: 'gascmdusd',
  'NG1!': 'gascmdusd',
};

const US_EXCHANGES = new Set(['NASDAQ', 'NYSE', 'AMEX', 'NYSEARCA', 'ARCA', 'BATS', 'CBOE', 'CBOEEU']);
const CRYPTO_QUOTES = ['USDT', 'USDC', 'BUSD', 'FDUSD', 'USD'];

let cacheDir: string | null = null;

export function setCacheDir(dir: string): void {
  cacheDir = path.join(dir, 'dukascopy');
}

function known(key: string): string | null {
  return key in instrumentMetaData ? key : null;
}

/**
 * The Dukascopy instrument for a symbol: a TradingView id (`FX:EURUSD`, `NASDAQ:AAPL`,
 * `BINANCE:BTCUSDT`), a plain ticker (`EURUSD`, `US500`, `AAPL`) or a Dukascopy key
 * (`usa500idxusd`). Returns null when Dukascopy has no match.
 */
export function resolveInstrument(symbol: string): string | null {
  const raw = symbol.trim();
  const [exchange, ticker] = raw.includes(':') ? raw.split(':', 2) : ['', raw];
  const t = (ticker ?? '').toUpperCase().replace(/\s+/g, '');
  const ex = (exchange ?? '').toUpperCase();
  if (!t) return null;

  const direct = known(t.toLowerCase().replace(/[^a-z0-9]/g, ''));
  if (direct) return direct;
  if (ALIASES[t]) return known(ALIASES[t]);

  // Crypto pairs quoted in stablecoins trade like the USD pair.
  const plain = t.replace(/\.P$/, '').replace(/PERP$/, '');
  for (const q of CRYPTO_QUOTES) {
    if (plain.endsWith(q) && plain.length > q.length) {
      const hit = known(`${plain.slice(0, -q.length).toLowerCase()}usd`);
      if (hit) return hit;
    }
  }

  // US listed stocks and ETFs: AAPL → aaplususd.
  if (!ex || US_EXCHANGES.has(ex)) {
    const hit = known(`${t.toLowerCase().replace(/[^a-z0-9]/g, '')}ususd`);
    if (hit) return hit;
  }
  return null;
}

export function instrumentInfo(key: string): { name: string; description: string } {
  const meta = instrumentMetaData[key];
  return { name: meta?.name ?? key, description: meta?.description ?? key };
}

interface JsonCandle {
  timestamp: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume?: number;
}

/** Candles between two instants (seconds). Weekly candles are built from daily ones. */
export async function history(
  instrumentOrSymbol: string,
  tf: Timeframe,
  from: number,
  to: number,
): Promise<{ instrument: string; bars: Bar[] }> {
  const instrument = resolveInstrument(instrumentOrSymbol);
  if (!instrument) {
    throw new RpcError(
      'NOT_FOUND',
      `Dukascopy has no instrument for "${instrumentOrSymbol}". It covers forex pairs, metals, major indices (US500, NAS100, GER40...), large US stocks and major crypto.`,
    );
  }
  if (to <= from) throw new RpcError('BAD_REQUEST', 'The start date must be before the end date.');
  const spec = TIMEFRAMES[tf];
  const days = (to - from) / 86400;
  if (days > spec.maxDays) {
    throw new RpcError(
      'TOO_LARGE',
      `That range is too long for ${tf} candles (at most ${spec.maxDays} days). Use a larger timeframe or a shorter range.`,
    );
  }
  const now = Math.floor(Date.now() / 1000);
  const end = Math.min(to, now);
  const config = {
    instrument,
    dates: { from: new Date(from * 1000), to: new Date(end * 1000) },
    timeframe: spec.dukascopy ?? 'd1',
    format: 'json',
    priceType: 'bid',
    volumes: true,
    ignoreFlats: true,
    // Dukascopy answers 429 to bursts: few files at a time, with pauses and retries.
    batchSize: 4,
    pauseBetweenBatchesMs: 400,
    retryCount: 5,
    pauseBetweenRetriesMs: 1200,
    retryOnEmpty: true,
    useCache: cacheDir !== null,
    ...(cacheDir ? { cacheFolderPath: cacheDir } : {}),
  };
  let raw: JsonCandle[] = [];
  for (let attempt = 1; ; attempt += 1) {
    try {
      raw = (await getHistoricalRates(config)) as JsonCandle[];
      break;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (attempt >= 4 || !/429|ECONNRESET|ETIMEDOUT|socket hang up/i.test(message)) {
        throw new RpcError('NETWORK', `Dukascopy did not answer: ${message}`);
      }
      await new Promise((r) => setTimeout(r, 2000 * attempt));
    }
  }
  let bars: Bar[] = raw
    .filter((c) => Number.isFinite(c.close))
    .map((c) => ({
      t: Math.floor(c.timestamp / 1000),
      o: c.open,
      h: c.high,
      l: c.low,
      c: c.close,
      v: c.volume ?? 0,
    }));
  if (tf === '1w') bars = weekly(bars);
  return { instrument, bars };
}

/** Groups daily candles into weeks starting on Monday (UTC). */
export function weekly(daily: Bar[]): Bar[] {
  const out: Bar[] = [];
  for (const d of daily) {
    const date = new Date(d.t * 1000);
    const monday = Date.UTC(
      date.getUTCFullYear(),
      date.getUTCMonth(),
      date.getUTCDate() - ((date.getUTCDay() + 6) % 7),
    );
    const week = Math.floor(monday / 1000);
    const last = out[out.length - 1];
    if (last && last.t === week) {
      last.h = Math.max(last.h, d.h);
      last.l = Math.min(last.l, d.l);
      last.c = d.c;
      last.v += d.v;
    } else {
      out.push({ ...d, t: week });
    }
  }
  return out;
}
