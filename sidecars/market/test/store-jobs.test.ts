// Download jobs over a real store in a temp dir, with a fetcher double answering from synthetic
// buckets (no network).

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, test } from 'node:test';

import { type Bar, RpcError } from '../src/protocol.ts';
import { type Tier } from '../src/store/buckets.ts';
import { DukascopyStore } from '../src/store/dukascopy-store.ts';
import { Jobs, type TvSession, isProbe } from '../src/store/jobs.ts';
import { Planner } from '../src/store/planner.ts';
import { Series } from '../src/store/series.ts';
import { TvStore } from '../src/store/tv-store.ts';
import { walkBack } from '../src/tradingview.ts';
import { DAY, FakeFetcher, bucketBars, bucketBody, cleanup, iso, s, tempDir, until } from './store-helpers.ts';

const NOW = s('2024-06-01T12:00:00Z');
const opened: Jobs[] = [];

afterEach(async () => {
  for (const jobs of opened.splice(0)) await jobs.close();
  cleanup();
});

type Data = (instrument: string, tier: Tier, start: number) => Bar[] | 'unavailable';

/** Every bucket full of forex-session bars (m1 thinned to one bar an hour). */
const fx: Data = (_i, tier, start) => bucketBars(tier, start, { every: tier === 'm1' ? 3600 : undefined });

function harness(data: Data = fx, opts: { cache?: string; tvSession?: TvSession } = {}) {
  const cache = opts.cache ?? tempDir();
  const clock = () => NOW * 1000;
  const store = new DukascopyStore(cache, { now: clock, manifestDelayMs: 20 });
  const tv = new TvStore(cache, { now: clock, flushDelayMs: 20 });
  const series = new Series({ store, tv, now: clock });
  const fetcher = new FakeFetcher(store, data, clock);
  const events: Array<{ event: string; job: { id: string; status: string } }> = [];
  let jobs: Jobs | null = null;
  const planner = new Planner({ store, fetcher, tv, series, jobs: () => jobs?.views() ?? [], now: clock });
  jobs = new Jobs({
    cacheDir: cache,
    store,
    fetcher,
    planner,
    series,
    tv,
    tvSession: opts.tvSession,
    emit: (event, params) => events.push({ event, job: (params as { job: { id: string; status: string } }).job }),
    now: clock,
  });
  opened.push(jobs);
  return { cache, store, tv, series, fetcher, planner, jobs, events };
}

const status = (jobs: Jobs, id: string) => jobs.get(id)?.status ?? null;
const day = (key: string) => iso(Number(key.split('/')[2])).slice(0, 10);

test('a resumed job re-plans from the manifest and never fetches a bucket twice', async () => {
  const a = harness();
  a.fetcher.hold = true;
  const range = {
    symbol: 'FX:EURUSD',
    from: s('2024-03-01T00:00:00Z'),
    to: s('2024-04-01T00:00:00Z'),
    origin: 'chat' as const,
  };
  const { jobId, plan } = await a.jobs.start(range);
  assert.deepEqual(plan.tiers, ['m1']);
  assert.equal(plan.requests, 31, 'every 1-minute day of March, weekends included');
  for (let i = 0; i < 12; i += 1) {
    await until(() => a.fetcher.held > 0, 5000, 'a held request');
    a.fetcher.release(1);
  }
  await until(() => a.jobs.get(jobId)!.done >= 10, 5000, 'ten requests done');
  // The app exits: the job keeps its status and resumes on the next start.
  await a.jobs.close();
  // Let a request that was already past the queue land before the store flushes.
  await new Promise((resolve) => setTimeout(resolve, 50));
  await a.store.close();
  const saved = JSON.parse(fs.readFileSync(path.join(a.cache, 'jobs', `${jobId}.json`), 'utf8'));
  assert.equal(saved.status, 'running');
  assert.ok(saved.done >= 10);

  const b = harness(fx, { cache: a.cache });
  await b.jobs.load();
  await until(() => status(b.jobs, jobId) === 'done', 10_000, 'the resumed job to finish');
  const all = [...a.fetcher.requests, ...b.fetcher.requests];
  assert.equal(new Set(all).size, all.length, 'no bucket was fetched twice');
  assert.ok(b.fetcher.requests.length > 0);
  const job = b.jobs.get(jobId)!;
  assert.equal(job.done, job.total);
  // Only 1-minute days, Saturdays included (nothing coarser is fetched to infer them from).
  assert.ok(all.every((k) => k.includes('/m1/')));
  assert.ok(all.some((k) => new Date(Number(k.split('/')[2]) * 1000).getUTCDay() === 6));
  assert.deepEqual(b.series.missing('EURUSD', '1m', range.from, range.to - 1), []);
  assert.ok(b.events.some((e) => e.event === 'download.done' && e.job.id === jobId));
  assert.equal((await b.planner.plan(range)).complete, true);
});

