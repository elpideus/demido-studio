import { useMemo, useRef, useState } from 'react';
import { MessagesSquare, MoreHorizontal, Pencil, Pin, PinOff, Search, SquarePen, Trash2 } from 'lucide-react';
import { Button, Dialog, EmptyState, IconButton, Menu, TextField, cx, matchesQuery } from '@demido/ui';

import { recencyGroup } from '@/lib/format';
import type { Chat } from '@/lib/types';
import { useChats } from '@/stores/chats';
import styles from './ChatList.module.css';

const GROUP_ORDER = ['Pinned', 'Today', 'Yesterday', 'Previous 7 days', 'Previous 30 days', 'Older'];

function ChatRow({
  chat,
  active,
  running,
  onRename,
  onDelete,
}: {
  chat: Chat;
  active: boolean;
  running: boolean;
  onRename: () => void;
  onDelete: () => void;
}) {
  const open = useChats((s) => s.open);
  const pin = useChats((s) => s.pin);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLButtonElement>(null);
  return (
    <div className={cx(styles.row, active && styles.active, menuOpen && styles.menuOpen)}>
      <button type="button" className={styles.rowButton} onClick={() => void open(chat.id)} title={chat.title}>
        {running && <span className={styles.running} aria-label="Answering" />}
        <span className={styles.rowTitle}>{chat.title}</span>
      </button>
      <IconButton
        ref={menuRef}
        icon={MoreHorizontal}
        label="Chat options"
        size="xs"
        tooltip={false}
        className={styles.more}
        onClick={() => setMenuOpen(true)}
      />
      <Menu
        open={menuOpen}
        onClose={() => setMenuOpen(false)}
        anchorRef={menuRef}
        placement="bottom-start"
        items={[
          { id: 'rename', label: 'Rename', icon: Pencil, onSelect: onRename },
          {
            id: 'pin',
            label: chat.pinned ? 'Unpin' : 'Pin',
            icon: chat.pinned ? PinOff : Pin,
            onSelect: () => void pin(chat.id, !chat.pinned),
          },
          'separator',
          { id: 'delete', label: 'Delete', icon: Trash2, danger: true, onSelect: onDelete },
        ]}
      />
    </div>
  );
}

function RenameRow({ chat, onDone }: { chat: Chat; onDone: () => void }) {
  const rename = useChats((s) => s.rename);
  const [value, setValue] = useState(chat.title);
  const commit = () => {
    if (value.trim() && value.trim() !== chat.title) void rename(chat.id, value.trim());
    onDone();
  };
  return (
    <div className={cx(styles.row, styles.active)}>
      <input
        autoFocus
        className={styles.renameInput}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
          if (e.key === 'Escape') onDone();
        }}
        onFocus={(e) => e.target.select()}
      />
    </div>
  );
}

/** The list of conversations, grouped by recency, shown beside the chat. */
export function ChatList() {
  const chats = useChats((s) => s.chats);
  const activeId = useChats((s) => s.activeId);
  const running = useChats((s) => s.running);
  const open = useChats((s) => s.open);
  const remove = useChats((s) => s.remove);
  const [query, setQuery] = useState('');
  const [renaming, setRenaming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<Chat | null>(null);

  const groups = useMemo(() => {
    const filtered = query ? chats.filter((c) => matchesQuery(c.title, query)) : chats;
    const map = new Map<string, Chat[]>();
    for (const chat of filtered) {
      const key = chat.pinned ? 'Pinned' : recencyGroup(chat.updatedAt);
      map.set(key, [...(map.get(key) ?? []), chat]);
    }
    return GROUP_ORDER.filter((g) => map.has(g)).map((g) => ({ label: g, chats: map.get(g)! }));
  }, [chats, query]);

  return (
    <aside className={styles.panel} aria-label="Chats">
      <header className={styles.header}>
        <span className={styles.heading}>Chats</span>
        <IconButton icon={SquarePen} label="New chat (Ctrl+N)" size="sm" onClick={() => void open(null)} />
      </header>
      <div className={styles.search}>
        <TextField
          icon={Search}
          size="sm"
          placeholder="Search chats"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <div className={styles.list}>
        {groups.map((group) => (
          <section key={group.label} className={styles.group}>
            <h3 className={styles.groupLabel}>{group.label}</h3>
            {group.chats.map((chat) =>
              renaming === chat.id ? (
                <RenameRow key={chat.id} chat={chat} onDone={() => setRenaming(null)} />
              ) : (
                <ChatRow
                  key={chat.id}
                  chat={chat}
                  active={chat.id === activeId}
                  running={!!running[chat.id]}
                  onRename={() => setRenaming(chat.id)}
                  onDelete={() => setDeleting(chat)}
                />
              ),
            )}
          </section>
        ))}
        {groups.length === 0 && (
          <EmptyState
            compact
            icon={MessagesSquare}
            title={query ? 'No matching chats' : 'No chats yet'}
            description={query ? 'Try other words.' : 'Your conversations will appear here.'}
          />
        )}
      </div>
      <Dialog
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        title="Delete this chat?"
        description={deleting?.title}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              icon={Trash2}
              onClick={() => {
                if (deleting) void remove(deleting.id);
                setDeleting(null);
              }}
            >
              Delete
            </Button>
          </>
        }
      >
        The conversation and the files the assistant created in it will be deleted. This cannot be undone.
      </Dialog>
    </aside>
  );
}
