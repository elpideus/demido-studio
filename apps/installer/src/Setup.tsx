import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check } from 'lucide-react';
import { Logo, Spinner, cx } from '@demido/ui';

import { DonePage, InstallPage, useInstall } from './pages/Install';
import { LocationPage } from './pages/Location';
import { ModelPage } from './pages/Model';
import { ReviewPage } from './pages/Review';
import { RuntimePage } from './pages/Runtime';
import { UninstallFlow } from './pages/Uninstall';
import { WelcomePage } from './pages/Welcome';
import type { Context, WizardState } from './types';
import styles from './Setup.module.css';

const STEPS = ['Welcome', 'AI runtime', 'Install location', 'Model', 'Review', 'Install', 'Finish'];

function initialState(ctx: Context): WizardState {
  if (ctx.resume) return ctx.resume;
  const backend = ctx.defaultBackend;
  const scope = ctx.existing?.scope ?? 'user';
  return {
    step: 0,
    backend,
    scope,
    installDir: ctx.existing?.dir ?? ctx.defaultDirs[scope],
    customDir: false,
    family: ctx.recommendations[backend]?.defaultFamily ?? null,
    shortcuts: true,
  };
}

export function Setup() {
  const [ctx, setCtx] = useState<Context | null>(null);
  const [state, setState] = useState<WizardState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const install = useInstall();

  useEffect(() => {
    invoke<Context>('setup_context')
      .then((c) => {
        setCtx(c);
        setState(initialState(c));
      })
      .catch((e) => setError(String(e)))
      .finally(() => requestAnimationFrame(() => void invoke('window_ready')));
  }, []);

  const update = (patch: Partial<WizardState>) => setState((s) => (s ? { ...s, ...patch } : s));
  const go = (step: number) => update({ step });

  const sidebar = useMemo(
    () =>
      state && (
        <ol className={styles.steps}>
          {STEPS.map((label, i) => (
            <li
              key={label}
              className={cx(styles.step, i === state.step && styles.current, i < state.step && styles.completed)}
            >
              <span className={styles.stepMark}>{i < state.step ? <Check size={12} strokeWidth={3} /> : i + 1}</span>
              {label}
            </li>
          ))}
        </ol>
      ),
    [state],
  );

  if (error) return <div className={styles.fatal}>Setup could not start: {error}</div>;
  if (!ctx || !state) {
    return (
      <div className={styles.fatal}>
        <Spinner size={22} />
      </div>
    );
  }

  if (ctx.mode === 'uninstall') return <UninstallFlow ctx={ctx} />;

  const page = (() => {
    switch (state.step) {
      case 0:
        return <WelcomePage ctx={ctx} onNext={() => go(1)} />;
      case 1:
        return (
          <RuntimePage
            ctx={ctx}
            state={state}
            onChange={(backend) =>
              update({ backend, family: ctx.recommendations[backend]?.defaultFamily ?? state.family })
            }
            onBack={() => go(0)}
            onNext={() => go(2)}
          />
        );
      case 2:
        return <LocationPage ctx={ctx} state={state} onChange={update} onBack={() => go(1)} onNext={() => go(3)} />;
      case 3:
        return <ModelPage ctx={ctx} state={state} onChange={update} onBack={() => go(2)} onNext={() => go(4)} />;
      case 4:
        return (
          <ReviewPage
            ctx={ctx}
            state={state}
            onChange={update}
            onBack={() => go(3)}
            onInstall={() => {
              go(5);
              void install.start(state);
            }}
          />
        );
      case 5:
        return <InstallPage install={install} onDone={() => go(6)} />;
      default:
        return (
          <DonePage
            install={install}
            state={state}
            onRetry={() => {
              go(5);
              void install.start(state);
            }}
          />
        );
    }
  })();

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar}>
        <div className={styles.brand}>
          <Logo size={34} />
          <div>
            <div className={styles.brandName}>Demido Studio</div>
            <div className={styles.brandVersion}>Setup · {ctx.version}</div>
          </div>
        </div>
        {sidebar}
      </aside>
      <main className={styles.main}>{page}</main>
    </div>
  );
}
