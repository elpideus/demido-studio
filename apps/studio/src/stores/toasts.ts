import { create } from 'zustand';

export type ToastTone = 'info' | 'success' | 'warning' | 'error';

export interface Toast {
  id: number;
  tone: ToastTone;
  title: string;
  body?: string;
  action?: { label: string; run: () => void };
}

interface ToastStore {
  toasts: Toast[];
  push: (toast: Omit<Toast, 'id'>, ttlMs?: number) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToasts = create<ToastStore>((set, get) => ({
  toasts: [],
  push: (toast, ttlMs = 6000) => {
    const id = nextId++;
    // The same message twice in a row replaces the first instead of stacking.
    const rest = get().toasts.filter((t) => !(t.title === toast.title && t.body === toast.body));
    set({ toasts: [...rest, { ...toast, id }].slice(-4) });
    if (ttlMs > 0) window.setTimeout(() => get().dismiss(id), ttlMs);
  },
  dismiss: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}));

export const toast = {
  info: (title: string, body?: string) => useToasts.getState().push({ tone: 'info', title, body }),
  success: (title: string, body?: string) => useToasts.getState().push({ tone: 'success', title, body }),
  warning: (title: string, body?: string) => useToasts.getState().push({ tone: 'warning', title, body }, 9000),
  error: (title: string, body?: string) => useToasts.getState().push({ tone: 'error', title, body }, 10000),
};
