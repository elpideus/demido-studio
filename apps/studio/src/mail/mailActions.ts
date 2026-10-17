// What the Mail window offers to do with messages and folders, and how a selection moves.

import type { MailAccount, MailFolder, MailSummary } from '@/lib/types';

/** Folders the mail system keeps, which are neither renamed nor deleted (as the backend's `system_folder`). */
export function isSystemFolder(folder: Pick<MailFolder, 'path' | 'role'>): boolean {
  return folder.role !== null || folder.path.toUpperCase() === 'INBOX';
}

/** What the account calls its folders: Gmail has labels. */
export function folderWord(account: Pick<MailAccount, 'kind'>): 'label' | 'folder' {
  return account.kind === 'gmail' ? 'label' : 'folder';
}

export function roleFolder(folders: MailFolder[] | null, role: string): MailFolder | null {
  return folders?.find((f) => f.role === role) ?? null;
}

/** Where Archive sends mail: the archive folder, or All Mail on Gmail. Null when there is none. */
export function archiveFolder(folders: MailFolder[] | null, account: Pick<MailAccount, 'kind'>): MailFolder | null {
  return roleFolder(folders, 'archive') ?? (account.kind === 'gmail' ? roleFolder(folders, 'all') : null);
}

/** Folders whose mail can be archived from: the inbox and the person's own. */
export function archivesFrom(role: string | null): boolean {
  return role === null || role === 'inbox';
}

/** Gmail's views of mail that is somewhere else; nothing is moved into them. */
const VIEWS = new Set(['flagged', 'important']);

/**
 * Folders messages in `current` can be moved to, or with `copy` copied to (labelled with, on
 * Gmail): never the one they are in, nor Drafts, and copies only go to the inbox and the person's
 * own folders.
 */
export function moveTargets(folders: MailFolder[] | null, current: string, copy: boolean): MailFolder[] {
  return (folders ?? []).filter((f) => {
    if (!f.selectable || f.path === current) return false;
    if (f.role === 'drafts' || (f.role && VIEWS.has(f.role))) return false;
    return !copy || f.role === null || f.role === 'inbox';
  });
}

/** Each folder's name with the folders it is in, like "Work / Clients", from the list's tree order. */
export function folderTrails(folders: MailFolder[]): Map<string, string> {
  const trail = new Map<string, string>();
  const stack: string[] = [];
  for (const f of folders) {
    stack.length = f.depth;
    stack[f.depth] = f.name;
    trail.set(f.path, stack.filter(Boolean).join(' / '));
  }
  return trail;
}

/** The ids from one message to another, both included, in list order: what a Shift-click picks. */
export function rangeIds(messages: MailSummary[], from: number, to: number): number[] {
  const a = messages.findIndex((m) => m.id === from);
  const b = messages.findIndex((m) => m.id === to);
  if (b < 0) return [];
  if (a < 0) return [to];
  const [lo, hi] = a < b ? [a, b] : [b, a];
  return messages.slice(lo, hi + 1).map((m) => m.id);
}

/**
 * The message to show once `gone` leave the list: the first after the one shown that stays, else
 * the nearest before it.
 */
export function nextAfter(
  messages: MailSummary[],
  gone: ReadonlySet<number>,
  shown: number | null,
): MailSummary | null {
  let at = shown === null ? -1 : messages.findIndex((m) => m.id === shown);
  if (at < 0) at = messages.findIndex((m) => gone.has(m.id));
  if (at < 0) return null;
  for (let i = at; i < messages.length; i++) if (!gone.has(messages[i]!.id)) return messages[i]!;
  for (let i = at - 1; i >= 0; i--) if (!gone.has(messages[i]!.id)) return messages[i]!;
  return null;
}

/** "1 message", "3 messages". */
export function messageCount(n: number): string {
  return n === 1 ? '1 message' : `${n.toLocaleString()} messages`;
}

/** Whether a message is one the person is still writing. */
export function isDraft(m: Pick<MailSummary, 'draft'>, role: string | null): boolean {
  return m.draft || role === 'drafts';
}

/** The most a message's own attachments may weigh together (`MAX_ATTACHMENTS` in the backend). */
export const MAX_ATTACHMENTS = 25 * 1024 * 1024;

/** Addresses written in one field, split at the commas and semicolons between them (not inside quotes). */
export function splitAddresses(text: string): string[] {
  const out: string[] = [];
  let current = '';
  let quoted = false;
  for (const c of text) {
    if (c === '"') quoted = !quoted;
    if ((c === ',' || c === ';') && !quoted) {
      out.push(current);
      current = '';
    } else {
      current += c;
    }
  }
  out.push(current);
  return out.map((a) => a.trim()).filter(Boolean);
}
