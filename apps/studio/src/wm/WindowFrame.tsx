import { useRef, useState, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import {
  Copy,
  Maximize2,
  PanelLeft,
  PanelRight,
  Pin,
  PinOff,
  Square,
  X,
  type LucideIcon,
} from 'lucide-react';
import { IconButton, Menu, cx, type MenuEntry } from '@demido/ui';

import { WINDOW_SPECS, type WindowState, useWindows } from '@/stores/windows';
import {
  type Edge,
  type SnapZone,
  displayRect,
  previewRect,
  resizeRect,
  snapZone,
} from './geometry';
import styles from './WindowFrame.module.css';

interface Props {
  win: WindowState;
  focused: boolean;
  icon: LucideIcon;
  title: string;
  children: ReactNode;
}

const EDGES: Edge[] = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'];

function layerBox(el: Element): DOMRect {
  return (el.closest('[data-window-layer]') ?? document.body).getBoundingClientRect();
}

/** A window: title bar with caption buttons, draggable, snappable, resizable. */
export function WindowFrame({ win, focused, icon: Icon, title, children }: Props) {
  const bounds = useWindows((s) => s.bounds);
  const windows = useWindows((s) => s.windows);
  const wm = useWindows.getState;
  const [interacting, setInteracting] = useState(false);
  const [menu, setMenu] = useState<'pin' | 'context' | null>(null);
  const pinRef = useRef<HTMLButtonElement>(null);
  const contextAnchor = useRef<HTMLSpanElement>(null);
  const [contextPos, setContextPos] = useState({ x: 0, y: 0 });

  const rect = displayRect(win, bounds);
  const spec = WINDOW_SPECS[win.kind];
  const otherSide = (side: 'left' | 'right') =>
    windows
      .filter((w) => w.id !== win.id && w.mode === 'docked' && w.side === (side === 'left' ? 'right' : 'left'))
      .reduce((m, w) => Math.max(m, w.dockWidth), 0);

  const onTitlePointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest('button, input, [data-no-drag]')) return;
    wm().focus(win.id);
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
      zone = snapZone(px, py, wm().bounds);
      wm().setPreview(
        zone
          ? { rect: previewRect(zone, wm().bounds, win.dockWidth, zone === 'top' ? 0 : otherSide(zone)) }
          : null,
      );
    };
    const onUp = () => {
      target.removeEventListener('pointermove', onMove);
      target.removeEventListener('pointerup', onUp);
      target.removeEventListener('pointercancel', onUp);
      wm().setPreview(null);
      setInteracting(false);
      if (!zone) return;
      if (zone === 'top') wm().toggleMaximize(win.id);
      else wm().dock(win.id, zone);
    };
    target.addEventListener('pointermove', onMove);
    target.addEventListener('pointerup', onUp);
    target.addEventListener('pointercancel', onUp);
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
    const startWidth = win.dockWidth;
    setInteracting(true);
    const onMove = (ev: PointerEvent) => {
      const dx = ev.clientX - sx;
      const dy = ev.clientY - sy;
      if (win.mode === 'docked') {
        wm().setDockWidth(win.id, win.side === 'left' ? startWidth + dx : startWidth - dx);
      } else {
        wm().setRect(win.id, resizeRect(startRect, edge, dx, dy, spec.minSize, wm().bounds));
      }
    };
    const onUp = () => {
      target.removeEventListener('pointermove', onMove);
      target.removeEventListener('pointerup', onUp);
      target.removeEventListener('pointercancel', onUp);
      setInteracting(false);
    };
    target.addEventListener('pointermove', onMove);
    target.addEventListener('pointerup', onUp);
    target.addEventListener('pointercancel', onUp);
  };

  const handles: Edge[] =
    win.mode === 'floating' ? EDGES : win.mode === 'docked' ? [win.side === 'left' ? 'e' : 'w'] : [];

  const maximized = win.mode === 'maximized';
  const docked = win.mode === 'docked';
  const pinItems: MenuEntry[] = [
    {
      id: 'left',
      label: 'Pin to the left',
      icon: PanelLeft,
      checked: docked && win.side === 'left',
      onSelect: () => wm().dock(win.id, 'left'),
    },
    {
      id: 'right',
      label: 'Pin to the right',
      icon: PanelRight,
      checked: docked && win.side === 'right',
      onSelect: () => wm().dock(win.id, 'right'),
    },
    ...(docked
      ? ([{ id: 'unpin', label: 'Unpin', icon: PinOff, onSelect: () => wm().float(win.id) }] as MenuEntry[])
      : []),
  ];
  const contextItems: MenuEntry[] = [
    ...pinItems,
    'separator',
    {
      id: 'max',
      label: maximized ? 'Restore' : 'Maximize',
      icon: maximized ? Copy : Square,
      onSelect: () => wm().toggleMaximize(win.id),
    },
    'separator',
    { id: 'close', label: 'Close', icon: X, danger: true, onSelect: () => wm().close(win.id) },
  ];

  return (
    <section
      className={cx(
        styles.frame,
        focused && styles.focused,
        interacting && styles.interacting,
        maximized && styles.maximized,
      )}
      style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h, zIndex: win.z }}
      onPointerDownCapture={() => wm().focus(win.id)}
      aria-label={title}
      role="dialog"
      data-window={win.kind}
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
          setMenu('context');
        }}
      >
        <span className={styles.titleIcon}>
          <Icon size={15} strokeWidth={1.9} aria-hidden />
        </span>
        <span className={styles.title}>{title}</span>
        <span ref={contextAnchor} className={styles.contextAnchor} style={{ left: contextPos.x, top: contextPos.y }} />
        <div className={styles.captions}>
          <IconButton
            ref={pinRef}
            icon={docked ? Pin : Pin}
            label={docked ? 'Pinned' : 'Pin to a side'}
            size="sm"
            active={docked}
            onClick={() => setMenu(menu === 'pin' ? null : 'pin')}
            tooltipPlacement="bottom"
          />
          <IconButton
            icon={maximized ? Copy : Maximize2}
            label={maximized ? 'Restore' : 'Maximize'}
            size="sm"
            onClick={() => wm().toggleMaximize(win.id)}
            tooltipPlacement="bottom"
          />
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
      <Menu open={menu === 'pin'} onClose={() => setMenu(null)} anchorRef={pinRef} items={pinItems} width={190} />
      <Menu
        open={menu === 'context'}
        onClose={() => setMenu(null)}
        anchorRef={contextAnchor}
        items={contextItems}
        placement="bottom-start"
        width={200}
      />
    </section>
  );
}
