import { useEffect } from 'react'
import { ArrowRight, Check, Wrench } from 'lucide-react'

import { loading, useChat } from '@/chat/chat'
import { AcceleratorControl, ManifestControl, ModelControl, ModelFolderControl } from './Controls'
import { useSetup, type Step } from './setup'
import styles from './Wizard.module.css'

/**
 * The guided set-up, front and centre over the desk.
 *
 * `docs/rules/setup.md` section 1: "Set-up is the front-and-centre path on
 * first launch, and every step has a way out. It is not skipped past, not
 * minimised by default, not a row in a corner. It is what the window is doing
 * when it opens."
 *
 * And the other half of that sentence, which is why [`SetupRow`] is in this
 * file: **leaving is always possible and never final.** A person who leaves
 * lands on the desk with a row offering the rest, and what that row says is
 * derived from disk rather than remembered.
 *
 * Every step renders a control out of `Controls.tsx`, which is the same
 * component the settings page renders. There is no second implementation of
 * any control here, and there is deliberately nowhere one could be added: a
 * step is a heading, a sentence and one of those components.
 */
export function Wizard() {
  const view = useSetup((setup) => setup.view)
  const step = useSetup((setup) => setup.step)
  const show = useSetup((setup) => setup.show)
  const choose = useSetup((setup) => setup.choose)
  const leave = useSetup((setup) => setup.leave)
  const finish = useSetup((setup) => setup.finish)
  const busy = useSetup((setup) => setup.busy)
  const presence = useChat((chat) => chat.presence)

  // Nothing until the first read has answered, and nothing once the wizard has
  // been closed. Closing is what leaving does and what finishing does, and it
  // is the only thing that hides this: a wizard that vanished the moment the
  // last step went green would be one nobody could press Finish on, and the
  // Finish is the load. A desk somebody set up months ago has a closed wizard
  // and never sees it again.
  if (!view || view.closed) return null

  const on = step ?? 'accelerator'
  const order: Step[] = view.plan.steps.map((planned) => planned.step)
  const at = order.indexOf(on)
  const last = at === order.length - 1
  // The neighbours, or nothing at the ends. Indexing an array is how the
  // wizard walks its own plan, and the plan is derived, so there is no
  // guarantee written anywhere that the index is in range.
  const before = at > 0 ? order[at - 1] : undefined
  const after = order[at + 1]

  /** Leave a step for the next one, taking its answer with you.
   *
   * The accelerator is the one step that opens already answered, so
   * continuing past it is what confirms the pre-selection: the answers then
   * carry what the archives were chosen for, and the step is settled rather
   * than sitting outstanding behind a person who has read it and agreed with
   * it (`docs/rules/setup.md` section 3). */
  async function go(next: Step) {
    if (on === 'accelerator' && view) await choose(view.accelerator.chosen)
    show(next)
  }

  return (
    <>
      <div className={styles.scrim} aria-hidden />
      <section className={styles.window} role="dialog" aria-label="Set up Demido">
        <header className={styles.bar}>
          <h1 className={styles.name}>Set up Demido</h1>
          {/* The way out, on every step and never behind a menu. It is a
           * button rather than a close cross because leaving is a decision
           * with a consequence the desk then carries, and a cross reads as
           * "cancel and lose this". */}
          <button type="button" className={styles.leave} onClick={() => void leave()}>
            Not now
          </button>
        </header>

        <nav className={styles.steps} aria-label="Steps">
          {view.plan.steps.map((planned) => (
            <button
              key={planned.step}
              type="button"
              className={styles.step}
              data-standing={planned.standing}
              aria-current={planned.step === on ? 'step' : undefined}
              onClick={() => show(planned.step)}
            >
              {planned.standing === 'done' ? (
                <Check className={styles.icon} strokeWidth={1.8} aria-hidden />
              ) : (
                <span className={styles.dot} aria-hidden />
              )}
              {title(planned.step)}
            </button>
          ))}
        </nav>

        <div className={styles.page}>
          <div className={styles.head}>
            <h2 className={styles.heading}>{title(on)}</h2>
            <p className={styles.why}>{why(on)}</p>
          </div>
          {on === 'accelerator' && <AcceleratorControl />}
          {on === 'runtimes' && <ManifestControl />}
          {on === 'models' && (
            <>
              <ModelFolderControl />
              <ModelControl />
            </>
          )}
          {on === 'first-answer' && (
            <p className={styles.why}>
              {presence.state === 'ready'
                ? `${presence.model} is loaded and the desk is ready.`
                : presence.state === 'loading'
                  ? loading(presence.model)
                  : presence.state === 'failed'
                    ? presence.detail
                    : 'Nothing has been loaded yet. Finishing starts the model you chose.'}
            </p>
          )}
        </div>

        <footer className={styles.foot}>
          {before && (
            <button type="button" className={styles.quiet} onClick={() => show(before)}>
              Back
            </button>
          )}
          <span className={styles.spacer} />
          {last ? (
            <button
              type="button"
              className={styles.action}
              disabled={busy}
              onClick={() => void finish()}
            >
              {busy ? 'Starting' : 'Finish'}
              <ArrowRight className={styles.icon} strokeWidth={1.8} aria-hidden />
            </button>
          ) : (
            <button
              type="button"
              className={styles.action}
              disabled={!after}
              onClick={() => after && void go(after)}
            >
              Continue
              <ArrowRight className={styles.icon} strokeWidth={1.8} aria-hidden />
            </button>
          )}
        </footer>
      </section>
    </>
  )
}

