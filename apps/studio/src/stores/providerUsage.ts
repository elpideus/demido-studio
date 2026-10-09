import { create } from 'zustand';

import { api } from '@/lib/api';
import type { ProviderUsage } from '@/lib/types';

/** Views opened one after another share one answer from this recent. */
const FRESH_MS = 30_000;

interface ProviderUsageStore {
  /** By provider id. */
  usage: Record<string, ProviderUsage>;
  checkedAt: number;
  /** Asks the providers again, unless they were asked moments ago (`force` asks anyway). */
  refresh: (force?: boolean) => Promise<void>;
}

let asking: Promise<void> | null = null;

export const useProviderUsage = create<ProviderUsageStore>((set, get) => ({
  usage: {},
  checkedAt: 0,

  refresh: (force = false) => {
    if (asking) return asking;
    if (!force && Date.now() - get().checkedAt < FRESH_MS) return Promise.resolve();
    asking = api
      .providerUsage()
      .then((list) => set({ usage: Object.fromEntries(list.map((u) => [u.providerId, u])), checkedAt: Date.now() }))
      .catch(() => undefined)
      .finally(() => {
        asking = null;
      });
    return asking;
  },
}));
