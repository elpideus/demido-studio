import { Cloud, Download, LineChart, Lightbulb, Sparkles, SquareTerminal, Terminal } from 'lucide-react';
import { Button, Logo } from '@demido/ui';

import type { ModelEntry } from '@/lib/types';
import { useSkills } from '@/stores/skills';
import { useWindows } from '@/stores/windows';
import styles from './EmptyChat.module.css';

interface Props {
  model: ModelEntry | undefined;
  onSuggest: (text: string) => void;
}

function greeting(): string {
  const h = new Date().getHours();
  if (h < 5) return 'Working late?';
  if (h < 12) return 'Good morning';
  if (h < 18) return 'Good afternoon';
  return 'Good evening';
}

/** The start of a new chat: a greeting and a few things to try. */
export function EmptyChat({ model, onSuggest }: Props) {
  const openWindow = useWindows((s) => s.open);
  const groups = useSkills((s) => s.groups);
  const on = (id: string) => groups.some((g) => g.id === id && g.enabled && g.available);

  if (!model) {
    return (
      <div className={styles.empty}>
        <Logo size={52} />
        <h1 className={styles.title}>No model is ready yet</h1>
        <p className={styles.subtitle}>
          Download a model to run privately on this computer, or connect Google Gemini or OpenRouter with an API key.
        </p>
        <div className={styles.actions}>
          <Button
            variant="primary"
            icon={Download}
            onClick={() => openWindow('settings', { tab: 'models', view: 'download' })}
          >
            Download a model
          </Button>
          <Button variant="secondary" icon={Cloud} onClick={() => openWindow('settings', { tab: 'providers' })}>
            Connect a provider
          </Button>
        </div>
      </div>
    );
  }

  const suggestions = [
    on('market') && {
      icon: LineChart,
      title: 'Check a market',
      text: "What's the EUR/USD price right now, and how has it moved today?",
    },
    on('market') &&
      on('coding') && {
        icon: Terminal,
        title: 'Analyse data',
        text: 'Get the last 3 months of daily gold (XAUUSD) candles, plot the closing price with a 20-day moving average, and describe the trend.',
      },
    on('coding') && {
      icon: SquareTerminal,
      title: 'Use your programs',
      text: 'Ping google.com four times and tell me the average response time.',
    },
    on('skills') && {
      icon: Sparkles,
      title: 'Teach it a skill',
      text: 'Create a skill that summarises any market symbol: its latest price, the change over the last week, and a one-paragraph outlook.',
    },
    {
      icon: Lightbulb,
      title: 'Explain a concept',
      text: 'Explain what a moving average crossover is, with a simple example.',
    },
  ]
    .filter(Boolean)
    // Two rows of two.
    .slice(0, 4) as Array<{ icon: typeof Lightbulb; title: string; text: string }>;

  return (
    <div className={styles.empty}>
      <Logo size={52} />
      <h1 className={styles.title}>{greeting()}</h1>
      <p className={styles.subtitle}>
        You are talking to <strong>{model.name}</strong>
        {model.source === 'local' ? ', running privately on this computer.' : '.'}
      </p>
      <div className={styles.grid}>
        {suggestions.map((s) => (
          <button key={s.title} type="button" className={styles.card} onClick={() => onSuggest(s.text)}>
            <s.icon size={17} strokeWidth={1.8} className={styles.cardIcon} aria-hidden />
            <span className={styles.cardTitle}>{s.title}</span>
            <span className={styles.cardText}>{s.text}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
