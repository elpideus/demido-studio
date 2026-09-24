import { AlertTriangle, CheckCircle2, Info, X, XCircle } from 'lucide-react';
import { cx } from '@demido/ui';

import { type ToastTone, useToasts } from '@/stores/toasts';
import styles from './Toaster.module.css';

const ICONS: Record<ToastTone, typeof Info> = {
  info: Info,
  success: CheckCircle2,
  warning: AlertTriangle,
  error: XCircle,
};

export function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);
  return (
    <div className={styles.stack} role="status" aria-live="polite">
      {toasts.map((t) => {
        const Icon = ICONS[t.tone];
        return (
          <div key={t.id} className={cx(styles.toast, styles[t.tone])}>
            <Icon size={17} strokeWidth={1.9} className={styles.icon} aria-hidden />
            <div className={styles.text}>
              <div className={styles.title}>{t.title}</div>
              {t.body && <div className={styles.body}>{t.body}</div>}
              {t.action && (
                <button
                  type="button"
                  className={styles.action}
                  onClick={() => {
                    t.action?.run();
                    dismiss(t.id);
                  }}
                >
                  {t.action.label}
                </button>
              )}
            </div>
            <button type="button" className={styles.close} aria-label="Dismiss" onClick={() => dismiss(t.id)}>
              <X size={14} aria-hidden />
            </button>
          </div>
        );
      })}
    </div>
  );
}
