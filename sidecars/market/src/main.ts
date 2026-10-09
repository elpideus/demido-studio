// Demido Studio's market data service. Started by the app, it reads requests on stdin and
// answers on stdout (see protocol.ts). On `shutdown` or when the app closes its stdin, it flushes
// the store and exits.
//
// Everything stored lives under DEMIDO_CACHE_DIR: Dukascopy buckets (store/dukascopy-store),
// TradingView bars (store/tv-store) and download jobs (store/jobs). The Pine scripts written in
// Demido are not a cache: they live under DEMIDO_PINE_DIR (indicators/pine). Every Dukascopy request goes
// through the one fetcher queue; every read goes through store/series. The old caches are migrated
// first: store requests wait for it.

import os from 'node:os';
import path from 'node:path';
import { createInterface } from 'node:readline';

import './net.ts';
import './http.ts';
import * as dukascopy from './dukascopy.ts';
import * as catalog from './indicators/catalog.ts';
import { type LayoutState, describe as describeMeta } from './indicators/describe.ts';
import * as pine from './indicators/pine.ts';
import * as studies from './indicators/studies.ts';
import { type Bar, RpcError, emit, fail, log, reply } from './protocol.ts';
import { instrumentMeta } from './store/buckets.ts';
import { DukascopyStore } from './store/dukascopy-store.ts';
import { Fetcher } from './store/fetcher.ts';
import { type Origin, Jobs } from './store/jobs.ts';
import { migrate } from './store/migrate.ts';
import { type Back, Planner } from './store/planner.ts';
import { IntervalSet, type Range } from './store/intervals.ts';
import { INTERACTIVE_BUDGET, Reads } from './store/reads.ts';
import { type ExportSummary, type ReadResult, Series, type Source } from './store/series.ts';
import { TvStore } from './store/tv-store.ts';
import { TIMEFRAMES, type Timeframe, parseDate, timeframe } from './timeframes.ts';
import * as tv from './tradingview.ts';

// Libraries must never write to stdout: it carries the protocol.
console.log = (...args: unknown[]) => process.stderr.write(`${args.map(String).join(' ')}\n`);
console.info = console.log;

const VERSION = '0.2.0';
const FAR = 1e11;
/** How often due provisional buckets are swept after the one at start-up. */
const SWEEP_MS = 30 * 60_000;

const cacheDir = process.env.DEMIDO_CACHE_DIR || path.join(os.tmpdir(), 'demido-market-cache');
const pineLibrary = new pine.PineLibrary(process.env.DEMIDO_PINE_DIR || path.join(os.tmpdir(), 'demido-pine'));
pine.useLibrary(pineLibrary);
const store = new DukascopyStore(cacheDir, { log });
const fetcher = new Fetcher(store, { log });
const tvStore = new TvStore(cacheDir, { log });
tv.useStore(tvStore);
const series = new Series({ store, tv: tvStore });
// The planner asks the jobs what they will fetch; the jobs ask the planner what is missing.
let jobs: Jobs | null = null;
const planner = new Planner({ store, fetcher, tv: tvStore, series, jobs: () => jobs?.views() ?? [] });
jobs = new Jobs({
  cacheDir,
  store,
  fetcher,
  planner,
  series,
  tv: tvStore,
  tvSession: { loggedIn: () => tv.isLoggedIn(), pageHistory: (s, tf, opts) => tv.pageHistory(s, tf, opts) },
  emit,
  log,
});
const jobList = jobs;
const reads = new Reads({ store, fetcher, planner, series });

// ---------------------------------------------------------------------------------------------
// store.updated: at most one per second per key, carrying the union of what changed.

const updates = new Map<string, { source: Source; key: string; from: number; to: number }>();
const lastUpdate = new Map<string, number>();

function storeUpdated(source: Source, key: string, from: number, to: number): void {
  const id = `${source}:${key}`;
  const pending = updates.get(id);
  if (pending) {
    pending.from = Math.min(pending.from, from);
    pending.to = Math.max(pending.to, to);
    return;
  }
  const wait = 1000 - (Date.now() - (lastUpdate.get(id) ?? 0));
  if (wait <= 0) {
    lastUpdate.set(id, Date.now());
    emit('store.updated', { source, key, from, to });
    return;
  }
  const entry = { source, key, from, to };
  updates.set(id, entry);
  setTimeout(() => {
    updates.delete(id);
    lastUpdate.set(id, Date.now());
    emit('store.updated', entry);
  }, wait).unref();
}

