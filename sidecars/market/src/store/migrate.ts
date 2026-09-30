// One-time move of the caches earlier versions left behind into the store. Idempotent: it runs on
// every start and finds nothing to do once done. It runs to completion before jobs load and before
// store requests are answered (main.ts queues them).
//
// - Dukascopy: dukascopy-node's flat cache, one raw response per file
//   (<cache>/dukascopy/candles%2F{minute|hour|day}%2F{CODE}%2FBID%2F{y}[%2F{m}[%2F{d}]].json). Every
//   file is parsed and validated; empty responses are recorded as empty; a file written less than
//   7 d + 1 h after its bucket closed becomes provisional (built at its mtime), else final. Invalid
//   files and unknown codes are left in place and logged. Phase 1 writes every bucket (fsynced) and
//   every manifest (flushed); phase 2 deletes the originals.
// - TradingView: bar files (tvcache's format, kept) without coverage get intervals rebuilt from their
//   bars (split where bars are further apart than the TradingView tolerance). The old coverage.json
//   only lends its symbol spellings, and is deleted after.
// - Jobs: unfinished old downloads (<cache>/downloads/*.json, status error/paused/running) become
//   new paused jobs over their range; finished ones are dropped; downloads/ is deleted after.
// - Coarse tiers: history is 1-minute candles only, every timeframe built from them. The hourly and
//   daily buckets earlier versions downloaded beside them are no longer read, so they are deleted,
//   with what the manifests record of them.
//
// API: migrate({cacheDir, store, tv, route, log?, now?}) -> MigrationReport

import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';

import { TIMEFRAMES, type Timeframe } from '../timeframes.ts';
import { type Tier, allInstruments, bucketKey, instrumentCode, nextBucket } from './buckets.ts';
import { decode, parseResponse } from './decode.ts';
import { DUE_AFTER_SECONDS, type DukascopyStore, writeAtomic } from './dukascopy-store.ts';
import { weekModeFor } from './aggregate.ts';
import { type JobRecord, convertedJob } from './jobs.ts';
import { type Route } from './series.ts';
import { type TvStore, fileKey, intervalsOf } from './tv-store.ts';

export interface MigrationReport {
  dukascopy: {
    files: number;
    migrated: Record<Tier, number>;
    empty: number;
    provisional: number;
    /** Recorded by an earlier, interrupted run. */
    already: number;
    invalid: string[];
    unknown: string[];
    deleted: number;
  };
  tradingview: { series: number };
  jobs: { converted: number; dropped: number };
  /** Hourly and daily tiers deleted, and the bytes that freed. */
  coarse: { tiers: number; bytes: number };
}

export interface MigrateDeps {
  cacheDir: string;
  store: DukascopyStore;
  tv: TvStore;
  route: (symbol: string) => Route;
  log?: (level: 'info' | 'warn' | 'error', message: string) => void;
  /** Clock in ms (tests). */
  now?: () => number;
}

const SOURCES: Record<string, Tier> = { minute: 'm1', hour: 'h1', day: 'd1' };
const FLAT = /^candles\/(minute|hour|day)\/([^/]+)\/BID\/(\d{4})(?:\/(\d{1,2}))?(?:\/(\d{1,2}))?$/;

interface FlatFile {
  file: string;
  instrument: string;
  tier: Tier;
  start: number;
}

/** Where a flat dukascopy-node file belongs, or why it does not. */
export function parseFlatName(
  name: string,
  codes: ReadonlyMap<string, string>,
): { ok: Omit<FlatFile, 'file'> } | { invalid: string } | { unknown: string } {
  let key: string;
  try {
    key = decodeURIComponent(name.slice(0, -'.json'.length));
  } catch {
    return { invalid: 'not a cache key' };
  }
  const m = FLAT.exec(key);
  if (!m) return { invalid: 'not a completed candle bucket' };
  const tier = SOURCES[m[1]!]!;
  const instrument = codes.get(m[2]!);
  if (!instrument) return { unknown: m[2]! };
  const y = Number(m[3]);
  const month = m[4] === undefined ? null : Number(m[4]);
  const day = m[5] === undefined ? null : Number(m[5]);
  if ((tier === 'm1') !== (day !== null) || (tier === 'd1') !== (month === null)) {
    return { invalid: 'date does not match the tier' };
  }
  const start = Date.UTC(y, (month ?? 1) - 1, day ?? 1) / 1000;
  const expected = tier === 'd1' ? `${y}` : tier === 'h1' ? `${y}-${String(month).padStart(2, '0')}` : null;
  const valid =
    tier === 'm1'
      ? bucketKey(tier, start) === `${y}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`
      : bucketKey(tier, start) === expected;
  if (!valid) return { invalid: 'no such date' };
  return { ok: { instrument, tier, start } };
}

