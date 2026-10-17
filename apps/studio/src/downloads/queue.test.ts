import { describe, expect, it } from 'vitest';

import type { DownloadJob, JobState } from '@/lib/types';
import { beforeFor, formatEta, queuePosition, shortName, summarize } from './queue';

function job(id: string, state: JobState, downloaded = 0, total = 100, bytesPerSecond = 0): DownloadJob {
  return {
    id,
    repo: 'unsloth/Qwen3-8B-GGUF',
    name: `${id}.gguf`,
    quant: 'Q4_K_M',
    total,
    downloaded,
    bytesPerSecond,
    state,
    error: null,
    createdAt: 0,
    files: 1,
    projector: false,
  };
}

describe('summarize', () => {
  it('counts what runs, waits and is done, and how long the rest takes', () => {
    const s = summarize([
      job('a', 'downloading', 40, 100, 10),
      job('b', 'queued', 0, 100),
      job('c', 'paused', 50, 100),
      job('d', 'failed', 10, 100),
      job('e', 'done', 100, 100),
    ]);
    expect(s.running.map((j) => j.id)).toEqual(['a']);
    expect(s.waiting.map((j) => j.id)).toEqual(['b', 'c']);
    expect([s.queued, s.paused, s.failed.length, s.done.length]).toEqual([1, 1, 1, 1]);
    expect([s.downloaded, s.total]).toEqual([100, 400]);
    // 60 left of a and 100 of b at 10 per second; c and d wait for the person.
    expect(s.eta).toBe(16);
  });

  it('has no estimate when nothing moves', () => {
    expect(summarize([job('a', 'paused', 1)]).eta).toBeNull();
  });
});

describe('the queue order', () => {
  const jobs = [job('run', 'downloading'), job('a', 'queued'), job('p', 'paused'), job('b', 'queued')];
  const waiting = jobs.filter((j) => j.state === 'queued' || j.state === 'paused');

  it('numbers the queued jobs, skipping paused ones', () => {
    expect(queuePosition(jobs, 'a')).toBe(1);
    expect(queuePosition(jobs, 'b')).toBe(2);
    expect(queuePosition(jobs, 'p')).toBeNull();
  });

  it('turns a place in the list into the job to go before', () => {
    expect(beforeFor(waiting, 'b', 0)).toBe('a');
    expect(beforeFor(waiting, 'a', 1)).toBe('b');
    expect(beforeFor(waiting, 'a', 2)).toBeNull();
  });
});

describe('words', () => {
  it('says how long is left the way people say it', () => {
    expect(formatEta(12.2)).toBe('13 s left');
    expect(formatEta(130)).toBe('3 min left');
    expect(formatEta(3600 + 1200)).toBe('1 h 20 min left');
    expect(formatEta(null)).toBe('');
  });

  it('names a repository without its author or format', () => {
    expect(shortName('unsloth/Qwen3-8B-GGUF')).toBe('Qwen3-8B');
    expect(shortName('bartowski/google_gemma-3-12b-it-GGUF')).toBe('google_gemma-3-12b-it');
  });
});
