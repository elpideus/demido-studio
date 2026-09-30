// The one-time migration of the old caches. One test runs on a temp COPY of this machine's real
// cache (only read, never modified; skipped when there is none or it was migrated already), one on
// a small synthetic cache that always exists.

import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, test } from 'node:test';

import { historyInstrument } from '../src/dukascopy.ts';
import { type Tier, nextBucket } from '../src/store/buckets.ts';
import { DUE_AFTER_SECONDS, DukascopyStore } from '../src/store/dukascopy-store.ts';
import { migrate } from '../src/store/migrate.ts';
import { Series } from '../src/store/series.ts';
import { TvStore } from '../src/store/tv-store.ts';
import { DAY, bucketBars, bucketBody, cleanup, s, tempDir } from './store-helpers.ts';

afterEach(cleanup);

const REAL = path.join(process.env.LOCALAPPDATA ?? '', 'Demido Studio', 'cache', 'market');
const FLAT = /^candles%2F(minute|hour|day)%2F([A-Z0-9.-]+)%2FBID%2F(\d{4})(%2F(\d{1,2}))?(%2F(\d{1,2}))?\.json$/;
const TIER: Record<string, Tier> = { minute: 'm1', hour: 'h1', day: 'd1' };

const flatNames = (cache: string) =>
  fs.existsSync(path.join(cache, 'dukascopy'))
    ? fs.readdirSync(path.join(cache, 'dukascopy')).filter((n) => n.startsWith('candles%2F') && n.endsWith('.json'))
    : [];
const realHasFlatFiles = flatNames(REAL).length > 0;

class FailingFlushStore extends DukascopyStore {
  override async flush(): Promise<void> {
    throw new Error('disk full');
  }
}

function open(cache: string, store?: DukascopyStore) {
  const s1 = store ?? new DukascopyStore(cache, { manifestDelayMs: 20 });
  const tv = new TvStore(cache, { flushDelayMs: 20 });
  const series = new Series({ store: s1, tv });
  const logs: string[] = [];
  const deps = {
    cacheDir: cache,
    store: s1,
    tv,
    route: (sym: string) => series.route(sym),
    log: (_l: string, m: string) => logs.push(m),
  };
  return { store: s1, tv, series, logs, deps };
}

function copyReal(): string {
  const cache = tempDir('demido-migrate-');
  fs.cpSync(REAL, cache, { recursive: true, preserveTimestamps: true });
  return cache;
}

