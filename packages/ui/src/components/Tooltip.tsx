import { useEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

import { useAnchoredPosition, type Placement } from '../hooks';
import { cx } from '../utils';
import styles from './Tooltip.module.css';

export type TooltipPlacement = Placement;

export interface TooltipProps {
  content: ReactNode;
  placement?: TooltipPlacement;
  delay?: number;
  disabled?: boolean;
  className?: string;
  children: ReactNode;
}

/** Shows `content` next to its child after a short hover or on keyboard focus. */
export function Tooltip({
  content,
  placement = 'top',
  delay = 420,
  disabled = false,
  className,
  children,
}: TooltipProps) {
  const anchor = useRef<HTMLSpanElement>(null);
  const floating = useRef<HTMLDivElement>(null);
  const timer = useRef<number | undefined>(undefined);
  const [open, setOpen] = useState(false);
  const pos = useAnchoredPosition(anchor, floating, placement, open && !disabled);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const show = () => {
    if (disabled) return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(true), delay);
  };
  const hide = () => {
    window.clearTimeout(timer.current);
    setOpen(false);
  };

  return (
    <span
      ref={anchor}
      className={cx(styles.anchor, className)}
      onPointerEnter={show}
      onPointerLeave={hide}
      onPointerDown={hide}
      onFocus={(e) => {
        if ((e.target as HTMLElement).matches(':focus-visible')) show();
      }}
      onBlur={hide}
    >
      {children}
      {open &&
        !disabled &&
        createPortal(
          <div
            ref={floating}
            role="tooltip"
            className={cx(styles.tooltip, pos && styles.visible)}
            style={{ top: pos?.top ?? -9999, left: pos?.left ?? -9999 }}
          >
            {content}
          </div>,
          document.body,
        )}
    </span>
  );
}