/**
 * The row on the desk that offers the rest of the set-up.
 *
 * **Derived from disk rather than remembered.** What it lists is the plan,
 * which `demido-setup` computes from the runtimes ledger and the file system
 * on every read, so a person who deleted their runtimes folder is offered the
 * runtimes step again without anything having to be un-remembered.
 *
 * Persistent, because section 1 says leaving is never final. It is drawn on
 * the desk under the transcript, above the composer, where the composer's own
 * sentence about set-up sits beside it.
 */
export function SetupRow() {
  const view = useSetup((setup) => setup.view)
  const resume = useSetup((setup) => setup.resume)
  if (!view || view.complete || !view.closed) return null

  const outstanding = view.plan.steps.filter((planned) => planned.standing !== 'done')

  return (
    <div className={styles.outstanding}>
      <Wrench className={styles.icon} strokeWidth={1.8} aria-hidden />
      <p className={styles.rest}>
        Set-up is not finished: {list(outstanding.map((planned) => title(planned.step)))}.
      </p>
      <button type="button" className={styles.action} onClick={() => void resume()}>
        Take it up
      </button>
    </div>
  )
}

/** What a step is called. */
function title(step: Step): string {
  switch (step) {
    case 'accelerator':
      return 'Accelerator'
    case 'runtimes':
      return 'What has to arrive'
    case 'models':
      return 'Models'
    case 'first-answer':
      return 'First answer'
  }
}

/** What the step is for, in one sentence. */
function why(step: Step): string {
  switch (step) {
    case 'accelerator':
      return 'Detected and already answered. Change it if the detection is wrong.'
    case 'runtimes':
      return 'Demido bundles nothing and fetches it here, not the first time you ask a question. Every row states its download and its size on disk before a byte is spent, and a row you clear is one Demido leaves alone.'
    case 'models':
      return 'Read from folders you already have, so nothing is moved and no symlink is made.'
    case 'first-answer':
      return 'Set-up is finished when a model has answered, never because the steps were clicked through.'
  }
}

/** A list of things, in a sentence. */
function list(items: string[]): string {
  if (items.length <= 1) return items.join('')
  return `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`
}

/**
 * Read the set-up once, from the desk.
 *
 * A hook rather than a call in `App`, so the one component that draws the
 * wizard is also the one that asks for it, and there is no second place that
 * could ask for it differently.
 */
export function useSetupOnce() {
  const open = useSetup((setup) => setup.open)
  useEffect(() => {
    void open()
  }, [open])
}
