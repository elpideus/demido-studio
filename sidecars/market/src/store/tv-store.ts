// TradingView candles on disk, and which time they cover. TradingView has no public history feed:
// everything it ever sent (live streams, history pages, downloads) is kept here, so no chart opens
// or scrolls back into data that was already fetched once.
//
//   <cache>/tradingview/<SYMBOL>_<tf>.json   one JSON array of bars per canonical symbol + timeframe
//                                            (tvcache's format; the symbol upper-cased, every character
//                                            other than A-Z 0-9 as `_`; the monthly file is `_1mo`)
//   <cache>/tradingview/index.json           {version: 1, series: {"<file key>": {symbol, tf, intervals,
//                                            reachedStart, checkedAt}}, aliases: {"<asked>": "<canonical>"},
//                                            infos: {"<symbol>": ChartInfo}, stats: {secondsPerPage, pages}}
//
// Coverage is intervals [from, to) of contiguous fetches. A live stream's coverage ends at its last
// closed bar: the forming bar is stored but not covered, so once no stream includes it any more,
// Dukascopy's final data wins for that period. `reachedStart` is set only after TradingView answered
// two consecutive history pages with nothing.
//
// Writes: bar files first, the index after them in the same flush, and the index only ever records
// the coverage of bars that reached the disk. Debounced to one flush per 5 s; flush() on stream close
// and shutdown. Live updates are O(1) (replace the last bar or append); history pages splice by
// binary search.
//
// API (class TvStore, one per process):
//   canonical(symbol), setAlias(asked, canonical), info(symbol), setInfo(symbol, info)
//   bars(symbol, tf)             ascending; do not mutate
//   coverage(symbol, tf)         {intervals (copy), reachedStart, checkedAt}
//   record(symbol, tf, bars, cover?)   splices a contiguous page and adds the covered [from, to)
//   live(symbol, tf, bar)        a stream's newest bar; cover(symbol, tf, from, to) extends coverage
//   setReachedStart(symbol, tf, t | null), clearReachedStart(symbol)
//   notePage(seconds)            rolling seconds per history page (download estimates); secondsPerPage
//   series()                     every stored series with its coverage and size
//   remove(symbol) -> bytes freed, flush(), subscribe(listener)
// Also exported: fileKey, toleratedGap (the TradingView gap tolerance), isClosure, barEnd, coverOf,
// intervalsOf (coverage rebuilt from bars).

import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';

import { type Bar } from '../protocol.ts';
import { TIMEFRAMES, type Timeframe } from '../timeframes.ts';
import { writeAtomic } from './dukascopy-store.ts';
import { IntervalSet, type Range } from './intervals.ts';

export const INDEX_VERSION = 1;
const DAY = 86400;
const HOUR = 3600;
const DEFAULT_SECONDS_PER_PAGE = 3;

export interface TvCoverage {
  intervals: IntervalSet;
  reachedStart: number | null;
  checkedAt: number | null;
}

export interface TvSeriesSummary {
  symbol: string;
  tf: Timeframe;
  count: number;
  first: number | null;
  last: number | null;
  bytes: number;
  intervals: Range[];
  reachedStart: number | null;
  checkedAt: number | null;
}

export interface TvChange {
  symbol: string;
  tf: Timeframe;
  from: number;
  to: number;
}

export type TvListener = (change: TvChange) => void;

export interface TvStoreOptions {
  /** Clock in ms (tests). */
  now?: () => number;
  log?: (level: 'info' | 'warn' | 'error', message: string) => void;
  flushDelayMs?: number;
}

interface Snapshot {
  intervals: Range[];
  reachedStart: number | null;
  checkedAt: number | null;
}

interface Series {
  key: string;
  symbol: string;
  tf: Timeframe;
  /** Null until first read from disk. */
  bars: Bar[] | null;
  intervals: IntervalSet;
  reachedStart: number | null;
  checkedAt: number | null;
  dirtyBars: boolean;
  /** What the index on disk says (only ever coverage of saved bars). */
  persisted: Snapshot;
}

const upper = (symbol: string) => symbol.trim().toUpperCase();