test('a range inside an unfinished job reuses it (and resumes it when paused)', async () => {
  const { jobs, fetcher } = harness();
  fetcher.hold = true;
  const march = {
    symbol: 'FX:EURUSD',
    from: s('2024-03-01T00:00:00Z'),
    to: s('2024-04-01T00:00:00Z'),
    origin: 'chart' as const,
  };
  const first = await jobs.start(march);
  const inside = await jobs.start({
    ...march,
    symbol: 'EURUSD',
    from: s('2024-03-10T00:00:00Z'),
    to: s('2024-03-20T00:00:00Z'),
  });
  assert.equal(inside.jobId, first.jobId);
  assert.equal(inside.plan.job?.id, first.jobId);
  const fewerTiers = await jobs.start({ ...march, tiers: ['m1'] });
  assert.equal(fewerTiers.jobId, first.jobId);
  const wider = await jobs.start({ ...march, from: s('2024-02-01T00:00:00Z'), to: s('2024-03-15T00:00:00Z') });
  assert.notEqual(wider.jobId, first.jobId);

  await jobs.pause(first.jobId);
  assert.equal(status(jobs, first.jobId), 'paused');
  const again = await jobs.start(march);
  assert.equal(again.jobId, first.jobId);
  assert.ok(['queued', 'running'].includes(status(jobs, first.jobId)!));
  assert.deepEqual(
    jobs
      .list('FX:EURUSD')
      .map((j) => j.id)
      .sort(),
    [first.jobId, wider.jobId].sort(),
  );
  assert.deepEqual(jobs.list('GBPCHF'), []);
  fetcher.release();
});

test('pause and cancel purge queued work; cancel removes the record', async () => {
  const { cache, jobs, fetcher } = harness();
  fetcher.hold = true;
  const range = {
    symbol: 'EURUSD',
    from: s('2024-03-01T00:00:00Z'),
    to: s('2024-04-01T00:00:00Z'),
    origin: 'data' as const,
  };
  const { jobId } = await jobs.start(range);
  await until(() => fetcher.held > 0, 5000, 'queued requests');
  await jobs.pause(jobId);
  assert.equal(status(jobs, jobId), 'paused');
  assert.ok(fetcher.cancelled.includes(`job:${jobId}`), 'the job was purged from the queue');
  const before = fetcher.requests.length;
  fetcher.release();
  await new Promise((resolve) => setTimeout(resolve, 50));
  assert.equal(fetcher.requests.length, before, 'nothing ran for a paused job');

  jobs.resume(jobId);
  await until(() => status(jobs, jobId) === 'done', 10_000, 'the job to finish');
  assert.ok(fs.existsSync(path.join(cache, 'jobs', `${jobId}.json`)));

  fetcher.hold = true;
  const other = await jobs.start({ ...range, from: s('2024-01-01T00:00:00Z'), to: s('2024-02-01T00:00:00Z') });
  await until(() => fetcher.held > 0, 5000, 'queued requests');
  const count = fetcher.requests.length;
  await jobs.cancel(other.jobId);
  fetcher.release();
  await new Promise((resolve) => setTimeout(resolve, 50));
  assert.equal(jobs.get(other.jobId), null);
  assert.equal(fs.existsSync(path.join(cache, 'jobs', `${other.jobId}.json`)), false);
  assert.equal(fetcher.requests.length, count);
  // A finished job stays (old chat cards still resolve) until it is cancelled.
  assert.equal(status(jobs, jobId), 'done');
  await jobs.cancel(jobId);
  assert.equal(jobs.get(jobId), null);
});

test('an empty streak probed monthly down to the start records the learned start and skips older days', async () => {
  const start = s('2024-03-04T00:00:00Z');
  // 1-minute data only from Monday 2024-03-04 (the metadata says 2003).
  const data: Data = (_i, tier, b) => bucketBars(tier, b, { every: 3600, from: start });
  const { store, jobs, fetcher, planner } = harness(data);
  // Everything: from the metadata's start in 2003 up to now.
  const { jobId, plan } = await jobs.start({ symbol: 'EURUSD', origin: 'chat' });
  assert.equal(plan.from, s('2003-05-04T19:00:00Z'));
  await until(() => status(jobs, jobId) === 'done', 30_000, 'the job to finish');
  const learned = store.learnedStart('eurusd', 'm1');
  assert.equal(learned?.t, start);
  assert.equal((learned!.evidence as { reason: string }).reason, 'no data in monthly probes down to the start');
  // Below the streak (and the few days already asked for when it was noticed) only probes were asked
  // for: one weekday a month from December 2023 back to May 2003, not every day for 20 years.
  const old = fetcher.requests.filter((k) => day(k) < '2024-01-01');
  assert.equal(old.length, 248, `${old.length} probes`);
  assert.ok(
    old.every((k) => isProbe('m1', Number(k.split('/')[2]))),
    old.map(day).join(' '),
  );
  assert.ok(
    fetcher.requests.every((k) => k.includes('/m1/')),
    'nothing coarser is fetched',
  );
  const job = jobs.get(jobId)!;
  assert.equal(job.done, job.total);
  assert.ok(job.skipped > 5000, `${job.skipped} days skipped`);
  // The learned start moves the history's start: nothing is left to plan.
  assert.equal(store.effectiveStart('eurusd', 'm1'), start);
  assert.equal((await planner.plan({ symbol: 'EURUSD' })).requests, 0);
});

