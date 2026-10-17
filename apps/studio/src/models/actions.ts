// Using a downloaded model, from the Models window, the downloads panel or the toast that says it
// arrived.

import { api, errorText } from '@/lib/api';
import type { DownloadJob, ModelEntry } from '@/lib/types';
import { useChats } from '@/stores/chats';
import { useModels } from '@/stores/models';
import { toast, useToasts } from '@/stores/toasts';
import { shortName } from '@/downloads/queue';

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** The local model a downloaded file became, if the library has found it. */
export function localModelFor(models: ModelEntry[], fileName: string): ModelEntry | undefined {
  const name = baseName(fileName);
  return models.find((m) => m.source === 'local' && m.path !== null && baseName(m.path) === name);
}

/** Makes a model the one new chats start with and the one the chat on screen answers with. */
export async function useLocalModel(model: ModelEntry): Promise<void> {
  try {
    if (!model.enabled) await api.setModelsEnabled([model.id], true);
    const models = await api.setDefaultModel(model.id);
    useModels.getState().setModels(models);
    useChats.getState().pickModel(model.id);
    toast.success(`${model.name} answers now`, 'It is the model for this chat and for new ones.');
  } catch (e) {
    toast.error(`Could not switch to ${model.name}`, errorText(e));
  }
}

/** The toast for a finished download, with a Use button once the library has the file. */
export function announceDownloaded(job: DownloadJob): void {
  const title = `${shortName(job.repo)}${job.quant ? ` ${job.quant}` : ''} is ready`;
  useToasts.getState().push({
    tone: 'success',
    title,
    body: 'Verified and added to your models.',
    action: {
      label: 'Use',
      run: () => {
        const model = localModelFor(useModels.getState().models, job.name);
        if (model) void useLocalModel(model);
        else toast.info('Still adding it', 'The model appears in a moment. Try again then.');
      },
    },
  });
}
