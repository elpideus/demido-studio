// Pure geometry and text behind the Data tab's coverage timelines, kept apart from React so it can
// be tested. Every time is in seconds; ranges are half-open [from, to).

import type {
  MarketCoverageItem,
  MarketJob,
  MarketJobStatus,
  MarketPlan,
  MarketSource,
  MarketTierCoverage,
} from '@/lib/types';

export type Range = [number, number];

/** Sorts ranges and merges overlapping or touching ones, dropping empty ones. */
export function normalize(ranges: readonly Range[] | undefined): Range[] {
  const sorted = (ranges ?? []).filter(([f, t]) => t > f).sort((a, b) => a[0] - b[0]);
  const out: Range[] = [];
  for (const [f, t] of sorted) {
    const last = out[out.length - 1];
    if (last && f <= last[1]) last[1] = Math.max(last[1], t);
    else out.push([f, t]);
  }
  return out;
}

/** `a` minus `b`; both normalized. */
export function subtract(a: readonly Range[], b: readonly Range[]): Range[] {
  const out: Range[] = [];
  let j = 0;
  for (const [from, to] of a) {
    let f = from;
    while (j < b.length && b[j]![1] <= f) j += 1;
    for (let k = j; k < b.length && b[k]![0] < to; k += 1) {
      const [bf, bt] = b[k]!;
      if (bf > f) out.push([f, bf]);
      f = Math.max(f, bt);
      if (f >= to) break;
    }
    if (f < to) out.push([f, to]);
  }
  return out;
}

/** The part of normalized `ranges` inside [from, to). */
export function clip(ranges: readonly Range[], from: number, to: number): Range[] {
  const out: Range[] = [];
  for (const [f, t] of ranges) {
    const cf = Math.max(f, from);
    const ct = Math.min(t, to);
    if (ct > cf) out.push([cf, ct]);
  }
  return out;
}

const length = (ranges: readonly Range[]) => ranges.reduce((sum, [f, t]) => sum + (t - f), 0);

/**
 * The time a tier's timeline spans: what it could hold (`available`), widened to anything it
 * holds outside that, or just what it holds when the source reports no range.
 */
export function tierSpan(tier: MarketTierCoverage): Range | null {
  const held = normalize([...tier.intervals, ...(tier.empty ?? [])]);
  const first = held[0];
  const last = held[held.length - 1];
  if (tier.available && tier.available[1] > tier.available[0]) {
    const [af, at] = tier.available;
    return [Math.min(af, first?.[0] ?? af), Math.max(at, last?.[1] ?? at)];
  }
  return first && last ? [first[0], last[1]] : null;
}

/** One axis for every timeline of a market, so tiers line up: the union of their spans. */
export function marketDomain(item: MarketCoverageItem): Range | null {
  let from = Infinity;
  let to = -Infinity;
  for (const source of item.sources) {
    for (const tier of source.tiers) {
      const span = tierSpan(tier);
      if (!span) continue;
      from = Math.min(from, span[0]);
      to = Math.max(to, span[1]);
    }
  }
  return to > from ? [from, to] : null;
}

/** The earliest and latest stored time of a market, whatever the tier or source. */
export function storedExtent(item: MarketCoverageItem): Range | null {
  let from = Infinity;
  let to = -Infinity;
  for (const source of item.sources) {
    for (const tier of source.tiers) {
      const data = subtract(normalize(tier.intervals), normalize(tier.empty));
      const first = data[0];
      const last = data[data.length - 1];
      if (first) from = Math.min(from, first[0]);
      if (last) to = Math.max(to, last[1]);
    }
  }
  return to > from ? [from, to] : null;
}

/** `data`: stored bars; `empty`: fetched, but the source has nothing there; `gap`: not downloaded. */
export type SegmentKind = 'data' | 'empty' | 'gap';

export interface Segment {
  kind: SegmentKind;
  from: number;
  to: number;
}

