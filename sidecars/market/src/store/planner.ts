// What a download would fetch, without fetching anything, and which buckets a chart or tool read
// needs right now.
//
// Routing: a symbol's history comes from Dukascopy when it maps to a Dukascopy instrument that is not
// a stock/ETF CFD, else from TradingView.
// Dukascopy plans count, per tier, the buckets in [max(from, effective start), min(to, now)) that are
// missing: not fetched, not provisional, not unavailable, and not queued by another running job
// (those are `queuedAhead`). m1 weekend days with no h1 bars inside a non-empty final h1 month are
// inferred empty without a request. A provisional copy due for its one re-fetch (a day stored while
// still open) is missing too, as a job counts it; an active bucket and a provisional one not yet due
// count as covered. Estimates use the fetcher's learned rate and rolling bytes per bucket.
// TradingView plans are pages per timeframe until TradingView's history runs out, and for a timeframe
// that reached its start once, pages for the holes between its stored runs: approximate.
//
// API (class Planner):
//   plan({symbol, from?, to?, tiers?}, {exclude?}) -> Plan
//   inferableDays(instrument, days) -> the m1 days (starts) that are known empty from h1
//   recordInferred(instrument, days)       stores them as fetched-empty m1 buckets
//   interactive(symbol, tf, from, to, max) -> [{tier, start}] newest first: what a read of [from, to)
//       needs fetched to be covered for tf (coarsest usable tier first), plus stale/due buckets of the
//       tier it reads; inferable weekend days are recorded instead of returned
//   containing(source, key, from, to, tiers, exclude?) -> the unfinished job covering that range (a
//       TradingView job covers any `from`: it pages back to TradingView's start)
// Also exported: TV_TIMEFRAMES, isWeekendDay, tvHoles(series, tv, key, tf) (holes between stored runs).

import { TIMEFRAMES, type Timeframe } from '../timeframes.ts';
import { TIERS, type Tier, bucketStart, bucketsIn, isActive, isTier, nextBucket } from './buckets.ts';
import { type DukascopyStore } from './dukascopy-store.ts';
import { type Fetcher } from './fetcher.ts';
import { IntervalSet, type Range } from './intervals.ts';
import { type Series, type Source, eligibleTiers } from './series.ts';
import { type TvStore } from './tv-store.ts';

/** Every chart timeframe a TradingView download fetches. */
export const TV_TIMEFRAMES: readonly Timeframe[] = ['1m', '5m', '15m', '1h', '4h', '1d', '1w'];
/** Pages TradingView usually serves per timeframe (5000 bars each), plus the two empty ones that end it. */
const TV_PAGES: Record<string, number> = { '1m': 4, '5m': 4, '15m': 4, '1h': 4, '4h': 3, '1d': 3, '1w': 1 };
const TV_PAGE_BARS = 5000;
const TV_BYTES_PER_PAGE = TV_PAGE_BARS * 70;
/** A weekend m1 day that still has to be asked for is nearly always an empty answer. */
const WEEKEND_BYTES = 140;
const DAY = 86400;
const FAR = 1e11;

export type JobStatus = 'queued' | 'running' | 'waiting' | 'paused' | 'done' | 'error';

export interface PlanParams {
  symbol: string;
  from?: number;
  to?: number;
  tiers?: string[];
}

export interface PlanTier {
  tier: string;
  from: number;
  to: number;
  requests: number;
}

export interface Plan {
  source: Source;
  key: string;
  name: string;
  from: number;
  to: number;
  tiers: string[];
  requests: number;
  bytes: number;
  seconds: number;
  queuedAhead: number;
  complete: boolean;
  approximate: boolean;
  perTier: PlanTier[];
  job: { id: string; status: JobStatus } | null;
}

/** What the planner needs to know about unfinished jobs. */
export interface JobView {
  id: string;
  status: JobStatus;
  source: Source;
  key: string;
  from: number;
  to: number;
  /** A job asked for "up to now": it keeps covering up to now while it runs. */
  openEnd: boolean;
  tiers: string[];
  /** Requests it still has to make (estimates). */
  remaining: number;
}

