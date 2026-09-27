// One global queue for every request to Dukascopy's candle API, so the whole app shares one polite
// request rate however many charts, tools and downloads want data at once.
//
// - Entries are keyed by (instrument, tier, bucketStart) over queued and in-flight requests; another
//   caller for the same bucket attaches as a waiter, so a bucket is requested once.
// - Lanes, highest first: 'interactive' (chart and tool reads), 'waiting' (buckets a tool waits on),
//   'bulk' (jobs, round-robin between job ids). An entry runs in the best lane any waiter asked for.
// - At dispatch the store is asked again (a bucket stored meanwhile, final or provisional and not due,
//   is dropped without a request) and the URL is built from the current time (active vs completed).
// - Rate: a token bucket whose rate follows AIMD. It starts at 3 req/s and gains 0.5 after 50
//   consecutive successes that kept the limiter busy, with at least 2 min since the last 429. A
//   429/403/503 halves it, and the whole queue pauses for Retry-After but at least 60 s. A 429 also
//   lowers the ceiling the rate may climb back to: to the highest rate sustained without one when it
//   came above that rate, else one step below the rate that failed (the limit moved). Ceiling and
//   safe rate persist in <cache>/dukascopy/stats.json; hard max 8. At most 4 requests in flight.
// - A failed request goes back into the queue with exponential backoff and jitter; a worker never
//   retries. After 8 consecutive failures, or at once when the network is unreachable, the circuit
//   breaker opens: the bulk lane pauses (interactive and waiting keep trying on their own backoff),
//   and after a backoff one bulk request probes. Any success closes it. Nothing fails during an
//   outage: entries wait.
// - 400/404 are permanent answers: the bucket is recorded `unavailable`. Except "From time is too
//   late" within an hour of the bucket's close: Dukascopy has not built it yet (it builds about a
//   second after the close, and clocks differ), so it is retried with backoff, not counted as failure.
// - A 200 is stored only when its `timestamp` lies in the requested bucket. `?from=` answers the
//   bucket containing Dukascopy's own now, whatever `from` says: when that is a later bucket, ours
//   closed while the request travelled, and its completed URL is asked once the close grace passed.
// - Finality: a completed bucket is final when its build time (Date header; without one, receive
//   time minus Age) is at least an hour after the bucket closed; otherwise it is stored provisional.
//   Active buckets (`?from=` URL) are always provisional.
//
// API (class Fetcher, one per process: `new Fetcher(store, opts?)`):
//   request({instrument, tier, start, lane, jobId?, signal?, failFast?, activeMaxAge?}) -> Promise<FetchResult>
//       {status: 'fetched', final, empty, bars, bytes} | {status: 'cached'} (the store had it at
//       dispatch) | {status: 'unavailable', http} | {status: 'cancelled'} (signal, cancelJob, close).
//       Rejects with FetchError only for permanent failures (unknown instrument, repeatedly invalid
//       responses, store write errors) or, with `failFast`, as soon as it would have to wait out an
//       open breaker, a 429 pause or a retry. Aborting `signal` removes only that waiter; to boost a
//       range into the waiting lane, request it again with lane 'waiting' and a signal aborted when
//       the wait ends.
//   cancelJob(jobId) -> waiters cancelled (their entries leave the queue unless someone else waits)
//   isPending(instrument, tier, start), queuedFor(jobId) (entries a job waits on),
//   backlog(lane) (entries served before or alongside that lane, for estimates)
//   status() -> FetcherStatus: rate, safeRate, ceiling, inFlight, queued per lane, bytesPerSecond,
//       pausedUntil (ms or null), breaker;  breaker -> BreakerState {state, reason, message, retryAt}
//   effectiveRate() requests/s to plan with;  tierStats(tier) rolling {secondsPerRequest, bytesPerBucket}
//   subscribe(listener) -> unsubscribe: {type: 'breaker', breaker} | {type: 'paused', until} | {type: 'rate', rate}
//   flush() writes stats.json;  close(graceMs?) stops, lets in-flight requests finish briefly, flushes
// Options: fetch and clock (tests), random (jitter), log, statsDelayMs.

import fs from 'node:fs';
import path from 'node:path';

import {
  TIERS,
  type Tier,
  activeUrl,
  bucketKey,
  bucketStart,
  completedUrl,
  instrumentCode,
  isActive,
  isFinalBuild,
  nextBucket,
} from './buckets.ts';
import { DecodeError, parseResponse } from './decode.ts';
import { CLOSE_GRACE_SECONDS, type DukascopyStore, type PutResult, writeAtomic } from './dukascopy-store.ts';

export type Lane = 'interactive' | 'waiting' | 'bulk';
export const LANES: readonly Lane[] = ['interactive', 'waiting', 'bulk'];
const PRIORITY: Record<Lane, number> = { interactive: 0, waiting: 1, bulk: 2 };

