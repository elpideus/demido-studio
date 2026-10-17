import { type PointerEvent, type ReactNode, useEffect, useRef, useState } from 'react';
import {
  Boxes,
  Check,
  Download,
  ExternalLink,
  FolderOpen,
  KeyRound,
  Pause,
  Play,
  SlidersHorizontal,
  X,
} from 'lucide-react';
import {
  Button,
  EmptyState,
  IconButton,
  Popover,
  SegmentedControl,
  TextField,
  Tooltip,
  cx,
  formatBytes,
} from '@demido/ui';

import { api } from '@/lib/api';
import type { DownloadJob, QueueOptions } from '@/lib/types';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { useWindows } from '@/stores/windows';
import { JobRow } from './JobRow';
import { useDownloadsPanel } from './panel';
import { beforeFor, formatEta, queuePosition, summarize } from './queue';
import styles from './Downloads.module.css';

const LIMITS: Array<{ value: string; label: string }> = [
  { value: '0', label: 'Off' },
  { value: '5000000', label: '5 MB/s' },
  { value: '10000000', label: '10 MB/s' },
  { value: '25000000', label: '25 MB/s' },
  { value: '50000000', label: '50 MB/s' },
];

const TRANSFERS: Array<{ value: string; label: string }> = [
  { value: 'http', label: 'Standard' },
  { value: 'xet', label: 'Xet' },
];

const HF_TOKENS_URL = 'https://huggingface.co/settings/tokens';

const RING = 2 * Math.PI * 13;

/**
 * The navigation rail's downloads button: a ring that fills as the queue arrives and a count of
 * what is left. Hovering says how it is going; a click opens the panel (Ctrl+J too).
 */
export function DownloadsButton() {
  const jobs = useModels((st) => st.downloads);
  const open = useDownloadsPanel((st) => st.open);
  const setOpen = useDownloadsPanel((st) => st.setOpen);
  const anchor = useRef<HTMLButtonElement>(null);
  const s = summarize(jobs);
  const live = s.running.length + s.waiting.length + s.failed.length;
  const fraction = s.total > 0 ? s.downloaded / s.total : 0;
  const count = s.running.length + s.queued;

  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'j') {
        e.preventDefault();
        setOpen(!useDownloadsPanel.getState().open);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [setOpen]);

  const summary = (
    <span className={styles.tip}>
      <span className={styles.tipTitle}>
        Downloads <span className={styles.tipKey}>Ctrl+J</span>
      </span>
      {live === 0 ? (
        <span className={styles.tipMuted}>
          {s.done.length ? `${s.done.length} finished. ` : ''}Nothing is downloading.
        </span>
      ) : (
        <>
          <span className={styles.tipLine}>
            <span>
              {s.running.length} downloading{s.queued ? `, ${s.queued} waiting` : ''}
            </span>
            <span className={styles.mono}>{Math.floor(fraction * 100)}%</span>
          </span>
          {s.bytesPerSecond > 0 && (
            <span className={cx(styles.tipLine, styles.tipMuted)}>
              <span>{formatBytes(s.bytesPerSecond)}/s</span>
              <span>{formatEta(s.eta)}</span>
            </span>
          )}
          {(s.paused > 0 || s.failed.length > 0) && (
            <span className={styles.tipMuted}>
              {[s.paused && `${s.paused} paused`, s.failed.length && `${s.failed.length} failed`]
                .filter(Boolean)
                .join(', ')}
            </span>
          )}
        </>
      )}
    </span>
  );

  return (
    <>
      <Tooltip content={summary} placement="right" delay={250} disabled={open}>
        <button
          ref={anchor}
          type="button"
          aria-label="Downloads"
          aria-pressed={open}
          aria-haspopup="dialog"
          className={cx(styles.button, open && styles.buttonOpen)}
          onClick={() => setOpen(!open)}
        >
          {live > 0 ? (
            <>
              <Download size={16} strokeWidth={1.8} aria-hidden />
              <svg className={styles.ring} viewBox="0 0 28 28" aria-hidden>
                <circle className={styles.ringTrack} cx="14" cy="14" r="13" />
                <circle
                  className={cx(styles.ringFill, s.running.length === 0 && styles.ringIdle)}
                  cx="14"
                  cy="14"
                  r="13"
                  strokeDasharray={RING}
                  strokeDashoffset={RING * (1 - fraction)}
                />
              </svg>
            </>
          ) : (
            <Download size={21} strokeWidth={1.7} aria-hidden />
          )}
          {s.failed.length > 0 ? (
            <span className={cx(styles.badge, styles.badgeFailed)}>!</span>
          ) : count > 0 ? (
            <span className={styles.badge}>{count}</span>
          ) : s.paused > 0 ? (
            <span className={cx(styles.badge, styles.badgePaused)}>{s.paused}</span>
          ) : null}
        </button>
      </Tooltip>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        anchorRef={anchor}
        placement="right-end"
        width={460}
        maxHeight="min(680px, calc(100vh - 32px))"
        aria-label="Downloads"
      >
        <DownloadsPanel onClose={() => setOpen(false)} />
      </Popover>
    </>
  );
}

