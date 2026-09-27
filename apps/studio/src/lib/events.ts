// Backend events the UI follows. Names match the constants in src-tauri.

import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  Bar,
  ChatEvent,
  DownloadJob,
  MarketJob,
  MarketStatus,
  MarketStoreUpdate,
  ModelEntry,
  RuntimeStatus,
  Skill,
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
  | { event: 'ready'; params: { version: string } };

interface EventMap {
  'chat://event': ChatEvent;
  'models://changed': ModelEntry[];
  'runtime://status': RuntimeStatus;
  'downloads://changed': DownloadJob;
  'skills://changed': Skill[];
  'market://status': MarketStatus;
  'market://event': MarketEvent;
}

export function on<K extends keyof EventMap>(name: K, handler: (payload: EventMap[K]) => void): Promise<UnlistenFn> {
  return listen<EventMap[K]>(name, (e) => handler(e.payload));
}
