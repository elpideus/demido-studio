import { describe, it, expect, beforeEach } from 'vitest'
import { useWindowManager } from './windowManager'

function store() { return useWindowManager.getState() }

function reset() {
  useWindowManager.setState({
    windows: {},
    snapLayout: { left: null, right: null },
    nextZIndex: 100,
  })
}

describe('openWindow', () => {
  beforeEach(reset)

  it('creates a window with correct defaults', () => {
    store().openWindow('settings', 'settings', 'Settings')
    const win = store().windows['settings']
    expect(win).toBeDefined()
    expect(win.component).toBe('settings')
    expect(win.title).toBe('Settings')
    expect(win.snapState).toBeNull()
    expect(win.zIndex).toBe(100)
  })

  it('focuses existing window instead of creating duplicate', () => {
    store().openWindow('settings', 'settings', 'Settings')
    const zBefore = store().windows['settings'].zIndex
    store().openWindow('settings', 'settings', 'Settings')
    expect(Object.keys(store().windows)).toHaveLength(1)
    expect(store().windows['settings'].zIndex).toBeGreaterThan(zBefore)
  })

  it('increments zIndex for each new window', () => {
    store().openWindow('a', 'settings', 'A')
    store().openWindow('b', 'settings', 'B')
    expect(store().windows['a'].zIndex).toBe(100)
    expect(store().windows['b'].zIndex).toBe(101)
  })
})

describe('closeWindow', () => {
  beforeEach(reset)

  it('removes the window from the registry', () => {
    store().openWindow('settings', 'settings', 'Settings')
    store().closeWindow('settings')
    expect(store().windows['settings']).toBeUndefined()
  })

  it('clears the snap slot when the snapped window is closed', () => {
    store().openWindow('settings', 'settings', 'Settings')
    store().snapWindow('settings', 'left', 1200)
    store().closeWindow('settings')
    expect(store().snapLayout.left).toBeNull()
  })
})

describe('focusWindow', () => {
  beforeEach(reset)

  it('assigns the next zIndex to the focused window', () => {
    store().openWindow('a', 'settings', 'A')
    store().openWindow('b', 'settings', 'B')
    store().focusWindow('a')
    expect(store().windows['a'].zIndex).toBeGreaterThan(store().windows['b'].zIndex)
  })
})

describe('moveWindow', () => {
  beforeEach(reset)

  it('updates position and lastFreePosition', () => {
    store().openWindow('settings', 'settings', 'Settings')
    store().moveWindow('settings', { x: 200, y: 150 })
    expect(store().windows['settings'].position).toEqual({ x: 200, y: 150 })
    expect(store().windows['settings'].lastFreePosition).toEqual({ x: 200, y: 150 })
  })
})

describe('resizeWindow', () => {
  beforeEach(reset)

  it('updates size and lastFreeSize', () => {
    store().openWindow('settings', 'settings', 'Settings')
    store().resizeWindow('settings', { width: 500, height: 400 })
    expect(store().windows['settings'].size).toEqual({ width: 500, height: 400 })
    expect(store().windows['settings'].lastFreeSize).toEqual({ width: 500, height: 400 })
  })
})

describe('snapWindow', () => {
  beforeEach(reset)

  it('snaps to the left edge and updates snap layout', () => {
    store().openWindow('settings', 'settings', 'Settings')
    const ok = store().snapWindow('settings', 'left', 1200)
    expect(ok).toBe(true)
    expect(store().snapLayout.left?.windowId).toBe('settings')
    expect(store().windows['settings'].snapState?.edge).toBe('left')
  })

  it('snaps to the right edge', () => {
    store().openWindow('settings', 'settings', 'Settings')
    const ok = store().snapWindow('settings', 'right', 1200)
    expect(ok).toBe(true)
    expect(store().snapLayout.right?.windowId).toBe('settings')
  })

  it('rejects snap when chat would be narrower than MIN_CHAT_WIDTH', () => {
    store().openWindow('a', 'settings', 'A')
    store().openWindow('b', 'settings', 'B')
    store().snapWindow('a', 'left', 1200)   // left=600px, chat=600px, ok
    const ok = store().snapWindow('b', 'right', 800) // right=400px, chat=0px, rejected
    expect(ok).toBe(false)
    expect(store().snapLayout.right).toBeNull()
  })

  it('clears the opposite slot if the same window was previously snapped there', () => {
    store().openWindow('settings', 'settings', 'Settings')
    store().snapWindow('settings', 'left', 1200)
    store().snapWindow('settings', 'right', 1200)
    expect(store().snapLayout.left).toBeNull()
    expect(store().snapLayout.right?.windowId).toBe('settings')
  })
})

describe('unsnapWindow', () => {
  beforeEach(reset)

  it('clears snapState and restores lastFreePosition/Size', () => {
    store().openWindow('settings', 'settings', 'Settings')
    store().moveWindow('settings', { x: 200, y: 150 })
    store().resizeWindow('settings', { width: 500, height: 400 })
    store().snapWindow('settings', 'left', 1200)
    store().unsnapWindow('settings')
    const win = store().windows['settings']
    expect(win.snapState).toBeNull()
    expect(win.position).toEqual({ x: 200, y: 150 })
    expect(win.size).toEqual({ width: 500, height: 400 })
    expect(store().snapLayout.left).toBeNull()
  })
})