export interface Need {
  tier: Tier;
  start: number;
}

export interface PlannerDeps {
  store: DukascopyStore;
  fetcher: Pick<Fetcher, 'backlog' | 'effectiveRate' | 'tierStats'>;
  tv: TvStore;
  series: Series;
  /** Unfinished jobs (every status but done). */
  jobs?: () => JobView[];
  now?: () => number;
}

const weekday = (t: number) => (((Math.floor(t / DAY) + 4) % 7) + 7) % 7;
export const isWeekendDay = (t: number) => weekday(t) === 0 || weekday(t) === 6;

const MOVING: ReadonlySet<JobStatus> = new Set(['queued', 'running', 'waiting']);

/** Uncovered stretches between a TradingView series' stored runs (the tolerance series.ts applies),
 *  oldest first. Time after the newest run is not a hole: a live stream fills it. */
export function tvHoles(series: Series, tv: TvStore, key: string, tf: Timeframe): Range[] {
  const intervals = tv.coverage(key, tf).intervals;
  const first = intervals.first();
  const last = intervals.last();
  if (first === null || last === null || intervals.count < 2) return [];
  return series.coverage(key, tf).covered.gaps(first, last);
}

export class Planner {
  readonly #d: PlannerDeps;

  constructor(deps: PlannerDeps) {
    this.#d = deps;
  }

  #now(): number {
    return Math.floor((this.#d.now ?? Date.now)() / 1000);
  }

  #jobs(): JobView[] {
    return this.#d.jobs?.() ?? [];
  }

  /** The unfinished job whose range and tiers contain this one's, if any. */
  containing(source: Source, key: string, from: number, to: number, tiers: string[], exclude?: string): JobView | null {
    const now = this.#now();
    let best: JobView | null = null;
    for (const job of this.#jobs()) {
      if (job.id === exclude || job.source !== source || job.key !== key || job.status === 'done') continue;
      const jobTo = job.openEnd ? Math.max(job.to, now) : job.to;
      // A TradingView job pages back to TradingView's start whatever `from` it recorded (the oldest
      // cached bar when it began, which its own pages then move back).
      const jobFrom = job.source === 'tradingview' ? -Infinity : job.from;
      if (jobFrom > from || jobTo < to || !tiers.every((t) => job.tiers.includes(t))) continue;
      // Prefer one that is moving, then the most recent.
      if (!best || (MOVING.has(job.status) && !MOVING.has(best.status))) best = job;
    }
    return best;
  }

  async plan(p: PlanParams, opts: { exclude?: string } = {}): Promise<Plan> {
    const route = this.#d.series.route(p.symbol);
    return route.instrument
      ? this.#dukascopy(route.instrument, route.name, p, opts.exclude)
      : this.#tradingview(route.key, p, opts.exclude);
  }

