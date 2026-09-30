// Raw Dukascopy buckets on disk, one manifest per instrument saying what has been fetched:
//
//   <cache>/dukascopy/<instrument>/<tier>/<yyyy>/<bucketKey>.json.gz   gzip of the raw response
//   <cache>/dukascopy/<instrument>/manifest.json
//     {version: 1, tiers: {m1: {fetched: [[f, t]...], empty: [[f, t]...], provisional: {"<start>": builtAt},
//      unavailable: [[f, t]...], learnedStart: {t, evidence} | null, bytes}, h1: {...}, d1: {...}}}
//
// `fetched` holds final buckets (never requested again), `provisional` buckets built too early to be
// trusted as complete (served, re-fetched once when due), `empty` the fetched or provisional buckets
// that held no candles (no file is written for them), `unavailable` permanent 400/404 answers.
// Invariants: fetched and provisional are disjoint; every bucket in (fetched ∪ provisional) − empty
// has a file; a file is always written before the manifest records it, and deleted before the
// manifest stops recording it, so a crash leaves at worst a file the next load picks up again.
// The file's mtime is the bucket's build time as far as it is known; a load that finds a file the
// manifest does not list takes it as final only when that time is past CloudFront's 7-day copy (the
// migration's rule, whose times are local receive times), else provisional. Times are seconds;
// `now` options take ms like Date.now.
//
// API (class DukascopyStore, one per process, `new DukascopyStore(cacheDir)`):
//   has(instr, tier, start)          recorded as final or provisional (covered)
//   status(instr, tier, start)       'missing' | 'final' | 'provisional' | 'unavailable'
//   needsFetch(instr, tier, start, now?, {activeMaxAge?})   missing, due provisional, or a stale
//                                    active bucket (older than activeMaxAge s, default 60)
//   put(instr, tier, start, buffer | null, {builtAt, final?, fsync?})   stores a response (null = empty);
//                                    `final` defaults to "built >= 1 h after the bucket closed";
//                                    refuses (DecodeError) a response whose timestamp is outside the bucket
//   markUnavailable(instr, tier, start), clearUnavailable(instr, tier?)
//   learnedStart / setLearnedStart(instr, tier, {t, evidence} | null), recheck(instr) (clears both)
//   tierStart(instr, tier) (metadata), effectiveStart(instr, tier) (max of metadata and learned)
//   readNative(instr, tier, from, to)   bars of the tier's data buckets in [from, to), ascending;
//                                    self-heals: a data bucket whose file is gone or unreadable is
//                                    dropped from the manifest (it becomes missing) and logged
//   sets(instr, tier)                {fetched, empty, provisional, unavailable, covered, data} copies
//   missingBuckets(instr, tier, from, to)   starts neither covered nor unavailable
//   dueBuckets(instr, tier, now?)    provisional buckets due for their one re-fetch
//   refreshedAt(instr, tier, start)  when a provisional copy was last asked for (null otherwise)
//   summary(instr), instruments(), remove(instr) -> bytes freed, dropTier(instr, tier) -> bytes freed,
//   flush(), close()
//   subscribe(listener)              {instrument, tier, from, to} after every change (store.updated)
// Also exported: writeAtomic (unique temp + rename with retries), provisionalDue, decodeBucket (a
// response's bars, refusing another bucket's answer; reads heal such a file like a corrupt one), constants.

import { randomBytes } from 'node:crypto';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import zlib from 'node:zlib';

import { type Bar } from '../protocol.ts';
import {
  TIERS,
  type Tier,
  bucketKey,
  bucketStart,
  bucketsIn,
  isActive,
  isFinalBuild,
  nextBucket,
  parseBucketKey,
  tierStart,
} from './buckets.ts';
import { DecodeError, decode, parseResponse } from './decode.ts';
import { IntervalSet, type Range } from './intervals.ts';

const gzip = promisify(zlib.gzip);
const gunzip = promisify(zlib.gunzip);

/** A provisional completed bucket is re-fetched once CloudFront's 7-day copy has expired. */
export const DUE_AFTER_SECONDS = 7 * 86400 + 3600;
/** A bucket built while still open is worth one completed fetch this long after it closes. */
export const CLOSE_GRACE_SECONDS = 60;
export const ACTIVE_MAX_AGE_SECONDS = 60;
export const MANIFEST_VERSION = 1;

const INSTRUMENT_PATTERN = /^[a-z0-9]+$/;
const STALE_TEMP_MS = 60_000;

export interface LearnedStart {
  t: number;
  evidence: unknown;
}

export interface PutMeta {
  builtAt: number;
  final?: boolean;
  /** Flush the bucket file to disk before it is recorded (the migration deletes originals after). */
  fsync?: boolean;
}

export interface PutResult {
  empty: boolean;
  final: boolean;
  bars: number;
  bytes: number;
  /** False when the put was ignored (a provisional copy never replaces a final bucket). */
  changed: boolean;
}

export type BucketStatus = 'missing' | 'final' | 'provisional' | 'unavailable';

export interface TierSets {
  fetched: IntervalSet;
  empty: IntervalSet;
  provisional: IntervalSet;
  unavailable: IntervalSet;
  /** fetched ∪ provisional: what coverage treats as done. */
  covered: IntervalSet;
  /** covered − empty: where bars exist. */
  data: IntervalSet;
}