store.subscribe((c) => storeUpdated('dukascopy', c.instrument, c.from, c.to));
tvStore.subscribe((c) => storeUpdated('tradingview', c.symbol, c.from, c.to));

// ---------------------------------------------------------------------------------------------
// Start-up: migrate, then load (and resume) jobs, then sweep due provisional buckets (bulk lane),
// again every half hour: a bucket fetched while open is due once it closes, sidecar running or not.

const ready: Promise<void> = (async () => {
  try {
    await migrate({ cacheDir, store, tv: tvStore, route: (s) => series.route(s), log });
  } catch (error) {
    log('error', `Migrating the old market cache failed: ${(error as Error).message}`);
  }
  await jobList.load();
  reads.sweepDue();
  setInterval(() => {
    if (!shuttingDown) reads.sweepDue();
  }, SWEEP_MS).unref();
})();

// ---------------------------------------------------------------------------------------------
// Parameters

type Params = Record<string, unknown>;

function str(params: Params, key: string, required = true): string {
  const v = params[key];
  if (typeof v === 'string' && v.trim()) return v.trim();
  if (required) throw new RpcError('BAD_REQUEST', `"${key}" is required.`);
  return '';
}

/** Pine source as sent, untrimmed (its line numbers must match the editor's). */
function source(params: Params, key = 'source'): string {
  const v = params[key];
  if (typeof v !== 'string') throw new RpcError('BAD_REQUEST', `"${key}" is required.`);
  return v;
}

function int(params: Params, key: string, fallback: number, min: number, max: number): number {
  const v = Number(params[key]);
  return Number.isFinite(v) && v > 0 ? Math.max(min, Math.min(Math.floor(v), max)) : fallback;
}

function time(params: Params, key: string, endOfDay = false): number | undefined {
  return parseDate(params[key], endOfDay) ?? undefined;
}

function tiers(params: Params): string[] | undefined {
  const v = params.tiers;
  return Array.isArray(v) && v.length ? v.map(String) : undefined;
}

/** `back`: a download of one more month or year of 1-minute history before what is stored. */
function back(params: Params): Back | undefined {
  return params.back === 'month' || params.back === 'year' ? params.back : undefined;
}

function origin(value: unknown): Origin {
  return value === 'chat' || value === 'data' ? value : 'chart';
}

/** What the editor shows of a compiled script: its inputs and plots. */
function describeBrief(metaInfo: Record<string, unknown>) {
  const m = describeMeta(metaInfo);
  return { name: m.name, overlay: m.overlay, inputs: m.inputs.filter((i) => !i.hidden), plots: m.plots, bands: m.bands };
}

/** An indicator's setup as the UI sends it (from a saved layout or its settings). */
function layoutState(v: unknown): LayoutState {
  const o = v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
  const record = (x: unknown) =>
    x && typeof x === 'object' && !Array.isArray(x) ? (x as Record<string, Record<string, unknown>>) : undefined;
  const state: LayoutState = {};
  if (record(o.inputs)) state.inputs = o.inputs as Record<string, unknown>;
  if (record(o.styles)) state.styles = record(o.styles);
  if (record(o.palettes)) state.palettes = record(o.palettes);
  if (Array.isArray(o.bands)) state.bands = o.bands;
  if (typeof o.overlay === 'boolean') state.overlay = o.overlay;
  return state;
}

const nowSec = () => Math.floor(Date.now() / 1000);

// ---------------------------------------------------------------------------------------------
// Reads

/** Prices quoted to at most this many decimals, as a TradingView-style pricescale. */
function priceScale(prices: number[]): number {
  let decimals = 0;
  for (const p of prices) {
    const text = String(p);
    const dot = text.indexOf('.');
    if (dot >= 0 && !text.includes('e')) decimals = Math.max(decimals, text.length - dot - 1);
  }
  return 10 ** Math.min(decimals, 8);
}

