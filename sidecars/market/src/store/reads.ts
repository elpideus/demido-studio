// What chart and tool reads fetch before they read the store (the read itself is store/series, which
// never touches the network). Reads ride the fetcher's interactive lane within a request budget and
// a deadline, and serve what the store has when Dukascopy cannot answer.
//
// API (class Reads):
//   fetchFor(symbol, tf, from, to, budget, ms) -> {asked, ok, error?}   the 1-minute days reading
//       [from, to) needs; ok is false (with the reason) when a request failed or the deadline passed
//   recentFrom(symbol, now) -> where the automatic fetch starts, the same whatever the timeframe: the
//       last week, or from the newest 1-minute data stored before it (catching up after a while away,
//       at most CATCH_UP_SECONDS back); older history is an explicit download
//   latest(symbol, tf, count) -> ReadResult & {fetchError?}   bars.latest: the newest `count` stored
//       bars after the automatic fetch; fetchError when nothing is stored and a fetch failed
//   refreshDue(symbol, tf, from, to) -> requests   a tool read: re-fetches the due provisional buckets
//       it would show (a bucket stored while open stays partial until then); never missing ones,
//       which tools download with the user's approval
//   sweepDue() -> requests                      every due provisional bucket, bulk lane (start-up,
//       then now and then)

import { TIMEFRAMES, type Timeframe } from '../timeframes.ts';
import { TIERS, isActive } from './buckets.ts';
import { type DukascopyStore } from './dukascopy-store.ts';
import { type Fetcher } from './fetcher.ts';
import { type Planner } from './planner.ts';
import { type ReadResult, type Series } from './series.ts';

/** Most Dukascopy requests one chart open or freshness fill may make; beyond, the chart offers an update. */
export const INTERACTIVE_BUDGET = 60;
/** What a chart fetches by itself at least, whatever its timeframe: the last week of 1-minute candles. */
export const RECENT_SECONDS = 7 * 86400;
/** How far back a chart catches up by itself after a while away: its request budget in days. */
export const CATCH_UP_SECONDS = INTERACTIVE_BUDGET * 86400;
const FAR = 1e11;
/** Most due buckets one tool read re-fetches first; the periodic sweep gets the rest. */
export const REFRESH_BUDGET = 20;
const LATEST_MS = 25_000;
const REFRESH_MS = 10_000;

export interface ReadsDeps {
  store: Pick<DukascopyStore, 'instruments' | 'dueBuckets' | 'sets'>;
  fetcher: Pick<Fetcher, 'request'>;
  planner: Pick<Planner, 'interactive'>;
  series: Series;
  /** Milliseconds, like Date.now (deadlines always use real time). */
  now?: () => number;
}

/** Waits for the given requests, but not longer than `ms`; they go on in the background after. */
async function settled<T>(promises: Promise<T>[], ms: number): Promise<PromiseSettledResult<T>[] | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<null>((resolve) => (timer = setTimeout(() => resolve(null), ms)));
  try {
    return await Promise.race([Promise.allSettled(promises), timeout]);
  } finally {
    clearTimeout(timer);
  }
}

export class Reads {
  readonly #d: ReadsDeps;

  constructor(deps: ReadsDeps) {
    this.#d = deps;
  }

  #now(): number {
    return Math.floor((this.#d.now ?? Date.now)() / 1000);
  }

  async fetchFor(
    symbol: string,
    tf: Timeframe,
    from: number,
    to: number,
    budget: number,
    ms: number,
  ): Promise<{ asked: number; ok: boolean; error?: string }> {
    const { series, planner, fetcher } = this.#d;
    const instrument = series.route(symbol).instrument;
    if (!instrument) return { asked: 0, ok: true };
    const deadline = Date.now() + ms;
    let asked = 0;
    // Rounds: a day that failed or answered late is asked for again while budget and time remain.
    for (let round = 0; round < 3 && asked < budget && Date.now() < deadline; round += 1) {
      const needs = await planner.interactive(symbol, tf, from, to, budget - asked);
      if (!needs.length) break;
      asked += needs.length;
      const results = await settled(
        needs.map((n) =>
          fetcher.request({ instrument, tier: n.tier, start: n.start, lane: 'interactive', failFast: true }),
        ),
        Math.max(0, deadline - Date.now()),
      );
      if (!results) return { asked, ok: false, error: 'Dukascopy did not answer in time' };
      const failed = results.find((r): r is PromiseRejectedResult => r.status === 'rejected');
      if (failed) return { asked, ok: false, error: (failed.reason as Error)?.message ?? String(failed.reason) };
    }
    return { asked, ok: true };
  }

  /** Where a chart's automatic fetch starts, the same whatever its timeframe: the last week, or from
   *  the newest 1-minute data stored before it, so a chart opened after a while catches up (at most
   *  CATCH_UP_SECONDS back). Only the Dukascopy store counts: TradingView bars of one timeframe must
   *  not decide what the 1-minute history holds. Everything older is an explicit download. */
  recentFrom(symbol: string, now: number): number {
    const recent = now - RECENT_SECONDS;
    const instrument = this.#d.series.route(symbol).instrument;
    if (!instrument) return recent;
    const sets = this.#d.store.sets(instrument, 'm1');
    const last = sets.covered.union(sets.unavailable).clip(-FAR, recent).last();
    return Math.max(Math.min(last ?? recent, recent), now - CATCH_UP_SECONDS);
  }

  async latest(
    symbol: string,
    tf: Timeframe,
    count: number,
    ms = LATEST_MS,
  ): Promise<ReadResult & { fetchError?: string }> {
    const { series } = this.#d;
    const read = () => series.read({ symbol, tf, count, latest: true });
    if (!series.route(symbol).instrument) return read();
    const now = this.#now();
    const got = await this.fetchFor(
      symbol,
      tf,
      this.recentFrom(symbol, now),
      now + TIMEFRAMES[tf].seconds,
      INTERACTIVE_BUDGET,
      ms,
    );
    const result = await read();
    // An empty chart says why only when Dukascopy could not be reached (not for a closed market).
    if (!got.ok && result.bars.length === 0) return { ...result, fetchError: got.error ?? 'Offline' };
    return result;
  }

  async refreshDue(
    symbol: string,
    tf: Timeframe,
    from: number,
    to: number,
    budget = REFRESH_BUDGET,
    ms = REFRESH_MS,
  ): Promise<number> {
    const { series, fetcher } = this.#d;
    const instrument = series.route(symbol).instrument;
    if (!instrument) return 0;
    const due = series.due(symbol, tf, from, to).slice(0, budget);
    if (!due.length) return 0;
    await settled(
      due.map((d) =>
        fetcher.request({ instrument, tier: d.tier, start: d.start, lane: 'interactive', failFast: true }),
      ),
      ms,
    );
    return due.length;
  }

  sweepDue(): number {
    const { store, fetcher } = this.#d;
    const now = this.#now();
    let n = 0;
    for (const instrument of store.instruments()) {
      for (const tier of TIERS) {
        for (const start of store.dueBuckets(instrument, tier, now)) {
          // Not active buckets: reads refresh the ones someone looks at, and a sweep must not ask for
          // every market's newest data.
          if (isActive(tier, start, now)) continue;
          fetcher.request({ instrument, tier, start, lane: 'bulk' }).catch(() => undefined);
          n += 1;
        }
      }
    }
    return n;
  }
}
