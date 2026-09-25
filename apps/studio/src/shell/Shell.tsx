import { useMemo } from 'react';
import { cx } from '@demido/ui';

import { ChatList } from '@/chat/ChatList';
import { ChatView } from '@/chat/ChatView';
import { useApp } from '@/stores/app';
import { useWindows } from '@/stores/windows';
import { WindowLayer } from '@/wm/WindowLayer';
import { chatInsets, dockLayout } from '@/wm/geometry';
import { ActivityBar } from './ActivityBar';
import styles from './Shell.module.css';

/**
 * The desktop: the navigation rail, then the chat (always present, never a window) with the
 * windows floating above it. Pinned windows take their space away from the chat.
 */
export function Shell() {
  const windows = useWindows((s) => s.windows);
  const splits = useWindows((s) => s.splits);
  const resizing = useWindows((s) => s.resizing);
  const chatListOpen = useApp((s) => s.settings?.chatListOpen ?? true);
  const insets = useMemo(() => chatInsets(dockLayout(windows, splits)), [windows, splits]);

  return (
    <div className={styles.shell}>
      <ActivityBar />
      <main className={styles.desktop}>
        <div
          className={cx(styles.workspace, resizing && styles.resizing)}
          style={{
            paddingLeft: insets.left,
            paddingRight: insets.right,
            paddingTop: insets.top,
            paddingBottom: insets.bottom,
          }}
        >
          {chatListOpen && <ChatList />}
          <ChatView />
        </div>
        <WindowLayer />
      </main>
    </div>
  );
}
