import { Check, ChevronRight, CircleSlash, Files, X, type LucideIcon } from 'lucide-react';
import { Spinner, cx, formatDuration } from '@demido/ui';

import { bundleKey, callKey, thinkingKey, useOpenMap, useOpenState } from './openState';
import { summarizeFiles, thoughtSeconds, type FilesSummary, type Step } from './steps';
import { ThinkingBlock } from './ThinkingBlock';
import { ToolCard } from './ToolCard';
import cardStyles from './ToolCard.module.css';
import styles from './FileBundle.module.css';

const STATUS_ICONS: Partial<
  Record<FilesSummary['status'], { icon: LucideIcon; className: string | undefined; label: string }>
> = {
  done: { icon: Check, className: cardStyles.statusOk, label: 'Done' },
  failed: { icon: X, className: cardStyles.statusFailed, label: 'Failed' },
  stopped: { icon: CircleSlash, className: cardStyles.statusMuted, label: 'Stopped' },
};

/** A run of workspace file calls as one card that opens to every call and the reasoning between them. */
export function FileBundle({ id, steps }: { id: string; steps: Step[] }) {
  const opened = useOpenMap();
  // Starts open when the person had opened something that has since moved into the bundle.
  const openInside = steps.some(
    ({ message, calls }) => opened.get(thinkingKey(message.id)) || calls.some((c) => opened.get(callKey(c.call.id))),
  );
  const [open, setOpen] = useOpenState(bundleKey(id), openInside);
  const summary = summarizeFiles(steps.flatMap((s) => s.calls));
  const running = summary.status === 'running';
  const status = STATUS_ICONS[summary.status];

  return (
    <div className={cardStyles.card}>
      <button type="button" className={cardStyles.header} onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className={cx(styles.icon, summary.failure && styles.iconFailed)}>
          <Files size={15} strokeWidth={1.8} aria-hidden />
        </span>
        <span className={styles.summary}>
          <span className={styles.label}>{summary.label}</span>
          {summary.count && <span className={styles.count}>{summary.count}</span>}
          {summary.failure && <span className={styles.failure}>· {summary.failure}</span>}
        </span>
        {running && <Spinner size={13} />}
        {status && (
          <status.icon
            size={14}
            strokeWidth={2.2}
            className={cx(cardStyles.status, status.className)}
            aria-label={status.label}
          />
        )}
        {summary.durationMs !== undefined && !running && (
          <span className={cardStyles.duration}>{formatDuration(summary.durationMs / 1000)}</span>
        )}
        <ChevronRight size={14} className={cx(cardStyles.chevron, open && cardStyles.chevronOpen)} aria-hidden />
      </button>
      {open && (
        <div className={styles.body}>
          {steps.map(({ message, calls, lead }) => (
            <div key={message.id} className={styles.step}>
              {message.reasoning && !lead && (
                <div className={styles.thinking}>
                  <ThinkingBlock
                    messageId={message.id}
                    text={message.reasoning}
                    live={false}
                    seconds={thoughtSeconds(message)}
                  />
                </div>
              )}
              {!lead && message.content.trim() && (
                <p className={cx(styles.narration, 'selectable')}>{message.content.trim()}</p>
              )}
              {calls.map((c) => (
                <ToolCard key={c.call.id} call={c.call} result={c.result} orphaned={c.orphaned} flat />
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
