import { useState } from 'react';
import {
  Check,
  ChevronRight,
  CircleSlash,
  CloudUpload,
  Code2,
  Database,
  Download,
  FileCode,
  FilePen,
  FileSearch,
  FileText,
  FlaskConical,
  FolderCog,
  FolderInput,
  LineChart,
  Mail,
  MailCheck,
  MailOpen,
  MailSearch,
  Mails,
  MailPlus,
  MailX,
  Paperclip,
  PenLine,
  Search,
  Send,
  ShieldQuestion,
  Star,
  SquareFunction,
  Sparkles,
  SquareTerminal,
  Terminal,
  Wrench,
  X,
  type LucideIcon,
} from 'lucide-react';
import { Button, Spinner, cx, formatDuration } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { ApprovalDecision, Message, ToolApproval, ToolCall } from '@/lib/types';
import { planLine } from '@/market/DownloadProgress';
import { toast } from '@/stores/toasts';
import { callKey, useOpenState } from './openState';
import { ToolDisplay, isProminent } from './ToolDisplay';
import styles from './ToolCard.module.css';

const ICONS: Record<string, LucideIcon> = {
  market_search: Search,
  market_quote: LineChart,
  market_candles: LineChart,
  market_history: LineChart,
  market_download: Download,
  market_data_status: Database,
  run_python: Terminal,
  run_command: SquareTerminal,
  list_files: FileText,
  read_file: FileText,
  write_file: FileText,
  search_files: FileSearch,
  create_skill: Sparkles,
  read_skill_file: Sparkles,
  pine_list: FileCode,
  pine_read: FileCode,
  pine_edit: FilePen,
  pine_save: FileCode,
  pine_test: FlaskConical,
  pine_publish: CloudUpload,
  chart_add_indicator: SquareFunction,
  chart_draw: PenLine,
  mail_list: Mail,
  mail_search: MailSearch,
  mail_read: MailOpen,
  mail_export: Mails,
  mail_attachment: Paperclip,
  mail_mark_read: MailCheck,
  mail_mark: Star,
  mail_move: FolderInput,
  mail_delete: MailX,
  mail_folder: FolderCog,
  mail_send: Send,
  mail_draft: MailPlus,
  load_tools: Wrench,
};

type DownloadCard = Extract<ToolApproval, { kind: 'download' }>;
type PinePublishCard = Extract<ToolApproval, { kind: 'pinePublish' }>;
type MailMarkReadCard = Extract<ToolApproval, { kind: 'mailMarkRead' }>;
type MailActionCard = Extract<ToolApproval, { kind: 'mailAction' }>;
type MailSendCard = Extract<ToolApproval, { kind: 'mailSend' }>;

/** Tools that run code on the person's computer: their card shows the code. */
const CODE_TOOLS = new Set(['run_command', 'run_python']);

/** Answers an approval; the tool keeps waiting if the answer did not arrive, so the choices must stay usable. */
function useDecide(message: Message) {
  const [busy, setBusy] = useState(false);
  const decide = async (decision: ApprovalDecision) => {
    setBusy(true);
    try {
      await api.resolveApproval(message.id, decision);
    } catch (e) {
      setBusy(false);
      toast.error('Could not answer', errorText(e));
    }
  };
  return { busy, decide };
}

/** Any call the permission mode asks about, shown as it is: the code it runs, or what it does and
 *  its arguments. "Always allow" only where the card offers it (Balanced mode). */
