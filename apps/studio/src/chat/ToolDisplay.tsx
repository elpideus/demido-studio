// How each tool's result looks in the chat. The `display` object comes from the backend tool;
// its `kind` picks the renderer here.

import { useLayoutEffect, useRef, useState } from 'react';
import {
  ExternalLink,
  FileImage,
  FileSpreadsheet,
  FileText,
  FolderOpen,
  Sparkles,
  CircleStop,
  File as FileIcon,
} from 'lucide-react';
import { Button, cx, formatBytes } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { fileUrl, formatPercent, formatPrice } from '@/lib/format';
import type { Message, Quote, ToolDisplay as Display } from '@/lib/types';
import { DownloadProgress } from '@/market/DownloadProgress';
import { currentModel, useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { useWindows } from '@/stores/windows';
import { DataStatus } from './DataStatus';
import { downloadView } from './downloadView';
import styles from './ToolDisplay.module.css';

const str = (v: unknown) => (typeof v === 'string' ? v : '');
const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : undefined);

function openFile(path: string, reveal = false) {
  (reveal ? api.revealPath(path) : api.openPath(path)).catch((e) =>
    toast.error('Could not open the file', errorText(e)),
  );
}

export function FileChip({ path, name, size, kind }: { path: string; name: string; size?: number; kind?: string }) {
  const Icon =
    kind === 'image' ? FileImage : kind === 'table' ? FileSpreadsheet : kind === 'text' ? FileText : FileIcon;
  return (
    <span className={styles.chip}>
      <button type="button" className={styles.chipMain} onClick={() => openFile(path)} title={`Open ${name}`}>
        <Icon size={14} strokeWidth={1.8} aria-hidden />
        <span className={styles.chipName}>{name}</span>
        {size !== undefined && <span className={styles.chipSize}>{formatBytes(size)}</span>}
      </button>
      <button type="button" className={styles.chipReveal} onClick={() => openFile(path, true)} title="Show in folder">
        <FolderOpen size={13} strokeWidth={1.8} aria-hidden />
      </button>
    </span>
  );
}

/** A small line chart of closes (data drawing, not an icon). */
export function Sparkline({ values, width = 220, height = 44 }: { values: number[]; width?: number; height?: number }) {
  const clean = values.filter((v) => Number.isFinite(v));
  if (clean.length < 2) return null;
  const min = Math.min(...clean);
  const max = Math.max(...clean);
  const span = max - min || 1;
  const points = clean
    .map(
      (v, i) =>
        `${((i / (clean.length - 1)) * width).toFixed(1)},${(height - 3 - ((v - min) / span) * (height - 6)).toFixed(1)}`,
    )
    .join(' ');
  const up = clean[clean.length - 1]! >= clean[0]!;
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} className={styles.spark} aria-hidden>
      <polyline
        points={points}
        fill="none"
        stroke={up ? 'var(--market-up)' : 'var(--market-down)'}
        strokeWidth="1.6"
        strokeLinejoin="round"
      />
    </svg>
  );
}

const CONTINUE_TEXT = 'The download finished — please continue with my request.';

/**
 * "Continue" for a finished download: sends the follow-up in the chat the card is in, while that
 * chat is on screen, idle, and nothing was said after this call.
 */
function useContinue(message: Message | undefined): (() => void) | undefined {
  const chatId = message?.chatId ?? null;
  const seq = message?.seq ?? Infinity;
  const onScreen = useChats((s) => chatId !== null && s.activeId === chatId);
  const running = useChats((s) => chatId !== null && !!s.running[chatId]);
  const answered = useChats((s) =>
    chatId === null ? true : (s.messages[chatId] ?? []).some((m) => m.role === 'user' && m.seq > seq),
  );
  const chat = useChats((s) => s.chats.find((c) => c.id === chatId));
  const picked = useChats((s) => s.pickedModelId);
  const send = useChats((s) => s.send);
  const models = useModels((s) => s.models);
  const model = currentModel(models, chat, picked);
  if (!onScreen || running || answered || !model) return undefined;
  return () => void send(CONTINUE_TEXT, model.id);
}

