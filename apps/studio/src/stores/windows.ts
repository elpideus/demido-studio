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
  capDocks,
  clampRect,
  clampSplit,
  coveredShare,
  initialRect,
  pinWindow,
  refitDocks,
  resizeDock,
  restoreUnderPointer,
  sideOf,
} from '@/wm/geometry';

export type WindowKind = 'settings' | 'market' | 'inspector';

export interface WindowSpec {
  title: string;
  defaultSize: Size;
  minSize: Size;
  /** Largest it can be resized to, floating or pinned: past this its content stretches apart. */
  maxSize: Size;
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
    maxSize: { w: 1280, h: 960 },
    singleton: true,
    defaultDockWidth: 560,
    defaultDockHeight: 380,
  },
  market: {
    title: 'Market',
    defaultSize: { w: 1040, h: 660 },
    minSize: { w: 520, h: 380 },
    maxSize: { w: 1920, h: 1200 },
    singleton: true,
    defaultDockWidth: 620,
    defaultDockHeight: 400,
  },
  inspector: {
    title: 'Inspector',
    defaultSize: { w: 860, h: 640 },
    minSize: { w: 480, h: 360 },
    maxSize: { w: 1400, h: 1080 },
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
  /** Where it is pinned; kept while maximized from a pin, so Restore can go back to it. */
  slot: Slot | null;
  dockWidth: number;
  dockHeight: number;
  /**
   * The geometry the person gave it. `rect` and the dock sizes are fitted into the desktop from
   * this, so a desktop that is smaller for a moment (the app window before it is maximized or
   * snapped) squeezes the window without losing its size. Only what the person does changes it.
   */
  wanted: Geometry;
  /**
   * When its column or row was last sized, by pinning it or dragging its edge (0: not yet). Of two
   * sides that no longer both fit, the one sized last keeps its size.
   */
  sizedAt: number;
  z: number;
}

export type Geometry = Pick<WindowState, 'rect' | 'dockWidth' | 'dockHeight'>;

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
  /** The shares the person set, which `splits` is fitted from like a window's `wanted`. */
  wantedSplits: Splits;
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
  remembered[w.kind] = { ...w.wanted, mode: w.mode, slot: w.slot };
}

let counter = 0;

function samePlacement(a: WindowState, b: WindowState): boolean {
  return a.mode === b.mode && a.slot === b.slot && a.dockWidth === b.dockWidth && a.dockHeight === b.dockHeight;
}

const maxOf = (w: WindowState) => WINDOW_SPECS[w.kind].maxSize;

/** No desktop to fit into: only a kind's own size limits apply. */
const ANY_DESKTOP: Size = { w: Infinity, h: Infinity };

/** A floating rect kept on the desktop and within its kind's size limits. */
function fitRect(kind: WindowKind, rect: Rect, bounds: Size): Rect {
  return clampRect(rect, bounds, WINDOW_SPECS[kind].minSize, WINDOW_SPECS[kind].maxSize);
}

/**
 * What every window shows: its `wanted` geometry fitted into the desktop. Every action that
 * changes a placement ends here, so what is on screen is always this function of what is wanted
 * and a smaller desktop takes nothing away for good.
 */
function fitToDesktop(windows: WindowState[], bounds: Size): WindowState[] {
  const wanted = windows.map((w) => ({ ...w, ...w.wanted, rect: fitRect(w.kind, w.wanted.rect, bounds) }));
  return capDocks(refitDocks(wanted, bounds), maxOf);
}

/**
 * The rect a window wants once the person has moved or resized it to `rect` (as shown): each of
 * its position and size they changed, and what it wanted before for the rest, since a size the
 * desktop is squeezing for now is not one they chose.
 */
function wantRect(w: WindowState, rect: Rect): Rect {
  const keep = (key: keyof Rect) => (rect[key] === w.rect[key] ? w.wanted.rect[key] : rect[key]);
  return { x: keep('x'), y: keep('y'), w: keep('w'), h: keep('h') };
}

/** Bumped each time a column or row is sized, by pinning or dragging (see `WindowState.sizedAt`). */
let sizing = 0;

/**
 * Pins a window. The pin is worked out on what the windows want, on a desktop with room for all
 * of it: a window joining a column wants the column's width, and the others keep what they want.
 * Fitting that into the real desktop is what makes the column across give way, for now.
 */