function Approval({ message, call, card }: { message: Message; call: ToolCall; card?: ToolApproval }) {
  const { busy, decide } = useDecide(message);
  const args = (message.toolResult?.args ?? {}) as Record<string, unknown>;
  const isCode = CODE_TOOLS.has(call.name);
  const always = card?.always === true;
  const code =
    typeof args.command === 'string'
      ? args.command
      : typeof args.code === 'string'
        ? args.code
        : typeof args.file === 'string'
          ? `# runs ${args.file}`
          : call.arguments;
  return (
    <div className={styles.approval}>
      <div className={styles.approvalTitle}>
        <ShieldQuestion size={16} aria-hidden />
        {isCode ? 'The assistant wants to run this on your computer' : 'The assistant asks before this step'}
      </div>
      {isCode ? (
        <>
          <pre className={cx(styles.code, 'selectable')}>{code}</pre>
          {typeof args.directory === 'string' && <div className={styles.planAlt}>In {args.directory}</div>}
        </>
      ) : (
        <div className={styles.plan}>
          <div className={styles.planLine}>{message.toolResult?.label ?? call.name}</div>
          <pre className={cx(styles.code, styles.source, 'selectable')}>{formatArgs(call.arguments)}</pre>
        </div>
      )}
      <div className={styles.approvalActions}>
        <Button size="sm" variant="primary" disabled={busy} onClick={() => void decide('once')}>
          {isCode ? 'Run once' : 'Allow'}
        </Button>
        {always && (
          <Button size="sm" variant="secondary" disabled={busy} onClick={() => void decide('always')}>
            Always allow
          </Button>
        )}
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void decide('deny')}>
          {isCode ? 'Don’t run' : 'Don’t allow'}
        </Button>
      </div>
    </div>
  );
}

/** A download estimated to take longer than the person's limit: download it, or not. It is 1-minute
 *  candles, which every timeframe is built from, so there is no smaller choice to offer. */
function DownloadApproval({ message, card }: { message: Message; card: DownloadCard }) {
  const { busy, decide } = useDecide(message);
  // A card this UI cannot read still gets the choices, just without the estimate.
  const known = typeof card.plan?.requests === 'number';
  return (
    <div className={styles.approval}>
      <div className={styles.approvalTitle}>
        <Download size={16} aria-hidden />
        The assistant wants to download market data
      </div>
      <div className={styles.plan}>
        <div className={styles.planLine}>{known ? planLine(card.plan) : 'Market data for this request'}</div>
        <div className={styles.planAlt}>It runs in the background and is kept for every timeframe, chart and chat.</div>
      </div>
      <div className={styles.approvalActions}>
        <Button size="sm" variant="primary" disabled={busy} onClick={() => void decide('once')}>
          Download
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void decide('deny')}>
          Don’t download
        </Button>
      </div>
    </div>
  );
}

/** Saving a Pine script to the person's TradingView account, which changes the account: once, always, or not. */
function PinePublishApproval({ message, card }: { message: Message; card: PinePublishCard }) {
  const { busy, decide } = useDecide(message);
  const [showSource, setShowSource] = useState(false);
  return (
    <div className={styles.approval}>
      <div className={styles.approvalTitle}>
        <CloudUpload size={16} aria-hidden />
        The assistant wants to save a Pine script to your TradingView account
      </div>
      <div className={styles.plan}>
        <div className={styles.planLine}>
          {card.name} · {card.lines} lines · {card.update ? 'a new version of the script saved before' : 'a new script'}
        </div>
        <div className={styles.planAlt}>
          It stays private, under My scripts in TradingView’s Indicators menu. Nothing is published to the community.
        </div>
      </div>
      {typeof card.source === 'string' && (
        <>
          <button type="button" className={styles.linkish} onClick={() => setShowSource(!showSource)}>
            {showSource ? 'Hide the script' : 'Show the script'}
          </button>
          {showSource && <pre className={cx(styles.code, styles.source, 'selectable')}>{card.source}</pre>}
        </>
      )}
      <div className={styles.approvalActions}>
        <Button size="sm" variant="primary" disabled={busy} onClick={() => void decide('once')}>
          Save once
        </Button>
        {card.always !== false && (
          <Button size="sm" variant="secondary" disabled={busy} onClick={() => void decide('always')}>
            Always allow
          </Button>
        )}
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void decide('deny')}>
          Don’t save
        </Button>
      </div>
    </div>
  );
}

/** Marking emails as read that the person did not ask the assistant to mark: allow it, or not. */
function MailMarkReadApproval({ message, card }: { message: Message; card: MailMarkReadCard }) {
  const { busy, decide } = useDecide(message);
  const sample = Array.isArray(card.messages) ? card.messages : [];
  const count = typeof card.count === 'number' ? card.count : sample.length;
  const more = count - sample.length;
  return (
    <div className={styles.approval}>
      <div className={styles.approvalTitle}>
        <MailCheck size={16} aria-hidden />
        {count === 1
          ? 'The assistant wants to mark an email as read'
          : `The assistant wants to mark ${count} emails as read`}
      </div>
      {sample.length > 0 && (
        <div className={styles.plan}>
          {sample.map((m, i) => (
            <div key={i} className={styles.mailLine}>
              <span className={styles.mailFrom}>{m.from}</span> {m.subject || '(no subject)'}
            </div>
          ))}
          {more > 0 && <div className={styles.planAlt}>and {more} more</div>}
        </div>
      )}
      <div className={styles.approvalActions}>
        <Button size="sm" variant="primary" disabled={busy} onClick={() => void decide('once')}>
          Allow
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void decide('deny')}>
          Don’t allow
        </Button>
      </div>
    </div>
  );
}

