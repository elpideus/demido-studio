// The global Dukascopy queue with a fake fetch and a fake clock: nothing here touches the network.

import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, test } from 'node:test';

import { type Bar } from '../src/protocol.ts';
import { activeUrl, bucketStart, completedUrl, isActive, nextBucket, type Tier } from '../src/store/buckets.ts';
import { decode } from '../src/store/decode.ts';
import {
  CLOSE_GRACE_SECONDS,
  DUE_AFTER_SECONDS,
  DukascopyStore,
  provisionalDue,
  type BucketStatus,
  type PutMeta,
  type PutResult,
} from '../src/store/dukascopy-store.ts';
import {
  BREAKER_FAILURES,
  CALM_MS,
  Fetcher,
  FetchError,
  MAX_INVALID_ATTEMPTS,
  REQUEST_TIMEOUT_MS,
  buildTime,
  retryAfterMs,
  type Clock,
  type FetchLike,
  type FetchRequest,
  type FetchResult,
  type FetcherEvent,
  type FetcherStore,
  type Lane,
} from '../src/store/fetcher.ts';

const s = (iso: string) => Date.parse(iso) / 1000;
const NOW = s('2024-06-01T12:00:00Z');
const DAY = s('2024-03-05T00:00:00Z');
const day = (i: number) => DAY + i * 86400;
const url = (start: number, tier: Tier = 'm1') => completedUrl('EUR-USD', tier, start);

const dirs: string[] = [];
const fetchers: Fetcher[] = [];
afterEach(async () => {
  for (const f of fetchers.splice(0)) await f.close(0);
  for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function tempDir(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'demido-fetcher-'));
  dirs.push(dir);
  return dir;
}

/** Runs timers in order as time advances, letting promises (and fake I/O) settle in between. */
class FakeClock implements Clock {
  t: number;
  #timers = new Map<number, { at: number; fn: () => void }>();
  #id = 0;

  constructor(t: number) {
    this.t = t;
  }

  now = () => this.t;

  setTimeout = (fn: () => void, ms: number) => {
    const id = ++this.#id;
    this.#timers.set(id, { at: this.t + Math.max(0, ms), fn });
    return id;
  };

  clearTimeout = (id: unknown) => {
    this.#timers.delete(id as number);
  };

  async advance(ms: number): Promise<void> {
    const target = this.t + ms;
    await settle();
    for (;;) {
      let nextId = -1;
      let next: { at: number; fn: () => void } | undefined;
      for (const [id, timer] of this.#timers) {
        if (timer.at <= target && (!next || timer.at < next.at)) {
          next = timer;
          nextId = id;
        }
      }
      if (!next) break;
      this.#timers.delete(nextId);
      this.t = Math.max(this.t, next.at);
      next.fn();
      await settle();
    }
    this.t = target;
    await settle();
  }
}

async function settle(): Promise<void> {
  for (let i = 0; i < 30; i += 1) await new Promise((resolve) => setImmediate(resolve));
}

/** An in-memory store with the real store's rules for status and due buckets. */
class FakeStore implements FetcherStore {
  readonly root: string;
  records = new Map<string, { final: boolean; builtAt: number; bars: Bar[] }>();
  unavailable = new Set<string>();
  puts: Array<{ key: string } & PutMeta> = [];
  putError: Error | null = null;

  constructor(root: string) {
    this.root = root;
  }

  status(instrument: string, tier: Tier, start: number): BucketStatus {
    const key = `${instrument}/${tier}/${start}`;
    const r = this.records.get(key);
    if (r) return r.final ? 'final' : 'provisional';
    return this.unavailable.has(key) ? 'unavailable' : 'missing';
  }

  needsFetch(instrument: string, tier: Tier, start: number, now = NOW, opts: { activeMaxAge?: number } = {}): boolean {
    const key = `${instrument}/${tier}/${start}`;
    const r = this.records.get(key);
    if (!r) return !this.unavailable.has(key);
    if (r.final) return false;
    if (isActive(tier, start, now)) return now - r.builtAt >= (opts.activeMaxAge ?? 60);
    return provisionalDue(tier, start, r.builtAt, now);
  }

  async put(
    instrument: string,
    tier: Tier,
    start: number,
    buffer: Buffer | Uint8Array | null,
    meta: PutMeta,
  ): Promise<PutResult> {
    await Promise.resolve();
    if (this.putError) throw this.putError;
    const key = `${instrument}/${tier}/${start}`;
    const bars = buffer ? decode(buffer) : [];
    this.puts.push({ key, ...meta });
    this.records.set(key, { final: meta.final ?? false, builtAt: meta.builtAt, bars });
    this.unavailable.delete(key);
    return {
      empty: bars.length === 0,
      final: meta.final ?? false,
      bars: bars.length,
      bytes: bars.length * 100,
      changed: true,
    };
  }

  markUnavailable(instrument: string, tier: Tier, start: number): void {
    this.unavailable.add(`${instrument}/${tier}/${start}`);
  }
}

/** A raw response of `n` minute candles for the bucket starting at `start` (seconds). */
function candles(n: number, start = DAY): Buffer {
  return Buffer.from(
    JSON.stringify({
      timestamp: start * 1000,
      multiplier: 0.00001,
      shift: 60_000,
      open: n ? 1.1 : null,
      high: n ? 1.1 : null,
      low: n ? 1.1 : null,
      close: n ? 1.1 : null,
      times: Array.from({ length: n }, (_, i) => (i === 0 ? 0 : 1)),
      opens: new Array(n).fill(0),
      highs: new Array(n).fill(0),
      lows: new Array(n).fill(0),
      closes: new Array(n).fill(0),
      volumes: new Array(n).fill(1),
    }),
  );
}

