import { useRef, useState } from 'react';
import { Brain, Check } from 'lucide-react';
import { Popover, cx } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { canThink, currentThinking, thinkingChoices, thinkingInfo, withThinking } from '@/lib/thinking';
import type { ModelEntry } from '@/lib/types';
import { toast } from '@/stores/toasts';
import styles from './Composer.module.css';

/** The composer's thinking button: how hard the model thinks, for the models that can be asked. */
export function ThinkingPicker({ model }: { model: ModelEntry | undefined }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  if (!model || !canThink(model)) return null;

  const choices = thinkingChoices(model);
  const current = currentThinking(model);
  // "Thinking" says what the button is for when its value alone would not.
  const label =
    current === 'default' || current === 'on'
      ? 'Thinking'
      : current === 'off'
        ? 'No thinking'
        : thinkingInfo(current).label;

  return (
    <>
      <button
        ref={anchor}
        type="button"
        className={styles.chip}
        onClick={() => setOpen(!open)}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`Thinking: ${thinkingInfo(current).label}`}
        title="How hard the model thinks before answering"
      >
        <Brain size={15} strokeWidth={1.8} aria-hidden />
        <span className={styles.chipLabel}>{label}</span>
      </button>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        anchorRef={anchor}
        placement="top-start"
        width={320}
        aria-label="Thinking"
      >
        <div className={styles.modes} role="radiogroup" aria-label={`How hard ${model.name} thinks`}>
          {choices.map((choice) => {
            const info = thinkingInfo(choice, model);
            const selected = choice === current;
            return (
              <button
                key={choice}
                type="button"
                role="radio"
                aria-checked={selected}
                className={cx(styles.mode, selected && styles.modeSelected)}
                onClick={() => {
                  setOpen(false);
                  if (selected) return;
                  api
                    .updateModel(model.id, withThinking(model.settings, choice))
                    .catch((e) => toast.error('Could not change the thinking level', errorText(e)));
                }}
              >
                <span className={styles.modeText}>
                  <span className={styles.modeLabel}>{info.label}</span>
                  <span className={styles.modeDescription}>{info.description}</span>
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