export const START_RATE = 3;
export const MAX_RATE = 8;
export const MIN_RATE = 0.25;
export const RATE_STEP = 0.5;
export const SUCCESSES_PER_STEP = 50;
/** Time since the last 429 before the rate may grow. */
export const CALM_MS = 120_000;
/** A success streak counts as sustained when it ran at no less than this share of the rate. */
const SUSTAINED_SHARE = 0.75;
export const MAX_IN_FLIGHT = 4;
export const THROTTLE_PAUSE_MS = 60_000;
export const BREAKER_FAILURES = 8;
export const REQUEST_TIMEOUT_MS = 30_000;
/** Attempts before a bucket whose answers cannot be stored fails for good. */
export const MAX_INVALID_ATTEMPTS = 5;
/** How long after a bucket's close "From time is too late" may still mean "not built yet". */
export const NOT_BUILT_SECONDS = 3600;
const TOO_LATE = /from time is too late/i;
const BACKOFF_BASE_MS = 1000;
const BACKOFF_MAX_MS = 5 * 60_000;
// First and longest breaker wait per reason; each failed probe doubles it.
const BREAKER_WAIT_MS = { throttled: [5 * 60_000, 30 * 60_000], offline: [30_000, 5 * 60_000] } as const;
const BYTES_WINDOW_MS = 10_000;
const EWMA_ALPHA = 0.05;
const STATS_VERSION = 1;
const KB = 1024;
/** Rolling defaults until measured: m1 day ≈ 9 KB, h1 month ≈ 5 KB, d1 year ≈ 4 KB gzipped. */
const DEFAULT_BYTES: Record<Tier, number> = { m1: 9 * KB, h1: 5 * KB, d1: 4 * KB };
const DEFAULT_SECONDS = 0.3;
// Errors that mean this machine has no network, not that Dukascopy is unwell.
const OFFLINE_CODES = new Set(['ENOTFOUND', 'EAI_AGAIN', 'ENETUNREACH', 'ENETDOWN', 'EHOSTUNREACH']);

export type FetchResult =
  | { status: 'fetched'; final: boolean; empty: boolean; bars: number; bytes: number }
  | { status: 'cached' }
  | { status: 'unavailable'; http: number | null }
  | { status: 'cancelled' };

export type FetchErrorCode =
  | 'UNKNOWN_INSTRUMENT'
  | 'INVALID_RESPONSE'
  | 'STORE'
  | 'HTTP'
  | 'THROTTLED'
  | 'OFFLINE'
  | 'NETWORK'
  | 'TIMEOUT'
  | 'CLOSED';

export class FetchError extends Error {
  code: FetchErrorCode;

  constructor(code: FetchErrorCode, message: string) {
    super(message);
    this.code = code;
  }
}

export interface FetchRequest {
  instrument: string;
  tier: Tier;
  /** Any time inside the bucket; normalised to its start. */
  start: number;
  lane: Lane;
  /** Bulk entries are served round-robin by job id. */
  jobId?: string;
  signal?: AbortSignal;
  /** Reject instead of waiting through an outage or a retry. */
  failFast?: boolean;
  /** Seconds an active bucket's copy may age before it is fetched again (store default 60). */
  activeMaxAge?: number;
}

export interface BreakerState {
  state: 'closed' | 'open' | 'half-open';
  reason: 'throttled' | 'offline' | null;
  /** "Dukascopy is throttling; retrying in 5 min" | "Offline" (for job status lines). */
  message: string | null;
  /** When the next probe may run (ms), while not closed. */
  retryAt: number | null;
}

export interface TierStats {
  secondsPerRequest: number;
  bytesPerBucket: number;
  /** Requests measured / data buckets measured. */
  samples: number;
  buckets: number;
}

export interface FetcherStatus {
  rate: number;
  safeRate: number | null;
  ceiling: number;
  inFlight: number;
  queued: Record<Lane, number>;
  bytesPerSecond: number;
  pausedUntil: number | null;
  breaker: BreakerState;
}

export type FetcherEvent =
  { type: 'breaker'; breaker: BreakerState } | { type: 'paused'; until: number } | { type: 'rate'; rate: number };

export type FetcherListener = (event: FetcherEvent) => void;

export interface Clock {
  /** Milliseconds, like Date.now. */
  now(): number;
  setTimeout(fn: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
}

export type FetchLike = (
  url: string,
  init: { headers: Record<string, string>; signal: AbortSignal },
) => Promise<Response>;

export type FetcherStore = Pick<DukascopyStore, 'root' | 'status' | 'needsFetch' | 'put' | 'markUnavailable'>;

export interface FetcherOptions {
  /** Defaults to the global fetch (looked up per call, so main.ts's user-agent wrapper applies). */
  fetch?: FetchLike;
  clock?: Clock;
  /** 0 <= x < 1, for jitter. */
  random?: () => number;
  log?: (level: 'info' | 'warn' | 'error', message: string) => void;
  /** Debounce of stats.json writes. */
  statsDelayMs?: number;
}

const realClock: Clock = {
  now: () => Date.now(),
  setTimeout: (fn, ms) => {
    const handle = setTimeout(fn, ms);
    // The sidecar lives on stdin; a pending wake-up alone keeps nothing alive.
    handle.unref?.();
    return handle;
  },
  clearTimeout: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
};

const CLOSED: BreakerState = { state: 'closed', reason: null, message: null, retryAt: null };

interface Waiter {
  lane: Lane;
  jobId: string | null;
  failFast: boolean;
  activeMaxAge: number | undefined;
  resolve: (result: FetchResult) => void;
  reject: (error: Error) => void;
  signal: AbortSignal | undefined;
  onAbort: (() => void) | undefined;
}

interface Entry {
  key: string;
  instrument: string;
  tier: Tier;
  start: number;
  lane: Lane;
  /** The bulk job this entry is served for in the round-robin ('' for none). */
  owner: string;
  activeMaxAge: number | undefined;
  waiters: Set<Waiter>;
  seq: number;
  inFlight: boolean;
  notBefore: number;
  attempts: number;
  invalid: number;
  abort: AbortController | undefined;
}

/** `notBefore` (ms): a retry that must wait for a known moment, whatever the backoff says. */
type Verdict = { done: FetchResult } | { fail: Error } | { retry: FetchError; notBefore?: number };

/** A response's build time in seconds: the Date header, else receive time minus Age. */
export function buildTime(headers: Headers, receivedAtMs: number): number {
  const date = Date.parse(headers.get('date') ?? '');
  if (!Number.isNaN(date)) return Math.floor(date / 1000);
  const age = Number(headers.get('age'));
  return Math.floor(receivedAtMs / 1000) - (Number.isFinite(age) && age > 0 ? Math.floor(age) : 0);
}

/** Retry-After as a wait in ms (seconds or an HTTP date), or null when absent or unreadable. */
export function retryAfterMs(value: string | null, nowMs: number): number | null {
  const text = value?.trim();
  if (!text) return null;
  if (/^\d+$/.test(text)) return Number(text) * 1000;
  const at = Date.parse(text);
  return Number.isNaN(at) ? null : Math.max(0, at - nowMs);
}

function errorCode(error: unknown): string {
  let e: unknown = error;
  for (let depth = 0; depth < 4 && e && typeof e === 'object'; depth += 1) {
    const code = (e as { code?: unknown }).code;
    if (typeof code === 'string') return code;
    e = (e as { cause?: unknown }).cause;
  }
  return '';
}

const asError = (error: unknown) => (error instanceof Error ? error : new Error(String(error)));

export class Fetcher {
  readonly #store: FetcherStore;
  readonly #fetch: FetchLike;
  readonly #clock: Clock;
  readonly #random: () => number;
  readonly #log: (level: 'info' | 'warn' | 'error', message: string) => void;
  readonly #statsPath: string;
  readonly #statsDelayMs: number;