interface Drag {
  id: string;
  over: string | null;
  pos: 'before' | 'after';
}

function DownloadsPanel({ onClose }: { onClose: () => void }) {
  const jobs = useModels((st) => st.downloads);
  const [options, setOptions] = useState<QueueOptions | null>(null);
  const [showOptions, setShowOptions] = useState(false);
  const [folder, setFolder] = useState<string | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const s = summarize(jobs);
  const moving = s.running.length + s.queued > 0;

  useEffect(() => {
    void api.downloadOptions().then(setOptions);
    void api.modelFolders().then((f) => setFolder(f[0]?.path ?? null));
  }, []);

  const change = (patch: Partial<QueueOptions>) => {
    if (!options) return;
    const next = { ...options, ...patch };
    setOptions(next);
    void api.setDownloadOptions(next).then(setOptions);
  };

  const reorder = (id: string, before: string | null) => void api.reorderDownload(id, before);
  const moveBy = (job: DownloadJob, to: 'up' | 'down' | 'top' | 'bottom') => {
    const i = s.waiting.findIndex((j) => j.id === job.id);
    const index = to === 'top' ? 0 : to === 'bottom' ? s.waiting.length : to === 'up' ? i - 1 : i + 1;
    reorder(job.id, beforeFor(s.waiting, job.id, index));
  };

  // Dragging by the grip: the row under the pointer, and which half of it, is where it lands.
  const startDrag = (job: DownloadJob) => (e: PointerEvent) => {
    e.preventDefault();
    let current: Drag = { id: job.id, over: null, pos: 'before' };
    setDrag(current);
    const onMove = (ev: globalThis.PointerEvent) => {
      const rows = [...(listRef.current?.querySelectorAll<HTMLElement>('[data-waiting]') ?? [])];
      let over: HTMLElement | undefined;
      for (const row of rows) {
        const r = row.getBoundingClientRect();
        if (ev.clientY < r.bottom || row === rows.at(-1)) {
          over = row;
          break;
        }
      }
      if (!over) return;
      const r = over.getBoundingClientRect();
      current = {
        id: job.id,
        over: over.dataset.waiting ?? null,
        pos: ev.clientY < r.top + r.height / 2 ? 'before' : 'after',
      };
      setDrag(current);
    };
    const onUp = () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      setDrag(null);
      const { id, over, pos } = current;
      if (!over || over === id) return;
      const others = s.waiting.filter((j) => j.id !== id);
      const at = others.findIndex((j) => j.id === over);
      if (at < 0) return;
      const before = pos === 'before' ? over : (others[at + 1]?.id ?? null);
      const now = s.waiting.findIndex((j) => j.id === id);
      const nowBefore = s.waiting[now + 1]?.id ?? null;
      if (before !== nowBefore) reorder(id, before);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  };

  const openModels = () => {
    onClose();
    useWindows.getState().open('models');
  };

  return (
    <div className={styles.panel}>
      <header className={styles.head}>
        <h2 className={styles.title}>Downloads</h2>
        <span className={styles.summary}>
          {s.bytesPerSecond > 0
            ? `${formatBytes(s.bytesPerSecond)}/s · ${formatEta(s.eta)}`
            : s.running.length + s.waiting.length > 0
              ? 'Nothing moving'
              : ''}
        </span>
        {moving ? (
          <Button size="sm" variant="ghost" icon={Pause} onClick={() => void api.pauseAllDownloads()}>
            Pause all
          </Button>
        ) : (
          s.paused + s.failed.length > 0 && (
            <Button size="sm" variant="ghost" icon={Play} onClick={() => void api.resumeAllDownloads()}>
              Resume all
            </Button>
          )
        )}
        <IconButton
          icon={SlidersHorizontal}
          label="Queue options"
          size="sm"
          active={showOptions}
          onClick={() => setShowOptions((o) => !o)}
        />
        <IconButton icon={X} label="Close" size="sm" onClick={onClose} />
      </header>

      {s.total > 0 && (
        <div className={styles.total}>
          <div className={styles.bar}>
            <span style={{ width: `${(s.downloaded / s.total) * 100}%` }} />
          </div>
          <div className={styles.totalMeta}>
            <span>
              {formatBytes(s.downloaded)} of {formatBytes(s.total)}
            </span>
            <span>
              {s.running.length + s.waiting.length + s.failed.length}{' '}
              {s.running.length + s.waiting.length + s.failed.length === 1 ? 'download' : 'downloads'}
            </span>
          </div>
        </div>
      )}

      {showOptions && options && (
        <div className={styles.options}>
          <div className={styles.option}>
            <span>Download at the same time</span>
            <SegmentedControl
              size="sm"
              value={String(options.concurrency)}
              onChange={(v) => change({ concurrency: Number(v) })}
              options={['1', '2', '3'].map((v) => ({ value: v, label: v }))}
            />
          </div>
          <div className={styles.option}>
            <span>Speed limit</span>
            <SegmentedControl
              size="sm"
              value={LIMITS.some((l) => l.value === String(options.limit)) ? String(options.limit) : '0'}
              onChange={(v) => change({ limit: Number(v) })}
              options={LIMITS}
            />
          </div>
          <div className={styles.option}>
            <span>Transfer</span>
            <SegmentedControl
              size="sm"
              value={options.xet ? 'xet' : 'http'}
              onChange={(v) => change({ xet: v === 'xet' })}
              options={TRANSFERS}
            />
          </div>
          <HfToken />
          {folder && (
            <div className={styles.option}>
              <span>Saves to</span>
              <button type="button" className={styles.folder} onClick={() => void api.openPath(folder)}>
                <FolderOpen size={12} aria-hidden />
                <span className={styles.ellipsis}>{folder}</span>
              </button>
            </div>
          )}
          <p className={styles.optionNote}>
            One at a time gives the model you are waiting for the whole connection. Every file is checked against its
            SHA-256 before it is offered, and a paused or failed download continues where it stopped, also after a
            restart.
          </p>
          <p className={styles.optionNote}>
            Xet is Hugging Face’s own transfer engine: it fetches a file in pieces over many connections, which helps
            most on slow or distant routes, and holds up to 300 MB of memory while it runs. A token lets either one
            fetch the gated and private models your account can open.
          </p>
        </div>
      )}

      <div ref={listRef} className={styles.body}>
        {jobs.length === 0 && (
          <EmptyState
            compact
            icon={Download}
            title="No downloads"
            description="Pick a model and a quantization in Models, then Download."
          />
        )}
        {s.running.length > 0 && (
          <Section
            title="Downloading"
            note={options ? `${s.running.length} of ${options.concurrency} at a time` : undefined}
          >
            {s.running.map((job) => (
              <JobRow key={job.id} job={job} />
            ))}
          </Section>
        )}
        {s.waiting.length > 0 && (
          <Section title="Up next" note={s.waiting.length > 1 ? 'Drag to reorder' : undefined}>
            {s.waiting.map((job, i) => (
              <div key={job.id} data-waiting={job.id}>
                <JobRow
                  job={job}
                  position={queuePosition(jobs, job.id)}
                  first={i === 0}
                  last={i === s.waiting.length - 1}
                  onMove={(to) => moveBy(job, to)}
                  drag={{
                    dragging: drag?.id === job.id,
                    drop: drag && drag.over === job.id && drag.id !== job.id ? drag.pos : null,
                    onGripDown: startDrag(job),
                  }}
                />
              </div>
            ))}
          </Section>
        )}
        {s.failed.length > 0 && (
          <Section title="Failed">
            {s.failed.map((job) => (
              <JobRow key={job.id} job={job} />
            ))}
          </Section>
        )}
        {s.done.length > 0 && (
          <Section
            title="Finished"
            action={
              <Button size="sm" variant="ghost" onClick={() => void api.clearDownloads()}>
                Clear
              </Button>
            }
          >
            {s.done.map((job) => (
              <JobRow key={job.id} job={job} />
            ))}
          </Section>
        )}
      </div>

      <footer className={styles.foot}>
        <span>{options?.limit ? `Limited to ${formatBytes(options.limit)}/s` : 'No speed limit'}</span>
        <span className={styles.grow} />
        <Button size="sm" variant="ghost" icon={Boxes} onClick={openModels}>
          Browse models
        </Button>
      </footer>
    </div>
  );
}