test('an empty streak with older data below it is a hole: probe sparsely, walk densely where data is', async () => {
  // Nothing from 2023-12-01 to 2024-02-29; data before and after.
  const data: Data = (_i, tier, b) =>
    tier === 'm1' && b >= s('2023-12-01T00:00:00Z') && b < s('2024-03-01T00:00:00Z')
      ? []
      : bucketBars(tier, b, { every: tier === 'm1' ? 3600 : undefined });
  const { store, jobs, fetcher, series, planner } = harness(data);
  const range = {
    symbol: 'EURUSD',
    from: s('2023-10-01T00:00:00Z'),
    to: s('2024-04-01T00:00:00Z'),
    tiers: ['m1', 'h1'],
  };
  const { jobId } = await jobs.start({ ...range, origin: 'chat' });
  await until(() => status(jobs, jobId) === 'done', 10_000, 'the job to finish');
  assert.equal(store.learnedStart('eurusd', 'm1'), null);
  const m1 = fetcher.requests.filter((k) => k.includes('/m1/')).map(day);
  const days = new Set(m1);
  assert.ok(isProbe('m1', s('2023-12-20T00:00:00Z')) && isProbe('m1', s('2023-11-15T00:00:00Z')));
  assert.ok(m1.indexOf('2023-12-20') < m1.indexOf('2023-12-05'), 'probed December before walking it densely');
  assert.ok(days.has('2023-11-15'), 'probed November, and found data');
  // Dense again from just below the streak: the data days above the probe and the hole days the probes
  // passed over are fetched too, so the range ends covered.
  for (const d of ['2023-11-20', '2023-11-30', '2023-12-05', '2024-01-10', '2023-11-14', '2023-10-02'])
    assert.ok(days.has(d), `${d} was fetched`);
  assert.equal(new Set(fetcher.requests).size, fetcher.requests.length, 'no bucket was fetched twice');
  const job = jobs.get(jobId)!;
  assert.equal(job.done, job.total);
  assert.equal(job.skipped, 0);
  assert.deepEqual(series.missing('EURUSD', '1m', range.from, range.to - 1), []);
  assert.equal((await planner.plan(range)).complete, true);
  // Nothing is left for the same download to do.
  const before = fetcher.requests.length;
  const again = await jobs.start({ ...range, origin: 'chat' });
  await until(() => status(jobs, again.jobId) === 'done', 10_000, 'the second job to finish');
  assert.equal(fetcher.requests.length, before);
});

test('a six-week hole is found by the monthly probes alone: nothing coarser is fetched', async () => {
  const data: Data = (_i, tier, b) =>
    b >= s('2023-12-01T00:00:00Z') && b < s('2024-01-15T00:00:00Z') ? [] : bucketBars(tier, b, { every: 3600 });
  const { store, jobs, fetcher, series, planner } = harness(data);
  const range = { symbol: 'EURUSD', from: s('2023-10-01T00:00:00Z'), to: s('2024-02-01T00:00:00Z') };
  const { jobId } = await jobs.start({ ...range, origin: 'chat' });
  await until(() => status(jobs, jobId) === 'done', 10_000, 'the job to finish');
  assert.equal(store.learnedStart('eurusd', 'm1'), null, 'a hole, not the start of the history');
  assert.ok(
    fetcher.requests.every((k) => k.includes('/m1/')),
    fetcher.requests.join(' '),
  );
  assert.ok(new Set(fetcher.requests.map(day)).has('2023-11-20'));
  assert.deepEqual(series.missing('EURUSD', '1m', range.from, range.to - 1), []);
  assert.equal((await planner.plan({ ...range, to: s('2023-12-01T00:00:00Z') })).complete, true);
  const job = jobs.get(jobId)!;
  assert.equal(job.done, job.total);
});