/** File name stem for a symbol and timeframe. `1M` would collide with `1m` on case-insensitive disks. */
export function fileKey(symbol: string, tf: Timeframe): string {
  return `${upper(symbol).replace(/[^A-Z0-9]/g, '_')}_${tf === '1M' ? '1mo' : tf}`;
}

/** 0 = Sunday ... 6 = Saturday, for a day number (days since 1970-01-01, a Thursday). */
const weekday = (day: number) => (((day + 4) % 7) + 7) % 7;

/** Market closures overlapping [from, to): weekend windows (unless the market trades at weekends)
 *  and the Dec 25 / Jan 1 holidays. The weekend window is Fri 21:00 -> Sun 21:00 UTC, widened by an
 *  hour each way: sessions move with daylight saving and some feeds close or reopen an hour off. */
function closures(from: number, to: number, weekends: boolean): IntervalSet {
  const set = new IntervalSet();
  const firstDay = Math.floor(from / DAY) - 3;
  const lastDay = Math.floor(to / DAY) + 1;
  for (let d = firstDay; d <= lastDay; d += 1) {
    const date = new Date(d * DAY * 1000);
    const m = date.getUTCMonth();
    const dom = date.getUTCDate();
    if ((m === 11 && dom === 25) || (m === 0 && dom === 1)) set.add(d * DAY - 3 * HOUR, (d + 1) * DAY);
    if (weekends && weekday(d) === 5) set.add(d * DAY + 20 * HOUR, (d + 2) * DAY + 22 * HOUR);
  }
  return set;
}

/** Whether all of [from, to) lies inside market closures. */
export function isClosure(from: number, to: number, weekends = true): boolean {
  if (!(to > from)) return true;
  // No run of closures lasts this long (a weekend next to a holiday is about four days).
  if (to - from > 5 * DAY) return false;
  return closures(from, to, weekends).covers(from, to);
}

/** A stretch next to TradingView coverage that is not a real gap: shorter than 3 bars once market
 *  closures are left out (so a closure alone, or a short gap alone, is tolerated). */
export function toleratedGap(from: number, to: number, tf: Timeframe, weekends = true): boolean {
  const limit = 3 * TIMEFRAMES[tf].seconds;
  if (to - from < limit) return true;
  if (to - from > 5 * DAY + limit) return false;
  let open = 0;
  for (const [f, t] of closures(from, to, weekends).gaps(from, to)) open += t - f;
  return open < limit;
}

/** When a TradingView bar closes: the next calendar month for monthly bars, else its length later. */
export function barEnd(t: number, tf: Timeframe): number {
  if (tf !== '1M') return t + TIMEFRAMES[tf].seconds;
  // A few days in, so an exchange time zone west of UTC still lands in the bar's own month.
  const d = new Date((t + 3 * DAY) * 1000);
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1) / 1000;
}

/** What a snapshot of bars covers: from its first bar to the end of its last closed one (the bar
 *  still forming at `now` is stored but not covered). */
export function coverOf(bars: readonly Bar[], tf: Timeframe, now: number): [number, number] | null {
  const first = bars[0];
  const last = bars[bars.length - 1];
  if (!first || !last) return null;
  const end = barEnd(last.t, tf) > now ? last.t : barEnd(last.t, tf);
  return end > first.t ? [first.t, end] : null;
}

/** Coverage rebuilt from stored bars alone: split where bars are further apart than the tolerance. */
export function intervalsOf(bars: readonly Bar[], tf: Timeframe, weekends = true): IntervalSet {
  const set = new IntervalSet();
  let runStart: number | null = null;
  let prevEnd = 0;
  for (const bar of bars) {
    if (runStart === null) runStart = bar.t;
    else if (bar.t > prevEnd && !toleratedGap(prevEnd, bar.t, tf, weekends)) {
      set.add(runStart, prevEnd);
      runStart = bar.t;
    }
    prevEnd = Math.max(prevEnd, barEnd(bar.t, tf));
  }
  if (runStart !== null) set.add(runStart, prevEnd);
  return set;
}

function sanitize(bars: Bar[]): Bar[] {
  // Some brokers report no volume at all; the chart's histogram throws on anything but a number.
  return bars.map((b) => (Number.isFinite(b.v) ? b : { ...b, v: 0 }));
}

