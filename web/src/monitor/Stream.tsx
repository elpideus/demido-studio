import { MAIN, scoped, useMonitor, type Event } from './log'
import { digest, ICONS, weighed } from './sources'
import styles from './Stream.module.css'

/**
 * The log, grouped into turns.
 *
 * `design/windows.md` keeps three things from v2's third attempt, and this is
 * the second of them: "A stream grouped into turns, with chunk runs folded into
 * the message they assembled."
 *
 * **The fold is the log's own, and that is the point.** Tokens arrive on
 * `chat://update` while a turn runs and never become events: what is recorded
 * is the `turn/completion` they assembled, once, with the answer on it. So a
 * run reads as a conversation here because the record is a conversation, rather
 * than because this component threw a firehose away. A monitor that had to fold
 * would be a monitor whose fold could disagree with the export's.
 *
 * A call and its result stay two rows. The transcript pairs them into one
 * (`web/src/chat/ToolCall.tsx`), because what a reader wants there is what one
 * call did; here they are two events with their own source and their own
 * weight, which is what the ledger and the cost axis are counted from.
 *
 * Turn 0 is what belongs to the session rather than to an exchange.
 */

export function Stream() {
  const all = useMonitor((monitor) => monitor.events)
  const agents = useMonitor((monitor) => monitor.agents)
  const scope = useMonitor((monitor) => monitor.scope)
  const selected = useMonitor((monitor) => monitor.selected)
  const select = useMonitor((monitor) => monitor.select)

  // Selecting an agent filters the stream to its events
  // ([#68](https://github.com/elpideus/demido-studio/issues/68)). Unscoped, a
  // sub-agent's rows stay where they happened in the run and carry its name in
  // the delegated colour, indented by its depth, so the shape of the chain
  // reads here the way it reads in the column.
  const events = scoped(all, scope)
  const depths = new Map(agents.map((agent) => [agent.agent, agent.depth]))

  if (events.length === 0) {
    return (
      <p className={styles.empty}>
        Nothing has been recorded yet. The log fills as soon as a turn is sent.
      </p>
    )
  }

  return (
    <div className={styles.stream}>
      {turns(events).map((turn) => (
        <section key={turn.number} className={styles.turn}>
          <h3 className={styles.number}>{turn.number === 0 ? 'Session' : `Turn ${turn.number}`}</h3>
          {turn.events.map((event) => (
            <button
              key={event.seq}
              type="button"
              className={styles.row}
              data-source={event.source}
              style={
                scope === null
                  ? ({ '--depth': depths.get(event.agent) ?? 0 } as React.CSSProperties)
                  : undefined
              }
              aria-current={event.seq === selected ? 'true' : undefined}
              onClick={() => void select(event.seq)}
            >
              <Mark event={event} />
              <span className={styles.kind}>{event.event}</span>
              <span className={styles.agent}>
                {scope === null && event.agent !== MAIN ? event.agent : ''}
              </span>
              <span className={styles.said}>{summary(event)}</span>
              {/* The number alone. The bar beside it is the cost axis, which is
               * not in this slice. */}
              <span className={styles.weight}>{weighed(event.weight)}</span>
            </button>
          ))}
        </section>
      ))}
    </div>
  )
}

/** The row's source as an icon as well as a colour, which is the rule every
 * row in this window follows. */
function Mark({ event }: { event: Event }) {
  const Glyph = ICONS[event.source]
  return <Glyph className={styles.icon} strokeWidth={1.8} aria-hidden />
}

/** The events of one exchange, in the order they happened. */
type Turn = { number: number; events: Event[] }

/** The log grouped by turn, keeping the log's own order. Turns are numbered
 * from one and events arrive in order, so this is a fold rather than a sort. */
function turns(events: Event[]): Turn[] {
  const turns: Turn[] = []
  for (const event of events) {
    const last = turns.at(-1)
    if (last?.number === event.turn) last.events.push(event)
    else turns.push({ number: event.turn, events: [event] })
  }
  return turns
}

/** A field of a line, when it is there and is text. A kind this build has
 * never heard of still draws as a row, so nothing here may assume a shape. */
function text(event: Event, field: string): string {
  const value = event[field]
  return typeof value === 'string' ? value : ''
}

/** A field of a line said as text, when it is there and is a number. */
function numeric(event: Event, field: string): string {
  const value = event[field]
  return typeof value === 'number' ? String(value) : ''
}

/** How many things are in a field that is a list. */
function count(event: Event, field: string): number {
  const value = event[field]
  return Array.isArray(value) ? value.length : 0
}

/**
 * One line of the log, said in one line.
 *
 * Every kind the log has, because a row with nothing on it is a row that has to
 * be clicked to be read, and the stream is the thing somebody scans. The event
 * name is beside this on the row, so this says **what**, never what kind: the
 * two together are "chat/message" and what was typed.
 *
 * A fragment and a refusal are the interesting case: what they carry is a hash
 * and the values that filled it, never the text they produced, so what a row
 * can honestly say is which wording it was. The rebuild is where that paragraph
 * is filled in and read.
 */
function summary(event: Event): string {
  switch (event.event) {
    case 'prompt/version':
      return once(text(event, 'id'))
    case 'tool/version':
      return once(text(event, 'name'))
    case 'tools/offered':
      return `${count(event, 'tools')} tools, ${text(event, 'layer')}`
    case 'prompt/fragment':
    case 'tool/refusal':
      return digest(text(event, 'hash'))
    case 'chat/message':
      return once(text(event, 'text'))
    case 'turn/parameters':
      return once(text(event, 'model'))
    case 'turn/assembly':
      return `${count(event, 'blocks')} blocks`
    case 'turn/completion':
      return once(text(event, 'text')) || once(text(event, 'reason'))
    case 'tool/call':
      return once(`${text(event, 'name')} ${text(event, 'arguments')}`)
    case 'tool/decision':
      return once(text(event, 'decision'))
    // `child` rather than `agent`: the line's own `agent` is the parent that
    // wrote it, and the child it opened is a field of the body (#67).
    case 'agent/delegated':
      return `${text(event, 'child')}, depth ${numeric(event, 'depth')}`
    case 'agent/returned':
      return once(text(event, 'child'))
    case 'tool/result':
      return once(text(event, 'text'))
    case 'turn/failure':
      return once(`${text(event, 'kind')}: ${text(event, 'detail')}`)
    default:
      return ''
  }
}

/** Whatever it is, on one line. The row is one line tall and a result is not,
 * so the newlines are collapsed here rather than clipped by the box. */
function once(said: string): string {
  return said.replace(/\s+/g, ' ').trim()
}
