// Background downloads. A job is one requested range (`from`, `to`) and tier set for one key;
// several jobs may share a key, and the fetcher's bucket-level dedupe fetches a bucket once for
// every job that wants it.
//
// - Dukascopy jobs re-plan from the manifest on every (re)start, so a resume never re-fetches, and
//   feed the fetcher lazily (a window of 2 x in-flight of their next buckets, bulk lane). Order: d1,
//   then h1 for the whole range (cheap; each is the coarser tier the next one's empty-streak check
//   reads, and h1 makes m1 weekends inferable), then m1, newest first throughout.
// - A run's plan is the buckets of its range the manifest lacks, plus provisional copies due for
//   their one re-fetch (a day stored while it was still open). Every planned bucket that becomes
//   covered while the job runs counts as done with its bytes, whoever fetched it (a boost, a chart,
//   another job). m1 weekend days known empty from h1 leave the plan (they are not work), and buckets
//   below a learned start are `skipped` (reported apart from `total`); nothing else leaves it undone.
// - Empty streak (a backstop for bad metadata), armed once the tier returned a non-empty bucket in
//   this walk: 20 consecutive empty weekday m1 buckets (h1: 3 months; d1: never) are checked against
//   the coarser tier (h1 for m1, d1 for h1), walked back from the streak (fetching what nobody has)
//   until it shows older data or reaches its own start. If it has data older than the streak, it is a
//   hole: probe sparsely (one weekday a month; h1: one month a year). Where a probe finds data, walk
//   densely again from just below the streak, so nothing the probes passed over is left out. If the
//   probes reach the tier's start without data, the tier starts after the hole (learned start); if
//   the range ends before that, or the coarser tier could not be read, the passed-over buckets are
//   fetched after all. With the coarser tier known empty down to its start, the learned start is
//   recorded with the evidence and older buckets are skipped.
// - TradingView jobs page back through every timeframe (one chart session each) until two
//   consecutive empty pages, writing into the TradingView store as they go. A timeframe that already
//   reached TradingView's start pages only its holes: from the run above each hole back until the
//   pages meet the run below (the first page covers up to the run above, `to` being exclusive); a
//   hole TradingView serves nothing for is recorded as covered without bars. They need a session.
// - Statuses: queued | running | waiting (the fetcher's breaker is open or Dukascopy asked for a
//   pause) | paused | done | error. Records live in <cache>/jobs/<id>.json (atomic writes); running,
//   queued and waiting jobs resume when the sidecar starts. Jobs are never pruned: old chat cards
//   must still resolve; `cancel` removes one (event `download.removed {jobId}`). Progress events at
//   most 4 per second per job.
//
// API (class Jobs): load(), start({symbol, from?, to?, tiers?, origin}) -> {jobId, plan},
//   pause(id), resume(id), cancel(id), get(id), list(symbol?), views() (for the planner),
//   wait({symbol, tf, from, to, jobId?, timeoutMs, boost?}) -> {covered, status}, busy(market), close()

import fsp from 'node:fs/promises';
import path from 'node:path';

import { RpcError } from '../protocol.ts';
import { type Timeframe } from '../timeframes.ts';
import { TIERS, type Tier, bucketKey, bucketStart, bucketsIn, isActive, nextBucket } from './buckets.ts';
import { type DukascopyStore, type StoreChange, writeAtomic } from './dukascopy-store.ts';
import { type FetchResult, type Fetcher, MAX_IN_FLIGHT } from './fetcher.ts';
import { type Range } from './intervals.ts';
import { type JobStatus, type JobView, type Plan, type Planner, TV_TIMEFRAMES, tvHoles } from './planner.ts';
import { type Series, type Source } from './series.ts';
import { type TvStore } from './tv-store.ts';

export type Origin = 'chart' | 'chat' | 'data';

export interface Job {
  id: string;
  source: Source;
  key: string;
  symbol: string;
  name: string;
  from: number;
  to: number;
  tiers: string[];
  status: JobStatus;
  total: number;
  done: number;
  /** Planned files the source has nothing for (below a learned start): no longer in `total`. */
  skipped: number;
  bytes: number;
  etaSeconds: number | null;
  rate: number;
  inFlight: number;
  perTier: Array<{ tier: string; total: number; done: number }>;
  origin: Origin;
  createdAt: number;
  updatedAt: number;
  message?: string;
}

export interface JobRecord extends Job {
  version: 1;
  /** Asked for "up to now": `to` follows now while it runs. */
  openEnd: boolean;
  /** Buckets that failed for good in the current run. */
  failed: number;
  /** Dukascopy: the planned buckets not yet counted, per tier, as runs of consecutive buckets. A
   *  resume starts from them, so ones covered meanwhile (a chart, a tool read) still count as done. */
  pending?: Partial<Record<Tier, Range[]>>;
}

export interface TvSession {
  loggedIn(): boolean;
  pageHistory(
    symbol: string,
    tf: Timeframe,
    opts: {
      to?: number;
      /** Stop once the pages reach back to this time (a hole's lower edge). */
      stopAt?: number;
      signal?: AbortSignal;
      onPage?: (bars: number, seconds: number) => void;
    },
  ): Promise<unknown>;
}

export interface JobsDeps {
  cacheDir: string;
  store: DukascopyStore;
  fetcher: Pick<Fetcher, 'request' | 'cancelJob' | 'status' | 'effectiveRate' | 'subscribe'>;
  planner: Planner;
  series: Series;
  tv: TvStore;
  tvSession?: TvSession;
  emit: (event: string, params: unknown) => void;
  log?: (level: 'info' | 'warn' | 'error', message: string) => void;
  /** Clock in ms (tests). */
  now?: () => number;
}

export interface StartParams {
  symbol: string;
  from?: number;
  to?: number;
  tiers?: string[];
  origin: Origin;
}

export interface WaitParams {
  symbol: string;
  tf: Timeframe;
  from: number;
  to: number;
  jobId?: string;
  timeoutMs: number;
  /** Move what the range needs for `tf` into the waiting lane (default true). A wait for a whole job
   *  passes false: the job's own order and the bulk lane's fairness decide then. */
  boost?: boolean;
}

export const WINDOW = 2 * MAX_IN_FLIGHT;
/** Consecutive empty weekday buckets (m1) or months (h1) that make the empty-streak check. */
export const STREAK: Partial<Record<Tier, number>> = { m1: 20, h1: 3 };
const PROGRESS_MS = 250;
const RATE_WINDOW_MS = 30_000;
const DAY = 86400;
const FAR = 1e11;
/** Shortest bucket length per tier, to tell a one-bucket store change from a wide one. */
const MIN_BUCKET_SECONDS: Record<Tier, number> = { m1: DAY, h1: 28 * DAY, d1: 365 * DAY };
const MOVING: ReadonlySet<JobStatus> = new Set(['queued', 'running', 'waiting']);

