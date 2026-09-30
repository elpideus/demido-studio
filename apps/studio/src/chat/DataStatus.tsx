// A market_data_status result: what is stored per market and where it came from, what is still
// missing, and the downloads filling it. Drawn with the Data tab's timeline, so both read the same:
// one per market, since every timeframe is built from the same 1-minute history.

import { useMemo, useState } from 'react';
import { ChevronRight } from 'lucide-react';
import { cx, formatBytes } from '@demido/ui';

import type { MarketCoverageItem, ToolDisplay } from '@/lib/types';
import { DownloadProgress, SOURCE_NAMES } from '@/market/DownloadProgress';
import {
  isUnfinished,
  leadJob,
  marketBytes,
  marketDomain,
  monthRangeText,
  sortMarkets,
  sortSources,
  storedExtent,
} from '@/market/data/coverage';
import { MarketTimeline, TimelineAxis } from '@/market/data/MarketTimeline';
import palette from '@/market/data/sources.module.css';
import timeline from '@/market/data/Timeline.module.css';
import { dataStatusView, gapsText, marketGaps } from './dataStatusView';
import styles from './DataStatus.module.css';

function StatusMarket({ item, open, onToggle }: { item: MarketCoverageItem; open: boolean; onToggle: () => void }) {
  const domain = useMemo(() => marketDomain(item), [item]);
  const extent = useMemo(() => storedExtent(item), [item]);
  const sources = useMemo(() => sortSources(item.sources), [item]);
  const gaps = useMemo(() => marketGaps(item), [item]);
  const sourceNames = [...new Set(sources.map((s) => s.source))];
  const jobs = item.jobs.filter((j) => isUnfinished(j.status));
  const lead = leadJob(jobs);

  return (
    <div className={styles.market}>
      <div className={styles.head}>
        <button type="button" className={styles.toggle} aria-expanded={open} onClick={onToggle}>
          <ChevronRight size={14} className={cx(styles.chevron, open && styles.chevronOpen)} aria-hidden />
          <span className={styles.name}>{item.name}</span>
          {sourceNames.map((source) => (
            <span key={source} className={styles.badge} data-source={source}>
              <span className={palette.dot} aria-hidden />
              {SOURCE_NAMES[source]}
            </span>
          ))}
          <span className={styles.meta}>{extent ? monthRangeText(extent[0], extent[1]) : 'Nothing stored yet'}</span>
        </button>
        {!open && lead && <DownloadProgress key={lead.id} jobId={lead.id} initialJob={lead} compact />}
        <span className={styles.size}>{formatBytes(marketBytes(item))}</span>
      </div>
      {open && (
        <div className={styles.body}>
          {domain ? (
            <div className={timeline.timelines}>
              <div className={timeline.sourceHead}>
                {sources.map((s) => (
                  <span key={`${s.source}:${s.key}`} className={timeline.sourceItem} data-source={s.source}>
                    <span className={palette.dot} aria-hidden />
                    {SOURCE_NAMES[s.source]}
                    <span className={timeline.sourceBytes}>
                      {s.key} · {formatBytes(s.bytes)}
                    </span>
                  </span>
                ))}
              </div>
              <MarketTimeline item={item} domain={domain} />
              <TimelineAxis domain={domain} />
            </div>
          ) : (
            <div className={styles.muted}>Nothing is stored for this market yet.</div>
          )}
          {domain && (
            <div className={styles.gaps}>
              {gaps.length === 0 ? (
                <span className={styles.muted}>No gaps: everything the sources offer is stored.</span>
              ) : (
                <div className={styles.gapRow}>
                  <span className={styles.gapText}>{gapsText(gaps)}</span>
                </div>
              )}
            </div>
          )}
          {jobs.map((job) => (
            <div key={job.id} className={styles.job}>
              <DownloadProgress jobId={job.id} initialJob={job} inline />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** The stored-data view; one market opens by itself, a longer list starts folded. */
export function DataStatus({ d }: { d: ToolDisplay }) {
  const view = useMemo(() => dataStatusView(d), [d]);
  const items = useMemo(() => sortMarkets(view.items), [view]);
  const [open, setOpen] = useState<ReadonlySet<string>>(
    () => new Set(items.length === 1 ? items.map((i) => i.market) : []),
  );
  if (!items.length) {
    return (
      <div className={styles.muted}>
        {view.symbol ? `Nothing is stored for ${view.symbol} yet.` : 'No market history is stored yet.'}
      </div>
    );
  }
  const toggle = (market: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (!next.delete(market)) next.add(market);
      return next;
    });
  return (
    <div className={cx(palette.palette, styles.status)}>
      {items.length > 1 && (
        <div className={styles.summary}>
          {items.length} markets · {formatBytes(view.bytes)} stored
        </div>
      )}
      <div className={styles.markets}>
        {items.map((item) => (
          <StatusMarket
            key={item.market}
            item={item}
            open={open.has(item.market)}
            onToggle={() => toggle(item.market)}
          />
        ))}
      </div>
    </div>
  );
}