test('a download asking for hourly or daily candles downloads 1-minute days', async () => {
  const { jobs, fetcher, series } = harness();
  const range = { symbol: 'EURUSD', from: s('2024-03-01T00:00:00Z'), to: s('2024-03-15T00:00:00Z') };
  const { jobId, plan } = await jobs.start({ ...range, tiers: ['h1', 'd1'], origin: 'chat' });
  assert.deepEqual(plan.tiers, ['m1']);
  assert.equal(plan.requests, 14);
  await until(() => status(jobs, jobId) === 'done', 10_000, 'the job to finish');
  assert.deepEqual(jobs.get(jobId)!.tiers, ['m1']);
  assert.equal(fetcher.requests.length, 14);
  assert.ok(fetcher.requests.every((k) => k.includes('/m1/')));
  // Every timeframe reads them.
  for (const tf of ['1m', '1h', '1d'] as const) {
    assert.deepEqual(series.missing('EURUSD', tf, range.from, range.to - 1), [], tf);
  }
});

test('a job re-fetches a day stored while still open once it is due', async () => {
  const { store, jobs, fetcher } = harness();
  const thu = s('2024-05-23T00:00:00Z');
  // Stored at 15:00 while the day was open: bars up to 14:59 only.
  const partial = bucketBars('m1', thu, { every: 3600, to: thu + 15 * 3600 });
  await store.put('eurusd', 'm1', thu, bucketBody('m1', thu, partial), { builtAt: thu + 15 * 3600, final: false });
  assert.equal(store.needsFetch('eurusd', 'm1', thu), true);
  const { jobId } = await jobs.start({ symbol: 'EURUSD', from: thu, to: thu + DAY, tiers: ['m1'], origin: 'chat' });
  await until(() => status(jobs, jobId) === 'done', 10_000, 'the job to finish');
  assert.ok(fetcher.requests.includes(`eurusd/m1/${thu}`), 'the due provisional day was asked for again');
  assert.equal(store.status('eurusd', 'm1', thu), 'final');
  const job = jobs.get(jobId)!;
  assert.deepEqual([job.done, job.total], [1, 1]);
});

test('every planned bucket covered while a job runs counts for it with its bytes, whoever fetched it', async () => {
  const { cache, store, jobs, fetcher } = harness();
  fetcher.hold = true;
  const range = { symbol: 'EURUSD', from: s('2024-03-01T00:00:00Z'), to: s('2024-04-01T00:00:00Z') };
  const { jobId, plan } = await jobs.start({ ...range, origin: 'chat' });
  await until(() => fetcher.held > 0, 5000, 'queued requests');
  // A chart (no job) stores a few of the job's m1 days meanwhile.
  for (const d of ['2024-03-25', '2024-03-12', '2024-03-05']) {
    const t = s(`${d}T00:00:00Z`);
    await store.put('eurusd', 'm1', t, bucketBody('m1', t, fx('eurusd', 'm1', t) as Bar[]), {
      builtAt: t + DAY + 7200,
      final: true,
    });
  }
  fetcher.release();
  await until(() => status(jobs, jobId) === 'done', 10_000, 'the job to finish');
  const job = jobs.get(jobId)!;
  // Nothing leaves the plan: the chart's days count as done.
  assert.equal(job.total, plan.requests, 'the total shrank below the plan');
  assert.equal(job.done, job.total);
  assert.equal(job.skipped, 0);
  // Its bytes are every file of its range, the chart's included.
  let files = 0;
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      if (e.isDirectory()) walk(path.join(dir, e.name));
      else if (e.name.endsWith('.json.gz')) files += fs.statSync(path.join(dir, e.name)).size;
    }
  };
  walk(path.join(cache, 'dukascopy', 'eurusd'));
  await until(() => jobs.get(jobId)!.bytes === files, 2000, `bytes ${jobs.get(jobId)!.bytes} to be ${files}`);
});

test('a boost counts for the job waited on; a whole-job wait does not boost', async () => {
  const { jobs, fetcher } = harness();
  fetcher.hold = true;
  const range = { symbol: 'EURUSD', from: s('2024-03-01T00:00:00Z'), to: s('2024-04-01T00:00:00Z') };
  const { jobId } = await jobs.start({ ...range, origin: 'chat' });
  await until(() => fetcher.held > 0, 5000, 'queued requests');
  const whole = await jobs.wait({ ...range, tf: '1m', jobId, timeoutMs: 300, boost: false });
  assert.equal(whole.covered, false);
  assert.ok(!fetcher.calls.some((c) => c.startsWith('waiting ')), 'no boost for a whole-job wait');

  const waiting = jobs.wait({ ...range, tf: '1h', jobId, timeoutMs: 5000 });
  await until(() => fetcher.calls.some((c) => c.startsWith('waiting ')), 5000, 'a boost');
  fetcher.release();
  assert.equal((await waiting).covered, true);
  const boosts = fetcher.calls.filter((c) => c.startsWith('waiting '));
  // Every boost is a 1-minute day (1h is built from them) and names the job.
  assert.ok(
    boosts.every((c) => c.startsWith(`waiting ${jobId} `) && c.includes('/m1/')),
    boosts.join(' | '),
  );
  await until(() => status(jobs, jobId) === 'done', 10_000, 'the job to finish');
  const job = jobs.get(jobId)!;
  assert.equal(job.done, job.total);
  assert.equal(job.skipped, 0);
});

