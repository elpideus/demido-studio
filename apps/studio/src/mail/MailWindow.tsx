import { type KeyboardEvent, type MouseEvent, memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  AlertCircle,
  CalendarDays,
  CheckCheck,
  Download,
  ExternalLink,
  FileText,
  FolderCog,
  FolderPen,
  FolderPlus,
  FolderTree,
  Forward,
  Image as ImageIcon,
  ImageOff,
  KeyRound,
  Mail,
  MailCheck,
  MoreHorizontal,
  Paperclip,
  Plus,
  RefreshCw,
  Reply,
  ReplyAll,
  Search,
  ShieldCheck,
  SquarePen,
  Star,
  Tag,
  Trash2,
  X,
} from 'lucide-react';
import {
  Avatar,
  Button,
  Dialog,
  EmptyState,
  IconButton,
  Menu,
  type MenuEntry,
  Notice,
  SegmentedControl,
  Select,
  Spinner,
  TextField,
  cx,
  formatBytes,
  useDebounced,
} from '@demido/ui';
import { save } from '@tauri-apps/plugin-dialog';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import { formatDateTime } from '@/lib/format';
import type {
  MailAccount,
  MailAttachment,
  MailDateRange,
  MailFolder,
  MailMessage,
  MailPage,
  MailStart,
  MailSummary,
} from '@/lib/types';
import { useMail } from '@/stores/mail';
import { useApp } from '@/stores/app';
import { type ToastTone, toast, useToasts } from '@/stores/toasts';
import { type WindowState, useWindows } from '@/stores/windows';
import { type ComposeRequest, Composer } from './Composer';
import { ConnectForm } from './ConnectForm';
import { DateFilter } from './DateFilter';
import { rangeLabel } from './dateRange';
import { HtmlBody, TextBody } from './EmailBody';
import { FolderNameForm } from './FolderNameForm';
import { LinkDialog, asksFirst, openLink } from './LinkDialog';
import {
  archiveFolder,
  folderWord,
  isDraft,
  isSystemFolder,
  messageCount,
  nextAfter,
  rangeIds,
  roleFolder,
} from './mailActions';
import {
  Lru,
  accountLabel,
  folderOptions,
  listDate,
  personLabel,
  personName,
  rowPeople,
  startFolder,
} from './mailView';
import {
  BulkPane,
  FolderPicker,
  type MessageActs,
  type Opening,
  PointAnchor,
  type Point,
  ReaderToolbar,
  messageMenu,
  offers,
} from './MessageActions';
import { NameForm } from './NameForm';
import styles from './MailWindow.module.css';

/** Messages read from the cache at a time, and added as the list scrolls. */
const STEP = 100;

/** Messages opened this session, so going back to one asks nothing of the backend. */
const opened = new Lru<number, MailMessage>(30);

const SHOWN = [
  { value: 'all' as const, label: 'All' },
  { value: 'unread' as const, label: 'Unread' },
];

const NONE: ReadonlySet<number> = new Set();
/** How long a move or delete can be undone from its toast. */
const UNDO_MS = 8000;

/** A folder to make (then perhaps move mail into), rename, delete or empty. */
type FolderTask =
  | { kind: 'create'; parent: MailFolder | null; then?: (path: string) => void }
  | { kind: 'rename'; folder: MailFolder }
  | { kind: 'delete'; folder: MailFolder }
  | { kind: 'empty'; folder: MailFolder };

const capital = (word: string) => word.charAt(0).toUpperCase() + word.slice(1);

/** Email browsing: the folder's messages on the left, the open message on the right. */
export function MailWindow({ win }: { win: WindowState }) {
  const accounts = useMail((s) => s.accounts);

  // The inboxes are watched from a moment after the app starts; while this window is open their
  // connections also stay open, so opening a message asks for nothing but its body.
  useEffect(() => {
    void api.mailWindowOpen(true);
    return () => void api.mailWindowOpen(false);
  }, []);

  if (!accounts.length) {
    return (
      <div className={styles.welcome}>
        <div className={styles.welcomeCard}>
          <Mail size={26} className={styles.welcomeIcon} />
          <h2 className={styles.welcomeTitle}>Connect your email</h2>
          <p className={styles.welcomeText}>
            Browse your mail here, and let the assistant find and read messages for you.
          </p>
          <ConnectForm onDone={(a) => useWindows.getState().setProps(win.id, { account: a.id, folder: null })} />
        </div>
      </div>
    );
  }
  return <MailBrowser win={win} accounts={accounts} />;
}

