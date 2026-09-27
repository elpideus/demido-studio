// What the market store holds, kept current. Stored bars and downloads announce themselves through
// `market://event`; while a download runs that is several events a second, so reloads trail the
// first event a little and then come at most every few seconds.

import { useCallback, useEffect, useRef, useState } from 'react';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import type { MarketCacheSummary, MarketJob } from '@/lib/types';
import { unfinishedJobs } from './coverage';

const SETTLE_MS = 600;
const MIN_GAP_MS = 2500;

export interface StoredData {
  /** Null until the first load succeeds. */
  summary: MarketCacheSummary | null;
  /** Unfinished downloads, newest first. */
  jobs: MarketJob[];
  /** The last load's error; the previous summary stays shown. */
  error: string | null;
  /** Reloads now (after the load in flight, if any). */
  reload: () => void;
}

export function useStoredData(): StoredData {
  const [summary, setSummary] = useState<MarketCacheSummary | null>(null);
  const [jobs, setJobs] = useState<MarketJob[]>([]);
  const [error, setError] = useState<string | null>(null);
  const reloadNow = useRef<() => void>(() => {});

  useEffect(() => {
    let disposed = false;
    let running = false;
    // What to do once the load in flight ends: an explicit reload runs at once, an event waits its turn.
    let pending: 'now' | 'later' | null = null;
    let timer: number | undefined;
    let lastAt = 0;
    // Status of each job shown, so a progress event that only moves the bar doesn't reload.
    const known = new Map<string, MarketJob['status']>();

    const run = async () => {
      if (running) {
        pending = 'now';
        return;
      }
      running = true;
      lastAt = Date.now();
      try {
        // The job list also has jobs too new to have stored anything; without it the summary's lists do.
        const [next, listed] = await Promise.all([
          api.marketCacheSummary(),
          api.marketListDownloads().catch(() => null),
        ]);
        if (disposed) return;
        const unfinished = unfinishedJobs(next.items, listed);
        known.clear();
        for (const job of unfinished) known.set(job.id, job.status);
        setSummary(next);
        setJobs(unfinished);
        setError(null);
      } catch (e) {
        if (!disposed) setError(errorText(e));
      } finally {
        running = false;
        if (!disposed) {
          const then = pending;
          pending = null;
          if (then === 'now') void run();
          else if (then === 'later') schedule();
        }
      }
    };

    const schedule = () => {
      if (running) {
        pending ??= 'later';
        return;
      }
      if (timer !== undefined) return;
      const wait = Math.max(SETTLE_MS, lastAt + MIN_GAP_MS - Date.now());
      timer = window.setTimeout(() => {
        timer = undefined;
        void run();
      }, wait);
    };

    reloadNow.current = () => {
      window.clearTimeout(timer);
      timer = undefined;
      void run();
    };

    void run();
    const unlisten = on('market://event', (e) => {
      if (
        e.event === 'store.updated' ||
        e.event === 'download.done' ||
        e.event === 'download.error' ||
        e.event === 'download.removed'
      ) {
        schedule();
      } else if (e.event === 'download.progress') {
        // A download started elsewhere (a chart, the chat) first shows up as progress of an unknown
        // job; a paused or resumed one changes what the market rows and the delete check go by.
        const job = e.params.job as MarketJob | undefined;
        if (!job?.id || known.get(job.id) !== job.status) schedule();
      }
    });
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      void unlisten.then((f) => f());
    };
  }, []);

  const reload = useCallback(() => reloadNow.current(), []);
  return { summary, jobs, error, reload };
}
