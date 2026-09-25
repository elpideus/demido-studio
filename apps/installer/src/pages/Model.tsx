import { ArrowLeft, ArrowRight, Ban, Sparkles } from 'lucide-react';
import { Badge, Button, cx, formatBytes } from '@demido/ui';

import type { Context, WizardState } from '../types';
import styles from '../Setup.module.css';

interface Props {
  ctx: Context;
  state: WizardState;
  onChange: (patch: Partial<WizardState>) => void;
  onBack: () => void;
  onNext: () => void;
}

export function ModelPage({ ctx, state, onChange, onBack, onNext }: Props) {
  const rec = ctx.recommendations[state.backend];
  const choice = ctx.choices.find((c) => c.backend === state.backend);
  const budget = choice?.usesSystemMemory
    ? `${Math.round(ctx.hardware.totalMemory / 1024 ** 3)} GB of memory`
    : `${Math.round(choice?.memoryBudgetGb ?? 0)} GB of graphics memory`;
  return (
    <div className={styles.page}>
      <div className={styles.pageBody}>
        <h1 className={styles.title}>Choose your first model</h1>
        <p className={styles.lead}>
          Picked for {budget}. Both are good all-rounders; you can download others later from Settings.
        </p>
        <div className={styles.options} role="radiogroup" aria-label="Model">
          {rec?.picks.map(([family, pick]) => {
            const fam = ctx.families.find((f) => f.id === family);
            const selected = state.family === family;
            return (
              <button
                key={family}
                type="button"
                role="radio"
                aria-checked={selected}
                className={cx(styles.option, selected && styles.selected)}
                onClick={() => onChange({ family })}
              >
                <span className={styles.radio} />
                <span className={styles.optionIcon}>
                  <Sparkles size={20} strokeWidth={1.8} aria-hidden />
                </span>
                <span className={styles.optionText}>
                  <span className={styles.optionTitle}>
                    {pick.name}
                    {fam?.recommended && <Badge tone="accent">Recommended</Badge>}
                  </span>
                  <span className={styles.optionDesc}>{fam?.description}</span>
                  <span className={styles.optionNote}>
                    {fam?.vendor} · {pick.quant} · {pick.repo}
                  </span>
                </span>
                <span className={styles.optionSide}>{formatBytes(pick.size)}</span>
              </button>
            );
          })}
          <button
            type="button"
            role="radio"
            aria-checked={state.family === null}
            className={cx(styles.option, state.family === null && styles.selected)}
            onClick={() => onChange({ family: null })}
          >
            <span className={styles.radio} />
            <span className={styles.optionIcon}>
              <Ban size={20} strokeWidth={1.8} aria-hidden />
            </span>
            <span className={styles.optionText}>
              <span className={styles.optionTitle}>Skip for now</span>
              <span className={styles.optionDesc}>
                Download a model later, or use a cloud model such as Google Gemini with an API key.
              </span>
            </span>
          </button>
        </div>
      </div>
      <div className={styles.footer}>
        <Button variant="ghost" icon={ArrowLeft} onClick={onBack}>
          Back
        </Button>
        <span className={styles.footerInfo} />
        <Button variant="primary" size="lg" iconRight={ArrowRight} onClick={onNext}>
          Next
        </Button>
      </div>
    </div>
  );
}
