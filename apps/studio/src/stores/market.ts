import { create } from 'zustand';

import { api } from '@/lib/api';
import type { MarketStatus } from '@/lib/types';

interface MarketStore {
  status: MarketStatus | null;
  load: () => Promise<void>;
  set: (status: MarketStatus) => void;
  login: () => Promise<void>;
  logout: () => Promise<void>;
}

export const useMarket = create<MarketStore>((set) => ({
  status: null,
  load: async () => set({ status: await api.marketStatus() }),
  set: (status) => set({ status }),
  login: async () => {
    await api.marketLogin();
  },
  logout: async () => set({ status: await api.marketLogout() }),
}));