test('running jobs wait while the fetcher breaker is open', async () => {
  const { jobs, fetcher } = harness();
  fetcher.hold = true;
  const { jobId } = await jobs.start({
    symbol: 'EURUSD',
    from: s('2024-03-01T00:00:00Z'),
    to: s('2024-04-01T00:00:00Z'),
    origin: 'chat',
  });
  await until(() => status(jobs, jobId) === 'running', 5000, 'running');
  fetcher.breaker = { state: 'open', reason: 'offline', message: 'Offline', retryAt: NOW * 1000 + 30_000 };
  fetcher.emit({ type: 'breaker', breaker: fetcher.breaker });
  assert.equal(status(jobs, jobId), 'waiting');
  assert.equal(jobs.get(jobId)!.message, 'Offline');
  fetcher.breaker = { state: 'closed', reason: null, message: null, retryAt: null };
  fetcher.emit({ type: 'breaker', breaker: fetcher.breaker });
  assert.equal(status(jobs, jobId), 'running');
  fetcher.release();
  await until(() => status(jobs, jobId) === 'done', 10_000, 'done');
});

test('download.wait: covered, stopped jobs and timeouts; the waited range rides the waiting lane', async () => {
  const { jobs, fetcher } = harness();
  const range = { symbol: 'EURUSD', tf: '1h' as const, from: s('2024-03-04T00:00:00Z'), to: s('2024-03-08T00:00:00Z') };
  // Nothing stored, no job: the wait itself fetches what the range needs (its 1-minute days).
  const covered = await jobs.wait({ ...range, timeoutMs: 5000 });
  assert.deepEqual(covered, { covered: true, status: null });
  assert.ok(fetcher.lanes.some((l) => l.startsWith('waiting eurusd/m1/')));
  assert.ok(fetcher.lanes.every((l) => l.includes('/m1/')));

  fetcher.hold = true;
  const { jobId } = await jobs.start({
    symbol: 'EURUSD',
    from: s('2024-01-01T00:00:00Z'),
    to: s('2024-02-01T00:00:00Z'),
    origin: 'chat',
  });
  const pausing = jobs.wait({
    symbol: 'EURUSD',
    tf: '5m',
    from: s('2024-01-08T00:00:00Z'),
    to: s('2024-01-10T00:00:00Z'),
    jobId,
    timeoutMs: 5000,
  });
  await until(() => fetcher.held > 0, 5000, 'queued requests');
  await jobs.pause(jobId);
  assert.deepEqual(await pausing, { covered: false, status: 'paused' });

  const timed = await jobs.wait({
    symbol: 'EURUSD',
    tf: '5m',
    from: s('2024-01-15T00:00:00Z'),
    to: s('2024-01-16T00:00:00Z'),
    timeoutMs: 100,
  });
  assert.equal(timed.covered, false);
  fetcher.release();
  const removed = jobs.wait({
    symbol: 'EURUSD',
    tf: '5m',
    from: s('2023-01-15T00:00:00Z'),
    to: s('2023-01-16T00:00:00Z'),
    jobId: 'nope',
    timeoutMs: 5000,
  });
  assert.deepEqual(await removed, { covered: false, status: null });
});

test('TradingView jobs need a session, and page every timeframe until its start', async () => {
  let loggedIn = false;
  const pages: string[] = [];
  const session: TvSession = {
    loggedIn: () => loggedIn,
    async pageHistory(symbol, tf, opts) {
      for (let i = 0; i < 3; i += 1) {
        pages.push(`${symbol} ${tf}`);
        opts.onPage?.(i < 1 ? 5000 : 0, 0.01);
      }
      h.tv.setReachedStart(symbol, tf, s('2010-01-01T00:00:00Z'));
      return {};
    },
  };
  const h = harness(fx, { tvSession: session });
  await assert.rejects(
    h.jobs.start({ symbol: 'NASDAQ:AAPL', origin: 'chat' }),
    (e: unknown) => e instanceof RpcError && e.code === 'NOT_LOGGED_IN',
  );
  loggedIn = true;
  const { jobId, plan } = await h.jobs.start({ symbol: 'NASDAQ:AAPL', origin: 'chat' });
  assert.equal(plan.source, 'tradingview');
  await until(() => status(h.jobs, jobId) === 'done', 5000, 'done');
  assert.equal(pages.length, 7 * 3);
  const job = h.jobs.get(jobId)!;
  assert.equal(job.done, 21);
  assert.equal(job.total, 21);
  assert.equal((await h.planner.plan({ symbol: 'NASDAQ:AAPL' })).complete, true);
});

