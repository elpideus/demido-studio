import { useEffect, useRef, useState, type KeyboardEvent, type RefObject } from 'react';
import type { LucideIcon } from 'lucide-react';

import type { Placement } from '../hooks';
import { cx } from '../utils';
import { SwitchMark } from './Controls';
import { Popover } from './Popover';
import styles from './Menu.module.css';

export type MenuEntry =
  | {
      id: string;
      label: string;
      icon?: LucideIcon;
      shortcut?: string;
      danger?: boolean;
      disabled?: boolean;
      checked?: boolean;
      /** An on/off setting, shown as a switch; the menu stays open when it is flipped. */
      toggle?: boolean;
      onSelect: () => void;
    }
  | 'separator';

export interface MenuProps {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  items: MenuEntry[];
  placement?: Placement;
  width?: number;
}

/** A dropdown or context menu with keyboard navigation. */
export function Menu({ open, onClose, anchorRef, items, placement = 'bottom-end', width = 200 }: MenuProps) {
  const actionable = items.filter((i): i is Exclude<MenuEntry, 'separator'> => i !== 'separator');
  const [active, setActive] = useState(-1);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) {
      setActive(-1);
      window.setTimeout(() => listRef.current?.focus(), 0);
    }
  }, [open]);

  const run = (index: number) => {
    const item = actionable[index];
    if (!item || item.disabled) return;
    if (item.toggle === undefined) onClose();
    item.onSelect();
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActive((a) => (a + 1) % actionable.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive((a) => (a <= 0 ? actionable.length - 1 : a - 1));
    } else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      run(active);
    }
  };

  let index = -1;
  return (
    <Popover open={open} onClose={onClose} anchorRef={anchorRef} placement={placement} width={width}>
      <div ref={listRef} role="menu" tabIndex={-1} className={styles.menu} onKeyDown={onKeyDown}>
        {items.map((item, i) => {
          if (item === 'separator') return <div key={`sep-${i}`} className={styles.separator} />;
          index += 1;
          const myIndex = index;
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              type="button"
              role={item.toggle === undefined ? 'menuitem' : 'menuitemcheckbox'}
              aria-checked={item.toggle}
              disabled={item.disabled}
              className={cx(styles.item, item.danger && styles.danger, myIndex === active && styles.active)}
              onPointerMove={() => setActive(myIndex)}
              onClick={() => run(myIndex)}
            >
              <span className={styles.icon}>{Icon && <Icon size={15} strokeWidth={1.8} aria-hidden />}</span>
              <span className={styles.label}>{item.label}</span>
              {item.checked && <span className={styles.check} aria-label="selected" />}
              {item.toggle !== undefined && <SwitchMark checked={item.toggle} />}
              {item.shortcut && <span className={styles.shortcut}>{item.shortcut}</span>}
            </button>
          );
        })}
      </div>
    </Popover>
  );
}