/** The emails a card names: the first few, and how many more. */
function MailSample({ messages, count }: { messages: unknown; count: unknown }) {
  const sample = Array.isArray(messages) ? (messages as Array<{ from: string; subject: string }>) : [];
  const more = (typeof count === 'number' ? count : sample.length) - sample.length;
  if (sample.length === 0) return null;
  return (
    <div className={styles.plan}>
      {sample.map((m, i) => (
        <div key={i} className={styles.mailLine}>
          <span className={styles.mailFrom}>{m.from}</span> {m.subject || '(no subject)'}
        </div>
      ))}
      {more > 0 && <div className={styles.planAlt}>and {more} more</div>}
    </div>
  );
}

/** Changing emails or folders (marking, moving, deleting): what will happen, to which emails. */
function MailActionApproval({ message, call, card }: { message: Message; call: ToolCall; card: MailActionCard }) {
  const { busy, decide } = useDecide(message);
  const Icon = ICONS[call.name] ?? Mail;
  return (
    <div className={styles.approval}>
      <div className={styles.approvalTitle}>
        <Icon size={16} aria-hidden />
        {card.title || message.toolResult?.label || 'The assistant wants to change your email'}
      </div>
      <MailSample messages={card.messages} count={card.count} />
      {card.detail && <div className={styles.planAlt}>{card.detail}</div>}
      <div className={styles.approvalActions}>
        <Button size="sm" variant="primary" disabled={busy} onClick={() => void decide('once')}>
          {card.confirm || 'Allow'}
        </Button>
        {card.always === true && (
          <Button size="sm" variant="secondary" disabled={busy} onClick={() => void decide('always')}>
            Always allow
          </Button>
        )}
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void decide('deny')}>
          {card.deny || 'Don’t allow'}
        </Button>
      </div>
    </div>
  );
}

function fileSize(bytes: number): string {
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  if (bytes >= 1 << 10) return `${Math.ceil(bytes / (1 << 10))} KB`;
  return `${bytes} bytes`;
}

/** An email about to be sent, or saved as a draft, shown whole: who it goes to, and what it says. */
function MailSendApproval({ message, card }: { message: Message; card: MailSendCard }) {
  const { busy, decide } = useDecide(message);
  const list = (l: unknown) => (Array.isArray(l) ? (l as string[]).join(', ') : '');
  const rows: Array<[string, string]> = [
    ['From', card.from ?? ''],
    ['To', list(card.to)],
    ['Cc', list(card.cc)],
    ['Bcc', list(card.bcc)],
    ['Subject', card.subject || '(no subject)'],
  ];
  const attachments = Array.isArray(card.attachments) ? card.attachments : [];
  return (
    <div className={styles.approval}>
      <div className={styles.approvalTitle}>
        {card.draft ? <MailPlus size={16} aria-hidden /> : <Send size={16} aria-hidden />}
        {card.draft ? 'The assistant wants to save this draft' : 'The assistant wants to send this email'}
      </div>
      <div className={styles.mailHeaders}>
        {rows
          .filter(([, value]) => value)
          .map(([name, value]) => (
            <div key={name} className={styles.mailHeader}>
              <span className={styles.mailHeaderName}>{name}</span>
              <span className="selectable">{value}</span>
            </div>
          ))}
      </div>
      <pre className={cx(styles.code, styles.source, styles.mailBody, 'selectable')}>{card.body}</pre>
      {attachments.length > 0 && (
        <div className={styles.plan}>
          {attachments.map((a, i) => (
            <div key={i} className={styles.mailLine}>
              <Paperclip size={12} aria-hidden /> <span className={styles.mailFrom}>{a.name}</span>{' '}
              {typeof a.size === 'number' ? fileSize(a.size) : ''}
            </div>
          ))}
        </div>
      )}
      <div className={styles.approvalActions}>
        <Button size="sm" variant="primary" disabled={busy} onClick={() => void decide('once')}>
          {card.draft ? 'Save draft' : 'Send'}
        </Button>
        {card.always === true && (
          <Button size="sm" variant="secondary" disabled={busy} onClick={() => void decide('always')}>
            Always allow
          </Button>
        )}
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void decide('deny')}>
          {card.draft ? 'Don’t save' : 'Don’t send'}
        </Button>
      </div>
    </div>
  );
}

