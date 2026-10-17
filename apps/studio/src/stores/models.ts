import { create } from 'zustand';

import { api, errorText } from '@/lib/api';
import type { DownloadJob, ModelEntry, RuntimeStatus } from '@/lib/types';
import { useChats } from './chats';
import { toast } from './toasts';

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

/** Frees the memory the local model holds; the next message to it loads it again. */
export function unloadModel(): void {
  api.unloadModel().catch((e) => toast.error('Could not unload the model', errorText(e)));
}

/** Whether a chat is being answered by the loaded local model, which unloading would cut off. */
export function useLoadedModelAnswering(): boolean {
  const loadedId = useModels((s) => (s.runtime?.state === 'ready' ? s.runtime.modelId : null));
  return useChats((s) => loadedId !== null && s.chats.some((c) => s.running[c.id] && c.modelId === loadedId));
}