/** Consecutive segments tiling `span` exactly. The source may or may not count empties as covered. */
export function segmentsOf(tier: MarketTierCoverage, span: Range): Segment[] {
  const [from, to] = span;
  const empty = clip(normalize(tier.empty), from, to);
  const data = clip(subtract(normalize(tier.intervals), empty), from, to);
  const marked = [
    ...data.map(([f, t]): Segment => ({ kind: 'data', from: f, to: t })),
    ...empty.map(([f, t]): Segment => ({ kind: 'empty', from: f, to: t })),
  ].sort((a, b) => a.from - b.from);
  const out: Segment[] = [];
  let at = from;
  for (const seg of marked) {
    if (seg.from > at) out.push({ kind: 'gap', from: at, to: seg.from });
    out.push(seg);
    at = seg.to;
  }
  if (at < to) out.push({ kind: 'gap', from: at, to });
  return out;
}

/**
 * Folds runs of stored and empty segments shorter than `minLength` into one segment of their
 * dominant kind. Minute data alternates five stored weekdays with an empty weekend; drawn raw over
 * 20 years that is a thousand sub-pixel stripes, and hovering it would flicker between them.
 * Gaps are never folded: what is missing is what the timeline is for, so each keeps its exact dates.
 */
export function simplify(segments: readonly Segment[], minLength: number): Segment[] {
  const out: Segment[] = [];
  const push = (seg: Segment) => {
    const last = out[out.length - 1];
    if (last && last.kind === seg.kind && last.to === seg.from) last.to = seg.to;
    else out.push({ ...seg });
  };
  let group: Segment[] = [];
  const flush = () => {
    const run = group;
    group = [];
    const first = run[0];
    const last = run[run.length - 1];
    if (!first || !last) return;
    const prev = out[out.length - 1];
    // A short leftover (the weekend before a gap, the one at the end) joins the run it follows.
    if (last.to - first.from < minLength && prev && prev.kind !== 'gap' && prev.to === first.from) {
      prev.to = last.to;
      return;
    }
    let data = 0;
    let empty = 0;
    for (const seg of run) {
      if (seg.kind === 'data') data += seg.to - seg.from;
      else empty += seg.to - seg.from;
    }
    push({ kind: data >= empty ? 'data' : 'empty', from: first.from, to: last.to });
  };
  for (const seg of segments) {
    if (seg.kind === 'gap' || seg.to - seg.from >= minLength) {
      flush();
      push(seg);
      continue;
    }
    group.push(seg);
    if (seg.to - group[0]!.from >= minLength) flush();
  }
  flush();
  return out;
}

/** The segment under time `t`, if any. */
export function segmentAt(segments: readonly Segment[], t: number): Segment | null {
  return segments.find((seg) => t >= seg.from && t < seg.to) ?? null;
}

const DAY_SECONDS = 86_400;
// Longest known-empty stretch (a weekend with a holiday) that still reads as part of the gap around it.
const CLOSURE_SECONDS = 4 * DAY_SECONDS;

/**
 * The stretches of a tier's available time that are neither stored nor known empty: what a download
 * would fill. A weekend known to be empty does not split a stretch not downloaded around it (only
 * stored bars do), and a hole shorter than a day at the newest end is left out: the next read fetches it.
 */
export function tierGaps(tier: MarketTierCoverage): Range[] {
  const available = tier.available;
  if (!available || !(available[1] > available[0])) return [];
  const empty = normalize(tier.empty);
  const data = subtract(normalize(tier.intervals), empty);
  const out: Range[] = [];
  for (const [f, t] of subtract([[available[0], available[1]]], normalize([...tier.intervals, ...empty]))) {
    const last = out[out.length - 1];
    if (last && f - last[1] <= CLOSURE_SECONDS && clip(data, last[1], f).length === 0) last[1] = t;
    else out.push([f, t]);
  }
  return out.filter(([f, t]) => t < available[1] || t - f >= DAY_SECONDS);
}

