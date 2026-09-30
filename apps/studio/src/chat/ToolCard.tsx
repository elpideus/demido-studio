import { useState } from 'react';
import {
  Check,
  ChevronRight,
  CircleSlash,
  Code2,
  Database,
  Download,
  FileText,
  LineChart,
  Search,
  ShieldQuestion,
  Sparkles,
  Terminal,
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
  list_files: FileText,
  read_file: FileText,
  write_file: FileText,
  create_skill: Sparkles,
  read_skill_file: Sparkles,
};

function Approval({ message, call }: { message: Message; call: ToolCall }) {
  const [busy, setBusy] = useState(false);
  const args = (message.toolResult?.args ?? {}) as Record<string, unknown>;
  const code =
    typeof args.code === 'string' ? args.code : typeof args.file === 'string' ? `# runs ${args.file}` : call.arguments;
  const decide = async (decision: 'once' | 'always' | 'deny') => {
    setBusy(true);
    await api.resolveApproval(message.id, decision);
  };
  return (
    <div className={styles.approval}>
      <div className={styles.approvalTitle}>
        <ShieldQuestion size={16} aria-hidden />
        The assistant wants to run this on your computer
      </div>
      <pre className={cx(styles.code, 'selectable')}>{code}</pre>
      <div className={styles.approvalActions}>
        <Button size="sm" variant="primary" disabled={busy} onClick={() => void decide('once')}>
          Run once
        </Button>
        <Button size="sm" variant="secondary" disabled={busy} onClick={() => void decide('always')}>
          Always allow
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void decide('deny')}>
          Don’t run
        </Button>
      </div>
    </div>
  );
}

/** A download estimated to take longer than the person's limit: download it, or not. It is 1-minute
 *  candles, which every timeframe is built from, so there is no smaller choice to offer. */
function DownloadApproval({ message, card }: { message: Message; card: ToolApproval }) {
  const [busy, setBusy] = useState(false);
  // A card this UI cannot read still gets the choices, just without the estimate.
  const known = typeof card.plan?.requests === 'number';
  const decide = async (decision: ApprovalDecision) => {
    setBusy(true);
    // The tool keeps waiting if the answer did not arrive, so the choices must stay usable.
    try {
      await api.resolveApproval(message.id, decision);
    } catch (e) {
      setBusy(false);
      toast.error('Could not answer', errorText(e));
    }
  };
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
        ) : (
          <Approval message={result} call={call} />
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