const TIER_OF: Record<string, Tier> = { minute: 'm1', hour: 'h1', day: 'd1' };

/** The bucket fake Dukascopy answers a URL with: the period a completed URL names; for `?from=`, the
 *  bucket containing its own now, whatever `from` says (as measured on the real API). */
function answeredBucket(u: string, serverNowMs: number): number {
  const m = /\/candles\/(minute|hour|day)\/[^/]+\/BID(?:\/(\d+)(?:\/(\d+))?(?:\/(\d+))?)?(?:\?from=\d+)?$/.exec(u);
  if (!m) throw new Error(`not a candle URL: ${u}`);
  const tier = TIER_OF[m[1]!]!;
  if (m[2] === undefined) return bucketStart(tier, Math.floor(serverNowMs / 1000));
  return Date.UTC(Number(m[2]), Number(m[3] ?? 1) - 1, Number(m[4] ?? 1)) / 1000;
}

/** A response, or one built for the URL and fake Dukascopy's clock when the request is answered. */
type Answer = Response | ((url: string, serverNowMs: number) => Response);

const httpDate = (sec: number) => new Date(sec * 1000).toUTCString();
const ok =
  (builtAt = NOW, n = 2, headers: Record<string, string> = {}): Answer =>
  (u, serverNowMs) =>
    new Response(candles(n, answeredBucket(u, serverNowMs)), {
      status: 200,
      headers: { date: httpDate(builtAt), ...headers },
    });
const answer = (status: number, headers: Record<string, string> = {}, text = '{"error":"Too Many Requests"}') =>
  new Response(text, { status, headers });
const tooLate = () => answer(400, { 'cache-control': 'no-store' }, '{"error":"From time is too late"}');

type Responder = (url: string, signal: AbortSignal) => Answer | Promise<Answer>;

function setup(
  opts: {
    respond?: Responder;
    store?: FetcherStore;
    start?: number;
    random?: () => number;
    /** How far fake Dukascopy's clock runs ahead of ours (ms). */
    skewMs?: number;
  } = {},
) {
  const clock = new FakeClock((opts.start ?? NOW) * 1000);
  const store = opts.store ?? new FakeStore(tempDir());
  const calls: Array<{ url: string; headers: Record<string, string>; at: number }> = [];
  let respond: Responder = opts.respond ?? (() => ok());
  const fetch: FetchLike = async (u, init) => {
    calls.push({ url: u, headers: init.headers, at: clock.t });
    const a = await respond(u, init.signal);
    return typeof a === 'function' ? a(u, clock.t + (opts.skewMs ?? 0)) : a;
  };
  const events: FetcherEvent[] = [];
  const logs: string[] = [];
  const fetcher = new Fetcher(store, {
    fetch,
    clock,
    random: opts.random ?? (() => 0),
    statsDelayMs: 1e12,
    log: (level, message) => logs.push(`${level}: ${message}`),
  });
  fetchers.push(fetcher);
  fetcher.subscribe((e) => events.push(e));
  const req = (start: number, lane: Lane = 'bulk', more: Partial<FetchRequest> = {}) =>
    fetcher.request({ instrument: 'eurusd', tier: 'm1', start, lane, ...more });
  return {
    clock,
    store,
    calls,
    events,
    logs,
    fetcher,
    req,
    urls: () => calls.map((c) => c.url),
    respondWith: (r: Responder) => {
      respond = r;
    },
  };
}

/** A response the test resolves by hand; aborting the request rejects it like fetch does. */
function manual() {
  const pending: Array<{ url: string; resolve: (r: Answer) => void }> = [];
  const respond: Responder = (u, signal) =>
    new Promise<Answer>((resolve, reject) => {
      pending.push({ url: u, resolve });
      signal.addEventListener('abort', () => reject(signal.reason));
    });
  return { pending, respond };
}

const track = <T>(p: Promise<T>) => {
  const state: { settled: boolean; value?: T; error?: unknown } = { settled: false };
  p.then(
    (value) => Object.assign(state, { settled: true, value }),
    (error: unknown) => Object.assign(state, { settled: true, error }),
  );
  return state;
};

test('headers: Date is the build time, else receive time minus Age; Retry-After in both forms', () => {
  const at = s('2024-03-06T00:00:01Z');
  assert.equal(buildTime(new Headers({ date: httpDate(at), age: '500' }), NOW * 1000), at);
  assert.equal(buildTime(new Headers({ age: '3600' }), NOW * 1000), NOW - 3600);
  assert.equal(buildTime(new Headers({ date: 'nonsense' }), NOW * 1000 + 999), NOW);
  assert.equal(retryAfterMs('120', 0), 120_000);
  assert.equal(retryAfterMs(httpDate(NOW + 90), NOW * 1000), 90_000);
  assert.equal(retryAfterMs(null, 0), null);
  assert.equal(retryAfterMs('soon', 0), null);
});

