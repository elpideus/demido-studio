// The market window's Data tab: what the market store holds and where it came from. Downloads
// still running come first, then one row per market with a coverage timeline per tier.

import { useMemo, useState } from 'react';
import { Database, RefreshCw, X } from 'lucide-react';
import { Button, EmptyState, IconButton, Notice, Spinner, cx, formatBytes } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { MarketJob } from '@/lib/types';
import { toast } from '@/stores/toasts';
import { DownloadProgress } from './DownloadProgress';
import { jobsFor, sortMarkets } from './data/coverage';
import { MarketRow } from './data/MarketRow';
import { useStoredData } from './data/useStoredData';
import palette from './data/sources.module.css';
import styles from './data/DataTab.module.css';

export function DataTab({ onOpenChart }: { onOpenChart: (symbol: string) => void }) {
  const { summary, jobs, error, reload } = useStoredData();
  // Null until the first toggle: a lone market starts open, since there is nothing to choose between.
  const [open, setOpen] = useState<ReadonlySet<string> | null>(null);
  const items = useMemo(() => sortMarkets(summary?.items ?? []), [summary]);
  const initial = items.length === 1 && items[0] ? [items[0].market] : [];
  const openSet = open ?? new Set(initial);
  const learned = items.some((i) => i.sources.some((s) => s.tiers.some((t) => typeof t.learnedStart === 'number')));

  const toggle = (market: string) =>
    setOpen((prev) => {
      const next = new Set(prev ?? initial);
      if (next.has(market)) next.delete(market);
      else next.add(market);
      return next;
    });

  return (
    <div className={cx(palette.palette, styles.page)}>
      <header className={styles.header}>
        <div className={styles.headerText}>
          <h2 className={styles.title}>Market data</h2>
          <p className={styles.subtitle}>Downloaded once, then used by every chart and by the assistant.</p>
        </div>
        {summary && items.length > 0 && (
          <div className={styles.total}>
            <span className={styles.totalValue}>{formatBytes(summary.bytes)}</span>
            <span className={styles.totalLabel}>stored</span>
          </div>
        )}
        <IconButton icon={RefreshCw} label="Refresh" size="sm" onClick={reload} />
      </header>

      {items.length > 0 && <Legend learned={learned} />}

      <div className={styles.scroll}>
        {error && (
          <Notice
            tone="danger"
            title="Could not read the stored data"
            action={
              <Button size="sm" variant="secondary" onClick={reload}>
                Try again
              </Button>
            }
          >
            {error}
          </Notice>
        )}

        {!summary && !error && (
          <div className={styles.loading}>
            <Spinner size={18} />
          </div>
        )}

        {jobs.length > 0 && (
          <section className={styles.section}>
            <h3 className={styles.sectionTitle}>Downloads</h3>
            <div className={styles.jobs}>
              {jobs.map((job) => (
                <JobCard key={job.id} job={job} onChanged={reload} />
              ))}
            </div>
          </section>
        )}

        {summary && items.length === 0 && jobs.length === 0 && (
          <EmptyState
            icon={Database}
            title="Nothing stored yet"
            description="History downloaded from a chart or by the assistant shows up here, with what came from Dukascopy and what from TradingView."
          />
        )}

        {items.length > 0 && (
          <section className={styles.section}>
            <h3 className={styles.sectionTitle}>Markets</h3>
            <div className={styles.markets}>
              {items.map((item) => (
                <MarketRow
                  key={item.market}
                  item={item}
                  jobs={jobsFor(item, jobs)}
                  expanded={openSet.has(item.market)}
                  onToggle={() => toggle(item.market)}
                  onOpenChart={onOpenChart}
                  onChanged={reload}
                />
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}

function JobCard({ job, onChanged }: { job: MarketJob; onChanged: () => void }) {
  const [busy, setBusy] = useState(false);
  const cancel = async () => {
    setBusy(true);
    try {
      await api.marketCancelDownload(job.id);
      onChanged();
    } catch (e) {
      toast.error('Could not cancel the download', errorText(e));
      setBusy(false);
    }
  };
  return (
    <div className={styles.job}>
      <div className={styles.jobMain}>
        <DownloadProgress jobId={job.id} initialJob={job} />
      </div>
      <IconButton
        icon={X}
        label="Cancel download (what it fetched is kept)"
        size="sm"
        disabled={busy}
        className={styles.jobCancel}
        onClick={() => void cancel()}
      />
    </div>
  );
}

// What the timelines' colours and patterns mean.
function Legend({ learned }: { learned: boolean }) {
  return (
    <div className={styles.legend} aria-label="Legend">
      <span className={styles.legendItem} data-source="dukascopy">
        <span className={cx(styles.swatch, palette.fillData)} aria-hidden />
        Dukascopy
      </span>
      <span className={styles.legendItem} data-source="tradingview">
        <span className={cx(styles.swatch, palette.fillData)} aria-hidden />
        TradingView
      </span>
      <span className={styles.legendItem}>
        <span className={cx(styles.swatch, palette.fillEmpty)} aria-hidden />
        No data at source
      </span>
      <span className={styles.legendItem}>
        <span className={cx(styles.swatch, palette.fillGap)} aria-hidden />
        Not downloaded
      </span>
      {learned && (
        <span className={styles.legendItem}>
          <span className={styles.swatchLearned} aria-hidden />
          Data seems to start here
        </span>
      )}
      <span className={styles.legendHint}>Hover a bar for dates</span>
    </div>
  );
}
