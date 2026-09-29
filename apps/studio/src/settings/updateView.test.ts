// The Updates tab's status card, the activity bar's update button and the "Update ready" toast.

import { describe, expect, it } from 'vitest';
import { formatBytes } from '@demido/ui';

import type { UpdateRelease, UpdateStatus } from '@/lib/types';
import { ADMIN_NOTE, afterStatus, formatDay, readyToast, relativeTime, statusCard, updateButton } from './updateView';

const NOW = Date.parse('2026-09-28T12:00:00Z');

const release = (over: Partial<UpdateRelease> = {}): UpdateRelease => ({
  version: '0.5.0',
  notes: '## New\n- Things',
  publishedAt: '2026-09-20T10:00:00Z',
  url: 'https://github.com/elpideus/demido-studio/releases/tag/v0.5.0',
  size: 25_000_000,
  prerelease: false,
  ...over,
});

const status = (over: Partial<UpdateStatus> = {}): UpdateStatus => ({
  currentVersion: '0.4.0',
  channel: 'release',
  auto: true,
  phase: 'idle',
  release: null,
  progress: null,
  lastChecked: null,
  error: null,
  unsupported: null,
  needsAdmin: false,
  failedAttempt: false,
  ...over,
});

describe('relativeTime', () => {
  const ago = (ms: number) => new Date(NOW - ms).toISOString();

  it('reads like speech for recent times', () => {
    expect(relativeTime(ago(10_000), NOW)).toBe('just now');
    expect(relativeTime(ago(60_000), NOW)).toBe('a minute ago');
    expect(relativeTime(ago(5 * 60_000), NOW)).toBe('5 minutes ago');
    expect(relativeTime(ago(60 * 60_000), NOW)).toBe('an hour ago');
    expect(relativeTime(ago(3 * 3600_000), NOW)).toBe('3 hours ago');
    expect(relativeTime(ago(26 * 3600_000), NOW)).toBe('yesterday');
    expect(relativeTime(ago(4 * 86_400_000), NOW)).toBe('4 days ago');
  });

  it('falls back to the date after a month, and to nothing for garbage', () => {
    const then = NOW - 45 * 86_400_000;
    expect(relativeTime(new Date(then).toISOString(), NOW)).toBe(`on ${formatDay(then)}`);
    expect(relativeTime('not a date', NOW)).toBe('');
  });

  it('treats a clock slightly ahead as now', () => {
    expect(relativeTime(new Date(NOW + 5000).toISOString(), NOW)).toBe('just now');
  });
});

