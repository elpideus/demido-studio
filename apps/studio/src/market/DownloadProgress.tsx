// Progress of one market download. The chat card, the chart's popup and toolbar and the Data tab
// all draw this; the job itself lives in the sidecar, so this follows it by id and survives the
// card being scrolled away or the app restarting.

import { useEffect, useState } from 'react';
import { ArrowRight, ChevronRight, Pause, Play, RotateCcw } from 'lucide-react';
import { Button, ProgressBar, cx, formatBytes } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import type { MarketJob, MarketJobStatus, MarketPlan, MarketSource } from '@/lib/types';
import styles from './DownloadProgress.module.css';

export const SOURCE_NAMES: Record<MarketSource, string> = { dukascopy: 'Dukascopy', tradingview: 'TradingView' };

const TIER_LABELS: Record<string, string> = { m1: '1-minute', h1: 'hourly', d1: 'daily' };

/** `m1` → `1-minute`; TradingView timeframes stay as they are. */
export function tierLabel(tier: string): string {
  return TIER_LABELS[tier] ?? tier;
}

/** A spoken-style estimate: "less than a minute", "about 40 min", "about 2 h 10 min", "about 3 days". */
export function estimateText(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 60) return 'less than a minute';
  if (seconds < 90 * 60) return `about ${Math.round(seconds / 60)} min`;
  if (seconds < 36 * 3600) {
    const tens = Math.round(seconds / 600) * 10;
    const h = Math.floor(tens / 60);
    const m = tens % 60;
    return m ? `about ${h} h ${m} min` : `about ${h} h`;
  }
  return `about ${Math.round(seconds / 86_400)} days`;
}

// A Dukascopy request fetches one file; a TradingView one, one page of bars.
const unitOf = (source: MarketSource, n: number) => (source === 'dukascopy' ? 'file' : 'page') + (n === 1 ? '' : 's');

/** "~8,400 files · ~75 MB · about 40 min". */
export function planSummary(plan: MarketPlan): string {
  if (plan.complete || plan.requests === 0) return 'Already downloaded';
  const parts = [`~${plan.requests.toLocaleString()} ${unitOf(plan.source, plan.requests)}`];
  if (plan.bytes > 0) parts.push(`~${formatBytes(plan.bytes)}`);
  parts.push(estimateText(plan.seconds));
  return parts.join(' · ');
}

/** What the data will serve: "every timeframe at 1-minute detail". */
export function planDetail(plan: MarketPlan): string {
  if (plan.source === 'tradingview') {
    return plan.tiers.length === 1 ? `${plan.tiers[0]} only` : 'every timeframe TradingView allows';
  }
  if (plan.tiers.includes('m1')) return 'every timeframe at 1-minute detail';
  if (plan.tiers.includes('h1')) return 'hourly detail';
  return 'daily detail';
}

/** "EUR/USD from Dukascopy · ~8,400 files · ~75 MB · about 40 min · every timeframe at 1-minute detail". */
export function planLine(plan: MarketPlan): string {
  return `${plan.name} from ${SOURCE_NAMES[plan.source]} · ${planSummary(plan)} · ${planDetail(plan)}`;
}

const active = (status: MarketJobStatus) => status === 'running' || status === 'waiting' || status === 'queued';

/** "1,234 / 8,400 files"; just "8,400 files" once done. */
function countText(job: MarketJob): string {
  const unit = unitOf(job.source, job.total);
  return job.status === 'done'
    ? `${job.total.toLocaleString()} ${unit}`
    : `${job.done.toLocaleString()} / ${job.total.toLocaleString()} ${unit}`;
}

/** "1,234 / 8,400 files · 45 MB · about 12 min left · 3.5 files/s". */
export function jobLine(job: MarketJob): string {
  const parts = [countText(job)];
  if (job.bytes > 0) parts.push(formatBytes(job.bytes));
  // Left out of the count rather than counted as fetched: the source has nothing that far back.
  if (job.skipped) parts.push(`${job.skipped.toLocaleString()} skipped (no data at source)`);
  if (active(job.status) && job.etaSeconds !== null && job.done < job.total) {
    parts.push(`${estimateText(job.etaSeconds)} left`);
  }
  if (job.status === 'running' && job.rate > 0) parts.push(`${job.rate.toFixed(1)} ${unitOf(job.source, 0)}/s`);
  return parts.join(' · ');
}

const STATUS_LABELS: Record<MarketJobStatus, string> = {
  queued: 'Queued',
  running: 'Downloading',
  waiting: 'Waiting',
  paused: 'Paused',
  done: 'Downloaded',
  error: 'Stopped',
};

const TONES: Record<MarketJobStatus, 'accent' | 'warning' | 'danger' | 'neutral'> = {
  queued: 'neutral',
  running: 'accent',
  waiting: 'warning',
  paused: 'neutral',
  done: 'accent',
  error: 'danger',
};

function fraction(job: MarketJob): number | null {
  if (job.status === 'done') return 1;
  return job.total > 0 ? Math.min(1, job.done / job.total) : null;
}

