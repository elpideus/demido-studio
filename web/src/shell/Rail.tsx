import { useEffect, useRef, useState } from 'react'
import {
  Activity,
  Boxes,
  ChartCandlestick,
  FolderTree,
  Globe,
  MessagesSquare,
  Settings,
  Waypoints,
  type LucideIcon,
} from 'lucide-react'

import { useDesk, type Panel, type Side } from './desk'
import styles from './Rail.module.css'

/**
 * The rail: icons only, on either edge.
 *
 * Brief B42: "a VSCode-like Icons-only sidebar"
 *
 * `design/shell.md` fixes the order and the amendment that removed Sub-agents
 * from it, and `design/system.md` fixes the four states a rail item has. Two of
 * them are drawn: closed, and open and focused, which is what Settings is now
 * that there is a panel behind it. The other two need a panel that can be open
 * without being focused and one that can be pinned, and both of those are the
 * window manager's. An item with nothing to open stays disabled, because an
 * item that lit up for a panel that does not exist would make the rail lie
 * about what is open, and the rail is the only place in the UI that answers
 * that question.
 */

/** The rail's default order, top to bottom, from `design/shell.md`. There is no
 * Sub-agents entry: a sub-agent is a scope on the session log rather than a
 * window, which amended the brief's own listing on #8. */
const NAVIGATION: { label: string; icon: LucideIcon; panel?: Pinned; floats?: Panel }[] = [
  { label: 'Chats', icon: MessagesSquare },
  { label: 'Files', icon: FolderTree },
  { label: 'Code graph', icon: Waypoints },
  { label: 'Market charts', icon: ChartCandlestick },
  // The one entry with something behind it
  // ([#57](https://github.com/elpideus/demido-studio/issues/57)). It names the
  // panel rather than being matched by its words, so the row and the store
  // cannot come apart over a rename, and the second panel to arrive is another
  // field rather than another comparison.
  { label: 'Session monitor', icon: Activity, panel: 'monitor' },
  { label: 'Browser', icon: Globe },
  // Also opened from the composer, which is a way in rather than a report
  // (`design/shell.md`, amended on #75).
  { label: 'Models', icon: Boxes, floats: 'models' },
]

/** A panel the rail opens pinned. One so far, and the rail is the only place
 * in the UI that reports what is open (`design/shell.md`). */
type Pinned = 'monitor'

/** Where a menu was raised. The rail's own coordinates, in the viewport. */
type At = { x: number; y: number }

export function Rail() {
  const rail = useDesk((desk) => desk.rail)
  const dock = useDesk((desk) => desk.dock)
  const panel = useDesk((desk) => desk.panel)
  const toggle = useDesk((desk) => desk.toggle)
  const monitor = useDesk((desk) => desk.monitor)
  const toggleMonitor = useDesk((desk) => desk.toggleMonitor)
  const [menu, setMenu] = useState<At | null>(null)
  const floating = (which: Panel) => () => toggle(which)

  return (
    <nav
      className={styles.rail}
      aria-label="Navigation"
      // Right-clicking empty rail space opens the rail's own menu
      // (`design/shell.md`), so the webview's is refused rather than stacked
      // under it.
      onContextMenu={(event) => {
        event.preventDefault()
        setMenu({ x: event.clientX, y: event.clientY })
      }}
    >
      <ul className={styles.group}>
        {NAVIGATION.map((entry) => (
          <Item
            key={entry.label}
            label={entry.label}
            icon={entry.icon}
            // The rail says only that a pinned panel is open. The pinned state
            // has its own marker in `design/system.md` and drawing it is the
            // window manager's, which is also what will make a panel able to be
            // open without being focused.
            open={entry.panel ? monitor : entry.floats ? panel === entry.floats : undefined}
            onOpen={entry.panel ? toggleMonitor : entry.floats ? floating(entry.floats) : undefined}
          />
        ))}
      </ul>

      {/* Settings sits alone at the other end, per the brief's own aside. It is
       * the one entry with something behind it, so it is the one that can
       * report a state other than closed. */}
      <ul className={styles.group}>
        <Item
          label="Settings"
          icon={Settings}
          open={panel === 'settings'}
          onOpen={() => toggle('settings')}
        />
      </ul>

      {menu && <Menu at={menu} rail={rail} dock={dock} close={() => setMenu(null)} />}
    </nav>
  )
}