function pin(windows: WindowState[], id: string, slot: Slot, bounds: Size, sizedAt = sizing + 1): WindowState[] {
  const wanted = windows.map((w) => ({ ...w, dockWidth: w.wanted.dockWidth, dockHeight: w.wanted.dockHeight }));
  const pinned = capDocks(pinWindow(wanted, id, slot, ANY_DESKTOP), maxOf).map((w) => ({
    ...w,
    wanted: { ...w.wanted, dockWidth: w.dockWidth, dockHeight: w.dockHeight },
    sizedAt: w.id === id ? sizedAt : w.sizedAt,
  }));
  return fitToDesktop(pinned, bounds);
}

/** The layout a pin would give, for the outline shown while a drag would snap. */
export function pinPreview(windows: WindowState[], id: string, slot: Slot, bounds: Size): WindowState[] {
  return pin(windows, id, slot, bounds);
}

/** Pins a window unless that would move, shrink or unpin another one; then leaves it as it is. */
function pinUndisturbed(windows: WindowState[], id: string, slot: Slot, bounds: Size): WindowState[] {
  sizing += 1;
  const pinned = pin(windows, id, slot, bounds, sizing);
  return pinned.every((w, i) => w.id === id || samePlacement(w, windows[i]!)) ? pinned : windows;
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
  wantedSplits: EVEN_SPLITS,
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
      if (slot) sizing += 1;
      set({
        windows: slot ? pin(raised, existing.id, slot, bounds, sizing) : raised,
        focusedId: existing.id,
        zTop: zTop + 1,
      });
      return existing.id;
    }
    const saved = remembered[kind];
    const wantedRect = saved
      ? fitRect(kind, saved.rect, ANY_DESKTOP)
      : initialRect(
          spec.defaultSize,
          bounds,
          windows.map((w) => w.rect),
        );
    counter += 1;
    const id = `${kind}-${counter}`;
    const dockWidth = saved?.dockWidth ?? spec.defaultDockWidth;
    const dockHeight = saved?.dockHeight ?? spec.defaultDockHeight;
    const win: WindowState = {
      id,
      kind,
      props,
      rect: fitRect(kind, wantedRect, bounds),
      mode: saved?.mode === 'maximized' ? 'maximized' : 'floating',
      slot: saved?.mode === 'maximized' ? (saved.slot ?? null) : null,
      dockWidth,
      dockHeight,
      wanted: { rect: wantedRect, dockWidth, dockHeight },
      sizedAt: 0,
      z: zTop + 1,
    };
    let next = [...windows, win];
    if (slot) {
      sizing += 1;
      next = pin(next, id, slot, bounds, sizing);
    } else if (saved?.mode === 'docked' && saved.slot) {
      // Back into the slot it was pinned to, unless that would move or shrink another window.
      next = pinUndisturbed(next, id, saved.slot, bounds);
    }
    set({ windows: fitToDesktop(next, bounds), focusedId: id, zTop: zTop + 1 });
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
    // A column it was squeezing gets its room back.
    set({ windows: fitToDesktop(rest, get().bounds), focusedId: nextFocus });
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
    set(({ windows, wantedSplits }) => ({
      bounds,
      splits: { left: clampSplit(wantedSplits.left, bounds), right: clampSplit(wantedSplits.right, bounds) },
      windows: fitToDesktop(windows, bounds),
    }));
  },

  setRect: (id, rect) =>
    set(({ windows, bounds }) => ({
      windows: fitToDesktop(
        windows.map((w) =>
          w.id === id ? { ...w, wanted: { ...w.wanted, rect: wantRect(w, fitRect(w.kind, rect, bounds)) } } : w,
        ),
        bounds,
      ),
    })),

  toggleMaximize: (id) =>
    set(({ windows, bounds }) => {
      const win = windows.find((w) => w.id === id);
      if (!win) return {};
      if (win.mode !== 'maximized') {
        // A pinned window keeps its slot while maximized, so Restore puts it back there.
        const slot = win.mode === 'docked' ? win.slot : null;
        const maximized = windows.map((w) => (w.id === id ? { ...w, mode: 'maximized' as const, slot } : w));
        return { windows: fitToDesktop(maximized, bounds) };
      }
      const floating = windows.map((w) => (w.id === id ? { ...w, mode: 'floating' as const, slot: null } : w));
      // Back into its slot, or floating when another window has been pinned there meanwhile.
      return { windows: fitToDesktop(win.slot ? pinUndisturbed(floating, id, win.slot, bounds) : floating, bounds) };
    }),

  dock: (id, slot) =>
    set(({ windows, bounds }) => {
      sizing += 1;
      return { windows: pin(windows, id, slot, bounds, sizing) };
    }),

  float: (id) =>
    set(({ windows, bounds }) => ({
      windows: fitToDesktop(
        windows.map((w) => (w.id === id ? { ...w, mode: 'floating', slot: null } : w)),
        bounds,
      ),
    })),

  tearOff: (id, px, py, grabRatio) =>
    set(({ windows, bounds }) => ({
      windows: fitToDesktop(
        windows.map((w) =>
          w.id === id
            ? {
                ...w,
                mode: 'floating',
                slot: null,
                wanted: {
                  ...w.wanted,
                  rect: wantRect(w, fitRect(w.kind, restoreUnderPointer(w.rect, px, py, grabRatio), bounds)),
                },
              }
            : w,
        ),
        bounds,
      ),
    })),

  setDockSize: (id, size) =>
    set(({ windows, bounds }) => {
      const win = windows.find((w) => w.id === id);
      if (!win || win.mode !== 'docked' || !win.slot) return {};
      // The edge moves as far as the column or row across leaves room for, as shown.
      const side = sideOf(win.slot);
      const resized = capDocks(resizeDock(windows, id, size, bounds), maxOf).find((w) => w.id === id)!;
      const key = side === 'top' || side === 'bottom' ? 'dockHeight' : 'dockWidth';
      sizing += 1;
      const next = windows.map((w) =>
        w.mode === 'docked' && w.slot && sideOf(w.slot) === side
          ? { ...w, wanted: { ...w.wanted, [key]: resized[key] }, sizedAt: sizing }
          : w,
      );
      return { windows: fitToDesktop(next, bounds) };
    }),

  setSplit: (column, split) =>
    set(({ splits, wantedSplits, bounds }) => {
      const share = clampSplit(split, bounds);
      return { splits: { ...splits, [column]: share }, wantedSplits: { ...wantedSplits, [column]: share } };
    }),

  setProps: (id, props) =>
    set(({ windows }) => ({
      windows: windows.map((w) => (w.id === id ? { ...w, props: { ...w.props, ...props } } : w)),
    })),

  setPreview: (preview) => set({ preview }),

  setResizing: (resizing) => set({ resizing }),

  snapshot: () => {
    const { windows, wantedSplits } = get();
    for (const w of windows) remember(w);
    return {
      windows: [...windows]
        .sort((a, b) => a.z - b.z)
        // The inspector shows one message's trace; reopening it after a restart would be noise.
        .filter((w) => w.kind !== 'inspector')
        // What each window wants is saved, not what a small desktop has squeezed it to.
        .map(({ kind, props, mode, slot, wanted }) => ({ kind, props, mode, slot, ...wanted })),
      geometry: { ...remembered },
      splits: wantedSplits,
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
        const wanted: Geometry = {
          rect: fitRect(w.kind, w.rect, ANY_DESKTOP),
          dockWidth: w.dockWidth ?? spec.defaultDockWidth,
          dockHeight: w.dockHeight ?? spec.defaultDockHeight,
        };
        return {
          id: `${w.kind}-${counter}`,
          kind: w.kind,
          props: w.props ?? {},
          ...wanted,
          mode,
          slot: mode === 'docked' || mode === 'maximized' ? slot : null,
          wanted,
          sizedAt: 0,
          z,
        };
      });
    const split = (value: unknown) => (typeof value === 'number' && value > 0 && value < 1 ? value : 0.5);
    const wantedSplits = { left: split(saved.splits?.left), right: split(saved.splits?.right) };
    // The desktop may not have measured itself yet, its size still a placeholder: fitting into it
    // costs nothing, as setBounds fits again from what the windows want once it has.
    const { bounds } = get();
    set({
      windows: fitToDesktop(windows, bounds),
      splits: { left: clampSplit(wantedSplits.left, bounds), right: clampSplit(wantedSplits.right, bounds) },
      wantedSplits,
      zTop: z,
      focusedId: windows[windows.length - 1]?.id ?? null,
    });
  },
}));
