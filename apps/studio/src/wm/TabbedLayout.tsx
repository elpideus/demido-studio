import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { Tooltip, cx } from '@demido/ui';

import styles from './TabbedLayout.module.css';

/** Below this layout width the rail shows icons only; Settings (980) and Inspector (860) open expanded. */
const COLLAPSE_BELOW = 720;

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
 * so title bar and rail read as one frame around the darker page. In a narrow window the rail
 * collapses to icons, each naming itself in a tooltip.
 */
export function TabbedLayout<T extends string>({ tabs, active, onChange, children }: Props<T>) {
  const root = useRef<HTMLDivElement>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [animated, setAnimated] = useState(false);

  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return undefined;
    // Measured before the first paint and animated only from the observer's first report on,
    // so a window that opens narrow starts collapsed instead of collapsing in front of you.
    const measure = () => setCollapsed(el.clientWidth < COLLAPSE_BELOW);
    measure();
    const observer = new ResizeObserver(() => {
      measure();
      setAnimated(true);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={root} className={styles.layout}>
      <nav
        className={cx(styles.rail, collapsed && styles.collapsed, animated && styles.animated)}
        aria-label="Sections"
        data-collapsed={collapsed}
      >
        {tabs.map(({ id, label, icon: Icon, badge }) => (
          <Tooltip
            key={id}
            content={
              <span className={styles.tip}>
                {label}
                {badge !== undefined && <span className={styles.tipBadge}>{badge}</span>}
              </span>
            }
            placement="right"
            delay={250}
            disabled={!collapsed}
            className={styles.anchor}
          >
            <button
              type="button"
              role="tab"
              aria-selected={id === active}
              aria-label={collapsed ? label : undefined}
              className={cx(styles.tab, id === active && styles.active)}
              onClick={() => onChange(id)}
            >
              <Icon className={styles.icon} size={16} strokeWidth={1.8} aria-hidden />
              <span className={styles.label}>{label}</span>
              {badge !== undefined && (
                <>
                  <span className={styles.badge}>{badge}</span>
                  <span className={styles.dot} aria-hidden />
                </>
              )}
            </button>
          </Tooltip>
        ))}
      </nav>
      <div className={styles.page} role="tabpanel">
        {children}
      </div>
    </div>
  );
}
