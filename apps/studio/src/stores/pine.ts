// Demido's Pine library: the scripts written here (by the user or the assistant), kept by the
// market sidecar and mirrored from its `pine.changed` events.

import { create } from 'zustand';

import { api, errorText } from '@/lib/api';
import type { PineSummary } from '@/lib/types';

interface PineStore {
  /** Last changed first. */
  scripts: PineSummary[];
  loaded: boolean;
  error?: string;
  load: () => Promise<void>;
  /** A script saved (or deleted: null) here or by the assistant. */
  changed: (id: string, script: PineSummary | null) => void;
}

const newestFirst = (list: PineSummary[]) => [...list].sort((a, b) => b.modified - a.modified);

export const usePine = create<PineStore>((set) => ({
  scripts: [],
  loaded: false,
  load: async () => {
    try {
      set({ scripts: newestFirst(await api.marketPineList()), loaded: true, error: undefined });
    } catch (e) {
      set({ loaded: true, error: errorText(e) });
    }
  },
  changed: (id, script) =>
    set(({ scripts }) => {
      const rest = scripts.filter((s) => s.id !== id);
      return { scripts: script ? newestFirst([...rest, script]) : rest };
    }),
}));
