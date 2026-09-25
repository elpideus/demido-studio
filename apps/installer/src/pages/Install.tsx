import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AlertTriangle, Check, ChevronDown, Circle, CircleSlash, PartyPopper, RotateCcw, X } from 'lucide-react';
import { Button, Checkbox, Notice, ProgressBar, Spinner, cx, formatBytes, formatDuration } from '@demido/ui';

import type { ProvisionEvent, StepId, StepInfo, StepState, WizardState } from '../types';
import styles from '../Setup.module.css';

interface StepView extends StepInfo {
  state: StepState;
  done: number;
  total: number | null;
  speed: number;
  activity: string;
  message?: string;
}

export interface InstallModel {
  steps: StepView[];
  finished: { success: boolean; failed: StepId[] } | null;
  fatal: string | null;
  log: string[];
  installDir: string | null;
  start: (state: WizardState) => Promise<void>;
  cancel: () => void;
}

/** Follows the installation engine's events. */
export function useInstall(): InstallModel {
  const [steps, setSteps] = useState<StepView[]>([]);
  const [finished, setFinished] = useState<InstallModel['finished']>(null);
  const [fatal, setFatal] = useState<string | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const [installDir, setInstallDir] = useState<string | null>(null);

  useEffect(() => {
    const off = listen<ProvisionEvent>('setup://event', ({ payload: e }) => {
      switch (e.type) {
        case 'plan':
          setSteps(
            e.steps.map((s) => ({ ...s, state: 'pending', done: 0, total: s.size || null, speed: 0, activity: '' })),
          );
          break;
        case 'step':
          setSteps((list) => list.map((s) => (s.id === e.id ? { ...s, state: e.state, message: e.message } : s)));
          break;
        case 'progress':
          setSteps((list) =>
            list.map((s) =>
              s.id === e.id ? { ...s, done: e.done, total: e.total, speed: e.bytesPerSecond, activity: e.activity } : s,
            ),
          );
          break;
        case 'log':
          setLog((l) => [...l.slice(-400), e.line]);
          break;
        case 'finished':
          setFinished({ success: e.success, failed: e.failed });
          break;
      }
    });
    const offFatal = listen<string>('setup://fatal', ({ payload }) => {
      setFatal(payload);
      // A failure before the engine got going never sends `finished`; end the run here so the
      // wizard moves on to the finish page, which shows the error.
      setFinished((f) => f ?? { success: false, failed: [] });
    });
    return () => {
      void off.then((f) => f());
      void offFatal.then((f) => f());
    };
  }, []);

  const start = useCallback(async (state: WizardState) => {
    setFinished(null);
    setFatal(null);
    setLog([]);
    setSteps([]);
    setInstallDir(state.installDir);
    try {
      await invoke('start_install', {
        request: {
          scope: state.scope,
          installDir: state.installDir,
          backend: state.backend,
          family: state.family,
          shortcuts: state.shortcuts,
        },
      });
    } catch (e) {
      setFatal(String(e));
      setFinished({ success: false, failed: [] });
    }
  }, []);

  return { steps, finished, fatal, log, installDir, start, cancel: () => void invoke('cancel_install') };
}

function StepRow({ step }: { step: StepView }) {
  const active = step.state === 'running';
  const fraction = step.total && step.total > 1 ? Math.min(1, step.done / step.total) : null;
  const remaining = step.speed > 0 && step.total ? (step.total - step.done) / step.speed : null;
  return (
    <div className={cx(styles.job, active && styles.jobActive)}>
      <div className={styles.jobHead}>
        <span
          className={cx(
            styles.jobIcon,
            step.state === 'done' && styles.jobDone,
            step.state === 'failed' && styles.jobFailed,
          )}
        >
          {step.state === 'running' && <Spinner size={14} />}
          {step.state === 'done' && <Check size={16} strokeWidth={2.6} />}
          {step.state === 'failed' && <X size={16} strokeWidth={2.6} />}
          {step.state === 'skipped' && <CircleSlash size={15} />}
          {step.state === 'pending' && <Circle size={14} />}
        </span>
        <span className={styles.jobLabel}>{step.label}</span>
        {step.size > 0 && <span className={styles.summarySize}>{formatBytes(step.size)}</span>}
      </div>
      {active && (
        <>
          <ProgressBar value={fraction} size="xs" />
          <div className={styles.jobMeta}>
            {step.activity}
            {step.speed > 0 && ` · ${formatBytes(step.speed)}/s`}
            {remaining !== null && remaining > 1 && ` · ${formatDuration(remaining)} left`}
          </div>
        </>
      )}
      {step.state === 'failed' && step.message && <div className={styles.jobError}>{step.message}</div>}
      {step.state === 'skipped' && step.message && <div className={styles.jobMeta}>{step.message}</div>}
    </div>
  );
}