test('a bucket is requested once: callers attach to queued and in-flight entries', async () => {
  const m = manual();
  const { req, calls, fetcher, clock } = setup({ respond: m.respond });
  // The first request takes the one token and is in flight at once.
  const a1 = req(day(0), 'bulk', { jobId: 'a' });
  const a2 = req(day(0), 'interactive');
  assert.equal(calls.length, 1);
  // The others wait for tokens; a second caller for a queued bucket joins it.
  const b1 = req(day(1));
  const b2 = req(day(1), 'waiting', { jobId: 'x' });
  assert.equal(fetcher.isPending('eurusd', 'm1', day(1) + 3600), true);
  assert.equal(fetcher.status().queued.waiting, 1);
  await clock.advance(5_000);
  assert.deepEqual(
    calls.map((c) => c.url),
    [url(day(0)), url(day(1))],
  );
  assert.equal(calls[0]!.headers['accept-encoding'], 'gzip');
  for (const p of m.pending) p.resolve(ok());
  const results = await Promise.all([a1, a2, b1, b2]);
  for (const r of results) assert.equal(r.status, 'fetched');
  assert.equal(fetcher.isPending('eurusd', 'm1', day(1)), false);
  // Finished: a new request for it is dropped at dispatch, without a request.
  assert.deepEqual(await req(day(1)), { status: 'cached' });
  assert.equal(calls.length, 2);
});

test('dispatch re-checks the store: buckets stored meanwhile are dropped without a request', async () => {
  const store = new FakeStore(tempDir());
  const { req, calls, clock } = setup({ store });
  const first = req(day(0));
  const fetchedMeanwhile = req(day(1));
  const provisionalNotDue = req(day(2));
  const provisionalDueNow = req(day(3));
  store.records.set(`eurusd/m1/${day(1)}`, { final: true, builtAt: NOW, bars: [] });
  store.records.set(`eurusd/m1/${day(2)}`, { final: false, builtAt: NOW - 3600, bars: [] });
  store.records.set(`eurusd/m1/${day(3)}`, { final: false, builtAt: NOW - 8 * 86400, bars: [] });
  await clock.advance(5_000);
  assert.equal((await first).status, 'fetched');
  assert.deepEqual(await fetchedMeanwhile, { status: 'cached' });
  assert.deepEqual(await provisionalNotDue, { status: 'cached' });
  assert.equal((await provisionalDueNow).status, 'fetched');
  assert.deepEqual(
    calls.map((c) => c.url),
    [url(day(0)), url(day(3))],
  );
  // Dropped entries cost no token: the second request went out one token (1/3 s) after the first.
  assert.equal(calls[1]!.at - calls[0]!.at, 334);
});

test('the URL is built at dispatch: a bucket that closed while queued gets its completed URL', async () => {
  const today = s('2024-06-01T00:00:00Z');
  const end = nextBucket('m1', today);
  const m = manual();
  const { req, urls, clock } = setup({ respond: m.respond, start: end - 0.1 });
  const other = req(day(0));
  const late = req(today);
  // Queued behind the token for 1/3 s, it dispatches after the day closed.
  await clock.advance(1_000);
  assert.deepEqual(urls(), [url(day(0)), completedUrl('EUR-USD', 'm1', today)]);
  const open = setup({ start: end - 3600 });
  const active = await open.req(today, 'interactive');
  assert.deepEqual(open.urls(), [activeUrl('EUR-USD', 'm1', today)]);
  assert.equal(active.status === 'fetched' && active.final, false);
  for (const p of m.pending) p.resolve(ok());
  await Promise.all([other, late]);
});

test("lanes: interactive, then waiting, then bulk; an entry runs in its best waiter's lane", async () => {
  const { req, urls, clock, fetcher } = setup();
  const all = [req(day(0)), req(day(1), 'bulk'), req(day(2), 'waiting'), req(day(3), 'interactive'), req(day(4))];
  // A bulk entry gains the interactive lane when an interactive caller joins it...
  all.push(req(day(4), 'interactive'));
  // ...and loses it again when that caller gives up.
  const ctl = new AbortController();
  const gaveUp = req(day(1), 'interactive', { signal: ctl.signal });
  ctl.abort();
  assert.deepEqual(await gaveUp, { status: 'cancelled' });
  assert.equal(fetcher.backlog('interactive'), 2);
  assert.equal(fetcher.backlog('waiting'), 3);
  assert.equal(fetcher.backlog('bulk'), 5);
  await clock.advance(5_000);
  await Promise.all(all);
  assert.deepEqual(urls(), [url(day(0)), url(day(3)), url(day(4)), url(day(2)), url(day(1))]);
});

test('bulk entries are served round-robin between jobs', async () => {
  const { req, urls, clock } = setup();
  const all = [
    req(day(0), 'bulk', { jobId: 'a' }),
    req(day(1), 'bulk', { jobId: 'a' }),
    req(day(2), 'bulk', { jobId: 'a' }),
    req(day(10), 'bulk', { jobId: 'b' }),
    req(day(11), 'bulk', { jobId: 'b' }),
    req(day(12), 'bulk', { jobId: 'b' }),
    req(day(20), 'bulk', { jobId: 'c' }),
  ];
  await clock.advance(10_000);
  await Promise.all(all);
  assert.deepEqual(
    urls(),
    [0, 10, 20, 1, 11, 2, 12].map((i) => url(day(i))),
  );
});

