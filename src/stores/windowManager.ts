import { create } from 'zustand'
import type { ManagedWindow, SnapLayout, WindowComponent } from '../types'

const MIN_CHAT_WIDTH = 400
const DEFAULT_SIZE = { width: 720, height: 600 }
const INITIAL_Z = 100
const DEFAULT_SNAP_FRACTION = 0.5

interface WindowManagerState {
  windows: Record<string, ManagedWindow>
  snapLayout: SnapLayout
  nextZIndex: number

  openWindow(
    id: string,
    component: WindowComponent,
    title: string,
    opts?: { initialSize?: { width: number; height: number } }
  ): void
  closeWindow(id: string): void
  focusWindow(id: string): void
  moveWindow(id: string, position: { x: number; y: number }): void
  resizeWindow(id: string, size: { width: number; height: number }): void
  /** Returns false if the snap would leave the chat narrower than MIN_CHAT_WIDTH. */
  snapWindow(id: string, edge: 'left' | 'right', appWidth: number): boolean
  /** Clears snap state and restores the free position/size. Pass overridePosition to
   *  set the restored position directly (used when dragging from snap to avoid teleport). */
  unsnapWindow(id: string, overridePosition?: { x: number; y: number }): void
  /** Updates the fraction of a snapped window by resizing its inner edge. Returns false
   *  if the new fraction would leave the chat narrower than MIN_CHAT_WIDTH. */
  resizeSnapFraction(id: string, newWidth: number, appWidth: number): boolean
}

export const useWindowManager = create<WindowManagerState>((set, get) => ({
  windows: {},
  snapLayout: { left: null, right: null },
  nextZIndex: INITIAL_Z,

  openWindow(id, component, title, opts) {
    const { windows, nextZIndex, focusWindow } = get()
    if (windows[id]) {
      focusWindow(id)
      return
    }
    const size = opts?.initialSize ?? DEFAULT_SIZE
    const position = {
      x: Math.max(0, Math.round((window.innerWidth - size.width) / 2)),
      y: Math.max(0, Math.round((window.innerHeight - size.height) / 2)),
    }
    set(s => ({
      windows: {
        ...s.windows,
        [id]: {
          id, title, component, position, size,
          zIndex: nextZIndex,
          snapState: null,
          lastFreePosition: position,
          lastFreeSize: size,
        },
      },
      nextZIndex: s.nextZIndex + 1,
    }))
  },

  closeWindow(id) {
    set(s => {
      const { [id]: _, ...rest } = s.windows
      const layout: SnapLayout = {
        left:  s.snapLayout.left?.windowId  === id ? null : s.snapLayout.left,
        right: s.snapLayout.right?.windowId === id ? null : s.snapLayout.right,
      }
      return { windows: rest, snapLayout: layout }
    })
  },

  focusWindow(id) {
    set(s => ({
      windows: { ...s.windows, [id]: { ...s.windows[id], zIndex: s.nextZIndex } },
      nextZIndex: s.nextZIndex + 1,
    }))
  },

  moveWindow(id, position) {
    set(s => ({
      windows: {
        ...s.windows,
        [id]: { ...s.windows[id], position, lastFreePosition: position },
      },
    }))
  },

  resizeWindow(id, size) {
    set(s => ({
      windows: {
        ...s.windows,
        [id]: { ...s.windows[id], size, lastFreeSize: size },
      },
    }))
  },

  snapWindow(id, edge, appWidth) {
    const { snapLayout } = get()
    const fraction = DEFAULT_SNAP_FRACTION
    // When computing remaining chat space, exclude any slot currently held by
    // this same window (it will be vacated as part of the snap operation).
    const oppositeEdge = edge === 'left' ? 'right' : 'left'
    const oppositeSlot = snapLayout[oppositeEdge]
    const otherFraction = (oppositeSlot && oppositeSlot.windowId !== id)
      ? oppositeSlot.fraction
      : 0
    const remainingChat = appWidth * (1 - fraction - otherFraction)
    if (remainingChat < MIN_CHAT_WIDTH) return false

    set(s => {
      const layout: SnapLayout = {
        left:  s.snapLayout.left?.windowId  === id ? null : s.snapLayout.left,
        right: s.snapLayout.right?.windowId === id ? null : s.snapLayout.right,
      }
      layout[edge] = { windowId: id, fraction }
      return {
        windows: {
          ...s.windows,
          [id]: { ...s.windows[id], snapState: { edge, fraction } },
        },
        snapLayout: layout,
      }
    })
    return true
  },

  resizeSnapFraction(id, newWidth, appWidth) {
    const { windows, snapLayout } = get()
    const win = windows[id]
    if (!win?.snapState) return false
    const { edge } = win.snapState
    const fraction = newWidth / appWidth
    const oppositeEdge = edge === 'left' ? 'right' : 'left'
    const oppositeSlot = snapLayout[oppositeEdge]
    const otherFraction = (oppositeSlot && oppositeSlot.windowId !== id) ? oppositeSlot.fraction : 0
    if (appWidth * (1 - fraction - otherFraction) < MIN_CHAT_WIDTH) return false

    set(s => ({
      windows: {
        ...s.windows,
        [id]: { ...s.windows[id], snapState: { edge, fraction } },
      },
      snapLayout: {
        ...s.snapLayout,
        [edge]: { windowId: id, fraction },
      },
    }))
    return true
  },

  unsnapWindow(id, overridePosition) {
    set(s => {
      const win = s.windows[id]
      if (!win) return s
      return {
        windows: {
          ...s.windows,
          [id]: {
            ...win,
            snapState: null,
            position: overridePosition ?? win.lastFreePosition,
            size: win.lastFreeSize,
          },
        },
        snapLayout: {
          left:  s.snapLayout.left?.windowId  === id ? null : s.snapLayout.left,
          right: s.snapLayout.right?.windowId === id ? null : s.snapLayout.right,
        },
      }
    })
  },
}))