export function InstallPage({ install, onDone }: { install: InstallModel; onDone: () => void }) {
  const [showLog, setShowLog] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);
  const total = install.steps.reduce((sum, s) => sum + Math.max(s.size, 1), 0);
  const done = install.steps.reduce((sum, s) => {
    const weight = Math.max(s.size, 1);
    if (s.state === 'done' || s.state === 'skipped' || s.state === 'failed') return sum + weight;
    if (s.state === 'running' && s.total) return sum + weight * Math.min(1, s.done / s.total);
    return sum;
  }, 0);

  useEffect(() => {
    if (install.finished) onDone();
  }, [install.finished, onDone]);

  useEffect(() => {
    if (showLog) logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [install.log, showLog]);

  return (
    <div className={styles.page}>
      <div className={styles.pageBody}>
        <h1 className={styles.title}>Installing</h1>
        <p className={styles.lead}>
          This takes a few minutes, mostly for the model download. You can keep using your computer.
        </p>
        <div className={styles.overall}>
          <div className={styles.overallTop}>
            <span>{install.steps.length ? `${Math.round((done / total) * 100)}%` : 'Preparing…'}</span>
            <span>{install.steps.find((s) => s.state === 'running')?.label ?? ''}</span>
          </div>
          <ProgressBar value={install.steps.length ? done / total : null} size="md" />
        </div>
        <div className={styles.jobList}>
          {install.steps.map((s) => (
            <StepRow key={s.id} step={s} />
          ))}
        </div>
        <Button
          variant="ghost"
          size="sm"
          iconRight={ChevronDown}
          className={styles.spacerTop}
          onClick={() => setShowLog(!showLog)}
        >
          {showLog ? 'Hide details' : 'Show details'}
        </Button>
        {showLog && (
          <pre ref={logRef} className={styles.log}>
            {install.log.join('\n') || 'Nothing yet.'}
          </pre>
        )}
      </div>
      <div className={styles.footer}>
        <span className={styles.footerInfo}>
          Partial downloads are kept, so a cancelled install resumes where it stopped.
        </span>
        <Button variant="ghost" onClick={install.cancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

export function DonePage({
  install,
  state,
  onRetry,
}: {
  install: InstallModel;
  state: WizardState;
  onRetry: () => void;
}) {
  const [run, setRun] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const failed = install.steps.filter((s) => s.state === 'failed');
  const appFailed = failed.some((s) => s.id === 'app' || s.id === 'finalize') || !!install.fatal;
  const success = install.finished?.success ?? false;

  const finish = async () => {
    if (run && !appFailed) {
      try {
        await invoke('launch_app', { installDir: state.installDir });
        return;
      } catch (e) {
        setError(String(e));
        return;
      }
    }
    await invoke('quit');
  };

  return (
    <div className={styles.page}>
      <div className={styles.pageBody}>
        <div className={styles.hero}>
          <span className={styles.heroIcon}>{success ? <PartyPopper size={28} /> : <AlertTriangle size={28} />}</span>
          <h1 className={styles.title}>
            {success ? 'Demido Studio is ready' : appFailed ? 'Setup did not finish' : 'Installed, with a few problems'}
          </h1>
          <p className={styles.lead}>
            {success
              ? 'Everything was downloaded and checked. Your first model is waiting for you.'
              : appFailed
                ? (install.fatal ?? 'The app itself could not be installed.')
                : 'Demido Studio works, but some parts are missing. Retry them now, or later by running setup again.'}
          </p>
        </div>
        {failed.length > 0 && (
          <div className={styles.summary}>
            {failed.map((s) => (
              <div key={s.id} className={styles.summaryRow}>
                <X size={15} className={styles.jobFailed} />
                <div className={styles.summaryLabel}>
                  <div>{s.label}</div>
                  <div className={styles.summaryDetail}>{s.message}</div>
                </div>
              </div>
            ))}
          </div>
        )}
        {!appFailed && (
          <Checkbox className={styles.spacerTop} checked={run} onChange={setRun} label="Run Demido Studio" />
        )}
        {error && (
          <Notice tone="danger" className={styles.spacerTop}>
            {error}
          </Notice>
        )}
      </div>
      <div className={styles.footer}>
        {failed.length > 0 && (
          <Button variant="secondary" icon={RotateCcw} onClick={onRetry}>
            Retry
          </Button>
        )}
        <span className={styles.footerInfo} />
        <Button variant="primary" size="lg" onClick={() => void finish()}>
          {appFailed ? 'Close' : 'Finish'}
        </Button>
      </div>
    </div>
  );
}