const weekday = (t: number) => (((Math.floor(t / DAY) + 4) % 7) + 7) % 7;
const isWeekday = (t: number) => weekday(t) !== 0 && weekday(t) !== 6;
const coarserOf = (tier: Tier): Tier => (tier === 'm1' ? 'h1' : 'd1');
const iso = (t: number) => new Date(t * 1000).toISOString();

/** The one bucket per month (m1: the third-week Wednesday) or per year (h1: July) a sparse walk asks for. */
export function isProbe(tier: Tier, start: number): boolean {
  const d = new Date(start * 1000);
  if (tier === 'h1') return d.getUTCMonth() === 6;
  return d.getUTCDate() >= 15 && d.getUTCDate() <= 21 && d.getUTCDay() === 3;
}

const previousBucket = (tier: Tier, start: number) => bucketStart(tier, start - 1);

function publicJob(r: JobRecord): Job {
  const {
    version: _v,
    openEnd: _o,
    failed: _f,
    pending: _p,
    migratedFrom: _m,
    ...job
  } = r as JobRecord & { migratedFrom?: string };
  return { ...job, perTier: job.perTier.map((t) => ({ ...t })) };
}

/** Planned buckets as runs of consecutive buckets (what a record keeps of a stopped plan). */
function toRuns(tier: Tier, buckets: Iterable<number>): Range[] {
  const out: Range[] = [];
  for (const b of [...buckets].sort((a, c) => a - c)) {
    const last = out[out.length - 1];
    if (last && last[1] === b) last[1] = nextBucket(tier, b);
    else out.push([b, nextBucket(tier, b)]);
  }
  return out;
}

function fromRuns(tier: Tier, runs: unknown): Set<number> {
  const out = new Set<number>();
  if (!Array.isArray(runs)) return out;
  for (const run of runs) {
    if (Array.isArray(run) && typeof run[0] === 'number' && typeof run[1] === 'number') {
      for (const b of bucketsIn(tier, run[0], run[1])) out.add(b);
    }
  }
  return out;
}

interface TierPlan {
  lo: number;
  hi: number;
  /** Planned buckets not yet covered; they leave as they are fetched, answered or skipped. */
  buckets: Set<number>;
}

interface Run {
  controller: AbortController;
  promise: Promise<void>;
  inFlight: number;
  completions: number[];
  /** Dukascopy: the plan per tier (the store listener counts buckets others fetch for it). */
  plans: Map<Tier, TierPlan>;
  /** `tier/start` -> requests out for this job (its walk, or a boost naming it): their results count
   *  those buckets, with the bytes fetched. */
  requesting: Map<string, number>;
  /** Dukascopy: `plans` hold the whole plan (the record's `pending` may be taken from them). */
  planned: boolean;
}

type Outcome = 'data' | 'empty' | 'skip' | 'aborted';

/** What the coarser tier says about an empty streak: older data (a hole), none (the tier's start),
 *  or it could not be read. */
type Verdict = 'hole' | 'start' | 'unknown';

interface Pending {
  start: number;
  requested: boolean;
  controller: AbortController | null;
  outcome: Promise<Outcome>;
}

export class Jobs {
  readonly #d: JobsDeps;
  readonly #dir: string;
  readonly #jobs = new Map<string, JobRecord>();
  readonly #runs = new Map<string, Run>();
  readonly #saveTimers = new Map<string, ReturnType<typeof setTimeout>>();
  readonly #saving = new Map<string, Promise<void>>();
  readonly #progressTimers = new Map<string, ReturnType<typeof setTimeout>>();
  readonly #lastProgress = new Map<string, number>();
  #closing = false;
  #pauseTimer: ReturnType<typeof setTimeout> | null = null;
  readonly #unsubscribe: Array<() => void> = [];

  constructor(deps: JobsDeps) {
    this.#d = deps;
    this.#dir = path.join(deps.cacheDir, 'jobs');
    this.#unsubscribe.push(deps.fetcher.subscribe(() => this.#applyFetcherState()));
    this.#unsubscribe.push(deps.store.subscribe((change) => this.#stored(change)));
  }

  #nowMs(): number {
    return (this.#d.now ?? Date.now)();
  }

  #now(): number {
    return Math.floor(this.#nowMs() / 1000);
  }

