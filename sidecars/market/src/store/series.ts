// The one read path for candles, from the store only (it never touches the network). Charts, tools
// and the Data tab all read through here, so every caller sees the same bars and the same gaps.
//
// Dukascopy history is 1-minute candles (the store's m1 tier): every timeframe is built from them, so
// one download serves them all and every timeframe shows the same coverage. Coverage at time t: m1
// has data at t, or has fetched t (empty included: weekends); unavailable (400/404) buckets count as
// fetched with nothing in them. History is available from m1's effective start. Dukascopy gaps are
// exact. Reading aggregates the stretch's 1-minute bars once; a T bucket not fully covered is dropped
// (its time is a gap), except the bucket containing now. (The store can hold coarser tiers; nothing
// reads them.)
//
// TradingView (store/tv-store) holds bars per timeframe with interval coverage. TradingView wins
// wherever its coverage for T includes a time; Dukascopy fills the rest. A stretch between two
// TradingView intervals, or at a TradingView/Dukascopy seam, is not a gap when it is shorter than
// 3 bars or lies inside market closures (no weekend closures for 24/7 markets).
//
// API (class Series):
//   route(symbol) -> {source, key, instrument, tvKey, name};  keys(symbol)
//   coverage(symbol, tf) -> {covered, start}   covered time with the tolerance, available start
//   missing(symbol, tf, from, to)              uncovered available ranges inside [from, to]
//   due(symbol, tf, from, to) -> [{tier, start}]  provisional buckets due for their re-fetch that a
//       read of [from, to] shows (store/reads refreshes them before a tool reads)
//   read({symbol, tf, before?, count?, from?, to?, limit?}) -> {bars, spans, more, gap?, missing, truncated, keys}
//       paging (before/count: the newest `count` bars before `before`) or a range (from/to, the
//       newest `limit` bars, default 20 000, truncated when capped)
//   exportCsv({symbol, tf, from, to, path, limit?}) -> summary; streams up to 2 000 000 rows of
//       time,open,high,low,close,volume,source without holding them
//   eligibleTiers(tf) (the tiers a timeframe is built from: m1, whatever the timeframe)

import fs from 'node:fs';

import { historyInstrument, instrumentInfo } from '../dukascopy.ts';
import { type Bar } from '../protocol.ts';
import { TIMEFRAMES, type Timeframe } from '../timeframes.ts';
import { type AggregateOptions, aggregate, bucketOf, bucketSpan, dropPartial, weekModeFor } from './aggregate.ts';
import { TIER_BAR_SECONDS, type Tier, isActive, nextBucket } from './buckets.ts';
import { type DukascopyStore } from './dukascopy-store.ts';
import { IntervalSet, type Range } from './intervals.ts';
import { type TvStore, toleratedGap } from './tv-store.ts';

export type Source = 'dukascopy' | 'tradingview';
export type More = 'cached' | 'gap' | 'none';

export interface Keys {
  dukascopy?: string;
  tradingview?: string;
}

export interface Span {
  source: Source;
  from: number;
  to: number;
  count: number;
}

export interface Gap {
  from: number;
  to: number;
  source: Source;
}

export interface Route {
  source: Source;
  /** The Dukascopy instrument, else the TradingView symbol. */
  key: string;
  instrument: string | null;
  tvKey: string;
  name: string;
}

export interface ReadParams {
  symbol: string;
  tf: Timeframe;
  before?: number;
  count?: number;
  from?: number;
  /** Inclusive. */
  to?: number;
  limit?: number;
  /** Paging: serve the newest stored bars even when uncovered or bar-less time separates them from `before`. */
  latest?: boolean;
}

export interface ReadResult {
  bars: Bar[];
  spans: Span[];
  /** Per bar, parallel to `bars`. */
  sources: Source[];
  more: More;
  gap?: Gap;
  missing: Range[];
  truncated: boolean;
  keys: Keys;
}

export interface ExportSummary {
  count: number;
  from: number | null;
  to: number | null;
  firstClose: number | null;
  lastClose: number | null;
  high: number | null;
  low: number | null;
  spans: Span[];
  lastRows: Array<[number, number, number, number, number, number, Source]>;
  closes: number[];
  missing: Range[];
  truncated: boolean;
  keys: Keys;
}

export const DEFAULT_LIMIT = 20_000;
export const EXPORT_LIMIT = 2_000_000;
const FAR = 1e11;

