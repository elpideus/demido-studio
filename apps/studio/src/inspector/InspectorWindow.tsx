import { useEffect, useMemo, useState } from 'react';
import { Braces, FileJson, Gauge, MessageSquareText, Wrench } from 'lucide-react';
import { EmptyState, Spinner, cx } from '@demido/ui';

import { CopyButton } from '@/chat/Markdown';
import { api, errorText } from '@/lib/api';
import { formatDateTime } from '@/lib/format';
import type { Trace } from '@/lib/types';
import type { WindowState } from '@/stores/windows';
import { TabbedLayout, type TabSpec } from '@/wm/TabbedLayout';
import styles from './InspectorWindow.module.css';

type Tab = 'overview' | 'prompt' | 'tools' | 'response' | 'raw';

const TABS: Array<TabSpec<Tab>> = [
  { id: 'overview', label: 'Overview', icon: Gauge },
  { id: 'prompt', label: 'Prompt', icon: MessageSquareText },
  { id: 'tools', label: 'Tools offered', icon: Wrench },
  { id: 'response', label: 'Response', icon: Braces },
  { id: 'raw', label: 'Raw JSON', icon: FileJson },
];

interface PromptMessage {
  role: string;
  text: string;
  extra?: string;
}

interface Normalized {
  system: string;
  messages: PromptMessage[];
  tools: Array<{ name: string; description: string; parameters: unknown }>;
  params: Record<string, unknown>;
}

type Json = Record<string, unknown>;

/** Reads both request shapes (OpenAI-compatible and Gemini) into one view. */
function normalize(request: Json): Normalized {
  if (Array.isArray(request.contents)) {
    const sys = (request.systemInstruction as Json | undefined)?.parts as Json[] | undefined;
    const decls = ((request.tools as Json[] | undefined)?.[0]?.functionDeclarations ?? []) as Json[];
    return {
      system: (sys ?? []).map((p) => String(p.text ?? '')).join('\n'),
      messages: (request.contents as Json[]).map((c) => {
        const parts = (c.parts as Json[]) ?? [];
        const text = parts.map((p) => (typeof p.text === 'string' ? p.text : '')).join('');
        const calls = parts.filter((p) => p.functionCall).map((p) => JSON.stringify(p.functionCall));
        const results = parts.filter((p) => p.functionResponse).map((p) => JSON.stringify(p.functionResponse));
        return { role: String(c.role), text, extra: [...calls, ...results].join('\n') || undefined };
      }),
      tools: decls.map((d) => ({
        name: String(d.name),
        description: String(d.description ?? ''),
        parameters: d.parameters,
      })),
      params: (request.generationConfig as Json) ?? {},
    };
  }
  const messages = (request.messages as Json[] | undefined) ?? [];
  const system = messages
    .filter((m) => m.role === 'system')
    .map((m) => String(m.content ?? ''))
    .join('\n');
  const { messages: _m, tools: _t, ...params } = request;
  return {
    system,
    messages: messages
      .filter((m) => m.role !== 'system')
      .map((m) => ({
        role: String(m.role),
        text: typeof m.content === 'string' ? m.content : JSON.stringify(m.content ?? ''),
        extra: m.tool_calls
          ? JSON.stringify(m.tool_calls, null, 2)
          : m.tool_call_id
            ? `result of ${String(m.tool_call_id)}`
            : undefined,
      })),
    tools: ((request.tools as Json[] | undefined) ?? []).map((t) => {
      const f = t.function as Json;
      return { name: String(f.name), description: String(f.description ?? ''), parameters: f.parameters };
    }),
    params,
  };
}

function Stat({ label, value }: { label: string; value: string | number | null | undefined }) {
  if (value === null || value === undefined || value === '') return null;
  return (
    <div className={styles.stat}>
      <div className={styles.statValue}>{value}</div>
      <div className={styles.statLabel}>{label}</div>
    </div>
  );
}

function Pre({ text }: { text: string }) {
  return (
    <div className={styles.preWrap}>
      <div className={styles.preBar}>
        <CopyButton text={text} />
      </div>
      <pre className={cx(styles.pre, 'selectable')}>{text}</pre>
    </div>
  );
}

