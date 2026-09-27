// Window geometry as pure functions: where a window is drawn, where it snaps, how it resizes.
// The window store and the frame component only call these, so the rules can be unit tested.

export interface Size {
  w: number;
  h: number;
}

export interface Rect extends Size {
  x: number;
  y: number;
}

export type Column = 'left' | 'right';
export type Row = 'top' | 'bottom';
/**
 * Where a pinned window sits. A column runs down one side of the desktop, whole or split into a
 * top and a bottom half; a row sits above or below the chat, between the columns. The chat takes
 * whatever is left.
 */
export type Slot = Column | Row | 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';
export type Mode = 'floating' | 'maximized' | 'docked';
export type SnapZone = Slot | 'maximize';
export type Edge = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';

export const SLOTS: Slot[] = ['left', 'right', 'top', 'bottom', 'top-left', 'top-right', 'bottom-left', 'bottom-right'];

/** Gap between docked windows and the desktop edges, matching the island look. */
export const GAP = 8;
/** The chat keeps at least this much room when windows are docked around it. */
export const MIN_CHAT_WIDTH = 440;
export const MIN_CHAT_HEIGHT = 260;
export const MIN_DOCK_WIDTH = 340;
export const MIN_DOCK_HEIGHT = 200;
/** Smallest half of a split column. */
export const MIN_HALF_HEIGHT = 150;
/** How close to an edge the pointer must be for a drag to snap. */
export const SNAP_DISTANCE = 14;
/** How far along an edge from a corner still counts as that corner. */
export const CORNER_REACH = 110;
export const TITLE_BAR_HEIGHT = 38;
/** Part of a floating window that must stay on screen so it can always be grabbed. */
const KEEP_VISIBLE = 96;

export interface Placement {
  mode: Mode;
  slot: Slot | null;
  rect: Rect;
  dockWidth: number;
  dockHeight: number;
}

/** Share of a split column its top half takes, per column. */
export type Splits = Record<Column, number>;

export const EVEN_SPLITS: Splits = { left: 0.5, right: 0.5 };

/** The sizes of everything pinned: 0 for a column or row nobody is pinned to. */
export interface DockLayout {
  left: number;
  right: number;
  top: number;
  bottom: number;
  splits: Splits;
}

export function columnOf(slot: Slot): Column | null {
  if (slot === 'left' || slot === 'top-left' || slot === 'bottom-left') return 'left';
  if (slot === 'right' || slot === 'top-right' || slot === 'bottom-right') return 'right';
  return null;
}

export function isRow(slot: Slot): slot is Row {
  return slot === 'top' || slot === 'bottom';
}

/** Whether two slots cover some of the same space, so they cannot both hold a window. */
export function slotsOverlap(a: Slot, b: Slot): boolean {
  if (a === b) return true;
  const col = columnOf(a);
  if (!col || col !== columnOf(b)) return false;
  // Halves of one column only collide with the whole column.
  return a === col || b === col;
}

/** Widest a docked column may be, given the width docked on the other side. */
export function maxDockWidth(bounds: Size, otherSide: number): number {
  const reservedOther = otherSide > 0 ? otherSide + GAP : 0;
  return Math.max(MIN_DOCK_WIDTH, bounds.w - MIN_CHAT_WIDTH - reservedOther - 3 * GAP);
}

export function clampDockWidth(width: number, bounds: Size, otherSide: number): number {
  return Math.round(Math.min(Math.max(width, MIN_DOCK_WIDTH), maxDockWidth(bounds, otherSide)));
}

/** Tallest a docked row may be, given the height docked in the other row. */
export function maxDockHeight(bounds: Size, otherRow: number): number {
  const reservedOther = otherRow > 0 ? otherRow + GAP : 0;
  return Math.max(MIN_DOCK_HEIGHT, bounds.h - MIN_CHAT_HEIGHT - reservedOther - 3 * GAP);
}

export function clampDockHeight(height: number, bounds: Size, otherRow: number): number {
  return Math.round(Math.min(Math.max(height, MIN_DOCK_HEIGHT), maxDockHeight(bounds, otherRow)));
}

/** Keeps both halves of a split column at least `MIN_HALF_HEIGHT` tall. */
export function clampSplit(split: number, bounds: Size): number {
  const usable = bounds.h - 3 * GAP;
  if (usable <= 2 * MIN_HALF_HEIGHT) return 0.5;
  const min = MIN_HALF_HEIGHT / usable;
  return Math.min(Math.max(split, min), 1 - min);
}

