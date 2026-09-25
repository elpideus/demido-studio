import { useEffect, useRef, useState, type RefObject } from 'react';
import { MessagesSquare, PinOff } from 'lucide-react';
import { Popover, cx } from '@demido/ui';

import type { Slot } from './geometry';
import styles from './SnapLayouts.module.css';

const SLOT_LABELS: Record<Slot, string> = {
  left: 'left',
  right: 'right',
  top: 'top',
  bottom: 'bottom',
  'top-left': 'top left',
  'top-right': 'top right',
  'bottom-left': 'bottom left',
  'bottom-right': 'bottom right',
};

/** The layouts offered, each drawn as a small desktop with the chat in the space left over. */
const LAYOUTS: Array<{ id: string; slots: Slot[] }> = [
  { id: 'sides', slots: ['left', 'right'] },
  { id: 'corners', slots: ['top-left', 'bottom-left', 'top-right', 'bottom-right'] },
  { id: 'rows', slots: ['top', 'bottom'] },
];

interface Props {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  /** The slot the window is pinned to, if it is. */
  current: Slot | null;
  onPick: (slot: Slot) => void;
  onUnpin: () => void;
  /** Focus the first layout when opened, for keyboard use. */
  autoFocus?: boolean;
  onPointerEnter?: () => void;
  onPointerLeave?: () => void;
}

/** Where a window can be pinned, shown like Windows 11's snap layouts when hovering Maximize. */
export function SnapLayouts({
  open,
  onClose,
  anchorRef,
  current,
  onPick,
  onUnpin,
  autoFocus,
  onPointerEnter,
  onPointerLeave,
}: Props) {
  const [hovered, setHovered] = useState<Slot | null>(null);
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      setHovered(null);
      return;
    }
    if (autoFocus) window.setTimeout(() => panel.current?.querySelector<HTMLElement>('button')?.focus(), 0);
  }, [open, autoFocus]);

  const caption = hovered
    ? hovered === current
      ? `Unpin from the ${SLOT_LABELS[hovered]}`
      : `Pin to the ${SLOT_LABELS[hovered]}`
    : current
      ? `Pinned to the ${SLOT_LABELS[current]}`
      : 'Pin this window';

  return (
    <Popover open={open} onClose={onClose} anchorRef={anchorRef} placement="bottom-end" aria-label="Snap layouts">
      <div ref={panel} className={styles.panel} onPointerEnter={onPointerEnter} onPointerLeave={onPointerLeave}>
        <div className={styles.layouts}>
          {LAYOUTS.map((layout) => (
            <div key={layout.id} className={cx(styles.layout, styles[layout.id])}>
              {layout.slots.map((slot) => (
                <button
                  key={slot}
                  type="button"
                  className={cx(styles.zone, styles[`zone-${slot}`], slot === current && styles.current)}
                  aria-label={`Pin to the ${SLOT_LABELS[slot]}`}
                  aria-pressed={slot === current}
                  onPointerEnter={() => setHovered(slot)}
                  onPointerLeave={() => setHovered(null)}
                  onFocus={() => setHovered(slot)}
                  onBlur={() => setHovered(null)}
                  onClick={() => {
                    onClose();
                    if (slot === current) onUnpin();
                    else onPick(slot);
                  }}
                />
              ))}
              <span className={styles.chat} aria-hidden>
                <MessagesSquare size={11} strokeWidth={1.8} />
              </span>
            </div>
          ))}
        </div>
        <div className={styles.footer}>
          <span className={styles.caption}>{caption}</span>
          {current && (
            <button
              type="button"
              className={styles.unpin}
              onClick={() => {
                onClose();
                onUnpin();
              }}
            >
              <PinOff size={12} strokeWidth={1.9} aria-hidden />
              Unpin
            </button>
          )}
        </div>
      </div>
    </Popover>
  );
}
