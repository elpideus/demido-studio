import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { WINDOW_SPECS as Specs, useWindows as UseWindows } from './windows';

let useWindows: typeof UseWindows;
let WINDOW_SPECS: typeof Specs;

// Every test gets a fresh store: remembered geometry lives at module level.
beforeEach(async () => {
  vi.resetModules();
  ({ useWindows, WINDOW_SPECS } = await import('./windows'));
});

const wm = () => useWindows.getState();
const placement = (kind: string) => {
  const w = wm().windows.find((x) => x.kind === kind);
  return w && { mode: w.mode, slot: w.slot, dockWidth: w.dockWidth };
};

/** Pins a window, then closes it, so the store remembers it as pinned. */
function pinAndClose(kind: 'market' | 'settings', slot: 'left' | 'right') {
  const id = wm().open(kind);
  wm().dock(id, slot);
  wm().close(id);
}

describe('reopening a pinned window', () => {
  it('goes back into its slot when that leaves every other window as it is', () => {
    wm().setBounds({ w: 2000, h: 900 });
    pinAndClose('market', 'right');
    wm().dock(wm().open('settings'), 'left');
    wm().toggle('market');
    expect(placement('market')).toMatchObject({ mode: 'docked', slot: 'right' });
    expect(placement('settings')).toMatchObject({ mode: 'docked', slot: 'left', dockWidth: 560 });
  });

  it('opens floating when its slot would squeeze or unpin another window', () => {
    wm().setBounds({ w: 1100, h: 800 });
    pinAndClose('market', 'right');
    wm().dock(wm().open('settings'), 'left');
    wm().toggle('market');
    expect(placement('market')).toMatchObject({ mode: 'floating', slot: null });
    expect(placement('settings')).toMatchObject({ mode: 'docked', slot: 'left', dockWidth: 560 });
  });

  it('goes where snap assist puts it, not where it was before', () => {
    wm().setBounds({ w: 1400, h: 900 });
    pinAndClose('market', 'right');
    wm().dock(wm().open('settings'), 'top-left');
    wm().open('market', {}, 'bottom-left');
    expect(placement('market')).toMatchObject({ mode: 'docked', slot: 'bottom-left', dockWidth: 560 });
    expect(placement('settings')).toMatchObject({ mode: 'docked', slot: 'top-left', dockWidth: 560 });
  });
});

describe('pinning to a corner', () => {
  it('moves the window pinned to that side into the other half instead of unpinning it', () => {
    wm().setBounds({ w: 1600, h: 900 });
    wm().dock(wm().open('settings'), 'left');
    wm().dock(wm().open('market'), 'top-left');
    expect(placement('market')).toMatchObject({ mode: 'docked', slot: 'top-left', dockWidth: 560 });
    expect(placement('settings')).toMatchObject({ mode: 'docked', slot: 'bottom-left', dockWidth: 560 });
  });
});

describe('maximizing a pinned window', () => {
  it('goes back to its slot and width on Restore', () => {
    wm().setBounds({ w: 2000, h: 900 });
    const id = wm().open('market');
    wm().dock(id, 'right');
    wm().setDockSize(id, 700);
    wm().toggleMaximize(id);
    expect(placement('market')).toMatchObject({ mode: 'maximized' });
    wm().toggleMaximize(id);
    expect(placement('market')).toMatchObject({ mode: 'docked', slot: 'right', dockWidth: 700 });
  });

  it('floats at the size it had before it was pinned once dragged away', () => {
    wm().setBounds({ w: 2000, h: 900 });
    const id = wm().open('market');
    const floating = wm().windows[0]!.rect;
    wm().dock(id, 'right');
    wm().toggleMaximize(id);
    wm().tearOff(id, 1000, 20, 0.5);
    const w = wm().windows[0]!;
    expect(w).toMatchObject({ mode: 'floating', slot: null });
    expect({ w: w.rect.w, h: w.rect.h }).toEqual({ w: floating.w, h: floating.h });
    wm().toggleMaximize(id);
    wm().toggleMaximize(id);
    expect(placement('market')).toMatchObject({ mode: 'floating', slot: null });
  });

  it('floats on Restore when another window has been pinned to its slot meanwhile', () => {
    wm().setBounds({ w: 2000, h: 900 });
    const market = wm().open('market');
    wm().dock(market, 'right');
    wm().toggleMaximize(market);
    wm().dock(wm().open('settings'), 'right');
    wm().toggleMaximize(market);
    expect(placement('market')).toMatchObject({ mode: 'floating', slot: null });
    expect(placement('settings')).toMatchObject({ mode: 'docked', slot: 'right' });
  });
});

