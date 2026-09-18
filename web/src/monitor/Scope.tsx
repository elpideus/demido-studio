import { useEffect } from 'react'
import { Bot, MessagesSquare, RotateCcw } from 'lucide-react'

import { useChat, type Limit } from '@/chat/chat'
import { Control } from '@/settings/Control'
import { useSettings, type Row } from '@/settings/ladder'
import { isMain, MAIN, scopeOf, scoped, useMonitor, type Agent, type Event } from './log'
import { ICONS, tallied, weighed } from './sources'
import styles from './Scope.module.css'

/**
 * The monitor's left column: the agents on top, the source ledger beneath.
 *
 * It is what replaced v2's Sub-Agents window, one of the rail entries v2's own
 * ledger records as never once opened. `design/windows.md`: "Sub-agents are a
 * scope on this log, not a window", so delegation is read here, beside the run
 * it belongs to, rather than in a second window over the same stream that could
 * come to disagree with it.
 *
 * Its header carries the two numbers the brief names, because they govern the
 * run the column describes: "User should be able to manually set the amount of
 * parallel agents they want to run at the same time, and how deep agents can
 * delegate one another". Both are the chat tier's, on the ladder every other
 * value is on, so this conversation is the last word and a number left alone
 * follows the global one.
 *
 * `reduced` is the monitor's stated reduced form (`design/shell.md`): the
 * column drops to icons, and the header and the ledger go with the width.
 */
export function Scope({ reduced }: { reduced: boolean }) {
  const agents = useMonitor((monitor) => monitor.agents)
  const events = useMonitor((monitor) => monitor.events)
  const scope = useMonitor((monitor) => monitor.scope)
  const scopeTo = useMonitor((monitor) => monitor.scopeTo)
  const read = useSettings((settings) => settings.read)

  // The chat tier's rows, which the composer usually reads first. Read again
  // here so the column never draws a header waiting on another surface.
  useEffect(() => {
    void read('chat')
  }, [read])

  // The conversation is always first, even before the log has been read, so the
  // way back is on screen from the moment the panel opens.
  const listed = agents.length > 0 ? agents : [MAIN_ONLY]

  if (reduced) {
    return (
      <nav className={styles.icons} aria-label="Agents">
        {listed.map((agent) => (
          <button
            key={agent.agent}
            type="button"
            className={styles.icon}
            data-delegated={!isMain(agent.agent)}
            aria-label={
              isMain(agent.agent) ? 'Main session' : `${agent.agent}: ${task(events, agent)}`
            }
            aria-current={selected(agent, scope) ? 'true' : undefined}
            title={isMain(agent.agent) ? 'Main session' : task(events, agent)}
            onClick={() => void scopeTo(scopeOf(agent.agent))}
          >
            <Glyph agent={agent} />
          </button>
        ))}
      </nav>
    )
  }

  return (
    <div className={styles.column}>
      <Header />
      <nav className={styles.agents} aria-label="Agents">
        {listed.map((agent) => (
          <button
            key={agent.agent}
            type="button"
            className={styles.agent}
            data-delegated={!isMain(agent.agent)}
            // Depth is the indent (`design/windows.md`), so the shape of the
            // chain is the shape of the list.
            style={{ '--depth': agent.depth } as React.CSSProperties}
            aria-current={selected(agent, scope) ? 'true' : undefined}
            onClick={() => void scopeTo(scopeOf(agent.agent))}
          >
            <Glyph agent={agent} />
            {isMain(agent.agent) ? (
              <span className={styles.name}>Main session</span>
            ) : (
              <>
                <span className={styles.id}>{agent.agent}</span>
                <span className={styles.task}>{task(events, agent)}</span>
              </>
            )}
          </button>
        ))}
      </nav>
      <Ledger events={scoped(events, scope)} />
    </div>
  )
}

/** The conversation, before the log has said anything. */
const MAIN_ONLY: Agent = { agent: MAIN, depth: 0, parent: null, call: null, generating: false }

/** `Main session` is the unscoped run, so it is selected when nothing is. */
function selected(agent: Agent, scope: string | null): boolean {
  return isMain(agent.agent) ? scope === null : agent.agent === scope
}

/** The conversation's icon, or the bot the transcript's delegation row carries
 * (`design/system.md`), so a sub-agent looks like itself in both places. */
function Glyph({ agent }: { agent: Agent }) {
  const Icon = isMain(agent.agent) ? MessagesSquare : Bot
  return <Icon className={styles.glyph} strokeWidth={1.8} aria-hidden />
}

/**
 * What a sub-agent was asked, read off the call that opened it.
 *
 * The call's own arguments rather than a copy on the agent, because the task is
 * what the model wrote and the log already has it once. A call whose arguments
 * do not parse still names its agent: the id is beside this on the row.
 */
function task(events: Event[], agent: Agent): string {
  const call = events.find((event) => event.seq === agent.call)
  if (typeof call?.arguments !== 'string') return ''
  try {
    const parsed: unknown = JSON.parse(call.arguments)
    if (parsed && typeof parsed === 'object' && 'task' in parsed) {
      const task = (parsed as { task: unknown }).task
      if (typeof task === 'string') return task.replace(/\s+/g, ' ').trim()
    }
  } catch {
    // A model that wrote arguments that are not JSON is recorded as it wrote
    // them, and the row falls back to the id.
  }
  return ''
}

/**
 * The header: the two numbers, and the slot strip beside them.
 *
 * Parallel agents is a preference, and the card may hold fewer. When it does,
 * the limit is drawn as the card's, in a sentence of its own under the number
 * the person set, rather than by changing that number: a field that silently
 * showed 1 after somebody typed 2 would be a setting that did not save.
 */
