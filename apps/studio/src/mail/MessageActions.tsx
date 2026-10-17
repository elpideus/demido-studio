import { type RefObject, forwardRef, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import {
  Archive,
  FilePen,
  Folder,
  FolderInput,
  FolderPlus,
  Forward,
  Inbox,
  type LucideIcon,
  Mail,
  MailOpen,
  Mails,
  MoreHorizontal,
  Reply,
  ReplyAll,
  Send,
  ShieldAlert,
  ShieldCheck,
  Star,
  StarOff,
  Tag,
  Trash2,
  X,
} from 'lucide-react';
import { Button, IconButton, Menu, type MenuEntry, Popover, SearchList, cx } from '@demido/ui';

import type { MailAccount, MailFolder, MailStart, MailSummary } from '@/lib/types';
import {
  archiveFolder,
  archivesFrom,
  folderTrails,
  folderWord,
  isDraft,
  messageCount,
  moveTargets,
  roleFolder,
} from './mailActions';
import styles from './MailWindow.module.css';

export interface Point {
  x: number;
  y: number;
}

/** Where a menu or picker opens: at the pointer (a context menu) or under a button. */
export type Opening = 'point' | 'button';

/** What can be done with the messages of one folder, carried out by the folder's view. */
export interface MessageActs {
  account: MailAccount;
  /** The role of the folder the messages are in. */
  role: string | null;
  folders: MailFolder[] | null;
  compose: (start: MailStart, message: MailSummary) => void;
  setFlag: (targets: MailSummary[], flag: 'read' | 'starred', on: boolean) => void;
  archive: (targets: MailSummary[]) => void;
  /** Opens the folder picker, to move them or (`copy`) copy or label them. */
  pickFolder: (targets: MailSummary[], copy: boolean, from: Opening) => void;
  remove: (targets: MailSummary[], permanent: boolean) => void;
  /** Reports them as spam, or (`false`) takes them out of the spam folder. */
  spam: (targets: MailSummary[], on: boolean) => void;
}

/** Which actions fit these messages where they are. */
export function offers(acts: MessageActs, targets: MailSummary[]) {
  const { account, role, folders } = acts;
  const one = targets.length === 1 ? targets[0]! : null;
  const gmail = account.kind === 'gmail';
  return {
    one,
    draft: one !== null && isDraft(one, role),
    archive: archivesFrom(role) && archiveFolder(folders, account) !== null,
    /** On Gmail, archiving from a label takes the label off. */
    archiveLabel: gmail && role === null ? 'Remove label' : 'Archive',
    copyLabel: gmail ? 'Add label…' : 'Copy to…',
    spam: role !== 'junk' && role !== 'trash' && role !== 'drafts' && roleFolder(folders, 'junk') !== null,
    notSpam: role === 'junk' && roleFolder(folders, 'inbox') !== null,
    /** Deleting already deletes for good in the trash, and where there is none. */
    forGood: role !== 'trash' && roleFolder(folders, 'trash') !== null,
    unread: targets.some((m) => m.unread),
    unstarred: targets.some((m) => !m.flagged),
  };
}

/** The menu for messages: a row's context menu. */
export function messageMenu(acts: MessageActs, targets: MailSummary[], from: Opening): MenuEntry[] {
  const o = offers(acts, targets);
  const items: MenuEntry[] = [];
  if (o.one && o.draft) {
    const draft = o.one;
    items.push({ id: 'edit', label: 'Edit draft', icon: FilePen, onSelect: () => acts.compose('draft', draft) });
    items.push('separator');
  } else if (o.one) {
    const m = o.one;
    items.push(
      { id: 'reply', label: 'Reply', icon: Reply, shortcut: 'R', onSelect: () => acts.compose('reply', m) },
      {
        id: 'replyAll',
        label: 'Reply all',
        icon: ReplyAll,
        shortcut: 'A',
        onSelect: () => acts.compose('replyAll', m),
      },
      { id: 'forward', label: 'Forward', icon: Forward, shortcut: 'F', onSelect: () => acts.compose('forward', m) },
      'separator',
    );
  }
  items.push(
    o.unread
      ? {
          id: 'read',
          label: 'Mark as read',
          icon: MailOpen,
          shortcut: 'U',
          onSelect: () => acts.setFlag(targets, 'read', true),
        }
      : {
          id: 'unread',
          label: 'Mark as unread',
          icon: Mail,
          shortcut: 'U',
          onSelect: () => acts.setFlag(targets, 'read', false),
        },
    o.unstarred
      ? { id: 'star', label: 'Star', icon: Star, shortcut: 'S', onSelect: () => acts.setFlag(targets, 'starred', true) }
      : {
          id: 'unstar',
          label: 'Remove star',
          icon: StarOff,
          shortcut: 'S',
          onSelect: () => acts.setFlag(targets, 'starred', false),
        },
    'separator',
  );
  if (o.archive) {
    items.push({
      id: 'archive',
      label: o.archiveLabel,
      icon: Archive,
      shortcut: 'E',
      onSelect: () => acts.archive(targets),
    });
  }
  items.push(
    {
      id: 'move',
      label: 'Move to…',
      icon: FolderInput,
      shortcut: 'M',
      onSelect: () => acts.pickFolder(targets, false, from),
    },
    { id: 'copy', label: o.copyLabel, icon: Tag, onSelect: () => acts.pickFolder(targets, true, from) },
  );
  if (o.spam)
    items.push({ id: 'spam', label: 'Report spam', icon: ShieldAlert, onSelect: () => acts.spam(targets, true) });
  if (o.notSpam)
    items.push({ id: 'notSpam', label: 'Not spam', icon: ShieldCheck, onSelect: () => acts.spam(targets, false) });
  items.push('separator', {
    id: 'delete',
    label: 'Delete',
    icon: Trash2,
    shortcut: 'Del',
    danger: true,
    onSelect: () => acts.remove(targets, false),
  });
  if (o.forGood) {
    items.push({
      id: 'destroy',
      label: 'Delete for good…',
      icon: Trash2,
      shortcut: 'Shift+Del',
      danger: true,
      onSelect: () => acts.remove(targets, true),
    });
  }
  return items;
}

/** A spot at the pointer for a context menu, and the folder picker opened from it, to open at. */
export const PointAnchor = forwardRef<HTMLSpanElement, { at: Point | null }>(function PointAnchor({ at }, ref) {
  if (!at) return null;
  return createPortal(
    <span ref={ref} className={styles.pointAnchor} style={{ left: at.x, top: at.y }} />,
    document.body,
  );
});

/** The buttons over the open message. */
export function ReaderToolbar({
  acts,
  message: m,
  moveRef,
}: {
  acts: MessageActs;
  message: MailSummary;
  moveRef: RefObject<HTMLButtonElement | null>;
}) {
  const o = offers(acts, [m]);
  const [more, setMore] = useState(false);
  const moreRef = useRef<HTMLButtonElement>(null);
  const targets = [m];
  const moreItems: MenuEntry[] = [
    { id: 'copy', label: o.copyLabel, icon: Tag, onSelect: () => acts.pickFolder(targets, true, 'button') },
  ];
  if (o.spam)
    moreItems.push({ id: 'spam', label: 'Report spam', icon: ShieldAlert, onSelect: () => acts.spam(targets, true) });
  if (o.forGood) {
    moreItems.push('separator', {
      id: 'destroy',
      label: 'Delete for good…',
      icon: Trash2,
      shortcut: 'Shift+Del',
      danger: true,
      onSelect: () => acts.remove(targets, true),
    });
  }
  return (
    <div className={styles.readerBar} role="toolbar" aria-label="Message actions">
      {o.draft ? (
        <Button size="sm" variant="primary" icon={FilePen} onClick={() => acts.compose('draft', m)}>
          Edit draft
        </Button>
      ) : (
        <>
          <IconButton
            icon={Reply}
            label="Reply"
            size="sm"
            aria-keyshortcuts="R"
            onClick={() => acts.compose('reply', m)}
          />
          <IconButton
            icon={ReplyAll}
            label="Reply all"
            size="sm"
            aria-keyshortcuts="A"
            onClick={() => acts.compose('replyAll', m)}
          />
          <IconButton
            icon={Forward}
            label="Forward"
            size="sm"
            aria-keyshortcuts="F"
            onClick={() => acts.compose('forward', m)}
          />
        </>
      )}
      <span className={styles.barSep} />
      {o.archive && (
        <IconButton
          icon={Archive}
          label={o.archiveLabel}
          size="sm"
          aria-keyshortcuts="E"
          onClick={() => acts.archive(targets)}
        />
      )}
      <IconButton
        ref={moveRef}
        icon={FolderInput}
        label="Move to…"
        size="sm"
        aria-keyshortcuts="M"
        onClick={() => acts.pickFolder(targets, false, 'button')}
      />
      {o.notSpam && (
        <IconButton icon={ShieldCheck} label="Not spam" size="sm" onClick={() => acts.spam(targets, false)} />
      )}
      <IconButton
        icon={Trash2}
        label="Delete"
        size="sm"
        aria-keyshortcuts="Delete"
        onClick={() => acts.remove(targets, false)}
      />
      <span className={styles.barSep} />
      <IconButton
        icon={m.unread ? MailOpen : Mail}
        label={m.unread ? 'Mark as read' : 'Mark as unread'}
        size="sm"
        aria-keyshortcuts="U"
        onClick={() => acts.setFlag(targets, 'read', m.unread)}
      />
      <IconButton
        icon={Star}
        label={m.flagged ? 'Remove star' : 'Star'}
        size="sm"
        aria-keyshortcuts="S"
        aria-pressed={m.flagged}
        className={cx(m.flagged && styles.starred)}
        onClick={() => acts.setFlag(targets, 'starred', !m.flagged)}
      />
      <span className={styles.grow} />
      <IconButton
        ref={moreRef}
        icon={MoreHorizontal}
        label="More"
        size="sm"
        active={more}
        onClick={() => setMore((v) => !v)}
      />
      <Menu open={more} onClose={() => setMore(false)} anchorRef={moreRef} items={moreItems} width={220} />
    </div>
  );
}

/** What shows in place of a message when several are picked: what can be done with them all. */
export function BulkPane({
  acts,
  targets,
  moveRef,
  onClear,
}: {
  acts: MessageActs;
  targets: MailSummary[];
  moveRef: RefObject<HTMLButtonElement | null>;
  onClear: () => void;
}) {
  const o = offers(acts, targets);
  return (
    <div className={styles.bulk}>
      <Mails size={28} className={styles.bulkIcon} />
      <h2 className={styles.bulkTitle}>{messageCount(targets.length)} selected</h2>
      <div className={styles.bulkActions}>
        <Button size="sm" icon={o.unread ? MailOpen : Mail} onClick={() => acts.setFlag(targets, 'read', o.unread)}>
          {o.unread ? 'Mark as read' : 'Mark as unread'}
        </Button>
        <Button
          size="sm"
          icon={o.unstarred ? Star : StarOff}
          onClick={() => acts.setFlag(targets, 'starred', o.unstarred)}
        >
          {o.unstarred ? 'Star' : 'Remove star'}
        </Button>
        {o.archive && (
          <Button size="sm" icon={Archive} onClick={() => acts.archive(targets)}>
            {o.archiveLabel}
          </Button>
        )}
        <Button ref={moveRef} size="sm" icon={FolderInput} onClick={() => acts.pickFolder(targets, false, 'button')}>
          Move to…
        </Button>
        {o.spam && (
          <Button size="sm" icon={ShieldAlert} onClick={() => acts.spam(targets, true)}>
            Report spam
          </Button>
        )}
        {o.notSpam && (
          <Button size="sm" icon={ShieldCheck} onClick={() => acts.spam(targets, false)}>
            Not spam
          </Button>
        )}
        <Button size="sm" variant="danger" icon={Trash2} onClick={() => acts.remove(targets, false)}>
          Delete
        </Button>
      </div>
      <Button size="sm" variant="ghost" icon={X} onClick={onClear}>
        Clear selection
      </Button>
    </div>
  );
}

/** The icon a folder shows by what it is for. */
export function folderIcon(folder: Pick<MailFolder, 'role'>, gmail: boolean): LucideIcon {
  switch (folder.role) {
    case 'inbox':
      return Inbox;
    case 'sent':
      return Send;
    case 'drafts':
      return FilePen;
    case 'archive':
    case 'all':
      return Archive;
    case 'junk':
      return ShieldAlert;
    case 'trash':
      return Trash2;
    case 'flagged':
      return Star;
    default:
      return gmail ? Tag : Folder;
  }
}

/** A searchable list of the folders messages can go to, with a way to make a new one. */
export function FolderPicker({
  open,
  onClose,
  anchorRef,
  account,
  folders,
  current,
  copy,
  onPick,
  onNew,
}: {
  open: boolean;
  onClose: () => void;
  anchorRef: RefObject<HTMLElement | null>;
  account: MailAccount;
  folders: MailFolder[] | null;
  /** The folder the messages are in. */
  current: string;
  copy: boolean;
  onPick: (path: string) => void;
  onNew: () => void;
}) {
  const gmail = account.kind === 'gmail';
  const trails = folderTrails(folders ?? []);
  const items = moveTargets(folders, current, copy).map((f) => {
    const Icon = folderIcon(f, gmail);
    return {
      id: f.path,
      label: trails.get(f.path) ?? f.name,
      leading: <Icon size={15} strokeWidth={1.8} />,
    };
  });
  const word = folderWord(account);
  return (
    <Popover
      open={open}
      onClose={onClose}
      anchorRef={anchorRef}
      width={300}
      placement="bottom-start"
      aria-label={copy ? (gmail ? 'Add a label' : 'Copy to a folder') : 'Move to a folder'}
    >
      <SearchList
        items={items}
        placeholder={copy ? (gmail ? 'Label as…' : 'Copy to…') : 'Move to…'}
        emptyText={`No ${word} matches.`}
        onSelect={(item) => {
          onClose();
          onPick(item.id);
        }}
        footer={{
          icon: FolderPlus,
          label: `New ${word}…`,
          onSelect: () => {
            onClose();
            onNew();
          },
        }}
      />
    </Popover>
  );
}