  async #dukascopy(instrument: string, name: string, p: PlanParams, exclude?: string): Promise<Plan> {
    const { store, fetcher } = this.#d;
    const now = this.#now();
    const asked = (p.tiers ?? []).filter(isTier);
    const tiers = asked.length ? TIERS.filter((t) => asked.includes(t)) : [...TIERS];
    const starts = tiers.map((t) => store.effectiveStart(instrument, t)).filter((t): t is number => t !== null);
    const planFrom = p.from ?? (starts.length ? Math.min(...starts) : now);
    const planTo = Math.min(p.to ?? now, now);
    // Buckets other moving jobs will fetch anyway.
    const others = this.#jobs().filter(
      (j) => j.id !== exclude && j.source === 'dukascopy' && j.key === instrument && MOVING.has(j.status),
    );
    const perTier: PlanTier[] = [];
    let requests = 0;
    let queuedAhead = 0;
    let bytes = 0;
    for (const tier of tiers) {
      const eff = store.effectiveStart(instrument, tier);
      const from = Math.max(planFrom, eff ?? FAR);
      const to = planTo;
      if (eff === null || !(to > from)) {
        perTier.push({ tier, from: Math.min(from, to), to, requests: 0 });
        continue;
      }
      const queued = new IntervalSet();
      for (const j of others) {
        if (j.tiers.includes(tier)) queued.add(j.from, j.openEnd ? Math.max(j.to, now) : j.to);
      }
      let missing = store.missingBuckets(instrument, tier, from, to);
      if (tier === 'm1') {
        const inferred = await this.inferableDays(instrument, missing);
        missing = missing.filter((b) => !inferred.has(b));
      }
      // Due provisional copies are work too (jobs.ts plans them the same way): until re-fetched the
      // bucket stays partial, so the range is not complete.
      const first = bucketStart(tier, from);
      for (const b of store.dueBuckets(instrument, tier, now)) {
        if (b >= first && b < to && !isActive(tier, b, now)) missing.push(b);
      }
      const ahead = missing.filter((b) => queued.contains(b));
      missing = missing.filter((b) => !queued.contains(b));
      const perBucket = fetcher.tierStats(tier).bytesPerBucket;
      for (const b of missing) bytes += tier === 'm1' && isWeekendDay(b) ? WEEKEND_BYTES : perBucket;
      requests += missing.length;
      queuedAhead += ahead.length;
      perTier.push({ tier, from, to, requests: missing.length });
    }
    const backlog = Math.max(
      fetcher.backlog('bulk'),
      others.reduce((sum, j) => sum + j.remaining, 0),
    );
    const rate = Math.max(fetcher.effectiveRate(), 0.01);
    const job = this.containing('dukascopy', instrument, planFrom, planTo, tiers, exclude);
    return {
      source: 'dukascopy',
      key: instrument,
      name,
      from: planFrom,
      to: planTo,
      tiers,
      requests,
      bytes: Math.round(bytes),
      seconds: requests === 0 ? 0 : Math.ceil((requests + backlog) / rate),
      queuedAhead,
      complete: requests === 0 && queuedAhead === 0,
      approximate: false,
      perTier,
      job: job ? { id: job.id, status: job.status } : null,
    };
  }

  async #tradingview(key: string, p: PlanParams, exclude?: string): Promise<Plan> {
    const { tv } = this.#d;
    const now = this.#now();
    const asked = (p.tiers ?? []).filter((t) => (TV_TIMEFRAMES as readonly string[]).includes(t));
    const tfs = asked.length ? TV_TIMEFRAMES.filter((t) => asked.includes(t)) : [...TV_TIMEFRAMES];
    const perTier: PlanTier[] = [];
    let requests = 0;
    let complete = true;
    let earliest = now;
    for (const tf of tfs) {
      const cov = tv.coverage(key, tf);
      const first = cov.reachedStart ?? cov.intervals.first() ?? now;
      earliest = Math.min(earliest, first);
      let pages = (TV_PAGES[tf] ?? 3) + 2;
      if (cov.reachedStart !== null) {
        // History reached TradingView's start once: only holes between stored runs are left to page.
        const step = TIMEFRAMES[tf].seconds;
        pages = 0;
        for (const [f, t] of tvHoles(this.#d.series, tv, key, tf))
          pages += Math.ceil((t - f) / step / TV_PAGE_BARS) + 1;
      }
      if (pages > 0) complete = false;
      requests += pages;
      perTier.push({ tier: tf, from: p.from ?? first, to: Math.min(p.to ?? now, now), requests: pages });
    }
    const from = p.from ?? earliest;
    const to = Math.min(p.to ?? now, now);
    const job = this.containing('tradingview', key, from, to, tfs, exclude);
    return {
      source: 'tradingview',
      key,
      name: key,
      from,
      to,
      tiers: tfs,
      requests,
      bytes: requests * TV_BYTES_PER_PAGE,
      seconds: Math.ceil(requests * tv.secondsPerPage),
      queuedAhead: 0,
      complete,
      approximate: true,
      perTier,
      job: job ? { id: job.id, status: job.status } : null,
    };
  }

  /** m1 weekend days with no h1 bars inside a final, non-empty h1 month: known empty without asking. */
  async inferableDays(instrument: string, days: readonly number[]): Promise<Set<number>> {
    const { store } = this.#d;
    const out = new Set<number>();
    const byMonth = new Map<number, number[]>();
    for (const day of days) {
      if (!isWeekendDay(day)) continue;
      const month = bucketStart('h1', day);
      const list = byMonth.get(month);
      if (list) list.push(day);
      else byMonth.set(month, [day]);
    }
    if (byMonth.size === 0) return out;
    const h1 = store.sets(instrument, 'h1');
    for (const [month, list] of byMonth) {
      // Never from an empty h1 month (coarse tiers have holes), nor a provisional one (days after its
      // build would look empty).
      if (!h1.fetched.contains(month) || h1.empty.contains(month)) continue;
      const bars = await store.readNative(instrument, 'h1', month, nextBucket('h1', month));
      if (bars.length === 0) continue;
      const traded = new Set(bars.map((b) => Math.floor(b.t / DAY)));
      for (const day of list) if (!traded.has(Math.floor(day / DAY))) out.add(day);
    }
    return out;
  }

  async recordInferred(instrument: string, days: Iterable<number>): Promise<number> {
    const now = this.#now();
    let n = 0;
    for (const day of days) {
      if (this.#d.store.status(instrument, 'm1', day) !== 'missing') continue;
      await this.#d.store.put(instrument, 'm1', day, null, { builtAt: now, final: true });
      n += 1;
    }
    return n;
  }

  /** What reading [from, to) at tf needs fetched: see the header. */
  async interactive(symbol: string, tf: Timeframe, from: number, to: number, max: number): Promise<Need[]> {
    const { store, series } = this.#d;
    const route = series.route(symbol);
    const instrument = route.instrument;
    if (!instrument || max <= 0) return [];
    const now = this.#now();
    const end = Math.min(to, now + 1);
    if (!(end > from)) return [];
    const { covered } = series.coverage(symbol, tf);
    let uncovered = new IntervalSet([[from, end]]).minus(covered);
    const needs: Need[] = [];
    const seen = new Set<string>();
    const push = (tier: Tier, start: number) => {
      const key = `${tier}/${start}`;
      if (seen.has(key)) return;
      seen.add(key);
      needs.push({ tier, start });
    };
    const tiers = eligibleTiers(tf);
    for (const tier of tiers) {
      if (uncovered.isEmpty) break;
      const eff = store.effectiveStart(instrument, tier);
      if (eff === null) continue;
      const sets = store.sets(instrument, tier);
      // Where this tier is known to hold nothing, a finer one has to answer.
      const want = uncovered.clip(eff, FAR).minus(sets.empty.union(sets.unavailable));
      const missing: number[] = [];
      for (const [f, t] of want) {
        for (const b of bucketsIn(tier, f, t))
          if (!sets.covered.contains(b) && !sets.unavailable.contains(b)) missing.push(b);
      }
      let ask = missing;
      if (tier === 'm1') {
        const inferred = await this.inferableDays(instrument, missing);
        if (inferred.size) {
          await this.recordInferred(instrument, inferred);
          ask = missing.filter((b) => !inferred.has(b));
        }
      }
      for (const b of ask) push(tier, b);
      uncovered = uncovered.minus(want);
    }
    // Refresh what the read will show: stale active and due provisional buckets of the tier it reads.
    const readTier = tiers.find((tier) => !store.sets(instrument, tier).data.clip(from, end).isEmpty);
    if (readTier) {
      for (const [f, t] of store.sets(instrument, readTier).provisional.clip(from, end)) {
        for (const b of bucketsIn(readTier, f, t))
          if (store.needsFetch(instrument, readTier, b, now)) push(readTier, b);
      }
    }
    return needs.sort((a, b) => b.start - a.start || tiers.indexOf(a.tier) - tiers.indexOf(b.tier)).slice(0, max);
  }
}
