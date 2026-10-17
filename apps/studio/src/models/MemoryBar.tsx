import { Cpu, MemoryStick } from 'lucide-react';
import { Tooltip, cx, formatBytes } from '@demido/ui';

import type { MemoryBudget } from '@/lib/types';
import { type Fit, reserveBytes } from './fit';
import styles from './ModelsWindow.module.css';

/**
 * Where a download would sit in the GPU's memory (or the computer's, without a GPU): the model,
 * what is kept free for the conversation, search and speech, and what is left over.
 */
export function MemoryBar({ bytes, fit, budget }: { bytes: number; fit: Fit; budget: MemoryBudget }) {
  const total = budget.memory;
  const reserve = reserveBytes(budget);
  const share = (b: number) => `${Math.max(0, Math.min(100, (b / total) * 100))}%`;
  const kept = Math.max(0, Math.min(reserve, total - bytes));
  const spare = Math.max(0, total - bytes - reserve);
  const over = bytes > total;
  const parts = budget.reserve.map((r) => `${r.label}: ${formatBytes(r.bytes)}`).join('\n');

  return (
    <div className={styles.memory}>
      <div className={styles.memoryBar} role="img" aria-label={`${formatBytes(bytes)} of ${formatBytes(total)}`}>
        <span className={cx(styles.memoryModel, styles[`fill-${fit.tone}`])} style={{ width: share(bytes) }} />
        <span className={styles.memoryReserve} style={{ width: share(kept) }} />
      </div>
      <div className={styles.legend}>
        <span>
          <i className={cx(styles.swatch, styles[`fill-${fit.tone}`])} />
          Model {formatBytes(bytes)}
          {over && `, ${formatBytes(bytes - total)} in system memory`}
        </span>
        <Tooltip content={<span className={styles.tipLines}>{parts}</span>} delay={150}>
          <span>
            <i className={cx(styles.swatch, styles.swatchReserve)} />
            Kept free {formatBytes(reserve)}
          </span>
        </Tooltip>
        {spare > 0 && (
          <span>
            <i className={cx(styles.swatch, styles.swatchSpare)} />
            Spare {formatBytes(spare)}
          </span>
        )}
        <span className={styles.grow} />
        <span className={styles.device}>
          {budget.gpu ? <Cpu size={12} aria-hidden /> : <MemoryStick size={12} aria-hidden />}
          {budget.gpu ? `${budget.gpu}, ${formatBytes(total)}` : `${formatBytes(total)} of memory for models`}
        </span>
      </div>
    </div>
  );
}
