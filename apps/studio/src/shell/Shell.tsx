import { useMemo } from 'react';

import { ChatList } from '@/chat/ChatList';
import { ChatView } from '@/chat/ChatView';
import { useApp } from '@/stores/app';
import { useWindows } from '@/stores/windows';
import { WindowLayer } from '@/wm/WindowLayer';
import { chatInsets } from '@/wm/geometry';
import { ActivityBar } from './ActivityBar';
import styles from './Shell.module.css';

/**
 * The desktop: the navigation rail, then the chat (always present, never a window) with the
 * windows floating above it. Windows pinned to a side take their width away from the chat.
 */
export function Shell() {
  const windows = useWindows((s) => s.windows);
  const chatListOpen = useApp((s) => s.settings?.chatListOpen ?? true);
  const insets = useMemo(() => chatInsets(windows), [windows]);

  return (
    <div className={styles.shell}>
      <ActivityBar />
      <main className={styles.desktop}>
        <div className={styles.workspace} style={{ paddingLeft: insets.left, paddingRight: insets.right }}>
          {chatListOpen && <ChatList />}
          <ChatView />
        </div>
        <WindowLayer />
      </main>
    </div>
  );
}