/** Measures the columns and rows from the windows pinned to them. */
export function dockLayout(placements: Placement[], splits: Splits = EVEN_SPLITS): DockLayout {
  const layout: DockLayout = { left: 0, right: 0, top: 0, bottom: 0, splits };
  for (const p of placements) {
    if (p.mode !== 'docked' || !p.slot) continue;
    const col = columnOf(p.slot);
    if (col) layout[col] = Math.max(layout[col], p.dockWidth);
    else if (isRow(p.slot)) layout[p.slot] = Math.max(layout[p.slot], p.dockHeight);
  }
  return layout;
}

/** The rectangle a slot covers in a layout. */
export function slotRect(slot: Slot, layout: DockLayout, bounds: Size): Rect {
  const height = Math.max(0, bounds.h - 2 * GAP);
  const col = columnOf(slot);
  if (col) {
    const w = Math.max(0, Math.min(layout[col], bounds.w - 2 * GAP));
    const x = col === 'left' ? GAP : bounds.w - w - GAP;
    if (slot === col) return { x, y: GAP, w, h: height };
    const top = Math.round((height - GAP) * layout.splits[col]);
    return slot.startsWith('top')
      ? { x, y: GAP, w, h: top }
      : { x, y: GAP + top + GAP, w, h: Math.max(0, height - top - GAP) };
  }
  // Rows span the space between the columns.
  const x = layout.left > 0 ? layout.left + 2 * GAP : GAP;
  const end = layout.right > 0 ? bounds.w - layout.right - 2 * GAP : bounds.w - GAP;
  const h = Math.max(0, Math.min(layout[slot as Row], height));
  return { x, y: slot === 'top' ? GAP : bounds.h - GAP - h, w: Math.max(0, end - x), h };
}

/** Where a window is drawn inside the desktop. */
export function displayRect(p: Placement, bounds: Size, layout: DockLayout): Rect {
  switch (p.mode) {
    case 'maximized':
      return { x: 0, y: 0, w: bounds.w, h: bounds.h };
    case 'docked':
      return slotRect(p.slot ?? 'right', layout, bounds);
    default:
      return p.rect;
  }
}

/** Space the chat must leave free on each side for pinned windows. */
export function chatInsets(layout: DockLayout): { left: number; right: number; top: number; bottom: number } {
  const inset = (size: number) => (size > 0 ? size + GAP : 0);
  return { left: inset(layout.left), right: inset(layout.right), top: inset(layout.top), bottom: inset(layout.bottom) };
}

/** A window as the pinning rules need it. */
export interface Pinnable extends Placement {
  id: string;
}

/** The two columns across the chat share its width; the two rows share its height. */
interface Axis {
  sides: [Column, Column] | [Row, Row];
  key: 'dockWidth' | 'dockHeight';
  total: number;
  minChat: number;
  minDock: number;
}

function axisOf(slot: Slot, bounds: Size): Axis {
  return columnOf(slot)
    ? { sides: ['left', 'right'], key: 'dockWidth', total: bounds.w, minChat: MIN_CHAT_WIDTH, minDock: MIN_DOCK_WIDTH }
    : {
        sides: ['top', 'bottom'],
        key: 'dockHeight',
        total: bounds.h,
        minChat: MIN_CHAT_HEIGHT,
        minDock: MIN_DOCK_HEIGHT,
      };
}

/** The column or row a slot is part of. */
export function sideOf(slot: Slot): Column | Row {
  return columnOf(slot) ?? (slot as Row);
}

function onSide(w: Placement, side: Column | Row): boolean {
  return w.mode === 'docked' && w.slot !== null && sideOf(w.slot) === side;
}

/** How wide a column (or tall a row) is, leaving out one window. */
function sideSize(windows: Pinnable[], side: Column | Row, axis: Axis, exceptId?: string): number {
  return windows.filter((w) => w.id !== exceptId && onSide(w, side)).reduce((m, w) => Math.max(m, w[axis.key]), 0);
}

/** What the docks on one axis may share once the chat has its minimum and the gaps are paid. */
function dockRoom(axis: Axis, both: boolean): number {
  return axis.total - axis.minChat - 3 * GAP - (both ? GAP : 0);
}

