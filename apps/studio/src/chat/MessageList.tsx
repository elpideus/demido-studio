import { memo, useEffect, useMemo, useState } from 'react';
import {
  AlertCircle,
  Copy,
  Check,
  ChevronDown,
  FileSearch,
  FoldVertical,
  Pencil,
  RefreshCw,
  Route,
  ScrollText,
} from 'lucide-react';
import { Avatar, Button, IconButton, Spinner, TextArea, Tooltip, cx } from '@demido/ui';

import { fileUrl, formatTokens } from '@/lib/format';
import type { Message, ModelEntry } from '@/lib/types';
import { useChats, type Retry } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { useWindows } from '@/stores/windows';
import { MessageAttachments } from './Attachments';
import { FileBundle } from './FileBundle';
import { OpenStateProvider, bundleKey } from './openState';
import { routedPicks } from './routed';
import { Markdown } from './Markdown';
import { summaryLine } from './slashView';
import { thoughtSeconds, turnBlocks, type Step } from './steps';
import { ThinkingBlock } from './ThinkingBlock';
import { ToolCard } from './ToolCard';
import styles from './MessageList.module.css';

type Turn =
  | { kind: 'user'; message: Message }
  | { kind: 'summary'; message: Message }
  | { kind: 'assistant'; id: string; messages: Message[] };

/**
 * Consecutive assistant and tool messages after a user message form one visual turn; a summary
 * stands between turns.
 */
export function groupTurns(messages: Message[]): Turn[] {
  const turns: Turn[] = [];
  for (const m of messages) {
    if (m.role === 'user' || m.role === 'summary') {
      turns.push({ kind: m.role, message: m });
      continue;
    }
    const last = turns[turns.length - 1];
    if (last && last.kind === 'assistant') last.messages.push(m);
    else turns.push({ kind: 'assistant', id: m.id, messages: [m] });
  }
  return turns;
}

function CopyIcon({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <IconButton
      icon={copied ? Check : Copy}
      label={copied ? 'Copied' : 'Copy'}
      size="xs"
      onClick={() =>
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        })
      }
    />
  );
}

