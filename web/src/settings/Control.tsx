import { useEffect, useState } from 'react'
import { RotateCcw } from 'lucide-react'

import type { Kind, Row, Setting } from './ladder'
import styles from './Control.module.css'

/**
 * One setting, drawn from its declaration.
 *
 * **This file has two hosts in mind and knows about neither.** It is given a
 * row and a way to change it, and it has no opinion about which tier it is
 * editing, which window it is in, or what happens after. That is the whole
 * point: the set-up wizard's single strongest constraint is that every step
 * renders the same control the settings page renders, and a wizard that had to
 * reimplement a number field would be a wizard whose ranges and refusals drift
 * from the page's ([#48](https://github.com/elpideus/demido-studio/issues/48)).
 *
 * So there are two exports and the line between them is the line between the
 * hosts:
 *
 * - [`Control`] is the widget alone, drawn from `Setting.kind`. A wizard step
 *   renders this under its own heading.
 * - [`Field`] is a settings page's row: the title, the sentence under it, the
 *   control, and the way back to the value below. A wizard does not render this,
 *   because a step has one setting and no tier to revert to.
 *
 * `design/system.md` gives the setting row two states and no provenance chip:
 * following global shows the inherited value and carries nothing, overridden
 * marks the row `signal` and carries the way back to global.
 */

/** What a host hands a control: what to draw, and what to do about a change. */
type Change = {
  row: Row
  /** Take a new value, and say whether it was taken. Called on every change for
   * a switch, and on commit for anything typed.
   *
   * The answer is what a typed control puts its draft back from. A field left
   * holding a number the ladder refused is a settings page telling the person
   * something is saved when it is not, which is the one thing this whole crate
   * is arranged to prevent. */
  onChange: (value: unknown) => Promise<boolean>
}

export function Control({ row, onChange }: Change) {
  const { setting, value } = row

  switch (setting.kind.control) {
    case 'amount':
      return <Amount setting={setting} kind={setting.kind} value={value} onChange={onChange} />
    case 'count':
      return <Count setting={setting} kind={setting.kind} value={value} onChange={onChange} />
    case 'text':
      return <Text setting={setting} kind={setting.kind} value={value} onChange={onChange} />
  }
}

/**
 * A settings page's row: the control, its name, its sentence, and its state.
 *
 * `revert` is what a tier below is reached through. The main settings window
 * passes nothing, because there is nothing under the global tier to fall back
 * to and a revert that restored a schema default would be a different gesture
 * wearing the same icon.
 */
export function Field({
  row,
  onChange,
  revert,
}: Change & {
  revert?: () => void
}) {
  // Three states rather than two, because the main settings window is not one
  // of them. `design/system.md` gives a setting row "following global" and
  // "overridden", and both are about a tier that has something under it: the
  // global page has nothing to follow, so its rows are neither.
  const inheriting = revert !== undefined
  const overridden = inheriting && row.setHere

  return (
    <div
      className={styles.field}
      data-overridden={overridden}
      // Following global reads the inherited value in `ink-4`
      // (`design/system.md`). It is the one place this application puts a value
      // in that ink on purpose: the number is not what this tier says, it is
      // what it is being handed.
      data-following={inheriting && !row.setHere}
    >
      <div className={styles.head}>
        <label className={styles.title} htmlFor={row.setting.id}>
          {row.setting.title}
        </label>
        {/* The way back to the value below, and only where there is one. A row
         * that is following shows nothing rather than a disabled button: the
         * state is carried by the thing itself (design/shell.md's ban on a
         * status dot), which here is the row's own accent. */}
        {overridden && (
          <button type="button" className={styles.revert} onClick={revert}>
            <RotateCcw className={styles.icon} strokeWidth={1.8} aria-hidden />
            Follow global
          </button>
        )}
      </div>
      <Control row={row} onChange={onChange} />
      <p className={styles.summary}>{row.setting.summary}</p>
    </div>
  )
}

/**
 * A switch and a field beside it.
 *
 * Off is not zero, and this control is the reason the distinction survives the
 * trip to the window: switching it off sends `null`, which is a request that
 * does not carry the parameter at all, and typing `0` sends zero.
 */