export interface TierSummary {
  tier: Tier;
  start: number | null;
  effectiveStart: number | null;
  fetched: Range[];
  empty: Range[];
  provisional: Range[];
  unavailable: Range[];
  covered: Range[];
  learnedStart: LearnedStart | null;
  bytes: number;
}

export interface InstrumentSummary {
  instrument: string;
  bytes: number;
  tiers: Record<Tier, TierSummary>;
}

export interface StoreChange {
  instrument: string;
  tier: Tier;
  from: number;
  to: number;
}

export type StoreListener = (change: StoreChange) => void;

export interface StoreOptions {
  /** Clock in ms (tests). */
  now?: () => number;
  log?: (level: 'info' | 'warn' | 'error', message: string) => void;
  /** Decoded buckets kept in memory. */
  lruSize?: number;
  /** Debounce of manifest writes; at most one write per this many ms. */
  manifestDelayMs?: number;
  /** Test hook for rename failures. */
  rename?: (from: string, to: string) => Promise<void>;
}

interface TierState {
  fetched: IntervalSet;
  empty: IntervalSet;
  provisional: Map<number, number>;
  unavailable: IntervalSet;
  learnedStart: LearnedStart | null;
  bytes: number;
}

type InstrumentState = Record<Tier, TierState>;

// ---------------------------------------------------------------------------------------------
// Atomic writes

const liveTemps = new Set<string>();
let tempSeq = 0;
const RETRYABLE = new Set(['EPERM', 'EACCES', 'EBUSY']);
const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

export interface AtomicWriteOptions {
  /** Sets the file's mtime (seconds) before it appears. */
  mtime?: number;
  fsync?: boolean;
  /** How long to keep retrying a rename Windows refuses (a reader, indexer or antivirus holds it). */
  retryForMs?: number;
  rename?: (from: string, to: string) => Promise<void>;
}

async function writeTemp(tmp: string, data: Buffer | string, opts: AtomicWriteOptions): Promise<void> {
  const handle = await fsp.open(tmp, 'w');
  try {
    await handle.writeFile(data);
    if (opts.fsync) await handle.sync();
  } finally {
    await handle.close();
  }
  if (opts.mtime !== undefined) await fsp.utimes(tmp, opts.mtime, opts.mtime);
}

/** Writes through a uniquely named temp file and renames it into place, so readers never see a
 *  partial file and concurrent writers never share a temp. Keeps the data in memory while it
 *  retries, rewriting the temp if it vanished. */
export async function writeAtomic(file: string, data: Buffer | string, opts: AtomicWriteOptions = {}): Promise<void> {
  const rename = opts.rename ?? fsp.rename;
  const tmp = `${file}.${process.pid}-${(tempSeq++).toString(36)}-${randomBytes(4).toString('hex')}.tmp`;
  liveTemps.add(tmp);
  try {
    await fsp.mkdir(path.dirname(file), { recursive: true });
    await writeTemp(tmp, data, opts);
    const deadline = Date.now() + (opts.retryForMs ?? 10_000);
    for (let wait = 20; ; wait = Math.min(wait * 2, 500)) {
      try {
        await rename(tmp, file);
        return;
      } catch (error) {
        const code = (error as NodeJS.ErrnoException).code ?? '';
        if (Date.now() >= deadline || !(RETRYABLE.has(code) || code === 'ENOENT')) throw error;
        await sleep(wait);
        if (code === 'ENOENT') {
          await fsp.mkdir(path.dirname(file), { recursive: true });
          if (!fs.existsSync(tmp)) await writeTemp(tmp, data, opts);
        }
      }
    }
  } catch (error) {
    await fsp.rm(tmp, { force: true }).catch(() => {});
    throw error;
  } finally {
    liveTemps.delete(tmp);
  }
}

// ---------------------------------------------------------------------------------------------
// Pure helpers

/** A provisional bucket's single re-fetch is due: right after it closes when it was built while
 *  open, else once CloudFront's copy (built `builtAt`) has expired. */
export function provisionalDue(tier: Tier, start: number, builtAt: number, now: number): boolean {
  const end = nextBucket(tier, start);
  if (builtAt < end) return now >= end + CLOSE_GRACE_SECONDS;
  return now >= builtAt + DUE_AFTER_SECONDS;
}

/** A raw response's bars, refusing (DecodeError) an answer for another bucket: a `?from=` request
 *  that crossed a close gets the next bucket, which must never replace this one's data. */
export function decodeBucket(tier: Tier, start: number, raw: Buffer | Uint8Array): Bar[] {
  const response = parseResponse(raw);
  const t = Math.floor(response.timestamp / 1000);
  if (t < start || t >= nextBucket(tier, start)) {
    const answered = bucketKey(tier, bucketStart(tier, t));
    throw new DecodeError(`the response is for ${answered}, not ${bucketKey(tier, start)}`);
  }
  return decode(response);
}

function emptyTier(): TierState {
  return {
    fetched: new IntervalSet(),
    empty: new IntervalSet(),
    provisional: new Map(),
    unavailable: new IntervalSet(),
    learnedStart: null,
    bytes: 0,
  };
}

