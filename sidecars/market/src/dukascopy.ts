// Maps TradingView symbols to Dukascopy instruments, which is how history from both sources is
// joined into one series, and decides which source a symbol's history comes from. Every request to
// Dukascopy itself goes through store/fetcher.ts.

import dukascopy from 'dukascopy-node';

import { type Bar } from './protocol.ts';

const { instrumentMetaData } = dukascopy as unknown as {
  instrumentMetaData: Record<string, InstrumentMeta>;
};

interface InstrumentMeta {
  name: string;
  description: string;
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
/** Equity venues: a ticker there is a stock or an ETF (NASDAQ:DAX is an ETF, NYSE:WTI a stock), never
 *  the index or commodity an alias names. CBOE is not one: it publishes VIX and SPX itself. */
const STOCK_EXCHANGES = new Set(['NASDAQ', 'NYSE', 'AMEX', 'NYSEARCA', 'ARCA', 'BATS', 'CBOEEU']);
/** Indices a stock exchange publishes under its own prefix. */
const EXCHANGE_INDICES = new Set(['NASDAQ:NDX']);
const CRYPTO_QUOTES = ['USDT', 'USDC', 'BUSD', 'FDUSD', 'USD'];

function known(key: string): string | null {
  return Object.hasOwn(instrumentMetaData, key) ? key : null;
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
  if (ALIASES[t] && (!STOCK_EXCHANGES.has(ex) || EXCHANGE_INDICES.has(`${ex}:${t}`))) return known(ALIASES[t]);

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
  const meta = known(key) ? instrumentMetaData[key] : undefined;
  return { name: meta?.name ?? key, description: meta?.description ?? key };
}

/**
 * Stock and ETF CFDs (`AAPL.US/USD`, `AAL.GB/GBX`): Dukascopy's prices for them are not
 * split-adjusted and their volumes are in other units than TradingView's, so they are never a
 * history source.
 */
export function isStockCfd(key: string): boolean {
  return /\.[A-Z]{2}\//.test(instrumentInfo(key).name);
}

/** The Dukascopy instrument a symbol's history comes from, or null when TradingView is the source. */
export function historyInstrument(symbol: string): string | null {
  const key = resolveInstrument(symbol);
  return key && !isStockCfd(key) ? key : null;
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
