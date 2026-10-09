import { describe, expect, it } from 'vitest';

import type { MailFolder, MailSummary } from '@/lib/types';
import {
  contentPolicy,
  hasRemoteCss,
  isRemoteSrcset,
  isRemoteUrl,
  isUnsafeUrl,
  linkTarget,
  splitLinks,
} from './emailHtml';
import { Lru, folderOptions, listDate, personLabel, rowPeople, startFolder } from './mailView';

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

const summary = (extra: Partial<MailSummary> = {}): MailSummary => ({
  id: 1,
  account: 'a',
  folder: 'INBOX',
  uid: 1,
  date: 0,
  from: { name: 'Ada', email: 'ada@example.com' },
  to: [{ name: '', email: 'bob@example.com' }],
  cc: [],
  subject: 'Hi',
  snippet: '',
  unread: false,
  flagged: false,
  answered: false,
  draft: false,
  attachments: 0,
  labels: [],
  size: 0,
  messageId: null,
  ...extra,
});

describe('email html', () => {
  it('loads remote images only once they are shown', () => {
    expect(contentPolicy(false)).toContain('img-src data:;');
    expect(contentPolicy(true)).toContain('img-src data: https:;');
    for (const policy of [contentPolicy(false), contentPolicy(true)]) {
      expect(policy).toContain("default-src 'none'");
      expect(policy).not.toContain('script');
    }
  });

  it('tells remote addresses from embedded ones', () => {
    expect(isRemoteUrl('https://t.example.com/p.gif')).toBe(true);
    expect(isRemoteUrl(' //cdn.example.com/x.png')).toBe(true);
    expect(isRemoteUrl('data:image/png;base64,AAAA')).toBe(false);
    expect(isRemoteUrl(null)).toBe(false);
    expect(isRemoteSrcset('a.png 1x, https://x.example/b.png 2x')).toBe(true);
    expect(isRemoteSrcset('data:image/png;base64,AA 1x')).toBe(false);
    expect(hasRemoteCss("background: url('https://x.example/bg.png')")).toBe(true);
    expect(hasRemoteCss('background:url( //x.example/bg.png )')).toBe(true);
    expect(hasRemoteCss('color: red; background: url(data:image/png;base64,AA)')).toBe(false);
  });

  it('drops links that run code', () => {
    expect(isUnsafeUrl('javascript:alert(1)')).toBe(true);
    expect(isUnsafeUrl(' JaVa\tScRipt:alert(1)')).toBe(true);
    expect(isUnsafeUrl('data:text/html,<b>x</b>')).toBe(true);
    expect(isUnsafeUrl('data:image/png;base64,AA')).toBe(false);
    expect(isUnsafeUrl('https://example.com')).toBe(false);
    expect(linkTarget('https://example.com/a')).toBe('https://example.com/a');
    expect(linkTarget('mailto:ada@example.com')).toBe('mailto:ada@example.com');
    expect(linkTarget('#top')).toBeNull();
    expect(linkTarget('file:///C:/x')).toBeNull();
  });

  it('finds the links in plain text', () => {
    expect(splitLinks('See https://example.com/a?b=1. Thanks')).toEqual([
      { text: 'See ' },
      { text: 'https://example.com/a?b=1', href: 'https://example.com/a?b=1' },
      { text: '. Thanks' },
    ]);
    expect(splitLinks('(https://en.wikipedia.org/wiki/Rust_(language))').map((p) => p.href)).toEqual([
      undefined,
      'https://en.wikipedia.org/wiki/Rust_(language)',
      undefined,
    ]);
    expect(splitLinks('no links here')).toEqual([{ text: 'no links here' }]);
  });
});

describe('mail view', () => {
  it('dates recent mail by time and old mail by day', () => {
    const now = new Date(2026, 9, 8, 15, 0);
    expect(listDate(new Date(2026, 9, 8, 9, 5).getTime(), now)).toMatch(/09.05|9.05/);
    expect(listDate(new Date(2026, 2, 3).getTime(), now)).not.toMatch(/2026/);
    expect(listDate(new Date(2024, 2, 3).getTime(), now)).toMatch(/2024/);
    expect(listDate(0, now)).toBe('');
  });

  it('names the sender, or the recipients of sent mail', () => {
    expect(rowPeople(summary(), 'inbox')).toBe('Ada');
    expect(rowPeople(summary(), 'sent')).toBe('To: bob@example.com');
    expect(rowPeople(summary({ from: { name: '', email: '' } }), null)).toBe('(unknown sender)');
    expect(personLabel({ name: 'Ada', email: 'ada@example.com' })).toBe('Ada <ada@example.com>');
    expect(personLabel({ name: '', email: 'ada@example.com' })).toBe('ada@example.com');
  });

  it('lists the folders that can be opened, indented, with unread counts', () => {
    const folders = [
      folder('INBOX', { role: 'inbox', name: 'Inbox', unseen: 3 }),
      folder('[Gmail]', { selectable: false }),
      folder('[Gmail]/Sent Mail', { role: 'sent' }),
    ];
    expect(folderOptions(folders)).toEqual([
      { value: 'INBOX', label: 'Inbox (3)' },
      { value: '[Gmail]/Sent Mail', label: '\u00a0\u00a0\u00a0Sent Mail' },
    ]);
    expect(startFolder(folders, '[Gmail]/Sent Mail')).toBe('[Gmail]/Sent Mail');
    expect(startFolder(folders, '[Gmail]')).toBe('INBOX');
    expect(startFolder(folders, undefined)).toBe('INBOX');
    expect(startFolder([], 'Gone')).toBe('INBOX');
  });

  it('keeps the most recently used entries', () => {
    const lru = new Lru<number, string>(2);
    lru.set(1, 'a');
    lru.set(2, 'b');
    expect(lru.get(1)).toBe('a');
    lru.set(3, 'c');
    expect(lru.get(2)).toBeUndefined();
    expect(lru.get(1)).toBe('a');
    expect(lru.size).toBe(2);
  });
});
