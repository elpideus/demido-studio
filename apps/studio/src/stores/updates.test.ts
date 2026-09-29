// The updates store: "Update" downloads, and the restart that installs comes once the download is
// verified, asking first while a reply is still being written.

import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { UpdateRelease, UpdateStatus } from '@/lib/types';
import type { useChats as UseChats } from './chats';
import type { useUpdates as UseUpdates } from './updates';

const mocks = vi.hoisted(() => ({
  applyUpdate: vi.fn(),
  cancelUpdate: vi.fn(),
  setUpdatePreferences: vi.fn(),
  toasts: [] as Array<{ title: string }>,
  errors: [] as string[],
}));

vi.mock('@/lib/api', () => ({
  api: {
    applyUpdate: mocks.applyUpdate,
    cancelUpdate: mocks.cancelUpdate,
    setUpdatePreferences: mocks.setUpdatePreferences,
  },
  errorText: (e: unknown) => String(e),
}));

vi.mock('./toasts', () => ({
  useToasts: { getState: () => ({ push: (t: { title: string }) => mocks.toasts.push(t) }) },
  toast: {
    error: (title: string) => mocks.errors.push(title),
    info: () => {},
    success: () => {},
    warning: () => {},
  },
}));

const release: UpdateRelease = {
  version: '0.5.0',
  notes: '',
  publishedAt: null,
  url: 'https://github.com/elpideus/demido-studio/releases/tag/v0.5.0',
  size: 25_000_000,
  prerelease: false,
};

const status = (phase: UpdateStatus['phase'], over: Partial<UpdateStatus> = {}): UpdateStatus => ({
  currentVersion: '0.4.0',
  channel: 'release',
  auto: false,
  phase,
  release: phase === 'upToDate' || phase === 'idle' ? null : release,
  progress: null,
  lastChecked: null,
  error: null,
  unsupported: null,
  needsAdmin: false,
  failedAttempt: false,
  ...over,
});

let useUpdates: typeof UseUpdates;
let useChats: typeof UseChats;

// Every test gets a fresh store: the event counter lives at module level.
beforeEach(async () => {
  vi.resetModules();
  mocks.applyUpdate.mockReset();
  mocks.cancelUpdate.mockReset();
  mocks.setUpdatePreferences.mockReset();
  mocks.toasts.length = 0;
  mocks.errors.length = 0;
  ({ useUpdates } = await import('./updates'));
  ({ useChats } = await import('./chats'));
});

const store = () => useUpdates.getState();
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const writing = (on: boolean) => useChats.setState({ running: on ? { chat: true } : {} });

/** Clicks "Update" on an available version and lets the download start. */
async function clickUpdate() {
  useUpdates.setState({ status: status('available') });
  mocks.applyUpdate.mockResolvedValueOnce(status('downloading'));
  store().update();
  await settle();
  store().receive(status('downloading'));
}

describe('Update', () => {
  it('downloads first and restarts once the download is verified', async () => {
    await clickUpdate();
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(1);
    expect(store().installRequested).toBe(true);

    mocks.applyUpdate.mockResolvedValueOnce(status('installing'));
    store().receive(status('ready'));
    // The second call installs.
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(2);
    expect(store().confirmingRestart).toBe(false);
    expect(mocks.toasts).toEqual([]);
  });

  it('asks when the download is done, not at the click, while a reply is being written', async () => {
    writing(true);
    await clickUpdate();
    expect(store().confirmingRestart).toBe(false);
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(1);

    store().receive(status('ready'));
    expect(store().confirmingRestart).toBe(true);
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(1);

    mocks.applyUpdate.mockResolvedValueOnce(status('installing'));
    store().confirmRestart();
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(2);
  });

  it('stays ready when the person cancels the restart', async () => {
    writing(true);
    await clickUpdate();
    store().receive(status('ready'));
    store().cancelRestart();
    expect(store()).toMatchObject({ confirmingRestart: false, installRequested: false });
    // A later check that ends on "ready" again restarts nothing.
    store().receive(status('checking'));
    store().receive(status('ready'));
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(1);
    expect(store().confirmingRestart).toBe(false);
  });

  it('does not restart after a cancel that came while the download was being verified', async () => {
    await clickUpdate();
    // The backend answers "downloading": the cancel came too late to stop it.
    mocks.cancelUpdate.mockResolvedValueOnce(status('downloading'));
    await store().cancel();
    expect(store().installRequested).toBe(false);

    store().receive(status('ready'));
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(1);
    expect(store().confirmingRestart).toBe(false);
    expect(store().status?.phase).toBe('ready');
  });

  it('forgets the request when going back to Release drops the pre-release being downloaded', async () => {
    const beta = { ...release, version: '0.5.0-beta.1', prerelease: true };
    useUpdates.setState({ status: status('available', { channel: 'prerelease', release: beta }) });
    mocks.applyUpdate.mockResolvedValueOnce(status('downloading', { channel: 'prerelease', release: beta }));
    store().update();
    await settle();
    expect(store().installRequested).toBe(true);

    mocks.setUpdatePreferences.mockResolvedValueOnce(status('ready', { release }));
    await store().setPreferences({ channel: 'release' });
    expect(store().installRequested).toBe(false);
    // An older stable update that was staged stays ready, and nothing restarts into it.
    store().receive(status('ready', { release }));
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(1);
  });

  it('forgets the request when the download fails', async () => {
    await clickUpdate();
    store().receive(status('error', { error: 'GitHub answered 404 to the download.' }));
    expect(store().installRequested).toBe(false);
  });
});

describe('Restart and update', () => {
  it('installs at once when no reply is being written', () => {
    useUpdates.setState({ status: status('ready') });
    mocks.applyUpdate.mockResolvedValueOnce(status('installing'));
    store().update();
    expect(mocks.applyUpdate).toHaveBeenCalledTimes(1);
    expect(store().installRequested).toBe(true);
  });

  it('asks first while a reply is being written', () => {
    writing(true);
    useUpdates.setState({ status: status('ready') });
    store().restart();
    expect(store().confirmingRestart).toBe(true);
    expect(mocks.applyUpdate).not.toHaveBeenCalled();
  });
});

describe('the "Update ready" toast', () => {
  it('announces a background download in automatic mode, with a restart button', () => {
    useUpdates.setState({ status: status('downloading', { auto: true }) });
    store().receive(status('ready', { auto: true }));
    expect(mocks.toasts.map((t) => t.title)).toEqual(['Update ready']);
    expect(mocks.applyUpdate).not.toHaveBeenCalled();
  });
});
