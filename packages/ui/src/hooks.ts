import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';

/** Calls `handler` on a pointer press outside every element in `refs`. */
export function useClickOutside(refs: Array<RefObject<HTMLElement | null>>, handler: () => void, enabled = true): void {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    if (!enabled) return undefined;
    const onDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (refs.some((r) => r.current?.contains(target))) return;
      latest.current();
    };
    // Capture phase, so a press on something that stops propagation still closes us.
    document.addEventListener('pointerdown', onDown, true);
    return () => document.removeEventListener('pointerdown', onDown, true);
  }, [enabled, refs]);
}

/** Calls `handler` when Escape is pressed. */
export function useEscape(handler: () => void, enabled = true): void {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    if (!enabled) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        latest.current();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [enabled]);
}

export type Placement =
  'top-start' | 'top-end' | 'top' | 'bottom-start' | 'bottom-end' | 'bottom' | 'right' | 'right-start' | 'left';

interface Position {
  top: number;
  left: number;
  placement: Placement;
}

const GAP = 6;
const MARGIN = 8;

/**
 * Positions a floating element next to an anchor, flipping to the opposite side when it would
 * leave the viewport and clamping it inside the viewport otherwise.
 */
export function useAnchoredPosition(
  anchor: RefObject<HTMLElement | null>,
  floating: RefObject<HTMLElement | null>,
  placement: Placement,
  open: boolean,
): Position | null {
  const [pos, setPos] = useState<Position | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return undefined;
    }
    const update = () => {
      const a = anchor.current?.getBoundingClientRect();
      const f = floating.current?.getBoundingClientRect();
      if (!a || !f) return;
      const vw = window.innerWidth;
      const vh = window.innerHeight;
      let p = placement;
      const side = p.split('-')[0];
      if (side === 'top' && a.top - f.height - GAP < MARGIN && a.bottom + f.height + GAP < vh) {
        p = p.replace('top', 'bottom') as Placement;
      } else if (side === 'bottom' && a.bottom + f.height + GAP > vh - MARGIN && a.top - f.height - GAP > MARGIN) {
        p = p.replace('bottom', 'top') as Placement;
      }
      let top = 0;
      let left = 0;
      switch (p) {
        case 'top-start':
          top = a.top - f.height - GAP;
          left = a.left;
          break;
        case 'top-end':
          top = a.top - f.height - GAP;
          left = a.right - f.width;
          break;
        case 'top':
          top = a.top - f.height - GAP;
          left = a.left + a.width / 2 - f.width / 2;
          break;
        case 'bottom-start':
          top = a.bottom + GAP;
          left = a.left;
          break;
        case 'bottom-end':
          top = a.bottom + GAP;
          left = a.right - f.width;
          break;
        case 'bottom':
          top = a.bottom + GAP;
          left = a.left + a.width / 2 - f.width / 2;
          break;
        case 'right':
          top = a.top + a.height / 2 - f.height / 2;
          left = a.right + GAP + 2;
          break;
        case 'right-start':
          top = a.top;
          left = a.right + GAP;
          break;
        case 'left':
          top = a.top + a.height / 2 - f.height / 2;
          left = a.left - f.width - GAP - 2;
          break;
      }
      top = Math.max(MARGIN, Math.min(top, vh - f.height - MARGIN));
      left = Math.max(MARGIN, Math.min(left, vw - f.width - MARGIN));
      setPos((prev) =>
        prev && prev.top === top && prev.left === left && prev.placement === p ? prev : { top, left, placement: p },
      );
    };
    update();
    const observer = new ResizeObserver(update);
    if (floating.current) observer.observe(floating.current);
    if (anchor.current instanceof Element) observer.observe(anchor.current);
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', update, true);
    };
  }, [anchor, floating, placement, open]);

  return pos;
}

/** A value that trails `value` by `delay` ms (for search boxes). */
export function useDebounced<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = window.setTimeout(() => setDebounced(value), delay);
    return () => window.clearTimeout(id);
  }, [value, delay]);
  return debounced;
}