test('download.wait ends at `to` (exclusive), and cancel announces the removal', async () => {
  const { jobs, fetcher, events } = harness();
  // Exactly the 1-minute days of March, newest first: nothing after them is asked for.
  const covered = await jobs.wait({
    symbol: 'EURUSD',
    tf: '1h',
    from: s('2024-03-01T00:00:00Z'),
    to: s('2024-04-01T00:00:00Z'),
    timeoutMs: 5000,
  });
  assert.deepEqual(covered, { covered: true, status: null });
  const march: string[] = [];
  for (let d = s('2024-03-31T00:00:00Z'); d >= s('2024-03-01T00:00:00Z'); d -= DAY)
    march.push(`waiting eurusd/m1/${d}`);
  assert.deepEqual(fetcher.lanes, march);

  fetcher.hold = true;
  const { jobId } = await jobs.start({
    symbol: 'EURUSD',
    from: s('2024-01-01T00:00:00Z'),
    to: s('2024-02-01T00:00:00Z'),
    origin: 'chat',
  });
  await jobs.cancel(jobId);
  assert.equal(jobs.get(jobId), null);
  assert.equal(events.at(-1)?.event, 'download.removed');
  fetcher.release();
});

test('a TradingView download started with nothing cached is reused by any later start, whatever its from', async () => {
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => (release = resolve));
  const session: TvSession = {
    loggedIn: () => true,
    async pageHistory(symbol, tf, opts) {
      // One page of old bars lands (the cached start moves back), then TradingView is slow.
      const t0 = s('2019-01-07T00:00:00Z');
      h.tv.record(symbol, tf, [{ t: t0, o: 1, h: 1, l: 1, c: 1, v: 1 }], [t0, t0 + 86400]);
      opts.onPage?.(1, 0.01);
      await gate;
      h.tv.setReachedStart(symbol, tf, t0);
      return {};
    },
  };
  const h = harness(fx, { tvSession: session });
  const first = await h.jobs.start({ symbol: 'NASDAQ:AAPL', origin: 'chat' });
  assert.equal(first.plan.from, NOW, 'nothing cached: the plan starts at now');
  await until(() => h.tv.coverage('NASDAQ:AAPL', '1m').intervals.first() !== null, 5000, 'a first page');
  const everything = await h.jobs.start({ symbol: 'NASDAQ:AAPL', origin: 'data' });
  assert.equal(everything.jobId, first.jobId);
  const since2020 = await h.planner.plan({ symbol: 'NASDAQ:AAPL', from: s('2020-01-01T00:00:00Z') });
  assert.equal(since2020.job?.id, first.jobId);
  release();
  await until(() => status(h.jobs, first.jobId) === 'done', 5000, 'done');
  assert.equal(h.jobs.list('NASDAQ:AAPL').length, 1);
});

/** A stock's 1m session bars (13:00 to 20:00 UTC) in [from, to). */
function session1m(from: number, to: number): Bar[] {
  const out: Bar[] = [];
  for (let t = from; t < to; t += 60) {
    const h = new Date(t * 1000).getUTCHours();
    if (h >= 13 && h < 20) out.push({ t, o: 1, h: 1, l: 1, c: 1, v: 1 });
  }
  return out;
}

/** A TradingView session serving `all` like TradingView does: bars before `to` (exclusive), 300 a
 *  page, paged by the real walk (so coverage is exactly what pageHistory would record). */
function tvServing(
  all: () => Bar[],
  h: () => ReturnType<typeof harness>,
  calls: Array<{ to?: number; stopAt?: number }>,
) {
  const session: TvSession = {
    loggedIn: () => true,
    async pageHistory(symbol, tf, opts) {
      calls.push({ to: opts.to, stopAt: opts.stopAt });
      const before = all().filter((b) => opts.to === undefined || b.t < opts.to);
      let n = 300;
      return walkBack(
        {
          first: before.slice(-n),
          firstSeconds: 0.01,
          async more() {
            n += 300;
            return before.slice(-n);
          },
        },
        tf,
        { ...opts, keep: (bars, cover) => h().tv.record(symbol, tf, bars, cover) },
      );
    },
  };
  return session;
}

/** Every TradingView timeframe but 1m reached its start with one stored run (no holes). */
function tvStartsReached(h: ReturnType<typeof harness>, sym: string, at: number): void {
  for (const tf of ['1m', '5m', '15m', '1h', '4h', '1d', '1w'] as const) {
    if (tf !== '1m') h.tv.record(sym, tf, [{ t: at, o: 1, h: 1, l: 1, c: 1, v: 1 }], [at, at + 604800]);
    h.tv.setReachedStart(sym, tf, at);
  }
}

