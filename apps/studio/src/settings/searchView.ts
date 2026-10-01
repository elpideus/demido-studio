// What Settings, General shows of the search model: which one, how far it has indexed the
// attached files, or the download that adds it.

import { formatBytes } from '@demido/ui';

import type { DownloadJob, SearchStatus } from '@/lib/types';

export type SearchState = 'ready' | 'indexing' | 'downloading' | 'missing' | 'error';

export interface SearchView {
  state: SearchState;
  title: string;
  meta: string;
  /** 0 to 1 while indexing or downloading. */
  progress: number | null;
  /** The download button, when there is no model and none is downloading. */
  offerDownload: boolean;
}

const ABOUT = 'Finds passages by what they mean, in any language';

/** The download of the suggested model, when one is queued, running or failed. */
export function searchDownload(status: SearchStatus, downloads: DownloadJob[]): DownloadJob | undefined {
  return downloads.find((j) => j.repo === status.suggested.repo && j.state !== 'done');
}

export function searchView(status: SearchStatus, download?: DownloadJob): SearchView {
  const { model, suggested, indexed, total, error } = status;
  if (!model) {
    if (download && download.state !== 'failed') {
      return {
        state: 'downloading',
        title: suggested.name,
        meta: `Downloading: ${formatBytes(download.downloaded)} of ${formatBytes(download.total || suggested.size)}`,
        progress: download.total ? download.downloaded / download.total : null,
        offerDownload: false,
      };
    }
    return {
      state: 'missing',
      title: 'Search by meaning is not installed',
      meta: download?.error
        ? `The download failed: ${download.error}`
        : `Files are searched by their words only. ${suggested.name} (${formatBytes(suggested.size)}) adds meaning.`,
      progress: null,
      offerDownload: true,
    };
  }
  if (error) {
    return { state: 'error', title: model.name, meta: error, progress: null, offerDownload: false };
  }
  if (indexed < total) {
    return {
      state: 'indexing',
      title: model.name,
      meta: `Indexing: ${indexed.toLocaleString()} of ${total.toLocaleString()} passages`,
      progress: total ? indexed / total : null,
      offerDownload: false,
    };
  }
  return {
    state: 'ready',
    title: model.name,
    meta: total ? `${ABOUT} · ${total.toLocaleString()} passages indexed` : ABOUT,
    progress: null,
    offerDownload: false,
  };
}