/** A download a tool started: its live progress, and why it stopped if it failed. */
function DownloadBlock({ d, message }: { d: Display; message: Message | undefined }) {
  const onContinue = useContinue(message);
  const view = downloadView(d);
  const jobError = str(d.jobError);
  return (
    <div className={styles.download}>
      {view.kind === 'progress' && (
        <DownloadProgress jobId={view.jobId} plan={view.plan} onContinue={view.continuable ? onContinue : undefined} />
      )}
      {view.kind === 'note' && <div className={styles.muted}>{view.text}</div>}
      {jobError && <div className={styles.error}>{jobError}</div>}
    </div>
  );
}

/** Rows per source, from the read's spans (or an older result's per-source counts). */
function sourceCounts(d: Display): Array<{ source: string; count: number }> {
  if (Array.isArray(d.sources)) return d.sources as Array<{ source: string; count: number }>;
  if (!Array.isArray(d.spans)) return [];
  const counts = new Map<string, number>();
  for (const span of d.spans as Array<{ source: string; count: number }>) {
    counts.set(span.source, (counts.get(span.source) ?? 0) + span.count);
  }
  return [...counts].map(([source, count]) => ({ source, count }));
}

function Candles({ d, message }: { d: Display; message: Message | undefined }) {
  const change = num(d.changePercent);
  const sources = sourceCounts(d);
  const path = str(d.path);
  const file = str(d.file);
  const download = d.download && typeof d.download === 'object' ? (d.download as Display) : null;
  return (
    <div className={styles.candles}>
      <div className={styles.candlesHead}>
        <div>
          <div className={styles.symbol}>
            {str(d.symbol)} <span className={styles.muted}>· {str(d.timeframe)}</span>
          </div>
          <div className={styles.muted}>{str(d.description)}</div>
        </div>
        <div className={styles.priceBlock}>
          <div className={styles.price}>{formatPrice(num(d.lastClose))}</div>
          <div className={cx(styles.change, (change ?? 0) >= 0 ? styles.up : styles.down)}>{formatPercent(change)}</div>
        </div>
      </div>
      <Sparkline values={Array.isArray(d.closes) ? (d.closes as number[]) : []} width={420} height={56} />
      <div className={styles.meta}>
        <span>{num(d.count)?.toLocaleString()} candles</span>
        <span>
          {str(d.from).slice(0, 10)} → {str(d.to).slice(0, 10)}
        </span>
        {sources.map((s) => (
          <span key={s.source} className={styles.source}>
            {s.source === 'tradingview' ? 'TradingView' : 'Dukascopy'} · {num(s.count)?.toLocaleString()}
          </span>
        ))}
      </div>
      {path && <FileChip path={path} name={file.split('/').pop() ?? file} kind="table" />}
      {str(d.note) ? (
        <div className={styles.muted}>{str(d.note)}</div>
      ) : (
        d.denied === true && (
          <div className={styles.muted}>You chose not to download more, so this is only what was already stored.</div>
        )
      )}
      {download ? (
        <DownloadBlock d={{ jobError: d.jobError, ...download }} message={message} />
      ) : (
        str(d.jobError) && <div className={styles.error}>{str(d.jobError)}</div>
      )}
    </div>
  );
}

function Quotes({ d }: { d: Display }) {
  const quotes = (Array.isArray(d.quotes) ? d.quotes : []) as Quote[];
  return (
    <div className={styles.table}>
      {quotes.map((q) => (
        <div key={q.symbol} className={styles.quoteRow}>
          <div className={styles.quoteName}>
            <span className={styles.symbol}>{q.symbol}</span>
            <span className={styles.muted}>{q.error ?? q.description}</span>
          </div>
          {!q.error && (
            <>
              <span className={styles.price}>{formatPrice(q.price)}</span>
              <span className={cx(styles.change, (q.changePercent ?? 0) >= 0 ? styles.up : styles.down)}>
                {formatPercent(q.changePercent)}
              </span>
            </>
          )}
        </div>
      ))}
    </div>
  );
}

