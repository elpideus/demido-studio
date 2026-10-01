// The search model's card in Settings, General.

import { describe, expect, it } from 'vitest';

import type { DownloadJob, SearchStatus } from '@/lib/types';
import { searchDownload, searchView } from './searchView';

const GEMMA = {
  id: 'embeddinggemma-300m',
  name: 'EmbeddingGemma 300M',
  repo: 'unsloth/embeddinggemma-300m-GGUF',
  size: 328_577_056,
};

function status(over: Partial<SearchStatus> = {}): SearchStatus {
  return { model: GEMMA, suggested: GEMMA, indexed: 0, total: 0, error: null, ...over };
}

function job(over: Partial<DownloadJob> = {}): DownloadJob {
  return {
    id: 'j',
    repo: GEMMA.repo,
    name: GEMMA.name,
    quant: 'Q8_0',
    total: 1000,
    downloaded: 250,
    bytesPerSecond: 0,
    state: 'downloading',
    error: null,
    createdAt: 0,
    ...over,
  };
}

describe('searchView', () => {
  it('offers the download when no model is installed', () => {
    const view = searchView(status({ model: null }));
    expect(view.state).toBe('missing');
    expect(view.offerDownload).toBe(true);
    expect(view.meta).toBe('Files are searched by their words only. EmbeddingGemma 300M (329 MB) adds meaning.');
  });

  it('follows the download, and offers it again when it failed', () => {
    const s = status({ model: null });
    const downloading = searchView(s, searchDownload(s, [job()]));
    expect(downloading.state).toBe('downloading');
    expect(downloading.progress).toBe(0.25);
    expect(downloading.offerDownload).toBe(false);
    const failed = searchView(s, job({ state: 'failed', error: 'checksum mismatch' }));
    expect(failed.offerDownload).toBe(true);
    expect(failed.meta).toBe('The download failed: checksum mismatch');
    // Another model's download is not this one.
    expect(searchDownload(s, [job({ repo: 'unsloth/other' })])).toBeUndefined();
  });

  it('shows indexing progress, then ready', () => {
    const indexing = searchView(status({ indexed: 40, total: 160 }));
    expect(indexing.state).toBe('indexing');
    expect(indexing.progress).toBe(0.25);
    expect(indexing.meta).toBe('Indexing: 40 of 160 passages');
    const ready = searchView(status({ indexed: 160, total: 160 }));
    expect(ready.state).toBe('ready');
    expect(ready.meta).toBe('Finds passages by what they mean, in any language · 160 passages indexed');
    expect(searchView(status()).state).toBe('ready');
  });

  it('says why the model could not run', () => {
    const view = searchView(status({ indexed: 1, total: 9, error: 'the search model took too long to load' }));
    expect(view.state).toBe('error');
    expect(view.meta).toBe('the search model took too long to load');
  });
});
