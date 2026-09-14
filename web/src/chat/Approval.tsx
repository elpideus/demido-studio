import { Hand } from 'lucide-react'

import { useKey } from '@/shell/keys'
import { useApprovals, type Ability, type Asking } from './approvals'
import styles from './Approval.module.css'

/**
 * A call waiting on the person.
 *
 * **A row in the transcript, not a dialog** (`design/shell.md`), on `raised`
 * with an `amber` accent and a raised hand while it waits
 * (`design/system.md`). There is no status dot: the hand is the state, and a
 * small coloured circle beside a label is banned from this UI.
 *
 * It shows three things, and the third is the one that matters:
 *
 * - **the tool**, so the person knows what is asking;
 * - **the ability it declares**, which is the axis the permission matrix ruled
 *   on (`docs/rules/tools.md`), rather than a category this row invented;
 * - **the exact arguments that will run**, already parsed and checked against
 *   the tool's schema. Not a paraphrase and not the model's promise about them.
 *   A prompt that describes a call instead of showing it is a prompt somebody
 *   can agree to without knowing what they agreed to.
 *
 * ## Enter and Escape
 *
 * Bound at `panel` scope (`web/src/shell/keys.ts`), which is the exact case
 * `design/windows.md`'s keys section introduced scope for: a prompt waiting
 * somewhere on the desk must not steal the key the composer uses most. With one
 * of these open, Enter in the composer still sends the message, because a chord
 * typed into a field is not a shortcut.
 *
 * The caps are drawn, always, and that is one of exactly two permitted
 * exceptions to `design/system.md`'s rule that nothing inside the transcript
 * carries a keycap: **a control that takes a decision from the person may show
 * them**, because a decision made under time pressure should not need a hover
 * to learn.
 *
 * ## Always for this tool
 *
 * How Cautious decays into something livable by the person's choice rather than
 * by nagging. It writes the **chat** tier of the ladder and never the global one
 * (`demido_settings::id::TOOLS_ALWAYS`), so it is an answer about this
 * conversation rather than consent for every conversation they ever open.
 *
 * It is **absent on a destructive call**. Not disabled and not greyed: the
 * matrix's floor is that such a call asks every time and that *always* cannot
 * waive it, so offering the button and refusing it would be offering something
 * that is not on offer. The row says why instead, which is the actionable half.
 */
export function Approval({ asking }: { asking: Asking }) {
  const decide = useApprovals((approval) => approval.decide)

  useKey('panel', 'Enter', () => decide('allow'))
  useKey('panel', 'Escape', () => decide('deny'))

  return (
    <section className={styles.approval} aria-label={`Approve ${asking.tool}`}>
      <div className={styles.head}>
        <Hand className={styles.icon} strokeWidth={1.8} aria-hidden />
        <span className={styles.tool}>{asking.tool}</span>
        <span className={styles.ability}>{ABILITY[asking.ability]}</span>
      </div>

      <p className={styles.summary}>{asking.summary}</p>
      <pre className={styles.arguments}>{JSON.stringify(asking.arguments, null, 2)}</pre>

      {asking.destructive && (
        <p className={styles.floor}>
          This cannot be undone, so it is asked every time and cannot be answered always.
        </p>
      )}

      <div className={styles.answers}>
        <button type="button" className={styles.allow} onClick={() => decide('allow')}>
          Allow
          <Cap>Enter</Cap>
        </button>
        <button type="button" className={styles.deny} onClick={() => decide('deny')}>
          Deny
          <Cap>Esc</Cap>
        </button>
        {!asking.destructive && (
          <button type="button" className={styles.always} onClick={() => decide('always')}>
            Always for this tool
          </button>
        )}
      </div>
    </section>
  )
}

/**
 * A keycap.
 *
 * `design/system.md` gives it `--radius-control`, the silkscreen voice and
 * `ink-3`, and no hairline, because its surface already clears whatever it sits
 * on. That clause is written for a cap on `chrome` moving to `panel`; this row
 * is `raised`, so the cap moves the same one step, to `chrome`.
 */
function Cap({ children }: { children: string }) {
  return <kbd className={styles.cap}>{children}</kbd>
}

/**
 * What each ability means to somebody deciding, in the fewest words that are
 * still true. The definitions are `docs/rules/tools.md`'s; this is the wording,
 * and the wording is the window's.
 */
const ABILITY: Record<Ability, string> = {
  read: 'reads a file',
  write: 'changes a file',
  shell: 'runs a program',
  network: 'reaches off this machine',
}
