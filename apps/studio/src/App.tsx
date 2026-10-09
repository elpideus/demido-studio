import { useEffect, useState } from 'react';
import { Spinner } from '@demido/ui';

import { api, errorText } from '@/lib/api';
import { on } from '@/lib/events';
import { chartPatch } from '@/market/chartCommands';
import { Disclaimer } from '@/shell/Disclaimer';
import { RestartDialog } from '@/shell/RestartDialog';
import { Shell } from '@/shell/Shell';
import { Toaster } from '@/shell/Toaster';
import { useApp } from '@/stores/app';
import { useChats } from '@/stores/chats';
import { useMail } from '@/stores/mail';
import { useMarket } from '@/stores/market';
import { findModel, useModels } from '@/stores/models';
import { usePine } from '@/stores/pine';
import { useSkills } from '@/stores/skills';
import { toast } from '@/stores/toasts';
import { useUpdates } from '@/stores/updates';
import { useWindows } from '@/stores/windows';
import styles from './App.module.css';

/** Mirrors backend events into the stores for the lifetime of the app. */
function subscribe(): Array<Promise<() => void>> {
  return [
    on('chat://event', (e) => useChats.getState().apply(e)),
    on('models://changed', (models) => useModels.getState().setModels(models)),
    on('runtime://status', (status) => useModels.getState().setRuntime(status)),
    on('downloads://changed', (job) => {
      const before = useModels.getState().downloads.find((j) => j.id === job.id);
      useModels.getState().upsertDownload(job);
      if (job.state === 'done' && before?.state !== 'done')
        toast.success('Download complete', `${job.name} is ready to use.`);
      if (job.state === 'failed' && before?.state !== 'failed' && job.error !== 'cancelled') {
        toast.error('Download failed', job.error ?? job.name);
      }
    }),
    on('skills://changed', (skills) => {
      useSkills.getState().setSkills(skills);
      void useSkills.getState().refreshGroups();
    }),
    on('market://status', (status) => {
      const before = useMarket.getState().status;
      useMarket.getState().set(status);
      if (status.loggedIn && !before?.loggedIn) toast.success('Signed in to TradingView', status.username ?? undefined);
    }),
    on('market://event', (e) => {
      if (e.event === 'pine.changed') usePine.getState().changed(e.params.id, e.params.script);
    }),
    // The assistant's `chart_add_indicator` and `chart_draw`: the Market window shows them.
    on('market://chart', (cmd) => {
      const { windows, open, setProps } = useWindows.getState();
      const win = windows.find((w) => w.kind === 'market');
      if (cmd.action === 'undraw') {
        if (win) setProps(win.id, chartPatch(win.props, cmd));
      } else open('market', chartPatch(win?.props ?? {}, cmd));
    }),
    on('updater://status', (status) => useUpdates.getState().receive(status)),
    on('mail://accounts', (accounts) => useMail.getState().set(accounts)),
  ];
}

function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const key = e.key.toLowerCase();
      if (key === 'n' && !e.shiftKey) {
        e.preventDefault();
        void useChats.getState().open(null);
        window.dispatchEvent(new Event('demido:focus-composer'));
      } else if (key === 'b') {
        e.preventDefault();
        const open = useApp.getState().settings?.chatListOpen ?? true;
        void useApp.getState().patchSettings({ chatListOpen: !open });
      } else if (key === ',') {
        e.preventDefault();
        useWindows.getState().open('settings');
      } else if (key === 'w') {
        const focused = useWindows.getState().focusedId;
        if (focused) {
          e.preventDefault();
          useWindows.getState().close(focused);
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}

/** Saves the window layout a moment after it last changed. */
function usePersistLayout(ready: boolean) {
  useEffect(() => {
    if (!ready) return undefined;
    let timer: number | undefined;
    const unsubscribe = useWindows.subscribe((state, prev) => {
      if (state.windows === prev.windows && state.splits === prev.splits) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        void api.updateSettings({ windowLayout: useWindows.getState().snapshot() });
      }, 800);
    });
    return () => {
      window.clearTimeout(timer);
      unsubscribe();
    };
  }, [ready]);
}

export function App() {
  const [ready, setReady] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(() => {
    const unsubs = subscribe();
    (async () => {
      await useApp.getState().init();
      const results = await Promise.allSettled([
        useModels.getState().load(),
        useSkills.getState().load(),
        useMarket.getState().load(),
        useMail.getState().load(),
        usePine.getState().load(),
        useChats.getState().loadChats(),
        useUpdates.getState().load(),
      ]);
      for (const r of results) {
        if (r.status === 'rejected') toast.error('Part of the app did not load', errorText(r.reason));
      }
      const settings = useApp.getState().settings;
      useWindows.getState().restore(settings?.windowLayout);
      const last = settings?.lastChatId;
      if (last && useChats.getState().chats.some((c) => c.id === last)) await useChats.getState().open(last);
      setReady(true);
    })()
      .catch((e) => setFailure(errorText(e)))
      .finally(() => requestAnimationFrame(() => void api.windowReady()));
    return () => {
      for (const u of unsubs) void u.then((f) => f());
    };
  }, []);

  useShortcuts();
  usePersistLayout(ready);

  if (failure) {
    return (
      <div className={styles.boot}>
        <h1>Demido Studio could not start</h1>
        <p>{failure}</p>
      </div>
    );
  }
  if (!ready) {
    return (
      <div className={styles.boot}>
        <Spinner size={22} />
      </div>
    );
  }
  return (
    <>
      <Shell />
      <Disclaimer />
      <Toaster />
      <RestartDialog />
    </>
  );
}
