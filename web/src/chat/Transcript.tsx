import { useEffect, useRef } from 'react'

import { Markdown } from './Markdown'
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

        {transcript.map((said) => (
          <Bubble key={said.seq} role={said.role} text={said.text} />
        ))}

        {/* The message that was just sent. It is on the log already; it is drawn
         * from here because the log is read back once, at the end of the turn,
         * rather than after every event. */}
        {pending && <Bubble role="user" text={pending} />}

        {running && <Answering />}

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
