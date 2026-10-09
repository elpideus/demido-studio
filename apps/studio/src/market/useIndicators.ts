// Runs a chart window's saved indicators: as TradingView studies on the live stream when signed in,
// computed from the stored bars when not (the classics market/indicators.ts knows), and otherwise
// listed with a sign-in note. Every stream gets them again, in their saved order, so TradingView's
// indicators-per-chart limit refuses the last ones rather than random ones. Their look (the
// settings' Style tab) is applied here, over TradingView's description, without computing them again.

import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';

import { api, errorText } from '@/lib/api';
import type { MarketEvent } from '@/lib/events';
import type { IndicatorLook, IndicatorMeta } from '@/lib/types';
import { toast } from '@/stores/toasts';
import { localMeta } from './indicators';
import { applyLook } from './look';
import type { IndicatorView, PriceChartHandle } from './PriceChart';
import { isPlanLimit, runKey, type SavedIndicator } from './savedIndicators';

interface Running {
  /** The run key of the setup it runs with. */
  run: string;
  /** The stream ('tv:<id>') or 'local' it runs on. */
  context: string;
  status: IndicatorView['status'];
  message?: string;
  meta?: IndicatorMeta;
  local: boolean;
  /** TradingView's study, once added. */
  tvId?: string;
  /** The look it is drawn with (JSON), to tell when it changed. */
  look?: string;
  /** Which attempt this is: a newer one makes an answer to an older one moot. */
  attempt: number;
}

type IndicatorEvent = Extract<MarketEvent, { event: 'indicator.data' | 'indicator.graphics' | 'indicator.error' }>;

/** Events for a study that arrived before its `indicator.add` answer did. */
const EARLY_LIMIT = 50;

export interface ChartIndicators {
  views: IndicatorView[];
  /** The description of a running indicator (its inputs, for the settings), without its look. */
  meta: (key: string) => IndicatorMeta | undefined;
  /** Indicator events from the market sidecar. */
  onEvent: (e: MarketEvent) => boolean;
  /** Tries the indicators the plan's limit refused once more (after one was removed). */
  retry: (key?: string) => void;
  /** Whether TradingView computes any of them on the live stream. */
  onTradingView: () => boolean;
  /** Keys added by the user just now: a refusal of these is also told in a toast. */
  markAdded: (keys: string[]) => void;
}

/**
 * `context` is where indicators run: `tv:<stream id>` while a live stream is open, `local` while
 * stored history is shown, null while neither (loading, or nothing to show).
 */
