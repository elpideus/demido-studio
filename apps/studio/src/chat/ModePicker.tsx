import { useRef, useState } from 'react';
import { Check } from 'lucide-react';
import { Popover, cx } from '@demido/ui';

import { errorText } from '@/lib/api';
import { PERMISSION_MODES, permissionMode } from '@/lib/permissionModes';
import { useApp } from '@/stores/app';
import { toast } from '@/stores/toasts';
import styles from './Composer.module.css';

/** The composer's permission mode button: how often the assistant asks before using a tool. */
export function ModePicker() {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const value = useApp((s) => s.settings?.permissionMode);
  const patchSettings = useApp((s) => s.patchSettings);
  const current = permissionMode(value);
  const Icon = current.icon;

  return (
    <>
      <button
        ref={anchor}
        type="button"
        className={cx(styles.chip, current.value === 'autonomous' && styles.chipWarning)}
        onClick={() => setOpen(!open)}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`Permissions: ${current.label}`}
        title="When the assistant asks before using a tool"
      >
        <Icon size={15} strokeWidth={1.8} aria-hidden />
        <span className={styles.chipLabel}>{current.label}</span>
      </button>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        anchorRef={anchor}
        placement="top-start"
        width={340}
        aria-label="Permissions"
      >
        <div className={styles.modes} role="radiogroup" aria-label="Permissions">
          {PERMISSION_MODES.map((m) => {
            const ModeIcon = m.icon;
            const selected = m.value === current.value;
            return (
              <button
                key={m.value}
                type="button"
                role="radio"
                aria-checked={selected}
                className={cx(styles.mode, selected && styles.modeSelected)}
                onClick={() => {
                  setOpen(false);
                  if (!selected) {
                    patchSettings({ permissionMode: m.value }).catch((e) =>
                      toast.error('Could not change the mode', errorText(e)),
                    );
                  }
                }}
              >
                <ModeIcon size={16} strokeWidth={1.8} aria-hidden className={styles.modeIcon} />
                <span className={styles.modeText}>
                  <span className={styles.modeLabel}>{m.label}</span>
                  <span className={styles.modeDescription}>{m.description}</span>
                </span>
                {selected && <Check size={15} aria-hidden className={styles.modeCheck} />}
              </button>
            );
          })}
        </div>
      </Popover>
    </>
  );
}
