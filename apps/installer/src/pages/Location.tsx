import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { ArrowLeft, ArrowRight, FolderOpen, ShieldCheck, User, Users } from 'lucide-react';
import { Badge, Button, Field, Notice, TextField, cx, formatBytes } from '@demido/ui';

import type { Context, DirInfo, Scope, WizardState } from '../types';
import styles from '../Setup.module.css';

interface Props {
  ctx: Context;
  state: WizardState;
  onChange: (patch: Partial<WizardState>) => void;
  onBack: () => void;
  onNext: () => void;
}

export function LocationPage({ ctx, state, onChange, onBack, onNext }: Props) {
  const [info, setInfo] = useState<DirInfo | null>(null);
  const [elevating, setElevating] = useState(false);
  const [declined, setDeclined] = useState(false);

  useEffect(() => {
    const t = window.setTimeout(() => {
      void invoke<DirInfo>('check_dir', { path: state.installDir }).then(setInfo);
    }, 200);
    return () => window.clearTimeout(t);
  }, [state.installDir]);

  const pickScope = (scope: Scope) => {
    onChange({ scope, installDir: state.customDir ? state.installDir : ctx.defaultDirs[scope] });
  };

  const needsAdmin = state.scope === 'machine' && !ctx.elevated;
  const unwritable = info !== null && !info.writable && !needsAdmin;
  const problem = info?.problem ?? null;
  const next = async () => {
    if (!needsAdmin) {
      onNext();
      return;
    }
    // Everything after this point runs with administrator rights, in a new window.
    setElevating(true);
    setDeclined(false);
    try {
      const accepted = await invoke<boolean>('relaunch_elevated', { wizard: { ...state, step: 3 } });
      if (!accepted) setDeclined(true);
    } finally {
      setElevating(false);
    }
  };

  const Option = ({
    scope,
    icon: Icon,
    title,
    description,
    badge,
  }: {
    scope: Scope;
    icon: typeof User;
    title: string;
    description: string;
    badge?: string;
  }) => (
    <button
      type="button"
      role="radio"
      aria-checked={state.scope === scope}
      className={cx(styles.option, state.scope === scope && styles.selected)}
      onClick={() => pickScope(scope)}
    >
      <span className={styles.radio} />
      <span className={styles.optionIcon}>
        <Icon size={20} strokeWidth={1.8} aria-hidden />
      </span>
      <span className={styles.optionText}>
        <span className={styles.optionTitle}>
          {title}
          {badge && <Badge tone="accent">{badge}</Badge>}
        </span>
        <span className={styles.optionDesc}>{description}</span>
      </span>
    </button>
  );

  return (
    <div className={styles.page}>
      <div className={styles.pageBody}>
        <h1 className={styles.title}>Who is it for?</h1>
        <p className={styles.lead}>Install Demido Studio just for you, or for everyone who uses this computer.</p>
        <div className={styles.options} role="radiogroup" aria-label="Install for">
          <Option
            scope="user"
            icon={User}
            title="Just me"
            badge="Recommended"
            description="Installs in your user folder. No administrator permission needed."
          />
          <Option
            scope="machine"
            icon={Users}
            title="Everyone on this computer"
            description="Installs in Program Files for all users. Windows will ask for administrator permission."
          />
        </div>
        <Field
          className={styles.spacerTop}
          label="Install folder"
          description={
            info
              ? `${info.freeBytes !== null ? `${formatBytes(info.freeBytes)} free` : ''}${info.hasInstall ? ' · an existing installation will be updated' : ''}`
              : ' '
          }
        >
          <div className={styles.row}>
            <TextField
              value={state.installDir}
              onChange={(e) => onChange({ installDir: e.target.value, customDir: true })}
              invalid={unwritable || problem !== null}
            />
            <Button
              variant="secondary"
              icon={FolderOpen}
              onClick={async () => {
                const dir = await open({
                  directory: true,
                  title: 'Choose where to install Demido Studio',
                  defaultPath: state.installDir,
                });
                if (typeof dir === 'string') {
                  const leaf = dir
                    .replace(/[\\/]+$/, '')
                    .split(/[\\/]/)
                    .pop();
                  onChange({ installDir: leaf === 'Demido Studio' ? dir : `${dir}\\Demido Studio`, customDir: true });
                }
              }}
            >
              Browse
            </Button>
          </div>
        </Field>
        {problem && (
          <Notice tone="danger" className={styles.spacerTop}>
            {problem}
          </Notice>
        )}
        {unwritable && !problem && (
          <Notice tone="danger" className={styles.spacerTop}>
            This folder cannot be written to. Pick another folder, or install for everyone.
          </Notice>
        )}
        {needsAdmin && (
          <Notice tone="info" icon={ShieldCheck} className={styles.spacerTop}>
            Setup will restart with administrator permission and continue where you are.
          </Notice>
        )}
        {declined && (
          <Notice tone="warning" className={styles.spacerTop}>
            Administrator permission was not given. Choose “Just me” to install without it.
          </Notice>
        )}
      </div>
      <div className={styles.footer}>
        <Button variant="ghost" icon={ArrowLeft} onClick={onBack}>
          Back
        </Button>
        <span className={styles.footerInfo} />
        <Button
          variant="primary"
          size="lg"
          iconRight={ArrowRight}
          loading={elevating}
          disabled={!state.installDir.trim() || unwritable || problem !== null}
          onClick={() => void next()}
        >
          Next
        </Button>
      </div>
    </div>
  );
}