/** The Hugging Face access token downloads send, for gated and private repositories. */
function HfToken() {
  const [saved, setSaved] = useState<boolean | null>(null);
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void api.hasHfToken().then(setSaved);
  }, []);

  const store = async (token: string | null) => {
    setBusy(true);
    try {
      if (await api.setHfToken(token)) {
        setValue('');
        setSaved(token !== null);
        if (token) toast.success('Token saved', 'It is kept in your system’s credential manager.');
      } else {
        toast.error('The token could not be saved');
      }
    } finally {
      setBusy(false);
    }
  };

  if (saved === null) return null;
  return (
    <div className={styles.token}>
      <div className={styles.option}>
        <span>Hugging Face token</span>
        {saved ? (
          <span className={styles.tokenSaved}>
            <Check size={12} aria-hidden />
            Saved
            <Button size="sm" variant="ghost" loading={busy} onClick={() => void store(null)}>
              Remove
            </Button>
          </span>
        ) : (
          <a
            href={HF_TOKENS_URL}
            className={styles.link}
            onClick={(e) => {
              e.preventDefault();
              void api.openUrl(HF_TOKENS_URL);
            }}
          >
            Get a token <ExternalLink size={11} />
          </a>
        )}
      </div>
      {!saved && (
        <div className={styles.tokenRow}>
          <TextField
            size="sm"
            icon={KeyRound}
            type="password"
            placeholder="hf_…"
            aria-label="Hugging Face access token"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && value.trim() && void store(value)}
          />
          <Button
            size="sm"
            variant="secondary"
            disabled={!value.trim()}
            loading={busy}
            onClick={() => void store(value)}
          >
            Save
          </Button>
        </div>
      )}
    </div>
  );
}

function Section({
  title,
  note,
  action,
  children,
}: {
  title: string;
  note?: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className={styles.section} aria-label={title}>
      <h3 className={styles.sectionHead}>
        {title}
        {note && <span className={styles.sectionNote}>{note}</span>}
        <span className={styles.grow} />
        {action}
      </h3>
      {children}
    </section>
  );
}