describe('size limits', () => {
  it('stops a floating window at its maximum size, even on a larger desktop', () => {
    wm().setBounds({ w: 3400, h: 1400 });
    const id = wm().open('settings');
    wm().setRect(id, { x: 0, y: 0, w: 3000, h: 1300 });
    const { w, h } = wm().windows[0]!.rect;
    expect({ w, h }).toEqual(WINDOW_SPECS.settings.maxSize);
  });

  it('stops a pinned column at its maximum width', () => {
    wm().setBounds({ w: 3400, h: 1400 });
    const id = wm().open('settings');
    wm().dock(id, 'left');
    wm().setDockSize(id, 2500);
    expect(placement('settings')?.dockWidth).toBe(WINDOW_SPECS.settings.maxSize.w);
  });
});

describe('a smaller desktop', () => {
  const big = { w: 2400, h: 1400 };
  const small = { w: 1328, h: 880 };
  const rectOf = (kind: string) => {
    const { w, h } = wm().windows.find((x) => x.kind === kind)!.rect;
    return { w, h };
  };

  it('does not shrink a restored layout to the placeholder size the desktop has before it measures', () => {
    wm().restore({
      windows: [{ kind: 'market', props: {}, rect: { x: 40, y: 20, w: 1800, h: 1100 }, mode: 'floating', slot: null }],
      geometry: {},
    });
    wm().setBounds(big);
    expect(rectOf('market')).toEqual({ w: 1800, h: 1100 });
  });

  it('squeezes windows for as long as it lasts, then gives their size back', () => {
    wm().setBounds(big);
    const market = wm().open('market');
    wm().setRect(market, { x: 40, y: 20, w: 1800, h: 1100 });
    wm().dock(wm().open('settings'), 'right');
    wm().setDockSize(wm().windows[1]!.id, 700);
    wm().setBounds(small);
    expect(rectOf('market').w).toBeLessThanOrEqual(small.w);
    wm().setBounds(big);
    expect(rectOf('market')).toEqual({ w: 1800, h: 1100 });
    expect(placement('settings')?.dockWidth).toBe(700);
  });

  it('gives pinned columns their width back', () => {
    wm().setBounds(big);
    wm().dock(wm().open('market'), 'left');
    wm().setDockSize(wm().windows[0]!.id, 900);
    wm().dock(wm().open('settings'), 'right');
    wm().setDockSize(wm().windows[1]!.id, 700);
    wm().setBounds(small);
    expect(placement('market')!.dockWidth + placement('settings')!.dockWidth).toBeLessThan(900 + 700);
    wm().setBounds(big);
    expect(placement('market')?.dockWidth).toBe(900);
    expect(placement('settings')?.dockWidth).toBe(700);
  });

  it('keeps the size a squeezed window wants when it is only moved, but not when it is resized', () => {
    wm().setBounds(big);
    const market = wm().open('market');
    wm().setRect(market, { x: 40, y: 20, w: 1800, h: 1100 });
    wm().setBounds(small);
    const squeezed = wm().windows[0]!.rect;
    wm().setRect(market, { ...squeezed, x: squeezed.x + 30 });
    wm().setBounds(big);
    expect(wm().windows[0]!.rect).toMatchObject({ x: squeezed.x + 30, w: 1800, h: 1100 });
    wm().setBounds(small);
    wm().setRect(market, { x: 0, y: 0, w: 900, h: 600 });
    wm().setBounds(big);
    expect(rectOf('market')).toEqual({ w: 900, h: 600 });
  });

  it('saves the sizes the windows want, not the squeezed ones', () => {
    wm().setBounds(big);
    wm().setRect(wm().open('market'), { x: 40, y: 20, w: 1800, h: 1100 });
    wm().setBounds(small);
    const saved = wm().snapshot();
    expect(saved.windows[0]!.rect).toMatchObject({ w: 1800, h: 1100 });
    expect(saved.geometry.market?.rect).toMatchObject({ w: 1800, h: 1100 });
  });

  it('keeps both halves of a column wanting one width when a window joins it on a small desktop', () => {
    wm().setBounds(big);
    const market = wm().open('market');
    wm().dock(market, 'top-left');
    wm().setDockSize(market, 900);
    wm().setBounds(small);
    wm().open('settings', {}, 'bottom-left');
    wm().setBounds(big);
    wm().close(market);
    wm().setBounds({ w: 2400, h: 1390 });
    expect(placement('settings')?.dockWidth).toBe(900);
    expect(wm().snapshot().windows[0]!.dockWidth).toBe(900);
  });

  it('gives a pin its width back after Restore or a reopen on a small desktop', () => {
    wm().setBounds(big);
    const market = wm().open('market');
    wm().dock(market, 'right');
    wm().setDockSize(market, 900);
    wm().toggleMaximize(market);
    wm().setBounds(small);
    wm().toggleMaximize(market);
    expect(placement('market')).toMatchObject({ mode: 'docked', slot: 'right' });
    wm().setBounds(big);
    expect(placement('market')?.dockWidth).toBe(900);
    wm().close(market);
    wm().setBounds(small);
    wm().toggle('market');
    wm().setBounds(big);
    expect(placement('market')).toMatchObject({ mode: 'docked', slot: 'right', dockWidth: 900 });
  });

  it('only squeezes the column across from a new pin for as long as the desktop is small', () => {
    wm().setBounds(big);
    const market = wm().open('market');
    wm().dock(market, 'left');
    wm().setDockSize(market, 900);
    wm().setBounds(small);
    wm().dock(wm().open('settings'), 'right');
    expect(placement('market')?.dockWidth).toBeLessThan(900);
    wm().setBounds(big);
    expect(placement('market')?.dockWidth).toBe(900);
    expect(placement('settings')?.dockWidth).toBe(WINDOW_SPECS.settings.defaultDockWidth);
  });

  it('shows after every action exactly what fitting it into the same desktop again gives', () => {
    const shown = () =>
      wm().windows.map(({ mode, slot, rect, dockWidth, dockHeight }) => ({ mode, slot, rect, dockWidth, dockHeight }));
    const steady = () => {
      const before = shown();
      wm().setBounds({ ...wm().bounds });
      expect(shown()).toEqual(before);
    };
    wm().setBounds(big);
    const market = wm().open('market');
    wm().dock(market, 'left');
    wm().setDockSize(market, 900);
    const settings = wm().open('settings');
    wm().dock(settings, 'right');
    wm().setDockSize(settings, 700);
    wm().setBounds(small);
    steady();
    wm().setDockSize(settings, 400);
    steady();
    // The column across takes back the room, up to what it wants.
    expect(placement('market')?.dockWidth).toBeGreaterThan(340);
    wm().setDockSize(market, 300);
    steady();
    wm().toggleMaximize(settings);
    steady();
    wm().toggleMaximize(settings);
    steady();
    wm().float(market);
    steady();
    wm().setBounds(big);
    expect(placement('settings')?.dockWidth).toBe(400);
  });

  it('gives a split column its share back', () => {
    wm().setBounds(big);
    wm().setSplit('left', 0.12);
    wm().setBounds(small);
    expect(wm().splits.left).toBeGreaterThan(0.12);
    wm().setBounds(big);
    expect(wm().splits.left).toBe(0.12);
  });
});

describe('the navigation rail', () => {
  it('opens a closed window and closes one in plain view', () => {
    wm().toggle('settings');
    expect(placement('settings')).toBeDefined();
    wm().toggle('settings');
    expect(placement('settings')).toBeUndefined();
  });

  it('brings a window hidden behind another forward before closing it', () => {
    const settings = wm().open('settings');
    const market = wm().open('market');
    wm().toggleMaximize(market);
    wm().toggle('settings');
    expect(wm().focusedId).toBe(settings);
    wm().toggle('settings');
    expect(placement('settings')).toBeUndefined();
  });

  it('closes a window on top even when most of it is off the desktop', () => {
    wm().setBounds({ w: 1400, h: 900 });
    const id = wm().open('settings');
    wm().setRect(id, { x: -800, y: 100, w: 980, h: 680 });
    wm().toggle('settings');
    expect(placement('settings')).toBeUndefined();
  });
});