test('cancelJob and aborted signals remove only their waiters', async () => {
  const { req, calls, clock, fetcher } = setup();
  const first = req(day(0), 'bulk', { jobId: 'a' });
  const a1 = req(day(1), 'bulk', { jobId: 'a' });
  const shared = req(day(2), 'bulk', { jobId: 'a' });
  const sharedB = req(day(2), 'bulk', { jobId: 'b' });
  assert.equal(fetcher.queuedFor('a'), 3);
  assert.equal(fetcher.cancelJob('a'), 3);
  assert.deepEqual(await a1, { status: 'cancelled' });
  assert.deepEqual(await shared, { status: 'cancelled' });
  assert.equal(fetcher.isPending('eurusd', 'm1', day(1)), false);
  assert.equal(fetcher.isPending('eurusd', 'm1', day(2)), true);
  await clock.advance(5_000);
  assert.equal((await sharedB).status, 'fetched');
  // The in-flight request of a cancelled job still lands: the data is kept.
  assert.deepEqual(await first, { status: 'cancelled' });
  assert.deepEqual(
    calls.map((c) => c.url),
    [url(day(0)), url(day(2))],
  );
  const ctl = new AbortController();
  ctl.abort();
  assert.deepEqual(await req(day(5), 'bulk', { signal: ctl.signal }), { status: 'cancelled' });
});

test('the rate: 3 req/s at start, at most 4 requests in flight', async () => {
  const m = manual();
  const { req, calls, clock } = setup({ respond: m.respond });
  const all = Array.from({ length: 8 }, (_, i) => req(day(i)));
  assert.equal(calls.length, 1);
  await clock.advance(333);
  assert.equal(calls.length, 1);
  await clock.advance(1);
  assert.equal(calls.length, 2);
  await clock.advance(10_000);
  assert.equal(calls.length, 4, 'in-flight cap');
  m.pending.shift()!.resolve(ok());
  await clock.advance(0);
  assert.equal(calls.length, 5);
  while (m.pending.length) {
    m.pending.shift()!.resolve(ok());
    await clock.advance(1_000);
  }
  for (const r of await Promise.all(all)) assert.equal(r.status, 'fetched');
});

test('429: the rate halves once, the queue pauses for Retry-After but at least 60 s', async () => {
  const { req, calls, clock, fetcher, events, respondWith } = setup({
    respond: () => answer(429, { 'retry-after': '120' }),
  });
  const p = track(req(day(0)));
  // A caller that will not wait is told at once when the pause starts...
  const impatient = track(req(day(9), 'interactive', { failFast: true }));
  await clock.advance(0);
  assert.equal(calls.length, 1);
  const st = fetcher.status();
  assert.equal(st.rate, 1.5);
  assert.equal(st.pausedUntil, NOW * 1000 + 120_000);
  assert.deepEqual(
    events.filter((e) => e.type !== 'rate'),
    [{ type: 'paused', until: NOW * 1000 + 120_000 }],
  );
  assert.ok(impatient.error instanceof FetchError && impatient.error.code === 'THROTTLED');
  // ...and during it.
  await assert.rejects(req(day(9), 'interactive', { failFast: true }), /retrying in 120 s/);
  // Nothing goes out during the pause, in any lane, and nothing else fails.
  const q = track(req(day(1), 'interactive'));
  respondWith(() => ok());
  await clock.advance(119_000);
  assert.equal(calls.length, 1);
  assert.equal(p.settled || q.settled, false);
  await clock.advance(5_000);
  assert.equal(calls.length, 3);
  assert.equal(calls[1]!.url, url(day(1)), 'interactive first after the pause');
  assert.equal(calls[2]!.url, url(day(0)), 'the retry went back through the limiter');
  assert.equal((p.value as FetchResult).status, 'fetched');

  // A short Retry-After still pauses 60 s.
  const short = setup({ respond: () => answer(429, { 'retry-after': '5' }) });
  void short.req(day(0));
  await short.clock.advance(0);
  assert.equal(short.fetcher.status().pausedUntil, NOW * 1000 + 60_000);
  // 403 and 503 throttle too.
  for (const code of [403, 503]) {
    const other = setup({ respond: () => answer(code) });
    void other.req(day(0));
    await other.clock.advance(0);
    assert.equal(other.fetcher.status().rate, 1.5, String(code));
  }
});

test('429s from requests already in flight halve the rate only once', async () => {
  const m = manual();
  const { req, calls, clock, fetcher } = setup({ respond: m.respond });
  for (let i = 0; i < 4; i += 1) void req(day(i));
  await clock.advance(1_100);
  assert.equal(calls.length, 4);
  for (const p of m.pending.splice(0)) p.resolve(answer(429));
  await clock.advance(0);
  assert.equal(fetcher.status().rate, 1.5);
});