function Header() {
  const rows = useSettings((settings) => settings.rows.chat)
  const presence = useChat((chat) => chat.presence)

  const parallel = rows?.find((row) => row.setting.id === PARALLEL)
  const depth = rows?.find((row) => row.setting.id === DEPTH)
  const asked = typeof parallel?.value === 'number' ? parallel.value : 1

  return (
    <header className={styles.governs}>
      <h3 className={styles.heading}>Agents</h3>
      {parallel && <Knob row={parallel} label="Parallel" />}
      {presence.state === 'ready' && presence.limit && (
        <p className={styles.limit}>
          <span className={styles.held}>{`The card holds ${presence.slots}.`}</span>{' '}
          {why(presence.limit)}
        </p>
      )}
      {depth && <Knob row={depth} label="Depth" />}
      <Slots asked={asked} />
    </header>
  )
}

/** The Rust `id::PARALLEL_AGENTS` and `id::DELEGATION_DEPTH`. */
const PARALLEL = 'tools.parallel_agents'
const DEPTH = 'tools.delegation_depth'

/**
 * One of the two numbers, on the chat tier.
 *
 * The settings page's own control rather than a second number field, so the
 * range, the refusal and the reload after a slot count changes are the ones
 * every other surface has. Overridden marks the label `signal` and carries the
 * way back to global, which is `design/windows.md`'s two states for a control in
 * a chat's own settings.
 */
function Knob({ row, label }: { row: Row; label: string }) {
  const set = useSettings((settings) => settings.set)
  const clear = useSettings((settings) => settings.clear)

  return (
    <div className={styles.knob} data-overridden={row.setHere} data-following={!row.setHere}>
      <label className={styles.label} htmlFor={row.setting.id} title={row.setting.summary}>
        {label}
      </label>
      <Control row={row} onChange={(value) => set('chat', row.setting, value)} />
      {row.setHere && (
        <button
          type="button"
          className={styles.revert}
          aria-label={`${row.setting.title}: back to global`}
          title="Back to global"
          onClick={() => void clear('chat', row.setting)}
        >
          <RotateCcw className={styles.glyph} strokeWidth={1.8} aria-hidden />
        </button>
      )}
    </div>
  )
}

/** Why the card held fewer slots than were asked for, in this window's words.
 * Rust carries the fact and the numbers, and the sentence is written here. */
function why(limit: Limit): string {
  switch (limit.queued) {
    case 'no-room':
      return `${mib(limit.free)} free, and one more slot needs ${mib(limit.needed)}.`
    case 'unmeasured':
      return 'What one more slot costs on this model has not been measured, so none is opened on a guess.'
  }
}

function mib(bytes: number): string {
  return `${Math.round(bytes / (1024 * 1024))} MiB`
}

/**
 * The slot strip: filled, queued and free, so "why is nothing happening" is
 * answered on screen (`design/windows.md`).
 *
 * **Filled** is an agent with a request out, read off the log. **Free** is an
 * open slot with nobody generating on it. **Queued** is a slot the person asked
 * for and the card did not open, which is where the work that would have run
 * on it waits. Each is a pill in its state's colour and the three are counted
 * in words beside them, because colour is never the only signal.
 *
 * Nothing is generating unless a turn is running: the log is the record of the
 * past, and a line left mid-request by a run that never finished is not a slot
 * anybody is using now.
 */
function Slots({ asked }: { asked: number }) {
  const presence = useChat((chat) => chat.presence)
  const running = useChat((chat) => chat.running)
  const agents = useMonitor((monitor) => monitor.agents)

  if (presence.state !== 'ready') {
    return <p className={styles.caption}>No model is loaded, so there are no slots.</p>
  }

  const open = presence.slots
  const generating = running ? agents.filter((agent) => agent.generating).length : 0
  const filled = Math.min(generating, open)
  const queued = Math.max(asked - open, 0)
  const free = open - filled
  const cells: ('filled' | 'free' | 'queued')[] = [
    ...Array<'filled'>(filled).fill('filled'),
    ...Array<'free'>(free).fill('free'),
    ...Array<'queued'>(queued).fill('queued'),
  ]

  return (
    <div className={styles.slots}>
      <div className={styles.strip} role="img" aria-label="Slots">
        {cells.map((state, at) => (
          <span key={at} className={styles.slot} data-state={state} />
        ))}
      </div>
      <p className={styles.caption}>{`${filled} filled, ${queued} queued, ${free} free`}</p>
    </div>
  )
}

/**
 * The source ledger: count and tokens per source, for what the scope shows.
 *
 * `design/windows.md` makes the source column "a ledger, count and tokens per
 * source, not a legend beside a search box", and scoping filters it with the
 * stream. It is summed from the rows the stream draws, so the two cannot
 * disagree about what the scope holds (`tallied`).
 */
function Ledger({ events }: { events: Event[] }) {
  return (
    <dl className={styles.ledger} aria-label="Source ledger">
      {tallied(events).map(([source, tally]) => {
        const Icon = ICONS[source]
        return (
          <div key={source} className={styles.source} data-source={source}>
            <dt className={styles.sourceName}>
              <Icon className={styles.glyph} strokeWidth={1.8} aria-hidden />
              {source}
            </dt>
            <dd className={styles.tally}>{tally.events}</dd>
            <dd className={styles.tally}>{weighed(tally.weight)}</dd>
          </div>
        )
      })}
    </dl>
  )
}
