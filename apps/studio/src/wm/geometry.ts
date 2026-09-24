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

export type Side = 'left' | 'right';
export type Mode = 'floating' | 'maximized' | 'docked';
export type SnapZone = Side | 'top';
export type Edge = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';

/** Gap between docked windows and the desktop edges, matching the island look. */
export const GAP = 8;
/** The chat keeps at least this much width when windows are docked beside it. */
export const MIN_CHAT_WIDTH = 440;
export const MIN_DOCK_WIDTH = 340;
/** How close to an edge the pointer must be for a drag to snap. */
export const SNAP_DISTANCE = 14;
export const TITLE_BAR_HEIGHT = 38;
/** Part of a floating window that must stay on screen so it can always be grabbed. */
const KEEP_VISIBLE = 96;

export interface Placement {
  mode: Mode;
  side: Side | null;
  rect: Rect;
  dockWidth: number;
}

/** Where a window is drawn inside the desktop. */
export function displayRect(p: Placement, bounds: Size): Rect {
  switch (p.mode) {
    case 'maximized':
      return { x: 0, y: 0, w: bounds.w, h: bounds.h };
    case 'docked':
      return dockedRect(p.side ?? 'right', p.dockWidth, bounds);
    default:
      return p.rect;
  }
}

export function dockedRect(side: Side, width: number, bounds: Size): Rect {
  const w = Math.max(0, Math.min(width, bounds.w - 2 * GAP));
  return {
    x: side === 'left' ? GAP : bounds.w - w - GAP,
    y: GAP,
    w,
    h: Math.max(0, bounds.h - 2 * GAP),
  };
}

/** Widest a docked window may be, given the width docked on the other side. */
export function maxDockWidth(bounds: Size, otherSide: number): number {
  const reservedOther = otherSide > 0 ? otherSide + GAP : 0;
  return Math.max(MIN_DOCK_WIDTH, bounds.w - MIN_CHAT_WIDTH - reservedOther - 3 * GAP);
}

export function clampDockWidth(width: number, bounds: Size, otherSide: number): number {
  return Math.round(Math.min(Math.max(width, MIN_DOCK_WIDTH), maxDockWidth(bounds, otherSide)));
}

/** Space the chat must leave free on each side for docked windows. */
export function chatInsets(placements: Placement[]): { left: number; right: number } {
  let left = 0;
  let right = 0;
  for (const p of placements) {
    if (p.mode !== 'docked') continue;
    if (p.side === 'left') left = Math.max(left, p.dockWidth + GAP);
    else right = Math.max(right, p.dockWidth + GAP);
  }
  return { left, right };
}

/** Keeps a floating window within reach: its title bar stays grabbable. */
export function clampRect(r: Rect, bounds: Size, min: Size): Rect {
  const w = Math.max(min.w, Math.min(r.w, bounds.w));
  const h = Math.max(min.h, Math.min(r.h, bounds.h));
  const x = Math.min(Math.max(r.x, KEEP_VISIBLE - w), bounds.w - KEEP_VISIBLE);
  const y = Math.min(Math.max(r.y, 0), Math.max(0, bounds.h - TITLE_BAR_HEIGHT));
  return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
}

/** The snap target under the pointer while dragging, if any. */
export function snapZone(px: number, py: number, bounds: Size): SnapZone | null {
  if (px <= SNAP_DISTANCE) return 'left';
  if (px >= bounds.w - SNAP_DISTANCE) return 'right';
  if (py <= SNAP_DISTANCE / 2) return 'top';
  return null;
}

/** The outline shown while a drag would snap. */
export function previewRect(zone: SnapZone, bounds: Size, dockWidth: number, otherSide: number): Rect {
  if (zone === 'top') return { x: 0, y: 0, w: bounds.w, h: bounds.h };
  return dockedRect(zone, clampDockWidth(dockWidth, bounds, otherSide), bounds);
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

/** Resizes a floating rect from an edge by a pointer delta, respecting the minimum size. */
export function resizeRect(start: Rect, edge: Edge, dx: number, dy: number, min: Size, bounds: Size): Rect {
  let { x, y, w, h } = start;
  if (edge.includes('e')) w = Math.min(Math.max(start.w + dx, min.w), bounds.w - start.x);
  if (edge.includes('s')) h = Math.min(Math.max(start.h + dy, min.h), bounds.h - start.y);
  if (edge.includes('w')) {
    const nx = Math.min(Math.max(start.x + dx, 0), start.x + start.w - min.w);
    w = start.w + (start.x - nx);
    x = nx;
  }
  if (edge.includes('n')) {
    const ny = Math.min(Math.max(start.y + dy, 0), start.y + start.h - min.h);
    h = start.h + (start.y - ny);
    y = ny;
  }
  return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
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
