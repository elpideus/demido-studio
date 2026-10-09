import { useEffect, useRef, useState } from 'react';
import { FoldVertical, Settings2 } from 'lucide-react';
import { Button, Popover, Tooltip, cx } from '@demido/ui';

import { api } from '@/lib/api';
import type { ContextUsage, ModelEntry } from '@/lib/types';
import { useApp } from '@/stores/app';
import { useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { useSkills } from '@/stores/skills';
import { useWindows } from '@/stores/windows';
import { contextView, estimateTokens, type ContextView } from './contextView';
import styles from './ContextMeter.module.css';

/** The parts of a request, in the order the bar stacks them. */
const PARTS = [
  { key: 'system', label: 'System prompt' },
  { key: 'tools', label: 'Tools' },
  { key: 'summary', label: 'Summary' },
  { key: 'files', label: 'Attached files' },
  { key: 'conversation', label: 'Conversation' },
] as const;

const RADIUS = 7;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

/**
 * The usage of `chatId` with `modelId`, fetched again whenever what the next request holds may
 * have changed: a message settles, the model, its window, the tools, the skills or the
 * auto-compact setting. Streamed text does not refetch; the answer counts once it is done.
 */
function useContextUsage(chatId: string | null, model: ModelEntry | undefined): ContextUsage | null {
  const messages = useChats((s) => (chatId ? s.messages[chatId] : undefined));
  const last = messages?.[messages.length - 1];
  const settled = `${messages?.length ?? 0}:${last?.id ?? ''}:${last?.status ?? ''}`;
  const autoCompact = useApp((s) => s.settings?.autoCompact);
  const autoCompactTokens = useApp((s) => s.settings?.autoCompactTokens);
  const runtime = useModels((s) => s.runtime);
  const served = `${runtime?.state}:${runtime?.modelId}:${runtime?.contextLength}`;
  const skills = useSkills((s) => s.skills);
  const groups = useSkills((s) => s.groups);
  const [result, setResult] = useState<{ key: string; usage: ContextUsage } | null>(null);
  const key = `${chatId ?? ''}|${model?.id ?? ''}`;

  useEffect(() => {
    if (!model) return;
    let current = true;
    const timer = window.setTimeout(() => {
      api.contextUsage(chatId, model.id).then(
        (usage) => current && setResult({ key, usage }),
        () => undefined,
      );
    }, 200);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [key, chatId, model, settled, autoCompact, autoCompactTokens, served, skills, groups]);

  return result?.key === key ? result.usage : null;
}

interface Props {
  chatId: string | null;
  model: ModelEntry | undefined;
  /** The message being written, counted toward the next request. */
  draft: string;
  running: boolean;
  /** Runs `/compact`. */
  onCompact: () => void;
}

/**
 * A ring in the composer that fills up as the chat nears compaction (or, with auto-compact off,
 * the end of the model's context window). Hovering says how far; clicking shows what the
 * context holds, part by part.
 */
export function ContextMeter({ chatId, model, draft, running, onCompact }: Props) {
  const usage = useContextUsage(chatId, model);
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const openWindow = useWindows((s) => s.open);
  const draftTokens = estimateTokens(draft.trim());

  useEffect(() => setOpen(false), [chatId]);

  if (!usage || !model) return null;
  const view = contextView(usage, draftTokens, running, model.name);
  const showPercent = view.level !== 'low';

  return (
    <>
      <Tooltip
        placement="top"
        delay={250}
        disabled={open}
        content={
          <span className={styles.tip}>
            <span className={styles.tipTitle}>
              <span className={cx(styles.dot, styles[view.level])} aria-hidden />
              {view.percentLabel} of context used
            </span>
            <span className={styles.tipText}>{view.headline}</span>
            <span className={styles.tipHint}>Click for details</span>
          </span>
        }
      >
        <button
          ref={anchor}
          type="button"
          className={cx(styles.meter, showPercent && styles.meterWide, styles[view.level])}
          onClick={() => setOpen(!open)}
          aria-haspopup="dialog"
          aria-expanded={open}
          aria-label={`Context: ${view.percentLabel} used. ${view.headline}`}
        >
          <Ring share={view.share} />
          {showPercent && <span className={styles.percent}>{view.percentLabel}</span>}
        </button>
      </Tooltip>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        anchorRef={anchor}
        placement="top-end"
        width={316}
        aria-label="Context"
      >
        <Details
          usage={usage}
          view={view}
          draft={draftTokens}
          canCompact={!!chatId && !running}
          onCompact={() => {
            setOpen(false);
            onCompact();
          }}
          onSettings={() => {
            setOpen(false);
            openWindow('settings', { tab: 'general' });
          }}
        />
      </Popover>
    </>
  );
}

function Ring({ share }: { share: number }) {
  // A sliver stays visible from the first token, so an empty-looking ring never means "broken".
  const shown = share > 0 ? Math.max(share, 0.04) : 0;
  return (
    <svg className={styles.ring} width="18" height="18" viewBox="0 0 18 18" aria-hidden>
      <circle className={styles.track} cx="9" cy="9" r={RADIUS} />
      <circle
        className={styles.fill}
        cx="9"
        cy="9"
        r={RADIUS}
        strokeDasharray={CIRCUMFERENCE}
        strokeDashoffset={CIRCUMFERENCE * (1 - shown)}
      />
    </svg>
  );
}

interface DetailsProps {
  usage: ContextUsage;
  view: ContextView;
  draft: number;
  canCompact: boolean;
  onCompact: () => void;
  onSettings: () => void;
}

function Details({ usage, view, draft, canCompact, onCompact, onSettings }: DetailsProps) {
  const parts = PARTS.map((p) => ({ ...p, tokens: usage.parts[p.key] })).filter((p) => p.tokens > 0);
  // The bar spans the way to full; past it, everything there is.
  const span = Math.max(view.limit, view.used);
  const rest = span - view.used;
  const auto = usage.threshold !== null;

  return (
    <div className={styles.details}>
      <div className={styles.head}>
        <span className={styles.title}>Context</span>
        <span className={styles.headPercent}>
          <span className={cx(styles.dot, styles[view.level])} aria-hidden />
          {view.percentLabel}
        </span>
      </div>
      <div className={styles.sub}>
        {n(view.used)} of {n(view.limit)} tokens {auto ? 'before auto-compact' : 'before the window fills'}
      </div>

      <div
        className={styles.bar}
        role="img"
        aria-label={`${view.percentLabel} of the way to ${auto ? 'auto-compact' : 'a full window'}`}
      >
        {parts.map((p) => (
          <span key={p.key} className={cx(styles.segment, styles[p.key])} style={{ flexGrow: p.tokens }} />
        ))}
        {draft > 0 && <span className={cx(styles.segment, styles.draft)} style={{ flexGrow: draft }} />}
        {rest > 0 && <span className={cx(styles.segment, styles.rest)} style={{ flexGrow: rest }} />}
      </div>

      <ul className={styles.legend}>
        {parts.map((p) => (
          <li key={p.key} className={styles.row}>
            <span className={cx(styles.swatch, styles[p.key])} aria-hidden />
            <span className={styles.rowLabel}>{p.label}</span>
            <span className={styles.rowValue}>{n(p.tokens)}</span>
          </li>
        ))}
        {draft > 0 && (
          <li className={styles.row}>
            <span className={cx(styles.swatch, styles.draft)} aria-hidden />
            <span className={styles.rowLabel}>Your message</span>
            <span className={styles.rowValue}>{n(draft)}</span>
          </li>
        )}
      </ul>

      <dl className={styles.facts}>
        <div className={styles.fact}>
          <dt>Auto-compact</dt>
          <dd>{auto ? `at ${n(usage.threshold!)}` : 'Off'}</dd>
        </div>
        <div className={styles.fact}>
          <dt>Kept for the answer</dt>
          <dd>{n(usage.reserve)}</dd>
        </div>
        <div className={styles.fact}>
          <dt>Context window</dt>
          <dd>
            {n(usage.window)}
          </dd>
        </div>
        {usage.compactions > 0 && (
          <div className={styles.fact}>
            <dt>Compacted</dt>
            <dd>{usage.compactions === 1 ? 'once' : `${usage.compactions} times`}</dd>
          </div>
        )}
      </dl>

      <p className={cx(styles.outlook, styles[`outlook-${view.level}`])}>{view.outlook}</p>
      {view.warning && <p className={styles.warning}>{view.warning}</p>}
      <p className={styles.source}>
        {usage.counted
          ? 'Counted by the model on its last answer; the split between parts is estimated.'
          : 'Estimated from the text, about three characters a token. The model’s own count is used when it is higher.'}
      </p>

      <div className={styles.actions}>
        <Button size="sm" variant="secondary" icon={FoldVertical} disabled={!canCompact} onClick={onCompact}>
          Compact now
        </Button>
        <Button size="sm" variant="ghost" icon={Settings2} onClick={onSettings}>
          Settings
        </Button>
      </div>
    </div>
  );
}

function n(tokens: number): string {
  return tokens.toLocaleString();
}