/** First index whose bar is at or after t. */
function lowerBound(bars: readonly Bar[], t: number): number {
  let lo = 0;
  let hi = bars.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (bars[mid]!.t < t) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

const snapshotOf = (s: Series): Snapshot => ({
  intervals: s.intervals.toJSON(),
  reachedStart: s.reachedStart,
  checkedAt: s.checkedAt,
});

export class TvStore {
  readonly root: string;
  readonly #now: () => number;
  readonly #log: (level: 'info' | 'warn' | 'error', message: string) => void;
  readonly #flushDelayMs: number;

  readonly #series = new Map<string, Series>();
  readonly #aliases = new Map<string, string>();
  readonly #infos = new Map<string, Record<string, unknown>>();
  readonly #listeners = new Set<TvListener>();
  #stats = { secondsPerPage: DEFAULT_SECONDS_PER_PAGE, pages: 0 };
  #dirtyIndex = false;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #writing: Promise<void> | null = null;

  constructor(cacheDir: string, opts: TvStoreOptions = {}) {
    this.root = path.join(cacheDir, 'tradingview');
    this.#now = opts.now ?? Date.now;
    this.#log = opts.log ?? (() => {});
    this.#flushDelayMs = opts.flushDelayMs ?? 5000;
    this.#loadIndex();
  }

  #nowSec(): number {
    return Math.floor(this.#now() / 1000);
  }

  #loadIndex(): void {
    let json: Record<string, unknown>;
    try {
      json = JSON.parse(fs.readFileSync(path.join(this.root, 'index.json'), 'utf8')) as Record<string, unknown>;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') {
        this.#log('warn', `TradingView cache index is unreadable (${(error as Error).message}); starting a new one`);
      }
      return;
    }
    if (!json || json.version !== INDEX_VERSION) return;
    const series = (json.series ?? {}) as Record<string, Record<string, unknown>>;
    for (const [key, entry] of Object.entries(series)) {
      try {
        const tf = entry.tf as Timeframe;
        if (typeof entry.symbol !== 'string' || !(tf in TIMEFRAMES)) continue;
        const intervals = IntervalSet.fromJSON(entry.intervals ?? []);
        const reachedStart = typeof entry.reachedStart === 'number' ? entry.reachedStart : null;
        const checkedAt = typeof entry.checkedAt === 'number' ? entry.checkedAt : null;
        this.#series.set(key, {
          key,
          symbol: entry.symbol,
          tf,
          bars: null,
          intervals,
          reachedStart,
          checkedAt,
          dirtyBars: false,
          persisted: { intervals: intervals.toJSON(), reachedStart, checkedAt },
        });
      } catch {
        this.#log('warn', `TradingView cache index: dropped the unreadable entry ${key}`);
      }
    }
    for (const [asked, canonical] of Object.entries((json.aliases ?? {}) as Record<string, unknown>)) {
      if (typeof canonical === 'string') this.#aliases.set(asked, canonical);
    }
    for (const [symbol, info] of Object.entries((json.infos ?? {}) as Record<string, unknown>)) {
      if (info && typeof info === 'object') this.#infos.set(symbol, info as Record<string, unknown>);
    }
    const stats = (json.stats ?? {}) as Record<string, unknown>;
    if (typeof stats.secondsPerPage === 'number' && stats.secondsPerPage > 0 && stats.secondsPerPage < 600) {
      this.#stats.secondsPerPage = stats.secondsPerPage;
    }
    if (typeof stats.pages === 'number' && stats.pages >= 0) this.#stats.pages = Math.floor(stats.pages);
  }

  // -------------------------------------------------------------------------------------------
  // Symbols

  /** The symbol TradingView calls this one (`EURUSD` -> `FX:EURUSD` once a chart resolved it). */
  canonical(symbol: string): string {
    const key = upper(symbol);
    return this.#aliases.get(key) ?? key;
  }

  setAlias(asked: string, canonical: string): void {
    const from = upper(asked);
    const to = upper(canonical);
    if (!from || !to || from === to || this.#aliases.get(from) === to) return;
    this.#aliases.set(from, to);
    this.#markIndex();
  }

  info(symbol: string): Record<string, unknown> | null {
    return this.#infos.get(this.canonical(symbol)) ?? null;
  }

  setInfo(symbol: string, info: object): void {
    const key = upper(symbol);
    const json = JSON.stringify(info);
    if (JSON.stringify(this.#infos.get(key) ?? null) === json) return;
    this.#infos.set(key, JSON.parse(json) as Record<string, unknown>);
    this.#markIndex();
  }

  // -------------------------------------------------------------------------------------------
  // Reads

  #get(symbol: string, tf: Timeframe, create: true): Series;
  #get(symbol: string, tf: Timeframe, create: false): Series | null;
  #get(symbol: string, tf: Timeframe, create: boolean): Series | null {
    const key = fileKey(symbol, tf);
    let s = this.#series.get(key);
    if (!s && create) {
      s = {
        key,
        symbol: upper(symbol),
        tf,
        bars: null,
        intervals: new IntervalSet(),
        reachedStart: null,
        checkedAt: null,
        dirtyBars: false,
        persisted: { intervals: [], reachedStart: null, checkedAt: null },
      };
      this.#series.set(key, s);
    }
    return s ?? null;
  }

  #file(key: string): string {
    return path.join(this.root, `${key}.json`);
  }

  #bars(s: Series): Bar[] {
    if (s.bars) return s.bars;
    let bars: Bar[] = [];
    try {
      const json = JSON.parse(fs.readFileSync(this.#file(s.key), 'utf8')) as unknown;
      if (Array.isArray(json)) {
        bars = sanitize(
          json.filter(
            (b): b is Bar =>
              !!b && typeof b === 'object' && Number.isFinite((b as Bar).t) && Number.isFinite((b as Bar).c),
          ),
        ).sort((a, b) => a.t - b.t);
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') {
        this.#log('warn', `TradingView cache ${s.key} is unreadable: ${(error as Error).message}`);
      }
    }
    s.bars = bars;
    return bars;
  }

  bars(symbol: string, tf: Timeframe): readonly Bar[] {
    const s = this.#get(this.canonical(symbol), tf, false);
    return s ? this.#bars(s) : [];
  }

  coverage(symbol: string, tf: Timeframe): TvCoverage {
    const s = this.#get(this.canonical(symbol), tf, false);
    if (!s) return { intervals: new IntervalSet(), reachedStart: null, checkedAt: null };
    return { intervals: s.intervals.clone(), reachedStart: s.reachedStart, checkedAt: s.checkedAt };
  }

  series(): TvSeriesSummary[] {
    const out: TvSeriesSummary[] = [];
    // Series with bar files the index does not know yet (written before it existed) count too.
    let names: string[] = [];
    try {
      names = fs.readdirSync(this.root);
    } catch {
      // Nothing stored yet.
    }
    for (const s of this.#series.values()) {
      const bars = this.#bars(s);
      if (bars.length === 0 && s.intervals.isEmpty) continue;
      let bytes = 0;
      if (names.includes(`${s.key}.json`)) {
        try {
          bytes = fs.statSync(this.#file(s.key)).size;
        } catch {
          // Counted next time.
        }
      }
      out.push({
        symbol: s.symbol,
        tf: s.tf,
        count: bars.length,
        first: bars[0]?.t ?? null,
        last: bars[bars.length - 1]?.t ?? null,
        bytes,
        intervals: s.intervals.toJSON(),
        reachedStart: s.reachedStart,
        checkedAt: s.checkedAt,
      });
    }
    return out.sort((a, b) => a.symbol.localeCompare(b.symbol) || a.tf.localeCompare(b.tf));
  }

  /** Stored series without an index entry, as file keys (the migration rebuilds their coverage). */
  unindexedFiles(): string[] {
    let names: string[] = [];
    try {
      names = fs.readdirSync(this.root);
    } catch {
      return [];
    }
    return names
      .filter((n) => n.endsWith('.json') && n !== 'index.json' && !n.endsWith('.tmp'))
      .map((n) => n.slice(0, -'.json'.length))
      .filter((key) => !this.#series.has(key));
  }

  /** Reads a bar file by its key, for the migration. */
  readFile(key: string): Bar[] {
    try {
      const json = JSON.parse(fs.readFileSync(this.#file(key), 'utf8')) as unknown;
      return Array.isArray(json) ? sanitize(json as Bar[]).sort((a, b) => a.t - b.t) : [];
    } catch {
      return [];
    }
  }

  // -------------------------------------------------------------------------------------------
  // Writes

  /** Stores a contiguous page of bars (ascending) and records `cover` = [from, to) as fetched. */
  record(symbol: string, tf: Timeframe, page: readonly Bar[], cover?: readonly [number, number] | null): void {
    const s = this.#get(symbol, tf, true);
    const bars = this.#bars(s);
    const clean = sanitize([...page]).sort((a, b) => a.t - b.t);
    if (clean.length) {
      const lo = lowerBound(bars, clean[0]!.t);
      const hi = lowerBound(bars, clean[clean.length - 1]!.t + 1);
      // The page is what TradingView says about its span: it replaces whatever was stored there.
      if (clean.length + bars.length < 50_000) bars.splice(lo, hi - lo, ...clean);
      else s.bars = bars.slice(0, lo).concat(clean, bars.slice(hi));
      s.dirtyBars = true;
    }
    if (cover && cover[1] > cover[0]) s.intervals.add(cover[0], cover[1]);
    if (!clean.length && !(cover && cover[1] > cover[0])) return;
    s.checkedAt = this.#nowSec();
    this.#markIndex();
    const step = TIMEFRAMES[tf].seconds;
    const from = Math.min(clean[0]?.t ?? Infinity, cover?.[0] ?? Infinity);
    const to = Math.max((clean[clean.length - 1]?.t ?? -Infinity) + step, cover?.[1] ?? -Infinity);
    this.#emit({ symbol: s.symbol, tf, from, to });
  }

  /** A live stream's newest bar: replaces the last bar, or appends a new one. */
  live(symbol: string, tf: Timeframe, bar: Bar): void {
    const s = this.#get(symbol, tf, true);
    const bars = this.#bars(s);
    const [clean] = sanitize([bar]);
    const last = bars[bars.length - 1];
    if (!last || clean!.t > last.t) bars.push(clean!);
    else if (clean!.t === last.t) bars[bars.length - 1] = clean!;
    else {
      // A late update of an older bar (the closed bar's last ticks after a rollover).
      const i = lowerBound(bars, clean!.t);
      if (bars[i]?.t === clean!.t) bars[i] = clean!;
      else bars.splice(i, 0, clean!);
    }
    s.dirtyBars = true;
    this.#markIndex();
    this.#emit({ symbol: s.symbol, tf, from: clean!.t, to: clean!.t + TIMEFRAMES[tf].seconds });
  }

  /** Records [from, to) as covered (a live stream's closed bars). */
  cover(symbol: string, tf: Timeframe, from: number, to: number): void {
    if (!(to > from)) return;
    const s = this.#get(symbol, tf, true);
    if (s.intervals.covers(from, to)) return;
    s.intervals.add(from, to);
    s.checkedAt = this.#nowSec();
    this.#markIndex();
    this.#emit({ symbol: s.symbol, tf, from, to });
  }

  /** Replaces the coverage of a series outright (the migration's rebuild). */
  setIntervals(symbol: string, tf: Timeframe, intervals: IntervalSet): void {
    const s = this.#get(symbol, tf, true);
    s.intervals = intervals.clone();
    s.checkedAt = this.#nowSec();
    // The bars are on disk already: the index may claim them at once.
    if (!s.dirtyBars) s.persisted = snapshotOf(s);
    this.#markIndex();
  }

  setReachedStart(symbol: string, tf: Timeframe, t: number | null): void {
    const s = this.#get(symbol, tf, true);
    if (s.reachedStart === t) return;
    s.reachedStart = t;
    this.#markIndex();
    const first = s.intervals.first() ?? t ?? 0;
    this.#emit({ symbol: s.symbol, tf, from: Math.min(first, t ?? first), to: s.intervals.last() ?? first });
  }

  /** "Check for older data": TradingView may have added history since. */
  clearReachedStart(symbol: string): void {
    const key = this.canonical(symbol);
    for (const s of this.#series.values()) if (s.symbol === key) this.setReachedStart(s.symbol, s.tf, null);
  }

  get secondsPerPage(): number {
    return this.#stats.secondsPerPage;
  }

  notePage(seconds: number): void {
    if (!(seconds >= 0) || !Number.isFinite(seconds)) return;
    this.#stats.pages += 1;
    const alpha = Math.max(1 / this.#stats.pages, 0.1);
    this.#stats.secondsPerPage += alpha * (seconds - this.#stats.secondsPerPage);
    this.#markIndex();
  }

  /** Deletes every timeframe stored for a symbol. Returns the bytes freed. */
  async remove(symbol: string): Promise<number> {
    const key = this.canonical(symbol);
    await this.#writing?.catch(() => {});
    let bytes = 0;
    for (const s of [...this.#series.values()]) {
      if (s.symbol !== key) continue;
      const file = this.#file(s.key);
      bytes += await fsp.stat(file).then(
        (st) => st.size,
        () => 0,
      );
      await fsp.rm(file, { force: true, maxRetries: 10, retryDelay: 100 });
      this.#series.delete(s.key);
      this.#emit({ symbol: s.symbol, tf: s.tf, from: 0, to: this.#nowSec() + DAY });
    }
    for (const [asked, canonical] of [...this.#aliases]) if (canonical === key) this.#aliases.delete(asked);
    this.#infos.delete(key);
    this.#markIndex();
    await this.flush();
    return bytes;
  }

  // -------------------------------------------------------------------------------------------
  // Flushing

  #markIndex(): void {
    this.#dirtyIndex = true;
    if (this.#timer || this.#writing) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.flush().catch(() => {});
    }, this.#flushDelayMs);
    this.#timer.unref?.();
  }

  /** Writes every changed bar file, then the index. */
  async flush(): Promise<void> {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = null;
    while (this.#writing) await this.#writing.catch(() => {});
    if (!this.#dirtyIndex) return;
    this.#dirtyIndex = false;
    // Snapshot everything synchronously: the index then describes exactly the bars being written.
    const writes: Array<{ s: Series; json: string; snap: Snapshot }> = [];
    for (const s of this.#series.values()) {
      if (s.dirtyBars && s.bars) {
        writes.push({ s, json: JSON.stringify(s.bars), snap: snapshotOf(s) });
        s.dirtyBars = false;
      } else if (!s.dirtyBars) {
        s.persisted = snapshotOf(s);
      }
    }
    const task = (async () => {
      for (const w of writes) {
        try {
          await writeAtomic(this.#file(w.s.key), w.json);
          w.s.persisted = w.snap;
        } catch (error) {
          w.s.dirtyBars = true;
          this.#dirtyIndex = true;
          this.#log('error', `Could not save TradingView bars ${w.s.key}: ${(error as Error).message}`);
        }
      }
      const series: Record<string, unknown> = {};
      for (const s of this.#series.values()) {
        if (s.persisted.intervals.length === 0 && s.persisted.reachedStart === null && !s.bars?.length) continue;
        series[s.key] = { symbol: s.symbol, tf: s.tf, ...s.persisted };
      }
      const json = JSON.stringify({
        version: INDEX_VERSION,
        series,
        aliases: Object.fromEntries(this.#aliases),
        infos: Object.fromEntries(this.#infos),
        stats: this.#stats,
      });
      try {
        await writeAtomic(path.join(this.root, 'index.json'), json, { fsync: true });
      } catch (error) {
        this.#dirtyIndex = true;
        this.#log('error', `Could not save the TradingView cache index: ${(error as Error).message}`);
      }
    })();
    this.#writing = task;
    try {
      await task;
    } finally {
      this.#writing = null;
      if (this.#dirtyIndex) this.#markIndex();
    }
  }

  subscribe(listener: TvListener): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #emit(change: TvChange): void {
    for (const listener of this.#listeners) {
      try {
        listener(change);
      } catch (error) {
        this.#log('error', `TradingView store listener failed: ${(error as Error).message}`);
      }
    }
  }
}
