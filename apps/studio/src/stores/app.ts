import { create } from 'zustand';

import { api } from '@/lib/api';
import type { AppInfo, Settings } from '@/lib/types';

interface AppStore {
  info: AppInfo | null;
  settings: Settings | null;
  init: () => Promise<void>;
  acceptDisclaimer: () => Promise<void>;
  patchSettings: (patch: Partial<Settings>) => Promise<void>;
  refreshInfo: () => Promise<void>;
}

export const useApp = create<AppStore>((set, get) => ({
  info: null,
  settings: null,

  init: async () => {
    const [info, settings] = await Promise.all([api.appInfo(), api.getSettings()]);
    set({ info, settings });
  },

  refreshInfo: async () => set({ info: await api.appInfo() }),

  acceptDisclaimer: async () => {
    await api.acceptDisclaimer();
    const info = get().info;
    if (info) set({ info: { ...info, disclaimerAccepted: true } });
  },

  patchSettings: async (patch) => {
    const current = get().settings;
    if (current) set({ settings: { ...current, ...patch } });
    set({ settings: await api.updateSettings(patch) });
  },
}));
