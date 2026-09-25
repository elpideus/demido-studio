import {
  Apple,
  ArrowLeft,
  ArrowRight,
  Cpu,
  Gpu,
  Layers,
  MemoryStick,
  Microchip,
  MonitorCog,
  type LucideIcon,
} from 'lucide-react';
import { Badge, Button, cx, formatBytes } from '@demido/ui';

import type { Backend, Context, WizardState } from '../types';
import styles from '../Setup.module.css';

const INFO: Record<Backend, { title: string; icon: LucideIcon; description: string }> = {
  cuda: {
    title: 'NVIDIA CUDA',
    icon: Gpu,
    description:
      'For NVIDIA GeForce and RTX graphics cards. The fastest choice on NVIDIA hardware: the whole model runs on the card.',
  },
  rocm: {
    title: 'AMD ROCm',
    icon: Microchip,
    description:
      'For recent AMD Radeon cards (RX 6800 and newer). Runs the model on the graphics card with AMD’s compute platform.',
  },
  metal: {
    title: 'Apple Silicon',
    icon: Apple,
    description: 'For Macs with an M-series chip. Uses the chip’s GPU and fast shared memory.',
  },
  vulkan: {
    title: 'Vulkan',
    icon: Layers,
    description:
      'Works with almost any modern graphics card, including Intel Arc and older Radeon cards. Choose it when the options above do not match your card.',
  },
  cpu: {
    title: 'CPU only',
    icon: Cpu,
    description:
      'Runs on the processor alone. It works on every computer, but answers arrive much more slowly. Not recommended when a supported graphics card is available.',
  },
};

interface Props {
  ctx: Context;
  state: WizardState;
  onChange: (backend: Backend) => void;
  onBack: () => void;
  onNext: () => void;
}

export function RuntimePage({ ctx, state, onChange, onBack, onNext }: Props) {
  const hw = ctx.hardware;
  const gpu = hw.gpus.find((g) => !g.integrated) ?? hw.gpus[0];
  return (
    <div className={styles.page}>
      <div className={styles.pageBody}>
        <h1 className={styles.title}>Choose how the AI runs</h1>
        <p className={styles.lead}>We checked your computer and selected the best option. You can pick another one.</p>
        <div className={styles.hardware}>
          {gpu && (
            <span className={styles.chip}>
              <MonitorCog size={14} aria-hidden /> {gpu.name}
              {!gpu.integrated && ` · ${Math.round(gpu.vram / 1024 ** 3)} GB`}
            </span>
          )}
          <span className={styles.chip}>
            <Cpu size={14} aria-hidden /> {hw.cpu.name}
          </span>
          <span className={styles.chip}>
            <MemoryStick size={14} aria-hidden /> {Math.round(hw.totalMemory / 1024 ** 3)} GB memory
          </span>
        </div>
        <div className={styles.options} role="radiogroup" aria-label="AI runtime">
          {ctx.choices
            .filter((c) => c.available || c.backend !== 'metal' || hw.os === 'macos')
            .map((c) => {
              const info = INFO[c.backend];
              const Icon = info.icon;
              const selected = state.backend === c.backend;
              return (
                <button
                  key={c.backend}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  disabled={!c.available}
                  className={cx(styles.option, selected && styles.selected)}
                  onClick={() => onChange(c.backend)}
                >
                  <span className={styles.radio} />
                  <span className={styles.optionIcon}>
                    <Icon size={20} strokeWidth={1.8} aria-hidden />
                  </span>
                  <span className={styles.optionText}>
                    <span className={styles.optionTitle}>
                      {info.title}
                      {c.recommended && <Badge tone="accent">Recommended</Badge>}
                      {c.backend === 'cpu' && c.available && <Badge tone="warning">Slow</Badge>}
                    </span>
                    <span className={styles.optionDesc}>{info.description}</span>
                    <span className={styles.optionNote}>{c.note}</span>
                  </span>
                  {c.available && <span className={styles.optionSide}>{formatBytes(c.downloadSize)}</span>}
                </button>
              );
            })}
        </div>
      </div>
      <div className={styles.footer}>
        <Button variant="ghost" icon={ArrowLeft} onClick={onBack}>
          Back
        </Button>
        <span className={styles.footerInfo} />
        <Button variant="primary" size="lg" iconRight={ArrowRight} onClick={onNext}>
          Next
        </Button>
      </div>
    </div>
  );
}
