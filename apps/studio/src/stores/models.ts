import { create } from 'zustand';

import { api } from '@/lib/api';
import type { DownloadJob, ModelEntry, RuntimeStatus } from '@/lib/types';

interface ModelsStore {
  models: ModelEntry[];
  runtime: RuntimeStatus | null;
  downloads: DownloadJob[];
  loaded: boolean;
  load: () => Promise<void>;
  setModels: (models: ModelEntry[]) => void;
  setRuntime: (status: RuntimeStatus) => void;
  upsertDownload: (job: DownloadJob) => void;
  setDownloads: (jobs: DownloadJob[]) => void;
}

export const useModels = create<ModelsStore>((set, get) => ({
  models: [],
  runtime: null,
  downloads: [],
  loaded: false,

  load: async () => {
    const [models, runtime, downloads] = await Promise.all([
      api.listModels(),
      api.runtimeStatus(),
      api.listDownloads(),
    ]);
    set({ models, runtime, downloads, loaded: true });
  },

  setModels: (models) => set({ models }),
  setRuntime: (runtime) => set({ runtime }),

  upsertDownload: (job) => {
    const list = get().downloads;
    const i = list.findIndex((j) => j.id === job.id);
    if (job.state === 'failed' && job.error === 'cancelled') {
      set({ downloads: list.filter((j) => j.id !== job.id) });
      return;
    }
    set({ downloads: i >= 0 ? list.map((j) => (j.id === job.id ? job : j)) : [...list, job] });
  },

  setDownloads: (downloads) => set({ downloads }),
}));

export function enabledModels(models: ModelEntry[]): ModelEntry[] {
  return models.filter((m) => m.enabled);
}

export function findModel(models: ModelEntry[], id: string | null | undefined): ModelEntry | undefined {
  return id ? models.find((m) => m.id === id) : undefined;
}