test('the rate grows after 50 busy successes and 2 calm minutes, and never passes a proven limit again', async () => {
  const { req, clock, fetcher, respondWith, store } = setup();
  const batch = async (from: number) => {
    const all = Array.from({ length: 50 }, (_, i) => req(day(from + i)));
    await clock.advance(60_000);
    await Promise.all(all);
  };
  // 50 successes before the calm period is over: no change yet.
  await batch(0);
  assert.equal(fetcher.status().rate, 3);
  await clock.advance(CALM_MS);
  await batch(100);
  assert.equal(fetcher.status().rate, 3.5);
  assert.equal(fetcher.status().safeRate, 3);
  // A 429 while probing above the proven rate caps the rate there.
  respondWith(() => answer(429));
  const throttled = req(day(200));
  await clock.advance(1_000);
  respondWith(() => ok());
  assert.equal(fetcher.status().rate, 1.75);
  assert.equal(fetcher.status().ceiling, 3);
  await clock.advance(70_000);
  assert.equal((await throttled).status, 'fetched');
  await clock.advance(CALM_MS);
  const rates: number[] = [];
  for (let round = 0; round < 4; round += 1) {
    await batch(300 + round * 100);
    rates.push(fetcher.status().rate);
  }
  assert.deepEqual(rates, [2.25, 2.75, 3, 3]);
  // The proven limit survives a restart.
  await fetcher.flush();
  const stats = JSON.parse(fs.readFileSync(path.join(store.root, 'stats.json'), 'utf8'));
  assert.equal(stats.version, 1);
  assert.equal(stats.safeRate, 3);
  assert.equal(stats.ceiling, 3);
  assert.ok(stats.tiers.m1.samples >= 300);
  assert.equal(stats.tiers.m1.bytesPerBucket, 200);
  const idle = async () => answer(500);
  const again = new Fetcher(store, { clock, fetch: idle });
  fetchers.push(again);
  assert.equal(again.status().ceiling, 3);
  assert.equal(again.status().safeRate, 3);
  assert.equal(again.status().rate, 3);
  assert.equal(again.tierStats('m1').bytesPerBucket, 200);
  fs.writeFileSync(path.join(store.root, 'stats.json'), JSON.stringify({ ...stats, ceiling: 1.5 }));
  const slow = new Fetcher(store, { clock, fetch: idle });
  fetchers.push(slow);
  assert.equal(slow.status().rate, 1.5);
});

test('an idle streak says nothing about a faster rate', async () => {
  const { req, clock, fetcher } = setup();
  await clock.advance(CALM_MS);
  for (let i = 0; i < 60; i += 1) {
    await req(day(i));
    await clock.advance(10_000);
  }
  assert.equal(fetcher.status().rate, 3);
  assert.equal(fetcher.status().safeRate, null);
});

test('a failed request is retried through the queue with backoff, never inside the worker', async () => {
  let failures = 2;
  const { req, calls, clock, fetcher } = setup({
    respond: () => (failures-- > 0 ? answer(500) : ok()),
  });
  const p = track(req(day(0)));
  await clock.advance(0);
  assert.equal(calls.length, 1);
  assert.equal(fetcher.isPending('eurusd', 'm1', day(0)), true);
  // Backoff with random() = 0: 0.5 s, then 1 s.
  await clock.advance(499);
  assert.equal(calls.length, 1);
  await clock.advance(1);
  assert.equal(calls.length, 2);
  await clock.advance(999);
  assert.equal(calls.length, 2);
  await clock.advance(1);
  assert.equal(calls.length, 3);
  assert.equal((p.value as FetchResult).status, 'fetched');
  // Jitter spreads the wait between half and all of the backoff.
  const jitter = setup({ respond: () => answer(500), random: () => 0.999 });
  void jitter.req(day(0));
  await jitter.clock.advance(998);
  assert.equal(jitter.calls.length, 1);
  await jitter.clock.advance(2);
  assert.equal(jitter.calls.length, 2);
});

test('a request that hangs is aborted after 30 s and retried', async () => {
  const m = manual();
  const { req, calls, clock, respondWith } = setup({ respond: m.respond });
  const p = req(day(0));
  await clock.advance(REQUEST_TIMEOUT_MS - 1);
  assert.equal(calls.length, 1);
  respondWith(() => ok());
  await clock.advance(1);
  await clock.advance(1_000);
  assert.equal(calls.length, 2);
  assert.equal((await p).status, 'fetched');
});

test('the circuit breaker: 8 failures pause the bulk lane, a probe after the backoff closes it', async () => {
  const { req, calls, clock, fetcher, events, respondWith } = setup({ respond: () => answer(500) });
  const bulk = Array.from({ length: BREAKER_FAILURES }, (_, i) => track(req(day(i), 'bulk', { jobId: 'j' })));
  await clock.advance(BREAKER_FAILURES * 334);
  assert.equal(calls.length, BREAKER_FAILURES);
  const b = fetcher.breaker;
  assert.equal(b.state, 'open');
  assert.equal(b.reason, 'throttled');
  assert.equal(b.message, 'Dukascopy is throttling; retrying in 5 min');
  // Opened by the 8th failure, 5 minutes before its retry.
  const openedAt = b.retryAt! - 5 * 60_000;
  assert.ok(openedAt > NOW * 1000 && openedAt <= clock.t);
  assert.ok(events.some((e) => e.type === 'breaker' && e.breaker.state === 'open'));
  // While open: bulk waits (nothing is marked failed), interactive still goes out.
  await clock.advance(60_000);
  assert.equal(calls.length, BREAKER_FAILURES);
  assert.equal(
    bulk.some((p) => p.settled),
    false,
  );
  await assert.rejects(req(day(50), 'interactive', { failFast: true }), (e: unknown) => {
    return e instanceof FetchError && e.code === 'THROTTLED';
  });
  const interactive = track(req(day(40), 'interactive'));
  await clock.advance(400);
  assert.equal(calls.length, BREAKER_FAILURES + 1);
  assert.equal(calls[calls.length - 1]!.url, url(day(40)));
  assert.equal(interactive.settled, false, 'failed and requeued');
  // After the backoff one bulk probe goes out alone; it fails, so the wait doubles.
  await clock.advance(openedAt + 5 * 60_000 - clock.t - 1);
  const beforeProbe = calls.length;
  await clock.advance(1);
  await clock.advance(5_000);
  const probes = calls.slice(beforeProbe).filter((c) => c.url !== url(day(40)));
  assert.equal(probes.length, 1);
  assert.equal(fetcher.breaker.state, 'open');
  assert.equal(fetcher.breaker.message, 'Dukascopy is throttling; retrying in 10 min');
  // The next probe succeeds: closed, and the queue drains.
  respondWith(() => ok());
  await clock.advance(10 * 60_000 + 5_000);
  assert.equal(fetcher.breaker.state, 'closed');
  await clock.advance(60_000);
  assert.equal(
    bulk.every((p) => p.settled && (p.value as FetchResult).status === 'fetched'),
    true,
  );
  assert.equal(interactive.settled, true);
});

