// Backend events the UI follows. Names match the constants in src-tauri.

import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type {
  Bar,
  ChatEvent,
  DownloadJob,
  MarketStatus,
  ModelEntry,
  RuntimeStatus,
  Skill,
} from './types';

export type MarketEvent =
  | { event: 'stream.update'; params: { id: string; bar: Bar } }
  | { event: 'stream.error'; params: { id: string; message: string } }
  | { event: 'stream.closed'; params: { id: string; reason: string } }
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

export function on<K extends keyof EventMap>(
  name: K,
  handler: (payload: EventMap[K]) => void,
): Promise<UnlistenFn> {
  return listen<EventMap[K]>(name, (e) => handler(e.payload));
}
