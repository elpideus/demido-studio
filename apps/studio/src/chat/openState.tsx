// Which tool cards and thinking blocks of a turn are open. The turn keeps this by id rather than
// each component on its own, so something the person opened stays open when its step moves into
// a file bundle while the model keeps working.

import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';

interface OpenState {
  open: ReadonlyMap<string, boolean>;
  set: (id: string, open: boolean) => void;
}

const OpenContext = createContext<OpenState | null>(null);

// A call is named by its message too: some providers reuse call ids from one step to the next.
export const callKey = (messageId: string, callId: string) => `call:${messageId}:${callId}`;
export const thinkingKey = (messageId: string) => `thinking:${messageId}`;
export const bundleKey = (bundleId: string) => `files:${bundleId}`;

/** Holds the open state for everything inside one assistant turn. */
export function OpenStateProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState<ReadonlyMap<string, boolean>>(() => new Map());
  const set = useCallback((id: string, value: boolean) => setOpen((prev) => new Map(prev).set(id, value)), []);
  const value = useMemo(() => ({ open, set }), [open, set]);
  return <OpenContext.Provider value={value}>{children}</OpenContext.Provider>;
}

/** Whether something was opened by the person, if it ever was. */
export function useOpenMap(): ReadonlyMap<string, boolean> {
  return useContext(OpenContext)?.open ?? EMPTY;
}

const EMPTY: ReadonlyMap<string, boolean> = new Map();

/**
 * Open state of one card or block: kept by the turn when inside one, else by the component.
 * `fallback` is used until the person opens or closes it.
 */
export function useOpenState(id: string, fallback = false): [boolean, (open: boolean) => void] {
  const context = useContext(OpenContext);
  const [local, setLocal] = useState<boolean | undefined>(undefined);
  const setShared = context?.set;
  const setOpen = useCallback(
    (value: boolean) => (setShared ? setShared(id, value) : setLocal(value)),
    [setShared, id],
  );
  return [(context ? context.open.get(id) : local) ?? fallback, setOpen];
}