test('offline opens the breaker at once and says so', async () => {
  const offline = () => Promise.reject(new TypeError('fetch failed', { cause: { code: 'ENOTFOUND' } }));
  const { req, clock, fetcher, respondWith } = setup({ respond: offline });
  const p = track(req(day(0)));
  const fast = track(req(day(1), 'interactive', { failFast: true }));
  await clock.advance(0);
  assert.deepEqual(fetcher.breaker, {
    state: 'open',
    reason: 'offline',
    message: 'Offline',
    retryAt: NOW * 1000 + 30_000,
  });
  await clock.advance(1_000);
  assert.equal(fast.settled && fast.error instanceof FetchError && fast.error.code, 'OFFLINE');
  respondWith(() => ok());
  await clock.advance(31_000);
  assert.equal(fetcher.breaker.state, 'closed');
  assert.equal((p.value as FetchResult).status, 'fetched');
});

test('400 and 404 are permanent: the bucket is recorded unavailable and never asked again', async () => {
  const store = new FakeStore(tempDir());
  const { req, calls, clock } = setup({ store, respond: (u) => answer(u === url(day(0)) ? 404 : 400) });
  const a = req(day(0));
  const b = req(day(1));
  await clock.advance(2_000);
  assert.deepEqual(await a, { status: 'unavailable', http: 404 });
  assert.deepEqual(await b, { status: 'unavailable', http: 400 });
  assert.equal(store.status('eurusd', 'm1', day(0)), 'unavailable');
  assert.deepEqual(await req(day(0)), { status: 'unavailable', http: null });
  assert.equal(calls.length, 2);
});

test("400 'From time is too late' just after the close is retried with backoff, never recorded or counted", async () => {
  const store = new FakeStore(tempDir());
  const end = s('2024-06-01T00:00:00Z');
  const closed = end - 86400;
  let early = 10;
  const { req, calls, clock, fetcher } = setup({
    store,
    // Our clock says the day closed 0.3 s ago; Dukascopy has not built it yet (or its clock is behind).
    start: end + 0.3,
    respond: (u) => (u === url(day(0)) || early-- > 0 ? tooLate() : ok(end + 1)),
  });
  const p = track(req(closed, 'interactive'));
  await clock.advance(0);
  assert.deepEqual(calls[0]?.url, url(closed));
  // Ten answers take 0.5 + 1 + 2 + ... s of backoff: more than the breaker's failures, never counted.
  await clock.advance(200_000);
  assert.equal(calls.length, 9);
  assert.equal(store.status('eurusd', 'm1', closed), 'missing');
  assert.equal(fetcher.breaker.state, 'closed');
  assert.equal(p.settled, false);
  await clock.advance(300_000);
  assert.equal(calls.length, 11);
  assert.equal((p.value as FetchResult).status, 'fetched');
  assert.equal(store.status('eurusd', 'm1', closed), 'provisional');
  assert.equal(fetcher.breaker.state, 'closed');
  // A bucket that closed long ago and is "too late" is answered for good.
  const old = track(req(day(0)));
  await clock.advance(1_000);
  assert.deepEqual(old.value, { status: 'unavailable', http: 400 });
  assert.equal(store.status('eurusd', 'm1', day(0)), 'unavailable');
});

