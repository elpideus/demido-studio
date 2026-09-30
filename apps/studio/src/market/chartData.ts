// The chart's bookkeeping that needs no React: which source each bar came from, which downloads
// belong to the chart, and the ranges the download popup offers. Every time is in seconds.

import type {
  Bar,
  MarketGap,
  MarketJob,
  MarketJobStatus,
  MarketKeys,
  MarketLatestBars,
  MarketRange,
  MarketSource,
  MarketSpan,
} from '@/lib/types';

/** One span over bars that all came from one source (a live TradingView stream). */
export function spanOf(bars: Bar[], source: MarketSource): MarketSpan[] {
  const first = bars[0];
  const last = bars[bars.length - 1];
  return first && last ? [{ source, from: first.t, to: last.t, count: bars.length }] : [];
}

/** Both lists in time order, with neighbouring runs from the same source joined. Pages are contiguous
 *  in bars, so a run that ends one page and starts the next is one run. */
export function mergeSpans(a: MarketSpan[], b: MarketSpan[]): MarketSpan[] {
  const out: MarketSpan[] = [];
  for (const span of [...a, ...b].sort((x, y) => x.from - y.from)) {
    const prev = out[out.length - 1];
    if (prev && prev.source === span.source) {
      out[out.length - 1] = {
        source: prev.source,
        from: prev.from,
        to: Math.max(prev.to, span.to),
        count: prev.count + span.count,
      };
    } else {
      out.push({ ...span });
    }
  }
  return out;
}

/** The source of the bar at `t`: the last span starting at or before it, so a live bar newer than every
 *  span belongs to the newest run. Null only when there are no spans. */
export function sourceAt(spans: MarketSpan[], t: number): MarketSource | null {
  let found: MarketSource | null = spans[0]?.source ?? null;
  for (const span of spans) {
    if (span.from > t) break;
    found = span.source;
  }
  return found;
}

/** `sourceAt` for every bar of a time-ordered list, in one pass. */
export function sourcesOf(bars: Bar[], spans: MarketSpan[]): Array<MarketSource | null> {
  const out: Array<MarketSource | null> = [];
  let i = 0;
  let current: MarketSource | null = spans[0]?.source ?? null;
  for (const bar of bars) {
    while (i < spans.length && spans[i]!.from <= bar.t) current = spans[i++]!.source;
    out.push(current);
  }
  return out;
}

const same = (a: string | undefined, b: string) => !!a && a.toLowerCase() === b.toLowerCase();

/** Whether a store key (`store.updated`, a job) is one the chart reads from. */
export function keysMatch(keys: MarketKeys, source: MarketSource, key: string): boolean {
  return source === 'dukascopy' ? same(keys.dukascopy, key) : same(keys.tradingview, key);
}

/** A job fills this chart when it writes one of the chart's keys; before the keys are known, when it
 *  was started for the same symbol. */
export function isChartJob(job: MarketJob, keys: MarketKeys, symbol: string): boolean {
  return keysMatch(keys, job.source, job.key) || same(job.symbol, symbol);
}

const RANK: Record<MarketJob['status'], number> = { running: 0, waiting: 0, queued: 0, paused: 1, error: 2, done: 3 };

/** Fetching, or about to: a paused or failed job only moves again when someone resumes it. */
export const isMoving = (status: MarketJobStatus) =>
  status === 'running' || status === 'waiting' || status === 'queued';

/** The job the toolbar shows: one that is moving before a paused one before a failed one, the most
 *  recently updated within each. */
export function pickJob(jobs: MarketJob[]): MarketJob | null {
  let best: MarketJob | null = null;
  for (const job of jobs) {
    if (job.status === 'done') continue;
    if (!best || RANK[job.status] < RANK[best.status]) best = job;
    else if (RANK[job.status] === RANK[best.status] && job.updatedAt > best.updatedAt) best = job;
  }
  return best;
}

const DAY = 86_400;

/** Whether what a job fetches serves a chart timeframe: a Dukascopy download is 1-minute candles, which
 *  serve every timeframe; a TradingView job only the timeframes it pages. */
function servesTimeframe(job: MarketJob, timeframe: string): boolean {
  if (job.source === 'tradingview') return job.tiers.includes(timeframe);
  return job.tiers.includes('m1');
}

/**
 * The moving job whose range and tiers take in the whole gap at the chart's edge: the popup shows its
 * progress instead of the picker, since every choice there would only wait for it. A paused or failed
 * job, or one over another range or detail, never hides the picker: it would leave the gap unfilled.
 */
export function coveringJob(jobs: readonly MarketJob[], gap: MarketGap | null, timeframe: string): MarketJob | null {
  if (!gap) return null;
  let best: MarketJob | null = null;
  for (const job of jobs) {
    if (!isMoving(job.status) || job.source !== gap.source) continue;
    if (job.from > gap.from || job.to < gap.to || !servesTimeframe(job, timeframe)) continue;
    if (!best || job.updatedAt > best.updatedAt) best = job;
  }
  return best;
}

/** How the chart's toolbar and download popup show its unfinished downloads. */
export interface ChartJobs {
  /** The download the toolbar shows (moving first, then paused, then failed). */
  lead: MarketJob | null;
  /** A download is fetching: the toolbar shows it instead of the Download and "Update to today" buttons. */
  moving: boolean;
  /** Shown by the popup instead of its picker. */
  covering: MarketJob | null;
  /** Shown as one line above the popup's picker, with its Resume or Pause. */
  other: MarketJob | null;
}

