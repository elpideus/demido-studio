import { describe, expect, it } from 'vitest';

import {
  EVEN_SPLITS,
  GAP,
  MIN_CHAT_HEIGHT,
  MIN_CHAT_WIDTH,
  MIN_DOCK_HEIGHT,
  MIN_DOCK_WIDTH,
  MIN_HALF_HEIGHT,
  type Pinnable,
  type Slot,
  capDocks,
  chatInsets,
  clampDockHeight,
  clampDockWidth,
  clampRect,
  clampSplit,
  coveredShare,
  displayRect,
  dockLayout,
  initialRect,
  pinWindow,
  previewRect,
  refitDocks,
  resizeDock,
  resizeRect,
  restoreUnderPointer,
  slotRect,
  slotsOverlap,
  snapZone,
} from './geometry';

const bounds = { w: 1400, h: 900 };
const rect = { x: 5, y: 5, w: 400, h: 300 };

function win(
  id: string,
  slot: Slot | null,
  extra: Partial<Pinnable & { z: number; sizedAt: number }> = {},
): Pinnable & { z: number; sizedAt?: number } {
  return {
    id,
    mode: slot ? 'docked' : 'floating',
    slot,
    rect,
    dockWidth: 500,
    dockHeight: 300,
    z: 1,
    ...extra,
  };
}

describe('placement', () => {
  it('draws maximized windows over the whole desktop', () => {
    const p = { ...win('a', null), mode: 'maximized' as const };
    expect(displayRect(p, bounds, dockLayout([p]))).toEqual({ x: 0, y: 0, w: 1400, h: 900 });
  });

  it('docks to a side with the island gap', () => {
    const p = win('a', 'right');
    expect(displayRect(p, bounds, dockLayout([p]))).toEqual({ x: 1400 - 500 - GAP, y: GAP, w: 500, h: 900 - 2 * GAP });
  });

  it('splits a column into a top and a bottom half with a gap between', () => {
    const top = win('a', 'top-left');
    const bottom = win('b', 'bottom-left');
    const layout = dockLayout([top, bottom], EVEN_SPLITS);
    const a = slotRect('top-left', layout, bounds);
    const b = slotRect('bottom-left', layout, bounds);
    expect(a.x).toBe(GAP);
    expect(a.y).toBe(GAP);
    expect(b.y).toBe(a.y + a.h + GAP);
    expect(b.y + b.h).toBe(bounds.h - GAP);
    expect(a.w).toBe(500);
  });

  it('fits rows between the columns', () => {
    const windows = [win('a', 'left', { dockWidth: 400 }), win('b', 'right', { dockWidth: 300 }), win('c', 'top')];
    const r = slotRect('top', dockLayout(windows), bounds);
    expect(r.x).toBe(400 + 2 * GAP);
    expect(r.x + r.w).toBe(bounds.w - 300 - 2 * GAP);
    expect(r).toMatchObject({ y: GAP, h: 300 });
    const bottom = slotRect('bottom', dockLayout([win('d', 'bottom', { dockHeight: 250 })]), bounds);
    expect(bottom).toEqual({ x: GAP, y: bounds.h - GAP - 250, w: bounds.w - 2 * GAP, h: 250 });
  });

  it('reserves chat space only for docked windows, on every side', () => {
    const insets = chatInsets(
      dockLayout([
        win('a', 'left', { dockWidth: 380 }),
        win('b', 'bottom', { dockHeight: 240 }),
        win('c', null, { dockWidth: 999 }),
      ]),
    );
    expect(insets).toEqual({ left: 380 + GAP, right: 0, top: 0, bottom: 240 + GAP });
  });
});

