// The text of the chat's download approval and progress card (helpers from market/DownloadProgress).

import { describe, expect, it } from 'vitest';
import { formatBytes } from '@demido/ui';

import type { MarketJob, MarketPlan } from '@/lib/types';
import { estimateText, jobLine, planDetail, planLine, planSummary, tierLabel } from '@/market/DownloadProgress';
import { downloadView } from './downloadView';

const n = (v: number) => v.toLocaleString();

const plan = (over: Partial<MarketPlan> = {}): MarketPlan => ({
  source: 'dukascopy',
  key: 'eurusd',
  name: 'EUR/USD',
  from: 1_051_747_200,
  to: 1_790_380_800,
  tiers: ['m1', 'h1', 'd1'],
  requests: 8400,
  bytes: 75_000_000,
  seconds: 40 * 60,
  queuedAhead: 0,
  complete: false,
  approximate: false,
  perTier: [],
  job: null,
  ...over,
});

const job = (over: Partial<MarketJob> = {}): MarketJob => ({
  id: 'j1',
  source: 'dukascopy',
  key: 'eurusd',
  symbol: 'FX:EURUSD',
  name: 'EUR/USD',
  from: 1_051_747_200,
  to: 1_790_380_800,
  tiers: ['m1', 'h1', 'd1'],
  status: 'running',
  total: 8400,
  done: 1234,
  bytes: 45_000_000,
  etaSeconds: 12 * 60,
  rate: 3.5,
  inFlight: 4,
  perTier: [],
  origin: 'chat',
  createdAt: 0,
  updatedAt: 0,
  ...over,
});

describe('estimateText', () => {
  it('rounds to what a person would say', () => {
    expect(estimateText(30)).toBe('less than a minute');
    expect(estimateText(12 * 60)).toBe('about 12 min');
    expect(estimateText(89 * 60)).toBe('about 89 min');
    expect(estimateText(2 * 3600 + 10 * 60)).toBe('about 2 h 10 min');
    expect(estimateText(117 * 60)).toBe('about 2 h');
    expect(estimateText(3 * 86_400)).toBe('about 3 days');
    expect(estimateText(Number.NaN)).toBe('less than a minute');
  });
});

describe('plan text', () => {
  it('names the source, the size and the detail', () => {
    expect(planLine(plan())).toBe(
      `EUR/USD from Dukascopy · ~${n(8400)} files · ~${formatBytes(75_000_000)} · about 40 min · 1-minute candles for every timeframe`,
    );
  });

  it('says every Dukascopy download is 1-minute candles, whatever tiers an older plan named', () => {
    for (const tiers of [['m1'], ['m1', 'h1', 'd1'], ['d1']]) {
      expect(planDetail(plan({ tiers }))).toBe('1-minute candles for every timeframe');
    }
  });

  it('counts TradingView pages', () => {
    const tiers = ['1m', '5m', '15m', '1h', '4h', '1d', '1w'];
    const tv = plan({ source: 'tradingview', tiers, requests: 40, bytes: 0, seconds: 120, approximate: true });
    expect(planSummary(tv)).toBe('~40 pages · about 2 min');
    expect(planDetail(tv)).toBe('every timeframe TradingView allows');
  });

  it('says when there is nothing to fetch', () => {
    expect(planSummary(plan({ complete: true, requests: 0 }))).toBe('Already downloaded');
  });
});

describe('jobLine', () => {
  it('shows files, size, time left and rate while running', () => {
    expect(jobLine(job())).toBe(
      `${n(1234)} / ${n(8400)} files · ${formatBytes(45_000_000)} · about 12 min left · 3.5 files/s`,
    );
  });

  it('drops the estimate and rate once paused or done', () => {
    expect(jobLine(job({ status: 'paused' }))).toBe(`${n(1234)} / ${n(8400)} files · ${formatBytes(45_000_000)}`);
    expect(jobLine(job({ status: 'done', done: 8400, bytes: 75_000_000 }))).toBe(
      `${n(8400)} files · ${formatBytes(75_000_000)}`,
    );
  });

  it('reports files skipped for lack of data apart from the ones fetched', () => {
    expect(jobLine(job({ status: 'done', total: 5, done: 5, bytes: 25_000, skipped: 120 }))).toBe(
      `5 files · ${formatBytes(25_000)} · 120 skipped (no data at source)`,
    );
    expect(jobLine(job({ status: 'paused', skipped: 0 }))).toBe(
      `${n(1234)} / ${n(8400)} files · ${formatBytes(45_000_000)}`,
    );
  });

  it('keeps the waiting estimate but not a rate', () => {
    expect(jobLine(job({ status: 'waiting' }))).toBe(
      `${n(1234)} / ${n(8400)} files · ${formatBytes(45_000_000)} · about 12 min left`,
    );
  });
});

describe('downloadView', () => {
  it('follows a job, offering Continue only when the turn ended before it finished', () => {
    const running = downloadView({ kind: 'download', jobId: 'j1', plan: plan(), status: 'running' });
    expect(running).toEqual({ kind: 'progress', jobId: 'j1', plan: plan(), continuable: true });
    // Still inside the tool call: no status yet, the running turn hides Continue anyway.
    expect(downloadView({ kind: 'download', jobId: 'j1', plan: plan() })).toMatchObject({ continuable: true });
    expect(downloadView({ kind: 'download', jobId: 'j1', plan: plan(), status: 'done' })).toMatchObject({
      kind: 'progress',
      continuable: false,
    });
  });

  it('says why nothing was started', () => {
    const denied = { kind: 'download', jobId: null, plan: plan(), status: 'denied', note: 'You chose not to.' };
    expect(downloadView(denied)).toEqual({ kind: 'note', text: 'You chose not to.' });
    expect(downloadView({ kind: 'download', jobId: null, denied: true })).toEqual({
      kind: 'note',
      text: 'You chose not to download this.',
    });
    expect(downloadView({ kind: 'download', jobId: null, plan: plan(), status: 'done' })).toEqual({
      kind: 'note',
      text: 'EUR/USD is already downloaded.',
    });
    expect(downloadView({ kind: 'download', jobId: null, plan: plan({ complete: true, requests: 0 }) })).toEqual({
      kind: 'note',
      text: 'EUR/USD is already downloaded.',
    });
  });

  it('shows only the error when the download could not start', () => {
    expect(downloadView({ kind: 'download', jobId: null, plan: plan(), jobError: 'Offline' })).toEqual({
      kind: 'none',
    });
  });

  it('ignores a plan it cannot read', () => {
    expect(downloadView({ kind: 'download', jobId: null, plan: { old: true } })).toEqual({
      kind: 'progress',
      jobId: null,
      plan: null,
      continuable: false,
    });
  });
});

describe('tierLabel', () => {
  it('names Dukascopy tiers and leaves TradingView timeframes alone', () => {
    expect(tierLabel('m1')).toBe('1-minute');
    expect(tierLabel('h1')).toBe('hourly');
    expect(tierLabel('d1')).toBe('daily');
    expect(tierLabel('4h')).toBe('4h');
  });
});