test(
  'a copy of the real cache migrates completely, and a second run changes nothing',
  { skip: !realHasFlatFiles && 'no unmigrated local cache' },
  async () => {
    const cache = copyReal();
    // What the old cache holds, counted independently of the migration.
    const expected = { m1: 0, h1: 0, d1: 0, empty: 0, provisional: 0 };
    for (const name of flatNames(cache)) {
      const m = FLAT.exec(name);
      if (!m) continue;
      const tier = TIER[m[1]!]!;
      expected[tier] += 1;
      const file = path.join(cache, 'dukascopy', name);
      const json = JSON.parse(fs.readFileSync(file, 'utf8')) as { times: number[] };
      if (json.times.length === 0) expected.empty += 1;
      const start = Date.UTC(Number(m[3]), Number(m[5] ?? 1) - 1, Number(m[7] ?? 1)) / 1000;
      if (Math.floor(fs.statSync(file).mtimeMs / 1000) < nextBucket(tier, start) + DUE_AFTER_SECONDS)
        expected.provisional += 1;
    }
    const oldJobs = fs.existsSync(path.join(cache, 'downloads'))
      ? fs
          .readdirSync(path.join(cache, 'downloads'))
          .map((n) => JSON.parse(fs.readFileSync(path.join(cache, 'downloads', n), 'utf8')))
      : [];
    // Files the migration must leave alone.
    const eurusdH1 = fs.readFileSync(
      path.join(
        cache,
        'dukascopy',
        flatNames(cache).find((n) => n.includes('%2Fhour%2FEUR-USD%2F'))!,
      ),
    );
    const strays = {
      'candles%2Fhour%2FEUR-USD%2FBID%2F2003%2F13.json': eurusdH1,
      'candles%2Fhour%2FZZZ-QQQ%2FBID%2F2020%2F1.json': eurusdH1,
      'candles%2Fhour%2FEUR-USD%2FBID%2F1999%2F1.json': Buffer.from('{"timestamp": 1, "times": [1'),
    };
    for (const [name, body] of Object.entries(strays)) fs.writeFileSync(path.join(cache, 'dukascopy', name), body);

    const a = open(cache);
    const report = await migrate(a.deps);
    assert.deepEqual(report.dukascopy.migrated, { m1: expected.m1, h1: expected.h1, d1: expected.d1 });
    assert.equal(report.dukascopy.empty, expected.empty);
    assert.ok(report.dukascopy.empty > 100, `the old cache's empty answers are recorded (${report.dukascopy.empty})`);
    assert.equal(report.dukascopy.provisional, expected.provisional);
    assert.equal(report.dukascopy.deleted, expected.m1 + expected.h1 + expected.d1);
    assert.equal(report.dukascopy.invalid.length, 2);
    assert.equal(report.dukascopy.unknown.length, 1);
    assert.deepEqual(flatNames(cache).sort(), Object.keys(strays).sort(), 'only the strays are left');
    assert.equal(fs.existsSync(path.join(cache, 'coverage.json')), false);
    assert.equal(fs.existsSync(path.join(cache, 'downloads')), false);

    // A fresh store reads it all back from disk: EUR/USD h1 covers 2003-05 to 2026-08.
    const b = open(cache);
    const h1 = b.store.sets('eurusd', 'h1');
    if (expected.h1 >= 280) assert.ok(h1.covered.covers(s('2003-05-01T00:00:00Z'), s('2026-09-01T00:00:00Z')));
    let empties = 0;
    for (const instrument of b.store.instruments()) {
      for (const tier of ['m1', 'h1', 'd1'] as const) {
        for (const [f, t] of b.store.sets(instrument, tier).empty) empties += tier === 'm1' ? (t - f) / DAY : 1;
      }
    }
    assert.ok(empties >= expected.empty - 5, 'empties are in the manifests');

    // Unfinished old jobs are paused jobs now; finished ones are gone.
    const unfinished = oldJobs.filter((j) => ['error', 'paused', 'running'].includes(j.status));
    const jobs = fs
      .readdirSync(path.join(cache, 'jobs'))
      .map((n) => JSON.parse(fs.readFileSync(path.join(cache, 'jobs', n), 'utf8')));
    assert.equal(report.jobs.converted, unfinished.length);
    assert.equal(report.jobs.dropped, oldJobs.length - unfinished.length);
    for (const old of unfinished) {
      const job = jobs.find((j) => j.migratedFrom === old.id)!;
      assert.ok(job, `${old.id} was converted`);
      assert.equal(job.status, 'paused');
      assert.equal(job.key, old.instrument);
      assert.equal(job.from, old.from);
      assert.equal(job.to, old.to);
      assert.deepEqual(job.tiers, ['m1', 'h1', 'd1']);
      assert.ok(job.total > 0);
    }
    if (oldJobs.some((j) => j.id === 'dukascopy_gbpchf_1h_1059930000_1753660210')) {
      assert.ok(
        jobs.some((j) => j.key === 'gbpchf' && j.status === 'paused'),
        'the GBP/CHF job that failed is paused',
      );
    }

    // TradingView bars got coverage; FX:EURUSD 1h is one run.
    for (const sr of b.tv.series()) assert.ok(sr.intervals.length > 0, `${sr.symbol} ${sr.tf} has coverage`);
    if (b.tv.series().some((sr) => sr.symbol === 'FX:EURUSD' && sr.tf === '1h')) {
      assert.equal(b.tv.coverage('FX:EURUSD', '1h').intervals.count, 1);
      assert.equal(historyInstrument('FX:EURUSD'), 'eurusd');
    }

    // Idempotent: nothing left to do.
    const again = await migrate(b.deps);
    assert.deepEqual(again.dukascopy.migrated, { m1: 0, h1: 0, d1: 0 });
    assert.equal(again.dukascopy.deleted, 0);
    assert.equal(again.tradingview.series, 0);
    assert.equal(again.jobs.converted, 0);
    assert.equal(fs.readdirSync(path.join(cache, 'jobs')).length, jobs.length);
  },
);

test(
  'originals are deleted only after everything was written and flushed',
  { skip: !realHasFlatFiles && 'no unmigrated local cache' },
  async () => {
    const cache = copyReal();
    const before = flatNames(cache).length;
    const failing = open(cache, new FailingFlushStore(cache, { manifestDelayMs: 60_000 }));
    await assert.rejects(migrate(failing.deps), /disk full/);
    assert.equal(flatNames(cache).length, before, 'no original was deleted');
    assert.ok(fs.existsSync(path.join(cache, 'downloads')));
    // The next start finishes the job: what was written is recognised, the rest is written.
    const next = open(cache);
    const report = await migrate(next.deps);
    const d = report.dukascopy;
    assert.equal(d.migrated.m1 + d.migrated.h1 + d.migrated.d1 + d.already, before);
    assert.equal(d.deleted, before);
    assert.equal(flatNames(cache).length, 0);
  },
);

