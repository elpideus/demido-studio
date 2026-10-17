import { describe, expect, it } from 'vitest';

import type { MailFolder, MailSummary } from '@/lib/types';
import {
  archiveFolder,
  archivesFrom,
  folderTrails,
  isSystemFolder,
  moveTargets,
  nextAfter,
  rangeIds,
  splitAddresses,
} from './mailActions';

const folder = (path: string, extra: Partial<MailFolder> = {}): MailFolder => ({
  path,
  name: path.split('/').at(-1)!,
  role: null,
  selectable: true,
  depth: path.split('/').length - 1,
  total: null,
  unseen: null,
  ...extra,
});

const message = (id: number): MailSummary => ({
  id,
  account: 'a',
  folder: 'INBOX',
  uid: id,
  date: 0,
  from: { name: '', email: 'ada@example.com' },
  to: [],
  cc: [],
  subject: '',
  snippet: '',
  unread: false,
  flagged: false,
  answered: false,
  draft: false,
  attachments: 0,
  labels: [],
  size: 0,
  messageId: null,
});

const gmail = [
  folder('INBOX', { role: 'inbox', name: 'Inbox' }),
  folder('[Gmail]', { selectable: false }),
  folder('[Gmail]/All Mail', { role: 'all' }),
  folder('[Gmail]/Drafts', { role: 'drafts' }),
  folder('[Gmail]/Starred', { role: 'flagged' }),
  folder('[Gmail]/Trash', { role: 'trash' }),
  folder('Work'),
  folder('Work/Clients'),
];

describe('mail actions', () => {
  it('keeps the folders the mail system needs', () => {
    expect(isSystemFolder(folder('INBOX'))).toBe(true);
    expect(isSystemFolder(folder('Trash', { role: 'trash' }))).toBe(true);
    expect(isSystemFolder(folder('Work'))).toBe(false);
  });

  it('archives to the archive folder, or to All Mail on Gmail', () => {
    expect(archiveFolder(gmail, { kind: 'gmail' })?.path).toBe('[Gmail]/All Mail');
    expect(archiveFolder(gmail, { kind: 'imap' })).toBeNull();
    const imap = [folder('INBOX', { role: 'inbox' }), folder('Archive', { role: 'archive' })];
    expect(archiveFolder(imap, { kind: 'imap' })?.path).toBe('Archive');
    expect(archivesFrom('inbox')).toBe(true);
    expect(archivesFrom(null)).toBe(true);
    expect(archivesFrom('trash')).toBe(false);
  });

  it('moves mail only where it can go', () => {
    const moves = moveTargets(gmail, 'INBOX', false).map((f) => f.path);
    expect(moves).toEqual(['[Gmail]/All Mail', '[Gmail]/Trash', 'Work', 'Work/Clients']);
    const labels = moveTargets(gmail, 'Work', true).map((f) => f.path);
    expect(labels).toEqual(['INBOX', 'Work/Clients']);
  });

  it('names folders with the folders they are in', () => {
    const trails = folderTrails(gmail);
    expect(trails.get('Work/Clients')).toBe('Work / Clients');
    expect(trails.get('[Gmail]/Trash')).toBe('[Gmail] / Trash');
    expect(trails.get('INBOX')).toBe('Inbox');
  });

  it('picks a range either way', () => {
    const list = [1, 2, 3, 4, 5].map(message);
    expect(rangeIds(list, 2, 4)).toEqual([2, 3, 4]);
    expect(rangeIds(list, 4, 2)).toEqual([2, 3, 4]);
    expect(rangeIds(list, 9, 3)).toEqual([3]);
    expect(rangeIds(list, 1, 9)).toEqual([]);
  });

  it('shows the next message once some leave the list', () => {
    const list = [1, 2, 3, 4, 5].map(message);
    expect(nextAfter(list, new Set([2, 3]), 2)?.id).toBe(4);
    expect(nextAfter(list, new Set([4, 5]), 5)?.id).toBe(3);
    expect(nextAfter(list, new Set([3]), null)?.id).toBe(4);
    expect(nextAfter(list, new Set([1, 2, 3, 4, 5]), 1)).toBeNull();
  });

  it('splits addresses outside quoted names', () => {
    expect(splitAddresses('"Doe, Jane" <jane@example.com>; bo@example.com,, ')).toEqual([
      '"Doe, Jane" <jane@example.com>',
      'bo@example.com',
    ]);
  });
});
