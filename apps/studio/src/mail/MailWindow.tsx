import { type KeyboardEvent, memo, useCallback, useEffect, useRef, useState } from 'react';
import {
  AlertCircle,
  CalendarDays,
  Download,
  ExternalLink,
  FileText,
  Image as ImageIcon,
  ImageOff,
  KeyRound,
  Mail,
  MoreHorizontal,
  Paperclip,
  Plus,
  RefreshCw,
  Search,
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
import { openUrl } from '@tauri-apps/plugin-opener';

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
  MailSummary,
} from '@/lib/types';
import { useMail } from '@/stores/mail';
import { useApp } from '@/stores/app';
import { toast } from '@/stores/toasts';
import { type WindowState, useWindows } from '@/stores/windows';
import { ConnectForm } from './ConnectForm';
import { DateFilter } from './DateFilter';
import { rangeLabel } from './dateRange';
import { HtmlBody, TextBody } from './EmailBody';
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

/** Email browsing: the folder's messages on the left, the open message on the right. */
export function MailWindow({ win }: { win: WindowState }) {
  const accounts = useMail((s) => s.accounts);

  // While the window is open the inboxes are watched, so new mail shows without asking.
  useEffect(() => {
    void api.mailSetWatching(true);
    return () => void api.mailSetWatching(false);
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
  const shownAccount = useRef(account.id);
  shownAccount.current = account.id;

  // The folder list comes from the cache; the backend reads it from the server again when it is
  // a minute old.
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
  const role = folders?.find((f) => f.path === folder)?.role ?? (folder === 'INBOX' ? 'inbox' : null);
  const options = folders ? folderOptions(folders) : [];

  const menu: MenuEntry[] = [
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
              ref={menuAnchor}
              icon={MoreHorizontal}
              label="Accounts"
              size="sm"
              active={menuOpen}
              onClick={() => setMenuOpen((o) => !o)}
            />
            <Menu open={menuOpen} onClose={() => setMenuOpen(false)} anchorRef={menuAnchor} items={menu} width={240} />
          </div>
          {options.length > 0 && (
            <Select
              size="sm"
              aria-label="Folder"
              options={options}
              value={folder}
              onChange={(e) => setProps(win.id, { folder: e.target.value })}
            />
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
          onRefreshFolders={() => void loadFolders(account.id, true)}
        />
      </div>
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

/**
 * One folder: its messages from the cache, kept up to date with the server only when the backend
 * thinks it may be behind, and the message open on the right.
 */
function FolderView({
  account,
  folder,
  role,
  onRefreshFolders,
}: {
  account: MailAccount;
  folder: string;
  role: string | null;
  onRefreshFolders: () => void;
}) {
  const [query, setQuery] = useState('');
  const filter = useDebounced(query.trim(), 200);
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [range, setRange] = useState<MailDateRange | null>(null);
  const [picking, setPicking] = useState(false);
  const pickerAnchor = useRef<HTMLButtonElement>(null);
  const searches = useRef(0);
  const alwaysImages = useApp((s) => s.settings?.mailShowImages ?? false);
  const patchSettings = useApp((s) => s.patchSettings);
  const [limit, setLimit] = useState(STEP);
  const [page, setPage] = useState<MailPage | null>(null);
  const [results, setResults] = useState<Results | null>(null);
  const [selected, setSelected] = useState<MailSummary | null>(null);
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
  // or when it watches the folder (the inbox, while this window is open).
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
  const messages = (results ? results.messages : null) ?? page?.messages ?? [];
  const current = selected ? (messages.find((m) => m.id === selected.id) ?? selected) : null;

  const move = (delta: number) => {
    if (!messages.length) return;
    const at = current ? messages.findIndex((m) => m.id === current.id) : -1;
    const next = messages[at < 0 ? 0 : Math.max(0, Math.min(messages.length - 1, at + delta))]!;
    setSelected(next);
    list.current?.querySelector(`[data-id="${next.id}"]`)?.scrollIntoView({ block: 'nearest' });
  };
  const onListKey = (e: KeyboardEvent) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      move(e.key === 'ArrowDown' ? 1 : -1);
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
          <IconButton
            icon={alwaysImages ? ImageIcon : ImageOff}
            label="Load images automatically"
            size="sm"
            active={alwaysImages}
            aria-pressed={alwaysImages}
            onClick={() => void patchSettings({ mailShowImages: !alwaysImages })}
          />
          <IconButton
            icon={RefreshCw}
            label="Check for new mail"
            size="sm"
            className={cx(syncing && styles.spinning)}
            disabled={syncing}
            onClick={refresh}
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
        <div ref={list} className={styles.list} tabIndex={0} role="listbox" aria-label="Messages" onKeyDown={onListKey}>
          {messages.map((m) => (
            <Row key={m.id} message={m} role={role} selected={m.id === current?.id} onSelect={setSelected} />
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
      <section className={styles.reader}>
        {current ? (
          <Reader key={current.id} summary={current} account={account} />
        ) : (
          <EmptyState icon={Mail} title="No message selected" description="Choose a message to read it here." />
        )}
      </section>
    </>
  );
}

const Row = memo(function Row({
  message: m,
  role,
  selected,
  onSelect,
}: {
  message: MailSummary;
  role: string | null;
  selected: boolean;
  onSelect: (message: MailSummary) => void;
}) {
  return (
    <div
      role="option"
      aria-selected={selected}
      data-id={m.id}
      className={cx(styles.row, selected && styles.selected, m.unread && styles.unread)}
      onClick={() => onSelect(m)}
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

/** The open message. Its body is fetched once, then kept by the backend and in memory. */
function Reader({ summary, account }: { summary: MailSummary; account: MailAccount }) {
  const [message, setMessage] = useState<MailMessage | null>(() => opened.get(summary.id) ?? null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const alwaysImages = useApp((s) => s.settings?.mailShowImages ?? false);
  const patchSettings = useApp((s) => s.patchSettings);
  const [imagesOnce, setShowImages] = useState(false);
  const showImages = alwaysImages || imagesOnce;
  const [remote, setRemote] = useState(false);

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
            <IconButton icon={ExternalLink} label="Open in Gmail" size="sm" onClick={() => void openUrl(gmail)} />
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
          <TextBody text={message.text || '(This message has no text.)'} />
        )}
      </div>
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
