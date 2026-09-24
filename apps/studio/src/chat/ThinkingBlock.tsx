import { useState } from 'react';
import { Brain, ChevronRight } from 'lucide-react';
import { cx } from '@demido/ui';

import styles from './ThinkingBlock.module.css';

interface Props {
  text: string;
  /** Still being written. */
  live: boolean;
  /** Rough duration in seconds, when known. */
  seconds?: number;
}

/** The model's reasoning, collapsed to one line unless opened. */
export function ThinkingBlock({ text, live, seconds }: Props) {
  const [open, setOpen] = useState(false);
  const label = live ? 'Thinking' : seconds && seconds >= 1 ? `Thought for ${Math.round(seconds)}s` : 'Thoughts';
  return (
    <div className={styles.block}>
      <button type="button" className={styles.toggle} onClick={() => setOpen(!open)} aria-expanded={open}>
        <Brain size={14} strokeWidth={1.8} aria-hidden />
        <span className={cx(live && styles.shimmer)}>{label}</span>
        <ChevronRight size={13} className={cx(styles.chevron, open && styles.open)} aria-hidden />
      </button>
      {open && <div className={cx(styles.text, 'selectable')}>{text.trim()}</div>}
    </div>
  );
}
