// Shared fixtures for the store tests: temp caches, synthetic Dukascopy responses shaped like a
// forex market's sessions, and a fetcher double that answers from them without a network.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { type Bar } from '../src/protocol.ts';
import { type Tier, TIER_BAR_SECONDS, bucketStart, nextBucket } from '../src/store/buckets.ts';
import { type DukascopyStore } from '../src/store/dukascopy-store.ts';
import { type FetchRequest, type FetchResult, type FetcherEvent, type FetcherStatus } from '../src/store/fetcher.ts';

export const s = (iso: string) => Date.parse(iso) / 1000;
export const iso = (t: number) => new Date(t * 1000).toISOString();
export const DAY = 86400;

const dirs: string[] = [];

export function tempDir(prefix = 'demido-s2-'): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  dirs.push(dir);
  return dir;
}

export function cleanup(): void {
  for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5 });
}

const MULTIPLIER = 0.00001;

/** A raw candle response for these bars (prices must be multiples of 0.00001). */
export function encode(timestamp: number, shift: number, bars: readonly Bar[]): Buffer {
  const units = (p: number) => Math.round(p / MULTIPLIER);
  const first = bars[0];
  const times: number[] = [];
  const cols = { opens: [] as number[], highs: [] as number[], lows: [] as number[], closes: [] as number[] };
  let t = timestamp;
  let prev = first ? { o: units(first.o), h: units(first.h), l: units(first.l), c: units(first.c) } : null;
  for (const b of bars) {
    times.push((b.t - t) / shift);
    t = b.t;
    const cur = { o: units(b.o), h: units(b.h), l: units(b.l), c: units(b.c) };
    cols.opens.push(cur.o - prev!.o);
    cols.highs.push(cur.h - prev!.h);
    cols.lows.push(cur.l - prev!.l);
    cols.closes.push(cur.c - prev!.c);
    prev = cur;
  }
  return Buffer.from(
    JSON.stringify({
      timestamp: timestamp * 1000,
      multiplier: MULTIPLIER,
      shift: shift * 1000,
      open: first ? first.o : null,
      high: first ? first.h : null,
      low: first ? first.l : null,
      close: first ? first.c : null,
      times,
      ...cols,
      volumes: bars.map((b) => b.v),
    }),
  );
}

const weekday = (t: number) => new Date(t * 1000).getUTCDay();

/** Whether a forex market trades at t: Sunday 21:00 to Friday 21:00 UTC. */
export function fxOpen(t: number): boolean {
  const day = weekday(t);
  const hour = new Date(t * 1000).getUTCHours();
  if (day === 6) return false;
  if (day === 0) return hour >= 21;
  if (day === 5) return hour < 21;
  return true;
}

/** Deterministic price for a time (so every tier agrees on what a bar is). */
export function priceAt(t: number): number {
  return Math.round((1.1 + ((Math.floor(t / 60) % 997) - 498) * 0.00001) * 100000) / 100000;
}

/**
 * Synthetic bars of one bucket. `every` thins them out (one bar per `every` seconds, still on bar
 * boundaries) so long histories stay small; open() decides when the market trades.
 */
export function bucketBars(
  tier: Tier,
  start: number,
  opts: { every?: number; open?: (t: number) => boolean; from?: number; to?: number } = {},
): Bar[] {
  const step = TIER_BAR_SECONDS[tier];
  const every = Math.max(step, opts.every ?? step);
  const open = opts.open ?? fxOpen;
  const out: Bar[] = [];
  const end = Math.min(nextBucket(tier, start), opts.to ?? Infinity);
  for (let t = Math.max(start, opts.from ?? start); t < end; t += step) {
    if ((t - start) % every !== 0) continue;
    // A d1 bar exists when any of its day trades.
    const trades = tier === 'd1' ? open(t + 22 * 3600) || open(t + 12 * 3600) : open(t);
    if (!trades) continue;
    const p = priceAt(t);
    out.push({
      t,
      o: p,
      h: Math.round((p + 0.0002) * 1e5) / 1e5,
      l: Math.round((p - 0.0002) * 1e5) / 1e5,
      c: p,
      v: 1.5,
    });
  }
  return out;
}

export function bucketBody(tier: Tier, start: number, bars: readonly Bar[]): Buffer {
  return encode(start, TIER_BAR_SECONDS[tier], bars);
}

/** Stores a bucket as final (or provisional) with these synthetic bars. */
export async function putBucket(
  store: DukascopyStore,
  instrument: string,
  tier: Tier,
  start: number,
  bars: readonly Bar[] | null,
  final = true,
): Promise<void> {
  const b = bucketStart(tier, start);
  const builtAt = final ? nextBucket(tier, b) + 7200 : nextBucket(tier, b) + 60;
  await store.put(instrument, tier, b, bars && bars.length ? bucketBody(tier, b, bars) : null, { builtAt, final });
}

/**
 * The fetcher's interface, answering from a data function instead of Dukascopy: at dispatch it drops
 * what the store has (like the real one), dedupes by bucket, and records every request it makes.
 * `hold` keeps requests waiting until release().
 */
