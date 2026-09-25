// The window manager's state: which windows are open, where, in which mode and in what order.
// Geometry rules live in wm/geometry.ts; this store applies them.

import { create } from 'zustand';

import {
  EVEN_SPLITS,
  type Column,
  type Mode,
  type Rect,
  type Size,
  type Slot,
  type Splits,
  SLOTS,
  clampRect,
  clampSplit,
  coveredShare,
  initialRect,
  pinWindow,
  refitDocks,
  resizeDock,
  restoreUnderPointer,
} from '@/wm/geometry';

export type WindowKind = 'settings' | 'market' | 'inspector';

export interface WindowSpec {
  title: string;
  defaultSize: Size;
  minSize: Size;
  /** Only one window of this kind can be open. */
  singleton: boolean;
  defaultDockWidth: number;
  defaultDockHeight: number;
}

export const WINDOW_SPECS: Record<WindowKind, WindowSpec> = {
  settings: {
    title: 'Settings',
    defaultSize: { w: 980, h: 680 },
    minSize: { w: 560, h: 420 },
    singleton: true,
    defaultDockWidth: 560,
    defaultDockHeight: 380,
  },
  market: {
    title: 'Market',
    defaultSize: { w: 1040, h: 660 },
    minSize: { w: 520, h: 380 },
    singleton: true,
    defaultDockWidth: 620,
    defaultDockHeight: 400,
  },
  inspector: {
    title: 'Inspector',
    defaultSize: { w: 860, h: 640 },
    minSize: { w: 480, h: 360 },
    singleton: true,
    defaultDockWidth: 560,
    defaultDockHeight: 360,
  },
};

export interface WindowState {
  id: string;
  kind: WindowKind;
  props: Record<string, unknown>;
  /** Floating geometry, kept while maximized or docked so the window can go back to it. */
  rect: Rect;
  mode: Mode;
  slot: Slot | null;
  dockWidth: number;
  dockHeight: number;
  z: number;
}

export interface SnapPreview {
  rect: Rect;
}

interface WindowsStore {
  windows: WindowState[];
  focusedId: string | null;
  bounds: Size;
  zTop: number;
  /** Share of each split column its top half takes. */
  splits: Splits;
  preview: SnapPreview | null;
  /** A docked window's edge is being dragged: every window follows it without animating. */
  resizing: boolean;
  /**
   * Opens a window, or brings forward the one that is open. `slot` pins it there; otherwise a
   * closed window comes back where it was, pinned or not.
   */
  open: (kind: WindowKind, props?: Record<string, unknown>, slot?: Slot) => string;
  /** The navigation rail's button: opens the window, brings a mostly hidden one forward, else closes it. */
  toggle: (kind: WindowKind, props?: Record<string, unknown>) => void;
  close: (id: string) => void;
  focus: (id: string) => void;
  setBounds: (bounds: Size) => void;
  setRect: (id: string, rect: Rect) => void;
  toggleMaximize: (id: string) => void;
  dock: (id: string, slot: Slot) => void;
  float: (id: string) => void;
  /** Leaves maximized/docked mode because the title bar was dragged. */
  tearOff: (id: string, px: number, py: number, grabRatio: number) => void;
  /** Width of a docked window's column, or height of its row. */
  setDockSize: (id: string, size: number) => void;
  setSplit: (column: Column, split: number) => void;
  setProps: (id: string, props: Record<string, unknown>) => void;
  setPreview: (preview: SnapPreview | null) => void;
  setResizing: (resizing: boolean) => void;
  /** Serializable layout for settings.json. */
  snapshot: () => SavedLayout;
  restore: (layout: unknown) => void;
}

type SavedWindow = Pick<WindowState, 'kind' | 'props' | 'rect' | 'mode' | 'slot' | 'dockWidth' | 'dockHeight'>;
type Remembered = Pick<WindowState, 'rect' | 'dockWidth' | 'dockHeight'> & Partial<Pick<WindowState, 'mode' | 'slot'>>;

export interface SavedLayout {
  windows: SavedWindow[];
  geometry: Partial<Record<WindowKind, Remembered>>;
  splits?: Splits;
}

/** Last geometry per kind, so a reopened window comes back where it was, pinned or not. */
const remembered: Partial<Record<WindowKind, Remembered>> = {};

