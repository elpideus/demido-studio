import { useEffect, useRef, useState } from 'react'
import {
  Activity,
  ChartCandlestick,
  FolderTree,
  Globe,
  MessagesSquare,
  Settings,
  Waypoints,
  type LucideIcon,
} from 'lucide-react'

import { useDesk, type Side } from './desk'
import styles from './Rail.module.css'

/**
 * The rail: icons only, on either edge.
 *
 * Brief B42: "a VSCode-like Icons-only sidebar"
 *
 * `design/shell.md` fixes the order and the amendment that removed Sub-agents
 * from it, and `design/system.md` fixes the four states a rail item has. Only
 * the closed one is drawn here, because nothing opens yet: an item that lit up
 * for a panel that does not exist would make the rail lie about what is open,
 * and the rail is the only place in the UI that answers that question.
 */

/** The rail's default order, top to bottom, from `design/shell.md`. There is no
 * Sub-agents entry: a sub-agent is a scope on the session log rather than a
 * window, which amended the brief's own listing on #8. */
const NAVIGATION: { id: string; label: string; icon: LucideIcon }[] = [
  { id: 'chats', label: 'Chats', icon: MessagesSquare },
  { id: 'files', label: 'Files', icon: FolderTree },
  { id: 'code', label: 'Code graph', icon: Waypoints },
  { id: 'market', label: 'Market charts', icon: ChartCandlestick },
  { id: 'session', label: 'Session monitor', icon: Activity },
  { id: 'browser', label: 'Browser', icon: Globe },
]

export function Rail() {
  const rail = useDesk((desk) => desk.rail)
  const dock = useDesk((desk) => desk.dock)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)

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
          <Item key={entry.id} label={entry.label} icon={entry.icon} />
        ))}
      </ul>

      {/* Settings sits alone at the other end, per the brief's own aside. */}
      <ul className={styles.group}>
        <Item label="Settings" icon={Settings} />
      </ul>

      {menu && <Menu at={menu} rail={rail} dock={dock} close={() => setMenu(null)} />}
    </nav>
  )
}

/**
 * One rail icon, closed.
 *
 * Closed is `ink-4` with no marker (`design/system.md`). The other three states
 * arrive with the panels that can be in them, on the window manager's own
 * ticket; drawing them now would mean inventing what they report.
 */
function Item({ label, icon: Icon }: { label: string; icon: LucideIcon }) {
  // The name sits on the slot rather than on the button, because the button is
  // disabled until it has a panel to open and a disabled control reports
  // nothing on hover.
  return (
    <li className={styles.slot} title={label}>
      <button type="button" className={styles.item} aria-label={label} disabled>
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
  at: { x: number; y: number }
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
