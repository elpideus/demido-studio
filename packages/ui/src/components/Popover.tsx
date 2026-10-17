import { useEffect, useRef, type CSSProperties, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';

import { useAnchoredPosition, useClickOutside, useEscape, type Placement } from '../hooks';
import { cx } from '../utils';
import styles from './Popover.module.css';

export interface PopoverProps {
  open: boolean;
  onClose: () => void;
  /** Element the popover attaches to. */
  anchorRef: RefObject<HTMLElement | null>;
  placement?: Placement;
  width?: number | string;
  maxHeight?: number | string;
  className?: string;
  style?: CSSProperties;
  /** Close when the pointer is pressed outside the anchor and the popover. */
  dismissOnOutside?: boolean;
  children: ReactNode;
  'aria-label'?: string;
}

/**
 * Open popovers, oldest first. One opened from inside another (a menu in a panel) is nested in it:
 * a press inside the newer one is not outside the older, and Escape closes only the newest.
 */
const openStack: number[] = [];
let opened = 0;

/** A floating panel anchored to an element, rendered above everything else. */
export function Popover({
  open,
  onClose,
  anchorRef,
  placement = 'bottom-start',
  width,
  maxHeight,
  className,
  style,
  dismissOnOutside = true,
  children,
  'aria-label': ariaLabel,
}: PopoverProps) {
  const floating = useRef<HTMLDivElement>(null);
  const pos = useAnchoredPosition(anchorRef, floating, placement, open);
  const refs = useRef([anchorRef, floating]).current;
  const order = useRef(0);
  useEffect(() => {
    if (!open) return undefined;
    const mine = ++opened;
    order.current = mine;
    openStack.push(mine);
    return () => {
      openStack.splice(openStack.indexOf(mine), 1);
    };
  }, [open]);
  useClickOutside(
    refs,
    (event) => {
      const other = (event.target as Element | null)?.closest?.<HTMLElement>('[data-popover]');
      if (other && Number(other.dataset.popover) > order.current) return;
      onClose();
    },
    open && dismissOnOutside,
  );
  useEscape(() => {
    if (openStack.at(-1) === order.current) onClose();
  }, open);

  if (!open) return null;
  const side = pos?.placement.split('-')[0] ?? placement.split('-')[0];
  return createPortal(
    <div
      ref={floating}
      role="dialog"
      aria-label={ariaLabel}
      data-popover={order.current}
      className={cx(styles.popover, pos && styles.visible, styles[`from-${side}`], className)}
      style={{
        top: pos?.top ?? -9999,
        left: pos?.left ?? -9999,
        width,
        maxHeight,
        ...style,
      }}
    >
      {children}
    </div>,
    document.body,
  );
}
