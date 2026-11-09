import { describe, expect, it } from 'vitest';

import type { BrowserTab } from '@/lib/types';
import { addressOf, countdown, coverPoints, insideHandles, isBlank, sameRect, shownTab, tabLabel } from './browserView';

const tab = (over: Partial<BrowserTab>): BrowserTab => ({
  id: 1,
  url: '',
  title: '',
  loading: false,
  owner: { kind: 'user' },
  active: false,
  ...over,
});

describe('browser view', () => {
  it('names a tab by its title, then its host', () => {
    expect(tabLabel(tab({ title: '  Example Domain ' }))).toBe('Example Domain');
    expect(tabLabel(tab({ url: 'https://example.com/a?b' }))).toBe('example.com');
    expect(tabLabel(tab({ url: 'about:blank' }))).toBe('New tab');
    expect(isBlank('')).toBe(true);
    expect(addressOf(tab({ url: 'about:blank' }))).toBe('');
    expect(addressOf(tab({ url: 'https://example.com/' }))).toBe('https://example.com/');
  });

  it('counts down in minutes and seconds', () => {
    expect(countdown(185_000, 0)).toBe('3:05');
    expect(countdown(0, 5000)).toBe('0:00');
    expect(countdown(1_500, 1_000)).toBe('0:01');
  });

  it('ignores moves under half a pixel', () => {
    const r = { x: 10, y: 20, width: 300, height: 200 };
    expect(sameRect(r, { ...r, x: 10.3 })).toBe(true);
    expect(sameRect(r, { ...r, width: 301 })).toBe(false);
    expect(sameRect(null, null)).toBe(true);
    expect(sameRect(r, null)).toBe(false);
  });

  it('tests the edges and the inside of the page', () => {
    const points = coverPoints({ x: 0, y: 0, width: 100, height: 50 }, 32);
    const xs = [...new Set(points.map(([x]) => x))];
    const ys = [...new Set(points.map(([, y]) => y))];
    expect(xs).toEqual([1, 32, 64, 96, 99]);
    expect(ys).toEqual([1, 32, 49]);
    expect(coverPoints({ x: 0, y: 0, width: 1, height: 40 })).toEqual([]);
  });

  it('leaves the resize handles of the window free', () => {
    const page = { x: 100, y: 100, width: 400, height: 300 };
    const handles = [
      { edge: 'w', at: { x: 99, y: 10, width: 6, height: 380 } },
      { edge: 'e', at: { x: 495, y: 10, width: 6, height: 380 } },
      { edge: 's', at: { x: 110, y: 395, width: 380, height: 6 } },
      // Above the page: on the title bar.
      { edge: 'n', at: { x: 110, y: 50, width: 380, height: 6 } },
      { edge: 'se', at: { x: 489, y: 389, width: 12, height: 12 } },
    ];
    expect(insideHandles(page, handles)).toEqual({ x: 105, y: 100, width: 390, height: 295 });
    expect(insideHandles(page, [])).toEqual(page);
  });

  it('shows the active tab, else the last', () => {
    expect(shownTab([tab({ id: 1 }), tab({ id: 2, active: true }), tab({ id: 3 })])?.id).toBe(2);
    expect(shownTab([tab({ id: 1 }), tab({ id: 3 })])?.id).toBe(3);
    expect(shownTab([])).toBeUndefined();
  });
});
