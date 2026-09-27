import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, test } from 'node:test';
import zlib from 'node:zlib';

import { bucketStart, nextBucket, type Tier } from '../src/store/buckets.ts';
import { DecodeError } from '../src/store/decode.ts';
import {
  CLOSE_GRACE_SECONDS,
  DUE_AFTER_SECONDS,
  DukascopyStore,
  provisionalDue,
  writeAtomic,
  type StoreChange,
  type StoreOptions,
} from '../src/store/dukascopy-store.ts';

const s = (iso: string) => Date.parse(iso) / 1000;
const DAY = s('2024-03-05T00:00:00Z');
const NOW = s('2024-06-01T00:00:00Z');

const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
});

function tempCache(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'demido-store-'));
  dirs.push(dir);
  return dir;
}

/** A raw candle response with `n` bars every `step` seconds from `start`. */
function body(start: number, n: number, step = 60): Buffer {
  return Buffer.from(
    JSON.stringify({
      timestamp: start * 1000,
      multiplier: 0.00001,
      shift: step * 1000,
      open: n ? 1.1 : null,
      high: n ? 1.1 : null,
      low: n ? 1.1 : null,
      close: n ? 1.1 : null,
      times: Array.from({ length: n }, (_, i) => (i === 0 ? 0 : 1)),
      opens: new Array(n).fill(1),
      highs: new Array(n).fill(2),
      lows: new Array(n).fill(0),
      closes: new Array(n).fill(1),
      volumes: new Array(n).fill(1.5),
    }),
  );
}

interface Harness {
  cache: string;
  store: DukascopyStore;
  logs: string[];
  clock: { now: number };
  open: (opts?: StoreOptions) => DukascopyStore;
}

function harness(opts: StoreOptions = {}): Harness {
  const cache = tempCache();
  const logs: string[] = [];
  const clock = { now: NOW * 1000 };
  const open = (more: StoreOptions = {}) =>
    new DukascopyStore(cache, {
      now: () => clock.now,
      log: (level, message) => logs.push(`${level}: ${message}`),
      manifestDelayMs: 20,
      ...opts,
      ...more,
    });
  return { cache, store: open(), logs, clock, open };
}

const bucketFile = (cache: string, instrument: string, tier: Tier, key: string) =>
  path.join(cache, 'dukascopy', instrument, tier, key.slice(0, 4), `${key}.json.gz`);
const manifestFile = (cache: string, instrument: string) => path.join(cache, 'dukascopy', instrument, 'manifest.json');
const readManifest = (cache: string, instrument: string) =>
  JSON.parse(fs.readFileSync(manifestFile(cache, instrument), 'utf8'));
const final = (tier: Tier, start: number) => ({ builtAt: nextBucket(tier, start) + 7200, final: true });
/** Final and built after CloudFront's first copy expired: final even to a load without the manifest. */
const settled = (tier: Tier, start: number) => ({ builtAt: nextBucket(tier, start) + DUE_AFTER_SECONDS, final: true });

test('a final bucket is written as gzip at its path, recorded, and read back', async () => {
  const { cache, store } = harness();
  const raw = body(DAY, 3);
  const result = await store.put('eurusd', 'm1', DAY, raw, final('m1', DAY));
  assert.deepEqual(result, { empty: false, final: true, bars: 3, bytes: result.bytes, changed: true });
  const file = bucketFile(cache, 'eurusd', 'm1', '2024-03-05');
  assert.deepEqual(zlib.gunzipSync(fs.readFileSync(file)), raw);
  assert.equal(result.bytes, fs.statSync(file).size);
  // The file's mtime is the build time, so a load without the manifest can judge finality.
  assert.equal(Math.floor(fs.statSync(file).mtimeMs / 1000), nextBucket('m1', DAY) + 7200);
  assert.equal(store.status('eurusd', 'm1', DAY), 'final');
  assert.equal(store.has('eurusd', 'm1', DAY + 3600), true);
  assert.equal(store.needsFetch('eurusd', 'm1', DAY), false);
  const bars = await store.readNative('eurusd', 'm1', DAY, DAY + 86400);
  assert.deepEqual(
    bars.map((b) => b.t),
    [DAY, DAY + 60, DAY + 120],
  );
  assert.deepEqual(bars[1], { t: DAY + 60, o: 1.10002, h: 1.10004, l: 1.1, c: 1.10002, v: 1.5 });
  assert.deepEqual(
    (await store.readNative('eurusd', 'm1', DAY + 60, DAY + 120)).map((b) => b.t),
    [DAY + 60],
  );
  const sets = store.sets('eurusd', 'm1');
  assert.deepEqual(sets.fetched.toJSON(), [[DAY, DAY + 86400]]);
  assert.deepEqual(sets.data.toJSON(), [[DAY, DAY + 86400]]);
  assert.equal(sets.empty.isEmpty, true);
});