function MailBrowser({ win, accounts }: { win: WindowState; accounts: MailAccount[] }) {
  const setProps = useWindows((s) => s.setProps);
  const account = accounts.find((a) => a.id === win.props.account) ?? accounts[0]!;
  const [folders, setFolders] = useState<MailFolder[] | null>(null);
  const [connecting, setConnecting] = useState<'new' | MailAccount | null>(null);
  const [naming, setNaming] = useState<MailAccount | null>(null);
  const [removing, setRemoving] = useState<MailAccount | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuAnchor = useRef<HTMLButtonElement>(null);
  const alwaysImages = useApp((s) => s.settings?.mailShowImages ?? false);
  const markRead = useApp((s) => s.settings?.mailMarkRead ?? true);
  const confirmLinks = useApp((s) => s.settings?.mailConfirmLinks ?? true);
  const patchSettings = useApp((s) => s.patchSettings);
  const [folderMenuOpen, setFolderMenuOpen] = useState(false);
  const folderMenuAnchor = useRef<HTMLButtonElement>(null);
  const [task, setTask] = useState<FolderTask | null>(null);
  const [working, setWorking] = useState(false);
  const [compose, setCompose] = useState<ComposeRequest | null>(null);
  const [raise, setRaise] = useState(0);
  const composeDirty = useRef(false);
  const shownAccount = useRef(account.id);
  shownAccount.current = account.id;

  // The folder list comes from the cache; when it is a minute old the backend reads it from the
  // server again in the background, and says so with `mail://folders`.
  const loadFolders = useCallback(async (id: string, refresh = false) => {
    try {
      const list = await api.mailFolders(id, refresh);
      if (shownAccount.current === id) setFolders(list);
    } catch {
      if (shownAccount.current === id) setFolders((f) => f ?? []);
    }
  }, []);

  useEffect(() => {
    setFolders(null);
    void loadFolders(account.id);
  }, [account.id, loadFolders]);

  // Unread counts follow the changes the folders report, and the list follows folders and
  // labels made or deleted elsewhere.
  useEffect(() => {
    let timer = 0;
    const reload = (e: { account: string }) => {
      if (e.account !== account.id) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => void loadFolders(account.id), 400);
    };
    const unlisten = [on('mail://changed', reload), on('mail://folders', reload)];
    return () => {
      window.clearTimeout(timer);
      for (const u of unlisten) void u.then((f) => f());
    };
  }, [account.id, loadFolders]);

  // Coming back to the app checks for folders and labels made or deleted meanwhile.
  useEffect(() => {
    const onFocus = () => void loadFolders(account.id);
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [account.id, loadFolders]);

  // The saved folder shows from the cache right away, before the folder list arrives.
  const saved = win.props.folder;
  const folder = folders ? startFolder(folders, saved) : typeof saved === 'string' && saved ? saved : 'INBOX';
  const shownFolder = folders?.find((f) => f.path === folder) ?? null;
  const role = shownFolder?.role ?? (folder === 'INBOX' ? 'inbox' : null);
  const options = folders ? folderOptions(folders) : [];
  const word = folderWord(account);
  const signedIn = !account.error && !account.needsPassword;

  // One message is written at a time: one with changes in it is shown again rather than lost.
  const startCompose = useCallback(
    (start: MailStart | null, message: MailSummary | null) => {
      if (composeDirty.current) {
        setRaise((r) => r + 1);
        toast.info('You are writing a message already', 'Send it, save it as a draft or close it first.');
        return;
      }
      setCompose({ key: Date.now(), account: message?.account ?? account.id, start, id: message?.id ?? null });
    },
    [account.id],
  );
  const closeCompose = useCallback(() => {
    composeDirty.current = false;
    setCompose(null);
  }, []);
  const onComposeDirty = useCallback((dirty: boolean) => {
    composeDirty.current = dirty;
  }, []);
  const onNewFolder = useCallback(
    (then: (path: string) => void) => setTask({ kind: 'create', parent: null, then }),
    [],
  );

  const markAllRead = async () => {
    try {
      const n = await api.mailMarkFolderRead(account.id, folder);
      toast.success(n ? `${messageCount(n)} marked as read` : 'Nothing was unread', shownFolder?.name);
    } catch (e) {
      toast.error('Could not mark the folder as read', errorText(e));
    }
  };

  const folderMenu: MenuEntry[] = [
    {
      id: 'new',
      label: `New ${word}…`,
      icon: FolderPlus,
      disabled: !signedIn,
      onSelect: () => setTask({ kind: 'create', parent: null }),
    },
  ];
  if (shownFolder && !isSystemFolder(shownFolder)) {
    folderMenu.push(
      {
        id: 'inside',
        label: `New ${word} inside ${shownFolder.name}…`,
        icon: FolderTree,
        disabled: !signedIn,
        onSelect: () => setTask({ kind: 'create', parent: shownFolder }),
      },
      {
        id: 'rename',
        label: `Rename ${shownFolder.name}…`,
        icon: FolderPen,
        disabled: !signedIn,
        onSelect: () => setTask({ kind: 'rename', folder: shownFolder }),
      },
      {
        id: 'delete',
        label: `Delete ${shownFolder.name}…`,
        icon: Trash2,
        danger: true,
        disabled: !signedIn,
        onSelect: () => setTask({ kind: 'delete', folder: shownFolder }),
      },
    );
  }
  folderMenu.push('separator', {
    id: 'read',
    label: 'Mark all as read',
    icon: CheckCheck,
    disabled: !signedIn,
    onSelect: () => void markAllRead(),
  });
  if (shownFolder && (role === 'trash' || role === 'junk')) {
    folderMenu.push({
      id: 'empty',
      label: role === 'trash' ? 'Empty the trash…' : 'Empty spam…',
      icon: Trash2,
      danger: true,
      disabled: !signedIn,
      onSelect: () => setTask({ kind: 'empty', folder: shownFolder }),
    });
  }

  const createFolder = async (name: string) => {
    if (task?.kind !== 'create') return;
    const path = await api.mailCreateFolder(account.id, name, task.parent?.path ?? null);
    setTask(null);
    void loadFolders(account.id);
    toast.success(`${capital(word)} created`, name);
    task.then?.(path);
  };

  const renameFolder = async (name: string) => {
    if (task?.kind !== 'rename') return;
    const path = await api.mailRenameFolder(account.id, task.folder.path, name);
    setTask(null);
    if (task.folder.path === folder) setProps(win.id, { folder: path });
    void loadFolders(account.id);
    toast.success(`${capital(word)} renamed`, name);
  };

  // Deleting a folder and emptying one cannot be undone, so they are asked about first.
  const finishTask = async () => {
    if (!task || (task.kind !== 'delete' && task.kind !== 'empty')) return;
    const target = task.folder;
    setWorking(true);
    try {
      if (task.kind === 'delete') {
        await api.mailDeleteFolder(account.id, target.path);
        if (target.path === folder) setProps(win.id, { folder: null });
        void loadFolders(account.id);
        toast.success(`${capital(word)} deleted`, target.name);
      } else {
        const n = await api.mailEmptyFolder(account.id, target.path);
        toast.success(`${target.name} emptied`, n ? `${messageCount(n)} deleted for good` : 'It was empty already');
      }
      setTask(null);
    } catch (e) {
      toast.error(
        task.kind === 'delete' ? `Could not delete the ${word}` : `Could not empty ${target.name}`,
        errorText(e),
      );
    } finally {
      setWorking(false);
    }
  };

  // The reading preferences hold for every account; they sit here to keep the list's toolbar quiet.
  const menu: MenuEntry[] = [
    {
      id: 'images',
      label: 'Load images automatically',
      icon: ImageIcon,
      toggle: alwaysImages,
      onSelect: () => void patchSettings({ mailShowImages: !alwaysImages }),
    },
    {
      id: 'markRead',
      label: 'Mark as read when opened',
      icon: MailCheck,
      toggle: markRead,
      onSelect: () => void patchSettings({ mailMarkRead: !markRead }),
    },
    {
      id: 'links',
      label: 'Ask before opening links',
      icon: ShieldCheck,
      toggle: confirmLinks,
      onSelect: () => void patchSettings({ mailConfirmLinks: !confirmLinks }),
    },
    'separator',
    { id: 'add', label: 'Add account…', icon: Plus, onSelect: () => setConnecting('new') },
    { id: 'name', label: 'Name this account…', icon: Tag, onSelect: () => setNaming(account) },
    { id: 'password', label: 'Change password…', icon: KeyRound, onSelect: () => setConnecting(account) },
    'separator',
    {
      id: 'remove',
      label: `Remove ${account.email}`,
      icon: Trash2,
      danger: true,
      onSelect: () => setRemoving(account),
    },
  ];

  const connected = (a: MailAccount) => {
    const same = a.id === account.id;
    setConnecting(null);
    if (!same) setProps(win.id, { account: a.id, folder: null });
    toast.success(same ? 'Signed in' : 'Account connected', a.email);
  };

  return (
    <div className={styles.window}>
      <div className={styles.browser}>
        <div className={styles.sideHead}>
          <div className={styles.accountRow}>
            {accounts.length > 1 ? (
              <Select
                size="sm"
                className={styles.grow}
                aria-label="Account"
                options={accounts.map((a) => ({ value: a.id, label: accountLabel(a) }))}
                value={account.id}
                onChange={(e) => setProps(win.id, { account: e.target.value, folder: null })}
              />
            ) : (
              <span className={styles.accountName} title={accountLabel(account)}>
                {accountLabel(account)}
              </span>
            )}
            <IconButton
              icon={SquarePen}
              label="New message"
              size="sm"
              variant="accent"
              onClick={() => startCompose(null, null)}
            />
            <IconButton
              ref={menuAnchor}
              icon={MoreHorizontal}
              label="Mail settings and accounts"
              size="sm"
              active={menuOpen}
              onClick={() => setMenuOpen((o) => !o)}
            />
            <Menu open={menuOpen} onClose={() => setMenuOpen(false)} anchorRef={menuAnchor} items={menu} width={280} />
          </div>
          {options.length > 0 && (
            <div className={styles.accountRow}>
              <Select
                size="sm"
                className={styles.grow}
                aria-label="Folder"
                options={options}
                value={folder}
                onChange={(e) => setProps(win.id, { folder: e.target.value })}
              />
              <IconButton
                ref={folderMenuAnchor}
                icon={FolderCog}
                label={`${capital(word)} actions`}
                size="sm"
                active={folderMenuOpen}
                onClick={() => setFolderMenuOpen((o) => !o)}
              />
              <Menu
                open={folderMenuOpen}
                onClose={() => setFolderMenuOpen(false)}
                anchorRef={folderMenuAnchor}
                items={folderMenu}
                width={250}
              />
            </div>
          )}
          {(account.error || account.needsPassword) && (
            <Notice
              tone="danger"
              action={
                <Button size="sm" variant="secondary" onClick={() => setConnecting(account)}>
                  Sign in
                </Button>
              }
            >
              {account.error ?? 'The password of this account is missing.'}
            </Notice>
          )}
          {account.sessionOnly && (
            <Notice tone="warning">The credential store refused the password: it is kept until Demido closes.</Notice>
          )}
        </div>
        <FolderView
          key={`${account.id}\n${folder}`}
          account={account}
          folder={folder}
          role={role}
          folders={folders}
          onRefreshFolders={() => void loadFolders(account.id, true)}
          onCompose={startCompose}
          onNewFolder={onNewFolder}
        />
      </div>
      {compose && (
        <Composer
          key={compose.key}
          request={compose}
          accounts={accounts}
          raise={raise}
          onDirty={onComposeDirty}
          onClose={closeCompose}
        />
      )}
      <Dialog
        open={task?.kind === 'create' || task?.kind === 'rename'}
        onClose={() => setTask(null)}
        title={
          task?.kind === 'rename'
            ? `Rename ${task.folder.name}`
            : task?.kind === 'create' && task.parent
              ? `New ${word} inside ${task.parent.name}`
              : `New ${word}`
        }
        description={task?.kind === 'create' && task.then ? `The mail goes into it once it is made.` : undefined}
        width={420}
      >
        {task?.kind === 'create' && (
          <FolderNameForm
            label="Name"
            description={task.parent ? undefined : `Write Work/Clients to put it inside Work.`}
            submitLabel="Create"
            onSubmit={createFolder}
            onCancel={() => setTask(null)}
          />
        )}
        {task?.kind === 'rename' && (
          <FolderNameForm
            label="New name"
            initial={task.folder.name}
            submitLabel="Rename"
            onSubmit={renameFolder}
            onCancel={() => setTask(null)}
          />
        )}
      </Dialog>
      <Dialog
        open={task?.kind === 'delete' || task?.kind === 'empty'}
        onClose={() => !working && setTask(null)}
        title={
          task?.kind === 'delete'
            ? `Delete the ${word} ${task.folder.name}?`
            : task?.kind === 'empty'
              ? `Empty ${task.folder.name}?`
              : undefined
        }
        footer={
          <>
            <Button variant="ghost" disabled={working} onClick={() => setTask(null)}>
              Cancel
            </Button>
            <Button variant="danger" icon={Trash2} loading={working} onClick={() => void finishTask()}>
              {task?.kind === 'empty' ? 'Delete everything' : 'Delete'}
            </Button>
          </>
        }
      >
        {task?.kind === 'empty'
          ? 'Every message in it is deleted for good, including mail older than what Demido has loaded. This cannot be undone.'
          : account.kind === 'gmail'
            ? 'The label is taken off its messages; they stay in All Mail and under their other labels.'
            : 'On most servers the messages in it are deleted with it. This cannot be undone.'}
      </Dialog>
      <Dialog
        open={connecting !== null}
        onClose={() => setConnecting(null)}
        title={connecting === 'new' ? 'Add an email account' : 'Sign in again'}
        description={connecting && connecting !== 'new' ? connecting.email : undefined}
        width={460}
      >
        {connecting && (
          <ConnectForm
            initial={connecting === 'new' ? undefined : connecting}
            onCancel={() => setConnecting(null)}
            onDone={connected}
          />
        )}
      </Dialog>
      <Dialog
        open={naming !== null}
        onClose={() => setNaming(null)}
        title="Name this account"
        description={naming?.email}
        width={420}
      >
        {naming && (
          <NameForm
            account={naming}
            onCancel={() => setNaming(null)}
            onDone={(a) => {
              setNaming(null);
              toast.success(a.nickname ? `Named ${a.nickname}` : 'Name removed', a.email);
            }}
          />
        )}
      </Dialog>
      <Dialog
        open={removing !== null}
        onClose={() => setRemoving(null)}
        title="Remove this account?"
        description={removing?.email}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRemoving(null)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              icon={Trash2}
              onClick={() => {
                const target = removing;
                setRemoving(null);
                if (!target) return;
                useMail
                  .getState()
                  .remove(target.id)
                  .catch((e) => toast.error('Could not remove the account', errorText(e)));
              }}
            >
              Remove
            </Button>
          </>
        }
      >
        Demido forgets the password and the mail it saved on this computer. The mailbox itself does not change.
      </Dialog>
    </div>
  );
}

