import { Shield } from 'lucide-react'

import { spoken } from '@/settings/Control'
import { MODE, useSettings } from '@/settings/ladder'
import styles from './ModeControl.module.css'

/**
 * The agent mode: what runs without asking.
 *
 * `docs/rules/tools.md` keeps it apart from the picker beside it. **Permitted**
 * is the mode's, read by the permission matrix and nothing else, and never said
 * to the model in prose. So the mode is invisible during a conversation that
 * calls no tool, and its one piece of standing visibility is the **shield on
 * Cautious** that `design/shell.md` draws: the strictest mode carries a mark,
 * and the other two carry nothing standing.
 *
 * The control is the chat tier's `tools.mode`, on the same ladder as every other
 * value, so the chat is the last word and a mode chosen here reaches this
 * conversation alone. The menu is its only surface in the composer; the main
 * settings window holds the default a new chat opens with.
 */
export function ModeControl({
  open,
  toggle,
  close,
}: {
  open: boolean
  toggle: () => void
  close: () => void
}) {
  const row = useSettings((settings) => settings.rows.chat?.find((it) => it.setting.id === MODE))
  const set = useSettings((settings) => settings.set)

  if (!row || row.setting.kind.control !== 'choice') return null
  const setting = row.setting
  const mode = typeof row.value === 'string' ? row.value : row.setting.kind.default
  const cautious = mode === 'cautious'

  return (
    <>
      {open && (
        <div
          className={styles.popover}
          role="dialog"
          aria-label="Agent mode"
          onKeyDown={(event) => {
            if (event.key === 'Escape') close()
          }}
        >
          <h2 className={styles.name}>Agent mode</h2>
          <div className={styles.modes} role="radiogroup" aria-label="Agent mode">
            {row.setting.kind.options.map((option) => (
              <button
                key={option}
                type="button"
                role="radio"
                className={styles.mode}
                aria-checked={option === mode}
                onClick={() => {
                  if (option !== mode) void set('chat', setting, option)
                  close()
                }}
              >
                <span className={styles.label}>
                  {option === 'cautious' && (
                    <Shield className={styles.icon} strokeWidth={1.8} aria-hidden />
                  )}
                  {spoken(option)}
                </span>
                <span className={styles.what}>{WHAT[option] ?? ''}</span>
              </button>
            ))}
          </div>
        </div>
      )}
      <button
        type="button"
        className={styles.trigger}
        aria-label={`Agent mode: ${spoken(mode)}`}
        aria-pressed={open}
        data-cautious={cautious}
        onClick={toggle}
      >
        {cautious && <Shield className={styles.icon} strokeWidth={1.8} aria-hidden />}
        {spoken(mode)}
      </button>
    </>
  )
}

/** What each row of the matrix does, in a sentence. UI copy about the matrix in
 * `demido-permission`, and never sent to a model. */
const WHAT: Record<string, string> = {
  cautious: 'Asks before anything that writes or runs a program.',
  balanced: 'Writes without asking. Asks before running a program.',
  autonomous: 'Runs everything without asking, except what cannot be undone.',
}