function Search({ d }: { d: Display }) {
  const results = (Array.isArray(d.results) ? d.results : []) as Array<Record<string, string>>;
  if (!results.length) return <div className={styles.muted}>No symbols matched.</div>;
  return (
    <div className={styles.table}>
      {results.slice(0, 8).map((r) => (
        <div key={r.symbol} className={styles.quoteRow}>
          <div className={styles.quoteName}>
            <span className={styles.symbol}>{r.symbol}</span>
            <span className={styles.muted}>{r.description}</span>
          </div>
          <span className={styles.muted}>{r.type}</span>
        </div>
      ))}
    </div>
  );
}

/** Where search_files found passages: one line per passage, file and page. */
function Passages({ d }: { d: Display }) {
  const hits = (Array.isArray(d.hits) ? d.hits : []) as Array<{ file: string; page: number | null }>;
  if (!hits.length) return <div className={styles.muted}>No passage matched.</div>;
  return (
    <div className={styles.table}>
      {hits.map((h, i) => (
        <div key={`${h.file}-${h.page}-${i}`} className={styles.quoteRow}>
          <span className={styles.symbol}>{h.file}</span>
          {h.page !== null && <span className={styles.muted}>page {h.page}</span>}
        </div>
      ))}
    </div>
  );
}

function Python({ d }: { d: Display }) {
  const [showCode, setShowCode] = useState(false);
  const files = (Array.isArray(d.files) ? d.files : []) as Array<{
    path: string;
    absolute: string;
    size: number;
    kind: string;
  }>;
  const images = files.filter((f) => f.kind === 'image');
  const others = files.filter((f) => f.kind !== 'image');
  const stdout = str(d.stdout).trim();
  const stderr = str(d.stderr).trim();
  const code = str(d.code);
  const failed = d.timedOut === true || (typeof d.exitCode === 'number' && d.exitCode !== 0);
  return (
    <div className={styles.python}>
      {code && (
        <button type="button" className={styles.linkish} onClick={() => setShowCode(!showCode)}>
          {showCode ? 'Hide code' : 'Show code'}
        </button>
      )}
      {showCode && <pre className={cx(styles.output, 'selectable')}>{code}</pre>}
      {stdout && <pre className={cx(styles.output, 'selectable')}>{stdout}</pre>}
      {stderr && <pre className={cx(styles.output, styles.stderr, 'selectable')}>{stderr}</pre>}
      {failed && !stderr && (
        <div className={styles.error}>
          {d.timedOut ? 'The script ran too long and was stopped.' : `Exited with code ${String(d.exitCode)}`}
        </div>
      )}
      {images.map((f) => (
        <button
          key={f.absolute}
          type="button"
          className={styles.imageButton}
          onClick={() => openFile(f.absolute)}
          title={f.path}
        >
          <img src={fileUrl(f.absolute) ?? ''} alt={f.path} className={styles.image} />
        </button>
      ))}
      {others.length > 0 && (
        <div className={styles.chips}>
          {others.map((f) => (
            <FileChip key={f.absolute} path={f.absolute} name={f.path} size={f.size} kind={f.kind} />
          ))}
        </div>
      )}
    </div>
  );
}

/** How a command ended, when that needs saying. */
function commandEnding(d: Display): { text: string; failed: boolean } | null {
  if (d.ending === 'timedOut') return { text: `Stopped at its time limit of ${num(d.timeout)} s`, failed: false };
  if (d.ending === 'stopped') return { text: 'You stopped it', failed: false };
  const code = num(d.exitCode);
  return code !== undefined && code !== 0 ? { text: `Exited with code ${code}`, failed: true } : null;
}