/**
 * One rail icon.
 *
 * Closed is `ink-4` with no marker; open and focused is `signal` on `edge` with
 * an 18px `signal` tick (`design/system.md`). The other two states, open but
 * not focused and pinned, arrive with the panels that can be in them, on the
 * window manager's own ticket; drawing them now would mean inventing what they
 * report.
 *
 * An item with no `onOpen` has nothing to open and is disabled.
 */
function Item({
  label,
  icon: Icon,
  open = false,
  onOpen,
}: {
  label: string
  icon: LucideIcon
  open?: boolean
  onOpen?: () => void
}) {
  // The name sits on the slot rather than on the button, because most items are
  // still disabled and a disabled control reports nothing on hover.
  //
  // `title` is the browser's tooltip and design/system.md specifies Demido's
  // own, revealed after --delay-keycap on a panel face. This is a stand-in
  // until that component exists: a rail of unlabelled icons that says nothing
  // on hover is worse than one that says it in the wrong shape.
  return (
    <li className={styles.slot} title={label} data-open={open}>
      <button
        type="button"
        className={styles.item}
        aria-label={label}
        aria-pressed={onOpen ? open : undefined}
        disabled={!onOpen}
        onClick={onOpen}
      >
        <Icon className={styles.icon} strokeWidth={1.8} aria-hidden />
      </button>
    </li>
  )
}

/**
 * The rail's context menu.
 *
 * Transient chrome, not a panel: `panel` with `shadow-float` at
 * `--radius-control`, the same shape `design/system.md` gives the snap menu,
 * because it is the same kind of thing and a second shape for it would be a
 * second thing to keep in step.
 *
 * Edit Navbar belongs in this menu too (`design/shell.md`) and is not here:
 * reordering the rail is its own gesture and its own ticket, and a row that
 * does nothing is worse than a row that is not there yet.
 */
function Menu({
  at,
  rail,
  dock,
  close,
}: {
  at: At
  rail: Side
  dock: (side: Side) => void
  close: () => void
}) {
  const menu = useRef<HTMLDivElement>(null)

  useEffect(() => {
    // The first row the user can actually take, rather than the menu itself: a
    // ring around the whole popover says the container is the thing being
    // operated, and the row is.
    menu.current?.querySelector<HTMLButtonElement>('button:not([disabled])')?.focus()
    const dismiss = (event: MouseEvent) => {
      if (!menu.current?.contains(event.target as Node)) close()
    }
    // Captured, so a click that lands on a control still closes the menu
    // before that control acts on it.
    document.addEventListener('mousedown', dismiss, true)
    return () => document.removeEventListener('mousedown', dismiss, true)
  }, [close])

  return (
    <div
      ref={menu}
      className={styles.menu}
      role="menu"
      // Opened away from the edge the rail is on, so a menu raised at the right
      // hand side of the window does not run off it. Nothing in this
      // application scrolls sideways, so the alternative is clipping it.
      data-flip={rail === 'right'}
      style={{ left: at.x, top: at.y }}
      onKeyDown={(event) => {
        if (event.key === 'Escape') close()
      }}
    >
      {(['left', 'right'] as const).map((side) => (
        <button
          key={side}
          type="button"
          role="menuitem"
          className={styles.row}
          disabled={rail === side}
          onClick={() => {
            dock(side)
            close()
          }}
        >
          {side === 'left' ? 'Dock left' : 'Dock right'}
        </button>
      ))}
    </div>
  )
}