export class FakeFetcher {
  requests: string[] = [];
  cancelled: string[] = [];
  lanes: string[] = [];
  /** `<lane> <jobId or -> <key>` per request call: who asked, for job accounting. */
  calls: string[] = [];
  hold = false;
  breaker: FetcherStatus['breaker'] = { state: 'closed', reason: null, message: null, retryAt: null };
  #held: Array<() => void> = [];
  #inflight = new Map<string, Promise<FetchResult>>();
  #waiters = new Map<string, number>();
  #listeners = new Set<(e: FetcherEvent) => void>();
  readonly store: DukascopyStore;
  readonly data: (instrument: string, tier: Tier, start: number) => Bar[] | 'unavailable';
  readonly now: () => number;

  constructor(
    store: DukascopyStore,
    data: (instrument: string, tier: Tier, start: number) => Bar[] | 'unavailable',
    now: () => number,
  ) {
    this.store = store;
    this.data = data;
    this.now = now;
  }

  request(req: FetchRequest): Promise<FetchResult> {
    const start = bucketStart(req.tier, req.start);
    const key = `${req.instrument}/${req.tier}/${start}`;
    if (req.signal?.aborted) return Promise.resolve({ status: 'cancelled' });
    this.lanes.push(`${req.lane} ${key}`);
    this.calls.push(`${req.lane} ${req.jobId ?? '-'} ${key}`);
    this.#waiters.set(key, (this.#waiters.get(key) ?? 0) + 1);
    const existing = this.#inflight.get(key);
    const run =
      existing ??
      (async (): Promise<FetchResult> => {
        if (this.hold) await new Promise<void>((resolve) => this.#held.push(resolve));
        await new Promise((resolve) => setImmediate(resolve));
        // Like the real queue: an entry nobody waits for any more leaves without a request.
        if (!this.#waiters.get(key)) return { status: 'cancelled' };
        if (!this.store.needsFetch(req.instrument, req.tier, start, Math.floor(this.now() / 1000))) {
          return { status: 'cached' };
        }
        this.requests.push(key);
        const bars = this.data(req.instrument, req.tier, start);
        if (bars === 'unavailable') {
          this.store.markUnavailable(req.instrument, req.tier, start);
          return { status: 'unavailable', http: 404 };
        }
        const builtAt = nextBucket(req.tier, start) + 7200;
        const put = await this.store.put(
          req.instrument,
          req.tier,
          start,
          bars.length ? bucketBody(req.tier, start, bars) : null,
          { builtAt, final: builtAt <= Math.floor(this.now() / 1000) },
        );
        return { status: 'fetched', final: put.final, empty: put.empty, bars: put.bars, bytes: put.bytes };
      })().finally(() => {
        this.#inflight.delete(key);
        this.#waiters.delete(key);
      });
    if (!existing) this.#inflight.set(key, run);
    const leave = () => this.#waiters.set(key, Math.max(0, (this.#waiters.get(key) ?? 1) - 1));
    if (!req.signal) return run;
    return new Promise((resolve, reject) => {
      const onAbort = () => {
        this.cancelled.push(key);
        leave();
        resolve({ status: 'cancelled' });
      };
      req.signal!.addEventListener('abort', onAbort, { once: true });
      run.then(
        (r) => {
          req.signal!.removeEventListener('abort', onAbort);
          resolve(r);
        },
        (e: unknown) => {
          req.signal!.removeEventListener('abort', onAbort);
          reject(e);
        },
      );
    });
  }

  /** Lets `n` held requests go on (all, and stops holding, by default). */
  release(n = Infinity): void {
    if (n === Infinity) this.hold = false;
    for (const resolve of this.#held.splice(0, n)) resolve();
  }

  get held(): number {
    return this.#held.length;
  }

  cancelJob(jobId: string): number {
    this.cancelled.push(`job:${jobId}`);
    return 0;
  }

  status(): FetcherStatus {
    return {
      rate: 3,
      safeRate: null,
      ceiling: 8,
      inFlight: this.#inflight.size,
      queued: { interactive: 0, waiting: 0, bulk: 0 },
      bytesPerSecond: 0,
      pausedUntil: null,
      breaker: { ...this.breaker },
    };
  }

  effectiveRate(): number {
    return 3;
  }

  backlog(): number {
    return 0;
  }

  tierStats(tier: Tier) {
    const bytes = { m1: 9216, h1: 5120, d1: 4096 }[tier];
    return { secondsPerRequest: 0.3, bytesPerBucket: bytes, samples: 0, buckets: 0 };
  }

  subscribe(listener: (e: FetcherEvent) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  emit(event: FetcherEvent): void {
    for (const l of this.#listeners) l(event);
  }
}

/** Resolves once `check` holds, polling (fails the test after `ms`). */
export async function until(check: () => boolean, ms = 10_000, what = 'condition'): Promise<void> {
  const deadline = Date.now() + ms;
  while (!check()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}
