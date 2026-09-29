import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, ArrowRight, Download, RefreshCw, Wrench } from 'lucide-react';
import { Button, Logo, Notice } from '@demido/ui';

import type { Context, Relation } from '../types';
import styles from '../Setup.module.css';

interface Props {
  ctx: Context;
  /** The full wizard, which also updates an installation in place with new choices. */
  onNext: () => void;
  /** Updates the installation in `dir` as it is, without the wizard. */
  onUpdate: (dir: string) => void;
}

export function WelcomePage({ ctx, onNext, onUpdate }: Props) {
  // Without an app inside it, setup has nothing to update the installation with.
  const existing = ctx.hasPayload ? ctx.existing : null;
  const [elevating, setElevating] = useState(false);
  const [declined, setDeclined] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const update = async () => {
    if (!existing) return;
    if (existing.scope !== 'machine' || ctx.elevated) {
      onUpdate(existing.dir);
      return;
    }
    // An installation for everyone is updated by an elevated copy of setup, in its own window.
    setElevating(true);
    setDeclined(false);
    setError(null);
    try {
      if (!(await invoke<boolean>('elevate_update', { dir: existing.dir }))) setDeclined(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setElevating(false);
    }
  };

  const action: Record<Relation, { label: string; icon: typeof Download }> = {
    older: { label: `Update to ${ctx.version}`, icon: RefreshCw },
    same: { label: 'Repair', icon: Wrench },
    newer: { label: `Install ${ctx.version}`, icon: Download },
  };

  return (
    <div className={styles.page}>
      <div className={styles.pageBody}>
        <div className={styles.hero}>
          <Logo size={56} className={styles.heroLogo} />
          <h1 className={styles.title}>Welcome to Demido Studio</h1>
          <p className={styles.lead}>
            An AI workspace that runs on your own computer. Setup downloads everything it needs and picks what fits your
            hardware, so it works as soon as it opens.
          </p>
        </div>
        {existing ? (
          <div className={styles.installed}>
            <div className={styles.installedText}>
              <div className={styles.installedTitle}>
                Demido Studio {existing.version} is installed in {existing.dir}
              </div>
              <div className={styles.installedNote}>Your chats, models and settings are kept.</div>
              {existing.relation === 'newer' && (
                <div className={styles.installedWarning}>
                  <AlertTriangle size={14} aria-hidden />
                  Version {ctx.version} is older than the installed version.
                </div>
              )}
            </div>
          </div>
        ) : (
          <ul className={styles.list}>
            <li>An AI runtime matched to your graphics card</li>
            <li>Python and Node.js, used by the assistant's tools (analysis, market data)</li>
            <li>A first model sized for your computer, from the Qwen or Gemma family</li>
          </ul>
        )}
        {!existing && ctx.existing && (
          <Notice tone="info" title={`Demido Studio ${ctx.existing.version} is already installed`}>
            Continuing updates it in {ctx.existing.dir}. Your chats, models and settings are kept.
          </Notice>
        )}
        {declined && (
          <Notice tone="warning" className={styles.spacerTop}>
            Administrator permission was not given.
          </Notice>
        )}
        {error && (
          <Notice tone="danger" className={styles.spacerTop}>
            {error}
          </Notice>
        )}
        {!ctx.hasPayload && (
          <Notice tone="warning" title="Development build" className={styles.spacerTop}>
            This setup has no app inside it, so it only installs the runtimes.
          </Notice>
        )}
      </div>
      <div className={styles.footer}>
        <span className={styles.footerInfo}>You need an internet connection during setup.</span>
        {existing ? (
          <>
            <Button variant="ghost" onClick={onNext}>
              Change options
            </Button>
            <Button
              variant="primary"
              size="lg"
              icon={action[existing.relation].icon}
              loading={elevating}
              onClick={() => void update()}
            >
              {action[existing.relation].label}
            </Button>
          </>
        ) : (
          <Button variant="primary" size="lg" iconRight={ArrowRight} onClick={onNext}>
            Get started
          </Button>
        )}
      </div>
    </div>
  );
}
