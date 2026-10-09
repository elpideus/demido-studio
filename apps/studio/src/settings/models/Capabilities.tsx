import { Brain, Ear, Eye, Wrench, type LucideIcon } from 'lucide-react';
import { Spinner, Tooltip, cx } from '@demido/ui';

import type { ModelCapabilities, ModelEntry } from '@/lib/types';
import styles from './Capabilities.module.css';

interface Capability {
  key: keyof ModelCapabilities;
  icon: LucideIcon;
  label: string;
  /** What having it means, in a few words. */
  meaning: string;
}

/** Every capability, in the order they are shown everywhere. */
const CAPABILITIES: Capability[] = [
  { key: 'vision', icon: Eye, label: 'Vision', meaning: 'Understands images' },
  { key: 'audio', icon: Ear, label: 'Audio', meaning: 'Understands audio' },
  { key: 'tools', icon: Wrench, label: 'Tools', meaning: 'Uses tools, like market data and Python' },
  { key: 'thinking', icon: Brain, label: 'Thinking', meaning: 'Thinks before answering' },
];

const CHECKING = 'Checking what this model can do';

/**
 * An icon for each thing a model can do, explained on hover. While llama.cpp checks a local
 * model, a spinner stands in for them.
 */
export function CapabilityIcons({ model, className }: { model: ModelEntry; className?: string }) {
  const present = CAPABILITIES.filter((c) => model.capabilities[c.key] === true);
  if (!model.checkingCapabilities && present.length === 0) return null;
  return (
    <span className={cx(styles.icons, className)}>
      {model.checkingCapabilities ? (
        <Tooltip content={CHECKING}>
          <span role="img" aria-label={CHECKING} className={styles.icon}>
            <Spinner size={11} />
          </span>
        </Tooltip>
      ) : (
        present.map(({ key, icon: Icon, meaning }) => (
          <Tooltip key={key} content={meaning}>
            <span role="img" aria-label={meaning} className={styles.icon} data-capability={key}>
              <Icon size={14} strokeWidth={1.8} aria-hidden />
            </span>
          </Tooltip>
        ))
      )}
    </span>
  );
}

/** Every capability with the model's answer, and where the answers came from. */
export function CapabilityList({ model }: { model: ModelEntry }) {
  return (
    <div className={styles.list}>
      <ul className={styles.grid}>
        {CAPABILITIES.map(({ key, icon: Icon, label, meaning }) => {
          const value = model.capabilities[key];
          return (
            <li key={key} className={cx(styles.item, value === true && styles.has)}>
              <span className={styles.itemIcon}>
                <Icon size={16} strokeWidth={1.8} aria-hidden />
              </span>
              <span className={styles.itemText}>
                <span className={styles.itemLabel}>{label}</span>
                <span className={styles.itemMeaning}>{meaning}</span>
              </span>
              <span className={styles.status}>
                {model.checkingCapabilities ? (
                  <Spinner size={12} />
                ) : value === true ? (
                  'Yes'
                ) : value === false ? (
                  'No'
                ) : (
                  'Unknown'
                )}
              </span>
            </li>
          );
        })}
      </ul>
      <p className={styles.source}>{sourceNote(model)}</p>
    </div>
  );
}

function sourceNote(m: ModelEntry): string {
  if (m.source === 'openrouter') return 'From OpenRouter’s list of models.';
  if (m.source !== 'local') {
    const { vision, audio, tools } = m.capabilities;
    return [vision, audio, tools].some((v) => v === null)
      ? 'Thinking comes from Google’s list of models. models.dev, the open database of AI models the rest comes from, does not list this one yet.'
      : 'Thinking comes from Google’s list of models, the rest from models.dev, an open database of AI models.';
  }
  if (m.checkingCapabilities) {
    return 'llama.cpp, the engine that runs this model, is reading its files to find out. It takes a few seconds, once.';
  }
  const known = Object.values(m.capabilities).some((v) => v !== null);
  return known
    ? 'Reported by llama.cpp, the engine that runs this model, from its files.'
    : 'llama.cpp could not open this model to find out. It tries again the next time Demido Studio starts.';
}
