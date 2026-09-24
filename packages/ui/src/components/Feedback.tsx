import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';

import { cx, hueFor, initials } from '../utils';
import styles from './Feedback.module.css';

/* ----------------------------------------------------------- ProgressBar */

export interface ProgressBarProps {
  /** 0 to 1; omit for an indeterminate bar. */
  value?: number | null;
  tone?: 'accent' | 'warning' | 'danger' | 'neutral';
  size?: 'xs' | 'sm' | 'md';
  className?: string;
  label?: string;
}

export function ProgressBar({ value, tone = 'accent', size = 'sm', className, label }: ProgressBarProps) {
  const indeterminate = value === undefined || value === null;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(value * 100)}
      className={cx(styles.track, styles[`track-${size}`], className)}
    >
      <div
        className={cx(styles.fill, styles[`fill-${tone}`], indeterminate && styles.indeterminate)}
        style={indeterminate ? undefined : { width: `${Math.max(0, Math.min(1, value)) * 100}%` }}
      />
    </div>
  );
}

/* ----------------------------------------------------------------- Badge */

export interface BadgeProps {
  tone?: 'neutral' | 'accent' | 'warning' | 'danger' | 'info';
  icon?: LucideIcon;
  children: ReactNode;
  className?: string;
  title?: string;
}

export function Badge({ tone = 'neutral', icon: Icon, children, className, title }: BadgeProps) {
  return (
    <span className={cx(styles.badge, styles[`badge-${tone}`], className)} title={title}>
      {Icon && <Icon size={11} strokeWidth={2.2} aria-hidden />}
      {children}
    </span>
  );
}

/* ------------------------------------------------------------------- Kbd */

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className={styles.kbd}>{children}</kbd>;
}

/* ---------------------------------------------------------------- Avatar */

export interface AvatarProps {
  name: string;
  src?: string | null;
  size?: number;
  shape?: 'circle' | 'rounded';
  className?: string;
}

/** A picture, or the name's initials on a color derived from the name. */
export function Avatar({ name, src, size = 28, shape = 'rounded', className }: AvatarProps) {
  const hue = hueFor(name);
  return (
    <span
      className={cx(styles.avatar, styles[`avatar-${shape}`], className)}
      style={{
        width: size,
        height: size,
        fontSize: Math.max(9, Math.round(size * 0.38)),
        background: src ? undefined : `hsl(${hue} 32% 26%)`,
        color: src ? undefined : `hsl(${hue} 60% 78%)`,
      }}
      aria-hidden
    >
      {src ? <img src={src} alt="" draggable={false} /> : initials(name)}
    </span>
  );
}

/* ------------------------------------------------------------ EmptyState */

export interface EmptyStateProps {
  icon?: LucideIcon;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
  className?: string;
  compact?: boolean;
}

export function EmptyState({ icon: Icon, title, description, action, className, compact }: EmptyStateProps) {
  return (
    <div className={cx(styles.empty, compact && styles.emptyCompact, className)}>
      {Icon && (
        <span className={styles.emptyIcon}>
          <Icon size={compact ? 20 : 24} strokeWidth={1.6} aria-hidden />
        </span>
      )}
      <p className={styles.emptyTitle}>{title}</p>
      {description && <p className={styles.emptyDescription}>{description}</p>}
      {action && <div className={styles.emptyAction}>{action}</div>}
    </div>
  );
}

/* ------------------------------------------------------------------ Field */

export interface FieldProps {
  label: ReactNode;
  description?: ReactNode;
  /** Control rendered on the right (inline) or below (stacked). */
  children: ReactNode;
  layout?: 'inline' | 'stacked';
  htmlFor?: string;
  className?: string;
}

/** A labelled settings row. */
export function Field({ label, description, children, layout = 'stacked', htmlFor, className }: FieldProps) {
  return (
    <div className={cx(styles.fieldRow, styles[`field-${layout}`], className)}>
      <div className={styles.fieldText}>
        <label className={styles.fieldLabel} htmlFor={htmlFor}>
          {label}
        </label>
        {description && <div className={styles.fieldDescription}>{description}</div>}
      </div>
      <div className={styles.fieldControl}>{children}</div>
    </div>
  );
}

/* ---------------------------------------------------------------- Notice */

export interface NoticeProps {
  tone?: 'info' | 'warning' | 'danger' | 'accent';
  icon?: LucideIcon;
  title?: ReactNode;
  children?: ReactNode;
  action?: ReactNode;
  className?: string;
}

/** An inline message box. */
export function Notice({ tone = 'info', icon: Icon, title, children, action, className }: NoticeProps) {
  return (
    <div className={cx(styles.notice, styles[`notice-${tone}`], className)}>
      {Icon && <Icon size={16} strokeWidth={1.9} className={styles.noticeIcon} aria-hidden />}
      <div className={styles.noticeBody}>
        {title && <div className={styles.noticeTitle}>{title}</div>}
        {children && <div className={styles.noticeText}>{children}</div>}
      </div>
      {action && <div className={styles.noticeAction}>{action}</div>}
    </div>
  );
}