describe('pinning', () => {
  it('floats the window already in an overlapping slot', () => {
    const windows = [win('a', 'left'), win('b', null)];
    const after = pinWindow(windows, 'b', 'top-left', bounds);
    expect(after.find((w) => w.id === 'a')).toMatchObject({ mode: 'floating', slot: null });
    expect(after.find((w) => w.id === 'b')).toMatchObject({ mode: 'docked', slot: 'top-left' });
  });

  it('lets the two halves of a column and the rows coexist', () => {
    expect(slotsOverlap('top-left', 'bottom-left')).toBe(false);
    expect(slotsOverlap('top-left', 'left')).toBe(true);
    expect(slotsOverlap('top', 'top-left')).toBe(false);
    expect(slotsOverlap('top', 'bottom')).toBe(false);
    expect(slotsOverlap('right', 'right')).toBe(true);
  });

  it('gives a window joining a column the width of the window already there', () => {
    const windows = [win('a', 'top-right', { dockWidth: 420 }), win('b', null, { dockWidth: 700 })];
    const after = pinWindow(windows, 'b', 'bottom-right', bounds);
    expect(after.find((w) => w.id === 'b')?.dockWidth).toBe(420);
  });

  it('resizes both halves of a column together', () => {
    const windows = [win('a', 'top-right'), win('b', 'bottom-right'), win('c', 'left', { dockWidth: 400 })];
    const after = resizeDock(windows, 'a', 450, bounds);
    expect(after.map((w) => w.dockWidth)).toEqual([450, 450, 400]);
  });

  it('makes the row across give way so the chat keeps its minimum height', () => {
    const laptop = { w: 1300, h: 700 };
    const top = win('a', null, { dockHeight: 380 });
    const bottom = win('b', null, { dockHeight: 360 });
    const pinned = pinWindow(pinWindow([top, bottom], 'a', 'top', laptop), 'b', 'bottom', laptop);
    const insets = chatInsets(dockLayout(pinned));
    expect(laptop.h - insets.top - insets.bottom).toBeGreaterThanOrEqual(MIN_CHAT_HEIGHT);
    expect(pinned.every((w) => w.mode === 'docked' && w.dockHeight >= MIN_DOCK_HEIGHT)).toBe(true);
  });

  it('floats the row across when not even the smallest rows fit', () => {
    const small = { w: 1300, h: 620 };
    const windows = pinWindow([win('a', null), win('b', null)], 'a', 'top', small);
    const after = pinWindow(windows, 'b', 'bottom', small);
    expect(after.find((w) => w.id === 'a')).toMatchObject({ mode: 'floating', slot: null });
    // Alone in its axis, the row gets the height it wants, not the share it would have had.
    expect(after.find((w) => w.id === 'b')).toMatchObject({ mode: 'docked', slot: 'bottom', dockHeight: 300 });
  });

  it('floats the side used least recently when the desktop gets too narrow for two columns', () => {
    const tiny = { w: 908, h: 700 };
    const windows = [win('a', 'left', { dockWidth: 560, z: 5 }), win('b', 'right', { dockWidth: 620, z: 3 })];
    const [a, b] = refitDocks(windows, tiny);
    expect(b).toMatchObject({ mode: 'floating', slot: null });
    expect(a).toMatchObject({ mode: 'docked', slot: 'left' });
    expect(tiny.w - chatInsets(dockLayout([a!, b!])).left).toBeGreaterThanOrEqual(MIN_CHAT_WIDTH);
  });

  it('shrinks both columns alike when the desktop gets narrower', () => {
    const narrow = { w: 1300, h: 900 };
    const room = narrow.w - MIN_CHAT_WIDTH - 4 * GAP;
    const [a, b] = refitDocks([win('a', 'left', { dockWidth: 600 }), win('b', 'right', { dockWidth: 500 })], narrow);
    expect(a!.dockWidth + b!.dockWidth).toBeLessThanOrEqual(room);
    expect(a!.dockWidth / b!.dockWidth).toBeCloseTo(1.2, 1);
    // A column held at its minimum width leaves the other only what remains.
    const [c, d] = refitDocks([win('c', 'left', { dockWidth: 600 }), win('d', 'right', { dockWidth: 400 })], narrow);
    expect(d!.dockWidth).toBe(MIN_DOCK_WIDTH);
    expect(c!.dockWidth + d!.dockWidth).toBe(room);
  });

  it('keeps the width of the column sized most recently when two no longer fit', () => {
    const narrow = { w: 1300, h: 900 };
    const room = narrow.w - MIN_CHAT_WIDTH - 4 * GAP;
    const [a, b] = refitDocks(
      [win('a', 'left', { dockWidth: 600, sizedAt: 1 }), win('b', 'right', { dockWidth: 400, sizedAt: 2 })],
      narrow,
    );
    expect(b!.dockWidth).toBe(400);
    expect(a!.dockWidth).toBe(room - 400);
    // Even the column sized last leaves the other its minimum.
    const [c, d] = refitDocks(
      [win('c', 'left', { dockWidth: 900, sizedAt: 2 }), win('d', 'right', { dockWidth: 400, sizedAt: 1 })],
      narrow,
    );
    expect(d!.dockWidth).toBe(MIN_DOCK_WIDTH);
    expect(c!.dockWidth).toBe(room - MIN_DOCK_WIDTH);
  });

  it('floats the column sized least recently when not even two minimum widths fit', () => {
    const tiny = { w: 908, h: 700 };
    const [a, b] = refitDocks(
      [win('a', 'left', { dockWidth: 560, z: 5, sizedAt: 1 }), win('b', 'right', { dockWidth: 620, z: 3, sizedAt: 2 })],
      tiny,
    );
    expect(a).toMatchObject({ mode: 'floating', slot: null });
    expect(b).toMatchObject({ mode: 'docked', slot: 'right' });
  });

  it('previews exactly where the window would land', () => {
    const windows = [win('a', 'left', { dockWidth: 420 }), win('b', null)];
    expect(previewRect('bottom-left', windows, 'b', bounds, EVEN_SPLITS)).toEqual(
      slotRect('bottom-left', dockLayout(pinWindow(windows, 'b', 'bottom-left', bounds)), bounds),
    );
    expect(previewRect('maximize', windows, 'b', bounds, EVEN_SPLITS)).toEqual({ x: 0, y: 0, w: 1400, h: 900 });
  });
});

