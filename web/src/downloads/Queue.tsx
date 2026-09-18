import type { ReactNode } from 'react'
import { Pause, Play, RotateCw, X } from 'lucide-react'

import { useChat } from '@/chat/chat'
import { useSetup } from '@/setup/setup'
import { size } from '@/shell/bytes'
import {
  answerWith,
  cancel,
  moving,
  pause,
  pauseAll,
  resume,
  useRows,
  type Failure,
  type Row,
} from './downloads'
import styles from './Queue.module.css'

/**
 * The download queue, opened from the indicator in the application title bar.
 *
 * `design/shell.md`: **a failed download is a row, never a dialog.** It states
 * what happened and offers retry in place, and the retry resumes rather than
 * restarting, because `downloads_resume` on a failed item asks for the bytes
 * still missing. The other rows keep running while it sits there.
 *
 * `design/system.md` gives the row its colours: progress `signal`, paused
 * `amber`, failed `rose`. The bar is coloured by its own state, which is how a
 * state is carried without a coloured circle beside the words
 * (`design/shell.md` bans those).
 *
 * Progress is in bytes against the total, so a person can judge how long is
 * left. A percentage alone says how far and not how much.
 */
export function Queue() {
  const rows = useRows()
  // Waiting rows as well as running ones: pausing a row that is waiting for a
  // slot is what stops it taking the next one.
  const pausable = rows.some((row) => moving(row) && row.state !== 'verifying')

  return (
    <section className={styles.queue} aria-label="Downloads">
      <header className={styles.head}>
        <h2 className={styles.title}>Downloads</h2>
        <button
          type="button"
          className={styles.chip}
          disabled={!pausable}
          onClick={() => void pauseAll()}
        >
          <Pause className={styles.chip_icon} strokeWidth={1.8} aria-hidden />
          Pause all
        </button>
      </header>
      {rows.length === 0 ? (
        <p className={styles.empty}>Nothing is downloading.</p>
      ) : (
        <ul className={styles.rows}>
          {rows.map((row) => (
            <QueueRow key={row.id} row={row} />
          ))}
        </ul>
      )}
    </section>
  )
}

function QueueRow({ row }: { row: Row }) {
  const chosen = useSetup((setup) => setup.view?.models.chosen ?? null)
  const presence = useChat((chat) => chat.presence.state)
  const running = useChat((chat) => chat.running)
  const busy = useSetup((setup) => setup.busy)
  const answering = chosen === row.path && (presence === 'ready' || presence === 'loading')
  const share = row.total > 0 ? Math.min(row.received / row.total, 1) : 0

  return (
    <li className={styles.row} data-state={row.state} aria-label={row.name}>
      <div className={styles.line}>
        <div className={styles.names}>
          <span className={styles.name}>{row.name}</span>
          <span className={styles.repo}>{row.repo}</span>
        </div>
        <div className={styles.actions}>
          {(row.state === 'running' || row.state === 'queued') && (
            <IconButton label={`Pause ${row.name}`} onClick={() => void pause(row.id)}>
              <Pause className={styles.icon} strokeWidth={1.8} aria-hidden />
            </IconButton>
          )}
          {row.state === 'paused' && (
            <IconButton label={`Resume ${row.name}`} onClick={() => void resume(row.id)}>
              <Play className={styles.icon} strokeWidth={1.8} aria-hidden />
            </IconButton>
          )}
          {/* Retry is a word rather than an icon, because it is the one thing
           * a failed row is for and it must not be mistaken for resume. */}
          {row.state === 'failed' && (
            <button type="button" className={styles.chip} onClick={() => void resume(row.id)}>
              <RotateCw className={styles.chip_icon} strokeWidth={1.8} aria-hidden />
              Retry
            </button>
          )}
          {/* Arriving and being usable are the same event: the finished row
           * offers the model it brought. Not while a turn runs, because loading
           * a model under an answer that is still arriving ends that answer. */}
          {row.state === 'done' && !answering && (
            <button
              type="button"
              className={styles.chip}
              disabled={running || busy}
              onClick={() => void answerWith(row)}
            >
              Use
            </button>
          )}
          {/* Verifying has no cancel: the bytes are all in, and the check is
           * seconds. A cancel on a finished row dismisses it and leaves the
           * model where it is. */}
          {row.state !== 'verifying' && (
            <IconButton
              label={row.state === 'done' ? `Dismiss ${row.name}` : `Cancel ${row.name}`}
              onClick={() => void cancel(row.id)}
            >
              <X className={styles.icon} strokeWidth={1.8} aria-hidden />
            </IconButton>
          )}
        </div>
      </div>
      <div className={styles.bar} aria-hidden>
        <div className={styles.filled} style={{ width: `${share * 100}%` }} />
      </div>
      <p className={styles.figures}>
        <span className={styles.bytes}>
          {size(row.received)} of {size(row.total)}
        </span>
        <span className={styles.said}>{said(row, answering)}</span>
      </p>
    </li>
  )
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string
  onClick: () => void
  children: ReactNode
}) {
  return (
    <button type="button" className={styles.action} aria-label={label} onClick={onClick}>
      {children}
    </button>
  )
}

/** What the row says about itself, beside its bytes. */
function said(row: Row, answering: boolean): string {
  switch (row.state) {
    case 'queued':
      return 'Waiting for a slot.'
    case 'running':
      return 'Downloading.'
    case 'verifying':
      return 'Checking the files.'
    case 'paused':
      return row.received > 0 ? 'Paused. The bytes so far are kept.' : 'Paused.'
    case 'failed':
      return cause(row.failure)
    case 'done':
      return answering ? 'Verified, and answering.' : 'Verified and in the library.'
  }
}

/** What happened, as a person can act on it. Each is a different thing to do:
 * a reset wants a retry, a full disk wants room, a gated file wants an account
 * this fetch does not have. */
function cause(failure: Failure): string {
  switch (failure.kind) {
    case 'no-room':
      return `Not enough room: ${size(failure.needs)} still to come and ${size(failure.free)} free.`
    case 'disk-full':
      return 'The disk filled up. The bytes so far are kept.'
    case 'gated':
      return 'This file needs a Hugging Face account or accepted terms.'
    case 'missing':
      return 'The file is not in the repository any more.'
    case 'rate-limited':
      return 'Hugging Face is limiting requests for now.'
    case 'refused':
      return `The host refused with ${failure.status}.`
    case 'unreachable':
      return `The host could not be reached: ${failure.detail}.`
    case 'reset':
      return 'The connection was reset. The bytes so far are kept.'
    case 'ended-early':
      return 'The transfer ended early. The bytes so far are kept.'
    case 'stalled':
      return `Nothing arrived for ${failure.seconds} seconds.`
    case 'wrong-size':
      return 'The host is sending a different size than the one listed.'
    case 'not-the-file':
      return 'The host sent a web page instead of the file.'
    case 'corrupt':
      return 'The file did not match its published digest, so it was deleted. A retry fetches it again.'
    case 'damaged':
      return 'The file arrived and is not a readable model, so it was deleted. A retry fetches it again.'
    case 'disk':
      return `The disk refused: ${failure.detail}.`
  }
}