export function InspectorWindow({ win }: { win: WindowState }) {
  const messageId = typeof win.props.messageId === 'string' ? win.props.messageId : null;
  const [trace, setTrace] = useState<Trace | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('overview');

  useEffect(() => {
    setTrace(undefined);
    setError(null);
    if (messageId) api.getTrace(messageId).then(setTrace, (e) => setError(errorText(e)));
  }, [messageId]);

  const view = useMemo(() => (trace ? normalize(trace.request) : null), [trace]);
  const response = (trace?.response ?? {}) as Json;
  const usage = (response.usage ?? {}) as Json;
  const timings = (response.timings ?? {}) as Json;

  if (trace === undefined && !error) {
    return (
      <div className={styles.center}>
        <Spinner />
      </div>
    );
  }
  if (!trace || !view) {
    return <EmptyState title="No record for this answer" description={error ?? 'It may predate the inspector.'} />;
  }

  return (
    <TabbedLayout tabs={TABS} active={tab} onChange={setTab}>
      <div className={styles.page}>
        {tab === 'overview' && (
          <>
            <h2 className={styles.title}>{String(response.model ?? trace.modelId ?? 'Model')}</h2>
            <p className={styles.subtitle}>{formatDateTime(trace.createdAt)}</p>
            {trace.error && <div className={styles.error}>{trace.error}</div>}
            <div className={styles.stats}>
              <Stat label="Prompt tokens" value={usage.promptTokens as number} />
              <Stat label="Reused from cache" value={(timings.cache_n as number) ?? (usage.cachedTokens as number)} />
              <Stat label="Answer tokens" value={usage.completionTokens as number} />
              <Stat
                label="Tokens per second"
                value={timings.predicted_per_second ? Number(timings.predicted_per_second).toFixed(1) : null}
              />
              <Stat
                label="Prompt tokens per second"
                value={timings.prompt_per_second ? Math.round(Number(timings.prompt_per_second)) : null}
              />
              <Stat label="Total time" value={trace.durationMs ? `${(trace.durationMs / 1000).toFixed(2)}s` : null} />
              <Stat label="Finish reason" value={response.finishReason as string} />
              <Stat label="Messages sent" value={view.messages.length} />
              <Stat label="Tools offered" value={view.tools.length} />
            </div>
            <h3 className={styles.section}>Sampling</h3>
            <Pre text={JSON.stringify(view.params, null, 2)} />
          </>
        )}
        {tab === 'prompt' && (
          <>
            <h3 className={styles.section}>System prompt</h3>
            <Pre text={view.system || '(none)'} />
            <h3 className={styles.section}>Conversation as sent</h3>
            <div className={styles.messages}>
              {view.messages.map((m, i) => (
                <div key={i} className={styles.message}>
                  <div className={styles.role}>{m.role}</div>
                  {m.text && <div className={cx(styles.messageText, 'selectable')}>{m.text}</div>}
                  {m.extra && <pre className={cx(styles.extra, 'selectable')}>{m.extra}</pre>}
                </div>
              ))}
            </div>
          </>
        )}
        {tab === 'tools' && (
          <>
            {view.tools.length === 0 && <p className={styles.subtitle}>No tools were offered in this request.</p>}
            {view.tools.map((t) => (
              <div key={t.name} className={styles.tool}>
                <div className={styles.toolName}>{t.name}</div>
                <p className={styles.toolDesc}>{t.description}</p>
                <Pre text={JSON.stringify(t.parameters, null, 2)} />
              </div>
            ))}
          </>
        )}
        {tab === 'response' && (
          <>
            {typeof response.reasoning === 'string' && response.reasoning && (
              <>
                <h3 className={styles.section}>Reasoning</h3>
                <Pre text={response.reasoning} />
              </>
            )}
            <h3 className={styles.section}>Answer</h3>
            <Pre text={String(response.content ?? '') || '(empty)'} />
            {Array.isArray(response.toolCalls) && response.toolCalls.length > 0 && (
              <>
                <h3 className={styles.section}>Tool calls</h3>
                <Pre text={JSON.stringify(response.toolCalls, null, 2)} />
              </>
            )}
          </>
        )}
        {tab === 'raw' && (
          <>
            <h3 className={styles.section}>Request</h3>
            <Pre text={JSON.stringify(trace.request, null, 2)} />
            <h3 className={styles.section}>Response</h3>
            <Pre text={JSON.stringify(trace.response, null, 2)} />
          </>
        )}
      </div>
    </TabbedLayout>
  );
}
