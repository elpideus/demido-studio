// The updater: its status, mirrored from `updater://status`, and what the Updates tab, the
// activity bar and the "Update ready" toast can ask of it.

import { create } from 'zustand';

import { api, errorText } from '@/lib/api';
import type { UpdateChannel, UpdateStatus } from '@/lib/types';
import { afterStatus } from '@/settings/updateView';
import { useChats } from './chats';
import { toast, useToasts } from './toasts';

interface UpdatesStore {
  status: UpdateStatus | null;
  /** A restart was asked for while a reply is still being written: the dialog asks first. */
  confirmingRestart: boolean;
  /**
   * The person asked to install. After "Update" the download runs first, and the app restarts
   * once it is verified; Cancel, on the download or on the dialog, takes the request back.
   */
  installRequested: boolean;
  load: () => Promise<void>;
  /** Applies a status from `updater://status`. */
  receive: (status: UpdateStatus) => void;
  check: () => Promise<void>;
  /** "Update": downloads the new version, then restarts to install it (see `restart`). */
  update: () => void;
  /** Installs what is ready, or downloads what was found, straight away. */
  apply: () => Promise<void>;
  /** Installs, first asking when a reply is still being written (it would stop halfway). */
  restart: () => void;
  confirmRestart: () => void;
  cancelRestart: () => void;
  cancel: () => Promise<void>;
  setPreferences: (prefs: { channel?: UpdateChannel; auto?: boolean }) => Promise<void>;
}

// Every change arrives as an event. A command's answer is applied only when no event came while
// it ran: an event is always the newer state (a cancelled download answers "downloading" and
// then reports where it stopped).
let received = 0;

async function run(call: () => Promise<UpdateStatus>, set: (status: UpdateStatus) => void): Promise<UpdateStatus> {
  const before = received;
  const status = await call();
  if (received === before) set(status);
  return status;
}

export const useUpdates = create<UpdatesStore>((set, get) => {
  const setStatus = (status: UpdateStatus) => set({ status });
  return {
    status: null,
    confirmingRestart: false,
    installRequested: false,

    load: async () => {
      await run(api.updateStatus, setStatus);
    },

    receive: (status) => {
      received += 1;
      const step = afterStatus(get().status, status, get().installRequested);
      set({ status, installRequested: step.installRequested });
      if (step.restart) {
        get().restart();
        return;
      }
      if (step.toast) {
        useToasts.getState().push(
          {
            tone: 'success',
            title: step.toast.title,
            body: step.toast.body,
            action: { label: 'Restart now', run: () => get().restart() },
          },
          15_000,
        );
      }
    },

    check: async () => {
      try {
        await run(api.checkForUpdates, setStatus);
      } catch (e) {
        toast.error('Could not check for updates', errorText(e));
      }
    },

    update: () => {
      // Only the download starts now. The restart, and the question before it while a reply is
      // being written, come once the download is verified (see `receive`).
      if (get().status?.phase === 'available') void get().apply();
      else get().restart();
    },

    apply: async () => {
      set({ installRequested: true, confirmingRestart: false });
      try {
        const status = await run(api.applyUpdate, setStatus);
        if (status.phase === 'error' && status.error) {
          set({ installRequested: false });
          toast.error('Could not update', status.error);
        }
      } catch (e) {
        set({ installRequested: false });
        toast.error('Could not update', errorText(e));
      }
    },

    restart: () => {
      const writing = Object.values(useChats.getState().running).some(Boolean);
      if (writing) set({ confirmingRestart: true });
      else void get().apply();
    },

    confirmRestart: () => void get().apply(),

    // The update stays ready for a click; nothing restarts by itself.
    cancelRestart: () => set({ confirmingRestart: false, installRequested: false }),

    cancel: async () => {
      // Even a cancel that comes too late to stop the download (it is being verified) means the
      // person does not want the restart: the update ends ready and waits for a click.
      set({ installRequested: false });
      try {
        await run(api.cancelUpdate, setStatus);
      } catch (e) {
        toast.error('Could not cancel the download', errorText(e));
      }
    },

    setPreferences: async (prefs) => {
      // The switch moves at once; the backend's answer follows.
      const current = get().status;
      const wasRequested = get().installRequested;
      const before = received;
      // Back on Release, a pre-release being downloaded is dropped: the restart asked for it must
      // not install whatever else is staged. Before the await, since "ready" can beat the answer.
      const dropsRequest = prefs.channel === 'release' && current?.release?.prerelease === true;
      if (current) {
        set({
          status: { ...current, channel: prefs.channel ?? current.channel, auto: prefs.auto ?? current.auto },
          ...(dropsRequest ? { installRequested: false } : {}),
        });
      }
      try {
        await run(() => api.setUpdatePreferences(prefs), setStatus);
      } catch (e) {
        if (current && received === before) set({ status: current, installRequested: wasRequested });
        toast.error('Could not save the update settings', errorText(e));
      }
    },
  };
});
