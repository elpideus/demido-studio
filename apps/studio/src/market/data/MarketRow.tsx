// One market in the Data tab: its sources and size, expanding to a coverage timeline per tier and
// the actions that fill or clear it.

import { Fragment, useMemo, useState } from 'react';
import { ChartCandlestick, ChevronRight, Download, History, Trash2 } from 'lucide-react';
import { Button, Dialog, Notice, cx, formatBytes } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { MarketCoverageItem, MarketJob, MarketPlan, MarketTierCoverage } from '@/lib/types';
import { toast } from '@/stores/toasts';
import { DownloadProgress, SOURCE_NAMES, tierLabel } from '../DownloadProgress';
import {
  chartSymbol,
  formatDay,
  leadJob,
  marketBytes,
  marketDomain,
  monthRangeText,
  sortSources,
  sortTiers,
  storedExtent,
} from './coverage';
import { PlanPanel } from './PlanPanel';
import { TierTimeline, TimelineAxis } from './TierTimeline';
import palette from './sources.module.css';
import styles from './DataTab.module.css';
import timeline from './Timeline.module.css';

// The sidecar refuses TradingView downloads up front when signed out; say what to do about it.
function startErrorText(e: unknown): string {
  const text = errorText(e);
  return /NOT_LOGGED_IN/i.test(text) ? 'Sign in to TradingView first (from a chart), then try again.' : text;
}

export interface MarketRowProps {
  item: MarketCoverageItem;
  /** Unfinished downloads for this market. */
  jobs: MarketJob[];
  expanded: boolean;
  onToggle: () => void;
  onOpenChart: (symbol: string) => void;
  /** Something changed that the summary should show. */
  onChanged: () => void;
}