interface Results {
  query: string;
  range: MailDateRange | null;
  /** Null while the server is searched. */
  messages: MailSummary[] | null;
  error?: string;
}

/** What the results bar says: what was searched for, and how much was found. */
function searchLabel({ query, range, messages }: Results): string {
  const text = query ? `for “${query}”` : '';
  const dates = range ? rangeLabel(range) : '';
  if (!messages) return `Searching ${[text, dates].filter(Boolean).join(', ')}…`;
  return [`${messages.length} found ${text}`.trim(), dates].filter(Boolean).join(' · ');
}

/** Keys typed into these are text, not shortcuts. */
function typing(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName));
}

/**
 * One folder: its messages from the cache, kept up to date with the server only when the backend
 * thinks it may be behind, and the message open on the right. Messages are picked like files:
 * Ctrl-click adds one, Shift-click a run of them.
 */
function FolderView({
  account,
  folder,
  role,
  folders,
  onRefreshFolders,
  onCompose,
  onNewFolder,
}: {
  account: MailAccount;
  folder: string;
  role: string | null;
  folders: MailFolder[] | null;
  onRefreshFolders: () => void;
  onCompose: (start: MailStart, message: MailSummary) => void;
  /** Asks for a new folder's name, makes it, then hands over its path. */
  onNewFolder: (then: (path: string) => void) => void;
}) {
  const [query, setQuery] = useState('');
  const filter = useDebounced(query.trim(), 200);
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [range, setRange] = useState<MailDateRange | null>(null);
  const [picking, setPicking] = useState(false);
  const pickerAnchor = useRef<HTMLButtonElement>(null);
  const searches = useRef(0);
  const [limit, setLimit] = useState(STEP);
  const [page, setPage] = useState<MailPage | null>(null);
  const [results, setResults] = useState<Results | null>(null);
  // The messages picked; the one the keyboard is on, which shows while it is the only one; and
  // the one a Shift-click counts from.
  const [picked, setPicked] = useState<ReadonlySet<number>>(NONE);
  const [focus, setFocus] = useState<MailSummary | null>(null);
  const anchor = useRef<number | null>(null);
  // Moved or deleted, so gone from here before the server says so.
  const [hidden, setHidden] = useState<ReadonlySet<number>>(NONE);
  const [point, setPoint] = useState<Point | null>(null);
  const pointRef = useRef<HTMLSpanElement>(null);
  const moveRef = useRef<HTMLButtonElement>(null);
  const [rowMenu, setRowMenu] = useState<{ id: number; targets: MailSummary[] } | null>(null);
  // The row whose menu just closed, so pressing its ⋯ again closes the menu rather than reopening it.
  const menuClosed = useRef<{ id: number; at: number } | null>(null);
  // The latest move or delete, while its toast can still undo it.
  const lastUndo = useRef<{ toast: number; run: () => void } | null>(null);
  const [picker, setPicker] = useState<{ targets: MailSummary[]; copy: boolean; from: Opening } | null>(null);
  const [confirm, setConfirm] = useState<{ targets: MailSummary[]; permanent: boolean } | null>(null);
  const [syncs, setSyncs] = useState(0);
  const [syncError, setSyncError] = useState<string | null>(null);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const list = useRef<HTMLDivElement>(null);
  const sentinel = useRef<HTMLDivElement>(null);
  const reads = useRef(0);
  const olderBusy = useRef(false);

  const reload = useCallback(async () => {
    const n = ++reads.current;
    try {
      const next = await api.mailMessages(account.id, folder, limit, filter || null, unreadOnly, range);
      if (n === reads.current) setPage(next);
    } catch (e) {
      if (n === reads.current) setSyncError(errorText(e));
    }
  }, [account.id, folder, limit, filter, unreadOnly, range]);
  const latestReload = useRef(reload);
  latestReload.current = reload;

  useEffect(() => void reload(), [reload]);

  const sync = useCallback(
    async (force = false) => {
      setSyncs((n) => n + 1);
      try {
        const changed = await api.mailSync(account.id, folder, force);
        setSyncError(null);
        if (changed) void latestReload.current();
      } catch (e) {
        setSyncError(errorText(e));
      } finally {
        setSyncs((n) => n - 1);
      }
    },
    [account.id, folder],
  );

  // The folder is brought up to date when it opens, when the account signs in again and when
  // the app comes back to the front. The backend skips the server when it checked a moment ago,
  // or when it watches the folder (the inbox).
  const signedIn = !account.error && !account.needsPassword;
  useEffect(() => {
    if (!signedIn) return;
    void sync();
    const onFocus = () => void sync();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [sync, signedIn]);

  useEffect(() => {
    const unlisten = on('mail://changed', (e) => {
      if (e.account === account.id && e.folder === folder) void latestReload.current();
    });
    return () => void unlisten.then((f) => f());
  }, [account.id, folder]);

  const refresh = () => {
    onRefreshFolders();
    void sync(true);
  };

  // Scrolling to the end shows more of the cache, then asks the server for older mail. A
  // filtered list never asks the server: it would page through the whole mailbox.
  const more = useRef(() => {});
  more.current = () => {
    if (results || !page) return;
    if (page.messages.length >= limit) {
      setLimit((l) => l + STEP);
      return;
    }
    if (filter || unreadOnly || range || !page.loaded || page.complete || olderBusy.current) return;
    olderBusy.current = true;
    setLoadingOlder(true);
    api
      .mailLoadOlder(account.id, folder)
      .then((added) => {
        if (added > 0) void latestReload.current();
      })
      .catch((e) => setSyncError(errorText(e)))
      .finally(() => {
        olderBusy.current = false;
        setLoadingOlder(false);
      });
  };

  // Observed again when the list changes, so one still too short to scroll keeps filling. Not
  // when a load fails: offline, that would ask the server again and again.
  useEffect(() => {
    const root = list.current;
    const target = sentinel.current;
    if (!root || !target) return;
    const observer = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && more.current(), {
      root,
      rootMargin: '0px 0px 600px 0px',
    });
    observer.observe(target);
    return () => observer.disconnect();
  }, [page, results]);

  // Only the latest search shows its results.
  const runSearch = async (q: string, dates: MailDateRange | null) => {
    const n = ++searches.current;
    if (!q && !dates) return setResults(null);
    setResults({ query: q, range: dates, messages: null });
    try {
      const found = await api.mailSearch(account.id, folder, q, dates);
      if (n === searches.current) setResults({ query: q, range: dates, messages: found });
    } catch (e) {
      if (n === searches.current) setResults({ query: q, range: dates, messages: [], error: errorText(e) });
    }
  };
  const search = () => runSearch(query.trim(), range);
  const applyRange = (next: MailDateRange | null) => {
    setPicking(false);
    setRange(next);
    if (next) void runSearch(query.trim(), next);
    else void runSearch(results?.query ?? '', null);
  };
  const clearText = () => {
    setQuery('');
    void runSearch('', range);
  };
  const clearSearch = () => {
    setQuery('');
    setRange(null);
    void runSearch('', null);
  };

  // While the server is searched, the saved mail that matches shows.
  const listed = (results ? results.messages : null) ?? page?.messages;
  const messages = useMemo(
    () => (hidden.size && listed ? listed.filter((m) => !hidden.has(m.id)) : (listed ?? [])),
    [listed, hidden],
  );
  // One message picked shows, even once it leaves the list (a search ends); several show what
  // can be done with them all.
  const only = picked.size === 1 ? picked.values().next().value! : null;
  const current = only !== null ? (messages.find((m) => m.id === only) ?? (focus?.id === only ? focus : null)) : null;
  const chosen = picked.size > 1 ? messages.filter((m) => picked.has(m.id)) : current ? [current] : [];

  const pickOne = (m: MailSummary) => {
    setPicked(new Set([m.id]));
    setFocus(m);
    anchor.current = m.id;
  };
  const pick = (m: MailSummary, how: { toggle?: boolean; range?: boolean } = {}) => {
    if (how.range && anchor.current !== null) {
      const ids = rangeIds(messages, anchor.current, m.id);
      setPicked(new Set(ids.length ? ids : [m.id]));
      setFocus(m);
    } else if (how.toggle) {
      const next = new Set(picked);
      if (next.has(m.id)) next.delete(m.id);
      else next.add(m.id);
      setPicked(next);
      setFocus(m);
      anchor.current = m.id;
    } else {
      pickOne(m);
    }
  };
  const step = (delta: number, extend: boolean) => {
    if (!messages.length) return;
    const at = focus ? messages.findIndex((m) => m.id === focus.id) : -1;
    const next = messages[at < 0 ? 0 : Math.max(0, Math.min(messages.length - 1, at + delta))]!;
    if (extend) pick(next, { range: true });
    else pickOne(next);
    list.current?.querySelector(`[data-id="${next.id}"]`)?.scrollIntoView({ block: 'nearest' });
  };

  // Changes the list shows at once. The cached list follows with `mail://changed`; search
  // results are the server's, kept here.
  const patch = useCallback((changes: Map<number, Partial<MailSummary>>) => {
    const apply = (list: MailSummary[]) =>
      list.map((m) => {
        const change = changes.get(m.id);
        return change ? { ...m, ...change } : m;
      });
    setPage((p) => p && { ...p, messages: apply(p.messages) });
    setResults((r) => (r?.messages ? { ...r, messages: apply(r.messages) } : r));
    setFocus((m) => {
      const change = m && changes.get(m.id);
      return m && change ? { ...m, ...change } : m;
    });
  }, []);
  const onRead = useCallback((id: number) => patch(new Map([[id, { unread: false }]])), [patch]);

  const setFlag = async (targets: MailSummary[], flag: 'read' | 'starred', on: boolean) => {
    const read = flag === 'read';
    const todo = targets.filter((m) => (read ? m.unread === on : m.flagged !== on));
    if (!todo.length) return;
    const as = (m: MailSummary, undo: boolean): Partial<MailSummary> =>
      read ? { unread: undo ? m.unread : !on } : { flagged: undo ? m.flagged : on };
    patch(new Map(todo.map((m) => [m.id, as(m, false)])));
    try {
      const report = await api.mailSetFlag(
        todo.map((m) => m.id),
        flag,
        on,
      );
      if (report.stopped) {
        toast.warning('Only some messages were changed', report.stopped);
        void latestReload.current();
      }
    } catch (e) {
      patch(new Map(todo.map((m) => [m.id, as(m, true)])));
      toast.error(read ? 'Could not change whether it is read' : 'Could not change the star', errorText(e));
    }
  };

  /**
   * A toast for something done that `undo` takes back (a move, a delete into the trash), with an
   * Undo button, which Ctrl+Z presses too while it shows.
   */
  const offerUndo = (tone: ToastTone, title: string, body: string, undo: number, unhide: () => void) => {
    const home = nameOf(folder);
    const run = () => {
      api
        .mailUndo(undo)
        .then((report) => {
          unhide();
          if (report.stopped) toast.warning('Only some messages were put back', report.stopped);
          else if (report.missing)
            toast.warning(
              `${messageCount(report.changed)} put back in ${home}`,
              `${messageCount(report.missing)} could not be found where ${report.missing === 1 ? 'it' : 'they'} went.`,
            );
          else toast.success('Undone', `${messageCount(report.changed)} back in ${home}`);
        })
        .catch((e) => toast.error('Could not undo it', errorText(e)));
    };
    const id = useToasts.getState().push({ tone, title, body, action: { label: 'Undo', run } }, UNDO_MS);
    lastUndo.current = { toast: id, run };
  };
  /** Undoes the latest move or delete, if its toast still shows. */
  const undoLast = () => {
    const last = lastUndo.current;
    lastUndo.current = null;
    const toasts = useToasts.getState();
    if (!last || !toasts.toasts.some((t) => t.id === last.toast)) return false;
    toasts.dismiss(last.toast);
    last.run();
    return true;
  };

  /**
   * Takes messages out of the list while `run` moves or deletes them, the next one shown in their
   * place; what is taken away can then be undone.
   */
  const takeAway = async <R extends { stopped: string | null; undo: number | null }>(
    targets: MailSummary[],
    failure: string,
    run: (ids: number[]) => Promise<R>,
    done: (report: R) => string,
  ) => {
    if (!targets.length) return;
    const ids = new Set(targets.map((m) => m.id));
    if ([...picked].some((id) => ids.has(id))) {
      const next = nextAfter(messages, ids, focus?.id ?? null);
      if (next) pickOne(next);
      else {
        setPicked(NONE);
        setFocus(null);
      }
    }
    setHidden((h) => new Set([...h, ...ids]));
    const unhide = () =>
      setHidden((h) => {
        const left = new Set(h);
        for (const id of ids) left.delete(id);
        return left;
      });
    try {
      const report = await run([...ids]);
      if (report.stopped) {
        const title = `${failure}: it stopped partway`;
        if (report.undo !== null) offerUndo('warning', title, report.stopped, report.undo, unhide);
        else toast.warning(title, report.stopped);
        unhide();
        void latestReload.current();
        if (results) void runSearch(results.query, results.range);
      } else if (report.undo !== null) {
        offerUndo('success', done(report), messageCount(ids.size), report.undo, unhide);
      } else {
        toast.success(done(report), messageCount(ids.size));
      }
    } catch (e) {
      unhide();
      toast.error(failure, errorText(e));
    }
  };

  const nameOf = (path: string) => folders?.find((f) => f.path === path)?.name ?? path;

  const moveTo = (targets: MailSummary[], path: string, copy: boolean) => {
    const ids = targets.map((m) => m.id);
    if (!copy) {
      void takeAway(
        targets,
        'Could not move the mail',
        (list) => api.mailMove(list, path),
        () => `Moved to ${nameOf(path)}`,
      );
      return;
    }
    const gmail = account.kind === 'gmail';
    api
      .mailMove(ids, path, true)
      .then((report) => {
        if (report.stopped) toast.warning('Only some messages were copied', report.stopped);
        else toast.success(gmail ? `Labelled ${nameOf(path)}` : `Copied to ${nameOf(path)}`, messageCount(ids.length));
      })
      .catch((e) => toast.error(gmail ? 'Could not add the label' : 'Could not copy the mail', errorText(e)));
  };

  const remove = async (targets: MailSummary[], permanent: boolean) => {
    if (!targets.length) return;
    let forGood = permanent;
    try {
      forGood = await api.mailDeletesForGood(
        targets.map((m) => m.id),
        permanent,
      );
    } catch {
      // Not knowing, ask.
      forGood = true;
    }
    if (forGood) setConfirm({ targets, permanent });
    else void destroy(targets, permanent);
  };
  const destroy = (targets: MailSummary[], permanent: boolean) =>
    takeAway(
      targets,
      'Could not delete the mail',
      (ids) => api.mailDelete(ids, permanent),
      (report) => (report.trashed ? 'Moved to the trash' : 'Deleted for good'),
    );

  const acts: MessageActs = {
    account,
    role,
    folders,
    compose: onCompose,
    setFlag: (targets, flag, on) => void setFlag(targets, flag, on),
    archive: (targets) => {
      const into = archiveFolder(folders, account);
      if (!into) return;
      const label = account.kind === 'gmail' && role === null ? 'Label removed' : 'Archived';
      void takeAway(
        targets,
        'Could not archive the mail',
        (ids) => api.mailMove(ids, into.path),
        () => label,
      );
    },
    pickFolder: (targets, copy, from) => setPicker({ targets, copy, from }),
    remove: (targets, permanent) => void remove(targets, permanent),
    spam: (targets, on) => {
      const into = roleFolder(folders, on ? 'junk' : 'inbox');
      if (!into) return;
      void takeAway(
        targets,
        on ? 'Could not report the spam' : 'Could not move the mail',
        (ids) => api.mailMove(ids, into.path),
        () => (on ? 'Reported as spam' : 'Moved to the inbox'),
      );
    },
  };

  /**
   * Opens a row's menu at `at`. A row of the selection acts for all of it; another one acts alone,
   * and the selection stays.
   */
  const openRowMenu = (m: MailSummary, at: Point) => {
    setPoint(at);
    setRowMenu({ id: m.id, targets: picked.has(m.id) ? chosen : [m] });
  };

  // Rows keep the same handlers, so they are not drawn again when anything else changes.
  const rowHandlers = useRef({ pick, acts, openRowMenu });
  rowHandlers.current = { pick, acts, openRowMenu };
  const onRowClick = useCallback(
    (m: MailSummary, e: MouseEvent) =>
      rowHandlers.current.pick(m, { toggle: e.ctrlKey || e.metaKey, range: e.shiftKey }),
    [],
  );
  const onRowMenu = useCallback((m: MailSummary, e: MouseEvent) => {
    e.preventDefault();
    rowHandlers.current.openRowMenu(m, { x: e.clientX, y: e.clientY });
  }, []);
  // The buttons a row shows under the pointer act on that row alone; its ⋯ opens the row's menu.
  const onRowStar = useCallback((m: MailSummary) => rowHandlers.current.acts.setFlag([m], 'starred', !m.flagged), []);
  const onRowDelete = useCallback((m: MailSummary) => rowHandlers.current.acts.remove([m], false), []);
  const onRowMore = useCallback((m: MailSummary, button: HTMLElement) => {
    const closed = menuClosed.current;
    if (closed?.id === m.id && performance.now() - closed.at < 300) return;
    const r = button.getBoundingClientRect();
    rowHandlers.current.openRowMenu(m, { x: r.left, y: r.bottom + 2 });
  }, []);

  // Shortcuts, as the menus show them, for the messages picked.
  const onKeys = (e: KeyboardEvent) => {
    // Not keys meant for a menu opened from here: those bubble up from its portal.
    if (e.defaultPrevented || !e.currentTarget.contains(e.target as Node)) return;
    if (typing(e.target) || e.altKey) return;
    const mod = e.ctrlKey || e.metaKey;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      step(e.key === 'ArrowDown' ? 1 : -1, e.shiftKey);
      return;
    }
    if (mod) {
      // In the list; in the reader it selects the message's text.
      if (e.key.toLowerCase() === 'a' && e.currentTarget === list.current && messages.length) {
        e.preventDefault();
        setPicked(new Set(messages.map((m) => m.id)));
      }
      if (e.key.toLowerCase() === 'z' && !e.shiftKey && undoLast()) e.preventDefault();
      return;
    }
    // The menu of the message the keyboard is on, under its row.
    if ((e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) && focus) {
      const row = list.current?.querySelector(`[data-id="${focus.id}"]`);
      if (!row) return;
      e.preventDefault();
      const r = row.getBoundingClientRect();
      openRowMenu(focus, { x: r.left + 24, y: r.bottom });
      return;
    }
    if (e.key === 'Escape' && picked.size > 1) {
      e.preventDefault();
      if (focus) pickOne(focus);
      return;
    }
    if (!chosen.length) return;
    const o = offers(acts, chosen);
    const one = o.one;
    const run = (f: () => void) => {
      e.preventDefault();
      f();
    };
    switch (e.key === 'Delete' ? 'Delete' : e.key.toLowerCase()) {
      case 'Delete':
        return run(() => acts.remove(chosen, e.shiftKey));
      case 'r':
        if (one && !o.draft) run(() => onCompose('reply', one));
        return;
      case 'a':
        if (one && !o.draft) run(() => onCompose('replyAll', one));
        return;
      case 'f':
        if (one && !o.draft) run(() => onCompose('forward', one));
        return;
      case 'e':
        if (o.archive) run(() => acts.archive(chosen));
        return;
      case 's':
        return run(() => acts.setFlag(chosen, 'starred', o.unstarred));
      case 'u':
        return run(() => acts.setFlag(chosen, 'read', o.unread));
      case 'm':
        return run(() => acts.pickFolder(chosen, false, moveRef.current ? 'button' : 'point'));
    }
  };

  const syncing = syncs > 0;
  const count = page?.unseen ? `${page.unseen} unread` : page?.total != null ? `${page.total} messages` : '';

  return (
    <>
      <div className={styles.side}>
        <div className={styles.tools}>
          <TextField
            size="sm"
            icon={Search}
            className={styles.grow}
            placeholder="Search"
            aria-label="Search mail"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void search();
              if (e.key === 'Escape' && (query || results || range)) {
                e.stopPropagation();
                clearSearch();
              }
            }}
            trailing={
              <>
                {query && <IconButton icon={X} label="Clear" size="xs" tooltip={false} onClick={clearText} />}
                <IconButton
                  ref={pickerAnchor}
                  icon={CalendarDays}
                  label={range ? `Dates: ${rangeLabel(range)}` : 'Search by date'}
                  size="xs"
                  active={!!range || picking}
                  aria-expanded={picking}
                  onClick={() => setPicking((o) => !o)}
                />
              </>
            }
          />
          <DateFilter
            open={picking}
            onClose={() => setPicking(false)}
            anchorRef={pickerAnchor}
            value={range}
            onApply={applyRange}
          />
        </div>
        <div className={styles.filters}>
          <SegmentedControl
            size="sm"
            value={unreadOnly ? 'unread' : 'all'}
            onChange={(v) => setUnreadOnly(v === 'unread')}
            options={SHOWN}
          />
          <span className={styles.count}>{count}</span>
          <IconButton
            icon={RefreshCw}
            label="Check for new mail"
            size="sm"
            className={cx(syncing && styles.spinning)}
            disabled={syncing}
            onClick={refresh}
          />
        </div>
        {results && (
          <div className={styles.resultsBar}>
            <Search size={12} />
            <span className={styles.grow}>{searchLabel(results)}</span>
            <IconButton icon={X} label="Back to the folder" size="xs" onClick={clearSearch} />
          </div>
        )}
        {syncError && page?.loaded && !results && (
          <div className={styles.offline} title={syncError}>
            <AlertCircle size={12} />
            <span>Showing saved mail: {syncError}</span>
          </div>
        )}
        <div
          ref={list}
          className={styles.list}
          tabIndex={0}
          role="listbox"
          aria-label="Messages"
          aria-multiselectable
          onKeyDown={onKeys}
        >
          {messages.map((m) => (
            <Row
              key={m.id}
              message={m}
              role={role}
              selected={picked.has(m.id)}
              contextual={rowMenu?.id === m.id && !picked.has(m.id)}
              menuOpen={rowMenu?.id === m.id}
              onPick={onRowClick}
              onMenu={onRowMenu}
              onStar={onRowStar}
              onDelete={onRowDelete}
              onMore={onRowMore}
            />
          ))}
          <ListEnd
            page={page}
            results={results}
            empty={messages.length === 0}
            filtered={!!filter || !!range}
            unreadOnly={unreadOnly}
            syncing={syncing}
            syncError={syncError}
            loadingOlder={loadingOlder}
            onRetry={() => void sync(true)}
            onSearch={() => void search()}
          />
          <div ref={sentinel} className={styles.sentinel} />
        </div>
      </div>
      <section className={styles.reader} onKeyDown={onKeys}>
        {picked.size > 1 ? (
          <BulkPane
            acts={acts}
            targets={chosen}
            moveRef={moveRef}
            onClear={() => (focus ? pickOne(focus) : setPicked(NONE))}
          />
        ) : current ? (
          <>
            <ReaderToolbar acts={acts} message={current} moveRef={moveRef} />
            <Reader
              key={current.id}
              summary={current}
              account={account}
              onRead={onRead}
              onCompose={isDraft(current, role) ? undefined : (start) => onCompose(start, current)}
            />
          </>
        ) : (
          <EmptyState icon={Mail} title="No message selected" description="Choose a message to read it here." />
        )}
      </section>
      <PointAnchor ref={pointRef} at={point} />
      <Menu
        open={rowMenu !== null}
        onClose={() => {
          if (rowMenu) menuClosed.current = { id: rowMenu.id, at: performance.now() };
          setRowMenu(null);
        }}
        anchorRef={pointRef}
        items={rowMenu ? messageMenu(acts, rowMenu.targets, 'point') : []}
        placement="bottom-start"
        width={240}
      />
      <FolderPicker
        open={picker !== null}
        onClose={() => setPicker(null)}
        anchorRef={picker?.from === 'point' ? pointRef : moveRef}
        account={account}
        folders={folders}
        current={folder}
        copy={picker?.copy ?? false}
        onPick={(path) => picker && moveTo(picker.targets, path, picker.copy)}
        onNew={() => {
          if (!picker) return;
          const { targets, copy } = picker;
          onNewFolder((path) => moveTo(targets, path, copy));
        }}
      />
      <Dialog
        open={confirm !== null}
        onClose={() => setConfirm(null)}
        title={
          confirm
            ? `Delete ${confirm.targets.length === 1 ? 'this message' : messageCount(confirm.targets.length)} for good?`
            : undefined
        }
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button
              variant="danger"
              icon={Trash2}
              onClick={() => {
                const asked = confirm;
                setConfirm(null);
                if (asked) void destroy(asked.targets, asked.permanent);
              }}
            >
              Delete for good
            </Button>
          </>
        }
      >
        {role === 'trash'
          ? 'They are deleted from the trash, and cannot be brought back.'
          : 'They do not go to the trash: they cannot be brought back.'}
      </Dialog>
    </>
  );
}

