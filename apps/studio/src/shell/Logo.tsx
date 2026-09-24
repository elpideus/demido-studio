import { Orbit } from 'lucide-react';

import styles from './Logo.module.css';

/** The Demido mark: Lucide's "orbit" on the green tile, as in the app icon. */
export function Logo({ size = 28 }: { size?: number }) {
  return (
    <span className={styles.logo} style={{ width: size, height: size, borderRadius: size * 0.26 }} aria-hidden>
      <Orbit size={size * 0.62} strokeWidth={2.2} />
    </span>
  );
}
