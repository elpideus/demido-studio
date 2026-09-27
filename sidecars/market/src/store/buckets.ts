// Dukascopy's storage unit: one API response ("bucket") per tier and period. m1 buckets hold one
// UTC day of minute candles, h1 buckets one UTC month of hourly candles, d1 buckets one UTC year of
// daily candles. All times are seconds since the epoch; bucket starts are UTC period starts.
//
// API:
//   TIERS, type Tier ('m1' | 'h1' | 'd1'), TIER_SOURCE (URL path segment), TIER_BAR_SECONDS
//   bucketStart(tier, t)            start of the bucket containing t
//   nextBucket(tier, t)             start of the bucket after the one containing t (= its end)
//   bucketsIn(tier, from, to)       starts of every bucket overlapping [from, to), ascending
//   bucketKey(tier, start)          '2024-03-05' / '2024-03' / '2024' (file names)
//   parseBucketKey(tier, key)       the inverse, or null for anything that is not a valid key
//   isActive(tier, start, now)      the bucket contains now (still being built by Dukascopy)
//   isFinalBuild(tier, start, builtAt)   built at least an hour after the bucket closed
//   completedUrl(code, tier, start) / activeUrl(code, tier, start) / bucketUrl(code, tier, start, now)
//   instrumentMeta(instrument), instrumentCode(instrument)
//   tierStart(instrument, tier)     first instant with data per the metadata (rule below), or null
//   tierStartFromMeta(meta, tier)   the same rule on a metadata record (tests run it on all of them)

import dukascopy from 'dukascopy-node';

export type Tier = 'm1' | 'h1' | 'd1';

export const TIERS: readonly Tier[] = ['m1', 'h1', 'd1'];

export const TIER_SOURCE: Record<Tier, 'minute' | 'hour' | 'day'> = { m1: 'minute', h1: 'hour', d1: 'day' };

export const TIER_BAR_SECONDS: Record<Tier, number> = { m1: 60, h1: 3600, d1: 86400 };

export const DATA_API_ROOT = 'https://jetta.dukascopy.com/v1';

/** A completed bucket's copy is final once it was built this long after the bucket closed. */
export const FINAL_AFTER_SECONDS = 3600;

export interface InstrumentMeta {
  name: string;
  code: string;
  description: string;
  startHourForTicks: string;
  startDayForMinuteCandles: string;
  startMonthForHourlyCandles: string;
  startYearForDailyCandles: string;
}

const { instrumentMetaData } = dukascopy as unknown as { instrumentMetaData: Record<string, InstrumentMeta> };

export function isTier(value: unknown): value is Tier {
  return value === 'm1' || value === 'h1' || value === 'd1';
}

export function bucketStart(tier: Tier, t: number): number {
  const d = new Date(Math.floor(t) * 1000);
  const y = d.getUTCFullYear();
  if (tier === 'd1') return Date.UTC(y, 0, 1) / 1000;
  if (tier === 'h1') return Date.UTC(y, d.getUTCMonth(), 1) / 1000;
  return Math.floor(t / 86400) * 86400;
}

export function nextBucket(tier: Tier, t: number): number {
  const d = new Date(bucketStart(tier, t) * 1000);
  const y = d.getUTCFullYear();
  if (tier === 'd1') return Date.UTC(y + 1, 0, 1) / 1000;
  if (tier === 'h1') return Date.UTC(y, d.getUTCMonth() + 1, 1) / 1000;
  return bucketStart(tier, t) + 86400;
}

export function bucketsIn(tier: Tier, from: number, to: number): number[] {
  const out: number[] = [];
  if (!(to > from)) return out;
  for (let b = bucketStart(tier, from); b < to; b = nextBucket(tier, b)) out.push(b);
  return out;
}

const pad = (n: number) => String(n).padStart(2, '0');