const Row = memo(function Row({
  message: m,
  role,
  selected,
  contextual,
  menuOpen,
  onPick,
  onMenu,
  onStar,
  onDelete,
  onMore,
}: {
  message: MailSummary;
  role: string | null;
  selected: boolean;
  /** Its context menu is open, though it is not picked. */
  contextual: boolean;
  /** Its menu is open: its buttons stay. */
  menuOpen: boolean;
  onPick: (message: MailSummary, e: MouseEvent) => void;
  onMenu: (message: MailSummary, e: MouseEvent) => void;
  onStar: (message: MailSummary) => void;
  onDelete: (message: MailSummary) => void;
  onMore: (message: MailSummary, button: HTMLElement) => void;
}) {
  return (
    <div
      role="option"
      aria-selected={selected}
      data-id={m.id}
      className={cx(
        styles.row,
        selected && styles.selected,
        contextual && styles.contextual,
        m.unread && styles.unread,
      )}
      // Shift-click picks a run of messages, not the text between.
      onMouseDown={(e) => e.shiftKey && e.preventDefault()}
      onClick={(e) => onPick(m, e)}
      onContextMenu={(e) => onMenu(m, e)}
    >
      <div className={styles.rowTop}>
        {m.unread && <span className={styles.dot} aria-label="Unread" />}
        <span className={styles.people}>{rowPeople(m, role)}</span>
        {m.attachments > 0 && <Paperclip size={12} className={styles.rowIcon} aria-label="Attachments" />}
        {m.flagged && <Star size={12} className={styles.star} aria-label="Starred" />}
        <span className={styles.date}>{listDate(m.date)}</span>
      </div>
      <div className={styles.subject}>{m.subject || '(no subject)'}</div>
      {m.snippet && <div className={styles.snippet}>{m.snippet}</div>}
      {/* Under the pointer. Pressing them leaves the focus on the list, for its shortcuts. */}
      <div
        className={cx(styles.rowActions, menuOpen && styles.shown)}
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => e.stopPropagation()}
      >
        <IconButton
          icon={Star}
          label={m.flagged ? 'Remove star' : 'Star'}
          size="xs"
          tabIndex={-1}
          className={cx(m.flagged && styles.starred)}
          onClick={() => onStar(m)}
        />
        <IconButton
          icon={Trash2}
          label={role === 'trash' ? 'Delete for good' : 'Delete'}
          size="xs"
          tabIndex={-1}
          onClick={() => onDelete(m)}
        />
        <IconButton
          icon={MoreHorizontal}
          label="More actions"
          size="xs"
          tabIndex={-1}
          active={menuOpen}
          onClick={(e) => onMore(m, e.currentTarget)}
        />
      </div>
    </div>
  );
});

