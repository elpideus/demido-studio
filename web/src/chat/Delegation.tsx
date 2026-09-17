import { useLayoutEffect, useRef, useState } from 'react'
import { Bot, ChevronDown, TriangleAlert } from 'lucide-react'

import { Markdown } from './Markdown'
import type { Delegation as Delegated } from './chat'
import styles from './Delegation.module.css'

/**
 * A delegation, where it happened: the task that went out, and the sub-agent's
 * answer coming back.
 *
 * [#67](https://github.com/elpideus/demido-studio/issues/67). **One exchange,
 * not fifteen file reads.** A delegation exists so that a sub-agent's work
 * stays out of the conversation's context, and a clean context should also be a
 * clean transcript: what the child called and what came back from each call is
 * on the log under the child's own agent, where the session monitor's agent
 * scope reads it, and none of it is here.
 *
 * Three things the row is careful about.
 *
 * **The answer is framed as somebody else's.** It is the one text in this
 * transcript that the model neither wrote nor was asked by a person, so it is
 * drawn in a recess under a byline rather than as prose on the rack. A bubble
 * would say the model said it; a tool call row would say Demido did.
 *
 * **It is not also a call row.** The delegation is a `tool/call` on the log like
 * any other, and drawing both would be one thing on screen twice. The
 * projection (`demido_trace::Replay::transcript`) hands the window a
 * [`Delegated`] where it would otherwise hand it a `Called`, so there is no
 * decision taken here about which to draw.
 *
 * **A long answer folds by height.** By height rather than by line count, which
 * is what the tool call row does, because this text is Markdown and a fold that
 * sliced it would cut a fence in half. The fold is `--fold-answer`, and the
 * control only appears when there is really something behind it.
 *
 * `docs/rules/surfaces.md` rule 4: delegated is violet. A run that ended badly
 * takes `rose` instead, for the reason a failed tool result does: what came
 * back is what went wrong rather than an answer, and the row should not need
 * reading to tell those apart.
 */
export function Delegation({ delegation }: { delegation: Delegated }) {
  const [open, setOpen] = useState(false)
  const [folds, setFolds] = useState(false)
  const answer = useRef<HTMLDivElement>(null)

  // Whether there is more answer than the fold shows, measured on the rendered
  // text rather than guessed from its length: what a paragraph of prose and a
  // table of the same length come to on screen are not the same height.
  useLayoutEffect(() => {
    const shown = answer.current
    if (!shown) return
    setFolds(shown.scrollHeight > shown.clientHeight)
  }, [delegation.answer?.text])

  const state = delegation.answer ? (delegation.answer.failed ? 'failed' : 'done') : 'running'

  return (
    <section
      className={styles.delegation}
      data-state={state}
      aria-label={`Delegated to ${delegation.agent}`}
    >
      <div className={styles.head}>
        {state === 'failed' ? (
          <TriangleAlert className={styles.icon} strokeWidth={1.8} aria-hidden />
        ) : (
          <Bot className={styles.icon} strokeWidth={1.8} aria-hidden />
        )}
        {/* The equipment label voice, and the agent's own name, which is what
         * the session monitor's agent column calls it. */}
        <span className={styles.agent}>Sub-agent</span>
        <span className={styles.name}>{delegation.agent}</span>
      </div>

      {/* The task as the sub-agent was given it, which is the model's own
       * words. It is what went out, so it reads as a quotation rather than as
       * something Demido wrote. */}
      <p className={styles.task}>{delegation.task}</p>

      {delegation.answer ? (
        <>
          <div className={styles.answer} data-open={open} ref={answer}>
            <Markdown>{delegation.answer.text}</Markdown>
          </div>
          {folds && (
            <button
              type="button"
              className={styles.more}
              aria-expanded={open}
              onClick={() => setOpen(!open)}
            >
              <ChevronDown className={styles.chevron} strokeWidth={1.8} aria-hidden />
              {open ? 'Show less' : 'Show the whole answer'}
            </button>
          )}
        </>
      ) : (
        // It says what is happening rather than spinning, which is the same
        // answer the streaming bubble gives: there is no indeterminate bar on
        // this desk (`design/shell.md`).
        <p className={styles.working}>Working.</p>
      )}
    </section>
  )
}
