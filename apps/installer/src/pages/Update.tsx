import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, Check, ChevronDown, ChevronUp, CircleAlert, RotateCcw, ShieldCheck, X } from 'lucide-react';
import { Button, Checkbox, Logo, Notice, ProgressBar, Spinner, cx } from '@demido/ui';

import type { Context, DirInfo, ExistingInstall } from '../types';
import { StepRow, overallProgress, type InstallModel } from './Install';
import { checksFirst, failureActions, installedAfter, type FailureAction } from './updateOutcome';
import setup from '../Setup.module.css';
import styles from './Update.module.css';

/** Where the flow is before the engine runs: checking the folder, waiting on the person to let
 * setup close the app, closing it, or stuck because it would not close. */
type Phase = 'checking' | 'open' | 'closing' | 'stuck' | 'running';

interface Props {
  ctx: Context;
  existing: ExistingInstall;
  dir: string;
  /** Started by the app to install an update it downloaded: nothing to ask, and the app starts
   * again at the end. Otherwise the person started it, and setup asks before closing the app. */
  auto: boolean;
  install: InstallModel;
  /** Back to the wizard; without it, leaving closes setup. */
  onBack?: () => void;
}

/** Updates the installation in `dir` to this setup's version, keeping every choice made when it
 * was installed. Starts at once. */