test('a synthetic flat cache: empties, provisional by mtime, strays left, old jobs and TradingView coverage', async () => {
  const cache = tempDir('demido-migrate-');
  const flat = path.join(cache, 'dukascopy');
  fs.mkdirSync(flat, { recursive: true });
  const write = (name: string, body: Buffer, mtime: number) => {
    const file = path.join(flat, name);
    fs.writeFileSync(file, body);
    fs.utimesSync(file, mtime, mtime);
  };
  const march = s('2020-03-01T00:00:00Z');
  write(
    'candles%2Fhour%2FEUR-USD%2FBID%2F2020%2F3.json',
    bucketBody('h1', march, bucketBars('h1', march)),
    s('2020-06-01T00:00:00Z'),
  );
  const sat = s('2020-03-07T00:00:00Z');
  write('candles%2Fminute%2FEUR-USD%2FBID%2F2020%2F3%2F7.json', bucketBody('m1', sat, []), s('2020-06-01T00:00:00Z'));
  const y2019 = s('2019-01-01T00:00:00Z');
  write(
    'candles%2Fday%2FEUR-USD%2FBID%2F2019.json',
    bucketBody('d1', y2019, bucketBars('d1', y2019)),
    s('2020-02-01T00:00:00Z'),
  );
  // Written three days after its day closed: CloudFront may still have been serving an early copy.
  const recent = Math.floor(Date.now() / 1000 / DAY) * DAY - 10 * DAY;
  const d = new Date(recent * 1000);
  const recentName = `candles%2Fminute%2FGBP-CHF%2FBID%2F${d.getUTCFullYear()}%2F${d.getUTCMonth() + 1}%2F${d.getUTCDate()}.json`;
  write(recentName, bucketBody('m1', recent, bucketBars('m1', recent, { open: () => true })), recent + 4 * DAY);
  write('candles%2Fhour%2FEUR-USD%2FBID%2F2020%2F0.json', Buffer.from('{}'), s('2020-06-01T00:00:00Z'));
  write('candles%2Fhour%2FNOPE-NOPE%2FBID%2F2020%2F1.json', bucketBody('h1', march, []), s('2020-06-01T00:00:00Z'));
  write('candles%2Fhour%2FEUR-USD%2FBID%2F2020%2F4.json', Buffer.from('not json'), s('2020-06-01T00:00:00Z'));

  // tvcache files and the old coverage file that spells their symbols.
  fs.mkdirSync(path.join(cache, 'tradingview'));
  const hour = (t: number) => ({ t, o: 1.1, h: 1.1, l: 1.1, c: 1.1, v: 1 });
  const bars = [
    ...[0, 1, 2, 3].map((i) => hour(s('2024-05-08T10:00:00Z') + i * 3600)),
    // Five hours later on a Wednesday: a new run.
    ...[0, 1].map((i) => hour(s('2024-05-08T18:00:00Z') + i * 3600)),
  ];
  fs.writeFileSync(path.join(cache, 'tradingview', 'FX_IDC_AUDCHF_1h.json'), JSON.stringify(bars));
  fs.writeFileSync(
    path.join(cache, 'coverage.json'),
    JSON.stringify({ 'tradingview:fx_idc:audchf:1h': { earliest: 0, latest: 1 } }),
  );
  fs.mkdirSync(path.join(cache, 'downloads'));
  fs.writeFileSync(
    path.join(cache, 'downloads', 'a.json'),
    JSON.stringify({
      id: 'a',
      symbol: 'SAXO:GBPCHF',
      instrument: 'gbpchf',
      source: 'dukascopy',
      from: s('2010-01-01T00:00:00Z'),
      to: s('2011-01-01T00:00:00Z'),
      status: 'error',
    }),
  );
  fs.writeFileSync(
    path.join(cache, 'downloads', 'b.json'),
    JSON.stringify({ id: 'b', instrument: 'eurusd', from: 1, to: 2, status: 'done' }),
  );

  const { store, tv, deps } = open(cache);
  const report = await migrate(deps);
  assert.deepEqual(report.dukascopy.migrated, { m1: 2, h1: 1, d1: 1 });
  assert.equal(report.dukascopy.empty, 1);
  assert.equal(report.dukascopy.provisional, 1);
  assert.equal(report.dukascopy.invalid.length, 2);
  assert.deepEqual(report.dukascopy.unknown, ['candles%2Fhour%2FNOPE-NOPE%2FBID%2F2020%2F1.json']);
  assert.equal(flatNames(cache).length, 3);

  assert.equal(store.status('eurusd', 'm1', sat), 'final');
  assert.ok(store.sets('eurusd', 'm1').empty.contains(sat));
  assert.equal(store.status('gbpchf', 'm1', recent), 'provisional');
  // The hourly and daily buckets moved with the rest, then went: history is 1-minute candles.
  assert.equal(store.status('eurusd', 'h1', march), 'missing');
  assert.equal(store.status('eurusd', 'd1', y2019), 'missing');
  assert.equal(report.coarse.tiers, 2);
  assert.ok(!fs.existsSync(path.join(cache, 'dukascopy', 'eurusd', 'h1')));
  assert.ok(!fs.existsSync(path.join(cache, 'dukascopy', 'eurusd', 'd1')));

  const cov = tv.coverage('FX_IDC:AUDCHF', '1h');
  assert.deepEqual(cov.intervals.toJSON(), [
    [s('2024-05-08T10:00:00Z'), s('2024-05-08T14:00:00Z')],
    [s('2024-05-08T18:00:00Z'), s('2024-05-08T20:00:00Z')],
  ]);
  assert.equal(fs.existsSync(path.join(cache, 'coverage.json')), false);

  const jobs = fs
    .readdirSync(path.join(cache, 'jobs'))
    .map((n) => JSON.parse(fs.readFileSync(path.join(cache, 'jobs', n), 'utf8')));
  assert.equal(jobs.length, 1);
  assert.equal(jobs[0].status, 'paused');
  assert.equal(jobs[0].key, 'gbpchf');
  assert.equal(jobs[0].symbol, 'SAXO:GBPCHF');
  assert.equal(report.jobs.dropped, 1);
  assert.equal(fs.existsSync(path.join(cache, 'downloads')), false);
});