test('a TradingView hole that ends at a session open is filled: the first page covers up to the run above', async () => {
  const sym = 'NASDAQ:AAPL';
  const all = session1m(s('2024-05-01T00:00:00Z'), NOW);
  const calls: Array<{ to?: number; stopAt?: number }> = [];
  let h: ReturnType<typeof harness> | null = null;
  h = harness(fx, {
    tvSession: tvServing(
      () => all,
      () => h!,
      calls,
    ),
  });
  tvStartsReached(h, sym, s('2024-05-01T13:00:00Z'));
  // A full download once (to Tue 20:00), then a live chart from the Wednesday open: the hole's top
  // edge follows a night, which the gap tolerance does not count as a closure.
  const old = session1m(s('2024-05-01T00:00:00Z'), s('2024-05-21T20:00:00Z'));
  h.tv.record(sym, '1m', old, [old[0]!.t, s('2024-05-21T20:00:00Z')]);
  const fresh = session1m(s('2024-05-29T13:00:00Z'), NOW);
  h.tv.record(sym, '1m', fresh, [fresh[0]!.t, fresh.at(-1)!.t + 60]);
  const plan = await h.planner.plan({ symbol: sym });
  assert.equal(plan.complete, false, 'the hole is work to do');
  const { jobId } = await h.jobs.start({ symbol: sym, origin: 'chart' });
  await until(() => status(h.jobs, jobId) === 'done', 10_000, 'done');
  assert.deepEqual(calls, [{ to: s('2024-05-29T13:00:00Z'), stopAt: s('2024-05-21T20:00:00Z') }], 'one walk');
  assert.equal(h.tv.coverage(sym, '1m').intervals.count, 1, 'the runs joined');
  const page = await h.series.read({ symbol: sym, tf: '1m', before: fresh[0]!.t, count: 200 });
  assert.equal(page.more, 'cached');
  assert.equal((await h.planner.plan({ symbol: sym })).complete, true);
});

test('a TradingView hole it serves nothing for is recorded as covered, so plans stop counting it', async () => {
  const sym = 'NASDAQ:AAPL';
  // TradingView keeps 1m bars from May 20 on only; the older run was stored back when it had more.
  const all = session1m(s('2024-05-20T00:00:00Z'), NOW);
  const calls: Array<{ to?: number; stopAt?: number }> = [];
  let h: ReturnType<typeof harness> | null = null;
  h = harness(fx, {
    tvSession: tvServing(
      () => all,
      () => h!,
      calls,
    ),
  });
  tvStartsReached(h, sym, s('2024-05-01T13:00:00Z'));
  const old = session1m(s('2024-05-01T00:00:00Z'), s('2024-05-10T20:00:00Z'));
  h.tv.record(sym, '1m', old, [old[0]!.t, s('2024-05-10T20:00:00Z')]);
  const fresh = session1m(s('2024-05-29T13:00:00Z'), NOW);
  h.tv.record(sym, '1m', fresh, [fresh[0]!.t, fresh.at(-1)!.t + 60]);
  const { jobId } = await h.jobs.start({ symbol: sym, origin: 'chart' });
  await until(() => status(h.jobs, jobId) === 'done', 10_000, 'done');
  assert.equal(h.tv.coverage(sym, '1m').intervals.count, 1, 'what TradingView has joined; the rest is known empty');
  const plan = await h.planner.plan({ symbol: sym });
  assert.equal(plan.complete, true, `still ${plan.requests} page(s) planned`);
  // A later download does not walk it again.
  const before = calls.length;
  const again = await h.jobs.start({ symbol: sym, origin: 'chart' });
  await until(() => status(h.jobs, again.jobId) === 'done', 10_000, 'done again');
  assert.equal(calls.length, before);
});

test('a range that ends above the start fetches what the probes passed over, and learns no start', async () => {
  // 1-minute data only from 2024-03-04; the range asks for a year from June 2023.
  const start = s('2024-03-04T00:00:00Z');
  const data: Data = (_i, tier, b) => bucketBars(tier, b, { every: 3600, from: start });
  const { store, jobs, series } = harness(data);
  const range = { symbol: 'EURUSD', from: s('2023-06-01T00:00:00Z'), to: s('2024-06-01T00:00:00Z') };
  const { jobId } = await jobs.start({ ...range, origin: 'chat' });
  await until(() => status(jobs, jobId) === 'done', 20_000, 'the job to finish');
  assert.equal(store.learnedStart('eurusd', 'm1'), null, 'the walk never reached the history start');
  assert.deepEqual(series.missing('EURUSD', '1m', range.from, range.to - 1), [], 'every day asked for is stored');
  const job = jobs.get(jobId)!;
  assert.equal(job.skipped, 0);
  assert.equal(job.done, job.total);
});

