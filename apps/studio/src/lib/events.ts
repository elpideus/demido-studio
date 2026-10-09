// Backend events the UI follows. Names match the constants in src-tauri.

import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  Bar,
  ChartDrawing,
  ChatEvent,
  DownloadJob,
  IndicatorGraphics,
  IndicatorRow,
  MailAccount,
  MarketJob,
  MarketStatus,
  MarketStoreUpdate,
  ModelEntry,
  PineSummary,
  RuntimeStatus,
  Skill,
  UpdateStatus,
} from './types';

export type MarketEvent =
  | { event: 'stream.update'; params: { id: string; bar: Bar } }
  | { event: 'stream.error'; params: { id: string; message: string } }
  | { event: 'stream.closed'; params: { id: string; reason: string } }
  | { event: 'download.progress'; params: { job: MarketJob } }
  | { event: 'download.done'; params: { job: MarketJob } }
  | { event: 'download.error'; params: { job: MarketJob } }
  /** A job was cancelled: its record is gone (the data it fetched stays). */
  | { event: 'download.removed'; params: { jobId: string } }
  /** New bars were stored for a key; coalesced to at most one per second per key. */
  | { event: 'store.updated'; params: MarketStoreUpdate }
  /** A TradingView indicator's values: `full` replaces all rows, otherwise they merge by time. */
  | { event: 'indicator.data'; params: { id: string; full: boolean; rows: IndicatorRow[] } }
  /** Everything a TradingView indicator draws besides its plots; replaces what it drew before. */
  | { event: 'indicator.graphics'; params: { id: string; graphics: IndicatorGraphics } }
  /** TradingView stopped computing an indicator (it is gone from the chart). */
  | { event: 'indicator.error'; params: { id: string; code: string; message: string } }
  /** A library script was saved (`script`) or deleted (null). */
  | { event: 'pine.changed'; params: { id: string; script: PineSummary | null } }
  | { event: 'ready'; params: { version: string } };

/** What the assistant puts on the Market window's chart (src-tauri tools/pine.rs). */
export type ChartCommand =
  | {
      action: 'indicator';
      script: string;
      name: string | null;
      inputs: Record<string, unknown>;
      symbol: string | null;
      timeframe: string | null;
    }
  | { action: 'draw'; drawing: ChartDrawing; symbol: string | null; timeframe: string | null }
  /** `name` "*" removes every set. */
  | { action: 'undraw'; name: string };

interface EventMap {
  'chat://event': ChatEvent;
  'models://changed': ModelEntry[];
  'runtime://status': RuntimeStatus;
  'downloads://changed': DownloadJob;
  'skills://changed': Skill[];
  'market://status': MarketStatus;
  'market://event': MarketEvent;
  'market://chart': ChartCommand;
  'updater://status': UpdateStatus;
  'mail://accounts': MailAccount[];
  /** A folder's cached messages changed. */
  'mail://changed': { account: string; folder: string };
  /** An account's folder list changed: a folder or label was made or deleted elsewhere. */
  'mail://folders': { account: string };
}

export function on<K extends keyof EventMap>(name: K, handler: (payload: EventMap[K]) => void): Promise<UnlistenFn> {
  return listen<EventMap[K]>(name, (e) => handler(e.payload));
}
