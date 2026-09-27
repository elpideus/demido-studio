// What a market_data_status result draws. The backend sends `{kind: 'dataStatus', symbol, bytes,
// items}`, `items` being the store's summary as it was when the tool ran (see MarketCoverageItem).

import type { MarketCoverageItem, MarketJob, MarketSource, MarketTierCoverage, ToolDisplay } from '@/lib/types';
import { rangeText, tierGaps, type Range } from '@/market/data/coverage';

export interface DataStatusView {
  /** The market asked about; null when the tool listed everything. */
  symbol: string | null;
  bytes: number;
  items: MarketCoverageItem[];
}

const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : 0);
const isRange = (v: unknown): v is Range =>
  Array.isArray(v) && v.length === 2 && typeof v[0] === 'number' && typeof v[1] === 'number';
const ranges = (v: unknown): Range[] => (Array.isArray(v) ? v.filter(isRange) : []);
const isSource = (v: unknown): v is MarketSource => v === 'dukascopy' || v === 'tradingview';

// A result saved by an older backend has another shape; drawing it must not throw inside the chat, so
// everything is checked and whatever cannot be read is left out.
function tierOf(v: unknown): MarketTierCoverage | null {
  const t = v as Partial<MarketTierCoverage> | null;
  if (!t || typeof t.tier !== 'string') return null;
  return {
    tier: t.tier,
    intervals: ranges(t.intervals),
    empty: ranges(t.empty),
    available: isRange(t.available) ? t.available : null,
    learnedStart: typeof t.learnedStart === 'number' ? t.learnedStart : null,
    bytes: num(t.bytes),
  };
}

function jobOf(v: unknown): MarketJob | null {
  const job = v as Partial<MarketJob> | null;
  return job && typeof job.id === 'string' && typeof job.total === 'number' && Array.isArray(job.perTier)
    ? (job as MarketJob)
    : null;
}

function itemOf(v: unknown): MarketCoverageItem | null {
  const item = v as Partial<MarketCoverageItem> | null;
  if (!item || typeof item.market !== 'string' || !Array.isArray(item.sources)) return null;
  const sources = item.sources.flatMap((s) => {
    if (!s || !isSource(s.source) || typeof s.key !== 'string') return [];
    const tiers = Array.isArray(s.tiers) ? s.tiers.map(tierOf).filter((t): t is MarketTierCoverage => !!t) : [];
    return [{ source: s.source, key: s.key, bytes: num(s.bytes), tiers }];
  });
  return {
    market: item.market,
    name: typeof item.name === 'string' && item.name ? item.name : item.market,
    symbols: Array.isArray(item.symbols) ? item.symbols.filter((s): s is string => typeof s === 'string') : [],
    sources,
    jobs: Array.isArray(item.jobs) ? item.jobs.map(jobOf).filter((j): j is MarketJob => !!j) : [],
  };
}

export function dataStatusView(d: ToolDisplay): DataStatusView {
  const items = Array.isArray(d.items) ? d.items.map(itemOf).filter((i): i is MarketCoverageItem => !!i) : [];
  const stored = items.reduce((sum, i) => sum + i.sources.reduce((s, src) => s + src.bytes, 0), 0);
  return {
    symbol: typeof d.symbol === 'string' && d.symbol ? d.symbol : null,
    bytes: typeof d.bytes === 'number' && Number.isFinite(d.bytes) ? d.bytes : stored,
    items,
  };
}

/** One tier's missing stretches, for the line under a market's timelines. */
export interface TierGaps {
  source: MarketSource;
  tier: string;
  gaps: Range[];
}

/** Every tier of a market that has stretches left to download, in the market's source order. */
export function marketGaps(item: MarketCoverageItem): TierGaps[] {
  return item.sources.flatMap((s) =>
    s.tiers.map((t) => ({ source: s.source, tier: t.tier, gaps: tierGaps(t) })).filter((t) => t.gaps.length > 0),
  );
}

/** "2 gaps: 4 May 2003 – 24 Sep 2025, 1 Feb 2013 – 28 Feb 2013"; past `max` ranges, "and 3 more". */
export function gapsText(gaps: readonly Range[], max = 3): string {
  const shown = gaps.slice(0, max).map(([f, t]) => rangeText(f, t));
  const rest = gaps.length - shown.length;
  const head = `${gaps.length} ${gaps.length === 1 ? 'gap' : 'gaps'}: ${shown.join(', ')}`;
  return rest > 0 ? `${head} and ${rest} more` : head;
}
