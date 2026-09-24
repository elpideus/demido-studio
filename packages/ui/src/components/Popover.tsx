import { useRef, type CSSProperties, type ReactNode, type RefObject } from 'react';
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
  useClickOutside(refs, onClose, open && dismissOnOutside);
  useEscape(onClose, open);

  if (!open) return null;
  const side = pos?.placement.split('-')[0] ?? placement.split('-')[0];
  return createPortal(
    <div
      ref={floating}
      role="dialog"
      aria-label={ariaLabel}
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
