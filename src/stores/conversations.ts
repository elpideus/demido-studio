import { create } from 'zustand'
import { listen } from '@tauri-apps/api/event'
import { db } from '../lib/tauri'
import type { Conversation } from '../types'

interface ConversationsStore {
  conversations: Conversation[]
  activeId: string | null
  load: () => Promise<void>
  create: (providerId: string, modelId: string) => Promise<Conversation>
  remove: (id: string) => Promise<void>
  setActive: (id: string | null) => void
  updateTitle: (id: string, title: string) => Promise<void>
  setAgentMode: (id: string, mode: 'off' | 'cautious' | 'balanced' | 'autonomous') => Promise<void>
  setWorkingDirectory: (id: string, path: string | null) => Promise<void>
  listenForTitleUpdates: () => Promise<() => void>
}

export const useConversations = create<ConversationsStore>((set) => ({
  conversations: [],
  activeId: null,

  load: async () => {
    const conversations = await db.listConversations()
    set({ conversations })
  },

  create: async (providerId, modelId) => {
    const conv = await db.createConversation(providerId, modelId)
    set(s => ({ conversations: [conv, ...s.conversations], activeId: conv.id }))
    return conv
  },

  remove: async (id) => {
    await db.deleteConversation(id)
    set(s => ({
      conversations: s.conversations.filter(c => c.id !== id),
      activeId: s.activeId === id ? null : s.activeId,
    }))
  },

  setActive: (id) => set({ activeId: id }),

  updateTitle: async (id, title) => {
    await db.updateConversationTitle(id, title)
    set(s => ({
      conversations: s.conversations.map(c => c.id === id ? { ...c, title } : c),
    }))
  },

  setAgentMode: async (id, mode) => {
    await db.setAgentMode(id, mode)
    set(s => ({
      conversations: s.conversations.map(c => c.id === id ? { ...c, agent_mode: mode } : c),
    }))
  },

  setWorkingDirectory: async (id, path) => {
    await db.setWorkingDirectory(id, path)
    set(s => ({
      conversations: s.conversations.map(c => c.id === id ? { ...c, working_directory: path } : c),
    }))
  },

  listenForTitleUpdates: async () => {
    const unlisten = await listen<{ id: string; title: string }>('conversation_title_updated', (e) => {
      set(s => ({
        conversations: s.conversations.map(c =>
          c.id === e.payload.id ? { ...c, title: e.payload.title } : c
        ),
      }))
    })
    return unlisten
  },
}))
