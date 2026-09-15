import { useState } from 'react'
import { Check, CircleSlash, Minus, Unplug, type LucideIcon } from 'lucide-react'

import { useMonitor, type Assembly, type Event, type Grouped, type Placed } from './log'
import { digest, ICONS, tokens, weighed } from './sources'
import styles from './Inspector.module.css'

/**
 * The two columns beside the stream: what the model saw, and the record.
 *
 * `design/windows.md` asks the first of them for something a list of events is
 * not:
 *
 * > Selecting an event rebuilds the prompt **as it stood at that moment**,
 * > block by block, **diffed against the previous assembly**: an injection
 * > appears as an inserted block you can read, an evicted one as a struck-out
 * > block with its cost.
 *
 * Neither column computes any of that. The rebuild is
 * `demido_trace::Replay::rebuild`, over the log, because it is a projection of
 * the log and a second one written here could come to disagree with the export
 * ([#57](https://github.com/elpideus/demido-studio/issues/57): "The rebuild
 * gets no seam of its own").
 *
 * What both columns are careful about is the same thing the transcript is
 * careful about: **nothing is tidied on the way to being read.** A block is the
 * text that was sent, a tool's description is the wording it was sent in rather
 * than the wording in the register today, and the last tab is the line itself.
 */

/** The block header's word for a change, and nothing for the ordinary case:
 * every block being labelled would leave nothing for a label to mean. */
const CHANGED: Record<Placed['change'], string> = {
  unchanged: '',
  inserted: 'inserted',
  evicted: 'evicted',
}

/**
 * The assembly as it stood at the selected event.
 *
 * The edit affordance `design/windows.md` puts on each block header would open
 * the prompt editor, which is now a Settings page
 * ([#58](https://github.com/elpideus/demido-studio/issues/58)); Replay from
 * here is what makes editing worth doing, and it has nothing behind it yet.
 * Neither is drawn, because a jump into a page from a block that names no
 * paragraph is a button that cannot honour what it says, and the block that
 * would name one is the assembly's, not this ticket's.
 */
export function Assembled({ assembly }: { assembly: Assembly | null }) {
  if (!assembly) {
    return (
      <p className={styles.empty}>
        Nothing was sent at that moment. The first lines of a log are the paragraphs that went into
        a prompt before it was composed.
      </p>
    )
  }

  return (
    <div className={styles.column}>
      <header className={styles.head}>
        <h3 className={styles.title}>What the model saw</h3>
        <p className={styles.meta}>
          {`Turn ${assembly.turn}, assembled at #${assembly.seq}, sent to ${assembly.model}`}
          {assembly.previous === null
            ? ', the first assembly of this session'
            : `, against #${assembly.previous}`}
        </p>
      </header>

      {assembly.blocks.map((block) => (
        <Block key={`${block.seq}-${block.change}`} block={block} />
      ))}

      <Offered assembly={assembly} />
    </div>
  )
}

/** One block: where it came from, what it cost, whether it is new, and the
 * text itself. */
function Block({ block }: { block: Placed }) {
  const Glyph = ICONS[block.source]
  return (
    <article className={styles.block} data-source={block.source} data-change={block.change}>
      <header className={styles.bar}>
        <Glyph className={styles.icon} strokeWidth={1.8} aria-hidden />
        <span className={styles.role}>{block.role}</span>
        <span className={styles.at}>{`#${block.seq}`}</span>
        {CHANGED[block.change] && <span className={styles.change}>{CHANGED[block.change]}</span>}
        <span className={styles.weight}>{weighed(block.weight)}</span>
      </header>
      <pre className={styles.text}>{block.text}</pre>
    </article>
  )
}

/**
 * The tools that went with the assembly, and what became of every group that
 * did not.
 *
 * `docs/rules/tools.md` is what this is for: a person debugging *why did it not
 * use the file tools* must be able to tell a deliberate absence from a dropped
 * one. So a group says which it is, in words and with an icon, and the
 * descriptions listed are the wording that was actually sent rather than
 * whatever the register says today.
 */
