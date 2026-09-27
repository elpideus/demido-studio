// What chart and tool reads fetch before they read the store (the read itself is store/series, which
// never touches the network). Reads ride the fetcher's interactive lane within a request budget and
// a deadline, and serve what the store has when Dukascopy cannot answer.
//
// API (class Reads):
//   fetchFor(symbol, tf, from, to, budget, ms) -> {asked, ok, error?}   what reading [from, to) at tf
//       needs, in rounds (an empty coarse bucket reveals finer ones); ok is false (with the reason)
//       when a request failed or the deadline passed
//   latest(symbol, tf, count) -> ReadResult & {fetchError?}   bars.latest: the newest `count` bars
//       after fetching what they need, walking back over closed-market buckets (weekends, holidays)
//       to the last trading ones; fetchError when nothing is stored and a fetch failed
//   refreshDue(symbol, tf, from, to) -> requests   a tool read: re-fetches the due provisional buckets
//       it would show (a bucket stored while open stays partial until then); never missing ones,
//       which tools download with the user's approval
//   sweepDue() -> requests                      every due provisional bucket, bulk lane (start-up,
//       then now and then)

import { TIMEFRAMES, type Timeframe } from '../timeframes.ts';
import { TIERS, isActive } from './buckets.ts';
import { type DukascopyStore } from './dukascopy-store.ts';
import { type Fetcher } from './fetcher.ts';
import { type Range } from './intervals.ts';
import { type Planner } from './planner.ts';
import { type ReadResult, type Series } from './series.ts';

/** Most Dukascopy requests one chart open or freshness fill may make; beyond, the chart offers an update. */
export const INTERACTIVE_BUDGET = 60;
/** Most due buckets one tool read re-fetches first; the periodic sweep gets the rest. */
export const REFRESH_BUDGET = 20;
const LATEST_MS = 25_000;
const REFRESH_MS = 10_000;

export interface ReadsDeps {
  store: Pick<DukascopyStore, 'instruments' | 'dueBuckets'>;
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

  async latest(
    symbol: string,
    tf: Timeframe,
    count: number,
    ms = LATEST_MS,
  ): Promise<ReadResult & { fetchError?: string }> {
    const { series } = this.#d;
    const read = () => series.read({ symbol, tf, count, latest: true });
    if (!series.route(symbol).instrument) return read();
    const step = TIMEFRAMES[tf].seconds;
    const deadline = Date.now() + ms;
    const now = this.#now();
    // First the time the newest bars span when the market trades throughout.
    let got = await this.fetchFor(symbol, tf, now - Math.ceil(count * step * 1.5), now + step, INTERACTIVE_BUDGET, ms);
    let asked = got.asked;
    let result = await read();
    // Then what they still miss. On a weekend that window held no bars, so this walks back to Friday.
    let previous = '';
    while (got.ok && asked < INTERACTIVE_BUDGET && Date.now() < deadline) {
      const want = this.#latestNeeds(symbol, tf, count, result, now);
      const key = JSON.stringify(want);
      // Nothing left to fetch, or the last round did not move the gap.
      if (!want.length || key === previous) break;
      previous = key;
      // One span, so the budget goes to the newest buckets first; what lies between is covered.
      const from = Math.min(...want.map((r) => r[0]));
      const to = Math.max(...want.map((r) => r[1]));
      got = await this.fetchFor(symbol, tf, from, to, INTERACTIVE_BUDGET - asked, deadline - Date.now());
      if (got.asked === 0) break;
      asked += got.asked;
      result = await read();
    }
    // An empty chart says why only when Dukascopy could not be reached (not for a closed market).
    if (!got.ok && result.bars.length === 0) return { ...result, fetchError: got.error ?? 'Offline' };
    return result;
  }

  /** Uncovered time between the oldest bar served and now (the newest stored bars may be days
   *  behind), and when fewer than `count` came back, the stretch below the gap they stopped at. */
  #latestNeeds(symbol: string, tf: Timeframe, count: number, read: ReadResult, now: number): Range[] {
    const oldest = read.bars[0]?.t;
    const out = oldest === undefined ? [] : this.#d.series.missing(symbol, tf, oldest, now);
    if (read.bars.length < count && read.more === 'gap') out.push(...read.missing);
    return out;
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
