import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, Download } from 'lucide-react';
import { Button, Checkbox, Notice, formatBytes } from '@demido/ui';

import { downloadSize, type Context, type DirInfo, type VolumeNeed, type WizardState } from '../types';
import styles from '../Setup.module.css';

const RUNTIME_LABEL: Record<string, string> = {
  cuda: 'NVIDIA CUDA',
  rocm: 'AMD ROCm',
  metal: 'Apple Silicon (Metal)',
  vulkan: 'Vulkan',
  cpu: 'CPU',
};

interface Props {
  ctx: Context;
  state: WizardState;
  onChange: (patch: Partial<WizardState>) => void;
  onBack: () => void;
  onInstall: () => void;
}

export function ReviewPage({ ctx, state, onChange, onBack, onInstall }: Props) {
  const [info, setInfo] = useState<DirInfo | null>(null);
  const choice = ctx.choices.find((c) => c.backend === state.backend);
  const pick = ctx.recommendations[state.backend]?.picks.find(([f]) => f === state.family)?.[1];

  const check = () => void invoke<DirInfo>('check_dir', { path: state.installDir }).then(setInfo);
  useEffect(check, [state.installDir]);

  // `included` parts travel inside Setup; everything else is downloaded.
  const items: Array<{ label: string; detail: string; size: number; included?: boolean }> = [
    ...(ctx.hasPayload
      ? [
          {
            label: 'Demido Studio',
            detail: `Version ${ctx.version} · included in Setup`,
            size: ctx.fixedSizes.app ?? 0,
            included: true,
          },
        ]
      : []),
    { label: `AI runtime (${RUNTIME_LABEL[state.backend]})`, detail: 'llama.cpp', size: choice?.downloadSize ?? 0 },
    {
      label: 'Python with data packages',
      detail: 'numpy, pandas, matplotlib, requests',
      size: (ctx.fixedSizes.python ?? 0) + (ctx.fixedSizes.uv ?? 0),
    },
    { label: 'Node.js', detail: 'Runs the market data service', size: ctx.fixedSizes.node ?? 0 },
    ...(pick ? [{ label: pick.name, detail: `${pick.quant} · ${pick.repo}`, size: downloadSize(pick) }] : []),
  ];
  const download = items.reduce((sum, i) => sum + (i.included ? 0 : i.size), 0);
  const needed = items.reduce((sum, i) => sum + i.size, 0);
  // The model goes to the starter models folder, often on another drive than the app.
  const modelBytes = pick ? downloadSize(pick) : 0;
  const [volumes, setVolumes] = useState<VolumeNeed[]>([]);
  useEffect(() => {
    void invoke<VolumeNeed[]>('check_space', {
      scope: state.scope,
      installDir: state.installDir,
      installBytes: needed - modelBytes,
      modelBytes,
    }).then(setVolumes);
  }, [state.scope, state.installDir, needed, modelBytes]);
  const short = volumes.filter((v) => v.freeBytes < v.neededBytes * 1.2);
  const problem = info?.problem ?? null;

  return (
    <div className={styles.page}>
      <div className={styles.pageBody}>
        <h1 className={styles.title}>Ready to install</h1>
        <p className={styles.lead}>
          Setup downloads each part from its official source and checks it before installing it in{' '}
          <strong>{state.installDir}</strong>.
        </p>
        <div className={styles.summary}>
          {items.map((i) => (
            <div key={i.label} className={styles.summaryRow}>
              <div className={styles.summaryLabel}>
                <div>{i.label}</div>
                <div className={styles.summaryDetail}>{i.detail}</div>
              </div>
              <span className={styles.summarySize}>{formatBytes(i.size)}</span>
            </div>
          ))}
          <div className={`${styles.summaryRow} ${styles.total}`}>
            <div className={styles.summaryLabel}>Total download</div>
            <span className={styles.summarySize}>{formatBytes(download)}</span>
          </div>
        </div>
        <Checkbox
          className={styles.spacerTop}
          checked={state.shortcuts}
          onChange={(v) => onChange({ shortcuts: v })}
          label="Create Start menu and desktop shortcuts"
        />
        {info?.running && (
          <Notice
            tone="warning"
            className={styles.spacerTop}
            action={
              <Button size="sm" onClick={check}>
                Check again
              </Button>
            }
          >
            Demido Studio is running from this folder. Close it to continue.
          </Notice>
        )}
        {problem && (
          <Notice tone="danger" className={styles.spacerTop}>
            {problem}
          </Notice>
        )}
        {short.map((v) => (
          <Notice key={v.mount} tone="danger" className={styles.spacerTop}>
            There may not be enough free space on {v.mount} ({formatBytes(v.freeBytes)} free,{' '}
            {formatBytes(v.neededBytes)} needed).
          </Notice>
        ))}
      </div>
      <div className={styles.footer}>
        <Button variant="ghost" icon={ArrowLeft} onClick={onBack}>
          Back
        </Button>
        <span className={styles.footerInfo}>Downloads can resume if the connection drops.</span>
        <Button
          variant="primary"
          size="lg"
          icon={Download}
          disabled={!!info?.running || problem !== null}
          onClick={onInstall}
        >
          Install
        </Button>
      </div>
    </div>
  );
}
