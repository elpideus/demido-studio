import { describe, expect, it } from 'vitest';

import type { HfModelFile, MemoryBudget } from '@/lib/types';
import { bestFile, bitsPerWeight, fitFor, isUsual, quantLabels } from './fit';

const GB = 1e9;
const MB = 1024 * 1024;

/** A 12 GB card with the reserve `model_budget` reports for it: about 4.4 GB. */
const card: MemoryBudget = {
  gpu: 'NVIDIA GeForce RTX 4070',
  memory: 12 * GB,
  system: 32 * GB,
  reserve: [
    { label: 'Conversation', bytes: 1024 * MB },
    { label: 'Search', bytes: 1245 * MB },
    { label: 'Speech', bytes: 2068 * MB },
  ],
};

const BITS: Array<[string, number]> = [
  ['IQ2_M', 2.7],
  ['Q3_K_M', 3.91],
  ['Q4_K_M', 4.89],
  ['UD-Q4_K_XL', 5.02],
  ['Q5_K_M', 5.69],
  ['Q6_K', 6.56],
  ['Q8_0', 8.5],
  ['BF16', 16],
];

function repo(parameters: number): HfModelFile[] {
  return BITS.map(([quant, bits]) => {
    const size = Math.round((parameters * bits) / 8);
    return { name: `m-${quant}.gguf`, paths: [`m-${quant}.gguf`], sizes: [size], sha256: [null], size, quant };
  });
}

describe('fitFor', () => {
  it('keeps the reserve free on the GPU before calling a file a full fit', () => {
    expect(fitFor(7 * GB, card).key).toBe('full');
    expect(fitFor(9 * GB, card).key).toBe('tight');
    expect(fitFor(15 * GB, card).key).toBe('partial');
    expect(fitFor(40 * GB, card).key).toBe('large');
  });

  it('speaks of memory, not a GPU, on a computer without one', () => {
    const cpu: MemoryBudget = { ...card, gpu: null, memory: 16 * GB };
    expect(fitFor(4 * GB, cpu).label).toBe('Fits');
    expect(fitFor(20 * GB, cpu).label).toBe('Slow');
  });
});

describe('bestFile', () => {
  it('picks the most faithful quant that leaves the reserve free', () => {
    // 8B: Q6_K is 6.6 GB and leaves 5.4 GB; Q8_0 (8.5 GB) would not leave the reserve.
    expect(bestFile(repo(8.2e9), 8.2e9, 0, card)?.quant).toBe('Q6_K');
    // 12B: Q4_K_M (7.3 GB) leaves 4.7 GB; UD-Q4_K_XL (7.5 GB) would eat into the reserve.
    expect(bestFile(repo(12e9), 12e9, 0, card)?.quant).toBe('Q4_K_M');
  });

  it('counts the projector that comes with the model', () => {
    expect(bestFile(repo(8.2e9), 8.2e9, 1.2 * GB, card)?.quant).toBe('Q5_K_M');
  });

  it('offers Q4_K_M with partial offload rather than going below 4 bits', () => {
    expect(bestFile(repo(30e9), 30e9, 0, card)?.quant).toBe('Q4_K_M');
  });

  it('never offers full precision', () => {
    expect(bestFile(repo(0.5e9), 0.5e9, 0, card)?.quant).toBe('Q8_0');
  });

  it('reads bits from quant names when the parameter count is unknown', () => {
    const files = repo(8.2e9);
    expect(bitsPerWeight(files[2]!, null)).toBe(4.89);
    expect(bestFile(files, null, 0, card)?.quant).toBe('Q6_K');
  });
});

describe('quantLabels', () => {
  it('names files by their quant, and by their own suffix where quants repeat', () => {
    const file = (name: string, quant: string | null): HfModelFile => ({
      name,
      paths: [name],
      sizes: [1],
      sha256: [null],
      size: 1,
      quant,
    });
    const labels = quantLabels([
      file('SmolLM2-135M-Instruct-Q4_0.gguf', 'Q4_0'),
      file('SmolLM2-135M-Instruct-Q4_0_4_4.gguf', 'Q4_0'),
      file('SmolLM2-135M-Instruct-Q4_K_M.gguf', 'Q4_K_M'),
      file('SmolLM2-135M-Instruct-f16.gguf', null),
    ]);
    expect([...labels.values()]).toEqual(['Q4_0', 'Q4_0_4_4', 'Q4_K_M', 'f16']);
  });
});

describe('isUsual', () => {
  it('hides the extreme and full-precision quants', () => {
    const files = repo(8e9);
    expect(files.filter((f) => isUsual(f, 8e9)).map((f) => f.quant)).toEqual([
      'Q3_K_M',
      'Q4_K_M',
      'UD-Q4_K_XL',
      'Q5_K_M',
      'Q6_K',
      'Q8_0',
    ]);
  });
});
