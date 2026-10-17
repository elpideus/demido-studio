import { type KeyboardEvent, type PointerEvent, type ReactNode, useRef, useState } from 'react';
import {
  ChevronDown,
  ChevronUp,
  ChevronsDown,
  ChevronsUp,
  ExternalLink,
  Eye,
  FolderOpen,
  GripVertical,
  MoreHorizontal,
  Pause,
  Play,
  PlayCircle,
  RotateCcw,
  Trash2,
  X,
} from 'lucide-react';
import { Button, IconButton, Menu, type MenuEntry, Tooltip, cx, formatBytes } from '@demido/ui';

import { api } from '@/lib/api';
import type { DownloadJob } from '@/lib/types';
import { localModelFor, useLocalModel } from '@/models/actions';
import { useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { formatEta, isRunning, isWaiting, shortName } from './queue';
import styles from './Downloads.module.css';

/** Below this much downloaded, canceling deletes it without asking. */
const CONFIRM_BYTES = 50_000_000;

export interface DragHandlers {
  dragging: boolean;
  /** Where the dragged row would land, if on this one. */
  drop: 'before' | 'after' | null;
  onGripDown: (e: PointerEvent) => void;
}

interface JobRowProps {
  job: DownloadJob;
  /** Its place among the queued downloads (1 is next), for one that is queued. */
  position?: number | null;
  first?: boolean;
  last?: boolean;
  drag?: DragHandlers;
  /** Moves a waiting row: by a step, or to an end. */
  onMove?: (to: 'up' | 'down' | 'top' | 'bottom') => void;
}

/** One download in the panel: what it is, how far it got, and what can be done with it. */
export function JobRow({ job, position, first, last, drag, onMove }: JobRowProps) {
  const [confirming, setConfirming] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLButtonElement>(null);
  const models = useModels((st) => st.models);
  const picked = useChats((st) => st.pickedModelId);
  const waiting = isWaiting(job);
  const fraction = job.total > 0 ? job.downloaded / job.total : 0;
  const model = job.state === 'done' ? localModelFor(models, job.name) : undefined;
  const inUse = model && (picked === model.id || (picked === null && model.isDefault));
  const title = shortName(job.repo);

  const cancel = () => {
    if (job.downloaded > CONFIRM_BYTES && !confirming) {
      setConfirming(true);
      return;
    }
    setConfirming(false);
    void api.cancelDownload(job.id);
    toast.info(
      'Download canceled',
      `${title}${job.quant ? ` ${job.quant}` : ''}${job.downloaded > 0 ? `, ${formatBytes(job.downloaded)} deleted` : ''}`,
    );
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if (!onMove || !e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown')) return;
    e.preventDefault();
    onMove(e.key === 'ArrowUp' ? 'up' : 'down');
  };

  const huggingFace = {
    id: 'hf',
    label: 'Open on Hugging Face',
    icon: ExternalLink,
    onSelect: () => void api.openUrl(`https://huggingface.co/${job.repo}`),
  };
  const items: MenuEntry[] = waiting
    ? [
        { id: 'now', label: 'Start now', icon: PlayCircle, onSelect: () => void api.startDownloadNow(job.id) },
        'separator',
        { id: 'top', label: 'Move to top', icon: ChevronsUp, disabled: first, onSelect: () => onMove?.('top') },
        {
          id: 'up',
          label: 'Move up',
          icon: ChevronUp,
          shortcut: 'Alt+↑',
          disabled: first,
          onSelect: () => onMove?.('up'),
        },
        {
          id: 'down',
          label: 'Move down',
          icon: ChevronDown,
          shortcut: 'Alt+↓',
          disabled: last,
          onSelect: () => onMove?.('down'),
        },
        {
          id: 'bottom',
          label: 'Move to bottom',
          icon: ChevronsDown,
          disabled: last,
          onSelect: () => onMove?.('bottom'),
        },
        'separator',
        huggingFace,
      ]
    : job.state === 'done'
      ? [
          ...(model?.path
            ? [
                {
                  id: 'show',
                  label: 'Show in folder',
                  icon: FolderOpen,
                  onSelect: () => void api.revealPath(model.path!),
                },
              ]
            : []),
          huggingFace,
          'separator',
          { id: 'remove', label: 'Remove from list', icon: X, onSelect: () => void api.removeDownload(job.id) },
        ]
      : job.state === 'failed'
        ? [
            { id: 'retry', label: 'Retry', icon: RotateCcw, onSelect: () => void api.resumeDownload(job.id) },
            huggingFace,
          ]
        : [huggingFace];

  let meta: ReactNode;
  switch (job.state) {
    case 'downloading':
      meta = (
        <>
          <span className={styles.mono}>
            {formatBytes(job.downloaded)} of {formatBytes(job.total)}
          </span>
          <span>
            {job.bytesPerSecond > 0
              ? `${formatBytes(job.bytesPerSecond)}/s · ${formatEta((job.total - job.downloaded) / job.bytesPerSecond)}`
              : 'Connecting'}
          </span>
        </>
      );
      break;
    case 'verifying':
      meta = (
        <>
          <span>Checking SHA-256{job.files > 1 ? ` of ${job.files} files` : ''}</span>
          <span className={styles.mono}>{formatBytes(job.total)}</span>
        </>
      );
      break;
    case 'queued':
      meta = (
        <>
          <span>{position === 1 ? 'Next up' : position ? `Waiting, ${position - 1} ahead` : 'Waiting'}</span>
          <span className={styles.mono}>
            {job.downloaded > 0 && `${formatBytes(job.downloaded)} of `}
            {formatBytes(job.total)}
          </span>
        </>
      );
      break;
    case 'paused':
      meta = (
        <>
          <span>Paused at {Math.floor(fraction * 100)}%</span>
          <span className={styles.mono}>
            {formatBytes(job.downloaded)} of {formatBytes(job.total)}
          </span>
        </>
      );
      break;
    case 'failed':
      meta = <span className={styles.error}>{job.error ?? 'The download failed.'}</span>;
      break;
    case 'done':
      meta = (
        <>
          <span>Verified, in your models</span>
          <span className={styles.mono}>{formatBytes(job.total)}</span>
        </>
      );
      break;
  }

  return (
    <div
      className={cx(
        styles.job,
        waiting && styles.jobWaiting,
        drag?.dragging && styles.dragging,
        drag?.drop === 'before' && styles.dropBefore,
        drag?.drop === 'after' && styles.dropAfter,
      )}
      data-job={job.id}
      tabIndex={waiting ? 0 : -1}
      onKeyDown={onKeyDown}
    >
      <div className={styles.jobTop}>
        {waiting && drag && (
          <Tooltip content="Drag to reorder, or Alt and the arrow keys" delay={400}>
            <span className={styles.grip} onPointerDown={drag.onGripDown} aria-hidden>
              <GripVertical size={14} />
            </span>
          </Tooltip>
        )}
        <div className={styles.jobTitle}>
          <div className={styles.jobName}>
            <span className={styles.ellipsis}>{title}</span>
            {job.quant && <span className={styles.quant}>{job.quant}</span>}
            {job.projector && (
              <Tooltip content="With its projector, so it can see" delay={200}>
                <Eye size={12} className={styles.muted} aria-label="With its projector" />
              </Tooltip>
            )}
          </div>
          <div className={styles.jobRepo}>
            {job.repo.split('/')[0]}
            {job.files > 1 && ` · ${job.files} files`}
          </div>
        </div>
        {job.state === 'downloading' && (
          <IconButton icon={Pause} label="Pause" size="sm" onClick={() => void api.pauseDownload(job.id)} />
        )}
        {job.state === 'queued' && (
          <IconButton icon={Pause} label="Pause" size="sm" onClick={() => void api.pauseDownload(job.id)} />
        )}
        {job.state === 'paused' && (
          <IconButton icon={Play} label="Resume" size="sm" onClick={() => void api.resumeDownload(job.id)} />
        )}
        {job.state === 'failed' && (
          <Button size="sm" variant="secondary" icon={RotateCcw} onClick={() => void api.resumeDownload(job.id)}>
            Retry
          </Button>
        )}
        {job.state === 'done' &&
          (inUse ? (
            <span className={styles.inUse}>In use</span>
          ) : (
            model && (
              <Button size="sm" variant="secondary" onClick={() => void useLocalModel(model)}>
                Use
              </Button>
            )
          ))}
        <IconButton
          ref={menuRef}
          icon={MoreHorizontal}
          label="More"
          size="sm"
          active={menuOpen}
          onClick={() => setMenuOpen((o) => !o)}
        />
        {job.state !== 'done' && job.state !== 'verifying' && (
          <IconButton icon={X} label="Cancel and delete" size="sm" className={styles.cancel} onClick={cancel} />
        )}
      </div>
      {job.state !== 'done' && (
        <div className={cx(styles.bar, styles[`bar-${job.state}`])}>
          <span style={{ width: `${(isRunning(job) || job.downloaded > 0 ? fraction : 0) * 100}%` }} />
        </div>
      )}
      <div className={styles.meta}>{meta}</div>
      {confirming && job.state !== 'done' && job.state !== 'verifying' && (
        <div className={styles.confirm} role="alertdialog" aria-label="Cancel the download">
          <Trash2 size={14} className={styles.error} aria-hidden />
          <span className={styles.grow}>Delete the {formatBytes(job.downloaded)} already downloaded?</span>
          <Button size="sm" variant="ghost" onClick={() => setConfirming(false)}>
            Keep
          </Button>
          <Button size="sm" variant="danger" onClick={cancel}>
            Cancel download
          </Button>
        </div>
      )}
      <Menu open={menuOpen} onClose={() => setMenuOpen(false)} anchorRef={menuRef} items={items} width={210} />
    </div>
  );
}
