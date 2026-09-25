import { useEffect, useMemo, useRef } from 'react';
import { BookOpenText, CandlestickChart, PanelLeft, PanelRight, Settings, type LucideIcon } from 'lucide-react';
import { Button, cx } from '@demido/ui';

import { InspectorWindow } from '@/inspector/InspectorWindow';
import { MarketWindow } from '@/market/MarketWindow';
import { SettingsWindow } from '@/settings/SettingsWindow';
import { WINDOW_SPECS, type WindowKind, type WindowState, useWindows } from '@/stores/windows';
import { type Column, type Slot, columnOf, dockLayout, slotRect } from './geometry';
import { WindowFrame } from './WindowFrame';
import styles from './WindowLayer.module.css';

const ICONS: Record<WindowKind, LucideIcon> = {
  settings: Settings,
  market: CandlestickChart,
  inspector: BookOpenText,
};

function content(win: WindowState) {
  switch (win.kind) {
    case 'settings':
      return <SettingsWindow win={win} />;
    case 'market':
      return <MarketWindow win={win} />;
    case 'inspector':
      return <InspectorWindow win={win} />;
  }
}

function title(win: WindowState): string {
  const custom = win.props.title;
  return typeof custom === 'string' && custom ? custom : WINDOW_SPECS[win.kind].title;
}

/** The empty half of a column where only one half holds a window. */
function emptyHalves(windows: WindowState[]): Slot[] {
  const empty: Slot[] = [];
  for (const column of ['left', 'right'] as Column[]) {
    const halves = windows
      .filter((w) => w.mode === 'docked' && w.slot && w.slot !== column && columnOf(w.slot) === column)
      .map((w) => w.slot);
    if (halves.length !== 1) continue;
    empty.push(halves[0]!.startsWith('top') ? (`bottom-${column}` as Slot) : (`top-${column}` as Slot));
  }
  return empty;
}

/**
 * Like Windows' snap assist: offers the other windows for the free half of a split column, or
 * to stretch the window in the other half over the whole column.
 */
function SnapAssist({ slot, windows }: { slot: Slot; windows: WindowState[] }) {
  const bounds = useWindows((s) => s.bounds);
  const splits = useWindows((s) => s.splits);
  const resizing = useWindows((s) => s.resizing);
  const rect = slotRect(slot, dockLayout(windows, splits), bounds);
  const column = columnOf(slot)!;
  const neighbour = windows.find((w) => w.mode === 'docked' && w.slot && columnOf(w.slot) === column);
  // The inspector needs an answer to show, so it is only offered when it is already open.
  const kinds = (Object.keys(WINDOW_SPECS) as WindowKind[]).filter(
    (k) => k !== neighbour?.kind && (k !== 'inspector' || windows.some((w) => w.kind === k)),
  );
  const pin = (kind: WindowKind) => {
    const wm = useWindows.getState();
    wm.dock(wm.open(kind), slot);
  };
  return (
    <div
      className={cx(styles.assist, resizing && styles.resizing)}
      style={{ left: rect.x, top: rect.y, width: rect.w, height: rect.h }}
      data-snap-assist={slot}
    >
      <span className={styles.assistText}>Pin another window here</span>
      <div className={styles.assistActions}>
        {kinds.map((kind) => (
          <Button key={kind} size="sm" variant="secondary" icon={ICONS[kind]} onClick={() => pin(kind)}>
            {WINDOW_SPECS[kind].title}
          </Button>
        ))}
      </div>
      {neighbour && (
        <Button
          size="sm"
          variant="ghost"
          icon={column === 'left' ? PanelLeft : PanelRight}
          onClick={() => useWindows.getState().dock(neighbour.id, column)}
        >
          Give {WINDOW_SPECS[neighbour.kind].title} the whole side
        </Button>
      )}
    </div>
  );
}

/** Every open window, above the chat, plus the outline shown while a drag would snap. */
export function WindowLayer() {
  const ref = useRef<HTMLDivElement>(null);
  const windows = useWindows((s) => s.windows);
  const focusedId = useWindows((s) => s.focusedId);
  const preview = useWindows((s) => s.preview);
  const setBounds = useWindows((s) => s.setBounds);
  const free = useMemo(() => emptyHalves(windows), [windows]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return undefined;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setBounds({ w: entry.contentRect.width, h: entry.contentRect.height });
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [setBounds]);

  return (
    <div ref={ref} className={styles.layer} data-window-layer>
      {free.map((slot) => (
        <SnapAssist key={slot} slot={slot} windows={windows} />
      ))}
      {preview && (
        <div
          className={styles.preview}
          style={{ left: preview.rect.x, top: preview.rect.y, width: preview.rect.w, height: preview.rect.h }}
        />
      )}
      {windows.map((win) => (
        <WindowFrame key={win.id} win={win} focused={win.id === focusedId} icon={ICONS[win.kind]} title={title(win)}>
          {content(win)}
        </WindowFrame>
      ))}
    </div>
  );
}
