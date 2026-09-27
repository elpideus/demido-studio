// A market_data_status result: what is stored per market, source and tier, what is still missing,
// and the downloads filling it. Drawn with the Data tab's timelines, so both read the same.

import { Fragment, useMemo, useState } from 'react';
import { ChevronRight } from 'lucide-react';
import { cx, formatBytes } from '@demido/ui';

import type { MarketCoverageItem, ToolDisplay } from '@/lib/types';
import { DownloadProgress, SOURCE_NAMES, tierLabel } from '@/market/DownloadProgress';
import {
  isUnfinished,
  leadJob,
  marketBytes,
  marketDomain,
  monthRangeText,
  sortMarkets,
  sortSources,
  sortTiers,
  storedExtent,
} from '@/market/data/coverage';
import { TierTimeline, TimelineAxis } from '@/market/data/TierTimeline';
import palette from '@/market/data/sources.module.css';
import timeline from '@/market/data/Timeline.module.css';
import { dataStatusView, gapsText, marketGaps } from './dataStatusView';
import styles from './DataStatus.module.css';

function StatusMarket({ item, open, onToggle }: { item: MarketCoverageItem; open: boolean; onToggle: () => void }) {
  const domain = useMemo(() => marketDomain(item), [item]);
  const extent = useMemo(() => storedExtent(item), [item]);
  const sources = useMemo(
    () => sortSources(item.sources).map((s) => ({ ...s, tiers: sortTiers(s.source, s.tiers) })),
    [item],
  );
  const gaps = useMemo(() => marketGaps({ ...item, sources }), [item, sources]);
  const sourceNames = [...new Set(sources.map((s) => s.source))];
  const jobs = item.jobs.filter((j) => isUnfinished(j.status));
  const lead = leadJob(jobs);
  const both = sourceNames.length > 1;

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
            <div className={styles.muted}>Nothing is stored for this market yet.</div>
          )}
          {domain && (
            <div className={styles.gaps}>
              {gaps.length === 0 ? (
                <span className={styles.muted}>No gaps: everything the sources offer is stored.</span>
              ) : (
                gaps.map((g) => (
                  <div key={`${g.source}:${g.tier}`} className={styles.gapRow}>
                    <span className={styles.gapTier}>
                      {both ? `${SOURCE_NAMES[g.source]} ` : ''}
                      {tierLabel(g.tier)}
                    </span>
                    <span className={styles.gapText}>{gapsText(g.gaps)}</span>
                  </div>
                ))
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
