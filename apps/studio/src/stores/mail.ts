import { create } from 'zustand';

import { api } from '@/lib/api';
import type { MailAccount, NewMailAccount } from '@/lib/types';

interface MailStore {
  accounts: MailAccount[];
  load: () => Promise<void>;
  set: (accounts: MailAccount[]) => void;
  /** Signs in to check the account, then saves it; the same address again updates its password. */
  add: (account: NewMailAccount) => Promise<MailAccount>;
  remove: (id: string) => Promise<void>;
}

export const useMail = create<MailStore>((set) => ({
  accounts: [],
  load: async () => set({ accounts: await api.mailAccounts() }),
  set: (accounts) => set({ accounts }),
  add: async (account) => {
    const added = await api.mailAddAccount(account);
    set({ accounts: await api.mailAccounts() });
    return added;
  },
  remove: async (id) => {
    await api.mailRemoveAccount(id);
    set({ accounts: await api.mailAccounts() });
  },
}));