/** Share of `span` that is downloaded (stored or known empty), 0..1. */
export function coveredShare(tier: MarketTierCoverage, span: Range): number {
  const total = span[1] - span[0];
  if (total <= 0) return 0;
  const covered = clip(normalize([...tier.intervals, ...(tier.empty ?? [])]), span[0], span[1]);
  return Math.min(1, length(covered) / total);
}

/** "96%", "<1%"; 100% only when nothing at all is missing. */
export function shareText(share: number): string {
  if (share >= 1) return '100%';
  if (share <= 0) return '0%';
  const pct = Math.floor(share * 100);
  return pct === 0 ? '<1%' : `${Math.min(99, pct)}%`;
}

/** Position of `t` along `domain` in percent. */
export function pct(t: number, domain: Range): number {
  return ((t - domain[0]) / (domain[1] - domain[0])) * 100;
}

// Data is bucketed by UTC day, month and year, so dates read in UTC.
const DAY = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' });
const MONTH = new Intl.DateTimeFormat(undefined, { month: 'short', year: 'numeric', timeZone: 'UTC' });
const DAY_MONTH = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', timeZone: 'UTC' });

export const formatDay = (t: number) => DAY.format(t * 1000);
export const formatMonth = (t: number) => MONTH.format(t * 1000);

/** "4 May 2003 – 26 Sep 2026" for [from, to); the end is exclusive, so the last day shown is `to - 1`. */
export function rangeText(from: number, to: number): string {
  const a = formatDay(from);
  const b = formatDay(Math.max(from, to - 1));
  return a === b ? a : `${a} – ${b}`;
}

/** "May 2003 – Sep 2026", for a market's summary line. */
export function monthRangeText(from: number, to: number): string {
  const a = formatMonth(from);
  const b = formatMonth(Math.max(from, to - 1));
  return a === b ? a : `${a} – ${b}`;
}

export interface Tick {
  t: number;
  label: string;
}

const YEAR_SECONDS = 365.25 * 86_400;
const utc = (y: number, m = 0, d = 1) => Date.UTC(y, m, d) / 1000;

/** At most `max` round-dated ticks strictly inside [from, to): years, else months, else days. */
export function axisTicks(from: number, to: number, max = 7): Tick[] {
  if (!(to > from) || max < 1) return [];
  const years = (to - from) / YEAR_SECONDS;
  const start = new Date(from * 1000);
  const out: Tick[] = [];
  if (years >= 2) {
    const step = [1, 2, 5, 10, 20, 25, 50, 100, 200].find((s) => years / s <= max) ?? 500;
    for (let y = Math.ceil(start.getUTCFullYear() / step) * step; utc(y) < to; y += step) {
      if (utc(y) > from) out.push({ t: utc(y), label: String(y) });
    }
    return out;
  }
  const months = years * 12;
  if (months >= 2) {
    const step = [1, 2, 3, 6].find((s) => months / s <= max) ?? 12;
    let y = start.getUTCFullYear();
    let m = Math.ceil(start.getUTCMonth() / step) * step;
    for (; utc(y, m) < to; m += step) {
      if (utc(y, m) > from) out.push({ t: utc(y, m), label: MONTH.format(utc(y, m) * 1000) });
    }
    return out;
  }
  const days = (to - from) / 86_400;
  const step = [1, 2, 7, 14].find((s) => days / s <= max) ?? 28;
  const first = utc(start.getUTCFullYear(), start.getUTCMonth(), start.getUTCDate());
  for (let t = first; t < to; t += step * 86_400) {
    if (t > from) out.push({ t, label: DAY_MONTH.format(t * 1000) });
  }
  return out;
}

const DUKASCOPY_ORDER = ['m1', 'h1', 'd1'];
const UNIT_SECONDS: Record<string, number> = { s: 1, m: 60, h: 3600, d: 86_400, w: 604_800, M: 2_592_000 };