function Amount({
  setting,
  kind,
  value,
  onChange,
}: {
  setting: Setting
  kind: Extract<Kind, { control: 'amount' }>
  value: unknown
  onChange: (value: unknown) => Promise<boolean>
}) {
  const on = typeof value === 'number'

  return (
    <div className={styles.row}>
      <label className={styles.switch}>
        <input
          type="checkbox"
          className={styles.checkbox}
          checked={on}
          onChange={(event) => void onChange(event.target.checked ? kind.default : null)}
        />
        <span className={styles.switchLabel}>{on ? 'On' : 'Off'}</span>
      </label>
      <Number
        id={setting.id}
        value={on ? value : null}
        min={kind.min}
        max={kind.max}
        step={0.1}
        disabled={!on}
        onCommit={onChange}
      />
      {/* What the server does when nothing is sent, said where the switch is
       * rather than only in the sentence under the row. */}
      {!on && <span className={styles.aside}>The server decides.</span>}
    </div>
  )
}

function Count({
  setting,
  kind,
  value,
  onChange,
}: {
  setting: Setting
  kind: Extract<Kind, { control: 'count' }>
  value: unknown
  onChange: (value: unknown) => Promise<boolean>
}) {
  return (
    <div className={styles.row}>
      <Number
        id={setting.id}
        value={typeof value === 'number' ? value : kind.default}
        min={kind.min}
        max={kind.max}
        step={1}
        disabled={false}
        onCommit={onChange}
      />
      <span className={styles.aside}>tokens</span>
    </div>
  )
}

function Text({
  setting,
  kind,
  value,
  onChange,
}: {
  setting: Setting
  kind: Extract<Kind, { control: 'text' }>
  value: unknown
  onChange: (value: unknown) => Promise<boolean>
}) {
  const stored = typeof value === 'string' ? value : ''
  const [draft, setDraft, commit] = useDraft(stored, async (typed) =>
    typed === stored ? true : onChange(typed),
  )

  return kind.multiline ? (
    <textarea
      id={setting.id}
      className={styles.text}
      rows={4}
      value={draft}
      // Committed when the field is left rather than on every keystroke: a
      // system prompt is written a sentence at a time, and a write per
      // character is a file rewritten a hundred times per paragraph.
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
    />
  ) : (
    <input
      id={setting.id}
      type="text"
      className={styles.text}
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === 'Enter') event.currentTarget.blur()
      }}
    />
  )
}

/**
 * What is in the field, and what happens when the person is finished with it.
 *
 * Both typed controls need the same three things and got them twice before this
 * existed: a draft to type into, the stored value winning whenever it moves
 * underneath (a revert, or the other surface writing the same setting), and a
 * commit that only fires on a real change.
 */
function useDraft(
  stored: string,
  save: (draft: string) => Promise<boolean>,
): [string, (draft: string) => void, () => void] {
  const [draft, setDraft] = useState(stored)

  // A draft nobody is typing into is not worth keeping over the truth.
  useEffect(() => setDraft(stored), [stored])

  return [
    draft,
    setDraft,
    () => {
      // A refused value goes back to what is saved. The ladder never took it,
      // so leaving it in the field would make the page disagree with itself,
      // and the toast beside it says why it went.
      void save(draft).then((taken) => {
        if (!taken) setDraft(stored)
      })
    },
  ]
}

/**
 * A number, committed when it is finished rather than while it is being typed.
 *
 * Halfway through typing `16384` the field holds `1`, and a control that saved
 * every keystroke would set the context length to one, be refused, and show the
 * refusal while the person is still typing. Enter and leaving the field are both
 * commits, which is what every settings field in Windows does.
 */
function Number({
  id,
  value,
  min,
  max,
  step,
  disabled,
  onCommit,
}: {
  id: string
  value: number | null
  min: number
  max: number
  step: number
  disabled: boolean
  onCommit: (value: number) => Promise<boolean>
}) {
  const shown = value === null ? '' : String(value)
  const [draft, setDraft, commit] = useDraft(shown, async (text) => {
    const typed = globalThis.Number(text)
    // What is left in the field is not a number, so there is nothing to save
    // and nothing to refuse. Putting the stored value back is what says so.
    if (text.trim() === '' || globalThis.Number.isNaN(typed)) return false
    if (typed === value) return true
    return onCommit(typed)
  })

  return (
    <input
      id={id}
      type="number"
      className={styles.number}
      value={draft}
      min={min}
      max={max}
      step={step}
      disabled={disabled}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === 'Enter') event.currentTarget.blur()
      }}
    />
  )
}