describe('statusCard', () => {
  it('offers a check before the first one', () => {
    const card = statusCard(status(), NOW);
    expect(card.title).toBe('Version 0.4.0');
    expect(card.meta).toBe('Not checked yet');
    expect(card.action).toEqual({ kind: 'check', label: 'Check for updates', primary: false });
  });

  it('says when it last checked while up to date', () => {
    const card = statusCard(status({ phase: 'upToDate', lastChecked: new Date(NOW - 5 * 60_000).toISOString() }), NOW);
    expect(card.title).toBe('Demido Studio is up to date');
    expect(card.meta).toBe('Version 0.4.0 · checked 5 minutes ago');
    expect(card.icon).toBe('ok');
    expect(card.action?.kind).toBe('check');
  });

  it('shows only a spinner while checking and restarting', () => {
    expect(statusCard(status({ phase: 'checking' }), NOW)).toMatchObject({
      icon: 'busy',
      title: 'Checking for updates…',
      action: null,
    });
    expect(statusCard(status({ phase: 'installing', release: release() }), NOW)).toMatchObject({
      icon: 'busy',
      title: 'Restarting to update…',
      action: null,
    });
  });

  it('offers an available version with its date and release notes', () => {
    const card = statusCard(status({ phase: 'available', release: release() }), NOW);
    expect(card.title).toBe('Version 0.5.0 is available');
    expect(card.meta).toBe(`Released ${formatDay(Date.parse('2026-09-20T10:00:00Z'))}`);
    expect(card.notesUrl).toBe(release().url);
    expect(card.action).toEqual({ kind: 'update', label: 'Update', primary: true });
    expect(card.adminNote).toBeNull();
  });

  it('marks pre-releases and a machine-wide install that asks for permission', () => {
    const card = statusCard(
      status({
        phase: 'available',
        needsAdmin: true,
        release: release({ version: '0.6.0-beta.1', prerelease: true, publishedAt: null }),
      }),
      NOW,
    );
    expect(card.title).toBe('Version 0.6.0-beta.1 is available');
    expect(card.meta).toBe('Pre-release');
    expect(card.adminNote).toBe(ADMIN_NOTE);
  });

  it('shows download progress and speed', () => {
    const card = statusCard(
      status({
        phase: 'downloading',
        release: release(),
        progress: { downloaded: 10_000_000, total: 25_000_000, bytesPerSecond: 2_500_000 },
      }),
      NOW,
    );
    expect(card.title).toBe('Downloading 0.5.0');
    expect(card.progress).toBeCloseTo(0.4);
    expect(card.meta).toBe(`${formatBytes(10_000_000)} of ${formatBytes(25_000_000)} · ${formatBytes(2_500_000)}/s`);
    expect(card.action).toEqual({ kind: 'cancel', label: 'Cancel', primary: false });
  });

  it('downloads of an unknown size show what arrived, without a fraction', () => {
    const card = statusCard(
      status({
        phase: 'downloading',
        release: release({ size: 0 }),
        progress: { downloaded: 1_000_000, total: null, bytesPerSecond: 0 },
      }),
      NOW,
    );
    expect(card.progress).toBeNull();
    expect(card.meta).toBe(formatBytes(1_000_000));
  });

  it('asks for a restart when an update is ready', () => {
    const card = statusCard(status({ phase: 'ready', release: release() }), NOW);
    expect(card.title).toBe('Version 0.5.0 is ready to install');
    expect(card.meta).toBe('Demido Studio restarts to finish. It takes a few seconds.');
    expect(card.action).toEqual({ kind: 'restart', label: 'Restart and update', primary: true });
    expect(card.warning).toBeNull();
  });

  it('says when the last attempt did not finish', () => {
    const card = statusCard(status({ phase: 'ready', release: release(), failedAttempt: true }), NOW);
    expect(card.warning).toBe('The last attempt to install 0.5.0 did not finish.');
    expect(card.action).toEqual({ kind: 'restart', label: 'Try again', primary: true });
  });

  it('shows the error and retries with a check', () => {
    const card = statusCard(status({ phase: 'error', error: 'GitHub is limiting update checks right now.' }), NOW);
    expect(card.title).toBe('Could not update');
    expect(card.meta).toBe('GitHub is limiting update checks right now.');
    expect(card.action).toEqual({ kind: 'check', label: 'Try again', primary: false });
  });

  it('sends a copy that cannot install updates to GitHub instead', () => {
    const unsupported = 'This is a development build; update it with git.';
    for (const phase of ['available', 'ready'] as const) {
      const card = statusCard(status({ phase, release: release(), unsupported, needsAdmin: true }), NOW);
      expect(card.title).toBe('Version 0.5.0 is available');
      expect(card.action).toEqual({ kind: 'github', label: 'Download from GitHub', primary: false });
      expect(card.adminNote).toBeNull();
      expect(card.warning).toBeNull();
    }
    // Checking still works.
    expect(statusCard(status({ phase: 'upToDate', unsupported }), NOW).action?.kind).toBe('check');
  });
});

