import { create } from 'zustand';

import { api } from '@/lib/api';
import type { Skill, ToolGroup } from '@/lib/types';

interface SkillsStore {
  skills: Skill[];
  groups: ToolGroup[];
  load: () => Promise<void>;
  setSkills: (skills: Skill[]) => void;
  toggleSkill: (id: string, enabled: boolean) => Promise<void>;
  toggleGroup: (id: string, enabled: boolean) => Promise<void>;
  refreshGroups: () => Promise<void>;
}

export const useSkills = create<SkillsStore>((set, get) => ({
  skills: [],
  groups: [],

  load: async () => {
    const [skills, groups] = await Promise.all([api.listSkills(), api.listToolGroups()]);
    set({ skills, groups });
  },

  setSkills: (skills) => set({ skills }),

  toggleSkill: async (id, enabled) => {
    set({ skills: get().skills.map((s) => (s.id === id ? { ...s, enabled } : s)) });
    set({ skills: await api.setSkillEnabled(id, enabled) });
  },

  toggleGroup: async (id, enabled) => {
    set({ groups: get().groups.map((g) => (g.id === id ? { ...g, enabled } : g)) });
    set({ groups: await api.setToolGroup(id, enabled) });
  },

  refreshGroups: async () => set({ groups: await api.listToolGroups() }),
}));