test("a store's hourly and daily tiers are deleted at start; its 1-minute days stay; a second run finds nothing", async () => {
  const cache = tempDir('demido-migrate-');
  const first = open(cache);
  const march = s('2024-03-01T00:00:00Z');
  const day = s('2024-03-04T00:00:00Z');
  await first.store.put('eurusd', 'm1', day, bucketBody('m1', day, bucketBars('m1', day)), {
    builtAt: day + 2 * DAY,
    final: true,
  });
  await first.store.put('eurusd', 'h1', march, bucketBody('h1', march, bucketBars('h1', march)), {
    builtAt: march + 40 * DAY,
    final: true,
  });
  await first.store.put('eurusd', 'd1', s('2023-01-01T00:00:00Z'), null, {
    builtAt: s('2024-02-01T00:00:00Z'),
    final: true,
  });
  first.store.setLearnedStart('gbpchf', 'h1', { t: s('2013-01-01T00:00:00Z'), evidence: 'test' });
  await first.store.close();

  const { store, deps } = open(cache);
  const report = await migrate(deps);
  assert.equal(report.coarse.tiers, 3, 'eurusd h1 and d1, gbpchf h1');
  assert.ok(report.coarse.bytes > 0);
  assert.equal(store.status('eurusd', 'm1', day), 'final');
  assert.equal((await store.readNative('eurusd', 'm1', day, day + DAY)).length, bucketBars('m1', day).length);
  assert.equal(store.status('eurusd', 'h1', march), 'missing');
  assert.equal(store.status('eurusd', 'd1', s('2023-01-01T00:00:00Z')), 'missing');
  assert.equal(store.learnedStart('gbpchf', 'h1'), null);
  assert.ok(!fs.existsSync(path.join(cache, 'dukascopy', 'eurusd', 'h1')));
  await store.flush();
  // The manifest no longer records them either, so a restart does not bring them back.
  const manifest = JSON.parse(fs.readFileSync(path.join(cache, 'dukascopy', 'eurusd', 'manifest.json'), 'utf8'));
  assert.deepEqual(manifest.tiers.h1.fetched, []);
  assert.deepEqual(manifest.tiers.d1.empty, []);
  assert.equal((await migrate(deps)).coarse.tiers, 0);
});