test('empty responses are recorded as empty without a file, and count as covered', async () => {
  const { cache, store } = harness();
  const sat = s('2024-03-09T00:00:00Z');
  const result = await store.put('eurusd', 'm1', sat, body(sat, 0), final('m1', sat));
  assert.equal(result.empty, true);
  assert.equal(fs.existsSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-09')), false);
  await store.put('eurusd', 'm1', sat + 86400, null, final('m1', sat));
  const sets = store.sets('eurusd', 'm1');
  assert.deepEqual(sets.empty.toJSON(), [[sat, sat + 2 * 86400]]);
  assert.deepEqual(sets.covered.toJSON(), [[sat, sat + 2 * 86400]]);
  assert.equal(sets.data.isEmpty, true);
  assert.deepEqual(await store.readNative('eurusd', 'm1', sat, sat + 2 * 86400), []);
  // A later copy with data replaces the empty record.
  await store.put('eurusd', 'm1', sat, body(sat, 2), final('m1', sat));
  assert.deepEqual(store.sets('eurusd', 'm1').empty.toJSON(), [[sat + 86400, sat + 2 * 86400]]);
  assert.equal((await store.readNative('eurusd', 'm1', sat, sat + 86400)).length, 2);
  // ...and an empty final copy removes the file again.
  await store.put('eurusd', 'm1', sat, null, final('m1', sat));
  assert.equal(fs.existsSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-09')), false);
  assert.equal(store.summary('eurusd').tiers.m1.bytes, 0);
});

test('provisional copies are served, due once, replaced by a final copy, never replace one', async () => {
  const { store } = harness();
  const end = nextBucket('m1', DAY);
  // Built a second after the close (CloudFront's first copy): provisional.
  const r = await store.put('eurusd', 'm1', DAY, body(DAY, 2), { builtAt: end + 1 });
  assert.equal(r.final, false);
  assert.equal(store.status('eurusd', 'm1', DAY), 'provisional');
  assert.equal(store.has('eurusd', 'm1', DAY), true);
  assert.deepEqual(store.sets('eurusd', 'm1').covered.toJSON(), [[DAY, end]]);
  assert.equal((await store.readNative('eurusd', 'm1', DAY, end)).length, 2);
  // Due exactly when CloudFront's 7-day copy has expired.
  assert.equal(store.needsFetch('eurusd', 'm1', DAY, end + 1 + DUE_AFTER_SECONDS - 1), false);
  assert.equal(store.needsFetch('eurusd', 'm1', DAY, end + 1 + DUE_AFTER_SECONDS), true);
  assert.deepEqual(store.dueBuckets('eurusd', 'm1', end + 1 + DUE_AFTER_SECONDS), [DAY]);
  assert.deepEqual(store.dueBuckets('eurusd', 'm1', end + 10), []);
  // Not missing: coverage and plans never count it.
  assert.deepEqual(store.missingBuckets('eurusd', 'm1', DAY, end), []);
  // The re-fetch is final and replaces it.
  await store.put('eurusd', 'm1', DAY, body(DAY, 3), final('m1', DAY));
  assert.equal(store.status('eurusd', 'm1', DAY), 'final');
  assert.equal(store.sets('eurusd', 'm1').provisional.isEmpty, true);
  assert.equal((await store.readNative('eurusd', 'm1', DAY, end)).length, 3);
  // A provisional copy arriving later never downgrades it.
  const again = await store.put('eurusd', 'm1', DAY, body(DAY, 1), { builtAt: end + 5, final: false });
  assert.equal(again.changed, false);
  assert.equal(store.status('eurusd', 'm1', DAY), 'final');
  assert.equal((await store.readNative('eurusd', 'm1', DAY, end)).length, 3);
});

test("another bucket's answer is refused, stored or read, and never replaces the bucket's data", async () => {
  const { cache, store } = harness();
  const end = nextBucket('m1', DAY);
  await store.put('eurusd', 'm1', DAY, body(DAY, 3), { builtAt: end - 120, final: false });
  // A `?from=` request that crossed the close came back with the next day, still empty.
  await assert.rejects(
    store.put('eurusd', 'm1', DAY, body(end, 0), { builtAt: end + 1, final: false }),
    (e: unknown) => e instanceof DecodeError && /response is for 2024-03-06, not 2024-03-05/.test(e.message),
  );
  await assert.rejects(store.put('eurusd', 'm1', DAY, body(DAY - 60, 2), final('m1', DAY)), /2024-03-04/);
  assert.equal(store.status('eurusd', 'm1', DAY), 'provisional');
  assert.deepEqual(store.sets('eurusd', 'm1').provisional.toJSON(), [[DAY, end]]);
  assert.equal(store.sets('eurusd', 'm1').empty.isEmpty, true);
  assert.equal(store.needsFetch('eurusd', 'm1', DAY, end + CLOSE_GRACE_SECONDS), true);
  assert.equal((await store.readNative('eurusd', 'm1', DAY, end)).length, 3);
  // The same for a month: the next month's first hours are not this month.
  const month = bucketStart('h1', DAY);
  const next = nextBucket('h1', month);
  await assert.rejects(store.put('eurusd', 'h1', month, body(next, 2, 3600), final('h1', month)), DecodeError);
  assert.equal(store.status('eurusd', 'h1', month), 'missing');
  // A file holding another bucket's answer (written before this rule) heals like a corrupt one.
  await store.flush();
  fs.writeFileSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-05'), zlib.gzipSync(body(end, 2)));
  const cold = new DukascopyStore(cache, { now: () => NOW * 1000 });
  assert.deepEqual(await cold.readNative('eurusd', 'm1', DAY, end), []);
  assert.equal(cold.status('eurusd', 'm1', DAY), 'missing');
});

test('active buckets refresh after 60 s; one built while open is due right after its close', async () => {
  const { store, clock } = harness();
  const today = s('2024-06-01T00:00:00Z');
  clock.now = (today + 3600) * 1000;
  await store.put('eurusd', 'm1', today, body(today, 5), { builtAt: today + 3600 });
  assert.equal(store.status('eurusd', 'm1', today), 'provisional');
  assert.equal(store.needsFetch('eurusd', 'm1', today, today + 3600 + 59), false);
  assert.equal(store.needsFetch('eurusd', 'm1', today, today + 3600 + 60), true);
  assert.equal(store.needsFetch('eurusd', 'm1', today, today + 3600 + 30, { activeMaxAge: 10 }), true);
  const end = nextBucket('m1', today);
  assert.equal(provisionalDue('m1', today, today + 3600, end + CLOSE_GRACE_SECONDS - 1), false);
  assert.equal(provisionalDue('m1', today, today + 3600, end + CLOSE_GRACE_SECONDS), true);
  assert.equal(store.needsFetch('eurusd', 'm1', today, end + CLOSE_GRACE_SECONDS), true);
});

test('the manifest round-trips in the documented shape', async () => {
  const { cache, store, open } = harness();
  const month = bucketStart('h1', DAY);
  await store.put('eurusd', 'm1', DAY, body(DAY, 2), final('m1', DAY));
  await store.put('eurusd', 'm1', DAY + 86400, null, final('m1', DAY + 86400));
  await store.put('eurusd', 'h1', month, body(month, 4, 3600), { builtAt: nextBucket('h1', month) + 10 });
  store.markUnavailable('eurusd', 'd1', s('1970-01-01T00:00:00Z'));
  store.setLearnedStart('eurusd', 'm1', { t: DAY, evidence: { streak: 20 } });
  await store.flush();
  const json = readManifest(cache, 'eurusd');
  assert.equal(json.version, 1);
  assert.deepEqual(Object.keys(json.tiers).sort(), ['d1', 'h1', 'm1']);
  assert.deepEqual(Object.keys(json.tiers.m1).sort(), [
    'bytes',
    'empty',
    'fetched',
    'learnedStart',
    'provisional',
    'unavailable',
  ]);
  assert.deepEqual(json.tiers.m1.fetched, [[DAY, DAY + 2 * 86400]]);
  assert.deepEqual(json.tiers.m1.empty, [[DAY + 86400, DAY + 2 * 86400]]);
  assert.deepEqual(json.tiers.m1.learnedStart, { t: DAY, evidence: { streak: 20 } });
  assert.deepEqual(json.tiers.h1.provisional, { [month]: nextBucket('h1', month) + 10 });
  assert.deepEqual(json.tiers.d1.unavailable, [[0, s('1971-01-01T00:00:00Z')]]);
  assert.ok(json.tiers.m1.bytes > 0);

  const reopened = open();
  for (const tier of ['m1', 'h1', 'd1'] as const) {
    const a = store.sets('eurusd', tier);
    const b = reopened.sets('eurusd', tier);
    for (const key of ['fetched', 'empty', 'provisional', 'unavailable', 'covered', 'data'] as const) {
      assert.deepEqual(b[key].toJSON(), a[key].toJSON(), `${tier} ${key}`);
    }
  }
  assert.deepEqual(reopened.summary('eurusd'), store.summary('eurusd'));
  assert.equal(reopened.effectiveStart('eurusd', 'm1'), DAY);
  assert.equal((await reopened.readNative('eurusd', 'h1', month, nextBucket('h1', month))).length, 4);
});

test('manifest writes are debounced and single-flight; flush writes at once', async () => {
  let manifestWrites = 0;
  const { cache, open } = harness();
  const store = open({
    manifestDelayMs: 60_000,
    rename: async (from, to) => {
      if (to.endsWith('manifest.json')) manifestWrites += 1;
      await fs.promises.rename(from, to);
    },
  });
  for (let i = 0; i < 20; i += 1) await store.put('eurusd', 'm1', DAY + i * 86400, null, final('m1', DAY));
  assert.equal(manifestWrites, 0);
  assert.equal(fs.existsSync(manifestFile(cache, 'eurusd')), false);
  await Promise.all([store.flush(), store.flush(), store.flush()]);
  assert.equal(manifestWrites, 1);
  assert.deepEqual(readManifest(cache, 'eurusd').tiers.m1.empty, [[DAY, DAY + 20 * 86400]]);
  await store.flush();
  assert.equal(manifestWrites, 1, 'nothing dirty, nothing written');
  // Within the debounce, a later change is written by the timer.
  const quick = open({ manifestDelayMs: 10 });
  await quick.put('gbpusd', 'm1', DAY, null, final('m1', DAY));
  await new Promise((resolve) => setTimeout(resolve, 200));
  assert.equal(fs.existsSync(manifestFile(cache, 'gbpusd')), true);
});

test('atomic writes retry a refused rename, keep unique temps, and leave no temp behind', async () => {
  const dir = tempCache();
  const file = path.join(dir, 'a', 'b.json');
  let refusals = 0;
  await writeAtomic(file, 'one', {
    rename: async (from, to) => {
      if (refusals < 3) {
        refusals += 1;
        throw Object.assign(new Error('busy'), { code: refusals === 2 ? 'EPERM' : 'EBUSY' });
      }
      await fs.promises.rename(from, to);
    },
  });
  assert.equal(fs.readFileSync(file, 'utf8'), 'one');
  assert.equal(refusals, 3);
  // Concurrent writers never share a temp file.
  await Promise.all(Array.from({ length: 10 }, (_, i) => writeAtomic(file, `v${i}`)));
  assert.match(fs.readFileSync(file, 'utf8'), /^v\d$/);
  assert.deepEqual(fs.readdirSync(path.dirname(file)), ['b.json']);
  // A rename refused for longer than the retry window fails, and the temp is removed.
  await assert.rejects(
    writeAtomic(file, 'two', {
      retryForMs: 50,
      rename: async () => {
        throw Object.assign(new Error('locked'), { code: 'EACCES' });
      },
    }),
    /locked/,
  );
  assert.equal(fs.readFileSync(file, 'utf8').startsWith('v'), true);
  assert.deepEqual(fs.readdirSync(path.dirname(file)), ['b.json']);
  // Other errors are not retried.
  let calls = 0;
  await assert.rejects(
    writeAtomic(file, 'three', {
      rename: async () => {
        calls += 1;
        throw Object.assign(new Error('bad'), { code: 'EINVAL' });
      },
    }),
    /bad/,
  );
  assert.equal(calls, 1);
});

test('a bucket is not recorded when its file could not be written', async () => {
  const { cache, open } = harness();
  const store = open({
    rename: async (_from, to) => {
      if (to.endsWith('.json.gz')) throw Object.assign(new Error('no space'), { code: 'ENOSPC' });
    },
  });
  await assert.rejects(store.put('eurusd', 'm1', DAY, body(DAY, 2), final('m1', DAY)), /no space/);
  assert.equal(store.status('eurusd', 'm1', DAY), 'missing');
  await store.flush();
  assert.equal(fs.existsSync(manifestFile(cache, 'eurusd')), false);
  const tierDir = path.join(cache, 'dukascopy', 'eurusd', 'm1', '2024');
  assert.deepEqual(fs.existsSync(tierDir) ? fs.readdirSync(tierDir) : [], []);
});

test('reconcile on load: files without a manifest are adopted, final or provisional by mtime', async () => {
  const { cache, open, logs } = harness();
  const write = (key: string, start: number, n: number, mtime: number, raw = body(start, n)) => {
    const file = bucketFile(cache, 'gbpusd', 'm1', key);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, zlib.gzipSync(raw));
    fs.utimesSync(file, mtime, mtime);
    return file;
  };
  const end = (start: number) => nextBucket('m1', start);
  // Built after CloudFront's first copy expired: final.
  write('2024-03-05', DAY, 3, end(DAY) + DUE_AFTER_SECONDS);
  write('2024-03-06', DAY + 86400, 2, end(DAY + 86400) + 5);
  // A migrated file (received two hours after the close) whose manifest never landed: the migration
  // made it provisional, because CloudFront may have served an older copy. So does the load.
  const received = end(DAY - 86400) + 7200;
  write('2024-03-04', DAY - 86400, 2, received);
  // An empty-bucket file, a corrupt one and another bucket's answer are not data: removed.
  write('2024-03-07', DAY + 2 * 86400, 0, NOW);
  const corrupt = bucketFile(cache, 'gbpusd', 'm1', '2024-03-08');
  fs.writeFileSync(corrupt, 'not gzip');
  const foreign = write('2024-03-10', DAY + 5 * 86400, 2, NOW, body(DAY + 6 * 86400, 2));
  // A stale temp from a crash is removed; a fresh one may belong to a live writer and stays.
  const stale = `${bucketFile(cache, 'gbpusd', 'm1', '2024-03-09')}.123-0-abcd.tmp`;
  fs.writeFileSync(stale, 'x');
  fs.utimesSync(stale, NOW - 3600, (Date.now() - 120_000) / 1000);
  const store = open();
  assert.equal(store.status('gbpusd', 'm1', DAY), 'final');
  assert.equal(store.status('gbpusd', 'm1', DAY + 86400), 'provisional');
  assert.equal(store.status('gbpusd', 'm1', DAY - 86400), 'provisional');
  // ...due for its one re-fetch when the migration would have made it due.
  assert.equal(store.needsFetch('gbpusd', 'm1', DAY - 86400, received + DUE_AFTER_SECONDS - 1), false);
  assert.equal(store.needsFetch('gbpusd', 'm1', DAY - 86400, received + DUE_AFTER_SECONDS), true);
  assert.equal(store.status('gbpusd', 'm1', DAY + 2 * 86400), 'missing');
  assert.equal(store.status('gbpusd', 'm1', DAY + 3 * 86400), 'missing');
  assert.equal(store.status('gbpusd', 'm1', DAY + 5 * 86400), 'missing');
  assert.equal(fs.existsSync(corrupt), false);
  assert.equal(fs.existsSync(foreign), false);
  assert.equal(fs.existsSync(stale), false);
  assert.equal((await store.readNative('gbpusd', 'm1', DAY - 86400, DAY + 2 * 86400)).length, 7);
  assert.ok(logs.some((l) => l.includes('recorded 3 bucket file(s)')));
  await store.flush();
  const manifest = readManifest(cache, 'gbpusd').tiers.m1;
  assert.deepEqual(manifest.fetched, [[DAY, DAY + 86400]]);
  assert.deepEqual(manifest.provisional, { [DAY - 86400]: received, [DAY + 86400]: end(DAY + 86400) + 5 });
  assert.ok(store.summary('gbpusd').bytes > 0);
});

test('reconcile on load: vanished files become missing; an unreadable manifest is rebuilt', async () => {
  const { cache, store, open, logs } = harness();
  await store.put('eurusd', 'm1', DAY, body(DAY, 2), final('m1', DAY));
  await store.put('eurusd', 'm1', DAY + 86400, body(DAY + 86400, 2), settled('m1', DAY + 86400));
  await store.put('eurusd', 'm1', DAY + 2 * 86400, null, final('m1', DAY + 2 * 86400));
  await store.flush();
  fs.rmSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-05'));
  const reopened = open();
  assert.equal(reopened.status('eurusd', 'm1', DAY), 'missing');
  assert.equal(reopened.status('eurusd', 'm1', DAY + 86400), 'final');
  // Empties have no file and survive.
  assert.equal(reopened.status('eurusd', 'm1', DAY + 2 * 86400), 'final');
  assert.deepEqual(reopened.missingBuckets('eurusd', 'm1', DAY, DAY + 3 * 86400), [DAY]);
  assert.ok(logs.some((l) => l.includes('1 recorded bucket(s) had no file')));

  fs.writeFileSync(manifestFile(cache, 'eurusd'), '{broken');
  const rebuilt = open();
  assert.equal(rebuilt.status('eurusd', 'm1', DAY + 86400), 'final');
  assert.ok(logs.some((l) => l.includes('unreadable')));
});

test('reconcile on load: a final copy written just before a crash promotes a provisional record', async () => {
  const { cache, store, open } = harness();
  await store.put('eurusd', 'm1', DAY, body(DAY, 2), { builtAt: nextBucket('m1', DAY) + 1 });
  await store.flush();
  // The final re-fetch reached the disk, the manifest did not.
  const file = bucketFile(cache, 'eurusd', 'm1', '2024-03-05');
  fs.writeFileSync(file, zlib.gzipSync(body(DAY, 3)));
  const mtime = nextBucket('m1', DAY) + 8 * 86400;
  fs.utimesSync(file, mtime, mtime);
  const reopened = open();
  assert.equal(reopened.status('eurusd', 'm1', DAY), 'final');
  assert.equal((await reopened.readNative('eurusd', 'm1', DAY, DAY + 86400)).length, 3);
});

test('self-healing reads: a missing or corrupt data file is forgotten and becomes missing', async () => {
  const { cache, store, logs } = harness();
  const changes: StoreChange[] = [];
  store.subscribe((c) => changes.push(c));
  await store.put('eurusd', 'm1', DAY, body(DAY, 2), final('m1', DAY));
  await store.put('eurusd', 'm1', DAY + 86400, body(DAY + 86400, 2), final('m1', DAY));
  await store.put('eurusd', 'm1', DAY + 2 * 86400, body(DAY + 2 * 86400, 2), { builtAt: NOW });
  // A fresh store has nothing cached in memory, so reads go to disk.
  const cold = new DukascopyStore(cache, { now: () => NOW * 1000, log: (l, m) => logs.push(`${l}: ${m}`) });
  await store.flush();
  fs.rmSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-06'));
  fs.writeFileSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-07'), 'garbage');
  // The first load already reconciles the vanished file; heal the rest on read.
  const bars = await cold.readNative('eurusd', 'm1', DAY, DAY + 3 * 86400);
  assert.deepEqual(
    bars.map((b) => b.t),
    [DAY, DAY + 60],
  );
  assert.equal(cold.status('eurusd', 'm1', DAY + 86400), 'missing');
  assert.equal(cold.status('eurusd', 'm1', DAY + 2 * 86400), 'missing');
  assert.equal(fs.existsSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-07')), false);
  assert.ok(logs.some((l) => l.includes('2024-03-07') && l.includes('fetched again')));

  // A file vanishing under a loaded store heals on the read that misses it.
  const warm = new DukascopyStore(cache, { now: () => NOW * 1000, lruSize: 0 });
  assert.equal(warm.status('eurusd', 'm1', DAY), 'final');
  const healed: StoreChange[] = [];
  warm.subscribe((c) => healed.push(c));
  fs.rmSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-05'));
  assert.deepEqual(await warm.readNative('eurusd', 'm1', DAY, DAY + 86400), []);
  assert.equal(warm.status('eurusd', 'm1', DAY), 'missing');
  assert.deepEqual(healed, [{ instrument: 'eurusd', tier: 'm1', from: DAY, to: DAY + 86400 }]);
  assert.equal(changes.length, 3);
});

test('the LRU serves recent buckets from memory and evicts old ones', async () => {
  const { cache, open } = harness();
  const store = open({ lruSize: 1 });
  await store.put('eurusd', 'm1', DAY, body(DAY, 2), final('m1', DAY));
  await store.put('eurusd', 'm1', DAY + 86400, body(DAY + 86400, 2), final('m1', DAY));
  // DAY + 1 is the most recent: its file can vanish and it still reads from memory.
  fs.rmSync(bucketFile(cache, 'eurusd', 'm1', '2024-03-06'));
  assert.equal((await store.readNative('eurusd', 'm1', DAY + 86400, DAY + 2 * 86400)).length, 2);
  // DAY was evicted: reading it goes to disk (and puts it in memory, evicting DAY + 1).
  assert.equal((await store.readNative('eurusd', 'm1', DAY, DAY + 86400)).length, 2);
  assert.equal((await store.readNative('eurusd', 'm1', DAY + 86400, DAY + 2 * 86400)).length, 0);
  assert.equal(store.status('eurusd', 'm1', DAY + 86400), 'missing');
});

test('unavailable, learned starts and recheck', async () => {
  const { store } = harness();
  const year = s('1971-01-01T00:00:00Z');
  store.markUnavailable('eurusd', 'd1', year + 1000);
  assert.equal(store.status('eurusd', 'd1', year), 'unavailable');
  assert.equal(store.has('eurusd', 'd1', year), false);
  assert.equal(store.needsFetch('eurusd', 'd1', year), false);
  assert.deepEqual(store.missingBuckets('eurusd', 'd1', year - 86400, nextBucket('d1', year)), [
    bucketStart('d1', year - 86400),
  ]);
  // A later answer with data wins over an old 404.
  await store.put('eurusd', 'd1', year, body(year, 2, 86400), final('d1', year));
  assert.equal(store.status('eurusd', 'd1', year), 'final');
  store.markUnavailable('eurusd', 'd1', year);
  assert.equal(store.status('eurusd', 'd1', year), 'final');

  store.markUnavailable('eurusd', 'h1', DAY);
  store.setLearnedStart('eurusd', 'm1', { t: s('2010-01-01T00:00:00Z'), evidence: 'empty streak' });
  assert.equal(store.effectiveStart('eurusd', 'm1'), s('2010-01-01T00:00:00Z'));
  assert.equal(store.tierStart('eurusd', 'm1'), s('2003-05-04T19:00:00Z'));
  // A learned start older than the metadata start never moves the start back.
  store.setLearnedStart('eurusd', 'h1', { t: 0, evidence: null });
  assert.equal(store.effectiveStart('eurusd', 'h1'), s('2003-05-04T19:00:00Z'));
  store.recheck('eurusd');
  assert.equal(store.learnedStart('eurusd', 'm1'), null);
  assert.equal(store.learnedStart('eurusd', 'h1'), null);
  assert.equal(store.status('eurusd', 'h1', DAY), 'missing');
  assert.equal(store.effectiveStart('eurusd', 'm1'), s('2003-05-04T19:00:00Z'));
});

test('summary, instruments and remove', async () => {
  const { cache, store } = harness();
  await store.put('eurusd', 'm1', DAY, body(DAY, 2), final('m1', DAY));
  await store.put('gbpusd', 'h1', bucketStart('h1', DAY), body(bucketStart('h1', DAY), 3, 3600), {
    builtAt: NOW,
    final: true,
  });
  await store.flush();
  assert.deepEqual(store.instruments(), ['eurusd', 'gbpusd']);
  const summary = store.summary('eurusd');
  assert.equal(summary.instrument, 'eurusd');
  assert.deepEqual(summary.tiers.m1.covered, [[DAY, DAY + 86400]]);
  assert.equal(summary.tiers.m1.start, s('2003-05-04T19:00:00Z'));
  assert.equal(summary.tiers.d1.start, s('1973-03-01T00:00:00Z'));
  assert.equal(summary.bytes, summary.tiers.m1.bytes);
  assert.ok(summary.bytes > 0);
  const freed = await store.remove('eurusd');
  assert.ok(freed >= summary.bytes);
  assert.equal(fs.existsSync(path.join(cache, 'dukascopy', 'eurusd')), false);
  assert.equal(store.status('eurusd', 'm1', DAY), 'missing');
  assert.deepEqual(store.instruments(), ['gbpusd']);
  assert.throws(() => store.status('../evil', 'm1', DAY), /Invalid Dukascopy instrument/);
});

test('changes are announced to subscribers', async () => {
  const { store } = harness();
  const changes: StoreChange[] = [];
  const off = store.subscribe((c) => changes.push(c));
  await store.put('eurusd', 'm1', DAY, null, final('m1', DAY));
  store.markUnavailable('eurusd', 'h1', DAY);
  off();
  await store.put('eurusd', 'm1', DAY + 86400, null, final('m1', DAY));
  assert.deepEqual(changes, [
    { instrument: 'eurusd', tier: 'm1', from: DAY, to: DAY + 86400 },
    { instrument: 'eurusd', tier: 'h1', from: bucketStart('h1', DAY), to: nextBucket('h1', DAY) },
  ]);
  await assert.rejects(store.put('eurusd', 'm1', DAY + 5, null, final('m1', DAY)), /not the start/);
});