export function MarketRow({ item, jobs, expanded, onToggle, onOpenChart, onChanged }: MarketRowProps) {
  const [plan, setPlan] = useState<MarketPlan | null>(null);
  const [planning, setPlanning] = useState(false);
  const [starting, setStarting] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [rechecking, setRechecking] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const domain = useMemo(() => marketDomain(item), [item]);
  const extent = useMemo(() => storedExtent(item), [item]);
  const sources = useMemo(
    () => sortSources(item.sources).map((s) => ({ ...s, tiers: sortTiers(s.source, s.tiers) })),
    [item],
  );
  // A market can hold several TradingView symbols (FX:EURUSD, OANDA:EURUSD); name each source once.
  const sourceNames = [...new Set(sources.map((s) => s.source))];
  const bytes = marketBytes(item);
  const lead = leadJob(jobs);
  const moving = jobs.some((j) => j.status === 'running' || j.status === 'queued' || j.status === 'waiting');
  const symbol = chartSymbol(item);
  const learned = sources
    .filter((s) => s.source === 'dukascopy')
    .flatMap((s) => s.tiers)
    .filter((t): t is MarketTierCoverage & { learnedStart: number } => typeof t.learnedStart === 'number');
  const meta = [
    item.symbols.join(', ') || item.market,
    extent ? monthRangeText(extent[0], extent[1]) : 'Nothing stored yet',
  ];

  const planMissing = async () => {
    setPlanning(true);
    setActionError(null);
    try {
      setPlan(await api.marketPlanDownload(symbol));
    } catch (e) {
      setActionError(`Could not plan the download: ${errorText(e)}`);
    } finally {
      setPlanning(false);
    }
  };

  const start = async () => {
    setStarting(true);
    setActionError(null);
    try {
      // The same request as the plan: everything, every tier. Its progress shows under Downloads.
      const res = await api.marketStartDownload(symbol, {}, 'data');
      // Starting resumes a paused job that covers the range; one stopped by an error needs asking.
      if (plan?.job?.status === 'error' && res.jobId === plan.job.id) await api.marketResumeDownload(res.jobId);
      setPlan(null);
      onChanged();
    } catch (e) {
      setActionError(startErrorText(e));
    } finally {
      setStarting(false);
    }
  };

  const recheck = async () => {
    setRechecking(true);
    try {
      await api.marketCacheRecheck(item.market);
      onChanged();
      // Forgetting the start fetches nothing by itself; show what looking further back would cost.
      void planMissing();
    } catch (e) {
      toast.error('Could not reset the start', errorText(e));
    } finally {
      setRechecking(false);
    }
  };

  const closeConfirm = () => {
    if (deleting) return;
    setConfirming(false);
    setDeleteError(null);
  };

  const remove = async () => {
    setDeleting(true);
    setDeleteError(null);
    try {
      const res = await api.marketCacheDelete(item.market);
      setConfirming(false);
      toast.success(`${item.name} data deleted`, `${formatBytes(res.bytes)} freed.`);
      onChanged();
    } catch (e) {
      // Refused while a download for the market runs; the dialog stays open to say so.
      setDeleteError(errorText(e));
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className={styles.market}>
      <div className={styles.marketHead}>
        <button type="button" className={styles.marketToggle} aria-expanded={expanded} onClick={onToggle}>
          <ChevronRight size={15} className={cx(styles.chevron, expanded && styles.chevronOpen)} aria-hidden />
          <span className={styles.marketMain}>
            <span className={styles.marketTitle}>
              <span className={styles.marketName}>{item.name}</span>
              {sourceNames.map((source) => (
                <span key={source} className={styles.sourceBadge} data-source={source}>
                  <span className={palette.dot} aria-hidden />
                  {SOURCE_NAMES[source]}
                </span>
              ))}
            </span>
            <span className={styles.marketMeta}>{meta.join(' · ')}</span>
          </span>
        </button>
        {lead && (
          <div className={styles.marketJob}>
            <DownloadProgress jobId={lead.id} initialJob={lead} compact />
          </div>
        )}
        <span className={styles.marketSize}>{formatBytes(bytes)}</span>
      </div>

      {expanded && (
        <div className={styles.marketBody}>
          {domain ? (
            <div className={timeline.timelines}>
              {sources.map((s) => (
                <Fragment key={`${s.source}:${s.key}`}>
                  <div className={timeline.sourceHead} data-source={s.source}>
                    <span className={palette.dot} aria-hidden />
                    {SOURCE_NAMES[s.source]}
                    <span className={timeline.sourceBytes}>
                      {s.key} · {formatBytes(s.bytes)}
                    </span>
                  </div>
                  {s.tiers.map((t) => (
                    <TierTimeline key={t.tier} source={s.source} tier={t} domain={domain} />
                  ))}
                </Fragment>
              ))}
              <TimelineAxis domain={domain} />
            </div>
          ) : (
            <p className={styles.muted}>Nothing is stored for this market yet.</p>
          )}

          {learned.length > 0 && (
            <div className={styles.learnedNote}>
              <span className={styles.learnedMark} aria-hidden />
              <span className={styles.learnedText}>
                Dukascopy seemed to have no{' '}
                {learned.map((t) => `${tierLabel(t.tier)} data before ${formatDay(t.learnedStart)}`).join(' and no ')},
                so older files are skipped.
              </span>
              <Button size="sm" variant="ghost" icon={History} loading={rechecking} onClick={() => void recheck()}>
                Check for older data
              </Button>
            </div>
          )}

          {plan && (
            <PlanPanel
              plan={plan}
              starting={starting}
              error={actionError}
              onStart={() => void start()}
              onClose={() => {
                setPlan(null);
                setActionError(null);
              }}
            />
          )}
          {!plan && actionError && <p className={styles.actionError}>{actionError}</p>}

          <div className={styles.actions}>
            <Button
              size="sm"
              variant="secondary"
              icon={Download}
              loading={planning}
              disabled={plan !== null}
              onClick={() => void planMissing()}
            >
              Download missing
            </Button>
            <Button size="sm" variant="secondary" icon={ChartCandlestick} onClick={() => onOpenChart(symbol)}>
              Open chart
            </Button>
            <span className={styles.flex} />
            <Button size="sm" variant="ghost" icon={Trash2} onClick={() => setConfirming(true)}>
              Delete
            </Button>
          </div>
        </div>
      )}

      <Dialog
        open={confirming}
        onClose={closeConfirm}
        dismissible={!deleting}
        title={`Delete ${item.name} data?`}
        description={`This frees ${formatBytes(bytes)} of disk space.`}
        footer={
          <>
            <Button variant="ghost" disabled={deleting} onClick={closeConfirm}>
              Cancel
            </Button>
            <Button variant="danger" icon={Trash2} loading={deleting} onClick={() => void remove()}>
              Delete
            </Button>
          </>
        }
      >
        <div className={styles.dialogBody}>
          <p>
            Everything stored for {item.name} from{' '}
            {sourceNames.map((s) => SOURCE_NAMES[s]).join(' and ') || 'any source'} is removed. Charts and the assistant
            download it again when they need it.
          </p>
          {moving && !deleteError && (
            <Notice tone="warning">A download for this market is running. Pause it under Downloads first.</Notice>
          )}
          {deleteError && (
            <Notice tone="danger" title="Nothing was deleted">
              {deleteError}
            </Notice>
          )}
        </div>
      </Dialog>
    </div>
  );
}
