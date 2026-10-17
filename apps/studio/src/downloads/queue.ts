// What the downloads button and panel say about the queue. The list comes from the backend in
// queue order: the front of the queue starts first.

import type { DownloadJob } from '@/lib/types';

/** Holds a download slot: transferring, or checking what arrived. */
export function isRunning(job: DownloadJob): boolean {
  return job.state === 'downloading' || job.state === 'verifying';
}

/** Waits its turn and can be reordered: queued, or paused where it stands. */
export function isWaiting(job: DownloadJob): boolean {
  return job.state === 'queued' || job.state === 'paused';
}

export interface QueueSummary {
  running: DownloadJob[];
  waiting: DownloadJob[];
  queued: number;
  paused: number;
  failed: DownloadJob[];
  done: DownloadJob[];
  /** Bytes here and in all, over everything not finished. */
  downloaded: number;
  total: number;
  bytesPerSecond: number;
  /** Seconds until everything running or queued has arrived, at the current speed. */
  eta: number | null;
}

export function summarize(jobs: DownloadJob[]): QueueSummary {
  const live = jobs.filter((j) => j.state !== 'done');
  const running = jobs.filter(isRunning);
  const waiting = jobs.filter(isWaiting);
  const bytesPerSecond = running.reduce((sum, j) => sum + j.bytesPerSecond, 0);
  const remaining = jobs
    .filter((j) => isRunning(j) || j.state === 'queued')
    .reduce((sum, j) => sum + Math.max(0, j.total - j.downloaded), 0);
  return {
    running,
    waiting,
    queued: waiting.filter((j) => j.state === 'queued').length,
    paused: waiting.filter((j) => j.state === 'paused').length,
    failed: jobs.filter((j) => j.state === 'failed'),
    done: jobs.filter((j) => j.state === 'done'),
    downloaded: live.reduce((sum, j) => sum + j.downloaded, 0),
    total: live.reduce((sum, j) => sum + j.total, 0),
    bytesPerSecond,
    eta: bytesPerSecond > 0 && remaining > 0 ? remaining / bytesPerSecond : null,
  };
}

/** Where a queued job stands among the queued ones (1 is next), or null when it is not queued. */
export function queuePosition(jobs: DownloadJob[], id: string): number | null {
  const queued = jobs.filter((j) => j.state === 'queued');
  const i = queued.findIndex((j) => j.id === id);
  return i < 0 ? null : i + 1;
}

/**
 * The `before` the backend's reorder takes, to move `id` to `index` among the waiting jobs (as the
 * panel shows them). Null moves it to the end.
 */
export function beforeFor(waiting: DownloadJob[], id: string, index: number): string | null {
  const others = waiting.filter((j) => j.id !== id);
  return others[Math.max(0, index)]?.id ?? null;
}

/** "3 min left", "40 s left": how long, rounded the way people say it. */
export function formatEta(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds)) return '';
  if (seconds < 60) return `${Math.max(1, Math.ceil(seconds))} s left`;
  if (seconds < 3600) return `${Math.ceil(seconds / 60)} min left`;
  const h = Math.floor(seconds / 3600);
  const m = Math.round((seconds - h * 3600) / 60);
  return m ? `${h} h ${m} min left` : `${h} h left`;
}

/** The repository's name without its author: `unsloth/Qwen3-8B-GGUF` → `Qwen3-8B`. */
export function shortName(repo: string): string {
  return (repo.split('/').pop() ?? repo).replace(/[-_.]?gguf$/i, '');
}
