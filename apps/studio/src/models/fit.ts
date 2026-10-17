// How a model file fits this computer, and which quantization of a repository to offer first.

import type { HfModelFile, MemoryBudget } from '@/lib/types';

export type FitKey = 'full' | 'tight' | 'partial' | 'large';

export interface Fit {
  key: FitKey;
  label: string;
  tone: 'accent' | 'warning' | 'info' | 'danger';
  /** One sentence on what it means for this file. */
  text: string;
}

/** GPU memory llama.cpp needs beside the weights before any conversation fits at all. */
const MIN_CONTEXT = 512 * 1024 * 1024;

/** Share of system memory a model spilling out of the GPU may take before the computer struggles. */
const SYSTEM_SHARE = 0.6;

/** Bits per weight of the usual quantizations, for a repository whose parameter count is unknown. */
const QUANT_BITS: Record<string, number> = {
  IQ1_S: 1.56,
  IQ1_M: 1.75,
  IQ2_XXS: 2.06,
  IQ2_XS: 2.31,
  IQ2_S: 2.5,
  IQ2_M: 2.7,
  Q2_K: 2.96,
  Q2_K_L: 3.0,
  Q2_K_XL: 3.1,
  IQ3_XXS: 3.06,
  IQ3_XS: 3.3,
  IQ3_S: 3.44,
  IQ3_M: 3.66,
  Q3_K_S: 3.5,
  Q3_K_M: 3.91,
  Q3_K_L: 4.27,
  Q3_K_XL: 4.3,
  IQ4_XS: 4.25,
  IQ4_NL: 4.5,
  Q4_0: 4.55,
  Q4_1: 5.0,
  Q4_K_S: 4.58,
  Q4_K_M: 4.89,
  Q4_K_L: 5.0,
  Q4_K_XL: 5.0,
  MXFP4: 4.25,
  MXFP4_MOE: 4.25,
  Q5_0: 5.5,
  Q5_1: 6.0,
  Q5_K_S: 5.54,
  Q5_K_M: 5.69,
  Q5_K_L: 5.8,
  Q5_K_XL: 5.9,
  Q6_K: 6.56,
  Q6_K_L: 6.7,
  Q6_K_XL: 6.8,
  Q8_0: 8.5,
  Q8_K_XL: 9.0,
  BF16: 16,
  F16: 16,
  F32: 32,
};

/** Below this, quality drops faster than speed rises; above it, a model is larger than it needs to be. */
export const USUAL_BITS = { min: 3.5, max: 8.6 } as const;
/** What the best fit is chosen from: 4 to 8 bits. */
const BEST_BITS = { min: 4.2, max: 8.6 } as const;

export function reserveBytes(budget: MemoryBudget): number {
  return budget.reserve.reduce((sum, r) => sum + r.bytes, 0);
}

/** Bits per weight of a file: from the repository's parameter count, else from its quant's name. */
export function bitsPerWeight(file: HfModelFile, parameters: number | null): number | null {
  if (parameters && parameters > 0) return (file.size * 8) / parameters;
  const label = file.quant?.replace(/^UD-/, '').toUpperCase();
  return label ? (QUANT_BITS[label] ?? null) : null;
}

/** How `bytes` of weights (with the projector, if it comes along) run on this computer. */
export function fitFor(bytes: number, budget: MemoryBudget): Fit {
  const gpu = budget.gpu !== null;
  const free = budget.memory - bytes;
  const reserve = reserveBytes(budget);
  if (free >= reserve) {
    return {
      key: 'full',
      label: gpu ? 'Full GPU' : 'Fits',
      tone: 'accent',
      text: gpu
        ? 'Runs entirely on the GPU, with room for the conversation, search and speech beside it.'
        : 'Fits in memory with room for the conversation, search and speech beside it.',
    };
  }
  if (free >= MIN_CONTEXT) {
    return {
      key: 'tight',
      label: 'Tight',
      tone: 'warning',
      text: gpu
        ? 'Fits on the GPU, but takes the room kept for search, speech and long conversations, which then run more slowly.'
        : 'Fits in memory, but leaves little room for search, speech and long conversations.',
    };
  }
  const ceiling = gpu ? budget.memory + budget.system * SYSTEM_SHARE : budget.system * SYSTEM_SHARE * 1.25;
  if (bytes <= ceiling) {
    return {
      key: 'partial',
      label: gpu ? 'Partial offload' : 'Slow',
      tone: 'info',
      text: gpu
        ? 'Larger than the GPU: part of it runs from system memory on the CPU, so it answers more slowly.'
        : 'Takes most of the memory, so it answers slowly and other programs may struggle.',
    };
  }
  return {
    key: 'large',
    label: 'Too large',
    tone: 'danger',
    text: 'Larger than this computer’s memory. It would not load.',
  };
}