  #log(level: 'info' | 'warn' | 'error', message: string): void {
    this.#d.log?.(level, message);
  }

  // -------------------------------------------------------------------------------------------
  // Records

  /** Reads every stored job and resumes the ones that were moving. */
  async load(): Promise<void> {
    let names: string[] = [];
    try {
      names = await fsp.readdir(this.#dir);
    } catch {
      return;
    }
    for (const name of names) {
      if (!name.endsWith('.json')) continue;
      try {
        const r = JSON.parse(await fsp.readFile(path.join(this.#dir, name), 'utf8')) as JobRecord;
        if (!r || typeof r.id !== 'string' || typeof r.key !== 'string' || !Array.isArray(r.tiers)) continue;
        r.version = 1;
        r.openEnd = r.openEnd === true;
        r.failed = 0;
        r.skipped = typeof r.skipped === 'number' ? r.skipped : 0;
        r.perTier ??= [];
        this.#jobs.set(r.id, r);
      } catch (error) {
        this.#log('warn', `Skipped the unreadable download record ${name}: ${(error as Error).message}`);
      }
    }
    for (const r of this.#jobs.values()) if (MOVING.has(r.status)) this.#run(r);
  }

  get(id: string): Job | null {
    const r = this.#jobs.get(id);
    return r ? publicJob(this.#refresh(r)) : null;
  }

  list(symbol?: string): Job[] {
    let rs = [...this.#jobs.values()];
    if (symbol?.trim()) {
      const route = this.#d.series.route(symbol);
      const asked = symbol.trim().toLowerCase();
      rs = rs.filter(
        (r) =>
          (r.source === route.source && r.key.toLowerCase() === route.key.toLowerCase()) ||
          r.symbol.toLowerCase() === asked,
      );
    }
    return rs.sort((a, b) => b.updatedAt - a.updatedAt).map((r) => publicJob(this.#refresh(r)));
  }

  views(): JobView[] {
    return [...this.#jobs.values()]
      .filter((r) => r.status !== 'done')
      .map((r) => ({
        id: r.id,
        status: r.status,
        source: r.source,
        key: r.key,
        from: r.from,
        to: r.to,
        openEnd: r.openEnd,
        tiers: r.tiers,
        remaining: Math.max(0, r.total - r.done),
      }));
  }

  /** A job for this market is moving (deleting its data now would race it). */
  busy(market: string): boolean {
    const m = market.toLowerCase();
    return [...this.#jobs.values()].some((r) => MOVING.has(r.status) && r.key.toLowerCase() === m);
  }

  #save(r: JobRecord, now = false): void {
    const pending = this.#saveTimers.get(r.id);
    if (pending && !now) return;
    if (pending) clearTimeout(pending);
    this.#saveTimers.delete(r.id);
    const write = () => {
      this.#saveTimers.delete(r.id);
      if (this.#jobs.get(r.id) !== r) return;
      // The plan's open buckets with the counts they go with: one snapshot, so a resume after a crash
      // neither counts a bucket twice nor drops one.
      const run = this.#runs.get(r.id);
      if (run) this.#keepPending(r, run);
      const json = JSON.stringify(r);
      const previous = this.#saving.get(r.id) ?? Promise.resolve();
      const next = previous
        .then(() => (this.#jobs.get(r.id) === r ? writeAtomic(path.join(this.#dir, `${r.id}.json`), json) : undefined))
        .catch((error: unknown) => this.#log('error', `Could not save download ${r.id}: ${(error as Error).message}`));
      this.#saving.set(r.id, next);
      void next.then(() => {
        if (this.#saving.get(r.id) === next) this.#saving.delete(r.id);
      });
    };
    if (now) write();
    else {
      const timer = setTimeout(write, 1000);
      timer.unref?.();
      this.#saveTimers.set(r.id, timer);
    }
  }

  /** Live fields (rate, in flight, ETA) from the run. */
  #refresh(r: JobRecord): JobRecord {
    const run = this.#runs.get(r.id);
    const now = this.#nowMs();
    if (!run || !MOVING.has(r.status)) {
      r.rate = 0;
      r.inFlight = 0;
      r.etaSeconds = r.status === 'done' ? 0 : null;
      return r;
    }
    run.completions = run.completions.filter((t) => t > now - RATE_WINDOW_MS);
    const first = run.completions[0];
    const elapsed = first === undefined ? 0 : Math.max(1, (now - first) / 1000);
    const measured = run.completions.length >= 5 ? run.completions.length / elapsed : 0;
    const moving = [...this.#runs.keys()].filter((id) => MOVING.has(this.#jobs.get(id)?.status ?? 'done')).length;
    // While Dukascopy is unreachable or throttling, nothing moves: no rate to promise.
    const share =
      r.source === 'dukascopy' && r.status !== 'waiting' ? this.#d.fetcher.effectiveRate() / Math.max(1, moving) : 0;
    const rate = r.status === 'waiting' ? 0 : measured || share;
    r.rate = Math.round(rate * 100) / 100;
    r.inFlight = Math.min(run.inFlight, r.source === 'dukascopy' ? this.#d.fetcher.status().inFlight : run.inFlight);
    const remaining = Math.max(0, r.total - r.done);
    r.etaSeconds = remaining === 0 ? 0 : rate > 0 ? Math.ceil(remaining / rate) : null;
    return r;
  }

  #progress(r: JobRecord): void {
    r.updatedAt = this.#nowMs();
    this.#save(r);
    if (this.#progressTimers.has(r.id)) return;
    const wait = PROGRESS_MS - (this.#nowMs() - (this.#lastProgress.get(r.id) ?? 0));
    const send = () => {
      this.#progressTimers.delete(r.id);
      if (this.#jobs.get(r.id) !== r) return;
      this.#lastProgress.set(r.id, this.#nowMs());
      this.#d.emit('download.progress', { job: publicJob(this.#refresh(r)) });
    };
    if (wait <= 0) send();
    else {
      const timer = setTimeout(send, wait);
      timer.unref?.();
      this.#progressTimers.set(r.id, timer);
    }
  }

  #setStatus(r: JobRecord, status: JobStatus, message?: string): void {
    r.status = status;
    if (message) r.message = message;
    else delete r.message;
    r.updatedAt = this.#nowMs();
    this.#save(r, true);
    const timer = this.#progressTimers.get(r.id);
    if (timer) clearTimeout(timer);
    this.#progressTimers.delete(r.id);
    this.#lastProgress.set(r.id, this.#nowMs());
    const event = status === 'done' ? 'download.done' : status === 'error' ? 'download.error' : 'download.progress';
    this.#d.emit(event, { job: publicJob(this.#refresh(r)) });
  }

  // -------------------------------------------------------------------------------------------
  // Commands

  async start(p: StartParams): Promise<{ jobId: string; plan: Plan }> {
    const route = this.#d.series.route(p.symbol);
    if (route.source === 'tradingview' && !this.#d.tvSession?.loggedIn()) {
      throw new RpcError('NOT_LOGGED_IN', 'Sign in to TradingView to download its history.');
    }
    const plan = await this.#d.planner.plan({ symbol: p.symbol, from: p.from, to: p.to, tiers: p.tiers });
    if (plan.job) {
      const existing = this.#jobs.get(plan.job.id);
      if (existing) {
        if (existing.status === 'paused' || existing.status === 'error') this.resume(existing.id);
        return { jobId: existing.id, plan };
      }
    }
    const now = this.#nowMs();
    const r: JobRecord = {
      version: 1,
      id: `j${now.toString(36)}${Math.random().toString(36).slice(2, 7)}`,
      source: route.source,
      key: plan.key,
      symbol: p.symbol.trim(),
      name: plan.name,
      from: plan.from,
      to: plan.to,
      tiers: plan.tiers,
      status: 'queued',
      total: plan.requests + plan.queuedAhead,
      done: 0,
      skipped: 0,
      bytes: 0,
      etaSeconds: null,
      rate: 0,
      inFlight: 0,
      perTier: plan.perTier.map((t) => ({ tier: t.tier, total: t.requests, done: 0 })),
      origin: p.origin,
      createdAt: now,
      updatedAt: now,
      openEnd: p.to === undefined,
      failed: 0,
    };
    this.#jobs.set(r.id, r);
    this.#setStatus(r, 'queued');
    this.#run(r);
    return { jobId: r.id, plan };
  }

  async pause(id: string): Promise<void> {
    const r = this.#jobs.get(id);
    if (!r || !MOVING.has(r.status)) return;
    await this.#stop(r);
    this.#setStatus(r, 'paused');
  }

  resume(id: string): void {
    const r = this.#jobs.get(id);
    if (!r || MOVING.has(r.status) || r.status === 'done') return;
    this.#setStatus(r, 'queued');
    this.#run(r);
  }

  async cancel(id: string): Promise<void> {
    const r = this.#jobs.get(id);
    if (!r) return;
    await this.#stop(r);
    this.#jobs.delete(id);
    const timer = this.#saveTimers.get(id);
    if (timer) clearTimeout(timer);
    this.#saveTimers.delete(id);
    await this.#saving.get(id)?.catch(() => {});
    await fsp.rm(path.join(this.#dir, `${id}.json`), { force: true }).catch(() => {});
    // Progress bars following the job (chat cards, the chart's toolbar) learn it is gone.
    this.#d.emit('download.removed', { jobId: id });
  }

  /** Stops a run and purges its queued requests; the record keeps its status. */
  async #stop(r: JobRecord): Promise<void> {
    const run = this.#runs.get(r.id);
    if (!run) return;
    run.controller.abort();
    this.#d.fetcher.cancelJob(r.id);
    await run.promise.catch(() => {});
  }

  /** Stops every run without touching statuses (they resume next start) and saves every record. */
  async close(): Promise<void> {
    this.#closing = true;
    for (const unsubscribe of this.#unsubscribe.splice(0)) unsubscribe();
    if (this.#pauseTimer) clearTimeout(this.#pauseTimer);
    const runs = [...this.#runs.values()];
    for (const run of runs) run.controller.abort();
    // A TradingView page may take seconds to answer; the app kills the sidecar 2 s after asking it
    // to stop, and the stores still have to be flushed after this.
    let timer: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([
      Promise.allSettled(runs.map((run) => run.promise)),
      new Promise((resolve) => (timer = setTimeout(resolve, 800))),
    ]);
    clearTimeout(timer);
    for (const r of this.#jobs.values()) this.#save(r, true);
    await Promise.allSettled([...this.#saving.values()]);
  }

  // -------------------------------------------------------------------------------------------
  // Running

  #run(r: JobRecord): void {
    if (this.#runs.has(r.id) || this.#closing) return;
    const controller = new AbortController();
    const run: Run = {
      controller,
      promise: Promise.resolve(),
      inFlight: 0,
      completions: [],
      plans: new Map(),
      requesting: new Map(),
      planned: false,
    };
    this.#runs.set(r.id, run);
    run.promise = (async () => {
      try {
        if (r.source === 'dukascopy') await this.#runDukascopy(r, run);
        else await this.#runTradingView(r, run);
      } catch (error) {
        if (!controller.signal.aborted) {
          this.#log('error', `Download ${r.id} failed: ${(error as Error).message}`);
          this.#setStatus(r, 'error', (error as Error).message);
        }
      } finally {
        if (r.status !== 'done') this.#keepPending(r, run);
        this.#runs.delete(r.id);
      }
    })();
  }

  #keepPending(r: JobRecord, run: Run): void {
    if (r.source !== 'dukascopy' || !run.planned || r.status === 'done') return;
    const pending: Partial<Record<Tier, Range[]>> = {};
    for (const [tier, plan] of run.plans) pending[tier] = toRuns(tier, plan.buckets);
    r.pending = pending;
  }

  #applyFetcherState(): void {
    if (this.#closing) return;
    const status = this.#d.fetcher.status();
    const now = this.#nowMs();
    let message: string | null = null;
    if (status.breaker.state !== 'closed') message = status.breaker.message ?? 'Dukascopy is unreachable';
    else if (status.pausedUntil !== null && status.pausedUntil > now) {
      message = `Dukascopy is throttling; retrying in ${Math.ceil((status.pausedUntil - now) / 1000)} s`;
      if (this.#pauseTimer) clearTimeout(this.#pauseTimer);
      this.#pauseTimer = setTimeout(() => this.#applyFetcherState(), status.pausedUntil - now + 50);
      this.#pauseTimer.unref?.();
    }
    for (const id of this.#runs.keys()) {
      const r = this.#jobs.get(id);
      if (!r || r.source !== 'dukascopy' || (r.status !== 'running' && r.status !== 'waiting')) continue;
      if (message && (r.status !== 'waiting' || r.message !== message)) this.#setStatus(r, 'waiting', message);
      else if (!message && r.status === 'waiting') this.#setStatus(r, 'running');
    }
  }

  #begin(r: JobRecord): void {
    this.#setStatus(r, 'running');
    this.#applyFetcherState();
  }

  #finish(r: JobRecord, run: Run): void {
    if (run.controller.signal.aborted) return;
    if (r.failed > 0) {
      this.#setStatus(r, 'error', r.message ?? `${r.failed} file(s) could not be downloaded`);
      return;
    }
    if (r.source === 'tradingview') {
      // Pages were estimates: the bar ends at what it took.
      for (const t of r.perTier) t.total = t.done;
      r.total = r.done;
    } else {
      // A planned bucket someone covered after the walk passed it counts now.
      const now = this.#now();
      for (const [tier, plan] of run.plans) {
        for (const b of [...plan.buckets]) {
          if (!this.#d.store.needsFetch(r.key, tier, b, now)) this.#settle(r, run, tier, b, 'disk');
        }
      }
      const open = [...run.plans.values()].reduce((n, plan) => n + plan.buckets.size, 0);
      if (open > 0 || r.total > r.done) {
        // Every planned bucket is fetched, answered, known empty or skipped by now; anything else is
        // work left undone, and says so (Try again re-plans) instead of passing as done or skipped.
        const n = Math.max(open, r.total - r.done);
        this.#log('error', `Download ${r.id}: ${n} planned file(s) were left without an answer`);
        this.#setStatus(r, 'error', `${n} file(s) were not downloaded`);
        return;
      }
    }
    delete r.pending;
    this.#setStatus(r, 'done');
  }

  // -------------------------------------------------------------------------------------------
  // Accounting

  #entry(r: JobRecord, tier: string): { tier: string; total: number; done: number } {
    let t = r.perTier.find((x) => x.tier === tier);
    if (!t) {
      t = { tier, total: 0, done: 0 };
      r.perTier.push(t);
    }
    return t;
  }

  #count(r: JobRecord, run: Run, tier: string, bytes = 0): void {
    const t = this.#entry(r, tier);
    t.done += 1;
    if (t.done > t.total) t.total = t.done;
    r.done += 1;
    if (r.done > r.total) r.total = r.done;
    r.bytes += bytes;
    run.completions.push(this.#nowMs());
    this.#progress(r);
  }

  /** Work the plan did not know of (a bucket that became missing, a coarser bucket a check needs). */
  #grow(r: JobRecord, tier: string): void {
    this.#entry(r, tier).total += 1;
    r.total += 1;
  }

  /** Planned buckets the source has nothing for: out of `total`, into `skipped`. */
  #skip(r: JobRecord, tier: string | null, n: number): void {
    if (n <= 0) return;
    if (tier !== null) {
      const t = this.#entry(r, tier);
      t.total = Math.max(t.done, t.total - n);
    }
    r.total = Math.max(r.done, r.total - n);
    r.skipped += n;
    this.#progress(r);
  }

  #plan(run: Run, tier: Tier): TierPlan {
    let plan = run.plans.get(tier);
    if (!plan) {
      plan = { lo: 0, hi: 0, buckets: new Set() };
      run.plans.set(tier, plan);
    }
    return plan;
  }

  #hold(run: Run, tier: Tier, start: number): void {
    const key = `${tier}/${start}`;
    run.requesting.set(key, (run.requesting.get(key) ?? 0) + 1);
  }

  #release(run: Run, tier: Tier, start: number): void {
    const key = `${tier}/${start}`;
    const n = (run.requesting.get(key) ?? 1) - 1;
    if (n > 0) run.requesting.set(key, n);
    else run.requesting.delete(key);
  }

  /** A planned bucket is covered or answered: it counts once for the run's job. `disk` takes its bytes
   *  from the stored file (someone else fetched it). */
  #settle(r: JobRecord, run: Run, tier: Tier, start: number, bytes: number | 'disk'): boolean {
    if (this.#runs.get(r.id) !== run || !run.plans.get(tier)?.buckets.delete(start)) return false;
    this.#count(r, run, tier, bytes === 'disk' ? 0 : bytes);
    if (bytes === 'disk') {
      void this.#storedBytes(r.key, tier, start).then((n) => {
        if (n <= 0 || this.#jobs.get(r.id) !== r) return;
        r.bytes += n;
        this.#progress(r);
      });
    }
    return true;
  }

  /** Size of a stored bucket (the store's layout: <instrument>/<tier>/<yyyy>/<key>.json.gz); an empty
   *  bucket has no file and no bytes. */
  async #storedBytes(instrument: string, tier: Tier, start: number): Promise<number> {
    const key = bucketKey(tier, start);
    const file = path.join(this.#d.store.root, instrument, tier, key.slice(0, 4), `${key}.json.gz`);
    return fsp.stat(file).then(
      (st) => st.size,
      () => 0,
    );
  }

  /** Something stored or answered a bucket: a planned one of a running job counts for it, whoever
   *  fetched it (a boost, a chart, another job, an inference). */
  #stored(c: StoreChange): void {
    if (this.#closing) return;
    let now: number | null = null;
    for (const [id, run] of this.#runs) {
      const plan = run.plans.get(c.tier);
      if (!plan || plan.buckets.size === 0) continue;
      const r = this.#jobs.get(id);
      if (!r || r.key !== c.instrument) continue;
      // A put names one bucket; a recheck may name years of them.
      const wide = (c.to - c.from) / MIN_BUCKET_SECONDS[c.tier] > plan.buckets.size;
      const starts = wide
        ? [...plan.buckets].filter((b) => b >= c.from && b < c.to)
        : bucketsIn(c.tier, c.from, c.to).filter((b) => plan.buckets.has(b));
      for (const start of starts) {
        // The run's own requests count their buckets when they return, with the bytes fetched.
        if (run.requesting.has(`${c.tier}/${start}`)) continue;
        now ??= this.#now();
        if (this.#d.store.needsFetch(r.key, c.tier, start, now)) continue;
        this.#settle(r, run, c.tier, start, 'disk');
      }
    }
  }

  // -------------------------------------------------------------------------------------------
  // Dukascopy

  /** The buckets of one tier a Dukascopy job still has to fetch, per the manifest, and (m1) the missing
   *  weekend days h1 already shows empty. */
  async #remaining(r: JobRecord, tier: Tier): Promise<TierPlan & { inferred: Set<number> }> {
    const { store, planner } = this.#d;
    const eff = store.effectiveStart(r.key, tier);
    const now = this.#now();
    const lo = Math.max(r.from, eff ?? Infinity);
    const hi = Math.min(r.openEnd ? now : r.to, now);
    const inferred = new Set<number>();
    if (eff === null || !(hi > lo)) return { lo, hi, buckets: new Set(), inferred };
    let missing = store.missingBuckets(r.key, tier, lo, hi);
    if (tier === 'm1') {
      for (const b of await planner.inferableDays(r.key, missing)) inferred.add(b);
      missing = missing.filter((b) => !inferred.has(b));
    }
    const buckets = new Set(missing);
    // A provisional copy due for its one re-fetch (a day stored while still open) is work too. The
    // active bucket is not: reads keep it fresh, and it cannot be final yet.
    const first = bucketStart(tier, lo);
    for (const b of store.dueBuckets(r.key, tier, now)) {
      if (b >= first && b < hi && !isActive(tier, b, now)) buckets.add(b);
    }
    return { lo, hi, buckets, inferred };
  }

  async #runDukascopy(r: JobRecord, run: Run): Promise<void> {
    const signal = run.controller.signal;
    r.failed = 0;
    delete r.message;
    // Coarse to fine: each tier is the coarser one the next one's empty-streak check reads, and h1
    // makes m1 weekends inferable.
    const tiers = (['d1', 'h1', 'm1'] as const).filter((t) => r.tiers.includes(t));
    const saved = r.pending;
    for (const tier of tiers) {
      if (saved) {
        // A resume: the plan as it was when the job stopped. The re-plans below count what was covered
        // meanwhile as done and add what became missing.
        run.plans.set(tier, { lo: 0, hi: 0, buckets: fromRuns(tier, saved[tier]) });
        continue;
      }
      const { lo, hi, buckets } = await this.#remaining(r, tier);
      run.plans.set(tier, { lo, hi, buckets });
    }
    if (signal.aborted) return;
    run.planned = true;
    // Totals: what this job did plus what it still has to do.
    const perTier = TIERS.filter((t) => r.tiers.includes(t) || r.perTier.some((e) => e.tier === t)).map((tier) => {
      const done = r.perTier.find((t) => t.tier === tier)?.done ?? 0;
      return { tier, done, total: done + (run.plans.get(tier)?.buckets.size ?? 0) };
    });
    r.perTier = perTier;
    r.total = perTier.reduce((s, t) => s + t.total, 0);
    r.done = perTier.reduce((s, t) => s + t.done, 0);
    this.#begin(r);
    if (saved) {
      for (const tier of tiers) {
        await this.#replan(r, run, tier);
        if (signal.aborted) return;
      }
    }
    for (const tier of tiers) {
      await this.#replan(r, run, tier);
      if (signal.aborted) return;
      await this.#walk(r, run, tier, run.plans.get(tier)!);
      if (signal.aborted) return;
    }
    this.#finish(r, run);
  }

  /** Right before a tier's walk: the tiers just walked may make m1 weekends inferable, the range may
   *  have grown (an open end), and buckets may have been covered meanwhile (they count). */
  async #replan(r: JobRecord, run: Run, tier: Tier): Promise<void> {
    const { store, planner } = this.#d;
    const fresh = await this.#remaining(r, tier);
    if (run.controller.signal.aborted) return;
    const plan = this.#plan(run, tier);
    plan.lo = fresh.lo;
    plan.hi = fresh.hi;
    for (const b of [...plan.buckets]) {
      if (fresh.buckets.has(b)) continue;
      if (fresh.inferred.has(b)) {
        // Known empty from h1 without asking: answered, with no bytes (recorded below).
        this.#settle(r, run, tier, b, 0);
      } else if (store.status(r.key, tier, b) !== 'missing') {
        this.#settle(r, run, tier, b, 'disk');
      } else {
        // Below the tier's start now (another job learned it): nothing to fetch there.
        plan.buckets.delete(b);
        this.#skip(r, tier, 1);
      }
    }
    for (const b of fresh.buckets) {
      if (plan.buckets.has(b)) continue;
      plan.buckets.add(b);
      this.#grow(r, tier);
    }
    if (fresh.inferred.size) await planner.recordInferred(r.key, fresh.inferred);
  }

  #emptyAt(instrument: string, tier: Tier, start: number): boolean {
    return this.#d.store.sets(instrument, tier).empty.contains(start);
  }

  /** One request of this job (bulk lane); its result counts the bucket, with the bytes fetched. */
  async #fetch(r: JobRecord, run: Run, tier: Tier, start: number, signal: AbortSignal): Promise<Outcome> {
    this.#hold(run, tier, start);
    try {
      const result: FetchResult = await this.#d.fetcher.request({
        instrument: r.key,
        tier,
        start,
        lane: 'bulk',
        jobId: r.id,
        signal,
      });
      if (result.status === 'cancelled') return 'aborted';
      if (result.status === 'unavailable') {
        this.#settle(r, run, tier, start, 0);
        return 'skip';
      }
      this.#settle(r, run, tier, start, result.status === 'fetched' ? result.bytes : 'disk');
      if (result.status === 'fetched') return result.empty ? 'empty' : 'data';
      return this.#emptyAt(r.key, tier, start) ? 'empty' : 'data';
    } catch (error) {
      if (signal.aborted) return 'aborted';
      r.failed += 1;
      r.message = `${r.failed} file(s) could not be downloaded: ${(error as Error).message}`;
      this.#log('warn', `Download ${r.id}: ${(error as Error).message}`);
      this.#settle(r, run, tier, start, 0);
      return 'skip';
    } finally {
      this.#release(run, tier, start);
    }
  }

  /** Whether a tier holds bars before t, per its stored buckets (the one holding t is read). */
  async #dataBefore(instrument: string, tier: Tier, t: number): Promise<boolean> {
    const data = this.#d.store.sets(instrument, tier).data;
    const own = bucketStart(tier, t);
    if (!data.clip(-FAR, own).isEmpty) return true;
    if (own >= t || !data.contains(own)) return false;
    return (await this.#d.store.readNative(instrument, tier, own, t)).length > 0;
  }

  /** What the coarser tier says about an empty streak whose oldest bucket starts at `t`. 'start' needs
   *  the coarser tier known empty from `t` all the way down to its own start: its buckets are walked
   *  back from the one holding `t`, fetching the ones nobody has, until one shows older data (a hole).
   *  A tier that was never fetched shows no data, which is no evidence of a start; a coarser bucket
   *  that cannot be read, or no known coarser start, makes the verdict 'unknown'. */
  async #coarserVerdict(r: JobRecord, run: Run, tier: Tier, t: number): Promise<Verdict> {
    const { store } = this.#d;
    const coarser = coarserOf(tier);
    const signal = run.controller.signal;
    if (await this.#dataBefore(r.key, coarser, t)) return 'hole';
    const eff = store.effectiveStart(r.key, coarser);
    if (eff === null) return 'unknown';
    const floor = bucketStart(coarser, eff);
    let b = bucketStart(coarser, t);
    while (b >= floor) {
      // A window at a time, newest first: usually the coarser tier's data (or start) is close.
      const batch: number[] = [];
      for (; batch.length < WINDOW && b >= floor; b = previousBucket(coarser, b)) batch.push(b);
      const plan = this.#plan(run, coarser);
      const fetches = batch
        .filter((x) => store.status(r.key, coarser, x) === 'missing')
        .map((x) => {
          if (!plan.buckets.has(x)) {
            plan.buckets.add(x);
            this.#grow(r, coarser);
          }
          return this.#fetch(r, run, coarser, x, signal);
        });
      await Promise.all(fetches);
      if (signal.aborted) return 'unknown';
      for (const x of batch) {
        const status = store.status(r.key, coarser, x);
        if (status === 'missing' || status === 'unavailable') return 'unknown';
      }
      if (await this.#dataBefore(r.key, coarser, t)) return 'hole';
    }
    return 'start';
  }

  /** Walks one tier newest first. See the header for the empty-streak rules. */
  async #walk(r: JobRecord, run: Run, tier: Tier, plan: TierPlan): Promise<void> {
    const { store, planner } = this.#d;
    const signal = run.controller.signal;
    if (!(plan.hi > plan.lo)) return;
    const first = bucketStart(tier, plan.lo);
    const eff = store.effectiveStart(r.key, tier);
    // Only a walk down to the tier's own start (not just the range's) may conclude where it starts.
    const toTierStart = eff !== null && first <= bucketStart(tier, eff);
    let next = bucketStart(tier, plan.hi - 1);
    let pending: Pending[] = [];
    let stopped = false;
    // Streak state, in walk order.
    let armed = false;
    let emptyRun = 0;
    let runOldest: number | null = null;
    let runNewest: number | null = null;
    let lastData: number | null = null;
    // Sparse walk: below `sparseFrom` only probes are asked for. `quietDownTo` marks a known hole being
    // walked densely again: its empty buckets start no streak.
    let sparse = false;
    let sparseFrom = first;
    let evidence: Verdict = 'unknown';
    let quietDownTo: number | null = null;
    const limit = STREAK[tier];

    const dispatch = (start: number): Pending => {
      const now = this.#now();
      const status = store.status(r.key, tier, start);
      const covered =
        status === 'final' ||
        (status === 'provisional' && (isActive(tier, start, now) || !store.needsFetch(r.key, tier, start, now)));
      if (covered) {
        // Fetched meanwhile (another job, a chart, a boost): it counts for this job too.
        this.#settle(r, run, tier, start, 'disk');
        const outcome: Outcome = this.#emptyAt(r.key, tier, start) ? 'empty' : 'data';
        return { start, requested: false, controller: null, outcome: Promise.resolve(outcome) };
      }
      if (status === 'unavailable') {
        this.#settle(r, run, tier, start, 0);
        return { start, requested: false, controller: null, outcome: Promise.resolve('skip') };
      }
      // Missing, or a provisional copy due for its one re-fetch.
      if (!plan.buckets.has(start)) {
        // It became missing after planning: this job's to fetch all the same.
        plan.buckets.add(start);
        this.#grow(r, tier);
      }
      const controller = new AbortController();
      const onAbort = () => controller.abort();
      signal.addEventListener('abort', onAbort, { once: true });
      run.inFlight += 1;
      const outcome = (async (): Promise<Outcome> => {
        try {
          if (tier === 'm1' && status === 'missing' && !isWeekday(start)) {
            const inferred = await planner.inferableDays(r.key, [start]);
            if (inferred.has(start)) {
              // Known empty without a request: answered, with no bytes, whoever infers it first.
              this.#settle(r, run, tier, start, 0);
              await planner.recordInferred(r.key, [start]);
              return 'empty';
            }
          }
          if (controller.signal.aborted) return 'aborted';
          return await this.#fetch(r, run, tier, start, controller.signal);
        } finally {
          run.inFlight -= 1;
          signal.removeEventListener('abort', onAbort);
        }
      })();
      return { start, requested: true, controller, outcome };
    };

    const dropPending = () => {
      for (const p of pending) p.controller?.abort();
      pending = [];
    };

    const coarserFirst = () => store.sets(r.key, coarserOf(tier)).data.first();

    const learn = (reason: string) => {
      stopped = true;
      if (lastData === null) return;
      store.setLearnedStart(r.key, tier, {
        t: lastData,
        evidence: {
          reason,
          emptyFrom: runOldest,
          emptyTo: runNewest === null ? null : nextBucket(tier, runNewest),
          emptyBuckets: emptyRun,
          coarser: coarserOf(tier),
          coarserFirst: coarserFirst(),
          job: r.id,
          at: this.#now(),
        },
      });
      this.#log('info', `Dukascopy ${r.key} ${tier}: no data before ${iso(lastData)}`);
      // Everything older is skipped: the source has nothing there.
      let n = 0;
      for (const b of [...plan.buckets]) {
        if (b >= lastData) continue;
        plan.buckets.delete(b);
        n += 1;
      }
      this.#skip(r, tier, n);
    };

    while (!signal.aborted) {
      while (!stopped && pending.length < WINDOW && next >= first && !signal.aborted) {
        const start = next;
        next = previousBucket(tier, start);
        if (sparse && !isProbe(tier, start)) continue;
        pending.push(dispatch(start));
      }
      const item = pending.shift();
      if (!item) {
        if (!sparse || stopped) break;
        // The probes reached the first bucket without finding data again.
        if (toTierStart && evidence === 'hole') {
          learn('no data after a hole');
          break;
        }
        // Nothing says the tier starts here (the range ends above its start, or the coarser tier could
        // not be read): what the probes passed over is fetched after all, as asked.
        sparse = false;
        quietDownTo = first;
        next = sparseFrom;
        continue;
      }
      const outcome = await item.outcome;
      if (outcome === 'aborted' || signal.aborted) continue;
      if (outcome === 'skip' || stopped) continue;
      if (outcome === 'data') {
        armed = true;
        lastData = lastData === null ? item.start : Math.min(lastData, item.start);
        emptyRun = 0;
        runOldest = null;
        runNewest = null;
        if (sparse) {
          // Data again: walk densely from just below the streak, over what the probes passed over,
          // through this bucket and on. The hole's empty buckets start no new streak.
          sparse = false;
          dropPending();
          next = sparseFrom;
          quietDownTo = item.start;
        }
        continue;
      }
      // An empty bucket.
      if (!armed || limit === undefined || sparse) continue;
      if (quietDownTo !== null && item.start >= quietDownTo) continue;
      if (tier === 'm1' && !isWeekday(item.start)) continue;
      emptyRun += 1;
      runOldest = item.start;
      runNewest ??= item.start;
      if (emptyRun < limit) continue;
      const verdict = await this.#coarserVerdict(r, run, tier, runOldest);
      if (signal.aborted) continue;
      if (verdict === 'start') {
        learn('empty streak');
        dropPending();
        continue;
      }
      // Older coarser data (a hole in this tier), or no way to tell: probe on.
      sparse = true;
      sparseFrom = previousBucket(tier, runOldest);
      evidence = verdict;
      emptyRun = 0;
    }
  }

  // -------------------------------------------------------------------------------------------
  // TradingView

  async #runTradingView(r: JobRecord, run: Run): Promise<void> {
    const signal = run.controller.signal;
    const session = this.#d.tvSession;
    if (!session?.loggedIn()) {
      this.#setStatus(r, 'error', 'Sign in to TradingView to continue this download.');
      return;
    }
    r.failed = 0;
    delete r.message;
    const tfs = TV_TIMEFRAMES.filter((tf) => r.tiers.includes(tf));
    this.#begin(r);
    for (const tf of tfs) {
      const cov = this.#d.tv.coverage(r.key, tf);
      const onPage = (_bars: number, seconds: number) => {
        this.#d.tv.notePage(seconds);
        this.#count(r, run, tf);
      };
      if (cov.reachedStart === null) {
        // Continue from the start of the newest stored run instead of paging through it again.
        const last = cov.intervals.last();
        const to = last === null ? undefined : (cov.intervals.rangeAt(last - 1)?.[0] ?? undefined);
        await session.pageHistory(r.key, tf, { to, signal, onPage });
      } else {
        await this.#fillHoles(r, tf, session, signal, onPage);
      }
      if (signal.aborted) return;
      const entry = r.perTier.find((t) => t.tier === tf);
      if (entry) entry.total = entry.done;
      r.total = r.perTier.reduce((s, t) => s + t.total, 0);
      this.#progress(r);
    }
    this.#finish(r, run);
  }

  /** Pages the holes of a timeframe that reached TradingView's start, newest first: from the run
   *  above each hole back until the pages meet the run below. Each hole gets one walk. */
  async #fillHoles(
    r: JobRecord,
    tf: Timeframe,
    session: TvSession,
    signal: AbortSignal,
    onPage: (bars: number, seconds: number) => void,
  ): Promise<void> {
    let ceiling = Infinity;
    let last: Range | null = null;
    while (!signal.aborted) {
      const hole = tvHoles(this.#d.series, this.#d.tv, r.key, tf)
        .filter(([, to]) => to <= ceiling)
        .at(-1);
      if (!hole) return;
      if (last && hole[0] === last[0] && hole[1] === last[1]) {
        // TradingView served nothing before the hole's top (a first page with bars would have closed
        // at least its upper part): it no longer has that time (intraday history is kept for a
        // limited window). Recorded as covered with no bars, like an empty Dukascopy bucket, so plans
        // stop counting pages for it and no download repeats the walk.
        this.#log('info', `TradingView ${r.key} ${tf}: nothing served from ${iso(hole[0])} to ${iso(hole[1])}`);
        this.#d.tv.cover(r.key, tf, hole[0], hole[1]);
        ceiling = hole[0];
        continue;
      }
      last = hole;
      await session.pageHistory(r.key, tf, { to: hole[1], stopAt: hole[0], signal, onPage });
    }
  }

  // -------------------------------------------------------------------------------------------
  // Waiting for a range

  /** Resolves when [from, to] is covered for tf, the job stops (paused, error, removed, done) or at
   *  the timeout. Meanwhile (unless `boost` is false) what the range needs for tf rides the `waiting`
   *  lane. */
  async wait(p: WaitParams): Promise<{ covered: boolean; status: JobStatus | null }> {
    // Real time, not the injected clock: the timeout is a promise to the caller.
    const deadline = Date.now() + Math.max(0, p.timeoutMs);
    const boost = new AbortController();
    let boostedAt = -Infinity;
    try {
      for (;;) {
        const now = this.#now();
        // `to` is exclusive here; series.missing takes the last second included.
        const missing = this.#d.series.missing(p.symbol, p.tf, p.from, Math.min(p.to, now) - 1);
        const job = p.jobId ? (this.#jobs.get(p.jobId) ?? null) : null;
        const status = job?.status ?? null;
        if (missing.length === 0) return { covered: true, status };
        if (p.jobId && (!job || status === 'paused' || status === 'error' || status === 'done')) {
          return { covered: false, status };
        }
        if (Date.now() >= deadline || this.#closing) return { covered: false, status };
        // Re-boost now and then: an empty coarse bucket reveals finer ones to ask for.
        if (p.boost !== false && Date.now() - boostedAt > 5000) {
          boostedAt = Date.now();
          void this.#boost(p.symbol, p.tf, missing, boost.signal, p.jobId);
        }
        await new Promise((resolve) => setTimeout(resolve, Math.min(500, Math.max(10, deadline - Date.now()))));
      }
    } finally {
      boost.abort();
    }
  }

  /** Asks, on the waiting lane, for what reading `missing` at tf needs: only tiers eligible for tf (the
   *  planner's interactive needs). The requests name the job waited on, and a bucket in its plan counts
   *  for it with the bytes fetched. */
  async #boost(
    symbol: string,
    tf: Timeframe,
    missing: Array<[number, number]>,
    signal: AbortSignal,
    jobId?: string,
  ): Promise<void> {
    const instrument = this.#d.series.route(symbol).instrument;
    if (!instrument || signal.aborted) return;
    const from = missing[0]![0];
    const to = missing[missing.length - 1]![1];
    try {
      const needs = await this.#d.planner.interactive(symbol, tf, from, to, 500);
      const r = jobId ? this.#jobs.get(jobId) : undefined;
      for (const need of needs) {
        if (signal.aborted) return;
        const run = r && r.source === 'dukascopy' && r.key === instrument ? this.#runs.get(r.id) : undefined;
        const mine = run !== undefined && run.plans.get(need.tier)?.buckets.has(need.start) === true;
        if (mine) this.#hold(run, need.tier, need.start);
        this.#d.fetcher
          .request({ instrument, tier: need.tier, start: need.start, lane: 'waiting', jobId, signal })
          .then(
            (result) => {
              if (!mine) return;
              if (result.status === 'fetched') this.#settle(r!, run, need.tier, need.start, result.bytes);
              else if (result.status === 'unavailable') this.#settle(r!, run, need.tier, need.start, 0);
              else if (result.status === 'cached') this.#settle(r!, run, need.tier, need.start, 'disk');
            },
            () => undefined,
          )
          .finally(() => {
            if (mine) this.#release(run, need.tier, need.start);
          });
      }
    } catch (error) {
      this.#log('warn', `Could not prioritise ${symbol}: ${(error as Error).message}`);
    }
  }
}

/** Old downloads.ts jobs worth continuing, as new paused jobs (the migration). */
export function convertedJob(
  old: { symbol?: unknown; instrument?: unknown; from?: unknown; to?: unknown; source?: unknown },
  route: { source: Source; key: string; name: string },
  now: number,
): JobRecord | null {
  const from = typeof old.from === 'number' ? old.from : null;
  const to = typeof old.to === 'number' ? old.to : null;
  if (from === null || to === null || !(to > from)) return null;
  const tiers = route.source === 'dukascopy' ? [...TIERS] : [...TV_TIMEFRAMES];
  return {
    version: 1,
    id: `j${now.toString(36)}${Math.random().toString(36).slice(2, 7)}`,
    source: route.source,
    key: route.key,
    symbol: typeof old.symbol === 'string' ? old.symbol : route.key,
    name: route.name,
    from,
    to,
    tiers,
    status: 'paused',
    total: 0,
    done: 0,
    skipped: 0,
    bytes: 0,
    etaSeconds: null,
    rate: 0,
    inFlight: 0,
    perTier: tiers.map((tier) => ({ tier, total: 0, done: 0 })),
    origin: 'chart',
    createdAt: now,
    updatedAt: now,
    message: 'Continued from an earlier download',
    openEnd: false,
    failed: 0,
  };
}
