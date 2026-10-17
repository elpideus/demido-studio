import { create } from 'zustand';

/** Whether the downloads panel beside the navigation rail is open: anything may open it. */
export const useDownloadsPanel = create<{ open: boolean; setOpen: (open: boolean) => void }>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}));