describe('updateButton', () => {
  it('stays hidden while there is nothing to install', () => {
    for (const phase of ['idle', 'checking', 'upToDate', 'error'] as const) {
      expect(updateButton(status({ phase }))).toBeNull();
    }
    expect(updateButton(null)).toBeNull();
  });

  it('describes each step of an update', () => {
    expect(updateButton(status({ phase: 'available', release: release() }))).toEqual({
      tooltip: 'Update available: 0.5.0',
      restart: false,
    });
    expect(
      updateButton(
        status({
          phase: 'downloading',
          release: release(),
          progress: { downloaded: 12_600_000, total: 25_000_000, bytesPerSecond: 1 },
        }),
      ),
    ).toEqual({ tooltip: 'Downloading update… 50%', restart: false });
    expect(updateButton(status({ phase: 'ready', release: release() }))).toEqual({
      tooltip: 'Restart to update to 0.5.0',
      restart: true,
    });
    expect(updateButton(status({ phase: 'installing', release: release() }))).toEqual({
      tooltip: 'Restarting…',
      restart: false,
    });
  });

  it('opens the Updates tab when this copy cannot install, or after a failed attempt', () => {
    expect(updateButton(status({ phase: 'ready', release: release(), unsupported: 'Not installed.' }))).toEqual({
      tooltip: 'Update available: 0.5.0',
      restart: false,
    });
    expect(updateButton(status({ phase: 'error', release: release(), failedAttempt: true }))?.restart).toBe(false);
  });
});

describe('readyToast', () => {
  const downloading = status({ phase: 'downloading', release: release() });
  const ready = status({ phase: 'ready', release: release() });

  it('announces a finished background download once', () => {
    expect(readyToast(downloading, ready, false)).toEqual({
      title: 'Update ready',
      body: 'Demido Studio 0.5.0 installs the next time it starts.',
    });
    expect(readyToast(ready, ready, false)).toBeNull();
    // A check that ends back on the update already ready is not news.
    expect(readyToast(status({ phase: 'checking', release: release() }), ready, false)).toBeNull();
    expect(readyToast(null, ready, false)).toBeNull();
  });

  it('stays quiet when the person is installing it, in manual mode, or when this copy cannot install', () => {
    expect(readyToast(downloading, ready, true)).toBeNull();
    expect(readyToast(downloading, { ...ready, auto: false }, false)).toBeNull();
    expect(readyToast(downloading, { ...ready, unsupported: 'Development build.' }, false)).toBeNull();
    expect(readyToast(downloading, { ...ready, phase: 'available' }, false)).toBeNull();
  });

  it('tells a machine-wide install that it waits for permission', () => {
    expect(readyToast(downloading, { ...ready, needsAdmin: true }, false)?.body).toBe(
      'Demido Studio 0.5.0 is ready to install. Windows will ask for permission.',
    );
  });
});

describe('afterStatus', () => {
  const downloading = status({ phase: 'downloading', release: release() });
  const ready = status({ phase: 'ready', release: release() });

  it('restarts once a download the person asked to install is verified, without a toast', () => {
    expect(afterStatus(downloading, ready, true)).toEqual({ installRequested: true, restart: true, toast: null });
    // Manual mode too: the backend never installs by itself.
    expect(afterStatus(downloading, { ...ready, auto: false }, true).restart).toBe(true);
  });

  it('keeps the request through the download and the install', () => {
    const available = status({ phase: 'available', release: release() });
    expect(afterStatus(available, downloading, true)).toEqual({ installRequested: true, restart: false, toast: null });
    expect(afterStatus(downloading, downloading, true).installRequested).toBe(true);
    expect(afterStatus(ready, status({ phase: 'installing', release: release() }), true)).toMatchObject({
      installRequested: true,
      restart: false,
    });
    // A check that ends back on "ready" (the dialog may be open) does not restart again.
    expect(afterStatus(ready, ready, true)).toMatchObject({ installRequested: true, restart: false });
  });

  it('drops the request when the update stops on the way', () => {
    for (const phase of ['error', 'available', 'upToDate'] as const) {
      expect(afterStatus(downloading, status({ phase, release: release() }), true)).toMatchObject({
        installRequested: false,
        restart: false,
      });
    }
  });

  it('only announces a download nobody asked to install', () => {
    // Cancelled while it was being verified: it ends ready, announced, and nothing restarts.
    expect(afterStatus(downloading, ready, false)).toEqual({
      installRequested: false,
      restart: false,
      toast: { title: 'Update ready', body: 'Demido Studio 0.5.0 installs the next time it starts.' },
    });
    expect(afterStatus(downloading, { ...ready, auto: false }, false)).toEqual({
      installRequested: false,
      restart: false,
      toast: null,
    });
  });
});