function Offered({ assembly }: { assembly: Assembly }) {
  const offered = assembly.tools
  return (
    <section className={styles.tools}>
      <header className={styles.head}>
        <h3 className={styles.title}>Tools offered</h3>
        <p className={styles.meta}>
          {offered === null
            ? 'None. This assembly carried no tools at all.'
            : `${offered.tools.length} sent, in the set recorded at #${offered.seq} by ${offered.layer}`}
        </p>
      </header>

      {assembly.groups.map((group) => (
        <Group key={group.group} group={group} />
      ))}

      {offered?.tools.map((tool) => (
        <details key={tool.hash} className={styles.tool}>
          <summary className={styles.summary}>
            <span className={styles.name}>{tool.name}</span>
            <span className={styles.hash}>{digest(tool.hash)}</span>
          </summary>
          <pre className={styles.text}>{tool.text}</pre>
        </details>
      ))}
    </section>
  )
}

/** What one group came to, said as a word and an icon rather than as a colour
 * alone. */
const STANDING: Record<Grouped['standing'], { icon: LucideIcon; said: string }> = {
  offered: { icon: Check, said: 'offered' },
  partial: { icon: Minus, said: 'partly offered' },
  'switched-off': { icon: CircleSlash, said: 'switched off in the picker' },
  dropped: { icon: Unplug, said: 'not offered by the registry' },
  // The one that names no reason, because the log has none to name: an empty
  // set is what both a picker with everything off and a registry with nowhere
  // to act write down.
  nothing: { icon: CircleSlash, said: 'nothing was offered this turn' },
}

function Group({ group }: { group: Grouped }) {
  const { icon: Glyph, said } = STANDING[group.standing]
  return (
    <div className={styles.group} data-standing={group.standing}>
      <Glyph className={styles.icon} strokeWidth={1.8} aria-hidden />
      <span className={styles.name}>{group.group}</span>
      <span className={styles.said}>{said}</span>
      <span className={styles.count}>
        {`${group.offered.length}/${group.offered.length + group.absent.length}`}
      </span>
    </div>
  )
}

/** The tabs of the detail pane. The last one is the raw JSON, because the
 * record is the record (`design/windows.md`). */
const TABS = ['event', 'json'] as const

type Tab = (typeof TABS)[number]

/**
 * The selected line, twice: read, and as it is on disk.
 *
 * The JSON tab is the last one deliberately and it is not a debug affordance.
 * It is the thing this whole window defers to when somebody does not believe
 * it, which is why the line is stringified from what crossed the boundary
 * rather than reassembled from the fields above it.
 */
export function Detail() {
  const [tab, setTab] = useState<Tab>('event')
  // Read here rather than passed in. What this pane draws is the selection, and
  // a panel that fetched it only to hand it over would be a component in the
  // middle of two that already agree.
  const event = useSelected()

  if (!event)
    return <p className={styles.empty}>Select a row to read the line it was drawn from.</p>

  return (
    <div className={styles.column}>
      <nav className={styles.tabs} aria-label="The selected event">
        {TABS.map((name) => (
          <button
            key={name}
            type="button"
            className={styles.tab}
            aria-current={name === tab ? 'page' : undefined}
            onClick={() => setTab(name)}
          >
            {name === 'event' ? 'Event' : 'Raw JSON'}
          </button>
        ))}
      </nav>

      {tab === 'event' ? (
        <dl className={styles.fields}>
          <Field name="Event">{event.event}</Field>
          <Field name="Position">{`#${event.seq}`}</Field>
          <Field name="Turn">{event.turn === 0 ? 'the session' : `${event.turn}`}</Field>
          <Field name="Source">{event.source}</Field>
          <Field name="Weight">
            {`${tokens(event.weight.tokens)} tokens, ${event.weight.basis}`}
          </Field>
          <Field name="At">{new Date(event.at).toLocaleTimeString()}</Field>
          <Field name="Session">{event.session}</Field>
        </dl>
      ) : (
        <pre className={styles.json}>{JSON.stringify(event, null, 2)}</pre>
      )}
    </div>
  )
}

function Field({ name, children }: { name: string; children: string }) {
  return (
    <div className={styles.field}>
      <dt className={styles.label}>{name}</dt>
      <dd className={styles.value}>{children}</dd>
    </div>
  )
}

/** The event the stream has selected, or nothing. Here rather than in the
 * panel, because the pane that draws it is the one that needs it. */
export function useSelected(): Event | null {
  const events = useMonitor((monitor) => monitor.events)
  const selected = useMonitor((monitor) => monitor.selected)
  return events.find((event) => event.seq === selected) ?? null
}
