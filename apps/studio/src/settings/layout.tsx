import { createContext, useContext, type CSSProperties, type ReactNode } from 'react';
import { cx } from '@demido/ui';

import { PAGES, type SettingsPage, type SettingsTab } from './pages';
import type { SettingTarget } from './searchIndex';
import s from './settings.module.css';

const NavContext = createContext<(target: SettingTarget) => void>(() => undefined);

/** Lets a page send the Settings window to another page or setting. */
export const SettingsNavProvider = NavContext.Provider;

/** Opens another page, or a setting on one, in this Settings window. */
export function useSettingsNav(): (target: SettingTarget) => void {
  return useContext(NavContext);
}

/** A page's icon on a tile of its color. */
export function PageTile({ page, size = 40 }: { page: SettingsPage; size?: number }) {
  const Icon = page.icon;
  return (
    <span
      className={s.tile}
      style={{ '--tint': page.tint, width: size, height: size, borderRadius: size * 0.28 } as CSSProperties}
      aria-hidden
    >
      <Icon size={Math.round(size * 0.5)} strokeWidth={1.8} />
    </span>
  );
}

/** A page's title, what it is for, and its actions, from its entry in `PAGES`. */
export function PageHeader({ page, actions }: { page: SettingsTab; actions?: ReactNode }) {
  const spec = PAGES.find((p) => p.id === page)!;
  return (
    <header className={s.header}>
      <PageTile page={spec} />
      <div className={s.headerText}>
        <h2 className={s.title}>{spec.label}</h2>
        <p className={s.subtitle}>{spec.description}</p>
      </div>
      {actions && <div className={s.headerActions}>{actions}</div>}
    </header>
  );
}

/** Settings that belong together: a heading, their rows, and a note on what they share. */
export function Group({
  title,
  aside,
  footer,
  setting,
  children,
}: {
  title?: ReactNode;
  /** Beside the heading, on its right: a status, or an action for the whole group. */
  aside?: ReactNode;
  footer?: ReactNode;
  /** Makes the whole group the place a search result points at. */
  setting?: string;
  children: ReactNode;
}) {
  return (
    <section className={s.section}>
      {(title || aside) && (
        <h3 className={s.sectionTitle}>
          {title}
          {aside}
        </h3>
      )}
      <div className={s.rows} data-setting={setting} data-group>
        {children}
      </div>
      {footer && <p className={s.footer}>{footer}</p>}
    </section>
  );
}

/** One setting: what it is on the left, its control on the right (or under it, when narrow). */
export function Row({
  setting,
  label,
  description,
  icon,
  className,
  children,
}: {
  /** What a search result for this setting points at. */
  setting?: string;
  label: ReactNode;
  description?: ReactNode;
  icon?: ReactNode;
  className?: string;
  children?: ReactNode;
}) {
  return (
    <div className={cx(s.row, className)} data-setting={setting}>
      {icon && <span className={s.rowIcon}>{icon}</span>}
      <div className={s.rowMain}>
        <div className={s.rowTitle}>{label}</div>
        {description && <div className={s.rowDescription}>{description}</div>}
      </div>
      {children !== undefined && children !== null && children !== false && (
        <div className={s.rowActions}>{children}</div>
      )}
    </div>
  );
}

/** A read-only value, on the right of its row. */
export function Value({ children, title }: { children: ReactNode; title?: string }) {
  return (
    <span className={s.value} title={title}>
      {children}
    </span>
  );
}
