import { describe, expect, it } from 'vitest';

import {
  GAP,
  MIN_CHAT_WIDTH,
  MIN_DOCK_WIDTH,
  chatInsets,
  clampDockWidth,
  clampRect,
  displayRect,
  initialRect,
  resizeRect,
  restoreUnderPointer,
  snapZone,
} from './geometry';

const bounds = { w: 1400, h: 900 };

describe('placement', () => {
  it('draws maximized windows over the whole desktop', () => {
    const r = displayRect(
      { mode: 'maximized', side: null, rect: { x: 5, y: 5, w: 400, h: 300 }, dockWidth: 400 },
      bounds,
    );
    expect(r).toEqual({ x: 0, y: 0, w: 1400, h: 900 });
  });

  it('docks to a side with the island gap', () => {
    const r = displayRect(
      { mode: 'docked', side: 'right', rect: { x: 5, y: 5, w: 400, h: 300 }, dockWidth: 500 },
      bounds,
    );
    expect(r).toEqual({ x: 1400 - 500 - GAP, y: GAP, w: 500, h: 900 - 2 * GAP });
  });

  it('reserves chat space only for docked windows', () => {
    const insets = chatInsets([
      { mode: 'docked', side: 'left', rect: { x: 0, y: 0, w: 1, h: 1 }, dockWidth: 380 },
      { mode: 'floating', side: null, rect: { x: 0, y: 0, w: 1, h: 1 }, dockWidth: 999 },
    ]);
    expect(insets).toEqual({ left: 380 + GAP, right: 0 });
  });
});

describe('docking width', () => {
  it('never squeezes the chat below its minimum', () => {
    const w = clampDockWidth(5000, bounds, 0);
    expect(bounds.w - w - 3 * GAP).toBeGreaterThanOrEqual(MIN_CHAT_WIDTH);
  });

  it('keeps a usable minimum width', () => {
    expect(clampDockWidth(50, bounds, 0)).toBe(MIN_DOCK_WIDTH);
  });

  it('accounts for a window docked on the other side', () => {
    expect(clampDockWidth(5000, bounds, 400)).toBeLessThan(clampDockWidth(5000, bounds, 0));
  });
});

describe('snapping', () => {
  it('snaps at the left, right and top edges only', () => {
    expect(snapZone(3, 400, bounds)).toBe('left');
    expect(snapZone(1398, 400, bounds)).toBe('right');
    expect(snapZone(700, 2, bounds)).toBe('top');
    expect(snapZone(700, 400, bounds)).toBeNull();
  });

  it('restores under the pointer at the grab ratio', () => {
    const r = restoreUnderPointer({ x: 0, y: 0, w: 600, h: 400 }, 700, 20, 0.5);
    expect(r.x).toBe(400);
    expect(r.w).toBe(600);
  });
});

describe('floating rects', () => {
  it('keeps the title bar reachable', () => {
    const r = clampRect({ x: -2000, y: -50, w: 600, h: 400 }, bounds, { w: 300, h: 200 });
    expect(r.x + r.w).toBeGreaterThan(0);
    expect(r.y).toBe(0);
  });

  it('resizes from the west edge without moving the east edge', () => {
    const start = { x: 200, y: 100, w: 600, h: 400 };
    const r = resizeRect(start, 'w', -50, 0, { w: 300, h: 200 }, bounds);
    expect(r.x).toBe(150);
    expect(r.x + r.w).toBe(start.x + start.w);
  });

  it('stops resizing at the minimum size', () => {
    const r = resizeRect({ x: 200, y: 100, w: 600, h: 400 }, 'se', -1000, -1000, { w: 300, h: 200 }, bounds);
    expect(r.w).toBe(300);
    expect(r.h).toBe(200);
  });

  it('steps new windows away from existing ones', () => {
    const first = initialRect({ w: 800, h: 600 }, bounds, []);
    const second = initialRect({ w: 800, h: 600 }, bounds, [first]);
    expect(second.x).toBeGreaterThan(first.x);
  });
});
