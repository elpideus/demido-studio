// What a download would fetch, without fetching anything, and which buckets a chart or tool read
// needs right now.
//
// Routing: a symbol's history comes from Dukascopy when it maps to a Dukascopy instrument that is not
// a stock/ETF CFD, else from TradingView.
// Dukascopy history is 1-minute candles only (every timeframe is built from them, see series.ts), so a
// Dukascopy plan is always the m1 tier, whatever timeframe asked. It counts the days in
// [max(from, m1's effective start), min(to, now)) that are missing: not fetched, not provisional, not
// unavailable, and not queued by another running job (those are `queuedAhead`). A provisional copy
// due for its one re-fetch (a day stored while still open) is missing too, as a job counts it; an
// active bucket and a provisional one not yet due count as covered. Estimates use the fetcher's
// learned rate and rolling bytes per bucket. `back` ('month' | 'year') sets `from` to that much before
// the stored 1-minute history (where its newest stretch starts, gaps shorter than two months bridged:
// the download fills them anyway; now when nothing is stored): the chart's "1 more month / year", the
// same whatever timeframe the chart shows.
// TradingView plans are pages per timeframe until TradingView's history runs out, and for a timeframe
// that reached its start once, pages for the holes between its stored runs: approximate.
//
// API (class Planner):
//   plan({symbol, from?, back?, to?, tiers?}, {exclude?}) -> Plan   (tiers only matter to TradingView)
//   storedFrom(instrument) -> start of the newest stretch of stored 1-minute history, or null
//   interactive(symbol, tf, from, to, max) -> [{tier, start}] newest first: the m1 days a read of
//       [from, to) needs fetched to be covered, plus its stale or due ones
//   containing(source, key, from, to, tiers, exclude?) -> the unfinished job covering that range (a
//       TradingView job covers any `from`: it pages back to TradingView's start)
// Also exported: TV_TIMEFRAMES, isWeekendDay, tvHoles(series, tv, key, tf) (holes between stored runs).

import { TIMEFRAMES, type Timeframe } from '../timeframes.ts';
import { type Tier, bucketStart, bucketsIn, isActive } from './buckets.ts';
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
/** Gaps in the stored history shorter than this do not end its newest stretch (see storedFrom). */
export const BRIDGE_SECONDS = 60 * DAY;
const FAR = 1e11;

export type JobStatus = 'queued' | 'running' | 'waiting' | 'paused' | 'done' | 'error';

export type Back = 'month' | 'year';

export interface PlanParams {
  symbol: string;
  from?: number;
  /** Instead of `from`: this much before the stored 1-minute history (Dukascopy only). */
  back?: Back;
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
  /** Dukascopy: where the newest stretch of stored 1-minute history starts (null: nothing stored). */
  storedFrom: number | null;
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

/** The UTC day `back` before `t` (a month or a year earlier, the same day of the month when it exists). */
export function backFrom(t: number, back: Back): number {
  const d = new Date(Math.floor(t / DAY) * DAY * 1000);
  const day = d.getUTCDate();
  d.setUTCDate(1);
  if (back === 'year') d.setUTCFullYear(d.getUTCFullYear() - 1);
  else d.setUTCMonth(d.getUTCMonth() - 1);
  const last = new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 0)).getUTCDate();
  d.setUTCDate(Math.min(day, last));
  return Math.floor(d.getTime() / 1000);
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

  /** Where the newest stretch of stored 1-minute history starts: fetched days (empty ones included)
   *  and days the source refuses, back from the newest one, over gaps shorter than BRIDGE_SECONDS (a
   *  day the chart fetched today after a few weeks away does not make the history start today). Null
   *  when nothing is stored. */
  storedFrom(instrument: string): number | null {
    const sets = this.#d.store.sets(instrument, 'm1');
    const runs = [...sets.covered.union(sets.unavailable)];
    let i = runs.length - 1;
    if (i < 0) return null;
    while (i > 0 && runs[i]![0] - runs[i - 1]![1] < BRIDGE_SECONDS) i -= 1;
    return runs[i]![0];
  }

  async #dukascopy(instrument: string, name: string, p: PlanParams, exclude?: string): Promise<Plan> {
    const { store, fetcher } = this.#d;
    const now = this.#now();
    const tier: Tier = 'm1';
    const eff = store.effectiveStart(instrument, tier);
    const storedFrom = this.storedFrom(instrument);
    let planFrom = p.from ?? (p.back ? backFrom(storedFrom ?? now, p.back) : (eff ?? now));
    if (p.back && eff !== null) planFrom = Math.max(planFrom, eff);
    const planTo = Math.min(p.to ?? now, now);
    // Buckets other moving jobs will fetch anyway.
    const others = this.#jobs().filter(
      (j) => j.id !== exclude && j.source === 'dukascopy' && j.key === instrument && MOVING.has(j.status),
    );
    let requests = 0;
    let queuedAhead = 0;
    let bytes = 0;
    const from = Math.max(planFrom, eff ?? FAR);
    const to = planTo;
    let perTier: PlanTier;
    if (eff === null || !(to > from)) {
      perTier = { tier, from: Math.min(from, to), to, requests: 0 };
    } else {
      const queued = new IntervalSet();
      for (const j of others) {
        if (j.tiers.includes(tier)) queued.add(j.from, j.openEnd ? Math.max(j.to, now) : j.to);
      }
      let missing = store.missingBuckets(instrument, tier, from, to);
      // Due provisional copies are work too (jobs.ts plans them the same way): until re-fetched the
      // day stays partial, so the range is not complete.
      const first = bucketStart(tier, from);
      for (const b of store.dueBuckets(instrument, tier, now)) {
        if (b >= first && b < to && !isActive(tier, b, now)) missing.push(b);
      }
      const ahead = missing.filter((b) => queued.contains(b));
      missing = missing.filter((b) => !queued.contains(b));
      const perBucket = fetcher.tierStats(tier).bytesPerBucket;
      for (const b of missing) bytes += isWeekendDay(b) ? WEEKEND_BYTES : perBucket;
      requests = missing.length;
      queuedAhead = ahead.length;
      perTier = { tier, from, to, requests };
    }
    const backlog = Math.max(
      fetcher.backlog('bulk'),
      others.reduce((sum, j) => sum + j.remaining, 0),
    );
    const rate = Math.max(fetcher.effectiveRate(), 0.01);
    const job = this.containing('dukascopy', instrument, planFrom, planTo, [tier], exclude);
    return {
      source: 'dukascopy',
      key: instrument,
      name,
      from: planFrom,
      to: planTo,
      tiers: [tier],
      requests,
      bytes: Math.round(bytes),
      seconds: requests === 0 ? 0 : Math.ceil((requests + backlog) / rate),
      queuedAhead,
      complete: requests === 0 && queuedAhead === 0,
      approximate: false,
      perTier: [perTier],
      job: job ? { id: job.id, status: job.status } : null,
      storedFrom,
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
      storedFrom: null,
    };
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
      for (const b of missing) push(tier, b);
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