function infoFor(symbol: string, prices: number[]): Record<string, unknown> {
  const route = series.route(symbol);
  if (route.instrument) {
    const meta = dukascopy.instrumentInfo(route.instrument);
    const quote = /\/([A-Z]{3})$/.exec(meta.name)?.[1] ?? '';
    return {
      symbol: meta.name,
      description: meta.description,
      exchange: 'Dukascopy',
      currency: quote,
      type: '',
      pricescale: priceScale(prices),
      timezone: 'Etc/UTC',
    };
  }
  const stored = tvStore.info(route.tvKey);
  return stored ?? { symbol: route.tvKey, description: '', exchange: '', pricescale: priceScale(prices) };
}

const lastPrices = (bars: Bar[]) => bars.slice(-100).flatMap((b) => [b.o, b.h, b.l, b.c]);

function page(read: ReadResult): Record<string, unknown> {
  return {
    bars: read.bars,
    spans: read.spans,
    more: read.more,
    ...(read.gap ? { gap: read.gap } : {}),
    keys: read.keys,
  };
}

function csvResult(symbol: string, tf: Timeframe, summary: ExportSummary, missing: Range[]) {
  const prices = summary.lastRows.flatMap((r) => [r[1], r[2], r[3], r[4]]);
  const info = infoFor(symbol, prices);
  return {
    symbol: info.symbol,
    info,
    timeframe: tf,
    count: summary.count,
    from: summary.from,
    to: summary.to,
    firstClose: summary.firstClose,
    lastClose: summary.lastClose,
    high: summary.high,
    low: summary.low,
    spans: summary.spans,
    lastRows: summary.lastRows,
    closes: summary.closes,
    missing,
    truncated: summary.truncated,
  };
}

function barsResult(symbol: string, tf: Timeframe, read: ReadResult, missing: Range[]) {
  const info = infoFor(symbol, lastPrices(read.bars));
  const counts = new Map<Source, number>();
  for (const s of read.spans) counts.set(s.source, (counts.get(s.source) ?? 0) + s.count);
  return {
    symbol: info.symbol,
    info,
    timeframe: tf,
    bars: read.bars,
    spans: read.spans,
    sources: [...counts].map(([source, count]) => ({ source, count })),
    missing,
    truncated: read.truncated,
  };
}

// ---------------------------------------------------------------------------------------------
// What is stored (cache.summary)

interface TierCoverage {
  tier: string;
  intervals: Range[];
  empty?: Range[];
  available: [number, number] | null;
  learnedStart?: number | null;
  bytes: number;
}

interface CoverageItem {
  market: string;
  name: string;
  symbols: string[];
  sources: Array<{ source: Source; key: string; bytes: number; tiers: TierCoverage[] }>;
  jobs: ReturnType<Jobs['list']>;
}

/** The market a TradingView symbol belongs to: its Dukascopy instrument when it has one. */
const marketOf = (symbol: string) => dukascopy.historyInstrument(symbol) ?? symbol.toUpperCase();

