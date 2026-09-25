import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check, Trash2 } from 'lucide-react';
import { Button, Checkbox, Logo, Notice } from '@demido/ui';

import type { Context } from '../types';
import styles from '../Setup.module.css';

/** `uninstall.exe --uninstall`: removes the app, and the person's data only when asked. */
export function UninstallFlow({ ctx }: { ctx: Context }) {
  const [removeData, setRemoveData] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setDone(await invoke<string[]>('uninstall', { removeUserData: removeData }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={styles.shell}>
      <main className={styles.main}>
        <div className={styles.page}>
          <div className={styles.pageBody}>
            <div className={styles.hero}>
              {done ? (
                <span className={styles.heroIcon}>
                  <Check size={28} />
                </span>
              ) : (
                <Logo size={56} className={styles.heroLogo} />
              )}
              <h1 className={styles.title}>{done ? 'Demido Studio was removed' : 'Uninstall Demido Studio'}</h1>
              <p className={styles.lead}>
                {done
                  ? 'Thank you for trying it.'
                  : `This removes the app, its runtimes and shortcuts from ${ctx.uninstallDir ?? 'this computer'}.`}
              </p>
            </div>
            {!done && (
              <Checkbox
                checked={removeData}
                onChange={setRemoveData}
                label="Also delete my chats, settings, skills and downloaded models"
                description="Leave this off to keep them for a future installation."
              />
            )}
            {done && done.length > 0 && (
              <ul className={styles.list}>
                {done.map((n) => (
                  <li key={n}>{n}</li>
                ))}
              </ul>
            )}
            {error && (
              <Notice tone="danger" className={styles.spacerTop}>
                {error}
              </Notice>
            )}
          </div>
          <div className={styles.footer}>
            <span className={styles.footerInfo} />
            {done ? (
              <Button variant="primary" size="lg" onClick={() => void invoke('quit')}>
                Close
              </Button>
            ) : (
              <>
                <Button variant="ghost" onClick={() => void invoke('quit')}>
                  Cancel
                </Button>
                <Button variant="danger" size="lg" icon={Trash2} loading={busy} onClick={() => void run()}>
                  Uninstall
                </Button>
              </>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}