/** Seconds in a TradingView timeframe like `15m`, `4h`, `1W`; Infinity when unknown. */
function tfSeconds(tf: string): number {
  const match = /^(\d*)([smhdwWM])$/.exec(tf);
  if (!match) return Infinity;
  const unit = match[2] === 'W' ? 'w' : match[2]!;
  return Number(match[1] || 1) * (UNIT_SECONDS[unit] ?? Infinity);
}

/** Finest tier first: Dukascopy m1, h1, d1; TradingView by timeframe length. */
export function sortTiers<T extends { tier: string }>(source: MarketSource, tiers: readonly T[]): T[] {
  const rank = (tier: string) => {
    if (source === 'dukascopy') {
      const i = DUKASCOPY_ORDER.indexOf(tier);
      return i < 0 ? DUKASCOPY_ORDER.length : i;
    }
    return tfSeconds(tier);
  };
  return [...tiers].sort((a, b) => rank(a.tier) - rank(b.tier) || a.tier.localeCompare(b.tier));
}

/** Dukascopy first; it is the deeper history. */
export function sortSources<T extends { source: MarketSource }>(sources: readonly T[]): T[] {
  return [...sources].sort((a, b) => Number(a.source !== 'dukascopy') - Number(b.source !== 'dukascopy'));
}

/** What "Open chart" and "Download missing" pass on: the first TradingView symbol, else the Dukascopy key. */
export function chartSymbol(item: MarketCoverageItem): string {
  return item.symbols[0] ?? item.sources.find((s) => s.source === 'dukascopy')?.key ?? item.market;
}

export function marketBytes(item: MarketCoverageItem): number {
  return item.sources.reduce((sum, s) => sum + s.bytes, 0);
}

export const isUnfinished = (status: MarketJobStatus) => status !== 'done';

const isActive = (status: MarketJobStatus) => status === 'running' || status === 'waiting' || status === 'queued';

/**
 * Every unfinished job, from the job list and from the summary's per-market lists, newest first.
 * A job only just started may not be in the summary yet; the list may be missing if its call failed.
 */
export function unfinishedJobs(items: readonly MarketCoverageItem[], listed: readonly MarketJob[] | null): MarketJob[] {
  const byId = new Map<string, MarketJob>();
  for (const job of [...(listed ?? []), ...items.flatMap((i) => i.jobs)]) {
    if (!isUnfinished(job.status)) continue;
    const seen = byId.get(job.id);
    if (!seen || job.updatedAt > seen.updatedAt) byId.set(job.id, job);
  }
  return [...byId.values()].sort((a, b) => b.createdAt - a.createdAt);
}

/** The unfinished jobs that fill `item`: matched by any of its store keys or symbols. */
export function jobsFor(item: MarketCoverageItem, jobs: readonly MarketJob[]): MarketJob[] {
  const keys = new Set([item.market, ...item.symbols, ...item.sources.map((s) => s.key)]);
  return jobs.filter((job) => keys.has(job.key) || keys.has(job.symbol));
}

/** The job a market row summarises: a moving one first, else the newest stopped one. */
export function leadJob(jobs: readonly MarketJob[]): MarketJob | null {
  return jobs.find((job) => isActive(job.status)) ?? jobs[0] ?? null;
}

/**
 * Why a plan adds no requests of its own, or null when it does. `complete`: everything is stored;
 * `stopped`: a paused or failed job covers the rest, and starting resumes it; `busy`: other downloads
 * already fetch the rest; `nothing`: nothing is left to ask for (the rest is unavailable at the source).
 */
export type PlanIdle = 'complete' | 'stopped' | 'busy' | 'nothing';

export function planIdle(plan: MarketPlan): PlanIdle | null {
  if (plan.complete) return 'complete';
  if (plan.requests > 0) return null;
  if (plan.job?.status === 'paused' || plan.job?.status === 'error') return 'stopped';
  if (plan.job || plan.queuedAhead > 0) return 'busy';
  return 'nothing';
}

export function sortMarkets(items: readonly MarketCoverageItem[]): MarketCoverageItem[] {
  return [...items].sort((a, b) => a.name.localeCompare(b.name) || a.market.localeCompare(b.market));
}
