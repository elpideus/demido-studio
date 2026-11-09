// What the Browser window shows, worked out apart from React so it can be tested.

import type { BrowserRect, BrowserTab } from '@/lib/types';

/** A tab with nothing loaded: the window shows its start page instead. */
export function isBlank(url: string): boolean {
  return !url || url === 'about:blank';
}

/** The name on a tab: its page's title, else its address's host, else "New tab". */
export function tabLabel(tab: Pick<BrowserTab, 'title' | 'url'>): string {
  if (tab.title.trim()) return tab.title.trim();
  if (isBlank(tab.url)) return 'New tab';
  try {
    return new URL(tab.url).host || tab.url;
  } catch {
    return tab.url;
  }
}

/** What the address bar shows for a tab: its address, nothing for a blank tab. */
export function addressOf(tab: Pick<BrowserTab, 'url'> | undefined): string {
  return tab && !isBlank(tab.url) ? tab.url : '';
}

/** "2:05" until `until` (milliseconds since the epoch), never below zero. */
export function countdown(until: number, now: number): string {
  const left = Math.max(0, Math.ceil((until - now) / 1000));
  return `${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')}`;
}

/** Rects that differ by less than half a pixel are the same place: the view is not moved for them. */
export function sameRect(a: BrowserRect | null, b: BrowserRect | null): boolean {
  if (!a || !b) return a === b;
  return (
    Math.abs(a.x - b.x) < 0.5 &&
    Math.abs(a.y - b.y) < 0.5 &&
    Math.abs(a.width - b.width) < 0.5 &&
    Math.abs(a.height - b.height) < 0.5
  );
}

/**
 * Points over `rect` to test for something drawn above the page: every `step` pixels, and along
 * its edges, a pixel in. A menu, a toast or another window over any of them hides the page,
 * since nothing the app draws can show over it.
 */
export function coverPoints(rect: BrowserRect, step = 32): Array<[number, number]> {
  if (rect.width < 2 || rect.height < 2) return [];
  const axis = (start: number, size: number) => {
    const out = [start + 1];
    for (let at = start + step; at < start + size - 1; at += step) out.push(at);
    out.push(start + size - 1);
    return out;
  };
  const xs = axis(rect.x, rect.width);
  const ys = axis(rect.y, rect.height);
  return ys.flatMap((y) => xs.map((x): [number, number] => [x, y]));
}

/**
 * `rect` without the window's resize handles along its edges (`handles`: each one's edge, such as
 * "w" or "se", and where it is). The page would hide them, and they would no longer resize the
 * window. Corners are left alone: their outer part, beyond the edges' strips, still works.
 */
export function insideHandles(rect: BrowserRect, handles: Array<{ edge: string; at: BrowserRect }>): BrowserRect {
  let left = rect.x;
  let top = rect.y;
  let right = rect.x + rect.width;
  let bottom = rect.y + rect.height;
  for (const { edge, at } of handles) {
    if (edge.length !== 1) continue;
    const overlaps = at.x < right && at.x + at.width > left && at.y < bottom && at.y + at.height > top;
    if (!overlaps) continue;
    if (edge === 'w') left = Math.max(left, at.x + at.width);
    else if (edge === 'e') right = Math.min(right, at.x);
    else if (edge === 's') bottom = Math.min(bottom, at.y);
    else if (edge === 'n') top = Math.max(top, at.y + at.height);
  }
  return { x: left, y: top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) };
}

/** The tab the window shows: the active one, else the last. */
export function shownTab(tabs: BrowserTab[]): BrowserTab | undefined {
  return tabs.find((t) => t.active) ?? tabs[tabs.length - 1];
}
