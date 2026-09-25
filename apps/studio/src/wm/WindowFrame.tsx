import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { Copy, LayoutGrid, Maximize2, PinOff, Square, X, type LucideIcon } from 'lucide-react';
import { IconButton, Menu, cx, type MenuEntry } from '@demido/ui';

import { WINDOW_SPECS, type WindowState, useWindows } from '@/stores/windows';
import {
  type Edge,
  type SnapZone,
  GAP,
  columnOf,
  displayRect,
  dockLayout,
  dockedHandles,
  isRow,
  previewRect,
  resizeRect,
  snapZone,
} from './geometry';
import { SnapLayouts } from './SnapLayouts';
import styles from './WindowFrame.module.css';

interface Props {
  win: WindowState;
  focused: boolean;
  icon: LucideIcon;
  title: string;
  children: ReactNode;
}

const EDGES: Edge[] = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'];
/** Hover time on Maximize before the snap layouts open, and grace time after the pointer leaves. */
const FLYOUT_OPEN_DELAY = 380;
const FLYOUT_CLOSE_DELAY = 260;

function layerBox(el: Element): DOMRect {
  return (el.closest('[data-window-layer]') ?? document.body).getBoundingClientRect();
}

/** A window: title bar with caption buttons, draggable, snappable, resizable. */
export function WindowFrame({ win, focused, icon: Icon, title, children }: Props) {
  const bounds = useWindows((s) => s.bounds);
  const windows = useWindows((s) => s.windows);
  const splits = useWindows((s) => s.splits);
  const resizing = useWindows((s) => s.resizing);
  const wm = useWindows.getState;
  const [interacting, setInteracting] = useState(false);
  const [menu, setMenu] = useState(false);
  const [flyout, setFlyout] = useState<'hover' | 'keyboard' | null>(null);
  const flyoutTimer = useRef<number | undefined>(undefined);
  const maxRef = useRef<HTMLButtonElement>(null);
  const contextAnchor = useRef<HTMLSpanElement>(null);
  const [contextPos, setContextPos] = useState({ x: 0, y: 0 });

  /** Stops the drag or resize in progress; a window can close in the middle of one (Ctrl+W). */
  const endDrag = useRef<(() => void) | null>(null);

  useEffect(
    () => () => {
      window.clearTimeout(flyoutTimer.current);
      endDrag.current?.();
    },
    [],
  );

  const rect = displayRect(win, bounds, dockLayout(windows, splits));
  const spec = WINDOW_SPECS[win.kind];

  const hoverFlyout = (show: boolean) => {
    window.clearTimeout(flyoutTimer.current);
    if (show && interacting) return;
    if (show && flyout) return;
    flyoutTimer.current = window.setTimeout(
      () => setFlyout(show ? 'hover' : null),
      show ? FLYOUT_OPEN_DELAY : FLYOUT_CLOSE_DELAY,
    );
  };
  const closeFlyout = () => {
    window.clearTimeout(flyoutTimer.current);
    // Opened from the keyboard, focus goes back where it came from instead of to the page.
    if (flyout === 'keyboard') maxRef.current?.focus();
    setFlyout(null);
  };

  const onTitlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest('button, input, [data-no-drag]')) return;
    wm().focus(win.id);
    closeFlyout();
    const box = layerBox(e.currentTarget);
    const target = e.currentTarget;
    target.setPointerCapture(e.pointerId);
    const startPx = e.clientX - box.left;
    const startPy = e.clientY - box.top;
    const grabRatio = (startPx - rect.x) / Math.max(rect.w, 1);
    let origin = { ...rect };
    let originPx = startPx;
    let originPy = startPy;
    let torn = win.mode === 'floating';
    let moved = false;
    let zone: SnapZone | null = null;

    const onMove = (ev: PointerEvent) => {
      const px = ev.clientX - box.left;
      const py = ev.clientY - box.top;
      if (!moved) {
        if (Math.hypot(px - startPx, py - startPy) < 4) return;
        moved = true;
        setInteracting(true);
      }
      if (!torn) {
        wm().tearOff(win.id, px, py, grabRatio);
        const now = wm().windows.find((w) => w.id === win.id);
        if (now) origin = { ...now.rect };
        originPx = px;
        originPy = py;
        torn = true;
        return;
      }
      wm().setRect(win.id, { ...origin, x: origin.x + (px - originPx), y: origin.y + (py - originPy) });
      const state = wm();
      zone = snapZone(px, py, state.bounds);
      state.setPreview(zone ? { rect: previewRect(zone, state.windows, win.id, state.bounds, state.splits) } : null);
    };
    const detach = () => {
      target.removeEventListener('pointermove', onMove);
      target.removeEventListener('pointerup', onUp);
      target.removeEventListener('pointercancel', onUp);
      target.removeEventListener('lostpointercapture', onUp);
      wm().setPreview(null);
      endDrag.current = null;
    };
    const onUp = () => {
      detach();
      setInteracting(false);
      if (!zone) return;
      if (zone === 'maximize') wm().toggleMaximize(win.id);
      else wm().dock(win.id, zone);
    };
    target.addEventListener('pointermove', onMove);
    target.addEventListener('pointerup', onUp);
    target.addEventListener('pointercancel', onUp);
    target.addEventListener('lostpointercapture', onUp);
    endDrag.current = detach;
  };

  const onResizeDown = (edge: Edge) => (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    wm().focus(win.id);
    const target = e.currentTarget;
    target.setPointerCapture(e.pointerId);
    const sx = e.clientX;
    const sy = e.clientY;
    const startRect = { ...win.rect };
    const startShown = { ...rect };
    const column = win.slot ? columnOf(win.slot) : null;
    // Height of a split column's top half; the edge between the halves moves where it splits.
    const usable = bounds.h - 3 * GAP;
    const startTop = edge === 's' ? startShown.h : usable * splits[column ?? 'left'];
    const docked = win.mode === 'docked';
    setInteracting(true);
    if (docked) wm().setResizing(true);
    const onMove = (ev: PointerEvent) => {
      const dx = ev.clientX - sx;
      const dy = ev.clientY - sy;
      if (win.mode !== 'docked' || !win.slot) {
        wm().setRect(win.id, resizeRect(startRect, edge, dx, dy, spec.minSize, wm().bounds));
      } else if (edge === 'e' || edge === 'w') {
        wm().setDockSize(win.id, edge === 'e' ? startShown.w + dx : startShown.w - dx);
      } else if (isRow(win.slot)) {
        wm().setDockSize(win.id, edge === 's' ? startShown.h + dy : startShown.h - dy);
      } else if (column) {
        wm().setSplit(column, (startTop + dy) / usable);
      }
    };
    const detach = () => {
      target.removeEventListener('pointermove', onMove);
      target.removeEventListener('pointerup', onUp);
      target.removeEventListener('pointercancel', onUp);
      target.removeEventListener('lostpointercapture', onUp);
      if (docked) wm().setResizing(false);
      endDrag.current = null;
    };
    const onUp = () => {
      detach();
      setInteracting(false);
    };
    target.addEventListener('pointermove', onMove);
    target.addEventListener('pointerup', onUp);
    target.addEventListener('pointercancel', onUp);
    target.addEventListener('lostpointercapture', onUp);
    endDrag.current = detach;
  };

  const maximized = win.mode === 'maximized';
  const docked = win.mode === 'docked' && win.slot !== null;
  const handles: Edge[] = win.mode === 'floating' ? EDGES : docked && win.slot ? dockedHandles(win.slot) : [];

  const contextItems: MenuEntry[] = [
    {
      id: 'max',
      label: maximized ? 'Restore' : 'Maximize',
      icon: maximized ? Copy : Square,
      onSelect: () => wm().toggleMaximize(win.id),
    },
    {
      id: 'snap',
      label: 'Pin to…',
      icon: LayoutGrid,
      onSelect: () => window.setTimeout(() => setFlyout('keyboard'), 0),
    },
    ...(docked
      ? ([{ id: 'unpin', label: 'Unpin', icon: PinOff, onSelect: () => wm().float(win.id) }] as MenuEntry[])
      : []),
    'separator',
    { id: 'close', label: 'Close', icon: X, danger: true, onSelect: () => wm().close(win.id) },
  ];

  return (
    <section
      className={cx(
        styles.frame,
        focused && styles.focused,
        (interacting || resizing) && styles.interacting,
        maximized && styles.maximized,
      )}
      style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h, zIndex: win.z }}
      onPointerDownCapture={() => wm().focus(win.id)}
      aria-label={title}
      role="dialog"
      data-window={win.kind}
      data-mode={win.mode}
      data-slot={win.slot ?? undefined}
    >
      <div
        className={styles.titleBar}
        onPointerDown={onTitlePointerDown}
        onDoubleClick={(e) => {
          if (!(e.target as HTMLElement).closest('button')) wm().toggleMaximize(win.id);
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          const box = e.currentTarget.getBoundingClientRect();
          setContextPos({ x: e.clientX - box.left, y: e.clientY - box.top });
          setMenu(true);
        }}
      >
        <span className={styles.titleIcon}>
          <Icon size={15} strokeWidth={1.9} aria-hidden />
        </span>
        <span className={styles.title}>{title}</span>
        <span ref={contextAnchor} className={styles.contextAnchor} style={{ left: contextPos.x, top: contextPos.y }} />
        <div className={styles.captions}>
          <span
            className={styles.maxWrap}
            onPointerEnter={(e) => e.pointerType === 'mouse' && hoverFlyout(true)}
            onPointerLeave={(e) => e.pointerType === 'mouse' && flyout !== 'keyboard' && hoverFlyout(false)}
          >
            <IconButton
              ref={maxRef}
              icon={maximized ? Copy : Maximize2}
              label={maximized ? 'Restore' : 'Maximize'}
              size="sm"
              tooltip={false}
              aria-haspopup="dialog"
              aria-expanded={flyout !== null}
              aria-keyshortcuts="ArrowDown"
              onClick={() => {
                closeFlyout();
                wm().toggleMaximize(win.id);
              }}
              onKeyDown={(e) => {
                if (e.key === 'ArrowDown') {
                  e.preventDefault();
                  setFlyout('keyboard');
                }
              }}
            />
          </span>
          <IconButton
            icon={X}
            label="Close"
            size="sm"
            className={styles.close}
            onClick={() => wm().close(win.id)}
            tooltipPlacement="bottom"
          />
        </div>
      </div>
      <div className={styles.body}>{children}</div>
      {handles.map((edge) => (
        <div key={edge} className={cx(styles.handle, styles[`h-${edge}`])} onPointerDown={onResizeDown(edge)} />
      ))}
      <SnapLayouts
        open={flyout !== null}
        onClose={closeFlyout}
        anchorRef={maxRef}
        current={docked ? win.slot : null}
        autoFocus={flyout === 'keyboard'}
        onPick={(slot) => wm().dock(win.id, slot)}
        onUnpin={() => wm().float(win.id)}
        onPointerEnter={() => flyout === 'hover' && hoverFlyout(true)}
        onPointerLeave={() => flyout === 'hover' && hoverFlyout(false)}
      />
      <Menu
        open={menu}
        onClose={() => setMenu(false)}
        anchorRef={contextAnchor}
        items={contextItems}
        placement="bottom-start"
        width={200}
      />
    </section>
  );
}