test('an active request that crosses the close stores nothing, then asks the completed URL after the grace', async () => {
  const cache = tempDir();
  const D = s('2024-06-11T00:00:00Z');
  const end = nextBucket('m1', D);
  let nowMs = () => (end - 120) * 1000;
  const store = new DukascopyStore(cache, { now: () => nowMs() });
  // Day D stored while open, two minutes before its close.
  await store.put('eurusd', 'm1', D, candles(1438, D), { builtAt: end - 120, final: false });
  const { req, calls, clock, urls, fetcher } = setup({
    store,
    // 23:59:59.9 here; Dukascopy's clock (or the request's arrival) is 0.3 s later, past midnight.
    start: end - 0.1,
    skewMs: 300,
    // `?from=` answers Dukascopy's current day, still empty; the completed URL the whole day.
    respond: (u) => (u.includes('?from=') ? ok(end, 0) : ok(end + 1, 1440)),
  });
  nowMs = () => clock.t;
  const request = req(D, 'interactive');
  const p = track(request);
  await clock.advance(0);
  // The answer has been handled (stored or not) once nothing is in flight; the store writes real files.
  while (fetcher.status().inFlight > 0) await new Promise((resolve) => setTimeout(resolve, 5));
  assert.deepEqual(urls(), [activeUrl('EUR-USD', 'm1', D)]);
  // The next day's empty answer did not wipe day D.
  assert.equal(store.status('eurusd', 'm1', D), 'provisional');
  assert.equal(store.sets('eurusd', 'm1').empty.isEmpty, true);
  assert.equal((await store.readNative('eurusd', 'm1', D, end)).length, 1438);
  assert.equal(p.settled, false);
  await clock.advance((end + CLOSE_GRACE_SECONDS) * 1000 - 1 - clock.t);
  assert.equal(calls.length, 1);
  await clock.advance(1);
  assert.deepEqual(urls(), [activeUrl('EUR-USD', 'm1', D), completedUrl('EUR-USD', 'm1', D)]);
  assert.equal(calls[1]!.at, (end + CLOSE_GRACE_SECONDS) * 1000);
  const result = await request;
  assert.equal(result.status === 'fetched' && result.bars, 1440);
  assert.equal((await store.readNative('eurusd', 'm1', D, end)).length, 1440);
  // CloudFront's first copy: provisional, due once its 7-day copy expired.
  assert.equal(store.status('eurusd', 'm1', D), 'provisional');
  assert.equal(store.needsFetch('eurusd', 'm1', D, end + 1 + DUE_AFTER_SECONDS - 1), false);
  assert.equal(store.needsFetch('eurusd', 'm1', D, end + 1 + DUE_AFTER_SECONDS), true);
});

test('any other 200 for another bucket is never stored', async () => {
  const store = new FakeStore(tempDir());
  // A completed URL answered with the next day's data: retried a few times, then it fails.
  const wrong = setup({
    store,
    respond: () => new Response(candles(2, day(1)), { headers: { date: httpDate(NOW) } }),
  });
  const p = track(wrong.req(day(0)));
  await wrong.clock.advance(10 * 60_000);
  assert.equal(wrong.calls.length, MAX_INVALID_ATTEMPTS);
  assert.ok(p.error instanceof FetchError && p.error.code === 'INVALID_RESPONSE');
  assert.match(p.error.message, /the answer was for 2024-03-06/);
  assert.deepEqual(store.puts, []);
  assert.equal(wrong.fetcher.breaker.state, 'closed');
  // `?from=` answered by a Dukascopy whose clock is still in the previous day: asked again shortly.
  const today = s('2024-06-01T00:00:00Z');
  const behind = setup({ start: today + 1, skewMs: -1_200 });
  const q = track(behind.req(today, 'interactive'));
  await behind.clock.advance(0);
  assert.equal(behind.calls.length, 1);
  assert.equal(q.settled, false);
  await behind.clock.advance(5_000);
  assert.deepEqual(behind.urls(), [activeUrl('EUR-USD', 'm1', today), activeUrl('EUR-USD', 'm1', today)]);
  assert.equal((q.value as FetchResult).status, 'fetched');
  assert.deepEqual(
    (behind.store as FakeStore).puts.map((put) => put.key),
    [`eurusd/m1/${today}`],
  );
});

test('a 429 at or below the proven rate lowers ceiling and proven rate below it, and both persist', async () => {
  const store = new FakeStore(tempDir());
  const statsFile = path.join(store.root, 'stats.json');
  fs.writeFileSync(statsFile, JSON.stringify({ version: 1, safeRate: 3, ceiling: 3, tiers: {} }));
  const { req, clock, fetcher, respondWith } = setup({ store, respond: () => answer(429) });
  assert.equal(fetcher.status().rate, 3);
  // Dukascopy's limit dropped below 3 (other traffic from this address): the 429 comes at the proven rate.
  const throttled = req(day(0));
  await clock.advance(0);
  assert.deepEqual([fetcher.status().rate, fetcher.status().ceiling, fetcher.status().safeRate], [1.5, 2.5, 2.5]);
  respondWith(() => ok());
  await clock.advance(70_000);
  assert.equal((await throttled).status, 'fetched');
  await fetcher.flush();
  const stats = JSON.parse(fs.readFileSync(statsFile, 'utf8'));
  assert.deepEqual([stats.ceiling, stats.safeRate], [2.5, 2.5]);
  // The climb back stops a step short of the rate that failed.
  await clock.advance(CALM_MS);
  const rates: number[] = [];
  for (let round = 0; round < 3; round += 1) {
    const all = Array.from({ length: 50 }, (_, i) => req(day(100 + round * 100 + i)));
    await clock.advance(60_000);
    await Promise.all(all);
    rates.push(fetcher.status().rate);
  }
  assert.deepEqual(rates, [2, 2.5, 2.5]);
  // ...and after a restart.
  const again = new Fetcher(store, { clock, fetch: async () => answer(500) });
  fetchers.push(again);
  assert.deepEqual([again.status().rate, again.status().ceiling, again.status().safeRate], [2.5, 2.5, 2.5]);
  // A 503 may be an outage rather than the limit: it halves the rate, the ceiling stays at the proven rate.
  const outageStore = new FakeStore(tempDir());
  fs.writeFileSync(path.join(outageStore.root, 'stats.json'), JSON.stringify({ version: 1, safeRate: 3, ceiling: 3 }));
  const outage = setup({ store: outageStore, respond: () => answer(503) });
  void outage.req(day(0));
  await outage.clock.advance(0);
  assert.deepEqual([outage.fetcher.status().rate, outage.fetcher.status().ceiling], [1.5, 3]);
});