function resizeSide<T extends Pinnable>(windows: T[], side: Column | Row, axis: Axis, size: number): T[] {
  return windows.map((w) => (onSide(w, side) ? { ...w, [axis.key]: size } : w));
}

/**
 * Pins a window to a slot. Windows already in a slot that overlaps it float back to where they
 * were; a window joining the other half of a column takes that column's width. If the chat would
 * get less than its minimum, the column or row across from it gives way first, down to its own
 * minimum, and floats back when even that does not fit.
 */
export function pinWindow<T extends Pinnable>(windows: T[], id: string, slot: Slot, bounds: Size): T[] {
  const win = windows.find((w) => w.id === id);
  if (!win) return windows;
  const axis = axisOf(slot, bounds);
  const side = sideOf(slot);
  const across = axis.sides[0] === side ? axis.sides[1] : axis.sides[0];
  let next = windows.map((w) =>
    w.id !== id && w.mode === 'docked' && w.slot && slotsOverlap(w.slot, slot)
      ? { ...w, mode: 'floating' as const, slot: null }
      : w,
  );
  const wanted = sideSize(next, side, axis, id) || win[axis.key];
  const fit = (room: number) => Math.round(Math.min(Math.max(wanted, axis.minDock), Math.max(axis.minDock, room)));
  const acrossSize = sideSize(next, across, axis, id);
  let size = fit(dockRoom(axis, false));
  if (acrossSize > 0) {
    const room = dockRoom(axis, true);
    if (room < 2 * axis.minDock) {
      next = next.map((w) => (onSide(w, across) ? { ...w, mode: 'floating' as const, slot: null } : w));
    } else {
      size = fit(room - axis.minDock);
      next = resizeSide(next, across, axis, Math.min(acrossSize, room - size));
    }
  }
  next = next.map((w) => (w.id === id ? { ...w, mode: 'docked' as const, slot } : w));
  return resizeSide(next, side, axis, size);
}

/** Resizes the column or row a docked window is in; both halves of a column move together. */
export function resizeDock<T extends Pinnable>(windows: T[], id: string, size: number, bounds: Size): T[] {
  const win = windows.find((w) => w.id === id);
  if (!win || win.mode !== 'docked' || !win.slot) return windows;
  const axis = axisOf(win.slot, bounds);
  const side = sideOf(win.slot);
  const across = sideSize(windows, axis.sides[0] === side ? axis.sides[1] : axis.sides[0], axis);
  const max = Math.max(axis.minDock, dockRoom(axis, across > 0) - across);
  return resizeSide(windows, side, axis, Math.round(Math.min(Math.max(size, axis.minDock), max)));
}

/**
 * Keeps every column no wider, and every row no taller, than the smallest maximum among the
 * windows in it: its other side is set by the desktop, but the one that can be dragged has a limit.
 */
export function capDocks<T extends Pinnable>(windows: T[], maxOf: (w: T) => Size): T[] {
  let next = windows;
  for (const side of ['left', 'right', 'top', 'bottom'] as const) {
    const members = next.filter((w) => onSide(w, side));
    if (!members.length) continue;
    const vertical = side === 'top' || side === 'bottom';
    const key = vertical ? 'dockHeight' : 'dockWidth';
    const max = Math.min(...members.map((w) => (vertical ? maxOf(w).h : maxOf(w).w)));
    if (members.some((w) => w[key] > max)) {
      next = next.map((w) => (onSide(w, side) && w[key] > max ? { ...w, [key]: max } : w));
    }
  }
  return next;
}

/**
 * Fits every column and row into the desktop. When two sides across from each other no longer
 * both fit, the one sized most recently (`sizedAt`: pinned or dragged) keeps its size, down to
 * what leaves the other its minimum, and the other takes what remains; sides never sized shrink
 * alike. When even two minimum sizes do not fit, the side sized, or else used, least recently floats.
 */