const UserMessage = memo(function UserMessage({
  message,
  canEdit,
  onEdit,
}: {
  message: Message;
  canEdit: boolean;
  onEdit: (text: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(message.content);
  const [showSent, setShowSent] = useState(false);
  const command = message.command;
  const typed = command ? `/${command.name}${command.args ? ` ${command.args}` : ''}` : message.content;
  // An edit keeps the message's files, so it may go without text.
  const files = message.attachments ?? [];
  if (editing) {
    return (
      <div className={styles.userRow}>
        {files.length > 0 && <MessageAttachments attachments={files} />}
        <div className={styles.editBox}>
          <TextArea autoSize={{ min: 2, max: 14 }} value={draft} onChange={(e) => setDraft(e.target.value)} autoFocus />
          <div className={styles.editActions}>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              Cancel
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={!draft.trim() && files.length === 0}
              onClick={() => {
                setEditing(false);
                onEdit(draft);
              }}
            >
              Send
            </Button>
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className={styles.userRow}>
      {files.length > 0 && <MessageAttachments attachments={files} />}
      {command ? (
        <div className={cx(styles.userBubble, 'selectable')}>
          <span className={styles.commandName}>/{command.name}</span>
          {command.args && ` ${command.args}`}
        </div>
      ) : (
        message.content && <div className={cx(styles.userBubble, 'selectable')}>{message.content}</div>
      )}
      {command && showSent && <div className={cx(styles.sentText, 'selectable')}>{message.content}</div>}
      <div className={styles.userActions}>
        {typed && <CopyIcon text={typed} />}
        {command && (
          <IconButton
            icon={ScrollText}
            label={showSent ? 'Hide the message it sent' : 'Show the message it sent'}
            size="xs"
            onClick={() => setShowSent(!showSent)}
          />
        )}
        {canEdit && (
          <IconButton
            icon={Pencil}
            label="Edit"
            size="xs"
            onClick={() => {
              setDraft(message.content);
              setEditing(true);
            }}
          />
        )}
      </div>
    </div>
  );
});

/** Where the chat was compacted: the summary that stands in for what came before it. */
const SummaryDivider = memo(function SummaryDivider({
  message,
  workspace,
}: {
  message: Message;
  workspace: string | null;
}) {
  const [open, setOpen] = useState(false);
  const openWindow = useWindows((s) => s.open);
  const { label, detail } = summaryLine(message);
  const live = message.status === 'streaming';
  const retry = useChats((s) => s.retrying[message.id]);
  const failed = message.status === 'error';
  const icon = live ? (
    <Spinner size={12} />
  ) : failed ? (
    <AlertCircle size={14} aria-hidden />
  ) : (
    <FoldVertical size={14} aria-hidden />
  );
  return (
    <div className={styles.summary}>
      <div className={styles.summaryRule}>
        <Tooltip content={detail} placement="top" disabled={!detail || failed} className={styles.summaryTip}>
          <button
            type="button"
            className={cx(styles.summaryLabel, failed && styles.summaryFailed)}
            onClick={() => setOpen(!open)}
            disabled={!message.content}
            aria-expanded={open}
          >
            {icon}
            <span>{label}</span>
            {message.content && (
              <ChevronDown size={13} className={cx(styles.chevron, open && styles.chevronOpen)} aria-hidden />
            )}
          </button>
        </Tooltip>
      </div>
      {failed && detail && <div className={styles.summaryError}>{detail}</div>}
      {live && retry && <RetryNotice retry={retry} className={styles.summaryRetry} />}
      {open && message.content && (
        <div className={styles.summaryBody}>
          <Markdown text={message.content} workspace={workspace} className={cx(live && styles.streamingText)} />
          {!live && (
            <div className={styles.summaryActions}>
              <CopyIcon text={message.content} />
              <IconButton
                icon={FileSearch}
                label="Inspect what was sent and received"
                size="xs"
                onClick={() => openWindow('inspector', { messageId: message.id, title: 'Inspector · Summary' })}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
});

function Stats({ message }: { message: Message }) {
  const s = message.stats;
  if (!s) return null;
  const parts: string[] = [];
  if (s.tokensPerSecond) parts.push(`${s.tokensPerSecond.toFixed(1)} tok/s`);
  if (s.completionTokens) parts.push(`${formatTokens(s.completionTokens)} tokens`);
  if (s.durationMs) parts.push(`${(s.durationMs / 1000).toFixed(1)}s`);
  const detail = [
    s.promptTokens !== undefined && `Prompt: ${s.promptTokens} tokens`,
    s.cachedTokens ? `Reused from cache: ${s.cachedTokens}` : null,
    s.promptPerSecond ? `Prompt speed: ${s.promptPerSecond} tok/s` : null,
    s.ttftMs ? `First token after ${(s.ttftMs / 1000).toFixed(2)}s` : null,
    s.retries ? `The model was busy: tried ${s.retries + 1} times` : null,
    s.finishReason && `Finish: ${s.finishReason}`,
  ]
    .filter(Boolean)
    .join(' · ');
  return (
    <Tooltip content={detail || 'Generation stats'} placement="top">
      <span className={styles.stats}>{parts.join(' · ')}</span>
    </Tooltip>
  );
}

/** The models a router (OpenRouter's Free Models Router, say) picked to write this turn, behind an
 * icon beside the stats. */
function RoutedModels({ messages, router }: { messages: Message[]; router: string }) {
  const models = useModels((s) => s.models);
  const picks = useMemo(() => routedPicks(messages, models), [messages, models]);
  if (picks.length === 0) return null;
  const steps = picks.reduce((n, p) => n + p.steps, 0);
  const content = (
    <div className={styles.routed}>
      <div className={styles.routedTitle}>
        {router} picked {picks.length === 1 ? 'this model' : `${picks.length} models`}
      </div>
      {picks.map((p) => {
        const meta = [
          p.provider && `via ${p.provider}`,
          steps > 1 && `${p.steps} of ${steps} steps`,
        ].filter(Boolean);
        return (
          <div key={`${p.model} ${p.provider}`} className={styles.routedPick}>
            <span className={styles.routedName}>{p.name ?? p.model}</span>
            {p.name && <span className={styles.routedId}>{p.model}</span>}
            {meta.length > 0 && <span className={styles.routedMeta}>{meta.join(' · ')}</span>}
          </div>
        );
      })}
    </div>
  );
  return (
    <Tooltip content={content} placement="top" delay={150}>
      <span
        className={styles.routedIcon}
        tabIndex={0}
        aria-label={`${router} picked ${picks.map((p) => p.name ?? p.model).join(', ')}`}
      >
        <Route size={13} aria-hidden />
      </span>
    </Tooltip>
  );
}

/** A busy model's answer about to be asked for again: why, and a countdown to it. */
function RetryNotice({ retry, className }: { retry: Retry; className?: string }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, [retry]);
  const seconds = Math.ceil((retry.at - now) / 1000);
  const when = seconds > 0 ? `Trying again in ${seconds}s` : 'Trying again';
  return (
    <div className={cx(styles.retrying, className)} role="status">
      <Spinner size={12} />
      <div className={styles.retryText}>
        <span className={styles.retryReason} title={retry.reason}>
          {retry.reason}
        </span>
        <span>
          {when} · retry {retry.attempt} of {retry.attempts}
        </span>
      </div>
    </div>
  );
}

/** One model step: its reasoning, text, tool calls and how it ended. */
function AssistantStep({
  step: { message: m, calls },
  pendingTool,
  workspace,
}: {
  step: Step;
  /** The tool this step is still writing a call to, if any. */
  pendingTool: string | undefined;
  workspace: string | null;
}) {
  const streaming = m.status === 'streaming';
  const retry = useChats((s) => (streaming ? s.retrying[m.id] : undefined));
  const nothingYet = streaming && !m.content && !m.reasoning;
  const reasoningLive = streaming && !m.content && m.toolCalls.length === 0;
  return (
    <div className={styles.step}>
      {m.reasoning && (
        <ThinkingBlock messageId={m.id} text={m.reasoning} live={reasoningLive} seconds={thoughtSeconds(m)} />
      )}
      {retry && <RetryNotice retry={retry} />}
      {nothingYet && !pendingTool && !retry && (
        <div className={styles.waiting}>
          <span className={styles.dot} />
          <span className={styles.dot} />
          <span className={styles.dot} />
        </div>
      )}
      {m.content && (
        <Markdown text={m.content} workspace={workspace} className={cx(streaming && styles.streamingText)} />
      )}
      {calls.map((c) => (
        <ToolCard key={c.call.id} messageId={m.id} call={c.call} result={c.result} orphaned={c.orphaned} />
      ))}
      {streaming && pendingTool && m.toolCalls.length === 0 && (
        <div className={styles.preparing}>Preparing {pendingTool.replace(/_/g, ' ')}…</div>
      )}
      {m.status === 'error' && (
        <div className={styles.error}>
          <AlertCircle size={15} aria-hidden />
          <span>{m.error ?? 'Something went wrong.'}</span>
        </div>
      )}
      {m.status === 'cancelled' && <div className={styles.stopped}>Stopped</div>}
    </div>
  );
}

const AssistantTurn = memo(function AssistantTurn({
  messages,
  model,
  isLast,
  running,
  pendingTool,
  workspace,
  onRegenerate,
}: {
  messages: Message[];
  model: ModelEntry | undefined;
  isLast: boolean;
  running: boolean;
  pendingTool: Record<string, string>;
  workspace: string | null;
  onRegenerate: () => void;
}) {
  const open = useWindows((s) => s.open);
  const assistants = messages.filter((m) => m.role === 'assistant');
  const lastAssistant = assistants[assistants.length - 1];
  const finalText = assistants
    .map((m) => m.content)
    .filter(Boolean)
    .join('\n\n');
  const name = model?.name ?? lastAssistant?.stats?.model ?? 'Assistant';
  const liveTurn = isLast && running;
  const blocks = useMemo(() => turnBlocks(messages, liveTurn), [messages, liveTurn]);

  return (
    <div className={styles.assistant}>
      <div className={styles.assistantHeader}>
        <Avatar name={name} src={fileUrl(model?.avatarPath)} size={24} />
        <span className={styles.modelName}>{name}</span>
      </div>
      <div className={styles.assistantBody}>
        <OpenStateProvider>
          {blocks.map((block) =>
            block.kind === 'files' ? (
              <FileBundle key={bundleKey(block.id)} id={block.id} steps={block.steps} workspace={workspace} />
            ) : (
              <AssistantStep
                key={block.step.message.id}
                step={block.step}
                pendingTool={pendingTool[block.step.message.id]}
                workspace={workspace}
              />
            ),
          )}
        </OpenStateProvider>
      </div>
      {!liveTurn && lastAssistant && (
        <div className={styles.footer}>
          {finalText && <CopyIcon text={finalText} />}
          {isLast && <IconButton icon={RefreshCw} label="Regenerate" size="xs" onClick={onRegenerate} />}
          <IconButton
            icon={FileSearch}
            label="Inspect what was sent and received"
            size="xs"
            onClick={() => open('inspector', { messageId: lastAssistant.id, title: `Inspector · ${name}` })}
          />
          <Stats message={lastAssistant} />
          <RoutedModels messages={assistants} router={name} />
        </div>
      )}
    </div>
  );
});

interface Props {
  chatId: string;
  messages: Message[];
  workspace: string | null;
  sendModelId: string | undefined;
}

export function MessageList({ chatId, messages, workspace, sendModelId }: Props) {
  const models = useModels((s) => s.models);
  const running = useChats((s) => !!s.running[chatId]);
  const pendingTool = useChats((s) => s.pendingTool);
  const regenerate = useChats((s) => s.regenerate);
  const edit = useChats((s) => s.edit);
  const turns = useMemo(() => groupTurns(messages), [messages]);

  return (
    <div className={styles.list}>
      {turns.map((turn, i) =>
        turn.kind === 'summary' ? (
          <SummaryDivider key={turn.message.id} message={turn.message} workspace={workspace} />
        ) : turn.kind === 'user' ? (
          <UserMessage
            key={turn.message.id}
            message={turn.message}
            canEdit={!running && !!sendModelId}
            onEdit={(text) => sendModelId && void edit(turn.message.id, text, sendModelId)}
          />
        ) : (
          <AssistantTurn
            key={turn.id}
            messages={turn.messages}
            model={models.find((m) => m.id === turn.messages.find((x) => x.modelId)?.modelId)}
            isLast={i === turns.length - 1}
            running={running}
            pendingTool={pendingTool}
            workspace={workspace}
            onRegenerate={() => sendModelId && void regenerate(sendModelId)}
          />
        ),
      )}
    </div>
  );
}
