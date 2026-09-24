import { useEffect, useRef } from 'react';
import { BookOpenText, CandlestickChart, Settings, type LucideIcon } from 'lucide-react';

import { InspectorWindow } from '@/inspector/InspectorWindow';
import { MarketWindow } from '@/market/MarketWindow';
import { SettingsWindow } from '@/settings/SettingsWindow';
import { WINDOW_SPECS, type WindowKind, type WindowState, useWindows } from '@/stores/windows';
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

/** Every open window, above the chat, plus the outline shown while a drag would snap. */
export function WindowLayer() {
  const ref = useRef<HTMLDivElement>(null);
  const windows = useWindows((s) => s.windows);
  const focusedId = useWindows((s) => s.focusedId);
  const preview = useWindows((s) => s.preview);
  const setBounds = useWindows((s) => s.setBounds);

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
