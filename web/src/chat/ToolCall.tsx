import { useState } from 'react'
import { ChevronDown, CircleSlash, TriangleAlert, Wrench } from 'lucide-react'

import type { Called } from './chat'
import styles from './ToolCall.module.css'

/**
 * One call, where it happened.
 *
 * `design/system.md` gives the **tool call row** five states and one surface:
 * `raised`, with an `src-tool` accent, `signal` while it runs, `ink-3` when it
 * is done and `rose` when it failed.
 * [#55](https://github.com/elpideus/demido-studio/issues/55) puts it in the
 * transcript rather than behind a window, so that nothing has to be opened to
 * see what just happened.
 *
 * Three things the row is careful about.
 *
 * **The arguments are the model's own text.** Shown as written, not
 * pretty-printed: this is the one application that promises what the model sent
 * is inspectable, and tidying it before it is read is the small version of
 * breaking that.
 *
 * **A long result folds.** One directory listing must not push the conversation
 * off the screen, so anything past [`FOLD`] lines is a summary that says how
 * much there is and expands in place. It expands rather than opening
 * somewhere: the reason to look is the sentence beside it.
 *
 * **A failure is drawn as a failure.** The log carries the tool's own outcome
 * (`failed`), so a broken tool and a model paraphrasing one cannot look alike,
 * which is the thing this row exists to keep apart. A refusal is its own third
 * state and not a failure at all: nothing was attempted, and the person is
 * usually the reason.
 */

/** How many lines of a result are shown before it folds. Enough for a short
 * answer to stay whole, few enough that a listing cannot take the screen. */
const FOLD = 6

export function ToolCall({ called }: { called: Called }) {
  const [open, setOpen] = useState(false)

  const state = stateOf(called)
  const text = called.outcome?.text ?? ''
  const lines = text.split('\n')
  const long = lines.length > FOLD
  const shown = open || !long ? text : lines.slice(0, FOLD).join('\n')

  return (
    <div className={styles.call} data-state={state}>
      <div className={styles.head}>
        <Mark state={state} />
        <span className={styles.tool}>{called.tool}</span>
        <span className={styles.arguments}>{called.arguments}</span>
      </div>

      {called.outcome && (
        <>
          <pre className={styles.result}>{shown}</pre>
          {long && (
            <button
              type="button"
              className={styles.more}
              aria-expanded={open}
              onClick={() => setOpen(!open)}
            >
              <ChevronDown className={styles.chevron} strokeWidth={1.8} aria-hidden />
              {open ? 'Show less' : `${lines.length - FOLD} more lines`}
            </button>
          )}
        </>
      )}
    </div>
  )
}

/** The states `design/system.md` names, as far as a finished row can be in
 * them. A row that is waiting on somebody is drawn by the approval instead, and
 * a row with nothing back yet is a call that is running. */
type State = 'running' | 'done' | 'failed' | 'refused'

function stateOf(called: Called): State {
  if (!called.outcome) return 'running'
  if (called.outcome.outcome === 'refused') return 'refused'
  return called.outcome.failed ? 'failed' : 'done'
}

/**
 * The state as an icon as well as a colour.
 *
 * Never colour alone: that is the session log's rule in `design/windows.md`
 * ("every row carries its source's icon as well"), and a transcript row that
 * said "this failed" only in rose would be the same mistake one surface over.
 * There is no status dot here or anywhere (`design/shell.md`); the icon is the
 * thing itself.
 */
function Mark({ state }: { state: State }) {
  const Glyph = state === 'failed' ? TriangleAlert : state === 'refused' ? CircleSlash : Wrench
  return <Glyph className={styles.icon} strokeWidth={1.8} aria-hidden />
}
