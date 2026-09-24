import { forwardRef, type ButtonHTMLAttributes } from 'react';
import type { LucideIcon } from 'lucide-react';

import { cx } from '../utils';
import { Tooltip, type TooltipPlacement } from './Tooltip';
import styles from './IconButton.module.css';

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  icon: LucideIcon;
  /** Accessible name, also shown as a tooltip. */
  label: string;
  size?: 'xs' | 'sm' | 'md' | 'lg';
  variant?: 'ghost' | 'solid' | 'accent';
  active?: boolean;
  tooltip?: boolean;
  tooltipPlacement?: TooltipPlacement;
  strokeWidth?: number;
}

const ICON_SIZE = { xs: 13, sm: 15, md: 17, lg: 20 } as const;

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  {
    icon: Icon,
    label,
    size = 'md',
    variant = 'ghost',
    active = false,
    tooltip = true,
    tooltipPlacement = 'top',
    strokeWidth = 1.8,
    className,
    type = 'button',
    ...rest
  },
  ref,
) {
  const button = (
    <button
      ref={ref}
      type={type}
      aria-label={label}
      aria-pressed={active || undefined}
      className={cx(styles.button, styles[size], styles[variant], active && styles.active, className)}
      {...rest}
    >
      <Icon size={ICON_SIZE[size]} strokeWidth={strokeWidth} aria-hidden />
    </button>
  );
  return tooltip ? (
    <Tooltip content={label} placement={tooltipPlacement}>
      {button}
    </Tooltip>
  ) : (
    button
  );
});
