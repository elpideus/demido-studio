import { memo, useMemo, useState } from 'react';
import { AlertCircle, Copy, Check, FileSearch, Pencil, RefreshCw } from 'lucide-react';
import { Avatar, Button, IconButton, TextArea, Tooltip, cx } from '@demido/ui';

import { fileUrl, formatTokens } from '@/lib/format';
import type { Message, ModelEntry } from '@/lib/types';
import { useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { useWindows } from '@/stores/windows';
import { FileBundle } from './FileBundle';
import { OpenStateProvider, bundleKey } from './openState';
import { Markdown } from './Markdown';
import { thoughtSeconds, turnBlocks, type Step } from './steps';
import { ThinkingBlock } from './ThinkingBlock';
import { ToolCard } from './ToolCard';
import styles from './MessageList.module.css';

type Turn = { kind: 'user'; message: Message } | { kind: 'assistant'; id: string; messages: Message[] };

/** Consecutive assistant and tool messages after a user message form one visual turn. */
export function groupTurns(messages: Message[]): Turn[] {
  const turns: Turn[] = [];
  for (const m of messages) {
    if (m.role === 'user') {
      turns.push({ kind: 'user', message: m });
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
  if (editing) {
    return (
      <div className={styles.userRow}>
        <div className={styles.editBox}>
          <TextArea autoSize={{ min: 2, max: 14 }} value={draft} onChange={(e) => setDraft(e.target.value)} autoFocus />
          <div className={styles.editActions}>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              Cancel
            </Button>
            <Button
              size="sm"
              variant="primary"
              disabled={!draft.trim()}
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
      <div className={cx(styles.userBubble, 'selectable')}>{message.content}</div>
      <div className={styles.userActions}>
        <CopyIcon text={message.content} />
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
  const nothingYet = streaming && !m.content && !m.reasoning;
  const reasoningLive = streaming && !m.content && m.toolCalls.length === 0;
  return (
    <div className={styles.step}>
      {m.reasoning && (
        <ThinkingBlock messageId={m.id} text={m.reasoning} live={reasoningLive} seconds={thoughtSeconds(m)} />
      )}
      {nothingYet && !pendingTool && (
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
        <ToolCard key={c.call.id} call={c.call} result={c.result} orphaned={c.orphaned} />
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
              <FileBundle key={bundleKey(block.id)} id={block.id} steps={block.steps} />
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
        turn.kind === 'user' ? (
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
