// The built-in browser's tabs and the assistant's request for the person, mirrored from
// `browser://tabs` and `browser://attention`. The pages themselves are native views the backend
// lays over the Browser window (see browser/BrowserWindow.tsx).

import { create } from 'zustand';

import { api, errorText } from '@/lib/api';
import type { BrowserAttention, BrowserTab } from '@/lib/types';
import { toast } from './toasts';
import { useWindows } from './windows';

interface BrowserStore {
  tabs: BrowserTab[];
  attention: BrowserAttention | null;
  load: () => Promise<void>;
  setTabs: (tabs: BrowserTab[]) => void;
  setAttention: (attention: BrowserAttention | null) => void;
  /** Opens the Browser window on a tab (a chat's, from its tool card). */
  show: (tab: number) => void;
  /** Opens a tab of the person's at an address or a search, in the Browser window. */
  openTab: (input?: string) => void;
}

export const useBrowser = create<BrowserStore>((set, get) => ({
  tabs: [],
  attention: null,
  load: async () => {
    const [tabs, attention] = await Promise.all([api.browserTabs(), api.browserAttention()]);
    set({ tabs, attention });
  },
  setTabs: (tabs) => set({ tabs }),
  setAttention: (attention) => set({ attention }),
  show: (tab) => {
    if (get().tabs.some((t) => t.id === tab)) void api.browserActivate(tab);
    useWindows.getState().open('browser');
  },
  openTab: (input) => {
    useWindows.getState().open('browser');
    api.browserNewTab(input).catch((e) => toast.error('Could not open the page', errorText(e)));
  },
}));