test('finality from the Date header, stored in the real store', async () => {
  const cache = tempDir();
  const store = new DukascopyStore(cache, { now: () => NOW * 1000 });
  const end0 = nextBucket('m1', day(0));
  const end1 = nextBucket('m1', day(1));
  const end2 = nextBucket('m1', day(2));
  const end3 = nextBucket('m1', day(3));
  const responses: Record<string, () => Answer> = {
    // Built a second after the close: CloudFront's first copy, maybe missing the last minute.
    [url(day(0))]: () => ok(end0 + 1),
    // Built two hours after the close: final.
    [url(day(1))]: () => ok(end1 + 7200),
    // No Date header: receive time minus Age.
    [url(day(2))]: () => new Response(candles(2, day(2)), { headers: { age: String(NOW - end2 - 7200) } }),
    // A partial copy CloudFront cached while the day was still open.
    [url(day(3))]: () => ok(end3 - 3600),
  };
  const { fetcher, clock } = setup({ store, respond: (u) => responses[u]!() });
  const results = [0, 1, 2, 3].map((i) =>
    fetcher.request({ instrument: 'eurusd', tier: 'm1', start: day(i), lane: 'bulk' }),
  );
  await clock.advance(5_000);
  const [r0, r1, r2, r3] = await Promise.all(results);
  assert.deepEqual(r0, {
    status: 'fetched',
    final: false,
    empty: false,
    bars: 2,
    bytes: (r0 as { bytes: number }).bytes,
  });
  assert.equal(r1!.status === 'fetched' && r1!.final, true);
  assert.equal(r2!.status === 'fetched' && r2!.final, true);
  assert.equal(r3!.status === 'fetched' && r3!.final, false);
  assert.equal(store.status('eurusd', 'm1', day(0)), 'provisional');
  assert.equal(store.status('eurusd', 'm1', day(1)), 'final');
  assert.equal(store.status('eurusd', 'm1', day(2)), 'final');
  await store.flush();
  const manifest = JSON.parse(fs.readFileSync(path.join(cache, 'dukascopy', 'eurusd', 'manifest.json'), 'utf8'));
  assert.equal(manifest.tiers.m1.provisional[day(0)], end0 + 1);
  // The cached partial copy counts as built at the close, so its one re-fetch waits out CloudFront.
  assert.equal(manifest.tiers.m1.provisional[day(3)], end3);
  assert.equal((await store.readNative('eurusd', 'm1', day(0), day(4))).length, 8);
});

test('responses that cannot be stored are retried a few times, then fail', async () => {
  const { req, calls, clock } = setup({
    respond: () => new Response('<html>oops</html>', { headers: { date: httpDate(NOW) } }),
  });
  const p = track(req(day(0)));
  await clock.advance(10 * 60_000);
  assert.equal(calls.length, MAX_INVALID_ATTEMPTS);
  assert.ok(p.error instanceof FetchError && p.error.code === 'INVALID_RESPONSE');
  const store = new FakeStore(tempDir());
  store.putError = Object.assign(new Error('disk full'), { code: 'ENOSPC' });
  const disk = setup({ store });
  const q = track(disk.req(day(0)));
  await disk.clock.advance(10 * 60_000);
  assert.ok(q.error instanceof FetchError && q.error.code === 'STORE');
  // A local disk problem is not Dukascopy's fault: the breaker stays closed.
  assert.equal(disk.fetcher.breaker.state, 'closed');
});

test('unknown instruments fail at once, without a request', async () => {
  const { fetcher, calls } = setup();
  await assert.rejects(
    fetcher.request({ instrument: 'nosuchthing', tier: 'm1', start: DAY, lane: 'interactive' }),
    (e: unknown) => e instanceof FetchError && e.code === 'UNKNOWN_INSTRUMENT',
  );
  assert.equal(calls.length, 0);
});

test('live stats and rolling per-tier averages', async () => {
  const { req, clock, fetcher } = setup({ respond: () => ok(NOW, 3, { 'content-length': '900' }) });
  const all = [
    req(day(0)),
    req(day(1)),
    fetcher.request({ instrument: 'eurusd', tier: 'h1', start: DAY, lane: 'bulk' }),
  ];
  await clock.advance(1_000);
  await Promise.all(all);
  const st = fetcher.status();
  assert.equal(st.inFlight, 0);
  assert.deepEqual(st.queued, { interactive: 0, waiting: 0, bulk: 0 });
  assert.equal(st.bytesPerSecond, 270);
  assert.equal(fetcher.tierStats('m1').samples, 2);
  assert.equal(fetcher.tierStats('m1').bytesPerBucket, 300);
  assert.equal(fetcher.tierStats('h1').samples, 1);
  assert.equal(fetcher.tierStats('d1').bytesPerBucket, 4 * 1024);
  assert.equal(fetcher.effectiveRate(), 3);
  await clock.advance(11_000);
  assert.equal(fetcher.status().bytesPerSecond, 0);
});

test('close cancels queued waiters and writes the stats', async () => {
  const { req, fetcher, store } = setup();
  const first = req(day(0));
  const queued = req(day(1));
  await fetcher.close();
  assert.deepEqual(await queued, { status: 'cancelled' });
  assert.equal((await first).status, 'fetched');
  assert.deepEqual(await req(day(2)), { status: 'cancelled' });
  assert.equal(fs.existsSync(path.join(store.root, 'stats.json')), true);
});
