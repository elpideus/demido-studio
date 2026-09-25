import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { Search } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';

import { cx, matchesQuery } from '../utils';
import styles from './SearchList.module.css';

export interface SearchListItem {
  id: string;
  label: string;
  description?: string;
  /** Extra words the search should match (ids, file names, tags). */
  keywords?: string;
  group?: string;
  leading?: ReactNode;
  trailing?: ReactNode;
  disabled?: boolean;
}

export interface SearchListFooter {
  icon: LucideIcon;
  label: string;
  description?: string;
  onSelect: () => void;
}

export interface SearchListProps {
  items: SearchListItem[];
  onSelect: (item: SearchListItem) => void;
  selectedId?: string | null;
  placeholder?: string;
  emptyText?: string;
  /** Always-visible action at the bottom, outside the scrolling list. */
  footer?: SearchListFooter;
  autoFocus?: boolean;
  maxListHeight?: number;
  header?: ReactNode;
}

type Row =
  { kind: 'group'; key: string; label: string } | { kind: 'item'; key: string; item: SearchListItem; index: number };

/**
 * A search box over a keyboard-navigable list, with an optional pinned footer action.
 * Used by the model picker and the tools picker in the composer.
 */
export function SearchList({
  items,
  onSelect,
  selectedId,
  placeholder = 'Search',
  emptyText = 'Nothing matches your search.',
  footer,
  autoFocus = true,
  maxListHeight = 340,
  header,
}: SearchListProps) {
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const id = useId();

  const filtered = useMemo(
    () =>
      items.filter((it) =>
        matchesQuery(`${it.label} ${it.description ?? ''} ${it.keywords ?? ''} ${it.group ?? ''}`, query),
      ),
    [items, query],
  );

  const rows = useMemo(() => {
    const out: Row[] = [];
    let lastGroup: string | undefined;
    filtered.forEach((item, index) => {
      if (item.group && item.group !== lastGroup) {
        out.push({ kind: 'group', key: `g:${item.group}`, label: item.group });
      }
      lastGroup = item.group;
      out.push({ kind: 'item', key: item.id, item, index });
    });
    return out;
  }, [filtered]);

  // Navigable positions: every enabled item, then the footer.
  const count = filtered.length + (footer ? 1 : 0);

  useEffect(() => {
    const selected = filtered.findIndex((it) => it.id === selectedId);
    setActive(selected >= 0 ? selected : 0);
  }, [filtered, selectedId]);

  useEffect(() => {
    if (autoFocus) inputRef.current?.focus();
  }, [autoFocus]);

  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const choose = (index: number) => {
    if (footer && index === filtered.length) {
      footer.onSelect();
      return;
    }
    const item = filtered[index];
    if (item && !item.disabled) onSelect(item);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (count === 0) return;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setActive((a) => (a + 1) % count);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      setActive((a) => (a - 1 + count) % count);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      choose(active);
    }
  };

  const FooterIcon = footer?.icon;
  return (
    <div className={styles.root}>
      <div className={styles.searchRow}>
        <Search size={15} strokeWidth={1.9} className={styles.searchIcon} aria-hidden />
        <input
          ref={inputRef}
          className={styles.search}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={placeholder}
          aria-controls={`${id}-list`}
          aria-activedescendant={`${id}-opt-${active}`}
          spellCheck={false}
        />
      </div>
      {header}
      <div ref={listRef} id={`${id}-list`} role="listbox" className={styles.list} style={{ maxHeight: maxListHeight }}>
        {rows.length === 0 && <div className={styles.empty}>{emptyText}</div>}
        {rows.map((row) =>
          row.kind === 'group' ? (
            <div key={row.key} className={styles.group}>
              {row.label}
            </div>
          ) : (
            <div
              key={row.key}
              id={`${id}-opt-${row.index}`}
              role="option"
              aria-selected={row.item.id === selectedId}
              aria-disabled={row.item.disabled || undefined}
              data-index={row.index}
              className={cx(
                styles.item,
                row.index === active && styles.active,
                row.item.id === selectedId && styles.selected,
                row.item.disabled && styles.disabled,
              )}
              onPointerMove={() => setActive(row.index)}
              onClick={() => choose(row.index)}
            >
              {row.item.leading && <span className={styles.leading}>{row.item.leading}</span>}
              <span className={styles.text}>
                <span className={styles.label}>{row.item.label}</span>
                {row.item.description && <span className={styles.description}>{row.item.description}</span>}
              </span>
              {row.item.trailing && <span className={styles.trailing}>{row.item.trailing}</span>}
            </div>
          ),
        )}
      </div>
      {footer && FooterIcon && (
        <div className={styles.footer}>
          <div
            role="option"
            aria-selected={false}
            id={`${id}-opt-${filtered.length}`}
            data-index={filtered.length}
            className={cx(styles.item, styles.footerItem, active === filtered.length && styles.active)}
            onPointerMove={() => setActive(filtered.length)}
            onClick={() => choose(filtered.length)}
          >
            <span className={styles.leading}>
              <FooterIcon size={16} strokeWidth={1.8} aria-hidden />
            </span>
            <span className={styles.text}>
              <span className={styles.label}>{footer.label}</span>
              {footer.description && <span className={styles.description}>{footer.description}</span>}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
