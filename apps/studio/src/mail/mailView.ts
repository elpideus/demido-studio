// How the Mail window shows lists, dates and people.

import type { MailAccount, MailAddress, MailFolder, MailSummary } from '@/lib/types';

/** Folders whose mail was written by the account, so the list shows who it went to. */
const OUTGOING = new Set(['sent', 'drafts']);

/** A date for the message list: the time today, the day this year, the full date before. */
export function listDate(ms: number, now = new Date()): string {
  if (!ms) return '';
  const date = new Date(ms);
  if (date.toDateString() === now.toDateString()) {
    return date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }
  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }
  return date.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

export function personName(address: MailAddress): string {
  return address.name || address.email || '(unknown sender)';
}

/** A name and address the way a header shows them. */
export function personLabel(address: MailAddress): string {
  if (!address.name) return address.email;
  return address.email ? `${address.name} <${address.email}>` : address.name;
}

/** An account as the account picker shows it: its name first, when it has one. */
export function accountLabel(account: Pick<MailAccount, 'email' | 'nickname'>): string {
  return account.nickname ? `${account.nickname} (${account.email})` : account.email;
}

/** Who a list row names: the sender, or the recipients in Sent and Drafts. */
export function rowPeople(summary: MailSummary, role: string | null): string {
  if (role && OUTGOING.has(role)) {
    const to = [...summary.to, ...summary.cc].map(personName);
    return to.length ? `To: ${to.join(', ')}` : '(no recipients)';
  }
  return personName(summary.from);
}

/** Folder choices, indented under their parents, with the unread count. */
export function folderOptions(folders: MailFolder[]): Array<{ value: string; label: string }> {
  return folders
    .filter((f) => f.selectable)
    .map((f) => ({
      value: f.path,
      label: `${'\u00a0\u00a0\u00a0'.repeat(f.depth)}${f.name}${f.unseen ? ` (${f.unseen})` : ''}`,
    }));
}

/** The folder a window starts on: the one it showed before, else the inbox. */
export function startFolder(folders: MailFolder[], saved: unknown): string {
  if (typeof saved === 'string' && folders.some((f) => f.path === saved && f.selectable)) return saved;
  return folders.find((f) => f.role === 'inbox')?.path ?? 'INBOX';
}

/** A small least-recently-used map, for the messages opened this session. */
export class Lru<K, V> {
  private readonly items = new Map<K, V>();

  constructor(private readonly capacity: number) {}

  get(key: K): V | undefined {
    const value = this.items.get(key);
    if (value === undefined) return undefined;
    this.items.delete(key);
    this.items.set(key, value);
    return value;
  }

  set(key: K, value: V): void {
    this.items.delete(key);
    this.items.set(key, value);
    while (this.items.size > this.capacity) this.items.delete(this.items.keys().next().value as K);
  }

  delete(key: K): void {
    this.items.delete(key);
  }

  get size(): number {
    return this.items.size;
  }
}
