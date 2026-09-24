// The window manager's state: which windows are open, where, in which mode and in what order.
// Geometry rules live in wm/geometry.ts; this store applies them.

import { create } from 'zustand';

import {
  type Mode,
  type Rect,
  type Side,
  type Size,
  clampDockWidth,
  clampRect,
  initialRect,
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
}

export const WINDOW_SPECS: Record<WindowKind, WindowSpec> = {
  settings: {
    title: 'Settings',
    defaultSize: { w: 980, h: 680 },
    minSize: { w: 560, h: 420 },
    singleton: true,
    defaultDockWidth: 560,
  },
  market: {
    title: 'Market',
    defaultSize: { w: 1040, h: 660 },
    minSize: { w: 520, h: 380 },
    singleton: true,
    defaultDockWidth: 620,
  },
  inspector: {
    title: 'Inspector',
    defaultSize: { w: 860, h: 640 },
    minSize: { w: 480, h: 360 },
    singleton: true,
    defaultDockWidth: 560,
  },
};

export interface WindowState {
  id: string;
  kind: WindowKind;
  props: Record<string, unknown>;
  /** Floating geometry, kept while maximized or docked so the window can go back to it. */
  rect: Rect;
  mode: Mode;
  side: Side | null;
  dockWidth: number;
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
  preview: SnapPreview | null;
  open: (kind: WindowKind, props?: Record<string, unknown>) => string;
  close: (id: string) => void;
  focus: (id: string) => void;
  setBounds: (bounds: Size) => void;
  setRect: (id: string, rect: Rect) => void;
  toggleMaximize: (id: string) => void;
  dock: (id: string, side: Side) => void;
  float: (id: string) => void;
  /** Leaves maximized/docked mode because the title bar was dragged. */
  tearOff: (id: string, px: number, py: number, grabRatio: number) => void;
  setDockWidth: (id: string, width: number) => void;
  setProps: (id: string, props: Record<string, unknown>) => void;
  setPreview: (preview: SnapPreview | null) => void;
  /** Serializable layout for settings.json. */
  snapshot: () => SavedLayout;
  restore: (layout: unknown) => void;
}

export interface SavedLayout {
  windows: Array<Pick<WindowState, 'kind' | 'props' | 'rect' | 'mode' | 'side' | 'dockWidth'>>;
  geometry: Partial<Record<WindowKind, Pick<WindowState, 'rect' | 'dockWidth'>>>;
}

/** Last geometry per kind, so a reopened window comes back where it was. */
const remembered: Partial<Record<WindowKind, Pick<WindowState, 'rect' | 'dockWidth'>>> = {};

let counter = 0;

function otherDockWidth(windows: WindowState[], side: Side, exceptId: string): number {
  return windows
    .filter((w) => w.id !== exceptId && w.mode === 'docked' && w.side === side)
    .reduce((m, w) => Math.max(m, w.dockWidth), 0);
}