export function refitDocks<T extends Pinnable & { z?: number; sizedAt?: number }>(windows: T[], bounds: Size): T[] {
  let next = windows;
  for (const axis of [axisOf('left', bounds), axisOf('top', bounds)]) {
    const [first, second] = axis.sides;
    const latest = (side: Column | Row, key: 'sizedAt' | 'z') =>
      Math.max(0, ...next.filter((w) => onSide(w, side)).map((w) => w[key] ?? 0));
    const sizedFirst = latest(first, 'sizedAt');
    const sizedSecond = latest(second, 'sizedAt');
    if (
      sideSize(next, first, axis) > 0 &&
      sideSize(next, second, axis) > 0 &&
      dockRoom(axis, true) < 2 * axis.minDock
    ) {
      const firstNewer =
        sizedFirst !== sizedSecond ? sizedFirst > sizedSecond : latest(first, 'z') >= latest(second, 'z');
      const older = firstNewer ? second : first;
      next = next.map((w) => (onSide(w, older) ? { ...w, mode: 'floating' as const, slot: null } : w));
    }
    const a = sideSize(next, first, axis);
    const b = sideSize(next, second, axis);
    const room = dockRoom(axis, a > 0 && b > 0);
    const within = (size: number, space: number) => Math.round(Math.max(axis.minDock, Math.min(size, space)));
    let fitA = within(a, room);
    let fitB = within(b, room);
    if (a > 0 && b > 0 && a + b > room) {
      if (sizedFirst > sizedSecond) {
        fitA = within(a, room - axis.minDock);
        fitB = within(b, room - fitA);
      } else if (sizedSecond > sizedFirst) {
        fitB = within(b, room - axis.minDock);
        fitA = within(a, room - fitB);
      } else {
        const scale = room / (a + b);
        fitA = within(a * scale, room);
        fitB = within(b * scale, room);
        // A side held at its minimum leaves the other one only what remains.
        if (fitA + fitB > room) {
          if (fitA === axis.minDock) fitB = within(b, room - fitA);
          else fitA = within(a, room - fitB);
        }
      }
    }
    if (a > 0) next = resizeSide(next, first, axis, fitA);
    if (b > 0) next = resizeSide(next, second, axis, fitB);
  }
  return next;
}

/**
 * Share of a window, from 0 to 1, that cannot be seen: hidden by windows drawn above it or off the
 * desktop. Measured on a grid of points.
 */
export function coveredShare<T extends Pinnable & { z: number }>(
  windows: T[],
  id: string,
  bounds: Size,
  splits: Splits,
): number {
  const target = windows.find((w) => w.id === id);
  if (!target) return 0;
  const layout = dockLayout(windows, splits);
  const a = displayRect(target, bounds, layout);
  const above = windows.filter((w) => w.id !== id && w.z > target.z).map((w) => displayRect(w, bounds, layout));
  if (a.w <= 0 || a.h <= 0) return 0;
  const steps = 16;
  let hidden = 0;
  for (let i = 0; i < steps; i += 1) {
    for (let j = 0; j < steps; j += 1) {
      const x = a.x + ((i + 0.5) * a.w) / steps;
      const y = a.y + ((j + 0.5) * a.h) / steps;
      const offDesktop = x < 0 || y < 0 || x >= bounds.w || y >= bounds.h;
      if (offDesktop || above.some((b) => x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h)) hidden += 1;
    }
  }
  return hidden / (steps * steps);
}

const NO_MAX: Size = { w: Infinity, h: Infinity };

/** Keeps a floating window within reach (its title bar stays grabbable) and within its size limits. */
export function clampRect(r: Rect, bounds: Size, min: Size, max: Size = NO_MAX): Rect {
  const w = Math.max(min.w, Math.min(r.w, bounds.w, max.w));
  const h = Math.max(min.h, Math.min(r.h, bounds.h, max.h));
  const x = Math.min(Math.max(r.x, KEEP_VISIBLE - w), bounds.w - KEEP_VISIBLE);
  const y = Math.min(Math.max(r.y, 0), Math.max(0, bounds.h - TITLE_BAR_HEIGHT));
  return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
}

/**
 * The snap target under the pointer while dragging, if any. Like Windows: a side edge pins to
 * that column, a corner to that quarter, the top edge maximizes. The bottom edge pins below the chat.
 */