  readonly #entries = new Map<string, Entry>();
  readonly #listeners = new Set<FetcherListener>();
  readonly #running = new Set<Promise<void>>();
  #seq = 0;
  #inFlight = 0;
  #closed = false;
  #pumping = false;
  #timer: unknown = null;
  #timerAt = Infinity;
  #lastOwner = '';

  #rate = START_RATE;
  #safeRate: number | null = null;
  #ceiling = MAX_RATE;
  #tokens = 1;
  #refilledAt: number;
  #last429: number;
  #pausedUntil = 0;
  #successTimes: number[] = [];
  #failures = 0;

  #breaker: BreakerState = CLOSED;
  #breakerOpens = 0;
  #probing = false;

  #tiers: Record<Tier, TierStats>;
  #latency = DEFAULT_SECONDS;
  #latencySamples = 0;
  #bytesWindow: Array<[number, number]> = [];
  #statsTimer: unknown = null;
  #statsDirty = false;
  #statsWriting: Promise<void> | null = null;

  constructor(store: FetcherStore, opts: FetcherOptions = {}) {
    this.#store = store;
    this.#fetch = opts.fetch ?? ((url, init) => globalThis.fetch(url, init));
    this.#clock = opts.clock ?? realClock;
    this.#random = opts.random ?? Math.random;
    this.#log = opts.log ?? (() => {});
    this.#statsPath = path.join(store.root, 'stats.json');
    this.#statsDelayMs = opts.statsDelayMs ?? 10_000;
    this.#tiers = {
      m1: { secondsPerRequest: DEFAULT_SECONDS, bytesPerBucket: DEFAULT_BYTES.m1, samples: 0, buckets: 0 },
      h1: { secondsPerRequest: DEFAULT_SECONDS, bytesPerBucket: DEFAULT_BYTES.h1, samples: 0, buckets: 0 },
      d1: { secondsPerRequest: DEFAULT_SECONDS, bytesPerBucket: DEFAULT_BYTES.d1, samples: 0, buckets: 0 },
    };
    this.#loadStats();
    this.#rate = Math.min(START_RATE, this.#ceiling);
    const now = this.#clock.now();
    this.#refilledAt = now;
    // Nothing is known about the last 429 at start: the first increase waits the calm period.
    this.#last429 = now;
  }

  // -------------------------------------------------------------------------------------------
  // Public API

