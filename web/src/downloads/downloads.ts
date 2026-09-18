import { useSyncExternalStore } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useSetup, type Damage } from '@/setup/setup'
import { sentence } from '@/shell/failure'
import { useToasts } from '@/shell/toasts'

/**
 * The download queue, as the window holds it, outside React.
 *
 * `demido-download` owns the queue; this is the window's copy of its rows,
 * kept whole by `downloads_rows` once and by `downloads://event` after that.
 *
 * **Progress goes into a ref with a subscription, never `setState` per tick**
 * ([#76](https://github.com/elpideus/demido-studio/issues/76)). It is
 * `web/src/chat/stream.ts`'s rule applied to the other high-frequency stream
 * in the product: two transfers each report several times a second for as
 * long as a multi-gigabyte model takes, and putting each report through a
 * store would re-render every subscriber of that store per report. So an event
 * mutates the rows and tells nobody, subscribers are told once per animation
 * frame, and the snapshot they read is replaced only at that moment.
 *
 * **Arriving and being usable are the same event.** A row that turns `done`
 * reads the set-up view again, which is where the library is listed, so the
 * model is offered wherever a model is chosen without anybody asking for a
 * rescan. The row itself offers it too ([`answerWith`]).
 */

/** Why an item stopped. The Rust `demido_download::Failure`. The window writes
 * the sentence; each of these is a different thing for a person to do. */
export type Failure =
  | { kind: 'no-room'; needs: number; free: number }
  | { kind: 'disk-full' }
  | { kind: 'gated'; status: number }
  | { kind: 'missing' }
  | { kind: 'rate-limited' }
  | { kind: 'refused'; status: number }
  | { kind: 'unreachable'; detail: string }
  | { kind: 'reset'; received: number }
  | { kind: 'ended-early'; received: number; expected: number }
  | { kind: 'stalled'; seconds: number }
  | { kind: 'wrong-size'; stated: number; expected: number }
  | { kind: 'not-the-file'; content_type: string }
  | { kind: 'corrupt' }
  | { kind: 'damaged'; damage: Damage }
  | { kind: 'disk'; detail: string }

/** Where an item is. The Rust `demido_download::State`, flattened into the
 * row. */
export type State =
  | { state: 'queued' }
  | { state: 'running' }
  | { state: 'verifying' }
  | { state: 'paused' }
  | { state: 'failed'; failure: Failure }
  | { state: 'done' }

/** One model in the queue. The Rust `demido_download::Row`. */
export type Row = {
  id: number
  repo: string
  name: string
  /** The file a backend is handed once the row is done. */
  path: string
  /** Bytes on disk, against `total`. */
  received: number
  total: number
} & State

type Event = { event: 'row'; row: Row } | { event: 'gone'; id: number }

/** The buffer. Mutated on every event, read by nobody until a flush. */
const rows = new Map<number, Row>()

/** Ids an event said were gone. The first read of the queue can answer after
 * a cancel it predates has already arrived, and a row it still lists must not
 * come back. */
const gone = new Set<number>()

const NOTHING: Row[] = []
let snapshot: Row[] = NOTHING
let queued = false
const listeners = new Set<() => void>()

function flush() {
  queued = false
  // In the order they were queued, which is the order Rust keeps them in. Ids
  // only grow, so sorting by one restores it whatever order events and the
  // first read arrived in.
  snapshot = [...rows.values()].sort((a, b) => a.id - b.id)
  for (const listener of listeners) listener()
}

/** At most one flush per frame, however many reports arrived in it. */
function schedule() {
  if (queued) return
  queued = true
  requestAnimationFrame(flush)
}

/** One event, applied and not announced. */
function apply(event: Event) {
  if (event.event === 'gone') {
    gone.add(event.id)
    rows.delete(event.id)
  } else {
    const before = rows.get(event.row.id)
    rows.set(event.row.id, event.row)
    if (event.row.state === 'done' && before?.state !== 'done') arrived()
  }
  schedule()
}

/** Every row at once, replacing the copy: what the window is sent when it fell
 * behind, because a missed last change has nothing after it to repair it. */
function replace(all: Row[]) {
  const before = new Map(rows)
  rows.clear()
  for (const row of all) {
    rows.set(row.id, row)
    if (row.state === 'done' && before.get(row.id)?.state !== 'done') arrived()
  }
  for (const id of before.keys()) if (!rows.has(id)) gone.add(id)
  schedule()
}

/** Whether a row is still on its way: running, waiting for a slot, or being
 * checked. */
export function moving(row: Row): boolean {
  return row.state === 'running' || row.state === 'queued' || row.state === 'verifying'
}

/** A model is in the library. Every surface that lists models reads the set-up
 * view, so reading it again is what offers the new one there. */
function arrived() {
  void useSetup.getState().read()
}

export function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function read() {
  return snapshot
}

/** Every row, as of the last frame. */
export function useRows(): Row[] {
  return useSyncExternalStore(subscribe, read, read)
}

/** Subscribe, then read what the queue already holds. Once per window: a
 * second listener is a second copy of every event.
 *
 * Listening first is the order that loses nothing. A row that changes between
 * the two is then both in the first read and in an event, and the event is the
 * newer of the two, so the read never overwrites a row an event already set. */
let opened: Promise<UnlistenFn> | null = null

export async function open(): Promise<void> {
  if (opened) return
  opened = listen<Event>('downloads://event', (event) => apply(event.payload))
  try {
    await opened
    await listen<Row[]>('downloads://rows', (event) => replace(event.payload))
  } catch (error) {
    // Without the channel the indicator shows what was queued at startup and
    // never moves. The downloads themselves carry on.
    console.warn('the download queue will report no progress', error)
    opened = null
  }
  try {
    for (const row of await invoke<Row[]>('downloads_rows')) {
      if (!rows.has(row.id) && !gone.has(row.id)) rows.set(row.id, row)
    }
    schedule()
  } catch (error) {
    console.warn('the download queue could not be read', error)
  }
}

/** One command on one row, or on all of them. A refusal is a toast, because a
 * person pressed something a moment ago and it was not taken. The row itself
 * changes through the channel, never through what the call returns. */
async function command(name: string, args?: Record<string, unknown>) {
  try {
    await invoke(name, args)
  } catch (error) {
    useToasts.getState().show(sentence(error))
  }
}

export const pause = (id: number) => command('downloads_pause', { id })
export const pauseAll = () => command('downloads_pause_all')
/** Resume a paused row, or retry a failed one from the bytes on disk. */
export const resume = (id: number) => command('downloads_resume', { id })
/** Take a row out and delete its partial files. On a finished row it only
 * dismisses the row: the model stays in the library. */
export const cancel = (id: number) => command('downloads_cancel', { id })

/**
 * Answer with the model a finished row brought.
 *
 * The same two steps the wizard's last page takes, so there is one way a model
 * becomes the conversation's: settle it as the answer, then point the chat at
 * it and load. Settling first and stopping on a refusal matters, because
 * loading after a refused choice would reload whatever was chosen before.
 */
export async function answerWith(row: Row) {
  try {
    await invoke('setup_choose_model', { path: row.path })
  } catch (error) {
    useToasts.getState().show(sentence(error))
    return
  }
  await useSetup.getState().finish()
}
