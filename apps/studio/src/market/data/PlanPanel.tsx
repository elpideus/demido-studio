// "Download missing", step two: what the download would fetch, before anything is fetched.

import { CircleCheck, Download, Play } from 'lucide-react';
import { Button } from '@demido/ui';

import type { MarketPlan } from '@/lib/types';
import { SOURCE_NAMES, planLine, tierLabel } from '../DownloadProgress';
import { planIdle, rangeText, type PlanIdle } from './coverage';
import styles from './DataTab.module.css';

export interface PlanPanelProps {
  plan: MarketPlan;
  starting: boolean;
  error: string | null;
  onStart: () => void;
  onClose: () => void;
}

function idleText(idle: PlanIdle, plan: MarketPlan): string {
  switch (idle) {
    case 'complete':
      return `Everything ${SOURCE_NAMES[plan.source]} has for ${plan.name} is downloaded.`;
    case 'stopped':
      return `A stopped download already covers the rest of ${plan.name}.`;
    case 'busy':
      return `A download under Downloads is already fetching the rest of ${plan.name}.`;
    case 'nothing':
      return `There is nothing more to download for ${plan.name} right now.`;
  }
}

export function PlanPanel({ plan, starting, error, onStart, onClose }: PlanPanelProps) {
  const unit = plan.source === 'dukascopy' ? 'files' : 'pages';
  const idle = planIdle(plan);
  // Starting a range that a stopped job already covers resumes that job.
  const startable = idle === null || idle === 'stopped';
  const tiers = plan.perTier.filter((t) => t.requests > 0);
  const notes: string[] = [];
  if (!idle && plan.queuedAhead > 0) {
    notes.push(
      `Another download already fetches ${plan.queuedAhead.toLocaleString()} more ${unit}; they aren't counted.`,
    );
  }
  if (!idle && plan.job) notes.push('A download for this range exists already; starting continues it.');
  if (!idle && plan.approximate) notes.push("A rough estimate: TradingView doesn't say how far back its history goes.");
  if (startable && plan.source === 'tradingview') notes.push('Needs a TradingView sign-in.');

  return (
    <div className={styles.plan} role="group" aria-label="Download plan">
      {idle ? (
        <div className={styles.planDone}>
          <CircleCheck size={16} strokeWidth={1.9} aria-hidden />
          {idleText(idle, plan)}
        </div>
      ) : (
        <>
          <div className={styles.planLine}>{planLine(plan)}</div>
          {tiers.length > 0 && (
            <div className={styles.planTiers}>
              {tiers.map((t) => (
                <div key={t.tier} className={styles.planTier}>
                  <span className={styles.planTierName}>{tierLabel(t.tier)}</span>
                  <span className={styles.planTierRange}>{rangeText(t.from, t.to)}</span>
                  <span className={styles.planTierCount}>
                    {t.requests.toLocaleString()} {unit}
                  </span>
                </div>
              ))}
            </div>
          )}
        </>
      )}
      {notes.map((note) => (
        <p key={note} className={styles.planNote}>
          {note}
        </p>
      ))}
      {error && <p className={styles.planError}>{error}</p>}
      <div className={styles.planActions}>
        <Button size="sm" variant="ghost" onClick={onClose} disabled={starting}>
          {startable ? 'Cancel' : 'Close'}
        </Button>
        {startable && (
          <Button size="sm" variant="primary" icon={idle ? Play : Download} loading={starting} onClick={onStart}>
            {plan.job?.status === 'error'
              ? 'Try again'
              : idle || plan.job?.status === 'paused'
                ? 'Resume download'
                : 'Start download'}
          </Button>
        )}
      </div>
    </div>
  );
}