test('an old record naming hourly and daily tiers goes on with its 1-minute part only', async () => {
  const cache = tempDir();
  const dir = path.join(cache, 'jobs');
  fs.mkdirSync(dir, { recursive: true });
  const record = (id: string, tiers: string[], perTier: Array<{ tier: string; total: number; done: number }>) => ({
    version: 1,
    id,
    source: 'dukascopy',
    key: 'eurusd',
    symbol: 'EURUSD',
    name: 'EUR/USD',
    from: s('2024-03-01T00:00:00Z'),
    to: s('2024-04-01T00:00:00Z'),
    tiers,
    status: 'paused',
    total: perTier.reduce((n, t) => n + t.total, 0),
    done: perTier.reduce((n, t) => n + t.done, 0),
    skipped: 0,
    bytes: 0,
    etaSeconds: null,
    rate: 0,
    inFlight: 0,
    perTier,
    origin: 'chat',
    createdAt: NOW * 1000,
    updatedAt: NOW * 1000,
    openEnd: false,
    failed: 0,
  });
  fs.writeFileSync(
    path.join(dir, 'every.json'),
    JSON.stringify(
      record(
        'every',
        ['m1', 'h1', 'd1'],
        [
          { tier: 'm1', total: 31, done: 10 },
          { tier: 'h1', total: 1, done: 1 },
          { tier: 'd1', total: 1, done: 1 },
        ],
      ),
    ),
  );
  fs.writeFileSync(
    path.join(dir, 'daily.json'),
    JSON.stringify(record('daily', ['d1'], [{ tier: 'd1', total: 1, done: 0 }])),
  );
  const { jobs, fetcher } = harness(fx, { cache });
  await jobs.load();
  const every = jobs.get('every')!;
  assert.deepEqual(every.tiers, ['m1']);
  assert.deepEqual(
    every.perTier.map((t) => [t.tier, t.total, t.done]),
    [['m1', 31, 10]],
  );
  assert.deepEqual([every.total, every.done, every.status], [31, 10, 'paused']);
  // A daily-only download has nothing left: it ends, saying why.
  const daily = jobs.get('daily')!;
  assert.equal(daily.status, 'done');
  assert.match(daily.message ?? '', /1-minute/);
  jobs.resume('every');
  await until(() => status(jobs, 'every') === 'done', 10_000, 'the resumed job to finish');
  assert.ok(fetcher.requests.every((k) => k.includes('/m1/')));
});

test('a paused job counts what others covered meanwhile when it resumes', async () => {
  const { cache, jobs, fetcher } = harness();
  fetcher.hold = true;
  const range = { symbol: 'EURUSD', from: s('2024-02-01T00:00:00Z'), to: s('2024-05-01T00:00:00Z') };
  const { jobId, plan } = await jobs.start({ ...range, origin: 'chat' });
  await until(() => fetcher.held > 0, 5000, 'queued requests');
  fetcher.release(10);
  await new Promise((resolve) => setTimeout(resolve, 100));
  await jobs.pause(jobId);
  // A chart's read (no job) fetches the whole range while the job is paused.
  const read = jobs.wait({ ...range, tf: '1m', timeoutMs: 15_000 });
  fetcher.release();
  assert.equal((await read).covered, true);
  jobs.resume(jobId);
  await until(() => status(jobs, jobId) === 'done', 10_000, 'the job to finish');
  const job = jobs.get(jobId)!;
  assert.equal(job.total, plan.requests, 'the total shrank to what the job fetched itself');
  assert.equal(job.done, job.total);
  assert.equal(job.skipped, 0);
  let files = 0;
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      if (e.isDirectory()) walk(path.join(dir, e.name));
      else if (e.name.endsWith('.json.gz')) files += fs.statSync(path.join(dir, e.name)).size;
    }
  };
  walk(path.join(cache, 'dukascopy', 'eurusd'));
  await until(() => jobs.get(jobId)!.bytes === files, 2000, `bytes ${jobs.get(jobId)!.bytes} to be ${files}`);
});

test('a job restarted from its record resumes its saved plan', async () => {
  const cache = tempDir();
  const first = harness(fx, { cache });
  first.fetcher.hold = true;
  const range = { symbol: 'EURUSD', from: s('2024-03-01T00:00:00Z'), to: s('2024-04-01T00:00:00Z') };
  const { jobId, plan } = await first.jobs.start({ ...range, origin: 'chat' });
  await until(() => first.fetcher.held > 0, 5000, 'queued requests');
  first.fetcher.release(4);
  await new Promise((resolve) => setTimeout(resolve, 100));
  await first.jobs.pause(jobId);
  await first.jobs.close();
  await first.store.close();
  // Another app run fills the range before this job resumes.
  const second = harness(fx, { cache });
  await second.jobs.load();
  assert.equal((await second.jobs.wait({ ...range, tf: '1m', timeoutMs: 15_000 })).covered, true);
  second.jobs.resume(jobId);
  await until(() => status(second.jobs, jobId) === 'done', 10_000, 'the job to finish');
  const job = second.jobs.get(jobId)!;
  assert.deepEqual([job.done, job.total, job.skipped], [plan.requests, plan.requests, 0]);
});