const day = (seconds: number) =>
  new Date(seconds * 1000).toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric' });

// Progress events and the status reply can cross; the newer copy of a job wins.
function newer(current: MarketJob | null | undefined, next: MarketJob): MarketJob {
  return current && current.id === next.id && current.updatedAt > next.updatedAt ? current : next;
}

// A job from an older backend has another shape; drawing it would throw inside the chat.
function asJob(value: unknown): MarketJob | null {
  const job = value as Partial<MarketJob> | null | undefined;
  return job && typeof job.id === 'string' && typeof job.total === 'number' && Array.isArray(job.perTier)
    ? (job as MarketJob)
    : null;
}

export interface DownloadProgressProps {
  jobId: string | null;
  /** Drawn until the live status arrives. */
  initialJob?: MarketJob | null;
  /** The range the job was started for; "Download again" restarts it once the job is gone. */
  plan?: MarketPlan | null;
  /** One line with no buttons, for toolbars. */
  compact?: boolean;
  /** One line with its range and Pause / Resume, for a download beside other content (the chart's
   *  picker, the chat's stored-data view). Draws nothing once the job is gone. */
  inline?: boolean;
  /** Offers "Continue" once the download is done. */
  onContinue?: () => void;
}

export function DownloadProgress({ jobId, initialJob, plan, compact, inline, onContinue }: DownloadProgressProps) {
  // "Download again" swaps in the job it starts.
  const [id, setId] = useState(jobId);
  // undefined while the first status is loading; null once the job is gone.
  const [job, setJob] = useState<MarketJob | null | undefined>(() =>
    jobId === null ? null : initialJob?.id === jobId ? (asJob(initialJob) ?? undefined) : undefined,
  );
  // The last copy seen, for "Download again" after the job disappears.
  const [last, setLast] = useState<MarketJob | null>(() => asJob(initialJob));
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [reload, setReload] = useState(0);

  useEffect(() => setId(jobId), [jobId]);

  // A parent that refreshes its own job list (the Data tab) passes newer copies.
  useEffect(() => {
    const next = asJob(initialJob);
    if (next && next.id === id) setJob((prev) => newer(prev, next));
  }, [initialJob, id]);

  useEffect(() => {
    if (job) setLast(job);
  }, [job]);

  useEffect(() => {
    if (id === null) {
      setJob(null);
      return undefined;
    }
    let cancelled = false;
    setLoadError(null);
    setJob((prev) => (prev && prev.id === id ? prev : undefined));
    api.marketGetDownload(id).then(
      (reply) => {
        const res = asJob(reply);
        if (!cancelled) setJob((prev) => (res ? newer(prev, res) : null));
      },
      (e: unknown) => {
        if (!cancelled) setLoadError(errorText(e));
      },
    );
    const unlisten = on('market://event', (e) => {
      if (e.event === 'download.removed') {
        if (e.params.jobId === id) setJob(null);
        return;
      }
      if (e.event !== 'download.progress' && e.event !== 'download.done' && e.event !== 'download.error') return;
      const next = asJob(e.params.job);
      if (next?.id === id) setJob((prev) => newer(prev, next));
    });
    return () => {
      cancelled = true;
      void unlisten.then((f) => f());
    };
  }, [id, reload]);

  const act = async (run: () => Promise<unknown>) => {
    if (id === null) return;
    setBusy(true);
    setActionError(null);
    try {
      await run();
      const res = asJob(await api.marketGetDownload(id));
      setJob((prev) => (res ? newer(prev, res) : null));
    } catch (e) {
      setActionError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const restartSymbol = last?.symbol ?? plan?.key ?? null;
  const downloadAgain = async () => {
    const range = plan ?? last;
    if (!range || !restartSymbol) return;
    setBusy(true);
    setActionError(null);
    try {
      const started = await api.marketStartDownload(
        restartSymbol,
        { from: range.from, to: range.to, tiers: range.tiers },
        last?.origin ?? 'chat',
      );
      setJob(undefined);
      setId(started.jobId);
    } catch (e) {
      setActionError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  // Pause while it moves, Resume once paused, Try again after an error.
  const controls = (j: MarketJob) => (
    <>
      {active(j.status) && (
        <Button
          size="sm"
          variant="ghost"
          icon={Pause}
          disabled={busy}
          onClick={() => void act(() => api.marketPauseDownload(j.id))}
        >
          Pause
        </Button>
      )}
      {j.status === 'paused' && (
        <Button
          size="sm"
          variant="secondary"
          icon={Play}
          disabled={busy}
          onClick={() => void act(() => api.marketResumeDownload(j.id))}
        >
          Resume
        </Button>
      )}
      {j.status === 'error' && (
        <Button
          size="sm"
          variant="secondary"
          icon={RotateCcw}
          disabled={busy}
          onClick={() => void act(() => api.marketResumeDownload(j.id))}
        >
          Try again
        </Button>
      )}
    </>
  );

  if (inline) {
    if (!job) return null;
    return (
      <div className={styles.inline} title={`${job.name} · ${STATUS_LABELS[job.status]} · ${jobLine(job)}`}>
        <div className={styles.inlineMain}>
          <div className={styles.inlineHead}>
            <span className={cx(styles.status, styles[`status-${job.status}`])}>{STATUS_LABELS[job.status]}</span>
            <span className={styles.inlineCount}>{countText(job)}</span>
          </div>
          <div className={styles.inlineRange}>
            {day(job.from)} → {day(job.to)}
          </div>
        </div>
        <div className={styles.inlineControls}>{controls(job)}</div>
        {actionError && <div className={styles.error}>{actionError}</div>}
      </div>
    );
  }

  if (compact) {
    if (!job) return null;
    const value = fraction(job);
    return (
      <div className={styles.compact} title={`${job.name} · ${STATUS_LABELS[job.status]} · ${jobLine(job)}`}>
        <ProgressBar
          value={job.status === 'queued' ? null : value}
          tone={TONES[job.status]}
          size="xs"
          label="Download"
        />
        <span className={styles.compactText}>
          {job.status === 'running' && value !== null ? `${Math.floor(value * 100)}%` : STATUS_LABELS[job.status]}
        </span>
      </div>
    );
  }

  if (job === null) {
    return (
      <div className={styles.progress}>
        <div className={styles.removed}>
          <span className={styles.muted}>This download was cancelled or removed</span>
          {(plan ?? last) && restartSymbol && (
            <Button size="sm" variant="secondary" icon={RotateCcw} loading={busy} onClick={() => void downloadAgain()}>
              Download again
            </Button>
          )}
        </div>
        {actionError && <div className={styles.error}>{actionError}</div>}
      </div>
    );
  }

  if (job === undefined) {
    return (
      <div className={styles.progress}>
        <div className={styles.head}>
          <span className={styles.title}>
            <span className={styles.name}>{plan?.name ?? 'Download'}</span>
            {plan && <span className={styles.source}>{SOURCE_NAMES[plan.source]}</span>}
          </span>
        </div>
        {loadError ? (
          <div className={styles.removed}>
            <span className={styles.error}>Could not read the download: {loadError}</span>
            <Button size="sm" variant="ghost" icon={RotateCcw} onClick={() => setReload((n) => n + 1)}>
              Try again
            </Button>
          </div>
        ) : (
          <>
            <ProgressBar value={null} label="Download" />
            {plan && <div className={styles.line}>{planSummary(plan)}</div>}
          </>
        )}
      </div>
    );
  }

  const value = fraction(job);
  const note =
    job.status === 'waiting'
      ? (job.message ?? `Waiting for ${SOURCE_NAMES[job.source]}; it retries on its own.`)
      : job.status === 'error'
        ? job.message
        : job.status === 'queued'
          ? (job.message ?? 'Starts when the downloads ahead of it finish.')
          : null;

  return (
    <div className={styles.progress}>
      <div className={styles.head}>
        <span className={styles.title} title={`${job.name} · ${SOURCE_NAMES[job.source]}`}>
          <span className={styles.name}>{job.name}</span>
          <span className={styles.source}>{SOURCE_NAMES[job.source]}</span>
        </span>
        <span className={cx(styles.status, styles[`status-${job.status}`])}>{STATUS_LABELS[job.status]}</span>
        <span className={styles.spacer} />
        {controls(job)}
      </div>
      <ProgressBar
        value={job.status === 'queued' && !job.done ? null : value}
        tone={TONES[job.status]}
        label="Download"
      />
      <div className={styles.lineRow}>
        <span className={styles.line}>{jobLine(job)}</span>
        {job.perTier.length > 0 && (
          <button
            type="button"
            className={styles.toggle}
            onClick={() => setExpanded(!expanded)}
            aria-expanded={expanded}
          >
            Details
            <ChevronRight size={13} className={cx(styles.chevron, expanded && styles.chevronOpen)} aria-hidden />
          </button>
        )}
      </div>
      {note && <div className={job.status === 'error' ? styles.error : styles.note}>{note}</div>}
      {actionError && <div className={styles.error}>{actionError}</div>}
      {expanded && (
        <div className={styles.tiers}>
          <div className={styles.range}>
            {day(job.from)} → {day(job.to)}
          </div>
          {job.perTier.map((t) => (
            <div key={t.tier} className={styles.tier}>
              <span className={styles.tierName}>{tierLabel(t.tier)}</span>
              <ProgressBar
                value={t.total > 0 ? t.done / t.total : job.status === 'done' ? 1 : 0}
                tone={TONES[job.status]}
                size="xs"
                className={styles.tierBar}
                label={tierLabel(t.tier)}
              />
              <span className={styles.tierCount}>
                {t.done.toLocaleString()} / {t.total.toLocaleString()}
              </span>
            </div>
          ))}
        </div>
      )}
      {job.status === 'done' && onContinue && (
        <div className={styles.actions}>
          <Button size="sm" variant="primary" iconRight={ArrowRight} onClick={onContinue}>
            Continue
          </Button>
        </div>
      )}
    </div>
  );
}
