// What the chat draws for a market_data_status result (display kind 'dataStatus').

import { describe, expect, it } from 'vitest';

import type { MarketCoverageItem } from '@/lib/types';
import { rangeText } from '@/market/data/coverage';
import { dataStatusView, gapsText, marketGaps } from './dataStatusView';

const utc = (y: number, m = 1, d = 1) => Date.UTC(y, m - 1, d) / 1000;
const now = utc(2026, 9, 27);

// The shape tools/market.rs `data_status` sends: the store's cache.summary items, untouched.
const eurusd: MarketCoverageItem = {
  market: 'eurusd',
  name: 'EUR/USD',
  symbols: ['FX:EURUSD'],
  sources: [
    {
      source: 'dukascopy',
      key: 'eurusd',
      bytes: 9_000_000,
      tiers: [
        {
          tier: 'm1',
          intervals: [
            [utc(2003, 5, 4), utc(2010)],
            [utc(2012), now],
          ],
          empty: [[utc(2010), utc(2010, 2)]],
          available: [utc(2003, 5, 4), now],
          learnedStart: null,
          bytes: 8_000_000,
        },
        { tier: 'h1', intervals: [[utc(2003, 5), now]], available: [utc(2003, 5), now], bytes: 1_000_000 },
      ],
    },
    {
      source: 'tradingview',
      key: 'FX:EURUSD',
      bytes: 50_000,
      tiers: [{ tier: '1h', intervals: [[utc(2026, 9), now]], available: [utc(2026, 9), now], bytes: 50_000 }],
    },
  ],
  jobs: [],
};

describe('dataStatusView', () => {
  it('reads the summary the tool sends', () => {
    const view = dataStatusView({ kind: 'dataStatus', symbol: 'EURUSD', bytes: 9_050_000, items: [eurusd] });
    expect(view.symbol).toBe('EURUSD');
    expect(view.bytes).toBe(9_050_000);
    expect(view.items).toHaveLength(1);
    expect(view.items[0]!.sources.map((s) => s.tiers.map((t) => t.tier))).toEqual([['m1', 'h1'], ['1h']]);
  });

  it('says nothing is stored rather than failing on an empty or missing list', () => {
    expect(dataStatusView({ kind: 'dataStatus', symbol: null, bytes: 0, items: [] })).toEqual({
      symbol: null,
      bytes: 0,
      items: [],
    });
    expect(dataStatusView({ kind: 'dataStatus' }).items).toEqual([]);
  });

  it('leaves out what an older backend sent in another shape', () => {
    const view = dataStatusView({
      kind: 'dataStatus',
      items: [
        { market: 'x' },
        null,
        {
          market: 'gbpchf',
          sources: [
            { source: 'dukascopy', key: 'gbpchf', bytes: 10, tiers: [{ tier: 'm1', intervals: [[1, 2], 'bad'] }, {}] },
            { source: 'yahoo', key: 'GBPCHF', tiers: [] },
          ],
          jobs: [{ id: 'j1', total: 3, perTier: [] }, { id: 'old' }],
        },
      ],
    });
    expect(view.items.map((i) => i.market)).toEqual(['gbpchf']);
    const item = view.items[0]!;
    // Missing names fall back to the market id; bytes add up when the total is missing.
    expect(item.name).toBe('gbpchf');
    expect(view.bytes).toBe(10);
    expect(item.sources).toHaveLength(1);
    expect(item.sources[0]!.tiers).toEqual([
      { tier: 'm1', intervals: [[1, 2]], empty: [], available: null, learnedStart: null, bytes: 0 },
    ]);
    expect(item.jobs.map((j) => j.id)).toEqual(['j1']);
  });
});

describe('gaps', () => {
  it('lists each tier with time left to download, per source', () => {
    const gaps = marketGaps(eurusd);
    expect(gaps).toEqual([{ source: 'dukascopy', tier: 'm1', gaps: [[utc(2010, 2), utc(2012)]] }]);
  });

  it('names a few ranges and counts the rest', () => {
    const ranges: Array<[number, number]> = [
      [utc(2004), utc(2005)],
      [utc(2006), utc(2007)],
      [utc(2008), utc(2009)],
      [utc(2010), utc(2011)],
      [utc(2012), utc(2013)],
    ];
    expect(gapsText(ranges.slice(0, 1))).toBe(`1 gap: ${rangeText(utc(2004), utc(2005))}`);
    expect(gapsText(ranges)).toBe(
      `5 gaps: ${rangeText(utc(2004), utc(2005))}, ${rangeText(utc(2006), utc(2007))}, ${rangeText(utc(2008), utc(2009))} and 2 more`,
    );
  });
});