async function summary(symbol?: string): Promise<{ bytes: number; items: CoverageItem[] }> {
  const now = nowSec();
  const items = new Map<string, CoverageItem>();
  const item = (market: string, name: string) => {
    let it = items.get(market);
    if (!it) {
      it = { market, name, symbols: [], sources: [], jobs: [] };
      items.set(market, it);
    }
    return it;
  };
  for (const instrument of store.instruments()) {
    const s = store.summary(instrument);
    // History is 1-minute candles: the one tier a market's Dukascopy coverage is about.
    const tiersOut: TierCoverage[] = (['m1'] as const).map((tier) => {
      const t = s.tiers[tier];
      // A permanent 400/404 is "no data at the source" too.
      const unavailable = new IntervalSet(t.unavailable);
      return {
        tier,
        intervals: new IntervalSet(t.covered).addSet(unavailable).toJSON(),
        empty: new IntervalSet(t.empty).addSet(unavailable).toJSON(),
        available: t.effectiveStart === null ? null : [t.effectiveStart, now],
        learnedStart: t.learnedStart?.t ?? null,
        bytes: t.bytes,
      };
    });
    item(instrument, dukascopy.instrumentInfo(instrument).name).sources.push({
      source: 'dukascopy',
      key: instrument,
      bytes: s.bytes,
      tiers: tiersOut,
    });
  }
  const bySymbol = new Map<string, ReturnType<TvStore['series']>>();
  for (const sr of tvStore.series()) {
    const list = bySymbol.get(sr.symbol);
    if (list) list.push(sr);
    else bySymbol.set(sr.symbol, [sr]);
  }
  for (const [sym, list] of bySymbol) {
    const market = marketOf(sym);
    const it = item(market, market === sym ? sym : dukascopy.instrumentInfo(market).name);
    if (!it.symbols.includes(sym)) it.symbols.push(sym);
    it.sources.push({
      source: 'tradingview',
      key: sym,
      bytes: list.reduce((sum, sr) => sum + sr.bytes, 0),
      tiers: list.map((sr) => ({
        tier: sr.tf,
        intervals: sr.intervals,
        available: [sr.reachedStart ?? sr.first ?? sr.intervals[0]?.[0] ?? now, now] as [number, number],
        bytes: sr.bytes,
      })),
    });
  }
  for (const job of jobList.list()) {
    if (job.status === 'done') continue;
    const market = job.source === 'dukascopy' ? job.key : marketOf(job.key);
    const it = item(market, job.name);
    if (job.source === 'tradingview' && !it.symbols.includes(job.key)) it.symbols.push(job.key);
    it.jobs.push(job);
  }
  let list = [...items.values()];
  if (symbol?.trim()) {
    const route = series.route(symbol);
    const market = route.instrument ?? marketOf(route.tvKey);
    list = list.filter((it) => it.market === market);
  }
  list.sort((a, b) => a.name.localeCompare(b.name));
  const bytes = list.reduce((sum, it) => sum + it.sources.reduce((s, src) => s + src.bytes, 0), 0);
  return { bytes, items: list };
}

const isDukascopyMarket = (market: string) => instrumentMeta(market) !== null;

// ---------------------------------------------------------------------------------------------
// Handlers

let exitAfterReply = false;
let shuttingDown: Promise<void> | null = null;

/** Flushes everything (manifests, jobs, TradingView bars). Jobs keep their status, so they resume. */
function shutdown(): Promise<void> {
  shuttingDown ??= (async () => {
    await ready.catch(() => undefined);
    await jobList.close();
    await fetcher.close(500);
    await store.close();
    await tvStore.flush();
  })().catch((error: unknown) => log('error', `Could not save everything on exit: ${(error as Error).message}`));
  return shuttingDown;
}

