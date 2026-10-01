import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { ArrowDown, FileUp, FolderOpen } from 'lucide-react';
import { IconButton } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import type { Message } from '@/lib/types';
import { currentModel, useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { toast } from '@/stores/toasts';
import { ATTACH_FILES_EVENT, MAX_FILES } from './attachmentView';
import { Composer } from './Composer';
import { EmptyChat } from './EmptyChat';
import { MessageList } from './MessageList';
import styles from './ChatView.module.css';

const NO_MESSAGES: Message[] = [];

/**
 * Whether files are being dragged over the window. Dropped files go to the composer. Tauri takes
 * drops itself (the webview's own drop events carry no paths), so this listens to the webview.
 */
function useFileDrop(): boolean {
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === 'enter') setDragging(payload.paths.length > 0);
      else if (payload.type === 'leave') setDragging(false);
      else if (payload.type === 'drop') {
        setDragging(false);
        if (payload.paths.length > 0) {
          window.dispatchEvent(new CustomEvent<string[]>(ATTACH_FILES_EVENT, { detail: payload.paths }));
        }
      }
    });
    return () => void unlisten.then((f) => f());
  }, []);
  return dragging;
}

/** The main element of the app: the conversation and its composer. */
export function ChatView() {
  const activeId = useChats((s) => s.activeId);
  const chat = useChats((s) => s.chats.find((c) => c.id === s.activeId));
  const messages = useChats((s) => (s.activeId ? (s.messages[s.activeId] ?? NO_MESSAGES) : NO_MESSAGES));
  const turnError = useChats((s) => (s.activeId ? s.turnError[s.activeId] : null));
  const picked = useChats((s) => s.pickedModelId);
  const models = useModels((s) => s.models);
  const model = currentModel(models, chat, picked);
  const [workspace, setWorkspace] = useState<string | null>(null);
  const [prefill, setPrefill] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const pinnedToBottom = useRef(true);
  const [showJump, setShowJump] = useState(false);
  const dragging = useFileDrop();

  useEffect(() => {
    setWorkspace(null);
    if (activeId) api.workspaceDir(activeId).then(setWorkspace, () => undefined);
  }, [activeId]);

  const scrollToBottom = useCallback((smooth = false) => {
    const el = scroller.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: smooth ? 'smooth' : 'auto' });
  }, []);

  // Follow the answer as it streams, unless the person scrolled up to read.
  useLayoutEffect(() => {
    if (pinnedToBottom.current) scrollToBottom();
  }, [messages, scrollToBottom]);

  useLayoutEffect(() => {
    pinnedToBottom.current = true;
    scrollToBottom();
  }, [activeId, scrollToBottom]);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    pinnedToBottom.current = distance < 80;
    setShowJump(distance > 400);
  };

  useEffect(() => {
    if (turnError) toast.error('The answer stopped', turnError);
  }, [turnError]);

  return (
    <section className={styles.view} aria-label="Chat">
      <header className={styles.header}>
        <h2 className={styles.title}>{chat?.title ?? 'New chat'}</h2>
        {activeId && workspace && (
          <IconButton
            icon={FolderOpen}
            label="Open this chat's files"
            size="sm"
            tooltipPlacement="bottom"
            onClick={() => api.openPath(workspace).catch((e) => toast.error('Could not open the folder', errorText(e)))}
          />
        )}
      </header>
      <div ref={scroller} className={styles.scroller} onScroll={onScroll}>
        {activeId && messages.length > 0 ? (
          <MessageList chatId={activeId} messages={messages} workspace={workspace} sendModelId={model?.id} />
        ) : (
          <EmptyChat model={model} onSuggest={setPrefill} />
        )}
      </div>
      {showJump && (
        <button
          type="button"
          className={styles.jump}
          onClick={() => scrollToBottom(true)}
          aria-label="Jump to the latest message"
        >
          <ArrowDown size={16} aria-hidden />
        </button>
      )}
      <Composer chatId={activeId} model={model} prefill={prefill} />
      {dragging && (
        <div className={styles.drop}>
          <div className={styles.dropZone}>
            <span className={styles.dropIcon}>
              <FileUp size={26} strokeWidth={1.6} aria-hidden />
            </span>
            <p className={styles.dropTitle}>Drop files to add them to your message</p>
            <p className={styles.dropHint}>Up to {MAX_FILES} files, 100 MB each</p>
          </div>
        </div>
      )}
    </section>
  );
}