function emptyInstrument(): InstrumentState {
  return { m1: emptyTier(), h1: emptyTier(), d1: emptyTier() };
}

function provisionalSet(tier: Tier, provisional: Map<number, number>): IntervalSet {
  const set = new IntervalSet();
  for (const start of provisional.keys()) set.add(start, nextBucket(tier, start));
  return set;
}

function parseTier(json: unknown): TierState {
  const state = emptyTier();
  if (json === undefined || json === null) return state;
  if (typeof json !== 'object') throw new Error('tier is not an object');
  const t = json as Record<string, unknown>;
  state.fetched = IntervalSet.fromJSON(t.fetched ?? []);
  state.empty = IntervalSet.fromJSON(t.empty ?? []);
  state.unavailable = IntervalSet.fromJSON(t.unavailable ?? []);
  const prov = t.provisional ?? {};
  if (typeof prov !== 'object' || Array.isArray(prov)) throw new Error('provisional is not an object');
  for (const [key, builtAt] of Object.entries(prov as Record<string, unknown>)) {
    const start = Number(key);
    if (!Number.isFinite(start) || typeof builtAt !== 'number' || !Number.isFinite(builtAt)) {
      throw new Error('provisional entry is not a number');
    }
    state.provisional.set(start, builtAt);
  }
  const learned = t.learnedStart;
  if (learned && typeof learned === 'object' && typeof (learned as LearnedStart).t === 'number') {
    state.learnedStart = { t: (learned as LearnedStart).t, evidence: (learned as LearnedStart).evidence ?? null };
  }
  state.bytes = typeof t.bytes === 'number' && Number.isFinite(t.bytes) ? t.bytes : 0;
  return state;
}

function parseManifest(json: unknown): InstrumentState {
  if (!json || typeof json !== 'object') throw new Error('not an object');
  const m = json as { version?: unknown; tiers?: Record<string, unknown> };
  if (m.version !== MANIFEST_VERSION) throw new Error(`unknown version ${String(m.version)}`);
  const tiers = m.tiers ?? {};
  return { m1: parseTier(tiers.m1), h1: parseTier(tiers.h1), d1: parseTier(tiers.d1) };
}

function serializeManifest(state: InstrumentState): unknown {
  const tiers: Record<string, unknown> = {};
  for (const tier of TIERS) {
    const s = state[tier];
    const provisional: Record<string, number> = {};
    for (const [start, builtAt] of [...s.provisional].sort((a, b) => a[0] - b[0])) provisional[start] = builtAt;
    tiers[tier] = {
      fetched: s.fetched.toJSON(),
      empty: s.empty.toJSON(),
      provisional,
      unavailable: s.unavailable.toJSON(),
      learnedStart: s.learnedStart,
      bytes: s.bytes,
    };
  }
  return { version: MANIFEST_VERSION, tiers };
}

// Recorded ranges outside this window are nonsense; never enumerate buckets beyond it.
const SANE_FROM = Date.UTC(1900, 0, 1) / 1000;
const saneTo = (now: number) => now + 2 * 366 * 86400;

// ---------------------------------------------------------------------------------------------

export class DukascopyStore {
  readonly root: string;
  readonly #now: () => number;
  readonly #log: (level: 'info' | 'warn' | 'error', message: string) => void;
  readonly #lruSize: number;
  readonly #manifestDelayMs: number;
  readonly #rename: ((from: string, to: string) => Promise<void>) | undefined;

  readonly #states = new Map<string, InstrumentState>();
  readonly #listeners = new Set<StoreListener>();
  // Per-bucket write chains, so two puts for one bucket never interleave.
  readonly #chains = new Map<string, Promise<unknown>>();
  // Bumped on every put or heal: a read that started earlier must not cache what it read.
  readonly #versions = new Map<string, number>();
  readonly #lastFetch = new Map<string, number>();
  readonly #lru = new Map<string, Bar[]>();
  readonly #removing = new Map<string, Promise<unknown>>();

  readonly #dirty = new Set<string>();
  #timer: ReturnType<typeof setTimeout> | null = null;
  #writing: Promise<void> | null = null;

  constructor(cacheDir: string, opts: StoreOptions = {}) {
    this.root = path.join(cacheDir, 'dukascopy');
    this.#now = opts.now ?? Date.now;
    this.#log = opts.log ?? (() => {});
    this.#lruSize = opts.lruSize ?? 300;
    this.#manifestDelayMs = opts.manifestDelayMs ?? 1000;
    this.#rename = opts.rename;
  }

  #nowSec(): number {
    return Math.floor(this.#now() / 1000);
  }