interface Props {
  /** The assistant message that made the call. */
  messageId: string;
  call: ToolCall;
  /** The tool message holding the call's status and result; absent until it starts. */
  result: Message | undefined;
  /** The turn ended before this call could run. */
  orphaned: boolean;
  /** Drawn as a row inside another card, without a border of its own. */
  flat?: boolean;
}

/** One tool call: what it does, whether it ran, and what came back. */
export function ToolCard({ messageId, call, result, orphaned, flat }: Props) {
  const [open, setOpen] = useOpenState(callKey(messageId, call.id));
  const Icon = ICONS[call.name] ?? Code2;
  const tr = result?.toolResult;
  const label = tr?.label ?? call.name;
  const status = result?.status;
  const awaiting = status === 'awaitingApproval';
  const running = status === 'running' || (!result && !orphaned);
  const failed = tr?.ok === false || status === 'error';
  const cancelled = status === 'cancelled' || (orphaned && !result);
  const prominent = isProminent(tr?.display);

  let StatusIcon: LucideIcon | null = null;
  if (!running && !awaiting) StatusIcon = cancelled ? CircleSlash : failed ? X : Check;

  return (
    <div className={cx(styles.card, flat && styles.flat, failed && styles.failed, awaiting && styles.awaiting)}>
      <button type="button" className={styles.header} onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className={styles.icon}>
          <Icon size={15} strokeWidth={1.8} aria-hidden />
        </span>
        <span className={styles.label}>{label}</span>
        {running && !awaiting && <Spinner size={13} />}
        {StatusIcon && (
          <StatusIcon
            size={14}
            strokeWidth={2.2}
            className={cx(
              styles.status,
              failed ? styles.statusFailed : cancelled ? styles.statusMuted : styles.statusOk,
            )}
            aria-label={failed ? 'Failed' : cancelled ? 'Stopped' : 'Done'}
          />
        )}
        {tr?.durationMs !== undefined && !running && (
          <span className={styles.duration}>{formatDuration(tr.durationMs / 1000)}</span>
        )}
        <ChevronRight size={14} className={cx(styles.chevron, open && styles.chevronOpen)} aria-hidden />
      </button>
      {awaiting &&
        result &&
        (tr?.approval?.kind === 'download' ? (
          <DownloadApproval message={result} card={tr.approval} />
        ) : tr?.approval?.kind === 'pinePublish' ? (
          <PinePublishApproval message={result} card={tr.approval} />
        ) : tr?.approval?.kind === 'mailMarkRead' ? (
          <MailMarkReadApproval message={result} card={tr.approval} />
        ) : tr?.approval?.kind === 'mailAction' ? (
          <MailActionApproval message={result} call={call} card={tr.approval} />
        ) : tr?.approval?.kind === 'mailSend' ? (
          <MailSendApproval message={result} card={tr.approval} />
        ) : (
          <Approval message={result} call={call} card={tr?.approval} />
        ))}
      {!awaiting && (prominent || open) && tr?.display && (
        <div className={styles.body}>
          <ToolDisplay display={tr.display} message={result} />
        </div>
      )}
      {!awaiting && !prominent && !open && failed && tr?.display?.error && (
        <div className={styles.body}>
          <ToolDisplay display={tr.display} message={result} />
        </div>
      )}
      {open && (
        <div className={styles.details}>
          <div className={styles.detailLabel}>Arguments</div>
          <pre className={cx(styles.code, 'selectable')}>{formatArgs(call.arguments)}</pre>
          {result?.content && (
            <>
              <div className={styles.detailLabel}>Sent back to the model</div>
              <pre className={cx(styles.code, 'selectable')}>{formatArgs(result.content)}</pre>
            </>
          )}
        </div>
      )}
    </div>
  );
}

function formatArgs(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}