/** What the end of the list says: why it is empty, or that more is on the way. */
function ListEnd({
  page,
  results,
  empty,
  filtered,
  unreadOnly,
  syncing,
  syncError,
  loadingOlder,
  onRetry,
  onSearch,
}: {
  page: MailPage | null;
  results: Results | null;
  empty: boolean;
  filtered: boolean;
  unreadOnly: boolean;
  syncing: boolean;
  syncError: string | null;
  loadingOlder: boolean;
  onRetry: () => void;
  onSearch: () => void;
}) {
  if (!page || (results && !results.messages) || (empty && !page.loaded && syncing) || loadingOlder) {
    return (
      <div className={styles.listNote}>
        <Spinner size={14} />
        {loadingOlder ? 'Loading older mail…' : results ? 'Searching the server…' : empty ? 'Loading your mail…' : null}
      </div>
    );
  }
  if (results) {
    if (results.error)
      return <EmptyState compact icon={AlertCircle} title="The search failed" description={results.error} />;
    return empty ? (
      <EmptyState compact icon={Search} title="Nothing found" description="No message in this folder matches." />
    ) : null;
  }
  const searchServer = (
    <Button size="sm" variant="secondary" icon={Search} onClick={onSearch}>
      Search the server
    </Button>
  );
  if (empty) {
    if (filtered) {
      return (
        <EmptyState
          compact
          icon={Search}
          title="No match in the saved mail"
          description="Older mail may match: press Enter to search the whole folder."
          action={searchServer}
        />
      );
    }
    if (!page.loaded && syncError) {
      return (
        <EmptyState
          compact
          icon={AlertCircle}
          title="Could not load this folder"
          description={syncError}
          action={
            <Button size="sm" variant="secondary" icon={RefreshCw} onClick={onRetry}>
              Try again
            </Button>
          }
        />
      );
    }
    return <EmptyState compact icon={Mail} title={unreadOnly ? 'No unread mail' : 'No messages'} />;
  }
  if (filtered) return <div className={styles.listNote}>Press Enter to search older mail on the server.</div>;
  return null;
}