describe('docking sizes', () => {
  it('never squeezes the chat below its minimum width', () => {
    const w = clampDockWidth(5000, bounds, 0);
    expect(bounds.w - w - 3 * GAP).toBeGreaterThanOrEqual(MIN_CHAT_WIDTH);
  });

  it('never squeezes the chat below its minimum height', () => {
    const h = clampDockHeight(5000, bounds, 200);
    expect(bounds.h - h - 200 - 4 * GAP).toBeGreaterThanOrEqual(MIN_CHAT_HEIGHT);
  });

  it('keeps a usable minimum width', () => {
    expect(clampDockWidth(50, bounds, 0)).toBe(MIN_DOCK_WIDTH);
  });

  it('accounts for a window docked on the other side', () => {
    expect(clampDockWidth(5000, bounds, 400)).toBeLessThan(clampDockWidth(5000, bounds, 0));
  });

  it('keeps a column within the smallest maximum width of the windows in it', () => {
    const maxOf = (w: Pinnable) => (w.id === 'a' ? { w: 700, h: 600 } : { w: 900, h: 600 });
    const windows = [
      win('a', 'top-right', { dockWidth: 850 }),
      win('b', 'bottom-right', { dockWidth: 850 }),
      win('c', 'left', { dockWidth: 850 }),
      win('d', 'bottom', { dockHeight: 750 }),
      win('e', null, { dockWidth: 5000 }),
    ];
    const after = capDocks(windows, maxOf);
    expect(after.map((w) => w.dockWidth)).toEqual([700, 700, 850, 500, 5000]);
    expect(after.find((w) => w.id === 'd')?.dockHeight).toBe(600);
  });

  it('keeps both halves of a split column usable', () => {
    const usable = bounds.h - 3 * GAP;
    expect(clampSplit(0, bounds) * usable).toBeCloseTo(MIN_HALF_HEIGHT);
    expect((1 - clampSplit(1, bounds)) * usable).toBeCloseTo(MIN_HALF_HEIGHT);
    expect(clampSplit(0.3, bounds)).toBe(0.3);
  });
});