  #dir(instrument: string): string {
    if (!INSTRUMENT_PATTERN.test(instrument)) throw new Error(`Invalid Dukascopy instrument "${instrument}"`);
    return path.join(this.root, instrument);
  }

  #bucketPath(instrument: string, tier: Tier, start: number): string {
    const key = bucketKey(tier, start);
    return path.join(this.#dir(instrument), tier, key.slice(0, 4), `${key}.json.gz`);
  }

  // -------------------------------------------------------------------------------------------
  // Loading and reconciling

  #state(instrument: string): InstrumentState {
    const hit = this.#states.get(instrument);
    if (hit) return hit;
    const state = this.#load(instrument);
    this.#states.set(instrument, state);
    return state;
  }

  #load(instrument: string): InstrumentState {
    const dir = this.#dir(instrument);
    const manifestPath = path.join(dir, 'manifest.json');
    let state: InstrumentState;
    let trusted = true;
    try {
      state = parseManifest(JSON.parse(fs.readFileSync(manifestPath, 'utf8')));
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code !== 'ENOENT') {
        this.#log(
          'warn',
          `Dukascopy manifest for ${instrument} is unreadable (${(error as Error).message}); rebuilding it`,
        );
      }
      state = emptyInstrument();
      trusted = false;
    }
    this.#cleanTemps(dir, false);
    let changed = !trusted && fs.existsSync(dir);
    for (const tier of TIERS) changed = this.#reconcileTier(instrument, tier, state[tier], !trusted) || changed;
    if (changed) this.#markDirty(instrument);
    return state;
  }

  #cleanTemps(dir: string, recursive: boolean): void {
    let entries: fs.Dirent[];
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory() && recursive) this.#cleanTemps(full, true);
      else if (entry.isFile() && entry.name.endsWith('.tmp')) this.#removeStaleTemp(full);
    }
  }

  #removeStaleTemp(file: string): void {
    if (liveTemps.has(file)) return;
    try {
      if (Date.now() - fs.statSync(file).mtimeMs < STALE_TEMP_MS) return;
      fs.rmSync(file, { force: true });
    } catch {
      // Gone already, or locked: the next load tries again.
    }
  }

  /** Makes one tier's manifest agree with its files. Returns whether anything changed. */
  #reconcileTier(instrument: string, tier: Tier, s: TierState, recountBytes: boolean): boolean {
    const tierDir = path.join(this.#dir(instrument), tier);
    const present = new Map<number, string>();
    let years: fs.Dirent[] = [];
    try {
      years = fs.readdirSync(tierDir, { withFileTypes: true });
    } catch {
      // No files for this tier.
    }
    for (const year of years) {
      if (!year.isDirectory() || !/^\d{4}$/.test(year.name)) continue;
      const yearDir = path.join(tierDir, year.name);
      let names: string[] = [];
      try {
        names = fs.readdirSync(yearDir);
      } catch {
        continue;
      }
      for (const name of names) {
        const full = path.join(yearDir, name);
        if (name.endsWith('.tmp')) {
          this.#removeStaleTemp(full);
          continue;
        }
        if (!name.endsWith('.json.gz')) continue;
        const key = name.slice(0, -'.json.gz'.length);
        const start = parseBucketKey(tier, key);
        if (start !== null && key.startsWith(year.name)) present.set(start, full);
      }
    }

    let changed = false;
    const now = this.#nowSec();
    const covered = s.fetched.union(provisionalSet(tier, s.provisional));
    const data = covered.minus(s.empty).clip(SANE_FROM, saneTo(now));
    let lost = 0;
    for (const [f, t] of data) {
      for (const b of bucketsIn(tier, f, t)) {
        if (present.has(b)) continue;
        s.fetched.subtract(b, nextBucket(tier, b));
        s.provisional.delete(b);
        lost += 1;
      }
    }
    if (lost > 0) {
      changed = true;
      this.#log(
        'warn',
        `Dukascopy ${instrument} ${tier}: ${lost} recorded bucket(s) had no file and will be fetched again`,
      );
    }

    let adopted = 0;
    for (const [start, file] of present) {
      const end = nextBucket(tier, start);
      const recorded = (s.fetched.contains(start) || s.provisional.has(start)) && !s.empty.contains(start);
      if (recorded) {
        const builtAt = s.provisional.get(start);
        if (builtAt === undefined) continue;
        // A final copy written just before a crash, with the manifest still saying provisional.
        try {
          const mtime = Math.floor(fs.statSync(file).mtimeMs / 1000);
          if (mtime > builtAt && !isActive(tier, start, now) && isFinalBuild(tier, start, mtime)) {
            s.provisional.delete(start);
            s.fetched.add(start, end);
            changed = true;
          }
        } catch {
          // Unreadable: the next read heals it.
        }
        continue;
      }
      // A file the manifest does not know: keep it if it is a valid, non-empty bucket. Its mtime may
      // be a local receive time (a migration killed before its flush), and CloudFront may have served
      // an older copy, so it is final only past the 7-day copy, like the migration decides. A final
      // copy the fetcher wrote just before a crash costs at most one extra request.
      try {
        const bars = decodeBucket(tier, start, zlib.gunzipSync(fs.readFileSync(file)));
        if (bars.length === 0) throw new Error('empty bucket file');
        const mtime = Math.floor(fs.statSync(file).mtimeMs / 1000);
        s.empty.subtract(start, end);
        if (!isActive(tier, start, now) && mtime >= end + DUE_AFTER_SECONDS) {
          s.provisional.delete(start);
          s.fetched.add(start, end);
        } else {
          s.fetched.subtract(start, end);
          s.provisional.set(start, mtime);
        }
        adopted += 1;
      } catch (error) {
        this.#log(
          'warn',
          `Dukascopy ${instrument} ${tier}: removing invalid file ${path.basename(file)} (${(error as Error).message})`,
        );
        fs.rmSync(file, { force: true });
        present.delete(start);
      }
      changed = true;
    }
    if (adopted > 0)
      this.#log(
        'info',
        `Dukascopy ${instrument} ${tier}: recorded ${adopted} bucket file(s) missing from the manifest`,
      );

    if (changed || recountBytes) {
      let bytes = 0;
      for (const file of present.values()) {
        try {
          bytes += fs.statSync(file).size;
        } catch {
          // Counted on the next reconcile.
        }
      }
      if (bytes !== s.bytes) changed = true;
      s.bytes = bytes;
    }
    return changed;
  }

  // -------------------------------------------------------------------------------------------
  // Queries

  status(instrument: string, tier: Tier, start: number): BucketStatus {
    const s = this.#state(instrument)[tier];
    if (s.fetched.contains(start)) return 'final';
    if (s.provisional.has(bucketStart(tier, start))) return 'provisional';
    if (s.unavailable.contains(start)) return 'unavailable';
    return 'missing';
  }

  has(instrument: string, tier: Tier, start: number): boolean {
    const status = this.status(instrument, tier, start);
    return status === 'final' || status === 'provisional';
  }

  needsFetch(
    instrument: string,
    tier: Tier,
    start: number,
    now: number = this.#nowSec(),
    opts: { activeMaxAge?: number } = {},
  ): boolean {
    const s = this.#state(instrument)[tier];
    start = bucketStart(tier, start);
    if (s.fetched.contains(start)) return false;
    const builtAt = s.provisional.get(start);
    if (builtAt === undefined) return !s.unavailable.contains(start);
    if (isActive(tier, start, now)) {
      // An active copy may itself be cached; its age counts from when we last asked.
      const fetchedAt = Math.max(builtAt, this.#lastFetch.get(`${instrument}/${tier}/${start}`) ?? 0);
      return now - fetchedAt >= (opts.activeMaxAge ?? ACTIVE_MAX_AGE_SECONDS);
    }
    return provisionalDue(tier, start, builtAt, now);
  }

  /** When a provisional copy was last asked for (its build time, or a later cached answer); null for
   *  a bucket that is not provisional. Bars after it may still be missing from an active copy. */
  refreshedAt(instrument: string, tier: Tier, start: number): number | null {
    start = bucketStart(tier, start);
    const builtAt = this.#state(instrument)[tier].provisional.get(start);
    if (builtAt === undefined) return null;
    return Math.max(builtAt, this.#lastFetch.get(`${instrument}/${tier}/${start}`) ?? 0);
  }

  sets(instrument: string, tier: Tier): TierSets {
    const s = this.#state(instrument)[tier];
    const provisional = provisionalSet(tier, s.provisional);
    const covered = s.fetched.union(provisional);
    return {
      fetched: s.fetched.clone(),
      empty: s.empty.clone(),
      provisional,
      unavailable: s.unavailable.clone(),
      covered,
      data: covered.minus(s.empty),
    };
  }

  missingBuckets(instrument: string, tier: Tier, from: number, to: number): number[] {
    const { covered, unavailable } = this.sets(instrument, tier);
    const done = covered.union(unavailable);
    const out: number[] = [];
    for (const [f, t] of done.gaps(from, to)) {
      for (const b of bucketsIn(tier, f, t)) if (out[out.length - 1] !== b && !done.contains(b)) out.push(b);
    }
    return out;
  }

  dueBuckets(instrument: string, tier: Tier, now: number = this.#nowSec()): number[] {
    const s = this.#state(instrument)[tier];
    const out: number[] = [];
    for (const start of s.provisional.keys()) if (this.needsFetch(instrument, tier, start, now)) out.push(start);
    return out.sort((a, b) => a - b);
  }

  tierStart(instrument: string, tier: Tier): number | null {
    return tierStart(instrument, tier);
  }

  learnedStart(instrument: string, tier: Tier): LearnedStart | null {
    return this.#state(instrument)[tier].learnedStart;
  }

  effectiveStart(instrument: string, tier: Tier): number | null {
    const meta = tierStart(instrument, tier);
    const learned = this.learnedStart(instrument, tier)?.t ?? null;
    if (meta === null) return learned;
    return learned === null ? meta : Math.max(meta, learned);
  }

  summary(instrument: string): InstrumentSummary {
    const state = this.#state(instrument);
    let bytes = 0;
    const tiers = {} as Record<Tier, TierSummary>;
    for (const tier of TIERS) {
      const s = state[tier];
      const sets = this.sets(instrument, tier);
      bytes += s.bytes;
      tiers[tier] = {
        tier,
        start: tierStart(instrument, tier),
        effectiveStart: this.effectiveStart(instrument, tier),
        fetched: sets.fetched.toJSON(),
        empty: sets.empty.toJSON(),
        provisional: sets.provisional.toJSON(),
        unavailable: sets.unavailable.toJSON(),
        covered: sets.covered.toJSON(),
        learnedStart: s.learnedStart,
        bytes: s.bytes,
      };
    }
    return { instrument, bytes, tiers };
  }

  /** Instruments with anything stored (directories with a manifest or tier folders, or records not
   *  yet flushed), sorted. Merely asking about an instrument does not list it. */
  instruments(): string[] {
    const found = new Set<string>();
    for (const [instrument, state] of this.#states) {
      const recorded = TIERS.some((tier) => {
        const s = state[tier];
        return (
          !s.fetched.isEmpty || !s.empty.isEmpty || s.provisional.size > 0 || !s.unavailable.isEmpty || s.learnedStart
        );
      });
      if (recorded) found.add(instrument);
    }
    let entries: fs.Dirent[] = [];
    try {
      entries = fs.readdirSync(this.root, { withFileTypes: true });
    } catch {
      // Nothing stored yet.
    }
    for (const entry of entries) {
      if (!entry.isDirectory() || !INSTRUMENT_PATTERN.test(entry.name)) continue;
      const dir = path.join(this.root, entry.name);
      if (['manifest.json', ...TIERS].some((name) => fs.existsSync(path.join(dir, name)))) found.add(entry.name);
    }
    return [...found].sort();
  }

  // -------------------------------------------------------------------------------------------
  // Writes

  #serial<T>(key: string, task: () => Promise<T>): Promise<T> {
    const previous = this.#chains.get(key) ?? Promise.resolve();
    const next = previous.then(task, task);
    const settled = next.catch(() => {});
    this.#chains.set(key, settled);
    void settled.then(() => {
      if (this.#chains.get(key) === settled) this.#chains.delete(key);
    });
    return next;
  }

  #bump(key: string): void {
    this.#versions.set(key, (this.#versions.get(key) ?? 0) + 1);
  }

  async put(
    instrument: string,
    tier: Tier,
    start: number,
    buffer: Buffer | Uint8Array | null,
    meta: PutMeta,
  ): Promise<PutResult> {
    if (bucketStart(tier, start) !== start) throw new Error(`${start} is not the start of a ${tier} bucket`);
    await this.#removing.get(instrument);
    const key = `${instrument}/${tier}/${start}`;
    return this.#serial(key, async () => {
      const state = this.#state(instrument);
      const s = state[tier];
      const now = this.#nowSec();
      const builtAt = Math.floor(meta.builtAt);
      const final = meta.final ?? (!isActive(tier, start, now) && isFinalBuild(tier, start, builtAt));
      const end = nextBucket(tier, start);
      const bars = buffer ? decodeBucket(tier, start, buffer) : [];
      if (!final && s.fetched.contains(start)) {
        return { empty: bars.length === 0, final: true, bars: bars.length, bytes: 0, changed: false };
      }
      const file = this.#bucketPath(instrument, tier, start);
      const oldSize = await fsp.stat(file).then(
        (st) => st.size,
        () => 0,
      );
      let bytes = 0;
      // The file changes first, the manifest after (see the invariants above).
      if (bars.length > 0) {
        const gz = await gzip(buffer!);
        await writeAtomic(file, gz, { mtime: builtAt, rename: this.#rename, fsync: meta.fsync });
        bytes = gz.length;
      } else if (oldSize > 0) {
        await fsp.rm(file, { force: true });
      }
      if (this.#states.get(instrument) !== state) {
        // Removed while writing; the removal deletes what was just written.
        return { empty: bars.length === 0, final, bars: bars.length, bytes, changed: false };
      }
      s.bytes = Math.max(0, s.bytes + bytes - oldSize);
      if (bars.length > 0) s.empty.subtract(start, end);
      else s.empty.add(start, end);
      s.unavailable.subtract(start, end);
      if (final) {
        s.fetched.add(start, end);
        s.provisional.delete(start);
      } else {
        s.fetched.subtract(start, end);
        s.provisional.set(start, builtAt);
      }
      this.#lastFetch.set(key, now);
      this.#bump(key);
      if (bars.length > 0) this.#remember(key, bars);
      else this.#lru.delete(key);
      this.#markDirty(instrument);
      this.#emit({ instrument, tier, from: start, to: end });
      return { empty: bars.length === 0, final, bars: bars.length, bytes, changed: true };
    });
  }

  markUnavailable(instrument: string, tier: Tier, start: number): void {
    const s = this.#state(instrument)[tier];
    start = bucketStart(tier, start);
    if (s.fetched.contains(start) || s.provisional.has(start)) return;
    s.unavailable.add(start, nextBucket(tier, start));
    this.#markDirty(instrument);
    this.#emit({ instrument, tier, from: start, to: nextBucket(tier, start) });
  }

  clearUnavailable(instrument: string, tier?: Tier): void {
    const state = this.#state(instrument);
    for (const t of tier ? [tier] : TIERS) {
      const range = [state[t].unavailable.first(), state[t].unavailable.last()] as const;
      if (range[0] === null || range[1] === null) continue;
      state[t].unavailable = new IntervalSet();
      this.#markDirty(instrument);
      this.#emit({ instrument, tier: t, from: range[0], to: range[1] });
    }
  }

  setLearnedStart(instrument: string, tier: Tier, value: LearnedStart | null): void {
    const s = this.#state(instrument)[tier];
    const before = s.learnedStart?.t ?? null;
    s.learnedStart = value ? { t: value.t, evidence: value.evidence ?? null } : null;
    this.#markDirty(instrument);
    const after = value?.t ?? null;
    if (before !== after) {
      const points = [before, after].filter((t): t is number => t !== null);
      this.#emit({ instrument, tier, from: SANE_FROM, to: Math.max(...points) });
    }
  }

  /** "Check for older data" / "check again": forget learned starts and permanent 400/404s. */
  /** Forgets one tier of an instrument: its files, then what the manifest records of it. Returns the
   *  bytes freed; 0 when the tier held nothing. */
  async dropTier(instrument: string, tier: Tier): Promise<number> {
    await this.#removing.get(instrument);
    const state = this.#state(instrument);
    const prefix = `${instrument}/${tier}/`;
    await Promise.allSettled([...this.#chains].filter(([key]) => key.startsWith(prefix)).map(([, p]) => p));
    const s = state[tier];
    const dir = path.join(this.#dir(instrument), tier);
    const bytes = await diskUsage(dir);
    const recorded =
      !s.fetched.isEmpty || !s.empty.isEmpty || s.provisional.size > 0 || !s.unavailable.isEmpty || s.learnedStart;
    if (bytes === 0 && !recorded && !fs.existsSync(dir)) return 0;
    // Files first, the manifest after (see the invariants above): a crash in between leaves records
    // whose files are gone, which reads heal and the next drop clears.
    await fsp.rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    if (this.#states.get(instrument) !== state) return bytes;
    state[tier] = emptyTier();
    for (const key of [...this.#lru.keys()]) if (key.startsWith(prefix)) this.#lru.delete(key);
    for (const key of [...this.#versions.keys()]) if (key.startsWith(prefix)) this.#bump(key);
    this.#markDirty(instrument);
    this.#emit({ instrument, tier, from: SANE_FROM, to: saneTo(this.#nowSec()) });
    return bytes;
  }

  recheck(instrument: string): void {
    this.clearUnavailable(instrument);
    for (const tier of TIERS) if (this.learnedStart(instrument, tier)) this.setLearnedStart(instrument, tier, null);
  }

  /** Deletes everything stored for an instrument. Returns the bytes freed. */
  async remove(instrument: string): Promise<number> {
    const dir = this.#dir(instrument);
    const pending = [...this.#chains].filter(([key]) => key.startsWith(`${instrument}/`)).map(([, p]) => p);
    const task = (async () => {
      this.#states.delete(instrument);
      this.#dirty.delete(instrument);
      await Promise.allSettled(pending);
      if (this.#writing) await this.#writing.catch(() => {});
      const bytes = await diskUsage(dir);
      await fsp.rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
      return bytes;
    })();
    this.#removing.set(
      instrument,
      task.catch(() => {}),
    );
    try {
      return await task;
    } finally {
      this.#removing.delete(instrument);
      this.#states.delete(instrument);
      this.#dirty.delete(instrument);
      for (const key of [...this.#lru.keys()]) if (key.startsWith(`${instrument}/`)) this.#lru.delete(key);
      for (const tier of TIERS) this.#emit({ instrument, tier, from: SANE_FROM, to: saneTo(this.#nowSec()) });
    }
  }

  // -------------------------------------------------------------------------------------------
  // Reads

  #remember(key: string, bars: Bar[]): void {
    this.#lru.delete(key);
    this.#lru.set(key, bars);
    while (this.#lru.size > this.#lruSize) this.#lru.delete(this.#lru.keys().next().value!);
  }

  async readNative(instrument: string, tier: Tier, from: number, to: number): Promise<Bar[]> {
    await this.#removing.get(instrument);
    if (!(to > from)) return [];
    const { data } = this.sets(instrument, tier);
    const starts: number[] = [];
    for (const [f, t] of data.clip(bucketStart(tier, from), to)) {
      for (const b of bucketsIn(tier, f, t)) if (starts[starts.length - 1] !== b) starts.push(b);
    }
    const chunks = await mapLimit(starts, 8, (start) => this.#readBucket(instrument, tier, start));
    const out: Bar[] = [];
    for (const bars of chunks) for (const b of bars) if (b.t >= from && b.t < to) out.push(b);
    return out;
  }

  async #readBucket(instrument: string, tier: Tier, start: number): Promise<Bar[]> {
    const key = `${instrument}/${tier}/${start}`;
    const hit = this.#lru.get(key);
    if (hit) {
      this.#remember(key, hit);
      return hit;
    }
    const version = this.#versions.get(key) ?? 0;
    const state = this.#states.get(instrument);
    const file = this.#bucketPath(instrument, tier, start);
    let gz: Buffer;
    for (let attempt = 0; ; attempt += 1) {
      try {
        gz = await fsp.readFile(file);
        break;
      } catch (error) {
        const code = (error as NodeJS.ErrnoException).code ?? '';
        if (code === 'ENOENT') {
          this.#heal(instrument, tier, start, state, version, 'file missing', false);
          return [];
        }
        // Locked by an indexer or antivirus: wait a little, then give up without forgetting it.
        if (attempt >= 5 || !['EPERM', 'EACCES', 'EBUSY', 'EMFILE'].includes(code)) throw error;
        await sleep(50 * (attempt + 1));
      }
    }
    let bars: Bar[];
    try {
      bars = decodeBucket(tier, start, await gunzip(gz));
    } catch (error) {
      this.#heal(instrument, tier, start, state, version, (error as Error).message, true);
      return [];
    }
    if ((this.#versions.get(key) ?? 0) === version && this.#states.get(instrument) === state) {
      this.#remember(key, bars);
    }
    return bars;
  }

  /** A data bucket that cannot be read is forgotten, so it shows as missing and is fetched again. */
  #heal(
    instrument: string,
    tier: Tier,
    start: number,
    state: InstrumentState | undefined,
    version: number,
    reason: string,
    deleteFile: boolean,
  ): void {
    const key = `${instrument}/${tier}/${start}`;
    // Replaced or removed while we were reading: nothing to heal.
    if (!state || this.#states.get(instrument) !== state || (this.#versions.get(key) ?? 0) !== version) return;
    const s = state[tier];
    const end = nextBucket(tier, start);
    const file = this.#bucketPath(instrument, tier, start);
    if (deleteFile) {
      try {
        s.bytes = Math.max(0, s.bytes - fs.statSync(file).size);
        fs.rmSync(file, { force: true });
      } catch {
        // Already gone.
      }
    } else {
      // The vanished file's size is unknown: count the tier again.
      const tierDir = path.join(this.#dir(instrument), tier);
      void diskUsage(tierDir).then((bytes) => {
        if (this.#states.get(instrument) !== state || s.bytes === bytes) return;
        s.bytes = bytes;
        this.#markDirty(instrument);
      });
    }
    s.fetched.subtract(start, end);
    s.provisional.delete(start);
    this.#bump(key);
    this.#lru.delete(key);
    this.#log('warn', `Dukascopy ${instrument} ${tier} ${bucketKey(tier, start)}: ${reason}; it will be fetched again`);
    this.#markDirty(instrument);
    this.#emit({ instrument, tier, from: start, to: end });
  }

  // -------------------------------------------------------------------------------------------
  // Manifest writer: single flight, at most one write per manifestDelayMs, flush() on demand.

  #markDirty(instrument: string): void {
    this.#dirty.add(instrument);
    this.#schedule();
  }

  #schedule(): void {
    if (this.#timer || this.#writing || this.#dirty.size === 0) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.#writeDirty().catch(() => {});
    }, this.#manifestDelayMs);
    // The sidecar lives on stdin and flushes on shutdown; a pending write alone keeps nothing alive.
    this.#timer.unref?.();
  }

  async #writeDirty(): Promise<void> {
    if (this.#writing) return this.#writing;
    const list = [...this.#dirty];
    this.#dirty.clear();
    const errors: unknown[] = [];
    this.#writing = (async () => {
      for (const instrument of list) {
        const state = this.#states.get(instrument);
        if (!state) continue;
        // Serialized synchronously, so the file is one consistent snapshot.
        const json = JSON.stringify(serializeManifest(state));
        try {
          await writeAtomic(path.join(this.#dir(instrument), 'manifest.json'), json, {
            fsync: true,
            rename: this.#rename,
          });
        } catch (error) {
          if (this.#states.get(instrument) === state) this.#dirty.add(instrument);
          this.#log('error', `Could not save the Dukascopy manifest for ${instrument}: ${(error as Error).message}`);
          errors.push(error);
        }
      }
    })();
    try {
      await this.#writing;
    } finally {
      this.#writing = null;
      this.#schedule();
    }
    if (errors.length) throw errors[0];
  }

  /** Writes every pending manifest now (shutdown, migration). */
  async flush(): Promise<void> {
    const cancel = () => {
      if (this.#timer) clearTimeout(this.#timer);
      this.#timer = null;
    };
    cancel();
    // A write in flight may have snapshotted before the latest change; wait for it, then write
    // whatever is still dirty. A failed write stays dirty and its retry timer stays scheduled.
    if (this.#writing) await this.#writing.catch(() => {});
    cancel();
    if (this.#dirty.size) await this.#writeDirty();
  }

  async close(): Promise<void> {
    await Promise.allSettled([...this.#chains.values()]);
    await this.flush();
  }

  // -------------------------------------------------------------------------------------------

  subscribe(listener: StoreListener): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #emit(change: StoreChange): void {
    for (const listener of this.#listeners) {
      try {
        listener(change);
      } catch (error) {
        this.#log('error', `Store listener failed: ${(error as Error).message}`);
      }
    }
  }
}

async function diskUsage(dir: string): Promise<number> {
  let total = 0;
  let entries: fs.Dirent[];
  try {
    entries = await fsp.readdir(dir, { withFileTypes: true });
  } catch {
    return 0;
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) total += await diskUsage(full);
    else
      total += await fsp.stat(full).then(
        (s) => s.size,
        () => 0,
      );
  }
  return total;
}

async function mapLimit<T, R>(items: readonly T[], limit: number, fn: (item: T) => Promise<R>): Promise<R[]> {
  const out = new Array<R>(items.length);
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const i = next++;
      out[i] = await fn(items[i]!);
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return out;
}
