import { useRef, useEffect } from 'react'
import { flushSync } from 'react-dom'
import { Rnd } from 'react-rnd'
import type { DraggableEvent } from 'react-draggable'
import type { DraggableData } from 'react-rnd'
import { X } from 'lucide-react'
import type { ManagedWindow } from '../../types'
import { useWindowManager } from '../../stores/windowManager'
import { getCurrentWindow } from '@tauri-apps/api/window'

const SNAP_THRESHOLD = 40

interface Props {
  window: ManagedWindow
  children: React.ReactNode
  titleBarActions?: React.ReactNode
  /** Sits right of the title, before the actions — e.g. which file the window is showing. */
  titleBarInfo?: React.ReactNode
  onSnapCandidateChange: (edge: 'left' | 'right' | null) => void
}

function detectSnapEdge(
  cursorX: number,
  snapLayout: { left: { fraction: number } | null; right: { fraction: number } | null },
): 'left' | 'right' | null {
  const appW = window.innerWidth
  const leftBoundary  = snapLayout.left  ? appW * snapLayout.left.fraction  : 0
  const rightBoundary = snapLayout.right ? appW * (1 - snapLayout.right.fraction) : appW

  if (cursorX <= leftBoundary + SNAP_THRESHOLD) return 'left'
  if (cursorX >= rightBoundary - SNAP_THRESHOLD) return 'right'

  return null
}

export function WindowFrame({ window: win, children, titleBarActions, titleBarInfo, onSnapCandidateChange }: Props) {
  const { focusWindow, closeWindow, moveWindow, resizeWindow, snapWindow, unsnapWindow, resizeSnapFraction, snapLayout } = useWindowManager()
  const snapCandidateRef = useRef<'left' | 'right' | null>(null)
  // True once we've already unsnapped for the current drag gesture
  const hasUnsnappedRef = useRef(false)

  // Release cursor grab if OS steals focus mid-drag (Alt+Tab, system popup)
  useEffect(() => {
    const release = () => getCurrentWindow().setCursorGrab(false).catch(() => {})
    window.addEventListener('blur', release)
    return () => window.removeEventListener('blur', release)
  }, [])

  // Unsnap eagerly on drag-handle mousedown using flushSync so React commits
  // the DOM update (free size) BEFORE react-draggable's mousedown handler runs
  // and measures its internal offset. Without flushSync the offset is computed
  // against the snap height (full viewport), locking y movement for the gesture.
  function handleDragHandleMouseDown(e: React.MouseEvent) {
    // Release any lingering grab before acquiring a new one (safety cleanup).
    getCurrentWindow().setCursorGrab(false)
      .then(() => getCurrentWindow().setCursorGrab(true))
      .catch(() => {})
    if (!win.snapState) return
    const snapX = win.snapState.edge === 'left' ? 0 : window.innerWidth * (1 - win.snapState.fraction)
    const snapWidth = window.innerWidth * win.snapState.fraction
    const freeWidth = win.lastFreeSize.width
    // Keep the cursor at the same fractional position across the title bar after
    // the window shrinks from snap size to free size.
    const relFraction = (e.clientX - snapX) / snapWidth
    const newX = e.clientX - relFraction * freeWidth
    flushSync(() => {
      unsnapWindow(win.id, { x: newX, y: 0 })
    })
    hasUnsnappedRef.current = true
  }

  function handleDrag(e: DraggableEvent, data: DraggableData) {
    moveWindow(win.id, { x: data.x, y: data.y })
    const candidate = detectSnapEdge((e as MouseEvent).clientX, snapLayout)
    if (candidate !== snapCandidateRef.current) {
      snapCandidateRef.current = candidate
      onSnapCandidateChange(candidate)
    }
  }

  function handleDragStop(_e: DraggableEvent, data: DraggableData) {
    getCurrentWindow().setCursorGrab(false).catch(() => {})
    hasUnsnappedRef.current = false
    const candidate = snapCandidateRef.current
    snapCandidateRef.current = null
    onSnapCandidateChange(null)

    if (candidate) {
      const ok = snapWindow(win.id, candidate, window.innerWidth)
      if (!ok) {
        moveWindow(win.id, { x: data.x, y: data.y })
      }
    } else {
      moveWindow(win.id, { x: data.x, y: data.y })
    }
  }

  // When snapped, override position and size to fill the snap slot
  const isSnapped = !!win.snapState
  const appW = window.innerWidth
  const appH = window.innerHeight
  const position = isSnapped
    ? { x: win.snapState!.edge === 'left' ? 0 : appW * (1 - win.snapState!.fraction), y: 0 }
    : win.position
  const size = isSnapped
    ? { width: appW * win.snapState!.fraction, height: appH }
    : win.size

  return (
    <Rnd
      className="pointer-events-auto"
      position={position}
      size={size}
      minWidth={280}
      minHeight={200}
      dragHandleClassName="wm-drag-handle"
      enableResizing={isSnapped ? {
        top: false, bottom: false, topRight: false, bottomRight: false, bottomLeft: false, topLeft: false,
        right: win.snapState!.edge === 'left',
        left:  win.snapState!.edge === 'right',
      } : {
        top: true, right: true, bottom: true, left: true,
        topRight: true, bottomRight: true, bottomLeft: true, topLeft: true,
      }}
      style={{ zIndex: win.zIndex }}
      onMouseDown={() => focusWindow(win.id)}
      onDrag={handleDrag}
      onDragStop={handleDragStop}
      onResize={(_e, _dir, ref) => {
        if (isSnapped) resizeSnapFraction(win.id, ref.offsetWidth, window.innerWidth)
      }}
      onResizeStop={(_e, _dir, ref, _delta, pos) => {
        if (isSnapped) {
          resizeSnapFraction(win.id, ref.offsetWidth, window.innerWidth)
        } else {
          resizeWindow(win.id, { width: ref.offsetWidth, height: ref.offsetHeight })
          moveWindow(win.id, pos)
        }
      }}
    >
      <div className={`flex flex-col h-full bg-card border border-border shadow-2xl overflow-hidden${isSnapped ? '' : ' rounded-xl'}`}>
        <div className="wm-drag-handle flex items-center justify-between px-4 py-3 border-b border-border bg-[#1b1b1b] shrink-0 cursor-grab active:cursor-grabbing select-none" onMouseDown={e => handleDragHandleMouseDown(e)}>
          <span className="text-sm font-semibold text-foreground shrink-0">{win.title}</span>
          {titleBarInfo}
          <div className="flex items-center gap-1 ml-auto pl-2" onMouseDown={e => e.stopPropagation()}>
            {titleBarActions}
          <button
            className="p-1.5 rounded-md hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
            onMouseDown={e => e.stopPropagation()}
            onClick={() => closeWindow(win.id)}
          >
            <X size={14} />
          </button>
          </div>
        </div>
        <div className="flex-1 overflow-hidden">
          {children}
        </div>
      </div>
    </Rnd>
  )
}