describe('snapping', () => {
  it('snaps like Windows: sides, corners, top maximizes, bottom pins below the chat', () => {
    expect(snapZone(3, 400, bounds)).toBe('left');
    expect(snapZone(1398, 400, bounds)).toBe('right');
    expect(snapZone(700, 2, bounds)).toBe('maximize');
    expect(snapZone(700, 898, bounds)).toBe('bottom');
    expect(snapZone(2, 20, bounds)).toBe('top-left');
    expect(snapZone(40, 2, bounds)).toBe('top-left');
    expect(snapZone(1398, 880, bounds)).toBe('bottom-right');
    expect(snapZone(1360, 2, bounds)).toBe('top-right');
    expect(snapZone(2, 880, bounds)).toBe('bottom-left');
    expect(snapZone(700, 400, bounds)).toBeNull();
  });

  it('restores under the pointer at the grab ratio', () => {
    const r = restoreUnderPointer({ x: 0, y: 0, w: 600, h: 400 }, 700, 20, 0.5);
    expect(r.x).toBe(400);
    expect(r.w).toBe(600);
  });
});

describe('covering', () => {
  it('measures how much of a window the windows above hide', () => {
    const below = win('a', null, { rect: { x: 0, y: 0, w: 400, h: 400 }, z: 1 });
    const quarter = win('b', null, { rect: { x: 200, y: 200, w: 400, h: 400 }, z: 2 });
    expect(coveredShare([below, quarter], 'a', bounds, EVEN_SPLITS)).toBeCloseTo(0.25);
    expect(coveredShare([below, quarter], 'b', bounds, EVEN_SPLITS)).toBe(0);
    const maximized = { ...win('c', null, { z: 3 }), mode: 'maximized' as const };
    expect(coveredShare([below, quarter, maximized], 'a', bounds, EVEN_SPLITS)).toBe(1);
  });

  it('does not count windows side by side', () => {
    const left = win('a', 'left', { z: 1 });
    const right = win('b', 'right', { z: 2 });
    expect(coveredShare([left, right], 'a', bounds, EVEN_SPLITS)).toBe(0);
  });

  it('counts the part off the desktop as out of sight', () => {
    const offLeft = win('a', null, { rect: { x: -200, y: 0, w: 400, h: 400 }, z: 1 });
    expect(coveredShare([offLeft], 'a', bounds, EVEN_SPLITS)).toBeCloseTo(0.5);
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

  it('stops resizing at the maximum size, from any edge', () => {
    const start = { x: 400, y: 300, w: 600, h: 400 };
    const min = { w: 300, h: 200 };
    const max = { w: 800, h: 500 };
    expect(resizeRect(start, 'se', 5000, 5000, min, bounds, max)).toMatchObject({ x: 400, y: 300, w: 800, h: 500 });
    const nw = resizeRect(start, 'nw', -5000, -5000, min, bounds, max);
    expect(nw).toMatchObject({ w: 800, h: 500 });
    expect(nw.x + nw.w).toBe(start.x + start.w);
    expect(nw.y + nw.h).toBe(start.y + start.h);
  });

  it('shrinks a rect larger than its maximum, such as one saved before the limit existed', () => {
    const r = clampRect({ x: 20, y: 20, w: 1300, h: 880 }, bounds, { w: 300, h: 200 }, { w: 1000, h: 700 });
    expect(r).toEqual({ x: 20, y: 20, w: 1000, h: 700 });
  });

  it('steps new windows away from existing ones', () => {
    const first = initialRect({ w: 800, h: 600 }, bounds, []);
    const second = initialRect({ w: 800, h: 600 }, bounds, [first]);
    expect(second.x).toBeGreaterThan(first.x);
  });
});