function remember(w: WindowState): void {
  remembered[w.kind] = { rect: w.rect, dockWidth: w.dockWidth, dockHeight: w.dockHeight, mode: w.mode, slot: w.slot };
}

let counter = 0;

function samePlacement(a: WindowState, b: WindowState): boolean {
  return a.mode === b.mode && a.slot === b.slot && a.dockWidth === b.dockWidth && a.dockHeight === b.dockHeight;
}

function isSlot(value: unknown): value is Slot {
  return SLOTS.includes(value as Slot);
}

export const useWindows = create<WindowsStore>((set, get) => ({
  windows: [],
  focusedId: null,
  bounds: { w: 1280, h: 800 },
  zTop: 10,
  splits: EVEN_SPLITS,
  preview: null,
  resizing: false,

  open: (kind, props = {}, slot) => {
    const spec = WINDOW_SPECS[kind];
    const { windows, bounds, zTop } = get();
    const existing = spec.singleton ? windows.find((w) => w.kind === kind) : undefined;
    if (existing) {
      const raised = windows.map((w) =>
        w.id === existing.id ? { ...w, props: { ...w.props, ...props }, z: zTop + 1 } : w,
      );
      set({
        windows: slot ? pinWindow(raised, existing.id, slot, bounds) : raised,
        focusedId: existing.id,
        zTop: zTop + 1,
      });
      return existing.id;
    }
    const saved = remembered[kind];
    const rect = saved
      ? clampRect(saved.rect, bounds, spec.minSize)
      : initialRect(
          spec.defaultSize,
          bounds,
          windows.map((w) => w.rect),
        );
    counter += 1;
    const id = `${kind}-${counter}`;
    const win: WindowState = {
      id,
      kind,
      props,
      rect,
      mode: saved?.mode === 'maximized' ? 'maximized' : 'floating',
      slot: null,
      dockWidth: saved?.dockWidth ?? spec.defaultDockWidth,
      dockHeight: saved?.dockHeight ?? spec.defaultDockHeight,
      z: zTop + 1,
    };
    let next = [...windows, win];
    if (slot) {
      next = pinWindow(next, id, slot, bounds);
    } else if (saved?.mode === 'docked' && saved.slot) {
      // Back into the slot it was pinned to, unless that would move or shrink another window.
      const pinned = pinWindow(next, id, saved.slot, bounds);
      const undisturbed = pinned.every((w, i) => w.id === id || samePlacement(w, next[i]!));
      if (undisturbed) next = pinned;
    }
    set({ windows: next, focusedId: id, zTop: zTop + 1 });
    return id;
  },

  toggle: (kind, props) => {
    const { windows, bounds, splits } = get();
    const win = windows.find((w) => w.kind === kind);
    if (!win) get().open(kind, props);
    // Mostly out of sight behind other windows: the click is taken as "show me", not "close".
    else if (windows.some((w) => w.z > win.z) && coveredShare(windows, win.id, bounds, splits) > 0.5) {
      get().focus(win.id);
    } else get().close(win.id);
  },

  close: (id) => {
    const { windows, focusedId } = get();
    const win = windows.find((w) => w.id === id);
    if (win) remember(win);
    const rest = windows.filter((w) => w.id !== id);
    const nextFocus = focusedId === id ? ([...rest].sort((a, b) => b.z - a.z)[0]?.id ?? null) : focusedId;
    set({ windows: rest, focusedId: nextFocus });
  },

  focus: (id) => {
    const { windows, zTop, focusedId } = get();
    const top = windows.find((w) => w.id === id);
    if (!top || (focusedId === id && top.z === zTop)) return;
    set({
      windows: windows.map((w) => (w.id === id ? { ...w, z: zTop + 1 } : w)),
      focusedId: id,
      zTop: zTop + 1,
    });
  },

  setBounds: (bounds) => {
    if (bounds.w <= 0 || bounds.h <= 0) return;
    set(({ windows, splits }) => ({
      bounds,
      splits: { left: clampSplit(splits.left, bounds), right: clampSplit(splits.right, bounds) },
      windows: refitDocks(
        windows.map((w) => ({ ...w, rect: clampRect(w.rect, bounds, WINDOW_SPECS[w.kind].minSize) })),
        bounds,
      ),
    }));
  },

  setRect: (id, rect) =>
    set(({ windows, bounds }) => ({
      windows: windows.map((w) =>
        w.id === id ? { ...w, rect: clampRect(rect, bounds, WINDOW_SPECS[w.kind].minSize) } : w,
      ),
    })),

  toggleMaximize: (id) =>
    set(({ windows }) => ({
      windows: windows.map((w) =>
        w.id === id ? { ...w, mode: w.mode === 'maximized' ? 'floating' : 'maximized', slot: null } : w,
      ),
    })),

  dock: (id, slot) => set(({ windows, bounds }) => ({ windows: pinWindow(windows, id, slot, bounds) })),

  float: (id) =>
    set(({ windows }) => ({
      windows: windows.map((w) => (w.id === id ? { ...w, mode: 'floating', slot: null } : w)),
    })),

  tearOff: (id, px, py, grabRatio) =>
    set(({ windows, bounds }) => ({
      windows: windows.map((w) =>
        w.id === id
          ? {
              ...w,
              mode: 'floating',
              slot: null,
              rect: clampRect(restoreUnderPointer(w.rect, px, py, grabRatio), bounds, WINDOW_SPECS[w.kind].minSize),
            }
          : w,
      ),
    })),

  setDockSize: (id, size) => set(({ windows, bounds }) => ({ windows: resizeDock(windows, id, size, bounds) })),

  setSplit: (column, split) =>
    set(({ splits, bounds }) => ({ splits: { ...splits, [column]: clampSplit(split, bounds) } })),

  setProps: (id, props) =>
    set(({ windows }) => ({
      windows: windows.map((w) => (w.id === id ? { ...w, props: { ...w.props, ...props } } : w)),
    })),

  setPreview: (preview) => set({ preview }),

  setResizing: (resizing) => set({ resizing }),

  snapshot: () => {
    const { windows, splits } = get();
    for (const w of windows) remember(w);
    return {
      windows: [...windows]
        .sort((a, b) => a.z - b.z)
        // The inspector shows one message's trace; reopening it after a restart would be noise.
        .filter((w) => w.kind !== 'inspector')
        .map(({ kind, props, rect, mode, slot, dockWidth, dockHeight }) => ({
          kind,
          props,
          rect,
          mode,
          slot,
          dockWidth,
          dockHeight,
        })),
      geometry: { ...remembered },
      splits,
    };
  },

  restore: (layout) => {
    if (!layout || typeof layout !== 'object') return;
    const saved = layout as Partial<SavedLayout>;
    for (const [kind, geometry] of Object.entries(saved.geometry ?? {})) {
      if (!(kind in WINDOW_SPECS) || !geometry) continue;
      const spec = WINDOW_SPECS[kind as WindowKind];
      remembered[kind as WindowKind] = {
        rect: geometry.rect,
        dockWidth: geometry.dockWidth ?? spec.defaultDockWidth,
        dockHeight: geometry.dockHeight ?? spec.defaultDockHeight,
        mode: geometry.mode,
        slot: isSlot(geometry.slot) ? geometry.slot : null,
      };
    }
    const { bounds } = get();
    let z = 10;
    const windows: WindowState[] = (saved.windows ?? [])
      .filter((w) => w && w.kind in WINDOW_SPECS)
      .map((w) => {
        counter += 1;
        z += 1;
        const spec = WINDOW_SPECS[w.kind];
        // Layouts saved before slots existed call the slot `side`.
        const slot = [w.slot, (w as { side?: unknown }).side].find(isSlot) ?? null;
        const mode = w.mode === 'docked' && !slot ? 'floating' : (w.mode ?? 'floating');
        return {
          id: `${w.kind}-${counter}`,
          kind: w.kind,
          props: w.props ?? {},
          rect: clampRect(w.rect, bounds, spec.minSize),
          mode,
          slot: mode === 'docked' ? slot : null,
          dockWidth: w.dockWidth ?? spec.defaultDockWidth,
          dockHeight: w.dockHeight ?? spec.defaultDockHeight,
          z,
        };
      });
    const split = (value: unknown) => (typeof value === 'number' && value > 0 && value < 1 ? value : 0.5);
    const splits = { left: split(saved.splits?.left), right: split(saved.splits?.right) };
    // Docked sizes and splits are fitted by setBounds once the desktop has measured itself.
    set({ windows, splits, zTop: z, focusedId: windows[windows.length - 1]?.id ?? null });
  },
}));