export function chartJobs(jobs: MarketJob[], gap: MarketGap | null, timeframe: string): ChartJobs {
  const lead = pickJob(jobs);
  const covering = coveringJob(jobs, gap, timeframe);
  return { lead, moving: !!lead && isMoving(lead.status), covering, other: covering ? null : lead };
}

/** The unfinished jobs with `job` replaced or added; a finished job drops out. A stale copy (older
 *  `updatedAt`) never replaces a newer one. */
export function upsertJob(jobs: MarketJob[], job: MarketJob): MarketJob[] {
  const current = jobs.find((j) => j.id === job.id);
  if (current && current.updatedAt > job.updatedAt) return jobs;
  const rest = jobs.filter((j) => j.id !== job.id);
  return job.status === 'done' ? rest : [...rest, job];
}

/** A fresh listing replacing the jobs held: a job missing from it was cancelled (which sends no event),
 *  while a copy held that is newer than the listed one (a progress event that overtook the reply) stays. */
export function mergeListed(held: MarketJob[], listed: MarketJob[]): MarketJob[] {
  return listed
    .filter((job) => job.status !== 'done')
    .map((job) => {
      const current = held.find((j) => j.id === job.id);
      return current && current.updatedAt > job.updatedAt ? current : job;
    });
}

/** Where older history for the chart comes from: the gap says so when known; otherwise Dukascopy
 *  whenever the symbol has a Dukascopy key (it fills everything TradingView does not). */
export function historySource(keys: MarketKeys, gap: MarketGap | null): MarketSource {
  if (gap) return gap.source;
  return keys.dukascopy ? 'dukascopy' : 'tradingview';
}

/** What the signed-out chart shows for its first read; `offer` opens the download popup at once. */
export type LatestView = { mode: 'history'; offer: boolean } | { mode: 'signin' } | { mode: 'error'; message: string };

/**
 * A market with Dukascopy history shows as history even with no bars: nothing stored near now (a first
 * open, or a weekend whose empty days were all the read reached) is an edge to download from, not a
 * failure. Only a fetch that really failed says Dukascopy could not be reached.
 */
export function latestView(res: Pick<MarketLatestBars, 'bars' | 'more' | 'stale' | 'keys' | 'fetchError'>): LatestView {
  if (res.bars.length) return { mode: 'history', offer: false };
  // A market only TradingView has (or its bars saved earlier): nothing to show until sign-in.
  if (res.stale || !res.keys?.dukascopy) return { mode: 'signin' };
  if (res.fetchError) {
    return {
      mode: 'error',
      message: `Nothing is stored for this market yet, and Dukascopy could not be reached: ${res.fetchError}`,
    };
  }
  if (res.more === 'none') return { mode: 'error', message: 'Dukascopy has no history for this market.' };
  return { mode: 'history', offer: res.more === 'gap' };
}

/** `t` moved by whole calendar months in UTC (negative goes back), clamped to the month's last day. */
export function shiftMonths(t: number, months: number): number {
  const d = new Date(t * 1000);
  const day = d.getUTCDate();
  d.setUTCDate(1);
  d.setUTCMonth(d.getUTCMonth() + months);
  const last = new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 0)).getUTCDate();
  d.setUTCDate(Math.min(day, last));
  return Math.floor(d.getTime() / 1000);
}

export type HistoryOption = 'month' | 'year' | 'all' | 'from' | 'gap';

export interface OptionContext {
  gap: MarketGap | null;
  /** The "From date…" pick, `YYYY-MM-DD` (UTC). */
  fromDate: string;
  now: number;
}

/** `YYYY-MM-DD` → seconds at 00:00 UTC, or null when it is not a date. */
export function parseDay(value: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
  const ms = Date.parse(`${value}T00:00:00Z`);
  return Number.isFinite(ms) ? Math.floor(ms / 1000) : null;
}

/** Seconds → `YYYY-MM-DD` in UTC, for a date field's bounds. */
export function formatDay(t: number): string {
  return new Date(t * 1000).toISOString().slice(0, 10);
}

/**
 * The range a popup option downloads, or null when it cannot be downloaded as picked. Every range
 * reaches now, so one download leaves the whole history from its start to today stored, and none of
 * them depends on the chart's timeframe: history is 1-minute candles, which every timeframe is built
 * from. "1 more month / year" count from the stored 1-minute history (the store knows where it starts),
 * "From date" and "Fill the gap" start at their date, Everything at the source's first data.
 */
export function optionRange(option: HistoryOption, ctx: OptionContext): MarketRange | null {
  switch (option) {
    case 'month':
      return { back: 'month' };
    case 'year':
      return { back: 'year' };
    case 'all':
      return {};
    case 'from': {
      const from = parseDay(ctx.fromDate);
      return from !== null && from < ctx.now ? { from } : null;
    }
    case 'gap':
      return ctx.gap && ctx.gap.from < ctx.gap.to ? { from: ctx.gap.from } : null;
  }
}

/** Whether the newest bar is older than market closures explain (a weekend, a holiday), so the chart
 *  offers to bring the stored history up to today. */
export function isBehind(lastT: number, tfSeconds: number, now: number): boolean {
  return now - lastT > tfSeconds + 3 * DAY;
}
