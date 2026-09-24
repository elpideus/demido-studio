import { useEffect, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';

import { useEscape } from '../hooks';
import { cx } from '../utils';
import { IconButton } from './IconButton';
import styles from './Dialog.module.css';

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title?: ReactNode;
  description?: ReactNode;
  icon?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  width?: number;
  /** When false the dialog can only be closed through its own buttons. */
  dismissible?: boolean;
  className?: string;
}

/** A modal dialog centered over a dimmed backdrop. */
export function Dialog({
  open,
  onClose,
  title,
  description,
  icon,
  children,
  footer,
  width = 460,
  dismissible = true,
  className,
}: DialogProps) {
  const panel = useRef<HTMLDivElement>(null);
  useEscape(() => dismissible && onClose(), open);

  useEffect(() => {
    if (!open) return undefined;
    const previous = document.activeElement as HTMLElement | null;
    const first = panel.current?.querySelector<HTMLElement>(
      'input, textarea, select, button:not([data-dialog-close]), [tabindex]:not([tabindex="-1"])',
    );
    (first ?? panel.current)?.focus();
    return () => previous?.focus?.();
  }, [open]);

  if (!open) return null;
  return createPortal(
    <div
      className={styles.backdrop}
      onPointerDown={(e) => {
        if (dismissible && e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        className={cx(styles.panel, className)}
        style={{ width }}
      >
        {(title || dismissible) && (
          <div className={styles.header}>
            {icon && <div className={styles.icon}>{icon}</div>}
            <div className={styles.headings}>
              {title && <h2 className={styles.title}>{title}</h2>}
              {description && <p className={styles.description}>{description}</p>}
            </div>
            {dismissible && (
              <IconButton
                icon={X}
                label="Close"
                size="sm"
                tooltip={false}
                onClick={onClose}
                data-dialog-close
              />
            )}
          </div>
        )}
        {children && <div className={styles.body}>{children}</div>}
        {footer && <div className={styles.footer}>{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}
