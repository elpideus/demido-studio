import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { cx } from '@demido/ui';

import styles from './TabbedLayout.module.css';

export interface TabSpec<T extends string> {
  id: T;
  label: string;
  icon: LucideIcon;
  badge?: ReactNode;
}

interface Props<T extends string> {
  tabs: Array<TabSpec<T>>;
  active: T;
  onChange: (id: T) => void;
  children: ReactNode;
}

/**
 * A window body with its tabs in a rail on the left. The rail shares the title bar's color,
 * so title bar and rail read as one frame around the darker page.
 */
export function TabbedLayout<T extends string>({ tabs, active, onChange, children }: Props<T>) {
  return (
    <div className={styles.layout}>
      <nav className={styles.rail} aria-label="Sections">
        {tabs.map(({ id, label, icon: Icon, badge }) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={id === active}
            className={cx(styles.tab, id === active && styles.active)}
            onClick={() => onChange(id)}
          >
            <Icon size={16} strokeWidth={1.8} aria-hidden />
            <span className={styles.label}>{label}</span>
            {badge !== undefined && <span className={styles.badge}>{badge}</span>}
          </button>
        ))}
      </nav>
      <div className={styles.page} role="tabpanel">
        {children}
      </div>
    </div>
  );
}
