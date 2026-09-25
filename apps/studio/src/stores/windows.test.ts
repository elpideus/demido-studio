import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { useWindows as UseWindows } from './windows';

let useWindows: typeof UseWindows;

// Every test gets a fresh store: remembered geometry lives at module level.
beforeEach(async () => {
  vi.resetModules();
  ({ useWindows } = await import('./windows'));
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