/** A command in the terminal: its prompt line and what it printed, live while it runs. */
function Command({ d, message }: { d: Display; message: Message | undefined }) {
  const output = str(d.output);
  const running = d.running === true;
  const screen = useRef<HTMLPreElement>(null);
  // Follows the newest output, unless the person scrolled up to read.
  const follow = useRef(true);
  const [stopping, setStopping] = useState(false);
  useLayoutEffect(() => {
    const el = screen.current;
    if (el && follow.current) el.scrollTop = el.scrollHeight;
  }, [output]);
  const files = (Array.isArray(d.files) ? d.files : []) as Array<{
    path: string;
    absolute: string;
    size: number;
    kind: string;
  }>;
  const ending = commandEnding(d);
  const stop = () => {
    if (!message) return;
    setStopping(true);
    api.stopTool(message.id).catch((e) => {
      setStopping(false);
      toast.error('Could not stop the command', errorText(e));
    });
  };
  return (
    <div className={styles.command}>
      <pre
        ref={screen}
        className={cx(styles.terminal, d.fullScreen === true && styles.fullScreen, 'selectable')}
        onScroll={(e) => {
          const el = e.currentTarget;
          follow.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        }}
      >
        <span className={styles.prompt}>{d.powershell === true ? `PS ${str(d.directory)}>` : '$'}</span>{' '}
        <span className={styles.commandLine}>{str(d.command)}</span>
        {output && `\n${output}`}
        {running && <span className={styles.cursor} aria-hidden />}
      </pre>
      <div className={styles.commandFoot}>
        <span className={styles.shell}>{str(d.shell)}</span>
        {ending && <span className={ending.failed ? styles.error : styles.muted}>{ending.text}</span>}
        {running && message && (
          <Button
            size="sm"
            variant="ghost"
            icon={CircleStop}
            disabled={stopping}
            onClick={stop}
            className={styles.stop}
          >
            Stop
          </Button>
        )}
      </div>
      {files.length > 0 && (
        <div className={styles.chips}>
          {files.map((f) => (
            <FileChip key={f.absolute} path={f.absolute} name={f.path} size={f.size} kind={f.kind} />
          ))}
        </div>
      )}
    </div>
  );
}

function Skill({ d }: { d: Display }) {
  const open = useWindows((s) => s.open);
  return (
    <div className={styles.skill}>
      <Sparkles size={16} className={styles.skillIcon} aria-hidden />
      <div className={styles.skillText}>
        <div className={styles.symbol}>{str(d.name)}</div>
        <div className={styles.muted}>{str(d.description)}</div>
      </div>
      <Button
        size="sm"
        variant="secondary"
        iconRight={ExternalLink}
        onClick={() => open('settings', { tab: 'skills', skill: str(d.id) })}
      >
        Open
      </Button>
    </div>
  );
}

function Files({ d }: { d: Display }) {
  const files = (Array.isArray(d.files) ? d.files : []) as Array<{ path: string; size: number; kind: string }>;
  if (!files.length) return <div className={styles.muted}>The workspace is empty.</div>;
  return (
    <div className={styles.fileList}>
      {files.slice(0, 30).map((f) => (
        <div key={f.path} className={styles.fileRow}>
          <span>{f.path}</span>
          <span className={styles.muted}>{formatBytes(f.size)}</span>
        </div>
      ))}
    </div>
  );
}

/**
 * The result area of a tool card. Returns null when there is nothing worth showing. `message` is
 * the tool message the display belongs to, for actions that answer in its chat.
 */
export function ToolDisplay({ display, message }: { display: Display | undefined; message?: Message }) {
  if (!display) return null;
  // A declined download still has cached candles or a job worth showing.
  if (display.denied && display.kind !== 'candles' && display.kind !== 'download') {
    return <div className={styles.muted}>You declined this.</div>;
  }
  if (display.error) return <div className={styles.error}>{String(display.error)}</div>;
  switch (display.kind) {
    case 'candles':
      return <Candles d={display} message={message} />;
    case 'download':
      return <DownloadBlock d={display} message={message} />;
    case 'dataStatus':
      return <DataStatus d={display} />;
    case 'quotes':
      return <Quotes d={display} />;
    case 'search':
      return <Search d={display} />;
    case 'passages':
      return <Passages d={display} />;
    case 'python':
      return <Python d={display} />;
    case 'command':
      return <Command d={display} message={message} />;
    case 'skill':
      return <Skill d={display} />;
    case 'files':
      return <Files d={display} />;
    case 'file':
      return display.absolute ? (
        <FileChip path={str(display.absolute)} name={str(display.path)} size={num(display.size)} kind="text" />
      ) : (
        <div className={styles.muted}>
          {str(display.path)} · {num(display.lines)} lines
        </div>
      );
    default:
      return null;
  }
}

/** Whether a result is shown without expanding the card. */
export function isProminent(display: Display | undefined): boolean {
  return (
    !!display &&
    !display.error &&
    ['candles', 'quotes', 'python', 'command', 'skill', 'download', 'dataStatus'].includes(String(display.kind))
  );
}