export function bucketKey(tier: Tier, start: number): string {
  const d = new Date(start * 1000);
  const y = String(d.getUTCFullYear()).padStart(4, '0');
  if (tier === 'd1') return y;
  if (tier === 'h1') return `${y}-${pad(d.getUTCMonth() + 1)}`;
  return `${y}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
}

const KEY_PATTERN: Record<Tier, RegExp> = {
  m1: /^(\d{4})-(\d{2})-(\d{2})$/,
  h1: /^(\d{4})-(\d{2})$/,
  d1: /^(\d{4})$/,
};

export function parseBucketKey(tier: Tier, key: string): number | null {
  const m = KEY_PATTERN[tier].exec(key);
  if (!m) return null;
  const y = Number(m[1]);
  const month = m[2] === undefined ? 1 : Number(m[2]);
  const day = m[3] === undefined ? 1 : Number(m[3]);
  const ms = Date.UTC(y, month - 1, day);
  const start = ms / 1000;
  // Reject keys like 2024-02-31 that Date.UTC silently rolls over.
  return bucketKey(tier, start) === key ? start : null;
}

export function isActive(tier: Tier, start: number, now: number): boolean {
  return now >= start && now < nextBucket(tier, start);
}

export function isFinalBuild(tier: Tier, start: number, builtAt: number): boolean {
  return builtAt >= nextBucket(tier, start) + FINAL_AFTER_SECONDS;
}

export function completedUrl(code: string, tier: Tier, start: number): string {
  const d = new Date(start * 1000);
  const base = `${DATA_API_ROOT}/candles/${TIER_SOURCE[tier]}/${code}/BID/${d.getUTCFullYear()}`;
  if (tier === 'd1') return base;
  if (tier === 'h1') return `${base}/${d.getUTCMonth() + 1}`;
  return `${base}/${d.getUTCMonth() + 1}/${d.getUTCDate()}`;
}

export function activeUrl(code: string, tier: Tier, start: number): string {
  return `${DATA_API_ROOT}/candles/${TIER_SOURCE[tier]}/${code}/BID?from=${start * 1000}`;
}

/** The URL to ask for a bucket right now: the completed period URL, or `?from=` while it is open. */
export function bucketUrl(code: string, tier: Tier, start: number, now: number): string {
  return isActive(tier, start, now) ? activeUrl(code, tier, start) : completedUrl(code, tier, start);
}

export function instrumentMeta(instrument: string): InstrumentMeta | null {
  return Object.hasOwn(instrumentMetaData, instrument) ? instrumentMetaData[instrument]! : null;
}

export function instrumentCode(instrument: string): string | null {
  return instrumentMeta(instrument)?.code ?? null;
}

/** dukascopy-node's "unknown" start for minute candles and ticks. */
export const PLACEHOLDER_START = '2000-01-01T00:00:00.000Z';

function parseSeconds(iso: string | undefined): number | null {
  if (!iso) return null;
  const ms = Date.parse(iso);
  return Number.isNaN(ms) ? null : ms / 1000;
}

/**
 * h1 and d1 start at their metadata start. The minute start is trusted only when it is not the
 * placeholder, is whole minutes (28 US stocks carry a bogus 2015 start with seconds in it) and is
 * no more than a day before the hourly start; otherwise minute data starts with the hourly data, or
 * with the first tick (floored to its minute) when that is known and later.
 */
export function tierStartFromMeta(meta: InstrumentMeta, tier: Tier): number | null {
  const h1 = parseSeconds(meta.startMonthForHourlyCandles);
  if (tier === 'h1') return h1;
  if (tier === 'd1') return parseSeconds(meta.startYearForDailyCandles);
  const raw = meta.startDayForMinuteCandles;
  const m1Ms = raw && raw !== PLACEHOLDER_START ? Date.parse(raw) : NaN;
  if (!Number.isNaN(m1Ms) && m1Ms % 60_000 === 0 && (h1 === null || m1Ms / 1000 >= h1 - 86400)) return m1Ms / 1000;
  const tickRaw = meta.startHourForTicks;
  const tick = tickRaw && tickRaw !== PLACEHOLDER_START ? parseSeconds(tickRaw) : null;
  const tickMinute = tick === null ? null : Math.floor(tick / 60) * 60;
  if (h1 === null) return tickMinute;
  return tickMinute === null ? h1 : Math.max(h1, tickMinute);
}

export function tierStart(instrument: string, tier: Tier): number | null {
  const meta = instrumentMeta(instrument);
  return meta ? tierStartFromMeta(meta, tier) : null;
}

/** Every instrument key in dukascopy-node's metadata (tests walk all of them). */
export function allInstruments(): string[] {
  return Object.keys(instrumentMetaData);
}