/** Whether a file is one of the quantizations most people pick from (the rest are behind "Show more"). */
export function isUsual(file: HfModelFile, parameters: number | null): boolean {
  const bits = bitsPerWeight(file, parameters);
  return bits === null || (bits >= USUAL_BITS.min && bits <= USUAL_BITS.max);
}

const FULL_PRECISION = /(^|-)(BF16|F16|F32)$/i;

/**
 * The quantization to offer first: the most faithful one between 4 and 8 bits that runs entirely
 * on the GPU with the reserve left free. When none does, Q4_K_M (or the nearest 4-bit one), which
 * then runs partly from system memory: below 4 bits a model loses more than it gains in speed.
 * Of two the same size, unsloth's dynamic quants win.
 */
export function bestFile(
  files: HfModelFile[],
  parameters: number | null,
  projector: number,
  budget: MemoryBudget,
): HfModelFile | undefined {
  const sized = files
    .filter((f) => !FULL_PRECISION.test(f.quant ?? ''))
    .map((f) => ({ f, bits: bitsPerWeight(f, parameters) }));
  const usable = sized.filter(({ bits }) => bits !== null && bits >= BEST_BITS.min && bits <= BEST_BITS.max);
  const fitting = usable.filter(({ f }) => fitFor(f.size + projector, budget).key === 'full');
  const rank = (a: { f: HfModelFile }, b: { f: HfModelFile }) =>
    a.f.size - b.f.size || Number(isDynamic(a.f)) - Number(isDynamic(b.f));
  if (fitting.length) return fitting.sort(rank).at(-1)!.f;
  const q4 = usable.find(({ f }) => f.quant?.replace(/^UD-/, '') === 'Q4_K_M');
  if (q4) return q4.f;
  if (usable.length) return usable.sort(rank)[0]!.f;
  // Nothing says how many bits its files are: the largest that fits, else the smallest.
  const all = sized.map(({ f }) => f).sort((a, b) => a.size - b.size);
  return all.filter((f) => fitFor(f.size + projector, budget).key === 'full').at(-1) ?? all[0] ?? files[0];
}

function isDynamic(file: HfModelFile): boolean {
  return file.quant?.startsWith('UD-') ?? false;
}

/**
 * What each file is called in the dropdown: its quant, or, when two files share one (Q4_0_4_4 and
 * Q4_0_8_8 both read as Q4_0), the part of its name that tells it apart from the others.
 */
export function quantLabels(files: HfModelFile[]): Map<string, string> {
  const stem = (f: HfModelFile) => f.name.replace(/\.gguf$/i, '');
  const stems = files.map(stem);
  let prefix = stems[0] ?? '';
  for (const s of stems) while (!s.startsWith(prefix)) prefix = prefix.slice(0, -1);
  // Cut back to a separator, so `Q4_0_4_4` is not left as `4_4`.
  const cut = Math.max(prefix.lastIndexOf('-'), prefix.lastIndexOf('.')) + 1;
  const counts = new Map<string, number>();
  for (const f of files) if (f.quant) counts.set(f.quant, (counts.get(f.quant) ?? 0) + 1);
  return new Map(
    files.map((f) => {
      const own = stem(f).slice(files.length > 1 ? cut : 0) || stem(f);
      return [f.name, f.quant && counts.get(f.quant) === 1 ? f.quant : own];
    }),
  );
}