async function mapLimit<T>(items: readonly T[], limit: number, fn: (item: T) => Promise<void>): Promise<void> {
  let next = 0;
  const worker = async () => {
    while (next < items.length) await fn(items[next++]!);
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
}

export async function migrate(deps: MigrateDeps): Promise<MigrationReport> {
  const { cacheDir, store, tv } = deps;
  const log = deps.log ?? (() => {});
  const nowMs = (deps.now ?? Date.now)();
  const report: MigrationReport = {
    dukascopy: {
      files: 0,
      migrated: { m1: 0, h1: 0, d1: 0 },
      empty: 0,
      provisional: 0,
      already: 0,
      invalid: [],
      unknown: [],
      deleted: 0,
    },
    tradingview: { series: 0 },
    jobs: { converted: 0, dropped: 0 },
    coarse: { tiers: 0, bytes: 0 },
  };

  // ---------------------------------------------------------------------------------------------
  // Phase 1: everything written and flushed, nothing deleted.

  const flatDir = path.join(cacheDir, 'dukascopy');
  let names: string[] = [];
  try {
    names = (await fsp.readdir(flatDir, { withFileTypes: true }))
      .filter((e) => e.isFile() && e.name.startsWith('candles%2F') && e.name.endsWith('.json'))
      .map((e) => e.name);
  } catch {
    // No flat cache.
  }
  const migrated: string[] = [];
  if (names.length) {
    const codes = new Map<string, string>();
    for (const key of allInstruments()) {
      const code = instrumentCode(key);
      if (code) codes.set(code, key);
    }
    report.dukascopy.files = names.length;
    await mapLimit(names, 8, async (name) => {
      const file = path.join(flatDir, name);
      const parsed = parseFlatName(name, codes);
      if ('invalid' in parsed) {
        report.dukascopy.invalid.push(name);
        log('warn', `Migration: left ${name} in place (${parsed.invalid})`);
        return;
      }
      if ('unknown' in parsed) {
        report.dukascopy.unknown.push(name);
        log('warn', `Migration: left ${name} in place (unknown Dukascopy code ${parsed.unknown})`);
        return;
      }
      const { instrument, tier, start } = parsed.ok;
      try {
        const [buffer, stat] = await Promise.all([fsp.readFile(file), fsp.stat(file)]);
        const response = parseResponse(buffer);
        const end = nextBucket(tier, start);
        if (response.timestamp / 1000 < start || response.timestamp / 1000 >= end) {
          throw new Error('its timestamp is outside its bucket');
        }
        const bars = decode(response);
        if (store.status(instrument, tier, start) !== 'missing') {
          report.dukascopy.already += 1;
        } else {
          const mtime = Math.floor(stat.mtimeMs / 1000);
          const final = mtime >= end + DUE_AFTER_SECONDS;
          await store.put(instrument, tier, start, bars.length ? buffer : null, { builtAt: mtime, final, fsync: true });
          report.dukascopy.migrated[tier] += 1;
          if (!bars.length) report.dukascopy.empty += 1;
          if (!final) report.dukascopy.provisional += 1;
        }
        migrated.push(file);
      } catch (error) {
        report.dukascopy.invalid.push(name);
        log('warn', `Migration: left ${name} in place (${(error as Error).message})`);
      }
    });
    await store.flush();
  }

  // TradingView coverage from the bars (old coverage.json names the symbols the file names mangle).
  const spelled = new Map<string, string>();
  const coveragePath = path.join(cacheDir, 'coverage.json');
  try {
    const old = JSON.parse(await fsp.readFile(coveragePath, 'utf8')) as Record<string, unknown>;
    for (const key of Object.keys(old)) {
      const m = /^tradingview:(.+):([^:]+)$/.exec(key);
      if (!m || !(m[2]! in TIMEFRAMES)) continue;
      const symbol = m[1]!.toUpperCase();
      spelled.set(fileKey(symbol, m[2] as Timeframe), symbol);
    }
  } catch {
    // No old coverage: file names are enough.
  }
  const renamed: string[] = [];
  for (const key of tv.unindexedFiles()) {
    const m = /^(.+)_(1m|5m|15m|30m|1h|4h|1d|1w|1mo|1M)$/.exec(key);
    if (!m) continue;
    const tf = (m[2] === '1mo' ? '1M' : m[2]) as Timeframe;
    // EXCHANGE_TICKER: the last underscore separates them (FX_IDC_AUDCHF -> FX_IDC:AUDCHF).
    const stem = m[1]!;
    const cut = stem.lastIndexOf('_');
    const symbol = spelled.get(key) ?? (cut > 0 ? `${stem.slice(0, cut)}:${stem.slice(cut + 1)}` : stem);
    const bars = tv.readFile(key);
    if (!bars.length) continue;
    const weekends = weekModeFor(symbol, TIMEFRAMES[tf].seconds <= 86400 ? bars : null) === 'session';
    if (fileKey(symbol, tf) !== key) {
      // tvcache named monthly files like the minute ones: store them under the new name.
      tv.record(symbol, tf, bars, null);
      renamed.push(path.join(tv.root, `${key}.json`));
    }
    tv.setIntervals(symbol, tf, intervalsOf(bars, tf, weekends));
    report.tradingview.series += 1;
  }
  if (report.tradingview.series) await tv.flush();

  // Old download jobs.
  const oldJobs = path.join(cacheDir, 'downloads');
  const jobsDir = path.join(cacheDir, 'jobs');
  let jobsConverted = true;
  let oldNames: string[] = [];
  try {
    oldNames = (await fsp.readdir(oldJobs)).filter((n) => n.endsWith('.json'));
  } catch {
    // None.
  }
  if (oldNames.length) {
    // An interrupted earlier run may have converted some already.
    const done = new Set<string>();
    try {
      for (const n of await fsp.readdir(jobsDir)) {
        try {
          const r = JSON.parse(await fsp.readFile(path.join(jobsDir, n), 'utf8')) as { migratedFrom?: string };
          if (r.migratedFrom) done.add(r.migratedFrom);
        } catch {
          // Not ours to judge here.
        }
      }
    } catch {
      // No jobs yet.
    }
    for (const name of oldNames) {
      try {
        const old = JSON.parse(await fsp.readFile(path.join(oldJobs, name), 'utf8')) as Record<string, unknown>;
        const status = String(old.status ?? '');
        if (!['error', 'paused', 'running'].includes(status)) {
          report.jobs.dropped += 1;
          continue;
        }
        const id = typeof old.id === 'string' ? old.id : name;
        if (done.has(id)) continue;
        const symbol = typeof old.symbol === 'string' ? old.symbol : String(old.instrument ?? '');
        const instrument = typeof old.instrument === 'string' ? old.instrument : '';
        // The old job named its instrument; route it the way the store would today.
        const route = deps.route(old.source === 'dukascopy' && instrument ? instrument : symbol);
        const job = convertedJob(old, route, nowMs) as (JobRecord & { migratedFrom?: string }) | null;
        if (!job) {
          report.jobs.dropped += 1;
          continue;
        }
        job.symbol = symbol || route.key;
        job.migratedFrom = id;
        if (job.source === 'dukascopy') {
          // What it would still fetch, so the paused job shows a sensible bar before it resumes.
          for (const entry of job.perTier) {
            const tier = entry.tier as Tier;
            const eff = store.effectiveStart(job.key, tier);
            const lo = Math.max(job.from, eff ?? Infinity);
            const hi = Math.min(job.to, Math.floor(nowMs / 1000));
            entry.total = eff !== null && hi > lo ? store.missingBuckets(job.key, tier, lo, hi).length : 0;
          }
          job.total = job.perTier.reduce((sum, t) => sum + t.total, 0);
        }
        await writeAtomic(path.join(jobsDir, `${job.id}.json`), JSON.stringify(job), { fsync: true });
        report.jobs.converted += 1;
      } catch (error) {
        jobsConverted = false;
        log('warn', `Migration: could not convert the old download ${name}: ${(error as Error).message}`);
      }
    }
  }

  // ---------------------------------------------------------------------------------------------
  // Phase 2: the originals go.

  for (const file of migrated) {
    try {
      await fsp.rm(file, { force: true, maxRetries: 5, retryDelay: 100 });
      report.dukascopy.deleted += 1;
    } catch (error) {
      log('warn', `Migration: could not delete ${path.basename(file)}: ${(error as Error).message}`);
    }
  }
  for (const file of renamed) await fsp.rm(file, { force: true }).catch(() => {});
  if (fs.existsSync(coveragePath)) await fsp.rm(coveragePath, { force: true }).catch(() => {});
  if (jobsConverted && fs.existsSync(oldJobs)) {
    await fsp.rm(oldJobs, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }).catch(() => {});
  }

  // ---------------------------------------------------------------------------------------------
  // Phase 3: only 1-minute candles are history now; hourly and daily tiers go.

  let dropped = false;
  for (const instrument of store.instruments()) {
    for (const tier of ['h1', 'd1'] as const) {
      try {
        const sets = store.sets(instrument, tier);
        const recorded =
          !sets.covered.isEmpty || !sets.unavailable.isEmpty || store.learnedStart(instrument, tier) !== null;
        const bytes = await store.dropTier(instrument, tier);
        if (bytes === 0 && !recorded) continue;
        report.coarse.tiers += 1;
        report.coarse.bytes += bytes;
        dropped = true;
      } catch (error) {
        log('warn', `Migration: could not delete the ${tier} history of ${instrument}: ${(error as Error).message}`);
      }
    }
  }
  if (dropped) {
    await store.flush();
    log(
      'info',
      `Migration: deleted ${report.coarse.tiers} hourly/daily tier(s) (${report.coarse.bytes} bytes); history is 1-minute candles`,
    );
  }

  const d = report.dukascopy;
  if (d.files || report.tradingview.series || report.jobs.converted || report.jobs.dropped) {
    log(
      'info',
      `Migration: ${d.migrated.m1} m1, ${d.migrated.h1} h1, ${d.migrated.d1} d1 bucket(s) moved (${d.empty} empty, ` +
        `${d.provisional} provisional, ${d.already} already done), ${d.deleted} original(s) deleted, ` +
        `${d.invalid.length + d.unknown.length} left in place; ${report.tradingview.series} TradingView series ` +
        `indexed; ${report.jobs.converted} old download(s) kept as paused jobs, ${report.jobs.dropped} dropped`,
    );
  }
  return report;
}