export const useWindows = create<WindowsStore>((set, get) => ({
  windows: [],
  focusedId: null,
  bounds: { w: 1280, h: 800 },
  zTop: 10,
  preview: null,

  open: (kind, props = {}) => {
    const spec = WINDOW_SPECS[kind];
    const { windows, bounds, zTop } = get();
    const existing = spec.singleton ? windows.find((w) => w.kind === kind) : undefined;
    if (existing) {
      set({
        windows: windows.map((w) =>
          w.id === existing.id ? { ...w, props: { ...w.props, ...props }, z: zTop + 1 } : w,
        ),
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
      mode: 'floating',
      side: null,
      dockWidth: saved?.dockWidth ?? spec.defaultDockWidth,
      z: zTop + 1,
    };
    set({ windows: [...windows, win], focusedId: id, zTop: zTop + 1 });
    return id;
  },

  close: (id) => {
    const { windows, focusedId } = get();
    const win = windows.find((w) => w.id === id);
    if (win) remembered[win.kind] = { rect: win.rect, dockWidth: win.dockWidth };
    const rest = windows.filter((w) => w.id !== id);
    const nextFocus =
      focusedId === id ? ([...rest].sort((a, b) => b.z - a.z)[0]?.id ?? null) : focusedId;
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
    set(({ windows }) => ({
      bounds,
      windows: windows.map((w) => ({
        ...w,
        rect: clampRect(w.rect, bounds, WINDOW_SPECS[w.kind].minSize),
        dockWidth:
          w.mode === 'docked' && w.side
            ? clampDockWidth(w.dockWidth, bounds, otherDockWidth(windows, w.side === 'left' ? 'right' : 'left', w.id))
            : w.dockWidth,
      })),
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
        w.id === id ? { ...w, mode: w.mode === 'maximized' ? 'floating' : 'maximized', side: null } : w,
      ),
    })),

  dock: (id, side) =>
    set(({ windows, bounds }) => {
      const other = otherDockWidth(windows, side === 'left' ? 'right' : 'left', id);
      return {
        windows: windows.map((w) => {
          if (w.id === id) {
            return { ...w, mode: 'docked', side, dockWidth: clampDockWidth(w.dockWidth, bounds, other) };
          }
          // One window per side: the one already there floats back to where it was.
          if (w.mode === 'docked' && w.side === side) return { ...w, mode: 'floating', side: null };
          return w;
        }),
      };
    }),

  float: (id) =>
    set(({ windows }) => ({
      windows: windows.map((w) => (w.id === id ? { ...w, mode: 'floating', side: null } : w)),
    })),

  tearOff: (id, px, py, grabRatio) =>
    set(({ windows, bounds }) => ({
      windows: windows.map((w) =>
        w.id === id
          ? {
              ...w,
              mode: 'floating',
              side: null,
              rect: clampRect(restoreUnderPointer(w.rect, px, py, grabRatio), bounds, WINDOW_SPECS[w.kind].minSize),
            }
          : w,
      ),
    })),

  setDockWidth: (id, width) =>
    set(({ windows, bounds }) => ({
      windows: windows.map((w) =>
        w.id === id && w.side
          ? { ...w, dockWidth: clampDockWidth(width, bounds, otherDockWidth(windows, w.side === 'left' ? 'right' : 'left', id)) }
          : w,
      ),
    })),

  setProps: (id, props) =>
    set(({ windows }) => ({
      windows: windows.map((w) => (w.id === id ? { ...w, props: { ...w.props, ...props } } : w)),
    })),

  setPreview: (preview) => set({ preview }),

  snapshot: () => {
    const { windows } = get();
    for (const w of windows) remembered[w.kind] = { rect: w.rect, dockWidth: w.dockWidth };
    return {
      windows: [...windows]
        .sort((a, b) => a.z - b.z)
        // The inspector shows one message's trace; reopening it after a restart would be noise.
        .filter((w) => w.kind !== 'inspector')
        .map(({ kind, props, rect, mode, side, dockWidth }) => ({ kind, props, rect, mode, side, dockWidth })),
      geometry: { ...remembered },
    };
  },

  restore: (layout) => {
    if (!layout || typeof layout !== 'object') return;
    const saved = layout as Partial<SavedLayout>;
    Object.assign(remembered, saved.geometry ?? {});
    const { bounds } = get();
    let z = 10;
    const windows: WindowState[] = (saved.windows ?? [])
      .filter((w) => w && w.kind in WINDOW_SPECS)
      .map((w) => {
        counter += 1;
        z += 1;
        return {
          id: `${w.kind}-${counter}`,
          kind: w.kind,
          props: w.props ?? {},
          rect: clampRect(w.rect, bounds, WINDOW_SPECS[w.kind].minSize),
          mode: w.mode ?? 'floating',
          side: w.side ?? null,
          dockWidth: w.dockWidth ?? WINDOW_SPECS[w.kind].defaultDockWidth,
          z,
        };
      });
    set({ windows, zTop: z, focusedId: windows[windows.length - 1]?.id ?? null });
  },
}));