const handlers: Record<string, (params: Params) => Promise<unknown> | unknown> = {
  ping: () => ({ version: VERSION, loggedIn: tv.isLoggedIn(), username: tv.currentUser() }),

  'auth.set': (p) => tv.setCredentials({ session: str(p, 'session'), signature: str(p, 'signature', false) }),

  'auth.clear': () => {
    tv.clearCredentials();
    return {};
  },

  search: (p) => tv.search(str(p, 'query'), typeof p.type === 'string' ? p.type : undefined),

  quote: (p) => {
    const symbols = Array.isArray(p.symbols) ? p.symbols.map(String).filter(Boolean) : [];
    if (!symbols.length) throw new RpcError('BAD_REQUEST', 'Give at least one symbol.');
    return tv.quote(symbols.slice(0, 20));
  },

  'dukascopy.resolve': (p) => {
    const instrument = dukascopy.historyInstrument(str(p, 'symbol'));
    return instrument ? { instrument, ...dukascopy.instrumentInfo(instrument) } : null;
  },

  // The signed-out chart: what is stored, after fetching what the newest bars need.
  'bars.latest': async (p) => {
    await ready;
    const symbol = str(p, 'symbol');
    const tf = timeframe(p.timeframe);
    const count = int(p, 'count', 500, 1, 5000);
    const route = series.route(symbol);
    const read = await reads.latest(symbol, tf, count);
    const info = infoFor(symbol, lastPrices(read.bars));
    return {
      symbol: route.instrument ? info.symbol : route.tvKey,
      info,
      timeframe: tf,
      ...page(read),
      ...(read.fetchError ? { fetchError: read.fetchError } : {}),
      ...(!route.instrument && !tv.isLoggedIn() ? { stale: true } : {}),
    };
  },

  // Chart paging, both modes: the store only.
  'bars.older': async (p) => {
    await ready;
    const before = time(p, 'before');
    if (before === undefined) throw new RpcError('BAD_REQUEST', '"before" is required.');
    const read = await series.read({
      symbol: str(p, 'symbol'),
      tf: timeframe(p.timeframe),
      before,
      count: int(p, 'count', 500, 1, 5000),
      // A `before` in the future asks for the newest stored bars (the signed-out chart picking up a
      // download), even when the store ends before now.
      latest: before > nowSec(),
    });
    return page(read);
  },

  // A signed-in chart opened: bring the stored history up to now (the chart refreshes on store.updated).
  'bars.freshen': async (p) => {
    await ready;
    const symbol = str(p, 'symbol');
    const tf = timeframe(p.timeframe);
    const instrument = series.route(symbol).instrument;
    if (!instrument) return { requests: 0 };
    const now = nowSec();
    // The same stretch whatever the timeframe (see reads.recentFrom): history is 1-minute candles,
    // and anything older than the newest stored data is an explicit download.
    const from = reads.recentFrom(symbol, now);
    const needs = await planner.interactive(symbol, tf, from, now + TIMEFRAMES[tf].seconds, INTERACTIVE_BUDGET);
    for (const n of needs) {
      fetcher.request({ instrument, tier: n.tier, start: n.start, lane: 'interactive' }).catch(() => undefined);
    }
    return { requests: needs.length };
  },

  'download.plan': async (p) => {
    await ready;
    return planner.plan({
      symbol: str(p, 'symbol'),
      from: time(p, 'from'),
      back: back(p),
      to: time(p, 'to', true),
      tiers: tiers(p),
    });
  },

  'download.start': async (p) => {
    await ready;
    return jobList.start({
      symbol: str(p, 'symbol'),
      from: time(p, 'from'),
      back: back(p),
      to: time(p, 'to', true),
      tiers: tiers(p),
      origin: origin(p.origin),
    });
  },

  'download.pause': async (p) => {
    await ready;
    await jobList.pause(str(p, 'jobId'));
    return {};
  },

  'download.resume': async (p) => {
    await ready;
    jobList.resume(str(p, 'jobId'));
    return {};
  },

  'download.cancel': async (p) => {
    await ready;
    await jobList.cancel(str(p, 'jobId'));
    return {};
  },

  'download.status': async (p) => {
    await ready;
    return jobList.get(str(p, 'jobId'));
  },

  'download.list': async (p) => {
    await ready;
    return jobList.list(str(p, 'symbol', false) || undefined);
  },

  'download.wait': async (p) => {
    await ready;
    const from = time(p, 'from');
    const to = time(p, 'to', true);
    if (from === undefined || to === undefined) throw new RpcError('BAD_REQUEST', '"from" and "to" are required.');
    const params = {
      symbol: str(p, 'symbol'),
      tf: timeframe(p.timeframe),
      from,
      to,
      jobId: str(p, 'jobId', false) || undefined,
      timeoutMs: int(p, 'timeoutMs', 90_000, 0, 600_000),
      // false: only watch (a progress card), without pushing the range into the waiting lane.
      boost: p.boost !== false,
    };
    return jobList.wait(params);
  },

  'cache.summary': async (p) => {
    await ready;
    return summary(str(p, 'symbol', false) || undefined);
  },

  'cache.delete': async (p) => {
    await ready;
    const market = str(p, 'market');
    const symbols = tvStore
      .series()
      .map((s) => s.symbol)
      .filter(
        (s, i, all) =>
          all.indexOf(s) === i && marketOf(s) === (isDukascopyMarket(market) ? market : market.toUpperCase()),
      );
    if ([market, ...symbols].some((k) => jobList.busy(k))) {
      throw new RpcError('BUSY', 'A download for this market is running. Pause or cancel it first.');
    }
    let bytes = 0;
    if (isDukascopyMarket(market)) bytes += await store.remove(market);
    for (const s of symbols) bytes += await tvStore.remove(s);
    return { bytes };
  },

  'cache.recheck': async (p) => {
    await ready;
    const market = str(p, 'market');
    if (isDukascopyMarket(market)) store.recheck(market);
    for (const s of tvStore.series())
      if (marketOf(s.symbol) === market || s.symbol === market.toUpperCase()) {
        tvStore.clearReachedStart(s.symbol);
      }
    return {};
  },

  // A tool read: the store only (the tool downloads what `missing` names first).
  history: async (p) => {
    await ready;
    const symbol = str(p, 'instrument');
    const tf = timeframe(p.timeframe);
    const from = time(p, 'from');
    if (from === undefined) throw new RpcError('BAD_REQUEST', '"from" is required (YYYY-MM-DD).');
    const to = Math.min(time(p, 'to', true) ?? nowSec(), nowSec());
    if (to < from) throw new RpcError('BAD_REQUEST', 'The start date must be before the end date.');
    const step = TIMEFRAMES[tf].seconds;
    // A bucket stored while still open stays partial until its re-fetch: do the due ones it shows.
    await reads.refreshDue(symbol, tf, from, to);
    // The newest bars change until their bucket closes: refresh the active bucket when stale.
    if (to >= nowSec() - 2 * step) {
      await reads.fetchFor(symbol, tf, Math.max(from, to - 2 * step), to + step, 4, 10_000);
    }
    const missing = series.missing(symbol, tf, from, to);
    const csvPath = str(p, 'csvPath', false);
    if (csvPath) {
      const out = await series.exportCsv({ symbol, tf, from, to, path: csvPath });
      return csvResult(symbol, tf, out, missing);
    }
    return barsResult(symbol, tf, await series.read({ symbol, tf, from, to }), missing);
  },

  // TradingView live (kept in the store), extended back with stored Dukascopy history.
  candles: async (p) => {
    await ready;
    const symbol = str(p, 'symbol');
    const tf = timeframe(p.timeframe);
    const count = int(p, 'bars', 300, 1, 20_000);
    const from = time(p, 'from');
    const now = nowSec();
    const to = Math.min(time(p, 'to', true) ?? now, now);
    const step = TIMEFRAMES[tf].seconds;
    const route = series.route(symbol);
    const wanted = from === undefined ? count : Math.min(20_000, Math.max(1, Math.ceil((to - from) / step)));
    try {
      await tv.candles(symbol, tf, wanted, to < now ? to : undefined);
    } catch (error) {
      // History from the store still answers for a market Dukascopy has.
      if (!route.instrument || (error instanceof RpcError && error.code === 'NOT_LOGGED_IN')) throw error;
      log('warn', `TradingView had no ${symbol} ${tf}: ${(error as Error).message}`);
    }
    const csvPath = str(p, 'csvPath', false);
    let start = from;
    let read: ReadResult | null = null;
    // A Dukascopy bucket stored while still open stays partial until its re-fetch: do the due ones
    // the read shows first.
    if (start === undefined) {
      const newest = () => series.read({ symbol, tf, count, before: to + 1, latest: true });
      read = await newest();
      if (read.bars.length && (await reads.refreshDue(symbol, tf, read.bars[0]!.t, to))) read = await newest();
      start = read.bars[0]?.t ?? to;
    } else {
      await reads.refreshDue(symbol, tf, start, to);
    }
    const missing = route.instrument
      ? from === undefined
        ? (read?.missing ?? [])
        : series.missing(symbol, tf, from, to)
      : [];
    if (csvPath) {
      const out = await series.exportCsv({ symbol, tf, from: start, to, path: csvPath });
      return csvResult(symbol, tf, out, missing);
    }
    read ??= await series.read({ symbol, tf, from: start, to });
    return barsResult(symbol, tf, read, missing);
  },

  'stream.open': async (p) => {
    await ready;
    const symbol = str(p, 'symbol');
    const opened = await tv.openStream(symbol, timeframe(p.timeframe), int(p, 'bars', 500, 10, 5000));
    const instrument = dukascopy.historyInstrument(symbol) ?? dukascopy.historyInstrument(opened.info.symbol);
    // The same key string store.updated and jobs use for this symbol.
    const tradingview = tvStore.canonical(opened.info.symbol);
    return { ...opened, keys: { ...(instrument ? { dukascopy: instrument } : {}), tradingview } };
  },

  'stream.close': (p) => {
    tv.closeStream(str(p, 'id'));
    return {};
  },

  /** More history on a live chart's session, so its indicators reach further back. */
  'stream.extend': (p) => ({ asked: tv.extendStream(str(p, 'id'), int(p, 'bars', 500, 1, 5000)) }),

  'indicators.catalog': (p) => {
    if (p.refresh === true) catalog.refresh();
    return catalog.catalog();
  },

  'indicators.search': (p) => catalog.search(str(p, 'query')),

  'indicators.layouts': () => catalog.layouts(),

  'indicators.layout': (p) => catalog.layout(str(p, 'id')),

  'indicator.add': (p) =>
    studies.add({
      stream: str(p, 'stream'),
      script: str(p, 'script'),
      version: str(p, 'version', false) || null,
      state: layoutState(p.state),
    }),

  'indicator.remove': (p) => {
    studies.remove(str(p, 'id'));
    return {};
  },

  /** The Pine library, most recently changed first (no sources). */
  'pine.list': () => pineLibrary.list(),

  'pine.get': async (p) => {
    const s = await pineLibrary.get(str(p, 'id'));
    return { ...pine.summary(s), source: s.source };
  },

  /** Creates a script (no `id`) or replaces its source; answers like `pine.get`. */
  'pine.save': async (p) => {
    const s = await pineLibrary.save({
      id: str(p, 'id', false) || undefined,
      source: source(p),
      name: str(p, 'name', false) || undefined,
    });
    return { ...pine.summary(s), source: s.source };
  },

  'pine.delete': async (p) => {
    await pineLibrary.remove(str(p, 'id'));
    return {};
  },

  /** Compiles without saving anything: errors, warnings, what it declares. */
  'pine.check': async (p) => {
    const c = await pine.compile(source(p));
    const meta = c.script ? describeBrief(c.script.metaInfo) : undefined;
    return { ok: c.ok, errors: c.errors, warnings: c.warnings, kind: c.kind, title: c.title, overlay: c.overlay, ...(meta ? { meta } : {}) };
  },

  /** Runs a script once on a chart of its own: its values, or where it failed. */
  'pine.test': (p) =>
    studies.trial({
      ...(typeof p.source === 'string' ? { source: p.source } : { script: str(p, 'script') }),
      symbol: str(p, 'symbol'),
      tf: timeframe(p.timeframe),
      bars: int(p, 'bars', 500, 10, 5000),
      state: layoutState(p.state),
    }),

  /** Saves a library script to the user's TradingView account (the app asked them first). */
  'pine.publish': (p) => pine.publish(pineLibrary, str(p, 'id'), { name: str(p, 'name', false) || undefined }),

  /** Copies a TradingView script's source into the library (own scripts stay linked). */
  'pine.import': (p) => pine.importScript(pineLibrary, str(p, 'script')),

  shutdown: async () => {
    await shutdown();
    exitAfterReply = true;
    return {};
  },
};

async function handle(line: string): Promise<void> {
  let message: { id?: number; method?: string; params?: Params };
  try {
    message = JSON.parse(line);
  } catch {
    log('warn', `ignored a line that is not JSON: ${line.slice(0, 120)}`);
    return;
  }
  const { id, method, params } = message;
  if (typeof id !== 'number' || typeof method !== 'string') return;
  const handler = handlers[method];
  if (!handler) {
    fail(id, new RpcError('UNKNOWN_METHOD', `Unknown method ${method}.`));
    return;
  }
  try {
    reply(id, await handler(params ?? {}));
  } catch (error) {
    fail(id, error);
  }
  if (exitAfterReply) process.stdout.write('', () => process.exit(0));
}

tv.restoreFromEnvironment();

process.on('uncaughtException', (error) => log('error', `uncaught: ${error.stack ?? error.message}`));
process.on('unhandledRejection', (reason) => log('error', `unhandled: ${String(reason)}`));

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on('line', (line) => {
  if (line.trim()) void handle(line);
});
rl.on('close', () => {
  void shutdown().finally(() => process.exit(0));
});
emit('ready', { version: VERSION });