/**
 * The open message. Its body is fetched once, then kept by the backend and in memory. Once it
 * shows, an unread message is marked read on the server, unless that is switched off.
 */
function Reader({
  summary,
  account,
  onRead,
  onCompose,
}: {
  summary: MailSummary;
  account: MailAccount;
  onRead: (id: number) => void;
  /** Answers or forwards it; not offered for drafts. */
  onCompose?: (start: MailStart) => void;
}) {
  const [message, setMessage] = useState<MailMessage | null>(() => opened.get(summary.id) ?? null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const alwaysImages = useApp((s) => s.settings?.mailShowImages ?? false);
  const patchSettings = useApp((s) => s.patchSettings);
  const [imagesOnce, setShowImages] = useState(false);
  const showImages = alwaysImages || imagesOnce;
  const [remote, setRemote] = useState(false);
  const markRead = useApp((s) => s.settings?.mailMarkRead ?? true);
  const marked = useRef(false);
  const [link, setLink] = useState<string | null>(null);

  const follow = useCallback((url: string) => (asksFirst(url) ? setLink(url) : openLink(url)), []);
  useEffect(() => {
    const unlisten = on('app://new-window', follow);
    return () => void unlisten.then((f) => f());
  }, [follow]);

  useEffect(() => {
    const cached = opened.get(summary.id);
    if (cached) return setMessage(cached);
    let live = true;
    setError(null);
    api
      .mailOpen(summary.id)
      .then((m) => {
        opened.set(summary.id, m);
        if (live) setMessage(m);
      })
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [summary.id, attempt]);

  const shown = message !== null;
  useEffect(() => {
    if (!shown || !markRead || !summary.unread || marked.current) return;
    marked.current = true;
    api
      .mailMarkRead([summary.id])
      .then(() => onRead(summary.id))
      .catch((e) => {
        marked.current = false;
        toast.error('Could not mark the message as read', errorText(e));
      });
  }, [shown, markRead, summary.id, summary.unread, onRead]);

  const s = message?.summary ?? summary;
  const gmail =
    account.kind === 'gmail' && s.messageId
      ? `https://mail.google.com/mail/?authuser=${encodeURIComponent(account.email)}#search/rfc822msgid%3A${encodeURIComponent(s.messageId)}`
      : null;

  return (
    <div className={styles.readerScroll}>
      <header className={styles.readerHead}>
        <div className={styles.subjectRow}>
          <h2 className={styles.readerSubject}>{s.subject || '(no subject)'}</h2>
          {gmail && (
            <IconButton icon={ExternalLink} label="Open in Gmail" size="sm" onClick={() => void api.openUrl(gmail)} />
          )}
        </div>
        <div className={styles.fromRow}>
          <Avatar name={personName(s.from)} size={32} />
          <div className={styles.fromText}>
            <div className={styles.fromLine}>
              <span className={styles.fromName}>{personName(s.from)}</span>
              {s.from.name && s.from.email && <span className={styles.fromEmail}>{s.from.email}</span>}
              <span className={styles.readerDate}>{s.date ? formatDateTime(s.date) : ''}</span>
            </div>
            <People label="To" people={s.to} />
            <People label="Cc" people={s.cc} />
          </div>
        </div>
        {message && message.attachments.length > 0 && <Attachments message={message} />}
      </header>
      {remote && !showImages && (
        <Notice
          tone="info"
          icon={ImageOff}
          className={styles.imagesNotice}
          action={
            <>
              <Button size="sm" variant="secondary" onClick={() => setShowImages(true)}>
                Show images
              </Button>
              <Button size="sm" variant="ghost" onClick={() => void patchSettings({ mailShowImages: true })}>
                Always show
              </Button>
            </>
          }
        >
          Images from the internet are hidden: loading them can tell the sender you read this.
        </Notice>
      )}
      <div className={styles.readerBody}>
        {error ? (
          <Notice
            tone="danger"
            className={styles.bodyNotice}
            action={
              <Button size="sm" variant="secondary" onClick={() => setAttempt((a) => a + 1)}>
                Try again
              </Button>
            }
          >
            {error}
          </Notice>
        ) : !message ? (
          <div className={styles.bodyLoading}>
            <Spinner />
          </div>
        ) : message.html ? (
          <HtmlBody html={message.html} showImages={showImages} onRemote={setRemote} />
        ) : (
          <TextBody text={message.text || '(This message has no text.)'} onLink={follow} />
        )}
      </div>
      {message && onCompose && (
        <div className={styles.replyRow}>
          <Button size="sm" icon={Reply} onClick={() => onCompose('reply')}>
            Reply
          </Button>
          <Button size="sm" icon={ReplyAll} onClick={() => onCompose('replyAll')}>
            Reply all
          </Button>
          <Button size="sm" icon={Forward} onClick={() => onCompose('forward')}>
            Forward
          </Button>
        </div>
      )}
      <LinkDialog url={link} onClose={() => setLink(null)} />
    </div>
  );
}

function People({ label, people }: { label: string; people: MailSummary['to'] }) {
  if (!people.length) return null;
  const text = people.map(personLabel).join(', ');
  return (
    <div className={styles.recipients} title={text}>
      {label}: {text}
    </div>
  );
}

function Attachments({ message }: { message: MailMessage }) {
  const [saving, setSaving] = useState<string | null>(null);
  const saveOne = async (a: MailAttachment) => {
    const path = await save({ title: 'Save attachment', defaultPath: a.name });
    if (!path) return;
    setSaving(a.section);
    try {
      await api.mailSaveAttachment(message.summary.id, a.section, path);
      toast.success('Attachment saved', a.name);
    } catch (e) {
      toast.error('Could not save the attachment', errorText(e));
    } finally {
      setSaving(null);
    }
  };
  return (
    <div className={styles.attachments}>
      {message.attachments.map((a) => (
        <button
          key={a.section}
          type="button"
          className={styles.attachment}
          title={`Save ${a.name}`}
          disabled={saving === a.section}
          onClick={() => void saveOne(a)}
        >
          {saving === a.section ? <Spinner size={13} /> : <FileText size={14} />}
          <span className={styles.attachmentName}>{a.name}</span>
          <span className={styles.attachmentSize}>{formatBytes(a.size)}</span>
          <Download size={12} className={styles.attachmentSave} />
        </button>
      ))}
    </div>
  );
}