export function useIndicators(
  saved: SavedIndicator[],
  context: string | null,
  chart: RefObject<PriceChartHandle | null>,
): ChartIndicators {
  const running = useRef(new Map<string, Running>());
  const byStudy = useRef(new Map<string, string>());
  const early = useRef(new Map<string, IndicatorEvent[]>());
  const queue = useRef<Promise<void>>(Promise.resolve());
  const attempts = useRef(0);
  const contextRef = useRef(context);
  contextRef.current = context;
  const added = useRef(new Set<string>());
  const retryLimited = useRef<Set<string> | 'all' | null>(null);
  // The latest looks: an answer from TradingView is drawn in the look of the moment it arrives.
  const looks = useRef(new Map<string, IndicatorLook | undefined>());
  looks.current = new Map(saved.map((s) => [s.key, s.look]));
  const [, setVersion] = useState(0);
  const bump = useCallback(() => setVersion((n) => n + 1), []);

  /** Queued: TradingView's limit counts studies in the order they are added and removed. */
  const enqueue = useCallback((task: () => Promise<void>) => {
    queue.current = queue.current.then(task).catch(() => undefined);
  }, []);

  const freeStudy = useCallback(
    (r: Running) => {
      const id = r.tvId;
      if (!id) return;
      r.tvId = undefined;
      byStudy.current.delete(id);
      // Only a study of the open stream still exists: a closed stream took its studies along.
      if (r.context === contextRef.current) enqueue(() => api.marketIndicatorRemove(id).catch(() => undefined));
    },
    [enqueue],
  );

  const apply = useCallback(
    (key: string, e: IndicatorEvent) => {
      const c = chart.current;
      if (e.event === 'indicator.data') c?.indicatorData(key, e.params.rows, e.params.full);
      else if (e.event === 'indicator.graphics') c?.indicatorGraphics(key, e.params.graphics);
      else {
        const r = running.current.get(key);
        if (!r) return;
        byStudy.current.delete(e.params.id);
        r.tvId = undefined;
        r.status = 'error';
        r.message = e.params.message;
        c?.clearIndicator(key);
        bump();
      }
    },
    [bump, chart],
  );

  /** Draws an indicator in its current look. */
  const show = useCallback(
    (key: string, r: Running, meta: IndicatorMeta, local: boolean) => {
      const look = looks.current.get(key);
      r.look = JSON.stringify(look ?? null);
      chart.current?.showIndicator(key, applyLook(meta, look), local);
    },
    [chart],
  );

  const start = useCallback(
    (s: SavedIndicator, ctx: string) => {
      const prev = running.current.get(s.key);
      if (prev) freeStudy(prev);
      const attempt = ++attempts.current;
      const run = runKey(s);
      const told = added.current.delete(s.key);
      if (ctx === 'local') {
        const meta = localMeta(s.script, s.setup);
        const entry: Running = {
          run,
          context: ctx,
          status: meta ? 'ready' : 'signin',
          meta: meta ?? undefined,
          local: !!meta,
          attempt,
        };
        running.current.set(s.key, entry);
        if (meta) show(s.key, entry, meta, true);
        else chart.current?.clearIndicator(s.key);
        return;
      }
      const stream = ctx.slice(3);
      const entry: Running = { run, context: ctx, status: 'loading', meta: prev?.meta, local: false, attempt };
      running.current.set(s.key, entry);
      enqueue(async () => {
        if (running.current.get(s.key)?.attempt !== attempt || contextRef.current !== ctx) return;
        try {
          const res = await api.marketIndicatorAdd(stream, s.script, s.version, s.setup);
          const now = running.current.get(s.key);
          if (now?.attempt !== attempt || contextRef.current !== ctx) {
            // Settings changed or the chart moved on while TradingView was adding it.
            if (contextRef.current === ctx) await api.marketIndicatorRemove(res.id).catch(() => undefined);
            return;
          }
          now.tvId = res.id;
          now.meta = res.meta;
          now.status = 'ready';
          now.message = undefined;
          byStudy.current.set(res.id, s.key);
          show(s.key, now, res.meta, false);
          for (const e of early.current.get(res.id) ?? []) apply(s.key, e);
          early.current.delete(res.id);
        } catch (err) {
          const now = running.current.get(s.key);
          if (now?.attempt !== attempt) return;
          now.status = 'error';
          now.message = errorText(err);
          chart.current?.clearIndicator(s.key);
          // One toast for the limit, however many of the indicators just added it refused.
          if (told && isPlanLimit(now.message)) toast.warning('No room for more indicators', now.message);
          else if (told) toast.error(`Could not add ${s.name}`, now.message);
        } finally {
          bump();
        }
      });
    },
    [apply, bump, chart, enqueue, freeStudy, show],
  );

  const stop = useCallback(
    (key: string) => {
      const r = running.current.get(key);
      if (!r) return;
      freeStudy(r);
      running.current.delete(key);
      chart.current?.clearIndicator(key);
    },
    [chart, freeStudy],
  );

  // A new stream (or none) leaves the previous stream's studies and early events behind.
  useEffect(() => {
    early.current.clear();
    if (context !== null) return;
    for (const r of running.current.values()) {
      if (r.context.startsWith('tv:')) {
        r.status = 'loading';
        r.tvId = undefined;
      }
    }
    byStudy.current.clear();
    bump();
  }, [context, bump]);

  // Indicators follow the saved list: new ones start, removed ones stop, changed ones start over.
  const runs = saved.map((s) => `${s.key}=${runKey(s)}`).join('\n');
  const [retryTick, setRetryTick] = useState(0);
  useEffect(() => {
    const keep = new Set(saved.map((s) => s.key));
    let freed = false;
    for (const [key, r] of [...running.current]) {
      if (keep.has(key)) continue;
      if (r.tvId) freed = true;
      stop(key);
    }
    if (context === null) {
      bump();
      return;
    }
    const retry = retryLimited.current;
    retryLimited.current = null;
    for (const s of saved) {
      const r = running.current.get(s.key);
      const limited = r?.status === 'error' && isPlanLimit(r.message);
      const again = limited && (freed || retry === 'all' || (retry instanceof Set && retry.has(s.key)));
      const errorRetry = r?.status === 'error' && retry instanceof Set && retry.has(s.key);
      if (r && r.context === context && r.run === runKey(s) && !again && !errorRetry) continue;
      start(s, context);
    }
    bump();
    // `runs` changes exactly when the saved keys or setups do.
  }, [runs, context, retryTick]);

  // A new look redraws what is shown, values kept.
  const lookKeys = saved.map((s) => `${s.key}=${JSON.stringify(s.look ?? null)}`).join('\n');
  useEffect(() => {
    for (const s of saved) {
      const r = running.current.get(s.key);
      if (!r?.meta || r.status !== 'ready') continue;
      const look = JSON.stringify(s.look ?? null);
      if (r.look === look) continue;
      r.look = look;
      chart.current?.restyleIndicator(s.key, applyLook(r.meta, s.look));
    }
    // `lookKeys` changes exactly when a look does.
  }, [lookKeys]);

  // Derived on every render: `running` is a ref, and `bump` re-renders whenever it changes.
  const views = saved.map((s): IndicatorView => {
    const r = running.current.get(s.key);
    return {
      key: s.key,
      name: s.name,
      status: r?.status ?? 'loading',
      message: r?.message,
      hidden: !!s.hidden,
      local: !!r?.local,
      legendInputs: s.look?.legendInputs !== false,
      legendValues: s.look?.legendValues !== false,
    };
  });

  const onEvent = useCallback(
    (e: MarketEvent): boolean => {
      if (e.event !== 'indicator.data' && e.event !== 'indicator.graphics' && e.event !== 'indicator.error') return false;
      const key = byStudy.current.get(e.params.id);
      if (key) apply(key, e);
      else {
        const list = early.current.get(e.params.id) ?? [];
        if (list.length < EARLY_LIMIT) list.push(e);
        early.current.set(e.params.id, list);
      }
      return true;
    },
    [apply],
  );

  return {
    views,
    meta: (key) => running.current.get(key)?.meta,
    onEvent,
    retry: (key) => {
      retryLimited.current = key ? new Set([key]) : 'all';
      setRetryTick((n) => n + 1);
    },
    onTradingView: () => [...running.current.values()].some((r) => !!r.tvId),
    markAdded: (keys) => {
      for (const k of keys) added.current.add(k);
    },
  };
}