export function UpdateFlow({ ctx, existing, dir, auto, install, onBack }: Props) {
  const [phase, setPhase] = useState<Phase>(auto ? 'running' : 'checking');
  const [showDetails, setShowDetails] = useState(false);
  const [run, setRun] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const started = useRef(false);
  const logRef = useRef<HTMLPreElement>(null);
  const { update } = install;

  const begin = useCallback(
    async (retry = false) => {
      setError(null);
      if (checksFirst(auto, retry)) {
        setPhase('checking');
        const info = await invoke<DirInfo>('check_dir', { path: dir }).catch(() => null);
        if (info?.running) {
          setPhase('open');
          return;
        }
      }
      setPhase('running');
      await update(dir);
    },
    [auto, dir, update],
  );

  useEffect(() => {
    // Once, even when React runs effects twice in development.
    if (started.current) return;
    started.current = true;
    void begin();
  }, [begin]);

  const closeAndUpdate = async () => {
    setPhase('closing');
    const closed = await invoke<boolean>('close_app', { dir }).catch(() => false);
    if (!closed) {
      setPhase('stuck');
      return;
    }
    setPhase('running');
    await update(dir);
  };

  const failed = install.steps.filter((s) => s.state === 'failed');
  const critical = failed.find((s) => s.id === 'app' || s.id === 'finalize');
  const finished = phase === 'running' && install.finished !== null;
  const appFailed = finished && (!!critical || !!install.fatal);
  const success = finished && !appFailed && (install.finished?.success ?? false);
  const installed = installedAfter(install.steps);

  /** Opens the app and closes setup. `skipUpdate` keeps the app from starting the same update
   * again at this launch. */
  const launch = useCallback(
    async (skipUpdate = false) => {
      try {
        await invoke('launch_app', { installDir: dir, skipUpdate });
      } catch (e) {
        setError(String(e));
      }
    },
    [dir],
  );

  useEffect(() => {
    if (!(auto && success)) return;
    // Long enough to read that the update worked.
    const t = window.setTimeout(() => void launch(), 700);
    return () => window.clearTimeout(t);
  }, [auto, success, launch]);

  useEffect(() => {
    if (showDetails) logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [install.log, showDetails]);

  const leave = onBack ?? (() => void invoke('quit'));
  const versions = `${existing.version} → ${ctx.version}`;
  const finish = () => void (run ? launch() : invoke('quit'));
  const errorNotice = error && <Notice tone="danger">{error}</Notice>;
  const runBox = <Checkbox checked={run} onChange={setRun} label="Run Demido Studio" />;
  const retry = (
    <Button variant="secondary" icon={RotateCcw} onClick={() => void begin(true)}>
      Retry
    </Button>
  );

  if (phase !== 'running') {
    const closing = phase === 'closing';
    return (
      <Layout
        icon={<Mark />}
        title="Update Demido Studio"
        lead={versions}
        footer={
          <>
            <span className={setup.footerInfo}>Your chats, models and settings are kept.</span>
            <Button variant="ghost" onClick={leave} disabled={closing}>
              Cancel
            </Button>
            {phase === 'stuck' ? (
              <Button variant="primary" icon={RotateCcw} onClick={() => void closeAndUpdate()}>
                Try again
              </Button>
            ) : (
              phase !== 'checking' && (
                <Button variant="primary" loading={closing} onClick={() => void closeAndUpdate()}>
                  Close and update
                </Button>
              )
            )}
          </>
        }
      >
        {phase === 'checking' && (
          <div className={styles.waiting}>
            <Spinner size={14} /> Checking the installation…
          </div>
        )}
        {(phase === 'open' || closing) && (
          <Notice tone="warning">Demido Studio is open. Setup closes it to update.</Notice>
        )}
        {phase === 'stuck' && (
          <Notice tone="danger">Demido Studio could not be closed. Close it yourself, then try again.</Notice>
        )}
      </Layout>
    );
  }

  if (!finished) {
    const progress = overallProgress(install.steps);
    const running = install.steps.find((s) => s.state === 'running');
    return (
      <Layout
        icon={<Mark />}
        title="Updating Demido Studio"
        lead={versions}
        footer={
          <>
            <span className={setup.footerInfo}>Your chats, models and settings are kept.</span>
            {!auto && (
              <Button variant="ghost" onClick={install.cancel}>
                Cancel
              </Button>
            )}
          </>
        }
      >
        <div className={styles.progress}>
          <div className={styles.progressTop}>
            <span className={styles.step}>{running?.label ?? 'Preparing…'}</span>
            {progress !== null && <span className={styles.percent}>{Math.round(progress * 100)}%</span>}
          </div>
          <ProgressBar value={progress} size="md" />
          <div className={styles.activity}>{running?.activity || install.log.at(-1) || ''}</div>
        </div>
        <Button
          variant="ghost"
          size="sm"
          iconRight={showDetails ? ChevronUp : ChevronDown}
          className={styles.details}
          onClick={() => setShowDetails(!showDetails)}
        >
          {showDetails ? 'Hide details' : 'Show details'}
        </Button>
        {showDetails && (
          <>
            <div className={setup.jobList}>
              {install.steps.map((s) => (
                <StepRow key={s.id} step={s} />
              ))}
            </div>
            <pre ref={logRef} className={setup.log}>
              {install.log.join('\n') || 'Nothing yet.'}
            </pre>
          </>
        )}
      </Layout>
    );
  }

  if (success) {
    return auto ? (
      <Layout
        icon={<Status tone="done" />}
        title="Demido Studio is up to date"
        lead={`Starting Demido Studio ${ctx.version}…`}
        footer={
          <>
            <span className={setup.footerInfo} />
            {error && (
              <Button variant="primary" onClick={() => void invoke('quit')}>
                Close
              </Button>
            )}
          </>
        }
      >
        {errorNotice}
      </Layout>
    ) : (
      <Layout
        icon={<Status tone="done" />}
        title="Demido Studio is updated"
        lead={`Version ${ctx.version} is installed. Your chats, models and settings are kept.`}
        footer={
          <>
            <span className={setup.footerInfo} />
            <Button variant="primary" onClick={finish}>
              Finish
            </Button>
          </>
        }
      >
        {runBox}
        {errorNotice}
      </Layout>
    );
  }

  if (!appFailed) {
    return (
      <Layout
        icon={<Status tone="warn" />}
        title="Updated, with a few problems"
        lead="Demido Studio works, but some parts could not be updated. Retry them now, or later by running Setup again."
        footer={
          <>
            {retry}
            <span className={setup.footerInfo} />
            {auto ? (
              <Button variant="primary" onClick={() => void launch()}>
                Open Demido Studio
              </Button>
            ) : (
              <Button variant="primary" onClick={finish}>
                Finish
              </Button>
            )}
          </>
        }
      >
        <div className={setup.summary}>
          {failed.map((s) => (
            <div key={s.id} className={setup.summaryRow}>
              <X size={15} className={setup.jobFailed} />
              <div className={setup.summaryLabel}>
                <div>{s.label}</div>
                <div className={setup.summaryDetail}>{s.message}</div>
              </div>
            </div>
          ))}
        </div>
        {!auto && runBox}
        {errorNotice}
      </Layout>
    );
  }

  const actions = failureActions(auto, installed);
  const action = (name: FailureAction, primary: boolean) => {
    switch (name) {
      case 'retry':
        return (
          <Button
            key={name}
            variant={primary ? 'primary' : 'secondary'}
            icon={RotateCcw}
            onClick={() => void begin(true)}
          >
            Retry
          </Button>
        );
      case 'open':
        // Without `skipUpdate`, the app would start this same update again instead of opening.
        return (
          <Button key={name} variant={primary ? 'primary' : 'secondary'} onClick={() => void launch(true)}>
            Open Demido Studio
          </Button>
        );
      case 'close':
        return (
          <Button key={name} variant={primary ? 'primary' : 'ghost'} onClick={() => void invoke('quit')}>
            Close
          </Button>
        );
    }
  };
  return (
    <Layout
      icon={<Status tone="danger" />}
      title="The update did not finish"
      lead={critical?.message ?? install.fatal ?? 'The new version could not be installed.'}
      footer={
        <>
          {actions.others.map((name) => action(name, false))}
          <span className={setup.footerInfo} />
          {action(actions.primary, true)}
        </>
      }
    >
      {installed === 'previous' && (
        <div className={styles.kept}>
          <ShieldCheck size={15} aria-hidden /> Your previous version is still installed.
        </div>
      )}
      {installed === 'mixed' && <Notice tone="danger">The update is half done. Retry finishes it.</Notice>}
      {errorNotice}
    </Layout>
  );
}

/** `--update` for a folder that holds no installation setup can act on. */
export function UpdateMissing({ dir }: { dir: string }) {
  return (
    <Layout
      icon={<Status tone="danger" />}
      title="Nothing to update"
      lead={`No Demido Studio installation was found in ${dir}.`}
      footer={
        <>
          <span className={setup.footerInfo} />
          <Button variant="primary" onClick={() => void invoke('quit')}>
            Close
          </Button>
        </>
      }
    />
  );
}

function Layout({
  icon,
  title,
  lead,
  footer,
  children,
}: {
  icon: ReactNode;
  title: string;
  lead: ReactNode;
  footer: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className={styles.flow}>
      <div className={styles.body}>
        <div className={styles.content}>
          <div className={styles.head}>
            {icon}
            <div className={styles.headText}>
              <h1 className={styles.title}>{title}</h1>
              <p className={styles.lead}>{lead}</p>
            </div>
          </div>
          {children}
        </div>
      </div>
      <div className={cx(setup.footer, styles.footer)}>{footer}</div>
    </div>
  );
}

function Mark() {
  return (
    <span className={cx(styles.icon, styles.mark)}>
      <Logo size={26} />
    </span>
  );
}

function Status({ tone }: { tone: 'done' | 'warn' | 'danger' }) {
  const Icon = tone === 'done' ? Check : tone === 'warn' ? AlertTriangle : CircleAlert;
  return (
    <span className={cx(styles.icon, styles[tone])}>
      <Icon size={22} strokeWidth={tone === 'done' ? 2.6 : 2} aria-hidden />
    </span>
  );
}