interface Item {
  bar: Bar;
  source: Source;
}

interface Context {
  tf: Timeframe;
  step: number;
  opts: AggregateOptions;
  now: number;
  route: Route;
  /** Eligible tiers with their data, coarsest first. */
  tiers: Array<{ tier: Tier; data: IntervalSet }>;
  dukaCovered: IntervalSet;
  tvBars: readonly Bar[];
  tvCovered: IntervalSet;
  /** Everything covered, with the TradingView tolerance. */
  covered: IntervalSet;
  /** Where bars can exist: Dukascopy data and TradingView coverage. */
  data: IntervalSet;
  /** The earliest time anything is available (null: unknown, TradingView without a known start). */
  start: number | null;
}

/** The tiers timeframe T is built from: 1-minute candles, whatever T is. */
export function eligibleTiers(_tf: Timeframe): Tier[] {
  return ['m1'];
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

export function spansOf(items: readonly Item[]): Span[] {
  const out: Span[] = [];
  for (const { bar, source } of items) {
    const last = out[out.length - 1];
    if (last && last.source === source) {
      last.to = bar.t;
      last.count += 1;
    } else {
      out.push({ source, from: bar.t, to: bar.t, count: 1 });
    }
  }
  return out;
}

const iso = (t: number) => new Date(t * 1000).toISOString().replace('.000Z', 'Z');

export class Series {
  readonly #store: DukascopyStore;
  readonly #tv: TvStore;
  readonly #now: () => number;

  constructor(deps: { store: DukascopyStore; tv: TvStore; now?: () => number }) {
    this.#store = deps.store;
    this.#tv = deps.tv;
    this.#now = deps.now ?? Date.now;
  }

  route(symbol: string): Route {
    const instrument = historyInstrument(symbol);
    const tvKey = this.#tv.canonical(symbol);
    if (instrument) {
      return { source: 'dukascopy', key: instrument, instrument, tvKey, name: instrumentInfo(instrument).name };
    }
    return { source: 'tradingview', key: tvKey, instrument: null, tvKey, name: tvKey };
  }

  keys(symbol: string): Keys {
    const r = this.route(symbol);
    return r.instrument ? { dukascopy: r.instrument, tradingview: r.tvKey } : { tradingview: r.tvKey };
  }

  #context(symbol: string, tf: Timeframe): Context {
    const route = this.route(symbol);
    const now = Math.floor(this.#now() / 1000);
    const step = TIMEFRAMES[tf].seconds;
    const tvBars = this.#tv.bars(route.tvKey, tf);
    const tvCoverage = this.#tv.coverage(route.tvKey, tf);
    const tvCovered = tvCoverage.intervals;
    let week: AggregateOptions['week'];
    if (route.instrument) week = weekModeFor(route.instrument);
    else {
      const type = this.#tv.info(route.tvKey)?.type;
      const sample = step <= 86400 ? tvBars.slice(-2000) : null;
      week = type === 'crypto' ? 'utc' : weekModeFor(route.tvKey, sample);
    }
    const opts: AggregateOptions = { week };

    const tiers: Context['tiers'] = [];
    const dukaCovered = new IntervalSet();
    const dukaData = new IntervalSet();
    let dukaStart: number | null = null;
    if (route.instrument) {
      // Finest first: each tier is the finest available one from its start up to the next finer start.
      let upper = FAR;
      for (const tier of [...eligibleTiers(tf)].reverse()) {
        const sets = this.#store.sets(route.instrument, tier);
        const eff = this.#store.effectiveStart(route.instrument, tier);
        tiers.unshift({ tier, data: sets.data });
        dukaData.addSet(sets.data);
        if (eff === null) continue;
        if (eff < upper) dukaCovered.addSet(sets.covered.union(sets.unavailable).clip(eff, upper));
        upper = Math.min(upper, eff);
        dukaStart = dukaStart === null ? eff : Math.min(dukaStart, eff);
      }
      dukaCovered.addSet(dukaData);
    }

    const union = dukaCovered.union(tvCovered);
    const covered = union.clone();
    if (!tvCovered.isEmpty && !union.isEmpty) {
      for (const [a, b] of union.gaps(union.first()!, union.last()!)) {
        const nextToTv = tvCovered.contains(a - 1) || tvCovered.contains(b);
        if (nextToTv && toleratedGap(a, b, tf, week === 'session')) covered.add(a, b);
      }
    }

    let start: number | null;
    if (route.instrument) {
      const tvFirst = tvCovered.first();
      start = dukaStart === null ? tvFirst : tvFirst === null ? dukaStart : Math.min(dukaStart, tvFirst);
    } else {
      start = tvCoverage.reachedStart;
    }
    return {
      tf,
      step,
      opts,
      now,
      route,
      tiers,
      dukaCovered,
      tvBars,
      tvCovered,
      covered,
      data: dukaData.union(tvCovered),
      start,
    };
  }

  coverage(symbol: string, tf: Timeframe): { covered: IntervalSet; start: number | null } {
    const ctx = this.#context(symbol, tf);
    return { covered: ctx.covered, start: ctx.start };
  }

  #missing(ctx: Context, from: number, toInclusive: number): Range[] {
    const lo = Math.max(from, ctx.start ?? from);
    const hi = Math.min(toInclusive + 1, ctx.now);
    return hi > lo ? ctx.covered.gaps(lo, hi) : [];
  }

  missing(symbol: string, tf: Timeframe, from: number, to: number): Range[] {
    return this.#missing(this.#context(symbol, tf), from, to);
  }

  /** Provisional buckets due for their one re-fetch that a read of [from, to] (inclusive) would
   *  show, newest first: per stretch the tier the read uses (where TradingView does not win), and
   *  where no tier has bars, every eligible tier's (an empty copy built while open hides a day).
   *  An active bucket counts when it is stale and the read reaches past its last fetch (a 1h read of
   *  yesterday served by this month's h1 copy stored yesterday afternoon). */
  due(symbol: string, tf: Timeframe, from: number, to: number): Array<{ tier: Tier; start: number }> {
    const ctx = this.#context(symbol, tf);
    const instrument = ctx.route.instrument;
    const lo = this.#spanStart(ctx, from);
    const hi = this.#endBoundary(ctx, Math.min(to, FAR), true);
    if (!instrument || !(hi > lo)) return [];
    const out: Array<{ tier: Tier; start: number }> = [];
    const collect = (tier: Tier, where: IntervalSet) => {
      for (const start of this.#store.dueBuckets(instrument, tier, ctx.now)) {
        let from = start;
        if (isActive(tier, start, ctx.now)) {
          // Only the time after its last fetch is missing from it (less a bar, which may have been
          // forming then).
          const at = this.#store.refreshedAt(instrument, tier, start);
          if (at === null) continue;
          from = Math.max(start, at - TIER_BAR_SECONDS[tier]);
        }
        if (where.clip(from, nextBucket(tier, start)).isEmpty) continue;
        if (!out.some((d) => d.tier === tier && d.start === start)) out.push({ tier, start });
      }
    };
    let remaining = new IntervalSet([[lo, hi]]).minus(ctx.tvCovered);
    for (const { tier, data } of ctx.tiers) {
      const use = data.intersect(remaining);
      if (use.isEmpty) continue;
      collect(tier, use);
      remaining = remaining.minus(use);
    }
    if (!remaining.isEmpty) for (const { tier } of ctx.tiers) collect(tier, remaining);
    return out.sort((a, b) => b.start - a.start);
  }

  // -------------------------------------------------------------------------------------------
  // Building bars for a window whose ends are bucket boundaries

  #spanStart(ctx: Context, t: number): number {
    return bucketSpan(bucketOf(t, ctx.tf, ctx.opts), ctx.tf, ctx.opts)[0];
  }

  #spanEnd(ctx: Context, t: number): number {
    return bucketSpan(bucketOf(t, ctx.tf, ctx.opts), ctx.tf, ctx.opts)[1];
  }

  /** The boundary below every bucket labelled before `t` (paging) or at/before it (`inclusive`). */
  #endBoundary(ctx: Context, t: number, inclusive: boolean): number {
    const label = bucketOf(t, ctx.tf, ctx.opts);
    const [s, e] = bucketSpan(label, ctx.tf, ctx.opts);
    return label < t || (inclusive && label === t) ? e : s;
  }

  /** Bars whose bucket starts in [from, to) (TradingView bars: whose time is in [from, tvTo)). */
  async #build(ctx: Context, from: number, to: number, tvTo = to): Promise<Item[]> {
    const items: Item[] = [];
    const { route, tf, opts } = ctx;
    if (route.instrument && to > from) {
      // TradingView wins where it has coverage; read Dukascopy only elsewhere.
      let remaining = new IntervalSet([[from, to]]).minus(ctx.tvCovered);
      const native: Bar[] = [];
      for (const { tier, data } of ctx.tiers) {
        const use = data.intersect(remaining);
        if (use.isEmpty) continue;
        for (const [f, t] of use) {
          for (const b of await this.#store.readNative(route.instrument, tier, f, t)) native.push(b);
        }
        remaining = remaining.minus(use);
        if (remaining.isEmpty) break;
      }
      native.sort((a, b) => a.t - b.t);
      for (const bar of dropPartial(aggregate(native, tf, opts), tf, opts, ctx.dukaCovered, ctx.now)) {
        const [s, e] = bucketSpan(bar.t, tf, opts);
        if (s < from || s >= to) continue;
        if (!ctx.tvCovered.clip(s, e).isEmpty) continue;
        items.push({ bar, source: 'dukascopy' });
      }
    }
    if (ctx.tvBars.length && tvTo > from) {
      const lastCovered = ctx.tvCovered.last();
      const tvOnly = route.instrument === null;
      const tv: Item[] = [];
      for (let i = lowerBound(ctx.tvBars, from); i < ctx.tvBars.length; i += 1) {
        const bar = ctx.tvBars[i]!;
        if (bar.t >= tvTo) break;
        // A TradingView-only market also shows the bar that was still forming when last stored.
        if (ctx.tvCovered.contains(bar.t) || (tvOnly && bar.t === lastCovered)) tv.push({ bar, source: 'tradingview' });
      }
      if (items.length === 0) return tv;
      const merged: Item[] = [];
      let i = 0;
      for (const item of tv) {
        while (i < items.length && items[i]!.bar.t < item.bar.t) merged.push(items[i++]!);
        if (i < items.length && items[i]!.bar.t === item.bar.t) i += 1;
        merged.push(item);
      }
      while (i < items.length) merged.push(items[i++]!);
      return merged;
    }
    return items;
  }

  /** Whether more exists right before `edge`, and if it is stored. */
  #edge(ctx: Context, edge: number): { more: More; gap?: Gap } {
    let e = edge;
    for (let guard = 0; guard < 10_000; guard += 1) {
      if (ctx.start !== null && e <= ctx.start) return { more: 'none' };
      const run = ctx.covered.rangeAt(e - 1);
      if (!run) {
        const prevEnd = ctx.covered.clip(-FAR, e).last();
        const from = Math.max(prevEnd ?? ctx.start ?? 0, ctx.start ?? -FAR);
        // Nothing is known to exist before a TradingView-only market's oldest stored bar.
        if (ctx.start === null && ctx.data.isEmpty) return { more: 'none' };
        return { more: 'gap', gap: { from, to: e, source: ctx.route.source } };
      }
      if (!ctx.data.clip(run[0], e).isEmpty) return { more: 'cached' };
      e = run[0];
    }
    return { more: 'none' };
  }

  /** The newest `count` bars before `before`. `latest` first skips everything between `before` and
   *  the newest stored bars: uncovered stretches (what is stored is served even when it is behind)
   *  and covered ones without bars (a fetched weekend must not hide Friday). */
  async #page(
    ctx: Context,
    before: number,
    count: number,
    latest = false,
  ): Promise<{ items: Item[]; more: More; gap?: Gap }> {
    const chunks: Item[][] = [];
    let n = 0;
    let cursor = this.#endBoundary(ctx, before, false);
    let tvCursor = before;
    let span = Math.max(count * ctx.step * 2, ctx.step * 8);
    if (latest) {
      // The TradingView bar that was forming when last stored sits right at the coverage's end.
      const dataEnd = ctx.data.clip(-FAR, cursor).last();
      if (dataEnd !== null) cursor = dataEnd;
    }
    for (let guard = 0; guard < 100_000 && n < count; guard += 1) {
      if (ctx.start !== null && cursor <= ctx.start) break;
      const run = ctx.covered.rangeAt(cursor - 1);
      if (!run) break;
      // Skip covered stretches without bars (fetched-empty history) in one step.
      const dataEnd = ctx.data.clip(run[0], cursor).last();
      if (dataEnd === null) {
        cursor = run[0];
        tvCursor = Math.min(tvCursor, cursor);
        continue;
      }
      if (dataEnd < cursor) {
        cursor = Math.min(cursor, this.#spanEnd(ctx, dataEnd - 1));
        tvCursor = Math.min(tvCursor, cursor);
      }
      const from = this.#spanStart(ctx, Math.max(run[0], cursor - span));
      const items = await this.#build(ctx, from, cursor, tvCursor);
      const kept = items.filter((it) => it.bar.t < before);
      chunks.push(kept);
      n += kept.length;
      cursor = from;
      tvCursor = from;
      span *= 2;
    }
    const all = chunks.reverse().flat();
    const items = all.slice(-count);
    // Bars older than the page were read: more is stored.
    if (all.length > count) return { items, more: 'cached' };
    // Otherwise the walk read every bar in [cursor, before), so the edge is where it stopped, not the
    // oldest bar: coverage is kept per bucket, and the oldest bar's own bucket (a Sunday that opens at
    // 21:00) would pass for stored data before it.
    return { items, ...this.#edge(ctx, cursor) };
  }

  /** The newest `limit` bars labelled in [from, to]; gaps are skipped, not stopped at. */
  async #range(ctx: Context, from: number, to: number, limit: number): Promise<{ items: Item[]; truncated: boolean }> {
    const chunks: Item[][] = [];
    let n = 0;
    const floor = this.#spanStart(ctx, from);
    let cursor = this.#endBoundary(ctx, to, true);
    let tvCursor = to + 1;
    let span = Math.max(Math.min(limit, 5000) * ctx.step * 2, ctx.step * 8);
    while (cursor > floor && n <= limit) {
      const dataEnd = ctx.data.clip(floor, cursor).last();
      if (dataEnd === null) break;
      if (dataEnd < cursor) {
        cursor = Math.min(cursor, this.#spanEnd(ctx, dataEnd - 1));
        tvCursor = Math.min(tvCursor, cursor);
      }
      const lo = this.#spanStart(ctx, Math.max(floor, cursor - span));
      const items = await this.#build(ctx, lo, cursor, tvCursor);
      const kept = items.filter((it) => it.bar.t >= from && it.bar.t <= to);
      chunks.push(kept);
      n += kept.length;
      cursor = lo;
      tvCursor = lo;
      span *= 2;
    }
    const all = chunks.reverse().flat();
    return { items: all.length > limit ? all.slice(-limit) : all, truncated: all.length > limit };
  }

  async read(p: ReadParams): Promise<ReadResult> {
    const ctx = this.#context(p.symbol, p.tf);
    const keys = this.keys(p.symbol);
    const result = (items: Item[], extra: Partial<ReadResult>): ReadResult => ({
      bars: items.map((it) => it.bar),
      spans: spansOf(items),
      sources: items.map((it) => it.source),
      more: 'none',
      missing: [],
      truncated: false,
      keys,
      ...extra,
    });
    if (p.from !== undefined || (p.to !== undefined && p.count === undefined)) {
      const to = Math.min(p.to ?? ctx.now, FAR);
      const from = p.from ?? ctx.start ?? to - DEFAULT_LIMIT * ctx.step;
      const { items, truncated } = await this.#range(ctx, from, to, p.limit ?? DEFAULT_LIMIT);
      const edge = items[0] ? bucketSpan(items[0].bar.t, ctx.tf, ctx.opts)[0] : from;
      return result(items, { truncated, missing: this.#missing(ctx, from, to), ...this.#edge(ctx, edge) });
    }
    const count = Math.max(1, Math.floor(p.count ?? 500));
    const before = p.before ?? (p.to !== undefined ? p.to + 1 : ctx.now + 2 * ctx.step);
    const { items, more, gap } = await this.#page(ctx, before, count, p.latest ?? false);
    // Missing: what the page would still need, from the gap it stopped at.
    let missing: Range[] = [];
    if (items.length < count && more === 'gap' && gap) {
      const wanted = gap.to - Math.ceil((count - items.length) * ctx.step * 1.5);
      missing = [[Math.max(gap.from, wanted), gap.to]];
    }
    return result(items, { more, ...(gap ? { gap } : {}), missing });
  }

  // -------------------------------------------------------------------------------------------
  // CSV export

  async exportCsv(p: ReadParams & { from: number; to: number; path: string }): Promise<ExportSummary> {
    const ctx = this.#context(p.symbol, p.tf);
    const limit = p.limit ?? EXPORT_LIMIT;
    const to = Math.min(p.to, FAR);
    let from = p.from;
    let truncated = false;
    // More buckets than the cap: find where the newest `limit` bars start (cheap when they fit).
    if ((to - from) / ctx.step + 2 > limit) {
      let n = 0;
      let oldest: number | null = null;
      let cursor = this.#endBoundary(ctx, to, true);
      const floor = this.#spanStart(ctx, from);
      while (cursor > floor) {
        const dataEnd = ctx.data.clip(floor, cursor).last();
        if (dataEnd === null) break;
        cursor = Math.min(cursor, this.#spanEnd(ctx, dataEnd - 1));
        const lo = this.#spanStart(ctx, Math.max(floor, cursor - 200_000 * ctx.step));
        const items = (await this.#build(ctx, lo, cursor)).filter((it) => it.bar.t >= p.from && it.bar.t <= to);
        if (n + items.length > limit) {
          from = n < limit ? items[items.length - (limit - n)]!.bar.t : oldest!;
          truncated = true;
          break;
        }
        n += items.length;
        oldest = items[0]?.bar.t ?? oldest;
        cursor = lo;
      }
    }

    const out = fs.createWriteStream(p.path, { encoding: 'utf8' });
    const done = new Promise<void>((resolve, reject) => {
      out.on('finish', resolve);
      out.on('error', reject);
    });
    const write = (text: string) =>
      new Promise<void>((resolve, reject) => {
        const ok = out.write(text, (error) => (error ? reject(error) : undefined));
        if (ok) resolve();
        else out.once('drain', resolve);
      });
    const summary: ExportSummary = {
      count: 0,
      from: null,
      to: null,
      firstClose: null,
      lastClose: null,
      high: null,
      low: null,
      spans: [],
      lastRows: [],
      closes: [],
      missing: this.#missing(ctx, p.from, to),
      truncated,
      keys: this.keys(p.symbol),
    };
    let stride = 1;
    let sampled: number[] = [];
    try {
      await write('time,open,high,low,close,volume,source\n');
      const end = this.#endBoundary(ctx, to, true);
      let cursor = this.#spanStart(ctx, from);
      const chunk = 50_000 * ctx.step;
      while (cursor < end) {
        let next = Math.min(end, this.#spanStart(ctx, cursor + chunk));
        if (next <= cursor) next = Math.min(end, this.#spanEnd(ctx, cursor + chunk));
        if (ctx.data.clip(cursor, next).isEmpty) {
          // Jump over stored-empty or uncovered stretches.
          const after = ctx.data.clip(next, end).first();
          cursor = after === null ? end : Math.max(next, this.#spanStart(ctx, after));
          continue;
        }
        const items = (await this.#build(ctx, cursor, next)).filter((it) => it.bar.t >= from && it.bar.t <= to);
        let text = '';
        for (const { bar: b, source } of items) {
          text += `${iso(b.t)},${b.o},${b.h},${b.l},${b.c},${b.v},${source}\n`;
          summary.count += 1;
          summary.from ??= b.t;
          summary.to = b.t;
          summary.firstClose ??= b.c;
          summary.lastClose = b.c;
          summary.high = summary.high === null ? b.h : Math.max(summary.high, b.h);
          summary.low = summary.low === null ? b.l : Math.min(summary.low, b.l);
          const last = summary.spans[summary.spans.length - 1];
          if (last && last.source === source) {
            last.to = b.t;
            last.count += 1;
          } else summary.spans.push({ source, from: b.t, to: b.t, count: 1 });
          summary.lastRows.push([b.t, b.o, b.h, b.l, b.c, b.v, source]);
          if (summary.lastRows.length > 10) summary.lastRows.shift();
          if ((summary.count - 1) % stride === 0) {
            sampled.push(b.c);
            // Keep the sample small without knowing the total: halve it and double the stride.
            if (sampled.length > 320) {
              sampled = sampled.filter((_, i) => i % 2 === 0);
              stride *= 2;
            }
          }
        }
        if (text) await write(text);
        cursor = next;
      }
    } finally {
      out.end();
      await done;
    }
    // Up to 160 evenly spaced closes for a sparkline, ending on the last one.
    const n = Math.min(160, sampled.length);
    summary.closes = Array.from({ length: n }, (_, i) =>
      n === 1 ? sampled[0]! : sampled[Math.round((i * (sampled.length - 1)) / (n - 1))]!,
    );
    if (summary.lastClose !== null && n > 0) summary.closes[n - 1] = summary.lastClose;
    return summary;
  }
}