  request(req: FetchRequest): Promise<FetchResult> {
    if (this.#closed || req.signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    const blocked = req.failFast ? this.#blockedError() : null;
    if (blocked) return Promise.reject(blocked);
    const start = bucketStart(req.tier, req.start);
    const key = `${req.instrument}/${req.tier}/${start}`;
    return new Promise<FetchResult>((resolve, reject) => {
      let entry = this.#entries.get(key);
      if (!entry) {
        entry = {
          key,
          instrument: req.instrument,
          tier: req.tier,
          start,
          lane: req.lane,
          owner: '',
          activeMaxAge: undefined,
          waiters: new Set(),
          seq: this.#seq++,
          inFlight: false,
          notBefore: 0,
          attempts: 0,
          invalid: 0,
          abort: undefined,
        };
        this.#entries.set(key, entry);
      }
      const waiter: Waiter = {
        lane: req.lane,
        jobId: req.jobId ?? null,
        failFast: req.failFast ?? false,
        activeMaxAge: req.activeMaxAge,
        resolve,
        reject,
        signal: req.signal,
        onAbort: undefined,
      };
      entry.waiters.add(waiter);
      if (req.signal) {
        const target = entry;
        waiter.onAbort = () => {
          this.#settle(target, waiter, { status: 'cancelled' });
          this.#relane(target);
        };
        req.signal.addEventListener('abort', waiter.onAbort, { once: true });
      }
      this.#relane(entry);
      this.#pump();
    });
  }

  cancelJob(jobId: string): number {
    let cancelled = 0;
    for (const entry of [...this.#entries.values()]) {
      for (const waiter of [...entry.waiters]) {
        if (waiter.jobId !== jobId) continue;
        this.#settle(entry, waiter, { status: 'cancelled' });
        cancelled += 1;
      }
      this.#relane(entry);
    }
    return cancelled;
  }

  isPending(instrument: string, tier: Tier, start: number): boolean {
    return this.#entries.has(`${instrument}/${tier}/${bucketStart(tier, start)}`);
  }

  queuedFor(jobId: string): number {
    let n = 0;
    for (const entry of this.#entries.values()) if ([...entry.waiters].some((w) => w.jobId === jobId)) n += 1;
    return n;
  }

  backlog(lane: Lane): number {
    let n = 0;
    for (const entry of this.#entries.values()) if (PRIORITY[entry.lane] <= PRIORITY[lane]) n += 1;
    return n;
  }

  get breaker(): BreakerState {
    return { ...this.#breaker };
  }

  status(): FetcherStatus {
    const now = this.#clock.now();
    const queued: Record<Lane, number> = { interactive: 0, waiting: 0, bulk: 0 };
    for (const entry of this.#entries.values()) if (!entry.inFlight) queued[entry.lane] += 1;
    return {
      rate: this.#rate,
      safeRate: this.#safeRate,
      ceiling: this.#ceiling,
      inFlight: this.#inFlight,
      queued,
      bytesPerSecond: this.#bytesPerSecond(now),
      pausedUntil: this.#pausedUntil > now ? this.#pausedUntil : null,
      breaker: this.breaker,
    };
  }

  /** Requests per second to plan with: the rate, or fewer when latency with 4 in flight caps it. */
  effectiveRate(): number {
    return Math.min(this.#rate, MAX_IN_FLIGHT / Math.max(this.#latency, 0.05));
  }

  tierStats(tier: Tier): TierStats {
    return { ...this.#tiers[tier] };
  }

  subscribe(listener: FetcherListener): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  async flush(): Promise<void> {
    if (this.#statsTimer !== null) this.#clock.clearTimeout(this.#statsTimer);
    this.#statsTimer = null;
    if (this.#statsWriting) await this.#statsWriting.catch(() => {});
    if (this.#statsDirty) await this.#writeStats();
  }

  /** Stops the queue: queued waiters resolve 'cancelled', in-flight requests get `graceMs` to land. */
  async close(graceMs = 1000): Promise<void> {
    if (!this.#closed) {
      this.#closed = true;
      if (this.#timer !== null) this.#clock.clearTimeout(this.#timer);
      this.#timer = null;
      this.#timerAt = Infinity;
      for (const entry of [...this.#entries.values()]) {
        if (!entry.inFlight) this.#finish(entry, { status: 'cancelled' });
      }
      if (this.#running.size > 0) {
        let timer: ReturnType<typeof setTimeout> | undefined;
        const grace = new Promise<void>((resolve) => (timer = setTimeout(resolve, graceMs)));
        await Promise.race([Promise.allSettled([...this.#running]), grace]);
        clearTimeout(timer);
        for (const entry of this.#entries.values()) entry.abort?.abort(new FetchError('CLOSED', 'The fetcher closed'));
        await Promise.allSettled([...this.#running]);
      }
    }
    await this.flush();
  }

  // -------------------------------------------------------------------------------------------
  // Queue

  /** Recomputes an entry's lane from its waiters; an idle entry nobody waits for leaves. */
  #relane(entry: Entry): void {
    if (entry.waiters.size === 0) {
      if (!entry.inFlight && this.#entries.get(entry.key) === entry) this.#entries.delete(entry.key);
      return;
    }
    let lane: Lane = 'bulk';
    let owner: string | null = null;
    let maxAge: number | undefined;
    for (const w of entry.waiters) {
      if (PRIORITY[w.lane] < PRIORITY[lane]) lane = w.lane;
      if (owner === null && w.lane === 'bulk') owner = w.jobId ?? '';
      if (w.activeMaxAge !== undefined) maxAge = Math.min(maxAge ?? Infinity, w.activeMaxAge);
    }
    entry.lane = lane;
    entry.owner = owner ?? '';
    entry.activeMaxAge = maxAge;
  }

  #settle(entry: Entry, waiter: Waiter, outcome: FetchResult | Error): void {
    if (!entry.waiters.delete(waiter)) return;
    if (waiter.onAbort) waiter.signal?.removeEventListener('abort', waiter.onAbort);
    if (outcome instanceof Error) waiter.reject(outcome);
    else waiter.resolve(outcome);
  }

  #finish(entry: Entry, outcome: FetchResult | Error): void {
    if (this.#entries.get(entry.key) === entry) this.#entries.delete(entry.key);
    for (const waiter of [...entry.waiters]) this.#settle(entry, waiter, outcome);
  }

  #requeue(entry: Entry, error: FetchError, notBefore = 0): void {
    entry.attempts += 1;
    const now = this.#clock.now();
    const exp = Math.min(BACKOFF_MAX_MS, BACKOFF_BASE_MS * 2 ** (entry.attempts - 1));
    // Equal jitter: half the backoff is fixed, half random, so retries of a burst spread out.
    entry.notBefore = Math.max(now + exp / 2 + this.#random() * (exp / 2), this.#pausedUntil, notBefore);
    for (const waiter of [...entry.waiters]) if (waiter.failFast) this.#settle(entry, waiter, error);
    this.#relane(entry);
  }

  #pump(): void {
    if (this.#closed || this.#pumping) return;
    this.#pumping = true;
    try {
      while (this.#inFlight < MAX_IN_FLIGHT) {
        const now = this.#clock.now();
        if (now < this.#pausedUntil) {
          if (this.#entries.size > this.#inFlight) this.#wake(this.#pausedUntil);
          return;
        }
        this.#tickBreaker(now);
        const entry = this.#next(now);
        if (!entry) {
          this.#wakeForQueued(now);
          return;
        }
        // Dropping what the store already has costs no token.
        const early = this.#recheck(entry, now);
        if (early) {
          this.#finish(entry, early);
          continue;
        }
        this.#refill(now);
        if (this.#tokens < 1) {
          this.#wake(now + Math.ceil(((1 - this.#tokens) / this.#rate) * 1000));
          return;
        }
        this.#tokens -= 1;
        this.#start(entry, now);
      }
    } finally {
      this.#pumping = false;
    }
  }

  #bulkAllowed(): boolean {
    if (this.#breaker.state === 'closed') return true;
    return this.#breaker.state === 'half-open' && !this.#probing;
  }

  /** Interactive, then waiting, in arrival order; then bulk, round-robin between jobs. */
  #next(now: number): Entry | null {
    let best: Entry | null = null;
    const bulk = new Map<string, Entry>();
    const bulkAllowed = this.#bulkAllowed();
    for (const entry of this.#entries.values()) {
      if (entry.inFlight || entry.notBefore > now) continue;
      if (entry.lane === 'bulk') {
        if (!bulkAllowed) continue;
        const current = bulk.get(entry.owner);
        if (!current || entry.seq < current.seq) bulk.set(entry.owner, entry);
      } else if (
        !best ||
        PRIORITY[entry.lane] < PRIORITY[best.lane] ||
        (entry.lane === best.lane && entry.seq < best.seq)
      ) {
        best = entry;
      }
    }
    if (best || bulk.size === 0) return best;
    const owners = [...bulk.keys()].sort();
    return bulk.get(owners.find((o) => o > this.#lastOwner) ?? owners[0]!)!;
  }

  #wakeForQueued(now: number): void {
    let at = Infinity;
    const bulkAllowed = this.#bulkAllowed();
    for (const entry of this.#entries.values()) {
      if (entry.inFlight) continue;
      if (entry.lane === 'bulk' && !bulkAllowed) {
        // A probe in flight wakes the queue when it lands; an open breaker at its retry time.
        if (this.#breaker.state === 'open' && this.#breaker.retryAt !== null) {
          at = Math.min(at, Math.max(this.#breaker.retryAt, entry.notBefore));
        }
      } else if (entry.notBefore > now) {
        at = Math.min(at, entry.notBefore);
      }
    }
    if (at !== Infinity) this.#wake(at);
  }

  #wake(at: number): void {
    if (this.#closed || (this.#timer !== null && this.#timerAt <= at)) return;
    if (this.#timer !== null) this.#clock.clearTimeout(this.#timer);
    this.#timerAt = at;
    this.#timer = this.#clock.setTimeout(
      () => {
        this.#timer = null;
        this.#timerAt = Infinity;
        this.#pump();
      },
      Math.max(0, at - this.#clock.now()),
    );
  }

  #recheck(entry: Entry, now: number): FetchResult | Error | null {
    const { instrument, tier, start } = entry;
    try {
      if (instrumentCode(instrument) === null) {
        return new FetchError('UNKNOWN_INSTRUMENT', `Dukascopy has no instrument "${instrument}"`);
      }
      if (this.#store.status(instrument, tier, start) === 'unavailable') return { status: 'unavailable', http: null };
      const nowSec = Math.floor(now / 1000);
      const due = this.#store.needsFetch(instrument, tier, start, nowSec, { activeMaxAge: entry.activeMaxAge });
      return due ? null : { status: 'cached' };
    } catch (error) {
      return asError(error);
    }
  }

  #refill(now: number): void {
    this.#tokens = Math.min(1, this.#tokens + ((now - this.#refilledAt) / 1000) * this.#rate);
    this.#refilledAt = now;
  }

  #start(entry: Entry, now: number): void {
    entry.inFlight = true;
    this.#inFlight += 1;
    if (entry.lane === 'bulk') this.#lastOwner = entry.owner;
    const probe = entry.lane === 'bulk' && this.#breaker.state === 'half-open';
    if (probe) this.#probing = true;
    const run = this.#run(entry, now, probe).finally(() => this.#running.delete(run));
    this.#running.add(run);
  }

  // -------------------------------------------------------------------------------------------
  // One request. Never loops: a retry goes back into the queue.

  async #run(entry: Entry, startedAt: number, probe: boolean): Promise<void> {
    const { instrument, tier, start } = entry;
    const code = instrumentCode(instrument)!;
    const active = isActive(tier, start, Math.floor(startedAt / 1000));
    const url = active ? activeUrl(code, tier, start) : completedUrl(code, tier, start);
    const controller = new AbortController();
    entry.abort = controller;
    const timer = this.#clock.setTimeout(
      () => controller.abort(new FetchError('TIMEOUT', 'Dukascopy did not answer within 30 s')),
      REQUEST_TIMEOUT_MS,
    );
    let verdict: Verdict;
    try {
      verdict = await this.#attempt(entry, url, active, startedAt, controller.signal);
    } catch (error) {
      verdict = this.#closed ? { done: { status: 'cancelled' } } : this.#networkFailure(error);
    } finally {
      this.#clock.clearTimeout(timer);
      entry.abort = undefined;
    }
    entry.inFlight = false;
    this.#inFlight -= 1;
    if (probe) this.#probing = false;
    if ('done' in verdict) this.#finish(entry, verdict.done);
    else if ('fail' in verdict) this.#finish(entry, verdict.fail);
    else if (this.#closed) this.#finish(entry, { status: 'cancelled' });
    else this.#requeue(entry, verdict.retry, verdict.notBefore);
    this.#pump();
  }

  async #attempt(entry: Entry, url: string, active: boolean, startedAt: number, signal: AbortSignal): Promise<Verdict> {
    const res = await this.#fetch(url, { headers: { 'accept-encoding': 'gzip' }, signal });
    const body = Buffer.from(await res.arrayBuffer());
    const receivedAt = this.#clock.now();
    this.#countBytes(receivedAt, Number(res.headers.get('content-length')) || body.length);
    const { instrument, tier, start } = entry;
    if (res.status === 200) {
      let answered: number;
      try {
        answered = bucketStart(tier, Math.floor(parseResponse(body).timestamp / 1000));
      } catch (error) {
        return this.#invalid(entry, error);
      }
      if (answered !== start) return this.#otherBucket(entry, active, answered, startedAt, receivedAt);
      const built = buildTime(res.headers, receivedAt);
      const final = !active && isFinalBuild(tier, start, built);
      // A completed-URL copy built before its bucket closed is a partial copy CloudFront cached; its
      // one re-fetch must wait for that copy to expire, not repeat right after the close.
      const builtAt = active ? built : Math.max(built, nextBucket(tier, start));
      let put: PutResult;
      try {
        put = await this.#store.put(instrument, tier, start, body, { builtAt, final });
      } catch (error) {
        return this.#invalid(entry, error);
      }
      this.#succeeded(tier, receivedAt - startedAt, put);
      return { done: { status: 'fetched', final: put.final, empty: put.empty, bars: put.bars, bytes: put.bytes } };
    }
    if (res.status === 400 || res.status === 404) {
      // An answer all the same: it counts as one for the breaker and the rate, never as a failure.
      this.#succeeded(tier, receivedAt - startedAt, null);
      // "Too late" for a bucket that closed moments ago (by our clock, which may run ahead of
      // Dukascopy's) means it is not built yet, not that it never will be.
      const closedLongAgo = receivedAt / 1000 >= nextBucket(tier, start) + NOT_BUILT_SECONDS;
      if (!closedLongAgo && TOO_LATE.test(body.toString('utf8'))) {
        const what = `${instrument} ${tier} ${bucketKey(tier, start)}`;
        return { retry: new FetchError('HTTP', `Dukascopy has not built ${what} yet (HTTP ${res.status})`) };
      }
      this.#store.markUnavailable(instrument, tier, start);
      return { done: { status: 'unavailable', http: res.status } };
    }
    if (res.status === 429 || res.status === 403 || res.status === 503) {
      this.#throttled(res.status, res.headers, receivedAt);
      return { retry: new FetchError('THROTTLED', `Dukascopy answered ${res.status}`) };
    }
    this.#failed(false);
    const error = new FetchError('HTTP', `Dukascopy answered ${res.status} for ${url}`);
    return res.status >= 500 ? { retry: error } : this.#invalid(entry, error);
  }

  /** A 200 for another bucket than the one asked for, which must never be stored under this one. */
  #otherBucket(entry: Entry, active: boolean, answered: number, startedAt: number, receivedAt: number): Verdict {
    const { instrument, tier, start } = entry;
    const other = `the answer was for ${bucketKey(tier, answered)}`;
    if (active && answered > start) {
      // Our bucket closed while the request travelled (or Dukascopy's clock is ahead of ours). Its
      // completed copy is built about a second after the close: ask for it once the grace passed.
      this.#succeeded(tier, receivedAt - startedAt, null);
      const closed = Math.max(receivedAt / 1000, nextBucket(tier, start));
      const what = `${instrument} ${tier} ${bucketKey(tier, start)}`;
      return {
        retry: new FetchError('HTTP', `Dukascopy ${what} closed while it was asked for (${other})`),
        notBefore: (closed + CLOSE_GRACE_SECONDS) * 1000,
      };
    }
    return this.#invalid(entry, new FetchError('INVALID_RESPONSE', other));
  }

  /** An answer that cannot be stored: retried a few times, then the bucket fails for good. */
  #invalid(entry: Entry, error: unknown): Verdict {
    entry.invalid += 1;
    const decoding = error instanceof DecodeError;
    // Garbage from Dukascopy counts toward the breaker; a local disk problem does not.
    if (decoding) this.#failed(false);
    const code: FetchErrorCode = error instanceof FetchError ? error.code : decoding ? 'INVALID_RESPONSE' : 'STORE';
    const message = `Dukascopy ${entry.instrument} ${entry.tier} ${entry.start}: ${asError(error).message}`;
    if (entry.invalid >= MAX_INVALID_ATTEMPTS) {
      this.#log('warn', `${message}; giving up after ${entry.invalid} attempts`);
      return { fail: new FetchError(code, message) };
    }
    return { retry: new FetchError(code, message) };
  }

  #networkFailure(error: unknown): Verdict {
    const timeout = error instanceof FetchError && error.code === 'TIMEOUT';
    const offline = !timeout && OFFLINE_CODES.has(errorCode(error));
    this.#failed(offline);
    const code: FetchErrorCode = timeout ? 'TIMEOUT' : offline ? 'OFFLINE' : 'NETWORK';
    return { retry: new FetchError(code, `Dukascopy could not be reached: ${asError(error).message}`) };
  }

  // -------------------------------------------------------------------------------------------
  // Rate control and the breaker

  #succeeded(tier: Tier, latencyMs: number, put: PutResult | null): void {
    const now = this.#clock.now();
    this.#failures = 0;
    if (this.#breaker.state !== 'closed') {
      this.#breakerOpens = 0;
      this.#setBreaker(CLOSED);
      this.#log('info', 'Dukascopy answers again');
    }
    this.#successTimes.push(now);
    if (this.#successTimes.length > SUCCESSES_PER_STEP) this.#successTimes.shift();
    this.#maybeSpeedUp(now);

    const seconds = Math.max(0, latencyMs) / 1000;
    this.#latencySamples += 1;
    this.#latency += Math.max(1 / this.#latencySamples, EWMA_ALPHA) * (seconds - this.#latency);
    const s = this.#tiers[tier];
    s.samples += 1;
    s.secondsPerRequest += Math.max(1 / s.samples, EWMA_ALPHA) * (seconds - s.secondsPerRequest);
    if (put && !put.empty && put.bytes > 0) {
      s.buckets += 1;
      s.bytesPerBucket += Math.max(1 / s.buckets, EWMA_ALPHA) * (put.bytes - s.bytesPerBucket);
    }
    this.#markStatsDirty();
  }

  #maybeSpeedUp(now: number): void {
    const times = this.#successTimes;
    if (times.length < SUCCESSES_PER_STEP || now - this.#last429 < CALM_MS) return;
    const span = (times[times.length - 1]! - times[0]!) / 1000;
    const achieved = (times.length - 1) / Math.max(span, 0.001);
    // Only a streak that kept the limiter busy says the current rate is sustainable; an idle one
    // keeps sliding until the last 50 successes were busy.
    if (achieved < this.#rate * SUSTAINED_SHARE) return;
    this.#successTimes = [];
    this.#safeRate = Math.max(this.#safeRate ?? 0, this.#rate);
    const next = Math.min(this.#rate + RATE_STEP, this.#ceiling, MAX_RATE);
    if (next > this.#rate) this.#setRate(next);
    this.#markStatsDirty();
  }

  #throttled(status: number, headers: Headers, now: number): void {
    this.#failed(false);
    // Requests already in flight when the first 429 arrived answer 429 too: halve once per pause.
    if (now >= this.#pausedUntil) {
      const before = this.#rate;
      const proven = this.#safeRate;
      this.#last429 = now;
      if (status === 429) {
        // The limit is below the rate that failed: never climb back that far. Above the highest rate
        // sustained without a 429 that rate is the ceiling; at or below it the limit has moved (other
        // traffic from this address, a new policy), so the ceiling goes one step below the failure
        // and the proven rate follows it down. Both persist, so a restart does not climb back either.
        const cap = proven !== null && before > proven ? proven : before - RATE_STEP;
        this.#ceiling = Math.max(MIN_RATE, Math.min(this.#ceiling, cap));
        if (proven !== null) this.#safeRate = Math.min(proven, this.#ceiling);
      } else if (proven !== null && before > proven) {
        // A 403/503 may be an outage rather than the limit: back to the proven rate, never below it.
        this.#ceiling = Math.min(this.#ceiling, proven);
      }
      this.#setRate(Math.max(MIN_RATE, Math.min(before / 2, this.#ceiling)));
      this.#markStatsDirty();
    }
    const wait = Math.max(THROTTLE_PAUSE_MS, retryAfterMs(headers.get('retry-after'), now) ?? 0);
    if (now + wait > this.#pausedUntil) {
      this.#pausedUntil = now + wait;
      this.#tokens = 0;
      this.#refilledAt = this.#pausedUntil;
      this.#log('warn', `Dukascopy is throttling; pausing for ${Math.round(wait / 1000)} s at ${this.#rate} req/s`);
      this.#emit({ type: 'paused', until: this.#pausedUntil });
      this.#rejectFailFast();
    }
  }

  #failed(offline: boolean): void {
    const now = this.#clock.now();
    this.#failures += 1;
    this.#successTimes = [];
    const reason = offline ? 'offline' : 'throttled';
    if (this.#breaker.state === 'half-open') this.#openBreaker(reason, now);
    else if (this.#breaker.state === 'closed' && (offline || this.#failures >= BREAKER_FAILURES)) {
      this.#openBreaker(reason, now);
    }
  }

  #openBreaker(reason: 'throttled' | 'offline', now: number): void {
    const [first, longest] = BREAKER_WAIT_MS[reason];
    const wait = Math.min(longest, first * 2 ** this.#breakerOpens);
    this.#breakerOpens += 1;
    const message =
      reason === 'offline' ? 'Offline' : `Dukascopy is throttling; retrying in ${Math.ceil(wait / 60_000)} min`;
    this.#setBreaker({ state: 'open', reason, message, retryAt: now + wait });
    this.#log('warn', `Dukascopy downloads paused: ${message}`);
    this.#rejectFailFast();
  }

  #tickBreaker(now: number): void {
    const b = this.#breaker;
    if (b.state === 'open' && b.retryAt !== null && now >= b.retryAt) this.#setBreaker({ ...b, state: 'half-open' });
  }

  /** Why a request would have to wait out an outage or a throttling pause right now, if it would. */
  #blockedError(): FetchError | null {
    const b = this.#breaker;
    if (b.state !== 'closed') {
      return new FetchError(b.reason === 'offline' ? 'OFFLINE' : 'THROTTLED', b.message ?? 'Dukascopy is unreachable');
    }
    const wait = this.#pausedUntil - this.#clock.now();
    if (wait > 0)
      return new FetchError('THROTTLED', `Dukascopy is throttling; retrying in ${Math.ceil(wait / 1000)} s`);
    return null;
  }

  /** Callers that asked not to wait through an outage or a pause learn about it at once. */
  #rejectFailFast(): void {
    const error = this.#blockedError();
    if (!error) return;
    for (const entry of [...this.#entries.values()]) {
      if (entry.inFlight) continue;
      for (const waiter of [...entry.waiters]) if (waiter.failFast) this.#settle(entry, waiter, error);
      this.#relane(entry);
    }
  }

  #setBreaker(state: BreakerState): void {
    this.#breaker = state;
    this.#emit({ type: 'breaker', breaker: { ...state } });
  }

  #setRate(rate: number): void {
    this.#rate = rate;
    this.#emit({ type: 'rate', rate });
  }

  #emit(event: FetcherEvent): void {
    for (const listener of this.#listeners) {
      try {
        listener(event);
      } catch (error) {
        this.#log('error', `Fetcher listener failed: ${asError(error).message}`);
      }
    }
  }

  #countBytes(now: number, bytes: number): void {
    this.#bytesWindow.push([now, bytes]);
    while (this.#bytesWindow.length > 0 && this.#bytesWindow[0]![0] <= now - BYTES_WINDOW_MS) this.#bytesWindow.shift();
  }

  #bytesPerSecond(now: number): number {
    let sum = 0;
    for (const [t, bytes] of this.#bytesWindow) if (t > now - BYTES_WINDOW_MS) sum += bytes;
    return sum / (BYTES_WINDOW_MS / 1000);
  }

  // -------------------------------------------------------------------------------------------
  // stats.json: {version, safeRate, ceiling, tiers: {m1: {secondsPerRequest, bytesPerBucket, samples, buckets}}}

  #loadStats(): void {
    let json: Record<string, unknown>;
    try {
      json = JSON.parse(fs.readFileSync(this.#statsPath, 'utf8')) as Record<string, unknown>;
    } catch {
      return; // First run, or unreadable: the defaults are safe.
    }
    if (!json || json.version !== STATS_VERSION) return;
    const num = (v: unknown, lo: number, hi: number) =>
      typeof v === 'number' && Number.isFinite(v) && v >= lo && v <= hi ? v : null;
    this.#safeRate = num(json.safeRate, MIN_RATE, MAX_RATE);
    this.#ceiling = num(json.ceiling, MIN_RATE, MAX_RATE) ?? MAX_RATE;
    const tiers = (json.tiers ?? {}) as Record<string, Record<string, unknown> | undefined>;
    for (const tier of TIERS) {
      const t = tiers[tier];
      if (!t || typeof t !== 'object') continue;
      const s = this.#tiers[tier];
      s.secondsPerRequest = num(t.secondsPerRequest, 0, 600) ?? s.secondsPerRequest;
      s.bytesPerBucket = num(t.bytesPerBucket, 0, 1e9) ?? s.bytesPerBucket;
      s.samples = Math.floor(num(t.samples, 0, Number.MAX_SAFE_INTEGER) ?? 0);
      s.buckets = Math.floor(num(t.buckets, 0, Number.MAX_SAFE_INTEGER) ?? 0);
    }
    const measured = TIERS.filter((tier) => this.#tiers[tier].samples > 0);
    if (measured.length > 0) {
      this.#latency = measured.reduce((sum, tier) => sum + this.#tiers[tier].secondsPerRequest, 0) / measured.length;
      this.#latencySamples = 1 / EWMA_ALPHA;
    }
  }

  #markStatsDirty(): void {
    this.#statsDirty = true;
    if (this.#statsTimer !== null || this.#statsWriting || this.#closed) return;
    this.#statsTimer = this.#clock.setTimeout(() => {
      this.#statsTimer = null;
      void this.#writeStats().catch(() => {});
    }, this.#statsDelayMs);
  }

  async #writeStats(): Promise<void> {
    if (this.#statsWriting) return this.#statsWriting;
    this.#statsDirty = false;
    const json = JSON.stringify({
      version: STATS_VERSION,
      safeRate: this.#safeRate,
      ceiling: this.#ceiling,
      tiers: this.#tiers,
    });
    this.#statsWriting = writeAtomic(this.#statsPath, json).catch((error: unknown) => {
      this.#statsDirty = true;
      this.#log('warn', `Could not save Dukascopy download stats: ${asError(error).message}`);
    });
    try {
      await this.#statsWriting;
    } finally {
      this.#statsWriting = null;
      if (this.#statsDirty && !this.#closed) this.#markStatsDirty();
    }
  }
}
