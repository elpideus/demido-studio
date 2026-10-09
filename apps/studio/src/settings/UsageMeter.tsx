import { useEffect } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ExternalLink } from 'lucide-react';
import { ProgressBar } from '@demido/ui';

import type { Allowance } from '@/lib/types';
import { useProviderUsage } from '@/stores/providerUsage';
import { ALLOWANCE_LABELS, amount, leftInShort, resets, share, tightest, tone } from './usage';
import styles from './UsageMeter.module.css';

const AI_STUDIO_LIMITS = 'https://aistudio.google.com/rate-limit';

/** Keeps the providers' usage fresh while a view shows it: now, then every minute. */
export function useUsageRefresh() {
  const refresh = useProviderUsage((st) => st.refresh);
  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 60_000);
    return () => window.clearInterval(timer);
  }, [refresh]);
}

function AllowanceMeter({ allowance: a }: { allowance: Allowance }) {
  const label = ALLOWANCE_LABELS[a.kind];
  const reset = resets(a);
  return (
    <div className={styles.meter}>
      <div className={styles.meterLine}>
        <span className={styles.meterLabel}>{label}</span>
        <span className={styles.meterValue}>
          {`${amount(a, a.remaining)} of ${amount(a, a.limit)} left`}
          {reset && <span className={styles.reset}>{` · ${reset}`}</span>}
        </span>
      </div>
      <ProgressBar value={share(a)} tone={tone(a)} size="sm" label={label} />
    </div>
  );
}

/** What a provider card says of its key's usage: a meter for each allowance the provider reports. */
export function ProviderUsageMeters({ providerId }: { providerId: string }) {
  const usage = useProviderUsage((st) => st.usage[providerId]);
  if (!usage) return null;
  if (usage.status === 'failed') {
    return <div className={styles.usage}>{`Could not check the usage: ${usage.message}`}</div>;
  }
  if (usage.status === 'unreported') {
    return (
      <div className={styles.usage}>
        <p>
          Google does not tell apps how much of the quota is left.{' '}
          <a
            href={AI_STUDIO_LIMITS}
            onClick={(e) => {
              e.preventDefault();
              void openUrl(AI_STUDIO_LIMITS);
            }}
          >
            See it in AI Studio <ExternalLink size={11} />
          </a>
        </p>
      </div>
    );
  }
  if (usage.allowances.length === 0) return null;
  return (
    <div className={styles.usage}>
      {usage.allowances.map((a) => (
        <AllowanceMeter key={a.kind} allowance={a} />
      ))}
    </div>
  );
}

/** A short meter for a group of models: whichever of its provider's allowances is closest to running out. */
export function UsageChip({ providerId }: { providerId: string }) {
  const usage = useProviderUsage((st) => st.usage[providerId]);
  const a = usage?.status === 'reported' ? tightest(usage.allowances) : null;
  if (!a) return null;
  const label = ALLOWANCE_LABELS[a.kind];
  const reset = resets(a);
  const detail = `${label}: ${amount(a, a.remaining)} of ${amount(a, a.limit)} left${reset ? `, ${reset}` : ''}`;
  return (
    <span className={styles.chip} title={detail}>
      <ProgressBar value={share(a)} tone={tone(a)} size="xs" label={label} className={styles.chipBar} />
      <span className={styles.chipText}>{leftInShort(a)}</span>
    </span>
  );
}
