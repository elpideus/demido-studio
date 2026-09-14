import { useEffect, useRef } from 'react'

import { Approval } from './Approval'
import { Markdown } from './Markdown'
import { ToolCall } from './ToolCall'
import { useApprovals } from './approvals'
import { useChat, type Said } from './chat'
import { useStreaming } from './stream'
import styles from './Transcript.module.css'

/**
 * What has been said, and what is being said right now.
 *
 * The transcript is the rack itself rather than an island on it
 * (`docs/rules/surfaces.md`), which is what makes chat read as the surface the
 * application is rather than as the largest card on one.
 *
 * Everything above the last bubble comes from the session log
 * (`chat.transcript`). The last one, while a turn is running, comes from
 * `stream.ts`, which is a buffer outside React notified once per frame. That
 * split is the reason a long answer arriving at sixty tokens a second does not
 * re-render the conversation above it: only [`Answering`] subscribes.
 */
export function Transcript() {
  const transcript = useChat((chat) => chat.transcript)
  const pending = useChat((chat) => chat.pending)
  const running = useChat((chat) => chat.running)
  const failure = useChat((chat) => chat.failure)
  const hydrated = useChat((chat) => chat.hydrated)
  const asking = useApprovals((approval) => approval.asking)

  const scroller = useRef<HTMLDivElement>(null)
  const foot = useRef<HTMLDivElement>(null)

  // Follow the answer down, unless the reader has gone up to look at something.
  // A transcript that yanks itself to the bottom while somebody is reading
  // earlier is worse than one that does not follow at all.
  useEffect(() => {
    const view = scroller.current
    if (!view) return
    const away = view.scrollHeight - view.scrollTop - view.clientHeight
    if (away < view.clientHeight) foot.current?.scrollIntoView({ block: 'end' })
  })

  // "Nothing has been said yet" is a claim about the log, so it waits until the
  // log has been read. Drawn before that it is a sentence that is wrong for one
  // frame at every launch of a conversation that has something in it.
  const empty = hydrated && transcript.length === 0 && !pending

  return (
    <div className={styles.transcript} ref={scroller}>
      <div className={styles.column}>
        {empty && <p className={styles.quiet}>Nothing has been said yet.</p>}

        {/* What was said and what was called, in the order the log has them, so
         * a call is drawn between the answer that asked for it and whatever the
         * model said next (#55).
         *
         * The one call being asked about is left out here and drawn below as
         * the approval instead. It is on the log already, because it is
         * recorded before anybody is asked, and a row saying it is running over
         * a row asking whether it may would be one call drawn twice, in two
         * states, neither of them the true one. `design/system.md` gives the
         * call row "awaiting approval" as a state, and the approval prompt is
         * what that state looks like. */}
        {transcript.map((moment) =>
          moment.moment === 'said' ? (
            <Bubble key={moment.seq} role={moment.role} text={moment.text} />
          ) : moment.seq === asking?.call ? null : (
            <ToolCall key={moment.seq} called={moment} />
          ),
        )}

        {/* The message that was just sent. It is on the log already; it is drawn
         * from here because the log is read back at the end of the turn and
         * whenever a call is recorded, rather than after every event. */}
        {pending && <Bubble role="user" text={pending} />}

        {/* Not while a call waits on somebody: the model is not thinking then,
         * it is stopped, and a "Thinking." over an approval row would be the
         * window saying the opposite of what is happening. */}
        {running && !asking && <Answering />}

        {/* Last, because it is the thing that just happened: every call before
         * it has an answer, and this is the one that does not. */}
        {asking && <Approval asking={asking} />}

        {failure && (
          <p className={styles.failure} role="status">
            {failure}
          </p>
        )}

        <div ref={foot} />
      </div>
    </div>
  )
}

/**
 * The answer as it is generated.
 *
 * The one component in the application that reads the token buffer, so it is
 * the only one a frame of new tokens re-renders.
 */
function Answering() {
  const streaming = useStreaming()

  if (!streaming.text && !streaming.thinking) {
    // Between the send and the first token. It says what is happening rather
    // than spinning: the tokens themselves are the progress indicator, and
    // there is no indeterminate bar on this desk (`design/shell.md`).
    return <p className={styles.quiet}>Thinking.</p>
  }

  return (
    <div className={styles.bubble} data-role="assistant">
      {streaming.thinking && <p className={styles.reasoning}>{streaming.thinking}</p>}
      <Markdown>{streaming.text}</Markdown>
    </div>
  )
}

/**
 * One message.
 *
 * `design/system.md`: user on `raised` at `--radius-island`, assistant unfilled
 * on the rack. The difference in surface is what tells them apart; there is no
 * byline and no avatar, because the two are never adjacent to anything they
 * could be confused with.
 */
function Bubble({ role, text }: { role: Said['role']; text: string }) {
  return (
    <div className={styles.bubble} data-role={role}>
      <Markdown>{text}</Markdown>
    </div>
  )
}