export function snapZone(px: number, py: number, bounds: Size): SnapZone | null {
  const nearLeft = px <= SNAP_DISTANCE;
  const nearRight = px >= bounds.w - SNAP_DISTANCE;
  const nearTop = py <= SNAP_DISTANCE / 2;
  const nearBottom = py >= bounds.h - SNAP_DISTANCE / 2;
  const topCorner = py <= CORNER_REACH;
  const bottomCorner = py >= bounds.h - CORNER_REACH;
  if (nearLeft) return topCorner ? 'top-left' : bottomCorner ? 'bottom-left' : 'left';
  if (nearRight) return topCorner ? 'top-right' : bottomCorner ? 'bottom-right' : 'right';
  if (nearTop) {
    if (px <= CORNER_REACH) return 'top-left';
    if (px >= bounds.w - CORNER_REACH) return 'top-right';
    return 'maximize';
  }
  if (nearBottom) {
    if (px <= CORNER_REACH) return 'bottom-left';
    if (px >= bounds.w - CORNER_REACH) return 'bottom-right';
    return 'bottom';
  }
  return null;
}

/**
 * The outline shown while a drag would snap: exactly where the window would land, pinned by `pin`
 * (the window store's own pinning, when it differs from `pinWindow`).
 */
export function previewRect<T extends Pinnable>(
  zone: SnapZone,
  windows: T[],
  id: string,
  bounds: Size,
  splits: Splits,
  pin: (windows: T[], id: string, slot: Slot, bounds: Size) => T[] = pinWindow,
): Rect {
  if (zone === 'maximize') return { x: 0, y: 0, w: bounds.w, h: bounds.h };
  return slotRect(zone, dockLayout(pin(windows, id, zone, bounds), splits), bounds);
}

/**
 * Leaving a maximized or docked window by dragging: the window goes back to its floating size
 * and sits under the pointer at the same relative horizontal spot it was grabbed at.
 */
export function restoreUnderPointer(floating: Rect, px: number, py: number, grabRatio: number): Rect {
  const ratio = Math.min(Math.max(grabRatio, 0.05), 0.95);
  return {
    x: Math.round(px - floating.w * ratio),
    y: Math.round(Math.max(0, py - TITLE_BAR_HEIGHT / 2)),
    w: floating.w,
    h: floating.h,
  };
}

/** Resizes a floating rect from an edge by a pointer delta, between the minimum and maximum size. */
export function resizeRect(
  start: Rect,
  edge: Edge,
  dx: number,
  dy: number,
  min: Size,
  bounds: Size,
  max: Size = NO_MAX,
): Rect {
  let { x, y, w, h } = start;
  if (edge.includes('e')) w = Math.min(Math.max(start.w + dx, min.w), max.w, bounds.w - start.x);
  if (edge.includes('s')) h = Math.min(Math.max(start.h + dy, min.h), max.h, bounds.h - start.y);
  if (edge.includes('w')) {
    const right = start.x + start.w;
    const nx = Math.min(Math.max(start.x + dx, 0, right - max.w), right - min.w);
    w = right - nx;
    x = nx;
  }
  if (edge.includes('n')) {
    const bottom = start.y + start.h;
    const ny = Math.min(Math.max(start.y + dy, 0, bottom - max.h), bottom - min.h);
    h = bottom - ny;
    y = ny;
  }
  return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
}

/** The edges a docked window can be resized from: the ones that face the chat or its partner. */
export function dockedHandles(slot: Slot): Edge[] {
  switch (slot) {
    case 'left':
      return ['e'];
    case 'right':
      return ['w'];
    case 'top':
      return ['s'];
    case 'bottom':
      return ['n'];
    case 'top-left':
      return ['e', 's'];
    case 'bottom-left':
      return ['e', 'n'];
    case 'top-right':
      return ['w', 's'];
    case 'bottom-right':
      return ['w', 'n'];
  }
}

/** Where a newly opened window goes: centered, stepped down from windows already there. */
export function initialRect(size: Size, bounds: Size, occupied: Rect[]): Rect {
  const w = Math.min(size.w, bounds.w - 2 * GAP);
  const h = Math.min(size.h, bounds.h - 2 * GAP);
  let x = Math.round((bounds.w - w) / 2);
  let y = Math.round(Math.max(GAP, (bounds.h - h) / 2.4));
  for (let i = 0; i < 8 && occupied.some((r) => Math.abs(r.x - x) < 16 && Math.abs(r.y - y) < 16); i += 1) {
    x += 28;
    y += 28;
  }
  return clampRect({ x, y, w, h }, bounds, { w: Math.min(w, 320), h: Math.min(h, 200) });
}
